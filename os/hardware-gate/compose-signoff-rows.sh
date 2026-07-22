#!/usr/bin/env bash
set -euo pipefail

REPO_ROOT="${REPO_ROOT:-$(cd "$(dirname "$0")/../.." && pwd -P)}"
cd "$REPO_ROOT"

sha256_file() {
  if command -v sha256sum >/dev/null 2>&1; then
    sha256sum "$1" | awk '{print $1}'
  elif command -v shasum >/dev/null 2>&1; then
    shasum -a 256 "$1" | awk '{print $1}'
  else
    echo "sha256sum or shasum is required to compose signoff notes." >&2
    return 1
  fi
}

CANDIDATE_COMMIT="${GOBLINS_OS_CANDIDATE_COMMIT:-}"
if [[ ! "$CANDIDATE_COMMIT" =~ ^[0-9a-f]{40}$ ]]; then
  echo "GOBLINS_OS_CANDIDATE_COMMIT must be the exact lowercase 40-hex candidate commit." >&2
  exit 2
fi
if [ "$(git rev-parse HEAD | tr '[:upper:]' '[:lower:]')" != "$CANDIDATE_COMMIT" ]; then
  echo "The composition checkout must be the exact candidate commit $CANDIDATE_COMMIT." >&2
  exit 2
fi
UNEXPECTED_SOURCE_CHANGES="$({
  git -c core.quotepath=false diff --name-only --no-ext-diff
  git -c core.quotepath=false diff --cached --name-only --no-ext-diff
  git -c core.quotepath=false ls-files --others --exclude-standard
} | sed '/^$/d' | sort -u | grep -Ev '^os/(iso/output/|signoff-proofs/|screenshots/hardware-gate/)|^os/signoff-notes[.]md$' || true)"
if [ -n "$UNEXPECTED_SOURCE_CHANGES" ]; then
  echo "Composition checkout has changes outside generated proof paths:" >&2
  printf '%s\n' "$UNEXPECTED_SOURCE_CHANGES" >&2
  exit 2
fi
if [ "$#" -ne 1 ]; then
  echo "Usage: GOBLINS_OS_CANDIDATE_COMMIT=<commit> $0 <aarch64-signoff-row.md>" >&2
  exit 2
fi

SIGNOFF_NOTES="$REPO_ROOT/os/signoff-notes.md"
SIGNOFF_ROW="$1"
STAGING_PARENT="$REPO_ROOT/os/signoff-proofs"
if [ -L "$SIGNOFF_NOTES" ] || [ ! -f "$SIGNOFF_NOTES" ]; then
  echo "Refusing unsafe or missing canonical signoff notes: $SIGNOFF_NOTES" >&2
  exit 2
fi
if [ -L "$SIGNOFF_ROW" ] || [ ! -f "$SIGNOFF_ROW" ]; then
  echo "Refusing unsafe or missing staged signoff row: $SIGNOFF_ROW" >&2
  exit 2
fi
for component in "$REPO_ROOT/os" "$STAGING_PARENT"; do
  [ ! -L "$component" ] || {
    echo "Refusing symlinked signoff staging path: $component" >&2
    exit 2
  }
done
mkdir -p "$STAGING_PARENT"

ORIGINAL_SIGNOFF_SHA="$(sha256_file "$SIGNOFF_NOTES")"
STAGED_SIGNOFF="$(mktemp "$STAGING_PARENT/.signoff-notes-stage.XXXXXX")"
cleanup() {
  rm -f -- "$STAGED_SIGNOFF"
}
trap cleanup EXIT HUP INT TERM

python3 - \
  "$REPO_ROOT" \
  "$SIGNOFF_NOTES" \
  "$STAGED_SIGNOFF" \
  "$CANDIDATE_COMMIT" \
  "$SIGNOFF_ROW" <<'PY'
from __future__ import annotations

from datetime import date
import hashlib
import json
import os
from pathlib import Path
import re
import stat
import subprocess
import sys

repo = Path(sys.argv[1])
notes_path = Path(sys.argv[2])
staged_path = Path(sys.argv[3])
candidate_commit = sys.argv[4]
row_path = Path(sys.argv[5])
architecture = "aarch64"
source_repository = "https://github.com/Joe-Simo/goblins-os"
max_text_bytes = 16 * 1024 * 1024


