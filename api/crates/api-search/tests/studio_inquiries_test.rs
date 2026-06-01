// T-011 Phase 4 — studio inquiry inbox.
//
// Asserts: ownership boundary, status filter modes, ordering, the
// derived `status` string, and that anon-pending inquiries appear
// alongside delivered ones.

#![allow(dead_code)]

mod common;

use common::{app_with_test_auth, get_json_authed, get_status_authed, MIGRATOR};
use serde::Deserialize;
use sqlx::PgPool;

const ALICE: &str = "test-user_test_alice";
const BOB: &str = "test-user_test_bob";
const ARTIST_ALICE: &str = "aaa11111-1111-1111-1111-111111111111";
const ARTIST_BRUNO: &str = "aaa22222-2222-2222-2222-222222222222";
const ARTWORK_BLUE_MORNING: &str = "bbb11111-1111-1111-1111-111111111111";
const ARTWORK_CRIMSON_FIELD: &str = "bbb22222-2222-2222-2222-222222222222";
const ARTWORK_STONE_FORM: &str = "bbb33333-3333-3333-3333-333333333333"; // Bruno

#[derive(Deserialize, Debug)]
struct Page<T> {
    items: Vec<T>,
}

#[derive(Deserialize, Debug)]
struct Inquiry {
    id: String,
    artwork_id: String,
    artwork_title: Option<String>,
    artwork_primary_image_url: Option<String>,
    from_name: String,
    from_email: String,
    message: String,
    budget_range: Option<String>,
    status: String,
    delivered_at: Option<String>,
}

// ─────────────────────────────────────────────────────────────────────────────
// Helpers
// ─────────────────────────────────────────────────────────────────────────────

async fn insert_delivered(
    pool: &PgPool,
    artwork_id: &str,
    artist_id: &str,
    from_name: &str,
    from_email: &str,
    message: &str,
    minutes_ago: i64,
) {
    sqlx::query(
        r#"
        INSERT INTO inquiries (
            artwork_id, artist_id, from_email, from_name, message,
            delivery_channel, verified_at, delivered_at, created_at
        )
        VALUES (
            $1::uuid, $2::uuid, $3, $4, $5,
            'platform', now() - ($6 * interval '1 minute'),
                       now() - ($6 * interval '1 minute'),
                       now() - ($6 * interval '1 minute')
        )
        "#,
    )
    .bind(artwork_id)
    .bind(artist_id)
    .bind(from_email)
    .bind(from_name)
    .bind(message)
    .bind(minutes_ago)
    .execute(pool)
    .await
    .unwrap();
}

#[allow(clippy::too_many_arguments)]
async fn insert_pending(
    pool: &PgPool,
    artwork_id: &str,
    artist_id: &str,
    from_name: &str,
    from_email: &str,
    message: &str,
    token: &str,
    minutes_ago: i64,
) {
    sqlx::query(
        r#"
        INSERT INTO inquiries (
            artwork_id, artist_id, from_email, from_name, message,
            delivery_channel, verification_token, created_at
        )
        VALUES (
            $1::uuid, $2::uuid, $3, $4, $5,
            'platform', $6, now() - ($7 * interval '1 minute')
        )
        "#,
    )
    .bind(artwork_id)
    .bind(artist_id)
    .bind(from_email)
    .bind(from_name)
    .bind(message)
    .bind(token)
    .bind(minutes_ago)
    .execute(pool)
    .await
    .unwrap();
}

// ─────────────────────────────────────────────────────────────────────────────
// Tests
// ─────────────────────────────────────────────────────────────────────────────

#[sqlx::test(migrator = "MIGRATOR", fixtures("seed"))]
async fn empty_inbox_returns_empty_items(pool: PgPool) {
    let app = app_with_test_auth(pool);
    let (status, page): (_, Page<Inquiry>) =
        get_json_authed(app, "/v1/studio/inquiries", ALICE).await;
    assert_eq!(status, 200);
    assert!(page.items.is_empty());
}

