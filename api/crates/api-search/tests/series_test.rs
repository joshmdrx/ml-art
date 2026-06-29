//! T-058 — series CRUD + public reads + artwork PATCH integration.
//!
//! Eight scenarios cover the load-bearing behaviour:
//!   1. create + list
//!   2. slug collision → 409
//!   3. set_artworks (multi-select) — atomic replace semantics
//!   4. soft-delete clears membership
//!   5. public list filters empty series
//!   6. public detail returns 404 for non-existent / empty / hidden series
//!   7. public detail returns series + artist + artworks for a populated one
//!   8. artwork PATCH integrates with series_id

mod common;

use axum::{
    body::{to_bytes, Body},
    http::{Request, StatusCode},
    Router,
};
use common::{app_with_test_auth, MIGRATOR};
use serde_json::{json, Value};
use sqlx::PgPool;
use tower::ServiceExt;
use uuid::Uuid;

const ALICE_USER: &str = "test-user_test_alice";
const ALICE_ARTIST_ID: Uuid = Uuid::from_u128(0xaaa1_1111_1111_1111_1111_1111_1111_1111);
const ARTWORK_1: Uuid = Uuid::from_u128(0xbbb1_1111_1111_1111_1111_1111_1111_1111);
const ARTWORK_2: Uuid = Uuid::from_u128(0xbbb2_2222_2222_2222_2222_2222_2222_2222);

async fn send(
    app: Router,
    method: &str,
    uri: &str,
    bearer: Option<&str>,
    body: Option<Value>,
) -> (StatusCode, Value) {
    let mut req = Request::builder().method(method).uri(uri);
    if let Some(b) = bearer {
        req = req.header("Authorization", format!("Bearer {b}"));
    }
    let body_kind = if let Some(b) = &body {
        req = req.header("content-type", "application/json");
        Body::from(b.to_string())
    } else {
        Body::empty()
    };
    let resp = app
        .oneshot(req.body(body_kind).expect("build req"))
        .await
        .expect("oneshot");
    let status = resp.status();
    let bytes = to_bytes(resp.into_body(), 256 * 1024).await.unwrap();
    let parsed: Value = if bytes.is_empty() {
        Value::Null
    } else {
        serde_json::from_slice(&bytes).unwrap_or(Value::Null)
    };
    (status, parsed)
}

// ─────────────────────────────────────────────────────────────────────────────
// Studio CRUD
// ─────────────────────────────────────────────────────────────────────────────

#[sqlx::test(migrator = "MIGRATOR", fixtures("seed"))]
async fn create_and_list_series(pool: PgPool) {
    let app = app_with_test_auth(pool.clone());
    let (status, body) = send(
        app,
        "POST",
        "/v1/studio/series",
        Some(ALICE_USER),
        Some(json!({ "title": "Quiet Mornings", "statement": "A study in early light." })),
    )
    .await;
    assert_eq!(status, StatusCode::CREATED);
    assert_eq!(body["title"], "Quiet Mornings");
    assert_eq!(body["slug"], "quiet-mornings");
    assert_eq!(body["statement"], "A study in early light.");
    assert_eq!(body["artwork_count"], 0);

    let app = app_with_test_auth(pool);
    let (status, body) = send(app, "GET", "/v1/studio/series", Some(ALICE_USER), None).await;
    assert_eq!(status, StatusCode::OK);
    assert_eq!(body["items"].as_array().unwrap().len(), 1);
    assert_eq!(body["items"][0]["slug"], "quiet-mornings");
}

#[sqlx::test(migrator = "MIGRATOR", fixtures("seed"))]
async fn slug_collision_409(pool: PgPool) {
    let app = app_with_test_auth(pool.clone());
    let (status, _) = send(
        app,
        "POST",
        "/v1/studio/series",
        Some(ALICE_USER),
        Some(json!({ "title": "Blue Period" })),
    )
    .await;
    assert_eq!(status, StatusCode::CREATED);

    // Same title → same slug → 409.
    let app = app_with_test_auth(pool);
    let (status, _) = send(
        app,
        "POST",
        "/v1/studio/series",
        Some(ALICE_USER),
        Some(json!({ "title": "Blue Period" })),
    )
    .await;
    assert_eq!(status, StatusCode::CONFLICT);
}

