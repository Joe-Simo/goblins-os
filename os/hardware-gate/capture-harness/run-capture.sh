#!/usr/bin/env bash
# Drive the full hardware-gate capture in a display-backed qemu VM and close-signoff.
#
# Boots the hardware-gate ISO built with os/iso/verify-config.toml (so the
# embedded /osbuild.ks, not a sidecar disk, drives Anaconda), waits for the bootc
# install + first-boot GDM-autologin desktop, completes first boot through the
# same core API contracts as the private/offline UI path, publishes the
# in-session orchestrator for the verification-only user service, captures
# the 32 required shots by QMP-screendump on each HTTP signal,
# writes proof-manifest.json, and runs close-signoff.sh.
#
# Honest: every shot is a real framebuffer capture of the real installed OS.
# Gaming uses the OS's own lavapipe/gamescope/pipewire stack; studio-live uses a
# host-served model over 10.0.2.2. Works on a native Linux/KVM host (CI) and on
# macOS/hvf. KVM is required for x86_64 at usable speed; aarch64 also runs on hvf.
set -euo pipefail

normalize_semantic_screenshot_frame() {
  local source_file="$1"
  local output_file="$2"
  local width height crop_width crop_height crop_file

  if command -v magick >/dev/null 2>&1; then
    magick "$source_file" -gravity Center -crop '1200x800+0+0' +repage \
      -resize 32x24! -background white -alpha remove -alpha off \
      "BMP3:$output_file" >/dev/null 2>&1
  elif command -v convert >/dev/null 2>&1; then
    convert "$source_file" -gravity Center -crop '1200x800+0+0' +repage \
      -resize 32x24! -background white -alpha remove -alpha off \
      "BMP3:$output_file" >/dev/null 2>&1
  elif command -v sips >/dev/null 2>&1; then
    width="$(sips -g pixelWidth "$source_file" 2>/dev/null | awk '/pixelWidth:/{print $2; exit}')"
    height="$(sips -g pixelHeight "$source_file" 2>/dev/null | awk '/pixelHeight:/{print $2; exit}')"
    [ -n "$width" ] && [ -n "$height" ] && [ "$width" -gt 0 ] && [ "$height" -gt 0 ] || return 1
    if [ "$width" -lt 1200 ]; then crop_width="$width"; else crop_width=1200; fi
    if [ "$height" -lt 800 ]; then crop_height="$height"; else crop_height=800; fi
    crop_file="$output_file.crop.png"
    sips --cropToHeightWidth "$crop_height" "$crop_width" \
      "$source_file" --out "$crop_file" >/dev/null 2>&1 \
      && sips --resampleHeightWidth 24 32 -s format bmp \
        "$crop_file" --out "$output_file" >/dev/null 2>&1
  else
    echo "[semantic-frame][FAIL] normalization requires ImageMagick or macOS sips" >&2
    return 1
  fi
  [ -s "$output_file" ]
}

semantic_screenshot_frames_are_distinct() {
  local screenshot_dir="$1"
  local output_mode="${2:-verbose}"
  local scratch_dir frame_spec frame_name frame_file checker_rc

  # Normalize a fixed central application crop, not the whole framebuffer. This
  # excludes the shell clock and most pointer travel. A tolerant coarse-grid
  # comparison then requires a substantive multi-cell change, so a clock glyph
  # or pointer cannot turn one reused application state into distinct proof.
  scratch_dir="$(mktemp -d "${TMPDIR:-/tmp}/goblins-semantic-frame.XXXXXX")"
  for frame_spec in \
    "login|03-login.png" \
    "home|07-home.png" \
    "studio-before|13-studio-before.png" \
    "studio-running|14-studio-running.png" \
    "studio-result-app-detail|15-studio-app-detail.png" \
    "studio-built-open|16-built-app-open.png"; do
    frame_name="${frame_spec%%|*}"
    frame_file="${frame_spec#*|}"
    if ! normalize_semantic_screenshot_frame \
      "$screenshot_dir/$frame_file" "$scratch_dir/$frame_name.bmp"; then
      echo "[semantic-frame][FAIL] could not normalize $screenshot_dir/$frame_file" >&2
      rm -rf "$scratch_dir"
      return 1
    fi
  done

  if python3 - "$output_mode" \
    "$scratch_dir/login.bmp" \
    "$scratch_dir/home.bmp" \
    "$scratch_dir/studio-before.bmp" \
    "$scratch_dir/studio-running.bmp" \
    "$scratch_dir/studio-result-app-detail.bmp" \
    "$scratch_dir/studio-built-open.bmp" <<'PY'
import struct
import sys
from pathlib import Path

QUIET = sys.argv[1] == "quiet"
FRAME_NAMES = (
    "login",
    "home",
    "studio-before",
    "studio-running",
    "studio-result-app-detail",
    "studio-built-open",
)
MIN_DISTANCE_PPM = 2500
MIN_CHANGED_CELLS = 8


def read_normalized_bmp(path):
    encoded = Path(path).read_bytes()
    if len(encoded) < 54 or encoded[:2] != b"BM":
        raise ValueError(f"{path} is not a BMP")
    pixel_offset = struct.unpack_from("<I", encoded, 10)[0]
    dib_size = struct.unpack_from("<I", encoded, 14)[0]
    if dib_size < 40:
        raise ValueError(f"{path} has an unsupported BMP header")
    width, stored_height = struct.unpack_from("<ii", encoded, 18)
    planes, bits_per_pixel = struct.unpack_from("<HH", encoded, 26)
    compression = struct.unpack_from("<I", encoded, 30)[0]
    if width != 32 or abs(stored_height) != 24:
        raise ValueError(f"{path} is not the expected 32x24 normalized frame")
    if planes != 1 or bits_per_pixel not in (24, 32) or compression != 0:
        raise ValueError(f"{path} has unsupported BMP pixel data")

    height = abs(stored_height)
    row_stride = ((width * bits_per_pixel + 31) // 32) * 4
    required_size = pixel_offset + row_stride * height
    if len(encoded) < required_size:
        raise ValueError(f"{path} has truncated BMP pixel data")

    top_down = stored_height < 0
    pixels = []
    bytes_per_pixel = bits_per_pixel // 8
    for y in range(height):
        stored_y = y if top_down else height - y - 1
        row_start = pixel_offset + stored_y * row_stride
        for x in range(width):
            index = row_start + x * bytes_per_pixel
            blue, green, red = encoded[index : index + 3]
            pixels.append((red, green, blue))
    return pixels


def frame_distance(left, right):
    absolute_difference = 0
    changed_cells = 0
    for left_pixel, right_pixel in zip(left, right):
        channel_difference = [
            abs(left_pixel[channel] - right_pixel[channel]) for channel in range(3)
        ]
        absolute_difference += sum(channel_difference)
        if sum(channel_difference) // 3 >= 8:
            changed_cells += 1
    denominator = len(left) * 3 * 255
    return (absolute_difference * 1_000_000) // denominator, changed_cells


pairs = (
    ("login vs Home", "login", "home"),
    ("Studio before vs running", "studio-before", "studio-running"),
    (
        "Studio before vs result/app-detail",
        "studio-before",
        "studio-result-app-detail",
    ),
    ("Studio before vs built-open", "studio-before", "studio-built-open"),
    (
        "Studio running vs result/app-detail",
        "studio-running",
        "studio-result-app-detail",
    ),
    ("Studio running vs built-open", "studio-running", "studio-built-open"),
    (
        "Studio result/app-detail vs built-open",
        "studio-result-app-detail",
        "studio-built-open",
    ),
)

try:
    frames = {
        name: read_normalized_bmp(path)
        for name, path in zip(FRAME_NAMES, sys.argv[2:])
    }
except (OSError, ValueError, struct.error) as error:
    print(f"[semantic-frame][FAIL] could not read normalized screenshot: {error}", file=sys.stderr)
    raise SystemExit(1)

failed = False
for label, left_name, right_name in pairs:
    distance_ppm, changed_cells = frame_distance(frames[left_name], frames[right_name])
    detail = f"distance_ppm={distance_ppm} changed_cells={changed_cells}"
    if distance_ppm < MIN_DISTANCE_PPM or changed_cells < MIN_CHANGED_CELLS:
        print(
            f"[semantic-frame][FAIL] {label} reused the same central application state ({detail})",
            file=sys.stderr,
        )
        failed = True
    elif not QUIET:
        print(f"[semantic-frame][PASS] {label} is distinct ({detail})")

if failed:
    raise SystemExit(1)
PY
  then
    checker_rc=0
  else
    checker_rc=$?
  fi
  rm -rf "$scratch_dir"
  return "$checker_rc"
}

if [ "${1:-}" = "--check-semantic-screenshots" ]; then
  if [ "$#" -lt 2 ] || [ "$#" -gt 3 ]; then
    echo "usage: $0 --check-semantic-screenshots <screenshot-dir> [quiet]" >&2
    exit 2
  fi
  semantic_screenshot_frames_are_distinct "$2" "${3:-verbose}"
  exit $?
fi

ARCH="${GOBLINS_OS_ARCH:-$(uname -m)}"
case "$ARCH" in arm64|aarch64) ARCH=aarch64; QEMU=qemu-system-aarch64;; x86_64|amd64) ARCH=x86_64; QEMU=qemu-system-x86_64;; *) echo "unsupported arch $ARCH"; exit 2;; esac
CANDIDATE_COMMIT="${GOBLINS_OS_CANDIDATE_COMMIT:-}"
if [[ ! "$CANDIDATE_COMMIT" =~ ^[0-9a-fA-F]{40}$ ]]; then
  echo "GOBLINS_OS_CANDIDATE_COMMIT must name the exact 40-hex source commit used for the verification ISO." >&2
  exit 2
fi
CANDIDATE_COMMIT="$(printf '%s' "$CANDIDATE_COMMIT" | tr '[:upper:]' '[:lower:]')"
export GOBLINS_OS_CANDIDATE_COMMIT="$CANDIDATE_COMMIT"
REPO="${REPO_ROOT:-$(pwd)}"
REPO="$(cd "$REPO" && pwd -P)"
. "$REPO/os/iso/manifest-provenance.sh"
. "$REPO/os/hardware-gate/release-evidence.sh"
SCRIPT_DIR="$(cd "$(dirname "$0")" && pwd -P)"
HERE="$REPO/os/hardware-gate/capture-harness"
if [ "$SCRIPT_DIR" != "$HERE" ]; then
  echo "Run the capture harness from the exact candidate checkout: $HERE/run-capture.sh" >&2
  exit 2
fi
if ! git -C "$REPO" rev-parse --is-inside-work-tree >/dev/null 2>&1; then
  echo "REPO_ROOT must identify the exact candidate Git checkout: $REPO" >&2
  exit 2
fi
SOURCE_HEAD="$(git -C "$REPO" rev-parse HEAD | tr '[:upper:]' '[:lower:]')"
if [ "$SOURCE_HEAD" != "$CANDIDATE_COMMIT" ]; then
  echo "Capture tooling checkout $SOURCE_HEAD does not match candidate $CANDIDATE_COMMIT." >&2
  exit 2
fi
UNEXPECTED_SOURCE_CHANGES="$({
  git -C "$REPO" -c core.quotepath=false diff --name-only --no-ext-diff
  git -C "$REPO" -c core.quotepath=false diff --cached --name-only --no-ext-diff
  git -C "$REPO" -c core.quotepath=false ls-files --others --exclude-standard
} | sed '/^$/d' | sort -u | grep -Ev '^os/(iso/output/|signoff-proofs/|screenshots/hardware-gate/)|^os/signoff-notes[.]md$' || true)"
if [ -n "$UNEXPECTED_SOURCE_CHANGES" ]; then
  echo "Capture checkout has changes outside generated proof paths:" >&2
  printf '%s\n' "$UNEXPECTED_SOURCE_CHANGES" >&2
  exit 2
