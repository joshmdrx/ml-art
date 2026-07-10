//! Stripe integration primitives for the direct-sales marketplace.
//!
//! Two concerns live here, both transport-level and free of any
//! request/DB context so every marketplace surface can share them:
//!
//!   1. [`verify_webhook_signature`] — validates the `Stripe-Signature`
//!      header on `POST /v1/webhooks/stripe` (M-03). Stripe signs the
//!      *raw* request body with HMAC-SHA256 keyed on the endpoint's
//!      `whsec_` secret, so the webhook handler must verify before it
//!      parses. Same HMAC-SHA256 primitive as [`crate::reply_address`].
//!   2. [`StripeClient`] — a thin authenticated wrapper over `reqwest`
//!      for Stripe's form-encoded REST API. Typed high-level calls
//!      (Connect account creation, checkout sessions, refunds) land with
//!      the tickets that need them (M-01, M-04, M-08); this is the shared
//!      transport they build on. Mirrors [`crate::geocoding`]'s client.
//!
//! No key ⇒ no client: construct via [`StripeClient::from_config`], which
//! returns `None` when `STRIPE_SECRET_KEY` is unset so dev instances
//! without Stripe credentials keep building (the marketplace endpoints
//! 503 at the entry instead).

use hmac::{Hmac, Mac};
use sha2::Sha256;
use std::time::Duration;

type HmacSha256 = Hmac<Sha256>;

/// Stripe's default webhook timestamp tolerance — reject events whose
/// signed timestamp is more than this far from now, in either direction.
/// Blunts replay of a captured payload. Matches Stripe's own libraries.
const DEFAULT_TOLERANCE_SECS: i64 = 300;

/// Outbound HTTP timeout for Stripe REST calls. Stripe is normally
/// single-digit-ms; a checkout-session create in the request path must
/// not hang the buyer, so keep this tight.
const HTTP_TIMEOUT: Duration = Duration::from_secs(10);

const STRIPE_API_BASE: &str = "https://api.stripe.com";

// ─────────────────────────── Webhook signatures ───────────────────────────

/// Why a `Stripe-Signature` header failed verification. The webhook
/// handler maps every variant to a 400/401 — a signature we can't verify
/// is never processed, so Stripe re-delivers (and an attacker forging one
/// gets nothing).
#[derive(Debug, thiserror::Error, PartialEq, Eq)]
pub enum SignatureError {
    #[error("no signing secret configured")]
    NoSecret,
    #[error("signature header missing or malformed")]
    MalformedHeader,
    #[error("timestamp outside tolerance")]
    TimestampOutOfTolerance,
    #[error("no signature matched")]
    NoMatch,
}

