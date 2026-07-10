# M-09 Legal — action items for Josh

External work (2-week lead-time). Kick this off NOW so the T&Cs are
ready by the time M-08 (admin refund flow) lands. Waiting on legal
after everything else is built is the classic launch-blocker.

## What you need

Three documents:

1. **Marketplace terms of service** — Wander is the platform, the
   artist is seller-of-record. Governs commission structure, when
   payouts happen, dispute liability if buyer initiates a chargeback.
2. **Buyer terms of purchase** — conditions of sale, when the sale
   is final, return / refund policy (per UK Consumer Rights Act 2015
   + Consumer Contracts Regulations 2013).
3. **Artist seller agreement** — accepted at Stripe Connect onboarding.
   Commission %, payout timing, chargeback liability if the buyer
   disputes and wins, warranty that the artist owns the work.

## Provisional decisions to give the lawyer

- Wander commission: **15% flat** on gross sale price
- Payout timing: Stripe standard schedule (2-7 days after order status
  `delivered` or 14 days after `shipped` if buyer doesn't confirm)
- Refund policy: **admin-initiated only**, no automatic buyer refund
- Territory: **UK only** for v1 (buyers in UK, artists in UK). Expand
  later.
- Consumer Rights Act 2015: 14-day cooling-off applies unless the
  work is "bespoke or personalised" — the lawyer needs to advise
  whether "original art bought from an artist" qualifies for the
  exemption. If not, we owe a 14-day return window on every sale.
- VAT: Wander's turnover is < £85k pre-launch → we're VAT-exempt.
  Stripe Tax handles per-artist VAT if any individual artist crosses
  their own threshold.

## Where to go — cheapest→safest (2026)

There is **no** fully-automated service that safely covers a marketplace
taking third-party money. Generators do the privacy/cookie/basic-terms
boilerplate; they do **not** handle the two risk clauses (chargeback
liability + the CCR bespoke-exemption). So the game is: cheap draft
engine for the bulk, one small paid review for the risky bit.

**Recommended (cheapest safe path, ~£25–400, ~1 week):**

1. **Draft with Sparqa Legal** (~£25–40/mo, cancel after) —
   https://www.sparqalegal.com. It has UK **marketplace-specific**
   templates (platform T&Cs, seller agreement, buyer terms) *and* an
   unlimited solicitor Q&A helpline. Use the helpline to get the two
   risk questions below answered in writing. This is the one-stop "easy
   service" you're after. One month's subscription usually does it.
   - Alternatives if you want cheaper drafts: **Genie AI**
     (https://www.genieai.co — free/low-cost AI drafting, UK marketplace
     templates) or **Docular / SEQ Legal** (https://docular.net — very
     cheap template library, marketplace website T&Cs).
   - **Termly / iubenda** — fine for the privacy policy + cookie banner
     only. Do NOT rely on them for the marketplace/seller-of-record
     terms.

2. **Then a fixed-fee solicitor review of just the two risk clauses**
   (~£150–400, optional if Sparqa's helpline answers them clearly):
   **LawBite** (https://www.lawbite.co.uk) or **Lexoo** — post the two
   questions + your drafts, get a commercial-tech solicitor to sign off
   the chargeback + CCR clauses. Don't pay for a full bespoke draft
   (~£3–5k) at this stage; you don't have the volume to justify it.

**Do NOT** DIY the whole thing off a generic generator and ship it —
chargeback liability alone is a real, uncapped risk if the wording is
wrong.

## Draft email you can send today

```
Subject: Marketplace T&Cs + seller agreement for a small UK art platform

Hi,

I'm launching a marketplace for independent contemporary artists +
galleries (wander.gallery). Discovery-first, with an opt-in direct-sales
path where the artist ships from studio. Volumes will be tiny at launch —
hoping to have < £5k GMV in the first 3 months.

I need three documents, tailored to UK law:

  1. Marketplace terms of service (Wander = platform, artist =
     seller-of-record)
  2. Buyer terms of purchase (14-day cooling-off applicability?)
  3. Artist seller agreement (payout timing, chargeback liability,
     warranty of ownership)

Provisional decisions:
  - 15% flat commission, Wander invoices the artist
  - Payout via Stripe Connect standard schedule
  - Admin-initiated refunds only
  - UK-only launch (both buyers and artists)

Two specific questions I need answered:
  - Does original art sold direct from an artist qualify for the
    "bespoke or personalised" exemption under the Consumer Contracts
    Regulations 2013 (Reg 28)?
  - As a platform under £85k turnover, do I need any VAT-related
    disclosures beyond Stripe Tax's per-artist handling?

Timeline: 2 weeks would be ideal. Happy with a template + review
approach rather than bespoke drafting.

Best,
Josh Matthews
Wander Gallery
```

## What to do with the delivered docs

- `web/src/app/terms/page.tsx` — replace current placeholder with
  buyer terms of purchase + link to marketplace T&Cs. Also update
  `about`, `privacy`, `for-artists` copy to reference the new docs
  where relevant.
- `web/src/app/legal/marketplace.tsx` (new route) — marketplace
  terms of service, linked from the artist Stripe-onboarding
  consent screen (M-01).
- `web/src/app/legal/seller-agreement.tsx` (new route) — artist
  seller agreement, presented as a checkbox on the Stripe onboarding
  entry.
- Update this file to link the specific solicitor + template used,
  version number, review date, and next-review date (annual or
  when the model changes).
