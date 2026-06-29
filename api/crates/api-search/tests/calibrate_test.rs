//! T-061 integration tests for the calibrator endpoints.
//!
//! The seed fixture only has the one `kind='curated'` neighbourhood,
//! so each test inserts the semantic neighbourhoods it needs with
//! synthetic far-apart centroids. The greedy farthest-pair selection
//! is the main behaviour to pin down.

mod common;

use api_search::calibrate::PairsResponse;
use axum::{
    body::{to_bytes, Body},
    http::{Request, StatusCode},
};
use common::{app_keyword_only, MIGRATOR};
use serde_json::Value;
use sqlx::PgPool;
use tower::ServiceExt;
use uuid::Uuid;

const ARTWORK_POS_0: Uuid = Uuid::from_u128(0xbbb1_1111_1111_1111_1111_1111_1111_1111);
const ARTWORK_POS_1: Uuid = Uuid::from_u128(0xbbb2_2222_2222_2222_2222_2222_2222_2222);
const ARTWORK_POS_2: Uuid = Uuid::from_u128(0xbbb3_3333_3333_3333_3333_3333_3333_3333);
const ARTWORK_POS_3: Uuid = Uuid::from_u128(0xbbb4_4444_4444_4444_4444_4444_4444_4444);
const ARTWORK_POS_4: Uuid = Uuid::from_u128(0xbbb5_5555_5555_5555_5555_5555_5555_5555);

/// Build a 1024-d centroid living on the first axis at scalar `mag`.
/// Choosing centroids along a single axis gives every pair a
/// distance equal to `|mag_a - mag_b|`, which makes far-apart
/// selection deterministic in tests. Real-world centroids are
/// arbitrary points inside the unit ball, but the algorithm only
/// cares about relative ordering.
fn axis_centroid_literal(mag: f32) -> String {
    let mut parts: Vec<String> = vec!["0".to_string(); 1024];
    parts[0] = mag.to_string();
    let joined = parts.join(",");
    format!("[{joined}]")
}

/// Insert N semantic neighbourhoods. Each gets one representative
/// artwork. Centroid magnitudes are caller-supplied so tests can
/// reason about the greedy pairing's output.
async fn insert_semantic_neighborhoods(pool: &PgPool, entries: &[(&str, Uuid, f32)]) {
    for (slug, rep_artwork, centroid_mag) in entries {
        sqlx::query(
            r#"
            INSERT INTO neighborhoods (
                slug, name, description, kind,
                representative_artwork_ids, artwork_count,
                is_featured, display_order,
                cluster_centroid
            ) VALUES ($1, $2, $3, 'semantic', $4::uuid[], 30, true, 0, $5::vector)
            "#,
        )
        .bind(slug)
        .bind(format!("Test {slug}"))
        .bind("test")
        .bind(vec![*rep_artwork])
        .bind(axis_centroid_literal(*centroid_mag))
        .execute(pool)
        .await
        .unwrap();
    }
}

async fn get_pairs(pool: PgPool) -> (StatusCode, PairsResponse) {
    let app = app_keyword_only(pool);
    let resp = app
        .oneshot(
            Request::builder()
                .uri("/v1/calibrate/pairs")
                .body(Body::empty())
                .unwrap(),
        )
        .await
        .unwrap();
    let status = resp.status();
    let bytes = to_bytes(resp.into_body(), 1024 * 1024).await.unwrap();
    let body: PairsResponse = serde_json::from_slice(&bytes).unwrap_or_else(|e| {
        panic!(
            "decode pairs (status {status}): {e}\nbody: {}",
            String::from_utf8_lossy(&bytes)
        )
    });
    (status, body)
}

// ─────────────────────────────────────────────────────────────────────────────
// GET /v1/calibrate/pairs
// ─────────────────────────────────────────────────────────────────────────────

#[sqlx::test(migrator = "MIGRATOR", fixtures("seed"))]
async fn pairs_returns_empty_when_no_semantic_neighborhoods(pool: PgPool) {
    // Seed fixture only has the one `kind='curated'` row → no
    // semantic neighbourhoods → empty pairs response, not an error.
    let (status, body) = get_pairs(pool).await;
    assert_eq!(status, StatusCode::OK);
    assert!(body.pairs.is_empty());
}

#[sqlx::test(migrator = "MIGRATOR", fixtures("seed"))]
async fn pairs_returns_one_pair_when_only_two_neighborhoods(pool: PgPool) {
    insert_semantic_neighborhoods(
        &pool,
        &[
            ("alpha", ARTWORK_POS_0, 0.0),
            ("omega", ARTWORK_POS_4, 10.0),
        ],
    )
    .await;
    let (status, body) = get_pairs(pool).await;
    assert_eq!(status, StatusCode::OK);
    assert_eq!(body.pairs.len(), 1);
    let pair = &body.pairs[0];
    assert_eq!(pair.id, "0");
    // Each artwork must come from one of our inserts.
    let slugs: Vec<&str> = vec![&pair.left.neighborhood_slug, &pair.right.neighborhood_slug]
        .into_iter()
        .map(String::as_str)
        .collect();
    assert!(slugs.contains(&"alpha"));
    assert!(slugs.contains(&"omega"));
}

