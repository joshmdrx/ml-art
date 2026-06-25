-- T-073 — medium taxonomy.
--
-- Adds `artworks.medium_category` — a controlled enum that drives the
-- search filter, the studio category select, and (downstream) T-057's
-- algorithmic neighbourhoods + T-061's taste calibrator.
--
-- The existing `medium text` column stays as-is and is repurposed as
-- the free-text "materials" field (e.g. "oil on linen, 90×60cm"). No
-- column rename — less churn, and "medium" still makes semantic sense
-- as a label for the specifics. Display combines both as
-- `Painting · Oil on linen`.
--
-- The category is nullable at the DB layer:
--   - drafts can lack one (artist hasn't decided yet)
--   - legacy rows from before this migration land NULL until backfilled
--     (see `scripts/backfill_medium_category.sh`)
--   - the studio UI nudges (but doesn't block) at draft → published
--     transition, mirroring T-070's dimensions soft-confirm pattern
--
-- The CHECK constraint pins the v1 list. Adding a category later is
-- an additive migration (`ALTER TABLE … DROP CONSTRAINT … ADD
-- CONSTRAINT … CHECK (… new value)`). Removing one is harder and
-- requires backfilling the rows that use it — deliberately structural.
-- Decision recorded in `decisions.md` 2026-06-23.

ALTER TABLE artworks
    ADD COLUMN medium_category text
    CHECK (medium_category IN (
        'painting',
        'drawing',
        'photography',
        'print',
        'sculpture',
        'mixed_media',
        'collage',
        'textile',
        'ceramic',
        'digital',
        'other'
    ));

-- Index for the search filter. Predicate is "WHERE medium_category =
-- ANY(...)" — btree handles equality + ANY-list cheaply. Partial on
-- non-null + non-deleted so legacy / drafted rows don't bloat the index.
CREATE INDEX artworks_medium_category_idx
    ON artworks (medium_category)
    WHERE medium_category IS NOT NULL AND deleted_at IS NULL;
