//! Jobs queue — driver-agnostic background work.
//!
//! Two drivers, identical handler code:
//!
//! - **Local dev**: `JobsBackend::Postgres` writes to the `jobs` table
//!   (migration `0012_jobs.sql`). A sibling worker binary
//!   (`api/crates/jobs-worker`) polls with `SELECT … FOR UPDATE SKIP
//!   LOCKED` and runs the handler.
//! - **Prod**: `JobsBackend::Sqs` (deferred until we deploy) sends an
//!   SQS message; a `cargo-lambda` binary triggered on SQS receive
//!   runs the same handler.
//!
//! Why same enum shape as `ObjectStore` / `GeocodingClient`: it's
//! the pattern we already use in this crate. No new dep
//! (`async-trait`), no `dyn` plumbing, and the variant set is sealed
//! (we won't grow past Postgres + SQS).
//!
//! Why this exists at all: the v1 jobs surface (geocoding now,
//! T-032 email + T-008 moderation + T-033 anon-merge next) is too
//! big to keep on `tokio::spawn` — those die on api restart and have
//! no retries. See `decisions.md` 2026-05-29 — jobs queue:
//! Postgres local, SQS+Lambda prod.

use crate::db::Pool;
use serde::{Deserialize, Serialize};
use std::sync::Arc;
use std::time::Duration;
use uuid::Uuid;

/// Every kind of background work the system can enqueue. Tagged by
/// `kind` so the on-the-wire shape works for both `jobs.payload`
/// (jsonb) and SQS message bodies.
///
/// Add a new variant + a match arm in `dispatch::handle` to ship a
/// new background job. Handlers live in their domain modules
/// (`core::geocoding`, future `core::emails`, etc.) — this enum is
/// just the dispatch table.
#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
#[serde(tag = "kind", content = "payload", rename_all = "snake_case")]
pub enum JobEvent {
    /// Forward-geocode a single `artist_locations` row. Was
    /// `trigger_background_geocode`'s `tokio::spawn` (T-038).
    ArtistLocationGeocode { location_id: Uuid },
}

impl JobEvent {
    /// Discriminator used by the postgres `jobs.kind` column + SQS
    /// message attribute. Mirrors the `#[serde(tag = "kind")]` value
    /// — kept as an inherent fn so handlers can match without
    /// re-serializing.
    pub fn kind(&self) -> &'static str {
        match self {
            JobEvent::ArtistLocationGeocode { .. } => "artist_location_geocode",
        }
    }
}

/// Options for `enqueue` — added per call site, not on the event
/// itself (the same event might be enqueued with different
/// dedup / retry policies depending on context).
#[derive(Debug, Clone, Default)]
pub struct EnqueueOpts {
    /// When set, an insert with the same key returns success without
    /// creating a duplicate row. Use case: "geocode this row" should
    /// be a no-op if there's already a pending job for it.
    pub idempotency_key: Option<String>,
    /// Override the per-job retry limit. Defaults to the column
    /// default (5).
    pub max_attempts: Option<i32>,
}

/// Job-queue driver. Real production path will be `Sqs` once we
/// deploy; today only `Postgres` exists.
#[derive(Clone)]
pub struct JobsBackend {
    inner: Arc<Inner>,
}

enum Inner {
    Postgres {
        pool: Pool,
    },
    /// In-memory backend for tests. Captures enqueued events in an
    /// `Arc<Mutex<Vec>>` so assertions can read what was enqueued
    /// without touching Postgres.
    Memory {
        captured: std::sync::Mutex<Vec<JobEvent>>,
    },
}

impl JobsBackend {
    pub fn postgres(pool: Pool) -> Self {
        Self {
            inner: Arc::new(Inner::Postgres { pool }),
        }
    }

    pub fn for_tests() -> Self {
        Self {
            inner: Arc::new(Inner::Memory {
                captured: std::sync::Mutex::new(Vec::new()),
            }),
        }
    }

    /// Test-only: pull the list of events captured so far. Returns a
    /// fresh `Vec` (so callers can assert on length without holding
    /// the lock).
    pub fn captured(&self) -> Vec<JobEvent> {
        match &*self.inner {
            Inner::Memory { captured } => captured.lock().unwrap().clone(),
            Inner::Postgres { .. } => {
                panic!("captured() is only meaningful on for_tests() backends")
            }
        }
    }

    /// Enqueue a job for background processing. Returns `Ok(())` for
    /// success-or-idempotent-skip — the caller doesn't need to
    /// distinguish "new job created" from "duplicate suppressed."
    pub async fn enqueue(&self, event: JobEvent, opts: EnqueueOpts) -> Result<(), JobsError> {
        match &*self.inner {
            Inner::Memory { captured } => {
                captured.lock().unwrap().push(event);
                Ok(())
            }
            Inner::Postgres { pool } => postgres::enqueue(pool, event, opts).await,
        }
    }
}

