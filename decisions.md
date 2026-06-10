# Decisions Log

Chronological log of significant architectural and product decisions.
Lightweight by design — heavier ADRs are overkill at this stage. If an entry
gets revised, link the original and add a follow-up entry rather than editing.

Format:

```
## YYYY-MM-DD — short title
**Context:** what was the situation
**Decided:** what we chose
**Alternatives:** what else we considered
**Why:** rationale
**Reversibility:** Low / Medium / High — how hard is this to undo later
```

---

## 2026-05-29 — Jobs queue: Postgres local, SQS + Lambda prod

**Context:** Several v1 surfaces need background work — geocoding `artist_locations` rows (currently `tokio::spawn`, fragile across api restarts), email delivery via Resend (T-032), image moderation via Rekognition (T-008), and the deferred LLM-assisted onboarding (T-012 Phase 2). The state-of-the-build review surfaced the worker-runtime question as the biggest unblocker for those.

The pragmatic options:
- **Inngest** — 50k step-runs/mo free, excellent step-function model, but no first-class Rust SDK. Handler code would have to live in TypeScript with calls back into the Rust API, doubling the deployable surface.
- **AWS SQS + Lambda** — fits the existing `04-stack-and-infra.md` AWS targeting. cargo-lambda support is mature. Free at v0 scale (1M Lambda invocations + 1M SQS messages/mo).
- **Cloudflare Queues** — JS/WASM-centric; awkward for Rust.
- **Postgres-backed jobs table** — self-contained, zero new infra, slow at high QPS but fine at v1 volume.

**Decided:** Same handler code, two drivers.
- **Local dev**: Postgres `jobs` table (migration `0012_jobs.sql`) + a sibling Rust binary (`api/crates/jobs-worker`) that polls with `FOR UPDATE SKIP LOCKED`. Zero external dependencies; runs in the same `make dev` loop as the api.
- **Prod**: SQS queue + cargo-lambda binary triggered on receive. Same `core::jobs::handle` dispatch function runs in both environments.

The abstraction lives in `core::jobs::JobsBackend` — an enum of `Postgres` (today) and `Sqs` (deferred until we deploy), matching the `ObjectStore` / `GeocodingClient` pattern. `JobEvent` is the tagged-enum wire format; the same JSON shape serializes into both a `jobs.payload` jsonb column and an SQS message body. Handlers (`core::geocoding::geocode_and_update` today; future `core::emails::*`, `core::moderation::*`) take a `JobsDeps` struct and return `Result<()>` — no driver knowledge in the handler.

**Alternatives considered + rejected:**
- **Inngest** — rejected: no Rust SDK means writing handlers in TS, doubling auth + config + deployment. The 50k-runs-free is generous but doesn't pay back the bilingual cost for our Rust-heavy backend. Revisit only if T-012 Phase 2 (LLM extract + scrape) lands as TS for other reasons.
- **Keep `tokio::spawn`** — rejected: works for geocoding (where re-saves are cheap) but won't extend to email (where a lost message is a missed inquiry) or moderation (where a lost message is unmoderated content reaching the public surface).
- **One backend now, port later** — rejected for the same reason we picked the enum: we know we'll need a different driver in prod, so the abstraction has to exist from day one. Otherwise every handler call site bakes in the local assumption.

**Why:** Pays back across every future background job. Each new job is one `JobEvent` variant + one handler fn + one match arm in `handle()` — local + prod both just work. The migration to SQS+Lambda when we deploy is a new ~50-line binary + an env flag, not a rewrite.

**Reversibility:** Medium — `core::jobs` is the central abstraction; rewriting it would touch every job-enqueuing site. But the on-the-wire format is plain JSON, so adopting a different orchestrator (Inngest, Trigger.dev, Hatchet) later is just a new driver impl — handler code stays put.

---

## 2026-05-29 — Map-search filter semantics: per-artist, not per-artwork

**Context:** `/search?q=ukiyo&map=1` could plausibly mean two things: (a) "venues whose artist has *any* artwork matching ukiyo" (per-artist), or (b) "venues with at least one artwork matching ukiyo on display right now" (per-artwork, stricter). The data layer can express either — the EXISTS subquery on `artworks` is a one-line change.

**Decided:** Per-artist match. A venue surfaces if the artist who lists it has any matching artwork in their portfolio.

**Alternatives:**
- **Per-artwork match** — stricter, more accurate to "find ukiyo prints near me." But artists list venues at the artist level (an `artist_locations` row says "you can see *me* at Foo Gallery"), not at the artwork level. We don't model "which artworks are at which venue" at all in v1 — that's the deferred shows / events Phase 2 work. Per-artwork match would imply a contract we can't currently honor.
- **Defer the decision** — option C from the original triage. Rejected because the question is binary and shipping required a choice; leaving it ambiguous would have meant inconsistent behavior across keyword vs medium vs artist filters.

**Why:** Matches the model of the data — venues are per-artist, so filtering venues should be per-artist. Lower friction: a viewer searching "ukiyo near me" gets every venue that has an artist working in that style, which is what they actually want for a Saturday gallery crawl. Stricter "this specific painting is at this gallery today" UX needs the post-v1 events model.

**Reversibility:** High — one SQL change in `api-search::search_map`, no schema impact. If real users ask "I went to the gallery and the ukiyo print wasn't there," we revisit.

---

## 2026-05-28 — Geography promoted from post-v1 to v1 (lean slice)

