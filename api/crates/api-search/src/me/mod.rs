//! `/v1/me/*` — endpoints scoped to the authenticated user.
//!
//! Every handler in this module verifies a Clerk JWT via
//! `ml_art_core::auth::authenticate`, which lazily upserts the user into
//! our `users` table on first sight.

pub mod collections;
pub mod current_user;

pub use current_user::current_user;
