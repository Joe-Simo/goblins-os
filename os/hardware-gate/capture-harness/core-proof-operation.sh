#!/usr/bin/env bash
# Root side of the verification-image control-plane handoff. The unprivileged
# capture session may start only the finite systemd unit instances authorized by
# the verification-only polkit rule. Every instance maps to one fixed method,
# path, and (where applicable) fixed payload on the server-only release-proof
# capability socket; no caller-controlled route or generic proxy exists.
set -euo pipefail

if [ "$(id -u)" -ne 0 ]; then
  echo "core-proof-operation must run as root" >&2
  exit 77
fi

OPERATION="${1:-}"
OPERATION_RESULT="${2:-unknown}"
CORE_SOCKET=/run/goblins-os-core/release-proof/control.sock
CORE_URL=http://localhost
RESULT_DIR=/run/goblins-hwgate-core-proof
FIXTURE_STATE=/run/goblins-hwgate-fixture-state
FIXTURE_BLOCK=/run/goblins-hwgate-fixture-block
FIXTURE_RESIDENT_SOCKET=$FIXTURE_STATE/resident/resident.sock
BODY_TMP=""
STATUS_TMP=""
INPUT_TMP=""
FIXTURE_SWAP_IN_PROGRESS=false

install -d -m 0755 -o root -g root "$RESULT_DIR"

cleanup_temps() {
  [ -z "$BODY_TMP" ] || rm -f "$BODY_TMP"
  [ -z "$STATUS_TMP" ] || rm -f "$STATUS_TMP"
  [ -z "$INPUT_TMP" ] || rm -f "$INPUT_TMP"
  if [ "$FIXTURE_SWAP_IN_PROGRESS" = true ]; then
    systemctl stop goblins-hwgate-fixture-resident.service \
      >/dev/null 2>&1 || true
    systemctl stop goblins-hwgate-fixture-core.service \
      >/dev/null 2>&1 || true
    systemctl --no-block start goblins-os-core.service \
      goblins-os-resident.service >/dev/null 2>&1 || true
  fi
}
trap cleanup_temps EXIT

publish_static_result() {
  local operation="$1"
  local status="$2"
  local body="$3"

  BODY_TMP="$(mktemp "$RESULT_DIR/.${operation}.body.XXXXXX")"
  STATUS_TMP="$(mktemp "$RESULT_DIR/.${operation}.status.XXXXXX")"
  printf '%s\n' "$body" >"$BODY_TMP"
  printf '%s\n' "$status" >"$STATUS_TMP"
  chmod 0644 "$BODY_TMP" "$STATUS_TMP"
  mv -f "$BODY_TMP" "$RESULT_DIR/${operation}.json"
  mv -f "$STATUS_TMP" "$RESULT_DIR/${operation}.status"
  BODY_TMP=""
  STATUS_TMP=""
}

core_proof_curl() {
  setpriv --regid=goblins-core-release-proof --clear-groups -- \
    curl --unix-socket "$CORE_SOCKET" "$@"
}

wait_for_proof_socket() {
  local attempt
  for attempt in $(seq 1 120); do
    if [ -S "$CORE_SOCKET" ] \
      && core_proof_curl -sf --max-time 2 "$CORE_URL/health" >/dev/null 2>&1; then
      return 0
    fi
    sleep 0.25
  done
  return 1
}

wait_for_fixture_resident() {
  local attempt
  for attempt in $(seq 1 120); do
    if systemctl is-active --quiet goblins-hwgate-fixture-resident.service \
      && [ -S "$FIXTURE_RESIDENT_SOCKET" ]; then
      return 0
    fi
    sleep 0.25
  done
  return 1
}

restore_production_services() {
  local restored=true

  systemctl stop goblins-hwgate-fixture-resident.service \
    >/dev/null 2>&1 || restored=false
  systemctl stop goblins-hwgate-fixture-core.service \
    >/dev/null 2>&1 || restored=false
  systemctl start goblins-os-core.service goblins-os-resident.service \
    >/dev/null 2>&1 || restored=false
  wait_for_proof_socket || restored=false
  systemctl is-active --quiet goblins-os-core.service || restored=false
  systemctl is-active --quiet goblins-os-resident.service || restored=false
  if [ "$restored" = true ]; then
    FIXTURE_SWAP_IN_PROGRESS=false
    return 0
  fi
  return 1
}

fail_fixture_start() {
  local stage="$1"
  local restored=false
  local body

  if restore_production_services; then
    restored=true
  fi
  printf -v body \
    '{"ok":false,"text":"fixture startup failed","stage":"%s","production_restored":%s}' \
    "$stage" "$restored"
  publish_static_result fixture-start 503 "$body"
  return 1
}

