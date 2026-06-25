//! T-050 — behavioural events writer.
//!
//! Two tiers of test:
//!
//!   1. **Emission** — hit a handler, assert a `kind = 'event_log'`
//!      row landed in the `jobs` table with the expected `name`.
//!      Doesn't drain the queue.
//!
//!   2. **Persistence** — call `core::jobs::handle` directly on a
//!      synthesized `JobEvent::EventLog` and assert the `events`
//!      table got the row with the correct columns.
//!
//! The Postgres-backed jobs backend (`app_with_postgres_jobs`) makes
//! both tiers cheap — the same pool sees both the queue write and
//! the events INSERT.

#![allow(dead_code)]

mod common;

use common::{app_with_postgres_jobs, MIGRATOR};
use ml_art_core::{
    emails::EmailClient,
    events::EventName,
    geocoding::GeocodingClient,
    jobs::{self, JobEvent, JobsBackend, JobsDeps},
    moderation::ModerationClient,
};
use serde_json::json;
use sqlx::PgPool;
use uuid::Uuid;

const ARTWORK_BLUE_MORNING: &str = "bbb11111-1111-1111-1111-111111111111";

/// Count `event_log` rows in `jobs` whose payload has the given
/// `name` field. The payload is jsonb-stringified; `->>` extracts the
/// text. Multiple event types means a separate query per type — fine,
/// keeps each assertion specific.
async fn count_event_log_rows(pool: &PgPool, name: &str) -> i64 {
    sqlx::query_scalar::<_, i64>(
        "SELECT count(*)::bigint FROM jobs
         WHERE kind = 'event_log' AND payload->>'name' = $1",
    )
    .bind(name)
    .fetch_one(pool)
    .await
    .unwrap()
}

#[sqlx::test(migrator = "MIGRATOR", fixtures("seed"))]
async fn search_emits_search_executed(pool: PgPool) {
    let app = app_with_postgres_jobs(pool.clone());
    let (status, _) = common::send_json(app, "GET", "/v1/search?q=morning", None).await;
    assert_eq!(status, 200);

    assert_eq!(
        count_event_log_rows(&pool, "search_executed").await,
        1,
        "one search_executed event"
    );
}

#[sqlx::test(migrator = "MIGRATOR", fixtures("seed"))]
async fn search_paginated_only_emits_on_first_page(pool: PgPool) {
    // Page 1 → emits. Page 2 (offset>0) → does NOT emit; we treat the
    // search as a single intent, not one-per-scroll.
    let app = app_with_postgres_jobs(pool.clone());
    let (_, _) = common::send_json(app.clone(), "GET", "/v1/search?q=morning", None).await;
    // Compose a cursor for offset=24 (the default limit).
    let cursor = ml_art_core::cursor::PageCursor::from_offset(24).encode();
    let url = format!("/v1/search?q=morning&cursor={cursor}");
    let (status, _) = common::send_json(app, "GET", &url, None).await;
    assert_eq!(status, 200);

    assert_eq!(
        count_event_log_rows(&pool, "search_executed").await,
        1,
        "still one — page 2 must not re-emit"
    );
}

#[sqlx::test(migrator = "MIGRATOR", fixtures("seed"))]
async fn artwork_detail_emits_artwork_viewed(pool: PgPool) {
    let app = app_with_postgres_jobs(pool.clone());
    let url = format!("/v1/artworks/{ARTWORK_BLUE_MORNING}");
    let (status, _) = common::send_json(app, "GET", &url, None).await;
    assert_eq!(status, 200);

    assert_eq!(count_event_log_rows(&pool, "artwork_viewed").await, 1);
}

#[sqlx::test(migrator = "MIGRATOR", fixtures("seed"))]
async fn artwork_detail_404_does_not_emit(pool: PgPool) {
    // 404 short-circuits before the emit. The events row must not
    // exist — we'd be polluting analytics with fake views.
    let app = app_with_postgres_jobs(pool.clone());
    let bogus = "00000000-0000-0000-0000-000000000000";
    let url = format!("/v1/artworks/{bogus}");
    let (status, _) = common::send_json(app, "GET", &url, None).await;
    assert_eq!(status, 404);

    assert_eq!(count_event_log_rows(&pool, "artwork_viewed").await, 0);
}

#[sqlx::test(migrator = "MIGRATOR", fixtures("seed"))]
async fn anonymous_inquiry_emits_inquiry_submitted(pool: PgPool) {
    let app = app_with_postgres_jobs(pool.clone());
    let body = serde_json::json!({
        "name": "Anon Buyer",
        "email": "buyer@example.com",
        "message": "Tell me about this work.",
    })
    .to_string();
    let url = format!("/v1/artworks/{ARTWORK_BLUE_MORNING}/inquiries");
    let (status, _) = common::send_json(app, "POST", &url, Some(&body)).await;
    assert_eq!(status, 200);

    assert_eq!(count_event_log_rows(&pool, "inquiry_submitted").await, 1);
    let anon_flag: serde_json::Value = sqlx::query_scalar(
        "SELECT payload->'properties'->'anonymous' FROM jobs WHERE kind = 'event_log'",
    )
    .fetch_one(&pool)
    .await
    .unwrap();
    assert_eq!(anon_flag, serde_json::json!(true));
}

#[sqlx::test(migrator = "MIGRATOR", fixtures("seed"))]
async fn handle_event_log_writes_row_to_events_table(pool: PgPool) {
    // Wire the handler directly — proves the storage destination
    // (the `events` table) actually receives a row with the columns
    // we expect.
    let deps = JobsDeps {
        pool: pool.clone(),
        geocoder: GeocodingClient::for_tests(vec![]),
        emails: EmailClient::for_tests(),
        moderation: ModerationClient::disabled(),
        web_base_url: "https://test.example.com".to_string(),
        anon_cookie_secret: "test-cookie-secret".to_string(),
        reply_email_domain: "reply.test.example.com".to_string(),
        jobs: JobsBackend::for_tests(),
    };
    let anon_id = Uuid::new_v4();
    let event = JobEvent::EventLog {
        name: EventName::ArtworkViewed,
        anonymous_id: Some(anon_id),
        user_id: None,
        properties: json!({ "artwork_id": ARTWORK_BLUE_MORNING }),
        context: json!({ "ip": "203.0.113.4", "user_agent": "Mozilla/5.0" }),
    };
    jobs::handle(event, &deps).await.unwrap();

    let (name, stored_anon, props_artwork, ctx_ip): (
        String,
        Option<Uuid>,
        serde_json::Value,
        serde_json::Value,
    ) = sqlx::query_as(
        "SELECT event_name, anonymous_id, properties->'artwork_id', context->'ip'
         FROM events ORDER BY occurred_at DESC LIMIT 1",
    )
    .fetch_one(&pool)
    .await
    .unwrap();
    assert_eq!(name, "artwork_viewed");
    assert_eq!(stored_anon, Some(anon_id));
    assert_eq!(props_artwork, serde_json::json!(ARTWORK_BLUE_MORNING));
    assert_eq!(ctx_ip, serde_json::json!("203.0.113.4"));
}
