//! `/v1/artworks/:id` and `/v1/artworks/:id/similar`.

use crate::extractors::AuthedUser;
use crate::AppState;
use axum::{
    extract::{Path, Query, State},
    http::HeaderMap,
    Json,
};
use chrono::{DateTime, Utc};
use ml_art_core::{
    auth::OptionalAnonId,
    error::ApiError,
    events::{self, EventName},
    images::url_for_s3_key,
    models::{ArtworkArtist, ArtworkFull, ArtworkImage, ArtworkSummary, Paginated},
};
use serde::Deserialize;
use sqlx::FromRow;
use std::sync::Arc;
use uuid::Uuid;

const SIMILAR_LIMIT_DEFAULT: i64 = 8;
const SIMILAR_LIMIT_MAX: i64 = 24;

// ─────────────────────────────────────────────────────────────────────────────
// GET /v1/artworks/:id
// ─────────────────────────────────────────────────────────────────────────────

pub async fn detail(
    State(state): State<Arc<AppState>>,
    Path(id): Path<Uuid>,
    // T-083 — admins bypass the `status='published'` +
    // `ar.status='active'` filters so they can preview drafts and
    // artworks by non-active artists inline. Non-admin callers see
    // exactly what they'd see today.
    auth: Option<AuthedUser>,
    OptionalAnonId(anon_id): OptionalAnonId,
    headers: HeaderMap,
) -> Result<Json<ArtworkFull>, ApiError> {
    let is_admin = auth.as_ref().is_some_and(|AuthedUser(u)| u.is_admin);
    let detail_sql: &str = if is_admin {
        r#"
        SELECT
            a.id, a.title, a.description, a.year_created,
            a.medium, a.medium_category, a.dimensions,
            a.price_cents, a.currency, a.availability,
            a.external_url, a.published_at,
            ar.id AS artist_id, ar.slug AS artist_slug,
            ar.display_name AS artist_name, ar.location AS artist_location
        FROM artworks a
        JOIN artists ar ON ar.id = a.artist_id
        WHERE a.id = $1
          AND a.deleted_at IS NULL
          AND ar.deleted_at IS NULL
        "#
    } else {
        r#"
        SELECT
            a.id, a.title, a.description, a.year_created,
            a.medium, a.medium_category, a.dimensions,
            a.price_cents, a.currency, a.availability,
            a.external_url, a.published_at,
            ar.id AS artist_id, ar.slug AS artist_slug,
            ar.display_name AS artist_name, ar.location AS artist_location
        FROM artworks a
        JOIN artists ar ON ar.id = a.artist_id
        WHERE a.id = $1
          AND a.deleted_at IS NULL
          AND a.status = 'published'
          AND ar.deleted_at IS NULL
          AND ar.status = 'active'
        "#
    };
    let row: Option<ArtworkRow> = sqlx::query_as(detail_sql)
        .bind(id)
        .fetch_optional(&state.pool)
        .await?;

    let Some(row) = row else {
        return Err(ApiError::NotFound);
    };

    // Images for this artwork, ordered for display.
    let images: Vec<ImageRow> = sqlx::query_as(
        r#"
        SELECT id, s3_key, width, height, is_primary, display_order
        FROM artwork_images
        WHERE artwork_id = $1
          AND moderation_status = 'approved'
        ORDER BY is_primary DESC, display_order ASC
        "#,
    )
    .bind(id)
    .fetch_all(&state.pool)
    .await?;

    // T-050 — artwork_viewed. Best-effort enqueue; never blocks the response.
    events::emit(
        &state.jobs,
        events::event_log(
            EventName::ArtworkViewed,
            anon_id,
            None,
            serde_json::json!({ "artwork_id": id }),
            events::extract_request_context(&headers),
        ),
    )
    .await;

    Ok(Json(row.into_full(images)))
}

// ─────────────────────────────────────────────────────────────────────────────
// GET /v1/artworks/:id/similar
// ─────────────────────────────────────────────────────────────────────────────

#[derive(Debug, Deserialize, Default)]
pub struct SimilarParams {
    #[serde(default)]
    limit: Option<i64>,
    #[serde(default)]
    include_same_artist: Option<bool>,
}

