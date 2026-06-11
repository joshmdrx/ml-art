#!/usr/bin/env bash
#
# Build the Next.js app with OpenNext and publish to AWS.
#
# Three things move on every deploy:
#   1. The server Lambda's code (SSR + RSC handler bundle)
#   2. The static assets in S3 (.next/static + public/, hashed filenames)
#   3. A CloudFront invalidation so the CDN picks up the new HTML
#
# Prereqs (one-time per machine):
#   - pnpm install in web/  (regular dev setup)
#   - pnpm add -D @opennextjs/aws  in web/  (see infra/POST_DEPLOY.md
#     for the Next 16 compatibility caveat)
#   - AWS CLI configured + `aws sso login --profile ml-art`
#
# Usage:
#   scripts/deploy-web.sh           # full build + upload + invalidate
#   scripts/deploy-web.sh --check   # OpenNext build only, no upload
#
# Build output (per OpenNext convention):
#   web/.open-next/
#     ├── server-function/   ← zipped to update web Lambda
#     ├── assets/            ← synced to s3://ml-art-prod-web-assets/
#     ├── cache/             ← ISR cache (not used in v1)
#     └── image-optimization-function/  ← Phase 2 — TODO

set -euo pipefail

PROFILE="${AWS_PROFILE:-ml-art}"
REGION="${AWS_REGION:-us-east-1}"
FUNCTION_NAME="${WEB_FUNCTION_NAME:-ml-art-prod-web}"
ASSETS_BUCKET="${WEB_ASSETS_BUCKET:-ml-art-prod-web-assets}"
DISTRIBUTION_ID="${WEB_DISTRIBUTION_ID:-ERMPJ0JZ75NWL}"  # web CloudFront distro
WEB_URL="https://wander.gallery"

DO_UPLOAD=1
if [[ "${1:-}" == "--check" ]]; then
  DO_UPLOAD=0
fi

# Switch to web/ — OpenNext expects the Next.js project as cwd.
cd "$(dirname "$0")/../web"

if [[ ! -d node_modules ]]; then
  echo "✘ node_modules missing — run \`pnpm install\` first." >&2
  exit 1
fi

# pnpm sniffs whether opennextjs-aws is installed by looking for it
# in node_modules/. Cheaper than parsing package.json.
if [[ ! -d node_modules/@opennextjs ]]; then
  echo "✘ @opennextjs/aws not installed. See infra/POST_DEPLOY.md → 'Deploy the web app'." >&2
  exit 1
fi

echo "▶ pnpm exec open-next build"
# Node 23 (matches pnpm >= v11.3 requirement). The OpenNext build
# shells out to `pnpm build` internally — Node version must satisfy
# both the workspace's pnpm and OpenNext's own scripts.
PATH=/opt/homebrew/opt/node/bin:$PATH pnpm exec open-next build

SERVER_DIR=".open-next/server-functions/default"
ASSETS_DIR=".open-next/assets"

if [[ ! -d "$SERVER_DIR" ]]; then
  echo "✘ OpenNext build produced no $SERVER_DIR — investigate output above." >&2
  exit 1
fi

if [[ "$DO_UPLOAD" == "0" ]]; then
  echo "▶ --check mode; skipping upload."
  exit 0
fi

# ─── 1. Server Lambda ─────────────────────────────────────────────────────
# OpenNext puts the handler at server-function/index.mjs. Zip the
# whole directory contents (preserving node_modules).
ZIP_PATH="$(mktemp -d)/web-server.zip"
echo "▶ zipping $SERVER_DIR → $ZIP_PATH"
(cd "$SERVER_DIR" && zip -qr "$ZIP_PATH" .)
ZIP_SIZE=$(stat -f%z "$ZIP_PATH" 2>/dev/null || stat -c%s "$ZIP_PATH")
echo "  $(( ZIP_SIZE / 1024 / 1024 )) MB"

echo "▶ aws lambda update-function-code → $FUNCTION_NAME"
NEW_VERSION=$(aws lambda update-function-code \
  --profile "$PROFILE" \
  --region "$REGION" \
  --function-name "$FUNCTION_NAME" \
  --zip-file "fileb://$ZIP_PATH" \
  --publish \
  --query 'Version' --output text)
echo "  → published version $NEW_VERSION"

