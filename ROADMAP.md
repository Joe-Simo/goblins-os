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

## ⏩ Session status — RESUME HERE (updated 2026-06-27)

Proven code head before the current QMP-startup fix is `d9354b0` on `main`. The
latest completed source passes shipped the Sound Recognition and Live Captions
substrates, fixed the Fedora 44 `sushi` package name, added the App Exposé / Hot
Corner desktop-proof hooks, changed the image workflow to avoid exporting the
full bootc image into the runner daemon, and added nonblocking BuildKit GHA
cache scopes for the expensive bootc image builds. Host gates for that source:
`cargo fmt --all --check`, `cargo clippy --workspace -- -D warnings`,
`cargo test --workspace`, `git diff --check`, and
`goblins-os-verify --source-root .` → **blocked=0 (1553)**.

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

**Done so far (21 of 26 features advanced):**
- **Batch 1 (Bucket A) — complete:** Live Text/OCR (core+handoff+markup Copy Text),
  Color picker. *(IME read+list also shipped.)*
- **Batch 2 (shell) — shipped with CI/qemu render proof:** App Exposé, Hot
  Corners, Snap Assist.
- **Batch 3 (Settings surfaces) — all 9 have a shipped read/status/UI surface:**
  Accessibility rows, Firewall, Keyboard shortcuts, Focus (substrate+gschema),
  Migration (substrate), Multi-display (read side via `displays.rs`), Personal
  Hotspot, Per-app privacy, Keychain. **Gated WRITES remain qemu-pending** for
  firewall toggle, IME set, focus arm, per-app revoke, multi-display apply, and
  keyboard rebind.
- **Batch 4 (engines) — 7 of 7 SUBSTRATES shipped (cores only; UI/engines deferred):**
  Text Shortcuts, Voice Control, Visual Look Up, Switch Control, Widgets/Today,
  Sound Recognition, Live Captions.

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

Current implementation continuation: the IME/input-source **set** substrate is
now source-gated but not shipped. Core exposes `/v1/input/sources`, validates the
existing configured sources with a narrow `xkb`/`ibus` allowlist, encodes the
`a(ss)` GVariant, and honestly fails when gsettings or the schema/key is absent.
Settings ▸ Keyboard now adds source-row Move up / Move down / Remove controls
against that route; the last source cannot be removed, and this pass does not
install new CJK engines, add a source picker, change IME environment defaults, or
restore `Super+Space`. Local source gates: `cargo fmt --all --check`,
`cargo clippy --workspace -- -D warnings`, `cargo test --workspace`,
`goblins-os-verify --source-root .` → **blocked=0 (1563)**, scoped
`git diff --check`, `bash -n os/hardware-gate/verify-shipping-status.sh`, and
the Rust 1.88 GTK container
`cargo clippy -p goblins-os-settings --features goblins-os-settings/native-desktop -- -D warnings`.
GTK render, live source switching, menu-bar indicator, candidate window, and
input-source interaction proof remain CI/qemu-pending.

Current Focus continuation: Focus arm/disarm/tick is now source-gated but not
shipped. Core exposes `/v1/focus/activate`, `/v1/focus/deactivate`, and
`/v1/focus/tick`; validates configured mode JSON, snapshots/restores global
notification banners through the shared notifications bridge, records whether
Focus was armed by a schedule, and makes the tick path leave manual Focus modes
alone. The system gschema now includes `armed-by-schedule`, `restore-banners`,
and reserved `restore-apps` keys. Local source gates: `cargo fmt --all --check`,
`cargo clippy --workspace -- -D warnings`, `cargo test --workspace`,
container `glib-compile-schemas --dry-run os/glib-schemas`, scoped
`git diff --check`, `bash -n os/hardware-gate/verify-shipping-status.sh`, and
`goblins-os-verify --source-root .` → **blocked=0 (1571)**. Settings/Control
Center/menu-bar surfaces, per-app breakthroughs, a user timer, and live qemu
write proof remain deferred.

