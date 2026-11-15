//! `/v1/artists/:slug` — artist profile + first page of artworks.
//!
//! v0 returns the artist row and up to 24 published artworks in one round trip.
//! Pagination for the artworks subresource lands as `/v1/artists/:slug/artworks`
//! once we have cursor pagination plumbed.

use crate::extractors::AuthedUser;
use crate::AppState;
use axum::{
    extract::{Path, State},
    http::HeaderMap,
    Json,
};
use chrono::{DateTime, Utc};
use ml_art_core::{
    auth::OptionalAnonId,
    error::ApiError,
    events::{self, EventName},
    images::url_for_s3_key,
    models::{ArtistDetail, ArtistFull, ArtistLocation, ArtworkSummary, Paginated},
};
use sqlx::FromRow;
use std::sync::Arc;
use uuid::Uuid;

const FIRST_PAGE_LIMIT: i64 = 24;
const REPRESENTATIVE_COUNT: usize = 3;

pub async fn handle(
    State(state): State<Arc<AppState>>,
    Path(slug): Path<String>,
    // T-052 — when present we light up `is_following` for the
    // Follow button. Missing/invalid auth is non-fatal; the field
    // defaults to false.
    // T-083 — additionally, admins get the `status='active'` filter
    // lifted so paused / pending / declined artists are visible.
    // The response's `artist.status` field then drives the "Admin
    // view" banner on the web page.
    auth: Option<AuthedUser>,
    OptionalAnonId(anon_id): OptionalAnonId,
    headers: HeaderMap,
) -> Result<Json<ArtistDetail>, ApiError> {
    let is_admin = auth.as_ref().is_some_and(|AuthedUser(u)| u.is_admin);

    // 1. Look up the artist. Admins bypass the `status='active'`
    //    filter so they can review non-active artists inline.
    let artist_row: Option<ArtistRow> = if is_admin {
        sqlx::query_as(
            r#"
            SELECT
                id, slug, display_name, bio, artist_statement,
                location, city, country, lat, lng,
                website_url, socials, commissioning_preferences, status, entity_type
            FROM artists
            WHERE slug = $1 AND deleted_at IS NULL
            "#,
        )
    } else {
        sqlx::query_as(
            r#"
            SELECT
                id, slug, display_name, bio, artist_statement,
                location, city, country, lat, lng,
                website_url, socials, commissioning_preferences, status, entity_type
            FROM artists
            WHERE slug = $1
              AND deleted_at IS NULL
              AND status = 'active'
            "#,
        )
    }
    .bind(&slug)
    .fetch_optional(&state.pool)
    .await?;

    let Some(artist) = artist_row else {
        return Err(ApiError::NotFound);
    };

    // 2–3. Artworks (admins get drafts included so they can see the
    //      full portfolio before deciding) + public locations. Shared
    //      with the T-086 preview page via `load_profile`.
    let statuses = if is_admin {
        ADMIN_ARTWORK_STATUSES
    } else {
        PUBLIC_ARTWORK_STATUSES
    };
    let artist_id = artist.id;
    let Profile {
        artist: full,
        artworks: artwork_summaries,
        locations,
    } = load_profile(&state.pool, artist, statuses).await?;

    // 4. Follow graph state (T-052): follower count + this caller's
    //    is_following flag. Two queries because the count is unconditional
    //    and the flag is per-caller; combining them would mean coalescing
    //    a NULL for signed-out requests for marginal gain.
    let follower_count: i64 =
        sqlx::query_scalar("SELECT count(*) FROM follows WHERE artist_id = $1")
            .bind(artist_id)
            .fetch_one(&state.pool)
            .await
            .unwrap_or(0);

    let is_following = match &auth {
        Some(AuthedUser(user)) => sqlx::query_scalar::<_, bool>(
            "SELECT EXISTS(SELECT 1 FROM follows WHERE user_id = $1 AND artist_id = $2)",
        )
        .bind(user.id)
        .bind(artist_id)
        .fetch_one(&state.pool)
        .await
        .unwrap_or(false),
        None => false,
    };

    // T-050 — artist_viewed. Emitted on success (404 short-circuits
    // above with `return Err(NotFound)` so dead-slug views never
    // pollute the analytics). Signed-in user gets attribution if
    // present; anon_id covers the rest.
    events::emit(
        &state.jobs,
        events::event_log(
            EventName::ArtistViewed,
            anon_id,
            auth.as_ref().map(|AuthedUser(u)| u.id),
            serde_json::json!({ "artist_id": artist_id, "slug": slug }),
            events::extract_request_context(&headers),
        ),
    )
    .await;

    Ok(Json(ArtistDetail {
        artist: full,
        artworks: Paginated {
            items: artwork_summaries,
            next_cursor: None,
        },
        locations,
        is_following,
        follower_count: follower_count as i32,
    }))
}

