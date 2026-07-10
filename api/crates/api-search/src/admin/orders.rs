//! M-08 — admin marketplace order queue + refund flow.
//!
//!   - `GET  /v1/admin/orders`         — all orders, optional `?status=`
//!   - `GET  /v1/admin/orders/:id`     — one order, full detail
//!   - `POST /v1/admin/orders/:id/refund` — refund a paid/shipped/
//!     delivered order (fires the Stripe refund; the `charge.refunded`
//!     webhook confirms → status `refunded` + buyer/artist emails)
//!
//! Admin-only via the `AdminUser` extractor. Refunds are audited (the
//! intent is recorded *before* the Stripe call, same posture as the
//! artist-status transitions) and idempotent Stripe-side on the order id.

use axum::{
    extract::{Path, Query, State},
    Json,
};
use chrono::{DateTime, Utc};
use ml_art_core::{
    admin::{action, audit, target},
    error::ApiError,
    images::url_for_s3_key,
    stripe::StripeClient,
};
use serde::{Deserialize, Serialize};
use sqlx::FromRow;
use std::sync::Arc;
use uuid::Uuid;

use crate::extractors::AdminUser;
use crate::AppState;

// ─────────────────────────────── List ─────────────────────────────────────

#[derive(Debug, Deserialize)]
pub struct ListParams {
    /// Optional status filter (`paid`, `shipped`, `disputed`, …).
    pub status: Option<String>,
}

#[derive(Debug, Serialize, FromRow)]
pub struct AdminOrderSummary {
    pub id: Uuid,
    pub status: String,
    pub amount_cents_gbp: i64,
    pub commission_cents_gbp: i64,
    pub buyer_name: Option<String>,
    pub artist_name: String,
    pub artwork_title: Option<String>,
    pub created_at: DateTime<Utc>,
}

pub async fn list(
    State(state): State<Arc<AppState>>,
    AdminUser(_admin): AdminUser,
    Query(params): Query<ListParams>,
) -> Result<Json<Vec<AdminOrderSummary>>, ApiError> {
    // `$1 IS NULL OR status = $1` keeps the filter optional without
    // dynamic SQL. Newest first.
    let orders: Vec<AdminOrderSummary> = sqlx::query_as(
        r#"
        SELECT
            o.id, o.status, o.amount_cents_gbp, o.commission_cents_gbp,
            bu.display_name AS buyer_name,
            ar.display_name AS artist_name,
            a.title AS artwork_title,
            o.created_at
        FROM orders o
        JOIN users    bu ON bu.id = o.buyer_user_id
        JOIN artists  ar ON ar.id = o.artist_id
        JOIN artworks a  ON a.id = o.artwork_id
        WHERE ($1::text IS NULL OR o.status = $1)
        ORDER BY o.created_at DESC
        "#,
    )
    .bind(params.status.as_deref())
    .fetch_all(&state.pool)
    .await?;
    Ok(Json(orders))
}

// ─────────────────────────────── Detail ───────────────────────────────────

#[derive(Debug, Serialize)]
pub struct AdminOrderDetail {
    pub id: Uuid,
    pub status: String,
    pub amount_cents_gbp: i64,
    pub commission_cents_gbp: i64,
    pub payout_cents_gbp: Option<i64>,
    pub stripe_payment_intent_id: Option<String>,
    pub refund_reason: Option<String>,
    pub tracking_carrier: Option<String>,
    pub tracking_number: Option<String>,
    pub created_at: DateTime<Utc>,
    pub shipping_address: serde_json::Value,
    pub buyer_name: Option<String>,
    pub buyer_email: Option<String>,
    pub artist_name: String,
    pub artwork: AdminOrderArtwork,
}

#[derive(Debug, Serialize)]
pub struct AdminOrderArtwork {
    pub id: Uuid,
    pub title: Option<String>,
    pub image_url: Option<String>,
}

#[derive(FromRow)]
struct DetailRow {
    id: Uuid,
    status: String,
    amount_cents_gbp: i64,
    commission_cents_gbp: i64,
    payout_cents_gbp: Option<i64>,
    stripe_payment_intent_id: Option<String>,
    refund_reason: Option<String>,
    tracking_carrier: Option<String>,
    tracking_number: Option<String>,
    created_at: DateTime<Utc>,
    shipping_address: serde_json::Value,
    buyer_name: Option<String>,
    buyer_email: Option<String>,
    artist_name: String,
    artwork_id: Uuid,
    artwork_title: Option<String>,
    image_s3_key: Option<String>,
}

