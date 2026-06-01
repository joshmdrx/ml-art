//! `GET /v1/studio/inquiries` — the artist's inquiry inbox.
//!
//! Returns every inquiry addressed to the calling artist (signed-in or
//! anonymous), newest first. The email handler (T-032) already sends a
//! notification to the artist on `delivered_at`; this endpoint is the
//! in-app companion so the artist can re-read past inquiries, see
//! anonymous inquiries that haven't yet been verified, and triage
//! follow-ups.
//!
//! Filtering:
//!
//! - `?status=delivered`    — only inquiries with `delivered_at IS NOT NULL`
//! - `?status=pending`      — only inquiries with `delivered_at IS NULL`
//!   (i.e. anonymous, waiting on verification-link click)
//! - `?status=all` (default) — everything
//!
//! Ownership: the SQL filters on `artist_id = current_artist_id(user)`.
//! No cross-artist visibility. Like the rest of `/v1/studio/*`, a
//! non-artist caller gets 404 from `current_artist_id`, not 403, to
//! avoid leaking the existence of artist rows.
//!
//! Pagination: no cursor yet. Sorted by `created_at DESC` with a hard
//! `LIMIT` (50 — same shape as artworks list). When an artist's inbox
//! crosses that we'll add `?cursor=…` per T-037.

use axum::{
    extract::{Query, State},
    Json,
};
use chrono::{DateTime, Utc};
use ml_art_core::{
    error::ApiError, images::url_for_s3_key, models::Paginated,
};
use serde::{Deserialize, Serialize};
use sqlx::FromRow;
use std::sync::Arc;
use uuid::Uuid;

use crate::extractors::AuthedUser;
use crate::studio::current_artist_id;
use crate::AppState;

const PAGE_LIMIT: i64 = 50;

// ─────────────────────────────────────────────────────────────────────────────
// Wire shapes
// ─────────────────────────────────────────────────────────────────────────────

/// One row in the inbox. `status` is derived server-side from
/// `delivered_at` rather than stored — keeps the wire shape tidy
/// without adding a column.
#[derive(Debug, Serialize)]
pub struct StudioInquiry {
    pub id: Uuid,
    pub artwork_id: Uuid,
    pub artwork_title: Option<String>,
    pub artwork_primary_image_url: Option<String>,
    pub from_name: String,
    pub from_email: String,
    pub message: String,
    pub budget_range: Option<String>,
    /// `"delivered"` when `delivered_at IS NOT NULL`, else
    /// `"pending_verification"`. Mirrors the create-endpoint string.
    pub status: String,
    pub created_at: DateTime<Utc>,
    pub delivered_at: Option<DateTime<Utc>>,
}

#[derive(Debug, Deserialize, Default)]
pub struct ListQuery {
    /// `pending`, `delivered`, or `all` (default). Tolerant — any unknown
    /// value collapses to `all` instead of 400 so the front-end can
    /// pass through whatever the URL had.
    #[serde(default)]
    pub status: Option<String>,
}

#[derive(FromRow)]
struct InquiryRow {
    id: Uuid,
    artwork_id: Uuid,
    artwork_title: Option<String>,
    primary_s3_key: Option<String>,
    from_name: String,
    from_email: String,
    message: String,
    budget_range: Option<serde_json::Value>,
    created_at: DateTime<Utc>,
    delivered_at: Option<DateTime<Utc>>,
}

impl InquiryRow {
    fn into_wire(self) -> StudioInquiry {
        let status = if self.delivered_at.is_some() {
            "delivered".to_string()
        } else {
            "pending_verification".to_string()
        };
        let budget = self
            .budget_range
            .as_ref()
            .and_then(|v| v.as_str().map(String::from));
        StudioInquiry {
            id: self.id,
            artwork_id: self.artwork_id,
            artwork_title: self.artwork_title,
            artwork_primary_image_url: self.primary_s3_key.as_deref().map(url_for_s3_key),
            from_name: self.from_name,
            from_email: self.from_email,
            message: self.message,
            budget_range: budget,
            status,
            created_at: self.created_at,
            delivered_at: self.delivered_at,
        }
    }
}

// ─────────────────────────────────────────────────────────────────────────────
// Handler
// ─────────────────────────────────────────────────────────────────────────────

pub async fn list(
    State(state): State<Arc<AppState>>,
    Query(params): Query<ListQuery>,
    AuthedUser(user): AuthedUser,
) -> Result<Json<Paginated<StudioInquiry>>, ApiError> {
    let artist_id = current_artist_id(&state.pool, &user).await?;

    // Three modes: pending-only, delivered-only, all. Anything else
    // (or absent) treats as all. Encoded as a small int passed to the
    // single SQL statement so the planner sees a stable shape.
    let mode: i32 = match params.status.as_deref() {
        Some("pending") => 1,
        Some("delivered") => 2,
        _ => 0,
    };

    // Primary image is filtered to `moderation_status = 'approved'`
    // (matches the rest of our public-surface joins, T-008). The
    // studio inbox is artist-private, but the thumbnail is the same
    // surface a public viewer would see — pending/rejected primary
    // images stay invisible. The inquiry row itself is unconditional.
    let rows: Vec<InquiryRow> = sqlx::query_as(
        r#"
        SELECT
            i.id,
            i.artwork_id,
            a.title         AS artwork_title,
            ai.s3_key       AS primary_s3_key,
            i.from_name,
            i.from_email,
            i.message,
            i.budget_range,
            i.created_at,
            i.delivered_at
        FROM inquiries i
        JOIN artworks a ON a.id = i.artwork_id
        LEFT JOIN artwork_images ai
               ON ai.artwork_id = a.id
              AND ai.is_primary
              AND ai.moderation_status = 'approved'
        WHERE i.artist_id = $1
          AND ($2 = 0
            OR ($2 = 1 AND i.delivered_at IS NULL)
            OR ($2 = 2 AND i.delivered_at IS NOT NULL))
        ORDER BY i.created_at DESC
        LIMIT $3
        "#,
    )
    .bind(artist_id)
    .bind(mode)
    .bind(PAGE_LIMIT)
    .fetch_all(&state.pool)
    .await?;

    let items: Vec<StudioInquiry> = rows.into_iter().map(InquiryRow::into_wire).collect();
    Ok(Json(Paginated {
        items,
        next_cursor: None,
    }))
}
