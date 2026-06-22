//! `/v1/studio/inquiries` — the artist's inquiry inbox + reply UX.
//!
//! Three endpoints share this module:
//!
//! - `GET  /v1/studio/inquiries`            — list, with replies + read state
//! - `POST /v1/studio/inquiries/:id/reply`  — artist replies to one inquiry
//! - `POST /v1/studio/inquiries/read`       — bulk mark-as-read on inbox view
//!
//! The email handler (T-032) already sends a notification to the artist
//! on `delivered_at`; this surface is the in-app companion so the artist
//! can re-read past inquiries, see pending verifications, and now —
//! T-011 Phase 4b — reply directly without leaving the studio.
//!
//! Ownership: every SQL path filters on `artist_id = current_artist_id(user)`.
//! No cross-artist visibility. A non-artist caller gets 404 from
//! `current_artist_id`, not 403, mirroring the rest of `/v1/studio/*`.
//!
//! Pagination: no cursor yet. Sorted by `created_at DESC` with a hard
//! `LIMIT` (50 — same shape as artworks list). When an artist's inbox
//! crosses that we'll add `?cursor=…` per T-037.

use axum::{
    extract::{Path, Query, State},
    Json,
};
use chrono::{DateTime, Utc};
use ml_art_core::{
    error::ApiError,
    images::url_for_s3_key,
    jobs::{EnqueueOpts, JobEvent},
    models::Paginated,
};
use serde::{Deserialize, Serialize};
use sqlx::FromRow;
use std::sync::Arc;
use uuid::Uuid;

use crate::extractors::AuthedUser;
use crate::studio::current_artist_id;
use crate::AppState;

const PAGE_LIMIT: i64 = 50;

/// Cap on reply length. Picked to match the inquiry-message cap on
/// the inquire endpoint (we want replies and inquiries to feel
/// symmetric). If you change one, change both.
const REPLY_MESSAGE_MAX_LEN: usize = 4000;

// ─────────────────────────────────────────────────────────────────────────────
// Wire shapes
// ─────────────────────────────────────────────────────────────────────────────

/// One row in the inbox. `status` is derived server-side from
/// `delivered_at`; `replies` and `read_at` come straight from the DB.
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
    /// When the artist last opened this inquiry in their inbox.
    /// `null` ≡ unread. T-011 Phase 4b.
    pub read_at: Option<DateTime<Utc>>,
    /// Artist's outgoing replies, oldest first. Empty when the artist
    /// hasn't replied yet. T-011 Phase 4b.
    pub replies: Vec<InquiryReply>,
}

#[derive(Debug, Serialize)]
pub struct InquiryReply {
    pub id: Uuid,
    /// `"artist"` for a studio-inbox reply, `"inquirer"` for a reply
    /// stitched back in from the inbound-email webhook (T-054). Drives
    /// the sender chip + alignment in the inbox thread.
    pub from_role: String,
    pub message: String,
    pub created_at: DateTime<Utc>,
    /// Set once the email handler completes the Resend send.
    /// `null` ≡ in-flight or queued; the UI can render a pending
    /// state if it wants to surface that.
    pub sent_at: Option<DateTime<Utc>>,
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
    read_at: Option<DateTime<Utc>>,
}

#[derive(FromRow)]
struct ReplyRow {
    id: Uuid,
    inquiry_id: Uuid,
    from_role: String,
    message: String,
    created_at: DateTime<Utc>,
    sent_at: Option<DateTime<Utc>>,
}

impl InquiryRow {
    fn into_wire(self, replies: Vec<InquiryReply>) -> StudioInquiry {
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
            read_at: self.read_at,
            replies,
        }
    }
}

// ─────────────────────────────────────────────────────────────────────────────
// GET /v1/studio/inquiries
// ─────────────────────────────────────────────────────────────────────────────

pub async fn list(
    State(state): State<Arc<AppState>>,
    Query(params): Query<ListQuery>,
    AuthedUser(user): AuthedUser,
) -> Result<Json<Paginated<StudioInquiry>>, ApiError> {
    let artist_id = current_artist_id(&state.pool, &user).await?;

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
            i.delivered_at,
            i.read_at
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

    // Single follow-up query for all replies across the page — avoids
    // an N+1 (one reply-list query per row). Filtered by the same
    // artist_id so it's ownership-safe even if the inquiry ids were
    // somehow forged: the JOIN ensures a reply only attaches to an
    // inquiry this artist owns.
    let inquiry_ids: Vec<Uuid> = rows.iter().map(|r| r.id).collect();
    let reply_rows: Vec<ReplyRow> = if inquiry_ids.is_empty() {
        Vec::new()
    } else {
        sqlx::query_as(
            r#"
            SELECT r.id, r.inquiry_id, r.from_role, r.message, r.created_at, r.sent_at
            FROM inquiry_replies r
            JOIN inquiries i ON i.id = r.inquiry_id
            WHERE i.artist_id = $1
              AND r.inquiry_id = ANY($2)
            ORDER BY r.created_at ASC
            "#,
        )
        .bind(artist_id)
        .bind(&inquiry_ids)
        .fetch_all(&state.pool)
        .await?
    };

    // Bucket replies by inquiry_id so we hand each StudioInquiry its
    // own slice without an O(N²) scan.
    let mut replies_by_inquiry: std::collections::HashMap<Uuid, Vec<InquiryReply>> =
        std::collections::HashMap::new();
    for r in reply_rows {
        replies_by_inquiry
            .entry(r.inquiry_id)
            .or_default()
            .push(InquiryReply {
                id: r.id,
                from_role: r.from_role,
                message: r.message,
                created_at: r.created_at,
                sent_at: r.sent_at,
            });
    }

    let items: Vec<StudioInquiry> = rows
        .into_iter()
        .map(|r| {
            let replies = replies_by_inquiry.remove(&r.id).unwrap_or_default();
            r.into_wire(replies)
        })
        .collect();
    Ok(Json(Paginated {
        items,
        next_cursor: None,
    }))
}

