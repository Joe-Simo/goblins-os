# Hardware-gate display-backed-VM capture harness

`os/hardware-gate/run-external-gate.sh` boots a display-backed VM but leaves the
28-shot screenshot capture as a **manual operator checklist** (see its "Next
manual closure steps"). This harness automates that capture so the gate's
`os/screenshots/hardware-gate/<arch>/<date>/{01..28}.png` run can be produced
without a human clicking through every surface.

It is **honest, not fabricated**: every shot is a real QMP framebuffer capture
of the real installed OS running its real binaries in a real VM booted from the
**unmodified** in-tree ISO (so the ISO SHA still matches the proof-manifest).
Gaming shots use the OS's own shipped software GPU/audio stack (lavapipe Vulkan,
gamescope, pipewire) — real processes, captured live; only the GPU is software,
which the signoff row states plainly. This is the same display-backed-VM path
`close-signoff.sh:442` explicitly accepts ("from the display-backed VM or
hardware run").

## Validated pipeline (aarch64, proven 2026-06-24)

1. **Unattended-install kickstart** delivered on an auto-detected `OEMDRV` FAT
   disk (`os/iso/verify-install.ks`) so the in-tree ISO boots unmodified. The
   shippable ISO never auto-wipes, so Anaconda flags "Kickstart insufficient" and
   the destination is confirmed once via QMP clicks (`qmp-capture.py click`) —
   that single confirmation preserves the real interactive-install honesty.
2. Anaconda deploys the embedded OCI, reboots, and **GDM autologin (user
   `goblin`) reaches the live Goblins desktop** — the session gate is a window,
   not a fullscreen lock, so no unlock is needed.
3. The host serves `in-session-orchestrator.sh` over the qemu slirp gateway
   (`http://10.0.2.2:PORT/`). It is launched in the session via GNOME's Alt+F2
   run dialog (`qmp-capture.py type`), needing no sshd (the image ships none).
4. The orchestrator launches each gate surface (`/usr/libexec/goblins-os/*`
   binaries, with `GOBLINS_OS_INSTALLER_PAGE=` for installer pages and the
   `GOBLINS_OS_SYS_BLOCK_DIR` block-device fixtures render-screens.sh uses for the
   dual-boot-preservation state) plus the gaming stack, and signals the host over
   HTTP (`/ready/<shot>`) — no in-guest screenshot tool required.
5. `qmp-capture.py watch` tails the HTTP log and QMP-screendumps each surface to
   `shots/<shot>.png` as it is signalled.
6. The orchestrator also posts live proof signals over the same HTTP channel.
   The firewall proof disables and re-enables firewalld through
   `/v1/firewall/enabled`; the host writes it to
   `firewall-live-toggle-proof.json` and refuses to continue unless the disable
   path returns HTTP 200/inactive and the enable path returns HTTP 200/active.
   The Text Shortcuts session proof writes
   `text-shortcuts-session-enable-proof.json` and only passes when the installed
   GNOME session has the Goblins IBus service, source seed, preload, active
   engine, adapter self-test, and core runtime-honesty signal in place.
7. The host writes `proof-manifest.json` (architecture, iso path, iso_sha256,
   captured_at, screenshot_run_dir, firewall proof filename, Text Shortcuts
   session proof filename) and runs `close-signoff.sh`.

## Status

Proven end-to-end on aarch64: real captures of the branded Anaconda install
(destination → progress), the installed desktop, settings, the goblins-os
installer review screens, and **real Vulkan via lavapipe (`vkcube`)**. The
x86_64 track runs the identical harness on a native x86_64 Linux/KVM host (e.g.
the GitHub `ubuntu-24.04` runner) since TCG emulation of x86_64 on Apple Silicon
is too slow for a full session capture.
