#!/usr/bin/env bash
set -euo pipefail

ROOT="${ROOT:-$(cd "$(dirname "$0")/../.." && pwd)}"
cd "$ROOT"
. "$ROOT/os/hardware-gate/secret-scan.sh"
. "$ROOT/os/hardware-gate/rpm-sbom-arch.sh"

SHIP_DECL="SHIP.md"
WORKFLOW=".github/workflows/build.yml"
SCREENSHOT_ROOT="os/screenshots/hardware-gate"
SIGNOFF="os/signoff-notes.md"
RUNBOOK="os/hardware-gate/runbook.md"
SCREENSHOT_RUN_DIR="${SCREENSHOT_RUN_DIR:-${SCREENSHOT_DIR:-}}"
FAIL_COUNT=0
ARCHES=(aarch64 x86_64)

REQ_SCREENSHOTS=(
  "01-installer.png"
  "02-install-network.png"
  "03-login.png"
  "04-desktop.png"
  "06-onboarding.png"
  "07-home.png"
  "08-shell-home.png"
  "09-shell-dark.png"
  "10-settings.png"
  "11-settings-models.png"
  "12-settings-dark.png"
  "13-studio-before.png"
  "14-studio-running.png"
  "15-studio-app-detail.png"
  "16-built-app-open.png"
  "17-dark-motion.png"
  "18-light-motion.png"
  "19-vulkan-vkcube.png"
  "20-gamemode-active.png"
  "21-gamescope-session.png"
  "22-mangohud-overlay.png"
  "23-controller-detection.png"
  "24-audio-output.png"
  "25-install-destination.png"
  "26-install-storage-summary.png"
  "27-dual-boot-preserve-existing-os.png"
  "28-bootloader-efi-summary.png"
  "29-preview-pdf-open.png"
  "30-preview-image-open.png"
  "31-text-shortcuts-candidate-bubble-render.png"
  "32-text-shortcuts-live-ibus-runtime-render.png"
)
FIREWALL_LIVE_TOGGLE_PROOF="firewall-live-toggle-proof.json"
TEXT_SHORTCUTS_SESSION_ENABLE_PROOF="text-shortcuts-session-enable-proof.json"
TEXT_SHORTCUTS_CANDIDATE_METADATA_PROOF="text-shortcuts-candidate-metadata-proof.json"
TEXT_SHORTCUTS_OVERLAY_INTENT_PROOF="text-shortcuts-overlay-intent-proof.json"
TEXT_SHORTCUTS_CANDIDATE_BUBBLE_FRAME_PROOF="text-shortcuts-candidate-bubble-frame-proof.json"
TEXT_SHORTCUTS_CANDIDATE_BUBBLE_LAYOUT_PROOF="text-shortcuts-candidate-bubble-layout-proof.json"
TEXT_SHORTCUTS_CANDIDATE_BUBBLE_RENDER_INTENT_PROOF="text-shortcuts-candidate-bubble-render-intent-proof.json"
TEXT_SHORTCUTS_CANDIDATE_BUBBLE_RENDER_PROOF="text-shortcuts-candidate-bubble-render-proof.json"
TEXT_SHORTCUTS_LIVE_IBUS_RUNTIME_RENDER_PROOF="text-shortcuts-live-ibus-runtime-render-proof.json"
KEYBOARD_SHORTCUTS_ROUNDTRIP_PROOF="keyboard-shortcuts-roundtrip-proof.json"
INPUT_SOURCES_ROUNDTRIP_PROOF="input-sources-roundtrip-proof.json"
MULTI_DISPLAY_APPLY_PROOF="multi-display-apply-proof.json"
FOCUS_ARM_ROUNDTRIP_PROOF="focus-arm-roundtrip-proof.json"
APP_PRIVACY_REVOKE_PROOF="app-privacy-revoke-proof.json"
PREVIEW_OPEN_RENDER_PROOF="preview-open-render-proof.json"
AUDIO_OUTPUT_PROOF="audio-output-proof.json"
RUNTIME_BUILD_PROOF="runtime-build-proof.json"

check() {
  local label="$1"
  local test_cmd="$2"
  if eval "$test_cmd"; then
    echo "[PASS] $label"
  else
    echo "[FAIL] $label"
    FAIL_COUNT=$((FAIL_COUNT + 1))
  fi
}

fail_check() {
  local label="$1"
  echo "[FAIL] $label"
  FAIL_COUNT=$((FAIL_COUNT + 1))
}

check_file() {
  local label="$1"
  local path="$2"
  if [ -f "$path" ]; then
    echo "[PASS] $label"
    return 0
  fi
  echo "[FAIL] $label: missing $path"
  FAIL_COUNT=$((FAIL_COUNT + 1))
  return 1
}

check_file_contains() {
  local label="$1"
  local path="$2"
  local pattern="$3"
  if [ ! -f "$path" ]; then
    echo "[FAIL] $label: missing $path"
    FAIL_COUNT=$((FAIL_COUNT + 1))
    return 1
  fi
  if rg -q "$pattern" "$path"; then
    echo "[PASS] $label"
    return 0
  fi
  echo "[FAIL] $label: $path does not contain $pattern"
  FAIL_COUNT=$((FAIL_COUNT + 1))
  return 1
}

check_sha256_file() {
  local label="$1"
  local sha_path="$2"
  local expected actual artifact sha_dir sha_base

  if [ ! -f "$sha_path" ]; then
    echo "[FAIL] $label: missing $sha_path"
    FAIL_COUNT=$((FAIL_COUNT + 1))
    return 1
  fi

  # Verify from the checksum file's own directory so a portable, basename-relative
  # checksum resolves correctly (an absolute legacy path also still works).
  sha_dir="$(dirname "$sha_path")"
  sha_base="$(basename "$sha_path")"
  if command -v sha256sum >/dev/null 2>&1; then
    if (cd "$sha_dir" && sha256sum -c "$sha_base" >/dev/null 2>&1); then
      echo "[PASS] $label"
      return 0
    fi
  elif command -v shasum >/dev/null 2>&1; then
    read -r expected artifact < "$sha_path"
    if [ -n "$expected" ] && [ -n "$artifact" ] && (cd "$sha_dir" && [ -f "$artifact" ]); then
      actual="$(cd "$sha_dir" && shasum -a 256 "$artifact" | awk '{print $1}')"
      if [ "$actual" = "$expected" ]; then
        echo "[PASS] $label"
        return 0
      fi
    fi
  else
    echo "[FAIL] $label: no sha256sum or shasum command available"
    FAIL_COUNT=$((FAIL_COUNT + 1))
    return 1
  fi

  echo "[FAIL] $label: checksum verification failed for $sha_path"
  FAIL_COUNT=$((FAIL_COUNT + 1))
  return 1
}

check_bib_manifest_payload_ref() {
  local label="$1"
  local path="$2"

  if [ ! -f "$path" ]; then
    echo "[FAIL] $label: missing $path"
    FAIL_COUNT=$((FAIL_COUNT + 1))
    return 1
  fi
  if rg -q 'bootc switch --mutate-in-place --transport registry (host\.docker\.internal|localhost[:/]|127\.|0\.0\.0\.0[:/]|goblins-os:|docker\.io/library/goblins-os:)' "$path"; then
    echo "[FAIL] $label: installer payload tracks a local-only Docker/test registry"
    FAIL_COUNT=$((FAIL_COUNT + 1))
    return 1
  fi
  echo "[PASS] $label"
  return 0
}

source_secret_scan() {
  local output="${TMPDIR:-/tmp}/goblins_os_secret_scan.$$"
  : > "$output"

  rg -n --hidden --no-ignore-vcs --no-ignore \
    '^[[:space:]]*(export[[:space:]]+)?(OPENAI_API_KEY|AI_GATEWAY_API_KEY|OPENAI_ACCOUNT_CLIENT_SECRET)[[:space:]]*=[[:space:]]*([^<[:space:]#][^#]*)' \
    . \
    --glob '!.git/**' \
    --glob '!.claude/**' \
    --glob '!target/**' \
    --glob '!.ci-target/**' \
    --glob '!.ci-target-amd64/**' \
    --glob '!artifacts/**' \
    --glob '!libpod/**' \
    --glob '!os/signoff-proofs/**' \
    --glob '!os/screenshots/**' \
    --glob '!os/iso/output*/**' \
    --glob '!os/brand/*.png' \
    >> "$output" || true

  rg -n --hidden --no-ignore-vcs --no-ignore \
    '(^|[^A-Za-z0-9_-])(sk-proj-[A-Za-z0-9_-]{24,}|sk-[A-Za-z0-9_-]{29,})' \
    . \
    --glob '!.git/**' \
    --glob '!.claude/**' \
    --glob '!target/**' \
    --glob '!.ci-target/**' \
    --glob '!.ci-target-amd64/**' \
    --glob '!artifacts/**' \
    --glob '!libpod/**' \
    --glob '!os/signoff-proofs/**' \
    --glob '!os/screenshots/**' \
    --glob '!os/iso/output*/**' \
    --glob '!os/brand/*.png' \
    | rg -vi 'placeholder|example|secretvalue|abcdefghijklmnopqrstuvwxyz|server-side-only-gateway-key' \
    >> "$output" || true

  if [ -s "$output" ]; then
    echo "Possible live secrets found:"
    sed -n '1,20p' "$output"
    rm -f "$output"
    return 1
  fi

  rm -f "$output"
  return 0
}

screenshot_run_is_complete() {
  local run_dir="$1"
  local arch
  local shot
  arch="$(screenshot_run_arch "$run_dir")"
  [ -n "$arch" ] || return 1
  for shot in "${REQ_SCREENSHOTS[@]}"; do
    screenshot_file_is_valid_png "$run_dir/$shot" || return 1
  done
  screenshot_manifest_matches_iso "$run_dir" "$arch" || return 1
  firewall_live_toggle_proof_passes "$run_dir/$FIREWALL_LIVE_TOGGLE_PROOF" || return 1
  text_shortcuts_session_enable_proof_passes "$run_dir/$TEXT_SHORTCUTS_SESSION_ENABLE_PROOF" || return 1
  text_shortcuts_candidate_metadata_proof_passes "$run_dir/$TEXT_SHORTCUTS_CANDIDATE_METADATA_PROOF" || return 1
  text_shortcuts_overlay_intent_proof_passes "$run_dir/$TEXT_SHORTCUTS_OVERLAY_INTENT_PROOF" || return 1
  text_shortcuts_candidate_bubble_frame_proof_passes "$run_dir/$TEXT_SHORTCUTS_CANDIDATE_BUBBLE_FRAME_PROOF" || return 1
  text_shortcuts_candidate_bubble_layout_proof_passes "$run_dir/$TEXT_SHORTCUTS_CANDIDATE_BUBBLE_LAYOUT_PROOF" || return 1
  text_shortcuts_candidate_bubble_render_intent_proof_passes "$run_dir/$TEXT_SHORTCUTS_CANDIDATE_BUBBLE_RENDER_INTENT_PROOF" || return 1
  text_shortcuts_candidate_bubble_render_proof_passes "$run_dir/$TEXT_SHORTCUTS_CANDIDATE_BUBBLE_RENDER_PROOF" || return 1
  text_shortcuts_live_ibus_runtime_render_proof_passes "$run_dir/$TEXT_SHORTCUTS_LIVE_IBUS_RUNTIME_RENDER_PROOF" || return 1
  keyboard_shortcuts_roundtrip_proof_passes "$run_dir/$KEYBOARD_SHORTCUTS_ROUNDTRIP_PROOF" || return 1
  input_sources_roundtrip_proof_passes "$run_dir/$INPUT_SOURCES_ROUNDTRIP_PROOF" || return 1
  multi_display_apply_proof_passes "$run_dir/$MULTI_DISPLAY_APPLY_PROOF" || return 1
  focus_arm_roundtrip_proof_passes "$run_dir/$FOCUS_ARM_ROUNDTRIP_PROOF" || return 1
  app_privacy_revoke_proof_passes "$run_dir/$APP_PRIVACY_REVOKE_PROOF" || return 1
  preview_open_render_proof_passes "$run_dir/$PREVIEW_OPEN_RENDER_PROOF" || return 1
  audio_output_proof_passes "$run_dir/$AUDIO_OUTPUT_PROOF" || return 1
  runtime_build_proof_passes "$run_dir/$RUNTIME_BUILD_PROOF" || return 1
  return 0
}

