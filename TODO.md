# Engineering TODOs

Open engineering items in rough priority order. Strategic items live in
`STRATEGY.md`; settled choices in `decisions.md`; what's shipped in
`CHANGELOG.md`.

Move items into `CHANGELOG.md` when they land. Add a strikethrough here
if the item was dropped, with a one-line reason.

---

## Now (active build)

### `T-032` Real inquiry delivery via Resend + Inngest
**Where:** new Inngest function `inquiry.deliver`; Resend HTTP client in `core::email`
**Acceptance:**
- After a signed-in inquiry hits `delivered_at=now()`, an Inngest job sends the actual email to the artist via Resend
- After an anonymous inquiry's verify endpoint flips `delivered_at`, same job fires
- Email contains: artwork title + thumbnail, sender name + email (reply-to set to sender), message, budget if present
- Drop the `debug_verification_token` field from the API response in non-dev envs

---

## Soon (this milestone)

### `T-022` Pricing/dimensions polish (partial — formatters in place, seed data null)
**Where:** seed script (optional) + `lib/api.ts` (done) + ArtworkDetail panel (done)
**State:** `formatDimensions` and `formatPrice` work; the seeded demo artworks have null dimensions/price. Either backfill in `seed.py` with plausible random values, or leave demo content as-is (price/dim only matter for real artists). Decide before launch.

### `T-004` Incremental cache saves in `CachedEmbedder`
**Where:** `ml/ml_art/embeddings/cache.py`
**Why:** current design writes all `.npy` files at the end of `embed_images`. A mid-run crash on a 2000-image embed loses everything. Burned us once during the WikiArt pass.
**Acceptance:** stream-style writes; survives `kill -9` mid-run; existing tests pass; add a partial-completion resume test.

### `T-008` Image moderation Inngest job
**Where:** `api/crates/api-uploads/` (new binary) + Inngest handler
**Acceptance:**
- `image.moderate` job calls AWS Rekognition `DetectModerationLabels` on a new image
- Sets `artwork_images.moderation_status` or `uploads.moderation_status`
- Search filters out `moderation_status != 'approved'` artwork images
- Local dev: stub always-approves (gated by `REKOGNITION_ENABLED=false`)

### `T-010` Visual search upload + modifier UI
**Where:** new `api-uploads` binary + new search-page modifier-button row
**Acceptance:**
- `POST /v1/uploads/image` stores in S3 under `uploads/`, enqueues `image.moderate` + `image.embed`
- Embedding generated on upload (not lazy) — first search using the upload doesn't pay the Jina roundtrip
- Search page has modifier buttons ("moodier", "warmer", "more minimal", "more textured", "more graphic") that POST against `/v1/search?image_upload_id=…&modifiers=…`
- Implementation: delta vectors at α=0.8 per the spike findings (`ml/spikes/2026-05-modifier-deltas/FINDINGS.md`)

---

## Auth + identity follow-ups

### `T-033` Merge anonymous behavior into user on sign-in
**Where:** `POST /v1/me/merge-anonymous`, called from Clerk's sign-in callback
**Acceptance:** Reads `anon_id` cookie, copies behavioral signal (searches, saves intent, anything keyed off anon_id) to the now-known `user_id`. Idempotent.

### `T-014` Dev login-as-artist (partly obsolete now)
**State:** real Clerk auth works; for testing the *artist studio* we may still want a way to assume a specific artist without signing up with their email. Defer until studio lands; possibly fold into `T-031` (web test-mode bypass).

---

## Later (large pieces of v1)

### `T-011` Artist studio (`/studio/*` endpoints + pages)
- ✅ **Phase 1 (landed):** artwork CRUD API + `/v1/studio/me`, ownership-by-artist boundary
- ✅ **Phase 2 (landed):** `/v1/studio/settings` PATCH + `/studio/settings` page (bio, statement, location, website, visibility toggle). Public surfaces now respect `artists.status='active'` so the Unpublish toggle is honest
- Phase 3: `/studio` portfolio page (grid + add/edit modal, no LLM)
- Phase 4: `/v1/studio/inquiries` GET + `/studio/analytics` stub (full analytics blocked on events-table writes — separate gap)
- Phase 5: Bulk image upload (depends on `T-010` `POST /v1/uploads/image`)

### `T-012` Onboarding flow `/onboarding`
- Stepped, progressive disclosure
- Website scrape Inngest job
- Per-artwork conversational metadata extraction (Anthropic, behind `ANTHROPIC_ENABLED`)

### `T-015` Spend caps + budget alarms
**Where:** `infra/` (Terraform, not yet started)
**Acceptance:**
- AWS Budgets at $20/mo (prod) → email
- Per-service spend monitors via Inngest cron
- Pre-launch checklist in `COST.md` satisfied

### `T-034` Edge rate limiting (AWS WAF / CloudFront)
**Where:** `infra/` (with `T-015`)
**Why:** API-layer rate limit (`T-007`) protects the paid Jina call but a request that gets 429'd still cold-starts a Lambda. WAF blocks volumetric attacks at the edge before any Lambda invocation.
**Acceptance:**
- AWS WAF rate-based rule (or CloudFront viewer-request function w/ KeyValueStore) at ~1000 req / 5 min / IP in front of the Lambda Function URL
- Logs forwarded to CloudWatch for visibility on what's being shed
- Sized loosely so legit bursty traffic isn't shed — this is the volumetric tier, not the per-user tier

### `T-035` Vercel edge rate limit on write surfaces
**Where:** `web/middleware.ts` (extend) + Vercel KV
**Why:** the Next.js frontdoor sees public traffic before the API. Cheap to add a per-IP burst guard on `/search` and on the inquiry/save server-action paths so abuse can't tie up Vercel server functions or our DB connection pool.
**Acceptance:**
- Vercel KV (or in-edge `next-safe-action`-style counter) at ~30/min/IP for `/search?*` page hits
- Same for the inquiry server action and save server action
- Skipped when `process.env.RATE_LIMIT_DISABLED === 'true'` for local dev

---

## Soft maintenance (do when it bites)

### `T-016` Convert `events` table to monthly partitioning
- Already documented in `db/README.md` as a v1 deviation
- Trigger: events table > 10M rows or queries get slow

### `T-017` Search facet counts
- Spec'd but currently returns nothing. Costs per-search COUNT queries; needs precomputation or approximation at scale.

### `T-018` Query embedding cache TTL job
- `query_cache.cleanup` Inngest cron, daily
- `DELETE WHERE last_used_at < now() - interval '30 days'`

### `T-019` Voyage multimodal embedder
- Second `Embedder` impl for A/B
- Trigger: only if there's a compelling reason to compare against Jina

### `T-037` Cursor pagination on `/v1/search` (and friends)
**Where:** `api-search::search` + the other endpoints that return `Paginated<T>` with `next_cursor: None`.
**Why:** every paginated endpoint currently caps at the in-handler limit and never returns a cursor. Fine for v0 demo content; not fine when an artist's portfolio has 100 works or a search returns more than 24 hits. Pre-launch nice-to-have, not a release blocker.
**Acceptance:**
- Opaque cursor in the response when there's a next page (base64 of `(rank_score, id)`)
- `?cursor=<opaque>` query param accepted; decoded server-side, used as a WHERE filter against the same SQL
- Backward compatible: omitting the cursor yields the first page like today

---

## Dropped / on-ice

_Nothing yet — write `~~text~~ — reason` here when dropping an item._
