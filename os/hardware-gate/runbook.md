# Goblins OS External Sign-off Runbook

Run this on a Linux host with a display-backed VM path available.

Set:

```sh
REPO_ROOT="${REPO_ROOT:-$(pwd)}"
cd "$REPO_ROOT"
```

## 0) Preflight
- Confirm runtime requirements on the host:
  - `docker` (required for the local image build, install ISO, and bootc-image-builder flow)
  - `qemu-system-x86_64` for x86_64 display-backed VM gate
  - `qemu-system-aarch64` plus aarch64 EDK2/AAVMF UEFI pflash code and writable variable store for aarch64 display-backed VM gate
  - `qemu-img` and at least one screenshot capture tool for the host.
  - readable/writable `/dev/kvm`; the display-backed proof uses native KVM acceleration, not architecture emulation.
  - at least 120 GiB free on both the repo filesystem and VM scratch filesystem before building release media; override `MIN_HOST_FREE_GB` only on runners with separately provisioned image/cache capacity.
  - `docker info` returns promptly before starting the build; restart Docker or free host resources if it hangs.
- Confirm repo at `$REPO_ROOT` and you are in that directory.
- Choose a native architecture: `ARCH=x86_64` or `ARCH=aarch64`.
- Choose the real pullable release bootc image ref for that architecture:
  `RELEASE_IMAGE=<registry>/<namespace>/goblins-os:$ARCH`. The Docker-local
  `localhost/goblins-os:$ARCH` handoff is only for artifact testing and cannot
  satisfy shipping proof.
- Run the fail-closed runner preflight before starting the build. This checks the
  native architecture, Docker health, free space, QEMU/KVM, and aarch64 UEFI
  paths when applicable; it does not create shipping artifacts or satisfy proof by itself:
  ```sh
  PREFLIGHT_ONLY=1 GOBLINS_OS_ARCH="$ARCH" REPO_ROOT="$REPO_ROOT" os/hardware-gate/run-external-gate.sh
  ```
- Prepare a writable scratch VM disk if preflight passed and you are not letting
  the helper create it: `qemu-img create -f qcow2 /tmp/goblins-os-$ARCH.qcow2 80G`.

### Docker artifact testing on a non-native machine

For local testing only, Docker Desktop or another Docker engine may be used to
try a non-native artifact build with emulation:

```sh
GOBLINS_OS_ARCH=x86_64 \
RUN_QEMU=0 \
GOBLINS_OS_ALLOW_EMULATED_DOCKER=1 \
MIN_HOST_FREE_GB=120 \
REPO_ROOT="$REPO_ROOT" \
os/hardware-gate/run-external-gate.sh
```

This path is intentionally not release proof. It does not launch the
display-backed VM, cannot satisfy screenshot or signoff rows, and still fails
fast if the Docker emulation backend cannot run the Rust toolchain. Use it only
to debug artifact generation before moving to a native Linux/KVM runner.

## 1) Build installer ISO
```sh
cd "$REPO_ROOT"
ARCH=x86_64 # or aarch64 on a native aarch64 Linux runner
# Optional: clean image cache for deterministic run
docker rmi -f "localhost/goblins-os:$ARCH" localhost/goblins-os:ci || true
docker build -f os/bootc/Containerfile -t "localhost/goblins-os:$ARCH" .
GOBLINS_OS_CONTAINER_RUNTIME=docker \
GOBLINS_OS_ARCH="$ARCH" \
GOBLINS_OS_IMAGE="localhost/goblins-os:$ARCH" \
GOBLINS_OS_BIB_SOURCE_IMAGE="$RELEASE_IMAGE" \
GOBLINS_OS_SHIPPABLE_RELEASE=1 \
os/iso/build-iso.sh
```

Expected outputs:
- `os/iso/output/$ARCH/bootiso/goblins-os-$ARCH.iso`
- `os/iso/output/$ARCH/bootiso/goblins-os-$ARCH.iso.sha256`
- `os/iso/output/$ARCH/manifest-goblins-os-$ARCH.json`

