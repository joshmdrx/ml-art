-- 0011_artist_locations.sql
-- T-038 G1 — per-artist physical locations.
--
-- Until now `artists.city/country/lat/lng` captured one "based in" point
-- per artist. Useful for a fuzzy "London painters" pin but not for the
-- in-person-discovery loop the product cares about: a viewer wants to
-- know where they can actually go see this artist's work tomorrow.
--
-- This table is a row per such place: a gallery the artist is represented
-- by, or a studio they show by appointment. Shows / events as time-bound
-- entities are still post-v1 (`99-deferred.md` Phase 2); when they land
-- we migrate these rows into `spaces` + `space_artists` join rows.
--
-- Trust model: self-listed by the artist. The public surface shows a
-- "Listed by the artist" label on each pin (`decisions.md` 2026-05-28).
-- No admin moderation in v1.
--
-- Geocoding: address is captured raw at insert; an Inngest job
-- (`artist_location.geocode`) populates lat/lng/normalized_city/country
-- via Mapbox. Rows with NULL lat/lng are hidden from public surfaces
-- until the geocode lands — the studio UI shows them as "Locating…".

CREATE TABLE artist_locations (
    id              uuid PRIMARY KEY DEFAULT gen_random_uuid(),
    artist_id       uuid NOT NULL REFERENCES artists(id) ON DELETE CASCADE,
    -- 'gallery' covers anywhere an artist is represented (commercial,
    -- artist-run, project space). 'studio' is the artist's own space,
    -- typically open by appointment.
    kind            text NOT NULL
        CHECK (kind IN ('gallery', 'studio')),
    -- Display name of the venue: "Foo Gallery", "Open studio", etc.
    name            text NOT NULL,
    -- Street-level address as the artist typed it. The geocoder may
    -- normalize this; we keep the original verbatim for display.
    address         text NOT NULL,
    -- City + ISO 3166-1 alpha-2 country, populated by the geocoder.
    -- Nullable until the geocode lands.
    city            text,
    country         text,
    lat             double precision,
    lng             double precision,
    -- Optional outbound link — gallery's site, the venue's page, etc.
    website_url     text,
    -- Sort order within an artist's list. Hand-set in the studio UI.
    display_order   integer NOT NULL DEFAULT 0,
    -- Geocoding bookkeeping: NULL = never attempted, set = last attempt
    -- (success or failure). The job re-tries when address changes.
    geocoded_at     timestamptz,
    created_at      timestamptz NOT NULL DEFAULT now(),
    updated_at      timestamptz NOT NULL DEFAULT now(),
    deleted_at      timestamptz
);

-- Lookup by artist for studio UI + artist profile payloads.
CREATE INDEX artist_locations_artist_id_idx
    ON artist_locations (artist_id, display_order)
    WHERE deleted_at IS NULL;

-- Bbox queries from /v1/search?map=1. Composite (lat, lng) index works
-- for "WHERE lat BETWEEN $1 AND $2 AND lng BETWEEN $3 AND $4" scans —
-- pgvector is for embeddings, not 2-D points, and we don't expect the
-- row count to justify PostGIS for v1.
CREATE INDEX artist_locations_geo_idx
    ON artist_locations (lat, lng)
    WHERE lat IS NOT NULL
      AND lng IS NOT NULL
      AND deleted_at IS NULL;

-- City filter compatibility — matches `artists_city_idx` pattern so the
-- existing location-text filter on /v1/search can be extended to also
-- look at artist_locations.city without a sequential scan.
CREATE INDEX artist_locations_city_idx
    ON artist_locations (city)
    WHERE deleted_at IS NULL;

-- Bookkeeping for the geocode Inngest job: find rows that need
-- geocoding (never attempted, or address changed since last attempt).
CREATE INDEX artist_locations_geocode_pending_idx
    ON artist_locations (created_at)
    WHERE geocoded_at IS NULL AND deleted_at IS NULL;
