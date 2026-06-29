//! T-058 — public series reads.
//!
//! Two endpoints powering the artist-page `?view=series` toggle + the
//! shareable per-series URL:
//!
//! - `GET /v1/artists/:slug/series` — list a single artist's series
//!   that have at least one published artwork. Empty series are
//!   visible only in studio.
//! - `GET /v1/artists/:slug/series/:series_slug` — series header +
//!   first page of member artworks.

use crate::AppState;
use axum::{
    extract::{Path, State},
    Json,
};
use ml_art_core::{
    error::ApiError,
    images::url_for_s3_key,
    models::{ArtistSummary, ArtworkSummary, Paginated, SeriesDetail, SeriesSummary},
};
use sqlx::FromRow;
use std::sync::Arc;
use uuid::Uuid;

const DETAIL_PAGE_LIMIT: i64 = 24;

// ─────────────────────────────────────────────────────────────────────────────
// GET /v1/artists/:slug/series — list
// ─────────────────────────────────────────────────────────────────────────────

#[derive(FromRow)]
struct PublicSeriesRow {
    id: Uuid,
    slug: String,
    title: String,
    statement: Option<String>,
    cover_s3_key: Option<String>,
    artwork_count: i64,
}

impl PublicSeriesRow {
    fn into_summary(self) -> SeriesSummary {
        SeriesSummary {
            id: self.id,
            slug: self.slug,
            title: self.title,
            statement: self.statement,
            cover_image_url: self.cover_s3_key.as_deref().map(url_for_s3_key),
            artwork_count: i32::try_from(self.artwork_count).unwrap_or(0),
        }
    }
}

/// Shared SQL fragment for the public series shape. Empty series
/// (artwork_count = 0) are filtered out — only the studio surface
/// shows them. Cover falls back to the first member's primary image
/// when `cover_artwork_id` is unset (matches the studio fallback).
const PUBLIC_SERIES_SQL: &str = r#"
SELECT
    s.id,
    s.slug,
    s.title,
    s.statement,
    COALESCE(cover_ai.s3_key, first_ai.s3_key) AS cover_s3_key,
    (SELECT COUNT(*)::bigint FROM artworks a
     WHERE a.series_id = s.id
       AND a.deleted_at IS NULL
       AND a.status = 'published') AS artwork_count
FROM series s
JOIN artists ar ON ar.id = s.artist_id
LEFT JOIN artwork_images cover_ai
       ON cover_ai.artwork_id = s.cover_artwork_id
      AND cover_ai.is_primary
      AND cover_ai.moderation_status = 'approved'
LEFT JOIN LATERAL (
    SELECT ai.s3_key
    FROM artworks a
    JOIN artwork_images ai
           ON ai.artwork_id = a.id
          AND ai.is_primary
          AND ai.moderation_status = 'approved'
    WHERE a.series_id = s.id
      AND a.deleted_at IS NULL
      AND a.status = 'published'
    ORDER BY a.published_at DESC NULLS LAST, a.created_at DESC
    LIMIT 1
) first_ai ON TRUE
WHERE ar.slug = $1
  AND ar.deleted_at IS NULL
  AND ar.status = 'active'
  AND s.deleted_at IS NULL
"#;

#[derive(serde::Serialize)]
pub struct PublicSeriesList {
    pub items: Vec<SeriesSummary>,
}

pub async fn list(
    State(state): State<Arc<AppState>>,
    Path(artist_slug): Path<String>,
) -> Result<Json<PublicSeriesList>, ApiError> {
    let sql = format!(
        "{PUBLIC_SERIES_SQL} AND EXISTS (\
            SELECT 1 FROM artworks a WHERE a.series_id = s.id \
              AND a.deleted_at IS NULL AND a.status = 'published'\
         ) ORDER BY s.created_at DESC"
    );
    let rows: Vec<PublicSeriesRow> = sqlx::query_as(sqlx::AssertSqlSafe(sql))
        .bind(&artist_slug)
        .fetch_all(&state.pool)
        .await?;
    Ok(Json(PublicSeriesList {
        items: rows
            .into_iter()
            .map(PublicSeriesRow::into_summary)
            .collect(),
    }))
}

// ─────────────────────────────────────────────────────────────────────────────
// GET /v1/artists/:slug/series/:series_slug — detail
// ─────────────────────────────────────────────────────────────────────────────

#[derive(FromRow)]
struct ArtworkInSeriesRow {
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

impl ArtworkInSeriesRow {
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

#[derive(FromRow)]
struct ArtistMiniRow {
    id: Uuid,
    slug: String,
    display_name: String,
    location: Option<String>,
    city: Option<String>,
    country: Option<String>,
}

pub async fn detail(
    State(state): State<Arc<AppState>>,
    Path((artist_slug, series_slug)): Path<(String, String)>,
) -> Result<Json<SeriesDetail>, ApiError> {
    // Load the series header (404 if missing OR if it's empty —
    // empty series are studio-only).
    let series_row: Option<PublicSeriesRow> = sqlx::query_as(sqlx::AssertSqlSafe(format!(
        "{PUBLIC_SERIES_SQL} AND s.slug = $2"
    )))
    .bind(&artist_slug)
    .bind(&series_slug)
    .fetch_optional(&state.pool)
    .await?;
    let Some(series_row) = series_row else {
        return Err(ApiError::NotFound);
    };
    if series_row.artwork_count == 0 {
        return Err(ApiError::NotFound);
    }

    // Load the artist mini for the response shell.
    let artist_row: Option<ArtistMiniRow> = sqlx::query_as(
        r#"SELECT id, slug, display_name, location, city, country
           FROM artists
           WHERE slug = $1 AND deleted_at IS NULL AND status = 'active'"#,
    )
    .bind(&artist_slug)
    .fetch_optional(&state.pool)
    .await?;
    let Some(artist_row) = artist_row else {
        return Err(ApiError::NotFound);
    };

    let artworks: Vec<ArtworkInSeriesRow> = sqlx::query_as(
        r#"
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
        FROM artworks a
        JOIN artists ar ON ar.id = a.artist_id
        LEFT JOIN artwork_images ai
               ON ai.artwork_id = a.id
              AND ai.is_primary
              AND ai.moderation_status = 'approved'
        WHERE a.series_id = $1
          AND a.deleted_at IS NULL
          AND a.status = 'published'
          AND ar.deleted_at IS NULL
          AND ar.status = 'active'
        ORDER BY a.published_at DESC NULLS LAST, a.created_at DESC
        LIMIT $2
        "#,
    )
    .bind(series_row.id)
    .bind(DETAIL_PAGE_LIMIT)
    .fetch_all(&state.pool)
    .await?;

    Ok(Json(SeriesDetail {
        series: series_row.into_summary(),
        artist: ArtistSummary {
            id: artist_row.id,
            slug: artist_row.slug,
            display_name: artist_row.display_name,
            location: artist_row.location,
            city: artist_row.city,
            country: artist_row.country,
            // Thumbnails come from the series's artwork grid below;
            // the artist-shell card in SeriesDetail doesn't render
            // representative thumbs separately.
            representative_image_urls: Vec::new(),
        },
        artworks: Paginated {
            items: artworks
                .into_iter()
                .map(ArtworkInSeriesRow::into_summary)
                .collect(),
            next_cursor: None,
        },
    }))
}
