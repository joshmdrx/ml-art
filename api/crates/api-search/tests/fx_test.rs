//! T-080 — FX rate maintenance + canonical-GBP price filter tests.
//!
//! `refresh_rates` itself isn't exercised here — it makes a real HTTP
//! call to Frankfurter and the test environment isn't networked.
//! `compute_price_gbp_cents` + the studio write maintenance + the
//! search SQL filter ARE exercised end-to-end against the real DB.

mod common;

use axum::{
    body::{to_bytes, Body},
    http::{Request, StatusCode},
};
use common::{app_keyword_only, app_with_test_auth, MIGRATOR};
use ml_art_core::fx;
use serde_json::{json, Value};
use sqlx::PgPool;
use tower::ServiceExt;
use uuid::Uuid;

const ALICE_USER: &str = "test-user_test_alice";

// ─────────────────────────────────────────────────────────────────────────────
// fx::compute_price_gbp_cents — point lookup used at write time
// ─────────────────────────────────────────────────────────────────────────────

#[sqlx::test(migrator = "MIGRATOR", fixtures("seed"))]
async fn compute_gbp_for_gbp_is_identity(pool: PgPool) {
    let r = fx::compute_price_gbp_cents(&pool, Some(50_000), "GBP")
        .await
        .unwrap();
    assert_eq!(r, Some(50_000), "GBP→GBP should pass through");
}

#[sqlx::test(migrator = "MIGRATOR", fixtures("seed"))]
async fn compute_gbp_for_usd_uses_seeded_rate(pool: PgPool) {
    // Migration seeds USD at 0.79 GBP per USD. So $500 → £395.
    let r = fx::compute_price_gbp_cents(&pool, Some(50_000), "USD")
        .await
        .unwrap();
    assert_eq!(r, Some(39_500));
}

#[sqlx::test(migrator = "MIGRATOR", fixtures("seed"))]
async fn compute_gbp_none_in_none_out(pool: PgPool) {
    let r = fx::compute_price_gbp_cents(&pool, None, "USD")
        .await
        .unwrap();
    assert!(r.is_none(), "POA prices have no GBP value");
}

#[sqlx::test(migrator = "MIGRATOR", fixtures("seed"))]
async fn compute_gbp_unknown_currency_returns_none(pool: PgPool) {
    // No fx_rates row → None (defensive; should be rare in practice).
    let r = fx::compute_price_gbp_cents(&pool, Some(50_000), "ZZZ")
        .await
        .unwrap();
    assert!(r.is_none());
}

// ─────────────────────────────────────────────────────────────────────────────
// Studio writes maintain price_gbp_cents
// ─────────────────────────────────────────────────────────────────────────────

async fn create_artwork(pool: PgPool, body: Value) -> (StatusCode, Value) {
    let app = app_with_test_auth(pool);
    let resp = app
        .oneshot(
            Request::builder()
                .method("POST")
                .uri("/v1/studio/artworks")
                .header("Authorization", format!("Bearer {ALICE_USER}"))
                .header("Content-Type", "application/json")
                .body(Body::from(body.to_string()))
                .unwrap(),
        )
        .await
        .unwrap();
    let status = resp.status();
    let bytes = to_bytes(resp.into_body(), 64 * 1024).await.unwrap();
    let parsed = serde_json::from_slice(&bytes).unwrap_or(Value::Null);
    (status, parsed)
}

#[sqlx::test(migrator = "MIGRATOR", fixtures("seed"))]
async fn create_artwork_sets_price_gbp_cents(pool: PgPool) {
    let (status, body) = create_artwork(
        pool.clone(),
        json!({ "title": "USD test", "price_cents": 50_000, "currency": "USD" }),
    )
    .await;
    assert_eq!(status, StatusCode::CREATED);
    let id = body["id"].as_str().unwrap();

    let row: (Option<i64>, Option<i64>) =
        sqlx::query_as("SELECT price_cents, price_gbp_cents FROM artworks WHERE id = $1::uuid")
            .bind(id)
            .fetch_one(&pool)
            .await
            .unwrap();
    assert_eq!(row.0, Some(50_000));
    assert_eq!(row.1, Some(39_500), "USD→GBP at seed rate 0.79");
}

#[sqlx::test(migrator = "MIGRATOR", fixtures("seed"))]
async fn create_with_no_price_leaves_gbp_null(pool: PgPool) {
    let (status, body) = create_artwork(
        pool.clone(),
        json!({ "title": "POA", "currency": "USD", "availability": "inquire" }),
    )
    .await;
    assert_eq!(status, StatusCode::CREATED);
    let id = body["id"].as_str().unwrap();
    let row: (Option<i64>,) =
        sqlx::query_as("SELECT price_gbp_cents FROM artworks WHERE id = $1::uuid")
            .bind(id)
            .fetch_one(&pool)
            .await
            .unwrap();
    assert!(row.0.is_none());
}

#[sqlx::test(migrator = "MIGRATOR", fixtures("seed"))]
async fn patch_price_recomputes_gbp(pool: PgPool) {
    // Create at $100, patch up to $1000. GBP should track.
    let (_, created) = create_artwork(
        pool.clone(),
        json!({ "title": "Patch target", "price_cents": 10_000, "currency": "USD" }),
    )
    .await;
    let id = created["id"].as_str().unwrap().to_string();

    let app = app_with_test_auth(pool.clone());
    let resp = app
        .oneshot(
            Request::builder()
                .method("PATCH")
                .uri(format!("/v1/studio/artworks/{id}"))
                .header("Authorization", format!("Bearer {ALICE_USER}"))
                .header("Content-Type", "application/json")
                .body(Body::from(json!({ "price_cents": 100_000 }).to_string()))
                .unwrap(),
        )
        .await
        .unwrap();
    assert_eq!(resp.status(), StatusCode::OK);

    let row: (Option<i64>, Option<i64>) =
        sqlx::query_as("SELECT price_cents, price_gbp_cents FROM artworks WHERE id = $1::uuid")
            .bind(&id)
            .fetch_one(&pool)
            .await
            .unwrap();
    assert_eq!(row.0, Some(100_000));
    assert_eq!(row.1, Some(79_000), "$1000 → £790");
}

