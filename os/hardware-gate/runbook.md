# Goblins OS External Sign-off Runbook

Run the release build and the `run-external-gate.sh` path on a native Linux host
with Docker, QEMU, and a display-backed VM path available. The capture harness
boots a verification-only ISO built with `os/iso/verify-config.toml` from the
same real pullable bootc image ref used by release media. Do not point the
automated capture harness at hydrated public release media: release ISOs are
human-safe and intentionally leave storage interactive.

Set:

```sh
set -euo pipefail

REPO_ROOT="${REPO_ROOT:-$(pwd)}"
cd "$REPO_ROOT"
export GOBLINS_OS_CANDIDATE_COMMIT="$(git rev-parse HEAD)"
```

## 0) Preflight
- Confirm runtime requirements on the host:
  - `docker` (required for the local image build, install ISO, and bootc-image-builder flow)
  - `qemu-system-x86_64` for x86_64 display-backed VM gate
  - `qemu-system-aarch64` plus aarch64 EDK2/AAVMF UEFI pflash code and writable variable store for aarch64 display-backed VM gate
  - `qemu-img` and at least one screenshot capture tool for the host.
  - readable/writable `/dev/kvm`; the display-backed proof uses native KVM acceleration, not architecture emulation.
  - at least 120 GiB free on both the repo filesystem and VM scratch filesystem before building release media; override `MIN_HOST_FREE_GB` only on runners with separately provisioned image/cache capacity.
  - `docker info` returns promptly before starting the build; restart Docker or free host resources if it hangs.
- Confirm repo at `$REPO_ROOT` and you are in that directory.
- Select one exact 40-hex source commit in `GOBLINS_OS_CANDIDATE_COMMIT`. Use
  that same value for the aarch64 and x86_64 artifact, capture, and signoff runs.
- Choose a native architecture: `ARCH=x86_64` or `ARCH=aarch64`.
- Choose the immutable pullable release bootc image ref for that architecture:
  `RELEASE_IMAGE=<registry>/<namespace>/goblins-os@sha256:<64-hex-digest>`. The Docker-local
  `localhost/goblins-os:$ARCH` handoff is only for artifact testing and cannot
  satisfy shipping proof.
- Run the fail-closed runner preflight before starting the build. This checks the
  native architecture, Docker health, free space, QEMU/KVM, and aarch64 UEFI
  paths when applicable; it does not create shipping artifacts or satisfy proof by itself:
  ```sh
  set -euo pipefail

  PREFLIGHT_ONLY=1 GOBLINS_OS_ARCH="$ARCH" \
    GOBLINS_OS_BIB_SOURCE_IMAGE="$RELEASE_IMAGE" \
    REPO_ROOT="$REPO_ROOT" os/hardware-gate/run-external-gate.sh
  ```
- Prepare a writable scratch VM disk if preflight passed and you are not letting
  the helper create it: `qemu-img create -f qcow2 /tmp/goblins-os-$ARCH.qcow2 80G`.

### Rotating the immutable installer-branding tool

Do this only when `os/iso/branding-tool.Containerfile` or its base image changes.
The tool must be built natively on both architectures, reviewed, publicly
pullable, and digest-pinned before any candidate ISO uses it.

```sh
set -euo pipefail

git fetch --no-tags origin main
test -z "$(git status --porcelain --untracked-files=normal)"
TOOL_COMMIT="$(git rev-parse HEAD)"
test "$TOOL_COMMIT" = "$(git rev-parse origin/main)"
TOOL_RUN_URL="$(gh workflow run branding-tool-image.yml --ref main \
  -f candidate_commit="$TOOL_COMMIT")"
[[ "$TOOL_RUN_URL" =~ /actions/runs/[0-9]+$ ]]
TOOL_RUN_ID="${TOOL_RUN_URL##*/}"
gh run watch "$TOOL_RUN_ID" --exit-status
TOOL_RUN_ATTEMPT="$(gh run view "$TOOL_RUN_ID" --json attempt --jq '.attempt')"
[[ "$TOOL_RUN_ATTEMPT" =~ ^[1-9][0-9]*$ ]]
TOOL_TMP="$(mktemp -d "${TMPDIR:-/tmp}/goblins-branding-review.XXXXXX")"
trap 'rm -rf "$TOOL_TMP"' EXIT
TOOL_RUN_METADATA="$TOOL_TMP/workflow-run.json"
gh api "repos/Joe-Simo/goblins-os/actions/runs/$TOOL_RUN_ID/attempts/$TOOL_RUN_ATTEMPT" \
  > "$TOOL_RUN_METADATA"
jq -e \
  --arg commit "$TOOL_COMMIT" \
  --arg run "$TOOL_RUN_URL" \
  --argjson attempt "$TOOL_RUN_ATTEMPT" \
  '.html_url == $run
   and .conclusion == "success"
   and .head_sha == $commit
   and .event == "workflow_dispatch"
   and .path == ".github/workflows/branding-tool-image.yml"
   and .run_attempt == $attempt' \
  "$TOOL_RUN_METADATA" >/dev/null
gh run download "$TOOL_RUN_ID" \
  -n "goblins-os-branding-tool-$TOOL_COMMIT-index" \
  -D "$TOOL_TMP"
TOOL_INDEX="$TOOL_TMP/image-ref.json"
test -s "$TOOL_INDEX"
test ! -L "$TOOL_INDEX"
TOOL_REF="$(jq -er '.image_ref' "$TOOL_INDEX")"
jq -e \
  --arg commit "$TOOL_COMMIT" \
  --arg run "$TOOL_RUN_URL" \
  --argjson attempt "$TOOL_RUN_ATTEMPT" \
  '.schema == "goblins-os-installer-branding-tool-index-v1"
   and .candidate_commit == $commit
   and .workflow_run == $run
   and .workflow_run_attempt == $attempt
   and (.image_ref | test("^ghcr\\.io/joe-simo/goblins-os-installer-branding-tool@sha256:[0-9a-f]{64}$"))
   and (.native_images.x86_64 | test("@sha256:[0-9a-f]{64}$"))
   and (.native_images.aarch64 | test("@sha256:[0-9a-f]{64}$"))' \
  "$TOOL_INDEX" >/dev/null
test "$(shasum -a 256 "$TOOL_TMP/rpm-packages-x86_64.tsv" | awk '{print $1}')" = \
  "$(jq -er '.rpm_inventory_sha256.x86_64' "$TOOL_INDEX")"
test "$(shasum -a 256 "$TOOL_TMP/rpm-packages-aarch64.tsv" | awk '{print $1}')" = \
  "$(jq -er '.rpm_inventory_sha256.aarch64' "$TOOL_INDEX")"
PUBLIC_DOCKER_CONFIG="$TOOL_TMP/public-docker"
mkdir -p "$PUBLIC_DOCKER_CONFIG"
DOCKER_CONFIG="$PUBLIC_DOCKER_CONFIG" docker buildx imagetools inspect "$TOOL_REF" >/dev/null
```

Review both full RPM inventories and their licenses. Then update
`os/release/installer-branding-tool.toml` with the exact index, native refs,
inventory hashes/counts, source commit, workflow run and attempt, base image,
Containerfile SHA256, and public-pull date. Propagate that index through every
release workflow and `os/iso/build-iso.sh`, then run `goblins-os-verify`; its
semantic provenance check rejects Containerfile, base-image, architecture, or
pin drift. Never substitute a tag for the reviewed digest.

### Canonical exact-candidate build

Build both native architectures through the single non-promotional candidate
workflow. It accepts only the current, clean, pushed `origin/main` commit. Save
the exact run URL returned by the dispatch; never substitute whichever run is
merely latest.

```sh
set -euo pipefail

git fetch --no-tags origin main
test -z "$(git status --porcelain --untracked-files=normal)"
export GOBLINS_OS_CANDIDATE_COMMIT="$(git rev-parse origin/main)"
test "$(git rev-parse HEAD)" = "$GOBLINS_OS_CANDIDATE_COMMIT"

CANDIDATE_RUN_URL="$(gh workflow run candidate-artifacts.yml --ref main \
  -f candidate_commit="$GOBLINS_OS_CANDIDATE_COMMIT")"
printf '%s\n' "$CANDIDATE_RUN_URL"
if [[ ! "$CANDIDATE_RUN_URL" =~ /actions/runs/[0-9]+$ ]]; then
  echo "The dispatch did not return an exact run URL; record the candidate-filtered run ID before continuing." >&2
  gh run list --workflow candidate-artifacts.yml \
    --commit "$GOBLINS_OS_CANDIDATE_COMMIT" --event workflow_dispatch --limit 10
  exit 1
fi
CANDIDATE_RUN_ID="${CANDIDATE_RUN_URL##*/}"
[[ "$CANDIDATE_RUN_ID" =~ ^[0-9]+$ ]] || exit 1
gh run watch "$CANDIDATE_RUN_ID" --exit-status
```

Download the metadata-only artifacts, not the multi-gigabyte ISO artifacts, to
obtain each immutable image reference. Validate the architecture, commit, and
non-promotional marker before using either digest:

