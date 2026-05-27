//! Writes to the `artwork_embeddings` table and the `process_image`
//! pipeline that composes Jina embed + DB write.
//!
//! `T-036`. Until now nothing in the production code path took a
//! freshly-created `artworks` row → called Jina → wrote an embedding
//! row. Seeded artworks have embeddings (Python local pass at seed
//! time), but anything new went unembedded. That gap blocked `T-011`
//! (studio create) and `T-010` (upload-driven visual search) alike.
//!
//! Studio create handlers (and later, an `image.process` Inngest job)
//! call `process_image(...)` after the upload row lands. For v0 we call
//! inline; when we add Rekognition gating + scale concerns we'll lift
//! to a job queue. The function contract stays the same either way.

use crate::db::Pool;
use crate::embedder::Embedder;
use pgvector::Vector;
use uuid::Uuid;

/// INSERT (or upsert) one row into `artwork_embeddings`. PK is
/// `(artwork_id, model_name, model_version)` so re-embedding the same
/// artwork with the same model is idempotent — useful when a backfill
/// retries the same row. Re-embedding with a *different* model
/// (`model_version='v3'` later, say) adds a new row alongside the
/// existing one, which is what we want for A/B + safe rollout.
pub async fn write(
    pool: &Pool,
    artwork_id: Uuid,
    model_name: &str,
    model_version: &str,
    embedding: &Vector,
) -> sqlx::Result<()> {
    sqlx::query(
        r#"
        INSERT INTO artwork_embeddings
            (artwork_id, model_name, model_version, embedding)
        VALUES ($1, $2, $3, $4)
        ON CONFLICT (artwork_id, model_name, model_version) DO UPDATE
           SET embedding = EXCLUDED.embedding,
               created_at = now()
        "#,
    )
    .bind(artwork_id)
    .bind(model_name)
    .bind(model_version)
    .bind(embedding)
    .execute(pool)
    .await?;
    Ok(())
}

/// End-to-end: download nothing, ask Jina for the image embedding,
/// write the row. Callers (studio create handler today; future
/// `image.process` job tomorrow) pass the public URL that Jina's
/// workers can fetch.
///
/// This is the single function studio handlers depend on; the rest of
/// the pipeline (Rekognition moderation gate, async retries, etc.) sits
/// either side of it as scope grows.
pub async fn process_image(
    pool: &Pool,
    embedder: &Embedder,
    artwork_id: Uuid,
    image_url: &str,
) -> anyhow::Result<()> {
    let vector = embedder.embed_image_from_url(image_url).await?;
    write(
        pool,
        artwork_id,
        embedder.model_name(),
        embedder.model_version(),
        &vector,
    )
    .await?;
    Ok(())
}
