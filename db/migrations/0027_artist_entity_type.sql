-- 0027_artist_entity_type.sql
--
-- Distinguish "individual artist" from "gallery / space" without
-- forking the schema. Direct sequel to 0026_drop_venues.sql: v1
-- treats galleries as artists everywhere downstream (same routes,
-- same admin queue, same artwork model), but we still want to
-- surface the difference on the profile page + adjust some
-- onboarding copy.
--
-- Adding a single enum column beats a dedicated `venues` table:
-- no route duplication, no admin fork, no ownership ambiguity
-- with `artist_locations`. If gallery-specific behaviour ever
-- needs richer data (opening hours, staff, ticket links), we
-- extend from here rather than resurrecting the T-081 shape.
--
-- Default is 'individual' so existing rows keep working. The
-- CHECK constraint pins the v1 vocabulary — extend the list with
-- a follow-up ALTER when a third type earns its keep.

ALTER TABLE artists
    ADD COLUMN entity_type text NOT NULL DEFAULT 'individual'
        CHECK (entity_type IN ('individual', 'gallery'));

CREATE INDEX artists_entity_type_idx ON artists (entity_type)
    WHERE deleted_at IS NULL;