pub async fn similar(
    State(state): State<Arc<AppState>>,
    Path(id): Path<Uuid>,
    Query(params): Query<SimilarParams>,
) -> Result<Json<Paginated<ArtworkSummary>>, ApiError> {
    let limit = params
        .limit
        .unwrap_or(SIMILAR_LIMIT_DEFAULT)
        .clamp(1, SIMILAR_LIMIT_MAX);
    let include_same_artist = params.include_same_artist.unwrap_or(false);

    // Use the embedding of the anchor artwork as the query vector.
    // pgvector's `<=>` operator is cosine distance; we order ascending.
    let same_artist_clause = if include_same_artist {
        ""
    } else {
        "AND a.artist_id != anchor.artist_id"
    };

    let sql = format!(
        r#"
        WITH anchor AS (
            SELECT a.id AS artwork_id, a.artist_id, ae.embedding
            FROM artworks a
            JOIN artwork_embeddings ae ON ae.artwork_id = a.id
            WHERE a.id = $1
              AND ae.model_name = $2
              AND ae.model_version = $3
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
        FROM anchor, artwork_embeddings ae
        JOIN artworks a  ON a.id = ae.artwork_id
        JOIN artists ar  ON ar.id = a.artist_id
        LEFT JOIN artwork_images ai
               ON ai.artwork_id = a.id
              AND ai.is_primary
              AND ai.moderation_status = 'approved'
        WHERE ae.model_name = $2
          AND ae.model_version = $3
          AND a.deleted_at IS NULL
          AND a.status = 'published'
          AND ar.deleted_at IS NULL
          AND ar.status = 'active'
          AND a.id != anchor.artwork_id
          {same_artist_clause}
        ORDER BY ae.embedding <=> anchor.embedding
        LIMIT $4
        "#
    );

    let rows: Vec<SimilarRow> =
        sqlx::query_as_with::<_, SimilarRow, _>(sqlx::AssertSqlSafe(sql), {
            let mut a = sqlx::postgres::PgArguments::default();
            use sqlx::Arguments;
            a.add(id)
                .map_err(|e| ApiError::Internal(anyhow::anyhow!("{e}")))?;
            a.add(state.cfg.embedding_model_name.clone())
                .map_err(|e| ApiError::Internal(anyhow::anyhow!("{e}")))?;
            a.add(state.cfg.embedding_model_version.clone())
                .map_err(|e| ApiError::Internal(anyhow::anyhow!("{e}")))?;
            a.add(limit)
                .map_err(|e| ApiError::Internal(anyhow::anyhow!("{e}")))?;
            a
        })
        .fetch_all(&state.pool)
        .await?;

    let items: Vec<ArtworkSummary> = rows.into_iter().map(SimilarRow::into_summary).collect();

    Ok(Json(Paginated {
        items,
        next_cursor: None,
    }))
}

// ─────────────────────────────────────────────────────────────────────────────
// Row types
// ─────────────────────────────────────────────────────────────────────────────

#[derive(FromRow)]
struct ArtworkRow {
    id: Uuid,
    title: Option<String>,
    description: Option<String>,
    year_created: Option<i32>,
    medium: Option<String>,
    medium_category: Option<String>,
    dimensions: Option<serde_json::Value>,
    price_cents: Option<i64>,
    currency: String,
    availability: String,
    external_url: Option<String>,
    published_at: Option<DateTime<Utc>>,
    artist_id: Uuid,
    artist_slug: String,
    artist_name: String,
    artist_location: Option<String>,
}

impl ArtworkRow {
    fn into_full(self, images: Vec<ImageRow>) -> ArtworkFull {
        ArtworkFull {
            id: self.id,
            title: self.title,
            description: self.description,
            year_created: self.year_created,
            medium: self.medium,
            medium_category: self.medium_category,
            dimensions: self.dimensions,
            price_cents: self.price_cents,
            currency: self.currency,
            availability: self.availability,
            external_url: self.external_url,
            published_at: self.published_at,
            artist: ArtworkArtist {
                id: self.artist_id,
                slug: self.artist_slug,
                display_name: self.artist_name,
                location: self.artist_location,
            },
            images: images.into_iter().map(ImageRow::into_image).collect(),
        }
    }
}

#[derive(FromRow)]
struct ImageRow {
    id: Uuid,
    s3_key: String,
    width: Option<i32>,
    height: Option<i32>,
    is_primary: bool,
    display_order: i32,
}

impl ImageRow {
    fn into_image(self) -> ArtworkImage {
        ArtworkImage {
            id: self.id,
            url: url_for_s3_key(&self.s3_key),
            width: self.width,
            height: self.height,
            is_primary: self.is_primary,
            display_order: self.display_order,
        }
    }
}

#[derive(FromRow)]
struct SimilarRow {
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

impl SimilarRow {
    fn into_summary(self) -> ArtworkSummary {
        ArtworkSummary {
            id: self.id,
            title: self.title,
            artist_id: self.artist_id,
            artist_name: self.artist_name,
            artist_slug: self.artist_slug,
            primary_image_url: self.primary_s3_key.map(|k| url_for_s3_key(&k)),
            price_cents: self.price_cents,
            currency: self.currency,
            availability: self.availability,
        }
    }
}
