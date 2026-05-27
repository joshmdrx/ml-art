# Strategy & Open Questions

Non-engineering tracks that need ownership but don't block code from being
written. Kept separate from `decisions.md` (which records *settled* choices)
and `99-deferred.md` (which is post-v1 feature backlog).

Update this file when something gets resolved — promote the item into
`decisions.md`, then delete the entry here.

---

## 1. Cold-start: real artist outreach

**Status:** owner = founder; engineering ungated.

**The problem:** the platform's value depends on having interesting indie
artists publishing work. The current build serves a synthetic-demo corpus
(WikiArt) — *good for development, not for launch*. Public launch with only
demo data is a non-starter.

**What needs doing (in rough order):**

1. **Define the target artist profile.** Indie contemporary; what mediums,
   what stage of career, what geographies first?
2. **Build a target list of 30–50 artists.** Mix of:
   - Personal network referrals
   - Instagram art-discovery rabbit holes (manual, not scraped)
   - Are.na public channels
   - Local art-school graduate showcases
3. **Write outreach copy.** Short, honest:
   *"I'm building a discovery platform for independent artists — no
   commissions, no marketplace, just better search. Would you be one of the
   first 30 to put a portfolio on it? Free, you keep all inquiries direct."*
4. **Process for onboarding.** Live spreadsheet → DM → form → onboarding
   call → portfolio published. Manually for the first cohort.
5. **First-cohort metrics:** % of contacts who reply, % of repliers who
   publish, time-to-first-inquiry per artist.

**Engineering dependency:** the onboarding flow itself (`/onboarding` + LLM
intake) is in the v1 build plan. First cohort can be onboarded via direct
DB inserts / a guided session with the founder until that flow lands.

## 2. Build order within v1

**Status:** open.

The spec defines v1 as a coherent product, but v1 is too big to ship as a
single milestone. The user wants v1 overall — meaning the *target* is v1
parity, but we still need an order to build in. Proposed:

| Stage | What | Why this order |
|---|---|---|
| **Stage 1** | Search + artwork detail + artist portfolio (read-only, against demo corpus) | Proves the discovery story end-to-end. Highest-confidence ML loop. |
| **Stage 2** | Anonymous browse + signed cookie infra; rate limiting; image moderation pipeline | Foundation for any user-uploaded content. Must precede stage 3. |
| **Stage 3** | Save to collection (auth via Clerk); inquiry flow with email verification | First authed surface; first real artist value (inquiries). |
| **Stage 4** | Artist studio (CRUD over artworks) + onboarding flow | First artist-facing surface. Replaces direct-DB onboarding for the first cohort. |
| **Stage 5** | Visual search + modifier buttons (delta vectors per spike findings) | Differentiator. Once core is solid. |
| **Stage 6** | Geographic filters + curated geographic neighborhoods | Pulls in location dimension. |
| **Stage 7** | Studio analytics + admin polish + pre-launch hardening | Polish + ops. |

**Open question:** does any cohort of users see Stage 1–3 before Stage 4
ships? If yes, we need a "beta — invited artists only" gating. If no,
Stage 1–7 ship together as v1 launch.

## 3. Threat / abuse priorities

**Status:** decided in `decisions.md` (rate limiting + image moderation
must precede any user-uploaded content; inquiry verification precedes the
inquiry endpoint going live).

Documented here as a checklist:

- [ ] Rate limiting middleware (Upstash, sliding window) — before Stage 2
- [ ] Image moderation Inngest job (Rekognition) — before any artist
      uploads
- [ ] Visual-search upload moderation — same job
- [ ] Inquiry email verification flow — before Stage 3
- [ ] Per-IP rate limit on `/inquiries` (3/hr) — before Stage 3
- [ ] CAPTCHA on anonymous inquiry? — TBD, depends on observed spam

## 4. Brand / naming

**Status:** open. Current working name: `ml-art` (placeholder).

**Decisions blocked by this:**

- Domain registration and DNS setup
- Clerk dev instance's display name (currently can be anything)
- Email sender identity (Resend `from:` address)
- OG share metadata (`<meta property="og:site_name">`)
- Repository renames
- README / pitch deck if either materializes

**Constraints to keep in mind:**

- Pronounceable in English; ideally short
- Available `.com` or `.art` domain
- Not "Saatchi", "Artsy", "Singulart" (existing competitors)
- Not "Gallery" (defines us as something we're not)
- Suggests *discovery* or *taste*, not *commerce*

**Until decided:** all paths use `ml-art` internally. Easy find-and-replace.

## 5. Privacy / legal preflight

**Status:** open. Required before public launch, not before engineering
work.

**Deliverables:**

- [ ] Privacy policy (template-based, then reviewed)
- [ ] Terms of service
- [ ] DMCA contact + process page
- [ ] Cookie consent banner copy
- [ ] Data retention policy
- [ ] GDPR data subject request process (export, delete account)
- [ ] Artist agreement (what we do with their content, what they retain)
- [ ] Inquiry sender consent UI ("by submitting, you agree your email is
      shared with this artist")

**Recommended approach:** Termly or iubenda for the templated docs (~$100
once); skip lawyer review until the platform has real artist users
contributing IP-sensitive content. Add lawyer review when first non-trivial
legal contact arrives (DMCA, GDPR DSR, etc.).

## 6. Spend monitoring (operations)

**Status:** documented in `COST.md`; setup pending until first deploy.

When any infra is deployed beyond local:

- [ ] AWS Budgets alarm at $20/mo, emailed
- [ ] Anthropic console spend cap $30/mo
- [ ] Jina console spend cap (whatever they allow)
- [ ] PostHog event-quota notification at 80%
- [ ] Resend quota notification at 80%

## 7. Embedding model migration plan

**Status:** deferred — write a runbook when we want to evaluate switching
to Voyage multimodal-3 or a fine-tuned model.

The mechanics are already supported (`artwork_embeddings` table is keyed by
`(artwork_id, model_name, model_version)`, so A/B and re-embed are safe).
What's missing is the *procedure*: how to roll a new model into prod,
how to A/B, how to swap defaults, how to retire old embeddings.

Doc lives in `99-deferred.md` until needed.
