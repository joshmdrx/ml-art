-- 0003_collections.sql
-- user_collections, collection_artworks.

CREATE TABLE user_collections (
    id              uuid PRIMARY KEY DEFAULT gen_random_uuid(),
    user_id         uuid NOT NULL REFERENCES users(id),
    name            text NOT NULL,
    description     text,
    is_public       boolean NOT NULL DEFAULT false,
    share_id        text UNIQUE,
    created_at      timestamptz NOT NULL DEFAULT now(),
    updated_at      timestamptz NOT NULL DEFAULT now(),
    deleted_at      timestamptz
);

CREATE INDEX user_collections_user_id_idx ON user_collections (user_id)
    WHERE deleted_at IS NULL;

CREATE TABLE collection_artworks (
    collection_id   uuid NOT NULL REFERENCES user_collections(id) ON DELETE CASCADE,
    artwork_id      uuid NOT NULL REFERENCES artworks(id) ON DELETE CASCADE,
    notes           text,           -- schema supports, UI v1.1
    display_order   integer,        -- schema supports, UI v1.1
    added_at        timestamptz NOT NULL DEFAULT now(),
    PRIMARY KEY (collection_id, artwork_id)
);