fi
ISO="${GOBLINS_OS_CAPTURE_ISO:-$REPO/os/iso/output/$ARCH/bootiso/goblins-os-$ARCH.iso}"
SHA_FILE="${GOBLINS_OS_CAPTURE_ISO_SHA256:-$ISO.sha256}"
ISO_MANIFEST="${GOBLINS_OS_CAPTURE_ISO_MANIFEST:-$(dirname "$(dirname "$ISO")")/manifest-goblins-os-$ARCH.json}"
BIB_MANIFEST="${GOBLINS_OS_CAPTURE_BIB_MANIFEST:-$(dirname "$ISO_MANIFEST")/manifest-anaconda-iso.json}"
CAPTURE_EVIDENCE_DIR="${GOBLINS_OS_CAPTURE_RELEASE_EVIDENCE_DIR:-}"
CAPTURE_NATIVE_GATE_PROOF="${GOBLINS_OS_CAPTURE_NATIVE_PACKAGING_GATE_PROOF:-}"
CAPTURE_NATIVE_GATE_RUN_URL="${GOBLINS_OS_CAPTURE_NATIVE_PACKAGING_GATE_RUN_URL:-}"
CAPTURE_NATIVE_GATE_RUN_ATTEMPT="${GOBLINS_OS_CAPTURE_NATIVE_PACKAGING_GATE_RUN_ATTEMPT:-}"
EXPECTED_IMAGE_REF="${GOBLINS_OS_CAPTURE_EXPECTED_IMAGE_REF:-${GOBLINS_OS_IMAGE:-}}"
if [[ ! "$EXPECTED_IMAGE_REF" =~ ^[^[:space:]@]+@sha256:[0-9a-f]{64}$ ]]; then
  echo "GOBLINS_OS_CAPTURE_EXPECTED_IMAGE_REF must name the exact digest-pinned candidate image selected for this proof." >&2
  exit 2
fi
CAPTURE_WORKFLOW_RUN_URL="${GOBLINS_OS_CAPTURE_WORKFLOW_RUN_URL:-}"
CAPTURE_WORKFLOW_RUN_ATTEMPT="${GOBLINS_OS_CAPTURE_WORKFLOW_RUN_ATTEMPT:-}"
if [ -n "$CAPTURE_WORKFLOW_RUN_URL" ]; then
  [[ "$CAPTURE_WORKFLOW_RUN_URL" =~ ^https://github\.com/[^/]+/[^/]+/actions/runs/[0-9]+$ ]] || {
    echo "GOBLINS_OS_CAPTURE_WORKFLOW_RUN_URL must be an exact GitHub Actions run URL." >&2
    exit 2
  }
  [[ "$CAPTURE_WORKFLOW_RUN_ATTEMPT" =~ ^[1-9][0-9]*$ ]] || {
    echo "GOBLINS_OS_CAPTURE_WORKFLOW_RUN_ATTEMPT must be a positive integer when a workflow run URL is provided." >&2
    exit 2
  }
elif [ -n "$CAPTURE_WORKFLOW_RUN_ATTEMPT" ]; then
  echo "GOBLINS_OS_CAPTURE_WORKFLOW_RUN_ATTEMPT requires GOBLINS_OS_CAPTURE_WORKFLOW_RUN_URL." >&2
  exit 2
fi
CAPTURE_REQUIRE_COMPLETE="${GOBLINS_OS_CAPTURE_REQUIRE_COMPLETE:-0}"
case "$CAPTURE_REQUIRE_COMPLETE" in
  0|1) ;;
  *)
    echo "GOBLINS_OS_CAPTURE_REQUIRE_COMPLETE must be 0 or 1." >&2
    exit 2
    ;;
esac
BASE_WORK="${WORK_DIR:-/tmp/gos-hwgate-$ARCH}"
WORK="$BASE_WORK"
PORT="${HTTP_PORT:-8099}"
DATE="${RUN_DATE:?set RUN_DATE=YYYY-MM-DD (scripts cannot read the clock)}"
if [[ ! "$DATE" =~ ^[0-9]{4}-[0-9]{2}-[0-9]{2}$ ]] \
  || ! python3 - "$DATE" <<'PY'
from datetime import date
import sys

try:
    parsed = date.fromisoformat(sys.argv[1])
except ValueError:
    raise SystemExit(1)
raise SystemExit(0 if parsed.isoformat() == sys.argv[1] else 1)
PY
then
  echo "RUN_DATE must be a real calendar date in YYYY-MM-DD form." >&2
  exit 2
fi
RUN_ROOT="$REPO/os/screenshots/hardware-gate/$ARCH"
RUN_DIR="$RUN_ROOT/$DATE"
RUN_ROOT_COMPONENT="$REPO"
for run_root_segment in os screenshots hardware-gate "$ARCH"; do
  RUN_ROOT_COMPONENT="$RUN_ROOT_COMPONENT/$run_root_segment"
  if [ -L "$RUN_ROOT_COMPONENT" ]; then
    echo "Refusing symlinked hardware-gate path component: $RUN_ROOT_COMPONENT" >&2
    exit 2
  fi
done
mkdir -p "$RUN_ROOT"
RUN_ROOT_REAL="$(cd "$RUN_ROOT" && pwd -P)"
if [ "$RUN_ROOT_REAL" != "$RUN_ROOT" ]; then
  echo "Hardware-gate run root resolves outside the canonical candidate checkout: $RUN_ROOT_REAL" >&2
  exit 2
fi
HTTPD=""
QEMU_PID=""
CAPTURE_STARTED=0
INSTALL_MARKER_RC="${GOS_INSTALL_MARKER_EXIT_CODE:-71}"
AARCH64_INSTALL_REBOOT_TIMEOUT="${GOS_AARCH64_INSTALL_REBOOT_TIMEOUT:-420}"

dump_file_tail() {
  local label="$1"
  local path="$2"
  if [ -s "$path" ]; then
    echo "---- $label: $path ----"
    tail -n 200 "$path" || true
  else
    echo "---- $label missing or empty: $path ----"
  fi
}

sha256_file() {
  if command -v sha256sum >/dev/null 2>&1; then
    sha256sum "$1" | awk '{print $1}'
  elif command -v shasum >/dev/null 2>&1; then
    shasum -a 256 "$1" | awk '{print $1}'
  else
    echo "sha256sum or shasum is required to hash release proof artifacts." >&2
    return 1
  fi
}

require_repo_generated_directory() {
  local relative="$1"
  local current="$REPO"
  local segment resolved expected
  local -a path_segments

  IFS='/' read -r -a path_segments <<< "$relative"
  for segment in "${path_segments[@]}"; do
    [ -n "$segment" ] || continue
    current="$current/$segment"
    if [ -L "$current" ]; then
      echo "Refusing symlinked generated-artifact path component: $current" >&2
      exit 2
    fi
  done
  mkdir -p "$current"
  resolved="$(cd "$current" && pwd -P)"
  expected="$REPO/$relative"
  if [ "$resolved" != "$expected" ]; then
    echo "Generated-artifact directory resolves outside the candidate checkout: $resolved" >&2
    exit 2
  fi
}

copy_file_atomically() {
  local source="$1"
  local destination="$2"
  local temporary

  [ ! -L "$destination" ] || {
    echo "Refusing symlinked generated-artifact destination: $destination" >&2
    exit 2
  }
  temporary="$(mktemp "$(dirname "$destination")/.goblins-copy.XXXXXX")"
  if ! cp "$source" "$temporary"; then
    rm -f -- "$temporary"
    return 1
  fi
  mv -f -- "$temporary" "$destination"
}

copy_capture_logs() {
  local suffix="${1:-}"
  local target="$RUN_DIR/_capture-logs"
  if [ -n "$suffix" ]; then
    target="$target/$suffix"
  fi
  mkdir -p "$target"
  local name
  for name in qemu.log serial.log httpd.log; do
    if [ -e "$WORK/$name" ]; then
      cp -f "$WORK/$name" "$target/$name" || true
    fi
  done
}

dump_capture_logs() {
  copy_capture_logs
  echo "QEMU startup diagnostics"
  command -v "$QEMU" >/dev/null 2>&1 && "$QEMU" --version | head -n 1 || true
  [ -e /dev/kvm ] && ls -l /dev/kvm || true
  [ -n "${QEMU_PID:-}" ] && ps -p "$QEMU_PID" -o pid,stat,etime,command || true
  [ -S "$WORK/qmp.sock" ] && echo "QMP socket exists: $WORK/qmp.sock" || echo "QMP socket missing: $WORK/qmp.sock"
  dump_file_tail "qemu.log" "$WORK/qemu.log"
  dump_file_tail "serial.log" "$WORK/serial.log"
  dump_file_tail "httpd.log" "$WORK/httpd.log"
}

cleanup() {
  local rc=$?
  if [ "$rc" -ne 0 ] && [ "${CAPTURE_STARTED:-0}" = "1" ]; then
    dump_capture_logs
  fi
  [ -n "${QEMU_PID:-}" ] && kill "$QEMU_PID" 2>/dev/null || true
  [ -n "${HTTPD:-}" ] && kill "$HTTPD" 2>/dev/null || true
}
trap cleanup EXIT

[ -f "$ISO" ] || { echo "missing ISO $ISO"; exit 1; }
[ -f "$SHA_FILE" ] || { echo "missing ISO SHA256 file $SHA_FILE"; exit 1; }
[ -f "$ISO_MANIFEST" ] || { echo "missing ISO manifest $ISO_MANIFEST"; exit 1; }
[ -f "$BIB_MANIFEST" ] || { echo "missing bootc-image-builder manifest $BIB_MANIFEST"; exit 1; }
for capture_input in "$ISO" "$SHA_FILE" "$ISO_MANIFEST" "$BIB_MANIFEST"; do
  if [ -L "$capture_input" ]; then
    echo "Refusing symlinked capture input artifact: $capture_input" >&2
    exit 2
  fi
done
if ! grep -Fq '"architecture": "'"$ARCH"'"' "$ISO_MANIFEST" \
  || ! grep -Fq '"candidate_commit": "'"$CANDIDATE_COMMIT"'"' "$ISO_MANIFEST"; then
  echo "ISO manifest must bind architecture $ARCH to candidate commit $CANDIDATE_COMMIT: $ISO_MANIFEST" >&2
  exit 1
fi
IMAGE_REF="$(awk -F'"' '/"builder_source_image"/ { print $4; exit }' "$ISO_MANIFEST")"
if [[ ! "$IMAGE_REF" =~ ^[^[:space:]@]+@sha256:[0-9a-f]{64}$ ]]; then
  echo "ISO manifest must bind the installer payload to an immutable registry digest: $ISO_MANIFEST" >&2
  exit 1
fi
if [ "$IMAGE_REF" != "$EXPECTED_IMAGE_REF" ]; then
  echo "ISO image provenance $IMAGE_REF does not match selected candidate image $EXPECTED_IMAGE_REF." >&2
  exit 1
fi
if ! BIB_IMAGE_REF="$(goblins_os_bib_manifest_payload_ref "$BIB_MANIFEST")"; then
  echo "Bootc manifest must contain exactly one installer payload image reference: $BIB_MANIFEST" >&2
  exit 1
fi
if [ "$BIB_IMAGE_REF" != "$IMAGE_REF" ]; then
  echo "Bootc installer payload $BIB_IMAGE_REF does not match ISO image provenance $IMAGE_REF." >&2
  exit 1
fi

NATIVE_GATE_PROOF_RELATIVE=""

require_verification_iso() {
  local missing=0
  local needle
  for needle in \
    "GOBLINS_VERIFY_INSTALL_DONE" \
    "ignoredisk --only-use=vda" \
    "goblins-hwgate-session-orchestrator"; do
    if ! LC_ALL=C grep -aFq "$needle" "$ISO"; then
      echo "verification ISO guard: missing $needle in $ISO" >&2
      missing=1
    fi
  done
  if [ "$missing" -ne 0 ]; then
    cat >&2 <<EOF
capture harness requires the verification-only hardware-gate ISO built with:
  GOBLINS_OS_ISO_CONFIG=os/iso/verify-config.toml

The public release ISO is intentionally human-safe and leaves storage
interactive, so it cannot satisfy automated display-backed proof. Build or
point GOBLINS_OS_CAPTURE_ISO at the verification ISO generated from the real
pullable release bootc image.
EOF
    exit 2
  fi
}

