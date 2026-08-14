#!/usr/bin/env bash
# Hydrate immutable Goblins OS release artifacts without confusing historical
# release assets with the active exact-candidate proof set.
set -euo pipefail

REPO_ROOT="$(cd "$(dirname "$0")/../.." && pwd -P)"
cd "$REPO_ROOT"
. "$REPO_ROOT/os/hardware-gate/release-evidence.sh"
. "$REPO_ROOT/os/hardware-gate/rpm-sbom-arch.sh"
. "$REPO_ROOT/os/iso/manifest-provenance.sh"

MODE="${GOBLINS_OS_HYDRATION_MODE:-}"
ARCH="aarch64"
DOWNLOAD_ISO="${GOBLINS_OS_DOWNLOAD_ISO:-0}"
CLEANUP_DIR=""

case "${GOBLINS_OS_ARCH:-aarch64}" in
  aarch64 | arm64) ;;
  *) echo "error: release hydration supports only aarch64" >&2; exit 2 ;;
esac

cleanup() {
  case "$CLEANUP_DIR" in
    "${TMPDIR:-/tmp}"/goblins-historical-alpha.* | "${TMPDIR:-/tmp}"/goblins-exact-candidate.*)
      rm -rf -- "$CLEANUP_DIR"
      ;;
  esac
}
trap cleanup EXIT

usage() {
  cat >&2 <<'EOF'
Usage:
  GOBLINS_OS_HYDRATION_MODE=historical-alpha \
  GOBLINS_OS_RELEASE_TAG=<explicit-alpha-tag> \
    os/release/hydrate-release-artifacts.sh

  GOBLINS_OS_HYDRATION_MODE=exact-candidate \
  GOBLINS_OS_CANDIDATE_COMMIT=<40-lowercase-hex> \
  GOBLINS_OS_CANDIDATE_IMAGE_REF=<ghcr.io/...@sha256:...> \
  GOBLINS_OS_CANDIDATE_WORKFLOW_RUN=<canonical-actions-run-url> \
  GOBLINS_OS_CANDIDATE_WORKFLOW_RUN_ATTEMPT=<positive-integer> \
    os/release/hydrate-release-artifacts.sh

historical-alpha writes only below os/release/historical-alpha/<tag>/ and can
never satisfy current signoff. exact-candidate downloads and validates the
single commit-scoped GitHub Actions artifact before replacing active artifacts.
EOF
  exit 2
}

require_command() {
  command -v "$1" >/dev/null 2>&1 || {
    echo "error: missing required command: $1" >&2
    exit 1
  }
}

sha256_file() {
  if command -v sha256sum >/dev/null 2>&1; then
    sha256sum "$1" | awk '{print $1}'
  elif command -v shasum >/dev/null 2>&1; then
    shasum -a 256 "$1" | awk '{print $1}'
  else
    echo "error: no sha256sum or shasum command available" >&2
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
    echo "error: no sha256sum or shasum command available" >&2
    exit 1
  fi
}

normalize_sha256_file_paths() {
  local path="$1"
  local tmp_path="$path.normalized"
  local sum file lower_sum

  : >"$tmp_path"
  while read -r sum file; do
    [ -n "${sum:-}" ] || continue
    [[ "$sum" =~ ^[0-9a-fA-F]{64}$ ]] || {
      echo "error: invalid SHA256 record in $path" >&2
      exit 1
    }
    file="${file#\*}"
    [ "$(basename "$file")" = "$file" ] || {
      echo "error: checksum record contains a path: $file" >&2
      exit 1
    }
    lower_sum="$(printf '%s' "$sum" | tr '[:upper:]' '[:lower:]')"
    printf '%s  %s\n' "$lower_sum" "$file" >>"$tmp_path"
  done <"$path"
  [ -s "$tmp_path" ] || {
    echo "error: checksum file is empty: $path" >&2
    exit 1
  }
  mv -f "$tmp_path" "$path"
}

