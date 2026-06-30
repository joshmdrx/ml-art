//! T-081 — venues + venue_artworks (galleries / shops as discovery
//! destinations).
//!
//! Three concerns, kept in one file for now (split into a folder when
//! it grows past ~800 lines):
//!   - **Studio CRUD** on venues — the venue owner manages their
//!     listings. New venues default to `status='pending_review'` and
//!     stay hidden from public surfaces until an admin flips them.
//!   - **Invitation flow** via venue_artworks — the venue invites an
//!     artwork (`POST .../artworks/:artwork_id`), creating a pending
//!     row. The artwork's owning artist accepts or declines via
//!     `/v1/studio/venue-requests/...`. Only `accepted` rows surface
//!     publicly.
//!   - **Public reads** — `/v1/venues` index + `/v1/venues/:slug`
//!     detail. Inactive venues 404 indistinguishably from non-existent.
//!
//! Geocoding mirrors `artist_locations`: address goes in at create/patch
//! time; `JobEvent::VenueGeocode` updates lat/lng/city/country
//! out-of-band. Pre-geocode rows have `lat/lng` NULL and are filtered
//! out of map pins.

use crate::extractors::AuthedUser;
use crate::studio::current_artist_id;
use crate::AppState;
use axum::{
    extract::{Path, Query, State},
    http::StatusCode,
    Json,
};
use ml_art_core::{
    cursor::{CursorError, PageCursor},
    error::ApiError,
    jobs::{EnqueueOpts, JobEvent},
};
use serde::{Deserialize, Serialize};
use sqlx::FromRow;
use std::sync::Arc;
use uuid::Uuid;

const MAX_NAME_LEN: usize = 200;
const MAX_ABOUT_LEN: usize = 4000;
const MAX_OPENING_INFO_LEN: usize = 500;
const MAX_ADDRESS_LEN: usize = 500;
const DEFAULT_LIMIT: i64 = 24;
const MAX_LIMIT: i64 = 100;

const VENUE_KINDS: &[&str] = &[
    "gallery",
    "shop",
    "studio_collective",
    "cafe_collab",
    "other",
];

fn slugify(name: &str) -> String {
    crate::onboarding::slugify(name)
}

// ─────────────────────────────────────────────────────────────────────────────
// Row shape
// ─────────────────────────────────────────────────────────────────────────────

#[derive(Debug, Serialize, FromRow)]
pub struct Venue {
    pub id: Uuid,
    pub slug: String,
    pub name: String,
    pub kind: String,
    pub about: Option<String>,
    pub address: Option<String>,
    pub city: Option<String>,
    pub country: Option<String>,
    pub lat: Option<f64>,
    pub lng: Option<f64>,
    pub geocoded_at: Option<chrono::DateTime<chrono::Utc>>,
    pub website_url: Option<String>,
    pub instagram_handle: Option<String>,
    pub opening_info: Option<String>,
    pub owner_user_id: Uuid,
    pub status: String,
    pub created_at: chrono::DateTime<chrono::Utc>,
    pub updated_at: chrono::DateTime<chrono::Utc>,
}

const VENUE_COLUMNS: &str = r#"
    id, slug, name, kind, about,
    address, city, country, lat, lng, geocoded_at,
    website_url, instagram_handle, opening_info,
    owner_user_id, status, created_at, updated_at
"#;

// ─────────────────────────────────────────────────────────────────────────────
// Studio CRUD
// ─────────────────────────────────────────────────────────────────────────────

#[derive(Debug, Deserialize)]
pub struct CreateVenueBody {
    pub name: String,
    pub kind: String,
    #[serde(default)]
    pub about: Option<String>,
    #[serde(default)]
    pub address: Option<String>,
    #[serde(default)]
    pub website_url: Option<String>,
    #[serde(default)]
    pub instagram_handle: Option<String>,
    #[serde(default)]
    pub opening_info: Option<String>,
    /// Optional slug; if absent we derive from `name`. Conflicts
    /// against existing non-deleted rows return 409.
    #[serde(default)]
    pub slug: Option<String>,
}