/// Verify the `Stripe-Signature` header against the raw request body.
///
/// `header` is the raw value, e.g. `t=1680000000,v1=abc…,v1=def…`
/// (Stripe may include several `v1` entries during a secret rotation;
/// any one matching is a pass). `payload` is the **exact** bytes of the
/// request body — re-serialising parsed JSON would change the bytes and
/// break the HMAC, so the handler must verify before it deserialises.
/// `now_unix` is injected (not read from the clock) so tests are
/// deterministic; the handler passes `chrono::Utc::now().timestamp()`.
///
/// Returns `Ok(())` only when the timestamp is within tolerance **and**
/// some `v1` equals `HMAC-SHA256(secret, "{t}.{payload}")`.
pub fn verify_webhook_signature(
    payload: &[u8],
    header: &str,
    secret: &str,
    now_unix: i64,
) -> Result<(), SignatureError> {
    if secret.is_empty() {
        return Err(SignatureError::NoSecret);
    }

    // Parse `k=v` pairs: capture `t` and every `v1` candidate.
    let mut timestamp: Option<i64> = None;
    let mut v1_sigs: Vec<&str> = Vec::new();
    for part in header.split(',') {
        let (k, v) = part
            .split_once('=')
            .ok_or(SignatureError::MalformedHeader)?;
        match k.trim() {
            "t" => timestamp = v.trim().parse().ok(),
            "v1" => v1_sigs.push(v.trim()),
            _ => {} // v0 (test), scheme markers, future fields — ignore.
        }
    }
    let timestamp = timestamp.ok_or(SignatureError::MalformedHeader)?;
    if v1_sigs.is_empty() {
        return Err(SignatureError::MalformedHeader);
    }

    // Replay guard: reject stale (or absurdly future) timestamps.
    if (now_unix - timestamp).abs() > DEFAULT_TOLERANCE_SECS {
        return Err(SignatureError::TimestampOutOfTolerance);
    }

    // signed_payload = "{timestamp}.{raw body}"
    let mut mac =
        HmacSha256::new_from_slice(secret.as_bytes()).expect("HMAC accepts any key length");
    mac.update(timestamp.to_string().as_bytes());
    mac.update(b".");
    mac.update(payload);
    let expected = mac.finalize().into_bytes();

    // Constant-time compare against each candidate. `verify_slice`
    // consumes the Mac, so recompute per candidate rather than leak an
    // early-exit byte compare.
    for sig_hex in v1_sigs {
        let Ok(sig) = hex::decode(sig_hex) else {
            continue;
        };
        if sig.len() == expected.len() && constant_time_eq(&sig, &expected) {
            return Ok(());
        }
    }
    Err(SignatureError::NoMatch)
}

/// Constant-time byte equality over equal-length slices. Content is
/// compared without early exit; callers gate on length first. Same
/// idiom as the T-054 inbound-webhook secret check.
fn constant_time_eq(a: &[u8], b: &[u8]) -> bool {
    debug_assert_eq!(a.len(), b.len());
    let mut diff = 0u8;
    for (x, y) in a.iter().zip(b.iter()) {
        diff |= x ^ y;
    }
    diff == 0
}

// ─────────────────────────────── REST client ──────────────────────────────

/// Failure modes of a Stripe REST call.
#[derive(Debug, thiserror::Error)]
pub enum StripeError {
    #[error("stripe http transport error: {0}")]
    Transport(#[from] reqwest::Error),
    /// Stripe returned a non-2xx. `status` is the HTTP code; `body` is
    /// the raw response (Stripe puts a structured `error.message` in it)
    /// for logging — never surfaced verbatim to buyers.
    #[error("stripe api error {status}: {body}")]
    Api { status: u16, body: String },
}

/// Authenticated transport for Stripe's form-encoded REST API. Cheap to
/// clone (the inner `reqwest::Client` is an `Arc`), so hand it around by
/// value like [`crate::geocoding::GeocodingClient`].
#[derive(Clone)]
pub struct StripeClient {
    http: reqwest::Client,
    secret_key: String,
}

impl StripeClient {
    /// Build a client from the platform secret key. Returns `None` when
    /// the key is absent so the marketplace stays gracefully disabled in
    /// dev (`Config::stripe_secret_key` is `Option`).
    pub fn from_config(secret_key: Option<&str>) -> Option<Self> {
        let key = secret_key?.to_owned();
        let http = reqwest::Client::builder()
            .timeout(HTTP_TIMEOUT)
            .build()
            .expect("reqwest client");
        Some(Self {
            http,
            secret_key: key,
        })
    }

    /// POST form-encoded `params` to a Stripe REST path (e.g.
    /// `/v1/checkout/sessions`) and deserialise the JSON response.
    ///
    /// Stripe's API is `application/x-www-form-urlencoded`, with nested
    /// fields expressed as flat `foo[bar]` keys — callers pass those
    /// already flattened. `idempotency_key` maps to the
    /// `Idempotency-Key` header so a retried create doesn't double-charge
    /// or double-refund; pass a stable per-operation key.
    pub async fn post_form<T: serde::de::DeserializeOwned>(
        &self,
        path: &str,
        params: &[(&str, String)],
        idempotency_key: Option<&str>,
    ) -> Result<T, StripeError> {
        let mut req = self
            .http
            .post(format!("{STRIPE_API_BASE}{path}"))
            .bearer_auth(&self.secret_key)
            .form(params);
        if let Some(key) = idempotency_key {
            req = req.header("Idempotency-Key", key);
        }
        Self::read(req.send().await?).await
    }

