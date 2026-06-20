// T-052c — anon pending-actions queue + merge replay.
//
// Per-file allow: `Deserialize`-only fields trigger dead_code under
// `-D warnings`. See `decisions.md` 2026-05-27 — Pre-commit hooks.
#![allow(dead_code)]

mod common;

use common::{app_with_test_auth, send_authed, MIGRATOR};
use serde::Deserialize;
use sqlx::PgPool;
use uuid::Uuid;

// One of the seeded user/clerk-id pairs.
const ALICE: &str = "test-user_test_alice";
const ALICE_USER_ID: &str = "88888888-8888-8888-8888-888888888888";
const ARTIST_ALICE_PAINTER: &str = "aaa11111-1111-1111-1111-111111111111";

const ANON_ID: &str = "019eaaaa-1111-7777-8888-000000000001";

// ─────────────────────────────────────────────────────────────────────────────
// POST /v1/anon/pending/follows/:artist_id
// ─────────────────────────────────────────────────────────────────────────────

#[sqlx::test(migrator = "MIGRATOR", fixtures("seed"))]
async fn queue_follow_returns_400_without_anon_id_header(pool: PgPool) {
    let app = app_with_test_auth(pool);
    let (status, _) = common::send_json(
        app,
        "POST",
        &format!("/v1/anon/pending/follows/{ARTIST_ALICE_PAINTER}"),
        None,
    )
    .await;
    assert_eq!(status, 400);
}

#[sqlx::test(migrator = "MIGRATOR", fixtures("seed"))]
async fn queue_follow_404s_for_unknown_artist(pool: PgPool) {
    let app = app_with_test_auth(pool);
    let nonexistent = "00000000-0000-0000-0000-000000000000";
    let (status, _) = common::send_with_anon_id(
        app,
        "POST",
        &format!("/v1/anon/pending/follows/{nonexistent}"),
        ANON_ID,
        None,
    )
    .await;
    assert_eq!(status, 404);
}

#[sqlx::test(migrator = "MIGRATOR", fixtures("seed"))]
async fn queue_follow_inserts_row(pool: PgPool) {
    let app = app_with_test_auth(pool.clone());
    let (status, _) = common::send_with_anon_id(
        app,
        "POST",
        &format!("/v1/anon/pending/follows/{ARTIST_ALICE_PAINTER}"),
        ANON_ID,
        None,
    )
    .await;
    assert_eq!(status, 204);

    let count: i64 = sqlx::query_scalar(
        "SELECT count(*) FROM anon_pending_actions WHERE anon_id = $1::uuid AND kind = 'follow_artist'",
    )
    .bind(ANON_ID)
    .fetch_one(&pool)
    .await
    .unwrap();
    assert_eq!(count, 1);
}

#[sqlx::test(migrator = "MIGRATOR", fixtures("seed"))]
async fn queue_follow_is_idempotent(pool: PgPool) {
    let app = app_with_test_auth(pool.clone());
    for _ in 0..3 {
        let (status, _) = common::send_with_anon_id(
            app.clone(),
            "POST",
            &format!("/v1/anon/pending/follows/{ARTIST_ALICE_PAINTER}"),
            ANON_ID,
            None,
        )
        .await;
        assert_eq!(status, 204);
    }

    let count: i64 = sqlx::query_scalar(
        "SELECT count(*) FROM anon_pending_actions WHERE anon_id = $1::uuid",
    )
    .bind(ANON_ID)
    .fetch_one(&pool)
    .await
    .unwrap();
    assert_eq!(count, 1, "repeat clicks of Follow shouldn't dup-queue");
}

// ─────────────────────────────────────────────────────────────────────────────
// merge-anonymous drains the queue
// ─────────────────────────────────────────────────────────────────────────────

#[derive(Deserialize, Debug)]
struct MergeResp {
    uploads_merged: u64,
    events_merged: u64,
    #[serde(default)]
    follows_replayed: u64,
}

