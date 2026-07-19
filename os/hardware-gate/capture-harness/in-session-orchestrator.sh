#!/bin/bash
# Goblins OS hardware-gate in-session capture orchestrator (full 28-shot).
# Real captures of the real installed OS in the real VM. Gaming via the OS's own
# lavapipe/gamescope/pipewire software stack. Dual-boot uses a root-controlled
# fixture core swapped onto the exact production AF_UNIX capability sockets.
exec >/tmp/gate-cap.log 2>&1
set -x
LOCK_DIR=/tmp/goblins-hwgate-orchestrator.lock
if ! mkdir "$LOCK_DIR" 2>/dev/null; then
  echo "GOBLINS_HWGATE_ORCHESTRATOR_ALREADY_RUNNING"
  exit 0
fi
MODEL_LOOPBACK_PID=""
FIXTURE_ACTIVE=false
CORE_HEALTH_URL=http://127.0.0.1:8787/health
CORE_PROOF_RESULT_DIR=/run/goblins-hwgate-core-proof

core_proof_request(){
  local operation="$1"
  local output_file="$2"
  local unit="goblins-hwgate-core-proof@${operation}.service"
  local status_file="$CORE_PROOF_RESULT_DIR/${operation}.status"
  local body_file="$CORE_PROOF_RESULT_DIR/${operation}.json"

  if ! systemctl --no-ask-password --wait start "$unit"; then
    printf '{"ok":false,"text":"root proof operation failed: %s"}\n' "$operation" >"$output_file"
    printf '000\n'
    return 1
  fi
  if [ ! -r "$status_file" ] || [ ! -r "$body_file" ]; then
    printf '{"ok":false,"text":"root proof result missing: %s"}\n' "$operation" >"$output_file"
    printf '000\n'
    return 1
  fi
  cp "$body_file" "$output_file"
  tr -cd '0-9' <"$status_file" | cut -c1-3
}

restore_fixture_core(){
  if [ "$FIXTURE_ACTIVE" = "true" ]; then
    if core_proof_request fixture-restore /tmp/gate-fixture-restore.json >/dev/null 2>&1; then
      FIXTURE_ACTIVE=false
      return 0
    fi
    return 1
  fi
  return 0
}