The generated ISO manifest must record `"installer_payload_source_local_only": false`
and `"shippable_release": true`. If it records a Docker-local registry, discard
that ISO for release signoff and rebuild with `GOBLINS_OS_BIB_SOURCE_IMAGE`
pointing at the real release image.

The GitHub `hardware-gate-capture` workflow uses the same release-image rule but
pushes the bootc image directly to GHCR with `docker buildx build --push`, then
runs `os/iso/build-iso.sh` with `GOBLINS_OS_SKIP_LOCAL_IMAGE_BUILD=1`. That
avoids exporting the full bootc image into the runner's local Docker daemon
before bootc-image-builder pulls the real registry source.

## 2) Write ISO + boot display-backed VM
```sh
ARCH=x86_64
ISO="os/iso/output/$ARCH/bootiso/goblins-os-$ARCH.iso"
qemu-system-x86_64 -m 8192 -smp 4 \
  -accel kvm \
  -cdrom "$ISO" \
  -drive file=/tmp/goblins-os-$ARCH.qcow2,if=virtio,format=qcow2 \
  -boot d -vga std -display gtk \
  -serial mon:stdio
```

For aarch64 on a native aarch64 Linux runner:

```sh
ARCH=aarch64
ISO="os/iso/output/$ARCH/bootiso/goblins-os-$ARCH.iso"
AARCH64_UEFI_CODE="${AARCH64_UEFI_CODE:-/usr/share/edk2/aarch64/QEMU_EFI-pflash.raw}"
AARCH64_UEFI_VARS="${AARCH64_UEFI_VARS:-/tmp/goblins-os-$ARCH-uefi-vars.fd}"
AARCH64_UEFI_VARS_TEMPLATE="${AARCH64_UEFI_VARS_TEMPLATE:-/usr/share/edk2/aarch64/vars-template-pflash.raw}"
[ -f "$AARCH64_UEFI_VARS" ] || cp "$AARCH64_UEFI_VARS_TEMPLATE" "$AARCH64_UEFI_VARS"
qemu-system-aarch64 -machine virt,accel=kvm,gic-version=max -cpu host -m 8192 -smp 4 \
  -drive if=pflash,format=raw,readonly=on,file="$AARCH64_UEFI_CODE" \
  -drive if=pflash,format=raw,file="$AARCH64_UEFI_VARS" \
  -cdrom "$ISO" \
  -drive file=/tmp/goblins-os-$ARCH.qcow2,if=virtio,format=qcow2 \
  -boot d -device virtio-gpu-pci -display gtk \
  -serial mon:stdio
```

For headless/debug capture only, remove `-display` and use `-nographic`.

Complete the install flow, reboot into the Goblins OS session, and verify the
first-boot identity/onboarding gate rather than creating an installer-local
password.

## 3) Capture required proof assets (during the run)
Use the host screenshot tool for the live session and save to:

`os/screenshots/hardware-gate/<arch>/<YYYY-MM-DD>/`

Legacy/non-shipping screenshot roots that are not under
`os/screenshots/hardware-gate/<arch>/<YYYY-MM-DD>/` are migration history only.
Do not copy, rename, or re-date them into an architecture root. Reboot the
current ISO in the display-backed VM or hardware path, capture fresh screenshots,
and generate a new `proof-manifest.json` tied to the current ISO and SHA.

Add `proof-manifest.json` beside the screenshots so the proof root is tied to
the release media that was booted:

```json
{
  "architecture": "<arch>",
  "iso": "os/iso/output/<arch>/bootiso/goblins-os-<arch>.iso",
  "iso_sha256": "<64-char sha256 from the matching .sha256 file>",
  "captured_at": "<UTC timestamp>",
  "screenshot_run_dir": "os/screenshots/hardware-gate/<arch>/<YYYY-MM-DD>",
  "firewall_live_toggle_proof": "firewall-live-toggle-proof.json",
  "text_shortcuts_session_enable_proof": "text-shortcuts-session-enable-proof.json"
}
```

