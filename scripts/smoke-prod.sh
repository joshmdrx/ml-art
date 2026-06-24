#!/usr/bin/env bash
#
# Production smoke suite (T-075).
#
# Run after a deploy, or any time you want to verify prod is still
# reachable, responding, and serving sane content. Read-path only —
# write-path bugs need authenticated tests and a non-prod fixture
# (see T-069 for that direction).
#
# Run manually:
#
#   make smoke-prod
#   # or directly:
#   scripts/smoke-prod.sh
#
# Auto-runs at the tail of `make deploy-api` and `make deploy-web` so
# bad deploys fail loud at the deploy step instead of in the wild.
#
# Each check hits a public endpoint, asserts the HTTP status, and
# fingerprints the body for content we'd want to see (artist's name on
# a profile page, the canonical site title on the homepage, etc.).
# Failing fingerprints are louder than "5xx is bad" — they catch
# silent regressions where the page loads but the content is wrong.
#
# Adding new checks: drop a `check_*` invocation in the section that
# matches its tier (api / web / images / og). Use `assert_contains` for
# body assertions and `assert_status` for the HTTP code; the helpers
# handle counting + colour + the timing line for you.
#
# What this does NOT catch:
#   - write-path bugs (POST /v1/uploads/image broken; PATCH semantics
#     broken; etc.) — needs auth, fixtures, and rollback after
#   - DB-state drift (the new column doesn't exist yet) — only catches
#     when it causes a 500 on a covered endpoint
#   - mail delivery / job worker / SQS plumbing — needs cron-style
#     synthetic monitoring with a real round-trip
#
# Threshold for "we should add a check": a bug that bit us in prod that
# would have been caught by a 1-second curl + grep before the artist /
# buyer noticed.

set -uo pipefail

API_ORIGIN="${API_ORIGIN:-https://api.wander.gallery}"
WEB_ORIGIN="${WEB_ORIGIN:-https://wander.gallery}"
IMAGES_ORIGIN="${IMAGES_ORIGIN:-https://images.wander.gallery}"

# Known-stable fixtures. Picked from the WikiArt demo seed corpus —
# that data is reseeded deterministically, so individual UUIDs are
# stable AND immune to artist self-deletion (no human owns it). If
# the seed rotation changes, update this and the smoke goes red,
# which is the desired forcing function.
FIXTURE_ARTIST_SLUG="demo-ukiyo-e"
FIXTURE_ARTIST_NAME="Ukiyo E Studio (Demo)"
FIXTURE_ARTWORK_ID="fbc3702b-7dc9-4b3f-a829-eae7f34af73d"
FIXTURE_ARTWORK_TITLE="Untitled (Ukiyo E)"

# Per-request timeout. Generous because cold-start Lambdas can take
# a few seconds on a quiet day.
TIMEOUT_S=15

# ──────────────────────────────────────────────────────────────────────
# Output helpers — colours, counts, body assertions.
# ──────────────────────────────────────────────────────────────────────

if [[ -t 1 ]]; then
  GREEN=$'\033[32m'; RED=$'\033[31m'; DIM=$'\033[2m'; BOLD=$'\033[1m'; RESET=$'\033[0m'
else
  GREEN=""; RED=""; DIM=""; BOLD=""; RESET=""
fi

PASSED=0
FAILED=0
FAIL_LINES=()

