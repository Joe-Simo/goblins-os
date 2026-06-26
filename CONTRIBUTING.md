# Contributing to Goblins OS

Thanks for your interest in Goblins OS — the OS you build yourself, at macOS-grade
polish, in an OpenAI-style design language.

## Licensing of contributions

- Goblins OS's own source is licensed **AGPL-3.0-or-later** (see [`LICENSE`](LICENSE)).
- Before your first contribution is merged, you must agree to the
  **[Contributor License Agreement](CLA.md)**. This keeps the project's copyright
  clean and lets the owner offer commercial licenses alongside the AGPL. Typically a
  bot records your agreement when you open your first pull request.
- The **"Goblins OS" name and marks are reserved trademarks** — see
  [`TRADEMARKS.md`](TRADEMARKS.md). You may build, run, modify, and redistribute the
  code under the AGPL, but you may not use the Goblins OS name or marks to brand a
  fork or imply endorsement without permission.

## Before you open a pull request

The OS ships only when the gate is green. Run, at minimum:

```sh
cargo fmt --all -- --check        # format (per-crate `-p` if a host/CI rustfmt skew bites)
cargo clippy --workspace --all-targets -- -D warnings
cargo test --workspace
```

The full release gate additionally builds the bootc image and runs
`goblins-os-verify` (expects `blocked=0`) plus light+dark render checks in CI/qemu —
the GTK/gnome-shell desktop code is `cfg(target_os = "linux")` and only renders in a
real session, so those checks run in CI, not on a macOS host.

## Scope and design

- New software is added to the OS image via `os/bootc/Containerfile`; runtime
  defaults via dconf (`os/dconf`) and systemd units; app surfaces are Rust crates;
  shell features are gnome-shell extensions.
- The product thesis is **you build your apps** (described to the on-device model) —
  there is no app store and no bundled productivity apps. Keep changes consistent
  with that, and with the honest-status, no-fake-data, server-side-secrets rules.

## Reporting issues

Open an issue with clear reproduction steps. For security issues, please disclose
privately to the maintainers rather than in a public issue.
