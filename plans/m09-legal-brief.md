# M-09 — ready-to-send legal brief

Copy-paste this into Sparqa Legal's solicitor helpline, LawBite, or any
fixed-fee solicitor. It's written to get the three documents drafted +
the two risk clauses reviewed. See `marketplace-legal.md` for the fuller
context and the recommended providers.

---

## The brief (paste this)

> **Subject: Marketplace T&Cs + seller agreement for a small UK art platform**
>
> I'm launching an opt-in direct-sales feature on Wander
> (wander.gallery), a discovery platform for independent contemporary
> artists. Today it's discovery + direct inquiry; I'm adding a checkout
> where the artist ships from their own studio and I take a commission.
> Volumes will be tiny at launch — hoping for < £5k GMV in the first 3
> months. UK-only for v1 (both buyers and artists in the UK).
>
> **How it works technically:** payments run through **Stripe Connect
> Express**. The buyer's card is charged on Wander's platform account;
> Stripe takes my commission as an application fee and routes the balance
> to the artist's connected Stripe account. Wander is the platform of
> record; the artist is the seller of record and ships the work.
>
> **I need three documents, tailored to UK law:**
>
>   1. **Marketplace terms of service** — Wander = platform, artist =
>      seller-of-record. Commission structure, when payouts happen,
>      dispute/chargeback liability.
>   2. **Buyer terms of purchase** — conditions of sale, when a sale is
>      final, return/refund policy (Consumer Rights Act 2015 + Consumer
>      Contracts Regulations 2013).
>   3. **Artist seller agreement** — accepted at Stripe Connect
>      onboarding. Commission %, payout timing, chargeback liability if
>      the buyer disputes and wins, warranty that the artist owns the
>      work + right to sell.
>
> **Provisional commercial decisions (please flag if any are unwise):**
>   - Wander commission: **15% flat** on the gross sale price.
>   - Payouts: Stripe Connect standard schedule (funds released after the
>     order is `delivered`, or ~14 days after `shipped` if the buyer
>     doesn't confirm).
>   - Refunds: **admin-initiated only**, no automatic buyer-side refund.
>   - Territory: **UK only** for v1.
>
> **The two questions I most need answered (in writing):**
>   1. **Chargeback liability** — if a buyer files a chargeback and wins,
>      I want that loss to fall on the **artist** (seller of record), not
>      Wander, and I want the seller agreement worded so I can recover it
>      from their future payouts. Is that enforceable under UK law, and
>      how should it be drafted?
>   2. **Consumer Contracts Regulations 2013 (Reg 28)** — does
>      **original art bought direct from the artist** qualify for the
>      "made to the consumer's specifications / clearly personalised"
>      exemption from the 14-day cooling-off right? If not, I owe a
>      14-day return window on every sale and need the buyer terms to say
>      so.
>
> **Also, briefly:** as a platform with turnover < £85k, do I need any
> VAT-related disclosures beyond Stripe Tax's per-artist handling?
>
> Timeline: ~1 week would be ideal. Happy with a template + review
> approach rather than bespoke drafting.
>
> Thanks,
> Josh Matthews — Wander

---

## When the docs come back — wire-up checklist

Already scaffolded in the app (routes exist / will be added):

- `web/src/app/terms/page.tsx` — replace placeholder with **buyer terms
  of purchase**; link the marketplace T&Cs.
- `web/src/app/legal/marketplace/page.tsx` (new) — **marketplace terms
  of service**. Link from the artist Stripe-onboarding consent screen
  (the `EnableSalesButton` on `/studio/settings/payouts`).
- `web/src/app/legal/seller-agreement/page.tsx` (new) — **artist seller
  agreement**, shown as a checkbox before onboarding starts.
- Add a "By continuing you agree to the seller agreement" checkbox gate
  in `EnableSalesButton` (M-01/M-06) before it calls
  `startPayoutOnboarding()`.
- Add the buyer-terms acceptance line to the checkout summary on
  `/artworks/[id]/buy` (M-05).
- Record the provider used, doc version, review date + next-review date
  (annual, or when the commission model changes) back in
  `marketplace-legal.md`.