assert_destination_is_safe() {
  local destination="$1"
  local relative parent relative_parent resolved component current
  local -a components=()

  case "$destination" in
    "$REPO_ROOT"/*) relative="${destination#"$REPO_ROOT"/}" ;;
    *) echo "error: destination escapes the checkout: $destination" >&2; exit 1 ;;
  esac
  case "$relative" in
    os/release/historical-alpha/* | os/iso/output/aarch64/* | os/signoff-proofs/sbom/aarch64/* | os/signoff-proofs/candidate/aarch64/*) ;;
    *) echo "error: destination is outside an approved hydration root: $relative" >&2; exit 1 ;;
  esac
  parent="$(dirname "$destination")"
  relative_parent="${parent#"$REPO_ROOT"/}"
  current="$REPO_ROOT"
  IFS='/' read -r -a components <<<"$relative_parent"
  for component in "${components[@]}"; do
    current="$current/$component"
    if [ -e "$current" ]; then
      [ -d "$current" ] && [ ! -L "$current" ] || {
        echo "error: refusing unsafe hydration path component: $current" >&2
        exit 1
      }
    else
      mkdir "$current"
    fi
  done
  [ ! -L "$destination" ] || {
    echo "error: refusing symlinked hydration destination: $destination" >&2
    exit 1
  }
  resolved="$(cd "$parent" && pwd -P)"
  case "$resolved/" in
    "$REPO_ROOT"/*/) ;;
    *) echo "error: hydration destination parent escaped the checkout: $resolved" >&2; exit 1 ;;
  esac
}

install_validated_file() {
  local source="$1"
  local destination="$2"
  local temporary

  [ -s "$source" ] && [ -f "$source" ] && [ ! -L "$source" ] || {
    echo "error: validated source is not a regular non-empty file: $source" >&2
    exit 1
  }
  assert_destination_is_safe "$destination"
  temporary="$(mktemp "$(dirname "$destination")/.hydrate.$(basename "$destination").XXXXXX")"
  cp -- "$source" "$temporary"
  chmod 0644 "$temporary"
  mv -f -- "$temporary" "$destination"
}

download_release_asset() {
  local base_url="$1"
  local asset="$2"
  local destination="$3"

  echo "==> download historical asset: $asset"
  curl --fail --location --proto '=https' --proto-redir '=https' --tlsv1.2 \
    --retry 5 --retry-delay 2 \
    --output "$destination" "$base_url/$asset"
  [ -s "$destination" ] || {
    echo "error: downloaded historical asset is empty: $asset" >&2
    exit 1
  }
}

