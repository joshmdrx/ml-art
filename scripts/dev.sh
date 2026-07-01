#!/usr/bin/env bash
# Start api + web in the foreground.
# Both processes share this script's process group; Ctrl-C tears them down
# cleanly via the EXIT trap.
#
# Logs are streamed to /tmp/api.log and /tmp/web.log so `make logs-api` /
# `make logs-web` work from another terminal.
set -euo pipefail

cd "$(dirname "$0")/.."

# Sanity: cargo-watch gives the api binary the same hot-reload story the
# Next.js dev server has by default. Without it every endpoint change
# requires a manual `make down && make dev`.
if ! command -v cargo-watch >/dev/null 2>&1; then
  echo "✘ cargo-watch is not installed. Install once with:"
  echo "    cargo install cargo-watch"
  echo "  Then re-run 'make dev'."
  exit 1
fi

# Reclaim 9100/3000 in case a previous `make dev` died ungracefully (terminal
# closed, SIGKILL, etc.) and left the api binary or next-dev holding the
# port. Without this, `make dev` twice in a row would fail with EADDRINUSE
# on the api side and silently degrade — the situation that wasted ~10
# minutes of debugging when a stale binary served /v1/health but 404'd new
# routes.
scripts/kill-port.sh 9100 api >/dev/null
scripts/kill-port.sh 3000 web >/dev/null

# Walk the process tree from $2 and send signal $1 to every descendant,
# leaves first so reparenting doesn't strand a grandchild while we kill
# its parent. cargo-watch in particular spawns `cargo run`, which spawns
# the api binary; only the leaf holds port 9100. A flat `pkill -P $$`
# would kill the immediate subshell and orphan the binary, leaving the
# port bound until the next manual `pkill`.
kill_descendants() {
  local sig=$1 pid=$2 child
  for child in $(pgrep -P "$pid" 2>/dev/null); do
    kill_descendants "$sig" "$child"
    kill -"$sig" "$child" 2>/dev/null || true
  done
}

cleanup() {
  echo ""
  echo "stopping…"
  kill_descendants TERM $$
  sleep 0.5
  kill_descendants KILL $$
}
trap cleanup EXIT INT TERM

# ─── api ────────────────────────────────────────────────────────────────────
echo "→ starting api (logs: /tmp/api.log)  [auto-reloads on *.rs / *.sql]"
(
  cd api
  # Picks up DATABASE_URL, JINA_API_KEY, etc. from api/.env via dotenvy.
  # cargo-watch rebuilds + restarts the binary on changes to Rust sources
  # and SQL migrations. `--why` prints the changed path into /tmp/api.log
  # so a confused restart is easy to trace.
  # WANDER_ADMIN_EMAIL_ALLOWLIST is a comma-separated list of email
  # suffixes; any user whose email ends with one is auto-promoted to
  # is_admin=true on first sign-in. The E2E harness randomises a
  # prefix + tacks on `-admin+clerk_test@example.com`, so this seam
  # lets admin specs drive the /admin surface locally without a
  # post-signup psql edit. Empty in prod. Clerk's test-email
  # convention already blocks these literals in a real signup.
  PORT=9100 \
  WANDER_ADMIN_EMAIL_ALLOWLIST=-admin+clerk_test@example.com \
  cargo watch \
    --why \
    --watch crates \
    --watch ../db/migrations \
    -x 'run -p api-search' \
    >/tmp/api.log 2>&1
) &
API_PID=$!

# ─── jobs-worker ────────────────────────────────────────────────────────────
# Polls the jobs table; runs background handlers (geocoding today, email
# + moderation later). Same handler code that prod runs from Lambda+SQS.
# Logs go to /tmp/worker.log; cargo-watch restarts on the same triggers
# as the api so handler edits land instantly.
echo "→ starting jobs-worker (logs: /tmp/worker.log)  [auto-reloads]"
(
  cd api
  cargo watch \
    --why \
    --watch crates \
    --watch ../db/migrations \
    -x 'run -p jobs-worker' \
    >/tmp/worker.log 2>&1
) &
WORKER_PID=$!

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
echo "    web    http://localhost:3000"
echo "    api    http://localhost:9100/v1/health"
echo "    worker (background, see /tmp/worker.log)"
echo "    minio  http://localhost:9001  (dev / devpassword)"
echo "    mail   http://localhost:8025"
echo ""
echo "  tail logs in another terminal:"
echo "    make logs-api   |   make logs-web   |   tail -f /tmp/worker.log"
echo ""
echo "  Ctrl-C to stop both."

# Block until either child exits (or Ctrl-C fires the trap).
wait
