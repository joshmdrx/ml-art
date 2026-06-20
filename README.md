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
| Search | `/search?q=…&medium=…&price=…&availability=…&location=…&image_upload_id=…&modifiers=…` | `/v1/search` (hybrid + geographic + filters + visual anchor + modifier δ-vectors at α=0.8) |
| Visual upload | camera icon on the hero | `/v1/uploads/image` (multipart → S3 → inline Jina embed) |
| Artwork detail | `/artworks/[id]` | `/v1/artworks/:id` + `/v1/artworks/:id/similar` |
| Artwork share preview | shared link of `/artworks/[id]` | `/artworks/[id]/opengraph-image` — 1200×630 PNG composed at request time (T-051) |
| Artist portfolio | `/artists/[slug]` | `/v1/artists/:slug` (incl. `locations[]` → map widget; `is_following` + `follower_count`) |
| Artist share preview | shared link of `/artists/[slug]` | `/artists/[slug]/opengraph-image` — name + 2×2 work grid (T-051) |
| Follow / unfollow an artist | button on `/artists/[slug]` | `POST`/`DELETE /v1/me/follows/:artist_id`; `<FollowButton>` queues the intent against the anon cookie if signed-out so the merge handler replays it after sign-in (T-052 + T-052c) |
| Search by map | `/search?map=1&bbox=…&artist=…` | `/v1/search/map` — clustered pins, "Near me" geolocation, city-pivot pills, per-artist filter |
| Map city pivots | strip above `/search?map=1` | `/v1/search/map/cities` — top cities by venue count |
| Neighborhoods index | `/neighborhoods` | `/v1/neighborhoods` |
| Neighborhood detail | `/neighborhoods/[slug]` | `/v1/neighborhoods/:slug` (filterable) |
| Sign in / up | `/sign-in`, `/sign-up` | Clerk |
| Become an artist | `/onboarding` | `/v1/onboarding/start` + `/v1/onboarding/complete` (5-step wizard) |
| Artist studio | `/studio`, `/studio/settings`, `/studio/inquiries` | `/v1/studio/*` (artworks CRUD, settings, locations CRUD, inquiry inbox + threaded replies, follower count) |
| Current user (debug) | `/me` | `/v1/me` |
| Save to collection | modal on artwork detail | `/v1/me/collections/*` |
| My collections | `/collections`, `/collections/[id]` | `/v1/me/collections/*` |
| Public collection link | `/c/[share_id]` (anyone) | `/v1/collections/share/:share_id` — owner toggles via the "Sharing" panel on `/collections/[id]`; per-collection OG card (T-053) |
| Inquire about an artwork | modal on artwork detail | `/v1/artworks/:id/inquiries` (anonymous → verify email link → delivered via Resend) |
| Verify anonymous inquiry | `/inquiries/verify/[token]` | `/v1/inquiries/verify/:token` |
| Email notification settings | `/me/settings/notifications` | `/v1/me/notification-preferences` — per-kind toggles + master kill switch (T-068) |
| Unsubscribe from a kind | `/u/[token]` (from email footer; GET or RFC 8058 POST) | `/v1/notifications/unsubscribe[/oneclick]` (T-068) |
| Daily new-works digest | email (daily 11:00 UTC) | EventBridge cron → SQS → `JobEvent::NotifyFollowersDigestKickoff` → per-user fan-out → Resend with `List-Unsubscribe` headers (T-052b) |

Vector search activates when `JINA_API_KEY` is set in `api/.env`. Without
it, search degrades cleanly to keyword-only and the empty state explains
why. Rate limiting (`/search` 60/min, inquiry 3/hr per key) is on by
default in dev; set `RATE_LIMIT_DISABLED=true` to hammer-test locally.

Geographic features (artist-profile map, `/search?map=1`, studio location
CRUD with live geocoding) activate when `MAPBOX_TOKEN` is set in
`api/.env` AND `NEXT_PUBLIC_MAPBOX_TOKEN` is set in `web/.env.local`.
Without them, the schema + APIs still work but map widgets fall back to
a non-interactive list view (no JS map bundle loads). Free tier covers
100k geocoding + 50k map loads per month — well above any v0 traffic.

Real email delivery (Resend) is wired and live in prod for inquiry
verification, inquiry-delivered-to-artist, artist replies (T-032 + T-011
Phase 4b), and the daily new-works digest (T-052b). Dev mode still
returns the verification token in the response body when `RESEND_API_KEY`
is unset so manual testing works without an inbox.

Every notification email carries an unsubscribe link + RFC 8058
`List-Unsubscribe` + `List-Unsubscribe-Post` headers so Gmail/Outlook
render their built-in one-click unsubscribe UI (T-068).

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
- **Auth:** Clerk
- **Geocoding + maps:** Mapbox v6 forward-geocoding + Mapbox GL JS (T-038)
- **Background jobs (planned):** Inngest — currently a `tokio::spawn` stub for the geocoding worker; the real runtime unblocks `T-032` (inquiry email), `T-008` (image moderation), and `T-012 Phase 2` (LLM-assisted onboarding)
- **Infra (planned):** Terraform, OpenNext for Next on AWS

See [`04-stack-and-infra.md`](04-stack-and-infra.md) for the full picture
including the topology diagram and cost projection.

## Testing

~257 tests total, ~10s locally for the Rust + Vitest tiers (E2E adds another ~30s against the full stack):

- **206 Rust tests** — `make test-api` (per-test ephemeral Postgres via `#[sqlx::test]`; unit + integration combined)
- **~24 Playwright E2E tests** — `make test-e2e` (against the full local stack)
- **27 Vitest unit tests** — `make test-web` (pure functions only)
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
