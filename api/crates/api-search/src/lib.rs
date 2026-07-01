//! api-search library — exposes the route handlers and app builder so
//! integration tests can call them without binding a network port.

pub mod admin;
pub mod anon;
pub mod artist;
pub mod artwork;
pub mod calibrate;
pub mod events;
pub mod extractors;
pub mod inquiries;
pub mod map_cities;
pub mod me;
pub mod meta;
pub mod neighborhoods;
pub mod onboarding;
pub mod recommendations;
pub mod search;
pub mod search_map;
mod serde_helpers;
pub mod series;
pub mod studio;
pub mod testfixtures;
pub mod uploads;
pub mod webhooks;

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
    jobs::JobsBackend,
    middleware::{inquiry_limit, search_limit, RateLimiters},
    object_store::ObjectStore,
};
use std::sync::Arc;
use tower_http::{
    cors::{Any, CorsLayer},
    trace::TraceLayer,
};

#[derive(Clone)]
pub struct AppState {
    pub pool: Pool,
    pub embedder: Embedder,
    pub jwt_verifier: JwtVerifier,
    pub cfg: Config,
    /// Backend for the `uploads/` bucket. Real S3/MinIO in dev + prod;
    /// in-memory stub via `ObjectStore::for_tests` for integration tests.
    pub object_store: ObjectStore,
    /// Jobs queue. Studio handlers enqueue `JobEvent::*` events;
    /// `jobs-worker` (locally) or a Lambda (in prod) consumes them.
    /// See `core::jobs` and `decisions.md` 2026-05-29.
    pub jobs: JobsBackend,
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

