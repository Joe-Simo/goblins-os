# GOAL — "The OS you build yourself," at macOS-grade polish, in an OpenAI design language

> Living north-star + tracker for the visual & product elevation of this OS.
> Companion to `SHIP.md` (build/verify/ship gates). This file defines *what
> "exceptionally beautiful, polished, high quality" means here* and the phased
> plan to get there. Update the **Status** line per phase as work lands.

**Product name:** Goblins OS. ("OpenAI" here means the *design language / style*,
not the product name — the OS is **Goblins OS** end to end: boot, onboarding,
menu bar, lock, everything.)

**Status:** _2026-06-19 — Window management is now built, wired, rebuilt into `goblins-os:latest`, and verified at real pixels. The system has a real desktop workflow, not a kiosk: Goblins GNOME Shell extension `goblins-wm@goblins.os` provides Mission Control with live `Clutter.Clone` thumbnails, Spaces, app/window switcher, Snap Assist, Window Actions HUD, motion/material styling, and touch support (`touch-event`, swipe thresholds, 44px targets, `libinput`/`libwacom`). Stage-style grouping stays intentionally off beyond lightweight app grouping because the optional mode was not better than the core desktop workflow. Current image digest: `sha256:8c8a9b4b5b16ed36a874287acb0c74c2e575359fade76f06bb99b23aa72cce1c`. Installed verifier PASS `total=88 blocked=0` (`os/signoff-proofs/2026-06-19-wm/installed-verify-current-after-dark-qemu.log`); `gate.Dockerfile` PASS (`fmt`, `clippy -D warnings`, tests; `.../gate-current-after-dark-qemu.log`). Render proofs: Xvfb desktop/WM surfaces light+dark (`os/screenshots/2026-06-19-wm/desktop/`), real-session QEMU light (`.../qemu/76-light-installed-f9.png`, `77-light-installed-switcher.png`, `82-light-installed-hud-left-snap-2.png`), and real-session QEMU dark (`.../qemu-dark-current/dark-final-{mission-control,switcher,hud,snap-left}.png`). Final branded ISO remains rebuilt at `/tmp/goblins-os-bib-output/bootiso/install.iso`, SHA256 `946c43559ec0b34dff5cebee5e84edacd70295d40a95a7f7d5f704e2e573fea1`, volume `GOBLINS_OS`. Remaining external gate remains physical GPU motion feel on hardware._

**Prior status:** _2026-06-18 — full chain verified at real pixels; a multi-agent design review then drove a macOS/OpenAI-grade polish pass. Headline: a DISTINCT Goblins mark now owns the system identity (menu bar/lock/installer/app tiles), OpenAI bloom kept only as the provider badge; raw-Markdown build results now render as Pango; app names use the model's title (not a prompt slice); blue-diamond fallback killed via StartupWMClass + Icon on every .desktop; dev-path + raw-ifname leaks removed. fmt/clippy/test green, verify blocked=0. Real-pixel re-verify done for onboarding/lock/home/build/app-detail AND (via a fresh qemu install of the rebuilt image) the Activities overview — the menu bar shows the new Goblins mark and the window/dock icons carry it with ZERO blue diamonds (proof `os/screenshots/polish-overview-no-diamonds.png`). An adversarial 3-lens re-review rated the branded surfaces "Apple-tier"; its two "blockers" were STALE pre-polish dark captures — refuted by re-capturing the DARK onboarding/lock/overview from the current image (Goblins mark + "GOBLINS OS", zero blue diamonds; `os/screenshots/installed-{desktop-dark-overview,welcome-onboarding-dark,lock-dark}.png`). Static composition is macOS-grade across light+dark/both code paths. The only inherently-uncapturable item remains GPU motion FEEL (hardware-QA)._
**It is now a real windowed OS, not a kiosk:** the home is a rounded, shadowed
window on the desktop (wallpaper + translucent menu bar + custom glass dock around
it), `os-release` says Goblins OS, the dock has graphite icon tiles, the home
header is brand-only, and `verify` stays **blocked=0**. P0 (render harness) done;
P3 (desktop environment) substantially done. Light is the shipped default and looks
premium; dark-mode wallpaper switch is a headless-render quirk to confirm on real
hardware.

