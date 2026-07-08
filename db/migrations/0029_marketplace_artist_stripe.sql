-- 0029_marketplace_artist_stripe.sql — M-02
--
-- Fields that tie an artist to their Stripe Connect Express account
-- + fields on artworks that Stripe / the fulfilment flow need.
--
-- `stripe_account_id` is set once when the artist starts onboarding
-- (Stripe returns it from the account-create call). `charges_enabled`
-- + `payouts_enabled` are updated by the `account.updated` webhook
-- as Stripe finishes KYC / bank verification — both must be true
-- before an artwork is considered sellable, checked in the api-layer
-- gate.
--
-- On artworks: `weight_grams` needed for the shipping-cost calc a
-- future ticket may add (v1 flat rate is fine); `ships_from_country`
-- is what Stripe Tax uses for VAT calc + what we show buyers on the
-- artwork page.
--
-- All new columns are nullable / defaulted so this migration doesn't
-- retroactively invalidate existing artists or artworks.

ALTER TABLE artists
    ADD COLUMN stripe_account_id       text UNIQUE,
    ADD COLUMN stripe_charges_enabled  boolean NOT NULL DEFAULT false,
    ADD COLUMN stripe_payouts_enabled  boolean NOT NULL DEFAULT false,
    ADD COLUMN stripe_onboarded_at     timestamptz;

-- Partial index for the "who can sell?" queries. Skipped when both
-- flags are false (the majority pre-launch) — index only ~grows with
-- artists who've actually finished Stripe KYC.
CREATE INDEX artists_stripe_ready_idx ON artists (id)
    WHERE stripe_charges_enabled = true
      AND stripe_payouts_enabled = true
      AND deleted_at IS NULL;

ALTER TABLE artworks
    ADD COLUMN weight_grams       integer CHECK (weight_grams > 0),
    ADD COLUMN ships_from_country text;
