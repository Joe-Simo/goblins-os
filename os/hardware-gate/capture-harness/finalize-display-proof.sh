#!/usr/bin/env bash
set -euo pipefail

if [ "$#" -ne 2 ]; then
  echo "usage: $0 os/screenshots/hardware-gate/aarch64/YYYY-MM-DD /absolute/path/attestation.json.cms" >&2
  exit 2
fi

SCRIPT_DIR="$(cd "$(dirname "$0")" && pwd -P)"
REPO_ROOT="${REPO_ROOT:-$(cd "$SCRIPT_DIR/../../.." && pwd -P)}"
RUN_RELATIVE="$1"
SIGNATURE_SOURCE="$2"
AUTHORITY_CERTIFICATE="$REPO_ROOT/os/release/display-proof-authority2.pem"
AUTHORITY_FINGERPRINT="$REPO_ROOT/os/release/display-proof-authority2.sha256"
AUTHORITY_CA_CERTIFICATE="$REPO_ROOT/os/release/display-proof-authority2-ca.pem"
AUTHORITY_CA_FINGERPRINT="$REPO_ROOT/os/release/display-proof-authority2-ca.sha256"

case "$RUN_RELATIVE" in
  os/screenshots/hardware-gate/aarch64/????-??-??) ;;
  *) echo "run directory must be the canonical in-repository ARM64 date directory" >&2; exit 2 ;;
esac
if [ "${SIGNATURE_SOURCE#/}" = "$SIGNATURE_SOURCE" ]; then
  echo "detached CMS input must use an absolute path" >&2
  exit 2
fi

RUN_DIRECTORY="$REPO_ROOT/$RUN_RELATIVE"
MANIFEST="$RUN_DIRECTORY/proof-manifest.json"
RUNTIME_PROOF="$RUN_DIRECTORY/runtime-build-proof.json"
METADATA="$(python3 - "$MANIFEST" "$RUNTIME_PROOF" "$RUN_RELATIVE" <<'PY'
import json
import re
import sys
from datetime import date

manifest_path, runtime_path, expected_run = sys.argv[1:]
try:
    with open(manifest_path, "rb") as stream:
        manifest = json.load(stream)
    with open(runtime_path, "rb") as stream:
        runtime = json.load(stream)
except (OSError, ValueError, TypeError) as error:
    raise SystemExit(f"could not read canonical capture metadata: {error}")
if type(manifest) is not dict or type(runtime) is not dict:
    raise SystemExit("capture metadata must be JSON objects")
candidate = manifest.get("candidate_commit")
image = manifest.get("image_ref")
captured = manifest.get("captured_at")
native_proof = manifest.get("native_packaging_gate_proof")
native_run = manifest.get("native_packaging_gate_run")
native_attempt = manifest.get("native_packaging_gate_run_attempt")
runtime_source = runtime.get("engine_source")
if not isinstance(candidate, str) or re.fullmatch(r"[0-9a-f]{40}", candidate) is None:
    raise SystemExit("capture candidate commit is invalid")
if not isinstance(image, str) or re.fullmatch(r"[^\s@]+@sha256:[0-9a-f]{64}", image) is None:
    raise SystemExit("capture image reference is invalid")
if not isinstance(captured, str) or re.fullmatch(r"[0-9]{4}-[0-9]{2}-[0-9]{2}T00:00:00Z", captured) is None:
    raise SystemExit("capture date is invalid")
run_date = captured[:10]
try:
    if date.fromisoformat(run_date).isoformat() != run_date:
        raise ValueError
except ValueError:
    raise SystemExit("capture date is not a real canonical date")
if manifest.get("architecture") != "aarch64" or manifest.get("screenshot_run_dir") != expected_run or not expected_run.endswith("/" + run_date):
    raise SystemExit("capture manifest is not bound to the requested ARM64 run directory")
if not isinstance(native_proof, str) or not native_proof or native_proof.startswith("/") or ".." in native_proof.split("/") or any(ch.isspace() for ch in native_proof):
    raise SystemExit("native packaging proof path is invalid")
if not isinstance(native_run, str) or re.fullmatch(r"https://github\.com/Joe-Simo/goblins-os/actions/runs/[1-9][0-9]*", native_run) is None:
    raise SystemExit("native packaging workflow run is invalid")
if type(native_attempt) is not int or native_attempt <= 0:
    raise SystemExit("native packaging workflow attempt is invalid")
