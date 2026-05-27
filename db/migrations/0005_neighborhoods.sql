-- 0005_neighborhoods.sql
-- Manually-curated neighborhoods for v1; clustering algorithm comes later.

CREATE TABLE neighborhoods (
    id                          uuid PRIMARY KEY DEFAULT gen_random_uuid(),
    slug                        text UNIQUE NOT NULL,
    name                        text NOT NULL,
    description                 text,
    -- v1: kind defaults to 'curated'. Phase 1 of the geographic track adds
    -- 'geographic' as another value. Algorithmic clustering would use 'semantic'.
    kind                        text NOT NULL DEFAULT 'curated'
        CHECK (kind IN ('curated', 'semantic', 'geographic')),
    cluster_centroid            vector(1024),
    representative_artwork_ids  uuid[] NOT NULL DEFAULT ARRAY[]::uuid[],
    artwork_count               integer NOT NULL DEFAULT 0,
    display_order               integer NOT NULL DEFAULT 0,
    is_featured                 boolean NOT NULL DEFAULT false,
    computed_at                 timestamptz,
    created_at                  timestamptz NOT NULL DEFAULT now(),
    updated_at                  timestamptz NOT NULL DEFAULT now()
);

CREATE INDEX neighborhoods_slug_idx ON neighborhoods (slug);
CREATE INDEX neighborhoods_featured_order_idx
    ON neighborhoods (is_featured DESC, display_order ASC);

CREATE TABLE neighborhood_artworks (
    neighborhood_id      uuid NOT NULL REFERENCES neighborhoods(id) ON DELETE CASCADE,
    artwork_id           uuid NOT NULL REFERENCES artworks(id) ON DELETE CASCADE,
    distance_to_centroid real,
    PRIMARY KEY (neighborhood_id, artwork_id)
);

CREATE INDEX neighborhood_artworks_dist_idx
    ON neighborhood_artworks (neighborhood_id, distance_to_centroid);
