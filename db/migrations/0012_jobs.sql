-- 0012_jobs.sql
-- Local-dev jobs queue. In prod the same job events flow through SQS +
-- Lambda; this table is the local-equivalent driver so handler code
-- stays identical across environments. See `decisions.md` 2026-05-29
-- — jobs queue: Postgres local, SQS+Lambda prod.
--
-- One row per pending / running / done / failed unit of work. The
-- worker (api/crates/jobs-worker) polls with
--   SELECT … FOR UPDATE SKIP LOCKED LIMIT 1
-- so multiple workers can run concurrently without grabbing the same
-- job. Handlers are written to be idempotent — re-runs (after a
-- worker crash, or after `attempts` increments) shouldn't double up.
--
-- `kind` is the JobEvent discriminator (snake_case match for the
-- Rust enum's `#[serde(tag = "kind")]`). `payload` is the per-variant
-- arguments as jsonb — same shape we'll later serialize into an SQS
-- message body, so deserialization is identical in either driver.
--
-- `idempotency_key` is optional but UNIQUE when present; lets callers
-- enqueue the same logical work twice without duplicating rows
-- (e.g. "geocode location X" enqueued from two code paths in the
-- same handler).

CREATE TABLE jobs (
    id                uuid PRIMARY KEY DEFAULT gen_random_uuid(),
    kind              text NOT NULL,
    payload           jsonb NOT NULL,
    status            text NOT NULL DEFAULT 'pending'
        CHECK (status IN ('pending', 'running', 'done', 'failed')),
    attempts          integer NOT NULL DEFAULT 0,
    max_attempts      integer NOT NULL DEFAULT 5,
    -- Earliest time the row is eligible to be picked up. Set on
    -- enqueue (= now()), pushed out on retry by exponential backoff.
    next_run_at       timestamptz NOT NULL DEFAULT now(),
    -- Optional dedup key for "enqueue at most once" semantics.
    idempotency_key   text UNIQUE,
    -- Populated on the last failure (truncated to a reasonable size by
    -- the worker before insert).
    last_error        text,
    created_at        timestamptz NOT NULL DEFAULT now(),
    updated_at        timestamptz NOT NULL DEFAULT now(),
    completed_at      timestamptz
);

-- The pickable index. Partial-indexed on the (small) set of rows the
-- worker actually scans — keeps the cost constant as `done` rows
-- accumulate.
CREATE INDEX jobs_pickable_idx
    ON jobs (next_run_at)
    WHERE status = 'pending';

-- Lookup by kind for ops-y queries ("how many email jobs failed last
-- hour?"); cheap to maintain.
CREATE INDEX jobs_kind_status_idx
    ON jobs (kind, status);
