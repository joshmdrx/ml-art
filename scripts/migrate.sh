#!/usr/bin/env bash
# Apply all migrations in db/migrations/ in lexicographic order.
# Idempotent if the SQL itself uses IF NOT EXISTS / ON CONFLICT.
#
# Usage: scripts/migrate.sh
set -euo pipefail

cd "$(dirname "$0")/.."

if ! docker ps --format '{{.Names}}' | grep -q '^ml-art-postgres$'; then
  echo "✘ postgres container is not running. Bring it up first:"
  echo "    make up"
  exit 1
fi

# Wait for postgres to actually accept connections (it can take a few
# seconds after `docker compose up` on a cold start).
for i in {1..20}; do
  if docker exec ml-art-postgres pg_isready -U ml_art -d ml_art_dev >/dev/null 2>&1; then
    break
  fi
  sleep 1
done

shopt -s nullglob
files=(db/migrations/*.sql)
if [ ${#files[@]} -eq 0 ]; then
  echo "no migrations to apply"
  exit 0
fi

for f in "${files[@]}"; do
  printf "  applying %s ... " "$(basename "$f")"
  if docker exec -i ml-art-postgres \
       psql -U ml_art -d ml_art_dev -v ON_ERROR_STOP=1 -q < "$f" >/dev/null; then
    echo "ok"
  else
    echo "FAILED"
    exit 1
  fi
done

echo "✔ migrations complete"
