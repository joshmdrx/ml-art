mod common;

use axum::{
    body::{to_bytes, Body},
    http::{Request, StatusCode},
};
use common::{app_keyword_only, MIGRATOR};
use sqlx::PgPool;
use tower::ServiceExt;

const VALID_ANON: &str = "01911234-aabb-7ccd-8eef-001122334455";

async fn send_with_optional_header(
    pool: PgPool,
    header_value: Option<&str>,
) -> (StatusCode, Vec<u8>) {
    let app = app_keyword_only(pool);
    let mut req = Request::builder().uri("/v1/search?limit=2");
    if let Some(v) = header_value {
        req = req.header("X-Anonymous-Id", v);
    }
    let response = app
        .oneshot(req.body(Body::empty()).unwrap())
        .await
        .expect("router oneshot");
    let status = response.status();
    let bytes = to_bytes(response.into_body(), 4 * 1024 * 1024)
        .await
        .expect("body");
    (status, bytes.to_vec())
}

#[sqlx::test(migrator = "MIGRATOR", fixtures("seed"))]
async fn search_accepts_request_with_no_anon_header(pool: PgPool) {
    let (status, _) = send_with_optional_header(pool, None).await;
    assert_eq!(status, 200);
}

#[sqlx::test(migrator = "MIGRATOR", fixtures("seed"))]
async fn search_accepts_request_with_valid_anon_header(pool: PgPool) {
    let (status, _) = send_with_optional_header(pool, Some(VALID_ANON)).await;
    assert_eq!(status, 200);
}

#[sqlx::test(migrator = "MIGRATOR", fixtures("seed"))]
async fn search_400s_on_malformed_anon_header(pool: PgPool) {
    let (status, body) = send_with_optional_header(pool, Some("not-a-uuid")).await;
    assert_eq!(status, 400);
    let s = String::from_utf8_lossy(&body);
    assert!(
        s.contains("not a valid UUID"),
        "expected validation message, got: {s}"
    );
}

#[sqlx::test(migrator = "MIGRATOR", fixtures("seed"))]
async fn search_400s_on_non_utf8_anon_header(pool: PgPool) {
    // HeaderValue rejects non-visible-ASCII bytes outright; we test the
    // post-parse path with an empty string, which UUID parsing rejects.
    let (status, _) = send_with_optional_header(pool, Some("")).await;
    assert_eq!(status, 400);
}
