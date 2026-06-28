#!/bin/bash
# Goblins OS hardware-gate in-session capture orchestrator (full 28-shot).
# Real captures of the real installed OS in the real VM. Gaming via the OS's own
# lavapipe/gamescope/pipewire software stack. Dual-boot via a fixture core
# (GOBLINS_OS_SYS_BLOCK_DIR, the render-harness mechanism) on an alt port.
exec >/tmp/gate-cap.log 2>&1
set -x
H=10.0.2.2:8099
B=/usr/libexec/goblins-os
LIVE_URL=http://127.0.0.1:8787
export GDK_BACKEND=wayland XDG_RUNTIME_DIR="${XDG_RUNTIME_DIR:-/run/user/1000}"
# Maximize every captured GTK surface so the host QMP screendump catches it filling
# the work area (keeping window chrome + the menu bar/dock) instead of an ambiguous
# windowed surface that may not be foregrounded at screendump time — the root cause
# of the prior duplicate-capture plateau. Honest: a framebuffer read of the real
# maximized surface, no compositor/session change. Login + installer already
# fullscreen by design.
export GOBLINS_OS_RENDER_FULLSCREEN=1
sig(){ curl -s "http://$H/ready/$1" >/dev/null 2>&1; sleep 5; }
proof_firewall(){ curl -s "http://$H/proof/firewall-live-toggle?$1" >/dev/null 2>&1 || true; }
proof_text_shortcuts(){ curl -s "http://$H/proof/text-shortcuts-session-enable?$1" >/dev/null 2>&1 || true; }
proof_text_shortcuts_live(){ curl -s "http://$H/proof/text-shortcuts-live-keystroke?$1" >/dev/null 2>&1 || true; }
proof_text_shortcuts_candidate(){ curl -s "http://$H/proof/text-shortcuts-candidate-metadata?$1" >/dev/null 2>&1 || true; }
proof_text_shortcuts_overlay_intent(){ curl -s "http://$H/proof/text-shortcuts-overlay-intent?$1" >/dev/null 2>&1 || true; }
proof_text_shortcuts_candidate_bubble_frame(){ curl -s "http://$H/proof/text-shortcuts-candidate-bubble-frame?$1" >/dev/null 2>&1 || true; }
proof_keyboard_shortcuts_roundtrip(){ curl -s "http://$H/proof/keyboard-shortcuts-roundtrip?$1" >/dev/null 2>&1 || true; }
proof_input_sources_roundtrip(){ curl -s "http://$H/proof/input-sources-roundtrip?$1" >/dev/null 2>&1 || true; }
proof_preview_open_render(){ curl -s "http://$H/proof/preview-open-render?$1" >/dev/null 2>&1 || true; }
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
firewall_live_toggle_proof(){
  local status_file=/tmp/gate-firewall-status.json
  local disable_file=/tmp/gate-firewall-disable.json
  local enable_file=/tmp/gate-firewall-enable.json
  local status_code disable_code enable_code before_available before_manageable
  local disable_ok disable_enabled disable_active enable_ok enable_enabled enable_active

  for _ in $(seq 1 60); do
    curl -sf "$LIVE_URL/health" >/dev/null 2>&1 && break
    sleep 0.5
  done

  status_code=$(curl -s -o "$status_file" -w '%{http_code}' "$LIVE_URL/v1/firewall/status" || true)
  before_available=$(json_field "$status_file" available)
  before_manageable=$(json_field "$status_file" manageable)
  if [ "$status_code" != "200" ] || [ "$before_available" != "true" ] || [ "$before_manageable" != "true" ]; then
    proof_firewall "status=fail&stage=status&status_http=${status_code:-000}&available=${before_available:-missing}&manageable=${before_manageable:-missing}"
    return 1
  fi

  disable_code=$(curl -s -o "$disable_file" -w '%{http_code}' \
    -H 'Content-Type: application/json' \
    -d '{"enabled":false}' \
    "$LIVE_URL/v1/firewall/enabled" || true)
  disable_ok=$(json_field "$disable_file" ok)
  disable_enabled=$(json_field "$disable_file" enabled)
  sleep 2
  curl -s -o "$status_file" "$LIVE_URL/v1/firewall/status" >/dev/null 2>&1 || true
  disable_active=$(json_field "$status_file" active)

  enable_code=$(curl -s -o "$enable_file" -w '%{http_code}' \
    -H 'Content-Type: application/json' \
    -d '{"enabled":true}' \
    "$LIVE_URL/v1/firewall/enabled" || true)
  enable_ok=$(json_field "$enable_file" ok)
  enable_enabled=$(json_field "$enable_file" enabled)
  sleep 2
  curl -s -o "$status_file" "$LIVE_URL/v1/firewall/status" >/dev/null 2>&1 || true
  enable_active=$(json_field "$status_file" active)

  if [ "$disable_code" = "200" ] && [ "$disable_ok" = "true" ] && [ "$disable_enabled" = "false" ] && [ "$disable_active" = "false" ] \
    && [ "$enable_code" = "200" ] && [ "$enable_ok" = "true" ] && [ "$enable_enabled" = "true" ] && [ "$enable_active" = "true" ]; then
    proof_firewall "status=pass&route=/v1/firewall/enabled&status_route=/v1/firewall/status&disable_http=200&disable_ok=true&disable_enabled=false&disable_active=false&enable_http=200&enable_ok=true&enable_enabled=true&enable_active=true&unit_template=goblins-os-firewall@.service&polkit_rule=60-goblins-os-firewall.rules"
    return 0
  fi

  proof_firewall "status=fail&stage=toggle&disable_http=${disable_code:-000}&disable_ok=${disable_ok:-missing}&disable_enabled=${disable_enabled:-missing}&disable_active=${disable_active:-missing}&enable_http=${enable_code:-000}&enable_ok=${enable_ok:-missing}&enable_enabled=${enable_enabled:-missing}&enable_active=${enable_active:-missing}"
  return 1
}
text_shortcuts_session_enable_proof(){
  local core_file=/tmp/gate-text-shortcuts-core.json
  local service_state input_sources preload_engines core_code core_engine_available core_runtime_loop
  local input_source_configured preload_configured engine_listed adapter_self_test active_engine engine_set

  for _ in $(seq 1 60); do
    service_state="$(systemctl --user is-active org.goblins.OS.IBus.service 2>/dev/null || true)"
    [ "$service_state" = "active" ] && break
    sleep 0.5
  done

  if [ "$service_state" != "active" ]; then
    proof_text_shortcuts "status=fail&stage=user-service&service=${service_state:-missing}&service_unit=org.goblins.OS.IBus.service"
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

  ibus read-cache >/dev/null 2>&1 || true
  if ibus list-engine 2>/dev/null | grep -Fq 'goblins-textshortcuts'; then
    engine_listed=true
  else
    engine_listed=false
  fi
  if [ "$engine_listed" != "true" ]; then
    proof_text_shortcuts "status=fail&stage=engine-list&service=active&input_source_configured=true&preload_configured=true&engine_listed=false"
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

  if ibus engine goblins-textshortcuts >/dev/null 2>&1; then
    engine_set=pass
  else
    engine_set=fail
  fi
  active_engine="$(ibus engine 2>/dev/null | tr -d '\n' || true)"
  if [ "$engine_set" != "pass" ] || [ "$active_engine" != "goblins-textshortcuts" ]; then
    proof_text_shortcuts "status=fail&stage=active-engine&service=active&engine_set=$engine_set&active_engine=${active_engine:-missing}"
    return 1
  fi

  core_code=$(curl -s -o "$core_file" -w '%{http_code}' "$LIVE_URL/v1/text-shortcuts" || true)
  core_engine_available=$(json_field "$core_file" engine_available)
  core_runtime_loop=$(json_field "$core_file" engine.runtime_loop_available)
  if [ "$core_code" != "200" ] || [ "$core_engine_available" != "false" ] || [ "$core_runtime_loop" != "false" ]; then
    proof_text_shortcuts "status=fail&stage=core-honesty&core_http=${core_code:-000}&core_engine_available=${core_engine_available:-missing}&core_runtime_loop_available=${core_runtime_loop:-missing}"
    return 1
  fi

  proof_text_shortcuts "status=pass&route=/v1/text-shortcuts&service=active&service_unit=org.goblins.OS.IBus.service&input_source_configured=true&preload_configured=true&engine_listed=true&adapter_self_test=pass&engine_set=pass&active_engine=goblins-textshortcuts&core_http=200&core_engine_available=false&core_runtime_loop_available=false&runtime_ready_claim=false"
  return 0
}
text_shortcuts_live_keystroke_proof(){
  local config_dir="${XDG_CONFIG_HOME:-$HOME/.config}/goblins-os"
  local table_file="$config_dir/text-shortcuts.json"
  local normal_file=/tmp/gate-text-shortcuts-normal.txt
  local passthrough_file=/tmp/gate-text-shortcuts-passthrough.txt
  local password_file=/tmp/gate-text-shortcuts-password.txt
  local dismiss_file=/tmp/gate-text-shortcuts-dismiss.txt
  local normal_actual passthrough_actual password_actual dismiss_actual normal_pid passthrough_pid password_pid dismiss_pid active_engine

  mkdir -p "$config_dir"
  printf '[{"replace":"omw","with":"onmyway"}]\n' > "$table_file"
  rm -f "$normal_file" "$passthrough_file" "$password_file" "$dismiss_file"

  pkill -f goblins-textshortcuts-ibus 2>/dev/null || true
  pkill -f goblins-textshortcuts-engine 2>/dev/null || true
  sleep 1
  if ! ibus engine goblins-textshortcuts >/dev/null 2>&1; then
    proof_text_shortcuts_live "status=fail&stage=engine-set&input_driver=wtype&active_engine=missing"
    return 1
  fi
  active_engine="$(ibus engine 2>/dev/null | tr -d '\n' || true)"
  if [ "$active_engine" != "goblins-textshortcuts" ]; then
    proof_text_shortcuts_live "status=fail&stage=engine-active&input_driver=wtype&active_engine=${active_engine:-missing}"
    return 1
  fi

  GOBLINS_OS_TEXT_SHORTCUTS_PROOF_FILE="$normal_file" "$B/goblins-os-shell" --text-shortcuts-proof normal >/tmp/gate-text-shortcuts-normal.log 2>&1 &
  normal_pid=$!
  sleep 4
  if ! wtype -- "omw." >/dev/null 2>&1; then
    kill "$normal_pid" 2>/dev/null || true
    proof_text_shortcuts_live "status=fail&stage=normal-wtype&input_driver=wtype&active_engine=goblins-textshortcuts"
    return 1
  fi
  for _ in $(seq 1 20); do
    normal_actual="$(cat "$normal_file" 2>/dev/null || true)"
    [ "$normal_actual" = "onmyway." ] && break
    sleep 0.5
  done
  kill "$normal_pid" 2>/dev/null || true
  wait "$normal_pid" 2>/dev/null || true
  if [ "$normal_actual" != "onmyway." ]; then
    proof_text_shortcuts_live "status=fail&stage=normal-readback&input_driver=wtype&active_engine=goblins-textshortcuts&normal_expected=onmyway.&normal_actual=${normal_actual:-missing}"
    return 1
  fi

  GOBLINS_OS_TEXT_SHORTCUTS_PROOF_FILE="$passthrough_file" "$B/goblins-os-shell" --text-shortcuts-proof passthrough >/tmp/gate-text-shortcuts-passthrough.log 2>&1 &
  passthrough_pid=$!
  sleep 4
  if ! wtype -- "hello." >/dev/null 2>&1; then
    kill "$passthrough_pid" 2>/dev/null || true
    proof_text_shortcuts_live "status=fail&stage=passthrough-wtype&input_driver=wtype&active_engine=goblins-textshortcuts&passthrough_input=hello."
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
    proof_text_shortcuts_live "status=fail&stage=passthrough-readback&input_driver=wtype&active_engine=goblins-textshortcuts&passthrough_expected=hello.&passthrough_actual=${passthrough_actual:-missing}&passthrough_unchanged=false"
    return 1
  fi

  GOBLINS_OS_TEXT_SHORTCUTS_PROOF_FILE="$dismiss_file" "$B/goblins-os-shell" --text-shortcuts-proof dismiss >/tmp/gate-text-shortcuts-dismiss.log 2>&1 &
  dismiss_pid=$!
  sleep 4
  if ! wtype -- "omw" >/dev/null 2>&1; then
    kill "$dismiss_pid" 2>/dev/null || true
    proof_text_shortcuts_live "status=fail&stage=dismiss-type&input_driver=wtype&active_engine=goblins-textshortcuts&dismiss_trigger=omw"
    return 1
  fi
  sleep 1
  if ! wtype -P Escape -p Escape >/dev/null 2>&1; then
    kill "$dismiss_pid" 2>/dev/null || true
    proof_text_shortcuts_live "status=fail&stage=dismiss-escape&input_driver=wtype&active_engine=goblins-textshortcuts&dismiss_trigger=omw&dismiss_key=Escape"
    return 1
  fi
  for _ in $(seq 1 20); do
    dismiss_actual="$(cat "$dismiss_file" 2>/dev/null || true)"
    [ "$dismiss_actual" = "omw" ] && break
    sleep 0.5
  done
  kill "$dismiss_pid" 2>/dev/null || true
  wait "$dismiss_pid" 2>/dev/null || true
  if [ "$dismiss_actual" != "omw" ]; then
    proof_text_shortcuts_live "status=fail&stage=dismiss-readback&input_driver=wtype&active_engine=goblins-textshortcuts&dismiss_expected=omw&dismiss_actual=${dismiss_actual:-missing}&dismiss_no_commit=false"
    return 1
  fi

  GOBLINS_OS_TEXT_SHORTCUTS_PROOF_FILE="$password_file" "$B/goblins-os-shell" --text-shortcuts-proof password >/tmp/gate-text-shortcuts-password.log 2>&1 &
  password_pid=$!
  sleep 4
  if ! wtype -- "omw." >/dev/null 2>&1; then
    kill "$password_pid" 2>/dev/null || true
    proof_text_shortcuts_live "status=fail&stage=password-wtype&input_driver=wtype&active_engine=goblins-textshortcuts"
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
    proof_text_shortcuts_live "status=fail&stage=password-readback&input_driver=wtype&active_engine=goblins-textshortcuts&password_expected=omw.&password_actual=${password_actual:-missing}"
    return 1
  fi

  proof_text_shortcuts_live "status=pass&route=/v1/text-shortcuts&surface=goblins-os-shell-text-shortcuts-proof&input_driver=wtype&active_engine=goblins-textshortcuts&normal_trigger=omw.&normal_expected=onmyway.&normal_actual=onmyway.&passthrough_input=hello.&passthrough_expected=hello.&passthrough_actual=hello.&passthrough_unchanged=true&dismiss_trigger=omw&dismiss_key=Escape&dismiss_expected=omw&dismiss_actual=omw&dismiss_no_commit=true&password_expected=omw.&password_actual=omw.&password_refusal=true&runtime_ready_claim=false"
  return 0
}
text_shortcuts_candidate_metadata_proof(){
  local candidate_file=/tmp/gate-text-shortcuts-candidate.txt
  local candidate_pid

  rm -f "$candidate_file"
  GOBLINS_OS_TEXT_SHORTCUTS_PROOF_FILE="$candidate_file" "$B/goblins-os-shell" --text-shortcuts-proof candidate >/tmp/gate-text-shortcuts-candidate.log 2>&1 &
  candidate_pid=$!
  sleep 4
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
keyboard_shortcuts_roundtrip_proof(){
  local shortcut_set_file=/tmp/gate-keyboard-shortcut-set.json
  local shortcut_reset_file=/tmp/gate-keyboard-shortcut-reset.json
  local modifier_set_file=/tmp/gate-keyboard-modifier-set.json
  local modifier_reset_file=/tmp/gate-keyboard-modifier-reset.json
  local shortcut_code shortcut_ok shortcut_after_set reset_code reset_ok shortcut_after_reset
  local modifier_code modifier_ok xkb_after_set modifier_reset_code modifier_reset_ok xkb_after_reset

  for _ in $(seq 1 60); do
    curl -sf "$LIVE_URL/health" >/dev/null 2>&1 && break
    sleep 0.5
  done

  shortcut_code=$(curl -s -o "$shortcut_set_file" -w '%{http_code}' \
    -H 'Content-Type: application/json' \
    -d '{"action":"window-hud","bindings":["<Super><Shift>H"]}' \
    "$LIVE_URL/v1/keyboard/shortcuts/binding" || true)
  shortcut_ok=$(json_field "$shortcut_set_file" ok)
  shortcut_after_set="$(gsettings get org.goblins.shell.extensions.wm window-hud 2>/dev/null || true)"
  if [ "$shortcut_code" != "200" ] || [ "$shortcut_ok" != "true" ] || ! printf '%s\n' "$shortcut_after_set" | grep -Fq "'<Super><Shift>H'"; then
    proof_keyboard_shortcuts_roundtrip "status=fail&stage=shortcut-set&route=/v1/keyboard/shortcuts/binding&shortcut_http=${shortcut_code:-000}&shortcut_ok=${shortcut_ok:-missing}&shortcut_action=window-hud&shortcut_binding=%3CSuper%3E%3CShift%3EH"
    return 1
  fi

  reset_code=$(curl -s -o "$shortcut_reset_file" -w '%{http_code}' \
    -H 'Content-Type: application/json' \
    -d '{"action":"window-hud","reset":true}' \
    "$LIVE_URL/v1/keyboard/shortcuts/binding" || true)
  reset_ok=$(json_field "$shortcut_reset_file" ok)
  shortcut_after_reset="$(gsettings get org.goblins.shell.extensions.wm window-hud 2>/dev/null || true)"
  if [ "$reset_code" != "200" ] || [ "$reset_ok" != "true" ] || ! printf '%s\n' "$shortcut_after_reset" | grep -Fq "'<Super>w'" || printf '%s\n' "$shortcut_after_reset" | grep -Fq "'<Super><Shift>H'"; then
    proof_keyboard_shortcuts_roundtrip "status=fail&stage=shortcut-reset&route=/v1/keyboard/shortcuts/binding&reset_http=${reset_code:-000}&reset_ok=${reset_ok:-missing}&shortcut_action=window-hud&default_binding=%3CSuper%3Ew"
    return 1
  fi

  modifier_code=$(curl -s -o "$modifier_set_file" -w '%{http_code}' \
    -H 'Content-Type: application/json' \
    -d '{"target":"caps-lock","value":"control"}' \
    "$LIVE_URL/v1/keyboard/modifier-remap" || true)
  modifier_ok=$(json_field "$modifier_set_file" ok)
  xkb_after_set="$(gsettings get org.gnome.desktop.input-sources xkb-options 2>/dev/null || true)"
  if [ "$modifier_code" != "200" ] || [ "$modifier_ok" != "true" ] || ! printf '%s\n' "$xkb_after_set" | grep -Fq "'ctrl:nocaps'"; then
    proof_keyboard_shortcuts_roundtrip "status=fail&stage=modifier-set&route=/v1/keyboard/modifier-remap&modifier_http=${modifier_code:-000}&modifier_ok=${modifier_ok:-missing}&modifier_target=caps-lock&modifier_value=control"
    return 1
  fi

  modifier_reset_code=$(curl -s -o "$modifier_reset_file" -w '%{http_code}' \
    -H 'Content-Type: application/json' \
    -d '{"target":"caps-lock","value":"default"}' \
    "$LIVE_URL/v1/keyboard/modifier-remap" || true)
  modifier_reset_ok=$(json_field "$modifier_reset_file" ok)
  xkb_after_reset="$(gsettings get org.gnome.desktop.input-sources xkb-options 2>/dev/null || true)"
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
    curl -sf "$LIVE_URL/health" >/dev/null 2>&1 && break
    sleep 0.5
  done

  original_sources="$(gsettings get org.gnome.desktop.input-sources sources 2>/dev/null || true)"
  original_current="$(gsettings get org.gnome.desktop.input-sources current 2>/dev/null || true)"
  original_current_value="$(printf '%s\n' "$original_current" | awk '{print $NF}' | tr -d "'")"
  if [ -z "$original_sources" ] || ! printf '%s\n' "$original_sources" | grep -Fq "(" || ! printf '%s\n' "$original_current_value" | grep -Eq '^[0-9]+$'; then
    proof_input_sources_roundtrip "status=fail&stage=baseline&source_route=/v1/input/sources&switch_route=/v1/input/switch-next&original_sources_reported=false"
    return 1
  fi

  set_code=$(curl -s -o "$set_file" -w '%{http_code}' \
    -H 'Content-Type: application/json' \
    -d '{"sources":[{"kind":"xkb","id":"us"},{"kind":"xkb","id":"gb"}]}' \
    "$LIVE_URL/v1/input/sources" || true)
  set_ok=$(json_field "$set_file" ok)
  sources_after_set="$(gsettings get org.gnome.desktop.input-sources sources 2>/dev/null || true)"
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

  switch_code=$(curl -s -o "$switch_file" -w '%{http_code}' -X POST \
    "$LIVE_URL/v1/input/switch-next" || true)
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
    curl -sf "$LIVE_URL/health" >/dev/null 2>&1 && break
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

  status_code=$(curl -s -o "$status_file" -w '%{http_code}' "$LIVE_URL/v1/preview/status" || true)
  available=$(json_field "$status_file" available)
  xdg_open=$(json_field "$status_file" xdg_open_available)
  papers=$(json_field "$status_file" papers_available)
  loupe=$(json_field "$status_file" loupe_available)
  if [ "$status_code" != "200" ] || [ "$available" != "true" ] || [ "$xdg_open" != "true" ] || [ "$papers" != "true" ] || [ "$loupe" != "true" ]; then
    proof_preview_open_render "status=fail&stage=status&status_route=/v1/preview/status&route=/v1/preview/open&status_http=${status_code:-000}&available=${available:-missing}&xdg_open=${xdg_open:-missing}&papers=${papers:-missing}&loupe=${loupe:-missing}&pdf_default=$pdf_default&image_default=$image_default"
    return 1
  fi

  pdf_code=$(curl -s -o "$pdf_file" -w '%{http_code}' \
    -H 'Content-Type: application/json' \
    -d "$(json_path_payload "$preview_pdf")" \
    "$LIVE_URL/v1/preview/open" || true)
  pdf_ok=$(json_field "$pdf_file" ok)
  pdf_kind=$(json_field "$pdf_file" kind)
  if [ "$pdf_code" != "200" ] || [ "$pdf_ok" != "true" ] || [ "$pdf_kind" != "pdf" ] || ! wait_process papers; then
    proof_preview_open_render "status=fail&stage=pdf-open&status_route=/v1/preview/status&route=/v1/preview/open&status_http=200&available=true&xdg_open=true&papers=true&loupe=true&pdf_default=$pdf_default&image_default=$image_default&pdf_http=${pdf_code:-000}&pdf_ok=${pdf_ok:-missing}&pdf_kind=${pdf_kind:-missing}"
    pkill -x papers 2>/dev/null || true
    return 1
  fi
  sleep 5
  sig 29-preview-pdf-open
  pkill -x papers 2>/dev/null || true

  image_code=$(curl -s -o "$image_file" -w '%{http_code}' \
    -H 'Content-Type: application/json' \
    -d "$(json_path_payload "$preview_png")" \
    "$LIVE_URL/v1/preview/open" || true)
  image_ok=$(json_field "$image_file" ok)
  image_kind=$(json_field "$image_file" kind)
  if [ "$image_code" != "200" ] || [ "$image_ok" != "true" ] || [ "$image_kind" != "image" ] || ! wait_process loupe; then
    proof_preview_open_render "status=fail&stage=image-open&status_route=/v1/preview/status&route=/v1/preview/open&status_http=200&available=true&xdg_open=true&papers=true&loupe=true&pdf_default=$pdf_default&image_default=$image_default&pdf_http=200&pdf_ok=true&pdf_kind=pdf&image_http=${image_code:-000}&image_ok=${image_ok:-missing}&image_kind=${image_kind:-missing}&pdf_screenshot=29-preview-pdf-open.png&rendered_pdf_frame=true"
    pkill -x loupe 2>/dev/null || true
    return 1
  fi
  sleep 5
  sig 30-preview-image-open
  pkill -x loupe 2>/dev/null || true

  unsupported_code=$(curl -s -o "$unsupported_file" -w '%{http_code}' \
    -H 'Content-Type: application/json' \
    -d "$(json_path_payload "$preview_txt")" \
    "$LIVE_URL/v1/preview/open" || true)
  unsupported_ok=$(json_field "$unsupported_file" ok)
  if [ "$unsupported_code" != "400" ] || [ "$unsupported_ok" = "true" ]; then
    proof_preview_open_render "status=fail&stage=unsupported&status_route=/v1/preview/status&route=/v1/preview/open&status_http=200&available=true&xdg_open=true&papers=true&loupe=true&pdf_default=$pdf_default&image_default=$image_default&pdf_http=200&pdf_ok=true&pdf_kind=pdf&image_http=200&image_ok=true&image_kind=image&unsupported_http=${unsupported_code:-000}&unsupported_ok=${unsupported_ok:-missing}&pdf_screenshot=29-preview-pdf-open.png&image_screenshot=30-preview-image-open.png&rendered_pdf_frame=true&rendered_image_frame=true"
    return 1
  fi

  proof_preview_open_render "status=pass&status_route=/v1/preview/status&route=/v1/preview/open&status_http=200&available=true&xdg_open=true&papers=true&loupe=true&pdf_default=org.gnome.Papers.desktop&image_default=org.gnome.Loupe.desktop&jpeg_default=org.gnome.Loupe.desktop&pdf_http=200&pdf_ok=true&pdf_kind=pdf&pdf_process=papers&pdf_screenshot=29-preview-pdf-open.png&rendered_pdf_frame=true&image_http=200&image_ok=true&image_kind=image&image_process=loupe&image_screenshot=30-preview-image-open.png&rendered_image_frame=true&unsupported_http=400&unsupported_ok=false&unsupported_rejected=true"
  return 0
}
# shot <name> <cmd...>  (env prefixes before `shot` propagate into the launch)
# After capture, fully wait for the binary to exit before returning — GtkApplication
# is single-instance, so relaunching the same binary (e.g. the installer with a new
# GOBLINS_OS_INSTALLER_PAGE, or the shell in dark) before the prior instance dies
# just re-focuses the old window, producing duplicate captures. Waiting for exit
# guarantees the next launch creates a fresh window with the new args/env/theme.
shot(){ local n="$1"; shift; dbus-run-session -- "$@" >/dev/null 2>&1 & local p=$!; sleep 7; sig "$n"; kill "$p" 2>/dev/null; pkill -f "$1" 2>/dev/null; for _ in $(seq 1 24); do pgrep -f "$1" >/dev/null 2>&1 || break; sleep 0.3; done; sleep 1; }
darkon(){ gsettings set org.gnome.desktop.interface color-scheme prefer-dark 2>/dev/null; sleep 1; }
darkoff(){ gsettings set org.gnome.desktop.interface color-scheme default 2>/dev/null; sleep 1; }

sleep 3
curl -s "http://$H/ready/ORCH_START" >/dev/null 2>&1
pkill -f goblins-os-login 2>/dev/null; pkill -f goblins-os-installer 2>/dev/null; sleep 2
firewall_live_toggle_proof || true
text_shortcuts_session_enable_proof || true
text_shortcuts_live_keystroke_proof || true
text_shortcuts_candidate_metadata_proof || true
text_shortcuts_overlay_intent_proof || true
text_shortcuts_candidate_bubble_frame_proof || true
keyboard_shortcuts_roundtrip_proof || true
input_sources_roundtrip_proof || true
preview_open_render_proof || true

# ---- seed a multi-OS fixture disk + start a fixture core on :8788 (dual-boot) ----
FIX=/tmp/fix; rm -rf $FIX; mkdir -p $FIX/nvme0n1/queue $FIX/nvme0n1/device
printf '536870912\n' > $FIX/nvme0n1/size; printf '0\n' > $FIX/nvme0n1/removable
printf '0\n' > $FIX/nvme0n1/queue/rotational; printf 'Goblins NVMe SSD\n' > $FIX/nvme0n1/device/model
seedpart(){ mkdir -p $FIX/nvme0n1/nvme0n1p$1; printf '%s\n' "$1" > $FIX/nvme0n1/nvme0n1p$1/partition; printf '%s\n' "$2" > $FIX/nvme0n1/nvme0n1p$1/uevent; }
seedpart 1 $'DEVNAME=nvme0n1p1\nDEVTYPE=partition\nPARTNAME=EFI System Partition\nPART_ENTRY_TYPE=c12a7328-f81f-11d2-ba4b-00a0c93ec93b'
seedpart 2 $'DEVNAME=nvme0n1p2\nDEVTYPE=partition\nTYPE=ntfs\nPARTLABEL=Windows'
seedpart 3 $'DEVNAME=nvme0n1p3\nDEVTYPE=partition\nTYPE=apfs\nPARTLABEL=Macintosh HD'
seedpart 4 $'DEVNAME=nvme0n1p4\nDEVTYPE=partition\nTYPE=crypto_LUKS\nPARTLABEL=Linux encrypted root'
GOBLINS_OS_CORE_PORT=8788 GOBLINS_OS_SYS_BLOCK_DIR=$FIX GOBLINS_OS_RAM_GB=32 \
  GOBLINS_OS_LOCAL_MODEL_RUNTIME=os-managed-runtime GOBLINS_OS_LOCAL_RUNTIME_URL=http://10.0.2.2:11434 \
  "$B/goblins-os-core" >/tmp/fixcore.log 2>&1 &
FIXCORE=$!
GOBLINS_OS_CORE_PORT=8788 GOBLINS_OS_LOCAL_RUNTIME_URL=http://10.0.2.2:11434 \
  "$B/goblins-os-resident" >/tmp/fixres.log 2>&1 &
sleep 5
FIX_URL=http://127.0.0.1:8788

# ---- login + onboarding ----
shot 03-login         "$B/goblins-os-login"
shot 06-onboarding    "$B/goblins-os-installer"
sig 04-desktop

# ---- session apps (light) ----
shot 07-home          "$B/goblins-os-shell"
shot 08-shell-home    "$B/goblins-os-shell"
shot 10-settings      "$B/goblins-os-settings"
shot 11-settings-models "$B/goblins-os-settings" --panel=models
shot 13-studio-before "$B/goblins-os-shell" --studio
shot 24-audio-output  "$B/goblins-os-settings" --panel=sound
shot 23-controller-detection "$B/goblins-os-settings" --panel=games

# ---- light/dark motion (shell mid-interaction is the closest honest motion frame) ----
shot 18-light-motion  "$B/goblins-os-shell"

# ---- dark variants ----
darkon
GOBLINS_OS_THEME=dark shot 09-shell-dark    "$B/goblins-os-shell"
GOBLINS_OS_THEME=dark shot 12-settings-dark "$B/goblins-os-settings"
GOBLINS_OS_THEME=dark shot 17-dark-motion   "$B/goblins-os-shell"
darkoff

# ---- installer pages (real core = this VM's blank scratch disk) ----
GOBLINS_OS_INSTALLER_PAGE=appearance     shot 01-installer "$B/goblins-os-installer"
GOBLINS_OS_INSTALLER_PAGE=network        shot 02-install-network "$B/goblins-os-installer"
GOBLINS_OS_INSTALLER_PAGE=install-disk   shot 25-install-destination "$B/goblins-os-installer"
GOBLINS_OS_INSTALLER_PAGE=install-review shot 26-install-storage-summary "$B/goblins-os-installer"
GOBLINS_OS_INSTALLER_PAGE=details        shot 28-bootloader-efi-summary "$B/goblins-os-installer"

# ---- dual-boot preservation (fixture core shows the multi-OS disk) ----
GOBLINS_OS_CORE_URL=$FIX_URL GOBLINS_OS_INSTALLER_PAGE=install-disk \
  shot 27-dual-boot-preserve-existing-os "$B/goblins-os-installer"

# ---- gaming stack (real software substrate) ----
shot 19-vulkan-vkcube  vkcube
shot 20-gamemode-active gamemoderun vkcube
shot 22-mangohud-overlay mangohud vkcube
shot 21-gamescope-session gamescope -W 960 -H 600 -b -- vkcube

# ---- studio-live (needs the host model; best-effort) ----
curl -s -X POST "$FIX_URL/v1/policy/permissions/grant" -H 'content-type: application/json' \
  -d '{"control_id":"app-builder","acknowledgement":"GRANT GOBLINS OS PERMISSION app-builder FOR consumer"}' >/dev/null 2>&1
curl -s -X POST "$FIX_URL/v1/apps/builds" -H 'content-type: application/json' \
  -d '{"intent":"A focus timer that counts down 25 minutes and rings."}' >/tmp/build.json 2>&1 &
GOBLINS_OS_CORE_URL=$FIX_URL shot 14-studio-running "$B/goblins-os-shell" --studio
sleep 20  # let the build finish
GOBLINS_OS_CORE_URL=$FIX_URL shot 15-studio-app-detail "$B/goblins-os-shell" --studio
GOBLINS_OS_CORE_URL=$FIX_URL shot 16-built-app-open "$B/goblins-os-shell" --studio

curl -s "http://$H/ready/ORCH_ALLDONE" >/dev/null 2>&1
sleep 2