Current Per-app Privacy continuation: app-keyed portal permission revokes are now
source-gated but not shipped. Core exposes `/v1/app-privacy/revoke`, validates
the known PermissionStore tables plus safe desktop app/resource IDs, and calls
`org.freedesktop.impl.portal.PermissionStore.DeletePermission(table, id, app)`
only for app-keyed grants; resource-keyed device grants remain read-only until
the store can map resources back to owning apps. Settings ▸ Privacy now renders
one row per app-keyed grant with a Revoke action and reports the exact core
outcome. Local source gates: `cargo fmt --all`, `cargo clippy --workspace -- -D warnings`,
`cargo test --workspace`, the Rust 1.88 GTK container
`cargo clippy -p goblins-os-settings --features goblins-os-settings/native-desktop -- -D warnings`,
scoped `git diff --check`, `bash -n os/hardware-gate/verify-shipping-status.sh`,
and `goblins-os-verify --source-root .` → **blocked=0 (1575)**. CI/qemu render
and a live portal revoke/reload proof remain pending.

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

Current Keyboard continuation: shortcut rebinding and Caps Lock remap are now
source-gated but not shipped. Core aliases `/v1/keyboard/shortcuts/status`,
exposes `/v1/keyboard/shortcuts/binding` for allowlisted Goblins WM binding
set/reset, and exposes `/v1/keyboard/modifier-remap` for the reversible
Caps Lock→Control xkb option. The write path validates accelerator grammar,
refuses conflicts with other allowlisted Goblins bindings, edits only the
`ctrl:*`/`caps:*` xkb option token, and keeps the Settings editor disabled until
qemu proves the live gsettings round trip. Local source gates:
`cargo fmt --all --check`, `cargo clippy --workspace -- -D warnings`,
`cargo test --workspace`, the Rust 1.88 GTK container
`cargo clippy -p goblins-os-settings --features goblins-os-settings/native-desktop -- -D warnings`,
`git diff --check`, `bash -n os/hardware-gate/verify-shipping-status.sh`, and
`goblins-os-verify --source-root .` → **blocked=0 (1585)**.

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
language. No RPM, keybinding, QuickToggle, or live STT loop is claimed in this
pass. Local source gates: `cargo fmt --all --check`,
`cargo clippy --workspace -- -D warnings`, `cargo test --workspace`,
`goblins-os-verify --source-root .` → **blocked=0 (1602)**,
`node --check os/gnome-shell-extensions/goblins-captions@goblins.os/extension.js`,
`glib-compile-schemas --dry-run os/glib-schemas` in a Linux container,
`git diff --check`, `bash -n os/hardware-gate/verify-shipping-status.sh`, and
targeted `cargo test -p goblins-os-core live_captions -- --nocapture`.
CI/qemu still must prove the shell extension render, menu/shortcut control,
system-audio capture, transcription stream, and overlay behavior before Live
Captions can ship.

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
desktop file is claimed in this pass; the proposed `<Shift><Super>4` binding
collides with the shipped GNOME screenshot UI binding and needs CI/qemu proof
before enabling a replacement. Local source gates: `cargo fmt --all --check`,
`cargo clippy --workspace -- -D warnings`, `cargo test --workspace`,
`goblins-os-verify --source-root .` → **blocked=0 (1615)**,
`git diff --check`, targeted visual-lookup/AI/settings tests, and the Rust 1.88
GTK container
`cargo clippy -p goblins-os-visual-lookup --features goblins-os-visual-lookup/native-desktop -- -D warnings`.
CI/qemu still must prove the portal region capture, GTK card render,
launcher/menu entry, and final non-conflicting shortcut before Visual Look Up
can ship.