#[sqlx::test(migrator = "MIGRATOR", fixtures("seed"))]
async fn inbox_lists_alices_inquiries_newest_first(pool: PgPool) {
    // Two delivered + one pending on Alice's artworks.
    insert_delivered(
        &pool,
        ARTWORK_BLUE_MORNING,
        ARTIST_ALICE,
        "Jane Buyer",
        "jane@example.com",
        "Love this — is it still available?",
        30,
    )
    .await;
    insert_delivered(
        &pool,
        ARTWORK_CRIMSON_FIELD,
        ARTIST_ALICE,
        "Sam Collector",
        "sam@example.com",
        "Could you tell me about the framing?",
        5,
    )
    .await;
    insert_pending(
        &pool,
        ARTWORK_BLUE_MORNING,
        ARTIST_ALICE,
        "Anon Curious",
        "anon@example.com",
        "Hi! I would love to know more.",
        "tok-anon-1",
        15,
    )
    .await;

    let app = app_with_test_auth(pool);
    let (status, page): (_, Page<Inquiry>) =
        get_json_authed(app, "/v1/studio/inquiries", ALICE).await;
    assert_eq!(status, 200);
    assert_eq!(page.items.len(), 3, "all three of Alice's inquiries listed");

    // Newest first: Sam (5m), then Anon (15m), then Jane (30m).
    assert_eq!(page.items[0].from_name, "Sam Collector");
    assert_eq!(page.items[0].status, "delivered");
    assert!(page.items[0].artwork_primary_image_url.is_some());
    assert_eq!(page.items[0].artwork_title.as_deref(), Some("Crimson Field"));

    assert_eq!(page.items[1].from_name, "Anon Curious");
    assert_eq!(page.items[1].status, "pending_verification");
    assert!(
        page.items[1].delivered_at.is_none(),
        "pending row has no delivered_at"
    );

    assert_eq!(page.items[2].from_name, "Jane Buyer");
    assert_eq!(page.items[2].status, "delivered");
}

#[sqlx::test(migrator = "MIGRATOR", fixtures("seed"))]
async fn status_filter_pending_returns_only_pending(pool: PgPool) {
    insert_delivered(
        &pool,
        ARTWORK_BLUE_MORNING,
        ARTIST_ALICE,
        "Jane",
        "j@example.com",
        "Hi",
        10,
    )
    .await;
    insert_pending(
        &pool,
        ARTWORK_BLUE_MORNING,
        ARTIST_ALICE,
        "Anon",
        "a@example.com",
        "Hi",
        "tok-p",
        5,
    )
    .await;

    let app = app_with_test_auth(pool);
    let (status, page): (_, Page<Inquiry>) =
        get_json_authed(app, "/v1/studio/inquiries?status=pending", ALICE).await;
    assert_eq!(status, 200);
    assert_eq!(page.items.len(), 1);
    assert_eq!(page.items[0].from_name, "Anon");
}

#[sqlx::test(migrator = "MIGRATOR", fixtures("seed"))]
async fn status_filter_delivered_returns_only_delivered(pool: PgPool) {
    insert_delivered(
        &pool,
        ARTWORK_BLUE_MORNING,
        ARTIST_ALICE,
        "Jane",
        "j@example.com",
        "Hi",
        10,
    )
    .await;
    insert_pending(
        &pool,
        ARTWORK_BLUE_MORNING,
        ARTIST_ALICE,
        "Anon",
        "a@example.com",
        "Hi",
        "tok-p2",
        5,
    )
    .await;

    let app = app_with_test_auth(pool);
    let (status, page): (_, Page<Inquiry>) =
        get_json_authed(app, "/v1/studio/inquiries?status=delivered", ALICE).await;
    assert_eq!(status, 200);
    assert_eq!(page.items.len(), 1);
    assert_eq!(page.items[0].from_name, "Jane");
}

