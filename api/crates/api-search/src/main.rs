//! api-search binary entry point. All real logic lives in `lib.rs` so it
//! can be exercised by integration tests.

use api_search::{build_app, AppState};
use ml_art_core::{
    auth::JwtVerifier, config::Config, embedder::Embedder, jobs::JobsBackend,
    object_store::ObjectStore,
};
use std::sync::Arc;

#[tokio::main]
async fn main() -> anyhow::Result<()> {
    ml_art_core::telemetry::init();
    let cfg = Config::load()?;
    let pool = ml_art_core::db::make_pool(&cfg.database_url).await?;
    let embedder = Embedder::new(
        pool.clone(),
        cfg.jina_api_key.clone(),
        cfg.embedding_model_name.clone(),
        cfg.embedding_model_version.clone(),
    );
    let jwt_verifier = JwtVerifier::new(
        cfg.clerk_issuer.clone(),
        cfg.clerk_jwks_url.clone(),
        cfg.clerk_secret_key.clone(),
    );
    let object_store = ObjectStore::new(
        cfg.uploads_bucket.clone(),
        cfg.uploads_public_url_prefix.clone(),
        cfg.s3_endpoint_url.clone(),
        cfg.s3_region.clone(),
        cfg.s3_access_key.clone(),
        cfg.s3_secret_key.clone(),
    )
    .await;

    // JobsBackend driver selection: prod gets SQS (env var set by TF,
    // see infra/modules/api/), dev gets Postgres (jobs-worker polls).
    let jobs = match cfg.jobs_queue_url.as_deref() {
        Some(queue_url) => {
            tracing::info!(queue_url, "jobs backend: sqs");
            // BehaviorVersion is pinned by aws-config's
            // `behavior-version-latest` feature in workspace deps.
            let aws_cfg = aws_config::load_from_env().await;
            let sqs_client = aws_sdk_sqs::Client::new(&aws_cfg);
            JobsBackend::sqs(sqs_client, queue_url.to_string())
        }
        None => {
            tracing::info!("jobs backend: postgres (local jobs-worker should be running)");
            JobsBackend::postgres(pool.clone())
        }
    };
    let state = Arc::new(AppState {
        pool,
        embedder,
        jwt_verifier,
        cfg: cfg.clone(),
        object_store,
        jobs,
    });
    let app = build_app(state);

    if std::env::var("AWS_LAMBDA_RUNTIME_API").is_ok() {
        lambda_http::run(app)
            .await
            .map_err(|e| anyhow::anyhow!(e.to_string()))?;
    } else {
        let addr = format!("0.0.0.0:{}", cfg.port);
        tracing::info!("api-search listening on {addr}");
        let listener = tokio::net::TcpListener::bind(&addr).await?;
        axum::serve(listener, app).await?;
    }
    Ok(())
}
