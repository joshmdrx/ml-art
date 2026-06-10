#!/usr/bin/env bash
#
# Mirror the local WikiArt corpus into s3://ml-art-prod-artworks/.
# One-shot; safe to re-run (idempotent — `aws s3 sync` only copies
# what's missing or changed).
#
# Until this runs, `https://images.wander.gallery/artworks/<id>/<file>`
# 403s because the bucket is empty. After running, every demo artwork's
# image URL resolves.
#
# Layout in S3 (matches what `core::images` constructs):
#   artworks/<artwork_id>/<variant>.<ext>
# where variant is `original`, `thumb-256`, `thumb-1024`, etc.
#
# Prereqs:
#   - `make seed` (local) has run at least once, so the WikiArt corpus
#     is present at ml/spikes/2026-05-modifier-deltas/data/wikiart/
#   - AWS SSO active
#
# Usage:
#   scripts/sync-artworks-to-s3.sh             # dry-run summary first
#   scripts/sync-artworks-to-s3.sh --apply     # actually copy
#
# This script INTENTIONALLY does not delete files in the bucket that
# aren't in the local corpus — once an artwork is live we want it
# stable, even if the local seed shrinks.

set -euo pipefail

PROFILE="${AWS_PROFILE:-ml-art}"
REGION="${AWS_REGION:-us-east-1}"
BUCKET="${ARTWORKS_BUCKET:-ml-art-prod-artworks}"
LOCAL_DIR="${ARTWORKS_LOCAL_DIR:-ml/spikes/2026-05-modifier-deltas/data/wikiart}"

DO_APPLY=0
if [[ "${1:-}" == "--apply" ]]; then
  DO_APPLY=1
fi

REPO_ROOT="$(cd "$(dirname "$0")/.." && pwd)"
cd "$REPO_ROOT"

if [[ ! -d "$LOCAL_DIR" ]]; then
  echo "✘ local corpus not found at $LOCAL_DIR" >&2
  echo "  Fetch first: cd ml && uv run python -m ml_art.datasets.wikiart \\" >&2
  echo "    --out spikes/2026-05-modifier-deltas/data/wikiart --per-style 80 --max-total 2000" >&2
  exit 1
fi

LOCAL_COUNT=$(find "$LOCAL_DIR" -type f \( -name '*.jpg' -o -name '*.jpeg' -o -name '*.png' -o -name '*.webp' \) | wc -l | tr -d ' ')
LOCAL_BYTES=$(du -sk "$LOCAL_DIR" | awk '{ print $1 * 1024 }')
echo "▶ local: $LOCAL_COUNT image files, ~$(( LOCAL_BYTES / 1024 / 1024 )) MB"

# Cheap remote-side count via list-objects-v2. For >1k objects this
# requires pagination — `aws s3 ls --summarize --recursive` handles it.
REMOTE_COUNT=$(aws --profile "$PROFILE" --region "$REGION" s3 ls "s3://$BUCKET/artworks/" --recursive --summarize 2>/dev/null | grep 'Total Objects:' | awk '{print $3}' || echo "0")
REMOTE_COUNT="${REMOTE_COUNT:-0}"
echo "▶ remote: $REMOTE_COUNT object(s) currently under s3://$BUCKET/artworks/"

if [[ "$DO_APPLY" == "0" ]]; then
  echo ""
  echo "▶ DRY-RUN — what would be copied:"
  aws --profile "$PROFILE" --region "$REGION" s3 sync "$LOCAL_DIR" "s3://$BUCKET/artworks/" \
    --dryrun \
    --exclude "*" \
    --include "*.jpg" --include "*.jpeg" --include "*.png" --include "*.webp" \
    | head -20
  echo "  …(truncated; re-run with --apply to do it)"
  exit 0
fi

echo ""
echo "▶ syncing $LOCAL_DIR → s3://$BUCKET/artworks/ …"
aws --profile "$PROFILE" --region "$REGION" s3 sync "$LOCAL_DIR" "s3://$BUCKET/artworks/" \
  --exclude "*" \
  --include "*.jpg" --include "*.jpeg" --include "*.png" --include "*.webp" \
  --cache-control "public, max-age=31536000, immutable"

echo "✔ done."
echo ""
echo "  Verify one:"
echo "    curl -I https://images.wander.gallery/artworks/<first-file>"
