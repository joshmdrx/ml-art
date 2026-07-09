//! M-04 — buyer checkout for the direct-sales marketplace.
//!
//! `POST /v1/artworks/:id/checkout` (authed buyer) validates that the
//! artwork is *sellable*, captures the shipping address, creates a
//! `pending` order, then opens a Stripe Checkout Session (a Connect
//! destination charge: buyer pays on the platform account, Stripe takes
//! Wander's commission as the application fee and routes the balance to
//! the artist's connected account). Returns the hosted-checkout URL for
//! the client to redirect to.
//!
//! The order is written *before* the Stripe call so `checkout.session.
//! completed` (M-03) can map the session back to it. Double-submit is
//! idempotent: a `pending` order for the same (buyer, artwork) < 30 min
//! old is reused rather than duplicated (Stripe-side key on the order id
//! is the second guard).
//!
//! Sellable = published + available + priced (GBP) + has dimensions,
//! weight, and ships-from country + the artist has completed Stripe
//! onboarding (`stripe_charges_enabled`). Anything missing is a 400 with
//! a specific reason (the Buy button in M-05 mirrors these predicates, so
//! this is defence-in-depth rather than the primary UX gate).

use axum::{
    extract::{Path, State},
    Json,
};
use chrono::{DateTime, Utc};
use ml_art_core::{
    error::ApiError,
    images::url_for_s3_key,
    stripe::{CheckoutSessionRequest, StripeClient, StripeShipping},
};
use serde::{Deserialize, Serialize};
use sqlx::FromRow;
use std::sync::Arc;
use uuid::Uuid;

use crate::extractors::AuthedUser;
use crate::AppState;

/// Wander's commission, in basis points of the gross sale price. 15%
/// flat for v1 (see plans/marketplace.md — revisit once we have
/// conversion data). Recorded per-order at creation so a future rate
/// change doesn't rewrite past orders.
const COMMISSION_BPS: i64 = 1500;

#[derive(Debug, Deserialize)]
pub struct CheckoutRequest {
    pub shipping_address: ShippingAddress,
}

/// Buyer-entered destination address. Stored on the order as jsonb and
/// forwarded to Stripe. `name` is the recipient, not the account holder.
#[derive(Debug, Deserialize, Serialize)]
pub struct ShippingAddress {
    pub name: String,
    pub line1: String,
    pub line2: Option<String>,
    pub city: String,
    pub postal_code: String,
    /// ISO-3166 alpha-2.
    pub country: String,
}

#[derive(Debug, Serialize)]
pub struct CheckoutResponse {
    /// The Stripe hosted-checkout URL to redirect the buyer to.
    pub checkout_url: String,
    /// The pending order id (the buyer lands on `/orders/:id` after pay).
    pub order_id: Uuid,
}

/// Sellability projection — the artwork plus its artist's payout state.
#[derive(FromRow)]
struct SellableRow {
    title: Option<String>,
    price_gbp_cents: Option<i64>,
    availability: String,
    status: String,
    has_dimensions: bool,
    weight_grams: Option<i32>,
    ships_from_country: Option<String>,
    artist_id: Uuid,
    artist_user_id: Uuid,
    stripe_account_id: Option<String>,
    stripe_charges_enabled: bool,
}

