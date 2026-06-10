#!/usr/bin/env bash
#
# Build jobs-lambda via cargo-lambda and publish to AWS Lambda.
#
# Counterpart to scripts/deploy-api.sh. Same shape — different
# function name + no HTTP smoke-test (jobs-lambda is SQS-triggered;
# verification is "enqueue a JobEvent + watch CloudWatch logs").
#
# Usage:
#   scripts/deploy-jobs.sh          # build + upload + wait
#   scripts/deploy-jobs.sh --check  # build only

set -euo pipefail

PROFILE="${AWS_PROFILE:-ml-art}"
REGION="${AWS_REGION:-us-east-1}"
FUNCTION_NAME="${JOBS_FUNCTION_NAME:-ml-art-prod-jobs}"

DO_UPLOAD=1
if [[ "${1:-}" == "--check" ]]; then
  DO_UPLOAD=0
fi

if ! command -v cargo-lambda >/dev/null 2>&1; then
  echo "✘ cargo-lambda not installed. See infra/POST_DEPLOY.md." >&2
  exit 1
fi

cd "$(dirname "$0")/../api"

echo "▶ cargo lambda build --release --arm64 -p jobs-lambda"
cargo lambda build --release --arm64 -p jobs-lambda

# jobs-lambda's bin name is `bootstrap` (set in its Cargo.toml), so
# cargo-lambda writes the artifact straight into target/lambda/jobs-lambda/bootstrap
# without renaming.
ARTIFACT="target/lambda/jobs-lambda/bootstrap"
if [[ ! -f "$ARTIFACT" ]]; then
  echo "✘ expected build artifact at $ARTIFACT" >&2
  exit 1
fi
ARTIFACT_SIZE=$(stat -f%z "$ARTIFACT" 2>/dev/null || stat -c%s "$ARTIFACT")
echo "  bootstrap = $ARTIFACT  ($ARTIFACT_SIZE bytes)"

if [[ "$DO_UPLOAD" == "0" ]]; then
  echo "▶ --check mode; skipping upload."
  exit 0
fi

ZIP_PATH="$(mktemp -d)/jobs-lambda.zip"
(cd "$(dirname "$ARTIFACT")" && zip -q "$ZIP_PATH" bootstrap)
echo "▶ zip = $ZIP_PATH"

echo "▶ aws lambda update-function-code → $FUNCTION_NAME"
NEW_VERSION=$(aws lambda update-function-code \
  --profile "$PROFILE" \
  --region "$REGION" \
  --function-name "$FUNCTION_NAME" \
  --zip-file "fileb://$ZIP_PATH" \
  --publish \
  --query 'Version' --output text)
echo "  → published version $NEW_VERSION"

echo "▶ waiting for function to become active…"
aws lambda wait function-updated \
  --profile "$PROFILE" \
  --region "$REGION" \
  --function-name "$FUNCTION_NAME"
echo "  ✔ active"

echo "✔ deploy complete."
echo ""
echo "  To verify, enqueue a no-op event onto the SQS queue and tail CloudWatch:"
echo "    aws --profile $PROFILE sqs send-message \\"
echo "      --queue-url \"\$(cd infra && terraform output -raw jobs_queue_url)\" \\"
echo "      --message-body '{\"kind\":\"artist_location_geocode\",\"payload\":{\"location_id\":\"00000000-0000-0000-0000-000000000000\"}}'"
echo "    aws --profile $PROFILE logs tail /aws/lambda/$FUNCTION_NAME --since 1m --follow"
