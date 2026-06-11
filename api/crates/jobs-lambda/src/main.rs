//! SQS-triggered Lambda for background jobs. Counterpart to
//! `jobs-worker` (the local Postgres-polling loop). Both share the
//! same handler dispatch in `ml_art_core::jobs::handle`.
//!
//! Wiring (see `infra/modules/jobs/`):
//!
//!   SQS queue (`ml-art-prod-jobs`)
//!     │  event_source_mapping
//!     ▼
//!   this Lambda  ──┐
//!     │           │ failed records → BatchItemFailures
//!     │           ▼  (SQS keeps them visible; eventually → DLQ
//!     │              after `max_receive_count` retries)
//!     ▼
//!   core::jobs::handle  ──→ domain handlers
//!
//! Message body shape: the same JSON the `JobEvent` enum serializes
//! to, i.e. `{"kind": "<snake_case>", "payload": {...}}`. The API's
//! `JobsBackend::Sqs` driver (TBD — currently only `Postgres` exists)
//! is the producer side.
//!
//! Partial-batch failures: we return `SqsBatchResponse` with a list of
//! `batch_item_failures`. Lambda + SQS then re-deliver only those
//! specific records on the next poll. Without this, *any* failure in
//! a batch of 5 would re-deliver the whole batch (including the 4
//! successes), causing duplicate-side-effects.
//!
//! Cold-start init: dependencies are built once outside the handler
//! closure (Lambda keeps the container warm between invocations; the
//! init runs on the first invocation only). DB pool, HTTP clients,
//! moderation client all initialised here.

use aws_lambda_events::sqs::{BatchItemFailure, SqsBatchResponse, SqsEventObj, SqsMessageObj};
use lambda_runtime::{service_fn, Error, LambdaEvent};
use ml_art_core::{
    config::Config,
    emails::EmailClient,
    geocoding::GeocodingClient,
    jobs::{self, JobEvent, JobsDeps},
    moderation::ModerationClient,
};
use std::sync::Arc;
use tracing::{error, info, warn};

#[tokio::main]
async fn main() -> Result<(), Error> {
    // Lambda's default subscriber. CloudWatch consumes the JSON-line
    // output and parses level + fields; matches what `jobs-worker`
    // uses locally so log shapes are identical.
    tracing_subscriber::fmt()
        .with_env_filter(
            tracing_subscriber::EnvFilter::try_from_default_env()
                .unwrap_or_else(|_| tracing_subscriber::EnvFilter::new("info")),
        )
        .json()
        .with_target(false)
        .with_current_span(false)
        .without_time() // CloudWatch already timestamps each line
        .init();

    // SSM → env injection before Config::load reads anything. The
    // execution role has `ssm:GetParametersByPath` on this prefix
    // (set in modules/jobs/main.tf).
    if let Ok(prefix) = std::env::var("CONFIG_PARAMETER_PATH") {
        ml_art_core::config::bootstrap_ssm(&prefix).await?;
    }

    // Build deps ONCE — outside the handler closure — so the cost is
    // amortised across all invocations of a warm container. DB
    // connection setup is the most expensive piece (~200ms for the
    // first Postgres handshake over TLS).
    let cfg = Config::load()?;
    let pool = ml_art_core::db::make_pool(&cfg.database_url).await?;
    let deps = Arc::new(JobsDeps {
        pool,
        geocoder: GeocodingClient::from_env(),
        emails: EmailClient::from_env(),
        moderation: ModerationClient::from_env(),
        web_base_url: cfg.web_base_url.clone(),
    });

    info!("jobs-lambda init complete; entering handler loop");

    // service_fn is the Lambda-runtime adapter — turns a plain async
    // fn into a `tower::Service` it can drive.
    let handler = service_fn(move |event: LambdaEvent<SqsEventObj<serde_json::Value>>| {
        let deps = Arc::clone(&deps);
        async move { handle_batch(event, deps).await }
    });
    lambda_runtime::run(handler).await
}

/// Process one SQS batch. Each record is independent — we try them
/// all, and report the failures so SQS knows which to redeliver.
async fn handle_batch(
    event: LambdaEvent<SqsEventObj<serde_json::Value>>,
    deps: Arc<JobsDeps>,
) -> Result<SqsBatchResponse, Error> {
    let records = event.payload.records;
    let total = records.len();
    let mut failures: Vec<BatchItemFailure> = Vec::new();

    for record in records {
        match handle_record(&record, &deps).await {
            Ok(()) => {
                // success — SQS will delete the message
            }
            Err(e) => {
                // Two failure modes have different observability needs:
                //   - parse failures = `kind`/`payload` schema drift —
                //     warn, but still mark as a batch failure so SQS
                //     retries up to `max_receive_count`, then DLQs.
                //   - handler failures = transient (downstream API
                //     down, DB blip) — same: retry, then DLQ.
                let message_id = record.message_id.clone().unwrap_or_default();
                warn!(%message_id, error = %e, "record failed");
                failures.push(BatchItemFailure {
                    item_identifier: message_id,
                });
            }
        }
    }

    if !failures.is_empty() {
        info!(
            total = total,
            failed = failures.len(),
            "batch complete with partial failures"
        );
    }

    Ok(SqsBatchResponse {
        batch_item_failures: failures,
    })
}

/// Parse + dispatch one SQS record. Returns `Err` for either parse or
/// handler failure — the caller turns either into a batch-item failure.
async fn handle_record(
    record: &SqsMessageObj<serde_json::Value>,
    deps: &JobsDeps,
) -> Result<(), HandleError> {
    // `body` is the raw message JSON the producer enqueued. With
    // `SqsEventObj<serde_json::Value>` the runtime already parsed
    // the JSON wrapper, so `body` is a `serde_json::Value` — no
    // second parse step needed.
    let event: JobEvent = serde_json::from_value(record.body.clone())
        .map_err(|e| HandleError::Decode(e.to_string()))?;

    let kind = event.kind();
    let message_id = record.message_id.as_deref().unwrap_or("<no-id>");

    match jobs::handle(event, deps).await {
        Ok(()) => {
            tracing::debug!(%message_id, %kind, "ok");
            Ok(())
        }
        Err(e) => {
            error!(%message_id, %kind, error = %e, "handler error");
            Err(HandleError::Handler(e.to_string()))
        }
    }
}

#[derive(Debug, thiserror::Error)]
enum HandleError {
    #[error("decode failed: {0}")]
    Decode(String),
    #[error("handler failed: {0}")]
    Handler(String),
}
