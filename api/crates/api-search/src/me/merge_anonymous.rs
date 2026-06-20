//! `POST /v1/me/merge-anonymous` — T-033.
//!
//! Called once after sign-in, when the same browser session has both
//! the Clerk-issued user identity AND a still-valid anonymous-id cookie.
//! Walks every table keyed on `anonymous_id` and stamps the now-known
//! `user_id` onto rows that lack one.
//!
//! Idempotent: the `WHERE user_id IS NULL` predicate makes a second
//! call a no-op (the first call set the column on the matching rows).
//!
//! Ownership safety: we never overwrite an existing `user_id`. If the
//! cookie has been shared across users (account switching, multiple
//! profiles on one machine), the first signed-in user "wins" and
//! later users see no rows merged from those shared anon-trails.
//!
//! We preserve `anonymous_id` on the row — it's the original trace,
//! not PII — and just add the user link. Keeps the audit story clean.
//!
//! Surface today:
//!
//! - `uploads.anonymous_id` (visual-search uploads from anon browsers)
//! - `events.anonymous_id`  (behavioral analytics; no writers yet but
//!   the column + indexes exist per migration 0006)
//! - `anon_pending_actions` — T-052c. Captured intents (today: queued
//!   follow-artist clicks) get drained + replayed onto the user.
//!   `follow_artist` becomes a `follows` row; the pending row is
//!   deleted whether or not it replayed cleanly (the alternative is
//!   surprising the user weeks later if they sign in after the
//!   feature is half-broken).
//!
//! All updates run in a single transaction so a partial merge can't
//! leave the user only partly linked.

use axum::{extract::State, Json};
use ml_art_core::{auth::OptionalAnonId, error::ApiError};
use serde::Serialize;
use std::sync::Arc;

use crate::extractors::AuthedUser;
use crate::AppState;

#[derive(Debug, Serialize)]
pub struct MergeResponse {
    /// Number of `uploads` rows linked to the user.
    pub uploads_merged: u64,
    /// Number of `events` rows linked to the user. Always 0 today
    /// (no events writers); shape is here so the client doesn't break
    /// when T-016 starts writing events.
    pub events_merged: u64,
    /// T-052c — pending follow-artist intents replayed as follows.
    /// Idempotent: existing follows aren't duplicated. Zero when the
    /// anon user never clicked Follow before signing in.
    #[serde(default)]
    pub follows_replayed: u64,
}

pub async fn merge_anonymous(
    State(state): State<Arc<AppState>>,
    AuthedUser(user): AuthedUser,
    OptionalAnonId(anon): OptionalAnonId,
) -> Result<Json<MergeResponse>, ApiError> {
    let Some(anon_id) = anon else {
        // No anon cookie on this request — nothing to merge. Return a
        // success-shaped response so the caller doesn't need to
        // distinguish "no work to do" from "work done."
        return Ok(Json(MergeResponse {
            uploads_merged: 0,
            events_merged: 0,
            follows_replayed: 0,
        }));
    };

    // Transaction so a partial merge (e.g. db hiccup between the two
    // updates) doesn't leave the user half-linked.
    let mut tx = state.pool.begin().await?;

    let uploads_merged = sqlx::query(
        r#"
        UPDATE uploads
           SET user_id = $1
         WHERE anonymous_id = $2
           AND user_id IS NULL
        "#,
    )
    .bind(user.id)
    .bind(anon_id)
    .execute(&mut *tx)
    .await?
    .rows_affected();

    let events_merged = sqlx::query(
        r#"
        UPDATE events
           SET user_id = $1
         WHERE anonymous_id = $2
           AND user_id IS NULL
        "#,
    )
    .bind(user.id)
    .bind(anon_id)
    .execute(&mut *tx)
    .await?
    .rows_affected();

    // T-052c — drain queued anonymous intents. Today only
    // `follow_artist` is wired; future kinds (save-to-collection,
    // inquiry-start) add another match arm here. We delete all rows
    // for this anon_id whether or not their kind was recognised: a
    // half-broken intent shouldn't surprise the user weeks later
    // when this code learns a new kind name.
    let pending: Vec<PendingRow> = sqlx::query_as(
        r#"
        SELECT kind, payload
          FROM anon_pending_actions
         WHERE anon_id = $1
           AND expires_at > now()
        "#,
    )
    .bind(anon_id)
    .fetch_all(&mut *tx)
    .await?;

    let mut follows_replayed: u64 = 0;
    for row in &pending {
        match row.kind.as_str() {
            "follow_artist" => {
                let Some(artist_id) = row
                    .payload
                    .get("artist_id")
                    .and_then(|v| v.as_str())
                    .and_then(|s| uuid::Uuid::parse_str(s).ok())
                else {
                    tracing::warn!(?row, "malformed follow_artist payload; dropping");
                    continue;
                };
                let res = sqlx::query(
                    r#"
                    INSERT INTO follows (user_id, artist_id)
                    SELECT $1, $2
                     WHERE EXISTS (
                       SELECT 1 FROM artists
                        WHERE id = $2 AND deleted_at IS NULL
                     )
                     ON CONFLICT (user_id, artist_id) DO NOTHING
                    "#,
                )
                .bind(user.id)
                .bind(artist_id)
                .execute(&mut *tx)
                .await?;
                follows_replayed += res.rows_affected();
            }
            other => {
                tracing::warn!(kind = %other, "unknown pending action kind; dropping");
            }
        }
    }

    // Drain everything for this anon_id — recognised + unknown
    // kinds, expired + still-valid. A drained-but-failed intent is
    // better than a stale intent that fires next month.
    sqlx::query("DELETE FROM anon_pending_actions WHERE anon_id = $1")
        .bind(anon_id)
        .execute(&mut *tx)
        .await?;

    tx.commit().await?;

    if uploads_merged > 0 || events_merged > 0 || follows_replayed > 0 {
        tracing::info!(
            user_id = %user.id,
            %anon_id,
            uploads_merged,
            events_merged,
            follows_replayed,
            "merged anonymous trail into user",
        );
    }

    Ok(Json(MergeResponse {
        uploads_merged,
        events_merged,
        follows_replayed,
    }))
}

#[derive(Debug, sqlx::FromRow)]
struct PendingRow {
    kind: String,
    payload: serde_json::Value,
}
