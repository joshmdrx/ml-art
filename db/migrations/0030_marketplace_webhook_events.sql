-- 0030_marketplace_webhook_events.sql — M-02
--
-- Idempotency table for Stripe webhooks. Every incoming
-- `POST /v1/webhooks/stripe` runs `INSERT ON CONFLICT DO NOTHING`
-- on `event_id`; if the row already exists, we ack without
-- processing.
--
-- Same shape as the T-054 inbound-email dedup on
-- `inquiry_replies.inbound_message_id` — Stripe retries webhooks
-- aggressively (>= 3× within an hour on 2xx-not-received), so
-- replay-idempotency is not optional.
--
-- `raw_payload` is kept for forensics. Not queryable in normal
-- flow; a debug read after a mysterious refund / dispute needs
-- the full body. Rows never deleted (small — bounded by API
-- surface, not user traffic).

CREATE TABLE stripe_webhook_events (
    event_id      text PRIMARY KEY,      -- Stripe's `evt_*` id
    event_type    text NOT NULL,          -- e.g. checkout.session.completed
    order_id      uuid REFERENCES orders(id),
    processed_at  timestamptz NOT NULL DEFAULT now(),
    raw_payload   jsonb NOT NULL
);

-- Occasional "list recent events by type" for the operator; cheap.
CREATE INDEX stripe_webhook_events_type_idx
    ON stripe_webhook_events (event_type, processed_at DESC);