```sh
set -euo pipefail

CANDIDATE_METADATA_DIR="$(mktemp -d "${TMPDIR:-/tmp}/goblins-os-candidate-ref.XXXXXX")"
for ARCH in x86_64 aarch64; do
  ARCH_METADATA_DIR="$CANDIDATE_METADATA_DIR/$ARCH"
  gh run download "$CANDIDATE_RUN_ID" \
    -n "goblins-os-candidate-ref-$GOBLINS_OS_CANDIDATE_COMMIT-$ARCH" \
    -D "$ARCH_METADATA_DIR"
  REF_JSON_COUNT="$(find "$ARCH_METADATA_DIR" -type f -name image-ref.json -print | wc -l | tr -d '[:space:]')"
  test "$REF_JSON_COUNT" = 1
  REF_JSON="$(find "$ARCH_METADATA_DIR" -type f -name image-ref.json -print)"
  jq -e \
    --arg arch "$ARCH" \
    --arg commit "$GOBLINS_OS_CANDIDATE_COMMIT" \
    --arg run "$CANDIDATE_RUN_URL" \
    '.schema == "goblins-os-candidate-image-ref-v2"
     and .architecture == $arch
     and .candidate_commit == $commit
     and .oci_revision == $commit
     and .candidate_tag_authoritative == false
     and .non_promotional == true
     and .installer_config == "os/iso/config.toml"
     and .source_repository == "https://github.com/Joe-Simo/goblins-os"
     and .workflow_name == "candidate-artifacts"
     and .workflow_run == $run
     and ((.workflow_run_attempt | type) == "number")
     and .workflow_run_attempt >= 1
     and .exact_candidate_gates.source_verifier == "pass"
     and .exact_candidate_gates.installed_root_verifier == "pass"
     and .exact_candidate_gates.services_selftest == "pass"
     and (.immutable_image_ref | test("^ghcr\\.io/joe-simo/goblins-os@sha256:[0-9a-f]{64}$"))' \
    "$REF_JSON" >/dev/null
  case "$ARCH" in
    x86_64) X86_64_REF_JSON="$REF_JSON" ;;
    aarch64) AARCH64_REF_JSON="$REF_JSON" ;;
  esac
done

X86_64_CANDIDATE_RUN_ATTEMPT="$(jq -er '.workflow_run_attempt' "$X86_64_REF_JSON")"
AARCH64_CANDIDATE_RUN_ATTEMPT="$(jq -er '.workflow_run_attempt' "$AARCH64_REF_JSON")"
test "$X86_64_CANDIDATE_RUN_ATTEMPT" = "$AARCH64_CANDIDATE_RUN_ATTEMPT"
CANDIDATE_RUN_ATTEMPT="$X86_64_CANDIDATE_RUN_ATTEMPT"
CANDIDATE_RUN_METADATA="$(mktemp "${TMPDIR:-/tmp}/goblins-os-candidate-run.XXXXXX")"
gh api "repos/Joe-Simo/goblins-os/actions/runs/$CANDIDATE_RUN_ID/attempts/$CANDIDATE_RUN_ATTEMPT" > "$CANDIDATE_RUN_METADATA"
jq -e \
  --arg commit "$GOBLINS_OS_CANDIDATE_COMMIT" \
  --arg run "$CANDIDATE_RUN_URL" \
  --argjson attempt "$CANDIDATE_RUN_ATTEMPT" \
  '.html_url == $run
   and .head_sha == $commit
   and .event == "workflow_dispatch"
   and .conclusion == "success"
   and .path == ".github/workflows/candidate-artifacts.yml"
   and .run_attempt == $attempt' \
  "$CANDIDATE_RUN_METADATA" >/dev/null
X86_64_IMAGE_REF="$(jq -er '.immutable_image_ref' "$X86_64_REF_JSON")"
AARCH64_IMAGE_REF="$(jq -er '.immutable_image_ref' "$AARCH64_REF_JSON")"
```

The commit-scoped image tag is only a build locator and can move on a rebuild.
The `immutable_image_ref` digest is the release-proof identity.

For the x86_64 display-backed capture, pass that exact digest to the read-only
workflow and retain its exact run URL:

`gh workflow run --ref` selects a branch or tag, so these dispatches use
`main`; the immutable commit remains a separate required input. The commands
below reject the run unless its recorded `head_sha` is that exact commit. If
`main` moves between selection and dispatch, stop and select a new candidate.

```sh
set -euo pipefail

RUN_DATE="${RUN_DATE:-$(date -u +%F)}"
X86_64_RUN_URL="$(gh workflow run hardware-gate-capture.yml --ref main \
  -f run_date="$RUN_DATE" \
  -f candidate_commit="$GOBLINS_OS_CANDIDATE_COMMIT" \
  -f candidate_image_ref="$X86_64_IMAGE_REF")"
printf '%s\n' "$X86_64_RUN_URL"
[[ "$X86_64_RUN_URL" =~ /actions/runs/[0-9]+$ ]] || {
  echo "The x86_64 dispatch did not return an exact run URL; stop and record its candidate-filtered run ID." >&2
  exit 1
}
X86_64_RUN_ID="${X86_64_RUN_URL##*/}"
[[ "$X86_64_RUN_ID" =~ ^[0-9]+$ ]] || exit 1
gh run watch "$X86_64_RUN_ID" --exit-status
X86_64_RUN_ATTEMPT="$(gh run view "$X86_64_RUN_ID" --json attempt --jq '.attempt')"
[[ "$X86_64_RUN_ATTEMPT" =~ ^[1-9][0-9]*$ ]]
X86_64_PROOF_DIR="$(mktemp -d "${TMPDIR:-/tmp}/goblins-os-x86_64-proof.XXXXXX")"
gh run download "$X86_64_RUN_ID" \
  -n "hardware-gate-evidence-$GOBLINS_OS_CANDIDATE_COMMIT-x86_64-$RUN_DATE-attempt-$X86_64_RUN_ATTEMPT" \
  -D "$X86_64_PROOF_DIR"
X86_64_SCREENSHOT_RUN_DIR="$X86_64_PROOF_DIR/screenshots/hardware-gate/x86_64/$RUN_DATE"
X86_64_SIGNOFF_ROW="$X86_64_SCREENSHOT_RUN_DIR/signoff-row.md"
X86_64_PROOF_MANIFEST="$X86_64_SCREENSHOT_RUN_DIR/proof-manifest.json"
test "$(find "$X86_64_SCREENSHOT_RUN_DIR" -maxdepth 1 -type f -name signoff-row.md -print | wc -l | tr -d '[:space:]')" = 1
test -s "$X86_64_SIGNOFF_ROW"
test -s "$X86_64_PROOF_MANIFEST"
test ! -L "$X86_64_SIGNOFF_ROW"
test ! -L "$X86_64_PROOF_MANIFEST"
test "$(jq -er 'select(((.capture_workflow_run_attempt | type) == "number") and .capture_workflow_run_attempt >= 1) | .capture_workflow_run_attempt' "$X86_64_PROOF_MANIFEST")" = "$X86_64_RUN_ATTEMPT"
X86_64_RUN_METADATA="$(mktemp "${TMPDIR:-/tmp}/goblins-os-x86_64-run.XXXXXX")"
gh api "repos/Joe-Simo/goblins-os/actions/runs/$X86_64_RUN_ID/attempts/$X86_64_RUN_ATTEMPT" > "$X86_64_RUN_METADATA"
jq -e \
  --arg commit "$GOBLINS_OS_CANDIDATE_COMMIT" \
  --arg run "$X86_64_RUN_URL" \
  --argjson attempt "$X86_64_RUN_ATTEMPT" \
  '.html_url == $run
   and .head_sha == $commit
   and .event == "workflow_dispatch"
   and .conclusion == "success"
   and .path == ".github/workflows/hardware-gate-capture.yml"
   and .run_attempt == $attempt' \
  "$X86_64_RUN_METADATA" >/dev/null
jq -e \
  --arg run "$X86_64_RUN_URL" \
  --argjson attempt "$X86_64_RUN_ATTEMPT" \
  '.capture_workflow_run == $run
   and .capture_workflow_run_attempt == $attempt' \
  "$X86_64_PROOF_MANIFEST" >/dev/null
test "$(grep -Fxc -- "- Capture workflow run: $X86_64_RUN_URL" "$X86_64_SIGNOFF_ROW")" = 1
test "$(grep -Fxc -- "- Capture workflow run attempt: $X86_64_RUN_ATTEMPT" "$X86_64_SIGNOFF_ROW")" = 1
```

The hardware workflows have read-only repository permission and only upload
short-lived artifacts. The x86_64 workflow fails unless `close-signoff.sh`
records a complete row; an uploaded artifact from a failed run is diagnostic,
not release proof. Review and overlay the exact x86_64 and aarch64 outputs
into a disposable checkout of the selected candidate, run the final gate there,
and attach the reviewed evidence to the release. Do not advance or rebuild the
selected source candidate merely to store generated proof.

### aarch64 macOS/HVF capture route

The Linux external gate remains the artifact/SBOM build authority. For the
display-backed aarch64 screenshot run, an Apple-Silicon host can boot an
already materialized verification-only hardware-gate ISO with the capture
harness. That ISO must be built from the real pullable release bootc image ref
with `GOBLINS_OS_ISO_CONFIG=os/iso/verify-config.toml`; hydrated public release
ISOs do not include the noninteractive hardware-gate kickstart and cannot
satisfy this proof.

If the local Apple-Silicon machine does not have Docker running or enough free
space to build release media, build only the aarch64 verification ISO on the
native GitHub arm runner and download the short-lived artifact:

