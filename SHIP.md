# Goblins OS Release Engineering

Goblins OS is an all-Rust, native Linux (Fedora bootc immutable) desktop OS. The
steps below produce and verify the installable artifacts on Linux, and define
the display-backed gate required for final signoff.

## Shipping decisions (final)

- **Base platform**: Fedora bootc remains the OS foundation. No custom kernel
  ownership work is planned.
- **Font stack**: Inter is the final shipped font stack (`rsms-inter-fonts`
  with `google-noto-sans-fonts` fallback in packaging for compatibility).
- **Typography boundary**: no non-Inter brand font dependency is required or
  shipped.

All commands assume a native **x86_64 or aarch64 Linux host with Docker**. Release
artifacts are architecture-specific; do not treat one ISO as covering both CPU
families. CI runs the same native matrix — see `.github/workflows/build.yml`.

## What CI enforces (`.github/workflows/build.yml`)

- **rust** job: per-architecture `cargo fmt --all --check`, `cargo clippy --workspace --features
  <native-desktop> -- -D warnings`, `cargo test`, and a release build — the
  canonical format/lint/type gate on native x86_64 and aarch64 runners.
- **image** job: builds the bootc image per architecture, runs `goblins-os-verify` (must report
  `blocked=0`), runs the install + services **self-test**
  (`os/bootc/selftest.suffix.Dockerfile`; a non-zero result fails the build), and
  renders the design-proof screenshots.
- **installer-iso** job: builds architecture-named installable ISOs with
  `bootc-image-builder`: `goblins-os-x86_64.iso` and `goblins-os-aarch64.iso`.

> Non-Linux development hosts can run a useful subset of source checks, but the
> native-desktop build, installer, and display-backed proof paths are
> authoritative on native Linux runners.

## Secrets & provisioning (server-side only)

The image bakes in **no credentials**. Operators supply OpenAI account / relay
secrets in `/etc/goblins-os/openai-secrets.env` (shipped empty, mode `0600
root:root`), which is read **only** by root — systemd (PID 1) loads it into the OS
services (`goblins-os-core`/`-resident`/`-model-cache`) via their `EnvironmentFile`
before dropping to the service user, so no desktop user or group can read it.
It is **never** sourced into the desktop session — the world-readable
`/etc/goblins-os/environment` holds non-secret config only, and the client GUIs
receive booleans and file paths from the core, never tokens.

## 1. Build the OS image

```sh
ARCH=x86_64 # or aarch64 on a native aarch64 Linux host
DOCKER_BUILDKIT=1 docker build -f os/bootc/Containerfile -t "localhost/goblins-os:$ARCH" .
```

This compiles the Rust workspace, assembles the Fedora-bootc image (GNOME session,
the four native apps, the core daemon, systemd units), and runs `bootc container
lint` as the final layer. A clean build = the image is well-formed.

## 2. Verify the packaging contract

```sh
docker run --rm "localhost/goblins-os:$ARCH" /usr/libexec/goblins-os/goblins-os-verify
```

Expect the final line to end with `blocked=0`; the check total grows as the
packaging contract adds coverage. Any
`blocked` > 0 means a required binary / unit / .desktop / session / state-dir /
secret file is missing — fix before shipping.

## 3. Render the design proofs (optional, for review)

```sh
cat os/bootc/Containerfile os/bootc/render.suffix.Dockerfile > /tmp/render.Dockerfile
mkdir -p screenshots
DOCKER_BUILDKIT=1 docker build -f /tmp/render.Dockerfile \
  --target screenshots --output type=local,dest=screenshots .
```

Produces the genuine first-boot/desktop screens (installer, login, shell home,
Build Studio, settings, the disk-install flow, the built-app detail view) in light
and dark — the actual installed pixels, not mockups.

## 4. Generate release and SBOM evidence

```sh
ARCH=x86_64 # or aarch64
cargo run -p goblins-os-verify -- \
  --source-root . \
  --release-evidence "os/signoff-proofs/sbom/$ARCH" \
  --arch "$ARCH"
```

This writes `release-evidence-manifest.json`, `cargo-lock-packages.tsv`, and an
`rpm-packages.command` for the built image. Run the same command inside the
installed Goblins OS image, or run `rpm-packages.command` there, to capture
architecture-specific RPM names, versions, architectures, and license tags in
`rpm-packages.tsv`. If the command is run on a host without `rpm`, it records a
`rpm-packages.not-generated.txt` blocker instead of inventing package data.
Generated release evidence, ISO manifests, SHA files, signoff notes, release
tables, and command files must also pass the artifact/evidence secret scan
before the hardware gate accepts them.

## 5. Build the installer ISO

```sh
GOBLINS_OS_CONTAINER_RUNTIME=docker \
GOBLINS_OS_ARCH="$ARCH" \
GOBLINS_OS_IMAGE="localhost/goblins-os:$ARCH" \
os/iso/build-iso.sh
# uses privileged bootc-image-builder in Docker
```

