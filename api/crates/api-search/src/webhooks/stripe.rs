//! M-03 — Stripe webhook receiver for the marketplace.
//!
//! `POST /v1/webhooks/stripe` is Stripe's callback for asynchronous
//! events (a checkout completing, Connect KYC finishing, a dispute
//! opening). It:
//!
//!   1. verifies the `Stripe-Signature` HMAC against the **raw** request
//!      body (`core::stripe::verify_webhook_signature`) — re-serialising
//!      parsed JSON changes the bytes and breaks the HMAC, so we read the
//!      body as `Bytes` and verify before we deserialise;
//!   2. dedups on Stripe's event id via `stripe_webhook_events`
//!      (`INSERT ON CONFLICT DO NOTHING`) — Stripe retries aggressively
//!      (>= 3× within the hour on a non-2xx), so every event is
//!      replay-idempotent. Same shape as the T-054 inbound-email dedup;
//!   3. dispatches on event type to advance order / artist state.
//!
//! Dedup + state change run in **one transaction**: if dispatch fails,
//! the dedup row rolls back with it, so Stripe's retry reprocesses
//! cleanly instead of us acking a half-applied event.
//!
//! Handled today:
//!   - `account.updated` — Connect KYC → `artists.stripe_*` flags (the
//!     async half of M-01's onboarding);
//!   - `checkout.session.completed` — order `pending → paid` (M-04);
//!   - `charge.dispute.created` — order `→ disputed`;
//!   - `charge.refunded` — order `→ refunded` (confirms an M-08 refund).
//!
//! Unrecognised types are acked and ignored — Stripe delivers many event
//! types we don't act on. The buyer/artist/admin notifications that hang
//! off these transitions land with M-07.

use axum::{body::Bytes, extract::State, http::HeaderMap, Json};
use chrono::Utc;
use ml_art_core::{
    error::ApiError,
    jobs::{EnqueueOpts, JobEvent, OrderNotifyKind},
    stripe::verify_webhook_signature,
};
use serde::{Deserialize, Serialize};
use sqlx::{Postgres, Transaction};
use std::sync::Arc;
use uuid::Uuid;

use crate::AppState;

/// The header Stripe signs each delivery with.
const SIGNATURE_HEADER: &str = "stripe-signature";

/// Minimal projection of a Stripe event envelope. `data.object` stays a
/// raw `Value` — each handler reads only the few fields it needs, so we
/// don't chase Stripe's (large, versioned) object schemas.
#[derive(Debug, Deserialize)]
struct StripeEvent {
    /// `evt_*` — the dedup key.
    id: String,
    #[serde(rename = "type")]
    event_type: String,
    data: EventData,
}

#[derive(Debug, Deserialize)]
struct EventData {
    object: serde_json::Value,
}

#[derive(Debug, Serialize)]
pub struct Ack {
    /// `"processed"` for a freshly-handled event; `"duplicate"` when the
    /// event id was already recorded (idempotent replay); `"ignored"`
    /// for an event type we don't act on.
    status: &'static str,
}

