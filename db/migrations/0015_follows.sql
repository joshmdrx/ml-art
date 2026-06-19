-- T-052 — Follow an artist.
--
-- A signed-in user can follow an artist. Composite PK on
-- (user_id, artist_id) so the API can use a single UPSERT for both
-- happy-path inserts and double-click idempotency, and the reverse
-- "who follows this artist" lookup is supported by a secondary index
-- on (artist_id, created_at).
--
-- Soft-delete is intentionally not modelled — unfollow is a row
-- delete. We don't need an audit trail for "Alice unfollowed Bob
-- on Tuesday." If we ever do, a follows_history events stream
-- (the T-050 events table) is the right home, not this table.
--
-- No FK action on user/artist deletion: deferred until we know
-- the deletion semantics for either. Today both are soft-deleted
-- (`deleted_at`), so a hard FK would break those flows. Add
-- ON DELETE CASCADE if/when we move to hard deletes.

CREATE TABLE follows (
    user_id     uuid NOT NULL REFERENCES users(id),
    artist_id   uuid NOT NULL REFERENCES artists(id),
    created_at  timestamptz NOT NULL DEFAULT now(),
    PRIMARY KEY (user_id, artist_id)
);

-- Reverse lookup: "who follows this artist, most recent first."
-- Powers the studio "N followers" count + (eventually) the
-- per-publish-event NotifyFollowers fan-out.
CREATE INDEX follows_artist_recent_idx
    ON follows (artist_id, created_at DESC);