copy_display_payload() {
  local source="$1"
  INPUT_TMP="$(mktemp "$RESULT_DIR/.display-input.XXXXXX")"
  python3 - "$source" "$INPUT_TMP" <<'PY'
import json
import os
import stat
import sys

source, destination = sys.argv[1:]
fd = os.open(source, os.O_RDONLY | os.O_CLOEXEC | os.O_NOFOLLOW)
try:
    metadata = os.fstat(fd)
    if not stat.S_ISREG(metadata.st_mode) or metadata.st_uid != 1000:
        raise SystemExit("display payload must be a regular file owned by the capture user")
    if metadata.st_size <= 0 or metadata.st_size > 65536:
        raise SystemExit("display payload size is outside the proof contract")
    encoded = b""
    while len(encoded) <= 65536:
        chunk = os.read(fd, 65537 - len(encoded))
        if not chunk:
            break
        encoded += chunk
finally:
    os.close(fd)

payload = json.loads(encoded)
if not isinstance(payload, dict):
    raise SystemExit("display payload must be a JSON object")
with open(destination, "w", encoding="utf-8") as output:
    json.dump(payload, output, separators=(",", ":"))
    output.write("\n")
PY
  chmod 0600 "$INPUT_TMP"
}

request() {
  local operation="$1"
  local method="$2"
  local route="$3"
  local payload="${4:-}"
  local input_file="${5:-}"
  local timeout=30
  local code rc=0
  local curl_args=(
    -sS
    --connect-timeout 2
    -o ""
    -w '%{http_code}'
    -X "$method"
  )

  wait_for_proof_socket || {
    publish_static_result "$operation" 000 '{"ok":false,"text":"release-proof socket unavailable"}'
    return 1
  }
  BODY_TMP="$(mktemp "$RESULT_DIR/.${operation}.body.XXXXXX")"
  STATUS_TMP="$(mktemp "$RESULT_DIR/.${operation}.status.XXXXXX")"
  curl_args[4]="$BODY_TMP"

  if [ "$operation" = "app-build" ]; then
    timeout=3900
  fi
  curl_args+=(--max-time "$timeout")
  if [ -n "$input_file" ]; then
    copy_display_payload "$input_file"
    curl_args+=(-H 'Content-Type: application/json' --data-binary "@$INPUT_TMP")
  elif [ -n "$payload" ]; then
    curl_args+=(-H 'Content-Type: application/json' --data-binary "$payload")
  fi

  if ! code="$(core_proof_curl "${curl_args[@]}" "$CORE_URL$route")"; then
    code=000
    rc=1
  fi
  printf '%s\n' "$code" >"$STATUS_TMP"
  chmod 0644 "$BODY_TMP" "$STATUS_TMP"
  mv -f "$BODY_TMP" "$RESULT_DIR/${operation}.json"
  mv -f "$STATUS_TMP" "$RESULT_DIR/${operation}.status"
  BODY_TMP=""
  STATUS_TMP=""
  return "$rc"
}

grant_app_builder() {
  local status_body profile acknowledgement payload

  request policy-status GET /v1/policy/status
  status_body="$RESULT_DIR/policy-status.json"
  profile="$(python3 - "$status_body" <<'PY'
import json
import sys

try:
    value = json.load(open(sys.argv[1], encoding="utf-8")).get("profile", "")
    print(value if isinstance(value, str) else "")
except Exception:
    print("")
PY
)"
  if [ -z "$profile" ]; then
    publish_static_result policy-grant-app-builder 500 \
      '{"ok":false,"text":"active policy profile unavailable"}'
    return 1
  fi
  acknowledgement="GRANT GOBLINS OS PERMISSION app-builder FOR $profile"
  payload="$(python3 - "$acknowledgement" <<'PY'
import json
import sys
print(json.dumps({"control_id": "app-builder", "acknowledgement": sys.argv[1]}))
PY
)"
  request policy-grant-app-builder POST /v1/policy/permissions/grant "$payload"
}

