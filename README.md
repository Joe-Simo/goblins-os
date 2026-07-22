# Goblins OS

Goblins OS is an open AI-native desktop for building local software. It is built
on Fedora bootc, ships for 64-bit Arm (`aarch64`), and keeps credentials out of
the desktop image.

The project is open source. The code is licensed under AGPL-3.0-or-later. The
Goblins OS name, marks, release identity, installer identity, desktop identity,
icons, wallpapers, and associated trade dress are reserved.

## Download

The current public release is available on the GitHub releases page:

- [Goblins OS releases](https://github.com/Joe-Simo/goblins-os/releases)
- [Website](https://goblinsos.com)

The current installer is built only for `aarch64`. Its verified configuration
scope is a UEFI aarch64 virtual machine; bare-metal Arm support is not claimed
until an exact device model completes the hardware matrix. Apple Silicon is an
HVF proof host, not a bare-metal Goblins OS install target. Intel and AMD
`x86_64` systems are not supported.

The `x86_64` files already attached to
[`v0.1.0-alpha.20260703`](https://github.com/Joe-Simo/goblins-os/releases/tag/v0.1.0-alpha.20260703)
are retained as immutable historical release records. They are not a supported
or current Goblins OS target and must not be promoted into a current channel.

Always verify the published SHA256 checksums before attaching the installer ISO
to a supported UEFI aarch64 virtual machine.

## What it is

- A Fedora bootc desktop OS with image-based updates and rollback.
- A native desktop environment with Goblins OS branding and installer flows.
- A local app-building surface where users describe software, review the
  generated project, preview it locally, inspect files and logs, then export or
  containerize it.
- A project with explicit packaging, release, SBOM, and signoff checks.

## Scope

Goblins OS focuses on the desktop operating system, the local app-building
workflow, container-friendly release artifacts, and transparent verification.
Credentials and API keys stay outside the OS image and are not shipped to the
desktop as client-side secrets.

## Containers

The current `ghcr.io/joe-simo/goblins-os:aarch64` bootc container image is
intended for Docker/Podman inspection, verification, automation, and
derived-image workflows. Use the ISO when you want the full graphical desktop
installer.

Container package visibility is tracked separately from the public source repo.
If a `docker pull` or `podman pull` command asks for authentication, the GHCR
package has not yet been made public.

## Development

- [Contributing](CONTRIBUTING.md)
- [Roadmap](ROADMAP.md)
- [Release engineering](SHIP.md)

## Forks and attribution

You can study, modify, and redistribute the source under the AGPL. Modified
distributions must keep the required license and attribution notices, state what
changed, provide the required source, and use their own product name and branding
unless they have written permission to use the Goblins OS marks.

Automated rebranding, AI-generated patches, copied release pages, renamed ISO
artifacts, or generated derivatives do not create permission to remove notices,
claim official status, or use Goblins OS identity. See [NOTICE](NOTICE) and
[TRADEMARKS.md](TRADEMARKS.md).

## Licensing

| What | Terms |
| --- | --- |
| Goblins OS source in this repository | AGPL-3.0-or-later. See [LICENSE](LICENSE). |
| Bundled OS components | Each component keeps its upstream license. Release SBOMs and package evidence are generated under `os/signoff-proofs/sbom/`. |
| Goblins OS name, marks, and product identity | Reserved project marks. See [NOTICE](NOTICE) and [TRADEMARKS.md](TRADEMARKS.md). |
| Contributions | Contributions require the [Contributor License Agreement](CLA.md). |

For legal or trademark questions, review the relevant files with qualified
counsel before relying on them for production or commercial use.