if not isinstance(runtime_source, str) or not runtime_source or len(runtime_source) > 128 or any(ch.isspace() for ch in runtime_source):
    raise SystemExit("runtime engine source is invalid")
print("\t".join((candidate, image, run_date, native_proof, native_run, str(native_attempt), runtime_source)))
PY
)" || exit 2
IFS=$'\t' read -r CANDIDATE_COMMIT IMAGE_REF RUN_DATE NATIVE_PROOF NATIVE_RUN NATIVE_ATTEMPT RUNTIME_ENGINE_SOURCE <<<"$METADATA"

SOURCE_HEAD="$(git -C "$REPO_ROOT" rev-parse HEAD | tr '[:upper:]' '[:lower:]')"
if [ "$SOURCE_HEAD" != "$CANDIDATE_COMMIT" ]; then
  echo "finalizer checkout $SOURCE_HEAD does not match captured candidate $CANDIDATE_COMMIT" >&2
  exit 2
fi
UNEXPECTED_SOURCE_CHANGES="$({
  git -C "$REPO_ROOT" -c core.quotepath=false diff --name-only --no-ext-diff
  git -C "$REPO_ROOT" -c core.quotepath=false diff --cached --name-only --no-ext-diff
  git -C "$REPO_ROOT" -c core.quotepath=false ls-files --others --exclude-standard
} | sed '/^$/d' | sort -u | grep -Ev '^os/(iso/output/|signoff-proofs/|screenshots/hardware-gate/)|^os/signoff-notes[.]md$' || true)"
if [ -n "$UNEXPECTED_SOURCE_CHANGES" ]; then
  echo "finalizer checkout has changes outside generated proof paths:" >&2
  printf '%s\n' "$UNEXPECTED_SOURCE_CHANGES" >&2
  exit 2
fi

for authority_file in \
  "$AUTHORITY_CERTIFICATE" "$AUTHORITY_FINGERPRINT" \
  "$AUTHORITY_CA_CERTIFICATE" "$AUTHORITY_CA_FINGERPRINT"; do
  [ -s "$authority_file" ] || { echo "missing pinned Authority 2 public file: $authority_file" >&2; exit 2; }
done

python3 "$SCRIPT_DIR/evidence_bundle.py" verify-authority-certificate \
  --certificate "$AUTHORITY_CERTIFICATE" \
  --certificate-sha256 "$AUTHORITY_FINGERPRINT" \
  --ca-certificate "$AUTHORITY_CA_CERTIFICATE" \
  --ca-certificate-sha256 "$AUTHORITY_CA_FINGERPRINT" >/dev/null
python3 "$SCRIPT_DIR/evidence_bundle.py" verify \
  --repository "$REPO_ROOT" \
  --run-dir "$RUN_RELATIVE" \
  --architecture aarch64 \
  --candidate-commit "$CANDIDATE_COMMIT" \
  --image-ref "$IMAGE_REF" \
  --run-date "$RUN_DATE"
python3 "$SCRIPT_DIR/evidence_bundle.py" verify-attestation \
  --seal "$RUN_DIRECTORY/evidence-bundle.json" \
  --record "$RUN_DIRECTORY/aarch64-local-display-attestation.json" \
  --signature "$SIGNATURE_SOURCE" \
  --certificate "$AUTHORITY_CERTIFICATE" \
  --certificate-sha256 "$AUTHORITY_FINGERPRINT" \
  --ca-certificate "$AUTHORITY_CA_CERTIFICATE" \
  --ca-certificate-sha256 "$AUTHORITY_CA_FINGERPRINT" \
  --candidate-commit "$CANDIDATE_COMMIT" \
  --image-ref "$IMAGE_REF" \
  --run-date "$RUN_DATE"

SIGNATURE_DESTINATION="$RUN_DIRECTORY/aarch64-local-display-attestation.json.cms"
if [ -e "$SIGNATURE_DESTINATION" ] || [ -L "$SIGNATURE_DESTINATION" ]; then
  echo "refusing to overwrite an existing display-proof signature: $SIGNATURE_DESTINATION" >&2
  exit 2
fi
SIGNATURE_IMPORTED=0
cleanup_unverified_signature() {
  if [ "$SIGNATURE_IMPORTED" = 0 ]; then
    rm -f -- "$SIGNATURE_DESTINATION"
  fi
}
trap cleanup_unverified_signature EXIT
python3 - "$SIGNATURE_SOURCE" "$SIGNATURE_DESTINATION" <<'PY'
import os
import stat
import sys

