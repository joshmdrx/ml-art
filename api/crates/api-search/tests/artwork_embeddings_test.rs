// `Deserialize`-only struct fields trigger dead_code under `-D warnings`.
// See `decisions.md` 2026-05-27 — Pre-commit hooks.
#![allow(dead_code)]

mod common;

use common::{app_with_fixed_vector, embedder_with_fixed_vector, get_json, MIGRATOR};
use ml_art_core::artwork_embeddings;
use pgvector::Vector;
use serde::Deserialize;
use sqlx::PgPool;
use uuid::Uuid;

/// Build a unit vector with a 1.0 at `pos`. Same trick the fixture seed
/// uses; gives us deterministic vectors that round-trip cleanly through
/// pgvector's cosine distance.
fn unit_vector_at(pos: usize) -> Vector {
    let mut v = vec![0.0_f32; 1024];
    v[pos] = 1.0;
    Vector::from(v)
}

const ARTWORK_BLUE_MORNING: &str = "bbb11111-1111-1111-1111-111111111111";

// ─────────────────────────────────────────────────────────────────────────────
// `write` — direct DB round-trip
// ─────────────────────────────────────────────────────────────────────────────

#[sqlx::test(migrator = "MIGRATOR", fixtures("seed"))]
async fn write_round_trips_through_pgvector(pool: PgPool) {
    let id: Uuid = ARTWORK_BLUE_MORNING.parse().unwrap();
    let vec = unit_vector_at(42);

    // The seed already populated this artwork at model_version='v2', so
    // calling write again must upsert (PK collision on the composite key).
    artwork_embeddings::write(&pool, id, "jinaai/jina-clip-v2", "v2", &vec)
        .await
        .expect("write");

    // Confirm we can read it back as a Vector.
    let (read,): (Vector,) = sqlx::query_as(
        r#"SELECT embedding FROM artwork_embeddings
           WHERE artwork_id = $1 AND model_name = $2 AND model_version = $3"#,
    )
    .bind(id)
    .bind("jinaai/jina-clip-v2")
    .bind("v2")
    .fetch_one(&pool)
    .await
    .expect("read");

    // Exact byte equality — pgvector should preserve f32 components.
    assert_eq!(read.as_slice(), vec.as_slice());
}

#[sqlx::test(migrator = "MIGRATOR", fixtures("seed"))]
async fn write_is_idempotent_under_same_pk(pool: PgPool) {
    let id: Uuid = ARTWORK_BLUE_MORNING.parse().unwrap();

    artwork_embeddings::write(&pool, id, "jinaai/jina-clip-v2", "v2", &unit_vector_at(7))
        .await
        .expect("first write");

    artwork_embeddings::write(&pool, id, "jinaai/jina-clip-v2", "v2", &unit_vector_at(7))
        .await
        .expect("second write is upsert");

    // Still exactly one row per (artwork, model, version).
    let (n,): (i64,) = sqlx::query_as(
        r#"SELECT count(*) FROM artwork_embeddings
           WHERE artwork_id = $1 AND model_name = $2 AND model_version = $3"#,
    )
    .bind(id)
    .bind("jinaai/jina-clip-v2")
    .bind("v2")
    .fetch_one(&pool)
    .await
    .unwrap();
    assert_eq!(n, 1);
}

