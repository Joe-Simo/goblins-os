#!/usr/bin/env bash
set -euo pipefail

REPO_ROOT="${REPO_ROOT:-$(cd "$(dirname "$0")/../.." && pwd)}"
cd "$REPO_ROOT"
. "$REPO_ROOT/os/hardware-gate/secret-scan.sh"
. "$REPO_ROOT/os/hardware-gate/rpm-sbom-arch.sh"
. "$REPO_ROOT/os/hardware-gate/release-evidence.sh"

log() { echo "[signoff] $*"; }
warn() { echo "[signoff][warn] $*" >&2; }
fail() { echo "[signoff][fail] $*" >&2; }

STAMP="$(date -u +%Y-%m-%dT%H%M%SZ)"
OUT="os/signoff-notes.md"
SHIP_DECL="SHIP.md"
SCREENSHOT_DIR="${SCREENSHOT_DIR:-${SCREENSHOT_RUN_DIR:-}}"
IMAGE_PROVENANCE_REF=""
NATIVE_PACKAGING_GATE_PROOF="${GOBLINS_OS_NATIVE_PACKAGING_GATE_PROOF:-}"
NATIVE_PACKAGING_GATE_RUN_URL="${GOBLINS_OS_NATIVE_PACKAGING_GATE_RUN_URL:-}"
NATIVE_PACKAGING_GATE_RUN_ATTEMPT="${GOBLINS_OS_NATIVE_PACKAGING_GATE_RUN_ATTEMPT:-}"
NATIVE_PACKAGING_GATE_STATUS="not used"
SIGNOFF_ROW_OUTPUT="${SIGNOFF_ROW_OUTPUT:-}"
REQUIRE_COMPLETE="${REQUIRE_COMPLETE:-0}"
case "$REQUIRE_COMPLETE" in
  0|1) ;;
  *)
    fail "REQUIRE_COMPLETE must be 0 or 1."
    exit 2
    ;;
esac
normalize_arch() {
  case "$1" in
    aarch64|arm64) echo "aarch64" ;;
    x86_64|amd64) echo "x86_64" ;;
    *) echo "unsupported" ;;
  esac
}
image_ref_is_digest_pinned() {
  [[ "$1" =~ ^[^[:space:]@]+@sha256:[0-9a-f]{64}$ ]]
}
sha256_file() {
  if command -v sha256sum >/dev/null 2>&1; then
    sha256sum "$1" | awk '{print $1}'
  elif command -v shasum >/dev/null 2>&1; then
    shasum -a 256 "$1" | awk '{print $1}'
  else
    fail "sha256sum or shasum is required to hash signoff artifacts."
    return 1
  fi
}
ARCH="$(normalize_arch "${GOBLINS_OS_ARCH:-$(uname -m)}")"
if [ "$ARCH" = "unsupported" ]; then
  fail "Unsupported architecture '${GOBLINS_OS_ARCH:-$(uname -m)}'; expected aarch64 or x86_64."
  exit 1
fi
CANDIDATE_COMMIT="${GOBLINS_OS_CANDIDATE_COMMIT:-${GITHUB_SHA:-}}"
if [[ ! "$CANDIDATE_COMMIT" =~ ^[0-9a-fA-F]{40}$ ]]; then
  fail "GOBLINS_OS_CANDIDATE_COMMIT must identify the exact 40-hex source commit selected for this signoff."
  exit 1