fn validate_create(body: &CreateVenueBody) -> Result<(), ApiError> {
    if body.name.trim().is_empty() {
        return Err(ApiError::BadRequest("name: required".into()));
    }
    if body.name.len() > MAX_NAME_LEN {
        return Err(ApiError::BadRequest("name: too long".into()));
    }
    if !VENUE_KINDS.contains(&body.kind.as_str()) {
        return Err(ApiError::BadRequest(format!(
            "kind: must be one of {}",
            VENUE_KINDS.join(", ")
        )));
    }
    if let Some(a) = &body.about {
        if a.len() > MAX_ABOUT_LEN {
            return Err(ApiError::BadRequest("about: too long".into()));
        }
    }
    if let Some(a) = &body.address {
        if a.len() > MAX_ADDRESS_LEN {
            return Err(ApiError::BadRequest("address: too long".into()));
        }
    }
    if let Some(o) = &body.opening_info {
        if o.len() > MAX_OPENING_INFO_LEN {
            return Err(ApiError::BadRequest("opening_info: too long".into()));
        }
    }
    Ok(())
}

pub async fn create(
    State(state): State<Arc<AppState>>,
    AuthedUser(user): AuthedUser,
    Json(body): Json<CreateVenueBody>,
) -> Result<(StatusCode, Json<Venue>), ApiError> {
    validate_create(&body)?;
    // Slug: explicit value wins; otherwise derive from name. Collision
    // returns 409 rather than auto-suffixing (mirrors T-058 series).
    let base_slug = body
        .slug
        .as_deref()
        .map(str::to_owned)
        .unwrap_or_else(|| slugify(&body.name));
    if base_slug.is_empty() {
        return Err(ApiError::BadRequest(
            "slug: derived slug was empty; provide one explicitly".into(),
        ));
    }

    let row = sqlx::query_as::<_, Venue>(sqlx::AssertSqlSafe(format!(
        r#"
        INSERT INTO venues (slug, name, kind, about, address,
                            website_url, instagram_handle, opening_info,
                            owner_user_id, status)
        VALUES ($1, $2, $3, $4, $5, $6, $7, $8, $9, 'pending_review')
        RETURNING {VENUE_COLUMNS}
        "#
    )))
    .bind(&base_slug)
    .bind(body.name.trim())
    .bind(&body.kind)
    .bind(body.about.as_deref().map(str::trim))
    .bind(body.address.as_deref().map(str::trim))
    .bind(body.website_url.as_deref().map(str::trim))
    .bind(body.instagram_handle.as_deref().map(str::trim))
    .bind(body.opening_info.as_deref().map(str::trim))
    .bind(user.id)
    .fetch_one(&state.pool)
    .await
    .map_err(|e| {
        if let sqlx::Error::Database(db_err) = &e {
            if db_err.constraint() == Some("venues_slug_active_idx") {
                return ApiError::Conflict(format!(
                    "slug `{base_slug}` already in use"
                ));
            }
        }
        e.into()
    })?;

    // Best-effort geocode if an address was provided.
    if row.address.is_some() {
        let _ = state
            .jobs
            .enqueue(
                JobEvent::VenueGeocode { venue_id: row.id },
                EnqueueOpts {
                    idempotency_key: Some(format!("venue_geocode:{}:create", row.id)),
                    ..Default::default()
                },
            )
            .await;
    }
    Ok((StatusCode::CREATED, Json(row)))
}

pub async fn list_own(
    State(state): State<Arc<AppState>>,
    AuthedUser(user): AuthedUser,
) -> Result<Json<Vec<Venue>>, ApiError> {
    let rows = sqlx::query_as::<_, Venue>(sqlx::AssertSqlSafe(format!(
        r#"
        SELECT {VENUE_COLUMNS}
        FROM venues
        WHERE owner_user_id = $1 AND deleted_at IS NULL
        ORDER BY created_at DESC
        "#
    )))
    .bind(user.id)
    .fetch_all(&state.pool)
    .await?;
    Ok(Json(rows))
}

