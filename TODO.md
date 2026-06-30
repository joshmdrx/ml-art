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
- ~~Replace `tokio::spawn` in `trigger_background_geocode` with the canonical `JobEvent::ArtistLocationGeocode` enqueue.~~ Shipped 2026-05-29 with `T-044` (jobs queue) — see `decisions.md`.
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

### `T-016` Convert `events` table to monthly partitioning
**Where:** new migration `0015_events_partition.sql`; lands with `T-050`.
**Why:** Promoted from soft maintenance. Partitioning is the prerequisite that makes the eventual hot → cold tier swap (S3 Parquet archive) safe and cheap. Without it, pruning becomes a destructive whole-table operation and analytical queries get progressively slower past ~100M rows. See `decisions.md` 2026-06-17 "Event storage."
**Acceptance:**
- Convert `events` to `PARTITION BY RANGE (occurred_at)`, monthly partitions, auto-created N months ahead by a small DDL job.
- Migration is online: copy existing rows into the first partition, swap with a single transactional rename.
- All three indexes from `0006_events_profiles.sql` recreated per partition.
- Integration test: insert spanning a partition boundary still queries cleanly via the parent.
- Lands in lockstep with `T-050` so the writer never targets the un-partitioned table.

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

---

## Later (large pieces of v1)

### ~~`T-011` Artist studio (`/studio/*` endpoints + pages)~~ ✓ shipped (all 5 phases)
- ✅ **Phase 1:** artwork CRUD API + `/v1/studio/me`, ownership-by-artist boundary
- ✅ **Phase 2:** `/v1/studio/settings` + `/studio/settings` page + public-surface `ar.status='active'` filter
- ✅ **Phase 3:** `/studio` portfolio page — grid with status filter pills (All / Drafts / Published), create+edit modal w/ image management, delete with confirmation. LLM-assisted intake is `T-012`
- ✅ **Phase 4a (2026-06-01):** `GET /v1/studio/inquiries` + `/studio/inquiries` inbox page with status filter (All / Pending / Delivered). 9 Rust integration tests.
- ✅ **Phase 4b (2026-06-09):** reply-from-inbox + auto-mark-as-read. Migration `0013_inquiry_replies.sql` (new table + `inquiries.read_at`). Three endpoints: `GET` extended with replies + read_at; `POST .../reply`; `POST .../read`. New `JobEvent::InquirySendReply` + handler + `templates::artist_reply` (Resend). Web: client-side `<InquiryInbox>` with per-card reply form, optimistic append, auto-fire mark-as-read on view. 7 new integration tests (16 inbox total).
  - **Deferred (now tracked elsewhere):** `/studio/analytics` stub (will pull from the events writer that T-050 shipped). Inbound artist-to-inquirer replies shipped via `T-054`.
- ✅ **Phase 5 (2026-06-09):** Bulk image upload. `<input multiple>` + `onFilesSelected` in `<ArtworkEditModal>`. Per-file validate, drop bad with a per-file note, batch cap 20. Sequential upload through the existing `uploadArtworkImage`. "Uploading N of M" caption + multi-line error block. No server changes.

### `T-012` Onboarding flow `/onboarding`
- ✅ **Phase 1 (landed 2026-05-28):** `POST /v1/onboarding/start` (mint artist + link `user_id`, slug w/ collision suffix) and `POST /v1/onboarding/complete` (`pending → active`, idempotent). Five-step wizard at `/onboarding` (identity → profile → artworks → locations → review) reusing existing studio mutations. `/studio` + `/studio/settings` redirect signed-in non-artists into the wizard. 10 Rust integration tests + 6 slugify unit tests + 1 Playwright spec.
- Phase 2 (blocked on the LLM-extraction + scrape handlers landing in `core::jobs`):
  - `POST /v1/onboarding/import` — website / Instagram scrape job; pre-fills bio + image URLs
  - `POST /v1/onboarding/extract-metadata` — Anthropic conversational extraction per artwork (gated by `ANTHROPIC_ENABLED`)
  - `POST /v1/onboarding/polish-statement` — optional LLM polish on the artist statement

### `T-015` Spend caps + budget alarms
**Where:** `infra/` (Terraform, not yet started)
**Acceptance:**
- AWS Budgets at $20/mo (prod) → email
- Per-service spend monitors via scheduled jobs (cron-enqueued `JobEvent`)
- Pre-launch checklist in `COST.md` satisfied

### `T-034` Edge rate limiting (AWS WAF / CloudFront)
**Where:** `infra/` (with `T-015`)
**Why:** API-layer rate limit (`T-007`) protects the paid Jina call but a request that gets 429'd still cold-starts a Lambda. WAF blocks volumetric attacks at the edge before any Lambda invocation.
**Acceptance:**
- AWS WAF rate-based rule (or CloudFront viewer-request function w/ KeyValueStore) at ~1000 req / 5 min / IP in front of the Lambda Function URL
- Logs forwarded to CloudWatch for visibility on what's being shed
- Sized loosely so legit bursty traffic isn't shed — this is the volumetric tier, not the per-user tier

---

## Post-launch tracks (v1.x — retention + ML)

The retention loop + ML discovery surface, defined as a coherent body of
work in the 2026-06-17 strategy session. See `decisions.md` for the four
underlying positions: no in-platform messaging, ML-driven discovery,
algorithmic neighbourhoods as primary primitive, Postgres-hot / S3-cold
event storage.

Ordered roughly by precondition graph: foundation (`T-050`) → retention
(`T-051..T-053`) → loops (`T-054`) → ML core (`T-055..T-057`) → UX
additions (`T-058..T-063`).

### ~~`T-050` Behavioural events writer~~ — shipped 2026-06-25

The `events` table (migration 0006) has existed since launch; nothing wrote to it. T-055/56/57/60/61 (taste vector, "for you", neighbourhoods, digest, calibrator) were all gated on event data flowing. This unblocks them.