#[sqlx::test(migrator = "MIGRATOR", fixtures("seed"))]
async fn cross_artist_inquiry_is_not_visible(pool: PgPool) {
    // Inquiry sent to Bruno; Alice must NOT see it in her inbox.
    insert_delivered(
        &pool,
        ARTWORK_STONE_FORM,
        ARTIST_BRUNO,
        "Other Buyer",
        "o@example.com",
        "For Bruno only",
        10,
    )
    .await;

    let app = app_with_test_auth(pool);
    let (status, page): (_, Page<Inquiry>) =
        get_json_authed(app, "/v1/studio/inquiries", ALICE).await;
    assert_eq!(status, 200);
    assert!(
        page.items.is_empty(),
        "Bruno's inquiries don't bleed into Alice's inbox"
    );
}

#[sqlx::test(migrator = "MIGRATOR", fixtures("seed"))]
async fn non_artist_user_gets_404(pool: PgPool) {
    // Bob is a signed-in user but has no `artists` row.
    let app = app_with_test_auth(pool);
    let (status, _) = get_status_authed(app, "/v1/studio/inquiries", BOB).await;
    assert_eq!(status, 404);
}

#[sqlx::test(migrator = "MIGRATOR", fixtures("seed"))]
async fn anonymous_request_returns_401(pool: PgPool) {
    // No Bearer header. Studio surfaces gate on the User extractor;
    // /v1/studio/inquiries must 401 (not 404 — the route exists, it's
    // the extractor that rejects).
    use axum::{body::Body, http::Request};
    use tower::ServiceExt;
    let app = app_with_test_auth(pool);
    let resp = app
        .oneshot(
            Request::builder()
                .uri("/v1/studio/inquiries")
                .body(Body::empty())
                .unwrap(),
        )
        .await
        .unwrap();
    assert_eq!(resp.status(), 401);
}

#[sqlx::test(migrator = "MIGRATOR", fixtures("seed"))]
async fn unknown_status_param_collapses_to_all(pool: PgPool) {
    insert_delivered(
        &pool,
        ARTWORK_BLUE_MORNING,
        ARTIST_ALICE,
        "Jane",
        "j@example.com",
        "Hi",
        10,
    )
    .await;
    insert_pending(
        &pool,
        ARTWORK_BLUE_MORNING,
        ARTIST_ALICE,
        "Anon",
        "a@example.com",
        "Hi",
        "tok-u",
        5,
    )
    .await;

    let app = app_with_test_auth(pool);
    // Garbage value — should be treated as "all" rather than 400.
    let (status, page): (_, Page<Inquiry>) =
        get_json_authed(app, "/v1/studio/inquiries?status=garbage", ALICE).await;
    assert_eq!(status, 200);
    assert_eq!(page.items.len(), 2);
}

#[sqlx::test(migrator = "MIGRATOR", fixtures("seed"))]
async fn budget_range_string_round_trips(pool: PgPool) {
    sqlx::query(
        r#"
        INSERT INTO inquiries (
            artwork_id, artist_id, from_email, from_name, message,
            budget_range, delivery_channel, verified_at, delivered_at
        )
        VALUES (
            $1::uuid, $2::uuid, 'j@example.com', 'Jane', 'Hi',
            '"£500-1k"'::jsonb, 'platform', now(), now()
        )
        "#,
    )
    .bind(ARTWORK_BLUE_MORNING)
    .bind(ARTIST_ALICE)
    .execute(&pool)
    .await
    .unwrap();

    let app = app_with_test_auth(pool);
    let (_, page): (_, Page<Inquiry>) =
        get_json_authed(app, "/v1/studio/inquiries", ALICE).await;
    assert_eq!(page.items.len(), 1);
    assert_eq!(page.items[0].budget_range.as_deref(), Some("£500-1k"));
}
