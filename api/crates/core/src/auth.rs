//! Request-time identity extractors.
//!
//! Two kinds:
//!   - `AnonId(Uuid)` / `OptionalAnonId(Option<Uuid>)` — anonymous identity
//!     from the `X-Anonymous-Id` header forwarded by Next.js
//!     (see `decisions.md` 2026-05-26)
//!   - `User { id, clerk_user_id }` — authenticated identity from a Clerk
//!     JWT (Authorization: Bearer ...). On first sight of a `clerk_user_id`
//!     we haven't synced, we fetch the user's email from Clerk's API and
//!     create a row in our `users` table.
//!
//! The state needs to expose a `JwtVerifier` and a Postgres `Pool` to let
//! the `User` extractor work. Bind via the `HasAuthContext` trait.

use axum::{
    extract::FromRequestParts,
    http::{request::Parts, HeaderName, StatusCode},
};
use jsonwebtoken::{decode, decode_header, jwk::JwkSet, Algorithm, DecodingKey, Validation};
use serde::Deserialize;
use sqlx::PgPool;
use std::future::Future;
use std::pin::Pin;
use std::sync::Arc;
use tokio::sync::RwLock;
use uuid::Uuid;

use crate::error::ApiError;

// ─────────────────────────────────────────────────────────────────────────────
// Anonymous identity (forwarded from Next.js)
// ─────────────────────────────────────────────────────────────────────────────

pub const ANONYMOUS_ID_HEADER: HeaderName = HeaderName::from_static("x-anonymous-id");

#[derive(Debug, Clone, Copy)]
pub struct AnonId(pub Uuid);

impl<S> FromRequestParts<S> for AnonId
where
    S: Send + Sync,
{
    type Rejection = ApiError;

    fn from_request_parts<'a, 'b, 'fut>(
        parts: &'a mut Parts,
        _state: &'b S,
    ) -> Pin<Box<dyn Future<Output = Result<Self, Self::Rejection>> + Send + 'fut>>
    where
        'a: 'fut,
        'b: 'fut,
        S: 'fut,
    {
        let result = parse_required_anon(parts);
        Box::pin(async move { result })
    }
}

#[derive(Debug, Clone, Copy, Default)]
pub struct OptionalAnonId(pub Option<Uuid>);

impl<S> FromRequestParts<S> for OptionalAnonId
where
    S: Send + Sync,
{
    type Rejection = (StatusCode, String);

    fn from_request_parts<'a, 'b, 'fut>(
        parts: &'a mut Parts,
        _state: &'b S,
    ) -> Pin<Box<dyn Future<Output = Result<Self, Self::Rejection>> + Send + 'fut>>
    where
        'a: 'fut,
        'b: 'fut,
        S: 'fut,
    {
        let result = parse_optional_anon(parts);
        Box::pin(async move { result })
    }
}

fn parse_required_anon(parts: &Parts) -> Result<AnonId, ApiError> {
    let raw = parts
        .headers
        .get(&ANONYMOUS_ID_HEADER)
        .ok_or_else(|| ApiError::BadRequest("missing X-Anonymous-Id header".into()))?
        .to_str()
        .map_err(|_| ApiError::BadRequest("X-Anonymous-Id is not valid UTF-8".into()))?;
    let uuid = Uuid::parse_str(raw)
        .map_err(|_| ApiError::BadRequest("X-Anonymous-Id is not a valid UUID".into()))?;
    Ok(AnonId(uuid))
}

fn parse_optional_anon(parts: &Parts) -> Result<OptionalAnonId, (StatusCode, String)> {
    let Some(value) = parts.headers.get(&ANONYMOUS_ID_HEADER) else {
        return Ok(OptionalAnonId(None));
    };
    let s = value.to_str().map_err(|_| {
        (
            StatusCode::BAD_REQUEST,
            "X-Anonymous-Id is not valid UTF-8".to_string(),
        )
    })?;
    let uuid = Uuid::parse_str(s).map_err(|_| {
        (
            StatusCode::BAD_REQUEST,
            "X-Anonymous-Id is not a valid UUID".to_string(),
        )
    })?;
    Ok(OptionalAnonId(Some(uuid)))
}

// ─────────────────────────────────────────────────────────────────────────────
// Clerk JWT verification + lazy user sync
// ─────────────────────────────────────────────────────────────────────────────

