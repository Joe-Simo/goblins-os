#!/usr/bin/env bash
# Complete the verification VM first-boot choice through the root-only
# release-proof capability socket.
set -euo pipefail

HOST_READY_URL="${GOBLINS_HWGATE_HOST_URL:-http://10.0.2.2:@GOS_PORT@}"
CAPTURE_TOKEN="${GOBLINS_HWGATE_CAPTURE_TOKEN:?missing capture bearer token}"
if [[ ! "$HOST_READY_URL" =~ ^http://10[.]0[.]2[.]2:[0-9]{4,5}$ ]] \
  || [[ ! "$CAPTURE_TOKEN" =~ ^[0-9a-f]{64}$ ]]; then
  echo "invalid authenticated capture channel configuration" >&2
  exit 78
fi
CORE_PROOF_SOCKET=/run/goblins-os-core/release-proof/control.sock
CORE_PROOF_URL=http://localhost
CORE_UNIT=goblins-os-core.service
CORE_UNIT_FRAGMENT=/usr/lib/systemd/system/goblins-os-core.service
CORE_TRUSTED_DROPIN=/usr/lib/systemd/system/service.d/10-timeout-abort.conf
CORE_TRUSTED_DROPIN_SHA256=ae6b234f92bc22f1201a7572b59b454c9809f33c80d13f361b9674e1801acc37
CORE_EXECUTABLE=/usr/libexec/goblins-os/goblins-os-core
CORE_READ_WRITE_PATHS="/run/goblins-os-core /var/lib/goblins-os/installer /var/lib/goblins-os/session /var/lib/goblins-os/policy /var/lib/goblins-os/ai /var/lib/goblins-os/models /var/lib/goblins-os/voice/work /var/lib/goblins-os/secrets/openai /var/lib/goblins-os/apps /var/lib/goblins-os/codex"
CORE_CAPABILITY_SLUGS=(
  control-center dictate file-builder focus-tick installer launcher login markup
  open release-proof resident screenshot-context settings shell today visual-lookup
  voice-control
)
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
    -H "Authorization: Bearer $CAPTURE_TOKEN" \
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
  setpriv --regid=goblins-core-release-proof --clear-groups -- \
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

mount_is_effectively_writable() {
  local main_pid="$1"
  local path="$2"
  local options
  options="$(
    nsenter --target "$main_pid" --mount -- \
      findmnt --noheadings --output VFS-OPTIONS --target "$path" 2>/dev/null \
      | tr -d '[:space:]'
  )" || return 1
  case ",$options," in
    *,rw,*) return 0 ;;
    *) return 1 ;;
  esac
}

