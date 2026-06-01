// T-033 integration tests — POST /v1/me/merge-anonymous.
//
// Asserts the calling user's anon_id-keyed rows (uploads, events) get
// stamped with their now-known user_id, that the call is idempotent,
// and that we never trample another user's link.

#![allow(dead_code)]

mod common;

use axum::{
    body::{to_bytes, Body},
    http::{Request, StatusCode},
    Router,
};
use common::{app_with_test_auth, MIGRATOR};
use serde::Deserialize;
use sqlx::PgPool;
use tower::ServiceExt;
use uuid::Uuid;

const ALICE: &str = "test-user_test_alice";
const ALICE_USER_ID: &str = "88888888-8888-8888-8888-888888888888";
const BOB: &str = "test-user_test_bob";
const BOB_USER_ID: &str = "77777777-7777-7777-7777-777777777777";

#[derive(Deserialize, Debug)]
struct MergeResp {
    uploads_merged: u64,
    events_merged: u64,
}

async fn post_merge(
    app: Router,
    bearer: Option<&str>,
    anon: Option<&str>,
) -> (StatusCode, Vec<u8>) {
    let mut req = Request::builder()
        .method("POST")
        .uri("/v1/me/merge-anonymous");
    if let Some(b) = bearer {
        req = req.header("Authorization", format!("Bearer {b}"));
    }
    if let Some(a) = anon {
        req = req.header("X-Anonymous-Id", a);
    }
    let resp = app
        .oneshot(req.body(Body::empty()).unwrap())
        .await
        .unwrap();
    let status = resp.status();
    let bytes = to_bytes(resp.into_body(), 4 * 1024 * 1024)
        .await
        .unwrap();
    (status, bytes.to_vec())
}

async fn insert_anon_upload(pool: &PgPool, s3_key: &str, anon: Uuid) -> Uuid {
    let id = Uuid::new_v4();
    sqlx::query(
        r#"INSERT INTO uploads (id, s3_key, anonymous_id)
           VALUES ($1, $2, $3)"#,
    )
    .bind(id)
    .bind(s3_key)
    .bind(anon)
    .execute(pool)
    .await
    .unwrap();
    id
}

async fn insert_anon_event(pool: &PgPool, name: &str, anon: Uuid) -> Uuid {
    let id = Uuid::new_v4();
    sqlx::query(
        r#"INSERT INTO events (id, anonymous_id, event_name)
           VALUES ($1, $2, $3)"#,
    )
    .bind(id)
    .bind(anon)
    .bind(name)
    .execute(pool)
    .await
    .unwrap();
    id
}

async fn upload_user_id(pool: &PgPool, id: Uuid) -> Option<Uuid> {
    let (uid,): (Option<Uuid>,) =
        sqlx::query_as("SELECT user_id FROM uploads WHERE id = $1")
            .bind(id)
            .fetch_one(pool)
            .await
            .unwrap();
    uid
}

async fn event_user_id(pool: &PgPool, id: Uuid) -> Option<Uuid> {
    let (uid,): (Option<Uuid>,) =
        sqlx::query_as("SELECT user_id FROM events WHERE id = $1")
            .bind(id)
            .fetch_one(pool)
            .await
            .unwrap();
    uid
}

// ─────────────────────────────────────────────────────────────────────────────
// Happy path
// ─────────────────────────────────────────────────────────────────────────────

#[sqlx::test(migrator = "MIGRATOR", fixtures("seed"))]
async fn merge_stamps_user_id_on_anon_rows(pool: PgPool) {
    let anon = Uuid::new_v4();
    let upload_id = insert_anon_upload(&pool, "uploads/anon-1.jpg", anon).await;
    let event_id = insert_anon_event(&pool, "search", anon).await;

    let app = app_with_test_auth(pool.clone());
    let (status, bytes) =
        post_merge(app, Some(ALICE), Some(&anon.to_string())).await;
    assert_eq!(status, 200, "body: {}", String::from_utf8_lossy(&bytes));
    let resp: MergeResp = serde_json::from_slice(&bytes).unwrap();
    assert_eq!(resp.uploads_merged, 1);
    assert_eq!(resp.events_merged, 1);

    assert_eq!(
        upload_user_id(&pool, upload_id).await,
        Some(Uuid::parse_str(ALICE_USER_ID).unwrap()),
        "upload row was stamped with alice's id"
    );
    assert_eq!(
        event_user_id(&pool, event_id).await,
        Some(Uuid::parse_str(ALICE_USER_ID).unwrap()),
        "event row was stamped with alice's id"
    );
}

// ─────────────────────────────────────────────────────────────────────────────
// No-op shapes
// ─────────────────────────────────────────────────────────────────────────────

#[sqlx::test(migrator = "MIGRATOR", fixtures("seed"))]
async fn merge_without_anon_header_is_zero_op(pool: PgPool) {
    // Some rows exist for an unrelated anon — they MUST be untouched
    // because the caller didn't supply X-Anonymous-Id.
    let other_anon = Uuid::new_v4();
    let upload_id = insert_anon_upload(&pool, "uploads/other.jpg", other_anon).await;

    let app = app_with_test_auth(pool.clone());
    let (status, bytes) = post_merge(app, Some(ALICE), None).await;
    assert_eq!(status, 200);
    let resp: MergeResp = serde_json::from_slice(&bytes).unwrap();
    assert_eq!(resp.uploads_merged, 0);
    assert_eq!(resp.events_merged, 0);

    assert!(
        upload_user_id(&pool, upload_id).await.is_none(),
        "the unrelated anon row is untouched",
    );
}