`close-signoff.sh` rejects missing, empty, or non-PNG screenshot files and
rejects a manifest that does not match the current architecture ISO and SHA. It
also rejects the run unless `firewall-live-toggle-proof.json` records the live
core route disabling firewalld with HTTP 200 and observed inactive status, then
enabling it with HTTP 200 and observed active status through the scoped systemd
oneshot/polkit bridge.

The same run must include `text-shortcuts-session-enable-proof.json`. That proof
only covers the live session plumbing: active `org.goblins.OS.IBus.service`, the
seeded `goblins-textshortcuts` input source and preload engine, active IBus
engine selection, adapter self-test, and core honesty that runtime expansion is
still gated off. It does not ship Text Shortcuts expansion; the keystroke commit
proof remains a separate qemu gate.

Capture exactly at minimum these names:
1. `01-installer.png` — ISO boot + installer launch
2. `02-install-network.png` — installer network/progress
3. `03-login.png` — login screen
4. `04-desktop.png` — first native desktop session
5. `06-onboarding.png` — first-boot onboarding page
6. `07-home.png` — post-onboarding home
7. `08-shell-home.png` — shell launch
8. `09-shell-dark.png` — shell dark-theme state
9. `10-settings.png` — settings page
10. `11-settings-models.png` — settings models section
11. `12-settings-dark.png` — settings dark-theme state
12. `13-studio-before.png` — Build Studio prompt
13. `14-studio-running.png` — studio running
14. `15-studio-app-detail.png` — built-app detail
15. `16-built-app-open.png` — open built app
16. `17-dark-motion.png` — dark-theme motion/interactions
17. `18-light-motion.png` — light-theme motion/interactions
18. `19-vulkan-vkcube.png` — native Vulkan sample running in the installed session
19. `20-gamemode-active.png` — GameMode activation command result
20. `21-gamescope-session.png` — Gamescope-launched nested session or app
21. `22-mangohud-overlay.png` — MangoHud overlay visible over a user-launched sample
22. `23-controller-detection.png` — connected controller/gamepad detected by the OS
23. `24-audio-output.png` — PipeWire audio sink/output proof while a test sound is playing
24. `25-install-destination.png` — advanced storage Installation Destination showing explicit disk choice
25. `26-install-storage-summary.png` — storage summary showing formatting/root filesystem before writing changes
26. `27-dual-boot-preserve-existing-os.png` — the native installer's Open advanced storage path or the desktop Install Goblins OS Beside Another OS entry, followed by Custom/manual storage or Reclaim Space showing Goblins OS installed into unallocated free space or a dedicated disk while existing Windows, macOS/APFS, Linux, other OS, recovery, and EFI partitions are preserved
27. `28-bootloader-efi-summary.png` — bootloader/EFI target summary before beginning install

Suggested installed-session commands for the gaming screenshots:

```sh
# Native Vulkan sample. Capture the window while it is rendering.
vkcube

# Vulkan/device summary. Useful to keep visible beside vkcube when space allows.
vulkaninfo --summary

# Video acceleration diagnostics. Capture the supported VA-API profile output.
vainfo

# VDPAU wrapper diagnostics. Capture the provider result when a GPU exposes VDPAU.
vdpauinfo

# GameMode activation path. Capture the terminal result.
gamemoded -t || gamemoderun sh -lc 'echo "GameMode launch path executed"; sleep 5'

# Gamescope nested compositor/session. Launch a short sample and capture the window.
gamescope -- vkcube

# MangoHud overlay over a user-launched sample. Capture the overlay text.
mangohud vkcube

# Controller detection. Attach a controller or pass one through to the VM first.
cat /proc/bus/input/devices | rg -i 'gamepad|joystick|controller|xbox|dualsense|dualshock'
lsusb
evtest --query /dev/input/event0 EV_KEY BTN_GAMEPAD || true

# Audio output. Capture sink listing plus audible/signal activity.
wpctl status
pw-cli info 0
pactl list short sinks
speaker-test -t sine -l 1
```