**Context:** During the feature-review pass, the user surfaced geography as a key personal motivator: "I don't find it easy to find local galleries or artists whose work I can go look at in person." The current shape is half a foundation — `artists.city/country/lat/lng` columns, `/v1/search?near_lat&near_lng`, a location filter on FilterBar — but no map UI, no live geocoding job, no street-level locations. `99-deferred.md` carves the full geographic story into three phases; Phase 1 (map view + geo neighborhoods) was deferred largely because it didn't have an internal champion yet.

City-only pins are useless on a map ("the artist is somewhere in Berlin"). Useful pins need a street address, which means a place a viewer can actually go — a gallery the artist is represented by, or an open studio. That's a different entity from the artist's "based in" city.

**Decided:** Promote a lean geography slice to v1 as `T-038`. Specifically:

1. Add `artist_locations` table — one row per place an artist's work can be seen. `kind` is `'gallery' | 'studio'` (shows deferred). Street-level address, geocoded to lat/lng.
2. Mapbox geocoding via an Inngest job. Stubs to no-op when `MAPBOX_TOKEN` is absent, matching the existing degrades-gracefully pattern.
3. Studio settings gets a "Where to see my work" CRUD section. Self-listed, trust-based, with a "Listed by the artist" label on the public pin (no admin verification in v1).
4. Artist profile gets a map widget showing the artist's `artist_locations` pins; falls back to a "based in {city}" pill if none.
5. `/search?map=1` toggles grid → map. Clustered pins are `artist_locations` rows. Bounds in URL so views are shareable.

Explicitly **not** in this slice:
- Shows / events as time-bound entities (still post-v1; needs the `events` table).
- `spaces` as first-class entities with their own pages. Two artists at the same gallery just have duplicated `artist_locations` rows; we eat the denormalization for v1 because the venue page is not the product yet.
- Admin moderation queue for galleries. The "Listed by the artist" label is the trust model; a 'Report listing' link can come later if abuse appears.
- Geographic neighborhoods (`neighborhoods.kind = 'geographic'`). Still post-v1 — editorial work, not a code path.

**Alternatives:**
- **Stay deferred, ship v1 without maps** — fastest. But the user has explicitly named this as a differentiator they care about; shipping v1 without it means relaunching the surface later.
- **Full Phase 2 (`spaces` + `events` tables, claim flows, admin moderation)** — the "right" model long-term, but several weeks of build and a moderation problem we're not ready to own. Deferred.
- **"Just-cities" pins** — what 99-deferred's Phase 1 had. Rejected: city-level pins don't drive in-person discovery, which is the whole point.
- **Google Maps embed (user's first instinct)** — Mapbox already has a token slot in env config and is in `04-stack-and-infra.md`'s cost model. Mapbox GL JS is open-source-licensed, supports vector tiles + clustering natively, and the free tier (50k monthly map loads) is generous for v0 traffic.

**Why:** The intermediate "artist_locations as a JSON column on artists" was tempting (no new table). Rejected because we need to query pins by bbox for the `/search?map=1` map mode, and a JSON-blob filter is harder to index than a (lat, lng) on a normalized row. The shape we're picking is forward-compatible with Phase 2 — when we eventually add `spaces`, we migrate `artist_locations` rows into `space_artists` join rows; nothing thrown away.

Cost impact: Mapbox geocoding is free up to 100k requests/month, well above any v0 traffic. Map loads via GL JS: 50k/month free. Both line up with existing `COST.md` guardrails.

**Reversibility:** Medium — `artist_locations` is one table + one Inngest job + two UI surfaces. If we decide to consolidate into `spaces` later, migration is mechanical (one row per `artist_locations.id` → `spaces` + `space_artists` join). The studio CRUD surface stays; only the underlying table changes.

---

## 2026-05-27 — Pre-commit hooks via lefthook

**Context:** Today's audit caught silent drift: `cargo fmt --check` fails (`artwork.rs`), `cargo clippy -- -D warnings` fails (`auth.rs`, `models.rs`), `eslint` fails (`SaveModal.tsx`'s `set-state-in-effect`). All four CI workflows enforce these, so either CI is currently red or we've been lucky on toolchain timing. Local development is on the honor system and the honor system has stopped working.

