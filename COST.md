# Cost Management

This is a side project. Burning money on idle infra defeats the point. This
doc captures the spend posture, free-tier audit, and code-level guardrails.

## Posture

- **Default mode:** free-tier everywhere. No service incurs charges at v0/v1
  traffic.
- **Defensive code:** every paid API has an env-var gate. Missing key →
  feature degrades gracefully (e.g. no embedding cache hit fallback to
  baseline, no LLM-polish button shown).
- **Hard ceiling:** AWS Budgets alarm at $20/mo total. Email + the deploy
  pipeline refuses to apply Terraform if a recent budget breach is detected.

## Free-tier audit

| Service | Free tier | Plausible v1 use | Headroom |
|---|---|---|---|
| Neon Postgres | 0.5 GB storage, generous compute | 2000 demo artworks + ~50 real = <50 MB | 10× |
| Clerk | 10,000 MAU | <100 users at v1 | 100× |
| Inngest | 50,000 step runs / mo | <5k expected | 10× |
| Upstash Redis (rate limiting) | 10k commands / day | Unlikely to use; Postgres caches are first choice | unused |
| PostHog | 1M events / mo | ~10k–50k events from 1k DAU | 20× |
| Resend | 3,000 emails / mo | Verification + inquiries — should be <500 | 6× |
| Mapbox geocoding | 100k requests / mo | Only on artist update → <100 / mo | 1000× |
| Hugging Face Hub | Free downloads | One-time model pull on dev machine | n/a |
| AWS Lambda | 1M invocations + 400k GB-s / mo | <100k at v1 | 10× |
| AWS S3 | 5 GB storage, 20k GET, 2k PUT / mo | ~2 GB for demo + few hundred real artworks | borderline |
| AWS CloudFront | 1 TB egress + 10M req / mo | Heavily depends on image traffic | wide range |
| AWS Rekognition (moderation) | first 5000 images / mo free | ~2000 demo (one-time) + new artworks (low) | 2× |

**Watch items:**
1. **S3 + CloudFront** as image traffic grows — bandwidth is the real risk
2. **Anthropic / Jina embeddings** — pay-per-call, no free tier

## Pay-per-call services and expected v1 spend

| Service | Unit | Rate | v1 expected |
|---|---|---|---|
| Jina embeddings (jina-clip-v2) | per embedding | ~$0.000018 (~$0.02 / 1000) | ~$1–5 / mo (query cache helps) |
| Anthropic Claude (intake + polish) | per token | varies by model | ~$5–20 / mo (only during onboarding) |
| OpenAI (fallback) | per token | varies | $0 if Anthropic stays default |

**Total expected v1 burn: $10–30 / mo** before any deploy. Most of that is
embedding API + Anthropic.

## Concrete guardrails to add to code

### 1. Required-env-var gates on all paid APIs

Each paid integration ships with two paths:

```rust
// pseudo
match cfg.jina_api_key {
    Some(k) => JinaEmbedder::new(k).embed_text(q).await,
    None => fall back to keyword search only, log warning,
}
```

This means **local dev can run without ANY paid keys**. The seed script
already requires only the local embedder. Search will degrade to keyword-only
without a Jina key — useful even on developer laptops.

### 2. Per-environment kill switches

Env vars per service:

| Var | Purpose | Default in dev |
|---|---|---|
| `JINA_ENABLED` | Skip Jina API calls if false | `true` if key present |
| `ANTHROPIC_ENABLED` | Skip LLM intake / polish | `false` (saves spend) |
| `MAPBOX_ENABLED` | Skip geocoding | `false` (saves quota) |
| `REKOGNITION_ENABLED` | Skip moderation | `false` (uses always-approve stub) |
| `POSTHOG_ENABLED` | Skip event submission | `false` (writes to local events table only) |

Setting `*_ENABLED=false` in `.env.local` lets you develop offline with no
external calls.

### 3. AWS Budgets alarm

Terraform module: `infra/modules/cost/budget.tf` defines a $20/mo alarm
emailed to the developer. Tripping it should fire a Slack webhook (when we
have Slack) and disable Inngest production functions until acknowledged.

To add: an Inngest job `cost.health_check` that runs hourly and reads AWS
Cost Explorer + Anthropic + Jina usage; emails if any service is on track
to exceed its per-month threshold.

### 4. Rate limiting at the edge

Already specified in `03-api-data-spec.md` (Upstash). Doubly important for
cost: a runaway scraper hitting `/search` 10,000 times costs us 10,000
embedding API calls if the query cache doesn't catch them. Rate limit per
IP + per anon_id, 60/min on search.

### 5. Query embedding cache

`query_embedding_cache` table (added in migration 0008) deduplicates text
query embeddings. Common queries ("moody coastal", "abstract painting") pay
the Jina cost once, not per request.

### 6. Image hosting

Images uploaded by artists go to S3, served via CloudFront with `Cache-Control: public, max-age=31536000, immutable`. Variants generated on-the-fly by an image proxy with its own cache. The single biggest egress risk is
hotlinks from external sites — mitigation: signed URLs for non-thumbnail
variants in v1.1 if it becomes a problem.

## Spend monitoring

Pre-launch checklist (before any code reaches `prod` Terraform workspace):

- [ ] AWS Budgets alarm configured at $20/mo (production account only)
- [ ] Anthropic spend cap in the Anthropic console set to $30/mo
- [ ] Jina spend cap set
- [ ] PostHog quota notification at 80% of free tier
- [ ] Resend quota notification at 80% of free tier
- [ ] CloudWatch metric: `lambda_invocations` per route group; alarm if
      any route exceeds 100k/day
- [ ] CloudWatch metric: `cloudfront_bytes_downloaded`; alarm at 500 GB/mo

## Decommissioning costs (the easy ones to forget)

Things that cost money even at zero traffic:

- Route53 hosted zone: $0.50/mo per zone
- Domain registration: ~$10–30/yr
- ACM certs are free
- CloudFront distributions are free at rest
- S3 storage is per-GB-month (cheap, but compounds)
- Neon non-free plans bill for compute hours even when idle (their free
  tier scales to zero — stay on it as long as possible)

## Re-evaluation triggers

Revisit this doc when any of the following hold:

- We onboard 50+ real artists (S3 / CloudFront usage will jump)
- Search traffic exceeds 1k requests / day
- Any paid service shows up on a budget alarm
- We start paying for a service that was previously free
