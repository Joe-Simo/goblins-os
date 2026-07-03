#!/usr/bin/env bash
set -euo pipefail

REPO_ROOT="${REPO_ROOT:-$(pwd)}"
cd "$REPO_ROOT"
. "$REPO_ROOT/os/hardware-gate/secret-scan.sh"
. "$REPO_ROOT/os/hardware-gate/rpm-sbom-arch.sh"

log() { echo "[goblin-signoff] $*"; }
warn() { echo "[goblin-signoff][warn] $*" >&2; }

DATE="${DATE:-$(date -u +%Y-%m-%d)}"
normalize_arch() {
  case "$1" in
    aarch64|arm64) echo "aarch64" ;;
    x86_64|amd64) echo "x86_64" ;;
    *) echo "unsupported" ;;
  esac
}

usage() {
  cat <<'EOF'
Usage:
  REPO_ROOT=/path os/hardware-gate/run-external-gate.sh

Optional env:
  GOBLINS_OS_ARCH=aarch64|x86_64
  DATE=YYYY-MM-DD            Screenshot directory date segment (default UTC date)
  SCREENSHOT_DIR=path         Full screenshot directory path
  SCREENSHOT_RUN_DIR=path     Alias for SCREENSHOT_DIR
  IMAGE_NAME=tag              Image tag for helper (default localhost/goblins-os:<arch>)
  RELEASE_EVIDENCE_DIR=path   SBOM/provenance output dir (default os/signoff-proofs/sbom/<arch>)
  GENERATE_RELEASE_EVIDENCE=1 Generate Cargo/RPM release evidence before VM launch (default 1)
  RUN_QEMU=1|0                Launch display-backed VM (default 1; set 0 for artifact/SBOM build only)
  RUN_CLOSEOFF=1|0            Run close-signoff (default 0; requires screenshots)
  PREFLIGHT_ONLY=1|0          Validate native runner prerequisites and exit without building (default 0)
  GOBLINS_OS_CONTAINER_RUNTIME=docker
                              Host container runtime for image, ISO, and SBOM
                              steps (default docker)
  GOBLINS_OS_ALLOW_EMULATED_DOCKER=1
                              Allow RUN_QEMU=0 Docker artifact testing when
                              the host architecture differs from
                              GOBLINS_OS_ARCH. This can never satisfy release
                              proof; display-backed signoff still requires a
                              native Linux/KVM runner for the target arch.
  GOBLINS_OS_BIB_SOURCE_IMAGE=registry.example/org/goblins-os:<arch>
                              Real pullable bootc image ref used by
                              bootc-image-builder for shippable media. Required
                              for RUN_QEMU=1 so the installed system tracks a
                              release registry ref instead of a Docker-local
                              test registry.
  GOBLINS_OS_SHIPPABLE_RELEASE=1|0
                              Require a nonlocal installer payload source
                              (default 1 for RUN_QEMU=1, 0 for RUN_QEMU=0)
  QCOW2_PATH=/tmp/goblins-os-<arch>.qcow2
  VM_MEMORY=8192
  VM_CPU=4
  QEMU_ACCEL=kvm              Required native VM acceleration for display-backed proof
  MIN_HOST_FREE_GB=120        Minimum free space required on repo and VM scratch filesystems
  CONTAINER_RUNTIME_HEALTH_TIMEOUT_SECS=20
                              Seconds allowed for docker info before failing fast
  AARCH64_UEFI_CODE=path      aarch64 UEFI pflash code image
  AARCH64_UEFI_VARS=path      writable aarch64 UEFI variable store for this VM
  AARCH64_UEFI_VARS_TEMPLATE=path
                              template copied to AARCH64_UEFI_VARS when needed

Preflight:
  PREFLIGHT_ONLY=1 GOBLINS_OS_ARCH=<arch> REPO_ROOT=/path os/hardware-gate/run-external-gate.sh

Preflight validates the native architecture, container runtime health, free
space, and, when RUN_QEMU=1, the native Linux host, QEMU/KVM requirements, and
aarch64 UEFI paths when applicable. It does not build the image, generate ISOs,
create SBOM proof, launch QEMU, or satisfy shipping evidence by itself.
EOF
}

if [[ "${1:-}" == "-h" || "${1:-}" == "--help" ]]; then
  usage
  exit 0
