#!/usr/bin/env bash
# Build the Goblins OS bare-metal install ISO from the bootc image using the
# supported bootc-image-builder (anaconda-iso). The ISO boots Anaconda, which
# deploys the immutable Goblins OS image to the disk the person explicitly
# chooses, then reboots into the native Goblins OS first-boot GUI.
#
# This builds a normal installable ISO. The supported host container runtime is
# Docker: the bootc image is pushed to a local registry and bootc-image-builder
# consumes that registry image from a privileged builder container.
#
# Usage:
#   os/iso/build-iso.sh
# Env overrides:
#   GOBLINS_OS_ARCH   target architecture: aarch64 or x86_64 (default host arch)
#   GOBLINS_OS_IMAGE   container image to install (default localhost/goblins-os:<arch>)
#   GOBLINS_OS_ROOTFS  installed root filesystem  (default xfs, matching the
#                      bootc install config in os/bootc-install/00-goblins-os.toml)
#   GOBLINS_OS_ISO_CONFIG
#                      bootc-image-builder config path (default os/iso/config.toml).
#                      Hardware proof jobs use os/iso/verify-config.toml; release
#                      media must keep the default interactive config.
#   OUTDIR             output directory           (default os/iso/output/<arch>)
#   BIB_IMAGE          digest-pinned bootc-image-builder image (default the
#                      reviewed multi-architecture digest below)
#   GOBLINS_OS_CONTAINER_RUNTIME
#                      docker (default docker)
#   GOBLINS_OS_ALLOW_EMULATED_DOCKER
#                      set 1 to allow a Docker engine whose architecture differs
#                      from GOBLINS_OS_ARCH; native matching remains the default
#                      for release media.
#   GOBLINS_OS_DOCKER_PLATFORM
#                      Docker platform for non-release Docker artifact testing
#                      (default linux/arm64 for aarch64, linux/amd64 for x86_64)
#   GOBLINS_OS_DOCKER_REGISTRY_PORT
#                      local registry port for Docker BIB handoff (default 5002)
#   GOBLINS_OS_DOCKER_REGISTRY_NAME
#                      local registry container name (default goblins-os-registry)
#   GOBLINS_OS_DOCKER_REGISTRY_NETWORK
#                      dedicated internal Docker bridge used only by the local
#                      registry and BIB (default goblins-os-bib-<registry-port>)
#   GOBLINS_OS_DOCKER_EGRESS_NETWORK
#                      dedicated non-internal Docker bridge used only for BIB
#                      internet egress (default goblins-os-bib-egress-<registry-port>)
#   GOBLINS_OS_DOCKER_REGISTRY_PROBE_TIMEOUT_SECS
#                      maximum seconds for the BIB-network registry readiness
#                      probe (default 20)
#   GOBLINS_OS_BIB_STORAGE_VOLUME
#                      Docker volume for bootc-image-builder storage
#   GOBLINS_OS_BIB_SOURCE_IMAGE
#                      source image passed to bootc-image-builder. If omitted,
#                      Docker local testing uses the dedicated registry container
#                      DNS name on its isolated bridge network.
#                      Shippable release media must use a real pullable registry
#                      ref, because Anaconda ISO installs track this ref for
#                      post-install bootc updates.
#   GOBLINS_OS_SKIP_LOCAL_IMAGE_BUILD
#                      set 1 only when GOBLINS_OS_BIB_SOURCE_IMAGE points at a
#                      real pullable registry image that was already built. This
#                      avoids exporting the full bootc image into the local Docker
#                      daemon on constrained CI runners.
#   GOBLINS_OS_SHIPPABLE_RELEASE
#                      set 1 to fail if the BIB source image is local/test-only
#   GOBLINS_OS_CANDIDATE_COMMIT
#                      exact 40-hex source commit used for this image and ISO;
#                      required for every artifact, including non-release tests
set -euo pipefail