#[sqlx::test(migrator = "MIGRATOR", fixtures("seed"))]
async fn merge_with_anon_having_no_rows_returns_zero(pool: PgPool) {
    // Fresh anon id with nothing keyed off it — should be a clean zero.
    let anon = Uuid::new_v4();
    let app = app_with_test_auth(pool);
    let (status, bytes) =
        post_merge(app, Some(ALICE), Some(&anon.to_string())).await;
    assert_eq!(status, 200);
    let resp: MergeResp = serde_json::from_slice(&bytes).unwrap();
    assert_eq!(resp.uploads_merged, 0);
    assert_eq!(resp.events_merged, 0);
}

// ─────────────────────────────────────────────────────────────────────────────
// Idempotency
// ─────────────────────────────────────────────────────────────────────────────

#[sqlx::test(migrator = "MIGRATOR", fixtures("seed"))]
async fn second_merge_is_zero_op(pool: PgPool) {
    let anon = Uuid::new_v4();
    insert_anon_upload(&pool, "uploads/dup-1.jpg", anon).await;
    insert_anon_upload(&pool, "uploads/dup-2.jpg", anon).await;

    let app = app_with_test_auth(pool.clone());
    let (_, bytes1) =
        post_merge(app.clone(), Some(ALICE), Some(&anon.to_string())).await;
    let r1: MergeResp = serde_json::from_slice(&bytes1).unwrap();
    assert_eq!(r1.uploads_merged, 2);

    // Second call should find no `user_id IS NULL` rows.
    let (_, bytes2) =
        post_merge(app, Some(ALICE), Some(&anon.to_string())).await;
    let r2: MergeResp = serde_json::from_slice(&bytes2).unwrap();
    assert_eq!(r2.uploads_merged, 0, "second merge is a no-op");
}

// ─────────────────────────────────────────────────────────────────────────────
// Ownership safety — never overwrite an existing user_id
// ─────────────────────────────────────────────────────────────────────────────

#[sqlx::test(migrator = "MIGRATOR", fixtures("seed"))]
async fn merge_never_overwrites_existing_user_id(pool: PgPool) {
    // A row already linked to Bob. Alice calls merge with the same
    // anon_id — Bob's row must NOT flip to Alice.
    let anon = Uuid::new_v4();
    let upload_id = Uuid::new_v4();
    sqlx::query(
        r#"INSERT INTO uploads (id, s3_key, anonymous_id, user_id)
           VALUES ($1, 'uploads/bobs.jpg', $2, $3)"#,
    )
    .bind(upload_id)
    .bind(anon)
    .bind(Uuid::parse_str(BOB_USER_ID).unwrap())
    .execute(&pool)
    .await
    .unwrap();

    let app = app_with_test_auth(pool.clone());
    let (status, bytes) =
        post_merge(app, Some(ALICE), Some(&anon.to_string())).await;
    assert_eq!(status, 200);
    let resp: MergeResp = serde_json::from_slice(&bytes).unwrap();
    assert_eq!(resp.uploads_merged, 0, "row already owned — not migrated");

    assert_eq!(
        upload_user_id(&pool, upload_id).await,
        Some(Uuid::parse_str(BOB_USER_ID).unwrap()),
        "Bob's link is preserved"
    );
}

#[sqlx::test(migrator = "MIGRATOR", fixtures("seed"))]
async fn merge_only_picks_up_calling_users_anon(pool: PgPool) {
    // Two different anons. Alice supplies hers — Bob's anon-rows
    // must not be touched.
    let alice_anon = Uuid::new_v4();
    let bob_anon = Uuid::new_v4();
    let alice_upload = insert_anon_upload(&pool, "uploads/alice.jpg", alice_anon).await;
    let bob_upload = insert_anon_upload(&pool, "uploads/bob.jpg", bob_anon).await;

    let app = app_with_test_auth(pool.clone());
    let (status, bytes) =
        post_merge(app, Some(ALICE), Some(&alice_anon.to_string())).await;
    assert_eq!(status, 200);
    let resp: MergeResp = serde_json::from_slice(&bytes).unwrap();
    assert_eq!(resp.uploads_merged, 1);

    assert_eq!(
        upload_user_id(&pool, alice_upload).await,
        Some(Uuid::parse_str(ALICE_USER_ID).unwrap()),
        "Alice's upload claimed"
    );
    assert!(
        upload_user_id(&pool, bob_upload).await.is_none(),
        "Bob's anon row is untouched — different anon_id"
    );
}

// ─────────────────────────────────────────────────────────────────────────────
// Auth gate
// ─────────────────────────────────────────────────────────────────────────────

#[sqlx::test(migrator = "MIGRATOR", fixtures("seed"))]
async fn anonymous_request_returns_401(pool: PgPool) {
    let anon = Uuid::new_v4();
    let app = app_with_test_auth(pool);
    let (status, _) = post_merge(app, None, Some(&anon.to_string())).await;
    assert_eq!(status, StatusCode::UNAUTHORIZED);
}

#[sqlx::test(migrator = "MIGRATOR", fixtures("seed"))]
async fn malformed_anon_header_returns_400(pool: PgPool) {
    let app = app_with_test_auth(pool);
    let (status, _) = post_merge(app, Some(ALICE), Some("not-a-uuid")).await;
    assert_eq!(status, StatusCode::BAD_REQUEST);
}
