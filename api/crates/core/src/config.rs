//! Environment configuration.
//!
//! Loaded once at startup. Required vars produce a startup-time error in
//! production; optional vars (paid APIs) gracefully degrade per `COST.md`.

use std::env;

#[derive(Debug, Clone)]
pub struct Config {
    pub env: Environment,
    pub database_url: String,
    pub port: u16,
    /// Defaults `jinaai/jina-clip-v2`. Persisted with each cached embedding.
    pub embedding_model_name: String,
    pub embedding_model_version: String,
    /// If absent, search degrades to keyword-only (saves Jina spend in dev).
    pub jina_api_key: Option<String>,
    /// If absent, geocoding job no-ops.
    pub mapbox_token: Option<String>,
    /// If absent, image moderation auto-approves (dev shortcut).
    pub rekognition_enabled: bool,
    /// Cookie HMAC secret for anonymous_id signing.
    pub anon_cookie_secret: String,
    /// Clerk JWT issuer + JWKS URL for verifying user tokens.
    pub clerk_issuer: Option<String>,
    pub clerk_jwks_url: Option<String>,
    /// Clerk backend secret. Used to call Clerk's API for user metadata
    /// (e.g. email) on first sight of a `clerk_user_id` we haven't synced.
    pub clerk_secret_key: Option<String>,
    /// When true, the rate-limit middleware short-circuits to pass-through.
    /// Set via `RATE_LIMIT_DISABLED=true` for dev hammer-testing; defaults
    /// to true in `Config::for_tests` so integration tests don't need to
    /// fake clocks. See `core::middleware::rate_limit`.
    pub rate_limit_disabled: bool,
    /// Per-minute quota for `/v1/search`, keyed per user / anon / IP.
    /// Override via `RATE_LIMIT_SEARCH_PER_MIN` (test fixtures dial this
    /// way down to assert 429 behavior without 61-request loops).
    pub rate_limit_search_per_min: u32,
    /// Per-hour quota for inquiry submission, same keying.
    /// Override via `RATE_LIMIT_INQUIRY_PER_HOUR`.
    pub rate_limit_inquiry_per_hour: u32,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Environment {
    Dev,
    Staging,
    Prod,
}

impl Environment {
    pub fn is_dev(&self) -> bool {
        matches!(self, Self::Dev)
    }
}

impl Config {
    /// Load from process environment. Panics on startup if a required var is missing.
    ///
    /// Also loads `.env` from the current directory if present (best-effort —
    /// no error if absent, so production deploys reading from real env vars
    /// aren't surprised).
    pub fn load() -> anyhow::Result<Self> {
        let _ = dotenvy::dotenv(); // best-effort, ignore missing-file errors

        let env = match env::var("ML_ART_ENV").as_deref() {
            Ok("staging") => Environment::Staging,
            Ok("prod") => Environment::Prod,
            _ => Environment::Dev,
        };

        let required = |name: &str| -> anyhow::Result<String> {
            env::var(name).map_err(|_| anyhow::anyhow!("missing required env var: {name}"))
        };

        let cfg = Config {
            env,
            database_url: required("DATABASE_URL")?,
            port: env::var("PORT")
                .ok()
                .and_then(|s| s.parse().ok())
                .unwrap_or(9000),
            embedding_model_name: env::var("EMBEDDING_MODEL_NAME")
                .unwrap_or_else(|_| "jinaai/jina-clip-v2".to_string()),
            // Unified label as of migration 0009 (T-024). Python local seed
            // and Rust HTTP path both write `'v2'` for jinaai/jina-clip-v2.
            embedding_model_version: env::var("EMBEDDING_MODEL_VERSION")
                .unwrap_or_else(|_| "v2".to_string()),
            jina_api_key: env::var("JINA_API_KEY").ok(),
            mapbox_token: env::var("MAPBOX_TOKEN").ok(),
            rekognition_enabled: env::var("REKOGNITION_ENABLED")
                .map(|s| s == "true")
                .unwrap_or(false),
            anon_cookie_secret: env::var("ANON_COOKIE_SECRET")
                .unwrap_or_else(|_| "dev-secret-rotate-in-prod".to_string()),
            clerk_issuer: env::var("CLERK_ISSUER").ok(),
            clerk_jwks_url: env::var("CLERK_JWKS_URL").ok(),
            clerk_secret_key: env::var("CLERK_SECRET_KEY").ok(),
            rate_limit_disabled: env::var("RATE_LIMIT_DISABLED")
                .map(|s| s == "true")
                .unwrap_or(false),
            rate_limit_search_per_min: env::var("RATE_LIMIT_SEARCH_PER_MIN")
                .ok()
                .and_then(|s| s.parse().ok())
                .unwrap_or(60),
            rate_limit_inquiry_per_hour: env::var("RATE_LIMIT_INQUIRY_PER_HOUR")
                .ok()
                .and_then(|s| s.parse().ok())
                .unwrap_or(3),
        };

        // Production sanity checks. Prevent footguns from a missing secret in prod.
        if cfg.env == Environment::Prod {
            if cfg.anon_cookie_secret == "dev-secret-rotate-in-prod" {
                anyhow::bail!("ANON_COOKIE_SECRET must be set to a real secret in prod");
            }
            if cfg.clerk_issuer.is_none() || cfg.clerk_jwks_url.is_none() {
                anyhow::bail!("CLERK_ISSUER and CLERK_JWKS_URL required in prod");
            }
        }

        Ok(cfg)
    }

    /// Construct a deterministic Config for integration tests. No env access.
    pub fn for_tests(database_url: String) -> Self {
        Config {
            env: Environment::Dev,
            database_url,
            port: 0,
            embedding_model_name: "jinaai/jina-clip-v2".to_string(),
            embedding_model_version: "v2".to_string(),
            jina_api_key: None,
            mapbox_token: None,
            rekognition_enabled: false,
            anon_cookie_secret: "test-cookie-secret".to_string(),
            clerk_issuer: None,
            clerk_jwks_url: None,
            clerk_secret_key: None,
            // Off by default for the broad suite — individual rate-limit tests
            // build a Config with this flipped on and the quotas dialed down.
            rate_limit_disabled: true,
            rate_limit_search_per_min: 60,
            rate_limit_inquiry_per_hour: 3,
        }
    }
}