fi

ARCH="$(normalize_arch "${GOBLINS_OS_ARCH:-$(uname -m)}")"
if [[ "$ARCH" == "unsupported" ]]; then
  warn "Unsupported architecture '${GOBLINS_OS_ARCH:-$(uname -m)}'; expected aarch64 or x86_64."
  exit 1
fi
HOST_OS="$(uname -s)"
HOST_ARCH="$(normalize_arch "$(uname -m)")"
if [[ "$HOST_ARCH" == "unsupported" ]]; then
  warn "Unsupported host architecture '$(uname -m)'; expected native aarch64 or x86_64."
  exit 1
fi
RUN_QEMU="${RUN_QEMU:-1}"
CONTAINER_RUNTIME="${GOBLINS_OS_CONTAINER_RUNTIME:-docker}"
if [[ "$CONTAINER_RUNTIME" != "docker" ]]; then
  warn "GOBLINS_OS_CONTAINER_RUNTIME must be docker; got '$CONTAINER_RUNTIME'."
  exit 1
fi
ALLOW_EMULATED_DOCKER="${GOBLINS_OS_ALLOW_EMULATED_DOCKER:-0}"
if [[ "$HOST_ARCH" != "$ARCH" ]]; then
  if [[ "$RUN_QEMU" == "0" && "$CONTAINER_RUNTIME" == "docker" && "$ALLOW_EMULATED_DOCKER" == "1" ]]; then
    warn "Requested $ARCH artifact-only Docker test on $HOST_ARCH host."
    warn "This is not release proof; Goblins OS release media and display proof must still be produced on a native $ARCH Linux runner."
  else
    warn "Requested $ARCH gate on $HOST_ARCH host."
    warn "Goblins OS release media and display proof must be produced on a native $ARCH Linux runner."
    warn "For non-release Docker artifact testing only, set RUN_QEMU=0 and GOBLINS_OS_ALLOW_EMULATED_DOCKER=1."
    exit 1
  fi
fi
SCREENSHOT_DIR="${SCREENSHOT_DIR:-${SCREENSHOT_RUN_DIR:-os/screenshots/hardware-gate/$ARCH/$DATE}}"
ISO_DIR="os/iso/output/$ARCH/bootiso"
IMAGE_NAME="${IMAGE_NAME:-localhost/goblins-os:$ARCH}"
RELEASE_EVIDENCE_DIR="${RELEASE_EVIDENCE_DIR:-os/signoff-proofs/sbom/$ARCH}"
GENERATE_RELEASE_EVIDENCE="${GENERATE_RELEASE_EVIDENCE:-1}"
RUN_CLOSEOFF="${RUN_CLOSEOFF:-0}"
PREFLIGHT_ONLY="${PREFLIGHT_ONLY:-0}"
BIB_SOURCE_IMAGE="${GOBLINS_OS_BIB_SOURCE_IMAGE:-}"
SHIPPABLE_RELEASE="${GOBLINS_OS_SHIPPABLE_RELEASE:-}"
QCOW2_PATH="${QCOW2_PATH:-/tmp/goblins-os-$ARCH.qcow2}"
VM_MEMORY="${VM_MEMORY:-8192}"
VM_CPU="${VM_CPU:-4}"
QEMU_ACCEL="${QEMU_ACCEL:-kvm}"
MIN_HOST_FREE_GB="${MIN_HOST_FREE_GB:-120}"
CONTAINER_RUNTIME_HEALTH_TIMEOUT_SECS="${CONTAINER_RUNTIME_HEALTH_TIMEOUT_SECS:-20}"
CONTAINER_CMD=()

require_bool() {
  local name="$1"
  local value="$2"

  if [[ "$value" != "0" && "$value" != "1" ]]; then
    warn "$name must be 0 or 1; got '$value'"
    exit 1
  fi
}

require_file() {
  local label="$1"
  local path="$2"

  if [[ ! -f "$path" ]]; then
    warn "$label missing: $path"
    exit 1
  fi
}

require_file_contains() {
  local label="$1"
  local path="$2"
  local pattern="$3"

  require_file "$label" "$path"
  if ! rg -q --fixed-strings "$pattern" "$path"; then
    warn "$label does not contain expected text '$pattern': $path"
    exit 1
  fi
}