cleanup(){
  restore_fixture_core
  if [ -n "${MODEL_LOOPBACK_PID:-}" ]; then
    kill "$MODEL_LOOPBACK_PID" 2>/dev/null || true
  fi
  rmdir "$LOCK_DIR" 2>/dev/null || true
}
trap cleanup EXIT
H=10.0.2.2:8099
B=/usr/libexec/goblins-os
TEXT_SHORTCUTS_INPUT_DRIVER=qmp-keyboard
TEXT_SHORTCUTS_IBUS_SERVICE=org.freedesktop.IBus.session.GNOME.service
export GDK_BACKEND=wayland
export XDG_RUNTIME_DIR="${XDG_RUNTIME_DIR:-/run/user/1000}"
export XDG_SESSION_TYPE="${XDG_SESSION_TYPE:-wayland}"
export WAYLAND_DISPLAY="${WAYLAND_DISPLAY:-wayland-0}"
export DISPLAY="${DISPLAY:-:0}"
export XDG_CURRENT_DESKTOP="${XDG_CURRENT_DESKTOP:-GNOME}"
export XDG_SESSION_DESKTOP="${XDG_SESSION_DESKTOP:-goblins-os}"
export DESKTOP_SESSION="${DESKTOP_SESSION:-goblins-os}"
# Maximize every captured GTK surface so the host QMP screendump catches it filling
# the work area (keeping window chrome + the menu bar/dock) instead of an ambiguous
# windowed surface that may not be foregrounded at screendump time — the root cause
# of the prior duplicate-capture plateau. Honest: a framebuffer read of the real
# maximized surface, no compositor/session change. Login + installer already
# fullscreen by design.
export GOBLINS_OS_RENDER_FULLSCREEN=1
sig(){
  curl --max-time "${GOS_READY_SIGNAL_TIMEOUT_SECONDS:-5}" -s "http://$H/ready/$1" >/dev/null 2>&1 || true
  sleep 5
}
proof_firewall(){ curl -s "http://$H/proof/firewall-live-toggle?$1" >/dev/null 2>&1 || true; }
proof_text_shortcuts(){ curl -s "http://$H/proof/text-shortcuts-session-enable?$1" >/dev/null 2>&1 || true; }
proof_text_shortcuts_candidate(){ curl -s "http://$H/proof/text-shortcuts-candidate-metadata?$1" >/dev/null 2>&1 || true; }
proof_text_shortcuts_overlay_intent(){ curl -s "http://$H/proof/text-shortcuts-overlay-intent?$1" >/dev/null 2>&1 || true; }
proof_text_shortcuts_candidate_bubble_frame(){ curl -s "http://$H/proof/text-shortcuts-candidate-bubble-frame?$1" >/dev/null 2>&1 || true; }
proof_text_shortcuts_candidate_bubble_layout(){ curl -s "http://$H/proof/text-shortcuts-candidate-bubble-layout?$1" >/dev/null 2>&1 || true; }
proof_text_shortcuts_candidate_bubble_render_intent(){ curl -s "http://$H/proof/text-shortcuts-candidate-bubble-render-intent?$1" >/dev/null 2>&1 || true; }
proof_text_shortcuts_candidate_bubble_render(){ curl -s "http://$H/proof/text-shortcuts-candidate-bubble-render?$1" >/dev/null 2>&1 || true; }
proof_text_shortcuts_live_ibus_runtime_render(){ curl -s "http://$H/proof/text-shortcuts-live-ibus-runtime-render?$1" >/dev/null 2>&1 || true; }
proof_keyboard_shortcuts_roundtrip(){ curl -s "http://$H/proof/keyboard-shortcuts-roundtrip?$1" >/dev/null 2>&1 || true; }
proof_input_sources_roundtrip(){ curl -s "http://$H/proof/input-sources-roundtrip?$1" >/dev/null 2>&1 || true; }
proof_multi_display_apply(){ curl -s "http://$H/proof/multi-display-apply?$1" >/dev/null 2>&1 || true; }
proof_focus_arm_roundtrip(){ curl -s "http://$H/proof/focus-arm-roundtrip?$1" >/dev/null 2>&1 || true; }
proof_app_privacy_revoke(){ curl -s "http://$H/proof/app-privacy-revoke?$1" >/dev/null 2>&1 || true; }
proof_preview_open_render(){ curl -s "http://$H/proof/preview-open-render?$1" >/dev/null 2>&1 || true; }
proof_audio_output(){ curl -s "http://$H/proof/audio-output?$1" >/dev/null 2>&1 || true; }
proof_runtime_build(){ curl -s "http://$H/proof/runtime-build?$1" >/dev/null 2>&1 || true; }
proof_query_value(){
  python3 - "$1" <<'PY'
import sys
from urllib.parse import quote

value = sys.argv[1].replace("\r", " ").replace("\n", " ").replace("\t", " ")
print(quote(value[:220], safe="._:-"))
PY
}
file_size_value(){
  if [ -e "$1" ]; then
    wc -c < "$1" 2>/dev/null | tr -d '[:space:]'
  else
    printf 'missing'
  fi
}
file_tail_query_value(){
  proof_query_value "$(tail -n 30 "$1" 2>/dev/null || true)"
}
start_capture_model_loopback(){
  rm -f /tmp/model-loopback.log /tmp/model-loopback-tags.json /tmp/model-loopback-tags.err
  python3 - <<'PY' >/tmp/model-loopback.log 2>&1 &
import socket
import threading

LISTEN = ("127.0.0.1", 41134)
TARGET = ("10.0.2.2", 11434)

def close(sock):
    try:
        sock.shutdown(socket.SHUT_RDWR)
    except OSError:
        pass
    try:
        sock.close()
    except OSError:
        pass

def pump(src, dst):
    try:
        while True:
            data = src.recv(65536)
            if not data:
                break
            dst.sendall(data)
    except OSError:
        pass
    finally:
        close(src)
        close(dst)

def handle(client):
    try:
        upstream = socket.create_connection(TARGET, timeout=10)
    except OSError as exc:
        print(f"connect failed: {exc}", flush=True)
        close(client)
        return
    threading.Thread(target=pump, args=(client, upstream), daemon=True).start()
    threading.Thread(target=pump, args=(upstream, client), daemon=True).start()

listener = socket.socket(socket.AF_INET, socket.SOCK_STREAM)
listener.setsockopt(socket.SOL_SOCKET, socket.SO_REUSEADDR, 1)
listener.bind(LISTEN)
listener.listen(32)
print(f"forwarding {LISTEN[0]}:{LISTEN[1]} to {TARGET[0]}:{TARGET[1]}", flush=True)
while True:
    client, _ = listener.accept()
    threading.Thread(target=handle, args=(client,), daemon=True).start()
PY
  MODEL_LOOPBACK_PID=$!
  for _ in $(seq 1 20); do
    if curl -sf http://127.0.0.1:41134/api/tags >/tmp/model-loopback-tags.json 2>/tmp/model-loopback-tags.err; then
      return 0
    fi
    sleep 1
  done
  echo "GOBLINS_HWGATE_MODEL_LOOPBACK_NOT_READY"
  tail -n 20 /tmp/model-loopback.log 2>/dev/null || true
  tail -n 20 /tmp/model-loopback-tags.err 2>/dev/null || true
  return 1
}
start_capture_model_contract_relay(){
  rm -f /tmp/model-contract.log /tmp/model-contract-direct.json /tmp/model-contract-direct.err
  CAPTURE_LOCAL_MODEL="$CAPTURE_LOCAL_MODEL" \
  CAPTURE_MODEL_KEEP_ALIVE="$CAPTURE_MODEL_KEEP_ALIVE" \
  python3 - <<'PY' >/tmp/model-contract.log 2>&1 &
import http.client
import json
import os
from http.server import BaseHTTPRequestHandler, ThreadingHTTPServer

LISTEN = ("127.0.0.1", 41135)
TARGET = ("10.0.2.2", 11434)
MODEL = os.environ["CAPTURE_LOCAL_MODEL"]
KEEP_ALIVE = os.environ["CAPTURE_MODEL_KEEP_ALIVE"]
MAX_BODY = 1024 * 1024

class Handler(BaseHTTPRequestHandler):
    def log_message(self, fmt, *args):
        print(fmt % args, flush=True)

    def send_json(self, status, payload):
        body = json.dumps(payload).encode("utf-8")
        self.send_response(status)
        self.send_header("content-type", "application/json")
        self.send_header("content-length", str(len(body)))
        self.end_headers()
        self.wfile.write(body)

    def do_POST(self):
        if self.path != "/v1/resident":
            self.send_json(404, {"text": ""})
            return
        try:
            length = int(self.headers.get("content-length", "0"))
        except ValueError:
            self.send_json(400, {"text": ""})
            return
        if length <= 0 or length > MAX_BODY:
            self.send_json(413, {"text": ""})
            return
        try:
            request = json.loads(self.rfile.read(length))
            message = str(request.get("message", "")).strip()
        except Exception as exc:
            print(f"request parse failed: {exc}", flush=True)
            self.send_json(400, {"text": ""})
            return
        if not message:
            self.send_json(400, {"text": ""})
            return
        payload = json.dumps({
            "model": MODEL,
            "prompt": message,
            "stream": False,
            "keep_alive": KEEP_ALIVE,
        }).encode("utf-8")
        conn = None
        try:
            conn = http.client.HTTPConnection(TARGET[0], TARGET[1], timeout=180)
            conn.request(
                "POST",
                "/api/generate",
                body=payload,
                headers={"content-type": "application/json"},
            )
            response = conn.getresponse()
            response_body = response.read()
            if response.status < 200 or response.status >= 300:
                print(
                    f"ollama status={response.status} body_tail={response_body[-400:].decode('utf-8', 'replace')}",
                    flush=True,
                )
                self.send_json(502, {"text": ""})
                return
            reply = json.loads(response_body)
            text = str(reply.get("response", "")).strip()
            if not text:
                print("ollama response was empty", flush=True)
                self.send_json(502, {"text": ""})
                return
            self.send_json(200, {"text": text})
        except Exception as exc:
            print(f"ollama request failed: {exc}", flush=True)
            self.send_json(502, {"text": ""})
        finally:
            if conn is not None:
                conn.close()

server = ThreadingHTTPServer(LISTEN, Handler)
print(f"contract relay {LISTEN[0]}:{LISTEN[1]} to {TARGET[0]}:{TARGET[1]} model={MODEL}", flush=True)
server.serve_forever()
PY
  MODEL_CONTRACT_PID=$!
  for _ in $(seq 1 20); do
    if curl -sf -X POST http://127.0.0.1:41135/v1/resident \
      -H 'content-type: application/json' \
      -d '{"message":"Reply with READY."}' >/tmp/model-contract-direct.json 2>/tmp/model-contract-direct.err; then
      return 0
    fi
    sleep 1
  done
  echo "GOBLINS_HWGATE_MODEL_CONTRACT_NOT_READY"
  tail -n 20 /tmp/model-contract.log 2>/dev/null || true
  tail -n 20 /tmp/model-contract-direct.err 2>/dev/null || true
  return 1
}
json_string_literal(){
  python3 - "$1" <<'PY'
import json
import sys

print(json.dumps(sys.argv[1]))
PY
}
# GNOME ships org.gnome.Shell.Eval disabled, so window titles cannot be read
# from the shell. Surfaces that support GOBLINS_OS_CAPTURE_PRESENT_LEDGER write
# their real mapped title from their own frame clock; this waits on that file.
wait_for_present_ledger(){
  local title="$1"
  local attempts="${2:-40}"
  local ledger="${GOBLINS_OS_CAPTURE_PRESENT_LEDGER:-}"
  [ -n "$ledger" ] || return 1
  for _ in $(seq 1 "$attempts"); do
    if [ -s "$ledger" ] && python3 - "$ledger" "$title" <<'PY'
import json
import sys

try:
    data = json.load(open(sys.argv[1], encoding="utf-8"))
except Exception:
    raise SystemExit(1)
raise SystemExit(0 if data.get("title") == sys.argv[2] else 1)
PY
    then
      return 0
    fi
    sleep 1
  done
  return 1
}
wait_for_window_title(){
  local title="$1"
  local attempts="${2:-40}"
  local helper_timeout="${GOS_SHOT_HELPER_TIMEOUT_SECONDS:-3}"
  local js_title script out

  js_title="$(json_string_literal "$title")"
  for _ in $(seq 1 "$attempts"); do
    if command -v gdbus >/dev/null 2>&1; then
      script="(() => { const expected = $js_title; for (const actor of global.get_window_actors()) { const w = actor.meta_window; if (!w) continue; const t = String(w.get_title ? w.get_title() : ''); if (t.includes(expected) && !w.minimized) { w.activate(global.get_current_time()); return 'found'; } } return 'missing'; })();"
      out="$(gdbus call --session \
        --timeout "$helper_timeout" \
        --dest org.gnome.Shell \
        --object-path /org/gnome/Shell \
        --method org.gnome.Shell.Eval \
        "$script" 2>/dev/null || true)"
      case "$out" in
        *found*) return 0 ;;
      esac
    fi
    sleep 0.5
  done
  return 1
}
ibus_session_bus_owned(){
  command -v gdbus >/dev/null 2>&1 \
    && gdbus call --session \
      --dest org.freedesktop.DBus \
      --object-path /org/freedesktop/DBus \
      --method org.freedesktop.DBus.NameHasOwner \
      org.freedesktop.IBus 2>/dev/null | grep -Fq "true"
}
ibus_bus_owner_value(){
  if ibus_session_bus_owned; then
    printf 'true'
  else
    printf 'false'
  fi
}
wait_ibus_bus_owned(){
  local attempts="${1:-80}"
  for _ in $(seq 1 "$attempts"); do
    ibus_session_bus_owned && return 0
    sleep 0.5
  done
  return 1
}
ibus_service_diag_query_value(){
  proof_query_value "$(systemctl --user show "$TEXT_SHORTCUTS_IBUS_SERVICE" -p Type -p ActiveState -p SubState -p Result -p MainPID -p ExecMainStatus 2>/dev/null | tr '\n' ' ')"
}
ibus_daemon_process_query_value(){
  proof_query_value "$(pgrep -af 'ibus-daemon' 2>/dev/null | head -n 3 | tr '\n' ';')"
}
ibus_session_env_query_value(){
  proof_query_value "session_type=${XDG_SESSION_TYPE:-missing} wayland_display=${WAYLAND_DISPLAY:+present} display=${DISPLAY:+present} dbus_session_bus=${DBUS_SESSION_BUS_ADDRESS:+present}"
}
host_type_text(){
  local token="$1"
  local text="$2"
  curl -s "http://$H/input/text/$token?text=$(proof_query_value "$text")" >/dev/null 2>&1 || true
  sleep 1
}
host_click(){
  local token="$1"
  local x="${2:-0.5}"
  local y="${3:-0.5}"
  curl -s "http://$H/input/click/$token?x=$x&y=$y" >/dev/null 2>&1 || true
  sleep 0.5
}
host_focus_text_shortcuts_field(){
  local token="$1"
  host_click "${token}-window" 0.5 0.5
  host_click "${token}-entry-a" 0.5 0.53
  host_click "${token}-entry-b" 0.5 0.56
  sleep 0.25
}
host_press_key(){
  local token="$1"
  local key_name="$2"
  curl -s "http://$H/input/key/$token?key=$(proof_query_value "$key_name")" >/dev/null 2>&1 || true
  sleep 0.5
}
dismiss_shell_overview(){
  local token="$1"
  # The installed session can be left in GNOME overview/search after first boot.
  # Dismiss it before typing into proof windows so QMP keyboard events target
  # the foreground GTK entry instead of the shell search field.
  host_press_key "${token}-escape-a" Escape
  host_press_key "${token}-escape-b" Escape
  sleep 0.5
}
run_bounded_quiet(){
  local seconds="$1"
  shift
  if [ -z "$seconds" ] || [ "$#" -eq 0 ]; then
    return 0
  fi
  if command -v timeout >/dev/null 2>&1; then
    timeout -k 2s "${seconds}s" "$@" >/dev/null 2>&1
    local rc=$?
    if [ "$rc" -eq 124 ] || [ "$rc" -eq 137 ]; then
      echo "GOBLINS_HWGATE_BOUNDED_COMMAND_TIMED_OUT seconds=$seconds command=$*"
    fi
    return "$rc"
  fi

  "$@" >/dev/null 2>&1 &
  local bounded_pid=$!
  local waited=0
  while kill -0 "$bounded_pid" 2>/dev/null; do
    if [ "$waited" -ge "$seconds" ]; then
      echo "GOBLINS_HWGATE_BOUNDED_COMMAND_TIMED_OUT seconds=$seconds command=$*"
      kill "$bounded_pid" 2>/dev/null || true
      sleep 0.2
      kill -9 "$bounded_pid" 2>/dev/null || true
      wait "$bounded_pid" 2>/dev/null || true
      return 124
    fi
    sleep 1
    waited=$((waited + 1))
  done
  wait "$bounded_pid" 2>/dev/null
}
switch_control_off(){
  local helper_timeout="${GOS_SHOT_HELPER_TIMEOUT_SECONDS:-3}"
  run_bounded_quiet "$helper_timeout" gsettings set org.goblins.os.a11y.switch-control enabled false || true
  run_bounded_quiet "$helper_timeout" gdbus call --session \
    --timeout "$helper_timeout" \
    --dest org.gnome.Shell \
    --object-path /org/gnome/Shell \
    --method org.gnome.Shell.Eval \
    "if (globalThis.goblinsSwitchControl) globalThis.goblinsSwitchControl.hide(); 'switch-control-hidden';" || true
  if command -v gnome-extensions >/dev/null 2>&1; then
    run_bounded_quiet "$helper_timeout" gnome-extensions disable goblins-switch@goblins.os || true
  fi
  sleep 0.8
}
json_field(){
  python3 - "$1" "$2" <<'PY'
import json
import sys

try:
    value = json.load(open(sys.argv[1], encoding="utf-8"))
    for part in sys.argv[2].split("."):
        value = value[part]
    if isinstance(value, bool):
        print("true" if value else "false")
    else:
        print(value)
except Exception:
    print("")
PY
}
grant_policy_permission(){
  local control_id="$1"
  local grant_file="$2"
  local status_file="$3"
  local status_http grant_http grant_ok

  [ "$control_id" = "app-builder" ] || return 64
  status_http="$(core_proof_request policy-status "$status_file" || true)"
  [ "$status_http" = "200" ] || return 1
  grant_http="$(core_proof_request policy-grant-app-builder "$grant_file" || true)"
  grant_ok="$(json_field "$grant_file" ok)"
  [ "$grant_http" = "200" ] && [ "$grant_ok" = "true" ]
}
gsettings_string_value(){
  python3 - "$1" <<'PY'
import ast
import sys

try:
    value = ast.literal_eval(sys.argv[1])
    print(value if isinstance(value, str) else "")
except Exception:
    print("")
PY
}
json_path_payload(){
  python3 - "$1" <<'PY'
import json
import sys

print(json.dumps({"path": sys.argv[1]}))
PY
}
wait_process(){
  local process="$1"
  for _ in $(seq 1 30); do
    pgrep -x "$process" >/dev/null 2>&1 && return 0
    sleep 0.5
  done
  return 1
}
wait_process_or_bus(){
  local process="$1"
  local bus_name="$2"
  for _ in $(seq 1 30); do
    pgrep -x "$process" >/dev/null 2>&1 && return 0
    pgrep -f "$process" >/dev/null 2>&1 && return 0
    if [ -n "$bus_name" ] && command -v gdbus >/dev/null 2>&1 \
      && gdbus call --session \
        --dest org.freedesktop.DBus \
        --object-path /org/freedesktop/DBus \
        --method org.freedesktop.DBus.NameHasOwner \
        "$bus_name" 2>/dev/null | grep -Fq "true"; then
      return 0
    fi
    sleep 0.5
  done
  return 1
}
wait_session_bus_name(){
  local bus_name="$1"
  if command -v gdbus >/dev/null 2>&1; then
    gdbus call --session \
      --dest org.freedesktop.DBus \
      --object-path /org/freedesktop/DBus \
      --method org.freedesktop.DBus.StartServiceByName \
      "$bus_name" 0 >/dev/null 2>&1 || true
  fi
  for _ in $(seq 1 30); do
    if command -v gdbus >/dev/null 2>&1 \
      && gdbus call --session \
        --dest org.freedesktop.DBus \
        --object-path /org/freedesktop/DBus \
        --method org.freedesktop.DBus.NameHasOwner \
        "$bus_name" 2>/dev/null | grep -Fq "true"; then
      return 0
    fi
    sleep 0.5
  done
  return 1
}
active_ibus_engine(){
  ibus engine 2>/dev/null | tr -d '\n' || true
}
wait_ibus_cli_ready(){
  local out_file="$1"
  local err_file="$2"
  local attempts="${3:-80}"
  : > "$out_file"
  : > "$err_file"
  for _ in $(seq 1 "$attempts"); do
    wait_ibus_bus_owned 1 || true
    if ibus list-engine >"$out_file" 2>"$err_file"; then
      return 0
    fi
    sleep 0.5
  done
  return 1
}
ensure_textshortcuts_ibus_component(){
  local user_component_dir="${XDG_DATA_HOME:-$HOME/.local/share}/ibus/component"
  mkdir -p "$user_component_dir"
  if [ -f /usr/share/ibus/component/goblins-textshortcuts.xml ]; then
    cp -f /usr/share/ibus/component/goblins-textshortcuts.xml "$user_component_dir/goblins-textshortcuts.xml"
  fi
}
activate_goblins_textshortcuts_engine(){
  local active_engine list_out=/tmp/gate-text-shortcuts-activate-list-engine.out list_err=/tmp/gate-text-shortcuts-activate-list-engine.err
  for _ in $(seq 1 40); do
    ensure_textshortcuts_ibus_component
    ibus write-cache >/dev/null 2>&1 || true
    wait_ibus_cli_ready "$list_out" "$list_err" 4 || true
    if ! grep -Fq 'goblins-textshortcuts' "$list_out" 2>/dev/null; then
      sleep 0.5
      continue
    fi
    ibus engine goblins-textshortcuts >/dev/null 2>&1 || true
    active_engine="$(active_ibus_engine)"
    [ "$active_engine" = "goblins-textshortcuts" ] && return 0
    sleep 0.5
  done
  return 1
}
wait_proof_file_nonempty(){
  local proof_file="$1"
  local attempts="${2:-40}"
  for _ in $(seq 1 "$attempts"); do
    [ -s "$proof_file" ] && return 0
    sleep 0.25
  done
  return 1
}
generate_audio_probe_wav(){
  local wav="$1"
  python3 - "$wav" <<'PY'
import math
import struct
import sys
import wave

path = sys.argv[1]
sample_rate = 48000
seconds = 45
amplitude = 0.18
with wave.open(path, "wb") as out:
    out.setnchannels(2)
    out.setsampwidth(2)
    out.setframerate(sample_rate)
    one_second = bytearray()
    for i in range(sample_rate):
        # Two quiet tones make the proof audibly distinct without being harsh.
        value = int(32767 * amplitude * (
            math.sin(2 * math.pi * 440 * i / sample_rate) * 0.65
            + math.sin(2 * math.pi * 660 * i / sample_rate) * 0.35
        ))
        one_second.extend(struct.pack("<hh", value, value))
    for _ in range(seconds):
        out.writeframesraw(one_second)
PY
}
generate_audio_probe_wav_bounded(){
  local wav="$1"
  local seconds="${2:-10}"
  local waited=0
  local generator_pid

  rm -f /tmp/gate-audio-output-generate.log "$wav"
  generate_audio_probe_wav "$wav" >/tmp/gate-audio-output-generate.log 2>&1 &
  generator_pid=$!
  while kill -0 "$generator_pid" 2>/dev/null; do
    if [ "$waited" -ge "$seconds" ]; then
      echo "GOBLINS_HWGATE_AUDIO_WAV_GENERATION_TIMED_OUT seconds=$seconds"
      kill "$generator_pid" 2>/dev/null || true
      sleep 0.2
      kill -9 "$generator_pid" 2>/dev/null || true
      wait "$generator_pid" 2>/dev/null || true
      return 124
    fi
    sleep 1
    waited=$((waited + 1))
  done

  if ! wait "$generator_pid" 2>/dev/null; then
    echo "GOBLINS_HWGATE_AUDIO_WAV_GENERATION_FAILED"
    return 1
  fi
  [ -s "$wav" ]
}
audio_status_http_code(){
  local status_file="$1"
  : >"$status_file"
  core_proof_request audio-status "$status_file" || true
}
audio_core_restart_count(){
  timeout 3 systemctl show goblins-os-core -p NRestarts --value 2>/dev/null \
    | head -n 1 | tr -cd '0-9' | cut -c1-8
}
audio_core_service_diag(){
  # Unprivileged, bounded diagnostics for a failing /v1/audio/status probe: a
  # second core route plus the systemd unit state distinguish "core daemon down
  # (and why)" from "the audio route alone timing out".
  local probe_http diag state key value
  probe_http=$(core_proof_request preview-status /tmp/gate-preview-diagnostic.json || true)
  diag="core_probe_route=/v1/preview/status&core_probe_http=${probe_http:-000}"
  state=$(timeout 3 systemctl show goblins-os-core \
    -p ActiveState,SubState,Result,NRestarts,ExecMainCode,ExecMainStatus 2>/dev/null || true)
  for key in ActiveState:core_active SubState:core_substate Result:core_result \
    NRestarts:core_restarts ExecMainCode:core_exec_code ExecMainStatus:core_exec_status; do
    value=$(printf '%s\n' "$state" | sed -n "s/^${key%%:*}=//p" | head -n 1 \
      | tr -cd 'A-Za-z0-9._-' | cut -c1-48)
    diag="$diag&${key##*:}=${value:-unknown}"
  done
  state=$(timeout 3 systemctl --user show org.goblins.OS.SessionBridge.service \
    -p ActiveState,SubState,Result,NRestarts 2>/dev/null || true)
  for key in ActiveState:bridge_active SubState:bridge_substate \
    Result:bridge_result NRestarts:bridge_restarts; do
    value=$(printf '%s\n' "$state" | sed -n "s/^${key%%:*}=//p" | head -n 1 \
      | tr -cd 'A-Za-z0-9._-' | cut -c1-48)
    diag="$diag&${key##*:}=${value:-unknown}"
  done
  value=$(awk '/MemAvailable:/ {print int($2/1024)}' /proc/meminfo 2>/dev/null)
  diag="$diag&mem_available_mb=${value:-unknown}"
  value=$(df -Pm /var 2>/dev/null | awk 'NR==2 {print $4}')
  diag="$diag&var_avail_mb=${value:-unknown}"
  if [ -s /tmp/gate-audio-sound-present.json ]; then
    diag="$diag&present_ledger=$(proof_query_value "$(cat /tmp/gate-audio-sound-present.json 2>/dev/null || true)")"
  else
    diag="$diag&present_ledger=absent"
  fi
  diag="$diag&shot_log_tail=$(file_tail_query_value /tmp/gate-shot-24-audio-output.log)"
  printf '%s' "$diag"
}
audio_output_status_ready(){
  local status_file="$1"
  local status_code output_available wireplumber_available

  status_code=$(audio_status_http_code "$status_file")
  output_available=$(json_field "$status_file" output.available)
  wireplumber_available=$(json_field "$status_file" wireplumber_available)

  [ "$status_code" = "200" ] && [ "$output_available" = "true" ] && [ "$wireplumber_available" = "true" ]
}
audio_output_shot(){
  local status_file=/tmp/gate-audio-output-status.json
  local wav=/tmp/gate-audio-output-proof.wav
  local player="" player_pid="" audio_ready=false player_started=false
  local status_code output_available output_volume output_muted wireplumber_available
  local rendered_sound_panel=false
  local wav_generated=false failure_stage=audio-output-preflight
  local status_attempts="${GOS_AUDIO_STATUS_ATTEMPTS:-8}"

  echo "GOBLINS_HWGATE_AUDIO_PROOF_START"
  for _ in $(seq 1 "$status_attempts"); do
    if audio_output_status_ready "$status_file"; then
      audio_ready=true
      break
    fi
    sleep 0.5
  done

  status_code=$(audio_status_http_code "$status_file")
  output_available=$(json_field "$status_file" output.available)
  output_volume=$(json_field "$status_file" output.volume_percent)
  output_muted=$(json_field "$status_file" output.muted)
  wireplumber_available=$(json_field "$status_file" wireplumber_available)
  if [ "$audio_ready" != "true" ]; then
    echo "GOBLINS_HWGATE_AUDIO_STATUS_ATTEMPTS_EXHAUSTED attempts=$status_attempts status_http=${status_code:-000}"
    failure_stage=audio-status
  fi

  if command -v pw-play >/dev/null 2>&1; then
    player="pw-play"
  elif command -v paplay >/dev/null 2>&1; then
    player="paplay"
  fi

  if [ -n "$player" ]; then
    if generate_audio_probe_wav_bounded "$wav" "${GOS_AUDIO_WAV_TIMEOUT_SECONDS:-10}"; then
      wav_generated=true
      "$player" "$wav" >/tmp/gate-audio-output-play.log 2>&1 &
      player_pid=$!
      sleep 1
      if kill -0 "$player_pid" 2>/dev/null; then
        player_started=true
      else
        failure_stage=audio-player-start
      fi
    else
      failure_stage=audio-wav-generation
    fi
  else
    failure_stage=audio-player-missing
  fi

  rm -f /tmp/gate-audio-sound-present.json
  if GOBLINS_OS_CAPTURE_EXPECT_TITLE="Goblins OS Settings - Sound" \
    GOBLINS_OS_CAPTURE_PRESENT_LEDGER=/tmp/gate-audio-sound-present.json \
    GOS_SHOT_WINDOW_WAIT_ATTEMPTS="${GOS_AUDIO_SHOT_WINDOW_WAIT_ATTEMPTS:-8}" \
    GOS_SHOT_HELPER_TIMEOUT_SECONDS="${GOS_AUDIO_SHOT_HELPER_TIMEOUT_SECONDS:-1}" \
    GOBLINS_OS_SETTINGS_CORE_WAIT_SECS="${GOS_SETTINGS_CAPTURE_CORE_WAIT_SECS:-8}" \
    shot 24-audio-output "$B/goblins-os-settings" --panel=sound; then
    rendered_sound_panel=true
  fi

  if [ "$audio_ready" = "true" ] && [ "$player_started" = "true" ] && [ "$rendered_sound_panel" = "true" ]; then
    local core_restarts_now
    core_restarts_now=$(audio_core_restart_count)
    proof_audio_output "status=pass&status_route=/v1/audio/status&status_http=200&wireplumber_available=true&output_available=true&output_volume=${output_volume:-unknown}&output_muted=${output_muted:-unknown}&player=$player&test_tone_seconds=45&screenshot=24-audio-output.png&rendered_sound_panel=true&core_restarts=${core_restarts_now:-unknown}"
  else
    if [ "$rendered_sound_panel" != "true" ] && [ "$failure_stage" = "audio-output-preflight" ]; then
      failure_stage=audio-sound-panel-render
    fi
    proof_audio_output "status=fail&stage=$failure_stage&status_route=/v1/audio/status&status_http=${status_code:-000}&wireplumber_available=${wireplumber_available:-missing}&output_available=${output_available:-missing}&output_volume=${output_volume:-missing}&output_muted=${output_muted:-missing}&player=${player:-missing}&wav_generated=$wav_generated&player_started=$player_started&screenshot=24-audio-output.png&rendered_sound_panel=$rendered_sound_panel&generate_log_tail=$(file_tail_query_value /tmp/gate-audio-output-generate.log)&play_log_tail=$(file_tail_query_value /tmp/gate-audio-output-play.log)&$(audio_core_service_diag)"
  fi

  if [ -n "$player_pid" ]; then
    kill "$player_pid" 2>/dev/null || true
    wait "$player_pid" 2>/dev/null || true
  fi
  rm -f "$wav"
}
firewall_live_toggle_proof(){
  local status_file=/tmp/gate-firewall-status.json
  local disable_file=/tmp/gate-firewall-disable.json
  local enable_file=/tmp/gate-firewall-enable.json
  local status_code disable_code enable_code before_available before_manageable
  local disable_ok disable_enabled disable_active enable_ok enable_enabled enable_active
  local disable_text enable_text

  for _ in $(seq 1 60); do
    curl -sf "$CORE_HEALTH_URL" >/dev/null 2>&1 && break
    sleep 0.5
  done

  status_code=$(core_proof_request firewall-status "$status_file" || true)
  before_available=$(json_field "$status_file" available)
  before_manageable=$(json_field "$status_file" manageable)
  if [ "$status_code" != "200" ] || [ "$before_available" != "true" ] || [ "$before_manageable" != "true" ]; then
    proof_firewall "status=fail&stage=status&status_http=${status_code:-000}&available=${before_available:-missing}&manageable=${before_manageable:-missing}"
    return 1
  fi

  disable_code=$(core_proof_request firewall-disable "$disable_file" || true)
  disable_ok=$(json_field "$disable_file" ok)
  disable_enabled=$(json_field "$disable_file" enabled)
  disable_text=$(json_field "$disable_file" text)
  sleep 2
  core_proof_request firewall-status "$status_file" >/dev/null 2>&1 || true
  disable_active=$(json_field "$status_file" active)

  enable_code=$(core_proof_request firewall-enable "$enable_file" || true)
  enable_ok=$(json_field "$enable_file" ok)
  enable_enabled=$(json_field "$enable_file" enabled)
  enable_text=$(json_field "$enable_file" text)
  sleep 2
  core_proof_request firewall-status "$status_file" >/dev/null 2>&1 || true
  enable_active=$(json_field "$status_file" active)

  if [ "$disable_code" = "200" ] && [ "$disable_ok" = "true" ] && [ "$disable_enabled" = "false" ] && [ "$disable_active" = "false" ] \
    && [ "$enable_code" = "200" ] && [ "$enable_ok" = "true" ] && [ "$enable_enabled" = "true" ] && [ "$enable_active" = "true" ]; then
    proof_firewall "status=pass&route=/v1/firewall/enabled&status_route=/v1/firewall/status&disable_http=200&disable_ok=true&disable_enabled=false&disable_active=false&enable_http=200&enable_ok=true&enable_enabled=true&enable_active=true&unit_template=goblins-os-firewall@.service&polkit_rule=60-goblins-os-firewall.rules"
    return 0
  fi

  proof_firewall "status=fail&stage=toggle&disable_http=${disable_code:-000}&disable_ok=${disable_ok:-missing}&disable_enabled=${disable_enabled:-missing}&disable_active=${disable_active:-missing}&disable_text=$(proof_query_value "${disable_text:-missing}")&enable_http=${enable_code:-000}&enable_ok=${enable_ok:-missing}&enable_enabled=${enable_enabled:-missing}&enable_active=${enable_active:-missing}&enable_text=$(proof_query_value "${enable_text:-missing}")"
  return 1
}
text_shortcuts_session_enable_proof(){
  local core_file=/tmp/gate-text-shortcuts-core.json
  local service_state input_sources preload_engines core_code core_engine_available core_runtime_loop
  local input_source_configured preload_configured engine_listed adapter_self_test active_engine engine_set

  ensure_textshortcuts_ibus_component
  systemctl --user import-environment DISPLAY WAYLAND_DISPLAY XDG_SESSION_TYPE XDG_CURRENT_DESKTOP XDG_SESSION_DESKTOP DESKTOP_SESSION DBUS_SESSION_BUS_ADDRESS 2>/dev/null || true
  dbus-update-activation-environment --systemd DISPLAY WAYLAND_DISPLAY XDG_SESSION_TYPE XDG_CURRENT_DESKTOP XDG_SESSION_DESKTOP DESKTOP_SESSION DBUS_SESSION_BUS_ADDRESS 2>/dev/null || true
  ibus write-cache >/tmp/gate-text-shortcuts-session-write-cache.log 2>&1 || true
  systemctl --user reset-failed "$TEXT_SHORTCUTS_IBUS_SERVICE" >/tmp/gate-text-shortcuts-session-ibus-reset-failed.log 2>&1 || true
  systemctl --user restart "$TEXT_SHORTCUTS_IBUS_SERVICE" >/tmp/gate-text-shortcuts-session-ibus-restart.log 2>&1 || true
  for _ in $(seq 1 80); do
    service_state="$(systemctl --user is-active "$TEXT_SHORTCUTS_IBUS_SERVICE" 2>/dev/null || true)"
    if [ "$service_state" = "active" ] && wait_ibus_bus_owned 1; then
      break
    fi
    sleep 0.5
  done

  if [ "$service_state" != "active" ]; then
    proof_text_shortcuts "status=fail&stage=user-service&service=${service_state:-missing}&service_unit=$TEXT_SHORTCUTS_IBUS_SERVICE&cache_refreshed=true&daemon_restarted=true&user_component_seeded=true&bus_owner=$(ibus_bus_owner_value)&service_diag=$(ibus_service_diag_query_value)&daemon_process=$(ibus_daemon_process_query_value)&session_env=$(ibus_session_env_query_value)"
    return 1
  fi

  input_sources="$(gsettings get org.gnome.desktop.input-sources sources 2>/dev/null || true)"
  preload_engines="$(gsettings get org.freedesktop.ibus.general preload-engines 2>/dev/null || true)"
  case "$input_sources" in *"('ibus', 'goblins-textshortcuts')"*) input_source_configured=true;; *) input_source_configured=false;; esac
  case "$preload_engines" in *"'goblins-textshortcuts'"*) preload_configured=true;; *) preload_configured=false;; esac
  if [ "$input_source_configured" != "true" ] || [ "$preload_configured" != "true" ]; then
    proof_text_shortcuts "status=fail&stage=dconf&service=active&input_source_configured=$input_source_configured&preload_configured=$preload_configured"
    return 1
  fi

  engine_listed=false
  for _ in $(seq 1 40); do
    ensure_textshortcuts_ibus_component
    ibus write-cache >/tmp/gate-text-shortcuts-session-write-cache.log 2>&1 || true
    if wait_ibus_cli_ready /tmp/gate-text-shortcuts-session-list-engine.out /tmp/gate-text-shortcuts-session-list-engine.err 4 \
      && grep -Fq 'goblins-textshortcuts' /tmp/gate-text-shortcuts-session-list-engine.out; then
      engine_listed=true
      break
    fi
    sleep 0.5
  done
  if [ "$engine_listed" != "true" ]; then
    proof_text_shortcuts "status=fail&stage=engine-list&service=${service_state:-missing}&input_source_configured=true&preload_configured=true&engine_listed=false&cache_refreshed=true&daemon_restarted=true&user_component_seeded=true&bus_owner=$(ibus_bus_owner_value)&list_error=$(proof_query_value "$(cat /tmp/gate-text-shortcuts-session-list-engine.err 2>/dev/null || true)")&service_diag=$(ibus_service_diag_query_value)&daemon_process=$(ibus_daemon_process_query_value)&session_env=$(ibus_session_env_query_value)"
    return 1
  fi

  if /usr/libexec/goblins-os/goblins-textshortcuts-ibus --self-test >/dev/null 2>&1; then
    adapter_self_test=pass
  else
    adapter_self_test=fail
  fi
  if [ "$adapter_self_test" != "pass" ]; then
    proof_text_shortcuts "status=fail&stage=adapter-self-test&service=active&engine_listed=true&adapter_self_test=fail"
    return 1
  fi

  if activate_goblins_textshortcuts_engine; then
    engine_set=pass
  else
    engine_set=fail
  fi
  active_engine="$(active_ibus_engine)"
  if [ "$engine_set" != "pass" ] || [ "$active_engine" != "goblins-textshortcuts" ]; then
    proof_text_shortcuts "status=fail&stage=active-engine&service=active&engine_set=$engine_set&active_engine=${active_engine:-missing}&bus_owner=$(ibus_bus_owner_value)&service_diag=$(ibus_service_diag_query_value)&daemon_process=$(ibus_daemon_process_query_value)&session_env=$(ibus_session_env_query_value)"
    return 1
  fi

  # The live readiness flip propagates through the session bridge's ibus
  # probe; poll briefly instead of failing on the first read.
  for _ in $(seq 1 8); do
    core_code=$(core_proof_request text-shortcuts-status "$core_file" || true)
    core_engine_available=$(json_field "$core_file" engine_available)
    core_runtime_loop=$(json_field "$core_file" engine.runtime_loop_available)
    if [ "$core_code" = "200" ] && [ "$core_engine_available" = "true" ] && [ "$core_runtime_loop" = "true" ]; then
      break
    fi
    sleep 1
  done
  if [ "$core_code" != "200" ] || [ "$core_engine_available" != "true" ] || [ "$core_runtime_loop" != "true" ]; then
    bridge_env_probe=$(timeout 5 systemd-run --user --pipe --quiet ibus engine 2>&1 | tail -n 1)
    proof_text_shortcuts "status=fail&stage=core-honesty&core_http=${core_code:-000}&core_engine_available=${core_engine_available:-missing}&core_runtime_loop_available=${core_runtime_loop:-missing}&core_engine_detail=$(proof_query_value "$(json_field "$core_file" engine.detail)")&session_ibus_engine=$(proof_query_value "$(active_ibus_engine)")&bridge_env_probe=$(proof_query_value "${bridge_env_probe:-empty}")"
    return 1
  fi

  proof_text_shortcuts "status=pass&route=/v1/text-shortcuts&service=active&service_unit=$TEXT_SHORTCUTS_IBUS_SERVICE&input_source_configured=true&preload_configured=true&engine_listed=true&adapter_self_test=pass&engine_set=pass&active_engine=goblins-textshortcuts&core_http=200&core_engine_available=true&core_runtime_loop_available=true&runtime_ready_claim=true"
  return 0
}
text_shortcuts_candidate_metadata_proof(){
  local candidate_file=/tmp/gate-text-shortcuts-candidate.txt
  local candidate_pid

  rm -f "$candidate_file"
  GOBLINS_OS_TEXT_SHORTCUTS_PROOF_FILE="$candidate_file" "$B/goblins-os-shell" --text-shortcuts-proof candidate >/tmp/gate-text-shortcuts-candidate.log 2>&1 &
  candidate_pid=$!
  sleep 4
  wait_proof_file_nonempty "$candidate_file" 40 || true
  kill "$candidate_pid" 2>/dev/null || true
  wait "$candidate_pid" 2>/dev/null || true

  if [ ! -s "$candidate_file" ]; then
    proof_text_shortcuts_candidate "status=fail&stage=candidate-file&surface=goblins-os-shell-text-shortcuts-candidate-proof"
    return 1
  fi
  if ! grep -Fxq "replacement=on my way" "$candidate_file" \
    || ! grep -Fxq "accept_on=word-boundary" "$candidate_file" \
    || ! grep -Fxq "dismiss_key=Escape" "$candidate_file" \
    || ! grep -Fxq "rendered_bubble_ready_claim=false" "$candidate_file"; then
    proof_text_shortcuts_candidate "status=fail&stage=candidate-metadata&surface=goblins-os-shell-text-shortcuts-candidate-proof&rendered_bubble_ready_claim=missing"
    return 1
  fi

  proof_text_shortcuts_candidate "status=pass&route=/v1/text-shortcuts&surface=goblins-os-shell-text-shortcuts-candidate-proof&candidate_replacement=on%20my%20way&candidate_accept_on=word-boundary&candidate_dismiss_key=Escape&rendered_bubble_ready_claim=false&live_overlay_claim=false&runtime_ready_claim=false"
  return 0
}
text_shortcuts_overlay_intent_proof(){
  local overlay_file=/tmp/gate-text-shortcuts-overlay-intent.json
  local status surface show_count hide_count rendered_claim live_claim runtime_claim

  rm -f "$overlay_file"
  if ! /usr/libexec/goblins-os/goblins-textshortcuts-ibus --overlay-intent-self-test > "$overlay_file" 2>/tmp/gate-text-shortcuts-overlay-intent.log; then
    proof_text_shortcuts_overlay_intent "status=fail&stage=adapter-overlay-intent-self-test&surface=goblins-textshortcuts-ibus-adapter-overlay-intent"
    return 1
  fi

  status="$(json_field "$overlay_file" status)"
  surface="$(json_field "$overlay_file" surface)"
  show_count="$(json_field "$overlay_file" show_count)"
  hide_count="$(json_field "$overlay_file" hide_count)"
  rendered_claim="$(json_field "$overlay_file" rendered_bubble_ready_claim)"
  live_claim="$(json_field "$overlay_file" live_overlay_claim)"
  runtime_claim="$(json_field "$overlay_file" runtime_ready_claim)"
  if [ "$status" != "pass" ] \
    || [ "$surface" != "goblins-textshortcuts-ibus-adapter-overlay-intent" ] \
    || [ "$show_count" != "2" ] \
    || [ "$hide_count" != "2" ] \
    || [ "$rendered_claim" != "false" ] \
    || [ "$live_claim" != "false" ] \
    || [ "$runtime_claim" != "false" ] \
    || ! grep -Fq '"reason": "dismissed"' "$overlay_file" \
    || ! grep -Fq '"reason": "committed"' "$overlay_file"; then
    proof_text_shortcuts_overlay_intent "status=fail&stage=overlay-intent-fields&surface=${surface:-missing}&show_count=${show_count:-missing}&hide_count=${hide_count:-missing}&rendered_bubble_ready_claim=${rendered_claim:-missing}&live_overlay_claim=${live_claim:-missing}&runtime_ready_claim=${runtime_claim:-missing}"
    return 1
  fi

  proof_text_shortcuts_overlay_intent "status=pass&route=/v1/text-shortcuts&surface=goblins-textshortcuts-ibus-adapter-overlay-intent&adapter_self_test=pass&show_count=2&hide_count=2&dismissed_reason=true&committed_reason=true&rendered_bubble_ready_claim=false&live_overlay_claim=false&runtime_ready_claim=false"
  return 0
}
text_shortcuts_candidate_bubble_frame_proof(){
  local frame_file=/tmp/gate-text-shortcuts-candidate-bubble-frame.json
  local status surface show_count hide_count dismissed_frame committed_frame
  local replacement accept_on dismiss_key style_class text_style_class hint_style_class
  local font_family sensitive_refusal rendered_claim live_claim runtime_claim

  rm -f "$frame_file"
  if ! /usr/libexec/goblins-os/goblins-textshortcuts-ibus --candidate-bubble-frame-self-test > "$frame_file" 2>/tmp/gate-text-shortcuts-candidate-bubble-frame.log; then
    proof_text_shortcuts_candidate_bubble_frame "status=fail&stage=adapter-candidate-bubble-frame-self-test&surface=goblins-textshortcuts-accept-bubble-frame"
    return 1
  fi

  status="$(json_field "$frame_file" status)"
  surface="$(json_field "$frame_file" surface)"
  show_count="$(json_field "$frame_file" show_frame_count)"
  hide_count="$(json_field "$frame_file" hide_frame_count)"
  dismissed_frame="$(json_field "$frame_file" dismissed_frame)"
  committed_frame="$(json_field "$frame_file" committed_frame)"
  replacement="$(json_field "$frame_file" replacement)"
  accept_on="$(json_field "$frame_file" accept_on)"
  dismiss_key="$(json_field "$frame_file" dismiss_key)"
  style_class="$(json_field "$frame_file" style_class)"
  text_style_class="$(json_field "$frame_file" text_style_class)"
  hint_style_class="$(json_field "$frame_file" hint_style_class)"
  font_family="$(json_field "$frame_file" font_family)"
  sensitive_refusal="$(json_field "$frame_file" sensitive_field_refusal)"
  rendered_claim="$(json_field "$frame_file" rendered_bubble_ready_claim)"
  live_claim="$(json_field "$frame_file" live_overlay_claim)"
  runtime_claim="$(json_field "$frame_file" runtime_ready_claim)"
  if [ "$status" != "pass" ] \
    || [ "$surface" != "goblins-textshortcuts-accept-bubble-frame" ] \
    || [ "$show_count" != "2" ] \
    || [ "$hide_count" != "2" ] \
    || [ "$dismissed_frame" != "true" ] \
    || [ "$committed_frame" != "true" ] \
    || [ "$replacement" != "on my way" ] \
    || [ "$accept_on" != "word-boundary" ] \
    || [ "$dismiss_key" != "Escape" ] \
    || [ "$style_class" != "gos-text-shortcuts-candidate" ] \
    || [ "$text_style_class" != "gos-text-shortcuts-candidate-text" ] \
    || [ "$hint_style_class" != "gos-text-shortcuts-candidate-hint" ] \
    || [ "$font_family" != "Inter" ] \
    || [ "$sensitive_refusal" != "true" ] \
    || [ "$rendered_claim" != "false" ] \
    || [ "$live_claim" != "false" ] \
    || [ "$runtime_claim" != "false" ] \
    || ! grep -Fq '"Space"' "$frame_file" \
    || ! grep -Fq '"Return"' "$frame_file"; then
    proof_text_shortcuts_candidate_bubble_frame "status=fail&stage=candidate-bubble-frame-fields&surface=${surface:-missing}&show_frame_count=${show_count:-missing}&hide_frame_count=${hide_count:-missing}&rendered_bubble_ready_claim=${rendered_claim:-missing}&live_overlay_claim=${live_claim:-missing}&runtime_ready_claim=${runtime_claim:-missing}"
    return 1
  fi

  proof_text_shortcuts_candidate_bubble_frame "status=pass&route=/v1/text-shortcuts&surface=goblins-textshortcuts-accept-bubble-frame&adapter_self_test=pass&show_frame_count=2&hide_frame_count=2&dismissed_frame=true&committed_frame=true&replacement=on%20my%20way&accept_on=word-boundary&accept_keys=Space,Return&dismiss_key=Escape&style_class=gos-text-shortcuts-candidate&text_style_class=gos-text-shortcuts-candidate-text&hint_style_class=gos-text-shortcuts-candidate-hint&font_family=Inter&sensitive_field_refusal=true&rendered_bubble_ready_claim=false&live_overlay_claim=false&runtime_ready_claim=false"
  return 0
}
text_shortcuts_candidate_bubble_layout_proof(){
  local layout_file=/tmp/gate-text-shortcuts-candidate-bubble-layout.json
  local status surface frame_surface layout_count visible_layout_count
  local right_edge_clamped bottom_edge_flipped hidden_frame_collapses
  local style_class font_family rendered_claim live_claim runtime_claim

  rm -f "$layout_file"
  if ! /usr/libexec/goblins-os/goblins-textshortcuts-ibus --candidate-bubble-layout-self-test > "$layout_file" 2>/tmp/gate-text-shortcuts-candidate-bubble-layout.log; then
    proof_text_shortcuts_candidate_bubble_layout "status=fail&stage=adapter-candidate-bubble-layout-self-test&surface=goblins-textshortcuts-accept-bubble-layout"
    return 1
  fi

  status="$(json_field "$layout_file" status)"
  surface="$(json_field "$layout_file" surface)"
  frame_surface="$(json_field "$layout_file" frame_surface)"
  layout_count="$(json_field "$layout_file" layout_count)"
  visible_layout_count="$(json_field "$layout_file" visible_layout_count)"
  right_edge_clamped="$(json_field "$layout_file" right_edge_clamped)"
  bottom_edge_flipped="$(json_field "$layout_file" bottom_edge_flipped)"
  hidden_frame_collapses="$(json_field "$layout_file" hidden_frame_collapses)"
  style_class="$(json_field "$layout_file" style_class)"
  font_family="$(json_field "$layout_file" font_family)"
  rendered_claim="$(json_field "$layout_file" rendered_bubble_ready_claim)"
  live_claim="$(json_field "$layout_file" live_overlay_claim)"
  runtime_claim="$(json_field "$layout_file" runtime_ready_claim)"
  if [ "$status" != "pass" ] \
    || [ "$surface" != "goblins-textshortcuts-accept-bubble-layout" ] \
    || [ "$frame_surface" != "goblins-textshortcuts-accept-bubble-frame" ] \
    || [ "$layout_count" != "4" ] \
    || [ "$visible_layout_count" != "3" ] \
    || [ "$right_edge_clamped" != "true" ] \
    || [ "$bottom_edge_flipped" != "true" ] \
    || [ "$hidden_frame_collapses" != "true" ] \
    || [ "$style_class" != "gos-text-shortcuts-candidate" ] \
    || [ "$font_family" != "Inter" ] \
    || [ "$rendered_claim" != "false" ] \
    || [ "$live_claim" != "false" ] \
    || [ "$runtime_claim" != "false" ]; then
    proof_text_shortcuts_candidate_bubble_layout "status=fail&stage=candidate-bubble-layout-fields&surface=${surface:-missing}&layout_count=${layout_count:-missing}&visible_layout_count=${visible_layout_count:-missing}&right_edge_clamped=${right_edge_clamped:-missing}&bottom_edge_flipped=${bottom_edge_flipped:-missing}&hidden_frame_collapses=${hidden_frame_collapses:-missing}&rendered_bubble_ready_claim=${rendered_claim:-missing}&live_overlay_claim=${live_claim:-missing}&runtime_ready_claim=${runtime_claim:-missing}"
    return 1
  fi

  proof_text_shortcuts_candidate_bubble_layout "status=pass&route=/v1/text-shortcuts&surface=goblins-textshortcuts-accept-bubble-layout&adapter_self_test=pass&frame_surface=goblins-textshortcuts-accept-bubble-frame&layout_count=4&visible_layout_count=3&right_edge_clamped=true&bottom_edge_flipped=true&hidden_frame_collapses=true&style_class=gos-text-shortcuts-candidate&font_family=Inter&rendered_bubble_ready_claim=false&live_overlay_claim=false&runtime_ready_claim=false"
  return 0
}

