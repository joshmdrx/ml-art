-- 0033_users_editing_draft.sql — T-086
--
-- Lets an admin edit a draft profile (0031) through the ordinary
-- studio UI. While `editing_draft_artist_id` is set, the admin's
-- `/v1/studio/*` calls resolve to that draft instead of their own
-- artist row (see `studio::current_artist_id`). Server-side state
-- rather than a header so every existing studio handler and page
-- works unchanged.
--
-- Only honoured for admins, and only while the target is still an
-- unclaimed draft — a claim or takedown silently ends the session.

ALTER TABLE users
    ADD COLUMN editing_draft_artist_id uuid REFERENCES artists(id);
