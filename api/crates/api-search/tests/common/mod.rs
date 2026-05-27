//! Shared helpers for api-search integration tests.
//!
//! Each `tests/*.rs` file compiles to its own crate, and any helper that
//! crate doesn't reference triggers `dead_code` (which clippy upgrades to
//! an error under `-D warnings`). Module-level allow keeps the helpers as
//! one cohesive surface without forcing each test file to import every
//! function. See `decisions.md` 2026-05-27 — `User` axum extractor for
//! the analogous "abstraction worth its weight" framing.
#![allow(dead_code)]

use api_search::{build_app, AppState};
use axum::{
    body::{to_bytes, Body},
    http::{Request, StatusCode},
    Router,
};
use ml_art_core::{
    auth::JwtVerifier, config::Config, db::Pool, embedder::Embedder, object_store::ObjectStore,
};
use serde::de::DeserializeOwned;
use sqlx::migrate::Migrator;
use std::sync::Arc;
use tower::ServiceExt;

/// The same migrations that production runs, mounted relative to this file.
/// Used by `#[sqlx::test(migrator = "common::MIGRATOR", fixtures("seed"))]`.
pub static MIGRATOR: Migrator = sqlx::migrate!("../../../db/migrations");

/// Build the Axum router with a disabled embedder. Use this for tests that
/// exercise the keyword-only path.
pub fn app_keyword_only(pool: Pool) -> Router {
    let cfg = Config::for_tests(String::new());
    let embedder = Embedder::disabled(pool.clone());
    // No JWKS / issuer in tests — JwtVerifier::enabled() returns false and
    // any call to `verify` returns Unauthorized. Endpoints that require
    // auth respond 401, which is what we want.
    let jwt_verifier = JwtVerifier::new(None, None, None);
    build_app(Arc::new(AppState {
        pool,
        embedder,
        jwt_verifier,
        cfg,
        object_store: ObjectStore::for_tests("uploads"),
    }))
}

/// Build the Axum router with the test-mode JwtVerifier. Tokens like
/// `Authorization: Bearer test-user_test_alice` are accepted and resolve
/// to whichever seeded user has that `clerk_user_id`.
pub fn app_with_test_auth(pool: Pool) -> Router {
    let cfg = Config::for_tests(String::new());
    let embedder = Embedder::disabled(pool.clone());
    let jwt_verifier = JwtVerifier::for_tests();
    build_app(Arc::new(AppState {
        pool,
        embedder,
        jwt_verifier,
        cfg,
        object_store: ObjectStore::for_tests("uploads"),
    }))
}

/// Send a GET with a Bearer token, parse JSON, return (status, body).
pub async fn get_json_authed<T: DeserializeOwned>(
    app: Router,
    uri: &str,
    bearer: &str,
) -> (StatusCode, T) {
    let resp = app
        .oneshot(
            Request::builder()
                .uri(uri)
                .header("Authorization", format!("Bearer {bearer}"))
                .body(Body::empty())
                .expect("build request"),
        )
        .await
        .expect("router oneshot");
    let status = resp.status();
    let bytes = to_bytes(resp.into_body(), 4 * 1024 * 1024)
        .await
        .expect("read body");
    let parsed: T = serde_json::from_slice(&bytes).unwrap_or_else(|e| {
        panic!(
            "decode {uri} (status {status}): {e}\nbody: {}",
            String::from_utf8_lossy(&bytes)
        )
    });
    (status, parsed)
}

/// Authed status-only variant — useful for write endpoints that return 204.
pub async fn send_authed(
    app: Router,
    method: &str,
    uri: &str,
    bearer: &str,
    body: Option<&str>,
) -> (StatusCode, Vec<u8>) {
    let mut req = Request::builder()
        .uri(uri)
        .method(method)
        .header("Authorization", format!("Bearer {bearer}"));
    let body_kind = if let Some(b) = body {
        req = req.header("Content-Type", "application/json");
        Body::from(b.to_string())
    } else {
        Body::empty()
    };
    let resp = app
        .oneshot(req.body(body_kind).expect("build request"))
        .await
        .expect("router oneshot");
    let status = resp.status();
    let bytes = to_bytes(resp.into_body(), 4 * 1024 * 1024)
        .await
        .expect("read body");
    (status, bytes.to_vec())
}

