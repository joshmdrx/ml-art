# Engineering TODOs

Open engineering items in rough priority order. Strategic items live in
`STRATEGY.md`; settled choices in `decisions.md`; what's shipped in
`CHANGELOG.md`.

Move items into `CHANGELOG.md` when they land. Add a strikethrough here
if the item was dropped, with a one-line reason.

---

## Now (active build)

### ~~`T-044` Jobs queue — Postgres local, SQS+Lambda prod~~ — shipped 2026-05-29

- ✅ `db/migrations/0012_jobs.sql` + `core::jobs` module (JobEvent + JobsBackend + Postgres driver + handler dispatch)
- ✅ `api/crates/jobs-worker` polling binary, wired into `scripts/dev.sh`
- ✅ Canary: geocoding migrated off `tokio::spawn` → `state.jobs.enqueue(JobEvent::ArtistLocationGeocode)`
- ✅ 4 unit + 6 integration tests; 216 Rust total
- **Deferred:** `JobsBackend::Sqs` variant + `jobs-lambda` crate. Lands when we deploy.

### ~~`T-041` + `T-042` + `T-043` Map discovery v1~~ — shipped 2026-05-29

- ✅ **T-041** — `?artist=<slug>` filter on `/v1/search/map`; "See on full map →" CTA on artist profile; scoping pill on the search map view. 3 integration tests.
- ✅ **T-042** — new `/v1/search/map/cities` aggregation endpoint; `CityPivotStrip` component (horizontal pill row above the map). Solves the cold-start "blank world" problem. 5 integration tests.
- ✅ **T-043** — `NearMeButton` component using browser geolocation; homepage hero gets a "📍 Near me · or · Explore the map →" row.

### ~~`T-038` Geography slice — `artist_locations` + Mapbox + map UI~~ — all five phases shipped 2026-05-28

- ✅ **G1** — `0011_artist_locations.sql` schema; `ArtistDetail.locations` extended; pre-geocode rows hidden from public payload
- ✅ **G2** — `core::geocoding` Mapbox v6 client (Real / Disabled / Test variants); `trigger_background_geocode` + `geocode_and_update`; `AppState.geocoder` plumbing
- ✅ **G3** — `/v1/studio/locations` CRUD with `deserialize_double_option` helper for real PATCH semantics; `StudioLocationsManager` UI on `/studio/settings`; polls every 3s until pins land
- ✅ **G4** — `ArtistLocationsMap` on `/artists/[slug]`; Mapbox GL JS dynamic import; fallback list view when `NEXT_PUBLIC_MAPBOX_TOKEN` is absent
- ✅ **G5** — `/v1/search/map` endpoint (bbox + q + medium + location); `/search?map=1` Grid/Map toggle with clustered pins, URL-synced bounds, popups, `searchMapClient.ts` browser-only fetch wrapper
- 22 new Rust tests + 4 new Playwright specs

**Follow-ups (not blocking v1):**
- Replace `tokio::spawn` in `trigger_background_geocode` with a real Inngest `artist_location.geocode` function once the Inngest runtime lands (same signature, same semantics — one-line swap)
- Seed at least one `artist_locations` row for the WikiArt demo corpus so the artist-profile map + `/search?map=1` show real pins out of the box
- Geographic neighborhoods (`neighborhoods.kind = 'geographic'`) — editorial work, still deferred per `99-deferred.md` Phase 1

### ~~`T-032` Real inquiry delivery via Resend~~ — shipped 2026-05-29