validate_json_envelopes() {
  local root="$1"
  local expected_commit="${2:-}"
  local expected_image="${3:-}"
  local expected_run="${4:-}"
  local expected_attempt="${5:-}"

  python3 - "$root" "$ARCH" "$expected_commit" "$expected_image" "$expected_run" "$expected_attempt" <<'PY'
import hashlib
import json
from pathlib import Path
import re
import sys

root, architecture, commit, image_ref, workflow_run, workflow_attempt = sys.argv[1:7]
root = Path(root)

def reject_duplicate(pairs):
    output = {}
    for key, value in pairs:
        if key in output:
            raise ValueError(f"duplicate JSON key: {key}")
        output[key] = value
    return output

def load(relative):
    path = root / relative
    metadata = path.lstat()
    if path.is_symlink() or not path.is_file() or metadata.st_size <= 0 or metadata.st_size > 16 * 1024 * 1024:
        raise SystemExit(f"unsafe JSON artifact: {path}")
    try:
        return json.loads(path.read_text(encoding="utf-8"), object_pairs_hook=reject_duplicate)
    except (UnicodeError, json.JSONDecodeError, ValueError) as error:
        raise SystemExit(f"invalid JSON artifact {path}: {error}") from error

def sha(relative):
    digest = hashlib.sha256()
    with (root / relative).open("rb") as handle:
        for chunk in iter(lambda: handle.read(1024 * 1024), b""):
            digest.update(chunk)
    return digest.hexdigest()

iso_relative = f"os/iso/output/{architecture}/bootiso/goblins-os-{architecture}.iso"
checksum_relative = f"{iso_relative}.sha256"
manifest_relative = f"os/iso/output/{architecture}/manifest-goblins-os-{architecture}.json"
bib_relative = f"os/iso/output/{architecture}/manifest-anaconda-iso.json"
evidence_root = f"os/signoff-proofs/sbom/{architecture}"
evidence_relative = f"{evidence_root}/release-evidence-manifest.json"
cargo_relative = f"{evidence_root}/cargo-lock-packages.tsv"
rpm_command_relative = f"{evidence_root}/rpm-packages.command"
rpm_relative = f"{evidence_root}/rpm-packages.tsv"

manifest = load(manifest_relative)
evidence = load(evidence_relative)
if manifest.get("architecture") != architecture or evidence.get("architecture") != architecture:
    raise SystemExit("downloaded manifests are not aarch64")
if not commit:
    if evidence.get("schema") != "goblins-os-release-evidence-v4":
        raise SystemExit("historical-alpha release evidence does not use the archived v4 schema")
    if "rpm_command_sha256" in evidence:
        raise SystemExit("historical-alpha v4 release evidence has noncanonical command-hash fields")
    historical_commit = manifest.get("candidate_commit")
    historical_image = manifest.get("builder_source_image")
    if not isinstance(historical_commit, str) or re.fullmatch(r"[0-9a-f]{40}", historical_commit) is None:
        raise SystemExit("historical ISO manifest has no exact candidate commit")
    if not isinstance(historical_image, str) or re.fullmatch(r"[^\s]+@sha256:[0-9a-f]{64}", historical_image) is None:
        raise SystemExit("historical ISO manifest has no immutable image")
    if manifest.get("image") != historical_image:
        raise SystemExit("historical ISO manifest changes image identity")
    if manifest.get("iso") != f"bootiso/goblins-os-{architecture}.iso" or manifest.get("sha256_file") != f"bootiso/goblins-os-{architecture}.iso.sha256":
        raise SystemExit("historical ISO manifest has non-canonical artifact paths")
    if evidence.get("candidate_commit") != historical_commit or evidence.get("image_ref") != historical_image:
        raise SystemExit("historical ISO and release evidence identities differ")
    historical_checksum = (root / checksum_relative).read_text(encoding="utf-8").splitlines()
    if len(historical_checksum) != 1 or re.fullmatch(
        rf"[0-9a-f]{{64}}  goblins-os-{re.escape(architecture)}\.iso",
        historical_checksum[0],
    ) is None:
        raise SystemExit("historical ISO checksum does not name the canonical artifact")
    raise SystemExit(0)

if evidence.get("schema") != "goblins-os-release-evidence-v5":
    raise SystemExit("exact-candidate release evidence does not use the required v5 schema")

metadata_relative = f"candidate-output/{architecture}/image-ref.json"
metadata = load(metadata_relative)
expected_manifest = {
    "architecture": architecture,
    "candidate_commit": commit,
    "image": image_ref,
    "builder_source_image": image_ref,
    "native_host_os": "Linux",
    "native_host_arch": architecture,
    "container_engine_arch": architecture,
    "installer_config": "os/iso/config.toml",
    "installer_branding_applied": True,
    "installer_payload_source_kind": "release-registry",
    "installer_payload_source_local_only": False,
    "shippable_release": True,
}
if any(manifest.get(key) != value for key, value in expected_manifest.items()):
    raise SystemExit("public ISO manifest is not the requested shippable exact candidate")
if not re.fullmatch(r"[^\s]+@sha256:[0-9a-f]{64}", manifest.get("installer_branding_image", "")):
    raise SystemExit("public ISO manifest does not pin the branding tool")
if manifest.get("installer_branding_ownership_helper_image") != manifest.get("installer_branding_image"):
    raise SystemExit("public ISO manifest changes branding-tool identity")
if not re.fullmatch(r"[^\s]+@sha256:[0-9a-f]{64}", manifest.get("builder_image", "")):
    raise SystemExit("public ISO manifest does not pin bootc-image-builder")
if manifest.get("builder_output_ownership_helper_image") != manifest.get("builder_image"):
    raise SystemExit("public ISO manifest changes builder identity")

expected_evidence = {
    "schema": "goblins-os-release-evidence-v5",
    "architecture": architecture,
    "candidate_commit": commit,
    "image_ref": image_ref,
    "image_digest_pinned": True,
    "cargo_packages_tsv": "cargo-lock-packages.tsv",
    "cargo_packages_sha256": sha(cargo_relative),
    "rpm_command_file": "rpm-packages.command",
    "rpm_command_sha256": sha(rpm_command_relative),
    "rpm_packages_tsv": "rpm-packages.tsv",
    "rpm_packages_sha256": sha(rpm_relative),
    "rpm_status": "generated from rpm database",
}
if any(evidence.get(key) != value for key, value in expected_evidence.items()):
    raise SystemExit("release evidence is not the requested exact candidate")

attempt = int(workflow_attempt)
expected_metadata = {
    "schema": "goblins-os-candidate-image-ref-v3",
    "product": "Goblins OS",
    "architecture": architecture,
    "platform": "linux/arm64",
    "candidate_commit": commit,
    "image_digest": image_ref.rsplit("@", 1)[1],
    "immutable_image_ref": image_ref,
    "oci_revision": commit,
    "iso_sha256": sha(iso_relative),
    "iso_manifest_sha256": sha(manifest_relative),
    "bib_manifest_sha256": sha(bib_relative),
    "release_evidence_manifest_sha256": sha(evidence_relative),
    "cargo_packages_sha256": sha(cargo_relative),
    "rpm_command_sha256": sha(rpm_command_relative),
    "rpm_packages_sha256": sha(rpm_relative),
    "installer_config": "os/iso/config.toml",
    "source_repository": "https://github.com/Joe-Simo/goblins-os",
    "workflow_run": workflow_run,
    "workflow_run_attempt": attempt,
    "workflow_name": "candidate-artifacts",
    "candidate_tag_authoritative": False,
    "non_promotional": True,
}
if any(metadata.get(key) != value for key, value in expected_metadata.items()):
    raise SystemExit("candidate workflow metadata does not bind every public artifact byte")
gates = metadata.get("exact_candidate_gates")
if not isinstance(gates, dict) or any(
    gates.get(name) != "pass"
    for name in ("source_verifier", "installed_root_verifier", "services_selftest")
):
    raise SystemExit("candidate workflow metadata does not record all exact-candidate gates")

checksum = (root / checksum_relative).read_text(encoding="utf-8").splitlines()
expected_checksum = f"{sha(iso_relative)}  goblins-os-{architecture}.iso"
if checksum != [expected_checksum]:
    raise SystemExit("public ISO checksum file is not exact")
PY
}