text_shortcuts_file_contract() {
  local body generated
  body='{"ok":false,"parent_directory":false,"parent_owner":false,"parent_mode":false,"table_regular":false,"table_owner":false,"table_mode":false,"table_single_link":false,"table_size_bounded":false,"table_read_bounded":false,"canonical_entry":false,"legacy_service_table_absent":false}'

  if generated="$(python3 - <<'PY'
import grp
import json
import os
import pwd
import stat
import sys

PARENT = "/var/home/goblin/.config/goblins-os"
TABLE_NAME = "text-shortcuts.json"
LEGACY = "/var/lib/goblins-os/.config/goblins-os/text-shortcuts.json"
MAX_BYTES = 48 * 1024
EXPECTED = [{"replace": "omw", "with": "on my way"}]

result = {
    "ok": False,
    "parent_directory": False,
    "parent_owner": False,
    "parent_mode": False,
    "table_regular": False,
    "table_owner": False,
    "table_mode": False,
    "table_single_link": False,
    "table_size_bounded": False,
    "table_read_bounded": False,
    "canonical_entry": False,
    "legacy_service_table_absent": False,
}
parent_fd = None
table_fd = None

try:
    goblin_uid = pwd.getpwnam("goblin").pw_uid
    goblin_gid = grp.getgrnam("goblin").gr_gid
    parent_fd = os.open(
        PARENT,
        os.O_RDONLY | os.O_DIRECTORY | os.O_CLOEXEC | os.O_NOFOLLOW,
    )
    parent_metadata = os.fstat(parent_fd)
    result["parent_directory"] = stat.S_ISDIR(parent_metadata.st_mode)
    result["parent_owner"] = (
        parent_metadata.st_uid == goblin_uid and parent_metadata.st_gid == goblin_gid
    )
    result["parent_mode"] = stat.S_IMODE(parent_metadata.st_mode) == 0o700

    table_fd = os.open(
        TABLE_NAME,
        os.O_RDONLY | os.O_CLOEXEC | os.O_NOFOLLOW,
        dir_fd=parent_fd,
    )
    table_metadata = os.fstat(table_fd)
    result["table_regular"] = stat.S_ISREG(table_metadata.st_mode)
    result["table_owner"] = (
        table_metadata.st_uid == goblin_uid and table_metadata.st_gid == goblin_gid
    )
    result["table_mode"] = stat.S_IMODE(table_metadata.st_mode) == 0o600
    result["table_single_link"] = table_metadata.st_nlink == 1
    result["table_size_bounded"] = 0 < table_metadata.st_size <= MAX_BYTES

    chunks = []
    remaining = MAX_BYTES + 1
    while remaining > 0:
        chunk = os.read(table_fd, remaining)
        if not chunk:
            break
        chunks.append(chunk)
        remaining -= len(chunk)
    raw = b"".join(chunks)
    result["table_read_bounded"] = 0 < len(raw) <= MAX_BYTES
    if result["table_read_bounded"]:
        result["canonical_entry"] = json.loads(raw.decode("utf-8")) == EXPECTED
except (KeyError, OSError, UnicodeDecodeError, json.JSONDecodeError):
    pass
finally:
    if table_fd is not None:
        os.close(table_fd)
    if parent_fd is not None:
        os.close(parent_fd)

try:
    os.lstat(LEGACY)
except FileNotFoundError:
    result["legacy_service_table_absent"] = True
except OSError:
    pass

result["ok"] = all(value for key, value in result.items() if key != "ok")
print(json.dumps(result, sort_keys=True, separators=(",", ":")))
sys.exit(0 if result["ok"] else 1)
PY
)"; then
    body="$generated"
    publish_static_result text-shortcuts-file-contract 200 "$body"
    return 0
  fi

  [ -z "$generated" ] || body="$generated"
  publish_static_result text-shortcuts-file-contract 500 "$body"
  return 1
}

seed_fixture_block() {
  find "$FIXTURE_BLOCK" -mindepth 1 -delete || return 1
  install -d -m 0755 -o root -g root \
    "$FIXTURE_BLOCK/nvme0n1/queue" "$FIXTURE_BLOCK/nvme0n1/device" || return 1
  printf '536870912\n' >"$FIXTURE_BLOCK/nvme0n1/size" || return 1
  printf '0\n' >"$FIXTURE_BLOCK/nvme0n1/removable" || return 1
  printf '0\n' >"$FIXTURE_BLOCK/nvme0n1/queue/rotational" || return 1
  printf 'Goblins NVMe SSD\n' >"$FIXTURE_BLOCK/nvme0n1/device/model" || return 1

  local partition uevent
  for partition in 1 2 3 4; do
    install -d -m 0755 -o root -g root \
      "$FIXTURE_BLOCK/nvme0n1/nvme0n1p${partition}" || return 1
    printf '%s\n' "$partition" \
      >"$FIXTURE_BLOCK/nvme0n1/nvme0n1p${partition}/partition" || return 1
    case "$partition" in
      1) uevent=$'DEVNAME=nvme0n1p1\nDEVTYPE=partition\nPARTNAME=EFI System Partition\nPART_ENTRY_TYPE=c12a7328-f81f-11d2-ba4b-00a0c93ec93b' ;;
      2) uevent=$'DEVNAME=nvme0n1p2\nDEVTYPE=partition\nTYPE=ntfs\nPARTLABEL=Windows' ;;
      3) uevent=$'DEVNAME=nvme0n1p3\nDEVTYPE=partition\nTYPE=apfs\nPARTLABEL=Macintosh HD' ;;
      4) uevent=$'DEVNAME=nvme0n1p4\nDEVTYPE=partition\nTYPE=crypto_LUKS\nPARTLABEL=Linux encrypted root' ;;
    esac
    printf '%s\n' "$uevent" \
      >"$FIXTURE_BLOCK/nvme0n1/nvme0n1p${partition}/uevent" || return 1
  done
  chmod -R a=rX,u+w "$FIXTURE_BLOCK" || return 1
}