/// Claims we read from a Clerk session JWT. Clerk's default token only
/// includes `sub` (clerk user id), `iss`, `exp`, etc — no email or name.
/// Custom session-token templates can add more; we don't rely on them.
#[derive(Debug, Clone, Deserialize)]
pub struct ClerkClaims {
    pub sub: String,
    pub iss: String,
    pub exp: usize,
    pub iat: usize,
    pub nbf: Option<usize>,
    #[serde(default)]
    pub sid: Option<String>,
}

#[derive(Clone)]
pub struct JwtVerifier {
    inner: Arc<JwtVerifierInner>,
}

struct JwtVerifierInner {
    issuer: String,
    jwks_url: String,
    clerk_secret_key: Option<String>,
    cached_jwks: RwLock<Option<JwkSet>>,
    http: reqwest::Client,
    /// When true, `verify` accepts any token of the form `test-<sub>` and
    /// returns a fake `ClerkClaims` with that `sub`. Construct via
    /// `JwtVerifier::for_tests()`. Production code never sets this.
    test_mode: bool,
}

impl JwtVerifier {
    pub fn new(
        issuer: Option<String>,
        jwks_url: Option<String>,
        clerk_secret_key: Option<String>,
    ) -> Self {
        // If either is missing the verifier is effectively disabled — any
        // call to verify() returns Unauthorized. Avoids panicking at startup
        // in dev when Clerk isn't configured.
        let http = reqwest::Client::builder()
            .timeout(std::time::Duration::from_secs(10))
            .build()
            .expect("reqwest client");
        Self {
            inner: Arc::new(JwtVerifierInner {
                issuer: issuer.unwrap_or_default(),
                jwks_url: jwks_url.unwrap_or_default(),
                clerk_secret_key,
                cached_jwks: RwLock::new(None),
                http,
                test_mode: false,
            }),
        }
    }

    /// Test-only constructor: bypasses JWKS entirely. Any token of the form
    /// `test-<sub>` (e.g. `test-user_test_alice`) verifies and resolves to
    /// `ClerkClaims { sub: "user_test_alice", ... }`. Tests should pre-seed
    /// a `users` row whose `clerk_user_id` matches the token suffix so the
    /// upsert path hits the fast SELECT branch (no Clerk API call).
    ///
    /// Production code MUST NOT call this — the presence of the call is the gate.
    pub fn for_tests() -> Self {
        Self {
            inner: Arc::new(JwtVerifierInner {
                issuer: "test://".to_string(),
                jwks_url: "test://".to_string(),
                clerk_secret_key: None,
                cached_jwks: RwLock::new(None),
                http: reqwest::Client::new(),
                test_mode: true,
            }),
        }
    }

    pub fn enabled(&self) -> bool {
        !self.inner.issuer.is_empty() && !self.inner.jwks_url.is_empty()
    }

    /// Verify a Clerk JWT, returning its claims.
    pub async fn verify(&self, token: &str) -> Result<ClerkClaims, ApiError> {
        if !self.enabled() {
            return Err(ApiError::Unauthorized);
        }

        // Test mode: bypass JWKS, accept `test-<sub>`.
        if self.inner.test_mode {
            let sub = token.strip_prefix("test-").ok_or(ApiError::Unauthorized)?;
            return Ok(ClerkClaims {
                sub: sub.to_string(),
                iss: "test://".to_string(),
                exp: i64::MAX as usize,
                iat: 0,
                nbf: None,
                sid: None,
            });
        }

        let header = decode_header(token).map_err(|_| ApiError::Unauthorized)?;
        let kid = header.kid.ok_or(ApiError::Unauthorized)?;

        let key = self.find_key(&kid).await?;

        let mut validation = Validation::new(Algorithm::RS256);
        validation.set_issuer(std::slice::from_ref(&self.inner.issuer));
        // Clerk doesn't always set `aud`; don't enforce.
        validation.validate_aud = false;

        let data =
            decode::<ClerkClaims>(token, &key, &validation).map_err(|_| ApiError::Unauthorized)?;
        Ok(data.claims)
    }

