# Goblins OS Roadmap

This roadmap tracks product and release work at a public level. Detailed CI
runs, release evidence, and raw proof logs live in the release artifacts and
signoff files, not in this overview.

## Current Release State

- Public website: live at <https://goblinsos.com>.
- Source repository: public.
- Current release: `v0.1.0-alpha.20260703` for 64-bit Arm (`aarch64`).
- Production and release architecture: `aarch64` only.
- ISO media: the current Arm installer is published as split GitHub release
  assets.
- Container image: public anonymous pulls are available from GHCR for
  `ghcr.io/joe-simo/goblins-os:aarch64`.
- Historical record: the already-published `x86_64` alpha assets remain attached
  to `v0.1.0-alpha.20260703` as immutable provenance. They are not supported,
  current, or eligible for promotion.
- Stable release status: still alpha. Stable promotion requires one exact Arm
  candidate commit tied to an immutable image digest, fresh `aarch64` release
  media, display-backed proof, coherent signoff hashes, reviewed external
  evidence, and synchronized stable website data.

## Shipped Foundation

- Fedora bootc image-based base.
- Open AI-native desktop direction for building local software under user
  control.
- Native desktop surfaces built primarily in Rust.
- Goblins OS branding for installer, desktop, settings, and release media.
- Native `aarch64` release workflow and architecture-bound evidence.
- Package evidence and SBOM generation for Cargo and RPM dependencies.
- Secret boundary that keeps credentials out of the image and desktop session.
- Installer guardrails for Arm compatibility, checksum verification, storage
  review, and dual-boot preservation.
- Website with downloads, container image commands, install guidance, checksum
  verification, source links, notice, and marks policy.

## Active Release Work

- Keep the mismatched July 5 `aarch64` verification-ISO proof and signoff row as
  an incomplete historical record only. Their media hashes differ, so neither
  may be repaired in place, reused, or promoted as current evidence.
- Select an exact pushed stable candidate and use the digest-bound,
  non-promotional candidate workflow plus read-only capture paths for a fresh
  Arm image, installer, release evidence, and display-backed proof that all name
  that same commit, digest, workflow attempt, and media.
- Publish a stable tag only after the exact-candidate gates and signoff close.
- Keep the website release data synchronized with the published artifacts.

## Product Work

- Continue hardening Settings panels for display, sound, privacy, accessibility,
  developer, storage, recovery, and update workflows.
- Improve the app-building flow across describe, project review, local preview,
  file/log inspection, export, and containerization.
- Strengthen update and rollback UX for bootc deployments.
- Expand hardware and device proof for audio, controller, display, input source,
  Bluetooth, printer, and accessibility paths.
- Keep gaming support Steam-free by default while verifying Vulkan, GameMode,
  gamescope, MangoHud, PipeWire, and controller diagnostics.

## Release Boundaries

- Keep the product lane focused on an open AI-native desktop for building local
  software, with container-friendly release artifacts and transparent
  verification.
- Keep user-facing claims tied to verified hardware, runtime, package,
  installer, and app-generation evidence.
- Keep credentials and API keys outside the OS image and out of ordinary desktop
  UI surfaces.