Current Today/Widgets continuation: the GTK Today panel surface is now
source-gated but not shipped. The new `goblins-os-today` crate reads
`/v1/today/status` over a loopback-only core URL, normalizes the widget layout,
renders local Date and Clock cards from real local values, and renders Weather,
Calendar, and Daily Brief as honest empty states until location services, a
calendar account, and a local model are actually available. The image build now
builds/copies the binary, the app has a desktop launcher, and
`20-goblins-os-today` seeds the default widget order. Verifier coverage pins the
binary, desktop launcher, dconf seed, native feature, core route fetch, shared UI
theming, and honest empty-state copy. Web verification found Fedora 44 has
`gtk4-layer-shell-devel`, but upstream documents GTK4 layer shell as unsupported
on GNOME Wayland, so this pass intentionally adds no new RPMs and does not claim
right-edge layer-shell anchoring. Local source gates: `cargo fmt --all --check`,
`cargo clippy --workspace -- -D warnings`, `cargo test --workspace`,
`goblins-os-verify --source-root .` → **blocked=0 (1631)**,
`git diff --check`, targeted `cargo test -p goblins-os-today`, and the Rust 1.88
GTK container
`cargo clippy -p goblins-os-today --features goblins-os-today/native-desktop -- -D warnings`.
CI/qemu still must prove the GTK render, GNOME Shell/menu-bar date entry,
edge-open behavior, and any future live weather/calendar/brief integrations
before Today/Widgets can ship.

Current Sound Recognition continuation: the Settings controls + write bridge are
now source-gated but not shipped. Core exposes
`/v1/sound-recognition/preference` and `/v1/sound-recognition/sound-toggle`,
writes only the allowlisted `org.goblins.SoundRecognition` keys, rejects unknown
sound ids, clamps confidence, and never reports listening just because a
preference saved. Settings ▸ Accessibility now shows model/listener/capture
readiness, the reliability caveat, the master toggle, per-sound toggles,
sensitivity, confidence, and alert options through those core routes. No RPM,
listener daemon, model weights, capture loop, notification integration, or live
mic behavior is claimed in this pass. Local source gates: `cargo fmt --all --check`,
`cargo clippy --workspace -- -D warnings`, `cargo test --workspace`,
`goblins-os-verify --source-root .` → **blocked=0 (1638)**,
`git diff --check`, targeted `cargo test -p goblins-os-core sound_recognition`,
targeted `cargo test -p goblins-os-settings sound_recognition`, and the Rust 1.88
GTK container
`cargo clippy -p goblins-os-settings --features goblins-os-settings/native-desktop -- -D warnings`.
CI/qemu must still prove the GTK render, installed schema/write behavior,
session-user listener, PipeWire capture, notification/flash path, and reliability
copy before Sound Recognition can ship.

Current Switch Control continuation: the preference bridge + Settings subsection
are now source-gated but not shipped. Core exposes
`/v1/accessibility/switch-control/preference`, writes only the allowlisted
`org.goblins.os.a11y.switch-control` keys, type-checks mode/scanning/timing
values, clamps the timing ranges, and returns honest saved-but-not-scanning copy
until the scanner engine is active. Settings ▸ Accessibility now renders the
Switch Control status, master toggle, scan mode/style, and timing controls
through that route. No GNOME Shell extension, AT-SPI tree walk, highlight ring,
crosshair, switch input, or synthetic selection is claimed in this pass. Local
source gates: `cargo fmt --all --check`,
`cargo clippy --workspace -- -D warnings`, `cargo test --workspace`,
`goblins-os-verify --source-root .` → **blocked=0 (1643)**,
`git diff --check`, targeted `cargo test -p goblins-os-core switch_control`,
targeted `cargo test -p goblins-os-settings switch_control`, and the Rust 1.88
GTK container
`cargo clippy -p goblins-os-settings --features goblins-os-settings/native-desktop -- -D warnings`.
CI/qemu must still prove the Settings render, installed schema/write behavior,
real scanner state machine, AT-SPI walk, overlay, and gated input injection
before Switch Control can ship.

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

**NEXT — pick up exactly here:**
1. **Batch 4 implementation pass (current direction — CI/qemu at the end):**
   continue the deferred engine UIs/overlays one feature at a time. The remaining
   high-risk engine work is Text Shortcuts/IBus. Use
   host-tested pure logic first, keep every live/render surface `in-progress`
   until CI/qemu proof is green, and do not add `whisper-cpp` as a CLI
   dependency until the actual Fedora 44 `whisper-cli` provider is proven.
