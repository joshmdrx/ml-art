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
//! completed` (M-03) can map the session back to it.
//!
//! T-090 — the pending order **holds a copy** (see `crate::stock`) until
//! its Checkout Session expires (`CHECKOUT_WINDOW`, set explicitly on the
//! session) plus `HOLD_GRACE`. [`reserve`] takes the artwork's row lock,
//! so two buyers can't both claim the last copy of an original or an
//! edition; the loser gets a 409. The same buyer coming back reuses their
//! hold: same address → the same session (identical Stripe request,
//! idempotency-keyed on order + expiry); new address → the old session is
//! expired first (never left payable alongside the new one) and a fresh
//! one opened on the same order. Shipping is UK-only for v1.
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
use crate::stock;
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

    let address = normalize_address(req.shipping_address)?;
    let shipping_json = serde_json::to_value(&address)
        .map_err(|e| ApiError::Internal(anyhow::anyhow!("serialise shipping: {e}")))?;

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

    let reservation = reserve(
        &state.pool,
        artwork_id,
        user.id,
        row.artist_id,
        &shipping_json,
        amount_cents,
        commission_cents,
    )
    .await?;

    let (order_id, expires_at, amount_cents, commission_cents) = match reservation {
        Reservation::Reuse(h) => (h.order_id, h.expires_at, h.amount_cents, h.commission_cents),
        Reservation::New(h) => (h.order_id, h.expires_at, h.amount_cents, h.commission_cents),
        Reservation::Replace { hold, old_session } => {
            // The buyer came back with a different address (or their
            // session is about to lapse). Their old session must not stay
            // payable alongside the new one, or they could pay twice.
            if let Some(old) = old_session.as_deref() {
                let status = client.checkout_session_status(old).await.map_err(|e| {
                    ApiError::Internal(anyhow::anyhow!("stripe session lookup: {e}"))
                })?;
                match status.as_str() {
                    "complete" => {
                        return Err(ApiError::Conflict(
                            "You've already paid for this work — it's in your orders.".into(),
                        ))
                    }
                    "open" => client.expire_checkout_session(old).await.map_err(|e| {
                        ApiError::Internal(anyhow::anyhow!("stripe session expire: {e}"))
                    })?,
                    _ => {}
                }
            }
            let expires_at = Utc::now() + stock::CHECKOUT_WINDOW;
            let updated = sqlx::query(
                r#"
                UPDATE orders SET
                    shipping_address = $2,
                    hold_expires_at = $3,
                    stripe_checkout_session_id = NULL,
                    updated_at = now()
                WHERE id = $1 AND status = 'pending'
                "#,
            )
            .bind(hold.order_id)
            .bind(&shipping_json)
            .bind(expires_at + stock::HOLD_GRACE)
            .execute(&state.pool)
            .await?
            .rows_affected();
            if updated == 0 {
                return Err(ApiError::Conflict(
                    "This order just completed — check your orders.".into(),
                ));
            }
            (
                hold.order_id,
                expires_at,
                hold.amount_cents,
                hold.commission_cents,
            )
        }
    };

    // Open the hosted Checkout Session for this order. Same order + expiry
    // = same idempotency key, so a resubmit gets the session it already has.
    let order_ref = order_id.to_string();
    let success_url = format!(
        "{}/orders/{}?checkout=success",
        state.cfg.web_base_url, order_id
    );
    let cancel_url = format!(
        "{}/artworks/{}?checkout=cancelled",
        state.cfg.web_base_url, artwork_id
    );
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
                name: &address.name,
                line1: &address.line1,
                line2: address.line2.as_deref(),
                city: &address.city,
                postal_code: &address.postal_code,
                country: &address.country,
            },
            customer_email: Some(&user.email),
            expires_at_unix: expires_at.timestamp(),
        })
        .await;

    let session = match session {
        Ok(s) => s,
        Err(e) => {
            // Don't leave a copy reserved behind a session that never
            // opened — release the hold so other buyers aren't blocked.
            sqlx::query(
                "UPDATE orders SET status = 'cancelled', updated_at = now() \
                 WHERE id = $1 AND status = 'pending' AND stripe_checkout_session_id IS NULL",
            )
            .bind(order_id)
            .execute(&state.pool)
            .await?;
            return Err(ApiError::Internal(anyhow::anyhow!(
                "stripe checkout create: {e}"
            )));
        }
    };

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

