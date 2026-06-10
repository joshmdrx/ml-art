# Post-deploy runbook

`terraform apply` ships the infra shape — Lambdas with placeholder
code, empty SSM parameters, empty S3 buckets. This doc covers what
needs to happen ONCE per environment, by hand, before real traffic
can flow.

Order matters: each step blocks the next. If you do them out of order
the symptoms aren't always obvious — write the env var, then watch the
relevant CloudWatch log group for the next thing that breaks.

## 0. Prereqs (one-time, per developer machine)

```sh
# AWS SSO setup (if you haven't already)
aws configure sso  # use the profile name "ml-art"
aws sso login --profile ml-art

# cargo-lambda for cross-compiling Rust to Linux ARM64.
# Either of these works on macOS:
brew install cargo-lambda      # the easy path
# OR:
pip3 install cargo-lambda      # the universal path

# Verify
cargo lambda --version

# Zig is bundled with cargo-lambda from Homebrew, but the pip3 path
# may need it separately:
brew install zig
```

## 1. Create the Neon project + capture connection string

Neon is managed out-of-band per `decisions.md` 2026-05-24 (the rest
of the cost surface). Free tier is fine for v1 (0.5 GB, autosuspend).

1. Sign up at <https://neon.tech> with the same email as the AWS
   account if possible (one fewer mailbox to monitor).
2. Create a project: `ml-art-prod`, region `us-east-1` (same as the
   Lambdas — minimises latency).
3. Enable pgvector under "Database → Extensions" — search migrations
   assume it's present.
4. Copy the pooled connection string. It looks like:
   `postgres://<user>:<pass>@ep-xxxxx-pooler.us-east-1.aws.neon.tech/neondb?sslmode=require`
5. Apply our migrations from your laptop:
   ```sh
   DATABASE_URL="<paste-the-string>" psql -f db/migrations/0001_v1.sql
   # …and 0002 through 0014, in order. Or use `make migrate`-style
   # tooling if you've wired Neon into it.
   ```

## 2. Populate SSM secrets

The TF apply creates 9 SecureString parameters with placeholder values.
Each Lambda reads them on cold start; setting them is the gate between
"infra is up" and "real responses." `lifecycle.ignore_changes` on
each parameter means subsequent TF applies *won't* revert your values.

Tip: keep a private 1Password / Bitwarden entry titled `ml-art prod
SSM` mirroring this list. Rotation = update both places.

```sh
PREFIX=$(cd infra && terraform output -raw ssm_parameter_path_prefix)
# → "/ml-art-prod/"
P() { aws --profile ml-art ssm put-parameter --type SecureString --overwrite \
       --name "${PREFIX}$1" --value "$2"; }

P database_url      'postgres://...neon...'              # from step 1
P jina_api_key      'jina_xxxxxxxxxxxxxxxxxxxxxx'       # https://jina.ai
P clerk_secret_key  'sk_live_xxxxxxxxxx'                # Clerk dashboard
P clerk_issuer      'https://clerk.wander.gallery'      # Clerk dashboard
P clerk_jwks_url    'https://clerk.wander.gallery/.well-known/jwks.json'
P resend_api_key    're_xxxxxxxxxx'                     # https://resend.com
P resend_from_email 'inquiries@wander.gallery'          # must be verified domain in Resend
P mapbox_token      'pk.xxxxxxxxxx'                     # https://account.mapbox.com
P web_base_url      'https://wander.gallery'            # apex; used in email links
```

Verify:
```sh
aws --profile ml-art ssm get-parameters-by-path \
  --path "/ml-art-prod/" --recursive --with-decryption \
  --query 'Parameters[*].[Name,Value]' --output table
```

## 3. Deploy the Rust lambdas

```sh
make deploy-api    # builds api-search via cargo-lambda, uploads, smoke-tests
make deploy-jobs   # same for jobs-lambda
```

Each script:
1. `cargo lambda build --release --arm64 -p <crate>`
2. zips the `bootstrap` binary
3. `aws lambda update-function-code --publish`
4. `aws lambda wait function-updated`
5. (api only) curls `https://api.wander.gallery/v1/health` and prints status

If `make deploy-api` succeeds and the smoke-test returns 200 you're
through the riskiest infra-to-code seam. If it 500s, check logs:

```sh
aws --profile ml-art logs tail /aws/lambda/ml-art-prod-api --since 5m
```

Common first failures:
- `Config::load: missing DATABASE_URL` → step 2 not run, or wrong key
- `failed to connect to Postgres` → Neon project paused (auto-suspends
  after inactivity on the free tier; first cold-start request wakes it)
- `JWKS fetch failed` → wrong `clerk_jwks_url`; check the Clerk dashboard

## 4. Deploy the Next.js app

The web Lambda starts on the Node 20 placeholder. To deploy the real
Next.js app, install OpenNext as a dev dep in `web/`, then run
`make deploy-web`.

```sh
# One-time, in web/:
cd web
pnpm add -D @opennextjs/aws

# Add an open-next.config.ts at the project root (web/), minimal form:
cat > open-next.config.ts <<'EOF'
import type { OpenNextConfig } from "@opennextjs/aws/types/open-next";

const config: OpenNextConfig = {
  // Defaults are fine for v1 — single server lambda, no ISR cache yet.
  default: {},
};
export default config;
EOF

# Verify the build works locally:
pnpm exec opennextjs-aws build
# → produces web/.open-next/{server-function,assets,…}
```