2. **Deferred gated writes proof pass:** firewall CI image/render proof is green
   and the hardware-gate image/ISO path is past the export blocker; later
   push/dispatch the QMP-startup fix, inspect the display-backed VM logs if
   startup still fails, and inspect `firewall-live-toggle-proof.json` only if
   the session reaches the in-guest firewall toggle. That proof must show the
   live systemd/polkit oneshot success path for the firewall toggle. Only flip
   it to `shipped` if the render, live POST, and polkit oneshot path are green.
   Then prove the IME set, Focus, per-app permission revoke, multi-display
   apply, keyboard rebinding, and Voice Control interactions in CI/qemu. Do not
   flip any of these from `in-progress` until the write path and qemu interaction
   proof are green.
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
- [ ] **Deferred (risk-gated):** install the IBus CJK engines (`ibus-libpinyin`/`-anthy`/`-hangul`/`-gtk4`), the Containerfile IME-env relaxation, an **Add input source…** sheet that lists only installed engines, the menu-bar active-source indicator, and re-enabling `Super+Space` switching — the last reverses an intentional boot/launcher decision, so it lands deliberately, not blind.
- **Packages:** `ibus-libpinyin`, `ibus-anthy`, `ibus-hangul`, `ibus-gtk4`, `ibus-setup` (CJK engines verified fc44).
- **dconf/gsettings:** `org.gnome.desktop.input-sources` `sources` (`a(ss)`), `mru-sources`, `per-window`, `xkb-options`, `show-all-sources`; **revert** the `switch-input-source`/`-backward` emptying in `10-goblins-os-desktop:50-51` — bind `['<Super>space']` **only when >1 source**, and resolve `Super+Space` ownership with the launcher.
- **Files:** `os/bootc/Containerfile` (install 5 ibus packages; relax the IME-disabling env at lines 307-311 so `GTK_IM_MODULE`=ibus and IBus autostarts), `os/dconf/db/local.d/10-goblins-os-desktop`, `crates/goblins-os-core/src/input.rs` (`INPUT_SOURCES_SCHEMA` + `a(ss)` encode/decode, list/add/remove/reorder/set-current, `ibus list-engine` probe), `crates/goblins-os-core/src/main.rs` (extend existing `/v1/input/*` payloads), `crates/goblins-os-settings/src/main.rs` (real ordered-source list replacing the placeholder `input_source_summary_spec`), `os/gnome-shell-extensions/goblins-menubar@goblins.os/extension.js` (active-source abbreviation indicator when >1 source).
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

### `TODO` PDF / image Preview viewer
- [ ] Open any PDF/image as the default viewer (macOS Preview altitude — view, page, basic annotate; not a deep editor). The Goblins markup editor already covers screenshot annotation; this fills the "double-click a PDF" gap.
- **Approach:** themed_gnome_fallback (deep long tail — a stock GNOME viewer branded via `os/gtk-4.0/gtk.css`, not a custom build) for v1; a Goblins-native viewer is a later option.
- **Packages:** verify the fc44 name first — GNOME's document viewer was renamed `evince`→`papers` around F41; **confirm `papers` vs `evince` against the fc44 repo before adding** (wrong name breaks the image build). Image viewer: `loupe` (GNOME Image Viewer, fc44).
- **Files:** `os/bootc/Containerfile` (package + `rpm -q`), default-application dconf / mimeapps so PDFs/images open in it, `os/gtk-4.0/gtk.css` (already brands stock GTK apps — confirm coverage).
- **Goblins-grade:** branded via the gtk.css bridge (window/headerbar/sidebar/accent in Goblins tokens). Honest gating: n/a (a viewer is always present once packaged).
- **Verifiable:** CI/qemu only (package + render). **Effort:** S · **Risk:** LOW once the package name is confirmed.
- _Note: spec agent connection-failed; package name + mimeapps wiring must be web-verified before building._