```sh
set -euo pipefail

RUN_DATE="${RUN_DATE:-$(date -u +%F)}"
AARCH64_RUN_URL="$(gh workflow run aarch64-verification-iso.yml --ref main \
  -f run_date="$RUN_DATE" \
  -f candidate_commit="$GOBLINS_OS_CANDIDATE_COMMIT" \
  -f candidate_image_ref="$AARCH64_IMAGE_REF")"
printf '%s\n' "$AARCH64_RUN_URL"
[[ "$AARCH64_RUN_URL" =~ /actions/runs/[0-9]+$ ]] || {
  echo "The aarch64 dispatch did not return an exact run URL; stop and record its candidate-filtered run ID." >&2
  exit 1
}
AARCH64_RUN_ID="${AARCH64_RUN_URL##*/}"
[[ "$AARCH64_RUN_ID" =~ ^[0-9]+$ ]] || exit 1
gh run watch "$AARCH64_RUN_ID" --exit-status
AARCH64_RUN_ATTEMPT="$(gh run view "$AARCH64_RUN_ID" --json attempt --jq '.attempt')"
[[ "$AARCH64_RUN_ATTEMPT" =~ ^[1-9][0-9]*$ ]]
AARCH64_RUN_METADATA="$(mktemp "${TMPDIR:-/tmp}/goblins-os-aarch64-run.XXXXXX")"
gh api "repos/Joe-Simo/goblins-os/actions/runs/$AARCH64_RUN_ID/attempts/$AARCH64_RUN_ATTEMPT" > "$AARCH64_RUN_METADATA"
jq -e \
  --arg commit "$GOBLINS_OS_CANDIDATE_COMMIT" \
  --arg run "$AARCH64_RUN_URL" \
  --argjson attempt "$AARCH64_RUN_ATTEMPT" \
  '.html_url == $run
   and .head_sha == $commit
   and .event == "workflow_dispatch"
   and .conclusion == "success"
   and .path == ".github/workflows/aarch64-verification-iso.yml"
   and .run_attempt == $attempt' \
  "$AARCH64_RUN_METADATA" >/dev/null
AARCH64_PROOF_DIR="$(mktemp -d "${TMPDIR:-/tmp}/goblins-os-aarch64-verification-iso.XXXXXX")"
gh run download "$AARCH64_RUN_ID" \
  -n "goblins-os-aarch64-verification-iso-$GOBLINS_OS_CANDIDATE_COMMIT-$RUN_DATE-attempt-$AARCH64_RUN_ATTEMPT" \
  -D "$AARCH64_PROOF_DIR"
AARCH64_NATIVE_GATE_PROOF="$AARCH64_PROOF_DIR/signoff-proofs/native-gate/aarch64/native-packaging-gate.json"
AARCH64_VERIFICATION_ISO="$AARCH64_PROOF_DIR/iso/output/aarch64/bootiso/goblins-os-aarch64.iso"
AARCH64_VERIFICATION_ISO_CHECKSUM="$AARCH64_VERIFICATION_ISO.sha256"
AARCH64_VERIFICATION_ISO_MANIFEST="$AARCH64_PROOF_DIR/iso/output/aarch64/manifest-goblins-os-aarch64.json"
AARCH64_VERIFICATION_BIB_MANIFEST="$AARCH64_PROOF_DIR/iso/output/aarch64/manifest-anaconda-iso.json"
AARCH64_VERIFICATION_EVIDENCE_DIR="$AARCH64_PROOF_DIR/signoff-proofs/sbom/aarch64"
AARCH64_VERIFICATION_EVIDENCE_MANIFEST="$AARCH64_VERIFICATION_EVIDENCE_DIR/release-evidence-manifest.json"
for artifact in \
  "$AARCH64_NATIVE_GATE_PROOF" \
  "$AARCH64_VERIFICATION_ISO" \
  "$AARCH64_VERIFICATION_ISO_CHECKSUM" \
  "$AARCH64_VERIFICATION_ISO_MANIFEST" \
  "$AARCH64_VERIFICATION_BIB_MANIFEST" \
  "$AARCH64_VERIFICATION_EVIDENCE_MANIFEST"; do
  test -s "$artifact"
  test ! -L "$artifact"
done
test "$(find "$AARCH64_PROOF_DIR" -type f -name goblins-os-aarch64.iso -print | wc -l | tr -d '[:space:]')" = 1
. "$REPO_ROOT/os/hardware-gate/release-evidence.sh"
goblins_os_release_evidence_hashes_match "$AARCH64_VERIFICATION_EVIDENCE_DIR"
jq -e \
  --arg commit "$GOBLINS_OS_CANDIDATE_COMMIT" \
  --arg image "$AARCH64_IMAGE_REF" \
  '.schema == "goblins-os-release-evidence-v4"
   and .architecture == "aarch64"
   and .candidate_commit == $commit
   and .image_ref == $image
   and .image_digest_pinned == true
   and .rpm_status == "generated from rpm database"' \
  "$AARCH64_VERIFICATION_EVIDENCE_MANIFEST" >/dev/null
if command -v sha256sum >/dev/null 2>&1; then
  AARCH64_VERIFICATION_ISO_SHA="$(sha256sum "$AARCH64_VERIFICATION_ISO" | awk '{print $1}')"
  AARCH64_VERIFICATION_ISO_MANIFEST_SHA="$(sha256sum "$AARCH64_VERIFICATION_ISO_MANIFEST" | awk '{print $1}')"
  AARCH64_VERIFICATION_BIB_MANIFEST_SHA="$(sha256sum "$AARCH64_VERIFICATION_BIB_MANIFEST" | awk '{print $1}')"
  AARCH64_VERIFICATION_EVIDENCE_MANIFEST_SHA="$(sha256sum "$AARCH64_VERIFICATION_EVIDENCE_MANIFEST" | awk '{print $1}')"
else
  AARCH64_VERIFICATION_ISO_SHA="$(shasum -a 256 "$AARCH64_VERIFICATION_ISO" | awk '{print $1}')"
  AARCH64_VERIFICATION_ISO_MANIFEST_SHA="$(shasum -a 256 "$AARCH64_VERIFICATION_ISO_MANIFEST" | awk '{print $1}')"
  AARCH64_VERIFICATION_BIB_MANIFEST_SHA="$(shasum -a 256 "$AARCH64_VERIFICATION_BIB_MANIFEST" | awk '{print $1}')"
  AARCH64_VERIFICATION_EVIDENCE_MANIFEST_SHA="$(shasum -a 256 "$AARCH64_VERIFICATION_EVIDENCE_MANIFEST" | awk '{print $1}')"
fi
test "$(awk '{print $1; exit}' "$AARCH64_VERIFICATION_ISO_CHECKSUM")" = "$AARCH64_VERIFICATION_ISO_SHA"
test "$(awk '{print $2; exit}' "$AARCH64_VERIFICATION_ISO_CHECKSUM")" = "$(basename "$AARCH64_VERIFICATION_ISO")"
jq -e \
  --arg commit "$GOBLINS_OS_CANDIDATE_COMMIT" \
  --arg image "$AARCH64_IMAGE_REF" \
  --arg run "$AARCH64_RUN_URL" \
  --argjson attempt "$AARCH64_RUN_ATTEMPT" \
  --arg iso_sha "$AARCH64_VERIFICATION_ISO_SHA" \
  --arg iso_manifest_sha "$AARCH64_VERIFICATION_ISO_MANIFEST_SHA" \
  --arg bib_manifest_sha "$AARCH64_VERIFICATION_BIB_MANIFEST_SHA" \
  --arg evidence_manifest_sha "$AARCH64_VERIFICATION_EVIDENCE_MANIFEST_SHA" \
  '.architecture == "aarch64"
   and .candidate_commit == $commit
   and .image_ref == $image
   and .source_verifier == "pass"
   and .installed_root_verifier == "pass"
   and .services_selftest == "pass"
   and .verification_iso_sha256 == $iso_sha
   and .iso_manifest_sha256 == $iso_manifest_sha
   and .bib_manifest_sha256 == $bib_manifest_sha
   and .release_evidence_manifest_sha256 == $evidence_manifest_sha
   and .native_runner == true
   and .workflow_run == $run
   and .workflow_run_attempt == $attempt' \
  "$AARCH64_NATIVE_GATE_PROOF" >/dev/null
```

This verification-ISO artifact is not public release media and is retained only
long enough to feed the local HVF capture. The workflow also uploads the exact
`native-packaging-gate.json` as a small 90-day candidate/date/attempt-bound
artifact; `close-signoff.sh` and final shipping verification require its bytes
to match the local proof. Keep release downloads on GitHub release assets; keep
verification ISO artifacts inside Actions.

```sh
set -euo pipefail

RUN_DATE="$RUN_DATE" \
GOBLINS_OS_ARCH=aarch64 \
GOBLINS_OS_CANDIDATE_COMMIT="$GOBLINS_OS_CANDIDATE_COMMIT" \
GOBLINS_OS_CAPTURE_EXPECTED_IMAGE_REF="$AARCH64_IMAGE_REF" \
GOBLINS_OS_CAPTURE_NATIVE_PACKAGING_GATE_PROOF="$AARCH64_NATIVE_GATE_PROOF" \
GOBLINS_OS_CAPTURE_NATIVE_PACKAGING_GATE_RUN_URL="$AARCH64_RUN_URL" \
GOBLINS_OS_CAPTURE_NATIVE_PACKAGING_GATE_RUN_ATTEMPT="$AARCH64_RUN_ATTEMPT" \
GOBLINS_OS_CAPTURE_ISO="$AARCH64_VERIFICATION_ISO" \
GOBLINS_OS_CAPTURE_ISO_SHA256="$AARCH64_VERIFICATION_ISO_CHECKSUM" \
GOBLINS_OS_CAPTURE_ISO_MANIFEST="$AARCH64_VERIFICATION_ISO_MANIFEST" \
GOBLINS_OS_CAPTURE_BIB_MANIFEST="$AARCH64_VERIFICATION_BIB_MANIFEST" \
GOBLINS_OS_CAPTURE_RELEASE_EVIDENCE_DIR="$AARCH64_VERIFICATION_EVIDENCE_DIR" \
GOBLINS_OS_CAPTURE_REQUIRE_COMPLETE=0 \
REPO_ROOT="$REPO_ROOT" \
os/hardware-gate/capture-harness/run-capture.sh
```

The local capture writes `evidence-bundle.json` only after all 32 required PNGs,
including `05-first-boot-private-unlock.png`, every required proof JSON, and the
three copied verification manifests have been produced. The seal records each
file's exact SHA-256 and byte size, records each PNG's dimensions, and rejects a
run unless every PNG has the same realistic framebuffer dimensions. Symlinks,
non-regular files, path escapes, duplicate paths, duplicate JSON keys, and
non-canonical seal encoding are rejected.

The local HVF host cannot attest itself. Reconstruct and sign the exact seal on
a GitHub-hosted runner, then hydrate only the run-bound attestation record back
into the capture directory:

