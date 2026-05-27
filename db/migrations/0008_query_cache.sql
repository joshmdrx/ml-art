-- 0008_query_cache.sql
-- Postgres-backed cache for text-query embeddings.
--
-- Search endpoints embed the user's text query at request time. The Jina
-- API call is the bottleneck (~100–300ms). Caching common queries amortizes
-- that to a single call per unique string.
--
-- Free alternative to Redis (Upstash / ElastiCache). Keyed by exact text
-- match; case + whitespace normalization happens at the application layer.

CREATE TABLE query_embedding_cache (
    query_text      text NOT NULL,
    model_name      text NOT NULL,
    model_version   text NOT NULL,
    embedding       vector(1024) NOT NULL,
    created_at      timestamptz NOT NULL DEFAULT now(),
    last_used_at    timestamptz NOT NULL DEFAULT now(),
    hit_count       integer NOT NULL DEFAULT 1,
    PRIMARY KEY (query_text, model_name, model_version)
);

-- Cleanup index: jobs that prune entries unused for >30 days hit this.
CREATE INDEX query_embedding_cache_last_used_idx
    ON query_embedding_cache (last_used_at);

-- Application contract:
--   On lookup, UPDATE last_used_at = now(), hit_count = hit_count + 1.
--   On miss, INSERT.
--   Scheduled Inngest job `query_cache.cleanup` runs daily:
--     DELETE FROM query_embedding_cache WHERE last_used_at < now() - interval '30 days';
