//! `/v1/studio/locations` — CRUD for the artist's `artist_locations`
//! rows (T-038 G3).
//!
//! Unlike the public `/v1/artists/:slug` payload (which only returns
//! geocoded rows), this endpoint returns *every* row the artist owns
//! including pre-geocode ones — the studio UI shows them as "Locating…"
//! so the artist gets feedback that the row exists and is queued.
//!
//! Background geocoding: POST and PATCH (when `address` changes) call
//! `trigger_background_geocode`, which `tokio::spawn`s a Mapbox lookup
//! that writes lat/lng/city/country back to the row. The HTTP response
//! returns immediately with the un-geocoded row; the studio UI re-polls
//! or refetches after a short delay to see the pin land.
//!
//! Ownership: every handler resolves `current_artist_id` first, then
//! gates every SQL operation on `artist_id = $current_artist_id`. We
//! return 404 (never 403) when a location id belongs to another artist,
//! to avoid leaking existence.

use axum::{
    extract::{Path, State},
    http::StatusCode,
    Json,
};
use chrono::{DateTime, Utc};
use ml_art_core::{error::ApiError, geocoding::trigger_background_geocode, models::ArtistLocation};
use serde::{Deserialize, Deserializer, Serialize};
use sqlx::FromRow;
use std::sync::Arc;
use uuid::Uuid;

use crate::extractors::AuthedUser;
use crate::studio::current_artist_id;
use crate::AppState;

// Validation caps. Generous — Mapbox accepts long addresses, names can
// be verbose ("Foo & Friends Project Space at the Old Workshop"), and
// URLs vary wildly.
const MAX_NAME_LEN: usize = 200;
const MAX_ADDRESS_LEN: usize = 500;
const MAX_WEBSITE_LEN: usize = 500;
/// Soft per-artist cap. Reads in the studio UI become noisy past this,
/// and an artist with 50+ galleries probably needs the post-v1 `spaces`
/// model rather than a flat list. Hard-failure here is fine; the cap is
/// well above any real artist's count.
const MAX_LOCATIONS_PER_ARTIST: i64 = 50;

const ALLOWED_KINDS: &[&str] = &["gallery", "studio"];

// ─────────────────────────────────────────────────────────────────────────────
// GET /v1/studio/locations — list this artist's locations (all of them,
// geocoded and not)
// ─────────────────────────────────────────────────────────────────────────────

pub async fn list(
    State(state): State<Arc<AppState>>,
    AuthedUser(user): AuthedUser,
) -> Result<Json<Vec<ArtistLocation>>, ApiError> {
    let artist_id = current_artist_id(&state.pool, &user).await?;

    let rows: Vec<LocationRow> = sqlx::query_as(
        r#"
        SELECT
            id, kind, name, address, city, country, lat, lng,
            website_url, display_order, geocoded_at
        FROM artist_locations
        WHERE artist_id = $1 AND deleted_at IS NULL
        ORDER BY display_order ASC, created_at ASC
        "#,
    )
    .bind(artist_id)
    .fetch_all(&state.pool)
    .await?;

    Ok(Json(rows.into_iter().map(LocationRow::into_dto).collect()))
}

// ─────────────────────────────────────────────────────────────────────────────
// POST /v1/studio/locations — create a new location and trigger geocoding
// ─────────────────────────────────────────────────────────────────────────────

#[derive(Debug, Deserialize)]
pub struct CreateLocation {
    pub kind: String,
    pub name: String,
    pub address: String,
    #[serde(default)]
    pub website_url: Option<String>,
    /// Optional sort position; defaults to "end of the list" if omitted.
    #[serde(default)]
    pub display_order: Option<i32>,
}

