//! Axum extractors that bind directly to `AppState`. Lives here (in the
//! binary crate) rather than in `core` because of orphan rules — the
//! `State` type is owned by the binary. See `decisions.md` 2026-05-27 —
//! `User` as an axum `FromRequestParts` extractor.
//!
//! Newtype-wrapped because we can't `impl FromRequestParts<Arc<AppState>>
//! for ml_art_core::auth::User` directly — all of `User`, `Arc`, and
//! `FromRequestParts` are foreign, and `AppState` doesn't appear as a
//! covered type parameter. The wrapper costs one extra destructure at
//! the call site and gives us a hook for binary-specific policy later
//! (e.g. refusing non-admin tokens from a `/v1/admin/*` surface).
//!
//! Uses the boxed-future shape of `FromRequestParts` to match axum 0.7's
//! pre-async-trait-stabilization API. Same pattern `core::auth::AnonId`
//! follows. On axum 0.8+ this becomes a plain `async fn`.

use axum::{extract::FromRequestParts, http::request::Parts};
use ml_art_core::{auth, error::ApiError};
use std::future::Future;
use std::pin::Pin;
use std::sync::Arc;

use crate::AppState;

/// Authenticated caller. Equivalent to `ml_art_core::auth::User`, just
/// wrapped so the `FromRequestParts` impl is locally-owned. Handlers
/// pattern-match: `AuthedUser(user): AuthedUser`.
pub struct AuthedUser(pub auth::User);

impl FromRequestParts<Arc<AppState>> for AuthedUser {
    type Rejection = ApiError;

    fn from_request_parts<'a, 'b, 'fut>(
        parts: &'a mut Parts,
        state: &'b Arc<AppState>,
    ) -> Pin<Box<dyn Future<Output = Result<Self, Self::Rejection>> + Send + 'fut>>
    where
        'a: 'fut,
        'b: 'fut,
    {
        Box::pin(async move {
            let user = auth::authenticate(&parts.headers, &state.jwt_verifier, &state.pool).await?;
            Ok(AuthedUser(user))
        })
    }
}