pub async fn handle(
    State(state): State<Arc<AppState>>,
    headers: HeaderMap,
    body: Bytes,
) -> Result<Json<Ack>, ApiError> {
    // 1. Signature verification over the raw body. When the webhook
    //    secret is configured we enforce it (prod, and local `stripe
    //    listen`); when it's unset we skip — a documented dev shortcut so
    //    the endpoint is reachable without the Stripe CLI running.
    match state.cfg.stripe_webhook_secret.as_deref() {
        Some(secret) => {
            let sig = headers
                .get(SIGNATURE_HEADER)
                .and_then(|v| v.to_str().ok())
                .ok_or_else(|| ApiError::BadRequest("missing Stripe-Signature header".into()))?;
            verify_webhook_signature(&body, sig, secret, Utc::now().timestamp()).map_err(|e| {
                tracing::warn!(error = %e, "stripe webhook signature verification failed");
                ApiError::Unauthorized
            })?;
        }
        None => {
            tracing::warn!(
                "STRIPE_WEBHOOK_SECRET unset — skipping Stripe webhook signature \
                 verification (dev only; set the whsec_ secret in prod)"
            );
        }
    }

    // 2. Parse. `raw` is stored verbatim for forensics; `event` is the
    //    typed projection we dispatch on.
    let raw: serde_json::Value =
        serde_json::from_slice(&body).map_err(|_| ApiError::BadRequest("invalid JSON".into()))?;
    let event: StripeEvent = serde_json::from_value(raw.clone())
        .map_err(|_| ApiError::BadRequest("unexpected event envelope".into()))?;

    // 3. Claim the event id (dedup) + dispatch, atomically.
    let mut tx = state.pool.begin().await?;

    let claimed: Option<(String,)> = sqlx::query_as(
        r#"
        INSERT INTO stripe_webhook_events (event_id, event_type, raw_payload)
        VALUES ($1, $2, $3)
        ON CONFLICT (event_id) DO NOTHING
        RETURNING event_id
        "#,
    )
    .bind(&event.id)
    .bind(&event.event_type)
    .bind(&raw)
    .fetch_optional(&mut *tx)
    .await?;

    if claimed.is_none() {
        // Already processed — drop the tx (nothing written) and ack.
        tracing::info!(event_id = %event.id, "stripe webhook duplicate — acking");
        return Ok(Json(Ack {
            status: "duplicate",
        }));
    }

    let dispatched = dispatch(&mut tx, &event).await?;

    // Backfill the affected order onto the event row for later forensics
    // ("which refund/dispute touched this order?").
    if let Some(order_id) = dispatched.order_id {
        sqlx::query("UPDATE stripe_webhook_events SET order_id = $1 WHERE event_id = $2")
            .bind(order_id)
            .bind(&event.id)
            .execute(&mut *tx)
            .await?;
    }

    tx.commit().await?;

    // M-07 — fan out notification jobs *after* the state change is
    // durable. Enqueue is best-effort: the webhook already committed +
    // returns 200, so a failed enqueue is a lost email, not a retried
    // event. Idempotency key on (order, kind) makes a Stripe replay that
    // somehow re-transitions a no-op.
    if let Some(order_id) = dispatched.order_id {
        for kind in &dispatched.notifications {
            let key = format!("order_notify:{order_id}:{}", notify_key(*kind));
            if let Err(e) = state
                .jobs
                .enqueue(
                    JobEvent::OrderNotify {
                        order_id,
                        kind: *kind,
                    },
                    EnqueueOpts {
                        idempotency_key: Some(key),
                        ..Default::default()
                    },
                )
                .await
            {
                tracing::error!(%order_id, ?kind, error = %e, "failed to enqueue order notification");
            }
        }
    }

    Ok(Json(Ack {
        status: dispatched.status,
    }))
}

/// Stable string per notification kind for the idempotency key.
fn notify_key(kind: OrderNotifyKind) -> &'static str {
    match kind {
        OrderNotifyKind::BuyerPaid => "buyer_paid",
        OrderNotifyKind::ArtistSale => "artist_sale",
        OrderNotifyKind::BuyerShipped => "buyer_shipped",
        OrderNotifyKind::BuyerRefunded => "buyer_refunded",
        OrderNotifyKind::ArtistRefunded => "artist_refunded",
    }
}

/// What a dispatch produced: the order it touched (if any, for the
/// forensics backfill), the ack status, and the M-07 notifications to
/// enqueue once the transaction commits.
struct Dispatched {
    order_id: Option<Uuid>,
    status: &'static str,
    notifications: Vec<OrderNotifyKind>,
}

async fn dispatch(
    tx: &mut Transaction<'_, Postgres>,
    event: &StripeEvent,
) -> Result<Dispatched, ApiError> {
    let obj = &event.data.object;
    // Each arm returns the order it touched + the notifications that state
    // change warrants. Notifications only fire when a row actually
    // transitioned (order_id is Some), so a replay/no-op doesn't email.
    let (order_id, notifications) = match event.event_type.as_str() {
        "account.updated" => {
            account_updated(tx, obj).await?;
            (None, vec![])
        }
        "checkout.session.completed" => {
            let id = checkout_completed(tx, obj).await?;
            let notes = if id.is_some() {
                vec![OrderNotifyKind::BuyerPaid, OrderNotifyKind::ArtistSale]
            } else {
                vec![]
            };
            (id, notes)
        }
        "charge.dispute.created" => {
            // TODO(M-07 followup): alert admin (needs an admin-email/SNS
            // sink). No buyer/artist email on a dispute.
            (dispute_created(tx, obj).await?, vec![])
        }
        "charge.refunded" => {
            let id = charge_refunded(tx, obj).await?;
            let notes = if id.is_some() {
                vec![
                    OrderNotifyKind::BuyerRefunded,
                    OrderNotifyKind::ArtistRefunded,
                ]
            } else {
                vec![]
            };
            (id, notes)
        }
        other => {
            tracing::debug!(
                event_type = other,
                "stripe webhook type not handled — ignoring"
            );
            return Ok(Dispatched {
                order_id: None,
                status: "ignored",
                notifications: vec![],
            });
        }
    };
    Ok(Dispatched {
        order_id,
        status: "processed",
        notifications,
    })
}