text_shortcuts_candidate_bubble_render_intent_proof(){
  local intent_file=/tmp/gate-text-shortcuts-candidate-bubble-render-intent.json
  local status surface frame_surface layout_surface render_count show_count hide_count
  local dismissed_intent committed_intent focus_out_hide sensitive_hide
  local pass_through_unchanged sink_failure_fail_open style_class font_family
  local rendered_claim live_claim runtime_claim

  rm -f "$intent_file"
  if ! /usr/libexec/goblins-os/goblins-textshortcuts-ibus --candidate-bubble-render-intent-self-test > "$intent_file" 2>/tmp/gate-text-shortcuts-candidate-bubble-render-intent.log; then
    proof_text_shortcuts_candidate_bubble_render_intent "status=fail&stage=adapter-candidate-bubble-render-intent-self-test&surface=goblins-textshortcuts-accept-bubble-render-intent"
    return 1
  fi

  status="$(json_field "$intent_file" status)"
  surface="$(json_field "$intent_file" surface)"
  frame_surface="$(json_field "$intent_file" frame_surface)"
  layout_surface="$(json_field "$intent_file" layout_surface)"
  render_count="$(json_field "$intent_file" render_intent_count)"
  show_count="$(json_field "$intent_file" show_intent_count)"
  hide_count="$(json_field "$intent_file" hide_intent_count)"
  dismissed_intent="$(json_field "$intent_file" dismissed_intent)"
  committed_intent="$(json_field "$intent_file" committed_intent)"
  focus_out_hide="$(json_field "$intent_file" focus_out_hide)"
  sensitive_hide="$(json_field "$intent_file" sensitive_hide)"
  pass_through_unchanged="$(json_field "$intent_file" pass_through_unchanged)"
  sink_failure_fail_open="$(json_field "$intent_file" sink_failure_fail_open)"
  style_class="$(json_field "$intent_file" style_class)"
  font_family="$(json_field "$intent_file" font_family)"
  rendered_claim="$(json_field "$intent_file" rendered_bubble_ready_claim)"
  live_claim="$(json_field "$intent_file" live_overlay_claim)"
  runtime_claim="$(json_field "$intent_file" runtime_ready_claim)"
  if [ "$status" != "pass" ] \
    || [ "$surface" != "goblins-textshortcuts-accept-bubble-render-intent" ] \
    || [ "$frame_surface" != "goblins-textshortcuts-accept-bubble-frame" ] \
    || [ "$layout_surface" != "goblins-textshortcuts-accept-bubble-layout" ] \
    || [ "$render_count" != "8" ] \
    || [ "$show_count" != "4" ] \
    || [ "$hide_count" != "4" ] \
    || [ "$dismissed_intent" != "true" ] \
    || [ "$committed_intent" != "true" ] \
    || [ "$focus_out_hide" != "true" ] \
    || [ "$sensitive_hide" != "true" ] \
    || [ "$pass_through_unchanged" != "true" ] \
    || [ "$sink_failure_fail_open" != "true" ] \
    || [ "$style_class" != "gos-text-shortcuts-candidate" ] \
    || [ "$font_family" != "Inter" ] \
    || [ "$rendered_claim" != "false" ] \
    || [ "$live_claim" != "false" ] \
    || [ "$runtime_claim" != "false" ]; then
    proof_text_shortcuts_candidate_bubble_render_intent "status=fail&stage=candidate-bubble-render-intent-fields&surface=${surface:-missing}&render_intent_count=${render_count:-missing}&show_intent_count=${show_count:-missing}&hide_intent_count=${hide_count:-missing}&focus_out_hide=${focus_out_hide:-missing}&sensitive_hide=${sensitive_hide:-missing}&pass_through_unchanged=${pass_through_unchanged:-missing}&sink_failure_fail_open=${sink_failure_fail_open:-missing}&rendered_bubble_ready_claim=${rendered_claim:-missing}&live_overlay_claim=${live_claim:-missing}&runtime_ready_claim=${runtime_claim:-missing}"
    return 1
  fi

  proof_text_shortcuts_candidate_bubble_render_intent "status=pass&route=/v1/text-shortcuts&surface=goblins-textshortcuts-accept-bubble-render-intent&adapter_self_test=pass&frame_surface=goblins-textshortcuts-accept-bubble-frame&layout_surface=goblins-textshortcuts-accept-bubble-layout&render_intent_count=8&show_intent_count=4&hide_intent_count=4&dismissed_intent=true&committed_intent=true&focus_out_hide=true&sensitive_hide=true&pass_through_unchanged=true&sink_failure_fail_open=true&style_class=gos-text-shortcuts-candidate&font_family=Inter&rendered_bubble_ready_claim=false&live_overlay_claim=false&runtime_ready_claim=false"
  return 0
}
text_shortcuts_candidate_bubble_render_proof(){
  local render_file=/tmp/gate-text-shortcuts-candidate-bubble-render.txt
  local render_pid

  rm -f "$render_file"
  dismiss_shell_overview text-shortcuts-candidate-render
  GOBLINS_OS_TEXT_SHORTCUTS_PROOF_FILE="$render_file" "$B/goblins-os-shell" --text-shortcuts-proof candidate-render >/tmp/gate-text-shortcuts-candidate-bubble-render.log 2>&1 &
  render_pid=$!
  sleep 4
  host_focus_text_shortcuts_field candidate-render-focus
  wait_proof_file_nonempty "$render_file" 40 || true
  sig 31-text-shortcuts-candidate-bubble-render
  kill "$render_pid" 2>/dev/null || true
  wait "$render_pid" 2>/dev/null || true

  if [ ! -s "$render_file" ]; then
    proof_text_shortcuts_candidate_bubble_render "status=fail&stage=candidate-bubble-render-file&surface=goblins-os-shell-text-shortcuts-candidate-bubble-render&screenshot=31-text-shortcuts-candidate-bubble-render.png"
    return 1
  fi
  if ! grep -Fxq "surface=goblins-os-shell-text-shortcuts-candidate-bubble-render" "$render_file" \
    || ! grep -Fxq "render_intent_surface=goblins-textshortcuts-accept-bubble-render-intent" "$render_file" \
    || ! grep -Fxq "layout_surface=goblins-textshortcuts-accept-bubble-layout" "$render_file" \
    || ! grep -Fxq "frame_surface=goblins-textshortcuts-accept-bubble-frame" "$render_file" \
    || ! grep -Fxq "replacement=on my way" "$render_file" \
    || ! grep -Fxq "accept_on=word-boundary" "$render_file" \
    || ! grep -Fxq "dismiss_key=Escape" "$render_file" \
    || ! grep -Fxq "style_class=gos-text-shortcuts-candidate" "$render_file" \
    || ! grep -Fxq "text_style_class=gos-text-shortcuts-candidate-text" "$render_file" \
    || ! grep -Fxq "hint_style_class=gos-text-shortcuts-candidate-hint" "$render_file" \
    || ! grep -Fxq "font_family=Inter" "$render_file" \
    || ! grep -Fxq "screenshot=31-text-shortcuts-candidate-bubble-render.png" "$render_file" \
    || ! grep -Fxq "rendered_candidate_surface=true" "$render_file" \
    || ! grep -Fxq "rendered_bubble_ready_claim=false" "$render_file" \
    || ! grep -Fxq "live_overlay_claim=false" "$render_file" \
    || ! grep -Fxq "runtime_ready_claim=false" "$render_file"; then
    proof_text_shortcuts_candidate_bubble_render "status=fail&stage=candidate-bubble-render-fields&surface=goblins-os-shell-text-shortcuts-candidate-bubble-render&screenshot=31-text-shortcuts-candidate-bubble-render.png"
    return 1
  fi

  proof_text_shortcuts_candidate_bubble_render "status=pass&route=/v1/text-shortcuts&surface=goblins-os-shell-text-shortcuts-candidate-bubble-render&render_intent_surface=goblins-textshortcuts-accept-bubble-render-intent&layout_surface=goblins-textshortcuts-accept-bubble-layout&frame_surface=goblins-textshortcuts-accept-bubble-frame&replacement=on%20my%20way&accept_on=word-boundary&dismiss_key=Escape&style_class=gos-text-shortcuts-candidate&text_style_class=gos-text-shortcuts-candidate-text&hint_style_class=gos-text-shortcuts-candidate-hint&font_family=Inter&screenshot=31-text-shortcuts-candidate-bubble-render.png&rendered_candidate_surface=true&rendered_bubble_ready_claim=false&live_overlay_claim=false&runtime_ready_claim=false"
  return 0
}
text_shortcuts_live_ibus_runtime_render_proof(){
  local config_dir="${XDG_CONFIG_HOME:-$HOME/.config}/goblins-os"
  local table_file="$config_dir/text-shortcuts.json"
  local ledger_file=/tmp/gate-text-shortcuts-live-ibus-runtime-render-events.jsonl
  local render_file=/tmp/gate-text-shortcuts-live-ibus-runtime-render.txt
  local passthrough_file=/tmp/gate-text-shortcuts-live-ibus-runtime-render-passthrough.txt
  local password_file=/tmp/gate-text-shortcuts-live-ibus-runtime-render-password.txt
  local render_pid passthrough_pid password_pid service_state active_engine
  local normal_actual passthrough_actual password_actual
  local focused_field_callback process_key_event_callback text_input_v3_commit rendered_accept_bubble
  local style_class_seen font_family_seen

  mkdir -p "$config_dir"
  printf '[{"replace":"omw","with":"onmyway"}]\n' > "$table_file"
  rm -f "$ledger_file" "$render_file" "$passthrough_file" "$password_file"
  : > "$ledger_file"

  if ! systemctl --user set-environment GOBLINS_TEXTSHORTCUTS_PROOF_EVENTS="$ledger_file" >/dev/null 2>&1; then
    proof_text_shortcuts_live_ibus_runtime_render "status=fail&stage=proof-env&route=/v1/text-shortcuts&surface=goblins-textshortcuts-live-ibus-runtime-render&input_driver=$TEXT_SHORTCUTS_INPUT_DRIVER&active_engine=missing&normal_actual=missing&passthrough_actual=missing&password_refusal=false&focused_field_callback=false&text_input_v3_commit=false&rendered_accept_bubble=false&screenshot=32-text-shortcuts-live-ibus-runtime-render.png&style_class=gos-text-shortcuts-candidate&font_family=Inter&rendered_bubble_ready_claim=false&live_overlay_claim=false&runtime_ready_claim=false&core_readiness_flip=live"
    return 1
  fi
  systemctl --user import-environment DISPLAY WAYLAND_DISPLAY XDG_SESSION_TYPE XDG_CURRENT_DESKTOP XDG_SESSION_DESKTOP DESKTOP_SESSION DBUS_SESSION_BUS_ADDRESS GOBLINS_TEXTSHORTCUTS_PROOF_EVENTS 2>/dev/null || true
  dbus-update-activation-environment --systemd DISPLAY WAYLAND_DISPLAY XDG_SESSION_TYPE XDG_CURRENT_DESKTOP XDG_SESSION_DESKTOP DESKTOP_SESSION DBUS_SESSION_BUS_ADDRESS GOBLINS_TEXTSHORTCUTS_PROOF_EVENTS 2>/dev/null || true
  systemctl --user reset-failed "$TEXT_SHORTCUTS_IBUS_SERVICE" >/tmp/gate-text-shortcuts-live-ibus-reset-failed.log 2>&1 || true
  systemctl --user restart "$TEXT_SHORTCUTS_IBUS_SERVICE" >/tmp/gate-text-shortcuts-live-ibus-service.log 2>&1 || true
  for _ in $(seq 1 80); do
    service_state="$(systemctl --user is-active "$TEXT_SHORTCUTS_IBUS_SERVICE" 2>/dev/null || true)"
    if [ "$service_state" = "active" ] && wait_ibus_bus_owned 1; then
      break
    fi
    sleep 0.5
  done
  if [ "$service_state" != "active" ]; then
    proof_text_shortcuts_live_ibus_runtime_render "status=fail&stage=user-service&route=/v1/text-shortcuts&surface=goblins-textshortcuts-live-ibus-runtime-render&input_driver=$TEXT_SHORTCUTS_INPUT_DRIVER&active_engine=missing&service=${service_state:-missing}&bus_owner=$(ibus_bus_owner_value)&normal_actual=missing&passthrough_actual=missing&password_refusal=false&focused_field_callback=false&text_input_v3_commit=false&rendered_accept_bubble=false&screenshot=32-text-shortcuts-live-ibus-runtime-render.png&style_class=gos-text-shortcuts-candidate&font_family=Inter&rendered_bubble_ready_claim=false&live_overlay_claim=false&runtime_ready_claim=false&core_readiness_flip=live&service_diag=$(ibus_service_diag_query_value)&daemon_process=$(ibus_daemon_process_query_value)&session_env=$(ibus_session_env_query_value)"
    return 1
  fi

  ibus write-cache >/tmp/gate-text-shortcuts-live-ibus-write-cache.log 2>&1 || true
  wait_ibus_cli_ready /tmp/gate-text-shortcuts-live-ibus-list-engine.out /tmp/gate-text-shortcuts-live-ibus-list-engine.err 80 || true
  if ! activate_goblins_textshortcuts_engine; then
    active_engine="$(active_ibus_engine)"
    proof_text_shortcuts_live_ibus_runtime_render "status=fail&stage=engine-set&route=/v1/text-shortcuts&surface=goblins-textshortcuts-live-ibus-runtime-render&input_driver=$TEXT_SHORTCUTS_INPUT_DRIVER&active_engine=${active_engine:-missing}&bus_owner=$(ibus_bus_owner_value)&list_error=$(proof_query_value "$(cat /tmp/gate-text-shortcuts-activate-list-engine.err 2>/dev/null || true)")&normal_actual=missing&passthrough_actual=missing&password_refusal=false&focused_field_callback=false&text_input_v3_commit=false&rendered_accept_bubble=false&screenshot=32-text-shortcuts-live-ibus-runtime-render.png&style_class=gos-text-shortcuts-candidate&font_family=Inter&rendered_bubble_ready_claim=false&live_overlay_claim=false&runtime_ready_claim=false&core_readiness_flip=live&service_diag=$(ibus_service_diag_query_value)&daemon_process=$(ibus_daemon_process_query_value)&session_env=$(ibus_session_env_query_value)"
    return 1
  fi
  active_engine="$(active_ibus_engine)"
  if [ "$active_engine" != "goblins-textshortcuts" ]; then
    proof_text_shortcuts_live_ibus_runtime_render "status=fail&stage=engine-active&route=/v1/text-shortcuts&surface=goblins-textshortcuts-live-ibus-runtime-render&input_driver=$TEXT_SHORTCUTS_INPUT_DRIVER&active_engine=${active_engine:-missing}&normal_actual=missing&passthrough_actual=missing&password_refusal=false&focused_field_callback=false&text_input_v3_commit=false&rendered_accept_bubble=false&screenshot=32-text-shortcuts-live-ibus-runtime-render.png&style_class=gos-text-shortcuts-candidate&font_family=Inter&rendered_bubble_ready_claim=false&live_overlay_claim=false&runtime_ready_claim=false&core_readiness_flip=live"
    return 1
  fi

  dismiss_shell_overview text-shortcuts-live-runtime-render
  GOBLINS_OS_TEXT_SHORTCUTS_PROOF_FILE="$render_file" \
    GOBLINS_TEXTSHORTCUTS_PROOF_EVENTS="$ledger_file" \
    "$B/goblins-os-shell" --text-shortcuts-proof live-runtime-render >/tmp/gate-text-shortcuts-live-ibus-runtime-render.log 2>&1 &
  render_pid=$!
  sleep 4
  host_focus_text_shortcuts_field runtime-render-focus
  if ! host_type_text runtime-render-omw "omw"; then
    kill "$render_pid" 2>/dev/null || true
    proof_text_shortcuts_live_ibus_runtime_render "status=fail&stage=render-qmp-keyboard&route=/v1/text-shortcuts&surface=goblins-textshortcuts-live-ibus-runtime-render&input_driver=$TEXT_SHORTCUTS_INPUT_DRIVER&active_engine=goblins-textshortcuts&normal_actual=missing&passthrough_actual=missing&password_refusal=false&focused_field_callback=false&text_input_v3_commit=false&rendered_accept_bubble=false&screenshot=32-text-shortcuts-live-ibus-runtime-render.png&style_class=gos-text-shortcuts-candidate&font_family=Inter&rendered_bubble_ready_claim=false&live_overlay_claim=false&runtime_ready_claim=false&core_readiness_flip=live"
    return 1
  fi
  for _ in $(seq 1 40); do
    if grep -Fxq "focused_field_callback=true" "$render_file" 2>/dev/null \
      && grep -Fxq "rendered_accept_bubble=true" "$render_file" 2>/dev/null; then
      break
    fi
    sleep 0.25
  done
  if ! grep -Fxq "focused_field_callback=true" "$render_file" 2>/dev/null \
    || ! grep -Fxq "rendered_accept_bubble=true" "$render_file" 2>/dev/null; then
    # One bounded re-focus + retype: under the software renderer the first
    # focus click can land while the proof window is still presenting, sending
    # the keystrokes to the shell instead of the entry. The retry still
    # requires the real live render ledger — it can only turn a lost-focus
    # race into an honest pass, never fabricate one.
    dismiss_shell_overview text-shortcuts-live-runtime-render-retry
    host_focus_text_shortcuts_field runtime-render-focus-retry
    host_type_text runtime-render-omw-retry "omw" || true
    for _ in $(seq 1 40); do
      if grep -Fxq "focused_field_callback=true" "$render_file" 2>/dev/null \
        && grep -Fxq "rendered_accept_bubble=true" "$render_file" 2>/dev/null; then
        break
      fi
      sleep 0.25
    done
  fi
  if ! grep -Fxq "focused_field_callback=true" "$render_file" 2>/dev/null \
    || ! grep -Fxq "rendered_accept_bubble=true" "$render_file" 2>/dev/null; then
    kill "$render_pid" 2>/dev/null || true
    proof_text_shortcuts_live_ibus_runtime_render "status=fail&stage=render-ledger&route=/v1/text-shortcuts&surface=goblins-textshortcuts-live-ibus-runtime-render&input_driver=$TEXT_SHORTCUTS_INPUT_DRIVER&active_engine=goblins-textshortcuts&normal_actual=missing&passthrough_actual=missing&password_refusal=false&focused_field_callback=false&text_input_v3_commit=false&rendered_accept_bubble=false&screenshot=32-text-shortcuts-live-ibus-runtime-render.png&style_class=gos-text-shortcuts-candidate&font_family=Inter&rendered_bubble_ready_claim=false&live_overlay_claim=false&runtime_ready_claim=false&core_readiness_flip=live&render_file_bytes=$(file_size_value "$render_file")&render_log_tail=$(file_tail_query_value /tmp/gate-text-shortcuts-live-ibus-runtime-render.log)&ledger_bytes=$(file_size_value "$ledger_file")&ledger_tail=$(file_tail_query_value "$ledger_file")"
    return 1
  fi
  sig 32-text-shortcuts-live-ibus-runtime-render
  if ! host_type_text runtime-render-boundary "."; then
    kill "$render_pid" 2>/dev/null || true
    proof_text_shortcuts_live_ibus_runtime_render "status=fail&stage=normal-boundary-qmp-keyboard&route=/v1/text-shortcuts&surface=goblins-textshortcuts-live-ibus-runtime-render&input_driver=$TEXT_SHORTCUTS_INPUT_DRIVER&active_engine=goblins-textshortcuts&normal_actual=missing&passthrough_actual=missing&password_refusal=false&focused_field_callback=true&text_input_v3_commit=false&rendered_accept_bubble=true&screenshot=32-text-shortcuts-live-ibus-runtime-render.png&style_class=gos-text-shortcuts-candidate&font_family=Inter&rendered_bubble_ready_claim=true&live_overlay_claim=true&runtime_ready_claim=false&core_readiness_flip=live"
    return 1
  fi
  for _ in $(seq 1 40); do
    normal_actual="$(grep -E '^entry_text=' "$render_file" 2>/dev/null | tail -n 1 | cut -d= -f2- || true)"
    [ "$normal_actual" = "onmyway." ] && break
    sleep 0.25
  done
  kill "$render_pid" 2>/dev/null || true
  wait "$render_pid" 2>/dev/null || true
  if [ "$normal_actual" != "onmyway." ]; then
    proof_text_shortcuts_live_ibus_runtime_render "status=fail&stage=normal-readback&route=/v1/text-shortcuts&surface=goblins-textshortcuts-live-ibus-runtime-render&input_driver=$TEXT_SHORTCUTS_INPUT_DRIVER&active_engine=goblins-textshortcuts&normal_actual=${normal_actual:-missing}&passthrough_actual=missing&password_refusal=false&focused_field_callback=true&text_input_v3_commit=false&rendered_accept_bubble=true&screenshot=32-text-shortcuts-live-ibus-runtime-render.png&style_class=gos-text-shortcuts-candidate&font_family=Inter&rendered_bubble_ready_claim=true&live_overlay_claim=true&runtime_ready_claim=false&core_readiness_flip=live&render_file_bytes=$(file_size_value "$render_file")&render_log_tail=$(file_tail_query_value /tmp/gate-text-shortcuts-live-ibus-runtime-render.log)&ledger_bytes=$(file_size_value "$ledger_file")&ledger_tail=$(file_tail_query_value "$ledger_file")"
    return 1
  fi

  GOBLINS_OS_TEXT_SHORTCUTS_PROOF_FILE="$passthrough_file" "$B/goblins-os-shell" --text-shortcuts-proof passthrough >/tmp/gate-text-shortcuts-live-ibus-passthrough.log 2>&1 &
  passthrough_pid=$!
  sleep 4
  host_focus_text_shortcuts_field runtime-passthrough-focus
  if ! host_type_text runtime-passthrough-hello "hello."; then
    kill "$passthrough_pid" 2>/dev/null || true
    proof_text_shortcuts_live_ibus_runtime_render "status=fail&stage=passthrough-qmp-keyboard&route=/v1/text-shortcuts&surface=goblins-textshortcuts-live-ibus-runtime-render&input_driver=$TEXT_SHORTCUTS_INPUT_DRIVER&active_engine=goblins-textshortcuts&normal_actual=onmyway.&passthrough_actual=missing&password_refusal=false&focused_field_callback=true&text_input_v3_commit=false&rendered_accept_bubble=true&screenshot=32-text-shortcuts-live-ibus-runtime-render.png&style_class=gos-text-shortcuts-candidate&font_family=Inter&rendered_bubble_ready_claim=true&live_overlay_claim=true&runtime_ready_claim=false&core_readiness_flip=live"
    return 1
  fi
  for _ in $(seq 1 20); do
    passthrough_actual="$(cat "$passthrough_file" 2>/dev/null || true)"
    [ "$passthrough_actual" = "hello." ] && break
    sleep 0.5
  done
  kill "$passthrough_pid" 2>/dev/null || true
  wait "$passthrough_pid" 2>/dev/null || true
  if [ "$passthrough_actual" != "hello." ]; then
    proof_text_shortcuts_live_ibus_runtime_render "status=fail&stage=passthrough-readback&route=/v1/text-shortcuts&surface=goblins-textshortcuts-live-ibus-runtime-render&input_driver=$TEXT_SHORTCUTS_INPUT_DRIVER&active_engine=goblins-textshortcuts&normal_actual=onmyway.&passthrough_actual=${passthrough_actual:-missing}&password_refusal=false&focused_field_callback=true&text_input_v3_commit=false&rendered_accept_bubble=true&screenshot=32-text-shortcuts-live-ibus-runtime-render.png&style_class=gos-text-shortcuts-candidate&font_family=Inter&rendered_bubble_ready_claim=true&live_overlay_claim=true&runtime_ready_claim=false&core_readiness_flip=live"
    return 1
  fi

  GOBLINS_OS_TEXT_SHORTCUTS_PROOF_FILE="$password_file" "$B/goblins-os-shell" --text-shortcuts-proof password >/tmp/gate-text-shortcuts-live-ibus-password.log 2>&1 &
  password_pid=$!
  sleep 4
  host_focus_text_shortcuts_field runtime-password-focus
  if ! host_type_text runtime-password-omw "omw."; then
    kill "$password_pid" 2>/dev/null || true
    proof_text_shortcuts_live_ibus_runtime_render "status=fail&stage=password-qmp-keyboard&route=/v1/text-shortcuts&surface=goblins-textshortcuts-live-ibus-runtime-render&input_driver=$TEXT_SHORTCUTS_INPUT_DRIVER&active_engine=goblins-textshortcuts&normal_actual=onmyway.&passthrough_actual=hello.&password_refusal=false&focused_field_callback=true&text_input_v3_commit=false&rendered_accept_bubble=true&screenshot=32-text-shortcuts-live-ibus-runtime-render.png&style_class=gos-text-shortcuts-candidate&font_family=Inter&rendered_bubble_ready_claim=true&live_overlay_claim=true&runtime_ready_claim=false&core_readiness_flip=live"
    return 1
  fi
  for _ in $(seq 1 20); do
    password_actual="$(cat "$password_file" 2>/dev/null || true)"
    [ "$password_actual" = "omw." ] && break
    sleep 0.5
  done
  kill "$password_pid" 2>/dev/null || true
  wait "$password_pid" 2>/dev/null || true
  if [ "$password_actual" != "omw." ]; then
    proof_text_shortcuts_live_ibus_runtime_render "status=fail&stage=password-readback&route=/v1/text-shortcuts&surface=goblins-textshortcuts-live-ibus-runtime-render&input_driver=$TEXT_SHORTCUTS_INPUT_DRIVER&active_engine=goblins-textshortcuts&normal_actual=onmyway.&passthrough_actual=hello.&password_refusal=false&focused_field_callback=true&text_input_v3_commit=false&rendered_accept_bubble=true&screenshot=32-text-shortcuts-live-ibus-runtime-render.png&style_class=gos-text-shortcuts-candidate&font_family=Inter&rendered_bubble_ready_claim=true&live_overlay_claim=true&runtime_ready_claim=false&core_readiness_flip=live"
    return 1
  fi

  focused_field_callback=false
  process_key_event_callback=false
  text_input_v3_commit=false
  rendered_accept_bubble=false
  style_class_seen=false
  font_family_seen=false
  grep -Fq '"callback":"focus-in"' "$ledger_file" && focused_field_callback=true
  grep -Fq '"callback":"process-key-event"' "$ledger_file" && process_key_event_callback=true
  grep -Fq '"commit-text"' "$ledger_file" && text_input_v3_commit=true
  grep -Fq '"action":"show-candidate"' "$ledger_file" && rendered_accept_bubble=true
  grep -Fq '"style_class":"gos-text-shortcuts-candidate"' "$ledger_file" && style_class_seen=true
  grep -Fq '"font_family":"Inter"' "$ledger_file" && font_family_seen=true
  if [ "$focused_field_callback" != "true" ] \
    || [ "$process_key_event_callback" != "true" ] \
    || [ "$text_input_v3_commit" != "true" ] \
    || [ "$rendered_accept_bubble" != "true" ] \
    || [ "$style_class_seen" != "true" ] \
    || [ "$font_family_seen" != "true" ]; then
    proof_text_shortcuts_live_ibus_runtime_render "status=fail&stage=ledger-final&route=/v1/text-shortcuts&surface=goblins-textshortcuts-live-ibus-runtime-render&input_driver=$TEXT_SHORTCUTS_INPUT_DRIVER&active_engine=goblins-textshortcuts&normal_actual=onmyway.&passthrough_actual=hello.&password_refusal=true&focused_field_callback=$focused_field_callback&text_input_v3_commit=$text_input_v3_commit&rendered_accept_bubble=$rendered_accept_bubble&screenshot=32-text-shortcuts-live-ibus-runtime-render.png&style_class=gos-text-shortcuts-candidate&font_family=Inter&rendered_bubble_ready_claim=$rendered_accept_bubble&live_overlay_claim=$rendered_accept_bubble&runtime_ready_claim=false&core_readiness_flip=live"
    return 1
  fi

  proof_text_shortcuts_live_ibus_runtime_render "status=pass&route=/v1/text-shortcuts&surface=goblins-textshortcuts-live-ibus-runtime-render&input_driver=$TEXT_SHORTCUTS_INPUT_DRIVER&active_engine=goblins-textshortcuts&normal_actual=onmyway.&passthrough_actual=hello.&password_refusal=true&focused_field_callback=true&text_input_v3_commit=true&rendered_accept_bubble=true&screenshot=32-text-shortcuts-live-ibus-runtime-render.png&style_class=gos-text-shortcuts-candidate&font_family=Inter&rendered_bubble_ready_claim=true&live_overlay_claim=true&runtime_ready_claim=true&core_readiness_flip=live"
  return 0
}
keyboard_shortcuts_roundtrip_proof(){
  local shortcut_set_file=/tmp/gate-keyboard-shortcut-set.json
  local shortcut_reset_file=/tmp/gate-keyboard-shortcut-reset.json
  local modifier_set_file=/tmp/gate-keyboard-modifier-set.json
  local modifier_reset_file=/tmp/gate-keyboard-modifier-reset.json
  local shortcut_code shortcut_ok shortcut_after_set reset_code reset_ok shortcut_after_reset
  local modifier_code modifier_ok xkb_after_set modifier_reset_code modifier_reset_ok xkb_after_reset

  for _ in $(seq 1 60); do
    curl -sf "$CORE_HEALTH_URL" >/dev/null 2>&1 && break
    sleep 0.5
  done

  shortcut_code=$(core_proof_request keyboard-shortcut-set "$shortcut_set_file" || true)
  shortcut_ok=$(json_field "$shortcut_set_file" ok)
  for _ in $(seq 1 20); do
    shortcut_after_set="$(gsettings get org.goblins.shell.extensions.wm window-hud 2>/dev/null || true)"
    if printf '%s\n' "$shortcut_after_set" | grep -Eq "'(<Super><Shift>H|<Shift><Super>H)'"; then
      break
    fi
    sleep 0.25
  done
  if [ "$shortcut_code" != "200" ] || [ "$shortcut_ok" != "true" ] || ! printf '%s\n' "$shortcut_after_set" | grep -Eq "'(<Super><Shift>H|<Shift><Super>H)'"; then
    proof_keyboard_shortcuts_roundtrip "status=fail&stage=shortcut-set&route=/v1/keyboard/shortcuts/binding&shortcut_http=${shortcut_code:-000}&shortcut_ok=${shortcut_ok:-missing}&shortcut_action=window-hud&shortcut_binding=%3CSuper%3E%3CShift%3EH"
    return 1
  fi

  reset_code=$(core_proof_request keyboard-shortcut-reset "$shortcut_reset_file" || true)
  reset_ok=$(json_field "$shortcut_reset_file" ok)
  for _ in $(seq 1 20); do
    shortcut_after_reset="$(gsettings get org.goblins.shell.extensions.wm window-hud 2>/dev/null || true)"
    printf '%s\n' "$shortcut_after_reset" | grep -Fq "'<Super>w'" && break
    sleep 0.25
  done
  if [ "$reset_code" != "200" ] || [ "$reset_ok" != "true" ] || ! printf '%s\n' "$shortcut_after_reset" | grep -Fq "'<Super>w'" || printf '%s\n' "$shortcut_after_reset" | grep -Fq "'<Super><Shift>H'"; then
    proof_keyboard_shortcuts_roundtrip "status=fail&stage=shortcut-reset&route=/v1/keyboard/shortcuts/binding&reset_http=${reset_code:-000}&reset_ok=${reset_ok:-missing}&shortcut_action=window-hud&default_binding=%3CSuper%3Ew"
    return 1
  fi

  modifier_code=$(core_proof_request keyboard-modifier-set "$modifier_set_file" || true)
  modifier_ok=$(json_field "$modifier_set_file" ok)
  for _ in $(seq 1 20); do
    xkb_after_set="$(gsettings get org.gnome.desktop.input-sources xkb-options 2>/dev/null || true)"
    printf '%s\n' "$xkb_after_set" | grep -Fq "'ctrl:nocaps'" && break
    sleep 0.25
  done
  if [ "$modifier_code" != "200" ] || [ "$modifier_ok" != "true" ] || ! printf '%s\n' "$xkb_after_set" | grep -Fq "'ctrl:nocaps'"; then
    proof_keyboard_shortcuts_roundtrip "status=fail&stage=modifier-set&route=/v1/keyboard/modifier-remap&modifier_http=${modifier_code:-000}&modifier_ok=${modifier_ok:-missing}&modifier_target=caps-lock&modifier_value=control"
    return 1
  fi

  modifier_reset_code=$(core_proof_request keyboard-modifier-reset "$modifier_reset_file" || true)
  modifier_reset_ok=$(json_field "$modifier_reset_file" ok)
  for _ in $(seq 1 20); do
    xkb_after_reset="$(gsettings get org.gnome.desktop.input-sources xkb-options 2>/dev/null || true)"
    ! printf '%s\n' "$xkb_after_reset" | grep -Fq "'ctrl:nocaps'" && break
    sleep 0.25
  done
  if [ "$modifier_reset_code" != "200" ] || [ "$modifier_reset_ok" != "true" ] || printf '%s\n' "$xkb_after_reset" | grep -Fq "'ctrl:nocaps'"; then
    proof_keyboard_shortcuts_roundtrip "status=fail&stage=modifier-reset&route=/v1/keyboard/modifier-remap&modifier_reset_http=${modifier_reset_code:-000}&modifier_reset_ok=${modifier_reset_ok:-missing}&modifier_target=caps-lock&modifier_restore=default"
    return 1
  fi

  proof_keyboard_shortcuts_roundtrip "status=pass&shortcut_route=/v1/keyboard/shortcuts/binding&modifier_route=/v1/keyboard/modifier-remap&shortcut_action=window-hud&shortcut_binding=%3CSuper%3E%3CShift%3EH&shortcut_http=200&shortcut_gsettings_readback=true&shortcut_reset_http=200&shortcut_reset_binding=%3CSuper%3Ew&modifier_target=caps-lock&modifier_value=control&modifier_http=200&modifier_gsettings_readback=ctrl:nocaps&modifier_reset_http=200&modifier_restore=default&roundtrip_restored=true"
  return 0
}
restore_input_sources_state(){
  local original_sources="$1"
  local original_current="$2"

  [ -n "$original_sources" ] && gsettings set org.gnome.desktop.input-sources sources "$original_sources" >/dev/null 2>&1 || true
  [ -n "$original_current" ] && gsettings set org.gnome.desktop.input-sources current "$original_current" >/dev/null 2>&1 || true
}
input_sources_roundtrip_proof(){
  local set_file=/tmp/gate-input-sources-set.json
  local switch_file=/tmp/gate-input-sources-switch.json
  local original_sources original_current original_current_value sources_after_set current_after_seed
  local set_code set_ok switch_code switch_ok switch_switched current_after_switch current_after_switch_value
  local sources_after_restore current_after_restore current_after_restore_value restore_sources_ok restore_current_ok

  for _ in $(seq 1 60); do
    curl -sf "$CORE_HEALTH_URL" >/dev/null 2>&1 && break
    sleep 0.5
  done

  original_sources="$(gsettings get org.gnome.desktop.input-sources sources 2>/dev/null || true)"
  original_current="$(gsettings get org.gnome.desktop.input-sources current 2>/dev/null || true)"
  original_current_value="$(printf '%s\n' "$original_current" | awk '{print $NF}' | tr -d "'")"
  if [ -z "$original_sources" ] || ! printf '%s\n' "$original_sources" | grep -Fq "(" || ! printf '%s\n' "$original_current_value" | grep -Eq '^[0-9]+$'; then
    proof_input_sources_roundtrip "status=fail&stage=baseline&source_route=/v1/input/sources&switch_route=/v1/input/switch-next&original_sources_reported=false"
    return 1
  fi

  set_code=$(core_proof_request input-sources-set "$set_file" || true)
  set_ok=$(json_field "$set_file" ok)
  for _ in $(seq 1 20); do
    sources_after_set="$(gsettings get org.gnome.desktop.input-sources sources 2>/dev/null || true)"
    if printf '%s\n' "$sources_after_set" | grep -Fq "('xkb', 'us')" \
      && printf '%s\n' "$sources_after_set" | grep -Fq "('xkb', 'gb')"; then
      break
    fi
    sleep 0.25
  done
  if [ "$set_code" != "200" ] || [ "$set_ok" != "true" ] \
    || ! printf '%s\n' "$sources_after_set" | grep -Fq "('xkb', 'us')" \
    || ! printf '%s\n' "$sources_after_set" | grep -Fq "('xkb', 'gb')"; then
    restore_input_sources_state "$original_sources" "$original_current_value"
    proof_input_sources_roundtrip "status=fail&stage=set&source_route=/v1/input/sources&set_http=${set_code:-000}&set_ok=${set_ok:-missing}&test_sources=xkb-us,xkb-gb"
    return 1
  fi

  if ! gsettings set org.gnome.desktop.input-sources current 0 >/dev/null 2>&1; then
    restore_input_sources_state "$original_sources" "$original_current_value"
    proof_input_sources_roundtrip "status=fail&stage=current-seed&source_route=/v1/input/sources&switch_route=/v1/input/switch-next&current_before_switch=missing"
    return 1
  fi
  current_after_seed="$(gsettings get org.gnome.desktop.input-sources current 2>/dev/null | awk '{print $NF}' | tr -d "'" || true)"
  if [ "$current_after_seed" != "0" ]; then
    restore_input_sources_state "$original_sources" "$original_current_value"
    proof_input_sources_roundtrip "status=fail&stage=current-seed-readback&source_route=/v1/input/sources&switch_route=/v1/input/switch-next&current_before_switch=${current_after_seed:-missing}"
    return 1
  fi

  switch_code=$(core_proof_request input-switch-next "$switch_file" || true)
  switch_ok=$(json_field "$switch_file" ok)
  switch_switched=$(json_field "$switch_file" switched)
  current_after_switch="$(gsettings get org.gnome.desktop.input-sources current 2>/dev/null || true)"
  current_after_switch_value="$(printf '%s\n' "$current_after_switch" | awk '{print $NF}' | tr -d "'")"
  if [ "$switch_code" != "200" ] || [ "$switch_ok" != "true" ] || [ "$switch_switched" != "true" ] || [ "$current_after_switch_value" != "1" ]; then
    restore_input_sources_state "$original_sources" "$original_current_value"
    proof_input_sources_roundtrip "status=fail&stage=switch&source_route=/v1/input/sources&switch_route=/v1/input/switch-next&switch_http=${switch_code:-000}&switch_ok=${switch_ok:-missing}&switch_switched=${switch_switched:-missing}&current_after_switch=${current_after_switch_value:-missing}"
    return 1
  fi

  restore_input_sources_state "$original_sources" "$original_current_value"
  sources_after_restore="$(gsettings get org.gnome.desktop.input-sources sources 2>/dev/null || true)"
  current_after_restore="$(gsettings get org.gnome.desktop.input-sources current 2>/dev/null || true)"
  current_after_restore_value="$(printf '%s\n' "$current_after_restore" | awk '{print $NF}' | tr -d "'")"
  restore_sources_ok=false
  restore_current_ok=false
  [ "$sources_after_restore" = "$original_sources" ] && restore_sources_ok=true
  [ "$current_after_restore_value" = "$original_current_value" ] && restore_current_ok=true
  if [ "$restore_sources_ok" != "true" ] || [ "$restore_current_ok" != "true" ]; then
    proof_input_sources_roundtrip "status=fail&stage=restore&source_route=/v1/input/sources&switch_route=/v1/input/switch-next&restore_sources=$restore_sources_ok&restore_current=$restore_current_ok&roundtrip_restored=false"
    return 1
  fi

  proof_input_sources_roundtrip "status=pass&source_route=/v1/input/sources&switch_route=/v1/input/switch-next&test_sources=xkb-us,xkb-gb&set_http=200&set_ok=true&sources_gsettings_readback=true&current_before_switch=0&switch_http=200&switch_ok=true&switch_switched=true&current_after_switch=1&restore_sources=true&restore_current=true&roundtrip_restored=true"
  return 0
}
multi_display_apply_payloads(){
  python3 - "$1" "$2" "$3" "$4" "$5" <<'PY'
import json
import re
import sys

state_path, verify_path, temporary_path, persistent_guard_path, stale_path = sys.argv[1:6]
state = open(state_path, encoding="utf-8").read()
serial_match = re.search(r"^\s*\(\s*(?:uint32\s+)?([0-9]+)\s*,", state, re.S)
if not serial_match:
    raise SystemExit("missing DisplayConfig serial")
serial = int(serial_match.group(1))
monitor_match = re.search(
    r"\(\('([^']+)',\s*'[^']*',\s*'[^']*',\s*'[^']*'\),\s*\[(.*?)\]\s*,\s*\{",
    state,
    re.S,
)
if not monitor_match:
    raise SystemExit("missing DisplayConfig monitor tuple")
connector = monitor_match.group(1)
modes = monitor_match.group(2)
current_mode = re.search(r"\('([^']+)'.*?\{[^{}]*'is-current': <true>", modes, re.S)
if not current_mode:
    current_mode = re.search(r"\('([^']+)'", modes, re.S)
if not current_mode:
    raise SystemExit("missing DisplayConfig current mode id")
mode_id = current_mode.group(1)
if not re.match(r"^[A-Za-z0-9._-]{1,80}$", connector):
    raise SystemExit("unsafe connector id")
if not re.match(r"^[A-Za-z0-9._@-]{1,120}$", mode_id):
    raise SystemExit("unsafe mode id")

base = {
    "serial": serial,
    "method": "verify",
    "logical_monitors": [
        {
            "x": 0,
            "y": 0,
            "scale": 1.0,
            "transform": 0,
            "primary": True,
            "monitors": [{"connector": connector, "mode_id": mode_id}],
        }
    ],
}
payloads = {
    verify_path: base,
    temporary_path: {**base, "method": "temporary"},
    persistent_guard_path: {**base, "method": "persistent"},
    stale_path: {**base, "serial": serial + 999999},
}
for path, payload in payloads.items():
    with open(path, "w", encoding="utf-8") as fh:
        json.dump(payload, fh, separators=(",", ":"))
        fh.write("\n")
with open("/tmp/gate-multi-display-apply-meta.json", "w", encoding="utf-8") as fh:
    json.dump(
        {
            "serial": serial,
            "stale_serial": serial + 999999,
            "connector": connector,
            "mode_id": mode_id,
        },
        fh,
        sort_keys=True,
    )
    fh.write("\n")
PY
}
multi_display_apply_proof(){
  local status_file=/tmp/gate-multi-display-status.json
  local state_file=/tmp/gate-multi-display-state.txt
  local state_err=/tmp/gate-multi-display-state.err
  local verify_payload=/tmp/gate-multi-display-verify-payload.json
  local temporary_payload=/tmp/gate-multi-display-temporary-payload.json
  local persistent_guard_payload=/tmp/gate-multi-display-persistent-guard-payload.json
  local stale_payload=/tmp/gate-multi-display-stale-payload.json
  local verify_file=/tmp/gate-multi-display-verify.json
  local temporary_file=/tmp/gate-multi-display-temporary.json
  local persistent_guard_file=/tmp/gate-multi-display-persistent-guard.json
  local stale_file=/tmp/gate-multi-display-stale.json
  local status_code available allowed serial connector mode_id state_serial stale_serial
  local verify_code verify_ok temporary_code temporary_ok guard_code guard_ok stale_code stale_ok
  local state_error

  for _ in $(seq 1 60); do
    curl -sf "$CORE_HEALTH_URL" >/dev/null 2>&1 && break
    sleep 0.5
  done

  status_code=$(core_proof_request displays-status "$status_file" || true)
  available=$(json_field "$status_file" mutter_display_config_available)
  allowed=$(json_field "$status_file" mutter_display_apply_allowed)
  serial=$(json_field "$status_file" display_config_serial)
  if [ "$status_code" != "200" ] || [ "$available" != "true" ] || [ "$allowed" != "true" ] || [ -z "$serial" ]; then
    proof_multi_display_apply "status=fail&stage=status&status_route=/v1/displays/status&apply_route=/v1/displays/apply&status_http=${status_code:-000}&display_config_available=${available:-missing}&apply_allowed=${allowed:-missing}&serial=${serial:-missing}"
    return 1
  fi

  if ! gdbus call --session \
    --dest org.gnome.Mutter.DisplayConfig \
    --object-path /org/gnome/Mutter/DisplayConfig \
    --method org.gnome.Mutter.DisplayConfig.GetCurrentState >"$state_file" 2>"$state_err"; then
    state_error="$(proof_query_value "$(cat "$state_err" 2>/dev/null || true)")"
    proof_multi_display_apply "status=fail&stage=current-state&status_route=/v1/displays/status&apply_route=/v1/displays/apply&display_config=org.gnome.Mutter.DisplayConfig&state_error=$state_error"
    return 1
  fi
  if ! multi_display_apply_payloads "$state_file" "$verify_payload" "$temporary_payload" "$persistent_guard_payload" "$stale_payload" >"$state_err" 2>&1; then
    state_error="$(proof_query_value "$(cat "$state_err" 2>/dev/null || true)")"
    proof_multi_display_apply "status=fail&stage=payload&status_route=/v1/displays/status&apply_route=/v1/displays/apply&display_config=org.gnome.Mutter.DisplayConfig&state_error=$state_error"
    return 1
  fi

  state_serial=$(json_field /tmp/gate-multi-display-apply-meta.json serial)
  stale_serial=$(json_field /tmp/gate-multi-display-apply-meta.json stale_serial)
  connector=$(json_field /tmp/gate-multi-display-apply-meta.json connector)
  mode_id=$(json_field /tmp/gate-multi-display-apply-meta.json mode_id)
  if [ "$state_serial" != "$serial" ]; then
    proof_multi_display_apply "status=fail&stage=serial-mismatch&status_route=/v1/displays/status&apply_route=/v1/displays/apply&status_serial=${serial:-missing}&state_serial=${state_serial:-missing}"
    return 1
  fi

  verify_code=$(core_proof_request display-apply-verify "$verify_file" || true)
  verify_ok=$(json_field "$verify_file" ok)
  if [ "$verify_code" != "200" ] || [ "$verify_ok" != "true" ]; then
    proof_multi_display_apply "status=fail&stage=verify&status_route=/v1/displays/status&apply_route=/v1/displays/apply&display_config=org.gnome.Mutter.DisplayConfig&connector=$(proof_query_value "$connector")&mode_id=$(proof_query_value "$mode_id")&serial=$state_serial&verify_http=${verify_code:-000}&verify_ok=${verify_ok:-missing}"
    return 1
  fi

  temporary_code=$(core_proof_request display-apply-temporary "$temporary_file" || true)
  temporary_ok=$(json_field "$temporary_file" ok)
  if [ "$temporary_code" != "200" ] || [ "$temporary_ok" != "true" ]; then
    proof_multi_display_apply "status=fail&stage=temporary&status_route=/v1/displays/status&apply_route=/v1/displays/apply&display_config=org.gnome.Mutter.DisplayConfig&connector=$(proof_query_value "$connector")&mode_id=$(proof_query_value "$mode_id")&serial=$state_serial&verify_http=200&verify_ok=true&temporary_http=${temporary_code:-000}&temporary_ok=${temporary_ok:-missing}"
    return 1
  fi

  guard_code=$(core_proof_request display-apply-persistent-guard "$persistent_guard_file" || true)
  guard_ok=$(json_field "$persistent_guard_file" ok)
  if [ "$guard_code" != "400" ] || [ "$guard_ok" = "true" ]; then
    proof_multi_display_apply "status=fail&stage=persistent-guard&status_route=/v1/displays/status&apply_route=/v1/displays/apply&display_config=org.gnome.Mutter.DisplayConfig&connector=$(proof_query_value "$connector")&mode_id=$(proof_query_value "$mode_id")&serial=$state_serial&persistent_guard_http=${guard_code:-000}&persistent_guard_ok=${guard_ok:-missing}&persistent_confirmation_required=false"
    return 1
  fi

  stale_code=$(core_proof_request display-apply-stale "$stale_file" || true)
  stale_ok=$(json_field "$stale_file" ok)
  if [ "$stale_code" != "409" ] || [ "$stale_ok" = "true" ]; then
    proof_multi_display_apply "status=fail&stage=stale-serial&status_route=/v1/displays/status&apply_route=/v1/displays/apply&display_config=org.gnome.Mutter.DisplayConfig&connector=$(proof_query_value "$connector")&mode_id=$(proof_query_value "$mode_id")&serial=$state_serial&stale_serial=${stale_serial:-missing}&stale_serial_http=${stale_code:-000}&stale_serial_ok=${stale_ok:-missing}&stale_serial_rejected=false"
    return 1
  fi

  proof_multi_display_apply "status=pass&status_route=/v1/displays/status&apply_route=/v1/displays/apply&display_config=org.gnome.Mutter.DisplayConfig&connector=$(proof_query_value "$connector")&mode_id=$(proof_query_value "$mode_id")&serial=$state_serial&verify_http=200&verify_ok=true&temporary_http=200&temporary_ok=true&persistent_guard_http=400&persistent_confirmation_required=true&stale_serial=$stale_serial&stale_serial_http=409&stale_serial_rejected=true&roundtrip_restored=true&persistent_keep_claim=false&same_layout_noop=true"
  return 0
}
restore_focus_roundtrip_state(){
  local original_modes="$1"
  local original_active="$2"
  local original_armed="$3"
  local original_restore="$4"
  local original_banners="$5"

  [ -n "$original_modes" ] && gsettings set org.goblins.os.focus modes "$original_modes" >/dev/null 2>&1 || true
  gsettings set org.goblins.os.focus active-mode "$original_active" >/dev/null 2>&1 || true
  [ -n "$original_armed" ] && gsettings set org.goblins.os.focus armed-by-schedule "$original_armed" >/dev/null 2>&1 || true
  gsettings set org.goblins.os.focus restore-banners "$original_restore" >/dev/null 2>&1 || true
  [ -n "$original_banners" ] && gsettings set org.gnome.desktop.notifications show-banners "$original_banners" >/dev/null 2>&1 || true
}
permission_store_permissions_variant(){
  python3 - "$1" <<'PY'
import ast
import sys

try:
    value = ast.literal_eval(sys.argv[1].strip())
    permissions = value[0] if isinstance(value, tuple) and value else []
    if not isinstance(permissions, (list, tuple)):
        permissions = []
    print("[" + ", ".join(repr(str(item)) for item in permissions) + "]")
except Exception:
    print("[]")
PY
}
permission_store_get_permission(){
  gdbus call --session \
    --dest org.freedesktop.impl.portal.PermissionStore \
    --object-path /org/freedesktop/impl/portal/PermissionStore \
    --method org.freedesktop.impl.portal.PermissionStore.GetPermission \
    "$1" "$2" "$3" 2>/dev/null || true
}
permission_store_set_permission(){
  local permissions="$4"
  local typed_permissions="$permissions"
  local plain_permissions="$permissions"
  local set_log=/tmp/gate-app-privacy-permission-store-set.log
  local typed_log=/tmp/gate-app-privacy-permission-store-set-typed.log
  local mode_file=/tmp/gate-app-privacy-permission-store-set-mode

  case "$typed_permissions" in
    @as*) ;;
    *) typed_permissions="@as $typed_permissions" ;;
  esac
  case "$plain_permissions" in
    "@as "*) plain_permissions="${plain_permissions#@as }" ;;
    @as*) plain_permissions="${plain_permissions#@as}" ;;
  esac

  if gdbus call --session \
    --dest org.freedesktop.impl.portal.PermissionStore \
    --object-path /org/freedesktop/impl/portal/PermissionStore \
    --method org.freedesktop.impl.portal.PermissionStore.SetPermission \
    "$1" true "$2" "$3" "$typed_permissions" >"$set_log" 2>&1; then
    printf 'typed\n' >"$mode_file"
    return 0
  fi

  cp "$set_log" "$typed_log" 2>/dev/null || true
  if [ "$plain_permissions" != "$typed_permissions" ] && gdbus call --session \
    --dest org.freedesktop.impl.portal.PermissionStore \
    --object-path /org/freedesktop/impl/portal/PermissionStore \
    --method org.freedesktop.impl.portal.PermissionStore.SetPermission \
    "$1" true "$2" "$3" "$plain_permissions" >"$set_log" 2>&1; then
    printf 'plain\n' >"$mode_file"
    return 0
  fi

  printf 'failed\n' >"$mode_file"
  return 1
}
permission_store_set_attempt(){
  cat /tmp/gate-app-privacy-permission-store-set-mode 2>/dev/null || printf 'missing'
}
permission_store_set_error(){
  proof_query_value "$(cat /tmp/gate-app-privacy-permission-store-set.log 2>/dev/null || true)"
}
permission_store_delete_permission(){
  gdbus call --session \
    --dest org.freedesktop.impl.portal.PermissionStore \
    --object-path /org/freedesktop/impl/portal/PermissionStore \
    --method org.freedesktop.impl.portal.PermissionStore.DeletePermission \
    "$1" "$2" "$3" >/dev/null 2>&1 || true
}
restore_app_privacy_gate_permission(){
  local table="$1"
  local id="$2"
  local app="$3"
  local prior_permissions="$4"

  if [ -n "$prior_permissions" ] && [ "$prior_permissions" != "[]" ]; then
    permission_store_set_permission "$table" "$id" "$app" "$prior_permissions" || true
  else
    permission_store_delete_permission "$table" "$id" "$app"
  fi
}
app_privacy_revoke_proof(){
  local table=location
  local app=org.goblins.GatePrivacyProof
  local id=org.goblins.GatePrivacyProof
  local revoke_file=/tmp/gate-app-privacy-revoke.json
  local prior_reply prior_permissions seeded_reply seeded_permissions seed_attempt seed_error
  local revoke_code revoke_ok after_reply after_permissions restored_reply restored_permissions
  local seed_readback post_revoke_absent restore_prior_state

  for _ in $(seq 1 60); do
    curl -sf "$CORE_HEALTH_URL" >/dev/null 2>&1 && break
    sleep 0.5
  done

  if ! command -v gdbus >/dev/null 2>&1; then
    proof_app_privacy_revoke "status=fail&stage=gdbus&route=/v1/app-privacy/revoke&permission_store=missing"
    return 1
  fi
  if ! wait_session_bus_name org.freedesktop.impl.portal.PermissionStore; then
    proof_app_privacy_revoke "status=fail&stage=permission-store&route=/v1/app-privacy/revoke&permission_store=inactive"
    return 1
  fi
  if ! mkdir -p "$HOME/.local/share/flatpak/db"; then
    proof_app_privacy_revoke "status=fail&stage=permission-db-dir&route=/v1/app-privacy/revoke&permission_store=active&db_dir=$HOME/.local/share/flatpak/db"
    return 1
  fi

  prior_reply="$(permission_store_get_permission "$table" "$id" "$app")"
  prior_permissions="$(permission_store_permissions_variant "$prior_reply")"
  if ! permission_store_set_permission "$table" "$id" "$app" "['yes']"; then
    seed_attempt="$(permission_store_set_attempt)"
    seed_error="$(permission_store_set_error)"
    proof_app_privacy_revoke "status=fail&stage=seed&route=/v1/app-privacy/revoke&table=$table&app=$app&seed_method=PermissionStore.SetPermission&seed_grant=yes&seed_attempt=$seed_attempt&seed_error=$seed_error"
    return 1
  fi
  seed_attempt="$(permission_store_set_attempt)"
  seeded_reply="$(permission_store_get_permission "$table" "$id" "$app")"
  seeded_permissions="$(permission_store_permissions_variant "$seeded_reply")"
  seed_readback=false
  [ "$seeded_permissions" = "['yes']" ] && seed_readback=true
  if [ "$seed_readback" != "true" ]; then
    restore_app_privacy_gate_permission "$table" "$id" "$app" "$prior_permissions"
    proof_app_privacy_revoke "status=fail&stage=seed-readback&route=/v1/app-privacy/revoke&table=$table&app=$app&seed_method=PermissionStore.SetPermission&seed_grant=yes&seed_attempt=$seed_attempt&seed_readback=false"
    return 1
  fi

  revoke_code=$(core_proof_request app-privacy-revoke "$revoke_file" || true)
  revoke_ok=$(json_field "$revoke_file" ok)
  after_reply="$(permission_store_get_permission "$table" "$id" "$app")"
  after_permissions="$(permission_store_permissions_variant "$after_reply")"
  post_revoke_absent=false
  [ "$after_permissions" = "[]" ] && post_revoke_absent=true
  if [ "$revoke_code" != "200" ] || [ "$revoke_ok" != "true" ] || [ "$post_revoke_absent" != "true" ]; then
    restore_app_privacy_gate_permission "$table" "$id" "$app" "$prior_permissions"
    proof_app_privacy_revoke "status=fail&stage=revoke&route=/v1/app-privacy/revoke&table=$table&app=$app&seed_method=PermissionStore.SetPermission&revoke_method=PermissionStore.DeletePermission&readback_method=PermissionStore.GetPermission&seed_attempt=$seed_attempt&seed_readback=true&revoke_http=${revoke_code:-000}&revoke_ok=${revoke_ok:-missing}&post_revoke_absent=$post_revoke_absent"
    return 1
  fi

  restore_app_privacy_gate_permission "$table" "$id" "$app" "$prior_permissions"
  restored_reply="$(permission_store_get_permission "$table" "$id" "$app")"
  restored_permissions="$(permission_store_permissions_variant "$restored_reply")"
  restore_prior_state=false
  [ "$restored_permissions" = "$prior_permissions" ] && restore_prior_state=true
  if [ "$restore_prior_state" != "true" ]; then
    proof_app_privacy_revoke "status=fail&stage=restore&route=/v1/app-privacy/revoke&table=$table&app=$app&restore_prior_state=false&roundtrip_restored=false"
    return 1
  fi

  proof_app_privacy_revoke "status=pass&route=/v1/app-privacy/revoke&table=$table&app=$app&id=$id&seed_method=PermissionStore.SetPermission&revoke_method=PermissionStore.DeletePermission&readback_method=PermissionStore.GetPermission&seed_grant=yes&seed_attempt=$seed_attempt&seed_readback=true&revoke_http=200&revoke_ok=true&post_revoke_absent=true&restore_prior_state=true&roundtrip_restored=true&resource_keyed_claim=false&device_revoke_claim=false"
  return 0
}
focus_arm_roundtrip_proof(){
  local status_file=/tmp/gate-focus-status.json
  local activate_file=/tmp/gate-focus-activate.json
  local deactivate_file=/tmp/gate-focus-deactivate.json
  local focus_mode_seed_code
  local original_modes original_active_raw original_active original_armed original_restore_raw original_restore original_banners
  local status_code available activate_code activate_ok activate_active active_after_activate
  local armed_after_activate restore_after_activate banners_after_activate
  local deactivate_code deactivate_ok deactivate_active active_after_deactivate
  local armed_after_deactivate restore_after_deactivate banners_after_deactivate
  local modes_after_restore active_after_restore armed_after_restore restore_after_restore banners_after_restore
  local original_focus_state_restored original_notification_banners_restored

  for _ in $(seq 1 60); do
    curl -sf "$CORE_HEALTH_URL" >/dev/null 2>&1 && break
    sleep 0.5
  done

  original_modes="$(gsettings get org.goblins.os.focus modes 2>/dev/null || true)"
  original_active_raw="$(gsettings get org.goblins.os.focus active-mode 2>/dev/null || true)"
  original_active="$(gsettings_string_value "$original_active_raw")"
  original_armed="$(gsettings get org.goblins.os.focus armed-by-schedule 2>/dev/null || true)"
  original_restore_raw="$(gsettings get org.goblins.os.focus restore-banners 2>/dev/null || true)"
  original_restore="$(gsettings_string_value "$original_restore_raw")"
  original_banners="$(gsettings get org.gnome.desktop.notifications show-banners 2>/dev/null || true)"
  if [ -z "$original_modes" ] || [ -z "$original_active_raw" ] || [ -z "$original_armed" ] || [ -z "$original_restore_raw" ] || [ -z "$original_banners" ]; then
    proof_focus_arm_roundtrip "status=fail&stage=baseline&status_route=/v1/focus/status&activate_route=/v1/focus/activate&deactivate_route=/v1/focus/deactivate&schema=org.goblins.os.focus"
    return 1
  fi

  if ! gsettings set org.goblins.os.focus modes "'[]'" >/dev/null 2>&1 \
    || ! gsettings set org.goblins.os.focus active-mode '' >/dev/null 2>&1 \
    || ! gsettings set org.goblins.os.focus armed-by-schedule false >/dev/null 2>&1 \
    || ! gsettings set org.goblins.os.focus restore-banners '' >/dev/null 2>&1 \
    || ! gsettings set org.gnome.desktop.notifications show-banners true >/dev/null 2>&1; then
    restore_focus_roundtrip_state "$original_modes" "$original_active" "$original_armed" "$original_restore" "$original_banners"
    proof_focus_arm_roundtrip "status=fail&stage=seed&status_route=/v1/focus/status&activate_route=/v1/focus/activate&deactivate_route=/v1/focus/deactivate&test_mode=gate-work&test_mode_configured=false"
    return 1
  fi
  focus_mode_seed_code=$(core_proof_request focus-mode-seed /tmp/gate-focus-mode-seed.json || true)
  if [ "$focus_mode_seed_code" != "200" ] || [ "$(json_field /tmp/gate-focus-mode-seed.json ok)" != "true" ]; then
    restore_focus_roundtrip_state "$original_modes" "$original_active" "$original_armed" "$original_restore" "$original_banners"
    proof_focus_arm_roundtrip "status=fail&stage=seed&status_route=/v1/focus/status&activate_route=/v1/focus/activate&deactivate_route=/v1/focus/deactivate&test_mode=gate-work&test_mode_configured=false"
    return 1
  fi

  status_code=$(core_proof_request focus-status "$status_file" || true)
  available=$(json_field "$status_file" available)
  if [ "$status_code" != "200" ] || [ "$available" != "true" ]; then
    restore_focus_roundtrip_state "$original_modes" "$original_active" "$original_armed" "$original_restore" "$original_banners"
    proof_focus_arm_roundtrip "status=fail&stage=status&status_route=/v1/focus/status&status_http=${status_code:-000}&available=${available:-missing}&test_mode=gate-work"
    return 1
  fi

  activate_code=$(core_proof_request focus-activate "$activate_file" || true)
  activate_ok=$(json_field "$activate_file" ok)
  activate_active=$(json_field "$activate_file" active_mode)
  active_after_activate="$(gsettings_string_value "$(gsettings get org.goblins.os.focus active-mode 2>/dev/null || true)")"
  armed_after_activate="$(gsettings get org.goblins.os.focus armed-by-schedule 2>/dev/null || true)"
  restore_after_activate="$(gsettings_string_value "$(gsettings get org.goblins.os.focus restore-banners 2>/dev/null || true)")"
  banners_after_activate="$(gsettings get org.gnome.desktop.notifications show-banners 2>/dev/null || true)"
  if [ "$activate_code" != "200" ] || [ "$activate_ok" != "true" ] || [ "$activate_active" != "gate-work" ] \
    || [ "$active_after_activate" != "gate-work" ] || [ "$armed_after_activate" != "false" ] \
    || [ "$restore_after_activate" != "true" ] || [ "$banners_after_activate" != "false" ]; then
    restore_focus_roundtrip_state "$original_modes" "$original_active" "$original_armed" "$original_restore" "$original_banners"
    proof_focus_arm_roundtrip "status=fail&stage=activate&status_route=/v1/focus/status&activate_route=/v1/focus/activate&deactivate_route=/v1/focus/deactivate&test_mode=gate-work&activate_http=${activate_code:-000}&activate_ok=${activate_ok:-missing}&activate_active_mode=${activate_active:-missing}&active_mode_gsettings_readback=${active_after_activate:-missing}&armed_by_schedule_after_activate=${armed_after_activate:-missing}&restore_banners_after_activate=${restore_after_activate:-missing}&notification_banners_after_activate=${banners_after_activate:-missing}"
    return 1
  fi

  deactivate_code=$(core_proof_request focus-deactivate "$deactivate_file" || true)
  deactivate_ok=$(json_field "$deactivate_file" ok)
  deactivate_active=$(json_field "$deactivate_file" active_mode)
  active_after_deactivate="$(gsettings_string_value "$(gsettings get org.goblins.os.focus active-mode 2>/dev/null || true)")"
  armed_after_deactivate="$(gsettings get org.goblins.os.focus armed-by-schedule 2>/dev/null || true)"
  restore_after_deactivate="$(gsettings_string_value "$(gsettings get org.goblins.os.focus restore-banners 2>/dev/null || true)")"
  banners_after_deactivate="$(gsettings get org.gnome.desktop.notifications show-banners 2>/dev/null || true)"
  if [ "$deactivate_code" != "200" ] || [ "$deactivate_ok" != "true" ] || [ -n "$deactivate_active" ] \
    || [ -n "$active_after_deactivate" ] || [ "$armed_after_deactivate" != "false" ] \
    || [ -n "$restore_after_deactivate" ] || [ "$banners_after_deactivate" != "true" ]; then
    restore_focus_roundtrip_state "$original_modes" "$original_active" "$original_armed" "$original_restore" "$original_banners"
    proof_focus_arm_roundtrip "status=fail&stage=deactivate&status_route=/v1/focus/status&activate_route=/v1/focus/activate&deactivate_route=/v1/focus/deactivate&test_mode=gate-work&deactivate_http=${deactivate_code:-000}&deactivate_ok=${deactivate_ok:-missing}&deactivate_active_mode=${deactivate_active:-missing}&active_mode_after_deactivate=${active_after_deactivate:-missing}&armed_by_schedule_after_deactivate=${armed_after_deactivate:-missing}&restore_banners_after_deactivate=${restore_after_deactivate:-missing}&notification_banners_after_deactivate=${banners_after_deactivate:-missing}"
    return 1
  fi

  restore_focus_roundtrip_state "$original_modes" "$original_active" "$original_armed" "$original_restore" "$original_banners"
  modes_after_restore="$(gsettings get org.goblins.os.focus modes 2>/dev/null || true)"
  active_after_restore="$(gsettings_string_value "$(gsettings get org.goblins.os.focus active-mode 2>/dev/null || true)")"
  armed_after_restore="$(gsettings get org.goblins.os.focus armed-by-schedule 2>/dev/null || true)"
  restore_after_restore="$(gsettings_string_value "$(gsettings get org.goblins.os.focus restore-banners 2>/dev/null || true)")"
  banners_after_restore="$(gsettings get org.gnome.desktop.notifications show-banners 2>/dev/null || true)"
  original_focus_state_restored=false
  original_notification_banners_restored=false
  [ "$modes_after_restore" = "$original_modes" ] && [ "$active_after_restore" = "$original_active" ] \
    && [ "$armed_after_restore" = "$original_armed" ] && [ "$restore_after_restore" = "$original_restore" ] \
    && original_focus_state_restored=true
  [ "$banners_after_restore" = "$original_banners" ] && original_notification_banners_restored=true
  if [ "$original_focus_state_restored" != "true" ] || [ "$original_notification_banners_restored" != "true" ]; then
    proof_focus_arm_roundtrip "status=fail&stage=restore&status_route=/v1/focus/status&activate_route=/v1/focus/activate&deactivate_route=/v1/focus/deactivate&test_mode=gate-work&original_focus_state_restored=$original_focus_state_restored&original_notification_banners_restored=$original_notification_banners_restored&roundtrip_restored=false"
    return 1
  fi

  proof_focus_arm_roundtrip "status=pass&status_route=/v1/focus/status&activate_route=/v1/focus/activate&deactivate_route=/v1/focus/deactivate&status_http=200&available=true&test_mode=gate-work&test_mode_configured=true&baseline_active_mode=&baseline_banners=true&activate_http=200&activate_ok=true&activate_active_mode=gate-work&active_mode_gsettings_readback=gate-work&armed_by_schedule_after_activate=false&restore_banners_after_activate=true&notification_banners_after_activate=false&deactivate_http=200&deactivate_ok=true&deactivate_active_mode=&active_mode_after_deactivate=&armed_by_schedule_after_deactivate=false&restore_banners_after_deactivate=&notification_banners_after_deactivate=true&original_focus_state_restored=true&original_notification_banners_restored=true&roundtrip_restored=true&mode_crud_claim=false&schedule_claim=false&per_app_breakthroughs_claim=false"
  return 0
}
preview_open_render_proof(){
  local preview_pdf=/usr/share/goblins-os/proof/preview-open-render.pdf
  local preview_png=/usr/share/goblins-os/proof/preview-open-render.png
  local preview_txt=/usr/share/goblins-os/proof/preview-open-render.txt
  local status_file=/tmp/gate-preview-status.json
  local pdf_file=/tmp/gate-preview-open-pdf.json
  local image_file=/tmp/gate-preview-open-image.json
  local unsupported_file=/tmp/gate-preview-open-unsupported.json
  local status_code pdf_code image_code unsupported_code
  local available xdg_open papers loupe pdf_default image_default jpeg_default
  local pdf_ok pdf_kind image_ok image_kind unsupported_ok

  pkill -x papers 2>/dev/null || true
  pkill -x loupe 2>/dev/null || true

  for _ in $(seq 1 60); do
    curl -sf "$CORE_HEALTH_URL" >/dev/null 2>&1 && break
    sleep 0.5
  done

  if [ ! -r "$preview_pdf" ] || [ ! -r "$preview_png" ] || [ ! -r "$preview_txt" ]; then
    proof_preview_open_render "status=fail&stage=fixtures&status_route=/v1/preview/status&route=/v1/preview/open&pdf_fixture=$preview_pdf&image_fixture=$preview_png"
    return 1
  fi

  pdf_default="$(xdg-mime query default application/pdf 2>/dev/null || true)"
  image_default="$(xdg-mime query default image/png 2>/dev/null || true)"
  jpeg_default="$(xdg-mime query default image/jpeg 2>/dev/null || true)"
  if [ "$pdf_default" != "org.gnome.Papers.desktop" ] || [ "$image_default" != "org.gnome.Loupe.desktop" ] || [ "$jpeg_default" != "org.gnome.Loupe.desktop" ]; then
    proof_preview_open_render "status=fail&stage=xdg-mime&status_route=/v1/preview/status&route=/v1/preview/open&pdf_default=${pdf_default:-missing}&image_default=${image_default:-missing}&jpeg_default=${jpeg_default:-missing}"
    return 1
  fi

  status_code=$(core_proof_request preview-status "$status_file" || true)
  available=$(json_field "$status_file" available)
  xdg_open=$(json_field "$status_file" xdg_open_available)
  papers=$(json_field "$status_file" papers_available)
  loupe=$(json_field "$status_file" loupe_available)
  if [ "$status_code" != "200" ] || [ "$available" != "true" ] || [ "$xdg_open" != "true" ] || [ "$papers" != "true" ] || [ "$loupe" != "true" ]; then
    proof_preview_open_render "status=fail&stage=status&status_route=/v1/preview/status&route=/v1/preview/open&status_http=${status_code:-000}&available=${available:-missing}&xdg_open=${xdg_open:-missing}&papers=${papers:-missing}&loupe=${loupe:-missing}&pdf_default=$pdf_default&image_default=$image_default"
    return 1
  fi

  pdf_code=$(core_proof_request preview-open-pdf "$pdf_file" || true)
  pdf_ok=$(json_field "$pdf_file" ok)
  pdf_kind=$(json_field "$pdf_file" kind)
  if [ "$pdf_code" != "200" ] || [ "$pdf_ok" != "true" ] || [ "$pdf_kind" != "pdf" ] || ! wait_process_or_bus papers org.gnome.Papers; then
    proof_preview_open_render "status=fail&stage=pdf-open&status_route=/v1/preview/status&route=/v1/preview/open&status_http=200&available=true&xdg_open=true&papers=true&loupe=true&pdf_default=$pdf_default&image_default=$image_default&pdf_http=${pdf_code:-000}&pdf_ok=${pdf_ok:-missing}&pdf_kind=${pdf_kind:-missing}"
    pkill -x papers 2>/dev/null || true
    return 1
  fi
  sleep 5
  sig 29-preview-pdf-open
  pkill -x papers 2>/dev/null || true

  image_code=$(core_proof_request preview-open-image "$image_file" || true)
  image_ok=$(json_field "$image_file" ok)
  image_kind=$(json_field "$image_file" kind)
  if [ "$image_code" != "200" ] || [ "$image_ok" != "true" ] || [ "$image_kind" != "image" ] || ! wait_process_or_bus loupe org.gnome.Loupe; then
    proof_preview_open_render "status=fail&stage=image-open&status_route=/v1/preview/status&route=/v1/preview/open&status_http=200&available=true&xdg_open=true&papers=true&loupe=true&pdf_default=$pdf_default&image_default=$image_default&pdf_http=200&pdf_ok=true&pdf_kind=pdf&image_http=${image_code:-000}&image_ok=${image_ok:-missing}&image_kind=${image_kind:-missing}&pdf_screenshot=29-preview-pdf-open.png&rendered_pdf_frame=true"
    pkill -x loupe 2>/dev/null || true
    return 1
  fi
  sleep 5
  sig 30-preview-image-open
  pkill -x loupe 2>/dev/null || true

  unsupported_code=$(core_proof_request preview-open-unsupported "$unsupported_file" || true)
  unsupported_ok=$(json_field "$unsupported_file" ok)
  if [ "$unsupported_code" != "400" ] || [ "$unsupported_ok" = "true" ]; then
    proof_preview_open_render "status=fail&stage=unsupported&status_route=/v1/preview/status&route=/v1/preview/open&status_http=200&available=true&xdg_open=true&papers=true&loupe=true&pdf_default=$pdf_default&image_default=$image_default&pdf_http=200&pdf_ok=true&pdf_kind=pdf&image_http=200&image_ok=true&image_kind=image&unsupported_http=${unsupported_code:-000}&unsupported_ok=${unsupported_ok:-missing}&pdf_screenshot=29-preview-pdf-open.png&image_screenshot=30-preview-image-open.png&rendered_pdf_frame=true&rendered_image_frame=true"
    return 1
  fi

  proof_preview_open_render "status=pass&status_route=/v1/preview/status&route=/v1/preview/open&status_http=200&available=true&xdg_open=true&papers=true&loupe=true&pdf_default=org.gnome.Papers.desktop&image_default=org.gnome.Loupe.desktop&jpeg_default=org.gnome.Loupe.desktop&pdf_http=200&pdf_ok=true&pdf_kind=pdf&pdf_process=papers&pdf_screenshot=29-preview-pdf-open.png&rendered_pdf_frame=true&image_http=200&image_ok=true&image_kind=image&image_process=loupe&image_screenshot=30-preview-image-open.png&rendered_image_frame=true&unsupported_http=400&unsupported_ok=false&unsupported_rejected=true"
  return 0
}
# shot <name> <cmd...>  (env prefixes before `shot` propagate into the launch)
# Capture launches run in the current GNOME session with a capture-only non-unique
# GtkApplication flag. We still kill/wait around each shot so stale windows cannot
# re-focus or overlap the next proof surface.
shot(){
  local n="$1"
  shift
  local bin="$1"
  local base
  base="$(basename "$bin" 2>/dev/null || printf '%s' "$bin")"
  local log="/tmp/gate-shot-$n.log"
  local settle="${GOS_SHOT_SETTLE_SECONDS:-12}"
  local title_ready=true
  local env_args=(
    "GOBLINS_OS_CAPTURE_NON_UNIQUE=1"
    "GOBLINS_OS_RENDER_FULLSCREEN=1"
  )
  for key in GOBLINS_OS_THEME GOBLINS_OS_INSTALLER_PAGE GOBLINS_OS_INSTALLER_CORE_WAIT_SECS GOBLINS_OS_SETTINGS_CORE_WAIT_SECS GOBLINS_OS_CAPTURE_PRESENT_LEDGER; do
    if [ "${!key+x}" ]; then
      env_args+=("$key=${!key}")
    fi
  done
  echo "GOBLINS_HWGATE_SHOT_START name=$n command=$*"
  switch_control_off
  # Stray QMP keystrokes from earlier typing proofs can leave the shell
  # overview/search covering the work area; clear it before every capture.
  dismiss_shell_overview "shot-$n"
  pkill -x "$base" 2>/dev/null || true
  pkill -f -- "$bin" 2>/dev/null || true
  sleep 0.5
  env "${env_args[@]}" "$@" >"$log" 2>&1 &
  local p=$!
  sleep "$settle"
  if ! kill -0 "$p" 2>/dev/null; then
    echo "GOBLINS_HWGATE_SHOT_EXITED_BEFORE_CAPTURE name=$n command=$*"
    tail -n 80 "$log" 2>/dev/null || true
  fi
  switch_control_off
  if [ -n "${GOBLINS_OS_CAPTURE_EXPECT_TITLE:-}" ]; then
    local title_probe=wait_for_window_title
    if [ -n "${GOBLINS_OS_CAPTURE_PRESENT_LEDGER:-}" ]; then
      title_probe=wait_for_present_ledger
    fi
    if "$title_probe" "$GOBLINS_OS_CAPTURE_EXPECT_TITLE" "${GOS_SHOT_WINDOW_WAIT_ATTEMPTS:-40}"; then
      echo "GOBLINS_HWGATE_SHOT_WINDOW_READY name=$n title=$GOBLINS_OS_CAPTURE_EXPECT_TITLE probe=$title_probe"
    else
      echo "GOBLINS_HWGATE_SHOT_WINDOW_MISSING name=$n title=$GOBLINS_OS_CAPTURE_EXPECT_TITLE probe=$title_probe"
      title_ready=false
    fi
  fi
  echo "GOBLINS_HWGATE_SHOT_SIGNALING name=$n"
  sig "$n"
  echo "GOBLINS_HWGATE_SHOT_SIGNALED name=$n"
  kill "$p" 2>/dev/null || true
  pkill -x "$base" 2>/dev/null || true
  pkill -f -- "$bin" 2>/dev/null || true
  for _ in $(seq 1 24); do
    if ! pgrep -x "$base" >/dev/null 2>&1 && ! pgrep -f -- "$bin" >/dev/null 2>&1; then
      break
    fi
    sleep 0.3
  done
  switch_control_off
  sleep 1
  [ "$title_ready" = "true" ]
}
installer_shot(){
  local page="$1"
  local name="$2"
  GOBLINS_OS_INSTALLER_CORE_WAIT_SECS="${GOS_INSTALLER_CAPTURE_CORE_WAIT_SECS:-3}" \
    GOBLINS_OS_INSTALLER_PAGE="$page" \
    shot "$name" "$B/goblins-os-installer"
}
darkon(){ gsettings set org.gnome.desktop.interface color-scheme prefer-dark 2>/dev/null; sleep 1; }
darkoff(){ gsettings set org.gnome.desktop.interface color-scheme default 2>/dev/null; sleep 1; }

