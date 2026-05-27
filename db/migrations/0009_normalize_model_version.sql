-- 0009_normalize_model_version.sql
-- T-024 + T-036 fold-in.
--
-- Until now, Python local seed wrote `model_version='local'` and the Rust
-- HTTP path was about to write `model_version='api'` — same model, two
-- labels. The two-label situation only became load-bearing as `T-036`
-- (the embedding pipeline for new artworks) starts writing from Rust.
--
-- Decided: pick `'v2'` and migrate both existing tables. After this
-- migration, both Python tooling and Rust handlers write `'v2'` for the
-- `jinaai/jina-clip-v2` model. The HNSW index in 0002 already filters
-- on `model_name`, so the label change is invisible to query plans.
--
-- Idempotent: re-running this migration is a no-op (no rows match the
-- WHERE clause after the first run).

UPDATE artwork_embeddings
   SET model_version = 'v2'
 WHERE model_name = 'jinaai/jina-clip-v2'
   AND model_version IN ('local', 'api');

UPDATE query_embedding_cache
   SET model_version = 'v2'
 WHERE model_name = 'jinaai/jina-clip-v2'
   AND model_version IN ('local', 'api');