### `TODO` Fingerprint unlock (Touch ID analogue)
- [ ] Enroll a fingerprint and unlock the session / authorize sudo with it (laptop readers). Secure-Enclave parity is HW-bound; generic `fprintd` reader support is the achievable, real win.
- **Approach:** custom_surface (a Goblins "Fingerprint" enrollment flow in Settings ▸ Security on the `fprintd` D-Bus) + config (PAM via `authselect`).
- **Packages:** `fprintd`, `fprintd-pam`, `libfprint` (verify fc44).
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
- [ ] Optional later: the magnifier zoom-factor + lens-mode sliders (non-boolean controls).
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
- [ ] **Gated start/stop (deferred):** turn the radio into a WPA2/WPA3 AP sharing the uplink — on/off switch, editable SSID + password, live connected-devices readout via NetworkManager shared-mode, gated by the Wi-Fi-connect policy profile; needs `dnsmasq` in the image (`ipv4.method shared`).
- **Packages:** `dnsmasq` (verified `2.92rel2-9.fc44`; **mandatory** — `ipv4.method shared` needs it for DHCP/NAT, not pulled by NetworkManager-wifi).
- **Files:** `crates/goblins-os-core/src/hotspot.rs` (NEW — nmcli status/start/stop, SSID + password validation, uplink/single-radio gating, PSK error sanitization, tests), `crates/goblins-os-core/src/main.rs` (`mod hotspot` + `/v1/network/hotspot/status`, `/v1/network/hotspot`), `crates/goblins-os-settings/src/main.rs` (`append_hotspot_management` in `build_network`, modeled on `append_bluetooth_power_control`; `HotspotStatus` + `set_hotspot`), `os/bootc/Containerfile` (`dnsmasq`).
- **APIs:** `nmcli` AP profile (`802-11-wireless.mode ap`, `band bg`, `ipv4.method shared`, `wifi-sec.key-mgmt wpa-psk`/`sae`, `wifi-sec.psk`), reusing `network.rs` `split_terse` + `policy_state_for_control("settings-control")`; GTK4 `Switch`/`Entry`/`PasswordEntry`.
- **Goblins-grade:** "Personal Hotspot" subsection; prominent switch-row whose copy flips by state; an inset card with Network name + Password rows (disabled while live; edits apply on next enable, matching macOS); when ON, `health_row` status pills for client count / SSID / shared uplink. Copy: "Passwords are used once to configure the hotspot and are never shown here."
- **Honest gating (4 gates):** nmcli missing → "Networking is not ready in this session…"; no AP-capable adapter → "This device has no Wi-Fi adapter that can broadcast a hotspot"; **the macOS-parity gate** — Wi-Fi is the only uplink on a single radio → disabled "Connect to the internet over Ethernet to share it over Wi-Fi"; policy denies `settings-control` → 403. Password `<8` rejected pre-nmcli; SSID `-`-prefix rejected, length-capped 32; connect errors sanitized so the PSK never leaks.
- **Verifiable:** host — SSID/password validation, PSK-leak sanitization, single-radio/uplink decision, terse parsing. CI/qemu — panel render + live AP (needs a virtual/passed-through Wi-Fi device).
- **Effort:** M · **Risk:** MED. Route writes through policy (no ungated path); start/stop idempotent (fixed con-name "Goblins Hotspot"); never persist the PSK.

