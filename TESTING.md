# Testing strategy

> Goal: every commit that passes CI is safe to merge.
> Every commit that passes staging E2E is safe to promote to prod.
>
> Anything beyond that — full coverage, visual regression, property tests —
> we earn into, never preemptively. A 5k-line test suite nobody maintains
> is worse than a 500-line one everybody trusts.

This doc captures *what* we test, *how*, and *why we don't test* certain
things. Move items to `CHANGELOG.md` when the corresponding test infra
lands. Strategy decisions belong in `decisions.md` — this doc is the
operational reference.

---

## Posture

Solo-dev / side-project. We optimize for:

- **Fast iteration** — tests run in seconds, not minutes
- **High signal per test** — one test that catches a class of bugs beats
  ten that each catch one
- **Layered confidence** — unit < integration < E2E, each layer catches
  what the layer below can't see
- **Determinism** — flakiness is a bug; if a test is flaky we fix or
  delete it

We accept:

- Some false confidence (integration tests pass, prod still has UX bugs)
- Coverage gaps for things that change too fast to test
- One retry on E2E flakiness; two failures = bug

We don't do:

- Component-level React Testing Library tests (mostly JSON rendering;
  E2E covers the visible behavior)
- Visual regression / screenshot diffs (too much churn at v0)
- Property-based testing (overkill for CRUD)
- Load / perf tests (nothing to optimize against yet)
- 100% coverage targets (incentivises trivial tests)

---

## The tiers

### Tier 1 — Rust API integration tests

**Why first:** the Rust API is the contract. If it returns wrong JSON,
every page on the site is wrong invisibly. Integration tests catch
SQL bugs, handler logic, JSON shapes, status codes, and filter
behavior in one go.

**Tools:**
- `cargo test` with the built-in `#[sqlx::test]` macro
- Ephemeral per-test Postgres databases (sqlx applies migrations
  automatically before each test)
- A small SQL fixture file (`tests/fixtures/seed.sql`) inserting the
  minimum data needed: 3 artists, 6 artworks, 1 neighborhood, a few
  embeddings, primary images
- `tower::ServiceExt::oneshot` to call the Axum router directly — no
  ports bound, no networking overhead
- Stubbed embedder (`Embedder::with_fixed_vector`) so tests are
  deterministic and free

**Coverage targets:**
- Every endpoint: happy path + 404 + filter behavior
- Validation errors (e.g. `sort=nearest` without coords) return
  RFC 7807 problem+json with the right status
- Search returns expected ordering with text queries
- Geographic filter Haversine returns artworks within the radius
- Similar-artworks excludes same artist by default
- Soft-deleted rows don't appear in any query

**Where:**
- `api/crates/api-search/tests/` — integration tests live here
- `api/crates/api-search/tests/common/` — shared helpers
  (`spawn_app`, fixture loaders)
- `api/crates/api-search/tests/fixtures/` — SQL files

**Auth in integration tests** — covered endpoints divide into:
- **Unauthed**: `health`, `search`, `artist`, `artwork`, `neighborhoods` — built with `app_keyword_only(pool)` (no Clerk verifier configured).
- **Authed**: `me`, `me/collections`, `inquiries` (signed-in branch) — built with `app_with_test_auth(pool)`, which uses `JwtVerifier::for_tests()`. Tokens like `Bearer test-user_test_alice` are accepted and resolve to the pre-seeded `users` row with that `clerk_user_id`. Fixtures seed `alice` and `bob` for ownership-boundary checks.

The test-mode bypass is an explicit constructor (`for_tests()`), not env-gated — there's no way to enable it in a production binary without code changes. See `decisions.md` 2026-05-26 — test-mode JwtVerifier.

