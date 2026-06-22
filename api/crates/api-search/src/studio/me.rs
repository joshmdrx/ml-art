//! `GET /v1/studio/me` — the artist record for the authenticated caller.
//!
//! Returns 404 (intentionally — same shape as the rest of `/v1/studio/*`)
//! when the user has no `artists` row linked. Lets the web client tell
//! "signed-in non-artist" from "signed-in artist" without exposing
//! whether any *other* artist exists.

use axum::{extract::State, Json};
use chrono::{DateTime, Utc};
use ml_art_core::error::ApiError;
use serde::Serialize;
use sqlx::FromRow;
use std::sync::Arc;
use uuid::Uuid;

use crate::extractors::AuthedUser;
use crate::AppState;

/// Studio view of the artist — includes editable fields the public
/// `/v1/artists/:slug` response doesn't surface (`status`,
/// `inquiry_preferences`, `commissioning_preferences`, all timestamps).
#[derive(Debug, Serialize, FromRow)]
pub struct StudioArtist {
    pub id: Uuid,
    pub slug: String,
    pub display_name: String,
    pub bio: Option<String>,
    pub artist_statement: Option<String>,
    pub location: Option<String>,
    pub city: Option<String>,
    pub country: Option<String>,
    pub website_url: Option<String>,
    pub socials: serde_json::Value,
    pub commissioning_preferences: Option<serde_json::Value>,
    pub inquiry_preferences: serde_json::Value,
    pub status: String,
    pub created_at: DateTime<Utc>,
    pub updated_at: DateTime<Utc>,
}

/// Response wrapper: studio artist record + lightweight stats the
/// dashboard surfaces. T-052 adds `follower_count`; future stats join
/// this struct without changing the artist row shape.
#[derive(Debug, Serialize)]
pub struct StudioMe {
    #[serde(flatten)]
    pub artist: StudioArtist,
    pub follower_count: i32,
}

pub async fn current_artist(
    State(state): State<Arc<AppState>>,
    AuthedUser(user): AuthedUser,
) -> Result<Json<StudioMe>, ApiError> {
    let artist: StudioArtist = sqlx::query_as(
        r#"
        SELECT
            id, slug, display_name, bio, artist_statement,
            location, city, country, website_url,
            socials, commissioning_preferences, inquiry_preferences,
            status, created_at, updated_at
        FROM artists
        WHERE user_id = $1
          AND deleted_at IS NULL
        "#,
    )
    .bind(user.id)
    .fetch_optional(&state.pool)
    .await?
    .ok_or(ApiError::NotFound)?;

    let follower_count: i64 =
        sqlx::query_scalar("SELECT count(*) FROM follows WHERE artist_id = $1")
            .bind(artist.id)
            .fetch_one(&state.pool)
            .await
            .unwrap_or(0);

    Ok(Json(StudioMe {
        artist,
        follower_count: follower_count as i32,
    }))
}
