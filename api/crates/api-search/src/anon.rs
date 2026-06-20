//! T-052c — endpoints for anonymous users to queue intents the
//! merge-anonymous handler replays after sign-in.
//!
//! Today this is just `POST /v1/anon/pending/follows/:artist_id` —
//! captures "anonymous viewer X wanted to follow artist Y" so that
//! when X later signs up + the merge bridge fires, the follow is
//! created automatically (without X having to re-click).
//!
//! Future intents (save-to-collection, inquiry-start) plug into the
//! same `anon_pending_actions` table with a different `kind`; the
//! merge handler grows a new match arm. See migration
//! `0018_anon_pending_actions.sql`.
//!
//! Auth: the only credential is the signed `X-Anonymous-Id` header
//! (extracted by `OptionalAnonId`). No bearer required — the entire
//! point is that the caller is signed-out. If the anon cookie isn't
//! present we return 400 (we have nothing to key on).

use axum::{
    extract::{Path, State},
    http::StatusCode,
};
use ml_art_core::{auth::OptionalAnonId, error::ApiError};
use serde_json::json;
use std::sync::Arc;
use uuid::Uuid;

use crate::AppState;

/// Per-anon cap. ~50 pending intents covers any plausible
/// browse-then-sign-up flow; we'd rather reject the 51st than let an
/// abusive caller stuff the table.
const PER_ANON_CAP: i64 = 50;

/// `POST /v1/anon/pending/follows/:artist_id` — queue a "follow this
/// artist when I sign up" intent. Idempotent at both the DB level
/// (unique index on `(anon_id, kind, payload)`) and HTTP level
/// (clicking the same Follow button twice still 204s).
///
/// 404 when the artist doesn't exist (matches the signed-in
/// `POST /v1/me/follows/:artist_id` semantics).
/// 400 when no anon cookie is present (we can't queue without a key).
/// 429 when the per-anon cap is hit.
pub async fn queue_follow(
    State(state): State<Arc<AppState>>,
    Path(artist_id): Path<Uuid>,
    OptionalAnonId(anon): OptionalAnonId,
) -> Result<StatusCode, ApiError> {
    let Some(anon_id) = anon else {
        return Err(ApiError::BadRequest(
            "an anonymous identity cookie is required to queue intent".into(),
        ));
    };

    // 404 on unknown / soft-deleted artist before consuming a row.
    let exists: Option<(Uuid,)> = sqlx::query_as(
        "SELECT id FROM artists WHERE id = $1 AND deleted_at IS NULL",
    )
    .bind(artist_id)
    .fetch_optional(&state.pool)
    .await?;
    if exists.is_none() {
        return Err(ApiError::NotFound);
    }

    // Cap check — drop the row if the queue is full. Race against
    // simultaneous inserts is fine: at worst we end up one over the
    // cap, which is well inside "harmless." Per-anon cap, not per-
    // anon-per-kind, so a noisy follower can't also drown out their
    // own future inquiry intents.
    let count: i64 = sqlx::query_scalar(
        "SELECT count(*) FROM anon_pending_actions WHERE anon_id = $1 AND expires_at > now()",
    )
    .bind(anon_id)
    .fetch_one(&state.pool)
    .await
    .unwrap_or(0);
    if count >= PER_ANON_CAP {
        // Not really rate-limited (caller isn't being abusive in the
        // burst sense), it's that the queue is full and they need to
        // sign in to drain it before queueing more.
        return Err(ApiError::BadRequest(
            "too many pending intents — sign in to apply them".into(),
        ));
    }

    sqlx::query(
        r#"
        INSERT INTO anon_pending_actions (anon_id, kind, payload)
        VALUES ($1, 'follow_artist', $2)
        ON CONFLICT (anon_id, kind, payload) DO NOTHING
        "#,
    )
    .bind(anon_id)
    .bind(json!({ "artist_id": artist_id }))
    .execute(&state.pool)
    .await?;

    Ok(StatusCode::NO_CONTENT)
}