pub async fn create_checkout(
    State(state): State<Arc<AppState>>,
    AuthedUser(user): AuthedUser,
    Path(artwork_id): Path<Uuid>,
    Json(req): Json<CheckoutRequest>,
) -> Result<Json<CheckoutResponse>, ApiError> {
    // Gate: marketplace disabled without a Stripe key (dev).
    let client = StripeClient::from_config(state.cfg.stripe_secret_key.as_deref())
        .ok_or_else(|| ApiError::ServiceUnavailable("stripe not configured".into()))?;

    validate_address(&req.shipping_address)?;

    // Pull the artwork + its artist's payout state in one shot.
    let row: SellableRow = sqlx::query_as(
        r#"
        SELECT
            a.title,
            a.price_gbp_cents,
            a.availability,
            a.status,
            a.dimensions IS NOT NULL AS has_dimensions,
            a.weight_grams,
            a.ships_from_country,
            ar.id           AS artist_id,
            ar.user_id      AS artist_user_id,
            ar.stripe_account_id,
            ar.stripe_charges_enabled
        FROM artworks a
        JOIN artists ar ON ar.id = a.artist_id
        WHERE a.id = $1 AND a.deleted_at IS NULL
        "#,
    )
    .bind(artwork_id)
    .fetch_optional(&state.pool)
    .await?
    .ok_or(ApiError::NotFound)?;

    let (amount_cents, account_id) = check_sellable(&row, user.id)?;
    let commission_cents = amount_cents * COMMISSION_BPS / 10_000;

    // Idempotency: reuse a fresh pending order for this (buyer, artwork)
    // rather than spawning a duplicate on a double-click.
    let existing: Option<(Uuid,)> = sqlx::query_as(
        r#"
        SELECT id FROM orders
        WHERE buyer_user_id = $1 AND artwork_id = $2 AND status = 'pending'
          AND created_at > now() - interval '30 minutes'
        ORDER BY created_at DESC
        LIMIT 1
        "#,
    )
    .bind(user.id)
    .bind(artwork_id)
    .fetch_optional(&state.pool)
    .await?;

    let shipping_json = serde_json::to_value(&req.shipping_address)
        .map_err(|e| ApiError::Internal(anyhow::anyhow!("serialise shipping: {e}")))?;

    let order_id = match existing {
        Some((id,)) => id,
        None => {
            sqlx::query_scalar(
                r#"
            INSERT INTO orders (
                buyer_user_id, artwork_id, artist_id,
                amount_cents_gbp, commission_cents_gbp,
                status, shipping_address
            )
            VALUES ($1, $2, $3, $4, $5, 'pending', $6)
            RETURNING id
            "#,
            )
            .bind(user.id)
            .bind(artwork_id)
            .bind(row.artist_id)
            .bind(amount_cents)
            .bind(commission_cents)
            .bind(&shipping_json)
            .fetch_one(&state.pool)
            .await?
        }
    };

    // Open the hosted Checkout Session for this order.
    let order_ref = order_id.to_string();
    let success_url = format!(
        "{}/orders/{}?checkout=success",
        state.cfg.web_base_url, order_id
    );
    let cancel_url = format!(
        "{}/artworks/{}?checkout=cancelled",
        state.cfg.web_base_url, artwork_id
    );
    let addr = &req.shipping_address;
    let session = client
        .create_checkout_session(&CheckoutSessionRequest {
            client_reference_id: &order_ref,
            product_name: row.title.as_deref().unwrap_or("Untitled artwork"),
            amount_cents,
            currency: "gbp",
            application_fee_cents: commission_cents,
            destination_account: &account_id,
            success_url: &success_url,
            cancel_url: &cancel_url,
            shipping: StripeShipping {
                name: &addr.name,
                line1: &addr.line1,
                line2: addr.line2.as_deref(),
                city: &addr.city,
                postal_code: &addr.postal_code,
                country: &addr.country,
            },
        })
        .await
        .map_err(|e| ApiError::Internal(anyhow::anyhow!("stripe checkout create: {e}")))?;

    // Record the session id so the webhook can match it back to the order.
    sqlx::query(
        "UPDATE orders SET stripe_checkout_session_id = $1, updated_at = now() WHERE id = $2",
    )
    .bind(&session.id)
    .bind(order_id)
    .execute(&state.pool)
    .await?;

    let checkout_url = session
        .url
        .ok_or_else(|| ApiError::Internal(anyhow::anyhow!("stripe returned no checkout url")))?;

    Ok(Json(CheckoutResponse {
        checkout_url,
        order_id,
    }))
}

// ─────────────────────────────── Order detail ─────────────────────────────
//
// GET /v1/orders/:id — the buyer's own order. Powers the post-checkout
// confirmation page (`/orders/[id]`). Scoped to the caller: an order that
// isn't theirs is a 404, not a 403.