# Run one HTTP check. Args:
#   $1 — tier label (api | web | og | img) for output alignment
#   $2 — HTTP method (almost always GET here)
#   $3 — URL
#   $4 — expected HTTP status (e.g. 200)
#   $5 — body substring that MUST appear, or empty string to skip
#
# Side effects: increments PASSED or FAILED. Writes body to /tmp on
# failure so the operator can grep it.
check() {
  local tier="$1" method="$2" url="$3" want_status="$4" want_substring="$5"
  local body_file http_code start_ms end_ms elapsed_ms
  body_file=$(mktemp)
  start_ms=$(python3 -c 'import time; print(int(time.time()*1000))')
  http_code=$(curl -sS -L -m "$TIMEOUT_S" -X "$method" \
    -o "$body_file" -w "%{http_code}" \
    -H "user-agent: ml-art-smoke/1" \
    "$url" 2>/dev/null || echo "000")
  end_ms=$(python3 -c 'import time; print(int(time.time()*1000))')
  elapsed_ms=$((end_ms - start_ms))

  local pretty_url="${url#https://}"
  # Pad tier for alignment.
  local tier_padded
  tier_padded=$(printf "%-3s" "$tier")

  if [[ "$http_code" != "$want_status" ]]; then
    FAILED=$((FAILED + 1))
    FAIL_LINES+=("$tier $method $url — expected $want_status, got $http_code (${elapsed_ms}ms)")
    printf "  %s✘%s %s %s %-50s ${DIM}got %s, want %s · %dms%s\n" \
      "$RED" "$RESET" "$tier_padded" "$method" "$pretty_url" \
      "$http_code" "$want_status" "$elapsed_ms" "$RESET"
    printf "     %sbody (first 240 chars):%s\n" "$DIM" "$RESET"
    head -c 240 "$body_file" | sed 's/^/       /'
    printf "\n"
    rm -f "$body_file"
    return 1
  fi

  if [[ -n "$want_substring" ]] && ! grep -qF "$want_substring" "$body_file"; then
    FAILED=$((FAILED + 1))
    FAIL_LINES+=("$tier $method $url — body missing fingerprint: $want_substring")
    printf "  %s✘%s %s %s %-50s ${DIM}body missing \"%s\" · %dms%s\n" \
      "$RED" "$RESET" "$tier_padded" "$method" "$pretty_url" \
      "$want_substring" "$elapsed_ms" "$RESET"
    printf "     %sbody (first 240 chars):%s\n" "$DIM" "$RESET"
    head -c 240 "$body_file" | sed 's/^/       /'
    printf "\n"
    rm -f "$body_file"
    return 1
  fi

  PASSED=$((PASSED + 1))
  printf "  %s✔%s %s %s %-50s ${DIM}%s · %dms%s\n" \
    "$GREEN" "$RESET" "$tier_padded" "$method" "$pretty_url" \
    "$http_code" "$elapsed_ms" "$RESET"
  rm -f "$body_file"
  return 0
}

# Specialised image check — asserts content-type starts with `image/`
# and the body is non-trivially-sized (OG cards should be PNG, several
# KB minimum).
check_image() {
  local tier="$1" url="$2"
  local body_file content_type size start_ms end_ms elapsed_ms http_code
  body_file=$(mktemp)
  start_ms=$(python3 -c 'import time; print(int(time.time()*1000))')
  IFS=$'\n' read -d '' -r http_code content_type < <(curl -sS -L -m "$TIMEOUT_S" \
    -o "$body_file" -w "%{http_code}\n%{content_type}\n" \
    -H "user-agent: ml-art-smoke/1" \
    "$url" 2>/dev/null || true; printf '\0')
  end_ms=$(python3 -c 'import time; print(int(time.time()*1000))')
  elapsed_ms=$((end_ms - start_ms))
  size=$(stat -f%z "$body_file" 2>/dev/null || stat -c%s "$body_file")

  local pretty_url="${url#https://}"
  local tier_padded
  tier_padded=$(printf "%-3s" "$tier")

  if [[ "$http_code" != "200" ]] || [[ ! "$content_type" =~ ^image/ ]] || (( size < 1024 )); then
    FAILED=$((FAILED + 1))
    FAIL_LINES+=("$tier image $url — status=$http_code ct=$content_type size=${size}B")
    printf "  %s✘%s %s GET %-50s ${DIM}status=%s ct=%s size=%dB · %dms%s\n" \
      "$RED" "$RESET" "$tier_padded" "$pretty_url" \
      "$http_code" "$content_type" "$size" "$elapsed_ms" "$RESET"
    rm -f "$body_file"
    return 1
  fi

  PASSED=$((PASSED + 1))
  printf "  %s✔%s %s GET %-50s ${DIM}%s · %dKB · %dms%s\n" \
    "$GREEN" "$RESET" "$tier_padded" "$pretty_url" \
    "$content_type" $((size / 1024)) "$elapsed_ms" "$RESET"
  rm -f "$body_file"
  return 0
}