Uses the supported `bootc-image-builder --type anaconda-iso` (config in
`os/iso/config.toml`). This local Docker path is for artifact proof: it pushes
the just-built image through a Docker-local registry so bootc-image-builder can
embed it. On a development host whose Docker engine supports both platforms,
non-release artifact testing may set `GOBLINS_OS_ALLOW_EMULATED_DOCKER=1` and
`GOBLINS_OS_DOCKER_PLATFORM=linux/amd64` or `linux/arm64`; that only fills local
ISO/SHA/manifest evidence and does not satisfy the native runner or screenshot
proof gates. Final shippable media must instead build on a native runner from the
real pullable release image ref with `GOBLINS_OS_SHIPPABLE_RELEASE=1` and
`GOBLINS_OS_BIB_SOURCE_IMAGE=<real release bootc image ref>`, because the
Anaconda ISO records that source ref for post-install bootc tracking. The ISO embeds the image and opens Goblins OS advanced storage for disk selection. Storage is interactive: no
`clearpart`/`autopart` command is baked into the kickstart, so the person must
explicitly choose the target disk, review formatting, and confirm the
bootloader/EFI target before writes happen. Dual boot with Windows, macOS,
Linux, or another OS uses advanced storage with existing system, recovery, and EFI partitions
preserved; Custom/manual storage or Reclaim
Space must make the choice visible before any write. The safe dual-boot
path is to back up first, create unallocated free space from the OS being kept
when possible, then install Goblins OS into that free space or a dedicated disk
while leaving Windows, macOS/APFS, Linux, other OS, recovery, and EFI partitions
untouched unless the user is intentionally replacing that OS. The native live
installer presents the decision as three paths: **Keep my current OS** for dual
boot through advanced storage, **Replace one blank disk** for the guarded
whole-disk simple flow, and **Advanced storage** for encryption, ext4, btrfs,
separate `/home`, resized partitions, or mixed disks. Users keeping another OS
get an **Install beside an existing OS** route with a direct **Open advanced storage**
action in the storage screen, and the live desktop exposes **Install Goblins OS Beside Another OS**
for the same manual-storage handoff. That route requires a final summary showing
the Goblins OS target, every filesystem that will be formatted, every preserved
partition, and the bootloader/EFI target
before writes happen. The simple flow is
blank-disk, whole-disk erase only: if it detects existing OS, recovery, EFI, or
data partitions, it protects that disk from the simple flow and points the user
to advanced storage. Whole-disk erase still requires a
typed device-specific acknowledgement. The simple flow uses xfs on a fresh GPT
layout; ext4, btrfs, separate `/home`, resized free space, encryption, TPM2
LUKS, LUKS/LVM, and any custom partitioning stay in advanced storage where
the formatting, mount points, bootloader/EFI target, and
preserved partitions are visible before writing. The installer also exposes a
**Dual-boot assistant** for
people keeping Windows, macOS, Linux, another OS/data partition, or a dedicated
existing disk. Each path states what to do before install, where Goblins OS
should be installed, what must stay unformatted, and how to choose between
operating systems from the firmware boot picker after install. A structured
**Dual-boot decision map** renders concise Windows, macOS, Linux, other OS/data,
and separate-disk rows with best-fit guidance, space-preparation steps, the safe
install target, preserved partitions, and the post-install startup picker check
so the user can pick the correct path before disk selection. The native
installer also shows **Dual-boot readiness** for Windows/macOS/Linux/other OS
paths: back up and prepare space in the OS being kept when possible, pick
`Keep my current OS` or manual storage, install only into unallocated free space
or a dedicated Goblins OS disk, and confirm both systems boot before changing
boot order. Before any simple-flow write, the installer shows a **Before writing to disk** plan covering
the selected blank disk, fresh GPT layout, bootloader/EFI target, xfs root
filesystem, manual-storage handoff for custom formatting/encryption, and
firmware boot-picker recovery path. Outputs:
- `os/iso/output/x86_64/bootiso/goblins-os-x86_64.iso`
- `os/iso/output/aarch64/bootiso/goblins-os-aarch64.iso`
- matching `.sha256` files and `manifest-goblins-os-<arch>.json` manifests.

## 6. Boot and install (the real-hardware gate)

Write the matching architecture ISO to a USB stick, or attach it to a VM **with a
display** (`qemu-system-x86_64` for x86_64, `qemu-system-aarch64` with UEFI for
aarch64), then:

1. Boot the ISO → choose the disk/storage layout in advanced storage.
   For dual boot, back up first, create free space from Windows/macOS/Linux when
   possible, then use Installation Destination → Custom/manual storage or Reclaim Space.
   Install into unallocated free space or a dedicated disk and preserve existing
   system, recovery, and EFI partitions. For single-OS installs, confirm the
   whole-disk layout on a blank disk; disks with existing partitions must show
   the preservation/manual-storage path or an intentional replacement decision.
2. Reboot → the native **first-boot onboarding** appears.
3. Pick an engine: **GPT-OSS** (downloads the on-device model), **your OpenAI API
   key**, or **Codex** (your OpenAI account).
4. On the home, describe an app → the engine builds it → it appears in the ledger →
   click it → the **built-app detail view** → **Open in Build Studio**.
5. Confirm the **motion/interaction feel**: press states, hover, transitions, the
   thinking pulse, Light/Dark/Auto switching live with the desktop preference.

Capture and save evidence for this exact run in:
- `os/screenshots/hardware-gate/<arch>/<run-date>/` (screenshots)
- `os/signoff-notes.md` (step-by-step checklist + timestamps)
- `os/hardware-gate/runbook.md` (reproducible command flow used)
Generated release evidence and ISO metadata are scanned for live keys before signoff.

This step cannot run in the headless build sandbox — it is the **only remaining
external gate** for full sign-off, and it requires a machine or display-backed VM.

## External verification gates

- **Real-hardware/VM boot + interaction feel** — step 5 above. Everything up to it
  is automated and verified; the perceived smoothness of motion can only be judged
  on a real display.
- **Typography** — the shipped font stack is final and Inter-only (with Noto
  Sans fallback).
- **A runtime model** — exercising *actual* app generation needs GPT-OSS downloaded
  (or a BYO key / Codex configured). The GUI + core build path is complete and
  honest; generation runs once an engine is present.
