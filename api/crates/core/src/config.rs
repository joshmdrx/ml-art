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
    /// S3 / MinIO settings used by `core::object_store` for the
    /// `uploads` bucket (visual-search uploads). The `artworks` bucket
    /// is read-only at request time — handled by `core::images`.
    pub uploads_bucket: String,
    /// Override the SDK's endpoint. `Some("http://localhost:9000")` for
    /// MinIO; `None` for real AWS S3.
    pub s3_endpoint_url: Option<String>,
    pub s3_region: String,
    pub s3_access_key: Option<String>,
    pub s3_secret_key: Option<String>,
    /// Public URL prefix for objects in the uploads bucket. Dev is
    /// MinIO; prod is the CloudFront distribution that fronts it.
    pub uploads_public_url_prefix: String,
    /// Public-facing URL for the web app — what email links resolve
    /// to. Dev: `http://localhost:3000`. Prod: the platform's
    /// canonical hostname. Used by the inquiry-email handlers to
    /// build `…/inquiries/verify/<token>` and artwork-detail links.
    pub web_base_url: String,
    /// SQS queue URL for the jobs queue. When set, `JobsBackend`
    /// boots in `Sqs` mode (prod). When absent, `Postgres` (local
    /// dev — driven by the `jobs-worker` polling binary). The api
    /// Lambda receives this via env var from `infra/modules/api/`.
    pub jobs_queue_url: Option<String>,
    /// T-054 — domain the tokenised inquiry reply-to addresses live
    /// under (`r-<inquiry_id>-<hmac>@<reply_email_domain>`). Prod:
    /// `reply.wander.gallery`. Used by the jobs handler that emails the
    /// inquirer an artist reply; the matching inbound webhook verifies
    /// the token against `anon_cookie_secret`.
    pub reply_email_domain: String,
    /// T-054 — shared secret the Cloudflare Email Worker presents (as
    /// the `X-Inbound-Secret` header) when POSTing parsed inbound mail
    /// to `/v1/webhooks/email/inbound`. The webhook is closed unless
    /// this is set AND the header matches. Required in prod.
    pub inbound_email_secret: Option<String>,
    /// T-056.3 — default-on switch for the personalised search blend.
    /// When `false` (default), `/v1/search` only adds the taste channel
    /// to the RRF fusion if the caller explicitly opts in with
    /// `?personalize=on`. When `true`, eligible signed-in users get
    /// personalised results unless they pass `?personalize=off`.
    /// Pre-cohort A/B; flipping this is the operator-side kill switch.
    pub search_personalize_enabled: bool,
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
            uploads_bucket: env::var("S3_UPLOADS_BUCKET").unwrap_or_else(|_| "uploads".to_string()),
            s3_endpoint_url: env::var("S3_ENDPOINT_URL").ok(),
            s3_region: env::var("AWS_REGION").unwrap_or_else(|_| "us-east-1".to_string()),
            s3_access_key: env::var("AWS_ACCESS_KEY_ID").ok(),
            s3_secret_key: env::var("AWS_SECRET_ACCESS_KEY").ok(),
            uploads_public_url_prefix: env::var("UPLOADS_PUBLIC_URL_PREFIX")
                .unwrap_or_else(|_| "http://localhost:9000/uploads".to_string()),
            web_base_url: env::var("WEB_BASE_URL")
                .unwrap_or_else(|_| "http://localhost:3000".to_string()),
            jobs_queue_url: env::var("JOBS_QUEUE_URL").ok(),
            reply_email_domain: env::var("REPLY_EMAIL_DOMAIN")
                .unwrap_or_else(|_| "reply.localhost".to_string()),
            inbound_email_secret: env::var("INBOUND_EMAIL_SECRET").ok(),
            search_personalize_enabled: env::var("SEARCH_PERSONALIZE_ENABLED")
                .map(|s| s == "true")
                .unwrap_or(false),
        };

        // Production sanity checks. Prevent footguns from a missing secret in prod.
        if cfg.env == Environment::Prod {
            if cfg.anon_cookie_secret == "dev-secret-rotate-in-prod" {
                anyhow::bail!("ANON_COOKIE_SECRET must be set to a real secret in prod");
            }
            if cfg.clerk_issuer.is_none() || cfg.clerk_jwks_url.is_none() {
                anyhow::bail!("CLERK_ISSUER and CLERK_JWKS_URL required in prod");
            }
            if cfg.inbound_email_secret.is_none() {
                anyhow::bail!("INBOUND_EMAIL_SECRET required in prod (inbound-reply webhook auth)");
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
            uploads_bucket: "uploads".to_string(),
            s3_endpoint_url: None,
            s3_region: "us-east-1".to_string(),
            s3_access_key: None,
            s3_secret_key: None,
            uploads_public_url_prefix: "https://test.example.com/uploads".to_string(),
            web_base_url: "https://test.example.com".to_string(),
            jobs_queue_url: None,
            reply_email_domain: "reply.test.example.com".to_string(),
            inbound_email_secret: Some("test-inbound-secret".to_string()),
            // Off in tests by default — the T-056.3 toggle tests explicitly
            // flip this on or pass ?personalize=on.
            search_personalize_enabled: false,
        }
    }
}

