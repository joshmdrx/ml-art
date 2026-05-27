mod common;

use axum::{
    body::{to_bytes, Body},
    http::{Request, StatusCode},
    Router,
};
use common::{app_keyword_only, app_with_rate_limit, MIGRATOR};
use serde_json::{json, Value};
use sqlx::PgPool;
use tower::ServiceExt;

const ARTWORK_BLUE_MORNING: &str = "bbb11111-1111-1111-1111-111111111111";

/// Hit `/v1/search` once with a given anonymous id (defaults to the same
/// UUID across calls so they share a bucket). Returns the full response.
async fn search_once(app: Router, anon: &str) -> axum::http::Response<Body> {
    app.oneshot(
        Request::builder()
            .uri("/v1/search?limit=1")
            .header("X-Anonymous-Id", anon)
            .body(Body::empty())
            .unwrap(),
    )
    .await
    .expect("router oneshot")
}

/// Submit an inquiry once. Anonymous (no Bearer token), small valid body.
async fn submit_inquiry(app: Router, anon: &str) -> axum::http::Response<Body> {
    let body = json!({
        "name":    "Stranger",
        "email":   "stranger@example.com",
        "message": "Is this still available?"
    })
    .to_string();
    app.oneshot(
        Request::builder()
            .method("POST")
            .uri(format!("/v1/artworks/{ARTWORK_BLUE_MORNING}/inquiries"))
            .header("X-Anonymous-Id", anon)
            .header("Content-Type", "application/json")
            .body(Body::from(body))
            .unwrap(),
    )
    .await
    .expect("router oneshot")
}

const ANON_A: &str = "01911234-aabb-7ccd-8eef-000000000001";
const ANON_B: &str = "01911234-aabb-7ccd-8eef-000000000002";

// ─────────────────────────────────────────────────────────────────────────────
// Bypass
// ─────────────────────────────────────────────────────────────────────────────

#[sqlx::test(migrator = "MIGRATOR", fixtures("seed"))]
async fn rate_limit_disabled_by_default_in_tests(pool: PgPool) {
    // `app_keyword_only` uses `Config::for_tests` which has the bypass on.
    // Hammer past any reasonable burst — every request should be 200.
    for _ in 0..30 {
        let resp = search_once(app_keyword_only(pool.clone()), ANON_A).await;
        assert_eq!(resp.status(), StatusCode::OK);
    }
}

// ─────────────────────────────────────────────────────────────────────────────
// Search policy
// ─────────────────────────────────────────────────────────────────────────────

#[sqlx::test(migrator = "MIGRATOR", fixtures("seed"))]
async fn search_burst_then_429(pool: PgPool) {
    // Tight burst of 3 succeeds; the 4th must be denied with a Retry-After
    // header and an `application/problem+json` body. Build *one* router so
    // requests share the same in-process limiter state.
    let app = app_with_rate_limit(pool, /* search/min */ 3, /* inquiry/hr */ 100);
    for i in 0..3 {
        let resp = search_once(app.clone(), ANON_A).await;
        assert_eq!(
            resp.status(),
            StatusCode::OK,
            "request {i} within burst should pass"
        );
    }
    let denied = search_once(app, ANON_A).await;
    assert_eq!(denied.status(), StatusCode::TOO_MANY_REQUESTS);

    // Header + content-type checks.
    let retry_after = denied
        .headers()
        .get(axum::http::header::RETRY_AFTER)
        .expect("Retry-After header must be set on 429");
    let secs: u64 = retry_after
        .to_str()
        .unwrap()
        .parse()
        .expect("Retry-After is integer seconds");
    assert!(secs >= 1, "Retry-After should be ≥1, got {secs}");

    let ct = denied
        .headers()
        .get(axum::http::header::CONTENT_TYPE)
        .expect("Content-Type must be set");
    assert_eq!(ct, "application/problem+json");

    let bytes = to_bytes(denied.into_body(), 4 * 1024).await.unwrap();
    let body: Value = serde_json::from_slice(&bytes).unwrap();
    assert_eq!(body["status"], 429);
    assert_eq!(body["title"], "Too Many Requests");
}

#[sqlx::test(migrator = "MIGRATOR", fixtures("seed"))]
async fn search_buckets_are_per_key(pool: PgPool) {
    // Quota of 2 per minute. Burn anon A's bucket; anon B's must still pass.
    let app = app_with_rate_limit(pool, 2, 100);
    assert_eq!(search_once(app.clone(), ANON_A).await.status(), 200);
    assert_eq!(search_once(app.clone(), ANON_A).await.status(), 200);
    assert_eq!(
        search_once(app.clone(), ANON_A).await.status(),
        StatusCode::TOO_MANY_REQUESTS
    );
    // Different anon id, fresh bucket.
    assert_eq!(search_once(app.clone(), ANON_B).await.status(), 200);
    assert_eq!(search_once(app, ANON_B).await.status(), 200);
}

// ─────────────────────────────────────────────────────────────────────────────
// Inquiry policy
// ─────────────────────────────────────────────────────────────────────────────

#[sqlx::test(migrator = "MIGRATOR", fixtures("seed"))]
async fn inquiry_burst_then_429(pool: PgPool) {
    // Default v1 budget: 3/hr per key. Fourth submission denied.
    let app = app_with_rate_limit(pool, 100, 3);

    for i in 0..3 {
        let resp = submit_inquiry(app.clone(), ANON_A).await;
        // 201 (created — pending verification) is the happy path.
        let status = resp.status();
        assert!(
            status == StatusCode::CREATED || status == StatusCode::OK,
            "submission {i} expected 2xx, got {status}"
        );
    }
    let denied = submit_inquiry(app, ANON_A).await;
    assert_eq!(denied.status(), StatusCode::TOO_MANY_REQUESTS);
    assert!(
        denied
            .headers()
            .get(axum::http::header::RETRY_AFTER)
            .is_some(),
        "Retry-After missing on inquiry 429"
    );
}

#[sqlx::test(migrator = "MIGRATOR", fixtures("seed"))]
async fn search_and_inquiry_have_separate_buckets(pool: PgPool) {
    // Burning the search bucket must not affect the inquiry bucket for
    // the same key, and vice versa.
    let app = app_with_rate_limit(pool, 1, 1);

    // Drain search for ANON_A.
    assert_eq!(search_once(app.clone(), ANON_A).await.status(), 200);
    assert_eq!(
        search_once(app.clone(), ANON_A).await.status(),
        StatusCode::TOO_MANY_REQUESTS
    );

    // Inquiry bucket untouched → first inquiry succeeds.
    let first = submit_inquiry(app.clone(), ANON_A).await;
    assert!(
        first.status().is_success(),
        "first inquiry should pass; got {}",
        first.status()
    );
    // Second inquiry → 429.
    let denied = submit_inquiry(app, ANON_A).await;
    assert_eq!(denied.status(), StatusCode::TOO_MANY_REQUESTS);
}