def read_regular(path: Path, label: str, maximum: int = max_text_bytes) -> bytes:
    metadata = path.lstat()
    if (
        not stat.S_ISREG(metadata.st_mode)
        or stat.S_ISLNK(metadata.st_mode)
        or metadata.st_nlink != 1
        or metadata.st_size <= 0
        or metadata.st_size > maximum
    ):
        raise SystemExit(f"{label} is not a bounded single-link regular file: {path}")
    data = path.read_bytes()
    after = path.lstat()
    if (
        (metadata.st_dev, metadata.st_ino, metadata.st_size, metadata.st_mtime_ns)
        != (after.st_dev, after.st_ino, after.st_size, after.st_mtime_ns)
        or len(data) != metadata.st_size
    ):
        raise SystemExit(f"{label} changed while it was read: {path}")
    return data


def reject_duplicate_keys(pairs: list[tuple[str, object]]) -> dict[str, object]:
    result: dict[str, object] = {}
    for key, value in pairs:
        if key in result:
            raise ValueError(f"duplicate JSON key {key!r}")
        result[key] = value
    return result


def load_json(path: Path, label: str) -> dict[str, object]:
    try:
        value = json.loads(
            read_regular(path, label).decode("utf-8"),
            object_pairs_hook=reject_duplicate_keys,
            parse_constant=lambda value: (_ for _ in ()).throw(
                ValueError(f"non-finite JSON value {value}")
            ),
        )
    except (UnicodeDecodeError, json.JSONDecodeError, ValueError) as error:
        raise SystemExit(f"{label} is not strict UTF-8 JSON: {path}: {error}") from error
    if type(value) is not dict:
        raise SystemExit(f"{label} must be a JSON object: {path}")
    return value


def sha256(path: Path, maximum: int = 16 * 1024 * 1024 * 1024) -> str:
    metadata = path.lstat()
    if (
        not stat.S_ISREG(metadata.st_mode)
        or stat.S_ISLNK(metadata.st_mode)
        or metadata.st_nlink != 1
        or metadata.st_size <= 0
        or metadata.st_size > maximum
    ):
        raise SystemExit(f"artifact is not a bounded single-link regular file: {path}")
    flags = os.O_RDONLY | getattr(os, "O_NOFOLLOW", 0)
    descriptor = os.open(path, flags)
    digest = hashlib.sha256()
    try:
        opened = os.fstat(descriptor)
        if (opened.st_dev, opened.st_ino) != (metadata.st_dev, metadata.st_ino):
            raise SystemExit(f"artifact changed before hashing: {path}")
        total = 0
        while True:
            chunk = os.read(descriptor, 1024 * 1024)
            if not chunk:
                break
            total += len(chunk)
            if total > maximum:
                raise SystemExit(f"artifact exceeds its byte limit: {path}")
            digest.update(chunk)
        after = os.fstat(descriptor)
        if total != metadata.st_size or (
            opened.st_size,
            opened.st_mtime_ns,
            opened.st_ctime_ns,
        ) != (after.st_size, after.st_mtime_ns, after.st_ctime_ns):
            raise SystemExit(f"artifact changed while hashing: {path}")
    finally:
        os.close(descriptor)
    return digest.hexdigest()


base = read_regular(notes_path, "signoff notes").decode("utf-8")
row = read_regular(row_path, "staged signoff row").decode("utf-8").strip()
if "\x00" in row:
    raise SystemExit(f"{row_path}: NUL byte is not allowed")
lines = row.splitlines()
headings = re.findall(r"^## .+$", row, flags=re.MULTILINE)
if len(headings) != 1 or not headings[0].startswith("## Manual Gate Run: "):
    raise SystemExit(f"{row_path}: expected exactly one Manual Gate Run block")


def unique_value(prefix: str) -> str:
    matches = [line.removeprefix(prefix) for line in lines if line.startswith(prefix)]
    if len(matches) != 1:
        raise SystemExit(f"{row_path}: expected exactly one {prefix!r} line")
    return matches[0]


required_lines = (
    f"- Architecture: {architecture}",
    f"- Candidate/source commit: {candidate_commit}",
    "- Capture workflow run: not provided",
    "- Capture workflow run attempt: 0",
    "- Verify result (blocked=0): pass",
    "- Self-test result: pass",
    "- Current project completion status: complete",
)
for required in required_lines:
    if lines.count(required) != 1:
        raise SystemExit(f"{row_path}: missing unique required line {required!r}")