main_pid_owns_listener_socket() {
  local main_pid="$1"
  local path="$2"
  local socket_inode fd
  socket_inode="$(
    awk -v path="$path" \
      '$4 == "00010000" && $5 == "0001" && $6 == "01" && $8 == path { count += 1; inode = $7 }
       END { if (count == 1) print inode }' \
      "/proc/$main_pid/net/unix" 2>/dev/null
  )" || return 1
  case "$socket_inode" in
    ''|*[!0-9]*) return 1 ;;
  esac
  for fd in "/proc/$main_pid"/fd/*; do
    if [ "$(readlink "$fd" 2>/dev/null || true)" = "socket:[$socket_inode]" ]; then
      return 0
    fi
  done
  return 1
}

prove_production_capability_inventory() {
  local main_pid="$1"
  local root=/run/goblins-os-core
  local slug directory socket expected_group entry_count root_owner_mode
  local -A seen_slugs=()

  [ -d "$root" ] && [ ! -L "$root" ] || return 1
  root_owner_mode="$(stat -c '%U:%G:%a' "$root" 2>/dev/null)" || return 1
  [ "$root_owner_mode" = root:root:755 ] || return 1
  [ "${#CORE_CAPABILITY_SLUGS[@]}" = 17 ] || return 1
  entry_count="$(find "$root" -mindepth 1 -maxdepth 1 -type d -print 2>/dev/null | wc -l | tr -d '[:space:]')" || return 1
  [ "$entry_count" = 17 ] || return 1
  [ -z "$(find "$root" -mindepth 1 -maxdepth 1 ! -type d -print -quit 2>/dev/null)" ] || return 1

  for slug in "${CORE_CAPABILITY_SLUGS[@]}"; do
    [ -z "${seen_slugs[$slug]+present}" ] || return 1
    seen_slugs[$slug]=present
    directory="$root/$slug"
    socket="$directory/control.sock"
    expected_group="goblins-core-$slug"
    [ -d "$directory" ] && [ ! -L "$directory" ] || return 1
    [ "$(stat -c '%U:%G:%a' "$directory" 2>/dev/null)" = "goblins-os:$expected_group:2750" ] || return 1
    [ -S "$socket" ] && [ ! -L "$socket" ] || return 1
    [ "$(stat -c '%U:%G:%a' "$socket" 2>/dev/null)" = "goblins-os:$expected_group:660" ] || return 1
    main_pid_owns_listener_socket "$main_pid" "$socket" || return 1
  done
}

prove_production_core_unit() {
  local active substate main_pid fragment dropins protect_system read_write_paths
  local timeout_stop_failure_mode dropin_owner_mode dropin_sha256 dropin_package
  local running_executable installed_executable
  active="$(systemctl show "$CORE_UNIT" --property=ActiveState --value 2>/dev/null)" || return 1
  substate="$(systemctl show "$CORE_UNIT" --property=SubState --value 2>/dev/null)" || return 1
  main_pid="$(systemctl show "$CORE_UNIT" --property=MainPID --value 2>/dev/null)" || return 1
  fragment="$(systemctl show "$CORE_UNIT" --property=FragmentPath --value 2>/dev/null)" || return 1
  dropins="$(systemctl show "$CORE_UNIT" --property=DropInPaths --value 2>/dev/null)" || return 1
  protect_system="$(systemctl show "$CORE_UNIT" --property=ProtectSystem --value 2>/dev/null)" || return 1
  read_write_paths="$(systemctl show "$CORE_UNIT" --property=ReadWritePaths --value 2>/dev/null)" || return 1
  timeout_stop_failure_mode="$(systemctl show "$CORE_UNIT" --property=TimeoutStopFailureMode --value 2>/dev/null)" || return 1

  [ "$active" = active ] || return 1
  [ "$substate" = running ] || return 1
  case "$main_pid" in
    ''|*[!0-9]*) return 1 ;;
  esac
  [ "$main_pid" -gt 1 ] || return 1
  [ "$fragment" = "$CORE_UNIT_FRAGMENT" ] || return 1
  [ "$dropins" = "$CORE_TRUSTED_DROPIN" ] || return 1
  [ -f "$CORE_TRUSTED_DROPIN" ] && [ ! -L "$CORE_TRUSTED_DROPIN" ] || return 1
  dropin_owner_mode="$(stat -c '%U:%G:%a' "$CORE_TRUSTED_DROPIN" 2>/dev/null)" || return 1
  [ "$dropin_owner_mode" = root:root:644 ] || return 1
  dropin_sha256="$(sha256sum "$CORE_TRUSTED_DROPIN" 2>/dev/null | awk '{print $1}')" || return 1
  [ "$dropin_sha256" = "$CORE_TRUSTED_DROPIN_SHA256" ] || return 1
  dropin_package="$(rpm -qf --qf '%{NAME}' "$CORE_TRUSTED_DROPIN" 2>/dev/null)" || return 1
  [ "$dropin_package" = systemd ] || return 1
  [ "$timeout_stop_failure_mode" = abort ] || return 1
  [ "$protect_system" = strict ] || return 1
  [ "$read_write_paths" = "$CORE_READ_WRITE_PATHS" ] || return 1

  running_executable="$(stat -Lc '%d:%i' "/proc/$main_pid/exe" 2>/dev/null)" || return 1
  installed_executable="$(stat -Lc '%d:%i' "$CORE_EXECUTABLE" 2>/dev/null)" || return 1
  [ "$running_executable" = "$installed_executable" ] || return 1
  prove_production_capability_inventory "$main_pid" || return 1
  mount_is_effectively_writable "$main_pid" /run/goblins-os-core || return 1
  mount_is_effectively_writable "$main_pid" /var/lib/goblins-os/voice/work || return 1

  serial "GOBLINS_HWGATE_CORE_PRODUCTION_UNIT status=pass identity=systemd-main-pid dropin=vendor-sha256 listeners=17 runtime_mount=rw voice_work_mount=rw"
}

prove_voice_storage() {
  local response body http_status curl_rc=0
  CURRENT_STAGE=voice-storage
  response="$(
    core_proof_curl -sS --max-filesize 4096 -w $'\n%{http_code}' --max-time 10 \
      -X POST "$CORE_PROOF_URL/v1/release-proof/storage/voice" 2>/dev/null
  )" || curl_rc=$?
  http_status="${response##*$'\n'}"
  body="${response%$'\n'*}"
  if [ "$curl_rc" -ne 0 ] \
    || [ "$http_status" != 200 ] \
    || ! jq -e \
      '.ok == true and .storage == "voice-work" and .create_new == true and .write == true and .fsync == true and .unlink == true' \
      <<<"$body" >/dev/null 2>&1; then
    serial "GOBLINS_HWGATE_FIRSTBOOT_STAGE stage=voice-storage status=fail curl_rc=$curl_rc http_status=${http_status:-000}"
    return 1
  fi
  serial "GOBLINS_HWGATE_FIRSTBOOT_STAGE stage=voice-storage status=pass curl_rc=0 http_status=200 create_new=true write=true fsync=true unlink=true"
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
CURRENT_STAGE=core-production-unit
prove_production_core_unit
prove_voice_storage
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
  -H "Authorization: Bearer $CAPTURE_TOKEN" \
  "$HOST_READY_URL/ready/FIRSTBOOT_UNLOCK?status=pass" >/dev/null
serial "GOBLINS_HWGATE_FIRSTBOOT_UNLOCK_DONE"
