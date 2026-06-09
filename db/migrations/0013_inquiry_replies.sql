-- T-011 Phase 4b — Artist replies + per-inquiry read state.
--
-- Two changes:
--
--   1. `inquiry_replies` — artist's outgoing messages on a given
--      inquiry. Modelled as a separate table (rather than e.g. a
--      `reply_text` column on `inquiries`) so the design admits a
--      future thread of multiple replies without another migration.
--      One reply per row, ordered by `created_at`.
--
--   2. `inquiries.read_at` — when the artist last viewed this row
--      in their studio inbox. Nullable: NULL ≡ unread. Powers the
--      "Auto-mark-as-read on inbox view" UX, and (eventually) an
--      unread-count badge in the studio nav. Distinct from
--      `delivered_at` (whether the verification email was sent to
--      the artist).
--
-- Send tracking: we record `sent_at` on the reply once the email
-- handler completes. NULL until then. Matches `inquiries.delivered_at`
-- conceptually.

CREATE TABLE inquiry_replies (
    id            uuid PRIMARY KEY DEFAULT gen_random_uuid(),
    inquiry_id    uuid NOT NULL REFERENCES inquiries(id) ON DELETE CASCADE,
    -- Denormalised for ownership checks + email-handler convenience.
    -- The artist who wrote the reply. In v1 == inquiry.artist_id, but
    -- carried separately so a future "platform-staff replies" path
    -- (legal escalation, etc.) doesn't need a schema change.
    artist_id     uuid NOT NULL REFERENCES artists(id),
    message       text NOT NULL CHECK (length(message) > 0),
    created_at    timestamptz NOT NULL DEFAULT now(),
    sent_at       timestamptz
);

-- Inbox view orders replies by created_at within an inquiry, and the
-- studio summary endpoint pulls all replies for a list of inquiries.
CREATE INDEX inquiry_replies_inquiry_id_idx
    ON inquiry_replies (inquiry_id, created_at);

-- Unread-inquiries query: `WHERE artist_id = $1 AND read_at IS NULL`.
-- Partial index keeps the unread count cheap as the row count grows.
ALTER TABLE inquiries
    ADD COLUMN read_at timestamptz;

CREATE INDEX inquiries_artist_unread_idx
    ON inquiries (artist_id)
    WHERE read_at IS NULL;
