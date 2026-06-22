// T-052 — follow graph integration tests.
//
// Per-file allow: `Deserialize`-only fields trigger dead_code under
// `-D warnings`. See `decisions.md` 2026-05-27 — Pre-commit hooks.
#![allow(dead_code)]

mod common;

use common::{app_with_test_auth, get_json_authed, send_authed, MIGRATOR};
use serde::Deserialize;
use sqlx::PgPool;

const ALICE: &str = "test-user_test_alice";
const BOB: &str = "test-user_test_bob";
const ARTIST_ALICE_PAINTER: &str = "aaa11111-1111-1111-1111-111111111111";
const ARTIST_BRUNO: &str = "aaa22222-2222-2222-2222-222222222222";

#[derive(Deserialize, Debug)]
struct FollowsList {
    items: Vec<FollowedArtist>,
}

#[derive(Deserialize, Debug)]
struct FollowedArtist {
    slug: String,
    display_name: String,
    #[serde(default)]
    primary_image_url: Option<String>,
}

#[derive(Deserialize, Debug)]
struct ArtistDetailBody {
    artist: ArtistFull,
    #[serde(default)]
    is_following: bool,
    #[serde(default)]
    follower_count: i32,
}

#[derive(Deserialize, Debug)]
struct ArtistFull {
    slug: String,
    display_name: String,
}

// ─────────────────────────────────────────────────────────────────────────────
// Auth
// ─────────────────────────────────────────────────────────────────────────────

#[sqlx::test(migrator = "MIGRATOR", fixtures("seed"))]
async fn follows_list_requires_auth(pool: PgPool) {
    let app = app_with_test_auth(pool);
    let (status, _) = common::get_status(app, "/v1/me/follows").await;
    assert_eq!(status, 401);
}

#[sqlx::test(migrator = "MIGRATOR", fixtures("seed"))]
async fn follow_create_requires_auth(pool: PgPool) {
    let app = app_with_test_auth(pool);
    let (status, _) = send_authed(
        app,
        "POST",
        &format!("/v1/me/follows/{ARTIST_ALICE_PAINTER}"),
        "garbage-not-a-test-token",
        None,
    )
    .await;
    assert_eq!(status, 401);
}

// ─────────────────────────────────────────────────────────────────────────────
// Create / delete / idempotency
// ─────────────────────────────────────────────────────────────────────────────

#[sqlx::test(migrator = "MIGRATOR", fixtures("seed"))]
async fn follow_create_204_then_listed(pool: PgPool) {
    let app = app_with_test_auth(pool);

    let (status, _) = send_authed(
        app.clone(),
        "POST",
        &format!("/v1/me/follows/{ARTIST_ALICE_PAINTER}"),
        BOB,
        None,
    )
    .await;
    assert_eq!(status, 204);

    let (status, list): (_, FollowsList) = get_json_authed(app, "/v1/me/follows", BOB).await;
    assert_eq!(status, 200);
    assert_eq!(list.items.len(), 1);
    assert_eq!(list.items[0].slug, "alice-test");
    assert!(!list.items[0].display_name.is_empty());
}

#[sqlx::test(migrator = "MIGRATOR", fixtures("seed"))]
async fn follow_create_is_idempotent(pool: PgPool) {
    let app = app_with_test_auth(pool);

    for _ in 0..3 {
        let (status, _) = send_authed(
            app.clone(),
            "POST",
            &format!("/v1/me/follows/{ARTIST_ALICE_PAINTER}"),
            BOB,
            None,
        )
        .await;
        assert_eq!(status, 204);
    }

    let (_, list): (_, FollowsList) = get_json_authed(app, "/v1/me/follows", BOB).await;
    assert_eq!(list.items.len(), 1, "double-clicks must not duplicate rows");
}

#[sqlx::test(migrator = "MIGRATOR", fixtures("seed"))]
async fn follow_create_404_for_unknown_artist(pool: PgPool) {
    let app = app_with_test_auth(pool);
    let nonexistent = "00000000-0000-0000-0000-000000000000";
    let (status, _) = send_authed(
        app,
        "POST",
        &format!("/v1/me/follows/{nonexistent}"),
        BOB,
        None,
    )
    .await;
    assert_eq!(status, 404);
}

