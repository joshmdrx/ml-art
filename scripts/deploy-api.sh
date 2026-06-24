#!/usr/bin/env bash
#
# Build api-search via cargo-lambda and publish to AWS Lambda.
#
# Prereqs (one-time):
#   - cargo-lambda installed (see infra/POST_DEPLOY.md)
#   - AWS CLI configured (we use the `ml-art` SSO profile by default)
#   - `aws sso login --profile ml-art` if the SSO token expired
#
# Usage:
#   scripts/deploy-api.sh            # builds + uploads + waits for active
#   scripts/deploy-api.sh --check    # cargo lambda build only, no upload
#
# What it does:
#   1. cargo lambda build --release --arm64 -p api-search
#      → produces target/lambda/api-search/bootstrap (statically linked)
#   2. zips the bootstrap into a deploy package
#   3. aws lambda update-function-code → swaps the placeholder Node 20
#      zip for the Rust binary
#   4. aws lambda wait function-updated → blocks until the new code is
#      active (avoids racing a smoke-test against a still-deploying fn)
#   5. (optional) hits the live URL once and prints status code
#
# The Function URL config + IAM role + APIG wiring are already set in TF.
# This script only swaps code, never config — if you need to bump
# memory_size or timeout, edit modules/api/main.tf and `terraform apply`.

set -euo pipefail

PROFILE="${AWS_PROFILE:-ml-art}"
REGION="${AWS_REGION:-us-east-1}"
FUNCTION_NAME="${API_FUNCTION_NAME:-ml-art-prod-api}"
WEB_URL="https://api.wander.gallery"

# `--check` short-circuits before any AWS call. Useful in CI as a
# build-only verification step.
DO_UPLOAD=1
if [[ "${1:-}" == "--check" ]]; then
  DO_UPLOAD=0
fi

if ! command -v cargo-lambda >/dev/null 2>&1; then
  echo "✘ cargo-lambda not installed. See infra/POST_DEPLOY.md." >&2
  exit 1
fi

# Switch to api/ — cargo-lambda needs the workspace as cwd.
cd "$(dirname "$0")/../api"

echo "▶ cargo lambda build --release --arm64 -p api-search"
cargo lambda build --release --arm64 -p api-search

ARTIFACT="target/lambda/api-search/bootstrap"
if [[ ! -f "$ARTIFACT" ]]; then
  echo "✘ expected build artifact at $ARTIFACT — did cargo lambda fail silently?" >&2
  exit 1
fi
ARTIFACT_SIZE=$(stat -f%z "$ARTIFACT" 2>/dev/null || stat -c%s "$ARTIFACT")
echo "  bootstrap = $ARTIFACT  ($ARTIFACT_SIZE bytes)"

if [[ "$DO_UPLOAD" == "0" ]]; then
  echo "▶ --check mode; skipping upload."
  exit 0
fi

# Zip ONLY the bootstrap binary. cargo lambda places the zip in
# target/lambda/api-search/bootstrap.zip on `cargo lambda deploy`,
# but we're using update-function-code directly so we build our own.
ZIP_PATH="$(mktemp -d)/api-search.zip"
(cd "$(dirname "$ARTIFACT")" && zip -q "$ZIP_PATH" bootstrap)
echo "▶ zip = $ZIP_PATH"

# Lambda only needs the new code; runtime + handler + arch are already
# set on the function and aren't touched by update-function-code.
#
# `--publish` cuts a new version, which is good for rollback hygiene
# (we can always re-point the alias at the previous version if a
# deploy breaks the world).
echo "▶ aws lambda update-function-code → $FUNCTION_NAME"
NEW_VERSION=$(aws lambda update-function-code \
  --profile "$PROFILE" \
  --region "$REGION" \
  --function-name "$FUNCTION_NAME" \
  --zip-file "fileb://$ZIP_PATH" \
  --publish \
  --query 'Version' --output text)
echo "  → published version $NEW_VERSION"

# Wait for the new code to become active. Without this we can race
# the smoke-test against a function still in `Pending` state.
echo "▶ waiting for function to become active…"
aws lambda wait function-updated \
  --profile "$PROFILE" \
  --region "$REGION" \
  --function-name "$FUNCTION_NAME"
echo "  ✔ active"

# Quick smoke-test.
echo "▶ smoke-test: GET $WEB_URL/v1/health"
HTTP_CODE=$(curl -sS -m 15 -o /tmp/api-smoke.out -w "%{http_code}" "$WEB_URL/v1/health" || echo "000")
echo "  ← HTTP $HTTP_CODE"
echo "  body:"
sed 's/^/    /' /tmp/api-smoke.out
echo ""

if [[ "$HTTP_CODE" != "200" ]]; then
  echo "✘ smoke-test failed. Check CloudWatch logs:" >&2
  echo "    aws --profile $PROFILE --region $REGION logs tail /aws/lambda/$FUNCTION_NAME --since 5m" >&2
  exit 1
fi

# Full prod smoke suite (T-075) — read-only curl assertions across api +
# web + OG + CDN. ~5s. Catches more than the single-endpoint health
# check above (e.g. dependent-route 404s, body fingerprint regressions).
# Skip by setting SKIP_SMOKE=1 in env for an explicit "I'll smoke
# manually" deploy.
if [[ "${SKIP_SMOKE:-0}" != "1" ]]; then
  echo ""
  echo "▶ running prod smoke suite"
  "$(dirname "$0")/smoke-prod.sh"
else
  echo "▶ SKIP_SMOKE=1 — skipping prod smoke suite"
fi

echo "✔ deploy complete."
