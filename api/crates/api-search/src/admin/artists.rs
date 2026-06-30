//! T-083.1 — admin artist queue.
//!
//! Five endpoints, all under `/v1/admin/artists`:
//!   - `GET /v1/admin/artists?status=pending` — paginated list
//!   - `POST /v1/admin/artists/:id/approve`  — pending → active
//!   - `POST /v1/admin/artists/:id/decline`  — pending → rejected
//!   - `POST /v1/admin/artists/:id/pause`    — active → paused
//!   - `POST /v1/admin/artists/:id/unpause`  — paused → active
//!
//! Schema CHECK constraint pins the status set to
//! `('pending', 'active', 'paused', 'rejected')` — see migration 0001.
//! The decline path therefore maps to `rejected`; the wire word
//! ("decline") matches what admins read in the UI.

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
};
use serde::{Deserialize, Serialize};
use sqlx::FromRow;
use std::sync::Arc;
use uuid::Uuid;

const DEFAULT_LIMIT: i64 = 24;
const MAX_LIMIT: i64 = 100;

#[derive(Debug, Deserialize)]
pub struct ListParams {
    /// `pending` | `active` | `paused` | `rejected`. Unknown values
    /// silently fall through to `pending` — the queue is the default
    /// view; we don't 400 on a typo'd URL since the admin can read
    /// the response and correct.
    #[serde(default)]
    pub status: Option<String>,
    #[serde(default)]
    pub cursor: Option<String>,
    #[serde(default)]
    pub limit: Option<i64>,
}

#[derive(Debug, Serialize, FromRow)]
pub struct ArtistAdminRow {
    pub id: Uuid,
    pub slug: String,
    pub display_name: String,
    pub email: Option<String>,
    pub status: String,
    pub city: Option<String>,
    pub country: Option<String>,
    pub created_at: chrono::DateTime<chrono::Utc>,
    pub updated_at: chrono::DateTime<chrono::Utc>,
    pub artwork_count: i64,
}

#[derive(Debug, Serialize)]
pub struct ListResponse {
    pub items: Vec<ArtistAdminRow>,
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
    let status = p.status.as_deref().unwrap_or("pending");

    // Fetch limit+1 to compute next_cursor without a separate COUNT.
    let rows: Vec<ArtistAdminRow> = sqlx::query_as(
        r#"
        SELECT
            ar.id, ar.slug, ar.display_name, u.email,
            ar.status, ar.city, ar.country,
            ar.created_at, ar.updated_at,
            COALESCE(c.cnt, 0)::bigint AS artwork_count
        FROM artists ar
        LEFT JOIN users u ON u.id = ar.user_id
        LEFT JOIN (
            SELECT artist_id, COUNT(*) AS cnt
            FROM artworks
            WHERE deleted_at IS NULL
            GROUP BY artist_id
        ) c ON c.artist_id = ar.id
        WHERE ar.deleted_at IS NULL AND ar.status = $1
        ORDER BY ar.created_at DESC
        LIMIT $2 OFFSET $3
        "#,
    )
    .bind(status)
    .bind(limit + 1)
    .bind(offset)
    .fetch_all(&state.pool)
    .await?;

    let has_next = rows.len() > limit as usize;
    let items: Vec<ArtistAdminRow> = rows.into_iter().take(limit as usize).collect();
    let next_cursor = has_next.then(|| PageCursor::from_offset(offset + limit).encode());
    Ok(Json(ListResponse { items, next_cursor }))
}

#[derive(Debug, Serialize, FromRow, Clone)]
pub struct ArtistStatusRow {
    pub id: Uuid,
    pub status: String,
}

/// Run an admin-state transition with audit. Steps:
///   1. Read the current `(id, status)` of the target artist.
///   2. Validate the requested transition. Returns Conflict for
///      illegal transitions (e.g. approving an already-active artist
///      is idempotent and OK, but approving a `rejected` artist is
///      not — the admin should re-pending them first via a separate
///      UI affordance).
///   3. Write the audit row.
///   4. Apply the UPDATE.
///   5. Return the new row.
async fn transition(
    state: &Arc<AppState>,
    admin_id: Uuid,
    artist_id: Uuid,
    action_name: &str,
    new_status: &str,
    legal_from: &[&str],
) -> Result<Json<ArtistStatusRow>, ApiError> {
    let before: ArtistStatusRow = sqlx::query_as(
        "SELECT id, status FROM artists WHERE id = $1 AND deleted_at IS NULL",
    )
    .bind(artist_id)
    .fetch_optional(&state.pool)
    .await?
    .ok_or(ApiError::NotFound)?;

    // Idempotent re-application: already-target-status returns the
    // unchanged row with no audit + no UPDATE. Keeps "I double-clicked
    // approve" from cluttering the log.
    if before.status == new_status {
        return Ok(Json(before));
    }
    if !legal_from.contains(&before.status.as_str()) {
        return Err(ApiError::Conflict(format!(
            "cannot transition status {} → {}",
            before.status, new_status
        )));
    }

    let after = ArtistStatusRow {
        id: before.id,
        status: new_status.to_string(),
    };
    audit::record(
        &state.pool,
        Some(admin_id),
        action_name,
        target::ARTIST,
        Some(artist_id),
        Some(&before),
        Some(&after),
        None,
    )
    .await?;

    let updated: ArtistStatusRow = sqlx::query_as(
        r#"
        UPDATE artists SET status = $2, updated_at = now()
        WHERE id = $1 AND deleted_at IS NULL
        RETURNING id, status
        "#,
    )
    .bind(artist_id)
    .bind(new_status)
    .fetch_one(&state.pool)
    .await?;

    Ok(Json(updated))
}

pub async fn approve(
    State(state): State<Arc<AppState>>,
    AdminUser(admin): AdminUser,
    Path(id): Path<Uuid>,
) -> Result<Json<ArtistStatusRow>, ApiError> {
    transition(
        &state,
        admin.id,
        id,
        action::ARTIST_APPROVE,
        "active",
        &["pending"],
    )
    .await
}

pub async fn decline(
    State(state): State<Arc<AppState>>,
    AdminUser(admin): AdminUser,
    Path(id): Path<Uuid>,
) -> Result<Json<ArtistStatusRow>, ApiError> {
    transition(
        &state,
        admin.id,
        id,
        action::ARTIST_DECLINE,
        "rejected",
        &["pending"],
    )
    .await
}

pub async fn pause(
    State(state): State<Arc<AppState>>,
    AdminUser(admin): AdminUser,
    Path(id): Path<Uuid>,
) -> Result<Json<ArtistStatusRow>, ApiError> {
    transition(
        &state,
        admin.id,
        id,
        action::ARTIST_PAUSE,
        "paused",
        &["active"],
    )
    .await
}

pub async fn unpause(
    State(state): State<Arc<AppState>>,
    AdminUser(admin): AdminUser,
    Path(id): Path<Uuid>,
) -> Result<Json<ArtistStatusRow>, ApiError> {
    transition(
        &state,
        admin.id,
        id,
        action::ARTIST_UNPAUSE,
        "active",
        &["paused"],
    )
    .await
}
