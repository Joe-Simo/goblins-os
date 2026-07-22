# Goblins OS External Sign-off Runbook

Build and verify shippable release artifacts on a native aarch64 Linux host.
Capture the final display-backed proof separately on Apple Silicon with
Darwin/arm64 and HVF. The capture harness boots a verification-only ISO built
with `os/iso/verify-config.toml` from the same real pullable bootc image ref used
by release media. Do not point the capture harness at hydrated public release
media: release ISOs are human-safe and intentionally leave storage interactive.

Set:

```sh
set -euo pipefail

REPO_ROOT="${REPO_ROOT:-$(pwd)}"
cd "$REPO_ROOT"
export GOBLINS_OS_CANDIDATE_COMMIT="$(git rev-parse HEAD)"
```

## 0) Preflight
- Confirm runtime requirements on the host:
  - On the native aarch64 Linux packaging runner: `docker`, at least 120 GiB
    free, and a responsive `docker info`. QEMU/KVM is optional diagnostic
    coverage and never substitutes for final display proof.
  - On the Apple-Silicon capture host: `qemu-system-aarch64`, aarch64 UEFI
    pflash code and writable variable store, `qemu-img`, HVF, a screenshot
    capture tool, and OpenSSL. Run candidate capture only from a keyless capture
    account.
  - In a separate dedicated non-admin Aqua signing account: macOS `security`,
    `certtool`, Xcode Command Line Tools, OpenSSL, and the locked Authority 2
    identity described below. Never run candidate or Actions-runner code in
    this account.
  - at least 120 GiB free on both the repo filesystem and VM scratch filesystem before building release media; override `MIN_HOST_FREE_GB` only on runners with separately provisioned image/cache capacity.
  - `docker info` returns promptly before starting the build; restart Docker or free host resources if it hangs.
- Confirm repo at `$REPO_ROOT` and you are in that directory.
- Select one exact 40-hex source commit in `GOBLINS_OS_CANDIDATE_COMMIT`. Use
  that same value for the aarch64 artifact, capture, host signature, and signoff runs.
- Set the only supported architecture: `ARCH=aarch64`. The shell helpers accept
  `arm64` as an input alias, but persisted evidence always records `aarch64`.
- Choose the immutable pullable aarch64 release bootc image ref:
  `RELEASE_IMAGE=<registry>/<namespace>/goblins-os@sha256:<64-hex-digest>`. The Docker-local
  `localhost/goblins-os:$ARCH` handoff is only for artifact testing and cannot
  satisfy shipping proof.
- Run the fail-closed native Linux packaging preflight before starting the
  build. It does not create shipping artifacts or satisfy proof by itself:
  ```sh
  set -euo pipefail

  PREFLIGHT_ONLY=1 RUN_QEMU=0 GOBLINS_OS_SHIPPABLE_RELEASE=1 GOBLINS_OS_ARCH="$ARCH" \
    GOBLINS_OS_BIB_SOURCE_IMAGE="$RELEASE_IMAGE" \
    REPO_ROOT="$REPO_ROOT" os/hardware-gate/run-external-gate.sh
  ```
- Prepare the writable scratch VM disk on the Apple-Silicon capture host when
  the verification ISO is available; the capture harness can create it.

### Provisioning and rotating Display Proof Authority 2

Authority 2 is deliberately separate from candidate capture. Its private leaf
key exists only in a dedicated file Keychain owned by a dedicated non-admin
signing account. That Keychain is not the login/default Keychain, never appears
in the user search list, locks on sleep and after at most 300 seconds, and has no
trusted application or partition-list bypass. Signing and ACL changes require
an interactive Keychain passphrase prompt. Candidate code, CI, the capture
account, and the proof-reader runner must never see this Keychain.

The repository contains only four public files:

- `os/release/display-proof-authority2.pem`
- `os/release/display-proof-authority2.sha256`
- `os/release/display-proof-authority2-ca.pem`
- `os/release/display-proof-authority2-ca.sha256`

The leaf common name is exactly `Goblins OS Display Proof Authority 2`. It is a
non-CA signing leaf with `digitalSignature` key usage and `emailProtection`
extended key usage. The separately pinned offline CA is a CA certificate with
`keyCertSign`. The verifier rejects Authority 1, self-signed leaves, the wrong
chain, extra certificate uses, noncanonical PEM/fingerprints, and certificates
within 30 days of expiry.

Keep a signer-owned, read-only copy of the reviewed Authority 2 helper and its
matching `evidence_bundle.py` outside every candidate checkout. Record that
tooling commit and file hashes in the operator log. From that trusted copy,
print and follow the interactive checklist:

