//! T-083.3 — admin image-moderation queue.
//!
//! Two endpoints:
//!   - `GET  /v1/admin/images?status=rejected` — paginated list of
//!     auto-moderated-out images for human review.
//!   - `POST /v1/admin/images/:id/override`    — flip `rejected →
//!     approved`, clear `moderation_reason`, write audit.
//!
//! The override is intentionally one-way: from `rejected` back to
//! `approved`. We don't expose "re-reject" because the auto-moderator
//! is the source of `rejected` rows and admins shouldn't be reverting
//! its decisions through the same affordance.

use crate::extractors::AdminUser;
use crate::AppState;
use axum::{
    extract::{Path, Query, State},
    Json,
};
use ml_art_core::{
    admin::{action, audit, target},
    cursor::{CursorError, PageCursor},
    error::ApiError,
    images::url_for_s3_key,
};
use serde::{Deserialize, Serialize};
use sqlx::FromRow;
use std::sync::Arc;
use uuid::Uuid;

const DEFAULT_LIMIT: i64 = 24;
const MAX_LIMIT: i64 = 100;

#[derive(Debug, Deserialize)]
pub struct ListParams {
    /// `rejected` is the queue admins care about; `pending` and
    /// `approved` are available for diagnostics. Default = rejected.
    #[serde(default)]
    pub status: Option<String>,
    #[serde(default)]
    pub cursor: Option<String>,
    #[serde(default)]
    pub limit: Option<i64>,
}

#[derive(Debug, Serialize, FromRow)]
pub struct AdminImageRowDb {
    pub id: Uuid,
    pub artwork_id: Uuid,
    pub artwork_title: Option<String>,
    pub artist_id: Uuid,
    pub artist_slug: String,
    pub artist_display_name: String,
    pub s3_key: String,
    pub moderation_status: String,
    pub moderation_reason: Option<String>,
    pub is_primary: bool,
    pub created_at: chrono::DateTime<chrono::Utc>,
}

/// Wire-shape row — adds `url` derived from `s3_key` so the web side
/// doesn't need to know the CDN base URL.
#[derive(Debug, Serialize)]
pub struct AdminImageRow {
    pub id: Uuid,
    pub artwork_id: Uuid,
    pub artwork_title: Option<String>,
    pub artist_id: Uuid,
    pub artist_slug: String,
    pub artist_display_name: String,
    pub s3_key: String,
    pub url: String,
    pub moderation_status: String,
    pub moderation_reason: Option<String>,
    pub is_primary: bool,
    pub created_at: chrono::DateTime<chrono::Utc>,
}

impl From<AdminImageRowDb> for AdminImageRow {
    fn from(r: AdminImageRowDb) -> Self {
        let url = url_for_s3_key(&r.s3_key);
        Self {
            id: r.id,
            artwork_id: r.artwork_id,
            artwork_title: r.artwork_title,
            artist_id: r.artist_id,
            artist_slug: r.artist_slug,
            artist_display_name: r.artist_display_name,
            s3_key: r.s3_key,
            url,
            moderation_status: r.moderation_status,
            moderation_reason: r.moderation_reason,
            is_primary: r.is_primary,
            created_at: r.created_at,
        }
    }
}

#[derive(Debug, Serialize)]
pub struct ListResponse {
    pub items: Vec<AdminImageRow>,
    pub next_cursor: Option<String>,
}

pub async fn list(
    State(state): State<Arc<AppState>>,
    _admin: AdminUser,
    Query(p): Query<ListParams>,
) -> Result<Json<ListResponse>, ApiError> {
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
    let status = p.status.as_deref().unwrap_or("rejected");

    let rows: Vec<AdminImageRowDb> = sqlx::query_as(
        r#"
        SELECT
            ai.id,
            ai.artwork_id,
            a.title           AS artwork_title,
            ar.id             AS artist_id,
            ar.slug           AS artist_slug,
            ar.display_name   AS artist_display_name,
            ai.s3_key,
            ai.moderation_status,
            ai.moderation_reason,
            ai.is_primary,
            ai.created_at
        FROM artwork_images ai
        JOIN artworks a  ON a.id  = ai.artwork_id
        JOIN artists  ar ON ar.id = a.artist_id
        WHERE ai.moderation_status = $1
          AND a.deleted_at  IS NULL
          AND ar.deleted_at IS NULL
        ORDER BY ai.created_at DESC
        LIMIT $2 OFFSET $3
        "#,
    )
    .bind(status)
    .bind(limit + 1)
    .bind(offset)
    .fetch_all(&state.pool)
    .await?;

    let has_next = rows.len() > limit as usize;
    let items: Vec<AdminImageRow> = rows
        .into_iter()
        .take(limit as usize)
        .map(AdminImageRow::from)
        .collect();
    let next_cursor = has_next.then(|| PageCursor::from_offset(offset + limit).encode());
    Ok(Json(ListResponse { items, next_cursor }))
}

#[derive(Debug, Serialize, FromRow, Clone)]
pub struct ImageStatusRow {
    pub id: Uuid,
    pub moderation_status: String,
    pub moderation_reason: Option<String>,
}

pub async fn override_approve(
    State(state): State<Arc<AppState>>,
    AdminUser(admin): AdminUser,
    Path(id): Path<Uuid>,
) -> Result<Json<ImageStatusRow>, ApiError> {
    let before: ImageStatusRow = sqlx::query_as(
        "SELECT id, moderation_status, moderation_reason FROM artwork_images WHERE id = $1",
    )
    .bind(id)
    .fetch_optional(&state.pool)
    .await?
    .ok_or(ApiError::NotFound)?;

    // Idempotent re-apply: already-approved is OK, no audit clutter.
    if before.moderation_status == "approved" {
        return Ok(Json(before));
    }
    if before.moderation_status != "rejected" {
        // The override path is specifically for flipping auto-moderator
        // verdicts. `pending` images aren't decided yet; let the
        // pipeline land before manually clobbering it.
        return Err(ApiError::Conflict(format!(
            "cannot override status {} — only rejected → approved is supported",
            before.moderation_status
        )));
    }

    let after = ImageStatusRow {
        id: before.id,
        moderation_status: "approved".to_string(),
        moderation_reason: None,
    };
    audit::record(
        &state.pool,
        Some(admin.id),
        action::IMAGE_OVERRIDE_APPROVE,
        target::IMAGE,
        Some(id),
        Some(&before),
        Some(&after),
        None,
    )
    .await?;

    let updated: ImageStatusRow = sqlx::query_as(
        r#"
        UPDATE artwork_images
           SET moderation_status = 'approved',
               moderation_reason = NULL
        WHERE id = $1
        RETURNING id, moderation_status, moderation_reason
        "#,
    )
    .bind(id)
    .fetch_one(&state.pool)
    .await?;
    Ok(Json(updated))
}
