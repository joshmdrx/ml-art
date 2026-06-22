-- T-054 — Inquirer-inbound replies (email-stitched threads).
--
-- Until now every `inquiry_replies` row was written by an artist from
-- the studio inbox (migration 0013). This migration lets the *inquirer*
-- thread back in: an artist-reply email carries a tokenised Reply-To
-- (`r-<inquiry_id>-<hmac>@reply.<domain>`); when the inquirer replies it
-- routes through Cloudflare Email Routing → a Worker → our webhook, which
-- persists the message as an inquirer-authored row on the same inquiry.
--
-- Three changes:
--
--   1. `from_role` — who wrote this reply. Defaults to 'artist' so the
--      existing studio-reply INSERT (no column listed) keeps working
--      unchanged. Inbound-webhook rows set 'inquirer'.
--
--   2. `artist_id` nullable — inquirer-authored rows have no artist
--      author. Artist rows still set it (the ownership-checked INSERT in
--      studio/inquiries.rs is unchanged).
--
--   3. `inbound_message_id` — the inbound mail's Message-ID, used as a
--      replay guard. The webhook INSERTs `ON CONFLICT DO NOTHING` against
--      the partial unique index, so re-delivery of the same message is a
--      no-op. NULL for artist rows (no index entry).

ALTER TABLE inquiry_replies
    ADD COLUMN from_role text NOT NULL DEFAULT 'artist'
        CHECK (from_role IN ('artist', 'inquirer'));

ALTER TABLE inquiry_replies
    ALTER COLUMN artist_id DROP NOT NULL;

ALTER TABLE inquiry_replies
    ADD COLUMN inbound_message_id text;

CREATE UNIQUE INDEX inquiry_replies_inbound_message_id_key
    ON inquiry_replies (inbound_message_id)
    WHERE inbound_message_id IS NOT NULL;
