//! `PATCH /v1/studio/settings` — the artist's own profile, statement,
//! and visibility toggle.
//!
//! Editable here:
//!   - `bio`, `artist_statement`
//!   - `location` (free-text display string — geocoded to city/country
//!     by an async job, not via this endpoint)
//!   - `website_url`, `socials` (jsonb)
//!   - `commissioning_preferences`, `inquiry_preferences` (jsonb)
//!   - `status` — `'active'` (Published) or `'paused'` (Unpublished).
//!     Setting `'paused'` removes the artist from search, neighborhood
//!     listings, and any other public surface; their own studio still
//!     sees everything. Migration to `'pending'` or `'rejected'`
//!     happens via admin tooling, not this endpoint.
//!
//! NOT editable here:
//!   - `display_name`, `slug` — those touch URLs and analytics, go
//!     through a future "rename artist" admin path
//!   - `user_id`, timestamps — internal
//!   - geocoded fields (`city`/`country`/`lat`/`lng`) — derived from
//!     `location` by the geocode job, never directly settable

use axum::{extract::State, Json};
use ml_art_core::error::ApiError;
use serde::Deserialize;
use std::sync::Arc;

use crate::extractors::AuthedUser;
use crate::studio::current_artist_id;
use crate::studio::me::StudioArtist;
use crate::AppState;

const MAX_BIO_LEN: usize = 4_000;
const MAX_STATEMENT_LEN: usize = 8_000;
const MAX_LOCATION_LEN: usize = 200;
const MAX_WEBSITE_LEN: usize = 500;

/// Two states an artist can self-toggle into. `pending` / `rejected`
/// are admin-controlled, deliberately not accepted here.
const SELF_SERVE_STATUSES: &[&str] = &["active", "paused"];

#[derive(Debug, Deserialize)]
pub struct PatchSettings {
    // T-072 — each clearable field uses `deserialize_double_option` so
    // `null` lands as `Some(None)` (clear column), distinct from
    // absent (`None` — leave alone). Without the helper, serde's
    // default collapses both into `None` and "clear via null" silently
    // never fires. See `serde_helpers::deserialize_double_option`.
    #[serde(
        default,
        deserialize_with = "crate::serde_helpers::deserialize_double_option"
    )]
    pub bio: Option<Option<String>>,
    #[serde(
        default,
        deserialize_with = "crate::serde_helpers::deserialize_double_option"
    )]
    pub artist_statement: Option<Option<String>>,
    #[serde(
        default,
        deserialize_with = "crate::serde_helpers::deserialize_double_option"
    )]
    pub location: Option<Option<String>>,
    #[serde(
        default,
        deserialize_with = "crate::serde_helpers::deserialize_double_option"
    )]
    pub website_url: Option<Option<String>>,
    /// `socials` is `NOT NULL DEFAULT '{}'` in the schema, so a present
    /// value replaces wholesale (no null branch — not clearable).
    #[serde(default)]
    pub socials: Option<serde_json::Value>,
    #[serde(
        default,
        deserialize_with = "crate::serde_helpers::deserialize_double_option"
    )]
    pub commissioning_preferences: Option<Option<serde_json::Value>>,
    #[serde(default)]
    pub inquiry_preferences: Option<serde_json::Value>,
    /// `'active'` (Published) or `'paused'` (Unpublished).
    #[serde(default)]
    pub status: Option<String>,
}