    /// Look up a JWK by `kid`. Caches the JWKS in memory; refetches if the
    /// kid isn't in the cached set (Clerk rotates keys occasionally).
    async fn find_key(&self, kid: &str) -> Result<DecodingKey, ApiError> {
        // Fast path: try the cached JWKS.
        if let Some(jwks) = self.inner.cached_jwks.read().await.as_ref() {
            if let Some(jwk) = jwks.find(kid) {
                return DecodingKey::from_jwk(jwk).map_err(|_| ApiError::Unauthorized);
            }
        }
        // Slow path: refetch.
        let fresh = self.fetch_jwks().await?;
        let key = fresh
            .find(kid)
            .ok_or(ApiError::Unauthorized)
            .and_then(|jwk| DecodingKey::from_jwk(jwk).map_err(|_| ApiError::Unauthorized));
        *self.inner.cached_jwks.write().await = Some(fresh);
        key
    }

    async fn fetch_jwks(&self) -> Result<JwkSet, ApiError> {
        let resp = self
            .inner
            .http
            .get(&self.inner.jwks_url)
            .send()
            .await
            .map_err(|e| ApiError::Internal(anyhow::anyhow!("jwks fetch: {e}")))?;
        if !resp.status().is_success() {
            return Err(ApiError::Internal(anyhow::anyhow!(
                "jwks fetch returned {}",
                resp.status()
            )));
        }
        let jwks: JwkSet = resp
            .json()
            .await
            .map_err(|e| ApiError::Internal(anyhow::anyhow!("jwks parse: {e}")))?;
        Ok(jwks)
    }

    /// Fetch a Clerk user's primary email via the backend API. Used during
    /// lazy sync into our `users` table on the first sight of a new
    /// `clerk_user_id`.
    pub async fn fetch_clerk_email(&self, clerk_user_id: &str) -> Result<String, ApiError> {
        let key = self.inner.clerk_secret_key.as_deref().ok_or_else(|| {
            ApiError::Internal(anyhow::anyhow!("CLERK_SECRET_KEY not configured"))
        })?;
        let url = format!("https://api.clerk.com/v1/users/{}", clerk_user_id);
        let resp = self
            .inner
            .http
            .get(&url)
            .bearer_auth(key)
            .send()
            .await
            .map_err(|e| ApiError::Internal(anyhow::anyhow!("clerk users: {e}")))?;
        if !resp.status().is_success() {
            return Err(ApiError::Internal(anyhow::anyhow!(
                "clerk /users returned {}",
                resp.status()
            )));
        }
        let body: ClerkUserResponse = resp
            .json()
            .await
            .map_err(|e| ApiError::Internal(anyhow::anyhow!("clerk user parse: {e}")))?;
        let primary_id = body.primary_email_address_id.as_deref();
        let primary = body
            .email_addresses
            .iter()
            .find(|e| primary_id.map(|p| p == e.id).unwrap_or(false))
            .or_else(|| body.email_addresses.first())
            .ok_or_else(|| {
                ApiError::Internal(anyhow::anyhow!("clerk user has no email addresses"))
            })?;
        Ok(primary.email_address.clone())
    }
}

#[derive(Deserialize)]
struct ClerkUserResponse {
    #[allow(dead_code)]
    id: String,
    primary_email_address_id: Option<String>,
    email_addresses: Vec<ClerkEmail>,
}

#[derive(Deserialize)]
struct ClerkEmail {
    id: String,
    email_address: String,
}

// ─────────────────────────────────────────────────────────────────────────────
// User authentication (helper-function flavor — handlers call directly)
// ─────────────────────────────────────────────────────────────────────────────

/// Authenticated user. Acquired by passing a request's `Authorization`
/// header through `authenticate`, which verifies the Clerk JWT and
/// upserts a row in `users`.
#[derive(Debug, Clone)]
pub struct User {
    pub id: Uuid,
    pub clerk_user_id: String,
    pub email: String,
    pub is_admin: bool,
}

/// Verify the Bearer token on a request and resolve it to a local `User`,
/// creating the row on first sight. 401 for missing / invalid / expired.
///
/// Called directly from handlers rather than via `FromRequestParts` — the
/// orphan rules for foreign-trait extractors against cross-crate state
/// (`Arc<AppState>`) aren't worth the abstraction cost at this stage.
pub async fn authenticate(
    headers: &axum::http::HeaderMap,
    verifier: &JwtVerifier,
    pool: &PgPool,
) -> Result<User, ApiError> {
    let token = parse_bearer(headers)?;
    let claims = verifier.verify(&token).await?;
    upsert_user(pool, verifier, &claims).await
}

