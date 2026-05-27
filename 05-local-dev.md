# Art Discovery Platform — Local Development (v1)

## Goals

- One-command spin-up of the full stack on a dev machine.
- Real product flows work end-to-end locally: search, save, inquiry verification, onboarding intake, background jobs.
- No reliance on deployed AWS for day-to-day work.
- Explicit list of things that **don't** work locally — test those in staging.

## Topology

```
   ┌─────────────────── localhost ───────────────────┐
   │                                                  │
   │  next dev (:3000) ──── http ────► cargo lambda   │
   │       │                            watch (:9000) │
   │       │                                 │        │
   │       │                                 ▼        │
   │       │                          ┌──────────────┐│
   │       │                          │  postgres+   ││
   │       │                          │  pgvector    ││
   │       │                          │  (Docker)    ││
   │       │                          └──────────────┘│
   │       │                                          │
   │       │                          ┌──────────────┐│
   │       └──► inngest-cli dev ─────►│ same Rust    ││
   │            (:8288)               │ handlers via ││
   │                                  │ /inngest/*   ││
   │                                  └──────────────┘│
   │       │                                          │
   │       └──► MinIO (S3-compat, :9001)              │
   │       └──► Mailhog (:8025)                       │
   │                                                  │
   └──────────────────────────────────────────────────┘

   Real cloud (dev tier):
     - Clerk dev instance
     - Jina or Voyage (real API, dev key)
     - Anthropic / OpenAI (real API, dev key)
     - PostHog dev project
     - Resend sandbox (or Mailhog for email-only flows)
```

## Stack

### Docker Compose (`docker-compose.dev.yml`)

```yaml
services:
  postgres:
    image: pgvector/pgvector:pg16
    environment:
      POSTGRES_USER: ml_art
      POSTGRES_PASSWORD: dev
      POSTGRES_DB: ml_art_dev
    ports: ["5432:5432"]
    volumes: ["pgdata:/var/lib/postgresql/data"]

  minio:
    image: minio/minio
    command: server /data --console-address ":9001"
    environment:
      MINIO_ROOT_USER: dev
      MINIO_ROOT_PASSWORD: devpassword
    ports: ["9000:9000", "9001:9001"]
    volumes: ["miniodata:/data"]

  mailhog:
    image: mailhog/mailhog
    ports: ["1025:1025", "8025:8025"]  # SMTP, UI

volumes:
  pgdata:
  miniodata:
```

### Native processes

```bash
# Terminal 1: deps
docker compose -f docker-compose.dev.yml up

# Terminal 2: frontend
pnpm dev

# Terminal 3: API
cargo lambda watch

# Terminal 4: background jobs
pnpm inngest:dev   # wraps `npx inngest-cli@latest dev`
```