**Acceptance:** comprehensive coverage of contract + ownership + validation; full suite under 30 seconds locally. Current count: **131 tests, ~7s** — 114 integration (health 1, anon_id 4, artist 4, artwork 6, collections 14, inquiries 9, neighborhoods 8, search 14, rate_limit 5, artwork_embeddings 5, studio 28, uploads 16) + 17 core unit (`middleware::rate_limit` 9 + `images` 1 + `modifiers` 7) + 1 api-search unit (`meta::humanize`). Rate-limit tests use the `app_with_rate_limit(pool, search_per_min, inquiry_per_hour)` helper which flips the bypass off and dials quotas low so denial happens within 3–4 calls (no clock-faking required). Embedding-pipeline + studio image-add + uploads tests use `embedder_with_fixed_vector(pool, vec)` / `app_with_auth_and_fixed_vector(pool, vec)` so `process_image` and the upload-embed path run end-to-end without hitting Jina. Uploads tests use `ObjectStore::for_tests` (in-memory) so MinIO isn't required to run the suite. The visual-search-by-upload tests in `uploads_test.rs` (5 of the 11) seed an `uploads` row with a hand-crafted embedding so the search path runs against a known-deterministic anchor.

### Tier 2 — Playwright E2E (golden path)

**Why:** the only way to assert "a real user can complete this flow."
Catches React hydration bugs, missing data, broken styles, route
misconfigurations that integration tests can't see.

**Tools:**
- Playwright (TS), test files in a top-level `e2e/` directory
- Orchestrated against a real local stack: `docker compose up` +
  `cargo run -p api-search` + `pnpm dev` + the test runner
- Stub Jina via a wrapper (deterministic vectors per text query); we
  don't call real Jina from E2E

**Coverage:** [`docs/e2e-coverage.md`](./docs/e2e-coverage.md) is the
authoritative register. Add or update a row there when you ship a
user-visible feature — see [`CLAUDE.md`](./CLAUDE.md) → "E2E coverage
discipline".

At a glance, the suite covers:

- **Anonymous browse**: home, keyword search, artwork detail, artist
  portfolio, neighborhoods, 404s, empty state, location + generic
  FilterBar, visual-search shell, geography map shell
- **Anonymous writes**: inquire (with dev-verify link), signed-out
  save redirect, bogus verify token
- **Signed-in reads**: `/studio` + `/studio/settings` + `/studio/inquiries`
  + `/studio/series` gates, `/collections` index, onboarding identity
  step
- **Signed-in writes**: save modal (toggle + inline create), membership
  awareness, inquire (pre-filled email), follow toggle, notification
  prefs round-trip, collection make-public + share URL
- **Bridges**: anon → user merge (`/api/me/merge-anonymous`)
- **Unsubscribe**: bogus + missing token error copy on `/u/confirm`

**Signed-in test infrastructure**: `auth.setup.ts` signs up a fresh
Clerk user per run via the `*+clerk_test@*` test-email convention.
Storage state (cookies + localStorage) is persisted to
`e2e/.auth/user.json` and consumed by the `chromium-authed` project;
tests opt in by matching the filename pattern `*signed-in*.spec.ts`
(excluding `*admin-signed-in*` which routes to `chromium-admin`).

**Admin test infrastructure**: `admin.setup.ts` mirrors the flow with
an email suffixed `-admin+clerk_test@example.com`. The API's
`is_seeded_admin_email` matches that suffix against the
`WANDER_ADMIN_EMAIL_ALLOWLIST` env var (set by `scripts/dev.sh` +
`.github/workflows/e2e.yml`); the user is auto-promoted to
`is_admin=true` on first authenticated request. Storage state lives
in `e2e/.auth/admin.json`; the `chromium-admin` project picks up
`*admin-signed-in*.spec.ts`.

**Artist test infrastructure**: `artist.setup.ts` signs up a fresh
Clerk user AND drives them through the onboarding wizard end-to-end
(identity → publish), yielding a user with a linked `artists` row.
Metadata (email, display name, slug) persists to
`e2e/.auth/artist-meta.json` so downstream specs can reach the
public artist page or reference the display name. The
`chromium-artist` project consumes `e2e/.auth/artist.json` and picks
up `*artist-signed-in*.spec.ts`.

