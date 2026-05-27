#!/usr/bin/env bash
# Start api + web in the foreground.
# Both processes share this script's process group; Ctrl-C tears them down
# cleanly via the EXIT trap.
#
# Logs are streamed to /tmp/api.log and /tmp/web.log so `make logs-api` /
# `make logs-web` work from another terminal.
set -euo pipefail

cd "$(dirname "$0")/.."

# Sanity: make sure docker is up before we start anything else.
if ! docker ps --format '{{.Names}}' | grep -q '^ml-art-postgres$'; then
  echo "✘ docker services aren't running. Start them first:"
  echo "    make up"
  exit 1
fi

# Helper: kill the whole process group we created. Works even if children
# spawn grandchildren (cargo run forks the actual binary, next dev forks
# the next-server process).
cleanup() {
  echo ""
  echo "stopping…"
  # SIGTERM to the process group; trap re-runs the script's own EXIT.
  pkill -P $$ 2>/dev/null || true
  # Give cargo a beat to finish its drop, then SIGKILL any holdouts.
  sleep 0.5
  pkill -KILL -P $$ 2>/dev/null || true
}
trap cleanup EXIT INT TERM

# ─── api ────────────────────────────────────────────────────────────────────
echo "→ starting api (logs: /tmp/api.log)"
(
  cd api
  # Picks up DATABASE_URL, JINA_API_KEY, etc. from api/.env via dotenvy.
  PORT=9100 cargo run -p api-search >/tmp/api.log 2>&1
) &
API_PID=$!

# ─── web ────────────────────────────────────────────────────────────────────
echo "→ starting web (logs: /tmp/web.log)"
(
  cd web
  pnpm dev >/tmp/web.log 2>&1
) &
WEB_PID=$!

# ─── readiness ──────────────────────────────────────────────────────────────
echo -n "  waiting for api …"
for i in {1..60}; do
  if curl -sf http://localhost:9100/v1/health >/dev/null 2>&1; then
    echo " up"
    break
  fi
  if ! kill -0 "$API_PID" 2>/dev/null; then
    echo " CRASHED — last 30 lines of /tmp/api.log:"
    tail -30 /tmp/api.log || true
    exit 1
  fi
  sleep 1
done

echo -n "  waiting for web …"
for i in {1..60}; do
  if curl -sf http://localhost:3000 >/dev/null 2>&1; then
    echo " up"
    break
  fi
  if ! kill -0 "$WEB_PID" 2>/dev/null; then
    echo " CRASHED — last 30 lines of /tmp/web.log:"
    tail -30 /tmp/web.log || true
    exit 1
  fi
  sleep 1
done

echo ""
echo "✔ ml-art is up"
echo "    web   http://localhost:3000"
echo "    api   http://localhost:9100/v1/health"
echo "    minio http://localhost:9001  (dev / devpassword)"
echo "    mail  http://localhost:8025"
echo ""
echo "  tail logs in another terminal:"
echo "    make logs-api   |   make logs-web"
echo ""
echo "  Ctrl-C to stop both."

# Block until either child exits (or Ctrl-C fires the trap).
wait