    let router = Router::new()
        .route("/v1/health", get(health))
        .route(
            "/v1/search",
            get(search::handle).layer(from_fn_with_state(limiters.clone(), search_limit)),
        )
        // T-038 G5: sibling endpoint for the `/search?map=1` UI. Same
        // search-limit policy since it traverses the same filter shape.
        .route(
            "/v1/search/map",
            get(search_map::handle).layer(from_fn_with_state(limiters.clone(), search_limit)),
        )
        // T-042: top-cities aggregation. Powers the "where do I start?"
        // city-pivot pills on `/search?map=1`. Cheap GROUP BY query;
        // no rate-limit layer since it's a static, light call.
        .route("/v1/search/map/cities", get(map_cities::handle))
        .route("/v1/artists/:slug", get(artist::handle))
        // T-058 public series reads. The list endpoint hides empty
        // series; the detail returns 404 if the series doesn't exist
        // for the artist OR has no published artworks.
        .route("/v1/artists/:slug/series", get(series::list))
        .route("/v1/artists/:slug/series/:series_slug", get(series::detail))
        .route("/v1/artworks/:id", get(artwork::detail))
        .route("/v1/artworks/:id/similar", get(artwork::similar))
        .route("/v1/neighborhoods", get(neighborhoods::index))
        .route("/v1/neighborhoods/:slug", get(neighborhoods::detail))
        // T-061 first-session calibrator. GET samples pairs from
        // far-apart cluster centroids; POST logs the pick as a
        // `calibration_pick` event (T-055 picks it up at weight 2.0).
        .route("/v1/calibrate/pairs", get(calibrate::pairs))
        .route("/v1/calibrate/pick", post(calibrate::pick))
        // T-056.1 personalised retrieval. Requires sign-in. Returns
        // top-K artworks by nearest-to-`user_profiles.taste_embedding`
        // via HNSW, with a random shuffle on the candidate pool for
        // serendipity. Eligible iff `interaction_count >= 5` and the
        // taste vector is set; otherwise returns an empty `items` and
        // `eligible: false` so the web layer can render a fallback row.
        .route(
            "/v1/me/recommendations/for-you",
            get(recommendations::for_you),
        )
        .route("/v1/me", get(me::current_user))
        // T-033: called once after sign-in to copy behavioral signal
        // keyed on the anon_id cookie onto the now-known user. Body-
        // less + idempotent — the anon_id comes from `X-Anonymous-Id`.
        .route("/v1/me/merge-anonymous", post(me::merge_anonymous))
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
        // T-053 — public read by share token. No auth; 404 if private or
        // not found. The share token is the entire credential.
        .route(
            "/v1/collections/share/:share_id",
            get(me::collections::public_by_share),
        )
        // T-052 — follow graph. POST/DELETE are idempotent.
        .route("/v1/me/follows", get(me::follows::list))
        .route(
            "/v1/me/follows/:artist_id",
            post(me::follows::create).delete(me::follows::delete),
        )
        // T-068 — notification preferences. GET returns the full map
        // with defaults filled in; PATCH does sparse partial updates.
        .route(
            "/v1/me/notification-preferences",
            get(me::notifications::get).patch(me::notifications::patch),
        )
        // T-068 — public one-click unsubscribe. No auth — the signed
        // token in the request body is the credential. Two surfaces:
        // `/v1/notifications/unsubscribe` (web confirmation page POSTs
        // here, gets a friendly label back) and `/.../oneclick` (RFC
        // 8058 mail-client POST, returns 204).
        .route(
            "/v1/notifications/unsubscribe",
            post(me::notifications::unsubscribe),
        )
        .route(
            "/v1/notifications/unsubscribe/oneclick",
            post(me::notifications::unsubscribe_oneclick),
        )
        // T-052c — anon-side intent queue. POST is keyed on the
        // signed `X-Anonymous-Id` header; the merge-anonymous
        // handler drains + replays on sign-in.
        .route(
            "/v1/anon/pending/follows/:artist_id",
            post(anon::queue_follow),
        )
        .route(
            "/v1/artworks/:id/inquiries",
            post(inquiries::create).layer(from_fn_with_state(limiters.clone(), inquiry_limit)),
        )
        .route("/v1/inquiries/verify/:token", get(inquiries::verify))
        // T-054 — inbound-email webhook for email-stitched inquiry
        // threads. Unauthenticated (no user session); the Cloudflare
        // Email Worker authenticates with a shared-secret header. See
        // `webhooks::inbound_email`.
        .route("/v1/webhooks/email/inbound", post(webhooks::inbound_email))
        // T-050.3 — batched client-side events. Rate-limited by the
        // same per-anon search-tier policy (browsing intensity tracks
        // event volume well). Server derives identity from the
        // cookie + auth; client-supplied values are ignored.
        .route(
            "/v1/events",
            post(events::ingest).layer(from_fn_with_state(limiters.clone(), search_limit)),
        )
        // ── Onboarding (T-012 Phase 1). Mints + publishes an artist
        // row for the calling user. Subsequent edits go through the
        // existing /v1/studio/* surfaces.
        .route("/v1/onboarding/start", post(onboarding::start))
        .route("/v1/onboarding/complete", post(onboarding::complete))
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
        // ── Studio series (T-058). Artist-curated groupings of their
        // own works. Multi-select membership via PUT (replace
        // semantics). Single-artwork series assignment also possible
        // via the artwork PATCH endpoint.
        .route(
            "/v1/studio/series",
            get(studio::series::list).post(studio::series::create),
        )
        .route(
            "/v1/studio/series/:id",
            get(studio::series::detail)
                .patch(studio::series::patch)
                .delete(studio::series::delete),
        )
        .route(
            "/v1/studio/series/:id/artworks",
            axum::routing::put(studio::series::set_artworks),
        )
        // ── Studio inquiries inbox (T-011 Phase 4 + 4b). List the
        // calling artist's inquiries; reply to one; bulk-mark a batch
        // as read (auto-fired by the inbox page on view).
        .route("/v1/studio/inquiries", get(studio::inquiries::list))
        .route(
            "/v1/studio/inquiries/:id/reply",
            post(studio::inquiries::reply),
        )
        .route(
            "/v1/studio/inquiries/read",
            post(studio::inquiries::mark_read),
        )
        // ── Studio locations (T-038 G3): galleries / studios where
        // this artist's work can be seen in person. Public listing on
        // the artist profile only includes geocoded rows; the studio
        // sees all of them, including "Locating…" placeholders.
        .route(
            "/v1/studio/locations",
            get(studio::locations::list).post(studio::locations::create),
        )
        .route(
            "/v1/studio/locations/:id",
            axum::routing::patch(studio::locations::patch).delete(studio::locations::delete),
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
        // ── Metadata (cheap, static). T-010 Phase C exposes the
        // modifier registry so the search-page button row can render
        // without hardcoding labels client-side.
        .route("/v1/modifiers", get(meta::list_modifiers))
        // ── Admin surface (T-083). All endpoints gated by `AdminUser`
        // (extractor returns 403 for non-admins). Every mutation
        // writes one `admin_audit_log` row before applying.
        .route("/v1/admin/artists", get(admin::artists::list))
        .route(
            "/v1/admin/artists/:id/approve",
            post(admin::artists::approve),
        )
        .route(
            "/v1/admin/artists/:id/decline",
            post(admin::artists::decline),
        )
        .route("/v1/admin/artists/:id/pause", post(admin::artists::pause))
        .route(
            "/v1/admin/artists/:id/unpause",
            post(admin::artists::unpause),
        )
        // T-083.3 — image moderation override queue. Auto-moderator
        // (T-008) writes `moderation_status='rejected'` rows; the
        // admin reviews them here and can flip back to approved.
        .route("/v1/admin/images", get(admin::images::list))
        .route(
            "/v1/admin/images/:id/override",
            post(admin::images::override_approve),
        )
        // T-083.5 — read-only audit log viewer.
        .route("/v1/admin/audit-log", get(admin::audit_log::list))
        // T-084.1 — operator stats page. One-round-trip aggregate
        // over users / artists / artworks / inquiries + a 4-week
        // events funnel + recent admin activity.
        .route("/v1/admin/stats", get(admin::stats::handle));

    // Test-fixture insert routes — only registered when
    // `WANDER_TEST_FIXTURES_ENABLED` is set (E2E only; empty in prod).
    // See `testfixtures.rs` for the guarded shape.
    let router = if testfixtures::is_enabled() {
        router.merge(testfixtures::routes())
    } else {
        router
    };

    router
        .with_state(state)
        .layer(TraceLayer::new_for_http())
        // CORS: browser-direct calls from the Next dev server / future web
        // origin. `searchMapClient` hits `/v1/search/map` straight from
        // the browser (no server-side proxy) — without this layer the
        // browser blocks the response and the client gets "Failed to
        // fetch." Permissive in v1: any origin can read; no credentialed
        // cross-origin requests so this is safe. Tighten to an
        // allowlist (`AllowOrigin::list(...)`) when prod origins are
        // pinned.
        .layer(
            CorsLayer::new()
                .allow_origin(Any)
                .allow_methods(Any)
                .allow_headers(Any),
        )
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
