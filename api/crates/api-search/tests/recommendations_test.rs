//! T-056.1 integration tests — `/v1/me/recommendations/for-you`.
//!
//! Uses the seed fixture's 5 artworks with one-hot embeddings at
//! distinct positions, so a hand-set taste vector at position N
//! makes the artwork at position N the nearest neighbour.
//!
//! Each test seeds the user_profiles row directly rather than going
//! through the full event-stream → T-055 refresh path. The taste-
//! vector pipeline has its own tests in `user_profile_test.rs`; here
//! we're verifying the retrieval surface in isolation.

mod common;

use api_search::recommendations::{compute_for_you, ForYouResponse};
use axum::{
    body::{to_bytes, Body},
    http::{Request, StatusCode},
};
use common::{app_with_test_auth, MIGRATOR};
use sqlx::PgPool;
use tower::ServiceExt;
use uuid::Uuid;

const ALICE: Uuid = Uuid::from_u128(0x8888_8888_8888_8888_8888_8888_8888_8888);
const ARTWORK_POS_2: Uuid = Uuid::from_u128(0xbbb3_3333_3333_3333_3333_3333_3333_3333);

/// Build a 1024-d taste vector with `1.0` at the given index — the
/// shape mirrors the seed fixture's artwork embeddings so we can
/// predict the nearest-neighbour result.
fn one_hot_taste_literal(pos: usize) -> String {
    let parts: Vec<&str> = (0..1024)
        .map(|i| if i == pos { "1" } else { "0" })
        .collect();
    format!("[{}]", parts.join(","))
}

async fn seed_profile(pool: &PgPool, user_id: Uuid, interaction_count: i32, taste_pos: usize) {
    sqlx::query(
        r#"
        INSERT INTO user_profiles (
            user_id, taste_embedding, interaction_count,
            last_active, profile_updated_at
        ) VALUES ($1, $2::vector, $3, now(), now())
        ON CONFLICT (user_id) DO UPDATE SET
            taste_embedding = EXCLUDED.taste_embedding,
            interaction_count = EXCLUDED.interaction_count,
            profile_updated_at = now()
        "#,
    )
    .bind(user_id)
    .bind(one_hot_taste_literal(taste_pos))
    .bind(interaction_count)
    .execute(pool)
    .await
    .unwrap();
}

fn state_from_pool(pool: PgPool) -> std::sync::Arc<api_search::AppState> {
    use ml_art_core::{
        auth::JwtVerifier, config::Config, embedder::Embedder, jobs::JobsBackend,
        object_store::ObjectStore,
    };
    std::sync::Arc::new(api_search::AppState {
        pool: pool.clone(),
        embedder: Embedder::disabled(pool),
        jwt_verifier: JwtVerifier::for_tests(),
        cfg: Config::for_tests(String::new()),
        object_store: ObjectStore::for_tests("uploads"),
        jobs: JobsBackend::for_tests(),
    })
}

// ─────────────────────────────────────────────────────────────────────────────
// compute_for_you (the pure data path)
// ─────────────────────────────────────────────────────────────────────────────

#[sqlx::test(migrator = "MIGRATOR", fixtures("seed"))]
async fn empty_when_no_profile_row(pool: PgPool) {
    let state = state_from_pool(pool);
    let items = compute_for_you(&state, ALICE).await.unwrap();
    assert!(items.is_empty(), "no profile row → empty items");
}

#[sqlx::test(migrator = "MIGRATOR", fixtures("seed"))]
async fn empty_when_interaction_count_below_threshold(pool: PgPool) {
    // Profile row exists but interaction_count is below the gate.
    // Personalisation must skip — emit nothing.
    seed_profile(&pool, ALICE, 4, 2).await;
    let state = state_from_pool(pool);
    let items = compute_for_you(&state, ALICE).await.unwrap();
    assert!(items.is_empty(), "below threshold → empty items");
}

#[sqlx::test(migrator = "MIGRATOR", fixtures("seed"))]
async fn empty_when_taste_embedding_is_null(pool: PgPool) {
    // Manually inserted profile with NULL taste_embedding (the
    // sub-noise-floor T-055 path). High interaction_count alone
    // shouldn't unlock personalisation if there's no vector.
    sqlx::query(
        r#"
        INSERT INTO user_profiles (
            user_id, taste_embedding, interaction_count,
            last_active, profile_updated_at
        ) VALUES ($1, NULL, 50, now(), now())
        "#,
    )
    .bind(ALICE)
    .execute(&pool)
    .await
    .unwrap();
    let state = state_from_pool(pool);
    let items = compute_for_you(&state, ALICE).await.unwrap();
    assert!(items.is_empty(), "null vector → empty items");
}

