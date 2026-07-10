//! Test-fixture insert endpoints, guarded by `WANDER_TEST_FIXTURES_ENABLED`.
//!
//! Only registered when the env var is set at boot (see the conditional
//! `.merge(...)` in `build_app`); in prod the routes don't exist. Skip
//! auth, image upload, moderation, embedding — pure direct DB inserts so
//! E2E specs can set up world state without driving through UI flows
//! that pull in Jina / Clerk / S3.
//!
//! See `docs/e2e-coverage.md` → "Test fixtures" for the specs that use
//! these + the block reasons the seam was built to unblock (unread
//! inquiry badge, publish nudge, URL-driven artwork modal lifecycle).
//!
//! **Belt-and-suspenders:** every handler here also runs the
//! `is_enabled()` gate at request time, so a misconfigured router that
//! registers these routes without the env var still 404s at the edge.

use std::sync::Arc;

use axum::{extract::State, routing::post, Json, Router};
use ml_art_core::error::ApiError;
use serde::{Deserialize, Serialize};
use uuid::Uuid;

use crate::AppState;

const ENV_VAR: &str = "WANDER_TEST_FIXTURES_ENABLED";

/// True iff `WANDER_TEST_FIXTURES_ENABLED` is set to any non-empty
/// value. Unset (prod) → false → the routes never register and every
/// handler defensively 404s.
pub fn is_enabled() -> bool {
    std::env::var(ENV_VAR).is_ok_and(|v| !v.is_empty())
}

/// Build the test-fixtures sub-router. Call sites should only merge
/// this into the main router when `is_enabled()`.
pub fn routes() -> Router<Arc<AppState>> {
    Router::new()
        .route("/v1/testfixtures/artwork", post(create_artwork))
        .route("/v1/testfixtures/inquiry", post(create_inquiry))
        // M-10 — marketplace fixtures.
        .route("/v1/testfixtures/enable-payouts", post(enable_payouts))
        .route("/v1/testfixtures/make-sellable", post(make_sellable))
        .route("/v1/testfixtures/order", post(create_order))
}

// ─────────────────────────────────────────────────────────────────────────────
// POST /v1/testfixtures/artwork
// ─────────────────────────────────────────────────────────────────────────────

#[derive(Debug, Deserialize)]
pub struct CreateArtworkBody {
    /// Owning artist's slug. Must resolve to an `artists` row.
    pub artist_slug: String,
    #[serde(default = "default_title")]
    pub title: String,
    #[serde(default)]
    pub medium: Option<String>,
    #[serde(default)]
    pub price_cents: Option<i64>,
    #[serde(default = "default_currency")]
    pub currency: String,
    /// Free-form JSON blob matching `artworks.dimensions`. Omit to
    /// leave NULL — that path triggers the T-070 publish nudge.
    #[serde(default)]
    pub dimensions: Option<serde_json::Value>,
    /// "draft" or "published". Anything else is a bad request. Publish
    /// also stamps `published_at = now()` so ordering by published-at
    /// works predictably in specs.
    #[serde(default = "default_status")]
    pub status: String,
    /// When true (default), also insert an approved `artwork_images`
    /// row so the artwork is visible on public surfaces + can receive
    /// inquiries.
    #[serde(default = "default_with_image")]
    pub with_image: bool,
}

fn default_title() -> String {
    "Test artwork".to_string()
}
fn default_currency() -> String {
    "USD".to_string()
}
fn default_status() -> String {
    "published".to_string()
}
fn default_with_image() -> bool {
    true
}

#[derive(Debug, Serialize)]
pub struct CreateArtworkResp {
    pub id: Uuid,
    pub image_id: Option<Uuid>,
}