sleep 3
curl -s "http://$H/ready/ORCH_START" >/dev/null 2>&1
pkill -f goblins-os-login 2>/dev/null; pkill -f goblins-os-installer 2>/dev/null; sleep 2
dismiss_shell_overview text-shortcuts-proof-start
switch_control_off
firewall_live_toggle_proof || true
text_shortcuts_session_enable_proof || true
text_shortcuts_candidate_metadata_proof || true
text_shortcuts_overlay_intent_proof || true
text_shortcuts_candidate_bubble_frame_proof || true
text_shortcuts_candidate_bubble_layout_proof || true
text_shortcuts_candidate_bubble_render_intent_proof || true
text_shortcuts_candidate_bubble_render_proof || true
text_shortcuts_live_ibus_runtime_render_proof || true
keyboard_shortcuts_roundtrip_proof || true
input_sources_roundtrip_proof || true
multi_display_apply_proof || true
focus_arm_roundtrip_proof || true
app_privacy_revoke_proof || true
preview_open_render_proof || true

# The verification-only root service owns the fixture block/state directories
# and later swaps the fixture daemon onto the same production sockets. The
# session never receives a core URL, group membership, or second listener.
CAPTURE_LOCAL_MODEL=llama3.2:1b
CAPTURE_LOCAL_MODEL_JSON="$(json_string_literal "$CAPTURE_LOCAL_MODEL")"
CAPTURE_MODEL_RUNTIME_URL=http://127.0.0.1:41134
CAPTURE_MODEL_RELAY_URL=http://127.0.0.1:41135/v1/resident
CAPTURE_MODEL_KEEP_ALIVE=30m
CAPTURE_MODEL_KEEP_ALIVE_JSON="$(json_string_literal "$CAPTURE_MODEL_KEEP_ALIVE")"
MODEL_LOOPBACK_READY=false
if start_capture_model_loopback; then
  MODEL_LOOPBACK_READY=true
