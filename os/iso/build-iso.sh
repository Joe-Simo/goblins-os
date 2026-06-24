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
#   OUTDIR             output directory           (default os/iso/output/<arch>)
#   BIB_IMAGE          bootc-image-builder image  (default the quay.io latest)
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
#   GOBLINS_OS_BIB_STORAGE_VOLUME
#                      Docker volume for bootc-image-builder storage
#   GOBLINS_OS_BIB_SOURCE_IMAGE
#                      source image passed to bootc-image-builder. If omitted,
#                      Docker local testing uses host.docker.internal:<port>.
#                      Shippable release media must use a real pullable registry
#                      ref, because Anaconda ISO installs track this ref for
#                      post-install bootc updates.
#   GOBLINS_OS_SHIPPABLE_RELEASE
#                      set 1 to fail if the BIB source image is local/test-only
set -euo pipefail

REPO_ROOT="$(cd "$(dirname "$0")/../.." && pwd)"
CONFIG="$REPO_ROOT/os/iso/config.toml"
BIB="${BIB_IMAGE:-quay.io/centos-bootc/bootc-image-builder:latest}"
ROOTFS="${GOBLINS_OS_ROOTFS:-xfs}"
CONTAINER_RUNTIME="${GOBLINS_OS_CONTAINER_RUNTIME:-docker}"
ALLOW_EMULATED_DOCKER="${GOBLINS_OS_ALLOW_EMULATED_DOCKER:-0}"
DOCKER_REGISTRY_PORT="${GOBLINS_OS_DOCKER_REGISTRY_PORT:-5002}"
DOCKER_REGISTRY_NAME="${GOBLINS_OS_DOCKER_REGISTRY_NAME:-goblins-os-registry}"
BIB_STORAGE_VOLUME="${GOBLINS_OS_BIB_STORAGE_VOLUME:-goblins-os-bib-storage-$DOCKER_REGISTRY_PORT}"
BIB_SOURCE_IMAGE_OVERRIDE="${GOBLINS_OS_BIB_SOURCE_IMAGE:-}"
SHIPPABLE_RELEASE="${GOBLINS_OS_SHIPPABLE_RELEASE:-0}"
BIB_SOURCE_IMAGE_USED=""
BIB_SOURCE_KIND=""
BIB_SOURCE_LOCAL_ONLY="false"
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