/// A buyer's live claim on one copy: their pending order, the moment its
/// Checkout Session expires, and the price locked in when it was made.
#[derive(Debug)]
pub struct Hold {
    pub order_id: Uuid,
    pub expires_at: DateTime<Utc>,
    pub amount_cents: i64,
    pub commission_cents: i64,
}

/// What [`reserve`] decided for this checkout attempt.
#[derive(Debug)]
pub enum Reservation {
    /// No live hold for this buyer: a new pending order now holds a copy.
    New(Hold),
    /// Same buyer, same address, session still comfortably open: reopen
    /// that session (an identical Stripe request replays it).
    Reuse(Hold),
    /// Same buyer with a live hold but a new address (or a session about
    /// to lapse): keep the order and its copy, replace the session.
    Replace {
        hold: Hold,
        old_session: Option<String>,
    },
}

#[derive(FromRow)]
struct HoldRow {
    id: Uuid,
    stripe_checkout_session_id: Option<String>,
    shipping_address: serde_json::Value,
    hold_expires_at: DateTime<Utc>,
    amount_cents_gbp: i64,
    commission_cents_gbp: i64,
}

/// T-090 — claim a copy for this checkout, atomically. Locks the artwork
/// row, then either finds this buyer's live hold or — if a copy is free
/// once other buyers' holds are counted — inserts a pending order holding
/// one until `CHECKOUT_WINDOW + HOLD_GRACE` from now. Sold out → 409; last
/// copy on someone else's hold → 409 saying when it might free up.
///
/// Public so the hold rules are integration-testable without Stripe.
pub async fn reserve(
    pool: &sqlx::PgPool,
    artwork_id: Uuid,
    buyer_id: Uuid,
    artist_id: Uuid,
    shipping_json: &serde_json::Value,
    amount_cents: i64,
    commission_cents: i64,
) -> Result<Reservation, ApiError> {
    let mut tx = pool.begin().await?;
    let stock = stock::lock(&mut tx, artwork_id, Some(buyer_id), None)
        .await?
        .ok_or(ApiError::NotFound)?;

    let mine: Option<HoldRow> = sqlx::query_as(
        r#"
        SELECT id, stripe_checkout_session_id, shipping_address, hold_expires_at,
               amount_cents_gbp, commission_cents_gbp
        FROM orders
        WHERE buyer_user_id = $1 AND artwork_id = $2
          AND status = 'pending' AND hold_expires_at > now()
        ORDER BY created_at DESC
        LIMIT 1
        "#,
    )
    .bind(buyer_id)
    .bind(artwork_id)
    .fetch_optional(&mut *tx)
    .await?;

    let now = Utc::now();
    let reservation = match mine {
        Some(h) => {
            let hold = Hold {
                order_id: h.id,
                expires_at: h.hold_expires_at - stock::HOLD_GRACE,
                amount_cents: h.amount_cents_gbp,
                commission_cents: h.commission_cents_gbp,
            };
            let same_address = &h.shipping_address == shipping_json;
            let comfortably_open = hold.expires_at > now + chrono::Duration::minutes(5);
            if same_address && comfortably_open && h.stripe_checkout_session_id.is_some() {
                Reservation::Reuse(hold)
            } else {
                Reservation::Replace {
                    hold,
                    old_session: h.stripe_checkout_session_id,
                }
            }
        }
        None => {
            if stock.remaining() == 0 {
                return Err(ApiError::Conflict("This work has sold out.".into()));
            }
            if stock.free_now() == 0 {
                return Err(ApiError::Conflict(
                    "Another collector is checking out the last copy. If they don't \
                     complete, it'll be available again within the hour."
                        .into(),
                ));
            }
            let expires_at = now + stock::CHECKOUT_WINDOW;
            let order_id: Uuid = sqlx::query_scalar(
                r#"
                INSERT INTO orders (
                    buyer_user_id, artwork_id, artist_id,
                    amount_cents_gbp, commission_cents_gbp,
                    status, shipping_address, hold_expires_at
                )
                VALUES ($1, $2, $3, $4, $5, 'pending', $6, $7)
                RETURNING id
                "#,
            )
            .bind(buyer_id)
            .bind(artwork_id)
            .bind(artist_id)
            .bind(amount_cents)
            .bind(commission_cents)
            .bind(shipping_json)
            .bind(expires_at + stock::HOLD_GRACE)
            .fetch_one(&mut *tx)
            .await?;
            Reservation::New(Hold {
                order_id,
                expires_at,
                amount_cents,
                commission_cents,
            })
        }
    };
    tx.commit().await?;
    Ok(reservation)
}

