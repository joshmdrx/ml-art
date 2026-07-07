# Marketplace — plan

**Status:** scoped, not built. Trigger to start = ≥50% of the first
5–10 onboarded artists say they'd rather sell direct than run
inquiry-only. Until then, the inquiry flow handles offline settlement
fine.

**Owner:** Josh. Est. build for a solo builder: **4–6 weeks focused
work** on top of a ~2-week legal T&Cs prep that can run in parallel.

**Related:**
- `decisions.md` should get an entry the day we commit to build
  (payment provider choice, commission %, whether we go Stripe Tax)
- `TODO.md` — the M-01…M-11 tickets below live there
- `docs/e2e-coverage.md` — buy-happy-path + refund + mark-shipped
  will be new register rows

---

## Why marketplace at all

Wander's core value prop today is *discovery + direct inquiry*. Direct
sales is a natural extension for artists who:

- Don't have their own e-commerce site
- Don't want to run Shopify + a domain + payment infra
- Trust Wander to handle checkout + payout + basic dispute infra
- Want the Wander URL to be their store

Not a marketplace-first pitch. This is a bolt-on for the artists who
want it. Some artists will stick with inquiry-only forever, and
that's fine.

---

## Payment provider — Stripe Connect Express

**Decision:** Stripe Connect Express.

Why not the alternatives:
- **Stripe Standard** — artist manages their own Stripe dashboard.
  Great for pros; wrong for our audience (small artists don't want
  to think about it).
- **PayPal** — no Connect equivalent that handles UK VAT well.
- **Direct bank transfer / Wise** — no dispute infrastructure, no
  chargeback protection, doesn't scale.
- **Shopify Payments** — puts Shopify in the middle. Defeats the
  point.

Express account gives us:
- Artist does a ~5-min KYC (bank details, ID) inside Stripe's hosted
  flow. We link them via `stripe_account_id` on `artists`.
- Wander is the platform of record — we handle disputes, refunds,
  1099-K equivalents.
- Automatic UK VAT + international tax handling (Stripe Tax add-on,
  costs 0.5% per successful charge).
- Split payments via `PaymentIntent.transfer_data.destination` — one
  charge on the buyer's card, Stripe splits + routes commission to
  Wander's platform account, artist gets the balance minus fees.

## Commission model

**Recommended v1:** 15% Wander commission (down from Artfinder's ~33%
and Saatchi's ~30%, up from Etsy's 6.5%).

Total buyer pays for a £500 work:
- **£500** paid via card
- **£500 × 15% = £75** Wander commission
- **£8** Stripe card fee (1.5% + 20p domestic UK)
- **£2.50** Stripe Tax (0.5%)
- **£414.50** to artist