fi
rm -f /tmp/model-direct.json /tmp/model-direct.err
if [ "$MODEL_LOOPBACK_READY" = "true" ]; then
  curl -s --max-time 120 -X POST "$CAPTURE_MODEL_RUNTIME_URL/api/generate" \
    -H 'content-type: application/json' \
    -d "{\"model\":$CAPTURE_LOCAL_MODEL_JSON,\"prompt\":\"Reply with READY.\",\"stream\":false,\"keep_alive\":$CAPTURE_MODEL_KEEP_ALIVE_JSON}" \
    >/tmp/model-direct.json 2>/tmp/model-direct.err || true
fi
MODEL_CONTRACT_READY=false
if start_capture_model_contract_relay; then
  MODEL_CONTRACT_READY=true
fi

# ---- login + onboarding ----
shot 03-login         "$B/goblins-os-login"
installer_shot welcome 06-onboarding
sig 04-desktop

# ---- session apps (light) ----
shot 07-home          "$B/goblins-os-shell"
shot 08-shell-home    "$B/goblins-os-shell"
shot 10-settings      "$B/goblins-os-settings"
shot 11-settings-models "$B/goblins-os-settings" --panel=models
shot 13-studio-before "$B/goblins-os-shell" --studio
audio_output_shot
shot 23-controller-detection "$B/goblins-os-settings" --panel=games

