# Goblins OS — Roadmap

> **Vision.** A macOS-grade desktop that is exceptional, beautiful, and *ours* —
> Goblins-branded surfaces built **on** GNOME, never a fork. Fedora owns security
> and hardware; we own the experience. Every feature below is a verified,
> implementation-ready spec.

## How we build

- **CI-validated batches.** Land a batch → the **image gate** runs `fmt`/`clippy`/
  `cargo test` (host-side pure-Rust logic), then **render** (light+dark screenshots)
  and **qemu** confirm the live surface → only then the next batch. Never trust a
  prior "green"; re-run the gate fresh (per signoff lessons).
- **One design system.** Every custom surface is built on `crates/goblins-os-design`
  tokens — one accent (`@gos_accent`), one radius scale, one motion curve, the
  consolidated status-tone system. No ad-hoc px/hex, no second hue, no SF Pro
  (Inter only). Marks/icons ship as **PNG** (fedora-bootc:44 has no gdk-pixbuf SVG
  loader).
- **Honest gating, always.** A control never reports success when its model/device/
  schema is absent. Reuse the allowlisted core bridges (`accessibility.rs` /
  `notifications.rs` / `voice.rs` pattern): probe capability, degrade to a calm
  read-only/explained state, never fabricate.
- **Packages only via the Containerfile.** New RPMs go in **both** the `dnf install`
  list **and** the `rpm -q` verify block (`os/bootc/Containerfile`) so a wrong name
  fails the build loudly — never silently.
- **Host vs. CI split.** Core logic (`crates/*` pure Rust) unit-tests on the macOS
  dev host; all GTK / gnome-shell / portal / live-engine behavior is `cfg(linux,
  native-desktop)` and is provable **only** in CI/qemu.

**Status legend:** `TODO` · `in-progress` · `shipped`. Shipped items move to
`GO-LIVE.md` (Part 2 backlog) — this file tracks what's still open.

---

## ⏩ Session status — RESUME HERE (updated 2026-07-01)

Head `b5ddaca` is CI-green for the fast Rust build: GitHub Actions build run
`28549840219` passed on x86_64 and aarch64. Hardware-gate run `28549856369` at
that head passed bootc image publish, verification installer ISO build, model
prep, install/first boot, and the display-backed VM orchestrator. The artifact
contains the new screenshot set, `31-text-shortcuts-candidate-bubble-render.png`,
`32-text-shortcuts-live-ibus-runtime-render.png`, and all runtime proof JSONs
except `proof-manifest.json` because close-signoff failed before manifest write.

That failure is now narrowed to a stale proof contract, not to the runtime engine
itself. `text-shortcuts-live-ibus-runtime-render-proof.json` passed with
`normal_actual=onmyway.`, `passthrough_actual=hello.`, `password_refusal=true`,
`focused_field_callback=true`, `text_input_v3_commit=true`,
`rendered_accept_bubble=true`, `runtime_ready_claim=true`, and
`core_readiness_flip=deferred`. The older shallow
`text-shortcuts-live-keystroke-proof.json` still remained mandatory and failed at
`stage=normal-readback` with `normal_actual=missing` and
`normal_file_bytes=0`. That legacy proof is superseded by the stronger
runtime/render proof because the stronger proof covers normal expansion,
pass-through, password refusal, focused-field callback, Wayland text-input-v3
commit, and the rendered accept bubble in one installed GNOME/Wayland session.

The current source follow-up is qemu-pending and does **not** mark more features
shipped. It retires the superseded `text-shortcuts-live-keystroke-proof.json`
from the mandatory hardware signoff path while keeping
`text-shortcuts-live-ibus-runtime-render-proof.json` and
`32-text-shortcuts-live-ibus-runtime-render.png` required. The gate must still
fail closed unless the runtime/render proof records active `goblins-textshortcuts`
IBus engine selection, QMP keyboard input, normal expansion, pass-through,
password refusal, focused-field callback, text-input-v3 commit, rendered accept
bubble, Inter styling, and `core_readiness_flip=deferred`.

Previous hardware-gate run `28538674596` at `9f5ca8b` passed bootc image
publish, verification installer ISO build, model prep, install, first boot
diagnostics, the `%post` marker, and in-session orchestrator startup, but failed
on `text-shortcuts-live-keystroke`,
`text-shortcuts-live-ibus-runtime-render`, and `multi-display-apply`. The
follow-up at `2493d53` cleared those proof rows.

The latest reviewed display-backed proof artifact before the current close-
signoff work was run `28530031330` at `d540563`. It passed bootc image publish,
verification installer ISO build, model prep, and the in-session proof routes
that existed at that head. The proof JSONs for firewall live toggle, keyboard
shortcut rebind, input source roundtrip, Focus arm/disarm, app-keyed Per-app
Privacy revoke, Preview PDF/image open/render, and Text Shortcuts through live
IBus runtime/render all reported `status=pass`;
`32-text-shortcuts-live-ibus-runtime-render.png` exists. That run is **not** a
shipping signoff: `run-capture.sh` failed the duplicate surface honesty guard
with only 28/35 distinct captures, several installer captures were stale
desktop frames because the installer exited after completed first boot, Switch
Control was visible over unrelated screenshots, and the run predates
`multi-display-apply-proof.json`.

Previous hardware-gate run `28517091135` at `fffea01` passed bootc image
publish, verification installer ISO build, model serve, install/boot/session
automation, and most display-backed VM proof routes, but still failed before the
final Text Shortcuts live runtime render. The artifact has real display captures
through `31-text-shortcuts-candidate-bubble-render.png`; `29-preview-pdf-open.png`
and `30-preview-image-open.png` exist, but
`32-text-shortcuts-live-ibus-runtime-render.png` is still absent.

Run `28517091135` proved the firewall live toggle, app-keyed Per-app Privacy
revoke, keyboard shortcut rebind, input source switching, Focus arm/disarm,
Preview PDF/image open/render, Text Shortcuts candidate metadata, overlay
intent, deterministic candidate bubble frame/layout, deterministic candidate
bubble render proof routes, and active `goblins-textshortcuts` selection for the
basic live-keystroke path still pass. The remaining blocker then was concrete:
`text-shortcuts-session-enable-proof.json` and
`text-shortcuts-live-ibus-runtime-render-proof.json` failed at
`stage=user-service` with `bus_owner=true`,
`daemon_process=/usr/bin/ibus-daemon --panel disable`, but `service=inactive`
for the custom Goblins IBus unit. The live-keystroke proof no longer hit the GTK
`Unknown option --text-shortcuts-proof` parser failure; it reached
`stage=normal-readback`, proving the flag-strip fix worked and narrowing that
failure back to session IBus ownership/readiness.

Previous hardware-gate run `28510121319` at `118402b` passed bootc image
publish, verification installer ISO build, model serve, install/boot/session
automation, and most display-backed VM proof routes, but failed before the final
Text Shortcuts live input/runtime render path. That run narrowed the prior
blocker to `text-shortcuts-session-enable-proof.json` failing at
`stage=engine-list` with `service=active`, `input_source_configured=true`,
`preload_configured=true`, `engine_listed=false`, and
`list_error="Can't connect to IBus."`.

Previous hardware-gate run `28506688097` at `82f50ef` proved the earlier IBus
session follow-up once: `text-shortcuts-session-enable-proof.json` was `pass`
with `service=active`, `engine_listed=true`, `engine_set=pass`,
`active_engine=goblins-textshortcuts`, and `adapter_self_test=pass`. That run
narrowed the remaining live failure to Text Shortcuts input:
`text-shortcuts-live-keystroke-proof.json` failed at `stage=normal-readback`
with `normal_actual=missing`, and
`text-shortcuts-live-ibus-runtime-render-proof.json` failed at
`stage=render-ledger` with `focused_field_callback=false`,
`text_input_v3_commit=false`, `rendered_accept_bubble=false`, and no screenshot.

Previous hardware-gate run `28496717503` at `729ea69` on `main` proved the App
Privacy follow-up worked: `app-privacy-revoke-proof.json` was `pass` with
`PermissionStore.SetPermission` seed/readback, `/v1/app-privacy/revoke` HTTP
`200`, `PermissionStore.DeletePermission`, `post_revoke_absent=true`, and prior
state restored. It also narrowed the firewall failure to re-enable state
readback and the Text Shortcuts failure to IBus bus readiness.

Previous hardware-gate run `28493666844` at `52f0403` on `main` passed the fast
Rust build on both `ubuntu-24.04` and `ubuntu-24.04-arm`, then failed inside the
display-backed VM capture after the installed session reached `/ready/ORCH_START`
and `/ready/ORCH_ALLDONE`. That run proved the IBus component/cache follow-up
once (`text-shortcuts-session-enable-proof.json` was `pass` with
`engine_listed=true`, `engine_set=pass`, and
`active_engine=goblins-textshortcuts`) but App Privacy revoke and firewall
re-enable still failed.

Previous hardware-gate run `28486096503` at `00b0950` on `main` passed the bootc
image build and verification ISO build, then failed inside the display-backed VM
capture after the installed session reached `/ready/ORCH_START` and
`/ready/ORCH_ALLDONE`. It first proved Preview PDF/image screenshots existed but
still missed `32-text-shortcuts-live-ibus-runtime-render.png`.

Previous hardware-gate run `28485602839` at `5ec6b27` failed during the bootc
image build before qemu because the in-image verifier checked
`/usr/lib/tmpfiles.d/goblins-os-session.conf` before the Containerfile copied
`os/tmpfiles/goblins-os-session.conf`. Follow-up commit `00b0950` moved that
`COPY` before the verifier `RUN`, and `28486096503` proved the image/ISO stages
are unblocked again.

Previous qemu-reaching hardware-gate proof run `28483079955` at `4b54b1` on
`main` failed inside the display-backed VM capture after the image push,
verification ISO, model prep, installed boot, first-boot diagnostics, private
unlock helper download, root session-orchestrator starter, GNOME autostart
fallback path, host publish of `/orchestrator.sh`, HTTP `200` orchestrator
download, one `/ready/ORCH_START`, and `/ready/ORCH_ALLDONE` all ran. The
single-instance orchestrator lock fixed the competing proof execution path: the
launcher still fetches `/orchestrator.sh` twice, but only one orchestrator run
enters the proof sequence. The blocker is no longer CI billing, image export,
ISO build, GDM autologin, first-boot helper availability, session orchestrator
startup, or duplicate orchestrator execution.

Previous hardware-gate run `28463342553` at `5aec4a1` failed after the image
push, verification ISO, model prep, installed boot, first-boot diagnostics,
private unlock helper download, root session-orchestrator starter, GNOME
autostart fallback path, host publish of `/orchestrator.sh`, and HTTP `200`
orchestrator download all ran. That run exposed the duplicate
`/ready/ORCH_START` race that `4b54b1` addressed.

Previous hardware-gate run `28457665098` at `e1127ad` failed before signoff, but
it proved the earlier `/etc` helper-location fix. The image push, verification
ISO, model prep, installed boot, first-boot diagnostics, private unlock helper
download, root session-orchestrator starter, GNOME autostart fallback path,
deferred host publish of `/orchestrator.sh`, and HTTP `200` orchestrator
download all ran. The artifact includes real display captures through
`28-bootloader-efi-summary.png` plus
`31-text-shortcuts-candidate-bubble-render.png`, and the serial/http logs include
`GOBLINS_HWGATE_ETC_HELPERS_INSTALLED`,
`GOBLINS_HWGATE_DIAG_DONE`,
`GOBLINS_HWGATE_SESSION_ORCHESTRATOR_START_REQUESTED`, and the
`/orchestrator.sh` `200` fetch.

Hardware-gate run `28447129095` at `304010e` proved GitHub Actions billing is
unblocked, the bootc image build can publish to GHCR, the verification installer
ISO can build from that published ref, and the installed deployment reaches the
Goblins desktop with the first-boot UI visible. It still failed before signoff:
`/firstboot-unlock.sh` never appeared in `httpd.log`, `httpd.log` was empty, the
first-boot diagnostics marker `GOBLINS_HWGATE_DIAG_DONE` did not appear, and the
system-starter marker `GOBLINS_HWGATE_SESSION_ORCHESTRATOR_START_REQUESTED` did
not appear. The artifact includes only debug frames; `_debug-first-boot-desktop.png`
shows the live desktop/first-boot windows, and `_debug-first-boot-vt-f7-final.png`
confirms the session remained on VT7 after failure diagnostics. This narrows the
blocker to verification-only first-boot/session automation not starting, rather
than billing, image export, ISO build, GDM autologin, or GNOME session launch.

Hardware-gate run `28450854194` at `3fab445` proved the image push and
verification ISO again, booted the installed deployment into the Goblins
desktop, and reproduced the first-boot/session automation failure after the
direct wanted-symlink plus GNOME autostart fallback. No new session markers
appeared: `GOBLINS_HWGATE_DIAG_DONE`,
`GOBLINS_HWGATE_SESSION_ORCHESTRATOR_START_REQUESTED`,
`GOBLINS_HWGATE_SESSION_ORCHESTRATOR_STARTED`, and
`GOBLINS_HWGATE_FIRSTBOOT_HELPER_DOWNLOADED` were all absent; `httpd.log`
remained empty and `/firstboot-unlock.sh` was never fetched. The post-install
markers still appeared in the serial log and the desktop still rendered, so the
current blocker is narrowed to verification-only helper files/units not being
available to the installed booted deployment, not CI billing, image export, ISO
build, GDM autologin, or GNOME session launch.

The previous source follow-up is now qemu-tested: writing/asserting the IBus
system registry cache and using `ibus write-cache` fixed engine listing and
activation; prewriting deterministic Text Shortcuts candidate proof files fixed
the candidate-render proof; the PermissionStore `SetPermission` and firewall
firewalld start/stop follow-ups were not sufficient. Local gates for that pass:
hardware-gate shell syntax, `git diff --check`, fmt check for `goblins-os-shell`
and `goblins-os-verify`, focused shell/verify tests, workspace clippy with
`-D warnings`, workspace tests, Rust 1.88 GTK container clippy for
`goblins-os-shell` with `native-desktop`, and
`cargo run -p goblins-os-verify -- --source-root .` → **blocked=0 (2671)**.

Batch 5 btrfs `/home` + snapshots has its first source-only substrate in this
tree: `GET /v1/snapshots/status`, fail-closed
`POST /v1/snapshots/restore`, snapper machine-output parsing, mountinfo-based
btrfs `/var/home` detection, Fedora 44 `btrfs-progs`/`libbtrfsutil` package
lockstep, and verifier/shipping-status exact-string gates. It does **not** flip
the installer root filesystem, install snapper configs/timers, render Settings
UI, create snapshots, or restore files. Local gates for this pass:
`cargo fmt -p goblins-os-core -p goblins-os-verify -- --check`,
`cargo fmt --all -- --check`, `cargo test -p goblins-os-core snapshots` (4
passed), `cargo test -p goblins-os-core` (287 passed),
`cargo test -p goblins-os-verify` (19 passed),
`cargo clippy --workspace -- -D warnings`, `cargo test --workspace`,
`git diff --check`, `bash -n os/hardware-gate/verify-shipping-status.sh`, and
`cargo run -p goblins-os-verify -- --source-root .` →
**blocked=0 (2680)**. Full `verify-shipping-status.sh` still fails on missing
complete architecture ISO/screenshot/signoff artifacts; the new snapshots source
check passes there, but release proof is still incomplete.

Batch 5 FileVault-style encryption now has its first source-only posture
substrate in this tree: `GET /v1/security/encryption`, `/proc/self/mountinfo`
root mapping, `/etc/crypttab` parsing, read-only `cryptsetup status` and
`systemd-cryptenroll --list` probing, Secure Boot and TPM device posture, Fedora
44 `cryptsetup`/`tpm2-tss` package lockstep, and verifier/shipping-status
exact-string gates. It does **not** enable install-time encryption, mint or
escrow recovery keys, enroll TPM tokens, edit crypttab, change the installer
block setup, or flip any root filesystem. Package verification found
`systemd-cryptsetup` is not a Fedora 44 RPM name; the image gates
`systemd-cryptenroll` as a command from the Fedora `systemd` package instead.
Local gates for this pass: `cargo fmt -p goblins-os-core -p goblins-os-verify`,
`cargo fmt --all -- --check`, `cargo test -p goblins-os-core encryption` (5
passed), `cargo test -p goblins-os-core` (292 passed),
`cargo test -p goblins-os-verify` (19 passed),
`cargo clippy --workspace -- -D warnings`, `cargo test --workspace`,
`git diff --check`, `bash -n os/hardware-gate/verify-shipping-status.sh`, and
`cargo run -p goblins-os-verify -- --source-root .` →
**blocked=0 (2693)**. Full `verify-shipping-status.sh` still fails on missing
complete architecture ISO/screenshot/signoff artifacts; the new encryption
source check passes there, but release proof is still incomplete.

CI/qemu image proof is green for run `28287964440` at `7c8c76d`: both `image`
jobs passed the cache-only bootc build, in-image packaging verifier, self-test,
design screenshot render, desktop screenshot render, and artifact uploads on
`x86_64` and `aarch64`. Inspected artifacts:
`goblins-os-screenshots-{x86_64,aarch64}` (110 PNGs each, matching file sets) and
`goblins-os-desktop-screenshots-{x86_64,aarch64}` (18 PNGs each, matching file
sets; includes App Exposé, Hot Corner, Snap Assist, Mission Control, Spaces,
Switcher, and HUD light/dark captures). Pixel samples were nonblank. The
workflow's installer ISO jobs are still a separate long-running proof and do
not mark Batch 5 shipped.

