-- 0023_fx_rates.sql
-- T-080 — Currency-aware price filter, canonical GBP.
--
-- Today the price filter compares raw `price_cents` regardless of
-- currency, so a "Under £500" filter matches a $500 painting (~£395)
-- as if it were under £500. This migration adds:
--
--   1. `fx_rates` — one row per supported currency; `rate_to_gbp` is
--      what you multiply a price (in that currency's minor units) by
--      to get GBP minor units. GBP itself sits at 1.0.
--   2. `artworks.price_gbp_cents` — precomputed GBP value, indexed,
--      filtered against by `search.rs`. Maintained by:
--        - studio create / patch handlers at write time,
--        - the `JobEvent::FxRatesRefresh` job after a rate refresh.
--
-- Seed values are mid-2026 approximations so day-1 search works
-- without waiting for the first cron run. The values drift by
-- single-digit % over weeks; the cron (Frankfurter, ECB data) keeps
-- them current.

CREATE TABLE fx_rates (
    code         text PRIMARY KEY,
    -- 1 unit of `code` is worth this many GBP. GBP itself = 1.0.
    -- numeric(20, 10) gives plenty of precision for JPY-style small
    -- rates (0.0052…) and for USD/EUR alike.
    rate_to_gbp  numeric(20, 10) NOT NULL CHECK (rate_to_gbp > 0),
    fetched_at   timestamptz NOT NULL DEFAULT now()
);

-- Mid-2026 approximations. The FX-refresh job overwrites these on
-- its first run. Picked the six currencies our artwork-create flow
-- has historically accepted; new codes added when the cron sees them.
INSERT INTO fx_rates (code, rate_to_gbp) VALUES
    ('GBP', 1.0),
    ('USD', 0.79),
    ('EUR', 0.86),
    ('CAD', 0.58),
    ('AUD', 0.51),
    ('JPY', 0.0052);

-- ─────────────────────────────────────────────────────────────────────────────
-- artworks.price_gbp_cents — canonical GBP minor units for filtering.
-- ─────────────────────────────────────────────────────────────────────────────
-- Nullable: artworks with `price_cents IS NULL` (POA / inquire-only)
-- have no GBP value either. The price filter's `IS NOT NULL` predicate
-- already excludes these.

ALTER TABLE artworks ADD COLUMN price_gbp_cents bigint;

-- Backfill from the seeded rates. The math: price_cents is in the
-- artwork's own currency minor units; multiply by `rate_to_gbp` to
-- get GBP minor units. ROUND to bigint (no fractional pence).
--
-- For currencies not in fx_rates (unlikely, but defensive) the LEFT
-- JOIN leaves `price_gbp_cents` NULL — same as POA.
UPDATE artworks a
SET price_gbp_cents = ROUND(a.price_cents * fx.rate_to_gbp)::bigint
FROM fx_rates fx
WHERE fx.code = a.currency
  AND a.price_cents IS NOT NULL;

CREATE INDEX artworks_price_gbp_idx ON artworks (price_gbp_cents)
    WHERE deleted_at IS NULL AND price_gbp_cents IS NOT NULL;