pub async fn create_artwork(
    State(state): State<Arc<AppState>>,
    Json(body): Json<CreateArtworkBody>,
) -> Result<Json<CreateArtworkResp>, ApiError> {
    if !is_enabled() {
        return Err(ApiError::NotFound);
    }
    if body.status != "draft" && body.status != "published" {
        return Err(ApiError::BadRequest(
            "status must be 'draft' or 'published'".to_string(),
        ));
    }

    let (artist_id,): (Uuid,) = sqlx::query_as("SELECT id FROM artists WHERE slug = $1")
        .bind(&body.artist_slug)
        .fetch_optional(&state.pool)
        .await?
        .ok_or(ApiError::NotFound)?;

    let (id,): (Uuid,) = sqlx::query_as(
        r#"
        INSERT INTO artworks (
            artist_id, title, medium, price_cents, currency,
            dimensions, status, published_at, availability
        )
        VALUES (
            $1, $2, $3, $4, $5, $6, $7,
            CASE WHEN $7 = 'published' THEN now() ELSE NULL END,
            'available'
        )
        RETURNING id
        "#,
    )
    .bind(artist_id)
    .bind(&body.title)
    .bind(&body.medium)
    .bind(body.price_cents)
    .bind(&body.currency)
    .bind(&body.dimensions)
    .bind(&body.status)
    .fetch_one(&state.pool)
    .await?;

    let image_id = if body.with_image {
        // Use a synthetic s3_key — the file doesn't need to exist; the
        // public artwork page renders it via the CDN URL, and if the
        // image 404s the page still renders (broken img tag).
        let s3_key = format!("test/fixtures/{id}.jpg");
        let (img_id,): (Uuid,) = sqlx::query_as(
            r#"
            INSERT INTO artwork_images
                (artwork_id, s3_key, width, height, is_primary,
                 display_order, moderation_status)
            VALUES ($1, $2, 1200, 900, true, 0, 'approved')
            RETURNING id
            "#,
        )
        .bind(id)
        .bind(&s3_key)
        .fetch_one(&state.pool)
        .await?;
        Some(img_id)
    } else {
        None
    };

    Ok(Json(CreateArtworkResp { id, image_id }))
}

// ─────────────────────────────────────────────────────────────────────────────
// POST /v1/testfixtures/inquiry
// ─────────────────────────────────────────────────────────────────────────────

#[derive(Debug, Deserialize)]
pub struct CreateInquiryBody {
    pub artwork_id: Uuid,
    #[serde(default = "default_from_name")]
    pub from_name: String,
    #[serde(default = "default_from_email")]
    pub from_email: String,
    #[serde(default = "default_message")]
    pub message: String,
    /// "delivered" (verified_at + delivered_at = now()) or "pending"
    /// (both NULL — awaiting anon verification). Anything else = 400.
    /// The unread-count query filters on `delivered_at IS NOT NULL` so
    /// only "delivered" rows bump the studio badge.
    #[serde(default = "default_state")]
    pub state: String,
}

fn default_from_name() -> String {
    "Test Buyer".to_string()
}
fn default_from_email() -> String {
    "e2e-buyer@example.com".to_string()
}
fn default_message() -> String {
    "E2E test inquiry.".to_string()
}
fn default_state() -> String {
    "delivered".to_string()
}

#[derive(Debug, Serialize)]
pub struct CreateInquiryResp {
    pub id: Uuid,
}