- ✅ `core::emails` EmailClient enum (Real / Disabled / for_tests) + Resend HTTP send + two templates (verification, deliver-to-artist).
- ✅ `JobEvent::InquirySendVerification` + `InquiryDeliverToArtist` variants + handler dispatch.
- ✅ Three enqueue sites in `inquiries.rs`: anonymous create, signed-in create, verify endpoint. Idempotency keys dedupe double-clicks.
- ✅ Reply-to is set to the inquirer so the artist hitting reply lands in their inbox.
- ✅ Config gains `web_base_url`; env vars `RESEND_API_KEY`, `RESEND_FROM_EMAIL`, `WEB_BASE_URL`.
- ✅ 4 integration tests (signed-in / anonymous / verify-end-to-end / double-verify dedup) + 6 unit tests.
- **Deferred:** drop `debug_verification_token` from non-dev response. Currently gated by `cfg.env.is_dev()` already; revisit if it ever shows up in staging.

---

## Soon (this milestone)

### `T-045` Integrated map + grid layout (Airbnb-style "Where to see them")
**Where:** `web/src/app/search/page.tsx` + `web/src/components/Search{SplitView,SidePanel,MapBlock}.tsx` + `web/src/components/SearchMap/*` hook split.

**Why:** the Works tab and the Where-to-see-them tab show different views of the same query — forcing the user to choose between "what does it look like" and "where can I see it." Merging them into a single split view (grid as side panel + map as main) makes the relationship between an artwork and its venue navigable in one glance.

**State:** L1 shipped 2026-06-07; L2 + L3 + L4 (and city-pivot-as-filter + location-filter parity) shipped 2026-06-08. **Closed.**

- ✅ **L1 — Layout shell.** Two-column on desktop (`lg:grid-cols-[minmax(0,2fr)_minmax(0,3fr)]`), stacked on mobile. Inset shadow used for card highlight so the scrollable panel can clip flush at the edges.
- ✅ **L2 — Hover sync.** `useHighlightedArtist` + `promoteId: "location_id"` on the clustered source. Inset shadow on the card; `feature-state.highlighted` scales the pin + thickens the stroke.
- ✅ **L3 — Click sync.** `useFocusArtist(map, pins, { artistSlug, tick })`. Popup opens immediately and is anchored via `setLngLat`; `flyTo({ essential: true })` for users with reduced-motion preferences.
- ✅ **City pivot is a filter.** Chip click sets `location` + `bbox`; clear via the FilterBar facet (single source of truth). `useFitToInitialPins` handles camera refit on clear regardless of who dropped the param.
- ✅ **`/v1/search` location filter parity** with `/v1/search/map` + `/v1/search/map/cities`: now ORs in an `EXISTS (SELECT 1 FROM artist_locations …)` clause, so grid + map + strip agree on what "in X" means.
- ✅ **L4 — Polish.**
  - **Caption.** `<SearchMap>` exposes its live pin set via `onPinsChanged`; `<SearchSplitView>` mirrors it as `visiblePins` (derived-state synced to the server prop) and computes "N of M mapped". Replaces the old "24+ WORKS" line *and* the disconnect-explainer in one shot. Reads `N of M[+] mapped` always — never `All M+ mapped` (contradictory).
  - **Mobile bottom-sheet.** On `<lg` the side panel is a fixed-bottom sheet with peek (3rem handle) / expanded (~70dvh) snap states. Tap the handle to toggle. Map fills the viewport behind it. Desktop layout unchanged.
  - **Pan-aware sort: prototyped, removed.** Cards jumping mid-scroll was disorienting; the sidebar stays stable now, with pan only updating the caption count. Comment left in `SearchSplitView` so the next person doesn't re-add it.

**Risks captured up-front:**
- ~~Bidirectional sync loops~~ — handled via the single `highlightedArtistSlug` state in `SearchSplitView`; panel hovers are the only originators today.
- ~~GeoJSON pin styling by id~~ — `promoteId` keeps Mapbox-assigned cluster ids from clobbering ours.
- ~~Grid at ~520px panel width~~ — `grid-cols-1 sm:grid-cols-2` inside the panel.

