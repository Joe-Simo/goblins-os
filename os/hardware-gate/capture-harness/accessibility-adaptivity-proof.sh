#!/usr/bin/env bash
# Sourced by the verification-only desktop orchestrator. Every assertion comes
# from the installed core, desktop GSettings, Orca/AT-SPI, or a host-acknowledged
# QMP framebuffer. The original session preferences are restored on every path.

stop_accessibility_capture_windows(){
  if [ -n "$ACCESSIBILITY_SETTINGS_PID" ]; then
    kill "$ACCESSIBILITY_SETTINGS_PID" 2>/dev/null || true
    wait "$ACCESSIBILITY_SETTINGS_PID" 2>/dev/null || true
    ACCESSIBILITY_SETTINGS_PID=""
  fi
  if [ -n "$ACCESSIBILITY_LOCALE_PID" ]; then
    kill "$ACCESSIBILITY_LOCALE_PID" 2>/dev/null || true
    wait "$ACCESSIBILITY_LOCALE_PID" 2>/dev/null || true
    ACCESSIBILITY_LOCALE_PID=""
  fi
  pkill -x goblins-os-settings 2>/dev/null || true
  pkill -f '[g]nome-control-center' 2>/dev/null || true
}

restore_accessibility_proof_state(){
  stop_accessibility_capture_windows
  [ -z "$ORIGINAL_TEXT_SCALE" ] \
    || gsettings set org.gnome.desktop.interface text-scaling-factor "$ORIGINAL_TEXT_SCALE" >/dev/null 2>&1 || true
  [ -z "$ORIGINAL_HIGH_CONTRAST" ] \
    || gsettings set org.gnome.desktop.a11y.interface high-contrast "$ORIGINAL_HIGH_CONTRAST" >/dev/null 2>&1 || true
  [ -z "$ORIGINAL_REDUCE_TRANSPARENCY" ] \
    || gsettings set org.goblins.os.a11y.visual reduce-transparency "$ORIGINAL_REDUCE_TRANSPARENCY" >/dev/null 2>&1 || true
  [ -z "$ORIGINAL_ENABLE_ANIMATIONS" ] \
    || gsettings set org.gnome.desktop.interface enable-animations "$ORIGINAL_ENABLE_ANIMATIONS" >/dev/null 2>&1 || true
  [ -z "$ORIGINAL_SCREEN_READER" ] \
    || gsettings set org.gnome.desktop.a11y.applications screen-reader-enabled "$ORIGINAL_SCREEN_READER" >/dev/null 2>&1 || true
  if [ "$ACCESSIBILITY_ORCA_STARTED" = "true" ]; then
    [ -z "$ACCESSIBILITY_ORCA_PID" ] || kill "$ACCESSIBILITY_ORCA_PID" 2>/dev/null || true
    pkill -f '[o]rca' 2>/dev/null || true
    ACCESSIBILITY_ORCA_PID=""
    ACCESSIBILITY_ORCA_STARTED=false
  fi
  ACCESSIBILITY_PROOF_ACTIVE=false
}

accessibility_adaptivity_fail(){
  local stage="$1"
  shift
  restore_accessibility_proof_state
  proof_accessibility_adaptivity "status=fail&stage=$(proof_query_value "$stage")&architecture=aarch64&$*"
  return 1
}

start_accessibility_settings(){
  local maximized="$1"
  local panel="${2:-accessibility}"
  local language="${3:-}"
  local panel_title="Accessibility"
  local ledger=/tmp/gate-accessibility-present-ledger.json
  [ "$panel" != "language-region" ] || panel_title="Language & Region"
  stop_accessibility_capture_windows
  switch_control_off
  dismiss_shell_overview accessibility-adaptivity
  rm -f "$ledger" /tmp/gate-accessibility-settings.log
  export GOBLINS_OS_CAPTURE_PRESENT_LEDGER="$ledger"
  if [ "$maximized" = "true" ]; then
    env ${language:+LANG=de_DE.UTF-8} ${language:+LANGUAGE=de} \
      GOBLINS_OS_CAPTURE_NON_UNIQUE=1 \
      GOBLINS_OS_RENDER_FULLSCREEN=1 \
      GOBLINS_OS_CAPTURE_PRESENT_LEDGER="$ledger" \
      "$B/goblins-os-settings" "--panel=$panel" \
      >/tmp/gate-accessibility-settings.log 2>&1 &
  else
    env -u GOBLINS_OS_RENDER_FULLSCREEN \
      ${language:+LANG=de_DE.UTF-8} ${language:+LANGUAGE=de} \
      GOBLINS_OS_CAPTURE_NON_UNIQUE=1 \
      GOBLINS_OS_CAPTURE_PRESENT_LEDGER="$ledger" \
      "$B/goblins-os-settings" "--panel=$panel" \
      >/tmp/gate-accessibility-settings.log 2>&1 &
  fi
  ACCESSIBILITY_SETTINGS_PID=$!
  if ! wait_for_present_ledger "Goblins OS Settings - $panel_title" 60; then
    stop_accessibility_capture_windows
    return 1
  fi
  sleep 2
}

capture_accessibility_settings_shot(){
  local name="$1"
  local maximized="$2"
  start_accessibility_settings "$maximized" || return 1
  if ! sig "$name"; then
    stop_accessibility_capture_windows
    return 1
  fi
  stop_accessibility_capture_windows
}

