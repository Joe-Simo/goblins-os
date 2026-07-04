# Goblins OS Product Direction

Goblins OS is a desktop operating system for people who want to build and run
their own local software. The OS provides the base system, native desktop
surfaces, install/update flow, and app-building tools. Users bring the intent;
the system helps turn it into working local projects.

## Product Principles

- **Local-first app creation.** The main workflow is describing an app, reviewing
  the generated project, and keeping the result on the machine.
- **User-owned app library.** Goblins OS keeps built projects on the machine.
  System utilities ship with the OS; user apps are created, reviewed, and owned
  locally.
- **Native desktop quality.** Core surfaces should feel cohesive, responsive, and
  intentional, using the Goblins OS design system and Inter typography.
- **Fedora bootc foundation.** The base OS, updates, and rollback model stay
  image-based and release-verifiable.
- **Clear security boundary.** Credentials stay outside the public image and out
  of client-side code. Desktop apps receive status and capability information,
  not raw secrets.
- **Honest capability states.** If a runtime, device, permission, or service is
  unavailable, the UI should say so plainly and degrade safely.
- **Per-architecture releases.** Arm and x86_64 are separate native build tracks
  with separate media, checksums, manifests, and proof.

## Core Surfaces

- Installer and first boot
- Login, lock, and desktop shell
- Home and app-building flow
- Build Studio and generated-app detail views
- Settings, recovery, policy, storage, display, sound, privacy, and developer
  panels
- Release verification, package evidence, and updater paths

## Quality Bar

A surface is ready when it:

- Uses the shared Goblins OS design tokens and Inter typography.
- Works in light and dark appearance.
- Handles keyboard, pointer, and accessibility expectations.
- Avoids raw backend state, diagnostic labels, and implementation jargon in user
  copy.
- Has deterministic behavior under the verifier or hardware gate appropriate to
  that surface.
- Does not fabricate missing hardware, runtime, package, or screenshot evidence.

The OS is release-ready when the release gate passes for every supported
architecture and the public download, checksum, source, and package paths are
consistent.