```sh
set -euo pipefail

AUTHORITY_TOOL_ROOT=/absolute/path/to/signer-owned-reviewed-tools
"$AUTHORITY_TOOL_ROOT/display-authority2.py" provision-checklist \
  --keychain "$HOME/Library/Keychains/GoblinsOS-Display-Authority-2.keychain-db" \
  --certificate /absolute/path/display-proof-authority2.pem \
  --certificate-sha256 /absolute/path/display-proof-authority2.sha256 \
  --ca-certificate /absolute/path/display-proof-authority2-ca.pem \
  --ca-certificate-sha256 /absolute/path/display-proof-authority2-ca.sha256
```

The checklist creates the dedicated Keychain through SecurityAgent, generates
the leaf key directly in that Keychain, signs the CSR with an encrypted
ephemeral CA on a RAM disk, publishes only the two public certificates and
fingerprints, removes prohibited key operations, audits the exact ACL graph,
and relocks. Never use `security import -A`, `security import -T`,
`set-key-partition-list`, `Always Allow`, a password argument, or a
password-bearing environment variable.

Run the helper's `harden` and `audit` commands after provisioning and before
each signing window. Commit the four reviewed public files atomically. The
candidate source verifier must remain blocked until all four files pass the
Authority 2 chain contract.

Capture and signing are two separate phases:

1. Run `run-capture.sh` from the keyless capture account. It creates and seals
   the complete 42-screen ARM64/HVF run, creates the unsigned Authority 2
   record, stops all candidate processes, and exits with status 75.
2. Transfer only `evidence-bundle.json`,
   `aarch64-local-display-attestation.json`, and the four public authority files
   to the signing account. Do not transfer executable candidate code.
3. From the signer-owned reviewed tool copy, run `display-authority2.py sign`.
   Retype the full candidate commit, image digest, run date, screenshot count,
   and seal hash; approve both SecurityAgent prompts. The helper audits before
   and after signing, verifies the CMS against the pinned leaf/CA, relocks, and
   only then publishes the detached signature.

   ```sh
   "$AUTHORITY_TOOL_ROOT/display-authority2.py" sign \
     --keychain "$HOME/Library/Keychains/GoblinsOS-Display-Authority-2.keychain-db" \
     --certificate /absolute/path/display-proof-authority2.pem \
     --certificate-sha256 /absolute/path/display-proof-authority2.sha256 \
     --ca-certificate /absolute/path/display-proof-authority2-ca.pem \
     --ca-certificate-sha256 /absolute/path/display-proof-authority2-ca.sha256 \
     --seal /absolute/path/evidence-bundle.json \
     --record /absolute/path/aarch64-local-display-attestation.json \
     --signature /absolute/path/aarch64-local-display-attestation.json.cms \
     --candidate-commit "$GOBLINS_OS_CANDIDATE_COMMIT" \
     --image-ref "$AARCH64_IMAGE_REF" \
     --run-date "$RUN_DATE" \
     --screenshot-count 42
   ```

4. Return only the detached CMS file to the capture account and finalize:

   ```sh
   GOBLINS_OS_CAPTURE_REQUIRE_COMPLETE=1 \
     os/hardware-gate/capture-harness/finalize-display-proof.sh \
       os/screenshots/hardware-gate/aarch64/YYYY-MM-DD \
       /absolute/path/aarch64-local-display-attestation.json.cms
   ```

The finalizer recomputes the evidence bundle, verifies the Authority 2 chain
and detached CMS twice, imports the signature without following links or
overwriting an existing file, and only then runs close-signoff. GitHub verifies
this proof but never mints or replaces it.

For rotation, stop capture and signing, provision a new dedicated identity and
offline CA, replace all four public files in one reviewed source commit, run the
Authority 2 self-test/evidence self-test/source verifier, and capture a fresh
ISO from that exact commit. Never re-sign old evidence with a new authority;
historical proof remains bound to the public chain in its own immutable source
commit.

### Rotating the immutable installer-branding tool

Rotate the tool whenever its Containerfile, base image, supported-architecture
policy, workflow semantics, or provenance schema changes. The tool must be
built natively on aarch64, reviewed, anonymously pullable, and digest-pinned
before any candidate ISO uses it.

