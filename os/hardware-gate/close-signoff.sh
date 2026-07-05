#!/usr/bin/env bash
set -euo pipefail

REPO_ROOT="${REPO_ROOT:-$(cd "$(dirname "$0")/../.." && pwd)}"
cd "$REPO_ROOT"
. "$REPO_ROOT/os/hardware-gate/secret-scan.sh"
. "$REPO_ROOT/os/hardware-gate/rpm-sbom-arch.sh"

log() { echo "[signoff] $*"; }
warn() { echo "[signoff][warn] $*" >&2; }
fail() { echo "[signoff][fail] $*" >&2; }

STAMP="$(date -u +%Y-%m-%dT%H%M%SZ)"
OUT="os/signoff-notes.md"
SHIP_DECL="SHIP.md"
SCREENSHOT_DIR="${SCREENSHOT_DIR:-${SCREENSHOT_RUN_DIR:-}}"
normalize_arch() {
  case "$1" in
    aarch64|arm64) echo "aarch64" ;;
    x86_64|amd64) echo "x86_64" ;;
    *) echo "unsupported" ;;
  esac
}
ARCH="$(normalize_arch "${GOBLINS_OS_ARCH:-$(uname -m)}")"
if [ "$ARCH" = "unsupported" ]; then
  fail "Unsupported architecture '${GOBLINS_OS_ARCH:-$(uname -m)}'; expected aarch64 or x86_64."
  exit 1
fi
VERIFY_LOG="/tmp/goblins-os-verify.log"
VERIFY_ERR="/tmp/verify.err"
SELFTEST_LOG="/tmp/goblins-os-selftest.log"
SELFTEST_DOCKERFILE="/tmp/selftest.Dockerfile"
BASE_SCREENSHOTS=(
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
)
GAMING_SCREENSHOTS=(
  "19-vulkan-vkcube.png"
  "20-gamemode-active.png"
  "21-gamescope-session.png"
  "22-mangohud-overlay.png"
  "23-controller-detection.png"
  "24-audio-output.png"
)
INSTALL_STORAGE_SCREENSHOTS=(
  "25-install-destination.png"
  "26-install-storage-summary.png"
  "27-dual-boot-preserve-existing-os.png"
  "28-bootloader-efi-summary.png"
)
PREVIEW_SCREENSHOTS=(
  "29-preview-pdf-open.png"
  "30-preview-image-open.png"
)
TEXT_SHORTCUTS_SCREENSHOTS=(
  "31-text-shortcuts-candidate-bubble-render.png"
  "32-text-shortcuts-live-ibus-runtime-render.png"
)
SCREENSHOT_REQUIRED=(
  "${BASE_SCREENSHOTS[@]}"
  "${GAMING_SCREENSHOTS[@]}"
  "${INSTALL_STORAGE_SCREENSHOTS[@]}"
  "${PREVIEW_SCREENSHOTS[@]}"
  "${TEXT_SHORTCUTS_SCREENSHOTS[@]}"
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
GAMING_SCREENSHOT_STATUS="not checked"
GAMING_AUDIO_OUTPUT_STATUS="not checked"
INSTALL_STORAGE_STATUS="not checked"
RELEASE_EVIDENCE_STATUS="not checked"
MOTION_INTERACTIONS_STATUS="not checked"
FIREWALL_TOGGLE_STATUS="not checked"
TEXT_SHORTCUTS_SESSION_STATUS="not checked"
TEXT_SHORTCUTS_KEYSTROKE_STATUS="not checked"
TEXT_SHORTCUTS_CANDIDATE_STATUS="not checked"
TEXT_SHORTCUTS_OVERLAY_INTENT_STATUS="not checked"
TEXT_SHORTCUTS_CANDIDATE_BUBBLE_FRAME_STATUS="not checked"
TEXT_SHORTCUTS_CANDIDATE_BUBBLE_LAYOUT_STATUS="not checked"
TEXT_SHORTCUTS_CANDIDATE_BUBBLE_RENDER_INTENT_STATUS="not checked"
TEXT_SHORTCUTS_CANDIDATE_BUBBLE_RENDER_STATUS="not checked"
TEXT_SHORTCUTS_LIVE_IBUS_RUNTIME_RENDER_STATUS="not checked"
KEYBOARD_SHORTCUTS_ROUNDTRIP_STATUS="not checked"
INPUT_SOURCES_ROUNDTRIP_STATUS="not checked"
MULTI_DISPLAY_APPLY_STATUS="not checked"
FOCUS_ARM_ROUNDTRIP_STATUS="not checked"
APP_PRIVACY_REVOKE_STATUS="not checked"
PREVIEW_OPEN_RENDER_STATUS="not checked"
RUNTIME_ENGINE_MODE="${RUNTIME_ENGINE_MODE:-}"
RUNTIME_ENGINE_SOURCE="${RUNTIME_ENGINE_SOURCE:-}"
RUNTIME_ENGINE_CONFIG="${RUNTIME_ENGINE_CONFIG:-}"
BUILT_ARTIFACT_PATH_URL="${BUILT_ARTIFACT_PATH_URL:-}"

ci_run_url() {
  if [ -n "${GITHUB_SERVER_URL:-}" ] && [ -n "${GITHUB_REPOSITORY:-}" ] && [ -n "${GITHUB_RUN_ID:-}" ]; then
    printf '%s/%s/actions/runs/%s' "$GITHUB_SERVER_URL" "$GITHUB_REPOSITORY" "$GITHUB_RUN_ID"
  fi
}

signoff_runner() {
  if [ -n "${SIGNOFF_RUNNER:-}" ]; then
    printf '%s' "$SIGNOFF_RUNNER"
    return
  fi

  if [ -n "${GITHUB_ACTIONS:-}" ]; then
    local runner="${RUNNER_NAME:-github-actions}"
    local os="${RUNNER_OS:-unknown-os}"
    local arch="${RUNNER_ARCH:-unknown-arch}"
    local workflow="${GITHUB_WORKFLOW:-unknown-workflow}"
    local run_url
    run_url="$(ci_run_url)"
    if [ -n "$run_url" ]; then
      printf '%s (%s/%s, %s, %s)' "$runner" "$os" "$arch" "$workflow" "$run_url"
    else
      printf '%s (%s/%s, %s)' "$runner" "$os" "$arch" "$workflow"
    fi
    return
  fi

  printf '%s' "${USER:-$(id -un 2>/dev/null || echo unknown)}@$(hostname 2>/dev/null || echo unknown-host)"
}

CI_RUN_URL="$(ci_run_url)"
SIGNOFF_RUNNER_VALUE="$(signoff_runner)"
CI_RUST_URL="${CI_RUST_URL:-${CI_RUN_URL:-}}"
CI_IMAGE_URL="${CI_IMAGE_URL:-${CI_RUN_URL:-}}"
CI_INSTALLER_ISO_URL="${CI_INSTALLER_ISO_URL:-${CI_RUN_URL:-}}"

choose_runtime() {
  if command -v docker >/dev/null 2>&1; then
    echo "docker"
    return
  fi

  echo ""
}

runtime_image_exists() {
  local runtime="$1"
  local image="$2"

  case "$runtime" in
    docker)
      docker image inspect "$image" >/dev/null 2>&1 || docker pull "$image" >/dev/null 2>&1
      ;;
    *)
      return 1
      ;;
  esac
}

