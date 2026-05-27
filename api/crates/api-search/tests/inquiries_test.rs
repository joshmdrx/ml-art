mod common;

use common::{app_with_test_auth, get_json_authed, send_authed, MIGRATOR};
use serde::Deserialize;
use serde_json::{json, Value};
use sqlx::PgPool;

const ALICE: &str = "test-user_test_alice";
const ARTWORK_BLUE_MORNING: &str = "bbb11111-1111-1111-1111-111111111111";
const DRAFT_ARTWORK: &str = "bbb66666-6666-6666-6666-666666666666";

#[derive(Deserialize, Debug)]
struct InquiryAck {
    id: String,
    status: String,
    #[serde(default)]
    debug_verification_token: Option<String>,
}

// ─────────────────────────────────────────────────────────────────────────────
// Anonymous submission
// ─────────────────────────────────────────────────────────────────────────────

#[sqlx::test(migrator = "MIGRATOR", fixtures("seed"))]
async fn anonymous_inquiry_returns_pending_verification(pool: PgPool) {
    let app = app_with_test_auth(pool.clone());
    let body = json!({
        "name":    "Stranger",
        "email":   "stranger@example.com",
        "message": "Is this still available?"
    })
    .to_string();
    let (status, bytes) = send_authed(
        app,
        "POST",
        &format!("/v1/artworks/{ARTWORK_BLUE_MORNING}/inquiries"),
        // No / unverifiable bearer → handler treats sender as anonymous.
        "anonymous-no-such-token",
        Some(&body),
    )
    .await;
    assert_eq!(status, 200);

    let ack: InquiryAck = serde_json::from_slice(&bytes).unwrap();
    assert_eq!(ack.status, "pending_verification");
    let token = ack
        .debug_verification_token
        .expect("dev mode returns token");

    // DB row should be pending: verified_at IS NULL
    let row: (
        Option<chrono::DateTime<chrono::Utc>>,
        Option<chrono::DateTime<chrono::Utc>>,
    ) = sqlx::query_as(
        "SELECT verified_at, delivered_at FROM inquiries WHERE verification_token = $1",
    )
    .bind(&token)
    .fetch_one(&pool)
    .await
    .unwrap();
    assert!(row.0.is_none());
    assert!(row.1.is_none());
}

#[sqlx::test(migrator = "MIGRATOR", fixtures("seed"))]
async fn anonymous_inquiry_then_verify_marks_delivered(pool: PgPool) {
    let app = app_with_test_auth(pool.clone());
    let body = json!({
        "name":    "Stranger",
        "email":   "stranger@example.com",
        "message": "Is this still available?"
    })
    .to_string();
    let (_, bytes) = send_authed(
        app.clone(),
        "POST",
        &format!("/v1/artworks/{ARTWORK_BLUE_MORNING}/inquiries"),
        "anonymous",
        Some(&body),
    )
    .await;
    let ack: InquiryAck = serde_json::from_slice(&bytes).unwrap();
    let token = ack.debug_verification_token.unwrap();

    // Hit verify
    let (status, vbody): (_, Value) =
        get_json_authed(app, &format!("/v1/inquiries/verify/{token}"), "ignored").await;
    assert_eq!(status, 200);
    assert_eq!(vbody["status"], "delivered");

    // DB now reflects both timestamps
    let row: (
        Option<chrono::DateTime<chrono::Utc>>,
        Option<chrono::DateTime<chrono::Utc>>,
    ) = sqlx::query_as(
        "SELECT verified_at, delivered_at FROM inquiries WHERE verification_token = $1",
    )
    .bind(&token)
    .fetch_one(&pool)
    .await
    .unwrap();
    assert!(row.0.is_some());
    assert!(row.1.is_some());
}

#[sqlx::test(migrator = "MIGRATOR", fixtures("seed"))]
async fn verify_with_unknown_token_is_404(pool: PgPool) {
    let app = app_with_test_auth(pool);
    let (status, _) =
        common::get_status(app, "/v1/inquiries/verify/never-issued-this-token-yo").await;
    assert_eq!(status, 404);
}

// ─────────────────────────────────────────────────────────────────────────────
// Signed-in submission
// ─────────────────────────────────────────────────────────────────────────────

