//! T-086 — private preview links for draft profiles (public side).
//!
//! The organisation an admin built a draft for receives
//! `/preview/<token>`. Holding the token is the whole credential:
//!
//!   - `GET  /v1/preview/:token`        — view the draft (no auth)
//!   - `POST /v1/preview/:token/claim`  — link it to the signed-in user;
//!     it stays `pending` (unpublished) until they publish it themselves.
//!     Refused (403) for admin accounts, so checking a link can't claim it
//!   - `POST /v1/preview/:token/remove` — delete the draft (no auth: the
//!     person holding the link can always say no)
//!
//! Every path answers 404 for a token that is malformed, unknown,
//! expired, revoked or already claimed, so a probe can't tell those
//! apart. Draft artworks are included in the preview (the admin builds
//! the example as drafts); images are limited to moderation-approved
//! ones, same as every other surface. The web page marks itself
//! `noindex` and `/preview` is disallowed in robots.txt.
//!
//! Admin side (create, edit, mint links, take down): `admin::drafts`.

use crate::admin::drafts::{clear_editing_sessions, remove_draft, revoke_live_links};
use crate::artist::{fetch_artist_row_by_id, load_profile, Profile, PREVIEW_ARTWORK_STATUSES};
use crate::extractors::AuthedUser;
use crate::studio::me::StudioArtist;
use crate::AppState;
use axum::{
    extract::{Path, State},
    http::StatusCode,
    Json,
};
use chrono::{DateTime, Utc};
use ml_art_core::{
    admin::{action, audit, target},
    db::Pool,
    error::ApiError,
    models::{ArtistFull, ArtistLocation, ArtworkSummary},
    preview_token,
};
use serde::Serialize;
use std::sync::Arc;
use uuid::Uuid;

#[derive(Debug, Serialize)]
pub struct PreviewResponse {
    pub artist: ArtistFull,
    pub artworks: Vec<ArtworkSummary>,
    pub locations: Vec<ArtistLocation>,
    pub expires_at: DateTime<Utc>,
}

/// Resolve a presented token to `(link_id, artist_id, expires_at)` for a
/// live link on an unclaimed, undeleted draft. `None` covers every
/// failure mode on purpose.
async fn resolve_live(
    pool: &Pool,
    token: &str,
) -> Result<Option<(Uuid, Uuid, DateTime<Utc>)>, ApiError> {
    if !preview_token::looks_valid(token) {
        return Ok(None);
    }
    Ok(sqlx::query_as(
        r#"
        SELECT l.id, l.artist_id, l.expires_at
        FROM artist_preview_links l
        JOIN artists a ON a.id = l.artist_id
        WHERE l.token_hash = $1
          AND l.revoked_at IS NULL
          AND l.claimed_at IS NULL
          AND l.expires_at > now()
          AND a.is_prebuilt
          AND a.user_id IS NULL
          AND a.deleted_at IS NULL
        "#,
    )
    .bind(preview_token::hash(token))
    .fetch_optional(pool)
    .await?)
}

pub async fn view(
    State(state): State<Arc<AppState>>,
    Path(token): Path<String>,
) -> Result<Json<PreviewResponse>, ApiError> {
    let (_, artist_id, expires_at) = resolve_live(&state.pool, &token)
        .await?
        .ok_or(ApiError::NotFound)?;
    let row = fetch_artist_row_by_id(&state.pool, artist_id)
        .await?
        .ok_or(ApiError::NotFound)?;
    let Profile {
        artist,
        artworks,
        locations,
    } = load_profile(&state.pool, row, PREVIEW_ARTWORK_STATUSES).await?;
    Ok(Json(PreviewResponse {
        artist,
        artworks,
        locations,
        expires_at,
    }))
}

pub async fn claim(
    State(state): State<Arc<AppState>>,
    AuthedUser(user): AuthedUser,
    Path(token): Path<String>,
) -> Result<Json<StudioArtist>, ApiError> {
    let (link_id, artist_id, _) = resolve_live(&state.pool, &token)
        .await?
        .ok_or(ApiError::NotFound)?;

    // Admins open these links to check them before sending; a stray click
    // must not take the organisation's page. Admins use
    // `POST /v1/admin/drafts/:id/assign` when they mean to move a page.
    if user.is_admin {
        return Err(ApiError::Forbidden);
    }

    // One profile per account (migration 0010). Say so plainly rather
    // than letting the unique index turn it into a 500.
    let existing: Option<(Uuid,)> =
        sqlx::query_as("SELECT id FROM artists WHERE user_id = $1 AND deleted_at IS NULL")
            .bind(user.id)
            .fetch_optional(&state.pool)
            .await?;
    if existing.is_some() {
        return Err(ApiError::Conflict(
            "this account already has a profile on Wander; sign in with a different account to claim this page".into(),
        ));
    }

    let mut tx = state.pool.begin().await?;
    // Re-check under a row lock so two concurrent claims can't both win.
    let locked: Option<(Uuid,)> = sqlx::query_as(
        r#"
        SELECT id FROM artist_preview_links
        WHERE id = $1 AND revoked_at IS NULL AND claimed_at IS NULL AND expires_at > now()
        FOR UPDATE
        "#,
    )
    .bind(link_id)
    .fetch_optional(&mut *tx)
    .await?;
    if locked.is_none() {
        return Err(ApiError::NotFound);
    }

    let artist: Option<StudioArtist> = sqlx::query_as(
        r#"
        UPDATE artists SET user_id = $2, updated_at = now()
        WHERE id = $1 AND is_prebuilt AND user_id IS NULL AND deleted_at IS NULL
        RETURNING
            id, slug, display_name, bio, artist_statement,
            location, city, country, website_url,
            socials, commissioning_preferences, inquiry_preferences,
            status, entity_type, created_at, updated_at
        "#,
    )
    .bind(artist_id)
    .bind(user.id)
    .fetch_optional(&mut *tx)
    .await?;
    let artist = artist.ok_or(ApiError::NotFound)?;

    sqlx::query(
        r#"
        UPDATE artist_preview_links
        SET claimed_at = now(), claimed_by_user_id = $2, updated_at = now()
        WHERE id = $1
        "#,
    )
    .bind(link_id)
    .bind(user.id)
    .execute(&mut *tx)
    .await?;
    revoke_live_links(&mut tx, artist_id).await?;
    clear_editing_sessions(&mut tx, artist_id).await?;
    sqlx::query("UPDATE users SET is_artist = true, updated_at = now() WHERE id = $1")
        .bind(user.id)
        .execute(&mut *tx)
        .await?;
    tx.commit().await?;

    Ok(Json(artist))
}

pub async fn remove(
    State(state): State<Arc<AppState>>,
    Path(token): Path<String>,
) -> Result<StatusCode, ApiError> {
    let (_, artist_id, _) = resolve_live(&state.pool, &token)
        .await?
        .ok_or(ApiError::NotFound)?;
    // System-context audit row (no admin): the organisation said no.
    audit::record(
        &state.pool,
        None,
        action::DRAFT_TAKEDOWN,
        target::ARTIST,
        Some(artist_id),
        None::<&()>,
        None::<&()>,
        Some(serde_json::json!({ "via": "preview_link" })),
    )
    .await?;
    remove_draft(&state.pool, artist_id).await?;
    Ok(StatusCode::NO_CONTENT)
}