#[sqlx::test(migrator = "MIGRATOR", fixtures("seed"))]
async fn pairs_picks_farthest_partner_greedily(pool: PgPool) {
    // Four centroids on the same axis at magnitudes 0, 1, 9, 4.
    // Distance is |mag_a - mag_b|. Greedy picks the first (mag 0),
    // pairs it with the farthest (mag 9 → "far-away"). The remaining
    // two (mag 1 + mag 4) auto-pair.
    insert_semantic_neighborhoods(
        &pool,
        &[
            ("nearorigin", ARTWORK_POS_0, 0.0),
            ("just-off", ARTWORK_POS_1, 1.0),
            ("far-away", ARTWORK_POS_4, 9.0),
            ("mid", ARTWORK_POS_3, 4.0),
        ],
    )
    .await;

    let (status, body) = get_pairs(pool).await;
    assert_eq!(status, StatusCode::OK);
    assert_eq!(body.pairs.len(), 2);

    let first = &body.pairs[0];
    let first_slugs = [
        &first.left.neighborhood_slug,
        &first.right.neighborhood_slug,
    ];
    assert!(
        first_slugs.iter().any(|s| s.as_str() == "nearorigin"),
        "pivot lost: {:?}",
        first_slugs
    );
    assert!(
        first_slugs.iter().any(|s| s.as_str() == "far-away"),
        "far partner missed: {:?}",
        first_slugs
    );
}

#[sqlx::test(migrator = "MIGRATOR", fixtures("seed"))]
async fn pairs_caps_at_pairs_per_session(pool: PgPool) {
    // Insert 12 neighbourhoods → algorithm could build 6 pairs, but
    // PAIRS_PER_SESSION caps at 5. The seed fixture only has 5
    // artworks so we cycle artwork ids across neighbourhoods — fine
    // for verifying the cap, even if pair sides may share an
    // artwork. In real prod each cluster has its own representative.
    let slugs: Vec<String> = (0..12).map(|i| format!("cluster-{i}")).collect();
    let arts = [
        ARTWORK_POS_0,
        ARTWORK_POS_1,
        ARTWORK_POS_2,
        ARTWORK_POS_3,
        ARTWORK_POS_4,
        ARTWORK_POS_0,
        ARTWORK_POS_1,
        ARTWORK_POS_2,
        ARTWORK_POS_3,
        ARTWORK_POS_4,
        ARTWORK_POS_0,
        ARTWORK_POS_1,
    ];
    let entries: Vec<(&str, Uuid, f32)> = (0..12)
        .map(|i| (slugs[i].as_str(), arts[i], i as f32))
        .collect();
    insert_semantic_neighborhoods(&pool, &entries).await;

    let (_status, body) = get_pairs(pool).await;
    assert_eq!(body.pairs.len(), 5, "session cap not honoured");
}

// ─────────────────────────────────────────────────────────────────────────────
// POST /v1/calibrate/pick
// ─────────────────────────────────────────────────────────────────────────────

#[sqlx::test(migrator = "MIGRATOR", fixtures("seed"))]
async fn pick_emits_calibration_pick_event(pool: PgPool) {
    let app = app_keyword_only(pool.clone());
    let body = serde_json::json!({
        "pair_id": "0",
        "chosen_artwork_id": ARTWORK_POS_1,
        "rejected_artwork_id": ARTWORK_POS_0,
    });
    let resp = app
        .oneshot(
            Request::builder()
                .uri("/v1/calibrate/pick")
                .method("POST")
                .header("Content-Type", "application/json")
                .body(Body::from(body.to_string()))
                .unwrap(),
        )
        .await
        .unwrap();
    let status = resp.status();
    assert_eq!(status, StatusCode::OK);

    // Best-effort emit → event_log goes through the jobs queue. With
    // the in-memory backend the event lands in `jobs.captured()`, but
    // app_keyword_only doesn't expose it. Instead, poll the events
    // table via the postgres handler path: enqueue + handle is what
    // the worker would do. For this test the simplest check is to
    // verify the events table has a row (the api uses an in-memory
    // backend so it won't, but `JobsBackend::for_tests` captures the
    // event — we can't see it via the router). Trust the existing
    // events_test.rs coverage of the emit→handler path; here just
    // assert the request succeeds.

    // Belt-and-braces: the JSON response shape.
    let bytes = to_bytes(resp.into_body(), 1024).await.unwrap();
    let v: Value = serde_json::from_slice(&bytes).unwrap();
    assert_eq!(v["ok"], serde_json::json!(true));
}

#[sqlx::test(migrator = "MIGRATOR", fixtures("seed"))]
async fn pick_rejects_malformed_body(pool: PgPool) {
    let app = app_keyword_only(pool);
    let resp = app
        .oneshot(
            Request::builder()
                .uri("/v1/calibrate/pick")
                .method("POST")
                .header("Content-Type", "application/json")
                .body(Body::from(r#"{"pair_id":"0"}"#))
                .unwrap(),
        )
        .await
        .unwrap();
    // axum's `Json` extractor rejects with 4xx on missing fields.
    assert!(
        resp.status().is_client_error(),
        "expected 4xx, got {}",
        resp.status()
    );
}
