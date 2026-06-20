-- T-052c — capture intents from anonymous users so the next-most-
-- important moment in their journey (signing up right after clicking
-- Follow) doesn't waste their highest-intent signal. The merge-
-- anonymous handler drains rows keyed on the anon_id cookie and
-- replays each onto the now-known user.
--
-- Why a generic table rather than a `pending_follows` column: the same
-- "anon click → bounce to sign-in → lose the intent" leak applies to
-- save-to-collection, inquiry-start, save-search-as-alert, etc. One
-- shape, one merge codepath. New kinds add a `kind` value + a match
-- arm in the merge handler.
--
-- TTL via `expires_at`: stale intents shouldn't replay weeks later
-- (e.g. user signs up months after a one-off browse). 7 days strikes
-- the right balance — long enough for "I'll sign up tomorrow," short
-- enough that an intent doesn't surprise the user.

CREATE TABLE anon_pending_actions (
    id          uuid        PRIMARY KEY DEFAULT gen_random_uuid(),
    anon_id     uuid        NOT NULL,
    kind        text        NOT NULL,
    payload     jsonb       NOT NULL,
    created_at  timestamptz NOT NULL DEFAULT now(),
    expires_at  timestamptz NOT NULL DEFAULT (now() + interval '7 days')
);

-- Drain query: `WHERE anon_id = $1 AND expires_at > now()`.
CREATE INDEX anon_pending_actions_anon_id_idx
    ON anon_pending_actions (anon_id);

-- Cleanup query: `WHERE expires_at < now()` for a future
-- `purge_expired_anon_actions` scheduled job.
CREATE INDEX anon_pending_actions_expires_idx
    ON anon_pending_actions (expires_at);

-- Stop the same intent being queued twice from a double-click (which
-- would otherwise replay as two follows; safe because the follow
-- INSERT is itself idempotent, but cheap to enforce).
--
-- The jsonb cast in the unique index lets PG hash it deterministically.
CREATE UNIQUE INDEX anon_pending_actions_dedup_idx
    ON anon_pending_actions (anon_id, kind, payload);
