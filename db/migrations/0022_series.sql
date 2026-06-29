-- 0022_series.sql
-- T-058 — Series concept for artists.
--
-- A `series` is an artist-curated grouping of their own artworks — Behance-
-- style "project format". One artwork can belong to at most one series (FK,
-- not many-to-many); the relationship is owned from the artwork side so
-- assigning / unassigning is a single column update.
--
-- Public-page integration (`?view=series` on the artist page) reads from
-- this table; the algorithmic neighbourhoods + taste-vector surfaces are
-- agnostic to it. A series might later become a clusterable / recommendable
-- entity in its own right (T-058 follow-up); v1 is just the studio +
-- artist-page wiring.

CREATE TABLE series (
    id                  uuid PRIMARY KEY DEFAULT gen_random_uuid(),
    artist_id           uuid NOT NULL REFERENCES artists(id),
    -- Per-artist unique kebab-case slug. URL shape:
    --   /artists/:artist-slug/series/:series-slug
    -- Two artists can both have a "blue-period" series — slug is only
    -- unique within the artist's namespace.
    slug                text NOT NULL,
    title               text NOT NULL,
    -- 500-char curatorial statement. Matches the artist bio shape.
    -- Plain text in v1; markdown is a follow-up if anyone asks.
    statement           text,
    -- Cover comes from the artist's own artworks (picker, not a separate
    -- upload path). NULL → public reads fall back to the first artwork's
    -- primary image. ON DELETE SET NULL so picking a cover then deleting
    -- the artwork doesn't break the series.
    cover_artwork_id    uuid REFERENCES artworks(id) ON DELETE SET NULL,
    created_at          timestamptz NOT NULL DEFAULT now(),
    updated_at          timestamptz NOT NULL DEFAULT now(),
    -- Soft-delete consistency with artworks. Public reads filter
    -- deleted_at IS NULL; studio reads include them when ?include_deleted=1.
    deleted_at          timestamptz
);

CREATE UNIQUE INDEX series_artist_slug_unique_idx
    ON series (artist_id, slug)
    WHERE deleted_at IS NULL;

CREATE INDEX series_artist_idx ON series (artist_id) WHERE deleted_at IS NULL;

-- ─────────────────────────────────────────────────────────────────────────────
-- artworks.series_id — one series per artwork
-- ─────────────────────────────────────────────────────────────────────────────
--
-- ON DELETE SET NULL: deleting a series un-series's its artworks rather
-- than cascading the delete. The artworks themselves stay intact; only
-- their series membership clears. Matches the spec call in `TODO.md`.

ALTER TABLE artworks
    ADD COLUMN series_id uuid REFERENCES series(id) ON DELETE SET NULL;

CREATE INDEX artworks_series_idx ON artworks (series_id)
    WHERE deleted_at IS NULL AND series_id IS NOT NULL;
