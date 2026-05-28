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

// ─────────────────────────────────────────────────────────────────────────────
// Phase B — /v1/search?image_upload_id=…
// ─────────────────────────────────────────────────────────────────────────────

#[derive(Deserialize, Debug)]
struct SearchPage {
    items: Vec<SearchItem>,
}
#[derive(Deserialize, Debug)]
struct SearchItem {
    id: String,
    title: Option<String>,
}

/// Insert an `uploads` row with a pre-computed embedding so the
/// search-by-upload path can be tested without exercising the upload
/// endpoint (which would also embed). Keeps the search-side concerns
/// isolated.
async fn seed_upload(pool: &PgPool, embedding: Vector) -> uuid::Uuid {
    let id = uuid::Uuid::new_v4();
    sqlx::query(
        r#"
        INSERT INTO uploads (id, s3_key, embedding)
        VALUES ($1, $2, $3)
        "#,
    )
    .bind(id)
    .bind(format!("uploads/{id}.png"))
    .bind(&embedding)
    .execute(pool)
    .await
    .unwrap();
    id
}

#[sqlx::test(migrator = "MIGRATOR", fixtures("seed"))]
async fn search_by_image_upload_returns_vector_ranked_results(pool: PgPool) {
    // Seed an upload whose embedding matches one of the fixture
    // artworks exactly (unit vector at the same dim). The search must
    // return that artwork as its top result — cosine distance = 0.
    //
    // The fixture writes each artwork's embedding as a one-hot vector
    // whose `1.0` position is the artwork's row order in the VALUES
    // list (0 = Blue Morning). Picking pos=0 makes Blue Morning the
    // nearest neighbour.
    let upload_id = seed_upload(&pool, unit_vector_at(0)).await;

    let app = common::app_with_fixed_vector(pool, unit_vector_at(999));
    let (status, page): (_, SearchPage) = common::get_json(
        app,
        &format!("/v1/search?image_upload_id={upload_id}&limit=5"),
    )
    .await;
    assert_eq!(status, 200);
    assert!(!page.items.is_empty(), "should return semantic matches");
    // Blue Morning at pos=0 should rank first.
    assert_eq!(page.items[0].title.as_deref(), Some("Blue Morning"));
}

#[sqlx::test(migrator = "MIGRATOR", fixtures("seed"))]
async fn search_by_unknown_image_upload_id_404s(pool: PgPool) {
    let app = common::app_with_fixed_vector(pool, unit_vector_at(1));
    let bogus = uuid::Uuid::new_v4();
    let (status, _) = common::get_status(app, &format!("/v1/search?image_upload_id={bogus}")).await;
    assert_eq!(status, 404);
}

#[sqlx::test(migrator = "MIGRATOR", fixtures("seed"))]
async fn search_by_upload_without_embedding_400s(pool: PgPool) {
    // Insert an `uploads` row with NULL embedding (the state a
    // mid-flight upload would be in if the embed step hadn't completed).
    let upload_id = uuid::Uuid::new_v4();
    sqlx::query("INSERT INTO uploads (id, s3_key) VALUES ($1, $2)")
        .bind(upload_id)
        .bind(format!("uploads/{upload_id}.png"))
        .execute(&pool)
        .await
        .unwrap();

    let app = common::app_with_fixed_vector(pool, unit_vector_at(1));
    let (status, _) =
        common::get_status(app, &format!("/v1/search?image_upload_id={upload_id}")).await;
    assert_eq!(status, 400);
}

#[sqlx::test(migrator = "MIGRATOR", fixtures("seed"))]
async fn search_image_upload_wins_over_text_for_semantic_anchor(pool: PgPool) {
    // With both `q` AND `image_upload_id`, the image vector should
    // drive the semantic ranking. We seed an upload at pos=0 (Blue
    // Morning's vector) and pass `q=zzz-no-match` so the keyword side
    // returns nothing — leaving only the semantic side, which must
    // surface Blue Morning first.
    let upload_id = seed_upload(&pool, unit_vector_at(0)).await;

    // Fixed-vector embedder returns pos=999 for the text query, which
    // wouldn't match any seeded artwork. If the image anchor is being
    // ignored in favour of text, we'd see no results.
    let app = common::app_with_fixed_vector(pool, unit_vector_at(999));
    let (_, page): (_, SearchPage) = common::get_json(
        app,
        &format!("/v1/search?image_upload_id={upload_id}&q=zzz-no-match"),
    )
    .await;
    assert!(
        page.items
            .iter()
            .any(|a| a.title.as_deref() == Some("Blue Morning")),
        "Blue Morning should appear via the image anchor even when text doesn't match"
    );
}

#[sqlx::test(migrator = "MIGRATOR", fixtures("seed"))]
async fn search_by_image_upload_respects_filters(pool: PgPool) {
    // Image anchor + medium filter narrows the result set. Blue
    // Morning is medium='Painting'; passing medium=Sculpture should
    // drop it from the results even though it's the nearest neighbour.
    let upload_id = seed_upload(&pool, unit_vector_at(0)).await;
    let app = common::app_with_fixed_vector(pool, unit_vector_at(1));
    let (_, page): (_, SearchPage) = common::get_json(
        app,
        &format!("/v1/search?image_upload_id={upload_id}&medium=Sculpture"),
    )
    .await;
    assert!(
        !page
            .items
            .iter()
            .any(|a| a.title.as_deref() == Some("Blue Morning")),
        "medium=Sculpture filter should exclude Blue Morning (medium='Painting')"
    );
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