**Hidden gotchas surfaced and fixed:**
- Next 15 `useSearchParams` is reactive to `history.replaceState`, which made our pan-handler self-feed. `bboxesApproxEqual` guard in `useUrlBboxFitBounds`.
- Refetch-on-pan-when-filtered was wasteful (server already returned every match). Gated via `refetchOnPan` in `useMapBboxSync`.
- FilterBar's location clear used to leave `bbox` in the URL, so the server-side map fetch kept spatially clipping. Now `bbox: null` rides with every location mutation.
- `'server-only'` cannot be imported from client components, so `formatPrice` / `formatDimensions` moved out of `lib/api.ts` into `lib/format.ts`.

### ~~`T-022` Pricing/dimensions polish~~ — shipped 2026-06-09

- ✅ `formatDimensions` + `formatPrice` (`lib/format.ts`).
- ✅ `seed.py` writes deterministic per-sha price + dimensions on INSERT. `_demo_price_cents` quantises to nearest $10 in $50–$2500 range; `_demo_dimensions` produces cm widths/heights.
- ✅ One-off backfill SQL at `db/seeds/0001_demo_prices_dimensions.sql` for the already-inserted 2000 rows. Idempotent.

### ~~`T-039` Artist-facing price input UX~~ — shipped 2026-05-29

- ✅ `lib/parsePrice.ts` + `formatPriceForInput` + ISO 4217 minor-unit table; 17 vitest tests.
- ✅ `ArtworkEditModal` price field: text input + currency dropdown (USD/GBP/EUR/CAD/AUD/JPY/CHF/SEK/NOK/DKK). On blur reformats. On submit derives `price_cents`.
- **Deferred:** server-side tighten on `price_cents` (negative + overflow). Open if bad input is observed.
- **Deferred:** auto-detect currency from artist `based in` city.

### ~~`T-040` Studio location validation feedback~~ — shipped 2026-05-29

- ✅ Three-state pin status on `StudioLocationsManager`: "Locating…" (in-flight) → "Pin set · {city, country}" (success) → "Couldn't find this address — try adding city + country" (Mapbox returned empty).
- **Deferred:** Mapbox Places autocomplete on the address field.
- **Deferred:** Same three-state pattern for the artist's own `based in` field (needs the field to actually flow through the geocoder, which it doesn't yet — currently only the raw string is stored).

### `T-004` Incremental cache saves in `CachedEmbedder`
**Where:** `ml/ml_art/embeddings/cache.py`
**Why:** current design writes all `.npy` files at the end of `embed_images`. A mid-run crash on a 2000-image embed loses everything. Burned us once during the WikiArt pass.
**Acceptance:** stream-style writes; survives `kill -9` mid-run; existing tests pass; add a partial-completion resume test.

### ~~`T-008` Image moderation — artwork_images pipeline~~ — shipped 2026-06-01

- ✅ `core::moderation` `ModerationClient` enum (`Disabled` / `for_tests`); `JobEvent::ArtworkImageModerate` variant + handler dispatch.
- ✅ Enqueue from `studio::artworks::add_image` with idempotency key `moderate:artwork_image:{id}`.
- ✅ Public surfaces tightened to `moderation_status = 'approved'` (was `!= 'rejected'` at one site + no filter elsewhere).
- ✅ 7 integration + 3 unit tests; `JobsDeps` + jobs-worker + env wired.

**Deferred (open follow-ups):**
- `T-008a` Real Rekognition wire-up — pull `aws-sdk-rekognition`, build `Real` variant, gate on `REKOGNITION_ENABLED`. Lands alongside the AWS deploy.
- ~~`T-008b` Moderation on the `uploads` bucket~~ — shipped 2026-06-01. `JobEvent::UploadModerate` + `moderate_upload` handler + enqueue from `uploads::create` + visual-search anchor refuses rejected rows. 8 integration tests.
- ~~`T-008c` Surface rejection reason in studio~~ — shipped 2026-06-09. New `artwork_images.moderation_reason` column persists comma-joined Rekognition labels on rejection (cleared on re-approve). `<ModerationBadge>` in `ArtworkEditModal` shows "Hidden · <labels>" on rejected tiles + dims/grayscales the image. 2 new integration tests (310 total Rust).