pub async fn patch(
    State(state): State<Arc<AppState>>,
    AuthedUser(user): AuthedUser,
    Json(body): Json<PatchSettings>,
) -> Result<Json<StudioArtist>, ApiError> {
    let artist_id = current_artist_id(&state.pool, &user).await?;

    // Validation pass before SQL.
    if let Some(Some(b)) = &body.bio {
        if b.trim().len() > MAX_BIO_LEN {
            return Err(ApiError::BadRequest(format!(
                "bio exceeds {MAX_BIO_LEN}-char limit"
            )));
        }
    }
    if let Some(Some(s)) = &body.artist_statement {
        if s.trim().len() > MAX_STATEMENT_LEN {
            return Err(ApiError::BadRequest(format!(
                "artist_statement exceeds {MAX_STATEMENT_LEN}-char limit"
            )));
        }
    }
    if let Some(Some(loc)) = &body.location {
        if loc.trim().len() > MAX_LOCATION_LEN {
            return Err(ApiError::BadRequest(format!(
                "location exceeds {MAX_LOCATION_LEN}-char limit"
            )));
        }
    }
    if let Some(Some(url)) = &body.website_url {
        if url.trim().len() > MAX_WEBSITE_LEN {
            return Err(ApiError::BadRequest(format!(
                "website_url exceeds {MAX_WEBSITE_LEN}-char limit"
            )));
        }
        // Loose URL sanity check — accept http(s) only. A `.parse::<Url>()`
        // would be stricter; this is what we tolerate from real users.
        let u = url.trim();
        if !u.is_empty() && !u.starts_with("http://") && !u.starts_with("https://") {
            return Err(ApiError::BadRequest(
                "website_url must start with http:// or https://".into(),
            ));
        }
    }
    if let Some(s) = &body.status {
        if !SELF_SERVE_STATUSES.contains(&s.as_str()) {
            return Err(ApiError::BadRequest(format!(
                "status must be one of: {SELF_SERVE_STATUSES:?}"
            )));
        }
    }

    // Touching geocoded fields would invalidate the city/country/lat/lng;
    // we clear them when `location` changes so the geocode job re-runs.
    let clears_geocoded = body.location.is_some();

    let updated: Option<StudioArtist> = sqlx::query_as(
        r#"
        UPDATE artists SET
            bio                       = CASE WHEN $2::boolean THEN $3 ELSE bio END,
            artist_statement          = CASE WHEN $4::boolean THEN $5 ELSE artist_statement END,
            location                  = CASE WHEN $6::boolean THEN $7 ELSE location END,
            website_url               = CASE WHEN $8::boolean THEN $9 ELSE website_url END,
            socials                   = COALESCE($10::jsonb, socials),
            commissioning_preferences = CASE WHEN $11::boolean THEN $12::jsonb ELSE commissioning_preferences END,
            inquiry_preferences       = COALESCE($13::jsonb, inquiry_preferences),
            status                    = COALESCE($14, status),
            -- When location changes, clear the geocoded shadow fields so
            -- the geocode job re-runs against the new value.
            city                      = CASE WHEN $15::boolean THEN NULL ELSE city END,
            country                   = CASE WHEN $15::boolean THEN NULL ELSE country END,
            lat                       = CASE WHEN $15::boolean THEN NULL ELSE lat END,
            lng                       = CASE WHEN $15::boolean THEN NULL ELSE lng END,
            geocoded_at               = CASE WHEN $15::boolean THEN NULL ELSE geocoded_at END,
            updated_at                = now()
        WHERE id = $1 AND deleted_at IS NULL
        RETURNING
            id, slug, display_name, bio, artist_statement,
            location, city, country, website_url,
            socials, commissioning_preferences, inquiry_preferences,
            status, created_at, updated_at
        "#,
    )
    .bind(artist_id)
    .bind(body.bio.is_some())
    .bind(body.bio.flatten().map(|s| s.trim().to_string()))
    .bind(body.artist_statement.is_some())
    .bind(body.artist_statement.flatten().map(|s| s.trim().to_string()))
    .bind(body.location.is_some())
    .bind(body.location.flatten().map(|s| s.trim().to_string()))
    .bind(body.website_url.is_some())
    .bind(body.website_url.flatten().map(|s| s.trim().to_string()))
    .bind(body.socials.clone())
    .bind(body.commissioning_preferences.is_some())
    .bind(body.commissioning_preferences.clone().flatten())
    .bind(body.inquiry_preferences.clone())
    .bind(body.status.as_deref())
    .bind(clears_geocoded)
    .fetch_optional(&state.pool)
    .await?;

    updated.map(Json).ok_or(ApiError::NotFound)
}
