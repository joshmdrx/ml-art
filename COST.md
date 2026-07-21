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
| Neon Postgres | 0.5 GB storage + capped compute-hours/mo | 2000 demo artworks + ~50 real = <50 MB | storage 10×; **compute is the real limit** |
| Clerk | 10,000 MAU | <100 users at v1 | 100× |
| Inngest | 50,000 step runs / mo | <5k expected | 10× |
| Upstash Redis (rate limiting) | 10k commands / day | Unlikely to use; Postgres caches are first choice | unused |
| PostHog | 1M events / mo | ~10k–50k events from 1k DAU | 20× |
| Resend | 3,000 emails / mo | Verification + inquiries — should be <500 | 6× |
| Mapbox geocoding *(shipped, T-038)* | 100k requests / mo | On artist + location create/update → <100 / mo | 1000× |
| Mapbox GL JS map loads *(shipped, T-038)* | 50k loads / mo | Artist-profile + `/search?map=1` views — depends on traffic | wide range |
| Hugging Face Hub | Free downloads | One-time model pull on dev machine | n/a |
| AWS Lambda | 1M invocations + 400k GB-s / mo | <100k at v1 | 10× |
| AWS S3 | 5 GB storage, 20k GET, 2k PUT / mo | ~2 GB for demo + few hundred real artworks | borderline |
| AWS CloudFront | 1 TB egress + 10M req / mo | Heavily depends on image traffic | wide range |
| AWS Rekognition (moderation) | first 5000 images / mo free | ~2000 demo (one-time) + new artworks (low) | 2× |

**Watch items:**
1. **S3 + CloudFront** as image traffic grows — bandwidth is the real risk
2. **Anthropic / Jina embeddings** — pay-per-call, no free tier
3. **Neon compute-hours, not storage.** The free tier autosuspends when
   idle; anything that keeps the endpoint continuously awake (e.g. a 24/7
   crawler, a health-check loop) burns the monthly compute-hour allowance
   regardless of how little data we hold. Keep the endpoint *able* to idle.
   See `decisions.md` 2026-07-21.

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

### 4. Rate limiting and bot management at the edge

Rate limit per IP + per anon_id, 60/min on search. Doubly important for
cost: a runaway scraper hitting `/search` 10,000 times costs us 10,000
embedding API calls if the query cache doesn't catch them.

**Per-IP rate limiting is necessary but not sufficient** (learned the hard
way — see `decisions.md` 2026-07-21). Two gaps it doesn't cover:

- **IP-rotating crawlers.** PetalBot spreads a steady crawl across
  `114.119.0.0/16`, staying under any per-IP threshold while never letting
  the Neon endpoint idle into autosuspend. Mitigation: a WAF UA block
  (`BlockAbusiveBots` rule, `infra/modules/web/main.tf`) — enforced, not
  advisory.
- **Unbounded URL spaces.** `/search` takes faceted params
  (`?artist=&location=&map=&bbox=`) over a huge key space, so a crawler can
  enumerate effectively infinite distinct URLs — each a cache miss, an SSR
  render, and a ~5-call API fan-out. Mitigations: `robots.txt`
  (`web/src/app/robots.ts`) disallowing `/search`; prefer indexing
  canonical content routes over faceted URLs; drop bot events before they
  hit the `events` table (`core::events::emit`).

**Rule of thumb:** any anonymous, param-driven route ships with
`robots.txt` coverage from day one. Treat "is there an un-`robots`'d
infinite URL space?" as a launch-readiness question, not an incident.

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
- [x] CloudWatch metric: `lambda_invocations` per route group — alarm at
      **10k/day** (pre-launch; the original 100k missed the 2026-07 crawl
      which peaked at ~15k). `lambda_daily_invocations`,
      `infra/modules/observability`. **Raise at launch** (see re-eval
      triggers below).
- [ ] CloudWatch metric: `cloudfront_bytes_downloaded`; alarm at 500 GB/mo
- [x] `robots.txt` covers every anonymous, param-driven route (esp.
      `/search`) — `web/src/app/robots.ts` (2026-07-21)