**Test-fixture insert seam**: two POST routes in `api-search`
(`/v1/testfixtures/artwork` + `/v1/testfixtures/inquiry`), guarded
by `WANDER_TEST_FIXTURES_ENABLED=1`. Routes never register in prod.
Wrapped by `e2e/lib/fixtures.ts` (`createArtwork`, `createInquiry`,
and the M-10 marketplace seams `enablePayouts` / `makeSellable` /
`createOrder`) so specs can seed world state under the fixture artist
without driving through image-upload / moderation / Jina paths that
make E2E flaky. Currently used by specs 49–54 (unread badge, URL-driven
modal, publish nudge, Buy-button visibility, mark-shipped, buyer orders).

**Where:**
- `e2e/playwright.config.ts` — config + reporters
- `e2e/tests/*.spec.ts` — flows
- `e2e/fixtures/` — shared setup

**Retry policy:** retry failed tests once; two failures = real bug,
not flakiness.

**Acceptance:** flows pass against the local stack in under 60s. Current: 53 specs (2 marketplace specs — buy-happy-path, admin-refund — skipped behind `STRIPE_E2E`, needing the live Stripe test-mode loop) + 3 setup fixtures (2026-07-10).

### Tier 3 — Vitest units (web)

**Why:** tiny gate for pure functions where bugs are easy to introduce
and hard to spot in E2E.

**Tools:** Vitest (Next-compatible, no browser needed)

**Coverage targets:**
- `formatPrice`: currency formatting, null handling, edge cases
- `formatDimensions`: missing fields, units
- `toQueryString` (in api.ts): undefined / null / empty handling
- API error parsing
- `describeQuery` summarizer on `/search`

**Where:** `web/src/__tests__/*.test.ts` (colocated tests can come
later)

**Acceptance:** ~15 tests; runs in under 5s. Current: 11 tests on `formatPrice` + `formatDimensions`, ~200ms.

### Tier 4 — CI

CI is what makes everything above valuable. Without it, tests are an
"I should remember to run these" obligation. With it, they're a wall.

**Tools:** GitHub Actions; per-directory workflows gated on `paths:`

**Workflows:**

- `.github/workflows/api.yml`
  - Triggers on `api/**` or `db/migrations/**`
  - Steps: `cargo fmt --check`, `cargo clippy -- -D warnings`,
    `cargo test --workspace`
  - Services: `pgvector/pgvector:pg16` on a known port; tests connect
    via `DATABASE_URL` env

- `.github/workflows/web.yml`
  - Triggers on `web/**`
  - Steps: `pnpm install`, `pnpm typecheck`, `pnpm lint`, `pnpm test`
    (vitest)

- `.github/workflows/e2e.yml`
  - Triggers on `web/**` or `api/**`
  - Spins up docker-compose stack, builds and runs the Rust API,
    builds Next.js, applies migrations + a small fixture seed, runs
    Playwright
  - Slowest workflow; gated behind the others passing

- `.github/workflows/ml.yml`
  - Triggers on `ml/**`
  - `uv run pytest` + `ruff check`

---

## Stubbing strategy

Tests must be **deterministic** and **free** — no external calls during
test runs. Stubs we maintain:

| Service | Stub strategy |
|---|---|
| Jina embeddings | `Embedder::with_fixed_vector(pool, vec)` returns a deterministic vector per call. Tests that need ranking-by-content insert specific embeddings and verify ordering. `Embedder::disabled(pool)` for tests that exercise the keyword-only path. |
| Clerk JWTs (Rust) | `JwtVerifier::for_tests()` bypasses JWKS entirely. Bearer tokens of the form `test-<clerk_user_id>` resolve to the seeded `users` row with that `clerk_user_id`. Explicit test constructor — production code can't accidentally enable it. |
| Clerk session (web) | **Open gap** — Playwright can't currently drive a signed-in flow because Clerk's hosted sign-in requires email + OTP. Tracked as `T-031`. |
| Mapbox geocoding | `MAPBOX_TOKEN` unset → Inngest job is a no-op. Tests bypass entirely. |
| AWS Rekognition | `REKOGNITION_ENABLED=false` → moderation auto-approves. |
| Anthropic LLM | `ANTHROPIC_API_KEY` unset → onboarding intake degrades to plain form. |
| Resend email | `RESEND_API_KEY` unset → inquiry rows still flip `delivered_at` but no email goes out; the delivery Inngest job (T-032) is a no-op. |

