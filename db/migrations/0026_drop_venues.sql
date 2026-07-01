-- T-081 rollback — drop the venues + venue_artworks tables.
--
-- Reverted after four sub-commits shipped: v1 will treat gallery /
-- shop owners as regular artists (using the existing `artist_locations`
-- table for their space). One entity type = one map source, one page
-- shape, no consent-flow scaffolding needed pre-launch.
--
-- Safe drop: no venues or venue_artworks rows were ever created in
-- prod between the migration landing and this rollback (venues
-- started as `pending_review` and no admin approvals happened).
-- Verify with `SELECT count(*) FROM venues, venue_artworks;` before
-- applying if the assumption ever bends.
--
-- See `decisions.md` 2026-07-01 for the rationale. The revived
-- venue concept — if it ever comes back — would probably absorb
-- `artist_locations` rather than sit alongside it.

DROP TABLE IF EXISTS venue_artworks;
DROP TABLE IF EXISTS venues;
