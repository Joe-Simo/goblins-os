# Goblins OS Go-Live Checklist

This checklist tracks the live alpha release and the remaining steps before a
stable public release.

## Public Release Surface

- [x] Source repository is public.
- [x] Website is live at <https://goblinsos.com>.
- [x] GitHub release exists for the current alpha.
- [x] Split ISO downloads are hosted on GitHub release assets, not the website
  host.
- [x] SHA256 files are published for the split download parts, compressed ISO,
  and final ISO.
- [x] Website includes install and checksum verification guidance.
- [x] GHCR package visibility allows anonymous Docker and Podman pulls.
  Current check: GitHub Packages reports `goblins-os` visibility as `public`,
  and anonymous registry manifest requests for
  `ghcr.io/joe-simo/goblins-os:x86_64` and `:aarch64` return `200`.

## Published Alpha Verification

- [x] Release workflow builds architecture-specific media for `x86_64` and
  `aarch64`.
- [x] Release artifacts include manifests and package evidence.
- [x] Published release metadata/SBOM can be hydrated into the local gate layout
  without downloading multi-gigabyte ISO media by default:
  `os/release/hydrate-release-artifacts.sh`.
- [x] Full ISO release media can be hydrated from split GitHub release assets
  with `GOBLINS_OS_DOWNLOAD_ISO=1`, verified part-by-part, decompressed, and
  verified against the final ISO SHA256.
- [x] Source and generated artifact scans check for live secrets.
- [x] `x86_64` display-backed verification-ISO screenshot/runtime run is complete.
  Current proof: GitHub Actions run `28721788279` captured
  `os/screenshots/hardware-gate/x86_64/2026-07-04` from the verification ISO
  built from `ghcr.io/joe-simo/goblins-os:x86_64`; the proof manifest records
  ISO SHA256 `10d72f00b43d39411cb193154e51b8e8c98f142abcf1246fd87e7f4456046683`.
- [x] `x86_64` public release ISO artifacts are checked separately from
  automated screenshots.
  Current check: the completed x86_64 screenshot proof uses verification-only
  media because public release media intentionally leaves storage interactive.
  The hydrated public release ISO SHA
  `45abf064735fa2a2ba9ef034883d19453c4bfc02a3b0c311d29e3679c52db434` is
  checksum-verified by the release artifact gate instead of being used for
  automated capture.
- [ ] Reconcile the `aarch64` display-backed verification-ISO proof with its
  signoff row. The proof manifest records ISO SHA256
  `3c73a77335b8be7b1fdaeb73e7992bacf6ec253cb0755f030484a673b0c293dc`,
  while the latest `aarch64` signoff row records
  `539fe24454f5cf1b0bb3ac00c9b8a838614ada85a310511fb9605afa978686a7`.
  Treat the run as incomplete until one recapture and signoff identify the same
  verification ISO.
- [x] `aarch64` public release ISO artifacts are checked separately from
  automated screenshots.
  Current check: the aarch64 screenshot run uses verification-only media because
  public release media intentionally leaves storage interactive.
  The hydrated public release ISO SHA
  `13b2b59ea03054d66b3f8c0986c2314631437e57074685c515a1dffa3a4f6fbf` is
  checksum-verified by the release artifact gate instead of being used for
  automated capture.
- [ ] Produce coherent per-architecture signoff rows for one exact candidate.
  The historical `x86_64` row is internally coherent, but neither architecture
  is signed off for an exact stable candidate and the `aarch64` SHA mismatch
  remains unresolved.
- [ ] Run `./os/hardware-gate/verify-shipping-status.sh` after the SHA linkage and
  exact-candidate checks are enforced. A prior pass is not stable-readiness
  evidence because it predates the corrected media-linkage requirement and does
  not attest to an exact stable candidate.

## Stable Release Promotion

- [x] Make GHCR package public and verify:

```sh
docker buildx imagetools inspect ghcr.io/joe-simo/goblins-os:x86_64
docker buildx imagetools inspect ghcr.io/joe-simo/goblins-os:aarch64
podman manifest inspect ghcr.io/joe-simo/goblins-os:x86_64
podman manifest inspect ghcr.io/joe-simo/goblins-os:aarch64
```

These mutable alpha tags prove public package visibility only. They are never
stable-candidate provenance; every stable gate uses a digest reference from the
exact candidate workflow.

- [ ] Select a clean, pushed current `origin/main` commit and record it as the
  exact stable candidate.
- [ ] Export that full commit as `GOBLINS_OS_CANDIDATE_COMMIT` for every ISO,
  release-evidence, capture, close-signoff, and shipping-status command. Stable
  promotion fails if either architecture is missing it or records a different value.
- [ ] Dispatch the canonical `candidate-artifacts.yml` workflow for that commit,
  retain its exact run URL, and require both native architecture jobs to pass.
- [ ] Download the two metadata-only candidate artifacts from that exact run.
  Validate architecture, candidate commit, `non_promotional: true`, and each
  `immutable_image_ref`; never substitute the commit-scoped tag.
- [ ] Retain the digest-bound shippable media and package evidence for both
  architectures from that exact run without moving any public channel.
- [ ] Run the read-only x86_64 capture workflow with its exact digest. Build the
  aarch64 verification ISO with its exact digest, then complete the local native
  HVF display-backed capture. Do not use hydrated public release media for
  automated capture.
- [ ] Review and overlay the exact Actions/capture artifacts in a disposable
  checkout of the selected candidate. Require each ISO, BIB manifest, SBOM,
  screenshot proof, and signoff row to name the same candidate and per-arch
  immutable image digest, with each signoff row naming its verification ISO SHA.
- [ ] Run `GOBLINS_OS_CANDIDATE_COMMIT="$GOBLINS_OS_CANDIDATE_COMMIT"
  ./os/hardware-gate/verify-shipping-status.sh` in that evidence workspace and
  require a fully green result before any promotion.
- [ ] Preserve the selected source commit; attach reviewed generated evidence to
  the release instead of advancing or rebuilding the candidate merely to store proof.
- [ ] Create a stable release tag.
- [ ] Update website release data from alpha to stable.
- [ ] Run website checks after the stable release data is updated:

```sh
bun run lint
bun run typecheck
bun run build
```

- [ ] Deploy the stable production website.
- [ ] Verify the stable live domain, download links, checksum links, source
  links, and container pull commands.