Or wrap all four in a single `make dev` / `pnpm dev:all` using [`concurrently`](https://www.npmjs.com/package/concurrently) once you're tired of four terminals.

## Configuration

`.env.local` (gitignored) per developer. `.env.example` (committed) has the keys.

```dotenv
# Database
DATABASE_URL=postgres://ml_art:dev@localhost:5432/ml_art_dev

# Object storage (MinIO in dev)
S3_ENDPOINT=http://localhost:9000
S3_BUCKET_ARTWORKS=artworks
S3_BUCKET_UPLOADS=uploads
AWS_ACCESS_KEY_ID=dev
AWS_SECRET_ACCESS_KEY=devpassword
AWS_REGION=us-east-1

# Auth (real Clerk dev instance)
CLERK_PUBLISHABLE_KEY=pk_test_...
CLERK_SECRET_KEY=sk_test_...

# Embeddings (real, dev key with spend cap)
JINA_API_KEY=...
# or VOYAGE_API_KEY=...

# LLM (real, dev key with spend cap)
ANTHROPIC_API_KEY=...

# Email (Mailhog SMTP locally)
SMTP_HOST=localhost
SMTP_PORT=1025

# Analytics
POSTHOG_KEY=phc_dev_...
POSTHOG_HOST=https://eu.posthog.com

# Cookie signing
ANON_COOKIE_SECRET=dev-secret-rotate-in-prod

# Inngest local dev signs nothing
INNGEST_DEV=1
```

## Seed data

`scripts/seed.ts` (or `cargo run --bin seed`):

1. Creates 3 admin users.
2. Inserts ~30 sample artists with bios, slugs, inquiry preferences.
3. Inserts ~300 artworks with images uploaded to MinIO (downloaded once from a public source or committed to `seed/images/`).
4. Generates embeddings (calls real Jina/Voyage with dev key; ~$1 in API spend per seed run; cached in `seed/embeddings.json` to skip on subsequent runs).
5. Creates 6 manually-curated neighborhoods with representative artworks.
6. Loads `eval_set` with ~30 hand-curated queries.

```bash
pnpm seed         # first run: hits embedding API, ~$1, ~2min
pnpm seed --fast  # uses cached embeddings if present
pnpm seed --reset # truncates first
```

This makes search, recommendations, and neighborhoods genuinely usable in local dev, not a sad empty-state demo.

## What works locally

- Full HTTP request lifecycle: middleware → API → DB → response.
- Anonymous cookie issuance and behavior.
- Search (keyword + semantic + visual via real embedding calls).
- Save / collections / inquiry flow including email verification (Mailhog).
- LLM onboarding intake (real API).
- Inngest jobs (run in-process via `inngest-cli dev`).
- Background image moderation: stub locally (always-approve unless filename starts with `nsfw_`), real Rekognition only in staging/prod.

## What does NOT work locally — test in staging

- **CloudFront caching & headers** — local has no CDN; staging does.
- **Image proxy / on-the-fly transforms** — locally we serve originals from MinIO; size-prefixed URLs are translated to originals via a dev middleware.
- **OpenNext-deployed Next.js quirks** — middleware in Lambda@Edge, ISR revalidation, image optimization at the edge.
- **Real Lambda cold starts** — `cargo lambda watch` is a persistent process.
- **IAM permission boundaries** — local code runs with full creds; deployed Lambdas have scoped roles.
- **API Gateway request/response transformations and limits.**
- **AWS Rekognition** — we stub locally.
- **Real Clerk webhook delivery** — Clerk dev instance can post webhooks to a tunneled localhost (use `cloudflared tunnel` or `ngrok`), but it's easier to test webhook handlers in staging.

For each of the above: cover with **staging E2E** before promoting to prod.

## Test tiers

| Tier | What | Runs where | When |
|---|---|---|---|
| Unit | Rust handlers, TS components, pure functions | Local + CI | On every save / push |
| Integration | API against Docker Postgres + MinIO | Local + CI (with service containers) | On every push |
| E2E (local) | Playwright against `next dev` + local API + Docker deps | Local + CI | On every push |
| E2E (staging) | Same Playwright suite against deployed staging URL | CI | After merge to `main`, before prod promote |

Eval-set runs (`eval.run` Inngest job) are not in the test pipeline — they run on a schedule and post results separately. A drop in NDCG@10 doesn't block CI, but does post a high-visibility alert.

## Common pitfalls

- **Embedding dimension drift** — if you change embedding model mid-development, the existing `artwork_embeddings.embedding` column dimension may mismatch. Reset DB or use a new `model_version`.
- **Cookie SameSite in local dev** — set `SameSite=Lax` not `Strict` so flows that bounce through Clerk work.
- **MinIO ↔ S3 SDK quirks** — use path-style URLs (`s3_force_path_style: true` in the SDK config).
- **Inngest local function discovery** — restart `inngest-cli dev` after adding a new function; it watches the introspection endpoint but doesn't always pick up new ones cleanly.
- **Long search latency in dev** — embedding API roundtrip dominates. Cache common queries in a dev-only in-memory store if it's blocking iteration.

## First-time setup checklist

1. `brew install rustup-init pnpm docker awscli` (or equivalent on Linux).
2. `cargo install cargo-lambda`.
3. Clone, `pnpm install`.
4. Create Clerk dev instance, copy keys to `.env.local`.
5. Generate dev API keys for Jina/Voyage and Anthropic. Set monthly spend cap (~$20).
6. `docker compose -f docker-compose.dev.yml up -d`.
7. `pnpm db:migrate`.
8. `pnpm seed`.
9. `pnpm dev:all`.
10. Open `localhost:3000`, search "moody coastal", confirm results.

If all 10 work, you're set up.