atspi_accessibility_snapshot(){
  local output="$1"
  local mode="$2"
  run_bounded_quiet 15 python3 - "$output" "$mode" <<'PY'
import json
import sys

import pyatspi

output, mode = sys.argv[1:3]
desktop = pyatspi.Registry.getDesktop(0)


def walk(root):
    queue = [root]
    seen = set()
    nodes = []
    while queue and len(nodes) < 4096:
        node = queue.pop(0)
        identity = id(node)
        if identity in seen:
            continue
        seen.add(identity)
        try:
            name = str(node.name or "")
        except Exception:
            name = ""
        try:
            role = str(node.getRoleName() or "")
        except Exception:
            role = ""
        nodes.append((node, name, role))
        try:
            for index in range(min(int(node.childCount), 256)):
                queue.append(node.getChildAtIndex(index))
        except Exception:
            pass
    return nodes


nodes = walk(desktop)
focused = None
settings_frame = None
for node, name, role in nodes:
    if role in {"frame", "window"} and name.startswith("Goblins OS Settings - "):
        settings_frame = node

settings_nodes = walk(settings_frame) if settings_frame is not None else []
locale_facts = {}


def locale_fact_id(name):
    if name.startswith(("Requested interface language:", "Goblins OS interface language:")):
        return "interface-language"
    for fact_id, prefix in (
        ("regional-locale", "Regional format:"),
        ("date-time", "Date and time:"),
        ("number-format", "Number format:"),
        ("character-encoding", "Text encoding:"),
    ):
        if name.startswith(prefix):
            return fact_id
    return None


for node, name, role in settings_nodes:
    try:
        if node.getState().contains(pyatspi.STATE_FOCUSED):
            focused = {"name": name or "unnamed", "role": role or "unknown"}
    except Exception:
        pass
    fact_id = locale_fact_id(name)
    if fact_id is not None:
        locale_facts[fact_id] = name

zoom_action_invoked = False
if mode == "zoom":
    for node, name, _role in settings_nodes:
        if name != "Zoom":
            continue
        try:
            actions = node.queryAction()
            for index in range(actions.nActions):
                if actions.getName(index) in {"click", "press", "activate"}:
                    zoom_action_invoked = bool(actions.doAction(index))
                    break
            if not zoom_action_invoked and actions.nActions:
                zoom_action_invoked = bool(actions.doAction(0))
        except Exception:
            zoom_action_invoked = False
        break

width = height = x = y = 0
if settings_frame is not None:
    try:
        rect = settings_frame.queryComponent().getExtents(pyatspi.DESKTOP_COORDS)
        x, y, width, height = int(rect.x), int(rect.y), int(rect.width), int(rect.height)
    except Exception:
        pass

accessible_names = [name for _node, name, _role in settings_nodes if name]
visible_named_node_count = 0
clipped_visible_node_count = 0
if settings_frame is not None and width > 0 and height > 0:
    for node, name, _role in settings_nodes:
        if not name:
            continue
        try:
            state = node.getState()
            if not state.contains(pyatspi.STATE_SHOWING) or not state.contains(pyatspi.STATE_VISIBLE):
                continue
            rect = node.queryComponent().getExtents(pyatspi.DESKTOP_COORDS)
            if int(rect.width) <= 0 or int(rect.height) <= 0:
                continue
            visible_named_node_count += 1
            if (
                int(rect.x) < x
                or int(rect.y) < y
                or int(rect.x + rect.width) > x + width
                or int(rect.y + rect.height) > y + height
            ):
                clipped_visible_node_count += 1
        except Exception:
            pass

payload = {
    "atspi_node_count": len(settings_nodes),
    "accessible_names": accessible_names,
    "clipped_visible_node_count": clipped_visible_node_count,
    "focused": focused is not None,
    "focused_name": (focused or {}).get("name", ""),
    "focused_role": (focused or {}).get("role", ""),
    "goblins_copy_english_disclosed": "English" in locale_facts.get("interface-language", ""),
    "locale_fact_count": len(locale_facts),
    "locale_fact_names": locale_facts,
    "regional_content_present": all(
        fact_id in locale_facts
        for fact_id in (
            "interface-language",
            "regional-locale",
            "date-time",
            "number-format",
            "character-encoding",
        )
    ),
    "regional_format_accessible_name": locale_facts.get("regional-locale", ""),
    "settings_window_present": settings_frame is not None,
    "translated_goblins_copy_claimed": any(
        token in name
        for name in locale_facts.values()
        for token in ("Einstellungen", "Sprache und Region", "Regionale Formate")
    ),
    "visible_named_node_count": visible_named_node_count,
    "window_height": height,
    "window_width": width,
    "window_x": x,
    "window_y": y,
    "zoom_action_invoked": zoom_action_invoked,
}
with open(output, "w", encoding="utf-8") as handle:
    json.dump(payload, handle, sort_keys=True)
    handle.write("\n")
PY
}