image_ref_is_local_only() {
  case "$1" in
    localhost/*|localhost:*|127.*|0.0.0.0:*|host.docker.internal:*|goblins-os:*|docker.io/library/goblins-os:*)
      return 0
      ;;
    *)
      return 1
      ;;
  esac
}

require_shippable_source_ref() {
  local ref="$1"

  if [ "$SHIPPABLE_RELEASE" != "1" ]; then
    return 0
  fi
  if image_ref_is_local_only "$ref"; then
    echo "error: shippable release media cannot track local/test-only installer payload ref: $ref" >&2
    echo "       Push the bootc image to a real release registry and set GOBLINS_OS_BIB_SOURCE_IMAGE to that pullable ref." >&2
    exit 1
  fi
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
DOCKER_PLATFORM="${GOBLINS_OS_DOCKER_PLATFORM:-$(docker_platform_for_arch "$ARCH")}"
if [ "$(arch_for_docker_platform "$DOCKER_PLATFORM")" != "$ARCH" ]; then
  echo "error: GOBLINS_OS_DOCKER_PLATFORM='$DOCKER_PLATFORM' does not match GOBLINS_OS_ARCH='$ARCH'." >&2
  exit 1
fi

IMAGE="${GOBLINS_OS_IMAGE:-localhost/goblins-os:$ARCH}"
OUTDIR="${OUTDIR:-$REPO_ROOT/os/iso/output/$ARCH}"
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
    echo "==> Skipping Anaconda installer branding (GOBLINS_OS_SKIP_INSTALLER_BRANDING=1)"
    return 0
  fi
  dir="$(cd "$(dirname "$iso")" && pwd)"
  base="$(basename "$iso")"
  echo "==> Branding the Anaconda installer (Goblins sidebar/logo/accent)"
  docker run --rm \
    -v "$REPO_ROOT/os/brand/anaconda":/brand:ro \
    -v "$REPO_ROOT/os/iso":/scripts:ro \
    -v "$dir":/iso:ro \
    -v "$dir":/work \
    -e ISO_IN="/iso/$base" \
    -e ISO_OUT="/work/$base.branded" \
    docker.io/library/fedora:44 bash /scripts/remaster-anaconda-branding.sh
  # The remaster container writes the branded ISO as root; reclaim ownership (a
  # throwaway container, no host sudo) before swapping it in.
  docker run --rm -v "$dir":/work docker.io/library/alpine:latest \
    chown -R "$(id -u):$(id -g)" /work 2>/dev/null || true
  mv -f "$iso.branded" "$iso"
}

finalize_outputs() {
  local latest_iso

  latest_iso="$(find "$OUTDIR/bootiso" -maxdepth 1 -type f -name '*.iso' -print | sort | tail -n 1 || true)"
  if [ -z "$latest_iso" ]; then
    echo "error: bootc-image-builder completed but no ISO was found under $OUTDIR/bootiso" >&2
    exit 1
  fi
  if [ "$latest_iso" != "$ISO_PATH" ]; then
    cp "$latest_iso" "$ISO_PATH"
  fi
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
  "image": "$IMAGE",
  "container_runtime": "$CONTAINER_RUNTIME",
  "rootfs": "$ROOTFS",
  "iso": "bootiso/$ISO_NAME",
  "sha256_file": "bootiso/$ISO_NAME.sha256",
  "built_on": "$(date -u +%Y-%m-%dT%H:%M:%SZ)",
  "native_host_arch": "$HOST_ARCH",
  "container_engine_arch": "$RUNTIME_ARCH",
  "docker_platform": "$DOCKER_PLATFORM",
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

ensure_docker_registry() {
  if docker container inspect "$DOCKER_REGISTRY_NAME" >/dev/null 2>&1; then
    if [ "$(docker inspect -f '{{.State.Running}}' "$DOCKER_REGISTRY_NAME")" != "true" ]; then
      docker start "$DOCKER_REGISTRY_NAME" >/dev/null
    fi
  else
    docker run -d \
      --name "$DOCKER_REGISTRY_NAME" \
      -p "127.0.0.1:$DOCKER_REGISTRY_PORT:5000" \
      registry:2 >/dev/null
  fi
}

run_docker_builder() {
  local registry_image builder_image bib_pull_local
  local image_arch

  require_command docker
  verify_docker_emulation_runtime
  image_arch="$(docker image inspect --format '{{.Architecture}}' "$IMAGE" 2>/dev/null || true)"
  if [ "$(normalize_arch "$image_arch")" != "$ARCH" ]; then
    if [ -n "$image_arch" ]; then
      echo "==> Rebuilding $IMAGE for $ARCH; existing image architecture is $image_arch"
    else
      echo "==> Building $IMAGE from os/bootc/Containerfile with Docker"
    fi
    DOCKER_BUILDKIT=1 docker build --platform "$DOCKER_PLATFORM" -t "$IMAGE" -f "$REPO_ROOT/os/bootc/Containerfile" "$REPO_ROOT"
  fi

  if [ -n "$BIB_SOURCE_IMAGE_OVERRIDE" ]; then
    builder_image="$BIB_SOURCE_IMAGE_OVERRIDE"
    registry_image=""
    bib_pull_local=0
    BIB_SOURCE_KIND="release-registry"
    BIB_SOURCE_LOCAL_ONLY="false"
  else
    ensure_docker_registry
    registry_image="localhost:$DOCKER_REGISTRY_PORT/goblins-os:$ARCH"
    builder_image="host.docker.internal:$DOCKER_REGISTRY_PORT/goblins-os:$ARCH"
    echo "==> Publishing $IMAGE to local Docker registry as $registry_image"
    docker tag "$IMAGE" "$registry_image"
    docker push "$registry_image"
    bib_pull_local=1
    BIB_SOURCE_KIND="docker-local-registry"
    BIB_SOURCE_LOCAL_ONLY="true"
  fi
  require_shippable_source_ref "$builder_image"
  BIB_SOURCE_IMAGE_USED="$builder_image"

  mkdir -p "$OUTDIR"
  docker volume create "$BIB_STORAGE_VOLUME" >/dev/null
  echo "==> Building Goblins OS $ARCH install ISO ($ROOTFS root) from $builder_image"
  echo "==> Docker builder platform: $DOCKER_PLATFORM"
  if [ -n "$registry_image" ]; then
    echo "==> Using Docker local registry: $registry_image"
  else
    echo "==> Using release registry source: $builder_image"
  fi
  # Optional: a registry auth file so bootc-image-builder's podman can pull a
  # private release registry source image (e.g. a private GHCR package). Docker's
  # ~/.docker/config.json is a valid REGISTRY_AUTH_FILE for podman.
  local bib_auth_mounts=()
  if [ -n "${GOBLINS_OS_BIB_AUTH_FILE:-}" ]; then
    echo "==> Using registry auth file for the bootc-image-builder source pull"
    bib_auth_mounts=(-v "${GOBLINS_OS_BIB_AUTH_FILE}:/run/containers/0/auth.json:ro" -e "REGISTRY_AUTH_FILE=/run/containers/0/auth.json")
  fi
  docker run --rm --privileged --pull=missing \
    --platform "$DOCKER_PLATFORM" \
    --add-host=host.docker.internal:host-gateway \
    ${bib_auth_mounts[@]+"${bib_auth_mounts[@]}"} \
    -e BIB_SOURCE_IMAGE="$builder_image" \
    -e BIB_PULL_LOCAL="$bib_pull_local" \
    -e BIB_ROOTFS="$ROOTFS" \
    -v "$CONFIG":/config.toml:ro \
    -v "$OUTDIR":/output \
    -v "$BIB_STORAGE_VOLUME":/var/lib/containers/storage \
    --entrypoint /bin/bash \
    "$BIB" \
    -lc 'set -euo pipefail; mkdir -p /var/lib/containers/storage/overlay; if [ "$BIB_PULL_LOCAL" = "1" ]; then podman pull --tls-verify=false "$BIB_SOURCE_IMAGE"; else podman pull "$BIB_SOURCE_IMAGE"; fi; bootc-image-builder --verbose build --type anaconda-iso --rootfs "$BIB_ROOTFS" --output /output "$BIB_SOURCE_IMAGE"'

  # The privileged bootc-image-builder container writes /output as root. Reclaim
  # ownership via a throwaway container (no host sudo needed; a harmless no-op on
  # Docker Desktop) so the host user can rename the ISO and write the
  # sha/manifest in finalize_outputs.
  docker run --rm -v "$OUTDIR":/output docker.io/library/alpine:latest \
    chown -R "$(id -u):$(id -g)" /output 2>/dev/null || true

  finalize_outputs
}

run_docker_builder
