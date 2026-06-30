-- T-081 — venues: galleries / shops / studio collectives as
-- discovery destinations.
--
-- Schema mirrors the artist_locations pattern (T-038) for fields
-- shared in spirit (address, geocoding), but venues are top-level
-- entities owned by their own user (a gallery owner) rather than
-- per-artist rows. The consent flow happens via venue_artworks: a
-- venue invites an artwork; the artwork's owning artist accepts or
-- declines.
--
-- Design choices captured in TODO T-081 + decisions.md:
--   - Multiple venues per user (galleries with branches).
--   - One-direction consent: venue invites artwork → artist
--     accepts/declines. Bidirectional volunteer-flow deferred.
--   - Status starts `pending_review`; admin (T-083) flips to
--     `active` before public listing.
--   - Single concept per row in venue_artworks: "this artwork is at
--     this venue." No "on display" vs "for sale" distinction in v1.
--   - venue_artworks.artwork_id FK on delete cascade — the artwork
--     no longer exists, so it can't be at a venue.

CREATE TABLE venues (
    id                  uuid PRIMARY KEY DEFAULT gen_random_uuid(),
    slug                text NOT NULL,
    name                text NOT NULL,
    -- kind covers the spectrum from commercial gallery to artist-run
    -- shop to café with a curated wall. `other` is the escape hatch
    -- — admins can re-classify before approving if a more specific
    -- category emerges from real use.
    kind                text NOT NULL
        CHECK (kind IN ('gallery', 'shop', 'studio_collective',
                        'cafe_collab', 'other')),
    about               text,
    -- Address fields mirror artist_locations: raw text at insert,
    -- geocoder normalises city / country / lat / lng.
    address             text,
    city                text,
    country             text,
    lat                 double precision,
    lng                 double precision,
    geocoded_at         timestamptz,
    website_url         text,
    instagram_handle    text,
    -- Free-text opening info ("Tue–Sat 11–6"); structured-hours
    -- representation is deferred per TODO.
    opening_info        text,
    owner_user_id       uuid NOT NULL REFERENCES users(id),
    -- pending_review → active flip is the admin gate.
    status              text NOT NULL DEFAULT 'pending_review'
        CHECK (status IN ('pending_review', 'active', 'paused', 'declined')),
    created_at          timestamptz NOT NULL DEFAULT now(),
    updated_at          timestamptz NOT NULL DEFAULT now(),
    deleted_at          timestamptz
);

-- Slug uniqueness scoped to non-deleted rows so a delete-then-recreate
-- can reuse the slug.
CREATE UNIQUE INDEX venues_slug_active_idx
    ON venues (slug) WHERE deleted_at IS NULL;

CREATE INDEX venues_owner_idx
    ON venues (owner_user_id) WHERE deleted_at IS NULL;

-- Partial index for map-pin scans: only `active` venues with a pin
-- show on public surfaces.
CREATE INDEX venues_active_pins_idx
    ON venues (lat, lng)
    WHERE deleted_at IS NULL
      AND status = 'active'
      AND lat IS NOT NULL
      AND lng IS NOT NULL;

-- ─────────────────────────────────────────────────────────────────────────────
-- venue_artworks — many-to-many between venues and artworks, with
-- consent state.
--
-- The venue's owner invites; the artwork's artist accepts. Only
-- `accepted` rows appear on public surfaces. `requested_at` /
-- `decided_at` give analytics the response-time signal.
-- ─────────────────────────────────────────────────────────────────────────────

CREATE TABLE venue_artworks (
    venue_id        uuid NOT NULL REFERENCES venues(id) ON DELETE CASCADE,
    artwork_id      uuid NOT NULL REFERENCES artworks(id) ON DELETE CASCADE,
    status          text NOT NULL DEFAULT 'pending'
        CHECK (status IN ('pending', 'accepted', 'declined')),
    requested_at    timestamptz NOT NULL DEFAULT now(),
    decided_at      timestamptz,
    PRIMARY KEY (venue_id, artwork_id)
);

-- "Currently at:" reads on the artwork-detail page filter by artwork_id
-- + accepted; need the per-artwork index.
CREATE INDEX venue_artworks_artwork_accepted_idx
    ON venue_artworks (artwork_id) WHERE status = 'accepted';

-- Artist's pending-invitation inbox reads by venue's owning artist's
-- artworks (joined through artworks.artist_id). The
-- (status, requested_at) sort is the common path.
CREATE INDEX venue_artworks_status_requested_idx
    ON venue_artworks (status, requested_at DESC);
