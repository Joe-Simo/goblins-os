# Goblins OS Roadmap

This roadmap tracks product and release work at a public level. Detailed CI
runs, release evidence, and raw proof logs live in the release artifacts and
signoff files, not in this overview.

## Current Release State

- Public website: live at <https://goblinsos.com>.
- Source repository: public.
- Current release: `v0.1.0-alpha.20260703`.
- ISO media: published as split GitHub release assets for `x86_64` and
  `aarch64`.
- Container images: public anonymous pulls are available from GHCR for both
  `x86_64` and `aarch64`.
- Stable release status: still alpha. Stable promotion requires one exact
  candidate commit tied to fresh dual-architecture release media, display-backed
  proof, coherent signoff hashes, and the stable website data.

## Shipped Foundation

- Fedora bootc image-based base.
- Open AI-native desktop direction for building local software under user
  control.
- Native desktop surfaces built primarily in Rust.
- Goblins OS branding for installer, desktop, settings, and release media.
- Per-architecture release workflow for `x86_64` and `aarch64`.
- Package evidence and SBOM generation for Cargo and RPM dependencies.
- Secret boundary that keeps credentials out of the image and desktop session.
- Installer guardrails for architecture choice, checksum verification, storage
  review, and dual-boot preservation.
- Website with downloads, container image commands, install guidance, checksum
  verification, source links, notice, and marks policy.

## Active Release Work

- Reconcile the `aarch64` verification-ISO proof-manifest SHA with its signoff
  row; the recorded values currently identify different media.
- Select an exact stable candidate and capture fresh per-architecture release
  and display-backed proof for that same commit and media.
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
