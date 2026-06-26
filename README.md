# Goblins OS

**The OS you build yourself** — an image-based Linux desktop at macOS-grade polish,
in an OpenAI-style design language. You describe the app you want; the on-device
model builds it. No app store, no bundled productivity apps — just the OS, your
brand of computing, made by you.

Goblins OS is built on **fedora-bootc** (an immutable, image-based OS), so updates
ship as whole signed images that installed systems pull and apply **atomically, with
rollback** — distribution that's safer than package-by-package updates.

- **Design & product north star:** [`GOAL.md`](GOAL.md)
- **Build / verify / ship gates:** [`SHIP.md`](SHIP.md)
- **Contributing:** [`CONTRIBUTING.md`](CONTRIBUTING.md)

## Licensing

Goblins OS is **open source and owned** — the code is open; the brand is reserved.

| What | Terms |
|---|---|
| **Goblins OS's own source** (the `crates/` and `os/` work in this repo) | **AGPL-3.0-or-later** — see [`LICENSE`](LICENSE) |
| **Bundled OS components** (Fedora base, Linux kernel, GNOME, and other packages in the image) | Each keeps its **own upstream license**; Goblins OS redistributes them under their terms (see the SBOM / third-party notices under `os/release/` and `os/signoff-proofs/sbom/`) |
| **The "Goblins OS" name and marks** | **Reserved trademarks** — see [`TRADEMARKS.md`](TRADEMARKS.md). Not licensed under the AGPL. |
| **Contributions** | Require the [Contributor License Agreement](CLA.md), which keeps the copyright clean and lets the owner offer commercial terms alongside the AGPL. |

**Commercial licensing.** The AGPL's copyleft (including its network clause) requires
that anyone who uses or modifies the code — even as a hosted service — make their
changes available under the AGPL. If you need different terms (e.g. to ship a closed
product built on Goblins OS), a **commercial license is available from the project
owner**. This dual-licensing model keeps Goblins OS genuinely open while preserving
the owner's ability to sell.

> The license setup in this repo (LICENSE, CLA, TRADEMARKS) is a sound starting point
> but should be reviewed by IP counsel before it's relied upon commercially or before
> the first outside contribution is accepted.

## Free to use

Goblins OS is freely available to download, install, run, and modify under the AGPL —
like any open Linux distribution. Build it, install the ISO, and installed systems
auto-update from the published image.