#[sqlx::test(migrator = "MIGRATOR", fixtures("seed"))]
async fn follow_delete_round_trip(pool: PgPool) {
    let app = app_with_test_auth(pool);

    // Follow → list shows 1
    let _ = send_authed(
        app.clone(),
        "POST",
        &format!("/v1/me/follows/{ARTIST_ALICE_PAINTER}"),
        BOB,
        None,
    )
    .await;
    let (_, list): (_, FollowsList) = get_json_authed(app.clone(), "/v1/me/follows", BOB).await;
    assert_eq!(list.items.len(), 1);

    // Unfollow → list shows 0
    let (status, _) = send_authed(
        app.clone(),
        "DELETE",
        &format!("/v1/me/follows/{ARTIST_ALICE_PAINTER}"),
        BOB,
        None,
    )
    .await;
    assert_eq!(status, 204);
    let (_, list): (_, FollowsList) = get_json_authed(app.clone(), "/v1/me/follows", BOB).await;
    assert_eq!(list.items.len(), 0);

    // Second unfollow is still 204 (idempotent)
    let (status, _) = send_authed(
        app,
        "DELETE",
        &format!("/v1/me/follows/{ARTIST_ALICE_PAINTER}"),
        BOB,
        None,
    )
    .await;
    assert_eq!(status, 204);
}

#[sqlx::test(migrator = "MIGRATOR", fixtures("seed"))]
async fn follow_list_is_per_user_isolated(pool: PgPool) {
    let app = app_with_test_auth(pool);

    // Alice follows ARTIST_BRUNO; Bob follows ARTIST_ALICE_PAINTER.
    let _ = send_authed(
        app.clone(),
        "POST",
        &format!("/v1/me/follows/{ARTIST_BRUNO}"),
        ALICE,
        None,
    )
    .await;
    let _ = send_authed(
        app.clone(),
        "POST",
        &format!("/v1/me/follows/{ARTIST_ALICE_PAINTER}"),
        BOB,
        None,
    )
    .await;

    let (_, alice_list): (_, FollowsList) =
        get_json_authed(app.clone(), "/v1/me/follows", ALICE).await;
    let (_, bob_list): (_, FollowsList) = get_json_authed(app, "/v1/me/follows", BOB).await;

    assert_eq!(alice_list.items.len(), 1);
    assert_eq!(alice_list.items[0].slug, "bruno-test");
    assert_eq!(bob_list.items.len(), 1);
    assert_eq!(bob_list.items[0].slug, "alice-test");
}

// ─────────────────────────────────────────────────────────────────────────────
// is_following + follower_count flow through to artist detail
// ─────────────────────────────────────────────────────────────────────────────

#[sqlx::test(migrator = "MIGRATOR", fixtures("seed"))]
async fn artist_detail_is_following_for_signed_in_follower(pool: PgPool) {
    let app = app_with_test_auth(pool);

    // Pre-state: not following → is_following=false
    let (_, before): (_, ArtistDetailBody) =
        get_json_authed(app.clone(), "/v1/artists/alice-test", BOB).await;
    assert!(!before.is_following);
    let baseline_count = before.follower_count;

    // Follow.
    let _ = send_authed(
        app.clone(),
        "POST",
        &format!("/v1/me/follows/{ARTIST_ALICE_PAINTER}"),
        BOB,
        None,
    )
    .await;

    // Detail now returns is_following=true and follower_count incremented.
    let (status, after): (_, ArtistDetailBody) =
        get_json_authed(app, "/v1/artists/alice-test", BOB).await;
    assert_eq!(status, 200);
    assert!(after.is_following);
    assert_eq!(after.follower_count, baseline_count + 1);
}

#[sqlx::test(migrator = "MIGRATOR", fixtures("seed"))]
async fn artist_detail_is_following_false_for_signed_out(pool: PgPool) {
    let app = app_with_test_auth(pool);

    // Alice follows alice-test (sic — Alice the user, alice-test the artist).
    let _ = send_authed(
        app.clone(),
        "POST",
        &format!("/v1/me/follows/{ARTIST_ALICE_PAINTER}"),
        ALICE,
        None,
    )
    .await;

    // Signed-out request gets is_following=false but the count reflects
    // the follow that happened.
    let (status, body): (_, ArtistDetailBody) =
        common::get_json(app, "/v1/artists/alice-test").await;
    assert_eq!(status, 200);
    assert!(!body.is_following);
    assert!(body.follower_count >= 1);
}