    /// GET a Stripe REST path (e.g. a balance transaction to learn the
    /// exact fee after a charge succeeds) and deserialise the response.
    pub async fn get<T: serde::de::DeserializeOwned>(&self, path: &str) -> Result<T, StripeError> {
        let req = self
            .http
            .get(format!("{STRIPE_API_BASE}{path}"))
            .bearer_auth(&self.secret_key);
        Self::read(req.send().await?).await
    }

    /// M-01 — create a Connect **Express** account for an artist. Stripe
    /// collects the rest (bank details, ID) in its hosted onboarding; we
    /// only seed the country + email. Returns the `acct_*` id to persist
    /// on `artists.stripe_account_id`.
    ///
    /// `country` is ISO-3166 alpha-2 (`GB` for the UK launch). Idempotency
    /// is keyed on the artist so a double-submit of "enable direct sales"
    /// doesn't create two connected accounts.
    pub async fn create_express_account(
        &self,
        artist_id: uuid::Uuid,
        country: &str,
        email: &str,
    ) -> Result<StripeAccount, StripeError> {
        let params = [
            ("type", "express".to_string()),
            ("country", country.to_string()),
            ("email", email.to_string()),
            ("capabilities[card_payments][requested]", "true".to_string()),
            ("capabilities[transfers][requested]", "true".to_string()),
            // Trace back to our artist from the Stripe dashboard.
            ("metadata[artist_id]", artist_id.to_string()),
        ];
        self.post_form(
            "/v1/accounts",
            &params,
            Some(&format!("connect_account:{artist_id}")),
        )
        .await
    }

    /// M-01 — mint a single-use hosted-onboarding URL for a connected
    /// account. `refresh_url` is where Stripe bounces the artist if the
    /// link expired mid-flow (re-request a fresh one); `return_url` is
    /// where they land on completion. Both are our `/studio/...` pages.
    /// Account links are short-lived and single-use by design — not
    /// idempotent, so no key.
    pub async fn create_account_link(
        &self,
        account_id: &str,
        refresh_url: &str,
        return_url: &str,
    ) -> Result<AccountLink, StripeError> {
        let params = [
            ("account", account_id.to_string()),
            ("refresh_url", refresh_url.to_string()),
            ("return_url", return_url.to_string()),
            ("type", "account_onboarding".to_string()),
        ];
        self.post_form("/v1/account_links", &params, None).await
    }

    /// M-04 — create a hosted **Checkout Session** for a marketplace
    /// order. This is a Connect *destination charge*: the buyer's card is
    /// charged on Wander's platform account, Stripe takes our
    /// `application_fee_cents` as commission, and routes the balance to
    /// the artist's connected account (`destination_account`). Returns the
    /// session (its `url` is where the buyer is redirected).
    ///
    /// Idempotency is keyed on the order so a double-submit reuses the
    /// same session rather than opening two.
    pub async fn create_checkout_session(
        &self,
        req: &CheckoutSessionRequest<'_>,
    ) -> Result<CheckoutSession, StripeError> {
        let mut params: Vec<(&str, String)> = vec![
            ("mode", "payment".to_string()),
            ("success_url", req.success_url.to_string()),
            ("cancel_url", req.cancel_url.to_string()),
            ("client_reference_id", req.client_reference_id.to_string()),
            ("line_items[0][quantity]", "1".to_string()),
            (
                "line_items[0][price_data][currency]",
                req.currency.to_string(),
            ),
            (
                "line_items[0][price_data][unit_amount]",
                req.amount_cents.to_string(),
            ),
            (
                "line_items[0][price_data][product_data][name]",
                req.product_name.to_string(),
            ),
            (
                "payment_intent_data[application_fee_amount]",
                req.application_fee_cents.to_string(),
            ),
            (
                "payment_intent_data[transfer_data][destination]",
                req.destination_account.to_string(),
            ),
            ("metadata[order_id]", req.client_reference_id.to_string()),
        ];
        // Shipping → the PaymentIntent, so Stripe's fraud checks + the
        // artist's dashboard both see the destination address.
        let s = &req.shipping;
        params.push(("payment_intent_data[shipping][name]", s.name.to_string()));
        params.push((
            "payment_intent_data[shipping][address][line1]",
            s.line1.to_string(),
        ));
        if let Some(line2) = s.line2 {
            params.push((
                "payment_intent_data[shipping][address][line2]",
                line2.to_string(),
            ));
        }
        params.push((
            "payment_intent_data[shipping][address][city]",
            s.city.to_string(),
        ));
        params.push((
            "payment_intent_data[shipping][address][postal_code]",
            s.postal_code.to_string(),
        ));
        params.push((
            "payment_intent_data[shipping][address][country]",
            s.country.to_string(),
        ));

        self.post_form(
            "/v1/checkout/sessions",
            &params,
            Some(&format!("checkout:{}", req.client_reference_id)),
        )
        .await
    }