The ARM-only schema-2 transition is deliberately a two-commit bootstrap. First
push commit A with the reviewed ARM-only workflow and the existing schema-1
record, then build the branding tool from A. Commit B must record A's exact
workflow run, attempt, digest, and inventory in schema 2 and propagate that
digest through every build path. Select B—not A—as the OS candidate. This
one-time rotation is required even though the Containerfile and Fedora base
image themselves did not change.

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
  -n "goblins-os-branding-tool-$TOOL_COMMIT-aarch64" \
  -D "$TOOL_TMP"
TOOL_INDEX="$TOOL_TMP/image-ref.json"
TOOL_INVENTORY="$TOOL_TMP/rpm-packages.tsv"
test -s "$TOOL_INDEX"
test ! -L "$TOOL_INDEX"
test -s "$TOOL_INVENTORY"
test ! -L "$TOOL_INVENTORY"
TOOL_REF="$(jq -er '.image_ref' "$TOOL_INDEX")"
jq -e \
  --arg architecture "aarch64" \
  --arg commit "$TOOL_COMMIT" \
  --arg run "$TOOL_RUN_URL" \
  --argjson attempt "$TOOL_RUN_ATTEMPT" \
  '.schema == "goblins-os-installer-branding-tool-v2"
   and .architecture == $architecture
   and .candidate_commit == $commit
   and .workflow_run == $run
   and .workflow_run_attempt == $attempt
   and (.image_ref | test("^ghcr\\.io/joe-simo/goblins-os-installer-branding-tool@sha256:[0-9a-f]{64}$"))
   and (.base_image | test("^docker\\.io/library/fedora@sha256:[0-9a-f]{64}$"))
   and (.containerfile_sha256 | test("^[0-9a-f]{64}$"))
   and (.rpm_inventory_sha256 | test("^[0-9a-f]{64}$"))
   and .anonymous_pull_verified == true' \
  "$TOOL_INDEX" >/dev/null
test "$(shasum -a 256 "$TOOL_INVENTORY" | awk '{print $1}')" = \
  "$(jq -er '.rpm_inventory_sha256' "$TOOL_INDEX")"
PUBLIC_DOCKER_CONFIG="$TOOL_TMP/public-docker"
mkdir -p "$PUBLIC_DOCKER_CONFIG"
DOCKER_CONFIG="$PUBLIC_DOCKER_CONFIG" docker manifest inspect "$TOOL_REF" \
  > "$TOOL_TMP/public-manifest.json"
jq -e \
  '([.manifests[]? | select(.platform.os == "linux" and .platform.architecture == "arm64")] | length) == 1
   and ([.manifests[]? | select(.platform.architecture == "amd64")] | length) == 0' \
  "$TOOL_TMP/public-manifest.json" >/dev/null
```

Review the full aarch64 RPM inventory and its licenses. Then update
`os/release/installer-branding-tool.toml` with the exact immutable image and
`architectures.aarch64.native_image_ref`, inventory hash/count, source commit,
workflow run and attempt, `anonymous_pull_verified = true`, base image,
Containerfile SHA256, and public-pull date.
Propagate that native digest through every
release workflow and `os/iso/build-iso.sh`, then run `goblins-os-verify`; its
semantic provenance check rejects Containerfile, base-image, architecture, or
pin drift. Never substitute a tag for the reviewed digest.

### Canonical exact-candidate build

Build the native aarch64 release through the single non-promotional candidate
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

Download the metadata-only artifact, not the multi-gigabyte ISO artifact, to
obtain the immutable aarch64 image reference. Validate its architecture, commit,
and non-promotional marker before using the digest:

```sh
set -euo pipefail

CANDIDATE_METADATA_DIR="$(mktemp -d "${TMPDIR:-/tmp}/goblins-os-candidate-ref.XXXXXX")"
ARCH=aarch64
ARCH_METADATA_DIR="$CANDIDATE_METADATA_DIR/$ARCH"
gh run download "$CANDIDATE_RUN_ID" \
  -n "goblins-os-candidate-ref-$GOBLINS_OS_CANDIDATE_COMMIT-$ARCH" \
  -D "$ARCH_METADATA_DIR"