image_ref = unique_value("- Image digest reference: ")
if re.fullmatch(
    r"ghcr\.io/joe-simo/goblins-os@sha256:[0-9a-f]{64}", image_ref
) is None:
    raise SystemExit(f"{row_path}: image digest reference is missing or invalid")

screenshot_dir = unique_value("- Screenshot dir: ")
match = re.fullmatch(
    r"os/screenshots/hardware-gate/aarch64/([0-9]{4}-[0-9]{2}-[0-9]{2})",
    screenshot_dir,
)
if match is None:
    raise SystemExit(f"{row_path}: screenshot directory is not canonical aarch64 evidence")
try:
    if date.fromisoformat(match.group(1)).isoformat() != match.group(1):
        raise ValueError
except ValueError as error:
    raise SystemExit(f"{row_path}: screenshot directory date is invalid") from error
run_date = match.group(1)
expected_row_path = repo / screenshot_dir / "signoff-row.md"
if row_path.resolve(strict=True) != expected_row_path or expected_row_path.is_symlink():
    raise SystemExit(f"{row_path}: row is not the canonical screenshot-run signoff row")

verification_file = "os/iso/output/aarch64/bootiso/goblins-os-aarch64.iso"
verification_artifact_file = unique_value("- Verification ISO artifact file: ")
verification_sha = unique_value("- Verification ISO SHA256: ")
screenshot_verification_sha = unique_value(
    "- Screenshot proof verification ISO SHA256: "
)
if (
    re.fullmatch(r"[0-9a-f]{64}", verification_sha) is None
    or screenshot_verification_sha != verification_sha
):
    raise SystemExit(f"{row_path}: verification ISO SHA256 bindings are invalid")

proof_manifest = load_json(repo / screenshot_dir / "proof-manifest.json", "proof manifest")
proof_expected = {
    "architecture": architecture,
    "candidate_commit": candidate_commit,
    "image_ref": image_ref,
    "iso": verification_file,
    "iso_sha256": verification_sha,
    "screenshot_run_dir": screenshot_dir,
    "capture_workflow_run": "",
    "capture_workflow_run_attempt": 0,
    "accessibility_adaptivity_proof": "accessibility-adaptivity-proof.json",
}
if any(proof_manifest.get(key) != value for key, value in proof_expected.items()):
    raise SystemExit(f"{row_path}: proof manifest does not bind the verification ISO")

native_attempt = unique_value("- Native packaging gate run attempt: ")
if re.fullmatch(r"[1-9][0-9]*", native_attempt) is None:
    raise SystemExit(f"{row_path}: native packaging gate attempt is invalid")
native_run = unique_value("- Native packaging gate run: ")
if re.fullmatch(
    r"https://github\.com/Joe-Simo/goblins-os/actions/runs/[1-9][0-9]*",
    native_run,
) is None:
    raise SystemExit(f"{row_path}: native packaging gate run URL is invalid")
native_proof_path = repo / screenshot_dir / "native-packaging-gate.json"
if unique_value("- Native packaging gate proof: ") != str(
    native_proof_path.relative_to(repo)
):
    raise SystemExit(f"{row_path}: native packaging proof is not bound to the run")
native_proof = load_json(native_proof_path, "native packaging proof")
native_expected = {
    "schema": "goblins-os-native-packaging-gate-v1",
    "architecture": architecture,
    "candidate_commit": candidate_commit,
    "image_ref": image_ref,
    "verification_iso_sha256": verification_sha,
    "source_repository": source_repository,
    "workflow_run": native_run,
    "workflow_run_attempt": int(native_attempt),
}
if any(native_proof.get(key) != value for key, value in native_expected.items()):
    raise SystemExit(f"{row_path}: native proof does not bind the verification ISO")
verification_artifact = (
    f"goblins-os-aarch64-verification-iso-{candidate_commit}-{run_date}"
    f"-attempt-{native_attempt}"
)
if unique_value("- Verification ISO artifact: ") != verification_artifact:
    raise SystemExit(f"{row_path}: verification ISO artifact identity is not exact")
if verification_artifact_file != f"{verification_artifact}/{verification_file}":
    raise SystemExit(f"{row_path}: verification ISO artifact member is not exact")