#[sqlx::test(migrator = "MIGRATOR", fixtures("seed"))]
async fn signed_in_inquiry_is_delivered_immediately(pool: PgPool) {
    let app = app_with_test_auth(pool.clone());
    let body = json!({
        "name":    "Alice (overridden)",
        // Body email is IGNORED for signed-in users — we use Clerk-verified.
        "email":   "spoofed@example.com",
        "message": "Love this piece!"
    })
    .to_string();
    let (status, bytes) = send_authed(
        app,
        "POST",
        &format!("/v1/artworks/{ARTWORK_BLUE_MORNING}/inquiries"),
        ALICE,
        Some(&body),
    )
    .await;
    assert_eq!(status, 200);
    let ack: InquiryAck = serde_json::from_slice(&bytes).unwrap();
    assert_eq!(ack.status, "delivered");
    assert!(ack.debug_verification_token.is_none());

    // DB row uses Alice's verified email (from the seed fixture), not the spoofed one
    let inquiry_id = uuid::Uuid::parse_str(&ack.id).unwrap();
    let (email, verified, delivered): (
        String,
        Option<chrono::DateTime<chrono::Utc>>,
        Option<chrono::DateTime<chrono::Utc>>,
    ) = sqlx::query_as("SELECT from_email, verified_at, delivered_at FROM inquiries WHERE id = $1")
        .bind(inquiry_id)
        .fetch_one(&pool)
        .await
        .unwrap();
    assert_eq!(email, "alice@example.com");
    assert!(verified.is_some());
    assert!(delivered.is_some());
}

// ─────────────────────────────────────────────────────────────────────────────
// Validation
// ─────────────────────────────────────────────────────────────────────────────

#[sqlx::test(migrator = "MIGRATOR", fixtures("seed"))]
async fn inquiry_on_missing_artwork_is_404(pool: PgPool) {
    let app = app_with_test_auth(pool);
    let body = json!({"name": "n", "email": "x@y.com", "message": "hi"}).to_string();
    let (status, _) = send_authed(
        app,
        "POST",
        "/v1/artworks/00000000-0000-0000-0000-000000000000/inquiries",
        "anonymous",
        Some(&body),
    )
    .await;
    assert_eq!(status, 404);
}

#[sqlx::test(migrator = "MIGRATOR", fixtures("seed"))]
async fn inquiry_on_draft_artwork_is_404(pool: PgPool) {
    let app = app_with_test_auth(pool);
    let body = json!({"name": "n", "email": "x@y.com", "message": "hi"}).to_string();
    let (status, _) = send_authed(
        app,
        "POST",
        &format!("/v1/artworks/{DRAFT_ARTWORK}/inquiries"),
        "anonymous",
        Some(&body),
    )
    .await;
    assert_eq!(status, 404);
}

#[sqlx::test(migrator = "MIGRATOR", fixtures("seed"))]
async fn anonymous_inquiry_requires_email(pool: PgPool) {
    let app = app_with_test_auth(pool);
    let body = json!({"name": "n", "message": "hi"}).to_string();
    let (status, _) = send_authed(
        app,
        "POST",
        &format!("/v1/artworks/{ARTWORK_BLUE_MORNING}/inquiries"),
        "anonymous",
        Some(&body),
    )
    .await;
    assert_eq!(status, 400);
}

#[sqlx::test(migrator = "MIGRATOR", fixtures("seed"))]
async fn inquiry_rejects_bad_email(pool: PgPool) {
    let app = app_with_test_auth(pool);
    let body = json!({"name": "n", "email": "not-an-email", "message": "hi"}).to_string();
    let (status, _) = send_authed(
        app,
        "POST",
        &format!("/v1/artworks/{ARTWORK_BLUE_MORNING}/inquiries"),
        "anonymous",
        Some(&body),
    )
    .await;
    assert_eq!(status, 400);
}

#[sqlx::test(migrator = "MIGRATOR", fixtures("seed"))]
async fn inquiry_rejects_empty_message(pool: PgPool) {
    let app = app_with_test_auth(pool);
    let body = json!({"name": "n", "email": "x@y.com", "message": "   "}).to_string();
    let (status, _) = send_authed(
        app,
        "POST",
        &format!("/v1/artworks/{ARTWORK_BLUE_MORNING}/inquiries"),
        "anonymous",
        Some(&body),
    )
    .await;
    assert_eq!(status, 400);
}
