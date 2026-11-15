-- 0032_artist_preview_links.sql — T-086
--
-- Private preview links for draft profiles (see 0031). The link is a
-- bearer secret: anyone holding `/preview/<token>` can view the draft,
-- claim it (signed in) or remove it. We store only a SHA-256 hash of
-- the token so a database read can't reconstruct live links; the
-- plaintext is returned once, at creation, to the admin.
--
-- Lifecycle:
--   - Minting a new link revokes any earlier live link for the draft
--     (one live link per draft keeps "who can see this" answerable).
--   - `expires_at` defaults to 90 days out (set by the handler).
--   - `claimed_at` / `claimed_by_user_id` stamp the claim.
--   - `revoked_at` is set on re-mint, takedown or removal.
-- A link is live iff revoked_at IS NULL AND claimed_at IS NULL AND
-- expires_at > now().

CREATE TABLE artist_preview_links (
    id                  uuid PRIMARY KEY DEFAULT gen_random_uuid(),
    artist_id           uuid NOT NULL REFERENCES artists(id),
    token_hash          text NOT NULL UNIQUE,
    created_by          uuid REFERENCES users(id),
    expires_at          timestamptz NOT NULL,
    claimed_at          timestamptz,
    claimed_by_user_id  uuid REFERENCES users(id),
    revoked_at          timestamptz,
    created_at          timestamptz NOT NULL DEFAULT now(),
    updated_at          timestamptz NOT NULL DEFAULT now()
);

CREATE INDEX artist_preview_links_artist_idx ON artist_preview_links (artist_id);