REF_JSON_COUNT="$(find "$ARCH_METADATA_DIR" -type f -name image-ref.json -print | wc -l | tr -d '[:space:]')"
test "$REF_JSON_COUNT" = 1
AARCH64_REF_JSON="$(find "$ARCH_METADATA_DIR" -type f -name image-ref.json -print)"
jq -e \
  --arg commit "$GOBLINS_OS_CANDIDATE_COMMIT" \
  --arg run "$CANDIDATE_RUN_URL" \
  '.schema == "goblins-os-candidate-image-ref-v3"
   and .architecture == "aarch64"
   and .candidate_commit == $commit
   and .oci_revision == $commit
   and .candidate_tag_authoritative == false
   and .non_promotional == true
   and .installer_config == "os/iso/config.toml"
   and (.rpm_command_sha256 | test("^[0-9a-f]{64}$"))
   and .source_repository == "https://github.com/Joe-Simo/goblins-os"
   and .workflow_name == "candidate-artifacts"
   and .workflow_run == $run
   and ((.workflow_run_attempt | type) == "number")
   and .workflow_run_attempt >= 1
   and .exact_candidate_gates.source_verifier == "pass"
   and .exact_candidate_gates.installed_root_verifier == "pass"
   and .exact_candidate_gates.services_selftest == "pass"
   and (.immutable_image_ref | test("^ghcr\\.io/joe-simo/goblins-os@sha256:[0-9a-f]{64}$"))' \
  "$AARCH64_REF_JSON" >/dev/null

CANDIDATE_RUN_ATTEMPT="$(jq -er '.workflow_run_attempt' "$AARCH64_REF_JSON")"
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
AARCH64_IMAGE_REF="$(jq -er '.immutable_image_ref' "$AARCH64_REF_JSON")"
```

The commit-scoped image tag is only a build locator and can move on a rebuild.
The `immutable_image_ref` digest is the release-proof identity.

`gh workflow run --ref` selects a branch or tag, so the dispatch below uses
`main`; the immutable commit remains a separate required input. The command
rejects the run unless its recorded `head_sha` is that exact commit. If `main`
moves between selection and dispatch, stop and select a new candidate.

### aarch64 Apple Silicon/HVF capture route

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
  '.schema == "goblins-os-release-evidence-v5"
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
set +e

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
CAPTURE_RC=$?
set -e
test "$CAPTURE_RC" -eq 75
```

The local capture writes `evidence-bundle.json` only after all 42 required PNGs,
including `05-first-boot-private-unlock.png`, every required proof JSON, and the
three copied verification manifests have been produced. The seal records each
file's exact SHA-256 and byte size, records each PNG's dimensions, and rejects a
run unless every PNG has the same realistic framebuffer dimensions. Symlinks,
non-regular files, path escapes, duplicate paths, duplicate JSON keys, and
non-canonical seal encoding are rejected.

After candidate QEMU and the capture receiver have stopped, the harness creates
the evidence seal and then runs Apple Vision locally across the exact 42-image
inventory before it creates the Authority 2 record. The scanner reads each PNG
once through a no-follow file descriptor, requires the canonical 5120x2880
framebuffer, and matches its byte size and SHA-256 to the seal before OCR. It
uses a pinned Vision revision over overlapping bounded tiles and a whole-frame
downsample. The scan fails closed if an image is missing, malformed, unreadable
by Vision, has no recognized text, differs from the seal, or visibly contains a
credential pattern. It reports only the affected filename and credential class,
never recognized text or a secret. Stable promotion repeats the same sealed-byte
scan over the signed screenshots before it creates public release assets.

The protected `stable` GitHub environment and package policy are a required
single-writer boundary: no user, token, workflow, app, or deployment outside
`.github/workflows/stable-promotion.yml` may mutate the `:aarch64` or `:stable`
GHCR tags or publish stable GitHub releases. The workflow serializes its own
promotions and compares channel digests immediately before mutation, but a
registry tag update has no portable compare-and-swap operation; concurrency is
therefore not a substitute for revoking every other package writer.

`run-capture.sh` creates the unsigned
`aarch64-local-display-attestation.json`, stops every candidate process, and
exits with status 75. The isolated signing account then creates the detached
`aarch64-local-display-attestation.json.cms`; the keyless capture account
imports it only through `finalize-display-proof.sh`. The signed canonical
record directly binds the Authority 2 generation and chain, candidate commit,
digest-pinned image, verification ISO SHA256, evidence-bundle SHA256,
Darwin/arm64/HVF/QEMU facts, and the digest of the complete 42-image screenshot
manifest. Verify the finalized proof locally before closing signoff:

