#!/usr/bin/env bash
# Download published Goblins OS release metadata into the local gate layout.
#
# Default mode is bandwidth-conscious: manifests and SBOM evidence only. Set
# GOBLINS_OS_DOWNLOAD_ISO=1 to download split ISO parts, verify them, rebuild the
# compressed ISO, decompress the final ISO, and verify the final SHA256.
set -euo pipefail

REPO_ROOT="$(cd "$(dirname "$0")/../.." && pwd -P)"
cd "$REPO_ROOT"
. "$REPO_ROOT/os/hardware-gate/release-evidence.sh"
. "$REPO_ROOT/os/hardware-gate/rpm-sbom-arch.sh"

TAG="${GOBLINS_OS_RELEASE_TAG:-v0.1.0-alpha.20260703}"
BASE_URL="${GOBLINS_OS_RELEASE_BASE_URL:-https://github.com/Joe-Simo/goblins-os/releases/download/$TAG}"
DOWNLOAD_ISO="${GOBLINS_OS_DOWNLOAD_ISO:-0}"
FORCE="${GOBLINS_OS_FORCE_DOWNLOAD:-0}"

normalize_arch() {
  case "$1" in
    aarch64 | arm64) echo "aarch64" ;;
    x86_64 | amd64) echo "x86_64" ;;
    *) echo "unsupported" ;;
  esac
}

require_command() {
  if ! command -v "$1" >/dev/null 2>&1; then
    echo "error: missing required command: $1" >&2
    exit 1
  fi
}

sha256_check() {
  local dir="$1"
  local sha_file="$2"

  if command -v sha256sum >/dev/null 2>&1; then
    (cd "$dir" && sha256sum -c "$sha_file")
  elif command -v shasum >/dev/null 2>&1; then
    (cd "$dir" && shasum -a 256 -c "$sha_file")
  else
    echo "error: no sha256sum or shasum command available." >&2
    exit 1
  fi
}

normalize_sha256_file_paths() {
  local path="$1"
  local tmp_path="$path.tmp"
  local sum file

  while read -r sum file; do
    [ -n "${sum:-}" ] || continue
    file="${file#\*}"
    printf '%s  %s\n' "$sum" "$(basename "$file")"
  done <"$path" >"$tmp_path"
  mv "$tmp_path" "$path"
}

download_asset() {
  local asset="$1"
  local dest="$2"
  local url="$BASE_URL/$asset"

  mkdir -p "$(dirname "$dest")"
  if [ "$FORCE" = "1" ]; then
    rm -f "$dest"
  fi
  if [ -s "$dest" ]; then
    echo "==> present: $dest"
    return 0
  fi
  echo "==> download: $asset"
  curl -fL --retry 5 --retry-delay 2 --continue-at - -o "$dest" "$url"
}

