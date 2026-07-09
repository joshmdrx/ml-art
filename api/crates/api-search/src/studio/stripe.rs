//! M-01 — Stripe Connect Express onboarding for artists.
//!
//! `POST /v1/studio/stripe/onboarding-link` returns a single-use hosted-
//! onboarding URL. On first call it creates the artist's Connect Express
//! account and persists `artists.stripe_account_id`; subsequent calls
//! reuse that account. The `account.updated` webhook (M-03) later flips
//! `stripe_charges_enabled` / `stripe_payouts_enabled` once Stripe
//! finishes KYC — this endpoint only kicks the flow off.
//!
//! Gated like the rest of `/v1/studio/*`: a non-artist caller gets 404
//! (never 403), so we don't leak who is or isn't an artist. When
//! `STRIPE_SECRET_KEY` is unset the endpoint answers 503 at the entry.

use axum::{extract::State, Json};
use ml_art_core::{error::ApiError, stripe::StripeClient};
use serde::Serialize;
use std::sync::Arc;
use uuid::Uuid;

use crate::extractors::AuthedUser;
use crate::AppState;

#[derive(Debug, Serialize)]
pub struct OnboardingLink {
    /// Single-use Stripe hosted-onboarding URL. The client redirects here.
    url: String,
}

pub async fn onboarding_link(
    State(state): State<Arc<AppState>>,
    AuthedUser(user): AuthedUser,
) -> Result<Json<OnboardingLink>, ApiError> {
    // Gate: no Stripe key configured ⇒ marketplace disabled (dev shortcut).
    let client = StripeClient::from_config(state.cfg.stripe_secret_key.as_deref())
        .ok_or_else(|| ApiError::ServiceUnavailable("stripe not configured".into()))?;

    // Resolve the caller's artist row (404 for non-artists) plus the two
    // fields we need: the country to seed the Express account, and any
    // existing account id to reuse.
    let artist: Option<(Uuid, Option<String>, Option<String>)> = sqlx::query_as(
        r#"
        SELECT id, country, stripe_account_id
        FROM artists
        WHERE user_id = $1 AND deleted_at IS NULL
        "#,
    )
    .bind(user.id)
    .fetch_optional(&state.pool)
    .await?;
    let (artist_id, country, existing_account) = artist.ok_or(ApiError::NotFound)?;

    // Reuse the connected account if we've already made one; otherwise
    // create + persist. The Stripe-side idempotency key (keyed on the
    // artist) is a second guard against a race creating two accounts.
    let account_id = match existing_account {
        Some(id) => id,
        None => {
            // Country seeds the Express account; artists onboarded before
            // T-085 may lack one, so default to the UK launch market.
            let country = country.as_deref().unwrap_or("GB");
            let account = client
                .create_express_account(artist_id, country, &user.email)
                .await
                .map_err(|e| ApiError::Internal(anyhow::anyhow!("stripe account create: {e}")))?;
            sqlx::query(
                "UPDATE artists SET stripe_account_id = $1, updated_at = now() WHERE id = $2",
            )
            .bind(&account.id)
            .bind(artist_id)
            .execute(&state.pool)
            .await?;
            account.id
        }
    };

    // Hosted-onboarding link. Both return + refresh point at the payouts
    // settings page: on return we show KYC status; on an expired link the
    // page just re-requests a fresh one on load.
    let payouts_url = format!("{}/studio/settings/payouts", state.cfg.web_base_url);
    let link = client
        .create_account_link(&account_id, &payouts_url, &payouts_url)
        .await
        .map_err(|e| ApiError::Internal(anyhow::anyhow!("stripe account link: {e}")))?;

    Ok(Json(OnboardingLink { url: link.url }))
}
