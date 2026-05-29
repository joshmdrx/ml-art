//! `/v1/onboarding/*` — T-012 Phase 1.
//!
//! The minimum surface needed to turn a signed-in user (who has a
//! `users` row but no `artists` row) into a real artist with a portfolio
//! they can publish. Two endpoints:
//!
//! - `POST /v1/onboarding/start` — mints an `artists` row with
//!   `status='pending'`, links `user_id`, flips `users.is_artist=true`.
//!   The wizard then takes the artist through the existing studio
//!   surfaces (artwork CRUD, locations, settings) — no new write
//!   endpoints needed for those, just orchestration in the UI.
//! - `POST /v1/onboarding/complete` — flips `status` from `pending` to
//!   `active`, which makes the artist visible on every public surface.
//!   Idempotent on `active` so a re-submit doesn't error.
//!
//! What's intentionally NOT here (deferred to a later T-012 phase, once
//! Inngest is wired):
//! - Website-scrape pre-fill (`POST /v1/onboarding/scrape`)
//! - LLM-extracted artwork metadata (`POST /v1/onboarding/extract`)
//!
//! See `decisions.md` and TODO.md for the staged plan.

use axum::{extract::State, http::StatusCode, Json};
use ml_art_core::{db::Pool, error::ApiError};
use serde::Deserialize;
use std::sync::Arc;
use uuid::Uuid;

use crate::extractors::AuthedUser;
use crate::studio::me::StudioArtist;
use crate::AppState;

const MAX_DISPLAY_NAME_LEN: usize = 100;
const MAX_LOCATION_LEN: usize = 200;
/// Upper bound on slug-collision retries. With ~100 attempts we can
/// onboard ~99 artists named "Jane Doe" before we have to escalate;
/// that's well above any plausible v1 collision profile. Past the cap
/// we surface a 500 so an operator can intervene rather than minting
/// a 50-character random slug nobody can read.
const MAX_SLUG_ATTEMPTS: u32 = 100;

#[derive(Debug, Deserialize)]
pub struct StartBody {
    pub display_name: String,
    /// Free-text "Berlin, Germany" — geocoded into `city`/`country`/
    /// `lat`/`lng` by the existing artist-geocoder job (when the
    /// Inngest runtime lands; for now the row keeps the raw string).
    #[serde(default)]
    pub location: Option<String>,
}

