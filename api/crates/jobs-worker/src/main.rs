//! Local-dev jobs worker. Polls the `jobs` table, runs the right
//! handler for each event kind, marks done/retry/failed.
//!
//! In prod the same handler dispatch (`core::jobs::handle`) is
//! invoked from a `cargo-lambda` binary fed by SQS — see
//! `decisions.md` 2026-05-29 — jobs queue: Postgres local, SQS+Lambda
//! prod. This binary is the local driver; nothing in it should leak
//! into the handler code.
//!
//! Run as `cargo run -p jobs-worker`. `make dev` spawns it alongside
//! `api-search` so `tokio::spawn`s aren't needed for background work.

use std::time::Duration;

use ml_art_core::{
    config::Config,
    geocoding::GeocodingClient,
    jobs::{self, JobsDeps},
};
use tracing::{debug, error, info, warn};

const POLL_INTERVAL: Duration = Duration::from_secs(2);

#[tokio::main]
async fn main() -> anyhow::Result<()> {
    ml_art_core::telemetry::init();
    let cfg = Config::load()?;
    let pool = ml_art_core::db::make_pool(&cfg.database_url).await?;
    let geocoder = GeocodingClient::from_env();
    let deps = JobsDeps {
        pool: pool.clone(),
        geocoder,
    };

    info!(
        poll_interval_secs = POLL_INTERVAL.as_secs(),
        "jobs-worker started"
    );

    loop {
        match jobs::postgres::claim_one(&pool).await {
            Ok(Some(job)) => {
                let job_id = job.id;
                let attempts = job.attempts;
                let max_attempts = job.max_attempts;
                let event = match jobs::postgres::decode(&job) {
                    Ok(e) => e,
                    Err(e) => {
                        // Payload doesn't match the registered kind —
                        // schema drift. Mark failed straight away so the
                        // worker doesn't loop on this row forever.
                        error!(%job_id, kind = %job.kind, error = %e, "decode failed");
                        let _ = jobs::postgres::mark_failed_or_retry(
                            &pool,
                            job_id,
                            max_attempts, // force terminal
                            max_attempts,
                            &format!("decode: {e}"),
                        )
                        .await;
                        continue;
                    }
                };

                debug!(%job_id, kind = event.kind(), attempt = attempts, "running");
                match jobs::handle(event, &deps).await {
                    Ok(()) => {
                        if let Err(e) = jobs::postgres::mark_done(&pool, job_id).await {
                            error!(%job_id, error = %e, "mark_done failed");
                        } else {
                            debug!(%job_id, "done");
                        }
                    }
                    Err(e) => {
                        warn!(%job_id, attempt = attempts, error = %e, "handler failed");
                        if let Err(e2) = jobs::postgres::mark_failed_or_retry(
                            &pool,
                            job_id,
                            attempts,
                            max_attempts,
                            &e.to_string(),
                        )
                        .await
                        {
                            error!(%job_id, error = %e2, "mark_failed_or_retry failed");
                        }
                    }
                }
            }
            Ok(None) => {
                // No work — sleep and re-poll.
                tokio::time::sleep(POLL_INTERVAL).await;
            }
            Err(e) => {
                error!(error = %e, "claim_one failed; sleeping before retry");
                tokio::time::sleep(POLL_INTERVAL).await;
            }
        }
    }
}