### ~~`T-010` Visual search upload + modifier UI~~ — all four phases shipped
- ✅ **Phase A:** `POST /v1/uploads/image` — multipart in, S3/MinIO PUT, inline T-036-style embedding into `uploads.embedding`
- ✅ **Phase B:** `GET /v1/search?image_upload_id=…` — anchor the semantic side on an uploaded image's vector
- ✅ **Phase C:** `?modifiers=moodier,warmer,…` at α=0.8 per the spike. `core::modifiers` registry; `GET /v1/modifiers` lists them; unknown names → 400; modifiers require `image_upload_id`
- ✅ **Phase D:** Web UI — `VisualSearchUpload` (camera icon next to hero search), `ModifierBar` (URL-driven pill toggles on `/search`), `VisualAnchor` strip with "Clear image" affordance. Server action `uploadAndStartVisualSearch` POSTs the multipart and redirects to `/search?image_upload_id=…`

---

## Auth + identity follow-ups

### ~~`T-033` Merge anonymous behavior into user on sign-in~~ — shipped 2026-06-01

- ✅ `POST /v1/me/merge-anonymous` — transactional UPDATE on uploads + events keyed on `(anonymous_id = $anon AND user_id IS NULL)`. Idempotent + ownership-safe (never overwrites an existing link).
- ✅ Next.js route handler + `<AnonymousMergeBridge />` client component mounted in the root layout. Fires once per browser session via `sessionStorage` marker.
- ✅ 8 Rust integration tests covering happy path, no-anon no-op, no-rows no-op, second-call idempotency, never-overwrite-Bob, per-user isolation, 401, 400 malformed.

**Deferred:** the `events` writer doesn't exist yet (T-016 partitioning track) so the events_merged count is always 0 today. The merge code is already in place so it'll start working when events writes land.

### `T-014` Dev login-as-artist (partly obsolete now)
**State:** real Clerk auth works; for testing the *artist studio* we may still want a way to assume a specific artist without signing up with their email. Defer until studio lands; possibly fold into `T-031` (web test-mode bypass).

---

## Later (large pieces of v1)

### `T-011` Artist studio (`/studio/*` endpoints + pages)
- ✅ **Phase 1 (landed):** artwork CRUD API + `/v1/studio/me`, ownership-by-artist boundary
- ✅ **Phase 2 (landed):** `/v1/studio/settings` + `/studio/settings` page + public-surface `ar.status='active'` filter
- ✅ **Phase 3 (landed):** `/studio` portfolio page — grid with status filter pills (All / Drafts / Published), create+edit modal w/ image management, delete with confirmation. LLM-assisted intake is `T-012`
- ✅ **Phase 4a (landed 2026-06-01):** `GET /v1/studio/inquiries` + `/studio/inquiries` inbox page with status filter (All / Pending / Delivered). 9 Rust integration tests.
- ✅ **Phase 4b (landed 2026-06-09):** reply-from-inbox + auto-mark-as-read. Migration `0013_inquiry_replies.sql` (new table + `inquiries.read_at`). Three endpoints: `GET` extended with replies + read_at; `POST .../reply`; `POST .../read`. New `JobEvent::InquirySendReply` + handler + `templates::artist_reply` (Resend). Web: client-side `<InquiryInbox>` with per-card reply form, optimistic append, auto-fire mark-as-read on view. 7 new integration tests (16 inbox total).
  - **Deferred:** `/studio/analytics` stub (full analytics blocked on events-table writes — separate gap). Inbound replies from the inquirer back to the artist (needs an inbound-email webhook).
- ✅ **Phase 5 (landed 2026-06-09):** Bulk image upload. `<input multiple>` + `onFilesSelected` in `<ArtworkEditModal>`. Per-file validate, drop bad with a per-file note, batch cap 20. Sequential upload through the existing `uploadArtworkImage`. "Uploading N of M" caption + multi-line error block. No server changes.

