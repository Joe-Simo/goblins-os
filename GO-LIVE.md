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
- [ ] GHCR package visibility allows anonymous container pulls.

## Release Verification

- [x] Release workflow builds architecture-specific media for `x86_64` and
  `aarch64`.
- [x] Release artifacts include manifests and package evidence.
- [x] Source and generated artifact scans check for live secrets.
- [ ] `x86_64` display-backed signoff run is complete.
- [ ] `aarch64` display-backed signoff run is complete.
- [ ] Latest signoff row records runner, ISO, checksums, self-test, runtime
  proof, app-build proof, gaming proof, storage proof, and SBOM evidence.
- [ ] `./os/hardware-gate/verify-shipping-status.sh` passes.

## Stable Release Promotion

- [ ] Make GHCR package public and verify:

```sh
docker buildx imagetools inspect ghcr.io/joe-simo/goblins-os:x86_64
docker buildx imagetools inspect ghcr.io/joe-simo/goblins-os:aarch64
```

- [ ] Complete per-architecture display-backed signoff.
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