    /// M-08 — refund a marketplace order's charge. `reverse_transfer`
    /// pulls the artist's routed balance back, `refund_application_fee`
    /// returns Wander's commission too — i.e. a full unwind of the
    /// destination charge. Idempotency is keyed on the order so an admin
    /// double-clicking Refund doesn't fire two refunds.
    pub async fn create_refund(
        &self,
        payment_intent: &str,
        order_id: uuid::Uuid,
    ) -> Result<Refund, StripeError> {
        let params = [
            ("payment_intent", payment_intent.to_string()),
            ("reverse_transfer", "true".to_string()),
            ("refund_application_fee", "true".to_string()),
        ];
        self.post_form("/v1/refunds", &params, Some(&format!("refund:{order_id}")))
            .await
    }

    /// Map a response to `Ok(parsed)` on 2xx, `Err(Api{..})` otherwise.
    async fn read<T: serde::de::DeserializeOwned>(
        resp: reqwest::Response,
    ) -> Result<T, StripeError> {
        let status = resp.status();
        if status.is_success() {
            Ok(resp.json().await?)
        } else {
            Err(StripeError::Api {
                status: status.as_u16(),
                body: resp.text().await.unwrap_or_default(),
            })
        }
    }
}

// ───────────────────────────── Response types ─────────────────────────────
//
// Deserialise only the fields we act on. Stripe objects are large and
// versioned; `#[serde(default)]` / ignoring unknown fields keeps us
// forward-compatible when Stripe adds keys.

/// A Connect account (`acct_*`). Returned by [`StripeClient::create_express_account`].
#[derive(Debug, serde::Deserialize)]
pub struct StripeAccount {
    pub id: String,
}

/// A hosted-onboarding link. Returned by [`StripeClient::create_account_link`].
#[derive(Debug, serde::Deserialize)]
pub struct AccountLink {
    /// The single-use URL to redirect the artist to.
    pub url: String,
}

/// Destination address for a Checkout Session, forwarded to the
/// PaymentIntent's `shipping`. Borrowed — the caller owns the strings.
pub struct StripeShipping<'a> {
    pub name: &'a str,
    pub line1: &'a str,
    pub line2: Option<&'a str>,
    pub city: &'a str,
    pub postal_code: &'a str,
    /// ISO-3166 alpha-2 (`GB`).
    pub country: &'a str,
}

/// Inputs to [`StripeClient::create_checkout_session`]. One line item
/// (the artwork); amounts are in the currency's minor units (GBP pence).
pub struct CheckoutSessionRequest<'a> {
    /// Our order id — set as `client_reference_id` + `metadata.order_id`
    /// so the `checkout.session.completed` webhook maps back to the order.
    pub client_reference_id: &'a str,
    pub product_name: &'a str,
    pub amount_cents: i64,
    /// ISO-4217 lowercase (`gbp`).
    pub currency: &'a str,
    /// Wander's commission, taken as the Connect application fee.
    pub application_fee_cents: i64,
    /// The artist's connected account (`acct_*`) — the transfer destination.
    pub destination_account: &'a str,
    pub success_url: &'a str,
    pub cancel_url: &'a str,
    pub shipping: StripeShipping<'a>,
}

