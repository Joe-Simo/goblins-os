# Goblins OS Publisher Boundary

Goblins OS source and publication use two separate trust domains.

- `Joe-Simo/goblins-os` is the public source and proof repository. Its workflows
  have read-only repository permissions, consume no repository/environment
  secrets, never authenticate to GHCR, and cannot create tags or Releases.
- `Joe-Simo/goblins-os-publisher` is the only publication repository. Its
  protected workflows import already-verified OCI bytes, build the final
  digest-bound installer media, and perform GHCR/tag/Release writes.

The source repository ships no provider key and this boundary never needs an
OpenAI credential. Publication credentials belong only to the protected
publisher and must never be copied into this repository, its artifacts, its
logs, or an OS image.

## Source artifact contract

`candidate-artifacts.yml` builds the exact current `main` commit on native
`aarch64` Linux. It exports one OCI archive, verifies the manifest/config,
loads the archive locally, passes source and installed-root verification, runs
the service self-test, and generates the release-evidence inventory. It then
splits the archive into four ordered byte ranges.

The run produces these immutable Actions artifacts:

```text
goblins-os-source-oci-<commit>-aarch64-attempt-<attempt>-part-00
goblins-os-source-oci-<commit>-aarch64-attempt-<attempt>-part-01
goblins-os-source-oci-<commit>-aarch64-attempt-<attempt>-part-02
goblins-os-source-oci-<commit>-aarch64-attempt-<attempt>-part-03
goblins-os-source-oci-<commit>-aarch64-attempt-<attempt>-metadata
```

The metadata artifact contains:

- `handoff.json` using schema `goblins-os-source-oci-handoff-v1`;
- `publisher-envelope.json` using schema
  `goblins-os-actions-artifact-envelope-v1`;
- `SHA256SUMS` for the four raw parts; and
- the architecture-bound release-evidence manifest and Cargo/RPM inventories.

The handoff binds the source commit, native architecture, OCI top-level digest,
intended GHCR digest reference, full archive hash and size, each part hash and
size, source run/attempt, and verification results. The envelope additionally
binds the immutable GitHub artifact ID, URL, and SHA-256 digest returned for
each uploaded part. The metadata artifact has its own GitHub artifact digest,
available from the run API. None of these records claims the image is already
published.

`branding-tool-image.yml` applies the same contract to the native installer
branding tool:

```text
goblins-os-branding-tool-oci-<commit>-aarch64-attempt-<attempt>-part-00
goblins-os-branding-tool-oci-<commit>-aarch64-attempt-<attempt>-part-01
goblins-os-branding-tool-oci-<commit>-aarch64-attempt-<attempt>-part-02
goblins-os-branding-tool-oci-<commit>-aarch64-attempt-<attempt>-part-03
goblins-os-branding-tool-oci-<commit>-aarch64-attempt-<attempt>-metadata
```

Its metadata uses schema
`goblins-os-installer-branding-tool-handoff-v1` and also binds the Fedora base
digest, Containerfile digest, complete RPM inventory hash/count, required-tool
check, and intended branding-tool GHCR digest reference.

The four-part layout is bounded to a 32 GiB source archive, so no part can
exceed 8 GiB. Parts use `compression-level: 0`; OCI layers are already
compressed and must be reassembled byte-for-byte in suffix order.
Every artifact name includes the authenticated workflow run attempt. A rerun
therefore produces a new, non-colliding five-artifact set, and the publisher
must select only the set whose attempt equals the requested run attempt.

## Publisher import contract

The publisher must independently perform every check below. A successful
source run or artifact name is not sufficient by itself.

1. Authenticate the exact source run ID and attempt through the GitHub API.
   Require the expected workflow path, `workflow_dispatch`, successful
   conclusion, and exact candidate `head_sha`.
2. Fetch the metadata artifact and compare its downloaded archive SHA-256 to
   the digest returned by the GitHub Actions artifact API.