validate_candidate_run() {
  local run_record="$1"
  local artifacts_record="$2"
  local commit="$3"
  local run_url="$4"
  local attempt="$5"
  local artifact_name="$6"

  python3 - "$run_record" "$artifacts_record" "$commit" "$run_url" "$attempt" "$artifact_name" <<'PY'
import json
import re
import sys

run_path, artifacts_path, commit, run_url, attempt, artifact_name = sys.argv[1:7]

def reject_duplicate(pairs):
    result = {}
    for key, value in pairs:
        if key in result:
            raise ValueError(f"duplicate key: {key}")
        result[key] = value
    return result

with open(run_path, encoding="utf-8") as handle:
    run = json.load(handle, object_pairs_hook=reject_duplicate)
with open(artifacts_path, encoding="utf-8") as handle:
    response = json.load(handle, object_pairs_hook=reject_duplicate)

expected = {
    "html_url": run_url,
    "head_sha": commit,
    "run_attempt": int(attempt),
    "status": "completed",
    "conclusion": "success",
    "event": "workflow_dispatch",
}
if any(run.get(key) != value for key, value in expected.items()):
    raise SystemExit("GitHub workflow run does not match the exact candidate")
repository = run.get("head_repository") or {}
if repository.get("full_name") != "Joe-Simo/goblins-os":
    raise SystemExit("GitHub workflow run has the wrong source repository")
if run.get("path") not in (
    ".github/workflows/candidate-artifacts.yml",
    ".github/workflows/release.yml",
):
    raise SystemExit("GitHub workflow run has the wrong workflow path")

matches = [
    item for item in response.get("artifacts", [])
    if item.get("name") == artifact_name and item.get("expired") is False
]
if len(matches) != 1:
    raise SystemExit("exactly one unexpired candidate artifact is required")
artifact_run = matches[0].get("workflow_run") or {}
if artifact_run.get("id") != run.get("id") or artifact_run.get("head_sha") != commit:
    raise SystemExit("candidate artifact is not bound to the requested workflow run")
PY
}