# update-function-code returns immediately; the config update below
# will fail with ResourceConflictException if the code update is
# still in progress. Wait first.
aws lambda wait function-updated \
  --profile "$PROFILE" \
  --region "$REGION" \
  --function-name "$FUNCTION_NAME"

# ─── Sync server-side env vars from SSM ───────────────────────────────────
# OpenNext server lambda needs CLERK_SECRET_KEY (server middleware) at
# runtime. NEXT_PUBLIC_* vars are baked into the JS at build time via
# web/.env.production and don't need to be in the Lambda env.
#
# Pulling from SSM at deploy time keeps secrets out of TF state.
# Rotation: update SSM, re-run `make deploy-web`. The TF
# lifecycle.ignore_changes on `environment` means terraform plan
# won't fight whatever the deploy script set.
echo "▶ syncing CLERK_SECRET_KEY from SSM → web Lambda env"
export CLERK_SECRET
CLERK_SECRET=$(aws --profile "$PROFILE" --region "$REGION" ssm get-parameter \
  --name "/ml-art-prod/clerk_secret_key" --with-decryption \
  --query 'Parameter.Value' --output text)

# Merge our env additions on top of whatever else TF set (CONFIG_PARAMETER_PATH,
# NEXT_PUBLIC_API_BASE_URL, IMAGES_CDN_URL). Fetch current, edit, push back.
CURRENT_ENV=$(aws --profile "$PROFILE" --region "$REGION" lambda get-function-configuration \
  --function-name "$FUNCTION_NAME" \
  --query 'Environment.Variables' --output json)
NEW_ENV=$(echo "$CURRENT_ENV" | python3 -c "
import json, sys, os
env = json.load(sys.stdin) or {}
env['CLERK_SECRET_KEY'] = os.environ['CLERK_SECRET']
print(json.dumps({'Variables': env}))
")

aws --profile "$PROFILE" --region "$REGION" lambda update-function-configuration \
  --function-name "$FUNCTION_NAME" \
  --environment "$NEW_ENV" \
  > /dev/null
echo "  ✔ env updated"

echo "▶ waiting for function to become active…"
aws lambda wait function-updated \
  --profile "$PROFILE" \
  --region "$REGION" \
  --function-name "$FUNCTION_NAME"
echo "  ✔ active"

# ─── 2. Static assets ─────────────────────────────────────────────────────
# `--delete` purges old hashed assets — safe because Next.js content-
# hashes everything under /_next/static/, and the previous Lambda
# version is no longer referencing them after step 1.
echo "▶ s3 sync $ASSETS_DIR → s3://$ASSETS_BUCKET/"
aws s3 sync "$ASSETS_DIR" "s3://$ASSETS_BUCKET/" \
  --profile "$PROFILE" \
  --region "$REGION" \
  --delete \
  --cache-control "public, max-age=31536000, immutable"

# ─── 3. CloudFront invalidation ───────────────────────────────────────────
# Only invalidate dynamic paths. /_next/static/* never needs invalidation
# (content-hashed). Invalidating "/*" is wasteful + slow.
echo "▶ cloudfront create-invalidation → $DISTRIBUTION_ID"
INVALIDATION_ID=$(aws cloudfront create-invalidation \
  --profile "$PROFILE" \
  --distribution-id "$DISTRIBUTION_ID" \
  --paths "/" "/index.html" "/api/*" \
  --query 'Invalidation.Id' --output text)
echo "  → invalidation $INVALIDATION_ID submitted (~30-60s to propagate)"

# ─── 4. Smoke test ────────────────────────────────────────────────────────
echo "▶ smoke-test: GET $WEB_URL"
HTTP_CODE=$(curl -sS -m 30 -o /tmp/web-smoke.out -w "%{http_code}" "$WEB_URL" || echo "000")
echo "  ← HTTP $HTTP_CODE"
echo "  body (first 5 lines):"
head -5 /tmp/web-smoke.out | sed 's/^/    /'
echo ""

if [[ "$HTTP_CODE" != "200" ]]; then
  echo "✘ smoke-test failed. Check CloudWatch logs:" >&2
  echo "    aws --profile $PROFILE logs tail /aws/lambda/$FUNCTION_NAME --since 5m" >&2
  exit 1
fi

echo "✔ deploy complete."
