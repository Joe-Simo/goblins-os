# Hardware-gate display-backed-VM capture harness

`os/hardware-gate/run-external-gate.sh` boots a display-backed VM but leaves the
28-shot screenshot capture as a **manual operator checklist** (see its "Next
manual closure steps"). This harness automates that capture so the gate's
`os/screenshots/hardware-gate/<arch>/<date>/{01..28}.png` run can be produced
without a human clicking through every surface.

It is **honest, not fabricated**: every shot is a real QMP framebuffer capture
of the real installed OS running its real binaries in a real VM booted from a
proof ISO built from the same bootc image with `os/iso/verify-config.toml`.
Gaming shots use the OS's own shipped software GPU/audio stack (lavapipe Vulkan,
gamescope, pipewire) — real processes, captured live; only the GPU is software,
which the signoff row states plainly. This is the same display-backed-VM path
`close-signoff.sh:442` explicitly accepts ("from the display-backed VM or
hardware run").

## Pipeline contract

1. **Unattended-install kickstart** is embedded into the proof ISO with
   `GOBLINS_OS_ISO_CONFIG=os/iso/verify-config.toml`, because the generated ISO
   boots Anaconda with `inst.ks=hd:LABEL=GOBLINS_OS:/osbuild.ks`. The release
   ISO keeps `os/iso/config.toml`, which never auto-wipes and still leaves disk
   selection interactive for human installs.
2. Anaconda deploys the embedded OCI, reboots, and **GDM autologin (user
   `goblin`) reaches the live Goblins desktop** — the session gate is a window,
   not a fullscreen lock, so no unlock is needed.
3. The verification-only kickstart installs and globally enables a `goblin` user
   service for the user manager's `default.target`, so it runs inside the real
   GNOME session without relying on a specific VT or GNOME session target name.
   A verification-only system starter waits for the `goblin` user bus and
   explicitly requests that user service, writing serial markers if the bus or
   start request is missing. The helper scripts live under
   `/etc/goblins-os/hardware-gate/` so the verification-only install does not
   try to mutate the image-owned `/usr` tree. The same verification-only script
   is also installed as a GNOME autostart fallback, and the kickstart writes
   direct systemd `*.wants/` symlinks so chrooted `systemctl enable` behavior
   cannot silently drop the proof services. The host serves
   `firstboot-unlock.sh` over the qemu slirp gateway (`http://10.0.2.2:PORT/`),
   then publishes
   `in-session-orchestrator.sh` only after the host screenshot tailer is ready.
   No sshd, guest agent, or keystroke command injection is used.
4. The orchestrator launches each gate surface (`/usr/libexec/goblins-os/*`
   binaries, with `GOBLINS_OS_INSTALLER_PAGE=` for installer pages and the
   `GOBLINS_OS_SYS_BLOCK_DIR` block-device fixtures render-screens.sh uses for the
   dual-boot-preservation state) plus the gaming stack, and signals the host over
   HTTP (`/ready/<shot>`) — no in-guest screenshot tool required.
5. `qmp-capture.py watch` tails the HTTP log and QMP-screendumps each surface to
   `shots/<shot>.png` as it is signalled. After the PNG is durably written, the
   host publishes `/capture-acks/<shot>.captured` with the fully decoded PNG's
   SHA-256 and positive dimensions; the guest keeps the signalled surface
   unchanged until that validated acknowledgement arrives, including while the
   host retries a duplicate framebuffer.
6. The orchestrator also posts live proof signals over the same HTTP channel.
   The firewall proof disables and re-enables firewalld through
   `/v1/firewall/enabled`; the host writes it to
   `firewall-live-toggle-proof.json` and refuses to continue unless the disable
   path returns HTTP 200/inactive and the enable path returns HTTP 200/active.
   The Text Shortcuts session proof writes
   `text-shortcuts-session-enable-proof.json` and only passes when the installed
   GNOME session has the Goblins IBus service, source seed, preload, active
   engine, adapter self-test, and core runtime-honesty signal in place. The live
   Text Shortcuts shipping contract is covered by
   `text-shortcuts-live-ibus-runtime-render-proof.json` plus
   `32-text-shortcuts-live-ibus-runtime-render.png`. In one installed-session
   run it must prove secure private desktop-state write/read/preview/file
   roundtrips, live watcher reload, QMP-keyboard expansion to `on my way.`,
   unknown-word pass-through, zero commit before the accepting boundary, exactly
   one `process-key-event` commit in the boundary slice with exact focused-entry
   readback, and password-field suppression with no commit, candidate, or popup.
   The screenshot must show the chronologically latest native IBus lookup-table
   popup record anchored to the input context, with a positive generation and
   record ordinal. The host must acknowledge that PNG before the guest types the
   accepting boundary; the guest rechecks the same record at capture, then proves
   that the latest popup transitions to `hide-candidate` with reason `committed`.
   The proof rejects a synthetic overlay.

   The candidate metadata, overlay-intent, frame, layout, and render-intent
   self-tests are non-live build-time behavior contracts only. They do not prove
   the installed native popup. The manifest retains them as diagnostic preflight
   attachments, and signoff may record that those checks passed, but they cannot
   satisfy the production popup claim. In particular,
   `31-text-shortcuts-candidate-bubble-render.png` is a synthetic diagnostic
   surface; only screenshot 32 and its native IBus proof count as production UI
   evidence.
7. The host writes `proof-manifest.json` (architecture, iso path, iso_sha256,
   captured_at, screenshot_run_dir, firewall proof filename, Text Shortcuts
   session proof filename, Text Shortcuts live runtime/render proof filename,
   and the exact screenshot 32 SHA-256) and runs `close-signoff.sh`. The live
   proof, manifest, and decoded PNG must all carry the same digest.

## Status

Earlier aarch64 runs proved the display-backed capture orchestration with real
captures of the branded Anaconda install, the installed desktop, settings, the
goblins-os installer review screens, and **real Vulkan via lavapipe (`vkcube`)**.
The current embedded verification-config install path is source-gated only until
a fresh hardware-gate run reaches the installed session and produces the required
proof artifacts. The x86_64 track runs the identical harness on a native x86_64
Linux/KVM host (e.g. the GitHub `ubuntu-24.04` runner) since TCG emulation of
x86_64 on Apple Silicon is too slow for a full session capture.
