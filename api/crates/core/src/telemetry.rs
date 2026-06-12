//! Tracing / logging setup + Sentry init.
//!
//! Two surfaces:
//!
//!   - `telemetry::init()` configures `tracing` with env-aware output
//!     (human-readable locally, JSON-line in deployed Lambdas).
//!   - `telemetry::init_sentry(service)` reads `SENTRY_DSN` from env
//!     and starts a Sentry client. Returns a `Option<ClientInitGuard>`
//!     that must be held for the lifetime of the process to ensure
//!     events are flushed on shutdown. When `SENTRY_DSN` is unset
//!     (local dev, CI), it returns `None` and Sentry is a no-op.
//!
//! Init order at process boot:
//!
//! ```ignore
//! telemetry::init();
//! bootstrap_ssm(prefix).await?;   // sets SENTRY_DSN among other env vars
//! let _sentry = telemetry::init_sentry("api-search");
//! ```
//!
//! `_sentry` is leading-underscore-named because we hold but don't use
//! the guard — its `Drop` flushes pending events.

use tracing_subscriber::{fmt, prelude::*, EnvFilter};

pub fn init() {
    let filter = EnvFilter::try_from_default_env().unwrap_or_else(|_| EnvFilter::new("info"));
    let registry = tracing_subscriber::registry().with(filter);

    // In dev we want human-readable; in deployed envs we want JSON for log
    // aggregators (CloudWatch / Axiom).
    if std::env::var("ML_ART_ENV").as_deref() == Ok("dev") || std::env::var("ML_ART_ENV").is_err() {
        registry.with(fmt::layer().with_target(false)).init();
    } else {
        registry.with(fmt::layer().json().with_target(false)).init();
    }
}

/// Initialize Sentry for the calling Lambda / binary.
///
/// `service` tags every event with the running surface ("api-search",
/// "jobs-lambda"). Lets one Sentry project (wander-api) collect events
/// from both Rust services while keeping them filterable in the UI.
///
/// `environment` defaults to `ML_ART_ENV` (Dev / Staging / Prod) so
/// Sentry's environment filter lines up with our Config notion.
///
/// Panics in the Rust process are auto-captured (the `panic` feature
/// installs a panic hook). Manual capture for `Result::Err` paths via
/// `sentry::capture_error(&e)` or `sentry::capture_message(...)`.
///
/// Returns `None` and is a no-op when `SENTRY_DSN` is unset — caller
/// should still bind the result so the (non-)guard's lifetime matches
/// the process.
pub fn init_sentry(service: &'static str) -> Option<sentry::ClientInitGuard> {
    // bootstrap_ssm uppercases the SSM leaf into the env name, so the
    // `/ml-art-prod/sentry_dsn_api` parameter lands as `SENTRY_DSN_API`.
    // Prefer that — but fall back to `SENTRY_DSN` for local dev where
    // a developer may have a .env without the `_api` suffix.
    let dsn = std::env::var("SENTRY_DSN_API")
        .or_else(|_| std::env::var("SENTRY_DSN"))
        .ok()?;
    if dsn.is_empty() {
        return None;
    }

    let env_name = std::env::var("ML_ART_ENV").unwrap_or_else(|_| "dev".to_string());

    let guard = sentry::init((
        dsn,
        sentry::ClientOptions {
            release: sentry::release_name!(),
            environment: Some(env_name.clone().into()),
            // 1.0 = capture every event. We're low-volume; sampling
            // becomes interesting once we're paying for Sentry overages.
            sample_rate: 1.0,
            // Traces are pricier in Sentry's billing; off until we have
            // a concrete reason to enable.
            traces_sample_rate: 0.0,
            ..Default::default()
        },
    ));

    // Tag every event with the service so api-search vs jobs-lambda
    // events are distinguishable in the wander-api project.
    sentry::configure_scope(|scope| {
        scope.set_tag("service", service);
    });

    tracing::info!(service, env = %env_name, "sentry initialized");
    Some(guard)
}