run_verify() {
  local runtime="$1"
  local image="$2"

  case "$runtime" in
    docker)
      docker run --rm "$image" /usr/libexec/goblins-os/goblins-os-verify --installed-root /
      ;;
    *)
      return 1
      ;;
  esac
}

run_selftest() {
  local runtime="$1"
  local dockerfile="$2"

  case "$runtime" in
    docker)
      # cacheonly: the concatenated Containerfile has more layers than the
      # docker daemon can export as an image ("max depth exceeded"); the
      # self-test's value is that the build FAILS when the selftest stage
      # fails, exactly as the build workflow runs it.
      DOCKER_BUILDKIT=1 docker buildx build -f "$dockerfile" --target selftest --output type=cacheonly .
      ;;
    *)
      return 1
      ;;
  esac
}

generate_source_release_evidence() {
  local output_dir="$1"
  local arch="$2"

  if [ -x target/release/goblins-os-verify ]; then
    target/release/goblins-os-verify \
      --source-root . \
      --release-evidence "$output_dir" \
      --arch "$arch"
    return
  fi

  if command -v cargo >/dev/null 2>&1; then
    cargo run -p goblins-os-verify -- \
      --source-root . \
      --release-evidence "$output_dir" \
      --arch "$arch"
    return
  fi

  return 1
}

run_rpm_release_evidence() {
  local runtime="$1"
  local image="$2"
  local output_dir="$3"
  local output_abs
  output_abs="$(cd "$output_dir" && pwd)"

  case "$runtime" in
    docker)
      docker run --rm -v "$output_abs:/out" -w /out "$image" sh rpm-packages.command
      ;;
    *)
      return 1
      ;;
  esac
}

release_evidence_manifest_has_diligence_fields() {
  local manifest="$1"
  [ -f "$manifest" ] \
    && grep -Fq '"asset_provenance": "os/release/asset-provenance.toml"' "$manifest" \
    && grep -Fq '"third_party_notices": "os/release/third-party-notices.toml"' "$manifest" \
    && grep -Fq '"trademark_posture": "os/release/trademark-posture.toml"' "$manifest" \
    && grep -Fq '"source_tree_manifest": "os/release/source-tree-manifest.toml"' "$manifest"
}

release_evidence_complete() {
  local output_dir="$1"
  local manifest="$output_dir/release-evidence-manifest.json"
  release_evidence_manifest_has_diligence_fields "$manifest" \
    && [ -f "$output_dir/cargo-lock-packages.tsv" ] \
    && [ -f "$output_dir/rpm-packages.tsv" ] \
    && rpm_sbom_arch_matches "$output_dir/rpm-packages.tsv" "$ARCH"
}

require_fixed() {
  local label="$1"
  local path="$2"
  local needle="$3"

  if ! grep -Fq "$needle" "$path"; then
    fail "$label"
    exit 1
  fi
}

proof_field_is_real() {
  local value="$1"
  local lowered

  [ -n "$value" ] || return 1
  lowered="$(printf '%s' "$value" | tr '[:upper:]' '[:lower:]')"
  case "$lowered" in
    "n/a"|"na"|"none"|"unknown"|"missing"|"todo"|"tbd"|"not provided"|"not configured"|"not checked"|"not attempted"|"<"*">")
      return 1
      ;;
  esac
  ! printf '%s' "$lowered" | rg -q 'requires|external gate|not exercised|no live engine|placeholder|sample|example|dummy'
}

