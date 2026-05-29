//! Shared core for the ml-art API.
//!
//! Every route-group binary depends on this crate. Things that live here:
//!
//! - `config`   — env loading, single source of truth for `Config`
//! - `db`       — sqlx Postgres pool factory + connection helpers
//! - `auth`     — Clerk JWT verification + anonymous-cookie handling
//! - `embedder` — Jina HTTP client + Postgres-backed text-query cache
//! - `error`    — `ApiError` + RFC 7807 problem+json conversion
//! - `models`   — shared domain types serialised over the wire

pub mod artwork_embeddings;
pub mod auth;
pub mod config;
pub mod db;
pub mod embedder;
pub mod error;
pub mod geocoding;
pub mod images;
pub mod middleware;
pub mod models;
pub mod modifiers;
pub mod object_store;
pub mod telemetry;
