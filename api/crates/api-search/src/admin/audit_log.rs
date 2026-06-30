//! T-083.5 — read-only audit log viewer.
//!
//! `GET /v1/admin/audit-log` — paginated reverse-chronological. No
//! filtering by action / target_kind in v1; the table will be tiny for
//! years and a flat list is the easier read.

use crate::extractors::AdminUser;
use crate::AppState;
use axum::{
    extract::{Query, State},
    Json,
};
use ml_art_core::{
    cursor::{CursorError, PageCursor},
    error::ApiError,
};
use serde::{Deserialize, Serialize};
use sqlx::FromRow;
use std::sync::Arc;
use uuid::Uuid;

const DEFAULT_LIMIT: i64 = 50;
const MAX_LIMIT: i64 = 200;

#[derive(Debug, Deserialize)]
pub struct ListParams {
    #[serde(default)]
    pub cursor: Option<String>,
    #[serde(default)]
    pub limit: Option<i64>,
}

#[derive(Debug, Serialize, FromRow)]
pub struct AuditLogEntry {
    pub id: Uuid,
    pub admin_user_id: Option<Uuid>,
    pub admin_email: Option<String>,
    pub action: String,
    pub target_kind: String,
    pub target_id: Option<Uuid>,
    pub before_jsonb: Option<serde_json::Value>,
    pub after_jsonb: Option<serde_json::Value>,
    pub created_at: chrono::DateTime<chrono::Utc>,
}

#[derive(Debug, Serialize)]
pub struct ListResponse {
    pub items: Vec<AuditLogEntry>,
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

    let rows: Vec<AuditLogEntry> = sqlx::query_as(
        r#"
        SELECT
            al.id,
            al.admin_user_id,
            u.email AS admin_email,
            al.action,
            al.target_kind,
            al.target_id,
            al.before_jsonb,
            al.after_jsonb,
            al.created_at
        FROM admin_audit_log al
        LEFT JOIN users u ON u.id = al.admin_user_id
        ORDER BY al.created_at DESC
        LIMIT $1 OFFSET $2
        "#,
    )
    .bind(limit + 1)
    .bind(offset)
    .fetch_all(&state.pool)
    .await?;

    let has_next = rows.len() > limit as usize;
    let items: Vec<AuditLogEntry> = rows.into_iter().take(limit as usize).collect();
    let next_cursor = has_next.then(|| PageCursor::from_offset(offset + limit).encode());
    Ok(Json(ListResponse { items, next_cursor }))
}