3. Parse JSON with duplicate-key rejection. Require the exact schema, keyset,
   `aarch64`/`arm64` architecture, candidate commit, source run/attempt,
   non-promotional marker, and publisher repository identity.
4. Fetch each payload artifact by the exact ID in the envelope. Compare the
   downloaded Actions archive SHA-256 to its authenticated API digest, extract
   exactly one bounded regular part, and reject links, duplicate names, path
   escapes, and unexpected members.
5. Match every raw part to `SHA256SUMS`, concatenate parts `00` through `03`,
   and match the complete archive size and SHA-256 in `handoff.json`.
6. Inspect the OCI archive before any registry write. Require one Linux ARM64
   image, no amd64 child, the exact revision label, the ARM-only label for the
   OS image, and the exact top-level manifest digest.
7. Copy with digest preservation. After import, fetch the public registry
   manifest and require its SHA-256 to equal the handoff digest. A changed
   digest is a hard failure; do not rewrite evidence to fit it.
8. Run the installed-root verifier and service self-test again from the imported
   digest before treating the image as a candidate.

The branding-tool image is imported first. Its resulting immutable digest must
match the source handoff and the protected-publisher evidence used by the ISO
build.
The OS image is then imported under a commit-scoped candidate locator. Neither
operation moves `:aarch64`, `:stable`, or another public release channel.

### Branding publisher evidence

After importing and publicly reading back the branding image, the protected
`publish-branding-tool-aarch64.yml` workflow emits exactly one artifact named
`goblins-os-branding-tool-publisher-evidence-<source-commit>-aarch64`. It contains
exactly one file, `installer-branding-publisher-evidence.json`, using schema
`goblins-os-installer-branding-tool-publisher-evidence-v1`.

The JSON has exact top-level objects for `source_workflow`, `source_handoff`,
`publisher`, `published_image`, and `verification`. It binds:

- the source workflow path, run ID/attempt, source commit, four ordered payload
  artifact IDs/digests/sizes, and metadata artifact ID/digest/size;
- the handoff and envelope schemas and SHA-256 values, `SHA256SUMS` and RPM
  inventory hashes, package count, complete OCI archive hash/size, OCI manifest
  digest, intended immutable ref, Fedora base digest, and Containerfile digest;
- the publisher workflow path and commit, run ID/attempt, protected `candidate`
  environment, and native `aarch64` runner;
- the published immutable ref and manifest digest, Linux/ARM64 identity, source
  revision, inventory and build-input hashes, digest preservation, and public
  read-back; and
- explicit successful checks for source-run authentication, metadata and all
  four payload artifact digests, ordered-part reconstruction, OCI inspection,
  required tools, and the public manifest.

The evidence remains non-promotional and records that the source repository has
no publication authority. It contains no credential and is at most 32 KiB, a
deliberately conservative decoded bound that leaves room for base64 expansion
and the other required `workflow_dispatch` inputs.
Its containing artifact's identity is deliberately not embedded in the JSON,
because an artifact cannot truthfully contain its own post-upload digest. The
final shipping gate instead derives the exact artifact name from the source
commit, resolves exactly one unexpired artifact on the authenticated publisher
run through the GitHub API, verifies the downloaded ZIP against its API size and
SHA-256 digest, and requires its sole JSON member to match the ISO-bound evidence
byte for byte. `installer-branding-tool.toml` remains schema-1
bootstrap/diagnostic history only: it may support local diagnostics, but it can
never authorize a verification ISO, shippable ISO, or stable promotion.

Release-mode `os/iso/build-iso.sh` requires both the evidence file through
`GOBLINS_OS_INSTALLER_BRANDING_PUBLISHER_EVIDENCE` and its exact published ref
through `GOBLINS_OS_INSTALLER_BRANDING_IMAGE`. It validates the evidence before
host/runtime setup, copies the verified bytes beside the ISO manifest, and
binds their SHA-256 plus source and publisher run identities into
`goblins-os-iso-build-manifest-v2`. The final shipping gate authenticates both
workflow runs, all five source artifacts, the publisher artifact identity and
raw ZIP digest, the downloaded source metadata and publisher evidence bytes,
and the manifest binding.