REPO_ROOT="$(cd "$(dirname "$0")/../.." && pwd)"
. "$REPO_ROOT/os/iso/manifest-provenance.sh"
CONFIG="${GOBLINS_OS_ISO_CONFIG:-$REPO_ROOT/os/iso/config.toml}"
case "$CONFIG" in
  /*) ;;
  *) CONFIG="$REPO_ROOT/$CONFIG" ;;
esac
[ -f "$CONFIG" ] || { echo "error: missing ISO config: $CONFIG" >&2; exit 1; }
CONFIG_LABEL="$CONFIG"
case "$CONFIG_LABEL" in
  "$REPO_ROOT"/*) CONFIG_LABEL="${CONFIG_LABEL#"$REPO_ROOT/"}" ;;
esac
BIB="${BIB_IMAGE:-quay.io/centos-bootc/bootc-image-builder@sha256:2b52843ea2bfda73b0a08d97e76b734393b1d3a804681b9fabb26723bd3a2f0b}"
INSTALLER_BRANDING_IMAGE="${GOBLINS_OS_INSTALLER_BRANDING_IMAGE:-ghcr.io/joe-simo/goblins-os-installer-branding-tool@sha256:a5b2be1ce90514f1e4d1447bcd6eb6af51ea98644bc310c58ce649a7550e39c0}"
ROOTFS="${GOBLINS_OS_ROOTFS:-xfs}"
CONTAINER_RUNTIME="${GOBLINS_OS_CONTAINER_RUNTIME:-docker}"
ALLOW_EMULATED_DOCKER="${GOBLINS_OS_ALLOW_EMULATED_DOCKER:-0}"
DOCKER_REGISTRY_PORT="${GOBLINS_OS_DOCKER_REGISTRY_PORT:-5002}"
DOCKER_REGISTRY_NAME="${GOBLINS_OS_DOCKER_REGISTRY_NAME:-goblins-os-registry}"
DOCKER_REGISTRY_NETWORK="${GOBLINS_OS_DOCKER_REGISTRY_NETWORK:-goblins-os-bib-$DOCKER_REGISTRY_PORT}"
DOCKER_EGRESS_NETWORK="${GOBLINS_OS_DOCKER_EGRESS_NETWORK:-goblins-os-bib-egress-$DOCKER_REGISTRY_PORT}"
DOCKER_REGISTRY_PROBE_TIMEOUT_SECS="${GOBLINS_OS_DOCKER_REGISTRY_PROBE_TIMEOUT_SECS:-20}"
LOCAL_REGISTRY_IMAGE="registry:2"
BIB_STORAGE_VOLUME="${GOBLINS_OS_BIB_STORAGE_VOLUME:-goblins-os-bib-storage-$DOCKER_REGISTRY_PORT}"
BIB_SOURCE_IMAGE_OVERRIDE="${GOBLINS_OS_BIB_SOURCE_IMAGE:-}"
SKIP_LOCAL_IMAGE_BUILD="${GOBLINS_OS_SKIP_LOCAL_IMAGE_BUILD:-0}"
SHIPPABLE_RELEASE="${GOBLINS_OS_SHIPPABLE_RELEASE:-0}"
CANDIDATE_COMMIT="${GOBLINS_OS_CANDIDATE_COMMIT:-}"
BIB_SOURCE_IMAGE_USED=""
BIB_SOURCE_KIND=""
BIB_SOURCE_LOCAL_ONLY="false"
INSTALLER_BRANDING_APPLIED="false"
DOCKER_PLATFORM=""
DOCKER_EMULATION_PREFLIGHT_TIMEOUT_SECS="${GOBLINS_OS_DOCKER_EMULATION_PREFLIGHT_TIMEOUT_SECS:-20}"

normalize_arch() {
  case "$1" in
    aarch64|arm64) echo "aarch64" ;;
    x86_64|amd64) echo "x86_64" ;;
    *) echo "unsupported" ;;
  esac
}

docker_platform_for_arch() {
  case "$1" in
    aarch64) echo "linux/arm64" ;;
    x86_64) echo "linux/amd64" ;;
    *)
      echo "error: unsupported architecture for Docker platform: $1" >&2
      exit 1
      ;;
  esac
}

arch_for_docker_platform() {
  case "$1" in
    linux/arm64|linux/aarch64) echo "aarch64" ;;
    linux/amd64|linux/x86_64) echo "x86_64" ;;
    *)
      echo "unsupported"
      ;;
  esac
}

require_command() {
  local cmd="$1"
  if ! command -v "$cmd" >/dev/null 2>&1; then
    echo "error: missing required command: $cmd" >&2
    exit 1
  fi
}

require_docker_dns_label() {
  local label="$1"
  local value="$2"

  if [ "${#value}" -gt 63 ] \
    || [[ ! "$value" =~ ^[a-z0-9]([a-z0-9-]*[a-z0-9])?$ ]]; then
    echo "error: $label must be a lowercase Docker DNS label (1-63 characters; letters, digits, and interior hyphens only)." >&2
    exit 1
  fi
}

require_docker_object_name() {
  local label="$1"
  local value="$2"

  if [ "${#value}" -gt 128 ] \
    || [[ ! "$value" =~ ^[A-Za-z0-9][A-Za-z0-9_.-]*$ ]]; then
    echo "error: $label must be a Docker object name (1-128 letters, digits, dots, underscores, or hyphens; no leading punctuation)." >&2
    exit 1
  fi
}

require_user_defined_network_name() {
  local label="$1"
  local value="$2"

  case "$value" in
    bridge|host|none)
      echo "error: $label must name a user-defined Docker network, not the built-in '$value' network." >&2
      exit 1
      ;;
  esac
}

require_bounded_positive_integer() {
  local label="$1"
  local value="$2"
  local maximum="$3"

  if [[ ! "$value" =~ ^[1-9][0-9]{0,4}$ ]] \
    || [ "$((10#$value))" -gt "$maximum" ]; then
    echo "error: $label must be an integer from 1 through $maximum." >&2
    exit 1
  fi
}

docker_version_major() {
  local version="$1"

  if [[ "$version" =~ ^([0-9]{1,4})\. ]]; then
    printf '%s\n' "${BASH_REMATCH[1]}"
    return 0
  fi
  return 1
}

docker_versions_support_dual_network() {
  local client_major server_major

  client_major="$(docker_version_major "$1")" || return 1
  server_major="$(docker_version_major "$2")" || return 1
  [ "$((10#$client_major))" -ge 28 ] \
    && [ "$((10#$server_major))" -ge 28 ]
}

require_docker_dual_network_versions() {
  local client_version server_version

  client_version="$(docker version --format '{{.Client.Version}}')" || {
    echo "error: cannot read the Docker client version for the BIB dual-network preflight." >&2
    exit 1
  }
  server_version="$(docker version --format '{{.Server.Version}}')" || {
    echo "error: cannot read the Docker server version for the BIB dual-network preflight." >&2
    exit 1
  }
  if ! docker_versions_support_dual_network "$client_version" "$server_version"; then
    echo "error: the local-registry BIB route requires Docker 28 or newer on both client and server (client=$client_version server=$server_version)." >&2
    exit 1
  fi
}

require_bounded_positive_integer GOBLINS_OS_DOCKER_REGISTRY_PORT "$DOCKER_REGISTRY_PORT" 65535
require_docker_dns_label GOBLINS_OS_DOCKER_REGISTRY_NAME "$DOCKER_REGISTRY_NAME"
if [ "$DOCKER_REGISTRY_NAME" = "localhost" ]; then
  echo "error: GOBLINS_OS_DOCKER_REGISTRY_NAME cannot be localhost; that name is reserved for container loopback and cannot route to the managed registry." >&2
  exit 1
fi
require_docker_object_name GOBLINS_OS_DOCKER_REGISTRY_NETWORK "$DOCKER_REGISTRY_NETWORK"
require_docker_object_name GOBLINS_OS_DOCKER_EGRESS_NETWORK "$DOCKER_EGRESS_NETWORK"
require_user_defined_network_name GOBLINS_OS_DOCKER_REGISTRY_NETWORK "$DOCKER_REGISTRY_NETWORK"
require_user_defined_network_name GOBLINS_OS_DOCKER_EGRESS_NETWORK "$DOCKER_EGRESS_NETWORK"
require_docker_object_name GOBLINS_OS_BIB_STORAGE_VOLUME "$BIB_STORAGE_VOLUME"
require_bounded_positive_integer \
  GOBLINS_OS_DOCKER_REGISTRY_PROBE_TIMEOUT_SECS \
  "$DOCKER_REGISTRY_PROBE_TIMEOUT_SECS" \
  120
if [ "$DOCKER_EGRESS_NETWORK" = "$DOCKER_REGISTRY_NETWORK" ]; then
  echo "error: GOBLINS_OS_DOCKER_EGRESS_NETWORK and GOBLINS_OS_DOCKER_REGISTRY_NETWORK must name distinct Docker networks." >&2
  exit 1
fi

if [[ ! "$CANDIDATE_COMMIT" =~ ^[0-9a-fA-F]{40}$ ]]; then
  echo "error: GOBLINS_OS_CANDIDATE_COMMIT must be the exact 40-hex source commit used to build this ISO." >&2
  exit 1
fi
CANDIDATE_COMMIT="$(printf '%s' "$CANDIDATE_COMMIT" | tr '[:upper:]' '[:lower:]')"
if command -v git >/dev/null 2>&1 && git -C "$REPO_ROOT" rev-parse --is-inside-work-tree >/dev/null 2>&1; then
  SOURCE_HEAD="$(git -C "$REPO_ROOT" rev-parse HEAD)"
  if [ "$(printf '%s' "$SOURCE_HEAD" | tr '[:upper:]' '[:lower:]')" != "$CANDIDATE_COMMIT" ]; then
    echo "error: selected candidate $CANDIDATE_COMMIT does not match checked-out source HEAD $SOURCE_HEAD." >&2
    exit 1
  fi
  if [ -n "$(git -C "$REPO_ROOT" status --porcelain --untracked-files=normal)" ]; then
    echo "error: source worktree has uncommitted files; commit the exact candidate before generating release media." >&2
    exit 1
  fi
fi

classify_bib_source_route() {
  local ref="$1"
  local authority first_component

  if [ -z "$ref" ] || [[ "$ref" =~ [[:space:]] ]] || [[ "$ref" == -* ]]; then
    printf '%s\n' invalid
    return 0
  fi
  if [[ "$ref" != */* ]]; then
    printf '%s\n' unsupported-local
    return 0
  fi
  first_component="${ref%%/*}"
  case "$first_component" in
    *.*|*:*|localhost) authority="$first_component" ;;
    *)
      printf '%s\n' release-registry
      return 0
      ;;
  esac

  case "$authority" in
    "$DOCKER_REGISTRY_NAME":5000)
      printf '%s\n' managed-registry
      ;;
    host.docker.internal|host.docker.internal:*)
      printf '%s\n' host-gateway
      ;;
    localhost|localhost:*|*.localhost|*.localhost:*|127.*|0.0.0.0|0.0.0.0:*|'[::1]'|'[::1]':*)
      printf '%s\n' container-loopback
      ;;
    host.containers.internal|host.containers.internal:*|gateway.docker.internal|gateway.docker.internal:*|*.local|*.local:*|*.docker.internal|*.docker.internal:*)
      printf '%s\n' unsupported-local
      ;;
    *)
      if [[ "$authority" =~ ^[A-Za-z0-9_-]+:[0-9]+$ ]]; then
        printf '%s\n' unsupported-local
      else
        printf '%s\n' release-registry
      fi
      ;;
  esac
}

