//! `POST /v1/artworks/:id/inquiries` and `GET /v1/inquiries/verify/:token`.
//!
//! Two flows:
//!
//!   - **Signed-in** users: their email is taken from the Clerk-verified
//!     `users` row (the body's `email` is ignored). The inquiry is created
//!     in a `delivered` state; actual email send to the artist is wired
//!     by `T-032` (Resend + Inngest).
//!
//!   - **Anonymous** users: the inquiry is created in `pending` state with
//!     a one-time verification token. We email the user a link with that
//!     token; clicking it hits `GET /v1/inquiries/verify/:token`, which
//!     marks `verified_at` and `delivered_at` (kicking off delivery).
//!
//! V0 does NOT actually send email — `T-032` lands the Resend integration.
//! Inquiries are stored in the DB and the verification token is returned
//! in the response (dev mode only) so manual testing works.

use axum::{
    extract::{Path, State},
    Json,
};
use ml_art_core::{
    error::ApiError,
    jobs::{EnqueueOpts, JobEvent},
    models::InquiryAck,
};

use crate::extractors::AuthedUser;
use rand::distributions::{Alphanumeric, DistString};
use serde::{Deserialize, Serialize};
use sqlx::FromRow;
use std::sync::Arc;
use uuid::Uuid;

use crate::AppState;

const MAX_MESSAGE_LEN: usize = 4_000;
const MAX_NAME_LEN: usize = 120;

// ─────────────────────────────────────────────────────────────────────────────
// POST /v1/artworks/:id/inquiries
// ─────────────────────────────────────────────────────────────────────────────

#[derive(Debug, Deserialize)]
pub struct CreateInquiry {
    pub name: String,
    /// Required for anonymous senders; ignored for signed-in (we use the
    /// Clerk-verified email from the user record instead).
    #[serde(default)]
    pub email: Option<String>,
    pub message: String,
    /// Free-form for v0 (e.g. "$500–$1,000" or "open"). Could become a
    /// structured enum later.
    #[serde(default)]
    pub budget_range: Option<String>,
}

#[derive(Debug, Serialize)]
pub struct CreateInquiryResponse {
    #[serde(flatten)]
    pub ack: InquiryAck,
    /// Only populated in v0 dev mode where we can't actually send email.
    /// Lets manual testing follow the verification flow. Will go away
    /// when Resend is wired.
    #[serde(skip_serializing_if = "Option::is_none")]
    pub debug_verification_token: Option<String>,
}