pub async fn detail_own(
    State(state): State<Arc<AppState>>,
    AuthedUser(user): AuthedUser,
    Path(id): Path<Uuid>,
) -> Result<Json<Venue>, ApiError> {
    let row = sqlx::query_as::<_, Venue>(sqlx::AssertSqlSafe(format!(
        r#"
        SELECT {VENUE_COLUMNS}
        FROM venues
        WHERE id = $1 AND owner_user_id = $2 AND deleted_at IS NULL
        "#
    )))
    .bind(id)
    .bind(user.id)
    .fetch_optional(&state.pool)
    .await?
    .ok_or(ApiError::NotFound)?;
    Ok(Json(row))
}

#[derive(Debug, Deserialize)]
pub struct PatchVenueBody {
    #[serde(default)]
    pub name: Option<String>,
    #[serde(default)]
    pub kind: Option<String>,
    #[serde(default)]
    pub about: Option<String>,
    #[serde(default)]
    pub address: Option<String>,
    #[serde(default)]
    pub website_url: Option<String>,
    #[serde(default)]
    pub instagram_handle: Option<String>,
    #[serde(default)]
    pub opening_info: Option<String>,
}

pub async fn patch(
    State(state): State<Arc<AppState>>,
    AuthedUser(user): AuthedUser,
    Path(id): Path<Uuid>,
    Json(body): Json<PatchVenueBody>,
) -> Result<Json<Venue>, ApiError> {
    // Read current address so we can detect a change → re-geocode.
    let before: (Option<String>,) = sqlx::query_as(
        "SELECT address FROM venues WHERE id = $1 AND owner_user_id = $2 AND deleted_at IS NULL",
    )
    .bind(id)
    .bind(user.id)
    .fetch_optional(&state.pool)
    .await?
    .ok_or(ApiError::NotFound)?;

    if let Some(k) = &body.kind {
        if !VENUE_KINDS.contains(&k.as_str()) {
            return Err(ApiError::BadRequest(format!(
                "kind: must be one of {}",
                VENUE_KINDS.join(", ")
            )));
        }
    }
    if body.name.as_deref().is_some_and(|n| n.trim().is_empty()) {
        return Err(ApiError::BadRequest("name: must not be empty".into()));
    }

    // COALESCE-style update: NULL params leave the column unchanged.
    let row = sqlx::query_as::<_, Venue>(sqlx::AssertSqlSafe(format!(
        r#"
        UPDATE venues SET
            name             = COALESCE($3, name),
            kind             = COALESCE($4, kind),
            about            = COALESCE($5, about),
            address          = COALESCE($6, address),
            website_url      = COALESCE($7, website_url),
            instagram_handle = COALESCE($8, instagram_handle),
            opening_info     = COALESCE($9, opening_info),
            updated_at       = now()
        WHERE id = $1 AND owner_user_id = $2 AND deleted_at IS NULL
        RETURNING {VENUE_COLUMNS}
        "#
    )))
    .bind(id)
    .bind(user.id)
    .bind(body.name.as_deref().map(str::trim))
    .bind(body.kind.as_deref())
    .bind(body.about.as_deref().map(str::trim))
    .bind(body.address.as_deref().map(str::trim))
    .bind(body.website_url.as_deref().map(str::trim))
    .bind(body.instagram_handle.as_deref().map(str::trim))
    .bind(body.opening_info.as_deref().map(str::trim))
    .fetch_one(&state.pool)
    .await?;

    if body.address.is_some() && body.address.as_deref() != before.0.as_deref() {
        let _ = state
            .jobs
            .enqueue(
                JobEvent::VenueGeocode { venue_id: id },
                EnqueueOpts {
                    idempotency_key: Some(format!("venue_geocode:{id}:patch")),
                    ..Default::default()
                },
            )
            .await;
    }
    Ok(Json(row))
}