pub async fn create(
    State(state): State<Arc<AppState>>,
    AuthedUser(user): AuthedUser,
    Json(body): Json<CreateLocation>,
) -> Result<(StatusCode, Json<ArtistLocation>), ApiError> {
    let artist_id = current_artist_id(&state.pool, &user).await?;

    validate_kind(&body.kind)?;
    validate_name(&body.name)?;
    validate_address(&body.address)?;
    if let Some(url) = body.website_url.as_deref() {
        validate_website(url)?;
    }

    // Enforce the soft per-artist cap.
    let count: (i64,) = sqlx::query_as(
        r#"SELECT count(*)::bigint FROM artist_locations
           WHERE artist_id = $1 AND deleted_at IS NULL"#,
    )
    .bind(artist_id)
    .fetch_one(&state.pool)
    .await?;
    if count.0 >= MAX_LOCATIONS_PER_ARTIST {
        return Err(ApiError::BadRequest(format!(
            "max {MAX_LOCATIONS_PER_ARTIST} locations per artist; delete one first"
        )));
    }

    // Pick a display_order: caller's value if provided, else "after the
    // current max" so new rows append to the end.
    let display_order = match body.display_order {
        Some(n) => n,
        None => {
            let max: (Option<i32>,) = sqlx::query_as(
                r#"SELECT max(display_order) FROM artist_locations
                   WHERE artist_id = $1 AND deleted_at IS NULL"#,
            )
            .bind(artist_id)
            .fetch_one(&state.pool)
            .await?;
            max.0.map(|n| n + 1).unwrap_or(0)
        }
    };

    let row: LocationRow = sqlx::query_as(
        r#"
        INSERT INTO artist_locations
            (artist_id, kind, name, address, website_url, display_order)
        VALUES ($1, $2, $3, $4, $5, $6)
        RETURNING
            id, kind, name, address, city, country, lat, lng,
            website_url, display_order, geocoded_at
        "#,
    )
    .bind(artist_id)
    .bind(body.kind.trim())
    .bind(body.name.trim())
    .bind(body.address.trim())
    .bind(body.website_url.as_deref().map(str::trim))
    .bind(display_order)
    .fetch_one(&state.pool)
    .await?;

    // Fire-and-forget Mapbox lookup. Returns immediately; the UI will
    // see the unset lat/lng and show "Locating…", then refresh.
    trigger_background_geocode(state.geocoder.clone(), state.pool.clone(), row.id);

    Ok((StatusCode::CREATED, Json(row.into_dto())))
}

// ─────────────────────────────────────────────────────────────────────────────
// PATCH /v1/studio/locations/:id — partial update; re-geocode on
// address change
// ─────────────────────────────────────────────────────────────────────────────

#[derive(Debug, Deserialize)]
pub struct PatchLocation {
    #[serde(default)]
    pub kind: Option<String>,
    #[serde(default)]
    pub name: Option<String>,
    #[serde(default)]
    pub address: Option<String>,
    /// Outer Some = key present; inner None = explicitly set NULL.
    /// Plain `Option<Option<T>>` + `#[serde(default)]` doesn't actually
    /// distinguish missing-key from `null` (both come through as outer
    /// `None`) — `deserialize_double_option` is the small helper that
    /// gives us real PATCH semantics: missing → `None`, null →
    /// `Some(None)`, string → `Some(Some(s))`.
    #[serde(default, deserialize_with = "deserialize_double_option")]
    pub website_url: Option<Option<String>>,
    #[serde(default)]
    pub display_order: Option<i32>,
}

pub async fn patch(
    State(state): State<Arc<AppState>>,
    AuthedUser(user): AuthedUser,
    Path(id): Path<Uuid>,
    Json(body): Json<PatchLocation>,
) -> Result<Json<ArtistLocation>, ApiError> {
    let artist_id = current_artist_id(&state.pool, &user).await?;

    if let Some(k) = body.kind.as_deref() {
        validate_kind(k)?;
    }
    if let Some(n) = body.name.as_deref() {
        validate_name(n)?;
    }
    if let Some(a) = body.address.as_deref() {
        validate_address(a)?;
    }
    if let Some(Some(url)) = &body.website_url {
        validate_website(url)?;
    }

    // When `address` changes, we clear the geocoded shadow fields so
    // the geocode job re-runs against the new address (mirrors the
    // `location` clearing pattern in studio::settings).
    let address_changing = body.address.is_some();

    let updated: Option<LocationRow> = sqlx::query_as(
        r#"
        UPDATE artist_locations SET
            kind          = COALESCE($3, kind),
            name          = COALESCE($4, name),
            address       = COALESCE($5, address),
            website_url   = CASE WHEN $6::boolean THEN $7 ELSE website_url END,
            display_order = COALESCE($8, display_order),
            lat           = CASE WHEN $9::boolean THEN NULL ELSE lat END,
            lng           = CASE WHEN $9::boolean THEN NULL ELSE lng END,
            city          = CASE WHEN $9::boolean THEN NULL ELSE city END,
            country       = CASE WHEN $9::boolean THEN NULL ELSE country END,
            geocoded_at   = CASE WHEN $9::boolean THEN NULL ELSE geocoded_at END,
            updated_at    = now()
        WHERE id = $1 AND artist_id = $2 AND deleted_at IS NULL
        RETURNING
            id, kind, name, address, city, country, lat, lng,
            website_url, display_order, geocoded_at
        "#,
    )
    .bind(id)
    .bind(artist_id)
    .bind(body.kind.as_deref().map(str::trim))
    .bind(body.name.as_deref().map(str::trim))
    .bind(body.address.as_deref().map(str::trim))
    .bind(body.website_url.is_some())
    .bind(
        body.website_url
            .clone()
            .flatten()
            .map(|s| s.trim().to_string()),
    )
    .bind(body.display_order)
    .bind(address_changing)
    .fetch_optional(&state.pool)
    .await?;

    let row = updated.ok_or(ApiError::NotFound)?;

    if address_changing {
        trigger_background_geocode(state.geocoder.clone(), state.pool.clone(), row.id);
    }

    Ok(Json(row.into_dto()))
}

