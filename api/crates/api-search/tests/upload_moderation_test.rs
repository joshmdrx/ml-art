// T-008b integration tests — moderation on the uploads bucket.
//
// Asserts:
// 1. Uploads landing through POST /v1/uploads/image enqueue an
//    `upload_moderate` job with the right idempotency key.
// 2. The `moderate_upload` handler flips pending → approved (Disabled
//    client) or pending → rejected (canned Test client).
// 3. Visual search refuses to anchor on a row whose moderation_status
//    is 'rejected' — returns 404, same shape as a non-existent upload
//    (avoids leaking whether the abuse upload landed).
// 4. Pending and approved uploads still resolve as anchors — the
//    upload→moderate race window doesn't break the uploader's own
//    visual search.
// 5. Missing-row handler call is a no-op.

#![allow(dead_code)]

mod common;

use axum::{
    body::{to_bytes, Body},
    http::{Request, StatusCode},
    Router,
};
use common::{app_with_auth_fixed_vector_postgres_jobs, MIGRATOR};
use ml_art_core::{
    db::Pool,
    moderation::{moderate_upload, ModerationClient, ModerationResult},
};
use pgvector::Vector;
use serde::Deserialize;
use serde_json::Value;
use sqlx::PgPool;
use tower::ServiceExt;
use uuid::Uuid;

const ALICE: &str = "test-user_test_alice";

fn unit_vector_at(pos: usize) -> Vector {
    let mut v = vec![0.0_f32; 1024];
    v[pos] = 1.0;
    Vector::from(v)
}

fn multipart_body(boundary: &str, filename: &str, content_type: &str, bytes: &[u8]) -> Vec<u8> {
    let mut out = Vec::new();
    out.extend_from_slice(format!("--{boundary}\r\n").as_bytes());
    out.extend_from_slice(
        format!("Content-Disposition: form-data; name=\"image\"; filename=\"{filename}\"\r\n")
            .as_bytes(),
    );
    out.extend_from_slice(format!("Content-Type: {content_type}\r\n\r\n").as_bytes());
    out.extend_from_slice(bytes);
    out.extend_from_slice(format!("\r\n--{boundary}--\r\n").as_bytes());
    out
}

#[derive(Deserialize, Debug)]
struct UploadAck {
    upload_id: String,
    s3_key: String,
}

// Walk through the full /v1/uploads/image flow with Alice's bearer.
async fn do_upload(app: Router) -> UploadAck {
    let boundary = "----testboundary";
    let png_bytes: &[u8] = b"\x89PNG\r\n\x1a\nfake-png-body";
    let body = multipart_body(boundary, "test.png", "image/png", png_bytes);
    let resp = app
        .oneshot(
            Request::builder()
                .method("POST")
                .uri("/v1/uploads/image")
                .header(
                    "Content-Type",
                    format!("multipart/form-data; boundary={boundary}"),
                )
                .header("Authorization", format!("Bearer {ALICE}"))
                .body(Body::from(body))
                .unwrap(),
        )
        .await
        .unwrap();
    let status = resp.status();
    let bytes = to_bytes(resp.into_body(), 4 * 1024 * 1024).await.unwrap();
    assert_eq!(
        status,
        StatusCode::CREATED,
        "upload failed: {}",
        String::from_utf8_lossy(&bytes)
    );
    serde_json::from_slice(&bytes).unwrap()
}

async fn insert_upload(pool: &Pool, s3_key: &str, status: &str) -> Uuid {
    let id = Uuid::new_v4();
    let vec = unit_vector_at(7);
    sqlx::query(
        r#"INSERT INTO uploads (id, s3_key, embedding, moderation_status)
           VALUES ($1, $2, $3, $4)"#,
    )
    .bind(id)
    .bind(s3_key)
    .bind(&vec)
    .bind(status)
    .execute(pool)
    .await
    .unwrap();
    id
}

// ─────────────────────────────────────────────────────────────────────────────
// 1. uploads::create enqueues UploadModerate
// ─────────────────────────────────────────────────────────────────────────────

#[sqlx::test(migrator = "MIGRATOR", fixtures("seed"))]
async fn upload_enqueues_moderation_job(pool: PgPool) {
    let app = app_with_auth_fixed_vector_postgres_jobs(pool.clone(), unit_vector_at(11));
    let ack = do_upload(app).await;

    let (kind, payload): (String, Value) =
        sqlx::query_as("SELECT kind, payload FROM jobs WHERE idempotency_key = $1")
            .bind(format!("moderate:upload:{}", ack.upload_id))
            .fetch_one(&pool)
            .await
            .expect("upload moderation job present");
    assert_eq!(kind, "upload_moderate");
    assert_eq!(
        payload["upload_id"].as_str(),
        Some(ack.upload_id.as_str()),
        "payload references the upload"
    );

    // Fresh row defaults to pending — the worker hasn't run yet.
    let (status,): (String,) =
        sqlx::query_as("SELECT moderation_status FROM uploads WHERE id = $1")
            .bind(Uuid::parse_str(&ack.upload_id).unwrap())
            .fetch_one(&pool)
            .await
            .unwrap();
    assert_eq!(status, "pending");
}

