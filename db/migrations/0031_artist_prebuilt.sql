-- 0031_artist_prebuilt.sql — T-086
--
-- Consent-first draft profiles. An admin builds a draft gallery /
-- studio page *after* the organisation has said yes to an example,
-- shares it through a private preview link (0032), and the
-- organisation either claims it (links their account) or removes it.
-- See decisions.md 2026-11-15 — "Revive the claim flow, consent-first".
--
-- A draft is an ordinary `artists` row with:
--   - `user_id IS NULL`      (nobody owns it yet)
--   - `status = 'pending'`   (never on a public surface)
--   - `is_prebuilt = true`   (built by Wander, not self-onboarded)
--
-- `is_prebuilt` stays true after a claim — it's provenance ("we built
-- the first version"), not state. "Unclaimed draft" is always the pair
-- `is_prebuilt AND user_id IS NULL`.
--
-- `prebuilt_consent_note` records how the organisation agreed ("Replied
-- yes to the founding-studio email, 2026-11-20"). Required for every
-- prebuilt row so a draft can't exist without a consent trail.

ALTER TABLE artists
    ADD COLUMN is_prebuilt boolean NOT NULL DEFAULT false,
    ADD COLUMN prebuilt_consent_note text,
    ADD CONSTRAINT artists_prebuilt_consent_chk
        CHECK (NOT is_prebuilt OR prebuilt_consent_note IS NOT NULL);

-- The admin drafts list reads unclaimed + claimed drafts newest-first.
CREATE INDEX artists_prebuilt_idx ON artists (created_at DESC)
    WHERE is_prebuilt AND deleted_at IS NULL;
