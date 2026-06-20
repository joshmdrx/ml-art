-- T-068 — Email-notification preferences spine.
--
-- Two pieces:
--
--   1. `users.global_email_notifications_enabled` — master kill
--      switch. Toggling this off suppresses every non-transactional
--      email regardless of per-kind setting. Satisfies the legal
--      "easy single-step unsubscribe" requirement (CAN-SPAM / CASL /
--      GDPR).
--
--   2. `notification_preferences` — per-(user, kind) overrides. The
--      common case is "no row" → default on. A row only exists when
--      the user has explicitly toggled that kind. Keeps the table
--      tiny: most users will have zero rows.
--
-- Why no `notification_preferences.value` enum (just `enabled bool`):
-- "snoozed for 7 days" or "weekly instead of daily" are notification-
-- specific concerns; modelling them as a bool keeps the spine
-- generic. Future per-kind tables can layer richer state if needed.
--
-- Transactional emails (inquiry verification, artist reply forward,
-- account / security) bypass ALL of this — they're sent regardless.
-- The check happens in `core::notifications::user_wants`, which short-
-- circuits true for any `NotificationKind::is_transactional()` variant.

ALTER TABLE users
    ADD COLUMN global_email_notifications_enabled boolean NOT NULL DEFAULT true;

CREATE TABLE notification_preferences (
    user_id     uuid        NOT NULL REFERENCES users(id) ON DELETE CASCADE,
    kind        text        NOT NULL,
    enabled     boolean     NOT NULL DEFAULT true,
    updated_at  timestamptz NOT NULL DEFAULT now(),
    PRIMARY KEY (user_id, kind)
);

-- Cron-side "all users opted into kind X" lookup. Partial index keeps
-- the index small — only rows that are explicitly enabled show up here
-- (default-on users have no row, and `user_wants` defaults to true for
-- them anyway, so the index doesn't need to cover them).
CREATE INDEX notification_preferences_kind_enabled_idx
    ON notification_preferences (kind, user_id)
    WHERE enabled = true;
