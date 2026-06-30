-- T-083 — admin surface foundation.
--
-- `users.is_admin` already exists (0001_init.sql); this migration adds
-- the audit log and the bootstrap UPDATE that promotes the platform's
-- first admin.
--
-- Every admin mutation writes one row here before applying. NULL
-- `admin_user_id` represents a system action (auto-promotion from
-- `ADMIN_EMAILS`, future scheduled-job mutations). The before/after
-- jsonb snapshots are intentionally generic so we don't grow a column
-- per target_kind.

CREATE TABLE admin_audit_log (
    id              uuid PRIMARY KEY DEFAULT gen_random_uuid(),
    admin_user_id   uuid REFERENCES users(id), -- NULL = system action
    action          text NOT NULL,             -- e.g. 'artist.approve'
    target_kind     text NOT NULL,             -- 'artist' | 'image' | 'venue' | 'user'
    target_id       uuid,                      -- nullable for actions without a single subject
    before_jsonb    jsonb,
    after_jsonb     jsonb,
    context         jsonb,                     -- IP, UA, route, etc.
    created_at      timestamptz NOT NULL DEFAULT now()
);

-- Time-ordered reads dominate (the audit-log viewer is "show me the
-- last N actions"). Partial index on admin_user_id IS NOT NULL so
-- "who did what" lookups skip the system-action rows.
CREATE INDEX admin_audit_log_created_at_desc_idx
    ON admin_audit_log (created_at DESC);
CREATE INDEX admin_audit_log_admin_user_id_idx
    ON admin_audit_log (admin_user_id) WHERE admin_user_id IS NOT NULL;
CREATE INDEX admin_audit_log_target_idx
    ON admin_audit_log (target_kind, target_id) WHERE target_id IS NOT NULL;

-- Bootstrap the first admin. Idempotent — no-op if the row doesn't
-- exist yet (user hasn't signed in via Clerk before this deploy). The
-- upsert path in core::auth::upsert_user also seeds is_admin from
-- ADMIN_EMAILS on first sign-in, so either order works.
UPDATE users
   SET is_admin = true,
       updated_at = now()
 WHERE lower(email) = 'mrjoshuajmatthews@gmail.com'
   AND is_admin = false;