```sh
set -euo pipefail

AARCH64_SCREENSHOT_RUN_DIR="os/screenshots/hardware-gate/aarch64/$RUN_DATE"
AARCH64_EVIDENCE_SEAL="$AARCH64_SCREENSHOT_RUN_DIR/evidence-bundle.json"
AARCH64_AUTHORITY_RECORD="$AARCH64_SCREENSHOT_RUN_DIR/aarch64-local-display-attestation.json"
AARCH64_AUTHORITY_SIGNATURE="$AARCH64_AUTHORITY_RECORD.cms"
AARCH64_EVIDENCE_SHA="$(python3 os/hardware-gate/capture-harness/evidence_bundle.py inspect \
  --seal "$AARCH64_EVIDENCE_SEAL" \
  --architecture aarch64 \
  --candidate-commit "$GOBLINS_OS_CANDIDATE_COMMIT" \
  --image-ref "$AARCH64_IMAGE_REF" \
  --run-date "$RUN_DATE")"
python3 os/hardware-gate/capture-harness/evidence_bundle.py verify-attestation \
  --seal "$AARCH64_EVIDENCE_SEAL" \
  --record "$AARCH64_AUTHORITY_RECORD" \
  --signature "$AARCH64_AUTHORITY_SIGNATURE" \
  --certificate os/release/display-proof-authority2.pem \
  --certificate-sha256 os/release/display-proof-authority2.sha256 \
  --ca-certificate os/release/display-proof-authority2-ca.pem \
  --ca-certificate-sha256 os/release/display-proof-authority2-ca.sha256 \
  --candidate-commit "$GOBLINS_OS_CANDIDATE_COMMIT" \
  --image-ref "$AARCH64_IMAGE_REF" \
  --run-date "$RUN_DATE"
```

GitHub is optional verification and artifact replication only. It receives the
already signed seal, record, and signature; it has `contents: read`, no OIDC or
attestation-writing permission, and no private key. A dispatcher can submit
arbitrary bytes, but the workflow cannot accept or upload them unless the CMS
signature verifies against the Authority 2 leaf and CA pinned by the exact
candidate commit:

```sh
set -euo pipefail

AARCH64_EVIDENCE_BASE64="$(base64 < "$AARCH64_EVIDENCE_SEAL" | tr -d '\n')"
AARCH64_AUTHORITY_RECORD_BASE64="$(base64 < "$AARCH64_AUTHORITY_RECORD" | tr -d '\n')"
AARCH64_AUTHORITY_SIGNATURE_BASE64="$(base64 < "$AARCH64_AUTHORITY_SIGNATURE" | tr -d '\n')"

AARCH64_VERIFICATION_RUN_URL="$(gh workflow run aarch64-local-display-attestation.yml \
  --ref main \
  -f candidate_commit="$GOBLINS_OS_CANDIDATE_COMMIT" \
  -f candidate_image_ref="$AARCH64_IMAGE_REF" \
  -f run_date="$RUN_DATE" \
  -f evidence_bundle_sha256="$AARCH64_EVIDENCE_SHA" \
  -f evidence_bundle_base64="$AARCH64_EVIDENCE_BASE64" \
  -f authority_record_base64="$AARCH64_AUTHORITY_RECORD_BASE64" \
  -f authority_signature_base64="$AARCH64_AUTHORITY_SIGNATURE_BASE64")"
[[ "$AARCH64_VERIFICATION_RUN_URL" =~ /actions/runs/[0-9]+$ ]]
AARCH64_VERIFICATION_RUN_ID="${AARCH64_VERIFICATION_RUN_URL##*/}"
gh run watch "$AARCH64_VERIFICATION_RUN_ID" --exit-status
```

The GitHub run is not display authority and is not required by
`close-signoff.sh`; the pinned Authority 2 signature is. Rerun
`close-signoff.sh` with the same exact candidate/image/native-gate variables and
`REQUIRE_COMPLETE=1`. Final shipping verification independently recomputes the
seal and verifies the local record and CMS signature, so deleting, replacing,
or editing any screenshot or proof after signing invalidates the run.

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

Now hydrate the aarch64 public-media artifact from the exact successful
candidate run. The hydration command verifies the GitHub source repository,
workflow run and attempt, candidate commit, immutable image digest, human-safe
installer config, exact-candidate gates, checksum, manifests, and SBOM before it
atomically replaces individual canonical files. It never treats a historical
alpha as current proof:

```sh
set -euo pipefail

GOBLINS_OS_HYDRATION_MODE=exact-candidate \
GOBLINS_OS_CANDIDATE_COMMIT="$GOBLINS_OS_CANDIDATE_COMMIT" \
GOBLINS_OS_CANDIDATE_IMAGE_REF="$AARCH64_IMAGE_REF" \
GOBLINS_OS_CANDIDATE_WORKFLOW_RUN="$CANDIDATE_RUN_URL" \
GOBLINS_OS_CANDIDATE_WORKFLOW_RUN_ATTEMPT="$CANDIDATE_RUN_ATTEMPT" \
  os/release/hydrate-release-artifacts.sh
```