**Next.js 16 caveat:** the web app uses Next 16.2.x; OpenNext's
official support matrix may not include 16 yet (16 GA'd late in
the cycle). If `opennextjs-aws build` fails:
1. Try the latest OpenNext (`@opennextjs/aws@beta` or `@opennextjs/aws@canary`).
2. If still broken, **pin the web app to Next 15.x** by editing
   `web/package.json` — Next 15 → 16 has no userland-breaking changes
   for our app, and OpenNext 3.5+ fully supports 15.
3. Worst case, file an upstream issue; the placeholder will keep
   serving until a fix lands.

Once OpenNext builds clean:

```sh
make deploy-web
```

Which runs `scripts/deploy-web.sh` — same shape as `deploy-api.sh`
but with three steps:
1. `pnpm exec opennextjs-aws build` → `.open-next/`
2. zip `server-function/` → `aws lambda update-function-code --publish`
3. `aws s3 sync .open-next/assets/ s3://ml-art-prod-web-assets/ --delete`
4. `aws cloudfront create-invalidation` on `/`, `/index.html`, `/api/*`
   (content-hashed `/_next/static/*` never needs invalidation)
5. Smoke-test `https://wander.gallery`

The TF outputs already expose the names needed; the script reads them
from env or falls back to the prod literals:
- `web_server_lambda_name` = `ml-art-prod-web`
- `web_assets_bucket_name` = `ml-art-prod-web-assets`
- `web_cloudfront_distribution_id` = `ERMPJ0JZ75NWL`

## 5. Seed the artworks bucket (optional, for demo)

The `images.wander.gallery` distribution serves the artwork images.
Until something's in S3, every image URL 403s — which is fine while
working on the API surface, but obviously needed before launch.

```sh
# Single image upload (validate the pipeline):
aws --profile ml-art s3 cp test.jpg s3://ml-art-prod-artworks/artworks/test/test.jpg
curl -I https://images.wander.gallery/artworks/test/test.jpg
# → expect 200, Content-Type: image/jpeg

# Bulk re-seed from local WikiArt corpus:
# (TODO: write `scripts/sync-artworks-to-s3.sh` that mirrors
#  `ml/spikes/.../data/wikiart/` → s3://ml-art-prod-artworks/artworks/)
```

## 6. Smoke-test end-to-end

Once all the above is done:

```sh
# 1. API health
curl https://api.wander.gallery/v1/health
#   → { "status": "ok", "version": "..." }

# 2. API search (no auth required for read paths)
curl 'https://api.wander.gallery/v1/search?q=ocean'
#   → first call: ~2-5s (cold start + Jina embedding); next call: <1s

# 3. Enqueue a no-op job + watch it execute
QUEUE_URL=$(cd infra && terraform output -raw jobs_queue_url)
aws --profile ml-art sqs send-message --queue-url "$QUEUE_URL" \
  --message-body '{"kind":"artist_location_geocode","payload":{"location_id":"00000000-0000-0000-0000-000000000000"}}'
aws --profile ml-art logs tail /aws/lambda/ml-art-prod-jobs --since 1m --follow
#   → expect "handler error: location not found" (the UUID is bogus,
#     but the handler ran — which is what we're verifying)

# 4. Web app (after step 4 above)
open https://wander.gallery
```

## Rotation hygiene

- **Cloudflare API token** — used at TF plan/apply time only. Rotate at
  <https://dash.cloudflare.com/profile/api-tokens> on any suspicion.
  Required scopes: `Zone:Read` + `DNS:Edit` on `wander.gallery` only.
- **AWS SSO** — expires after ~12h by default. `aws sso login --profile ml-art`
  to refresh.
- **SSM secrets** — rotate by setting the new value, then redeploying the
  Lambdas (they only read on cold start; warm containers hold the old
  value). `make deploy-api && make deploy-jobs` is the bounce.
- **Anthropic / Jina / Resend / Mapbox / Clerk** — provider dashboards;
  rotate per their UI then update SSM + redeploy.

## When something's wrong

```sh
# Live logs (most useful):
aws --profile ml-art logs tail /aws/lambda/ml-art-prod-api  --since 10m --follow
aws --profile ml-art logs tail /aws/lambda/ml-art-prod-jobs --since 10m --follow
aws --profile ml-art logs tail /aws/lambda/ml-art-prod-web  --since 10m --follow

# What's in flight on SQS:
aws --profile ml-art sqs get-queue-attributes \
  --queue-url "$(cd infra && terraform output -raw jobs_queue_url)" \
  --attribute-names ApproximateNumberOfMessages ApproximateNumberOfMessagesNotVisible

# What's stuck in the DLQ:
DLQ_URL=$(aws --profile ml-art sqs list-queues --queue-name-prefix ml-art-prod-jobs-dlq --query 'QueueUrls[0]' --output text)
aws --profile ml-art sqs receive-message --queue-url "$DLQ_URL" --max-number-of-messages 10

# Force-invalidate CloudFront after a deploy that didn't take:
aws --profile ml-art cloudfront create-invalidation \
  --distribution-id "$(cd infra && terraform output -raw web_cloudfront_distribution_id)" \
  --paths '/*'

# Spend check (does the AWS bill match COST.md's estimate?):
aws --profile ml-art ce get-cost-and-usage \
  --time-period Start=$(date -u -v-7d +%Y-%m-%d),End=$(date -u +%Y-%m-%d) \
  --granularity DAILY --metrics UnblendedCost \
  --group-by Type=DIMENSION,Key=SERVICE
```