/// A refund (`re_*`). Returned by [`StripeClient::create_refund`].
#[derive(Debug, serde::Deserialize)]
pub struct Refund {
    pub id: String,
    /// `pending` | `succeeded` | `failed` | `canceled` — the terminal
    /// state arrives via the `charge.refunded` webhook.
    pub status: Option<String>,
}

/// A Checkout Session. Returned by [`StripeClient::create_checkout_session`].
#[derive(Debug, serde::Deserialize)]
pub struct CheckoutSession {
    /// `cs_*` — persisted on the order; the webhook matches on it.
    pub id: String,
    /// The hosted-checkout URL to redirect the buyer to. Present for
    /// `mode=payment` hosted sessions.
    pub url: Option<String>,
}

#[cfg(test)]
mod tests {
    use super::*;

    const SECRET: &str = "whsec_test_do_not_use_in_prod";

    /// Produce a valid header for `payload` at `ts` — mirrors what Stripe
    /// signs so the round-trip test doesn't hard-code an opaque hex.
    fn sign(payload: &[u8], ts: i64, secret: &str) -> String {
        let mut mac = HmacSha256::new_from_slice(secret.as_bytes()).unwrap();
        mac.update(ts.to_string().as_bytes());
        mac.update(b".");
        mac.update(payload);
        format!("t={ts},v1={}", hex::encode(mac.finalize().into_bytes()))
    }

    #[test]
    fn valid_signature_within_tolerance_passes() {
        let body = br#"{"id":"evt_1","type":"checkout.session.completed"}"#;
        let now = 1_700_000_000;
        let header = sign(body, now, SECRET);
        assert!(verify_webhook_signature(body, &header, SECRET, now).is_ok());
    }

    #[test]
    fn tampered_body_fails() {
        let now = 1_700_000_000;
        let header = sign(b"original", now, SECRET);
        assert_eq!(
            verify_webhook_signature(b"tampered", &header, SECRET, now),
            Err(SignatureError::NoMatch),
        );
    }

    #[test]
    fn wrong_secret_fails() {
        let body = b"body";
        let now = 1_700_000_000;
        let header = sign(body, now, SECRET);
        assert_eq!(
            verify_webhook_signature(body, &header, "whsec_other", now),
            Err(SignatureError::NoMatch),
        );
    }

    #[test]
    fn stale_timestamp_rejected() {
        let body = b"body";
        let signed_at = 1_700_000_000;
        let header = sign(body, signed_at, SECRET);
        // 10 minutes later — outside the 5-minute tolerance.
        let now = signed_at + 600;
        assert_eq!(
            verify_webhook_signature(body, &header, SECRET, now),
            Err(SignatureError::TimestampOutOfTolerance),
        );
    }

    #[test]
    fn multiple_v1_candidates_pass_if_any_matches() {
        let body = b"body";
        let now = 1_700_000_000;
        let good = sign(body, now, SECRET);
        // Splice a bogus v1 ahead of the real one (secret-rotation shape).
        let header = good.replace("v1=", "v1=deadbeef,v1=");
        assert!(verify_webhook_signature(body, &header, SECRET, now).is_ok());
    }

    #[test]
    fn empty_secret_is_no_secret() {
        assert_eq!(
            verify_webhook_signature(b"body", "t=1,v1=aa", "", 1),
            Err(SignatureError::NoSecret),
        );
    }

    #[test]
    fn malformed_header_rejected() {
        assert_eq!(
            verify_webhook_signature(b"body", "garbage", SECRET, 1),
            Err(SignatureError::MalformedHeader),
        );
    }
}