native_artifact = (
    f"goblins-os-aarch64-native-packaging-gate-{candidate_commit}-{run_date}"
    f"-attempt-{native_attempt}"
)
native_status = unique_value("- Native packaging gate checked: ")
if not native_status.startswith("yes (") or native_artifact not in native_status:
    raise SystemExit(f"{row_path}: native packaging proof did not bind its exact artifact")

if unique_value("- Evidence bundle: ") != f"{screenshot_dir}/evidence-bundle.json":
    raise SystemExit(f"{row_path}: evidence bundle path is not bound to the screenshot run")
if re.fullmatch(r"[0-9a-f]{64}", unique_value("- Evidence bundle SHA256: ")) is None:
    raise SystemExit(f"{row_path}: evidence bundle SHA256 is invalid")
if not unique_value("- Evidence bundle integrity checked: ").startswith("yes ("):
    raise SystemExit(f"{row_path}: evidence bundle integrity was not accepted")

authority_record_relative = f"{screenshot_dir}/aarch64-local-display-attestation.json"
authority_signature_relative = f"{authority_record_relative}.cms"
if unique_value("- Local display attestation: ") != authority_record_relative:
    raise SystemExit(f"{row_path}: local display attestation is not bound to the run")
if unique_value("- Local display attestation signature: ") != authority_signature_relative:
    raise SystemExit(f"{row_path}: local display authority signature is not bound to the run")
authority_record_path = repo / authority_record_relative
authority_signature_path = repo / authority_signature_relative
authority_record = load_json(authority_record_path, "local display authority record")
authority_certificate_fingerprint = unique_value(
    "- Local display authority certificate SHA256: "
)
authority_ca_certificate_fingerprint = unique_value(
    "- Local display authority CA certificate SHA256: "
)
authority_iso_sha256 = unique_value(
    "- Local display authority verification ISO SHA256: "
)
authority_screenshot_manifest_sha256 = unique_value(
    "- Local display authority screenshot manifest SHA256: "
)
for label, digest in (
    ("authority certificate", authority_certificate_fingerprint),
    ("authority CA certificate", authority_ca_certificate_fingerprint),
    ("authority verification ISO", authority_iso_sha256),
    ("authority screenshot manifest", authority_screenshot_manifest_sha256),
):
    if re.fullmatch(r"[0-9a-f]{64}", digest) is None:
        raise SystemExit(f"{row_path}: {label} SHA256 is invalid")
authority_expected = {
    "schema": "goblins-os-aarch64-local-display-authority-v2",
    "authority_generation": 2,
    "architecture": architecture,
    "candidate_commit": candidate_commit,
    "image_ref": image_ref,
    "run_date": run_date,
    "evidence_bundle_sha256": unique_value("- Evidence bundle SHA256: "),
    "authority_certificate_sha256": authority_certificate_fingerprint,
    "authority_ca_certificate_sha256": authority_ca_certificate_fingerprint,
    "verification_iso_sha256": verification_sha,
    "screenshot_count": 42,
    "screenshot_manifest_sha256": authority_screenshot_manifest_sha256,
    "signature_file": "aarch64-local-display-attestation.json.cms",
    "signature_purpose": "goblins-os-display-proof-authority-v2",
}
if any(authority_record.get(key) != value for key, value in authority_expected.items()):
    raise SystemExit(f"{row_path}: local display authority record does not bind the exact proof")
if authority_iso_sha256 != verification_sha:
    raise SystemExit(f"{row_path}: display authority signed a different verification ISO")
verification = subprocess.run(
    [
        "python3",
        str(repo / "os/hardware-gate/capture-harness/evidence_bundle.py"),
        "verify-attestation",
        "--seal",
        str(repo / screenshot_dir / "evidence-bundle.json"),
        "--record",
        str(authority_record_path),
        "--signature",
        str(authority_signature_path),
        "--certificate",
        str(repo / "os/release/display-proof-authority2.pem"),
        "--certificate-sha256",
        str(repo / "os/release/display-proof-authority2.sha256"),
        "--ca-certificate",
        str(repo / "os/release/display-proof-authority2-ca.pem"),
        "--ca-certificate-sha256",
        str(repo / "os/release/display-proof-authority2-ca.sha256"),
        "--candidate-commit",
        candidate_commit,
        "--image-ref",
        image_ref,
        "--run-date",
        run_date,
    ],
    stdin=subprocess.DEVNULL,
    stdout=subprocess.DEVNULL,
    stderr=subprocess.DEVNULL,
    check=False,
    timeout=30,
)
if verification.returncode != 0:
    raise SystemExit(f"{row_path}: pinned local display authority signature did not verify")