// ─────────────────────────────── Order list ───────────────────────────────
//
// GET /v1/me/orders — the buyer's purchase history (M-11). Newest first.

#[derive(Debug, Serialize)]
pub struct BuyerOrderSummary {
    pub id: Uuid,
    pub status: String,
    pub amount_cents_gbp: i64,
    pub created_at: DateTime<Utc>,
    pub artwork: OrderArtwork,
}

#[derive(FromRow)]
struct BuyerOrderRow {
    id: Uuid,
    status: String,
    amount_cents_gbp: i64,
    created_at: DateTime<Utc>,
    artwork_id: Uuid,
    artwork_title: Option<String>,
    image_s3_key: Option<String>,
    artist_name: String,
    artist_slug: String,
}

pub async fn list_my_orders(
    State(state): State<Arc<AppState>>,
    AuthedUser(user): AuthedUser,
) -> Result<Json<Vec<BuyerOrderSummary>>, ApiError> {
    let rows: Vec<BuyerOrderRow> = sqlx::query_as(
        r#"
        SELECT
            o.id, o.status, o.amount_cents_gbp, o.created_at,
            a.id AS artwork_id, a.title AS artwork_title,
            (SELECT s3_key FROM artwork_images
             WHERE artwork_id = a.id AND moderation_status = 'approved'
             ORDER BY is_primary DESC, display_order ASC
             LIMIT 1) AS image_s3_key,
            ar.display_name AS artist_name, ar.slug AS artist_slug
        FROM orders o
        JOIN artworks a  ON a.id  = o.artwork_id
        JOIN artists  ar ON ar.id = o.artist_id
        WHERE o.buyer_user_id = $1
        ORDER BY o.created_at DESC
        "#,
    )
    .bind(user.id)
    .fetch_all(&state.pool)
    .await?;

    let orders = rows
        .into_iter()
        .map(|r| BuyerOrderSummary {
            id: r.id,
            status: r.status,
            amount_cents_gbp: r.amount_cents_gbp,
            created_at: r.created_at,
            artwork: OrderArtwork {
                id: r.artwork_id,
                title: r.artwork_title,
                image_url: r.image_s3_key.as_deref().map(url_for_s3_key),
                artist_name: r.artist_name,
                artist_slug: r.artist_slug,
            },
        })
        .collect();
    Ok(Json(orders))
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
    // T-090 — UK-only v1. Prices include shipping and there's no customs
    // or international-rate handling, so artists would be shipping abroad
    // at their own cost. (GB covers England, Scotland, Wales + Northern
    // Ireland; the Channel Islands and Isle of Man have their own codes.)
    if !a.country.trim().eq_ignore_ascii_case("GB") {
        return Err(ApiError::BadRequest(
            "We can only ship within the UK for now.".into(),
        ));
    }
    Ok(())
}

/// Validate, then trim + canonicalise so the stored address (and the
/// "is this the same address as your open checkout?" comparison) doesn't
/// depend on stray whitespace or case.
fn normalize_address(a: ShippingAddress) -> Result<ShippingAddress, ApiError> {
    validate_address(&a)?;
    Ok(ShippingAddress {
        name: a.name.trim().to_string(),
        line1: a.line1.trim().to_string(),
        line2: a
            .line2
            .map(|l| l.trim().to_string())
            .filter(|l| !l.is_empty()),
        city: a.city.trim().to_string(),
        postal_code: a.postal_code.trim().to_uppercase(),
        country: "GB".to_string(),
    })
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
    fn non_uk_country_rejected() {
        let mut a = addr();
        a.country = "FR".into();
        assert!(validate_address(&a).is_err());
    }

    #[test]
    fn address_is_normalised() {
        let mut a = addr();
        a.country = "gb".into();
        a.postal_code = " ec1a 1bb ".into();
        a.line2 = Some("  ".into());
        let n = normalize_address(a).unwrap();
        assert_eq!(n.country, "GB");
        assert_eq!(n.postal_code, "EC1A 1BB");
        assert_eq!(n.line2, None);
    }

    #[test]
    fn non_iso2_country_rejected() {
        let mut a = addr();
        a.country = "United Kingdom".into();
        assert!(validate_address(&a).is_err());
    }
}
