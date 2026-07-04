# Goblins OS Go-Live Checklist

This checklist tracks what must be true before the alpha release can be promoted
to a stable public release.

## Public Release Surface

- [x] Source repository is public.
- [x] Website is live at <https://goblinsos.com>.
- [x] GitHub release exists for the current alpha.
- [x] Split ISO downloads are hosted on GitHub release assets, not the website
  host.
- [x] SHA256 files are published for the split download parts, compressed ISO,
  and final ISO.
- [x] Website includes install and checksum verification guidance.
- [ ] GHCR package visibility allows anonymous Docker and Podman pulls.
  Current check: anonymous `docker buildx imagetools inspect
  ghcr.io/joe-simo/goblins-os:x86_64` and `:aarch64` return `401 Unauthorized`;
  the connected GitHub CLI token also needs `read:packages`/`write:packages`
  before package visibility can be changed from this machine.

## Release Verification

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
- [ ] `aarch64` display-backed signoff run is complete.
  Current check: the local aarch64 macOS/HVF attempt correctly failed against
  hydrated public release media because that ISO leaves storage interactive; the
  capture harness now fail-closes before QEMU unless the ISO contains the
  verification-only hardware-gate kickstart. The manual
  `aarch64-verification-iso` workflow can build the capture-only ISO on a
  native GitHub arm runner when the local Apple-Silicon machine cannot build
  release media.
- [x] Latest signoff row records runner, ISO, checksums, self-test, runtime
  proof, app-build proof, gaming proof, storage proof, and SBOM evidence.
  Current check: the latest `x86_64` row from GitHub Actions run
  `28721788279` records runner, ISO, `blocked=0`, self-test, runtime/app-build,
  gaming, storage proof, release evidence/SBOM, and `Current project completion
  status: complete`.
- [ ] `./os/hardware-gate/verify-shipping-status.sh` passes.
  Current local check: with both release ISOs hydrated and checksum-verified,
  the gate still fails because `aarch64` has no complete display-backed
  screenshot/signoff row.

## Stable Release Promotion

- [ ] Make GHCR package public and verify:

```sh
docker buildx imagetools inspect ghcr.io/joe-simo/goblins-os:x86_64
docker buildx imagetools inspect ghcr.io/joe-simo/goblins-os:aarch64
podman manifest inspect ghcr.io/joe-simo/goblins-os:x86_64
podman manifest inspect ghcr.io/joe-simo/goblins-os:aarch64
```

- [ ] Complete per-architecture display-backed signoff.
- [ ] Hydrate release artifacts before local signoff. Use the default
  metadata/SBOM mode for lightweight review, or set `GOBLINS_OS_DOWNLOAD_ISO=1`
  only on a machine with enough disk and bandwidth for ISO reconstruction.
- [ ] Build or fetch the verification-only hardware-gate ISO for screenshot
  capture; do not use hydrated public release media for automated capture.
- [ ] Create a stable release tag.
- [ ] Update website release data from alpha to stable.
- [ ] Run website checks:

```sh
bun run lint
bun run typecheck
bun run build
```

- [ ] Deploy production website.
- [ ] Verify live domain, download links, checksum links, source links, and
  container pull commands.
