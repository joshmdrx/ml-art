// T-068 — notification preferences + unsubscribe integration tests.
//
// Per-file allow: `Deserialize`-only fields trigger dead_code under
// `-D warnings`. See `decisions.md` 2026-05-27 — Pre-commit hooks.
#![allow(dead_code)]

mod common;

use common::{app_with_test_auth, get_json_authed, send_authed, MIGRATOR};
use ml_art_core::notifications::{mint_unsubscribe_token, NotificationKind};
use serde::Deserialize;
use serde_json::json;
use sqlx::PgPool;
use uuid::Uuid;

const ALICE: &str = "test-user_test_alice";
const BOB: &str = "test-user_test_bob";
const ALICE_USER_ID: &str = "88888888-8888-8888-8888-888888888888";
// Matches `Config::for_tests`. Keeping the constant local rather than
// re-exporting it from core::config keeps the test independent of any
// future refactor of how secrets are wired in tests.
const TEST_SECRET: &[u8] = b"test-cookie-secret";

#[derive(Deserialize, Debug)]
struct Preferences {
    global_enabled: bool,
    kinds: std::collections::HashMap<String, bool>,
}

#[derive(Deserialize, Debug)]
struct UnsubscribeAck {
    kind: String,
    #[serde(default)]
    friendly_label: String,
}

// ─────────────────────────────────────────────────────────────────────────────
// Defaults + auth
// ─────────────────────────────────────────────────────────────────────────────

#[sqlx::test(migrator = "MIGRATOR", fixtures("seed"))]
async fn prefs_get_requires_auth(pool: PgPool) {
    let app = app_with_test_auth(pool);
    let (status, _) = common::get_status(app, "/v1/me/notification-preferences").await;
    assert_eq!(status, 401);
}

#[sqlx::test(migrator = "MIGRATOR", fixtures("seed"))]
async fn prefs_get_returns_defaults_for_clean_user(pool: PgPool) {
    let app = app_with_test_auth(pool);
    let (status, prefs): (_, Preferences) =
        get_json_authed(app, "/v1/me/notification-preferences", ALICE).await;
    assert_eq!(status, 200);
    assert!(prefs.global_enabled, "global default should be on");
    // Every user-facing kind should be present, default-on.
    let nwd = prefs.kinds.get("new_works_digest").copied();
    assert_eq!(nwd, Some(true), "kinds map should include new_works_digest=true by default");
    // Transactional kinds are NOT in the map (no toggle for them).
    assert!(!prefs.kinds.contains_key("inquiry_verification"));
    assert!(!prefs.kinds.contains_key("inquiry_reply"));
}

// ─────────────────────────────────────────────────────────────────────────────
// PATCH semantics
// ─────────────────────────────────────────────────────────────────────────────

#[sqlx::test(migrator = "MIGRATOR", fixtures("seed"))]
async fn prefs_patch_flips_per_kind(pool: PgPool) {
    let app = app_with_test_auth(pool);

    let patch = json!({"kinds": {"new_works_digest": false}}).to_string();
    let (status, _) = send_authed(
        app.clone(),
        "PATCH",
        "/v1/me/notification-preferences",
        ALICE,
        Some(&patch),
    )
    .await;
    assert_eq!(status, 200);

    let (_, prefs): (_, Preferences) =
        get_json_authed(app, "/v1/me/notification-preferences", ALICE).await;
    assert_eq!(prefs.kinds.get("new_works_digest").copied(), Some(false));
    // Global untouched.
    assert!(prefs.global_enabled);
}

#[sqlx::test(migrator = "MIGRATOR", fixtures("seed"))]
async fn prefs_patch_flips_global(pool: PgPool) {
    let app = app_with_test_auth(pool);

    let patch = json!({"global_enabled": false}).to_string();
    let (status, _) = send_authed(
        app.clone(),
        "PATCH",
        "/v1/me/notification-preferences",
        ALICE,
        Some(&patch),
    )
    .await;
    assert_eq!(status, 200);

    let (_, prefs): (_, Preferences) =
        get_json_authed(app, "/v1/me/notification-preferences", ALICE).await;
    assert!(!prefs.global_enabled);
    // Per-kind state isn't touched by global toggle.
    assert_eq!(prefs.kinds.get("new_works_digest").copied(), Some(true));
}

#[sqlx::test(migrator = "MIGRATOR", fixtures("seed"))]
async fn prefs_patch_partial_doesnt_clobber(pool: PgPool) {
    let app = app_with_test_auth(pool);

    // Step 1: turn off new_works_digest.
    let off = json!({"kinds": {"new_works_digest": false}}).to_string();
    send_authed(
        app.clone(),
        "PATCH",
        "/v1/me/notification-preferences",
        ALICE,
        Some(&off),
    )
    .await;

    // Step 2: PATCH only the global flag. The per-kind override should
    // survive — partial PATCH means "only touch what I sent".
    let global_off = json!({"global_enabled": false}).to_string();
    send_authed(
        app.clone(),
        "PATCH",
        "/v1/me/notification-preferences",
        ALICE,
        Some(&global_off),
    )
    .await;

    let (_, prefs): (_, Preferences) =
        get_json_authed(app, "/v1/me/notification-preferences", ALICE).await;
    assert!(!prefs.global_enabled);
    assert_eq!(prefs.kinds.get("new_works_digest").copied(), Some(false));
}

