# Art Discovery Platform — Stack & Infrastructure (v1)

## Principles

- All-AWS where it makes sense, fully Terraformed. Managed services where the free/cheap tier is meaningfully better than self-hosting (auth, vector DB hosting, background jobs).
- One IaC source of truth (Terraform). Same modules per environment, only variables differ.
- Three environments: `dev` (local Docker + native processes), `staging` (AWS, low-tier sizing), `prod`. Staging is the place we test anything that depends on deployed behavior (CloudFront, IAM, Lambda cold starts).
- No vendor lock-in we can't escape in a weekend.

## Topology

```
                ┌──────────────────────────────────────────────┐
                │                  CloudFront                   │
                │  (TLS, caching, image transforms via proxy)   │
                └──────┬───────────────────────────┬────────────┘
                       │                           │
              static/ssr│                     /v1/* │
                       ▼                           ▼
              ┌────────────────┐         ┌──────────────────┐
              │  Next.js via   │         │   API Gateway    │
              │   OpenNext     │         │  (REST, /v1/*)   │
              │  (Lambda + S3) │         └────────┬─────────┘
              └────────┬───────┘                  │
                       │                          ▼
                       │                ┌──────────────────┐
                       │                │  Rust Lambdas    │
                       │                │  (handlers per   │
                       │                │   route group)   │
                       │                └────────┬─────────┘
                       │                         │
                       ▼                         ▼
              ┌────────────────────────────────────────────┐
              │     Neon Postgres + pgvector (managed)     │
              └────────────────────────────────────────────┘

  Side systems:
    Clerk (hosted)         — auth UI + JWT issuance
    Inngest (hosted)       — background jobs (embed, moderate, import, eval, purge)
    Upstash Redis          — rate limiting + ephemeral counters
    S3 (uploads/, artworks/, static/) — object storage; CloudFront in front
    Rekognition            — image moderation
    Jina / Voyage          — multimodal embeddings (HTTP API)
    Anthropic / OpenAI     — LLM intake + statement polish (HTTP API)
    Resend                 — transactional email (verification, inquiry delivery)
    PostHog                — product analytics
    CloudWatch (+ optional Axiom) — logs & metrics
```

## Component choices and why

### Frontend hosting: Next.js via OpenNext → Lambda + CloudFront + S3

- OpenNext compiles a Next.js app to a Lambda (server) + static assets in S3 + a CloudFront distribution that fronts both.
- Gives us SSR (needed for SEO on artwork/artist pages), ISR (for periodic regeneration of artist pages), image optimization, server components, and route handlers — all on AWS, all Terraform-managed.
- Not Vercel: avoids the second cloud, gives full IaC, costs scale linearly with use rather than tiered.

### API: Rust Lambdas behind API Gateway