verify_sha256_file() {
  local sha_path="$1"
  local expected artifact actual

  if command -v sha256sum >/dev/null 2>&1; then
    sha256sum -c "$sha_path" >/dev/null
    return
  fi
  if command -v shasum >/dev/null 2>&1; then
    read -r expected artifact < "$sha_path"
    [[ -n "$expected" && -n "$artifact" && -f "$artifact" ]] || return 1
    actual="$(shasum -a 256 "$artifact" | awk '{print $1}')"
    [[ "$actual" == "$expected" ]]
    return
  fi
  return 1
}

sha256_of_file() {
  if command -v sha256sum >/dev/null 2>&1; then
    sha256sum "$1" | awk '{print $1}'
  else
    shasum -a 256 "$1" | awk '{print $1}'
  fi
}

find_first_existing() {
  local path

  for path in "$@"; do
    if [[ -f "$path" ]]; then
      printf '%s\n' "$path"
      return 0
    fi
  done
  return 1
}

free_gb_for_path() {
  local path="$1"
  local existing="$path"

  while [[ ! -e "$existing" && "$existing" != "/" ]]; do
    existing="$(dirname "$existing")"
  done
  df -Pk "$existing" | awk 'NR == 2 { printf "%d\n", int($4 / 1048576) }'
}

require_min_free_gb() {
  local label="$1"
  local failure_prefix="$2"
  local path="$3"
  local min_gb="$4"
  local free_gb

  free_gb="$(free_gb_for_path "$path")"
  if [[ -z "$free_gb" || "$free_gb" -lt "$min_gb" ]]; then
    warn "${failure_prefix} ${min_gb} GiB free before building release media; ${free_gb:-unknown} GiB free at $path."
    exit 1
  fi
  log "$label free space: ${free_gb} GiB at $path"
}

run_with_timeout() {
  local seconds="$1"
  shift

  if command -v timeout >/dev/null 2>&1; then
    timeout "$seconds" "$@"
  else
    "$@"
  fi
}

configure_container_cmd() {
  case "$CONTAINER_RUNTIME" in
    docker)
      CONTAINER_CMD=(docker)
      ;;
    *)
      warn "GOBLINS_OS_CONTAINER_RUNTIME must be docker; got '$CONTAINER_RUNTIME'."
      exit 1
      ;;
  esac
}

prepare_container_runtime() {
  log "Checking $CONTAINER_RUNTIME health before image build"
  if ! run_with_timeout "$CONTAINER_RUNTIME_HEALTH_TIMEOUT_SECS" "${CONTAINER_CMD[@]}" info >/dev/null; then
    warn "$CONTAINER_RUNTIME did not answer within ${CONTAINER_RUNTIME_HEALTH_TIMEOUT_SECS}s. Restart the container runtime or free host resources before running release proof."
    exit 1
  fi
  log "Container runtime command: ${CONTAINER_CMD[*]}"
}

prepare_native_qemu_acceleration() {
  if [[ "$QEMU_ACCEL" != "kvm" ]]; then
    warn "QEMU_ACCEL must be kvm for native display-backed release proof; got '$QEMU_ACCEL'."
    exit 1
  fi
  if [[ ! -e /dev/kvm || ! -r /dev/kvm || ! -w /dev/kvm ]]; then
    warn "Native display-backed proof requires readable/writable /dev/kvm on the $ARCH Linux runner."
    exit 1
  fi
}