### `shipped` Hot Corners
- [x] **Opt-in hot corners shipped** (`goblins-wm@goblins.os`): four `hot-corner-{top,bottom}-{left,right}` gschema keys (`s`, choices `none`/`mission-control`/`app-expose`, **default `none`** so nothing changes until opted in — macOS-style). Each enabled corner gets a tiny reactive actor (`addChrome`) that triggers the action on pointer entry, rebuilt on settings change, fully torn down on disable. Verified with `node --check`, `glib-compile-schemas`, verify gates, and CI/qemu desktop artifacts from build run `28287964440`: `52c-wm-hot-corner-{light,dark}.png` on both `x86_64` and `aarch64`.
- [ ] Optional polish: more corner actions (Show Desktop, Control/Notification Center, Lock), a modifier-key guard, and the Settings ▸ Desktop chooser UI; set `org.gnome.desktop.interface enable-hot-corners=false` in dconf if GNOME's built-in corner ever conflicts.
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
- [ ] **First-boot page + copy job (deferred, CI/qemu):** the installer "Bring your stuff over" branch, source-drive scan (reuse the `install_targets` sysfs scan), read-only `udisksctl` mount, `rsync --info=progress2` copy with the Copied/Skipped ledger, and the allowlisted dconf→gsettings preference import — plus adding `ntfs-3g`/`exfatprogs`/`udisks2` to the Containerfile (web-verify the exact fc44 names + add `rpm -q` checks first).
- **Packages:** `ntfs-3g` (`2026.2.25-1.fc44`), `exfatprogs` (`1.4.2-2.fc44`), `udisks2` (`2.11.1-2.fc44`) — verified.
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
- [ ] **Surfaces + timer + per-app breakthroughs (deferred):** mode/schedule CRUD, per-app breakthrough via the `notifications.rs` helper, the `SettingsPanel::Focus` editor + Control-Center tile + menu-bar indicator, and the `OnCalendar=minutely` user timer → `POST /v1/focus/tick`. (Drops iCloud/location/Smart Activation — absent, never stubbed.)
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
- [ ] **Recordable UI + live round trip (deferred):** make rows recordable, add a Caps Lock dropdown, inline conflict notice, per-row/global reset, and qemu gsettings round-trip proof.
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
- [ ] **Portal write proof + resource mappings (deferred):** CI/qemu render plus live revoke/reload proof, and `Lookup`/metadata mapping for camera/microphone resource-keyed grants before any device revoke UI.
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
- [ ] **Live capture/transcribe/menu proof (deferred, L):** implement/prove the privileged capture loop, real transcription stream, menu-bar toggle or non-conflicting shortcut, and rendered overlay behavior in CI/qemu. The feature remains `in-progress`.
- **Packages:** `whisper-cpp`/`whisper-cpp-devel` exist in Fedora 44 as `1.8.1-2.fc44`, but the current repoquery proof did **not** find a `whisper-cli` binary provider; do not add an RPM or `command -v whisper-cli` gate until the CLI provider is proven. **Do NOT** depend on `whisper-stream` (SDL2, often unpackaged, mic-via-SDL — wrong tool).
- **gsettings/dconf:** NEW `org.goblins.shell.extensions.captions` (enabled, toggle-captions `['<Super><Alt>c']`, source system|microphone|both, auto-hide, keep-onscreen, text-size, position) + a `30-captions` seed shipping installed-but-off (`enabled=false`).
- **Files:** `os/gnome-shell-extensions/goblins-captions@goblins.os/{metadata.json,extension.js,stylesheet.css,schemas/…captions.gschema.xml}` (NEW — overlay St actor + menu-bar QuickToggle + capture/transcribe driver), `os/dconf/db/local.d/30-captions`, `os/bootc/Containerfile` (`whisper-cpp` + `command -v whisper-cli`), `crates/goblins-os-core/src/captions.rs` (NEW — the **privileged** pw-record monitor-capture + whisper-cli loop + `/v1/captions/*`, mirroring `voice.rs`), `crates/goblins-os-core/src/main.rs` (`/v1/captions/status` + an SSE caption route), `crates/goblins-os-core/src/accelerators.rs` (allowlist the toggle key), `crates/goblins-os-settings/src/main.rs` (Accessibility "Live Captions" row), `os/gnome-shell-modes/goblins-os.json` (enable the uuid).
- **APIs:** `Main.layoutManager.addChrome` + St.BoxLayout/Label (the exact goblins-wm overlay idiom); QuickSettings SystemIndicator/QuickToggle; core **HTTP+SSE** stream so the privileged capture stays in core, not the shell; core: `pw-record` on the default-sink **monitor**, VAD/RMS segment, `whisper-cli -m <model> -otxt`; `wpctl`/`pw-cli` to resolve the monitor id.
- **Goblins-grade:** a glass caption capsule (`@gos_material_thick` + border + shadow, pill/HUD radius); text `GOS_TYPE_BODY`/`CALLOUT`/`TITLE_3` by size, **Inter**; newest line full-ink, prior line dims one tier (macOS settle); leading status dot (`gos_system_green` transcribing / neutral idle / `gos_system_orange` warming); opacity+rise arrival with the reduced-motion clean cut; positioned via work-area insets so it never collides with the dock.
- **Honest gating:** `/v1/captions/status` reports capture/model/runtime/pipewire like `voice_status`. Model absent → Settings "Add a speech model to turn on Live Captions" + toggle greyed with the reason on hover (never a dead toggle); no monitor source → "No system audio to caption" (not a blank box). Capture+STT fully local — stated in the subtitle.
- **Verifiable:** host — VAD/segment chunker, capability struct, whisper-cli argv builder, `/v1/captions/status` JSON; gschema `--dry-run`. CI/qemu — extension.js, live monitor capture, real transcription, the rendered overlay (light+dark).
- **Effort:** L · **Risk:** MED. Keep capture in the core service (runs as the service user); expose only a read-only stream to the shell. Chunk-on-silence adds 0.5-2s lag — small `base.en` default + in-progress dim line + VAD tuning; the UI says it's an accessibility aid. Ships disabled, not in the login path.