```sh
set -euo pipefail

AARCH64_SCREENSHOT_RUN_DIR="os/screenshots/hardware-gate/aarch64/$RUN_DATE"
AARCH64_EVIDENCE_SEAL="$AARCH64_SCREENSHOT_RUN_DIR/evidence-bundle.json"
AARCH64_EVIDENCE_SHA="$(python3 os/hardware-gate/capture-harness/evidence_bundle.py inspect \
  --seal "$AARCH64_EVIDENCE_SEAL" \
  --architecture aarch64 \
  --candidate-commit "$GOBLINS_OS_CANDIDATE_COMMIT" \
  --image-ref "$AARCH64_IMAGE_REF" \
  --run-date "$RUN_DATE")"
AARCH64_EVIDENCE_BASE64="$(base64 < "$AARCH64_EVIDENCE_SEAL" | tr -d '\n')"

AARCH64_ATTESTATION_RUN_URL="$(gh workflow run aarch64-local-display-attestation.yml \
  --ref main \
  -f candidate_commit="$GOBLINS_OS_CANDIDATE_COMMIT" \
  -f candidate_image_ref="$AARCH64_IMAGE_REF" \
  -f run_date="$RUN_DATE" \
  -f evidence_bundle_sha256="$AARCH64_EVIDENCE_SHA" \
  -f evidence_bundle_base64="$AARCH64_EVIDENCE_BASE64")"
[[ "$AARCH64_ATTESTATION_RUN_URL" =~ /actions/runs/[0-9]+$ ]]
AARCH64_ATTESTATION_RUN_ID="${AARCH64_ATTESTATION_RUN_URL##*/}"
gh run watch "$AARCH64_ATTESTATION_RUN_ID" --exit-status
AARCH64_ATTESTATION_ATTEMPT="$(gh run view "$AARCH64_ATTESTATION_RUN_ID" --json attempt --jq '.attempt')"
[[ "$AARCH64_ATTESTATION_ATTEMPT" =~ ^[1-9][0-9]*$ ]]
AARCH64_ATTESTATION_ARTIFACT="aarch64-local-display-attestation-$GOBLINS_OS_CANDIDATE_COMMIT-$RUN_DATE-attempt-$AARCH64_ATTESTATION_ATTEMPT"
AARCH64_ATTESTATION_DIR="$(mktemp -d "${TMPDIR:-/tmp}/goblins-os-aarch64-attestation.XXXXXX")"
gh run download "$AARCH64_ATTESTATION_RUN_ID" \
  --name "$AARCH64_ATTESTATION_ARTIFACT" \
  --dir "$AARCH64_ATTESTATION_DIR"
test -s "$AARCH64_ATTESTATION_DIR/evidence-bundle.json"
test -s "$AARCH64_ATTESTATION_DIR/aarch64-local-display-attestation.json"
test ! -L "$AARCH64_ATTESTATION_DIR/evidence-bundle.json"
test ! -L "$AARCH64_ATTESTATION_DIR/aarch64-local-display-attestation.json"
cmp "$AARCH64_EVIDENCE_SEAL" "$AARCH64_ATTESTATION_DIR/evidence-bundle.json"
cp "$AARCH64_ATTESTATION_DIR/aarch64-local-display-attestation.json" \
  "$AARCH64_SCREENSHOT_RUN_DIR/aarch64-local-display-attestation.json"
gh attestation verify "$AARCH64_EVIDENCE_SEAL" \
  --repo Joe-Simo/goblins-os \
  --signer-workflow Joe-Simo/goblins-os/.github/workflows/aarch64-local-display-attestation.yml \
  --signer-digest "$GOBLINS_OS_CANDIDATE_COMMIT" \
  --source-digest "$GOBLINS_OS_CANDIDATE_COMMIT" \
  --deny-self-hosted-runners
```

The attestation workflow needs only `contents: read`, `id-token: write`, and
`attestations: write`; it receives no client secret. GitHub repository artifact
attestations must be available for this public repository. After hydrating the
record, rerun `close-signoff.sh` with the same exact candidate/image/native-gate
variables and `REQUIRE_COMPLETE=1`. Final shipping verification independently
requires the successful workflow attempt, byte-identical uploaded seal and
record, and the signed seal subject from this exact signer workflow.

```sh
set -euo pipefail

AARCH64_RUNTIME_PROOF="$AARCH64_SCREENSHOT_RUN_DIR/runtime-build-proof.json"
AARCH64_RUNTIME_ENGINE_SOURCE="$(jq -er '.engine_source' "$AARCH64_RUNTIME_PROOF")"
GOBLINS_OS_ARCH=aarch64 \
GOBLINS_OS_CANDIDATE_COMMIT="$GOBLINS_OS_CANDIDATE_COMMIT" \
GOBLINS_OS_IMAGE="$AARCH64_IMAGE_REF" \
GOBLINS_OS_NATIVE_PACKAGING_GATE_PROOF="$AARCH64_SCREENSHOT_RUN_DIR/native-packaging-gate.json" \
GOBLINS_OS_NATIVE_PACKAGING_GATE_RUN_URL="$AARCH64_RUN_URL" \
GOBLINS_OS_NATIVE_PACKAGING_GATE_RUN_ATTEMPT="$AARCH64_RUN_ATTEMPT" \
SCREENSHOT_DIR="$AARCH64_SCREENSHOT_RUN_DIR" \
RUNTIME_ENGINE_MODE=local-model \
RUNTIME_ENGINE_SOURCE="$AARCH64_RUNTIME_ENGINE_SOURCE" \
RUNTIME_ENGINE_CONFIG="$AARCH64_RUNTIME_PROOF" \
BUILT_ARTIFACT_PATH_URL="$AARCH64_RUNTIME_PROOF" \
SIGNOFF_ROW_OUTPUT="$AARCH64_SCREENSHOT_RUN_DIR/signoff-row.md" \
REQUIRE_COMPLETE=1 \
os/hardware-gate/close-signoff.sh
```

Overlay only the architecture-scoped x86_64 screenshot proof into the same
disposable exact-candidate checkout. Do not copy the hardware artifact's
verification ISO, SBOM, or full `signoff-notes.md`; those are capture inputs,
not the public release-media authority:

```sh
set -euo pipefail

X86_64_SCREENSHOT_SOURCE="$X86_64_PROOF_DIR/screenshots/hardware-gate/x86_64/$RUN_DATE"
X86_64_SCREENSHOT_DESTINATION="$REPO_ROOT/os/screenshots/hardware-gate/x86_64/$RUN_DATE"
test -d "$X86_64_SCREENSHOT_SOURCE"
test ! -e "$X86_64_SCREENSHOT_DESTINATION"
mkdir -p "$(dirname "$X86_64_SCREENSHOT_DESTINATION")"
cp -a "$X86_64_SCREENSHOT_SOURCE" "$X86_64_SCREENSHOT_DESTINATION"
X86_64_SIGNOFF_ROW="$X86_64_SCREENSHOT_DESTINATION/signoff-row.md"
test -s "$X86_64_SIGNOFF_ROW"
```

Now download the two full public-media artifacts from the exact successful
candidate run. Validate their commit, image digest, human-safe installer config,
exact-candidate gates, and checksum before hydrating the canonical ISO and SBOM
paths. This step intentionally replaces any verification-only ISO/SBOM that the
capture routes materialized there:

```sh
set -euo pipefail

reset_generated_dir() {
  local generated_dir="$1"
  case "$generated_dir" in
    "$REPO_ROOT/os/iso/output/x86_64"|\
    "$REPO_ROOT/os/iso/output/aarch64"|\
    "$REPO_ROOT/os/signoff-proofs/sbom/x86_64"|\
    "$REPO_ROOT/os/signoff-proofs/sbom/aarch64"|\
    "$REPO_ROOT/os/signoff-proofs/candidate/x86_64"|\
    "$REPO_ROOT/os/signoff-proofs/candidate/aarch64") ;;
    *) echo "Refusing to reset unexpected generated directory: $generated_dir" >&2; return 1 ;;
  esac
  test ! -L "$generated_dir"
  mkdir -p "$generated_dir"
  find "$generated_dir" -mindepth 1 -maxdepth 1 -exec rm -rf -- {} +
  test -z "$(find "$generated_dir" -mindepth 1 -maxdepth 1 -print -quit)"
}

. "$REPO_ROOT/os/hardware-gate/release-evidence.sh"
. "$REPO_ROOT/os/hardware-gate/rpm-sbom-arch.sh"
CANDIDATE_PUBLIC_DIR="$(mktemp -d "${TMPDIR:-/tmp}/goblins-os-public-candidate.XXXXXX")"
for ARCH in x86_64 aarch64; do
  ARCH_PUBLIC_DIR="$CANDIDATE_PUBLIC_DIR/$ARCH"
  gh run download "$CANDIDATE_RUN_ID" \
    -n "goblins-os-candidate-$GOBLINS_OS_CANDIDATE_COMMIT-$ARCH" \
    -D "$ARCH_PUBLIC_DIR"

  PUBLIC_ISO_DIR="$ARCH_PUBLIC_DIR/os/iso/output/$ARCH"
  PUBLIC_SBOM_DIR="$ARCH_PUBLIC_DIR/os/signoff-proofs/sbom/$ARCH"
  PUBLIC_ISO="$PUBLIC_ISO_DIR/bootiso/goblins-os-$ARCH.iso"
  PUBLIC_SHA="$PUBLIC_ISO.sha256"
  PUBLIC_MANIFEST="$PUBLIC_ISO_DIR/manifest-goblins-os-$ARCH.json"
  PUBLIC_BIB_MANIFEST="$PUBLIC_ISO_DIR/manifest-anaconda-iso.json"
  PUBLIC_EVIDENCE_MANIFEST="$PUBLIC_SBOM_DIR/release-evidence-manifest.json"
  PUBLIC_CARGO_TSV="$PUBLIC_SBOM_DIR/cargo-lock-packages.tsv"
  PUBLIC_RPM_COMMAND="$PUBLIC_SBOM_DIR/rpm-packages.command"
  PUBLIC_RPM_TSV="$PUBLIC_SBOM_DIR/rpm-packages.tsv"
  PUBLIC_REF_JSON="$ARCH_PUBLIC_DIR/candidate-output/$ARCH/image-ref.json"
  for artifact in \
    "$PUBLIC_ISO" \
    "$PUBLIC_SHA" \
    "$PUBLIC_MANIFEST" \
    "$PUBLIC_BIB_MANIFEST" \
    "$PUBLIC_EVIDENCE_MANIFEST" \
    "$PUBLIC_CARGO_TSV" \
    "$PUBLIC_RPM_COMMAND" \
    "$PUBLIC_RPM_TSV" \
    "$PUBLIC_REF_JSON"; do
    test -s "$artifact"
    test ! -L "$artifact"
  done
  test "$(find "$ARCH_PUBLIC_DIR" -type f -name "goblins-os-$ARCH.iso" -print | wc -l | tr -d '[:space:]')" = 1
  test "$(find "$ARCH_PUBLIC_DIR" -type f -name image-ref.json -print | wc -l | tr -d '[:space:]')" = 1
  test ! -e "$PUBLIC_SBOM_DIR/rpm-packages.not-generated.txt"

  case "$ARCH" in
    x86_64) EXPECTED_IMAGE_REF="$X86_64_IMAGE_REF" ;;
    aarch64) EXPECTED_IMAGE_REF="$AARCH64_IMAGE_REF" ;;
  esac
  PUBLIC_ISO_SHA="$(awk '{ print $1; exit }' "$PUBLIC_SHA")"
  PUBLIC_SHA_ARTIFACT="$(awk '{ print $2; exit }' "$PUBLIC_SHA")"
  [[ "$PUBLIC_ISO_SHA" =~ ^[0-9a-f]{64}$ ]]
  test "$PUBLIC_SHA_ARTIFACT" = "$(basename "$PUBLIC_ISO")"
  goblins_os_release_evidence_hashes_match "$PUBLIC_SBOM_DIR"
  rpm_sbom_arch_matches "$PUBLIC_RPM_TSV" "$ARCH"
  PUBLIC_ISO_MANIFEST_SHA="$(goblins_os_release_evidence_sha256 "$PUBLIC_MANIFEST")"
  PUBLIC_BIB_MANIFEST_SHA="$(goblins_os_release_evidence_sha256 "$PUBLIC_BIB_MANIFEST")"
  PUBLIC_EVIDENCE_MANIFEST_SHA="$(goblins_os_release_evidence_sha256 "$PUBLIC_EVIDENCE_MANIFEST")"
  PUBLIC_CARGO_SHA="$(goblins_os_release_evidence_sha256 "$PUBLIC_CARGO_TSV")"
  PUBLIC_RPM_SHA="$(goblins_os_release_evidence_sha256 "$PUBLIC_RPM_TSV")"
  jq -e \
    --arg arch "$ARCH" \
    --arg commit "$GOBLINS_OS_CANDIDATE_COMMIT" \
    --arg image "$EXPECTED_IMAGE_REF" \
    '.schema == "goblins-os-release-evidence-v4"
     and .architecture == $arch
     and .candidate_commit == $commit
     and .image_ref == $image
     and .image_digest_pinned == true
     and .rpm_status == "generated from rpm database"' \
    "$PUBLIC_EVIDENCE_MANIFEST" >/dev/null
  jq -e \
    --arg arch "$ARCH" \
    --arg commit "$GOBLINS_OS_CANDIDATE_COMMIT" \
    --arg image "$EXPECTED_IMAGE_REF" \
    '.architecture == $arch
     and .candidate_commit == $commit
     and .image == $image
     and .builder_source_image == $image
     and .native_host_arch == $arch
     and .container_engine_arch == $arch
     and .installer_config == "os/iso/config.toml"
     and .installer_branding_applied == true
     and (.installer_branding_image | test("@sha256:[0-9a-f]{64}$"))
     and .installer_branding_ownership_helper_image == .installer_branding_image
     and (.builder_image | test("@sha256:[0-9a-f]{64}$"))
     and .builder_output_ownership_helper_image == .builder_image
     and .installer_payload_source_kind == "release-registry"
     and .installer_payload_source_local_only == false
     and .shippable_release == true' \
    "$PUBLIC_MANIFEST" >/dev/null
  jq -e \
    --arg arch "$ARCH" \
    --arg commit "$GOBLINS_OS_CANDIDATE_COMMIT" \
    --arg image "$EXPECTED_IMAGE_REF" \
    --arg sha "$PUBLIC_ISO_SHA" \
    --arg run "$CANDIDATE_RUN_URL" \
    --argjson attempt "$CANDIDATE_RUN_ATTEMPT" \
    --arg iso_manifest_sha "$PUBLIC_ISO_MANIFEST_SHA" \
    --arg bib_manifest_sha "$PUBLIC_BIB_MANIFEST_SHA" \
    --arg evidence_manifest_sha "$PUBLIC_EVIDENCE_MANIFEST_SHA" \
    --arg cargo_sha "$PUBLIC_CARGO_SHA" \
    --arg rpm_sha "$PUBLIC_RPM_SHA" \
    '.schema == "goblins-os-candidate-image-ref-v2"
     and .architecture == $arch
     and .candidate_commit == $commit
     and .oci_revision == $commit
     and .immutable_image_ref == $image
     and .iso_sha256 == $sha
     and .iso_manifest_sha256 == $iso_manifest_sha
     and .bib_manifest_sha256 == $bib_manifest_sha
     and .release_evidence_manifest_sha256 == $evidence_manifest_sha
     and .cargo_packages_sha256 == $cargo_sha
     and .rpm_packages_sha256 == $rpm_sha
     and .workflow_run == $run
     and .workflow_run_attempt == $attempt
     and .workflow_name == "candidate-artifacts"
     and .source_repository == "https://github.com/Joe-Simo/goblins-os"
     and .installer_config == "os/iso/config.toml"
     and .exact_candidate_gates.source_verifier == "pass"
     and .exact_candidate_gates.installed_root_verifier == "pass"
     and .exact_candidate_gates.services_selftest == "pass"
     and .candidate_tag_authoritative == false
     and .non_promotional == true' \
    "$PUBLIC_REF_JSON" >/dev/null
  if command -v sha256sum >/dev/null 2>&1; then
    PUBLIC_ACTUAL_SHA="$(sha256sum "$PUBLIC_ISO" | awk '{print $1}')"
  else
    PUBLIC_ACTUAL_SHA="$(shasum -a 256 "$PUBLIC_ISO" | awk '{print $1}')"
  fi
  test "$PUBLIC_ACTUAL_SHA" = "$PUBLIC_ISO_SHA"

  DEST_ISO_DIR="$REPO_ROOT/os/iso/output/$ARCH"
  DEST_SBOM_DIR="$REPO_ROOT/os/signoff-proofs/sbom/$ARCH"
  DEST_CANDIDATE_DIR="$REPO_ROOT/os/signoff-proofs/candidate/$ARCH"
  reset_generated_dir "$DEST_ISO_DIR"
  reset_generated_dir "$DEST_SBOM_DIR"
  reset_generated_dir "$DEST_CANDIDATE_DIR"
  mkdir -p "$DEST_ISO_DIR/bootiso"
  cp "$PUBLIC_ISO" "$DEST_ISO_DIR/bootiso/goblins-os-$ARCH.iso"
  cp "$PUBLIC_SHA" "$DEST_ISO_DIR/bootiso/goblins-os-$ARCH.iso.sha256"
  cp "$PUBLIC_MANIFEST" "$DEST_ISO_DIR/manifest-goblins-os-$ARCH.json"
  cp "$PUBLIC_BIB_MANIFEST" "$DEST_ISO_DIR/manifest-anaconda-iso.json"
  cp "$PUBLIC_CARGO_TSV" "$DEST_SBOM_DIR/cargo-lock-packages.tsv"
  cp "$PUBLIC_RPM_COMMAND" "$DEST_SBOM_DIR/rpm-packages.command"
  cp "$PUBLIC_RPM_TSV" "$DEST_SBOM_DIR/rpm-packages.tsv"
  # Copy the hash-sealing manifest last so interrupted hydration cannot look complete.
  cp "$PUBLIC_EVIDENCE_MANIFEST" "$DEST_SBOM_DIR/release-evidence-manifest.json"
  goblins_os_release_evidence_hashes_match "$DEST_SBOM_DIR"
  cp "$PUBLIC_REF_JSON" "$DEST_CANDIDATE_DIR/image-ref.json"
done
```

Compose the two complete per-architecture rows explicitly, then run the final
gate against the hydrated public release media:

```sh
set -euo pipefail

AARCH64_SIGNOFF_ROW="$REPO_ROOT/os/screenshots/hardware-gate/aarch64/$RUN_DATE/signoff-row.md"
test -s "$AARCH64_SIGNOFF_ROW"
test -s "$X86_64_SIGNOFF_ROW"
REPO_ROOT="$REPO_ROOT" \
GOBLINS_OS_CANDIDATE_COMMIT="$GOBLINS_OS_CANDIDATE_COMMIT" \
  bash os/hardware-gate/compose-signoff-rows.sh \
    "$X86_64_SIGNOFF_ROW" \
    "$AARCH64_SIGNOFF_ROW"

GOBLINS_OS_CANDIDATE_COMMIT="$GOBLINS_OS_CANDIDATE_COMMIT" \
  ./os/hardware-gate/verify-shipping-status.sh
```

The capture harness verifies the downloaded checksum and candidate provenance,
the selected immutable image reference, and the native Linux packaging gate;
then it temporarily hard-links or copies the verification ISO, both manifests,
and release evidence into canonical `os/` paths for close-signoff. Final
composition replaces those canonical media/evidence paths with the human-safe
public artifacts from the exact candidate run. The screenshot proof manifest
retains the separate verification ISO digest so the two roles cannot be confused.
This route still requires a native `qemu-system-aarch64`, UEFI firmware, and
enough free space for the VM scratch disk and proof output. The capture harness defaults to an
80G sparse scratch disk; set `GOBLINS_OS_CAPTURE_DISK_SIZE` only when the host
has a separately validated disk-size requirement. The harness boots the
verification ISO only for the install pass and then prefers the installed VM
disk after Anaconda reboots. `x86_64` uses QEMU one-time ISO boot order;
`aarch64` uses a two-phase capture because QEMU aarch64 does not support the
same boot-order override. For aarch64, the install ISO is presented as USB
storage so the scratch disk remains virtio vda for the verification kickstart.
Use `GOBLINS_OS_CAPTURE_ISO` and `GOBLINS_OS_CAPTURE_ISO_SHA256` only when the
verification ISO is stored outside the default output path. It does not replace
the GHCR/package visibility check
or the release artifact/SBOM build.

### Docker artifact testing on a non-native machine

For local testing only, Docker Desktop or another Docker engine may be used to
try a non-native artifact build with emulation:

```sh
set -euo pipefail

GOBLINS_OS_ARCH=x86_64 \
GOBLINS_OS_CANDIDATE_COMMIT="$GOBLINS_OS_CANDIDATE_COMMIT" \
RUN_QEMU=0 \
GOBLINS_OS_ALLOW_EMULATED_DOCKER=1 \
MIN_HOST_FREE_GB=120 \
REPO_ROOT="$REPO_ROOT" \
os/hardware-gate/run-external-gate.sh
```

This path is intentionally not release proof. It does not launch the
display-backed VM, cannot satisfy screenshot or signoff rows, and still fails
fast if the Docker emulation backend cannot run the Rust toolchain. Use it only
to debug artifact generation before moving to a native Linux/KVM runner.