#[derive(Debug, Serialize)]
pub struct OrderDetail {
    pub id: Uuid,
    /// pending | paid | shipped | delivered | cancelled | refunded | disputed
    pub status: String,
    pub amount_cents_gbp: i64,
    /// Always `"gbp"` for v1 — orders settle in the canonical currency.
    pub currency: &'static str,
    pub created_at: DateTime<Utc>,
    pub shipping_address: serde_json::Value,
    pub artwork: OrderArtwork,
}

#[derive(Debug, Serialize)]
pub struct OrderArtwork {
    pub id: Uuid,
    pub title: Option<String>,
    pub image_url: Option<String>,
    pub artist_name: String,
    pub artist_slug: String,
}

#[derive(FromRow)]
struct OrderRow {
    id: Uuid,
    status: String,
    amount_cents_gbp: i64,
    created_at: DateTime<Utc>,
    shipping_address: serde_json::Value,
    artwork_id: Uuid,
    artwork_title: Option<String>,
    image_s3_key: Option<String>,
    artist_name: String,
    artist_slug: String,
}

pub async fn get_order(
    State(state): State<Arc<AppState>>,
    AuthedUser(user): AuthedUser,
    Path(order_id): Path<Uuid>,
) -> Result<Json<OrderDetail>, ApiError> {
    let row: OrderRow = sqlx::query_as(
        r#"
        SELECT
            o.id, o.status, o.amount_cents_gbp, o.created_at, o.shipping_address,
            a.id AS artwork_id, a.title AS artwork_title,
            (SELECT s3_key FROM artwork_images
             WHERE artwork_id = a.id AND moderation_status = 'approved'
             ORDER BY is_primary DESC, display_order ASC
             LIMIT 1) AS image_s3_key,
            ar.display_name AS artist_name, ar.slug AS artist_slug
        FROM orders o
        JOIN artworks a  ON a.id  = o.artwork_id
        JOIN artists  ar ON ar.id = o.artist_id
        WHERE o.id = $1 AND o.buyer_user_id = $2
        "#,
    )
    .bind(order_id)
    .bind(user.id)
    .fetch_optional(&state.pool)
    .await?
    .ok_or(ApiError::NotFound)?;

    Ok(Json(OrderDetail {
        id: row.id,
        status: row.status,
        amount_cents_gbp: row.amount_cents_gbp,
        currency: "gbp",
        created_at: row.created_at,
        shipping_address: row.shipping_address,
        artwork: OrderArtwork {
            id: row.artwork_id,
            title: row.artwork_title,
            image_url: row.image_s3_key.as_deref().map(url_for_s3_key),
            artist_name: row.artist_name,
            artist_slug: row.artist_slug,
        },
    }))
}

/// Return `(amount_cents, artist_account_id)` if the artwork can be bought
/// by `buyer_id`, else a 400 naming the first failing predicate.
fn check_sellable(row: &SellableRow, buyer_id: Uuid) -> Result<(i64, String), ApiError> {
    if row.artist_user_id == buyer_id {
        return Err(ApiError::BadRequest(
            "you can't buy your own artwork".into(),
        ));
    }
    if row.status != "published" {
        return Err(ApiError::BadRequest("artwork is not published".into()));
    }
    if row.availability != "available" {
        return Err(ApiError::BadRequest(
            "artwork is not available for sale".into(),
        ));
    }
    if !row.has_dimensions {
        return Err(ApiError::BadRequest("artwork is missing dimensions".into()));
    }
    if row.weight_grams.is_none() {
        return Err(ApiError::BadRequest(
            "artwork is missing shipping weight".into(),
        ));
    }
    if row.ships_from_country.is_none() {
        return Err(ApiError::BadRequest(
            "artwork is missing a ships-from country".into(),
        ));
    }
    if !row.stripe_charges_enabled {
        return Err(ApiError::BadRequest(
            "this artist is not set up for direct purchase yet".into(),
        ));
    }
    let account_id = row.stripe_account_id.clone().ok_or_else(|| {
        ApiError::BadRequest("this artist is not set up for direct purchase yet".into())
    })?;
    let amount = row
        .price_gbp_cents
        .filter(|&p| p > 0)
        .ok_or_else(|| ApiError::BadRequest("artwork has no price".into()))?;
    Ok((amount, account_id))
}