Levers to revisit before launch:
- **Free tier for first N sales** — artists onboard easier ("try it
  free")
- **Tier by artwork value** — 10% for < £250, 15% for £250–£2000,
  12% for > £2000 (Artfinder does something similar)
- **Subscription instead of per-sale** — £15/mo, no per-sale
  commission. Higher-friction to sell, easier to model.

Don't over-engineer this pre-launch. Ship 15% flat, adjust once we
see conversion data.

---

## Database migrations

Numbered from where the tree is when we build (0028+ probably).

```sql
-- 0028_marketplace_orders.sql
CREATE TABLE orders (
    id                     uuid PRIMARY KEY DEFAULT gen_random_uuid(),
    buyer_user_id          uuid NOT NULL REFERENCES users(id),
    artwork_id             uuid NOT NULL REFERENCES artworks(id),
    artist_id              uuid NOT NULL REFERENCES artists(id),

    -- Money in canonical GBP pence (matches T-080).
    amount_cents_gbp       bigint NOT NULL CHECK (amount_cents_gbp > 0),
    commission_cents_gbp   bigint NOT NULL CHECK (commission_cents_gbp >= 0),
    stripe_fee_cents_gbp   bigint,   -- known after charge succeeds
    payout_cents_gbp       bigint,   -- amount_cents - commission - stripe_fee

    stripe_payment_intent_id     text UNIQUE,
    stripe_checkout_session_id   text UNIQUE,
    stripe_transfer_id           text,

    status text NOT NULL CHECK (status IN (
        'pending',      -- checkout session created, awaiting payment
        'paid',         -- payment_intent.succeeded
        'shipped',      -- artist marked shipped
        'delivered',    -- buyer confirmed / auto after N days
        'cancelled',    -- buyer cancelled before shipped (refunded)
        'refunded',     -- admin-initiated refund after paid
        'disputed'      -- chargeback filed
    )),

    -- Fulfilment. Captured at checkout, updated by artist.
    shipping_address       jsonb NOT NULL,   -- {line1, line2, city, postal, country}
    tracking_carrier       text,
    tracking_number        text,
    shipped_at             timestamptz,
    delivered_at           timestamptz,
    refunded_at            timestamptz,
    refund_reason          text,

    created_at             timestamptz NOT NULL DEFAULT now(),
    updated_at             timestamptz NOT NULL DEFAULT now()
);

CREATE INDEX orders_buyer_idx ON orders (buyer_user_id, created_at DESC);
CREATE INDEX orders_artist_idx ON orders (artist_id, created_at DESC);
CREATE INDEX orders_status_idx ON orders (status)
    WHERE status IN ('paid', 'shipped', 'disputed');   -- admin queues

-- 0029_marketplace_artist_stripe.sql
ALTER TABLE artists
    ADD COLUMN stripe_account_id       text UNIQUE,
    ADD COLUMN stripe_charges_enabled  boolean NOT NULL DEFAULT false,
    ADD COLUMN stripe_payouts_enabled  boolean NOT NULL DEFAULT false,
    ADD COLUMN stripe_onboarded_at     timestamptz;

ALTER TABLE artworks
    ADD COLUMN weight_grams        integer CHECK (weight_grams > 0),
    ADD COLUMN ships_from_country  text;   -- ISO-3166 alpha-2

-- 0030_marketplace_webhook_events.sql
CREATE TABLE stripe_webhook_events (
    event_id      text PRIMARY KEY,   -- Stripe evt_* id, dedup key
    event_type    text NOT NULL,
    order_id      uuid REFERENCES orders(id),
    processed_at  timestamptz NOT NULL DEFAULT now(),
    raw_payload   jsonb NOT NULL
);
```

---

## Order state machine

```
    ┌─────────┐   checkout.session.completed
    │ pending │──────────────────────────────┐
    └─────────┘                              ▼
         │                                ┌──────┐   artist marks shipped
         │ buyer abandons                 │ paid │───────────────────────┐
         ▼                                └──────┘                       ▼
    ┌───────────┐                             │                    ┌─────────┐
    │ cancelled │                             │ buyer cancels      │ shipped │
    │(auto 24h) │                             │   within 24h       └─────────┘
    └───────────┘                             ▼                         │
                                        ┌───────────┐                    │
                                        │ cancelled │                    │ auto 14d
                                        │ (refunded)│                    │ or buyer confirms
                                        └───────────┘                    ▼
                                                                   ┌───────────┐
                                                                   │ delivered │
                                                                   └───────────┘

    any-time transitions:
      any → disputed  (charge.dispute.created webhook)
      any → refunded  (admin action, fires Stripe refund + webhook confirms)
```

State transitions:
- `pending → paid` — Stripe webhook `checkout.session.completed`
- `pending → cancelled` — cron sweep, > 24h with no payment
- `paid → shipped` — artist action from `/studio/orders/[id]`
- `paid → cancelled` — buyer action from `/orders/[id]`, fires refund
- `shipped → delivered` — buyer action OR cron sweep 14d after shipped
- `paid|shipped|delivered → refunded` — admin action
- Any → `disputed` — Stripe `charge.dispute.created`

Never automatic refunds. Art is subjective; disputes are human.

---

## Idempotency

Three seams — all replay-safe:

1. **Checkout creation** — if `(buyer_user_id, artwork_id)` has a
   `pending` order < 30min old, return that instead of creating a new
   one. Prevents double-checkout when a user clicks "Buy" twice.
2. **Stripe webhooks** — `INSERT ON CONFLICT DO NOTHING` on
   `stripe_webhook_events.event_id`. Stripe retries; our handler is
   replay-idempotent. Same pattern as T-054's inbound-email dedup on
   `inbound_message_id`.
3. **Refund** — dedupe on `(order_id, refund_reason)`. An admin
   double-clicking Refund doesn't fire two refunds. Small unique
   partial index.

Copy the exact shape from `webhooks.rs` (T-054 inbound handler) —
that's a known-good template.

---

## Buyer flow

New surfaces:
- **Artwork detail** — new "Buy" button next to Inquire (visible only
  when artist has `stripe_charges_enabled=true` AND artwork has
  price + dimensions + weight)
- **`/artworks/[id]/buy`** — shipping address form + summary
- **Stripe Checkout** (hosted) — buyer completes payment
- **`/orders/[id]`** — post-checkout order confirmation
- **`/orders`** — buyer's list of past orders (M-11)

Address handling:
- Capture shipping address on our side (needed to compute shipping
  cost + display to artist).
- Also pass to Stripe as `shipping.address` on the PaymentIntent so
  Stripe's fraud + card-check has it.
- V1: single-address per order, no address book. Nice-to-have later.

---

## Artist flow

**Onboarding extension** (new step or optional 6th step):
- "Enable direct sales" panel
- Kicks off Stripe Connect Express hosted onboarding via
  `/v1/studio/stripe/onboarding-link`
- Return URL: `/studio/settings/payouts`
- Webhook `account.updated` updates `stripe_charges_enabled` +
  `stripe_payouts_enabled` when Stripe finishes KYC

**Per-artwork requirements** (to be *sellable*, on top of publishable):
- Price + currency ✓ (existing)
- Dimensions ✓ (existing, T-070)
- Weight (grams) — new field
- Ships-from country — new field, defaults to artist's `country`

**Studio fulfilment surface** — `/studio/orders`:
- List of orders in each status
- Order detail: buyer name, shipping address, message, Mark Shipped
  button (opens dialog: carrier + tracking number)
- Once shipped, artist sees delivery status + payout timing

---

## Admin surface

New under `/admin`:
- **`/admin/orders`** — list, filter by status, sort by amount
- **`/admin/orders/[id]`** — order detail, "Refund" button (with
  reason picker), "Open in Stripe" deep-link, dispute-evidence
  upload delegates to Stripe's own UI (deep link)
- **`/admin/stats`** — extend with sales tiles: GMV, commission
  earned, refund rate, dispute rate
- **Auto-flags** for admin attention:
  - Unfulfilled orders > N days old
  - Any dispute
  - Refund rate > 5% over trailing 30 days

---

## Notifications (Resend, extend existing pattern)

New templates in `core::templates`:

| Recipient | Trigger | Purpose |
|---|---|---|
| Buyer | `paid` | Order confirmation, ETA |
| Buyer | `shipped` | Tracking link |
| Buyer | 14d after `shipped` | "Did it arrive?" nudge |
| Artist | `paid` | Sale notification, buyer address, ship within N days |
| Artist | 3d after `paid` (not shipped) | Ship reminder |
| Admin | First N sales, plus any > £1000 | Awareness |
| Admin | `disputed` | Chargeback filed |
| Buyer | `refunded` | Refund confirmation |
| Artist | `refunded` | Refund processed (order pulled back) |

Same `JobEvent` + Resend pattern as inquiry emails. Zero new infra.

---

## Refunds + disputes

**Refunds:**
- Admin-only, from `/admin/orders/[id]`
- Reason picker (defective / not-as-described / non-delivery /
  artist-cancelled / other)
- Fires `stripe.refunds.create(payment_intent, reverse_transfer=true)`
- Webhook `refund.updated` confirms → order → `refunded`
- Notifications to buyer + artist

**Disputes:**
- Stripe webhook `charge.dispute.created` → order → `disputed`
- Admin gets alerted (SNS + email)
- Admin uploads evidence via Stripe dashboard (deep link from our
  admin page — we don't build the evidence UI in v1)
- Stripe `charge.dispute.closed` → order stays `disputed` (record
  the outcome in `refund_reason`)

---

## Legal + tax

**Non-trivial** — do NOT skip.

Before we accept money on behalf of third parties:

- **Marketplace T&Cs** — Wander is the platform, artist is
  seller-of-record. Defines commission structure, when payouts
  happen, dispute liability.
- **Buyer T&Cs** — conditions of sale, return policy.
- **Artist agreement** — accepted at Stripe Connect onboarding time.
  Covers commission %, chargeback liability if dispute is lost,
  representation warranties (they own the work, etc.).
- **UK e-commerce regulations** — Consumer Rights Act + Consumer
  Contracts Regulations 2013. 14-day cooling-off window for
  distance sales UNLESS the item is bespoke/personalised. Original
  art likely qualifies for exemption but *needs a lawyer's read*.
- **VAT** — if Wander's turnover < £85k, we can be VAT-exempt as a
  platform. Stripe Tax handles per-artist VAT if the artist crosses
  their own threshold. UK-specific — international expansion is a
  future problem.

**Recommend:** buy a UK marketplace-lawyer T&Cs template (~£1-2k).
Alternatives: Sparqa Legal, Rocket Lawyer templates + a solicitor
review. **Don't DIY this.**

---

## E2E coverage plan

New Playwright specs (all against test-fixture seam extended for
marketplace, or against a Stripe test-mode API):

| Spec | What it asserts |
|---|---|
| `buy-happy-path` | signed-in buyer, artwork with all fields, redirected to Stripe test checkout, back → order confirmation renders |
| `buy-blocked-when-artist-not-onboarded` | Buy button hidden when `stripe_charges_enabled=false` |
| `buy-blocked-when-artwork-missing-fields` | Buy button hidden when weight or ships_from_country missing |
| `studio-mark-shipped` | signed-in artist opens order, enters tracking, submits, order status flips |
| `admin-refund` | signed-in admin refunds an order, both buyer + artist notified (assert via inquiry_replies / notifications table) |
| `buyer-orders-page` | `/orders` lists the current user's orders, correct status per row (M-11 spec) |

Fixture seam extensions needed (`api/crates/api-search/src/testfixtures.rs`):
- `POST /v1/testfixtures/order` — insert an order in any state,
  optionally with a fake stripe_payment_intent_id
- `POST /v1/testfixtures/enable-payouts` — flip an artist's
  stripe_charges_enabled to true without going through Stripe onboarding

Stripe test mode: use their fixed test cards (`4242 4242 4242 4242`
for success) so we don't need to stub Stripe.

---

## Ticket breakdown — M-01..M-11

Filed in TODO.md when we commit to build. Ordering matches build
sequence (each depends roughly on the previous).

| Ticket | Description | Days |
|---|---|---|
| **M-01** | Stripe Connect Express onboarding (artist side): hosted-link endpoint, `stripe_account_id` on artists, `account.updated` webhook | 3 |
| **M-02** | Orders + webhook_events schema + Rust models | 1 |
| **M-03** | Stripe webhook handler + idempotency on `event_id` | 2 |
| **M-04** | Checkout session creation endpoint + address capture | 2 |
| **M-05** | Buy button + `/artworks/[id]/buy` page + `/orders/[id]` confirmation | 3 |
| **M-06** | Studio sales dashboard (`/studio/orders`, mark-shipped flow) | 3 |
| **M-07** | Notification templates + JobEvent variants (all recipients) | 2 |
| **M-08** | Admin `/admin/orders` + refund flow + dispute banner | 3 |
| **M-09** | Legal — T&Cs template + solicitor review (external, ~2 weeks in parallel with build) | — |
| **M-10** | E2E specs — buy happy path, mark-shipped, refund | 2 |
| **M-11** | Buyer `/orders` list page (their purchase history) | 1 |
| | **Total build** | **~22 days ≈ 4-5 weeks** |

---

## Sequencing / trigger to start

1. Onboard 5–10 real artists first via the existing inquiry flow.
2. Ask each: "would you sell direct via Wander if I built checkout?"
3. If ≥50% yes: commit to build, file M-01…M-11 tickets, kick off
   the legal T&Cs work in parallel.
4. If <50% yes: keep inquiry-only. Offline settlement between artist
   and buyer stays fine at low volume. Revisit annually.

Pre-launch marketplace = premature commitment to a business model
we haven't validated. Same as why T-081 (venues) got reverted —
build after you know it's wanted.