#[sqlx::test(migrator = "MIGRATOR", fixtures("seed"))]
async fn prefs_patch_unknown_kind_is_400(pool: PgPool) {
    let app = app_with_test_auth(pool);
    let patch = json!({"kinds": {"totally_made_up": false}}).to_string();
    let (status, _) = send_authed(
        app,
        "PATCH",
        "/v1/me/notification-preferences",
        ALICE,
        Some(&patch),
    )
    .await;
    assert_eq!(status, 400);
}

#[sqlx::test(migrator = "MIGRATOR", fixtures("seed"))]
async fn prefs_patch_transactional_kind_is_400(pool: PgPool) {
    let app = app_with_test_auth(pool);
    // The transactional kinds aren't in the user-facing list and have
    // no toggle. Trying to patch one should 400.
    let patch = json!({"kinds": {"inquiry_verification": false}}).to_string();
    let (status, _) = send_authed(
        app,
        "PATCH",
        "/v1/me/notification-preferences",
        ALICE,
        Some(&patch),
    )
    .await;
    assert_eq!(status, 400);
}

#[sqlx::test(migrator = "MIGRATOR", fixtures("seed"))]
async fn prefs_per_user_isolated(pool: PgPool) {
    let app = app_with_test_auth(pool);

    let patch = json!({"kinds": {"new_works_digest": false}}).to_string();
    send_authed(
        app.clone(),
        "PATCH",
        "/v1/me/notification-preferences",
        ALICE,
        Some(&patch),
    )
    .await;

    // Bob's prefs untouched.
    let (_, bob_prefs): (_, Preferences) =
        get_json_authed(app, "/v1/me/notification-preferences", BOB).await;
    assert!(bob_prefs.global_enabled);
    assert_eq!(bob_prefs.kinds.get("new_works_digest").copied(), Some(true));
}

// ─────────────────────────────────────────────────────────────────────────────
// Unsubscribe (public, token-authenticated)
// ─────────────────────────────────────────────────────────────────────────────

#[sqlx::test(migrator = "MIGRATOR", fixtures("seed"))]
async fn unsubscribe_flips_preference(pool: PgPool) {
    let app = app_with_test_auth(pool);

    let user_id = Uuid::parse_str(ALICE_USER_ID).unwrap();
    let token = mint_unsubscribe_token(user_id, NotificationKind::NewWorksDigest, TEST_SECRET)
        .unwrap();
    let body = json!({"token": token}).to_string();

    let (status, ack): (_, UnsubscribeAck) = {
        let (s, bytes) = common::send_json(
            app.clone(),
            "POST",
            "/v1/notifications/unsubscribe",
            Some(&body),
        )
        .await;
        (s, serde_json::from_slice(&bytes).unwrap())
    };
    assert_eq!(status, 200);
    assert_eq!(ack.kind, "new_works_digest");

    // Preference is now off.
    let (_, prefs): (_, Preferences) =
        get_json_authed(app, "/v1/me/notification-preferences", ALICE).await;
    assert_eq!(prefs.kinds.get("new_works_digest").copied(), Some(false));
}

#[sqlx::test(migrator = "MIGRATOR", fixtures("seed"))]
async fn unsubscribe_is_idempotent(pool: PgPool) {
    let app = app_with_test_auth(pool);
    let user_id = Uuid::parse_str(ALICE_USER_ID).unwrap();
    let token = mint_unsubscribe_token(user_id, NotificationKind::NewWorksDigest, TEST_SECRET)
        .unwrap();
    let body = json!({"token": token}).to_string();

    for _ in 0..3 {
        let (status, _) = common::send_json(
            app.clone(),
            "POST",
            "/v1/notifications/unsubscribe",
            Some(&body),
        )
        .await;
        assert_eq!(status, 200);
    }
}

#[sqlx::test(migrator = "MIGRATOR", fixtures("seed"))]
async fn unsubscribe_oneclick_returns_204(pool: PgPool) {
    let app = app_with_test_auth(pool);
    let user_id = Uuid::parse_str(ALICE_USER_ID).unwrap();
    let token = mint_unsubscribe_token(user_id, NotificationKind::NewWorksDigest, TEST_SECRET)
        .unwrap();
    let body = json!({"token": token}).to_string();

    let (status, _) = common::send_json(
        app,
        "POST",
        "/v1/notifications/unsubscribe/oneclick",
        Some(&body),
    )
    .await;
    assert_eq!(status, 204);
}

#[sqlx::test(migrator = "MIGRATOR", fixtures("seed"))]
async fn unsubscribe_rejects_bad_token(pool: PgPool) {
    let app = app_with_test_auth(pool);
    let body = json!({"token": "not.a.real.token"}).to_string();
    let (status, _) =
        common::send_json(app, "POST", "/v1/notifications/unsubscribe", Some(&body)).await;
    assert_eq!(status, 400);
}

#[sqlx::test(migrator = "MIGRATOR", fixtures("seed"))]
async fn unsubscribe_rejects_token_signed_with_different_secret(pool: PgPool) {
    let app = app_with_test_auth(pool);
    let user_id = Uuid::parse_str(ALICE_USER_ID).unwrap();
    // Token signed with the WRONG secret — the API won't accept it.
    let token =
        mint_unsubscribe_token(user_id, NotificationKind::NewWorksDigest, b"wrong-secret")
            .unwrap();
    let body = json!({"token": token}).to_string();
    let (status, _) =
        common::send_json(app, "POST", "/v1/notifications/unsubscribe", Some(&body)).await;
    assert_eq!(status, 400);
}