screenshot_run_arch() {
  case "/$1/" in
    */os/screenshots/hardware-gate/aarch64/*)
      echo "aarch64"
      ;;
    */os/screenshots/hardware-gate/x86_64/*)
      echo "x86_64"
      ;;
    *)
      echo ""
      ;;
  esac
}

screenshot_file_is_valid_png() {
  local file="$1"
  local signature

  [ -s "$file" ] || return 1
  signature="$(od -An -tx1 -N8 "$file" 2>/dev/null | tr -d ' \n')"
  [ "$signature" = "89504e470d0a1a0a" ]
}

screenshot_manifest_matches_iso() {
  local run_dir="$1"
  local arch="$2"
  local manifest="$run_dir/proof-manifest.json"
  local iso_path="os/iso/output/$arch/bootiso/goblins-os-$arch.iso"
  local sha_path="$iso_path.sha256"
  local iso_sha

  [ -s "$manifest" ] || return 1
  [ -f "$iso_path" ] || return 1
  [ -f "$sha_path" ] || return 1
  iso_sha="$(awk '{print $1; exit}' "$sha_path")"
  [ -n "$iso_sha" ] || return 1
  rg -q '"architecture"[[:space:]]*:[[:space:]]*"'"$arch"'"' "$manifest" \
    && rg -q '"iso"[[:space:]]*:[[:space:]]*"'"$iso_path"'"' "$manifest" \
    && rg -q '"iso_sha256"[[:space:]]*:[[:space:]]*"'"$iso_sha"'"' "$manifest" \
    && rg -q '"captured_at"[[:space:]]*:[[:space:]]*"[^"]+"' "$manifest" \
    && rg -q '"screenshot_run_dir"[[:space:]]*:[[:space:]]*"'"$run_dir"'"' "$manifest" \
    && rg -q '"firewall_live_toggle_proof"[[:space:]]*:[[:space:]]*"'"$FIREWALL_LIVE_TOGGLE_PROOF"'"' "$manifest" \
    && rg -q '"text_shortcuts_session_enable_proof"[[:space:]]*:[[:space:]]*"'"$TEXT_SHORTCUTS_SESSION_ENABLE_PROOF"'"' "$manifest" \
    && rg -q '"text_shortcuts_candidate_metadata_proof"[[:space:]]*:[[:space:]]*"'"$TEXT_SHORTCUTS_CANDIDATE_METADATA_PROOF"'"' "$manifest" \
    && rg -q '"text_shortcuts_overlay_intent_proof"[[:space:]]*:[[:space:]]*"'"$TEXT_SHORTCUTS_OVERLAY_INTENT_PROOF"'"' "$manifest" \
    && rg -q '"text_shortcuts_candidate_bubble_frame_proof"[[:space:]]*:[[:space:]]*"'"$TEXT_SHORTCUTS_CANDIDATE_BUBBLE_FRAME_PROOF"'"' "$manifest" \
    && rg -q '"text_shortcuts_candidate_bubble_layout_proof"[[:space:]]*:[[:space:]]*"'"$TEXT_SHORTCUTS_CANDIDATE_BUBBLE_LAYOUT_PROOF"'"' "$manifest" \
    && rg -q '"text_shortcuts_candidate_bubble_render_intent_proof"[[:space:]]*:[[:space:]]*"'"$TEXT_SHORTCUTS_CANDIDATE_BUBBLE_RENDER_INTENT_PROOF"'"' "$manifest" \
    && rg -q '"text_shortcuts_candidate_bubble_render_proof"[[:space:]]*:[[:space:]]*"'"$TEXT_SHORTCUTS_CANDIDATE_BUBBLE_RENDER_PROOF"'"' "$manifest" \
    && rg -q '"text_shortcuts_live_ibus_runtime_render_proof"[[:space:]]*:[[:space:]]*"'"$TEXT_SHORTCUTS_LIVE_IBUS_RUNTIME_RENDER_PROOF"'"' "$manifest" \
    && rg -q '"keyboard_shortcuts_roundtrip_proof"[[:space:]]*:[[:space:]]*"'"$KEYBOARD_SHORTCUTS_ROUNDTRIP_PROOF"'"' "$manifest" \
    && rg -q '"input_sources_roundtrip_proof"[[:space:]]*:[[:space:]]*"'"$INPUT_SOURCES_ROUNDTRIP_PROOF"'"' "$manifest" \
    && rg -q '"multi_display_apply_proof"[[:space:]]*:[[:space:]]*"'"$MULTI_DISPLAY_APPLY_PROOF"'"' "$manifest" \
    && rg -q '"focus_arm_roundtrip_proof"[[:space:]]*:[[:space:]]*"'"$FOCUS_ARM_ROUNDTRIP_PROOF"'"' "$manifest" \
    && rg -q '"app_privacy_revoke_proof"[[:space:]]*:[[:space:]]*"'"$APP_PRIVACY_REVOKE_PROOF"'"' "$manifest" \
    && rg -q '"preview_open_render_proof"[[:space:]]*:[[:space:]]*"'"$PREVIEW_OPEN_RENDER_PROOF"'"' "$manifest" \
    && rg -q '"audio_output_proof"[[:space:]]*:[[:space:]]*"'"$AUDIO_OUTPUT_PROOF"'"' "$manifest" \
    && rg -q '"runtime_build_proof"[[:space:]]*:[[:space:]]*"'"$RUNTIME_BUILD_PROOF"'"' "$manifest"
}

firewall_live_toggle_proof_passes() {
  local proof="$1"

  [ -s "$proof" ] \
    && rg -q '"status"[[:space:]]*:[[:space:]]*"pass"' "$proof" \
    && rg -q '"route"[[:space:]]*:[[:space:]]*"/v1/firewall/enabled"' "$proof" \
    && rg -q '"status_route"[[:space:]]*:[[:space:]]*"/v1/firewall/status"' "$proof" \
    && rg -q '"disable_http"[[:space:]]*:[[:space:]]*"200"' "$proof" \
    && rg -q '"disable_ok"[[:space:]]*:[[:space:]]*"true"' "$proof" \
    && rg -q '"disable_enabled"[[:space:]]*:[[:space:]]*"false"' "$proof" \
    && rg -q '"disable_active"[[:space:]]*:[[:space:]]*"false"' "$proof" \
    && rg -q '"enable_http"[[:space:]]*:[[:space:]]*"200"' "$proof" \
    && rg -q '"enable_ok"[[:space:]]*:[[:space:]]*"true"' "$proof" \
    && rg -q '"enable_enabled"[[:space:]]*:[[:space:]]*"true"' "$proof" \
    && rg -q '"enable_active"[[:space:]]*:[[:space:]]*"true"' "$proof" \
    && rg -q '"unit_template"[[:space:]]*:[[:space:]]*"goblins-os-firewall@\.service"' "$proof" \
    && rg -q '"polkit_rule"[[:space:]]*:[[:space:]]*"60-goblins-os-firewall.rules"' "$proof"
}

text_shortcuts_session_enable_proof_passes() {
  local proof="$1"

  [ -s "$proof" ] \
    && rg -q '"status"[[:space:]]*:[[:space:]]*"pass"' "$proof" \
    && rg -q '"route"[[:space:]]*:[[:space:]]*"/v1/text-shortcuts"' "$proof" \
    && rg -q '"service"[[:space:]]*:[[:space:]]*"active"' "$proof" \
    && rg -q '"service_unit"[[:space:]]*:[[:space:]]*"org.freedesktop.IBus.session.GNOME.service"' "$proof" \
    && rg -q '"input_source_configured"[[:space:]]*:[[:space:]]*"true"' "$proof" \
    && rg -q '"preload_configured"[[:space:]]*:[[:space:]]*"true"' "$proof" \
    && rg -q '"engine_listed"[[:space:]]*:[[:space:]]*"true"' "$proof" \
    && rg -q '"active_engine"[[:space:]]*:[[:space:]]*"goblins-textshortcuts"' "$proof" \
    && rg -q '"adapter_self_test"[[:space:]]*:[[:space:]]*"pass"' "$proof" \
    && rg -q '"core_http"[[:space:]]*:[[:space:]]*"200"' "$proof" \
    && rg -q '"core_engine_available"[[:space:]]*:[[:space:]]*"true"' "$proof" \
    && rg -q '"core_runtime_loop_available"[[:space:]]*:[[:space:]]*"true"' "$proof" \
    && rg -q '"runtime_ready_claim"[[:space:]]*:[[:space:]]*"true"' "$proof"
}

text_shortcuts_candidate_metadata_proof_passes() {
  local proof="$1"

  [ -s "$proof" ] \
    && rg -q '"status"[[:space:]]*:[[:space:]]*"pass"' "$proof" \
    && rg -q '"route"[[:space:]]*:[[:space:]]*"/v1/text-shortcuts"' "$proof" \
    && rg -q '"surface"[[:space:]]*:[[:space:]]*"goblins-os-shell-text-shortcuts-candidate-proof"' "$proof" \
    && rg -q '"candidate_replacement"[[:space:]]*:[[:space:]]*"on my way"' "$proof" \
    && rg -q '"candidate_accept_on"[[:space:]]*:[[:space:]]*"word-boundary"' "$proof" \
    && rg -q '"candidate_dismiss_key"[[:space:]]*:[[:space:]]*"Escape"' "$proof" \
    && rg -q '"rendered_bubble_ready_claim"[[:space:]]*:[[:space:]]*"false"' "$proof" \
    && rg -q '"live_overlay_claim"[[:space:]]*:[[:space:]]*"false"' "$proof" \
    && rg -q '"runtime_ready_claim"[[:space:]]*:[[:space:]]*"false"' "$proof"
}

text_shortcuts_overlay_intent_proof_passes() {
  local proof="$1"

  [ -s "$proof" ] \
    && rg -q '"status"[[:space:]]*:[[:space:]]*"pass"' "$proof" \
    && rg -q '"route"[[:space:]]*:[[:space:]]*"/v1/text-shortcuts"' "$proof" \
    && rg -q '"surface"[[:space:]]*:[[:space:]]*"goblins-textshortcuts-ibus-adapter-overlay-intent"' "$proof" \
    && rg -q '"adapter_self_test"[[:space:]]*:[[:space:]]*"pass"' "$proof" \
    && rg -q '"show_count"[[:space:]]*:[[:space:]]*"2"' "$proof" \
    && rg -q '"hide_count"[[:space:]]*:[[:space:]]*"2"' "$proof" \
    && rg -q '"dismissed_reason"[[:space:]]*:[[:space:]]*"true"' "$proof" \
    && rg -q '"committed_reason"[[:space:]]*:[[:space:]]*"true"' "$proof" \
    && rg -q '"rendered_bubble_ready_claim"[[:space:]]*:[[:space:]]*"false"' "$proof" \
    && rg -q '"live_overlay_claim"[[:space:]]*:[[:space:]]*"false"' "$proof" \
    && rg -q '"runtime_ready_claim"[[:space:]]*:[[:space:]]*"false"' "$proof"
}

text_shortcuts_candidate_bubble_frame_proof_passes() {
  local proof="$1"

  [ -s "$proof" ] \
    && rg -q '"status"[[:space:]]*:[[:space:]]*"pass"' "$proof" \
    && rg -q '"route"[[:space:]]*:[[:space:]]*"/v1/text-shortcuts"' "$proof" \
    && rg -q '"surface"[[:space:]]*:[[:space:]]*"goblins-textshortcuts-accept-bubble-frame"' "$proof" \
    && rg -q '"adapter_self_test"[[:space:]]*:[[:space:]]*"pass"' "$proof" \
    && rg -q '"show_frame_count"[[:space:]]*:[[:space:]]*"2"' "$proof" \
    && rg -q '"hide_frame_count"[[:space:]]*:[[:space:]]*"2"' "$proof" \
    && rg -q '"dismissed_frame"[[:space:]]*:[[:space:]]*"true"' "$proof" \
    && rg -q '"committed_frame"[[:space:]]*:[[:space:]]*"true"' "$proof" \
    && rg -q '"replacement"[[:space:]]*:[[:space:]]*"on my way"' "$proof" \
    && rg -q '"accept_on"[[:space:]]*:[[:space:]]*"word-boundary"' "$proof" \
    && rg -q '"accept_keys"[[:space:]]*:[[:space:]]*"Space,Return"' "$proof" \
    && rg -q '"dismiss_key"[[:space:]]*:[[:space:]]*"Escape"' "$proof" \
    && rg -q '"style_class"[[:space:]]*:[[:space:]]*"gos-text-shortcuts-candidate"' "$proof" \
    && rg -q '"text_style_class"[[:space:]]*:[[:space:]]*"gos-text-shortcuts-candidate-text"' "$proof" \
    && rg -q '"hint_style_class"[[:space:]]*:[[:space:]]*"gos-text-shortcuts-candidate-hint"' "$proof" \
    && rg -q '"font_family"[[:space:]]*:[[:space:]]*"Inter"' "$proof" \
    && rg -q '"sensitive_field_refusal"[[:space:]]*:[[:space:]]*"true"' "$proof" \
    && rg -q '"rendered_bubble_ready_claim"[[:space:]]*:[[:space:]]*"false"' "$proof" \
    && rg -q '"live_overlay_claim"[[:space:]]*:[[:space:]]*"false"' "$proof" \
    && rg -q '"runtime_ready_claim"[[:space:]]*:[[:space:]]*"false"' "$proof"
}

text_shortcuts_candidate_bubble_layout_proof_passes() {
  local proof="$1"

  [ -s "$proof" ] \
    && rg -q '"status"[[:space:]]*:[[:space:]]*"pass"' "$proof" \
    && rg -q '"route"[[:space:]]*:[[:space:]]*"/v1/text-shortcuts"' "$proof" \
    && rg -q '"surface"[[:space:]]*:[[:space:]]*"goblins-textshortcuts-accept-bubble-layout"' "$proof" \
    && rg -q '"adapter_self_test"[[:space:]]*:[[:space:]]*"pass"' "$proof" \
    && rg -q '"frame_surface"[[:space:]]*:[[:space:]]*"goblins-textshortcuts-accept-bubble-frame"' "$proof" \
    && rg -q '"layout_count"[[:space:]]*:[[:space:]]*"4"' "$proof" \
    && rg -q '"visible_layout_count"[[:space:]]*:[[:space:]]*"3"' "$proof" \
    && rg -q '"right_edge_clamped"[[:space:]]*:[[:space:]]*"true"' "$proof" \
    && rg -q '"bottom_edge_flipped"[[:space:]]*:[[:space:]]*"true"' "$proof" \
    && rg -q '"hidden_frame_collapses"[[:space:]]*:[[:space:]]*"true"' "$proof" \
    && rg -q '"style_class"[[:space:]]*:[[:space:]]*"gos-text-shortcuts-candidate"' "$proof" \
    && rg -q '"font_family"[[:space:]]*:[[:space:]]*"Inter"' "$proof" \
    && rg -q '"rendered_bubble_ready_claim"[[:space:]]*:[[:space:]]*"false"' "$proof" \
    && rg -q '"live_overlay_claim"[[:space:]]*:[[:space:]]*"false"' "$proof" \
    && rg -q '"runtime_ready_claim"[[:space:]]*:[[:space:]]*"false"' "$proof"
}

text_shortcuts_candidate_bubble_render_intent_proof_passes() {
  local proof="$1"

  [ -s "$proof" ] \
    && rg -q '"status"[[:space:]]*:[[:space:]]*"pass"' "$proof" \
    && rg -q '"route"[[:space:]]*:[[:space:]]*"/v1/text-shortcuts"' "$proof" \
    && rg -q '"surface"[[:space:]]*:[[:space:]]*"goblins-textshortcuts-accept-bubble-render-intent"' "$proof" \
    && rg -q '"adapter_self_test"[[:space:]]*:[[:space:]]*"pass"' "$proof" \
    && rg -q '"frame_surface"[[:space:]]*:[[:space:]]*"goblins-textshortcuts-accept-bubble-frame"' "$proof" \
    && rg -q '"layout_surface"[[:space:]]*:[[:space:]]*"goblins-textshortcuts-accept-bubble-layout"' "$proof" \
    && rg -q '"render_intent_count"[[:space:]]*:[[:space:]]*"8"' "$proof" \
    && rg -q '"show_intent_count"[[:space:]]*:[[:space:]]*"4"' "$proof" \
    && rg -q '"hide_intent_count"[[:space:]]*:[[:space:]]*"4"' "$proof" \
    && rg -q '"dismissed_intent"[[:space:]]*:[[:space:]]*"true"' "$proof" \
    && rg -q '"committed_intent"[[:space:]]*:[[:space:]]*"true"' "$proof" \
    && rg -q '"focus_out_hide"[[:space:]]*:[[:space:]]*"true"' "$proof" \
    && rg -q '"sensitive_hide"[[:space:]]*:[[:space:]]*"true"' "$proof" \
    && rg -q '"pass_through_unchanged"[[:space:]]*:[[:space:]]*"true"' "$proof" \
    && rg -q '"sink_failure_fail_open"[[:space:]]*:[[:space:]]*"true"' "$proof" \
    && rg -q '"style_class"[[:space:]]*:[[:space:]]*"gos-text-shortcuts-candidate"' "$proof" \
    && rg -q '"font_family"[[:space:]]*:[[:space:]]*"Inter"' "$proof" \
    && rg -q '"rendered_bubble_ready_claim"[[:space:]]*:[[:space:]]*"false"' "$proof" \
    && rg -q '"live_overlay_claim"[[:space:]]*:[[:space:]]*"false"' "$proof" \
	    && rg -q '"runtime_ready_claim"[[:space:]]*:[[:space:]]*"false"' "$proof"
}

text_shortcuts_candidate_bubble_render_proof_passes() {
  local proof="$1"

  [ -s "$proof" ] \
    && rg -q '"status"[[:space:]]*:[[:space:]]*"pass"' "$proof" \
    && rg -q '"route"[[:space:]]*:[[:space:]]*"/v1/text-shortcuts"' "$proof" \
    && rg -q '"surface"[[:space:]]*:[[:space:]]*"goblins-os-shell-text-shortcuts-candidate-bubble-render"' "$proof" \
    && rg -q '"render_intent_surface"[[:space:]]*:[[:space:]]*"goblins-textshortcuts-accept-bubble-render-intent"' "$proof" \
    && rg -q '"layout_surface"[[:space:]]*:[[:space:]]*"goblins-textshortcuts-accept-bubble-layout"' "$proof" \
    && rg -q '"frame_surface"[[:space:]]*:[[:space:]]*"goblins-textshortcuts-accept-bubble-frame"' "$proof" \
    && rg -q '"replacement"[[:space:]]*:[[:space:]]*"on my way"' "$proof" \
    && rg -q '"accept_on"[[:space:]]*:[[:space:]]*"word-boundary"' "$proof" \
    && rg -q '"dismiss_key"[[:space:]]*:[[:space:]]*"Escape"' "$proof" \
    && rg -q '"style_class"[[:space:]]*:[[:space:]]*"gos-text-shortcuts-candidate"' "$proof" \
    && rg -q '"text_style_class"[[:space:]]*:[[:space:]]*"gos-text-shortcuts-candidate-text"' "$proof" \
    && rg -q '"hint_style_class"[[:space:]]*:[[:space:]]*"gos-text-shortcuts-candidate-hint"' "$proof" \
    && rg -q '"font_family"[[:space:]]*:[[:space:]]*"Inter"' "$proof" \
    && rg -q '"screenshot"[[:space:]]*:[[:space:]]*"31-text-shortcuts-candidate-bubble-render\.png"' "$proof" \
    && rg -q '"rendered_candidate_surface"[[:space:]]*:[[:space:]]*"true"' "$proof" \
    && rg -q '"rendered_bubble_ready_claim"[[:space:]]*:[[:space:]]*"false"' "$proof" \
    && rg -q '"live_overlay_claim"[[:space:]]*:[[:space:]]*"false"' "$proof" \
    && rg -q '"runtime_ready_claim"[[:space:]]*:[[:space:]]*"false"' "$proof"
}

text_shortcuts_live_ibus_runtime_render_proof_passes() {
  local proof="$1"

  [ -s "$proof" ] \
    && rg -q '"status"[[:space:]]*:[[:space:]]*"pass"' "$proof" \
    && rg -q '"route"[[:space:]]*:[[:space:]]*"/v1/text-shortcuts"' "$proof" \
    && rg -q '"surface"[[:space:]]*:[[:space:]]*"goblins-textshortcuts-live-ibus-runtime-render"' "$proof" \
    && rg -q '"input_driver"[[:space:]]*:[[:space:]]*"qmp-keyboard"' "$proof" \
    && rg -q '"active_engine"[[:space:]]*:[[:space:]]*"goblins-textshortcuts"' "$proof" \
    && rg -q '"normal_actual"[[:space:]]*:[[:space:]]*"onmyway\."' "$proof" \
    && rg -q '"passthrough_actual"[[:space:]]*:[[:space:]]*"hello\."' "$proof" \
    && rg -q '"password_refusal"[[:space:]]*:[[:space:]]*"true"' "$proof" \
    && rg -q '"focused_field_callback"[[:space:]]*:[[:space:]]*"true"' "$proof" \
    && rg -q '"text_input_v3_commit"[[:space:]]*:[[:space:]]*"true"' "$proof" \
    && rg -q '"rendered_accept_bubble"[[:space:]]*:[[:space:]]*"true"' "$proof" \
    && rg -q '"screenshot"[[:space:]]*:[[:space:]]*"32-text-shortcuts-live-ibus-runtime-render\.png"' "$proof" \
    && rg -q '"style_class"[[:space:]]*:[[:space:]]*"gos-text-shortcuts-candidate"' "$proof" \
    && rg -q '"font_family"[[:space:]]*:[[:space:]]*"Inter"' "$proof" \
    && rg -q '"rendered_bubble_ready_claim"[[:space:]]*:[[:space:]]*"true"' "$proof" \
    && rg -q '"live_overlay_claim"[[:space:]]*:[[:space:]]*"true"' "$proof" \
    && rg -q '"runtime_ready_claim"[[:space:]]*:[[:space:]]*"true"' "$proof" \
    && rg -q '"core_readiness_flip"[[:space:]]*:[[:space:]]*"live"' "$proof"
}

keyboard_shortcuts_roundtrip_proof_passes() {
  local proof="$1"

  [ -s "$proof" ] \
    && rg -q '"status"[[:space:]]*:[[:space:]]*"pass"' "$proof" \
    && rg -q '"shortcut_route"[[:space:]]*:[[:space:]]*"/v1/keyboard/shortcuts/binding"' "$proof" \
    && rg -q '"modifier_route"[[:space:]]*:[[:space:]]*"/v1/keyboard/modifier-remap"' "$proof" \
    && rg -q '"shortcut_action"[[:space:]]*:[[:space:]]*"window-hud"' "$proof" \
    && rg -q '"shortcut_binding"[[:space:]]*:[[:space:]]*"<Super><Shift>H"' "$proof" \
    && rg -q '"shortcut_http"[[:space:]]*:[[:space:]]*"200"' "$proof" \
    && rg -q '"shortcut_gsettings_readback"[[:space:]]*:[[:space:]]*"true"' "$proof" \
    && rg -q '"shortcut_reset_http"[[:space:]]*:[[:space:]]*"200"' "$proof" \
    && rg -q '"shortcut_reset_binding"[[:space:]]*:[[:space:]]*"<Super>w"' "$proof" \
    && rg -q '"modifier_target"[[:space:]]*:[[:space:]]*"caps-lock"' "$proof" \
    && rg -q '"modifier_value"[[:space:]]*:[[:space:]]*"control"' "$proof" \
    && rg -q '"modifier_http"[[:space:]]*:[[:space:]]*"200"' "$proof" \
    && rg -q '"modifier_gsettings_readback"[[:space:]]*:[[:space:]]*"ctrl:nocaps"' "$proof" \
    && rg -q '"modifier_reset_http"[[:space:]]*:[[:space:]]*"200"' "$proof" \
    && rg -q '"modifier_restore"[[:space:]]*:[[:space:]]*"default"' "$proof" \
    && rg -q '"roundtrip_restored"[[:space:]]*:[[:space:]]*"true"' "$proof"
}

input_sources_roundtrip_proof_passes() {
  local proof="$1"

  [ -s "$proof" ] \
    && rg -q '"status"[[:space:]]*:[[:space:]]*"pass"' "$proof" \
    && rg -q '"source_route"[[:space:]]*:[[:space:]]*"/v1/input/sources"' "$proof" \
    && rg -q '"switch_route"[[:space:]]*:[[:space:]]*"/v1/input/switch-next"' "$proof" \
    && rg -q '"test_sources"[[:space:]]*:[[:space:]]*"xkb-us,xkb-gb"' "$proof" \
    && rg -q '"set_http"[[:space:]]*:[[:space:]]*"200"' "$proof" \
    && rg -q '"set_ok"[[:space:]]*:[[:space:]]*"true"' "$proof" \
    && rg -q '"sources_gsettings_readback"[[:space:]]*:[[:space:]]*"true"' "$proof" \
    && rg -q '"current_before_switch"[[:space:]]*:[[:space:]]*"0"' "$proof" \
    && rg -q '"switch_http"[[:space:]]*:[[:space:]]*"200"' "$proof" \
    && rg -q '"switch_ok"[[:space:]]*:[[:space:]]*"true"' "$proof" \
    && rg -q '"switch_switched"[[:space:]]*:[[:space:]]*"true"' "$proof" \
    && rg -q '"current_after_switch"[[:space:]]*:[[:space:]]*"1"' "$proof" \
    && rg -q '"restore_sources"[[:space:]]*:[[:space:]]*"true"' "$proof" \
    && rg -q '"restore_current"[[:space:]]*:[[:space:]]*"true"' "$proof" \
    && rg -q '"roundtrip_restored"[[:space:]]*:[[:space:]]*"true"' "$proof"
}

multi_display_apply_proof_passes() {
  local proof="$1"

  [ -s "$proof" ] \
    && rg -q '"status"[[:space:]]*:[[:space:]]*"pass"' "$proof" \
    && rg -q '"status_route"[[:space:]]*:[[:space:]]*"/v1/displays/status"' "$proof" \
    && rg -q '"apply_route"[[:space:]]*:[[:space:]]*"/v1/displays/apply"' "$proof" \
    && rg -q '"display_config"[[:space:]]*:[[:space:]]*"org.gnome.Mutter.DisplayConfig"' "$proof" \
    && rg -q '"connector"[[:space:]]*:[[:space:]]*"[^"]+"' "$proof" \
    && rg -q '"mode_id"[[:space:]]*:[[:space:]]*"[^"]+"' "$proof" \
    && rg -q '"serial"[[:space:]]*:[[:space:]]*"[0-9]+"' "$proof" \
    && rg -q '"verify_http"[[:space:]]*:[[:space:]]*"200"' "$proof" \
    && rg -q '"verify_ok"[[:space:]]*:[[:space:]]*"true"' "$proof" \
    && rg -q '"temporary_http"[[:space:]]*:[[:space:]]*"200"' "$proof" \
    && rg -q '"temporary_ok"[[:space:]]*:[[:space:]]*"true"' "$proof" \
    && rg -q '"persistent_guard_http"[[:space:]]*:[[:space:]]*"400"' "$proof" \
    && rg -q '"persistent_confirmation_required"[[:space:]]*:[[:space:]]*"true"' "$proof" \
    && rg -q '"stale_serial"[[:space:]]*:[[:space:]]*"[0-9]+"' "$proof" \
    && rg -q '"stale_serial_http"[[:space:]]*:[[:space:]]*"409"' "$proof" \
    && rg -q '"stale_serial_rejected"[[:space:]]*:[[:space:]]*"true"' "$proof" \
    && rg -q '"roundtrip_restored"[[:space:]]*:[[:space:]]*"true"' "$proof" \
    && rg -q '"persistent_keep_claim"[[:space:]]*:[[:space:]]*"false"' "$proof" \
    && rg -q '"same_layout_noop"[[:space:]]*:[[:space:]]*"true"' "$proof"
}

focus_arm_roundtrip_proof_passes() {
  local proof="$1"

  [ -s "$proof" ] \
    && rg -q '"status"[[:space:]]*:[[:space:]]*"pass"' "$proof" \
    && rg -q '"status_route"[[:space:]]*:[[:space:]]*"/v1/focus/status"' "$proof" \
    && rg -q '"activate_route"[[:space:]]*:[[:space:]]*"/v1/focus/activate"' "$proof" \
    && rg -q '"deactivate_route"[[:space:]]*:[[:space:]]*"/v1/focus/deactivate"' "$proof" \
    && rg -q '"test_mode"[[:space:]]*:[[:space:]]*"gate-work"' "$proof" \
    && rg -q '"test_mode_configured"[[:space:]]*:[[:space:]]*"true"' "$proof" \
    && rg -q '"activate_http"[[:space:]]*:[[:space:]]*"200"' "$proof" \
    && rg -q '"activate_ok"[[:space:]]*:[[:space:]]*"true"' "$proof" \
    && rg -q '"activate_active_mode"[[:space:]]*:[[:space:]]*"gate-work"' "$proof" \
    && rg -q '"active_mode_gsettings_readback"[[:space:]]*:[[:space:]]*"gate-work"' "$proof" \
    && rg -q '"armed_by_schedule_after_activate"[[:space:]]*:[[:space:]]*"false"' "$proof" \
    && rg -q '"restore_banners_after_activate"[[:space:]]*:[[:space:]]*"true"' "$proof" \
    && rg -q '"notification_banners_after_activate"[[:space:]]*:[[:space:]]*"false"' "$proof" \
    && rg -q '"deactivate_http"[[:space:]]*:[[:space:]]*"200"' "$proof" \
    && rg -q '"deactivate_ok"[[:space:]]*:[[:space:]]*"true"' "$proof" \
    && rg -q '"deactivate_active_mode"[[:space:]]*:[[:space:]]*""' "$proof" \
    && rg -q '"active_mode_after_deactivate"[[:space:]]*:[[:space:]]*""' "$proof" \
    && rg -q '"armed_by_schedule_after_deactivate"[[:space:]]*:[[:space:]]*"false"' "$proof" \
    && rg -q '"restore_banners_after_deactivate"[[:space:]]*:[[:space:]]*""' "$proof" \
    && rg -q '"notification_banners_after_deactivate"[[:space:]]*:[[:space:]]*"true"' "$proof" \
    && rg -q '"original_focus_state_restored"[[:space:]]*:[[:space:]]*"true"' "$proof" \
    && rg -q '"original_notification_banners_restored"[[:space:]]*:[[:space:]]*"true"' "$proof" \
    && rg -q '"roundtrip_restored"[[:space:]]*:[[:space:]]*"true"' "$proof" \
    && rg -q '"mode_crud_claim"[[:space:]]*:[[:space:]]*"false"' "$proof" \
    && rg -q '"schedule_claim"[[:space:]]*:[[:space:]]*"false"' "$proof" \
    && rg -q '"per_app_breakthroughs_claim"[[:space:]]*:[[:space:]]*"false"' "$proof"
}

app_privacy_revoke_proof_passes() {
  local proof="$1"

  [ -s "$proof" ] \
    && rg -q '"status"[[:space:]]*:[[:space:]]*"pass"' "$proof" \
    && rg -q '"route"[[:space:]]*:[[:space:]]*"/v1/app-privacy/revoke"' "$proof" \
    && rg -q '"table"[[:space:]]*:[[:space:]]*"location"' "$proof" \
    && rg -q '"app"[[:space:]]*:[[:space:]]*"org.goblins.GatePrivacyProof"' "$proof" \
    && rg -q '"seed_method"[[:space:]]*:[[:space:]]*"PermissionStore.SetPermission"' "$proof" \
    && rg -q '"revoke_method"[[:space:]]*:[[:space:]]*"PermissionStore.DeletePermission"' "$proof" \
    && rg -q '"readback_method"[[:space:]]*:[[:space:]]*"PermissionStore.GetPermission"' "$proof" \
    && rg -q '"seed_grant"[[:space:]]*:[[:space:]]*"yes"' "$proof" \
    && rg -q '"seed_readback"[[:space:]]*:[[:space:]]*"true"' "$proof" \
    && rg -q '"revoke_http"[[:space:]]*:[[:space:]]*"200"' "$proof" \
    && rg -q '"revoke_ok"[[:space:]]*:[[:space:]]*"true"' "$proof" \
    && rg -q '"post_revoke_absent"[[:space:]]*:[[:space:]]*"true"' "$proof" \
    && rg -q '"restore_prior_state"[[:space:]]*:[[:space:]]*"true"' "$proof" \
    && rg -q '"roundtrip_restored"[[:space:]]*:[[:space:]]*"true"' "$proof" \
    && rg -q '"resource_keyed_claim"[[:space:]]*:[[:space:]]*"false"' "$proof" \
    && rg -q '"device_revoke_claim"[[:space:]]*:[[:space:]]*"false"' "$proof"
}

preview_open_render_proof_passes() {
  local proof="$1"

  [ -s "$proof" ] \
    && rg -q '"status"[[:space:]]*:[[:space:]]*"pass"' "$proof" \
    && rg -q '"status_route"[[:space:]]*:[[:space:]]*"/v1/preview/status"' "$proof" \
    && rg -q '"route"[[:space:]]*:[[:space:]]*"/v1/preview/open"' "$proof" \
    && rg -q '"status_http"[[:space:]]*:[[:space:]]*"200"' "$proof" \
    && rg -q '"available"[[:space:]]*:[[:space:]]*"true"' "$proof" \
    && rg -q '"xdg_open"[[:space:]]*:[[:space:]]*"true"' "$proof" \
    && rg -q '"papers"[[:space:]]*:[[:space:]]*"true"' "$proof" \
    && rg -q '"loupe"[[:space:]]*:[[:space:]]*"true"' "$proof" \
    && rg -q '"pdf_default"[[:space:]]*:[[:space:]]*"org.gnome.Papers.desktop"' "$proof" \
    && rg -q '"image_default"[[:space:]]*:[[:space:]]*"org.gnome.Loupe.desktop"' "$proof" \
    && rg -q '"jpeg_default"[[:space:]]*:[[:space:]]*"org.gnome.Loupe.desktop"' "$proof" \
    && rg -q '"pdf_http"[[:space:]]*:[[:space:]]*"200"' "$proof" \
    && rg -q '"pdf_ok"[[:space:]]*:[[:space:]]*"true"' "$proof" \
    && rg -q '"pdf_kind"[[:space:]]*:[[:space:]]*"pdf"' "$proof" \
    && rg -q '"pdf_process"[[:space:]]*:[[:space:]]*"papers"' "$proof" \
    && rg -q '"pdf_screenshot"[[:space:]]*:[[:space:]]*"29-preview-pdf-open\.png"' "$proof" \
    && rg -q '"rendered_pdf_frame"[[:space:]]*:[[:space:]]*"true"' "$proof" \
    && rg -q '"image_http"[[:space:]]*:[[:space:]]*"200"' "$proof" \
    && rg -q '"image_ok"[[:space:]]*:[[:space:]]*"true"' "$proof" \
    && rg -q '"image_kind"[[:space:]]*:[[:space:]]*"image"' "$proof" \
    && rg -q '"image_process"[[:space:]]*:[[:space:]]*"loupe"' "$proof" \
    && rg -q '"image_screenshot"[[:space:]]*:[[:space:]]*"30-preview-image-open\.png"' "$proof" \
    && rg -q '"rendered_image_frame"[[:space:]]*:[[:space:]]*"true"' "$proof" \
    && rg -q '"unsupported_http"[[:space:]]*:[[:space:]]*"400"' "$proof" \
    && rg -q '"unsupported_ok"[[:space:]]*:[[:space:]]*"false"' "$proof" \
    && rg -q '"unsupported_rejected"[[:space:]]*:[[:space:]]*"true"' "$proof"
}

audio_output_proof_passes() {
  local proof="$1"

  [ -s "$proof" ] \
    && rg -q '"status"[[:space:]]*:[[:space:]]*"pass"' "$proof" \
    && rg -q '"status_route"[[:space:]]*:[[:space:]]*"/v1/audio/status"' "$proof" \
    && rg -q '"status_http"[[:space:]]*:[[:space:]]*"200"' "$proof" \
    && rg -q '"wireplumber_available"[[:space:]]*:[[:space:]]*"true"' "$proof" \
    && rg -q '"output_available"[[:space:]]*:[[:space:]]*"true"' "$proof" \
    && rg -q '"player"[[:space:]]*:[[:space:]]*"(pw-play|paplay)"' "$proof" \
    && rg -q '"test_tone_seconds"[[:space:]]*:[[:space:]]*"45"' "$proof" \
    && rg -q '"screenshot"[[:space:]]*:[[:space:]]*"24-audio-output\.png"' "$proof" \
    && rg -q '"rendered_sound_panel"[[:space:]]*:[[:space:]]*"true"' "$proof"
}

runtime_build_proof_passes() {
  local proof="$1"

  [ -s "$proof" ] \
    && rg -q '"status"[[:space:]]*:[[:space:]]*"pass"' "$proof" \
    && rg -q '"route"[[:space:]]*:[[:space:]]*"/v1/apps/builds"' "$proof" \
    && rg -q '"engine_mode"[[:space:]]*:[[:space:]]*"local-model"' "$proof" \
    && rg -q '"engine_source"[[:space:]]*:[[:space:]]*"[A-Za-z0-9._:-]+-built"' "$proof" \
    && rg -q '"built_artifact_id"[[:space:]]*:[[:space:]]*"[A-Za-z0-9._:-]+"' "$proof" \
    && rg -q '"built_artifact_name"[[:space:]]*:[[:space:]]*"[^"]+"' "$proof" \
    && rg -q '"intent"[[:space:]]*:[[:space:]]*"[^"]+"' "$proof"
}

print_missing_screenshot_paths() {
  local run_dir="$1"
  local missing=0
  local shot
  for shot in "${REQ_SCREENSHOTS[@]}"; do
    if ! screenshot_file_is_valid_png "$run_dir/$shot"; then
      echo "  $run_dir/$shot"
      missing=1
    fi
  done
  if [ ! -s "$run_dir/proof-manifest.json" ]; then
    echo "  $run_dir/proof-manifest.json"
    missing=1
  fi
  if ! firewall_live_toggle_proof_passes "$run_dir/$FIREWALL_LIVE_TOGGLE_PROOF"; then
    echo "  $run_dir/$FIREWALL_LIVE_TOGGLE_PROOF"
    missing=1
  fi
  if ! text_shortcuts_session_enable_proof_passes "$run_dir/$TEXT_SHORTCUTS_SESSION_ENABLE_PROOF"; then
    echo "  $run_dir/$TEXT_SHORTCUTS_SESSION_ENABLE_PROOF"
    missing=1
  fi
  if ! text_shortcuts_candidate_metadata_proof_passes "$run_dir/$TEXT_SHORTCUTS_CANDIDATE_METADATA_PROOF"; then
    echo "  $run_dir/$TEXT_SHORTCUTS_CANDIDATE_METADATA_PROOF"
    missing=1
  fi
  if ! text_shortcuts_overlay_intent_proof_passes "$run_dir/$TEXT_SHORTCUTS_OVERLAY_INTENT_PROOF"; then
    echo "  $run_dir/$TEXT_SHORTCUTS_OVERLAY_INTENT_PROOF"
    missing=1
  fi
  if ! text_shortcuts_candidate_bubble_frame_proof_passes "$run_dir/$TEXT_SHORTCUTS_CANDIDATE_BUBBLE_FRAME_PROOF"; then
    echo "  $run_dir/$TEXT_SHORTCUTS_CANDIDATE_BUBBLE_FRAME_PROOF"
    missing=1
  fi
  if ! text_shortcuts_candidate_bubble_layout_proof_passes "$run_dir/$TEXT_SHORTCUTS_CANDIDATE_BUBBLE_LAYOUT_PROOF"; then
    echo "  $run_dir/$TEXT_SHORTCUTS_CANDIDATE_BUBBLE_LAYOUT_PROOF"
    missing=1
  fi
  if ! text_shortcuts_candidate_bubble_render_intent_proof_passes "$run_dir/$TEXT_SHORTCUTS_CANDIDATE_BUBBLE_RENDER_INTENT_PROOF"; then
    echo "  $run_dir/$TEXT_SHORTCUTS_CANDIDATE_BUBBLE_RENDER_INTENT_PROOF"
    missing=1
  fi
  if ! text_shortcuts_candidate_bubble_render_proof_passes "$run_dir/$TEXT_SHORTCUTS_CANDIDATE_BUBBLE_RENDER_PROOF"; then
    echo "  $run_dir/$TEXT_SHORTCUTS_CANDIDATE_BUBBLE_RENDER_PROOF"
    missing=1
  fi
  if ! text_shortcuts_live_ibus_runtime_render_proof_passes "$run_dir/$TEXT_SHORTCUTS_LIVE_IBUS_RUNTIME_RENDER_PROOF"; then
    echo "  $run_dir/$TEXT_SHORTCUTS_LIVE_IBUS_RUNTIME_RENDER_PROOF"
    missing=1
  fi
  if ! keyboard_shortcuts_roundtrip_proof_passes "$run_dir/$KEYBOARD_SHORTCUTS_ROUNDTRIP_PROOF"; then
    echo "  $run_dir/$KEYBOARD_SHORTCUTS_ROUNDTRIP_PROOF"
    missing=1
  fi
  if ! input_sources_roundtrip_proof_passes "$run_dir/$INPUT_SOURCES_ROUNDTRIP_PROOF"; then
    echo "  $run_dir/$INPUT_SOURCES_ROUNDTRIP_PROOF"
    missing=1
  fi
  if ! multi_display_apply_proof_passes "$run_dir/$MULTI_DISPLAY_APPLY_PROOF"; then
    echo "  $run_dir/$MULTI_DISPLAY_APPLY_PROOF"
    missing=1
  fi
  if ! focus_arm_roundtrip_proof_passes "$run_dir/$FOCUS_ARM_ROUNDTRIP_PROOF"; then
    echo "  $run_dir/$FOCUS_ARM_ROUNDTRIP_PROOF"
    missing=1
  fi
  if ! app_privacy_revoke_proof_passes "$run_dir/$APP_PRIVACY_REVOKE_PROOF"; then
    echo "  $run_dir/$APP_PRIVACY_REVOKE_PROOF"
    missing=1
  fi
  if ! preview_open_render_proof_passes "$run_dir/$PREVIEW_OPEN_RENDER_PROOF"; then
    echo "  $run_dir/$PREVIEW_OPEN_RENDER_PROOF"
    missing=1
  fi
  if ! audio_output_proof_passes "$run_dir/$AUDIO_OUTPUT_PROOF"; then
    echo "  $run_dir/$AUDIO_OUTPUT_PROOF"
    missing=1
  fi
  if ! runtime_build_proof_passes "$run_dir/$RUNTIME_BUILD_PROOF"; then
    echo "  $run_dir/$RUNTIME_BUILD_PROOF"
    missing=1
  fi
  return "$missing"
}

print_latest_incomplete_screenshot_run() {
  local root_dir="$1"
  local label="$2"
  local latest=""

  if [ ! -d "$root_dir" ]; then
    echo "[INFO] $label screenshot root is missing: $root_dir"
    echo "[INFO] Expected screenshot proof files:"
    print_missing_screenshot_paths "$root_dir/<date>" || true
    return 0
  fi

  latest="$(find "$root_dir" -mindepth 1 -maxdepth 1 -type d | sort -r | head -n 1 || true)"
  if [ -z "$latest" ]; then
    echo "[INFO] $label screenshot root has no dated run directories: $root_dir"
    echo "[INFO] Expected screenshot proof files:"
    print_missing_screenshot_paths "$root_dir/<date>" || true
    return 0
  fi

  echo "[INFO] Latest incomplete $label screenshot run: $latest"
  echo "[INFO] Missing screenshot proof files:"
  print_missing_screenshot_paths "$latest" || true
}

print_legacy_screenshot_roots() {
  local dir
  local base
  local count=0
  local shown=0

  [ -d "$SCREENSHOT_ROOT" ] || return 0

  while IFS= read -r dir; do
    base="$(basename "$dir")"
    case "$base" in
      aarch64 | x86_64)
        continue
        ;;
    esac

    if [ "$count" -eq 0 ]; then
      echo "[INFO] Legacy/non-shipping screenshot roots ignored by architecture proof gate:"
    fi

    count=$((count + 1))
    if [ "$shown" -lt 12 ]; then
      echo "  $dir"
      shown=$((shown + 1))
    fi
  done < <(find "$SCREENSHOT_ROOT" -mindepth 1 -maxdepth 1 -type d | sort)

  if [ "$count" -gt "$shown" ]; then
    echo "  ... $((count - shown)) more"
  fi
}

print_screenshot_run_checks() {
  local run_dir="$1"
  local arch
  local missing=0
  local shot
  arch="$(screenshot_run_arch "$run_dir")"
  for shot in "${REQ_SCREENSHOTS[@]}"; do
    if screenshot_file_is_valid_png "$run_dir/$shot"; then
      echo "[PASS] $shot"
    else
      echo "[FAIL] $shot (missing, empty, or not a PNG)"
      missing=1
    fi
  done
  if [ -n "$arch" ] && screenshot_manifest_matches_iso "$run_dir" "$arch"; then
    echo "[PASS] proof-manifest.json"
  else
    echo "[FAIL] proof-manifest.json"
    missing=1
  fi
  if firewall_live_toggle_proof_passes "$run_dir/$FIREWALL_LIVE_TOGGLE_PROOF"; then
    echo "[PASS] $FIREWALL_LIVE_TOGGLE_PROOF"
  else
    echo "[FAIL] $FIREWALL_LIVE_TOGGLE_PROOF (missing or live firewall toggle proof failed)"
    missing=1
  fi
  if text_shortcuts_session_enable_proof_passes "$run_dir/$TEXT_SHORTCUTS_SESSION_ENABLE_PROOF"; then
    echo "[PASS] $TEXT_SHORTCUTS_SESSION_ENABLE_PROOF"
  else
    echo "[FAIL] $TEXT_SHORTCUTS_SESSION_ENABLE_PROOF (missing or Text Shortcuts session-enable proof failed)"
    missing=1
  fi
  if text_shortcuts_candidate_metadata_proof_passes "$run_dir/$TEXT_SHORTCUTS_CANDIDATE_METADATA_PROOF"; then
    echo "[PASS] $TEXT_SHORTCUTS_CANDIDATE_METADATA_PROOF"
  else
    echo "[FAIL] $TEXT_SHORTCUTS_CANDIDATE_METADATA_PROOF (missing or Text Shortcuts candidate metadata proof failed)"
    missing=1
  fi
  if text_shortcuts_overlay_intent_proof_passes "$run_dir/$TEXT_SHORTCUTS_OVERLAY_INTENT_PROOF"; then
    echo "[PASS] $TEXT_SHORTCUTS_OVERLAY_INTENT_PROOF"
  else
    echo "[FAIL] $TEXT_SHORTCUTS_OVERLAY_INTENT_PROOF (missing or Text Shortcuts overlay-intent proof failed)"
    missing=1
  fi
  if text_shortcuts_candidate_bubble_frame_proof_passes "$run_dir/$TEXT_SHORTCUTS_CANDIDATE_BUBBLE_FRAME_PROOF"; then
    echo "[PASS] $TEXT_SHORTCUTS_CANDIDATE_BUBBLE_FRAME_PROOF"
  else
    echo "[FAIL] $TEXT_SHORTCUTS_CANDIDATE_BUBBLE_FRAME_PROOF (missing or Text Shortcuts candidate-bubble-frame proof failed)"
    missing=1
  fi
  if text_shortcuts_candidate_bubble_layout_proof_passes "$run_dir/$TEXT_SHORTCUTS_CANDIDATE_BUBBLE_LAYOUT_PROOF"; then
    echo "[PASS] $TEXT_SHORTCUTS_CANDIDATE_BUBBLE_LAYOUT_PROOF"
  else
    echo "[FAIL] $TEXT_SHORTCUTS_CANDIDATE_BUBBLE_LAYOUT_PROOF (missing or Text Shortcuts candidate-bubble-layout proof failed)"
    missing=1
  fi
  if text_shortcuts_candidate_bubble_render_intent_proof_passes "$run_dir/$TEXT_SHORTCUTS_CANDIDATE_BUBBLE_RENDER_INTENT_PROOF"; then
    echo "[PASS] $TEXT_SHORTCUTS_CANDIDATE_BUBBLE_RENDER_INTENT_PROOF"
  else
    echo "[FAIL] $TEXT_SHORTCUTS_CANDIDATE_BUBBLE_RENDER_INTENT_PROOF (missing or Text Shortcuts candidate-bubble-render-intent proof failed)"
    missing=1
  fi
  if text_shortcuts_candidate_bubble_render_proof_passes "$run_dir/$TEXT_SHORTCUTS_CANDIDATE_BUBBLE_RENDER_PROOF"; then
    echo "[PASS] $TEXT_SHORTCUTS_CANDIDATE_BUBBLE_RENDER_PROOF"
  else
    echo "[FAIL] $TEXT_SHORTCUTS_CANDIDATE_BUBBLE_RENDER_PROOF (missing or Text Shortcuts candidate-bubble-render screenshot proof failed)"
    missing=1
  fi
  if text_shortcuts_live_ibus_runtime_render_proof_passes "$run_dir/$TEXT_SHORTCUTS_LIVE_IBUS_RUNTIME_RENDER_PROOF"; then
    echo "[PASS] $TEXT_SHORTCUTS_LIVE_IBUS_RUNTIME_RENDER_PROOF"
  else
    echo "[FAIL] $TEXT_SHORTCUTS_LIVE_IBUS_RUNTIME_RENDER_PROOF (missing or Text Shortcuts live IBus runtime/render proof failed)"
    missing=1
  fi
  if keyboard_shortcuts_roundtrip_proof_passes "$run_dir/$KEYBOARD_SHORTCUTS_ROUNDTRIP_PROOF"; then
    echo "[PASS] $KEYBOARD_SHORTCUTS_ROUNDTRIP_PROOF"
  else
    echo "[FAIL] $KEYBOARD_SHORTCUTS_ROUNDTRIP_PROOF (missing or Keyboard shortcuts roundtrip proof failed)"
    missing=1
  fi
  if input_sources_roundtrip_proof_passes "$run_dir/$INPUT_SOURCES_ROUNDTRIP_PROOF"; then
    echo "[PASS] $INPUT_SOURCES_ROUNDTRIP_PROOF"
  else
    echo "[FAIL] $INPUT_SOURCES_ROUNDTRIP_PROOF (missing or Input sources roundtrip proof failed)"
    missing=1
  fi
  if multi_display_apply_proof_passes "$run_dir/$MULTI_DISPLAY_APPLY_PROOF"; then
    echo "[PASS] $MULTI_DISPLAY_APPLY_PROOF"
  else
    echo "[FAIL] $MULTI_DISPLAY_APPLY_PROOF (missing or Multi-display apply proof failed)"
    missing=1
  fi
  if focus_arm_roundtrip_proof_passes "$run_dir/$FOCUS_ARM_ROUNDTRIP_PROOF"; then
    echo "[PASS] $FOCUS_ARM_ROUNDTRIP_PROOF"
  else
    echo "[FAIL] $FOCUS_ARM_ROUNDTRIP_PROOF (missing or Focus arm roundtrip proof failed)"
    missing=1
  fi
  if app_privacy_revoke_proof_passes "$run_dir/$APP_PRIVACY_REVOKE_PROOF"; then
    echo "[PASS] $APP_PRIVACY_REVOKE_PROOF"
  else
    echo "[FAIL] $APP_PRIVACY_REVOKE_PROOF (missing or App privacy revoke proof failed)"
    missing=1
  fi
  if preview_open_render_proof_passes "$run_dir/$PREVIEW_OPEN_RENDER_PROOF"; then
    echo "[PASS] $PREVIEW_OPEN_RENDER_PROOF"
  else
    echo "[FAIL] $PREVIEW_OPEN_RENDER_PROOF (missing or Preview open/render proof failed)"
    missing=1
  fi
  if audio_output_proof_passes "$run_dir/$AUDIO_OUTPUT_PROOF"; then
    echo "[PASS] $AUDIO_OUTPUT_PROOF"
  else
    echo "[FAIL] $AUDIO_OUTPUT_PROOF (missing or audio output proof failed)"
    missing=1
  fi
  if runtime_build_proof_passes "$run_dir/$RUNTIME_BUILD_PROOF"; then
    echo "[PASS] $RUNTIME_BUILD_PROOF"
  else
    echo "[FAIL] $RUNTIME_BUILD_PROOF (missing or runtime app-build proof failed)"
    missing=1
  fi
  return "$missing"
}

print_arch_next_steps() {
  local arch="$1"

  cat <<EOF

Next evidence command for $arch:
  GOBLINS_OS_ARCH=$arch \\
  GOBLINS_OS_CONTAINER_RUNTIME=docker \\
  RUN_QEMU=1 \\
  GOBLINS_OS_SHIPPABLE_RELEASE=1 \\
  GOBLINS_OS_BIB_SOURCE_IMAGE=<real release bootc image ref for $arch> \\
  REPO_ROOT="$ROOT" \\
  os/hardware-gate/run-external-gate.sh

Native runner preflight for $arch without building artifacts:
  GOBLINS_OS_ARCH=$arch PREFLIGHT_ONLY=1 REPO_ROOT="$ROOT" os/hardware-gate/run-external-gate.sh

Artifact/SBOM build for native $arch without display proof:
  GOBLINS_OS_ARCH=$arch RUN_QEMU=0 REPO_ROOT="$ROOT" os/hardware-gate/run-external-gate.sh

Docker-emulated artifact/SBOM build for non-native local testing:
  GOBLINS_OS_ARCH=$arch RUN_QEMU=0 GOBLINS_OS_ALLOW_EMULATED_DOCKER=1 REPO_ROOT="$ROOT" os/hardware-gate/run-external-gate.sh

Runtime app-build proof for $arch, from inside a Goblins OS image/container joined to a real local model runtime:
  PROOF_PATH=os/screenshots/hardware-gate/$arch/<date>/$RUNTIME_BUILD_PROOF \\
  BUILD_RESPONSE_PATH=os/screenshots/hardware-gate/$arch/<date>/build-response.json \\
  os/runtime-gate/build-an-app-live-model.sh

Final signoff row after the display-backed screenshots and runtime-built app proof exist:
  GOBLINS_OS_ARCH=$arch \\
  SCREENSHOT_RUN_DIR=os/screenshots/hardware-gate/$arch/<date> \\
  RUNTIME_ENGINE_MODE=<real-mode> \\
  RUNTIME_ENGINE_SOURCE=<real-engine-source> \\
  RUNTIME_ENGINE_CONFIG=<config-or-artifact-path> \\
  BUILT_ARTIFACT_PATH_URL=<real-built-app-path-or-url> \\
  ./os/hardware-gate/close-signoff.sh

Expected $arch proof files:
  os/iso/output/$arch/bootiso/goblins-os-$arch.iso
  os/iso/output/$arch/bootiso/goblins-os-$arch.iso.sha256
  os/iso/output/$arch/manifest-goblins-os-$arch.json
  os/signoff-proofs/sbom/$arch/rpm-packages.tsv
  os/screenshots/hardware-gate/$arch/<date>/${REQ_SCREENSHOTS[0]} ... ${REQ_SCREENSHOTS[$((${#REQ_SCREENSHOTS[@]} - 1))]}
  os/screenshots/hardware-gate/$arch/<date>/proof-manifest.json
  os/screenshots/hardware-gate/$arch/<date>/$FIREWALL_LIVE_TOGGLE_PROOF
  os/screenshots/hardware-gate/$arch/<date>/$TEXT_SHORTCUTS_SESSION_ENABLE_PROOF
  os/screenshots/hardware-gate/$arch/<date>/$TEXT_SHORTCUTS_CANDIDATE_METADATA_PROOF
  os/screenshots/hardware-gate/$arch/<date>/$TEXT_SHORTCUTS_OVERLAY_INTENT_PROOF
  os/screenshots/hardware-gate/$arch/<date>/$TEXT_SHORTCUTS_CANDIDATE_BUBBLE_FRAME_PROOF
  os/screenshots/hardware-gate/$arch/<date>/$TEXT_SHORTCUTS_CANDIDATE_BUBBLE_LAYOUT_PROOF
  os/screenshots/hardware-gate/$arch/<date>/$TEXT_SHORTCUTS_CANDIDATE_BUBBLE_RENDER_INTENT_PROOF
  os/screenshots/hardware-gate/$arch/<date>/$TEXT_SHORTCUTS_CANDIDATE_BUBBLE_RENDER_PROOF
  os/screenshots/hardware-gate/$arch/<date>/$TEXT_SHORTCUTS_LIVE_IBUS_RUNTIME_RENDER_PROOF
  os/screenshots/hardware-gate/$arch/<date>/$KEYBOARD_SHORTCUTS_ROUNDTRIP_PROOF
  os/screenshots/hardware-gate/$arch/<date>/$INPUT_SOURCES_ROUNDTRIP_PROOF
  os/screenshots/hardware-gate/$arch/<date>/$MULTI_DISPLAY_APPLY_PROOF
  os/screenshots/hardware-gate/$arch/<date>/$FOCUS_ARM_ROUNDTRIP_PROOF
  os/screenshots/hardware-gate/$arch/<date>/$APP_PRIVACY_REVOKE_PROOF
  os/screenshots/hardware-gate/$arch/<date>/$PREVIEW_OPEN_RENDER_PROOF
  os/screenshots/hardware-gate/$arch/<date>/$AUDIO_OUTPUT_PROOF
  os/screenshots/hardware-gate/$arch/<date>/$RUNTIME_BUILD_PROOF
EOF
}

signoff_block_contains() {
  local block="$1"
  local pattern="$2"

  printf '%s\n' "$block" | rg -q "$pattern"
}

signoff_block_has_real_field() {
  local block="$1"
  local pattern="$2"
  local line

  line="$(printf '%s\n' "$block" | rg "$pattern" || true)"
  [ -n "$line" ] || return 1
  ! printf '%s\n' "$line" | rg -qi 'n/a|not provided|not configured|requires|external gate|not exercised|none|unknown|missing|no live engine'
}

signoff_block_required_proof_is_complete() {
  local block="$1"
  local arch="${2:-}"

  signoff_block_contains "$block" "^- Runner: .+" || return 1
  if [ -n "$arch" ]; then
    signoff_block_contains "$block" "^- Architecture: $arch$" || return 1
    signoff_block_contains "$block" "^- ISO: .*goblins-os-$arch\\.iso" || return 1
  else
    signoff_block_contains "$block" "^- Architecture: (aarch64|x86_64)$" || return 1
    signoff_block_contains "$block" "^- ISO: .*goblins-os-(aarch64|x86_64)\\.iso" || return 1
  fi
  signoff_block_contains "$block" "^- ISO SHA256: [a-fA-F0-9]{64}$" || return 1
  signoff_block_contains "$block" "goblins-os-verify --installed-root /" || return 1
  signoff_block_contains "$block" "^- Verify result \\(blocked=0\\): pass" || return 1
  signoff_block_contains "$block" "^- Self-test command: .+" || return 1
  signoff_block_contains "$block" "^- Self-test result: pass" || return 1
  signoff_block_contains "$block" "^- Release evidence/SBOM checked: yes" || return 1
  signoff_block_contains "$block" "^- Screenshot dir: .+" || return 1
  if [ -n "$arch" ]; then
    signoff_block_contains "$block" "^- Screenshot dir: .*os/screenshots/hardware-gate/$arch/[^[:space:]]+" || return 1
  else
    signoff_block_contains "$block" "^- Screenshot dir: .*os/screenshots/hardware-gate/(aarch64|x86_64)/[^[:space:]]+" || return 1
  fi
  signoff_block_contains "$block" "^- Screenshot dir: .*not provided|stale screenshot|stale for this ISO|No fresh .*screenshots|missing current screenshot proof" && return 1
  signoff_block_has_real_field "$block" "^  - mode: .+" || return 1
  signoff_block_has_real_field "$block" "^  - engine source: .+" || return 1
  signoff_block_has_real_field "$block" "^  - built artifact path/URL: .+" || return 1
  signoff_block_contains "$block" "^- Motion/interactions checked: yes" || return 1
  signoff_block_contains "$block" "^- Firewall live toggle checked: yes" || return 1
  signoff_block_contains "$block" "^- Text Shortcuts session enablement checked: yes" || return 1
  signoff_block_contains "$block" "^- Text Shortcuts live keystrokes checked: yes" || return 1
  signoff_block_contains "$block" "^- Text Shortcuts candidate metadata checked: yes" || return 1
  signoff_block_contains "$block" "^- Text Shortcuts overlay intent checked: yes" || return 1
  signoff_block_contains "$block" "^- Text Shortcuts candidate bubble frame checked: yes" || return 1
  signoff_block_contains "$block" "^- Text Shortcuts candidate bubble layout checked: yes" || return 1
  signoff_block_contains "$block" "^- Text Shortcuts candidate bubble render intent checked: yes" || return 1
  signoff_block_contains "$block" "^- Text Shortcuts candidate bubble render screenshot checked: yes" || return 1
  signoff_block_contains "$block" "^- Keyboard shortcuts roundtrip checked: yes" || return 1
  signoff_block_contains "$block" "^- Input sources roundtrip checked: yes" || return 1
  signoff_block_contains "$block" "^- Multi-display apply checked: yes" || return 1
  signoff_block_contains "$block" "^- Focus arm roundtrip checked: yes" || return 1
  signoff_block_contains "$block" "^- App privacy revoke checked: yes" || return 1
  signoff_block_contains "$block" "^- Preview open/render checked: yes" || return 1
  signoff_block_contains "$block" "^- Audio output checked: yes" || return 1
  signoff_block_contains "$block" "^- Gaming readiness checked: yes" || return 1
  signoff_block_contains "$block" "^- Install storage/bootloader/dual-boot checked: yes" || return 1
  return 0
}

signoff_block_from_line() {
  local start="$1"

  awk -v start="$start" 'NR < start { next } NR == start { print; next } /^## / { exit } { print }' "$SIGNOFF"
}

signoff_run_for_arch_is_complete() {
  local arch="$1"
  local start block

  [ -f "$SIGNOFF" ] || return 1
  while IFS= read -r start; do
    block="$(signoff_block_from_line "$start")"

    signoff_block_required_proof_is_complete "$block" "$arch" || continue
    signoff_block_contains "$block" "^- Current project completion status: complete$" || continue
    return 0
  done < <(rg -n "^## Manual Gate Run:" "$SIGNOFF" | cut -d: -f1)

  return 1
}

echo "# Shipping status check"
echo

check "SHIP.md declares Fedora bootc foundation" "rg -q 'Fedora bootc remains the OS foundation' \"$SHIP_DECL\""
check "SHIP.md declares no custom kernel ownership" "rg -q 'no custom kernel|custom kernel' \"$SHIP_DECL\""
check "SHIP.md declares OpenAI Sans not used" "rg -q 'OpenAI Sans' \"$SHIP_DECL\""
check "No OpenAI Sans references outside SHIP" "rg -qi --hidden --no-ignore-vcs --no-ignore 'OpenAI Sans|openai sans|openai-sans' os .github --glob '!os/hardware-gate/verify-shipping-status.sh' --glob '!os/hardware-gate/close-signoff.sh' --glob '!os/iso/output*/**' --glob '!os/signoff-proofs/**' --glob '!os/screenshots/**' --glob '!os/brand/*.png' --glob '!SHIP.md' > /tmp/openai_sans_check.txt; [ ! -s /tmp/openai_sans_check.txt ]"
check "No typography licensing TODOs in signing docs" "! rg -qi 'licensing\s+TODO|TODO.*licensing' \"$SHIP_DECL\" \"$RUNBOOK\" \"$SIGNOFF\""
check "Source package secret scan finds no live keys" "source_secret_scan"
check "Generated artifact/evidence secret scan finds no live keys" "goblins_os_artifact_secret_scan \"$ROOT\""
check "installed-root verifier enforces secret file and directory modes" "rg -q 'installed-openai-secret-file-mode-0600' crates/goblins-os-verify/src/main.rs && rg -q 'installed-openai-secret-file-owner-root' crates/goblins-os-verify/src/main.rs && rg -q 'installed-openai-secret-file-empty' crates/goblins-os-verify/src/main.rs && rg -q 'var/lib/goblins-os/secrets/openai' crates/goblins-os-verify/src/main.rs"
check "OpenAI account credential is confined to the goblins-os service user" "rg -q 'codex-home-owner-only-0700' crates/goblins-os-verify/src/main.rs && rg -q 'codex-login-user-not-in-service-group' crates/goblins-os-verify/src/main.rs && ! rg -q 'usermod -aG goblins-os goblin' os/bootc/Containerfile && rg -q 'd /var/lib/goblins-os/codex 0700 goblins-os goblins-os' os/tmpfiles/goblins-os-codex.conf"
check "hosted OpenAI direct path uses Responses API" "rg -q '/v1/responses' crates/goblins-os-core/src/resident.rs && ! rg -q '/v1/chat.?completions' crates/goblins-os-core/src/resident.rs"
check "OpenAI SDK bridge endpoints stay server-side" "rg -q 'GOBLINS_OS_AGENTS_SDK_RELAY_URL' os/etc/goblins-os/openai-secrets.env && rg -q 'GOBLINS_OS_CHATKIT_RELAY_URL' os/etc/goblins-os/openai-secrets.env && rg -q 'GOBLINS_OS_REALTIME_RELAY_URL' os/etc/goblins-os/openai-secrets.env && rg -q 'GOBLINS_OS_IMAGES_RELAY_URL' os/etc/goblins-os/openai-secrets.env && ! rg -q 'OPENAI_OS_' os/etc/goblins-os/openai-secrets.env && rg -q 'Official OpenAI Agents SDK' crates/goblins-os-core/src/service_catalog.rs && ! rg -q 'pub struct OpenAIService' crates/goblins-os-core/src/service_catalog.rs"
check "Build Studio uses official Agents SDK relay only server-side" "rg -q 'GOBLINS_OS_AGENTS_SDK_RELAY_URL' crates/goblins-os-core/src/app_builder.rs && rg -q 'official-openai-agents-sdk' crates/goblins-os-core/src/app_builder.rs && rg -q 'handoffs' crates/goblins-os-core/src/app_builder.rs && rg -q 'guardrails' crates/goblins-os-core/src/app_builder.rs && rg -q 'tracing' crates/goblins-os-core/src/app_builder.rs && rg -q 'sandbox-execution' crates/goblins-os-core/src/app_builder.rs && rg -q 'Build Studio never receives raw API keys' crates/goblins-os-core/src/service_catalog.rs && ! rg -q 'OpenAI-centered Linux OS' crates/goblins-os-core/src/app_builder.rs"
check "Codex local chat wire is loopback-only compatibility" "rg -q 'This compatibility wire is local-only' os/codex/config.toml && rg -q 'base_url = \"http://127.0.0.1:11434/v1\"' os/codex/config.toml && rg -q 'wire_api = \"chat\"' os/codex/config.toml"
check "core URL env contract ships Goblins-native names with reader-side compatibility only" "rg -Fq 'GOBLINS_OS_CORE_URL=http://127.0.0.1:8787' os/etc/goblins-os/environment && rg -Fq 'GOBLINS_OS_CORE_PORT=8787' os/etc/goblins-os/environment && ! rg -Fq 'OPENAI_OS_' os/etc/goblins-os/environment && rg -Fq 'export GOBLINS_OS_CORE_URL=\"\${GOBLINS_OS_CORE_URL:-\${OPENAI_OS_CORE_URL:-http://127.0.0.1:8787}}\"' os/session/goblins-os-session && ! rg -Fq 'export OPENAI_OS_CORE_URL=' os/session/goblins-os-session && rg -Fq 'std::env::var(\"GOBLINS_OS_CORE_PORT\")' crates/goblins-os-core/src/main.rs && rg -Fq 'std::env::var(\"OPENAI_OS_CORE_PORT\")' crates/goblins-os-core/src/main.rs && rg -Fq 'GOBLINS_OS_CORE_URL must be a local http endpoint' crates/goblins-os-open/src/main.rs"
check "session bridge is source-gated for desktop user operations" "rg -Fq 'GOBLINS_OS_SESSION_BRIDGE_SOCKET=/run/goblins-os-session/session-bridge.sock' os/etc/goblins-os/environment && rg -Fq 'COPY os/tmpfiles/goblins-os-session.conf /usr/lib/tmpfiles.d/goblins-os-session.conf' os/bootc/Containerfile && rg -Fq 'd /run/goblins-os-session 0770 goblin goblins-session-bridge -' os/tmpfiles/goblins-os-session.conf && rg -Fq 'goblins-os-session-bridge' os/bootc/Containerfile && rg -Fq 'COPY --from=rust-build /out/ /' os/bootc/Containerfile && rg -Fq 'COPY --from=os-assets / /' os/bootc/Containerfile && rg -Fq 'goblins-os-session-bridge --self-test' os/bootc/Containerfile && rg -Fq 'groupadd --system goblins-session-bridge' os/bootc/Containerfile && rg -Fq 'SupplementaryGroups=goblins-session-bridge' os/systemd/goblins-os-core.service && rg -Fq 'ExecStart=/usr/libexec/goblins-os/goblins-os-session-bridge' os/systemd-user/org.goblins.OS.SessionBridge.service && rg -Fq 'Wants=org.goblins.OS.SessionBridge.service' os/systemd-user/gnome-session@goblins-os.target.d/goblins-os.session.conf && rg -Fq 'Wants=org.goblins.OS.Shell.service' os/systemd-user/gnome-session@goblins-os.target.d/goblins-os.session.conf && ! rg -Fq 'Requires=org.goblins.OS.Shell.target' os/systemd-user/gnome-session@goblins-os.target.d/goblins-os.session.conf && ! rg -Fq 'org.goblins.OS.Shell' os/gnome-session/goblins-os.session && rg -Fq 'UnixStream::connect' crates/goblins-os-core/src/session_bridge.rs && rg -Fq 'permission_store_delete_permission' crates/goblins-os-core/src/session_bridge.rs && rg -Fq 'display_config_apply_monitors' crates/goblins-os-core/src/session_bridge.rs && rg -Fq 'display_config_get_current_state' crates/goblins-os-core/src/displays.rs && rg -Fq 'non-allowlisted schema was accepted' crates/goblins-os-session-bridge/src/main.rs && rg -Fq 'PermissionStoreDelete' crates/goblins-os-session-bridge/src/main.rs && rg -Fq 'PermissionStore deletes are limited to app-keyed tables' crates/goblins-os-session-bridge/src/main.rs && rg -Fq 'DisplayConfigApplyMonitors' crates/goblins-os-session-bridge/src/main.rs && rg -Fq 'validate_display_config_logical_monitors' crates/goblins-os-session-bridge/src/main.rs"
check "shell user service does not directly export legacy core URL" "! rg -q 'Environment=OPENAI_OS_CORE_URL' os/systemd-user/org.goblins.OS.Shell.service"
check "desktop clients prefer GOBLINS_OS_CORE_URL over legacy alias" "rg -Fq 'env::var(\"GOBLINS_OS_CORE_URL\")' crates/goblins-os-installer/src/main.rs crates/goblins-os-login/src/main.rs crates/goblins-os-shell/src/main.rs crates/goblins-os-settings/src/main.rs crates/goblins-os-launcher/src/main.rs crates/goblins-os-control-center/src/main.rs crates/goblins-os-open/src/main.rs crates/goblins-os-file-builder/src/main.rs crates/goblins-os-resident/src/main.rs && rg -Fq 'env::var(\"OPENAI_OS_CORE_URL\")' crates/goblins-os-installer/src/main.rs crates/goblins-os-login/src/main.rs crates/goblins-os-shell/src/main.rs crates/goblins-os-settings/src/main.rs crates/goblins-os-launcher/src/main.rs crates/goblins-os-control-center/src/main.rs crates/goblins-os-open/src/main.rs crates/goblins-os-file-builder/src/main.rs crates/goblins-os-resident/src/main.rs"
check "installer proof page override bypasses completed first-boot exit" "rg -Fq 'should_exit_after_first_boot(first_boot_completed, installer_page_override_requested())' crates/goblins-os-installer/src/main.rs && rg -Fq 'GOBLINS_OS_INSTALLER_PAGE' crates/goblins-os-installer/src/main.rs"

check "rust job checks fmt" "rg -q 'cargo fmt --all --check' \"$WORKFLOW\""
check "rust job checks clippy" "rg -q 'clippy --workspace' \"$WORKFLOW\""
check "rust job checks native desktop tests" 'rg -q --fixed-strings '\''cargo test --workspace --features "$NATIVE_FEATURES"'\'' "$WORKFLOW"'
check "rust job checks release" "rg -q 'cargo build --release --workspace' \"$WORKFLOW\""
check "image job has verify" "rg -q 'goblins-os-verify' \"$WORKFLOW\""
check "image job checks blocked=0" "rg -q 'blocked=0' \"$WORKFLOW\""
check "image job has selftest" "rg -q 'selftest.suffix.Dockerfile' \"$WORKFLOW\" && rg -q 'target: selftest' \"$WORKFLOW\""
check "image job renders settings interaction proof" "rg -Fq 'GOBLINS_OS_RENDER_SCOPE=settings-interactions' \"$WORKFLOW\" && rg -Fq 'goblins-os-settings-interactions-' \"$WORKFLOW\""
check "image job has explicit push marker trigger" "rg -Fq \"contains(github.event.head_commit.message, '[image]')\" \"$WORKFLOW\" && rg -Fq \"github.event_name == 'push' && contains(github.event.head_commit.message, '[image]')\" \"$WORKFLOW\""
check "CI suffixes avoid extra chmod run layers" "rg -Fq 'COPY --chmod=0755 os/bootc/run-selftest.sh' os/bootc/selftest.suffix.Dockerfile && ! rg -Fq 'RUN chmod +x /usr/local/bin/run-selftest.sh' os/bootc/selftest.suffix.Dockerfile && rg -Fq 'COPY --chmod=0755 os/bootc/render-screens.sh' os/bootc/render.suffix.Dockerfile && ! rg -Fq 'RUN chmod +x /usr/local/bin/render-screens.sh' os/bootc/render.suffix.Dockerfile && rg -Fq 'COPY --chmod=0755 os/bootc/render-desktop.sh' os/bootc/render-desktop.suffix.Dockerfile && ! rg -Fq 'RUN chmod +x /usr/local/bin/render-desktop.sh' os/bootc/render-desktop.suffix.Dockerfile"
check "installer-iso job exists" "rg -q '^  installer-iso:' \"$WORKFLOW\""
check "installer-iso job generates release evidence" "rg -q -- '--release-evidence /out' \"$WORKFLOW\" && rg -q 'rpm-packages.command' \"$WORKFLOW\""
check "installer-iso job scans generated evidence for secrets" "rg -q 'goblins_os_artifact_secret_scan' \"$WORKFLOW\""
check "installer-iso job uploads release evidence artifacts" "rg -q 'goblins-os-release-evidence-' \"$WORKFLOW\""
check "workflow declares aarch64 runner" "rg -q 'ubuntu-24.04-arm|aarch64' \"$WORKFLOW\""
check "workflow declares x86_64 runner" "rg -q 'ubuntu-24.04|x86_64' \"$WORKFLOW\""
check "workflow asserts native runner architecture" "rg -q --fixed-strings 'Assert native runner architecture' \"$WORKFLOW\" && rg -q --fixed-strings 'test \"\$(uname -m)\" = \"\${{ matrix.expected_uname }}\"' \"$WORKFLOW\" && rg -q --fixed-strings 'expected_uname: aarch64' \"$WORKFLOW\" && rg -q --fixed-strings 'expected_uname: x86_64' \"$WORKFLOW\""

check "architecture contract records aarch64 artifact paths" "rg -q 'os/iso/output/aarch64/bootiso/goblins-os-aarch64\\.iso' os/release/architectures.toml && rg -q 'os/iso/output/aarch64/manifest-goblins-os-aarch64\\.json' os/release/architectures.toml"
check "architecture contract records x86_64 artifact paths" "rg -q 'os/iso/output/x86_64/bootiso/goblins-os-x86_64\\.iso' os/release/architectures.toml && rg -q 'os/iso/output/x86_64/manifest-goblins-os-x86_64\\.json' os/release/architectures.toml"
check "architecture contract records per-architecture SBOM paths" "rg -q 'os/signoff-proofs/sbom/aarch64/rpm-packages\\.tsv' os/release/architectures.toml && rg -q 'os/signoff-proofs/sbom/x86_64/rpm-packages\\.tsv' os/release/architectures.toml"
check "architecture contract records per-architecture QEMU commands" "rg -q 'qemu-system-aarch64' os/release/architectures.toml && rg -q 'qemu-system-x86_64' os/release/architectures.toml"
check "architecture contract records aarch64 UEFI pflash contract" "rg -q 'virt,accel=kvm,gic-version=max' os/release/architectures.toml && rg -q 'AARCH64_UEFI_CODE' os/release/architectures.toml && rg -q 'AARCH64_UEFI_VARS' os/release/architectures.toml"
check "architecture contract records native KVM proof" "rg -q 'qemu_accel = \"kvm\"' os/release/architectures.toml"
check "architecture contract rejects aarch64 emulation baseline" "rg -q 'do not use x86_64 emulation as baseline' os/release/architectures.toml"

check "ISO builder supports GOBLINS_OS_ARCH" "rg -q 'GOBLINS_OS_ARCH' os/iso/build-iso.sh"
check "ISO builder writes architecture ISO names" "rg -q 'goblins-os-\\\$ARCH.iso' os/iso/build-iso.sh"
check "ISO builder host runtime is Docker-only" "rg -q \"expected docker\" os/iso/build-iso.sh && ! rg -q 'docker or podman' os/iso/build-iso.sh && ! rg -q 'GOBLINS_OS_PODMAN_SUDO' os/iso/build-iso.sh && ! rg -q 'run_podman_builder' os/iso/build-iso.sh"
check "ISO builder uses Docker local registry handoff" "rg -q 'GOBLINS_OS_CONTAINER_RUNTIME' os/iso/build-iso.sh && rg -q 'host.docker.internal' os/iso/build-iso.sh && rg -q 'docker push' os/iso/build-iso.sh && ! rg -q -- '--rm -it' os/iso/build-iso.sh"
check "ISO builder separates local Docker handoff from shippable release source" "rg -q 'GOBLINS_OS_BIB_SOURCE_IMAGE' os/iso/build-iso.sh && rg -q 'GOBLINS_OS_SHIPPABLE_RELEASE' os/iso/build-iso.sh && rg -q 'shippable release media cannot track local/test-only installer payload ref' os/iso/build-iso.sh"
check "ISO builder can skip local image export for shippable registry source" "rg -q 'GOBLINS_OS_SKIP_LOCAL_IMAGE_BUILD' os/iso/build-iso.sh && rg -q 'Skipping local Docker image build' os/iso/build-iso.sh && rg -q 'requires GOBLINS_OS_BIB_SOURCE_IMAGE' os/iso/build-iso.sh"
check "ISO builder supports explicit installer config" "rg -q 'GOBLINS_OS_ISO_CONFIG' os/iso/build-iso.sh && rg -q '\"installer_config\": \"[$]CONFIG_LABEL\"' os/iso/build-iso.sh"
check "ISO builder supports explicit Docker platform for non-release artifact testing" "rg -q 'GOBLINS_OS_DOCKER_PLATFORM' os/iso/build-iso.sh && rg -q 'docker build --platform \"[$]DOCKER_PLATFORM\"' os/iso/build-iso.sh && rg -q -- '--platform \"[$]DOCKER_PLATFORM\"' os/iso/build-iso.sh && rg -q '\"docker_platform\": \"[$]DOCKER_PLATFORM\"' os/iso/build-iso.sh"
check "ISO builder fails fast when Docker emulation cannot run rustc" "rg -q 'verify_docker_emulation_runtime' os/iso/build-iso.sh && rg -q 'emulation cannot run rustc' os/iso/build-iso.sh && rg -q 'use a native [$]ARCH runner' os/iso/build-iso.sh"
check "workflow installer ISO uses cached Buildx image and evidence steps" "rg -q 'docker/build-push-action@v7' \"$WORKFLOW\" && rg -q 'load: true' \"$WORKFLOW\" && rg -q 'docker run --rm' \"$WORKFLOW\" && rg -q 'GOBLINS_OS_CONTAINER_RUNTIME=docker' \"$WORKFLOW\""
check "workflow image builds use nonblocking BuildKit GHA cache" "rg -q 'docker/setup-buildx-action@v3' \"$WORKFLOW\" && rg -q --fixed-strings 'type=gha,scope=goblins-os-bootc-\${{ matrix.arch }}' \"$WORKFLOW\" && rg -q 'mode=max,ignore-error=true' \"$WORKFLOW\""
check "hardware gate pushes bootc image without daemon export" "rg -q 'docker/build-push-action@v7' .github/workflows/hardware-gate-capture.yml && rg -q 'push: true' .github/workflows/hardware-gate-capture.yml && rg -q 'GOBLINS_OS_SKIP_LOCAL_IMAGE_BUILD=1' .github/workflows/hardware-gate-capture.yml && ! rg -q 'docker build -f os/bootc/Containerfile -t localhost/goblins-os' .github/workflows/hardware-gate-capture.yml"
check "hardware gate uses verification ISO config" "rg -q 'GOBLINS_OS_ISO_CONFIG=os/iso/verify-config.toml' .github/workflows/hardware-gate-capture.yml"
check "hardware gate uses nonblocking BuildKit GHA cache and cancels superseded runs" "rg -q 'docker/setup-buildx-action@v3' .github/workflows/hardware-gate-capture.yml && rg -q --fixed-strings 'type=gha,scope=goblins-os-bootc-\${{ matrix.arch }}' .github/workflows/hardware-gate-capture.yml && rg -q 'mode=max,ignore-error=true' .github/workflows/hardware-gate-capture.yml && rg -q 'cancel-in-progress: true' .github/workflows/hardware-gate-capture.yml"
check "hardware gate prepares readable writable KVM for qemu" "rg -q 'sudo chmod a[+]rw /dev/kvm' .github/workflows/hardware-gate-capture.yml && rg -q 'test -r /dev/kvm && test -w /dev/kvm' .github/workflows/hardware-gate-capture.yml"
check "hardware gate installs close-signoff search dependency" "rg -q 'ripgrep' .github/workflows/hardware-gate-capture.yml && rg -q '\\brg -q\\b' os/hardware-gate/close-signoff.sh"
check "external gate supports qemu-system-aarch64" "rg -q 'qemu-system-aarch64' os/hardware-gate/run-external-gate.sh"
check "external gate supports qemu-system-x86_64" "rg -q 'qemu-system-x86_64' os/hardware-gate/run-external-gate.sh"
check "external gate passes container runtime to ISO builder" "rg -q 'GOBLINS_OS_CONTAINER_RUNTIME=\"[$]CONTAINER_RUNTIME\"' os/hardware-gate/run-external-gate.sh"
check "external gate host runtime is Docker-only" "rg -q 'GOBLINS_OS_CONTAINER_RUNTIME must be docker' os/hardware-gate/run-external-gate.sh && ! rg -q 'docker or podman' os/hardware-gate/run-external-gate.sh && ! rg -q 'GOBLINS_OS_PODMAN_SUDO' os/hardware-gate/run-external-gate.sh && ! rg -q 'sudo podman' os/hardware-gate/run-external-gate.sh"
check "external gate requires real bootc source image for display proof" "rg -q 'Display-backed shipping proof requires GOBLINS_OS_BIB_SOURCE_IMAGE' os/hardware-gate/run-external-gate.sh && rg -q 'GOBLINS_OS_BIB_SOURCE_IMAGE=\"[$]BIB_SOURCE_IMAGE\"' os/hardware-gate/run-external-gate.sh && rg -q 'GOBLINS_OS_SHIPPABLE_RELEASE=\"[$]SHIPPABLE_RELEASE\"' os/hardware-gate/run-external-gate.sh"
check "runbook documents real release image source" "rg -q 'RELEASE_IMAGE=<registry>/<namespace>/goblins-os:[$]ARCH' os/hardware-gate/runbook.md && rg -q '\"installer_payload_source_local_only\": false' os/hardware-gate/runbook.md"
check "external gate requires native KVM acceleration" "rg -q 'QEMU_ACCEL must be kvm' os/hardware-gate/run-external-gate.sh && rg -q '/dev/kvm' os/hardware-gate/run-external-gate.sh"
check "external gate uses aarch64 UEFI pflash code and vars" "rg -q 'if=pflash,format=raw,readonly=on,file=[$]AARCH64_UEFI_CODE' os/hardware-gate/run-external-gate.sh && rg -q 'if=pflash,format=raw,file=[$]AARCH64_UEFI_VARS' os/hardware-gate/run-external-gate.sh"
check "external gate copies aarch64 UEFI vars template" "rg -q 'AARCH64_UEFI_VARS_TEMPLATE' os/hardware-gate/run-external-gate.sh && rg -q 'cp \"[$]template\" \"[$]AARCH64_UEFI_VARS\"' os/hardware-gate/run-external-gate.sh"
check "external gate requires Linux host before display proof" "rg -q 'External display-backed gate requires a native Linux host with Docker and QEMU' os/hardware-gate/run-external-gate.sh"
check "external gate fails non-native architecture before build" "rg -q 'Requested [$]ARCH gate on [$]HOST_ARCH host' os/hardware-gate/run-external-gate.sh && rg -q 'must be produced on a native [$]ARCH Linux runner' os/hardware-gate/run-external-gate.sh"
check "external gate allows explicit Docker emulation for artifact testing only" "rg -q 'GOBLINS_OS_ALLOW_EMULATED_DOCKER' os/hardware-gate/run-external-gate.sh && rg -q 'Docker-emulated [$]ARCH artifact testing' os/hardware-gate/run-external-gate.sh && rg -q 'not release proof' os/hardware-gate/run-external-gate.sh && rg -q 'Docker artifact testing on a non-native machine' os/hardware-gate/runbook.md"
check "external gate fails low disk before build" "rg -q 'MIN_HOST_FREE_GB' os/hardware-gate/run-external-gate.sh && rg -q 'Repository filesystem needs at least' os/hardware-gate/run-external-gate.sh && rg -q 'VM scratch filesystem needs at least' os/hardware-gate/run-external-gate.sh"
check "external gate checks container runtime health before build" "rg -q 'CONTAINER_RUNTIME_HEALTH_TIMEOUT_SECS' os/hardware-gate/run-external-gate.sh && rg -q 'Checking [$]CONTAINER_RUNTIME health' os/hardware-gate/run-external-gate.sh && rg -q 'did not answer within' os/hardware-gate/run-external-gate.sh"
check "external gate has fail-closed preflight-only mode" "rg -q 'PREFLIGHT_ONLY=1' os/hardware-gate/run-external-gate.sh && rg -q 'Preflight passed for native [$]ARCH release runner' os/hardware-gate/run-external-gate.sh && rg -q 'Docker artifact-only preflight passed for [$]ARCH on [$]HOST_ARCH; not release proof' os/hardware-gate/run-external-gate.sh && rg -q 'No image, ISO, SBOM, screenshot, or signoff artifact was generated' os/hardware-gate/run-external-gate.sh"
check "runbook documents external preflight command" "rg -q 'PREFLIGHT_ONLY=1 GOBLINS_OS_ARCH' os/hardware-gate/runbook.md && rg -q 'does not create shipping artifacts or satisfy proof by itself' os/hardware-gate/runbook.md"
check "external gate allows artifact-only mode without pretending proof is complete" "rg -q 'RUN_QEMU=0: built and verified artifacts only' os/hardware-gate/run-external-gate.sh"
check "external gate verifies ISO SHA256" "rg -q 'sha256sum -c' os/hardware-gate/run-external-gate.sh"
check "external gate generates release evidence" "rg -q -- '--release-evidence /out' os/hardware-gate/run-external-gate.sh"
check "external gate requires RPM SBOM TSV" "rg -q 'rpm-packages.tsv' os/hardware-gate/run-external-gate.sh"
check "installer policy exposes dual-boot preservation path" "rg -q 'dual_boot_preservation' crates/goblins-os-core/src/install_targets.rs"
check "installer policy exposes dual-boot preflight" "rg -q 'dual_boot_preflight' crates/goblins-os-core/src/install_targets.rs"
check "installer policy exposes safe dual-boot route" "rg -q 'dual_boot_safe_route' crates/goblins-os-core/src/install_targets.rs && rg -q 'DualBootSafeRoute' crates/goblins-os-core/src/install_targets.rs && rg -q 'Install beside an existing OS' crates/goblins-os-core/src/install_targets.rs && rg -q 'Install Goblins OS Beside Another OS' crates/goblins-os-core/src/install_targets.rs && rg -q 'every filesystem that will be formatted' crates/goblins-os-core/src/install_targets.rs"
check "installer policy exposes simple install erase scope" "rg -q 'simple_install_scope' crates/goblins-os-core/src/install_targets.rs && rg -q 'blank internal disk' crates/goblins-os-core/src/install_targets.rs && rg -q 'formats the new Goblins OS root filesystem' crates/goblins-os-core/src/install_targets.rs"
check "installer policy exposes bootloader recovery guidance" "rg -q 'bootloader_recovery' crates/goblins-os-core/src/install_targets.rs && rg -q 'firmware boot options' crates/goblins-os-core/src/install_targets.rs"
check "installer policy exposes storage review checklist" "rg -q 'storage_review_checklist' crates/goblins-os-core/src/install_targets.rs && rg -q 'StorageReviewItem' crates/goblins-os-core/src/install_targets.rs"
check "installer policy exposes recommended install path choices" "rg -q 'install_path_options' crates/goblins-os-core/src/install_targets.rs && rg -q 'Keep my current OS' crates/goblins-os-core/src/install_targets.rs && rg -q 'Replace one blank disk' crates/goblins-os-core/src/install_targets.rs && rg -q 'Advanced storage' crates/goblins-os-core/src/install_targets.rs"
check "installer policy exposes pre-write boot formatting plan" "rg -q 'pre_write_install_plan' crates/goblins-os-core/src/install_targets.rs && rg -q 'InstallPlanItem' crates/goblins-os-core/src/install_targets.rs && rg -q 'fresh GPT layout' crates/goblins-os-core/src/install_targets.rs && rg -q 'bootloader/EFI target' crates/goblins-os-core/src/install_targets.rs && rg -q 'xfs root' crates/goblins-os-core/src/install_targets.rs && rg -q 'TPM2 LUKS' crates/goblins-os-core/src/install_targets.rs"
check "installer policy exposes dual-boot readiness checklist" "rg -q 'dual_boot_readiness' crates/goblins-os-core/src/install_targets.rs && rg -q 'DualBootReadinessItem' crates/goblins-os-core/src/install_targets.rs && rg -q 'Windows readiness' crates/goblins-os-core/src/install_targets.rs && rg -q 'macOS readiness' crates/goblins-os-core/src/install_targets.rs && rg -q 'Linux readiness' crates/goblins-os-core/src/install_targets.rs && rg -q 'Other OS or data readiness' crates/goblins-os-core/src/install_targets.rs && rg -q 'Dedicated disk readiness' crates/goblins-os-core/src/install_targets.rs"
check "installer policy exposes dual-boot assistant choices" "rg -q 'dual_boot_choices' crates/goblins-os-core/src/install_targets.rs && rg -q 'DualBootChoice' crates/goblins-os-core/src/install_targets.rs && rg -q 'Keep Windows' crates/goblins-os-core/src/install_targets.rs && rg -q 'suspend BitLocker' crates/goblins-os-core/src/install_targets.rs && rg -q 'Keep macOS' crates/goblins-os-core/src/install_targets.rs && rg -q 'Keep Linux' crates/goblins-os-core/src/install_targets.rs && rg -q 'Keep another OS or data' crates/goblins-os-core/src/install_targets.rs && rg -q 'Use a dedicated disk' crates/goblins-os-core/src/install_targets.rs"
check "installer policy exposes guided dual-boot steps" "rg -q 'dual_boot_guide' crates/goblins-os-core/src/install_targets.rs && rg -q 'Disk Management' crates/goblins-os-core/src/install_targets.rs && rg -q 'Disk Utility' crates/goblins-os-core/src/install_targets.rs && rg -q 'Startup menu' crates/goblins-os-core/src/install_targets.rs && rg -q 'Final storage review' crates/goblins-os-core/src/install_targets.rs"
check "installer policy exposes dual-boot decision map" "rg -q 'dual_boot_decision_map' crates/goblins-os-core/src/install_targets.rs && rg -q 'DualBootDecision' crates/goblins-os-core/src/install_targets.rs && rg -q 'Windows beside Goblins OS' crates/goblins-os-core/src/install_targets.rs && rg -q 'macOS beside Goblins OS' crates/goblins-os-core/src/install_targets.rs && rg -q 'Linux beside Goblins OS' crates/goblins-os-core/src/install_targets.rs && rg -q 'Separate disk' crates/goblins-os-core/src/install_targets.rs"
check "installer policy exposes advanced storage handoff" "rg -q 'full_storage_installer' crates/goblins-os-core/src/install_targets.rs && rg -q '/usr/libexec/goblins-os/goblins-os-full-installer' crates/goblins-os-core/src/install_targets.rs && rg -q 'org.goblins.OS.FullInstaller.desktop' crates/goblins-os-core/src/install_targets.rs && rg -q 'Advanced storage' crates/goblins-os-core/src/install_targets.rs"
check "installer policy exposes dual-boot quick start" "rg -q 'dual_boot_quick_start' crates/goblins-os-core/src/install_targets.rs && rg -q 'Install beside another OS' crates/goblins-os-core/src/install_targets.rs && rg -q 'Confirm preserve, format, and bootloader' crates/goblins-os-core/src/install_targets.rs && rg -q 'Test every boot path' crates/goblins-os-core/src/install_targets.rs"
check "installer policy explains firmware startup picker" "rg -q 'firmware startup menu or boot picker' crates/goblins-os-core/src/install_targets.rs"
check "installer policy covers Windows macOS Linux and other OS" "rg -q 'Windows, macOS, Linux, or another OS' crates/goblins-os-core/src/install_targets.rs"
check "installer policy protects APFS and EFI partitions" "rg -q 'macOS/APFS, Linux, other OS, recovery, and EFI partitions' crates/goblins-os-core/src/install_targets.rs"
check "installer API explains blocked simple erase dual-boot handoff" "rg -q 'The simple erase flow will not install' crates/goblins-os-core/src/install_targets.rs && rg -q 'open advanced storage' crates/goblins-os-core/src/install_targets.rs && rg -q 'select only unallocated free space' crates/goblins-os-core/src/install_targets.rs"
check "installer scanner detects BitLocker Microsoft Reserved Apple HFS and Linux filesystems" "rg -q 'bitlocker' crates/goblins-os-core/src/install_targets.rs && rg -q 'e3c9e316-0b5c-4db8-817d-f92df00215ae' crates/goblins-os-core/src/install_targets.rs && rg -q '48465300-0000-11aa-aa11-00306543ecac' crates/goblins-os-core/src/install_targets.rs && rg -q 'f2fs' crates/goblins-os-core/src/install_targets.rs && rg -q 'bcachefs' crates/goblins-os-core/src/install_targets.rs"
check "installer scanner test covers Windows macOS Linux and data partitions" "rg -q 'scans_sys_block_and_routes_existing_operating_systems_to_manual_storage' crates/goblins-os-core/src/install_targets.rs && rg -q 'TYPE=ntfs' crates/goblins-os-core/src/install_targets.rs && rg -q 'TYPE=apfs' crates/goblins-os-core/src/install_targets.rs && rg -q 'TYPE=crypto_LUKS' crates/goblins-os-core/src/install_targets.rs && rg -q 'TYPE=zfs_member' crates/goblins-os-core/src/install_targets.rs"
check "installer render proof uses Docker fixture for Windows macOS Linux and data partitions" "rg -q 'TYPE=ntfs' os/bootc/render-screens.sh && rg -q 'TYPE=apfs' os/bootc/render-screens.sh && rg -q 'TYPE=crypto_LUKS' os/bootc/render-screens.sh && rg -q 'TYPE=zfs_member' os/bootc/render-screens.sh"
check "installer render proof captures full storage handoff screenshot" "rg -q 'Open advanced storage handoff' os/bootc/render-screens.sh && rg -q '27-dual-boot-preserve-existing-os\\.png' os/bootc/render-screens.sh"
check "desktop render proof documents Docker harness" "rg -q 'DOCKER_BUILDKIT=1 docker build' os/bootc/render-desktop.suffix.Dockerfile && ! rg -q 'podman build' os/bootc/render-desktop.suffix.Dockerfile"
check "render proofs do not use legacy demo or seeded app hooks" "rg -q 'GOBLINS_OS_RENDER_QUERY' os/bootc/render-screens.sh crates/goblins-os-launcher/src/main.rs && ! rg -q 'GOBLINS_OS_SHELL_DEMO|GOBLINS_OS_LAUNCHER_DEMO' os/bootc/render-screens.sh crates/goblins-os-shell/src/main.rs crates/goblins-os-launcher/src/main.rs && ! rg -q 'Render/design proof: seed' crates/goblins-os-launcher/src/main.rs"
check "installer UI shows best path for dual boot" "rg -q 'Best dual-boot path' crates/goblins-os-installer/src/main.rs"
check "installer UI shows simple path choice before disk erase" "rg -q 'Choose install path' crates/goblins-os-installer/src/main.rs && rg -q 'Replace one blank disk' crates/goblins-os-installer/src/main.rs"
check "installer UI makes dual boot the first storage choice" "rg -q 'Keeping another OS or data?' crates/goblins-os-installer/src/main.rs && rg -q 'start with advanced storage' crates/goblins-os-installer/src/main.rs"
check "installer UI renders recommended install paths" "rg -q 'append_install_path_options' crates/goblins-os-installer/src/main.rs && rg -q 'Recommended install paths' crates/goblins-os-installer/src/main.rs && rg -q 'install_path_options_summary' crates/goblins-os-installer/src/main.rs"
check "installer UI renders pre-write boot formatting plan" "rg -q 'append_pre_write_install_plan' crates/goblins-os-installer/src/main.rs && rg -q 'Before writing to disk' crates/goblins-os-installer/src/main.rs && rg -q 'pre_write_install_plan_summary' crates/goblins-os-installer/src/main.rs && rg -q 'dual boot and custom formatting stay in advanced storage' crates/goblins-os-installer/src/main.rs"
check "installer UI renders dual-boot quick start" "rg -q 'append_dual_boot_quick_start' crates/goblins-os-installer/src/main.rs && rg -q 'Dual-boot quick start' crates/goblins-os-installer/src/main.rs && rg -q 'final preserve, format, and bootloader summary' crates/goblins-os-installer/src/main.rs && rg -q 'dual_boot_quick_start_summary' crates/goblins-os-installer/src/main.rs"
check "installer UI renders dual-boot readiness checklist" "rg -q 'append_dual_boot_readiness' crates/goblins-os-installer/src/main.rs && rg -q 'Dual-boot readiness' crates/goblins-os-installer/src/main.rs && rg -q 'Use this checklist before writing storage changes' crates/goblins-os-installer/src/main.rs && rg -q 'dual_boot_readiness_summary' crates/goblins-os-installer/src/main.rs"
check "installer UI renders dual-boot assistant choices" "rg -q 'append_dual_boot_choices' crates/goblins-os-installer/src/main.rs && rg -q 'Dual-boot assistant' crates/goblins-os-installer/src/main.rs && rg -q 'Pick the operating system you are keeping' crates/goblins-os-installer/src/main.rs && rg -q 'dual_boot_choices_summary' crates/goblins-os-installer/src/main.rs"
check "installer UI renders dual-boot decision map" "rg -q 'append_dual_boot_decision_map' crates/goblins-os-installer/src/main.rs && rg -q 'Dual-boot decision map' crates/goblins-os-installer/src/main.rs && rg -q 'Best for:' crates/goblins-os-installer/src/main.rs && rg -q 'dual_boot_decision_map_summary' crates/goblins-os-installer/src/main.rs"
check "installer UI renders safe dual-boot route" "rg -q 'append_dual_boot_safe_route' crates/goblins-os-installer/src/main.rs && rg -q 'dual_boot_safe_route_summary' crates/goblins-os-installer/src/main.rs && rg -q 'Install beside an existing OS' crates/goblins-os-installer/src/main.rs && rg -q 'installer_dual_boot_safe_route_launch_error' crates/goblins-os-installer/src/main.rs"
check "installer UI exposes advanced storage button" "rg -q 'append_full_storage_installer_handoff' crates/goblins-os-installer/src/main.rs && rg -q 'Open advanced storage' crates/goblins-os-installer/src/main.rs && rg -q 'launch_full_storage_installer' crates/goblins-os-installer/src/main.rs && rg -q 'StorageInstallerCommand' crates/goblins-os-installer/src/main.rs"
check "installer UI turns detected existing OS disks into preservation actions" "rg -q 'Detected systems are actions' crates/goblins-os-installer/src/main.rs && rg -q 'Open advanced storage from detected disk' crates/goblins-os-installer/src/main.rs && rg -q 'installer_detected_disk_full_storage_launch_error' crates/goblins-os-installer/src/main.rs && rg -q 'row.set_sensitive(target.eligible || preservation_handoff)' crates/goblins-os-installer/src/main.rs"
check "installer wizard labels are title case and not shouted" "rg -q 'Step 1 of 3 · Choose disk' crates/goblins-os-installer/src/main.rs && rg -q 'Step 3 of 3 · Confirm' crates/goblins-os-installer/src/main.rs && rg -q 'Required Confirmation' crates/goblins-os-installer/src/main.rs && rg -q '.gos-onboarding-kicker' crates/goblins-os-design/src/lib.rs && rg -q 'text-transform: none;' crates/goblins-os-design/src/lib.rs && ! rg -q 'STEP ·|FINAL STEP|REQUIRED CONFIRMATION|WHAT HAPPENED|letter-spacing: 2\\.2px' crates/goblins-os-installer/src/main.rs crates/goblins-os-design/src/lib.rs"
check "installer UI shows detected OS preservation checklist" "rg -q 'Preservation checklist:' crates/goblins-os-installer/src/main.rs && rg -q 'Back up and save recovery keys' crates/goblins-os-installer/src/main.rs && rg -q 'detected_system_preparation_hint' crates/goblins-os-installer/src/main.rs && rg -q 'test every preserved system from the firmware boot picker' crates/goblins-os-installer/src/main.rs"
check "installer UI exposes guided install-beside launcher" "rg -q 'append_dual_boot_launcher' crates/goblins-os-installer/src/main.rs && rg -q 'Install beside another OS' crates/goblins-os-installer/src/main.rs && rg -q 'What are you keeping?' crates/goblins-os-installer/src/main.rs && rg -q 'installer_dual_boot_choice_launch_error' crates/goblins-os-installer/src/main.rs && rg -q '.gos-dual-boot-choice' crates/goblins-os-design/src/lib.rs"
check "installer UI shows erase scope and boot recovery" "rg -q 'Simple install scope' crates/goblins-os-installer/src/main.rs && rg -q 'Erase scope' crates/goblins-os-installer/src/main.rs && rg -q 'Startup recovery' crates/goblins-os-installer/src/main.rs && rg -q 'After reboot' crates/goblins-os-installer/src/main.rs"
check "installer UI renders storage review checklist" "rg -q 'append_storage_review_checklist' crates/goblins-os-installer/src/main.rs && rg -q 'Storage review checklist' crates/goblins-os-installer/src/main.rs"
check "installer UI renders guided dual-boot steps" "rg -q 'append_dual_boot_guide' crates/goblins-os-installer/src/main.rs && rg -q 'Dual-boot guide' crates/goblins-os-installer/src/main.rs"
check "installer UI labels keep existing OS path" "rg -q 'Keep an existing OS' crates/goblins-os-installer/src/main.rs"
check "installer network copy hides internal service wording" "rg -q 'The network service is not responding on this device' crates/goblins-os-installer/src/main.rs && rg -q 'Networking not ready' crates/goblins-os-installer/src/main.rs && ! rg -q 'NetworkManager isn.t responding|Networking unavailable' crates/goblins-os-installer/src/main.rs"
check "installer copy hides bootc and Anaconda implementation labels" "rg -q 'Install readiness' crates/goblins-os-installer/src/main.rs && ! rg -q 'Installer engine' crates/goblins-os-installer/src/main.rs && ! rg -q 'bootc installer' crates/goblins-os-installer/src/main.rs && ! rg -q 'bootc install command' crates/goblins-os-installer/src/main.rs && ! rg -q 'Fedora/Anaconda' crates/goblins-os-installer/src/bin/goblins-os-full-installer.rs && ! rg -q 'Anaconda;' os/applications/org.goblins.OS.FullInstaller.desktop"
check "native design system uses Goblins-native naming" "rg -q 'GOBLINS_NATIVE_CSS' crates/goblins-os-design/src/lib.rs && ! rg -q -e 'OPENAI_NATIVE_CSS' -e 'OpenAI-native' crates/goblins-os-design/src/lib.rs crates/goblins-os-installer/src/main.rs crates/goblins-os-login/src/main.rs crates/goblins-os-launcher/src/main.rs crates/goblins-os-control-center/src/main.rs os/plymouth/goblins-os/goblins-os.script os/iso/config.toml"
check "boot splash uses Goblins mark for OS identity" "rg -q 'brand/anaconda/sidebar-logo.png' os/bootc/Containerfile && rg -q 'Goblins OS boot splash.*Goblins mark' os/plymouth/goblins-os/goblins-os.plymouth && ! rg -q 'brand/OpenAI-white-monoblossom.png[[:space:]]*\\\\' os/bootc/Containerfile"
check "installer and login product copy uses Goblins desktop naming" "rg -q 'Goblins-native desktop' crates/goblins-os-installer/src/main.rs && rg -q 'Enter Goblins OS desktop' crates/goblins-os-installer/src/main.rs && rg -q 'Unlock Goblins OS desktop' crates/goblins-os-login/src/main.rs && rg -q 'Goblins OS desktop unlock was rejected by local OS services' crates/goblins-os-login/src/main.rs && ! rg -q -e 'OpenAI-native desktop' -e 'Enter OpenAI desktop' -e 'Unlock OpenAI desktop' -e 'OpenAI desktop unlock' crates/goblins-os-installer/src/main.rs crates/goblins-os-login/src/main.rs"
check "desktop metadata uses Goblins identity for OS surfaces" "rg -q 'Comment=Native Goblins OS identity gate' os/applications/org.goblins.OS.Login.desktop && rg -q 'Comment=Native recovery checks for the boot image, services, models, and Goblins identity' os/applications/org.goblins.OS.Recovery.desktop && rg -q 'Comment=Native Goblins OS policy, enterprise controls, data boundaries, and permission gates' os/applications/org.goblins.OS.Policy.desktop"
check "OpenAI service launcher copy is Goblins-native" "rg -Fq 'unknown Goblins OS service id' crates/goblins-os-open/src/main.rs && rg -Fq 'Goblins OS service {service_id} is blocked by the active Goblins OS policy' crates/goblins-os-open/src/main.rs && ! rg -Fq 'OpenAI OS service' crates/goblins-os-open/src/main.rs && rg -Fq 'Description=Goblins OS local AI service core' os/systemd/goblins-os-core.service"
check "core service owns policy state for permission grants" "rg -q '^StateDirectory=.*goblins-os/policy' os/systemd/goblins-os-core.service && rg -q '^StateDirectoryMode=0750$' os/systemd/goblins-os-core.service"
check "installer policy copy hides raw installer engine name" "rg -q 'advanced storage' crates/goblins-os-core/src/install_targets.rs && rg -q 'installer' crates/goblins-os-core/src/install_targets.rs && rg -q 'Goblins OS disk installer' crates/goblins-os-core/src/install_targets.rs && ! rg -q 'Anaconda' crates/goblins-os-core/src/install_targets.rs && ! rg -q 'bootc installer' crates/goblins-os-core/src/install_targets.rs && ! rg -q -e 'Ready for guarded bootc install preparation' -e 'bootc install was started by the Goblins OS core' -e 'could not spawn bootc install' -e 'core may spawn bootc install' crates/goblins-os-core/src/install_targets.rs"
check "installer UI copy uses advanced storage path" "rg -q 'open advanced storage' crates/goblins-os-core/src/install_targets.rs crates/goblins-os-installer/src/main.rs && rg -q 'advanced storage' crates/goblins-os-core/src/install_targets.rs crates/goblins-os-installer/src/main.rs && ! rg -q -e 'ISO manual storage' -e 'ISO Installation Destination' -e 'Installation Destination in the ISO' -e 'manual storage from the ISO' -e 'Use Installation Destination' crates/goblins-os-core/src/install_targets.rs crates/goblins-os-installer/src/main.rs"
check "installer docs use advanced storage language" "rg -q 'advanced storage Installation Destination' os/hardware-gate/runbook.md && rg -q 'advanced storage' \"$SHIP_DECL\" os/hardware-gate/runbook.md && rg -q 'advanced storage' os/iso/config.toml && ! rg -q -e 'uses Anaconda Installation Destination/manual storage' -e 'Anaconda manual storage summary' -e 'visible in Anaconda' -e 'to Anaconda manual storage' -e 'choose the disk/storage layout in Anaconda' \"$SHIP_DECL\" os/iso/config.toml os/hardware-gate/runbook.md"
check "settings detail copy hides raw setup state" "rg -Fq '(\"not configured\", \"not set up\")' crates/goblins-os-settings/src/main.rs && rg -Fq '(\"not available yet\", \"not ready yet\")' crates/goblins-os-settings/src/main.rs"
check "settings native app handoff uses image-owned copy" "rg -q 'Not Included' crates/goblins-os-settings/src/main.rs && rg -q 'included in the full Goblins OS image' crates/goblins-os-settings/src/main.rs && ! rg -q -e 'is not installed on this image' -e 'Not Installed' crates/goblins-os-settings/src/main.rs"
check "settings storage pressure plan is actionable" "rg -q 'append_storage_pressure_plan' crates/goblins-os-settings/src/main.rs && rg -q 'Storage pressure plan' crates/goblins-os-settings/src/main.rs && rg -q 'Open Disk Usage Analyzer' crates/goblins-os-settings/src/main.rs && rg -q 'Open Disks' crates/goblins-os-settings/src/main.rs && rg -q 'automatic removal of aged files' crates/goblins-os-settings/src/main.rs && ! rg -q 'needs GNOME' crates/goblins-os-settings/src/main.rs"
check "privacy cleanup copy uses aged wording" "rg -q 'Remove aged temporary files' crates/goblins-os-settings/src/main.rs crates/goblins-os-core/src/privacy.rs && ! rg -q 'Remove old temporary files' crates/goblins-os-settings/src/main.rs crates/goblins-os-core/src/privacy.rs"
check "settings built-in capability copy avoids install-manager wording" "rg -q 'Bluetooth support is not ready on this device' crates/goblins-os-settings/src/main.rs && rg -q 'Audio routing support is not ready in this build' crates/goblins-os-settings/src/main.rs && rg -q 'Codex · not included' crates/goblins-os-settings/src/main.rs && rg -q 'Required service support is not included in this build' crates/goblins-os-settings/src/main.rs"
check "core built-in capability copy avoids install-manager wording" "rg -q 'Bluetooth support is not ready on this device' crates/goblins-os-core/src/bluetooth.rs && rg -q 'Audio routing controls are not ready' crates/goblins-os-core/src/audio.rs && rg -q 'Codex account support is not included in this build' crates/goblins-os-core/src/codex.rs && ! rg -q -e 'Bluetooth support is not installed' -e 'WirePlumber control tooling is not installed' -e 'Codex CLI is not installed' crates/goblins-os-core/src/bluetooth.rs crates/goblins-os-core/src/audio.rs crates/goblins-os-core/src/codex.rs"
check "ISO/runbook document Custom or Reclaim Space dual boot" "rg -q 'Custom/manual storage or Reclaim Space' os/iso/config.toml os/hardware-gate/runbook.md"
check "ISO/runbook document advanced storage handoff" "rg -q 'Open advanced storage' os/iso/config.toml os/hardware-gate/runbook.md && rg -q 'Install Goblins OS Beside Another OS' os/hardware-gate/runbook.md"
check "runbook documents disk and Docker preflight" "rg -q '120 GiB free' os/hardware-gate/runbook.md && rg -q 'docker info' os/hardware-gate/runbook.md"
check "SHIP documents free-space or dedicated-disk dual boot" "rg -q 'unallocated free space or a dedicated disk' \"$SHIP_DECL\""
check "SHIP documents safe install-beside route" "rg -q 'Install beside an existing OS' \"$SHIP_DECL\" && rg -q 'every filesystem that will be formatted' \"$SHIP_DECL\""
check "SHIP documents dual-boot readiness checklist" "rg -q 'Dual-boot readiness' \"$SHIP_DECL\" && rg -q 'Windows/macOS/Linux/other OS' \"$SHIP_DECL\""
check "SHIP documents dual-boot assistant" "rg -q 'Dual-boot assistant' \"$SHIP_DECL\""
check "SHIP documents dual-boot decision map" "rg -q 'Dual-boot decision map' \"$SHIP_DECL\" && rg -q 'separate-disk rows' \"$SHIP_DECL\""
check "SHIP documents pre-write boot formatting plan" "rg -q 'Before writing to disk' \"$SHIP_DECL\" && rg -q 'fresh GPT layout' \"$SHIP_DECL\" && rg -q 'bootloader/EFI target' \"$SHIP_DECL\" && rg -q 'xfs root' \"$SHIP_DECL\""
check "SHIP documents advanced storage entry point" "rg -q 'Open advanced storage' \"$SHIP_DECL\" && rg -q 'Install Goblins OS Beside Another OS' \"$SHIP_DECL\""
check "external gate names preserved existing OS partitions" "rg -q 'preserved Windows/macOS/APFS/Linux/other OS/recovery/EFI partitions' os/hardware-gate/run-external-gate.sh"
check "external gate documents advanced storage entry point" "rg -q 'Open advanced storage' os/hardware-gate/run-external-gate.sh && rg -q 'Install Goblins OS Beside Another OS' os/hardware-gate/run-external-gate.sh"
check "bootc image includes advanced storage handoff" "rg -q 'anaconda-live' os/bootc/Containerfile && rg -q 'goblins-os-full-installer' os/bootc/Containerfile && rg -q 'org.goblins.OS.FullInstaller.desktop' os/bootc/Containerfile && rg -q 'desktop-file-validate /usr/share/applications/org.goblins.OS.FullInstaller.desktop' os/bootc/Containerfile"
check "core AI exposes notification context route" "rg -Fq '/v1/ai/notification-context' crates/goblins-os-core/src/main.rs && rg -Fq 'ask_notification_context' crates/goblins-os-core/src/main.rs"
check "core AI notification context is permission gated" "rg -Fq 'policy_state_for_control(\"notification-context\")' crates/goblins-os-core/src/ai.rs && rg -Fq 'Allow notification context in Privacy & Permissions' crates/goblins-os-core/src/ai.rs"
check "core AI notification context is bounded to one invoked notification" "rg -Fq 'Use only this invoked notification summary' crates/goblins-os-core/src/ai.rs && rg -Fq 'do not claim to inspect notification history, other notifications, files, screenshots, secrets, hidden windows, or background app data' crates/goblins-os-core/src/ai.rs"
check "core AI notification context audits registered action only" "rg -Fq 'audit_ai_action(\"answer-notification\"' crates/goblins-os-core/src/ai.rs && rg -Fq 'notification_context_prompt_is_invoked_and_bounded_to_one_notification' crates/goblins-os-core/src/ai.rs"
check "core AI runtime uses Goblins-native route with legacy compatibility" "rg -Fq '/v1/ai/runtime/status' crates/goblins-os-core/src/main.rs && rg -Fq '/v1/ai/runtime' crates/goblins-os-core/src/main.rs && rg -Fq '.route(\"/v1/codex/resident\", post(ai_runtime))' crates/goblins-os-core/src/main.rs"
check "desktop clients use Goblins-native AI runtime route" "rg -Fq '/v1/ai/runtime/status' crates/goblins-os-settings/src/main.rs crates/goblins-os-shell/src/main.rs && rg -Fq '/v1/ai/runtime' crates/goblins-os-launcher/src/main.rs && ! rg -Fq '\"/v1/codex/resident/status\"' crates/goblins-os-settings/src/main.rs crates/goblins-os-shell/src/main.rs && ! rg -Fq '\"/v1/codex/resident\"' crates/goblins-os-launcher/src/main.rs"
check "installed self-test checks AI runtime primary route and compatibility alias" "rg -Fq '/v1/ai/runtime/status' os/bootc/run-selftest.sh && rg -Fq '/v1/codex/resident/status' os/bootc/run-selftest.sh && rg -Fq 'Goblins AI runtime IPC socket live' os/bootc/run-selftest.sh"
check "settings exposes notification AI readiness" "rg -q 'append_notifications_ai_context' crates/goblins-os-settings/src/main.rs && rg -q 'Goblins AI for notifications' crates/goblins-os-settings/src/main.rs && rg -q 'answer-notification' crates/goblins-os-settings/src/main.rs"
check "voice assistant uses Goblin wake word truthfully" "rg -q 'VOICE_WAKE_WORD: &str = \"Goblin\"' crates/goblins-os-core/src/voice.rs && rg -q '\"Hey Goblin\"' crates/goblins-os-core/src/voice.rs && rg -q 'wake_listening' crates/goblins-os-core/src/voice.rs && rg -q 'Background wake listening is not ready' crates/goblins-os-core/src/voice.rs crates/goblins-os-settings/src/main.rs && rg -Fq 'Say {voice_word}' crates/goblins-os-shell/src/main.rs && rg -Fq 'Listening for {wake_word}…' crates/goblins-os-shell/src/main.rs && rg -q 'Goblin wake word' crates/goblins-os-settings/src/main.rs && rg -q 'Ask Goblin' crates/goblins-os-launcher/src/main.rs crates/goblins-os-settings/src/main.rs crates/goblins-os-control-center/src/main.rs crates/goblins-os-ai/src/lib.rs os/gnome-shell-extensions/goblins-menubar@goblins.os/extension.js && rg -Fq 'scripts/Ask Goblin about this' crates/goblins-os-verify/src/main.rs && test -f 'os/nautilus/scripts/Ask Goblin about this' && ! rg -q -e 'Talk[[:space:]]to[[:space:]]Goblins[[:space:]]OS' -e 'Ask[[:space:]]Goblins' -e 'Write[[:space:]]with[[:space:]]Goblins' -e 'Voice[[:space:]]model' crates/goblins-os-shell/src/main.rs crates/goblins-os-launcher/src/main.rs crates/goblins-os-settings/src/main.rs crates/goblins-os-ai/src/lib.rs os/gnome-shell-extensions/goblins-menubar@goblins.os/extension.js"
check "voice control dispatch is source-gated" "rg -q '/v1/voice/control' crates/goblins-os-core/src/main.rs && rg -q 'dispatch_voice_safe_setting_change' crates/goblins-os-core/src/voice_control.rs && rg -q 'fall_through_to_dictation: true' crates/goblins-os-core/src/voice_control.rs && rg -q 'id: \"voice-control\"' crates/goblins-os-ai/src/lib.rs && test -x os/voice/goblins-os-voice-control && rg -q 'goblins-os-voice-control' os/bootc/Containerfile && rg -q 'Voice Control is source-gated' crates/goblins-os-settings/src/main.rs"
check "sound recognition decision contract is source-gated" "rg -q 'evaluate_sound_recognition_window' crates/goblins-os-core/src/sound_recognition.rs && rg -q 'sound_recognition_notification_payload' crates/goblins-os-core/src/sound_recognition.rs && rg -q 'sound_recognition_notification_delivery_plan' crates/goblins-os-core/src/sound_recognition.rs && rg -q 'listener_runtime_capabilities' crates/goblins-os-core/src/sound_recognition.rs && rg -Fq 'payload.runtime_ready_claim.unwrap_or(false)' crates/goblins-os-core/src/sound_recognition.rs && rg -q 'capture_runtime_ready' crates/goblins-os-core/src/sound_recognition.rs os/sound-recognition/goblins-os-sound-listener os/bootc/Containerfile && rg -q 'notification_delivery_contract_ready' os/sound-recognition/goblins-os-sound-listener os/bootc/Containerfile && rg -q 'delivery_ready_claim' os/sound-recognition/goblins-os-sound-listener crates/goblins-os-core/src/sound_recognition.rs && rg -q 'decision_contract_ready' os/sound-recognition/goblins-os-sound-listener && rg -q -- '--decision-self-test' os/sound-recognition/goblins-os-sound-listener os/bootc/Containerfile && rg -q -- '--notification-self-test' os/sound-recognition/goblins-os-sound-listener os/bootc/Containerfile"
check "live captions overlay is source-gated" "rg -q '/v1/captions/stream' crates/goblins-os-core/src/main.rs && rg -q 'text/event-stream' crates/goblins-os-core/src/live_captions.rs && rg -q 'pipewire_monitor_targets_from_dump' crates/goblins-os-core/src/live_captions.rs && rg -q 'caption_capture_args' crates/goblins-os-core/src/live_captions.rs && rg -q 'capture_runtime_ready: false' crates/goblins-os-core/src/live_captions.rs && rg -q 'transcription_ready_claim: false' crates/goblins-os-core/src/live_captions.rs && rg -q 'no live monitor target, capture stream, or transcription loop is claimed yet' crates/goblins-os-core/src/live_captions.rs && test -f os/gnome-shell-extensions/goblins-captions@goblins.os/extension.js && rg -q 'waiting for the local caption stream' os/gnome-shell-extensions/goblins-captions@goblins.os/extension.js && rg -q 'showWaitingRenderProof' os/gnome-shell-extensions/goblins-captions@goblins.os/extension.js && rg -q 'captureRuntimeReadyClaim: false' os/gnome-shell-extensions/goblins-captions@goblins.os/extension.js && rg -q 'transcriptionReadyClaim: false' os/gnome-shell-extensions/goblins-captions@goblins.os/extension.js && rg -Fq '58-live-captions-waiting-\$suffix.png' os/bootc/render-desktop.sh && rg -q 'font-family: \"Inter\"' os/gnome-shell-extensions/goblins-captions@goblins.os/stylesheet.css && rg -q 'goblins-captions@goblins.os' os/gnome-shell-modes/goblins-os.json"
check "settings live captions row is source-gated" "rg -q '/v1/live-captions/status' crates/goblins-os-settings/src/main.rs && rg -q 'append_live_captions_settings' crates/goblins-os-settings/src/main.rs && rg -q 'Toggle lives in Quick Settings' crates/goblins-os-settings/src/main.rs && rg -q 'Captioning stays local.' crates/goblins-os-settings/src/main.rs"
check "visual lookup launcher is source-gated" "test -f os/applications/org.goblins.OS.VisualLookup.desktop && rg -Fq 'Name=Visual Look Up' os/applications/org.goblins.OS.VisualLookup.desktop && rg -Fq 'Exec=/usr/libexec/goblins-os/goblins-os-visual-lookup' os/applications/org.goblins.OS.VisualLookup.desktop && rg -Fq 'NoDisplay=false' os/applications/org.goblins.OS.VisualLookup.desktop && rg -Fq 'StartupWMClass=org.goblins.OS.VisualLookup' os/applications/org.goblins.OS.VisualLookup.desktop && rg -Fq 'desktop-file-validate /usr/share/applications/org.goblins.OS.VisualLookup.desktop' os/bootc/Containerfile && rg -Fq 'goblins-os-visual-lookup/native-desktop' os/bootc/Containerfile"
check "today panel render hook is source-gated" "rg -Fq 'capture goblins-os-today' os/bootc/render-screens.sh && rg -Fq '122-today.png' os/bootc/render-screens.sh && rg -Fq '123-today-dark.png' os/bootc/render-screens.sh && rg -Fq '/usr/libexec/goblins-os/goblins-os-today' os/bootc/render-screens.sh"
check "preview viewer packages and defaults are source-gated" "rg -q 'papers' os/bootc/Containerfile && rg -q 'loupe' os/bootc/Containerfile && rg -q 'command -v papers' os/bootc/Containerfile && rg -q 'command -v loupe' os/bootc/Containerfile && test -f os/applications/mimeapps.list && rg -q 'application/pdf=org.gnome.Papers.desktop' os/applications/mimeapps.list && rg -q 'image/png=org.gnome.Loupe.desktop' os/applications/mimeapps.list && rg -q 'image/jpeg=org.gnome.Loupe.desktop' os/applications/mimeapps.list"
check "preview open substrate is source-gated" "rg -q '/v1/preview/status' crates/goblins-os-core/src/main.rs && rg -q '/v1/preview/open' crates/goblins-os-core/src/main.rs && rg -Fq 'Command::new(\"xdg-open\")' crates/goblins-os-core/src/preview.rs && rg -q 'Papers for PDFs and Loupe for images' crates/goblins-os-core/src/preview.rs && rg -q 'It never reads file contents or claims rendered proof.' crates/goblins-os-core/src/preview.rs"
check "preview open uses session bridge before direct fallback" "rg -Fq 'crate::session_bridge::open_preview' crates/goblins-os-core/src/preview.rs && rg -Fq 'OpenPreview' crates/goblins-os-session-bridge/src/main.rs && rg -Fq 'xdg-open' crates/goblins-os-session-bridge/src/main.rs"
check "preview installed-image open self-test is source-gated" "rg -Fq 'GET /v1/preview/status -> HTTP' os/bootc/run-selftest.sh && rg -Fq 'available=\$preview_available xdg-open=\$preview_xdg_open papers=\$preview_papers loupe=\$preview_loupe' os/bootc/run-selftest.sh && rg -Fq 'supported_extensions | index(\"pdf\") and index(\"png\")' os/bootc/run-selftest.sh && rg -Fq 'POST /v1/preview/open PDF -> HTTP' os/bootc/run-selftest.sh && rg -Fq 'POST /v1/preview/open image -> HTTP' os/bootc/run-selftest.sh && rg -Fq 'POST /v1/preview/open unsupported -> HTTP' os/bootc/run-selftest.sh"
check "fingerprint unlock substrate is source-gated" "rg -q '/v1/fingerprint/status' crates/goblins-os-core/src/main.rs && rg -q 'authselect_has_fingerprint' crates/goblins-os-core/src/fingerprint.rs && rg -q 'net.reactivated.Fprint.service' crates/goblins-os-core/src/fingerprint.rs && rg -q 'password remains available' crates/goblins-os-core/src/fingerprint.rs && rg -q 'authselect enable-feature with-fingerprint' os/bootc/Containerfile && rg -q 'pam_fprintd.so' os/bootc/Containerfile && rg -q 'fprintd-pam' os/bootc/Containerfile && rg -q 'Fingerprint unlock' crates/goblins-os-settings/src/main.rs"
check "keychain collection metadata is source-gated" "rg -q '/v1/keychain/collections' crates/goblins-os-core/src/main.rs crates/goblins-os-settings/src/main.rs && rg -q 'org.freedesktop.Secret.Service' crates/goblins-os-core/src/keychain.rs && rg -q 'Secret values are never returned by Goblins OS' crates/goblins-os-core/src/keychain.rs && rg -q 'Secret values are never displayed in Settings' crates/goblins-os-settings/src/main.rs && ! rg -q 'GetSecrets' crates/goblins-os-core/src/keychain.rs"
check "keychain manager handoff is source-gated" "rg -q 'Open Passwords & Keys' crates/goblins-os-settings/src/main.rs && rg -Fq 'SEAHORSE_PASSWORDS_AND_KEYS: &str = \"seahorse\"' crates/goblins-os-settings/src/main.rs && rg -Fq 'append_keychain_manager_handoff(panel)' crates/goblins-os-settings/src/main.rs && rg -q 'manage saved passwords, keys, and certificates in the system keyring' crates/goblins-os-settings/src/main.rs"
check "personal hotspot write substrate is source-gated" "rg -q '/v1/hotspot/enabled' crates/goblins-os-core/src/main.rs && rg -Fq 'policy_state_for_control(\"settings-control\")' crates/goblins-os-core/src/hotspot.rs && rg -q 'dnsmasq_present' crates/goblins-os-core/src/hotspot.rs && rg -q 'Connect to the internet over Ethernet to share it over Wi-Fi.' crates/goblins-os-core/src/hotspot.rs && rg -q 'sanitize_hotspot_error' crates/goblins-os-core/src/hotspot.rs && rg -q 'dnsmasq' os/bootc/Containerfile && rg -q 'command -v dnsmasq' os/bootc/Containerfile && rg -q 'append_hotspot_management' crates/goblins-os-settings/src/main.rs && rg -q 'hotspot_settings_inputs' crates/goblins-os-settings/src/main.rs && rg -q 'Passwords are used once to configure the hotspot and are never shown here.' crates/goblins-os-settings/src/main.rs && rg -q 'connected_clients_known' crates/goblins-os-core/src/hotspot.rs && rg -q 'parse_dnsmasq_leases' crates/goblins-os-core/src/hotspot.rs && rg -q 'Connected devices' crates/goblins-os-settings/src/main.rs"
check "switch control overlay is source-gated" "test -f os/gnome-shell-extensions/goblins-switch@goblins.os/extension.js && rg -q \"const SCHEMA_ID = 'org.goblins.os.a11y.switch-control';\" os/gnome-shell-extensions/goblins-switch@goblins.os/extension.js && rg -Fq \"import('gi://Atspi')\" os/gnome-shell-extensions/goblins-switch@goblins.os/extension.js && rg -q 'This window has no scannable controls - using point scan.' os/gnome-shell-extensions/goblins-switch@goblins.os/extension.js && rg -q 'Point selection needs live qemu proof before pointer injection is enabled.' os/gnome-shell-extensions/goblins-switch@goblins.os/extension.js && rg -q 'font-family: \"Inter\"' os/gnome-shell-extensions/goblins-switch@goblins.os/stylesheet.css && rg -q 'goblins-switch@goblins.os' os/gnome-shell-modes/goblins-os.json && rg -q 'goblins-switch@goblins.os' os/dconf/db/local.d/10-goblins-os-desktop"
check "switch control desktop render hook is source-gated" "rg -q 'showPointScanDemo' os/gnome-shell-extensions/goblins-switch@goblins.os/extension.js && rg -Fq '57-switch-control-point-\$suffix.png' os/bootc/render-desktop.sh && rg -q 'showPointScanDemo' os/bootc/render-desktop.sh"
check "IME menu-bar input source render hook is source-gated" "rg -Fq '59-menubar-input-source-\$suffix.png' os/bootc/render-desktop.sh && rg -Fq \"[('xkb', 'us'), ('xkb', 'gb')]\" os/bootc/render-desktop.sh && rg -q 'gsettings set org.gnome.desktop.input-sources current 1' os/bootc/render-desktop.sh"
check "Today menu-bar date button is source-gated" "rg -Fq '/usr/libexec/goblins-os/goblins-os-today' os/gnome-shell-extensions/goblins-menubar@goblins.os/extension.js && rg -Fq \"this._today = new PanelMenu.Button(0.0, 'Today', true);\" os/gnome-shell-extensions/goblins-menubar@goblins.os/extension.js && rg -Fq \"Main.panel.addToStatusArea('goblins-today', this._today, 1, 'right');\" os/gnome-shell-extensions/goblins-menubar@goblins.os/extension.js && rg -Fq 'GLib.DateTime.new_now_local().format' os/gnome-shell-extensions/goblins-menubar@goblins.os/extension.js && rg -Fq 'changed::clock-format' os/gnome-shell-extensions/goblins-menubar@goblins.os/extension.js && rg -Fq 'this._clearTodayClockTimer();' os/gnome-shell-extensions/goblins-menubar@goblins.os/extension.js && rg -Fq '.goblins-date-indicator' os/gnome-shell-extensions/goblins-menubar@goblins.os/stylesheet.css os/themes/GoblinsOS/gnome-shell/gnome-shell-light.css"
check "Today menu-bar render hook is source-gated" "rg -Fq '59c-menubar-today-\$suffix.png' os/bootc/render-desktop.sh && rg -q 'gsettings set org.gnome.desktop.interface clock-show-weekday true' os/bootc/render-desktop.sh && rg -q 'gsettings set org.gnome.desktop.interface clock-show-seconds false' os/bootc/render-desktop.sh"
check "settings notification AI copy preserves privacy boundary" "rg -q \"only that notification's title, body, app, and chosen action label\" crates/goblins-os-settings/src/main.rs"
check "launcher search uses native accessible icon" "rg -Fq 'gtk::Image::from_icon_name(\"system-search-symbolic\")' crates/goblins-os-launcher/src/main.rs && rg -q 'Search Goblins OS' crates/goblins-os-launcher/src/main.rs && ! rg -q 'telephone-recorder' crates/goblins-os-launcher/src/main.rs"
check "control center controls use accessible title-case copy" "rg -q 'Connection & Appearance' crates/goblins-os-control-center/src/main.rs && rg -q 'Goblins AI' crates/goblins-os-control-center/src/main.rs && rg -q 'Sound' crates/goblins-os-control-center/src/main.rs && rg -q 'Display brightness' crates/goblins-os-control-center/src/main.rs && rg -q 'set_accessible_label_description' crates/goblins-os-control-center/src/main.rs && rg -q 'Use on-device GPT-OSS' crates/goblins-os-control-center/src/main.rs && ! rg -q -e 'CONNECTION & APPEARANCE' -e 'BUILD ENGINE' -e 'GOBLINS AI' -e 'SOUND' -e 'DISPLAY' crates/goblins-os-control-center/src/main.rs"
check "shell dock and window manager controls expose accessible names and focus states" "rg -q 'accessible_name: .*Open' os/gnome-shell-extensions/goblins-dock@goblins.os/extension.js && rg -q 'accessible_name: .*Activate' os/gnome-shell-extensions/goblins-wm@goblins.os/extension.js && rg -q \"accessible_name: 'Move to previous space'\" os/gnome-shell-extensions/goblins-wm@goblins.os/extension.js && rg -q '.goblins-dock-item:focus' os/gnome-shell-extensions/goblins-dock@goblins.os/stylesheet.css && rg -q '.goblins-wm-window-card:focus' os/gnome-shell-extensions/goblins-wm@goblins.os/stylesheet.css && rg -q '.goblins-wm-hud-button:focus' os/gnome-shell-extensions/goblins-wm@goblins.os/stylesheet.css"
check "core AI exposes confirmed safe setting route" "rg -q '/v1/ai/safe-setting-change' crates/goblins-os-core/src/main.rs && rg -q 'change_safe_setting' crates/goblins-os-core/src/main.rs"
check "core AI exposes open settings panel route" "rg -q '/v1/ai/open-settings-panel' crates/goblins-os-core/src/main.rs && rg -q 'open_settings_panel' crates/goblins-os-core/src/main.rs"
check "core AI open settings panel route is deterministic and offline" "rg -q 'OpenSettingsPanelRequest' crates/goblins-os-core/src/ai.rs && rg -q 'SETTINGS_PANEL_CANDIDATES' crates/goblins-os-core/src/ai.rs && rg -q 'resolve_open_settings_panel' crates/goblins-os-core/src/ai.rs && rg -q 'settings_panel_router_maps_exact_and_natural_language_requests' crates/goblins-os-core/src/ai.rs"
check "core AI open settings panel route uses policy and audit" "rg -Fq 'policy_state_for_control(\"resident-assistant\")' crates/goblins-os-core/src/ai.rs && rg -Fq 'audit_open_settings_panel' crates/goblins-os-core/src/ai.rs && rg -Fq 'launch_argument: format!(\"--panel={}\"' crates/goblins-os-core/src/ai.rs"
check "installed self-test checks open settings panel route" "rg -q '/v1/ai/open-settings-panel' os/bootc/run-selftest.sh && rg -q 'open wifi settings' os/bootc/run-selftest.sh"
check "core AI exposes system status route" "rg -q '/v1/ai/system-status' crates/goblins-os-core/src/main.rs && rg -q 'ask_system_status' crates/goblins-os-core/src/main.rs"
check "core AI system status route uses OS-owned bounded snapshot" "rg -q 'SystemStatusContextRequest' crates/goblins-os-core/src/ai.rs && rg -q 'bounded_system_status_snapshot' crates/goblins-os-core/src/ai.rs && rg -q 'Use only this OS-owned status snapshot' crates/goblins-os-core/src/ai.rs && rg -q 'system_status_prompt_uses_only_os_owned_snapshot' crates/goblins-os-core/src/ai.rs"
check "core AI system status route uses policy and audit" "rg -q 'system_troubleshooting_policy' crates/goblins-os-core/src/ai.rs && rg -Fq 'audit_ai_action(action_id, Some(\"troubleshooting\")' crates/goblins-os-core/src/ai.rs && rg -q 'system_status_action_id' crates/goblins-os-core/src/ai.rs"
check "installed self-test checks system status route" "rg -q '/v1/ai/system-status' os/bootc/run-selftest.sh && rg -q 'Summarize current system state' os/bootc/run-selftest.sh"
check "core input sources expose narrow write route and encoder" "rg -q '/v1/input/sources' crates/goblins-os-core/src/main.rs && rg -q 'normalize_input_sources' crates/goblins-os-core/src/input.rs && rg -q 'encode_input_sources' crates/goblins-os-core/src/input.rs"
check "settings input sources expose reorder and remove write controls" "rg -q 'input_source_action_button' crates/goblins-os-settings/src/main.rs && rg -q 'reordered_input_sources' crates/goblins-os-settings/src/main.rs && rg -q 'input_sources_without' crates/goblins-os-settings/src/main.rs && rg -q '/v1/input/sources' crates/goblins-os-settings/src/main.rs"
check "capture harness resets dated run dir before capture" "rg -Fq 'rm -rf \"\$RUN_DIR\"' os/hardware-gate/capture-harness/run-capture.sh && rg -q 'refusing to reset unexpected hardware-gate run dir' os/hardware-gate/capture-harness/run-capture.sh && rg -Fq '\"\$REPO\"/os/screenshots/hardware-gate/\"\$ARCH\"/*' os/hardware-gate/capture-harness/run-capture.sh"
check "capture harness rejects stale GDM screenshot sets" "rg -Fq 'stable_frame_hash' os/hardware-gate/capture-harness/run-capture.sh && rg -Fq 'cropping the top bar' os/hardware-gate/capture-harness/run-capture.sh && rg -Fq 'Refusing stale screenshot signoff' os/hardware-gate/capture-harness/run-capture.sh"
check "capture harness waits for unique required screenshots" "rg -Fq 'REQUIRED_FRAME_SETTLE_SECONDS' os/hardware-gate/capture-harness/drive-capture.py && rg -Fq 'framebuffer stayed duplicate' os/hardware-gate/capture-harness/drive-capture.py && rg -Fq \"! -name '_debug-*'\" os/hardware-gate/capture-harness/run-capture.sh && rg -Fq 'required captured surfaces are distinct' os/hardware-gate/capture-harness/run-capture.sh"
check "capture driver uses progress-aware ready-signal timeouts" "rg -Fq 'GOS_CAPTURE_TOTAL_TIMEOUT_SECONDS' os/hardware-gate/capture-harness/drive-capture.py && rg -Fq 'GOS_CAPTURE_INACTIVITY_TIMEOUT_SECONDS' os/hardware-gate/capture-harness/drive-capture.py && rg -Fq 'last_progress = time.time()' os/hardware-gate/capture-harness/drive-capture.py && rg -Fq 'EXPECTED_READY_SHOTS' os/hardware-gate/capture-harness/drive-capture.py && rg -Fq 'seconds_since_progress' os/hardware-gate/capture-harness/drive-capture.py"
check "capture harness launches current-session nonunique proof windows" "rg -Fq 'GOBLINS_OS_CAPTURE_NON_UNIQUE=1' os/hardware-gate/capture-harness/in-session-orchestrator.sh && rg -Fq 'GOS_SHOT_SETTLE_SECONDS' os/hardware-gate/capture-harness/in-session-orchestrator.sh && rg -Fq 'pkill -x \"\$base\"' os/hardware-gate/capture-harness/in-session-orchestrator.sh && rg -Fq 'pkill -f -- \"\$bin\"' os/hardware-gate/capture-harness/in-session-orchestrator.sh && ! rg -Fq 'dbus-run-session -- \"\$@\"' os/hardware-gate/capture-harness/in-session-orchestrator.sh"
check "capture harness bounds ready signals and shot helper cleanup" "rg -Fq 'GOS_READY_SIGNAL_TIMEOUT_SECONDS' os/hardware-gate/capture-harness/in-session-orchestrator.sh && rg -Fq 'GOS_SHOT_HELPER_TIMEOUT_SECONDS' os/hardware-gate/capture-harness/in-session-orchestrator.sh && rg -Fq 'GOBLINS_HWGATE_BOUNDED_COMMAND_TIMED_OUT' os/hardware-gate/capture-harness/in-session-orchestrator.sh && rg -Fq 'timeout -k 2s' os/hardware-gate/capture-harness/in-session-orchestrator.sh && rg -Fq 'GOBLINS_HWGATE_SHOT_SIGNALING' os/hardware-gate/capture-harness/in-session-orchestrator.sh"
check "capture harness routes installer screenshots through fixture core" "rg -Fq 'installer_shot()' os/hardware-gate/capture-harness/in-session-orchestrator.sh && rg -Fq 'GOBLINS_OS_INSTALLER_CORE_WAIT_SECS' os/hardware-gate/capture-harness/in-session-orchestrator.sh && rg -Fq 'GOS_INSTALLER_CAPTURE_CORE_WAIT_SECS' os/hardware-gate/capture-harness/in-session-orchestrator.sh && rg -Fq 'installer_shot welcome 06-onboarding' os/hardware-gate/capture-harness/in-session-orchestrator.sh && rg -Fq 'installer_shot network 02-install-network' os/hardware-gate/capture-harness/in-session-orchestrator.sh && rg -Fq 'Some(\"welcome\") => \"welcome\"' crates/goblins-os-installer/src/main.rs"
check "capture GTK apps support nonunique proof instances" "rg -Fq 'GOBLINS_OS_CAPTURE_NON_UNIQUE' crates/goblins-os-shell/src/main.rs && rg -Fq 'ApplicationFlags::NON_UNIQUE' crates/goblins-os-shell/src/main.rs && rg -Fq 'GOBLINS_OS_CAPTURE_NON_UNIQUE' crates/goblins-os-settings/src/main.rs && rg -Fq 'ApplicationFlags::NON_UNIQUE' crates/goblins-os-settings/src/main.rs && rg -Fq 'GOBLINS_OS_CAPTURE_NON_UNIQUE' crates/goblins-os-installer/src/main.rs && rg -Fq 'ApplicationFlags::NON_UNIQUE' crates/goblins-os-installer/src/main.rs"
check "capture harness disables switch overlay before screenshots" "rg -Fq 'gnome-extensions disable goblins-switch@goblins.os' os/hardware-gate/capture-harness/in-session-orchestrator.sh && rg -Fq 'globalThis.goblinsSwitchControl' os/hardware-gate/capture-harness/in-session-orchestrator.sh"
check "hardware gate requires Input sources roundtrip proof" "rg -q 'input-sources-roundtrip-proof.json' os/hardware-gate/capture-harness/drive-capture.py os/hardware-gate/capture-harness/run-capture.sh os/hardware-gate/close-signoff.sh os/hardware-gate/verify-shipping-status.sh && rg -q '/proof/input-sources-roundtrip' os/hardware-gate/capture-harness/in-session-orchestrator.sh && rg -q '/v1/input/sources' os/hardware-gate/capture-harness/in-session-orchestrator.sh os/hardware-gate/capture-harness/run-capture.sh && rg -q '/v1/input/switch-next' os/hardware-gate/capture-harness/in-session-orchestrator.sh os/hardware-gate/capture-harness/run-capture.sh && rg -q 'test_sources=xkb-us,xkb-gb' os/hardware-gate/capture-harness/in-session-orchestrator.sh && rg -q 'sources_gsettings_readback=true' os/hardware-gate/capture-harness/in-session-orchestrator.sh os/hardware-gate/capture-harness/run-capture.sh && rg -q 'switch_switched=true' os/hardware-gate/capture-harness/in-session-orchestrator.sh os/hardware-gate/capture-harness/run-capture.sh && rg -q 'restore_sources=true' os/hardware-gate/capture-harness/in-session-orchestrator.sh os/hardware-gate/capture-harness/run-capture.sh && rg -q 'Input sources roundtrip checked' os/hardware-gate/close-signoff.sh os/hardware-gate/verify-shipping-status.sh"
check "hardware gate requires Focus arm roundtrip proof" "rg -q 'focus-arm-roundtrip-proof.json' os/hardware-gate/capture-harness/drive-capture.py os/hardware-gate/capture-harness/run-capture.sh os/hardware-gate/close-signoff.sh os/hardware-gate/verify-shipping-status.sh && rg -q '/proof/focus-arm-roundtrip' os/hardware-gate/capture-harness/in-session-orchestrator.sh && rg -q '/v1/focus/status' os/hardware-gate/capture-harness/in-session-orchestrator.sh os/hardware-gate/capture-harness/run-capture.sh && rg -q '/v1/focus/activate' os/hardware-gate/capture-harness/in-session-orchestrator.sh os/hardware-gate/capture-harness/run-capture.sh && rg -q '/v1/focus/deactivate' os/hardware-gate/capture-harness/in-session-orchestrator.sh os/hardware-gate/capture-harness/run-capture.sh && rg -q 'active_mode_gsettings_readback=gate-work' os/hardware-gate/capture-harness/in-session-orchestrator.sh os/hardware-gate/capture-harness/run-capture.sh && rg -q 'notification_banners_after_activate=false' os/hardware-gate/capture-harness/in-session-orchestrator.sh os/hardware-gate/capture-harness/run-capture.sh && rg -q 'original_notification_banners_restored=true' os/hardware-gate/capture-harness/in-session-orchestrator.sh os/hardware-gate/capture-harness/run-capture.sh && rg -q 'mode_crud_claim=false' os/hardware-gate/capture-harness/in-session-orchestrator.sh os/hardware-gate/capture-harness/run-capture.sh && rg -q 'Focus arm roundtrip checked' os/hardware-gate/close-signoff.sh os/hardware-gate/verify-shipping-status.sh"
check "hardware gate requires App privacy revoke proof" "rg -q 'app-privacy-revoke-proof.json' os/hardware-gate/capture-harness/drive-capture.py os/hardware-gate/capture-harness/run-capture.sh os/hardware-gate/close-signoff.sh os/hardware-gate/verify-shipping-status.sh && rg -q '/proof/app-privacy-revoke' os/hardware-gate/capture-harness/in-session-orchestrator.sh && rg -q '/v1/app-privacy/revoke' os/hardware-gate/capture-harness/in-session-orchestrator.sh os/hardware-gate/capture-harness/run-capture.sh && rg -q 'PermissionStore.SetPermission' os/hardware-gate/capture-harness/in-session-orchestrator.sh os/hardware-gate/capture-harness/run-capture.sh && rg -q 'PermissionStore.DeletePermission' os/hardware-gate/capture-harness/in-session-orchestrator.sh os/hardware-gate/capture-harness/run-capture.sh && rg -q 'PermissionStore.GetPermission' os/hardware-gate/capture-harness/in-session-orchestrator.sh os/hardware-gate/capture-harness/run-capture.sh && rg -q 'org.goblins.GatePrivacyProof' os/hardware-gate/capture-harness/in-session-orchestrator.sh os/hardware-gate/capture-harness/run-capture.sh && rg -q 'post_revoke_absent=true' os/hardware-gate/capture-harness/in-session-orchestrator.sh os/hardware-gate/capture-harness/run-capture.sh && rg -q 'restore_prior_state=true' os/hardware-gate/capture-harness/in-session-orchestrator.sh os/hardware-gate/capture-harness/run-capture.sh && rg -q 'resource_keyed_claim=false' os/hardware-gate/capture-harness/in-session-orchestrator.sh os/hardware-gate/capture-harness/run-capture.sh && rg -q 'App privacy revoke checked' os/hardware-gate/close-signoff.sh os/hardware-gate/verify-shipping-status.sh"
check "hardware gate records App privacy seed fallback diagnostics" "rg -q 'plain_permissions' os/hardware-gate/capture-harness/in-session-orchestrator.sh && rg -q 'seed_attempt=' os/hardware-gate/capture-harness/in-session-orchestrator.sh && rg -q 'seed_error=' os/hardware-gate/capture-harness/in-session-orchestrator.sh && rg -q 'proof_query_value' os/hardware-gate/capture-harness/in-session-orchestrator.sh && rg -q '/var/home/goblin/.local/share/flatpak/db' os/bootc/Containerfile && rg -q '.local/share/flatpak/db' os/hardware-gate/capture-harness/in-session-orchestrator.sh && rg -q 'permission-db-dir' os/hardware-gate/capture-harness/in-session-orchestrator.sh"
check "hardware gate requires Preview open/render proof" "rg -q 'preview-open-render-proof.json' os/hardware-gate/capture-harness/drive-capture.py os/hardware-gate/capture-harness/run-capture.sh os/hardware-gate/close-signoff.sh os/hardware-gate/verify-shipping-status.sh && rg -q '/proof/preview-open-render' os/hardware-gate/capture-harness/in-session-orchestrator.sh && rg -q '/v1/preview/status' os/hardware-gate/capture-harness/in-session-orchestrator.sh os/hardware-gate/capture-harness/run-capture.sh && rg -q '/v1/preview/open' os/hardware-gate/capture-harness/in-session-orchestrator.sh os/hardware-gate/capture-harness/run-capture.sh && rg -q 'org.gnome.Papers.desktop' os/hardware-gate/capture-harness/in-session-orchestrator.sh os/hardware-gate/capture-harness/run-capture.sh && rg -q 'org.gnome.Loupe.desktop' os/hardware-gate/capture-harness/in-session-orchestrator.sh os/hardware-gate/capture-harness/run-capture.sh && rg -q '29-preview-pdf-open.png' os/hardware-gate/capture-harness/in-session-orchestrator.sh os/hardware-gate/capture-harness/run-capture.sh os/hardware-gate/close-signoff.sh os/hardware-gate/verify-shipping-status.sh && rg -q '30-preview-image-open.png' os/hardware-gate/capture-harness/in-session-orchestrator.sh os/hardware-gate/capture-harness/run-capture.sh os/hardware-gate/close-signoff.sh os/hardware-gate/verify-shipping-status.sh && rg -q 'unsupported_rejected=true' os/hardware-gate/capture-harness/in-session-orchestrator.sh && rg -q 'Preview open/render checked' os/hardware-gate/close-signoff.sh os/hardware-gate/verify-shipping-status.sh"
check "hardware gate requires Audio output proof" "rg -q 'audio-output-proof.json' os/hardware-gate/capture-harness/drive-capture.py os/hardware-gate/capture-harness/run-capture.sh os/hardware-gate/close-signoff.sh os/hardware-gate/verify-shipping-status.sh os/hardware-gate/runbook.md && rg -q '/proof/audio-output' os/hardware-gate/capture-harness/in-session-orchestrator.sh && rg -q '/v1/audio/status' os/hardware-gate/capture-harness/in-session-orchestrator.sh && rg -q 'pw-play' os/hardware-gate/capture-harness/in-session-orchestrator.sh os/bootc/Containerfile && rg -Fq -- '-audiodev none,id=audio0' os/hardware-gate/capture-harness/run-capture.sh && rg -q 'ich9-intel-hda' os/hardware-gate/capture-harness/run-capture.sh && rg -q 'hda-output,audiodev=audio0' os/hardware-gate/capture-harness/run-capture.sh && rg -q 'GOBLINS_OS_CAPTURE_EXPECT_TITLE=\"Goblins OS Settings - Sound\"' os/hardware-gate/capture-harness/in-session-orchestrator.sh && rg -Fq 'GOS_SHOT_WINDOW_WAIT_ATTEMPTS=\"\${GOS_AUDIO_SHOT_WINDOW_WAIT_ATTEMPTS:-8}\"' os/hardware-gate/capture-harness/in-session-orchestrator.sh && rg -Fq 'GOS_SHOT_HELPER_TIMEOUT_SECONDS=\"\${GOS_AUDIO_SHOT_HELPER_TIMEOUT_SECONDS:-1}\"' os/hardware-gate/capture-harness/in-session-orchestrator.sh && rg -Fq 'one_second = bytearray()' os/hardware-gate/capture-harness/in-session-orchestrator.sh && rg -Fq 'for _ in range(seconds):' os/hardware-gate/capture-harness/in-session-orchestrator.sh && rg -q 'GOS_AUDIO_CURL_MAX_TIME_SECONDS' os/hardware-gate/capture-harness/in-session-orchestrator.sh && rg -Fq 'GOS_AUDIO_CURL_MAX_TIME_SECONDS:-4' os/hardware-gate/capture-harness/in-session-orchestrator.sh && rg -q 'GOS_AUDIO_STATUS_ATTEMPTS' os/hardware-gate/capture-harness/in-session-orchestrator.sh && rg -q 'GOBLINS_HWGATE_AUDIO_WAV_GENERATION_TIMED_OUT' os/hardware-gate/capture-harness/in-session-orchestrator.sh && rg -Fq 'wav_generated=\$wav_generated' os/hardware-gate/capture-harness/in-session-orchestrator.sh && rg -q 'GOBLINS_OS_WPCTL_TIMEOUT_MS' crates/goblins-os-core/src/audio.rs && rg -q 'clamp_wpctl_timeout_ms' crates/goblins-os-core/src/audio.rs && rg -q 'audio_device_snapshot' crates/goblins-os-core/src/audio.rs && rg -q 'bounded_command_output' crates/goblins-os-core/src/audio.rs && rg -q 'try_wait()' crates/goblins-os-core/src/bounded.rs && rg -q 'WirePlumber did not answer before the audio status timeout.' crates/goblins-os-core/src/audio.rs && rg -q '24-audio-output.png' os/hardware-gate/capture-harness/in-session-orchestrator.sh os/hardware-gate/capture-harness/run-capture.sh os/hardware-gate/close-signoff.sh os/hardware-gate/verify-shipping-status.sh && rg -q 'Audio output checked' os/hardware-gate/close-signoff.sh os/hardware-gate/verify-shipping-status.sh"
check "hardware gate audio proof reports core service diagnostics" "rg -q 'core_probe_http' os/hardware-gate/capture-harness/in-session-orchestrator.sh && rg -q 'audio_core_service_diag' os/hardware-gate/capture-harness/in-session-orchestrator.sh && rg -q 'core_restarts=' os/hardware-gate/capture-harness/in-session-orchestrator.sh && rg -Fq 'Restart=always' os/systemd/goblins-os-core.service && rg -Fq 'StartLimitIntervalSec=0' os/systemd/goblins-os-core.service && rg -q 'GOBLINS_OS_CAPTURE_PRESENT_LEDGER' os/hardware-gate/capture-harness/in-session-orchestrator.sh && rg -q 'GOBLINS_OS_CAPTURE_PRESENT_LEDGER' crates/goblins-os-settings/src/main.rs && rg -Fq 'Restart=always' os/systemd-user/org.goblins.OS.SessionBridge.service && rg -Fq 'StartLimitIntervalSec=0' os/systemd-user/org.goblins.OS.SessionBridge.service"
check "IME CJK engine packages are source-gated" "rg -q 'ibus-libpinyin' os/bootc/Containerfile crates/goblins-os-core/src/input.rs && rg -q 'ibus-anthy' os/bootc/Containerfile crates/goblins-os-core/src/input.rs && rg -q 'ibus-hangul' os/bootc/Containerfile crates/goblins-os-core/src/input.rs && rg -q '/usr/share/ibus/component/libpinyin.xml' os/bootc/Containerfile crates/goblins-os-core/src/input.rs && rg -q '/usr/share/ibus/component/anthy.xml' os/bootc/Containerfile crates/goblins-os-core/src/input.rs && rg -q '/usr/share/ibus/component/hangul.xml' os/bootc/Containerfile crates/goblins-os-core/src/input.rs && rg -q '/usr/libexec/ibus-engine-libpinyin' os/bootc/Containerfile crates/goblins-os-core/src/input.rs && rg -q '/usr/libexec/ibus-engine-anthy' os/bootc/Containerfile crates/goblins-os-core/src/input.rs && rg -q '/usr/libexec/ibus-engine-hangul' os/bootc/Containerfile crates/goblins-os-core/src/input.rs && rg -q '/usr/lib64/gtk-4.0/4.0.0/immodules/libim-ibus.so' os/bootc/Containerfile && rg -q 'CJK engine packages' crates/goblins-os-settings/src/main.rs"
check "core audio probes WirePlumber through the session bridge" "rg -q 'Wpctl' crates/goblins-os-session-bridge/src/main.rs crates/goblins-os-core/src/session_bridge.rs && rg -q 'validate_wpctl_args' crates/goblins-os-session-bridge/src/main.rs && rg -q 'WirePlumber did not answer before the session bridge audio timeout.' crates/goblins-os-session-bridge/src/main.rs && rg -q 'org.gnome.desktop.sound' crates/goblins-os-session-bridge/src/main.rs && rg -q 'gsettings did not answer before the session bridge preference timeout.' crates/goblins-os-session-bridge/src/main.rs && rg -Fq 'pub(crate) fn wpctl' crates/goblins-os-core/src/session_bridge.rs && rg -q 'BRIDGE_IO_TIMEOUT' crates/goblins-os-core/src/session_bridge.rs && rg -q '"list-recursively"' crates/goblins-os-session-bridge/src/main.rs && rg -Fq 'session_bridge::wpctl(args)' crates/goblins-os-core/src/audio.rs && rg -q 'audio_endpoint_ready_without_volume_detail' crates/goblins-os-core/src/audio.rs && rg -Fq 'parse_wpctl_volume(suffix)' crates/goblins-os-core/src/audio.rs && rg -Fq 'session_bridge::gsettings(args)' crates/goblins-os-core/src/audio.rs && rg -Fq ', SOUND_SCHEMA])' crates/goblins-os-core/src/audio.rs && rg -q 'parse_sound_schema_snapshot' crates/goblins-os-core/src/audio.rs && rg -q 'audio_endpoint_default_volume_status' crates/goblins-os-core/src/audio.rs && rg -Fq 'wpctl(&[\"get-volume\", target.wpctl_id()])' crates/goblins-os-core/src/audio.rs && rg -q 'Audio endpoint readiness does not wait for desktop sound preference reads.' crates/goblins-os-core/src/audio.rs"
check "core keyboard rebinding exposes allowlisted write routes" "rg -q '/v1/keyboard/shortcuts/binding' crates/goblins-os-core/src/main.rs && rg -q '/v1/keyboard/modifier-remap' crates/goblins-os-core/src/main.rs && rg -q 'shortcut_conflict' crates/goblins-os-core/src/shortcuts.rs && rg -q 'remap_caps_lock_options' crates/goblins-os-core/src/shortcuts.rs"
check "settings keyboard reports source-gated shortcut bridge" "rg -q 'Protected shortcut writes are source-gated' crates/goblins-os-settings/src/main.rs && rg -q 'Caps Lock to Control is source-gated' crates/goblins-os-settings/src/main.rs"
check "hardware gate requires Keyboard shortcuts roundtrip proof" "rg -q 'keyboard-shortcuts-roundtrip-proof.json' os/hardware-gate/capture-harness/drive-capture.py os/hardware-gate/capture-harness/run-capture.sh os/hardware-gate/close-signoff.sh os/hardware-gate/verify-shipping-status.sh && rg -q '/proof/keyboard-shortcuts-roundtrip' os/hardware-gate/capture-harness/in-session-orchestrator.sh && rg -q '/v1/keyboard/shortcuts/binding' os/hardware-gate/capture-harness/in-session-orchestrator.sh os/hardware-gate/capture-harness/run-capture.sh && rg -q '/v1/keyboard/modifier-remap' os/hardware-gate/capture-harness/in-session-orchestrator.sh os/hardware-gate/capture-harness/run-capture.sh && rg -q 'shortcut_binding=%3CSuper%3E%3CShift%3EH' os/hardware-gate/capture-harness/in-session-orchestrator.sh && rg -q 'shortcut_gsettings_readback=true' os/hardware-gate/capture-harness/in-session-orchestrator.sh os/hardware-gate/capture-harness/run-capture.sh && rg -q 'modifier_gsettings_readback=ctrl:nocaps' os/hardware-gate/capture-harness/in-session-orchestrator.sh && rg -q 'roundtrip_restored=true' os/hardware-gate/capture-harness/in-session-orchestrator.sh os/hardware-gate/capture-harness/run-capture.sh && rg -q 'Keyboard shortcuts roundtrip checked' os/hardware-gate/close-signoff.sh os/hardware-gate/verify-shipping-status.sh"
check "Migration source scan is source-gated" "rg -q '/v1/migration/sources' crates/goblins-os-core/src/main.rs && rg -q 'build_migration_sources' crates/goblins-os-core/src/migration.rs && rg -q 'scan_migration_source_partitions_in' crates/goblins-os-core/src/install_targets.rs && rg -q 'migration_filesystem_readability' crates/goblins-os-core/src/migration.rs && rg -q 'migration_sources_classify_sysfs_partitions_without_mounting' crates/goblins-os-core/src/migration.rs && rg -q '/proc/self/mountinfo' crates/goblins-os-core/src/migration.rs && rg -q 'scan_errors' crates/goblins-os-core/src/migration.rs && rg -q 'partial' crates/goblins-os-core/src/migration.rs && rg -Fq \"Goblins can't read this disk's format (APFS).\" crates/goblins-os-core/src/migration.rs && rg -q 'executes_live_mount: false' crates/goblins-os-core/src/migration.rs && rg -q 'executes_live_copy: false' crates/goblins-os-core/src/migration.rs && rg -Fq 'Migration source scan is ready. No disks were mounted and no files were copied by this source scan.' crates/goblins-os-core/src/migration.rs"
check "Migration copy plan and packages are source-gated" "rg -q '/v1/migration/copy-plan' crates/goblins-os-core/src/main.rs && rg -q 'build_migration_copy_plan' crates/goblins-os-core/src/migration.rs && rg -q -- '--info=progress2' crates/goblins-os-core/src/migration.rs && rg -q -- '--ignore-existing' crates/goblins-os-core/src/migration.rs && rg -q 'executes_live_copy: false' crates/goblins-os-core/src/migration.rs && rg -q 'ntfs-3g' os/bootc/Containerfile && rg -q 'exfatprogs' os/bootc/Containerfile && rg -q 'udisks2' os/bootc/Containerfile && rg -q 'rsync' os/bootc/Containerfile && rg -q 'command -v ntfs-3g' os/bootc/Containerfile && rg -q 'command -v mount.ntfs-3g' os/bootc/Containerfile && rg -q 'command -v fsck.exfat' os/bootc/Containerfile && rg -q 'command -v udisksctl' os/bootc/Containerfile && rg -q 'command -v rsync' os/bootc/Containerfile && rg -q '/usr/lib/systemd/system/udisks2.service' os/bootc/Containerfile"
check "Migration category sizing is source-gated" "rg -q '/v1/migration/estimate' crates/goblins-os-core/src/main.rs && rg -q 'build_migration_estimate' crates/goblins-os-core/src/migration.rs && rg -q 'file_type.is_symlink()' crates/goblins-os-core/src/migration.rs && rg -q 'No files were mounted or copied by this sizing step.' crates/goblins-os-core/src/migration.rs && rg -q 'executes_live_copy: false' crates/goblins-os-core/src/migration.rs"
check "Migration copy job progress substrate is source-gated" "rg -q '/v1/migration/start' crates/goblins-os-core/src/main.rs && rg -q '/v1/migration/progress' crates/goblins-os-core/src/main.rs && rg -q 'build_migration_start_response' crates/goblins-os-core/src/migration.rs && rg -Fq 'Migration copy job is planned. No files were copied by this start substrate.' crates/goblins-os-core/src/migration.rs && rg -Fq 'Live migration copy execution is CI/qemu-gated; this source substrate did not run rsync.' crates/goblins-os-core/src/migration.rs && rg -q 'StatusCode::PRECONDITION_REQUIRED' crates/goblins-os-core/src/migration.rs && rg -q 'OnceLock<Mutex<MigrationCopyProgress>>' crates/goblins-os-core/src/migration.rs && rg -q 'refresh_migration_copy_progress_from_logs' crates/goblins-os-core/src/migration.rs && rg -q 'progress.log' crates/goblins-os-core/src/migration.rs && rg -q 'parse_rsync_progress_line' crates/goblins-os-core/src/migration.rs && rg -q 'parse_migration_ledger_counts' crates/goblins-os-core/src/migration.rs && rg -q 'count_migration_skipped_ledger_entries' crates/goblins-os-core/src/migration.rs && rg -q 'executes_live_copy: false' crates/goblins-os-core/src/migration.rs"
check "Snapshots status substrate is source-gated" "rg -q '/v1/snapshots/status' crates/goblins-os-core/src/main.rs && rg -q '/v1/snapshots/restore' crates/goblins-os-core/src/main.rs && rg -q 'parse_snapper_machine_readable' crates/goblins-os-core/src/snapshots.rs && rg -q '/proc/self/mountinfo' crates/goblins-os-core/src/snapshots.rs && rg -Fq 'Local snapshots need a btrfs /home' crates/goblins-os-core/src/snapshots.rs && rg -q 'executes_restore: false' crates/goblins-os-core/src/snapshots.rs && rg -q 'btrfs-progs' os/bootc/Containerfile && rg -q 'libbtrfsutil' os/bootc/Containerfile && rg -q 'command -v btrfs' os/bootc/Containerfile && rg -q '/v1/snapshots/status' crates/goblins-os-settings/src/main.rs && rg -q 'append_storage_snapshots_status' crates/goblins-os-settings/src/main.rs && rg -q 'append_recovery_snapshots_status' crates/goblins-os-settings/src/main.rs && rg -Fq 'Restore remains CI/qemu-gated' crates/goblins-os-settings/src/main.rs"
check "Encryption posture substrate is source-gated" "rg -q '/v1/security/encryption' crates/goblins-os-core/src/main.rs && rg -q '/proc/self/mountinfo' crates/goblins-os-core/src/encryption.rs && rg -q '/etc/crypttab' crates/goblins-os-core/src/encryption.rs && rg -Fq 'cryptsetup\", &[\"status\"' crates/goblins-os-core/src/encryption.rs && rg -Fq 'systemd-cryptenroll\", &[\"--list\"' crates/goblins-os-core/src/encryption.rs && rg -q 'executes_enrollment: false' crates/goblins-os-core/src/encryption.rs && rg -Fq 'must not enable TPM-only install without escrow' crates/goblins-os-core/src/encryption.rs && rg -q 'cryptsetup' os/bootc/Containerfile && rg -q 'tpm2-tss' os/bootc/Containerfile && rg -q 'command -v cryptsetup' os/bootc/Containerfile && rg -q 'command -v systemd-cryptenroll' os/bootc/Containerfile && rg -q '/usr/lib64/libtss2-esys.so.0' os/bootc/Containerfile && ! rg -q 'systemd-cryptsetup' os/bootc/Containerfile && rg -q '/v1/security/encryption' crates/goblins-os-settings/src/main.rs && rg -q 'append_security_encryption_status' crates/goblins-os-settings/src/main.rs && rg -Fq 'Recovery-key minting and TPM enrollment remain installer and hardware-gated' crates/goblins-os-settings/src/main.rs"
check "Migration preference import plan is source-gated" "rg -q '/v1/migration/preference-plan' crates/goblins-os-core/src/main.rs && rg -q 'build_migration_preference_plan' crates/goblins-os-core/src/migration.rs && rg -Fq 'Migration preference import plan is ready. No preferences were written by this source substrate.' crates/goblins-os-core/src/migration.rs && rg -q 'parse_dconf_dump' crates/goblins-os-core/src/migration.rs && rg -q 'migration_preference_target' crates/goblins-os-core/src/migration.rs && rg -Fq 'Preference is not in the Goblins OS migration allowlist.' crates/goblins-os-core/src/migration.rs && rg -q 'wallpaper_destination_uri_from_copied_paths' crates/goblins-os-core/src/migration.rs && rg -Fq 'Wallpaper file was not present in the copied-path evidence.' crates/goblins-os-core/src/migration.rs && rg -q 'available_schemas: Option<Vec<String>>' crates/goblins-os-core/src/migration.rs && rg -q 'executes_preference_import: false' crates/goblins-os-core/src/migration.rs"
check "core Focus exposes arm disarm and tick routes" "rg -q '/v1/focus/activate' crates/goblins-os-core/src/main.rs && rg -q '/v1/focus/deactivate' crates/goblins-os-core/src/main.rs && rg -q '/v1/focus/tick' crates/goblins-os-core/src/main.rs"
check "core Focus mode and schedule CRUD is source-gated" "rg -q '/v1/focus/mode' crates/goblins-os-core/src/main.rs && rg -q '/v1/focus/schedule' crates/goblins-os-core/src/main.rs && rg -q 'Delete schedules that use this Focus mode before deleting the mode.' crates/goblins-os-core/src/focus.rs && rg -q 'Focus schedules must be saved with a configured mode.' crates/goblins-os-core/src/focus.rs"
check "settings Focus controls source-gated" "rg -q '/v1/focus/status' crates/goblins-os-settings/src/main.rs && rg -q '/v1/focus/activate' crates/goblins-os-settings/src/main.rs && rg -q '/v1/focus/deactivate' crates/goblins-os-settings/src/main.rs && rg -q 'append_focus_settings' crates/goblins-os-settings/src/main.rs"
check "menu-bar Focus indicator source-gated" "rg -q 'org.goblins.os.focus' os/gnome-shell-extensions/goblins-menubar@goblins.os/extension.js && rg -q 'changed::active-mode' os/gnome-shell-extensions/goblins-menubar@goblins.os/extension.js && rg -Fq 'modes.find(entry => entry.id === activeMode)' os/gnome-shell-extensions/goblins-menubar@goblins.os/extension.js && rg -q -- '--panel=notifications' os/gnome-shell-extensions/goblins-menubar@goblins.os/extension.js && rg -q '.goblins-focus-indicator' os/gnome-shell-extensions/goblins-menubar@goblins.os/stylesheet.css"
check "menu-bar Focus render hook is source-gated" "rg -Fq '59b-menubar-focus-\$suffix.png' os/bootc/render-desktop.sh && rg -Fq '[{\"id\":\"work\",\"name\":\"Deep Work\"}]' os/bootc/render-desktop.sh && rg -q 'gsettings set org.goblins.os.focus active-mode work' os/bootc/render-desktop.sh"
check "control center Focus tile source-gated" "rg -Fq '/v1/focus/status' crates/goblins-os-control-center/src/main.rs && ! rg -Fq '/v1/focus/activate' crates/goblins-os-control-center/src/main.rs && ! rg -Fq '/v1/focus/deactivate' crates/goblins-os-control-center/src/main.rs && rg -Fq -- '--panel=notifications' crates/goblins-os-control-center/src/main.rs && rg -Fq 'status.modes' crates/goblins-os-control-center/src/main.rs && rg -Fq 'No Focus modes are configured yet.' crates/goblins-os-control-center/src/main.rs && rg -Fq 'Focus status is unavailable because Goblins OS core did not respond.' crates/goblins-os-control-center/src/main.rs && rg -Fq 'focus_tile_copy' crates/goblins-os-control-center/src/main.rs"
check "control center Focus render hook is source-gated" "rg -Fq '37b-control-center-focus.png' os/bootc/render-screens.sh && rg -Fq '39b-control-center-focus-dark.png' os/bootc/render-screens.sh && rg -Fq '[{\"id\":\"work\",\"name\":\"Deep Work\"}]' os/bootc/render-screens.sh && rg -q 'gsettings set org.goblins.os.focus active-mode work' os/bootc/render-screens.sh && rg -q \"gsettings set org.goblins.os.focus active-mode ''\" os/bootc/render-screens.sh && rg -Fq \"gsettings set org.goblins.os.focus modes '[]'\" os/bootc/render-screens.sh"
check "core Focus snapshots notification banners through bridge" "rg -q 'restore-banners' crates/goblins-os-core/src/focus.rs os/glib-schemas/org.goblins.os.focus.gschema.xml && rg -q 'apply_notification_banners' crates/goblins-os-core/src/focus.rs crates/goblins-os-core/src/notifications.rs && rg -q 'read_notification_banners' crates/goblins-os-core/src/focus.rs crates/goblins-os-core/src/notifications.rs"
check "Focus schedule timer is source-gated" "test -x os/focus/goblins-os-focus-tick && python3 -m py_compile os/focus/goblins-os-focus-tick && rg -q '/v1/focus/tick' os/focus/goblins-os-focus-tick && rg -q 'core URL must be local HTTP' os/focus/goblins-os-focus-tick && test -f os/systemd-user/org.goblins.OS.FocusTick.service && test -f os/systemd-user/org.goblins.OS.FocusTick.timer && rg -q 'ExecStart=/usr/libexec/goblins-os/goblins-os-focus-tick' os/systemd-user/org.goblins.OS.FocusTick.service && rg -q 'OnCalendar=minutely' os/systemd-user/org.goblins.OS.FocusTick.timer && rg -q 'Wants=org.goblins.OS.FocusTick.timer' os/systemd-user/gnome-session@goblins-os.target.d/goblins-os.session.conf && rg -q 'COPY --chmod=0755 os/focus/goblins-os-focus-tick /usr/libexec/goblins-os/goblins-os-focus-tick' os/bootc/Containerfile && rg -q 'command -v python3' os/bootc/Containerfile && rg -q 'os/focus/' os/release/source-tree-manifest.toml"
check "core app privacy exposes allowlisted revoke route" "rg -q '/v1/app-privacy/revoke' crates/goblins-os-core/src/main.rs && rg -q 'PermissionStore.DeletePermission' crates/goblins-os-core/src/app_permissions.rs && rg -q 'session_bridge::permission_store_delete_permission' crates/goblins-os-core/src/app_permissions.rs && rg -q 'permission_id_is_safe' crates/goblins-os-core/src/app_permissions.rs"
check "settings app privacy exposes revoke controls" "rg -q 'app_permission_revoke_row' crates/goblins-os-settings/src/main.rs && rg -q '/v1/app-privacy/revoke' crates/goblins-os-settings/src/main.rs"
check "core display apply exposes serial-gated Mutter route" "rg -q '/v1/displays/apply' crates/goblins-os-core/src/main.rs && rg -q 'ApplyMonitorsConfig' crates/goblins-os-core/src/displays.rs && rg -q 'validate_logical_monitors' crates/goblins-os-core/src/displays.rs && rg -q 'Display layout changed before apply' crates/goblins-os-core/src/displays.rs"
check "settings displays reports protected apply gate" "rg -q 'display_apply_detail' crates/goblins-os-settings/src/main.rs && rg -q 'Protected display apply is available' crates/goblins-os-settings/src/main.rs"
check "capture harness proves multi-display apply guarded route" "rg -q '/proof/multi-display-apply' os/hardware-gate/capture-harness/in-session-orchestrator.sh && rg -q '/v1/displays/status' os/hardware-gate/capture-harness/in-session-orchestrator.sh && rg -q '/v1/displays/apply' os/hardware-gate/capture-harness/in-session-orchestrator.sh && rg -q 'org.gnome.Mutter.DisplayConfig.GetCurrentState' os/hardware-gate/capture-harness/in-session-orchestrator.sh && rg -q 'persistent_confirmation_required=true' os/hardware-gate/capture-harness/in-session-orchestrator.sh && rg -q 'stale_serial_rejected=true' os/hardware-gate/capture-harness/in-session-orchestrator.sh && rg -q 'persistent_keep_claim=false' os/hardware-gate/capture-harness/in-session-orchestrator.sh"
check "capture driver persists multi-display apply proof" "rg -q 'multi-display-apply-proof.json' os/hardware-gate/capture-harness/drive-capture.py os/hardware-gate/capture-harness/run-capture.sh && rg -q 'multi-display-apply' os/hardware-gate/capture-harness/drive-capture.py && rg -q 'HONESTY GUARD: missing or failing multi-display apply proof' os/hardware-gate/capture-harness/run-capture.sh"
check "installed self-test checks firewall status and honest toggle route" "rg -q '/v1/firewall/status' os/bootc/run-selftest.sh && rg -q '/v1/firewall/enabled' os/bootc/run-selftest.sh && rg -Fq '502|503) [ \"\$firewall_toggle_ok\" != \"true\" ]' os/bootc/run-selftest.sh && rg -q 'firewall_toggle_body' os/bootc/run-selftest.sh"
check "settings interaction render captures firewall toggle failure" "rg -q 'capture_settings_firewall_toggle_interaction' os/bootc/render-screens.sh && rg -q '118-settings-firewall-before.png' os/bootc/render-screens.sh && rg -q '119-settings-firewall-toggle-failed.png' os/bootc/render-screens.sh"
check "firewall bridge rule is installed in image-owned polkit path" "rg -q '60-goblins-os-firewall.rules /usr/share/polkit-1/rules.d/60-goblins-os-firewall.rules' os/bootc/Containerfile && rg -q '/usr/share/polkit-1/rules.d/60-goblins-os-firewall.rules' crates/goblins-os-core/src/firewall.rs && rg -q '/usr/bin/systemctl' crates/goblins-os-core/src/firewall.rs"
check "firewall helper waits for firewalld and emits diagnostics" "rg -q 'systemctl reset-failed firewalld.service' os/bootc/goblins-os-firewall && rg -q 'systemctl unmask firewalld.service' os/bootc/goblins-os-firewall && rg -q 'systemctl daemon-reload' os/bootc/goblins-os-firewall && rg -q 'systemctl start firewalld.service || /usr/bin/systemctl restart firewalld.service' os/bootc/goblins-os-firewall && rg -Fq 'while [ \"\$i\" -lt 90 ]' os/bootc/goblins-os-firewall && rg -q 'firewall-cmd --state' os/bootc/goblins-os-firewall && rg -q 'firewalld did not report running after enable' os/bootc/goblins-os-firewall && rg -q 'systemctl --no-pager --full status firewalld.service' os/bootc/goblins-os-firewall && rg -Fq 'wait_for_firewall_state(enabled)' crates/goblins-os-core/src/firewall.rs && rg -q 'for _ in 0..180' crates/goblins-os-core/src/firewall.rs && rg -q 'is-active\", \"--quiet\", \"firewalld.service' crates/goblins-os-core/src/firewall.rs"
check "capture harness proves live firewall polkit toggle path" "rg -q '/proof/firewall-live-toggle' os/hardware-gate/capture-harness/in-session-orchestrator.sh && rg -q '/v1/firewall/enabled' os/hardware-gate/capture-harness/in-session-orchestrator.sh && rg -q 'disable_active=false' os/hardware-gate/capture-harness/in-session-orchestrator.sh && rg -q 'enable_active=true' os/hardware-gate/capture-harness/in-session-orchestrator.sh && rg -Fq 'enable_text=\$(proof_query_value' os/hardware-gate/capture-harness/in-session-orchestrator.sh"
check "capture driver persists live firewall proof" "rg -q 'firewall-live-toggle-proof.json' os/hardware-gate/capture-harness/drive-capture.py os/hardware-gate/capture-harness/run-capture.sh && rg -q 'require_proofs' os/hardware-gate/capture-harness/drive-capture.py && rg -q 'HONESTY GUARD: missing or failing live firewall toggle proof' os/hardware-gate/capture-harness/run-capture.sh"
check "capture driver persists Preview open/render proof" "rg -q 'preview-open-render-proof.json' os/hardware-gate/capture-harness/drive-capture.py os/hardware-gate/capture-harness/run-capture.sh && rg -q 'preview-open-render' os/hardware-gate/capture-harness/drive-capture.py && rg -q 'HONESTY GUARD: missing or failing Preview open/render proof' os/hardware-gate/capture-harness/run-capture.sh"
check "capture driver persists Focus arm roundtrip proof" "rg -q 'focus-arm-roundtrip-proof.json' os/hardware-gate/capture-harness/drive-capture.py os/hardware-gate/capture-harness/run-capture.sh && rg -q 'focus-arm-roundtrip' os/hardware-gate/capture-harness/drive-capture.py && rg -q 'HONESTY GUARD: missing or failing Focus arm roundtrip proof' os/hardware-gate/capture-harness/run-capture.sh"
check "capture driver persists App privacy revoke proof" "rg -q 'app-privacy-revoke-proof.json' os/hardware-gate/capture-harness/drive-capture.py os/hardware-gate/capture-harness/run-capture.sh && rg -q 'app-privacy-revoke' os/hardware-gate/capture-harness/drive-capture.py && rg -q 'HONESTY GUARD: missing or failing App privacy revoke proof' os/hardware-gate/capture-harness/run-capture.sh"
check "capture harness proves Text Shortcuts session plumbing without runtime claim" "rg -q '/proof/text-shortcuts-session-enable' os/hardware-gate/capture-harness/in-session-orchestrator.sh && rg -q 'TEXT_SHORTCUTS_IBUS_SERVICE=org.freedesktop.IBus.session.GNOME.service' os/hardware-gate/capture-harness/in-session-orchestrator.sh && rg -Fq 'service_unit=\$TEXT_SHORTCUTS_IBUS_SERVICE' os/hardware-gate/capture-harness/in-session-orchestrator.sh && rg -Fq 'systemctl --user restart \"\$TEXT_SHORTCUTS_IBUS_SERVICE\"' os/hardware-gate/capture-harness/in-session-orchestrator.sh && rg -q 'ibus engine goblins-textshortcuts' os/hardware-gate/capture-harness/in-session-orchestrator.sh && rg -q 'ensure_textshortcuts_ibus_component' os/hardware-gate/capture-harness/in-session-orchestrator.sh && rg -q 'wait_ibus_cli_ready' os/hardware-gate/capture-harness/in-session-orchestrator.sh && rg -q 'wait_ibus_bus_owned' os/hardware-gate/capture-harness/in-session-orchestrator.sh && rg -q 'user_component_seeded=true' os/hardware-gate/capture-harness/in-session-orchestrator.sh && rg -q 'list_error=' os/hardware-gate/capture-harness/in-session-orchestrator.sh && rg -q 'bus_owner=' os/hardware-gate/capture-harness/in-session-orchestrator.sh && rg -q 'service_diag=' os/hardware-gate/capture-harness/in-session-orchestrator.sh && rg -q 'daemon_process=' os/hardware-gate/capture-harness/in-session-orchestrator.sh && rg -q 'session_env=' os/hardware-gate/capture-harness/in-session-orchestrator.sh && ! test -f os/systemd-user/org.goblins.OS.IBus.service && rg -q 'Wants=org.freedesktop.IBus.session.GNOME.service' os/systemd-user/gnome-session@goblins-os.target.d/goblins-os.session.conf && rg -q 'Before=org.freedesktop.IBus.session.GNOME.service' os/systemd-user/org.goblins.OS.InputSourcesSeed.service && ! rg -q 'org.goblins.OS.IBus.service' os/systemd-user/gnome-session@goblins-os.target.d/goblins-os.session.conf os/systemd-user/org.goblins.OS.InputSourcesSeed.service os/hardware-gate/capture-harness/in-session-orchestrator.sh && ! rg -Fq 'application.run_with_args(&[\"goblins-os-shell\", \"--text-shortcuts-proof\"]);' crates/goblins-os-shell/src/main.rs && rg -q 'systemctl --user import-environment' os/session/goblins-os-session && rg -q 'dbus-update-activation-environment --systemd' os/session/goblins-os-session && rg -q 'WAYLAND_DISPLAY' os/session/goblins-os-session && rg -q '/var/home/goblin/.local/share/ibus/component/goblins-textshortcuts.xml' os/bootc/Containerfile && rg -Fq 'core_engine_available=true&core_runtime_loop_available=true&runtime_ready_claim=true' os/hardware-gate/capture-harness/in-session-orchestrator.sh && rg -q 'runtime_ready_claim=false' os/hardware-gate/capture-harness/in-session-orchestrator.sh"
check "capture driver persists Text Shortcuts session proof" "rg -q 'text-shortcuts-session-enable-proof.json' os/hardware-gate/capture-harness/drive-capture.py os/hardware-gate/capture-harness/run-capture.sh && rg -q 'text-shortcuts-session-enable' os/hardware-gate/capture-harness/drive-capture.py && rg -q 'HONESTY GUARD: missing or failing Text Shortcuts session-enable proof' os/hardware-gate/capture-harness/run-capture.sh"
check "Text Shortcuts one-shot input source seed is source-gated" "test -x os/input/goblins-os-input-source-seed && rg -q 'input-source-seeded' os/input/goblins-os-input-source-seed && rg -q 'gsettings set org.gnome.desktop.input-sources sources' os/input/goblins-os-input-source-seed && rg -q 'gsettings set org.freedesktop.ibus.general preload-engines' os/input/goblins-os-input-source-seed && rg -q 'COPY --chmod=0755 os/input/goblins-os-input-source-seed /usr/libexec/goblins-os/goblins-os-input-source-seed' os/bootc/Containerfile && rg -q 'bash -n /usr/libexec/goblins-os/goblins-os-input-source-seed' os/bootc/Containerfile && rg -q 'Wants=org.goblins.OS.InputSourcesSeed.service' os/systemd-user/gnome-session@goblins-os.target.d/goblins-os.session.conf && rg -q 'Wants=org.freedesktop.IBus.session.GNOME.service' os/systemd-user/gnome-session@goblins-os.target.d/goblins-os.session.conf && rg -q 'Before=org.freedesktop.IBus.session.GNOME.service' os/systemd-user/org.goblins.OS.InputSourcesSeed.service"
check "capture harness retired superseded Text Shortcuts live keystroke proof" "! rg -q '/proof/text-shortcuts-live-keystroke|text_shortcuts_live_keystroke_proof|proof_text_shortcuts_live[[:space:]]*\\(\\)' os/hardware-gate/capture-harness/in-session-orchestrator.sh && ! rg -q 'text-shortcuts-live-keystroke|TEXT_SHORTCUTS_LIVE_PROOF|HONESTY GUARD: missing or failing Text Shortcuts live keystroke proof' os/hardware-gate/capture-harness/drive-capture.py os/hardware-gate/capture-harness/run-capture.sh"
check "Text Shortcuts live keystrokes are covered by the runtime/render proof" "rg -q '/proof/text-shortcuts-live-ibus-runtime-render' os/hardware-gate/capture-harness/in-session-orchestrator.sh && rg -q 'normal_actual=onmyway\\.' os/hardware-gate/capture-harness/in-session-orchestrator.sh && rg -q 'passthrough_actual=hello\\.' os/hardware-gate/capture-harness/in-session-orchestrator.sh && rg -q 'password_refusal=true' os/hardware-gate/capture-harness/in-session-orchestrator.sh && rg -q 'focused_field_callback=true' os/hardware-gate/capture-harness/in-session-orchestrator.sh && rg -q 'text_input_v3_commit=true' os/hardware-gate/capture-harness/in-session-orchestrator.sh && rg -q 'rendered_accept_bubble=true' os/hardware-gate/capture-harness/in-session-orchestrator.sh && rg -q '32-text-shortcuts-live-ibus-runtime-render.png' os/hardware-gate/capture-harness/in-session-orchestrator.sh os/hardware-gate/capture-harness/run-capture.sh"
check "capture harness proves Text Shortcuts candidate metadata without live overlay claim" "rg -q '/proof/text-shortcuts-candidate-metadata' os/hardware-gate/capture-harness/in-session-orchestrator.sh && rg -q 'goblins-os-shell\" --text-shortcuts-proof candidate' os/hardware-gate/capture-harness/in-session-orchestrator.sh && rg -q 'replacement=on my way' os/hardware-gate/capture-harness/in-session-orchestrator.sh && rg -q 'accept_on=word-boundary' os/hardware-gate/capture-harness/in-session-orchestrator.sh && rg -q 'dismiss_key=Escape' os/hardware-gate/capture-harness/in-session-orchestrator.sh && rg -q 'rendered_bubble_ready_claim=false' os/hardware-gate/capture-harness/in-session-orchestrator.sh && rg -q 'live_overlay_claim=false' os/hardware-gate/capture-harness/in-session-orchestrator.sh && rg -q 'runtime_ready_claim=false' os/hardware-gate/capture-harness/in-session-orchestrator.sh"
check "capture driver persists Text Shortcuts candidate metadata proof" "rg -q 'text-shortcuts-candidate-metadata-proof.json' os/hardware-gate/capture-harness/drive-capture.py os/hardware-gate/capture-harness/run-capture.sh && rg -q 'text-shortcuts-candidate-metadata' os/hardware-gate/capture-harness/drive-capture.py && rg -q 'HONESTY GUARD: missing or failing Text Shortcuts candidate metadata proof' os/hardware-gate/capture-harness/run-capture.sh"
check "capture harness proves Text Shortcuts overlay intent without live overlay claim" "rg -q '/proof/text-shortcuts-overlay-intent' os/hardware-gate/capture-harness/in-session-orchestrator.sh && rg -q -- '--overlay-intent-self-test' os/hardware-gate/capture-harness/in-session-orchestrator.sh && rg -q 'goblins-textshortcuts-ibus-adapter-overlay-intent' os/hardware-gate/capture-harness/in-session-orchestrator.sh && rg -q 'show_count=2' os/hardware-gate/capture-harness/in-session-orchestrator.sh && rg -q 'hide_count=2' os/hardware-gate/capture-harness/in-session-orchestrator.sh && rg -q 'dismissed_reason=true' os/hardware-gate/capture-harness/in-session-orchestrator.sh && rg -q 'committed_reason=true' os/hardware-gate/capture-harness/in-session-orchestrator.sh && rg -q 'live_overlay_claim=false' os/hardware-gate/capture-harness/in-session-orchestrator.sh && rg -q 'runtime_ready_claim=false' os/hardware-gate/capture-harness/in-session-orchestrator.sh"
check "capture driver persists Text Shortcuts overlay intent proof" "rg -q 'text-shortcuts-overlay-intent-proof.json' os/hardware-gate/capture-harness/drive-capture.py os/hardware-gate/capture-harness/run-capture.sh && rg -q 'text-shortcuts-overlay-intent' os/hardware-gate/capture-harness/drive-capture.py && rg -q 'HONESTY GUARD: missing or failing Text Shortcuts overlay-intent proof' os/hardware-gate/capture-harness/run-capture.sh"
check "capture harness proves Text Shortcuts candidate bubble frame without live render claim" "rg -q '/proof/text-shortcuts-candidate-bubble-frame' os/hardware-gate/capture-harness/in-session-orchestrator.sh && rg -q -- '--candidate-bubble-frame-self-test' os/hardware-gate/capture-harness/in-session-orchestrator.sh && rg -q 'goblins-textshortcuts-accept-bubble-frame' os/hardware-gate/capture-harness/in-session-orchestrator.sh && rg -q 'show_frame_count=2' os/hardware-gate/capture-harness/in-session-orchestrator.sh && rg -q 'hide_frame_count=2' os/hardware-gate/capture-harness/in-session-orchestrator.sh && rg -q 'style_class=gos-text-shortcuts-candidate' os/hardware-gate/capture-harness/in-session-orchestrator.sh && rg -q 'font_family=Inter' os/hardware-gate/capture-harness/in-session-orchestrator.sh && rg -q 'sensitive_field_refusal=true' os/hardware-gate/capture-harness/in-session-orchestrator.sh && rg -q 'rendered_bubble_ready_claim=false' os/hardware-gate/capture-harness/in-session-orchestrator.sh && rg -q 'live_overlay_claim=false' os/hardware-gate/capture-harness/in-session-orchestrator.sh && rg -q 'runtime_ready_claim=false' os/hardware-gate/capture-harness/in-session-orchestrator.sh"
check "capture driver persists Text Shortcuts candidate bubble frame proof" "rg -q 'text-shortcuts-candidate-bubble-frame-proof.json' os/hardware-gate/capture-harness/drive-capture.py os/hardware-gate/capture-harness/run-capture.sh && rg -q 'text-shortcuts-candidate-bubble-frame' os/hardware-gate/capture-harness/drive-capture.py && rg -q 'HONESTY GUARD: missing or failing Text Shortcuts candidate-bubble-frame proof' os/hardware-gate/capture-harness/run-capture.sh"
check "capture harness proves Text Shortcuts candidate bubble layout without live render claim" "rg -q '/proof/text-shortcuts-candidate-bubble-layout' os/hardware-gate/capture-harness/in-session-orchestrator.sh && rg -q -- '--candidate-bubble-layout-self-test' os/hardware-gate/capture-harness/in-session-orchestrator.sh && rg -q 'goblins-textshortcuts-accept-bubble-layout' os/hardware-gate/capture-harness/in-session-orchestrator.sh && rg -q 'frame_surface=goblins-textshortcuts-accept-bubble-frame' os/hardware-gate/capture-harness/in-session-orchestrator.sh && rg -q 'layout_count=4' os/hardware-gate/capture-harness/in-session-orchestrator.sh && rg -q 'visible_layout_count=3' os/hardware-gate/capture-harness/in-session-orchestrator.sh && rg -q 'right_edge_clamped=true' os/hardware-gate/capture-harness/in-session-orchestrator.sh && rg -q 'bottom_edge_flipped=true' os/hardware-gate/capture-harness/in-session-orchestrator.sh && rg -q 'hidden_frame_collapses=true' os/hardware-gate/capture-harness/in-session-orchestrator.sh && rg -q 'style_class=gos-text-shortcuts-candidate' os/hardware-gate/capture-harness/in-session-orchestrator.sh && rg -q 'font_family=Inter' os/hardware-gate/capture-harness/in-session-orchestrator.sh && rg -q 'rendered_bubble_ready_claim=false' os/hardware-gate/capture-harness/in-session-orchestrator.sh && rg -q 'live_overlay_claim=false' os/hardware-gate/capture-harness/in-session-orchestrator.sh && rg -q 'runtime_ready_claim=false' os/hardware-gate/capture-harness/in-session-orchestrator.sh"
check "capture driver persists Text Shortcuts candidate bubble layout proof" "rg -q 'text-shortcuts-candidate-bubble-layout-proof.json' os/hardware-gate/capture-harness/drive-capture.py os/hardware-gate/capture-harness/run-capture.sh && rg -q 'text-shortcuts-candidate-bubble-layout' os/hardware-gate/capture-harness/drive-capture.py && rg -q 'HONESTY GUARD: missing or failing Text Shortcuts candidate-bubble-layout proof' os/hardware-gate/capture-harness/run-capture.sh"
check "capture harness proves Text Shortcuts candidate bubble render intent without live render claim" "rg -q '/proof/text-shortcuts-candidate-bubble-render-intent' os/hardware-gate/capture-harness/in-session-orchestrator.sh && rg -q -- '--candidate-bubble-render-intent-self-test' os/hardware-gate/capture-harness/in-session-orchestrator.sh && rg -q 'goblins-textshortcuts-accept-bubble-render-intent' os/hardware-gate/capture-harness/in-session-orchestrator.sh && rg -q 'frame_surface=goblins-textshortcuts-accept-bubble-frame' os/hardware-gate/capture-harness/in-session-orchestrator.sh && rg -q 'layout_surface=goblins-textshortcuts-accept-bubble-layout' os/hardware-gate/capture-harness/in-session-orchestrator.sh && rg -q 'render_intent_count=8' os/hardware-gate/capture-harness/in-session-orchestrator.sh && rg -q 'show_intent_count=4' os/hardware-gate/capture-harness/in-session-orchestrator.sh && rg -q 'hide_intent_count=4' os/hardware-gate/capture-harness/in-session-orchestrator.sh && rg -q 'focus_out_hide=true' os/hardware-gate/capture-harness/in-session-orchestrator.sh && rg -q 'sensitive_hide=true' os/hardware-gate/capture-harness/in-session-orchestrator.sh && rg -q 'pass_through_unchanged=true' os/hardware-gate/capture-harness/in-session-orchestrator.sh && rg -q 'sink_failure_fail_open=true' os/hardware-gate/capture-harness/in-session-orchestrator.sh && rg -q 'style_class=gos-text-shortcuts-candidate' os/hardware-gate/capture-harness/in-session-orchestrator.sh && rg -q 'font_family=Inter' os/hardware-gate/capture-harness/in-session-orchestrator.sh && rg -q 'rendered_bubble_ready_claim=false' os/hardware-gate/capture-harness/in-session-orchestrator.sh && rg -q 'live_overlay_claim=false' os/hardware-gate/capture-harness/in-session-orchestrator.sh && rg -q 'runtime_ready_claim=false' os/hardware-gate/capture-harness/in-session-orchestrator.sh"
check "capture driver persists Text Shortcuts candidate bubble render intent proof" "rg -q 'text-shortcuts-candidate-bubble-render-intent-proof.json' os/hardware-gate/capture-harness/drive-capture.py os/hardware-gate/capture-harness/run-capture.sh && rg -q 'text-shortcuts-candidate-bubble-render-intent' os/hardware-gate/capture-harness/drive-capture.py && rg -q 'HONESTY GUARD: missing or failing Text Shortcuts candidate-bubble-render-intent proof' os/hardware-gate/capture-harness/run-capture.sh"
check "capture harness proves Text Shortcuts candidate bubble rendered screenshot without live claim" "rg -q '/proof/text-shortcuts-candidate-bubble-render' os/hardware-gate/capture-harness/in-session-orchestrator.sh && rg -q -- '--text-shortcuts-proof candidate-render' os/hardware-gate/capture-harness/in-session-orchestrator.sh && rg -q '31-text-shortcuts-candidate-bubble-render' os/hardware-gate/capture-harness/in-session-orchestrator.sh os/hardware-gate/capture-harness/run-capture.sh os/hardware-gate/close-signoff.sh os/hardware-gate/verify-shipping-status.sh && rg -q 'render_intent_surface=goblins-textshortcuts-accept-bubble-render-intent' os/hardware-gate/capture-harness/in-session-orchestrator.sh && rg -q 'rendered_candidate_surface=true' os/hardware-gate/capture-harness/in-session-orchestrator.sh os/hardware-gate/capture-harness/run-capture.sh && rg -q 'style_class=gos-text-shortcuts-candidate' os/hardware-gate/capture-harness/in-session-orchestrator.sh os/hardware-gate/capture-harness/run-capture.sh && rg -q 'font_family=Inter' os/hardware-gate/capture-harness/in-session-orchestrator.sh os/hardware-gate/capture-harness/run-capture.sh && rg -q 'rendered_bubble_ready_claim=false' os/hardware-gate/capture-harness/in-session-orchestrator.sh os/hardware-gate/capture-harness/run-capture.sh && rg -q 'live_overlay_claim=false' os/hardware-gate/capture-harness/in-session-orchestrator.sh os/hardware-gate/capture-harness/run-capture.sh && rg -q 'runtime_ready_claim=false' os/hardware-gate/capture-harness/in-session-orchestrator.sh os/hardware-gate/capture-harness/run-capture.sh"
check "capture driver persists Text Shortcuts candidate bubble rendered screenshot proof" "rg -q 'text-shortcuts-candidate-bubble-render-proof.json' os/hardware-gate/capture-harness/drive-capture.py os/hardware-gate/capture-harness/run-capture.sh && rg -q 'text-shortcuts-candidate-bubble-render' os/hardware-gate/capture-harness/drive-capture.py && rg -q 'HONESTY GUARD: missing or failing Text Shortcuts candidate-bubble-render screenshot proof' os/hardware-gate/capture-harness/run-capture.sh"
check "capture harness drives Text Shortcuts live IBus runtime/render proof" "rg -q '/proof/text-shortcuts-live-ibus-runtime-render' os/hardware-gate/capture-harness/in-session-orchestrator.sh && rg -q -- '--text-shortcuts-proof live-runtime-render' os/hardware-gate/capture-harness/in-session-orchestrator.sh && rg -q 'GOBLINS_TEXTSHORTCUTS_PROOF_EVENTS' os/hardware-gate/capture-harness/in-session-orchestrator.sh && rg -q 'systemctl --user set-environment GOBLINS_TEXTSHORTCUTS_PROOF_EVENTS' os/hardware-gate/capture-harness/in-session-orchestrator.sh && rg -q 'host_focus_text_shortcuts_field runtime-render-focus' os/hardware-gate/capture-harness/in-session-orchestrator.sh && rg -q 'render_log_tail' os/hardware-gate/capture-harness/in-session-orchestrator.sh && rg -q 'ledger_tail' os/hardware-gate/capture-harness/in-session-orchestrator.sh && rg -q 'goblins-textshortcuts-live-ibus-runtime-render' os/hardware-gate/capture-harness/in-session-orchestrator.sh os/hardware-gate/capture-harness/run-capture.sh && rg -q '32-text-shortcuts-live-ibus-runtime-render.png' os/hardware-gate/capture-harness/in-session-orchestrator.sh os/hardware-gate/capture-harness/run-capture.sh os/hardware-gate/close-signoff.sh os/hardware-gate/verify-shipping-status.sh && rg -q 'focused_field_callback' os/hardware-gate/capture-harness/in-session-orchestrator.sh os/hardware-gate/capture-harness/run-capture.sh && rg -q 'text_input_v3_commit' os/hardware-gate/capture-harness/in-session-orchestrator.sh os/hardware-gate/capture-harness/run-capture.sh && rg -q 'rendered_accept_bubble' os/hardware-gate/capture-harness/in-session-orchestrator.sh os/hardware-gate/capture-harness/run-capture.sh && rg -q 'rendered_bubble_ready_claim=true' os/hardware-gate/capture-harness/in-session-orchestrator.sh && rg -q 'live_overlay_claim=true' os/hardware-gate/capture-harness/in-session-orchestrator.sh && rg -q 'runtime_ready_claim=true' os/hardware-gate/capture-harness/in-session-orchestrator.sh && rg -q 'core_readiness_flip=live' os/hardware-gate/capture-harness/in-session-orchestrator.sh && rg -q '\"core_readiness_flip\": \"live\"' os/hardware-gate/capture-harness/run-capture.sh && ! rg -q 'live-ibus-runtime-render-not-implemented' os/hardware-gate/capture-harness/in-session-orchestrator.sh"
check "capture driver persists Text Shortcuts live IBus runtime/render proof" "rg -q 'text-shortcuts-live-ibus-runtime-render-proof.json' os/hardware-gate/capture-harness/drive-capture.py os/hardware-gate/capture-harness/run-capture.sh && rg -q 'text-shortcuts-live-ibus-runtime-render' os/hardware-gate/capture-harness/drive-capture.py && rg -q 'HONESTY GUARD: missing or failing Text Shortcuts live IBus runtime/render proof' os/hardware-gate/capture-harness/run-capture.sh"
check "capture harness turns Switch Control off before ordinary screenshots" "rg -Fq 'switch_control_off(){' os/hardware-gate/capture-harness/in-session-orchestrator.sh && rg -Fq 'gsettings set org.goblins.os.a11y.switch-control enabled false' os/hardware-gate/capture-harness/in-session-orchestrator.sh && rg -Fq 'goblinsSwitchControl.hide' os/hardware-gate/capture-harness/in-session-orchestrator.sh && rg -Fq 'this._stopScanner();' os/gnome-shell-extensions/goblins-switch@goblins.os/extension.js && rg -Fq 'switch_control_off' os/hardware-gate/capture-harness/in-session-orchestrator.sh"
check "Text Shortcuts accept-bubble frame contract is source-gated" "rg -q -- '--candidate-bubble-frame-self-test' os/goblins-os-textshortcuts/goblins-textshortcuts-ibus os/bootc/Containerfile && rg -q 'goblins-textshortcuts-accept-bubble-frame' os/goblins-os-textshortcuts/goblins-textshortcuts-ibus os/bootc/Containerfile && rg -q 'goblins-textshortcuts-candidate-bubble-frame.json' os/bootc/Containerfile && rg -q 'show_frame_count' os/goblins-os-textshortcuts/goblins-textshortcuts-ibus os/bootc/Containerfile && rg -q 'hide_frame_count' os/goblins-os-textshortcuts/goblins-textshortcuts-ibus os/bootc/Containerfile && rg -q 'dismissed_frame' os/goblins-os-textshortcuts/goblins-textshortcuts-ibus os/bootc/Containerfile && rg -q 'committed_frame' os/goblins-os-textshortcuts/goblins-textshortcuts-ibus os/bootc/Containerfile && rg -q 'sensitive_field_refusal' os/goblins-os-textshortcuts/goblins-textshortcuts-ibus os/bootc/Containerfile && rg -q 'gos-text-shortcuts-candidate' os/goblins-os-textshortcuts/goblins-textshortcuts-ibus os/bootc/Containerfile && rg -q 'rendered_bubble_ready_claim' os/goblins-os-textshortcuts/goblins-textshortcuts-ibus os/bootc/Containerfile && rg -q 'live_overlay_claim' os/goblins-os-textshortcuts/goblins-textshortcuts-ibus os/bootc/Containerfile && rg -q 'runtime_ready_claim' os/goblins-os-textshortcuts/goblins-textshortcuts-ibus os/bootc/Containerfile"
check "Text Shortcuts accept-bubble layout contract is source-gated" "rg -q -- '--candidate-bubble-layout-self-test' os/goblins-os-textshortcuts/goblins-textshortcuts-ibus os/bootc/Containerfile && rg -q 'goblins-textshortcuts-accept-bubble-layout' os/goblins-os-textshortcuts/goblins-textshortcuts-ibus os/bootc/Containerfile && rg -q 'goblins-textshortcuts-candidate-bubble-layout.json' os/bootc/Containerfile && rg -q 'layout_count' os/goblins-os-textshortcuts/goblins-textshortcuts-ibus os/bootc/Containerfile && rg -q 'visible_layout_count' os/goblins-os-textshortcuts/goblins-textshortcuts-ibus os/bootc/Containerfile && rg -q 'right_edge_clamped' os/goblins-os-textshortcuts/goblins-textshortcuts-ibus os/bootc/Containerfile && rg -q 'bottom_edge_flipped' os/goblins-os-textshortcuts/goblins-textshortcuts-ibus os/bootc/Containerfile && rg -q 'hidden_frame_collapses' os/goblins-os-textshortcuts/goblins-textshortcuts-ibus os/bootc/Containerfile && rg -q 'gos-text-shortcuts-candidate' os/goblins-os-textshortcuts/goblins-textshortcuts-ibus os/bootc/Containerfile && rg -q 'font_family' os/goblins-os-textshortcuts/goblins-textshortcuts-ibus os/bootc/Containerfile && rg -q 'rendered_bubble_ready_claim' os/goblins-os-textshortcuts/goblins-textshortcuts-ibus os/bootc/Containerfile && rg -q 'live_overlay_claim' os/goblins-os-textshortcuts/goblins-textshortcuts-ibus os/bootc/Containerfile && rg -q 'runtime_ready_claim' os/goblins-os-textshortcuts/goblins-textshortcuts-ibus os/bootc/Containerfile"
check "Text Shortcuts accept-bubble render-intent bridge is source-gated" "rg -q -- '--candidate-bubble-render-intent-self-test' os/goblins-os-textshortcuts/goblins-textshortcuts-ibus os/bootc/Containerfile && rg -q 'goblins-textshortcuts-accept-bubble-render-intent' os/goblins-os-textshortcuts/goblins-textshortcuts-ibus os/bootc/Containerfile && rg -q 'CandidateBubbleRenderIntentController' os/goblins-os-textshortcuts/goblins-textshortcuts-ibus && rg -q 'CandidateBubbleRenderIntentSink' os/goblins-os-textshortcuts/goblins-textshortcuts-ibus && rg -q '_apply_response_operations_with_render_intent' os/goblins-os-textshortcuts/goblins-textshortcuts-ibus && rg -q 'goblins-textshortcuts-candidate-bubble-render-intent.json' os/bootc/Containerfile && rg -q 'render_intent_count' os/goblins-os-textshortcuts/goblins-textshortcuts-ibus os/bootc/Containerfile && rg -q 'show_intent_count' os/goblins-os-textshortcuts/goblins-textshortcuts-ibus os/bootc/Containerfile && rg -q 'hide_intent_count' os/goblins-os-textshortcuts/goblins-textshortcuts-ibus os/bootc/Containerfile && rg -q 'focus_out_hide' os/goblins-os-textshortcuts/goblins-textshortcuts-ibus os/bootc/Containerfile && rg -q 'sensitive_hide' os/goblins-os-textshortcuts/goblins-textshortcuts-ibus os/bootc/Containerfile && rg -q 'pass_through_unchanged' os/goblins-os-textshortcuts/goblins-textshortcuts-ibus os/bootc/Containerfile && rg -q 'sink_failure_fail_open' os/goblins-os-textshortcuts/goblins-textshortcuts-ibus os/bootc/Containerfile && rg -q 'gos-text-shortcuts-candidate' os/goblins-os-textshortcuts/goblins-textshortcuts-ibus os/bootc/Containerfile && rg -q 'font_family' os/goblins-os-textshortcuts/goblins-textshortcuts-ibus os/bootc/Containerfile && rg -q 'rendered_bubble_ready_claim' os/goblins-os-textshortcuts/goblins-textshortcuts-ibus os/bootc/Containerfile && rg -q 'live_overlay_claim' os/goblins-os-textshortcuts/goblins-textshortcuts-ibus os/bootc/Containerfile && rg -q 'runtime_ready_claim' os/goblins-os-textshortcuts/goblins-textshortcuts-ibus os/bootc/Containerfile"
check "capture harness prints qemu diagnostics on startup failure" "rg -q 'QEMU startup diagnostics' os/hardware-gate/capture-harness/run-capture.sh && rg -q 'qemu.log' os/hardware-gate/capture-harness/run-capture.sh && rg -q 'serial.log' os/hardware-gate/capture-harness/run-capture.sh && rg -q 'last connection error' os/hardware-gate/capture-harness/drive-capture.py"
check "capture driver fail-closes on serial VM stages and diagnostic frames" "rg -q 'GOS_SERIALLOG' os/hardware-gate/capture-harness/run-capture.sh os/hardware-gate/capture-harness/drive-capture.py && rg -Fq 'wait_serial_contains(\"ISO boot menu\"' os/hardware-gate/capture-harness/drive-capture.py && rg -Fq 'observe_serial_contains(\"ISO boot handoff\"' os/hardware-gate/capture-harness/drive-capture.py && rg -q 'continuing to framebuffer stages' os/hardware-gate/capture-harness/drive-capture.py && rg -Fq 'key(\"ret\")' os/hardware-gate/capture-harness/drive-capture.py && rg -q 'Anaconda automated kickstart progress' os/hardware-gate/capture-harness/drive-capture.py && rg -Fq '\"kickstart install post\"' os/hardware-gate/capture-harness/drive-capture.py && rg -q 'GOBLINS_VERIFY_INSTALL_DONE' os/hardware-gate/capture-harness/drive-capture.py os/iso/verify-config.toml && rg -Fq 'wait_stage(\"first boot desktop\"' os/hardware-gate/capture-harness/drive-capture.py && rg -q 'diagnostic framebuffer samples' os/hardware-gate/capture-harness/drive-capture.py && rg -q '_debug-' os/hardware-gate/capture-harness/drive-capture.py && ! rg -q 'Anaconda destination disk selected' os/hardware-gate/capture-harness/drive-capture.py && ! rg -q 'click(0.937, 0.895)' os/hardware-gate/capture-harness/drive-capture.py && ! rg -q 'require_frame\\(' os/hardware-gate/capture-harness/drive-capture.py && ! rg -q 'wait_frame\\(' os/hardware-gate/capture-harness/drive-capture.py"
check "capture harness retries transient install boot hangs with fresh VM state" "rg -Fq 'GOS_INSTALL_POST_TIMEOUT' os/hardware-gate/capture-harness/drive-capture.py && rg -Fq 'exit_code=INSTALL_POST_TIMEOUT_EXIT' os/hardware-gate/capture-harness/drive-capture.py && rg -Fq 'GOS_CAPTURE_MAX_ATTEMPTS' os/hardware-gate/capture-harness/run-capture.sh && rg -Fq 'prepare_vm_state \"\$attempt\"' os/hardware-gate/capture-harness/run-capture.sh && rg -Fq 'copy_capture_logs \"attempt-\$attempt\"' os/hardware-gate/capture-harness/run-capture.sh && rg -Fq 'stalled before kickstart marker; retrying with fresh VM state' os/hardware-gate/capture-harness/run-capture.sh && rg -Fq 'driver_rc\" -eq \"\$INSTALL_TIMEOUT_RC' os/hardware-gate/capture-harness/run-capture.sh"
check "capture driver completes first boot through private core path with debug frame" "rg -q 'first boot setup: completing private offline path through session core APIs' os/hardware-gate/capture-harness/drive-capture.py && rg -q 'post first boot private unlock' os/hardware-gate/capture-harness/drive-capture.py && rg -q 'firstboot-unlock.sh' os/hardware-gate/capture-harness/run-capture.sh && rg -q '/v1/privacy' os/hardware-gate/capture-harness/firstboot-unlock.sh && rg -q '/v1/installer/complete' os/hardware-gate/capture-harness/firstboot-unlock.sh && rg -q '/v1/session/unlock' os/hardware-gate/capture-harness/firstboot-unlock.sh && rg -q '/ready/FIRSTBOOT_UNLOCK' os/hardware-gate/capture-harness/firstboot-unlock.sh && rg -q 'first boot private unlock callback' os/hardware-gate/capture-harness/drive-capture.py"
check "hardware gate session automation uses verification-only service not Alt+F2 injection" "rg -q 'goblins-hwgate-session-orchestrator.service' os/iso/verify-config.toml && rg -q '99-goblins-hwgate-session-orchestrator.conf' os/iso/verify-config.toml && rg -q 'WantedBy=default.target' os/iso/verify-config.toml && rg -q 'systemctl --global enable goblins-hwgate-session-orchestrator.service' os/iso/verify-config.toml && rg -q '/etc/xdg/autostart/goblins-hwgate-session-orchestrator.desktop' os/iso/verify-config.toml && rg -q 'Exec=/etc/goblins-os/hardware-gate/goblins-hwgate-session-orchestrator' os/iso/verify-config.toml && rg -q '/etc/goblins-os/hardware-gate/goblins-hwgate-start-session-orchestrator' os/iso/verify-config.toml && rg -q 'GOBLINS_HWGATE_ETC_HELPERS_INSTALLED' os/iso/verify-config.toml && ! rg -q '/usr/libexec/goblins-hwgate' os/iso/verify-config.toml && rg -q 'multi-user.target.wants/goblins-hwgate-firstboot-diagnostics.service' os/iso/verify-config.toml && rg -q 'graphical.target.wants/goblins-hwgate-session-orchestrator-starter.service' os/iso/verify-config.toml && rg -q 'goblins-hwgate-session-orchestrator-starter.service' os/iso/verify-config.toml && rg -q 'GOBLINS_HWGATE_SESSION_ORCHESTRATOR_STARTED' os/iso/verify-config.toml && rg -q 'GOBLINS_HWGATE_FIRSTBOOT_HELPER_DOWNLOADED' os/iso/verify-config.toml && rg -q 'GOBLINS_HWGATE_SESSION_BUS_READY' os/iso/verify-config.toml && rg -q 'GOBLINS_HWGATE_SESSION_ORCHESTRATOR_START_REQUESTED' os/iso/verify-config.toml os/hardware-gate/capture-harness/drive-capture.py && rg -q 'download_with_wait firstboot-unlock.sh /tmp/gos-firstboot 240' os/iso/verify-config.toml && rg -q 'download_with_wait orchestrator.sh /tmp/gos-orchestrator 600' os/iso/verify-config.toml && rg -q 'GOS_ORCHESTRATOR_DEST' os/hardware-gate/capture-harness/run-capture.sh && rg -q 'publish_orchestrator()' os/hardware-gate/capture-harness/drive-capture.py && rg -q '\"GET /orchestrator.sh HTTP/1.1\" 200' os/hardware-gate/capture-harness/drive-capture.py && rg -q 'first boot setup failed before helper callback; collecting VT diagnostics' os/hardware-gate/capture-harness/drive-capture.py && ! rg -q 'key[(]\"alt[+]f2\"|run_alt_f2' os/hardware-gate/capture-harness/drive-capture.py"
check "hardware gate session automation imports display env before user service" "rg -q 'Environment=WAYLAND_DISPLAY=wayland-0' os/iso/verify-config.toml && rg -q 'Environment=DISPLAY=:0' os/iso/verify-config.toml && rg -q 'dbus-update-activation-environment --systemd DISPLAY WAYLAND_DISPLAY XDG_SESSION_TYPE' os/iso/verify-config.toml && rg -q 'systemctl --user import-environment DISPLAY WAYLAND_DISPLAY XDG_SESSION_TYPE' os/iso/verify-config.toml && rg -Fq 'export WAYLAND_DISPLAY=\"\${WAYLAND_DISPLAY:-wayland-0}\"' os/hardware-gate/capture-harness/in-session-orchestrator.sh && rg -Fq 'export DISPLAY=\"\${DISPLAY:-:0}\"' os/hardware-gate/capture-harness/in-session-orchestrator.sh"
check "verification ISO config pins scratch VM disk without touching release config" "rg -q 'ignoredisk --only-use=vda' os/iso/verify-config.toml && rg -q 'zerombr' os/iso/verify-config.toml && rg -q 'clearpart --all --initlabel --disklabel=gpt --drives=vda' os/iso/verify-config.toml && rg -q 'bootloader --location=mbr --boot-drive=vda' os/iso/verify-config.toml && rg -q 'part / --fstype=xfs --label=root --grow --size=1024 --ondisk=vda' os/iso/verify-config.toml && rg -q 'GOBLINS_VERIFY_INSTALL_DONE' os/iso/verify-config.toml && ! rg -q 'ostreecontainer --url' os/iso/verify-config.toml && ! rg -q 'GOBLINS_VERIFY_INSTALL_DONE' os/iso/config.toml"
check "capture harness no longer relies on OEMDRV sidecar kickstart" "! rg -q 'make-oemdrv.sh' os/hardware-gate/capture-harness/run-capture.sh && ! rg -q 'oemdrv.img' os/hardware-gate/capture-harness/run-capture.sh"
check "capture harness routes QMP input to display device" "rg -q 'virtio-gpu-pci,id=video0' os/hardware-gate/capture-harness/run-capture.sh && rg -q 'GOS_QMP_DISPLAY_DEVICE=video0' os/hardware-gate/capture-harness/run-capture.sh && rg -q 'DISPLAY_DEVICE = os.environ.get' os/hardware-gate/capture-harness/drive-capture.py os/hardware-gate/capture-harness/qmp-capture.py && rg -q 'device\": DISPLAY_DEVICE' os/hardware-gate/capture-harness/drive-capture.py os/hardware-gate/capture-harness/qmp-capture.py"
check "capture harness uses documented QMP absolute pointer range" "rg -Fq 'ABS_MAX = 0x7fff' os/hardware-gate/capture-harness/drive-capture.py os/hardware-gate/capture-harness/qmp-capture.py && rg -Fq 'abs_axis(value)' os/hardware-gate/capture-harness/drive-capture.py os/hardware-gate/capture-harness/qmp-capture.py && ! rg -q '0x7fffffff|32767' os/hardware-gate/capture-harness/drive-capture.py os/hardware-gate/capture-harness/qmp-capture.py"
check "capture driver fail-closes on QMP command errors" "rg -q 'QMP command .* failed' os/hardware-gate/capture-harness/drive-capture.py os/hardware-gate/capture-harness/qmp-capture.py && rg -q 'QMP query-mice' os/hardware-gate/capture-harness/drive-capture.py"
check "hardware gate uploads failure diagnostics" "rg -q 'copy_capture_logs' os/hardware-gate/capture-harness/run-capture.sh && rg -q '_capture-logs' os/hardware-gate/capture-harness/run-capture.sh && rg -q 'if: always()' .github/workflows/hardware-gate-capture.yml"
check "hardware gate requires live firewall proof in signoff" "rg -q 'firewall_live_toggle_proof_passes' os/hardware-gate/close-signoff.sh os/hardware-gate/verify-shipping-status.sh && rg -q 'Firewall live toggle checked' os/hardware-gate/close-signoff.sh && rg -q 'firewall-live-toggle-proof.json' os/hardware-gate/runbook.md"
check "hardware gate requires Text Shortcuts session proof in signoff" "rg -q 'text_shortcuts_session_enable_proof_passes' os/hardware-gate/close-signoff.sh os/hardware-gate/verify-shipping-status.sh && rg -q 'Text Shortcuts session enablement checked' os/hardware-gate/close-signoff.sh && rg -q 'text-shortcuts-session-enable-proof.json' os/hardware-gate/runbook.md"
check "hardware gate records Text Shortcuts live keystrokes through runtime/render signoff" "! rg -q 'text_shortcuts_live_keystroke_proof_passe[s][[:space:]]*\\(|text-shortcuts-live-keystroke-proof[.]json' os/hardware-gate/close-signoff.sh os/hardware-gate/verify-shipping-status.sh && rg -q 'Text Shortcuts live keystrokes checked' os/hardware-gate/close-signoff.sh && rg -Fq 'covered by \$TEXT_SHORTCUTS_LIVE_IBUS_RUNTIME_RENDER_PROOF' os/hardware-gate/close-signoff.sh && rg -q 'supersedes the old text-shortcuts-live-keystroke-proof[.]json' os/hardware-gate/runbook.md"
check "hardware gate requires Text Shortcuts candidate metadata proof in signoff" "rg -q 'text_shortcuts_candidate_metadata_proof_passes' os/hardware-gate/close-signoff.sh os/hardware-gate/verify-shipping-status.sh && rg -q 'Text Shortcuts candidate metadata checked' os/hardware-gate/close-signoff.sh os/hardware-gate/verify-shipping-status.sh && rg -q 'text-shortcuts-candidate-metadata-proof.json' os/hardware-gate/runbook.md"
check "hardware gate requires Text Shortcuts overlay intent proof in signoff" "rg -q 'text_shortcuts_overlay_intent_proof_passes' os/hardware-gate/close-signoff.sh os/hardware-gate/verify-shipping-status.sh && rg -q 'Text Shortcuts overlay intent checked' os/hardware-gate/close-signoff.sh os/hardware-gate/verify-shipping-status.sh && rg -q 'text-shortcuts-overlay-intent-proof.json' os/hardware-gate/runbook.md"
check "hardware gate requires Text Shortcuts candidate bubble frame proof in signoff" "rg -q 'text_shortcuts_candidate_bubble_frame_proof_passes' os/hardware-gate/close-signoff.sh os/hardware-gate/verify-shipping-status.sh && rg -q 'Text Shortcuts candidate bubble frame checked' os/hardware-gate/close-signoff.sh os/hardware-gate/verify-shipping-status.sh && rg -q 'text-shortcuts-candidate-bubble-frame-proof.json' os/hardware-gate/runbook.md"
check "hardware gate requires Text Shortcuts candidate bubble layout proof in signoff" "rg -q 'text_shortcuts_candidate_bubble_layout_proof_passes' os/hardware-gate/close-signoff.sh os/hardware-gate/verify-shipping-status.sh && rg -q 'Text Shortcuts candidate bubble layout checked' os/hardware-gate/close-signoff.sh os/hardware-gate/verify-shipping-status.sh && rg -q 'text-shortcuts-candidate-bubble-layout-proof.json' os/hardware-gate/runbook.md"
check "hardware gate requires Text Shortcuts candidate bubble render intent proof in signoff" "rg -q 'text_shortcuts_candidate_bubble_render_intent_proof_passes' os/hardware-gate/close-signoff.sh os/hardware-gate/verify-shipping-status.sh && rg -q 'Text Shortcuts candidate bubble render intent checked' os/hardware-gate/close-signoff.sh os/hardware-gate/verify-shipping-status.sh && rg -q 'text-shortcuts-candidate-bubble-render-intent-proof.json' os/hardware-gate/runbook.md"
check "hardware gate requires Text Shortcuts candidate bubble render screenshot proof in signoff" "rg -q 'text_shortcuts_candidate_bubble_render_proof_passes' os/hardware-gate/close-signoff.sh os/hardware-gate/verify-shipping-status.sh && rg -q 'Text Shortcuts candidate bubble render screenshot checked' os/hardware-gate/close-signoff.sh os/hardware-gate/verify-shipping-status.sh && rg -q 'text-shortcuts-candidate-bubble-render-proof.json' os/hardware-gate/runbook.md && rg -q '31-text-shortcuts-candidate-bubble-render.png' os/hardware-gate/runbook.md"
check "hardware gate requires Text Shortcuts live IBus runtime/render proof in signoff" "rg -q 'text_shortcuts_live_ibus_runtime_render_proof_passes' os/hardware-gate/close-signoff.sh os/hardware-gate/verify-shipping-status.sh && rg -q 'Text Shortcuts live IBus runtime/render checked' os/hardware-gate/close-signoff.sh os/hardware-gate/verify-shipping-status.sh && rg -q 'text-shortcuts-live-ibus-runtime-render-proof.json' os/hardware-gate/runbook.md && rg -q '32-text-shortcuts-live-ibus-runtime-render.png' os/hardware-gate/runbook.md"
check "hardware gate requires Preview open/render proof in signoff" "rg -q 'preview_open_render_proof_passes' os/hardware-gate/close-signoff.sh os/hardware-gate/verify-shipping-status.sh && rg -q 'Preview open/render checked' os/hardware-gate/close-signoff.sh os/hardware-gate/verify-shipping-status.sh && rg -q 'preview-open-render-proof.json' os/hardware-gate/runbook.md"
check "hardware gate requires Audio output proof in signoff" "rg -q 'audio_output_proof_passes' os/hardware-gate/close-signoff.sh os/hardware-gate/verify-shipping-status.sh && rg -q 'Audio output checked' os/hardware-gate/close-signoff.sh os/hardware-gate/verify-shipping-status.sh && rg -q 'audio-output-proof.json' os/hardware-gate/runbook.md"
check "hardware gate requires runtime app-build proof in signoff" "rg -q 'runtime_build_proof_passes' os/hardware-gate/close-signoff.sh os/hardware-gate/verify-shipping-status.sh && rg -q 'runtime-build-proof.json' os/hardware-gate/runbook.md && rg -q '/proof/runtime-build' os/hardware-gate/capture-harness/in-session-orchestrator.sh && rg -q 'runtime_build_proof' os/hardware-gate/capture-harness/run-capture.sh"
check "runtime model gate writes verifier runtime proof" "rg -q 'PROOF_PATH' os/runtime-gate/build-an-app-live-model.sh && rg -q 'runtime-build-proof.json' os/runtime-gate/build-an-app-live-model.sh && rg -q '\"route\": \"/v1/apps/builds\"' os/runtime-gate/build-an-app-live-model.sh && rg -q '\"engine_mode\": \"local-model\"' os/runtime-gate/build-an-app-live-model.sh"
check "runtime model gate grants app-builder for active policy profile" "rg -q '/v1/policy/status' os/runtime-gate/build-an-app-live-model.sh os/hardware-gate/capture-harness/in-session-orchestrator.sh && rg -q 'grant_app_builder_permission' os/runtime-gate/build-an-app-live-model.sh && rg -q 'grant_policy_permission' os/hardware-gate/capture-harness/in-session-orchestrator.sh && ! rg -q 'FOR consumer' os/runtime-gate/build-an-app-live-model.sh os/hardware-gate/capture-harness/in-session-orchestrator.sh"
check "capture fixture core uses ephemeral writable state" "rg -Fq 'FIX_STATE=/tmp/goblins-os-fixture-state' os/hardware-gate/capture-harness/in-session-orchestrator.sh && rg -Fq 'GOBLINS_OS_POLICY_STATE=\"\$FIX_STATE/policy\"' os/hardware-gate/capture-harness/in-session-orchestrator.sh && rg -Fq 'GOBLINS_OS_APPS_DIR=\"\$FIX_STATE/apps\"' os/hardware-gate/capture-harness/in-session-orchestrator.sh"
check "capture fixture core uses served local model" "rg -Fq 'CAPTURE_LOCAL_MODEL=\"\${GOBLINS_OS_LOCAL_MODEL:-llama3.2:1b}\"' os/hardware-gate/capture-harness/in-session-orchestrator.sh && rg -Fq 'start_capture_model_loopback' os/hardware-gate/capture-harness/in-session-orchestrator.sh && rg -Fq 'TARGET = (\"10.0.2.2\", 11434)' os/hardware-gate/capture-harness/in-session-orchestrator.sh && rg -Fq 'GOBLINS_OS_LOCAL_MODEL=\"\$CAPTURE_LOCAL_MODEL\"' os/hardware-gate/capture-harness/in-session-orchestrator.sh && rg -Fq 'GOBLINS_OS_LOCAL_RUNTIME_URL=http://127.0.0.1:11434' os/hardware-gate/capture-harness/in-session-orchestrator.sh && rg -Fq 'ollama pull llama3.2:1b' .github/workflows/hardware-gate-capture.yml"
check "hardware gate requires Focus arm roundtrip proof in signoff" "rg -q 'focus_arm_roundtrip_proof_passes' os/hardware-gate/close-signoff.sh os/hardware-gate/verify-shipping-status.sh && rg -q 'Focus arm roundtrip checked' os/hardware-gate/close-signoff.sh os/hardware-gate/verify-shipping-status.sh && rg -q 'focus-arm-roundtrip-proof.json' os/hardware-gate/runbook.md"
check "hardware gate requires Multi-display apply proof in signoff" "rg -q 'multi_display_apply_proof_passes' os/hardware-gate/close-signoff.sh os/hardware-gate/verify-shipping-status.sh && rg -q 'Multi-display apply checked' os/hardware-gate/close-signoff.sh os/hardware-gate/verify-shipping-status.sh && rg -q 'multi-display-apply-proof.json' os/hardware-gate/runbook.md && rg -q 'multi_display_apply_proof' os/hardware-gate/runbook.md"
check "hardware gate requires App privacy revoke proof in signoff" "rg -q 'app_privacy_revoke_proof_passes' os/hardware-gate/close-signoff.sh os/hardware-gate/verify-shipping-status.sh && rg -q 'App privacy revoke checked' os/hardware-gate/close-signoff.sh os/hardware-gate/verify-shipping-status.sh && rg -q 'app-privacy-revoke-proof.json' os/hardware-gate/runbook.md"
check "core AI safe setting route requires policy and confirmation" "rg -Fq 'policy_state_for_control(\"settings-control\")' crates/goblins-os-core/src/ai.rs && rg -q 'StatusCode::PRECONDITION_REQUIRED' crates/goblins-os-core/src/ai.rs && rg -Fq 'audit_ai_action(\"change-safe-setting\"' crates/goblins-os-core/src/ai.rs"
check "core AI safe setting route has narrow allowlist" "rg -q 'appearance.color-scheme, accessibility.reduce-motion, or notifications.show-banners' crates/goblins-os-core/src/ai.rs && rg -q 'safe_setting_change_rejects_arbitrary_settings_and_wrong_values' crates/goblins-os-core/src/ai.rs"
check "core AI safe setting route reuses settings wrappers" "rg -q 'apply_ai_color_scheme' crates/goblins-os-core/src/appearance.rs && rg -q 'apply_ai_reduce_motion' crates/goblins-os-core/src/accessibility.rs && rg -q 'apply_ai_notification_banners' crates/goblins-os-core/src/notifications.rs"
check "installed self-test checks app-builder routes" "rg -q '/v1/apps/build-catalog' os/bootc/run-selftest.sh && rg -q '/v1/apps/builds' os/bootc/run-selftest.sh && rg -q 'GOBLINS_OS_APPS_DIR=/tmp/goblins-os-selftest-apps' os/bootc/run-selftest.sh"
check "bootc image includes gaming Vulkan tools and compositor substrate" "rg -q 'mesa-vulkan-drivers' os/bootc/Containerfile && rg -q 'vulkan-tools' os/bootc/Containerfile && rg -q 'gamescope' os/bootc/Containerfile && rg -q 'gamemode' os/bootc/Containerfile && rg -q 'mangohud' os/bootc/Containerfile"
check "bootc image includes gaming video audio and controller diagnostics" "rg -q 'mesa-va-drivers' os/bootc/Containerfile && rg -q 'libvdpau' os/bootc/Containerfile && rg -q 'vdpauinfo' os/bootc/Containerfile && rg -q 'pipewire-utils' os/bootc/Containerfile && rg -q 'pipewire-pulseaudio' os/bootc/Containerfile && rg -q 'pipewire-alsa' os/bootc/Containerfile && rg -q 'command -v pw-play' os/bootc/Containerfile && rg -q 'command -v pw-record' os/bootc/Containerfile && rg -q 'command -v pw-dump' os/bootc/Containerfile && rg -q 'evtest' os/bootc/Containerfile && rg -q 'usbutils' os/bootc/Containerfile"
check "bootc image excludes Steam and steam-devices packages" "! rg -q '^[[:space:]]+steam([[:space:]\\\\]|$)|^[[:space:]]+steam-devices([[:space:]\\\\]|$)' os/bootc/Containerfile && rg -q '! rpm -q steam' os/bootc/Containerfile && rg -q '! rpm -q steam-devices' os/bootc/Containerfile"
check "settings Games panel explains Flatpak portals native architecture and user-initiated launchers" "rg -q 'App installs and desktop integration are ready' crates/goblins-os-settings/src/main.rs && rg -q 'Game tools run natively on this device' crates/goblins-os-settings/src/main.rs && rg -q 'Availability is checked per architecture at install time' crates/goblins-os-settings/src/main.rs && rg -q 'does not download Proton runtimes without user action' crates/goblins-os-settings/src/main.rs"
check "settings and installer hide GNOME as user-facing prerequisite copy" "! rg -q 'GNOME desktop portals|GNOME accessibility keys|needs GNOME|requires GNOME' crates/goblins-os-settings/src/main.rs crates/goblins-os-installer/src/main.rs"
check "installed-root verifier checks gaming tools and Steam absence" "rg -q 'usr/bin/pw-cli' crates/goblins-os-verify/src/main.rs && rg -q 'usr/bin/pw-play' crates/goblins-os-verify/src/main.rs && rg -q 'usr/bin/pw-record' crates/goblins-os-verify/src/main.rs && rg -q 'usr/bin/pw-dump' crates/goblins-os-verify/src/main.rs && rg -q 'usr/bin/evtest' crates/goblins-os-verify/src/main.rs && rg -q 'installed-steam-binary-absent' crates/goblins-os-verify/src/main.rs && rg -q 'installed-steam-devices-rules-absent' crates/goblins-os-verify/src/main.rs"
check "architecture contract records native non-Steam gaming policy" "rg -q 'non_steam_launcher_policy' os/release/architectures.toml && rg -q 'Steam and steam-devices are intentionally absent' os/release/architectures.toml && rg -q 'does not claim x86-only game runtimes work on Arm' os/release/architectures.toml"
check "runbook captures video controller and PipeWire gaming diagnostics" "rg -q 'vainfo' os/hardware-gate/runbook.md && rg -q 'evtest --query' os/hardware-gate/runbook.md && rg -q 'wpctl status' os/hardware-gate/runbook.md && rg -q 'pw-cli info 0' os/hardware-gate/runbook.md && rg -q 'pw-dump' os/hardware-gate/runbook.md"
check "release evidence mode exists" "rg -q -- '--release-evidence' crates/goblins-os-verify/src/main.rs"
check "asset provenance covers Goblins primary marks" "rg -q 'os/brand/Goblins-black-mark.svg' os/release/asset-provenance.toml && rg -q 'os/brand/Goblins-white-mark.svg' os/release/asset-provenance.toml"
check "asset provenance covers OpenAI mark variants" "rg -q 'OpenAI-black-wordmark.png' os/release/asset-provenance.toml && rg -q 'OpenAI-white-wordmark.png' os/release/asset-provenance.toml && rg -q 'OpenAI-black-monoblossom.png' os/release/asset-provenance.toml && rg -q 'OpenAI-white-monoblossom.png' os/release/asset-provenance.toml"
check "asset provenance covers installer artwork" "rg -q 'os/brand/anaconda/sidebar-bg.svg' os/release/asset-provenance.toml && rg -q 'os/brand/anaconda/sidebar-logo.png' os/release/asset-provenance.toml"
check "asset provenance covers wallpapers icons and sounds" "rg -q 'os/brand/wallpaper/goblins-os-light.svg' os/release/asset-provenance.toml && rg -q 'os/brand/icons/' os/release/asset-provenance.toml && rg -q 'os/sounds/GoblinsOS/' os/release/asset-provenance.toml"
check "asset provenance excludes Apple assets and SF Symbols" "rg -q 'apple_assets = \"Not used' os/release/asset-provenance.toml && rg -q 'sf_symbols = \"Not used' os/release/asset-provenance.toml"
check "source manifest classifies GOAL.md as source" "rg -q 'GOAL.md' os/release/source-tree-manifest.toml"
check "source manifest classifies CI and ignore policy sources" "rg -q '\\.github/' os/release/source-tree-manifest.toml && rg -q '\\.gitignore' os/release/source-tree-manifest.toml && rg -q '\\.dockerignore' os/release/source-tree-manifest.toml"
check "source manifest classifies local agent state" "rg -q '\\.claude/' os/release/source-tree-manifest.toml"
check "source manifest classifies generated proofs and release artifacts" "rg -q 'artifacts/' os/release/source-tree-manifest.toml && rg -q 'os/signoff-notes.md' os/release/source-tree-manifest.toml && rg -q 'os/signoff-proofs/' os/release/source-tree-manifest.toml && rg -q 'os/screenshots/' os/release/source-tree-manifest.toml && rg -q 'os/iso/output\\*/' os/release/source-tree-manifest.toml"
check "source manifest classifies local build and shell-fragment outputs" "rg -q '\\.ci-target/' os/release/source-tree-manifest.toml && rg -q '\\.ci-target-amd64/' os/release/source-tree-manifest.toml && rg -q 'target/' os/release/source-tree-manifest.toml && rg -q 'libpod/' os/release/source-tree-manifest.toml && rg -q '\\.DS_Store' os/release/source-tree-manifest.toml && rg -q --fixed-strings '%sn *' os/release/source-tree-manifest.toml && rg -q --fixed-strings -- '-background' os/release/source-tree-manifest.toml"
check "release readiness manifest records current source evidence" "rg -q 'rust_source_gates_available' os/release/release-readiness-delta.toml && rg -q 'source_package_materialized' os/release/release-readiness-delta.toml && rg -Fq 'root = \".\"' os/release/release-readiness-delta.toml && rg -Fq 'source_tree_manifest = \"os/release/source-tree-manifest.toml\"' os/release/release-readiness-delta.toml"
check "release readiness manifest records native release blockers" "rg -q 'native_linux_release_runner_required' os/release/release-readiness-delta.toml && rg -q 'shippable_release_iso_artifacts_incomplete' os/release/release-readiness-delta.toml && rg -q 'display_backed_architecture_proofs_missing' os/release/release-readiness-delta.toml && rg -q 'x86_64_rpm_sbom_present' os/release/release-readiness-delta.toml && rg -q 'complete_signoff_rows_missing' os/release/release-readiness-delta.toml"
check "release readiness manifest has no stale local blocker labels or local user paths" "! rg -q 'rust_toolchain_missing|source_files_dataless|disk_space_low|x86_64_rpm_sbom_missing|/Users/' os/release/release-readiness-delta.toml"
check "ignore files exclude local agent state" "rg -q '\\.claude/' .gitignore && rg -q '^\\.claude$' .dockerignore"
check "ignore files exclude generated proofs and release artifacts" "rg -q '^artifacts/' .gitignore && rg -q '^os/signoff-proofs/' .gitignore && rg -q '^os/screenshots/' .gitignore && rg -q '^os/iso/output\\*/' .gitignore && rg -q '^artifacts$' .dockerignore && rg -q '^os/signoff-proofs$' .dockerignore && rg -q '^os/screenshots$' .dockerignore && rg -q '^os/iso/output\\*$' .dockerignore"
check "ignore files exclude local build and shell-fragment outputs" "rg -q '^target$' .gitignore && rg -q '^target$' .dockerignore && rg -q '^\\.ci-target/' .gitignore && rg -q '^\\.ci-target$' .dockerignore && rg -q '^\\.ci-target-amd64/' .gitignore && rg -q '^\\.ci-target-amd64$' .dockerignore && rg -q '\\.DS_Store' .gitignore && rg -q '\\.DS_Store' .dockerignore && rg -q --fixed-strings '%sn *' .gitignore && rg -q --fixed-strings '%sn *' .dockerignore && rg -q --fixed-strings -- '-background' .gitignore && rg -q --fixed-strings -- '-background' .dockerignore"
check "trademark posture keeps Goblins OS primary" "rg -q 'Goblins OS remains the leading product identity' os/release/trademark-posture.toml"
check "trademark posture scopes OpenAI to provider integration" "rg -q 'Provider/integration reference only' os/release/trademark-posture.toml"
check "trademark posture scopes Fedora and Red Hat to base references" "rg -q 'Base-platform reference only' os/release/trademark-posture.toml"
check "trademark posture scopes GNOME marks to factual package references" "rg -q 'Runtime, toolkit, and package reference only' os/release/trademark-posture.toml"
check "trademark posture blocks Apple assets and copied trade dress" "rg -q 'Do not ship Apple fonts, logos, symbols, wallpapers, screenshots, app screens, product images, SF Symbols, or copied Apple trade dress' os/release/trademark-posture.toml"
check "third-party notices cover GNOME package SBOM path" "rg -q 'GNOME Shell, GTK, libadwaita/Adwaita assets' os/release/third-party-notices.toml"
check "third-party notices document release evidence generator" "rg -q -- '--release-evidence os/signoff-proofs/sbom/<arch>/' os/release/third-party-notices.toml"
check "third-party notices require cargo package TSV" "rg -q 'cargo-lock-packages.tsv' os/release/third-party-notices.toml"
check "third-party notices require RPM command file" "rg -q 'rpm-packages.command' os/release/third-party-notices.toml"
check "SHIP documents SBOM evidence command" "rg -q --fixed-strings -- '--release-evidence \"os/signoff-proofs/sbom/' \"$SHIP_DECL\""
check "shipping status rejects local-only installer payload refs" "rg -q 'installer payload tracks a local-only Docker/test registry' os/hardware-gate/verify-shipping-status.sh"
check "shipping status reports ignored legacy screenshot roots" "rg -q 'Legacy/non-shipping screenshot roots ignored by architecture proof gate' os/hardware-gate/verify-shipping-status.sh"
check "runbook rejects legacy non-arch screenshot roots" "rg -q 'Legacy/non-shipping screenshot roots' os/hardware-gate/runbook.md && rg -q '<arch>/<YYYY-MM-DD>' os/hardware-gate/runbook.md"

for shot in "${REQ_SCREENSHOTS[@]}"; do
  check "runbook includes required screenshot $shot" "rg -q --fixed-strings '$shot' \"$RUNBOOK\""
done

check "signoff notes contains runtime engine fields" "rg -q 'Runtime engine run:|Motion/interactions checked' \"$SIGNOFF\""
check "signoff notes contains gaming proof field" "rg -q 'Gaming readiness checked' \"$SIGNOFF\""
check "signoff notes contains install storage proof field" "rg -q 'Install storage/bootloader/dual-boot checked' \"$SIGNOFF\""
check "signoff notes contains release evidence proof field" "rg -q 'Release evidence/SBOM checked' \"$SIGNOFF\""
check "close-signoff writes fail-closed completion status" "rg -q 'PROJECT_COMPLETION_STATUS=\"incomplete\"' os/hardware-gate/close-signoff.sh && rg -q 'Current project completion status: \\$\\{PROJECT_COMPLETION_STATUS\\}' os/hardware-gate/close-signoff.sh"
check "close-signoff requires runtime and built-artifact proof before completion" "rg -q 'RUNTIME_ENGINE_MODE' os/hardware-gate/close-signoff.sh && rg -q 'BUILT_ARTIFACT_PATH_URL' os/hardware-gate/close-signoff.sh && rg -q '\\[ -n \"[$]RUNTIME_ENGINE_MODE\" \\]' os/hardware-gate/close-signoff.sh && rg -q '\\[ -n \"[$]BUILT_ARTIFACT_PATH_URL\" \\]' os/hardware-gate/close-signoff.sh"
check "close-signoff rejects placeholder runtime proof" "rg -q 'proof_field_is_real' os/hardware-gate/close-signoff.sh && rg -q 'validate_runtime_proof_fields' os/hardware-gate/close-signoff.sh && rg -q 'placeholders are not accepted' os/hardware-gate/close-signoff.sh"
check "close-signoff requires real built artifact reference" "rg -q 'built_artifact_reference_is_real' os/hardware-gate/close-signoff.sh && rg -q 'https URL, localhost URL, or existing local path' os/hardware-gate/close-signoff.sh"
check "close-signoff requires architecture screenshot directory" "rg -q 'screenshot_dir_matches_arch' os/hardware-gate/close-signoff.sh && rg -q 'os/screenshots/hardware-gate/[$]ARCH/<date>' os/hardware-gate/close-signoff.sh"
check "close-signoff workflow checks fail fast" "rg -q 'require_fixed' os/hardware-gate/close-signoff.sh && rg -q 'per-architecture image build target missing in workflow' os/hardware-gate/close-signoff.sh"
check "close-signoff uses Docker for assisted signoff testing" "rg -q 'Docker is required for assisted signoff testing' os/hardware-gate/close-signoff.sh && rg -q 'docker image inspect' os/hardware-gate/close-signoff.sh && rg -q 'docker run --rm' os/hardware-gate/close-signoff.sh && rg -q 'DOCKER_BUILDKIT=1 docker build' os/hardware-gate/close-signoff.sh && ! rg -q 'podman' os/hardware-gate/close-signoff.sh"
check "close-signoff expects per-architecture image tag" "rg -q 'goblins-os:\\$\\{\\{ matrix.arch \\}\\}' os/hardware-gate/close-signoff.sh"
check "close-signoff uses exact architecture ISO path" "rg -q 'expected_iso=\"os/iso/output/[$]ARCH/bootiso/goblins-os-[$]ARCH.iso\"' os/hardware-gate/close-signoff.sh"
check "shipping status bounds signoff rows at the next markdown heading" "rg -q 'signoff_block_from_line' os/hardware-gate/verify-shipping-status.sh && rg -q 'NR < start' os/hardware-gate/verify-shipping-status.sh && rg -Fq '/^## / { exit }' os/hardware-gate/verify-shipping-status.sh && ! rg -Fq \"start + \$((60 + 60))\" os/hardware-gate/verify-shipping-status.sh"

for arch in "${ARCHES[@]}"; do
  ISO_PATH="os/iso/output/$arch/bootiso/goblins-os-$arch.iso"
  SHA_PATH="$ISO_PATH.sha256"
  MANIFEST_PATH="os/iso/output/$arch/manifest-goblins-os-$arch.json"
  BIB_MANIFEST_PATH="os/iso/output/$arch/manifest-anaconda-iso.json"
  SBOM_DIR="os/signoff-proofs/sbom/$arch"
  SBOM_MANIFEST="$SBOM_DIR/release-evidence-manifest.json"
  CARGO_TSV="$SBOM_DIR/cargo-lock-packages.tsv"
  RPM_TSV="$SBOM_DIR/rpm-packages.tsv"
  ARCH_MISSING=()

  check_file "$arch ISO artifact exists" "$ISO_PATH" || ARCH_MISSING+=("ISO")
  check_file "$arch ISO SHA256 exists" "$SHA_PATH" || ARCH_MISSING+=("SHA256")
  if [ -f "$ISO_PATH" ] && [ -f "$SHA_PATH" ]; then
    check_sha256_file "$arch ISO SHA256 verifies" "$SHA_PATH" || ARCH_MISSING+=("SHA256 verification")
  fi
  check_file "$arch ISO manifest exists" "$MANIFEST_PATH" || ARCH_MISSING+=("ISO manifest")
  check_file_contains "$arch ISO manifest records architecture" "$MANIFEST_PATH" "\"architecture\": \"$arch\"" || ARCH_MISSING+=("ISO manifest architecture")
  check_file_contains "$arch ISO manifest records ISO name" "$MANIFEST_PATH" "\"iso\": \"bootiso/goblins-os-$arch.iso\"" || ARCH_MISSING+=("ISO manifest artifact")
  check_file_contains "$arch ISO manifest records SHA file" "$MANIFEST_PATH" "\"sha256_file\": \"bootiso/goblins-os-$arch.iso.sha256\"" || ARCH_MISSING+=("ISO manifest SHA")
  check_file_contains "$arch ISO manifest records builder source image" "$MANIFEST_PATH" "\"builder_source_image\":" || ARCH_MISSING+=("ISO manifest builder source")
  check_file_contains "$arch ISO manifest records installer payload source kind" "$MANIFEST_PATH" "\"installer_payload_source_kind\":" || ARCH_MISSING+=("ISO manifest payload source kind")
  check_file_contains "$arch ISO manifest records nonlocal installer payload source" "$MANIFEST_PATH" "\"installer_payload_source_local_only\": false" || ARCH_MISSING+=("ISO manifest nonlocal payload source")
  check_file_contains "$arch ISO manifest records shippable release mode" "$MANIFEST_PATH" "\"shippable_release\": true" || ARCH_MISSING+=("ISO manifest shippable release")
  check_bib_manifest_payload_ref "$arch BIB manifest uses shippable installer payload ref" "$BIB_MANIFEST_PATH" || ARCH_MISSING+=("shippable installer payload ref")
  check_file "$arch release evidence manifest exists" "$SBOM_MANIFEST" || ARCH_MISSING+=("release evidence manifest")
  check_file_contains "$arch release evidence manifest records architecture" "$SBOM_MANIFEST" "\"architecture\": \"$arch\"" || ARCH_MISSING+=("release evidence architecture")
  check_file_contains "$arch release evidence manifest records asset provenance" "$SBOM_MANIFEST" "\"asset_provenance\": \"os/release/asset-provenance.toml\"" || ARCH_MISSING+=("release evidence asset provenance")
  check_file_contains "$arch release evidence manifest records third-party notices" "$SBOM_MANIFEST" "\"third_party_notices\": \"os/release/third-party-notices.toml\"" || ARCH_MISSING+=("release evidence third-party notices")
  check_file_contains "$arch release evidence manifest records trademark posture" "$SBOM_MANIFEST" "\"trademark_posture\": \"os/release/trademark-posture.toml\"" || ARCH_MISSING+=("release evidence trademark posture")
  check_file_contains "$arch release evidence manifest records source tree manifest" "$SBOM_MANIFEST" "\"source_tree_manifest\": \"os/release/source-tree-manifest.toml\"" || ARCH_MISSING+=("release evidence source tree manifest")
  check_file "$arch Cargo SBOM package TSV exists" "$CARGO_TSV" || ARCH_MISSING+=("Cargo SBOM TSV")
  check_file "$arch RPM SBOM package TSV exists" "$RPM_TSV" || ARCH_MISSING+=("RPM SBOM TSV")
  if [ -f "$RPM_TSV" ]; then
    if rpm_sbom_arch_matches "$RPM_TSV" "$arch"; then
      echo "[PASS] $arch RPM SBOM package architectures match $arch or noarch"
    else
      echo "[FAIL] $arch RPM SBOM package architectures must match $arch or noarch"
      FAIL_COUNT=$((FAIL_COUNT + 1))
      ARCH_MISSING+=("RPM SBOM architecture")
    fi
  fi

  if [ -d "$SCREENSHOT_ROOT/$arch" ]; then
    LATEST_ARCH_RUN=""
    while IFS= read -r candidate; do
      if screenshot_run_is_complete "$candidate"; then
        LATEST_ARCH_RUN="$candidate"
        break
      fi
    done < <(find "$SCREENSHOT_ROOT/$arch" -mindepth 1 -maxdepth 1 -type d | sort -r)
    if [ -n "$LATEST_ARCH_RUN" ]; then
      echo "[PASS] $arch has complete hardware-gate screenshots: $LATEST_ARCH_RUN"
    else
      echo "[FAIL] $arch has no complete hardware-gate screenshot run under $SCREENSHOT_ROOT/$arch"
      print_latest_incomplete_screenshot_run "$SCREENSHOT_ROOT/$arch" "$arch"
      FAIL_COUNT=$((FAIL_COUNT + 1))
      ARCH_MISSING+=("complete screenshot run")
    fi
  else
    echo "[FAIL] $arch screenshot root missing: $SCREENSHOT_ROOT/$arch"
    print_latest_incomplete_screenshot_run "$SCREENSHOT_ROOT/$arch" "$arch"
    FAIL_COUNT=$((FAIL_COUNT + 1))
    ARCH_MISSING+=("screenshot root")
  fi

  if signoff_run_for_arch_is_complete "$arch"; then
    echo "[PASS] $arch has complete signoff row"
  else
    echo "[FAIL] $arch has no complete signoff row with runner, ISO, verify/self-test, SBOM, runtime, gaming, and install-storage proof"
    FAIL_COUNT=$((FAIL_COUNT + 1))
    ARCH_MISSING+=("complete signoff row")
  fi

  if [ "${#ARCH_MISSING[@]}" -eq 0 ]; then
    echo "[PASS] $arch architecture track complete"
  else
    echo "[FAIL] $arch architecture track missing: ${ARCH_MISSING[*]}"
    FAIL_COUNT=$((FAIL_COUNT + 1))
  fi
done

print_legacy_screenshot_roots

if [ -f "$SIGNOFF" ]; then
  RUN_LINE="$(rg -n "^## Manual Gate Run:" "$SIGNOFF" | tail -n1 | cut -d: -f1 || true)"
  if [ -n "$RUN_LINE" ]; then
    LATEST_RUN_BLOCK="$(awk -v start="$RUN_LINE" 'NR < start { next } NR == start { print; next } /^## / { exit } { print }' "$SIGNOFF")"
    if echo "$LATEST_RUN_BLOCK" | rg -q "^- Runner: .+"; then
      echo "[PASS] Latest signoff run has Runner"
    else
      echo "[FAIL] Latest signoff run missing Runner"
      FAIL_COUNT=$((FAIL_COUNT + 1))
    fi
    if echo "$LATEST_RUN_BLOCK" | rg -q "^- Architecture: (aarch64|x86_64)"; then
      echo "[PASS] Latest signoff run has architecture"
    else
      echo "[FAIL] Latest signoff run missing architecture"
      FAIL_COUNT=$((FAIL_COUNT + 1))
    fi
    if echo "$LATEST_RUN_BLOCK" | rg -q "^- Verify result \(blocked=0\): pass"; then
      echo "[PASS] Latest signoff run recorded blocked=0 pass"
    else
      echo "[FAIL] Latest signoff run missing/does not record blocked=0 pass"
      FAIL_COUNT=$((FAIL_COUNT + 1))
    fi
    if echo "$LATEST_RUN_BLOCK" | rg -q "^- Self-test result: pass"; then
      echo "[PASS] Latest signoff run recorded self-test pass"
    else
      echo "[FAIL] Latest signoff run missing/does not record self-test pass"
      FAIL_COUNT=$((FAIL_COUNT + 1))
    fi
    if signoff_block_has_real_field "$LATEST_RUN_BLOCK" "^  - mode: .+"; then
      echo "[PASS] Latest signoff run records real runtime engine mode"
    else
      echo "[FAIL] Latest signoff run missing real runtime engine mode"
      FAIL_COUNT=$((FAIL_COUNT + 1))
    fi
    if signoff_block_has_real_field "$LATEST_RUN_BLOCK" "^  - engine source: .+"; then
      echo "[PASS] Latest signoff run records real runtime engine source"
    else
      echo "[FAIL] Latest signoff run missing real runtime engine source"
      FAIL_COUNT=$((FAIL_COUNT + 1))
    fi
    if signoff_block_has_real_field "$LATEST_RUN_BLOCK" "^  - built artifact path/URL: .+"; then
      echo "[PASS] Latest signoff run has real built artifact proof"
    else
      echo "[FAIL] Latest signoff run missing real built artifact proof"
      FAIL_COUNT=$((FAIL_COUNT + 1))
    fi
    if echo "$LATEST_RUN_BLOCK" | rg -q "^- Motion/interactions checked: yes"; then
      echo "[PASS] Latest signoff run records motion/interaction proof"
    else
      echo "[FAIL] Latest signoff run missing motion/interaction proof"
      FAIL_COUNT=$((FAIL_COUNT + 1))
    fi
    if echo "$LATEST_RUN_BLOCK" | rg -q "^- Preview open/render checked: yes"; then
      echo "[PASS] Latest signoff run records Preview open/render proof"
    else
      echo "[FAIL] Latest signoff run missing Preview open/render proof"
      FAIL_COUNT=$((FAIL_COUNT + 1))
    fi
    if echo "$LATEST_RUN_BLOCK" | rg -q "^- Text Shortcuts candidate bubble render screenshot checked: yes"; then
      echo "[PASS] Latest signoff run records Text Shortcuts candidate bubble render screenshot proof"
    else
      echo "[FAIL] Latest signoff run missing Text Shortcuts candidate bubble render screenshot proof"
      FAIL_COUNT=$((FAIL_COUNT + 1))
    fi
    if echo "$LATEST_RUN_BLOCK" | rg -q "^- Focus arm roundtrip checked: yes"; then
      echo "[PASS] Latest signoff run records Focus arm roundtrip proof"
    else
      echo "[FAIL] Latest signoff run missing Focus arm roundtrip proof"
      FAIL_COUNT=$((FAIL_COUNT + 1))
    fi
    if echo "$LATEST_RUN_BLOCK" | rg -q "^- App privacy revoke checked: yes"; then
      echo "[PASS] Latest signoff run records App privacy revoke proof"
    else
      echo "[FAIL] Latest signoff run missing App privacy revoke proof"
      FAIL_COUNT=$((FAIL_COUNT + 1))
    fi
    if echo "$LATEST_RUN_BLOCK" | rg -q "^- Gaming readiness checked: yes"; then
      echo "[PASS] Latest signoff run records gaming readiness proof"
    else
      echo "[FAIL] Latest signoff run missing gaming readiness proof"
      FAIL_COUNT=$((FAIL_COUNT + 1))
    fi
    if echo "$LATEST_RUN_BLOCK" | rg -q "^- Install storage/bootloader/dual-boot checked: yes"; then
      echo "[PASS] Latest signoff run records install storage/dual-boot proof"
    else
      echo "[FAIL] Latest signoff run missing install storage/dual-boot proof"
      FAIL_COUNT=$((FAIL_COUNT + 1))
    fi
    if echo "$LATEST_RUN_BLOCK" | rg -q "^- Release evidence/SBOM checked: yes"; then
      echo "[PASS] Latest signoff run records release evidence/SBOM proof"
    else
      echo "[FAIL] Latest signoff run missing release evidence/SBOM proof"
      FAIL_COUNT=$((FAIL_COUNT + 1))
    fi
    if echo "$LATEST_RUN_BLOCK" | rg -qi "Screenshot dir: no fresh|stale screenshot|stale for this ISO|No fresh .*screenshots"; then
      echo "[FAIL] Latest signoff run records stale or missing current screenshot proof"
      FAIL_COUNT=$((FAIL_COUNT + 1))
    else
      echo "[PASS] Latest signoff run does not record stale/missing current screenshot proof"
    fi
    if signoff_block_required_proof_is_complete "$LATEST_RUN_BLOCK"; then
      if echo "$LATEST_RUN_BLOCK" | rg -q "^- Current project completion status: complete$"; then
        echo "[PASS] Latest signoff run completion status matches complete proof"
      else
        echo "[FAIL] Latest signoff run has complete proof but does not declare completion"
        FAIL_COUNT=$((FAIL_COUNT + 1))
      fi
    elif echo "$LATEST_RUN_BLOCK" | rg -q "^- Current project completion status: complete"; then
      echo "[FAIL] Latest signoff run declares completion before required proof is present"
      FAIL_COUNT=$((FAIL_COUNT + 1))
    else
      echo "[PASS] Latest signoff run does not claim completion with incomplete proof"
    fi
  else
    echo "[FAIL] No Manual Gate Run sections found in signoff notes"
    FAIL_COUNT=$((FAIL_COUNT + 1))
  fi
else
  echo "[FAIL] Signoff notes file missing"
  FAIL_COUNT=$((FAIL_COUNT + 1))
fi

if [ -n "$SCREENSHOT_RUN_DIR" ]; then
  if [ -d "$SCREENSHOT_RUN_DIR" ]; then
    LATEST_RUN="$SCREENSHOT_RUN_DIR"
    echo "Verifying provided screenshot run: $LATEST_RUN"
    if ! print_screenshot_run_checks "$LATEST_RUN"; then
      fail_check "Hardware-gate screenshot proof is incomplete"
    fi
  else
    fail_check "Provided SCREENSHOT_RUN_DIR not found: $SCREENSHOT_RUN_DIR"
  fi
else
  if [ -d "$SCREENSHOT_ROOT" ]; then
    LATEST_RUN=""
    while IFS= read -r candidate; do
      if screenshot_run_is_complete "$candidate"; then
        LATEST_RUN="$candidate"
        break
      fi
    done < <(find "$SCREENSHOT_ROOT" -mindepth 2 -maxdepth 2 -type d | sort -r)

    if [ -n "$LATEST_RUN" ]; then
      echo "Latest complete hardware-gate screenshot run: $LATEST_RUN"
      if ! print_screenshot_run_checks "$LATEST_RUN"; then
        fail_check "Hardware-gate screenshot proof is incomplete"
      fi
    else
      fail_check "No complete hardware-gate screenshot run found under $SCREENSHOT_ROOT"
      for arch in "${ARCHES[@]}"; do
        print_latest_incomplete_screenshot_run "$SCREENSHOT_ROOT/$arch" "$arch"
      done
    fi
  else
    fail_check "Screenshot root missing: $SCREENSHOT_ROOT"
    print_latest_incomplete_screenshot_run "$SCREENSHOT_ROOT" "hardware-gate"
  fi
fi

echo
for arch in "${ARCHES[@]}"; do
  print_arch_next_steps "$arch"
done

echo
echo "Run ./os/hardware-gate/close-signoff.sh on Linux to generate a full verified status row with verify/self-test results."
echo "Use SCREENSHOT_RUN_DIR or SCREENSHOT_DIR to validate screenshot completeness."

if [ "${FAIL_COUNT:-0}" -ne 0 ]; then
  echo "Shipping status gate: FAIL"
  exit 1
fi

echo "Shipping status gate: PASS"
exit 0