The exact-candidate mode always downloads the complete public ISO artifact; do
not set `GOBLINS_OS_DOWNLOAD_ISO`. Historical alpha assets require the separate
explicit `historical-alpha` mode and are written only below
`os/release/historical-alpha/<tag>/`, which can never satisfy current signoff.

Compose the complete aarch64 row explicitly, then run the final
gate against the hydrated public release media:

```sh
set -euo pipefail

AARCH64_SIGNOFF_ROW="$REPO_ROOT/os/screenshots/hardware-gate/aarch64/$RUN_DATE/signoff-row.md"
test -s "$AARCH64_SIGNOFF_ROW"
REPO_ROOT="$REPO_ROOT" \
GOBLINS_OS_CANDIDATE_COMMIT="$GOBLINS_OS_CANDIDATE_COMMIT" \
  bash os/hardware-gate/compose-signoff-rows.sh \
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
The display route still requires `qemu-system-aarch64`, UEFI firmware, and HVF
on the Apple Silicon Darwin/arm64 host, plus enough free space for the VM
scratch disk and proof output. The capture harness defaults to an
80G sparse scratch disk; set `GOBLINS_OS_CAPTURE_DISK_SIZE` only when the host
has a separately validated disk-size requirement. The harness boots the
verification ISO only for the install pass and then prefers the installed VM
disk after Anaconda reboots. The aarch64 route uses a two-phase capture because
QEMU aarch64 does not support the same boot-order override. The install ISO is presented as USB
storage so the scratch disk remains virtio vda for the verification kickstart.
Use `GOBLINS_OS_CAPTURE_ISO` and `GOBLINS_OS_CAPTURE_ISO_SHA256` only when the
verification ISO is stored outside the default output path. It does not replace
the GHCR/package visibility check
or the release artifact/SBOM build.

## Optional native Linux packaging rebuild (not display proof)
```sh
set -euo pipefail

cd "$REPO_ROOT"
ARCH=aarch64
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
Git. The `aarch64-verification-iso` workflow consumes that digest directly and
only uploads short-lived artifacts. The local-display
attestation workflow only re-verifies and uploads bytes already signed by the
approved capture host; it cannot mint display authority. None can write
repository contents. Download and review the aarch64 output in a disposable
exact-candidate checkout before attaching the proof to the release.

## Optional Linux/KVM boot diagnostic (never signoff)

Linux/KVM may be used only through the optional diagnostic route documented by
`os/hardware-gate/run-external-gate.sh`. It can test native packaging and boot
behavior, but its output is non-authoritative: never capture final screenshots,
generate a signoff row, or mark display proof complete from KVM.
KVM can never satisfy the display signoff gate.

