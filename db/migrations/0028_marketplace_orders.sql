-- 0028_marketplace_orders.sql — M-02
--
-- Core orders table for the direct-sales marketplace. See
-- plans/marketplace.md for the full design context.
--
-- Money storage:
--   - `amount_cents_gbp` is what the buyer paid (matches T-080's
--     canonical GBP-in-pence convention).
--   - `commission_cents_gbp` is what Wander keeps; recorded at
--     order creation so historical rate changes don't retroactively
--     rewrite past orders.
--   - `stripe_fee_cents_gbp` + `payout_cents_gbp` are populated
--     after the charge succeeds (the exact fee is on Stripe's
--     balance-transaction, which we fetch in the webhook handler).
--
-- Status vocabulary (see state machine in plans/marketplace.md):
--   pending    — checkout session created, awaiting payment
--   paid       — payment_intent.succeeded fired
--   shipped    — artist marked shipped, tracking captured
--   delivered  — buyer confirmed OR cron sweep 14d after shipped
--   cancelled  — buyer cancelled before shipped (refunded)
--   refunded   — admin-initiated refund after paid
--   disputed   — chargeback filed (Stripe charge.dispute.created)

CREATE TABLE orders (
    id                        uuid PRIMARY KEY DEFAULT gen_random_uuid(),

    -- Parties.
    buyer_user_id             uuid NOT NULL REFERENCES users(id),
    artwork_id                uuid NOT NULL REFERENCES artworks(id),
    artist_id                 uuid NOT NULL REFERENCES artists(id),

    -- Money — canonical GBP pence throughout, same as artworks.price_gbp_cents.
    amount_cents_gbp          bigint NOT NULL CHECK (amount_cents_gbp > 0),
    commission_cents_gbp      bigint NOT NULL CHECK (commission_cents_gbp >= 0),
    stripe_fee_cents_gbp      bigint,
    payout_cents_gbp          bigint,

    -- Stripe identifiers. `payment_intent_id` is the truth for
    -- refunds + reconciliation; `checkout_session_id` is the truth
    -- for "did the buyer complete checkout". Nullable during the
    -- brief window between DB write and Stripe API call succeeding.
    stripe_payment_intent_id  text UNIQUE,
    stripe_checkout_session_id text UNIQUE,
    stripe_transfer_id        text,

    -- State machine. Enum-y via CHECK (Postgres enums are a hassle
    -- to migrate).
    status text NOT NULL CHECK (status IN (
        'pending', 'paid', 'shipped', 'delivered',
        'cancelled', 'refunded', 'disputed'
    )),

    -- Fulfilment. Address captured at checkout; tracking captured
    -- when the artist marks shipped.
    shipping_address          jsonb NOT NULL,
    tracking_carrier          text,
    tracking_number           text,
    shipped_at                timestamptz,
    delivered_at              timestamptz,

    -- Refund + dispute.
    refunded_at               timestamptz,
    refund_reason             text,

    created_at                timestamptz NOT NULL DEFAULT now(),
    updated_at                timestamptz NOT NULL DEFAULT now()
);

-- Buyer's `/orders` list — cheap lookup by user, newest first.
CREATE INDEX orders_buyer_idx ON orders (buyer_user_id, created_at DESC);

-- Artist's studio sales list — same shape but keyed on artist.
CREATE INDEX orders_artist_idx ON orders (artist_id, created_at DESC);

-- Admin queues — only care about the actionable statuses. Partial
-- index keeps it small (settled orders are the majority in steady
-- state).
CREATE INDEX orders_status_idx ON orders (status)
    WHERE status IN ('paid', 'shipped', 'disputed');