require_verification_iso
ISO_SHA="$(awk '{print $1; exit}' "$SHA_FILE")"
if [[ ! "$ISO_SHA" =~ ^[0-9a-fA-F]{64}$ ]]; then
  echo "ISO checksum file does not begin with a valid SHA256 digest: $SHA_FILE" >&2
  exit 1
fi
ACTUAL_ISO_SHA="$(sha256_file "$ISO")"
ISO_SHA="$(printf '%s' "$ISO_SHA" | tr '[:upper:]' '[:lower:]')"
ACTUAL_ISO_SHA="$(printf '%s' "$ACTUAL_ISO_SHA" | tr '[:upper:]' '[:lower:]')"
if [ "$ACTUAL_ISO_SHA" != "$ISO_SHA" ]; then
  echo "Capture ISO checksum mismatch: expected $ISO_SHA, got $ACTUAL_ISO_SHA." >&2
  exit 1
fi

require_repo_generated_directory "os/iso/output/$ARCH/bootiso"
CANONICAL_OUTPUT="$REPO/os/iso/output/$ARCH"
CANONICAL_ISO="$CANONICAL_OUTPUT/bootiso/goblins-os-$ARCH.iso"
CANONICAL_SHA="$CANONICAL_ISO.sha256"
CANONICAL_ISO_MANIFEST="$CANONICAL_OUTPUT/manifest-goblins-os-$ARCH.json"
CANONICAL_BIB_MANIFEST="$CANONICAL_OUTPUT/manifest-anaconda-iso.json"
if [ "$ISO" != "$CANONICAL_ISO" ]; then
  copy_file_atomically "$ISO" "$CANONICAL_ISO"
  [ ! -L "$CANONICAL_SHA" ] || { echo "Refusing symlinked checksum destination: $CANONICAL_SHA" >&2; exit 2; }
  SHA_TEMP="$(mktemp "$(dirname "$CANONICAL_SHA")/.goblins-sha.XXXXXX")"
  printf '%s  %s\n' "$ISO_SHA" "$(basename "$CANONICAL_ISO")" > "$SHA_TEMP"
  mv -f -- "$SHA_TEMP" "$CANONICAL_SHA"
  copy_file_atomically "$ISO_MANIFEST" "$CANONICAL_ISO_MANIFEST"
  copy_file_atomically "$BIB_MANIFEST" "$CANONICAL_BIB_MANIFEST"
  ISO="$CANONICAL_ISO"
  SHA_FILE="$CANONICAL_SHA"
  ISO_MANIFEST="$CANONICAL_ISO_MANIFEST"
  BIB_MANIFEST="$CANONICAL_BIB_MANIFEST"
fi

if [ -n "$CAPTURE_EVIDENCE_DIR" ]; then
  EVIDENCE_MANIFEST="$CAPTURE_EVIDENCE_DIR/release-evidence-manifest.json"
  for evidence_file in \
    "$EVIDENCE_MANIFEST" \
    "$CAPTURE_EVIDENCE_DIR/cargo-lock-packages.tsv" \
    "$CAPTURE_EVIDENCE_DIR/rpm-packages.command" \
    "$CAPTURE_EVIDENCE_DIR/rpm-packages.tsv"; do
    [ -s "$evidence_file" ] || { echo "missing candidate release evidence $evidence_file" >&2; exit 1; }
  done
  grep -Fq '"architecture": "'"$ARCH"'"' "$EVIDENCE_MANIFEST" \
    && grep -Fq '"candidate_commit": "'"$CANDIDATE_COMMIT"'"' "$EVIDENCE_MANIFEST" \
    && grep -Fq '"image_ref": "'"$IMAGE_REF"'"' "$EVIDENCE_MANIFEST" \
    && grep -Fq '"image_digest_pinned": true' "$EVIDENCE_MANIFEST" || {
      echo "External release evidence is not bound to candidate $CANDIDATE_COMMIT and image $IMAGE_REF." >&2
      exit 1
    }
  goblins_os_release_evidence_hashes_match "$CAPTURE_EVIDENCE_DIR" || {
    echo "External release evidence Cargo/RPM inventories do not match their sealed SHA256 values." >&2
    exit 1
  }
  for evidence_file in \
    "$EVIDENCE_MANIFEST" \
    "$CAPTURE_EVIDENCE_DIR/cargo-lock-packages.tsv" \
    "$CAPTURE_EVIDENCE_DIR/rpm-packages.command" \
    "$CAPTURE_EVIDENCE_DIR/rpm-packages.tsv"; do
    if [ -L "$evidence_file" ]; then
      echo "Refusing symlinked release evidence input: $evidence_file" >&2
      exit 2
    fi
  done
  require_repo_generated_directory "os/signoff-proofs/sbom/$ARCH"
  CANONICAL_EVIDENCE_DIR="$REPO/os/signoff-proofs/sbom/$ARCH"
  for evidence_name in release-evidence-manifest.json cargo-lock-packages.tsv rpm-packages.command rpm-packages.tsv rpm-packages.not-generated.txt; do
    [ ! -L "$CANONICAL_EVIDENCE_DIR/$evidence_name" ] || {
      echo "Refusing symlinked release evidence destination: $CANONICAL_EVIDENCE_DIR/$evidence_name" >&2
      exit 2
    }
    rm -f -- "$CANONICAL_EVIDENCE_DIR/$evidence_name"
  done
  for evidence_name in cargo-lock-packages.tsv rpm-packages.command rpm-packages.tsv release-evidence-manifest.json; do
    copy_file_atomically \
      "$CAPTURE_EVIDENCE_DIR/$evidence_name" \
      "$CANONICAL_EVIDENCE_DIR/$evidence_name"
  done
  goblins_os_release_evidence_hashes_match "$CANONICAL_EVIDENCE_DIR" || {
    echo "Canonical release evidence failed hash validation after copy." >&2
    exit 1
  }
  EVIDENCE_MANIFEST="$CANONICAL_EVIDENCE_DIR/release-evidence-manifest.json"
fi