image_ref_is_digest_pinned() {
  [[ "$1" =~ ^[^[:space:]@]+@sha256:[0-9a-f]{64}$ ]]
}

require_shippable_source_ref() {
  local ref="$1"

  if [ "$SHIPPABLE_RELEASE" != "1" ]; then
    return 0
  fi
  if goblins_os_image_ref_is_local_only "$ref"; then
    echo "error: shippable release media cannot track local/test-only installer payload ref: $ref" >&2
    echo "       Push the bootc image to a real release registry and set GOBLINS_OS_BIB_SOURCE_IMAGE to that pullable ref." >&2
    exit 1
  fi
  if ! image_ref_is_digest_pinned "$ref"; then
    echo "error: shippable release media requires a digest-pinned installer payload ref: $ref" >&2
    echo "       Set GOBLINS_OS_BIB_SOURCE_IMAGE to <registry>/<image>@sha256:<64-hex-digest>." >&2
    exit 1
  fi
}

require_shippable_tool_ref() {
  local label="$1"
  local ref="$2"

  if [ "$SHIPPABLE_RELEASE" = "1" ] && ! image_ref_is_digest_pinned "$ref"; then
    echo "error: shippable release media requires a digest-pinned $label image: $ref" >&2
    echo "       Review and set $label to <registry>/<image>@sha256:<64-hex-digest>." >&2
    exit 1
  fi
}

require_shippable_branding_tool_ref() {
  if [ "$SHIPPABLE_RELEASE" != "1" ]; then
    return 0
  fi
  require_shippable_tool_ref GOBLINS_OS_INSTALLER_BRANDING_IMAGE "$INSTALLER_BRANDING_IMAGE"
  case "$INSTALLER_BRANDING_IMAGE" in
    */goblins-os-installer-branding-tool@sha256:*) ;;
    *)
      echo "error: shippable media requires the dedicated Goblins OS installer branding-tool image" >&2
      exit 1
      ;;
  esac
}

verify_docker_emulation_runtime() {
  local name output pid i status

  if [ "$ARCH" = "$RUNTIME_ARCH" ]; then
    return 0
  fi

  name="goblins-os-rustc-$ARCH-preflight-$$"
  output="${TMPDIR:-/tmp}/$name.log"
  rm -f "$output"
  docker rm -f "$name" >/dev/null 2>&1 || true

  echo "==> Checking Docker $DOCKER_PLATFORM emulation can run the Rust toolchain"
  (docker run --rm --name "$name" --platform "$DOCKER_PLATFORM" rust:1.88 rustc -Vv >"$output" 2>&1) &
  pid=$!
  for i in $(seq 1 "$DOCKER_EMULATION_PREFLIGHT_TIMEOUT_SECS"); do
    if ! kill -0 "$pid" 2>/dev/null; then
      status=0
      wait "$pid" || status=$?
      if [ "$status" -ne 0 ]; then
        cat "$output" >&2 || true
        echo "error: Docker $DOCKER_PLATFORM emulation cannot run rustc; use a native $ARCH runner for release artifacts or fix the host emulation backend before local artifact testing." >&2
        exit 1
      fi
      rm -f "$output"
      return 0
    fi
    sleep 1
  done

  docker rm -f "$name" >/dev/null 2>&1 || true
  kill "$pid" >/dev/null 2>&1 || true
  wait "$pid" >/dev/null 2>&1 || true
  cat "$output" >&2 || true
  rm -f "$output"
  echo "error: Docker $DOCKER_PLATFORM emulation preflight timed out after ${DOCKER_EMULATION_PREFLIGHT_TIMEOUT_SECS}s; use a native $ARCH runner for release artifacts or fix the host emulation backend before local artifact testing." >&2
  exit 1
}

docker_engine_arch() {
  local arch

  require_command docker
  arch="$(docker info --format '{{.Architecture}}' 2>/dev/null || true)"
  if [ -z "$arch" ]; then
    echo "error: Docker engine is not available." >&2
    exit 1
  fi
  normalize_arch "$arch"
}

HOST_ARCH="$(normalize_arch "$(uname -m)")"
if [ "$CONTAINER_RUNTIME" != "docker" ]; then
  echo "error: unsupported GOBLINS_OS_CONTAINER_RUNTIME='$CONTAINER_RUNTIME'; expected docker." >&2
  exit 1
fi
RUNTIME_ARCH="$(docker_engine_arch)"

ARCH="$(normalize_arch "${GOBLINS_OS_ARCH:-$RUNTIME_ARCH}")"
if [ "$ARCH" = "unsupported" ]; then
  echo "error: unsupported GOBLINS_OS_ARCH='${GOBLINS_OS_ARCH:-$(uname -m)}'; expected aarch64 or x86_64." >&2
  exit 1
fi
if [ "$RUNTIME_ARCH" = "unsupported" ]; then
  echo "error: unsupported $CONTAINER_RUNTIME engine architecture; expected native aarch64 or x86_64." >&2
  exit 1
fi
if [ "$ARCH" != "$RUNTIME_ARCH" ] && [ "$ALLOW_EMULATED_DOCKER" != "1" ]; then
  echo "error: requested $ARCH ISO on $RUNTIME_ARCH Docker engine." >&2
  echo "       Goblins OS release media must be built on a native $ARCH container engine." >&2
  echo "       For non-release Docker experiments only, set GOBLINS_OS_ALLOW_EMULATED_DOCKER=1." >&2
  exit 1
fi
if [ "$SHIPPABLE_RELEASE" = "1" ] \
  && { [ "$ARCH" != "$HOST_ARCH" ] || [ "$ARCH" != "$RUNTIME_ARCH" ]; }; then
  echo "error: shippable $ARCH media requires a native $ARCH host and container engine (host=$HOST_ARCH engine=$RUNTIME_ARCH)" >&2
  echo "       Emulated Docker builds are restricted to GOBLINS_OS_SHIPPABLE_RELEASE=0 experiments." >&2
  exit 1
fi
DOCKER_PLATFORM="${GOBLINS_OS_DOCKER_PLATFORM:-$(docker_platform_for_arch "$ARCH")}"
if [ "$(arch_for_docker_platform "$DOCKER_PLATFORM")" != "$ARCH" ]; then
  echo "error: GOBLINS_OS_DOCKER_PLATFORM='$DOCKER_PLATFORM' does not match GOBLINS_OS_ARCH='$ARCH'." >&2
  exit 1
fi