pub async fn start(
    State(state): State<Arc<AppState>>,
    AuthedUser(user): AuthedUser,
    Json(body): Json<StartBody>,
) -> Result<(StatusCode, Json<StudioArtist>), ApiError> {
    let display_name = body.display_name.trim();
    if display_name.is_empty() {
        return Err(ApiError::BadRequest("display_name is required".into()));
    }
    if display_name.chars().count() > MAX_DISPLAY_NAME_LEN {
        return Err(ApiError::BadRequest(format!(
            "display_name exceeds {MAX_DISPLAY_NAME_LEN}-char limit"
        )));
    }
    let location = body
        .location
        .as_deref()
        .map(str::trim)
        .filter(|s| !s.is_empty());
    if let Some(loc) = location {
        if loc.chars().count() > MAX_LOCATION_LEN {
            return Err(ApiError::BadRequest(format!(
                "location exceeds {MAX_LOCATION_LEN}-char limit"
            )));
        }
    }

    // Already an artist? Bail with a 400 + clear detail. We don't use
    // 409 Conflict because `ApiError` doesn't carry that variant yet;
    // a follow-up can sharpen it if we get a real call-site that needs
    // to distinguish "you already onboarded" from "your input is bad."
    let existing: Option<(Uuid,)> =
        sqlx::query_as(r#"SELECT id FROM artists WHERE user_id = $1 AND deleted_at IS NULL"#)
            .bind(user.id)
            .fetch_optional(&state.pool)
            .await?;
    if existing.is_some() {
        return Err(ApiError::BadRequest(
            "you already have an artist profile".into(),
        ));
    }

    let slug = generate_unique_slug(&state.pool, display_name).await?;

    // INSERT into artists. `inquiry_preferences` is NOT NULL, so we
    // give it a sensible platform-inbox default; the artist can edit
    // via /v1/studio/settings later. `status='pending'` is the v1
    // gate — the artist is not yet on public surfaces.
    let artist: StudioArtist = sqlx::query_as(
        r#"
        INSERT INTO artists (
            user_id, slug, display_name, location,
            inquiry_preferences, status
        )
        VALUES ($1, $2, $3, $4, '{"type":"platform"}'::jsonb, 'pending')
        RETURNING
            id, slug, display_name, bio, artist_statement,
            location, city, country, website_url,
            socials, commissioning_preferences, inquiry_preferences,
            status, created_at, updated_at
        "#,
    )
    .bind(user.id)
    .bind(&slug)
    .bind(display_name)
    .bind(location)
    .fetch_one(&state.pool)
    .await?;

    // Mark the user row as an artist. Cosmetic for now (no read path
    // gates on it), but matches the schema's intent and gives the web
    // client a single boolean to surface "you're an artist now."
    sqlx::query(r#"UPDATE users SET is_artist = true, updated_at = now() WHERE id = $1"#)
        .bind(user.id)
        .execute(&state.pool)
        .await?;

    Ok((StatusCode::CREATED, Json(artist)))
}

/// Flip the caller's artist `status` from `pending` → `active`. Returns
/// 404 if the caller has no artist row at all. Idempotent when the
/// artist is already `active` (returns the unchanged row, 200).
pub async fn complete(
    State(state): State<Arc<AppState>>,
    AuthedUser(user): AuthedUser,
) -> Result<Json<StudioArtist>, ApiError> {
    let updated: Option<StudioArtist> = sqlx::query_as(
        r#"
        UPDATE artists SET
            status = CASE WHEN status = 'pending' THEN 'active' ELSE status END,
            updated_at = CASE WHEN status = 'pending' THEN now() ELSE updated_at END
        WHERE user_id = $1 AND deleted_at IS NULL
        RETURNING
            id, slug, display_name, bio, artist_statement,
            location, city, country, website_url,
            socials, commissioning_preferences, inquiry_preferences,
            status, created_at, updated_at
        "#,
    )
    .bind(user.id)
    .fetch_optional(&state.pool)
    .await?;

    updated.map(Json).ok_or(ApiError::NotFound)
}

// ─────────────────────────────────────────────────────────────────────────────
// Slug generation
// ─────────────────────────────────────────────────────────────────────────────

/// Normalize a display name into a URL-safe slug:
///   "Jane Doe"   → "jane-doe"
///   "Jürgen Müller" → "j-rgen-m-ller" (non-ASCII replaced)
///   "  -- "      → "artist" (fallback when nothing survives)
///
/// Pure function — tested separately so the integration tests don't
/// have to spin up Postgres just to assert the slug shape.
pub fn slugify(name: &str) -> String {
    let mut s = String::with_capacity(name.len());
    let mut last_was_dash = true; // suppress leading dashes
    for ch in name.chars() {
        if ch.is_ascii_alphanumeric() {
            s.push(ch.to_ascii_lowercase());
            last_was_dash = false;
        } else if !last_was_dash {
            s.push('-');
            last_was_dash = true;
        }
    }
    // Strip trailing dash if any.
    while s.ends_with('-') {
        s.pop();
    }
    if s.is_empty() {
        return "artist".to_string();
    }
    s
}

/// Try `slugify(display_name)` first; if taken, append `-2`, `-3`, …
/// until we find a free one. Each candidate is a single round trip;
/// for the v1 collision profile (~zero) we expect exactly one query.
async fn generate_unique_slug(pool: &Pool, display_name: &str) -> Result<String, ApiError> {
    let base = slugify(display_name);
    for i in 0..MAX_SLUG_ATTEMPTS {
        let candidate = if i == 0 {
            base.clone()
        } else {
            format!("{base}-{}", i + 1)
        };
        let exists: (bool,) =
            sqlx::query_as(r#"SELECT EXISTS(SELECT 1 FROM artists WHERE slug = $1)"#)
                .bind(&candidate)
                .fetch_one(pool)
                .await?;
        if !exists.0 {
            return Ok(candidate);
        }
    }
    Err(ApiError::Internal(anyhow::anyhow!(
        "couldn't allocate a unique slug for {display_name:?} after {MAX_SLUG_ATTEMPTS} attempts"
    )))
}

// ─────────────────────────────────────────────────────────────────────────────
// Unit tests for `slugify`
// ─────────────────────────────────────────────────────────────────────────────

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn slugify_plain_name() {
        assert_eq!(slugify("Jane Doe"), "jane-doe");
    }

    #[test]
    fn slugify_collapses_whitespace_and_punctuation() {
        assert_eq!(slugify("Jane   Doe!!"), "jane-doe");
        assert_eq!(slugify("Foo & Bar"), "foo-bar");
    }

    #[test]
    fn slugify_drops_leading_trailing_dashes() {
        assert_eq!(slugify("--Jane--"), "jane");
        assert_eq!(slugify("!!!"), "artist");
    }

    #[test]
    fn slugify_lowercases() {
        assert_eq!(slugify("ALL CAPS"), "all-caps");
    }

    #[test]
    fn slugify_non_ascii_falls_back_to_dashes() {
        // We don't attempt unicode transliteration in v1 — the artist
        // can pick a different display name or edit the slug later via
        // admin tooling.
        assert_eq!(slugify("Jürgen Müller"), "j-rgen-m-ller");
    }

    #[test]
    fn slugify_empty_input() {
        assert_eq!(slugify(""), "artist");
        assert_eq!(slugify("   "), "artist");
    }
}