pub async fn create(
    State(state): State<Arc<AppState>>,
    Path(artwork_id): Path<Uuid>,
    // Optional: `None` covers both "no Bearer" and "bad Bearer" — the
    // anonymous branch handles both identically (asks for an email,
    // sends a verification link). If we ever need to distinguish, use
    // a `Result<AuthedUser, ApiError>` extractor here instead.
    auth: Option<AuthedUser>,
    Json(body): Json<CreateInquiry>,
) -> Result<Json<CreateInquiryResponse>, ApiError> {
    // Look up the artwork (and its artist). 404 if missing/unpublished.
    let target = sqlx::query_as::<_, InquiryTarget>(
        r#"
        SELECT a.id AS artwork_id, ar.id AS artist_id, ar.inquiry_preferences
        FROM artworks a
        JOIN artists ar ON ar.id = a.artist_id
        WHERE a.id = $1
          AND a.deleted_at IS NULL
          AND a.status = 'published'
          AND ar.deleted_at IS NULL
          AND ar.status = 'active'
        "#,
    )
    .bind(artwork_id)
    .fetch_optional(&state.pool)
    .await?
    .ok_or(ApiError::NotFound)?;

    let name = body.name.trim();
    if name.is_empty() {
        return Err(ApiError::BadRequest("name must not be empty".into()));
    }
    if name.len() > MAX_NAME_LEN {
        return Err(ApiError::BadRequest("name is too long".into()));
    }
    let message = body.message.trim();
    if message.is_empty() {
        return Err(ApiError::BadRequest("message must not be empty".into()));
    }
    if message.len() > MAX_MESSAGE_LEN {
        return Err(ApiError::BadRequest(format!(
            "message exceeds {MAX_MESSAGE_LEN}-char limit"
        )));
    }

    // Branch on auth state. The extractor handed us either Some(user) or None.
    let auth_user = auth.map(|AuthedUser(u)| u);

    let delivery_channel = inquiry_channel(&target.inquiry_preferences);
    let budget_range_json = body
        .budget_range
        .as_deref()
        .map(|s| serde_json::Value::String(s.to_string()));

    if let Some(user) = auth_user {
        // Signed-in: trust Clerk-verified email, deliver immediately.
        let row = sqlx::query_as::<_, InquiryRow>(
            r#"
            INSERT INTO inquiries (
                artwork_id, artist_id, from_user_id, from_email, from_name,
                message, budget_range, delivery_channel,
                verified_at, delivered_at
            )
            VALUES ($1, $2, $3, $4, $5, $6, $7, $8, now(), now())
            RETURNING id
            "#,
        )
        .bind(target.artwork_id)
        .bind(target.artist_id)
        .bind(user.id)
        .bind(&user.email)
        .bind(name)
        .bind(message)
        .bind(budget_range_json)
        .bind(&delivery_channel)
        .fetch_one(&state.pool)
        .await?;

        // Enqueue the artist-notification email. Signed-in case bypasses
        // the verification round-trip (we trust the Clerk-verified email).
        state
            .jobs
            .enqueue(
                JobEvent::InquiryDeliverToArtist { inquiry_id: row.id },
                EnqueueOpts {
                    // Dedup so a flaky double-click can't fire two
                    // notification emails for the same inquiry.
                    idempotency_key: Some(format!("inquiry_deliver:{}", row.id)),
                    ..Default::default()
                },
            )
            .await
            .map_err(|e| ApiError::Internal(anyhow::anyhow!("enqueue deliver: {e}")))?;

        tracing::info!(
            inquiry_id = %row.id,
            artist_id = %target.artist_id,
            from = %user.email,
            channel = %delivery_channel,
            "inquiry delivered (signed-in)"
        );

        return Ok(Json(CreateInquiryResponse {
            ack: InquiryAck {
                id: row.id,
                status: "delivered".to_string(),
            },
            debug_verification_token: None,
        }));
    }

    // Anonymous: email is required; queue with a verification token.
    let email_raw = body
        .email
        .as_deref()
        .ok_or_else(|| ApiError::BadRequest("email is required for anonymous inquiries".into()))?
        .trim();
    if !looks_like_email(email_raw) {
        return Err(ApiError::BadRequest("email is not a valid address".into()));
    }
    let token = new_verification_token();

    let row = sqlx::query_as::<_, InquiryRow>(
        r#"
        INSERT INTO inquiries (
            artwork_id, artist_id, from_email, from_name, message,
            budget_range, delivery_channel, verification_token
        )
        VALUES ($1, $2, $3, $4, $5, $6, $7, $8)
        RETURNING id
        "#,
    )
    .bind(target.artwork_id)
    .bind(target.artist_id)
    .bind(email_raw)
    .bind(name)
    .bind(message)
    .bind(budget_range_json)
    .bind(&delivery_channel)
    .bind(&token)
    .fetch_one(&state.pool)
    .await?;

    // Enqueue the verification email. jobs-worker calls Resend; the
    // inquirer clicks the link in the email → /v1/inquiries/verify
    // flips `delivered_at` → that endpoint enqueues the
    // deliver-to-artist email separately.
    state
        .jobs
        .enqueue(
            JobEvent::InquirySendVerification { inquiry_id: row.id },
            EnqueueOpts {
                idempotency_key: Some(format!("inquiry_verify:{}", row.id)),
                ..Default::default()
            },
        )
        .await
        .map_err(|e| ApiError::Internal(anyhow::anyhow!("enqueue verification: {e}")))?;

    tracing::info!(
        inquiry_id = %row.id,
        token = %token,
        "inquiry pending verification"
    );

    let debug_token = if state.cfg.env.is_dev() {
        Some(token)
    } else {
        None
    };

    Ok(Json(CreateInquiryResponse {
        ack: InquiryAck {
            id: row.id,
            status: "pending_verification".to_string(),
        },
        debug_verification_token: debug_token,
    }))
}