pub async fn delete(
    State(state): State<Arc<AppState>>,
    AuthedUser(user): AuthedUser,
    Path(id): Path<Uuid>,
) -> Result<StatusCode, ApiError> {
    let affected = sqlx::query(
        r#"UPDATE venues
           SET deleted_at = now(), updated_at = now()
           WHERE id = $1 AND owner_user_id = $2 AND deleted_at IS NULL"#,
    )
    .bind(id)
    .bind(user.id)
    .execute(&state.pool)
    .await?
    .rows_affected();
    if affected == 0 {
        return Err(ApiError::NotFound);
    }
    Ok(StatusCode::NO_CONTENT)
}

// ─────────────────────────────────────────────────────────────────────────────
// venue_artworks — invitation flow (venue side)
// ─────────────────────────────────────────────────────────────────────────────

#[derive(Debug, Serialize, FromRow)]
pub struct VenueArtworkRow {
    pub venue_id: Uuid,
    pub artwork_id: Uuid,
    pub artwork_title: Option<String>,
    pub artist_id: Uuid,
    pub artist_slug: String,
    pub artist_display_name: String,
    pub status: String,
    pub requested_at: chrono::DateTime<chrono::Utc>,
    pub decided_at: Option<chrono::DateTime<chrono::Utc>>,
}

pub async fn list_venue_artworks(
    State(state): State<Arc<AppState>>,
    AuthedUser(user): AuthedUser,
    Path(venue_id): Path<Uuid>,
) -> Result<Json<Vec<VenueArtworkRow>>, ApiError> {
    // Ownership check inline — the JOIN gates it.
    let rows = sqlx::query_as::<_, VenueArtworkRow>(
        r#"
        SELECT va.venue_id, va.artwork_id,
               a.title AS artwork_title,
               ar.id   AS artist_id,
               ar.slug AS artist_slug,
               ar.display_name AS artist_display_name,
               va.status, va.requested_at, va.decided_at
        FROM venue_artworks va
        JOIN venues v   ON v.id = va.venue_id
        JOIN artworks a ON a.id = va.artwork_id
        JOIN artists ar ON ar.id = a.artist_id
        WHERE va.venue_id = $1
          AND v.owner_user_id = $2
          AND v.deleted_at IS NULL
          AND a.deleted_at IS NULL
        ORDER BY va.requested_at DESC
        "#,
    )
    .bind(venue_id)
    .bind(user.id)
    .fetch_all(&state.pool)
    .await?;
    Ok(Json(rows))
}

pub async fn invite_artwork(
    State(state): State<Arc<AppState>>,
    AuthedUser(user): AuthedUser,
    Path((venue_id, artwork_id)): Path<(Uuid, Uuid)>,
) -> Result<StatusCode, ApiError> {
    // Verify the venue exists + belongs to the caller; 404 otherwise.
    let owns_venue: bool = sqlx::query_scalar(
        r#"SELECT EXISTS (
            SELECT 1 FROM venues
            WHERE id = $1 AND owner_user_id = $2 AND deleted_at IS NULL
        )"#,
    )
    .bind(venue_id)
    .bind(user.id)
    .fetch_one(&state.pool)
    .await?;
    if !owns_venue {
        return Err(ApiError::NotFound);
    }

    // Verify the artwork exists + isn't soft-deleted. The artwork may
    // belong to anyone; we're inviting it, not owning it.
    let artwork_exists: bool = sqlx::query_scalar(
        r#"SELECT EXISTS (
            SELECT 1 FROM artworks
            WHERE id = $1 AND deleted_at IS NULL
        )"#,
    )
    .bind(artwork_id)
    .fetch_one(&state.pool)
    .await?;
    if !artwork_exists {
        return Err(ApiError::NotFound);
    }

    // Idempotent INSERT — re-inviting a pending row is a no-op. A
    // previously declined row re-opens to pending (the artist can
    // change their mind, or the venue can re-invite after some
    // back-and-forth).
    sqlx::query(
        r#"
        INSERT INTO venue_artworks (venue_id, artwork_id, status)
        VALUES ($1, $2, 'pending')
        ON CONFLICT (venue_id, artwork_id) DO UPDATE
           SET status = CASE
                          WHEN venue_artworks.status = 'declined' THEN 'pending'
                          ELSE venue_artworks.status
                        END,
               decided_at = CASE
                              WHEN venue_artworks.status = 'declined' THEN NULL
                              ELSE venue_artworks.decided_at
                            END,
               requested_at = CASE
                                WHEN venue_artworks.status = 'declined' THEN now()
                                ELSE venue_artworks.requested_at
                              END
        "#,
    )
    .bind(venue_id)
    .bind(artwork_id)
    .execute(&state.pool)
    .await?;
    Ok(StatusCode::NO_CONTENT)
}

