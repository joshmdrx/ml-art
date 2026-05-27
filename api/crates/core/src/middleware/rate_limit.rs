//! Per-key sliding-window rate limiting via the `governor` crate (GCRA).
//!
//! ## Why in-process
//!
//! v0 runs as a single Lambda function (or one local process). One bucket of
//! request state per process is fine; when we go multi-process (separate
//! `api-uploads`, multiple Lambda concurrencies, etc.) we'll swap the
//! backend for Upstash/Redis behind the same `RateLimiters` API. The
//! middleware contract here doesn't change — only the storage moves.
//!
//! ## Why next to the API and not the edge
//!
//! Edge rate limiting (WAF, CloudFront, Vercel) blocks volumetric attacks
//! before Lambda — useful, but cheap. The actual cost-exposure surface is
//! the Jina embedding API behind `/v1/search` and (later) Anthropic /
//! Rekognition behind upload + onboarding jobs. Putting the rate limit
//! right next to the paid call is what caps spend.
//!
//! Edge layers are tracked separately as `T-034` (AWS WAF) and `T-035`
//! (Vercel middleware) — they ship with the deploy infra milestone.
//!
//! ## Keying
//!
//! Precedence: Bearer JWT (signed-in user) → `X-Anonymous-Id` header
//! (anon visitor with a cookie) → `X-Forwarded-For` (proxy hop) →
//! "unknown" (last-resort shared bucket; non-attacker traffic should
//! never land here). The Bearer key uses the raw token: an invalid token
//! gets one or two requests through before auth rejects it with 401,
//! which is fine — the per-token bucket is just a burst cap.

use std::num::NonZeroU32;
use std::sync::Arc;

use axum::{
    extract::{Request, State},
    http::{header, HeaderMap},
    middleware::Next,
    response::Response,
};
use governor::{
    clock::{Clock, DefaultClock, QuantaInstant},
    state::keyed::DefaultKeyedStateStore,
    NotUntil, Quota, RateLimiter,
};

use crate::auth::ANONYMOUS_ID_HEADER;
use crate::error::ApiError;

/// Default per-route limits — derived from `03-api-data-spec.md`. Exposed
/// as constants so tests and a future config dump can refer to them
/// without re-typing the magic number.
pub const SEARCH_DEFAULT_PER_MIN: u32 = 60;
pub const INQUIRY_DEFAULT_PER_HOUR: u32 = 3;
pub const UPLOADS_DEFAULT_PER_HOUR: u32 = 20;
pub const EVENTS_DEFAULT_PER_MIN: u32 = 200;

type KeyedLimiter = RateLimiter<String, DefaultKeyedStateStore<String>, DefaultClock>;

/// Which policy a request should be checked against. One enum keeps the
/// middleware function generic; per-policy wrappers below close over the
/// variant so each route can attach the limit it needs.
#[derive(Debug, Clone, Copy)]
pub enum RateLimitPolicy {
    Search,
    Inquiry,
}

/// Shared limiter state. Attached to `AppState` and threaded through
/// `axum::middleware::from_fn_with_state`.
pub struct RateLimiters {
    search: KeyedLimiter,
    inquiry: KeyedLimiter,
    disabled: bool,
    clock: DefaultClock,
}

impl RateLimiters {
    /// Build from the application config. When `disabled` is true the
    /// middleware short-circuits to pass-through — `Config::for_tests`
    /// defaults to disabled so the existing integration suite isn't
    /// rebuilt to fake clocks.
    pub fn new(search_per_min: u32, inquiry_per_hour: u32, disabled: bool) -> Arc<Self> {
        let search_quota = Quota::per_minute(non_zero(search_per_min));
        let inquiry_quota = Quota::per_hour(non_zero(inquiry_per_hour));
        Arc::new(Self {
            search: RateLimiter::keyed(search_quota),
            inquiry: RateLimiter::keyed(inquiry_quota),
            disabled,
            clock: DefaultClock::default(),
        })
    }

    /// Direct API for unit tests: check a key against a named policy
    /// without going through Axum. Returns `Ok(())` on allow or
    /// `Err(retry_after_secs)` on deny.
    pub fn check(&self, policy: RateLimitPolicy, key: &str) -> Result<(), u64> {
        if self.disabled {
            return Ok(());
        }
        let limiter = match policy {
            RateLimitPolicy::Search => &self.search,
            RateLimitPolicy::Inquiry => &self.inquiry,
        };
        limiter
            .check_key(&key.to_string())
            .map(|_| ())
            .map_err(|neg: NotUntil<QuantaInstant>| {
                // Add 1s of slack so a client that retries exactly at
                // `now + retry_after` doesn't race the bucket refill.
                neg.wait_time_from(self.clock.now())
                    .as_secs()
                    .saturating_add(1)
            })
    }
}

fn non_zero(n: u32) -> NonZeroU32 {
    NonZeroU32::new(n.max(1)).expect("max(1) is non-zero")
}

