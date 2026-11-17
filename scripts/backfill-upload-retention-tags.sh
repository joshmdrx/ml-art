#!/usr/bin/env bash
#
# T-089 — one-off backfill of `state` retention tags on the uploads bucket.
#
# Background: the uploads bucket's lifecycle rule used to expire EVERY
# object after 90 days, which deleted artists' artwork images (they share
# the bucket with throwaway visual-search uploads). The fixed rule only
# expires objects tagged `state=pending`; the api tags new uploads
# `pending` and retags them `attached` on artwork attach. Objects
# uploaded before T-089 carry no tag at all, so this script:
#
#   Pass 1 — tags every key referenced by `artwork_images` (uploads/
#            prefix) as `state=attached`. Includes images on soft-deleted
#            artworks (they can be restored). Keys already gone from S3
#            are reported as MISSING — those images were lost to the old
#            rule and need re-uploading by the artist.
#   Pass 2 — (only with --expire-orphans) tags every UNTAGGED, unreferenced
#            object under uploads/ as `state=pending` so old visual-search
#            uploads age out too. Objects that already carry a `state` tag
#            are left alone, so anything the new api wrote wins. Run this
#            ONLY after the T-089 api is deployed: an artist attaching an
#            untagged upload via the old api, after this pass tagged it
#            pending, would lose that image. Older-than-90-day orphans are
#            deleted by the lifecycle rule within ~a day of tagging.
#
# Then re-reads the tag on every referenced key and reports any that are
# not `attached`.
#
# Idempotent: re-running retags the same keys with the same value.
#
# Usage:
#   AWS_PROFILE=ml-art scripts/backfill-upload-retention-tags.sh                          # dry-run
#   AWS_PROFILE=ml-art scripts/backfill-upload-retention-tags.sh --apply                  # pass 1
#   AWS_PROFILE=ml-art scripts/backfill-upload-retention-tags.sh --apply --expire-orphans # pass 1 + 2
#
# Overrides: DATABASE_URL (else read from SSM /ml-art-prod/database_url),
# UPLOADS_BUCKET (default ml-art-prod-uploads), AWS_REGION (us-east-1).
#
# Needs: aws CLI, psql. The caller's AWS identity needs s3:ListBucket,
# s3:GetObjectTagging and s3:PutObjectTagging on the bucket.

set -euo pipefail

APPLY=0
EXPIRE_ORPHANS=0
for arg in "$@"; do
  case "$arg" in
    --apply) APPLY=1 ;;
    --expire-orphans) EXPIRE_ORPHANS=1 ;;
    *) echo "✘ unknown argument: $arg" >&2; exit 2 ;;
  esac
done

PROFILE="${AWS_PROFILE:-ml-art}"
REGION="${AWS_REGION:-us-east-1}"
BUCKET="${UPLOADS_BUCKET:-ml-art-prod-uploads}"
AWS=(aws --profile "$PROFILE" --region "$REGION")

if [[ -z "${DATABASE_URL:-}" ]]; then
  DATABASE_URL="$("${AWS[@]}" ssm get-parameter \
    --name /ml-art-prod/database_url --with-decryption \
    --query 'Parameter.Value' --output text)"
fi
if [[ -z "$DATABASE_URL" ]]; then
  echo "✘ DATABASE_URL is empty — set AWS_PROFILE or pass DATABASE_URL=… explicitly" >&2
  exit 1
fi

WORK="$(mktemp -d)"
trap 'rm -rf "$WORK"' EXIT

echo "▶ T-089 retention-tag backfill"
echo "  bucket  = $BUCKET"
echo "  db host = $(sed -E 's#^[^@]*@([^/:?]+).*#\1#' <<<"$DATABASE_URL")"
echo "  mode    = $([[ "$APPLY" == "1" ]] && echo "APPLY" || echo "DRY-RUN (pass --apply to write tags)")"
echo "  orphans = $([[ "$EXPIRE_ORPHANS" == "1" ]] && echo "tag untagged unreferenced objects pending" || echo "left untouched")"
echo