/// Build the Axum router with rate limiting ENABLED and the per-route
/// quotas dialed down for fast assertions. Test-mode auth verifier is in
/// place so Bearer tokens key per-user; absent Bearer the limiter keys
/// by `X-Anonymous-Id` (or `X-Forwarded-For`).
#[allow(dead_code)]
pub fn app_with_rate_limit(pool: Pool, search_per_min: u32, inquiry_per_hour: u32) -> Router {
    let mut cfg = Config::for_tests(String::new());
    cfg.rate_limit_disabled = false;
    cfg.rate_limit_search_per_min = search_per_min;
    cfg.rate_limit_inquiry_per_hour = inquiry_per_hour;
    let embedder = Embedder::disabled(pool.clone());
    let jwt_verifier = JwtVerifier::for_tests();
    build_app(Arc::new(AppState {
        pool,
        embedder,
        jwt_verifier,
        cfg,
        object_store: ObjectStore::for_tests("uploads"),
    }))
}

/// Build the Axum router with an embedder that returns a fixed vector for
/// every text query. Use this when the test needs the vector path to fire.
pub fn app_with_fixed_vector(pool: Pool, vec: pgvector::Vector) -> Router {
    let cfg = Config::for_tests(String::new());
    let embedder = Embedder::with_fixed_vector(
        pool.clone(),
        cfg.embedding_model_name.clone(),
        cfg.embedding_model_version.clone(),
        vec,
    );
    let jwt_verifier = JwtVerifier::new(None, None, None);
    build_app(Arc::new(AppState {
        pool,
        embedder,
        jwt_verifier,
        cfg,
        object_store: ObjectStore::for_tests("uploads"),
    }))
}

/// Build the Axum router with BOTH the fixed-vector embedder AND
/// `JwtVerifier::for_tests()`. Studio tests that exercise image-add
/// (which calls `process_image` inline) need the embedder enabled,
/// but those same endpoints require auth. The two flavors of
/// `app_with_*` helper aren't composable, so this is the third.
pub fn app_with_auth_and_fixed_vector(pool: Pool, vec: pgvector::Vector) -> Router {
    let cfg = Config::for_tests(String::new());
    let embedder = Embedder::with_fixed_vector(
        pool.clone(),
        cfg.embedding_model_name.clone(),
        cfg.embedding_model_version.clone(),
        vec,
    );
    let jwt_verifier = JwtVerifier::for_tests();
    build_app(Arc::new(AppState {
        pool,
        embedder,
        jwt_verifier,
        cfg,
        object_store: ObjectStore::for_tests("uploads"),
    }))
}

/// Build a standalone `Embedder` with `with_fixed_vector`. For pipeline
/// tests (`process_image`) that don't need the whole Axum router.
pub fn embedder_with_fixed_vector(pool: Pool, vec: pgvector::Vector) -> Embedder {
    let cfg = Config::for_tests(String::new());
    Embedder::with_fixed_vector(
        pool,
        cfg.embedding_model_name,
        cfg.embedding_model_version,
        vec,
    )
}

/// Convenience: send a GET, return the (status, parsed-JSON) tuple.
pub async fn get_json<T: DeserializeOwned>(app: Router, uri: &str) -> (StatusCode, T) {
    let resp = app
        .oneshot(
            Request::builder()
                .uri(uri)
                .body(Body::empty())
                .expect("build request"),
        )
        .await
        .expect("router oneshot");
    let status = resp.status();
    let bytes = to_bytes(resp.into_body(), 4 * 1024 * 1024)
        .await
        .expect("read body");
    let parsed: T = serde_json::from_slice(&bytes).unwrap_or_else(|e| {
        panic!(
            "decode {uri} (status {status}): {e}\nbody: {}",
            String::from_utf8_lossy(&bytes)
        )
    });
    (status, parsed)
}

/// Variant when the body can be either success JSON or an error JSON.
pub async fn get_status(app: Router, uri: &str) -> (StatusCode, Vec<u8>) {
    let resp = app
        .oneshot(
            Request::builder()
                .uri(uri)
                .body(Body::empty())
                .expect("build request"),
        )
        .await
        .expect("router oneshot");
    let status = resp.status();
    let bytes = to_bytes(resp.into_body(), 4 * 1024 * 1024)
        .await
        .expect("read body");
    (status, bytes.to_vec())
}

/// `get_status` plus a Bearer token. Used when an authed endpoint is
/// expected to fail (404 / 401) and we don't want to commit to a JSON
/// shape for the error body.
pub async fn get_status_authed(app: Router, uri: &str, bearer: &str) -> (StatusCode, Vec<u8>) {
    let resp = app
        .oneshot(
            Request::builder()
                .uri(uri)
                .header("Authorization", format!("Bearer {bearer}"))
                .body(Body::empty())
                .expect("build request"),
        )
        .await
        .expect("router oneshot");
    let status = resp.status();
    let bytes = to_bytes(resp.into_body(), 4 * 1024 * 1024)
        .await
        .expect("read body");
    (status, bytes.to_vec())
}