After the run, open [os/signoff-notes.md](os/signoff-notes.md) and fill:
- date/run id
- device/runner + ISO hash
- command used
- release evidence path under `os/signoff-proofs/sbom/<arch>/`
- each check pass/fail and screenshot filenames
- SBOM result, including `release-evidence-manifest.json`, `cargo-lock-packages.tsv`, and `rpm-packages.tsv`
- gaming readiness result, including Steam absence from installed-root verifier
- firewall toggle result, including `firewall-live-toggle-proof.json`
- Text Shortcuts session-enable result, including `text-shortcuts-session-enable-proof.json`
- install destination, formatting/root filesystem, bootloader/EFI, and dual-boot preservation result
- for custom formatting, encryption, separate `/home`, LUKS/LVM, TPM2 LUKS, ext4, or btrfs, show an advanced storage summary before writes
- if dual boot is tested, show the Open advanced storage action or Install Goblins OS Beside Another OS desktop entry, Custom/manual storage or Reclaim Space, the free-space/dedicated-disk target, the backup/free-space preparation note, and the untouched existing OS/recovery/EFI partitions
- if the native installer is used, show that the simple flow proceeds only for a blank disk and routes disks with existing Windows/macOS/APFS/Linux/other OS/recovery/EFI/data partitions to manual storage
- blockers
- verify every required file above exists before marking the run complete

Then validate the local proof set programmatically:

```sh
ARCH=x86_64 # or aarch64
SCREENSHOT_RUN_DIR="os/screenshots/hardware-gate/$ARCH/<YYYY-MM-DD>"
GOBLINS_OS_ARCH="$ARCH" SCREENSHOT_RUN_DIR="$SCREENSHOT_RUN_DIR" ./os/hardware-gate/close-signoff.sh
```

The helper generates source release evidence with `goblins-os-verify
--release-evidence` when the release verifier is available. If the architecture
image exists on the Linux runtime, it also runs `rpm-packages.command` inside the
built image to create `rpm-packages.tsv`. The final shipping gate still fails if
`release-evidence-manifest.json`, `cargo-lock-packages.tsv`, or
`rpm-packages.tsv` is missing for either architecture. The release evidence
manifest must also record `asset_provenance`, `third_party_notices`,
`trademark_posture`, and `source_tree_manifest` paths so acquisition reviewers can
trace each architecture artifact back to the source-package diligence files.
The helper and final shipping gate also run the artifact/evidence secret scan
over generated release evidence, signoff notes, ISO manifests, SHA files,
release tables, and command files. Binary ISO/image payloads and historical
runtime proof dumps are not treated as text scan inputs.

If the helper exits non-zero, fix missing artifacts and rerun.

## 4) Run runtime model path (choose one)
- Preferred: local model path (for example a downloaded GPT-OSS model folder).
- Alternative: BYO OpenAI key.
- Alternative: BYO Codex/session path.

Start a full Build Studio turn and verify:
- app card is created and visible in ledger
- opening it enters built-app detail
- Open in Build Studio works
- user-visible built app artifact appears (and opens)

Document the exact engine and result in [os/signoff-notes.md](os/signoff-notes.md).

## 5) Closed-loop verification on host image artifacts
Use this quick evidence audit first:

```sh
./os/hardware-gate/verify-shipping-status.sh
```

Use this helper first to validate local workflow expectations and run installed-root checks:

```sh
./os/hardware-gate/close-signoff.sh
```

It appends a scaffold run entry into `os/signoff-notes.md` and reports:
- workflow gate presence
- image existence
- ISO presence/hash
- verify blocked=0 result (if image is present)
- self-test container build attempt

From a host with Docker:

```sh
RUNTIME=docker

# Packaging contract
$RUNTIME run --rm localhost/goblins-os:$ARCH \
  /usr/libexec/goblins-os/goblins-os-verify --installed-root / | tee verify.log
grep -q "blocked=0" verify.log

# Self-test pass (installed rootfs)
cat os/bootc/Containerfile os/bootc/selftest.suffix.Dockerfile > /tmp/selftest.Dockerfile
DOCKER_BUILDKIT=1 $RUNTIME build -f /tmp/selftest.Dockerfile --target selftest -t goblins-os:selftest .
```

For CI confirmation, ensure the three workflow jobs complete successfully:
- rust
- image
- installer-iso