Each stub mirrors the real production behavior of "degrades gracefully
when the paid API is absent" — which is *also* how `COST.md` describes
local dev. Tests and dev get the same code paths.

---

## Build order

Each tier is independently useful. Stop after any tier and we're
still in a better place than now.

1. Tier 1 (API integration) + Tier 4a (`api.yml`)
2. Tier 2 (Playwright E2E)
3. Tier 3 (Vitest units) + Tier 4b (`web.yml`, `e2e.yml`)

---

## Conventions

- **Test names describe behavior, not implementation.** `returns_404_for_missing_artist` not `test_get_artist_handler_branch_2`.
- **One assertion per concept.** Multiple `assert!`s per test are fine
  when they test the same behavior; split into separate tests when
  they don't.
- **Fixtures over factories** for small, finite test data. We're not
  testing CRUD against generated data — we're testing JSON contracts.
- **Tests fail loud.** No silent retries inside test code. If a test
  needs to wait, use `until { … } else timeout`.
- **No `#[ignore]` checked in.** If a test is skipped, fix or delete.

---

## Manual smoke checklist

The automated tiers catch contract + regression bugs. They don't catch
"this whole flow feels wrong" — that's still a human job. Walk this
list:

- Before any meaningful demo
- After landing a change that touches more than one journey
- Before tagging a release once we have one

Two flavours: a **5-min smoke** for casual sanity, a **30-min
walkthrough** that exercises the four cross-cutting systems
(jobs queue, moderation, email delivery, anon→user merge) end-to-end
in a way no single Playwright spec covers.

### Pre-flight

```bash
make dev                     # docker + migrate + seed + api + worker + web
tail -f /tmp/worker.log      # so you see queue jobs land in real time
```

Expected: web on `:3000`, api on `:9100`, worker polling every 2s.

### 5-minute smoke — anonymous browse

1. `http://localhost:3000` → hero search + Near-me/Map row render
2. Search "blue" → grid populates (RRF score > 0 on hover-debug if dev tools)
3. Toggle **Map** → pins cluster, pan/zoom updates `?bbox=…`, city pills strip shows top cities
4. Click a popup → artwork detail; scroll → "More like this" populates
5. Click the artist name → portfolio; map widget renders pins
6. `/neighborhoods` → click a card → detail renders

**Pass:** no console errors, no 404s on image URLs, no `Internal Server Error`.

### 30-minute walkthrough — the systems

#### A. Moderation pipeline (T-008 + T-008b) — 5 min

