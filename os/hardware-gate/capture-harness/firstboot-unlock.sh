#!/usr/bin/env bash
# Complete the verification VM first-boot choice through the root-only
# release-proof capability socket.
set -euo pipefail

HOST_READY_URL="${GOBLINS_HWGATE_HOST_URL:-http://10.0.2.2:@GOS_PORT@}"
CORE_PROOF_SOCKET=/run/goblins-os-core/release-proof/control.sock
CORE_PROOF_URL=http://localhost

if [ "$(id -u)" -ne 0 ]; then
  echo "firstboot-unlock requires the verification image's root orchestrator" >&2
  exit 77
fi

core_proof_curl() {
  curl --unix-socket "$CORE_PROOF_SOCKET" "$@"
}

wait_for_core() {
  for _ in $(seq 1 90); do
    core_proof_curl -sf --max-time 2 "$CORE_PROOF_URL/health" >/dev/null 2>&1 && return 0
    sleep 0.5
  done
  core_proof_curl -sf --max-time 2 "$CORE_PROOF_URL/health" >/dev/null
}

post_json() {
  local route="$1"
  local body="$2"
  core_proof_curl -fsS --max-time 10 \
    -H 'Content-Type: application/json' \
    -d "$body" \
    "$CORE_PROOF_URL$route" >/dev/null
}

wait_for_core
post_json /v1/privacy '{"offline":true}'
post_json /v1/installer/complete '{"mode":"local-gpt-oss"}'
post_json /v1/session/unlock '{"mode":"local-gpt-oss"}'

# The first-boot UI normally quits after the private path succeeds. The root
# verification service triggers that same backend contract through its fixed
# capability, then closes the stale windows before the session proof starts.
pkill -f 'goblins-os-installer' 2>/dev/null || true
pkill -f 'goblins-os-login' 2>/dev/null || true

curl -s --max-time 2 "$HOST_READY_URL/ready/FIRSTBOOT_UNLOCK?status=pass" >/dev/null 2>&1 || true
echo GOBLINS_HWGATE_FIRSTBOOT_UNLOCK_DONE