if not unique_value("- Local display attestation checked: ").startswith("yes ("):
    raise SystemExit(f"{row_path}: local display attestation was not accepted")
if not unique_value("- Verification ISO candidate binding checked: ").startswith("yes ("):
    raise SystemExit(f"{row_path}: verification ISO candidate binding was not accepted")
if not unique_value("- Release evidence/SBOM checked: ").startswith("yes ("):
    raise SystemExit(f"{row_path}: release evidence was not accepted")
if not unique_value("- Accessibility/adaptivity checked: ").startswith("yes ("):
    raise SystemExit(f"{row_path}: accessibility/adaptivity evidence was not accepted")
if not unique_value("- Hosted-context review light/dark checked: ").startswith("yes ("):
    raise SystemExit(f"{row_path}: hosted-context review display evidence was not accepted")

public_iso = repo / verification_file
public_sha_file = Path(f"{public_iso}.sha256")
public_iso_manifest = repo / "os/iso/output/aarch64/manifest-goblins-os-aarch64.json"
public_bib_manifest = repo / "os/iso/output/aarch64/manifest-anaconda-iso.json"
public_evidence_dir = repo / "os/signoff-proofs/sbom/aarch64"
public_evidence_manifest = public_evidence_dir / "release-evidence-manifest.json"
public_cargo = public_evidence_dir / "cargo-lock-packages.tsv"
public_rpm_command = public_evidence_dir / "rpm-packages.command"
public_rpm = public_evidence_dir / "rpm-packages.tsv"
candidate_metadata_path = repo / "os/signoff-proofs/candidate/aarch64/image-ref.json"

public_iso_sha = sha256(public_iso)
checksum_text = read_regular(public_sha_file, "public ISO checksum").decode("utf-8")
checksum_match = re.fullmatch(
    r"([0-9a-f]{64})  goblins-os-aarch64\.iso\n?", checksum_text
)
if checksum_match is None or checksum_match.group(1) != public_iso_sha:
    raise SystemExit("public release ISO checksum file does not bind the current ISO bytes")

public_manifest = load_json(public_iso_manifest, "public ISO manifest")
public_manifest_expected = {
    "architecture": architecture,
    "candidate_commit": candidate_commit,
    "image": image_ref,
    "builder_source_image": image_ref,
    "installer_config": "os/iso/config.toml",
    "installer_payload_source_kind": "release-registry",
    "installer_payload_source_local_only": False,
    "shippable_release": True,
}
if any(public_manifest.get(key) != value for key, value in public_manifest_expected.items()):
    raise SystemExit("public release ISO manifest is not the exact human-safe candidate")

evidence_manifest = load_json(public_evidence_manifest, "public evidence manifest")
evidence_expected = {
    "schema": "goblins-os-release-evidence-v5",
    "architecture": architecture,
    "candidate_commit": candidate_commit,
    "image_ref": image_ref,
    "image_digest_pinned": True,
    "cargo_packages_sha256": sha256(public_cargo),
    "rpm_command_sha256": sha256(public_rpm_command),
    "rpm_packages_sha256": sha256(public_rpm),
    "rpm_status": "generated from rpm database",
}
if any(evidence_manifest.get(key) != value for key, value in evidence_expected.items()):
    raise SystemExit("public release evidence does not bind the exact candidate inventories")

candidate_metadata = load_json(candidate_metadata_path, "candidate workflow metadata")
workflow_run = candidate_metadata.get("workflow_run")
workflow_attempt = candidate_metadata.get("workflow_run_attempt")
if not isinstance(workflow_run, str) or re.fullmatch(
    r"https://github\.com/Joe-Simo/goblins-os/actions/runs/[1-9][0-9]*",
    workflow_run,
) is None:
    raise SystemExit("candidate workflow metadata has an invalid workflow run")
if type(workflow_attempt) is not int or workflow_attempt < 1:
    raise SystemExit("candidate workflow metadata has an invalid workflow attempt")
