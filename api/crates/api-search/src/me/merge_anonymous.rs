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
//!
//! Both tables are updated in a single transaction so a partial merge
//! can't leave the user only partly linked.

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

    tx.commit().await?;

    if uploads_merged > 0 || events_merged > 0 {
        tracing::info!(
            user_id = %user.id,
            %anon_id,
            uploads_merged,
            events_merged,
            "merged anonymous trail into user",
        );
    }

    Ok(Json(MergeResponse {
        uploads_merged,
        events_merged,
    }))
}