# ──────────────────────────────────────────────────────────────────────
# Runs
# ──────────────────────────────────────────────────────────────────────

printf "%sml-art prod smoke%s — %s\n\n" "$BOLD" "$RESET" "$(date -u +%FT%TZ)"

printf "%sAPI %s\n" "$BOLD" "$RESET"
check api GET "$API_ORIGIN/v1/health"                                                                   200 '"status":"ok"'
check api GET "$API_ORIGIN/v1/search?q=morning"                                                          200 '"items"'
check api GET "$API_ORIGIN/v1/artists/$FIXTURE_ARTIST_SLUG"                                              200 "$FIXTURE_ARTIST_NAME"
check api GET "$API_ORIGIN/v1/artworks/$FIXTURE_ARTWORK_ID"                                              200 "$FIXTURE_ARTWORK_TITLE"
check api GET "$API_ORIGIN/v1/artworks/$FIXTURE_ARTWORK_ID/similar"                                      200 '"items"'
check api GET "$API_ORIGIN/v1/neighborhoods"                                                             200 '"items"'
# /v1/search/map/cities returns a bare JSON array (not wrapped in an
# `items` envelope like the search endpoints). Fingerprint on the
# field name instead.
check api GET "$API_ORIGIN/v1/search/map/cities"                                                         200 '"city"'

printf "\n%sWeb %s\n" "$BOLD" "$RESET"
check web GET "$WEB_ORIGIN/"                                                                             200 "Wander"
check web GET "$WEB_ORIGIN/search"                                                                       200 "Wander"
check web GET "$WEB_ORIGIN/search?q=morning"                                                             200 "Wander"
check web GET "$WEB_ORIGIN/artists/$FIXTURE_ARTIST_SLUG"                                                 200 "$FIXTURE_ARTIST_NAME"
check web GET "$WEB_ORIGIN/artworks/$FIXTURE_ARTWORK_ID"                                                 200 "$FIXTURE_ARTWORK_TITLE"
check web GET "$WEB_ORIGIN/neighborhoods"                                                                200 "Wander"
check web GET "$WEB_ORIGIN/sign-in"                                                                      200 "Wander"

printf "\n%sShare cards %s\n" "$BOLD" "$RESET"
check_image og  "$WEB_ORIGIN/artists/$FIXTURE_ARTIST_SLUG/opengraph-image"
check_image og  "$WEB_ORIGIN/artworks/$FIXTURE_ARTWORK_ID/opengraph-image"

printf "\n%sImage CDN %s\n" "$BOLD" "$RESET"
# Hit a known image. CloudFront on this distribution serves from S3 +
# applies the standard cache headers; we just want to know the path
# resolves and returns image bytes.
check_image img "$IMAGES_ORIGIN/uploads/8a602f0e-78a0-4528-8865-bef491028d4c.png"

# ──────────────────────────────────────────────────────────────────────
# Summary
# ──────────────────────────────────────────────────────────────────────

printf "\n"
TOTAL=$((PASSED + FAILED))
if (( FAILED == 0 )); then
  printf "%s✔%s  %s%d/%d checks passed%s\n" "$GREEN" "$RESET" "$BOLD" "$PASSED" "$TOTAL" "$RESET"
  exit 0
else
  printf "%s✘%s  %s%d failed%s, %d passed (of %d)\n\n" \
    "$RED" "$RESET" "$BOLD" "$FAILED" "$RESET" "$PASSED" "$TOTAL"
  printf "%sFailures:%s\n" "$BOLD" "$RESET"
  for line in "${FAIL_LINES[@]}"; do
    printf "  • %s\n" "$line"
  done
  printf "\n%sNext steps:%s\n" "$DIM" "$RESET"
  printf "  ${DIM}- CloudWatch:%s aws --profile ml-art logs tail /aws/lambda/ml-art-prod-api --since 5m\n" "$RESET"
  printf "  ${DIM}- CloudWatch:%s aws --profile ml-art logs tail /aws/lambda/ml-art-prod-web --since 5m\n" "$RESET"
  printf "  ${DIM}- WAF samples:%s see scripts/check-waf-blocks.sh (or wafv2 get-sampled-requests)\n" "$RESET"
  exit 1
fi
