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
//!
//! **Adding a new job kind?** See `CONTRIBUTING.md` →
//! "Background jobs — Adding a new background job — checklist".
//! Four steps: new `JobEvent` variant + handler in domain module +
//! match arm in `handle()` below + `enqueue` at the call site.
//! Existing examples: `ArtistLocationGeocode` (T-038),
//! `InquirySendVerification` / `InquiryDeliverToArtist` (T-032).

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
    /// Email the inquirer a confirm-your-email link. Fired by the
    /// anonymous-inquiry create path. T-032.
    InquirySendVerification { inquiry_id: Uuid },
    /// Email the artist that they have a new inquiry. Fired when
    /// `delivered_at` flips — on signed-in inquiry create AND on
    /// the verify endpoint for anonymous inquiries. T-032.
    InquiryDeliverToArtist { inquiry_id: Uuid },
    /// Email the inquirer the artist's reply. Fired from
    /// `POST /v1/studio/inquiries/:id/reply` after the row is
    /// persisted. Idempotent on `reply_id` so duplicates don't
    /// double-send. T-011 Phase 4b.
    InquirySendReply { reply_id: Uuid },
    /// Forward an inquirer's inbound reply on to the artist. Fired by
    /// the inbound-email webhook after persisting an
    /// `inquiry_replies` row with `from_role='inquirer'`. Idempotent
    /// on `inquiry_replies.sent_at`. T-054.
    InquirySendReplyForward { reply_id: Uuid },
    /// Run the moderation client against a freshly-added artwork
    /// image. Writes back `artwork_images.moderation_status`. T-008.
    ArtworkImageModerate { artwork_image_id: Uuid },
    /// Run the moderation client against a freshly-PUT visual-search
    /// upload. Writes back `uploads.moderation_status`. T-008b.
    UploadModerate { upload_id: Uuid },
    /// T-052b — daily kickoff for the new-works-from-followed digest.
    /// Fired by EventBridge at 11:00 UTC (prod) or manually via
    /// `jobs-worker --enqueue` (local). Handler scans for users with
    /// new work from followed artists and fans out one
    /// `NotifyFollowersDigestUser` per qualifying user.
    NotifyFollowersDigestKickoff {},
    /// T-052b — build + send a single user's digest. Idempotent via
    /// the `user_notification_log` PK (user_id, kind, sent_on::date).
    NotifyFollowersDigestUser { user_id: Uuid },
    /// T-050 — append a row to the `events` table. The storage
    /// destination (Postgres now, S3 Parquet in the cold tier later
    /// per `decisions.md` 2026-06-17) lives behind `handle`; call
    /// sites only ever know this variant. Identity attribution:
    /// `anonymous_id` set for anon callers, `user_id` for signed-in.
    /// Both can be `None` for system-emitted events but the table
    /// allows the unattributed row.
    EventLog {
        name: crate::events::EventName,
        anonymous_id: Option<Uuid>,
        user_id: Option<Uuid>,
        properties: serde_json::Value,
        context: serde_json::Value,
    },
    /// T-055 — refresh one user's taste vector. Handler reads recent
    /// events, joins to artwork embeddings, writes the weighted+decayed
    /// sum to `user_profiles.taste_embedding`. Enqueued by the
    /// (currently dormant) scheduled trigger that scans for users with
    /// activity in the last 24h.
    UserProfileRefresh { user_id: Uuid },
    /// T-055.2 — fan-out kickoff. Handler scans for users with recent
    /// activity and enqueues one `UserProfileRefresh` per. Mirrors the
    /// digest kickoff pattern (EventBridge → SQS in prod, manual via
    /// `jobs-worker --enqueue` locally). Currently not on a live cron
    /// — the trigger code is shipped, the schedule wiring is deferred
    /// until real users exist.
    UserProfileRefreshKickoff {},
    /// T-080 — refresh FX rates against GBP from the Frankfurter API,
    /// then bulk-recompute `artworks.price_gbp_cents` so the search
    /// price filter operates on current values. Cron not live yet;
    /// manual invoke via `jobs-worker --enqueue` until artists onboard.
    FxRatesRefresh {},
}

