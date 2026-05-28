# ml-art

A discovery platform for independent contemporary artists. **Not a
marketplace** — no transactions, no commissions. Every artwork links out
to the artist's own site or socials so they own the buyer relationship.

The differentiator is search quality: hybrid keyword + multimodal vector
search (CLIP-style), geographic filters, curated thematic neighborhoods,
and — when the spike-validated path lands in UI — modifier buttons that
shift an image embedding semantically ("moodier", "warmer", "more
minimal").

> **Status:** side-project, pre-launch. Discovery loop (browse + filter +
> save + inquire) is working end-to-end against 2000 seeded demo artworks
> (WikiArt). No real artists onboarded yet — that's the next major surface
> (`T-011` studio + `T-012` onboarding). Brand name is still `ml-art` — a
> placeholder.

## What works today

| Surface | URL | Backing endpoint |
|---|---|---|
| Homepage | `/` | `/v1/search?sort=newest` + `/v1/neighborhoods` |
| Search | `/search?q=…&medium=…&price=…&availability=…&location=…` | `/v1/search` (hybrid + geographic + filters; also accepts `image_upload_id` as a visual anchor) |
| Artwork detail | `/artworks/[id]` | `/v1/artworks/:id` + `/v1/artworks/:id/similar` |
| Artist portfolio | `/artists/[slug]` | `/v1/artists/:slug` |
| Neighborhoods index | `/neighborhoods` | `/v1/neighborhoods` |
| Neighborhood detail | `/neighborhoods/[slug]` | `/v1/neighborhoods/:slug` (filterable) |
| Sign in / up | `/sign-in`, `/sign-up` | Clerk |
| Current user (debug) | `/me` | `/v1/me` |
| Save to collection | modal on artwork detail | `/v1/me/collections/*` |
| My collections | `/collections`, `/collections/[id]` | `/v1/me/collections/*` |
| Inquire about an artwork | modal on artwork detail | `/v1/artworks/:id/inquiries` (anonymous → verify email link → delivered) |
| Verify anonymous inquiry | `/inquiries/verify/[token]` | `/v1/inquiries/verify/:token` |

Vector search activates when `JINA_API_KEY` is set in `api/.env`. Without
it, search degrades cleanly to keyword-only and the empty state explains
why. Rate limiting (`/search` 60/min, inquiry 3/hr per key) is on by
default in dev; set `RATE_LIMIT_DISABLED=true` to hammer-test locally.

Real email delivery (Resend) is **not** wired yet — anonymous inquiries
return the verification token in the response body in dev mode so manual
testing works without an inbox (see `T-032`).

Vector search activates when `JINA_API_KEY` is set in `api/.env`. Without
it, search degrades cleanly to keyword-only and the empty state explains
why.

## Quick start

```bash
# Tooling required: Docker, Rust (stable), Node ≥ 22, pnpm, uv

make setup        # docker up + migrate + seed (one-time)
make dev          # api + web together; Ctrl-C stops both
```

That gives you:

- web at <http://localhost:3000>
- api at <http://localhost:9100/v1/health>
- MinIO console at <http://localhost:9001> (`dev` / `devpassword`)
- Mailhog at <http://localhost:8025>

Run `make` (with no target) to see the full target list. Common ones:

```bash
make status       # is anything listening?
make test         # api + web + ml unit tests (~10s)
make test-all     # everything including Playwright E2E
make check        # fmt + clippy + typecheck + lint (no tests)
make psql         # drop into the dev database
make seed-reset   # wipe and re-seed the demo corpus
make down         # stop services (keep data)
make nuke         # stop AND wipe volumes
```

## Repo layout

```
ml-art/
├── api/         Rust workspace — API binaries (currently: api-search)
├── web/         Next.js 16 app — TS + Tailwind + Radix, app router
├── ml/          Python package — embedding tooling, seed, spikes
├── db/          Postgres schema as numbered SQL migrations
├── e2e/         Playwright golden-path tests
├── scripts/     migrate.sh, dev.sh, status.sh
└── 01..05 / 99-deferred.md / decisions.md / STRATEGY.md / …
```

## Docs index

Strategy, decisions, and state-of-the-build all live as markdown at the
repo root. Each has a specific role:

| File | What's in it |
|---|---|
| [`CONTRIBUTING.md`](CONTRIBUTING.md) | Code conventions + pre-commit hook install |
| [`01-page-spec.md`](01-page-spec.md) | URL structure + page-by-page UX spec (**aspirational** — see `decisions.md` 2026-05-27) |
| [`02-component-library.md`](02-component-library.md) | Design tokens + component contracts (**aspirational**) |
| [`03-api-data-spec.md`](03-api-data-spec.md) | API endpoints + Postgres schema (**aspirational**) |
| [`04-stack-and-infra.md`](04-stack-and-infra.md) | Stack topology + cost-at-v1 estimates |
| [`05-local-dev.md`](05-local-dev.md) | Local-dev recipe + stub strategy |
| [`99-deferred.md`](99-deferred.md) | Post-v1 backlog (claim flow, spaces+events, monetization) |
| [`decisions.md`](decisions.md) | Chronological decision log w/ reversibility ratings |
| [`STRATEGY.md`](STRATEGY.md) | Open non-engineering tracks (outreach, brand, legal) |
| [`TODO.md`](TODO.md) | Engineering items, prioritized |
| [`CHANGELOG.md`](CHANGELOG.md) | What shipped, in date order — **truth for what was built** |
| [`COST.md`](COST.md) | Free-tier audit + spend guardrails |
| [`TESTING.md`](TESTING.md) | Test posture + tier-by-tier coverage |
| [`ml/spikes/.../FINDINGS.md`](ml/spikes/2026-05-modifier-deltas/FINDINGS.md) | Modifier-vector spike write-up |

## Stack

- **Frontend:** Next.js 16 (app router), Tailwind v4, Radix primitives, Vitest, Playwright
- **API:** Rust (axum + sqlx + pgvector), one Lambda binary per route group, `lambda_http` so the same handler runs locally as an Axum server and in deployed env
- **Database:** Postgres 16 + pgvector (Neon in prod)
- **Storage:** S3 + CloudFront (MinIO locally)
- **Embeddings:** `jinaai/jina-clip-v2` — local PyTorch for seed/spike, Jina HTTP API at request time
- **Auth (planned):** Clerk
- **Background jobs (planned):** Inngest
- **Geocoding (planned):** Mapbox
- **Infra (planned):** Terraform, OpenNext for Next on AWS

See [`04-stack-and-infra.md`](04-stack-and-infra.md) for the full picture
including the topology diagram and cost projection.

## Testing

50 tests total, ~7s locally:

- **28 Rust integration tests** — `make test-api` (per-test ephemeral Postgres via `#[sqlx::test]`)
- **11 Playwright E2E tests** — `make test-e2e` (against the full local stack)
- **11 Vitest unit tests** — `make test-web` (pure functions only)
- **Python pytest** — `make test-ml` (vector utils)

CI per-directory in `.github/workflows/` so PRs only run what they
touched. See [`TESTING.md`](TESTING.md) for the strategy and what's
deliberately out of scope.

## Contributing / working on this

Solo project right now. If you're reading this as a future me or a
collaborator: start with [`decisions.md`](decisions.md) for the *why*,
[`CHANGELOG.md`](CHANGELOG.md) for the *what's done*, and
[`TODO.md`](TODO.md) for the *what's next*. The biggest open question
isn't technical — it's whether real independent artists will publish
here, which is tracked in [`STRATEGY.md`](STRATEGY.md).

## Costs

Estimated **~$40–125/month** at v1 traffic (1k DAU). All paid APIs (Jina,
Mapbox, Anthropic, Rekognition) gracefully degrade to keyword-only / no-op
behavior when their env keys are absent, so local dev costs $0. Spend
guardrails in [`COST.md`](COST.md).
