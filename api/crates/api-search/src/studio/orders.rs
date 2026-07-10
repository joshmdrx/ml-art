//! M-06 — the artist's sales dashboard.
//!
//!   - `GET  /v1/studio/orders`      — the artist's orders, newest first
//!   - `GET  /v1/studio/orders/:id`  — one order incl. buyer + address
//!   - `POST /v1/studio/orders/:id/ship` — mark a paid order shipped
//!
//! Every handler is scoped to the calling artist via `current_artist_id`
//! (an order that isn't theirs is a 404, never a 403 — same posture as
//! the rest of `/v1/studio/*`). Money is displayed as the artist's
//! payout would land: `amount - commission` (Stripe fees settle later on
//! the balance-transaction, so `payout_cents_gbp` may be null until then;
//! we show the pre-fee estimate meanwhile).

use axum::{
    extract::{Path, State},
    Json,
};
use chrono::{DateTime, Utc};
use ml_art_core::{
    error::ApiError,
    images::url_for_s3_key,
    jobs::{EnqueueOpts, JobEvent, OrderNotifyKind},
};
use serde::{Deserialize, Serialize};
use sqlx::FromRow;
use std::sync::Arc;
use uuid::Uuid;

use crate::extractors::AuthedUser;
use crate::studio::current_artist_id;
use crate::AppState;

/// Upper bound on a tracking number — long enough for any carrier, short
/// enough to reject pasted junk.
const MAX_TRACKING_LEN: usize = 100;

// ─────────────────────────────── List ─────────────────────────────────────

#[derive(Debug, Serialize, FromRow)]
pub struct StudioOrderSummary {
    pub id: Uuid,
    pub status: String,
    pub amount_cents_gbp: i64,
    pub commission_cents_gbp: i64,
    pub buyer_name: Option<String>,
    pub artwork_title: Option<String>,
    pub created_at: DateTime<Utc>,
}

pub async fn list(
    State(state): State<Arc<AppState>>,
    AuthedUser(user): AuthedUser,
) -> Result<Json<Vec<StudioOrderSummary>>, ApiError> {
    let artist_id = current_artist_id(&state.pool, &user).await?;
    let orders: Vec<StudioOrderSummary> = sqlx::query_as(
        r#"
        SELECT
            o.id, o.status, o.amount_cents_gbp, o.commission_cents_gbp,
            u.display_name AS buyer_name,
            a.title AS artwork_title,
            o.created_at
        FROM orders o
        JOIN users    u ON u.id = o.buyer_user_id
        JOIN artworks a ON a.id = o.artwork_id
        WHERE o.artist_id = $1
        ORDER BY o.created_at DESC
        "#,
    )
    .bind(artist_id)
    .fetch_all(&state.pool)
    .await?;
    Ok(Json(orders))
}

// ─────────────────────────────── Detail ───────────────────────────────────

#[derive(Debug, Serialize)]
pub struct StudioOrderDetail {
    pub id: Uuid,
    pub status: String,
    pub amount_cents_gbp: i64,
    pub commission_cents_gbp: i64,
    pub payout_cents_gbp: Option<i64>,
    pub created_at: DateTime<Utc>,
    pub shipped_at: Option<DateTime<Utc>>,
    pub tracking_carrier: Option<String>,
    pub tracking_number: Option<String>,
    pub buyer_name: Option<String>,
    pub buyer_email: Option<String>,
    pub shipping_address: serde_json::Value,
    pub artwork: StudioOrderArtwork,
}

#[derive(Debug, Serialize)]
pub struct StudioOrderArtwork {
    pub id: Uuid,
    pub title: Option<String>,
    pub image_url: Option<String>,
}

#[derive(FromRow)]
struct OrderDetailRow {
    id: Uuid,
    status: String,
    amount_cents_gbp: i64,
    commission_cents_gbp: i64,
    payout_cents_gbp: Option<i64>,
    created_at: DateTime<Utc>,
    shipped_at: Option<DateTime<Utc>>,
    tracking_carrier: Option<String>,
    tracking_number: Option<String>,
    buyer_name: Option<String>,
    buyer_email: Option<String>,
    shipping_address: serde_json::Value,
    artwork_id: Uuid,
    artwork_title: Option<String>,
    image_s3_key: Option<String>,
}