prepare_aarch64_uefi() {
  local template

  if [[ -z "${AARCH64_UEFI_CODE:-}" ]]; then
    AARCH64_UEFI_CODE="$(find_first_existing \
      /usr/share/edk2/aarch64/QEMU_EFI-pflash.raw \
      /usr/share/edk2/aarch64/QEMU_EFI.fd \
      /usr/share/AAVMF/AAVMF_CODE.fd \
      /usr/share/AAVMF/AAVMF_CODE.ms.fd \
      /usr/share/qemu-efi-aarch64/QEMU_EFI.fd \
      /usr/share/edk2-armvirt/aarch64/QEMU_EFI.fd || true)"
  fi
  if [[ -z "$AARCH64_UEFI_CODE" || ! -f "$AARCH64_UEFI_CODE" ]]; then
    warn "aarch64 UEFI pflash code image missing. Set AARCH64_UEFI_CODE to a real EDK2/AAVMF firmware file."
    exit 1
  fi

  AARCH64_UEFI_VARS="${AARCH64_UEFI_VARS:-/tmp/goblins-os-$ARCH-uefi-vars.fd}"
  if [[ ! -f "$AARCH64_UEFI_VARS" ]]; then
    template="${AARCH64_UEFI_VARS_TEMPLATE:-}"
    if [[ -z "$template" ]]; then
      template="$(find_first_existing \
        /usr/share/edk2/aarch64/vars-template-pflash.raw \
        /usr/share/edk2/aarch64/QEMU_VARS-pflash.raw \
        /usr/share/AAVMF/AAVMF_VARS.fd \
        /usr/share/AAVMF/AAVMF_VARS.ms.fd \
        /usr/share/qemu-efi-aarch64/vars-template-pflash.raw || true)"
    fi
    if [[ -z "$template" || ! -f "$template" ]]; then
      warn "aarch64 UEFI variable store missing. Set AARCH64_UEFI_VARS to an existing writable store, or AARCH64_UEFI_VARS_TEMPLATE to a real EDK2/AAVMF vars template."
      exit 1
    fi
    mkdir -p "$(dirname "$AARCH64_UEFI_VARS")"
    cp "$template" "$AARCH64_UEFI_VARS"
  fi
  log "aarch64 UEFI pflash code: $AARCH64_UEFI_CODE"
  log "aarch64 UEFI variable store: $AARCH64_UEFI_VARS"
}

build_qemu_args() {
  local iso="$1"

  case "$ARCH" in
    x86_64)
      QEMU_ARGS=(-accel "$QEMU_ACCEL" -m "$VM_MEMORY" -smp "$VM_CPU" -cdrom "$iso" -drive "file=$QCOW2_PATH,if=virtio,format=qcow2" -boot d -vga std -display gtk -serial mon:stdio)
      ;;
    aarch64)
      prepare_aarch64_uefi
      QEMU_ARGS=(-machine "virt,accel=$QEMU_ACCEL,gic-version=max" -cpu host -m "$VM_MEMORY" -smp "$VM_CPU" -drive "if=pflash,format=raw,readonly=on,file=$AARCH64_UEFI_CODE" -drive "if=pflash,format=raw,file=$AARCH64_UEFI_VARS" -cdrom "$iso" -drive "file=$QCOW2_PATH,if=virtio,format=qcow2" -boot d -device virtio-gpu-pci -display gtk -serial mon:stdio)
      ;;
  esac
}

verify_iso_artifacts() {
  local iso_path="$1"
  local sha_path="$iso_path.sha256"
  local manifest_path="os/iso/output/$ARCH/manifest-goblins-os-$ARCH.json"
  local bib_manifest_path="os/iso/output/$ARCH/manifest-anaconda-iso.json"

  require_file "$ARCH ISO" "$iso_path"
  require_file "$ARCH ISO SHA256" "$sha_path"
  require_file "$ARCH ISO manifest" "$manifest_path"
  require_file_contains "$ARCH ISO manifest architecture" "$manifest_path" "\"architecture\": \"$ARCH\""
  require_file_contains "$ARCH ISO manifest image" "$manifest_path" "\"image\": \"$IMAGE_NAME\""
  if [[ "$RUN_QEMU" == "1" ]]; then
    require_file_contains "$ARCH ISO manifest nonlocal installer payload source" "$manifest_path" "\"installer_payload_source_local_only\": false"
    require_file_contains "$ARCH ISO manifest shippable release mode" "$manifest_path" "\"shippable_release\": true"
    require_file "$ARCH BIB manifest" "$bib_manifest_path"
    if rg -q 'bootc switch --mutate-in-place --transport registry (host\.docker\.internal|localhost[:/]|127\.|0\.0\.0\.0[:/]|goblins-os:|docker\.io/library/goblins-os:)' "$bib_manifest_path"; then
      warn "$ARCH BIB manifest still points at a local Docker/test registry; refusing display-backed release proof."
      exit 1
    fi
  fi

  if ! verify_sha256_file "$sha_path"; then
    warn "$ARCH ISO SHA256 verification failed: $sha_path"
    exit 1
  fi
  log "$ARCH ISO SHA256 file verified: $sha_path"
}