#[derive(Debug, thiserror::Error)]
pub enum JobsError {
    #[error("database error: {0}")]
    Db(#[from] sqlx::Error),
    #[error("payload serialization failed: {0}")]
    Serialize(#[from] serde_json::Error),
}

// ─────────────────────────────────────────────────────────────────────────────
// Postgres driver
// ─────────────────────────────────────────────────────────────────────────────

pub mod postgres {
    //! Postgres-backed jobs driver. Used in dev + the local worker
    //! binary. In prod the same handler dispatch runs from a
    //! Lambda binary fed by SQS — different driver, same handlers.

    use super::{EnqueueOpts, JobEvent, JobsError};
    use crate::db::Pool;
    use chrono::{DateTime, Utc};
    use sqlx::FromRow;
    use std::time::Duration;
    use uuid::Uuid;

    /// One row as fetched off the queue. The worker uses these
    /// fields directly; callers outside the worker shouldn't need
    /// them.
    #[derive(Debug, FromRow)]
    pub struct PendingJob {
        pub id: Uuid,
        pub kind: String,
        pub payload: serde_json::Value,
        pub attempts: i32,
        pub max_attempts: i32,
    }

    pub async fn enqueue(pool: &Pool, event: JobEvent, opts: EnqueueOpts) -> Result<(), JobsError> {
        let kind = event.kind();
        // Strip the tag from the JSON shape — the column already
        // stores `kind` separately, no need to duplicate inside the
        // jsonb payload. We serialize with the tag, then peel it
        // off.
        let value = serde_json::to_value(&event)?;
        let payload = value
            .get("payload")
            .cloned()
            .unwrap_or(serde_json::Value::Null);

        let max_attempts = opts.max_attempts.unwrap_or(5);

        // `ON CONFLICT DO NOTHING` is the idempotency hook: when a
        // caller passes `idempotency_key`, a second insert with the
        // same key is a silent no-op.
        sqlx::query(
            r#"
            INSERT INTO jobs (kind, payload, idempotency_key, max_attempts)
            VALUES ($1, $2, $3, $4)
            ON CONFLICT (idempotency_key) DO NOTHING
            "#,
        )
        .bind(kind)
        .bind(payload)
        .bind(opts.idempotency_key)
        .bind(max_attempts)
        .execute(pool)
        .await?;
        Ok(())
    }

    /// Atomically claim one ready job. Uses `FOR UPDATE SKIP LOCKED`
    /// in a sub-select so multiple workers can run concurrently
    /// without grabbing the same row. Single-statement (UPDATE …
    /// RETURNING) so the row returned reflects the post-increment
    /// state — callers can safely use `attempts` to decide whether
    /// to retry or fail.
    ///
    /// Returns `None` when there's nothing pending — the worker
    /// sleeps and tries again.
    pub async fn claim_one(pool: &Pool) -> Result<Option<PendingJob>, JobsError> {
        let row: Option<PendingJob> = sqlx::query_as(
            r#"
            UPDATE jobs
            SET status     = 'running',
                attempts   = attempts + 1,
                updated_at = now()
            WHERE id = (
                SELECT id FROM jobs
                WHERE status = 'pending'
                  AND next_run_at <= now()
                ORDER BY next_run_at
                FOR UPDATE SKIP LOCKED
                LIMIT 1
            )
            RETURNING id, kind, payload, attempts, max_attempts
            "#,
        )
        .fetch_optional(pool)
        .await?;
        Ok(row)
    }

    pub async fn mark_done(pool: &Pool, id: Uuid) -> Result<(), JobsError> {
        sqlx::query(
            r#"
            UPDATE jobs
            SET status = 'done',
                completed_at = now(),
                updated_at = now()
            WHERE id = $1
            "#,
        )
        .bind(id)
        .execute(pool)
        .await?;
        Ok(())
    }

    /// Either schedule a retry (with exponential backoff) or mark
    /// the job as permanently failed. Decision is based on the
    /// current `attempts` (which the claim step has already
    /// incremented) vs `max_attempts`.
    pub async fn mark_failed_or_retry(
        pool: &Pool,
        id: Uuid,
        attempts: i32,
        max_attempts: i32,
        error_message: &str,
    ) -> Result<(), JobsError> {
        // Truncate to keep the column from growing unbounded.
        let truncated: String = error_message.chars().take(2000).collect();

        if attempts >= max_attempts {
            sqlx::query(
                r#"
                UPDATE jobs
                SET status = 'failed',
                    last_error = $2,
                    updated_at = now()
                WHERE id = $1
                "#,
            )
            .bind(id)
            .bind(truncated)
            .execute(pool)
            .await?;
        } else {
            // Exponential backoff capped at 1h: 2s, 8s, 32s, 128s, 512s, 1h, …
            // Formula: 2 ^ (attempts * 2) seconds, capped at 3600.
            let secs = 2u64.saturating_pow((attempts as u32).saturating_mul(2));
            let backoff = Duration::from_secs(secs.min(3600));
            let next: DateTime<Utc> =
                Utc::now() + chrono::Duration::from_std(backoff).unwrap_or_default();

            sqlx::query(
                r#"
                UPDATE jobs
                SET status = 'pending',
                    next_run_at = $2,
                    last_error = $3,
                    updated_at = now()
                WHERE id = $1
                "#,
            )
            .bind(id)
            .bind(next)
            .bind(truncated)
            .execute(pool)
            .await?;
        }
        Ok(())
    }

    /// Decode a fetched job back into the strongly-typed JobEvent.
    /// Returns an error if the payload doesn't match the kind — that
    /// would indicate a schema drift between enqueue + worker.
    pub fn decode(job: &PendingJob) -> Result<JobEvent, JobsError> {
        // Reassemble the tagged JSON the enum expects.
        let value = serde_json::json!({
            "kind": job.kind,
            "payload": job.payload,
        });
        let event: JobEvent = serde_json::from_value(value)?;
        Ok(event)
    }
}

// ─────────────────────────────────────────────────────────────────────────────
// Handler dispatch
// ─────────────────────────────────────────────────────────────────────────────

/// Dependencies the handler functions need. Built once on worker
/// startup. Analogous to `AppState` but minimal — only what
/// background jobs touch.
#[derive(Clone)]
pub struct JobsDeps {
    pub pool: Pool,
    pub geocoder: crate::geocoding::GeocodingClient,
}

/// The dispatch fn — same code path runs from the Postgres worker
/// poll loop today and the Lambda SQS handler tomorrow. Every new
/// `JobEvent` variant adds a match arm here that delegates to its
/// domain module.
pub async fn handle(event: JobEvent, deps: &JobsDeps) -> Result<(), HandlerError> {
    match event {
        JobEvent::ArtistLocationGeocode { location_id } => {
            crate::geocoding::geocode_and_update(&deps.geocoder, &deps.pool, location_id)
                .await
                .map_err(|e| HandlerError::Domain(e.to_string()))?;
        }
    }
    Ok(())
}

#[derive(Debug, thiserror::Error)]
pub enum HandlerError {
    #[error("{0}")]
    Domain(String),
}

// Re-export Duration so the worker binary doesn't need a top-level
// `std::time::Duration` import alongside `core::jobs`.
pub use std::time::Duration as PollDuration;

// `Duration` is referenced by docs; suppress the unused-import warning
// in production builds while keeping the visible re-export.
#[allow(dead_code)]
fn _duration_anchor(_: Duration) {}

// ─────────────────────────────────────────────────────────────────────────────
// Tests
// ─────────────────────────────────────────────────────────────────────────────

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn job_event_serialization_round_trip() {
        let id = Uuid::new_v4();
        let evt = JobEvent::ArtistLocationGeocode { location_id: id };
        let json = serde_json::to_value(&evt).unwrap();
        // The on-wire shape is the tagged form — `kind` + `payload`.
        assert_eq!(json["kind"], "artist_location_geocode");
        assert_eq!(json["payload"]["location_id"], id.to_string());

        // Round-trip.
        let back: JobEvent = serde_json::from_value(json).unwrap();
        assert_eq!(back, evt);
    }

    #[test]
    fn job_event_kind_matches_discriminator() {
        let evt = JobEvent::ArtistLocationGeocode {
            location_id: Uuid::new_v4(),
        };
        assert_eq!(evt.kind(), "artist_location_geocode");
    }

    #[tokio::test]
    async fn for_tests_backend_captures_enqueues() {
        let backend = JobsBackend::for_tests();
        let id = Uuid::new_v4();
        backend
            .enqueue(
                JobEvent::ArtistLocationGeocode { location_id: id },
                EnqueueOpts::default(),
            )
            .await
            .unwrap();
        let captured = backend.captured();
        assert_eq!(captured.len(), 1);
        assert_eq!(
            captured[0],
            JobEvent::ArtistLocationGeocode { location_id: id }
        );
    }

    #[test]
    fn decode_reconstructs_event_from_pending_row() {
        let id = Uuid::new_v4();
        let job = postgres::PendingJob {
            id: Uuid::new_v4(),
            kind: "artist_location_geocode".to_string(),
            payload: serde_json::json!({ "location_id": id }),
            attempts: 1,
            max_attempts: 5,
        };
        let evt = postgres::decode(&job).unwrap();
        assert_eq!(evt, JobEvent::ArtistLocationGeocode { location_id: id });
    }
}