# Every key an artwork_images row points at in the uploads bucket.
psql "$DATABASE_URL" -v ON_ERROR_STOP=1 -At -c \
  "SELECT DISTINCT s3_key FROM artwork_images WHERE s3_key LIKE 'uploads/%' ORDER BY 1" \
  >"$WORK/referenced.txt"
echo "  referenced keys: $(wc -l <"$WORK/referenced.txt" | tr -d ' ')"

# Write `state=<value>` on one key. Echoes ok | missing; exits non-zero on
# anything other than NoSuchKey.
tag_key() {
  local key="$1" value="$2" err
  if [[ "$APPLY" != "1" ]]; then
    echo "would-tag"
    return 0
  fi
  if err="$("${AWS[@]}" s3api put-object-tagging --bucket "$BUCKET" --key "$key" \
    --tagging "TagSet=[{Key=state,Value=$value}]" 2>&1 >/dev/null)"; then
    echo "ok"
  elif grep -q 'NoSuchKey' <<<"$err"; then
    echo "missing"
  else
    echo "✘ tagging $key failed: $err" >&2
    return 1
  fi
}

# Current `state` tag on one key ("" if untagged, "MISSING" if gone).
read_state() {
  local key="$1" out
  if out="$("${AWS[@]}" s3api get-object-tagging --bucket "$BUCKET" --key "$key" \
    --query "TagSet[?Key=='state'].Value | [0]" --output text 2>&1)"; then
    [[ "$out" == "None" ]] && out=""
    echo "$out"
  elif grep -q 'NoSuchKey' <<<"$out"; then
    echo "MISSING"
  else
    echo "✘ reading tags on $key failed: $out" >&2
    return 1
  fi
}

# ── Pass 1: referenced → attached ───────────────────────────────────────────
echo
echo "── Pass 1: artwork_images keys → state=attached"
missing=0
while IFS= read -r key; do
  [[ -z "$key" ]] && continue
  result="$(tag_key "$key" attached)"
  [[ "$result" == "missing" ]] && missing=$((missing + 1))
  printf '  %-9s %s\n' "$result" "$key"
done <"$WORK/referenced.txt"

# ── Pass 2: untagged orphans → pending ──────────────────────────────────────
if [[ "$EXPIRE_ORPHANS" == "1" ]]; then
  echo
  echo "── Pass 2: untagged, unreferenced uploads/ objects → state=pending"
  "${AWS[@]}" s3api list-objects-v2 --bucket "$BUCKET" --prefix uploads/ \
    --query 'Contents[].Key' --output text >"$WORK/listing.txt"
  tr '\t' '\n' <"$WORK/listing.txt" | { grep -v -e '^None$' -e '^$' || true; } | sort -u >"$WORK/all.txt"
  sort -u "$WORK/referenced.txt" >"$WORK/referenced.sorted"
  comm -23 "$WORK/all.txt" "$WORK/referenced.sorted" >"$WORK/orphans.txt"
  echo "  objects under uploads/: $(wc -l <"$WORK/all.txt" | tr -d ' '), unreferenced: $(wc -l <"$WORK/orphans.txt" | tr -d ' ')"
  while IFS= read -r key; do
    [[ -z "$key" ]] && continue
    current="$(read_state "$key")"
    if [[ -n "$current" ]]; then
      printf '  %-9s %s (already state=%s)\n' "skip" "$key" "$current"
      continue
    fi
    result="$(tag_key "$key" pending)"
    printf '  %-9s %s\n' "$result" "$key"
  done <"$WORK/orphans.txt"
fi

# ── Verify ──────────────────────────────────────────────────────────────────
echo
echo "── Verify: every referenced key reads state=attached"
bad=0
while IFS= read -r key; do
  [[ -z "$key" ]] && continue
  state="$(read_state "$key")"
  if [[ "$state" != "attached" ]]; then
    bad=$((bad + 1))
    printf '  %-9s %s\n' "${state:-untagged}" "$key"
  fi
done <"$WORK/referenced.txt"

echo
echo "  missing from S3 (already lost): $missing"
echo "  referenced but not attached:    $bad"
if [[ "$APPLY" == "1" && "$bad" -gt "$missing" ]]; then
  echo "✘ some existing referenced objects are not tagged attached — investigate before the lifecycle change takes effect" >&2
  exit 1
fi
echo "✔ done"
