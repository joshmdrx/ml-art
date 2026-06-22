//! T-054 — inbound-email webhook (`POST /v1/webhooks/email/inbound`).
//!
//! Exercises the transport-neutral core the Cloudflare Email Worker drives:
//! shared-secret auth → HMAC token verify → replay-safe persist → forward
//! enqueue. We hit the real router with a Postgres-backed jobs backend so
//! every assertion is against committed DB state (the `inquiry_replies` row
//! and the `jobs` table), not in-memory capture.
//!
//! The endpoint authenticates with an `X-Inbound-Secret` header that
//! `common::send_json` can't set, so requests are built inline. Each call
//! gets a fresh router (`oneshot` consumes it) over the shared pool.

mod common;

use axum::{
    body::{to_bytes, Body},
    http::{Request, StatusCode},
};
use common::{app_with_postgres_jobs, MIGRATOR};
use ml_art_core::reply_address;
use serde_json::{json, Value};
use sqlx::PgPool;
use tower::ServiceExt;
use uuid::Uuid;

const ALICE_ARTIST: &str = "aaa11111-1111-1111-1111-111111111111";
const BLUE_MORNING: &str = "bbb11111-1111-1111-1111-111111111111";
/// Matches `Config::for_tests`: the HMAC secret the webhook verifies the
/// to-address against, and the shared header secret it authenticates with.
const COOKIE_SECRET: &[u8] = b"test-cookie-secret";
const INBOUND_SECRET: &str = "test-inbound-secret";
const REPLY_DOMAIN: &str = "reply.test.example.com";

/// Seed an inquiry on Blue Morning / Alice from `buyer@example.com`.
async fn seed_inquiry(pool: &PgPool) -> Uuid {
    sqlx::query_scalar(
        r#"INSERT INTO inquiries (artwork_id, artist_id, from_email, from_name, message)
           VALUES ($1, $2, 'buyer@example.com', 'Buyer', 'Is this available?')
           RETURNING id"#,
    )
    .bind(Uuid::parse_str(BLUE_MORNING).unwrap())
    .bind(Uuid::parse_str(ALICE_ARTIST).unwrap())
    .fetch_one(pool)
    .await
    .unwrap()
}

/// POST a JSON body to the webhook with an optional `X-Inbound-Secret`
/// header. Returns (status, parsed-body). A fresh router is built per call
/// over the shared pool, since `oneshot` consumes the `Router`.
async fn post_inbound(pool: &PgPool, secret: Option<&str>, body: &Value) -> (StatusCode, Value) {
    let app = app_with_postgres_jobs(pool.clone());
    let mut req = Request::builder()
        .method("POST")
        .uri("/v1/webhooks/email/inbound")
        .header("content-type", "application/json");
    if let Some(s) = secret {
        req = req.header("x-inbound-secret", s);
    }
    let resp = app
        .oneshot(
            req.body(Body::from(body.to_string()))
                .expect("build request"),
        )
        .await
        .expect("router oneshot");
    let status = resp.status();
    let bytes = to_bytes(resp.into_body(), 1024 * 1024)
        .await
        .expect("read body");
    let parsed = if bytes.is_empty() {
        Value::Null
    } else {
        serde_json::from_slice(&bytes).unwrap_or(Value::Null)
    };
    (status, parsed)
}

async fn count_replies(pool: &PgPool, inquiry_id: Uuid) -> i64 {
    sqlx::query_scalar("SELECT count(*)::bigint FROM inquiry_replies WHERE inquiry_id = $1")
        .bind(inquiry_id)
        .fetch_one(pool)
        .await
        .unwrap()
}

async fn count_forward_jobs(pool: &PgPool) -> i64 {
    sqlx::query_scalar(
        "SELECT count(*)::bigint FROM jobs WHERE kind = 'inquiry_send_reply_forward'",
    )
    .fetch_one(pool)
    .await
    .unwrap()
}