built_artifact_reference_is_real() {
  local value="$1"

  proof_field_is_real "$value" || return 1
  [[ "$value" =~ ^https://[^[:space:]]+$ ]] && return 0
  [[ "$value" =~ ^http://127\.0\.0\.1(:[0-9]+)?/[^[:space:]]+$ ]] && return 0
  [[ "$value" =~ ^http://localhost(:[0-9]+)?/[^[:space:]]+$ ]] && return 0
  [ -e "$value" ]
}

screenshot_dir_matches_arch() {
  local dir="${1%/}/"

  case "$dir" in
    os/screenshots/hardware-gate/"$ARCH"/* | */os/screenshots/hardware-gate/"$ARCH"/*)
      return 0
      ;;
    *)
      return 1
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
  local manifest="$1"

  [ -s "$manifest" ] || return 1
  rg -q '"architecture"[[:space:]]*:[[:space:]]*"'"$ARCH"'"' "$manifest" \
    && rg -q '"iso"[[:space:]]*:[[:space:]]*"'"$ISO_PATH"'"' "$manifest" \
    && rg -q '"iso_sha256"[[:space:]]*:[[:space:]]*"[a-fA-F0-9]{64}"' "$manifest" \
    && rg -q '"captured_at"[[:space:]]*:[[:space:]]*"[^"]+"' "$manifest" \
    && rg -q '"screenshot_run_dir"[[:space:]]*:[[:space:]]*"'"$SCREENSHOT_DIR"'"' "$manifest" \
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

screenshot_manifest_iso_sha() {
  awk -F'"' '/"iso_sha256"/ { print $4; exit }' "$1" 2>/dev/null || true
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

validate_runtime_proof_fields() {
  local any_runtime_proof=0

  [ -n "$RUNTIME_ENGINE_MODE" ] && any_runtime_proof=1
  [ -n "$RUNTIME_ENGINE_SOURCE" ] && any_runtime_proof=1
  [ -n "$RUNTIME_ENGINE_CONFIG" ] && any_runtime_proof=1
  [ -n "$BUILT_ARTIFACT_PATH_URL" ] && any_runtime_proof=1
  [ "$any_runtime_proof" -eq 0 ] && return 0

  if ! proof_field_is_real "$RUNTIME_ENGINE_MODE"; then
    fail "RUNTIME_ENGINE_MODE must name the real engine mode; placeholders such as n/a, <real-mode>, or external-gate text are not accepted."
    exit 1
  fi
  if ! proof_field_is_real "$RUNTIME_ENGINE_SOURCE"; then
    fail "RUNTIME_ENGINE_SOURCE must name the real runtime source; placeholders are not accepted."
    exit 1
  fi
  if [ -n "$RUNTIME_ENGINE_CONFIG" ] && ! proof_field_is_real "$RUNTIME_ENGINE_CONFIG"; then
    fail "RUNTIME_ENGINE_CONFIG must be blank or a real config/artifact reference; placeholders are not accepted."
    exit 1
  fi
  if ! built_artifact_reference_is_real "$BUILT_ARTIFACT_PATH_URL"; then
    fail "BUILT_ARTIFACT_PATH_URL must be an https URL, localhost URL, or existing local path for the real built app artifact."
    exit 1
  fi
  if [ -f "$BUILT_ARTIFACT_PATH_URL" ] \
    && [ "$(basename "$BUILT_ARTIFACT_PATH_URL")" = "$RUNTIME_BUILD_PROOF" ] \
    && ! runtime_build_proof_passes "$BUILT_ARTIFACT_PATH_URL"; then
    fail "$BUILT_ARTIFACT_PATH_URL must be a passing current-run /v1/apps/builds proof."
    exit 1
  fi
}

log "starting sign-off helper at $STAMP"

log "Checking build workflow expectations in .github/workflows/build.yml"
if [ ! -f .github/workflows/build.yml ]; then
  fail "Missing .github/workflows/build.yml"
  exit 1
fi

if [ ! -f "$SHIP_DECL" ]; then
  fail "Missing SHIP.md"
  exit 1
fi

log "Checking shipping declarations in SHIP.md"
if ! rg -q "Fedora bootc remains the OS foundation" "$SHIP_DECL"; then
  fail "SHIP.md does not declare Fedora bootc as the OS foundation."
  exit 1
fi
if ! rg -q "custom kernel" "$SHIP_DECL"; then
  fail "SHIP.md does not explicitly state no custom kernel ownership work is planned."
  exit 1
fi
if ! rg -q "OpenAI Sans" "$SHIP_DECL"; then
  fail "SHIP.md does not state OpenAI Sans policy."
  exit 1
fi
if rg -qi --hidden --no-ignore-vcs --no-ignore \
  "OpenAI Sans|openai sans|openai-sans" os .github \
  --glob '!os/hardware-gate/close-signoff.sh' \
  --glob '!os/hardware-gate/verify-shipping-status.sh' \
  | grep -v "^SHIP.md$" >/tmp/ship_openai_sans_hits 2>/dev/null; then
  fail "Unexpected OpenAI Sans references found outside SHIP.md:"
  cat /tmp/ship_openai_sans_hits
  exit 1
fi
if rg -qi --hidden --no-ignore-vcs --no-ignore \
  "licensing\\s+TODO|TODO.*licensing" \
  "$SHIP_DECL" os/hardware-gate/runbook.md os/signoff-notes.md \
  >/tmp/ship_lic_todo 2>/dev/null; then
  fail "Licensing TODO remains present in shipping/OS docs:"
  cat /tmp/ship_lic_todo
  exit 1
fi
rm -f /tmp/ship_openai_sans_hits /tmp/ship_lic_todo
log "SHIP.md declarations and font-policy search checks passed."

require_fixed "rust fmt check missing in workflow" .github/workflows/build.yml "cargo fmt --all --check"
require_fixed "clippy check missing in workflow" .github/workflows/build.yml "clippy --workspace"
require_fixed "native desktop test check missing in workflow" .github/workflows/build.yml 'cargo test --workspace --features "$NATIVE_FEATURES"'
require_fixed "release check missing in workflow" .github/workflows/build.yml "cargo build --release --workspace"
require_fixed "per-architecture image build target missing in workflow" .github/workflows/build.yml 'goblins-os:${{ matrix.arch }}'
require_fixed "self-test target missing in workflow" .github/workflows/build.yml "target: selftest"
require_fixed "installer-iso job missing" .github/workflows/build.yml "installer-iso"
require_fixed "verify blocked=0 gate missing" .github/workflows/build.yml "blocked=0"
log "workflow gates appear present"

LOGIC_ONLY=0
if [ "$(uname -s)" != "Linux" ]; then
warn "Non-Linux host detected; skipping image/self-test commands that require Linux + container runtime."
  LOGIC_ONLY=1
fi

CONTAINER_RUNTIME=""
if [ "$LOGIC_ONLY" -eq 0 ]; then
  CONTAINER_RUNTIME="$(choose_runtime)"
  if [ -z "$CONTAINER_RUNTIME" ]; then
    fail "Docker is required for assisted signoff testing."
    exit 1
  fi
  log "Using container runtime: $CONTAINER_RUNTIME"
fi

log "Checking local image+installer artifacts (if available)"
IMAGE="${GOBLINS_OS_IMAGE:-localhost/goblins-os:$ARCH}"
ISO_PATH="not-found"
ISO_SHA="not-found"
if [ "$LOGIC_ONLY" -eq 0 ] && runtime_image_exists "$CONTAINER_RUNTIME" "$IMAGE"; then
  log "Image exists: $IMAGE"
else
  warn "Image not checked/found: $IMAGE"
fi

expected_iso="os/iso/output/$ARCH/bootiso/goblins-os-$ARCH.iso"
if [ -f "$expected_iso" ]; then
  ISO_PATH="$expected_iso"
  ISO_SHA="$(sha256sum "$ISO_PATH" | awk '{print $1}')"
  log "Latest installer ISO: $ISO_PATH"
  log "SHA256: $ISO_SHA"
else
  warn "No $ARCH ISO found at $expected_iso (if available, verify on Linux host)"
fi

SBOM_DIR="${RELEASE_EVIDENCE_DIR:-os/signoff-proofs/sbom/$ARCH}"
mkdir -p "$SBOM_DIR"

if [ ! -f "$SBOM_DIR/release-evidence-manifest.json" ] \
  || [ ! -f "$SBOM_DIR/cargo-lock-packages.tsv" ] \
  || [ ! -f "$SBOM_DIR/rpm-packages.command" ] \
  || ! release_evidence_manifest_has_diligence_fields "$SBOM_DIR/release-evidence-manifest.json"; then
  log "Generating source release evidence in $SBOM_DIR"
  if ! generate_source_release_evidence "$SBOM_DIR" "$ARCH"; then
    warn "Could not generate source release evidence; run target/release/goblins-os-verify --source-root . --release-evidence $SBOM_DIR --arch $ARCH"
  fi
fi

if [ "$LOGIC_ONLY" -eq 0 ] \
  && runtime_image_exists "$CONTAINER_RUNTIME" "$IMAGE" \
  && [ -f "$SBOM_DIR/rpm-packages.command" ] \
  && [ ! -f "$SBOM_DIR/rpm-packages.tsv" ]; then
  log "Generating RPM release evidence from $IMAGE into $SBOM_DIR"
  if ! run_rpm_release_evidence "$CONTAINER_RUNTIME" "$IMAGE" "$SBOM_DIR"; then
    warn "Could not generate RPM release evidence from $IMAGE; run $SBOM_DIR/rpm-packages.command inside the built image"
  fi
fi

if goblins_os_artifact_secret_scan "$REPO_ROOT"; then
  log "generated artifact/evidence secret scan passed"
else
  fail "Generated artifact/evidence secret scan found possible live secrets."
  exit 1
fi

if release_evidence_complete "$SBOM_DIR"; then
  RELEASE_EVIDENCE_STATUS="yes (manifest, diligence links, Cargo TSV, and RPM TSV present in $SBOM_DIR)"
else
  warn "Release evidence incomplete for $ARCH; expected release-evidence-manifest.json with diligence links plus cargo-lock-packages.tsv and rpm-packages.tsv in $SBOM_DIR"
fi

if [ -n "$SCREENSHOT_DIR" ]; then
  log "Checking required proof screenshots in: $SCREENSHOT_DIR"
  if ! screenshot_dir_matches_arch "$SCREENSHOT_DIR"; then
    fail "SCREENSHOT_DIR must be architecture-specific: os/screenshots/hardware-gate/$ARCH/<date>"
    exit 1
  fi
  if [ ! -d "$SCREENSHOT_DIR" ]; then
    fail "Screenshot directory not found: $SCREENSHOT_DIR"
    exit 1
  fi

  MISSING_SCREENSHOTS=()
  INVALID_SCREENSHOTS=()
  for shot in "${SCREENSHOT_REQUIRED[@]}"; do
    if [ ! -f "$SCREENSHOT_DIR/$shot" ]; then
      MISSING_SCREENSHOTS+=("$shot")
    elif ! screenshot_file_is_valid_png "$SCREENSHOT_DIR/$shot"; then
      INVALID_SCREENSHOTS+=("$shot")
    fi
  done

  if [ "${#MISSING_SCREENSHOTS[@]}" -gt 0 ]; then
    fail "Missing required screenshots (${#MISSING_SCREENSHOTS[@]}): ${MISSING_SCREENSHOTS[*]}"
    fail "Expected all: ${SCREENSHOT_REQUIRED[*]}"
    exit 1
  fi
  if [ "${#INVALID_SCREENSHOTS[@]}" -gt 0 ]; then
    fail "Invalid screenshot PNG files (${#INVALID_SCREENSHOTS[@]}): ${INVALID_SCREENSHOTS[*]}"
    fail "Screenshots must be non-empty PNG captures from the display-backed VM or hardware run."
    exit 1
  fi
  manifest="$SCREENSHOT_DIR/proof-manifest.json"
  if ! screenshot_manifest_matches_iso "$manifest"; then
    fail "Screenshot proof manifest missing or incoherent for this architecture verification proof: $manifest"
    fail "Expected architecture=$ARCH, iso=$ISO_PATH, a 64-character iso_sha256, captured_at, screenshot_run_dir=$SCREENSHOT_DIR, firewall_live_toggle_proof=$FIREWALL_LIVE_TOGGLE_PROOF, text_shortcuts_session_enable_proof=$TEXT_SHORTCUTS_SESSION_ENABLE_PROOF, text_shortcuts_candidate_metadata_proof=$TEXT_SHORTCUTS_CANDIDATE_METADATA_PROOF, text_shortcuts_overlay_intent_proof=$TEXT_SHORTCUTS_OVERLAY_INTENT_PROOF, text_shortcuts_candidate_bubble_frame_proof=$TEXT_SHORTCUTS_CANDIDATE_BUBBLE_FRAME_PROOF, text_shortcuts_candidate_bubble_layout_proof=$TEXT_SHORTCUTS_CANDIDATE_BUBBLE_LAYOUT_PROOF, text_shortcuts_candidate_bubble_render_intent_proof=$TEXT_SHORTCUTS_CANDIDATE_BUBBLE_RENDER_INTENT_PROOF, text_shortcuts_candidate_bubble_render_proof=$TEXT_SHORTCUTS_CANDIDATE_BUBBLE_RENDER_PROOF, text_shortcuts_live_ibus_runtime_render_proof=$TEXT_SHORTCUTS_LIVE_IBUS_RUNTIME_RENDER_PROOF, keyboard_shortcuts_roundtrip_proof=$KEYBOARD_SHORTCUTS_ROUNDTRIP_PROOF, input_sources_roundtrip_proof=$INPUT_SOURCES_ROUNDTRIP_PROOF, multi_display_apply_proof=$MULTI_DISPLAY_APPLY_PROOF, focus_arm_roundtrip_proof=$FOCUS_ARM_ROUNDTRIP_PROOF, app_privacy_revoke_proof=$APP_PRIVACY_REVOKE_PROOF, preview_open_render_proof=$PREVIEW_OPEN_RENDER_PROOF, audio_output_proof=$AUDIO_OUTPUT_PROOF, and runtime_build_proof=$RUNTIME_BUILD_PROOF."
    exit 1
  fi
  screenshot_iso_sha="$(screenshot_manifest_iso_sha "$manifest")"
  log "Screenshot verification ISO SHA256: ${screenshot_iso_sha:-missing}"
  if [ "$ISO_SHA" != "not-found" ]; then
    log "Current hydrated architecture ISO SHA256: $ISO_SHA"
    if [ -n "$screenshot_iso_sha" ] && [ "$screenshot_iso_sha" != "$ISO_SHA" ]; then
      log "Screenshot proof uses verification-only media; hydrated release ISO artifacts are checked separately."
    fi
  fi
  if ! firewall_live_toggle_proof_passes "$SCREENSHOT_DIR/$FIREWALL_LIVE_TOGGLE_PROOF"; then
    fail "Firewall live toggle proof missing or failed: $SCREENSHOT_DIR/$FIREWALL_LIVE_TOGGLE_PROOF"
    fail "Expected live /v1/firewall/enabled disable=200/inactive and enable=200/active through the Goblins OS firewall bridge."
    exit 1
  fi
  if ! text_shortcuts_session_enable_proof_passes "$SCREENSHOT_DIR/$TEXT_SHORTCUTS_SESSION_ENABLE_PROOF"; then
    fail "Text Shortcuts session-enable proof missing or failed: $SCREENSHOT_DIR/$TEXT_SHORTCUTS_SESSION_ENABLE_PROOF"
    fail "Expected active Fedora GNOME IBus service, configured Goblins IBus source/preload, active goblins-textshortcuts engine, adapter self-test pass, and the live core runtime readiness flip."
    exit 1
  fi
  if ! text_shortcuts_candidate_metadata_proof_passes "$SCREENSHOT_DIR/$TEXT_SHORTCUTS_CANDIDATE_METADATA_PROOF"; then
    fail "Text Shortcuts candidate metadata proof missing or failed: $SCREENSHOT_DIR/$TEXT_SHORTCUTS_CANDIDATE_METADATA_PROOF"
    fail "Expected the candidate proof surface to record replacement=on my way, accept_on=word-boundary, dismiss_key=Escape, and rendered_bubble_ready_claim=false without claiming a live overlay."
    exit 1
  fi
  if ! text_shortcuts_overlay_intent_proof_passes "$SCREENSHOT_DIR/$TEXT_SHORTCUTS_OVERLAY_INTENT_PROOF"; then
    fail "Text Shortcuts overlay-intent proof missing or failed: $SCREENSHOT_DIR/$TEXT_SHORTCUTS_OVERLAY_INTENT_PROOF"
    fail "Expected the installed adapter overlay-intent self-test to record show_count=2, hide_count=2, dismissed and committed hide reasons, and no rendered/live/runtime readiness claims."
    exit 1
  fi
  if ! text_shortcuts_candidate_bubble_frame_proof_passes "$SCREENSHOT_DIR/$TEXT_SHORTCUTS_CANDIDATE_BUBBLE_FRAME_PROOF"; then
    fail "Text Shortcuts candidate-bubble-frame proof missing or failed: $SCREENSHOT_DIR/$TEXT_SHORTCUTS_CANDIDATE_BUBBLE_FRAME_PROOF"
    fail "Expected the installed adapter candidate-bubble-frame self-test to record show/hide frames, style classes, Inter font, sensitive-field refusal, and no rendered/live/runtime readiness claims."
    exit 1
  fi
  if ! text_shortcuts_candidate_bubble_layout_proof_passes "$SCREENSHOT_DIR/$TEXT_SHORTCUTS_CANDIDATE_BUBBLE_LAYOUT_PROOF"; then
    fail "Text Shortcuts candidate-bubble-layout proof missing or failed: $SCREENSHOT_DIR/$TEXT_SHORTCUTS_CANDIDATE_BUBBLE_LAYOUT_PROOF"
    fail "Expected the installed adapter candidate-bubble-layout self-test to record layout count, visible count, right-edge clamp, bottom-edge flip, hide-frame collapse, Inter font, and no rendered/live/runtime readiness claims."
    exit 1
  fi
  if ! text_shortcuts_candidate_bubble_render_intent_proof_passes "$SCREENSHOT_DIR/$TEXT_SHORTCUTS_CANDIDATE_BUBBLE_RENDER_INTENT_PROOF"; then
    fail "Text Shortcuts candidate-bubble-render-intent proof missing or failed: $SCREENSHOT_DIR/$TEXT_SHORTCUTS_CANDIDATE_BUBBLE_RENDER_INTENT_PROOF"
    fail "Expected the installed adapter candidate-bubble-render-intent self-test to record show/hide render intents, focus-out and sensitive hides, pass-through unchanged behavior, fail-open sink handling, Inter font, and no rendered/live/runtime readiness claims."
    exit 1
  fi
  if ! text_shortcuts_candidate_bubble_render_proof_passes "$SCREENSHOT_DIR/$TEXT_SHORTCUTS_CANDIDATE_BUBBLE_RENDER_PROOF"; then
    fail "Text Shortcuts candidate-bubble-render screenshot proof missing or failed: $SCREENSHOT_DIR/$TEXT_SHORTCUTS_CANDIDATE_BUBBLE_RENDER_PROOF"
    fail "Expected the display-backed VM to capture 31-text-shortcuts-candidate-bubble-render.png from the render-intent-backed Goblins candidate proof surface while keeping live/runtime readiness claims false."
    exit 1
  fi
  if ! text_shortcuts_live_ibus_runtime_render_proof_passes "$SCREENSHOT_DIR/$TEXT_SHORTCUTS_LIVE_IBUS_RUNTIME_RENDER_PROOF"; then
    fail "Text Shortcuts live IBus runtime/render proof missing or failed: $SCREENSHOT_DIR/$TEXT_SHORTCUTS_LIVE_IBUS_RUNTIME_RENDER_PROOF"
    fail "Expected 32-text-shortcuts-live-ibus-runtime-render.png plus QMP-keyboard-driven active goblins-textshortcuts IBus engine proof with focused-field callback, text-input-v3 commit, password refusal, rendered accept bubble, and core_readiness_flip=live."
    exit 1
  fi
  if ! keyboard_shortcuts_roundtrip_proof_passes "$SCREENSHOT_DIR/$KEYBOARD_SHORTCUTS_ROUNDTRIP_PROOF"; then
    fail "Keyboard shortcuts roundtrip proof missing or failed: $SCREENSHOT_DIR/$KEYBOARD_SHORTCUTS_ROUNDTRIP_PROOF"
    fail "Expected live /v1/keyboard/shortcuts/binding and /v1/keyboard/modifier-remap writes, gsettings read-back, and reset/restore before signoff."
    exit 1
  fi
  if ! input_sources_roundtrip_proof_passes "$SCREENSHOT_DIR/$INPUT_SOURCES_ROUNDTRIP_PROOF"; then
    fail "Input sources roundtrip proof missing or failed: $SCREENSHOT_DIR/$INPUT_SOURCES_ROUNDTRIP_PROOF"
    fail "Expected live /v1/input/sources and /v1/input/switch-next writes, gsettings read-back, and source/current restore before signoff."
    exit 1
  fi
  if ! multi_display_apply_proof_passes "$SCREENSHOT_DIR/$MULTI_DISPLAY_APPLY_PROOF"; then
    fail "Multi-display apply proof missing or failed: $SCREENSHOT_DIR/$MULTI_DISPLAY_APPLY_PROOF"
    fail "Expected live /v1/displays/apply verify + temporary same-layout apply, persistent-confirmation guard, stale-serial rejection, and no persistent keep claim before signoff."
    exit 1
  fi
  if ! focus_arm_roundtrip_proof_passes "$SCREENSHOT_DIR/$FOCUS_ARM_ROUNDTRIP_PROOF"; then
    fail "Focus arm roundtrip proof missing or failed: $SCREENSHOT_DIR/$FOCUS_ARM_ROUNDTRIP_PROOF"
    fail "Expected live /v1/focus/activate and /v1/focus/deactivate writes, active-mode/banner read-back, notification restore, and no mode/schedule/per-app breakthrough claims before signoff."
    exit 1
  fi
  if ! app_privacy_revoke_proof_passes "$SCREENSHOT_DIR/$APP_PRIVACY_REVOKE_PROOF"; then
    fail "App privacy revoke proof missing or failed: $SCREENSHOT_DIR/$APP_PRIVACY_REVOKE_PROOF"
    fail "Expected a seeded portal PermissionStore location grant, live /v1/app-privacy/revoke, GetPermission read-back proving absence, and restore of the prior grant state before signoff."
    exit 1
  fi
  if ! preview_open_render_proof_passes "$SCREENSHOT_DIR/$PREVIEW_OPEN_RENDER_PROOF"; then
    fail "Preview open/render proof missing or failed: $SCREENSHOT_DIR/$PREVIEW_OPEN_RENDER_PROOF"
    fail "Expected /v1/preview/status readiness, /v1/preview/open PDF/image launches, Papers/Loupe defaults, rendered screenshot frames, and unsupported-file rejection before signoff."
    exit 1
  fi
  if ! audio_output_proof_passes "$SCREENSHOT_DIR/$AUDIO_OUTPUT_PROOF"; then
    fail "Audio output proof missing or failed: $SCREENSHOT_DIR/$AUDIO_OUTPUT_PROOF"
    fail "Expected /v1/audio/status output readiness plus a bounded pw-play/paplay test tone while 24-audio-output.png renders the Sound panel."
    exit 1
  fi
  log "All required screenshot proof PNGs and proof manifest passed."
  log "Firewall live toggle proof passed."
  log "Text Shortcuts session-enable proof passed."
  log "Text Shortcuts candidate metadata proof passed."
  log "Text Shortcuts overlay-intent proof passed."
  log "Text Shortcuts candidate-bubble-frame proof passed."
  log "Text Shortcuts candidate-bubble-layout proof passed."
  log "Text Shortcuts candidate-bubble-render-intent proof passed."
  log "Text Shortcuts candidate-bubble-render screenshot proof passed."
  log "Text Shortcuts live IBus runtime/render proof passed."
  log "Keyboard shortcuts roundtrip proof passed."
  log "Input sources roundtrip proof passed."
  log "Multi-display apply proof passed."
  log "Focus arm roundtrip proof passed."
  log "App privacy revoke proof passed."
  log "Preview open/render proof passed."
  log "Audio output proof passed."
  GAMING_SCREENSHOT_STATUS="yes (screenshots ${GAMING_SCREENSHOTS[*]} present)"
  INSTALL_STORAGE_STATUS="yes (screenshots ${INSTALL_STORAGE_SCREENSHOTS[*]} present)"
  MOTION_INTERACTIONS_STATUS="yes (light/dark screenshots present in proof dir)"
  FIREWALL_TOGGLE_STATUS="yes ($FIREWALL_LIVE_TOGGLE_PROOF: disable=200/inactive, enable=200/active)"
  TEXT_SHORTCUTS_SESSION_STATUS="yes ($TEXT_SHORTCUTS_SESSION_ENABLE_PROOF: service/source/engine active; core reports live runtime readiness)"
  TEXT_SHORTCUTS_KEYSTROKE_STATUS="yes (covered by $TEXT_SHORTCUTS_LIVE_IBUS_RUNTIME_RENDER_PROOF + 32-text-shortcuts-live-ibus-runtime-render.png: normal expansion, pass-through, password refusal, focused-field callback, text-input-v3 commit, and rendered accept bubble)"
  TEXT_SHORTCUTS_CANDIDATE_STATUS="yes ($TEXT_SHORTCUTS_CANDIDATE_METADATA_PROOF: candidate metadata present; rendered bubble still gated false)"
  TEXT_SHORTCUTS_OVERLAY_INTENT_STATUS="yes ($TEXT_SHORTCUTS_OVERLAY_INTENT_PROOF: adapter show/hide overlay intents present; live overlay still gated false)"
  TEXT_SHORTCUTS_CANDIDATE_BUBBLE_FRAME_STATUS="yes ($TEXT_SHORTCUTS_CANDIDATE_BUBBLE_FRAME_PROOF: adapter accept-bubble frames present; rendered bubble still gated false)"
  TEXT_SHORTCUTS_CANDIDATE_BUBBLE_LAYOUT_STATUS="yes ($TEXT_SHORTCUTS_CANDIDATE_BUBBLE_LAYOUT_PROOF: adapter accept-bubble layouts present; rendered bubble still gated false)"
  TEXT_SHORTCUTS_CANDIDATE_BUBBLE_RENDER_INTENT_STATUS="yes ($TEXT_SHORTCUTS_CANDIDATE_BUBBLE_RENDER_INTENT_PROOF: adapter render intents present; rendered bubble still gated false)"
  TEXT_SHORTCUTS_CANDIDATE_BUBBLE_RENDER_STATUS="yes ($TEXT_SHORTCUTS_CANDIDATE_BUBBLE_RENDER_PROOF + 31-text-shortcuts-candidate-bubble-render.png: render-intent-backed candidate proof surface rendered; live overlay still gated false)"
  TEXT_SHORTCUTS_LIVE_IBUS_RUNTIME_RENDER_STATUS="yes ($TEXT_SHORTCUTS_LIVE_IBUS_RUNTIME_RENDER_PROOF + 32-text-shortcuts-live-ibus-runtime-render.png: live IBus callback, text-input-v3 commit, password refusal, and rendered accept bubble proved; core readiness flip live)"
  KEYBOARD_SHORTCUTS_ROUNDTRIP_STATUS="yes ($KEYBOARD_SHORTCUTS_ROUNDTRIP_PROOF: shortcut + Caps Lock writes round-tripped and restored)"
  INPUT_SOURCES_ROUNDTRIP_STATUS="yes ($INPUT_SOURCES_ROUNDTRIP_PROOF: input source set + switch writes round-tripped and restored)"
  MULTI_DISPLAY_APPLY_STATUS="yes ($MULTI_DISPLAY_APPLY_PROOF: DisplayConfig verify + temporary same-layout apply, persistent guard, and stale serial rejection proved)"
  FOCUS_ARM_ROUNDTRIP_STATUS="yes ($FOCUS_ARM_ROUNDTRIP_PROOF: Focus activate/deactivate writes round-tripped and notification banners restored)"
  APP_PRIVACY_REVOKE_STATUS="yes ($APP_PRIVACY_REVOKE_PROOF: seeded app permission revoked through PermissionStore and prior state restored)"
  PREVIEW_OPEN_RENDER_STATUS="yes ($PREVIEW_OPEN_RENDER_PROOF: Papers PDF and Loupe image windows opened/rendered in display-backed VM)"
  GAMING_AUDIO_OUTPUT_STATUS="yes ($AUDIO_OUTPUT_PROOF + 24-audio-output.png: /v1/audio/status output ready and bounded local test tone played through PipeWire)"
else
  warn "SCREENSHOT_DIR not set; proof screenshot presence check skipped."
fi

if [ "$LOGIC_ONLY" -eq 0 ] && runtime_image_exists "$CONTAINER_RUNTIME" "$IMAGE"; then
  log "Running installed-root verifier for $IMAGE"
  if run_verify "$CONTAINER_RUNTIME" "$IMAGE" 2>"$VERIFY_ERR" | tee "$VERIFY_LOG"; then
    if grep -q "blocked=0" "$VERIFY_LOG"; then
      log "verify blocked=0 pass"
      VERIFY_STATUS="pass"
    else
      VERIFY_STATUS="fail"
      fail "verify blocked!=0 or missing in output."
      sed -n '1,120p' "$VERIFY_ERR" || true
      exit 1
    fi
  else
    VERIFY_STATUS="fail"
    fail "goblins-os-verify command failed."
    sed -n '1,120p' "$VERIFY_ERR" || true
    exit 1
  fi
else
  warn "Skipping installed-root verify: requires Linux host and container image"
  VERIFY_STATUS="not attempted (linux-only)"
fi
if [ -f "$VERIFY_ERR" ] && [ -s "$VERIFY_ERR" ] && grep -q "." "$VERIFY_ERR"; then
  log "Verifier stderr captured at $VERIFY_ERR"
fi

if [ -f os/bootc/run-selftest.sh ]; then
  if [ -f "$SELFTEST_DOCKERFILE" ]; then
    rm -f "$SELFTEST_DOCKERFILE"
  fi
  cat os/bootc/Containerfile os/bootc/selftest.suffix.Dockerfile > "$SELFTEST_DOCKERFILE"
  if [ "$LOGIC_ONLY" -eq 0 ]; then
    if run_selftest "$CONTAINER_RUNTIME" "$SELFTEST_DOCKERFILE" | tee "$SELFTEST_LOG"; then
      log "self-test build succeeded"
      SELFTEST_STATUS="pass"
    else
      SELFTEST_STATUS="fail"
      fail "self-test container build failed."
      exit 1
    fi
  else
    warn "Skipping self-test container build: requires Linux host with Docker"
    SELFTEST_STATUS="not attempted (linux-only)"
  fi
fi

VERIFY_STATUS="${VERIFY_STATUS:-not attempted}"
SELFTEST_STATUS="${SELFTEST_STATUS:-not attempted}"
validate_runtime_proof_fields
PROJECT_COMPLETION_STATUS="incomplete"
if [ "$VERIFY_STATUS" = "pass" ] \
  && [ "$SELFTEST_STATUS" = "pass" ] \
  && [[ "$RELEASE_EVIDENCE_STATUS" == yes* ]] \
  && [[ "$GAMING_SCREENSHOT_STATUS" == yes* ]] \
  && [[ "$INSTALL_STORAGE_STATUS" == yes* ]] \
  && [[ "$MOTION_INTERACTIONS_STATUS" == yes* ]] \
  && [[ "$FIREWALL_TOGGLE_STATUS" == yes* ]] \
  && [[ "$TEXT_SHORTCUTS_SESSION_STATUS" == yes* ]] \
  && [[ "$TEXT_SHORTCUTS_KEYSTROKE_STATUS" == yes* ]] \
  && [[ "$TEXT_SHORTCUTS_CANDIDATE_STATUS" == yes* ]] \
  && [[ "$TEXT_SHORTCUTS_OVERLAY_INTENT_STATUS" == yes* ]] \
	  && [[ "$TEXT_SHORTCUTS_CANDIDATE_BUBBLE_FRAME_STATUS" == yes* ]] \
	  && [[ "$TEXT_SHORTCUTS_CANDIDATE_BUBBLE_LAYOUT_STATUS" == yes* ]] \
	  && [[ "$TEXT_SHORTCUTS_CANDIDATE_BUBBLE_RENDER_INTENT_STATUS" == yes* ]] \
	  && [[ "$TEXT_SHORTCUTS_CANDIDATE_BUBBLE_RENDER_STATUS" == yes* ]] \
	  && [[ "$TEXT_SHORTCUTS_LIVE_IBUS_RUNTIME_RENDER_STATUS" == yes* ]] \
	  && [[ "$KEYBOARD_SHORTCUTS_ROUNDTRIP_STATUS" == yes* ]] \
  && [[ "$INPUT_SOURCES_ROUNDTRIP_STATUS" == yes* ]] \
  && [[ "$MULTI_DISPLAY_APPLY_STATUS" == yes* ]] \
  && [[ "$FOCUS_ARM_ROUNDTRIP_STATUS" == yes* ]] \
  && [[ "$APP_PRIVACY_REVOKE_STATUS" == yes* ]] \
  && [[ "$PREVIEW_OPEN_RENDER_STATUS" == yes* ]] \
  && [[ "$GAMING_AUDIO_OUTPUT_STATUS" == yes* ]] \
  && [ "$ISO_PATH" != "not-found" ] \
  && [ "$ISO_SHA" != "not-found" ] \
  && proof_field_is_real "$RUNTIME_ENGINE_MODE" \
  && proof_field_is_real "$RUNTIME_ENGINE_SOURCE" \
  && built_artifact_reference_is_real "$BUILT_ARTIFACT_PATH_URL"; then
  PROJECT_COMPLETION_STATUS="complete"
fi

if [ "$LOGIC_ONLY" -eq 1 ]; then
  log "Sign-off helper completed in static-only mode."
  log "Remaining Linux/VM steps: installer-ISO build, installed-root verify, self-test, real hardware boot, onboarding/session, shell/settings, gaming proof, Build Studio app run."
else
  log "Sign-off helper completed. External steps remaining: hardware/VM install flow, session unlock, shell/settings launch, gaming proof, Build Studio real engine turn, screenshot proof in os/screenshots/hardware-gate/$ARCH/<date>/, and filled status in $OUT"
fi
cat >> "$OUT" <<EOF2

## Manual Gate Run: $STAMP (script assisted)
- Runner: ${SIGNOFF_RUNNER_VALUE}
- CI workflow references: verified in-repo at .github/workflows/build.yml
- Architecture: ${ARCH}
- CI run IDs/URLs:
  - rust: ${CI_RUST_URL:-not provided}
  - image: ${CI_IMAGE_URL:-not provided}
  - installer-iso: ${CI_INSTALLER_ISO_URL:-not provided}
- Image: ${IMAGE}
- ISO: ${ISO_PATH}
- ISO SHA256: ${ISO_SHA}
- Rootfs verify command: \
  ${CONTAINER_RUNTIME:-docker} run --rm ${IMAGE} /usr/libexec/goblins-os/goblins-os-verify --installed-root /
- Verify result (blocked=0): ${VERIFY_STATUS}
- Self-test command: DOCKER_BUILDKIT=1 ${CONTAINER_RUNTIME:-docker} buildx build -f ${SELFTEST_DOCKERFILE} --target selftest --output type=cacheonly .
- Self-test log: $SELFTEST_LOG
- Self-test result: $SELFTEST_STATUS
- Rootfs verify output: $VERIFY_LOG
- Release evidence/SBOM checked: ${RELEASE_EVIDENCE_STATUS}
- Screenshot dir: ${SCREENSHOT_DIR:-not provided}
- Runtime engine run:
  - mode: ${RUNTIME_ENGINE_MODE}
  - engine source: ${RUNTIME_ENGINE_SOURCE}
  - config path/artifact: ${RUNTIME_ENGINE_CONFIG}
  - built artifact path/URL: ${BUILT_ARTIFACT_PATH_URL}
- Motion/interactions checked: ${MOTION_INTERACTIONS_STATUS}
- Firewall live toggle checked: ${FIREWALL_TOGGLE_STATUS}
- Text Shortcuts session enablement checked: ${TEXT_SHORTCUTS_SESSION_STATUS}
- Text Shortcuts live keystrokes checked: ${TEXT_SHORTCUTS_KEYSTROKE_STATUS}
- Text Shortcuts candidate metadata checked: ${TEXT_SHORTCUTS_CANDIDATE_STATUS}
- Text Shortcuts overlay intent checked: ${TEXT_SHORTCUTS_OVERLAY_INTENT_STATUS}
- Text Shortcuts candidate bubble frame checked: ${TEXT_SHORTCUTS_CANDIDATE_BUBBLE_FRAME_STATUS}
- Text Shortcuts candidate bubble layout checked: ${TEXT_SHORTCUTS_CANDIDATE_BUBBLE_LAYOUT_STATUS}
- Text Shortcuts candidate bubble render intent checked: ${TEXT_SHORTCUTS_CANDIDATE_BUBBLE_RENDER_INTENT_STATUS}
- Text Shortcuts candidate bubble render screenshot checked: ${TEXT_SHORTCUTS_CANDIDATE_BUBBLE_RENDER_STATUS}
- Text Shortcuts live IBus runtime/render checked: ${TEXT_SHORTCUTS_LIVE_IBUS_RUNTIME_RENDER_STATUS}
- Keyboard shortcuts roundtrip checked: ${KEYBOARD_SHORTCUTS_ROUNDTRIP_STATUS}
- Input sources roundtrip checked: ${INPUT_SOURCES_ROUNDTRIP_STATUS}
- Multi-display apply checked: ${MULTI_DISPLAY_APPLY_STATUS}
- Focus arm roundtrip checked: ${FOCUS_ARM_ROUNDTRIP_STATUS}
- App privacy revoke checked: ${APP_PRIVACY_REVOKE_STATUS}
- Preview open/render checked: ${PREVIEW_OPEN_RENDER_STATUS}
- Audio output checked: ${GAMING_AUDIO_OUTPUT_STATUS}
- Gaming readiness checked: ${GAMING_SCREENSHOT_STATUS}
- Install storage/bootloader/dual-boot checked: ${INSTALL_STORAGE_STATUS}
- Current project completion status: ${PROJECT_COMPLETION_STATUS}
EOF2

log "Appended scaffold entry to $OUT"