// ─────────────────────────────────────────────────────────────────────────────
// 2. Handler flips pending → approved / rejected
// ─────────────────────────────────────────────────────────────────────────────

#[sqlx::test(migrator = "MIGRATOR", fixtures("seed"))]
async fn handler_approves_via_disabled_client(pool: PgPool) {
    let id = insert_upload(&pool, "uploads/clean.jpg", "pending").await;
    moderate_upload(&ModerationClient::disabled(), &pool, id)
        .await
        .unwrap();
    let (status,): (String,) =
        sqlx::query_as("SELECT moderation_status FROM uploads WHERE id = $1")
            .bind(id)
            .fetch_one(&pool)
            .await
            .unwrap();
    assert_eq!(status, "approved");
}

#[sqlx::test(migrator = "MIGRATOR", fixtures("seed"))]
async fn handler_rejects_via_canned_client(pool: PgPool) {
    let id = insert_upload(&pool, "uploads/bad.jpg", "pending").await;
    let client = ModerationClient::for_tests(vec![(
        "uploads/bad.jpg".to_string(),
        ModerationResult::rejected(vec!["Violence".to_string()]),
    )]);
    moderate_upload(&client, &pool, id).await.unwrap();
    let (status,): (String,) =
        sqlx::query_as("SELECT moderation_status FROM uploads WHERE id = $1")
            .bind(id)
            .fetch_one(&pool)
            .await
            .unwrap();
    assert_eq!(status, "rejected");
}

#[sqlx::test(migrator = "MIGRATOR", fixtures("seed"))]
async fn handler_noop_for_missing_row(pool: PgPool) {
    moderate_upload(&ModerationClient::disabled(), &pool, Uuid::new_v4())
        .await
        .unwrap();
}

// ─────────────────────────────────────────────────────────────────────────────
// 3. Visual search refuses rejected uploads
// ─────────────────────────────────────────────────────────────────────────────

async fn search_anchor_status(app: Router, upload_id: Uuid) -> StatusCode {
    let resp = app
        .oneshot(
            Request::builder()
                .method("GET")
                .uri(format!(
                    "/v1/search?image_upload_id={upload_id}&limit=1"
                ))
                .body(Body::empty())
                .unwrap(),
        )
        .await
        .unwrap();
    resp.status()
}

#[sqlx::test(migrator = "MIGRATOR", fixtures("seed"))]
async fn rejected_upload_returns_404_from_visual_search(pool: PgPool) {
    let id = insert_upload(&pool, "uploads/rejected.jpg", "rejected").await;
    let app = app_with_auth_fixed_vector_postgres_jobs(pool, unit_vector_at(13));
    assert_eq!(search_anchor_status(app, id).await, StatusCode::NOT_FOUND);
}

#[sqlx::test(migrator = "MIGRATOR", fixtures("seed"))]
async fn pending_upload_still_resolves_for_visual_search(pool: PgPool) {
    // Race window between upload + moderation — the uploader's own
    // immediate search should not 404. Status here is 200 (or 400 if
    // embedding NULL, but we set it above).
    let id = insert_upload(&pool, "uploads/pending.jpg", "pending").await;
    let app = app_with_auth_fixed_vector_postgres_jobs(pool, unit_vector_at(15));
    assert_eq!(search_anchor_status(app, id).await, StatusCode::OK);
}

#[sqlx::test(migrator = "MIGRATOR", fixtures("seed"))]
async fn approved_upload_resolves_for_visual_search(pool: PgPool) {
    let id = insert_upload(&pool, "uploads/approved.jpg", "approved").await;
    let app = app_with_auth_fixed_vector_postgres_jobs(pool, unit_vector_at(17));
    assert_eq!(search_anchor_status(app, id).await, StatusCode::OK);
}

// ─────────────────────────────────────────────────────────────────────────────
// 4. Idempotency at the queue layer
// ─────────────────────────────────────────────────────────────────────────────

#[sqlx::test(migrator = "MIGRATOR", fixtures("seed"))]
async fn double_enqueue_dedups(pool: PgPool) {
    use ml_art_core::jobs::{EnqueueOpts, JobEvent, JobsBackend};

    let backend = JobsBackend::postgres(pool.clone());
    let id = Uuid::new_v4();
    let key = format!("moderate:upload:{id}");
    let opts = EnqueueOpts {
        idempotency_key: Some(key.clone()),
        ..Default::default()
    };
    backend
        .enqueue(JobEvent::UploadModerate { upload_id: id }, opts.clone())
        .await
        .unwrap();
    backend
        .enqueue(JobEvent::UploadModerate { upload_id: id }, opts)
        .await
        .unwrap();

    let (n,): (i64,) =
        sqlx::query_as("SELECT count(*) FROM jobs WHERE idempotency_key = $1")
            .bind(&key)
            .fetch_one(&pool)
            .await
            .unwrap();
    assert_eq!(n, 1, "second enqueue with same key is a no-op");
}
