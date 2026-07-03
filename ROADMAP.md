# Goblins OS Roadmap

This roadmap tracks product and release work at a public level. Detailed CI runs,
operator notes, and raw proof logs live in the release artifacts and signoff
files, not in this overview.

## Current Release State

- Public website: live at <https://goblinsos.com>.
- Source repository: public.
- Current release: `v0.1.0-alpha.20260703`.
- ISO media: published as split GitHub release assets for `x86_64` and
  `aarch64`.
- Container images: built by the release workflow; public pull depends on GHCR
  package visibility.
- Stable release status: still alpha until the remaining per-architecture
  signoff requirements are complete.

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
- Complete the display-backed signoff path for every supported architecture.
- Publish a stable tag after the release gate passes.
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

## Non-Goals

- Goblins OS is not a mobile OS.
- Goblins OS is not an app store.
- Goblins OS does not bundle productivity apps as the main value proposition.
- Goblins OS should not claim hardware, runtime, package, or app-generation
  support that has not been verified.