### `in-progress` Switch Control (scanning input for adaptive switches)
- [x] **Status + schema substrate shipped** (`crates/goblins-os-core/src/switch_control.rs` + `/v1/accessibility/switch-control/status`, NEW `org.goblins.os.a11y.switch-control` gschema via the existing `os/glib-schemas/` plumbing — off by default): reads enabled/mode/scanning/timings with the same normalization the engine will trust (`normalize_mode`/`normalize_scanning`/`clamp_interval` 300–5000 / `clamp_ms`), honest-gated when the schema is absent. Pure normalizers unit-tested (193 core tests); `glib-compile-schemas` clean; clippy/fmt clean; route + schema verify gates.
- [x] **Preference bridge + Settings subsection source-gated (CI/qemu-pending):** core exposes `/v1/accessibility/switch-control/preference`, writes only the allowlisted `org.goblins.os.a11y.switch-control` keys, validates mode/scanning, clamps timing values, and returns honest saved-but-not-scanning copy until the scanner engine is active. Settings ▸ Accessibility renders status, master toggle, mode/style choices, and timing sliders through that route. No Shell extension, AT-SPI walk, highlight overlay, switch input, or selection injection is claimed yet.
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
- [x] **Today panel surface source-gated (CI/qemu-pending):** the `goblins-os-today` GTK crate reads `/v1/today/status`, renders local Date/Clock cards with real local values, and renders Weather/Calendar/Daily Brief as honest empty states until location services, a calendar account, and a local model are actually available. The app uses shared Goblins UI theming, has a desktop launcher, a dconf seed for the default widget order, and is copied into the image. Web verification found `gtk4-layer-shell-devel` in Fedora 44, but upstream documents GTK4 layer shell is unsupported on GNOME Wayland; this source-gated pass therefore does **not** add layer-shell packages or claim right-edge shell anchoring. Menu-bar date button, edge-swipe, live weather/calendar/brief data, and render proof remain CI/qemu-pending.
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
- [ ] **Live IBus engine + session enablement (deferred, XL/highest-risk):** the `goblins-textshortcuts` IBus engine loop (preedit/commit over `text-input-v3`, pass-through by default, never in password fields), the dconf input-source seed, accept bubble, and the optional model-gated autocorrect tier.
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