fixture_start() {
  FIXTURE_SWAP_IN_PROGRESS=true
  if ! systemctl stop goblins-os-resident.service goblins-os-core.service; then
    fail_fixture_start stop-production
    return 1
  fi
  systemctl reset-failed goblins-hwgate-fixture-core.service \
    goblins-hwgate-fixture-resident.service >/dev/null 2>&1 || true

  if ! find "$FIXTURE_STATE" -mindepth 1 -delete; then
    fail_fixture_start clear-fixture-state
    return 1
  fi
  if ! install -d -m 0750 -o goblins-os -g goblins-os \
    "$FIXTURE_STATE/policy" \
    "$FIXTURE_STATE/apps" \
    "$FIXTURE_STATE/ai" \
    "$FIXTURE_STATE/installer" \
    "$FIXTURE_STATE/session" \
    "$FIXTURE_STATE/models" \
    "$FIXTURE_STATE/models/install-state" \
    "$FIXTURE_STATE/voice"; then
    fail_fixture_start create-fixture-state
    return 1
  fi
  if ! install -d -m 0750 -o goblins-resident -g goblins-core-resident \
    "$FIXTURE_STATE/resident"; then
    fail_fixture_start create-resident-state
    return 1
  fi
  if ! seed_fixture_block; then
    fail_fixture_start seed-fixture-block
    return 1
  fi
  if ! install -m 0644 -o root -g root /dev/null \
    "$RESULT_DIR/fixture-core.log" "$RESULT_DIR/fixture-resident.log"; then
    fail_fixture_start create-fixture-logs
    return 1
  fi

  if ! systemctl start goblins-hwgate-fixture-core.service; then
    fail_fixture_start start-fixture-core
    return 1
  fi
  if ! wait_for_proof_socket; then
    fail_fixture_start wait-fixture-core
    return 1
  fi
  if ! systemctl start goblins-hwgate-fixture-resident.service; then
    fail_fixture_start start-fixture-resident
    return 1
  fi
  if ! wait_for_fixture_resident; then
    fail_fixture_start wait-fixture-resident
    return 1
  fi
  publish_static_result fixture-start 200 \
    '{"ok":true,"transport":"fixed AF_UNIX capability sockets","tcp_private_routes":false}'
  FIXTURE_SWAP_IN_PROGRESS=false
}

fixture_restore() {
  FIXTURE_SWAP_IN_PROGRESS=true
  if restore_production_services; then
    publish_static_result fixture-restore 200 \
      '{"ok":true,"production_core_restored":true}'
  else
    publish_static_result fixture-restore 503 \
      '{"ok":false,"production_core_restored":false}'
    return 1
  fi
}

fixture_start_finished() {
  local body

  if [ "$OPERATION_RESULT" = "success" ] \
    && systemctl is-active --quiet goblins-hwgate-fixture-core.service \
    && systemctl is-active --quiet goblins-hwgate-fixture-resident.service \
    && [ -S "$FIXTURE_RESIDENT_SOCKET" ] \
    && wait_for_proof_socket; then
    return 0
  fi

  FIXTURE_SWAP_IN_PROGRESS=true
  if restore_production_services; then
    printf -v body \
      '{"ok":false,"text":"fixture start did not finish successfully","service_result":"%s","production_restored":true}' \
      "$OPERATION_RESULT"
  else
    printf -v body \
      '{"ok":false,"text":"fixture start did not finish successfully","service_result":"%s","production_restored":false}' \
      "$OPERATION_RESULT"
  fi
  publish_static_result fixture-start 503 "$body"
  [ "$FIXTURE_SWAP_IN_PROGRESS" = false ]
}

