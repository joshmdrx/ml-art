//! T-032 — integration tests for inquiry email job enqueueing.
//!
//! Asserts the side effect of each inquiry write path: a row in the
//! `jobs` table with the right `kind`. The actual email rendering +
//! send path is covered by `core::emails::tests` (templates) +
//! `core::jobs::inquiry_handlers` (would-be wired tests, but those
//! need a Postgres pool + a full row, exercised here implicitly).
//!
//! Uses `app_with_postgres_jobs` so the jobs land in `jobs` instead
//! of an in-memory `for_tests()` capture — lets us observe via SQL
//! without threading AppState out of the test helper.

mod common;

use common::{app_with_postgres_jobs, send_authed, MIGRATOR};
use serde_json::json;
use sqlx::PgPool;

const ALICE: &str = "test-user_test_alice";
const ARTWORK_BLUE_MORNING: &str = "bbb11111-1111-1111-1111-111111111111";

#[sqlx::test(migrator = "MIGRATOR", fixtures("seed"))]
async fn signed_in_inquiry_enqueues_deliver_job(pool: PgPool) {
    let app = app_with_postgres_jobs(pool.clone());
    let body = json!({
        "name": "Test Buyer",
        "message": "Love this piece, is it still available?",
    })
    .to_string();
    let url = format!("/v1/artworks/{ARTWORK_BLUE_MORNING}/inquiries");
    let (status, _) = send_authed(app, "POST", &url, ALICE, Some(&body)).await;
    assert_eq!(status, 200);

    // Exactly one inquiry_deliver_to_artist job; no verification job
    // (signed-in path bypasses verification).
    // Filter event_log rows out — these tests predate T-050 and
    // care about the email-pipeline jobs, not the analytics emits.
    let counts: Vec<(String, i64)> = sqlx::query_as(
        "SELECT kind, count(*)::bigint FROM jobs WHERE kind <> 'event_log' GROUP BY kind ORDER BY kind",
    )
    .fetch_all(&pool)
    .await
    .unwrap();
    assert_eq!(
        counts.len(),
        1,
        "expected exactly one job kind, got {counts:?}"
    );
    assert_eq!(counts[0].0, "inquiry_deliver_to_artist");
    assert_eq!(counts[0].1, 1);
}

#[sqlx::test(migrator = "MIGRATOR", fixtures("seed"))]
async fn anonymous_inquiry_enqueues_verification_job(pool: PgPool) {
    let app = app_with_postgres_jobs(pool.clone());
    let body = json!({
        "name": "Anon Buyer",
        "email": "buyer@example.com",
        "message": "Curious about this work — could you tell me more?",
    })
    .to_string();
    let url = format!("/v1/artworks/{ARTWORK_BLUE_MORNING}/inquiries");
    let (status, bytes) = send_authed(app, "POST", &url, "bogus-token", Some(&body)).await;
    assert_eq!(status, 200);

    // Anonymous path enqueues the verification email, NOT the
    // delivery email. Delivery fires later via the verify endpoint.
    let kinds: Vec<(String,)> =
        sqlx::query_as("SELECT kind FROM jobs WHERE kind <> 'event_log' ORDER BY created_at")
            .fetch_all(&pool)
            .await
            .unwrap();
    assert_eq!(kinds.len(), 1);
    assert_eq!(kinds[0].0, "inquiry_send_verification");

    // The response surfaces the verification token in dev — make sure
    // we got one back so the next test step (verify) can use it.
    let body: serde_json::Value = serde_json::from_slice(&bytes).unwrap();
    assert!(
        body["debug_verification_token"]
            .as_str()
            .map(|s| !s.is_empty())
            .unwrap_or(false),
        "expected debug_verification_token in dev mode, got: {body}"
    );
}

#[sqlx::test(migrator = "MIGRATOR", fixtures("seed"))]
async fn anonymous_verify_enqueues_deliver_job(pool: PgPool) {
    // End-to-end: create anon inquiry → verify → both jobs land.
    let app = app_with_postgres_jobs(pool.clone());
    let body = json!({
        "name": "Anon Buyer",
        "email": "buyer@example.com",
        "message": "Hello!",
    })
    .to_string();
    let url = format!("/v1/artworks/{ARTWORK_BLUE_MORNING}/inquiries");
    let (status, bytes) = send_authed(app.clone(), "POST", &url, "bogus-token", Some(&body)).await;
    assert_eq!(status, 200);
    let create_resp: serde_json::Value = serde_json::from_slice(&bytes).unwrap();
    let token = create_resp["debug_verification_token"].as_str().unwrap();

    // Verify the inquiry.
    let verify_url = format!("/v1/inquiries/verify/{token}");
    let (vstatus, _) = send_authed(app, "GET", &verify_url, "bogus-token", None).await;
    assert_eq!(vstatus, 200);

    // After verify, both email-pipeline job kinds present. Event_log
    // rows from T-050's analytics emits are filtered out — they're
    // tangential to this test's purpose.
    let kinds: Vec<String> = sqlx::query_as::<_, (String,)>(
        "SELECT kind FROM jobs WHERE kind <> 'event_log' ORDER BY created_at",
    )
    .fetch_all(&pool)
    .await
    .unwrap()
    .into_iter()
    .map(|(k,)| k)
    .collect();
    assert_eq!(
        kinds,
        vec![
            "inquiry_send_verification".to_string(),
            "inquiry_deliver_to_artist".to_string(),
        ]
    );
}

#[sqlx::test(migrator = "MIGRATOR", fixtures("seed"))]
async fn idempotency_dedups_double_verify(pool: PgPool) {
    // Clicking the verify link twice (which can happen — email
    // clients pre-fetch links, users double-click) shouldn't fire
    // two delivery emails.
    let app = app_with_postgres_jobs(pool.clone());
    let body = json!({
        "name": "Anon Buyer",
        "email": "buyer@example.com",
        "message": "Hello!",
    })
    .to_string();
    let url = format!("/v1/artworks/{ARTWORK_BLUE_MORNING}/inquiries");
    let (_, bytes) = send_authed(app.clone(), "POST", &url, "bogus-token", Some(&body)).await;
    let create_resp: serde_json::Value = serde_json::from_slice(&bytes).unwrap();
    let token = create_resp["debug_verification_token"].as_str().unwrap();
    let verify_url = format!("/v1/inquiries/verify/{token}");

    // Two verify hits in a row.
    send_authed(app.clone(), "GET", &verify_url, "bogus-token", None).await;
    send_authed(app, "GET", &verify_url, "bogus-token", None).await;

    // Should still be exactly one delivery job.
    let count: (i64,) = sqlx::query_as(
        "SELECT count(*)::bigint FROM jobs WHERE kind = 'inquiry_deliver_to_artist'",
    )
    .fetch_one(&pool)
    .await
    .unwrap();
    assert_eq!(count.0, 1);
}
