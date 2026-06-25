//! T-052 — `/v1/me/follows/*` — the signed-in user's follow graph.
//!
//! Schema: see `db/migrations/0015_follows.sql`. Composite PK on
//! `(user_id, artist_id)` lets us use `INSERT ... ON CONFLICT DO NOTHING`
//! for both the happy-path insert and the idempotent double-click case,
//! and the reverse `(artist_id, created_at DESC)` index supports the
//! studio "N followers" count + the future per-publish NotifyFollowers
//! fan-out.

use axum::{
    extract::{Path, State},
    http::{HeaderMap, StatusCode},
    Json,
};
use chrono::{DateTime, Utc};
use ml_art_core::{
    error::ApiError,
    events::{self, EventName},
    images::url_for_s3_key,
    models::Paginated,
};
use serde::Serialize;
use sqlx::FromRow;
use std::sync::Arc;
use uuid::Uuid;

use crate::extractors::AuthedUser;
use crate::AppState;

const LIST_LIMIT: i64 = 200;

// ─────────────────────────────────────────────────────────────────────────────
// Wire shape
// ─────────────────────────────────────────────────────────────────────────────

/// One entry in `GET /v1/me/follows`. Carries enough to render a list
/// item without a second roundtrip: artist identity + a single thumb.
#[derive(Debug, Clone, Serialize)]
pub struct FollowedArtist {
    pub slug: String,
    pub display_name: String,
    pub city: Option<String>,
    pub country: Option<String>,
    /// First representative artwork's primary image. Used as the row
    /// avatar in the list view.
    pub primary_image_url: Option<String>,
    pub followed_at: DateTime<Utc>,
}

// ─────────────────────────────────────────────────────────────────────────────
// POST /v1/me/follows/:artist_id
// ─────────────────────────────────────────────────────────────────────────────

/// Idempotent: double-clicks (and the legitimate "follow → unfollow →
/// follow" cycle) all 204. Returns 404 if the artist doesn't exist or is
/// soft-deleted so we don't accept follows pointing at dead rows.
pub async fn create(
    State(state): State<Arc<AppState>>,
    Path(artist_id): Path<Uuid>,
    AuthedUser(user): AuthedUser,
    headers: HeaderMap,
) -> Result<StatusCode, ApiError> {
    // Cheap existence + status check before the INSERT. We don't
    // care about the race between this SELECT and the INSERT because
    // the worst case is following an artist who's just been
    // soft-deleted — handle that with a public-surface filter (we
    // already do).
    let exists: Option<(Uuid,)> =
        sqlx::query_as("SELECT id FROM artists WHERE id = $1 AND deleted_at IS NULL")
            .bind(artist_id)
            .fetch_optional(&state.pool)
            .await?;
    if exists.is_none() {
        return Err(ApiError::NotFound);
    }

    sqlx::query(
        r#"
        INSERT INTO follows (user_id, artist_id)
        VALUES ($1, $2)
        ON CONFLICT (user_id, artist_id) DO NOTHING
        "#,
    )
    .bind(user.id)
    .bind(artist_id)
    .execute(&state.pool)
    .await?;

    // T-050 — artist_followed. The follows row itself is the canonical
    // state; this event adds the temporal signal (the WHEN, which
    // drives "users who just followed Alice also followed…" later).
    events::emit(
        &state.jobs,
        events::event_log(
            EventName::ArtistFollowed,
            None,
            Some(user.id),
            serde_json::json!({ "artist_id": artist_id }),
            events::extract_request_context(&headers),
        ),
    )
    .await;

    Ok(StatusCode::NO_CONTENT)
}

// ─────────────────────────────────────────────────────────────────────────────
// DELETE /v1/me/follows/:artist_id
// ─────────────────────────────────────────────────────────────────────────────

/// Idempotent: deleting a non-existent follow is still 204. The caller
/// neither needs nor wants to distinguish "wasn't following" from "was
/// and now isn't."
pub async fn delete(
    State(state): State<Arc<AppState>>,
    Path(artist_id): Path<Uuid>,
    AuthedUser(user): AuthedUser,
    headers: HeaderMap,
) -> Result<StatusCode, ApiError> {
    sqlx::query("DELETE FROM follows WHERE user_id = $1 AND artist_id = $2")
        .bind(user.id)
        .bind(artist_id)
        .execute(&state.pool)
        .await?;

    // T-050 — artist_unfollowed. Negative-signal feature for the
    // taste vector and (later) the recommender.
    events::emit(
        &state.jobs,
        events::event_log(
            EventName::ArtistUnfollowed,
            None,
            Some(user.id),
            serde_json::json!({ "artist_id": artist_id }),
            events::extract_request_context(&headers),
        ),
    )
    .await;

    Ok(StatusCode::NO_CONTENT)
}

// ─────────────────────────────────────────────────────────────────────────────
// GET /v1/me/follows
// ─────────────────────────────────────────────────────────────────────────────

/// Returns the user's followed artists in most-recently-followed order.
/// Capped at `LIST_LIMIT` for v1 — pagination lands when someone has
/// >200 follows, which is well beyond realistic v1.x usage.
pub async fn list(
    State(state): State<Arc<AppState>>,
    AuthedUser(user): AuthedUser,
) -> Result<Json<Paginated<FollowedArtist>>, ApiError> {
    let rows: Vec<FollowedArtistRow> = sqlx::query_as(
        r#"
        SELECT
            ar.slug,
            ar.display_name,
            ar.city,
            ar.country,
            -- Cheapest "show me one thumb": the artist's most-recently-
            -- published artwork's primary image. NULL if they have no
            -- published work yet.
            (
                SELECT ai.s3_key
                FROM artworks a
                JOIN artwork_images ai
                  ON ai.artwork_id = a.id
                 AND ai.is_primary
                 AND ai.moderation_status = 'approved'
                WHERE a.artist_id = ar.id
                  AND a.deleted_at IS NULL
                  AND a.status = 'published'
                ORDER BY a.published_at DESC NULLS LAST
                LIMIT 1
            ) AS primary_s3_key,
            f.created_at AS followed_at
        FROM follows f
        JOIN artists ar ON ar.id = f.artist_id
        WHERE f.user_id = $1
          AND ar.deleted_at IS NULL
          AND ar.status = 'active'
        ORDER BY f.created_at DESC
        LIMIT $2
        "#,
    )
    .bind(user.id)
    .bind(LIST_LIMIT)
    .fetch_all(&state.pool)
    .await?;

    let items = rows
        .into_iter()
        .map(|r| FollowedArtist {
            slug: r.slug,
            display_name: r.display_name,
            city: r.city,
            country: r.country,
            primary_image_url: r.primary_s3_key.as_deref().map(url_for_s3_key),
            followed_at: r.followed_at,
        })
        .collect();

    Ok(Json(Paginated {
        items,
        next_cursor: None,
    }))
}

// ─────────────────────────────────────────────────────────────────────────────
// Row type
// ─────────────────────────────────────────────────────────────────────────────

#[derive(FromRow)]
struct FollowedArtistRow {
    slug: String,
    display_name: String,
    city: Option<String>,
    country: Option<String>,
    primary_s3_key: Option<String>,
    followed_at: DateTime<Utc>,
}