pub async fn detail(
    State(state): State<Arc<AppState>>,
    AdminUser(_admin): AdminUser,
    Path(order_id): Path<Uuid>,
) -> Result<Json<AdminOrderDetail>, ApiError> {
    let row: DetailRow = sqlx::query_as(
        r#"
        SELECT
            o.id, o.status, o.amount_cents_gbp, o.commission_cents_gbp,
            o.payout_cents_gbp, o.stripe_payment_intent_id, o.refund_reason,
            o.tracking_carrier, o.tracking_number, o.created_at, o.shipping_address,
            bu.display_name AS buyer_name, bu.email AS buyer_email,
            ar.display_name AS artist_name,
            a.id AS artwork_id, a.title AS artwork_title,
            (SELECT s3_key FROM artwork_images
             WHERE artwork_id = a.id AND moderation_status = 'approved'
             ORDER BY is_primary DESC, display_order ASC
             LIMIT 1) AS image_s3_key
        FROM orders o
        JOIN users    bu ON bu.id = o.buyer_user_id
        JOIN artists  ar ON ar.id = o.artist_id
        JOIN artworks a  ON a.id = o.artwork_id
        WHERE o.id = $1
        "#,
    )
    .bind(order_id)
    .fetch_optional(&state.pool)
    .await?
    .ok_or(ApiError::NotFound)?;

    Ok(Json(AdminOrderDetail {
        id: row.id,
        status: row.status,
        amount_cents_gbp: row.amount_cents_gbp,
        commission_cents_gbp: row.commission_cents_gbp,
        payout_cents_gbp: row.payout_cents_gbp,
        stripe_payment_intent_id: row.stripe_payment_intent_id,
        refund_reason: row.refund_reason,
        tracking_carrier: row.tracking_carrier,
        tracking_number: row.tracking_number,
        created_at: row.created_at,
        shipping_address: row.shipping_address,
        buyer_name: row.buyer_name,
        buyer_email: row.buyer_email,
        artist_name: row.artist_name,
        artwork: AdminOrderArtwork {
            id: row.artwork_id,
            title: row.artwork_title,
            image_url: row.image_s3_key.as_deref().map(url_for_s3_key),
        },
    }))
}

// ─────────────────────────────── Refund ───────────────────────────────────

#[derive(Debug, Deserialize)]
pub struct RefundRequest {
    /// Reason picker value — defective / not-as-described / non-delivery /
    /// artist-cancelled / other. Free text server-side; the UI constrains.
    pub reason: String,
}

#[derive(Debug, Serialize)]
pub struct RefundAck {
    pub refund_id: String,
    pub status: String,
}

pub async fn refund(
    State(state): State<Arc<AppState>>,
    AdminUser(admin): AdminUser,
    Path(order_id): Path<Uuid>,
    Json(req): Json<RefundRequest>,
) -> Result<Json<RefundAck>, ApiError> {
    let client = StripeClient::from_config(state.cfg.stripe_secret_key.as_deref())
        .ok_or_else(|| ApiError::ServiceUnavailable("stripe not configured".into()))?;

    let reason = req.reason.trim();
    if reason.is_empty() {
        return Err(ApiError::BadRequest("reason: required".into()));
    }

    let row: Option<(String, Option<String>)> =
        sqlx::query_as("SELECT status, stripe_payment_intent_id FROM orders WHERE id = $1")
            .bind(order_id)
            .fetch_optional(&state.pool)
            .await?;
    let (status, payment_intent) = row.ok_or(ApiError::NotFound)?;

    // Only a settled charge can be refunded. `refunded`/`cancelled`/
    // `pending` are 409s (nothing to unwind, or already unwound).
    if !matches!(
        status.as_str(),
        "paid" | "shipped" | "delivered" | "disputed"
    ) {
        return Err(ApiError::Conflict(format!(
            "order is '{status}' — not refundable"
        )));
    }
    let Some(payment_intent) = payment_intent else {
        return Err(ApiError::Conflict(
            "order has no captured payment to refund".into(),
        ));
    };

    // Audit the intent before the mutation (same posture as artist
    // transitions — records the attempt even if Stripe then errors).
    audit::record(
        &state.pool,
        Some(admin.id),
        action::ORDER_REFUND,
        target::ORDER,
        Some(order_id),
        Some(&serde_json::json!({ "status": status })),
        None::<&serde_json::Value>,
        Some(serde_json::json!({ "reason": reason })),
    )
    .await?;

    // Record the reason now; the `charge.refunded` webhook flips status →
    // `refunded`, stamps `refunded_at`, and fires the buyer/artist emails.
    sqlx::query("UPDATE orders SET refund_reason = $1, updated_at = now() WHERE id = $2")
        .bind(reason)
        .bind(order_id)
        .execute(&state.pool)
        .await?;

    let refund = client
        .create_refund(&payment_intent, order_id)
        .await
        .map_err(|e| ApiError::Internal(anyhow::anyhow!("stripe refund: {e}")))?;

    tracing::info!(%order_id, refund_id = %refund.id, reason, "admin refund fired");
    Ok(Json(RefundAck {
        refund_id: refund.id,
        status: refund.status.unwrap_or_else(|| "pending".into()),
    }))
}
