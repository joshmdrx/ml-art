//! `/v1/artists/:slug` — artist profile + first page of artworks.
//!
//! v0 returns the artist row and up to 24 published artworks in one round trip.
//! Pagination for the artworks subresource lands as `/v1/artists/:slug/artworks`
//! once we have cursor pagination plumbed.

use crate::AppState;
use axum::{
    extract::{Path, State},
    Json,
};
use ml_art_core::{
    error::ApiError,
    models::{ArtistDetail, ArtistFull, ArtworkSummary, Paginated},
};
use sqlx::FromRow;
use std::sync::Arc;
use uuid::Uuid;

const FIRST_PAGE_LIMIT: i64 = 24;
const REPRESENTATIVE_COUNT: usize = 3;

pub async fn handle(
    State(state): State<Arc<AppState>>,
    Path(slug): Path<String>,
) -> Result<Json<ArtistDetail>, ApiError> {
    // 1. Look up the artist.
    let artist_row: Option<ArtistRow> = sqlx::query_as(
        r#"
        SELECT
            id, slug, display_name, bio, artist_statement,
            location, city, country, lat, lng,
            website_url, socials, commissioning_preferences
        FROM artists
        WHERE slug = $1
          AND deleted_at IS NULL
          AND status = 'active'
        "#,
    )
    .bind(&slug)
    .fetch_optional(&state.pool)
    .await?;

    let Some(artist) = artist_row else {
        return Err(ApiError::NotFound);
    };

    // 2. First page of published artworks for this artist.
    let artworks: Vec<ArtworkRow> = sqlx::query_as(
        r#"
        SELECT
            a.id,
            a.title,
            ar.display_name AS artist_name,
            ar.slug         AS artist_slug,
            ai.s3_key       AS primary_s3_key,
            a.price_cents,
            a.currency,
            a.availability
        FROM artworks a
        JOIN artists ar ON ar.id = a.artist_id
        LEFT JOIN artwork_images ai
               ON ai.artwork_id = a.id AND ai.is_primary
        WHERE a.artist_id = $1
          AND a.deleted_at IS NULL
          AND a.status = 'published'
          AND ar.deleted_at IS NULL
        ORDER BY a.published_at DESC NULLS LAST
        LIMIT $2
        "#,
    )
    .bind(artist.id)
    .bind(FIRST_PAGE_LIMIT)
    .fetch_all(&state.pool)
    .await?;

    let artwork_summaries: Vec<ArtworkSummary> =
        artworks.into_iter().map(ArtworkRow::into_summary).collect();

    let representative_image_urls: Vec<String> = artwork_summaries
        .iter()
        .filter_map(|a| a.primary_image_url.clone())
        .take(REPRESENTATIVE_COUNT)
        .collect();

    let full = ArtistFull {
        id: artist.id,
        slug: artist.slug,
        display_name: artist.display_name,
        bio: artist.bio,
        artist_statement: artist.artist_statement,
        location: artist.location,
        city: artist.city,
        country: artist.country,
        lat: artist.lat,
        lng: artist.lng,
        website_url: artist.website_url,
        socials: artist.socials.unwrap_or(serde_json::json!({})),
        commissioning_preferences: artist.commissioning_preferences,
        representative_image_urls,
    };

    Ok(Json(ArtistDetail {
        artist: full,
        artworks: Paginated {
            items: artwork_summaries,
            next_cursor: None,
        },
    }))
}

// ─────────────────────────────────────────────────────────────────────────────
// Row types
// ─────────────────────────────────────────────────────────────────────────────

#[derive(FromRow)]
struct ArtistRow {
    id: Uuid,
    slug: String,
    display_name: String,
    bio: Option<String>,
    artist_statement: Option<String>,
    location: Option<String>,
    city: Option<String>,
    country: Option<String>,
    lat: Option<f64>,
    lng: Option<f64>,
    website_url: Option<String>,
    socials: Option<serde_json::Value>,
    commissioning_preferences: Option<serde_json::Value>,
}

#[derive(FromRow)]
struct ArtworkRow {
    id: Uuid,
    title: Option<String>,
    artist_name: String,
    artist_slug: String,
    primary_s3_key: Option<String>,
    price_cents: Option<i64>,
    currency: String,
    availability: String,
}

impl ArtworkRow {
    fn into_summary(self) -> ArtworkSummary {
        ArtworkSummary {
            id: self.id,
            title: self.title,
            artist_name: self.artist_name,
            artist_slug: self.artist_slug,
            primary_image_url: self.primary_s3_key.map(|k| {
                let base = std::env::var("IMAGE_BASE_URL")
                    .unwrap_or_else(|_| "http://localhost:9000/artworks".to_string());
                format!("{base}/{k}")
            }),
            price_cents: self.price_cents,
            currency: self.currency,
            availability: self.availability,
        }
    }
}