pub async fn create_inquiry(
    State(state): State<Arc<AppState>>,
    Json(body): Json<CreateInquiryBody>,
) -> Result<Json<CreateInquiryResp>, ApiError> {
    if !is_enabled() {
        return Err(ApiError::NotFound);
    }
    if body.state != "delivered" && body.state != "pending" {
        return Err(ApiError::BadRequest(
            "state must be 'delivered' or 'pending'".to_string(),
        ));
    }

    let (artist_id,): (Uuid,) = sqlx::query_as("SELECT artist_id FROM artworks WHERE id = $1")
        .bind(body.artwork_id)
        .fetch_optional(&state.pool)
        .await?
        .ok_or(ApiError::NotFound)?;

    let (id,): (Uuid,) = if body.state == "delivered" {
        sqlx::query_as(
            r#"
            INSERT INTO inquiries (
                artwork_id, artist_id, from_email, from_name, message,
                delivery_channel, verified_at, delivered_at
            )
            VALUES ($1, $2, $3, $4, $5, 'platform', now(), now())
            RETURNING id
            "#,
        )
        .bind(body.artwork_id)
        .bind(artist_id)
        .bind(&body.from_email)
        .bind(&body.from_name)
        .bind(&body.message)
        .fetch_one(&state.pool)
        .await?
    } else {
        sqlx::query_as(
            r#"
            INSERT INTO inquiries (
                artwork_id, artist_id, from_email, from_name, message,
                delivery_channel
            )
            VALUES ($1, $2, $3, $4, $5, 'platform')
            RETURNING id
            "#,
        )
        .bind(body.artwork_id)
        .bind(artist_id)
        .bind(&body.from_email)
        .bind(&body.from_name)
        .bind(&body.message)
        .fetch_one(&state.pool)
        .await?
    };

    Ok(Json(CreateInquiryResp { id }))
}

// ─────────────────────────────────────────────────────────────────────────────
// M-10 — marketplace fixtures
// ─────────────────────────────────────────────────────────────────────────────

#[derive(Debug, Deserialize)]
pub struct EnablePayoutsBody {
    /// Artist to flip "as if" Stripe onboarding completed.
    pub artist_slug: String,
}

/// Flip an artist to charges/payouts-enabled without the real Stripe
/// Connect flow — the E2E equivalent of `account.updated` arriving.
pub async fn enable_payouts(
    State(state): State<Arc<AppState>>,
    Json(body): Json<EnablePayoutsBody>,
) -> Result<Json<serde_json::Value>, ApiError> {
    if !is_enabled() {
        return Err(ApiError::NotFound);
    }
    let rows = sqlx::query(
        r#"
        UPDATE artists SET
            -- `stripe_account_id` is UNIQUE, and each E2E run mints a new
            -- artist, so derive a per-artist id rather than a constant.
            stripe_account_id = COALESCE(stripe_account_id, 'acct_test_' || substr(md5(slug), 1, 16)),
            stripe_charges_enabled = true,
            stripe_payouts_enabled = true,
            stripe_onboarded_at = COALESCE(stripe_onboarded_at, now()),
            updated_at = now()
        WHERE slug = $1 AND deleted_at IS NULL
        "#,
    )
    .bind(&body.artist_slug)
    .execute(&state.pool)
    .await?
    .rows_affected();
    if rows == 0 {
        return Err(ApiError::NotFound);
    }
    Ok(Json(serde_json::json!({ "status": "ok" })))
}

#[derive(Debug, Deserialize)]
pub struct MakeSellableBody {
    pub artwork_id: Uuid,
}

/// Fill in the fields an artwork needs to be *purchasable* (weight,
/// ships-from, GBP price, dimensions). Pair with `enable-payouts` on the
/// owning artist to make the Buy button show.
pub async fn make_sellable(
    State(state): State<Arc<AppState>>,
    Json(body): Json<MakeSellableBody>,
) -> Result<Json<serde_json::Value>, ApiError> {
    if !is_enabled() {
        return Err(ApiError::NotFound);
    }
    let rows = sqlx::query(
        r#"
        UPDATE artworks SET
            weight_grams = COALESCE(weight_grams, 1500),
            ships_from_country = COALESCE(ships_from_country, 'GB'),
            price_gbp_cents = COALESCE(price_gbp_cents, price_cents, 50000),
            dimensions = COALESCE(dimensions, '{"width_cm":30,"height_cm":40}'::jsonb),
            availability = 'available',
            status = 'published',
            updated_at = now()
        WHERE id = $1 AND deleted_at IS NULL
        "#,
    )
    .bind(body.artwork_id)
    .execute(&state.pool)
    .await?
    .rows_affected();
    if rows == 0 {
        return Err(ApiError::NotFound);
    }
    Ok(Json(serde_json::json!({ "status": "ok" })))
}

