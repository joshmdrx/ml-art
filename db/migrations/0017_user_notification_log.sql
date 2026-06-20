-- T-052b — log of notification emails actually sent.
--
-- Two jobs:
--
--   1. Idempotency under SQS at-least-once. The per-user digest
--      handler does `INSERT … ON CONFLICT (user_id, kind, sent_on) DO
--      NOTHING RETURNING id`; only the row that *won* the insert
--      proceeds to send the email. Redeliveries are no-ops.
--
--   2. Audit trail. `sent_at` is the actual time of send; `context`
--      stores the artwork_ids in the digest so we can answer "which
--      works did this user see on which day" cheaply when debugging.
--
-- `sent_on` is date-truncated so the PK gives us the daily uniqueness
-- the digest needs. Weekly-cadence notifications (T-059 saved-search
-- alerts, T-060 Discover Weekly) layer either a parallel `sent_week`
-- constraint or use a different kind taxonomy — out of scope here.

CREATE TABLE user_notification_log (
    user_id    uuid        NOT NULL REFERENCES users(id) ON DELETE CASCADE,
    kind       text        NOT NULL,
    sent_on    date        NOT NULL DEFAULT current_date,
    sent_at    timestamptz NOT NULL DEFAULT now(),
    context    jsonb,
    PRIMARY KEY (user_id, kind, sent_on)
);

-- Audit-trail lookups: "what was the most recent X-kind email I sent
-- to this user?" Cheap on the existing PK; secondary index on
-- `(kind, sent_at DESC)` covers the cron-side "what did we send
-- today across the cohort" reporting query.
CREATE INDEX user_notification_log_kind_sent_at_idx
    ON user_notification_log (kind, sent_at DESC);