language_region_runtime_readback(){
  local log_file="$1"
  local output_file="$2"
  run_bounded_quiet 10 python3 - "$log_file" "$output_file" <<'PY'
import json
import sys

log_path, output_path = sys.argv[1:3]
prefix = "settings_language_region_readback="
payload = None


def reject_duplicate_keys(pairs):
    result = {}
    for key, value in pairs:
        if key in result:
            raise ValueError(f"duplicate runtime readback key: {key}")
        result[key] = value
    return result


with open(log_path, encoding="utf-8", errors="replace") as handle:
    for raw_line in handle:
        line = raw_line.rstrip("\n")
        if line.startswith(prefix):
            if len(line) > 8192:
                raise SystemExit("oversized Settings language-region runtime readback")
            candidate = json.loads(
                line[len(prefix):], object_pairs_hook=reject_duplicate_keys
            )
            if isinstance(candidate, dict):
                payload = candidate
if payload is None:
    raise SystemExit("missing Settings language-region runtime readback")
with open(output_path, "w", encoding="utf-8") as handle:
    json.dump(payload, handle, sort_keys=True)
    handle.write("\n")
PY
}

accessibility_adaptivity_proof(){
  local status_file=/tmp/gate-accessibility-status.json
  local response_file=/tmp/gate-accessibility-response.json
  local atspi_file=/tmp/gate-accessibility-atspi.json
  local focus_file=/tmp/gate-accessibility-focus.json
  local resize_before_file=/tmp/gate-accessibility-resize-before.json
  local resize_action_file=/tmp/gate-accessibility-resize-action.json
  local resize_after_file=/tmp/gate-accessibility-resize-after.json
  local locale_baseline_file=/tmp/gate-accessibility-locale-baseline.json
  local locale_file=/tmp/gate-accessibility-locale.json
  local locale_baseline_runtime_file=/tmp/gate-accessibility-locale-baseline-runtime.json
  local locale_runtime_file=/tmp/gate-accessibility-locale-runtime.json
  local locale_compare_file=/tmp/gate-accessibility-locale-compare.json
  local status_code response_code response_ok response_target
  local gsettings_available interface_schema visual_schema reduce_transparency_schema
  local text_scale_readback high_contrast_readback transparency_readback motion_readback
  local screen_reader_readback atspi_bus_ready orca_process_active
  local atspi_window_present atspi_node_count
  local focused focused_name focused_role
  local regional_name regional_content locale_environment_readback
  local locale_active_messages locale_active_time locale_active_numeric locale_copy_english locale_fact_count locale_translation_claimed
  local locale_requested_valid locale_language_valid
  local locale_changed_count locale_longer_count locale_visible_count locale_clipped_count locale_window_present
  local locale_date_changed locale_number_changed
  local resized_before_width resized_before_height resized_after_width resized_after_height resize_action
  local text_scale_sha high_contrast_sha transparency_sha motion_sha locale_sha orca_sha focus_sha resize_sha
  local framebuffer_width framebuffer_height uniform_framebuffer=true

  ORIGINAL_TEXT_SCALE="$(gsettings get org.gnome.desktop.interface text-scaling-factor 2>/dev/null || true)"
  ORIGINAL_HIGH_CONTRAST="$(gsettings get org.gnome.desktop.a11y.interface high-contrast 2>/dev/null || true)"
  ORIGINAL_REDUCE_TRANSPARENCY="$(gsettings get org.goblins.os.a11y.visual reduce-transparency 2>/dev/null || true)"
  ORIGINAL_ENABLE_ANIMATIONS="$(gsettings get org.gnome.desktop.interface enable-animations 2>/dev/null || true)"
  ORIGINAL_SCREEN_READER="$(gsettings get org.gnome.desktop.a11y.applications screen-reader-enabled 2>/dev/null || true)"
  if [ -z "$ORIGINAL_TEXT_SCALE" ] \
    || [ -z "$ORIGINAL_HIGH_CONTRAST" ] \
    || [ -z "$ORIGINAL_REDUCE_TRANSPARENCY" ] \
    || [ -z "$ORIGINAL_ENABLE_ANIMATIONS" ] \
    || [ -z "$ORIGINAL_SCREEN_READER" ]; then
    accessibility_adaptivity_fail gsettings-prerequisites "schemas_available=false"
    return 1
  fi
  ACCESSIBILITY_PROOF_ACTIVE=true

  status_code="$(core_proof_request accessibility-status "$status_file" || true)"
  gsettings_available="$(json_field "$status_file" gsettings_available)"
  interface_schema="$(json_field "$status_file" interface.schema_available)"
  visual_schema="$(json_field "$status_file" visual.schema_available)"
  reduce_transparency_schema="$(json_field "$status_file" visual.reduce_transparency_schema_available)"
  if [ "$status_code" != "200" ] \
    || [ "$gsettings_available" != "true" ] \
    || [ "$interface_schema" != "true" ] \
    || [ "$visual_schema" != "true" ] \
    || [ "$reduce_transparency_schema" != "true" ]; then
    accessibility_adaptivity_fail core-status "status_http=${status_code:-000}&gsettings_available=${gsettings_available:-missing}&interface_schema_available=${interface_schema:-missing}&visual_schema_available=${visual_schema:-missing}&reduce_transparency_schema_available=${reduce_transparency_schema:-missing}"
    return 1
  fi

  response_code="$(core_proof_request accessibility-text-scale "$response_file" || true)"
  response_ok="$(json_field "$response_file" ok)"
  response_target="$(json_field "$response_file" target)"
  status_code="$(core_proof_request accessibility-status "$status_file" || true)"
  text_scale_readback="$(json_field "$status_file" interface.text_scale)"
  if [ "$response_code" != "200" ] || [ "$response_ok" != "true" ] \
    || [ "$response_target" != "text-scale" ] || [ "$status_code" != "200" ] \
    || [ "$text_scale_readback" != "1.25" ] \
    || [ "$(gsettings get org.gnome.desktop.interface text-scaling-factor 2>/dev/null)" != "1.25" ]; then
    accessibility_adaptivity_fail text-scale "preference_http=${response_code:-000}&status_http=${status_code:-000}&readback=${text_scale_readback:-missing}"
    return 1
  fi
  if ! capture_accessibility_settings_shot 33-accessibility-text-scaling true; then
    accessibility_adaptivity_fail text-scale-capture "screenshot=33-accessibility-text-scaling.png"
    return 1
  fi
  text_scale_sha="$CAPTURE_ACK_PNG_SHA256"
  framebuffer_width="$CAPTURE_ACK_PNG_WIDTH"
  framebuffer_height="$CAPTURE_ACK_PNG_HEIGHT"

  response_code="$(core_proof_request accessibility-high-contrast "$response_file" || true)"
  response_ok="$(json_field "$response_file" ok)"
  response_target="$(json_field "$response_file" target)"
  status_code="$(core_proof_request accessibility-status "$status_file" || true)"
  high_contrast_readback="$(json_field "$status_file" visual.high_contrast)"
  if [ "$response_code" != "200" ] || [ "$response_ok" != "true" ] \
    || [ "$response_target" != "high-contrast" ] || [ "$status_code" != "200" ] \
    || [ "$high_contrast_readback" != "true" ] \
    || [ "$(gsettings get org.gnome.desktop.a11y.interface high-contrast 2>/dev/null)" != "true" ]; then
    accessibility_adaptivity_fail high-contrast "preference_http=${response_code:-000}&status_http=${status_code:-000}&readback=${high_contrast_readback:-missing}"
    return 1
  fi
  if ! capture_accessibility_settings_shot 34-accessibility-high-contrast true; then
    accessibility_adaptivity_fail high-contrast-capture "screenshot=34-accessibility-high-contrast.png"
    return 1
  fi
  high_contrast_sha="$CAPTURE_ACK_PNG_SHA256"
  [ "$CAPTURE_ACK_PNG_WIDTH" = "$framebuffer_width" ] \
    && [ "$CAPTURE_ACK_PNG_HEIGHT" = "$framebuffer_height" ] || uniform_framebuffer=false

  response_code="$(core_proof_request accessibility-reduce-transparency "$response_file" || true)"
  response_ok="$(json_field "$response_file" ok)"
  response_target="$(json_field "$response_file" target)"
  status_code="$(core_proof_request accessibility-status "$status_file" || true)"
  transparency_readback="$(json_field "$status_file" visual.reduce_transparency)"
  if [ "$response_code" != "200" ] || [ "$response_ok" != "true" ] \
    || [ "$response_target" != "reduce-transparency" ] || [ "$status_code" != "200" ] \
    || [ "$transparency_readback" != "true" ] \
    || [ "$(gsettings get org.goblins.os.a11y.visual reduce-transparency 2>/dev/null)" != "true" ]; then
    accessibility_adaptivity_fail reduced-transparency "preference_http=${response_code:-000}&status_http=${status_code:-000}&readback=${transparency_readback:-missing}"
    return 1
  fi
  if ! capture_accessibility_settings_shot 35-accessibility-reduced-transparency true; then
    accessibility_adaptivity_fail reduced-transparency-capture "screenshot=35-accessibility-reduced-transparency.png"
    return 1
  fi
  transparency_sha="$CAPTURE_ACK_PNG_SHA256"
  [ "$CAPTURE_ACK_PNG_WIDTH" = "$framebuffer_width" ] \
    && [ "$CAPTURE_ACK_PNG_HEIGHT" = "$framebuffer_height" ] || uniform_framebuffer=false

  response_code="$(core_proof_request accessibility-reduce-motion "$response_file" || true)"
  response_ok="$(json_field "$response_file" ok)"
  response_target="$(json_field "$response_file" target)"
  status_code="$(core_proof_request accessibility-status "$status_file" || true)"
  motion_readback="$(json_field "$status_file" interface.reduce_motion)"
  if [ "$response_code" != "200" ] || [ "$response_ok" != "true" ] \
    || [ "$response_target" != "reduce-motion" ] || [ "$status_code" != "200" ] \
    || [ "$motion_readback" != "true" ] \
    || [ "$(gsettings get org.gnome.desktop.interface enable-animations 2>/dev/null)" != "false" ]; then
    accessibility_adaptivity_fail reduced-motion "preference_http=${response_code:-000}&status_http=${status_code:-000}&readback=${motion_readback:-missing}"
    return 1
  fi
  if ! capture_accessibility_settings_shot 36-accessibility-reduced-motion true; then
    accessibility_adaptivity_fail reduced-motion-capture "screenshot=36-accessibility-reduced-motion.png"
    return 1
  fi
  motion_sha="$CAPTURE_ACK_PNG_SHA256"
  [ "$CAPTURE_ACK_PNG_WIDTH" = "$framebuffer_width" ] \
    && [ "$CAPTURE_ACK_PNG_HEIGHT" = "$framebuffer_height" ] || uniform_framebuffer=false

  if ! start_accessibility_settings true language-region "" \
    || ! atspi_accessibility_snapshot "$locale_baseline_file" snapshot \
    || ! language_region_runtime_readback /tmp/gate-accessibility-settings.log "$locale_baseline_runtime_file"; then
    accessibility_adaptivity_fail locale-expansion-baseline "surface=goblins-os-settings-language-region"
    return 1
  fi
  stop_accessibility_capture_windows
  if ! start_accessibility_settings true language-region de \
    || ! atspi_accessibility_snapshot "$locale_file" snapshot \
    || ! language_region_runtime_readback /tmp/gate-accessibility-settings.log "$locale_runtime_file"; then
    accessibility_adaptivity_fail locale-expansion-candidate "surface=goblins-os-settings-language-region&locale=de_DE.UTF-8"
    return 1
  fi
  locale_environment_readback=false
  if tr '\0' '\n' <"/proc/$ACCESSIBILITY_SETTINGS_PID/environ" 2>/dev/null \
      | grep -Fxq 'LANGUAGE=de' \
    && tr '\0' '\n' <"/proc/$ACCESSIBILITY_SETTINGS_PID/environ" 2>/dev/null \
      | grep -Fxq 'LANG=de_DE.UTF-8'; then
    locale_environment_readback=true
  fi
  if ! run_bounded_quiet 10 python3 - \
    "$locale_baseline_file" "$locale_file" \
    "$locale_baseline_runtime_file" "$locale_runtime_file" \
    "$locale_compare_file" <<'PY'
import json
import sys

baseline_path, localized_path, baseline_runtime_path, localized_runtime_path, output_path = sys.argv[1:6]
with open(baseline_path, encoding="utf-8") as handle:
    baseline = json.load(handle)
with open(localized_path, encoding="utf-8") as handle:
    localized = json.load(handle)
with open(baseline_runtime_path, encoding="utf-8") as handle:
    baseline_runtime = json.load(handle)
with open(localized_runtime_path, encoding="utf-8") as handle:
    localized_runtime = json.load(handle)
required = {
    "interface-language",
    "regional-locale",
    "date-time",
    "number-format",
    "character-encoding",
}
baseline_facts = baseline.get("locale_fact_names", {})
localized_facts = localized.get("locale_fact_names", {})
changed = [
    fact_id
    for fact_id in sorted(required)
    if baseline_facts.get(fact_id) != localized_facts.get(fact_id)
]
longer = [
    fact_id
    for fact_id in changed
    if len(localized_facts.get(fact_id, "")) > len(baseline_facts.get(fact_id, ""))
]


def normalized_locale(value):
    return str(value or "").replace(".utf8", ".UTF-8")


active_messages_locale = normalized_locale(localized_runtime.get("active_messages_locale"))
active_time_locale = normalized_locale(localized_runtime.get("active_time_locale"))
active_numeric_locale = normalized_locale(localized_runtime.get("active_numeric_locale"))
with open(output_path, "w", encoding="utf-8") as handle:
    json.dump(
        {
            "active_messages_locale_valid": active_messages_locale == "de_DE.UTF-8",
            "active_numeric_locale_valid": active_numeric_locale == "de_DE.UTF-8",
            "active_time_locale_valid": active_time_locale == "de_DE.UTF-8",
            "changed_name_count": len(changed),
            "date_format_changed": (
                "date-time" in changed
                and baseline_runtime.get("date_pattern") != localized_runtime.get("date_pattern")
                and baseline_runtime.get("date_now") != localized_runtime.get("date_now")
            ),
            "language_request_valid": localized_runtime.get("interface_language") == "de",
            "longer_name_count": len(longer),
            "number_format_changed": (
                "number-format" in changed
                and baseline_runtime.get("decimal_separator") != localized_runtime.get("decimal_separator")
                and baseline_runtime.get("grouping_separator") != localized_runtime.get("grouping_separator")
            ),
            "requested_locale_valid": localized_runtime.get("requested_locale") == "de_DE.UTF-8",
        },
        handle,
        sort_keys=True,
    )
    handle.write("\n")
PY
  then
    accessibility_adaptivity_fail locale-expansion-compare "surface=goblins-os-settings-language-region"
    return 1
  fi
  regional_content="$(json_field "$locale_file" regional_content_present)"
  regional_name="$(json_field "$locale_file" regional_format_accessible_name)"
  locale_active_messages="$(json_field "$locale_compare_file" active_messages_locale_valid)"
  locale_active_time="$(json_field "$locale_compare_file" active_time_locale_valid)"
  locale_active_numeric="$(json_field "$locale_compare_file" active_numeric_locale_valid)"
  locale_copy_english="$(json_field "$locale_file" goblins_copy_english_disclosed)"
  locale_translation_claimed="$(json_field "$locale_file" translated_goblins_copy_claimed)"
  locale_fact_count="$(json_field "$locale_file" locale_fact_count)"
  locale_requested_valid="$(json_field "$locale_compare_file" requested_locale_valid)"
  locale_language_valid="$(json_field "$locale_compare_file" language_request_valid)"
  locale_changed_count="$(json_field "$locale_compare_file" changed_name_count)"
  locale_longer_count="$(json_field "$locale_compare_file" longer_name_count)"
  locale_date_changed="$(json_field "$locale_compare_file" date_format_changed)"
  locale_number_changed="$(json_field "$locale_compare_file" number_format_changed)"
  locale_visible_count="$(json_field "$locale_file" visible_named_node_count)"
  locale_clipped_count="$(json_field "$locale_file" clipped_visible_node_count)"
  locale_window_present="$(json_field "$locale_file" settings_window_present)"
  if [ "$locale_environment_readback" != "true" ] \
    || [ "$regional_content" != "true" ] \
    || [ -z "$regional_name" ] \
    || [ "$regional_name" != "Regional format: German (Germany)" ] \
    || [ "$locale_active_messages" != "true" ] \
    || [ "$locale_active_time" != "true" ] \
    || [ "$locale_active_numeric" != "true" ] \
    || [ "$locale_copy_english" != "true" ] \
    || [ "$locale_translation_claimed" != "false" ] \
    || [ "$locale_requested_valid" != "true" ] \
    || [ "$locale_language_valid" != "true" ] \
    || [ "$locale_fact_count" != "5" ] \
    || [ "$locale_window_present" != "true" ] \
    || ! [[ "$locale_changed_count" =~ ^[1-9][0-9]*$ ]] \
    || ! [[ "$locale_longer_count" =~ ^[1-9][0-9]*$ ]] \
    || [ "$locale_date_changed" != "true" ] \
    || [ "$locale_number_changed" != "true" ] \
    || ! [[ "$locale_visible_count" =~ ^[1-9][0-9]*$ ]] \
    || [ "$locale_clipped_count" != "0" ]; then
    accessibility_adaptivity_fail goblins-locale-expansion-not-ready "surface=goblins-os-settings-language-region&locale=de_DE.UTF-8&process_environment=$locale_environment_readback&runtime_locale=${locale_requested_valid:-missing}&runtime_language=${locale_language_valid:-missing}&regional_content=${regional_content:-missing}&active_messages_locale=${locale_active_messages:-missing}&active_time_locale=${locale_active_time:-missing}&active_numeric_locale=${locale_active_numeric:-missing}&translation_claimed=${locale_translation_claimed:-missing}&goblins_copy_english=${locale_copy_english:-missing}&facts=${locale_fact_count:-missing}&changed_names=${locale_changed_count:-missing}&longer_names=${locale_longer_count:-missing}&date_changed=${locale_date_changed:-missing}&number_changed=${locale_number_changed:-missing}&visible_nodes=${locale_visible_count:-missing}&clipped_nodes=${locale_clipped_count:-missing}"
    return 1
  fi
  if ! sig 37-accessibility-localization-expansion; then
    accessibility_adaptivity_fail locale-expansion-capture "screenshot=37-accessibility-localization-expansion.png"
    return 1
  fi
  locale_sha="$CAPTURE_ACK_PNG_SHA256"
  [ "$CAPTURE_ACK_PNG_WIDTH" = "$framebuffer_width" ] \
    && [ "$CAPTURE_ACK_PNG_HEIGHT" = "$framebuffer_height" ] || uniform_framebuffer=false
  stop_accessibility_capture_windows

  response_code="$(core_proof_request accessibility-screen-reader "$response_file" || true)"
  response_ok="$(json_field "$response_file" ok)"
  response_target="$(json_field "$response_file" target)"
  status_code="$(core_proof_request accessibility-status "$status_file" || true)"
  screen_reader_readback="$(json_field "$status_file" assistive.screen_reader)"
  if [ "$response_code" != "200" ] || [ "$response_ok" != "true" ] \
    || [ "$response_target" != "screen-reader" ] || [ "$status_code" != "200" ] \
    || [ "$screen_reader_readback" != "true" ] \
    || [ "$(gsettings get org.gnome.desktop.a11y.applications screen-reader-enabled 2>/dev/null)" != "true" ]; then
    accessibility_adaptivity_fail screen-reader "preference_http=${response_code:-000}&status_http=${status_code:-000}&readback=${screen_reader_readback:-missing}"
    return 1
  fi
  for _ in $(seq 1 20); do
    ACCESSIBILITY_ORCA_PID="$(pgrep -n -f '[o]rca' 2>/dev/null || true)"
    [ -n "$ACCESSIBILITY_ORCA_PID" ] && break
    sleep 0.5
  done
  if [ -z "$ACCESSIBILITY_ORCA_PID" ]; then
    orca --replace >/tmp/gate-accessibility-orca.log 2>&1 &
    ACCESSIBILITY_ORCA_PID=$!
  fi
  if [ "$ORIGINAL_SCREEN_READER" != "true" ]; then
    ACCESSIBILITY_ORCA_STARTED=true
  fi
  sleep 3
  orca_process_active=false
  kill -0 "$ACCESSIBILITY_ORCA_PID" 2>/dev/null && orca_process_active=true
  atspi_bus_ready=false
  if gdbus call --session \
      --dest org.freedesktop.DBus \
      --object-path /org/freedesktop/DBus \
      --method org.freedesktop.DBus.NameHasOwner org.a11y.Bus 2>/dev/null \
      | grep -Fq true; then
    atspi_bus_ready=true
  fi
  if ! start_accessibility_settings true \
    || ! atspi_accessibility_snapshot "$atspi_file" snapshot; then
    accessibility_adaptivity_fail orca-atspi-window "orca_process_active=$orca_process_active&atspi_bus_ready=$atspi_bus_ready"
    return 1
  fi
  atspi_window_present="$(json_field "$atspi_file" settings_window_present)"
  atspi_node_count="$(json_field "$atspi_file" atspi_node_count)"
  if [ "$orca_process_active" != "true" ] \
    || [ "$atspi_bus_ready" != "true" ] \
    || [ "$atspi_window_present" != "true" ] \
    || ! [[ "$atspi_node_count" =~ ^[1-9][0-9]*$ ]]; then
    accessibility_adaptivity_fail orca-atspi-readback "orca_process_active=$orca_process_active&atspi_bus_ready=$atspi_bus_ready&settings_window_present=${atspi_window_present:-missing}&atspi_node_count=${atspi_node_count:-missing}"
    return 1
  fi
  if ! sig 38-accessibility-orca-atspi; then
    accessibility_adaptivity_fail orca-atspi-capture "screenshot=38-accessibility-orca-atspi.png"
    return 1
  fi
  orca_sha="$CAPTURE_ACK_PNG_SHA256"
  [ "$CAPTURE_ACK_PNG_WIDTH" = "$framebuffer_width" ] \
    && [ "$CAPTURE_ACK_PNG_HEIGHT" = "$framebuffer_height" ] || uniform_framebuffer=false
  stop_accessibility_capture_windows

  if ! start_accessibility_settings true; then
    accessibility_adaptivity_fail keyboard-focus-window "screenshot=39-accessibility-keyboard-focus.png"
    return 1
  fi
  host_click accessibility-focus-window 0.5 0.5
  focused=false
  for index in $(seq 1 8); do
    host_press_key "accessibility-focus-tab-$index" Tab
    if atspi_accessibility_snapshot "$focus_file" snapshot \
      && [ "$(json_field "$focus_file" focused)" = "true" ]; then
      focused=true
      break
    fi
  done
  focused_name="$(json_field "$focus_file" focused_name)"
  focused_role="$(json_field "$focus_file" focused_role)"
  if [ "$focused" != "true" ] || [ -z "$focused_name" ] || [ -z "$focused_role" ]; then
    accessibility_adaptivity_fail keyboard-focus-readback "focused=$focused&focused_name=$(proof_query_value "${focused_name:-missing}")&focused_role=$(proof_query_value "${focused_role:-missing}")"
    return 1
  fi
  if ! sig 39-accessibility-keyboard-focus; then
    accessibility_adaptivity_fail keyboard-focus-capture "screenshot=39-accessibility-keyboard-focus.png"
    return 1
  fi
  focus_sha="$CAPTURE_ACK_PNG_SHA256"
  [ "$CAPTURE_ACK_PNG_WIDTH" = "$framebuffer_width" ] \
    && [ "$CAPTURE_ACK_PNG_HEIGHT" = "$framebuffer_height" ] || uniform_framebuffer=false
  stop_accessibility_capture_windows

  if ! start_accessibility_settings true \
    || ! atspi_accessibility_snapshot "$resize_before_file" snapshot \
    || ! atspi_accessibility_snapshot "$resize_action_file" zoom; then
    accessibility_adaptivity_fail window-resize-action "screenshot=40-accessibility-window-resize.png"
    return 1
  fi
  sleep 3
  if ! atspi_accessibility_snapshot "$resize_after_file" snapshot; then
    accessibility_adaptivity_fail window-resize-readback "screenshot=40-accessibility-window-resize.png"
    return 1
  fi
  resize_action="$(json_field "$resize_action_file" zoom_action_invoked)"
  resized_before_width="$(json_field "$resize_before_file" window_width)"
  resized_before_height="$(json_field "$resize_before_file" window_height)"
  resized_after_width="$(json_field "$resize_after_file" window_width)"
  resized_after_height="$(json_field "$resize_after_file" window_height)"
  if [ "$resize_action" != "true" ] \
    || ! [[ "$resized_before_width" =~ ^[1-9][0-9]*$ ]] \
    || ! [[ "$resized_before_height" =~ ^[1-9][0-9]*$ ]] \
    || ! [[ "$resized_after_width" =~ ^[1-9][0-9]*$ ]] \
    || ! [[ "$resized_after_height" =~ ^[1-9][0-9]*$ ]] \
    || [ "$resized_before_width" -le "$resized_after_width" ] \
    || [ "$resized_before_height" -le "$resized_after_height" ]; then
    accessibility_adaptivity_fail window-resize-observation "resize_action=${resize_action:-missing}&before_width=${resized_before_width:-missing}&before_height=${resized_before_height:-missing}&after_width=${resized_after_width:-missing}&after_height=${resized_after_height:-missing}"
    return 1
  fi
  if ! sig 40-accessibility-window-resize; then
    accessibility_adaptivity_fail window-resize-capture "screenshot=40-accessibility-window-resize.png"
    return 1
  fi
  resize_sha="$CAPTURE_ACK_PNG_SHA256"
  [ "$CAPTURE_ACK_PNG_WIDTH" = "$framebuffer_width" ] \
    && [ "$CAPTURE_ACK_PNG_HEIGHT" = "$framebuffer_height" ] || uniform_framebuffer=false
  stop_accessibility_capture_windows

  restore_accessibility_proof_state
  if [ "$(gsettings get org.gnome.desktop.interface text-scaling-factor 2>/dev/null)" != "$ORIGINAL_TEXT_SCALE" ] \
    || [ "$(gsettings get org.gnome.desktop.a11y.interface high-contrast 2>/dev/null)" != "$ORIGINAL_HIGH_CONTRAST" ] \
    || [ "$(gsettings get org.goblins.os.a11y.visual reduce-transparency 2>/dev/null)" != "$ORIGINAL_REDUCE_TRANSPARENCY" ] \
    || [ "$(gsettings get org.gnome.desktop.interface enable-animations 2>/dev/null)" != "$ORIGINAL_ENABLE_ANIMATIONS" ] \
    || [ "$(gsettings get org.gnome.desktop.a11y.applications screen-reader-enabled 2>/dev/null)" != "$ORIGINAL_SCREEN_READER" ]; then
    proof_accessibility_adaptivity "status=fail&stage=restore&architecture=aarch64&roundtrip_restored=false"
    return 1
  fi
  if [ "$uniform_framebuffer" != "true" ]; then
    proof_accessibility_adaptivity "status=fail&stage=framebuffer-uniformity&architecture=aarch64&uniform_framebuffer=false"
    return 1
  fi

  proof_accessibility_adaptivity "status=pass&architecture=aarch64&status_route=/v1/accessibility/status&preference_route=/v1/accessibility/preference&gsettings_available=true&interface_schema_available=true&visual_schema_available=true&reduce_transparency_schema_available=true&text_scale_schema=org.gnome.desktop.interface&text_scale_key=text-scaling-factor&text_scale_target=1.25&text_scale_http=200&text_scale_ok=true&text_scale_readback=1.25&text_scale_screenshot=33-accessibility-text-scaling.png&text_scale_screenshot_sha256=$text_scale_sha&high_contrast_schema=org.gnome.desktop.a11y.interface&high_contrast_key=high-contrast&high_contrast_http=200&high_contrast_ok=true&high_contrast_readback=true&high_contrast_screenshot=34-accessibility-high-contrast.png&high_contrast_screenshot_sha256=$high_contrast_sha&reduce_transparency_schema=org.goblins.os.a11y.visual&reduce_transparency_key=reduce-transparency&reduce_transparency_http=200&reduce_transparency_ok=true&reduce_transparency_readback=true&reduce_transparency_screenshot=35-accessibility-reduced-transparency.png&reduce_transparency_screenshot_sha256=$transparency_sha&reduce_motion_schema=org.gnome.desktop.interface&reduce_motion_key=enable-animations&reduce_motion_http=200&reduce_motion_ok=true&reduce_motion_readback=true&reduce_motion_screenshot=36-accessibility-reduced-motion.png&reduce_motion_screenshot_sha256=$motion_sha&locale_expansion_surface=goblins-os-settings-language-region&locale_expansion_requested_locale=de_DE.UTF-8&locale_expansion_language_request=de&locale_expansion_runtime=Goblins-Settings-AT-SPI&locale_expansion_process_environment=true&locale_expansion_active_time_locale=true&locale_expansion_active_numeric_locale=true&locale_expansion_translation_claimed=false&locale_expansion_goblins_copy_language=English&locale_expansion_regional_content_present=true&locale_expansion_fact_count=$locale_fact_count&locale_expansion_accessible_name=$(proof_query_value "$regional_name")&locale_expansion_changed_name_count=$locale_changed_count&locale_expansion_longer_name_count=$locale_longer_count&locale_expansion_date_format_changed=true&locale_expansion_number_format_changed=true&locale_expansion_visible_named_node_count=$locale_visible_count&locale_expansion_clipped_visible_node_count=0&locale_expansion_layout_bounds_checked=true&locale_expansion_screenshot=37-accessibility-localization-expansion.png&locale_expansion_screenshot_sha256=$locale_sha&screen_reader_schema=org.gnome.desktop.a11y.applications&screen_reader_key=screen-reader-enabled&screen_reader_http=200&screen_reader_ok=true&screen_reader_readback=true&orca_process_active=true&atspi_bus=org.a11y.Bus&atspi_bus_ready=true&atspi_settings_window_present=true&atspi_node_count=$atspi_node_count&orca_atspi_screenshot=38-accessibility-orca-atspi.png&orca_atspi_screenshot_sha256=$orca_sha&keyboard_input_driver=qmp-keyboard&keyboard_focus_key=Tab&keyboard_focused=true&keyboard_focus_role=$(proof_query_value "$focused_role")&keyboard_focus_name=$(proof_query_value "$focused_name")&keyboard_focus_screenshot=39-accessibility-keyboard-focus.png&keyboard_focus_screenshot_sha256=$focus_sha&resize_action=AT-SPI-Zoom&resize_action_invoked=true&window_maximized_width=$resized_before_width&window_maximized_height=$resized_before_height&window_resized_width=$resized_after_width&window_resized_height=$resized_after_height&window_resize_observed=true&framebuffer_width=$framebuffer_width&framebuffer_height=$framebuffer_height&window_resize_screenshot=40-accessibility-window-resize.png&window_resize_screenshot_sha256=$resize_sha&uniform_framebuffer=true&screenshot_capture_ack=true&roundtrip_restored=true"
}