- One Cargo workspace, one binary per route group (`search`, `artworks`, `artists`, `me`, `studio`, `onboarding`, `inquiries`, `uploads`, `events`). Each compiles to its own Lambda.
- All handlers use [`lambda_http`](https://docs.rs/lambda_http) so the same code can run as a local Axum HTTP server during dev (`cargo lambda watch`) and as a Lambda in deployed envs.
- Shared crate `core/` holds DB access (sqlx), domain types, embedding clients, validation, and the rate-limit middleware.
- Why Rust: fast cold starts (~30–80ms with a tuned bootstrap), tiny binaries, type-safe SQL via sqlx, low Lambda cost at scale. Cost: slower iteration vs TS — accepted.

### Database: Neon Postgres + pgvector (managed, not in AWS)

- Neon's serverless Postgres scales to zero, branches per PR, has pgvector built in.
- Lives outside our AWS account — that's fine; the Lambda VPC config calls out to Neon over the public internet with TLS.
- Alternative considered: RDS Postgres. Rejected for v1 — more setup, no branching, more expensive at low scale.

### Background jobs: Inngest (hosted)

- Free up to 50k runs/month, generous for v1.
- Step-functions-like DX without setting up Step Functions ourselves.
- Jobs are deployed as ordinary HTTP handlers (Rust Lambdas at `/v1/internal/inngest/*`) that Inngest invokes.

### Auth: Clerk (managed)

- Free up to 10k MAU, covers v1.
- Hosted UI components, magic links, OAuth providers all included.
- API validates Clerk JWTs server-side. User row mirrored in our DB on first sign-in (`users.clerk_user_id`).

### Object storage: S3 with CloudFront image proxy

- S3 buckets: `artworks/`, `uploads/` (visual search, 24h TTL via lifecycle rule), `static/` (OpenNext build assets).
- Image variants are **not stored** — generated on demand by an image proxy (imgproxy on Lambda, or AWS Serverless Image Handler) behind the same CloudFront distribution. URL pattern: `https://img.<domain>/<size>/<s3_key>`.
- Cache: CloudFront caches per URL, TTL 1 year on artwork images (immutable; rename on edit if needed).

### Rate limiting: Upstash Redis

- Sliding-window rate limiter via `@upstash/ratelimit` (TS) on Next.js routes, or a thin Rust client in the Lambdas.
- Free tier (10k commands/day) covers v1; pay-as-you-go after.
- Not Redis on AWS (ElastiCache) — overkill and expensive for v1.

### Image moderation: AWS Rekognition

- `DetectModerationLabels` on every uploaded image (artist artworks + visual-search uploads).
- ~$1 / 1000 images. Negligible at v1.
- Invoked from the `image.moderate` Inngest job.

### Geocoding + maps: Mapbox (shipped, T-038)

- **Forward-geocoding** via Mapbox v6 — wired in `core::geocoding`. Real / Disabled / Test variants behind a `GeocodingClient` abstraction; falls back to `Disabled` when `MAPBOX_TOKEN` is unset so dev works without a paid key.
- **Map widgets** via Mapbox GL JS — `/artists/[slug]` map widget + `/search?map=1` clustered map mode. Same token (as `NEXT_PUBLIC_MAPBOX_TOKEN`).
- Currently driven from the studio CRUD path (`POST /v1/studio/locations`) which `tokio::spawn`s the geocode work inline. When the Inngest runtime lands we swap that for an `artist_location.geocode` function — same `geocode_and_update` body, different driver. Documented in `core::geocoding` module docs.
- 100k requests/month free + 50k map loads/month free covers v1 by a large margin.
- Returns structured city, country, lat, lng.
- Alternative considered: Nominatim (OpenStreetMap) — free, self-host or use public servers — rejected for v1: public Nominatim is rate-limited to 1 req/s and unreliable. Self-hosted is extra infra. Revisit if Mapbox cost ever becomes a concern.
- Failure mode: `geocoded_at` is stamped, `lat`/`lng` stay NULL; row hidden from public surfaces. Artist re-edits to retry. Map widget falls back to a "based in {city}" pill or a list view.

### Embeddings: Jina or Voyage (HTTP API)

- Multimodal (image + text) so the same model embeds artworks and search queries.
- Versioned in our `artwork_embeddings` table by `(model_name, model_version)` so we can A/B and re-embed without downtime.
- Spike both, pick on quality + price.

### Observability: CloudWatch (v1) → Axiom (later)

- Lambda logs to CloudWatch by default. Structured logs (JSON) so we can grep + query.
- Custom metrics via CloudWatch EMF.
- Axiom is a candidate for v1.1 if CloudWatch insights query latency or cost becomes painful.

## Environments

| Env | Where | Purpose |
|---|---|---|
| `dev` | Local — Docker Compose + native processes | Day-to-day work, iterates fast. See `05-local-dev.md`. |
| `staging` | AWS account `staging`, full deploy via Terraform | Pre-prod validation. PR previews deploy here. Used for any test that depends on real CloudFront / Lambda / IAM behavior. |
| `prod` | AWS account `prod`, full deploy via Terraform | Live. Deploys only from `main` via CI after staging E2E passes. |

Same Terraform modules; environments differ only by `terraform.tfvars` (sizes, domains, secret references, scale).

## Terraform layout

```
infra/
├── modules/
│   ├── api/              # API Gateway + Rust Lambdas
│   ├── frontend/         # OpenNext output → Lambda + S3 + CloudFront
│   ├── images/           # S3 buckets + image proxy + CloudFront
│   ├── networking/       # CloudFront distributions, ACM, Route53
│   └── observability/    # CloudWatch log groups, alarms
├── envs/
│   ├── staging/
│   │   ├── main.tf
│   │   └── terraform.tfvars
│   └── prod/
│       ├── main.tf
│       └── terraform.tfvars
└── shared/
    └── backend.tf        # S3 + DynamoDB lock for state
```

Secrets in AWS Secrets Manager; Terraform references by ARN, never stores secret material.

## Deploy flow

1. PR opened → CI runs Rust + TS unit tests, `terraform plan` against staging.
2. PR merged to `main` → CI builds OpenNext bundle, compiles Rust Lambdas, runs `terraform apply` against staging, runs Playwright E2E against staging URL.
3. Manual promotion (one-click in CI, or `make promote-prod`) → `terraform apply` against prod with the same artifact.

No auto-deploy to prod in v1 — promotion is intentional.

## Estimated cost at v1 traffic

Rough monthly cost assuming ~1k DAU, ~10k searches/day, ~100GB image egress, ~50k embedding calls/month:

| Item | Cost |
|---|---|
| AWS Lambda (frontend SSR + API) | ~$5–10 |
| API Gateway | ~$3–5 |
| S3 + CloudFront (storage + egress) | ~$10–30 |
| Rekognition (moderation) | ~$1–5 |
| Neon Postgres | $0 free tier → ~$20 once we outgrow |
| Clerk | $0 |
| Inngest | $0 |
| Upstash Redis | $0 |
| PostHog | $0 |
| Jina/Voyage embeddings | ~$5–20 |
| Anthropic/OpenAI (intake + polish) | ~$10–30 |
| Mapbox geocoding | $0 (under 100k/mo, geocode-on-change only) |
| Resend | $0 (under 3k/mo) |
| Domain + Route53 | ~$2 |
| **Total** | **~$40–125/mo** |

This is well within the budget of a side-project / small seed-stage build.

## Kill / pivot metric

Monetization is deliberately deferred — we build the discovery experience and validate it before deciding on transaction handling, subscriptions, or lead-gen. To avoid the "unmonetized platform limps along forever" failure mode, define a concrete kill/pivot threshold now:

> **By [3 months after public launch], if we have not reached: 50 active artists with ≥10 published artworks each, 500 weekly active viewers, and ≥2 inquiries per artist per month — we pause feature work and either pivot the model (curated marketplace, paid subscription, etc.) or sunset.**

Adjust numbers once we have real baseline data. The point is to have a number, not the specific values.
