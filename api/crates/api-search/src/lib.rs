//! api-search library — exposes the route handlers and app builder so
//! integration tests can call them without binding a network port.

pub mod artist;
pub mod artwork;
pub mod extractors;
pub mod inquiries;
pub mod me;
pub mod neighborhoods;
pub mod search;
pub mod studio;
pub mod uploads;

use axum::{
    extract::State,
    middleware::from_fn_with_state,
    routing::{delete, get, post},
    Json, Router,
};
use ml_art_core::{
    auth::JwtVerifier,
    config::Config,
    db::Pool,
    embedder::Embedder,
    error::ApiError,
    middleware::{inquiry_limit, search_limit, RateLimiters},
    object_store::ObjectStore,
};
use std::sync::Arc;
use tower_http::trace::TraceLayer;

#[derive(Clone)]
pub struct AppState {
    pub pool: Pool,
    pub embedder: Embedder,
    pub jwt_verifier: JwtVerifier,
    pub cfg: Config,
    /// Backend for the `uploads/` bucket. Real S3/MinIO in dev + prod;
    /// in-memory stub via `ObjectStore::for_tests` for integration tests.
    pub object_store: ObjectStore,
}

/// Build the full Axum router for this binary. Used by both the runtime
/// entry point (`main`) and integration tests.
pub fn build_app(state: Arc<AppState>) -> Router {
    // Per-policy keyed rate limiters. Shared across all routes that opt
    // into a given policy. `Config::rate_limit_disabled` short-circuits
    // every check — set in `for_tests` so the broad suite isn't rebuilt.
    //
    // The limiters are attached at the *MethodRouter* level so we get
    // exactly the routes we want without `route_layer`'s "applies to
    // all currently-registered routes" footgun.
    let limiters = RateLimiters::new(
        state.cfg.rate_limit_search_per_min,
        state.cfg.rate_limit_inquiry_per_hour,
        state.cfg.rate_limit_disabled,
    );

    Router::new()
        .route("/v1/health", get(health))
        .route(
            "/v1/search",
            get(search::handle).layer(from_fn_with_state(limiters.clone(), search_limit)),
        )
        .route("/v1/artists/:slug", get(artist::handle))
        .route("/v1/artworks/:id", get(artwork::detail))
        .route("/v1/artworks/:id/similar", get(artwork::similar))
        .route("/v1/neighborhoods", get(neighborhoods::index))
        .route("/v1/neighborhoods/:slug", get(neighborhoods::detail))
        .route("/v1/me", get(me::current_user))
        .route(
            "/v1/me/collections",
            get(me::collections::list).post(me::collections::create),
        )
        .route(
            "/v1/me/collections/:id",
            get(me::collections::detail)
                .patch(me::collections::patch)
                .delete(me::collections::delete),
        )
        .route(
            "/v1/me/collections/:id/artworks",
            post(me::collections::add_artwork),
        )
        .route(
            "/v1/me/collections/:id/artworks/:artwork_id",
            delete(me::collections::remove_artwork),
        )
        .route(
            "/v1/artworks/:id/inquiries",
            post(inquiries::create).layer(from_fn_with_state(limiters.clone(), inquiry_limit)),
        )
        .route("/v1/inquiries/verify/:token", get(inquiries::verify))
        // ── Studio (artist-only authed surface). T-011.
        .route("/v1/studio/me", get(studio::me::current_artist))
        .route(
            "/v1/studio/artworks",
            get(studio::artworks::list).post(studio::artworks::create),
        )
        .route(
            "/v1/studio/artworks/:id",
            get(studio::artworks::detail)
                .patch(studio::artworks::patch)
                .delete(studio::artworks::delete),
        )
        .route(
            "/v1/studio/artworks/:id/images",
            post(studio::artworks::add_image),
        )
        .route(
            "/v1/studio/artworks/:id/images/:image_id",
            delete(studio::artworks::remove_image),
        )
        .route(
            "/v1/studio/settings",
            axum::routing::patch(studio::settings::patch),
        )
        // ── Uploads (visual-search entry point). T-010 Phase A.
        // Limit per `03-api-data-spec.md`: 20/hr per key. Reuses the
        // inquiry-limit policy since both are write-heavy + per-user
        // (a separate `uploads_limit` policy + Config knob lands when
        // we have signal that 20/hr is the wrong shape).
        .route(
            "/v1/uploads/image",
            post(uploads::create).layer(from_fn_with_state(limiters.clone(), inquiry_limit)),
        )
        .with_state(state)
        .layer(TraceLayer::new_for_http())
}

pub async fn health(
    State(state): State<Arc<AppState>>,
) -> Result<Json<serde_json::Value>, ApiError> {
    let (one,): (i32,) = sqlx::query_as("SELECT 1").fetch_one(&state.pool).await?;
    Ok(Json(serde_json::json!({
        "status": "ok",
        "db": one == 1,
        "embedder_enabled": state.embedder.enabled(),
        "auth_enabled": state.jwt_verifier.enabled(),
        "env": format!("{:?}", state.cfg.env),
    })))
}