/// Compute the bucket key for a request. See the module-level "Keying"
/// section for precedence + rationale.
pub fn extract_key(headers: &HeaderMap) -> String {
    if let Some(auth) = headers
        .get(header::AUTHORIZATION)
        .and_then(|v| v.to_str().ok())
    {
        if let Some(token) = auth.strip_prefix("Bearer ") {
            let trimmed = token.trim();
            if !trimmed.is_empty() {
                return format!("u:{trimmed}");
            }
        }
    }
    if let Some(anon) = headers
        .get(&ANONYMOUS_ID_HEADER)
        .and_then(|v| v.to_str().ok())
    {
        return format!("a:{anon}");
    }
    if let Some(forwarded) = headers.get("x-forwarded-for").and_then(|v| v.to_str().ok()) {
        if let Some(first) = forwarded.split(',').next() {
            let trimmed = first.trim();
            if !trimmed.is_empty() {
                return format!("ip:{trimmed}");
            }
        }
    }
    // Last-resort shared bucket. In practice every legit caller has at
    // least an X-Anonymous-Id (set by the Next.js middleware).
    "fallback".to_string()
}

/// Middleware: enforce the `/v1/search` policy.
pub async fn search_limit(
    State(limiters): State<Arc<RateLimiters>>,
    req: Request,
    next: Next,
) -> Result<Response, ApiError> {
    enforce(&limiters, RateLimitPolicy::Search, req, next).await
}

/// Middleware: enforce the inquiry-create policy.
pub async fn inquiry_limit(
    State(limiters): State<Arc<RateLimiters>>,
    req: Request,
    next: Next,
) -> Result<Response, ApiError> {
    enforce(&limiters, RateLimitPolicy::Inquiry, req, next).await
}

async fn enforce(
    limiters: &RateLimiters,
    policy: RateLimitPolicy,
    req: Request,
    next: Next,
) -> Result<Response, ApiError> {
    let key = extract_key(req.headers());
    match limiters.check(policy, &key) {
        Ok(()) => Ok(next.run(req).await),
        Err(retry_after_secs) => {
            tracing::info!(
                policy = ?policy,
                key = %key,
                retry_after_secs,
                "rate limited"
            );
            Err(ApiError::RateLimited { retry_after_secs })
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn key_prefers_bearer() {
        let mut h = HeaderMap::new();
        h.insert(header::AUTHORIZATION, "Bearer abc.def.ghi".parse().unwrap());
        h.insert(
            &ANONYMOUS_ID_HEADER,
            "11111111-1111-1111-1111-111111111111".parse().unwrap(),
        );
        assert_eq!(extract_key(&h), "u:abc.def.ghi");
    }

    #[test]
    fn key_falls_back_to_anon() {
        let mut h = HeaderMap::new();
        h.insert(
            &ANONYMOUS_ID_HEADER,
            "22222222-2222-2222-2222-222222222222".parse().unwrap(),
        );
        assert_eq!(extract_key(&h), "a:22222222-2222-2222-2222-222222222222");
    }

    #[test]
    fn key_falls_back_to_xff() {
        let mut h = HeaderMap::new();
        h.insert("x-forwarded-for", "203.0.113.42, 10.0.0.1".parse().unwrap());
        assert_eq!(extract_key(&h), "ip:203.0.113.42");
    }

    #[test]
    fn key_fallback_bucket() {
        let h = HeaderMap::new();
        assert_eq!(extract_key(&h), "fallback");
    }

    #[test]
    fn empty_bearer_does_not_key() {
        let mut h = HeaderMap::new();
        h.insert(header::AUTHORIZATION, "Bearer  ".parse().unwrap());
        h.insert(
            &ANONYMOUS_ID_HEADER,
            "33333333-3333-3333-3333-333333333333".parse().unwrap(),
        );
        // Falls through to anon since the trimmed token is empty.
        assert_eq!(extract_key(&h), "a:33333333-3333-3333-3333-333333333333");
    }

    #[test]
    fn disabled_always_allows() {
        let limiters = RateLimiters::new(1, 1, true);
        for _ in 0..50 {
            assert!(limiters.check(RateLimitPolicy::Search, "k").is_ok());
        }
    }

    #[test]
    fn search_denies_past_burst() {
        // Quota::per_minute(N) allows a burst of N tokens. The (N+1)th in
        // a tight loop must be denied.
        let limiters = RateLimiters::new(3, 100, false);
        for i in 0..3 {
            assert!(
                limiters.check(RateLimitPolicy::Search, "alice").is_ok(),
                "request {i} should succeed within burst"
            );
        }
        let err = limiters
            .check(RateLimitPolicy::Search, "alice")
            .expect_err("4th request must deny");
        assert!(err >= 1, "retry_after should be at least 1s, got {err}");
    }

    #[test]
    fn keys_are_isolated() {
        let limiters = RateLimiters::new(2, 100, false);
        // Burn alice's bucket.
        assert!(limiters.check(RateLimitPolicy::Search, "alice").is_ok());
        assert!(limiters.check(RateLimitPolicy::Search, "alice").is_ok());
        assert!(limiters.check(RateLimitPolicy::Search, "alice").is_err());
        // Bob is untouched.
        assert!(limiters.check(RateLimitPolicy::Search, "bob").is_ok());
        assert!(limiters.check(RateLimitPolicy::Search, "bob").is_ok());
    }

    #[test]
    fn policies_are_isolated() {
        // Same key, separate buckets per policy.
        let limiters = RateLimiters::new(1, 1, false);
        assert!(limiters.check(RateLimitPolicy::Search, "k").is_ok());
        assert!(limiters.check(RateLimitPolicy::Search, "k").is_err());
        // Inquiry bucket for the same key untouched.
        assert!(limiters.check(RateLimitPolicy::Inquiry, "k").is_ok());
        assert!(limiters.check(RateLimitPolicy::Inquiry, "k").is_err());
    }
}