hydrate_historical_alpha() {
  local tag="${GOBLINS_OS_RELEASE_TAG:-}"
  local base_url staging archive_root iso_dir boot_dir evidence_dir
  local parts_sha zst_sha zst_name iso_name part_ref part_name historical_image_ref

  [[ "$tag" =~ ^v[0-9]+\.[0-9]+\.[0-9]+-alpha\.[0-9]{8}([.-][0-9A-Za-z._-]+)?$ ]] || {
    echo "error: historical-alpha requires an explicit immutable alpha tag" >&2
    exit 2
  }
  [ -z "${GOBLINS_OS_CANDIDATE_COMMIT:-}${GOBLINS_OS_CANDIDATE_IMAGE_REF:-}${GOBLINS_OS_CANDIDATE_WORKFLOW_RUN:-}${GOBLINS_OS_CANDIDATE_WORKFLOW_RUN_ATTEMPT:-}" ] || {
    echo "error: candidate identity variables are forbidden in historical-alpha mode" >&2
    exit 2
  }
  case "$DOWNLOAD_ISO" in 0 | 1) ;; *) echo "error: GOBLINS_OS_DOWNLOAD_ISO must be 0 or 1" >&2; exit 2 ;; esac

  require_command curl
  require_command python3
  base_url="https://github.com/Joe-Simo/goblins-os/releases/download/$tag"
  staging="$(mktemp -d "${TMPDIR:-/tmp}/goblins-historical-alpha.XXXXXX")"
  CLEANUP_DIR="$staging"
  iso_dir="$staging/os/iso/output/$ARCH"
  boot_dir="$iso_dir/bootiso"
  evidence_dir="$staging/os/signoff-proofs/sbom/$ARCH"
  mkdir -p "$boot_dir" "$evidence_dir"

  download_release_asset "$base_url" "goblins-os-$ARCH.iso.sha256" "$boot_dir/goblins-os-$ARCH.iso.sha256"
  normalize_sha256_file_paths "$boot_dir/goblins-os-$ARCH.iso.sha256"
  download_release_asset "$base_url" "manifest-goblins-os-$ARCH.json" "$iso_dir/manifest-goblins-os-$ARCH.json"
  download_release_asset "$base_url" "manifest-anaconda-iso-$ARCH.json" "$iso_dir/manifest-anaconda-iso.json"
  download_release_asset "$base_url" "cargo-lock-packages-$ARCH.tsv" "$evidence_dir/cargo-lock-packages.tsv"
  download_release_asset "$base_url" "rpm-packages-$ARCH.command" "$evidence_dir/rpm-packages.command"
  download_release_asset "$base_url" "rpm-packages-$ARCH.tsv" "$evidence_dir/rpm-packages.tsv"
  download_release_asset "$base_url" "release-evidence-manifest-$ARCH.json" "$evidence_dir/release-evidence-manifest.json"

  goblins_os_historical_release_evidence_hashes_match "$evidence_dir" || {
    echo "error: historical release evidence is incomplete or hash-mismatched" >&2
    exit 1
  }
  rpm_sbom_arch_matches "$evidence_dir/rpm-packages.tsv" "$ARCH" || {
    echo "error: historical RPM inventory is not aarch64" >&2
    exit 1
  }
  validate_json_envelopes "$staging"
  historical_image_ref="$(python3 - "$iso_dir/manifest-goblins-os-$ARCH.json" <<'PY'
import json
import sys
with open(sys.argv[1], encoding="utf-8") as handle:
    print(json.load(handle)["builder_source_image"])