- [x] WAF UA block for crawlers that ignore robots / rotate IPs —
      `BlockAbusiveBots`, `infra/modules/web/main.tf` (2026-07-21)
- [ ] No un-`robots`'d route exposes an unbounded URL space (faceted
      params over UUIDs / bboxes / free-text)
- [ ] Neon: confirm the endpoint can still autosuspend (no 24/7 traffic
      source, no render-path health-check loop keeping it awake)

## Decommissioning costs (the easy ones to forget)

Things that cost money even at zero traffic:

- Route53 hosted zone: $0.50/mo per zone
- Domain registration: ~$10–30/yr
- ACM certs are free
- CloudFront distributions are free at rest
- S3 storage is per-GB-month (cheap, but compounds)
- Neon non-free plans bill for compute hours even when idle (their free
  tier scales to zero — stay on it as long as possible)

## v1 estimated monthly burn (post-deploy, idle traffic)

Tallied during the `infra/` scaffold (2026-06-10). Numbers are
list-price; free-tier credits where applicable have been subtracted.

**AWS — what's in `infra/`:**

| Item | $/mo | Notes |
|---|---|---|
| WAF (api + web distributions) | ~13 | Two ACLs ($5 each) + managed rules + rules. Fixed floor, traffic-independent — largest single line at idle. Confirmed ~$9.28 (Jun) / $8.23 (Jul MTD) in cost explorer; the earlier ~$6 estimate counted only the api ACL. |
| Route53 hosted zone | 0.50 | Flat per zone. Queries free at our scale. |
| Lambda (web + api + jobs) | 0 | 1M req + 400k GB-s free tier covers idle. |
| S3 (artworks, uploads, web-assets, tfstate) | <1 | $0.023/GB; ~10 GB. |
| CloudFront (web + api + images) | 0 → ~5 | 1 TB egress free year 1; then $0.085/GB. |
| SQS | 0 | 1M req/mo free forever. |
| SSM Parameter Store | 0 | Standard params are free. |
| CloudWatch Logs | 0–2 | 5 GB free, then $0.50/GB. Three log groups. |
| DynamoDB (TF state lock) | 0 | A few ops per `apply`. |
| AWS Budgets | 0 | First 2 budgets free. |
| ACM certs | 0 | Free. |
| **AWS subtotal** | **~$10** | Under the $20 AWS Budgets cap. |

**Off-AWS (paid via separate accounts):**

| Item | $/mo | Notes |
|---|---|---|
| Neon Launch | 0 → 19 | Free tier (0.5 GB, autosuspend) fits v1 traffic; $19 buys 10 GB + no suspend. |
| Jina embeddings | 1–5 | Per-call. Query cache (mig. 0008) keeps this small. |
| Anthropic / OpenAI | 5–20 | Only during studio onboarding flows. |
| Mapbox / Resend / Clerk / PostHog | 0 | All within free tiers (see audit above). |
| Domain (wander.gallery) | ~3 | ~$30–40/yr amortized. |
| **External subtotal** | **~$10–45** | Range is mostly Neon Launch and LLM use. |

**v1 grand total: ~$20–55/mo** at idle, depending on Neon tier and LLM use.

**Why all-AWS vs Vercel:** Vercel Pro adds a fixed $20/mo floor. The
all-AWS shape replaces it with ~$5–10 of CloudFront + Lambda + WAF
deltas for the web tier — meaningfully cheaper at v1, and the cost
curve is also smoother as traffic grows (Vercel function execution +
bandwidth pricing has steeper steps). See `decisions.md` 2026-05-24.

## Re-evaluation triggers

Revisit this doc when any of the following hold:

- We onboard 50+ real artists (S3 / CloudFront usage will jump)
- Search traffic exceeds 1k requests / day
- Any paid service shows up on a budget alarm
- We start paying for a service that was previously free
- **At launch:** raise the `lambda_daily_invocations` alarm threshold
  (10k → real-traffic level) so genuine user load doesn't page us. The
  10k value is a pre-launch, zero-user tripwire only.
