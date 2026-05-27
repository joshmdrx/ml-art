-- 0002_artworks.sql
-- artworks, artwork_images, artwork_embeddings, tags, artwork_tags.

CREATE TABLE artworks (
    id              uuid PRIMARY KEY DEFAULT gen_random_uuid(),
    artist_id       uuid NOT NULL REFERENCES artists(id),
    title           text,
    description     text,
    year_created    integer,
    medium          text,
    dimensions      jsonb,
    price_cents     bigint,
    currency        text NOT NULL DEFAULT 'USD',
    availability    text NOT NULL DEFAULT 'available'
        CHECK (availability IN ('available', 'sold', 'not_for_sale', 'inquire')),
    external_url    text,
    status          text NOT NULL DEFAULT 'draft'
        CHECK (status IN ('draft', 'published', 'archived')),
    is_demo         boolean NOT NULL DEFAULT false,
    -- tsvector for keyword search; refreshed by trigger below.
    search_tsv      tsvector,
    created_at      timestamptz NOT NULL DEFAULT now(),
    updated_at      timestamptz NOT NULL DEFAULT now(),
    deleted_at      timestamptz,
    published_at    timestamptz
);

CREATE INDEX artworks_artist_status_idx ON artworks (artist_id, status)
    WHERE deleted_at IS NULL;
CREATE INDEX artworks_published_idx ON artworks (status, published_at DESC)
    WHERE deleted_at IS NULL;
CREATE INDEX artworks_search_tsv_idx ON artworks USING gin (search_tsv);
CREATE INDEX artworks_is_demo_idx ON artworks (is_demo);

CREATE FUNCTION artworks_search_tsv_refresh() RETURNS trigger
LANGUAGE plpgsql AS $$
BEGIN
    NEW.search_tsv :=
        setweight(to_tsvector('english', coalesce(NEW.title, '')), 'A') ||
        setweight(to_tsvector('english', coalesce(NEW.medium, '')), 'B') ||
        setweight(to_tsvector('english', coalesce(NEW.description, '')), 'C');
    RETURN NEW;
END;
$$;

CREATE TRIGGER artworks_search_tsv_trg
    BEFORE INSERT OR UPDATE OF title, medium, description ON artworks
    FOR EACH ROW EXECUTE FUNCTION artworks_search_tsv_refresh();

-- ─────────────────────────────────────────────────────────────────────────────
-- artwork_images: one row per uploaded original. Variants served via CDN.
-- ─────────────────────────────────────────────────────────────────────────────

CREATE TABLE artwork_images (
    id                  uuid PRIMARY KEY DEFAULT gen_random_uuid(),
    artwork_id          uuid NOT NULL REFERENCES artworks(id) ON DELETE CASCADE,
    s3_key              text NOT NULL,
    width               integer,
    height              integer,
    is_primary          boolean NOT NULL DEFAULT false,
    display_order       integer NOT NULL DEFAULT 0,
    moderation_status   text NOT NULL DEFAULT 'pending'
        CHECK (moderation_status IN ('pending', 'approved', 'rejected')),
    created_at          timestamptz NOT NULL DEFAULT now()
);

CREATE INDEX artwork_images_artwork_order_idx
    ON artwork_images (artwork_id, display_order);

-- Exactly one primary image per artwork (when set).
CREATE UNIQUE INDEX artwork_images_one_primary_idx
    ON artwork_images (artwork_id) WHERE is_primary;

-- ─────────────────────────────────────────────────────────────────────────────
-- artwork_embeddings: separate table so we can A/B models or re-embed safely.
-- ─────────────────────────────────────────────────────────────────────────────

CREATE TABLE artwork_embeddings (
    artwork_id      uuid NOT NULL REFERENCES artworks(id) ON DELETE CASCADE,
    model_name      text NOT NULL,
    model_version   text NOT NULL,
    embedding       vector(1024) NOT NULL,
    created_at      timestamptz NOT NULL DEFAULT now(),
    PRIMARY KEY (artwork_id, model_name, model_version)
);

-- HNSW index for the current production model. When swapping models, create a
-- new index for the new (model_name, model_version) pair and drop the old.
CREATE INDEX artwork_embeddings_hnsw_jina_clip_v2_idx
    ON artwork_embeddings
    USING hnsw (embedding vector_cosine_ops)
    WHERE model_name = 'jinaai/jina-clip-v2';

-- ─────────────────────────────────────────────────────────────────────────────
-- tags
-- ─────────────────────────────────────────────────────────────────────────────

CREATE TABLE tags (
    id        uuid PRIMARY KEY DEFAULT gen_random_uuid(),
    slug      text UNIQUE NOT NULL,
    label     text NOT NULL,
    category  text CHECK (category IN ('medium', 'style', 'mood', 'subject', 'technique'))
);

CREATE TABLE artwork_tags (
    artwork_id  uuid NOT NULL REFERENCES artworks(id) ON DELETE CASCADE,
    tag_id      uuid NOT NULL REFERENCES tags(id) ON DELETE CASCADE,
    PRIMARY KEY (artwork_id, tag_id)
);

CREATE INDEX artwork_tags_tag_idx ON artwork_tags (tag_id);
