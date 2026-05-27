// Deserialize-only fields trigger dead_code under -D warnings.
// See `decisions.md` 2026-05-27 — Pre-commit hooks.
#![allow(dead_code)]

mod common;

use axum::{
    body::{to_bytes, Body},
    http::{Request, StatusCode},
    Router,
};
use common::{app_with_auth_and_fixed_vector, MIGRATOR};
use pgvector::Vector;
use serde::Deserialize;
use sqlx::PgPool;
use tower::ServiceExt;

const ALICE: &str = "test-user_test_alice";

fn unit_vector_at(pos: usize) -> Vector {
    let mut v = vec![0.0_f32; 1024];
    v[pos] = 1.0;
    Vector::from(v)
}

/// Build a minimal multipart body for the image upload endpoint. axum's
/// `Multipart` extractor accepts any spec-conformant body; we pick a
/// fixed boundary to keep the test deterministic.
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
    image_url: String,
}

async fn upload_image(
    app: Router,
    boundary: &str,
    body: Vec<u8>,
    bearer: Option<&str>,
    anon: Option<&str>,
) -> (StatusCode, Vec<u8>) {
    let mut req = Request::builder()
        .method("POST")
        .uri("/v1/uploads/image")
        .header(
            "Content-Type",
            format!("multipart/form-data; boundary={boundary}"),
        );
    if let Some(b) = bearer {
        req = req.header("Authorization", format!("Bearer {b}"));
    }
    if let Some(a) = anon {
        req = req.header("X-Anonymous-Id", a);
    }
    let resp = app
        .oneshot(req.body(Body::from(body)).expect("build req"))
        .await
        .expect("router oneshot");
    let status = resp.status();
    let bytes = to_bytes(resp.into_body(), 16 * 1024 * 1024)
        .await
        .expect("read body");
    (status, bytes.to_vec())
}

// ─────────────────────────────────────────────────────────────────────────────
// Happy path
// ─────────────────────────────────────────────────────────────────────────────

#[sqlx::test(migrator = "MIGRATOR", fixtures("seed"))]
async fn upload_image_signed_in_writes_row_and_embeds(pool: PgPool) {
    // Fixed-vector embedder so the inline embed call returns
    // immediately without touching Jina. In-memory ObjectStore (from
    // the helper's `for_tests` build) catches the PUT.
    let app = app_with_auth_and_fixed_vector(pool.clone(), unit_vector_at(42));
    let boundary = "----testboundary";
    let png_bytes: &[u8] = b"\x89PNG\r\n\x1a\nfake-png-body";
    let body = multipart_body(boundary, "test.png", "image/png", png_bytes);

    let (status, bytes) = upload_image(app, boundary, body, Some(ALICE), None).await;
    assert_eq!(
        status,
        StatusCode::CREATED,
        "body: {}",
        String::from_utf8_lossy(&bytes)
    );
    let ack: UploadAck = serde_json::from_slice(&bytes).unwrap();
    assert!(ack.s3_key.starts_with("uploads/"));
    assert!(ack.s3_key.ends_with(".png"));
    assert!(ack.image_url.ends_with(&ack.s3_key));

    // DB row exists, owned by alice, embedding written.
    let (user_id, anon_id, embedding): (Option<uuid::Uuid>, Option<uuid::Uuid>, Option<Vector>) =
        sqlx::query_as("SELECT user_id, anonymous_id, embedding FROM uploads WHERE id = $1")
            .bind(uuid::Uuid::parse_str(&ack.upload_id).unwrap())
            .fetch_one(&pool)
            .await
            .unwrap();
    assert_eq!(
        user_id,
        Some(uuid::Uuid::parse_str("88888888-8888-8888-8888-888888888888").unwrap())
    );
    assert!(anon_id.is_none());
    assert!(embedding.is_some(), "embedding should be populated");
}

