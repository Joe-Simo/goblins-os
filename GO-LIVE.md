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
- [ ] `x86_64` display-backed screenshot/runtime run matches the current
  hydrated release ISO.
  Current check: `os/screenshots/hardware-gate/x86_64/2026-07-03` contains the
  expected screenshot and proof files, but its `proof-manifest.json` records a
  different ISO SHA256 than the current hydrated `goblins-os-x86_64.iso`.
- [ ] `aarch64` display-backed signoff run is complete.
- [ ] Latest signoff row records runner, ISO, checksums, self-test, runtime
  proof, app-build proof, gaming proof, storage proof, and SBOM evidence.
  Current check: the latest `x86_64` row from GitHub Actions run
  `28710819638` records runner, ISO, `blocked=0`, self-test, runtime/app-build,
  gaming, and storage proof, but still records release evidence/SBOM as not
  checked.
- [ ] `./os/hardware-gate/verify-shipping-status.sh` passes.
  Current local check: with both release ISOs hydrated and checksum-verified,
  the gate still fails because neither architecture has a complete
  current-ISO display-backed screenshot/signoff row, and the latest signoff row
  does not record release evidence/SBOM.

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
