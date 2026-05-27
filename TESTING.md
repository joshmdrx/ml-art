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

**Acceptance:** comprehensive coverage of contract + ownership + validation; full suite under 30 seconds locally. Current count: **115 tests, ~7s** — 104 integration (health 1, anon_id 4, artist 4, artwork 6, collections 14, inquiries 9, neighborhoods 8, search 14, rate_limit 5, artwork_embeddings 5, studio 28, uploads 6) + 11 core unit (`middleware::rate_limit` 9 + `images` 2). Rate-limit tests use the `app_with_rate_limit(pool, search_per_min, inquiry_per_hour)` helper which flips the bypass off and dials quotas low so denial happens within 3–4 calls (no clock-faking required). Embedding-pipeline + studio image-add + uploads tests use `embedder_with_fixed_vector(pool, vec)` / `app_with_auth_and_fixed_vector(pool, vec)` so `process_image` and the upload-embed path run end-to-end without hitting Jina. Uploads tests use `ObjectStore::for_tests` (in-memory) so MinIO isn't required to run the suite.

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

**Coverage today (read-only browse + anonymous writes):**

1. `/` loads → "Recently added" grid is non-empty
2. Search "ukiyo" from hero → results page shows Ukiyo-E cards
3. Click a card → artwork detail page with image + "More like this"
4. From a card, click artist name → portfolio with works
5. `/neighborhoods` → click "Fields of Color" → detail page with works
6. Unknown artist / artwork / neighborhood slugs → Next 404 pages
7. Impossible filter (`location=nowhere-no-studio-here`) → empty state
8. `/search?location=berlin` → results restricted to Berlin studios
9. Anonymous Inquire end-to-end: open modal → submit → "Check your inbox" state → dev verify link resolves
10. Signed-out Save click → redirect to `/sign-in?redirect_url=…`
11. Verify page with bogus token → "Link doesn't look right"

**Gap: signed-in flows.** Save-modal interactions (open, toggle, create-with-first-artwork), Inquire when signed-in (email pre-filled, immediate "Sent"), and the future studio surfaces all need a way to drive Playwright as a signed-in user without Clerk's real OTP flow. Tracked as `T-031` in `TODO.md` (web test-mode session bypass mirroring the Rust `JwtVerifier::for_tests()` pattern).

**Where:**
- `e2e/playwright.config.ts` — config + reporters
- `e2e/tests/*.spec.ts` — flows
- `e2e/fixtures/` — shared setup

**Retry policy:** retry failed tests once; two failures = real bug,
not flakiness.

**Acceptance:** flows pass against the local stack in under 60s. Current: ~11 specs, ~5s.

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
