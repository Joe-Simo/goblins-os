# Goblins OS Release Engineering

Goblins OS is a Rust-first native Linux (Fedora bootc immutable) desktop OS. The
steps below produce and verify the installable artifacts on Linux, and define
the display-backed gate required for final signoff.

## Shipping decisions (final)

- **Base platform**: Fedora bootc remains the OS foundation. No custom kernel
  ownership work is planned.
- **Font stack**: Inter is the final shipped font stack (`rsms-inter-fonts`
  with `google-noto-sans-fonts` fallback in packaging for compatibility).
- **Typography boundary**: no non-Inter brand font dependency is required or
  shipped.
- **Production architecture**: 64-bit Arm (`aarch64`) is the sole supported
  product and release target.

Image, evidence, and ISO packaging commands assume a native **aarch64 Linux host
with Docker**. Final display capture uses the separate Apple Silicon/HVF route
defined in step 6. The Arm ISO is not compatible with Intel or AMD systems. The `x86_64` files already
published with `v0.1.0-alpha.20260703` are retained only as immutable historical
records; they are not supported, current, or eligible for promotion.

## What CI enforces (`.github/workflows/build.yml`)

- **publisher-boundary** check: every source workflow is read-only, consumes no
  repository/environment secret, contains no registry/tag/Release write path,
  and delegates publication to the separate protected publisher repository.
- **rust** job: `cargo fmt --all --check`, `cargo clippy --workspace --features
  <native-desktop> -- -D warnings`, `cargo test`, and a release build — the
  canonical format/lint/type gate on the native `aarch64` release runner.
- **image** job: builds the Arm bootc image, runs `goblins-os-verify` (must report
  `blocked=0`), runs the install + services **self-test**
  (`os/bootc/selftest.suffix.Dockerfile`; a non-zero result fails the build), and
  renders the design-proof screenshots.
- **installer-iso** job: builds the installable Arm ISO with
  `bootc-image-builder`: `goblins-os-aarch64.iso`.

> Non-Linux development hosts can run a useful subset of source checks. The
> native-desktop build and installer packaging authority is native aarch64
> Linux; the separate final display authority is Darwin/arm64 Apple Silicon
> with HVF. Neither route substitutes for the other.

## Secrets & provisioning (server-side only)

The stock public image, release workflow, screenshots, and canonical release
proof use **no maintainer-owned or provider credential**. Each installed user
chooses on-device GPT-OSS, signs in to their own OpenAI account through the
bundled Codex CLI, or adds their own OpenAI API key at runtime through the
protected per-user credential window.

Optional enterprise or self-hosted deployments may supply an operator-managed
OpenAI account or assistant-route secret in
`/etc/goblins-os/openai-secrets.env` (shipped empty, mode `0600 root:root`).
systemd (PID 1) copies it into `goblins-os-core`'s private runtime credential
directory with `LoadCredential=`; the core reads named values directly from that
file, never from its process environment. No desktop user or group can read it,
and generic core subprocesses receive a closed, non-secret environment. It is
**never** sourced into the desktop session — the world-readable
`/etc/goblins-os/environment` holds non-secret config only, and the client GUIs
receive readiness booleans and opaque storage labels, never tokens or credential
paths.

Encrypted-at-rest provisioning uses the same runtime contract. An operator can
replace the plaintext unit directive with this drop-in after creating the
encrypted payload with `systemd-creds`; no application configuration changes:

```ini
[Service]
LoadCredential=
LoadCredentialEncrypted=openai-secrets.env:/etc/credstore.encrypted/goblins-os-core.service/openai-secrets.env
```

The credential payload keeps the existing literal `NAME=VALUE` format and key
names. Matching outer quotes remain accepted for migration compatibility, but
shell expansion is not supported. Direct secret injection through service
environment variables is intentionally unsupported.

## ChatGPT web and compatible Codex Linux app boundary

The ChatGPT and Codex launchers first pass the existing core health, session,
and cloud-policy gates. ChatGPT opens only the fixed official
`https://chatgpt.com` surface. Codex invokes only `/usr/bin/chatgpt` with the
fixed `codex:` deep link when a compatible app is separately installed; only a
missing executable falls back to `https://chatgpt.com/codex`. Unknown service
states and other spawn failures fail closed. Every upstream-app or web-handler
command that Goblins starts uses an empty environment plus a reviewed allowlist
of desktop runtime variables and a fixed `PATH`, so that command inherits no
unrelated keys, tokens, credentials, proxy settings, loader hooks, or
service-owned `CODEX_HOME`.

Goblins installs a higher-priority hidden `chatgpt.desktop` association for the
`codex:` scheme that routes ordinary desktop links through the Goblins wrapper.
This protects OS-owned desktop entry and link flows; a user who manually invokes
a separately installed third-party executable remains outside the Goblins
launcher boundary. The compatible app owns its own per-user sign-in session.
Build Studio remains a separate Goblins OS surface with its existing GPT-OSS,
OpenAI account through Codex, and user-supplied API-key engine choices.

