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

### `T-035` Vercel edge rate limit on write surfaces
**Where:** `web/middleware.ts` (extend) + Vercel KV
**Why:** the Next.js frontdoor sees public traffic before the API. Cheap to add a per-IP burst guard on `/search` and on the inquiry/save server-action paths so abuse can't tie up Vercel server functions or our DB connection pool.
**Acceptance:**
- Vercel KV (or in-edge `next-safe-action`-style counter) at ~30/min/IP for `/search?*` page hits
- Same for the inquiry server action and save server action
- Skipped when `process.env.RATE_LIMIT_DISABLED === 'true'` for local dev

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

### `T-050` Behavioural events writer
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

### `T-055` User taste vector + nightly refresh
**Where:** New `core::user_profile` module; `JobEvent::UserProfileRefresh` variant + handler; cron trigger (CloudWatch Events → SQS in prod / Postgres job-poller cron in dev).
**Why:** Foundation for `T-056` (personalised search re-rank), `T-060` (Discover Weekly), cluster-of-a-user, similar-artists. Schema (`user_profiles.taste_embedding` + HNSW) already exists.
**Acceptance:**
- Job: for each user with new events in the last 24h, fetch recent N events + associated artwork embeddings, compute weighted EWMA, L2-normalise, persist with `profile_updated_at = now()`.
- Initial weights — `inquiry: 5 / follow: 4 / save: 3 / visual_upload: 3 / modifier: 2 / dwell>15s: 1 / click: 0.5 / scroll-past: -0.1`. Decay 0.95 per week.
- Anonymous users get refreshed too (keyed by `anonymous_id`); merge on sign-in via `T-033`.
- `interaction_count` on `user_profiles` is incremented per refresh so downstream surfaces can gate on it (≥10 to enable personalised retrieval).
- Acceptance test: ≥10 saves of artworks near a centroid → resulting taste vector cosine-sim to that centroid > 0.6.

### `T-056` Personalised search re-rank + "For you" row
**Where:** `api-search::search.rs` adds a `taste_ranked` CTE; new `/v1/me/recommendations/for-you` endpoint; homepage row.
**Why:** First user-visible payoff of `T-055`. Personalised retrieval is the bridge from "good search" to "comes back daily."
**Acceptance:**
- `search.rs` extends the RRF fusion with a third channel: `taste_ranked` = nearest artworks to `user_profiles.taste_embedding` via HNSW. Blended alongside `semantic_ranked` + `text_ranked`. Skipped when `interaction_count < 10`.
- `GET /v1/me/recommendations/for-you` returns top-K by sim-to-taste with a small jitter (random rank ±5 on top 50) for serendipity.
- Homepage shows "For you" row when `interaction_count ≥ 10`; otherwise the current curated/featured row.
- Mode flag `?personalize=off` for debugging on both API + page.
- Per-request log line `personalise=on user=<id> alpha=…` for diagnosis.

### `T-057` Algorithmic neighbourhoods (HDBSCAN + Claude label)
**Where:** `ml/scripts/compute_neighborhoods.py` (new); persistence via a new ingestion endpoint or direct DB write; runs as a containerised Lambda or scheduled batch.
**Why:** Replace hand-curated set with discovered clusters per `decisions.md` 2026-06-17. Schema (`neighborhoods.kind='semantic'`, `cluster_centroid`, `representative_artwork_ids`, `computed_at`) is fully ready.
**Acceptance:**
- Pull all `artworks.status='published'` embeddings; HDBSCAN with `min_cluster_size` tuned around ~30. Output per cluster: centroid, member artwork ids, top-K nearest to centroid.
- Label step: sample 10 nearest per cluster; one Claude call per cluster with structured output `{name: 2-4 words, one_sentence_description}`. Cache by centroid hash so re-runs only re-label drifted clusters.
- Persistence: write rows with `kind='semantic'`. Match old centroids to new by greedy nearest-match for slug stability across re-runs.
- Hand-curated set (`kind='curated'`) coexists. UX call (filter? merge? curated-first then algorithmic?) deferred to implementation time.
- Cadence: weekly via cron-enqueued job. Skip if artwork population shifted < 5% since last run.

### `T-058` Series concept for artists
**Where:** New migration (`series` table + `artworks.series_id` FK); studio UI; artist-page "View by series" layout option.
**Why:** Behance-style "project format." Real artists work in series; the current flat-grid portfolio is hostile to how artists present their practice. Series also becomes a clusterable / recommendable entity.
**Acceptance:**
- Migration: `series (id, artist_id, slug, title, statement, cover_artwork_id?, created_at, updated_at)` + `artworks.series_id uuid REFERENCES series(id) ON DELETE SET NULL`.
- Studio: new "Series" tab with CRUD; drag-to-assign existing artworks; cover-image picker.
- Artist page: optional "View by series" toggle that groups artworks under series headers.
- API: `GET /v1/artists/:slug/series`, `GET /v1/series/:id`.
- Artwork detail surfaces "More from this series" when `series_id` is set.

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

### `T-061` First-session taste calibrator
**Where:** Web component on first homepage visit (anon-cookie flag); `POST /v1/me/calibrate` endpoint that seeds the anonymous taste vector.
**Why:** Makes `T-056` "For you" useful from session one. Solves the cold-start problem where new users see generic surfaces until they've generated enough events.
**Acceptance:**
- Dismissable inline panel on first visit: "Help us tune what to show you — 5 quick comparisons."
- 5 pairs sampled from far-apart cluster centroids (uses `T-057` output); user picks one per pair.
- Each chosen artwork's embedding (weight 2.0) seeds the anonymous taste vector. Merges into user vector on sign-in via `T-033`.
- A/B against a no-calibration cohort once events flow — measure 7-day return rate.

### `T-062` Filter UI: size / price / medium
**Where:** `web/src/components/FilterBar.tsx` extensions + URL params; API already accepts most of these via `api-search::search`.
**Why:** Schema + API are mostly there. "Something small under £500" is a real query we silently can't serve. Keep the visual restraint — this isn't a marketplace.
**Acceptance:**
- Price range slider (currency-aware via `lib/format.ts`).
- Dimension band: S / M / L preset + custom range in cm. Depends on `T-070` (artists need a way to enter physical dims first).
- Medium multi-select (uses existing taxonomy).
- All URL-driven via the established `useUrlState` pattern.
- API-side gaps closed where present (currently `medium=` is a single-value param).

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

### `T-017` Search facet counts
- Spec'd but currently returns nothing. Costs per-search COUNT queries; needs precomputation or approximation at scale.

### `T-018` Query embedding cache TTL job
- `query_cache.cleanup` scheduled job, daily
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