fn validate_address(a: &ShippingAddress) -> Result<(), ApiError> {
    let required = [
        ("name", &a.name),
        ("line1", &a.line1),
        ("city", &a.city),
        ("postal_code", &a.postal_code),
        ("country", &a.country),
    ];
    for (field, val) in required {
        if val.trim().is_empty() {
            return Err(ApiError::BadRequest(format!("shipping {field}: required")));
        }
    }
    if a.country.trim().len() != 2 {
        return Err(ApiError::BadRequest(
            "shipping country: must be a 2-letter ISO code".into(),
        ));
    }
    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;

    const BUYER: Uuid = Uuid::from_u128(1);
    const ARTIST_USER: Uuid = Uuid::from_u128(2);

    /// A fully-sellable row: £500, all fields present, artist onboarded.
    fn sellable_row() -> SellableRow {
        SellableRow {
            title: Some("Blue Morning".into()),
            price_gbp_cents: Some(50_000),
            availability: "available".into(),
            status: "published".into(),
            has_dimensions: true,
            weight_grams: Some(2_000),
            ships_from_country: Some("GB".into()),
            artist_id: Uuid::from_u128(9),
            artist_user_id: ARTIST_USER,
            stripe_account_id: Some("acct_x".into()),
            stripe_charges_enabled: true,
        }
    }

    fn err_msg(r: Result<(i64, String), ApiError>) -> String {
        match r {
            Err(ApiError::BadRequest(m)) => m,
            other => panic!("expected BadRequest, got {other:?}"),
        }
    }

    #[test]
    fn sellable_returns_amount_and_account() {
        let (amount, acct) = check_sellable(&sellable_row(), BUYER).unwrap();
        assert_eq!(amount, 50_000);
        assert_eq!(acct, "acct_x");
    }

    #[test]
    fn commission_is_15_percent() {
        // The handler's arithmetic, pinned: £500 → £75.
        assert_eq!(50_000 * COMMISSION_BPS / 10_000, 7_500);
    }

    #[test]
    fn cannot_buy_own_artwork() {
        let r = check_sellable(&sellable_row(), ARTIST_USER);
        assert!(err_msg(r).contains("your own"));
    }

    #[test]
    fn rejects_unpublished_unavailable_and_incomplete() {
        type Mutate = fn(&mut SellableRow);
        let cases: &[(Mutate, &str)] = &[
            (|r| r.status = "draft".into(), "not published"),
            (|r| r.availability = "sold".into(), "not available"),
            (|r| r.has_dimensions = false, "dimensions"),
            (|r| r.weight_grams = None, "weight"),
            (|r| r.ships_from_country = None, "ships-from"),
            (|r| r.stripe_charges_enabled = false, "not set up"),
            (|r| r.price_gbp_cents = None, "no price"),
        ];
        for (mutate, want) in cases {
            let mut row = sellable_row();
            mutate(&mut row);
            let msg = err_msg(check_sellable(&row, BUYER));
            assert!(msg.contains(want), "case {want:?}: got {msg:?}");
        }
    }

    fn addr() -> ShippingAddress {
        ShippingAddress {
            name: "Jane Buyer".into(),
            line1: "1 Test St".into(),
            line2: None,
            city: "London".into(),
            postal_code: "E1 6AN".into(),
            country: "GB".into(),
        }
    }

    #[test]
    fn valid_address_passes() {
        assert!(validate_address(&addr()).is_ok());
    }

    #[test]
    fn blank_required_field_rejected() {
        let mut a = addr();
        a.line1 = "  ".into();
        assert!(validate_address(&a).is_err());
    }

    #[test]
    fn non_iso2_country_rejected() {
        let mut a = addr();
        a.country = "United Kingdom".into();
        assert!(validate_address(&a).is_err());
    }
}