## Stable handoff contract

After the digest-pinned candidate has completed the native Linux packaging gate
and the Authority 2 Apple Silicon/HVF display proof,
`stable-promotion.yml` creates one `goblins-os-publisher-request-v1` artifact.
It authenticates and binds the exact candidate, branding-tool, and signed
display-verification run/attempt plus every required Actions artifact digest.
It does not trigger the publisher and has no publication credential.
The branding-tool source commit is an explicit independent input because a
reviewed tool rotation may correctly predate the selected OS candidate commit.

The protected publisher consumes that request and must:

1. revalidate every source artifact and OCI byte as described above;
2. fetch the official `zstd-1.5.7.tar.gz` source archive only from
   `https://github.com/facebook/zstd/releases/download/v1.5.7/zstd-1.5.7.tar.gz`,
   require SHA-256
   `eb33e51f49a15e023950cd7825ca74a4a2b43db8354825ac24fc1b7ee09e6fa3`,
   build its native ARM64 CLI inside the protected job, and require the exact
   `*** Zstandard CLI (64-bit) v1.5.7, by Yann Collet ***` version string. The
   release verifier reconstructs the raw USTAR bytes and exact zstd bytes, so
   ignored USTAR padding and zstd skippable frames are hard failures;
3. build the shippable ISO on native aarch64 Linux from the public,
   digest-pinned candidate and branding-tool references;
4. verify ISO, BIB, SBOM, screenshot, Authority 2, signoff, and shipping-status
   evidence all bind the same commit, image digest, architecture, and media;
5. secret-scan the exact allowlisted release bytes;
6. create the final tag and draft Release through the publisher's narrowly
   scoped GitHub App identity;
7. upload and re-download every allowlisted asset, verifying size and SHA-256;
8. move `:aarch64` and `:stable` only after all immutable evidence is green;
9. publish the draft Release last, then verify the public package and release
   identities without credentials.

No source workflow can substitute for these publisher checks or claim stable
completion from a handoff artifact.

## One-time external bootstrap

The boundary is enforceable only after these owner-admin actions are complete:

1. Create `Joe-Simo/goblins-os-publisher`. Protect its default branch with a
   ruleset requiring pull requests, review, status checks, and no bypass for
   ordinary maintainers.
2. Add protected `candidate` and `stable` environments. Require reviewers,
   prevent self-review, restrict deployment branches/tags, and permit only the
   pinned publisher workflows.
3. In both GHCR packages, disable inherited access from
   `Joe-Simo/goblins-os`; remove source-repository write/admin Actions access;
   grant the publisher repository write access; retain only public anonymous
   read access where intended.
4. Set the source repository's default Actions token to read-only and protect
   `.github/workflows/**` plus this contract through required review/status
   checks. `os/release/verify-publisher-boundary.sh` must be required.
5. Create a dedicated GitHub App installed only where needed. Give it Actions
   read access to source artifacts and the minimum Contents permission needed
   to create source tags/Releases. Store its app identity and private key only
   in the publisher's protected environment. Branch rules must prevent it from
   writing `main` directly.
6. Implement pinned `publish-branding-tool-aarch64.yml` and
   `publish-aarch64.yml` workflows in the publisher. They must use native ARM64
   runners, the verification sequence above, per-target concurrency, and no
   candidate-controlled scripts after entering the write-authorized stage.
7. Test the boundary with a disposable candidate: prove source workflow tokens
   cannot push either package or create a source tag/Release, then prove the
   approved publisher can import only the exact sealed bytes.

Until all seven steps are completed and evidenced, the source artifacts are
verified handoffs only. They are not shippable media and do not satisfy stable
promotion.