PY
)"
  [ "$(goblins_os_bib_manifest_payload_ref "$iso_dir/manifest-anaconda-iso.json")" = "$historical_image_ref" ] || {
    echo "error: historical BIB manifest changes the archived image identity" >&2
    exit 1
  }

  if [ "$DOWNLOAD_ISO" = "1" ]; then
    require_command zstd
    parts_sha="goblins-os-$ARCH.iso.zst.parts.sha256"
    zst_sha="goblins-os-$ARCH.iso.zst.sha256"
    zst_name="goblins-os-$ARCH.iso.zst"
    iso_name="goblins-os-$ARCH.iso"
    download_release_asset "$base_url" "$parts_sha" "$boot_dir/$parts_sha"
    download_release_asset "$base_url" "$zst_sha" "$boot_dir/$zst_sha"
    normalize_sha256_file_paths "$boot_dir/$parts_sha"
    normalize_sha256_file_paths "$boot_dir/$zst_sha"
    while read -r _ part_ref; do
      [ -n "${part_ref:-}" ] || continue
      part_name="${part_ref#\*}"
      [[ "$part_name" =~ ^goblins-os-aarch64\.iso\.zst\.part-[0-9]{3,}$ ]] || {
        echo "error: historical ISO part name is not canonical: $part_name" >&2
        exit 1
      }
      download_release_asset "$base_url" "$part_name" "$boot_dir/$part_name"
    done <"$boot_dir/$parts_sha"
    sha256_check "$boot_dir" "$parts_sha"
    : >"$boot_dir/$zst_name"
    while read -r _ part_ref; do
      part_name="${part_ref#\*}"
      [ -n "$part_name" ] || continue
      cat -- "$boot_dir/$part_name" >>"$boot_dir/$zst_name"
    done <"$boot_dir/$parts_sha"
    sha256_check "$boot_dir" "$zst_sha"
    zstd -d --long=31 -f -o "$boot_dir/$iso_name" "$boot_dir/$zst_name"
    sha256_check "$boot_dir" "$iso_name.sha256"
  fi

  archive_root="$REPO_ROOT/os/release/historical-alpha/$tag"
  while IFS= read -r -d '' source; do
    install_validated_file "$source" "$archive_root/${source#"$staging"/}"
  done < <(find "$staging" -type f -print0)
  echo "==> Historical alpha archived at ${archive_root#"$REPO_ROOT"/}"
  echo "==> This archive is non-authoritative and cannot satisfy current signoff."
  cleanup
  CLEANUP_DIR=""
}