// ─────────────────────────────────────────────────────────────────────────────
// Shared profile loading (public page, admin view, T-086 preview)
// ─────────────────────────────────────────────────────────────────────────────

/// Artwork statuses each surface may show.
pub(crate) const PUBLIC_ARTWORK_STATUSES: &[&str] = &["published"];
pub(crate) const ADMIN_ARTWORK_STATUSES: &[&str] = &["draft", "published", "archived"];
/// T-086 — a draft profile's preview shows work-in-progress artworks
/// too (the admin builds the example as drafts), but not archived ones.
pub(crate) const PREVIEW_ARTWORK_STATUSES: &[&str] = &["draft", "published"];

/// Everything a profile page renders apart from the follow state.
pub(crate) struct Profile {
    pub artist: ArtistFull,
    pub artworks: Vec<ArtworkSummary>,
    pub locations: Vec<ArtistLocation>,
}

/// Load an artist row by id, ignoring `status` (callers gate on it).
pub(crate) async fn fetch_artist_row_by_id(
    pool: &ml_art_core::db::Pool,
    id: Uuid,
) -> Result<Option<ArtistRow>, ApiError> {
    Ok(sqlx::query_as(
        r#"
        SELECT
            id, slug, display_name, bio, artist_statement,
            location, city, country, lat, lng,
            website_url, socials, commissioning_preferences, status, entity_type
        FROM artists
        WHERE id = $1 AND deleted_at IS NULL
        "#,
    )
    .bind(id)
    .fetch_optional(pool)
    .await?)
}

/// First page of artworks (filtered to `statuses`), public locations,
/// and the `ArtistFull` header for an already-authorised artist row.
/// Images are always restricted to moderation-approved ones.
pub(crate) async fn load_profile(
    pool: &ml_art_core::db::Pool,
    artist: ArtistRow,
    statuses: &[&str],
) -> Result<Profile, ApiError> {
    let statuses: Vec<String> = statuses.iter().map(|s| s.to_string()).collect();
    let artworks: Vec<ArtworkRow> = sqlx::query_as(
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
        WHERE a.artist_id = $1
          AND a.deleted_at IS NULL
          AND a.status = ANY($3)
          AND ar.deleted_at IS NULL
        ORDER BY a.published_at DESC NULLS LAST, a.created_at DESC
        LIMIT $2
        "#,
    )
    .bind(artist.id)
    .bind(FIRST_PAGE_LIMIT)
    .bind(&statuses)
    .fetch_all(pool)
    .await?;

    let artwork_summaries: Vec<ArtworkSummary> =
        artworks.into_iter().map(ArtworkRow::into_summary).collect();

    let representative_image_urls: Vec<String> = artwork_summaries
        .iter()
        .filter_map(|a| a.primary_image_url.clone())
        .take(REPRESENTATIVE_COUNT)
        .collect();

    // Public locations for this artist. Only geocoded rows are
    // returned to the public surface (T-038 G1) — the studio UI uses a
    // different endpoint that includes pre-geocode rows so the artist
    // can see "Locating…" feedback.
    let location_rows: Vec<ArtistLocationRow> = sqlx::query_as(
        r#"
        SELECT
            id, kind, name, address, city, country, lat, lng,
            website_url, display_order, geocoded_at
        FROM artist_locations
        WHERE artist_id = $1
          AND deleted_at IS NULL
          AND lat IS NOT NULL
          AND lng IS NOT NULL
        ORDER BY display_order ASC, created_at ASC
        "#,
    )
    .bind(artist.id)
    .fetch_all(pool)
    .await?;

    let locations: Vec<ArtistLocation> = location_rows
        .into_iter()
        .map(ArtistLocationRow::into_dto)
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
        status: artist.status,
        entity_type: artist.entity_type,
    };

    Ok(Profile {
        artist: full,
        artworks: artwork_summaries,
        locations,
    })
}

// ─────────────────────────────────────────────────────────────────────────────
// Row types
// ─────────────────────────────────────────────────────────────────────────────

#[derive(FromRow)]
pub(crate) struct ArtistRow {
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
    status: String,
    entity_type: String,
}

#[derive(FromRow)]
struct ArtworkRow {
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

impl ArtworkRow {
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

#[derive(FromRow)]
struct ArtistLocationRow {
    id: Uuid,
    kind: String,
    name: String,
    address: String,
    city: Option<String>,
    country: Option<String>,
    lat: Option<f64>,
    lng: Option<f64>,
    website_url: Option<String>,
    display_order: i32,
    geocoded_at: Option<DateTime<Utc>>,
}

impl ArtistLocationRow {
    fn into_dto(self) -> ArtistLocation {
        ArtistLocation {
            id: self.id,
            kind: self.kind,
            name: self.name,
            address: self.address,
            city: self.city,
            country: self.country,
            lat: self.lat,
            lng: self.lng,
            website_url: self.website_url,
            display_order: self.display_order,
            geocoded_at: self.geocoded_at,
        }
    }
}