The public Goblins OS source, image, ISO, and release assets do not embed,
extract, modify, rehost, or redistribute OpenAI's proprietary RPM. The package
audited on 2026-08-12 was an `aarch64` runtime but also contained non-runtime
foreign-architecture native dependency prebuilds, so it was not eligible for
the literal Arm-only image contract. Re-audit every future upstream package;
do not treat this time-specific result as a permanent product claim. A local
RPM layer is not offered: it would make the bootc deployment locally modified
and would pin that package instead of preserving the OS and app update
contracts. Do not expose an install button until a supported,
transactional bootc-native lifecycle can download and verify OpenAI's bytes on
the user's machine, rebuild on both OS and app updates, stage a rollback-capable
derived deployment, and remove it cleanly. Compatible Codex launch routing is
real; publisher identity and bundled installation are intentionally not claimed.

## 1. Build the OS image

```sh
ARCH=aarch64
DOCKER_BUILDKIT=1 docker build -f os/bootc/Containerfile -t "localhost/goblins-os:$ARCH" .
```

This compiles the Rust workspace, assembles the Fedora-bootc image (GNOME session,
the native desktop surfaces and supporting system tools, the core daemon, and
systemd units), and runs `bootc container lint` as the final layer. A clean build
means the image is well-formed.

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

Produces deterministic diagnostic renders from the real application binaries
(installer, login, shell home, Build Studio, settings, disk-install flow, and
built-app detail) in light and dark. These are visual-regression fixtures, not
installed-session or display-backed release evidence; only step 6 can satisfy
that gate.

## 4. Generate the exact-candidate handoff and evidence

The public source repository does not publish containers. Dispatch
`candidate-artifacts.yml` for the exact current `main` commit. It builds and
verifies the native ARM64 image locally, then uploads four hash-sealed OCI
payload parts plus a metadata envelope and release evidence. Artifact names,
schemas, checksums, and the independent publisher verification contract are in
[`os/release/PUBLISHER-BOUNDARY.md`](os/release/PUBLISHER-BOUNDARY.md).

The protected `Joe-Simo/goblins-os-publisher` repository must authenticate that
exact source run/attempt, reassemble and verify the OCI archive, import it with
digest preservation, and prove that the public registry digest equals the
source handoff digest. Only its resulting metadata may be used as the
pullable-candidate input below.

```sh
set -euo pipefail

ARCH=aarch64
CANDIDATE_COMMIT="$(git rev-parse HEAD)"
CANDIDATE_REF_JSON="<downloaded protected-publisher candidate metadata>/image-ref.json"
jq -e --arg arch "$ARCH" --arg commit "$CANDIDATE_COMMIT" \
  '.architecture == $arch
   and .candidate_commit == $commit
   and .candidate_tag_authoritative == false
   and .non_promotional == true
   and (.immutable_image_ref | test("^ghcr\\.io/.+@sha256:[0-9a-f]{64}$"))' \
  "$CANDIDATE_REF_JSON" >/dev/null
IMAGE_REF="$(jq -er '.immutable_image_ref' "$CANDIDATE_REF_JSON")"
cargo run -p goblins-os-verify -- \
  --source-root . \
  --release-evidence "os/signoff-proofs/sbom/$ARCH" \
  --arch "$ARCH" \
  --candidate-commit "$CANDIDATE_COMMIT" \
  --image-ref "$IMAGE_REF"
```

Generate and download `CANDIDATE_REF_JSON` from the exact protected publisher
import run documented in the hardware-gate runbook. The source handoff's
`intended_immutable_image_ref` is a required expected value, not proof that the
image was published. Do not copy a mutable channel or commit-scoped tag into
this field; the independently verified registry digest is the evidence identity.

This source invocation can prepare diagnostic evidence and an
`rpm-packages.command`, but it cannot satisfy final release evidence. For the
current release architecture, run the packaged
`goblins-os-verify --release-evidence` from the
exact digest-pinned Goblins OS image so one invocation writes the v5 manifest,
Cargo inventory, architecture-specific RPM inventory, and RPM replay command.
The final gate requires the Cargo TSV, RPM TSV, and `rpm-packages.command`
SHA256 values to match that manifest; a standalone replay of
`rpm-packages.command` is diagnostic only. A host without `rpm` records a
`rpm-packages.not-generated.txt` blocker instead of inventing package data.
Generated release evidence, ISO manifests, SHA files, signoff notes, release
tables, and command files must also pass the artifact/evidence secret scan
before the hardware gate accepts them.

The ISO manifest, release-evidence manifest, screenshot proof manifest, and
signoff row must all record this same full candidate commit and immutable image
digest reference. The `aarch64` track must match its digest-bound media before
stable promotion; current Arm evidence without either field is intentionally
incomplete.

## 5. Build the installer ISO

```sh
GOBLINS_OS_CONTAINER_RUNTIME=docker \
GOBLINS_OS_ARCH="$ARCH" \
GOBLINS_OS_IMAGE="localhost/goblins-os:$ARCH" \
GOBLINS_OS_CANDIDATE_COMMIT="$CANDIDATE_COMMIT" \
os/iso/build-iso.sh
# uses privileged bootc-image-builder in Docker
```