IMAGE="${GOBLINS_OS_IMAGE:-localhost/goblins-os:$ARCH}"
OUTDIR="${OUTDIR:-$REPO_ROOT/os/iso/output/$ARCH}"
case "$OUTDIR" in
  /*) ;;
  *) OUTDIR="$REPO_ROOT/$OUTDIR" ;;
esac
mkdir -p "$OUTDIR"
OUTDIR="$(cd "$OUTDIR" && pwd -P)"
ISO_NAME="goblins-os-$ARCH.iso"
ISO_PATH="$OUTDIR/bootiso/$ISO_NAME"
SHA_PATH="$ISO_PATH.sha256"
MANIFEST_PATH="$OUTDIR/manifest-goblins-os-$ARCH.json"

sha256_file() {
  if command -v sha256sum >/dev/null 2>&1; then
    sha256sum "$1"
  elif command -v shasum >/dev/null 2>&1; then
    shasum -a 256 "$1"
  else
    echo "error: no sha256sum or shasum command available." >&2
    exit 1
  fi
}

brand_installer() {
  # Stock bootc-image-builder produces an Anaconda runtime (install.img) with
  # Fedora's sidebar art, logo, and accent. Replace them with the Goblins identity
  # so the installer carries ZERO Fedora chrome (os/iso/remaster-anaconda-branding.sh:
  # arch-agnostic squashfs swap + xorriso boot-preserving replay). Opt out with
  # GOBLINS_OS_SKIP_INSTALLER_BRANDING=1.
  local iso="$1" dir base
  if [ "${GOBLINS_OS_SKIP_INSTALLER_BRANDING:-0}" = "1" ]; then
    if [ "$SHIPPABLE_RELEASE" = "1" ]; then
      echo "error: shippable release media cannot skip Goblins installer branding" >&2
      exit 1
    fi
    echo "==> Skipping Anaconda installer branding (GOBLINS_OS_SKIP_INSTALLER_BRANDING=1)"
    return 0
  fi
  dir="$(cd "$(dirname "$iso")" && pwd)"
  base="$(basename "$iso")"
  echo "==> Branding the Anaconda installer (Goblins sidebar/logo/accent)"
  docker run --rm --pull=missing \
    --platform "$DOCKER_PLATFORM" \
    -v "$REPO_ROOT/os/brand/anaconda":/brand:ro \
    -v "$REPO_ROOT/os/iso":/scripts:ro \
    -v "$dir":/iso:ro \
    -v "$dir":/work \
    -e ISO_IN="/iso/$base" \
    -e ISO_OUT="/work/$base.branded" \
    "$INSTALLER_BRANDING_IMAGE" bash /scripts/remaster-anaconda-branding.sh
  # Reuse the same reviewed branding image to reclaim ownership without adding a
  # mutable helper to the trust boundary.
  docker run --rm --pull=missing \
    --platform "$DOCKER_PLATFORM" \
    -v "$dir":/work \
    --entrypoint /bin/chown \
    "$INSTALLER_BRANDING_IMAGE" \
    -R "$(id -u):$(id -g)" /work
  mv -f "$iso.branded" "$iso"
  INSTALLER_BRANDING_APPLIED="true"
}

finalize_outputs() {
  local source_iso="$1"
  local source_manifest="$2"
  local iso_count

  [ -s "$source_iso" ] || {
    echo "error: bootc-image-builder did not produce the exact expected bootiso/install.iso" >&2
    exit 1
  }
  [ -s "$source_manifest" ] || {
    echo "error: bootc-image-builder did not produce manifest-anaconda-iso.json" >&2
    exit 1
  }
  iso_count="$(find "$(dirname "$(dirname "$source_iso")")" -type f -name '*.iso' -print | wc -l | tr -d ' ')"
  [ "$iso_count" = "1" ] || {
    echo "error: bootc-image-builder output must contain exactly one ISO; found $iso_count" >&2
    exit 1
  }
  mkdir -p "$(dirname "$ISO_PATH")"
  mv -f "$source_iso" "$ISO_PATH"
  cp "$source_manifest" "$OUTDIR/manifest-anaconda-iso.json"
  # Replace Fedora's Anaconda chrome with the Goblins identity before sealing the
  # checksum, so the shipped ISO's installer carries zero Fedora branding.
  brand_installer "$ISO_PATH"
  # Emit a portable, basename-relative checksum so no machine-specific absolute
  # path is baked into a shipping artifact; verify with `cd <dir> && sha256sum -c`.
  (cd "$(dirname "$ISO_PATH")" && sha256_file "$(basename "$ISO_PATH")") > "$SHA_PATH"
  cat > "$MANIFEST_PATH" <<EOF
{
  "product": "Goblins OS",
  "architecture": "$ARCH",
  "candidate_commit": "$CANDIDATE_COMMIT",
  "image": "$IMAGE",
  "container_runtime": "$CONTAINER_RUNTIME",
  "rootfs": "$ROOTFS",
  "iso": "bootiso/$ISO_NAME",
  "sha256_file": "bootiso/$ISO_NAME.sha256",
  "built_on": "$(date -u +%Y-%m-%dT%H:%M:%SZ)",
  "native_host_arch": "$HOST_ARCH",
  "container_engine_arch": "$RUNTIME_ARCH",
  "docker_platform": "$DOCKER_PLATFORM",
  "installer_config": "$CONFIG_LABEL",
  "installer_branding_applied": $INSTALLER_BRANDING_APPLIED,
  "installer_branding_image": "$INSTALLER_BRANDING_IMAGE",
  "installer_branding_ownership_helper_image": "$INSTALLER_BRANDING_IMAGE",
  "builder_image": "$BIB",
  "builder_output_ownership_helper_image": "$BIB",
  "builder_source_image": "$BIB_SOURCE_IMAGE_USED",
  "installer_payload_source_kind": "$BIB_SOURCE_KIND",
  "installer_payload_source_local_only": $BIB_SOURCE_LOCAL_ONLY,
  "shippable_release": $([ "$SHIPPABLE_RELEASE" = "1" ] && echo true || echo false)
}
EOF

  echo "==> Goblins OS $ARCH installer ISO: $ISO_PATH"
  echo "==> SHA256: $SHA_PATH"
  echo "==> Manifest: $MANIFEST_PATH"
}

assert_dedicated_egress_network_membership() {
  local member members

  if ! members="$(
    docker network inspect \
      --format '{{range .Containers}}{{println .Name}}{{end}}' \
      "$DOCKER_EGRESS_NETWORK"
  )"; then
    echo "error: cannot inspect dedicated BIB egress network $DOCKER_EGRESS_NETWORK." >&2
    exit 1
  fi

  while IFS= read -r member; do
    [ -z "$member" ] && continue
    echo "error: dedicated BIB egress network $DOCKER_EGRESS_NETWORK has unexpected container $member attached; refusing to share the builder egress boundary." >&2
    exit 1
  done <<< "$members"
}

ensure_docker_egress_network() {
  local driver internal purpose scope

  if ! docker network inspect "$DOCKER_EGRESS_NETWORK" >/dev/null 2>&1; then
    docker network create \
      --driver bridge \
      --label org.goblins-os.purpose=installer-builder-egress \
      "$DOCKER_EGRESS_NETWORK" >/dev/null
  fi
  driver="$(docker network inspect --format '{{.Driver}}' "$DOCKER_EGRESS_NETWORK")"
  internal="$(docker network inspect --format '{{.Internal}}' "$DOCKER_EGRESS_NETWORK")"
  scope="$(docker network inspect --format '{{.Scope}}' "$DOCKER_EGRESS_NETWORK")"
  purpose="$(docker network inspect --format '{{index .Labels "org.goblins-os.purpose"}}' "$DOCKER_EGRESS_NETWORK")"
  if [ "$driver" != "bridge" ] \
    || [ "$internal" != "false" ] \
    || [ "$scope" != "local" ] \
    || [ "$purpose" != "installer-builder-egress" ]; then
    echo "error: Docker network $DOCKER_EGRESS_NETWORK does not satisfy the dedicated non-internal BIB egress bridge contract with local scope." >&2
    exit 1
  fi
  assert_dedicated_egress_network_membership
}

assert_dedicated_registry_network_membership() {
  local require_registry="${1:-false}"
  local member members registry_members

  if ! members="$(
    docker network inspect \
      --format '{{range .Containers}}{{println .Name}}{{end}}' \
      "$DOCKER_REGISTRY_NETWORK"
  )"; then
    echo "error: cannot inspect dedicated registry network $DOCKER_REGISTRY_NETWORK." >&2
    exit 1
  fi

  registry_members=0
  while IFS= read -r member; do
    [ -z "$member" ] && continue
    if [ "$member" != "$DOCKER_REGISTRY_NAME" ]; then
      echo "error: dedicated registry network $DOCKER_REGISTRY_NETWORK has unexpected container $member attached; refusing to expose the unauthenticated build registry to it." >&2
      exit 1
    fi
    registry_members=$((registry_members + 1))
  done <<< "$members"
  if [ "$require_registry" = "true" ] && [ "$registry_members" -ne 1 ]; then
    echo "error: dedicated registry network $DOCKER_REGISTRY_NETWORK must contain exactly the running registry container $DOCKER_REGISTRY_NAME." >&2
    exit 1
  fi
}

ensure_docker_registry_network() {
  local driver internal purpose scope

  if ! docker network inspect "$DOCKER_REGISTRY_NETWORK" >/dev/null 2>&1; then
    docker network create \
      --driver bridge \
      --internal \
      --label org.goblins-os.purpose=installer-registry-handoff \
      "$DOCKER_REGISTRY_NETWORK" >/dev/null
  fi
  driver="$(docker network inspect --format '{{.Driver}}' "$DOCKER_REGISTRY_NETWORK")"
  internal="$(docker network inspect --format '{{.Internal}}' "$DOCKER_REGISTRY_NETWORK")"
  scope="$(docker network inspect --format '{{.Scope}}' "$DOCKER_REGISTRY_NETWORK")"
  purpose="$(docker network inspect --format '{{index .Labels "org.goblins-os.purpose"}}' "$DOCKER_REGISTRY_NETWORK")"
  if [ "$driver" != "bridge" ] \
    || [ "$internal" != "true" ] \
    || [ "$scope" != "local" ] \
    || [ "$purpose" != "installer-registry-handoff" ]; then
    echo "error: Docker network $DOCKER_REGISTRY_NETWORK does not satisfy the dedicated internal registry bridge contract with local scope." >&2
    exit 1
  fi
  assert_dedicated_registry_network_membership
}

ensure_docker_registry() {
  local container_image endpoint_network_id expected_network_id network_count
  local expected_port_bindings port_binding purpose running

  ensure_docker_egress_network
  ensure_docker_registry_network
  expected_network_id="$(docker network inspect --format '{{.Id}}' "$DOCKER_REGISTRY_NETWORK")"
  if docker container inspect "$DOCKER_REGISTRY_NAME" >/dev/null 2>&1; then
    container_image="$(docker inspect --format '{{.Config.Image}}' "$DOCKER_REGISTRY_NAME")"
    network_count="$(docker inspect --format '{{len .NetworkSettings.Networks}}' "$DOCKER_REGISTRY_NAME")"
    endpoint_network_id="$(docker inspect --format "{{with index .NetworkSettings.Networks \"$DOCKER_REGISTRY_NETWORK\"}}{{.NetworkID}}{{end}}" "$DOCKER_REGISTRY_NAME")"
    port_binding="$(
      docker inspect --format '{{range (index .HostConfig.PortBindings "5000/tcp")}}{{println .HostIp .HostPort}}{{end}}' "$DOCKER_REGISTRY_NAME" \
        | sed '/^$/d' \
        | LC_ALL=C sort
    )"
    expected_port_bindings="$(
      printf '127.0.0.1 %s\n::1 %s\n' "$DOCKER_REGISTRY_PORT" "$DOCKER_REGISTRY_PORT" \
        | LC_ALL=C sort
    )"
    purpose="$(docker inspect --format '{{index .Config.Labels "org.goblins-os.purpose"}}' "$DOCKER_REGISTRY_NAME")"
    if [ "$container_image" != "$LOCAL_REGISTRY_IMAGE" ] \
      || [ "$network_count" != "1" ] \
      || [ -z "$endpoint_network_id" ] \
      || [ "$endpoint_network_id" != "$expected_network_id" ] \
      || [ "$port_binding" != "$expected_port_bindings" ] \
      || [ "$purpose" != "installer-local-registry" ]; then
      echo "error: existing Docker container $DOCKER_REGISTRY_NAME does not satisfy the isolated, loopback-only installer registry contract; remove that exact container explicitly before retrying." >&2
      exit 1
    fi
    if [ "$(docker inspect -f '{{.State.Running}}' "$DOCKER_REGISTRY_NAME")" != "true" ]; then
      docker start "$DOCKER_REGISTRY_NAME" >/dev/null
    fi
  else
    docker run -d \
      --name "$DOCKER_REGISTRY_NAME" \
      --network "$DOCKER_REGISTRY_NETWORK" \
      --label org.goblins-os.purpose=installer-local-registry \
      -p "127.0.0.1:$DOCKER_REGISTRY_PORT:5000" \
      -p "[::1]:$DOCKER_REGISTRY_PORT:5000" \
      "$LOCAL_REGISTRY_IMAGE" >/dev/null
  fi
  network_count="$(docker inspect --format '{{len .NetworkSettings.Networks}}' "$DOCKER_REGISTRY_NAME")"
  endpoint_network_id="$(docker inspect --format "{{with index .NetworkSettings.Networks \"$DOCKER_REGISTRY_NETWORK\"}}{{.NetworkID}}{{end}}" "$DOCKER_REGISTRY_NAME")"
  running="$(docker inspect --format '{{.State.Running}}' "$DOCKER_REGISTRY_NAME")"
  if [ "$running" != "true" ] \
    || [ "$network_count" != "1" ] \
    || [ -z "$endpoint_network_id" ] \
    || [ "$endpoint_network_id" != "$expected_network_id" ]; then
    echo "error: running registry $DOCKER_REGISTRY_NAME lacks the exact live endpoint for $DOCKER_REGISTRY_NETWORK." >&2
    exit 1
  fi
  assert_dedicated_registry_network_membership true
  assert_dedicated_egress_network_membership
}

bounded_docker_remove() {
  local container_name="$1"
  local remove_pid tick

  (docker rm -f "$container_name" >/dev/null 2>&1 || true) &
  remove_pid=$!
  for tick in 1 2 3 4 5; do
    if ! kill -0 "$remove_pid" 2>/dev/null; then
      wait "$remove_pid" >/dev/null 2>&1 || true
      return 0
    fi
    sleep 1
  done
  if kill -0 "$remove_pid" 2>/dev/null; then
    kill "$remove_pid" >/dev/null 2>&1 || true
  fi
  wait "$remove_pid" >/dev/null 2>&1 || true
}

bounded_docker_network_remove() {
  local network_name="$1"
  local remove_pid tick

  (docker network rm "$network_name" >/dev/null 2>&1 || true) &
  remove_pid=$!
  for tick in 1 2 3 4 5; do
    if ! kill -0 "$remove_pid" 2>/dev/null; then
      wait "$remove_pid" >/dev/null 2>&1 || true
      return 0
    fi
    sleep 1
  done
  if kill -0 "$remove_pid" 2>/dev/null; then
    kill "$remove_pid" >/dev/null 2>&1 || true
  fi
  wait "$remove_pid" >/dev/null 2>&1 || true
}

bounded_stop_process() {
  local process_id="$1"
  local tick

  kill "$process_id" >/dev/null 2>&1 || true
  for tick in 1 2 3 4 5; do
    if ! kill -0 "$process_id" 2>/dev/null; then
      wait "$process_id" >/dev/null 2>&1 || true
      return 0
    fi
    sleep 1
  done
  kill -9 "$process_id" >/dev/null 2>&1 || true
  wait "$process_id" >/dev/null 2>&1 || true
}

require_docker_dual_network_capability() (
  local network_count egress_priority registry_priority
  local egress_network_id registry_network_id
  local expected_egress_network_id expected_registry_network_id
  local egress_driver registry_driver egress_internal registry_internal
  local egress_scope registry_scope egress_purpose registry_purpose cleanup_failed
  local preflight_name="goblins-os-network-preflight-$ARCH-$$"
  local preflight_egress_network="goblins-os-network-preflight-egress-$ARCH-$$"
  local preflight_registry_network="goblins-os-network-preflight-registry-$ARCH-$$"
  local preflight_container_id=""
  local preflight_egress_network_id=""
  local preflight_registry_network_id=""
  local preflight_container_created=0
  local preflight_egress_network_created=0
  local preflight_registry_network_created=0

  cleanup_network_preflight() {
    cleanup_failed=0
    if [ "$preflight_container_created" = "1" ]; then
      bounded_docker_remove "$preflight_container_id"
      if docker container inspect "$preflight_container_id" >/dev/null 2>&1; then
        echo "error: Docker dual-network preflight container cleanup did not complete for exact ID $preflight_container_id ($preflight_name)." >&2
        cleanup_failed=1
      else
        preflight_container_created=0
      fi
    fi
    if [ "$preflight_registry_network_created" = "1" ]; then
      bounded_docker_network_remove "$preflight_registry_network_id"
      if docker network inspect "$preflight_registry_network_id" >/dev/null 2>&1; then
        echo "error: Docker dual-network preflight registry-network cleanup did not complete for exact ID $preflight_registry_network_id ($preflight_registry_network)." >&2
        cleanup_failed=1
      else
        preflight_registry_network_created=0
      fi
    fi
    if [ "$preflight_egress_network_created" = "1" ]; then
      bounded_docker_network_remove "$preflight_egress_network_id"
      if docker network inspect "$preflight_egress_network_id" >/dev/null 2>&1; then
        echo "error: Docker dual-network preflight egress-network cleanup did not complete for exact ID $preflight_egress_network_id ($preflight_egress_network)." >&2
        cleanup_failed=1
      else
        preflight_egress_network_created=0
      fi
    fi
    [ "$cleanup_failed" -eq 0 ]
  }
  trap cleanup_network_preflight EXIT
  trap 'exit 130' HUP INT TERM

  # Reject unsupported clients and daemons before creating any Docker object.
  require_docker_dual_network_versions
  if docker container inspect "$preflight_name" >/dev/null 2>&1 \
    || docker network inspect "$preflight_egress_network" >/dev/null 2>&1 \
    || docker network inspect "$preflight_registry_network" >/dev/null 2>&1; then
    echo "error: Docker dual-network preflight found a preexisting exact-name object; refusing to remove an object this invocation did not create." >&2
    exit 1
  fi
  # Reuse the registry image already in this script's trust boundary; do not
  # introduce another mutable helper image for capability detection.
  docker pull "$LOCAL_REGISTRY_IMAGE" >/dev/null
  preflight_egress_network_id="$(
    docker network create \
      --driver bridge \
      --label org.goblins-os.purpose=installer-network-preflight-egress \
      "$preflight_egress_network"
  )"
  preflight_egress_network_created=1
  preflight_registry_network_id="$(
    docker network create \
      --driver bridge \
      --internal \
      --label org.goblins-os.purpose=installer-network-preflight-registry \
      "$preflight_registry_network"
  )"
  preflight_registry_network_created=1
  if ! preflight_container_id="$(docker create \
    --name "$preflight_name" \
    --network "name=$preflight_egress_network,gw-priority=1" \
    --network "$preflight_registry_network" \
    --entrypoint /bin/true \
    "$LOCAL_REGISTRY_IMAGE")"; then
    echo "error: Docker rejected the two-user-defined-network/gw-priority contract required by the local-registry BIB route." >&2
    exit 1
  fi
  preflight_container_created=1
  # Docker records the requested endpoint priorities at create time, but it
  # does not materialize endpoint NetworkIDs until the container starts. Run
  # the fixed no-op entrypoint so the preflight validates the same live network
  # attachment lifecycle that the registry probe and privileged BIB use.
  if ! docker start -a "$preflight_container_id" >/dev/null; then
    echo "error: Docker could not start the two-user-defined-network preflight container." >&2
    exit 1
  fi

  network_count="$(docker inspect --format '{{len .NetworkSettings.Networks}}' "$preflight_name")"
  egress_priority="$(docker inspect --format "{{with index .NetworkSettings.Networks \"$preflight_egress_network\"}}{{.GwPriority}}{{end}}" "$preflight_name")"
  registry_priority="$(docker inspect --format "{{with index .NetworkSettings.Networks \"$preflight_registry_network\"}}{{.GwPriority}}{{end}}" "$preflight_name")"
  egress_network_id="$(docker inspect --format "{{with index .NetworkSettings.Networks \"$preflight_egress_network\"}}{{.NetworkID}}{{end}}" "$preflight_name")"
  registry_network_id="$(docker inspect --format "{{with index .NetworkSettings.Networks \"$preflight_registry_network\"}}{{.NetworkID}}{{end}}" "$preflight_name")"
  expected_egress_network_id="$(docker network inspect --format '{{.Id}}' "$preflight_egress_network")"
  expected_registry_network_id="$(docker network inspect --format '{{.Id}}' "$preflight_registry_network")"
  egress_driver="$(docker network inspect --format '{{.Driver}}' "$preflight_egress_network")"
  registry_driver="$(docker network inspect --format '{{.Driver}}' "$preflight_registry_network")"
  egress_internal="$(docker network inspect --format '{{.Internal}}' "$preflight_egress_network")"
  registry_internal="$(docker network inspect --format '{{.Internal}}' "$preflight_registry_network")"
  egress_scope="$(docker network inspect --format '{{.Scope}}' "$preflight_egress_network")"
  registry_scope="$(docker network inspect --format '{{.Scope}}' "$preflight_registry_network")"
  egress_purpose="$(docker network inspect --format '{{index .Labels "org.goblins-os.purpose"}}' "$preflight_egress_network")"
  registry_purpose="$(docker network inspect --format '{{index .Labels "org.goblins-os.purpose"}}' "$preflight_registry_network")"
  if [ "$network_count" != "2" ] \
    || [ "$egress_priority" != "1" ] \
    || [ "$registry_priority" != "0" ] \
    || [ "$egress_driver" != "bridge" ] \
    || [ "$registry_driver" != "bridge" ] \
    || [ "$egress_internal" != "false" ] \
    || [ "$registry_internal" != "true" ] \
    || [ "$egress_scope" != "local" ] \
    || [ "$registry_scope" != "local" ] \
    || [ "$egress_purpose" != "installer-network-preflight-egress" ] \
    || [ "$registry_purpose" != "installer-network-preflight-registry" ] \
    || [ -z "$egress_network_id" ] \
    || [ -z "$registry_network_id" ] \
    || [ "$egress_network_id" = "$registry_network_id" ] \
    || [ "$expected_egress_network_id" != "$preflight_egress_network_id" ] \
    || [ "$expected_registry_network_id" != "$preflight_registry_network_id" ] \
    || [ "$egress_network_id" != "$expected_egress_network_id" ] \
    || [ "$registry_network_id" != "$expected_registry_network_id" ]; then
    echo "error: Docker did not preserve the exact two-user-defined-network BIB contract with non-internal egress gw-priority=1 and the isolated registry endpoint." >&2
    exit 1
  fi

  cleanup_network_preflight
  trap - EXIT HUP INT TERM
)

probe_docker_registry_from_builder_network() (
  local status second
  local network_args=("$@")

  probe_name="goblins-os-registry-probe-$ARCH-$$"
  probe_log=""
  probe_pid=""
  cleanup_registry_probe() {
    if [ -n "${probe_pid:-}" ]; then
      bounded_stop_process "$probe_pid"
    fi
    if [ -n "${probe_name:-}" ]; then
      bounded_docker_remove "$probe_name"
    fi
    if [ -n "${probe_log:-}" ]; then
      rm -f -- "$probe_log"
    fi
  }
  trap cleanup_registry_probe EXIT
  trap 'exit 130' HUP INT TERM

  bounded_docker_remove "$probe_name"
  probe_log="$(mktemp "${TMPDIR:-/tmp}/goblins-os-registry-probe.XXXXXX")"
  # Pull outside the readiness deadline so that the probe measures only the
  # exact container-DNS route that the builder will use, not registry-image
  # download latency from the Docker daemon.
  docker pull --platform "$DOCKER_PLATFORM" "$BIB" >/dev/null
  echo "==> Verifying the BIB route to http://$DOCKER_REGISTRY_NAME:5000/v2/"
  (
    docker run --rm --pull=never \
      --name "$probe_name" \
      --platform "$DOCKER_PLATFORM" \
      "${network_args[@]}" \
      -e REGISTRY_PROBE_HOST="$DOCKER_REGISTRY_NAME" \
      -e REGISTRY_PROBE_PORT=5000 \
      --entrypoint /bin/bash \
      "$BIB" \
      -lc 'set -euo pipefail
exec 3<>"/dev/tcp/$REGISTRY_PROBE_HOST/$REGISTRY_PROBE_PORT"
printf "GET /v2/ HTTP/1.1\r\nHost: %s\r\nConnection: close\r\n\r\n" "$REGISTRY_PROBE_HOST" >&3
IFS= read -r status <&3
exec 3>&-
printf -v carriage_return "\r"
status="${status%$carriage_return}"
if [ "$status" != "HTTP/1.1 200 OK" ]; then
  echo "registry readiness probe returned unexpected status: $status" >&2
  exit 1
fi' \
      >"$probe_log" 2>&1
  ) &
  probe_pid=$!

  second=0
  while [ "$second" -lt "$DOCKER_REGISTRY_PROBE_TIMEOUT_SECS" ]; do
    if ! kill -0 "$probe_pid" 2>/dev/null; then
      break
    fi
    sleep 1
    second=$((second + 1))
  done

  # Recheck after the final sleep so a one-second deadline accepts a probe that
  # completed during that second instead of reporting a false timeout.
  if kill -0 "$probe_pid" 2>/dev/null; then
    cat "$probe_log" >&2 || true
    echo "error: BIB registry route probe timed out after ${DOCKER_REGISTRY_PROBE_TIMEOUT_SECS}s on Docker network $DOCKER_REGISTRY_NETWORK." >&2
    exit 1
  fi
  status=0
  wait "$probe_pid" || status=$?
  probe_pid=""
  if [ "$status" -ne 0 ]; then
    cat "$probe_log" >&2 || true
    echo "error: BIB cannot reach the local registry through egress network $DOCKER_EGRESS_NETWORK and registry network $DOCKER_REGISTRY_NETWORK." >&2
    exit 1
  fi
  assert_dedicated_registry_network_membership true
  assert_dedicated_egress_network_membership
)

run_docker_builder() {
  local registry_image builder_image bib_pull_local bib_output_dir
  local image_arch source_route
  local bib_host_args=()
  local bib_network_args=()

  require_command docker
  if [ -n "$BIB_SOURCE_IMAGE_OVERRIDE" ]; then
    builder_image="$BIB_SOURCE_IMAGE_OVERRIDE"
    source_route="$(classify_bib_source_route "$builder_image")"
  else
    builder_image="$DOCKER_REGISTRY_NAME:5000/goblins-os:$ARCH"
    source_route="managed-registry"
  fi
  case "$source_route" in
    invalid)
      echo "error: GOBLINS_OS_BIB_SOURCE_IMAGE must be one nonempty container image reference without whitespace or leading options." >&2
      exit 1
      ;;
    container-loopback)
      echo "error: GOBLINS_OS_BIB_SOURCE_IMAGE=$builder_image uses container loopback and cannot reach a host registry from BIB." >&2
      echo "       Use host.docker.internal:<port>/<image> for an explicit host registry, $DOCKER_REGISTRY_NAME:5000/<image> for the managed registry, or a fully qualified remote registry." >&2
      exit 1
      ;;
    unsupported-local)
      echo "error: GOBLINS_OS_BIB_SOURCE_IMAGE=$builder_image uses an unsupported local registry alias." >&2
      echo "       Supported local routes are host.docker.internal:<port>/<image> and $DOCKER_REGISTRY_NAME:5000/<image>; otherwise use a fully qualified remote registry." >&2
      exit 1
      ;;
  esac
  if [ "$SKIP_LOCAL_IMAGE_BUILD" = "1" ] && [ -z "$BIB_SOURCE_IMAGE_OVERRIDE" ]; then
    echo "error: GOBLINS_OS_SKIP_LOCAL_IMAGE_BUILD=1 requires GOBLINS_OS_BIB_SOURCE_IMAGE to a pullable image reference." >&2
    exit 1
  fi
  require_shippable_source_ref "$builder_image"
  case "$source_route" in
    managed-registry)
      # Validate Docker's exact dual-network contract before an expensive bootc
      # image build or any persistent registry/network object is created.
      require_docker_dual_network_capability
      bib_network_args=(
        --network "name=$DOCKER_EGRESS_NETWORK,gw-priority=1"
        --network "$DOCKER_REGISTRY_NETWORK"
      )
      ;;
    host-gateway)
      # host-gateway is intentionally available only for this explicit override.
      bib_host_args=(--add-host=host.docker.internal:host-gateway)
      ;;
    release-registry)
      # Public remote images intentionally use Docker's normal network only;
      # they create and attach neither managed local network.
      ;;
  esac
  verify_docker_emulation_runtime
  if [ "$SKIP_LOCAL_IMAGE_BUILD" = "1" ]; then
    echo "==> Skipping local Docker image build; bootc-image-builder will pull $BIB_SOURCE_IMAGE_OVERRIDE"
  else
    image_arch="$(docker image inspect --format '{{.Architecture}}' "$IMAGE" 2>/dev/null || true)"
    if [ "$(normalize_arch "$image_arch")" != "$ARCH" ]; then
      if [ -n "$image_arch" ]; then
        echo "==> Rebuilding $IMAGE for $ARCH; existing image architecture is $image_arch"
      else
        echo "==> Building $IMAGE from os/bootc/Containerfile with Docker"
      fi
      DOCKER_BUILDKIT=1 docker build --platform "$DOCKER_PLATFORM" -t "$IMAGE" -f "$REPO_ROOT/os/bootc/Containerfile" "$REPO_ROOT"
    fi
  fi

  if [ -n "$BIB_SOURCE_IMAGE_OVERRIDE" ]; then
    registry_image=""
    case "$source_route" in
      managed-registry)
        ensure_docker_registry
        probe_docker_registry_from_builder_network "${bib_network_args[@]}"
        bib_pull_local=1
        BIB_SOURCE_KIND="explicit-local-registry"
        BIB_SOURCE_LOCAL_ONLY="true"
        ;;
      host-gateway)
        bib_pull_local=1
        BIB_SOURCE_KIND="explicit-local-registry"
        BIB_SOURCE_LOCAL_ONLY="true"
        ;;
      release-registry)
        bib_pull_local=0
        BIB_SOURCE_KIND="release-registry"
        BIB_SOURCE_LOCAL_ONLY="false"
        ;;
      *)
        echo "error: unsupported internal BIB source route: $source_route" >&2
        exit 1
        ;;
    esac
  else
    ensure_docker_registry
    # Docker's local-registry exception uses localhost. Publish on both IPv4
    # and IPv6 loopback so resolver order cannot miss the fail-closed binding.
    registry_image="localhost:$DOCKER_REGISTRY_PORT/goblins-os:$ARCH"
    echo "==> Publishing $IMAGE to local Docker registry as $registry_image"
    docker tag "$IMAGE" "$registry_image"
    docker push "$registry_image"
    probe_docker_registry_from_builder_network "${bib_network_args[@]}"
    bib_pull_local=1
    BIB_SOURCE_KIND="docker-local-registry"
    BIB_SOURCE_LOCAL_ONLY="true"
  fi
  require_shippable_tool_ref BIB_IMAGE "$BIB"
  require_shippable_branding_tool_ref
  BIB_SOURCE_IMAGE_USED="$builder_image"

  bib_output_dir="$(mktemp -d "$OUTDIR/.bib-output.XXXXXX")"
  docker volume create "$BIB_STORAGE_VOLUME" >/dev/null
  echo "==> Building Goblins OS $ARCH install ISO ($ROOTFS root) from $builder_image"
  echo "==> Docker builder platform: $DOCKER_PLATFORM"
  if [ -n "$registry_image" ]; then
    echo "==> Using Docker local registry: $registry_image"
  elif [ "$BIB_SOURCE_LOCAL_ONLY" = "true" ]; then
    echo "==> Using explicit local/test registry source: $builder_image"
  else
    echo "==> Using release registry source: $builder_image"
  fi
  # Optional only for non-release private-registry testing. Shippable release
  # media must pull the public release package anonymously; never expose a
  # registry token to this privileged third-party builder container.
  local bib_auth_mounts=()
  if [ -n "${GOBLINS_OS_BIB_AUTH_FILE:-}" ]; then
    if [ "$SHIPPABLE_RELEASE" = "1" ]; then
      echo "error: shippable release media forbids GOBLINS_OS_BIB_AUTH_FILE; publish the release image for anonymous digest pulls" >&2
      exit 1
    fi
    echo "==> Using registry auth file for the bootc-image-builder source pull"
    bib_auth_mounts=(-v "${GOBLINS_OS_BIB_AUTH_FILE}:/run/containers/0/auth.json:ro" -e "REGISTRY_AUTH_FILE=/run/containers/0/auth.json")
  fi
  if [ "$source_route" = "managed-registry" ]; then
    # Recheck immediately before the privileged builder attaches; setup between
    # the readiness probe and this point must not widen either network boundary.
    assert_dedicated_registry_network_membership true
    assert_dedicated_egress_network_membership
  fi
  docker run --rm --privileged --pull=missing \
    --platform "$DOCKER_PLATFORM" \
    ${bib_host_args[@]+"${bib_host_args[@]}"} \
    ${bib_network_args[@]+"${bib_network_args[@]}"} \
    ${bib_auth_mounts[@]+"${bib_auth_mounts[@]}"} \
    -e BIB_SOURCE_IMAGE="$builder_image" \
    -e BIB_PULL_LOCAL="$bib_pull_local" \
    -e BIB_ROOTFS="$ROOTFS" \
    -v "$CONFIG":/config.toml:ro \
    -v "$bib_output_dir":/output \
    -v "$BIB_STORAGE_VOLUME":/var/lib/containers/storage \
    --entrypoint /bin/bash \
    "$BIB" \
    -lc 'set -euo pipefail; mkdir -p /var/lib/containers/storage/overlay; if [ "$BIB_PULL_LOCAL" = "1" ]; then podman pull --tls-verify=false "$BIB_SOURCE_IMAGE"; else podman pull "$BIB_SOURCE_IMAGE"; fi; bootc-image-builder --verbose build --type anaconda-iso --rootfs "$BIB_ROOTFS" --output /output "$BIB_SOURCE_IMAGE"'

  if [ "$source_route" = "managed-registry" ]; then
    # --rm must restore both managed networks to their expected idle state:
    # only the registry remains on the internal bridge, and egress is empty.
    assert_dedicated_registry_network_membership true
    assert_dedicated_egress_network_membership
  fi

  # The privileged builder writes /output as root. Reuse the same reviewed,
  # digest-pinned image without privileges to reclaim ownership; introducing a
  # second mutable helper image would expand the release trust boundary.
  docker run --rm --pull=missing \
    --platform "$DOCKER_PLATFORM" \
    -v "$bib_output_dir":/output \
    --entrypoint /bin/chown \
    "$BIB" \
    -R "$(id -u):$(id -g)" /output

  finalize_outputs \
    "$bib_output_dir/bootiso/install.iso" \
    "$bib_output_dir/manifest-anaconda-iso.json"
  case "$bib_output_dir" in
    "$OUTDIR"/.bib-output.*) rm -rf -- "$bib_output_dir" ;;
    *) echo "error: refusing to remove unexpected builder output path: $bib_output_dir" >&2; exit 1 ;;
  esac
}

run_docker_builder
