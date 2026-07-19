#!/usr/bin/env bash
# Complete the verification VM first-boot choice through the root-only
# release-proof capability socket.
set -euo pipefail

HOST_READY_URL="${GOBLINS_HWGATE_HOST_URL:-http://10.0.2.2:@GOS_PORT@}"
CORE_PROOF_SOCKET=/run/goblins-os-core/release-proof/control.sock
CORE_PROOF_URL=http://localhost
CORE_HEALTH_TIMEOUT_SECONDS=90
CURRENT_STAGE=bootstrap

serial() {
  printf '%s\n' "$*" | tee /dev/ttyS0 /dev/ttyAMA0 >/dev/null 2>/dev/null || true
}

report_core_unit_state() {
  local property value
  for property in ActiveState SubState Result ExecMainStatus NRestarts; do
    value="$(systemctl show goblins-os-core.service --property="$property" --value 2>/dev/null || true)"
    case "$value" in
      *[!A-Za-z0-9_.:@+-]*|'') value=unknown ;;
    esac
    serial "GOBLINS_HWGATE_CORE_UNIT_STATE property=$property value=$value"
  done
  if [ -S "$CORE_PROOF_SOCKET" ]; then
    serial "GOBLINS_HWGATE_CORE_PROOF_SOCKET state=present"
  else
    serial "GOBLINS_HWGATE_CORE_PROOF_SOCKET state=missing"
  fi
}

report_failure() {
  local rc="$1"
  serial "GOBLINS_HWGATE_FIRSTBOOT_UNLOCK_FAILED stage=$CURRENT_STAGE rc=$rc"
  report_core_unit_state
  curl -fsS --connect-timeout 2 --max-time 2 \
    "$HOST_READY_URL/failed/FIRSTBOOT_UNLOCK?stage=$CURRENT_STAGE&rc=$rc" \
    >/dev/null 2>&1 || true
}

on_exit() {
  local rc=$?
  trap - EXIT
  if [ "$rc" -ne 0 ]; then
    report_failure "$rc"
  fi
  exit "$rc"
}

trap on_exit EXIT
trap 'exit 130' INT
trap 'exit 143' TERM

if [ "$(id -u)" -ne 0 ]; then
  echo "firstboot-unlock requires the verification image's root orchestrator" >&2
  exit 77
fi

core_proof_curl() {
  curl --unix-socket "$CORE_PROOF_SOCKET" --connect-timeout 2 "$@"
}

wait_for_core() {
  local deadline=$((SECONDS + CORE_HEALTH_TIMEOUT_SECONDS))
  local curl_rc=0
  local http_status=000
  while [ "$SECONDS" -lt "$deadline" ]; do
    curl_rc=0
    http_status="$(
      core_proof_curl -sS -o /dev/null -w '%{http_code}' --max-time 2 \
        "$CORE_PROOF_URL/health" 2>/dev/null
    )" || curl_rc=$?
    if [ "$curl_rc" -eq 0 ] && [ "$http_status" = 200 ]; then
      serial "GOBLINS_HWGATE_FIRSTBOOT_STAGE stage=core-health status=pass curl_rc=0 http_status=200"
      return 0
    fi
    sleep 0.5
  done
  serial "GOBLINS_HWGATE_FIRSTBOOT_STAGE stage=core-health status=fail curl_rc=$curl_rc http_status=$http_status"
  return 1
}

post_json() {
  local stage="$1"
  local route="$2"
  local body="$3"
  local curl_rc=0
  local http_status=000
  CURRENT_STAGE="$stage"
  http_status="$(
    core_proof_curl -sS -o /dev/null -w '%{http_code}' --max-time 10 \
      -H 'Content-Type: application/json' \
      -d "$body" \
      "$CORE_PROOF_URL$route" 2>/dev/null
  )" || curl_rc=$?
  if [ "$curl_rc" -ne 0 ] || [[ ! "$http_status" =~ ^2[0-9][0-9]$ ]]; then
    serial "GOBLINS_HWGATE_FIRSTBOOT_STAGE stage=$stage status=fail curl_rc=$curl_rc http_status=$http_status"
    return 1
  fi
  serial "GOBLINS_HWGATE_FIRSTBOOT_STAGE stage=$stage status=pass curl_rc=0 http_status=$http_status"
}

CURRENT_STAGE=core-health
wait_for_core
post_json privacy /v1/privacy '{"offline":true}'
post_json installer-complete /v1/installer/complete '{"mode":"local-gpt-oss"}'
post_json session-unlock /v1/session/unlock '{"mode":"local-gpt-oss"}'

# The first-boot UI normally quits after the private path succeeds. The root
# verification service triggers that same backend contract through its fixed
# capability, then closes the stale windows before the session proof starts.
pkill -f 'goblins-os-installer' 2>/dev/null || true
pkill -f 'goblins-os-login' 2>/dev/null || true

CURRENT_STAGE=host-callback
curl -fsS --connect-timeout 2 --max-time 2 \
  "$HOST_READY_URL/ready/FIRSTBOOT_UNLOCK?status=pass" >/dev/null
serial "GOBLINS_HWGATE_FIRSTBOOT_UNLOCK_DONE"