#[sqlx::test(migrator = "MIGRATOR", fixtures("seed"))]
async fn returns_items_when_eligible(pool: PgPool) {
    // Taste vector at position 2 — the closest seed artwork is
    // ARTWORK_POS_2 at pos 2 (cosine distance 0). interaction_count
    // = 5 is exactly the threshold (calibrator-completed case).
    seed_profile(&pool, ALICE, 5, 2).await;
    let state = state_from_pool(pool);
    let items = compute_for_you(&state, ALICE).await.unwrap();
    assert!(!items.is_empty(), "eligible user should get items");
    // Only 5 artworks in the fixture; with candidate pool of 50 and
    // LIMIT 12 the result should include all 5.
    assert!(items.len() <= 5, "fixture only has 5 artworks");
    // ARTWORK_POS_2 must be among the returned (it's the literal
    // nearest neighbour; random shuffle of top-50 can't drop it
    // because the entire fixture fits inside the top-50).
    let ids: Vec<Uuid> = items.iter().map(|a| a.id).collect();
    assert!(
        ids.contains(&ARTWORK_POS_2),
        "nearest artwork missing from result: {:?}",
        ids
    );
}

#[sqlx::test(migrator = "MIGRATOR", fixtures("seed"))]
async fn excludes_drafts_and_deleted(pool: PgPool) {
    // The fixture has one `status='draft'` artwork (bbb6...). It must
    // never appear in the for-you results. We don't even need to set
    // up the taste vector specifically; any eligible user shouldn't
    // see a draft.
    seed_profile(&pool, ALICE, 5, 5).await;
    let state = state_from_pool(pool);
    let items = compute_for_you(&state, ALICE).await.unwrap();
    let draft_id = Uuid::from_u128(0xbbb6_6666_6666_6666_6666_6666_6666_6666);
    assert!(
        !items.iter().any(|a| a.id == draft_id),
        "draft artwork leaked into for-you"
    );
}

// ─────────────────────────────────────────────────────────────────────────────
// /v1/me/recommendations/for-you (the axum handler)
// ─────────────────────────────────────────────────────────────────────────────

#[sqlx::test(migrator = "MIGRATOR", fixtures("seed"))]
async fn endpoint_requires_auth(pool: PgPool) {
    // No Authorization header → 401. Confirms the AuthedUser extractor
    // is in the route signature, not a footgun where anonymous callers
    // get an empty list.
    let app = app_with_test_auth(pool);
    let resp = app
        .oneshot(
            Request::builder()
                .uri("/v1/me/recommendations/for-you")
                .body(Body::empty())
                .unwrap(),
        )
        .await
        .unwrap();
    assert_eq!(resp.status(), StatusCode::UNAUTHORIZED);
}

#[sqlx::test(migrator = "MIGRATOR", fixtures("seed"))]
async fn endpoint_returns_not_eligible_for_new_user(pool: PgPool) {
    // Signed in but no user_profiles row yet. Should be 200 with
    // `eligible: false, items: []` so the web layer can fall back to
    // a default row without inferring from an empty array alone.
    let app = app_with_test_auth(pool);
    let resp = app
        .oneshot(
            Request::builder()
                .uri("/v1/me/recommendations/for-you")
                .header("Authorization", "Bearer test-user_test_alice")
                .body(Body::empty())
                .unwrap(),
        )
        .await
        .unwrap();
    assert_eq!(resp.status(), StatusCode::OK);
    let bytes = to_bytes(resp.into_body(), 64 * 1024).await.unwrap();
    let body: ForYouResponse = serde_json::from_slice(&bytes).unwrap();
    assert!(!body.eligible);
    assert!(body.items.is_empty());
}

#[sqlx::test(migrator = "MIGRATOR", fixtures("seed"))]
async fn endpoint_returns_eligible_with_items(pool: PgPool) {
    // Seed alice's profile, then fetch via the handler. eligible=true
    // and items should be non-empty.
    seed_profile(&pool, ALICE, 5, 2).await;
    let app = app_with_test_auth(pool);
    let resp = app
        .oneshot(
            Request::builder()
                .uri("/v1/me/recommendations/for-you")
                .header("Authorization", "Bearer test-user_test_alice")
                .body(Body::empty())
                .unwrap(),
        )
        .await
        .unwrap();
    assert_eq!(resp.status(), StatusCode::OK);
    let bytes = to_bytes(resp.into_body(), 64 * 1024).await.unwrap();
    let body: ForYouResponse = serde_json::from_slice(&bytes).unwrap();
    assert!(body.eligible);
    assert!(!body.items.is_empty());
}