fn parse_bearer(headers: &axum::http::HeaderMap) -> Result<String, ApiError> {
    let value = headers
        .get(axum::http::header::AUTHORIZATION)
        .ok_or(ApiError::Unauthorized)?
        .to_str()
        .map_err(|_| ApiError::Unauthorized)?;
    let token = value
        .strip_prefix("Bearer ")
        .ok_or(ApiError::Unauthorized)?
        .trim();
    if token.is_empty() {
        return Err(ApiError::Unauthorized);
    }
    Ok(token.to_string())
}

/// T-083 — emails that are auto-promoted to `is_admin = true` on first
/// sign-in. Lowercase, ASCII. The migration `0024_admin_audit_log.sql`
/// also runs a one-off UPDATE for the same set so a user who signed in
/// before the deploy is promoted in place. Either path covers either
/// ordering.
///
/// Grow this list when the next admin onboards; the seed lookup is a
/// `==` check so there's no per-row cost at scale.
pub const ADMIN_EMAILS: &[&str] = &["mrjoshuajmatthews@gmail.com"];

/// Optional env-var-driven allowlist that supplements `ADMIN_EMAILS`.
/// Comma-separated list of email suffixes; any user whose email ends
/// with one of them is auto-promoted to `is_admin = true`. Empty
/// (unset) in prod — this seam only exists so the E2E harness can
/// promote a per-run randomized test user (matching the suffix
/// `-admin+clerk_test@example.com`) to admin without hardcoding
/// a test literal into the production const. See
/// `docs/e2e-coverage.md` → "Admin" for the register.
const ADMIN_ALLOWLIST_ENV: &str = "WANDER_ADMIN_EMAIL_ALLOWLIST";

fn is_seeded_admin_email(email: &str) -> bool {
    let lower = email.to_ascii_lowercase();
    if ADMIN_EMAILS.iter().any(|e| *e == lower) {
        return true;
    }
    match std::env::var(ADMIN_ALLOWLIST_ENV) {
        Ok(extra) => extra
            .split(',')
            .map(str::trim)
            .filter(|s| !s.is_empty())
            .any(|suffix| lower.ends_with(&suffix.to_ascii_lowercase())),
        Err(_) => false,
    }
}

/// Insert the user if we haven't seen this `clerk_user_id` before; either
/// way, return our internal `User` record. Calls Clerk's API for the email
/// on first sight (one extra HTTP request per user, lifetime).
async fn upsert_user(
    pool: &PgPool,
    verifier: &JwtVerifier,
    claims: &ClerkClaims,
) -> Result<User, ApiError> {
    // Fast path: already exists.
    if let Some(u) = sqlx::query_as::<_, UserRow>(
        r#"SELECT id, clerk_user_id, email, is_admin FROM users WHERE clerk_user_id = $1"#,
    )
    .bind(&claims.sub)
    .fetch_optional(pool)
    .await?
    {
        return Ok(u.into_user());
    }

    // Slow path: fetch email from Clerk and insert. Seed is_admin from
    // ADMIN_EMAILS so the platform's first admin can sign up without a
    // post-hoc psql edit; the migration covers the inverse case where
    // they signed in before the admin-email constant landed. The ON
    // CONFLICT branch OR's the seed flag against the existing value so
    // a manual promotion is never overwritten.
    let email = verifier.fetch_clerk_email(&claims.sub).await?;
    let seed_admin = is_seeded_admin_email(&email);
    let row: UserRow = sqlx::query_as(
        r#"
        INSERT INTO users (clerk_user_id, email, is_admin)
        VALUES ($1, $2, $3)
        ON CONFLICT (clerk_user_id) DO UPDATE
           SET email = EXCLUDED.email,
               is_admin = users.is_admin OR EXCLUDED.is_admin,
               updated_at = now()
        RETURNING id, clerk_user_id, email, is_admin
        "#,
    )
    .bind(&claims.sub)
    .bind(&email)
    .bind(seed_admin)
    .fetch_one(pool)
    .await?;
    Ok(row.into_user())
}

#[derive(sqlx::FromRow)]
struct UserRow {
    id: Uuid,
    clerk_user_id: String,
    email: String,
    is_admin: bool,
}

impl UserRow {
    fn into_user(self) -> User {
        User {
            id: self.id,
            clerk_user_id: self.clerk_user_id,
            email: self.email,
            is_admin: self.is_admin,
        }
    }
}