# ---- light/dark motion (shell mid-interaction is the closest honest motion frame) ----
shot 18-light-motion  "$B/goblins-os-shell"

# ---- dark variants ----
darkon
GOBLINS_OS_THEME=dark shot 09-shell-dark    "$B/goblins-os-shell"
GOBLINS_OS_THEME=dark shot 12-settings-dark "$B/goblins-os-settings"
GOBLINS_OS_THEME=dark shot 17-dark-motion   "$B/goblins-os-shell"
darkoff

# Swap the root-owned fixture daemon onto the same production capability
# sockets. Installed UIs keep their fixed transport; a timed systemd service
# and the EXIT trap both restore the real daemon.
fixture_start_http="$(core_proof_request fixture-start /tmp/gate-fixture-start.json || true)"
if [ "$fixture_start_http" != "200" ] || [ "$(json_field /tmp/gate-fixture-start.json ok)" != "true" ]; then
  echo "GOBLINS_HWGATE_FIXTURE_CORE_START_FAILED http=${fixture_start_http:-000}"
  exit 1
fi
FIXTURE_ACTIVE=true

# ---- installer pages through the root-owned multi-OS fixture core ----
installer_shot appearance 01-installer
installer_shot network 02-install-network
installer_shot install-disk 25-install-destination
installer_shot install-review 26-install-storage-summary
installer_shot details 28-bootloader-efi-summary

