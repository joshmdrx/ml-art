//! T-054 — inbound-email webhook for email-stitched inquiry threads.
//!
//! `POST /v1/webhooks/email/inbound` is called by the Cloudflare Email
//! Worker (see `infra/email-worker/`) after an inquirer replies to an
//! artist-reply email. The worker parses the raw message and POSTs the
//! extracted text here. This handler:
//!
//!   1. authenticates the worker via a shared secret header (the route
//!      is otherwise unauthenticated — there's no user session);
//!   2. verifies the HMAC in the destination address against
//!      `(inquiry_id, anon_cookie_secret)` — the same token
//!      `core::reply_address::mint` put in the outbound `Reply-To`;
//!   3. persists the inquirer's message as an `inquiry_replies` row with
//!      `from_role='inquirer'`, deduped on the inbound `Message-ID`
//!      (replay protection via the partial unique index from migration
//!      0019);
//!   4. enqueues `JobEvent::InquirySendReplyForward` so the jobs worker
//!      forwards the reply to the artist.
//!
//! Transport-neutral: the parse → verify → persist → enqueue core is
//! identical whether mail reaches us via Cloudflare or a future Resend
//! Inbound webhook — only the auth check (§1) and DNS would change.

use axum::{extract::State, http::HeaderMap, Json};
use ml_art_core::{
    error::ApiError,
    jobs::{EnqueueOpts, JobEvent},
    reply_address,
};
use serde::{Deserialize, Serialize};
use std::sync::Arc;
use uuid::Uuid;

use crate::AppState;

/// Header the Cloudflare Email Worker presents to authenticate. Compared
/// (constant-time) against `Config::inbound_email_secret`.
const SECRET_HEADER: &str = "x-inbound-secret";

/// Upper bound on the stored reply text. The worker already strips
/// quoted history and sends just the latest reply, so anything beyond
/// this is almost certainly junk; truncate rather than store unbounded
/// text per row. (The axum body limit is the outer guard.)
const MAX_INBOUND_LEN: usize = 16_000;

#[derive(Debug, Deserialize)]
pub struct InboundPayload {
    /// The destination address the inquirer replied to —
    /// `r-<inquiry_id>-<hmac>@<reply_domain>`. May be the full address
    /// or the bare local part; `reply_address::verify` accepts both.
    pub to: String,
    /// The inquirer's address (informational; the thread is keyed on the
    /// verified inquiry id, not this).
    #[allow(dead_code)]
    pub from: String,
    /// The extracted latest-reply text.
    pub message: String,
    /// The inbound `Message-ID` — the replay-dedup key.
    pub message_id: String,
}

#[derive(Debug, Serialize)]
pub struct InboundAck {
    /// `"accepted"` for a newly-threaded reply; `"duplicate"` when the
    /// `Message-ID` was already processed (idempotent replay).
    status: &'static str,
}

pub async fn inbound_email(
    State(state): State<Arc<AppState>>,
    headers: HeaderMap,
    Json(payload): Json<InboundPayload>,
) -> Result<Json<InboundAck>, ApiError> {
    // 1. Shared-secret auth. Closed by default: if the secret isn't
    //    configured, or the header is missing/wrong, reject.
    let Some(expected) = state.cfg.inbound_email_secret.as_deref() else {
        tracing::warn!("inbound-email webhook hit but INBOUND_EMAIL_SECRET is unset");
        return Err(ApiError::Unauthorized);
    };
    let presented = headers.get(SECRET_HEADER).and_then(|v| v.to_str().ok());
    if !presented.is_some_and(|p| ct_eq(p.as_bytes(), expected.as_bytes())) {
        return Err(ApiError::Unauthorized);
    }

    // 2. Verify the tokenised address → inquiry id.
    let inquiry_id = reply_address::verify(&payload.to, state.cfg.anon_cookie_secret.as_bytes())
        .ok_or_else(|| ApiError::BadRequest("to: invalid or unrecognised reply address".into()))?;

    // 3. Validate + normalise the message + message-id.
    let message = payload.message.trim();
    if message.is_empty() {
        return Err(ApiError::BadRequest("message: required".into()));
    }
    let message: String = message.chars().take(MAX_INBOUND_LEN).collect();
    let message_id = payload.message_id.trim();
    if message_id.is_empty() {
        // The replay guard depends on a stable id; the worker must supply
        // one (header value, or a hash fallback it computes).
        return Err(ApiError::BadRequest("message_id: required".into()));
    }

    // 4. Persist, deduped on Message-ID. The partial unique index from
    //    migration 0019 makes a re-delivery of the same message a no-op:
    //    no row returned ⇒ already processed ⇒ ack without enqueuing.
    let reply_id: Option<Uuid> = sqlx::query_scalar(
        r#"
        INSERT INTO inquiry_replies (inquiry_id, from_role, message, inbound_message_id)
        VALUES ($1, 'inquirer', $2, $3)
        ON CONFLICT (inbound_message_id) WHERE inbound_message_id IS NOT NULL DO NOTHING
        RETURNING id
        "#,
    )
    .bind(inquiry_id)
    .bind(&message)
    .bind(message_id)
    .fetch_optional(&state.pool)
    .await?;

    let Some(reply_id) = reply_id else {
        tracing::info!(%inquiry_id, message_id, "inbound reply ignored — duplicate Message-ID");
        return Ok(Json(InboundAck {
            status: "duplicate",
        }));
    };

    // 5. Enqueue the forward-to-artist job. Idempotent on the queue side;
    //    the handler also re-checks `sent_at`.
    state
        .jobs
        .enqueue(
            JobEvent::InquirySendReplyForward { reply_id },
            EnqueueOpts {
                idempotency_key: Some(format!("inquiry_reply_forward:{reply_id}:send")),
                ..Default::default()
            },
        )
        .await
        .map_err(|e| ApiError::Internal(anyhow::anyhow!("enqueue reply forward: {e}")))?;

    tracing::info!(%inquiry_id, %reply_id, "inbound reply threaded + forward enqueued");
    Ok(Json(InboundAck { status: "accepted" }))
}

/// Constant-time byte equality. Length is allowed to leak (the secret is
/// fixed-length); content is compared without early-exit.
fn ct_eq(a: &[u8], b: &[u8]) -> bool {
    if a.len() != b.len() {
        return false;
    }
    let mut diff = 0u8;
    for (x, y) in a.iter().zip(b.iter()) {
        diff |= x ^ y;
    }
    diff == 0
}