hydrate_metadata() {
  local arch="$1"
  local iso_dir="os/iso/output/$arch"
  local boot_dir="$iso_dir/bootiso"
  local sbom_dir="os/signoff-proofs/sbom/$arch"
  local staging_dir resolved_sbom_dir generated_path

  download_asset "goblins-os-$arch.iso.sha256" "$boot_dir/goblins-os-$arch.iso.sha256"
  normalize_sha256_file_paths "$boot_dir/goblins-os-$arch.iso.sha256"
  download_asset "manifest-goblins-os-$arch.json" "$iso_dir/manifest-goblins-os-$arch.json"
  download_asset "manifest-anaconda-iso-$arch.json" "$iso_dir/manifest-anaconda-iso.json"

  for component in os os/signoff-proofs os/signoff-proofs/sbom "$sbom_dir"; do
    [ ! -L "$component" ] || {
      echo "error: refusing symlinked release-evidence path: $component" >&2
      exit 1
    }
  done
  mkdir -p "$sbom_dir"
  resolved_sbom_dir="$(cd "$sbom_dir" && pwd -P)"
  [ "$resolved_sbom_dir" = "$REPO_ROOT/$sbom_dir" ] || {
    echo "error: release-evidence destination escaped the checkout: $resolved_sbom_dir" >&2
    exit 1
  }

  staging_dir="$(mktemp -d "${TMPDIR:-/tmp}/goblins-release-evidence-$arch.XXXXXX")"
  download_asset "cargo-lock-packages-$arch.tsv" "$staging_dir/cargo-lock-packages.tsv"
  download_asset "rpm-packages-$arch.command" "$staging_dir/rpm-packages.command"
  download_asset "rpm-packages-$arch.tsv" "$staging_dir/rpm-packages.tsv"
  download_asset "release-evidence-manifest-$arch.json" "$staging_dir/release-evidence-manifest.json"
  if ! goblins_os_release_evidence_hashes_match "$staging_dir"; then
    echo "error: downloaded $arch release evidence is incomplete or hash-mismatched" >&2
    exit 1
  fi
  if ! rpm_sbom_arch_matches "$staging_dir/rpm-packages.tsv" "$arch"; then
    echo "error: downloaded $arch RPM inventory contains the wrong architecture" >&2
    exit 1
  fi

  for generated_name in release-evidence-manifest.json cargo-lock-packages.tsv rpm-packages.command rpm-packages.tsv rpm-packages.not-generated.txt; do
    generated_path="$sbom_dir/$generated_name"
    [ ! -L "$generated_path" ] || {
      echo "error: refusing symlinked release-evidence file: $generated_path" >&2
      exit 1
    }
    rm -f "$generated_path"
  done
  cp "$staging_dir/cargo-lock-packages.tsv" "$sbom_dir/cargo-lock-packages.tsv"
  cp "$staging_dir/rpm-packages.command" "$sbom_dir/rpm-packages.command"
  cp "$staging_dir/rpm-packages.tsv" "$sbom_dir/rpm-packages.tsv"
  cp "$staging_dir/release-evidence-manifest.json" "$sbom_dir/release-evidence-manifest.json"
  rm -rf "$staging_dir"
  goblins_os_release_evidence_hashes_match "$sbom_dir" || {
    echo "error: hydrated $arch release evidence failed destination validation" >&2
    exit 1
  }
}

hydrate_iso() {
  local arch="$1"
  local boot_dir="os/iso/output/$arch/bootiso"
  local parts_sha="goblins-os-$arch.iso.zst.parts.sha256"
  local zst_sha="goblins-os-$arch.iso.zst.sha256"
  local zst_name="goblins-os-$arch.iso.zst"
  local iso_name="goblins-os-$arch.iso"
  local part_ref part_name

  require_command zstd
  download_asset "$parts_sha" "$boot_dir/$parts_sha"
  download_asset "$zst_sha" "$boot_dir/$zst_sha"
  normalize_sha256_file_paths "$boot_dir/$parts_sha"
  normalize_sha256_file_paths "$boot_dir/$zst_sha"

  while read -r _ part_ref; do
    [ -n "${part_ref:-}" ] || continue
    part_name="$(basename "${part_ref#\*}")"
    download_asset "$part_name" "$boot_dir/$part_name"
  done <"$boot_dir/$parts_sha"

  sha256_check "$boot_dir" "$parts_sha"
  rm -f "$boot_dir/$zst_name"
  while read -r _ part_ref; do
    [ -n "${part_ref:-}" ] || continue
    part_name="$(basename "${part_ref#\*}")"
    cat "$boot_dir/$part_name" >>"$boot_dir/$zst_name"
  done <"$boot_dir/$parts_sha"

  sha256_check "$boot_dir" "$zst_sha"
  zstd -d --long=31 -f -o "$boot_dir/$iso_name" "$boot_dir/$zst_name"
  sha256_check "$boot_dir" "$iso_name.sha256"
}

require_command curl

if [ -n "${GOBLINS_OS_ARCH:-}" ]; then
  ARCHES="$(normalize_arch "$GOBLINS_OS_ARCH")"
else
  ARCHES="aarch64 x86_64"
fi

for arch in $ARCHES; do
  if [ "$arch" = "unsupported" ]; then
    echo "error: unsupported architecture '${GOBLINS_OS_ARCH:-}'" >&2
    exit 1
  fi
  echo "==> Hydrating $arch release artifacts from $TAG"
  hydrate_metadata "$arch"
  if [ "$DOWNLOAD_ISO" = "1" ]; then
    hydrate_iso "$arch"
  else
    echo "==> Skipping split ISO download for $arch (set GOBLINS_OS_DOWNLOAD_ISO=1 to materialize it)"
  fi
done