// ─────────────────────────────────────────────────────────────────────────────
// POST /v1/studio/inquiries/:id/reply
// ─────────────────────────────────────────────────────────────────────────────

#[derive(Debug, Deserialize)]
pub struct ReplyBody {
    pub message: String,
}

pub async fn reply(
    State(state): State<Arc<AppState>>,
    Path(inquiry_id): Path<Uuid>,
    AuthedUser(user): AuthedUser,
    Json(body): Json<ReplyBody>,
) -> Result<Json<InquiryReply>, ApiError> {
    let artist_id = current_artist_id(&state.pool, &user).await?;

    let message = body.message.trim();
    if message.is_empty() {
        return Err(ApiError::BadRequest("message: required".into()));
    }
    if message.len() > REPLY_MESSAGE_MAX_LEN {
        return Err(ApiError::BadRequest(format!(
            "message: too long (max {REPLY_MESSAGE_MAX_LEN})"
        )));
    }

    // Ownership check + insert in one statement: the WHERE-clause
    // requires the inquiry to belong to this artist, so a forged id
    // returns "0 rows inserted" → 404. Avoids a TOCTOU window where
    // a SELECT-then-INSERT could race a delete.
    let inserted: Option<(Uuid, DateTime<Utc>)> = sqlx::query_as(
        r#"
        INSERT INTO inquiry_replies (inquiry_id, artist_id, message)
        SELECT i.id, $2, $3
        FROM inquiries i
        WHERE i.id = $1 AND i.artist_id = $2
        RETURNING id, created_at
        "#,
    )
    .bind(inquiry_id)
    .bind(artist_id)
    .bind(message)
    .fetch_optional(&state.pool)
    .await?;

    let (reply_id, created_at) = inserted.ok_or(ApiError::NotFound)?;

    // Enqueue the send. Idempotency key dedupes the queue side; the
    // handler itself ALSO checks `sent_at IS NULL` so a retry-from-
    // failure path can't double-send either.
    state
        .jobs
        .enqueue(
            JobEvent::InquirySendReply { reply_id },
            EnqueueOpts {
                idempotency_key: Some(format!("inquiry_reply:{reply_id}:send")),
                ..Default::default()
            },
        )
        .await
        .map_err(|e| ApiError::Internal(anyhow::anyhow!("enqueue inquiry reply: {e}")))?;

    Ok(Json(InquiryReply {
        id: reply_id,
        from_role: "artist".into(),
        message: message.to_string(),
        created_at,
        sent_at: None,
    }))
}

// ─────────────────────────────────────────────────────────────────────────────
// POST /v1/studio/inquiries/read
// ─────────────────────────────────────────────────────────────────────────────

#[derive(Debug, Deserialize)]
pub struct MarkReadBody {
    /// Inquiry ids to flip to read. Silently ignores ids that don't
    /// belong to the caller — same ownership pattern as `reply`,
    /// implemented via the `WHERE artist_id = …` filter.
    pub ids: Vec<Uuid>,
}

#[derive(Debug, Serialize)]
pub struct MarkReadAck {
    /// Number of rows actually updated. < ids.len() means some were
    /// already read OR weren't owned by this artist; we don't
    /// distinguish (the caller doesn't need to act on either).
    pub updated: u64,
}

pub async fn mark_read(
    State(state): State<Arc<AppState>>,
    AuthedUser(user): AuthedUser,
    Json(body): Json<MarkReadBody>,
) -> Result<Json<MarkReadAck>, ApiError> {
    let artist_id = current_artist_id(&state.pool, &user).await?;

    // Cap the per-request batch — protects against a malicious or
    // buggy client trying to update the whole table. Inbox page
    // never sends more than `PAGE_LIMIT` ids in practice.
    const MAX_IDS: usize = 100;
    if body.ids.len() > MAX_IDS {
        return Err(ApiError::BadRequest(format!(
            "ids: too many (max {MAX_IDS})"
        )));
    }
    if body.ids.is_empty() {
        return Ok(Json(MarkReadAck { updated: 0 }));
    }

    // `read_at IS NULL` predicate means a re-mark is a no-op rather
    // than touching the timestamp every page load.
    let res = sqlx::query(
        r#"
        UPDATE inquiries
        SET read_at = now()
        WHERE id = ANY($1)
          AND artist_id = $2
          AND read_at IS NULL
        "#,
    )
    .bind(&body.ids)
    .bind(artist_id)
    .execute(&state.pool)
    .await?;

    Ok(Json(MarkReadAck {
        updated: res.rows_affected(),
    }))
}
