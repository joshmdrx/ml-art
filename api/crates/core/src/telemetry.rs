//! Tracing / logging setup.

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