generate_release_evidence() {
  local repo_abs evidence_abs

  mkdir -p "$RELEASE_EVIDENCE_DIR"
  repo_abs="$(pwd)"
  evidence_abs="$(cd "$RELEASE_EVIDENCE_DIR" && pwd)"

  log "Generating $ARCH source release evidence in $RELEASE_EVIDENCE_DIR"
  "${CONTAINER_CMD[@]}" run --rm \
    -v "$repo_abs:/workspace:ro" \
    -v "$evidence_abs:/out" \
    -w /workspace \
    "$IMAGE_NAME" \
    /usr/libexec/goblins-os/goblins-os-verify \
    --source-root /workspace \
    --release-evidence /out \
    --arch "$ARCH"

  log "Generating $ARCH RPM release evidence from $IMAGE_NAME"
  "${CONTAINER_CMD[@]}" run --rm \
    -v "$evidence_abs:/out" \
    -w /out \
    "$IMAGE_NAME" \
    sh rpm-packages.command
}

verify_release_evidence() {
  local manifest="$RELEASE_EVIDENCE_DIR/release-evidence-manifest.json"

  require_file "$ARCH release evidence manifest" "$manifest"
  require_file_contains "$ARCH release evidence architecture" "$manifest" "\"architecture\": \"$ARCH\""
  require_file_contains "$ARCH release evidence asset provenance" "$manifest" "\"asset_provenance\": \"os/release/asset-provenance.toml\""
  require_file_contains "$ARCH release evidence third-party notices" "$manifest" "\"third_party_notices\": \"os/release/third-party-notices.toml\""
  require_file_contains "$ARCH release evidence trademark posture" "$manifest" "\"trademark_posture\": \"os/release/trademark-posture.toml\""
  require_file_contains "$ARCH release evidence source manifest" "$manifest" "\"source_tree_manifest\": \"os/release/source-tree-manifest.toml\""
  require_file "$ARCH Cargo SBOM package TSV" "$RELEASE_EVIDENCE_DIR/cargo-lock-packages.tsv"
  require_file "$ARCH RPM SBOM package TSV" "$RELEASE_EVIDENCE_DIR/rpm-packages.tsv"
  if ! rpm_sbom_arch_matches "$RELEASE_EVIDENCE_DIR/rpm-packages.tsv" "$ARCH"; then
    warn "$ARCH RPM SBOM contains packages outside $ARCH/noarch"
    exit 1
  fi
  log "$ARCH release evidence verified in $RELEASE_EVIDENCE_DIR"
}

case "$ARCH" in
  x86_64)
    QEMU_BIN=qemu-system-x86_64
    ;;
  aarch64)
    QEMU_BIN=qemu-system-aarch64
    ;;
esac

require_bool "GENERATE_RELEASE_EVIDENCE" "$GENERATE_RELEASE_EVIDENCE"
require_bool "RUN_QEMU" "$RUN_QEMU"
require_bool "RUN_CLOSEOFF" "$RUN_CLOSEOFF"
require_bool "PREFLIGHT_ONLY" "$PREFLIGHT_ONLY"
require_bool "GOBLINS_OS_ALLOW_EMULATED_DOCKER" "$ALLOW_EMULATED_DOCKER"
if [[ -z "$SHIPPABLE_RELEASE" ]]; then
  if [[ "$RUN_QEMU" == "1" ]]; then
    SHIPPABLE_RELEASE=1
  else
    SHIPPABLE_RELEASE=0
  fi
fi
require_bool "GOBLINS_OS_SHIPPABLE_RELEASE" "$SHIPPABLE_RELEASE"

if [[ "$RUN_QEMU" == "1" && "$HOST_OS" != "Linux" ]]; then
  warn "External display-backed gate requires a native Linux host with Docker and QEMU; got $HOST_OS."
  warn "Use RUN_QEMU=0 for Docker artifact/SBOM testing without claiming shipping proof."
  exit 1
fi
if [[ "$RUN_QEMU" == "1" && -z "$BIB_SOURCE_IMAGE" ]]; then
  warn "Display-backed shipping proof requires GOBLINS_OS_BIB_SOURCE_IMAGE to a real pullable bootc image ref."
  warn "The Docker-local registry path is allowed only for RUN_QEMU=0 artifact testing and cannot satisfy release signoff."
  exit 1