case "$OPERATION" in
  health) request health GET /health ;;
  audio-status) request audio-status GET /v1/audio/status ;;
  preview-status) request preview-status GET /v1/preview/status ;;
  firewall-status) request firewall-status GET /v1/firewall/status ;;
  firewall-disable) request firewall-disable POST /v1/firewall/enabled '{"enabled":false}' ;;
  firewall-enable) request firewall-enable POST /v1/firewall/enabled '{"enabled":true}' ;;
  text-shortcuts-seed) request text-shortcuts-seed POST /v1/text-shortcuts '{"shortcuts":[{"replace":"brb","with":"be right back"}]}' ;;
  text-shortcuts-set) request text-shortcuts-set POST /v1/text-shortcuts '{"shortcuts":[{"replace":"omw","with":"on my way"}]}' ;;
  text-shortcuts-status) request text-shortcuts-status GET /v1/text-shortcuts ;;
  text-shortcuts-preview) request text-shortcuts-preview GET '/v1/text-shortcuts/preview?trigger=omw' ;;
  text-shortcuts-file-contract) text_shortcuts_file_contract ;;
  keyboard-shortcut-set) request keyboard-shortcut-set POST /v1/keyboard/shortcuts/binding '{"action":"window-hud","bindings":["<Super><Shift>H"]}' ;;
  keyboard-shortcut-reset) request keyboard-shortcut-reset POST /v1/keyboard/shortcuts/binding '{"action":"window-hud","reset":true}' ;;
  keyboard-modifier-set) request keyboard-modifier-set POST /v1/keyboard/modifier-remap '{"target":"caps-lock","value":"control"}' ;;
  keyboard-modifier-reset) request keyboard-modifier-reset POST /v1/keyboard/modifier-remap '{"target":"caps-lock","value":"default"}' ;;
  input-sources-set) request input-sources-set POST /v1/input/sources '{"sources":[{"kind":"xkb","id":"us"},{"kind":"xkb","id":"gb"}]}' ;;
  input-switch-next) request input-switch-next POST /v1/input/switch-next ;;
  displays-status) request displays-status GET /v1/displays/status ;;
  display-apply-verify) request display-apply-verify POST /v1/displays/apply '' /tmp/gate-multi-display-verify-payload.json ;;
  display-apply-temporary) request display-apply-temporary POST /v1/displays/apply '' /tmp/gate-multi-display-temporary-payload.json ;;
  display-apply-persistent-guard) request display-apply-persistent-guard POST /v1/displays/apply '' /tmp/gate-multi-display-persistent-guard-payload.json ;;
  display-apply-stale) request display-apply-stale POST /v1/displays/apply '' /tmp/gate-multi-display-stale-payload.json ;;
  app-privacy-revoke) request app-privacy-revoke POST /v1/app-privacy/revoke '{"table":"location","id":"org.goblins.GatePrivacyProof","app":"org.goblins.GatePrivacyProof"}' ;;
  focus-mode-seed) request focus-mode-seed POST /v1/focus/mode '{"id":"gate-work","name":"Gate Work"}' ;;
  focus-status) request focus-status GET /v1/focus/status ;;
  focus-activate) request focus-activate POST /v1/focus/activate '{"mode":"gate-work"}' ;;
  focus-deactivate) request focus-deactivate POST /v1/focus/deactivate '{}' ;;
  preview-open-pdf) request preview-open-pdf POST /v1/preview/open '{"path":"/usr/share/goblins-os/proof/preview-open-render.pdf"}' ;;
  preview-open-image) request preview-open-image POST /v1/preview/open '{"path":"/usr/share/goblins-os/proof/preview-open-render.png"}' ;;
  preview-open-unsupported) request preview-open-unsupported POST /v1/preview/open '{"path":"/usr/share/goblins-os/proof/preview-open-render.txt"}' ;;
  policy-status) request policy-status GET /v1/policy/status ;;
  policy-grant-app-builder) grant_app_builder ;;
  app-build) request app-build POST /v1/apps/builds '{"intent":"A focus timer that counts down 25 minutes and rings."}' ;;
  fixture-start) fixture_start ;;
  fixture-restore) fixture_restore ;;
  fixture-start-finished) fixture_start_finished ;;
  *-finished) : ;;
  fixture-core-stopped)
    systemctl --no-block start goblins-os-core.service goblins-os-resident.service
    ;;
  *)
    echo "unsupported proof operation: $OPERATION" >&2
    exit 64
    ;;
esac