source, destination = sys.argv[1:]
flags = os.O_RDONLY | getattr(os, "O_CLOEXEC", 0) | getattr(os, "O_NOFOLLOW", 0)
descriptor = os.open(source, flags)
try:
    before = os.fstat(descriptor)
    if not stat.S_ISREG(before.st_mode) or before.st_nlink != 1 or not 1 <= before.st_size <= 256 * 1024:
        raise SystemExit("detached CMS input is not one bounded direct regular file")
    data = b""
    while len(data) <= 256 * 1024:
        chunk = os.read(descriptor, 64 * 1024)
        if not chunk:
            break
        data += chunk
    after = os.fstat(descriptor)
    if (before.st_dev, before.st_ino, before.st_size, before.st_mtime_ns, before.st_ctime_ns) != (after.st_dev, after.st_ino, after.st_size, after.st_mtime_ns, after.st_ctime_ns) or len(data) != before.st_size:
        raise SystemExit("detached CMS input changed while it was read")
finally:
    os.close(descriptor)
output_flags = os.O_WRONLY | os.O_CREAT | os.O_EXCL | getattr(os, "O_CLOEXEC", 0) | getattr(os, "O_NOFOLLOW", 0)
output = os.open(destination, output_flags, 0o644)
complete = False
try:
    view = memoryview(data)
    while view:
        written = os.write(output, view)
        if written <= 0:
            raise SystemExit("short write while importing detached CMS")
        view = view[written:]
    os.fsync(output)
    complete = True
finally:
    os.close(output)
    if not complete:
        try:
            os.unlink(destination)
        except OSError:
            pass
PY

python3 "$SCRIPT_DIR/evidence_bundle.py" verify-attestation \
  --seal "$RUN_DIRECTORY/evidence-bundle.json" \
  --record "$RUN_DIRECTORY/aarch64-local-display-attestation.json" \
  --signature "$SIGNATURE_DESTINATION" \
  --certificate "$AUTHORITY_CERTIFICATE" \
  --certificate-sha256 "$AUTHORITY_FINGERPRINT" \
  --ca-certificate "$AUTHORITY_CA_CERTIFICATE" \
  --ca-certificate-sha256 "$AUTHORITY_CA_FINGERPRINT" \
  --candidate-commit "$CANDIDATE_COMMIT" \
  --image-ref "$IMAGE_REF" \
  --run-date "$RUN_DATE"
SIGNATURE_IMPORTED=1
trap - EXIT

CAPTURE_REQUIRE_COMPLETE="${GOBLINS_OS_CAPTURE_REQUIRE_COMPLETE:-0}"
case "$CAPTURE_REQUIRE_COMPLETE" in 0|1) ;; *) echo "GOBLINS_OS_CAPTURE_REQUIRE_COMPLETE must be 0 or 1" >&2; exit 2 ;; esac
( cd "$REPO_ROOT" \
  && GOBLINS_OS_ARCH=aarch64 \
    GOBLINS_OS_CANDIDATE_COMMIT="$CANDIDATE_COMMIT" \
    GOBLINS_OS_IMAGE="$IMAGE_REF" \
    SCREENSHOT_DIR="$RUN_RELATIVE" \
    RUNTIME_ENGINE_MODE=local-model \
    RUNTIME_ENGINE_SOURCE="$RUNTIME_ENGINE_SOURCE" \
    RUNTIME_ENGINE_CONFIG="$RUN_RELATIVE/runtime-build-proof.json" \
    BUILT_ARTIFACT_PATH_URL="$RUN_RELATIVE/runtime-build-proof.json" \
    GOBLINS_OS_NATIVE_PACKAGING_GATE_PROOF="$NATIVE_PROOF" \
    GOBLINS_OS_NATIVE_PACKAGING_GATE_RUN_URL="$NATIVE_RUN" \
    GOBLINS_OS_NATIVE_PACKAGING_GATE_RUN_ATTEMPT="$NATIVE_ATTEMPT" \
    GOBLINS_OS_CAPTURE_WORKFLOW_RUN_URL= \
    GOBLINS_OS_CAPTURE_WORKFLOW_RUN_ATTEMPT=0 \
    SIGNOFF_ROW_OUTPUT="$RUN_RELATIVE/signoff-row.md" \
    REQUIRE_COMPLETE="$CAPTURE_REQUIRE_COMPLETE" \
    os/hardware-gate/close-signoff.sh )

echo "Authority 2 display proof finalized and signoff staged: $RUN_RELATIVE"