#[sqlx::test(migrator = "MIGRATOR", fixtures("seed"))]
async fn write_with_different_version_creates_second_row(pool: PgPool) {
    // The PK is (artwork_id, model_name, model_version), so writing the
    // same artwork at a *new* model_version adds a row alongside the
    // existing one — gives us safe A/B + rollout semantics when we
    // eventually bump the model.
    let id: Uuid = ARTWORK_BLUE_MORNING.parse().unwrap();
    artwork_embeddings::write(
        &pool,
        id,
        "jinaai/jina-clip-v2",
        "v3-future",
        &unit_vector_at(99),
    )
    .await
    .expect("write");

    let (n,): (i64,) =
        sqlx::query_as(r#"SELECT count(*) FROM artwork_embeddings WHERE artwork_id = $1"#)
            .bind(id)
            .fetch_one(&pool)
            .await
            .unwrap();
    assert_eq!(n, 2, "v2 (from seed) + v3-future");
}

// ─────────────────────────────────────────────────────────────────────────────
// `process_image` — embed + write composed
// ─────────────────────────────────────────────────────────────────────────────

#[sqlx::test(migrator = "MIGRATOR", fixtures("seed"))]
async fn process_image_writes_a_row_with_v2_label(pool: PgPool) {
    // Create a fresh artwork the seed didn't pre-embed. This is the
    // shape studio create will hit: artwork row exists, no embedding yet.
    let artwork_id = Uuid::new_v4();
    sqlx::query(
        r#"
        INSERT INTO artworks (id, artist_id, title, status, published_at)
        VALUES ($1, $2, 'Process Test', 'published', now())
        "#,
    )
    .bind(artwork_id)
    .bind(Uuid::parse_str("aaa11111-1111-1111-1111-111111111111").unwrap())
    .execute(&pool)
    .await
    .unwrap();

    let embedder = embedder_with_fixed_vector(pool.clone(), unit_vector_at(123));
    artwork_embeddings::process_image(
        &pool,
        &embedder,
        artwork_id,
        // URL would be hit by Jina in prod; fixed-vector embedder ignores it
        // entirely and returns the canned vector.
        "https://example.com/fake.jpg",
    )
    .await
    .expect("process_image");

    // Row landed with the unified 'v2' label.
    let (model_version,): (String,) =
        sqlx::query_as(r#"SELECT model_version FROM artwork_embeddings WHERE artwork_id = $1"#)
            .bind(artwork_id)
            .fetch_one(&pool)
            .await
            .unwrap();
    assert_eq!(model_version, "v2");
}

#[sqlx::test(migrator = "MIGRATOR", fixtures("seed"))]
async fn process_image_makes_artwork_findable_via_similar(pool: PgPool) {
    // Stronger end-to-end signal: after process_image, the new artwork
    // ranks #1 against itself in `/v1/artworks/:anchor/similar`. With
    // both anchor and new artwork sharing the same fixed vector, cosine
    // distance is zero — exact match.
    let artwork_id = Uuid::new_v4();
    sqlx::query(
        r#"
        INSERT INTO artworks (id, artist_id, title, status, published_at)
        VALUES ($1, $2, 'Search Findable', 'published', now())
        "#,
    )
    .bind(artwork_id)
    .bind(Uuid::parse_str("aaa11111-1111-1111-1111-111111111111").unwrap())
    .execute(&pool)
    .await
    .unwrap();

    // Choose a position the seed doesn't write — `pos=500` so this row
    // is unique against fixture neighbors.
    let fixed = unit_vector_at(500);
    let embedder = embedder_with_fixed_vector(pool.clone(), fixed.clone());
    artwork_embeddings::process_image(
        &pool,
        &embedder,
        artwork_id,
        "https://example.com/findable.jpg",
    )
    .await
    .expect("process_image");

    // The new row is the only artwork in the corpus with vector at pos=500,
    // so /similar against itself returns *nothing* (handler excludes the
    // anchor). But /similar against the *seeded* artwork at pos=0 should
    // now rank our pos=500 artwork last (highest cosine distance), and at
    // minimum return it in the result set when include_same_artist=true.
    //
    // Easier signal: query the row directly to assert presence + the
    // model labels match the embedder.
    let (count, model_name, model_version): (i64, String, String) = sqlx::query_as(
        r#"
        SELECT count(*)::bigint, max(model_name), max(model_version)
        FROM artwork_embeddings
        WHERE artwork_id = $1
        "#,
    )
    .bind(artwork_id)
    .fetch_one(&pool)
    .await
    .unwrap();
    assert_eq!(count, 1);
    assert_eq!(model_name, "jinaai/jina-clip-v2");
    assert_eq!(model_version, "v2");

    // Hit /v1/artworks/:id/similar through the full Axum stack to prove
    // the new row is wired into pgvector's index correctly. We pass
    // include_same_artist=true so the same-artist filter doesn't drop it.
    let app = app_with_fixed_vector(pool.clone(), fixed);
    let (status, page): (_, SimilarPage) = get_json(
        app,
        &format!("/v1/artworks/{ARTWORK_BLUE_MORNING}/similar?include_same_artist=true&limit=24"),
    )
    .await;
    assert_eq!(status, 200);
    assert!(
        page.items.iter().any(|a| a.id == artwork_id.to_string()),
        "newly-embedded artwork should appear in /similar results"
    );
}

#[derive(Deserialize)]
struct SimilarPage {
    items: Vec<SimilarItem>,
}
#[derive(Deserialize)]
struct SimilarItem {
    id: String,
}