#[derive(Debug, Deserialize)]
pub struct CreateOrderBody {
    /// Buyer, resolved by email → `users.id`. Omit to pick any user
    /// (when the buyer's identity doesn't matter, e.g. mark-shipped).
    #[serde(default)]
    pub buyer_email: Option<String>,
    /// Artwork to attach. Omit to pick any published artwork — lets
    /// buyer-side specs seed an order without knowing a specific id
    /// (the dev DB isn't the test seed).
    #[serde(default)]
    pub artwork_id: Option<Uuid>,
    /// Any order status. Defaults to `paid` (the common fulfilment case).
    #[serde(default = "default_order_status")]
    pub status: String,
    #[serde(default = "default_amount_cents")]
    pub amount_cents_gbp: i64,
}

fn default_order_status() -> String {
    "paid".to_string()
}
fn default_amount_cents() -> i64 {
    50_000
}

#[derive(Debug, Serialize)]
pub struct CreateOrderResp {
    pub id: Uuid,
}

/// Insert an order in any state — the seam the buyer-orders, mark-shipped,
/// and admin-refund specs build on without needing the Stripe loop.
pub async fn create_order(
    State(state): State<Arc<AppState>>,
    Json(body): Json<CreateOrderBody>,
) -> Result<Json<CreateOrderResp>, ApiError> {
    if !is_enabled() {
        return Err(ApiError::NotFound);
    }
    let buyer_id: Uuid = match &body.buyer_email {
        Some(email) => sqlx::query_scalar("SELECT id FROM users WHERE email = $1")
            .bind(email)
            .fetch_optional(&state.pool)
            .await?
            .ok_or_else(|| ApiError::BadRequest(format!("no user with email {email}")))?,
        None => sqlx::query_scalar("SELECT id FROM users ORDER BY created_at DESC LIMIT 1")
            .fetch_optional(&state.pool)
            .await?
            .ok_or_else(|| ApiError::BadRequest("no users to attach as buyer".into()))?,
    };
    // Resolve the artwork (explicit id, or any published one) + its artist.
    let (artwork_id, artist_id): (Uuid, Uuid) = match body.artwork_id {
        Some(id) => sqlx::query_as("SELECT id, artist_id FROM artworks WHERE id = $1")
            .bind(id)
            .fetch_optional(&state.pool)
            .await?
            .ok_or(ApiError::NotFound)?,
        None => sqlx::query_as(
            "SELECT id, artist_id FROM artworks
             WHERE deleted_at IS NULL AND status = 'published'
             ORDER BY created_at DESC LIMIT 1",
        )
        .fetch_optional(&state.pool)
        .await?
        .ok_or_else(|| ApiError::BadRequest("no published artwork to attach".into()))?,
    };

    let commission = body.amount_cents_gbp * 15 / 100;
    let id: Uuid = sqlx::query_scalar(
        r#"
        INSERT INTO orders (
            buyer_user_id, artwork_id, artist_id,
            amount_cents_gbp, commission_cents_gbp, status,
            shipping_address, stripe_payment_intent_id
        )
        VALUES ($1, $2, $3, $4, $5, $6,
                '{"name":"Test Buyer","line1":"1 Test St","city":"London",
                  "postal_code":"E1 6AN","country":"GB"}'::jsonb,
                'pi_test_' || substr(gen_random_uuid()::text, 1, 12))
        RETURNING id
        "#,
    )
    .bind(buyer_id)
    .bind(artwork_id)
    .bind(artist_id)
    .bind(body.amount_cents_gbp)
    .bind(commission)
    .bind(&body.status)
    .fetch_one(&state.pool)
    .await?;

    Ok(Json(CreateOrderResp { id }))
}
