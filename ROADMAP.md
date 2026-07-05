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
- Container images: built by the release workflow; public pull depends on GHCR
  package visibility.
- Stable release status: still alpha; the current release gate passes, and a
  stable tag waits on public container pulls and post-alpha hardening.

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

- Make the GHCR container package public so Docker and Podman users can pull
  without authentication.
- Keep display-backed signoff current for every release candidate.
- Publish a stable tag after GHCR visibility and post-alpha hardening are done.
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
