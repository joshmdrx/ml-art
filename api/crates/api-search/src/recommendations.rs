//! T-056 — personalised retrieval surfaces.
//!
//! Reads `user_profiles.taste_embedding` (built by T-055), finds the
//! nearest artworks by HNSW cosine, and returns them as the user-
//! facing "For you" row.
//!
//! Threshold: `interaction_count >= MIN_INTERACTION_COUNT` (5). A
//! completed T-061 calibrator session lands on exactly 5 — so finishing
//! the calibrator is enough to unlock personalisation. Below that the
//! signal is too thin to be meaningfully different from random.
//!
//! Jitter: rank top-50 by similarity then `ORDER BY random()` LIMIT 12.
//! Gives discovery within the user's taste neighbourhood without
//! always serving the same 12 artworks. Tuneable; the candidate pool
//! is `FOR_YOU_CANDIDATE_POOL` and the returned size is
//! `FOR_YOU_LIMIT`.

use crate::AppState;
use axum::{extract::State, Json};
use ml_art_core::{error::ApiError, images::url_for_s3_key, models::ArtworkSummary};
use sqlx::FromRow;
use std::sync::Arc;
use uuid::Uuid;

use crate::extractors::AuthedUser;

/// Min interaction_count before personalised retrieval activates.
/// A completed T-061 calibrator session = 5, so finishing the
/// calibrator is the minimum bar. Tune as data accumulates.
pub const MIN_INTERACTION_COUNT: i32 = 5;

/// Top-K by HNSW distance fed into the jitter step.
const FOR_YOU_CANDIDATE_POOL: i64 = 50;

/// Final response size. Matches the homepage grid's expected row.
const FOR_YOU_LIMIT: i64 = 12;

#[derive(serde::Serialize, serde::Deserialize)]
pub struct ForYouResponse {
    pub items: Vec<ArtworkSummary>,
    /// `true` when the user passes the `MIN_INTERACTION_COUNT` gate
    /// AND their taste vector is set. Lets the web layer decide
    /// whether to render the "For you" row or fall back to a default
    /// surface without inferring intent from `items.length == 0`
    /// (which can also mean "you have personalisation enabled but
    /// no artworks match" — rare but real).
    pub eligible: bool,
}

#[derive(FromRow)]
struct Row {
    id: Uuid,
    title: Option<String>,
    artist_id: Uuid,
    artist_name: String,
    artist_slug: String,
    primary_s3_key: Option<String>,
    price_cents: Option<i64>,
    currency: String,
    availability: String,
}

impl Row {
    fn into_summary(self) -> ArtworkSummary {
        ArtworkSummary {
            id: self.id,
            title: self.title,
            artist_id: self.artist_id,
            artist_name: self.artist_name,
            artist_slug: self.artist_slug,
            primary_image_url: self.primary_s3_key.as_deref().map(url_for_s3_key),
            price_cents: self.price_cents,
            currency: self.currency,
            availability: self.availability,
        }
    }
}

pub async fn for_you(
    State(state): State<Arc<AppState>>,
    AuthedUser(user): AuthedUser,
) -> Result<Json<ForYouResponse>, ApiError> {
    let items = compute_for_you(&state, user.id).await?;
    Ok(Json(ForYouResponse {
        eligible: !items.is_empty(),
        items,
    }))
}

/// Pure data path — separated from the axum handler so integration
/// tests can call it without spinning up a full router.
pub async fn compute_for_you(
    state: &AppState,
    user_id: Uuid,
) -> Result<Vec<ArtworkSummary>, ApiError> {
    // Gate: the SELECT returns `(taste_embedding, interaction_count)`
    // or no row if the user has never been refreshed by T-055.
    let profile: Option<(Option<pgvector::Vector>, i32)> = sqlx::query_as(
        "SELECT taste_embedding, interaction_count FROM user_profiles WHERE user_id = $1",
    )
    .bind(user_id)
    .fetch_optional(&state.pool)
    .await?;
    let Some((Some(taste), interaction_count)) = profile else {
        return Ok(Vec::new());
    };
    if interaction_count < MIN_INTERACTION_COUNT {
        return Ok(Vec::new());
    }

    // Top-K nearest by HNSW cosine over the active embedding model,
    // then RANDOM-shuffle within that pool. Single round-trip; the
    // HNSW index does the heavy lifting.
    let rows: Vec<Row> = sqlx::query_as(
        r#"
        WITH nearest AS (
            SELECT ae.artwork_id
            FROM artwork_embeddings ae
            WHERE ae.model_name = $1 AND ae.model_version = $2
            ORDER BY ae.embedding <=> $3
            LIMIT $4
        )
        SELECT
            a.id,
            a.title,
            ar.id           AS artist_id,
            ar.display_name AS artist_name,
            ar.slug         AS artist_slug,
            ai.s3_key       AS primary_s3_key,
            a.price_cents,
            a.currency,
            a.availability
        FROM nearest n
        JOIN artworks a   ON a.id = n.artwork_id
        JOIN artists ar   ON ar.id = a.artist_id
        LEFT JOIN artwork_images ai
               ON ai.artwork_id = a.id
              AND ai.is_primary
              AND ai.moderation_status = 'approved'
        WHERE a.deleted_at IS NULL
          AND a.status = 'published'
          AND ar.deleted_at IS NULL
          AND ar.status = 'active'
        ORDER BY random()
        LIMIT $5
        "#,
    )
    .bind(&state.cfg.embedding_model_name)
    .bind(&state.cfg.embedding_model_version)
    .bind(&taste)
    .bind(FOR_YOU_CANDIDATE_POOL)
    .bind(FOR_YOU_LIMIT)
    .fetch_all(&state.pool)
    .await?;

    Ok(rows.into_iter().map(Row::into_summary).collect())
}