**P5 (verifiable scope) done:** Codex→on-device-GPT-OSS keyless provider config
shipped+seeded; the engine picker is reframed to two primary paths ("On-device ·
GPT-OSS" / "OpenAI account · Codex") in Settings, API key demoted to Advanced;
the installer welcome states the build-it-yourself thesis. The LIVE Codex CLI +
GPT-OSS /v1 run remains SHIP.md's external "runtime model" gate.
**P1/P2 in progress:** menu-bar Goblins mark (gnome-shell extension); Plymouth
boot splash (calm dark + OpenAI mark) replacing Fedora's; installer welcome reframed.
**All in-sandbox phases landed (2026-06-17), verified at real pixels:**
- P1 ✓ menu-bar Goblins mark, layered panel shadows, engine-advanced demotion.
- P2 ✓ os-release = Goblins OS, Plymouth splash (boot visual = hardware gate);
  GRUB theme + GDM greeter skipped (autologin bypasses GDM; GRUB is bootc-managed/
  boot-menu-only — real-hardware niceties).
- P3 ✓ windowed desktop: menu bar + dock + wallpaper + rounded home window.
- P4 ✓ Files (nautilus) + Terminal (gnome-console) shipped + GoblinsOS icon theme →
  consistent graphite dock tiles; dock favorites + icon-theme via dconf.
- P5 ✓ two-path engine picker (Settings + installer), Codex→GPT-OSS keyless config.
- P6 ✓ **fmt + clippy(-D warnings) + 128 tests PASS; verify blocked=0; bootc lint;
  install+services selftest PASS.** Design-critique punch-list applied: Studio is
  fully monochrome (no git color), all off-brand status pills removed, Codex icon →
  AI sparkle (distinct from Terminal), login primary = prominent white pill.

**Installable ISO — BUILT in-sandbox (2026-06-17), verified:** docker
bootc-image-builder (anaconda-iso, xfs) via the registry + named-volume recipe;
`install.iso` 2.3 GB in `/tmp` (kept out of the iCloud repo). Booted in qemu →
Anaconda "for Goblins OS 44 started" → Installation Summary.

**Anaconda installer — FULLY rebranded (2026-06-17), verified at real pixels.**
The installer chrome (sidebar art + accent) ships in fedora-logos INSIDE the
installer-runtime squashfs (`/images/install.img`), which BIB builds from stock
Fedora packages — a runtime separate from the Goblins image, so image-level edits
can't reach it (the title rebrands via os-release; the sidebar does not). Fixed
properly with `os/iso/remaster-anaconda-branding.sh`: rebuild install.img with the
Goblins dark sidebar + white OpenAI-style mark + dark topbar + recolored `@fedora`
ink, then `xorriso` "replay" re-master preserving UEFI El Torito + `implantisomd5`.
Re-booted the remastered ISO in qemu → Installation Summary now shows the **dark
Goblins sidebar + white mark + "GOBLINS OS 44 INSTALLATION"** (proof:
`os/screenshots/iso-boot-anaconda-goblins.png`; ISO SHA256
`79289a30a1701db116f73424e8ceffeb2b1c89c0f180dce4db956031a1e2fc44`; full notes:
`os/signoff-proofs/iso-docker-bib-final-sha256-20260617.txt`).

**Runtime-model gate — VERIFIED headless, incl. real gpt-oss:20b (2026-06-17).**
Ran the REAL shipped `goblins-os-core` against a REAL on-device open-weight model
(Ollama protocol, loopback) and drove the actual product flow: permission-gate
`app-builder` → `POST /v1/apps/builds {intent}` → `resident_generate` → live local
inference → a real, coherent app plan persisted as an OS-owned `BuiltApp` (no apps,
no store — the user builds it). First proven with a fast stand-in (llama3.2:3b), then
with the **actual shipped `gpt-oss:20b` (13 GB, 20.91 B-param MoE)**: bumped the Docker
VM to its 12 GB cap (half of this 24 GB Mac), ran gpt-oss:20b via `use_mmap` (peak
~8.8 GB RSS, ~1.5–2 tok/s CPU), and built "Pomodoro Flow" through the daemon end to
end. Proof: `os/signoff-proofs/app-build-live-model-20260617.txt` +
`…-artifact-20260617.json` + `…-gpt-oss-20b-artifact-20260617.json`; harness:
`os/runtime-gate/build-an-app-live-model.sh`. Product improvement landed: the resident
read timeout is now env-configurable (`GOBLINS_OS_RESIDENT_TIMEOUT_SECS`, default 120s,
clamped 5–3600s) for slow on-device models on modest hardware — fmt/clippy clean, 79
core tests pass. Codex stays external by design (the OS shells out to the user's Codex
CLI; it never holds the credential — verified the relay refuses to act, exposes no secret).

**Full install → desktop chain — VERIFIED in qemu at real pixels (2026-06-18).**
Drove the unbroken chain on a real (virtual) machine: branded ISO + a verification-only
kickstart (scratch disk; shipped ISO unchanged — it still leaves disk selection
interactive) → Anaconda "Install Goblins OS 44" → auto-partition + ostree deploy →
reboot → the INSTALLED bootc system boots from disk (GRUB → kernel → ostree-prepare-root
→ systemd → `goblins-os-core` + `-resident` started → gdm → autologin → goblins-os
session) → rendered the branded **Welcome onboarding** (two-path GPT-OSS/Codex picker,
"no apps, no store: you build it"), the **session-gate login**, and the themed desktop
(Goblins menu bar + goblins-dock + wallpaper) — in **both Light and Dark**, with the
dark wallpaper (goblins-os-dark.svg) confirmed loading on real GNOME (a 2nd dark-default
verify install). Proof: `os/signoff-proofs/installed-desktop-qemu-20260618.txt` +
`os/screenshots/installed-*.png` (incl. `installed-{desktop-dark-overview,welcome-onboarding-dark}.png`).
ZERO Fedora/GNOME-default leakage across boot/install/onboard/desktop.
(Note: the headless render harness can't switch mutter's background to the dark wallpaper —
a mutter-headless limitation, not a product bug; dconf picture-uri-dark is correct, proven
on real GNOME above.)

**Remaining gate — genuinely physical hardware only:** qemu renders with llvmpipe
(software GL, no GPU), so the *motion feel* — 60 fps micro-interactions, the dark-mode
wallpaper switch, animation smoothness — must be judged on a real GPU-backed machine.
The build-and-use-in-GUI step additionally needs a model in the VM + first-boot unlock;
that path is already proven at the daemon layer with real gpt-oss:20b. For real-hardware
*updates*: push the image to a published registry so the ISO's `bootc switch` origin
isn't the local build registry.
**Render-only artifacts (NOT shipped defects):** the root "privileged user"
notification, orange lock badge, and text fringing (render runs gnome-shell as
root with `--unsafe-mode` under llvmpipe; shipped OS runs as unprivileged `goblin`).
**Refinement backlog:** per-scheme (light/dark) shell-theme menu-bar material;
settings radii/type-ladder; standardize one soft shadow token.

_Earlier (authored before the first render):_
- P0 render harness — `os/bootc/render-desktop.sh` + `render-desktop.suffix.Dockerfile`
  (headless GNOME Shell → composited-desktop screenshots, light+dark).
- Wallpaper — `os/brand/wallpaper/goblins-os-{light,dark}.svg` (ships via `COPY os/brand/`).
- GNOME Shell theme `GoblinsOS` — `os/themes/GoblinsOS/gnome-shell/gnome-shell.css`
  (menu bar / control center / dock glass / popups / lock).
- dconf desktop defaults + profile — `os/dconf/{profile/user,db/local.d/10-goblins-os-desktop}`
  (wallpaper, slate accent, Inter, macOS-left window buttons, dash-to-dock + user-theme).
- Shell mode — `os/gnome-shell-modes/goblins-os.json` now force-enables the dock +
  user-theme and adds a clock (dateMenu) to the menu bar.
- Containerfile — installs dash-to-dock + user-theme, ships the theme, rebrands
  `os-release` (NAME/PRETTY_NAME/VARIANT → Goblins OS; ID stays fedora).

**Remaining chrome (next, compile-free):** Plymouth + GRUB boot art; a left-side
Goblins mark in the menu bar (small extension); GTK named-color/accent theming for
the GNOME stock utilities (P4); and the shell-app fullscreen-kiosk → Spotlight
overlay refactor (Rust — deferred until the build loop is healthy).

**Next action that unblocks SEEING any of it:** free host disk (~60–80 GiB), then
build + run `render-desktop.suffix.Dockerfile`.

---

## 1. North star (one paragraph)

A beautifully crafted, OpenAI-styled, **macOS-quality** immutable Linux desktop
that ships with **no apps and no store**. Every surface the user touches is
either (a) a hand-crafted native OS surface we control to a fanatical polish
bar, or (b) a GNOME stock utility *themed to disappear* into the same language.
The headline loop: **describe what you need → Codex (your OpenAI subscription)
or on-device GPT-OSS builds it → it lands in your space.** The OS is the canvas;
the apps are yours.

## 2. Product thesis (non-negotiables)

- **No app store, no bundled user apps.** System *utilities* are themed GNOME
  stock — Files (Nautilus), Terminal (Console), Text editor, Image/PDF viewer
  (Loupe/Papers), System Monitor. Everything beyond that, the user builds.
- **Two engines, one clean choice:** **Codex on the user's OpenAI account**
  (default / cloud) and **GPT-OSS on-device** (local / private). Folds in and
  supersedes the standalone Codex-default goal (see memory `codex-default-builder-goal`).
- **Security contract is preserved:** secrets server-side only, loopback core,
  no client-side secrets, systemd-hardened services (per `SHIP.md`).
- **All-Rust crafted surfaces;** reuse known frameworks/themes (libadwaita,
  Plymouth, GTK4 CSS) — never hacks. Keep `goblins-os-verify` `blocked=0` and CI green.

## 3. Visual north star — "OpenAI × macOS Blend"

The chosen direction: OpenAI's calm near-monochrome palette + Inter typography,
fused with macOS's materials, depth, and fluid motion.

- **Color** — paper/ink token foundation already in `goblins-os-design` (Light/
  Dark/Auto), one restrained accent, calibrated functional status hues. Keep.
- **Type** — Inter (shipped) on a tightened, macOS-like scale; weight/optical
  contrast for hierarchy; tabular numerals where data aligns.
- **Materials & depth (the macOS half)** — translucency/vibrancy on chrome (top
  bar, dock, control center, popovers, lock), layered soft shadows, generous
  corner radii, hairline separators, inset top-edge sheen (already present —
  systematize and extend across all chrome).
- **Motion** — fluid spring / calibrated cubic-bezier transitions; press / hover
  / focus micro-interactions; the build "thinking" pulse; honor reduced-motion.
- **Chrome (macOS feel, OpenAI restraint)** — global **top menu bar**, a **dock**,
  a **control center**, **Spotlight** (the home already is this), refined window
  controls, login/lock.
- **Iconography** — one cohesive monoline system/dock/file-type icon set that
  matches the OpenAI mark.

## 4. Surfaces & scope

**Crafted (ours — fanatical polish bar):**
- **Boot** — Plymouth animated splash (mark on the night gradient), GRUB theme,
  quiet boot. _[NEW — none today]_
- **Installer** — branded Anaconda + our native first-boot wizard (already
  custom); refine art & motion.
- **Onboarding / first boot** — engine choice (**OpenAI account · Codex** vs
  **On-device · GPT-OSS**), the Goblins-native OpenAI provider hero; make it delightful.
- **Login / Lock** — night vibrancy surface (exists); elevate to glass.
- **Desktop shell** — Spotlight home + **dock** + **top menu bar** + **control
  center** + app-detail + session lock. _[dock / menu bar / control center NEW]_
- **Build Studio** — the multi-turn builder; the crown jewel.
- **Settings** — Overview / Models / Policy / Recovery; refine; collapse the
  3-way engine picker to the clean two-path choice.

**Themed (GNOME stock — must read as one OS):**
- Files, Terminal, Text editor, Image/PDF viewer, System Monitor — via
  libadwaita accent + named-color overrides + icon theme + fonts + `dconf`
  defaults, added to the image and the verifier contract.

## 5. Branding / art to own (replace ALL Fedora identity)

- `os-release` / `PRETTY_NAME` / logo → product identity. _[done]_
- Plymouth theme + GRUB theme (GRUB menu entries say "Install Goblins OS 44"). _[done]_
- GDM greeter theme (brief but on-brand) — autologin bypasses GDM (skipped by design).
- Anaconda product branding (sidebar art + white mark + dark accent + title) →
  `os/iso/remaster-anaconda-branding.sh`, verified in qemu. _[done]_
- **Wallpapers** light/dark (signature OpenAI gradient art). _[done]_
- Cursor theme, icon theme, subtle system sounds.
- App / file-type icon set.

Brand source assets present today: `os/brand/OpenAI-*-{monoblossom,wordmark}.{svg,png}`.

## 6. Engine / account model (folds in `codex-default-builder-goal`)

- **Default account + builder = Codex on the user's OpenAI subscription;**
  alternative = **GPT-OSS on-device.**
- Wire Codex → a local GPT-OSS OpenAI-compatible provider so the local path is
  fully offline and keyless.
- **Install the Codex CLI in the image** (`codex_available()` is false on a stock
  image today) and **ship a GPT-OSS runtime systemd unit** (none today).
- Collapse the runtime 3-way picker (`engine_pill`, settings 3 buttons, installer
  CTA) into **Account vs Local**; demote advanced engines.
- Key files: `crates/goblins-os-core/src/{openai_key.rs,resident.rs,codex.rs,
  installer.rs,session_gate.rs}`, the shell/settings/installer GUIs,
  `os/bootc/Containerfile`, and `goblins-os-verify`.

## 7. Engineering guardrails

- Keep `goblins-os-verify` `blocked=0` — *extend* the contract for new units /
  apps / themes; never bypass it. Mind the `no-web-kiosk-packaging-drift` denylist.
- CI green: `cargo fmt`, `clippy -D warnings`, `cargo test`, release; image build
  + selftest; installer-iso.
- Build/verify via **Docker on VM-native Linux storage** (the iCloud / virtio-fs
  EIO trap fakes verifier failures — see memory `goblins-os-build-env`);
  renders via the render container; the real-hardware *feel* is the external
  gate (`SHIP.md`).

## 8. Definition of done — the quality bar

A **surface** is done only when, in **both light and dark**, at the *real
rendered pixels*:
- it reads as one cohesive OpenAI × macOS language (color / type / material / motion);
- correct vibrancy, depth, hairlines, radii; **no off-system hues**, **no stray
  Fedora/GNOME-default chrome**;
- micro-interactions feel fluid (press / hover / focus / transition);
- spacing & alignment hold on the grid; optical balance;
- it survives a Jony-Ive-grade critique with no "obvious" flaw.

The **whole OS** is done when **boot → install → onboard → desktop → build an app
→ use it** is one unbroken, beautiful, on-brand experience with **zero
Fedora/GNOME-default leakage**; `verify blocked=0`; CI green; render proofs
captured light + dark; hardware-gate feel signed off.

## 9. Phased plan

| Phase | Outcome |
|------|---------|
| **P1** Design language v2 | Extend `goblins-os-design` tokens for materials/vibrancy/motion + macOS chrome primitives; re-render existing surfaces to confirm elevation and no regression. |
| **P2** Brand & boot art | Plymouth + GRUB + `os-release` + wallpapers + GDM + icon/cursor themes; replace all Fedora identity. |
| **P3** macOS chrome | Dock + global menu bar + control center in the shell; window/interaction feel. |
| **P4** GNOME stock theming | libadwaita accent / named-colors / icon theme / fonts / `dconf` so utilities match; add to image + verifier contract. |
| **P5** Engine/account convergence | Codex default on OpenAI account; GPT-OSS local; install Codex CLI + GPT-OSS runtime unit; collapse picker. |
| **P6** Polish & sign-off | Per-surface passes to §8; render light+dark; verify/CI green; prep hardware gate. |

## 10. Tracking

This file is the living tracker — update **Status** and the table as phases land.
Render proofs under `os/screenshots/`. Surface-level critique notes append to
`os/signoff-notes.md`.
