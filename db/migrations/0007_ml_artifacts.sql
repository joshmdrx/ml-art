-- 0007_ml_artifacts.sql
-- Import scrapes, LLM extraction artifacts, eval set.

CREATE TABLE import_sources (
    id              uuid PRIMARY KEY DEFAULT gen_random_uuid(),
    artwork_id      uuid REFERENCES artworks(id) ON DELETE CASCADE,
    source_url      text NOT NULL,
    source_type     text CHECK (source_type IN ('website', 'manual', 'wikiart_seed', 'met_seed')),
    scraped_at      timestamptz,
    metadata        jsonb
);

CREATE INDEX import_sources_artwork_idx ON import_sources (artwork_id);

CREATE TABLE llm_extraction_artifacts (
    id                  uuid PRIMARY KEY DEFAULT gen_random_uuid(),
    artwork_id          uuid REFERENCES artworks(id) ON DELETE CASCADE,
    input_text          text,
    input_image_url     text,
    output_json         jsonb,
    model               text,
    prompt_version      text,
    created_at          timestamptz NOT NULL DEFAULT now()
);

CREATE INDEX llm_extraction_artworks_idx
    ON llm_extraction_artifacts (artwork_id, created_at DESC);

-- ─────────────────────────────────────────────────────────────────────────────
-- eval_set: hand-curated ground-truth pairs for search/recommendation quality.
-- ─────────────────────────────────────────────────────────────────────────────

CREATE TABLE eval_set (
    id                      uuid PRIMARY KEY DEFAULT gen_random_uuid(),
    query_type              text NOT NULL
        CHECK (query_type IN ('text', 'image', 'image_modifier')),
    query_text              text,
    query_image_s3_key      text,
    modifiers               text[] NOT NULL DEFAULT ARRAY[]::text[],
    expected_artwork_ids    uuid[] NOT NULL,
    notes                   text,
    created_at              timestamptz NOT NULL DEFAULT now()
);

CREATE TABLE eval_runs (
    id              uuid PRIMARY KEY DEFAULT gen_random_uuid(),
    started_at      timestamptz NOT NULL DEFAULT now(),
    finished_at     timestamptz,
    git_sha         text,
    model_name      text NOT NULL,
    model_version   text NOT NULL,
    ndcg_at_10      real,
    n_queries       integer,
    per_query_json  jsonb       -- detail for the dashboard
);
