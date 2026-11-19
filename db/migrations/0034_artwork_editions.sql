-- 0034_artwork_editions.sql — T-090
--
-- Limited editions + checkout holds for the marketplace.
--
-- Stock model: an artwork has a capacity of COALESCE(edition_size, 1) —
-- NULL edition_size is a one-off original, which is how every artwork
-- behaved before this migration. Copies sold =
--   live orders (paid / shipped / delivered / disputed)
-- + editions_sold_offline (copies the artist sold elsewhere — fairs,
--   galleries — recorded in the studio).
-- `availability` stays the single public truth: for an artwork on sale
-- the API derives 'sold' (= sold out) vs 'available' from the counts.
--
-- Holds: a `pending` order reserves one copy until hold_expires_at
-- (the Stripe Checkout Session's expiry plus a grace period for the
-- webhook to land), so two buyers can't both pay for the last copy.
-- NULL on orders created before this migration = no hold.
--
-- Prod: apply manually via psql BEFORE deploying the api that reads
-- these columns (see CLAUDE.md → production migrations).

ALTER TABLE artworks
    ADD COLUMN edition_size integer
        CHECK (edition_size IS NULL OR edition_size BETWEEN 2 AND 10000),
    ADD COLUMN editions_sold_offline integer NOT NULL DEFAULT 0
        CHECK (editions_sold_offline >= 0),
    ADD CONSTRAINT artworks_offline_within_edition
        CHECK (editions_sold_offline <= COALESCE(edition_size, 0));

ALTER TABLE orders
    ADD COLUMN hold_expires_at timestamptz;

-- Hot path for the hold count at checkout: live pending holds per artwork.
CREATE INDEX orders_artwork_live_idx
    ON orders (artwork_id, status)
    WHERE status IN ('pending', 'paid', 'shipped', 'delivered', 'disputed');