**Reusable capabilities now in place** (use these — don't reinvent):
- **GTK container loop** — `git archive HEAD | tar -x -C /tmp/gob-build`, then a
  `rust:1.88` + `libgtk-4-dev` container (cached `target/` + a `gob-cargo-registry`
  volume) runs `cargo clippy -p <crate> --features <crate>/native-desktop -- -D warnings`.
  Per run: `apt-get update` before install; format `goblins-os-markup` with the
  **container's** rustfmt 1.88, never host. (See memory `goblins-gtk-container-build-loop`.)
- **System-gschema plumbing** — drop a `*.gschema.xml` in `os/glib-schemas/`; the
  Containerfile already COPYs that dir to `/usr/share/glib-2.0/schemas/` and runs
  `glib-compile-schemas`. (Used by Focus, Switch Control, Today.)
- **Shell-JS path** — `node --check` for syntax, `glib-compile-schemas --dry-run` for
  the extension schema, dconf conflict grep — then push (render is qemu-only).
- **Web-verify** — `WebSearch`/`WebFetch` confirm Fedora-44 package names + D-Bus
  shapes before any Containerfile/D-Bus change (did seahorse + the PermissionStore).

**Done so far (25 of 26 features advanced):**
- **Batch 1 (Bucket A) — complete:** Live Text/OCR (core+handoff+markup Copy Text),
  Color picker. *(IME read+list also shipped; Preview viewer package/default
  app wiring and Fingerprint package/status substrate are source-gated.)*
- **Batch 2 (shell) — shipped with CI/qemu render proof:** App Exposé, Hot
  Corners, Snap Assist.
- **Batch 3 (Settings surfaces) — all 9 have a shipped read/status/UI surface:**
  Accessibility rows, Firewall, Keyboard shortcuts, Focus (substrate+gschema),
  Migration (substrate), Multi-display (read side via `displays.rs`), Personal
  Hotspot, Per-app privacy, Keychain. **Gated WRITES still qemu-pending** for
  multi-display apply only. Firewall toggle, IME/input-source switching, Focus
  arm/disarm, keyboard rebind, and app-keyed Per-app Privacy revoke now have
  live display-backed VM proof.
- **Batch 4 (engines) — 7 of 7 SUBSTRATES shipped (cores only; UI/engines deferred):**
  Text Shortcuts, Voice Control, Visual Look Up, Switch Control, Widgets/Today,
  Sound Recognition, Live Captions.
- **Batch 5 (storage/security) — 2 of 2 source substrates started:** btrfs
  `/home` + snapshots now has a read-only core status/restore-fail-closed
  substrate and package gates; FileVault-style encryption now has a read-only
  posture endpoint and package gates. Neither is **shipped**; install-layout
  changes, encryption at install, recovery-key escrow, timers, Settings UI,
  real snapshot creation/restore, and boot/reboot proof remain CI/qemu-pending.

**Current local feature pass:** Firewall toggle substrate + Settings binding are
implemented and locally gated, but the feature remains `in-progress` until the
CI/qemu image pass proves the GTK render, polkit oneshot path, and live toggle.
Local proof: `cargo fmt --all --check`, `cargo clippy --workspace -- -D warnings`,
`cargo test --workspace`, `goblins-os-verify --source-root .` →
**blocked=0 (1553)**, `git diff --check`, helper `bash -n`, polkit rule
`node --check` via a temporary `.js` copy, and the Rust 1.88 GTK container
`cargo clippy -p goblins-os-settings --features goblins-os-settings/native-desktop -- -D warnings`.
The installed-image self-test now exercises `/v1/firewall/status` and the
`/v1/firewall/enabled` POST with an honest-success/honest-failure assertion;
the local aarch64 Docker bootc `selftest` target passes with the expected
non-systemd honest 502 firewall-toggle degradation. The local aarch64 Docker
`settings-interactions` render target now captures the Security firewall switch
before click and after the real `/v1/firewall/enabled` failure/revert path. The
image workflow now has a source-gated explicit `[image]` push marker so the
CI/qemu image proof can be started when manual workflow dispatch is unavailable
in the local tool session; unmarked pushes still run only the fast Rust gate, and
installer ISO artifacts remain manual-only. The first opt-in CI run
(`28289894898`) proved the aarch64 image build and packaging verifier, then
exposed a BuildKit overlay-depth failure before the installed self-test script
could run; the CI-only self-test/render suffixes now collapse their chmod/script
execution into fewer layers, and the collapsed local aarch64 Docker `selftest`
target passes. CI image proof is now green for run `28290845730` at `a97f164`:
Rust, image build, packaging verifier, installed self-test, standard design
screenshots, Settings interaction screenshots, and desktop-proof screenshots
passed on both `x86_64` and `aarch64`; `installer-iso` was skipped as intended.
Inspected artifacts: `goblins-os-screenshots-*` (110 PNGs each),
`goblins-os-settings-interactions-*` (6 PNGs each, including
`118-settings-firewall-before.png` and `119-settings-firewall-toggle-failed.png`),
and `goblins-os-desktop-screenshots-*` (18 PNGs each) all had matching
cross-arch file sets and nonblank pixel samples. This proves the CI GTK render,
installed self-test, and honest failure/revert interaction path; it does **not**
prove the live systemd/polkit oneshot success path, so Firewall remains
`in-progress` until live POST + polkit toggle proof is green. The display-backed
VM capture harness now fail-closes on `firewall-live-toggle-proof.json`: inside
the installed session it posts disable/enable to `/v1/firewall/enabled`, requires
HTTP 200 plus `/v1/firewall/status` inactive/active observations, writes the
proof beside the screenshot run, ties it into `proof-manifest.json`, and makes
`close-signoff.sh`/`verify-shipping-status.sh` reject runs without it. This
source/harness gate is local-only so far; no live VM run has proved it yet. The
first hardware-gate dispatch for that live proof (`28291639868` at `f2b29ae`)
completed the Containerfile build/lint path but failed during local Docker image
export (`#78 exporting layers`, exit 143, runner shutdown) before the VM capture
step, so it produced no `firewall-live-toggle-proof.json`. The hardware-gate
workflow now builds the bootc image with `docker buildx build --push` directly to
GHCR, then calls `os/iso/build-iso.sh` with
`GOBLINS_OS_SKIP_LOCAL_IMAGE_BUILD=1` and `GOBLINS_OS_BIB_SOURCE_IMAGE` so
bootc-image-builder pulls the registry image without exporting the full bootc
image into the runner daemon. That unblock is source-gated only; Firewall still
requires the next display-backed run to prove the live POST + polkit oneshot
success path. The CI speed pass is source-gated too: `build.yml`,
`hardware-gate-capture.yml`, and `release.yml` now use
`docker/setup-buildx-action@v3` plus a nonblocking per-arch
`type=gha,scope=goblins-os-bootc-${{ matrix.arch }}` BuildKit cache for expensive
bootc image builds; the hardware gate also cancels superseded manual runs by
ref/date. This does **not** make the installed OS faster and does **not** prove
Firewall shipped; it only reduces repeated CI/image-build work on later runs.
Hardware-gate run `28295478507` at `d9354b0` proved the action-based image push
and shippable ISO build, then failed at display-backed VM startup with `QMP never
came up` before any in-guest firewall POST ran. The current local fix prepares
readable/writable `/dev/kvm` for the GitHub runner, makes the Linux harness
fail before QEMU unless KVM is readable/writable, and prints `qemu.log`,
`serial.log`, and `httpd.log` on nonzero capture exits so a failed QMP startup
is diagnostic instead of opaque. This fix is source-gated only so far; Firewall
remains `in-progress` until a fresh hardware-gate run produces a passing
`firewall-live-toggle-proof.json`. The same local pass fixed release-check
plumbing exposed while validating the change: generated artifact secret scans now
prefilter only active secret assignments and real-length OpenAI key shapes before
the existing fail-closed validator, the BuildKit cache checks escape GitHub
expressions correctly under shell `eval`, and the selftest status check matches
the current Buildx `target: selftest` workflow. Local source gates for this
pass: scoped `git diff --check` over the changed files, `cargo fmt --all --check`,
`bash -n` for the capture and release-check scripts, `python3 -m py_compile` for
the capture driver, YAML parse for the edited workflows, fake-key
positive/negative artifact secret-scan checks, `cargo clippy --workspace -- -D warnings`,
`cargo test --workspace`, and `goblins-os-verify --source-root .` →
**blocked=0 (1558)**. `verify-shipping-status.sh`
now completes locally but remains **FAIL** on the known release-proof blockers:
the stale aarch64 BIB manifest local-ref row, missing complete aarch64/x86_64
hardware-gate screenshot runs, and missing complete signoff rows.
`systemd-analyze verify` is not available on this macOS host.

Current implementation continuation: the IME/input-source **set** and CJK
package substrates are now source-gated but not shipped. Core exposes
`/v1/input/sources`, validates the existing configured sources with a narrow
`xkb`/`ibus` allowlist, encodes the `a(ss)` GVariant, and honestly fails when
gsettings or the schema/key is absent. Settings ▸ Keyboard adds source-row
Move up / Move down / Remove controls against that route; the last source
cannot be removed. The current CJK package pass web/container-verified Fedora 44
`ibus-libpinyin`, `ibus-anthy`, `ibus-hangul`, and the existing `ibus-gtk4`
module, installs/asserts those engine packages in the bootc image, asserts the
IBus component XML files and engine binaries, and adds a pure core engine-package
registry plus read-only Settings package readiness rows. This pass does not add
a source picker, change IME environment defaults, restore `Super+Space`, or
claim live candidate/input switching. Local source gates for the current package
pass: Fedora 44 clean install probe for the CJK RPMs and paths, targeted
`cargo test -p goblins-os-core input`, targeted
`cargo test -p goblins-os-settings input_source`, `cargo fmt --all --check`,
`cargo clippy --workspace -- -D warnings`, `cargo test --workspace`,
`goblins-os-verify --source-root .` → **blocked=0 (1912)**, scoped
`git diff --check`, `bash -n os/hardware-gate/verify-shipping-status.sh`, and
the Rust 1.88 GTK container
`cargo clippy -p goblins-os-settings --features goblins-os-settings/native-desktop -- -D warnings`
from a clean temp Rust workspace. The current hardware-gate continuation requires
`input-sources-roundtrip-proof.json`: inside the installed session the harness
saves the current GNOME input-source list/current index, posts
`/v1/input/sources` with deterministic `xkb/us` plus `xkb/gb`, verifies gsettings
read-back, seeds current index `0`, posts `/v1/input/switch-next`, verifies the
current index becomes `1`, and restores the original source list/current index.
This is source-gated proof plumbing only; GTK render, menu-bar indicator,
candidate window, CJK switching, and reviewed qemu proof remain pending. Local
source gates for the current proof-hook pass: `python3 -m py_compile` for the
capture driver, `bash -n` for the capture and shipping scripts, scoped
`git diff --check`, `cargo fmt --all --check`,
`cargo clippy --workspace -- -D warnings`, `cargo test --workspace`, and
`goblins-os-verify --source-root .` → **blocked=0 (2183)**.

Current deferred-write proof gate hardening: `verify-shipping-status.sh` now
classifies a hardware-gate screenshot run as complete only when the keyboard
shortcuts roundtrip proof and input-source roundtrip proof JSONs pass, and its
missing-proof diagnostics now print those files explicitly. `goblins-os-verify`
pins those checks inside the relevant shell functions so a stray validator name
elsewhere in the script cannot mask drift. This is source-gated only; no live
qemu run has produced those proof artifacts yet.

Current hardware-gate unblock: run `28365113728` at `22de4b1` built and pushed
the bootc image, produced the verification installer ISO, launched QEMU, and
reached the installer initrd framebuffer, but failed before the kickstart `%post`
marker: serial output stopped after GRUB and the final debug frame sat at
`initrd-switch-root.service`. The local follow-up under validation makes the
verification-only kickstart explicitly `text --non-interactive` and drops the
QEMU default to two vCPUs (`GOBLINS_OS_QEMU_CPUS` can override) to match the CI
KVM warning. This is source-gated only until a fresh hardware-gate run produces
the installed-session proof JSONs and screenshots.

Current hardware-gate follow-up: run `28370034444` at `4181bfb` proved the
verification installer now completes (`GOBLINS_VERIFY_INSTALL_DONE`) and boots
the installed `Goblins OS (ostree:0)` deployment, but the framebuffer reached a
TTY login prompt instead of the GNOME/GDM session, so the driver typed the
orchestrator launch into `fedora login:` and all HTTP proof signals were missing.
Run `28374392709` at `95c4509` proved the image push, verification ISO, and
kickstart install again, but `_debug-first-boot-desktop.png` and
`_debug-post-first-boot-dismiss.png` still showed `fedora login:` on tty1 after
the graphical-target-only fix, and the proof JSONs were all missing. Run
`28382483951` at `2847fc3` proved the display-manager symlink pins made it into
the image build, but the first-boot framebuffer still showed tty1 and the
in-session proof callbacks were still all missing. Run `28400106080` at
`893be68` proved the image push and verification ISO again, then captured the
new VT probe frames: `tty1`, `tty2`, and legacy `tty7` all showed text login
prompts, not a hidden GNOME/GDM session; `httpd.log` stayed empty and every
in-session proof callback was still missing. The local fix under validation now
adds a verification-only first-boot diagnostics service through
`os/iso/verify-config.toml`; it prints the installed default target, GDM/display-
manager symlinks, unit status, and GDM/logind journal to serial before timeout.
Run `28404614220` at `2853f7e` proved the diagnostic path: before first boot,
the verification install had `/etc/systemd/system/default.target ->
multi-user.target` while the image-owned
`/usr/lib/systemd/system/default.target -> graphical.target` and both
display-manager symlinks pointed at GDM. The local fix under validation now
restores `graphical.target` in the verification-only `%post` after Anaconda's
text-install override and asserts that `/etc/systemd/system/default.target`
points back to `graphical.target`. Run `28409192428` at `463ca56` then proved
the restore marker (`GOBLINS_HWGATE_GRAPHICAL_TARGET_RESTORED`), built the
image and ISO, booted the installed deployment into the real GNOME/Goblins OS
desktop, and captured first-boot UI frames instead of tty prompts. It still
failed because the coordinate-based first-boot dismissal clicked through the
visible Welcome UI, blanked the display, and never launched the in-session
orchestrator; `httpd.log` stayed empty and every proof JSON was missing. The
local fix under validation now serves a verification-only `firstboot-unlock.sh`
helper through the same host HTTP server as the orchestrator, runs it from the
guest via Alt+F2, completes the real `/v1/privacy`, `/v1/installer/complete`,
and `/v1/session/unlock` core contracts for the private local path, waits for a
`/ready/FIRSTBOOT_UNLOCK` callback, and only then launches the proof
orchestrator. Run `28411988211` at `a290bf3` proved that helper path is
source-gated but failed before launch because the VT probe ended on the GDM
login surface (`tty2`) instead of the live Goblins session; `httpd.log` stayed
empty and the `/ready/FIRSTBOOT_UNLOCK` callback never arrived. The local fix
under validation now keeps the diagnostic VT frames but returns to `tty1`, where
the same run captured the live session, before sending Alt+F2 automation. Run
`28413878856` at `88daf9f` proved that the run still lands on the live GNOME
overview after the `tty1` return, but the helper was never downloaded: the host
`httpd.log` stayed empty and `/ready/FIRSTBOOT_UNLOCK` was still missing. The
local fix under validation now exits the overview before each Alt+F2 command and
fails earlier unless the host log proves `/firstboot-unlock.sh` was fetched. Run
`28415844049` at `dc01b1c` proved that the VM is on the GDM password surface
after leaving the overview; the helper download frame showed the password prompt
and `httpd.log` stayed empty. Run `28417768014` at `d1d346d` then failed the same
way after the first account-state follow-up. Run `28419767730` at `8227538`
proved the canonical GDM autologin config and timed-login fallback were present
and that autologin did reach the live Goblins desktop; the failure came later,
because the diagnostic VT probe ended on the GDM user picker before the Alt+F2
helper launch. The local fix under validation now keeps the VT diagnostics but
returns to `tty2` (`first boot vt f2 final`) before first-boot helper and
orchestrator automation. This is source-gated only until a fresh hardware-gate
run reaches the real desktop and produces the installed-session proof JSONs.

Current session bridge continuation: core desktop writes now prefer a
session-owned Unix-socket bridge before falling back to direct host `gsettings`.
The user service `org.goblins.OS.SessionBridge.service` owns
`/run/goblins-os-session/session-bridge.sock`; tmpfiles creates the parent
directory as `0770 goblin:goblins-session-bridge` so the system core service can
traverse it through the scoped `goblins-session-bridge` group only. The bridge
allowlists the GNOME/Goblins schemas already surfaced by Settings input,
Accessibility, Focus, Notifications, keyboard shortcuts, and the Preview open
handoff, rejects arbitrary schemas/control-character values, and runs
`xdg-open` inside the desktop user session for `/v1/preview/open`. This targets
the previous input-source readback, keyboard shortcut 503, Focus activate, and
Preview open/render failures. It is still source-gated until qemu proves live
readback, viewer processes, and screenshot artifacts.

Current IME menu-bar indicator continuation: the active-source indicator render
hook is now source-gated. The `goblins-menubar` shell extension binds the stable
GNOME `org.gnome.desktop.input-sources` `sources/current` keys, hides the
indicator when fewer than two sources are configured, hides rather than guessing
if the current source cannot be read, and renders a compact abbreviation chip for
known XKB/IBus sources using the canonical Goblins shell accent. The desktop
render harness now seeds deterministic `xkb/us` + `xkb/gb`, switches to the
second source, captures `59-menubar-input-source-$suffix.png`, then restores the
single-source state. This does not add a source picker, change IME environment
defaults, restore `Super+Space`, or claim live candidate/input switching; pixel
proof and live switching remain CI/qemu-pending.

Current IME add-source continuation: the **Add input source…** surface is now
source-gated. Core exposes `/v1/input/source`, re-reads the current GNOME
`sources` list, intersects installed CJK engine packages with `ibus list-engine`,
omits already-configured sources, and rejects any requested source that is not
both installed and reported by the live IBus runtime. Settings ▸ Keyboard renders
an add section driven only by core-provided choices and posts to the narrow add
route. This does not change IME environment defaults, restore `Super+Space`, or
claim live source switching/candidate-window behavior; render and live switching
remain CI/qemu-pending. Local source gates: targeted
`cargo test -p goblins-os-core input`, targeted
`cargo test -p goblins-os-settings input_source`, `cargo fmt --all --check`,
`cargo clippy --workspace -- -D warnings`, `cargo test --workspace`,
`goblins-os-verify --source-root .` → **blocked=0 (1999)**, scoped
`git diff --check`, `bash -n os/hardware-gate/verify-shipping-status.sh`, and
the Rust 1.88 GTK container
`cargo clippy -p goblins-os-settings --features goblins-os-settings/native-desktop -- -D warnings`.

Current IME Super+Space continuation: the shortcut ownership conflict is now
source-gated without blindly restoring GNOME's stock switcher. The seeded
`Super+Space` custom key still launches Goblins' own binary, now with
`--super-space`; the launcher first posts to core `/v1/input/switch-next`, and
core rotates `org.gnome.desktop.input-sources current` only when more than one
source is configured and the current index is reported clearly. With one source,
missing gsettings, a missing `current` key, or an out-of-range index, the launcher
opens as before. The stock GNOME `switch-input-source` bindings remain empty to
avoid a double owner; live switching remains CI/qemu-pending. Local source
gates: targeted `cargo test -p goblins-os-core input`, targeted
`cargo test -p goblins-os-launcher super_space`, `cargo fmt --all --check`,
`cargo clippy --workspace -- -D warnings`, `cargo test --workspace`,
`goblins-os-verify --source-root .` → **blocked=0 (2004)**, `git diff --check`,
`bash -n os/hardware-gate/verify-shipping-status.sh`, and the Rust 1.88 current
worktree GTK container
`cargo clippy -p goblins-os-launcher --features goblins-os-launcher/native-desktop -- -D warnings`.

Current Accessibility magnifier continuation: the optional magnifier zoom/lens
controls are now source-gated but still CI/qemu-pending for GTK render and live
GSettings writes. Core exposes the `org.gnome.desktop.a11y.magnifier`
`mag-factor` and `lens-mode` keys through the existing
`/v1/accessibility/preference` allowlist, clamps zoom to 1.0x-8.0x in 0.25x
steps, and rejects zoom/lens writes unless the desktop reports
`screen-magnifier-enabled=true`. Settings ▸ Accessibility adds a Magnifier
controls subsection that shows a clear read-only message until Magnifier is on
and the magnifier schema/keys are present. This pass does **not** claim the
rendered GTK row layout or live GNOME magnifier behavior. Local source gates:
targeted `cargo test -p goblins-os-core accessibility`, targeted
`cargo test -p goblins-os-settings accessibility`, `cargo fmt --all --check`,
`cargo clippy --workspace -- -D warnings`, `cargo test --workspace`,
`goblins-os-verify --source-root .` → **blocked=0 (2007)**, `git diff --check`,
`bash -n os/hardware-gate/verify-shipping-status.sh`, and the Rust 1.88 current
worktree GTK container
`cargo clippy -p goblins-os-settings --features goblins-os-settings/native-desktop -- -D warnings`.

Current Hot Corners Settings continuation: the Settings chooser is now
source-gated but still CI/qemu-pending for GTK render and live GSettings writes.
Core exposes `/v1/window-management/status` and
`/v1/window-management/hot-corner`, reads only the existing
`org.goblins.shell.extensions.wm` hot-corner keys, validates the four corner ids
plus the existing `none`/`mission-control`/`app-expose` action registry, and
returns a read-only message when the Goblins WM schema/session is absent.
Settings ▸ Multitasking now shows a Hot corners subsection with four chooser
rows driven by core status. This pass does **not** claim rendered GTK layout,
barrier geometry, or live shell dispatch behavior. Local source gates: targeted
`cargo test -p goblins-os-core window_management`, targeted
`cargo test -p goblins-os-settings hot_corner`, `cargo fmt --all --check`,
`cargo clippy --workspace -- -D warnings`, `cargo test --workspace`,
`goblins-os-verify --source-root .` → **blocked=0 (2015)**, `git diff --check`,
`bash -n os/hardware-gate/verify-shipping-status.sh`, and the Rust 1.88 current
worktree GTK container
`cargo clippy -p goblins-os-settings --features goblins-os-settings/native-desktop -- -D warnings`.

Current Focus continuation: Focus arm/disarm/tick is now source-gated but not
shipped. Core exposes `/v1/focus/activate`, `/v1/focus/deactivate`, and
`/v1/focus/tick`; validates configured mode JSON, snapshots/restores global
notification banners through the shared notifications bridge, records whether
Focus was armed by a schedule, and makes the tick path leave manual Focus modes
alone. The system gschema now includes `armed-by-schedule`, `restore-banners`,
and reserved `restore-apps` keys. The current continuation adds the user-session
`org.goblins.OS.FocusTick.{service,timer}` plus `/usr/libexec/goblins-os/goblins-os-focus-tick`,
which posts only to a local `/v1/focus/tick` core URL on `OnCalendar=minutely`;
the Goblins session target wants the timer, and the image asserts the helper and
unit files. Local source gates for the current timer pass:
`python3 -m py_compile os/focus/goblins-os-focus-tick`, local-core guard smoke,
Fedora 44 `systemd-analyze verify` for the service/timer (unit contents staged
inside the container to avoid macOS bind-mount deadlock), `cargo fmt --all --check`,
`cargo clippy --workspace -- -D warnings`, `cargo test --workspace`, scoped
`git diff --check`, `bash -n os/hardware-gate/verify-shipping-status.sh`, and
`goblins-os-verify --source-root .` → **blocked=0 (1922)**. Settings/Control
Center/menu-bar surfaces, per-app breakthroughs, and live qemu write proof remain
deferred.

Current Focus Settings continuation: Settings ▸ Notifications now fetches
`/v1/focus/status` and exposes a Focus section with a real active-mode chooser
over `/v1/focus/activate` and `/v1/focus/deactivate`. The surface never creates
sample/default modes; when the schema reports no configured modes, it stays
read-only and says so. This is source-gated only: GTK render, live Focus writes,
timer behavior, Control Center render proof, schedules, mode CRUD, and per-app
breakthroughs remain CI/qemu-pending.
Local source gates for this pass: targeted `cargo test -p goblins-os-settings focus`,
targeted `cargo test -p goblins-os-core focus`, `cargo fmt --all --check`,
`cargo clippy --workspace -- -D warnings`, `cargo test --workspace`,
`goblins-os-verify --source-root .` → **blocked=0 (2019)**,
`bash -n os/hardware-gate/verify-shipping-status.sh`, and the Rust 1.88 GTK
container `cargo clippy -p goblins-os-settings --features
goblins-os-settings/native-desktop -- -D warnings` from a minimal temp workspace.
The verifier regular-file reader now uses bounded positional reads for cached
contains checks, and the source secret scan uses `rg` candidate discovery plus
the existing Rust line rules, preserving coverage while avoiding the macOS file
read stalls hit on small source/release files.

Current Focus menu-bar continuation: the `goblins-menubar` extension now binds
the stable `org.goblins.os.focus` schema, watches `active-mode`/`modes`, hides
when Focus is off, hides rather than guessing when the active id is not in the
configured mode list, and shows only the configured active Focus mode name. The
chip opens Settings ▸ Notifications and performs no writes. The desktop render
harness now seeds a deterministic `work` Focus mode, sets it active, captures
`59b-menubar-focus-$suffix.png`, and restores Focus to off. This is source-gated
only: pixel proof, live active-mode changes, timer behavior, and live Focus
writes remain CI/qemu-pending. Local source gates for this pass:
`node --check os/gnome-shell-extensions/goblins-menubar@goblins.os/extension.js`,
Fedora 44 container `glib-compile-schemas --dry-run`, `cargo fmt --all --check`,
`cargo clippy --workspace -- -D warnings`, `cargo test --workspace`,
`cargo test -p goblins-os-verify`, `goblins-os-verify --source-root .` →
**blocked=0 (2024)**, `bash -n os/hardware-gate/verify-shipping-status.sh`, and
scoped `git diff --check`.

Current Focus Control Center continuation: Control Center now fetches
`/v1/focus/status` and adds a read-only Focus tile that reflects only
core-reported configured modes. It shows active/off/unavailable state honestly,
hides no core failures behind a fake value, opens Settings ▸ Notifications for
changes, and does **not** call `/v1/focus/activate` or
`/v1/focus/deactivate`. The installed-image render harness now seeds a
deterministic `work` / `Deep Work` Focus state, captures
`37b-control-center-focus.png` and `39b-control-center-focus-dark.png`, then
restores Focus to off. This is source-gated only: reviewed Control Center GTK
pixels, live Focus writes, timer behavior, schedules, mode CRUD, and per-app
breakthroughs remain CI/qemu-pending. Local source gates for this pass:
targeted `cargo test -p goblins-os-control-center focus`,
`cargo fmt --all --check`, `cargo clippy --workspace -- -D warnings`,
`cargo test --workspace`, `goblins-os-verify --source-root .` →
**blocked=0 (2235)**,
`bash -n os/bootc/render-screens.sh os/hardware-gate/verify-shipping-status.sh`,
scoped `git diff --check`, and the Rust 1.88 GTK container
`cargo clippy -p goblins-os-control-center --features
goblins-os-control-center/native-desktop -- -D warnings` from a minimal temp
workspace.

Current Focus live-write proof continuation: the display-backed VM capture
harness now requires `focus-arm-roundtrip-proof.json` before signoff. Inside the
installed session it saves Focus + notification banner state, seeds a
deterministic `gate-work` mode, posts `/v1/focus/activate`, verifies
`active-mode` and banner snapshot/read-back, posts `/v1/focus/deactivate`,
verifies Focus is off and banners are restored, restores the original state, and
links the proof in `proof-manifest.json`. `close-signoff.sh`,
`verify-shipping-status.sh`, and `goblins-os-verify` reject missing/failing
proof, and the proof explicitly keeps mode CRUD, schedule, and per-app
breakthrough claims false. This is source-gated proof plumbing only; no live
qemu run has produced the artifact yet, and Focus remains `in-progress`. Local
source gates for this pass: `python3 -m py_compile
os/hardware-gate/capture-harness/drive-capture.py`,
`bash -n os/hardware-gate/capture-harness/in-session-orchestrator.sh
os/hardware-gate/capture-harness/run-capture.sh
os/hardware-gate/close-signoff.sh os/hardware-gate/verify-shipping-status.sh`,
`cargo fmt --all --check`, `cargo test -p goblins-os-verify`,
`cargo clippy --workspace -- -D warnings`, `cargo test --workspace`,
`goblins-os-verify --source-root .` → **blocked=0 (2308)**, and
`git diff --check`.

Current Migration continuation: the copy-job/progress and preference-import
planner substrates are now source-gated but not shipped. Core exposes
`/v1/migration/start` and `/v1/migration/progress`; start reuses the validated
source root, destination home, and category contract, stores planned/blocked
progress with progress-log and copied/skipped ledger paths, and keeps live
execution fail-closed with `PRECONDITION_REQUIRED`. Core also exposes
`/v1/migration/preference-plan`, parses a read-only dconf dump narrowly, maps
only the allowlisted GNOME interface/background keys, reuses the existing color
scheme/text-scale normalizers, requires copied-path evidence before planning a
wallpaper URI, and returns `executes_preference_import=false`. These routes do
**not** mount disks, run `rsync`, copy files, read live dconf, write gsettings,
or render installer UI. Local source gates for this pass: `cargo fmt` checks,
targeted migration tests, `cargo clippy --workspace -- -D warnings`,
`cargo test --workspace`, `goblins-os-verify --source-root .` →
**blocked=0 (2090)**, `git diff --check`, and
`bash -n os/hardware-gate/verify-shipping-status.sh`. First-boot UI,
read-only udisks mounting, live rsync execution/streaming, real ledger
persistence, and executing the allowlisted preference import remain CI/qemu-pending.

Current Personal Hotspot continuation: the Settings binding is now source-gated
but not shipped. Core still owns `/v1/hotspot/enabled`, policy gating,
NetworkManager AP creation, `dnsmasq` shared-mode gating, single-radio Wi-Fi
uplink rejection, non-persistent `save no` AP profiles, and PSK-sanitized
errors. Settings ▸ Network renders the Personal Hotspot switch plus write-only
network-name/password rows, prevalidates SSID/password inputs before POST,
clears the password after a successful request, and reverts the switch on the
real core failure message. The current client-readout continuation adds an
honest dnsmasq lease-table parser and Settings count/list rows that only report
connected devices when NetworkManager shared-mode lease data is present; missing
lease data stays "unknown", never "0 devices." This pass does **not** add WPA3/SAE
selection or live AP proof. Local source gates:
`cargo fmt --all --check`, targeted `cargo test -p goblins-os-core hotspot`,
targeted `cargo test -p goblins-os-settings hotspot`,
`cargo clippy --workspace -- -D warnings`, `cargo test --workspace`, the Rust 1.88 GTK container
`cargo clippy -p goblins-os-settings --features goblins-os-settings/native-desktop -- -D warnings`,
`goblins-os-verify --source-root .` → **blocked=0 (1900)**, scoped
`git diff --check`, and `bash -n os/hardware-gate/verify-shipping-status.sh`.
CI/qemu must still prove the Settings render, policy-denied and live-write
paths, NetworkManager AP creation, DHCP/shared-mode behavior, and connected
client readout before Personal Hotspot can ship.

Current Preview viewer continuation: PDF/image default viewer wiring is now
source-gated but not shipped. Fedora 44 repo metadata was checked in a clean
Fedora container: `papers` provides `/usr/bin/papers` and
`org.gnome.Papers.desktop`; `loupe` provides `/usr/bin/loupe` and
`org.gnome.Loupe.desktop`. The bootc image installs and `rpm -q`/`command -v`
asserts both packages, and `/usr/share/applications/mimeapps.list` defaults PDFs
to Papers and common image formats to Loupe. The installed-image self-test now
exercises `/v1/preview/status`, opens a temporary PDF and image through
`/v1/preview/open`, and asserts an unsupported file is rejected honestly. This is
source-gated open-contract proof only. The display-backed hardware gate now also
requires `preview-open-render-proof.json`, linked from `proof-manifest.json` as
`preview_open_render_proof`, after opening installed PDF/PNG fixtures through
`/v1/preview/open`, waiting for real Papers/Loupe processes, capturing
`29-preview-pdf-open.png` and `30-preview-image-open.png`, and rejecting an
unsupported `.txt` fixture. This proof plumbing is source-gated only: reviewed
qemu artifacts with those screenshots, live double-click behavior, and themed
GTK app render remain CI/qemu-pending before Preview can ship.

Current Fingerprint continuation: package/PAM/status substrate is now
source-gated but not shipped. Fedora 44 repo metadata and a clean
`fedora-bootc:44` command test confirmed `authselect`, `fprintd`,
`fprintd-pam`, and `libfprint`; `fprintd` provides `/usr/sbin/fprintd-list`,
`/usr/sbin/fprintd-enroll`, the `net.reactivated.Fprint` D-Bus service, and
`fprintd.service`, while `fprintd-pam` provides `pam_fprintd.so`. The bootc
image installs and `rpm -q` asserts those packages, asserts the fprintd CLIs,
enables fingerprint PAM through `authselect enable-feature with-fingerprint`
(no hand-edited PAM), and verifies `authselect current` includes
`with-fingerprint`. Core exposes `/v1/fingerprint/status` with honest gates for
fprintd, the PAM module, authselect, reader detection, and enrolled fingers;
Settings Security adds a read-only Fingerprint unlock status row. This pass does
**not** add enroll/delete controls, store fingerprints, prove a reader, or prove
sudo/session unlock; CI/qemu plus real hardware must still prove fprintd D-Bus
enrollment/verification and password fallback before Fingerprint can ship.

Current Per-app Privacy continuation: app-keyed portal permission revokes are now
source-gated and live-qemu proven. Core exposes `/v1/app-privacy/revoke`, validates
the known PermissionStore tables plus safe desktop app/resource IDs, and calls
`org.freedesktop.impl.portal.PermissionStore.DeletePermission(table, id, app)`
only for app-keyed grants; resource-keyed device grants remain read-only until
the store can map resources back to owning apps. Settings ▸ Privacy now renders
one row per app-keyed grant with a Revoke action and reports the exact core
outcome. Local source gates: `cargo fmt --all`, `cargo clippy --workspace -- -D warnings`,
`cargo test --workspace`, the Rust 1.88 GTK container
`cargo clippy -p goblins-os-settings --features goblins-os-settings/native-desktop -- -D warnings`,
scoped `git diff --check`, `bash -n os/hardware-gate/verify-shipping-status.sh`,
and `goblins-os-verify --source-root .` → **blocked=0 (1575)**. Run
`28496717503` proves the live app-keyed portal revoke/reload path with
`app-privacy-revoke-proof.json` `status=pass`; resource-keyed device revokes
remain read-only and deferred.
Current Per-app Privacy hardware-proof continuation: the display-backed
hardware-gate harness now requires `app-privacy-revoke-proof.json`: inside the
installed session it snapshots the deterministic
`org.goblins.GatePrivacyProof` portal PermissionStore location grant, seeds that
grant with `PermissionStore.SetPermission`, posts `/v1/app-privacy/revoke`,
verifies `PermissionStore.GetPermission` no longer reports the grant, restores
the prior state, and writes the proof beside the screenshot run. A fully passing
close-signoff run will link it from `proof-manifest.json`. Run `28496717503`
produced the proof artifact and passed
the app-keyed revoke/readback path; resource-keyed device revokes remain
deferred.

Current Multi-display continuation: the guarded apply substrate is now
source-gated but not shipped. Core exposes `/v1/displays/apply`, reads
`ApplyMonitorsConfigAllowed`, requires the caller's compositor serial to match a
fresh `GetCurrentState`, validates a typed logical-monitor payload, encodes the
Mutter `a(iiduba(ssa{sv}))` tuple, and rejects stale serials before calling
`ApplyMonitorsConfig`. Settings ▸ Displays now reports whether protected display
apply is available, but the layout editor remains disabled. Local source gates:
`cargo fmt --all --check`, `cargo clippy --workspace -- -D warnings`,
`cargo test --workspace`, the Rust 1.88 GTK container
`cargo clippy -p goblins-os-settings --features goblins-os-settings/native-desktop -- -D warnings`,
`git diff --check`, `bash -n os/hardware-gate/verify-shipping-status.sh`, and
`goblins-os-verify --source-root .` → **blocked=0 (1579)**. CI/qemu still must
prove the apply/keep/revert flow before the feature can ship.

Current Multi-display hardware-proof continuation: the display-backed VM harness
now requires `multi-display-apply-proof.json` before signoff. The in-session
orchestrator reads the live Mutter `GetCurrentState` connector/mode/serial,
builds a same-layout `/v1/displays/apply` payload, proves `method=verify` and
`method=temporary` return HTTP `200`, proves persistent apply is rejected without
explicit keep confirmation, proves a stale serial is rejected with HTTP `409`,
and records `persistent_keep_claim=false` plus `same_layout_noop=true`. The
proof is linked in `proof-manifest.json`; `run-capture.sh`, `close-signoff.sh`,
`verify-shipping-status.sh`, and `goblins-os-verify` now reject a missing or
failing proof. This is still CI/qemu-pending and does **not** mark Multi-display
shipped.

Current Keyboard continuation: shortcut rebinding and Caps Lock remap are now
source-gated but not shipped. Core aliases `/v1/keyboard/shortcuts/status`,
exposes `/v1/keyboard/shortcuts/binding` for allowlisted Goblins WM binding
set/reset, and exposes `/v1/keyboard/modifier-remap` for the reversible
Caps Lock to Control xkb option. The write path validates accelerator grammar,
refuses conflicts with other allowlisted Goblins bindings, edits only the
`ctrl:*`/`caps:*` xkb option token, and keeps the Settings editor disabled until
qemu proves the live gsettings round trip. The current hardware-gate
continuation requires `keyboard-shortcuts-roundtrip-proof.json`: inside the
installed session the harness posts `/v1/keyboard/shortcuts/binding` to set
`window-hud` to `<Super><Shift>H`, verifies GNOME gsettings read-back, resets to
`<Super>w`, posts `/v1/keyboard/modifier-remap` to map Caps Lock to Control,
verifies `ctrl:nocaps`, then restores the default modifier state before signoff.
This is source-gated proof plumbing only; live qemu execution and rendered
recordable UI remain pending. Local source gates for the current proof-hook pass:
`python3 -m py_compile` for the capture driver, `bash -n` for the capture and
shipping scripts, scoped `git diff --check`, `cargo fmt --all --check`,
`cargo clippy --workspace -- -D warnings`, `cargo test --workspace`, and
`goblins-os-verify --source-root .` → **blocked=0 (2165)**.

Current Voice Control continuation: the push-to-talk dispatch route is now
source-gated but not shipped. Core exposes `/v1/voice/control`, can capture
through the existing dictation path or accept an injected transcript, resolves
only exact curated phrases, falls through to dictation when nothing matches, and
dispatches matched commands only through the existing
`open_settings_panel`/`change_safe_setting` policy + confirmation helpers. The
shared AI registry now has a `voice-control` action and `Voice` entrypoint, the
session helper is copied into the image, and Settings reports the source-gated
Voice Control status without a dead toggle. Fedora 44 package probing found
`whisper-cpp`/`whisper-cpp-devel` as `1.8.1-2.fc44`, but repoquery listed only
libraries/headers and no provider for `whisper-cli`, so this pass intentionally
does **not** add a new RPM or keybinding. The requested `<Super><Alt>c` binding
also collides with the shipped Color Picker binding. Local source gates:
`cargo fmt --all --check`, `cargo clippy --workspace -- -D warnings`,
`cargo test --workspace`, `goblins-os-verify --source-root .` →
**blocked=0 (1594)**, `git diff --check`,
`bash -n os/voice/goblins-os-voice-control os/hardware-gate/verify-shipping-status.sh`,
targeted `cargo check -p goblins-os-core -p goblins-os-ai`, targeted
`cargo test -p goblins-os-core voice_control -- --nocapture`, and the Rust 1.88
GTK container
`cargo clippy -p goblins-os-settings --features goblins-os-settings/native-desktop -- -D warnings`.
CI/qemu must still prove live capture/transcription, Settings render, helper
launch/type behavior, confirmation UI, and HUD before this feature can ship.

Current Live Captions continuation: the shell overlay/stream contract is now
source-gated but not shipped. Core aliases `/v1/captions/status` to the existing
status substrate and exposes `/v1/captions/stream` as a real
`text/event-stream` status event that never fabricates caption text while the
model/capture path is absent. The new `goblins-captions@goblins.os` shell
extension is enabled in the Goblins shell mode, but its existing GSettings
schema still defaults `enabled=false`, so it starts hidden; when explicitly
enabled before the live stream exists it shows an honest "waiting for the local
caption stream" capsule using Inter and the existing Goblins material/accent
language. The current shell-control continuation adds a source-gated Quick
Settings toggle bound to the existing `enabled=false` captions schema; it only
shows the honest waiting overlay when toggled and does not start capture or
fabricate captions. No RPM, keybinding, or live STT loop is claimed in this
pass. Local source gates: `cargo fmt --all --check`,
`cargo clippy --workspace -- -D warnings`, `cargo test --workspace`,
`goblins-os-verify --source-root .` → **blocked=0 (1810)**,
`node --check os/gnome-shell-extensions/goblins-captions@goblins.os/extension.js`,
`glib-compile-schemas --dry-run os/glib-schemas` in a Linux container,
`git diff --check`, `bash -n os/hardware-gate/verify-shipping-status.sh`, and
targeted `cargo test -p goblins-os-core live_captions -- --nocapture`.
CI/qemu still must prove the shell extension render, menu/shortcut control,
system-audio capture, transcription stream, and overlay behavior before Live
Captions can ship.

Current Live Captions Settings continuation: Settings ▸ Accessibility now
fetches `/v1/live-captions/status` and renders a read-only Live Captions row
with runtime/model/PipeWire/capture readiness, local-captioning copy, and the
explicit boundary that the toggle lives in Quick Settings until CI/qemu proves
the live overlay. It does not add capture, STT, a package, or a Settings write.
Local source gates for this Settings row: `cargo test -p goblins-os-settings
live_captions`, `cargo fmt -p goblins-os-settings -p goblins-os-verify
--check`, `cargo clippy --workspace -- -D warnings`, `cargo test --workspace`,
`goblins-os-verify --source-root .` → **blocked=0 (2036)**, `git diff
--check`, `bash -n os/hardware-gate/verify-shipping-status.sh`, and the Rust
1.88 GTK container `cargo clippy -p goblins-os-settings --features
goblins-os-settings/native-desktop -- -D warnings`. GTK render, Quick Settings
render, live stream, and capture/transcription behavior remain CI/qemu-pending.

Current Live Captions capture-plan continuation: the PipeWire capture-plan
contract is now source-gated but not shipped. Core exposes a deterministic
`pw-dump` monitor-source parser, a stable `pw-record --target <monitor> --rate
16000 --channels 1 --format s16 <chunk.wav>` argv builder, and serializes the
planned capture route while keeping `runtime_ready_claim=false`,
`capture_runtime_ready=false`, and `transcription_ready_claim=false`. Capture
command presence no longer makes Live Captions active. Settings displays the
same no-live-capture/no-live-transcription boundary. Fedora 44 clean-container
proof confirmed `pipewire-utils-1.6.7-1.fc44` provides `/usr/bin/pw-record` and
`/usr/bin/pw-dump`, so the bootc image now asserts both binaries without adding
new RPMs. Local source gates for this pass: Fedora 44 `pipewire-utils` install
probe, targeted `cargo test -p goblins-os-core live_captions -- --nocapture`,
targeted `cargo test -p goblins-os-settings live_captions -- --nocapture`,
`cargo fmt --all --check`, `cargo clippy --workspace -- -D warnings`,
`cargo test --workspace`, `goblins-os-verify --source-root .` →
**blocked=0 (2204)**, `bash -n os/hardware-gate/verify-shipping-status.sh`, and
`git diff --check`. Live monitor capture, transcription stream, overlay render,
and Quick Settings render proof remain CI/qemu-pending.

Current Live Captions waiting-overlay render-hook continuation: the existing
hidden-by-default shell extension now exposes a deterministic
`showWaitingRenderProof()` hook for the desktop render harness. The hook renders
only the honest "waiting for the local caption stream" capsule and returns false
capture/transcription/live-caption-text claims. `render-desktop.sh` now captures
`58-live-captions-waiting-$suffix.png` in light/dark after the Switch Control
overlay capture, then hides the overlay. This is source-gated only; no local
GTK/shell render is claimed from macOS, and CI/qemu still must produce and review
the artifact pixels before Live Captions can ship. Local source gates for this
pass: `node --check
os/gnome-shell-extensions/goblins-captions@goblins.os/extension.js`, `bash -n
os/bootc/render-desktop.sh os/hardware-gate/verify-shipping-status.sh`,
`git diff --check`, `cargo fmt --all --check`, `cargo clippy --workspace -- -D
warnings`, `cargo test --workspace`, and `goblins-os-verify --source-root .` →
**blocked=0 (2208)**.

Current Visual Look Up continuation: the region-capture card surface is now
source-gated but not shipped. The new `goblins-os-visual-lookup` crate checks
`/v1/vision/status` before any capture, requires a loopback local core URL, uses
the interactive xdg-desktop-portal screenshot flow for a user-selected region,
stores the captured file only in a 0700 runtime dir as a 0600 file, posts the
local path to `/v1/ai/visual-lookup`, deletes the temporary image afterward,
and renders a Goblins-branded GTK identification card with honest model-missing
and "Best guess" copy. Settings ▸ Goblin & Models now has a Vision row that
states GPT-OSS is text-only and a separate local VLM is required, and the shared
AI action registry exposes `identify-in-image`. No RPM, default keybinding, or
desktop shortcut is claimed in this pass; the proposed `<Shift><Super>4` binding
collides with the shipped GNOME screenshot UI binding and needs CI/qemu proof
before enabling a replacement. Current continuation adds
`org.goblins.OS.VisualLookup.desktop` as a source-gated applications-grid launcher
for the already-packaged helper, with Containerfile desktop-entry validation and
verifier pins. Local source gates for the launcher pass still need to be run from
this source. CI/qemu still must prove the portal region capture, GTK card render,
applications-grid launch/menu entry, and final non-conflicting shortcut before
Visual Look Up can ship.

Current Today/Widgets continuation: the GTK Today panel surface is now
source-gated but not shipped. The new `goblins-os-today` crate reads
`/v1/today/status` over a loopback-only core URL, normalizes the widget layout,
renders local Date and Clock cards from real local values, and renders Weather,
Calendar, and Daily Brief as honest empty states until location services, a
calendar account, and a local model are actually available. The image build now
builds/copies the binary, the app has a desktop launcher, and
`20-goblins-os-today` seeds the default widget order. Verifier coverage pins the
binary, desktop launcher, dconf seed, native feature, core route fetch, shared UI
theming, honest empty-state copy, and the installed-image render harness captures
`122-today.png` / `123-today-dark.png` from the installed binary. Web verification
found Fedora 44 has `gtk4-layer-shell-devel`, but upstream documents GTK4 layer
shell as unsupported on GNOME Wayland, so this pass intentionally adds no new RPMs
and does not claim right-edge layer-shell anchoring. Current continuation adds a
source-gated GNOME Shell menu-bar Today date button: `goblins-menubar` reads the
stable GNOME clock preferences, formats a local-time label with `GLib.DateTime`,
opens the installed `/usr/libexec/goblins-os/goblins-os-today` helper on click or
touch, and cleans up its clock timer/signals on disable. The desktop render
harness now seeds deterministic clock preferences and captures
`59c-menubar-today-$suffix.png`; this is screenshot-hook plumbing only and does
not claim a reviewed qemu render yet. Local source gates:
`bash -n os/bootc/render-screens.sh os/hardware-gate/verify-shipping-status.sh`,
`cargo fmt --all --check`, `cargo clippy --workspace -- -D warnings`,
`cargo test --workspace`, `goblins-os-verify --source-root .` →
**blocked=0 (2227)**, `git diff --check`, targeted
`cargo test -p goblins-os-today`, and the Rust 1.88
GTK container
`cargo clippy -p goblins-os-today --features goblins-os-today/native-desktop -- -D warnings`.
CI/qemu still must prove and review the GTK render, GNOME Shell/menu-bar date
entry screenshot, edge-open behavior, and any future live weather/calendar/brief
integrations before Today/Widgets can ship.

Current Sound Recognition continuation: the Settings controls + write bridge are
now source-gated but not shipped. Core exposes
`/v1/sound-recognition/preference` and `/v1/sound-recognition/sound-toggle`,
writes only the allowlisted `org.goblins.SoundRecognition` keys, rejects unknown
sound ids, clamps confidence, and never reports listening just because a
preference saved. Settings ▸ Accessibility now shows model/listener/capture
readiness, the reliability caveat, the master toggle, per-sound toggles,
sensitivity, confidence, and alert options through those core routes. No RPM,
model weights, classifier loop, notification integration, or live mic behavior
is claimed in this pass. The current listener-boundary continuation installs
`os/sound-recognition/goblins-os-sound-listener` as
`/usr/libexec/goblins-os/goblins-os-sound-listener` plus a session-user systemd
unit, but the listener only exposes `--capability-check`/`--self-test`, reports
`ready=false` and `runtime_ready_claim=false`, and exits without capturing
microphone audio. Core now consumes that capability report instead of treating
binary presence as listener readiness. Local source gates: `cargo fmt --all --check`,
`python3 -m py_compile os/sound-recognition/goblins-os-sound-listener`,
`python3 os/sound-recognition/goblins-os-sound-listener --self-test`,
`python3 os/sound-recognition/goblins-os-sound-listener --capability-check`,
`cargo clippy --workspace -- -D warnings`, `cargo test --workspace`,
`goblins-os-verify --source-root .` → **blocked=0 (1805)**,
`git diff --check`, targeted `cargo test -p goblins-os-core sound_recognition`,
targeted `cargo test -p goblins-os-settings sound_recognition`, and the Rust 1.88
GTK container
`cargo clippy -p goblins-os-settings --features goblins-os-settings/native-desktop -- -D warnings`.
CI/qemu must still prove the GTK render, installed schema/write behavior,
installed user-service behavior, PipeWire capture, notification/flash path, and
reliability copy before Sound Recognition can ship.

Current Sound Recognition decision-contract continuation: the classifier output
decision layer is now source-gated but not shipped. Core owns the pure
AudioSet-class → fixed-category mapping, sensitivity/confidence threshold,
per-category debounce, and notification payload contract through
`evaluate_sound_recognition_window`/`sound_recognition_notification_payload`;
the installed session listener mirrors that contract with `--decision-self-test`
and advertises `decision_contract_ready=true` while still returning `ready=false`
and `runtime_ready_claim=false`. No model weights, onnxruntime package,
microphone capture, notification delivery, sound, flash, or live listener loop
is claimed in this pass. Local source gates: `cargo fmt --all --check`,
`python3 -m py_compile os/sound-recognition/goblins-os-sound-listener`,
`python3 os/sound-recognition/goblins-os-sound-listener --self-test`,
`python3 os/sound-recognition/goblins-os-sound-listener --decision-self-test`,
`python3 os/sound-recognition/goblins-os-sound-listener --capability-check`,
targeted `cargo test -p goblins-os-core sound_recognition`,
`cargo clippy --workspace -- -D warnings`, `cargo test --workspace`,
`goblins-os-verify --source-root .` → **blocked=0**, `git diff --check`, and
`bash -n os/hardware-gate/verify-shipping-status.sh`. CI/qemu must still prove
the GTK render, installed schema/write behavior, installed user-service
behavior, PipeWire capture, notification/flash path, and reliability copy before
Sound Recognition can ship.

Current Sound Recognition notification-delivery contract continuation: the
Goblins notification handoff is now source-gated but not shipped. Core builds an
explicit delivery plan with `org.goblins.OS.SoundRecognition`, the Goblins icon,
desktop-entry/category hints, critical urgency for safety sounds, alert-sound /
flash / lock-screen intent, and `delivery_ready_claim=false`; the installed
listener mirrors that contract with `--notification-self-test` and advertises
`notification_delivery_contract_ready=true` while still returning `ready=false`,
`runtime_ready_claim=false`, and `capture_runtime_ready=false`. Core also treats
microphone capture readiness as listener-owned runtime capability now, so
`arecord`/driver presence alone cannot make Sound Recognition active. No model,
microphone capture, notification delivery, sound, flash, or live listener loop is
claimed in this pass. Local source gates: `cargo fmt --all --check`,
`cargo clippy --workspace -- -D warnings`, `cargo test --workspace`,
`goblins-os-verify --source-root .` → **blocked=0 (2196)**,
`git diff --check`, `bash -n os/hardware-gate/verify-shipping-status.sh`,
`python3 -m py_compile os/sound-recognition/goblins-os-sound-listener`, and
the listener `--self-test` / `--decision-self-test` / `--notification-self-test`.
CI/qemu must still prove the GTK render, installed schema/write behavior,
installed user-service behavior, PipeWire capture, notification/flash path, and
reliability copy before Sound Recognition can ship.

Current Switch Control continuation: the GNOME Shell scanner scaffold is now
source-gated but not shipped. The `goblins-switch@goblins.os` extension is
installed in the Goblins shell mode and dconf seed, reads the existing
`org.goblins.os.a11y.switch-control` system schema, stays inert while the
feature is disabled, attempts session AT-SPI target discovery when enabled,
renders a highlight ring or point-scan crosshair, supports auto/step scan
advance, and hard-disables on Escape. It falls back honestly to point scan when
AT-SPI controls are absent and keeps pointer injection paused with explicit qemu
proof copy instead of faking a click path. Local source gates for this pass:
`node --check` over every bundled shell extension, `cargo fmt --all --check`,
`cargo clippy --workspace -- -D warnings`, `cargo test --workspace`,
`goblins-os-verify --source-root .` → **blocked=0 (1765)**, `git diff --check`,
and `bash -n os/hardware-gate/verify-shipping-status.sh`. `glib-compile-schemas`
is not available on this macOS host and Docker did not respond for a container
dry-run; the Switch Control schema itself was not changed in this pass, and CI
image compile remains the schema proof. CI/qemu must still prove the Settings
render, installed schema/write behavior, live extension load, AT-SPI target
walk, overlay pixels, and gated selection/input before Switch Control can ship.

Current Switch Control continuation: the desktop render harness now has a
source-gated live-shell proof hook for the point-scan overlay. `render-desktop.sh`
calls `globalThis.goblinsSwitchControl.showPointScanDemo()`, captures
`57-switch-control-point-$suffix.png` in light and dark, and disables the
feature again after capture; the hook does not enable pointer injection or claim
selection success. Local source gates for this pass:
`node --check os/gnome-shell-extensions/goblins-switch@goblins.os/extension.js`,
`bash -n os/bootc/render-desktop.sh os/hardware-gate/verify-shipping-status.sh`,
`cargo fmt --all --check`, `cargo clippy --workspace -- -D warnings`,
`cargo test --workspace`, `git diff --check`, and
`goblins-os-verify --source-root .` → **blocked=0 (1769)**. The actual
screenshot artifacts are still CI/qemu-pending.

Current Text Shortcuts continuation: the Settings table editor is now
source-gated but not shipped. Settings ▸ Keyboard reads `/v1/text-shortcuts`,
shows the engine readiness honestly, lists saved Replace → With entries, can
remove entries, and can add/replace entries through the existing core bridge.
The editor sanitizes empty/identity entries and preserves the core last-wins
de-dupe contract before POSTing. No IBus packages, component XML, dconf seed,
global input environment change, candidate bubble, password-field handling, or
real text-input-v3 expansion is claimed in this pass. Local source gates:
`cargo fmt --all --check`, `cargo clippy --workspace -- -D warnings`,
`cargo test --workspace`, `goblins-os-verify --source-root .` →
**blocked=0 (1647)**, `git diff --check`, targeted
`cargo test -p goblins-os-settings text_shortcuts`, and the Rust 1.88 GTK
container
`cargo clippy -p goblins-os-settings --features goblins-os-settings/native-desktop -- -D warnings`.
CI/qemu must still prove the GTK render and later the real IBus
engine/keystroke selftest before Text Shortcuts can ship.

Current Text Shortcuts engine-readiness continuation: the core status now
requires all three engine facts before reporting `engine_available=true`: the
`ibus` command, the Goblins IBus component XML at
`/usr/share/ibus/component/goblins-textshortcuts.xml`, and the Goblins engine
binary at `/usr/libexec/goblins-os/goblins-textshortcuts-engine`. This prevents
future IBus/CJK package work from falsely claiming Text Shortcuts expansion is
active just because IBus is present. No RPMs, dconf seed, component XML,
session input-module change, engine process, password-field handling, or live
text expansion is claimed in this pass. Local source gates:
`cargo fmt --all --check`, `cargo clippy --workspace -- -D warnings`,
`cargo test --workspace`, `goblins-os-verify --source-root .` →
**blocked=0 (1650)**, `git diff --check`, and targeted
`cargo test -p goblins-os-core text_shortcuts`. CI/qemu must still prove the
installed component, engine startup, GTK render, and keystroke selftest before
Text Shortcuts can ship.

Current Text Shortcuts engine-decision continuation: the
`goblins-os-textshortcuts-engine` crate now owns the pure, host-tested decision
substrate for the future IBus engine. It sanitizes the core JSON table shape,
tracks the current word, shows a single replacement candidate on exact trigger
match, commits replacement text only on a boundary with an explicit
`delete_previous_chars`, and clears/passes through in password, hidden-text, and
sensitive content purposes. The binary is named `goblins-textshortcuts-engine`
and has a `--self-test`/`--preview` CLI for source proof. It is **not** copied
into the image, does not register an IBus component, does not install RPMs, does
not alter the session input path, and does not claim live text expansion.
Local source gates: `cargo fmt --all --check`,
`cargo clippy --workspace -- -D warnings`, `cargo test --workspace`,
`goblins-os-verify --source-root .` -> **blocked=0 (1656)**,
`git diff --check`, targeted `cargo test -p goblins-os-textshortcuts-engine`,
and `goblins-textshortcuts-engine --self-test`. CI/qemu must still prove the
real IBus process, installed component, GTK render, and keystroke selftest
before Text Shortcuts can ship.

Current Text Shortcuts shared-contract continuation: core now depends on
`goblins-os-textshortcuts-engine` and uses its `TextShortcut` JSON shape plus
`sanitize_shortcuts` table contract for `/v1/text-shortcuts` writes and reads.
This removes the duplicate sanitizer between core and the future IBus engine, so
the Settings editor, core bridge, and engine substrate stay on the same
trim/drop-identity/last-wins/cap-500 behavior. No image install, component XML,
RPM, session input-module change, or live expansion is claimed in this pass.
Local source gates: `cargo fmt --all --check`,
`cargo clippy --workspace -- -D warnings`, `cargo test --workspace`,
`goblins-os-verify --source-root .` -> **blocked=0 (1658)**,
`git diff --check`, targeted `cargo test -p goblins-os-core text_shortcuts`,
and targeted `cargo test -p goblins-os-textshortcuts-engine`. CI/qemu must still
prove the real IBus process, installed component, GTK render, and keystroke
selftest before Text Shortcuts can ship.

Current Text Shortcuts IBus registration continuation: the Fedora 44 IBus
packages are now web-verified and source-gated in both the Containerfile install
list and `rpm -q` assertion block (`ibus`, `ibus-gtk4`, `ibus-gtk3`,
`ibus-libs`, `python3-ibus`). The image now installs the
`goblins-textshortcuts-engine` binary and the
`/usr/share/ibus/component/goblins-textshortcuts.xml` component, and runs the
engine self-test plus component-contract check during image build. Core readiness
was tightened so those installed files are not enough to claim live expansion:
`engine_available` now also requires the Goblins IBus input source and the live
runtime loop. This pass intentionally does **not** seed
`goblins-textshortcuts` into dconf, start `ibus-daemon`, change
`GTK_IM_MODULE=gtk-im-context-simple`, or claim keystroke expansion. Local source
gates: `cargo fmt --all --check`, `cargo clippy --workspace -- -D warnings`,
`cargo test --workspace`, `goblins-os-verify --source-root .` -> **blocked=0
(1675)**, `git diff --check`, `python3` XML parse, targeted
`cargo test -p goblins-os-core text_shortcuts`, targeted
`cargo test -p goblins-os-textshortcuts-engine`, and
`cargo run -p goblins-os-textshortcuts-engine -- --component-check
os/goblins-os-textshortcuts/goblins-textshortcuts.xml`. CI/qemu must still prove
the live IBus runtime loop, dconf input-source seed, GTK render, and keystroke
selftest before Text Shortcuts can ship.

Current Text Shortcuts runtime-adapter continuation: the engine crate now maps
pure `EngineAction` decisions to host-tested IBus runtime operations:
candidate matches update preedit text without swallowing typed keys, boundary
matches atomically delete the typed trigger and commit the replacement text,
and candidate clears hide preedit while passing Backspace through. The installed
`--self-test` now asserts that IBus operation contract too. This is still
source-gated only: no GI/IBus event loop, no `ibus-daemon` user unit, no dconf
input-source seed, and no keystroke expansion is claimed. Local source gates:
`cargo fmt --all --check`, `cargo clippy --workspace -- -D warnings`,
`cargo test --workspace`, `goblins-os-verify --source-root .` -> **blocked=0
(1679)**, `git diff --check`, targeted
`cargo test -p goblins-os-textshortcuts-engine`, and
`cargo run -p goblins-os-textshortcuts-engine -- --self-test`. CI/qemu must
still prove the live IBus runtime loop, input-source seed, GTK render, and
keystroke selftest before Text Shortcuts can ship.

Current Text Shortcuts key-event continuation: the engine crate now has a
host-tested IBus key-event normalizer for the future GI loop. Printable
characters become `InputEvent::Character`/boundary events, Backspace maps to the
engine backspace path, Return/Tab are explicit boundaries, navigation/Delete/
Escape reset candidate state, command-modified shortcuts reset without
swallowing, and releases/unknown keys pass through. No session input path,
`ibus-daemon`, dconf seed, GI event loop, or live expansion is claimed in this
pass. Local source gates: `cargo fmt --all --check`,
`cargo clippy --workspace -- -D warnings`, `cargo test --workspace`,
`goblins-os-verify --source-root .` -> **blocked=0 (1683)**,
`git diff --check`, and targeted
`cargo test -p goblins-os-textshortcuts-engine`. CI/qemu must still prove the
live IBus runtime loop, input-source seed, GTK render, and keystroke selftest
before Text Shortcuts can ship.

Current Text Shortcuts runtime-pipeline continuation: the engine crate now
composes raw IBus key normalization, content-purpose gating, engine state, and
runtime operation emission behind `IbusTextShortcutsRuntime`. It owns the active
shortcut table, clears candidate state when the table or sensitive content
purpose changes, passes releases/unknown keys through, leaves candidate updates
as side effects, and handles only confirmed boundary commits with
delete-surrounding-text plus commit-text. The installed `--self-test` now
exercises that composed path instead of a lower-level state call. No session
input path, `ibus-daemon`, dconf seed, GI event loop, or live expansion is
claimed in this pass. Local source gates:
`cargo fmt -p goblins-os-textshortcuts-engine -p goblins-os-verify`,
`cargo fmt --all --check`, `cargo clippy --workspace -- -D warnings`,
`cargo test --workspace`, `goblins-os-verify --source-root .` ->
**blocked=0 (1687)**, `git diff --check`, targeted
`cargo test -p goblins-os-textshortcuts-engine -- --nocapture`, and
`cargo run -p goblins-os-textshortcuts-engine -- --self-test`. CI/qemu must
still prove the live IBus runtime loop, input-source seed, GTK render, and
keystroke selftest before Text Shortcuts can ship.

Current Text Shortcuts table-reload continuation: the engine crate now owns the
host-tested JSON table-store boundary the live IBus loop will use. The store
resolves the same `goblins-os/text-shortcuts.json` config path as core, loads
through the shared sanitizer, degrades missing/invalid/unreadable tables to an
empty pass-through table with explicit status, and `IbusTextShortcutsRuntime`
can refresh from that store while hiding any stale visible candidate. The CLI's
default `--preview` path now reuses this store, so absent user config returns a
truthful no-replacement result instead of inventing data. No session input path,
`ibus-daemon`, dconf seed, GI event loop, file watcher, or live expansion is
claimed in this pass. Local source gates:
`cargo fmt -p goblins-os-textshortcuts-engine -p goblins-os-verify`,
`cargo fmt --all --check`, `cargo clippy --workspace -- -D warnings`,
`cargo test --workspace`, `goblins-os-verify --source-root .` ->
**blocked=0 (1693)**, `git diff --check`, targeted
`cargo test -p goblins-os-textshortcuts-engine -- --nocapture`,
`cargo run -p goblins-os-textshortcuts-engine -- --self-test`, and
`cargo run -p goblins-os-textshortcuts-engine -- --preview omw`. CI/qemu must
still prove the live IBus runtime loop, input-source seed, GTK render, and
keystroke selftest before Text Shortcuts can ship.

Current Text Shortcuts runtime-event continuation: the engine crate now has a
host-tested `IbusRuntimeEvent` router for the future GI/IBus session loop. Key
events, focus-in, focus-out, reset, content-purpose changes, and table changes
all flow through one runtime boundary, clearing stale candidates and refusing
sensitive fields before the live loop can emit preedit/commit operations. The
installed `--self-test` now sends raw key input through that event router. No
session input path, `ibus-daemon`, dconf seed, GI event loop, file watcher, or
live expansion is claimed in this pass. Local source gates:
`cargo fmt -p goblins-os-textshortcuts-engine -p goblins-os-verify`,
`cargo fmt --all --check`, `cargo clippy --workspace -- -D warnings`,
`cargo test --workspace`, `goblins-os-verify --source-root .` ->
**blocked=0 (1699)**, `git diff --check`, targeted
`cargo test -p goblins-os-textshortcuts-engine -- --nocapture`, and
`cargo run -p goblins-os-textshortcuts-engine -- --self-test`. CI/qemu must
still prove the live IBus runtime loop, input-source seed, GTK render, and
keystroke selftest before Text Shortcuts can ship.

Current Text Shortcuts keystroke-selftest continuation: the engine crate now
exports a shared `run_text_shortcuts_keystroke_self_test` contract and the
installed binary exposes `--keystroke-self-test`. That source-gated scenario
drives the runtime event router through typed trigger → candidate preedit,
boundary commit, password-field pass-through, and focus-out cleanup, and the
Containerfile now runs it beside the component/self-test checks so image builds
catch drift before the live GI loop is enabled. No session input path,
`ibus-daemon`, dconf seed, GI event loop, file watcher, or live expansion is
claimed in this pass. Local source gates:
`cargo fmt -p goblins-os-textshortcuts-engine -p goblins-os-verify`,
`cargo fmt --all --check`, `cargo clippy --workspace -- -D warnings`,
`cargo test --workspace`, `goblins-os-verify --source-root .` ->
**blocked=0 (1703)**, `git diff --check`, targeted
`cargo test -p goblins-os-textshortcuts-engine -- --nocapture`,
`cargo run -p goblins-os-textshortcuts-engine -- --self-test`, and
`cargo run -p goblins-os-textshortcuts-engine -- --keystroke-self-test`.
CI/qemu must still prove the live IBus runtime loop, input-source seed, GTK
render, and keystroke selftest before Text Shortcuts can ship.

Current Text Shortcuts table-watch continuation: the engine crate now has a
host-tested table fingerprint + `TextShortcutTableWatcher` contract for the
future live GI/IBus loop. It reloads the runtime table only when the JSON table's
content state changes, preserves the current candidate when the file is
unchanged, hides stale preedit candidates when the table changes, and degrades
invalid or missing tables to pass-through. The installed binary exposes
`--table-watch-self-test`, and the Containerfile runs it beside the component
and keystroke checks so image builds catch drift. No session input path,
`ibus-daemon`, dconf seed, GI event loop, OS file watcher, or live expansion is
claimed in this pass. Local source gates:
`cargo fmt -p goblins-os-textshortcuts-engine -p goblins-os-verify`,
`cargo fmt --all --check`, `cargo clippy --workspace -- -D warnings`,
`cargo test --workspace`, `goblins-os-verify --source-root .` ->
**blocked=0 (1709)**, `git diff --check`, targeted
`cargo test -p goblins-os-textshortcuts-engine -- --nocapture`,
`cargo run -p goblins-os-textshortcuts-engine -- --self-test`,
`cargo run -p goblins-os-textshortcuts-engine -- --keystroke-self-test`, and
`cargo run -p goblins-os-textshortcuts-engine -- --table-watch-self-test`.
CI/qemu must still prove the live IBus runtime loop, input-source seed, GTK
render, and keystroke selftest before Text Shortcuts can ship.

Current Text Shortcuts content-purpose continuation: the engine crate now has a
host-tested IBus content-purpose decoder for the future GI loop. It maps
`IBUS_INPUT_PURPOSE_PASSWORD` and `IBUS_INPUT_PURPOSE_PIN` to non-replacing
runtime purposes, treats unknown purposes as normal free-form text, and exposes
an installed `--content-purpose-self-test` so the image build catches drift in
the hidden-input refusal contract. No session input path, `ibus-daemon`, dconf
seed, GI event loop, or live expansion is claimed in this pass. Local source
gates:
`cargo fmt -p goblins-os-textshortcuts-engine -p goblins-os-verify`,
`cargo fmt --all --check`, `cargo clippy --workspace -- -D warnings`,
`cargo test --workspace`, `goblins-os-verify --source-root .` ->
**blocked=0 (1715)**, `git diff --check`, targeted
`cargo test -p goblins-os-textshortcuts-engine -- --nocapture`,
`cargo run -p goblins-os-textshortcuts-engine -- --self-test`,
`cargo run -p goblins-os-textshortcuts-engine -- --keystroke-self-test`,
`cargo run -p goblins-os-textshortcuts-engine -- --table-watch-self-test`, and
`cargo run -p goblins-os-textshortcuts-engine -- --content-purpose-self-test`.
CI/qemu must still prove the live IBus runtime loop, input-source seed, GTK
render, and keystroke selftest before Text Shortcuts can ship.

Current Text Shortcuts stdio-protocol continuation: the engine crate now exposes
a line-oriented JSON runtime protocol for the future GI/IBus adapter. `--stdio`
loads the same JSON table and keeps the Rust runtime state alive across key,
focus, content-purpose, reset, and table-change requests, returning explicit
IBus operations (`update-preedit-text`, `delete-surrounding-text`, `commit-text`,
`hide-preedit-text`) as JSON responses. `--stdio-self-test` exercises the
trigger -> preedit -> boundary commit path plus PIN-field pass-through, and the
Containerfile runs it beside the other installed engine contracts. No session
input path, `ibus-daemon`, dconf seed, GI event loop, Python shim, or live
expansion is claimed in this pass. Local source gates:
`cargo fmt -p goblins-os-textshortcuts-engine -p goblins-os-verify`,
`cargo fmt --all --check`, `cargo clippy --workspace -- -D warnings`,
`cargo test --workspace`, `goblins-os-verify --source-root .` ->
**blocked=0 (1723)**, `git diff --check`, targeted
`cargo test -p goblins-os-textshortcuts-engine -- --nocapture`,
`cargo run -p goblins-os-textshortcuts-engine -- --self-test`,
`cargo run -p goblins-os-textshortcuts-engine -- --keystroke-self-test`,
`cargo run -p goblins-os-textshortcuts-engine -- --table-watch-self-test`,
`cargo run -p goblins-os-textshortcuts-engine -- --content-purpose-self-test`,
and `cargo run -p goblins-os-textshortcuts-engine -- --stdio-self-test`.
CI/qemu must still prove the live IBus runtime loop, input-source seed, GTK
render, and keystroke selftest before Text Shortcuts can ship.

Current Text Shortcuts IBus-adapter continuation: the IBus component now points
to `/usr/libexec/goblins-os/goblins-textshortcuts-ibus`, a Python GI adapter
that registers the `goblins-textshortcuts` `IBus.Engine`, translates key/focus/
content-purpose callbacks into the Rust `--stdio` JSON protocol, and applies
only explicit operations returned by that runtime (`update_preedit_text`,
`delete_surrounding_text`, `commit_text`, `hide_preedit_text`). The bridge is
fail-open: if the Rust child is missing, exits, times out, or returns invalid
JSON, key events pass through instead of blocking text input. The Containerfile
installs the adapter, runs `python3 -m py_compile`, runs
`goblins-textshortcuts-ibus --self-test`, and keeps the component XML contract
check tied to the adapter entrypoint. This pass does not seed the IBus input
source, does not flip the session input-method environment, and does not claim
qemu/live expansion proof; core still reports the runtime loop as pending until
the real session path is proven. Local source gates:
`cargo fmt -p goblins-os-textshortcuts-engine -p goblins-os-verify`,
`cargo fmt --all --check`, `cargo clippy --workspace -- -D warnings`,
`cargo test --workspace`, `goblins-os-verify --source-root .` ->
**blocked=0 (1738)**, `git diff --check`,
`python3 -m py_compile os/goblins-os-textshortcuts/goblins-textshortcuts-ibus`,
`python3 os/goblins-os-textshortcuts/goblins-textshortcuts-ibus --self-test`,
targeted `cargo test -p goblins-os-textshortcuts-engine -- --nocapture`,
`cargo run -p goblins-os-textshortcuts-engine -- --component-check
os/goblins-os-textshortcuts/goblins-textshortcuts.xml`, and
`cargo run -p goblins-os-textshortcuts-engine -- --stdio-self-test`. CI/qemu
must still prove the live IBus runtime loop, input-source seed, GTK render, and
keystroke selftest before Text Shortcuts can ship.

Current Text Shortcuts session-enable continuation: the Goblins session now
seeds `org.gnome.desktop.input-sources sources=[('ibus',
'goblins-textshortcuts')]`, preloads the Goblins IBus engine through
`org.freedesktop.ibus.general preload-engines`, orders the seed before Fedora's
`org.freedesktop.IBus.session.GNOME.service`, and removes the old
`GTK_IM_MODULE=gtk-im-context-simple` / `QT_IM_MODULE=simple` /
`XMODIFIERS=@im=none` overrides from both the session wrapper and installed
environment.d file. It does not set `GTK_IM_MODULE=ibus` globally; GNOME Wayland
is expected to broker IBus through Mutter/text-input-v3. Core still keeps
`runtime_loop_available=false`, so Settings cannot claim expansion is active
until qemu proves the real keystroke path. Local source gates:
`cargo fmt -p goblins-os-verify`, `cargo fmt --all --check`,
`cargo clippy --workspace -- -D warnings`, `cargo test --workspace`,
`goblins-os-verify --source-root .` -> **blocked=0 (1750)**,
`git diff --check`, and `bash -n os/session/goblins-os-session`.
`systemd-analyze verify` is not available on this macOS host. CI/qemu must still
prove the user-session IBus service starts, the input source is active, the
adapter receives key events, and replacement commits are pass-through-safe before
Text Shortcuts can ship.

Current Text Shortcuts hardware-proof continuation: the display-backed VM
capture harness now fail-closes on `text-shortcuts-session-enable-proof.json`.
Inside the installed GNOME session it requires active
`org.freedesktop.IBus.session.GNOME.service`, seeded `goblins-textshortcuts` input source,
preloaded Goblins engine, `ibus engine goblins-textshortcuts` as the active
engine, adapter `--self-test`, and `/v1/text-shortcuts` honesty that
`engine_available=false` / `runtime_loop_available=false` until a later
keystroke proof flips the runtime gate. The host persists the proof beside the
screenshots, records it in `proof-manifest.json`, and makes
`run-capture.sh`, `close-signoff.sh`, `verify-shipping-status.sh`, and
`goblins-os-verify` reject runs without it. Local source gates: `bash -n` for
the capture/signoff scripts, `python3 -m py_compile` for the capture driver,
`cargo fmt --all --check`, `cargo clippy --workspace -- -D warnings`,
`cargo test --workspace`, `git diff --check`, and
`cargo run -p goblins-os-verify -- --source-root .` -> **blocked=0 (1781)**.
This is still CI/qemu-pending and does **not** prove live key replacement,
adapter callbacks from a focused text field, password-field refusal in-session,
or the accept bubble.

Current Text Shortcuts live-keystroke proof continuation: the display-backed VM
capture harness now has a proof-only GTK text field surface in
`goblins-os-shell --text-shortcuts-proof normal|password` and fail-closes on
`text-shortcuts-live-keystroke-proof.json`. In qemu it seeds a single
`omw` -> `onmyway` shortcut, selects the `goblins-textshortcuts` IBus engine,
drives the focused field with host QMP keyboard input `omw.`, requires normal
text to read back as `onmyway.`, and requires the password-purpose field to read back as the
unchanged `omw.` with `password_refusal=true`. `run-capture.sh`,
`drive-capture.py`, `close-signoff.sh`, `verify-shipping-status.sh`, and
`goblins-os-verify` now reject screenshot/signoff runs without that live
keystroke proof, while core still keeps `runtime_ready_claim=false` until qemu
artifacts are reviewed and the runtime gate is intentionally flipped. Local
source gates: `cargo fmt --all --check`, `bash -n` for the capture/signoff
scripts, `python3 -m py_compile` for the capture driver,
`cargo clippy --workspace -- -D warnings`, `cargo test --workspace`,
`git diff --check`, `cargo run -p goblins-os-verify -- --source-root .` ->
**blocked=0 (1795)**, and the Rust 1.88 GTK container
`cargo clippy -p goblins-os-shell --features goblins-os-shell/native-desktop -- -D warnings`
after installing `libgtk-4-dev`, `pkg-config`, and the missing `clippy`
component in the disposable container. This is still CI/qemu-pending and does
**not** prove the live keystroke path locally or mark Text Shortcuts shipped.

Current Text Shortcuts adapter-capability continuation: the installed IBus
adapter now has `--capability-check`, which runs the Rust
`goblins-textshortcuts-engine --stdio-self-test` contract and emits JSON with
`adapter_contract_ready=true` while keeping `ready=false` and
`runtime_ready_claim=false`. The Containerfile fail-closes on that adapter/runtime
contract and the false runtime claim; this proves the installed adapter can talk
to the Rust stdio contract without claiming live IBus key events, focused-field
callbacks, text-input-v3 commits, password-field refusal in-session, or the
accept bubble. Local source gates: `python3 -m py_compile
os/goblins-os-textshortcuts/goblins-textshortcuts-ibus`, `python3
os/goblins-os-textshortcuts/goblins-textshortcuts-ibus --self-test`,
`GOBLINS_TEXTSHORTCUTS_ENGINE="$PWD/target/debug/goblins-textshortcuts-engine"
python3 os/goblins-os-textshortcuts/goblins-textshortcuts-ibus
--capability-check`, `cargo build -p goblins-os-textshortcuts-engine`,
`cargo test -p goblins-os-textshortcuts-engine`, `cargo fmt --all --check`,
`cargo clippy --workspace -- -D warnings`, `cargo test --workspace`,
`git diff --check`, and `goblins-os-verify --source-root .` ->
**blocked=0 (1819)**. This is still CI/qemu-pending and does **not** mark Text
Shortcuts shipped.

Current Text Shortcuts adapter table-reload continuation: the installed IBus
adapter now reads the same `~/.config/goblins-os/text-shortcuts.json` table as
core, sanitizes it with the shared last-wins/drop-empty/identity contract, and
sends a Rust stdio `table-changed` event on first use and whenever the file
content changes. Returned cleanup operations are applied so stale preedit can be
hidden before the next key event, but the feature still does not claim a live
IBus session, file monitor, text-input-v3 commit, or accept bubble. Local source
gates: `python3 -m py_compile
os/goblins-os-textshortcuts/goblins-textshortcuts-ibus`, `python3
os/goblins-os-textshortcuts/goblins-textshortcuts-ibus --self-test`, `cargo test
-p goblins-os-textshortcuts-engine`, `cargo fmt --all --check`, `cargo clippy
--workspace -- -D warnings`, `cargo test --workspace`, `git diff --check`, and
`goblins-os-verify --source-root .` -> **blocked=0 (1824)**. This is still
CI/qemu-pending and does **not** mark Text Shortcuts shipped.

Current Text Shortcuts autocorrect-gate continuation: core now exposes an
`autocorrect` capability in `/v1/text-shortcuts`, gated only by a local
autocorrect model path (`GOBLINS_TEXTSHORTCUTS_AUTOCORRECT_MODEL` or the
Goblins autocorrect model dir) or installed Hunspell dictionaries. Settings
renders a read-only Autocorrect status row that stays off when resources are
absent and "available" but still disabled when resources are present; no package,
model, toggle write, or live autocorrect behavior is claimed. Local source
gates: `cargo test -p goblins-os-core text_shortcuts`, `cargo test -p
goblins-os-settings text_shortcuts_editor_helpers_sanitize_and_preserve_engine_truth`,
Rust 1.88 GTK container `cargo clippy -p goblins-os-settings --features
goblins-os-settings/native-desktop -- -D warnings`, `cargo fmt --all --check`,
`cargo clippy --workspace -- -D warnings`, `cargo test --workspace`, `git diff
--check`, and `goblins-os-verify --source-root .` -> **blocked=0 (1831)**. This
is still CI/qemu-pending and does **not** mark Text Shortcuts shipped.

Current Text Shortcuts adapter runtime-selftest continuation: the installed IBus
adapter now has `--runtime-self-test`, which starts the real Rust `--stdio`
runtime through the Python `RuntimeBridge`, pushes a sanitized `table-changed`
event, drives `omw ` through the same JSON key protocol, verifies preedit +
delete/commit/hide operations, then verifies PIN-purpose pass-through without
operations. The Containerfile runs this installed adapter/runtime self-test, but
it still does **not** prove a live IBus bus, focused GTK field callbacks,
text-input-v3 commits, password-field refusal in-session, or the accept bubble.
Local source gates: `cargo build -p goblins-os-textshortcuts-engine`, `python3
-m py_compile os/goblins-os-textshortcuts/goblins-textshortcuts-ibus`, `python3
os/goblins-os-textshortcuts/goblins-textshortcuts-ibus --self-test`,
`GOBLINS_TEXTSHORTCUTS_ENGINE="$PWD/target/debug/goblins-textshortcuts-engine"
python3 os/goblins-os-textshortcuts/goblins-textshortcuts-ibus
--runtime-self-test`, `cargo test -p goblins-os-textshortcuts-engine`, and
`goblins-os-verify --source-root .` -> **blocked=0 (1835)**. This is still
CI/qemu-pending and does **not** mark Text Shortcuts shipped.

Current Text Shortcuts accept-bubble dismiss continuation: Escape now maps to a
dedicated candidate-dismiss event instead of the generic navigation reset. The
engine handles Escape only when a candidate is visible, hides preedit without
committing, clears the pending trigger, and otherwise passes Escape through. The
Rust keystroke and stdio self-tests cover this path, and the Python adapter
runtime self-test drives the real Rust `--stdio` child through the same Escape
protocol. This remains source-gated only: it does **not** prove a live IBus bus,
focused-field callbacks, text-input-v3 commits, password-field refusal in-session,
or the rendered accept bubble. This pass also hardens `goblins-os-verify` so its
source scan skips non-regular/large generated files and caches repeated
source-file reads; this keeps the required verifier gate usable on dirty local
worktrees without weakening source assertions. Local gates: `cargo build -p
goblins-os-textshortcuts-engine`, `python3 -m py_compile
os/goblins-os-textshortcuts/goblins-textshortcuts-ibus`, `python3
os/goblins-os-textshortcuts/goblins-textshortcuts-ibus --self-test`,
`GOBLINS_TEXTSHORTCUTS_ENGINE="$PWD/target/debug/goblins-textshortcuts-engine"
python3 os/goblins-os-textshortcuts/goblins-textshortcuts-ibus
--runtime-self-test`, `cargo run -p goblins-os-textshortcuts-engine --
--keystroke-self-test`, `cargo run -p goblins-os-textshortcuts-engine --
--stdio-self-test`, `cargo fmt --all --check`, `cargo clippy --workspace -- -D
warnings`, `cargo test --workspace`, scoped `git diff --check` over the changed
files, and `goblins-os-verify --source-root .` -> **blocked=0 (1839)**. This is
still CI/qemu-pending and does **not** mark Text Shortcuts shipped.

Current Text Shortcuts Escape-dismiss live-proof continuation: the display-backed
VM keystroke proof now has a third proof-only GTK field,
`goblins-os-shell --text-shortcuts-proof dismiss`, and the in-session harness
drives `omw` followed by host QMP keyboard `Escape`. The proof JSON must show
`dismiss_key=Escape`, `dismiss_expected=omw`, `dismiss_actual=omw`, and
`dismiss_no_commit=true` in addition to the existing normal expansion and
password refusal checks. `run-capture.sh`, `close-signoff.sh`,
`verify-shipping-status.sh`, and `goblins-os-verify` reject runs without the
dismiss proof fields. Local gates: `bash -n` for the capture/signoff scripts,
`cargo fmt --all --check`, `cargo test -p goblins-os-shell
parses_text_shortcuts_proof_targets`, `cargo clippy --workspace -- -D warnings`,
`cargo test --workspace`, Rust 1.88 GTK container `cargo clippy -p
goblins-os-shell --features goblins-os-shell/native-desktop -- -D warnings`,
and `goblins-os-verify --source-root .` -> **blocked=0 (1848)**. This is still
CI/qemu-pending and does **not** prove the live Wayland/IBus path locally or mark
Text Shortcuts shipped.

Current Text Shortcuts pass-through live-proof continuation: the display-backed
VM keystroke proof now has a fourth proof-only GTK field,
`goblins-os-shell --text-shortcuts-proof passthrough`, and the in-session harness
drives an unknown word with host QMP keyboard input `hello.`. The proof JSON must show
`passthrough_input=hello.`, `passthrough_expected=hello.`,
`passthrough_actual=hello.`, and `passthrough_unchanged=true` in addition to the
normal replacement, Escape dismiss, and password refusal fields. `run-capture.sh`,
`close-signoff.sh`, `verify-shipping-status.sh`, and `goblins-os-verify` reject
runs without the pass-through proof fields. This is still CI/qemu-pending and
does **not** prove the live Wayland/IBus path locally or mark Text Shortcuts
shipped.

Current Text Shortcuts candidate-metadata continuation: the Rust stdio runtime
now includes a `candidate` object only on visible preedit responses, carrying
the replacement text, `accept_on=word-boundary`, `dismiss_key=Escape`, and
`rendered_bubble_ready_claim=false`. The Python IBus adapter still ignores that
metadata for live behavior, but its runtime self-test now verifies the real Rust
stdio child emits it before Escape dismiss and boundary commit. This gives the
future accept-bubble render a stable protocol contract without claiming a live
rendered bubble, focused-field callback, or text-input-v3 proof. Local gates:
`cargo fmt --all --check`, `cargo test -p goblins-os-textshortcuts-engine -- --nocapture`,
`python3 -m py_compile os/goblins-os-textshortcuts/goblins-textshortcuts-ibus`,
`python3 os/goblins-os-textshortcuts/goblins-textshortcuts-ibus --self-test`,
`cargo run -p goblins-os-textshortcuts-engine -- --stdio-self-test`,
`GOBLINS_TEXTSHORTCUTS_ENGINE="$PWD/target/debug/goblins-textshortcuts-engine" python3 os/goblins-os-textshortcuts/goblins-textshortcuts-ibus --runtime-self-test`,
`cargo clippy --workspace -- -D warnings`, `cargo test --workspace`,
`goblins-os-verify --source-root .` -> **blocked=0 (1940)**. This is still
CI/qemu-pending and does **not** mark Text Shortcuts shipped.

Current Text Shortcuts candidate-bubble proof-surface continuation:
`goblins-os-shell --text-shortcuts-proof candidate` now exposes a proof-only GTK
field seeded with `omw` and a Goblins-native candidate bubble for `on my way`.
The proof file records `replacement=on my way`, `accept_on=word-boundary`,
`dismiss_key=Escape`, and `rendered_bubble_ready_claim=false`; the render script
now captures light/dark candidate-bubble screenshots for CI/qemu, and
`goblins-os-verify` pins the mode, style, honest render claim, and render targets.
This does **not** implement a live IBus overlay, focused-field callback, or
Wayland text-input-v3 bubble. Local gates: `bash -n os/bootc/render-screens.sh
os/hardware-gate/verify-shipping-status.sh`, `cargo fmt --all --check`, `cargo
test -p goblins-os-shell parses_text_shortcuts_proof_targets`, `cargo clippy
--workspace -- -D warnings`, `cargo test --workspace`, Rust 1.88 GTK container
`cargo clippy -p goblins-os-shell --features goblins-os-shell/native-desktop -- -D warnings`,
and `goblins-os-verify --source-root .` -> **blocked=0 (1945)**.
This is still CI/qemu-pending and does **not** mark Text Shortcuts shipped.

Current Text Shortcuts adapter-candidate metadata continuation: the Python IBus
adapter now parses the Rust stdio `candidate` object into a small
`CandidateMetadataState`, retains it only while the runtime reports a visible
candidate, clears it on Escape/commit/pass-through responses, and rejects any
candidate payload that claims `rendered_bubble_ready_claim=true`. The live
adapter stores this state for the future overlay path but still renders no
bubble and still applies only IBus preedit/delete/commit/hide operations. Local
gates: `python3 -m py_compile
os/goblins-os-textshortcuts/goblins-textshortcuts-ibus`, `python3
os/goblins-os-textshortcuts/goblins-textshortcuts-ibus --self-test`, `cargo
build -p goblins-os-textshortcuts-engine`,
`GOBLINS_TEXTSHORTCUTS_ENGINE="$PWD/target/debug/goblins-textshortcuts-engine"
python3 os/goblins-os-textshortcuts/goblins-textshortcuts-ibus --runtime-self-test`,
`cargo fmt --all --check`, `cargo test -p goblins-os-textshortcuts-engine`,
`cargo clippy --workspace -- -D warnings`, `cargo test --workspace`, and
`goblins-os-verify --source-root .` -> **blocked=0 (1949)**. This is still
CI/qemu-pending and does **not** mark Text Shortcuts shipped.

Current Text Shortcuts candidate-metadata hardware-proof continuation: the
display-backed capture/signoff harness now requires
`text-shortcuts-candidate-metadata-proof.json` beside the existing live
keystroke proof. The in-session orchestrator launches
`goblins-os-shell --text-shortcuts-proof candidate`, records
`candidate_replacement=on my way`, `candidate_accept_on=word-boundary`,
`candidate_dismiss_key=Escape`, `rendered_bubble_ready_claim=false`,
`live_overlay_claim=false`, and `runtime_ready_claim=false`, and
`run-capture.sh`, `close-signoff.sh`, `verify-shipping-status.sh`, the runbook,
and `goblins-os-verify` reject missing or overclaimed candidate metadata. Local
gates: `bash -n os/hardware-gate/capture-harness/in-session-orchestrator.sh
os/hardware-gate/capture-harness/run-capture.sh os/hardware-gate/close-signoff.sh
os/hardware-gate/verify-shipping-status.sh`, `python3 -m py_compile
os/hardware-gate/capture-harness/drive-capture.py`, `cargo fmt --all --check`,
`cargo clippy --workspace -- -D warnings`, `cargo test --workspace`, and
`goblins-os-verify --source-root .` -> **blocked=0 (1965)**. This is still
CI/qemu-pending and does **not** prove a live IBus overlay, focused-field
callback, Wayland text-input-v3 bubble, or mark Text Shortcuts shipped.

Current Text Shortcuts adapter-overlay intent continuation: the Python IBus
adapter now turns retained candidate metadata into a host-tested non-rendering
overlay intent ledger. It records `show-candidate` when a visible candidate
arrives, records `hide-candidate` with `reason=dismissed` or `reason=committed`
when Escape or boundary commit clears it, and every intent carries
`rendered_bubble_ready_claim=false`, `live_overlay_claim=false`, and
`runtime_ready_claim=false`. The live adapter still renders no bubble and still
applies only IBus preedit/delete/commit/hide operations. Local gates:
`cargo build -p goblins-os-textshortcuts-engine`, `python3 -m py_compile
os/goblins-os-textshortcuts/goblins-textshortcuts-ibus`, `python3
os/goblins-os-textshortcuts/goblins-textshortcuts-ibus --self-test`,
`GOBLINS_TEXTSHORTCUTS_ENGINE="$PWD/target/debug/goblins-textshortcuts-engine"
python3 os/goblins-os-textshortcuts/goblins-textshortcuts-ibus
--runtime-self-test`, `cargo fmt --all --check`, `cargo clippy --workspace -- -D warnings`, `cargo test --workspace`, and `goblins-os-verify --source-root .`
-> **blocked=0 (1970)**. This is still CI/qemu-pending and does **not** prove a
live IBus overlay, focused-field callback, Wayland text-input-v3 bubble, or mark
Text Shortcuts shipped.

Current Text Shortcuts overlay-intent proof-log continuation: the adapter now
has `--overlay-intent-self-test`, which launches the real Rust `--stdio` child,
drives candidate show, Escape dismiss, candidate show again, boundary commit,
and a password-purpose refusal, then emits JSON proof for the non-rendering
overlay intent contract. The image build stores that proof at
`/tmp/goblins-textshortcuts-overlay-intent.json` and asserts `status=pass`,
`surface=goblins-textshortcuts-ibus-adapter-overlay-intent`, `show_count=2`,
`hide_count=2`, `reason=dismissed`, `reason=committed`, and
`live_overlay_claim=false`; `goblins-os-verify` pins both the adapter command and
the Containerfile assertions. Local gates: `cargo build -p
goblins-os-textshortcuts-engine`, `python3 -m py_compile
os/goblins-os-textshortcuts/goblins-textshortcuts-ibus`, `python3
os/goblins-os-textshortcuts/goblins-textshortcuts-ibus --self-test`,
`GOBLINS_TEXTSHORTCUTS_ENGINE="$PWD/target/debug/goblins-textshortcuts-engine"
python3 os/goblins-os-textshortcuts/goblins-textshortcuts-ibus
--runtime-self-test`, `GOBLINS_TEXTSHORTCUTS_ENGINE="$PWD/target/debug/goblins-textshortcuts-engine"
python3 os/goblins-os-textshortcuts/goblins-textshortcuts-ibus
--overlay-intent-self-test`, `cargo fmt --all --check`, `cargo clippy
--workspace -- -D warnings`, `cargo test --workspace`, and
`goblins-os-verify --source-root .` -> **blocked=0 (1977)**. This is still
CI/qemu-pending and does **not** prove a live IBus overlay, focused-field
callback, Wayland text-input-v3 bubble, or mark Text Shortcuts shipped.

Current Text Shortcuts overlay-intent hardware-proof continuation: the
display-backed VM capture contract now requires
`text-shortcuts-overlay-intent-proof.json` in addition to the session,
live-keystroke, and candidate-metadata proofs. The in-session orchestrator runs
the installed `goblins-textshortcuts-ibus --overlay-intent-self-test`, posts a
proof only when the adapter records `show_count=2`, `hide_count=2`,
`dismissed_reason=true`, `committed_reason=true`, and keeps
`rendered_bubble_ready_claim=false`, `live_overlay_claim=false`, and
`runtime_ready_claim=false`; `drive-capture.py`, `run-capture.sh`,
`close-signoff.sh`, `verify-shipping-status.sh`, the runbook, and
`goblins-os-verify` all require the new artifact. Local gates: `bash -n` for
the capture orchestrator, run-capture, close-signoff, and shipping-status
scripts; `python3 -m py_compile os/hardware-gate/capture-harness/drive-capture.py`;
`cargo build -p goblins-os-textshortcuts-engine`; adapter `--self-test`,
`--runtime-self-test`, and `--overlay-intent-self-test` against the debug Rust
engine; `cargo fmt --all --check`; `git diff --check`;
`cargo clippy --workspace -- -D warnings`; `cargo test --workspace`; and
`goblins-os-verify --source-root .` -> **blocked=0 (1986)**. This is still
CI/qemu-pending and does **not** prove a rendered IBus overlay, focused-field
callback, Wayland text-input-v3 bubble, or mark Text Shortcuts shipped.

Current Text Shortcuts accept-bubble frame contract continuation: the installed
adapter now derives deterministic `goblins-textshortcuts-accept-bubble-frame`
show/hide frames from the existing overlay intents and reuses the
`gos-text-shortcuts-candidate` renderer style name, while keeping
`rendered_bubble_ready_claim=false`, `live_overlay_claim=false`, and
`runtime_ready_claim=false`. The image build runs
`goblins-textshortcuts-ibus --candidate-bubble-frame-self-test` and rejects the
proof unless it records two show frames, two hide frames, dismissed and committed
frames, sensitive-field refusal, and false render/live/runtime claims. This is
still CI/qemu-pending and does **not** prove a live rendered accept bubble.

Current Text Shortcuts accept-bubble frame hardware-proof continuation: the
display-backed VM capture contract now also requires
`text-shortcuts-candidate-bubble-frame-proof.json`. The in-session orchestrator
runs the installed `goblins-textshortcuts-ibus
--candidate-bubble-frame-self-test`, posts a proof only when the adapter records
two show frames, two hide frames, dismissed and committed frames, the
`gos-text-shortcuts-candidate` style contract, Inter font,
sensitive-field refusal, and keeps `rendered_bubble_ready_claim=false`,
`live_overlay_claim=false`, and `runtime_ready_claim=false`; `drive-capture.py`,
`run-capture.sh`, `close-signoff.sh`, `verify-shipping-status.sh`, the runbook,
and `goblins-os-verify` all require the new artifact. This is still
CI/qemu-pending and does **not** prove a live rendered accept bubble, focused
field callback, Wayland text-input-v3 bubble, or mark Text Shortcuts shipped.

Current Text Shortcuts accept-bubble layout contract continuation: the installed
adapter now derives deterministic `goblins-textshortcuts-accept-bubble-layout`
records from the existing accept-bubble frame records. The pure contract anchors
the future bubble to a focused cursor rect, clamps it inside the monitor, flips
above the cursor when the bottom edge would overflow, collapses hide frames, and
preserves the `gos-text-shortcuts-candidate`/Inter style contract while keeping
`rendered_bubble_ready_claim=false`, `live_overlay_claim=false`, and
`runtime_ready_claim=false`. The image build runs
`goblins-textshortcuts-ibus --candidate-bubble-layout-self-test` and rejects the
proof unless it records the below-cursor, right-edge clamp, bottom-edge flip, and
hide-collapse cases. This is still CI/qemu-pending and does **not** prove a live
rendered accept bubble, focused field callback, Wayland text-input-v3 bubble, or
mark Text Shortcuts shipped.

Current Text Shortcuts accept-bubble layout hardware-proof continuation: the
display-backed VM capture contract now also requires
`text-shortcuts-candidate-bubble-layout-proof.json`. The in-session orchestrator
runs the installed `goblins-textshortcuts-ibus
--candidate-bubble-layout-self-test`, posts a proof only when the adapter
records the layout surface, source frame surface, four layout records, three
visible layouts, right-edge clamp, bottom-edge flip, hide-frame collapse, the
`gos-text-shortcuts-candidate` style contract, Inter font, and keeps
`rendered_bubble_ready_claim=false`, `live_overlay_claim=false`, and
`runtime_ready_claim=false`; `drive-capture.py`, `run-capture.sh`,
`close-signoff.sh`, `verify-shipping-status.sh`, the runbook, and
`goblins-os-verify` all require the new artifact. Local source gates for this
proof-hook pass: `python3 -m py_compile` for the capture driver, `bash -n` for
the capture/signoff/shipping scripts, scoped `git diff --check`,
`cargo fmt --all --check`, `cargo clippy --workspace -- -D warnings`,
`cargo test --workspace`, and `goblins-os-verify --source-root .` →
**blocked=0 (2362)**. This is still CI/qemu-pending and does **not** prove a
live rendered accept bubble, focused field callback, Wayland text-input-v3
bubble, or mark Text Shortcuts shipped.

Current Text Shortcuts accept-bubble render-intent continuation: the Python IBus
adapter now emits deterministic `goblins-textshortcuts-accept-bubble-render-intent`
records from the existing overlay-intent, frame, and layout contracts into an
injectable fail-open sink. The source contract proves show/hide render intents
for visible candidates, Escape dismiss, word-boundary commit, focus-out cleanup,
sensitive-field hide, pass-through unchanged behavior, the Goblins candidate
style, Inter, and keeps `rendered_bubble_ready_claim=false`,
`live_overlay_claim=false`, and `runtime_ready_claim=false`. The boot image runs
`goblins-textshortcuts-ibus --candidate-bubble-render-intent-self-test` and
rejects the proof unless it records 8 render intents, 4 show intents, 4 hide
intents, dismissed/committed/focus-out/sensitive hide coverage, and sink-failure
fail-open behavior. The display-backed VM capture contract now also requires
`text-shortcuts-candidate-bubble-render-intent-proof.json`; the in-session
orchestrator posts it from the installed adapter self-test, `drive-capture.py`
persists it, `run-capture.sh`/`close-signoff.sh`/`verify-shipping-status.sh` and
`goblins-os-verify` all reject missing or failing proof, and the runbook names
the artifact. Local gates for this pass: Python pycompile for the adapter and
capture driver, `bash -n` for the capture/signoff/shipping scripts, all existing
adapter overlay/frame/layout/runtime/self tests plus the new render-intent
self-test against the local Rust engine, `git diff --check`,
`cargo fmt --all --check`, `cargo clippy --workspace -- -D warnings`,
`cargo test --workspace`, and `goblins-os-verify --source-root .` ->
**blocked=0 (2407)**. This is still CI/qemu-pending and does **not** prove a
live rendered accept bubble, focused field callback, Wayland text-input-v3
bubble, or mark Text Shortcuts shipped.

Current Text Shortcuts candidate-bubble render screenshot continuation: the
display-backed VM contract now requires a real qemu screenshot of the Goblins
candidate proof surface as `31-text-shortcuts-candidate-bubble-render.png` plus
`text-shortcuts-candidate-bubble-render-proof.json`. `goblins-os-shell
--text-shortcuts-proof candidate-render` writes a deterministic proof payload
linking that screenshot to the installed adapter render-intent, layout, and frame
contracts, the `gos-text-shortcuts-candidate` style classes, and Inter, while
keeping `rendered_bubble_ready_claim=false`, `live_overlay_claim=false`, and
`runtime_ready_claim=false`. The capture driver, run-capture guard,
`proof-manifest.json`, `close-signoff.sh`, `verify-shipping-status.sh`, runbook,
and `goblins-os-verify` now fail closed if the screenshot/proof is missing or
stale. Local gates for this pass: targeted
`cargo test -p goblins-os-shell parses_text_shortcuts_proof_targets`,
`cargo fmt --all --check`, `cargo clippy --workspace -- -D warnings`,
`cargo test --workspace`, helper `bash -n`, Python capture driver
`py_compile`, `git diff --check`, and
`goblins-os-verify --source-root .` -> **blocked=0 (2425)**. This is still
CI/qemu-pending and does **not** prove a live IBus overlay, focused-field
callback, Wayland text-input-v3 bubble, or mark Text Shortcuts shipped.

Current Text Shortcuts GI adapter contract continuation: the installed
`goblins-textshortcuts-ibus` adapter now has a
`--gi-adapter-contract-self-test` image-build proof that imports the real
`gi.repository.IBus` boundary, uses `IBus.Text.new_from_string` and
`IBus.keyval_to_unicode`, then drives the Rust stdio runtime through the
focus-in, process-key-event, set-content-type, focus-out, and reset callback
shape. The proof requires preedit update, boundary commit, Escape dismiss,
default pass-through, password/PIN refusal, and the existing render-intent
contract, while keeping `rendered_bubble_ready_claim=false`,
`live_overlay_claim=false`, `live_ibus_bus_claim=false`,
`text_input_v3_claim=false`, and `runtime_ready_claim=false`. The Containerfile
and `goblins-os-verify` fail closed if that contract or any false-claim guard is
missing. Local gates for this pass: `python3 -m py_compile
os/goblins-os-textshortcuts/goblins-textshortcuts-ibus`, helper `bash -n`,
`cargo fmt --all --check`, `cargo clippy --workspace -- -D warnings`,
`cargo test --workspace`, `git diff --check`, and
`goblins-os-verify --source-root .` → **blocked=0 (2465)**. The GI/IBus
self-test itself remains Fedora image-build gated, not locally run on this
macOS host. This is still source/image-gated and does **not** prove a live IBus
bus, a focused-field callback from a real application, a Wayland text-input-v3
commit, or mark Text Shortcuts shipped.

Current Text Shortcuts adapter callback ledger continuation: the installed
`goblins-textshortcuts-ibus` adapter now has an env-gated
`GOBLINS_TEXTSHORTCUTS_PROOF_EVENTS` callback ledger sink and an
`--adapter-callback-ledger-self-test` image-build proof. The ledger records only
callback categories, operation types, render-intent counts, style/font metadata,
and false readiness claims; it deliberately rejects proof output containing the
typed trigger or replacement value. The live `_run_ibus()` engine path wires the
same sink into `CandidateBubbleRenderIntentController`, and the Containerfile
plus `goblins-os-verify` fail closed if the proof, callback names, operation
types, redaction guard, or false live/runtime claims drift. Local gates for this
pass: `python3 -m py_compile
os/goblins-os-textshortcuts/goblins-textshortcuts-ibus`,
`GOBLINS_TEXTSHORTCUTS_ENGINE=target/debug/goblins-textshortcuts-engine python3
os/goblins-os-textshortcuts/goblins-textshortcuts-ibus
--adapter-callback-ledger-self-test`, `cargo fmt --all --check`,
`git diff --check`, `cargo clippy --workspace -- -D warnings`,
`cargo test --workspace`, and `goblins-os-verify --source-root .` ->
**blocked=0 (2499)**. This remains source/image-gated and does **not** prove a
live IBus bus, focused-field callback from a real application, Wayland
text-input-v3 commit, live rendered accept bubble, or mark Text Shortcuts
shipped.

Current Text Shortcuts live IBus runtime/render implementation continuation:
the display-backed VM harness now attempts the real in-session proof instead of
the old not-implemented fail proof. It seeds a deterministic `omw` -> `onmyway`
table, propagates `GOBLINS_TEXTSHORTCUTS_PROOF_EVENTS` into the user IBus
service, selects the active `goblins-textshortcuts` engine, launches
`goblins-os-shell --text-shortcuts-proof live-runtime-render`, drives host QMP
keyboard input, captures `32-text-shortcuts-live-ibus-runtime-render.png`, and requires the
redacted live adapter ledger to show focus/key callbacks, a `commit-text`
operation, render-intent show evidence, Inter, and
`gos-text-shortcuts-candidate`. The same proof then checks normal expansion to
`onmyway.`, pass-through of `hello.`, password-purpose refusal, and keeps
`core_readiness_flip=deferred`. Failures still post granular fail proofs, so
`drive-capture.py`, `run-capture.sh`, `close-signoff.sh`, and
`verify-shipping-status.sh` reject missing/partial artifacts. Local source
gates for this pass: `bash -n` for the edited shell scripts,
`cargo fmt --all --check`, `git diff --check`, targeted
`cargo test -p goblins-os-shell`, `cargo clippy --workspace -- -D warnings`,
`cargo test --workspace`, `goblins-os-verify --source-root .` ->
**blocked=0 (2525)**, and the Rust 1.88 GTK container
`cargo clippy -p goblins-os-shell --features goblins-os-shell/native-desktop -- -D warnings`.
This is still source/harness-gated and does **not** prove a live qemu session,
Wayland `text-input-v3`, a reviewed live rendered accept bubble, or mark Text
Shortcuts shipped.

Current Text Shortcuts Fedora 44 GI unicode compatibility continuation:
hardware-gate run `28336457879` at `87762b0` passed the hosted runner KVM
prerequisite but failed during the bootc image build before the VM launched.
The installed `goblins-textshortcuts-ibus --gi-adapter-contract-self-test`
exposed that Fedora 44 PyGObject/IBus returns a one-character string (`"o"`)
from `IBus.keyval_to_unicode`, while the adapter only accepted integer
codepoints. The adapter now accepts both integer codepoints and single-character
strings, keeps empty/non-single strings as `None`, and extends the pure
adapter self-test with a string-returning fake IBus binding. Local proof for
this unblock: `python3 -m py_compile
os/goblins-os-textshortcuts/goblins-textshortcuts-ibus`,
`python3 os/goblins-os-textshortcuts/goblins-textshortcuts-ibus --self-test`,
`cargo test -p goblins-os-textshortcuts-engine`, `cargo fmt --all --check`,
`cargo clippy --workspace -- -D warnings`, `cargo test --workspace`, and a
Fedora 44 container probe that imported `gi.repository.IBus` and showed
`IBus.keyval_to_unicode(ord("o"))` returns `str 'o'` while the patched
`_key_request` emits `"unicode": "o"` and Enter still emits `None`. This is
still source/image-gated and does **not** prove the live qemu IBus session or
mark Text Shortcuts shipped.

Current hardware-gate capture continuation: hardware-gate run `28336906753` at
`c10c89a` proved the GHCR image push and shippable installer ISO build, then
failed in the display-backed VM capture before any in-session proof reached the
host HTTP server. The diagnostic tail showed QMP alive, KVM readable/writable,
serial output still at the GRUB countdown/`Booting 'Install Goblins OS 44'`, and
an empty `httpd.log`; the old capture driver ignored both `wait_frame` return
values, so it could continue through blind clicks/Alt-F2 and report every proof
as missing. The current local fix exports `GOS_SERIALLOG`, waits for the ISO boot
menu/boot handoff serial markers, presses Enter to skip the GRUB timeout, makes
the Anaconda summary and first-boot desktop framebuffer waits required, and logs
framebuffer samples on timeout. `verify-shipping-status.sh` and
`goblins-os-verify` now pin those stage checks. This is source-gated only until
a fresh hardware-gate run proves the VM can reach the orchestrator and produce
the required live proof artifacts.

Follow-up hardware-gate run `28338292698` at `61ce761` again proved the GHCR
image push and shippable installer ISO build. The stricter capture driver caught
the next blocker: after observing the GRUB menu and pressing Enter, this QEMU/GRUB
path did not emit the old `Booting 'Install Goblins OS 44'` serial marker within
120 seconds, even though the serial screen cleared. The current local fix keeps
the boot-menu marker required, treats the boot-handoff marker as optional
diagnostic output, and continues to the required Anaconda framebuffer stage so
the next failure is tied to the real installer/desktop state rather than a GRUB
serial-string assumption. This is source-gated only until rerun.

Follow-up hardware-gate run `28339469677` at `5327994` again proved the GHCR
image push, shippable installer ISO build, and the optional GRUB handoff path,
then exposed that raw QEMU PPM byte size is not a valid screen-state detector:
the Anaconda stage timed out with a constant `3072016` byte framebuffer sample,
which reflects capture resolution rather than whether Anaconda is visible. The
current local fix removes the size-gated `require_frame` path, keeps the boot
menu marker required, records framebuffer hashes/screens only as diagnostic
`_debug-*` artifacts, waits a bounded Anaconda storage-confirmation interval,
then requires the real `GOBLINS_VERIFY_INSTALL_DONE` kickstart `%post` serial
marker before waiting for first boot. The hardware workflow also uploads
artifacts on failure, and the capture harness copies `qemu.log`, `serial.log`,
and `httpd.log` into `_capture-logs` so the next blocker is inspectable from the
artifact, not only the Actions tail. This is source-gated only until rerun.

Follow-up hardware-gate run `28341591378` at `a888bac` proved the GHCR image
push, shippable ISO build, model setup, artifact upload-on-failure path, and the
new diagnostic frame/log artifact path. The uploaded debug frame showed the VM
was on the branded Anaconda summary with `Installation Destination` marked
`Kickstart insufficient`, while `serial.log` never reached
`GOBLINS_VERIFY_INSTALL_DONE` and `httpd.log` stayed empty. The next source fix
made the Anaconda driver explicitly save destination-screen transition frames,
select the scratch disk, save the post-destination summary, and click the center
of the `Begin Installation` button instead of the page footer.

Follow-up hardware-gate run `28343942753` at `d78ebb7` proved the GHCR image
push, shippable ISO build, model setup, and all Anaconda transition debug
artifact uploads, but the destination-screen and disk-selected frames were still
the unchanged summary page. That means the QMP click events were not landing in
the installer UI. The next source attempt scaled QMP absolute pointer events
with the full `0x7fffffff` axis range in both the automated driver and manual
helper, then pinned that contract in `verify-shipping-status.sh` and
`goblins-os-verify`.

Follow-up hardware-gate run `28346403493` at `b9cc892` proved the GHCR image
push, shippable ISO build, model setup, and display-backed VM boot into
Anaconda, but the five Anaconda debug screenshots were byte-identical and
`serial.log` still contained only the GRUB menu. The attempted QMP pointer range
change was wrong: QEMU documents absolute coordinates as `0..0x7fff`, not
`0x7fffffff`, and the capture driver did not fail-close on QMP command errors.
The current local fix restores the documented absolute range, gives the
virtio-gpu device a stable `video0` id, routes pointer events to that display
device, logs `query-mice`, and makes QMP command errors fail loudly. This is
source-gated only until rerun.

Follow-up hardware-gate run `28349173698` at `9283b3f` proved the GHCR image
push, shippable ISO build, model setup, and display-backed VM boot into
Anaconda. The QMP diagnostic route initialized (`video0`; `query-mice` saw the
QEMU HID Tablet), but all five Anaconda debug screenshots were still
byte-identical on the branded summary with `Installation Destination` marked
`Kickstart insufficient`, `serial.log` never reached
`GOBLINS_VERIFY_INSTALL_DONE`, and `httpd.log` was empty. The next source fix
stops treating Anaconda pointer clicks as the install unblock: the
verification-only OEMDRV kickstart now pins the scratch VM disk with
`ignoredisk --only-use=vda`, `clearpart --drives=vda`,
`bootloader --boot-drive=vda`, and `part / ... --ondisk=vda`; the capture driver
waits for the real kickstart `%post` serial marker with periodic debug frames
instead of clicking Installation Destination / Begin Installation; and both
verifier gates reject drift back to the fragile click contract. This is
source-gated only until rerun.

Follow-up hardware-gate run `28351877832` at `e38b3e6` proved the GHCR image
push, shippable source-ref ISO build, model setup, QMP display input route, and
failure-artifact upload path, but it still stopped on the branded Anaconda
summary with `Installation Destination` marked `Kickstart insufficient`. The
debug frame hash matched the prior Anaconda-summary frame, `serial.log` never
reached `GOBLINS_VERIFY_INSTALL_DONE`, and `httpd.log` stayed empty. The source
fix in progress stops relying on an OEMDRV sidecar kickstart, because the
generated ISO boots Anaconda with `inst.ks=hd:LABEL=GOBLINS_OS:/osbuild.ks`.
The hardware workflow now builds its proof ISO with
`GOBLINS_OS_ISO_CONFIG=os/iso/verify-config.toml`, leaving release
`os/iso/config.toml` interactive/non-destructive while embedding the
verification-only scratch-`vda` storage automation into the ISO's `/osbuild.ks`.
This is source-gated only until rerun.

Follow-up hardware-gate run `28355883788` at `f271527` proved the GHCR image
push, verification ISO build with `os/iso/verify-config.toml`, display-backed
Anaconda install, reboot into the installed Goblins OS entry, and the
`GOBLINS_VERIFY_INSTALL_DONE` serial marker. The next blocker is first-boot
dismissal/orchestrator launch, not kickstart storage: the debug frame shows the
installed desktop in GNOME Overview with the Welcome window and login gate
visible, while `httpd.log` stayed empty and all in-session proof signals were
missing. The current local fix selects the Welcome window, clicks the
Private/offline path at the observed positions, and saves post-dismiss debug
frames before trying Alt-F2 orchestrator launch. This is source-gated only until
rerun.

Follow-up hardware-gate run `28359494431` at `1a7e9e7` proved the GHCR image
push, verification ISO build, display-backed install/reboot, first-boot
private/offline dismissal, Alt-F2 orchestrator launch, HTTP proof channel, and
28 screenshots. It still failed required live proof signals:
`firewall-live-toggle`, `text-shortcuts-session-enable`,
`text-shortcuts-live-keystroke`, `text-shortcuts-candidate-metadata`,
`text-shortcuts-candidate-bubble-render`,
`text-shortcuts-live-ibus-runtime-render`, `keyboard-shortcuts-roundtrip`,
`input-sources-roundtrip`, `focus-arm-roundtrip`, `app-privacy-revoke`, and
`preview-open-render`. The run did pass the Text Shortcuts overlay intent,
candidate frame, candidate layout, and render-intent proof hooks. The current
local fix adds a one-shot `InputSourcesSeed` user service before the IBus user
service so the installed session appends the Goblins IBus source and preload
engine once, without weakening the live IBus proof. This is source-gated only
until rerun.

Current local firewall bridge follow-up: the firewalld polkit rule is moving
from `/etc/polkit-1/rules.d` to the image-owned
`/usr/share/polkit-1/rules.d` path, while core accepts either the image-owned
rule or a local admin override and uses the stable `/usr/bin/systemctl` path
when starting the allowlisted helper instance. This targets the previous
`firewall-live-toggle` `manageable=false` proof failure, but Firewall remains
`in-progress` until a hardware-gate run proves disable/enable readback through
the live systemd/polkit path.

**NEXT — pick up exactly here:**
1. **Batch 4 implementation pass (current direction — CI/qemu at the end):**
   after source-gating the fail-closed capture-stage fix, rerun the
   display-backed hardware gate for the new Text Shortcuts live runtime/render
   path and inspect
   `text-shortcuts-live-ibus-runtime-render-proof.json` plus
   `32-text-shortcuts-live-ibus-runtime-render.png` only if the session reaches
   the in-guest IBus proof. Keep Text Shortcuts `in-progress` unless that proof
   shows the active engine, focused-field callback, `text-input-v3` commit,
   live rendered accept bubble, pass-through, password refusal, and deferred
   core readiness flip. Then continue the deferred engine UIs/overlays one
   feature at a time; do not add `whisper-cpp` as a CLI dependency until the
   actual Fedora 44 `whisper-cli` provider is proven.
2. **Deferred gated writes proof pass:** firewall CI image/render proof is green
   and the hardware-gate image/ISO path is past the export blocker; later
   push/dispatch the QMP-startup fix, inspect the display-backed VM logs if
   startup still fails, and inspect `firewall-live-toggle-proof.json` only if
   the session reaches the in-guest firewall toggle. That proof must show the
   live systemd/polkit oneshot success path for the firewall toggle. Only flip
   it to `shipped` if the render, live POST, and polkit oneshot path are green.
   Inspect `text-shortcuts-session-enable-proof.json` if the session reaches the
   in-guest IBus proof; it must remain a session-plumbing proof, not a live
   expansion claim. Then prove the IME set, Focus, per-app permission revoke,
   multi-display apply, keyboard rebinding, and Voice Control interactions in
   CI/qemu. Do not flip any of these from `in-progress` until the write path and
   qemu interaction proof are green.
3. **Batch 5 (Bucket D) LAST, qemu-gated:** FileVault-at-install, btrfs `/home` +
   snapshots — never blind-edit PAM/root-fs (use `authselect`); do under the hardware gate.

Each substrate follows the proven shape: **pure unit-tested core + honest capability
gating + no fake success**, GTK/engine deferred and marked in its ROADMAP entry. One
commit per feature; update its status here + add `goblins-os-verify` gates in lockstep.

---

## Bucket A — Quick & safe (package / config)

Low risk, high brand-impact. Real RPM binaries + the existing bridges; mostly host-testable logic with a thin CI/qemu render.

### `shipped` Live Text / OCR in screenshots & images
- [x] **Core capability shipped** (`crates/goblins-os-core/src/ocr.rs` + routes `/v1/ocr/status`, `/v1/ocr/recognize`; tesseract packaged; verify-gated): on-device Tesseract recognition, per-line bbox geometry from the TSV pass, honest-gated when the runtime/langpack is absent. Pure logic unit-tested on the host (4 tests).
- [x] **Screenshot → AI auto-OCR handoff shipped** (`crates/goblins-os-screenshot-context`): after capture it calls `/v1/ocr/recognize` over loopback and folds the recognized text into the model handoff summary (closing the "paste it yourself" gap). Host-compiled + 5 tests (ashpd/unix crate, no gtk); honest fallback to the plain summary when OCR is unavailable.
- [x] **Markup "Copy Text" action shipped** (`crates/goblins-os-markup`): a `.gos-markup-action` "Copy Text" button OCRs the source image via the local core (`/v1/ocr/recognize`) and copies the recognized text to the clipboard, off the UI loop via `gio::spawn_blocking` (no freeze) with honest "Recognizing…/No text found/Couldn't recognize" status. Pure request/response helpers unit-tested on the host (2 tests); compile- + `clippy -D warnings`-clean **and rustfmt-1.88-clean** in the native container; verify gate added. *(Selectable per-line overlay boxes remain an optional visual polish follow-up.)*
- **Packages:** `tesseract`, `tesseract-langpack-eng`, `leptonica` (all verified fc44; English OCR is **always** available — real baked binaries, no model download).
- **dconf:** none new. Reuse the existing `Super+Alt+V` `goblins-visual-context` binding (now auto-OCRs). OPTIONAL dedicated `<Super><Alt>t` `goblins-live-text` capture-to-clipboard entry. OCR language pref via env `GOBLINS_OS_OCR_LANGS` (not a schema), mirroring the voice env handling.
- **Files:** `crates/goblins-os-core/src/ocr.rs` (NEW — `recognize()` shelling `/usr/bin/tesseract`; `OcrOutcome{ok,text,lines,detail}` + `ocr_capability()`, modeled 1:1 on `voice.rs`), `crates/goblins-os-core/src/main.rs` (`mod ocr` + routes `/v1/ocr/status`, `/v1/ocr/recognize`), `crates/goblins-os-markup/src/main.rs` (`Copy Text` `.gos-markup-action` button; POST PNG, copy via `gdk::Display` clipboard, draw selectable per-line overlay boxes in the existing image-space cairo transform), `crates/goblins-os-screenshot-context/src/main.rs` (auto-OCR after capture; pass `GOBLINS_OS_SCREENSHOT_OCR_TEXT` to the launcher), `crates/goblins-os-launcher/src/main.rs` (consume OCR text in the VisualContext path), `os/bootc/Containerfile`, `crates/goblins-os-verify/src/main.rs` (gates: package, route, markup button, handoff, honest-gating).
- **APIs:** `tesseract <image> stdout -l eng --psm 3` + a `tsv` pass for per-line bbox geometry; axum get/post + Json; ashpd 0.13 portal `Screenshot`; GTK4 clipboard + cairo `ImageSurface` overlay.
- **Goblins-grade:** `.gos-markup-action` pill; selection boxes `alpha(@gos_accent,0.16)` fill / `alpha(@gos_accent,0.5)` border, 9px radius; status `.gos-markup-status`. Label **"Copy Text"** (macOS idiom) — never "OCR". Launcher framing **"Recognized on-device"**, no second hue.
- **Honest gating:** if `tesseract`/`eng` tessdata is somehow absent → `ok=false`, button shows "Text recognition is not available on this device." and copies nothing. Zero text → "No text found in this image." Non-eng langs gate on their langpack (opt-in add).
- **Verifiable:** host — `ocr_capability()`, `OcrOutcome` serde, tsv→lines/bbox parser, screenshot-context env wiring/copy. CI/qemu — markup overlay render + live tesseract shell-out.
- **Effort:** M · **Risk:** LOW-MED.

### `in-progress` Input sources / IME switching (CJK)
- [x] **Read substrate** (`crates/goblins-os-core/src/input.rs`): the `a(ss)` `org.gnome.desktop.input-sources sources` GVariant is parsed into ordered `InputSourceEntry` and surfaced in `/v1/input/status`. Pure parser unit-tested on the host.
- [x] **Settings list (GTK) shipped**: Settings ▸ Keyboard now renders a read-only **Input sources** list (friendly names via a unit-tested `input_source_label`, e.g. xkb `us` → "English (US)", ibus `libpinyin` → "Pinyin (Chinese)", honest raw-id fallback), with honest unavailable/empty rows. Compile- + `clippy -D warnings`-clean in the native container; 93 settings host tests; verify gate added.
- [x] **Set/reorder/remove substrate source-gated (CI/qemu-pending):** core exposes `/v1/input/sources`, validates only `xkb`/`ibus` source entries, encodes the `a(ss)` GVariant, and returns honest failure when gsettings or `org.gnome.desktop.input-sources sources` is absent. Settings ▸ Keyboard adds Move up / Move down / Remove row controls for existing configured sources only; the last source cannot be removed. Host tests cover `a(ss)` encode/decode, allowlist, reorder/remove, and the last-source rule; native GTK clippy passes in the Rust 1.88 container; verify gate added. **Not shipped** until CI/qemu proves render + interaction + live source switching.
- [x] **CJK engine package substrate source-gated (CI/qemu-pending):** Fedora 44 package metadata and a clean Fedora 44 install probe confirm `ibus-libpinyin`, `ibus-anthy`, `ibus-hangul`, and the existing `ibus-gtk4` module. The bootc image installs and `rpm -q` asserts the CJK engines, asserts `/usr/share/ibus/component/{libpinyin,anthy,hangul}.xml`, asserts `/usr/libexec/ibus-engine-{libpinyin,anthy,hangul}`, and asserts the GTK4 IBus module. Core reports a pure CJK engine package registry plus runtime path readiness; Settings ▸ Keyboard renders read-only CJK engine package rows. **Not shipped** until CI/qemu proves the installed image, Settings render, live IBus engine listing, source switching, and candidate window behavior.
- [x] **Menu-bar active-source indicator source-gated (CI/qemu-pending):** `goblins-menubar` reads GNOME's `org.gnome.desktop.input-sources` `sources/current` keys, hides itself when only one source is configured, hides rather than guessing if the schema/current key is not readable, and shows a compact Goblins-accent abbreviation chip for known XKB/IBus sources. The desktop render harness now captures `59-menubar-input-source-$suffix.png` after seeding `xkb/us` plus `xkb/gb` and switching current to index `1`, then restores the single-source state. **Not shipped** until CI/qemu proves the shell render and live source switching.
- [x] **Add input source sheet source-gated (CI/qemu-pending):** core exposes a narrow append-only `/v1/input/source` route that lists/adds only installed CJK IBus engines reported by `ibus list-engine` and not already configured; Settings ▸ Keyboard renders **Add input source…** choices from that core list. **Not shipped** until CI/qemu proves Settings render, installed-session `ibus list-engine`, real gsettings writes, menu-bar indicator update, source switching, and candidate-window behavior.
- [x] **Super+Space launcher handoff source-gated (CI/qemu-pending):** the seeded launcher binding calls `goblins-os-launcher --super-space`, which first asks core `/v1/input/switch-next` to rotate `org.gnome.desktop.input-sources current` only when more than one source exists. If switching is unavailable or unnecessary, the launcher opens normally. GNOME's stock switcher bindings stay empty so there is still only one owner of the key. **Not shipped** until CI/qemu proves live source switching and the launcher fallback path.
- [x] **Live input-source roundtrip hardware proof hook source-gated (CI/qemu-pending):** the display-backed capture harness now requires `input-sources-roundtrip-proof.json`, posts `/v1/input/sources` with `xkb/us` plus `xkb/gb`, verifies gsettings read-back, seeds current index `0`, posts `/v1/input/switch-next`, verifies current index `1`, restores the original source list/current index, links the proof in `proof-manifest.json`, and makes `close-signoff.sh`, `verify-shipping-status.sh`, and `goblins-os-verify` reject missing/failing proof, including screenshot-run completeness selection and missing-proof diagnostics. No live qemu run has produced the proof yet.
- [ ] **Deferred (risk-gated):** the Containerfile IME-env decision and live CJK candidate/input switching proof.
- **Packages:** `ibus-libpinyin`, `ibus-anthy`, `ibus-hangul`, `ibus-gtk4` (CJK engines verified fc44); `ibus-setup` remains a UI-picker question, not installed by the source-gated package substrate.
- **dconf/gsettings:** `org.gnome.desktop.input-sources` `sources` (`a(ss)`), `current`, `mru-sources`, `per-window`, `xkb-options`, `show-all-sources`; keep GNOME `switch-input-source`/`-backward` empty so the launcher remains the sole owner of `Super+Space`, and use `/v1/input/switch-next` from `goblins-os-launcher --super-space` to switch only when `sources.len() > 1`.
- **Files:** `os/bootc/Containerfile` (install/assert the CJK engine packages; keep the IME environment decision deferred), `os/dconf/db/local.d/10-goblins-os-desktop`, `crates/goblins-os-core/src/input.rs` (`INPUT_SOURCES_SCHEMA` + `a(ss)` encode/decode, CJK engine registry, list/add/remove/reorder/set-current, `ibus list-engine` probe, `/v1/input/switch-next` current-source rotation), `crates/goblins-os-core/src/main.rs` (extend existing `/v1/input/*` payloads), `crates/goblins-os-launcher/src/main.rs` (`--super-space` handoff/fallback), `crates/goblins-os-settings/src/main.rs` (real ordered-source list plus read-only engine package readiness, replacing the placeholder `input_source_summary_spec`), `os/gnome-shell-extensions/goblins-menubar@goblins.os/extension.js` (active-source abbreviation indicator when >1 source).
- **APIs:** `org.gnome.desktop.input-sources` (ships in gsettings-desktop-schemas), IBus D-Bus / `ibus` CLI, gnome-shell native `InputSourceManager` (we do **not** reimplement the candidate window — the engines render it), `ibus-gtk4` IM module.
- **Goblins-grade:** each source a `gos-row` (human name title, engine id copy, monospace abbreviation chip "PY/あ/한/US"); meaningful order via arrow/drag reorder; "Add input source…" sheet lists only installed engines; active source carries the calm accent selection language; candidate window themed via `os/gtk-4.0/gtk.css` to the rounded vibrant Goblins material.
- **Honest gating:** session absent → existing "not ready" copy, controls disabled; engine not installed → never listed; single source → zero new chrome (menu-bar indicator + binding only when `sources.len() > 1`); last source can't be removed.
- **Verifiable:** host — `a(ss)` encode/decode, allowlist, reorder/remove validation, last-source rule, >1 gating predicate. CI/qemu — package install, Settings render, menu-bar indicator, real switching, candidate window.
- **Effort:** L · **Risk:** HIGHEST in bucket (reverses an intentional boot/login + `Super+Space` decision). Gate IBus IM modules to engage cleanly at `sources>1`; keep `gtk-im-context-simple` the single-source default.

### `shipped` System color picker / eyedropper
- [x] **Shipped (`crates/goblins-os-color-picker`, headless, fully host-verified):** `<Super><Alt>c` runs the portal eyedropper (GNOME's magnified loupe); the sampled sRGB is formatted HEX / rgb() / hsl(), HEX copied via `wl-copy`, with a calm toast showing all three. Pure color-math (incl. sRGB→HSL) unit-tested on the host (3 tests); the whole flow compiles + tests on macOS (ashpd, no gtk). `wl-clipboard` packaged, binary COPY'd, keybinding seeded, 3 verify gates. Honest-gated: portal declined/absent → nothing copied, clear stderr; no `wl-copy` → value still printed.
- [ ] Optional enhancement (CI-gated): a branded Goblins swatch panel with one-click HEX/RGB/HSL cycling (today the toast shows all three).
- **Packages:** `wl-clipboard`.
- **dconf:** append `goblins-color-picker` to the media-keys `custom-keybindings` array; stanza `command=/usr/libexec/goblins-os/goblins-os-color-picker`, `binding=<Super><Alt>c` (free; `<Alt>` avoids the screenshot-clip `<Control>` conflict).
- **Files:** `crates/goblins-os-color-picker/{Cargo.toml,src/main.rs}` (NEW — headless launcher: ashpd `Color::pick()`, sRGB f64 → `#RRGGBB`/`rgb()`/`hsl()`, pipe to `wl-copy`, spawn swatch panel), workspace `Cargo.toml`, `os/bootc/Containerfile` (`wl-clipboard` + COPY binary to `/usr/libexec/goblins-os/`), `os/dconf/db/local.d/10-goblins-os-desktop`, `crates/goblins-os-verify/src/main.rs`, `crates/goblins-os-settings/src/main.rs` (OPTIONAL shortcut row).
- **APIs:** portal `Screenshot.PickColor` → `(ddd)` sRGB doubles in `[0,1]` (GNOME-implemented); `ashpd::desktop::Color::pick()`; `wl-copy`; GTK4 swatch panel via `native_css()`.
- **Goblins-grade:** GNOME portal's own magnified loupe (Wayland-correct, no compositor hacks) → small Goblins swatch panel: large rounded chip (radius 12), hex in `GOS_TYPE_TITLE_3` mono, `rgb()`/`hsl()` in footnote muted, single neutral "Copied to clipboard" status line, segmented HEX/RGB/HSL control re-copying on select; overlay radius 22, `MOTION_OVERLAY_MS` fade, accent only on the active segment; auto-dismiss on timeout/Escape.
- **Honest gating:** portal absent/declined/timed-out → "Color picker unavailable — the desktop portal did not respond. Nothing was copied." `wl-copy` missing → still show hex with "Could not copy automatically — value shown above." User-cancel → silent exit, no panel. Headless-first: clipboard write succeeds even if GTK init fails.
- **Verifiable:** host — sRGB→hex rounding/clamp, `rgb()`/`hsl()` formatting, round-trip + boundaries (0.0→00, 1.0→ff), format-cycle strings. CI/qemu — portal handshake, `wl-copy`, swatch render.
- **Effort:** M · **Risk:** LOW (boot untouched; hotkey-launched libexec).

### `in-progress` PDF / image Preview viewer
- [x] **Package/default-app substrate source-gated (CI/qemu-pending):** Fedora 44 repo metadata confirms `papers` (`/usr/bin/papers`, `org.gnome.Papers.desktop`) and `loupe` (`/usr/bin/loupe`, `org.gnome.Loupe.desktop`). The bootc image installs and `rpm -q`/`command -v` asserts both packages, and `os/applications/mimeapps.list` makes PDFs open in Papers and common image formats open in Loupe. This is not shipped until CI/qemu proves the installed desktop entries, MIME open behavior, and themed render.
- [x] **Open/status substrate source-gated (CI/qemu-pending):** core exposes `/v1/preview/status` and `/v1/preview/open`, validates only absolute local PDF/PostScript/common image files, requires `xdg-open` plus Papers/Loupe before launch, and hands files to the desktop default viewer without reading file contents or claiming rendered proof. Host tests cover the extension allowlist and honest missing-viewer states; the installed-image self-test now checks status readiness, opens temporary PDF/image files through the route, and rejects an unsupported file honestly. `goblins-os-verify` and the hardware gate check the route + xdg-open + selftest contract. This is not shipped until CI/qemu proves installed-image MIME open behavior and themed render.
- [x] **Display-backed open/render proof hook source-gated (CI/qemu-pending):** the hardware-gate capture harness now requires `preview-open-render-proof.json`, opens installed `/usr/share/goblins-os/proof/preview-open-render.{pdf,png}` fixtures through `/v1/preview/open`, verifies Papers/Loupe MIME defaults and viewer processes, captures `29-preview-pdf-open.png` and `30-preview-image-open.png`, rejects an unsupported `.txt` fixture, links the proof in `proof-manifest.json`, and makes `close-signoff.sh`, `verify-shipping-status.sh`, and `goblins-os-verify` reject missing/failing proof. No live qemu run has produced reviewed artifacts yet.
- [ ] Open any PDF/image as the default viewer (macOS Preview altitude — view, page, basic annotate; not a deep editor). The Goblins markup editor already covers screenshot annotation; this fills the "double-click a PDF" gap.
- **Approach:** themed_gnome_fallback (deep long tail — a stock GNOME viewer branded via `os/gtk-4.0/gtk.css`, not a custom build) for v1; a Goblins-native viewer is a later option.
- **Packages:** `papers` (GNOME Documents, verified in Fedora 44 repo metadata; `evince` is not used here) + `loupe` (GNOME Image Viewer, verified in Fedora 44 repo metadata).
- **Files:** `os/bootc/Containerfile` (package + `rpm -q`), default-application dconf / mimeapps so PDFs/images open in it, `os/gtk-4.0/gtk.css` (already brands stock GTK apps — confirm coverage).
- **Goblins-grade:** branded via the gtk.css bridge (window/headerbar/sidebar/accent in Goblins tokens). Honest gating: n/a (a viewer is always present once packaged).
- **Verifiable:** CI/qemu only (package + render). **Effort:** S · **Risk:** LOW once the package name is confirmed.
- _Note: spec agent connection-failed; package name + mimeapps wiring must be web-verified before building._

### `in-progress` Fingerprint unlock (Touch ID analogue)
- [x] **Package/PAM/status substrate source-gated (CI/qemu/hardware-pending):** Fedora 44 repo metadata and a `fedora-bootc:44` command test confirm `authselect`, `fprintd`, `fprintd-pam`, and `libfprint`, with `with-fingerprint` available on the bootc base's `local` authselect profile. The image installs + `rpm -q` asserts those packages, asserts the fprintd CLIs and `pam_fprintd.so`, enables fingerprint PAM through `authselect enable-feature with-fingerprint`, and core exposes `/v1/fingerprint/status` with honest fprintd/PAM/authselect/reader/enrollment gates. Settings ▸ Security shows a read-only Fingerprint unlock row. No enroll/delete UI or live auth proof is claimed yet.
- [ ] Enroll a fingerprint and unlock the session / authorize sudo with it (laptop readers). Secure-Enclave parity is HW-bound; generic `fprintd` reader support is the achievable, real win.
- **Approach:** custom_surface (a Goblins "Fingerprint" enrollment flow in Settings ▸ Security on the `fprintd` D-Bus) + config (PAM via `authselect`).
- **Packages:** `authselect`, `fprintd`, `fprintd-pam`, `libfprint` (verified fc44; `with-fingerprint` verified in the bootc base's `local` profile).
- **Files:** `os/bootc/Containerfile` (packages); PAM enablement via **`authselect` feature** (e.g. `with-fingerprint`) — NOT hand-edited PAM stacks (login-critical; a bad PAM edit locks users out); `crates/goblins-os-settings/src/main.rs` (enroll/remove rows on `net.reactivated.Fprint` D-Bus); `crates/goblins-os-verify/src/main.rs` (gate the authselect profile + packages).
- **APIs:** `net.reactivated.Fprint` D-Bus (Device.EnrollStart/VerifyStart); `authselect`.
- **Honest gating:** no reader detected → enrollment hidden/disabled with "No fingerprint reader found on this device."; password always remains a fallback.
- **Verifiable:** host — D-Bus payload/enroll-state logic. CI/qemu + real hardware — actual enroll/verify (no reader in CI → gate the daemon + authselect profile, not live enroll).
- **Effort:** M · **Risk:** MED-HIGH (PAM/login path — only via authselect, never blind PAM edits).
- _Note: spec agent connection-failed; authselect feature name + fc44 packages must be web-verified before building._

---

## Bucket B — Own-surface UI (GTK / shell — CI/qemu-validated)

Goblins-branded rows/cards on existing stable seams. Logic host-testable; render and writes proven in CI/qemu.

### `shipped` Branded Accessibility panel rows
- [x] **Core bridge** (`crates/goblins-os-core/src/accessibility.rs`): high contrast (`a11y.interface`), sticky/slow/bounce/mouse keys (`a11y.keyboard`), dwell click (`a11y.mouse`) read in `/v1/accessibility/status` + settable via `/v1/accessibility/preference` through the allowlisted, type-checked bridge — honest-gated per schema. Unit-tested on the host.
- [x] **GTK Settings rows** (`crates/goblins-os-settings`): Contrast / Typing assistance / Pointer assistance groups via `append_accessibility_bool_row`, with honest "unavailable" rows when a schema is absent. **Compile- + `clippy -D warnings`-clean in a Linux container** (the local native-build loop), host tests green (92), verify gate added.
- [x] **Magnifier zoom/lens controls source-gated (CI/qemu-pending):** core reads/writes `org.gnome.desktop.a11y.magnifier` `mag-factor`/`lens-mode` through the same allowlisted preference bridge, clamps zoom to 1.0x-8.0x in 0.25x steps, and Settings only exposes active controls when `screen-magnifier-enabled=true`; otherwise it renders honest read-only copy. GTK render and live GNOME magnifier writes remain CI/qemu-pending.
- **Packages:** none (schemas ship in gsettings-desktop-schemas, pulled by gnome-control-center).
- **gsettings:** `org.gnome.desktop.a11y.interface high-contrast`; `…a11y.keyboard` stickykeys/slowkeys(+delay)/bouncekeys(+delay)/mousekeys(+max-speed/init-delay/accel-time); `…a11y.mouse` dwell-click-enabled/dwell-time(`d`)/dwell-threshold/secondary-click-enabled/secondary-click-time(`d`); `…a11y.magnifier` mag-factor(`d`)/lens-mode/screen-position; gated by existing `…a11y.applications screen-magnifier-enabled`.
- **Files:** `crates/goblins-os-core/src/accessibility.rs` (new `AccessibilityPreferenceTarget` arms + normalizers/clamps), `crates/goblins-os-settings/src/main.rs` (new "Contrast"/"Typing assistance"/"Pointer assistance"/"Magnifier" groups via existing `switch_row_dynamic`/`slider_row`/`append_accessibility_bool_row`), `crates/goblins-os-design/src/lib.rs` (only if a new label fn is needed; reuse `gos-subsection-title` + `gos-switch-row` first).
- **APIs:** existing `accessibility.rs::gsettings()` bridge + mounted routes `GET /v1/accessibility/status`, `POST /v1/accessibility/preference`; GNOME consumers (gnome-settings-daemon, mutter magnifier) enforce — we only write.
- **Goblins-grade:** reuse `slider_row` + plain-English label fns (`milliseconds_label` for delays; add seconds + x-factor fns); calm honest detail copy; normalize/clamp every numeric in core so slider and stored value never drift.
- **Honest gating:** per-schema `schema_snapshot` + `has_key`; `None` → `system_row` "not available in the current desktop session"; magnifier sliders gate on availability **and** `screen-magnifier-enabled=true` ("Turn on Magnifier to adjust zoom"); use the existing `U32`/`F64` value kinds (no new signed/enum path — use the dedicated `lens-mode` (b) key, leave `screen-position` to the gnome-control-center handoff).
- **Verifiable:** host — target arms, specs, normalizers, type-check (extend `bounds_are_stable`). CI/qemu — row layout + real gsettings writes.
- **Effort:** L · **Risk:** LOW (runtime reads, no rpm install). No boot/login surface.

### `in-progress` Firewall toggle + status (firewalld) in Settings ▸ Security
- [x] **Status read** (`crates/goblins-os-core/src/firewall.rs` + `/v1/firewall/status`): honest read-only posture via `firewall-cmd --state` (running requires success AND "running" text — pure, unit-tested), honest-gated to "unavailable" when firewalld isn't installed.
- [x] **Settings row (GTK) shipped**: Settings ▸ Security ▸ Protection now shows a live **Firewall** row (on / off / unavailable) fed by the status endpoint, alongside the boot-image + keyring rows. Compile- + `clippy -D warnings`-clean in the native container; verify gate added.
- [x] **Gated On/Off toggle substrate + Settings binding (CI/qemu interaction proof pending):** core writes only by starting `goblins-os-firewall@enable/disable.service`, with a root helper that touches only `firewalld.service`, a scoped polkit rule for the `goblins-os` service user, image-time helper/unit/rule assertions, an installed-image self-test that exercises status + honest toggle outcomes, and a GTK switch that disables/reverts honestly when the bridge or live write fails. Feature remains `in-progress` until qemu render + live toggle proof are green.
- **Packages:** `firewalld` (verified canonical name; minimal/bootc images can omit it).
- **Files:** `crates/goblins-os-core/src/firewall.rs` (status + toggle, mirror `bluetooth.rs`), `crates/goblins-os-core/src/main.rs` (`GET /v1/firewall/status`, `POST /v1/firewall/enabled`), `crates/goblins-os-settings/src/main.rs` (`FirewallStatus` + `build_security` row + `set_firewall_enabled` mirroring `set_bluetooth_power`), `os/bootc/Containerfile` (`firewalld` + `systemctl enable firewalld.service`), `os/bootc/goblins-os-firewall` + `os/systemd-system/goblins-os-firewall@.service` + `os/bootc/60-goblins-os-firewall.rules` (privileged helper/oneshot plus **scoped** polkit rule).
- **APIs:** read path `firewall-cmd --state`/`--get-default-zone` + `systemctl is-active/is-enabled` (all unprivileged for the active session); write path via the oneshot helper.
- **Goblins-grade:** "Network protection" subsection in `build_security`; status pill on/off/checking, detail "The firewall blocks unwanted incoming connections. Zone: <default-zone>."; `gtk4::Switch` `gos-switch`, insensitive during in-flight POST, revert on failure; neutral plain-text tone, no new colors.
- **Honest gating (verified blocker):** core runs `User=goblins-os` + `NoNewPrivileges` + `ProtectSystem=strict`; firewalld write polkit default is `auth_admin_keep` → a direct `firewall-cmd` write hits a non-interactive denial. **Ship status read NOW**; for the toggle, the proper path is the root oneshot triggered over the system bus, gated by a polkit rule scoped to `unit==goblins-os-firewall@*.service`. Until that rule lands, render the toggle **disabled**: "Turning the firewall on or off is managed by the system." POST outcome reflects the real exit code (BAD_GATEWAY on failure). `firewall-cmd` absent → "Firewall service is not ready on this device."
- **Verifiable:** host — status-string mapping, absent-binary gate, failure-outcome code, truthful-copy assertion. CI/qemu — toggle render, live calls, polkit/oneshot path.
- **Effort:** M · **Risk:** MED. Keep the default zone as shipped (firewalld can interfere with NetworkManager/netavark); never author custom rules; scope the polkit rule to the single unit glob.

### `in-progress` Personal Hotspot toggle (Settings ▸ Network)
- [x] **Status read + row shipped** (`crates/goblins-os-core/src/hotspot.rs` + `/v1/hotspot/status`, Settings ▸ Network "Personal Hotspot" row): detects an active Wi-Fi access-point connection via `nmcli` (UUID-keyed lookup → no name-escaping; pure `active_wifi_devices`/`mode_is_ap` helpers unit-tested, 174 core tests), honest-gated to "unavailable" without NetworkManager. Container-verified (clippy `-D warnings`), verify gates added.
- [x] **Gated start/stop core substrate source-gated (CI/qemu-pending):** `/v1/hotspot/enabled` is policy-gated by `settings-control`, validates SSID/password before `nmcli`, requires `dnsmasq` for NetworkManager shared mode, rejects no-AP-adapter and single-radio Wi-Fi-uplink states, uses a non-persistent `save no` AP profile, removes the fixed "Goblins Hotspot" profile on stop/failure, and sanitizes errors so the PSK never leaks. The image installs and `rpm -q`/`command -v` asserts `dnsmasq`.
- [x] **Settings binding source-gated (CI/qemu-pending):** Settings ▸ Network now has the Personal Hotspot switch plus editable SSID/password rows, validates bad SSIDs and password length before POST, sends passwords only to core, clears the password after a successful request, and reverts on the real core outcome instead of faking success.
- [x] **Connected-client readout source-gated (CI/qemu-pending):** core parses NetworkManager/dnsmasq lease tables for the active hotspot device and Settings shows a connected-device count/list only when lease data is present. Missing lease data remains unknown instead of reporting a false zero.
- [ ] **Live AP proof + connected clients (deferred):** prove the radio can become a WPA2/WPA3 AP sharing the uplink, validate the connected-devices readout against live NetworkManager shared-mode leases, and capture qemu/live-device proof.
- **Packages:** `dnsmasq` (verified `2.92rel2-9.fc44`; **mandatory** — `ipv4.method shared` needs it for DHCP/NAT, not pulled by NetworkManager-wifi).
- **Files:** `crates/goblins-os-core/src/hotspot.rs` (NEW — nmcli status/start/stop, SSID + password validation, uplink/single-radio gating, PSK error sanitization, tests), `crates/goblins-os-core/src/main.rs` (`mod hotspot` + `/v1/network/hotspot/status`, `/v1/network/hotspot`), `crates/goblins-os-settings/src/main.rs` (`append_hotspot_management` in `build_network`, modeled on `append_bluetooth_power_control`; `HotspotStatus` + `set_hotspot`), `os/bootc/Containerfile` (`dnsmasq`).
- **APIs:** `nmcli` AP profile (`802-11-wireless.mode ap`, `band bg`, `ipv4.method shared`, `wifi-sec.key-mgmt wpa-psk`/`sae`, `wifi-sec.psk`), reusing `network.rs` `split_terse` + `policy_state_for_control("settings-control")`; GTK4 `Switch`/`Entry`/`PasswordEntry`.
- **Goblins-grade:** "Personal Hotspot" subsection; prominent switch-row whose copy flips by state; an inset card with Network name + Password rows (disabled while live; edits apply on next enable, matching macOS); when ON, `health_row` status pills for client count / SSID / shared uplink. Copy: "Passwords are used once to configure the hotspot and are never shown here."
- **Honest gating (4 gates):** nmcli missing → "Networking is not ready in this session…"; no AP-capable adapter → "This device has no Wi-Fi adapter that can broadcast a hotspot"; **the macOS-parity gate** — Wi-Fi is the only uplink on a single radio → disabled "Connect to the internet over Ethernet to share it over Wi-Fi"; policy denies `settings-control` → 403. Password `<8` rejected pre-nmcli; SSID `-`-prefix rejected, length-capped 32; connect errors sanitized so the PSK never leaks.
- **Verifiable:** host — SSID/password validation, PSK-leak sanitization, single-radio/uplink decision, terse parsing. CI/qemu — panel render + live AP (needs a virtual/passed-through Wi-Fi device).
- **Effort:** M · **Risk:** MED. Route writes through policy (no ungated path); start/stop idempotent (fixed con-name "Goblins Hotspot"); never persist the PSK.

### `shipped` Hot Corners
- [x] **Opt-in hot corners shipped** (`goblins-wm@goblins.os`): four `hot-corner-{top,bottom}-{left,right}` gschema keys (`s`, choices `none`/`mission-control`/`app-expose`, **default `none`** so nothing changes until opted in — macOS-style). Each enabled corner gets a tiny reactive actor (`addChrome`) that triggers the action on pointer entry, rebuilt on settings change, fully torn down on disable. Verified with `node --check`, `glib-compile-schemas`, verify gates, and CI/qemu desktop artifacts from build run `28287964440`: `52c-wm-hot-corner-{light,dark}.png` on both `x86_64` and `aarch64`.
- [x] **Settings chooser source-gated (CI/qemu-pending):** core exposes `/v1/window-management/status` and `/v1/window-management/hot-corner`, writes only the four allowlisted Goblins WM hot-corner keys, validates the existing action registry, and Settings ▸ Multitasking renders four chooser rows from core status. Render, live writes, and shell dispatch remain CI/qemu-pending.
- [ ] Optional polish: more corner actions (Show Desktop, Control/Notification Center, Lock), a modifier-key guard; set `org.gnome.desktop.interface enable-hot-corners=false` in dconf if GNOME's built-in corner ever conflicts.
- **Packages:** none.
- **gsettings:** EXTEND `org.goblins.shell.extensions.wm` — add `HotCornerAction` enum + `hot-corner-{top,bottom}-{left,right}` (`s`, default 'none'), `hot-corner-modifier` (none/super/ctrl/alt/shift), `hot-corners-enabled` (b). SET `org.gnome.desktop.interface enable-hot-corners=false` in dconf so GNOME's built-in corner doesn't fight the barriers.
- **Files:** `…/goblins-wm@goblins.os/schemas/…wm.gschema.xml` (enum + 6 keys), `…/goblins-wm@goblins.os/extension.js` (self-contained `HotCorners` manager: pressure barriers + guarded dispatch), `os/dconf/db/local.d/10-goblins-os-desktop`, `crates/goblins-os-settings/src/main.rs` (replace the read-only Multitasking "Hot corner" row with a live four-corner DropDown surface), `crates/goblins-os-core/src/window_management.rs` (NEW allowlisted gsettings bridge), `crates/goblins-os-core/src/lib.rs` (module + routes).
- **APIs:** `Meta.Barrier` (**GNOME 47+ constructor takes `backend:`, not `display:`** — the key compatibility caveat; metadata declares 46-50), `Layout.PressureBarrier` (debounces/re-arms like GNOME's own corner), `monitors-changed` rebuild, `globalThis.goblinsWindowManager` for native-surface actions, `loginctl lock-session`/busctl for lock/sleep.
- **Goblins-grade:** Settings card with a mock-desktop preview (radius 12, wallpaper tint) + four corner chips + four DropDowns (`.gos-combo`, height 38) + a "Require modifier" row; selected corner highlights with the flat desaturated accent; writes go through the bridge, never raw schema writes. Triggered surfaces are already Goblins-native.
- **Honest gating:** wm extension absent → dispatch no-ops, Settings shows "Hot corners need the Goblins window manager session"; backend unavailable (no screensaver/loginctl) → that option disabled; bridge reports `gsettings_available`/`schema_available`; unresolved multi-monitor geometry → corners stay disabled (never wrong-coordinate barriers).
- **Verifiable:** host — enum↔nick mapping, allowlist, request parsing, outcome strings; gschema `--dry-run`. CI/qemu — barrier/dispatch + the Settings card (Multitasking-panel render + a new interaction render).
- **Effort:** L · **Risk:** MED (barrier code runs in gnome-shell — wrap every dispatch in try/catch, tear down barriers in `disable()`, target `backend:` for 47+, fail-closed on any error).

### `shipped` Snap Assist (second-half chooser)
- [x] **Chooser shipped** (`goblins-wm@goblins.os`): after a `_snapWindow` half-snap, a self-contained overlay on the empty half lists the other usable windows; picking one snaps it to the complementary zone, Esc / a 4s timeout / a pick dismiss it. Guarded by the new `snap-assist` boolean (default true), recursion-guarded (assist-initiated snaps never re-trigger), and fully isolated/try-catch-wrapped so it can never break core snapping. Goblins-styled (`.goblins-wm-snap-assist*` in the existing palette). Verified with `node --check`, `glib-compile-schemas`, verify gates, and CI/qemu desktop artifacts from build run `28287964440`: `55-wm-snap-assist-{light,dark}.png` on both `x86_64` and `aarch64`.
- [ ] Optional polish: live window-thumbnail previews in the chooser (currently app + title rows), and a 4-finger/edge-drag trigger.
- **Packages:** none.
- **gsettings:** NEW `snap-assist` boolean (default true) in `…wm.gschema.xml`, recompiled by the existing `Containerfile:288` step. Reads existing `color-scheme` (light/dark) and `enable-animations` (reduced-motion). No new dconf seed.
- **Files:** `…/goblins-wm@goblins.os/extension.js` (`_snapAssist` surface wired into `_snapWindow`'s apply-timeout callback; reuse `_windowEntries`/`_thumbnail`/`_createOverlay`, scoped to the empty-half rect from `_rectForZone`), `…/stylesheet.css` (`.goblins-wm-snap-assist*` for `.dark` + `.light`), `…/schemas/…wm.gschema.xml`, `crates/goblins-os-design/src/lib.rs` (no change — the new CSS **must** use `GOS_CHROME_ACCENT_RGBA_PREFIX = 'rgba(0, 145, 255'` or the `chrome_stylesheets_pin_to_the_one_canonical_accent` test at lib.rs:2992 fails the whole Rust gate).
- **APIs:** `Clutter.Clone` over `global.get_window_actors()` (live thumbnails); `Main.layoutManager.addChrome({affectsStruts:false})`; `grab_key_focus` + key-press for Esc/Return/arrows (no `pushModal`); `GLib.timeout_add` for the post-snap defer; `Gio.Settings.get_boolean('snap-assist')` gate.
- **Goblins-grade:** vibrancy panel inside the empty half (inset ~10px, radius 22); cards = live thumbnails + app-icon/title row; the **three-state selection language already pinned** (hover white wash / accent-ring focus / accent-fill selected); 180ms fade-in + spring-on-arrival, honoring `enable-animations`; light/dark via `_schemeClass()`; anchor to the snapped window's monitor work area.
- **Honest gating:** zero other usable windows → **skip the chooser** (no hollow panel); `snap-assist=false` → plain half-tiling; zero-size actor → text placeholder; reduced-motion → clean cut; auto-dismiss on focus loss / workspace / monitor change.
- **Verifiable:** host — `cargo test -p goblins-os-design` accent-pin; gschema `--dry-run`; `node --check`. CI/qemu — chooser render, live clones, selection flow, second-half fill.
- **Effort:** M · **Risk:** MED (boot NOT affected — session extension; failure = chooser doesn't appear). Wrong gschema type bricks the schema compile → mirror the existing boolean-key form.

### `shipped` App Exposé (single-app Mission Control)
- [x] **Keyboard App Exposé shipped** (`goblins-wm@goblins.os`): `_showAppExpose` resolves the focused app via `Shell.WindowTracker` and reuses the Mission Control overlay pre-filtered to that app (the existing per-app rail filter; `hide()` clears it), titled with the app name. New `app-expose` gschema key (`['<Super>e', 'F10']` — F10 mirrors macOS, avoids the taken `<Super>Down`). Verified with `node --check`, `glib-compile-schemas`, no binding conflicts, verify gates, and CI/qemu desktop artifacts from build run `28287964440`: `52b-wm-app-expose-{light,dark}.png` on both `x86_64` and `aarch64`.
- [ ] Optional polish: 4-finger swipe-down (`Clutter.SwipeAction`), dock-icon-click → expose, and the window HUD entry.
- **Packages:** none (pure JS/CSS/gschema in an already-shipped extension — zero image-build risk).
- **gsettings:** NEW `app-expose` (`as`, default `['<Control>Down', 'F10']`) in `…wm.gschema.xml` — chosen to avoid the existing `<Super>Down` restore-window binding. Optional 4-finger swipe is JS-wired (`Clutter.SwipeAction`), no dconf key. Reads existing `color-scheme`/`enable-animations`.
- **Files:** `…/goblins-wm@goblins.os/extension.js` (`_showAppExpose`, `_appExposeEntries`, focused-app resolver via `Shell.WindowTracker.get_window_app(global.display.focus_window)`, recent-docs bottom strip, `showAppExposeDemo()` hook), `…/schemas/…wm.gschema.xml`, `…/stylesheet.css` (`.goblins-wm-app-expose*` + `.light`), `…/goblins-dock@goblins.os/extension.js` (dock-icon click → expose when RUNNING + `>1` window + already focused; else `activate()`), `os/bootc/render-desktop.sh` (`52b-wm-app-expose-$suffix.png` capture, light+dark).
- **APIs:** `Shell.App.get_windows()` (MRU/stacking order — the authoritative single-app enumeration), `Clutter.Clone`, `Main.wm.addKeybinding`, `Clutter.SwipeAction` (optional, feature-detected), `Meta.Window.activate`.
- **Goblins-grade:** focused-app header (28px icon + name in the 28px/700 ramp + muted "N windows") over a centered grid of live clones on a dimmed backdrop; reuse `.goblins-wm-window-card` + the three-state selection; near-square grid (`ceil(sqrt(n))` cols) scaled to fit so windows never overlap; 180ms fade + subtle per-card scale-from-0.96 stagger; light/dark via `_schemeClass()`.
- **Honest gating:** no focused app → return (no empty overlay); exactly one window → just activate it (macOS); zero-size actor → titled placeholder; SwipeAction unavailable → keyboard/dock/HUD paths still work; all enumeration/activation in try/catch + `logError`.
- **Verifiable:** host — limited (gschema `xmllint`/`--dry-run`, `node --check`, CSS self-consistency). CI/qemu — the render proof (`showAppExposeDemo()` → light+dark screenshots).
- **Effort:** M · **Risk:** LOW (boot none; only one `addKeybinding`). Verify `F10` isn't grabbed by a focused app; gate the dock-click change strictly.

### `in-progress` Multi-display arrangement / resolution / scale / refresh / mirror
- [x] **Apply substrate source-gated (CI/qemu-pending):** `/v1/displays/apply` exposes a serial-gated Mutter `ApplyMonitorsConfig` bridge. It checks `ApplyMonitorsConfigAllowed`, re-reads `GetCurrentState` before apply, rejects stale serials, validates connector/mode IDs and logical-monitor payloads, requires explicit confirmation for persistent `method=2`, and encodes the `a(iiduba(ssa{sv}))` request tuple. Settings reports the protected apply gate but keeps the editor disabled until live proof exists.
- [x] **Apply hardware proof hook source-gated (CI/qemu-pending):** the display-backed capture harness now requires `multi-display-apply-proof.json`, generated from the installed session's live Mutter DisplayConfig state, and rejects signoff unless `/v1/displays/apply` verifies the current same-layout payload, temporarily applies it, refuses persistent apply without explicit confirmation, and rejects a stale serial. This still does not ship the writable Displays panel, drag canvas, persistent Keep/Revert UI, or multi-output layout editing.
- [ ] A **writable** Goblins Displays panel driving `org.gnome.Mutter.DisplayConfig` through the allowlisted bridge, replacing today's read-only placeholders. Drag-to-arrange canvas, named scaled modes, scale, refresh, rotation, mirror — with a live-preview + Keep/Revert timer so a bad mode can't lock the user out.
- **Packages:** `mutter` (already present via gnome-shell — only confirm via `rpm -q`).
- **gsettings/dconf:** seed `org.gnome.mutter experimental-features = ['scale-monitor-framebuffer']` (additive) so fractional 125/150/175% steps exist at first boot. Mode/scale/rotation/position/primary/mirror are **not** gsettings — applied via `ApplyMonitorsConfig`; Mutter persists `method=2` to `~/.config/monitors.xml`.
- **Files:** `crates/goblins-os-core/src/displays.rs` (extend the existing reachability probe to a full state parse + apply), `crates/goblins-os-core/src/main.rs`, `crates/goblins-os-settings/src/main.rs` (replace the two read-only `system_row` placeholders in `build_displays`), `crates/goblins-os-design/src/lib.rs`, `os/dconf`, `os/bootc/Containerfile`, `crates/goblins-os-verify`.
- **APIs:** `GetCurrentState()` → serial + monitors (connector, modes incl. supported-scales) + logical layout + props (`layout-mode`/`supports-mirroring`); `ApplyMonitorsConfig(serial, method, logical_monitors, props)` with **method 0=verify, 1=temporary, 2=persistent**; `MonitorsChanged` for live refresh; `gdctl` as a debug-only CLI mirror. GTK4 `DrawingArea`/`Fixed` + `GestureDrag` canvas, `DropDown`s, `glib::spawn_future_local`.
- **Goblins-grade:** arrangement canvas (radius 12) of proportional tiles (radius 8) from logical geometry, primary tile in the flat desaturated accent; Resolution/Refresh/Scale as right-aligned DropDowns at height 30; plain-text neutral status; apply via live-preview + Keep/Revert modal (overlay radius 22) with a countdown, honoring reduce-motion.
- **Honest gating:** `GetCurrentState` unreachable → keep read-only copy, disable writes; mirror disabled unless `supports-mirroring`; fractional scales only when `supported-scales` contains them **and** the experimental-features key is set; canvas only with ≥2 outputs; always send `method=1` first, re-send `method=2` only on explicit Keep, auto-revert to the saved serial on timeout; stale serial → "display layout changed, reloading"; X11 → writes disabled.
- **Verifiable:** host — GVariant/JSON parse, mirror-common-mode intersection, named-scaled-mode labeling, serial-staleness, connector/mode allowlist, request-builder tuple. CI/qemu — canvas/drag/DropDowns/modal render + a scripted gdctl/D-Bus apply smoke test.
- **Effort:** XL · **Risk:** MED (a bad mode can black out a display — fully mitigated by verify→temporary→confirm→persistent + auto-revert; always `GetCurrentState` immediately before building the request and validate against the live snapshot). Boot/login risk LOW. **Land the read-side parse first (host-testable), the write path behind the capability gate second.**

### `in-progress` Migration Assistant (import a previous home / desktop settings)
- [x] **Capability substrate shipped** (`crates/goblins-os-core/src/migration.rs` + `/v1/migration/capabilities`): the filesystem-reader capability table (ext4/btrfs/xfs/FAT32 = kernel; NTFS/exFAT gated on `ntfs-3g`/`exfatprogs` being present; APFS/HFS+ never readable — so an unreadable drive is shown disabled, never silently skipped), the migration category model, and the allowlisted preference keys the import may write. Pure `filesystem_table` unit-tested (177 core tests); clippy/fmt clean; verify gate added.
- [x] **Source listing substrate source-gated (CI/qemu-pending):** core exposes `/v1/migration/sources`, reuses the install-target sysfs partition scan for read-only source discovery, reads only Linux block-device metadata plus the current `/proc/self/mountinfo` view, reports candidate source devices/mountpoints with filesystem readability from the existing migration capability table, and returns `partial`/`scan_errors`, `executes_live_mount=false`, and `executes_live_copy=false`. It performs no `udisksctl`, mount, `rsync`, preference import, installer UI work, or file copy; read-only udisks mounting and live copy remain deferred.
- [x] **Package + copy-plan substrate source-gated (CI/qemu-pending):** Fedora 44 package metadata and a clean Fedora 44 install probe confirm `ntfs-3g`, `exfatprogs`, `udisks2`, and `rsync`; the bootc image installs and `rpm -q`/`command -v` asserts all four plus the `udisks2.service` unit. Core exposes `/v1/migration/copy-plan`, validates absolute source/destination/category inputs, rejects duplicate/unknown categories and destination-inside-source plans, and returns the exact additive `rsync` argv (`--info=progress2`, `--ignore-existing`, `--safe-links`), copied/skipped ledger paths, and allowlisted preference keys with `executes_live_copy=false`. No mount/copy/import is performed by this source-gated route.
- [x] **Category sizing substrate source-gated (CI/qemu-pending):** core exposes `/v1/migration/estimate`, walks only the already-mounted selected category directories, reports byte/file counts plus missing/skipped paths, skips symlinks and unreadable entries, caps large scans, and returns `executes_live_copy=false`. It performs no `udisksctl`, mount, `rsync`, preference import, or installer UI work; live copy remains deferred.
- [x] **Copy-job/progress substrate source-gated (CI/qemu-pending):** core exposes `/v1/migration/start` and `/v1/migration/progress`; start reuses the validated source root, destination home, and category contract, stores planned/blocked progress with progress-log and copied/skipped ledger paths, and keeps execution fail-closed with `PRECONDITION_REQUIRED` plus `Live migration copy execution is CI/qemu-gated; this source substrate did not run rsync.` Progress parsing is host-testable for rsync progress lines, `progress.log` exit markers, and Copied/Skipped ledger counts. It performs no `udisksctl`, mount, `rsync`, preference import, installer UI work, or file copy; live progress streaming and real ledger persistence remain deferred.
- [x] **Preference import planner substrate source-gated (CI/qemu-pending):** core exposes `/v1/migration/preference-plan`, parses a read-only dconf dump narrowly, maps only `org.gnome.desktop.interface` color scheme/text scale/animations plus `org.gnome.desktop.background` wallpaper URI keys, reuses the existing GNOME normalization contracts, plans `gsettings` argv, skips unknown/malformed/unavailable-schema preferences, and only plans wallpaper when copied-path evidence proves the file landed in the destination home. It returns `executes_preference_import=false` and writes no preferences; live dconf reading and allowlisted gsettings writes remain deferred.
- [ ] **First-boot page + live copy job (deferred, CI/qemu):** the installer "Bring your stuff over" branch, source selection UI over `/v1/migration/sources`, read-only `udisksctl` mount for unmounted chosen sources, running the planned `rsync` copy with a Copied/Skipped ledger, live progress streaming, and executing the allowlisted dconf→gsettings preference import.
- **Packages:** `ntfs-3g` (`2026.2.25-1.fc44`), `exfatprogs` (`1.4.2-2.fc44`), `udisks2` (`2.11.1-2.fc44`), `rsync` (`3.4.1-7.fc44`) — verified.
- **gsettings/dconf:** write only an **allowlisted** key set through the existing appearance/accessibility bridges (`color-scheme`/`text-scaling-factor`/`enable-animations`; `background picture-uri*` only if a wallpaper file actually copied; optional pointer-feel). Read source prefs read-only via `dconf dump /` against the mounted profile — **never** blind-load a foreign dconf binary into the live profile.
- **Files:** `crates/goblins-os-installer/src/main.rs` (`build_migrate_page` + `populate_migrate_progress`; reuse `setup_choice`/`select_one`, the install-progress poll loop, `http_request`), `crates/goblins-os-core/src/migration.rs` (NEW — source scan, category sizing, rsync copy job with progress, allowlisted preference mapping), `crates/goblins-os-core/src/main.rs` (`/v1/migration/{sources,plan,start,progress}`), `crates/goblins-os-core/src/install_targets.rs` (reuse the sysfs block-device scan in reverse for source detection), `os/bootc/Containerfile`.
- **APIs:** sysfs `/sys/block/*/removable` + `/proc/self/mountinfo` (already implemented); `udisksctl` read-only mount (fallback `mount -o ro`); `ntfs-3g`/`exfatprogs` for Windows/cross-platform drives (ext4/btrfs/xfs by the kernel); `rsync --archive --info=progress2` (parse % for the bar); `dconf` read + the gsettings bridge for the write side.
- **Goblins-grade:** "Bring your stuff over" secondary on Welcome; Step-card layout shared with Appearance/Accessibility; source = `setup_choice` cards (model + size + filesystem badge); category checklist with right-aligned byte estimates; primary "Bring it over" disabled until a source + ≥1 category chosen; copy step reuses install-progress grammar with the honest status-tone ledger (neutral copied, muted skip — **never** red for an expected skip); hand into the existing `complete_and_unlock_first_boot`.
- **Honest gating:** no eligible source → calm empty-state + quiet Skip; unreadable filesystem (e.g. APFS — no driver shipped) → drive listed but disabled "Goblins can't read this disk's format (APFS)"; preference import only offers keys whose schema resolves here (`schema_snapshot` guard); wallpaper set only if the image copied; additive + read-only source so a partial failure still leaves a bootable clean session.
- **Verifiable:** host — sysfs/mountinfo parse (fixture trees like `install_targets.rs`), category sizing, filesystem-reader capability table, allowlisted dconf→gsettings mapping. CI/qemu — migrate page render, real udisks mount, rsync copy, end-to-end first-boot.
- **Effort:** L · **Risk:** MED (new packages — add to install **and** `rpm -q`; map only allowlisted keys; mount read-only). Not boot/login-critical.

### `in-progress` Named Focus modes + Do-Not-Disturb scheduling
- [x] **Substrate + storage + status route shipped**: NEW system gschema `org.goblins.os.focus` (active-mode + modes/schedules JSON), installed via `os/glib-schemas/` + a Containerfile `glib-compile-schemas /usr/share/glib-2.0/schemas` step (the repo's first *system* schema; host-validated with `glib-compile-schemas`, manifest-classified). `crates/goblins-os-core/src/focus.rs` + `/v1/focus/status` read it and evaluate the active/scheduled mode — pure `schedule_active` (incl. overnight midnight-wrap + weekday match), `parse_local_now` (timezone-aware via `date`, no new crate), and `unquote_gsettings_string`, all unit-tested (181 core tests). Honest-gated when the schema is absent. clippy/fmt clean; 3 verify gates.
- [x] **Arm/disarm/tick substrate source-gated (CI/qemu-pending):** `/v1/focus/activate`, `/v1/focus/deactivate`, and `/v1/focus/tick` write only the Goblins Focus schema plus global `org.gnome.desktop.notifications show-banners` through the shared `notifications.rs` bridge. Activating Focus snapshots `show-banners`, silences banners, records manual vs scheduled ownership, and deactivation restores the saved snapshot; the tick decision arms matching schedules, disarms schedule-owned modes when no schedule matches, and leaves manual Focus modes alone. Host tests cover mode/schedule JSON validation, scalar gsettings encoding, and tick decisions; gschema dry-run and verify gates pass. **Not shipped** until the UI/timer/live write proof lands.
- [x] **Schedule timer substrate source-gated (CI/qemu-pending):** `os/systemd-user/org.goblins.OS.FocusTick.{service,timer}` runs a user-session oneshot every minute; the helper posts to `/v1/focus/tick` only through a local HTTP core URL, exits quietly when core is unavailable, and never claims schedule success itself. The Goblins session drop-in wants the timer, the image installs/asserts the helper and units, the source manifest includes `os/focus/`, and verifier/release gates check the helper, timer, local-core guard, and Containerfile install. **Not shipped** until CI/qemu proves the user timer starts in session and the live tick writes/restores notification state.
- [x] **Settings Focus controls source-gated (CI/qemu-pending):** Settings ▸ Notifications fetches `/v1/focus/status`, shows an honest Focus section, and uses `/v1/focus/activate` plus `/v1/focus/deactivate` for the active-mode chooser. It does not create sample/default modes; empty or unavailable mode state is read-only. **Not shipped** until GTK render and live Focus write proof land.
- [x] **Menu-bar active Focus indicator source-gated (CI/qemu-pending):** `goblins-menubar` reads the system `org.goblins.os.focus` schema, hides when Focus is off or the active id is not in configured modes, shows only the configured active mode name, and opens Settings ▸ Notifications on click. The desktop render harness now captures `59b-menubar-focus-$suffix.png` after seeding a deterministic `work` mode and active state, then restores Focus to off. It performs no writes and makes no live timer/write claim. **Not shipped** until GNOME Shell render and live Focus state proof land.
- [x] **Control Center Focus tile source-gated (CI/qemu-pending):** Control Center fetches `/v1/focus/status`, renders a read-only Focus tile from core-reported configured modes, opens Settings ▸ Notifications for changes, and does not call Focus write routes. The render harness now seeds a deterministic `work` / `Deep Work` mode, captures `37b-control-center-focus.png` and `39b-control-center-focus-dark.png`, then restores Focus to off. It creates no sample/default modes in the product path and makes no schedule/timer/live-write claim. **Not shipped** until reviewed Control Center GTK pixels and live Focus proof land.
- [x] **Mode/schedule CRUD substrate source-gated (CI/qemu-pending):** core exposes `/v1/focus/mode` and `/v1/focus/schedule`, validates Focus ids/names/weekdays/time windows, upserts/deletes only through the Goblins Focus schema, refuses deletion of the active mode, and refuses deletion of a mode that schedules still reference. No default/sample modes are created, no per-app breakthrough policy is applied, and no Settings editor/render proof is claimed.
- [x] **Live arm/disarm hardware proof hook source-gated (CI/qemu-pending):** the display-backed capture harness now requires `focus-arm-roundtrip-proof.json`, posts `/v1/focus/activate` and `/v1/focus/deactivate` against a deterministic `gate-work` mode, verifies `active-mode` read-back, notification banner snapshot/silence/restore behavior, original state restoration, and explicit `mode_crud_claim=false`, `schedule_claim=false`, and `per_app_breakthroughs_claim=false`. The proof is linked from `proof-manifest.json`, and `close-signoff.sh`, `verify-shipping-status.sh`, and `goblins-os-verify` reject missing/failing proof. No live qemu run has produced the proof yet.
- [ ] **Surfaces + per-app breakthroughs (deferred):** per-app breakthrough via the `notifications.rs` helper and the `SettingsPanel::Focus` editor for mode/schedule CRUD. (Drops iCloud/location/Smart Activation — absent, never stubbed.)
- **Packages:** none.
- **gsettings/dconf:** DRIVES `org.gnome.desktop.notifications show-banners` (already allowlisted as `ShowBanners`) + per-app `…notifications.application` enable/show-banners. OWN a new `org.goblins.os.focus` schema (active-mode, modes JSON, schedules JSON, armed-by-schedule, restore-banners, restore-apps), compiled like the wm schema; dconf-seed default modes so first boot is non-empty (active-mode='', schedules='[]').
- **Files:** `crates/goblins-os-core/src/focus.rs` (NEW — mode CRUD, arm/disarm writing show-banners + per-app enable via the **same** `notifications.rs` helper, schedule CRUD + evaluation, snapshot/restore), `crates/goblins-os-core/src/main.rs` (`/v1/focus/{status,activate,mode,schedule,tick}`), `crates/goblins-os-settings/src/main.rs` (`SettingsPanel::Focus` + mode list / allowed-apps / schedule editor; Notifications cross-link), `crates/goblins-os-control-center/src/main.rs` (Focus quick-pick tile + "on until <time>"), `…/goblins-menubar@goblins.os/extension.js` (Focus entry + armed-only indicator glyph), `os/systemd-user/goblins-os-focus.{service,timer}` (NEW `OnCalendar=minutely` → `POST /v1/focus/tick`), `…/schemas/org.goblins.os.focus.gschema.xml` + `os/bootc/Containerfile` (glib-compile-schemas line), `os/dconf/db/local.d/10-goblins-os-desktop` (seed).
- **APIs:** gsettings CLI bridge; `org.goblins.os.focus`; axum routes; systemd **user** timer (no shell DBus dependency, survives UI close); GTK4 + GJS St/PopupMenu; glib-compile-schemas at build.
- **Goblins-grade:** inset cards (radius 12); mode rows = colored `gos-tint-*` icon-tile + name + quiet "Scheduled 9-5 Mon-Fri" subtitle; active mode carries the calm accent ring `alpha(@gos_primary_border,0.42)`; schedule editor with 30px controls + weekday pill toggles; allowed-apps reframed from the per-app notification registry as "breakthrough" chips; Control Center moon/mode tile; armed-only menu-bar glyph; arm/disarm `MOTION_FAST_MS`. Copy: "Work is on until 5:00 PM. Only allowed apps can interrupt."
- **Honest gating:** gsettings unavailable → read-only "…Focus is read-only in this session."; `show-banners` absent → engine reports unavailable; per-app schema absent → allowlist editor hides, mode still silences globally; no Smart Activation/location/cross-device (absent, not stubbed); tick is a no-op without schedules, and the panel says "Schedules need the Focus timer, which is not running" when the unit is inactive.
- **Verifiable:** host — schedule evaluator (arm/disarm due, next transition, midnight-wrap, end<start), JSON + gschema-string round-trips, snapshot/restore, per-app path/allowlist validation. CI/qemu — gsettings writes taking effect, Settings panel, Control Center tile, menu-bar indicator, the timer firing.
- **Effort:** L · **Risk:** LOW (no packages; a new gschema + a user timer). **Snapshot + faithfully restore** show-banners and per-app enable so leaving Focus never permanently mutes the user's own config; serialize writes through the single core service.

### `in-progress` Keyboard shortcut editor + modifier remap (Caps Lock → Control)
- [x] **Shortcuts reference shipped** (`crates/goblins-os-core/src/shortcuts.rs` + `/v1/shortcuts/status`, Settings ▸ Keyboard "Shortcuts" list): reads the 14 Goblins window-management bindings from `org.goblins.shell.extensions.wm` and shows each action with its humanized accelerator (`<Super><Shift>Left` → "Super + Shift + Left"; pure `humanize_accelerator`/`parse_gsettings_strv` unit-tested, 176 core tests), honest-gated to "unavailable" when the wm schema isn't installed. Container-verified (clippy `-D warnings`), 2 verify gates.
- [x] **Rebinding + Caps Lock remap substrate source-gated (CI/qemu-pending):** `/v1/keyboard/shortcuts/binding` writes only the allowlisted Goblins WM schema keys, supports reset, validates accelerator grammar, and refuses conflicts with other Goblins bindings. `/v1/keyboard/modifier-remap` edits only the `ctrl:*`/`caps:*` token in `xkb-options` so Caps Lock can become Control or return to default while preserving unrelated layout/compose options. Settings reports the source-gated bridge but keeps record/dropdown controls disabled until qemu proof is green.
- [x] **Live roundtrip hardware proof hook source-gated (CI/qemu-pending):** the display-backed capture harness now requires `keyboard-shortcuts-roundtrip-proof.json`, posts the live shortcut and modifier routes, verifies gsettings read-back for `<Super><Shift>H` and `ctrl:nocaps`, resets the shortcut to `<Super>w`, restores Caps Lock to default, links the proof in `proof-manifest.json`, and makes `close-signoff.sh`, `verify-shipping-status.sh`, and `goblins-os-verify` reject missing/failing proof. No live qemu run has produced the proof yet.
- [ ] **Recordable UI + reviewed live round trip (deferred):** make rows recordable, add a Caps Lock dropdown, inline conflict notice, per-row/global reset, and review the qemu gsettings round-trip proof before marking the UI shipped.
- **Packages:** none (all three schemas ship in gsettings-desktop-schemas).
- **gsettings:** `org.gnome.desktop.input-sources xkb-options` (Caps→Ctrl via `ctrl:nocaps`, editing **only** the `ctrl:*`/`caps:*` token, preserving `grp:`/`compose:`/`lv3:`); `org.gnome.desktop.wm.keybindings` (close/toggle-maximized/minimize/switch-applications(+backward)/switch-windows/show-desktop/toggle-fullscreen/begin-move/begin-resize); `org.gnome.settings-daemon.plugins.media-keys` (screenshot/screenshot-clip/area-screenshot/www/terminal/home/search). Reset = `gsettings reset SCHEMA KEY`. Custom-command keybindings → **read-only** v1 (handoff).
- **Files:** `crates/goblins-os-core/src/keyboard_shortcuts.rs` (NEW — allowlisted bridge mirroring `input.rs`: status + set/reset, action allowlist + spec table, conflict detection, separate modifier-remap target), `crates/goblins-os-core/src/main.rs` (`/v1/keyboard/shortcuts/status`, `/v1/keyboard/shortcuts/binding`, `/v1/keyboard/modifier-remap`), `crates/goblins-os-settings/src/main.rs` (replace the stub at 5622-5625 with the Shortcuts subsection + Modifier Keys row), `crates/goblins-os-verify/src/main.rs` (pin the new copy + no-stub assertion), `os/dconf/db/local.d/10-goblins-os-desktop` (OPTIONAL branded baseline so reset has a Goblins default).
- **APIs:** gsettings CLI (get/set/reset/list-keys, schema-snapshot existence check before any write); xkeyboard-config `ctrl:nocaps`/`caps:ctrl_modifier` (applied live by mutter on Wayland, no logout); GTK4 `EventControllerKey` for live chord recording.
- **Goblins-grade:** "Shortcuts" `gos-subsection-title` + a `gos-preference-group` of accelerator rows (title + right-aligned key-cap chip + record button at height 30 + subtle reset); "Modifier Keys" row with a DropDown; one accent for the recording ring, `gos_system_green` applied, `gos_system_orange` conflict, `gos_system_red` only for hard failure; honest detail strings in the house voice.
- **Honest gating:** schema/key not reported → rows read-only "…read-only because the required preference is not reported by this session."; recorded chord collides with another allowlisted binding → refuse + orange notice (never silently steal); Caps→Ctrl is the one safe reversible op (Control always reachable); custom commands surfaced read-only.
- **Verifiable:** host — xkb-options token parse/merge/remove, `<Mod>key` grammar validation, action allowlist + spec, conflict detection, unavailable/type-check paths. CI/qemu — rows, live recording, gsettings round-trip.
- **Effort:** L · **Risk:** LOW (user-session gsettings, no image/privileged change). Edit only the `ctrl:*`/`caps:*` token; validate chords before set + read-back; allowlist + conflict-refusal + always-available reset prevent stranding.

### `in-progress` Keychain / Passwords UI
- [x] **Status + manager handoff shipped** (`crates/goblins-os-core/src/keychain.rs` + `/v1/keychain/status`, Settings ▸ Security "Passwords & Keys" row): reports whether the Secret Service (gnome-keyring) and the Passwords & Keys manager are present, honest-gated, with **`seahorse` web-verified for Fedora 44** ([Fedora Packages](https://packages.fedoraproject.org/pkgs/libsecret/libsecret/)) and added to the Containerfile install + `rpm -q`. Pure `keychain_detail` unit-tested (182 core tests); container clippy `-D warnings` clean; route + package verify gates.
- [x] **Collection metadata substrate source-gated (CI/qemu-pending):** core exposes `/v1/keychain/collections`, reads only Secret Service collection metadata through `gdbus` (`org.freedesktop.Secret.Service` + `org.freedesktop.Secret.Collection`), and returns collection labels, lock state, and item counts without item labels or secret values. Settings ▸ Security renders a read-only Passwords & Keys metadata row and repeats that secret values are never displayed in Settings. No `GetSecrets`, reveal, edit, delete, secure note, or live unlocked-keyring proof is claimed. Local gates: `cargo test -p goblins-os-core keychain`, `cargo test -p goblins-os-settings keychain`, `cargo fmt -p goblins-os-core -p goblins-os-settings -p goblins-os-verify --check`, `cargo clippy --workspace -- -D warnings`, `cargo test --workspace`, `goblins-os-verify --source-root .` → **blocked=0 (2042)**, `git diff --check`, `bash -n os/hardware-gate/verify-shipping-status.sh`, and the Rust 1.88 GTK container `cargo clippy -p goblins-os-settings --features goblins-os-settings/native-desktop -- -D warnings`.
- [x] **Interim Passwords & Keys launch source-gated (CI/qemu-pending):** Settings ▸ Security now offers an **Open Passwords & Keys** handoff that launches the already-packaged `seahorse` manager through the existing native-app launcher. This reads no secrets, calls no Secret Service item APIs, and does not claim the future Goblins browse/edit surface.
- [ ] **Full browse/edit surface (deferred):** a Goblins-branded passwords panel on the `org.freedesktop.Secret` D-Bus (browse/search/view/edit/delete + secure notes), with seahorse as the interim manager launch.
- **Approach:** custom_surface (a Goblins-branded passwords panel on the `org.freedesktop.Secret` D-Bus / libsecret) preferred; `seahorse` packaged as the interim fallback (verify fc44 name).
- **Packages:** `gnome-keyring` (already shipped) + optionally `seahorse` (interim).
- **Files:** `crates/goblins-os-settings/src/main.rs` or a small new crate (a Goblins Passwords surface on the Secret Service D-Bus), `os/bootc/Containerfile` (only if `seahorse` interim), `crates/goblins-os-verify/src/main.rs` (gate).
- **APIs:** `org.freedesktop.Secret.Service` / libsecret; the login keyring is already unlocked at session start (PAM).
- **Goblins-grade:** `gos-row` per item (label + service + reveal-on-demand), search field, calm honest empty-state; never display secrets unprompted. Honest gating: keyring locked → "Unlock your login keyring to view saved passwords."
- **Verifiable:** host — Secret Service query/model logic. CI/qemu — render + live keyring.
- **Effort:** M · **Risk:** LOW-MED (read/edit a live credential store — never log or expose secrets; server-side/keyring boundary).
- _Note: spec agent connection-failed; libsecret API + `seahorse` fc44 name to web-verify before building._

### `in-progress` Per-app privacy permissions UI (camera / mic / location / files)
- [x] **Read substrate + surface shipped** (`crates/goblins-os-core/src/app_permissions.rs` + `/v1/app-privacy/status`, Settings ▸ Privacy "App permissions" group): reads the xdg `PermissionStore` over `gdbus` (`List(in s table, out as ids)`, **web-verified** against the spec — no new package, the portal already ships) for the `location`/`background`/`notifications`/`devices` tables and lists the entries per category, honest-gated when the store isn't running. Pure `parse_list_reply` unit-tested (183 core tests); container clippy `-D warnings` clean; route + surface verify gates.
- [x] **Per-app revoke substrate source-gated (CI/qemu-pending):** `/v1/app-privacy/revoke` validates the known PermissionStore tables and safe desktop IDs, then calls `DeletePermission(table, id, app)` only for app-keyed grants. Settings ▸ Privacy now renders per-app revoke rows with exact core feedback. Resource-keyed device grants and live portal reload proof remain deferred.
- [x] **Live app-keyed revoke hardware proof hook source-gated (CI/qemu-pending):** the display-backed capture harness now requires `app-privacy-revoke-proof.json`, seeds a deterministic app-keyed location grant through `PermissionStore.SetPermission`, posts `/v1/app-privacy/revoke`, verifies `PermissionStore.GetPermission` read-back is empty, restores the prior grant state, links the JSON in `proof-manifest.json`, and makes `close-signoff.sh`, `verify-shipping-status.sh`, and `goblins-os-verify` reject missing/failing proof. No live qemu run has produced the proof yet.
- [ ] **Resource mappings (deferred):** `Lookup`/metadata mapping for camera/microphone resource-keyed grants before any device revoke UI.
- **Approach:** custom_surface (own Goblins panel reading/writing the xdg-desktop-portal permission store).
- **Packages:** none (xdg-desktop-portal already shipped).
- **APIs:** `org.freedesktop.impl.portal.PermissionStore` D-Bus (Lookup/Set/Delete per table: `devices` for camera/mic, `location`, `screenshot`, `background`); flatpak app metadata for friendly names.
- **Files:** `crates/goblins-os-core/src/*` (a permission-store read/write bridge, allowlisted like accessibility.rs), `crates/goblins-os-settings/src/main.rs` (per-resource group: a row per app with a revoke toggle), `crates/goblins-os-verify/src/main.rs` (gate).
- **Goblins-grade:** group by resource (Camera, Microphone, Location, …), each a `gos-row` (app name + granted/denied switch); honest gating: no portal / empty store → "No apps have requested this yet."
- **Verifiable:** host — PermissionStore payload encode/decode + grant model. CI/qemu — render + live portal.
- **Effort:** M · **Risk:** LOW-MED (revoking is reversible; never broaden a grant silently).
- _Note: spec agent connection-failed; PermissionStore table/key names to web-verify before building._

---

## Bucket C — Net-new engines (real projects)

Genuinely new capability. Each carries an engine; weights are **never** bundled — the OS detects runtime + model and greys the feature with truthful copy until present (the `voice.rs`/`model_manager.rs` thesis).

### `in-progress` Voice Control (spoken command → action)
- [x] **Command-vocabulary substrate shipped** (`crates/goblins-os-core/src/voice_control.rs` + `/v1/voice/control/vocabulary` + `/v1/voice/control/resolve`): the curated phrase→action vocabulary, with pure `normalize_phrase` (lowercase/punctuation/whitespace) and deterministic `match_command` (exact-only — **never guesses**; no match → `fall_through_to_dictation`), echoing "Heard: X → Action Y". Resolve-only (never executes). `engine_available` honest-gated on whisper presence (`GOBLINS_OS_WHISPER_BIN` override). 188 core tests (incl. a test forbidding the Apple-assistant name); clippy/fmt clean; route gate.
- [x] **Push-to-talk dispatch route source-gated (CI/qemu-pending):** `/v1/voice/control` captures through the existing dictation path or accepts a transcript, resolves exact curated phrases, falls through to dictation when nothing matches, and dispatches matched commands only through the existing gated Settings/safe-setting helpers. The shared registry now has `voice-control` + `AiEntrypoint::Voice`; Settings shows a source-gated Voice Control row; `os/voice/goblins-os-voice-control` launches returned Settings routes or types no-match dictation text. It does **not** claim live capture proof, a HUD, or a shortcut yet.
- [ ] **Live capture/keybinding/HUD proof (deferred, L):** prove microphone capture and transcription in CI/qemu, add the non-conflicting keybinding, and build the push-to-talk HUD + confirmation surface. The helper exists, but the feature remains `in-progress`.
- **Packages:** Fedora 44 package probing found `whisper-cpp`/`whisper-cpp-devel` (`1.8.1-2.fc44`) but repoquery listed only libraries/headers and no provider for `*/whisper-cli`; do **not** add an RPM until the actual CLI provider is proven. `voice.rs` still defaults to `whisper-cli` with a `GOBLINS_OS_WHISPER_BIN` override, so a missing runtime degrades honestly.
- **dconf:** no new binding in the source-gated pass. The old `<Super><Alt>c` proposal collides with the shipped Color Picker binding (and Live Captions also proposed it), so pick/prove a non-conflicting shortcut before enabling Voice Control by default. **No new schema** — reuses the core bridge + the **existing per-action policy controls**; push-to-talk, so no always-listening key.
- **Files:** `crates/goblins-os-core/src/voice.rs` (`voice_control()`: capture → transcribe → resolve intent → dispatch to an `AiAction`; `VoiceControlOutcome{ok,transcript,matched_action_id,action_title,executed,needs_confirmation,text}`), `crates/goblins-os-ai/src/lib.rs` (one `AiAction` id `voice-control` + `AiEntrypoint::Voice` + a phrase→action table; bump `REGISTRY_VERSION`), `crates/goblins-os-core/src/main.rs` (`/v1/voice/control`), `os/voice/goblins-os-voice-control` (NEW helper mirroring `goblins-os-dictate`), `os/bootc/Containerfile`, `os/dconf/db/local.d/10-goblins-os-desktop`, `crates/goblins-os-core/src/ai.rs` (readiness + action-history audit), `crates/goblins-os-settings/src/main.rs` (Accessibility Voice Control card), `crates/goblins-os-verify/src/main.rs`.
- **APIs:** axum; whisper.cpp CLI; `arecord`/`aplay` over PipeWire (already packaged); the action registry as the command surface; `resident_generate()` for LLM-assisted intent fallback (**proposes only**, never auto-executes a state change); dispatch **through** the existing `change_safe_setting`/`open_settings_panel` + policy/confirmation handlers (never around them).
- **Goblins-grade:** a push-to-talk HUD (overlay radius 22, `native_css` material, `MOTION_OVERLAY_MS` fade) showing the live transcript (`GOS_TYPE_BODY`) + matched action title (`GOS_TYPE_TITLE_3`) — macOS's "show what I heard"; neutral status tone "Heard: turn on dark mode → Change a safe setting"; PermissionAndConfirmation actions still surface the explicit confirm card; "Goblin" wake word, never the Apple assistant name (a `voice.rs` test forbids it).
- **Honest gating:** no model/`whisper-cli` → `ok=false` with the existing "add a model" copy, card greys; no mic → "Microphone capture is not ready on this device."; **no command match → do NOT guess; fall through to plain dictation** (types the text); matched-but-engine-not-ready → `WaitingForEngine`; policy Denied/Gated → returned verbatim; confirmation-required → `executed=false, needs_confirmation=true`; no always-listening claim anywhere.
- **Verifiable:** host — phrase normalization, exact/fuzzy match, no-match→dictation branch, readiness/policy mapping, outcome serde; registry tests. CI/qemu — `arecord` capture, transcription, keybinding, Settings card, the HUD.
- **Effort:** L · **Risk:** MED. Executing actions by voice is a privilege surface — dispatch only through the gated handlers; deterministic match first, LLM proposes only, every match echoes "Heard: X → Action Y." Not boot/login-critical. v2 shell overlay deferred.

### `in-progress` Live Captions (real-time on-device caption overlay)
- [x] **Status/config substrate shipped** (`crates/goblins-os-core/src/live_captions.rs` + `/v1/live-captions/status`, NEW `org.goblins.shell.extensions.captions` gschema via `os/glib-schemas/`, dconf-seeded off): STT runtime/model/PipeWire/capture capability gates, caption config normalizers (source, text size, position, auto-hide, keep-onscreen), Whisper argv builder, and VAD/RMS segment helpers are pure + host-tested. Live capture/transcription remains CI/qemu-pending.
- [x] **Overlay + stream contract source-gated (CI/qemu-pending):** `/v1/captions/status` aliases the status substrate and `/v1/captions/stream` returns an honest SSE status event; `goblins-captions@goblins.os` is installed/enabled in the Goblins shell mode but hidden by default through the existing disabled schema. If explicitly enabled before the live engine exists, it shows "Live Captions are waiting for the local caption stream" rather than fake captions. Node syntax, gschema dry-run, host tests, and verifier gates are green; qemu render and live stream remain pending.
- [x] **Quick Settings toggle source-gated (CI/qemu-pending):** the shell extension registers a GNOME Quick Settings `SystemIndicator`/`QuickToggle` bound to the existing `enabled` key. The toggle exposes only the already-honest waiting overlay; it does not start capture, add an RPM, add a shortcut, or claim live captions.
- [x] **Settings Live Captions row source-gated (CI/qemu-pending):** Settings ▸ Accessibility fetches `/v1/live-captions/status` and mirrors readiness/configuration in a read-only row. It states captioning stays local, keeps the toggle boundary in Quick Settings, and does not claim live capture/transcription or add a Settings write.
- [x] **PipeWire capture-plan contract source-gated (CI/qemu-pending):** core now owns a pure `pw-dump` monitor-source parser and stable `pw-record --target <monitor> --rate 16000 --channels 1 --format s16 <chunk.wav>` argv builder for the future engine. The status JSON exposes the deterministic capture plan while keeping `runtime_ready_claim=false`, `capture_runtime_ready=false`, and `transcription_ready_claim=false`; command presence alone cannot make Live Captions active. Settings displays that boundary. The image asserts `pw-record` and `pw-dump` from the already-installed `pipewire-utils`; no Whisper package/model, daemon, capture stream, or caption text is claimed.
- [x] **Waiting-overlay desktop render hook source-gated (CI/qemu-pending):** `goblins-captions@goblins.os` exposes `showWaitingRenderProof()` for the existing desktop render harness, and `render-desktop.sh` captures `58-live-captions-waiting-$suffix.png` in light/dark. The hook renders only the waiting capsule and keeps capture/transcription/live-caption claims false. Pixel proof remains CI/qemu-pending.
- [ ] **Live capture/transcribe/menu proof (deferred, L):** implement/prove the privileged capture loop, real transcription stream, rendered Quick Settings control/overlay behavior, and a non-conflicting shortcut if one is added. The feature remains `in-progress`.
- **Packages:** existing `pipewire-utils` is now asserted for `pw-record` and `pw-dump`. `whisper-cpp`/`whisper-cpp-devel` exist in Fedora 44 as `1.8.1-2.fc44`, but the current repoquery proof did **not** find a `whisper-cli` binary provider; do not add an RPM or `command -v whisper-cli` gate until the CLI provider is proven. **Do NOT** depend on `whisper-stream` (SDL2, often unpackaged, mic-via-SDL — wrong tool).
- **gsettings/dconf:** NEW `org.goblins.shell.extensions.captions` (enabled, toggle-captions `['<Super><Alt>c']`, source system|microphone|both, auto-hide, keep-onscreen, text-size, position) + a `30-captions` seed shipping installed-but-off (`enabled=false`).
- **Files:** `os/gnome-shell-extensions/goblins-captions@goblins.os/{metadata.json,extension.js,stylesheet.css,schemas/…captions.gschema.xml}` (overlay St actor + menu-bar QuickToggle; waiting-only until qemu proof), `os/dconf/db/local.d/30-captions`, `os/bootc/Containerfile` (assert existing PipeWire capture tools only), `crates/goblins-os-core/src/live_captions.rs` (status/config, pure `pw-dump` monitor parser, `pw-record` capture argv, Whisper argv, VAD/RMS helpers), `crates/goblins-os-core/src/main.rs` (`/v1/captions/status` + SSE status route), `crates/goblins-os-settings/src/main.rs` (Accessibility "Live Captions" row), `os/gnome-shell-modes/goblins-os.json` (enable the uuid).
- **APIs:** `Main.layoutManager.addChrome` + St.BoxLayout/Label (the exact goblins-wm overlay idiom); QuickSettings SystemIndicator/QuickToggle; core **HTTP+SSE** stream so the privileged capture stays in core, not the shell; core: `pw-dump` monitor discovery, `pw-record` on the default-sink **monitor**, VAD/RMS segment, future `whisper-cli -m <model> -otxt`; `wpctl`/`pw-cli`/`pw-dump` to resolve the monitor id.
- **Goblins-grade:** a glass caption capsule (`@gos_material_thick` + border + shadow, pill/HUD radius); text `GOS_TYPE_BODY`/`CALLOUT`/`TITLE_3` by size, **Inter**; newest line full-ink, prior line dims one tier (macOS settle); leading status dot (`gos_system_green` transcribing / neutral idle / `gos_system_orange` warming); opacity+rise arrival with the reduced-motion clean cut; positioned via work-area insets so it never collides with the dock.
- **Honest gating:** `/v1/captions/status` reports capture/model/runtime/pipewire like `voice_status`, but `pw-record` presence alone never marks capture ready. Model absent → Settings "Add a speech model to turn on Live Captions" + toggle greyed with the reason on hover (never a dead toggle); no monitor source → "No system audio to caption" (not a blank box). Capture+STT fully local — stated in the subtitle.
- **Verifiable:** host — VAD/segment chunker, capability struct, whisper-cli argv builder, `/v1/captions/status` JSON; gschema `--dry-run`. CI/qemu — extension.js, live monitor capture, real transcription, the rendered overlay (light+dark).
- **Effort:** L · **Risk:** MED. Keep capture in the core service (runs as the service user); expose only a read-only stream to the shell. Chunk-on-silence adds 0.5-2s lag — small `base.en` default + in-progress dim line + VAD tuning; the UI says it's an accessibility aid. Ships disabled, not in the login path.

### `in-progress` Switch Control (scanning input for adaptive switches)
- [x] **Status + schema substrate shipped** (`crates/goblins-os-core/src/switch_control.rs` + `/v1/accessibility/switch-control/status`, NEW `org.goblins.os.a11y.switch-control` gschema via the existing `os/glib-schemas/` plumbing — off by default): reads enabled/mode/scanning/timings with the same normalization the engine will trust (`normalize_mode`/`normalize_scanning`/`clamp_interval` 300–5000 / `clamp_ms`), honest-gated when the schema is absent. Pure normalizers unit-tested (193 core tests); `glib-compile-schemas` clean; clippy/fmt clean; route + schema verify gates.
- [x] **Preference bridge + Settings subsection source-gated (CI/qemu-pending):** core exposes `/v1/accessibility/switch-control/preference`, writes only the allowlisted `org.goblins.os.a11y.switch-control` keys, validates mode/scanning, clamps timing values, and returns honest saved-but-not-scanning copy until the scanner engine is active. Settings ▸ Accessibility renders status, master toggle, mode/style choices, and timing sliders through that route. No Shell extension, AT-SPI walk, highlight overlay, switch input, or selection injection is claimed yet.
- [x] **Shell scanner scaffold source-gated (CI/qemu-pending):** `goblins-switch@goblins.os` is installed in the Goblins shell mode, seeded off through dconf, reads the system Switch Control schema, exposes item/point scan state, attempts AT-SPI discovery, renders the highlight ring/crosshair overlay, supports auto/step advance, and Escape disables the feature. AT-SPI runtime behavior, overlay pixels, switch input, and pointer injection remain qemu-pending; point selection explicitly stays paused until that proof exists.
- [x] **Desktop render hook source-gated (CI/qemu-pending):** `render-desktop.sh` now invokes the live shell extension hook and captures `57-switch-control-point-$suffix.png` in light/dark so CI can prove overlay pixels. The hook forces point-scan display only and leaves pointer injection/selection proof deferred.
- [ ] **Scanning engine + overlay/input proof (deferred, XL/highest-risk):** the `goblins-switch@goblins.os` extension (item/point scan state machine, AT-SPI tree walk, Clutter highlight ring/crosshair, gated input injection, hard Escape→disable, never on by default, session-only) plus qemu proof of highlighting, fallback, and selection behavior.
- **Packages:** `at-spi2-core` (already in the image at Containerfile L44 — no new RPM; gnome-shell/libei present too).
- **gsettings/dconf:** NEW `org.goblins.os.a11y.switch-control` (enabled, mode item|point, scanning auto|step, auto/interface-interval-ms, loops-before-pause, dwell-ms, switch-debounce-ms, point-precision, audio-cues, select/next/pause-key) shipped as a compiled gschema + dconf-seeded off. Reuse existing `…a11y.applications screen-keyboard-enabled` for the on-screen keyboard under scan.
- **Files:** `crates/goblins-os-core/src/switch_control.rs` (NEW — status + preference bridge mirroring `accessibility.rs`), `crates/goblins-os-core/src/main.rs` (`/v1/accessibility/switch-control/{status,preference}`), `crates/goblins-os-settings/src/main.rs` (Switch Control subsection in `build_accessibility` + summary tiles), `os/gnome-shell-extensions/goblins-switch@goblins.os/{extension.js,metadata.json,stylesheet.css,schemas/…gschema.xml}` (NEW — the scanning ENGINE + overlay), `os/gnome-shell-modes/goblins-os.json`, `os/dconf/db/local.d/10-goblins-os-desktop`, `os/bootc/Containerfile` (COPY + glib-compile-schemas; no new RPM).
- **APIs:** AT-SPI2 via the in-process `gi://Atspi` binding (walk the focused window's tree, query `AtspiComponent` extents, `AtspiAction.do_action`/`grab_focus`); Clutter/St/Meta overlay actors + virtual-input click injection (the goblins-wm idiom); `GLib.timeout_add` per tick; `Gio.Settings` in the extension, gsettings CLI in core.
- **Goblins-grade:** highlight ring (radius 8, 3px accent stroke + soft glow); crosshair = 2px accent at 40% opacity; step transitions `MOTION_FAST_MS`, the ring eases (Reduce-Motion → hard cut); Home panel = floating card (radius 22, material, `GOS_TYPE_TITLE_3`, 38px rows); soft audio tick; Settings summary-grid tiles (green ready / neutral off — **never** alarm-red for a disabled assistive feature).
- **Honest gating:** gsettings/schema absent → `gsettings_available=false`, read-only "Desktop preferences are not ready…"; AT-SPI tree unavailable for an app → auto-fall-back to point-scan with "This window has no scannable controls — using point scan"; synthetic input blocked → highlight still works, selection disabled "Selection is paused on this screen."; no switch connected → enabled-but-no-input, on-screen keys still respond to Space/Tab for self-test.
- **Verifiable:** host — value parsing, enum/range normalization (intervals 300-5000, debounce ≥0), honest-gating branch selection; gschema `--dry-run`. CI/qemu — the scanning state machine, AT-SPI walk, Clutter overlay, input injection (a qemu interaction render: highlight ring over a known app + the point-scan crosshair).
- **Effort:** XL · **Risk:** HIGHEST in bucket (net-new real-time engine that injects input + draws over everything). Bind only the configured keys (no global grab); a hard-wired, non-remappable **Escape→disable**; never enabled by default; **v1 scoped to the user session, explicitly NOT the GDM greeter**; reuse goblins-wm's proven actor/timeout patterns; fail-closed on any error.

### `in-progress` Sound Recognition (alerting for safety/attention sounds)
- [x] **Category registry + status substrate shipped** (`crates/goblins-os-core/src/sound_recognition.rs` + `/v1/sound-recognition/status`, NEW `org.goblins.SoundRecognition` gschema via `os/glib-schemas/`, dconf-seeded all-off): the fixed sound catalog, per-sound allowlist/normalizer, classifier-model/listener/capture capability gates, reliability caveat, and honest JSON status are host-testable. No listener or Settings UI is claimed yet; if the model/listener/capture/schema is absent the route reports exactly that.
- [x] **Settings controls + write bridge source-gated (CI/qemu-pending):** core exposes `/v1/sound-recognition/preference` and `/v1/sound-recognition/sound-toggle`, writes only the allowlisted `org.goblins.SoundRecognition` keys, rejects unknown sound ids, clamps confidence, and returns honest saved-but-not-listening copy until model/listener/capture/categories are ready. Settings ▸ Accessibility renders readiness, reliability caveat, master toggle, per-sound switches, sensitivity, confidence, and alert toggles through those routes. No listener, model weights, capture loop, notification firing, or live mic behavior is claimed yet.
- [x] **Session listener boundary source-gated (CI/qemu-pending):** `os/sound-recognition/goblins-os-sound-listener` is installed as `/usr/libexec/goblins-os/goblins-os-sound-listener`, exposes `--capability-check`/`--self-test`, reports `ready=false`/`runtime_ready_claim=false`, and exits without microphone capture until model provisioning, inference dependencies, capture integration, notifications, and qemu proof land together. Core consumes the listener capability report instead of treating binary presence as listener readiness; the user service is installed but not session-wanted. No model weights, listener loop, notifications, or mic capture are claimed.
- [x] **Detection decision contract source-gated (CI/qemu-pending):** core maps classifier AudioSet classes to the fixed category registry, applies sensitivity/confidence thresholds, debounces repeated per-category alerts, and builds the Goblins notification payload without delivering it. The installed listener mirrors that pure contract through `--decision-self-test` and reports `decision_contract_ready=true` while keeping `ready=false`/`runtime_ready_claim=false`; no model, capture, notification firing, sound, flash, or live daemon loop is claimed.
- [x] **Notification delivery-plan contract source-gated (CI/qemu-pending):** core and the installed listener now share the deterministic Goblins notification delivery plan (`org.goblins.OS.SoundRecognition`, Goblins icon, desktop-entry/category hints, safety-critical urgency, alert sound/flash/lock-screen intent) while keeping `delivery_ready_claim=false`, `runtime_ready_claim=false`, and `capture_runtime_ready=false`. Core treats capture readiness as a listener-owned runtime capability, so driver presence alone cannot make the feature active. No notification delivery, sound, flash, mic capture, model, or live daemon loop is claimed.
- [ ] Always-listening on-device recognition of a fixed catalog (smoke/fire alarm, siren, doorbell, knock, baby crying, dog bark, car horn, appliance beep, running water, shouting) firing a Goblins notification + optional sound/flash, for deaf/HoH users. **Reliability honesty is first-class** (not a footnote).
- **Packages:** `python3-onnxruntime` (`1.22.2`), `python3-numpy`, `libnotify` (`0.8.7-1.fc44`), `alsa-utils`, `pipewire`, `pipewire-alsa`, `wireplumber`, `sox` (audio stack already present; `sox` already used in the brand-sound layer).
- **gsettings/dconf:** NEW relocatable `org.goblins.SoundRecognition` (enabled, sounds `as`, sensitivity, alert-sound, alert-flash → drives `…a11y.keyboard visual-bell`, min-confidence, notify-in-lock-screen) seeded **all-off**. Reuse existing notifications + per-app registry so alerts respect DND/lock-screen.
- **Files:** `crates/goblins-os-core/src/sound_recognition.rs` (NEW — allowlisted bridge: status + per-sound toggle, capability gating, honest detail strings), `crates/goblins-os-core/src/main.rs` (`/v1/sound-recognition/{status,preference,sound-toggle}`), **`os/sound-recognition/goblins-os-sound-listener`** (NEW — the **in-session** python3 daemon: onnxruntime + a YAMNet-class model, reads GSettings via gio, captures 16kHz mono from PipeWire, runs the 521-class classifier on a ~1s sliding window, maps AudioSet classes → enabled ids, debounces, calls `notify-send`/the Notifications D-Bus iface with a Goblins app-id), **`os/systemd-user/org.goblins.OS.SoundRecognition.service`** (NEW — runs **in the user session** so it reaches the user PipeWire socket; the **key architectural fix** — core is `ProtectSystem=strict` with no audio, so the always-on mic loop cannot live in core), `os/gschemas/org.goblins.SoundRecognition.gschema.xml`, `os/dconf/db/<profile>.d/40-sound-recognition`, `os/bootc/Containerfile`, `crates/goblins-os-settings/src/main.rs` (Accessibility ▸ Sound Recognition panel), `crates/goblins-os-design/src/lib.rs` (reuse blue "listening"/orange "attention" tones — no new hue).
- **APIs:** onnxruntime CPU inference (YAMNet, 521 classes, 16kHz mono, 64 mel bins, ~100ms/2s on 2 threads); PipeWire capture via `parec`/`arecord`; `org.freedesktop.Notifications.Notify` (urgency=critical for alarm/siren); gio GSettings in the listener, the gsettings CLI bridge in core; `…a11y.keyboard visual-bell` as the honest flash path.
- **Goblins-grade:** Accessibility cards (radius 12); master toggle + an inset list of per-sound switches with category glyphs + one-line honest descriptions; calm `gos_system_blue` "Listening on this device" pill when ready, `gos_system_orange` only for an actual attention banner; notification with a **PNG** Goblins icon; Inter ramp; one blue / one radius / one motion.
- **Honest gating:** no `python3-onnxruntime` → `ready=false`; **weights never bundled** → model-missing with an "Add the recognition model" affordance (the local-model install/consent flow); no capture source → "Microphone capture is not ready on this device."; the listener **exits 0 doing nothing** when any dep is missing; mic contention → yields when voice capture is active and says so; **reliability string** "This recognizes sounds approximately and on-device only. Do not rely on it in emergencies or high-risk situations." (Apple's own caveat); defaults all-off (privacy: continuous mic).
- **Verifiable:** host — status struct serde, GSettings target mapping, per-sound id allowlist, honest-gating strings, capability-absent paths; gschema `--dry-run`. CI/qemu — onnxruntime/PipeWire/notify integration, gschema compile, the systemd-user unit, Settings render; the package adds are an image-build gate.
- **Effort:** XL · **Risk:** MED-HIGH. The listener **must** be a session-user unit (a core-side mic loop would silently never work). All-off defaults + explicit opt-in + fully on-device (no network in the listener). Convert the classifier to a static-input ONNX in the model-provisioning step, not at runtime. Boot/login untouched.

### `in-progress` Desktop Widgets + Today view
- [x] **Widget registry + layout substrate shipped** (`crates/goblins-os-core/src/today.rs` + `/v1/today/status` + `/v1/today/layout`, NEW `org.goblins.os.today` gschema via `os/glib-schemas/`): the glance-widget registry (each with its honest capability requirement — weather→location, brief→on-device model, calendar→account) and the layout model with pure `normalize_layout` (known-only, dedupe, preserve order) + `parse_gsettings_strv`, unit-tested (195 core tests). Honest-gated to a default layout when the schema is absent. `glib-compile-schemas` clean; clippy/fmt clean; route + schema verify gates.
- [x] **Today panel surface + menu-bar button source-gated (CI/qemu-pending):** the `goblins-os-today` GTK crate reads `/v1/today/status`, renders local Date/Clock cards with real local values, and renders Weather/Calendar/Daily Brief as honest empty states until location services, a calendar account, and a local model are actually available. The app uses shared Goblins UI theming, has a desktop launcher, a dconf seed for the default widget order, is copied into the image, and the render harness now captures `122-today.png` / `123-today-dark.png` from the installed binary. The `goblins-menubar` shell extension also exposes a local-clock Today date button that launches the installed Today helper, and `render-desktop.sh` now captures `59c-menubar-today-$suffix.png` after seeding deterministic clock preferences. Web verification found `gtk4-layer-shell-devel` in Fedora 44, but upstream documents GTK4 layer shell is unsupported on GNOME Wayland; this source-gated pass therefore does **not** add layer-shell packages or claim right-edge shell anchoring. Edge-swipe, live weather/calendar/brief data, reviewed pixel proof, and any shipped status remain CI/qemu-pending.
- **Packages:** **none in the source-gated GTK pass**. Do not add `gtk4-layer-shell` to the GNOME path until a GNOME-supported shell/portal strategy is proven. Future live weather/calendar work still needs exact Fedora 44 verification before adding `libgweather4`, `geoclue2`, or EDS packages.
- **gsettings/dconf:** READ `color-scheme`/`clock-format`/`clock-show-weekday`/`clock-show-seconds`; `org.gnome.GWeather4` units + default-location; `org.gnome.system.location enabled` (honest-gate auto-location/weather). OWN a compiled `org.goblins.os.today` (layout `a(sy)`, enabled-widgets, brief-enabled, weather-location, open-on-edge-swipe, reduce-translucency-respected) + a `20-goblins-os-today` seed.
- **Files:** `crates/goblins-os-today/{Cargo.toml,src/main.rs}` (NEW crate mirroring `goblins-os-control-center`; Today header + widget VBox, each widget returns a Goblins card with an honest empty state), workspace `Cargo.toml`, `os/bootc/Containerfile` (features + COPY binary + glib-compile-schemas **after** the gschema COPY), `os/glib-schemas/org.goblins.os.today.gschema.xml`, `os/dconf/db/local.d/20-goblins-os-today`, `…/goblins-menubar@goblins.os/extension.js` (future date/clock button + edge-swipe → spawn the binary), `os/applications/org.goblins.OS.Today.desktop`.
- **APIs:** GTK4 application window on the shared Goblins UI tokens; future shell/edge behavior belongs in the GNOME Shell extension path, not GTK layer shell, unless a GNOME-supported API is proven. Later live widgets use libgweather4 (prefer a gsettings-CLI read of the location for host-testability), geoclue2 D-Bus **only** when location enabled, EDS e-cal for the agenda, and the core AI bridge for the daily brief.
- **Goblins-grade:** mirror the control-center glass panel — `gos_material_thick` vibrancy, overlay radius 22, border+shadow; header long date `GOS_TYPE_TITLE_1` + weekday eyebrow + `themed_brand_mark(16)`; `gos-card` widget tiles (radius 12, 10px gaps); slide-in `MOTION_OVERLAY_MS` spring gated on animations; 360-380px right-anchored full-height column with a ScrolledWindow body.
- **Honest gating:** weather — location off/geoclue/network absent → "Turn on Location to see weather" deep-link (no fabricated forecast); agenda — no EDS account → "No calendars connected…"; daily brief — gated on the on-device resident (reuse `ResidentStatus`); model not loaded → "On-device brief unavailable…", **no cloud fallback**; world clock always works (pure-Rust tz math); reduced-translucency/high-contrast → opaque `gos_surface`, no spring.
- **Verifiable:** host — world-clock tz math, layout model (id+size order, add/remove/reorder), brief prompt assembly, weather-unit formatting, dconf layout parse. CI/qemu — layer-shell anchoring/slide-in, GTK render, menubar button + edge-swipe, geoclue/libgweather live data, EDS agenda (light+dark screenshots).
- **Effort:** XL · **Risk:** MED. Keep the layer-shell call behind a feature with a borderless right-aligned window fallback (verify Mutter anchoring at qemu render time); `glib-compile-schemas` must run **after** the gschema COPY; not boot/login-critical (spawned on demand). EDS empty on a fresh image is the honest empty state, not a bug.

### `in-progress` Autocorrect / Text Replacement (system-wide, own IBus engine)
- [x] **Curated-table substrate shipped** (`crates/goblins-os-core/src/text_shortcuts.rs` + `/v1/text-shortcuts` GET/POST + `/v1/text-shortcuts/preview`): the Replace→With table stored as JSON at `~/.config/goblins-os/text-shortcuts.json`, edited through the allowlisted bridge with the shared engine `sanitize_shortcuts` contract (trim, drop empties/identity, de-dupe last-wins, cap 500) and `find_replacement` (the exact word-boundary match the engine will perform) — both pure + unit-tested (185 core tests). `engine_available` honest-gating (the table is always editable; replacements apply only when the engine runs). The table needs no model — ships ready. clippy/fmt clean; route verify gate.
- [x] **Settings table editor source-gated (CI/qemu-pending):** Settings ▸ Keyboard fetches `/v1/text-shortcuts`, shows engine readiness honestly, lists saved Replace→With entries, removes entries, and adds/replaces entries through the existing core bridge. The UI sanitizes empty/identity entries and preserves the core last-wins de-dupe contract before POSTing. No IBus engine, packages, component XML, input-source seed, candidate bubble, password-field handling, or live text expansion is claimed yet.
- [x] **Engine-readiness gate source-gated (CI/qemu-pending):** core reports `engine_available=true` only when `ibus` is on PATH, the Goblins IBus component XML is installed, the Goblins engine binary is installed, the Goblins IBus input source is configured, and the live runtime loop is available. This keeps package/component installation from falsely marking Text Shortcuts expansion active before the session path is actually enabled and qemu-proven.
- [x] **Engine decision substrate source-gated (CI/qemu-pending):** `crates/goblins-os-textshortcuts-engine` provides pure trigger tracking, candidate, boundary commit, and password/hidden/sensitive-field refusal logic plus a `goblins-textshortcuts-engine --self-test` CLI. It is not installed in the image and does not claim live IBus expansion yet.
- [x] **Shared core/engine table contract source-gated (CI/qemu-pending):** core reuses the engine crate's `TextShortcut` JSON shape and `sanitize_shortcuts` helper for `/v1/text-shortcuts`, removing duplicate table behavior before live IBus integration.
- [x] **IBus package/component registration source-gated (CI/qemu-pending):** Fedora 44 package names are web-verified and lockstep-gated in the Containerfile install list and `rpm -q`; the image installs `/usr/libexec/goblins-os/goblins-textshortcuts-engine` plus `/usr/share/ibus/component/goblins-textshortcuts.xml` and runs the engine self-test/component-contract check. Core still reports `engine_available=false` until the input source and live runtime loop are present; no dconf seed or live expansion is claimed.
- [x] **IBus runtime-operation adapter source-gated (CI/qemu-pending):** engine decisions now map to explicit IBus operations: pass-through for ordinary keys, preedit update for candidates, delete-surrounding-text + commit-text for accepted replacements, and hide-preedit for clears. The installed `--self-test` asserts this contract, but no GI/IBus loop or live expansion is claimed.
- [x] **IBus key-event normalizer source-gated (CI/qemu-pending):** raw IBus key facts now normalize to the engine's `InputEvent` contract for characters, boundaries, Backspace, releases, navigation resets, and command-modified shortcuts. The future GI loop can reuse this without guessing at pass-through behavior.
- [x] **IBus runtime pipeline source-gated (CI/qemu-pending):** `IbusTextShortcutsRuntime` now composes raw-key normalization, content-purpose refusal, table/state ownership, and IBus operation emission behind one host-tested boundary. The installed self-test exercises candidate preedit and boundary commit through that pipeline, but no session loop, dconf seed, or live expansion is claimed.
- [x] **Engine table reload source-gated (CI/qemu-pending):** the engine crate now owns the JSON table-store path/status contract for `~/.config/goblins-os/text-shortcuts.json`, degrades missing/invalid/unreadable config to an empty pass-through table, and refreshes the runtime table while hiding stale preedit candidates. The CLI preview path uses the same store; no watcher or live IBus loop is claimed.
- [x] **IBus runtime event router source-gated (CI/qemu-pending):** the engine crate now routes key, focus, reset, content-purpose, and table-change events through one host-tested boundary. Focus loss/reset/table changes hide stale preedit candidates, password/sensitive focus remains pass-through, and the installed self-test uses the router; no live GI loop is claimed.
- [x] **Installed keystroke self-test source-gated (CI/qemu-pending):** `--keystroke-self-test` now exercises trigger typing, candidate preedit, boundary commit, password pass-through, and focus cleanup through the event router, and the Containerfile runs it during image build. This is still a source/image contract, not live text-input proof.
- [x] **Table watch/reload contract source-gated (CI/qemu-pending):** the engine crate now fingerprints the JSON table content, exposes `TextShortcutTableWatcher`, reloads only when the content state changes, preserves active candidates on unchanged polls, clears stale preedit candidates on changed/missing/invalid tables, and ships an installed `--table-watch-self-test` image-build gate. This is still a source/image contract; no live file monitor, GI loop, dconf seed, or text-input proof is claimed.
- [x] **IBus content-purpose decoder source-gated (CI/qemu-pending):** the engine crate decodes IBus numeric/symbolic content purposes and refuses replacements in PASSWORD/PIN fields through an installed `--content-purpose-self-test`; unknown purposes stay free-form. This is still a source/image contract; no live GI callback or text-input proof is claimed.
- [x] **IBus stdio runtime protocol source-gated (CI/qemu-pending):** `--stdio` provides a long-lived JSON protocol for key/focus/table events and returns explicit IBus operation JSON so the future GI shim can drive the Rust runtime without reimplementing replacement logic. The installed `--stdio-self-test` image gate covers candidate preedit, boundary commit, and PIN-field pass-through. This is still not a live IBus loop.
- [x] **IBus GI adapter source-gated (CI/qemu-pending):** `goblins-textshortcuts-ibus` registers the IBus engine, translates GI key/focus/content-purpose callbacks into the Rust `--stdio` runtime protocol, applies only returned preedit/delete/commit/hide operations, and fails open to pass-through on missing or unhealthy runtime state. The component XML points to this adapter, and the image runs pycompile + adapter self-test + component-contract gates. This is still not a seeded session input source or live expansion proof.
- [x] **IBus adapter capability handshake source-gated (CI/qemu-pending):** `goblins-textshortcuts-ibus --capability-check` proves the installed adapter can run the Rust `--stdio-self-test` contract and reports `adapter_contract_ready=true`, while keeping `ready=false` and `runtime_ready_claim=false`. The image build checks both the contract and the false runtime claim. This still does not prove live IBus callbacks, focused-field commits, password-field refusal in-session, or the accept bubble.
- [x] **IBus adapter table-reload bridge source-gated (CI/qemu-pending):** the adapter reads the curated table JSON, sanitizes it before sending, and emits a stdio `table-changed` request on first use and file-content changes so the Rust runtime can hide stale preedit and use current shortcuts. This still does not prove a live IBus session, file monitor, focused-field commits, password-field refusal in-session, or the accept bubble.
- [x] **IBus adapter runtime self-test source-gated (CI/qemu-pending):** `goblins-textshortcuts-ibus --runtime-self-test` launches the real Rust `--stdio` child through the Python bridge, proves table-change + key-event preedit/commit operations, and proves PIN-purpose pass-through with no operations. The image build runs it. This still does not prove a live IBus bus, focused-field commits, password-field refusal in-session, or the accept bubble.
- [x] **IBus accept-bubble dismiss contract source-gated (CI/qemu-pending):** Escape now normalizes to a dedicated candidate-dismiss event, handles the key only when a candidate is visible, hides preedit without committing, and stays pass-through otherwise. The Rust keystroke/stdio self-tests and Python adapter runtime self-test cover the contract. This still does not prove the live IBus bus, focused-field callbacks, or rendered accept bubble.
- [x] **Autocorrect capability gate source-gated (CI/qemu-pending):** `/v1/text-shortcuts` now reports a disabled autocorrect capability that becomes resource-available only when a local model path or Hunspell dictionary is present, and Settings shows a read-only Autocorrect row. This still does not add packages, enable a toggle, ship a model, or perform live autocorrect.
- [x] **IBus session seed source-gated (CI/qemu-pending):** the Goblins session starts a user `ibus-daemon`, seeds the `goblins-textshortcuts` IBus source and preload engine in dconf, and removes the old forced simple GTK/QT/XIM overrides without setting `GTK_IM_MODULE=ibus` globally. Core still keeps runtime readiness false until qemu proves the session service, active input source, adapter callbacks, and safe replacement commits.
- [x] **IBus session-enable hardware proof hook source-gated (CI/qemu-pending):** the display-backed VM harness now requires `text-shortcuts-session-enable-proof.json` before signoff, proving the installed session service/source/preload/active-engine path and adapter self-test while explicitly keeping core `engine_available=false` and `runtime_loop_available=false`. This does not prove live keystroke replacement, adapter callbacks from a focused text field, password-field refusal in-session, or the accept bubble.
- [x] **IBus live-keystroke hardware proof hook source-gated (CI/qemu-pending):** the display-backed VM harness now launches `goblins-os-shell --text-shortcuts-proof normal|passthrough|dismiss|password`, drives focused GTK entries with host QMP keyboard input, and requires normal replacement (`onmyway.`), unknown-word pass-through (`hello.` unchanged), Escape dismiss without replacement commit, and password-purpose refusal (`omw.` unchanged, `password_refusal=true`) before signoff. Core still keeps `runtime_ready_claim=false` until the qemu artifact is reviewed and the runtime gate is flipped deliberately.
- [x] **IBus overlay-intent hardware proof hook source-gated (CI/qemu-pending):** the display-backed VM harness now requires `text-shortcuts-overlay-intent-proof.json`, generated from the installed adapter's `--overlay-intent-self-test`, and rejects signoff unless it records two candidate show intents, two hide intents, dismissed and committed hide reasons, and false rendered/live/runtime readiness claims. This is still not live rendered overlay proof and does not mark Text Shortcuts shipped.
- [x] **IBus accept-bubble frame contract source-gated (CI/qemu-pending):** `goblins-textshortcuts-ibus --candidate-bubble-frame-self-test` now converts the adapter's overlay intents into deterministic `goblins-textshortcuts-accept-bubble-frame` show/hide frames for the existing `gos-text-shortcuts-candidate` renderer surface, proves dismissed and committed hide frames plus sensitive-field refusal, and keeps `rendered_bubble_ready_claim=false`, `live_overlay_claim=false`, and `runtime_ready_claim=false`. The Containerfile and verifier require the proof. This still does not prove a live rendered accept bubble.
- [x] **IBus accept-bubble layout contract source-gated (CI/qemu-pending):** `goblins-textshortcuts-ibus --candidate-bubble-layout-self-test` derives deterministic layout records from the accept-bubble frames, proves below-cursor placement, right-edge clamp, bottom-edge flip, hide-frame collapse, Inter, and the `gos-text-shortcuts-candidate` style contract while keeping `rendered_bubble_ready_claim=false`, `live_overlay_claim=false`, and `runtime_ready_claim=false`. The Containerfile, `goblins-os-verify`, and shipping-status gate require the proof. This still does not prove a live rendered accept bubble.
- [x] **IBus accept-bubble layout hardware proof hook source-gated (CI/qemu-pending):** the display-backed VM harness now requires `text-shortcuts-candidate-bubble-layout-proof.json`, generated from the installed adapter's `--candidate-bubble-layout-self-test`, and rejects signoff unless it records the layout/frame surfaces, four layout records, three visible layouts, right-edge clamp, bottom-edge flip, hide-frame collapse, Inter, the Goblins candidate style class, and false rendered/live/runtime readiness claims. This is still not live rendered overlay proof and does not mark Text Shortcuts shipped.
- [x] **IBus accept-bubble render-intent bridge + hardware proof hook source-gated (CI/qemu-pending):** `goblins-textshortcuts-ibus --candidate-bubble-render-intent-self-test` now drives the adapter through the real Rust stdio runtime and proves deterministic render-intent records sourced from the overlay/frame/layout contracts, including dismiss, commit, focus-out hide, sensitive-field hide, pass-through unchanged behavior, fail-open sink handling, Inter, and the `gos-text-shortcuts-candidate` style contract while keeping `rendered_bubble_ready_claim=false`, `live_overlay_claim=false`, and `runtime_ready_claim=false`. The Containerfile, `goblins-os-verify`, shipping-status gate, close-signoff, runbook, and display-backed VM capture harness now require `text-shortcuts-candidate-bubble-render-intent-proof.json`. This still does not prove a live rendered accept bubble.
- [x] **IBus accept-bubble render screenshot proof hook source-gated (CI/qemu-pending):** the display-backed VM harness now requires `31-text-shortcuts-candidate-bubble-render.png` plus `text-shortcuts-candidate-bubble-render-proof.json`, launched from `goblins-os-shell --text-shortcuts-proof candidate-render`, and rejects signoff unless the screenshot proof links the rendered Goblins candidate surface to the render-intent, layout, frame, Inter, and `gos-text-shortcuts-candidate` contracts while keeping rendered/live/runtime readiness claims false. This still does not prove a live IBus overlay, focused-field callback, or Wayland text-input-v3 bubble.
- [x] **IBus GI adapter contract source-gated (CI/qemu-pending):** `goblins-textshortcuts-ibus --gi-adapter-contract-self-test` imports the real `gi.repository.IBus` boundary in the image, uses `IBus.Text.new_from_string` and `IBus.keyval_to_unicode`, and drives the Rust stdio runtime through the focus/key/content-purpose/focus-out/reset callback shape. The proof requires preedit update, boundary commit, Escape dismiss, default pass-through, password/PIN refusal, and the existing render-intent contract, while keeping rendered/live bus/text-input/runtime claims false. This still does not prove live IBus callbacks from a focused app or a Wayland text-input-v3 commit.
- [x] **IBus adapter callback ledger source-gated (CI/qemu-pending):** `goblins-textshortcuts-ibus --adapter-callback-ledger-self-test` proves the adapter can record callback categories (`focus-in`, `process-key-event`, `set-content-type`, `focus-out`, `reset`), operation types (`update-preedit-text`, `delete-surrounding-text`, `commit-text`, `hide-preedit-text`), and render-intent counts without logging the typed trigger or replacement value. The live adapter path can emit the same redacted ledger only when `GOBLINS_TEXTSHORTCUTS_PROOF_EVENTS` is set. The Containerfile and verifier require the proof, redaction guard, and false rendered/live bus/text-input/runtime claims. This still does not prove live IBus callbacks from a focused app, Wayland text-input-v3 commits, or a live rendered accept bubble.
- [x] **Live IBus runtime/render qemu proof path source-gated (CI/qemu-pending):** the display-backed VM harness now requires `text-shortcuts-live-ibus-runtime-render-proof.json` and `32-text-shortcuts-live-ibus-runtime-render.png`, launches `goblins-os-shell --text-shortcuts-proof live-runtime-render`, propagates the redacted adapter ledger into the user IBus service, and rejects signoff unless the live run proves the active Goblins IBus engine, focused-field callback, `text-input-v3` commit, normal expansion, pass-through, password refusal, and rendered accept bubble while keeping the core readiness flip explicitly deferred. This still needs a real CI/qemu artifact review and does not mark Text Shortcuts shipped.
- [ ] **Live IBus runtime/render ship proof (deferred, XL/highest-risk):** prove the installed `goblins-textshortcuts` IBus engine loop in a real GNOME/Wayland session, focused-field preedit/commit over `text-input-v3`, pass-through by default, password-field refusal in-session, and the rendered accept bubble. The optional model-gated autocorrect tier remains resource-gated.
- **Packages:** `ibus`, `ibus-gtk4`, `ibus-gtk3`, `ibus-libs`, `python3-ibus` (web-verified for Fedora 44 and asserted with `rpm -q` per the Containerfile convention). NOTE `ibus-typing-booster` exists but is Hunspell prediction, **not** a curated table — wrong fit for the default.
- **gsettings/dconf:** `org.freedesktop.ibus.general preload-engines` (+`goblins-textshortcuts`); `org.gnome.desktop.input-sources sources=[('ibus','goblins-textshortcuts')]`, `per-window=false`; dconf seed in `10-goblins-os-desktop`. The replacement table itself is **JSON** under `~/.config/goblins-os/text-shortcuts.json`, written only through the core bridge — not a gsetting.
- **Files:** `os/bootc/Containerfile` (ibus packages + register the engine component XML; **reconcile** the existing `GTK_IM_MODULE=gtk-im-context-simple` block; enable ibus via the GNOME session, **not** a global env flip), `crates/goblins-os-core/src/text_shortcuts.rs` (NEW — allowlisted table CRUD, same Command/honest-gating shape), `crates/goblins-os-core/src/main.rs` (`/v1/text-shortcuts`), `crates/goblins-os-core/src/ai.rs` (allowlist the add-a-shortcut safe-setting target), `os/goblins-os-textshortcuts/` + `crates/goblins-os-textshortcuts-engine/` (NEW — the IBus engine: component XML + a native binary reading the JSON table and driving preedit/commit), `crates/goblins-os-settings/src/main.rs` (Text Shortcuts table editor + Autocorrect toggle), `os/systemd-user/` (ibus-daemon for the goblins-os session), `crates/goblins-os-verify/src/main.rs` (**REWRITE** the blunt `ibus-disabled-for-native-session` gate → a precise one: legacy GTK IM popover stays off, the goblins engine is registered + the input source seeded).
- **APIs:** IBus engine via GI (`IBus.Engine` subclass — `process_key_event`, `update_preedit_text`, `commit_text`, `hide_preedit_text`); component XML under `/usr/share/ibus/component/`; **`text-input-unstable-v3`** — mutter bridges IBus to GTK3/GTK4/Electron over this protocol **regardless of `GTK_IM_MODULE`** (this is why the feature is genuinely system-wide **and** why the current env flip does NOT actually block it); the core HTTP bridge for the table.
- **Goblins-grade:** a first-class Text Shortcuts editor (not a gnome-control-center handoff): grouped inset Replace→With rows (radius 12, height 38), a "+" footer rung, inline edit-in-place, calm graphite delete; the in-field accept bubble **rebranded** off stock IBus chrome (radius 22 surface + material + shadow, single candidate, Space/Return accept, Esc dismiss, design-system motion); Autocorrect is a single honest toggle with a plain neutral status line; faint design-system preedit underline, no IBus blue.
- **Honest gating:** the curated table needs **no model** — ships ready; if the daemon/engine isn't running → "Text Shortcuts are unavailable on this session" (no fake-success toggle). The autocorrect tier IS model/dictionary-gated — lights up only with the on-device model OR Hunspell dictionary present; absent that, the toggle shows but the status states it and the engine commits nothing. Per-app reality stated honestly (apps that ignore text-input-v3 won't get replacements).
- **Verifiable:** host — table CRUD, JSON schema, trigger/boundary matching, password-field refusal logic, ibus/gsettings-absent gating. CI/qemu — the engine (preedit/commit over text-input-v3), the Settings panel, and the verify-gate rewrite (only provable with a real GNOME session + a scripted keystroke selftest in `os/bootc/run-selftest.sh`).
- **Effort:** XL · **Risk:** HIGHEST in bucket — boot/login-adjacent (changes the session input path for **every** text field). The engine **must** be pass-through by default (`process_key_event` returns false except on a confirmed trigger+boundary); **never engage in password/secret fields** (honor IBus content-purpose PASSWORD); keep the legacy GTK IM popover OFF; gate the whole feature behind CI/qemu render + an end-to-end keystroke selftest before flipping the verify gate.

### `in-progress` Visual Look Up (identify the subject in any image)
- [x] **VLM relay substrate shipped** (`crates/goblins-os-core/src/vision.rs` + `/v1/vision/status` + `/v1/ai/visual-lookup`): capability gate + the on-device identify relay (base64 image → loopback runtime `/api/generate` → identification card), modeled on `voice.rs`/`resident.rs`. **Loopback-only** (`is_loopback_url` — `127.0.0.1`/`localhost`/`[::1]`, no exfil), zero new packages, honest-gated to "add a vision model" until a runtime is configured. Pure `is_loopback_url`/`extract_json_object`/`parse_identification` (JSON-or-honest-fallback) unit-tested (191 core tests); clippy/fmt clean; route gate.
- [x] **Region-capture card surface source-gated (CI/qemu-pending):** the `goblins-os-visual-lookup` crate checks `/v1/vision/status` before capture, uses the ashpd interactive `Screenshot` portal for user-selected regions, copies pixels into a 0700 runtime dir as a 0600 file, POSTs the local path to `/v1/ai/visual-lookup`, deletes the temp image, and renders a branded identification card with honest "Best guess"/model-missing copy. Settings ▸ Goblin & Models now has a Vision row and the shared AI action registry exposes `identify-in-image`. The GTK card/portal render remains CI/qemu-pending.
- [x] **Launcher desktop entry source-gated (CI/qemu-pending):** `org.goblins.OS.VisualLookup.desktop` exposes Visual Look Up in the applications grid and launches the already-packaged `/usr/libexec/goblins-os/goblins-os-visual-lookup` helper. The bootc image validates the desktop entry and Exec path; no default shortcut, model bundle, portal render, or identification success is claimed until CI/qemu proves it.
- **Packages:** **none** (the safest decision: `llama-cpp` is in Fedora 44 but `ollama` is COPR-only, and neither bundles a model — ship the capability gated and let users add a runtime+model, matching the `model_manager`/voice thesis; zero new `rpm -q` lines = zero image-build risk).
- **dconf:** no new binding in the source-gated pass. The old `<Shift><Super>4` proposal collides with the shipped GNOME screenshot UI binding in `10-goblins-os-desktop`, so pick/prove a non-conflicting shortcut in CI/qemu before enabling Visual Look Up by default. **No new schema** — env overrides `GOBLINS_OS_VISION_{DIR,RUNTIME_URL,MODEL}` (loopback http only); reuses the existing `screen-context` policy control as the gate.
- **Files:** `crates/goblins-os-core/src/vision.rs` (NEW — VLM capability + identify, modeled on `voice.rs` + the `resident.rs` Ollama relay; `VisionStatus` + `identify(image_path, hint)` POSTing base64 to the loopback runtime's `/api/generate` with `images[]` and a Visual-Look-Up system prompt → `{name,category,confidence,description,follow_ups}`), `crates/goblins-os-core/src/main.rs` (`GET /v1/vision/status`, `POST /v1/ai/visual-lookup`), `crates/goblins-os-ai/src/lib.rs` (one `AiAction` `identify-in-image`), `crates/goblins-os-visual-lookup/` (NEW crate — the branded capture+card surface: ashpd `Screenshot::request().interactive(true)` region select, 0700/0600 private capture, POST to core, render the card; reuses the screenshot-context portal/permission code), `os/dconf/db/local.d/10-goblins-os-desktop`, `os/bootc/Containerfile` (COPY the binary; **no** model/runtime packages), `os/applications` (optional .desktop), `crates/goblins-os-settings/src/main.rs` (AI & Models Vision row), `crates/goblins-os-verify/src/main.rs` (copy/keybinding pins — no Apple/Siri terms).
- **APIs:** portal `Screenshot` with `.interactive(true)` (sanctioned Wayland region capture; GNOME 42+ blocks external `org.gnome.Shell.Screenshot`); the loopback-only relay (Ollama `/api/generate` `images:[base64]`, or llama.cpp `--mmproj`), strictly `127.0.0.1`/`localhost`/`::1` reusing `resident.rs` `local_http_url`; `ureq` with bounded timeouts (vision turns are slower — honor `GOBLINS_OS_RESIDENT_TIMEOUT_SECS`).
- **Goblins-grade:** an identification **card** (overlay radius 22, shared vibrancy): subject name `GOS_TYPE_TITLE_2`, description `GOS_TYPE_BODY`, a category glyph chip (leaf/paw/landmark/artwork/tag) tinted from the **one** accent (never a second hue); confidence as **plain honest text** ("Likely a…"/"Best guess…"), not a colored badge; follow-ups ("Search the web", "Ask Goblin about this", "Copy name") on a 38px rung; **PNG** glyphs only; copy "Goblin identified…", never "Siri"/"Apple".
- **Honest gating (central constraint):** **gpt-oss is text-only and cannot see images** — Visual Look Up CANNOT reuse the default resident; it requires a separate VLM (Qwen2.5-VL / Gemma3 / LLaVA) the user adds, weights never bundled. Ladder: no runtime/model → greyed, card links to AI & Models; `screen-context` denied/offline → existing FORBIDDEN copy; portal cancelled/timed-out → screenshot-context recovery copy, no pixels sent; low confidence → say "Best guess" honestly. Pixels go **only** to a loopback runtime, never the network; capture file 0600 in a 0700 dir, deleted after.
- **Verifiable:** host — capability detection, identify request-body shape, loopback-only URL gate (clone `resident.rs` tests), offline/screen-context policy StatusCode, VisionStatus/card serde, copy pins; a localhost `TcpListener` fake round-trips a fake `/api/generate` vision reply end-to-end. CI/qemu — ashpd interactive capture, the GTK card (light+dark), the dconf keybinding firing.
- **Effort:** L · **Risk:** LOW (no packages → no image-build risk; new helper + endpoint only). Keep vision on a **separate** relay codepath/endpoint so the text-only resident path never regresses. Mitigate hallucination with "Best guess" copy + a verify pin.

---

## Bucket D — Boot/login-critical (qemu-gated)

**Land last.** These touch the install path / on-disk layout / boot unlock. Every item is gated behind the qemu kickstarts + the hardware gate, and several require **coordinated verify-crate rewrites** in the same change (the single biggest source of a red gate).

### `in-progress` FileVault-style full-disk encryption at install
- [ ] LUKS2 root bound to **TPM2 for auto-unlock**, with a **mandatory escrowed recovery key** — a first-class "Encrypt this disk" choice in the Goblins installer + a read-only Encryption posture row in Settings ▸ Security. Encrypt by default with transparent TPM boot, but **never** without a captured recovery key, and fall back to a recovery-key prompt whenever the TPM measurement changes (matching FileVault: hardware auto-unlock is convenience over an always-present credential).
- [x] **Read-only encryption posture substrate source-gated (CI/qemu-pending):** core exposes `GET /v1/security/encryption`, parses `/proc/self/mountinfo` and `/etc/crypttab`, reads `cryptsetup status` and `systemd-cryptenroll --list` only when present, reports Secure Boot and TPM-device posture, and always returns `executes_enrollment=false`. The bootc image now installs and `rpm -q` asserts Fedora 44 `cryptsetup` and `tpm2-tss`, plus `command -v cryptsetup`, `command -v systemd-cryptenroll`, and the TPM2 TSS runtime library. This does **not** enable install-time encryption, mint or escrow recovery keys, enroll TPM tokens, edit crypttab, change installer block setup, or flip any root filesystem.
- **Packages:** `cryptsetup`, `tpm2-tss` (add + `rpm -q` explicitly for the initramfs unlock path; `systemd-cryptenroll` ships with Fedora's `systemd` package). `systemd-cryptsetup` is **not** a Fedora 44 RPM package name, so it is not installed directly. `clevis` NOT needed.
- **gsettings/dconf:** none — it's a one-time install-engine decision, **not** a runtime toggle. Settings surfaces read-only live status via a new `/v1/security/encryption` (shells `cryptsetup status` + `systemd-cryptenroll --list`).
- **Files:** `crates/goblins-os-core/src/install_targets.rs` (accept `tpm2-luks`; build `--block-setup tpm2-luks`; tpm-absent→key-only degradation; recovery-key-required gate), `crates/goblins-os-installer/src/main.rs` (the encryption card + the mandatory recovery-key step), `crates/goblins-os-settings/src/main.rs` (Encryption posture row in `build_security`), `crates/goblins-os-verify/src/main.rs` (**REWRITE** the gate strings that currently pin the opposite reject-contract — `install-simple-api-routes-tpm2-luks-to-full-storage` / `install-policy-tpm2-luks-guidance` / `install-simple-api-direct-block-only-contract`), `os/bootc/Containerfile`, `os/iso/verify-install.ks` + `verify-install-dark.ks`, `crates/goblins-os-design/src/lib.rs`.
- **APIs:** `bootc install to-disk --block-setup tpm2-luks --filesystem xfs --wipe <dev>` (the documented LUKS-on-TPM2 path); `systemd-cryptenroll --tpm2-device=auto --tpm2-pcrs=7` for auto-unlock + `--recovery-key` for escrow; `cryptsetup luksDump`/`status` for read posture; `/etc/crypttab tpm2-device=auto,tpm2-pcrs=…`; **Plymouth** (existing goblins-os theme) for the branded recovery-key fallback prompt.
- **Goblins-grade:** an installer "Encryption" inset card right after disk selection / before the destructive-ack: "Encrypt this disk (recommended)" pre-selected + "Don't encrypt"; then a **mandatory** Recovery Key step mirroring FileVault — a monospace 24-char (8×3) copyable key, an "I've saved my recovery key" checkbox that **gates Continue**, "Goblins OS cannot recover your data without this key"; Security pill neutral "encrypted · TPM auto-unlock" vs amber "encrypted · key-only"; Inter + the mono ramp; brand the boot-time unlock via the Plymouth theme.
- **Honest gating:** TPM auto-unlock attempted only when a TPM device is present AND Secure Boot state is readable (reuse `SecureBootStatus` + a new tpm probe) — no TPM → drop to recovery-key/passphrase-only and **say so** ("This computer has no TPM, so you'll enter your recovery key at every boot"); the recovery key is **minted before any TPM binding** (closes bootc #421/#477 — bare tpm2-luks ships with no fallback and is unbootable when PCRs change); **PCR policy pinned to PCR7 only** to survive ostree updates (warn that firmware/Secure-Boot changes re-prompt once, per bootc #561); TPM enroll fails post-format → install still **succeeds** as key-only, Security reports "encrypted · recovery-key only."
- **Verifiable:** host — extend `simple_install_block_setup`/`simple_install_filesystem` to assert tpm2-luks accepted, the command vector contains `--block-setup tpm2-luks`, the tpm-absent degradation, the recovery-key gate; the new endpoint's luksDump/cryptenroll parse. CI/qemu — installer card + Security row render, real bootc tpm2-luks install, real cryptenroll, **PCR7 auto-unlock across a reboot**, the Plymouth recovery-key fallback (the qemu kickstarts + the hardware gate).
- **Effort:** L · **Risk:** BOOT-CRITICAL. The recovery-key escrow **is** the de-risk (bare `tpm2-luks` is a known unbootable break). Avoid PCR over-binding (PCR7 only — binding 0/4/11 breaks on every ostree update). Keep `direct` as the still-offered "Don't encrypt" path; never auto-enable without the captured-key gate; keep the destructive-ack + `GOBLINS_OS_ENABLE_DESTRUCTIVE_INSTALL` env gate exactly as-is.

### `in-progress` btrfs `/home` + local snapshots + restore UI (Time Machine analogue)
- [ ] Automatic local snapshots + an honest "last snapshot" status surface + a timestamped restore browser that recovers files from a chosen snapshot — never silently mutating the live system, always explicit and reversible (default side-by-side, no in-place rollback from the GUI).
- [x] **Read-only snapshots status substrate source-gated (CI/qemu-pending):** core exposes `GET /v1/snapshots/status` and fail-closed `POST /v1/snapshots/restore`, parses `/proc/self/mountinfo`, parses `snapper -c home list --machine-readable`, reports `available=false` with "Local snapshots need a btrfs /home" on xfs/missing-tool/config cases, and always returns `executes_restore=false` until a real non-destructive restore path is qemu-proved. The bootc image now installs and `rpm -q` asserts Fedora 44 `btrfs-progs` and `libbtrfsutil`, plus `command -v btrfs`. This does **not** flip the installer root filesystem, create snapper configs/timers, render Settings UI, create snapshots, or restore files.
- **Packages:** `btrfs-progs`, `libbtrfsutil`, `snapper`, `snapper-tools`, `python3-dnf-plugin-snapper`, `deja-dup` (snapper + deja-dup already installed + `rpm -q`-verified; **`btrfs-progs`/`libbtrfsutil` are the gap** — verify present in fc44 before adding).
- **gsettings/dconf:** no GNOME schema governs btrfs snapshots — snapper is file-based (`/etc/snapper/configs/home`, `/etc/sysconfig/snapper`) + D-Bus `org.opensuse.Snapper`. `deja-dup` (external-target fallback only) exposes `org.gnome.DejaDup` keys. So local snapshots are config-only at the OS layer, surfaced through a NEW allowlisted core bridge — deliberately no gsettings panel.
- **Files:** `os/bootc-install/00-goblins-os.toml` (**`[install.filesystem.root] type = "btrfs"`** replacing `xfs`), `os/bootc/Containerfile`, `crates/goblins-os-core/src/snapshots.rs` (NEW — read + restore engine; parse `snapper --machine-readable`; off-state when btrfs/snapper absent; **no fabrication**, mirroring `system_image.rs`), `crates/goblins-os-core/src/main.rs` (`GET /v1/snapshots/status`, `POST /v1/snapshots/restore`), `crates/goblins-os-settings/src/main.rs` (a "Snapshots" group in Recovery/Storage + the restore browser), `crates/goblins-os-verify/src/main.rs`, `os/snapper/home`, `os/systemd-system/goblins-os-snapshot-timeline.timer` + `…-cleanup.timer`.
- **APIs:** `snapper -c home list --machine-readable`/`create`/`delete`/`undochange` (read+restore via the Command bridge); D-Bus `org.opensuse.Snapper` alt; `bootc install-config [install.filesystem.root] type="btrfs"`; snapper config targets the **`/var/home` subvolume** (bootc home is `/var/home`, in the root stateroot); branded systemd timers for hourly/daily timeline + cleanup; axum read-only handlers (mirror `recovery_status`); GTK4 + libadwaita restore browser.
- **Goblins-grade:** **(1) Status** — a "Snapshots" group (mirror `build_recovery`/`build_storage`) with `health_row` headline ("Snapshots on — last local 14 min ago") + status tones (green/amber/neutral) + rows for count/oldest/disk used/schedule + an honest deja-dup external-target row. **(2) Restore browser** — a left-rail timeline of timestamps (relative + absolute), a file/folder picker for the chosen snapshot, and a single explicit "Restore selected to…" that copies **out** of the read-only snapshot (default side-by-side, never in-place without confirm); generous spacing, control-center vibrancy, motion tokens, a calm empty state ("No snapshots yet — the first runs within the hour").
- **Honest gating (mirror `system_image.rs`):** snapper/btrfs absent, root not btrfs (existing installs are **XFS** — this applies to NEW installs / re-formats), config missing, or command error → `available=false` + truthful detail ("Local snapshots need a btrfs /home; this system was installed on xfs") and an honest off-state, not a fake timeline; restore gated behind explicit confirmation + side-by-side default; deja-dup a separate clearly-labeled row reporting its own state, "not configured" until the user sets a target (no silent cloud/secret writes); the browser only lists snapshots snapper actually reports.
- **Verifiable:** host — parse `snapper --machine-readable`, off-state when absent, no-fabrication logic; the verify-crate gates (package presence, install-config btrfs, file-map mirrors). CI/qemu — the libadwaita restore browser, real snapper snapshot/restore, the btrfs subvolume layout, the real installer btrfs path (qemu render + selftest).
- **Effort:** XL · **Risk:** BOOT/IMAGE-CRITICAL. Flipping root xfs→btrfs changes the on-disk layout for **every** new install and the whole image-build/installer path — `install_targets.rs` currently hard-codes `xfs` as `DEFAULT_FILESYSTEM` and **rejects btrfs** (lines 1548-1556), so it must change in lockstep or the installer's own validation refuses the new default. bootc does **not** auto-create/mount a separate `/home` — snapper must target `/var` (or a declared `@home` subvol), or snapshots silently cover the wrong tree. **Lower-risk first cut:** ship snapshots only when the user picks btrfs (keep simple-install on xfs); land the btrfs root + snapper config + timers first and keep the bridge + UI read-only/honest so an xfs system shows off-state. Keep restore non-destructive (side-by-side). NOTE Fedora 44 PackageKit moved to DNF5 — snapper's DNF integration needs the dnf5 plugin path (relevant only if auto-snapshotting on package ops).

---

### Suggested sequence

Favor safe + high-brand-impact early; keep the boot-critical items last and qemu-gated.

1. **Batch 1 — Bucket A (Live Text/OCR, Color Picker).** Real RPM binaries, the proven screenshot-context/voice precedents, mostly host-testable logic, no boot surface. Highest brand-impact per unit of risk — ship first. *(IME/CJK is also Bucket A but defer it to Batch 4 — it reverses an intentional boot/login + `Super+Space` decision.)*
2. **Batch 2 — Bucket B shell surfaces with zero image-build risk (App Exposé, Snap Assist, Hot Corners).** Pure JS/CSS/gschema in already-shipped extensions; the only gate is the accent-pin test + a qemu render. Visible, delightful, contained.
3. **Batch 3 — Bucket B settings rows on the allowlisted bridge (Accessibility rows, Firewall **status**, Keyboard shortcut editor, Focus, Migration Assistant, Multi-display).** Each is "own a small surface on a stable seam"; land read/status paths first, gated writes second (Firewall toggle waits on the scoped polkit rule; Multi-display write waits behind the capability gate; land Personal Hotspot here once `dnsmasq` is in).
4. **Batch 4 — Bucket C engines + IME/CJK.** Net-new, weights-gated, each its own project. Order within: Voice Control → Live Captions → Visual Look Up (LOW image risk, no packages) first; then Today/Widgets (first layer-shell), Sound Recognition + Switch Control (XL real-time engines), Autocorrect/IBus + IME/CJK **last in the batch** (system-wide input path; needs the verify-gate rewrite + keystroke selftest).
5. **Batch 5 — Bucket D, last, fully qemu-gated (FileVault-at-install, then btrfs `/home` snapshots).** Touch the install path / on-disk layout / boot unlock, and each needs a **coordinated verify-crate rewrite** in the same change. Run the full hardware gate + a fresh install→auto-update→rollback cycle before either is called green.