fi
CANDIDATE_COMMIT="$(printf '%s' "$CANDIDATE_COMMIT" | tr '[:upper:]' '[:lower:]')"
export GOBLINS_OS_CANDIDATE_COMMIT="$CANDIDATE_COMMIT"
VERIFY_LOG="/tmp/goblins-os-verify.log"
VERIFY_ERR="/tmp/verify.err"
SELFTEST_LOG="/tmp/goblins-os-selftest.log"
SELFTEST_DOCKERFILE="/tmp/selftest.Dockerfile"
BASE_SCREENSHOTS=(
  "01-installer.png"
  "02-install-network.png"
  "03-login.png"
  "04-desktop.png"
  "05-first-boot-private-unlock.png"
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
SCREENSHOT_ISO_SHA="not checked"
EVIDENCE_BUNDLE_STATUS="not checked"
EVIDENCE_BUNDLE_SHA256="not checked"
EVIDENCE_BUNDLE_PATH="not provided"
LOCAL_DISPLAY_ATTESTATION_STATUS="not required"
LOCAL_DISPLAY_ATTESTATION_PATH="not provided"
LOCAL_DISPLAY_ATTESTATION_RUN="not provided"
LOCAL_DISPLAY_ATTESTATION_RUN_ATTEMPT="not provided"
LOCAL_DISPLAY_ATTESTATION_ARTIFACT="not provided"
ISO_CANDIDATE_STATUS="not checked"
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
CAPTURE_WORKFLOW_RUN_URL="${GOBLINS_OS_CAPTURE_WORKFLOW_RUN_URL:-${CI_RUN_URL:-}}"
CAPTURE_WORKFLOW_RUN_ATTEMPT="${GOBLINS_OS_CAPTURE_WORKFLOW_RUN_ATTEMPT:-${GITHUB_RUN_ATTEMPT:-0}}"

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

native_packaging_gate_proof_passes() {
  local proof="$1"
  local run_url="$2"
  local run_attempt="$3"
  local arch="$4"
  local commit="$5"
  local image_ref="$6"
  local iso_sha="$7"
  local iso_manifest_sha="$8"
  local bib_manifest_sha="$9"
  local evidence_manifest_sha="${10}"

  [ -s "$proof" ] || return 1
  [[ "$run_url" =~ ^https://github\.com/Joe-Simo/goblins-os/actions/runs/[0-9]+$ ]] || return 1
  [[ "$run_attempt" =~ ^[1-9][0-9]*$ ]] || return 1
  python3 - \
    "$proof" \
    "$run_url" \
    "$run_attempt" \
    "$arch" \
    "$commit" \
    "$image_ref" \
    "$iso_sha" \
    "$iso_manifest_sha" \
    "$bib_manifest_sha" \
    "$evidence_manifest_sha" <<'PY'
import json
import sys

(
    path,
    run_url,
    run_attempt,
    arch,
    commit,
    image_ref,
    iso_sha,
    iso_manifest_sha,
    bib_manifest_sha,
    evidence_manifest_sha,
) = sys.argv[1:11]
with open(path, encoding="utf-8") as handle:
    proof = json.load(handle)
expected = {
    "schema": "goblins-os-native-packaging-gate-v1",
    "architecture": arch,
    "candidate_commit": commit,
    "image_ref": image_ref,
    "image_digest_pinned": True,
    "source_verifier": "pass",
    "installed_root_verifier": "pass",
    "services_selftest": "pass",
    "verification_iso_sha256": iso_sha,
    "iso_manifest_sha256": iso_manifest_sha,
    "bib_manifest_sha256": bib_manifest_sha,
    "release_evidence_manifest_sha256": evidence_manifest_sha,
    "runner_os": "Linux",
    "runner_architecture": arch,
    "native_runner": True,
    "source_repository": run_url.split("/actions/runs/", 1)[0],
    "workflow_run": run_url,
    "workflow_run_attempt": int(run_attempt),
}
raise SystemExit(0 if all(proof.get(key) == value for key, value in expected.items()) else 1)
PY
}

generate_source_release_evidence() {
  local output_dir="$1"
  local arch="$2"

  if [ -x target/release/goblins-os-verify ]; then
    target/release/goblins-os-verify \
      --source-root . \
      --release-evidence "$output_dir" \
      --arch "$arch" \
      --candidate-commit "$CANDIDATE_COMMIT" \
      --image-ref "$IMAGE_PROVENANCE_REF"
    return
  fi

  if command -v cargo >/dev/null 2>&1; then
    cargo run -p goblins-os-verify -- \
      --source-root . \
      --release-evidence "$output_dir" \
      --arch "$arch" \
      --candidate-commit "$CANDIDATE_COMMIT" \
      --image-ref "$IMAGE_PROVENANCE_REF"
    return
  fi

  return 1
}

generate_image_release_evidence() {
  local runtime="$1"
  local image="$2"
  local output_dir="$3"
  local arch="$4"
  local output_abs repo_abs
  output_abs="$(cd "$output_dir" && pwd)"
  repo_abs="$(pwd -P)"

  case "$runtime" in
    docker)
      docker run --rm \
        -v "$repo_abs:/workspace:ro" \
        -v "$output_abs:/out" \
        -w /workspace \
        "$image" \
        /usr/libexec/goblins-os/goblins-os-verify \
        --source-root /workspace \
        --release-evidence /out \
        --arch "$arch" \
        --candidate-commit "$CANDIDATE_COMMIT" \
        --image-ref "$IMAGE_PROVENANCE_REF"
      ;;
    *)
      return 1
      ;;
  esac
}

release_evidence_manifest_has_diligence_fields() {
  local manifest="$1"
  [ -f "$manifest" ] \
    && grep -Fq '"candidate_commit": "'"$CANDIDATE_COMMIT"'"' "$manifest" \
    && grep -Fq '"image_ref": "'"$IMAGE_PROVENANCE_REF"'"' "$manifest" \
    && grep -Fq '"image_digest_pinned": true' "$manifest" \
    && grep -Fq '"asset_provenance": "os/release/asset-provenance.toml"' "$manifest" \
    && grep -Fq '"third_party_notices": "os/release/third-party-notices.toml"' "$manifest" \
    && grep -Fq '"trademark_posture": "os/release/trademark-posture.toml"' "$manifest" \
    && grep -Fq '"source_tree_manifest": "os/release/source-tree-manifest.toml"' "$manifest"
}

release_evidence_complete() {
  local output_dir="$1"
  local manifest="$output_dir/release-evidence-manifest.json"
  release_evidence_manifest_has_diligence_fields "$manifest" \
    && goblins_os_release_evidence_hashes_match "$output_dir" \
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

proof_json_passes() {
  local proof="$1"
  local schema="${2:-$(basename "$proof" -proof.json)}"

  python3 "$REPO_ROOT/os/hardware-gate/capture-harness/proof_validation.py" \
    --proof "$schema" "$proof"
}

evidence_bundle_passes() {
  local run_dir="$1"
  local run_date="${run_dir%/}"
  run_date="${run_date##*/}"

  python3 "$REPO_ROOT/os/hardware-gate/capture-harness/evidence_bundle.py" verify \
    --repository "$REPO_ROOT" \
    --run-dir "$run_dir" \
    --architecture "$ARCH" \
    --candidate-commit "$CANDIDATE_COMMIT" \
    --image-ref "$IMAGE_PROVENANCE_REF" \
    --run-date "$run_date"
}

local_display_attestation_fields() {
  local run_dir="$1"
  local run_date="${run_dir%/}"
  run_date="${run_date##*/}"

  python3 "$REPO_ROOT/os/hardware-gate/capture-harness/evidence_bundle.py" \
    verify-attestation \
    --seal "$run_dir/evidence-bundle.json" \
    --record "$run_dir/aarch64-local-display-attestation.json" \
    --candidate-commit "$CANDIDATE_COMMIT" \
    --image-ref "$IMAGE_PROVENANCE_REF" \
    --run-date "$run_date"
}

github_actions_run_is_successful() {
  local run_url="$1"
  local expected_commit="$2"
  local expected_attempt="$3"
  local expected_workflow_path="$4"
  local run_id

  [[ "$run_url" =~ ^https://github\.com/Joe-Simo/goblins-os/actions/runs/[0-9]+$ ]] || return 1
  [[ "$expected_commit" =~ ^[0-9a-f]{40}$ ]] || return 1
  [[ "$expected_attempt" =~ ^[1-9][0-9]*$ ]] || return 1
  run_id="${run_url##*/}"
  python3 - "$run_id" "$run_url" "$expected_commit" "$expected_attempt" "$expected_workflow_path" <<'PY'
import json
import os
import sys
import urllib.request

run_id, run_url, expected_commit, expected_attempt, expected_workflow_path = sys.argv[1:6]
request = urllib.request.Request(
    f"https://api.github.com/repos/Joe-Simo/goblins-os/actions/runs/{run_id}/attempts/{expected_attempt}",
    headers={
        "Accept": "application/vnd.github+json",
        "User-Agent": "goblins-os-release-verifier",
        "X-GitHub-Api-Version": "2022-11-28",
    },
)
token = os.environ.get("GH_TOKEN") or os.environ.get("GITHUB_TOKEN")
if token:
    request.add_header("Authorization", f"Bearer {token}")
try:
    with urllib.request.urlopen(request, timeout=20) as response:
        run = json.load(response)
except Exception:
    raise SystemExit(1)

expected = {
    "html_url": run_url,
    "status": "completed",
    "conclusion": "success",
    "head_sha": expected_commit,
    "run_attempt": int(expected_attempt),
    "event": "workflow_dispatch",
    "path": expected_workflow_path,
}
if run.get("repository", {}).get("full_name") != "Joe-Simo/goblins-os":
    raise SystemExit(1)
raise SystemExit(0 if all(run.get(key) == value for key, value in expected.items()) else 1)
PY
}

github_actions_artifact_file_matches() {
  local run_url="$1"
  local artifact_name="$2"
  local local_file="$3"
  local expected_basename="$4"
  local run_id scratch_dir downloaded_file file_count result

  command -v gh >/dev/null 2>&1 || return 1
  [[ "$run_url" =~ ^https://github\.com/Joe-Simo/goblins-os/actions/runs/[0-9]+$ ]] || return 1
  [ -s "$local_file" ] && [ ! -L "$local_file" ] || return 1
  run_id="${run_url##*/}"
  scratch_dir="$(mktemp -d "${TMPDIR:-/tmp}/goblins-actions-artifact.XXXXXX")" || return 1
  if ! gh run download "$run_id" \
    --repo Joe-Simo/goblins-os \
    --name "$artifact_name" \
    --dir "$scratch_dir" >/dev/null 2>&1; then
    rm -rf "$scratch_dir"
    return 1
  fi
  file_count="$(find "$scratch_dir" -type f -name "$expected_basename" -print | awk 'END { print NR + 0 }')"
  downloaded_file="$(find "$scratch_dir" -type f -name "$expected_basename" -print -quit)"
  result=1
  if [ "$file_count" = "1" ] \
    && [ -n "$downloaded_file" ] \
    && python3 - "$local_file" "$downloaded_file" <<'PY'
import os
import stat
import sys

MAX_ARTIFACT_PROOF_BYTES = 16 * 1024 * 1024


def read_stable_regular_file(path: str) -> bytes:
    before = os.lstat(path)
    if (
        not stat.S_ISREG(before.st_mode)
        or before.st_nlink != 1
        or before.st_uid != os.getuid()
        or before.st_size < 1
        or before.st_size > MAX_ARTIFACT_PROOF_BYTES
    ):
        raise RuntimeError("artifact proof is not a bounded private regular file")
    descriptor = os.open(
        path,
        os.O_RDONLY | getattr(os, "O_NOFOLLOW", 0) | getattr(os, "O_CLOEXEC", 0),
    )
    try:
        opened = os.fstat(descriptor)
        identity = (before.st_dev, before.st_ino, before.st_mode, before.st_nlink)
        if identity != (opened.st_dev, opened.st_ino, opened.st_mode, opened.st_nlink):
            raise RuntimeError("artifact proof changed before it was opened")
        chunks = []
        remaining = before.st_size
        while remaining:
            chunk = os.read(descriptor, min(1024 * 1024, remaining))
            if not chunk:
                raise RuntimeError("artifact proof was truncated while reading")
            chunks.append(chunk)
            remaining -= len(chunk)
        if os.read(descriptor, 1):
            raise RuntimeError("artifact proof grew while reading")
        after_open = os.fstat(descriptor)
    finally:
        os.close(descriptor)
    after_path = os.lstat(path)
    stable_fields = ("st_dev", "st_ino", "st_mode", "st_nlink", "st_size", "st_mtime_ns", "st_ctime_ns")
    if any(getattr(before, field) != getattr(after_open, field) for field in stable_fields):
        raise RuntimeError("artifact proof changed while reading")
    if any(getattr(before, field) != getattr(after_path, field) for field in stable_fields):
        raise RuntimeError("artifact proof path changed while reading")
    return b"".join(chunks)


try:
    local_bytes = read_stable_regular_file(sys.argv[1])
    downloaded_bytes = read_stable_regular_file(sys.argv[2])
except (OSError, RuntimeError):
    raise SystemExit(1)
raise SystemExit(0 if local_bytes == downloaded_bytes else 1)
PY
  then
    result=0
  fi
  rm -rf "$scratch_dir"
  return "$result"
}

local_display_attestation_signature_passes() {
  local seal="$1"

  command -v gh >/dev/null 2>&1 || return 1
  gh attestation verify "$seal" \
    --repo Joe-Simo/goblins-os \
    --signer-workflow Joe-Simo/goblins-os/.github/workflows/aarch64-local-display-attestation.yml \
    --signer-digest "$CANDIDATE_COMMIT" \
    --source-digest "$CANDIDATE_COMMIT" \
    --deny-self-hosted-runners \
    >/dev/null 2>&1
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

  "$REPO_ROOT/os/hardware-gate/capture-harness/run-capture.sh" \
    --check-png "$file"
}

screenshot_manifest_matches_iso() {
  local manifest="$1"
  local verification_evidence_manifest="$SCREENSHOT_DIR/verification-release-evidence-manifest.json"
  local recorded_evidence_manifest_sha actual_evidence_manifest_sha
  local live_proof live_screenshot recorded_manifest_screenshot_sha
  local recorded_proof_screenshot_sha actual_screenshot_sha

  [ -s "$manifest" ] || return 1
  rg -q '"architecture"[[:space:]]*:[[:space:]]*"'"$ARCH"'"' "$manifest" \
    && rg -q '"candidate_commit"[[:space:]]*:[[:space:]]*"'"$CANDIDATE_COMMIT"'"' "$manifest" \
    && rg -q '"image_ref"[[:space:]]*:[[:space:]]*"'"$IMAGE_PROVENANCE_REF"'"' "$manifest" \
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
    && rg -q '"text_shortcuts_live_ibus_runtime_render_screenshot_sha256"[[:space:]]*:[[:space:]]*"[0-9a-f]{64}"' "$manifest" \
    && rg -q '"keyboard_shortcuts_roundtrip_proof"[[:space:]]*:[[:space:]]*"'"$KEYBOARD_SHORTCUTS_ROUNDTRIP_PROOF"'"' "$manifest" \
    && rg -q '"input_sources_roundtrip_proof"[[:space:]]*:[[:space:]]*"'"$INPUT_SOURCES_ROUNDTRIP_PROOF"'"' "$manifest" \
    && rg -q '"multi_display_apply_proof"[[:space:]]*:[[:space:]]*"'"$MULTI_DISPLAY_APPLY_PROOF"'"' "$manifest" \
    && rg -q '"focus_arm_roundtrip_proof"[[:space:]]*:[[:space:]]*"'"$FOCUS_ARM_ROUNDTRIP_PROOF"'"' "$manifest" \
    && rg -q '"app_privacy_revoke_proof"[[:space:]]*:[[:space:]]*"'"$APP_PRIVACY_REVOKE_PROOF"'"' "$manifest" \
    && rg -q '"preview_open_render_proof"[[:space:]]*:[[:space:]]*"'"$PREVIEW_OPEN_RENDER_PROOF"'"' "$manifest" \
    && rg -q '"audio_output_proof"[[:space:]]*:[[:space:]]*"'"$AUDIO_OUTPUT_PROOF"'"' "$manifest" \
    && rg -q '"runtime_build_proof"[[:space:]]*:[[:space:]]*"'"$RUNTIME_BUILD_PROOF"'"' "$manifest" \
    || return 1
  recorded_evidence_manifest_sha="$(awk -F'"' '/"verification_release_evidence_manifest_sha256"/ { print $4; exit }' "$manifest")"
  actual_evidence_manifest_sha="$(sha256_file "$verification_evidence_manifest")" || return 1
  [[ "$recorded_evidence_manifest_sha" =~ ^[0-9a-f]{64}$ ]] \
    && [ "$recorded_evidence_manifest_sha" = "$actual_evidence_manifest_sha" ] \
    || return 1

  [ "$(rg -c '"text_shortcuts_live_ibus_runtime_render_screenshot_sha256"[[:space:]]*:' "$manifest")" = "1" ] \
    || return 1
  live_proof="$SCREENSHOT_DIR/$TEXT_SHORTCUTS_LIVE_IBUS_RUNTIME_RENDER_PROOF"
  live_screenshot="$SCREENSHOT_DIR/32-text-shortcuts-live-ibus-runtime-render.png"
  recorded_manifest_screenshot_sha="$(awk -F'"' '/"text_shortcuts_live_ibus_runtime_render_screenshot_sha256"/ { print $4; exit }' "$manifest")"
  recorded_proof_screenshot_sha="$(awk -F'"' '/"screenshot_sha256"/ { print $4; exit }' "$live_proof")"
  actual_screenshot_sha="$(sha256_file "$live_screenshot")" || return 1
  [[ "$recorded_manifest_screenshot_sha" =~ ^[0-9a-f]{64}$ ]] \
    && [ "$recorded_manifest_screenshot_sha" = "$recorded_proof_screenshot_sha" ] \
    && [ "$recorded_manifest_screenshot_sha" = "$actual_screenshot_sha" ] \
    && screenshot_file_is_valid_png "$live_screenshot"
}

screenshot_manifest_iso_sha() {
  awk -F'"' '/"iso_sha256"/ { print $4; exit }' "$1" 2>/dev/null || true
}

semantic_screenshot_frames_are_distinct() {
  "$REPO_ROOT/os/hardware-gate/capture-harness/run-capture.sh" \
    --check-semantic-screenshots "$1" "${2:-verbose}"
}

firewall_live_toggle_proof_passes() {
  local proof="$1"

  proof_json_passes "$proof" || return 1
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

  proof_json_passes "$proof" || return 1
  [ -s "$proof" ] \
    && rg -q '"status"[[:space:]]*:[[:space:]]*"pass"' "$proof" \
    && rg -q '"route"[[:space:]]*:[[:space:]]*"/v1/text-shortcuts"' "$proof" \
    && rg -q '"proof_scope"[[:space:]]*:[[:space:]]*"session-plumbing"' "$proof" \
    && rg -q '"service"[[:space:]]*:[[:space:]]*"active"' "$proof" \
    && rg -q '"service_unit"[[:space:]]*:[[:space:]]*"org.freedesktop.IBus.session.GNOME.service"' "$proof" \
    && rg -q '"input_source_configured"[[:space:]]*:[[:space:]]*"true"' "$proof" \
    && rg -q '"preload_configured"[[:space:]]*:[[:space:]]*"true"' "$proof" \
    && rg -q '"engine_listed"[[:space:]]*:[[:space:]]*"true"' "$proof" \
    && rg -q '"active_engine"[[:space:]]*:[[:space:]]*"goblins-textshortcuts"' "$proof" \
    && rg -q '"adapter_self_test"[[:space:]]*:[[:space:]]*"pass"' "$proof" \
    && rg -q '"core_http"[[:space:]]*:[[:space:]]*"200"' "$proof" \
    && rg -q '"core_ibus_available"[[:space:]]*:[[:space:]]*"true"' "$proof" \
    && rg -q '"core_component_registered"[[:space:]]*:[[:space:]]*"true"' "$proof" \
    && rg -q '"core_engine_binary_available"[[:space:]]*:[[:space:]]*"true"' "$proof" \
    && rg -q '"core_input_source_configured"[[:space:]]*:[[:space:]]*"true"' "$proof" \
    && rg -q '"runtime_ready_claim"[[:space:]]*:[[:space:]]*"false"' "$proof" \
    && { { rg -q '"core_engine_available"[[:space:]]*:[[:space:]]*"true"' "$proof" \
           && rg -q '"core_runtime_loop_available"[[:space:]]*:[[:space:]]*"true"' "$proof"; } \
         || { rg -q '"core_engine_available"[[:space:]]*:[[:space:]]*"false"' "$proof" \
              && rg -q '"core_runtime_loop_available"[[:space:]]*:[[:space:]]*"false"' "$proof"; }; }
}

text_shortcuts_candidate_metadata_proof_passes() {
  local proof="$1"

  proof_json_passes "$proof" || return 1
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

  proof_json_passes "$proof" || return 1
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

  proof_json_passes "$proof" || return 1
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

  proof_json_passes "$proof" || return 1
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

  proof_json_passes "$proof" || return 1
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
    && rg -q '"key_release_preserved_candidate"[[:space:]]*:[[:space:]]*"true"' "$proof" \
    && rg -q '"runtime_failure_cleanup"[[:space:]]*:[[:space:]]*"true"' "$proof" \
    && rg -q '"sink_failure_fail_open"[[:space:]]*:[[:space:]]*"true"' "$proof" \
    && rg -q '"style_class"[[:space:]]*:[[:space:]]*"gos-text-shortcuts-candidate"' "$proof" \
    && rg -q '"font_family"[[:space:]]*:[[:space:]]*"Inter"' "$proof" \
    && rg -q '"rendered_bubble_ready_claim"[[:space:]]*:[[:space:]]*"false"' "$proof" \
    && rg -q '"live_overlay_claim"[[:space:]]*:[[:space:]]*"false"' "$proof" \
	    && rg -q '"runtime_ready_claim"[[:space:]]*:[[:space:]]*"false"' "$proof"
}

text_shortcuts_candidate_bubble_render_proof_passes() {
  local proof="$1"

  proof_json_passes "$proof" || return 1
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
  local screenshot recorded_sha actual_sha

  proof_json_passes "$proof" || return 1
  [ -s "$proof" ] \
    && rg -q '"status"[[:space:]]*:[[:space:]]*"pass"' "$proof" \
    && rg -q '"route"[[:space:]]*:[[:space:]]*"/v1/text-shortcuts"' "$proof" \
    && rg -q '"preview_route"[[:space:]]*:[[:space:]]*"/v1/text-shortcuts/preview"' "$proof" \
    && rg -q '"surface"[[:space:]]*:[[:space:]]*"goblins-textshortcuts-live-ibus-runtime-render"' "$proof" \
    && rg -q '"input_driver"[[:space:]]*:[[:space:]]*"qmp-keyboard"' "$proof" \
    && rg -q '"active_engine"[[:space:]]*:[[:space:]]*"goblins-textshortcuts"' "$proof" \
    && rg -q '"core_write_http"[[:space:]]*:[[:space:]]*"200"' "$proof" \
    && rg -q '"core_read_http"[[:space:]]*:[[:space:]]*"200"' "$proof" \
    && rg -q '"core_preview_http"[[:space:]]*:[[:space:]]*"200"' "$proof" \
    && rg -q '"file_contract_http"[[:space:]]*:[[:space:]]*"200"' "$proof" \
    && rg -q '"seed_write_http"[[:space:]]*:[[:space:]]*"200"' "$proof" \
    && rg -q '"seed_read_http"[[:space:]]*:[[:space:]]*"200"' "$proof" \
    && rg -q '"seed_roundtrip"[[:space:]]*:[[:space:]]*"true"' "$proof" \
    && rg -q '"seed_loaded"[[:space:]]*:[[:space:]]*"true"' "$proof" \
    && rg -q '"core_table_roundtrip"[[:space:]]*:[[:space:]]*"true"' "$proof" \
    && rg -q '"core_preview_roundtrip"[[:space:]]*:[[:space:]]*"true"' "$proof" \
    && rg -q '"desktop_file_contract"[[:space:]]*:[[:space:]]*"true"' "$proof" \
    && rg -q '"desktop_parent_contract"[[:space:]]*:[[:space:]]*"true"' "$proof" \
    && rg -q '"desktop_file_owner_mode"[[:space:]]*:[[:space:]]*"true"' "$proof" \
    && rg -q '"desktop_file_single_link"[[:space:]]*:[[:space:]]*"true"' "$proof" \
    && rg -q '"desktop_file_size_bounded"[[:space:]]*:[[:space:]]*"true"' "$proof" \
    && rg -q '"desktop_file_bounded_read"[[:space:]]*:[[:space:]]*"true"' "$proof" \
    && rg -q '"legacy_service_table_absent"[[:space:]]*:[[:space:]]*"true"' "$proof" \
    && rg -q '"live_watcher_reload"[[:space:]]*:[[:space:]]*"true"' "$proof" \
    && rg -q '"post_keystroke_read_http"[[:space:]]*:[[:space:]]*"200"' "$proof" \
    && rg -q '"post_keystroke_file_http"[[:space:]]*:[[:space:]]*"200"' "$proof" \
    && rg -q '"post_keystroke_roundtrip"[[:space:]]*:[[:space:]]*"true"' "$proof" \
    && rg -q '"normal_actual"[[:space:]]*:[[:space:]]*"on my way\."' "$proof" \
    && rg -q '"passthrough_actual"[[:space:]]*:[[:space:]]*"hello\."' "$proof" \
    && rg -q '"password_refusal"[[:space:]]*:[[:space:]]*"true"' "$proof" \
    && rg -q '"password_sensitive_purpose"[[:space:]]*:[[:space:]]*"true"' "$proof" \
    && rg -q '"password_process_key_callback"[[:space:]]*:[[:space:]]*"true"' "$proof" \
    && rg -q '"password_commit_absent"[[:space:]]*:[[:space:]]*"true"' "$proof" \
    && rg -q '"password_candidate_absent"[[:space:]]*:[[:space:]]*"true"' "$proof" \
    && rg -q '"password_popup_absent"[[:space:]]*:[[:space:]]*"true"' "$proof" \
    && rg -q '"normal_stage_ledger_scoped"[[:space:]]*:[[:space:]]*"true"' "$proof" \
    && rg -q '"focused_field_callback"[[:space:]]*:[[:space:]]*"true"' "$proof" \
    && rg -q '"process_key_event_callback"[[:space:]]*:[[:space:]]*"true"' "$proof" \
    && rg -q '"cursor_location_callback"[[:space:]]*:[[:space:]]*"true"' "$proof" \
    && rg -q '"pre_boundary_commit_absent"[[:space:]]*:[[:space:]]*"true"' "$proof" \
    && rg -q '"boundary_stage_ledger_scoped"[[:space:]]*:[[:space:]]*"true"' "$proof" \
    && rg -q '"boundary_stage_commit_count"[[:space:]]*:[[:space:]]*"1"' "$proof" \
    && rg -q '"normal_stage_commit"[[:space:]]*:[[:space:]]*"true"' "$proof" \
    && rg -q '"ibus_commit_operation"[[:space:]]*:[[:space:]]*"true"' "$proof" \
    && rg -q '"focused_entry_readback"[[:space:]]*:[[:space:]]*"true"' "$proof" \
    && rg -q '"ibus_commit_delivered"[[:space:]]*:[[:space:]]*"true"' "$proof" \
    && rg -q '"boundary_popup_action"[[:space:]]*:[[:space:]]*"hide-candidate"' "$proof" \
    && rg -q '"boundary_popup_reason"[[:space:]]*:[[:space:]]*"committed"' "$proof" \
    && rg -q '"candidate_intent_seen"[[:space:]]*:[[:space:]]*"true"' "$proof" \
    && rg -q '"native_ibus_candidate_published"[[:space:]]*:[[:space:]]*"true"' "$proof" \
    && rg -q '"native_popup_generation"[[:space:]]*:[[:space:]]*"[1-9][0-9]*"' "$proof" \
    && rg -q '"native_popup_record_ordinal"[[:space:]]*:[[:space:]]*"[1-9][0-9]*"' "$proof" \
    && rg -q '"native_popup_generation_current"[[:space:]]*:[[:space:]]*"true"' "$proof" \
    && rg -q '"native_popup_record_current_at_capture"[[:space:]]*:[[:space:]]*"true"' "$proof" \
    && rg -q '"native_popup_action"[[:space:]]*:[[:space:]]*"show-candidate"' "$proof" \
    && rg -q '"native_popup_has_cursor_rect"[[:space:]]*:[[:space:]]*"true"' "$proof" \
    && rg -q '"native_popup_expected_replacement"[[:space:]]*:[[:space:]]*"true"' "$proof" \
    && rg -q '"native_popup_hint_published"[[:space:]]*:[[:space:]]*"true"' "$proof" \
    && rg -q '"renderer"[[:space:]]*:[[:space:]]*"native-ibus-lookup-table"' "$proof" \
    && rg -q '"cursor_anchor"[[:space:]]*:[[:space:]]*"ibus-input-context"' "$proof" \
    && rg -q '"synthetic_overlay"[[:space:]]*:[[:space:]]*"false"' "$proof" \
    && rg -q '"screenshot"[[:space:]]*:[[:space:]]*"32-text-shortcuts-live-ibus-runtime-render\.png"' "$proof" \
    && rg -q '"screenshot_sha256"[[:space:]]*:[[:space:]]*"[0-9a-f]{64}"' "$proof" \
    && rg -q '"screenshot_capture_ack"[[:space:]]*:[[:space:]]*"true"' "$proof" \
    && rg -q '"native_candidate_popup_ready_claim"[[:space:]]*:[[:space:]]*"true"' "$proof" \
    && rg -q '"live_overlay_claim"[[:space:]]*:[[:space:]]*"true"' "$proof" \
    && rg -q '"runtime_ready_claim"[[:space:]]*:[[:space:]]*"true"' "$proof" \
    && rg -q '"core_readiness_flip"[[:space:]]*:[[:space:]]*"live"' "$proof" \
    || return 1

  [ "$(rg -c '"screenshot_sha256"[[:space:]]*:' "$proof")" = "1" ] || return 1
  screenshot="$(dirname "$proof")/32-text-shortcuts-live-ibus-runtime-render.png"
  recorded_sha="$(awk -F'"' '/"screenshot_sha256"/ { print $4; exit }' "$proof")"
  actual_sha="$(sha256_file "$screenshot")" || return 1
  [[ "$recorded_sha" =~ ^[0-9a-f]{64}$ ]] \
    && [ "$recorded_sha" = "$actual_sha" ] \
    && screenshot_file_is_valid_png "$screenshot"
}

keyboard_shortcuts_roundtrip_proof_passes() {
  local proof="$1"

  proof_json_passes "$proof" || return 1
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

  proof_json_passes "$proof" || return 1
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

  proof_json_passes "$proof" || return 1
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

  proof_json_passes "$proof" || return 1
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

  proof_json_passes "$proof" || return 1
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

  proof_json_passes "$proof" || return 1
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

  proof_json_passes "$proof" || return 1
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

  proof_json_passes "$proof" || return 1
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
if ! rg -q "Inter is the final shipped font stack" "$SHIP_DECL" || ! rg -q "no non-Inter brand font dependency" "$SHIP_DECL"; then
  fail "SHIP.md does not state the Inter-only typography boundary."
  exit 1
fi
if rg -qi --hidden --no-ignore-vcs --no-ignore \
  "OpenAI[ -]Sans|openai[ -]sans|openai-sans" \
  README.md ROADMAP.md GO-LIVE.md SHIP.md CONTRIBUTING.md CLA.md NOTICE TRADEMARKS.md AGENTS.md apps/site/src apps/site/public \
  --glob '!apps/site/.next/**' \
  --glob '!apps/site/node_modules/**' >/tmp/ship_openai_sans_hits 2>/dev/null; then
  fail "Unexpected unused external brand font references found in public docs:"
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
IMAGE_PROVENANCE_REF="$IMAGE"
ISO_PATH="not-found"
ISO_SHA="not-found"

expected_iso="os/iso/output/$ARCH/bootiso/goblins-os-$ARCH.iso"
ISO_MANIFEST="os/iso/output/$ARCH/manifest-goblins-os-$ARCH.json"
BIB_MANIFEST="os/iso/output/$ARCH/manifest-anaconda-iso.json"
if [ -f "$expected_iso" ]; then
  ISO_PATH="$expected_iso"
  ISO_SHA="$(sha256_file "$ISO_PATH")"
  if [ ! -f "$ISO_MANIFEST" ] \
    || ! grep -Fq '"candidate_commit": "'"$CANDIDATE_COMMIT"'"' "$ISO_MANIFEST"; then
    fail "ISO manifest must bind $ARCH media to candidate commit $CANDIDATE_COMMIT: $ISO_MANIFEST"
    exit 1
  fi
  IMAGE_PROVENANCE_REF="$(awk -F'"' '/"builder_source_image"/ { print $4; exit }' "$ISO_MANIFEST")"
  if ! image_ref_is_digest_pinned "$IMAGE_PROVENANCE_REF"; then
    fail "ISO manifest must bind the installer payload to an immutable registry digest: $ISO_MANIFEST"
    exit 1
  fi
  if [ -n "${GOBLINS_OS_IMAGE:-}" ] && [ "$IMAGE" != "$IMAGE_PROVENANCE_REF" ]; then
    fail "GOBLINS_OS_IMAGE must equal the exact digest-pinned image in $ISO_MANIFEST: $IMAGE_PROVENANCE_REF"
    exit 1
  fi
  IMAGE="$IMAGE_PROVENANCE_REF"
  ISO_CANDIDATE_STATUS="yes ($ISO_MANIFEST binds $CANDIDATE_COMMIT)"
  log "Latest installer ISO: $ISO_PATH"
  log "SHA256: $ISO_SHA"
  log "Candidate/source commit: $CANDIDATE_COMMIT"
  log "Image digest reference: $IMAGE_PROVENANCE_REF"
else
  warn "No $ARCH ISO found at $expected_iso (if available, verify on Linux host)"
fi

if [ "$LOGIC_ONLY" -eq 0 ] && runtime_image_exists "$CONTAINER_RUNTIME" "$IMAGE"; then
  log "Exact candidate image exists: $IMAGE"
else
  warn "Exact candidate image not checked/found: $IMAGE"
fi

SBOM_DIR="${RELEASE_EVIDENCE_DIR:-os/signoff-proofs/sbom/$ARCH}"
mkdir -p "$SBOM_DIR"

if ! release_evidence_complete "$SBOM_DIR"; then
  if [ "$LOGIC_ONLY" -eq 0 ] && runtime_image_exists "$CONTAINER_RUNTIME" "$IMAGE"; then
    log "Generating sealed release evidence from exact candidate image $IMAGE in $SBOM_DIR"
    evidence_generated=0
    generate_image_release_evidence "$CONTAINER_RUNTIME" "$IMAGE" "$SBOM_DIR" "$ARCH" \
      || evidence_generated=$?
  else
    log "Generating source-only release evidence in $SBOM_DIR"
    evidence_generated=0
    generate_source_release_evidence "$SBOM_DIR" "$ARCH" || evidence_generated=$?
  fi
  if [ "$evidence_generated" -ne 0 ]; then
    warn "Could not generate source release evidence; run target/release/goblins-os-verify --source-root . --release-evidence $SBOM_DIR --arch $ARCH --candidate-commit $CANDIDATE_COMMIT --image-ref $IMAGE_PROVENANCE_REF"
  fi
fi

if goblins_os_artifact_secret_scan "$REPO_ROOT"; then
  log "generated artifact/evidence secret scan passed"
else
  fail "Generated artifact/evidence secret scan found possible live secrets."
  exit 1
fi

if release_evidence_complete "$SBOM_DIR"; then
  RELEASE_EVIDENCE_STATUS="yes (candidate $CANDIDATE_COMMIT, manifest, diligence links, Cargo TSV, and RPM TSV present in $SBOM_DIR)"
else
  warn "Release evidence incomplete for $ARCH candidate $CANDIDATE_COMMIT; expected a matching release-evidence-manifest.json with diligence links plus cargo-lock-packages.tsv and rpm-packages.tsv in $SBOM_DIR"
fi

if [ -n "$SCREENSHOT_DIR" ]; then
  log "Checking required proof screenshots in: $SCREENSHOT_DIR"
  if ! CANONICAL_SCREENSHOT_DIR="$(
    python3 "$REPO_ROOT/os/hardware-gate/capture-harness/proof_validation.py" \
      --run-directory "$SCREENSHOT_DIR" "$REPO_ROOT" "$ARCH"
  )"; then
    fail "SCREENSHOT_DIR must be a canonical, in-repository, non-symlinked architecture/date directory."
    exit 1
  fi
  SCREENSHOT_DIR="$CANONICAL_SCREENSHOT_DIR"
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
  if ! semantic_screenshot_frames_are_distinct "$SCREENSHOT_DIR"; then
    fail "Screenshot proof reuses a central application crop for named login/Home or Studio semantic states."
    fail "Clock, top-bar, and pointer-only changes cannot satisfy release proof."
    exit 1
  fi
  manifest="$SCREENSHOT_DIR/proof-manifest.json"
  if ! python3 "$REPO_ROOT/os/hardware-gate/capture-harness/proof_validation.py" \
    --manifest "$manifest" "$ARCH" "$CANDIDATE_COMMIT" "$IMAGE_PROVENANCE_REF" \
    "$ISO_PATH" "$SCREENSHOT_DIR"; then
    fail "Screenshot proof manifest failed its exact typed schema and duplicate-key gate: $manifest"
    exit 1
  fi
  if ! screenshot_manifest_matches_iso "$manifest"; then
    fail "Screenshot proof manifest missing or incoherent for this architecture verification proof: $manifest"
    fail "Expected architecture=$ARCH, candidate_commit=$CANDIDATE_COMMIT, iso=$ISO_PATH, a 64-character iso_sha256, captured_at, screenshot_run_dir=$SCREENSHOT_DIR, firewall_live_toggle_proof=$FIREWALL_LIVE_TOGGLE_PROOF, text_shortcuts_session_enable_proof=$TEXT_SHORTCUTS_SESSION_ENABLE_PROOF, text_shortcuts_candidate_metadata_proof=$TEXT_SHORTCUTS_CANDIDATE_METADATA_PROOF, text_shortcuts_overlay_intent_proof=$TEXT_SHORTCUTS_OVERLAY_INTENT_PROOF, text_shortcuts_candidate_bubble_frame_proof=$TEXT_SHORTCUTS_CANDIDATE_BUBBLE_FRAME_PROOF, text_shortcuts_candidate_bubble_layout_proof=$TEXT_SHORTCUTS_CANDIDATE_BUBBLE_LAYOUT_PROOF, text_shortcuts_candidate_bubble_render_intent_proof=$TEXT_SHORTCUTS_CANDIDATE_BUBBLE_RENDER_INTENT_PROOF, text_shortcuts_candidate_bubble_render_proof=$TEXT_SHORTCUTS_CANDIDATE_BUBBLE_RENDER_PROOF, text_shortcuts_live_ibus_runtime_render_proof=$TEXT_SHORTCUTS_LIVE_IBUS_RUNTIME_RENDER_PROOF, its exact 64-character screenshot SHA256 matching the live proof and decoded PNG, keyboard_shortcuts_roundtrip_proof=$KEYBOARD_SHORTCUTS_ROUNDTRIP_PROOF, input_sources_roundtrip_proof=$INPUT_SOURCES_ROUNDTRIP_PROOF, multi_display_apply_proof=$MULTI_DISPLAY_APPLY_PROOF, focus_arm_roundtrip_proof=$FOCUS_ARM_ROUNDTRIP_PROOF, app_privacy_revoke_proof=$APP_PRIVACY_REVOKE_PROOF, preview_open_render_proof=$PREVIEW_OPEN_RENDER_PROOF, audio_output_proof=$AUDIO_OUTPUT_PROOF, and runtime_build_proof=$RUNTIME_BUILD_PROOF."
    exit 1
  fi
  EVIDENCE_BUNDLE_PATH="$SCREENSHOT_DIR/evidence-bundle.json"
  if EVIDENCE_BUNDLE_SHA256="$(evidence_bundle_passes "$SCREENSHOT_DIR")"; then
    EVIDENCE_BUNDLE_STATUS="yes (canonical SHA256/size/dimension seal recomputed for all 32 PNGs and every required proof/verification JSON)"
    log "Evidence bundle integrity passed: $EVIDENCE_BUNDLE_SHA256"
  else
    fail "Canonical evidence bundle is missing, unsafe, non-uniform, or no longer matches this screenshot run: $EVIDENCE_BUNDLE_PATH"
    exit 1
  fi
  if [ "$ARCH" = "aarch64" ]; then
    LOCAL_DISPLAY_ATTESTATION_PATH="$SCREENSHOT_DIR/aarch64-local-display-attestation.json"
    if ATTESTATION_FIELDS="$(local_display_attestation_fields "$SCREENSHOT_DIR")"; then
      read -r LOCAL_DISPLAY_ATTESTATION_RUN LOCAL_DISPLAY_ATTESTATION_RUN_ATTEMPT LOCAL_DISPLAY_ATTESTATION_ARTIFACT <<<"$ATTESTATION_FIELDS"
      if github_actions_run_is_successful \
        "$LOCAL_DISPLAY_ATTESTATION_RUN" \
        "$CANDIDATE_COMMIT" \
        "$LOCAL_DISPLAY_ATTESTATION_RUN_ATTEMPT" \
        ".github/workflows/aarch64-local-display-attestation.yml" \
        && github_actions_artifact_file_matches \
          "$LOCAL_DISPLAY_ATTESTATION_RUN" \
          "$LOCAL_DISPLAY_ATTESTATION_ARTIFACT" \
          "$EVIDENCE_BUNDLE_PATH" \
          "evidence-bundle.json" \
        && github_actions_artifact_file_matches \
          "$LOCAL_DISPLAY_ATTESTATION_RUN" \
          "$LOCAL_DISPLAY_ATTESTATION_ARTIFACT" \
          "$LOCAL_DISPLAY_ATTESTATION_PATH" \
          "aarch64-local-display-attestation.json" \
        && local_display_attestation_signature_passes "$EVIDENCE_BUNDLE_PATH"; then
        LOCAL_DISPLAY_ATTESTATION_STATUS="yes (successful exact-candidate GitHub run; byte-identical seal/record artifact; signed exact seal subject and signer/source digest verified)"
      else
        LOCAL_DISPLAY_ATTESTATION_STATUS="invalid (GitHub run, artifact bytes, or signed seal provenance did not verify)"
        warn "The aarch64 local-display attestation did not pass its exact GitHub run, uploaded-byte, and signed-subject checks."
      fi
    else
      LOCAL_DISPLAY_ATTESTATION_STATUS="missing (dispatch aarch64-local-display-attestation.yml and hydrate its run-bound record)"
      warn "The local aarch64/HVF seal still needs its GitHub-hosted signed attestation record before a complete signoff row can be written."
    fi
  fi
  SCREENSHOT_ISO_SHA="$(screenshot_manifest_iso_sha "$manifest" | tr '[:upper:]' '[:lower:]')"
  log "Screenshot verification ISO SHA256: ${SCREENSHOT_ISO_SHA:-missing}"
  if [ "$ISO_SHA" = "not-found" ]; then
    fail "The associated verification ISO must be present so its SHA256 can be bound to this signoff row."
    exit 1
  fi
  log "Associated signoff-row verification ISO SHA256: $ISO_SHA"
  if [ -z "$SCREENSHOT_ISO_SHA" ] || [ "$SCREENSHOT_ISO_SHA" != "$ISO_SHA" ]; then
    fail "Screenshot proof-manifest ISO SHA256 does not equal the associated signoff-row verification ISO SHA256."
    fail "manifest=${SCREENSHOT_ISO_SHA:-missing} signoff=$ISO_SHA"
    exit 1
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
    fail "Expected 32-text-shortcuts-live-ibus-runtime-render.png plus QMP-keyboard-driven native IBus lookup-table publication, a host-acknowledged chronological popup record current at capture, zero pre-boundary commits, exactly one process-key-event boundary commit, a committed popup hide transition, focused entry readback, private-storage roundtrip, password suppression, and core_readiness_flip=live."
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
  TEXT_SHORTCUTS_KEYSTROKE_STATUS="yes (covered by $TEXT_SHORTCUTS_LIVE_IBUS_RUNTIME_RENDER_PROOF + 32-text-shortcuts-live-ibus-runtime-render.png: normal expansion, pass-through, password suppression, zero pre-boundary commits, one boundary commit, focused entry readback, and a host-acknowledged chronological native popup record)"
  TEXT_SHORTCUTS_CANDIDATE_STATUS="yes ($TEXT_SHORTCUTS_CANDIDATE_METADATA_PROOF: candidate metadata present; rendered bubble still gated false)"
  TEXT_SHORTCUTS_OVERLAY_INTENT_STATUS="yes ($TEXT_SHORTCUTS_OVERLAY_INTENT_PROOF: adapter show/hide overlay intents present; live overlay still gated false)"
  TEXT_SHORTCUTS_CANDIDATE_BUBBLE_FRAME_STATUS="yes ($TEXT_SHORTCUTS_CANDIDATE_BUBBLE_FRAME_PROOF: adapter accept-bubble frames present; rendered bubble still gated false)"
  TEXT_SHORTCUTS_CANDIDATE_BUBBLE_LAYOUT_STATUS="yes ($TEXT_SHORTCUTS_CANDIDATE_BUBBLE_LAYOUT_PROOF: adapter accept-bubble layouts present; rendered bubble still gated false)"
  TEXT_SHORTCUTS_CANDIDATE_BUBBLE_RENDER_INTENT_STATUS="yes ($TEXT_SHORTCUTS_CANDIDATE_BUBBLE_RENDER_INTENT_PROOF: adapter render intents present; rendered bubble still gated false)"
  TEXT_SHORTCUTS_CANDIDATE_BUBBLE_RENDER_STATUS="yes ($TEXT_SHORTCUTS_CANDIDATE_BUBBLE_RENDER_PROOF + 31-text-shortcuts-candidate-bubble-render.png: render-intent-backed candidate proof surface rendered; live overlay still gated false)"
  TEXT_SHORTCUTS_LIVE_IBUS_RUNTIME_RENDER_STATUS="yes ($TEXT_SHORTCUTS_LIVE_IBUS_RUNTIME_RENDER_PROOF + 32-text-shortcuts-live-ibus-runtime-render.png: chronological native IBus lookup popup current at capture, cursor anchor, zero pre-boundary commits, one process-key-event boundary commit, committed popup hide, focused readback, secure storage, password suppression, and host capture acknowledgement proved; core readiness flip live)"
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

NATIVE_PACKAGING_GATE_ACCEPTED=0
if [ "$LOGIC_ONLY" -eq 1 ] \
  && { [ -n "$NATIVE_PACKAGING_GATE_PROOF" ] \
    || [ -n "$NATIVE_PACKAGING_GATE_RUN_URL" ] \
    || [ -n "$NATIVE_PACKAGING_GATE_RUN_ATTEMPT" ]; }; then
  for native_gate_artifact in \
    "$ISO_MANIFEST" \
    "$BIB_MANIFEST" \
    "$SBOM_DIR/release-evidence-manifest.json"; do
    [ -s "$native_gate_artifact" ] || {
      fail "Native packaging gate artifact is missing: $native_gate_artifact"
      exit 1
    }
  done
  NATIVE_ISO_MANIFEST_SHA="$(sha256_file "$ISO_MANIFEST")"
  NATIVE_BIB_MANIFEST_SHA="$(sha256_file "$BIB_MANIFEST")"
  NATIVE_EVIDENCE_MANIFEST_SHA="$(sha256_file "$SBOM_DIR/release-evidence-manifest.json")"
  NATIVE_PACKAGING_GATE_RUN_DATE="${SCREENSHOT_DIR%/}"
  NATIVE_PACKAGING_GATE_RUN_DATE="${NATIVE_PACKAGING_GATE_RUN_DATE##*/}"
  NATIVE_PACKAGING_GATE_ARTIFACT="goblins-os-aarch64-native-packaging-gate-$CANDIDATE_COMMIT-$NATIVE_PACKAGING_GATE_RUN_DATE-attempt-$NATIVE_PACKAGING_GATE_RUN_ATTEMPT"
  if native_packaging_gate_proof_passes \
    "$NATIVE_PACKAGING_GATE_PROOF" \
    "$NATIVE_PACKAGING_GATE_RUN_URL" \
    "$NATIVE_PACKAGING_GATE_RUN_ATTEMPT" \
    "$ARCH" \
    "$CANDIDATE_COMMIT" \
    "$IMAGE_PROVENANCE_REF" \
    "$ISO_SHA" \
    "$NATIVE_ISO_MANIFEST_SHA" \
    "$NATIVE_BIB_MANIFEST_SHA" \
    "$NATIVE_EVIDENCE_MANIFEST_SHA" \
    && github_actions_run_is_successful \
      "$NATIVE_PACKAGING_GATE_RUN_URL" \
      "$CANDIDATE_COMMIT" \
      "$NATIVE_PACKAGING_GATE_RUN_ATTEMPT" \
      ".github/workflows/aarch64-verification-iso.yml" \
    && github_actions_artifact_file_matches \
      "$NATIVE_PACKAGING_GATE_RUN_URL" \
      "$NATIVE_PACKAGING_GATE_ARTIFACT" \
      "$NATIVE_PACKAGING_GATE_PROOF" \
      "native-packaging-gate.json"; then
    NATIVE_PACKAGING_GATE_ACCEPTED=1
    NATIVE_PACKAGING_GATE_STATUS="yes ($NATIVE_PACKAGING_GATE_PROOF; $NATIVE_PACKAGING_GATE_RUN_URL; attempt $NATIVE_PACKAGING_GATE_RUN_ATTEMPT; byte-identical artifact $NATIVE_PACKAGING_GATE_ARTIFACT)"
    log "Native Linux packaging gate proof passed for $ARCH candidate $CANDIDATE_COMMIT."
  else
    fail "Native packaging gate proof is invalid, its workflow run did not succeed, or its exact uploaded bytes do not bind $ARCH candidate $CANDIDATE_COMMIT and image $IMAGE_PROVENANCE_REF."
    exit 1
  fi
fi

if [ "$LOGIC_ONLY" -eq 0 ] && runtime_image_exists "$CONTAINER_RUNTIME" "$IMAGE"; then
  log "Running installed-root verifier for $IMAGE"
  if run_verify "$CONTAINER_RUNTIME" "$IMAGE" 2>"$VERIFY_ERR" | tee "$VERIFY_LOG"; then
    if grep -Eq '^goblins_os_verify_result total=[0-9]+ blocked=0$' "$VERIFY_LOG"; then
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
elif [ "$NATIVE_PACKAGING_GATE_ACCEPTED" -eq 1 ]; then
  VERIFY_STATUS="pass"
  log "Installed-root verifier accepted from the exact native Linux packaging gate proof."
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
  if image_ref_is_digest_pinned "$IMAGE"; then
    printf 'FROM %s AS goblins-os\n' "$IMAGE" > "$SELFTEST_DOCKERFILE"
    cat os/bootc/selftest.suffix.Dockerfile >> "$SELFTEST_DOCKERFILE"
  else
    cat os/bootc/Containerfile os/bootc/selftest.suffix.Dockerfile > "$SELFTEST_DOCKERFILE"
  fi
  if [ "$LOGIC_ONLY" -eq 0 ]; then
    if run_selftest "$CONTAINER_RUNTIME" "$SELFTEST_DOCKERFILE" | tee "$SELFTEST_LOG"; then
      log "self-test build succeeded"
      SELFTEST_STATUS="pass"
    else
      SELFTEST_STATUS="fail"
      fail "self-test container build failed."
      exit 1
    fi
  elif [ "$NATIVE_PACKAGING_GATE_ACCEPTED" -eq 1 ]; then
    SELFTEST_STATUS="pass"
    log "Install and services self-test accepted from the exact native Linux packaging gate proof."
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
  && [[ "$EVIDENCE_BUNDLE_STATUS" == yes* ]] \
  && { [ "$ARCH" != "aarch64" ] || [[ "$LOCAL_DISPLAY_ATTESTATION_STATUS" == yes* ]]; } \
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
  && [[ "$ISO_CANDIDATE_STATUS" == yes* ]] \
  && image_ref_is_digest_pinned "$IMAGE_PROVENANCE_REF" \
	  && [ "$SCREENSHOT_ISO_SHA" = "$ISO_SHA" ] \
	  && proof_field_is_real "$RUNTIME_ENGINE_MODE" \
  && proof_field_is_real "$RUNTIME_ENGINE_SOURCE" \
  && built_artifact_reference_is_real "$BUILT_ARTIFACT_PATH_URL"; then
  PROJECT_COMPLETION_STATUS="complete"
fi

if [ "$LOGIC_ONLY" -eq 1 ]; then
  if [ "$PROJECT_COMPLETION_STATUS" = "complete" ]; then
    log "Sign-off helper completed with exact native Linux packaging proof and local display-backed evidence."
  else
    log "Sign-off helper completed in static-only mode."
    log "Remaining proof is reflected by the incomplete row; native Linux verifier/self-test may be supplied only through a matching native packaging gate."
  fi
else
  log "Sign-off helper completed. External steps remaining: hardware/VM install flow, session unlock, shell/settings launch, gaming proof, Build Studio real engine turn, screenshot proof in os/screenshots/hardware-gate/$ARCH/<date>/, and filled status in $OUT"
fi
SIGNOFF_ROW_TEMP="$(mktemp "${TMPDIR:-/tmp}/goblins-os-signoff-row.XXXXXX")"
cat > "$SIGNOFF_ROW_TEMP" <<EOF2

## Manual Gate Run: $STAMP (script assisted)
- Runner: ${SIGNOFF_RUNNER_VALUE}
- CI workflow references: verified in-repo at .github/workflows/build.yml
- Architecture: ${ARCH}
- Candidate/source commit: ${CANDIDATE_COMMIT}
- Image digest reference: ${IMAGE_PROVENANCE_REF}
- Capture workflow run: ${CAPTURE_WORKFLOW_RUN_URL:-not provided}
- Capture workflow run attempt: ${CAPTURE_WORKFLOW_RUN_ATTEMPT}
- Native packaging gate proof: ${NATIVE_PACKAGING_GATE_PROOF:-not provided}
- Native packaging gate run: ${NATIVE_PACKAGING_GATE_RUN_URL:-not provided}
- Native packaging gate run attempt: ${NATIVE_PACKAGING_GATE_RUN_ATTEMPT:-not provided}
- Native packaging gate checked: ${NATIVE_PACKAGING_GATE_STATUS}
- CI run IDs/URLs:
  - rust: ${CI_RUST_URL:-not provided}
  - image: ${CI_IMAGE_URL:-not provided}
  - installer-iso: ${CI_INSTALLER_ISO_URL:-not provided}
- Image: ${IMAGE}
- ISO: ${ISO_PATH}
- ISO SHA256: ${ISO_SHA}
- Screenshot proof ISO SHA256: ${SCREENSHOT_ISO_SHA}
- Evidence bundle: ${EVIDENCE_BUNDLE_PATH}
- Evidence bundle SHA256: ${EVIDENCE_BUNDLE_SHA256}
- Evidence bundle integrity checked: ${EVIDENCE_BUNDLE_STATUS}
- Local display attestation: ${LOCAL_DISPLAY_ATTESTATION_PATH}
- Local display attestation run: ${LOCAL_DISPLAY_ATTESTATION_RUN}
- Local display attestation run attempt: ${LOCAL_DISPLAY_ATTESTATION_RUN_ATTEMPT}
- Local display attestation artifact: ${LOCAL_DISPLAY_ATTESTATION_ARTIFACT}
- Local display attestation checked: ${LOCAL_DISPLAY_ATTESTATION_STATUS}
- ISO candidate binding checked: ${ISO_CANDIDATE_STATUS}
- Rootfs verify command: \
  ${CONTAINER_RUNTIME:-docker} run --rm ${IMAGE} /usr/libexec/goblins-os/goblins-os-verify --installed-root /
- Verify result (blocked=0): ${VERIFY_STATUS}
- Self-test image: ${IMAGE}
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

if [ "$REQUIRE_COMPLETE" = "1" ] && [ "$PROJECT_COMPLETION_STATUS" != "complete" ]; then
  fail "This proof route requires a complete signoff row; recorded status was $PROJECT_COMPLETION_STATUS."
  rm -f "$SIGNOFF_ROW_TEMP"
  exit 1
fi

if [ -n "$SIGNOFF_ROW_OUTPUT" ]; then
  EXPECTED_SIGNOFF_ROW_OUTPUT="${SCREENSHOT_DIR%/}/signoff-row.md"
  if [ -z "$SCREENSHOT_DIR" ] || [ "$SIGNOFF_ROW_OUTPUT" != "$EXPECTED_SIGNOFF_ROW_OUTPUT" ]; then
    fail "SIGNOFF_ROW_OUTPUT must be exactly the current screenshot directory's signoff-row.md path."
    rm -f "$SIGNOFF_ROW_TEMP"
    exit 2
  fi
fi

cat "$SIGNOFF_ROW_TEMP" >> "$OUT"
if [ -n "$SIGNOFF_ROW_OUTPUT" ]; then
  mkdir -p "$(dirname "$SIGNOFF_ROW_OUTPUT")"
  cp "$SIGNOFF_ROW_TEMP" "$SIGNOFF_ROW_OUTPUT"
  log "Wrote architecture-scoped signoff row to $SIGNOFF_ROW_OUTPUT"
fi
rm -f "$SIGNOFF_ROW_TEMP"

log "Appended signoff entry to $OUT"