### `T-012` Onboarding flow `/onboarding`
- ✅ **Phase 1 (landed 2026-05-28):** `POST /v1/onboarding/start` (mint artist + link `user_id`, slug w/ collision suffix) and `POST /v1/onboarding/complete` (`pending → active`, idempotent). Five-step wizard at `/onboarding` (identity → profile → artworks → locations → review) reusing existing studio mutations. `/studio` + `/studio/settings` redirect signed-in non-artists into the wizard. 10 Rust integration tests + 6 slugify unit tests + 1 Playwright spec.
- Phase 2 (blocked on Inngest runtime):
  - `POST /v1/onboarding/import` — website / Instagram scrape job; pre-fills bio + image URLs
  - `POST /v1/onboarding/extract-metadata` — Anthropic conversational extraction per artwork (gated by `ANTHROPIC_ENABLED`)
  - `POST /v1/onboarding/polish-statement` — optional LLM polish on the artist statement

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

### ~~`T-037` Cursor pagination on `/v1/search`~~ — shipped 2026-06-08; 2026-06-09 lifted to URL-driven

- ✅ `ml_art_core::cursor::PageCursor` — opaque base64url-encoded JSON, `MAX_CURSOR_OFFSET = 1000`. Forward-compatible to keyset.
- ✅ `/v1/search` decodes `cursor` → offset, fetches `limit + 1`, returns `next_cursor` when there's more. 6 unit + 4 integration tests.
- ✅ **2026-06-09:** swapped client-side cursor state for URL-driven `?pages=N` (server loops cursor-chained fetches, cap 10). `<SearchSplitView>` is now driven entirely from props — no client `items` state, no sessionStorage. Load More is `router.push('?pages=N+1', { scroll: false })` + `useTransition`. See decisions.md 2026-06-09.
- ✅ Map sync on Load More — refetches pins via `searchMapClient` when a new page introduces an artist not yet in the pin set.

**v1 trade-off:** offset-based, not keyset. Hybrid search's RRF score is computed in SELECT, so true keyset would need an outer-SELECT subquery wrap. Offset is fine for a ~2000-row corpus (candidate pool capped at 200). The cursor shape is opaque to clients so a future keyset swap doesn't change the API.

**Open follow-ups:**
- Grid-mode pagination (non-map `/search` page). Today only `/search?map=1` paginates; the static grid view stops at the first 24. Needs converting `<ArtworkGrid>` to a client component or adding a server-action-driven Load More.
- Other paginated endpoints (`/v1/artists/:slug.artworks`, `/v1/collections/:id.artworks`, `/v1/studio/me`, `/v1/studio/inquiries`) still return `next_cursor: None`. The `PageCursor` helper is in place — each endpoint just needs the same plumbing.

### ~~`T-046` Visual search from a platform artwork + state-resume UX~~ — shipped 2026-06-09

- ✅ `seed_artwork_id` param on `/v1/search` — resolves to the artwork's existing CLIP embedding. Modifiers compose. Seed artwork excluded from results.
- ✅ "Find visually similar →" CTA on `/artworks/[id]` + `<SeedAnchor>` strip on `/search`.
- ✅ URL state-resume: `?pages=N`, `?focus=<artwork_id>`, plus `<BackToSearchLink>` that uses `router.back()` for full state restore when referrer is `/search`.
- ✅ Map default tightened to top-5-pins fit + viewport-preserve on clear-filter.
- ✅ `useFocusArtist` `flyTo` perf: `speed 2.0`, `curve 1.1`, `maxDuration 1200` — was 4–6s scenic-arc, now ~1.5s end-to-end including bbox URL write.

---

## Dropped / on-ice

_Nothing yet — write `~~text~~ — reason` here when dropping an item._