pub async fn uninvite_artwork(
    State(state): State<Arc<AppState>>,
    AuthedUser(user): AuthedUser,
    Path((venue_id, artwork_id)): Path<(Uuid, Uuid)>,
) -> Result<StatusCode, ApiError> {
    // Venue ownership gate.
    let owns_venue: bool = sqlx::query_scalar(
        r#"SELECT EXISTS (
            SELECT 1 FROM venues
            WHERE id = $1 AND owner_user_id = $2 AND deleted_at IS NULL
        )"#,
    )
    .bind(venue_id)
    .bind(user.id)
    .fetch_one(&state.pool)
    .await?;
    if !owns_venue {
        return Err(ApiError::NotFound);
    }
    sqlx::query("DELETE FROM venue_artworks WHERE venue_id = $1 AND artwork_id = $2")
        .bind(venue_id)
        .bind(artwork_id)
        .execute(&state.pool)
        .await?;
    Ok(StatusCode::NO_CONTENT)
}

// ─────────────────────────────────────────────────────────────────────────────
// Artist-side: pending-invitation inbox + accept / decline
// ─────────────────────────────────────────────────────────────────────────────

#[derive(Debug, Serialize, FromRow)]
pub struct VenueRequest {
    pub venue_id: Uuid,
    pub venue_slug: String,
    pub venue_name: String,
    pub venue_kind: String,
    pub venue_city: Option<String>,
    pub venue_country: Option<String>,
    pub artwork_id: Uuid,
    pub artwork_title: Option<String>,
    pub status: String,
    pub requested_at: chrono::DateTime<chrono::Utc>,
}

pub async fn list_venue_requests(
    State(state): State<Arc<AppState>>,
    AuthedUser(user): AuthedUser,
) -> Result<Json<Vec<VenueRequest>>, ApiError> {
    // Resolve the caller's artist row; non-artists get an empty list
    // rather than 404 — keeps the studio inbox surface uniform.
    let artist_id = match current_artist_id(&state.pool, &user).await {
        Ok(id) => id,
        Err(ApiError::NotFound) => return Ok(Json(vec![])),
        Err(e) => return Err(e),
    };
    let rows = sqlx::query_as::<_, VenueRequest>(
        r#"
        SELECT v.id AS venue_id, v.slug AS venue_slug, v.name AS venue_name,
               v.kind AS venue_kind, v.city AS venue_city, v.country AS venue_country,
               a.id  AS artwork_id, a.title AS artwork_title,
               va.status, va.requested_at
        FROM venue_artworks va
        JOIN venues v   ON v.id = va.venue_id
        JOIN artworks a ON a.id = va.artwork_id
        WHERE a.artist_id = $1
          AND v.deleted_at IS NULL
          AND a.deleted_at IS NULL
          AND va.status = 'pending'
        ORDER BY va.requested_at DESC
        "#,
    )
    .bind(artist_id)
    .fetch_all(&state.pool)
    .await?;
    Ok(Json(rows))
}