# ---- dual-boot preservation (fixture core shows the multi-OS disk) ----
installer_shot install-disk 27-dual-boot-preserve-existing-os

# ---- gaming stack (real software substrate) ----
shot 19-vulkan-vkcube  vkcube
shot 20-gamemode-active gamemoderun vkcube
shot 22-mangohud-overlay mangohud vkcube
shot 21-gamescope-session gamescope -W 960 -H 600 -b -- vkcube

# ---- studio-live (needs the host model; best-effort) ----
rm -f /tmp/build.json /tmp/build.err /tmp/build.rc /tmp/app-builder-grant.json /tmp/policy-status.json
if [ "$MODEL_CONTRACT_READY" != "true" ]; then
  printf '{"ok":false,"text":"Capture model contract relay was not ready.","app":null}\n' >/tmp/build.json
  : >/tmp/build.err
  echo 1 >/tmp/build.rc
  build_pid=""
elif grant_policy_permission app-builder /tmp/app-builder-grant.json /tmp/policy-status.json; then
  (
    set +e
    build_http="$(core_proof_request app-build /tmp/build.json 2>/tmp/build.err)"
    if [ "$build_http" = "200" ]; then build_rc=0; else build_rc=1; fi
    set -e
    echo "$build_rc" >/tmp/build.rc
  ) &
  build_pid=$!