// ─────────────────────────────────────────────────────────────────────────────
// DELETE /v1/studio/locations/:id — soft delete
// ─────────────────────────────────────────────────────────────────────────────

#[derive(Debug, Serialize)]
pub struct DeleteAck {
    pub id: Uuid,
    pub deleted: bool,
}

pub async fn delete(
    State(state): State<Arc<AppState>>,
    AuthedUser(user): AuthedUser,
    Path(id): Path<Uuid>,
) -> Result<Json<DeleteAck>, ApiError> {
    let artist_id = current_artist_id(&state.pool, &user).await?;

    let result = sqlx::query(
        r#"
        UPDATE artist_locations
        SET deleted_at = now(), updated_at = now()
        WHERE id = $1 AND artist_id = $2 AND deleted_at IS NULL
        "#,
    )
    .bind(id)
    .bind(artist_id)
    .execute(&state.pool)
    .await?;

    if result.rows_affected() == 0 {
        return Err(ApiError::NotFound);
    }

    Ok(Json(DeleteAck { id, deleted: true }))
}

/// Serde helper: deserialize a field into `Option<Option<T>>` such that:
///   missing key → `None`
///   `null`      → `Some(None)`
///   value       → `Some(Some(value))`
///
/// Without this, plain `#[serde(default)]` collapses both `null` and
/// "missing" into outer `None`, making it impossible to clear a column
/// over PATCH via JSON `null`. Generic so the same helper can be lifted
/// elsewhere if we want real PATCH semantics on other endpoints.
fn deserialize_double_option<'de, T, D>(deserializer: D) -> Result<Option<Option<T>>, D::Error>
where
    T: Deserialize<'de>,
    D: Deserializer<'de>,
{
    Option::<T>::deserialize(deserializer).map(Some)
}

// ─────────────────────────────────────────────────────────────────────────────
// Validation helpers
// ─────────────────────────────────────────────────────────────────────────────

fn validate_kind(kind: &str) -> Result<(), ApiError> {
    if !ALLOWED_KINDS.contains(&kind.trim()) {
        return Err(ApiError::BadRequest(format!(
            "kind must be one of: {ALLOWED_KINDS:?}"
        )));
    }
    Ok(())
}

fn validate_name(name: &str) -> Result<(), ApiError> {
    let n = name.trim();
    if n.is_empty() {
        return Err(ApiError::BadRequest("name is required".into()));
    }
    if n.len() > MAX_NAME_LEN {
        return Err(ApiError::BadRequest(format!(
            "name exceeds {MAX_NAME_LEN}-char limit"
        )));
    }
    Ok(())
}

fn validate_address(addr: &str) -> Result<(), ApiError> {
    let a = addr.trim();
    if a.is_empty() {
        return Err(ApiError::BadRequest("address is required".into()));
    }
    if a.len() > MAX_ADDRESS_LEN {
        return Err(ApiError::BadRequest(format!(
            "address exceeds {MAX_ADDRESS_LEN}-char limit"
        )));
    }
    Ok(())
}

fn validate_website(url: &str) -> Result<(), ApiError> {
    let u = url.trim();
    if u.len() > MAX_WEBSITE_LEN {
        return Err(ApiError::BadRequest(format!(
            "website_url exceeds {MAX_WEBSITE_LEN}-char limit"
        )));
    }
    if !u.is_empty() && !u.starts_with("http://") && !u.starts_with("https://") {
        return Err(ApiError::BadRequest(
            "website_url must start with http:// or https://".into(),
        ));
    }
    Ok(())
}

// ─────────────────────────────────────────────────────────────────────────────
// Row types
// ─────────────────────────────────────────────────────────────────────────────

#[derive(FromRow)]
struct LocationRow {
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

impl LocationRow {
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