impl JobEvent {
    /// Discriminator used by the postgres `jobs.kind` column + SQS
    /// message attribute. Mirrors the `#[serde(tag = "kind")]` value
    /// — kept as an inherent fn so handlers can match without
    /// re-serializing.
    pub fn kind(&self) -> &'static str {
        match self {
            JobEvent::ArtistLocationGeocode { .. } => "artist_location_geocode",
            JobEvent::InquirySendVerification { .. } => "inquiry_send_verification",
            JobEvent::InquiryDeliverToArtist { .. } => "inquiry_deliver_to_artist",
            JobEvent::InquirySendReply { .. } => "inquiry_send_reply",
            JobEvent::InquirySendReplyForward { .. } => "inquiry_send_reply_forward",
            JobEvent::ArtworkImageModerate { .. } => "artwork_image_moderate",
            JobEvent::UploadModerate { .. } => "upload_moderate",
            JobEvent::NotifyFollowersDigestKickoff {} => "notify_followers_digest_kickoff",
            JobEvent::NotifyFollowersDigestUser { .. } => "notify_followers_digest_user",
            JobEvent::EventLog { .. } => "event_log",
            JobEvent::UserProfileRefresh { .. } => "user_profile_refresh",
            JobEvent::UserProfileRefreshKickoff {} => "user_profile_refresh_kickoff",
            JobEvent::FxRatesRefresh {} => "fx_rates_refresh",
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

/// Job-queue driver. Picked at boot based on whether
/// `JOBS_QUEUE_URL` is set (see `api-search`'s `main.rs`):
///   - Set    → `Sqs`      (prod — Lambda + SQS + jobs-lambda)
///   - Unset  → `Postgres` (local dev — jobs table + jobs-worker)
///
/// Same `JobEvent` on the wire either way. The only side that varies
/// is enqueue (DB INSERT vs SQS SendMessage); the handler dispatch
/// in `handle()` is driver-agnostic.
#[derive(Clone)]
pub struct JobsBackend {
    inner: Arc<Inner>,
}

enum Inner {
    Postgres {
        pool: Pool,
    },
    /// Production driver — SendMessage to an SQS queue. The receive
    /// side lives in `api/crates/jobs-lambda` (SQS event-source mapping).
    Sqs {
        client: aws_sdk_sqs::Client,
        queue_url: String,
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

    /// SQS driver. `queue_url` is the full https URL from
    /// `terraform output jobs_queue_url` (e.g.
    /// `https://sqs.us-east-1.amazonaws.com/<account>/ml-art-prod-jobs`).
    /// The client should be constructed via `aws_config::load_from_env()`
    /// at boot — credentials come from the Lambda execution role.
    pub fn sqs(client: aws_sdk_sqs::Client, queue_url: String) -> Self {
        Self {
            inner: Arc::new(Inner::Sqs { client, queue_url }),
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
            Inner::Postgres { .. } | Inner::Sqs { .. } => {
                panic!("captured() is only meaningful on for_tests() backends")
            }
        }
    }

    /// Enqueue a job for background processing. Returns `Ok(())` for
    /// success-or-idempotent-skip — the caller doesn't need to
    /// distinguish "new job created" from "duplicate suppressed."
    ///
    /// SQS idempotency note: SQS doesn't have a native idempotency
    /// key — `EnqueueOpts.idempotency_key` is honoured by the Postgres
    /// driver only. For prod jobs that need it (e.g. inquiry reply
    /// emails), use a content-addressed key derived from the row id
    /// and rely on the handler being idempotent. See
    /// `decisions.md` 2026-05-29.
    pub async fn enqueue(&self, event: JobEvent, opts: EnqueueOpts) -> Result<(), JobsError> {
        match &*self.inner {
            Inner::Memory { captured } => {
                captured.lock().unwrap().push(event);
                Ok(())
            }
            Inner::Postgres { pool } => postgres::enqueue(pool, event, opts).await,
            Inner::Sqs { client, queue_url } => sqs::enqueue(client, queue_url, event).await,
        }
    }
}

#[derive(Debug, thiserror::Error)]
pub enum JobsError {
    #[error("database error: {0}")]
    Db(#[from] sqlx::Error),
    #[error("payload serialization failed: {0}")]
    Serialize(#[from] serde_json::Error),
    #[error("sqs send failed: {0}")]
    Sqs(String),
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
// SQS driver
// ─────────────────────────────────────────────────────────────────────────────

pub mod sqs {
    //! SQS-backed jobs driver. Used by the api Lambda in prod;
    //! mirrors the producer side of `core::jobs::handle`, which the
    //! `jobs-lambda` binary runs on the SQS receive side.
    //!
    //! Message body is the same JSON shape the `JobEvent` enum
    //! serializes to (`{"kind": "...", "payload": {...}}`). The
    //! `MessageAttributes.kind` attribute mirrors that for cheap
    //! filtering at the consumer if we ever shard the queue.
    //!
    //! Idempotency: `EnqueueOpts.idempotency_key` is intentionally
    //! ignored here. SQS Standard queues don't have native idempotency;
    //! FIFO queues do (via `MessageDeduplicationId`) but FIFO caps at
    //! 300 msg/s/queue, which is fine for v1 but a needless ceiling.
    //! The pattern is: handler should be safe to run twice (most are
    //! already content-addressed on row id), and the
    //! `aws_sqs_queue.max_receive_count = 5` redrive policy bounds
    //! retries.

    use super::{JobEvent, JobsError};
    use aws_sdk_sqs::types::MessageAttributeValue;

    pub async fn enqueue(
        client: &aws_sdk_sqs::Client,
        queue_url: &str,
        event: JobEvent,
    ) -> Result<(), JobsError> {
        let kind = event.kind();
        // Serialize the full tagged shape — same as Postgres but with
        // tag included (the Postgres driver strips the tag because
        // the column stores `kind` separately; SQS has no separate
        // column, so we keep it).
        let body = serde_json::to_string(&event)?;

        client
            .send_message()
            .queue_url(queue_url)
            .message_body(body)
            .message_attributes(
                "kind",
                MessageAttributeValue::builder()
                    .data_type("String")
                    .string_value(kind)
                    .build()
                    .map_err(|e| JobsError::Sqs(format!("attribute build: {e}")))?,
            )
            .send()
            .await
            .map_err(|e| JobsError::Sqs(format!("send_message: {e}")))?;

        Ok(())
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
    pub emails: crate::emails::EmailClient,
    pub moderation: crate::moderation::ModerationClient,
    /// `WEB_BASE_URL` — passed in so handlers can build email links
    /// without re-reading env or threading the whole `Config`.
    pub web_base_url: String,
    /// T-068 — HMAC secret for minting unsubscribe tokens. Same value
    /// as the anon-cookie secret (single secret, multiple uses; see
    /// `core::notifications` for the rationale). Notification
    /// handlers pull this out to embed an unsubscribe link in every
    /// email they send.
    pub anon_cookie_secret: String,
    /// T-054 — domain for the tokenised inquiry reply-to addresses
    /// (`r-<inquiry_id>-<hmac>@<reply_email_domain>`). The artist-reply
    /// handler mints this address so the inquirer's reply routes back
    /// through the inbound-email webhook. Mirrors `Config::reply_email_domain`.
    pub reply_email_domain: String,
    /// SQS / Postgres enqueue handle — needed by the kickoff handler
    /// to fan out per-user digest jobs. Wrapped in `Arc` for cheap
    /// clone-into-handlers.
    pub jobs: JobsBackend,
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
        JobEvent::InquirySendVerification { inquiry_id } => {
            inquiry_handlers::send_verification(deps, inquiry_id)
                .await
                .map_err(|e| HandlerError::Domain(e.to_string()))?;
        }
        JobEvent::InquiryDeliverToArtist { inquiry_id } => {
            inquiry_handlers::deliver_to_artist(deps, inquiry_id)
                .await
                .map_err(|e| HandlerError::Domain(e.to_string()))?;
        }
        JobEvent::InquirySendReply { reply_id } => {
            inquiry_handlers::send_reply(deps, reply_id)
                .await
                .map_err(|e| HandlerError::Domain(e.to_string()))?;
        }
        JobEvent::InquirySendReplyForward { reply_id } => {
            inquiry_handlers::send_reply_forward(deps, reply_id)
                .await
                .map_err(|e| HandlerError::Domain(e.to_string()))?;
        }
        JobEvent::ArtworkImageModerate { artwork_image_id } => {
            crate::moderation::moderate_artwork_image(
                &deps.moderation,
                &deps.pool,
                artwork_image_id,
            )
            .await
            .map_err(|e| HandlerError::Domain(e.to_string()))?;
        }
        JobEvent::UploadModerate { upload_id } => {
            crate::moderation::moderate_upload(&deps.moderation, &deps.pool, upload_id)
                .await
                .map_err(|e| HandlerError::Domain(e.to_string()))?;
        }
        JobEvent::NotifyFollowersDigestKickoff {} => {
            digest_handlers::kickoff(deps)
                .await
                .map_err(|e| HandlerError::Domain(e.to_string()))?;
        }
        JobEvent::EventLog {
            name,
            anonymous_id,
            user_id,
            properties,
            context,
        } => {
            // T-050 — single INSERT into the `events` table. Storage
            // destination is intentionally encapsulated here so a
            // future Postgres → S3 Parquet swap doesn't touch any
            // emit site. Schema-version stays at the column default (1).
            sqlx::query(
                r#"
                INSERT INTO events
                    (anonymous_id, user_id, event_name, properties, context)
                VALUES ($1, $2, $3, $4, $5)
                "#,
            )
            .bind(anonymous_id)
            .bind(user_id)
            .bind(name.as_str())
            .bind(&properties)
            .bind(&context)
            .execute(&deps.pool)
            .await
            .map_err(|e| HandlerError::Domain(format!("event_log insert: {e}")))?;
        }
        JobEvent::NotifyFollowersDigestUser { user_id } => {
            digest_handlers::send_user_digest(deps, user_id)
                .await
                .map_err(|e| HandlerError::Domain(e.to_string()))?;
        }
        JobEvent::UserProfileRefresh { user_id } => {
            // T-055 — compute weighted-decayed taste embedding from
            // recent events and upsert `user_profiles`. Domain logic
            // lives in `core::user_profile`; this arm is just dispatch.
            crate::user_profile::refresh_user(&deps.pool, user_id)
                .await
                .map_err(|e| HandlerError::Domain(format!("user_profile_refresh: {e}")))?;
        }
        JobEvent::UserProfileRefreshKickoff {} => {
            // T-055.2 — fan out one `UserProfileRefresh` per user with
            // recent activity. Logging lives inside `kickoff`.
            crate::user_profile::kickoff(deps)
                .await
                .map_err(|e| HandlerError::Domain(format!("user_profile_refresh_kickoff: {e}")))?;
        }
        JobEvent::FxRatesRefresh {} => {
            // T-080 — pull latest GBP-base rates from Frankfurter,
            // upsert `fx_rates`, recompute `price_gbp_cents` across
            // the corpus. Logging + counts live in `fx::refresh_rates`.
            crate::fx::refresh_rates(&deps.pool)
                .await
                .map_err(|e| HandlerError::Domain(format!("fx_rates_refresh: {e}")))?;
        }
    }
    Ok(())
}

// ─────────────────────────────────────────────────────────────────────────────
// Inquiry email handlers (T-032)
// ─────────────────────────────────────────────────────────────────────────────
//
// Inline rather than in `core::emails` because they're driven by jobs
// + handle DB IO. `core::emails` is the pure HTTP-client + templates
// surface; this is the orchestration.

mod inquiry_handlers {
    use super::JobsDeps;
    use crate::emails::templates;
    use sqlx::FromRow;
    use uuid::Uuid;

    /// Row shape for both inquiry email paths. Inquiries that need a
    /// verification email don't have an artist email yet (we email
    /// the inquirer); inquiries that need a delivery email do
    /// (we email the artist). The same row covers both since the
    /// fetch is the union of fields the templates touch.
    #[derive(Debug, FromRow)]
    struct InquiryWithContext {
        // Inquirer side.
        from_name: String,
        from_email: String,
        message: String,
        budget_range: Option<serde_json::Value>,
        verification_token: Option<String>,
        // Artwork + artist side.
        artwork_id: Uuid,
        artwork_title: Option<String>,
        artist_display_name: String,
        artist_email: Option<String>,
        artist_slug: String,
        primary_s3_key: Option<String>,
    }

    async fn load(
        pool: &crate::db::Pool,
        inquiry_id: Uuid,
    ) -> Result<InquiryWithContext, sqlx::Error> {
        sqlx::query_as::<_, InquiryWithContext>(
            r#"
            SELECT
                i.from_name,
                i.from_email,
                i.message,
                i.budget_range,
                i.verification_token,
                a.id            AS artwork_id,
                a.title         AS artwork_title,
                ar.display_name AS artist_display_name,
                u.email         AS artist_email,
                ar.slug         AS artist_slug,
                ai.s3_key       AS primary_s3_key
            FROM inquiries i
            JOIN artworks a   ON a.id = i.artwork_id
            JOIN artists  ar  ON ar.id = i.artist_id
            LEFT JOIN users u ON u.id = ar.user_id
            LEFT JOIN artwork_images ai
                   ON ai.artwork_id = a.id AND ai.is_primary
            WHERE i.id = $1
            "#,
        )
        .bind(inquiry_id)
        .fetch_one(pool)
        .await
    }

    pub async fn send_verification(deps: &JobsDeps, inquiry_id: Uuid) -> anyhow::Result<()> {
        let row = load(&deps.pool, inquiry_id).await?;
        let Some(token) = row.verification_token.as_deref() else {
            // No token means this inquiry was already verified (or
            // is signed-in). Nothing to send; succeed quietly.
            tracing::info!(%inquiry_id, "skip verification — no token (already verified?)");
            return Ok(());
        };
        let verify_url = format!(
            "{base}/inquiries/verify/{token}",
            base = deps.web_base_url.trim_end_matches('/'),
        );
        let (subject, body) = templates::verification(
            &verify_url,
            &row.from_name,
            row.artwork_title.as_deref(),
            &row.artist_display_name,
        );
        deps.emails
            .send(&row.from_email, &subject, &body, None)
            .await?;
        tracing::info!(%inquiry_id, to = %row.from_email, "verification email sent");
        Ok(())
    }

    /// Send the artist's reply email to the original inquirer.
    /// Idempotent on `inquiry_replies.sent_at` — a second call sees
    /// a non-NULL `sent_at` and returns Ok without re-sending. This
    /// matches `inquiries.delivered_at`'s pattern. T-011 Phase 4b.
    pub async fn send_reply(deps: &JobsDeps, reply_id: Uuid) -> anyhow::Result<()> {
        #[derive(Debug, FromRow)]
        struct ReplyRow {
            message: String,
            sent_at: Option<chrono::DateTime<chrono::Utc>>,
            inquiry_id: Uuid,
            from_name: String,
            from_email: String,
            artwork_id: Uuid,
            artwork_title: Option<String>,
            artist_display_name: String,
        }
        let row: ReplyRow = sqlx::query_as(
            r#"
            SELECT
                r.message,
                r.sent_at,
                i.id            AS inquiry_id,
                i.from_name,
                i.from_email,
                a.id            AS artwork_id,
                a.title         AS artwork_title,
                ar.display_name AS artist_display_name
            FROM inquiry_replies r
            JOIN inquiries i  ON i.id = r.inquiry_id
            JOIN artworks  a  ON a.id = i.artwork_id
            JOIN artists   ar ON ar.id = r.artist_id
            WHERE r.id = $1
            "#,
        )
        .bind(reply_id)
        .fetch_one(&deps.pool)
        .await?;
        if row.sent_at.is_some() {
            tracing::info!(%reply_id, "skip send_reply — already sent");
            return Ok(());
        }
        // T-054 — the reply-to is a tokenised per-inquiry address rather
        // than the artist's real email. When the inquirer hits reply it
        // routes through the inbound-email webhook, which verifies the
        // HMAC and threads the reply back onto this inquiry. The HMAC
        // secret is the same `anon_cookie_secret` reused app-wide.
        let reply_to = crate::reply_address::mint(
            row.inquiry_id,
            &deps.reply_email_domain,
            deps.anon_cookie_secret.as_bytes(),
        );
        let artwork_url = format!(
            "{base}/artworks/{id}",
            base = deps.web_base_url.trim_end_matches('/'),
            id = row.artwork_id,
        );
        let (subject, body) = templates::artist_reply(
            &artwork_url,
            row.artwork_title.as_deref(),
            &row.artist_display_name,
            &row.from_name,
            &row.message,
        );
        deps.emails
            .send(&row.from_email, &subject, &body, Some(&reply_to))
            .await?;
        sqlx::query("UPDATE inquiry_replies SET sent_at = now() WHERE id = $1")
            .bind(reply_id)
            .execute(&deps.pool)
            .await?;
        tracing::info!(
            %reply_id,
            inquiry_id = %row.inquiry_id,
            to = %row.from_email,
            "artist reply delivered"
        );
        Ok(())
    }

    /// T-054 — forward an inquirer's inbound reply to the artist. The
    /// reply row was written by the inbound-email webhook with
    /// `from_role='inquirer'` and a NULL `artist_id`, so the artist is
    /// resolved through the inquiry, not the reply. Idempotent on
    /// `inquiry_replies.sent_at` — a second run sees it set and bails.
    pub async fn send_reply_forward(deps: &JobsDeps, reply_id: Uuid) -> anyhow::Result<()> {
        #[derive(Debug, FromRow)]
        struct ForwardRow {
            message: String,
            sent_at: Option<chrono::DateTime<chrono::Utc>>,
            inquiry_id: Uuid,
            from_name: String,
            from_email: String,
            artwork_id: Uuid,
            artwork_title: Option<String>,
            artist_email: Option<String>,
        }
        let row: ForwardRow = sqlx::query_as(
            r#"
            SELECT
                r.message,
                r.sent_at,
                i.id            AS inquiry_id,
                i.from_name,
                i.from_email,
                a.id            AS artwork_id,
                a.title         AS artwork_title,
                u.email         AS artist_email
            FROM inquiry_replies r
            JOIN inquiries i  ON i.id = r.inquiry_id
            JOIN artworks  a  ON a.id = i.artwork_id
            JOIN artists   ar ON ar.id = i.artist_id
            LEFT JOIN users u ON u.id = ar.user_id
            WHERE r.id = $1
            "#,
        )
        .bind(reply_id)
        .fetch_one(&deps.pool)
        .await?;
        if row.sent_at.is_some() {
            tracing::info!(%reply_id, "skip send_reply_forward — already sent");
            return Ok(());
        }
        let Some(artist_email) = row.artist_email.as_deref() else {
            // Seeded demo artist with no linked user/email — nothing to
            // forward to. Log + succeed (no retry will help).
            tracing::warn!(
                %reply_id,
                inquiry_id = %row.inquiry_id,
                "skip forward — artist has no user email"
            );
            return Ok(());
        };
        let artwork_url = format!(
            "{base}/artworks/{id}",
            base = deps.web_base_url.trim_end_matches('/'),
            id = row.artwork_id,
        );
        let (subject, body) = templates::inquirer_reply_forward(
            &artwork_url,
            row.artwork_title.as_deref(),
            &row.from_name,
            &row.message,
        );
        // Reply-To = the inquirer's real email so the artist can respond
        // directly (or from the studio inbox). Capturing the artist's
        // *email* reply back into the thread is a deferred follow-up.
        deps.emails
            .send(artist_email, &subject, &body, Some(&row.from_email))
            .await?;
        sqlx::query("UPDATE inquiry_replies SET sent_at = now() WHERE id = $1")
            .bind(reply_id)
            .execute(&deps.pool)
            .await?;
        tracing::info!(
            %reply_id,
            inquiry_id = %row.inquiry_id,
            to = %artist_email,
            "inquirer reply forwarded to artist"
        );
        Ok(())
    }

    pub async fn deliver_to_artist(deps: &JobsDeps, inquiry_id: Uuid) -> anyhow::Result<()> {
        let row = load(&deps.pool, inquiry_id).await?;
        let Some(artist_email) = row.artist_email.as_deref() else {
            // Artist has no linked Clerk user (seeded demo artists),
            // so no email address. Log + bail — no retry will help.
            tracing::warn!(
                %inquiry_id,
                artist = %row.artist_slug,
                "skip deliver-to-artist — artist has no user email"
            );
            return Ok(());
        };
        let artwork_url = format!(
            "{base}/artworks/{id}",
            base = deps.web_base_url.trim_end_matches('/'),
            id = row.artwork_id,
        );
        let image_url = row
            .primary_s3_key
            .as_deref()
            .map(crate::images::url_for_s3_key);
        let budget_str = row
            .budget_range
            .as_ref()
            .and_then(|v| v.as_str().map(String::from));
        let (subject, body) = templates::delivered_to_artist(
            &artwork_url,
            row.artwork_title.as_deref(),
            image_url.as_deref(),
            &row.from_name,
            &row.from_email,
            &row.message,
            budget_str.as_deref(),
        );
        deps.emails
            .send(artist_email, &subject, &body, Some(&row.from_email))
            .await?;
        tracing::info!(
            %inquiry_id,
            to = %artist_email,
            "inquiry delivered to artist"
        );
        Ok(())
    }
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
// T-052b — new-works-from-followed-artists digest
// ─────────────────────────────────────────────────────────────────────────────
//
// Two-stage fan-out: a daily kickoff (EventBridge in prod, manual
// `jobs-worker --enqueue` in local) scans for users with new work
// from their followed artists, then enqueues one
// `NotifyFollowersDigestUser` per user. Per-user handler builds the
// payload, sends one email, writes a log row.
//
// Idempotency: `user_notification_log` PK includes `sent_on::date`,
// so even if SQS redelivers the per-user message we'll only write
// one log row + send one email per user per day. See the migration
// at db/migrations/0017_user_notification_log.sql for the schema
// rationale.

mod digest_handlers {
    use super::{JobEvent, JobsDeps};
    use crate::emails::templates::{self, DigestArtistGroup, DigestWork};
    use crate::images::url_for_s3_key;
    use crate::notifications::{
        mint_unsubscribe_token, unsubscribe_url, user_wants, NotificationKind,
    };
    use sqlx::FromRow;
    use uuid::Uuid;

    /// Cap per-user. ~12 looks right in a vertical email layout
    /// (3-4 artists × 3 works avg). Picked by eye; bump if real
    /// engagement data shows long-tail value.
    const PER_USER_WORK_CAP: i64 = 12;

    /// Daily kickoff. Scans for users whose followed artists
    /// published new work since the follow began (capped to the last
    /// 24h), filters out users who already got a digest today and
    /// users with opted-out preferences, and enqueues a per-user
    /// digest job for each remaining user.
    pub async fn kickoff(deps: &JobsDeps) -> anyhow::Result<()> {
        let candidates: Vec<(Uuid,)> = sqlx::query_as(
            r#"
            SELECT DISTINCT f.user_id
            FROM follows f
            JOIN artworks a
              ON a.artist_id = f.artist_id
             AND a.deleted_at IS NULL
             AND a.status = 'published'
             AND a.published_at > GREATEST(f.created_at, now() - interval '24 hours')
            JOIN artists ar
              ON ar.id = f.artist_id
             AND ar.deleted_at IS NULL
             AND ar.status = 'active'
            WHERE NOT EXISTS (
              SELECT 1 FROM user_notification_log unl
              WHERE unl.user_id = f.user_id
                AND unl.kind = 'new_works_digest'
                AND unl.sent_on = current_date
            )
            "#,
        )
        .fetch_all(&deps.pool)
        .await?;

        tracing::info!(
            candidates = candidates.len(),
            "digest kickoff: candidate users scanned",
        );

        let mut enqueued = 0usize;
        for (user_id,) in candidates {
            // Defensive: kickoff query doesn't check user_wants —
            // call it per-user before enqueueing so we don't burn
            // SQS / Lambda invocations on users who'll just have
            // their per-user handler bail out.
            match user_wants(&deps.pool, user_id, NotificationKind::NewWorksDigest).await {
                Ok(true) => {}
                Ok(false) => continue,
                Err(e) => {
                    tracing::warn!(?e, %user_id, "user_wants failed; skipping");
                    continue;
                }
            }

            deps.jobs
                .enqueue(
                    JobEvent::NotifyFollowersDigestUser { user_id },
                    Default::default(),
                )
                .await?;
            enqueued += 1;
        }

        tracing::info!(enqueued, "digest kickoff: per-user jobs enqueued");
        Ok(())
    }

    /// Per-user digest. Claims today's slot via the
    /// `user_notification_log` PK (only the first concurrent writer
    /// proceeds), builds + sends one email, then leaves the log row
    /// behind for the next cron run to see.
    pub async fn send_user_digest(deps: &JobsDeps, user_id: Uuid) -> anyhow::Result<()> {
        // Defensive re-check of preference state in case it changed
        // between kickoff scan and message delivery (e.g. user
        // unsubscribed while the message was in flight).
        if !user_wants(&deps.pool, user_id, NotificationKind::NewWorksDigest).await? {
            tracing::info!(%user_id, "user opted out between kickoff and send; skip");
            return Ok(());
        }

        // Claim today's slot. If a row already exists, another worker
        // is handling (or has already handled) this user today.
        let claimed: Option<(Uuid,)> = sqlx::query_as(
            r#"
            INSERT INTO user_notification_log (user_id, kind)
            VALUES ($1, 'new_works_digest')
            ON CONFLICT (user_id, kind, sent_on) DO NOTHING
            RETURNING user_id
            "#,
        )
        .bind(user_id)
        .fetch_optional(&deps.pool)
        .await?;
        if claimed.is_none() {
            tracing::info!(%user_id, "digest already sent today; skip");
            return Ok(());
        }

        // Fetch the user's email (needed for the To: field).
        let user: Option<UserRow> = sqlx::query_as("SELECT email FROM users WHERE id = $1")
            .bind(user_id)
            .fetch_optional(&deps.pool)
            .await?;
        let Some(user) = user else {
            // User vanished between claim and fetch — unusual but
            // harmless. The log row is left behind; nothing to send.
            tracing::warn!(%user_id, "user not found after claim; skip");
            return Ok(());
        };

        // Pull the new artworks per the same per-follow window as
        // the kickoff query.
        let rows: Vec<DigestRow> = sqlx::query_as(
            r#"
            SELECT
                a.id            AS artwork_id,
                a.title,
                a.published_at,
                ar.slug         AS artist_slug,
                ar.display_name AS artist_display_name,
                ai.s3_key       AS primary_s3_key
            FROM follows f
            JOIN artworks a
              ON a.artist_id = f.artist_id
             AND a.deleted_at IS NULL
             AND a.status = 'published'
             AND a.published_at > GREATEST(f.created_at, now() - interval '24 hours')
            JOIN artists ar
              ON ar.id = f.artist_id
             AND ar.deleted_at IS NULL
             AND ar.status = 'active'
            LEFT JOIN artwork_images ai
              ON ai.artwork_id = a.id
             AND ai.is_primary
             AND ai.moderation_status = 'approved'
            WHERE f.user_id = $1
            ORDER BY a.published_at DESC
            LIMIT $2
            "#,
        )
        .bind(user_id)
        .bind(PER_USER_WORK_CAP)
        .fetch_all(&deps.pool)
        .await?;

        if rows.is_empty() {
            // Edge case: kickoff said this user had new work but by
            // the time we look it's gone (deleted between scan + send).
            // The log row we just claimed will sit there preventing
            // re-attempts today, which is fine.
            tracing::info!(%user_id, "digest payload empty after claim; skip send");
            return Ok(());
        }

        // Build the per-artist groups (already DESC-sorted by query;
        // group runs in encounter order which means most-recent-
        // artist first within each user's digest).
        let groups = build_groups(&rows, &deps.web_base_url);

        // Mint the unsubscribe token for this kind. Same secret as
        // the anon-cookie HMAC (see T-068).
        let token = mint_unsubscribe_token(
            user_id,
            NotificationKind::NewWorksDigest,
            deps.anon_cookie_secret.as_bytes(),
        )?;
        let unsub_url = unsubscribe_url(&deps.web_base_url, &token);
        let manage_url = format!(
            "{}/me/settings/notifications",
            deps.web_base_url.trim_end_matches('/'),
        );

        let (subject, body) = templates::new_works_digest(&groups, &manage_url, &unsub_url);

        deps.emails
            .send_notification(&user.email, &subject, &body, &unsub_url)
            .await?;

        tracing::info!(
            %user_id,
            works = rows.len(),
            artists = groups.len(),
            "digest sent",
        );
        Ok(())
    }

    /// Group the flat rows by artist. Preserves the per-row published-
    /// at-DESC ordering: the first artist seen is the one with the
    /// most recently published work.
    fn build_groups<'a>(
        rows: &'a [DigestRow],
        web_base_url: &'a str,
    ) -> Vec<DigestArtistGroup<'a>> {
        use std::collections::HashMap;
        let base = web_base_url.trim_end_matches('/');

        let mut order: Vec<&str> = Vec::new();
        let mut by_slug: HashMap<&str, Vec<DigestWork<'a>>> = HashMap::new();
        let mut display: HashMap<&str, &str> = HashMap::new();

        for row in rows {
            let slug = row.artist_slug.as_str();
            if !by_slug.contains_key(slug) {
                order.push(slug);
                display.insert(slug, &row.artist_display_name);
            }
            let url = Box::leak(format!("{base}/artworks/{}", row.artwork_id).into_boxed_str());
            let image_url = row.primary_s3_key.as_deref().map(|k| {
                let s = url_for_s3_key(k);
                Box::leak(s.into_boxed_str()) as &str
            });
            by_slug.entry(slug).or_default().push(DigestWork {
                title: row.title.as_deref(),
                url,
                image_url,
            });
        }

        order
            .into_iter()
            .map(|slug| {
                let artist_display_name = *display.get(slug).expect("slug indexed");
                let artist_url =
                    Box::leak(format!("{base}/artists/{slug}").into_boxed_str()) as &str;
                DigestArtistGroup {
                    artist_display_name,
                    artist_url,
                    works: by_slug.remove(slug).expect("slug indexed"),
                }
            })
            .collect()
    }

    #[derive(FromRow)]
    struct DigestRow {
        artwork_id: Uuid,
        title: Option<String>,
        #[allow(dead_code)]
        published_at: chrono::DateTime<chrono::Utc>,
        artist_slug: String,
        artist_display_name: String,
        primary_s3_key: Option<String>,
    }

    #[derive(FromRow)]
    struct UserRow {
        email: String,
    }
}

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
        assert_eq!(
            JobEvent::InquirySendVerification {
                inquiry_id: Uuid::new_v4()
            }
            .kind(),
            "inquiry_send_verification"
        );
        assert_eq!(
            JobEvent::InquiryDeliverToArtist {
                inquiry_id: Uuid::new_v4()
            }
            .kind(),
            "inquiry_deliver_to_artist"
        );
        assert_eq!(
            JobEvent::ArtworkImageModerate {
                artwork_image_id: Uuid::new_v4()
            }
            .kind(),
            "artwork_image_moderate"
        );
        assert_eq!(
            JobEvent::UploadModerate {
                upload_id: Uuid::new_v4()
            }
            .kind(),
            "upload_moderate"
        );
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

    /// The SQS body shape is the contract between the producer (this
    /// crate's `JobsBackend::Sqs`) and the consumer (`jobs-lambda`'s
    /// `serde_json::from_value::<JobEvent>`). If either side drifts,
    /// every message DLQs. This test pins the shape so the contract
    /// is checked by CI rather than discovered in production.
    #[test]
    fn sqs_message_body_round_trips_through_serde() {
        let id = Uuid::new_v4();
        let evt = JobEvent::ArtworkImageModerate {
            artwork_image_id: id,
        };

        // Producer side: serialize the full tagged JSON. This is
        // exactly what `sqs::enqueue` puts into the message body.
        let body = serde_json::to_string(&evt).expect("serialize");
        assert!(body.contains(r#""kind":"artwork_image_moderate""#));
        assert!(body.contains(&id.to_string()));

        // Consumer side: jobs-lambda parses with `serde_json::from_value`
        // (via SqsEventObj<Value>). We mimic the same path here.
        let value: serde_json::Value = serde_json::from_str(&body).expect("parse body");
        let back: JobEvent = serde_json::from_value(value).expect("decode JobEvent");
        assert_eq!(back, evt);
    }
}
