//! Inbound webhooks — endpoints called by external services rather than
//! by our own clients. All are unauthenticated in the session sense; each
//! carries its own transport-level auth (a shared-secret header for the
//! Cloudflare email worker, an HMAC signature for Stripe).
//!
//!   - [`email`] — T-054 inbound-email → inquiry-thread stitching.
//!   - [`stripe`] — M-03 Stripe marketplace events.

pub mod email;
pub mod stripe;

// Keep the historical `webhooks::inbound_email` path stable for the
// route registration + T-054 tests.
pub use email::inbound_email;