// ─────────────────────────────────────────────────────────────────────────────
// GET /v1/inquiries/verify/:token
// ─────────────────────────────────────────────────────────────────────────────

#[derive(Debug, Serialize)]
pub struct VerifyResponse {
    pub status: String,
}

pub async fn verify(
    State(state): State<Arc<AppState>>,
    Path(token): Path<String>,
) -> Result<Json<VerifyResponse>, ApiError> {
    // Two states matter: already verified (idempotent — return success), and
    // not found (404). We atomically flip pending → verified+delivered.
    let updated = sqlx::query_as::<_, (Uuid, Option<chrono::DateTime<chrono::Utc>>)>(
        r#"
        UPDATE inquiries
           SET verified_at = COALESCE(verified_at, now()),
               delivered_at = COALESCE(delivered_at, now())
         WHERE verification_token = $1
        RETURNING id, delivered_at
        "#,
    )
    .bind(&token)
    .fetch_optional(&state.pool)
    .await?;

    let (id, _) = updated.ok_or(ApiError::NotFound)?;

    // Enqueue the deliver-to-artist email. Same `inquiry_deliver:<id>`
    // idempotency key as the signed-in path, so a verify-link click
    // that races a re-verify can't double-send.
    state
        .jobs
        .enqueue(
            JobEvent::InquiryDeliverToArtist { inquiry_id: id },
            EnqueueOpts {
                idempotency_key: Some(format!("inquiry_deliver:{id}")),
                ..Default::default()
            },
        )
        .await
        .map_err(|e| ApiError::Internal(anyhow::anyhow!("enqueue deliver: {e}")))?;

    tracing::info!(inquiry_id = %id, "inquiry verified + delivery enqueued");

    Ok(Json(VerifyResponse {
        status: "delivered".to_string(),
    }))
}

// ─────────────────────────────────────────────────────────────────────────────
// Helpers
// ─────────────────────────────────────────────────────────────────────────────

#[derive(FromRow)]
struct InquiryTarget {
    artwork_id: Uuid,
    artist_id: Uuid,
    inquiry_preferences: serde_json::Value,
}

#[derive(FromRow)]
struct InquiryRow {
    id: Uuid,
}

fn inquiry_channel(prefs: &serde_json::Value) -> String {
    prefs
        .get("type")
        .and_then(|v| v.as_str())
        .map(|s| s.to_string())
        .unwrap_or_else(|| "platform".to_string())
}

fn new_verification_token() -> String {
    // 32 url-safe chars, ~190 bits of entropy — single-use, fine to ship in
    // a clickable link.
    Alphanumeric.sample_string(&mut rand::thread_rng(), 32)
}

/// Cheap structural check — not RFC 5322 validation. Just guards against
/// "" / "abc" / spaces. Real verification happens on click anyway.
fn looks_like_email(s: &str) -> bool {
    let s = s.trim();
    if s.len() < 3 || s.len() > 254 {
        return false;
    }
    let mut parts = s.split('@');
    let local = parts.next().unwrap_or("");
    let domain = parts.next().unwrap_or("");
    if parts.next().is_some() {
        return false; // multiple @
    }
    !local.is_empty() && !domain.is_empty() && domain.contains('.') && !s.contains(' ')
}
