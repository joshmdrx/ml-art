//! T-064 — block direct hits to the API Gateway invoke URL by requiring
//! a shared secret CloudFront injects on every origin request.
//!
//! **Why:** The API Gateway URL (`*.execute-api.us-east-1.amazonaws.com`)
//! is publicly reachable, which lets any caller bypass CloudFront + WAF.
//! Not a critical vulnerability today (the Lambda serves the same
//! content, WAF is per-distribution not per-Lambda so direct hits skip
//! it), but two real consequences:
//!   - Search engines could index a duplicate-content copy under the
//!     execute-api URL.
//!   - The volumetric-attack cap in `aws_wafv2_web_acl.api` doesn't apply
//!     to direct traffic — an attacker can still hammer the Lambda.
//!
//! **How:** CloudFront's origin config adds a `custom_header`:
//!   `X-CloudFront-Secret: <random>` — TF-managed via `random_password`
//!   in `modules/api/main.tf` + written to SSM so the Lambda reads it
//!   on cold-start (via `bootstrap_ssm`).
//!
//! This middleware compares that header against `Config.cloudfront_
//! shared_secret` in constant time. When the secret is `None` /
//! empty (dev, CI, E2E), the gate is pass-through so localhost calls
//! keep working. When set, missing/mismatched headers get a 403 with
//! no body.
//!
//! Layered at the top of the api-search Router so it fires before any
//! per-route auth (a rejected caller shouldn't learn whether an
//! endpoint exists — the 403 is bland on purpose).

use std::sync::Arc;

use axum::{
    extract::{Request, State},
    http::StatusCode,
    middleware::Next,
    response::{IntoResponse, Response},
};

/// Header CloudFront injects; matched against `CloudFrontGate.secret`.
const HEADER: &str = "x-cloudfront-secret";

/// Shared state attached via `from_fn_with_state`. Wraps the config
/// secret so the middleware doesn't need to reach into `AppState`
/// (which lives in `api-search`, not `core`).
#[derive(Debug, Clone)]
pub struct CloudFrontGate {
    /// `None` → pass-through. `Some(secret)` → require matching header.
    secret: Option<String>,
}

impl CloudFrontGate {
    pub fn new(secret: Option<String>) -> Arc<Self> {
        Arc::new(Self { secret })
    }

    /// Test hook: build the gate directly from a value.
    #[cfg(test)]
    pub fn for_test(secret: Option<&str>) -> Arc<Self> {
        Arc::new(Self {
            secret: secret.map(str::to_string),
        })
    }

    fn header_ok(&self, header_value: Option<&[u8]>) -> bool {
        let Some(expected) = self.secret.as_deref() else {
            return true; // pass-through
        };
        let Some(presented) = header_value else {
            return false;
        };
        ct_eq(presented, expected.as_bytes())
    }
}

/// Middleware entry point. Attach with:
///   `.layer(from_fn_with_state(gate, cloudfront_gate))`
pub async fn cloudfront_gate(
    State(gate): State<Arc<CloudFrontGate>>,
    req: Request,
    next: Next,
) -> Response {
    let header = req.headers().get(HEADER).map(|v| v.as_bytes());
    if !gate.header_ok(header) {
        // Deliberately bland — see module doc: don't hint at
        // route existence to a caller that can't clear the gate.
        return (StatusCode::FORBIDDEN, "").into_response();
    }
    next.run(req).await
}

/// Constant-time byte equality. Length is allowed to leak (the secret
/// is fixed-length once set); content is compared without early-exit.
///
/// Mirrors the helper in `api-search/src/webhooks.rs` — kept private
/// per module rather than shared out of `core` because the two seams
/// are one-liners each and coupling them under a public crate export
/// is more coupling than it's worth. Consolidate if a third caller
/// lands.
fn ct_eq(a: &[u8], b: &[u8]) -> bool {
    if a.len() != b.len() {
        return false;
    }
    let mut diff = 0u8;
    for (x, y) in a.iter().zip(b.iter()) {
        diff |= x ^ y;
    }
    diff == 0
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn pass_through_when_secret_unset() {
        let gate = CloudFrontGate::for_test(None);
        assert!(gate.header_ok(None));
        assert!(gate.header_ok(Some(b"anything")));
        assert!(gate.header_ok(Some(b"")));
    }

    #[test]
    fn matching_header_allowed_when_set() {
        let gate = CloudFrontGate::for_test(Some("real-secret"));
        assert!(gate.header_ok(Some(b"real-secret")));
    }

    #[test]
    fn missing_header_rejected_when_set() {
        let gate = CloudFrontGate::for_test(Some("real-secret"));
        assert!(!gate.header_ok(None));
    }

    #[test]
    fn wrong_header_rejected_when_set() {
        let gate = CloudFrontGate::for_test(Some("real-secret"));
        assert!(!gate.header_ok(Some(b"wrong-secret")));
        // Different length — early-return path.
        assert!(!gate.header_ok(Some(b"real")));
        assert!(!gate.header_ok(Some(b"real-secret-plus-extra")));
    }

    #[test]
    fn empty_header_rejected_when_set() {
        let gate = CloudFrontGate::for_test(Some("real-secret"));
        assert!(!gate.header_ok(Some(b"")));
    }
}
