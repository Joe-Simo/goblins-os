#!/usr/bin/env bash
# Complete the verification VM first-boot choice through the real session APIs.
set -euo pipefail

LIVE_URL="${GOBLINS_OS_CORE_URL:-http://127.0.0.1:8787}"
HOST_READY_URL="${GOBLINS_HWGATE_HOST_URL:-http://10.0.2.2:@GOS_PORT@}"

wait_for_core() {
  for _ in $(seq 1 90); do
    curl -sf --max-time 2 "$LIVE_URL/health" >/dev/null 2>&1 && return 0
    sleep 0.5
  done
  curl -sf --max-time 2 "$LIVE_URL/health" >/dev/null
}

post_json() {
  local route="$1"
  local body="$2"
  curl -fsS --max-time 10 \
    -H 'Content-Type: application/json' \
    -d "$body" \
    "$LIVE_URL$route" >/dev/null
}

wait_for_core
post_json /v1/privacy '{"offline":true}'
post_json /v1/installer/complete '{"mode":"local-gpt-oss"}'
post_json /v1/session/unlock '{"mode":"local-gpt-oss"}'

# The first-boot UI normally quits after the private path succeeds. The gate
# triggers the same backend contract from Alt+F2, so close the now-stale windows
# before launching the proof orchestrator.
pkill -f 'goblins-os-installer' 2>/dev/null || true
pkill -f 'goblins-os-login' 2>/dev/null || true

curl -s --max-time 2 "$HOST_READY_URL/ready/FIRSTBOOT_UNLOCK?status=pass" >/dev/null 2>&1 || true
echo GOBLINS_HWGATE_FIRSTBOOT_UNLOCK_DONE