pub async fn detail(
    State(state): State<Arc<AppState>>,
    AuthedUser(user): AuthedUser,
    Path(order_id): Path<Uuid>,
) -> Result<Json<StudioOrderDetail>, ApiError> {
    let artist_id = current_artist_id(&state.pool, &user).await?;
    let row: OrderDetailRow = sqlx::query_as(
        r#"
        SELECT
            o.id, o.status, o.amount_cents_gbp, o.commission_cents_gbp,
            o.payout_cents_gbp, o.created_at, o.shipped_at,
            o.tracking_carrier, o.tracking_number,
            u.display_name AS buyer_name, u.email AS buyer_email,
            o.shipping_address,
            a.id AS artwork_id, a.title AS artwork_title,
            (SELECT s3_key FROM artwork_images
             WHERE artwork_id = a.id AND moderation_status = 'approved'
             ORDER BY is_primary DESC, display_order ASC
             LIMIT 1) AS image_s3_key
        FROM orders o
        JOIN users    u ON u.id = o.buyer_user_id
        JOIN artworks a ON a.id = o.artwork_id
        WHERE o.id = $1 AND o.artist_id = $2
        "#,
    )
    .bind(order_id)
    .bind(artist_id)
    .fetch_optional(&state.pool)
    .await?
    .ok_or(ApiError::NotFound)?;

    Ok(Json(StudioOrderDetail {
        id: row.id,
        status: row.status,
        amount_cents_gbp: row.amount_cents_gbp,
        commission_cents_gbp: row.commission_cents_gbp,
        payout_cents_gbp: row.payout_cents_gbp,
        created_at: row.created_at,
        shipped_at: row.shipped_at,
        tracking_carrier: row.tracking_carrier,
        tracking_number: row.tracking_number,
        buyer_name: row.buyer_name,
        buyer_email: row.buyer_email,
        shipping_address: row.shipping_address,
        artwork: StudioOrderArtwork {
            id: row.artwork_id,
            title: row.artwork_title,
            image_url: row.image_s3_key.as_deref().map(url_for_s3_key),
        },
    }))
}

// ─────────────────────────────── Mark shipped ─────────────────────────────

#[derive(Debug, Deserialize)]
pub struct ShipRequest {
    pub carrier: String,
    pub tracking_number: String,
}

#[derive(Debug, Serialize)]
pub struct ShipAck {
    pub status: String,
}

pub async fn ship(
    State(state): State<Arc<AppState>>,
    AuthedUser(user): AuthedUser,
    Path(order_id): Path<Uuid>,
    Json(req): Json<ShipRequest>,
) -> Result<Json<ShipAck>, ApiError> {
    let artist_id = current_artist_id(&state.pool, &user).await?;

    let carrier = req.carrier.trim();
    let tracking = req.tracking_number.trim();
    if carrier.is_empty() {
        return Err(ApiError::BadRequest("carrier: required".into()));
    }
    if tracking.is_empty() {
        return Err(ApiError::BadRequest("tracking_number: required".into()));
    }
    if tracking.len() > MAX_TRACKING_LEN {
        return Err(ApiError::BadRequest("tracking_number: too long".into()));
    }

    // Only a paid (not-yet-shipped) order the caller owns can transition.
    // The `status = 'paid'` guard makes a double-submit a no-op.
    let updated: Option<(Uuid,)> = sqlx::query_as(
        r#"
        UPDATE orders SET
            status = 'shipped',
            tracking_carrier = $1,
            tracking_number = $2,
            shipped_at = now(),
            updated_at = now()
        WHERE id = $3 AND artist_id = $4 AND status = 'paid'
        RETURNING id
        "#,
    )
    .bind(carrier)
    .bind(tracking)
    .bind(order_id)
    .bind(artist_id)
    .fetch_optional(&state.pool)
    .await?;

    if updated.is_none() {
        // Either not the caller's order, or not in a shippable state.
        // Disambiguate so the studio UI can message correctly.
        let exists: Option<(String,)> =
            sqlx::query_as("SELECT status FROM orders WHERE id = $1 AND artist_id = $2")
                .bind(order_id)
                .bind(artist_id)
                .fetch_optional(&state.pool)
                .await?;
        return match exists {
            None => Err(ApiError::NotFound),
            Some((status,)) => Err(ApiError::Conflict(format!(
                "order is '{status}', not 'paid' — can't mark shipped"
            ))),
        };
    }

    // M-07 — notify the buyer their order shipped (tracking link).
    // Best-effort: the transition already committed. Idempotency key on
    // (order, kind) so a double-submit doesn't double-send.
    if let Err(e) = state
        .jobs
        .enqueue(
            JobEvent::OrderNotify {
                order_id,
                kind: OrderNotifyKind::BuyerShipped,
            },
            EnqueueOpts {
                idempotency_key: Some(format!("order_notify:{order_id}:buyer_shipped")),
                ..Default::default()
            },
        )
        .await
    {
        tracing::error!(%order_id, error = %e, "failed to enqueue shipped notification");
    }

    Ok(Json(ShipAck {
        status: "shipped".into(),
    }))
}
