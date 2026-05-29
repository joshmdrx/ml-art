#!/usr/bin/env bash
# Kill any process listening on a TCP port. Idempotent.
#
# Usage: scripts/kill-port.sh <port> [label]
#
# Always exits 0. Prints what was killed, or "nothing to kill" if the port
# was already free. Used by `make down` to reclaim 9100/3000 after a stray
# `make dev` left behind a stale api binary or next-dev process.
set -uo pipefail

port=${1:?port required}
label=${2:-port $port}

pids=$(lsof -ti tcp:"$port" -sTCP:LISTEN 2>/dev/null || true)

if [ -z "$pids" ]; then
  printf "  %-12s — nothing to kill on :%s\n" "$label" "$port"
  exit 0
fi

# TERM first so the process can flush; escalate to KILL if it lingers.
kill $pids 2>/dev/null || true
for _ in 1 2 3 4; do
  remaining=$(lsof -ti tcp:"$port" -sTCP:LISTEN 2>/dev/null || true)
  [ -z "$remaining" ] && break
  sleep 0.5
done
remaining=$(lsof -ti tcp:"$port" -sTCP:LISTEN 2>/dev/null || true)
if [ -n "$remaining" ]; then
  kill -9 $remaining 2>/dev/null || true
fi

printf "  %-12s — killed pid(s) on :%s: %s\n" "$label" "$port" "$(echo $pids | tr '\n' ' ')"