The only canonical display capture and signoff route is the Apple Silicon
Darwin/arm64/HVF harness in [aarch64 Apple Silicon/HVF capture route](#aarch64-apple-siliconhvf-capture-route).
It must use `qemu-system-aarch64` with
`virt,accel=hvf,gic-version=max` and the exact verification-only ISO.

## Reference: proof asset contract generated by the HVF harness

The Darwin/arm64/HVF capture harness saves the live session proof under:

`os/screenshots/hardware-gate/aarch64/<YYYY-MM-DD>/`

Legacy/non-shipping screenshot roots that are not under
`os/screenshots/hardware-gate/aarch64/<YYYY-MM-DD>/` are migration history only.
Do not copy, rename, or re-date them into an architecture root. Run the current
verification ISO through the Darwin/arm64/HVF capture harness; it captures fresh
screenshots and generates a new `proof-manifest.json` tied to that ISO and SHA.

Add `proof-manifest.json` beside the screenshots so the proof root is tied to
the release media that was booted:

```json
{
  "architecture": "aarch64",
  "candidate_commit": "<same selected 40-hex source commit>",
  "image_ref": "<registry>/<namespace>/goblins-os@sha256:<64-hex-digest>",
  "iso": "os/iso/output/aarch64/bootiso/goblins-os-aarch64.iso",
  "iso_sha256": "<64-char sha256 from the matching .sha256 file>",
  "captured_at": "<UTC timestamp>",
  "screenshot_run_dir": "os/screenshots/hardware-gate/aarch64/<YYYY-MM-DD>",
  "capture_workflow_run": "",
  "capture_workflow_run_attempt": 0,
  "native_packaging_gate_proof": "os/screenshots/hardware-gate/aarch64/<YYYY-MM-DD>/native-packaging-gate.json",
  "native_packaging_gate_run": "https://github.com/Joe-Simo/goblins-os/actions/runs/<run-id>",
  "native_packaging_gate_run_attempt": 1,
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
  "runtime_build_proof": "runtime-build-proof.json",
  "accessibility_adaptivity_proof": "accessibility-adaptivity-proof.json"
}
```

`close-signoff.sh` fully decodes every screenshot and rejects missing, empty,
oversized, symlinked, multi-frame, or invalid PNG files. It also requires the
screenshot 32 SHA-256 in its live proof and manifest to equal the actual decoded
file, recomputes the canonical `evidence-bundle.json` covering all 42 uniform
framebuffer PNGs and every required JSON, and rejects a manifest that does not
match the current architecture ISO and SHA. It then verifies the detached CMS
signature with the public certificate and fingerprint pinned by the exact
candidate checkout; the signed record must repeat that ISO SHA256 and the
digest of the complete 42-screenshot manifest. Self-reported Darwin/HVF text or
a GitHub workflow result without this host signature is insufficient. It
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
`PROOF_PATH=os/screenshots/hardware-gate/aarch64/<date>/runtime-build-proof.json`
and `BUILD_RESPONSE_PATH=os/screenshots/hardware-gate/aarch64/<date>/build-response.json`.
Do not hand-write this file; the proof must be produced from the live
`/v1/apps/builds` response.

The accessibility/adaptivity gate is `accessibility-adaptivity-proof.json`,
linked from `proof-manifest.json` as `accessibility_adaptivity_proof`. Its eight
display-backed states are screenshots 33–40. The installed helper changes text
scale, high contrast, reduced transparency, reduced motion, and screen-reader
state only through the protected accessibility service, then requires both core
status and the corresponding real GSettings readback. It launches the real
Goblins Settings Language & Region panel first in the baseline locale and then
with `LANG=de_DE.UTF-8 LANGUAGE=de`. Locale expansion passes only when the real
Goblins surface reports German (Germany), its current date and number formats
come from the active German system locale, at least one changed production
accessible name is longer than its baseline, and every visible named AT-SPI
node remains inside the window bounds. Goblins-owned interface copy stays in
English and the proof records that no German translation is claimed. GNOME
Control Center translations are not accepted as proxy evidence. It requires a live
Orca process and owned `org.a11y.Bus`, walks the real AT-SPI tree, sends Tab via
the QMP keyboard, confirms the actual focused accessible, and invokes the real
AT-SPI `Zoom` action before proving the settings window dimensions shrank.
Every screenshot name and SHA-256 must equal its host capture acknowledgement,
the framebuffer must remain 5120×2880, and all original preferences must be
restored before `status=pass`. Synthetic labels, fabricated accessibility
nodes, mocked focus, hand-written JSON, and inferred resize state never count.

The Darwin/arm64/HVF harness must capture exactly these names:
1. `01-installer.png` — ISO boot + installer launch
2. `02-install-network.png` — installer network/progress
3. `03-login.png` — login screen
4. `04-desktop.png` — first native desktop session
5. `05-first-boot-private-unlock.png` — first-boot protected-service unlock
6. `06-onboarding.png` — first-boot onboarding page
7. `07-home.png` — post-onboarding home
8. `08-shell-home.png` — shell launch
9. `09-shell-dark.png` — shell dark-theme state
10. `10-settings.png` — settings page
11. `11-settings-models.png` — settings models section
12. `12-settings-dark.png` — settings dark-theme state
13. `13-studio-before.png` — Build Studio prompt
14. `14-studio-running.png` — studio running
15. `15-studio-app-detail.png` — built-app detail
16. `16-built-app-open.png` — open built app
17. `17-dark-motion.png` — dark-theme motion/interactions
18. `18-light-motion.png` — light-theme motion/interactions
19. `19-vulkan-vkcube.png` — native Vulkan sample running in the installed session
20. `20-gamemode-active.png` — GameMode activation command result
21. `21-gamescope-session.png` — Gamescope-launched nested session or app
22. `22-mangohud-overlay.png` — MangoHud overlay visible over a user-launched sample
23. `23-controller-detection.png` — connected controller/gamepad detected by the OS
24. `24-audio-output.png` — PipeWire audio sink/output proof while a test sound is playing
25. `25-install-destination.png` — advanced storage Installation Destination showing explicit disk choice
26. `26-install-storage-summary.png` — storage summary showing formatting/root filesystem before writing changes
27. `27-dual-boot-preserve-existing-os.png` — the native installer's Open advanced storage path or the desktop Install Goblins OS Beside Another OS entry, followed by Custom/manual storage or Reclaim Space showing Goblins OS installed into unallocated free space or a dedicated disk while existing OS, APFS/data, recovery, vendor, and EFI partitions are preserved; APFS is a preserve-only signal, not an Apple bare-metal support claim
28. `28-bootloader-efi-summary.png` — bootloader/EFI target summary before beginning install
29. `29-preview-pdf-open.png` — Papers showing the installed Preview proof PDF opened through `/v1/preview/open`
30. `30-preview-image-open.png` — Loupe showing the installed Preview proof PNG opened through `/v1/preview/open`
31. `31-text-shortcuts-candidate-bubble-render.png` — diagnostic candidate render surface
32. `32-text-shortcuts-live-ibus-runtime-render.png` — live native IBus candidate at capture time
33. `33-accessibility-text-scaling.png` — 1.25 text scale with protected-core and GSettings readback
34. `34-accessibility-high-contrast.png` — high contrast enabled and read back
35. `35-accessibility-reduced-transparency.png` — Goblins reduced-transparency setting enabled and read back
36. `36-accessibility-reduced-motion.png` — animations disabled and reduced motion read back
37. `37-accessibility-localization-expansion.png` — real Goblins Language & Region view using German (Germany) regional formats, English interface copy, and unclipped expanded accessibility content
38. `38-accessibility-orca-atspi.png` — live Orca process, accessibility bus, and settings tree
39. `39-accessibility-keyboard-focus.png` — QMP Tab input with real AT-SPI focused accessible
40. `40-accessibility-window-resize.png` — real AT-SPI Zoom action with smaller observed window dimensions
41. `41-hosted-context-review.png` — installed hosted-context review surface in light appearance, visibly marked as decision-incapable visual proof
42. `42-hosted-context-review-dark.png` — the same installed decision-incapable review surface in dark appearance

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
- release evidence path under `os/signoff-proofs/sbom/aarch64/`
- each check pass/fail and screenshot filenames
- canonical `evidence-bundle.json` SHA-256
- for local aarch64/HVF, the capture-host authority record, detached CMS
  signature, pinned leaf certificate SHA256, pinned CA certificate SHA256,
  signed verification-ISO SHA256, and signed complete-screenshot-manifest SHA256
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
- if the native installer is used, show that the simple flow proceeds only for a blank disk and routes disks with existing OS/APFS/data/recovery/vendor/EFI partitions to manual storage
- blockers
- verify every required file above exists before marking the run complete

Validate and stage this proof set only with the complete `close-signoff.sh`
command in the canonical Apple Silicon/HVF route above. It requires the exact
native packaging gate, local display attestation, runtime proof, and
`SIGNOFF_ROW_OUTPUT`; it does not mutate `os/signoff-notes.md`.

The helper may generate source-only evidence as a diagnostic, but final release
evidence must come from the packaged `goblins-os-verify --release-evidence`
inside the exact digest-pinned aarch64 image. Replaying
`rpm-packages.command` by itself does not satisfy final release evidence. The
accepted set must contain a v5 `release-evidence-manifest.json`,
`cargo-lock-packages.tsv`, `rpm-packages.command`, and `rpm-packages.tsv` from
that packaged-verifier invocation, with all three SHA256 values matching the
manifest. The
manifest must also record `asset_provenance`, `third_party_notices`,
`trademark_posture`, and `source_tree_manifest` paths so release reviewers can
trace the aarch64 artifact back to the source-package diligence files.
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

The HVF proof and staged row record the exact engine and result. Do not edit
[os/signoff-notes.md](os/signoff-notes.md) directly; the atomic composer adds
the row only after public-media and full shipping validation pass.

## 5) Closed-loop verification on host image artifacts
Use this quick evidence audit first:

```sh
set -euo pipefail

GOBLINS_OS_CANDIDATE_COMMIT="$GOBLINS_OS_CANDIDATE_COMMIT" \
  ./os/hardware-gate/verify-shipping-status.sh
```

Running `close-signoff.sh` without the canonical HVF proof variables is only an
incomplete diagnostic. It reports an incomplete result and does not append to
`os/signoff-notes.md`; complete proof must be written to an explicit
`SIGNOFF_ROW_OUTPUT` and later composed atomically. The diagnostic reports:
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