async fn decide(
    state: &Arc<AppState>,
    user: &ml_art_core::auth::User,
    venue_id: Uuid,
    artwork_id: Uuid,
    next: &str, // 'accepted' | 'declined'
) -> Result<StatusCode, ApiError> {
    let artist_id = current_artist_id(&state.pool, user).await?;
    let affected = sqlx::query(
        r#"
        UPDATE venue_artworks va
           SET status = $4, decided_at = now()
          FROM artworks a
         WHERE va.artwork_id = a.id
           AND va.venue_id    = $1
           AND va.artwork_id  = $2
           AND a.artist_id    = $3
           AND a.deleted_at IS NULL
           AND va.status = 'pending'
        "#,
    )
    .bind(venue_id)
    .bind(artwork_id)
    .bind(artist_id)
    .bind(next)
    .execute(&state.pool)
    .await?
    .rows_affected();
    if affected == 0 {
        // Either: not pending anymore, not the artist's artwork, or
        // venue/artwork doesn't exist. All collapse to 404 — we don't
        // disclose which one.
        return Err(ApiError::NotFound);
    }
    Ok(StatusCode::NO_CONTENT)
}

pub async fn accept_request(
    State(state): State<Arc<AppState>>,
    AuthedUser(user): AuthedUser,
    Path((venue_id, artwork_id)): Path<(Uuid, Uuid)>,
) -> Result<StatusCode, ApiError> {
    decide(&state, &user, venue_id, artwork_id, "accepted").await
}

pub async fn decline_request(
    State(state): State<Arc<AppState>>,
    AuthedUser(user): AuthedUser,
    Path((venue_id, artwork_id)): Path<(Uuid, Uuid)>,
) -> Result<StatusCode, ApiError> {
    decide(&state, &user, venue_id, artwork_id, "declined").await
}

// ─────────────────────────────────────────────────────────────────────────────
// Public reads
// ─────────────────────────────────────────────────────────────────────────────

#[derive(Debug, Deserialize)]
pub struct PublicListParams {
    #[serde(default)]
    pub city: Option<String>,
    #[serde(default)]
    pub cursor: Option<String>,
    #[serde(default)]
    pub limit: Option<i64>,
}

#[derive(Debug, Serialize, FromRow)]
pub struct PublicVenue {
    pub id: Uuid,
    pub slug: String,
    pub name: String,
    pub kind: String,
    pub city: Option<String>,
    pub country: Option<String>,
    pub lat: Option<f64>,
    pub lng: Option<f64>,
    pub website_url: Option<String>,
}

#[derive(Debug, Serialize)]
pub struct PublicListResponse {
    pub items: Vec<PublicVenue>,
    pub next_cursor: Option<String>,
}

pub async fn public_list(
    State(state): State<Arc<AppState>>,
    Query(p): Query<PublicListParams>,
) -> Result<Json<PublicListResponse>, ApiError> {
    let limit = p.limit.unwrap_or(DEFAULT_LIMIT).clamp(1, MAX_LIMIT);
    let offset: i64 = match p.cursor.as_deref() {
        None => 0,
        Some(c) => match PageCursor::decode(c) {
            Ok(p) => p.offset,
            Err(CursorError::Malformed) => {
                return Err(ApiError::BadRequest("cursor: malformed".into()))
            }
            Err(CursorError::OutOfRange) => {
                return Err(ApiError::BadRequest("cursor: out of range".into()))
            }
        },
    };

    let rows: Vec<PublicVenue> = sqlx::query_as(
        r#"
        SELECT id, slug, name, kind, city, country, lat, lng, website_url
        FROM venues
        WHERE deleted_at IS NULL
          AND status = 'active'
          AND ($1::text IS NULL OR lower(city) = lower($1))
        ORDER BY created_at DESC
        LIMIT $2 OFFSET $3
        "#,
    )
    .bind(p.city.as_deref())
    .bind(limit + 1)
    .bind(offset)
    .fetch_all(&state.pool)
    .await?;

    let has_next = rows.len() > limit as usize;
    let items: Vec<PublicVenue> = rows.into_iter().take(limit as usize).collect();
    let next_cursor = has_next.then(|| PageCursor::from_offset(offset + limit).encode());
    Ok(Json(PublicListResponse { items, next_cursor }))
}