#[sqlx::test(migrator = "MIGRATOR", fixtures("seed"))]
async fn happy_path_threads_reply_and_enqueues_forward(pool: PgPool) {
    let inquiry_id = seed_inquiry(&pool).await;
    let to = reply_address::mint(inquiry_id, REPLY_DOMAIN, COOKIE_SECRET);

    let body = json!({
        "to": to,
        "from": "buyer@example.com",
        "message": "Yes, still very interested!",
        "message_id": "<m-happy@mail>",
    });
    let (status, json) = post_inbound(&pool, Some(INBOUND_SECRET), &body).await;

    assert_eq!(status, StatusCode::OK, "body: {json}");
    assert_eq!(json["status"], "accepted");

    // Exactly one threaded row, written as an inquirer-inbound reply.
    let (from_role, has_artist, message): (String, bool, String) = sqlx::query_as(
        "SELECT from_role, artist_id IS NOT NULL, message
         FROM inquiry_replies WHERE inquiry_id = $1",
    )
    .bind(inquiry_id)
    .fetch_one(&pool)
    .await
    .unwrap();
    assert_eq!(from_role, "inquirer");
    assert!(!has_artist, "inquirer rows carry NULL artist_id");
    assert_eq!(message, "Yes, still very interested!");

    assert_eq!(count_forward_jobs(&pool).await, 1, "one forward enqueued");
}

#[sqlx::test(migrator = "MIGRATOR", fixtures("seed"))]
async fn tampered_token_is_rejected(pool: PgPool) {
    let inquiry_id = seed_inquiry(&pool).await;
    // Minted under a DIFFERENT secret — the HMAC won't verify under the
    // webhook's real `anon_cookie_secret`, so the address is unrecognised.
    let forged = reply_address::mint(inquiry_id, REPLY_DOMAIN, b"not-the-real-secret");

    let body = json!({
        "to": forged,
        "from": "attacker@example.com",
        "message": "let me in",
        "message_id": "<m-forged@mail>",
    });
    let (status, _json) = post_inbound(&pool, Some(INBOUND_SECRET), &body).await;

    assert_eq!(status, StatusCode::BAD_REQUEST);
    assert_eq!(
        count_replies(&pool, inquiry_id).await,
        0,
        "no row persisted"
    );
    assert_eq!(count_forward_jobs(&pool).await, 0, "nothing enqueued");
}

#[sqlx::test(migrator = "MIGRATOR", fixtures("seed"))]
async fn missing_or_wrong_secret_is_unauthorized(pool: PgPool) {
    let inquiry_id = seed_inquiry(&pool).await;
    let to = reply_address::mint(inquiry_id, REPLY_DOMAIN, COOKIE_SECRET);
    let body = json!({
        "to": to,
        "from": "buyer@example.com",
        "message": "Yes!",
        "message_id": "<m-noauth@mail>",
    });

    // No header at all.
    let (status, _) = post_inbound(&pool, None, &body).await;
    assert_eq!(status, StatusCode::UNAUTHORIZED);

    // Present but wrong.
    let (status, _) = post_inbound(&pool, Some("nope"), &body).await;
    assert_eq!(status, StatusCode::UNAUTHORIZED);

    // Auth is checked before any DB work — nothing should have landed.
    assert_eq!(count_replies(&pool, inquiry_id).await, 0);
    assert_eq!(count_forward_jobs(&pool).await, 0);
}

#[sqlx::test(migrator = "MIGRATOR", fixtures("seed"))]
async fn replay_same_message_id_is_idempotent(pool: PgPool) {
    let inquiry_id = seed_inquiry(&pool).await;
    let to = reply_address::mint(inquiry_id, REPLY_DOMAIN, COOKIE_SECRET);
    let body = json!({
        "to": to,
        "from": "buyer@example.com",
        "message": "Yes, still very interested!",
        "message_id": "<m-dup@mail>",
    });

    // First delivery threads the reply and enqueues a forward.
    let (status, json) = post_inbound(&pool, Some(INBOUND_SECRET), &body).await;
    assert_eq!(status, StatusCode::OK);
    assert_eq!(json["status"], "accepted");

    // Re-delivery of the same Message-ID is a no-op (partial unique index).
    let (status, json) = post_inbound(&pool, Some(INBOUND_SECRET), &body).await;
    assert_eq!(status, StatusCode::OK);
    assert_eq!(json["status"], "duplicate");

    assert_eq!(count_replies(&pool, inquiry_id).await, 1, "exactly one row");
    assert_eq!(count_forward_jobs(&pool).await, 1, "exactly one forward");
}