#[sqlx::test(migrator = "MIGRATOR", fixtures("seed"))]
async fn merge_replays_queued_follow(pool: PgPool) {
    let app = app_with_test_auth(pool.clone());

    // Anon visitor queues a follow.
    let (status, _) = common::send_with_anon_id(
        app.clone(),
        "POST",
        &format!("/v1/anon/pending/follows/{ARTIST_ALICE_PAINTER}"),
        ANON_ID,
        None,
    )
    .await;
    assert_eq!(status, 204);

    // ...then signs in (Alice) and the bridge fires merge-anonymous.
    let (status, bytes) = common::send_authed_with_anon_id(
        app,
        "POST",
        "/v1/me/merge-anonymous",
        ALICE,
        ANON_ID,
        None,
    )
    .await;
    assert_eq!(status, 200);
    let resp: MergeResp = serde_json::from_slice(&bytes).unwrap();
    assert_eq!(resp.follows_replayed, 1);

    // Follow now exists on Alice.
    let exists: bool = sqlx::query_scalar(
        "SELECT EXISTS(SELECT 1 FROM follows WHERE user_id = $1::uuid AND artist_id = $2::uuid)",
    )
    .bind(ALICE_USER_ID)
    .bind(ARTIST_ALICE_PAINTER)
    .fetch_one(&pool)
    .await
    .unwrap();
    assert!(exists);

    // Pending queue is drained.
    let remaining: i64 = sqlx::query_scalar(
        "SELECT count(*) FROM anon_pending_actions WHERE anon_id = $1::uuid",
    )
    .bind(ANON_ID)
    .fetch_one(&pool)
    .await
    .unwrap();
    assert_eq!(remaining, 0);
}

#[sqlx::test(migrator = "MIGRATOR", fixtures("seed"))]
async fn merge_is_idempotent_for_already_followed_artist(pool: PgPool) {
    let app = app_with_test_auth(pool.clone());

    // Pre-existing follow Alice already has.
    sqlx::query(
        "INSERT INTO follows (user_id, artist_id) VALUES ($1::uuid, $2::uuid) ON CONFLICT DO NOTHING",
    )
    .bind(ALICE_USER_ID)
    .bind(ARTIST_ALICE_PAINTER)
    .execute(&pool)
    .await
    .unwrap();

    // Anon also queued the same follow.
    sqlx::query(
        r#"
        INSERT INTO anon_pending_actions (anon_id, kind, payload)
        VALUES ($1::uuid, 'follow_artist', jsonb_build_object('artist_id', $2::text))
        "#,
    )
    .bind(ANON_ID)
    .bind(ARTIST_ALICE_PAINTER)
    .execute(&pool)
    .await
    .unwrap();

    let (status, bytes) = common::send_authed_with_anon_id(
        app,
        "POST",
        "/v1/me/merge-anonymous",
        ALICE,
        ANON_ID,
        None,
    )
    .await;
    assert_eq!(status, 200);
    let resp: MergeResp = serde_json::from_slice(&bytes).unwrap();
    // Insert was a no-op because the follow already existed.
    assert_eq!(resp.follows_replayed, 0);
    // Queue still drained.
    let remaining: i64 = sqlx::query_scalar(
        "SELECT count(*) FROM anon_pending_actions WHERE anon_id = $1::uuid",
    )
    .bind(ANON_ID)
    .fetch_one(&pool)
    .await
    .unwrap();
    assert_eq!(remaining, 0);
}

#[sqlx::test(migrator = "MIGRATOR", fixtures("seed"))]
async fn merge_ignores_expired_pending_actions(pool: PgPool) {
    let app = app_with_test_auth(pool.clone());

    // Queue an action with an expires_at in the past.
    sqlx::query(
        r#"
        INSERT INTO anon_pending_actions (anon_id, kind, payload, expires_at)
        VALUES ($1::uuid, 'follow_artist', jsonb_build_object('artist_id', $2::text), now() - interval '1 day')
        "#,
    )
    .bind(ANON_ID)
    .bind(ARTIST_ALICE_PAINTER)
    .execute(&pool)
    .await
    .unwrap();

    let (status, bytes) = common::send_authed_with_anon_id(
        app,
        "POST",
        "/v1/me/merge-anonymous",
        ALICE,
        ANON_ID,
        None,
    )
    .await;
    assert_eq!(status, 200);
    let resp: MergeResp = serde_json::from_slice(&bytes).unwrap();
    assert_eq!(resp.follows_replayed, 0);

    // The expired row gets drained anyway — we don't want a stale
    // intent to ever fire later.
    let remaining: i64 = sqlx::query_scalar(
        "SELECT count(*) FROM anon_pending_actions WHERE anon_id = $1::uuid",
    )
    .bind(ANON_ID)
    .fetch_one(&pool)
    .await
    .unwrap();
    assert_eq!(remaining, 0);
}

#[sqlx::test(migrator = "MIGRATOR", fixtures("seed"))]
async fn merge_with_no_anon_cookie_is_noop(pool: PgPool) {
    let app = app_with_test_auth(pool);
    // No X-Anonymous-Id header — merge returns zeros.
    let (status, bytes) =
        send_authed(app, "POST", "/v1/me/merge-anonymous", ALICE, None).await;
    assert_eq!(status, 200);
    let resp: MergeResp = serde_json::from_slice(&bytes).unwrap();
    assert_eq!(resp.uploads_merged, 0);
    assert_eq!(resp.events_merged, 0);
    assert_eq!(resp.follows_replayed, 0);
}
