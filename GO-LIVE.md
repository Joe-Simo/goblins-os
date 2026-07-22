# Goblins OS Go-Live Checklist

This checklist tracks the live Arm alpha release and the remaining steps before
a stable public release. The sole supported production and release architecture
is 64-bit Arm (`aarch64`).

## Public Release Surface

- [x] Source repository is public.
- [x] Website is live at <https://goblinsos.com>.
- [x] GitHub release `v0.1.0-alpha.20260703` exists for the current Arm alpha.
- [x] Split `aarch64` ISO downloads are hosted on GitHub release assets, not the
  website host.
- [x] SHA256 files are published for the Arm split download parts, compressed
  ISO, and final ISO.
- [x] Website includes Arm install and checksum verification guidance.
- [x] GHCR package visibility allows anonymous Docker and Podman pulls for
  `ghcr.io/joe-simo/goblins-os:aarch64`.

## Immutable Historical x86_64 Record

The `x86_64` files already published with `v0.1.0-alpha.20260703` are preserved
only as immutable release provenance. They are not supported, current, or
eligible for stable promotion, and no product surface may present them as an
installation choice or current container target.

- [x] The historical GitHub release retains its original `x86_64` filenames,
  manifests, checksums, and package evidence.
- [x] The historical public ISO SHA256 remains
  `45abf064735fa2a2ba9ef034883d19453c4bfc02a3b0c311d29e3679c52db434`.
- [x] Historical display-backed proof remains attributable to GitHub Actions run
  `28721788279`, `os/screenshots/hardware-gate/x86_64/2026-07-04`, and
  verification-ISO SHA256
  `10d72f00b43d39411cb193154e51b8e8c98f142abcf1246fd87e7f4456046683`.
- [x] The previously published mutable `:x86_64` GHCR tag is excluded from
  current release instructions. A mutable tag is not immutable provenance and
  must not be copied or promoted as a supported release.

## Current Arm Verification

- [x] The release workflow produces architecture-bound `aarch64` media.
- [x] Arm release artifacts include manifests and package evidence.
- [x] Published Arm release metadata and SBOMs can be hydrated into the local
  gate layout without downloading multi-gigabyte ISO media by default:
  `os/release/hydrate-release-artifacts.sh`.
- [x] Full Arm ISO release media can be hydrated from split GitHub release assets
  with `GOBLINS_OS_DOWNLOAD_ISO=1`, verified part-by-part, decompressed, and
  verified against the final ISO SHA256.
- [x] Display-backed verification-ISO screenshot and runtime proof is checked
  separately from the public release ISO artifact chain. Neither proof route
  can substitute for the other.
- [x] Source and generated artifact scans check for live secrets.
- [x] The hydrated Arm public release ISO SHA256 is
  `13b2b59ea03054d66b3f8c0986c2314631437e57074685c515a1dffa3a4f6fbf`.
- [x] Retire the mismatched July 5 `aarch64` display-backed verification-ISO
  proof and signoff row from current-candidate eligibility. The proof manifest
  records ISO SHA256
  `3c73a77335b8be7b1fdaeb73e7992bacf6ec253cb0755f030484a673b0c293dc`,
  while the latest `aarch64` signoff row records
  `539fe24454f5cf1b0bb3ac00c9b8a838614ada85a310511fb9605afa978686a7`.
  The mismatch is preserved for audit, but that run is obsolete and cannot be
  repaired or reused. Stable proof must come from a fresh exact-candidate
  capture and signoff that identify the same verification ISO.
- [ ] Produce one coherent `aarch64` signoff row for one exact candidate.
- [ ] Run `./os/hardware-gate/verify-shipping-status.sh` after the Arm-only policy,
  SHA linkage, and exact-candidate checks are enforced. A prior pass is not
  stable-readiness evidence because it predates the corrected media-linkage
  requirement and does not authenticate an exact stable candidate.

## Stable Release Promotion

- [x] Make the current Arm GHCR package public and verify:

```sh
docker buildx imagetools inspect ghcr.io/joe-simo/goblins-os:aarch64
podman manifest inspect ghcr.io/joe-simo/goblins-os:aarch64
```

The mutable Arm alpha tag proves public package visibility only. It is never
stable-candidate provenance; every stable gate uses a digest reference from the
exact candidate workflow.

- [ ] Select a clean, pushed current `origin/main` commit and record it as the
  exact stable candidate.
- [ ] Export that full commit as `GOBLINS_OS_CANDIDATE_COMMIT` for every Arm ISO,
  release-evidence, capture, close-signoff, and shipping-status command.
- [ ] Dispatch the canonical candidate workflow for that commit, retain its
  exact run URL, and require the native `aarch64` release job to pass.
- [ ] Download the metadata-only Arm candidate artifact from that exact run.
  Validate architecture, candidate commit, `non_promotional: true`, and its
  `immutable_image_ref`; never substitute the commit-scoped tag.
- [ ] Retain the digest-bound Arm shippable media and package evidence from that
  exact run without moving any public channel.
- [ ] Build the `aarch64` verification ISO with its exact digest, then complete
  the local native HVF display-backed capture. Do not use hydrated public release
  media for automated capture. Require the repository-pinned capture-host CMS
  signature; a dispatcher-created GitHub attestation is not display authority.
- [ ] Review and overlay the exact Actions and capture artifacts in a disposable
  checkout of the selected candidate. Require the Arm ISO, BIB manifest, SBOM,
  screenshot proof, and signoff row to name the same candidate and immutable
  image digest, with the signoff row naming its verification ISO SHA.
- [ ] Run `GOBLINS_OS_CANDIDATE_COMMIT="$GOBLINS_OS_CANDIDATE_COMMIT"
  ./os/hardware-gate/verify-shipping-status.sh` in that evidence workspace and
  require a fully green Arm result before any promotion.
- [ ] Preserve the selected source commit; attach reviewed generated evidence to
  the release instead of advancing or rebuilding the candidate merely to store
  proof.
- [ ] Create a stable release tag containing only current `aarch64` media and
  evidence. Do not copy historical `x86_64` assets into it.
- [ ] Update website release data from alpha to stable.
- [ ] Run website checks after the stable release data is updated:

```sh
bun run lint
bun run typecheck
bun run build
```

- [ ] Deploy the stable production website.
- [ ] Verify the stable live domain, Arm download links, checksum links, source
  links, and Arm container pull commands.