#[sqlx::test(migrator = "MIGRATOR", fixtures("seed"))]
async fn patch_currency_only_recomputes_gbp(pool: PgPool) {
    // Currency change (no price change) should also re-derive GBP.
    // $500 → £395; switch to GBP → £500.
    let (_, created) = create_artwork(
        pool.clone(),
        json!({ "title": "Currency switch", "price_cents": 50_000, "currency": "USD" }),
    )
    .await;
    let id = created["id"].as_str().unwrap().to_string();
    let before: (Option<i64>,) =
        sqlx::query_as("SELECT price_gbp_cents FROM artworks WHERE id = $1::uuid")
            .bind(&id)
            .fetch_one(&pool)
            .await
            .unwrap();
    assert_eq!(before.0, Some(39_500));

    let app = app_with_test_auth(pool.clone());
    let resp = app
        .oneshot(
            Request::builder()
                .method("PATCH")
                .uri(format!("/v1/studio/artworks/{id}"))
                .header("Authorization", format!("Bearer {ALICE_USER}"))
                .header("Content-Type", "application/json")
                .body(Body::from(json!({ "currency": "GBP" }).to_string()))
                .unwrap(),
        )
        .await
        .unwrap();
    assert_eq!(resp.status(), StatusCode::OK);
    let after: (Option<i64>,) =
        sqlx::query_as("SELECT price_gbp_cents FROM artworks WHERE id = $1::uuid")
            .bind(&id)
            .fetch_one(&pool)
            .await
            .unwrap();
    assert_eq!(after.0, Some(50_000), "GBP→GBP after currency switch");
}

// ─────────────────────────────────────────────────────────────────────────────
// Search filter uses price_gbp_cents
// ─────────────────────────────────────────────────────────────────────────────

#[sqlx::test(migrator = "MIGRATOR", fixtures("seed"))]
async fn price_filter_compares_in_gbp_across_currencies(pool: PgPool) {
    // Build a Blue Morning artwork priced at $500 (USD → £395).
    // Bob has Crimson Field at $2500 (USD → £1975). Search with
    // price_max=£500 should return the cheap one ONLY — proving
    // we're filtering in GBP, not raw cents.
    sqlx::query("UPDATE artworks SET currency = 'USD', price_cents = 50000, price_gbp_cents = 39500 WHERE id::text = $1")
        .bind("bbb11111-1111-1111-1111-111111111111")
        .execute(&pool)
        .await
        .unwrap();
    sqlx::query("UPDATE artworks SET currency = 'USD', price_cents = 250000, price_gbp_cents = 197500 WHERE id::text = $1")
        .bind("bbb22222-2222-2222-2222-222222222222")
        .execute(&pool)
        .await
        .unwrap();

    let app = app_keyword_only(pool);
    // £500 = 50000 minor units.
    let resp = app
        .oneshot(
            Request::builder()
                .uri("/v1/search?price_max=50000")
                .body(Body::empty())
                .unwrap(),
        )
        .await
        .unwrap();
    assert_eq!(resp.status(), StatusCode::OK);
    let bytes = to_bytes(resp.into_body(), 256 * 1024).await.unwrap();
    let body: Value = serde_json::from_slice(&bytes).unwrap();
    let ids: Vec<&str> = body["items"]
        .as_array()
        .unwrap()
        .iter()
        .map(|a| a["id"].as_str().unwrap())
        .collect();
    let blue = "bbb11111-1111-1111-1111-111111111111";
    let crimson = "bbb22222-2222-2222-2222-222222222222";
    assert!(
        ids.contains(&blue),
        "Blue Morning (£395) should match price_max=£500: {:?}",
        ids
    );
    assert!(
        !ids.contains(&crimson),
        "Crimson Field (£1975) should NOT match price_max=£500: {:?}",
        ids
    );
}

#[sqlx::test(migrator = "MIGRATOR", fixtures("seed"))]
async fn price_filter_excludes_null_gbp(pool: PgPool) {
    // An artwork with NULL price_gbp_cents (POA / untracked currency)
    // must NOT match any price-bounded filter.
    let id = Uuid::new_v4();
    sqlx::query(
        r#"INSERT INTO artworks (id, artist_id, title, currency, availability, status, price_cents, price_gbp_cents)
           VALUES ($1, 'aaa11111-1111-1111-1111-111111111111'::uuid, 'No GBP', 'USD', 'inquire', 'published', NULL, NULL)"#,
    )
    .bind(id)
    .execute(&pool)
    .await
    .unwrap();

    let app = app_keyword_only(pool);
    let resp = app
        .oneshot(
            Request::builder()
                .uri("/v1/search?price_max=1000000")
                .body(Body::empty())
                .unwrap(),
        )
        .await
        .unwrap();
    let bytes = to_bytes(resp.into_body(), 256 * 1024).await.unwrap();
    let body: Value = serde_json::from_slice(&bytes).unwrap();
    let ids: Vec<&str> = body["items"]
        .as_array()
        .unwrap()
        .iter()
        .map(|a| a["id"].as_str().unwrap())
        .collect();
    assert!(
        !ids.iter().any(|i| *i == id.to_string()),
        "NULL GBP artwork leaked into price-bounded filter: {:?}",
        ids
    );
}
