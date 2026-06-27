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
SCREENSHOT_REQUIRED=(
  "${BASE_SCREENSHOTS[@]}"
  "${GAMING_SCREENSHOTS[@]}"
  "${INSTALL_STORAGE_SCREENSHOTS[@]}"
)
FIREWALL_LIVE_TOGGLE_PROOF="firewall-live-toggle-proof.json"
GAMING_SCREENSHOT_STATUS="not checked"
INSTALL_STORAGE_STATUS="not checked"
RELEASE_EVIDENCE_STATUS="not checked"
MOTION_INTERACTIONS_STATUS="not checked"
FIREWALL_TOGGLE_STATUS="not checked"
RUNTIME_ENGINE_MODE="${RUNTIME_ENGINE_MODE:-}"
RUNTIME_ENGINE_SOURCE="${RUNTIME_ENGINE_SOURCE:-}"
RUNTIME_ENGINE_CONFIG="${RUNTIME_ENGINE_CONFIG:-}"
BUILT_ARTIFACT_PATH_URL="${BUILT_ARTIFACT_PATH_URL:-}"

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
      docker image inspect "$image" >/dev/null 2>&1
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
      DOCKER_BUILDKIT=1 docker build -f "$dockerfile" --target selftest -t goblins-os:selftest .
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

  [ "$ISO_PATH" != "not-found" ] || return 1
  [ "$ISO_SHA" != "not-found" ] || return 1
  [ -s "$manifest" ] || return 1
  rg -q '"architecture"[[:space:]]*:[[:space:]]*"'"$ARCH"'"' "$manifest" \
    && rg -q '"iso"[[:space:]]*:[[:space:]]*"'"$ISO_PATH"'"' "$manifest" \
    && rg -q '"iso_sha256"[[:space:]]*:[[:space:]]*"'"$ISO_SHA"'"' "$manifest" \
    && rg -q '"captured_at"[[:space:]]*:[[:space:]]*"[^"]+"' "$manifest" \
    && rg -q '"screenshot_run_dir"[[:space:]]*:[[:space:]]*"'"$SCREENSHOT_DIR"'"' "$manifest" \
    && rg -q '"firewall_live_toggle_proof"[[:space:]]*:[[:space:]]*"'"$FIREWALL_LIVE_TOGGLE_PROOF"'"' "$manifest"
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
require_fixed "self-test target missing in workflow" .github/workflows/build.yml "goblins-os:selftest"
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
  if ! screenshot_manifest_matches_iso "$SCREENSHOT_DIR/proof-manifest.json"; then
    fail "Screenshot proof manifest missing or not tied to this architecture ISO: $SCREENSHOT_DIR/proof-manifest.json"
    fail "Expected architecture=$ARCH, iso=$ISO_PATH, iso_sha256=$ISO_SHA, captured_at, screenshot_run_dir=$SCREENSHOT_DIR, and firewall_live_toggle_proof=$FIREWALL_LIVE_TOGGLE_PROOF."
    exit 1
  fi
  if ! firewall_live_toggle_proof_passes "$SCREENSHOT_DIR/$FIREWALL_LIVE_TOGGLE_PROOF"; then
    fail "Firewall live toggle proof missing or failed: $SCREENSHOT_DIR/$FIREWALL_LIVE_TOGGLE_PROOF"
    fail "Expected live /v1/firewall/enabled disable=200/inactive and enable=200/active through the Goblins OS firewall bridge."
    exit 1
  fi
  log "All required screenshot proof PNGs and proof manifest passed."
  log "Firewall live toggle proof passed."
  GAMING_SCREENSHOT_STATUS="yes (screenshots ${GAMING_SCREENSHOTS[*]} present)"
  INSTALL_STORAGE_STATUS="yes (screenshots ${INSTALL_STORAGE_SCREENSHOTS[*]} present)"
  MOTION_INTERACTIONS_STATUS="yes (light/dark screenshots present in proof dir)"
  FIREWALL_TOGGLE_STATUS="yes ($FIREWALL_LIVE_TOGGLE_PROOF: disable=200/inactive, enable=200/active)"
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
- Runner: 
- CI workflow references: verified in-repo at .github/workflows/build.yml
- Architecture: ${ARCH}
- CI run IDs/URLs:
  - rust: 
  - image: 
  - installer-iso: 
- Image: ${IMAGE}
- ISO: ${ISO_PATH}
- ISO SHA256: ${ISO_SHA}
- Rootfs verify command: \
  ${CONTAINER_RUNTIME:-docker} run --rm ${IMAGE} /usr/libexec/goblins-os/goblins-os-verify --installed-root /
- Verify result (blocked=0): ${VERIFY_STATUS}
- Self-test command: DOCKER_BUILDKIT=1 ${CONTAINER_RUNTIME:-docker} build -f ${SELFTEST_DOCKERFILE} --target selftest -t goblins-os:selftest .
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
- Gaming readiness checked: ${GAMING_SCREENSHOT_STATUS}
- Install storage/bootloader/dual-boot checked: ${INSTALL_STORAGE_STATUS}
- Current project completion status: ${PROJECT_COMPLETION_STATUS}
EOF2

log "Appended scaffold entry to $OUT"