fi

REQUIRED_CMDS=("$CONTAINER_RUNTIME" rg)
if ! command -v sha256sum >/dev/null 2>&1 && ! command -v shasum >/dev/null 2>&1; then
  REQUIRED_CMDS+=(sha256sum)
fi
if [[ "$RUN_QEMU" == "1" ]]; then
  REQUIRED_CMDS+=("$QEMU_BIN" qemu-img)
fi
for cmd in "${REQUIRED_CMDS[@]}"; do
  if ! command -v "$cmd" >/dev/null 2>&1; then
    warn "Missing required command: $cmd"
    exit 1
  fi
done
configure_container_cmd

require_min_free_gb "Repository filesystem" "Repository filesystem needs at least" "$REPO_ROOT" "$MIN_HOST_FREE_GB"
require_min_free_gb "VM scratch filesystem" "VM scratch filesystem needs at least" "$(dirname "$QCOW2_PATH")" "$MIN_HOST_FREE_GB"
prepare_container_runtime

log "Running external sign-off helper in: $REPO_ROOT"
log "Architecture: $ARCH"
if [[ "$RUN_QEMU" == "1" || "$RUN_CLOSEOFF" == "1" ]]; then
  log "Screenshot target: $SCREENSHOT_DIR"
else
  log "Screenshot target: not created for artifact-only Docker run"
fi
log "Release evidence target: $RELEASE_EVIDENCE_DIR"
log "Container runtime: $CONTAINER_RUNTIME"
log "Minimum host free space: ${MIN_HOST_FREE_GB} GiB"
if [[ -n "$BIB_SOURCE_IMAGE" ]]; then
  log "Installer payload source: $BIB_SOURCE_IMAGE"
fi
log "Shippable release ISO source enforcement: $SHIPPABLE_RELEASE"

if [[ "$PREFLIGHT_ONLY" == "1" ]]; then
  if [[ "$RUN_QEMU" == "1" ]]; then
    prepare_native_qemu_acceleration
    if [[ "$ARCH" == "aarch64" ]]; then
      prepare_aarch64_uefi
    fi
  fi
  if [[ "$RUN_QEMU" == "0" ]]; then
    log "Docker artifact-only preflight passed for $ARCH on $HOST_ARCH; not release proof."
    log "Native $ARCH Linux/KVM display proof is still required before signoff."
  elif [[ "$HOST_ARCH" != "$ARCH" ]]; then
    log "Preflight passed for Docker-emulated $ARCH artifact testing on $HOST_ARCH."
    log "This is not release proof and does not replace a native $ARCH Linux/KVM runner."
  else
    log "Preflight passed for native $ARCH release runner."
  fi
  log "No image, ISO, SBOM, screenshot, or signoff artifact was generated."
  cat <<EOF2

Next artifact command:
  GOBLINS_OS_ARCH=$ARCH REPO_ROOT="$REPO_ROOT" os/hardware-gate/run-external-gate.sh

Artifact-only command without display proof:
  GOBLINS_OS_ARCH=$ARCH RUN_QEMU=0 REPO_ROOT="$REPO_ROOT" os/hardware-gate/run-external-gate.sh

Docker-emulated artifact-only command for non-native local testing:
  GOBLINS_OS_ARCH=$ARCH RUN_QEMU=0 GOBLINS_OS_ALLOW_EMULATED_DOCKER=1 REPO_ROOT="$REPO_ROOT" os/hardware-gate/run-external-gate.sh

Shipping proof still requires GOBLINS_OS_BIB_SOURCE_IMAGE=<real release bootc image ref>,
the full artifact command, display-backed screenshots, and close-signoff.
EOF2
  exit 0
fi

if [[ "$RUN_QEMU" == "1" || "$RUN_CLOSEOFF" == "1" ]]; then
  mkdir -p "$SCREENSHOT_DIR"
fi
"${CONTAINER_CMD[@]}" rmi -f "$IMAGE_NAME" localhost/goblins-os:ci || true

