-- 0010_studio_user_artist_link.sql
-- T-011 Phase 1 prep.
--
-- Until now `artists.user_id` was nullable and unused — the seeded demo
-- artists (WikiArt etc.) have no Clerk user behind them. T-011 turns the
-- column into the actual ownership boundary: every `/v1/studio/*`
-- endpoint resolves the caller's `User` → `artists.user_id = $user_id`
-- → the artist they own.
--
-- For v1 a Clerk user is *at most* one artist (the spec assumes one
-- portfolio per identity; gallery-as-multiple-artists is a v2+ shape
-- in 99-deferred.md). Enforce that with a partial UNIQUE index. NULL
-- entries (seeded demo artists) are explicitly allowed — Postgres treats
-- NULL as distinct in unique indexes by default, but the `WHERE
-- user_id IS NOT NULL` clause makes the intent explicit and lets us
-- add a comment.
--
-- This is a constraint addition, not a backfill — existing rows are
-- unaffected (they all have NULL user_id).

CREATE UNIQUE INDEX artists_user_id_unique_idx
    ON artists (user_id)
    WHERE user_id IS NOT NULL;