hydrate_exact_candidate() {
  local commit="${GOBLINS_OS_CANDIDATE_COMMIT:-}"
  local image_ref="${GOBLINS_OS_CANDIDATE_IMAGE_REF:-}"
  local run_url="${GOBLINS_OS_CANDIDATE_WORKFLOW_RUN:-}"
  local attempt="${GOBLINS_OS_CANDIDATE_WORKFLOW_RUN_ATTEMPT:-}"
  local run_id artifact staging run_record artifacts_record metadata
  local iso checksum iso_manifest bib_manifest evidence_dir candidate_dir
  local source relative destination

  [ -z "${GOBLINS_OS_RELEASE_TAG:-}" ] || {
    echo "error: GOBLINS_OS_RELEASE_TAG is forbidden in exact-candidate mode" >&2
    exit 2
  }
  [ "$DOWNLOAD_ISO" = "0" ] || {
    echo "error: exact-candidate always hydrates the complete ISO artifact; leave GOBLINS_OS_DOWNLOAD_ISO unset" >&2
    exit 2
  }
  [[ "$commit" =~ ^[0-9a-f]{40}$ ]] || {
    echo "error: exact-candidate requires a lowercase 40-hex candidate commit" >&2
    exit 2
  }
  [[ "$image_ref" =~ ^ghcr\.io/joe-simo/goblins-os@sha256:[0-9a-f]{64}$ ]] || {
    echo "error: exact-candidate requires the canonical digest-pinned aarch64 image" >&2
    exit 2
  }
  [[ "$run_url" =~ ^https://github\.com/Joe-Simo/goblins-os/actions/runs/([1-9][0-9]*)$ ]] || {
    echo "error: exact-candidate requires the canonical GitHub Actions run URL" >&2
    exit 2
  }
  run_id="${BASH_REMATCH[1]}"
  [[ "$attempt" =~ ^[1-9][0-9]*$ ]] || {
    echo "error: exact-candidate requires a positive workflow run attempt" >&2
    exit 2
  }
  [ "$(git rev-parse HEAD | tr '[:upper:]' '[:lower:]')" = "$commit" ] || {
    echo "error: exact-candidate commit does not match the checked-out HEAD" >&2
    exit 1
  }

  require_command gh
  require_command git
  require_command python3
  artifact="goblins-os-candidate-$commit-$ARCH"
  staging="$(mktemp -d "${TMPDIR:-/tmp}/goblins-exact-candidate.XXXXXX")"
  CLEANUP_DIR="$staging"
  run_record="$staging/workflow-run.json"
  artifacts_record="$staging/workflow-artifacts.json"

  gh api "repos/Joe-Simo/goblins-os/actions/runs/$run_id/attempts/$attempt" >"$run_record"
  gh api "repos/Joe-Simo/goblins-os/actions/runs/$run_id/artifacts?per_page=100" >"$artifacts_record"
  validate_candidate_run "$run_record" "$artifacts_record" "$commit" "$run_url" "$attempt" "$artifact"
  gh run download "$run_id" \
    --repo Joe-Simo/goblins-os \
    --name "$artifact" \
    --dir "$staging/artifact"

  iso="$staging/artifact/os/iso/output/$ARCH/bootiso/goblins-os-$ARCH.iso"
  checksum="$iso.sha256"
  iso_manifest="$staging/artifact/os/iso/output/$ARCH/manifest-goblins-os-$ARCH.json"
  bib_manifest="$staging/artifact/os/iso/output/$ARCH/manifest-anaconda-iso.json"
  evidence_dir="$staging/artifact/os/signoff-proofs/sbom/$ARCH"
  candidate_dir="$staging/artifact/candidate-output/$ARCH"
  metadata="$candidate_dir/image-ref.json"
  for source in \
    "$iso" "$checksum" "$iso_manifest" "$bib_manifest" \
    "$evidence_dir/release-evidence-manifest.json" \
    "$evidence_dir/cargo-lock-packages.tsv" \
    "$evidence_dir/rpm-packages.command" \
    "$evidence_dir/rpm-packages.tsv" \
    "$metadata"; do
    [ -s "$source" ] && [ -f "$source" ] && [ ! -L "$source" ] || {
      echo "error: exact-candidate artifact is missing required file: ${source#"$staging/artifact"/}" >&2
      exit 1
    }
  done
  normalize_sha256_file_paths "$checksum"
  sha256_check "$(dirname "$iso")" "$(basename "$checksum")"
  goblins_os_release_evidence_hashes_match "$evidence_dir" || {
    echo "error: exact-candidate release evidence is hash-mismatched" >&2
    exit 1
  }
  rpm_sbom_arch_matches "$evidence_dir/rpm-packages.tsv" "$ARCH" || {
    echo "error: exact-candidate RPM inventory is not aarch64" >&2
    exit 1
  }
  [ "$(goblins_os_bib_manifest_payload_ref "$bib_manifest")" = "$image_ref" ] || {
    echo "error: exact-candidate BIB manifest does not bind the requested image" >&2
    exit 1
  }
  validate_json_envelopes "$staging/artifact" "$commit" "$image_ref" "$run_url" "$attempt"

  for source in \
    "$iso" "$checksum" "$iso_manifest" "$bib_manifest" \
    "$evidence_dir/release-evidence-manifest.json" \
    "$evidence_dir/cargo-lock-packages.tsv" \
    "$evidence_dir/rpm-packages.command" \
    "$evidence_dir/rpm-packages.tsv"; do
    relative="${source#"$staging/artifact"/}"
    destination="$REPO_ROOT/$relative"
    install_validated_file "$source" "$destination"
  done
  install_validated_file "$metadata" "$REPO_ROOT/os/signoff-proofs/candidate/$ARCH/image-ref.json"
  [ ! -L "$REPO_ROOT/os/signoff-proofs/sbom/$ARCH/rpm-packages.not-generated.txt" ] || {
    echo "error: refusing symlinked stale RPM marker" >&2
    exit 1
  }
  rm -f -- "$REPO_ROOT/os/signoff-proofs/sbom/$ARCH/rpm-packages.not-generated.txt"

  [ "$(sha256_file "$REPO_ROOT/os/iso/output/$ARCH/bootiso/goblins-os-$ARCH.iso")" = "$(sha256_file "$iso")" ] || {
    echo "error: hydrated public ISO changed during installation" >&2
    exit 1
  }
  echo "==> Hydrated exact aarch64 candidate $commit from run $run_id attempt $attempt"
  echo "==> Public ISO, manifests, SBOM, and workflow metadata share one validated identity."
  cleanup
  CLEANUP_DIR=""
}

case "$MODE" in
  historical-alpha) hydrate_historical_alpha ;;
  exact-candidate) hydrate_exact_candidate ;;
  *) usage ;;
esac
