//! T-012 Phase 1 — integration tests for `/v1/onboarding/*`.
//!
//! Bob is the "fresh user" in the seed (users row, no artists row).
//! Alice is the "already onboarded" user. We use both to assert the
//! mint + already-an-artist branches without inventing new fixtures.

mod common;

use common::{app_with_test_auth, send_authed, MIGRATOR};
use serde::Deserialize;
use sqlx::PgPool;

const ALICE: &str = "test-user_test_alice";
const BOB: &str = "test-user_test_bob";

#[derive(Debug, Deserialize)]
struct StudioArtist {
    id: String,
    slug: String,
    display_name: String,
    status: String,
    location: Option<String>,
}

// ─────────────────────────────────────────────────────────────────────────────
// POST /v1/onboarding/start
// ─────────────────────────────────────────────────────────────────────────────

#[sqlx::test(migrator = "MIGRATOR", fixtures("seed"))]
async fn start_creates_artist_for_new_user(pool: PgPool) {
    let app = app_with_test_auth(pool.clone());
    let body = r#"{"display_name": "Bob Painter", "location": "Lisbon, Portugal"}"#;
    let (status, bytes) = send_authed(app, "POST", "/v1/onboarding/start", BOB, Some(body)).await;
    assert_eq!(status, 201);
    let a: StudioArtist = serde_json::from_slice(&bytes).unwrap();
    assert_eq!(a.display_name, "Bob Painter");
    assert_eq!(a.slug, "bob-painter");
    assert_eq!(a.status, "pending");
    assert_eq!(a.location.as_deref(), Some("Lisbon, Portugal"));

    // user_id linked + is_artist flipped.
    let (is_artist,): (bool,) =
        sqlx::query_as("SELECT is_artist FROM users WHERE clerk_user_id = $1")
            .bind("user_test_bob")
            .fetch_one(&pool)
            .await
            .unwrap();
    assert!(is_artist, "users.is_artist must be true after onboarding");
}

#[sqlx::test(migrator = "MIGRATOR", fixtures("seed"))]
async fn start_for_existing_artist_is_400(pool: PgPool) {
    // Alice already has an artist row (`alice-test`). Calling start
    // again must error rather than mint a second.
    let app = app_with_test_auth(pool);
    let body = r#"{"display_name": "Alice Again"}"#;
    let (status, _) = send_authed(app, "POST", "/v1/onboarding/start", ALICE, Some(body)).await;
    assert_eq!(status, 400);
}

#[sqlx::test(migrator = "MIGRATOR", fixtures("seed"))]
async fn start_rejects_empty_display_name(pool: PgPool) {
    let app = app_with_test_auth(pool);
    let body = r#"{"display_name": "   "}"#;
    let (status, _) = send_authed(app, "POST", "/v1/onboarding/start", BOB, Some(body)).await;
    assert_eq!(status, 400);
}

#[sqlx::test(migrator = "MIGRATOR", fixtures("seed"))]
async fn start_rejects_overlong_display_name(pool: PgPool) {
    let app = app_with_test_auth(pool);
    let too_long = "x".repeat(101);
    let body = format!(r#"{{"display_name": "{too_long}"}}"#);
    let (status, _) = send_authed(app, "POST", "/v1/onboarding/start", BOB, Some(&body)).await;
    assert_eq!(status, 400);
}

#[sqlx::test(migrator = "MIGRATOR", fixtures("seed"))]
async fn start_generates_unique_slug_on_collision(pool: PgPool) {
    // Alice's slug is `alice-test`. If Bob registers with the same
    // display name, his slug should be `alice-test-2`.
    let app = app_with_test_auth(pool);
    let body = r#"{"display_name": "Alice Test"}"#;
    let (status, bytes) = send_authed(app, "POST", "/v1/onboarding/start", BOB, Some(body)).await;
    assert_eq!(status, 201);
    let a: StudioArtist = serde_json::from_slice(&bytes).unwrap();
    assert_eq!(a.slug, "alice-test-2");
    assert!(uuid::Uuid::parse_str(&a.id).is_ok());
}

#[sqlx::test(migrator = "MIGRATOR", fixtures("seed"))]
async fn start_without_auth_is_401(pool: PgPool) {
    let app = app_with_test_auth(pool);
    let body = r#"{"display_name": "Anon"}"#;
    let (status, _) = send_authed(app, "POST", "/v1/onboarding/start", "bogus", Some(body)).await;
    assert_eq!(status, 401);
}

// ─────────────────────────────────────────────────────────────────────────────
// POST /v1/onboarding/complete
// ─────────────────────────────────────────────────────────────────────────────

#[sqlx::test(migrator = "MIGRATOR", fixtures("seed"))]
async fn complete_flips_pending_to_active(pool: PgPool) {
    // First mint Bob's pending artist row.
    let app = app_with_test_auth(pool.clone());
    let (_, _) = send_authed(
        app.clone(),
        "POST",
        "/v1/onboarding/start",
        BOB,
        Some(r#"{"display_name": "Bob"}"#),
    )
    .await;

    // Then complete onboarding.
    let (status, bytes) = send_authed(app, "POST", "/v1/onboarding/complete", BOB, None).await;
    assert_eq!(status, 200);
    let a: StudioArtist = serde_json::from_slice(&bytes).unwrap();
    assert_eq!(a.status, "active");
}

#[sqlx::test(migrator = "MIGRATOR", fixtures("seed"))]
async fn complete_is_idempotent_when_already_active(pool: PgPool) {
    // Alice's seeded row is already `active`. Calling complete should
    // return 200 with the unchanged row (no error, no state flip).
    let app = app_with_test_auth(pool);
    let (status, bytes) = send_authed(app, "POST", "/v1/onboarding/complete", ALICE, None).await;
    assert_eq!(status, 200);
    let a: StudioArtist = serde_json::from_slice(&bytes).unwrap();
    assert_eq!(a.status, "active");
    assert_eq!(a.slug, "alice-test");
}

#[sqlx::test(migrator = "MIGRATOR", fixtures("seed"))]
async fn complete_for_non_artist_is_404(pool: PgPool) {
    // Bob has no artist row yet — complete should 404.
    let app = app_with_test_auth(pool);
    let (status, _) = send_authed(app, "POST", "/v1/onboarding/complete", BOB, None).await;
    assert_eq!(status, 404);
}

#[sqlx::test(migrator = "MIGRATOR", fixtures("seed"))]
async fn complete_without_auth_is_401(pool: PgPool) {
    let app = app_with_test_auth(pool);
    let (status, _) = send_authed(app, "POST", "/v1/onboarding/complete", "bogus", None).await;
    assert_eq!(status, 401);
}