## 1) Build installer ISO
```sh
set -euo pipefail

cd "$REPO_ROOT"
ARCH=x86_64 # or aarch64 on a native aarch64 Linux runner
docker pull "$RELEASE_IMAGE"
GOBLINS_OS_CONTAINER_RUNTIME=docker \
GOBLINS_OS_ARCH="$ARCH" \
GOBLINS_OS_IMAGE="$RELEASE_IMAGE" \
GOBLINS_OS_SKIP_LOCAL_IMAGE_BUILD=1 \
GOBLINS_OS_CANDIDATE_COMMIT="$GOBLINS_OS_CANDIDATE_COMMIT" \
GOBLINS_OS_BIB_SOURCE_IMAGE="$RELEASE_IMAGE" \
GOBLINS_OS_SHIPPABLE_RELEASE=1 \
os/iso/build-iso.sh
```

Expected outputs:
- `os/iso/output/$ARCH/bootiso/goblins-os-$ARCH.iso`
- `os/iso/output/$ARCH/bootiso/goblins-os-$ARCH.iso.sha256`
- `os/iso/output/$ARCH/manifest-goblins-os-$ARCH.json`

The generated ISO manifest must record `"installer_payload_source_local_only": false`,
`"shippable_release": true`, `"candidate_commit"` equal to the exact selected
commit, and `"builder_source_image"` equal to the digest-pinned `RELEASE_IMAGE`.
If any field differs, discard
that ISO for release signoff and rebuild with `GOBLINS_OS_BIB_SOURCE_IMAGE`
pointing at the real release image.

The GitHub `candidate-artifacts` workflow builds each exact candidate under a
commit-scoped GHCR tag, captures the registry digest, and produces shippable ISO
and SBOM artifacts without updating a release channel or writing evidence to
Git. The `hardware-gate-capture` and `aarch64-verification-iso` workflows consume
that digest directly and only upload short-lived artifacts. The local-display
attestation workflow uploads and signs only the canonical aarch64 evidence
seal. None can write repository contents. Download and review both architecture
outputs in a disposable exact-candidate checkout before attaching the proof to
the release.

## 2) Write ISO + boot display-backed VM
```sh
set -euo pipefail

ARCH=x86_64
ISO="os/iso/output/$ARCH/bootiso/goblins-os-$ARCH.iso"
qemu-system-x86_64 -m 8192 -smp 4 \
  -accel kvm \
  -cdrom "$ISO" \
  -drive file=/tmp/goblins-os-$ARCH.qcow2,if=virtio,format=qcow2 \
  -boot order=c,once=d -vga std -display gtk \
  -serial mon:stdio
```

For aarch64 on a native aarch64 Linux runner:

```sh
set -euo pipefail

ARCH=aarch64
ISO="os/iso/output/$ARCH/bootiso/goblins-os-$ARCH.iso"
AARCH64_UEFI_CODE="${AARCH64_UEFI_CODE:-/usr/share/edk2/aarch64/QEMU_EFI-pflash.raw}"
AARCH64_UEFI_VARS="${AARCH64_UEFI_VARS:-/tmp/goblins-os-$ARCH-uefi-vars.fd}"
AARCH64_UEFI_VARS_TEMPLATE="${AARCH64_UEFI_VARS_TEMPLATE:-/usr/share/edk2/aarch64/vars-template-pflash.raw}"
[ -f "$AARCH64_UEFI_VARS" ] || cp "$AARCH64_UEFI_VARS_TEMPLATE" "$AARCH64_UEFI_VARS"
qemu-system-aarch64 -machine virt,accel=kvm,gic-version=max -cpu host -m 8192 -smp 4 \
  -drive if=pflash,format=raw,readonly=on,file="$AARCH64_UEFI_CODE" \
  -drive if=pflash,format=raw,file="$AARCH64_UEFI_VARS" \
  -drive if=none,id=install_iso,file="$ISO",media=cdrom,readonly=on \
  -drive file=/tmp/goblins-os-$ARCH.qcow2,if=virtio,format=qcow2 \
  -device qemu-xhci \
  -device usb-storage,drive=install_iso,bootindex=1 \
  -no-reboot -device virtio-gpu-pci -display gtk \
  -serial mon:stdio
```

When the aarch64 installer reboots and QEMU exits, restart the same VM disk
without the ISO:

```sh
set -euo pipefail

qemu-system-aarch64 -machine virt,accel=kvm,gic-version=max -cpu host -m 8192 -smp 4 \
  -drive if=pflash,format=raw,readonly=on,file="$AARCH64_UEFI_CODE" \
  -drive if=pflash,format=raw,file="$AARCH64_UEFI_VARS" \
  -drive file=/tmp/goblins-os-$ARCH.qcow2,if=virtio,format=qcow2 \
  -device virtio-gpu-pci -display gtk \
  -serial mon:stdio
```

For headless/debug capture only, remove `-display` and use `-nographic`.

Complete the install flow, reboot into the Goblins OS session, and verify the
first-boot identity/onboarding gate rather than creating an installer-local
password.

## 3) Capture required proof assets (during the run)
Use the host screenshot tool for the live session and save to:

`os/screenshots/hardware-gate/<arch>/<YYYY-MM-DD>/`

Legacy/non-shipping screenshot roots that are not under
`os/screenshots/hardware-gate/<arch>/<YYYY-MM-DD>/` are migration history only.
Do not copy, rename, or re-date them into an architecture root. Reboot the
current ISO in the display-backed VM or hardware path, capture fresh screenshots,
and generate a new `proof-manifest.json` tied to the current ISO and SHA.

Add `proof-manifest.json` beside the screenshots so the proof root is tied to
the release media that was booted:

```json
{
  "architecture": "<arch>",
  "candidate_commit": "<same selected 40-hex source commit>",
  "image_ref": "<registry>/<namespace>/goblins-os@sha256:<64-hex-digest>",
  "iso": "os/iso/output/<arch>/bootiso/goblins-os-<arch>.iso",
  "iso_sha256": "<64-char sha256 from the matching .sha256 file>",
  "captured_at": "<UTC timestamp>",
  "screenshot_run_dir": "os/screenshots/hardware-gate/<arch>/<YYYY-MM-DD>",
  "firewall_live_toggle_proof": "firewall-live-toggle-proof.json",
  "text_shortcuts_session_enable_proof": "text-shortcuts-session-enable-proof.json",
  "text_shortcuts_candidate_metadata_proof": "text-shortcuts-candidate-metadata-proof.json",
  "text_shortcuts_overlay_intent_proof": "text-shortcuts-overlay-intent-proof.json",
  "text_shortcuts_candidate_bubble_frame_proof": "text-shortcuts-candidate-bubble-frame-proof.json",
  "text_shortcuts_candidate_bubble_layout_proof": "text-shortcuts-candidate-bubble-layout-proof.json",
  "text_shortcuts_candidate_bubble_render_intent_proof": "text-shortcuts-candidate-bubble-render-intent-proof.json",
  "text_shortcuts_candidate_bubble_render_proof": "text-shortcuts-candidate-bubble-render-proof.json",
  "text_shortcuts_live_ibus_runtime_render_proof": "text-shortcuts-live-ibus-runtime-render-proof.json",
  "text_shortcuts_live_ibus_runtime_render_screenshot_sha256": "<64-char sha256 of screenshot 32>",
  "keyboard_shortcuts_roundtrip_proof": "keyboard-shortcuts-roundtrip-proof.json",
  "input_sources_roundtrip_proof": "input-sources-roundtrip-proof.json",
  "multi_display_apply_proof": "multi-display-apply-proof.json",
  "focus_arm_roundtrip_proof": "focus-arm-roundtrip-proof.json",
  "app_privacy_revoke_proof": "app-privacy-revoke-proof.json",
  "preview_open_render_proof": "preview-open-render-proof.json",
  "audio_output_proof": "audio-output-proof.json",
  "runtime_build_proof": "runtime-build-proof.json"
}
```

`close-signoff.sh` fully decodes every screenshot and rejects missing, empty,
oversized, symlinked, multi-frame, or invalid PNG files. It also requires the
screenshot 32 SHA-256 in its live proof and manifest to equal the actual decoded
file, recomputes the canonical `evidence-bundle.json` covering all 32 uniform
framebuffer PNGs and every required JSON, and rejects a manifest that does not
match the current architecture ISO and SHA. It
also rejects the run unless `firewall-live-toggle-proof.json` records the live
core route disabling firewalld with HTTP 200 and observed inactive status, then
enabling it with HTTP 200 and observed active status through the scoped systemd
oneshot/polkit bridge.

The same run must include `text-shortcuts-session-enable-proof.json`. That proof
covers live session plumbing: the Fedora GNOME IBus service
(`org.freedesktop.IBus.session.GNOME.service`), the seeded
`goblins-textshortcuts` input source and preload engine, active engine selection,
the adapter self-test, and core confirmation that the runtime loop is available.
It is a prerequisite, not visual or keystroke release evidence by itself.

The candidate metadata probe and the adapter's `--overlay-intent-self-test`,
`--candidate-bubble-frame-self-test`, `--candidate-bubble-layout-self-test`, and
`--candidate-bubble-render-intent-self-test` are non-live build-time behavior
contracts. They may be retained to catch adapter regressions, but their outputs
must not satisfy the production popup claim. The capture manifest and signoff
may retain them as explicitly non-live diagnostic preflight attachments so
their regression checks remain traceable. In particular,
`31-text-shortcuts-candidate-bubble-render.png` is a synthetic diagnostic
surface, not evidence of the production popup; only screenshot 32 and its
native IBus proof may satisfy that release claim.

The Text Shortcuts release gate is
`text-shortcuts-live-ibus-runtime-render-proof.json` plus
`32-text-shortcuts-live-ibus-runtime-render.png`. It runs in the installed
GNOME session with the active `goblins-textshortcuts` IBus engine and host QMP
keyboard input. The only accepted candidate renderer is the native IBus
lookup-table popup; the proof must record `synthetic_overlay=false`.

Before typing, the gate writes and reads the private desktop-user shortcut table
through `/v1/text-shortcuts`, verifies `/v1/text-shortcuts/preview`, and checks
the bounded file contract. That contract requires a private parent directory, a
regular owner-only table, a single link, bounded size and bounded reads, plus
absence of the legacy service-user table. The live IBus watcher must reload the
new table, and the same API and file checks must still pass after the keystrokes.