#[sqlx::test(migrator = "MIGRATOR", fixtures("seed"))]
async fn set_artworks_replaces_membership(pool: PgPool) {
    // Create a series, then bulk-assign two artworks. Then bulk-replace
    // with a different set (one stays, one swaps in, one drops out).
    let app = app_with_test_auth(pool.clone());
    let (_, created) = send(
        app,
        "POST",
        "/v1/studio/series",
        Some(ALICE_USER),
        Some(json!({ "title": "Set One" })),
    )
    .await;
    let series_id = created["id"].as_str().unwrap().to_string();

    // First assignment: ARTWORK_1 + ARTWORK_2.
    let app = app_with_test_auth(pool.clone());
    let (status, ack) = send(
        app,
        "PUT",
        &format!("/v1/studio/series/{series_id}/artworks"),
        Some(ALICE_USER),
        Some(json!({ "artwork_ids": [ARTWORK_1, ARTWORK_2] })),
    )
    .await;
    assert_eq!(status, StatusCode::OK);
    assert_eq!(ack["added"], 2);
    assert_eq!(ack["removed"], 0);
    assert_eq!(ack["artwork_count"], 2);

    // Replacement: drop ARTWORK_2, add nothing new. ARTWORK_1 stays.
    let app = app_with_test_auth(pool.clone());
    let (status, ack) = send(
        app,
        "PUT",
        &format!("/v1/studio/series/{series_id}/artworks"),
        Some(ALICE_USER),
        Some(json!({ "artwork_ids": [ARTWORK_1] })),
    )
    .await;
    assert_eq!(status, StatusCode::OK);
    assert_eq!(ack["added"], 0);
    assert_eq!(ack["removed"], 1);
    assert_eq!(ack["artwork_count"], 1);

    // DB check: ARTWORK_2 has no series; ARTWORK_1 does.
    let r2: (Option<Uuid>,) = sqlx::query_as("SELECT series_id FROM artworks WHERE id = $1")
        .bind(ARTWORK_2)
        .fetch_one(&pool)
        .await
        .unwrap();
    assert!(
        r2.0.is_none(),
        "ARTWORK_2 should be un-series'd, got {:?}",
        r2.0
    );
    let r1: (Option<Uuid>,) = sqlx::query_as("SELECT series_id FROM artworks WHERE id = $1")
        .bind(ARTWORK_1)
        .fetch_one(&pool)
        .await
        .unwrap();
    assert!(r1.0.is_some());
}

#[sqlx::test(migrator = "MIGRATOR", fixtures("seed"))]
async fn soft_delete_clears_membership(pool: PgPool) {
    // Create + populate, then DELETE. The soft-delete tx clears
    // artworks.series_id so they stop pointing at a logically gone series.
    let app = app_with_test_auth(pool.clone());
    let (_, created) = send(
        app,
        "POST",
        "/v1/studio/series",
        Some(ALICE_USER),
        Some(json!({ "title": "Doomed" })),
    )
    .await;
    let series_id = created["id"].as_str().unwrap().to_string();

    let app = app_with_test_auth(pool.clone());
    send(
        app,
        "PUT",
        &format!("/v1/studio/series/{series_id}/artworks"),
        Some(ALICE_USER),
        Some(json!({ "artwork_ids": [ARTWORK_1] })),
    )
    .await;

    let app = app_with_test_auth(pool.clone());
    let (status, _) = send(
        app,
        "DELETE",
        &format!("/v1/studio/series/{series_id}"),
        Some(ALICE_USER),
        None,
    )
    .await;
    assert_eq!(status, StatusCode::NO_CONTENT);

    let r1: (Option<Uuid>,) = sqlx::query_as("SELECT series_id FROM artworks WHERE id = $1")
        .bind(ARTWORK_1)
        .fetch_one(&pool)
        .await
        .unwrap();
    assert!(r1.0.is_none(), "membership not cleared on soft-delete");
}

// ─────────────────────────────────────────────────────────────────────────────
// Public reads
// ─────────────────────────────────────────────────────────────────────────────

async fn create_populated_series(pool: &PgPool, title: &str, artworks: &[Uuid]) -> Uuid {
    // Skip the API; insert directly to keep these tests focused on the
    // public-read filter logic, not the studio CRUD again.
    let series_id: Uuid = sqlx::query_scalar(
        r#"INSERT INTO series (artist_id, slug, title)
           VALUES ($1, $2, $3) RETURNING id"#,
    )
    .bind(ALICE_ARTIST_ID)
    .bind(title.to_lowercase().replace(' ', "-"))
    .bind(title)
    .fetch_one(pool)
    .await
    .unwrap();
    for a in artworks {
        sqlx::query("UPDATE artworks SET series_id = $1 WHERE id = $2")
            .bind(series_id)
            .bind(a)
            .execute(pool)
            .await
            .unwrap();
    }
    series_id
}

