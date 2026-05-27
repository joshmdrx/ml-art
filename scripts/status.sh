#!/usr/bin/env bash
# Quick health check across local services. Read-only.
set -uo pipefail

cd "$(dirname "$0")/.."

check() {
  local name=$1
  local url=$2
  if curl -sf -o /dev/null --max-time 2 "$url"; then
    printf "  %-12s ✔ up    (%s)\n" "$name" "$url"
  else
    printf "  %-12s ✘ down  (%s)\n" "$name" "$url"
  fi
}

check_pg() {
  if docker exec ml-art-postgres pg_isready -U ml_art -d ml_art_dev >/dev/null 2>&1; then
    printf "  %-12s ✔ up    (postgres://localhost:5433)\n" "postgres"
  else
    printf "  %-12s ✘ down  (postgres://localhost:5433)\n" "postgres"
  fi
}

echo "ml-art local services"
echo ""
check_pg
check "minio"   "http://localhost:9000/minio/health/live"
check "mailhog" "http://localhost:8025"
check "api"     "http://localhost:9100/v1/health"
check "web"     "http://localhost:3000"
echo ""