else
  printf '{"ok":false,"text":"App builder permission grant failed.","app":null}\n' >/tmp/build.json
  : >/tmp/build.err
  echo 1 >/tmp/build.rc
  build_pid=""
fi
shot 14-studio-running "$B/goblins-os-shell" --studio
for _ in $(seq 1 60); do
  if [ -s /tmp/build.rc ]; then
    break
  fi
  sleep 1
done
if [ ! -s /tmp/build.rc ]; then
  [ -n "$build_pid" ] && kill "$build_pid" 2>/dev/null || true
  [ -n "$build_pid" ] && wait "$build_pid" 2>/dev/null || true
  proof_runtime_build "status=fail&stage=timeout&route=/v1/apps/builds&intent=$(proof_query_value "A focus timer that counts down 25 minutes and rings.")&engine_mode=local-model&engine_source=missing&built_artifact_id=missing&built_artifact_name=missing&response_bytes=$(file_size_value /tmp/build.json)&error_tail=$(file_tail_query_value /tmp/build.err)"
else
  [ -n "$build_pid" ] && wait "$build_pid" || true
  build_id="$(json_field /tmp/build.json app.id)"
  build_name="$(json_field /tmp/build.json app.name)"
  build_source="$(json_field /tmp/build.json app.source)"
  build_intent="$(json_field /tmp/build.json app.intent)"
  if [ -n "$build_id" ] && [ -n "$build_name" ] && [ -n "$build_source" ]; then
    proof_runtime_build "status=pass&route=/v1/apps/builds&intent=$(proof_query_value "${build_intent:-A focus timer that counts down 25 minutes and rings.}")&engine_mode=local-model&engine_source=$(proof_query_value "$build_source")&built_artifact_id=$(proof_query_value "$build_id")&built_artifact_name=$(proof_query_value "$build_name")&response_bytes=$(file_size_value /tmp/build.json)"
  elif [ "$MODEL_CONTRACT_READY" != "true" ]; then
    proof_runtime_build "status=fail&stage=model-contract&route=/v1/apps/builds&runtime_url=$(proof_query_value "$CAPTURE_MODEL_RUNTIME_URL")&relay_url=$(proof_query_value "$CAPTURE_MODEL_RELAY_URL")&model=$(proof_query_value "$CAPTURE_LOCAL_MODEL")&keep_alive=$(proof_query_value "$CAPTURE_MODEL_KEEP_ALIVE")&engine_mode=local-model&engine_source=missing&built_artifact_id=missing&built_artifact_name=missing&response_bytes=$(file_size_value /tmp/build.json)&contract_direct_tail=$(file_tail_query_value /tmp/model-contract-direct.json)&contract_log_tail=$(file_tail_query_value /tmp/model-contract.log)&model_tags_tail=$(file_tail_query_value /tmp/model-loopback-tags.json)&model_loopback_tail=$(file_tail_query_value /tmp/model-loopback.log)&core_log_tail=$(file_tail_query_value /run/goblins-hwgate-core-proof/fixture-core.log)&resident_log_tail=$(file_tail_query_value /run/goblins-hwgate-core-proof/fixture-resident.log)&error_tail=$(file_tail_query_value /tmp/model-contract-direct.err)"
  elif [ -s /tmp/app-builder-grant.json ] && [ "$(json_field /tmp/app-builder-grant.json ok)" != "true" ]; then
    proof_runtime_build "status=fail&stage=permission-grant&route=/v1/apps/builds&grant_route=/v1/policy/permissions/grant&intent=$(proof_query_value "A focus timer that counts down 25 minutes and rings.")&engine_mode=local-model&engine_source=missing&built_artifact_id=missing&built_artifact_name=missing&response_bytes=$(file_size_value /tmp/build.json)&grant_response_tail=$(file_tail_query_value /tmp/app-builder-grant.json)"
  else
    proof_runtime_build "status=fail&stage=response&route=/v1/apps/builds&runtime_url=$(proof_query_value "$CAPTURE_MODEL_RUNTIME_URL")&relay_url=$(proof_query_value "$CAPTURE_MODEL_RELAY_URL")&model=$(proof_query_value "$CAPTURE_LOCAL_MODEL")&keep_alive=$(proof_query_value "$CAPTURE_MODEL_KEEP_ALIVE")&intent=$(proof_query_value "A focus timer that counts down 25 minutes and rings.")&engine_mode=local-model&engine_source=$(proof_query_value "${build_source:-missing}")&built_artifact_id=$(proof_query_value "${build_id:-missing}")&built_artifact_name=$(proof_query_value "${build_name:-missing}")&response_bytes=$(file_size_value /tmp/build.json)&response_tail=$(file_tail_query_value /tmp/build.json)&contract_direct_tail=$(file_tail_query_value /tmp/model-contract-direct.json)&contract_log_tail=$(file_tail_query_value /tmp/model-contract.log)&model_direct_tail=$(file_tail_query_value /tmp/model-direct.json)&model_error_tail=$(file_tail_query_value /tmp/model-direct.err)&model_loopback_tail=$(file_tail_query_value /tmp/model-loopback.log)&core_log_tail=$(file_tail_query_value /run/goblins-hwgate-core-proof/fixture-core.log)&resident_log_tail=$(file_tail_query_value /run/goblins-hwgate-core-proof/fixture-resident.log)&error_tail=$(file_tail_query_value /tmp/build.err)"
  fi
fi
shot 15-studio-app-detail "$B/goblins-os-shell" --studio
shot 16-built-app-open "$B/goblins-os-shell" --studio

if ! restore_fixture_core; then
  echo "GOBLINS_HWGATE_PRODUCTION_CORE_RESTORE_FAILED"
  exit 1
fi

curl -s "http://$H/ready/ORCH_ALLDONE" >/dev/null 2>&1
sleep 2