#[sqlx::test(migrator = "MIGRATOR", fixtures("seed"))]
async fn public_list_hides_empty_series(pool: PgPool) {
    create_populated_series(&pool, "Filled", &[ARTWORK_1]).await;
    create_populated_series(&pool, "Empty", &[]).await;

    let app = app_with_test_auth(pool);
    let (status, body) = send(app, "GET", "/v1/artists/alice-test/series", None, None).await;
    assert_eq!(status, StatusCode::OK);
    let items = body["items"].as_array().unwrap();
    assert_eq!(items.len(), 1, "empty series should be hidden");
    assert_eq!(items[0]["slug"], "filled");
}

#[sqlx::test(migrator = "MIGRATOR", fixtures("seed"))]
async fn public_detail_returns_artist_plus_artworks(pool: PgPool) {
    create_populated_series(&pool, "On Display", &[ARTWORK_1, ARTWORK_2]).await;

    let app = app_with_test_auth(pool);
    let (status, body) = send(
        app,
        "GET",
        "/v1/artists/alice-test/series/on-display",
        None,
        None,
    )
    .await;
    assert_eq!(status, StatusCode::OK);
    assert_eq!(body["series"]["title"], "On Display");
    assert_eq!(body["artist"]["slug"], "alice-test");
    assert_eq!(body["artworks"]["items"].as_array().unwrap().len(), 2);
}

#[sqlx::test(migrator = "MIGRATOR", fixtures("seed"))]
async fn public_detail_404s_for_empty_or_missing(pool: PgPool) {
    create_populated_series(&pool, "Empty", &[]).await;

    // Empty series → 404
    let app = app_with_test_auth(pool.clone());
    let (status, _) = send(
        app,
        "GET",
        "/v1/artists/alice-test/series/empty",
        None,
        None,
    )
    .await;
    assert_eq!(status, StatusCode::NOT_FOUND);

    // Non-existent series → 404
    let app = app_with_test_auth(pool);
    let (status, _) = send(
        app,
        "GET",
        "/v1/artists/alice-test/series/never-existed",
        None,
        None,
    )
    .await;
    assert_eq!(status, StatusCode::NOT_FOUND);
}

// ─────────────────────────────────────────────────────────────────────────────
// Artwork PATCH integration
// ─────────────────────────────────────────────────────────────────────────────

#[sqlx::test(migrator = "MIGRATOR", fixtures("seed"))]
async fn artwork_patch_sets_and_clears_series(pool: PgPool) {
    // Create a series via the API so we have a valid id owned by alice.
    let app = app_with_test_auth(pool.clone());
    let (_, created) = send(
        app,
        "POST",
        "/v1/studio/series",
        Some(ALICE_USER),
        Some(json!({ "title": "Patch Target" })),
    )
    .await;
    let series_id = created["id"].as_str().unwrap().to_string();

    // Assign via artwork PATCH.
    let app = app_with_test_auth(pool.clone());
    let (status, _) = send(
        app,
        "PATCH",
        &format!("/v1/studio/artworks/{ARTWORK_1}"),
        Some(ALICE_USER),
        Some(json!({ "series_id": series_id })),
    )
    .await;
    assert_eq!(status, StatusCode::OK);
    let r: (Option<Uuid>,) = sqlx::query_as("SELECT series_id FROM artworks WHERE id = $1")
        .bind(ARTWORK_1)
        .fetch_one(&pool)
        .await
        .unwrap();
    assert_eq!(r.0.map(|u| u.to_string()), Some(series_id));

    // Clear via PATCH with null.
    let app = app_with_test_auth(pool.clone());
    let (status, _) = send(
        app,
        "PATCH",
        &format!("/v1/studio/artworks/{ARTWORK_1}"),
        Some(ALICE_USER),
        Some(json!({ "series_id": null })),
    )
    .await;
    assert_eq!(status, StatusCode::OK);
    let r: (Option<Uuid>,) = sqlx::query_as("SELECT series_id FROM artworks WHERE id = $1")
        .bind(ARTWORK_1)
        .fetch_one(&pool)
        .await
        .unwrap();
    assert!(r.0.is_none());

    // Bogus series id → 400.
    let bogus = Uuid::from_u128(0xdead_dead_dead_dead_dead_dead_dead_dead);
    let app = app_with_test_auth(pool);
    let (status, _) = send(
        app,
        "PATCH",
        &format!("/v1/studio/artworks/{ARTWORK_1}"),
        Some(ALICE_USER),
        Some(json!({ "series_id": bogus })),
    )
    .await;
    assert_eq!(status, StatusCode::BAD_REQUEST);
}
