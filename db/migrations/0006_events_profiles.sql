-- 0006_events_profiles.sql
-- Behavioral events + user taste profiles.
--
-- The spec calls for monthly partitioning on `events.occurred_at`. V1 ships
-- a single non-partitioned table; partitioning is a deferred follow-up
-- (see 99-deferred.md "Infrastructure & tooling"). The schema is the same
-- either way, so migrating later is a pg_partman / pg_pathman exercise.

CREATE TABLE events (
    id                      uuid PRIMARY KEY DEFAULT gen_random_uuid(),
    anonymous_id            uuid,
    user_id                 uuid REFERENCES users(id),
    event_name              text NOT NULL,
    event_schema_version    integer NOT NULL DEFAULT 1,
    occurred_at             timestamptz NOT NULL DEFAULT now(),
    session_id              uuid,
    properties              jsonb,
    context                 jsonb
);

CREATE INDEX events_user_occurred_idx ON events (user_id, occurred_at DESC);
CREATE INDEX events_anon_occurred_idx ON events (anonymous_id, occurred_at DESC);
CREATE INDEX events_name_occurred_idx ON events (event_name, occurred_at DESC);

-- ─────────────────────────────────────────────────────────────────────────────
-- user_profiles: derived taste embeddings + aggregates, refreshed by job.
-- ─────────────────────────────────────────────────────────────────────────────

CREATE TABLE user_profiles (
    user_id             uuid PRIMARY KEY REFERENCES users(id) ON DELETE CASCADE,
    taste_embedding     vector(1024),
    preferred_mediums   jsonb,
    price_range_seen    jsonb,
    color_affinity      jsonb,
    interaction_count   integer NOT NULL DEFAULT 0,
    last_active         timestamptz,
    profile_updated_at  timestamptz
);

-- Build the HNSW index only over rows that actually have an embedding.
CREATE INDEX user_profiles_taste_hnsw_idx
    ON user_profiles
    USING hnsw (taste_embedding vector_cosine_ops)
    WHERE taste_embedding IS NOT NULL;