#[derive(Debug, Serialize, FromRow)]
pub struct PublicVenueDetail {
    pub id: Uuid,
    pub slug: String,
    pub name: String,
    pub kind: String,
    pub about: Option<String>,
    pub address: Option<String>,
    pub city: Option<String>,
    pub country: Option<String>,
    pub lat: Option<f64>,
    pub lng: Option<f64>,
    pub website_url: Option<String>,
    pub instagram_handle: Option<String>,
    pub opening_info: Option<String>,
}

#[derive(Debug, Serialize, FromRow)]
pub struct PublicVenueArtwork {
    pub artwork_id: Uuid,
    pub title: Option<String>,
    pub artist_id: Uuid,
    pub artist_slug: String,
    pub artist_display_name: String,
    pub primary_image_url: Option<String>,
}

#[derive(Debug, Serialize)]
pub struct PublicVenueDetailResponse {
    #[serde(flatten)]
    pub venue: PublicVenueDetail,
    pub artworks: Vec<PublicVenueArtwork>,
}

pub async fn public_detail(
    State(state): State<Arc<AppState>>,
    Path(slug): Path<String>,
) -> Result<Json<PublicVenueDetailResponse>, ApiError> {
    let venue = sqlx::query_as::<_, PublicVenueDetail>(
        r#"
        SELECT id, slug, name, kind, about, address, city, country,
               lat, lng, website_url, instagram_handle, opening_info
        FROM venues
        WHERE slug = $1 AND deleted_at IS NULL AND status = 'active'
        "#,
    )
    .bind(&slug)
    .fetch_optional(&state.pool)
    .await?
    .ok_or(ApiError::NotFound)?;

    #[derive(FromRow)]
    struct ArtworkDbRow {
        artwork_id: Uuid,
        title: Option<String>,
        artist_id: Uuid,
        artist_slug: String,
        artist_display_name: String,
        s3_key: Option<String>,
    }
    let rows: Vec<ArtworkDbRow> = sqlx::query_as(
        r#"
        SELECT a.id AS artwork_id, a.title,
               ar.id AS artist_id, ar.slug AS artist_slug,
               ar.display_name AS artist_display_name,
               ai.s3_key
        FROM venue_artworks va
        JOIN artworks a ON a.id = va.artwork_id
        JOIN artists ar ON ar.id = a.artist_id
        LEFT JOIN artwork_images ai
               ON ai.artwork_id = a.id
              AND ai.is_primary
              AND ai.moderation_status = 'approved'
        WHERE va.venue_id = $1
          AND va.status = 'accepted'
          AND a.deleted_at IS NULL
          AND a.status = 'published'
          AND ar.deleted_at IS NULL
          AND ar.status = 'active'
        ORDER BY va.decided_at DESC NULLS LAST
        "#,
    )
    .bind(venue.id)
    .fetch_all(&state.pool)
    .await?;

    let artworks: Vec<PublicVenueArtwork> = rows
        .into_iter()
        .map(|r| PublicVenueArtwork {
            artwork_id: r.artwork_id,
            title: r.title,
            artist_id: r.artist_id,
            artist_slug: r.artist_slug,
            artist_display_name: r.artist_display_name,
            primary_image_url: r.s3_key.as_deref().map(ml_art_core::images::url_for_s3_key),
        })
        .collect();

    Ok(Json(PublicVenueDetailResponse { venue, artworks }))
}