1. Sign in as the seeded test artist (or any artist you've onboarded)
2. `/studio` → edit an artwork → add image with any `s3_key` (or `uploads/test.jpg`)
3. Worker log: `artwork_image_moderate` job claimed + run + done within 2s
4. `SELECT moderation_status FROM artwork_images ORDER BY created_at DESC LIMIT 1;` → `approved`
5. Manually flip: `UPDATE artwork_images SET moderation_status='rejected' WHERE id = '…';`
6. Public `/artworks/[id]` no longer shows the image ✓
7. Homepage → camera icon → upload an image
8. Worker log: `upload_moderate`; psql shows `uploads.moderation_status = 'approved'`
9. `UPDATE uploads SET moderation_status='rejected' WHERE id = '…';`
10. `GET /v1/search?image_upload_id=<that-id>` → 404 (not 200, not 400 — same shape as not-found)

#### B. Inquiries inbox + email loop (T-011 P4a + T-032) — 5 min

1. From a **logged-out** tab, inquire on one of the artist's artworks
2. Worker log: `inquiry_send_verification` (no actual email unless `RESEND_API_KEY` set)
3. Dev response body has `debug_verification_token`; visit `/inquiries/verify/<token>`
4. Worker log: `inquiry_deliver_to_artist`
5. Sign back in as the artist → `/studio/inquiries`
6. Card appears with **Delivered** badge, mailto link, message body
7. Filter pills (`All / Pending / Delivered`) toggle the URL + the list

#### C. Anon→user merge (T-033) — 3 min

1. Brand new **private window** (no Clerk session)
2. Upload an image via visual search; psql: `uploads.user_id IS NULL`, `anonymous_id` populated
3. Sign in/up in the same window
4. Dev tools → Network → `POST /api/me/merge-anonymous` fires once after sign-in (returns 200 with `uploads_merged: 1`)
5. psql: `uploads.user_id` now populated, `anonymous_id` preserved
6. Refresh — no second POST (`sessionStorage['mlart_anon_merged']` set)

#### D. Signed-in inquiry — direct delivery (3 min)

1. Different signed-in user (collector) inquires on an artist's work
2. Worker log: only `inquiry_deliver_to_artist` (no verification — Clerk email pre-trusted)
3. Artist's `/studio/inquiries` shows the row immediately as **Delivered**

#### E. Onboarding (5 min)

1. Sign up a brand-new user (no artist row)
2. Visit `/studio` → server redirects to `/onboarding`
3. Walk the 5 steps: identity → profile → artworks → locations → review
4. Verify in psql: `artists` row exists with `status='active'`, `user_id` linked
5. `/studio` opens to an empty portfolio
6. Add a location with a real address → "Locating…" → after worker runs, "Pin set · {city, country}"

#### F. Sanity sweeps (5 min)

- Hit `/v1/search` ~100 times fast → 429 after ~60/min
- >3 anon inquiries/hr from same anon cookie → 429
- `/studio` logged out → redirects to sign-in
- `/v1/me/merge-anonymous` without bearer → 401
- `/v1/studio/inquiries` as non-artist → 404
- Every page in journeys A–E: no console errors, no broken images

### Failure-mode catalogue

| Symptom | Likely cause |
|---|---|
| Jobs accumulate as `pending`, never run | jobs-worker not running. `ps aux \| grep jobs-worker` |
| Search returns keyword-only (no RRF) | `JINA_API_KEY` unset → degraded mode (expected, not a bug) |
| Location rows stuck at "Locating…" forever | `MAPBOX_TOKEN` unset → geocode no-ops |
| Inquiry verification email never arrives | `RESEND_API_KEY` unset → handler logs + returns Ok; use `debug_verification_token` from API response |
| Map widgets show list fallback only | `NEXT_PUBLIC_MAPBOX_TOKEN` not in `web/.env.local` |
| `/api/me/merge-anonymous` returns 401 | Browser dropped Clerk session; sign in again |

### What this catches that the automated tiers don't

- **Cross-system flows** — the moderation worker + the artist's experience seeing the result; the email worker + the artist's inbox showing the inquiry. Each side is unit/integration-tested; the join between them is human-only.
- **Visual regressions** — broken layouts, missing styles, hover states. We deliberately don't do screenshot diff (TESTING.md "We don't do").
- **Wrong data feeling** — "this should have 5 results, why does it show 50?" is a question only a human asks.

---

## When to evolve this doc

Revisit the strategy when:

- We have real users and uptime matters → add Tier 5 (synthetic
  monitoring against prod) and visual regression
- We onboard a second engineer → expand from solo-dev posture; add
  component tests
- We hit a class of bug that the current tiers miss → add the
  specific check, not the whole testing methodology
- We migrate to a new framework → reassess tool choices
- Test suite exceeds 5 minutes in CI → start caching, then start
  cutting

The goal isn't to test more — it's to test the right things.