if [[ "$HOST_ARCH" != "$ARCH" && "$RUN_QEMU" == "0" && "$CONTAINER_RUNTIME" == "docker" && "$ALLOW_EMULATED_DOCKER" == "1" ]]; then
  warn "Skipping native pre-build for Docker-emulated artifact testing; os/iso/build-iso.sh will build $IMAGE_NAME with Docker --platform."
else
  log "Building native $ARCH bootc image"
  "${CONTAINER_CMD[@]}" build -f os/bootc/Containerfile -t "$IMAGE_NAME" .
fi

log "Building $ARCH installer ISO"
GOBLINS_OS_ARCH="$ARCH" \
  GOBLINS_OS_IMAGE="$IMAGE_NAME" \
  GOBLINS_OS_CONTAINER_RUNTIME="$CONTAINER_RUNTIME" \
  GOBLINS_OS_ALLOW_EMULATED_DOCKER="$ALLOW_EMULATED_DOCKER" \
  GOBLINS_OS_BIB_SOURCE_IMAGE="$BIB_SOURCE_IMAGE" \
  GOBLINS_OS_SHIPPABLE_RELEASE="$SHIPPABLE_RELEASE" \
  os/iso/build-iso.sh

LATEST_ISO="$ISO_DIR/goblins-os-$ARCH.iso"
if [[ ! -f "$LATEST_ISO" ]]; then
  warn "Expected architecture ISO missing: $LATEST_ISO"
  exit 1
fi
verify_iso_artifacts "$LATEST_ISO"

ISO_SHA="$(sha256_of_file "$LATEST_ISO")"
log "Latest ISO: $LATEST_ISO"
log "ISO SHA256: $ISO_SHA"

if [[ "$GENERATE_RELEASE_EVIDENCE" == "1" ]]; then
  generate_release_evidence
fi
verify_release_evidence
goblins_os_artifact_secret_scan "$REPO_ROOT"
log "$ARCH generated artifact/evidence secret scan passed"

if [[ "$RUN_QEMU" == "1" ]]; then
  prepare_native_qemu_acceleration
  if [[ ! -f "$QCOW2_PATH" ]]; then
    qemu-img create -f qcow2 "$QCOW2_PATH" 80G
  fi

  log "Launching display-backed VM for real gate capture"
  build_qemu_args "$LATEST_ISO"
  "$QEMU_BIN" "${QEMU_ARGS[@]}"
  log "VM session ended. Continue with screenshot capture checklist."
else
  warn "RUN_QEMU=0: built and verified artifacts only. Shipping still requires a later display-backed VM run and screenshot proof."
fi

if [[ "$RUN_CLOSEOFF" == "1" ]]; then
  log "Running close-signoff with screenshot directory set."
  GOBLINS_OS_ARCH="$ARCH" SCREENSHOT_DIR="$SCREENSHOT_DIR" GOBLINS_OS_IMAGE="$IMAGE_NAME" ./os/hardware-gate/close-signoff.sh
fi

log "Done."
cat <<EOF2

Next manual closure steps (once screenshots are collected):
- Edit os/signoff-notes.md and update the latest 'Manual Gate Run' section:
  - Runner/device
  - Architecture: $ARCH
  - CI run IDs/URLs
  - ISO SHA: $ISO_SHA
  - verify/self-test and runtime-engine fields
- Confirm checklist items:
  - ISO boot/install
  - first-boot onboarding and session
  - shell and settings
  - Build Studio prompt -> built app open
  - light/dark motion/interactions
  - Open advanced storage or Install Goblins OS Beside Another OS, then Installation Destination, Custom/manual storage or Reclaim Space, preserved Windows/macOS/APFS/Linux/other OS/recovery/EFI partitions, and bootloader/EFI summary
  - Runtime engine mode/source/config and built artifact path or URL, passed to close-signoff through RUNTIME_ENGINE_MODE, RUNTIME_ENGINE_SOURCE, RUNTIME_ENGINE_CONFIG, and BUILT_ARTIFACT_PATH_URL or edited into the signoff row after proof
  - If runtime-build-proof.json is missing, run os/runtime-gate/build-an-app-live-model.sh from inside the Goblins OS image/container with PROOF_PATH set to the screenshot run dir; do not hand-write the proof
  - Current project completion status: complete only after ISO, verifier, self-test, SBOM, gaming screenshots, install-storage screenshots, runtime engine, and built artifact proof are all present
EOF2
