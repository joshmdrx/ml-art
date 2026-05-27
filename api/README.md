# api/ — Rust API

Cargo workspace. Each route group from `03-api-data-spec.md` ships as its own
Lambda binary; shared code (DB, types, embedder client, auth, error) lives in
the `core` crate.

## Layout

```
api/
├── Cargo.toml                 workspace manifest
├── rust-toolchain.toml        pinned 1.84.0
├── crates/
│   ├── core/                  shared: db, config, errors, embedder, auth
│   └── api-search/            binary: GET /v1/search and friends
│       └── (more route groups will land here: api-me, api-collections,
│            api-uploads, api-inquiries, api-studio, api-onboarding, api-events)
```

## Why one binary per route group

See `decisions.md` (2026-05-25 — Cargo workspace). Tradeoff is documented;
revisit if we discover the granularity is wrong.

## Running locally

```bash
# from repo root, with docker compose up:
cd api
export DATABASE_URL="postgres://ml_art:dev@localhost:5433/ml_art_dev"
cargo run -p api-search
```

The binary binds an Axum HTTP server in dev (port 9000), and the same handler
code runs as a Lambda in deployed environments via `lambda_http`.

## Running tests

```bash
cargo test --workspace
cargo clippy --workspace --all-targets -- -D warnings
cargo fmt --check
```

## Environment

See `.env.example` in the repo root for required variables. The API refuses
to start if any required var is missing in production but runs with permissive
defaults in dev (no Jina key → keyword-only search, no Mapbox key → geocoding
skipped, etc.). See `COST.md` for the philosophy.