#[sqlx::test(migrator = "MIGRATOR", fixtures("seed"))]
async fn upload_image_anonymous_uses_anon_id(pool: PgPool) {
    let app = app_with_auth_and_fixed_vector(pool.clone(), unit_vector_at(7));
    let boundary = "----b";
    let body = multipart_body(boundary, "t.jpg", "image/jpeg", b"jpeg-body");
    let anon = "01911234-aabb-7ccd-8eef-000000000099";
    let (status, bytes) = upload_image(app, boundary, body, None, Some(anon)).await;
    assert_eq!(
        status,
        StatusCode::CREATED,
        "body: {}",
        String::from_utf8_lossy(&bytes)
    );
    let ack: UploadAck = serde_json::from_slice(&bytes).unwrap();

    let (user_id, anon_id): (Option<uuid::Uuid>, Option<uuid::Uuid>) =
        sqlx::query_as("SELECT user_id, anonymous_id FROM uploads WHERE id = $1")
            .bind(uuid::Uuid::parse_str(&ack.upload_id).unwrap())
            .fetch_one(&pool)
            .await
            .unwrap();
    assert!(
        user_id.is_none(),
        "anonymous upload should not have a user_id"
    );
    assert_eq!(anon_id, Some(uuid::Uuid::parse_str(anon).unwrap()));
}

#[sqlx::test(migrator = "MIGRATOR", fixtures("seed"))]
async fn upload_image_extension_derived_from_content_type(pool: PgPool) {
    // The client uploaded a file named `foo.bin` but reported it as
    // image/webp. Extension comes from content-type, not the dodgy
    // filename — keeps s3_key shape predictable for the future
    // moderation + URL-validation passes.
    let app = app_with_auth_and_fixed_vector(pool, unit_vector_at(1));
    let boundary = "----b";
    let body = multipart_body(boundary, "foo.bin", "image/webp", b"webp-body");
    let (status, bytes) = upload_image(app, boundary, body, Some(ALICE), None).await;
    assert_eq!(status, StatusCode::CREATED);
    let ack: UploadAck = serde_json::from_slice(&bytes).unwrap();
    assert!(ack.s3_key.ends_with(".webp"));
}

// ─────────────────────────────────────────────────────────────────────────────
// Validation
// ─────────────────────────────────────────────────────────────────────────────

#[sqlx::test(migrator = "MIGRATOR", fixtures("seed"))]
async fn upload_image_rejects_non_image_content_type(pool: PgPool) {
    let app = app_with_auth_and_fixed_vector(pool, unit_vector_at(1));
    let boundary = "----b";
    let body = multipart_body(boundary, "boom.exe", "application/octet-stream", b"hi");
    let (status, _) = upload_image(app, boundary, body, Some(ALICE), None).await;
    assert_eq!(status, 400);
}

#[sqlx::test(migrator = "MIGRATOR", fixtures("seed"))]
async fn upload_image_rejects_empty_body(pool: PgPool) {
    let app = app_with_auth_and_fixed_vector(pool, unit_vector_at(1));
    let boundary = "----b";
    let body = multipart_body(boundary, "empty.png", "image/png", b"");
    let (status, _) = upload_image(app, boundary, body, Some(ALICE), None).await;
    assert_eq!(status, 400);
}

#[sqlx::test(migrator = "MIGRATOR", fixtures("seed"))]
async fn upload_image_rejects_missing_image_field(pool: PgPool) {
    // A multipart body with a different field name; the handler
    // explicitly looks for `image` and 400s otherwise.
    let app = app_with_auth_and_fixed_vector(pool, unit_vector_at(1));
    let boundary = "----b";
    let mut body = Vec::new();
    body.extend_from_slice(format!("--{boundary}\r\n").as_bytes());
    body.extend_from_slice(
        b"Content-Disposition: form-data; name=\"not_image\"\r\nContent-Type: text/plain\r\n\r\n",
    );
    body.extend_from_slice(b"hello");
    body.extend_from_slice(format!("\r\n--{boundary}--\r\n").as_bytes());

    let (status, _) = upload_image(app, boundary, body, Some(ALICE), None).await;
    assert_eq!(status, 400);
}
