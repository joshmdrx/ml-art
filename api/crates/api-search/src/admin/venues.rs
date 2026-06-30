//! T-081.4 / T-083.4 — admin venue queue. Same shape as
//! `admin::artists`; lists venues by status and transitions
//! `pending_review → active | declined` (plus pause/unpause for
//! active venues). The bootstrap policy mirrors artist verification:
//! new venues default to `pending_review` and stay hidden until an
//! admin flips them.

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
    #[serde(default)]
    pub status: Option<String>,
    #[serde(default)]
    pub cursor: Option<String>,
    #[serde(default)]
    pub limit: Option<i64>,
}

#[derive(Debug, Serialize, FromRow)]
pub struct VenueAdminRow {
    pub id: Uuid,
    pub slug: String,
    pub name: String,
    pub kind: String,
    pub owner_email: Option<String>,
    pub status: String,
    pub city: Option<String>,
    pub country: Option<String>,
    pub created_at: chrono::DateTime<chrono::Utc>,
    pub updated_at: chrono::DateTime<chrono::Utc>,
}

#[derive(Debug, Serialize)]
pub struct ListResponse {
    pub items: Vec<VenueAdminRow>,
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
    let status = p.status.as_deref().unwrap_or("pending_review");

    let rows: Vec<VenueAdminRow> = sqlx::query_as(
        r#"
        SELECT
            v.id, v.slug, v.name, v.kind,
            u.email AS owner_email,
            v.status, v.city, v.country,
            v.created_at, v.updated_at
        FROM venues v
        LEFT JOIN users u ON u.id = v.owner_user_id
        WHERE v.deleted_at IS NULL AND v.status = $1
        ORDER BY v.created_at DESC
        LIMIT $2 OFFSET $3
        "#,
    )
    .bind(status)
    .bind(limit + 1)
    .bind(offset)
    .fetch_all(&state.pool)
    .await?;

    let has_next = rows.len() > limit as usize;
    let items: Vec<VenueAdminRow> = rows.into_iter().take(limit as usize).collect();
    let next_cursor = has_next.then(|| PageCursor::from_offset(offset + limit).encode());
    Ok(Json(ListResponse { items, next_cursor }))
}

#[derive(Debug, Serialize, FromRow, Clone)]
pub struct VenueStatusRow {
    pub id: Uuid,
    pub status: String,
}

async fn transition(
    state: &Arc<AppState>,
    admin_id: Uuid,
    venue_id: Uuid,
    action_name: &str,
    new_status: &str,
    legal_from: &[&str],
) -> Result<Json<VenueStatusRow>, ApiError> {
    let before: VenueStatusRow = sqlx::query_as(
        "SELECT id, status FROM venues WHERE id = $1 AND deleted_at IS NULL",
    )
    .bind(venue_id)
    .fetch_optional(&state.pool)
    .await?
    .ok_or(ApiError::NotFound)?;

    if before.status == new_status {
        return Ok(Json(before));
    }
    if !legal_from.contains(&before.status.as_str()) {
        return Err(ApiError::Conflict(format!(
            "cannot transition status {} → {}",
            before.status, new_status
        )));
    }

    let after = VenueStatusRow {
        id: before.id,
        status: new_status.to_string(),
    };
    audit::record(
        &state.pool,
        Some(admin_id),
        action_name,
        target::VENUE,
        Some(venue_id),
        Some(&before),
        Some(&after),
        None,
    )
    .await?;

    let updated: VenueStatusRow = sqlx::query_as(
        r#"
        UPDATE venues SET status = $2, updated_at = now()
        WHERE id = $1 AND deleted_at IS NULL
        RETURNING id, status
        "#,
    )
    .bind(venue_id)
    .bind(new_status)
    .fetch_one(&state.pool)
    .await?;

    Ok(Json(updated))
}

pub async fn approve(
    State(state): State<Arc<AppState>>,
    AdminUser(admin): AdminUser,
    Path(id): Path<Uuid>,
) -> Result<Json<VenueStatusRow>, ApiError> {
    transition(
        &state,
        admin.id,
        id,
        action::VENUE_APPROVE,
        "active",
        &["pending_review"],
    )
    .await
}

pub async fn decline(
    State(state): State<Arc<AppState>>,
    AdminUser(admin): AdminUser,
    Path(id): Path<Uuid>,
) -> Result<Json<VenueStatusRow>, ApiError> {
    transition(
        &state,
        admin.id,
        id,
        action::VENUE_DECLINE,
        "declined",
        &["pending_review"],
    )
    .await
}