**Decided:** Adopt `lefthook` (https://github.com/evilmartians/lefthook) with a `lefthook.yml` at the repo root. Per-language path filters so a Rust change only triggers `cargo fmt --check` + `cargo clippy`, a web change only triggers `eslint` + `tsc --noEmit`, etc. Same lint set as CI, so anything passing pre-commit will pass CI.

**Alternatives:**
- **husky** — npm-installed, ties the hook system to `web/`'s package.json, weird for Rust-only PRs.
- **pre-commit** (Python framework) — slow startup (~1s+ per run), requires Python; we already touch Rust/TS/Python so adding another framework is friction.
- **Raw `.githooks/`** — no install/version story, hard to share, gets out of sync.
- **CI-only enforcement (status quo)** — push → red → fix → push cycle; expensive for trivial drift. Doesn't help local dev.

**Why:** Single Go binary, no per-language runtime, ~50ms startup, clean glob-based config, language-agnostic. Same maintainers (Evil Martians) keep up with Rust/TS toolchain quirks. Critically: it can also enforce our TODO comment convention via a regex check (see next entry).

**Reversibility:** High — `lefthook.yml` and `lefthook install` are the only artifacts; removing means deleting the file.

---

## 2026-05-27 — TODO comment format: `TODO(T-NNN): description`

**Context:** Grep across the Rust code shows 5 inline `TODO`s. Three of them — including `inquiries.rs:16` (`(TODO T-XXX)`) and `inquiries.rs:191` (`TODO: enqueue Inngest…` no ticket) — have no resolvable ticket reference. They're notes that will rot. `search.rs:109` cites `T-018` but that ticket is about something else, so the link is broken.

**Decided:** Every inline `TODO` in source code must reference a ticket from `TODO.md` in the form `TODO(T-NNN): short description`. `FIXME` and bare `TODO:` are not allowed. Enforced by a regex check in `lefthook.yml` (pre-commit) — the hook scans the staged diff and rejects commits introducing a bare TODO. CI runs the same check as a backstop.

**Alternatives:**
- **Honor system** — what we have now. Doesn't work.
- **Custom clippy lint** — overkill for what's basically a grep; would need a clippy plugin or a separate lint crate.
- **Allow free-form TODOs, archive in CHANGELOG when removed** — loses the "this code knows about an open ticket" signal that helps reviewers cross-check.

**Why:** A ticket-prefixed TODO is greppable (`grep -r 'TODO(T-007)'` lights up every site that depends on it), traceable (the ticket has the why), and removable (when the ticket lands, you `grep` and delete the stragglers). Bare TODOs accumulate forever; the X-X-X placeholders we currently have are proof.

**Reversibility:** High — undo the regex rule, the existing TODOs stay valid.

---

## 2026-05-27 — `User` as an axum `FromRequestParts` extractor

**Context:** Today, 9 handler call sites do `let user = auth::authenticate(&headers, &state.jwt_verifier, &state.pool).await?;` literally. The original `core::auth` module note ("orphan rules for foreign-trait extractors against cross-crate state aren't worth the abstraction cost at this stage") was correct at 1 site, debatable at 4, wrong at 9. T-011 (studio) will roughly double the count.

**Decided:** Add `impl FromRequestParts<Arc<AppState>> for User` in `api-search` (the binary crate that owns `AppState`), delegating to `core::auth::authenticate`. Handlers go from `headers: HeaderMap` + an explicit auth call → `User(user): User` in the signature. The unit-tested function stays in `core`; the extractor is a thin adapter that lives where the orphan rules allow.

**Alternatives:**
- **Keep inline calls** — verbosity scales linearly with the handler count, and forgetting the call is a silent auth bypass.
- **Generic `FromRequestParts<S> where S: HasAuthContext`** in `core` — more flexible (any AppState can implement the trait), but adds an abstraction that doesn't pay rent until we have a second binary. Worth doing when `api-uploads` lands.
- **`axum::middleware::from_fn` that injects a `User` into request extensions** — works but tests have to set extensions manually; the `FromRequestParts` route uses the same `authenticate` function for both runtime and tests.

**Why:** Orphan-rule-friendly placement (extractor lives with `AppState`, function lives with the logic). One concrete impl rather than a trait we don't need yet. Removes ~9 lines of boilerplate now, double that after studio. Auth failures become structurally impossible to forget at the handler level.

**Reversibility:** Medium — once handlers depend on the extractor signature, undoing means touching all of them. But the contract surface (`User { id, clerk_user_id, email, is_admin }`) doesn't change.

---

## 2026-05-27 — Error reporter shim (web) — one function today, Sentry later

**Context:** 9 web call sites use `console.error("...failed", e)`. In Vercel prod, these go to function logs nobody monitors. Observability is on the pre-launch checklist but Sentry-or-equivalent isn't wired and we don't have deploy infra yet anyway.

**Decided:** Introduce `web/src/lib/reportError.ts` exporting `reportError(err: unknown, context?: Record<string, unknown>): void`. Today it wraps `console.error` with a structured prefix (`[err]`) and the context object. When Sentry (or Axiom, or whatever) gets wired, only this file changes. Migrate the 9 existing call sites in the same pass. Going forward, `console.error` is reserved for genuinely-not-an-error logs (debug prints) and is grep-rejectable in code review.

**Alternatives:**
- **Wire Sentry now** — premature; no deploy infra, no traffic, no signal on what to capture.
- **Keep `console.error`, migrate later** — every call site changes twice (once when we standardize prefix/context shape, once when we add Sentry).
- **Class-based logger with levels** — over-engineered. We only have one level that matters (errors) until we have real users.

**Why:** Cheapest possible seam. Zero behavior change today, one-file change when the real reporter lands. The 9 call-site touches happen once, in this pass, while we're already touching the web tree.

**Reversibility:** High — it's a function. Delete the file and swap back to `console.error` if we change our minds.

---

## 2026-05-27 — Specs (`01..03-*.md`) are aspirational, CHANGELOG + decisions are truth

**Context:** `01-page-spec.md`, `02-component-library.md`, `03-api-data-spec.md` were written as the v1 product spec before the build started. Since then we've shipped rate-limit middleware, `contains_artwork`, neighborhood filters, FilterBar, SaveModal a11y, etc. — none of which is reflected back into the specs. Choice: (a) update the specs on every PR (tax, churn, mostly unread), (b) let them drift unbounded (currently happening), (c) reframe them.

**Decided:** Reframe. The specs describe the *intended v1 product* — useful as a holistic reference. `CHANGELOG.md` + `decisions.md` are the source of truth for *what was built and why*. Update specs only when (i) something in the spec materially contradicts shipped behavior or (ii) we're starting a new major surface and need the spec to scope the work. Otherwise, decisions log the deviation and CHANGELOG logs the build. Add a header line to each spec doc making this explicit.

**Alternatives:**
- **Update specs on every PR** — high overhead, low readership, real chance the spec lags anyway.
- **Delete the specs, let CHANGELOG carry everything** — loses the holistic "v1 product brief" that's still useful when scoping new pieces.
- **OpenAPI-generated API spec** — would solve 03-api-data-spec drift, but requires Rust handler annotation plumbing we don't have. Worth revisiting near launch.

**Why:** Matches how the docs have actually been used — read once at scoping time, rarely thereafter. Avoids spec-maintenance churn that nobody benefits from. Keeps the long-form v1 brief intact and useful.

**Reversibility:** High — the docs exist; switching to per-PR maintenance is a process change, not a code change.

---

## 2026-05-27 — Rate limiting lives at the API, not the edge (for now)

**Context:** Standing up rate limiting (`T-007`). Two reasonable places to put it: edge (AWS WAF in front of Lambda, or Vercel middleware), or right at the API.

**Decided:** Implement at the API layer first, in-process via `governor` (GCRA / leaky bucket), keyed per-user → per-anon → per-IP. Limit middleware lives in `core::middleware::rate_limit`. Edge rate limiting is tracked separately as `T-034` (AWS WAF) and `T-035` (Vercel middleware), gated on actual deploy infra.

**Alternatives:**
- AWS WAF rate-based rule in front of the Lambda Function URL. Coarser (per-IP, 5-min window minimum), and we don't have any infra yet so the Terraform would be untested.
- Vercel edge middleware with Vercel KV counters. Doable now since Next.js middleware runs in dev — but only protects traffic that goes through Next.js, and isn't where the actual cost is.
- Tower's built-in `RateLimitLayer`. Global per-process, no per-key state — useless for blocking one abusive caller without blocking everyone.
- Upstash (managed Redis) for distributed limiting. Right answer when we have more than one API process; premature today.

**Why:** The expensive surface is the Jina embedding call behind `/v1/search` and (later) Anthropic / Rekognition behind upload and onboarding jobs — not Lambda invocations themselves. To save $1 in Lambda we'd need to block ~2.5M requests; to save $1 in Jina spend we only need to block ~10k novel queries. Putting the rate limit right next to the paid call is what caps spend. Edge layers add defense-in-depth and they're worth doing — but they go with the deploy milestone, not before there's an edge to put them on.

**Reversibility:** High — the API-layer limiter is one module + a Config flag. Swapping the in-process `governor` for an Upstash-backed implementation is a single trait swap; the middleware contract doesn't change. Adding WAF / Vercel layers later is purely additive.

## 2026-05-24 — Defer the pre-built-portfolio claim flow

**Context:** Original spec had a cold-outreach mechanic where we'd scrape a target artist's website, build a private preview portfolio, and email them a tokenized link to claim or take down.

**Decided:** Remove from v1. Direct manual outreach to 20–30 artists for v0/v1 instead.

**Alternatives:** Build it private-by-default behind a token-gated URL.

**Why:** Even private, republishing scraped work without explicit consent has real legal and reputational risk. Direct outreach is slower but unambiguous. Schema fields for the claim flow are documented in `99-deferred.md` for when we revive it.

**Reversibility:** High — schema is documented, just not migrated.

---

## 2026-05-24 — All-AWS infra over Vercel

**Context:** Two viable hosting strategies — Vercel for frontend + Vercel functions for API, vs all-AWS via OpenNext + Lambda + Terraform.

**Decided:** All-AWS, fully Terraformed. OpenNext for Next.js, Rust Lambdas behind API Gateway, Neon Postgres, S3 + CloudFront, Inngest, Clerk.

**Alternatives:** Vercel + Next.js route handlers (~1 day faster to ship, more vendor lock-in).

**Why:** Single cloud, single IaC story, no cross-cloud secret management. Cost scales more predictably. Marginal extra setup; recovers itself in less iteration friction.

**Reversibility:** Medium — moving to Vercel later means rewriting `infra/` and adjusting OpenNext-specific edges.

---

## 2026-05-24 — Rust Lambdas for the API

**Context:** We could write the API in TypeScript (Next.js route handlers) or Rust (Lambda).

**Decided:** Rust Lambdas, structured as a Cargo workspace, deployed via Terraform.

**Alternatives:** Next.js route handlers (faster iteration, single language with the frontend).

**Why:** User preference + Rust's cold-start performance and type-safe SQL via sqlx. Accepted tradeoff: slower iteration, more boundary work for shared TS types.

**Reversibility:** Low — undoing this means rewriting the entire API.

---

## 2026-05-24 — Local embedder for spikes, HTTP API in production

**Context:** Multimodal embedding can run locally via PyTorch on MPS, or via Jina's HTTP API.

**Decided:** Both, behind the same `Embedder` Protocol. `LocalJinaClipEmbedder` for spikes / batch eval (free, no rate limits). `JinaEmbedder` HTTP client for production request-time embedding.

**Alternatives:** Only HTTP (simpler, pays per spike); only local (impractical in Lambda).

**Why:** Spikes do many embedding calls; HTTP would be slow and expensive. Production runtime can't load a 2GB model into Lambda.

**Reversibility:** High — both implementations exist behind the same Protocol.

---

## 2026-05-25 — Ship modifier delta vectors at α=0.8

**Context:** Visual-search modifier buttons ("moodier", "warmer", etc.). Two competing implementations: precomputed delta vectors added to query embedding, or text-fusion RRF.

**Decided:** Delta vectors at α=0.8 as the production path. Text-fusion retained as a fallback.

**Alternatives:** Text-fusion only (simpler, less to maintain).

**Why:** Round-2 spike on WikiArt (2000 images) showed clean modifier shifts at α=0.8 across all five modifiers, with results staying visually related to the source. Delta is also faster at runtime (one vector add vs two retrieval queries + RRF). See `ml/spikes/2026-05-modifier-deltas/FINDINGS.md`.

**Reversibility:** High — `Embedder` protocol abstracts both approaches.

---

## 2026-05-25 — Synthetic-artist demo seeding

**Context:** Need realistic local-dev data without exposing the platform to copyright issues by using real living artists' work.

**Decided:** Seed from WikiArt (2000 images, 27 styles); create one synthetic artist per style (e.g. "Impressionism Studio (Demo)"); flag every demo row with `is_demo = true`. Production deploys filter `is_demo = false`.

**Alternatives:** Use real artist names from WikiArt (impersonation risk), generate synthetic art (defeats the testing purpose), wait for real artists (blocks engineering).

**Why:** Clear separation between demo content and real artist content. `is_demo` is a single boolean filter at every query boundary.

**Reversibility:** High — a single `DELETE WHERE is_demo = true` wipes all demo content.

---

## 2026-05-25 — Geographic minimal in v1, full Spaces+Events in v2+

**Context:** Original spec had artist `location` as free-text only. The art world is structurally local — galleries, openings, fairs — and that's missing.

**Decided:**
- v1: structured `city`, `country`, `lat`, `lng` on `artists`. `location` + `near_me` filters on `/v1/search`. Mapbox geocoding via Inngest job.
- v2 (deferred): map view, geographic neighborhoods.
- v3 (deferred): "spaces" + "events" as first-class entities. Note the naming — "spaces" not "galleries", to include artist-run / project / fair / pop-up venues native to the indie ecosystem.

**Alternatives:** Defer all geographic to v2 (loses a real product axis).

**Why:** Minimal geographic is half a day of extra work and gives 80% of the "find Berlin artists" value. The full Spaces+Events build is weeks; correct to plan but premature to start.

**Reversibility:** High — Phase 2/3 schemas in `99-deferred.md` are additive.

---

## 2026-05-25 — Cargo workspace: one binary per route group (option B)

**Context:** Three options for the Rust API structure — one Lambda for the whole API, one per route group (~8 binaries), or one per handler.

**Decided:** One binary per route group. Initial groups: `api-search`, `api-me`, `api-collections`, `api-uploads`, `api-inquiries`, `api-studio`, `api-onboarding`, `api-events`.

**Alternatives:** One Lambda for everything (simpler, faster cold start because warm pool covers all routes).

**Why:** Different route groups have different memory/compute profiles (search is embedding-heavy, uploads handle file streams, studio is mostly DB-heavy reads). Independent scaling and deploy granularity helps later. Accepted tradeoff: ~8 deploy targets, slightly more cold-start surface area, more boilerplate.

**Reversibility:** Medium — merging Lambdas later is mechanical; splitting is harder.

---

## 2026-06-07 — Search + map are one surface, viewed two ways

**Context:** `/search` shipped with a `Works` / `Where to see them` toggle — two tabs over the same logical query (artworks + their artist locations). UX kept forcing users to choose between "what does it look like" and "where can I see it," and the two endpoints (grid + map) had drifted in subtle ways (different filter semantics, different result sets) that we patched piecemeal (artist_ids thread-through, q-filtered city pivots, disconnect-explainer banner).

**Decided:** the toggle stays as the affordance, but `?map=1` becomes a **split view** — the grid moves to a scrollable side panel (~40% width on desktop, stacked on mobile) and the map fills the rest. Hover/click syncs in both directions: card-hover emphasises pins, pin-hover scrolls the panel; click on either opens detail in the other. State of truth is a single `highlightedArtistId` lifted to the SearchPage; neither half mutates from a hover that originated in itself.

**Alternatives:**
1. Keep the tabs. Loses the relationship between an artwork and its venue — the disconnect-explainer hack proved we'd be papering over the gap forever.
2. Map-as-default with a tab to grid. Too aggressive — users browsing without a geographic intent want the simpler grid.
3. Full Airbnb (map dominant, list as overlay sheet). Right for travel sites where the map IS the product; wrong here because the artwork visual carries primary value.

**Why:** the split view models the relationship as it actually is — every artwork has an artist, every artist may have a location, the user wants both lenses simultaneously when they're searching geographically. The toggle preserves the lighter "just show me artworks" path for users who don't care where to physically encounter the work.

**Reversibility:** Medium. The split layout is a swap in `/search/page.tsx`'s render path; we keep the grid component and the map component separately usable, so falling back to the tab model is a layout change, not a data-model change.

**Implementation:** four slices (L1–L4) in `TODO.md` `T-045`. L1 (layout shell, no sync) is the smallest releasable unit and the right thing to ship first.

---

## 2026-06-09 — Search resume state belongs in the URL, not sessionStorage

**Context:** Users wanted "leave the search page → come back to the same view" — same page of results loaded, same artwork selected, same map viewport, same scroll. First pass used `sessionStorage` keyed by URL with snapshot/restore on mount. It worked, but the failure modes piled up: silent hydration races, dev hot-reload wiping state, the most common case (no Load More yet) wasn't even covered, and the state was invisible to anyone who didn't write the code.

**Decided:** the URL is the single source of truth for search resume state.
- `?pages=N` (cumulative cursor pagination on the server)
- `?focus=<artwork_id>` (selected artwork; set via replaceState on click, restored on mount)
- `?bbox=…` (already lived in URL)
- Filters already in URL

The `<BackToSearchLink>` component uses `router.back()` when the referrer is our `/search` so the full browser-history entry is reused (including scroll), and falls back to `router.push('/search')` otherwise.

**Alternatives:**
1. **`sessionStorage` snapshot + restore on mount.** What we tried. Brittle: hydration race, dev-mode loss, lost-on-first-page-of-session, un-shareable.
2. **In-memory route cache (Linear pattern).** Better than sessionStorage but still hides state in JS land — bookmark, refresh, share-link all break.
3. **bfcache reliance.** Browser's back/forward cache is great when it works; doesn't apply for refresh + share-link + bookmark, and is fragile (caching is disabled by many third-party scripts).

**Why:** the URL is the only address that lives outside the user's tab — making it the source of truth means refresh, share-link, bookmark, and back-nav all produce the same view by construction, not by careful state plumbing. The cost is N sequential `/v1/search` roundtrips per render for `pages=N`, which is acceptable at v1 scale (capped at 10 pages = ~1.5s p95). Future optimisation: parallelise the chase (cursor is internally an offset; we could compute offsets ourselves) — but the API contract stays opaque, so the option is available without an API change.

**Reversibility:** High. The URL-driven approach is additive — if we ever want to re-layer in-memory caching for snappier load-more, the URL stays as the canonical truth and the cache is just a paint optimisation. Reverting would mean nothing more than ignoring the URL params.

## 2026-05-25 — Postgres-backed text query embedding cache

**Context:** Search endpoints need to embed the user's text query at request time. Jina API takes 100–300ms per call. This dominates search latency.

**Decided:** A `query_embedding_cache` table in Postgres: `(query_text PK, embedding vector, model_name, model_version, last_used_at, hit_count)`. Lookup before calling Jina; insert on miss. TTL 30 days via scheduled cleanup job.

**Alternatives:** Redis (ElastiCache too expensive at v1; Upstash adds another service and rate limits), in-process LRU per Lambda (lost on cold start), no caching (slow + expensive).

**Why:** Zero extra infrastructure, free, fast (one Postgres query), survives Lambda restarts. Common queries amortize to a single embedding API call ever.

**Reversibility:** High — swap to Redis later if needed without changing the cache interface.

---

## 2026-05-25 — Dev-only `/dev/login-as/:slug` route for testing the studio surface

**Context:** Seeded demo artists have no Clerk users. To exercise the artist-studio flows locally we need a way to act as an artist without going through real auth.

**Decided:** A dev-only endpoint `GET /dev/login-as/:artist_slug` that mints a development JWT for the matching seeded artist. Gated by `ML_ART_ENV=dev` — refuses to register the route in staging or prod.

**Alternatives:** Create Clerk users for demo artists during seeding (pollutes the Clerk dev instance, gets confusing).

**Why:** Cleanest separation between auth (real Clerk users) and demo data (seeded artists). One env-flag check at startup, impossible to ship to prod.

**Reversibility:** High — delete the file.

---

## 2026-05-25 — Monorepo with per-directory CI

**Context:** We have `ml/`, `db/`, soon `api/`, `web/`, `infra/`. Single repo or split?

**Decided:** Monorepo. CI runs path-filtered workflows per directory.

**Alternatives:** One repo per service.

**Why:** Cross-directory edits are common in early product (schema change touches `db/`, `api/`, `ml/seed.py`, sometimes `web/`). Single repo means one PR.

**Reversibility:** Medium — splitting a monorepo later is mechanical but loses git history.

---

## 2026-05-26 — Clerk testing helper for E2E (real auth, not a web bypass)

**Context:** Playwright needs to cover signed-in flows (Save modal, Inquire when signed-in, future studio surfaces). The originally-tracked `T-031` proposed a web-side test-mode bypass mirroring the Rust `JwtVerifier::for_tests()` pattern — a cookie set by a dev-only route, read by `apiFetch`, forwarded as `Bearer test-<sub>`.

**Decided:** Use Clerk's official `@clerk/testing` package + their test-email convention. No custom bypass code in the web app at all.

How it works:
- Clerk's dev instance auto-accepts the OTP `424242` for any email matching `*+clerk_test@*` (a documented Clerk feature)
- `@clerk/testing/playwright` exports `clerkSetup()` (per-worker) and `setupClerkTestingToken({ page })` (per-test) which bypass Clerk's Smart CAPTCHA / bot fingerprinting so headless browsers can submit forms
- A Playwright `setup` project signs up a fresh user once per run and saves browser state to `e2e/.auth/user.json`
- A `chromium-authed` project picks that state up via `storageState`; tests in `*signed-in*.spec.ts` run under it

**Why this over a custom bypass:**
- No production code paths exist that bypass auth — the *only* thing different in tests is Clerk's bot-protection token. The auth model is real-Clerk-from-the-browser's-perspective.
- Less surface area to get wrong. A bypass cookie that's gated only by env var is one config-mistake away from prod-leaking; this has no equivalent failure mode.
- Real JWTs verify against real JWKS in our Rust API, exercising the actual production verification code path.

**Cost:** each Playwright run creates a real Clerk user in the dev instance + a row in our `users` table. Mild accumulation. A cleanup script (cron-driven, deleting `*+clerk_test@*` users older than a week) is a future-day chore.

**Reversibility:** High — uses an external library + standard Playwright patterns. If Clerk changes the testing helper API, we adapt.

---

## 2026-05-26 — Test-mode JwtVerifier (explicit constructor, not env-gated)

**Context:** Integration tests for authed endpoints (`/v1/me`, `/v1/me/collections`, signed-in `/v1/artworks/:id/inquiries`) need a way to authenticate without minting real Clerk JWTs. Three options were on the table:

1. **Env-flag bypass** in `authenticate()`: e.g. when `AUTH_DISABLED=true`, trust `X-Test-User-Id` header. Rejected — a misconfigured prod deploy could become "anyone can be anyone".
2. **Mint real Clerk JWTs in tests** via Clerk's backend API. Rejected — tests would need network access to Clerk, real secret key in CI, and we'd be testing Clerk's signing as much as our code.
3. **Explicit test constructor on `JwtVerifier`.** Picked.

**Decided:** `JwtVerifier::for_tests()` returns a verifier with a `test_mode: true` flag. In `verify()`, when that flag is set, accept any token starting with `test-` and return a synthetic `ClerkClaims { sub: token[5..] }`. Tests seed users with known `clerk_user_id` values (e.g. `user_test_alice`) and send `Bearer test-user_test_alice`. The `upsert_user` path hits the existing SELECT branch — no Clerk API call.

**Why explicit-constructor over env-flag:** the bypass requires a *code change* to reach (calling `for_tests()` instead of `new()`). Production code paths in `main.rs` call `new()`. There's no way for an environment variable or config file to flip a prod deploy into bypass mode.

**Limit:** doesn't cover the web side. Playwright can't drive Clerk's hosted sign-in. Signed-in browser flows are not covered by E2E yet. See `T-031` in `TODO.md`.

**Reversibility:** High — switching to real Clerk JWT minting in tests is purely additive; the test-mode path can stay.

---

## 2026-05-26 — Cross-user resource access returns 404, not 403

**Context:** Collections endpoints enforce `WHERE user_id = $auth_user_id` in SQL. When Bob tries to read Alice's collection, we have a choice of two error statuses: 403 (you're authenticated but not allowed) or 404 (no such collection).

