# Contributing to Goblins OS

Goblins OS is an open AI-native desktop for building local software on Fedora
bootc. Contributions should preserve the project's native desktop direction,
open-source release process, and server-side secret boundary.

## Licensing

- Goblins OS source is licensed AGPL-3.0-or-later. See [LICENSE](LICENSE).
- Keep the project [NOTICE](NOTICE), attribution, and license notices intact.
- Before a contribution is merged, contributors must agree to the
  [Contributor License Agreement](CLA.md).
- The Goblins OS name and marks are reserved project marks. See
  [TRADEMARKS.md](TRADEMARKS.md).

## Before opening a pull request

Run the relevant local checks for the files you changed. For Rust work, start
with:

```sh
cargo fmt --all -- --check
cargo clippy --workspace --all-targets -- -D warnings
cargo test --workspace
```

Release work also builds the bootc image, runs `goblins-os-verify`, generates
package evidence, and validates display-backed installer or desktop proof where
required. See [SHIP.md](SHIP.md) for the full release process.

## Engineering expectations

- Keep credentials out of client-side code and out of the OS image.
- Prefer existing system APIs, GNOME/GTK facilities, systemd units, and Rust
  crates over custom one-off mechanisms.
- Add software to the OS image through `os/bootc/Containerfile` and extend the
  verifier contract when a release-critical binary, service, desktop file, or
  package becomes required.
- Do not fake runtime, hardware, package, or screenshot proof. A degraded state
  should say what is unavailable and why.
- Do not strip Goblins OS attribution, release provenance, or trademark
  boundaries from public copy, generated files, release artifacts, or AI-created
  patches.

## Reporting issues

Open an issue with clear reproduction steps, expected behavior, and actual
behavior. Report security issues privately to the maintainers instead of opening
a public issue.
