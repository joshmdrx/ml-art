-- 0001_init.sql
-- Extensions, users, artists.

CREATE EXTENSION IF NOT EXISTS "uuid-ossp";
CREATE EXTENSION IF NOT EXISTS "pgcrypto";
CREATE EXTENSION IF NOT EXISTS "vector";

-- ─────────────────────────────────────────────────────────────────────────────
-- users
-- ─────────────────────────────────────────────────────────────────────────────

CREATE TABLE users (
    id              uuid PRIMARY KEY DEFAULT gen_random_uuid(),
    clerk_user_id   text UNIQUE NOT NULL,
    email           text UNIQUE NOT NULL,
    display_name    text,
    avatar_url      text,
    is_artist       boolean NOT NULL DEFAULT false,
    is_admin        boolean NOT NULL DEFAULT false,
    created_at      timestamptz NOT NULL DEFAULT now(),
    updated_at      timestamptz NOT NULL DEFAULT now()
);

CREATE INDEX users_clerk_user_id_idx ON users (clerk_user_id);

-- ─────────────────────────────────────────────────────────────────────────────
-- artists
-- ─────────────────────────────────────────────────────────────────────────────

CREATE TABLE artists (
    id                          uuid PRIMARY KEY DEFAULT gen_random_uuid(),
    user_id                     uuid REFERENCES users(id),
    slug                        text UNIQUE NOT NULL,
    display_name                text NOT NULL,
    bio                         text,
    artist_statement            text,
    -- Geographic
    location                    text,                     -- free-text display
    city                        text,                     -- structured, geocoded
    country                     text,                     -- ISO 3166-1 alpha-2
    lat                         double precision,
    lng                         double precision,
    geocoded_at                 timestamptz,
    -- Links and prefs
    website_url                 text,
    socials                     jsonb NOT NULL DEFAULT '{}'::jsonb,
    commissioning_preferences   jsonb,
    inquiry_preferences         jsonb NOT NULL,
    status                      text NOT NULL DEFAULT 'pending'
        CHECK (status IN ('pending', 'active', 'paused', 'rejected')),
    created_at                  timestamptz NOT NULL DEFAULT now(),
    updated_at                  timestamptz NOT NULL DEFAULT now(),
    deleted_at                  timestamptz
);

CREATE INDEX artists_slug_idx ON artists (slug);
CREATE INDEX artists_status_idx ON artists (status) WHERE deleted_at IS NULL;
CREATE INDEX artists_user_id_idx ON artists (user_id);
CREATE INDEX artists_city_idx ON artists (city) WHERE deleted_at IS NULL;
CREATE INDEX artists_country_idx ON artists (country) WHERE deleted_at IS NULL;
CREATE INDEX artists_geo_idx ON artists (lat, lng)
    WHERE lat IS NOT NULL AND lng IS NOT NULL AND deleted_at IS NULL;