**Decided:** 404 for everything cross-user. Same response shape as a missing resource.

**Why:** 403 leaks existence — Bob can infer that Alice has a collection with that UUID, which is information he shouldn't have. 404 is honest from Bob's perspective (the collection doesn't exist *for him*) and consistent with how we'd treat any unknown ID.

**Cost:** marginally worse error messages for the legitimate case where Alice mistypes her own collection ID — she also sees 404 instead of "this exists but you can't see it." Acceptable.

**Reversibility:** High — flipping back is a one-line change per handler.

---

## 2026-05-26 — Anonymous identity: cookie at Next, header to API

**Context:** The API spec calls for a signed first-party `anon_id` cookie. With Next.js on `:3000` and the Rust API on `:9100` in local dev (different origins), cookies don't traverse cleanly without CORS + credentials. In production the routing will likely consolidate (CloudFront fronts both `/` and `/v1/*`), so cookies *will* work natively — but only there.

**Decided:**
- Next.js owns identity: middleware sets a signed `anon_id` cookie (HMAC-SHA256 over UUID v7 with `ANON_COOKIE_SECRET`), HTTP-only, `SameSite=Lax`, 1-year expiry
- Server components verify the signature, then forward the *unsigned* UUID to the Rust API as `X-Anonymous-Id` header
- The Rust API treats `X-Anonymous-Id` as trusted because the only thing that should be reaching the API is Next.js (server-to-server). In production this is enforced by CloudFront / API Gateway routing — the API isn't directly reachable from the browser
- Missing header is fine; many endpoints (search, artist, artwork, neighborhoods) work without identity. Endpoints that need it (rate-limited writes, behavior tracking) require it explicitly via an extractor

**Alternatives considered:**
- Browser-direct cookie to API: needs CORS with credentials, separate `Domain` config, more complex
- Client-supplied unsigned header from anywhere: trivially spoofable; rejected
- JWT for anon identity: overkill — we just want a stable opaque id

**Why:** Solves identity in local dev without CORS headaches; matches the production routing model; lets us add real signature verification on the Rust side later if needed (the cookie is signed by *something*, we just choose to trust the Next.js boundary).

**Reversibility:** Medium — switching to browser-direct cookies later is a CORS config + a one-line change in the Rust extractor.

---

## 2026-05-26 — Tiered test posture: integration > E2E > unit, no components

**Context:** No tests exist beyond `ml/tests/test_vectors.py`. We have a working full stack but no automated way to know if a commit breaks things. We need a test posture matched to a solo-dev side project — leaner than enterprise paranoia, but real enough to gate merges.

**Decided:** Four tiers, built in priority order.
1. **Rust API integration tests** via `#[sqlx::test]` against per-test ephemeral Postgres — biggest signal-per-hour
2. **Playwright E2E golden-path suite** in top-level `e2e/` — ~8 flows covering every navigable surface
3. **Vitest units** for pure functions (`formatPrice`, `toQueryString`, etc.) — cheap correctness gate
4. **CI** via GitHub Actions, per-directory gated on `paths:` filters

**Stubbing:** all paid APIs (Jina, Mapbox, Anthropic, Rekognition, Clerk) have deterministic stubs. Same code paths as graceful-degradation dev mode, which `COST.md` already documents.

**Alternatives considered:**
- React Testing Library component tests — rejected: web is mostly JSON-rendering; E2E covers visible behavior, components without explicit tests are easier to refactor.
- Cypress instead of Playwright — rejected: Playwright is less flaky and TS-native.
- 100% coverage target — rejected: incentivises trivial tests.
- Visual regression — deferred: too much churn at v0.

**Why:** Integration tests catch the contract failures that hurt most (wrong JSON, wrong SQL). E2E catches user-visible failures the unit/integration layers can't see. Skipping component tests is the contrarian call — we keep them out *deliberately* because the cost of maintaining them outweighs their value when the components are mostly thin wrappers around fetched JSON.

**Reversibility:** High. Each tier is independent. Adding component tests later is purely additive.

**Full strategy:** see `TESTING.md`.

---

## 2026-05-25 — Artwork detail: full-page first, modal-overlay deferred

**Context:** Original spec calls for `/artworks/[id]` to open as a modal overlay on top of the previous page, with the URL updating; direct-load shows a full page. Next.js supports this via parallel + intercepting routes (`@modal/(.)artworks/[id]`).

**Decided:** Ship `/artworks/[id]` as a regular full page for v0. Defer the modal-overlay pattern to v1.1 (or later).

**Alternatives:** Build the modal-overlay pattern now.

**Why:** Parallel + intercepting routes are powerful but buggy in non-trivial cases (back button, share, SSR/CSR transitions, scroll restoration). Full-page first means: works on first try, easy to test, easy to crawl. The modal layer is a UX polish item, not a feature. Add when the rest of v1 is solid.

**Reversibility:** High — the page already lives at `/artworks/[id]`. Adding the modal is purely additive (new parallel-route slots beside the existing page).

---

## 2026-05-25 — Local-dev port remappings

**Context:** Default Postgres (5432) and Mailhog SMTP (1025) ports collided with existing local services on dev machines.

**Decided:** Map Postgres to `5433`, Mailhog SMTP to `2025`. MinIO (9000/9001) and Mailhog UI (8025) stay default.

**Alternatives:** Use the defaults and assume no conflicts.

**Why:** Conflicts are common on developer machines (local Postgres install, AirPlay on 1025). Non-standard ports documented in `docker-compose.dev.yml` and `decisions.md`.

**Reversibility:** High — change the ports back if the user prefers.

---

## 2026-06-10 — Cloudflare for DNS (forced by Cloudflare Registrar)

**Context:** Domain `wander.gallery` registered with Cloudflare Registrar. Initial TF design used Route53 for DNS (one zone + 3 ACM cert validations + 6 alias A/AAAA records to CloudFront). After bootstrap, discovered Cloudflare Registrar *mandates* Cloudflare nameservers — you cannot point NS at Route53. Transfers are locked for 60 days post-registration (ICANN rule), so we can't relocate the registrar tonight.

**Decided:** Move all DNS records into Cloudflare via the `cloudflare/cloudflare` TF provider. ACM certs stay in AWS (us-east-1) — only the DNS records change provider. Use Cloudflare CNAME-flattening at the apex (CloudFront's `domain_name` as a CNAME, even at `wander.gallery`). All records `proxied = false` so traffic goes direct to CloudFront, not through Cloudflare's CDN (no double-cache, no double-bill, no WAF confusion).

**Alternatives:**
- Transfer to Namecheap / Route53 after the 60-day lock — viable later, not tonight.
- Buy a new domain at AWS Route53 — wasteful.
- Use Cloudflare DNS by hand without TF provider — quick but creates drift.

**Why:** Cloudflare DNS is free, fast, and the TF provider is mature. CNAME-flattening at the apex is the one thing Route53 doesn't do natively (it has alias records, which serve the same purpose). For our shape — three subdomains pointing at CloudFront — Cloudflare DNS is a clean fit.

**Reversibility:** Medium — if we transfer the registrar to AWS later, we'd switch to Route53 (or just leave DNS at Cloudflare and only move the registrar).

**Operational note:** The `CLOUDFLARE_API_TOKEN` env var is required for `terraform plan/apply`. Token needs `Zone:Read` + `DNS:Edit` on the single zone only. Out-of-band rotation hygiene applies.

---

## 2026-06-10 — API Gateway HTTP API over Lambda Function URL

**Context:** Initial deploy used Lambda Function URLs as CloudFront origins — the lighter / cheaper alternative to API Gateway, recommended by AWS for our exact shape. After apply, **every request 403'd** with `Forbidden. For troubleshooting Function URL authorization issues...`, regardless of:
- `auth_type = NONE` + explicit `Principal: *` resource policy ✗
- `auth_type = AWS_IAM` + CloudFront Origin Access Control (OAC) with SigV4 signing ✗
- Custom origin-request policy excluding the `Authorization` header (the AWS-documented OAC collision workaround) ✗

CloudWatch logs confirmed the requests were rejected at the Function URL gateway, *before* reaching the Lambda. Direct `aws lambda invoke` worked perfectly — the function itself was healthy. The account joined the org on 2026-06-09 (one day before this debug session); the most plausible explanation is an **undocumented new-account anti-abuse restriction** that blocks public Function URLs in the account's first few days. No visible SCP / RCP / account-level setting documents this.

**Decided:** Pivot to AWS API Gateway HTTP API (v2) in front of each Lambda. CloudFront → APIG → Lambda. WAF stays attached to CloudFront. APIG was the topology originally expected; the Function URL detour was driven by the "smaller infra, no APIG bill" argument that turned out to be moot for a new account.

**Alternatives:**
- Open AWS support ticket to lift the Function URL block — possible, but days of latency, no guarantee.
- Wait a few days and retry Function URLs — possible but unverified.
- Keep debugging — already 30+ min in with no signal, low expected value.

**Why:** APIG HTTP API is well-trodden, observable, and doesn't have the new-account restriction. Cost is negligible at v1 (`$1.00/M requests`, free tier covers idle). Topology is what we'd build anyway if optimizing for "least surprise per dependency."

**Trade-offs accepted:**
- One extra hop (CloudFront → APIG → Lambda) — adds ~10-30ms p50.
- ~$0–1/mo at v1 traffic vs Function URL's $0.
- 30s hard cap on responses (vs Function URL's 15min) — fine for our workload (SSR p99 ~1s; search ~1s).

**Reversibility:** High — if AWS removes the new-account restriction we can swap APIG back out for Function URL + OAC with the same TF shape as before (the OAC iteration is in git history).

