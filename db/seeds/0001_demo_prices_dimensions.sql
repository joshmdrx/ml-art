-- T-022 — backfill plausible prices + dimensions on existing demo
-- artwork rows. One-off (the seed script now writes these on
-- INSERT for new runs, see ml/ml_art/seed.py).
--
-- Idempotent: only updates rows that are currently NULL. Re-running
-- is a no-op. Safe to run before or after a `seed --reset`.
--
-- Scope: rows marked `is_demo = true` OR belonging to a `demo-*`
-- slug artist. The dual condition guards against a future demo
-- artist that wasn't flagged on the artwork row.
--
-- Values: deterministic per-row via `hashtext(id)` so the same
-- row gets the same price/size every run. Price quantised to the
-- nearest £10 so the UI shows tidy values (e.g. "$320") rather
-- than the noise of a uniform random ("$317.42"). Dimensions in
-- cm — matches the units `formatDimensions` displays.

UPDATE artworks
SET
  price_cents = COALESCE(
    price_cents,
    -- 50..2500 dollars, rounded to nearest 10, then to cents.
    (50 + ((abs(hashtext(id::text || ':price')) % 246)) * 10) * 100
  ),
  dimensions = COALESCE(
    dimensions,
    jsonb_build_object(
      'width',  20 + (abs(hashtext(id::text || ':w')) % 81),  -- 20..100
      'height', 25 + (abs(hashtext(id::text || ':h')) % 86),  -- 25..110
      'unit',   'cm'
    )
  )
WHERE (price_cents IS NULL OR dimensions IS NULL)
  AND (
    is_demo = true
    OR artist_id IN (SELECT id FROM artists WHERE slug LIKE 'demo-%')
  );