Uses the supported `bootc-image-builder --type anaconda-iso` (config in
`os/iso/config.toml`). This local Docker path is for artifact proof: it pushes
the just-built image through a Docker-local registry so bootc-image-builder can
embed it. The builder rejects non-Arm hosts and non-Arm container engines;
architecture emulation is not an artifact or release path. A local macOS ARM64
build is diagnostic only and does not satisfy native Linux packaging or
display-backed proof gates. Final shippable media must build only in the
protected publisher repository on a native aarch64 Linux runner from the
immutable pullable release image ref with `GOBLINS_OS_SHIPPABLE_RELEASE=1` and
`GOBLINS_OS_BIB_SOURCE_IMAGE=<registry>/<image>@sha256:<64-hex-digest>`, because the
Anaconda ISO records that source ref for post-install bootc tracking. It also
requires the exact protected-publisher branding-tool record through
`GOBLINS_OS_INSTALLER_BRANDING_PUBLISHER_EVIDENCE` and its matching digest ref
through `GOBLINS_OS_INSTALLER_BRANDING_IMAGE`; the checked-in schema-1 record is
diagnostic only. The ISO embeds the image and opens Goblins OS advanced storage for disk selection. Storage is interactive: no
`clearpart`/`autopart` command is baked into the kickstart, so the person must
explicitly choose the target disk, review formatting, and confirm the
bootloader/EFI target before writes happen. Keeping another operating system or
data uses advanced storage with existing system, APFS/data, recovery, and EFI
partitions preserved; Custom/manual storage or Reclaim
Space must make the choice visible before any write. The safe dual-boot
path is to back up first, create unallocated free space from the OS being kept
when possible, then install Goblins OS into that free space or a dedicated disk
while leaving existing system, APFS/data, recovery, and EFI partitions
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
people keeping Windows, Linux, another OS/data partition, or a dedicated
existing disk. Each path states what to do before install, where Goblins OS
should be installed, what must stay unformatted, and how to choose between
operating systems from the firmware boot picker after install. A structured
**Dual-boot decision map** renders concise Windows, Linux, APFS/other data, and
separate-disk rows with best-fit guidance, space-preparation steps, the safe
install target, preserved partitions, and the post-install startup picker check
so the user can pick the correct path before disk selection. The native
installer also shows **Dual-boot readiness** for Windows/Linux/other OS or data
paths: back up and prepare space in the OS being kept when possible, pick
`Keep my current OS` or manual storage, install only into unallocated free space
or a dedicated Goblins OS disk, and confirm both systems boot before changing
boot order. Before any simple-flow write, the installer shows a **Before writing to disk** plan covering
the selected blank disk, fresh GPT layout, bootloader/EFI target, xfs root
filesystem, manual-storage handoff for custom formatting/encryption, and
firmware boot-picker recovery path. Outputs:
- `os/iso/output/aarch64/bootiso/goblins-os-aarch64.iso`
- matching `.sha256` files and the `manifest-goblins-os-aarch64.json` manifest.

## 6. Boot and install (the display-backed gate)

Attach the verification-only `aarch64` ISO to a UEFI aarch64 VM **with a real
display path**. The required local proof uses `qemu-system-aarch64` with HVF on
Apple Silicon. Apple Silicon is the proof host, not a claimed bare-metal install
target. No bare-metal Arm model is supported until that exact model completes
the install, input, graphics, network, audio, suspend, update, rollback, and
recovery matrix. Then:

1. Boot the ISO → choose the disk/storage layout in advanced storage.
   For preservation-flow proof, back up first, use a controlled virtual-disk
   fixture with existing system/APFS/data partitions, then use Installation
   Destination → Custom/manual storage or Reclaim Space.
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
- `os/screenshots/hardware-gate/aarch64/<run-date>/` (screenshots)
- `os/signoff-notes.md` (step-by-step checklist + timestamps)
- `os/hardware-gate/runbook.md` (reproducible command flow used)
Generated release evidence and ISO metadata are scanned for live keys before
signoff. Source-repository OCI handoffs are non-promotional and cannot satisfy
this gate by themselves.

This step cannot run in the headless build sandbox. It is one required external
gate and needs Apple Silicon/HVF with a display-backed VM. Native aarch64 Linux
packaging, exact-candidate artifact binding, capture-host signature, runtime app-build
proof, accessibility evidence, and coherent signoff must also pass.

## External verification gates

- **Apple Silicon/HVF VM boot + interaction feel** — step 6 above. Packaging and
  source gates are automated separately; smoothness, state fidelity, and input
  behavior must be judged on the real display path for the exact candidate.
- **Typography** — the shipped font stack is final and Inter-only (with Noto
  Sans fallback).
- **A runtime model** — canonical release proof exercises actual app generation
  with downloaded GPT-OSS. It never requests, reads, validates, or uses a
  maintainer's or user's API key or OpenAI account. Installed users may
  separately choose their own API key or OpenAI account through Codex at runtime;
  the GUI + core build path remains complete and honest once their selected
  engine is ready.