- ✅ `core::events` module: closed `EventName` taxonomy (12 codes), `extract_request_context` (IP + UA from `X-Forwarded-For` + `User-Agent`), `event_log()` builder, best-effort `emit()`.
- ✅ `JobEvent::EventLog` variant + handler arm `INSERT INTO events`. Storage destination encapsulated behind the queue per the 2026-06-17 decision.
- ✅ 10 server-side emits wired: `search_executed` (page-1 only), `artwork_viewed`, `artwork_saved`/`unsaved`, `inquiry_submitted` (both anon + signed-in paths attach the best-available identity), `visual_search_uploaded`, `artist_viewed`, `neighborhood_viewed`, `artist_followed`/`unfollowed`.
- ✅ 2 client-side emits via `POST /v1/events` (T-050.3): `modifier_applied`, `inquiry_started`. Server-side allowlist gates which names are accepted from the client.
- ✅ `web/src/lib/events.ts` batcher: count-flush at 10, timer-flush at 5s, `pagehide` + `visibilitychange→hidden` triggers, `keepalive: true` on the fetch.
- ✅ `web/src/app/api/events/route.ts` same-origin proxy (browser cookie scoped to wander.gallery, can't reach api.wander.gallery directly).
- ✅ T-033 merge handler already had `UPDATE events SET user_id = $new_user WHERE anonymous_id = $anon` from day 1 — covered by `merge_stamps_user_id_on_anon_rows` test. Now operational because emit sites populate rows.
- ✅ PII: IP + UA stored on every row. Documented in the `core::events` module-level doc. Retention + DSAR + cookie-consent banner are deferred follow-ups (separate tickets — privacy policy + banner work needed first).
- ✅ Tests: 5 unit tests on `core::events`, 18 integration tests on the emit + persistence + ingest paths, 4 Vitest cases on the web batcher.

**Verified live on prod** (api v19, jobs v7, web v43): both server and client emit paths flowing end-to-end into the `events` table.

**Deferred follow-ups (logged here for the next contributor):**
- `events.partition` (monthly) — T-016. Defer until volume justifies; current scale fits a single non-partitioned table comfortably.
- `inquiry_started` → `inquiry_submitted` funnel ratio dashboard. The data is now there; the analytics surface isn't.
- Cookie-consent banner + privacy-policy update before EU/UK launch. PII is in `context`; we're not GDPR-compliant on the disclosure side.
- Web Lambda → api forward of `X-Forwarded-For` so SSR-rendered events carry the user's real IP rather than the Lambda's. Currently 99% of `artwork_viewed` events show `user_agent = "node"` for this reason — same user, just attributed via the cookie not the IP.
**Where:** `core::events` (new module) + handler call sites in `search.rs`, `artwork.rs`, `inquiries.rs`, `studio/*`, `me/*`, `uploads.rs`.
**Why:** `0006_events_profiles.sql` shipped the table; no writer exists. Every taste-vector, recommendation, analytics, and CF feature below is blocked on event data flowing in. Single highest-leverage piece of plumbing on the post-launch board.
**Acceptance:**
- New `JobEvent::EventLog { name, anonymous_id, user_id, properties, context }` variant. Fire-and-forget enqueue from API handlers — never blocks the request path.
- Initial event set: `search_executed`, `artwork_viewed`, `artwork_saved`, `artwork_unsaved`, `inquiry_started`, `inquiry_submitted`, `modifier_applied`, `visual_search_uploaded`, `artist_viewed`, `neighborhood_viewed`, `artist_followed`, `artist_unfollowed`.
- Anonymous + signed-in paths both write; `T-033` merge logic now actually does something.
- Lands in lockstep with `T-016` so the writer never targets an unpartitioned table.
- Integration tests assert ≥1 event per relevant handler.
- Storage abstraction: handler-side only knows the `JobEvent::EventLog` variant. Storage destination is an implementation detail of `core::jobs::handle` and is swappable per the event-storage decision.

### ~~`T-051` Per-artwork + per-artist OG cards~~ — shipped 2026-06-18

- ✅ `web/src/app/artworks/[id]/opengraph-image.tsx` — 1200×630 split layout: primary artwork image on dark backdrop (left 630px), title + artist byline + domain footer in Instrument Serif on cream (right 570px). Title size clamps by length.
- ✅ `web/src/app/artists/[slug]/opengraph-image.tsx` — name + city left, 2×2 grid of `representative_image_urls` right. Pads with dark cells when artist has <4 representative images.
- ✅ Instrument Serif (regular + italic) bundled into `web/src/app/og-fonts/`; Turbopack hashes them into `.next/server/assets/` at build time.
- ✅ Page meta auto-wired by Next's `opengraph-image.tsx` convention — overrides the homepage `og.png` from `layout.tsx` per-route.
- ✅ `revalidate = 86_400` on both routes — social platforms re-crawl periodically; cache for a day.
- ✅ Fallback "Wander" card when the artwork/artist isn't found (deleted, unpublished, bad id) — never returns a broken share.

**Spike outcome (the gotcha):** the Next.js-docs pattern `fetch(new URL('./font.ttf', import.meta.url))` does **not** work under OpenNext on Node Lambda — Vercel's edge runtime supports `fetch('file://…')` but vanilla Node's undici throws `not implemented... yet...` on the `file:` scheme. Fix: read the bundled font with `readFile(fileURLToPath(new URL(…, import.meta.url)))`. Turbopack bundles the asset correctly either way; only the load path changes. Documented inline in both route files so the next person doesn't reach for `fetch` first.

### ~~`T-052` Follow-an-artist (Phase 1: graph + UI)~~ — shipped 2026-06-18

- ✅ Migration `0015_follows.sql` — `(user_id, artist_id)` PK + reverse `(artist_id, created_at DESC)` index.
- ✅ API: `POST /v1/me/follows/:artist_id` (idempotent UPSERT, 404 on unknown artist), `DELETE` (idempotent), `GET /v1/me/follows` (paginated list w/ slug + display_name + city/country + first thumb + `followed_at`).
- ✅ `ArtistDetail` now carries `is_following` (auth-conditional, defaults false when signed-out) + `follower_count`. Wired with `Option<AuthedUser>` so the public endpoint stays publicly readable.
- ✅ `GET /v1/studio/me` flattened into a `StudioMe` wrapper that adds `follower_count`. Forward-compatible via `#[serde(flatten)]` — the wire shape is identical for existing fields.
- ✅ Web: `<FollowButton>` client component on `/artists/[slug]` — optimistic flip, redirects to sign-in when signed-out (with `redirect_url=`), uses server actions so client bundle stays clean of `next/headers` + Clerk server-only modules.
- ✅ Web: artist page shows "N followers" pill when count > 0; `/studio` dashboard surfaces the same.
- ✅ 9 integration tests: auth gates / 204+listed / idempotent / 404 unknown / unfollow round-trip / per-user isolation / `is_following` true for signed-in follower / `is_following` false for signed-out + count still flows.
- ✅ Prod-verified: API endpoints, ArtistDetail new fields, signed-out artist page HTML contains the Follow control.

**Phase 1 limits, captured for follow-ups:**
- Events: `JobEvent::EventLog` writes for `artist_followed` / `artist_unfollowed` are marked `TODO(T-050)` in the handlers — they're the obvious next signal source for the taste vector.

### ~~`T-052b` Follow-an-artist: notification digest~~ — shipped 2026-06-20

- ✅ Migration `0017_user_notification_log.sql` — `(user_id, kind, sent_on)` PK enables daily-cadence dedup via `INSERT … ON CONFLICT DO NOTHING RETURNING id`; secondary `(kind, sent_at DESC)` index for cohort-level reporting.
- ✅ `JobEvent::NotifyFollowersDigestKickoff` (no payload) + `NotifyFollowersDigestUser { user_id }`. Dispatch wired in `core::jobs::handle`.
- ✅ EventBridge cron at `cron(0 11 * * ? *)` (11:00 UTC daily) drops the kickoff event onto the existing SQS queue via `aws_cloudwatch_event_target` with constant input matching the JobEvent JSON shape; SQS queue policy scopes `events.amazonaws.com:SendMessage` to that specific rule.
- ✅ Kickoff handler: SQL scan on `follows` × `artworks` × `artists` filtering on `a.published_at > GREATEST(f.created_at, now() - interval '24 hours')` and `NOT EXISTS` against `user_notification_log` for today; per-candidate `user_wants` check; SQS fan-out to per-user handler.
- ✅ Per-user handler: claims today's slot via `INSERT … ON CONFLICT DO NOTHING RETURNING`, defensively re-checks `user_wants` (preferences may have flipped between scan + delivery), pulls payload via the same per-follow window query (cap 12 artworks), builds the digest with `templates::new_works_digest`, sends via `EmailClient::send_notification`.
- ✅ `EmailClient::send_notification` wraps `send` adding `List-Unsubscribe: <url>, <mailto:>` + `List-Unsubscribe-Post: List-Unsubscribe=One-Click` headers (RFC 8058) so Gmail/Outlook honour the URL for one-click. `SentEmail` gains a `headers` field so the test backend can assert on them.
- ✅ `core::notifications::unsubscribe_url(base, token)` — single source of truth for the `/u/<token>` URL shape. Same kind enum + HMAC machinery as T-068.
- ✅ Email template `templates::new_works_digest`: subject `"1 new work from {artist}"` (single) or `"N new works from artists you follow"` (multi); body groups artworks under artist headings with thumbnails + titles; footer carries the visible unsubscribe link + the manage-preferences link.
- ✅ Backfill semantics use `follows.created_at` per-follow + a 24h floor (no `notifications_started_at` column needed) — a new follow today never backfills the artist's archive, an existing follow at launch only sees future artworks.
- ✅ Local-dev triggerability: `cargo run -p jobs-worker -- --enqueue '<json>'` flag drops any `JobEvent` into the local Postgres jobs table and exits; the worker loop picks it up on next poll.
- ✅ `JobsDeps` gains `anon_cookie_secret` (for token signing) and `jobs: JobsBackend` (so kickoff can fan out per-user jobs); jobs-lambda + jobs-worker construct both; tests updated.
- ✅ Jobs Lambda IAM + env: `sqs:SendMessage` on the main queue + `JOBS_QUEUE_URL` env var. The Lambda is now both consumer and producer.
- ✅ 11 integration tests in `digest_test.rs` covering kickoff (positive, no-new-work, already-sent-today, master-off, per-kind-off, per-follow backfill window) and per-user handler (email send, idempotency, opt-out re-check, multi-artist subject, empty-payload silence).
- ✅ Prod-verified end-to-end: manual SQS enqueue → kickoff handler logged `"digest kickoff: candidate users scanned",candidates:0` + `"digest kickoff: per-user jobs enqueued",enqueued:0` (correct empty-state behaviour — no users currently have follows with new artworks in the 24h window).

**Deferred from this slice:**
- "Skip if user viewed any work by this artist in last 24h" — gated on `T-050` events writer. Layer on when T-050 lands.
- Per-artist mute. Defer to follow-up.
- Quiet hours per user timezone. Defer until non-US user base appears.

**Original spec (kept for archaeology):**
**Where:** `JobEvent::NotifyFollowersDigestKickoff` + `NotifyFollowersDigestUser` variants + handlers in `core::jobs`; cron trigger via `aws_cloudwatch_event_rule` → SQS; new `core::emails::templates::new_works_digest`; `user_notification_log` table; `core::emails::EmailClient::send_notification` for List-Unsubscribe header wiring; `core::notifications::unsubscribe_url` utility.
**Depends on:** `T-068` (preferences spine). Builds on `T-052` follow graph (shipped 2026-06-18).
**Why:** Phase 1 shipped the graph; this turns it into the actual retention loop — a user signs up, follows 3-5 artists, comes back when one of them publishes new work. Daily-batched (avoids hammering the inbox when an artist seeds 10 works in one session); reuses the per-artwork OG cards from `T-051` for share-friendly previews when followers click through.

**Backfill semantics (no `notifications_started_at` column needed):**
The digest filter is `artworks.published_at > GREATEST(follows.created_at, now() - 24h)`. That single clause handles every case correctly: a new follow today never backfills the artist's archive; an existing follow at launch only sees future artworks; the 24h floor caps the lookback for old follows so one daily run can't dump a week of works.

**Idempotency under SQS at-least-once:**
`user_notification_log` PK includes `sent_on date` (date-truncated). Per-user handler does `INSERT ... ON CONFLICT (user_id, kind, sent_on) DO NOTHING RETURNING id` — only the row that *won* the insert proceeds to send. Survives SQS redeliveries without double-emails. Weekly-cadence notifications (T-059, T-060) layer a parallel `sent_week` constraint or a different kind name later.

**Acceptance:**
- Migration `0017_user_notification_log.sql`: `user_notification_log (user_id uuid, kind text, sent_on date default current_date, sent_at timestamptz default now(), context jsonb, PRIMARY KEY (user_id, kind, sent_on))`. `sent_at` for audit; `sent_on` for the dedup constraint.
- New JobEvent variants: `NotifyFollowersDigestKickoff` (no payload) and `NotifyFollowersDigestUser { user_id: Uuid }`.
- Daily cron at 11:00 UTC: `aws_cloudwatch_event_rule` (rate-based; us-east-1 default) → SQS target with constant input `{"NotifyFollowersDigestKickoff":{}}` matching the JobEvent JSON shape. Plus `aws_sqs_queue_policy` allowing `events.amazonaws.com` to `sqs:SendMessage`.
- Kickoff handler: SQL scans for users with ≥1 follow whose followed artist published an artwork within their per-follow `GREATEST(f.created_at, now() - 24h)` window AND who have no `user_notification_log` row with today's `sent_on`. For each, calls `notifications::user_wants(pool, user_id, NewWorksDigest)`; enqueues `NotifyFollowersDigestUser { user_id }` per qualifying user.
- Per-user handler:
  1. `INSERT INTO user_notification_log ... ON CONFLICT DO NOTHING` returns the affected row; if none, the handler returns silently (another worker handled it).
  2. Build digest payload (per-artist groups, cap 12 artworks total, sort by `published_at DESC`).
  3. If empty, no-op (defensive — shouldn't get here from the kickoff query).
  4. Build email via `templates::new_works_digest`. Subject: `"1 new work from {artist}"` (single artist) or `"{N} new works from artists you follow"` (multi).
  5. Mint unsubscribe token via `mint_unsubscribe_token(user_id, NewWorksDigest, anon_cookie_secret)` from T-068.
  6. Send via `EmailClient::send_notification(email, unsubscribe_url)` — wraps the existing `send` adding `List-Unsubscribe: <url>, <mailto:>` and `List-Unsubscribe-Post: List-Unsubscribe=One-Click` so Gmail/Outlook honour the URL for one-click.
- Local-dev triggerability: `cargo run -p jobs-worker -- --enqueue '<json>'` flag inserts a row into the local Postgres `jobs` table; the polling loop picks it up. Plus a `make trigger-digest` shortcut.
- Empty-state behaviour: silent.
- Tests: kickoff finds the right users, kickoff excludes already-sent-today users, kickoff respects `user_wants` master + per-kind, per-user handler is idempotent (re-running on same day no-ops), per-follow backfill window correct, multi-artist grouping correct, cap-at-12 preserves most-recent-first, empty payload doesn't send.

**Deferred from this slice:**
- "Skip if user viewed any work by this artist in last 24h" — gated on `T-050` events writer. Layer on when T-050 lands.
- Per-artist mute. Defer to follow-up.
- Quiet hours per user timezone. Defer until non-US user base appears.

### ~~`T-068` Email-notification preferences + unsubscribe machinery~~ — shipped 2026-06-20

- ✅ Migration `0016_notification_preferences.sql` — `notification_preferences (user_id, kind, enabled, updated_at)` table with composite PK + partial index on enabled rows; `users.global_email_notifications_enabled boolean default true` master kill switch. Default-on semantics implicit (no row = enabled).
- ✅ `core::notifications::NotificationKind` enum (`InquiryVerification`, `InquiryReply`, `NewWorksDigest`). `is_transactional()` method; user-facing kinds excluded from the toggle UI.
- ✅ `core::notifications::user_wants(pool, user_id, kind)` — single chokepoint. Returns true immediately for transactional kinds; otherwise checks master kill switch then per-kind row (default true).
- ✅ JWT-based unsubscribe token via `jsonwebtoken` (already a dep) — `mint_unsubscribe_token` + `verify_unsubscribe_token`. HS256 over `(sub: user_id, kind, exp)`, 90-day TTL. Signed with `ANON_COOKIE_SECRET` (existing infra, no new SSM param). Constant-time signature comparison via the jsonwebtoken crate.
- ✅ API `GET /v1/me/notification-preferences` — returns full preference map with every user-facing kind defaulted to true + master flag.
- ✅ API `PATCH /v1/me/notification-preferences` — sparse partial update; validates kind names (400 on unknown or transactional) before any DB write.
- ✅ API `POST /v1/notifications/unsubscribe` (no auth — token IS the credential) + `POST /v1/notifications/unsubscribe/oneclick` (returns 204 for RFC 8058 mail-client one-click).
- ✅ Web `/me/settings` index page (future home for account / privacy / data-export sections).
- ✅ Web `/me/settings/notifications` — server-rendered initial state + `<NotificationSettingsForm>` client component with optimistic toggles + server-action persistence.
- ✅ Web `/u/[token]` route — GET redirects to `/u/confirm?token=...` (user-friendly), POST returns 204 (Gmail/Outlook one-click).
- ✅ Web `/u/confirm` — server-renders the unsubscribe action, shows "you're unsubscribed from {friendly_label}" with a CTA back to settings.
- ✅ `<UserButton>` dropdown gets a "Settings" link to `/me/settings` via Clerk's `<UserButton.MenuItems>`.
- ✅ 7 unit tests (token round-trip, kind enum, tamper rejection, unknown kind, transactional detection, user-facing exclusion) + 13 integration tests (auth gates, defaults, partial PATCH semantics, per-user isolation, unsubscribe round-trip, idempotency, bad-token rejection, wrong-secret rejection, one-click 204).
- ✅ Prod-verified: `/v1/me/notification-preferences` requires auth, `/v1/notifications/unsubscribe` rejects bad tokens, web auth gating works, GET on `/u/<token>` redirects to confirm, POST returns 400 on bad tokens.

**Notes for `T-052b` (next):** the email-side wiring lives here. When `T-052b` sends the new-works digest, it calls `core::notifications::user_wants(NewWorksDigest)` to gate the send, then mints an unsubscribe token via `mint_unsubscribe_token(user_id, NewWorksDigest, anon_cookie_secret)` and embeds it in the footer as `https://wander.gallery/u/<token>` plus a `List-Unsubscribe: <https://wander.gallery/u/<token>>, <mailto:unsubscribe@wander.gallery>` header (with `List-Unsubscribe-Post: List-Unsubscribe=One-Click` so Gmail uses the URL path).

### ~~`T-052c` Follow-an-artist: anonymous queueing~~ — shipped 2026-06-20

- ✅ Migration `0018_anon_pending_actions.sql` — generic `(anon_id, kind, payload jsonb, created_at, expires_at)` table with composite unique index on `(anon_id, kind, payload)` for dedup + secondary indexes on `anon_id` (drain) and `expires_at` (future cleanup job). Generic shape so future intents (save-to-collection, inquiry-start) plug in by adding a `kind` value, not a new table.
- ✅ Picked server-side table over cookie storage — anon `X-Anonymous-Id` cookie stays minimal; intents survive cookie-size limits + are queryable for debugging. See "Server-side anon pending actions" decision in `decisions.md`.
- ✅ TTL: `expires_at` defaults to `now() + interval '7 days'`. Drain query filters on `expires_at > now()`. (Periodic cleanup job to delete expired rows is a follow-up — not blocking.)
- ✅ Per-anon cap of 50 pending intents enforced at API; over-cap returns a clean `BadRequest` with copy that points the user at signing in to flush.
- ✅ New endpoint `POST /v1/anon/pending/follows/:artist_id` — auth is the signed `X-Anonymous-Id` header (the `OptionalAnonId` extractor). 400 with no header, 404 for unknown / soft-deleted artist, 204 on success. Idempotent.
- ✅ Extended `POST /v1/me/merge-anonymous` (T-033) to drain `anon_pending_actions` for the cookie's anon_id. Each `follow_artist` payload replays as a `follows` INSERT (idempotent on `(user_id, artist_id)` PK). All rows for the anon_id are deleted post-replay — recognised + unknown kinds, valid + expired — so a stale intent never fires later when this code learns a new kind name.
- ✅ `MergeResponse` gains `follows_replayed: u64`; emitted only when > 0 so the existing T-033 callers can ignore the new field cleanly.
- ✅ Web `<FollowButton>` signed-out branch wraps the redirect in a `startTransition` that first calls `queueAnonFollowAction` (server action → `/v1/anon/pending/follows/:id` with the cookie's anon-id). Best-effort: a queue failure logs to Sentry and proceeds with the redirect anyway.
- ✅ 8 integration tests in `anon_pending_test.rs`: no-header → 400, unknown artist → 404, insert + idempotent re-insert, merge replays + drains, merge is idempotent when follow already existed, expired rows are drained-but-not-replayed, merge with no anon cookie is a no-op.
- ✅ Prod-verified: `POST /v1/anon/pending/follows/...` returns 400 without header and 404 for unknown artists.

**Open follow-up (not blocking):** scheduled cleanup of `expires_at < now()` rows. Today the table grows until users sign in or rows age out; cleanup keeps it tidy. Trigger to revive: row count > ~10k or query latency on drain feels slow.

### ~~`T-053` Shareable collections (public read)~~ — shipped 2026-06-18

- ✅ API: `GET /v1/collections/share/:share_id` — unauthenticated read, returns `CollectionDetail`. 404 indistinguishably for not-found / private / soft-deleted. Cheap pre-DB guard rejects malformed tokens (length + alphanumeric) before the query.
- ✅ Schema reuse: `user_collections.is_public` + `share_id` were already there from 0003. No migration. The existing `PATCH /v1/me/collections/:id` already mints + rotates `share_id` on public→toggle cycles.
- ✅ Factored `fetch_collection_artworks` out of the owner-side `detail` handler so both the owner read and the public read share the same filtering rules (published + active + approved primary image).
- ✅ Web: `/c/[share_id]/page.tsx` — public read-only view. `notFound()` on miss → clean 404.
- ✅ Web: `/c/[share_id]/opengraph-image.tsx` — per-collection OG card, 2×2 cover-image grid right + name + work-count left. Same Instrument Serif treatment as `T-051`.
- ✅ Web: `<CollectionShareControl>` client component on `/collections/[id]` — "Make public" button → `setCollectionPublicState` server action → renders the share URL + a Copy button + a "Make private" toggle. Explicit note in the UI that going private rotates the link.
- ✅ Server-action plumbing: `setCollectionPublicState` in `actions/collections.ts` so the client component doesn't pull `next/headers` + Clerk's server-only modules into the browser bundle.
- ✅ Privacy page updated: public collections may be indexed by search engines once shared; toggling private rotates the link.
- ✅ 5 new integration tests (19 collections total): happy / private / unknown / malformed / rotates-on-toggle-old-link-dies.
- ✅ Verified in prod: public surface routes return clean 404 + render fallback OG PNG on unknown tokens.

**Architectural note recorded inline:** the public-read handler currently lives in `me/collections.rs` despite not being a "me" route, to share the row types. If `api-search::collections` grows much more public surface area (T-058 series, T-057 neighbourhoods evolution), worth refactoring to a top-level `collections` module with `pub(crate)` row types.

### ~~`T-054` Inquirer-inbound replies (email-stitched threads)~~ — shipped 2026-06-22

- ✅ Migration `0019_inquiry_inbound_replies.sql` adds `from_role` (default 'artist'), drops NOT NULL on `artist_id`, adds nullable `inbound_message_id` with a partial unique index for replay-dedup.
- ✅ `core::reply_address` mints + verifies `r-<simple_uuid>-<hmac10>@reply.wander.gallery` (55-char local part, under RFC 5321's 64). Shared `anon_cookie_secret` reused via domain separation.
- ✅ `POST /v1/webhooks/email/inbound` — shared-secret auth (`X-Inbound-Secret`), HMAC verify, persist with `from_role='inquirer'`, enqueue `JobEvent::InquirySendReplyForward`. Returns `accepted` / `duplicate`.
- ✅ Studio inbox renders the thread with role-aware label + accent border on inquirer rows ([`InquiryInbox.tsx`](web/src/components/studio/InquiryInbox.tsx)).
- ✅ Cloudflare Email Routing MX + SPF for `reply.wander.gallery` via TF (`modules/dns/email_routing.tf`); priorities reconciled per zone-assigned values.
- ✅ Cloudflare Email Worker (`infra/email-worker/`, `postal-mime` parse + strip quoted history + POST to webhook with a `User-Agent` so AWS WAF's `NoUserAgent_HEADER` doesn't block it).
- ✅ 4 integration tests covering the chain; 7 unit tests on the token shape.
- **Productionisation fixes during 2026-06-22 cutover:** demoted 5 AWS WAF body-content sub-rules to COUNT (false positives on image-upload + JSON-webhook traffic — see `decisions.md` 2026-06-22); flipped on WAF logging in TF; wired `S3_UPLOADS_BUCKET` env on api+jobs Lambdas; fixed STS-cred override in `core::object_store` (was AccessDenied'ing on every S3 PUT under role-based creds).

### ~~`T-055` User taste vector + nightly refresh~~ ✓ shipped 2026-06-26
`core::user_profile` computes a per-user L2-normalised weighted-sum taste embedding from event-linked artwork embeddings, decayed by `0.95^(weeks_old)` (soft 13-week half-life). Base weights v1: `inquiry_submitted=5`, `artwork_saved=3` (`-3` on unsave), `artwork_viewed=0.5`. `JobEvent::UserProfileRefresh { user_id }` + `JobEvent::UserProfileRefreshKickoff {}` (two-stage fan-out, mirrors T-052b digest pattern). Persisted to `user_profiles.taste_embedding`; sub-noise-floor results return None rather than writing a meaningless direction. 20 tests total (7 unit + 13 integration).

**Live cron deferred until real users onboarding** — same trigger condition as T-057 (`make neighborhoods-build` scheduling). The kickoff handler + `users_with_recent_activity` are wired; the EventBridge → SQS schedule wiring is the only thing missing. Manual local invoke: `cargo run -p jobs-worker -- --enqueue '{"kind":"user_profile_refresh_kickoff","payload":{}}'`.

**Deferred to follow-ups (noted in module docs):**
- `artist_followed` / `artist_unfollowed` — need artist-centroid join (`AVG(embedding)` over artist's artworks). Add when first real follow events appear.
- `modifier_applied` / `visual_search_uploaded` — no `artwork_id` in `properties`; would need a modifier-vector lookup / upload-embedding fetch.
- Anonymous user profiles — `user_profiles.user_id` FKs into `users` so pre-signin taste lives implicitly in events until T-033 anon-merge links it. Natural home for an anon-taste store is T-061 (calibrator).

### ~~`T-056` Personalised search re-rank + "For you" row~~ ✓ shipped 2026-06-29
Three sub-commits closing the first user-visible payoff of T-055:

- **T-056.1** — `GET /v1/me/recommendations/for-you`. Top-K nearest by HNSW cosine to `user_profiles.taste_embedding`, candidate pool 50 → random shuffle → return 12. Eligibility: signed-in + `interaction_count >= 5` + vector set. Below the gate the endpoint returns `{eligible:false, items:[]}` so the web layer can fall back cleanly.
- **T-056.2** — Homepage "For you" row swaps in for eligible users; "Recently added" stays the fallback. SSR fetch is skipped entirely for anonymous callers.
- **T-056.3** — RRF blend with a third `taste_ranked` channel in `/v1/search`. **Off by default** until we have data to A/B against. Operator switch: `SEARCH_PERSONALIZE_ENABLED` env. Per-request override: `?personalize=on|off`. Same eligibility gate. Per-request log line when active.

Threshold deliberately lowered from the spec's `>= 10` to `>= 5` — a completed T-061 calibrator session alone unlocks personalisation. Important for the cold-start story.

**Open follow-ups before flipping `SEARCH_PERSONALIZE_ENABLED=true`:**
- Cohort assignment + experiment harness (today the toggle is global; we'd want a user-id hash → on/off split for proper A/B).
- Result-quality eval — pick N queries, score top-K results with/without taste, eyeball drift. Don't ship default-on until this passes.
- Decide on jitter for `/v1/me/recommendations/for-you` — currently `ORDER BY random()` over the top-50; consider replacing with a deterministic-per-user-per-day shuffle so the row is stable across page reloads on the same day.

### `T-076` End-to-end personalisation validation
**Where:** Manual + (optionally) automated tests against staging or prod with a real Clerk signup. Some can also run locally.
**Why:** We have integration tests on the API + handler surfaces, but no E2E walk-through of the full anon → signed-in → personalised loop. Most failure modes (a broken Clerk webhook, a missed JWT scope on the for-you endpoint, an issue with the bridge route's cookie forwarding) only show up when the whole chain runs together. Worth doing once before the first real user, then converting the steps into an automated playwright check (T-069 already exists for the broader retention loop).

**Walk-through script (one user, ~5 min):**
1. Visit homepage in a private window — confirm calibrator panel renders, 5 pairs visible, images load.
2. Pick through all 5 pairs. Confirm "Thanks" banner shows. SQL check:
   ```sql
   SELECT event_name, anonymous_id IS NOT NULL AS has_anon, user_id IS NOT NULL AS has_user
   FROM events WHERE event_name='calibration_pick';
   ```
   → 5 rows, all `has_anon=t / has_user=f`.
3. Sign up (Clerk dev test email). Confirm redirect back to homepage. `AnonymousMergeBridge` fires automatically.
4. Re-run the SQL above — all 5 rows should now have `has_user=t` (T-033 anon-merge linked them).
5. Trigger the per-user refresh manually:
   ```bash
   USER_ID=$(psql "$DATABASE_URL" -tA -c "SELECT id FROM users ORDER BY created_at DESC LIMIT 1")
   cargo run -p jobs-worker -- --enqueue "{\"kind\":\"user_profile_refresh\",\"payload\":{\"user_id\":\"$USER_ID\"}}"
   ```
   (Needs `jobs-worker` running in another terminal locally; on prod the deployed Lambda picks it up from SQS.)
6. SQL check: `user_profiles` row exists with `taste_embedding IS NOT NULL`, `interaction_count = 5`, `profile_updated_at = recent`.
7. Reload homepage — confirm **"For you" row** appears in place of "Recently added", with artworks visually adjacent to the picks.
8. Toggle the RRF blend on for the session: visit `/search?q=anything&personalize=on` while signed in. Confirm 200 + reasonable results. Inspect api logs for `personalize=on user_id=Some(...) rrf_k=60` line.
9. Negative case: pass `?personalize=off` on the same URL → no log line, results match the default-off (config) shape.

**Acceptance:** A new user walking through steps 1-9 sees the expected state at each checkpoint. Bug log if any. Then convert into a playwright script under `e2e/` and chain it from CI on a nightly cadence (separate ticket if it grows).

**Depends on:** `T-061` ✓ shipped, `T-033` ✓ shipped, `T-055` ✓ shipped, `T-056` ✓ shipped. Unblocked today.

### `T-077` Activate the personalisation crons
**Where:** Schedule the two ML batch jobs that are currently dormant. EventBridge → SQS in prod; cron-poller in dev (or just rely on a daily `make` target).
**Why:** T-055.2 (taste-vector refresh kickoff) and T-057's `neighborhoods-build` are both code-complete but **not scheduled**. Pre-launch with no users, scheduling them is dead weight — the events table is empty and the cluster shapes don't drift. Once real artists are onboarding AND we have ≥1 signed-up consumer user, both need to run on a cadence.

**Trigger condition:** the first 5-10 real artists are publishing AND we have at least one consumer user who's completed a calibrator + done some real engagement. Until then, manual one-off runs are fine.

**Acceptance:**
- **`user_profile_refresh_kickoff`** — daily at, say, 03:00 UTC. EventBridge rule → SQS message with `{"kind":"user_profile_refresh_kickoff","payload":{}}`. Existing handler scans for users active in the last 25h, fans out one `UserProfileRefresh` per. CloudWatch metric on `enqueued` count for sanity (alarm if 0 for two days running once we expect non-zero).
- **`neighborhoods-build`** — weekly at, say, Monday 06:00 UTC. GitHub Actions cron preferred (decision in `decisions.md` 2026-06-26 — Python `hdbscan` is the canonical implementation). Repo secrets carry `DATABASE_URL` + `ANTHROPIC_API_KEY`. Slack hook on failure (or rely on Actions UI red badges).
- Documented in `decisions.md` with the per-job decision context (cadence rationale, why EventBridge + SQS vs Actions, etc.).

**Depends on:** `T-055.2` ✓ shipped, `T-057` ✓ shipped. Trigger is external (artists onboarding).

### `T-078` RRF blend production rollout
**Where:** Cohort harness + eval suite + finally flipping `SEARCH_PERSONALIZE_ENABLED=true` in the api Lambda env.
**Why:** T-056.3 shipped default-off — the code path is dark in prod. Activating it for everyone at once is the highest-risk move on the search surface. We need (a) a way to measure what changed, and (b) a controlled rollout.

**Acceptance:**
- **Eval harness.** Define ~20 representative queries (mix of broad like "landscape", narrow like "watercolour boats", and emotional like "calm interior"). Build a `make search-eval` target that hits both `/v1/search?q=X` and `/v1/search?q=X&personalize=on` for a fixed seeded user, dumps top-K JSON side-by-side. Initial pass is eyeball-only; later add a numeric proxy (e.g. how many top-12 results overlap, how much rank shifts).
- **Cohort assignment.** Hash the user_id to a 0..99 bucket; opt buckets in by config (`SEARCH_PERSONALIZE_COHORT_BUCKETS=0-9` = 10% rollout). The search handler resolves the cohort at request time when `?personalize=` is absent. Document in `decisions.md`. Cheaper than the obvious "set a `users.personalize` column" — flips in a single env var without migrations.
- **Telemetry.** When the blend activates, emit a `search_personalized` event (T-050 surface) with `{cohort, rrf_k, has_text, has_visual_anchor}` so we can later join to downstream `inquiry_submitted` / `artwork_saved` events and measure conversion lift per cohort.
- **Rollout plan:** 10% → eyeball + check error rate / latency for a week → 50% → another week → 100% → flip default-on. Each step is one env var change + restart.

**Depends on:** `T-056.3` ✓ shipped. Trigger is enough real users (≥ ~50 signed-in actives in any given week) to make A/B statistically informative.

### `T-079` Multi-image artworks in semantic search — review when first complaint lands
**Where:** `core::artwork_embeddings` write path + `artwork_embeddings` table PK + `search.rs` semantic CTE.
**Why:** Today an artwork has exactly one row in `artwork_embeddings` keyed on `(artwork_id, model_name, model_version)`. The studio upload pipeline only embeds the **primary** image (`studio/artworks.rs:637` — `if is_primary && state.embedder.enabled()`). Non-primary images are stored in S3 and shown in the artwork gallery, but contribute zero signal to semantic search, similar-artworks, taste-vector matching, or cluster centroids. Invisible today (WikiArt seed has 1 image per artwork); will start mattering once real artists upload multi-angle / multi-view / multi-colourway pieces.

**Three approaches when this becomes worth fixing:**
- **MAX-over-images.** Extend the PK to `(artwork_id, image_id, model_name, model_version)`; at query time `GROUP BY artwork_id MIN(distance)`. Best quality — sculpture-from-5-angles becomes findable from any angle. 5× storage + 5× HNSW candidates per scan.
- **MEAN-over-images.** Keep one row per artwork; recompute as `AVG(per-image embeddings)`. No PK change. Smooth aggregate vibe; an outlier detail shot muddies the centroid.
- **Status quo + studio nudge.** No code change. Add a "this image drives how you appear in search — pick your best one" hint on the primary-image picker.

**Acceptance for the eventual fix:**
- Decide based on a real complaint, not speculation. Until then, ship the studio nudge so artists at least know which image counts.
- Whichever approach lands needs to update: the studio write path (embed all vs just primary), the search SQL (GROUP BY or AVG), the similar-artworks endpoint, the T-055 taste-vector JOIN (currently keys on `(artwork_id, model_name, model_version)`), and the T-057 cluster build (averages embeddings per cluster).
- Add a one-off backfill to write the new shape over the existing corpus.

**Trigger:** First real complaint that "my piece doesn't show up when I search for X angle / Y colour" that can be traced to this. Or routine audit shows ≥ ~10% of artworks have non-primary images and we're getting silent retrieval misses.

### `T-081` Venues — galleries/shops as discovery destinations
**Where:** New migration (`venues` + `venue_artworks` tables); new `api-search::venues` module; new `/v1/studio/venues/*` + `/v1/venues/*` endpoints; new web routes `/venues`, `/venues/:slug`, `/studio/venues/*`; map integration via `searchMapClient`.
**Depends on:** T-083 (admin surface) for the `pending_review → active` verification flip.
**Why:** Not every independent contemporary artist has their own studio worth visiting. Galleries, project spaces, and curated shops give the discovery story a richer supply side: "see this work in person at X" rather than only "see this work online." Widens the platform's reach without changing what an artwork *is* — venues are a parallel pin source on the map, with a controlled consent flow between venue admins and the artists they represent.

**Decisions confirmed 2026-06-29:**
- Multiple venues per user account (galleries with branches, shops with several locations).
- One-direction consent: **venue invites artwork → artist accepts/declines**. Bidirectional volunteer-flow deferred — keeps abuse vectors low for v1.
- Public verification: new venues default to `status='pending_review'`; admin (see T-083) flips to `active` before public listing. Mirrors the artist-onboarding admin gate.
- Single concept per row: *"this artwork is at this venue"* — no separate "on display" vs "for sale" distinction in v1. Whether the work is purchasable is already on the artwork (`availability`).
- Cascade-clear on artwork delete: `venue_artworks.artwork_id` FK → `ON DELETE CASCADE`. The artwork no longer exists, so it can't be at a venue.
- Single owner per venue (`venues.owner_user_id` FK to users). Multi-admin co-ownership deferred until requested.

**Acceptance:**
- Migration `0024_venues.sql`: `venues (id, slug, name, kind text check kind in ('gallery','shop','studio_collective','cafe_collab','other'), about, address, city, country, lat, lng, geocoded_at, website_url, instagram_handle, opening_info, owner_user_id, status text check status in ('pending_review','active','paused','declined'), created_at, updated_at, deleted_at)` + `venue_artworks (venue_id, artwork_id, status text check ('pending','accepted','declined'), requested_at, decided_at, primary key (venue_id, artwork_id), artwork_id fk on delete cascade)`. Indexes: `(slug)` unique partial on non-deleted; `(owner_user_id)`; `(status, lat, lng)` partial for map scan; `(artwork_id, status)` for "currently at" reads.
- `core::venues` module: row types, helpers, geocoding reuse via `JobEvent::VenueGeocode` (mirror `ArtistLocationGeocode`).
- Studio API:
  - `POST /v1/studio/venues` (create — `pending_review`)
  - `PATCH /v1/studio/venues/:id` (partial — owner only — `deserialize_double_option` per T-072)
  - `DELETE /v1/studio/venues/:id` (soft via `deleted_at`)
  - `GET /v1/studio/venues` (list own venues)
  - `POST /v1/studio/venues/:id/artworks/:artwork_id` (invite — creates `pending` row; 404 if artist soft-deleted artwork)
  - `DELETE /v1/studio/venues/:id/artworks/:artwork_id` (uninvite; idempotent)
  - `GET /v1/studio/venues/:id/artworks` (paginated; includes pending/accepted/declined)
  - Artist-side: `POST /v1/studio/venue-requests/:venue_id/:artwork_id/accept`, `.../decline` (artist accepts/declines a venue's invite for their own artwork)
  - `GET /v1/studio/venue-requests` (artist's pending inbox)
- Public API:
  - `GET /v1/venues` (paginated; `status='active' AND deleted_at IS NULL`; supports `?bbox=`, `?city=`)
  - `GET /v1/venues/:slug` (404 indistinguishably for not-found / pending / deleted)
  - `GET /v1/venues/:slug/artworks` (only `accepted` rows)
  - `GET /v1/search/map` extended to optionally include venue pins (`?include=venues` or merged by default — TBD during build)
  - `ArtworkFull.venues: Vec<VenueRef>` — list of accepted venues for "Currently at" surface
- Web:
  - `/studio/venues` — list owner's venues + create button; modal follows URL-driven pattern (`?id=`, `?tab=`) per `docs/ui-patterns.md`. Multi-step: details / artworks tab.
  - Artwork invitation in studio: multi-select grid mirrors `SeriesEditModal`'s artworks tab.
  - `/studio/venue-requests` — artist's inbox; accept/decline per row.
  - `/venues` — public index w/ grid + map overlay.
  - `/venues/:slug` — public detail page.
  - `/artworks/:id` — "Currently at:" strip linking to venue pages.
  - Map: venue pins styled distinctly from artist pins (different colour or icon).
- Verification flow (admin side, T-083): admin queue at `/admin/venues` lists `pending_review`; approve flips to `active`, decline flips to `declined`.
- Tests: ≥10 integration tests (CRUD, consent flow, ownership boundaries, public visibility gates, cascade-clear).

**Subcommit plan:**
- **T-081.1** — Schema + backend (migration, all venue + venue_artworks endpoints, consent flow, geocoding via JobEvent reuse, public reads, tests).
- **T-081.2** — Studio UI (venues list + edit modal + artwork-invite grid + artist's venue-requests inbox).
- **T-081.3** — Public surfaces (`/venues` index, `/venues/:slug` detail, map integration, ArtworkFull "Currently at" strip).
- **T-081.4** — Docs (decisions.md entry; CHANGELOG; STRATEGY.md note).

**Deferred follow-ups (don't block v1):**
- Bidirectional consent (artist volunteers work to a venue).
- Multi-admin co-ownership of a venue.
- Opening hours as structured data (today: free-text `opening_info`).
- Venue-level events / "this work on display until X date".
- Venue follow / notifications (parallel to artist follow).

### ~~`T-082` Refine-with-text on visual search~~ ✓ shipped 2026-06-30

Three sub-commits closing the feature end-to-end:

- **T-082.1** — Backend. `refine_ranked` CTE in `search.rs`'s `run_hybrid`, fourth RRF channel alongside keyword + semantic + taste. Joins the candidate-contribution clause (refine can pull in works the visual anchor missed) and adds `1/(60+rk)` to rrf_score. Only fires when a primary signal is also set (q / image_upload_id / seed_artwork_id); refine alone is silently dropped. Defensive 500-char input cap. 5 integration tests.
- **T-082.2** — Web UI. New `RefineBar` component between `ModifierBar` and `FilterBar`. Collapsed "+ Add refinement" → text input + Apply → active chip with × to clear. URL-driven via `?refine=…`. UI gates on primary-signal presence so the affordance never lies.
- **T-082.3** — Docs. `decisions.md` 2026-06-30 captures the alternatives (pre-blend into anchor, keyword boost, replace fixed modifiers, δ-vector compose) and why a 4th RRF channel is the right shape. `CHANGELOG.md` entry. `search_executed` event gains a `refine` property for funnel analytics.

**Original scope (kept for archaeology):**
**Where:** `api-search::search` (new `refine_ranked` CTE); `web/src/components/FilterBar.tsx` or a sibling refine control; URL param `?refine=...`.
**Why:** Visual search today is: "this image as the anchor + optional fixed-vocabulary modifiers (moodier / warmer / etc)." The modifiers cover a fixed delta vocabulary; they don't help when a user wants to say "this painting but more abstract" or "this sculpture but in stone." Free-form refine text adds an open-ended channel without dropping the visual anchor.

**Decisions confirmed 2026-06-29:**
- Implementation: **new fourth RRF channel (`refine_ranked`)**, not vector-blending with the existing anchor. The anchor stays untouched (user said "I want things like this image"); refine adds a *separate* preference signal that RRF blends in. Rejected: pre-blending refine into the visual vector at some α — feels hacky and loses the per-channel ranking guarantee.
- Composes with both text-search (`?q=`) and visual-search (`?image_upload_id=` / `?seed_artwork_id=`). Refuses to fire when no primary signal is set — alone, it'd just be regular text search, so we reject it gracefully (no `refine_ranked` CTE built).
- Naming: "Refine" (clear, neutral, doesn't promise too much). Alternatives considered: "And also...", "Steer", "Nudge", "Modify" — all worse.
- Does **not** replace the fixed-vocabulary modifiers. The two coexist: modifiers are one-click quick deltas anchored to WikiArt; refine is free-form. Different ergonomics for different intents.

**Acceptance:**
- API: `?refine=TEXT` on `/v1/search`. Embed via `Embedder::embed_text` (cache hit via existing `query_cache`).
- `search.rs`'s `run_hybrid` gains a `refine_ranked` CTE: `ROW_NUMBER() OVER (ORDER BY ae.embedding <=> $refine_vec)`. Added to the `rrf_score` SELECT as `1.0 / (60 + COALESCE(rr.rk, candidate_pool_size + 60))`. Same shape as `taste_ranked`.
- Per-request log line: `refine=<true|false>` + length of TEXT.
- Tests: refine alone (no primary) → behaves like keyword (no CTE built); refine + q → both channels in the RRF; refine + image → both channels; refine text length cap (defensive, ~500 chars).
- Web UI:
  - Collapsed `Refine →` button under the main search controls on `/search`. Click expands a text input + Apply button.
  - URL-driven: `?refine=TEXT`. Cleared via a small × on the active-refine chip.
  - Visible on every result view (text-search, visual-search, seed-artwork).
- Telemetry: `refine_applied` event (T-050 surface) on apply.
- Default off (no `refine_ranked` weight tuning needed — RRF is naturally calibrated by the `1/(60+rk)` shape).

**Subcommit plan:**
- **T-082.1** — Backend: `refine_ranked` channel + tests.
- **T-082.2** — Web UI: refine input on `/search`.
- **T-082.3** — Docs + decisions.

### ~~`T-083` Admin surface — approval queues + audit log~~ ✓ shipped 2026-06-30

Four sub-commits closing the foundational admin surface:

- **T-083.1** — Schema + backend. Migration `0024_admin_audit_log.sql` adds the audit table + bootstrap UPDATE on `mrjoshuajmatthews@gmail.com`. `core::auth::ADMIN_EMAILS` + auto-promote in `upsert_user`. `core::admin::audit::record` chokepoint. `AdminUser` extractor in api-search (403 for non-admins). `/v1/admin/artists` endpoints: list (paginated, status-filtered) + approve / decline / pause / unpause via a shared `transition` helper that asserts legal source state, writes audit, applies UPDATE. 11 integration tests.
- **T-083.2** — Web `/admin` shell + artists queue. Layout-level `notFound()` gate for non-admins. `/admin` index with queue-count tiles. `/admin/artists` with status tabs + `AdminArtistRow` client component (useConfirm for destructive transitions; toast.success on result).
- **T-083.3** — Image moderation override queue. Backend `/v1/admin/images` list + `POST /:id/override` (rejected → approved, clears `moderation_reason`, writes audit). Wire shape adds a `url` field built from `s3_key`. Web `/admin/images` page + `AdminImageRow` (50%-opacity + grayscale thumbnails; reason code chip; override goes through useConfirm). 5 more integration tests.
- **T-083.5** — Audit log viewer + docs. `GET /v1/admin/audit-log` paginated read-only feed (joins admin email). `/admin/audit-log` web page with `<details>` diffs of before/after JSON. `decisions.md` 2026-06-30 captures the eight design choices. `CHANGELOG.md` entry. 2 more integration tests (total: 18).

(T-083.4 — venue approval — slots into T-081 since the venue schema doesn't exist yet.)

**Design choices** (full record in `decisions.md` 2026-06-30):
- `users.is_admin` column over a separate roles table.
- Auto-promote on first sign-in from a hardcoded `ADMIN_EMAILS` list.
- Audit row written **before** the mutation it audits (captures intent even on failed mutations).
- 403 from the API, 404 from the web layer (different lies for different audiences).
- Idempotent re-application skips the audit; illegal source-state is 409.

**Deferred follow-ups:**
- Inquiry abuse / report queue. Real once we have signed-up users at volume.
- Scoped admin roles (read-only vs full mutate). Migrate when second admin appears.
- 2FA enforcement on admin accounts (Clerk-side config).
- Audit retention windowing. The table is tiny in row count for years.
- Re-pending a declined artist. UI affordance needs designing; not blocking pre-launch.

### `T-080` Currency-aware price filter — canonical GBP
**Where:** New migration (`fx_rates` table + `artworks.price_gbp_cents` column); `core::fx` module; new `JobEvent::FxRatesRefresh`; `search.rs` filter swap; FilterBar label changes.
**Why:** The price filter today compares raw `price_cents` regardless of currency — so a `Under £500` filter matches a $500 painting (≈£395) AND a €500 painting (≈£430) as if they're the same value. Wander accepts USD/GBP/EUR/CAD/AUD/JPY on the artwork row; the filter is currency-blind.

**Decision: GBP as the canonical platform currency.** Initial focus is UK artists; comparing in GBP is the right anchor. Artwork cards keep showing the artist's native currency (`formatPrice`); only the filter operates in GBP.

**Acceptance:**
- New `fx_rates (code text pk, rate_to_gbp numeric, fetched_at timestamptz)` table; seeded with mid-2026 approximations in the migration so day-1 search works.
- New `artworks.price_gbp_cents bigint` column; backfilled in the migration; indexed for the filter.
- `core::fx::refresh_rates` fetches from a free FX API (Frankfurter — ECB data, GBP-base, no key), upserts the table, then bulk-recomputes `price_gbp_cents` across all artworks. Wired through `JobEvent::FxRatesRefresh` so it runs on the existing jobs queue.
- Studio writes (`POST/PATCH /v1/studio/artworks`) maintain `price_gbp_cents` at insert/update time using the current rates.
- `search.rs` `build_filters` swaps `price_cents` → `price_gbp_cents` for `price_min`/`price_max` comparisons.
- FilterBar `PRICE_BUCKETS` switch to GBP amounts; pill labels use `£`.
- Cron not live yet (matches the T-077 deferral): trigger manually until real artists onboard. Daily rate drift is fine.

### ~~`T-057` Algorithmic neighbourhoods (HDBSCAN + Claude label)~~ ✓ shipped 2026-06-26
Pipeline lives at `ml/ml_art/neighborhoods.py`, runnable via `make neighborhoods-build`. HDBSCAN with `cluster_selection_method='leaf'`, `min_cluster_size=15`, `min_samples=2`, euclidean over L2-normalised embeddings — tuned after the initial `eom`/30 default produced one 856-artwork mega-bucket. Claude (Sonnet 4.6) labels each cluster with 5 centroid-nearest sample images; result persisted as `kind='semantic'`, top 3 clusters by size become `is_featured`. Pure-rebuild semantics (drops + re-inserts every run; no Hungarian-matching yet since no real bookmarks pre-launch). Curated set untouched — coexists by `kind` discriminator. Groq + Llama 4 Scout wired as a fallback/iteration provider behind `--provider groq`; baked off, Claude clearly wins on evocative register (`decisions.md` 2026-06-26). First prod build seeded 14 neighbourhoods from ~2000 eligible artworks.

**Follow-up: productionise the build schedule once real artists are onboarding.** While the corpus is static WikiArt-seed it's fine to rebuild ad-hoc; once real artists start adding work the cluster shapes will drift and we'll want a weekly rebuild. Recommended path is GitHub Actions cron (`0 6 * * 1`) shelling out to `make neighborhoods-build`, repo secrets for `DATABASE_URL` + `ANTHROPIC_API_KEY`. Rejected: porting to Rust + the jobs queue — the Python `hdbscan` package is the canonical implementation and the Rust reimplementations have smaller test surface; offline batch ML doesn't need to share infra with online services. See `decisions.md` 2026-06-26 for the labelling provider bake-off; the scheduling decision will get its own entry when we wire it up.

### ~~`T-058` Series concept for artists~~ ✓ shipped 2026-06-29

Three sub-commits closing the feature end-to-end:

- **T-058.1** — Schema + backend. New `series` table (per-artist-unique slug, statement, cover_artwork_id), `artworks.series_id` FK (ON DELETE SET NULL). `core::error::ApiError::Conflict` variant for 409s. Studio CRUD endpoints under `/v1/studio/series/*` including a bulk-replace `PUT /:id/artworks` for the multi-select primitive. Public reads at `/v1/artists/:slug/series` (hides empty) and `/v1/artists/:slug/series/:series_slug` (404s for empty). Studio artwork PATCH accepts `series_id`. 8 integration tests.
- **T-058.2** — Studio UI. New `/studio/series` page + nav link. `StudioSeriesManager` card grid; `SeriesEditModal` with two tabs (Details / Artworks) — the Artworks tab is the **multi-select checkbox grid** that powers atomic membership replace. Backend extension: `StudioArtworkSummary.series_id` so the modal pre-checks current members on open.
- **T-058.3** — Public artist-page integration. `?view=series` toggle on `/artists/:slug` (only renders when artist has ≥1 series). Public series detail page at `/artists/:slug/series/:series-slug` with cover + statement + artwork grid. Crumb back to the artist.

**Design choices** (full record in `decisions.md` 2026-06-29):
- One series per artwork (FK, not many-to-many) — simpler schema, dropdown UX, single-direction conflict resolution.
- Slugs unique per artist; 409 on collision rather than auto-suffix.
- Empty series studio-only — hidden from public lists + detail.
- No NFKD unicode fold in slug — keeps deps minimal; artist can manually edit.
- Multi-select checkbox grid over drag-to-assign — one mental model (manage from series side); the per-artwork dropdown handles the inverse direction.

**Deferred follow-ups:**
- **`ArtworkFull.series` on the public artwork DTO + "More from this series" cross-link on the artwork detail page.** Surfaces the series-as-cross-link from each artwork; the public series detail page already exists.
- **Per-artwork series dropdown in `ArtworkEditModal`.** Single-artwork series-set/clear from the artwork edit modal. Bulk multi-select covers the primary management workflow; this is a quality-of-life add.
- **Drag-to-assign.** Defer indefinitely; the multi-select grid is the better-tested mental model.
- **Manual ordering** of series on the artist page + artworks within a series. Default `created_at desc` for now; artists with narrative-driven sequences will want this eventually.
- **`/v1/series/:id` direct read** (the original TODO spec). Public reads currently go through the artist namespace; the bare id path is rejected for now since it'd compete with the artist namespace and isn't called by the UI.

### `T-059` Saved searches + alerts
**Where:** Migration (`saved_searches`); `core::user_searches`; weekly cron-enqueued job re-running each saved query.
**Depends on:** `T-068` (preferences spine) for the unsubscribe + opt-out plumbing. Adds `NotificationKind::SavedSearchAlert`.
**Why:** Etsy / Saatchi-style retention. Composes with the taste vector — a saved search is both a notification trigger and a strong explicit-intent signal feeding `T-055`.
**Acceptance:**
- Migration: `saved_searches (id, user_id, name, params jsonb, frequency text default 'weekly', last_notified_at, last_match_max_id, created_at)`.
- "Save this search" affordance on `/search`. Persists current query string + filter state.
- Weekly job: per saved search, re-run with `since=last_notified_at`, check `user_wants(NotificationKind::SavedSearchAlert)`, email up to top-5 new matches.
- Per-search mute via the search row itself; global notification opt-out via `T-068` machinery.
- `/me/saved-searches` management page (separate from `/me/settings/notifications`).

### `T-060` Discover Weekly digest
**Where:** Cron-driven variant of the `T-055` refresh job; new email template via `core::emails`.
**Depends on:** `T-055` (taste vector), `T-068` (preferences spine — adds `NotificationKind::DiscoverWeekly`).
**Why:** ML-driven equivalent of an editorial weekly. Same retention mechanic, taste-vector engine — explicitly preferred over editorial per `decisions.md` 2026-06-17.
**Acceptance:**
- Per-user, once per week: take the taste vector, sample 12 artworks the user hasn't seen (`events.artwork_viewed` cross-check), bias 8 from nearest clusters + 4 from far clusters for serendipity.
- New `templates::discover_weekly` Resend template — 4×3 grid, link straight to artwork pages.
- Skip if `interaction_count < 10`. Skip if user opened a previous digest within 2 days. Skip during quiet hours per user-TZ.
- Per-kind opt-out + master kill switch from `T-068`.

### ~~`T-061` First-session taste calibrator~~ ✓ shipped 2026-06-26
Backend at `api-search::calibrate` + frontend `CalibratePanel` on the homepage. `GET /v1/calibrate/pairs` samples 5 pairs from far-apart `kind='semantic'` cluster centroids via greedy farthest-first selection. `POST /v1/calibrate/pick` emits a `calibration_pick` event (weight 2.0 in T-055) with chosen + rejected artwork ids. Anon picks key on the `anon_id` cookie and fold into the user's taste vector at sign-in via T-033's anon-merge handler — no new schema, no separate taste-vector store. Panel auto-hides on returning visits via `localStorage["wander:calibrator"]`. 7 backend integration tests cover the math + endpoints; the frontend was verified via typecheck + lint (manual smoke-check during the next dev session).

**Followups:**
- A/B test calibrator-present vs absent once we have signed-in users — measure 7-day return rate, taste-vector cosine-stability over the first 10 events.
- Score-based pair selection (currently greedy farthest by euclidean over the raw centroids; could weight toward visually-recognisable / featured clusters more heavily).
- Re-trigger the panel on demand from /settings (e.g. "re-tune what we show you") once that surface exists.

### `T-062` Filter UI: price range slider
**Where:** `web/src/components/FilterBar.tsx` + URL params; API already supports `price_min`/`price_max` via `api-search::search`.
**Why:** "Something small under £500" is a real query we silently can't serve from the UI today. Schema + API have always been there; the only remaining gap is the slider component.
**Acceptance:**
- Currency-aware range slider via `lib/format.ts`.
- URL-driven via the established `useUrlState` pattern (matches the existing size/medium/availability chips).
- Mobile-friendly touch handles.

**Scope reduced 2026-06-29** — original ticket covered size + price + medium. Size shipped via `T-070`, medium multi-select shipped via `T-073`. Only price remains.

**Open gap on medium:** the `medium=` filter today is a preset enum that doesn't match what artists actually type. Artists publish free-text (`oil on linen`, `gouache and ink on cotton`) while users filter against a fixed list (`Painting`, `Print`, `Photography`). Either:
- (a) normalise artist input at save time against a controlled taxonomy (typeahead with free-text fallback into "Other"), or
- (b) retrieval-side mapping (Claude call on first publish picks the canonical bucket; cluster fallback via Jina embeddings).

T-057 / T-061 will need a stable medium taxonomy anyway for taste-vector grouping, so this isn't only a UX cleanup. Decide approach before T-062 lands the multi-select.

### `T-063` Inline "more like this" in grid
**Where:** `ArtworkCard` extension + a small flyout component; backend already has `GET /v1/artworks/:id/similar`.
**Why:** Pinterest-style discovery deep-dive. Engine exists; we just don't surface it inline.
**Acceptance:**
- Desktop: hover ≥600ms reveals a 4-thumb similar-works tray below the card.
- Mobile: long-press equivalent.
- Doesn't navigate the main page; clicking a similar thumb does.
- Lazy-loaded — only fires the fetch on hover threshold.

### `T-064` Lock API Gateway invoke URL to CloudFront-only
**Where:** `infra/modules/web/main.tf` — add a custom-header check on the integration; CloudFront origin config gains a shared secret header.
**Why:** The API Gateway invoke URL (`*.execute-api.us-east-1.amazonaws.com`) is publicly reachable today. Two real consequences:
- Direct hits serve the same Lambda as `wander.gallery` (Host is rewritten to the canonical host by parameter mapping, so content is correct) — but the address bar shows the ugly URL and search engines could index a duplicate-content copy.
- We initially tried a middleware-layer 308 redirect from API Gateway URL → `wander.gallery`. It produced an infinite loop because API Gateway's response handling rewrites absolute `Location` headers back to relative when the host matches the (rewritten) request Host. The middleware redirect is removed; the proper fix is to block direct hits entirely.

**Acceptance:**
- CloudFront origin `custom_header` adds a `X-CloudFront-Secret: <random>` (sourced from SSM).
- API Gateway integration / route has a request-validation rule (or a small Lambda authorizer) that requires the header and returns 403 otherwise.
- Direct curl to `https://*.execute-api…/` returns 403 with no body.
- Browser hits via `wander.gallery` continue to work unchanged.
- Rotate the secret via SSM; deploy script picks it up.

### `T-065` Re-attempt `@sentry/nextjs` on the web tier
**Where:** `web/next.config.ts`, `web/instrumentation.ts` (new), Sentry init.
**Why:** We deferred web Sentry in May because `@sentry/nextjs` 10.x injected a page-router `_error` stub during its post-build pass which OpenNext 4.0's `copyTracedFiles` couldn't reconcile. Both have shipped releases since. Today web errors only surface in CloudWatch Logs — Sentry on the Rust API + jobs already works, so we're flying half-blind on the bigger surface.
**Acceptance:**
- `pnpm add @sentry/nextjs`, `web/instrumentation.ts` + `web/instrumentation-client.ts` per Sentry's app-router docs.
- `next build` + `npx open-next build` succeed (the historical breakage point).
- Lambda env: `SENTRY_DSN_WEB` populated via `deploy-web.sh` SSM fetch (we already have the param).
- Verify an intentional `throw new Error("sentry-canary")` in a route surfaces in Sentry's wander-web project.
- Source maps uploaded to Sentry on build (Sentry's Webpack plugin handles it; OpenNext compat-check needed).

### `T-066` Consolidate Lambda config loading
**Where:** `scripts/deploy-web.sh` (drop the Python SSM-fetch + env-merge hack); web Lambda runtime gets the same in-process SSM fetch as api/jobs (or move to AWS Parameters & Secrets Lambda Extension if cold-start latency feels bad).
**Why:** Web's SSM-secret injection happens *at deploy time* via a custom shell + Python pipeline that fetches CLERK_SECRET_KEY and ANON_COOKIE_SECRET, then merges them into the Lambda's env via `update-function-configuration`. The Rust Lambdas do this *at runtime* via `core::config::bootstrap_ssm`. Asymmetric: rotating an SSM secret on the Rust side just needs a Lambda recycle (cold-start picks it up); on the web side it needs a redeploy. Picking one pattern reduces hack surface.
**Note:** the AWS Parameters and Secrets Lambda Extension is a good fit for both *if* cold-start frequency becomes a concern — but it doesn't help at our current scale (we already only fetch SSM once per cold start; the Extension caches across invocations, not across cold starts). Trigger to revive: cold-start latency complaints or sustained > 10 cold starts/min.

### `T-067` Type sharing Rust → TS
**Where:** generate `web/src/lib/api.ts` types from `api/crates/core/src/models.rs` via `ts-rs` (Rust-side derive) or schemars + `json-schema-to-typescript`.
**Why:** Today both files are maintained by hand. Drift risk: an API field rename only breaks at runtime. Caught us once with `is_following` (we shipped the Rust side then realised the TS side was stale). Risk grows with surface area.
**Acceptance:** every `pub struct` in `models.rs` with `#[derive(Serialize)]` round-trips to a TS `export interface` automatically on `cargo build` (or as a separate `make types` target); CI fails if the generated file drifts from what's checked in.

### ~~`T-073` Canonical medium taxonomy + filterable category~~ — shipped 2026-06-25

- ✅ Migration `0021_artwork_medium_category.sql` — adds `artworks.medium_category text` with a CHECK constraint pinning the 11-value v1 taxonomy + partial btree index.
- ✅ `core::media::CATEGORIES` — single source of truth; 3 unit tests pin count + reject typos.
- ✅ `core::validation::medium_category_v1` (strict, write-path) + `parse_medium_query` (tolerant, read-path). 7 new unit tests.
- ✅ POST + PATCH `/v1/studio/artworks` accept `medium_category` with T-072 double-option semantics. 3 new integration tests.
- ✅ `/v1/search?medium=` is now multi-value comma-separated against `medium_category`. Unknown tokens dropped silently. 3 new integration tests + 2 existing tests rewritten.
- ✅ Public `ArtworkFull` + Studio response shapes carry the new field.
- ✅ `scripts/backfill-medium-category.sh` — rules-first ILIKE + WikiArt-style mapping. 2000/2000 prod rows backfilled. Idempotent.
- ✅ Web: `lib/medium.ts` constants + helpers (`mediumLabel`, `formatMedium`, `isMediumCategory`); 11 new Vitest unit tests.
- ✅ Web: `ArtworkEditModal` — category select + renamed "Materials" free-text; soft-confirm on publish-without-category-or-dimensions (combined dialog).
- ✅ Web: `FilterBar` — multi-select pill with toggle items + active count display (`Medium: 2 selected`). Old MEDIUM_OPTIONS list removed.
- ✅ Web: `/artworks/[id]` + studio portfolio tile use `formatMedium` to render `Painting · Oil on linen`.

**Deferred follow-ups:**
- Move `STYLE_TO_CATEGORY` map into `ml/seed/` so re-seed produces categorised data straight away (script becomes a one-shot for legacy).
- Add NFT / Video / Installation when a real artist asks. Additive migration.
- Multi-tag if the single-value + `mixed_media` bucket starts feeling restrictive.

### ~~`T-074` Unread-inquiry badge on TopNav~~ — shipped 2026-06-23

- ✅ `/v1/studio/me` extended with `unread_inquiry_count: i32`. Filters on `delivered_at IS NOT NULL AND read_at IS NULL` so pending-verification inquiries (which the artist hasn't been emailed about yet) don't pad the count.
- ✅ `<UnreadBadge>` component — 6 Vitest tests covering: render when > 0, hide on 0 / negative, "9+" cap at 10+, exact-9 still shows "9".
- ✅ `<StudioNavLink>` async server component does its own `auth()` check so a brief SSR-vs-CSR auth-state mismatch never fires `/v1/studio/me` for anonymous viewers. Graceful no-badge fallback on fetch errors via `reportError`.
- ✅ SSR-fresh refresh model — count updates on every page navigation. No polling, no WebSocket. Acceptable v1 trade-off; if artist sits on one page for an hour, badge waits until next nav.
- ✅ Foreground-bg badge, not red — site is monochrome-restrained and unread inquiries are good news, not alarm. Aria-label always carries the count.

**Deferred follow-ups:**
- Tab-title prefix (`(3) Wander — …`, Gmail style). Cheap, high-signal; layer in only if the badge alone proves insufficient.
- Inquiries-tile badge on the `/studio` dashboard. ~15 min addition once we want richer dashboard surfaces.

### ~~`T-075` Prod smoke suite (read-only)~~ — shipped 2026-06-23

- ✅ `scripts/smoke-prod.sh` — bash + curl assertions over the public surface (`/v1/health`, search, artist + artwork detail, neighborhoods, OG cards, images CDN). 17 checks, ~10s end-to-end.
- ✅ `make smoke-prod` target + help-line entry.
- ✅ Auto-runs at the tail of `make deploy-api` and `make deploy-web` so bad deploys fail loud at the deploy step. Escape hatch: `SKIP_SMOKE=1`.
- ✅ Fixtures use the WikiArt demo seed (artist `demo-ukiyo-e`, artwork `fbc3702b-…`) — stable, not subject to user-deletion testing (which had already invalidated the obvious `josh-matthews` fixture during T-071 testing).

**Deliberately out of scope:**
- Write-path bugs (POST /v1/uploads/image, PATCH semantics) — needs authenticated test users + post-test cleanup. Lives with `T-069` (E2E retention loop) once that takes shape.
- Cron / synthetic monitoring — overkill for pre-launch traffic. Revisit if we ever hit a "silently broken for hours" incident.
- Mail-delivery + job-worker round-trips — needs a closed-loop test fixture (synthetic inquiry → verify the email arrives at a known mailbox); large enough to be its own ticket.

### ~~`T-071` UI feedback + dialog primitives (FieldError, useConfirm, sonner toasts)~~ — shipped 2026-06-22

- ✅ `<FieldError>` + `useConfirm()` + `<ConfirmDialogProvider>` shared primitives.
- ✅ `sonner` Toaster mounted at the root with richColors + closeButton defaults.
- ✅ ESLint `no-restricted-globals` + `no-restricted-properties` ban `confirm`/`alert`/`prompt` (both bare and `window.*`).
- ✅ `.open-next/**` added to ESLint globalIgnores (was producing 21k errors).
- ✅ Vitest tests: 4 for `<FieldError>`, 5 for `useConfirm` + provider (confirm / cancel / Escape / destructive / outside-provider). Per-test cleanup wired via `vitest.setup.ts`.
- ✅ `docs/ui-patterns.md` documents the patterns; `decisions.md` 2026-06-22 captures the picks (sonner over alternatives, hook over props, AlertDialog vs Dialog, JS-only form validation) with rejected alternatives + reversibility ratings.
- ✅ ArtworkEditModal refactored: JS year validation parity, no HTML `min`/`max` attrs, `<FieldError>` everywhere, `useConfirm()` for publish-nudge + delete (destructive), `toast.success` on save/delete, edit closes on save, create lifts to detail.

**Deferred follow-ups (logged here for the next contributor):**
- Migrate existing success-feedback surfaces (inquiry sent, reply sent, follow/unfollow, save-to-collection, settings saved) to `toast.success` as they're next touched.
- Toast-error-on-failure pass for the `/u/[token]` unsubscribe + `/me/settings/notifications` pages.
- HTML form-validation attribute ban as an AST-level ESLint rule (currently convention + review). Brittle to write generally — only worth doing if we ever see a regression.

### ~~`T-070` Studio: artwork-dimensions input + filterable physical size~~ — shipped 2026-06-22

- ✅ `core::validation::dimensions_v1` — closed-schema validator (19 unit tests).
- ✅ `studio::artworks::{create,patch}` call the validator before binding; invalid shapes 400 with field-level error.
- ✅ Dimensions stay **optional at every status** — drafts AND published can have NULL dims. Soft confirm fires on draft→published when missing (non-blocking).
- ✅ `/v1/search?size=s|m|l` filter clause over `GREATEST(width, height)`; non-dimensioned works silently excluded; unknown bands tolerant. 5 new integration tests.
- ✅ Studio modal: 3-input row (width / height / depth-cm) with inline validation + mirrored client-side rule.
- ✅ `FilterBar` size pill on `/search` + `/neighborhoods/[slug]`.
- ✅ Decisions captured at `decisions.md` 2026-06-22 (cm-only, 3 bands, single band per query, longest-side determinant — with rejected alternatives).

**Deferred follow-ups (logged inline in `T-062`'s open-gap note):**
- Inches input toggle (cm-only ships v1).
- Multi-select bands (`size=s,m`) — bundle with the same shape change to `medium=` + `availability=`.
- Custom `min_cm..max_cm` range — when band granularity bites.
- Aspect-ratio filter (portrait / landscape / square) — independent feature.

### `T-069` E2E coverage for the retention loop
**Where:** `e2e/tests/` — new Playwright specs covering Follow, anon-follow-queueing, public collections, OG cards, notification preferences, unsubscribe.
**Why:** T-051 / T-052 / T-052b / T-052c / T-053 / T-068 all shipped end-to-end with strong integration-test coverage on the API + DB sides, but the cross-tier flows (anon click → sign-up → merge replays → follow exists; user toggles preference → cron skips them; click unsubscribe link → preference flips off) are only manually-verified today. The retention loop is the highest-value surface we have; lack of E2E regression coverage means a future refactor can silently break it.
**Acceptance:**
- `follow-flow.spec.ts` — signed-in user follows + unfollows an artist; follower count updates; `/studio` reflects.
- `anon-follow-queue.spec.ts` — fresh incognito context clicks Follow → bounces to sign-up → signs up → lands on artist page already following. Verifies the merge-anonymous bridge fired and the pending row was drained.
- `public-collection.spec.ts` — owner creates a collection → toggles public → captures share URL → second context (unauthenticated) opens URL + sees the collection. Owner toggles private → second context refresh → 404.
- `notification-prefs.spec.ts` — `/me/settings/notifications` round-trips toggles + a clean reload reflects the persisted state.
- `unsubscribe-token.spec.ts` — mint a token via API helper (test-side), GET `/u/<token>` → confirmation page renders + preference flips off (assert via API).
- OG cards: `og-card.spec.ts` — fetch `/artworks/<id>/opengraph-image` + assert 200 + `image/png` + non-zero bytes; same for `/artists/<slug>/opengraph-image` + `/c/<share_id>/opengraph-image`.
- All specs use the existing Clerk test-mode storage state (set up in T-084).
- CI: hook into the existing E2E workflow (`.github/workflows/e2e.yml`).
- **Acceptance gate for shipping any future change to FollowButton / merge_anonymous / unsubscribe routes / OG generators:** these specs must pass.

### `T-017` Search facet counts — deferred indefinitely
- Spec'd but the endpoint currently returns empty `FacetCounts`. Real implementation would mean per-search COUNT queries (expensive) or precomputed/approximated buckets. No user has asked for it. Reconsider when (a) we have enough corpus for buckets to be informative, AND (b) someone says "I wish I could see how many works of each medium there are."

### `T-018` Query embedding cache TTL job
- `query_cache.cleanup` scheduled job, daily
- `DELETE WHERE last_used_at < now() - interval '30 days'`

### `T-019` Voyage multimodal embedder — deferred indefinitely
- Second `Embedder` impl for A/B against Jina
- Trigger: only if Jina retrieval quality becomes a measurable bottleneck. No current signal it's the limiting factor. Don't pick this up speculatively.

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