The normal-input ledger is sliced after seed setup so an earlier seed event
cannot satisfy the release gate. That slice must contain the focused-field,
process-key, cursor-location, and candidate-publication records. Immediately
before typing the accepting boundary, the gate records a second ledger offset.
The pre-boundary slice must contain zero `commit-text` operations. The boundary
slice must contain exactly one `commit-text` operation, and it must belong to a
handled `process-key-event` record. The focused entry must read back exactly
`on my way.`, while the unknown shortcut must read back exactly `hello.`. A
password-purpose field must process the keys without producing a commit,
candidate, or native popup.

Before screenshot 32, the gate selects the chronologically last native popup
record and requires a positive generation, a positive record ordinal, and a
published `show-candidate` action from `native-ibus-lookup-table`, plus a real
cursor rectangle, the expected replacement, and the published hint. Generation
is intentionally not used to sort records because it is local to an engine
instance and can restart. The guest holds that popup while the host settles and
writes the QMP framebuffer. The host then publishes
`/capture-acks/32-text-shortcuts-live-ibus-runtime-render.captured`; only after
that acknowledgement may the guest type the accepting boundary. The guest first
rechecks that the popup count still equals the captured ordinal and that the
chronologically last record still has the captured generation and show state.
After the boundary, exactly one new popup record must be published at the next
generation with action `hide-candidate`; its paired render intent must have
reason `committed`.

Because the HTTP proof serializer writes query values as JSON strings, a passing
artifact must contain this schema:

```json
{
  "status": "pass",
  "route": "/v1/text-shortcuts",
  "preview_route": "/v1/text-shortcuts/preview",
  "surface": "goblins-textshortcuts-live-ibus-runtime-render",
  "input_driver": "qmp-keyboard",
  "active_engine": "goblins-textshortcuts",
  "seed_write_http": "200",
  "seed_read_http": "200",
  "seed_roundtrip": "true",
  "seed_loaded": "true",
  "core_write_http": "200",
  "core_read_http": "200",
  "core_preview_http": "200",
  "file_contract_http": "200",
  "core_table_roundtrip": "true",
  "core_preview_roundtrip": "true",
  "desktop_file_contract": "true",
  "desktop_parent_contract": "true",
  "desktop_file_owner_mode": "true",
  "desktop_file_single_link": "true",
  "desktop_file_size_bounded": "true",
  "desktop_file_bounded_read": "true",
  "legacy_service_table_absent": "true",
  "live_watcher_reload": "true",
  "post_keystroke_read_http": "200",
  "post_keystroke_file_http": "200",
  "post_keystroke_roundtrip": "true",
  "normal_actual": "on my way.",
  "passthrough_actual": "hello.",
  "password_refusal": "true",
  "password_sensitive_purpose": "true",
  "password_process_key_callback": "true",
  "password_commit_absent": "true",
  "password_candidate_absent": "true",
  "password_popup_absent": "true",
  "normal_stage_ledger_scoped": "true",
  "focused_field_callback": "true",
  "process_key_event_callback": "true",
  "cursor_location_callback": "true",
  "pre_boundary_commit_absent": "true",
  "boundary_stage_ledger_scoped": "true",
  "boundary_stage_commit_count": "1",
  "normal_stage_commit": "true",
  "ibus_commit_operation": "true",
  "focused_entry_readback": "true",
  "ibus_commit_delivered": "true",
  "boundary_popup_action": "hide-candidate",
  "boundary_popup_reason": "committed",
  "candidate_intent_seen": "true",
  "native_ibus_candidate_published": "true",
  "native_popup_generation": "<positive decimal>",
  "native_popup_record_ordinal": "<positive decimal>",
  "native_popup_generation_current": "true",
  "native_popup_record_current_at_capture": "true",
  "native_popup_action": "show-candidate",
  "native_popup_has_cursor_rect": "true",
  "native_popup_expected_replacement": "true",
  "native_popup_hint_published": "true",
  "renderer": "native-ibus-lookup-table",
  "cursor_anchor": "ibus-input-context",
  "synthetic_overlay": "false",
  "screenshot": "32-text-shortcuts-live-ibus-runtime-render.png",
  "screenshot_sha256": "<64-char sha256 from the validated capture acknowledgement>",
  "screenshot_capture_ack": "true",
  "native_candidate_popup_ready_claim": "true",
  "live_overlay_claim": "true",
  "runtime_ready_claim": "true",
  "core_readiness_flip": "live"
}
```

`native_popup_generation` and `native_popup_record_ordinal` must both match
`^[1-9][0-9]*$`. `native_popup_generation_current=true` and
`native_popup_record_current_at_capture=true` describe the acknowledged
`show-candidate` record at screenshot-capture time; they do not claim it remains
visible after acceptance. The final chronological popup must instead be the
proved `hide-candidate` / `committed` transition. The exact focused-entry
readback, pre-boundary commit absence, single boundary commit, captured popup
identity, and host capture acknowledgement are all required. The host validates
the complete PNG stream before atomically publishing its acknowledgement; the
guest copies that exact SHA-256 into the live proof, and the proof manifest must
repeat the digest. Both signoff validators decode the file again and require all
three digests to match. No readiness boolean may substitute for this evidence.

The keyboard-shortcuts gate is `keyboard-shortcuts-roundtrip-proof.json`. It
posts to `/v1/keyboard/shortcuts/binding` to set the owned `window-hud` shortcut
to `<Super><Shift>H`, verifies the GNOME setting read-back, resets it to the
Goblins default `<Super>w`, posts to `/v1/keyboard/modifier-remap` to map Caps
Lock to Control, verifies `ctrl:nocaps`, then restores the default modifier
behavior. This is a live qemu write proof for the already allowlisted bridge; it
does not mark the Keyboard Settings UI render shipped on its own.

The input-sources gate is `input-sources-roundtrip-proof.json`. It saves the
current `org.gnome.desktop.input-sources` source list and current index, posts
to `/v1/input/sources` with the deterministic `xkb/us` plus `xkb/gb` list,
verifies gsettings read-back, seeds current index `0`, posts
`/v1/input/switch-next`, verifies the current index becomes `1`, then restores
the original source list and current index before signoff. This proves the
existing IME/input-source write and switch bridges in qemu without depending on
a CJK engine being active and without marking the Settings input-source UI
render shipped.

The multi-display apply gate is `multi-display-apply-proof.json`, linked from
`proof-manifest.json` as `multi_display_apply_proof`. It queries the live Mutter
DisplayConfig state, builds a same-layout `/v1/displays/apply` payload from the
current serial/connector/mode, proves `method=verify` and `method=temporary`
return HTTP 200, proves persistent apply is rejected without explicit Keep
confirmation, and proves a stale serial is rejected. This proves the protected
DisplayConfig write bridge in qemu; it does not claim the writable Displays
canvas, multi-output editing, or persistent Keep/Revert UI shipped.

The Focus arm gate is `focus-arm-roundtrip-proof.json`, linked from
`proof-manifest.json` as `focus_arm_roundtrip_proof`. It saves the current
Goblins Focus mode state and GNOME notification banner preference, seeds a
deterministic `gate-work` mode, posts `/v1/focus/activate`, verifies
`active-mode=gate-work`, `armed-by-schedule=false`, the saved banner snapshot,
and `show-banners=false`, then posts `/v1/focus/deactivate`, verifies the active
mode and restore snapshot are cleared and banners return to true, and finally
restores the original Focus and notification state before signoff. This proves
the existing arm/disarm bridge in qemu; it does not claim mode CRUD, schedule
timers, or per-app breakthrough behavior shipped.

The App privacy revoke gate is `app-privacy-revoke-proof.json`, linked from
`proof-manifest.json` as `app_privacy_revoke_proof`. It snapshots the
PermissionStore state for a deterministic `org.goblins.GatePrivacyProof`
location grant, seeds that grant through `PermissionStore.SetPermission`, posts
the existing `/v1/app-privacy/revoke` route, verifies
`PermissionStore.GetPermission` no longer reports the grant, and restores the
prior state before signoff. This proves the app-keyed revoke bridge in qemu; it
does not claim resource-keyed camera/microphone revoke behavior.

The Preview open/render gate is `preview-open-render-proof.json`, linked from
`proof-manifest.json` as `preview_open_render_proof`. It queries
`/v1/preview/status`, verifies Papers/Loupe are available through the core
status contract, verifies `xdg-mime` defaults for PDF/PNG/JPEG point to
`org.gnome.Papers.desktop` and `org.gnome.Loupe.desktop`, opens the installed
fixtures at `/usr/share/goblins-os/proof/preview-open-render.{pdf,png}` through
`/v1/preview/open`, waits for the real `papers` and `loupe` processes, captures
`29-preview-pdf-open.png` and `30-preview-image-open.png`, and confirms an
unsupported `.txt` fixture is rejected with HTTP 400. This proves the installed
desktop open path in a display-backed qemu session; it does not mark Preview
shipped until the qemu artifacts are reviewed.

The audio-output gate is `audio-output-proof.json`, linked from
`proof-manifest.json` as `audio_output_proof`. It queries `/v1/audio/status`,
requires WirePlumber and a default output to be reported by the core, generates
a bounded local WAV probe, plays it with `pw-play` or `paplay`, and captures
`24-audio-output.png` only after the real Sound panel window is mapped. This
proves PipeWire output readiness in qemu without claiming external speaker
hardware, microphone capture, or arbitrary app audio routing.

The runtime-build gate is `runtime-build-proof.json`, linked from
`proof-manifest.json` as `runtime_build_proof`. It grants the app-builder
control, calls `/v1/apps/builds` with a bounded app intent, waits for the live
response, and records the returned build id, name, and engine source. A run
without this proof cannot complete signoff because Build Studio screenshots
alone do not prove a real app-build turn.

If the display-backed screenshot run already exists but the runtime proof is
missing, run `os/runtime-gate/build-an-app-live-model.sh` from inside a Goblins
OS image/container that is joined to a real local model runtime. Set
`PROOF_PATH=os/screenshots/hardware-gate/<arch>/<date>/runtime-build-proof.json`
and `BUILD_RESPONSE_PATH=os/screenshots/hardware-gate/<arch>/<date>/build-response.json`.
Do not hand-write this file; the proof must be produced from the live
`/v1/apps/builds` response.

