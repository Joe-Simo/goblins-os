# Goblins OS — Go-Live & Feature Backlog

> Living checklist. Two parts: (1) what it takes to make Goblins OS **fully live**
> as a free, downloadable, auto-updating distro, and (2) the **feature backlog** —
> what's shipped vs. still open. Companion to `SHIP.md` (gates) and
> `os/release/acquisition-readiness-delta.toml` (release-evidence tracking).

## Part 1 — Make it fully live (free distro, auto-updating)

**Owner one-time actions** (only you can do these):
- [ ] Make the **GHCR package public** — GitHub → Packages → `goblins-os` → visibility → Public. (Without this, users can't pull updates.)
- [ ] **IP-counsel review** of `LICENSE` (AGPL-3.0), `CLA.md`, `TRADEMARKS.md` before commercial reliance / first outside contribution.
- [ ] (Optional) **Register the "Goblins OS" trademark** — strengthens the brand asset independent of the code license.
- [ ] Pick a **release cadence** (e.g. monthly) and kick the first one.

**Code / CI work** (I can do these):
- [ ] Wire **ISO publishing to a GitHub Release** in `release.yml` so there's a public download link (today the ISO is only a workflow artifact).
- [ ] Add a **`:stable` / `:testing` channel** so you can ship to testers, then promote to everyone — and point the installer ISO at `:stable`.
- [ ] (Optional, for true hands-off security) **Reorder `release.yml`** to gate *before* publish (it currently pushes to GHCR, then runs verify/SBOM), so a scheduled run can auto-ship safely.

**Verification before announcing** (CI / qemu):
- [ ] Run the **full hardware gate** (real-GPU screenshots + close-signoff) per `SHIP.md`.
- [ ] Confirm a **fresh install → auto-update → rollback** cycle end-to-end in qemu.
- [ ] First **`release.yml`** run: dual-arch image to GHCR + installer ISO + SBOM/secret-scan.

**Then:**
- [ ] Publish the download page / announce.

## Part 2 — Feature backlog (vs. macOS)

We did **not** implement every macOS feature — and shouldn't (some are deliberate
non-goals; some need Apple hardware). Status of the *achievable* set:

**✅ Shipped**
- Design parity pass (type + control ramps, materials, states, chrome).
- File search in the launcher; GSConnect phone bridge; backup tooling (deja-dup +
  snapper); dictation + Orca screen reader; Quick Look; Font Book; Show Desktop.
- License → AGPL + CLA + trademark; weekly upstream-drift gate; bootc auto-update.

**🔲 Open — worth doing (own surfaces / stable seams)**
- Branded **Accessibility / Firewall / Privacy / Personal-Hotspot Settings rows**
  (GTK — CI/qemu-verified, not blind-edited).
- **Hot Corners**; **Live Text / OCR** on screenshots; **Migration Assistant**
  (import a home dir); **fingerprint unlock** (fprintd); **named Focus modes** +
  scheduling; **btrfs `/home` snapshots + restore UI** (deja-dup covers file backup
  today); **Snap Assist** 2nd-step chooser; **App Exposé**; **multi-display config**.

**🔲 Open — larger / their own effort**
- **Voice Control**, **Live Captions**, **Switch Control**, **Sound Recognition**
  (a11y engines on the on-device model); **Desktop Widgets / Today view**;
  **autocorrect / text replacement** (IBus); **Visual Look Up**; **per-display Spaces**;
  **IME / CJK**; **FileVault-at-install**; **keychain UI**; **Preview / PDF viewer**.

**🧱 Deliberately NOT us** (per the thesis / hardware)
- App store, bundled productivity apps, Shortcuts/Automator (→ the AI-build thesis).
- Handoff, AirDrop, Sidecar, iPhone Mirroring, Find My, Passkeys, Force Touch
  (need Apple hardware/cloud — cross-vendor equivalents noted in the audit).
- **Builder-built** (ship the primitive, not the app): widgets, autosave/versioning,
  OCR surfaces, usage dashboards, the classic utilities.

**Did we implement all?** No — we shipped the highest-leverage batch plus all the
foundational/business work (design parity, license, distribution, update-safety).
A real but mostly MED/LOW feature tail remains; most of it is "own a small surface
on stable APIs," "leave to builders," or "GNOME fallback" — none of it requires
forking or owning security/hardware.