### `TODO` FileVault-style full-disk encryption at install
- [ ] LUKS2 root bound to **TPM2 for auto-unlock**, with a **mandatory escrowed recovery key** — a first-class "Encrypt this disk" choice in the Goblins installer + a read-only Encryption posture row in Settings ▸ Security. Encrypt by default with transparent TPM boot, but **never** without a captured recovery key, and fall back to a recovery-key prompt whenever the TPM measurement changes (matching FileVault: hardware auto-unlock is convenience over an always-present credential).
- **Packages:** `systemd-cryptsetup`, `cryptsetup`, `tpm2-tss` (add + `rpm -q` explicitly for the initramfs unlock path; `systemd-cryptenroll` ships with systemd). `clevis` NOT needed.
- **gsettings/dconf:** none — it's a one-time install-engine decision, **not** a runtime toggle. Settings surfaces read-only live status via a new `/v1/security/encryption` (shells `cryptsetup status` + `systemd-cryptenroll --list`).
- **Files:** `crates/goblins-os-core/src/install_targets.rs` (accept `tpm2-luks`; build `--block-setup tpm2-luks`; tpm-absent→key-only degradation; recovery-key-required gate), `crates/goblins-os-installer/src/main.rs` (the encryption card + the mandatory recovery-key step), `crates/goblins-os-settings/src/main.rs` (Encryption posture row in `build_security`), `crates/goblins-os-verify/src/main.rs` (**REWRITE** the gate strings that currently pin the opposite reject-contract — `install-simple-api-routes-tpm2-luks-to-full-storage` / `install-policy-tpm2-luks-guidance` / `install-simple-api-direct-block-only-contract`), `os/bootc/Containerfile`, `os/iso/verify-install.ks` + `verify-install-dark.ks`, `crates/goblins-os-design/src/lib.rs`.
- **APIs:** `bootc install to-disk --block-setup tpm2-luks --filesystem xfs --wipe <dev>` (the documented LUKS-on-TPM2 path); `systemd-cryptenroll --tpm2-device=auto --tpm2-pcrs=7` for auto-unlock + `--recovery-key` for escrow; `cryptsetup luksDump`/`status` for read posture; `/etc/crypttab tpm2-device=auto,tpm2-pcrs=…`; **Plymouth** (existing goblins-os theme) for the branded recovery-key fallback prompt.
- **Goblins-grade:** an installer "Encryption" inset card right after disk selection / before the destructive-ack: "Encrypt this disk (recommended)" pre-selected + "Don't encrypt"; then a **mandatory** Recovery Key step mirroring FileVault — a monospace 24-char (8×3) copyable key, an "I've saved my recovery key" checkbox that **gates Continue**, "Goblins OS cannot recover your data without this key"; Security pill neutral "encrypted · TPM auto-unlock" vs amber "encrypted · key-only"; Inter + the mono ramp; brand the boot-time unlock via the Plymouth theme.
- **Honest gating:** TPM auto-unlock attempted only when a TPM device is present AND Secure Boot state is readable (reuse `SecureBootStatus` + a new tpm probe) — no TPM → drop to recovery-key/passphrase-only and **say so** ("This computer has no TPM, so you'll enter your recovery key at every boot"); the recovery key is **minted before any TPM binding** (closes bootc #421/#477 — bare tpm2-luks ships with no fallback and is unbootable when PCRs change); **PCR policy pinned to PCR7 only** to survive ostree updates (warn that firmware/Secure-Boot changes re-prompt once, per bootc #561); TPM enroll fails post-format → install still **succeeds** as key-only, Security reports "encrypted · recovery-key only."
- **Verifiable:** host — extend `simple_install_block_setup`/`simple_install_filesystem` to assert tpm2-luks accepted, the command vector contains `--block-setup tpm2-luks`, the tpm-absent degradation, the recovery-key gate; the new endpoint's luksDump/cryptenroll parse. CI/qemu — installer card + Security row render, real bootc tpm2-luks install, real cryptenroll, **PCR7 auto-unlock across a reboot**, the Plymouth recovery-key fallback (the qemu kickstarts + the hardware gate).
- **Effort:** L · **Risk:** BOOT-CRITICAL. The recovery-key escrow **is** the de-risk (bare `tpm2-luks` is a known unbootable break). Avoid PCR over-binding (PCR7 only — binding 0/4/11 breaks on every ostree update). Keep `direct` as the still-offered "Don't encrypt" path; never auto-enable without the captured-key gate; keep the destructive-ack + `GOBLINS_OS_ENABLE_DESTRUCTIVE_INSTALL` env gate exactly as-is.

### `TODO` btrfs `/home` + local snapshots + restore UI (Time Machine analogue)
- [ ] Automatic local snapshots + an honest "last snapshot" status surface + a timestamped restore browser that recovers files from a chosen snapshot — never silently mutating the live system, always explicit and reversible (default side-by-side, no in-place rollback from the GUI).
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