Capture exactly at minimum these names:
1. `01-installer.png` — ISO boot + installer launch
2. `02-install-network.png` — installer network/progress
3. `03-login.png` — login screen
4. `04-desktop.png` — first native desktop session
5. `06-onboarding.png` — first-boot onboarding page
6. `07-home.png` — post-onboarding home
7. `08-shell-home.png` — shell launch
8. `09-shell-dark.png` — shell dark-theme state
9. `10-settings.png` — settings page
10. `11-settings-models.png` — settings models section
11. `12-settings-dark.png` — settings dark-theme state
12. `13-studio-before.png` — Build Studio prompt
13. `14-studio-running.png` — studio running
14. `15-studio-app-detail.png` — built-app detail
15. `16-built-app-open.png` — open built app
16. `17-dark-motion.png` — dark-theme motion/interactions
17. `18-light-motion.png` — light-theme motion/interactions
18. `19-vulkan-vkcube.png` — native Vulkan sample running in the installed session
19. `20-gamemode-active.png` — GameMode activation command result
20. `21-gamescope-session.png` — Gamescope-launched nested session or app
21. `22-mangohud-overlay.png` — MangoHud overlay visible over a user-launched sample
22. `23-controller-detection.png` — connected controller/gamepad detected by the OS
23. `24-audio-output.png` — PipeWire audio sink/output proof while a test sound is playing
24. `25-install-destination.png` — advanced storage Installation Destination showing explicit disk choice
25. `26-install-storage-summary.png` — storage summary showing formatting/root filesystem before writing changes
26. `27-dual-boot-preserve-existing-os.png` — the native installer's Open advanced storage path or the desktop Install Goblins OS Beside Another OS entry, followed by Custom/manual storage or Reclaim Space showing Goblins OS installed into unallocated free space or a dedicated disk while existing Windows, macOS/APFS, Linux, other OS, recovery, and EFI partitions are preserved
27. `28-bootloader-efi-summary.png` — bootloader/EFI target summary before beginning install
28. `29-preview-pdf-open.png` — Papers showing the installed Preview proof PDF opened through `/v1/preview/open`
29. `30-preview-image-open.png` — Loupe showing the installed Preview proof PNG opened through `/v1/preview/open`

Suggested installed-session commands for the gaming screenshots:

```sh
set -euo pipefail

# Native Vulkan sample. Capture the window while it is rendering.
vkcube

# Vulkan/device summary. Useful to keep visible beside vkcube when space allows.
vulkaninfo --summary

# Video acceleration diagnostics. Capture the supported VA-API profile output.
vainfo

# VDPAU wrapper diagnostics. Capture the provider result when a GPU exposes VDPAU.
vdpauinfo

# GameMode activation path. Capture the terminal result.
gamemoded -t || gamemoderun sh -lc 'echo "GameMode launch path executed"; sleep 5'

# Gamescope nested compositor/session. Launch a short sample and capture the window.
gamescope -- vkcube

# MangoHud overlay over a user-launched sample. Capture the overlay text.
mangohud vkcube

# Controller detection. Attach a controller or pass one through to the VM first.
cat /proc/bus/input/devices | rg -i 'gamepad|joystick|controller|xbox|dualsense|dualshock'
lsusb
evtest --query /dev/input/event0 EV_KEY BTN_GAMEPAD || true

# Audio output. Capture sink listing plus audible/signal activity.
wpctl status
pw-cli info 0
pw-dump | sed -n '1,200p'
pactl list short sinks
speaker-test -t sine -l 1
```

After the run, open [os/signoff-notes.md](os/signoff-notes.md) and fill:
- date/run id
- device/runner + ISO hash
- command used
- release evidence path under `os/signoff-proofs/sbom/<arch>/`
- each check pass/fail and screenshot filenames
- canonical `evidence-bundle.json` SHA-256 and the exact workflow artifact/run attempt that carried it
- for local aarch64/HVF, the GitHub-hosted signed attestation record and verified signer workflow
- SBOM result, including `release-evidence-manifest.json`, `cargo-lock-packages.tsv`, and `rpm-packages.tsv`
- gaming readiness result, including Steam absence from installed-root verifier
- firewall toggle result, including `firewall-live-toggle-proof.json`
- Text Shortcuts session-enable result, including `text-shortcuts-session-enable-proof.json`
- Text Shortcuts non-live diagnostic preflight results, including
  `text-shortcuts-candidate-metadata-proof.json`,
  `text-shortcuts-overlay-intent-proof.json`,
  `text-shortcuts-candidate-bubble-frame-proof.json`,
  `text-shortcuts-candidate-bubble-layout-proof.json`,
  `text-shortcuts-candidate-bubble-render-intent-proof.json`, and
  `text-shortcuts-candidate-bubble-render-proof.json`; these rows cannot satisfy
  the production popup claim
- Text Shortcuts live IBus result, including secure desktop-state roundtrips,
  watcher reload, zero pre-boundary commits, one boundary-stage commit and
  focused-entry readback, password suppression, the chronologically current
  captured native lookup-table popup and its committed hide transition, plus
  host-acknowledged `32-text-shortcuts-live-ibus-runtime-render.png`, all recorded
  by `text-shortcuts-live-ibus-runtime-render-proof.json`
- Keyboard shortcuts roundtrip result, including `keyboard-shortcuts-roundtrip-proof.json`
- Input sources roundtrip result, including `input-sources-roundtrip-proof.json`
- Multi-display apply result, including `multi-display-apply-proof.json`
- Focus arm roundtrip result, including `focus-arm-roundtrip-proof.json`
- App privacy revoke result, including `app-privacy-revoke-proof.json`
- Preview open/render result, including `preview-open-render-proof.json`, `29-preview-pdf-open.png`, and `30-preview-image-open.png`
- install destination, formatting/root filesystem, bootloader/EFI, and dual-boot preservation result
- for custom formatting, encryption, separate `/home`, LUKS/LVM, TPM2 LUKS, ext4, or btrfs, show an advanced storage summary before writes
- if dual boot is tested, show the Open advanced storage action or Install Goblins OS Beside Another OS desktop entry, Custom/manual storage or Reclaim Space, the free-space/dedicated-disk target, the backup/free-space preparation note, and the untouched existing OS/recovery/EFI partitions
- if the native installer is used, show that the simple flow proceeds only for a blank disk and routes disks with existing Windows/macOS/APFS/Linux/other OS/recovery/EFI/data partitions to manual storage
- blockers
- verify every required file above exists before marking the run complete

Then validate the local proof set programmatically:

```sh
set -euo pipefail

ARCH=x86_64 # or aarch64
RUN_DATE="${RUN_DATE:-$(date -u +%F)}"
SCREENSHOT_RUN_DIR="os/screenshots/hardware-gate/$ARCH/$RUN_DATE"
GOBLINS_OS_CANDIDATE_COMMIT="$GOBLINS_OS_CANDIDATE_COMMIT" \
GOBLINS_OS_ARCH="$ARCH" GOBLINS_OS_IMAGE="$RELEASE_IMAGE" \
SCREENSHOT_RUN_DIR="$SCREENSHOT_RUN_DIR" \
  ./os/hardware-gate/close-signoff.sh
```

The helper may generate source-only evidence as a diagnostic, but final release
evidence must come from the packaged `goblins-os-verify --release-evidence`
inside the exact digest-pinned architecture image. Replaying
`rpm-packages.command` by itself does not satisfy final release evidence. The
accepted set must contain a v4 `release-evidence-manifest.json`,
`cargo-lock-packages.tsv`, and `rpm-packages.tsv` from that packaged-verifier
invocation, with both inventory SHA256 values matching the manifest. The
manifest must also record `asset_provenance`, `third_party_notices`,
`trademark_posture`, and `source_tree_manifest` paths so release reviewers can
trace each architecture artifact back to the source-package diligence files.
It must also record the same `candidate_commit` and digest-pinned `image_ref` as
the ISO, screenshot proof, and signoff row. Missing or mismatched provenance
fields fail closed.
The helper and final shipping gate also run the artifact/evidence secret scan
over generated release evidence, signoff notes, ISO manifests, SHA files,
release tables, and command files. Binary ISO/image payloads and historical
runtime proof dumps are not treated as text scan inputs.

If the helper exits non-zero, fix missing artifacts and rerun.

## 4) Run runtime model path (choose one)
- Preferred: local model path (for example a downloaded GPT-OSS model folder).
- Alternative: BYO OpenAI key.
- Alternative: BYO Codex/session path.

Start a full Build Studio turn and verify:
- app card is created and visible in ledger
- opening it enters built-app detail
- Open in Build Studio works
- user-visible built app artifact appears (and opens)

Document the exact engine and result in [os/signoff-notes.md](os/signoff-notes.md).

## 5) Closed-loop verification on host image artifacts
Use this quick evidence audit first:

```sh
set -euo pipefail

GOBLINS_OS_CANDIDATE_COMMIT="$GOBLINS_OS_CANDIDATE_COMMIT" \
  ./os/hardware-gate/verify-shipping-status.sh
```

Use this helper first to validate local workflow expectations and run installed-root checks:

```sh
set -euo pipefail

GOBLINS_OS_CANDIDATE_COMMIT="$GOBLINS_OS_CANDIDATE_COMMIT" \
  ./os/hardware-gate/close-signoff.sh
```

It appends a scaffold run entry into `os/signoff-notes.md` and reports:
- workflow gate presence
- image existence
- ISO presence/hash
- verify blocked=0 result (if image is present)
- self-test container build attempt

From a host with Docker:

```sh
set -euo pipefail

RUNTIME=docker

# Packaging contract
$RUNTIME run --rm localhost/goblins-os:$ARCH \
  /usr/libexec/goblins-os/goblins-os-verify --installed-root / | tee verify.log
grep -q "blocked=0" verify.log

# Self-test pass (installed rootfs)
SELFTEST_DIR="$(mktemp -d "${TMPDIR:-/tmp}/goblins-os-selftest.XXXXXX")"
cat os/bootc/Containerfile os/bootc/selftest.suffix.Dockerfile > "$SELFTEST_DIR/selftest.Dockerfile"
DOCKER_BUILDKIT=1 $RUNTIME buildx build -f "$SELFTEST_DIR/selftest.Dockerfile" --target selftest --output type=cacheonly .
```

For CI confirmation, ensure the three workflow jobs complete successfully:
- rust
- image
- installer-iso