candidate_expected = {
    "schema": "goblins-os-candidate-image-ref-v3",
    "product": "Goblins OS",
    "architecture": architecture,
    "candidate_commit": candidate_commit,
    "image_digest": image_ref.rsplit("@", 1)[1],
    "immutable_image_ref": image_ref,
    "oci_revision": candidate_commit,
    "iso_sha256": public_iso_sha,
    "iso_manifest_sha256": sha256(public_iso_manifest),
    "bib_manifest_sha256": sha256(public_bib_manifest),
    "release_evidence_manifest_sha256": sha256(public_evidence_manifest),
    "cargo_packages_sha256": sha256(public_cargo),
    "rpm_command_sha256": sha256(public_rpm_command),
    "rpm_packages_sha256": sha256(public_rpm),
    "installer_config": "os/iso/config.toml",
    "source_repository": source_repository,
    "workflow_name": "candidate-artifacts",
    "candidate_tag_authoritative": False,
    "non_promotional": True,
}
if any(candidate_metadata.get(key) != value for key, value in candidate_expected.items()):
    raise SystemExit("candidate workflow metadata does not bind the public release bytes")
gates = candidate_metadata.get("exact_candidate_gates")
if type(gates) is not dict or any(
    gates.get(key) != "pass"
    for key in ("source_verifier", "installed_root_verifier", "services_selftest")
):
    raise SystemExit("candidate workflow metadata does not record all exact-candidate gates")

public_artifact = f"goblins-os-candidate-{candidate_commit}-aarch64"
public_lines = (
    f"- Public release artifact: {public_artifact}",
    f"- Public release ISO artifact file: {public_artifact}/{verification_file}",
    f"- Public release ISO: {verification_file}",
    f"- Public release ISO SHA256: {public_iso_sha}",
    f"- Public release candidate metadata: {candidate_metadata_path.relative_to(repo)}",
    f"- Public release source repository: {source_repository}",
    f"- Public release workflow run: {workflow_run}",
    f"- Public release workflow run attempt: {workflow_attempt}",
)
if any(line.startswith("- Public release ") for line in lines):
    raise SystemExit(f"{row_path}: staged verification row already contains public-media fields")
insert_after = f"- Screenshot proof verification ISO SHA256: {screenshot_verification_sha}"
insert_index = lines.index(insert_after) + 1
final_lines = lines[:insert_index] + list(public_lines) + lines[insert_index:]
final_row = "\n".join(final_lines) + "\n"

heading_pattern = re.compile(
    rf"(?ms)^{re.escape(headings[0])}\n.*?(?=^## |\Z)"
)
matches = list(heading_pattern.finditer(base))
if len(matches) > 1:
    raise SystemExit("signoff notes contain duplicate blocks with the staged row heading")
if matches:
    base = heading_pattern.sub("", base, count=1)
composed = base.rstrip() + "\n\n" + final_row
with staged_path.open("wb") as handle:
    handle.write(composed.encode("utf-8"))
    handle.flush()
    os.fsync(handle.fileno())
PY

chmod 0644 "$STAGED_SIGNOFF"
STAGED_SIGNOFF_SHA="$(sha256_file "$STAGED_SIGNOFF")"
GOBLINS_OS_SIGNOFF_NOTES="$STAGED_SIGNOFF" \
GOBLINS_OS_SIGNOFF_STAGING_VALIDATE=1 \
GOBLINS_OS_CANDIDATE_COMMIT="$CANDIDATE_COMMIT" \
  "$REPO_ROOT/os/hardware-gate/verify-shipping-status.sh"

if [ "$(sha256_file "$STAGED_SIGNOFF")" != "$STAGED_SIGNOFF_SHA" ]; then
  echo "Staged signoff notes changed during final validation; canonical notes remain untouched." >&2
  exit 1
fi
if [ "$(sha256_file "$SIGNOFF_NOTES")" != "$ORIGINAL_SIGNOFF_SHA" ]; then
  echo "Canonical signoff notes changed during composition; refusing to overwrite them." >&2
  exit 1
fi
echo "Canonical notes stayed byte-identical before atomic replacement."
mv -f -- "$STAGED_SIGNOFF" "$SIGNOFF_NOTES"
trap - EXIT HUP INT TERM

echo "Atomically composed the fully validated aarch64 signoff row into os/signoff-notes.md"