/// `account.updated` — Connect KYC progress. Flip the artist's capability
/// flags to match Stripe, and stamp `stripe_onboarded_at` the first time
/// charges go live. No-op if the account id isn't one of ours.
async fn account_updated(
    tx: &mut Transaction<'_, Postgres>,
    obj: &serde_json::Value,
) -> Result<(), ApiError> {
    let Some(account_id) = obj.get("id").and_then(|v| v.as_str()) else {
        return Ok(());
    };
    let charges_enabled = obj
        .get("charges_enabled")
        .and_then(|v| v.as_bool())
        .unwrap_or(false);
    let payouts_enabled = obj
        .get("payouts_enabled")
        .and_then(|v| v.as_bool())
        .unwrap_or(false);

    sqlx::query(
        r#"
        UPDATE artists SET
            stripe_charges_enabled = $1,
            stripe_payouts_enabled = $2,
            stripe_onboarded_at = COALESCE(
                stripe_onboarded_at,
                CASE WHEN $1 THEN now() ELSE NULL END
            ),
            updated_at = now()
        WHERE stripe_account_id = $3
        "#,
    )
    .bind(charges_enabled)
    .bind(payouts_enabled)
    .bind(account_id)
    .execute(&mut **tx)
    .await?;
    Ok(())
}

/// `checkout.session.completed` — the buyer paid. Advance the matching
/// `pending` order to `paid` and capture the payment-intent id (the
/// truth for later refund + reconciliation). Guarded on `status =
/// 'pending'` so a replayed/late event can't drag a shipped order back.
///
/// TODO(M-04): fetch the charge's balance-transaction to populate
/// `stripe_fee_cents_gbp` + `payout_cents_gbp` (needs the checkout →
/// order wiring that M-04 introduces).
async fn checkout_completed(
    tx: &mut Transaction<'_, Postgres>,
    obj: &serde_json::Value,
) -> Result<Option<Uuid>, ApiError> {
    let Some(session_id) = obj.get("id").and_then(|v| v.as_str()) else {
        return Ok(None);
    };
    let payment_intent = obj.get("payment_intent").and_then(|v| v.as_str());

    let row: Option<(Uuid,)> = sqlx::query_as(
        r#"
        UPDATE orders SET
            status = 'paid',
            stripe_payment_intent_id = COALESCE($2, stripe_payment_intent_id),
            updated_at = now()
        WHERE stripe_checkout_session_id = $1 AND status = 'pending'
        RETURNING id
        "#,
    )
    .bind(session_id)
    .bind(payment_intent)
    .fetch_optional(&mut **tx)
    .await?;

    // TODO(M-07): enqueue buyer order-confirmation + artist sale
    // notifications when a row transitioned here.
    Ok(row.map(|(id,)| id))
}

/// `charge.dispute.created` — a chargeback was filed. Move the order to
/// `disputed`; the admin handles evidence via Stripe's own UI (M-08 deep
/// link). Keyed on the payment intent the dispute references.
async fn dispute_created(
    tx: &mut Transaction<'_, Postgres>,
    obj: &serde_json::Value,
) -> Result<Option<Uuid>, ApiError> {
    let Some(payment_intent) = obj.get("payment_intent").and_then(|v| v.as_str()) else {
        return Ok(None);
    };
    let row: Option<(Uuid,)> = sqlx::query_as(
        r#"
        UPDATE orders SET status = 'disputed', updated_at = now()
        WHERE stripe_payment_intent_id = $1
        RETURNING id
        "#,
    )
    .bind(payment_intent)
    .fetch_optional(&mut **tx)
    .await?;

    // TODO(M-07): alert admin (SNS + email) on any dispute.
    Ok(row.map(|(id,)| id))
}

/// `charge.refunded` — a refund settled. Confirms the admin-initiated
/// refund from M-08 (or a full refund made in the Stripe dashboard).
/// Idempotent: skips orders already `refunded`.
async fn charge_refunded(
    tx: &mut Transaction<'_, Postgres>,
    obj: &serde_json::Value,
) -> Result<Option<Uuid>, ApiError> {
    let Some(payment_intent) = obj.get("payment_intent").and_then(|v| v.as_str()) else {
        return Ok(None);
    };
    let row: Option<(Uuid,)> = sqlx::query_as(
        r#"
        UPDATE orders SET status = 'refunded', refunded_at = now(), updated_at = now()
        WHERE stripe_payment_intent_id = $1 AND status <> 'refunded'
        RETURNING id
        "#,
    )
    .bind(payment_intent)
    .fetch_optional(&mut **tx)
    .await?;

    // TODO(M-07): buyer + artist refund-confirmation notifications.
    Ok(row.map(|(id,)| id))
}