/// Fetch every SecureString parameter under `prefix` and inject as
/// uppercased process env vars. Called once at Lambda cold start
/// **before** `Config::load()` runs, so the rest of the loader sees
/// SSM-backed values as if they were native env vars.
///
/// Naming convention: SSM `database_url` → env `DATABASE_URL`.
/// The mapping is just `to_ascii_uppercase` — the SSM paths were
/// chosen in `modules/secrets/` to match what `Config::load()` reads.
///
/// Safety: `std::env::set_var` is `unsafe` from Rust 1.81+ because env
/// access from multiple threads is a data race. We call this *before*
/// the Tokio runtime starts (in `main`, single-threaded), so the
/// invariant holds. Document this here so a future refactor doesn't
/// move the call inside an async context.
///
/// Lambda execution role needs `ssm:GetParametersByPath` on the
/// prefix — already wired in `modules/api/main.tf` +
/// `modules/jobs/main.tf` + `modules/web/main.tf`.
pub async fn bootstrap_ssm(prefix: &str) -> anyhow::Result<()> {
    let aws_cfg = aws_config::load_from_env().await;
    let client = aws_sdk_ssm::Client::new(&aws_cfg);

    let mut next_token: Option<String> = None;
    let mut total = 0usize;

    // SSM caps GetParametersByPath at 10 results per page. Paginate —
    // we're at ~10 keys today (right at the one-page boundary; T-054's
    // inbound_email_secret pushed us here), so the loop now genuinely
    // earns its keep rather than just future-proofing.
    loop {
        let resp = client
            .get_parameters_by_path()
            .path(prefix)
            .recursive(false)
            .with_decryption(true)
            .set_next_token(next_token.clone())
            .send()
            .await
            .map_err(|e| anyhow::anyhow!("ssm get-parameters-by-path failed: {e}"))?;

        for p in resp.parameters() {
            // Path: "/ml-art-prod/database_url" → key: "database_url"
            // → env: "DATABASE_URL"
            let name = p.name().unwrap_or("");
            let leaf = name.rsplit('/').next().unwrap_or(name);
            if let Some(value) = p.value() {
                let env_key = leaf.to_ascii_uppercase();
                // SAFETY: see fn-level doc — main()-time, pre-tokio,
                // single-threaded.
                unsafe {
                    std::env::set_var(&env_key, value);
                }
                total += 1;
            }
        }

        next_token = resp.next_token().map(|s| s.to_string());
        if next_token.is_none() {
            break;
        }
    }

    tracing::info!(
        ssm_path = prefix,
        count = total,
        "loaded SSM parameters into env"
    );
    Ok(())
}