EVIDENCE_MANIFEST="${EVIDENCE_MANIFEST:-$REPO/os/signoff-proofs/sbom/$ARCH/release-evidence-manifest.json}"
if [ -n "$CAPTURE_NATIVE_GATE_PROOF" ] \
  || [ -n "$CAPTURE_NATIVE_GATE_RUN_URL" ] \
  || [ -n "$CAPTURE_NATIVE_GATE_RUN_ATTEMPT" ]; then
  [ -s "$CAPTURE_NATIVE_GATE_PROOF" ] || {
    echo "GOBLINS_OS_CAPTURE_NATIVE_PACKAGING_GATE_PROOF must name a nonempty native gate proof." >&2
    exit 1
  }
  [ -s "$EVIDENCE_MANIFEST" ] || {
    echo "Native packaging proof requires the matching release evidence manifest: $EVIDENCE_MANIFEST" >&2
    exit 1
  }
  [[ "$CAPTURE_NATIVE_GATE_RUN_URL" =~ ^https://github\.com/Joe-Simo/goblins-os/actions/runs/[0-9]+$ ]] || {
    echo "GOBLINS_OS_CAPTURE_NATIVE_PACKAGING_GATE_RUN_URL must be the exact GitHub Actions run URL that produced the proof." >&2
    exit 1
  }
  [[ "$CAPTURE_NATIVE_GATE_RUN_ATTEMPT" =~ ^[1-9][0-9]*$ ]] || {
    echo "GOBLINS_OS_CAPTURE_NATIVE_PACKAGING_GATE_RUN_ATTEMPT must be the positive run attempt that produced the proof." >&2
    exit 1
  }
  case "$(cd "$(dirname "$CAPTURE_NATIVE_GATE_PROOF")" && pwd -P)/$(basename "$CAPTURE_NATIVE_GATE_PROOF")" in
    "$RUN_ROOT"/*)
      echo "Native packaging proof must be staged outside the dated capture root before that root is reset." >&2
      exit 1
      ;;
  esac
  ISO_MANIFEST_SHA="$(sha256_file "$ISO_MANIFEST")"
  BIB_MANIFEST_SHA="$(sha256_file "$BIB_MANIFEST")"
  EVIDENCE_MANIFEST_SHA="$(sha256_file "$EVIDENCE_MANIFEST")"
  python3 - \
    "$CAPTURE_NATIVE_GATE_PROOF" \
    "$ARCH" \
    "$CANDIDATE_COMMIT" \
    "$IMAGE_REF" \
    "$CAPTURE_NATIVE_GATE_RUN_URL" \
    "$CAPTURE_NATIVE_GATE_RUN_ATTEMPT" \
    "$ISO_SHA" \
    "$ISO_MANIFEST_SHA" \
    "$BIB_MANIFEST_SHA" \
    "$EVIDENCE_MANIFEST_SHA" <<'PY'
import json
import sys

(
    path,
    arch,
    commit,
    image_ref,
    run_url,
    run_attempt,
    iso_sha,
    iso_manifest_sha,
    bib_manifest_sha,
    evidence_manifest_sha,
) = sys.argv[1:11]
with open(path, encoding="utf-8") as handle:
    proof = json.load(handle)
source_repository = run_url.split("/actions/runs/", 1)[0]
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
    "source_repository": source_repository,
    "workflow_run": run_url,
    "workflow_run_attempt": int(run_attempt),
}
for key, value in expected.items():
    if proof.get(key) != value:
        raise SystemExit(f"native packaging proof field {key!r} is {proof.get(key)!r}, expected {value!r}")
PY
  NATIVE_GATE_PROOF_RELATIVE="${RUN_DIR#"$REPO/"}/native-packaging-gate.json"
fi

# Accel: KVM on Linux, HVF on macOS.
case "$(uname -s)" in
  Linux)
    if [ ! -e /dev/kvm ] || [ ! -r /dev/kvm ] || [ ! -w /dev/kvm ]; then
      echo "native Linux capture requires readable/writable /dev/kvm for QEMU/KVM" >&2
      [ -e /dev/kvm ] && ls -l /dev/kvm >&2 || true
      exit 1
    fi
    ACCEL=kvm
    CPU=host
    ;;
  Darwin)
    ACCEL=hvf
    CPU=host
    ;;
  *)
    echo "unsupported capture host $(uname -s): need native Linux/KVM or macOS/HVF" >&2
    exit 1
    ;;
esac
QEMU_SMP="${GOBLINS_OS_QEMU_CPUS:-2}"
SCRATCH_DISK_SIZE="${GOBLINS_OS_CAPTURE_DISK_SIZE:-80G}"
pick() { for f in "$@"; do [ -n "$f" ] && [ -f "$f" ] && { echo "$f"; return 0; }; done; return 1; }
VARS_TEMPLATE=""
if [ "$ARCH" = aarch64 ]; then
  MACHINE="virt,accel=$ACCEL,gic-version=max"
  CODE="$(pick "${AARCH64_UEFI_CODE:-}" /opt/homebrew/share/qemu/edk2-aarch64-code.fd /usr/share/AAVMF/AAVMF_CODE.fd /usr/share/edk2/aarch64/QEMU_EFI-silent.fd)"
  VARS_TEMPLATE="$(pick "${AARCH64_UEFI_VARS:-}" /usr/share/AAVMF/AAVMF_VARS.fd || true)"  # empty 64M also works on edk2-aarch64
else
  MACHINE="q35,accel=$ACCEL"
  CODE="$(pick "${X86_UEFI_CODE:-}" /usr/share/OVMF/OVMF_CODE_4M.fd /usr/share/OVMF/OVMF_CODE.fd /usr/share/edk2/ovmf/OVMF_CODE.fd)"
  # x86_64 OVMF requires a real VARS template matching the code build (4M code -> 4M vars).
  case "$CODE" in
    *_4M.fd) VARS_TEMPLATE="$(pick /usr/share/OVMF/OVMF_VARS_4M.fd /usr/share/edk2/ovmf/OVMF_VARS.fd)";;
    *)       VARS_TEMPLATE="$(pick /usr/share/OVMF/OVMF_VARS.fd /usr/share/edk2/ovmf/OVMF_VARS.fd)";;
  esac
fi
[ -n "$CODE" ] || { echo "no UEFI code firmware found for $ARCH"; exit 1; }
PFLASH=(-drive "if=pflash,format=raw,file=$WORK/code.fd,readonly=on" -drive "if=pflash,format=raw,file=$WORK/vars.fd")
QEMU_AUDIO=(-audiodev none,id=audio0 -device ich9-intel-hda -device hda-output,audiodev=audio0)

if [ "$RUN_DIR" != "$RUN_ROOT/$DATE" ] || [ "$(dirname "$RUN_DIR")" != "$RUN_ROOT" ]; then
  echo "refusing to reset unexpected hardware-gate run dir: $RUN_DIR"
  exit 2
fi
if [ -L "$RUN_DIR" ]; then
  rm -f "$RUN_DIR"
else
  rm -rf "$RUN_DIR"
fi
mkdir -p "$WORK" "$RUN_DIR"
cp "$ISO_MANIFEST" "$RUN_DIR/verification-iso-manifest.json"
cp "$BIB_MANIFEST" "$RUN_DIR/verification-bib-manifest.json"
if [ -s "$EVIDENCE_MANIFEST" ]; then
  cp "$EVIDENCE_MANIFEST" "$RUN_DIR/verification-release-evidence-manifest.json"
fi
VERIFICATION_EVIDENCE_MANIFEST_SHA="$(sha256_file "$RUN_DIR/verification-release-evidence-manifest.json")"
if [ -n "$NATIVE_GATE_PROOF_RELATIVE" ]; then
  cp "$CAPTURE_NATIVE_GATE_PROOF" "$RUN_DIR/native-packaging-gate.json"
fi

stop_qemu() {
  if [ -n "${QEMU_PID:-}" ]; then
    kill "$QEMU_PID" 2>/dev/null || true
    wait "$QEMU_PID" 2>/dev/null || true
    QEMU_PID=""
  fi
}

wait_for_qemu_exit() {
  local label="$1"
  local timeout="$2"
  local start now
  start="$(date +%s)"
  while [ -n "${QEMU_PID:-}" ] && kill -0 "$QEMU_PID" 2>/dev/null; do
    now="$(date +%s)"
    if [ $((now - start)) -ge "$timeout" ]; then
      echo "$label: QEMU did not exit within ${timeout}s"
      return 1
    fi
    sleep 2
  done
  if [ -n "${QEMU_PID:-}" ]; then
    wait "$QEMU_PID" 2>/dev/null || true
    QEMU_PID=""
  fi
}

prepare_vm_state() {
  local attempt="$1"
  echo "capture attempt $attempt: preparing fresh VM state"
  rm -f "$WORK/qmp.sock" "$WORK/serial.log" "$WORK/qemu.log" "$WORK/scratch.qcow2" "$WORK/orchestrator.sh"
  cp "$CODE" "$WORK/code.fd"
  if [ -n "$VARS_TEMPLATE" ]; then
    cp "$VARS_TEMPLATE" "$WORK/vars.fd"
  else
    : > "$WORK/vars.fd"; truncate -s 67108864 "$WORK/vars.fd" 2>/dev/null || dd if=/dev/zero of="$WORK/vars.fd" bs=1m count=64 2>/dev/null
  fi
  qemu-img create -f qcow2 "$WORK/scratch.qcow2" "$SCRATCH_DISK_SIZE" >/dev/null
}

start_qemu() {
  local attempt="$1"
  local phase="${2:-full}"
  local boot_args=()
  case "$ARCH:$phase" in
    aarch64:install)
      boot_args=(
        -drive "if=none,id=install_iso,file=$ISO,media=cdrom,readonly=on"
        -drive "file=$WORK/scratch.qcow2,if=virtio,format=qcow2"
        -device usb-storage,drive=install_iso,bootindex=1
        -no-reboot
      )
      ;;
    aarch64:firstboot)
      boot_args=(-drive "file=$WORK/scratch.qcow2,if=virtio,format=qcow2")
      ;;
    *)
      boot_args=(-cdrom "$ISO" -drive "file=$WORK/scratch.qcow2,if=virtio,format=qcow2" -boot order=c,once=d)
      ;;
  esac
  echo "capture attempt $attempt: starting QEMU ($phase)"
  "$QEMU" -machine "$MACHINE" -cpu "$CPU" -smp "$QEMU_SMP" -m 5120 "${PFLASH[@]}" \
    -netdev user,id=net0 -device virtio-net-pci,netdev=net0 \
    -device qemu-xhci "${boot_args[@]}" \
    -device virtio-gpu-pci,id=video0 -device usb-tablet -device usb-kbd \
    "${QEMU_AUDIO[@]}" \
    -serial file:"$WORK/serial.log" -display none -qmp "unix:$WORK/qmp.sock,server,nowait" >"$WORK/qemu.log" 2>&1 &
  QEMU_PID=$!
  CAPTURE_STARTED=1
  export GOS_QMP="$WORK/qmp.sock" GOS_SERIALLOG="$WORK/serial.log" GOS_HTTPLOG="$WORK/httpd.log" GOS_OUTDIR="$RUN_DIR" GOS_PORT="$PORT" GOS_QMP_DISPLAY_DEVICE=video0
  export GOS_ORCHESTRATOR_SOURCE="$HERE/in-session-orchestrator.sh" GOS_ORCHESTRATOR_DEST="$WORK/orchestrator.sh"
}

run_driver() {
  local phase="${1:-full}"
  set +e
  case "$phase" in
    install-marker)
      GOS_EXIT_AFTER_INSTALL_MARKER="$INSTALL_MARKER_RC" python3 "$HERE/drive-capture.py"
      ;;
    firstboot)
      GOS_SKIP_INSTALL_PHASE=1 python3 "$HERE/drive-capture.py"
      ;;
    *)
      python3 "$HERE/drive-capture.py"
      ;;
  esac
  local driver_rc=$?
  set -e
  return "$driver_rc"
}

# Serve first-boot helper and receive capture signals. The orchestrator is
# published by drive-capture.py only after the host has recorded the post-unlock
# log offset, so early /ready signals cannot race ahead of the screenshot tailer.
rm -f "$WORK/orchestrator.sh" "$WORK/core-proof-operation.sh"
( cd "$WORK" \
    && sed "s/@GOS_PORT@/$PORT/g" "$HERE/firstboot-unlock.sh" > firstboot-unlock.sh \
    && install -m 0644 "$HERE/core-proof-operation.sh" core-proof-operation.sh \
    && install -d -m 0755 ready failed \
    && install -m 0644 /dev/null ready/FIRSTBOOT_UNLOCK \
    && install -m 0644 /dev/null failed/FIRSTBOOT_UNLOCK \
    && python3 -m http.server "$PORT" --bind 0.0.0.0 >"$WORK/httpd.log" 2>&1 ) &
HTTPD=$!

# Phase the run with the QMP driver (waits for Anaconda, drives it, waits for the
# desktop, dismisses onboarding, launches the orchestrator, captures on signals).
MAX_ATTEMPTS="${GOS_CAPTURE_MAX_ATTEMPTS:-2}"
INSTALL_TIMEOUT_RC="${GOS_INSTALL_POST_TIMEOUT_EXIT:-70}"
attempt=1
while [ "$attempt" -le "$MAX_ATTEMPTS" ]; do
  prepare_vm_state "$attempt"
  if [ "$ARCH" = aarch64 ]; then
    start_qemu "$attempt" install
    if run_driver install-marker; then
      break
    else
      driver_rc=$?
    fi
    copy_capture_logs "attempt-$attempt-install"
    if [ "$driver_rc" -eq "$INSTALL_MARKER_RC" ]; then
      if ! wait_for_qemu_exit "capture attempt $attempt install reboot" "$AARCH64_INSTALL_REBOOT_TIMEOUT"; then
        exit 1
      fi
      start_qemu "$attempt" firstboot
      if run_driver firstboot; then
        break
      else
        driver_rc=$?
      fi
    fi
  else
    start_qemu "$attempt" full
    if run_driver full; then
      break
    else
      driver_rc=$?
    fi
  fi
  copy_capture_logs "attempt-$attempt"
  if [ "$driver_rc" -eq "$INSTALL_TIMEOUT_RC" ] && [ "$attempt" -lt "$MAX_ATTEMPTS" ]; then
    echo "capture attempt $attempt stalled before kickstart marker; retrying with fresh VM state"
    stop_qemu
    attempt=$((attempt + 1))
    continue
  fi
  exit "$driver_rc"
done

FIREWALL_PROOF="$RUN_DIR/firewall-live-toggle-proof.json"
TEXT_SHORTCUTS_PROOF="$RUN_DIR/text-shortcuts-session-enable-proof.json"
TEXT_SHORTCUTS_CANDIDATE_PROOF="$RUN_DIR/text-shortcuts-candidate-metadata-proof.json"
TEXT_SHORTCUTS_OVERLAY_INTENT_PROOF="$RUN_DIR/text-shortcuts-overlay-intent-proof.json"
TEXT_SHORTCUTS_CANDIDATE_BUBBLE_FRAME_PROOF="$RUN_DIR/text-shortcuts-candidate-bubble-frame-proof.json"
TEXT_SHORTCUTS_CANDIDATE_BUBBLE_LAYOUT_PROOF="$RUN_DIR/text-shortcuts-candidate-bubble-layout-proof.json"
TEXT_SHORTCUTS_CANDIDATE_BUBBLE_RENDER_INTENT_PROOF="$RUN_DIR/text-shortcuts-candidate-bubble-render-intent-proof.json"
TEXT_SHORTCUTS_CANDIDATE_BUBBLE_RENDER_PROOF="$RUN_DIR/text-shortcuts-candidate-bubble-render-proof.json"
TEXT_SHORTCUTS_LIVE_IBUS_RUNTIME_RENDER_PROOF="$RUN_DIR/text-shortcuts-live-ibus-runtime-render-proof.json"
KEYBOARD_SHORTCUTS_ROUNDTRIP_PROOF="$RUN_DIR/keyboard-shortcuts-roundtrip-proof.json"
INPUT_SOURCES_ROUNDTRIP_PROOF="$RUN_DIR/input-sources-roundtrip-proof.json"
MULTI_DISPLAY_APPLY_PROOF="$RUN_DIR/multi-display-apply-proof.json"
FOCUS_ARM_ROUNDTRIP_PROOF="$RUN_DIR/focus-arm-roundtrip-proof.json"
APP_PRIVACY_REVOKE_PROOF="$RUN_DIR/app-privacy-revoke-proof.json"
PREVIEW_OPEN_RENDER_PROOF="$RUN_DIR/preview-open-render-proof.json"
RUNTIME_BUILD_PROOF="$RUN_DIR/runtime-build-proof.json"
if ! grep -Fq '"status": "pass"' "$FIREWALL_PROOF" \
  || ! grep -Fq '"disable_http": "200"' "$FIREWALL_PROOF" \
  || ! grep -Fq '"disable_active": "false"' "$FIREWALL_PROOF" \
  || ! grep -Fq '"enable_http": "200"' "$FIREWALL_PROOF" \
  || ! grep -Fq '"enable_active": "true"' "$FIREWALL_PROOF"; then
  echo "HONESTY GUARD: missing or failing live firewall toggle proof at $FIREWALL_PROOF"
  exit 4
fi
if ! grep -Fq '"status": "pass"' "$TEXT_SHORTCUTS_PROOF" \
  || ! grep -Fq '"service": "active"' "$TEXT_SHORTCUTS_PROOF" \
  || ! grep -Fq '"service_unit": "org.freedesktop.IBus.session.GNOME.service"' "$TEXT_SHORTCUTS_PROOF" \
  || ! grep -Fq '"input_source_configured": "true"' "$TEXT_SHORTCUTS_PROOF" \
  || ! grep -Fq '"preload_configured": "true"' "$TEXT_SHORTCUTS_PROOF" \
  || ! grep -Fq '"engine_listed": "true"' "$TEXT_SHORTCUTS_PROOF" \
  || ! grep -Fq '"active_engine": "goblins-textshortcuts"' "$TEXT_SHORTCUTS_PROOF" \
  || ! grep -Fq '"adapter_self_test": "pass"' "$TEXT_SHORTCUTS_PROOF" \
  || ! grep -Fq '"core_http": "200"' "$TEXT_SHORTCUTS_PROOF" \
  || ! grep -Fq '"core_engine_available": "true"' "$TEXT_SHORTCUTS_PROOF" \
  || ! grep -Fq '"core_runtime_loop_available": "true"' "$TEXT_SHORTCUTS_PROOF" \
  || ! grep -Fq '"runtime_ready_claim": "true"' "$TEXT_SHORTCUTS_PROOF"; then
  echo "HONESTY GUARD: missing or failing Text Shortcuts session-enable proof at $TEXT_SHORTCUTS_PROOF"
  exit 4
fi
if ! grep -Fq '"status": "pass"' "$RUNTIME_BUILD_PROOF" \
  || ! grep -Fq '"route": "/v1/apps/builds"' "$RUNTIME_BUILD_PROOF" \
  || ! grep -Fq '"engine_mode": "local-model"' "$RUNTIME_BUILD_PROOF" \
  || ! rg -q '"engine_source"[[:space:]]*:[[:space:]]*"[A-Za-z0-9._:-]+-built"' "$RUNTIME_BUILD_PROOF"; then
  echo "HONESTY GUARD: missing or failing runtime app-build proof at $RUNTIME_BUILD_PROOF"
  exit 4
fi
if ! grep -Fq '"status": "pass"' "$TEXT_SHORTCUTS_CANDIDATE_PROOF" \
  || ! grep -Fq '"surface": "goblins-os-shell-text-shortcuts-candidate-proof"' "$TEXT_SHORTCUTS_CANDIDATE_PROOF" \
  || ! grep -Fq '"candidate_replacement": "on my way"' "$TEXT_SHORTCUTS_CANDIDATE_PROOF" \
  || ! grep -Fq '"candidate_accept_on": "word-boundary"' "$TEXT_SHORTCUTS_CANDIDATE_PROOF" \
  || ! grep -Fq '"candidate_dismiss_key": "Escape"' "$TEXT_SHORTCUTS_CANDIDATE_PROOF" \
  || ! grep -Fq '"rendered_bubble_ready_claim": "false"' "$TEXT_SHORTCUTS_CANDIDATE_PROOF" \
  || ! grep -Fq '"live_overlay_claim": "false"' "$TEXT_SHORTCUTS_CANDIDATE_PROOF" \
  || ! grep -Fq '"runtime_ready_claim": "false"' "$TEXT_SHORTCUTS_CANDIDATE_PROOF"; then
  echo "HONESTY GUARD: missing or failing Text Shortcuts candidate metadata proof at $TEXT_SHORTCUTS_CANDIDATE_PROOF"
  exit 4
fi
if ! grep -Fq '"status": "pass"' "$TEXT_SHORTCUTS_OVERLAY_INTENT_PROOF" \
  || ! grep -Fq '"surface": "goblins-textshortcuts-ibus-adapter-overlay-intent"' "$TEXT_SHORTCUTS_OVERLAY_INTENT_PROOF" \
  || ! grep -Fq '"adapter_self_test": "pass"' "$TEXT_SHORTCUTS_OVERLAY_INTENT_PROOF" \
  || ! grep -Fq '"show_count": "2"' "$TEXT_SHORTCUTS_OVERLAY_INTENT_PROOF" \
  || ! grep -Fq '"hide_count": "2"' "$TEXT_SHORTCUTS_OVERLAY_INTENT_PROOF" \
  || ! grep -Fq '"dismissed_reason": "true"' "$TEXT_SHORTCUTS_OVERLAY_INTENT_PROOF" \
  || ! grep -Fq '"committed_reason": "true"' "$TEXT_SHORTCUTS_OVERLAY_INTENT_PROOF" \
  || ! grep -Fq '"rendered_bubble_ready_claim": "false"' "$TEXT_SHORTCUTS_OVERLAY_INTENT_PROOF" \
  || ! grep -Fq '"live_overlay_claim": "false"' "$TEXT_SHORTCUTS_OVERLAY_INTENT_PROOF" \
  || ! grep -Fq '"runtime_ready_claim": "false"' "$TEXT_SHORTCUTS_OVERLAY_INTENT_PROOF"; then
  echo "HONESTY GUARD: missing or failing Text Shortcuts overlay-intent proof at $TEXT_SHORTCUTS_OVERLAY_INTENT_PROOF"
  exit 4
fi
if ! grep -Fq '"status": "pass"' "$TEXT_SHORTCUTS_CANDIDATE_BUBBLE_FRAME_PROOF" \
  || ! grep -Fq '"surface": "goblins-textshortcuts-accept-bubble-frame"' "$TEXT_SHORTCUTS_CANDIDATE_BUBBLE_FRAME_PROOF" \
  || ! grep -Fq '"adapter_self_test": "pass"' "$TEXT_SHORTCUTS_CANDIDATE_BUBBLE_FRAME_PROOF" \
  || ! grep -Fq '"show_frame_count": "2"' "$TEXT_SHORTCUTS_CANDIDATE_BUBBLE_FRAME_PROOF" \
  || ! grep -Fq '"hide_frame_count": "2"' "$TEXT_SHORTCUTS_CANDIDATE_BUBBLE_FRAME_PROOF" \
  || ! grep -Fq '"dismissed_frame": "true"' "$TEXT_SHORTCUTS_CANDIDATE_BUBBLE_FRAME_PROOF" \
  || ! grep -Fq '"committed_frame": "true"' "$TEXT_SHORTCUTS_CANDIDATE_BUBBLE_FRAME_PROOF" \
  || ! grep -Fq '"replacement": "on my way"' "$TEXT_SHORTCUTS_CANDIDATE_BUBBLE_FRAME_PROOF" \
  || ! grep -Fq '"accept_on": "word-boundary"' "$TEXT_SHORTCUTS_CANDIDATE_BUBBLE_FRAME_PROOF" \
  || ! grep -Fq '"accept_keys": "Space,Return"' "$TEXT_SHORTCUTS_CANDIDATE_BUBBLE_FRAME_PROOF" \
  || ! grep -Fq '"dismiss_key": "Escape"' "$TEXT_SHORTCUTS_CANDIDATE_BUBBLE_FRAME_PROOF" \
  || ! grep -Fq '"style_class": "gos-text-shortcuts-candidate"' "$TEXT_SHORTCUTS_CANDIDATE_BUBBLE_FRAME_PROOF" \
  || ! grep -Fq '"text_style_class": "gos-text-shortcuts-candidate-text"' "$TEXT_SHORTCUTS_CANDIDATE_BUBBLE_FRAME_PROOF" \
  || ! grep -Fq '"hint_style_class": "gos-text-shortcuts-candidate-hint"' "$TEXT_SHORTCUTS_CANDIDATE_BUBBLE_FRAME_PROOF" \
  || ! grep -Fq '"font_family": "Inter"' "$TEXT_SHORTCUTS_CANDIDATE_BUBBLE_FRAME_PROOF" \
  || ! grep -Fq '"sensitive_field_refusal": "true"' "$TEXT_SHORTCUTS_CANDIDATE_BUBBLE_FRAME_PROOF" \
  || ! grep -Fq '"rendered_bubble_ready_claim": "false"' "$TEXT_SHORTCUTS_CANDIDATE_BUBBLE_FRAME_PROOF" \
  || ! grep -Fq '"live_overlay_claim": "false"' "$TEXT_SHORTCUTS_CANDIDATE_BUBBLE_FRAME_PROOF" \
  || ! grep -Fq '"runtime_ready_claim": "false"' "$TEXT_SHORTCUTS_CANDIDATE_BUBBLE_FRAME_PROOF"; then
  echo "HONESTY GUARD: missing or failing Text Shortcuts candidate-bubble-frame proof at $TEXT_SHORTCUTS_CANDIDATE_BUBBLE_FRAME_PROOF"
  exit 4
fi
if ! grep -Fq '"status": "pass"' "$TEXT_SHORTCUTS_CANDIDATE_BUBBLE_LAYOUT_PROOF" \
  || ! grep -Fq '"surface": "goblins-textshortcuts-accept-bubble-layout"' "$TEXT_SHORTCUTS_CANDIDATE_BUBBLE_LAYOUT_PROOF" \
  || ! grep -Fq '"adapter_self_test": "pass"' "$TEXT_SHORTCUTS_CANDIDATE_BUBBLE_LAYOUT_PROOF" \
  || ! grep -Fq '"frame_surface": "goblins-textshortcuts-accept-bubble-frame"' "$TEXT_SHORTCUTS_CANDIDATE_BUBBLE_LAYOUT_PROOF" \
  || ! grep -Fq '"layout_count": "4"' "$TEXT_SHORTCUTS_CANDIDATE_BUBBLE_LAYOUT_PROOF" \
  || ! grep -Fq '"visible_layout_count": "3"' "$TEXT_SHORTCUTS_CANDIDATE_BUBBLE_LAYOUT_PROOF" \
  || ! grep -Fq '"right_edge_clamped": "true"' "$TEXT_SHORTCUTS_CANDIDATE_BUBBLE_LAYOUT_PROOF" \
  || ! grep -Fq '"bottom_edge_flipped": "true"' "$TEXT_SHORTCUTS_CANDIDATE_BUBBLE_LAYOUT_PROOF" \
  || ! grep -Fq '"hidden_frame_collapses": "true"' "$TEXT_SHORTCUTS_CANDIDATE_BUBBLE_LAYOUT_PROOF" \
  || ! grep -Fq '"style_class": "gos-text-shortcuts-candidate"' "$TEXT_SHORTCUTS_CANDIDATE_BUBBLE_LAYOUT_PROOF" \
  || ! grep -Fq '"font_family": "Inter"' "$TEXT_SHORTCUTS_CANDIDATE_BUBBLE_LAYOUT_PROOF" \
  || ! grep -Fq '"rendered_bubble_ready_claim": "false"' "$TEXT_SHORTCUTS_CANDIDATE_BUBBLE_LAYOUT_PROOF" \
  || ! grep -Fq '"live_overlay_claim": "false"' "$TEXT_SHORTCUTS_CANDIDATE_BUBBLE_LAYOUT_PROOF" \
  || ! grep -Fq '"runtime_ready_claim": "false"' "$TEXT_SHORTCUTS_CANDIDATE_BUBBLE_LAYOUT_PROOF"; then
  echo "HONESTY GUARD: missing or failing Text Shortcuts candidate-bubble-layout proof at $TEXT_SHORTCUTS_CANDIDATE_BUBBLE_LAYOUT_PROOF"
  exit 4
fi
if ! grep -Fq '"status": "pass"' "$TEXT_SHORTCUTS_CANDIDATE_BUBBLE_RENDER_INTENT_PROOF" \
  || ! grep -Fq '"surface": "goblins-textshortcuts-accept-bubble-render-intent"' "$TEXT_SHORTCUTS_CANDIDATE_BUBBLE_RENDER_INTENT_PROOF" \
  || ! grep -Fq '"adapter_self_test": "pass"' "$TEXT_SHORTCUTS_CANDIDATE_BUBBLE_RENDER_INTENT_PROOF" \
  || ! grep -Fq '"frame_surface": "goblins-textshortcuts-accept-bubble-frame"' "$TEXT_SHORTCUTS_CANDIDATE_BUBBLE_RENDER_INTENT_PROOF" \
  || ! grep -Fq '"layout_surface": "goblins-textshortcuts-accept-bubble-layout"' "$TEXT_SHORTCUTS_CANDIDATE_BUBBLE_RENDER_INTENT_PROOF" \
  || ! grep -Fq '"render_intent_count": "8"' "$TEXT_SHORTCUTS_CANDIDATE_BUBBLE_RENDER_INTENT_PROOF" \
  || ! grep -Fq '"show_intent_count": "4"' "$TEXT_SHORTCUTS_CANDIDATE_BUBBLE_RENDER_INTENT_PROOF" \
  || ! grep -Fq '"hide_intent_count": "4"' "$TEXT_SHORTCUTS_CANDIDATE_BUBBLE_RENDER_INTENT_PROOF" \
  || ! grep -Fq '"dismissed_intent": "true"' "$TEXT_SHORTCUTS_CANDIDATE_BUBBLE_RENDER_INTENT_PROOF" \
  || ! grep -Fq '"committed_intent": "true"' "$TEXT_SHORTCUTS_CANDIDATE_BUBBLE_RENDER_INTENT_PROOF" \
  || ! grep -Fq '"focus_out_hide": "true"' "$TEXT_SHORTCUTS_CANDIDATE_BUBBLE_RENDER_INTENT_PROOF" \
  || ! grep -Fq '"sensitive_hide": "true"' "$TEXT_SHORTCUTS_CANDIDATE_BUBBLE_RENDER_INTENT_PROOF" \
  || ! grep -Fq '"pass_through_unchanged": "true"' "$TEXT_SHORTCUTS_CANDIDATE_BUBBLE_RENDER_INTENT_PROOF" \
  || ! grep -Fq '"sink_failure_fail_open": "true"' "$TEXT_SHORTCUTS_CANDIDATE_BUBBLE_RENDER_INTENT_PROOF" \
  || ! grep -Fq '"style_class": "gos-text-shortcuts-candidate"' "$TEXT_SHORTCUTS_CANDIDATE_BUBBLE_RENDER_INTENT_PROOF" \
  || ! grep -Fq '"font_family": "Inter"' "$TEXT_SHORTCUTS_CANDIDATE_BUBBLE_RENDER_INTENT_PROOF" \
  || ! grep -Fq '"rendered_bubble_ready_claim": "false"' "$TEXT_SHORTCUTS_CANDIDATE_BUBBLE_RENDER_INTENT_PROOF" \
  || ! grep -Fq '"live_overlay_claim": "false"' "$TEXT_SHORTCUTS_CANDIDATE_BUBBLE_RENDER_INTENT_PROOF" \
  || ! grep -Fq '"runtime_ready_claim": "false"' "$TEXT_SHORTCUTS_CANDIDATE_BUBBLE_RENDER_INTENT_PROOF"; then
  echo "HONESTY GUARD: missing or failing Text Shortcuts candidate-bubble-render-intent proof at $TEXT_SHORTCUTS_CANDIDATE_BUBBLE_RENDER_INTENT_PROOF"
  exit 4
fi
if ! grep -Fq '"status": "pass"' "$TEXT_SHORTCUTS_CANDIDATE_BUBBLE_RENDER_PROOF" \
  || ! grep -Fq '"surface": "goblins-os-shell-text-shortcuts-candidate-bubble-render"' "$TEXT_SHORTCUTS_CANDIDATE_BUBBLE_RENDER_PROOF" \
  || ! grep -Fq '"render_intent_surface": "goblins-textshortcuts-accept-bubble-render-intent"' "$TEXT_SHORTCUTS_CANDIDATE_BUBBLE_RENDER_PROOF" \
  || ! grep -Fq '"layout_surface": "goblins-textshortcuts-accept-bubble-layout"' "$TEXT_SHORTCUTS_CANDIDATE_BUBBLE_RENDER_PROOF" \
  || ! grep -Fq '"frame_surface": "goblins-textshortcuts-accept-bubble-frame"' "$TEXT_SHORTCUTS_CANDIDATE_BUBBLE_RENDER_PROOF" \
  || ! grep -Fq '"screenshot": "31-text-shortcuts-candidate-bubble-render.png"' "$TEXT_SHORTCUTS_CANDIDATE_BUBBLE_RENDER_PROOF" \
  || ! grep -Fq '"rendered_candidate_surface": "true"' "$TEXT_SHORTCUTS_CANDIDATE_BUBBLE_RENDER_PROOF" \
  || ! grep -Fq '"style_class": "gos-text-shortcuts-candidate"' "$TEXT_SHORTCUTS_CANDIDATE_BUBBLE_RENDER_PROOF" \
  || ! grep -Fq '"font_family": "Inter"' "$TEXT_SHORTCUTS_CANDIDATE_BUBBLE_RENDER_PROOF" \
  || ! grep -Fq '"rendered_bubble_ready_claim": "false"' "$TEXT_SHORTCUTS_CANDIDATE_BUBBLE_RENDER_PROOF" \
  || ! grep -Fq '"live_overlay_claim": "false"' "$TEXT_SHORTCUTS_CANDIDATE_BUBBLE_RENDER_PROOF" \
  || ! grep -Fq '"runtime_ready_claim": "false"' "$TEXT_SHORTCUTS_CANDIDATE_BUBBLE_RENDER_PROOF" \
  || [ ! -s "$RUN_DIR/31-text-shortcuts-candidate-bubble-render.png" ]; then
  echo "HONESTY GUARD: missing or failing Text Shortcuts candidate-bubble-render screenshot proof at $TEXT_SHORTCUTS_CANDIDATE_BUBBLE_RENDER_PROOF"
  exit 4
fi
if ! grep -Fq '"status": "pass"' "$TEXT_SHORTCUTS_LIVE_IBUS_RUNTIME_RENDER_PROOF" \
  || ! grep -Fq '"route": "/v1/text-shortcuts"' "$TEXT_SHORTCUTS_LIVE_IBUS_RUNTIME_RENDER_PROOF" \
  || ! grep -Fq '"surface": "goblins-textshortcuts-live-ibus-runtime-render"' "$TEXT_SHORTCUTS_LIVE_IBUS_RUNTIME_RENDER_PROOF" \
  || ! grep -Fq '"input_driver": "qmp-keyboard"' "$TEXT_SHORTCUTS_LIVE_IBUS_RUNTIME_RENDER_PROOF" \
  || ! grep -Fq '"active_engine": "goblins-textshortcuts"' "$TEXT_SHORTCUTS_LIVE_IBUS_RUNTIME_RENDER_PROOF" \
  || ! grep -Fq '"normal_actual": "onmyway."' "$TEXT_SHORTCUTS_LIVE_IBUS_RUNTIME_RENDER_PROOF" \
  || ! grep -Fq '"passthrough_actual": "hello."' "$TEXT_SHORTCUTS_LIVE_IBUS_RUNTIME_RENDER_PROOF" \
  || ! grep -Fq '"password_refusal": "true"' "$TEXT_SHORTCUTS_LIVE_IBUS_RUNTIME_RENDER_PROOF" \
  || ! grep -Fq '"focused_field_callback": "true"' "$TEXT_SHORTCUTS_LIVE_IBUS_RUNTIME_RENDER_PROOF" \
  || ! grep -Fq '"text_input_v3_commit": "true"' "$TEXT_SHORTCUTS_LIVE_IBUS_RUNTIME_RENDER_PROOF" \
  || ! grep -Fq '"rendered_accept_bubble": "true"' "$TEXT_SHORTCUTS_LIVE_IBUS_RUNTIME_RENDER_PROOF" \
  || ! grep -Fq '"screenshot": "32-text-shortcuts-live-ibus-runtime-render.png"' "$TEXT_SHORTCUTS_LIVE_IBUS_RUNTIME_RENDER_PROOF" \
  || ! grep -Fq '"style_class": "gos-text-shortcuts-candidate"' "$TEXT_SHORTCUTS_LIVE_IBUS_RUNTIME_RENDER_PROOF" \
  || ! grep -Fq '"font_family": "Inter"' "$TEXT_SHORTCUTS_LIVE_IBUS_RUNTIME_RENDER_PROOF" \
  || ! grep -Fq '"rendered_bubble_ready_claim": "true"' "$TEXT_SHORTCUTS_LIVE_IBUS_RUNTIME_RENDER_PROOF" \
  || ! grep -Fq '"live_overlay_claim": "true"' "$TEXT_SHORTCUTS_LIVE_IBUS_RUNTIME_RENDER_PROOF" \
  || ! grep -Fq '"runtime_ready_claim": "true"' "$TEXT_SHORTCUTS_LIVE_IBUS_RUNTIME_RENDER_PROOF" \
  || ! grep -Fq '"core_readiness_flip": "live"' "$TEXT_SHORTCUTS_LIVE_IBUS_RUNTIME_RENDER_PROOF" \
  || [ ! -s "$RUN_DIR/32-text-shortcuts-live-ibus-runtime-render.png" ]; then
  echo "HONESTY GUARD: missing or failing Text Shortcuts live IBus runtime/render proof at $TEXT_SHORTCUTS_LIVE_IBUS_RUNTIME_RENDER_PROOF"
  exit 4
fi
if ! grep -Fq '"status": "pass"' "$KEYBOARD_SHORTCUTS_ROUNDTRIP_PROOF" \
  || ! grep -Fq '"shortcut_route": "/v1/keyboard/shortcuts/binding"' "$KEYBOARD_SHORTCUTS_ROUNDTRIP_PROOF" \
  || ! grep -Fq '"modifier_route": "/v1/keyboard/modifier-remap"' "$KEYBOARD_SHORTCUTS_ROUNDTRIP_PROOF" \
  || ! grep -Fq '"shortcut_action": "window-hud"' "$KEYBOARD_SHORTCUTS_ROUNDTRIP_PROOF" \
  || ! grep -Fq '"shortcut_binding": "<Super><Shift>H"' "$KEYBOARD_SHORTCUTS_ROUNDTRIP_PROOF" \
  || ! grep -Fq '"shortcut_http": "200"' "$KEYBOARD_SHORTCUTS_ROUNDTRIP_PROOF" \
  || ! grep -Fq '"shortcut_gsettings_readback": "true"' "$KEYBOARD_SHORTCUTS_ROUNDTRIP_PROOF" \
  || ! grep -Fq '"shortcut_reset_http": "200"' "$KEYBOARD_SHORTCUTS_ROUNDTRIP_PROOF" \
  || ! grep -Fq '"shortcut_reset_binding": "<Super>w"' "$KEYBOARD_SHORTCUTS_ROUNDTRIP_PROOF" \
  || ! grep -Fq '"modifier_target": "caps-lock"' "$KEYBOARD_SHORTCUTS_ROUNDTRIP_PROOF" \
  || ! grep -Fq '"modifier_value": "control"' "$KEYBOARD_SHORTCUTS_ROUNDTRIP_PROOF" \
  || ! grep -Fq '"modifier_http": "200"' "$KEYBOARD_SHORTCUTS_ROUNDTRIP_PROOF" \
  || ! grep -Fq '"modifier_gsettings_readback": "ctrl:nocaps"' "$KEYBOARD_SHORTCUTS_ROUNDTRIP_PROOF" \
  || ! grep -Fq '"modifier_reset_http": "200"' "$KEYBOARD_SHORTCUTS_ROUNDTRIP_PROOF" \
  || ! grep -Fq '"modifier_restore": "default"' "$KEYBOARD_SHORTCUTS_ROUNDTRIP_PROOF" \
  || ! grep -Fq '"roundtrip_restored": "true"' "$KEYBOARD_SHORTCUTS_ROUNDTRIP_PROOF"; then
  echo "HONESTY GUARD: missing or failing Keyboard shortcuts roundtrip proof at $KEYBOARD_SHORTCUTS_ROUNDTRIP_PROOF"
  exit 4
fi
if ! grep -Fq '"status": "pass"' "$INPUT_SOURCES_ROUNDTRIP_PROOF" \
  || ! grep -Fq '"source_route": "/v1/input/sources"' "$INPUT_SOURCES_ROUNDTRIP_PROOF" \
  || ! grep -Fq '"switch_route": "/v1/input/switch-next"' "$INPUT_SOURCES_ROUNDTRIP_PROOF" \
  || ! grep -Fq '"test_sources": "xkb-us,xkb-gb"' "$INPUT_SOURCES_ROUNDTRIP_PROOF" \
  || ! grep -Fq '"set_http": "200"' "$INPUT_SOURCES_ROUNDTRIP_PROOF" \
  || ! grep -Fq '"set_ok": "true"' "$INPUT_SOURCES_ROUNDTRIP_PROOF" \
  || ! grep -Fq '"sources_gsettings_readback": "true"' "$INPUT_SOURCES_ROUNDTRIP_PROOF" \
  || ! grep -Fq '"current_before_switch": "0"' "$INPUT_SOURCES_ROUNDTRIP_PROOF" \
  || ! grep -Fq '"switch_http": "200"' "$INPUT_SOURCES_ROUNDTRIP_PROOF" \
  || ! grep -Fq '"switch_ok": "true"' "$INPUT_SOURCES_ROUNDTRIP_PROOF" \
  || ! grep -Fq '"switch_switched": "true"' "$INPUT_SOURCES_ROUNDTRIP_PROOF" \
  || ! grep -Fq '"current_after_switch": "1"' "$INPUT_SOURCES_ROUNDTRIP_PROOF" \
  || ! grep -Fq '"restore_sources": "true"' "$INPUT_SOURCES_ROUNDTRIP_PROOF" \
  || ! grep -Fq '"restore_current": "true"' "$INPUT_SOURCES_ROUNDTRIP_PROOF" \
  || ! grep -Fq '"roundtrip_restored": "true"' "$INPUT_SOURCES_ROUNDTRIP_PROOF"; then
  echo "HONESTY GUARD: missing or failing Input sources roundtrip proof at $INPUT_SOURCES_ROUNDTRIP_PROOF"
  exit 4
fi
if ! grep -Fq '"status": "pass"' "$MULTI_DISPLAY_APPLY_PROOF" \
  || ! grep -Fq '"status_route": "/v1/displays/status"' "$MULTI_DISPLAY_APPLY_PROOF" \
  || ! grep -Fq '"apply_route": "/v1/displays/apply"' "$MULTI_DISPLAY_APPLY_PROOF" \
  || ! grep -Fq '"display_config": "org.gnome.Mutter.DisplayConfig"' "$MULTI_DISPLAY_APPLY_PROOF" \
  || ! grep -Fq '"verify_http": "200"' "$MULTI_DISPLAY_APPLY_PROOF" \
  || ! grep -Fq '"verify_ok": "true"' "$MULTI_DISPLAY_APPLY_PROOF" \
  || ! grep -Fq '"temporary_http": "200"' "$MULTI_DISPLAY_APPLY_PROOF" \
  || ! grep -Fq '"temporary_ok": "true"' "$MULTI_DISPLAY_APPLY_PROOF" \
  || ! grep -Fq '"persistent_guard_http": "400"' "$MULTI_DISPLAY_APPLY_PROOF" \
  || ! grep -Fq '"persistent_confirmation_required": "true"' "$MULTI_DISPLAY_APPLY_PROOF" \
  || ! grep -Fq '"stale_serial_http": "409"' "$MULTI_DISPLAY_APPLY_PROOF" \
  || ! grep -Fq '"stale_serial_rejected": "true"' "$MULTI_DISPLAY_APPLY_PROOF" \
  || ! grep -Fq '"roundtrip_restored": "true"' "$MULTI_DISPLAY_APPLY_PROOF" \
  || ! grep -Fq '"persistent_keep_claim": "false"' "$MULTI_DISPLAY_APPLY_PROOF" \
  || ! grep -Fq '"same_layout_noop": "true"' "$MULTI_DISPLAY_APPLY_PROOF"; then
  echo "HONESTY GUARD: missing or failing multi-display apply proof at $MULTI_DISPLAY_APPLY_PROOF"
  exit 4
fi
if ! grep -Fq '"status": "pass"' "$FOCUS_ARM_ROUNDTRIP_PROOF" \
  || ! grep -Fq '"status_route": "/v1/focus/status"' "$FOCUS_ARM_ROUNDTRIP_PROOF" \
  || ! grep -Fq '"activate_route": "/v1/focus/activate"' "$FOCUS_ARM_ROUNDTRIP_PROOF" \
  || ! grep -Fq '"deactivate_route": "/v1/focus/deactivate"' "$FOCUS_ARM_ROUNDTRIP_PROOF" \
  || ! grep -Fq '"test_mode": "gate-work"' "$FOCUS_ARM_ROUNDTRIP_PROOF" \
  || ! grep -Fq '"test_mode_configured": "true"' "$FOCUS_ARM_ROUNDTRIP_PROOF" \
  || ! grep -Fq '"activate_http": "200"' "$FOCUS_ARM_ROUNDTRIP_PROOF" \
  || ! grep -Fq '"activate_ok": "true"' "$FOCUS_ARM_ROUNDTRIP_PROOF" \
  || ! grep -Fq '"activate_active_mode": "gate-work"' "$FOCUS_ARM_ROUNDTRIP_PROOF" \
  || ! grep -Fq '"active_mode_gsettings_readback": "gate-work"' "$FOCUS_ARM_ROUNDTRIP_PROOF" \
  || ! grep -Fq '"armed_by_schedule_after_activate": "false"' "$FOCUS_ARM_ROUNDTRIP_PROOF" \
  || ! grep -Fq '"restore_banners_after_activate": "true"' "$FOCUS_ARM_ROUNDTRIP_PROOF" \
  || ! grep -Fq '"notification_banners_after_activate": "false"' "$FOCUS_ARM_ROUNDTRIP_PROOF" \
  || ! grep -Fq '"deactivate_http": "200"' "$FOCUS_ARM_ROUNDTRIP_PROOF" \
  || ! grep -Fq '"deactivate_ok": "true"' "$FOCUS_ARM_ROUNDTRIP_PROOF" \
  || ! grep -Fq '"deactivate_active_mode": ""' "$FOCUS_ARM_ROUNDTRIP_PROOF" \
  || ! grep -Fq '"active_mode_after_deactivate": ""' "$FOCUS_ARM_ROUNDTRIP_PROOF" \
  || ! grep -Fq '"armed_by_schedule_after_deactivate": "false"' "$FOCUS_ARM_ROUNDTRIP_PROOF" \
  || ! grep -Fq '"restore_banners_after_deactivate": ""' "$FOCUS_ARM_ROUNDTRIP_PROOF" \
  || ! grep -Fq '"notification_banners_after_deactivate": "true"' "$FOCUS_ARM_ROUNDTRIP_PROOF" \
  || ! grep -Fq '"original_focus_state_restored": "true"' "$FOCUS_ARM_ROUNDTRIP_PROOF" \
  || ! grep -Fq '"original_notification_banners_restored": "true"' "$FOCUS_ARM_ROUNDTRIP_PROOF" \
  || ! grep -Fq '"roundtrip_restored": "true"' "$FOCUS_ARM_ROUNDTRIP_PROOF" \
  || ! grep -Fq '"mode_crud_claim": "false"' "$FOCUS_ARM_ROUNDTRIP_PROOF" \
  || ! grep -Fq '"schedule_claim": "false"' "$FOCUS_ARM_ROUNDTRIP_PROOF" \
  || ! grep -Fq '"per_app_breakthroughs_claim": "false"' "$FOCUS_ARM_ROUNDTRIP_PROOF"; then
  echo "HONESTY GUARD: missing or failing Focus arm roundtrip proof at $FOCUS_ARM_ROUNDTRIP_PROOF"
  exit 4
fi
if ! grep -Fq '"status": "pass"' "$APP_PRIVACY_REVOKE_PROOF" \
  || ! grep -Fq '"route": "/v1/app-privacy/revoke"' "$APP_PRIVACY_REVOKE_PROOF" \
  || ! grep -Fq '"table": "location"' "$APP_PRIVACY_REVOKE_PROOF" \
  || ! grep -Fq '"app": "org.goblins.GatePrivacyProof"' "$APP_PRIVACY_REVOKE_PROOF" \
  || ! grep -Fq '"seed_method": "PermissionStore.SetPermission"' "$APP_PRIVACY_REVOKE_PROOF" \
  || ! grep -Fq '"revoke_method": "PermissionStore.DeletePermission"' "$APP_PRIVACY_REVOKE_PROOF" \
  || ! grep -Fq '"readback_method": "PermissionStore.GetPermission"' "$APP_PRIVACY_REVOKE_PROOF" \
  || ! grep -Fq '"seed_grant": "yes"' "$APP_PRIVACY_REVOKE_PROOF" \
  || ! grep -Fq '"seed_readback": "true"' "$APP_PRIVACY_REVOKE_PROOF" \
  || ! grep -Fq '"revoke_http": "200"' "$APP_PRIVACY_REVOKE_PROOF" \
  || ! grep -Fq '"revoke_ok": "true"' "$APP_PRIVACY_REVOKE_PROOF" \
  || ! grep -Fq '"post_revoke_absent": "true"' "$APP_PRIVACY_REVOKE_PROOF" \
  || ! grep -Fq '"restore_prior_state": "true"' "$APP_PRIVACY_REVOKE_PROOF" \
  || ! grep -Fq '"roundtrip_restored": "true"' "$APP_PRIVACY_REVOKE_PROOF" \
  || ! grep -Fq '"resource_keyed_claim": "false"' "$APP_PRIVACY_REVOKE_PROOF" \
  || ! grep -Fq '"device_revoke_claim": "false"' "$APP_PRIVACY_REVOKE_PROOF"; then
  echo "HONESTY GUARD: missing or failing App privacy revoke proof at $APP_PRIVACY_REVOKE_PROOF"
  exit 4
fi
if ! grep -Fq '"status": "pass"' "$PREVIEW_OPEN_RENDER_PROOF" \
  || ! grep -Fq '"status_route": "/v1/preview/status"' "$PREVIEW_OPEN_RENDER_PROOF" \
  || ! grep -Fq '"route": "/v1/preview/open"' "$PREVIEW_OPEN_RENDER_PROOF" \
  || ! grep -Fq '"status_http": "200"' "$PREVIEW_OPEN_RENDER_PROOF" \
  || ! grep -Fq '"available": "true"' "$PREVIEW_OPEN_RENDER_PROOF" \
  || ! grep -Fq '"xdg_open": "true"' "$PREVIEW_OPEN_RENDER_PROOF" \
  || ! grep -Fq '"papers": "true"' "$PREVIEW_OPEN_RENDER_PROOF" \
  || ! grep -Fq '"loupe": "true"' "$PREVIEW_OPEN_RENDER_PROOF" \
  || ! grep -Fq '"pdf_default": "org.gnome.Papers.desktop"' "$PREVIEW_OPEN_RENDER_PROOF" \
  || ! grep -Fq '"image_default": "org.gnome.Loupe.desktop"' "$PREVIEW_OPEN_RENDER_PROOF" \
  || ! grep -Fq '"jpeg_default": "org.gnome.Loupe.desktop"' "$PREVIEW_OPEN_RENDER_PROOF" \
  || ! grep -Fq '"pdf_http": "200"' "$PREVIEW_OPEN_RENDER_PROOF" \
  || ! grep -Fq '"pdf_ok": "true"' "$PREVIEW_OPEN_RENDER_PROOF" \
  || ! grep -Fq '"pdf_kind": "pdf"' "$PREVIEW_OPEN_RENDER_PROOF" \
  || ! grep -Fq '"pdf_process": "papers"' "$PREVIEW_OPEN_RENDER_PROOF" \
  || ! grep -Fq '"pdf_screenshot": "29-preview-pdf-open.png"' "$PREVIEW_OPEN_RENDER_PROOF" \
  || ! grep -Fq '"rendered_pdf_frame": "true"' "$PREVIEW_OPEN_RENDER_PROOF" \
  || ! grep -Fq '"image_http": "200"' "$PREVIEW_OPEN_RENDER_PROOF" \
  || ! grep -Fq '"image_ok": "true"' "$PREVIEW_OPEN_RENDER_PROOF" \
  || ! grep -Fq '"image_kind": "image"' "$PREVIEW_OPEN_RENDER_PROOF" \
  || ! grep -Fq '"image_process": "loupe"' "$PREVIEW_OPEN_RENDER_PROOF" \
  || ! grep -Fq '"image_screenshot": "30-preview-image-open.png"' "$PREVIEW_OPEN_RENDER_PROOF" \
  || ! grep -Fq '"rendered_image_frame": "true"' "$PREVIEW_OPEN_RENDER_PROOF" \
  || ! grep -Fq '"unsupported_http": "400"' "$PREVIEW_OPEN_RENDER_PROOF" \
  || ! grep -Fq '"unsupported_ok": "false"' "$PREVIEW_OPEN_RENDER_PROOF" \
  || ! grep -Fq '"unsupported_rejected": "true"' "$PREVIEW_OPEN_RENDER_PROOF"; then
  echo "HONESTY GUARD: missing or failing Preview open/render proof at $PREVIEW_OPEN_RENDER_PROOF"
  exit 4
fi

# HONESTY GUARD: refuse to write a signoff for a run whose surfaces aren't all
# distinct. GNOME 42+ returns AccessDenied to scripted screenshots (org.gnome.
# Shell.Screenshot), so the only automation path is the host QMP framebuffer
# screendump, which collapses some surfaces to byte-identical duplicates when a
# window doesn't foreground in time. The capture, close-signoff, and shipping
# status paths all enforce these guards so copied proof cannot bypass them later.
# Fail loudly rather than commit a dishonest run.
md5cmd() { command -v md5sum >/dev/null && md5sum "$@" || md5 -r "$@"; }
_required_pngs=()
while IFS= read -r -d '' png; do
  _required_pngs+=("$png")
done < <(find "$RUN_DIR" -maxdepth 1 -type f -name '*.png' ! -name '_debug-*' -print0)
_total="${#_required_pngs[@]}"
if [ "${_total:-0}" -eq 0 ]; then
  echo "HONESTY GUARD: no required screenshot surfaces were captured."
  exit 3
fi
_distinct="$(md5cmd "${_required_pngs[@]}" 2>/dev/null | awk '{print $1}' | sort -u | wc -l | tr -d ' ')"
if [ "${_distinct:-0}" -lt "${_total:-1}" ]; then
  echo "HONESTY GUARD: only $_distinct/$_total required captured surfaces are distinct."
  echo "GNOME Wayland blocks scripted per-window capture (AccessDenied); duplicate"
  echo "surfaces cannot be passed off as distinct proof. This run requires a human"
  echo "operator at the display (run-external-gate.sh) — refusing to close-signoff."
  exit 3
fi

stable_frame_hash() {
  local file="$1"
  local tmp width height crop_h
  # Ignore the top shell/GDM bar: its clock changes can make one stale screen
  # look byte-distinct while the actual proof surface never foregrounded.
  tmp="$(mktemp -d)"
  if command -v magick >/dev/null 2>&1; then
    if magick "$file" -gravity South -crop '100%x86%+0+0' \
      -resize 64x64! -colorspace Gray -depth 8 "$tmp/stable.png" >/dev/null 2>&1; then
      md5cmd "$tmp/stable.png" | awk '{print $1}'
      rm -rf "$tmp"
      return 0
    fi
  elif command -v convert >/dev/null 2>&1; then
    if convert "$file" -gravity South -crop '100%x86%+0+0' \
      -resize 64x64! -colorspace Gray -depth 8 "$tmp/stable.png" >/dev/null 2>&1; then
      md5cmd "$tmp/stable.png" | awk '{print $1}'
      rm -rf "$tmp"
      return 0
    fi
  elif command -v sips >/dev/null 2>&1; then
    width="$(sips -g pixelWidth "$file" 2>/dev/null | awk '/pixelWidth:/{print $2; exit}')"
    height="$(sips -g pixelHeight "$file" 2>/dev/null | awk '/pixelHeight:/{print $2; exit}')"
    if [ -n "$width" ] && [ -n "$height" ] && [ "$height" -gt 0 ] 2>/dev/null; then
      crop_h=$((height * 86 / 100))
      [ "$crop_h" -gt 0 ] || crop_h="$height"
      if sips --cropToHeightWidth "$crop_h" "$width" --resampleHeightWidth 64 64 \
        "$file" --out "$tmp/stable.png" >/dev/null 2>&1; then
        md5cmd "$tmp/stable.png" | awk '{print $1}'
        rm -rf "$tmp"
        return 0
      fi
    fi
  fi
  rm -rf "$tmp"
  echo "stable_frame_hash requires ImageMagick (magick/convert) or macOS sips" >&2
  return 1
}

_surface_shots=(
  04-desktop.png
  07-home.png
  08-shell-home.png
  10-settings.png
  11-settings-models.png
  12-settings-dark.png
  13-studio-before.png
  14-studio-running.png
  15-studio-app-detail.png
  16-built-app-open.png
  29-preview-pdf-open.png
  30-preview-image-open.png
  31-text-shortcuts-candidate-bubble-render.png
)
_stable_hashes="$(
  for shot in "${_surface_shots[@]}"; do
    [ -f "$RUN_DIR/$shot" ] || continue
    stable_frame_hash "$RUN_DIR/$shot" || true
  done | sort -u
)"
_stable_distinct="$(printf '%s\n' "$_stable_hashes" | sed '/^$/d' | wc -l | tr -d ' ')"
if [ "${_stable_distinct:-0}" -lt 8 ]; then
  echo "HONESTY GUARD: only $_stable_distinct stable app-surface frame(s) after cropping the top bar."
  echo "This usually means the VM is still on GDM, the login session timed out, or"
  echo "foregrounded proof windows were not captured. Refusing stale screenshot signoff."
  exit 3
fi

if ! semantic_screenshot_frames_are_distinct "$RUN_DIR"; then
  echo "HONESTY GUARD: named login/Home or Studio semantic states reused the same central application crop." >&2
  echo "Clock, top-bar, and pointer-only changes do not count as distinct application proof." >&2
  exit 3
fi

# Write the proof manifest + run close-signoff. The manifest records the
# repo-relative ISO path: close-signoff and verify-shipping-status both match
# the exact string "os/iso/output/$ARCH/bootiso/goblins-os-$ARCH.iso", and the
# committed manifest must not leak runner-absolute paths.
python3 - "$RUN_DIR" "${RUN_DIR#"$REPO/"}" "$ARCH" "${ISO#"$REPO/"}" "$ISO_SHA" "$DATE" "$CANDIDATE_COMMIT" "$IMAGE_REF" "$NATIVE_GATE_PROOF_RELATIVE" "$CAPTURE_WORKFLOW_RUN_URL" "${CAPTURE_WORKFLOW_RUN_ATTEMPT:-0}" "$VERIFICATION_EVIDENCE_MANIFEST_SHA" <<'PY'
import json,sys
run_dir,rel_run_dir,arch,iso,sha,date,candidate_commit,image_ref,native_gate_proof,capture_workflow_run,capture_workflow_attempt,verification_evidence_manifest_sha=sys.argv[1:13]
json.dump({"architecture":arch,"candidate_commit":candidate_commit,"image_ref":image_ref,"iso":iso,"iso_sha256":sha,
          "captured_at":date+"T00:00:00Z","screenshot_run_dir":rel_run_dir,
          "capture_workflow_run":capture_workflow_run,
          "capture_workflow_run_attempt":int(capture_workflow_attempt),
          "native_packaging_gate_proof":native_gate_proof,
          "verification_iso_manifest":"verification-iso-manifest.json",
          "verification_bib_manifest":"verification-bib-manifest.json",
          "verification_release_evidence_manifest":"verification-release-evidence-manifest.json",
          "verification_release_evidence_manifest_sha256":verification_evidence_manifest_sha,
          "firewall_live_toggle_proof":"firewall-live-toggle-proof.json",
          "text_shortcuts_session_enable_proof":"text-shortcuts-session-enable-proof.json",
          "text_shortcuts_candidate_metadata_proof":"text-shortcuts-candidate-metadata-proof.json",
          "text_shortcuts_overlay_intent_proof":"text-shortcuts-overlay-intent-proof.json",
          "text_shortcuts_candidate_bubble_frame_proof":"text-shortcuts-candidate-bubble-frame-proof.json",
          "text_shortcuts_candidate_bubble_layout_proof":"text-shortcuts-candidate-bubble-layout-proof.json",
          "text_shortcuts_candidate_bubble_render_intent_proof":"text-shortcuts-candidate-bubble-render-intent-proof.json",
          "text_shortcuts_candidate_bubble_render_proof":"text-shortcuts-candidate-bubble-render-proof.json",
          "text_shortcuts_live_ibus_runtime_render_proof":"text-shortcuts-live-ibus-runtime-render-proof.json",
          "keyboard_shortcuts_roundtrip_proof":"keyboard-shortcuts-roundtrip-proof.json",
          "input_sources_roundtrip_proof":"input-sources-roundtrip-proof.json",
          "multi_display_apply_proof":"multi-display-apply-proof.json",
          "focus_arm_roundtrip_proof":"focus-arm-roundtrip-proof.json",
          "app_privacy_revoke_proof":"app-privacy-revoke-proof.json",
          "preview_open_render_proof":"preview-open-render-proof.json",
          "audio_output_proof":"audio-output-proof.json",
          "runtime_build_proof":"runtime-build-proof.json",
          "capture_method":"display-backed qemu VM, software GPU/audio substrate (lavapipe/gamescope/pipewire), honestly labeled"},
         open(run_dir+"/proof-manifest.json","w"),indent=2)
PY
echo "capture complete: $RUN_DIR"
# Close-signoff matches the committed repo-relative run dir and reads its own
# relative paths (ISO, workflow) from the repo root.
RUNTIME_ENGINE_SOURCE="$(python3 - "$RUNTIME_BUILD_PROOF" <<'PY'
import json
import sys

try:
    print(json.load(open(sys.argv[1], encoding="utf-8")).get("engine_source", ""))
except Exception:
    print("")
PY
)"
( cd "$REPO" \
  && GOBLINS_OS_ARCH="$ARCH" \
    GOBLINS_OS_IMAGE="$IMAGE_REF" \
    SCREENSHOT_DIR="${RUN_DIR#"$REPO/"}" \
    RUNTIME_ENGINE_MODE="local-model" \
    RUNTIME_ENGINE_SOURCE="$RUNTIME_ENGINE_SOURCE" \
    RUNTIME_ENGINE_CONFIG="${RUN_DIR#"$REPO/"}/runtime-build-proof.json" \
    BUILT_ARTIFACT_PATH_URL="${RUN_DIR#"$REPO/"}/runtime-build-proof.json" \
    GOBLINS_OS_NATIVE_PACKAGING_GATE_PROOF="$NATIVE_GATE_PROOF_RELATIVE" \
    GOBLINS_OS_NATIVE_PACKAGING_GATE_RUN_URL="$CAPTURE_NATIVE_GATE_RUN_URL" \
    GOBLINS_OS_NATIVE_PACKAGING_GATE_RUN_ATTEMPT="$CAPTURE_NATIVE_GATE_RUN_ATTEMPT" \
    GOBLINS_OS_CAPTURE_WORKFLOW_RUN_URL="$CAPTURE_WORKFLOW_RUN_URL" \
    GOBLINS_OS_CAPTURE_WORKFLOW_RUN_ATTEMPT="${CAPTURE_WORKFLOW_RUN_ATTEMPT:-0}" \
    SIGNOFF_ROW_OUTPUT="${RUN_DIR#"$REPO/"}/signoff-row.md" \
    REQUIRE_COMPLETE="$CAPTURE_REQUIRE_COMPLETE" \
    os/hardware-gate/close-signoff.sh )
