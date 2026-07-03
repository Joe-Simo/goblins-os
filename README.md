# Goblins OS

Goblins OS is an open AI-native desktop for building local software. It is built
on Fedora bootc, ships as architecture-specific release media, and keeps
credentials out of the desktop image.

The project is open source. The code is licensed under AGPL-3.0-or-later. The
Goblins OS name, marks, release identity, installer identity, desktop identity,
icons, wallpapers, and associated trade dress are reserved.

## Download

The current public release is available on the GitHub releases page:

- [Goblins OS releases](https://github.com/Joe-Simo/goblins-os/releases)
- [Website](https://goblinsos.com)

Install media is built separately for each CPU family. Use the ISO that matches
the target system:

- `x86_64` for 64-bit Intel and AMD systems
- `aarch64` for Arm systems and Arm virtual machines

Always verify the published SHA256 checksums before writing an installer image
to USB or attaching it to a VM.

## What it is

- A Fedora bootc desktop OS with image-based updates and rollback.
- A native desktop environment with Goblins OS branding and installer flows.
- A local app-building surface where users describe software, review the
  generated project, preview it locally, inspect files and logs, then export or
  containerize it.
- A project with explicit packaging, release, SBOM, and signoff checks.

## What it is not

- It is not a mobile OS.
- It is not an app store.
- It does not ship with bundled productivity apps.
- It does not include credentials or client-side secrets in the OS image.

## Containers

The bootc container images are intended for Docker/Podman inspection,
verification, automation, and derived-image workflows. Use the ISO when you want
the full graphical desktop installer.

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
