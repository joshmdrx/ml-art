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
    let jobs = JobsBackend::postgres(pool.clone());
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
