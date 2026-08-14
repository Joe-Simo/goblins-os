#!/usr/bin/env bash
set -euo pipefail

ROOT="${ROOT:-$(cd "$(dirname "$0")/../.." && pwd)}"
cd "$ROOT"
. "$ROOT/os/hardware-gate/secret-scan.sh"
. "$ROOT/os/hardware-gate/rpm-sbom-arch.sh"
. "$ROOT/os/iso/manifest-provenance.sh"
. "$ROOT/os/hardware-gate/release-evidence.sh"

SHIP_DECL="SHIP.md"
WORKFLOW=".github/workflows/build.yml"
SCREENSHOT_ROOT="os/screenshots/hardware-gate"
SIGNOFF="${GOBLINS_OS_SIGNOFF_NOTES:-os/signoff-notes.md}"
if [ -n "${GOBLINS_OS_SIGNOFF_NOTES:-}" ]; then
  if [ "${GOBLINS_OS_SIGNOFF_STAGING_VALIDATE:-0}" != "1" ]; then
    echo "[FAIL] Alternate signoff notes are permitted only for atomic composition staging"
    exit 2
  fi
  case "$SIGNOFF" in
    "$ROOT/os/signoff-proofs/.signoff-notes-stage."*)
      STAGED_SIGNOFF_BASENAME="${SIGNOFF#"$ROOT/os/signoff-proofs/"}"
      [[ "$STAGED_SIGNOFF_BASENAME" != */* ]] || {
        echo "[FAIL] Staged signoff notes path must not contain nested components"
        exit 2
      }
      ;;
    *)
      echo "[FAIL] Staged signoff notes must be a private file under os/signoff-proofs"
      exit 2
      ;;
  esac
  if [ ! -f "$SIGNOFF" ] || [ -L "$SIGNOFF" ]; then
    echo "[FAIL] Staged signoff notes must be a regular non-symlink file"
    exit 2
  fi
fi
RUNBOOK="os/hardware-gate/runbook.md"
SCREENSHOT_RUN_DIR="${SCREENSHOT_RUN_DIR:-${SCREENSHOT_DIR:-}}"
FAIL_COUNT=0
ARCHES=(aarch64)
EXPECTED_BIB_IMAGE="quay.io/centos-bootc/bootc-image-builder@sha256:2b52843ea2bfda73b0a08d97e76b734393b1d3a804681b9fabb26723bd3a2f0b"
CORE_SERVICE_READ_WRITE_PATHS="/run/goblins-os-core /var/lib/goblins-os/installer /var/lib/goblins-os/session /var/lib/goblins-os/policy /var/lib/goblins-os/ai /var/lib/goblins-os/models /var/lib/goblins-os/voice/work /var/lib/goblins-os/secrets/openai /var/lib/goblins-os/apps /var/lib/goblins-os/codex"
SELECTED_CANDIDATE_COMMIT="${GOBLINS_OS_CANDIDATE_COMMIT:-${GITHUB_SHA:-}}"
CANDIDATE_SELECTION_VALID=1
if [[ ! "$SELECTED_CANDIDATE_COMMIT" =~ ^[0-9a-fA-F]{40}$ ]]; then
  CANDIDATE_SELECTION_VALID=0
  SELECTED_CANDIDATE_COMMIT=""
else
  SELECTED_CANDIDATE_COMMIT="$(printf '%s' "$SELECTED_CANDIDATE_COMMIT" | tr '[:upper:]' '[:lower:]')"
fi

if ! git rev-parse --is-inside-work-tree >/dev/null 2>&1; then
  echo "[FAIL] Final shipping verification requires the exact candidate Git checkout"
  FAIL_COUNT=$((FAIL_COUNT + 1))
  CANDIDATE_SELECTION_VALID=0
else
  SOURCE_HEAD="$(git rev-parse HEAD | tr '[:upper:]' '[:lower:]')"
  if [ "$CANDIDATE_SELECTION_VALID" -ne 1 ] || [ "$SOURCE_HEAD" != "$SELECTED_CANDIDATE_COMMIT" ]; then
    echo "[FAIL] Final shipping checkout HEAD $SOURCE_HEAD does not match selected candidate ${SELECTED_CANDIDATE_COMMIT:-missing}"
    FAIL_COUNT=$((FAIL_COUNT + 1))
    CANDIDATE_SELECTION_VALID=0
  else
    echo "[PASS] Final shipping checkout HEAD matches selected candidate $SELECTED_CANDIDATE_COMMIT"
  fi
  UNEXPECTED_SOURCE_CHANGES="$({
    git -c core.quotepath=false diff --name-only --no-ext-diff
    git -c core.quotepath=false diff --cached --name-only --no-ext-diff
    git -c core.quotepath=false ls-files --others --exclude-standard
  } | sed '/^$/d' | sort -u | grep -Ev '^os/(iso/output/|signoff-proofs/|screenshots/hardware-gate/)|^os/signoff-notes[.]md$' || true)"
  if [ -n "$UNEXPECTED_SOURCE_CHANGES" ]; then
    echo "[FAIL] Final shipping checkout has changes outside generated proof paths:"
    printf '%s\n' "$UNEXPECTED_SOURCE_CHANGES"
    FAIL_COUNT=$((FAIL_COUNT + 1))
    CANDIDATE_SELECTION_VALID=0
  else
    echo "[PASS] Final shipping checkout changes are confined to generated proof paths"
  fi
fi

REQ_SCREENSHOTS=(
  "01-installer.png"
  "02-install-network.png"
  "03-login.png"
  "04-desktop.png"
  "05-first-boot-private-unlock.png"
  "06-onboarding.png"
  "07-home.png"
  "08-shell-home.png"
  "09-shell-dark.png"
  "10-settings.png"
  "11-settings-models.png"
  "12-settings-dark.png"
  "13-studio-before.png"
  "14-studio-running.png"
  "15-studio-app-detail.png"
  "16-built-app-open.png"
  "17-dark-motion.png"
  "18-light-motion.png"
  "19-vulkan-vkcube.png"
  "20-gamemode-active.png"
  "21-gamescope-session.png"
  "22-mangohud-overlay.png"
  "23-controller-detection.png"
  "24-audio-output.png"
  "25-install-destination.png"
  "26-install-storage-summary.png"
  "27-dual-boot-preserve-existing-os.png"
  "28-bootloader-efi-summary.png"
  "29-preview-pdf-open.png"
  "30-preview-image-open.png"
  "31-text-shortcuts-candidate-bubble-render.png"
  "32-text-shortcuts-live-ibus-runtime-render.png"
  "33-accessibility-text-scaling.png"
  "34-accessibility-high-contrast.png"
  "35-accessibility-reduced-transparency.png"
  "36-accessibility-reduced-motion.png"
  "37-accessibility-localization-expansion.png"
  "38-accessibility-orca-atspi.png"
  "39-accessibility-keyboard-focus.png"
  "40-accessibility-window-resize.png"
  "41-hosted-context-review.png"
  "42-hosted-context-review-dark.png"
)
FIREWALL_LIVE_TOGGLE_PROOF="firewall-live-toggle-proof.json"
TEXT_SHORTCUTS_SESSION_ENABLE_PROOF="text-shortcuts-session-enable-proof.json"
TEXT_SHORTCUTS_CANDIDATE_METADATA_PROOF="text-shortcuts-candidate-metadata-proof.json"
TEXT_SHORTCUTS_OVERLAY_INTENT_PROOF="text-shortcuts-overlay-intent-proof.json"
TEXT_SHORTCUTS_CANDIDATE_BUBBLE_FRAME_PROOF="text-shortcuts-candidate-bubble-frame-proof.json"
TEXT_SHORTCUTS_CANDIDATE_BUBBLE_LAYOUT_PROOF="text-shortcuts-candidate-bubble-layout-proof.json"
TEXT_SHORTCUTS_CANDIDATE_BUBBLE_RENDER_INTENT_PROOF="text-shortcuts-candidate-bubble-render-intent-proof.json"
TEXT_SHORTCUTS_CANDIDATE_BUBBLE_RENDER_PROOF="text-shortcuts-candidate-bubble-render-proof.json"
TEXT_SHORTCUTS_LIVE_IBUS_RUNTIME_RENDER_PROOF="text-shortcuts-live-ibus-runtime-render-proof.json"
KEYBOARD_SHORTCUTS_ROUNDTRIP_PROOF="keyboard-shortcuts-roundtrip-proof.json"
INPUT_SOURCES_ROUNDTRIP_PROOF="input-sources-roundtrip-proof.json"
MULTI_DISPLAY_APPLY_PROOF="multi-display-apply-proof.json"
FOCUS_ARM_ROUNDTRIP_PROOF="focus-arm-roundtrip-proof.json"
APP_PRIVACY_REVOKE_PROOF="app-privacy-revoke-proof.json"
PREVIEW_OPEN_RENDER_PROOF="preview-open-render-proof.json"
AUDIO_OUTPUT_PROOF="audio-output-proof.json"
RUNTIME_BUILD_PROOF="runtime-build-proof.json"
ACCESSIBILITY_ADAPTIVITY_PROOF="accessibility-adaptivity-proof.json"

check() {
  local label="$1"
  local test_cmd="$2"
  if eval "$test_cmd"; then
    echo "[PASS] $label"
  else
    echo "[FAIL] $label"
    FAIL_COUNT=$((FAIL_COUNT + 1))
  fi
}

architecture_is_canonical() {
  [ "${1:-}" = "aarch64" ]
}

iso_builder_preflight_lifecycle_is_ordered() {
  awk '
    index($0, "if ! docker start -a \"$preflight_container_id\" >/dev/null; then") {
      starts += 1
      start_line = NR
    }
    /^[[:space:]]*network_count=.*preflight_name/ {
      inspections += 1
      inspect_line = NR
    }
    END {
      exit !(starts == 1 && inspections == 1 && start_line < inspect_line)
    }
  ' os/iso/build-iso.sh
}

proof_json_passes() {
  local proof="$1"
  local schema="${2:-$(basename "$proof" -proof.json)}"

  python3 "$ROOT/os/hardware-gate/capture-harness/proof_validation.py" \
    --proof "$schema" "$proof"
}

accessibility_adaptivity_proof_passes() {
  local proof="$1"
  local run_dir="$(dirname "$proof")"

  proof_json_passes "$proof" accessibility-adaptivity \
    && python3 "$ROOT/os/hardware-gate/capture-harness/proof_validation.py" \
      --proof-screenshots accessibility-adaptivity "$proof" "$run_dir"
}

evidence_bundle_digest() {
  local run_dir="$1"
  local arch="$2"
  local image_ref="$3"
  local run_date="${run_dir%/}"
  run_date="${run_date##*/}"

  architecture_is_canonical "$arch" || return 1

  python3 "$ROOT/os/hardware-gate/capture-harness/evidence_bundle.py" verify \
    --repository "$ROOT" \
    --run-dir "$run_dir" \
    --architecture "$arch" \
    --candidate-commit "$SELECTED_CANDIDATE_COMMIT" \
    --image-ref "$image_ref" \
    --run-date "$run_date"
}

aarch64_local_display_attestation_fields() {
  local run_dir="$1"
  local image_ref="$2"
  local run_date="${run_dir%/}"
  run_date="${run_date##*/}"

  python3 "$ROOT/os/hardware-gate/capture-harness/evidence_bundle.py" \
    verify-attestation \
    --seal "$run_dir/evidence-bundle.json" \
    --record "$run_dir/aarch64-local-display-attestation.json" \
    --signature "$run_dir/aarch64-local-display-attestation.json.cms" \
    --certificate "$ROOT/os/release/display-proof-authority2.pem" \
    --certificate-sha256 "$ROOT/os/release/display-proof-authority2.sha256" \
    --ca-certificate "$ROOT/os/release/display-proof-authority2-ca.pem" \
    --ca-certificate-sha256 "$ROOT/os/release/display-proof-authority2-ca.sha256" \
    --candidate-commit "$SELECTED_CANDIDATE_COMMIT" \
    --image-ref "$image_ref" \
    --run-date "$run_date"
}

release_workflow_action_pins_are_reviewed() {
  cargo run --locked --quiet --release -p goblins-os-verify -- \
    --workflow-action-pins "$ROOT" --quiet
}

release_workflow_deprecated_action_pins_are_absent() {
  ! rg -q \
    '34e114876b0b11c390a56381ad16ebd13914f8d5|ea165f8d65b6e75b540449e92b4886f43607fa02|d3f86a106a0bac45b974a628896c90dbdf5c8093|8d2750c68a42422c14e847fe6c8ac0403b4cbd6f' \
    .github/workflows -g '*.yml' -g '*.yaml'
}

core_service_writable_paths_are_exact() {
  local unit=os/systemd/goblins-os-core.service
  [ "$(grep -c '^ProtectSystem=' "$unit" || true)" = 1 ] \
    && grep -Fxq 'ProtectSystem=strict' "$unit" \
    && [ "$(grep -c '^ReadWritePaths=' "$unit" || true)" = 1 ] \
    && grep -Fxq "ReadWritePaths=$CORE_SERVICE_READ_WRITE_PATHS" "$unit"
}

hardware_core_proof_unit_is_narrowly_sandboxed() {
  local config=os/iso/verify-config.toml
  local unit
  unit="$(
    sed -n \
      "/cat > \/etc\/systemd\/system\/goblins-hwgate-core-proof@.service <<'EOF'/,/^EOF$/p" \
      "$config"
  )" || return 1

  [ "$(grep -c '^ProtectSystem=' <<<"$unit" || true)" = 1 ] \
    && grep -Fxq 'ProtectSystem=strict' <<<"$unit" \
    && [ "$(grep -c '^ProtectHome=' <<<"$unit" || true)" = 1 ] \
    && grep -Fxq 'ProtectHome=tmpfs' <<<"$unit" \
    && [ "$(grep -c '^BindReadOnlyPaths=' <<<"$unit" || true)" = 1 ] \
    && grep -Fxq 'BindReadOnlyPaths=-/var/home/goblin/.config/goblins-os' <<<"$unit" \
    && [ "$(grep -c '^ReadWritePaths=' <<<"$unit" || true)" = 1 ] \
    && grep -Fxq 'ReadWritePaths=/run/goblins-hwgate-core-proof /run/goblins-hwgate-fixture-state /run/goblins-hwgate-fixture-block' <<<"$unit" \
    && rg -Fq 'FIXTURE_RESIDENT_SOCKET=$FIXTURE_STATE/resident/resident.sock' os/hardware-gate/capture-harness/core-proof-operation.sh \
    && ! rg -Fq '/run/goblins-os/resident.sock' os/hardware-gate/capture-harness/core-proof-operation.sh \
    && rg -Fq 'Environment=GOBLINS_OS_RESIDENT_SOCKET=/run/goblins-hwgate-fixture-state/resident/resident.sock' "$config" \
    && rg -Fq 'ExecStopPost=-+/etc/goblins-os/hardware-gate/goblins-hwgate-core-proof-operation fixture-core-stopped' "$config" \
    && rg -Fq 'find "$FIXTURE_STATE" -mindepth 1 -delete' os/hardware-gate/capture-harness/core-proof-operation.sh \
    && rg -Fq 'find "$FIXTURE_BLOCK" -mindepth 1 -delete' os/hardware-gate/capture-harness/core-proof-operation.sh
}

firstboot_production_core_unit_proof_is_pinned() {
  local script=os/hardware-gate/capture-harness/firstboot-unlock.sh
  local needle
  for needle in \
    'prove_production_core_unit' \
    'CORE_UNIT_FRAGMENT=/usr/lib/systemd/system/goblins-os-core.service' \
    '--property=ActiveState' \
    '--property=SubState' \
    '--property=MainPID' \
    '--property=FragmentPath' \
    '--property=DropInPaths' \
    '--property=ProtectSystem' \
    '--property=ReadWritePaths' \
    "CORE_READ_WRITE_PATHS=\"$CORE_SERVICE_READ_WRITE_PATHS\"" \
    '[ "$active" = active ]' \
    '[ "$substate" = running ]' \
    '[ "$main_pid" -gt 1 ]' \
    '[ "$fragment" = "$CORE_UNIT_FRAGMENT" ]' \
    'CORE_TRUSTED_DROPIN=/usr/lib/systemd/system/service.d/10-timeout-abort.conf' \
    'CORE_TRUSTED_DROPIN_SHA256=ae6b234f92bc22f1201a7572b59b454c9809f33c80d13f361b9674e1801acc37' \
    '[ "$dropins" = "$CORE_TRUSTED_DROPIN" ]' \
    '[ "$dropin_owner_mode" = root:root:644 ]' \
    'sha256sum "$CORE_TRUSTED_DROPIN"' \
    '[ "$dropin_sha256" = "$CORE_TRUSTED_DROPIN_SHA256" ]' \
    "rpm -qf --qf '%{NAME}' \"\$CORE_TRUSTED_DROPIN\"" \
    '[ "$dropin_package" = systemd ]' \
    '[ "$timeout_stop_failure_mode" = abort ]' \
    '[ "$protect_system" = strict ]' \
    '[ "$read_write_paths" = "$CORE_READ_WRITE_PATHS" ]' \
    "stat -Lc '%d:%i' \"/proc/\$main_pid/exe\"" \
    '[ "$running_executable" = "$installed_executable" ]' \
    'prove_production_capability_inventory "$main_pid"' \
    '[ "${#CORE_CAPABILITY_SLUGS[@]}" = 17 ]' \
    '[ -z "${seen_slugs[$slug]+present}" ]' \
    'entry_count" = 17' \
    'goblins-os:$expected_group:2750' \
    'goblins-os:$expected_group:660' \
    '$4 == "00010000" && $5 == "0001" && $6 == "01" && $8 == path' \
    '"/proc/$main_pid/net/unix"' \
    '"socket:[$socket_inode]"' \
    'nsenter --target "$main_pid" --mount --' \
    'mount_is_effectively_writable "$main_pid" /run/goblins-os-core' \
    'mount_is_effectively_writable "$main_pid" /var/lib/goblins-os/voice/work' \
    'GOBLINS_HWGATE_CORE_PRODUCTION_UNIT status=pass identity=systemd-main-pid dropin=vendor-sha256 listeners=17 runtime_mount=rw voice_work_mount=rw' \
    'prove_voice_storage' \
    '/v1/release-proof/storage/voice' \
    '.ok == true and .storage == "voice-work" and .create_new == true and .write == true and .fsync == true and .unlink == true' \
    'GOBLINS_HWGATE_FIRSTBOOT_STAGE stage=voice-storage status=pass curl_rc=0 http_status=200 create_new=true write=true fsync=true unlink=true' \
    'CURRENT_STAGE=core-production-unit'; do
    grep -Fq -- "$needle" "$script" || return 1
  done
  local slug
  for slug in control-center dictate file-builder focus-tick installer launcher login markup open release-proof resident screenshot-context settings shell today visual-lookup voice-control; do
    grep -Eq "(^|[[:space:]])${slug}([[:space:]]|$)" "$script" || return 1
    grep -Fq "d /run/goblins-os-core/$slug 2750 goblins-os goblins-core-$slug -" \
      os/tmpfiles/goblins-os-core.conf || return 1
  done
  ! grep -Fq 'goblins-hwgate-fixture-core' "$script" \
    && grep -Fq $'prove_production_core_unit\nprove_voice_storage\npost_json privacy' "$script" \
    && grep -Fq 'systemctl restart goblins-os-core.service' os/iso/verify-config.toml \
    && rg -Fq 'pub async fn voice_storage_release_proof()' crates/goblins-os-core/src/voice.rs \
    && rg -Fq 'create_new(true)' crates/goblins-os-core/src/voice.rs \
    && rg -Fq 'directory.open_dir_nofollow(name)' crates/goblins-os-core/src/voice.rs \
    && rg -Fq '.follow(FollowSymlinks::No)' crates/goblins-os-core/src/voice.rs \
    && rg -Fq 'metadata.mode() & 0o7777 != REQUIRED_VOICE_WORK_MODE' crates/goblins-os-core/src/voice.rs \
    && rg -Fq 'file.sync_all()' crates/goblins-os-core/src/voice.rs \
    && rg -Fq 'work.remove_file(name)' crates/goblins-os-core/src/voice.rs \
    && rg -Fq 'sync_voice_work_directory(work)' crates/goblins-os-core/src/voice.rs \
    && rg -Fq '(POST, "/v1/release-proof/storage/voice")' crates/goblins-os-core/src/control_plane.rs
}

text_shortcuts_desktop_state_contract_is_pinned() {
  local core=crates/goblins-os-core/src/text_shortcuts.rs
  local core_main=crates/goblins-os-core/src/main.rs
  local client=crates/goblins-os-core/src/session_bridge.rs
  local bridge=crates/goblins-os-session-bridge/src/main.rs
  local engine=crates/goblins-os-textshortcuts-engine/src/lib.rs
  local ibus=os/goblins-os-textshortcuts/goblins-textshortcuts-ibus
  local seed=os/input/goblins-os-input-source-seed
  local proof=os/hardware-gate/capture-harness/in-session-orchestrator.sh

  rg -Fq 'session_bridge::text_shortcuts_read()' "$core" \
    && rg -Fq 'session_bridge::text_shortcuts_write(&table)' "$core" \
    && rg -Fq 'input_source_configured_from_bridge(session_bridge::gsettings(&[' "$core" \
    && rg -Fq 'session_bridge::text_shortcuts_runtime_status()' "$core" \
    && rg -Fq 'TextShortcutsRuntimeStatusResult::Success(status) if status.ready()' "$core" \
    && ! rg -Fq 'bounded_session_command_output("gsettings"' "$core" \
    && rg -Fq 'MAX_PREVIEW_TRIGGER_BYTES: usize = 256' "$core" \
    && rg -Fq 'TEXT_SHORTCUTS_REQUEST_LIMIT_BYTES: usize = 64 * 1024' "$core_main" \
    && rg -Fq '.layer(DefaultBodyLimit::max(TEXT_SHORTCUTS_REQUEST_LIMIT_BYTES))' "$core_main" \
    && rg -Fq 'text_shortcuts_route_rejects_bodies_above_its_private_table_envelope' "$core_main" \
    && ! rg -Fq 'fn table_path()' "$core" \
    && ! rg -Fq 'fs::write(' "$core" \
    && rg -Fq 'TextShortcutsRead' "$client" "$bridge" \
    && rg -Fq 'TextShortcutsWrite' "$client" "$bridge" \
    && rg -Fq 'TextShortcutsRuntimeStatus' "$client" "$bridge" \
    && rg -Fq '/run/goblins-os-session/text-shortcuts-runtime-status.json' "$bridge" \
    && rg -Fq 'const TEXT_SHORTCUTS_RUNTIME_STATUS_SCHEMA: &str = "goblins-os.text-shortcuts-runtime-status.v1";' "$client" \
    && rg -Fq 'const TEXT_SHORTCUTS_RUNTIME_STATUS_SCHEMA: &str = "goblins-os.text-shortcuts-runtime-status.v1";' "$bridge" \
    && rg -Fq 'const TEXT_SHORTCUTS_RUNTIME_STATUS_MAX_BYTES: usize = 4 * 1024;' "$bridge" \
    && rg -Fq 'const TEXT_SHORTCUTS_RUNTIME_STATUS_MAX_FUTURE_NS: u64 = 250_000_000;' "$client" \
    && rg -Fq 'const TEXT_SHORTCUTS_RUNTIME_STATUS_MAX_FUTURE_NS: u64 = 250_000_000;' "$bridge" \
    && rg -Fq '.custom_flags(libc::O_CLOEXEC | libc::O_NOFOLLOW | libc::O_NONBLOCK)' "$bridge" \
    && rg -Fq 'metadata.mode() & 0o7777 != TEXT_SHORTCUTS_RUNTIME_STATUS_MODE' "$bridge" \
    && rg -Fq 'TEXT_SHORTCUTS_RUNTIME_STATUS_MAX_AGE_NS: u64 = 5_000_000_000' "$client" \
    && rg -Fq 'TEXT_SHORTCUTS_RUNTIME_STATUS_MAX_AGE_NS: u64 = 5_000_000_000' "$bridge" \
    && rg -Fq 'enabled: bool,' "$client" \
    && rg -Fq 'enabled: bool,' "$bridge" \
    && rg -Fq 'surrounding_text_supported: bool,' "$client" \
    && rg -Fq 'surrounding_text_supported: bool,' "$bridge" \
    && rg -Fq 'snapshot_valid: bool,' "$client" \
    && rg -Fq 'snapshot_valid: bool,' "$bridge" \
    && rg -Fq '&& self.enabled' "$client" \
    && rg -Fq '&& self.surrounding_text_supported' "$client" \
    && rg -Fq '&& self.snapshot_valid' "$client" \
    && rg -Fq '.take((TEXT_SHORTCUTS_RUNTIME_STATUS_MAX_BYTES + 1) as u64)' "$bridge" \
    && rg -Fq 'before.dev() != after.dev()' "$bridge" \
    && rg -Fq 'before.ino() != after.ino()' "$bridge" \
    && rg -Fq 'before.len() != after.len()' "$bridge" \
    && rg -Fq 'after.len() != encoded.len() as u64' "$bridge" \
    && rg -Fq 'text_shortcuts_runtime_status_protocol_is_fixed_pathless_and_strict' "$bridge" \
    && rg -Fq 'text_shortcuts_runtime_status_rejects_stale_and_materially_future_timestamps' "$bridge" \
    && rg -Fq 'text_shortcuts_runtime_status_rejects_malformed_and_unknown_fields' "$bridge" \
    && rg -Fq 'text_shortcuts_runtime_status_preserves_all_readiness_signals' "$bridge" \
    && rg -Fq 'text_shortcuts_runtime_status_rejects_wrong_mode_and_owner' "$bridge" \
    && rg -Fq 'text_shortcuts_runtime_status_rejects_symlinks_and_oversize_files' "$bridge" \
    && rg -Fq 'text_shortcuts_runtime_status_rejects_fifo_without_blocking' "$bridge" \
    && rg -Fq 'runtime_loop_requires_active_engine_and_fresh_live_child_status' "$core" \
    && rg -Fq 'deny_unknown_fields' "$bridge" \
    && rg -Fq 'read_to_end_before(' "$bridge" \
    && rg -Fq 'MAX_TEXT_SHORTCUTS_TABLE_BYTES: usize = 48 * 1024' "$engine" \
    && rg -Fq 'libc::O_CLOEXEC | libc::O_NOFOLLOW' "$engine" \
    && rg -Fq 'metadata.mode() & 0o7777 == 0o600' "$engine" \
    && rg -Fq 'std::fs::rename(&temporary_path, &self.path)' "$engine" \
    && rg -Fq 'MAX_TEXT_SHORTCUTS_TABLE_BYTES = 48 * 1024' "$ibus" \
    && rg -Fq 'RUNTIME_REQUEST_MAX_BYTES = 64 * 1024' "$ibus" \
    && rg -Fq 'RUNTIME_RESPONSE_MAX_BYTES = 64 * 1024' "$ibus" \
    && rg -Fq 'os.set_blocking(self._process.stdin.fileno(), False)' "$ibus" \
    && rg -Fq 'os.set_blocking(self._process.stdout.fileno(), False)' "$ibus" \
    && rg -Fq 'def _write_request_frame' "$ibus" \
    && rg -Fq 'def _read_response_frame' "$ibus" \
    && rg -Fq 'runtime did not complete one response before timeout' "$ibus" \
    && rg -Fq 'runtime returned extra response framing' "$ibus" \
    && rg -Fq 'RUNTIME_RESTART_MAX_SECONDS = 5.0' "$ibus" \
    && rg -Fq '"SURROUNDING_TEXT"' "$ibus" \
    && rg -Fq 'trigger = delete.get("expected_text")' "$ibus" \
    && rg -Fq 'def _valid_replacement_transaction' "$ibus" \
    && rg -Fq 'operations[1].get("text") != expected_commit' "$ibus" \
    && rg -Fq 'swapped_commit["operations"][1]["text"] = "different replacement "' "$ibus" \
    && rg -Fq 'def _validated_runtime_response_operations' "$ibus" \
    && rg -Fq 'def _runtime_response_valid_for_request' "$ibus" \
    && rg -Fq 'if operations is None or response.get("error") is not None:' "$ibus" \
    && rg -Fq 'set(response) - allowed_keys' "$ibus" \
    && rg -Fq '["delete-surrounding-text", "commit-text", "hide-preedit-text"]' "$ibus" \
    && ! rg -Fq 'text_factory(str(operation.get("text", "")))' "$ibus" \
    && ! rg -Fq 'int(operation.get("cursor_pos", 0))' "$ibus" \
    && ! rg -Fq 'bool(operation.get("visible", False))' "$ibus" \
    && ! rg -Fq 'int(operation.get("offset", 0))' "$ibus" \
    && ! rg -Fq 'int(operation.get("n_chars", 0))' "$ibus" \
    && rg -Fq 'text_factory(operation["text"])' "$ibus" \
    && rg -Fq 'operation["n_chars"]' "$ibus" \
    && rg -Fq 'operation["offset"]' "$ibus" \
    && rg -Fq 'RUNTIME_TEXT_MAX_CHARACTERS = 64 * 1024' "$ibus" \
    && rg -Fq 'RUNTIME_TEXT_MAX_BYTES = 64 * 1024' "$ibus" \
    && rg -Fq 'SURROUNDING_TEXT_MAX_BYTES = 64 * 1024' "$ibus" \
    && rg -Fq 'value.encode("utf-8", errors="strict")' "$ibus" \
    && rg -Fq '0xD800 <= codepoint <= 0xDFFF' "$ibus" \
    && rg -Fq 'UnicodeEncodeError,' "$ibus" \
    && rg -Fq 'ValueError,' "$ibus" \
    && rg -Fq 'unencodable_request_runtime = RuntimeBridge(' "$ibus" \
    && rg -Fq 'huge_integer_frame = (' "$ibus" \
    && rg -Fq 'table_file.write(b"[" + (b"9" * 5000) + b"]")' "$ibus" \
    && rg -Fq 'prepared_text: dict[int, Any] = {}' "$ibus" \
    && rg -Fq 'assert factory_failure_target.calls == []' "$ibus" \
    && rg -Fq 'class FocusBoundSurroundingTextCache' "$ibus" \
    && rg -Fq 'self._surrounding_text_cache.observe(snapshot)' "$ibus" \
    && rg -Fq 'snapshot = self._surrounding_text_cache.current()' "$ibus" \
    && ! rg -Fq 'def _read_surrounding_text' "$ibus" \
    && rg -Fq 'snapshot_cache.end_focus()' "$ibus" \
    && rg -Fq 'return self._sink.publish(record) is not False' "$ibus" \
    && ! rg -Fq 'render intent sink failed: {error}' "$ibus" \
    && ! rg -Fq 'forced candidate hide failed: {error}' "$ibus" \
    && rg -Fq 'self._clear_candidate_ui("runtime-operation-application-failed")' "$ibus" \
    && rg -Fq 'def assert_failed_candidate_publication_disarms' "$ibus" \
    && rg -Fq '"native_show_failure_reported": native_show_failure_reported' "$ibus" \
    && rg -Fq '"retryable_force_hide": retryable_force_hide' "$ibus" \
    && rg -Fq 'table_changed, table_applied = _send_table_changed_if_needed(' "$ibus" \
    && rg -Fq 'self._clear_candidate_ui("table-change-application-failed")' "$ibus" \
    && rg -Fq '"table_change_hide_retry": table_change_hide_retry' "$ibus" \
    && rg -Fq 'IBUS_INPUT_HINT_PRIVATE_FALLBACK = 1 << 11' "$ibus" \
    && rg -Fq 'IBUS_INPUT_HINT_HIDDEN_TEXT_FALLBACK = 1 << 12' "$ibus" \
    && rg -Fq 'def _effective_content_purpose' "$ibus" \
    && rg -Fq '_effective_content_purpose(0, "invalid", FakeIbus) == 8' "$ibus" \
    && rg -Fq 'purpose_value > 0xFFFFFFFF' "$ibus" \
    && rg -Fq "replace.contains('\\0')" "$engine" \
    && rg -Fq "with_text.contains('\\0')" "$engine" \
    && rg -Fq 'def do_disable' "$ibus" \
    && rg -Fq 'self._runtime_status.set_enabled(False)' "$ibus" \
    && rg -Fq 'self._clear_candidate_ui("disabled")' "$ibus" \
    && rg -Fq 'def _clear_candidate_ui' "$ibus" \
    && rg -Fq 'self._candidate_render.clear(self._candidate_state, reason)' "$ibus" \
    && rg -Fq 'self._clear_candidate_ui("runtime-health-failed")' "$ibus" \
    && rg -Fq 'self._clear_candidate_ui("runtime-generation-changed")' "$ibus" \
    && rg -Fq 'class RuntimeStatusPublisher' "$ibus" \
    && rg -Fq 'RUNTIME_STATUS_SCHEMA = "goblins-os.text-shortcuts-runtime-status.v1"' "$ibus" \
    && rg -Fq 'RUNTIME_STATUS_PATH = "/run/goblins-os-session/text-shortcuts-runtime-status.json"' "$ibus" \
    && rg -Fq 'RUNTIME_STATUS_MAX_BYTES = 4096' "$ibus" \
    && rg -Fq '"enabled": self._enabled' "$ibus" \
    && rg -Fq '"surrounding_text_supported": self._surrounding_text_supported' "$ibus" \
    && rg -Fq '"snapshot_valid": self._snapshot_valid' "$ibus" \
    && rg -Fq 'os.fchmod(descriptor, 0o600)' "$ibus" \
    && rg -Fq 'os.fsync(descriptor)' "$ibus" \
    && rg -Fq 'os.rename(' "$ibus" \
    && rg -Fq 'os.fsync(directory)' "$ibus" \
    && rg -Fq 'health_callback=self._runtime_status.runtime_transport' "$ibus" \
    && rg -Fq 'def set_enabled' "$ibus" \
    && rg -Fq 'def set_surrounding_text_supported' "$ibus" \
    && rg -Fq 'def set_snapshot_valid' "$ibus" \
    && rg -Fq 'def _runtime_health_tick' "$ibus" \
    && rg -Fq 'partial_runtime = RuntimeBridge(' "$ibus" \
    && rg -Fq 'oversized_runtime = RuntimeBridge(' "$ibus" \
    && rg -Fq 'malformed_runtime_responses = [' "$ibus" \
    && rg -Fq 'same_trigger_elsewhere =' "$ibus" \
    && rg -Fq 'effective = self._surrounding_guard.validate_response(' "$ibus" \
    && rg -Fq 'not os.path.isabs(config_home)' "$ibus" \
    && rg -Fq 'not os.path.isabs(home)' "$ibus" \
    && rg -Fq 'getattr(os, "O_NOFOLLOW", 0)' "$ibus" \
    && rg -Fq 'metadata.st_uid != os.geteuid()' "$ibus" \
    && rg -Fq 'stat.S_IMODE(metadata.st_mode) != 0o600' "$ibus" \
    && rg -Fq 'not isinstance(replace_value, str)' "$ibus" \
    && rg -Fq 'not isinstance(with_value, str)' "$ibus" \
    && rg -Fq 'RuntimeProtocolRequest::Health => Ok(IbusRuntimeEvent::Health)' "$engine" \
    && rg -Fq 'IbusRuntimeEvent::Health => IbusRuntimeDecision::pass_through(),' "$engine" \
    && rg -Fq 'runtime_protocol_health_is_typed_and_does_not_mutate_edit_state' "$engine" \
    && rg -Fq 'expected_text: expected_text.clone()' "$engine" \
    && rg -Fq '[ "$value" = "true" ]' "$seed" \
    && ! rg -Fq '|| true' "$seed" \
    && rg -Fq 'parsed = ast.literal_eval(raw)' "$seed" \
    && rg -Fq 'annotation = "@a(ss) "' "$seed" \
    && rg -Fq 'annotation = "@as "' "$seed" \
    && rg -Fq 'input source settings were malformed; leaving every source unchanged' "$seed" \
    && rg -Fq 'sources.append((item[0], item[1]))' "$seed" \
    && rg -Fq 'engines.append(item)' "$seed" \
    && rg -Fq 'set_and_verify_input_sources()' "$seed" \
    && rg -Fq 'current_sources="$(gsettings get org.gnome.desktop.input-sources sources 2>/dev/null)"' "$seed" \
    && rg -Fq 'current_mru="$(gsettings get org.gnome.desktop.input-sources mru-sources 2>/dev/null)"' "$seed" \
    && rg -Fq 'current_preload="$(gsettings get org.freedesktop.ibus.general preload-engines 2>/dev/null)"' "$seed" \
    && rg -Fq 'rollback_originals()' "$seed" \
    && rg -Fq 'if [ "$touched" != "1" ]' "$seed" \
    && rg -Fq '[ "$canonical" = "$staged" ] || return 1' "$seed" \
    && rg -Fq 'restore_input_sources_if_unchanged sources "$sources_touched"' "$seed" \
    && rg -Fq 'restore_input_sources_if_unchanged mru-sources "$mru_touched"' "$seed" \
    && rg -Fq 'restore_preload_if_unchanged "$preload_touched"' "$seed" \
    && rg -Fq 'set_and_verify_input_sources sources "$canonical_sources" "$next_sources"' "$seed" \
    && rg -Fq 'set_and_verify_input_sources mru-sources "$canonical_mru" "$next_mru"' "$seed" \
    && rg -Fq 'set_and_verify_preload "$canonical_preload" "$next_preload"' "$seed" \
    && rg -Fq "printf 'seeded %s/%s\\n'" "$seed" \
    && rg -Fq 'DetailedBridgeResult::TransportUnavailable' "$client" \
    && rg -Fq 'text-shortcuts-set) request text-shortcuts-set POST /v1/text-shortcuts' os/hardware-gate/capture-harness/core-proof-operation.sh \
    && rg -Fq "text-shortcuts-preview) request text-shortcuts-preview GET '/v1/text-shortcuts/preview?trigger=omw'" os/hardware-gate/capture-harness/core-proof-operation.sh \
    && rg -Fq 'text-shortcuts-file-contract) text_shortcuts_file_contract' os/hardware-gate/capture-harness/core-proof-operation.sh \
    && rg -Fq 'core_write_http=200&core_read_http=200&core_preview_http=200&file_contract_http=200' "$proof" \
    && rg -Fq 'core_table_roundtrip=true&core_preview_roundtrip=true&desktop_file_contract=true' "$proof" \
    && rg -Fq 'desktop_file_contract=true&desktop_parent_contract=true&desktop_file_owner_mode=true&desktop_file_single_link=true&desktop_file_size_bounded=true&desktop_file_bounded_read=true&legacy_service_table_absent=true' "$proof" \
    && ! rg -Fq "printf '[{\"replace\":\"omw\"" "$proof"
}

fail_check() {
  local label="$1"
  echo "[FAIL] $label"
  FAIL_COUNT=$((FAIL_COUNT + 1))
}

check_file() {
  local label="$1"
  local path="$2"
  if [ -f "$path" ]; then
    echo "[PASS] $label"
    return 0
  fi
  echo "[FAIL] $label: missing $path"
  FAIL_COUNT=$((FAIL_COUNT + 1))
  return 1
}

check_file_contains() {
  local label="$1"
  local path="$2"
  local pattern="$3"
  if [ ! -f "$path" ]; then
    echo "[FAIL] $label: missing $path"
    FAIL_COUNT=$((FAIL_COUNT + 1))
    return 1
  fi
  if rg -q "$pattern" "$path"; then
    echo "[PASS] $label"
    return 0
  fi
  echo "[FAIL] $label: $path does not contain $pattern"
  FAIL_COUNT=$((FAIL_COUNT + 1))
  return 1
}

check_sha256_file() {
  local label="$1"
  local sha_path="$2"
  local expected_basename="$3"
  local expected actual artifact sha_dir sha_base nonempty_lines

  if [ ! -f "$sha_path" ]; then
    echo "[FAIL] $label: missing $sha_path"
    FAIL_COUNT=$((FAIL_COUNT + 1))
    return 1
  fi

  sha_dir="$(cd "$(dirname "$sha_path")" && pwd -P)" || return 1
  sha_base="$(basename "$sha_path")"
  nonempty_lines="$(awk 'NF { count += 1 } END { print count + 0 }' "$sha_dir/$sha_base")"
  read -r expected artifact < "$sha_dir/$sha_base"
  artifact="${artifact#\*}"
  if [ "$nonempty_lines" != "1" ] \
    || [[ ! "$expected" =~ ^[0-9a-f]{64}$ ]] \
    || [ "$artifact" != "$expected_basename" ] \
    || [[ "$artifact" == */* ]] \
    || [ ! -f "$sha_dir/$artifact" ]; then
    echo "[FAIL] $label: checksum must contain one lowercase digest for $expected_basename"
    FAIL_COUNT=$((FAIL_COUNT + 1))
    return 1
  fi

  if command -v sha256sum >/dev/null 2>&1; then
    actual="$(sha256sum "$sha_dir/$artifact" | awk '{print $1}')"
  elif command -v shasum >/dev/null 2>&1; then
    actual="$(shasum -a 256 "$sha_dir/$artifact" | awk '{print $1}')"
  else
    echo "[FAIL] $label: no sha256sum or shasum command available"
    FAIL_COUNT=$((FAIL_COUNT + 1))
    return 1
  fi

  if [ "$actual" = "$expected" ]; then
    echo "[PASS] $label"
    return 0
  fi

  echo "[FAIL] $label: checksum verification failed for $sha_path"
  FAIL_COUNT=$((FAIL_COUNT + 1))
  return 1
}

sha256_of_file() {
  if command -v sha256sum >/dev/null 2>&1; then
    sha256sum "$1" | awk '{print $1}'
  elif command -v shasum >/dev/null 2>&1; then
    shasum -a 256 "$1" | awk '{print $1}'
  else
    return 1
  fi
}

check_bib_manifest_payload_ref() {
  local label="$1"
  local path="$2"
  local expected_ref="$3"
  local actual_ref

  if [ ! -f "$path" ]; then
    echo "[FAIL] $label: missing $path"
    FAIL_COUNT=$((FAIL_COUNT + 1))
    return 1
  fi
  if ! actual_ref="$(goblins_os_bib_manifest_payload_ref "$path")"; then
    echo "[FAIL] $label: manifest must contain exactly one bootc installer payload image reference"
    FAIL_COUNT=$((FAIL_COUNT + 1))
    return 1
  fi
  if goblins_os_image_ref_is_local_only "$actual_ref"; then
    echo "[FAIL] $label: installer payload tracks a local-only Docker/test registry"
    FAIL_COUNT=$((FAIL_COUNT + 1))
    return 1
  fi
  if [ "$actual_ref" != "$expected_ref" ]; then
    echo "[FAIL] $label: installer payload $actual_ref does not match ISO image $expected_ref"
    FAIL_COUNT=$((FAIL_COUNT + 1))
    return 1
  fi
  echo "[PASS] $label"
  return 0
}

installer_local_ref_classifier_passes() {
  local digest="sha256:aaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaa"
  local ref
  local local_refs=(
    "10.0.0.1:5000/org/goblins-os@$digest"
    "172.17.0.1:5000/org/goblins-os@$digest"
    "192.168.1.5:5000/org/goblins-os@$digest"
    "169.254.1.1:5000/org/goblins-os@$digest"
    "127.0.0.1:5000/org/goblins-os@$digest"
    "100.64.0.1:5000/org/goblins-os@$digest"
    "192.0.2.1:5000/org/goblins-os@$digest"
    "[fd00::1]:5000/org/goblins-os@$digest"
    "[fe80::1]:5000/org/goblins-os@$digest"
    "[::1]:5000/org/goblins-os@$digest"
    "localhost.:5000/org/goblins-os@$digest"
    "host.docker.internal:5000/org/goblins-os@$digest"
    "registry:5000/org/goblins-os@$digest"
  )
  local public_refs=(
    "ghcr.io/joe-simo/goblins-os@$digest"
    "quay.io/example/goblins-os@$digest"
    "docker.io/library/goblins-os@$digest"
    "example/goblins-os@$digest"
    "8.8.8.8:5000/example/goblins-os@$digest"
    "[2606:4700:4700::1111]:5000/example/goblins-os@$digest"
  )

  for ref in "${local_refs[@]}"; do
    goblins_os_image_ref_is_local_only "$ref" || return 1
  done
  for ref in "${public_refs[@]}"; do
    if goblins_os_image_ref_is_local_only "$ref"; then
      return 1
    fi
  done
}

source_secret_scan() {
  local output
  local file_list
  local candidate_file
  local candidate_pattern
  local rg_status=0
  local batch=()

  if ! goblins_os_secret_scan_hasher_available; then
    printf '%s\n' "Source secret scan requires sha256sum or shasum." >&2
    return 2
  fi

  output="$(mktemp "${TMPDIR:-/tmp}/goblins-os-source-secret-scan.XXXXXX")" || return 2
  file_list="$(mktemp "${TMPDIR:-/tmp}/goblins-os-source-secret-files.XXXXXX")" || {
    rm -f "$output"
    return 2
  }

  candidate_pattern='([A-Z][A-Z0-9_]*_(API_KEY|ACCESS_TOKEN|AUTH_TOKEN|CLIENT_SECRET|PRIVATE_KEY|SECRET_KEY|PASSWORD|PASSWD|SECRET|TOKEN)|AWS_ACCESS_KEY_ID|AWS_SECRET_ACCESS_KEY)"?[[:space:]]*[:,=]|(^|[^A-Za-z0-9_])(github_pat_[A-Za-z0-9_]{40,}|gh[pousr]_[A-Za-z0-9]{36,})|(^|[^A-Za-z0-9_-])(sk-proj-[A-Za-z0-9_-]{24,}|sk-[A-Za-z0-9_-]{29,}|AIza[A-Za-z0-9_-]{35}|npm_[A-Za-z0-9]{36}|glpat-[A-Za-z0-9_-]{20,}|hf_[A-Za-z0-9]{30,}|xox[baprs]-[A-Za-z0-9-]{20,}|sk_live_[A-Za-z0-9]{16,}|ya29[.][A-Za-z0-9_-]{20,}|SG[.][A-Za-z0-9_-]{16,}[.][A-Za-z0-9_-]{16,})|(^|[^A-Z0-9])(AKIA|ASIA)[A-Z0-9]{16}($|[^A-Z0-9])'
  if ! printf '%s\n' 'const GITHUB_TOKEN: &str = "<source-discovery-fixture>";' \
      | rg -q "$candidate_pattern" \
    || ! printf '%s\n' 'GH_TOKEN: ${{ github.token }}' \
      | rg -q "$candidate_pattern" \
    || ! printf '%s\n' 'command.env("ACME_CLOUD_API_KEY", "<source-discovery-fixture>");' \
      | rg -q "$candidate_pattern"; then
    printf '%s\n' "Source secret candidate discovery self-test failed." >&2
    rm -f "$output" "$file_list"
    return 2
  fi

  rg -l --hidden --no-ignore-vcs --no-ignore -I --max-filesize 2048K \
    "$candidate_pattern" \
    . \
    --glob '!.git/**' \
    --glob '!.claude/**' \
    --glob '!**/node_modules/**' \
    --glob '!**/.next/**' \
    --glob '!**/.vercel/**' \
    --glob '!target/**' \
    --glob '!.ci-target/**' \
    --glob '!artifacts/**' \
    --glob '!libpod/**' \
    --glob '!os/signoff-proofs/**' \
    --glob '!os/screenshots/**' \
    --glob '!os/iso/output*/**' \
    --glob '!os/brand/*.png' \
    > "$file_list" || rg_status=$?

  if [ "$rg_status" -gt 1 ]; then
    printf '%s\n' "Source secret candidate discovery failed." >&2
    rm -f "$output" "$file_list"
    return 2
  fi

  sort -u "$file_list" -o "$file_list"
  while IFS= read -r candidate_file; do
    [ -f "$candidate_file" ] || continue
    batch+=("$candidate_file")
    if [ "${#batch[@]}" -ge 128 ]; then
      if ! goblins_os_scan_source_secret_batch "$output" "${batch[@]}"; then
        rm -f "$output" "$file_list"
        return 2
      fi
      batch=()
    fi
  done < "$file_list"
  if ! goblins_os_scan_source_secret_batch "$output" "${batch[@]}"; then
    rm -f "$output" "$file_list"
    return 2
  fi

  if [ -s "$output" ]; then
    printf '%s\n' "Possible live secrets found in source; matched content is suppressed:"
    sed -n '1,20p' "$output"
    rm -f "$output" "$file_list"
    return 1
  fi

  rm -f "$output" "$file_list"
  return 0
}

screenshot_run_is_complete() {
  local run_dir="$1"
  local arch
  local shot
  arch="$(screenshot_run_arch "$run_dir")"
  [ -n "$arch" ] || return 1
  for shot in "${REQ_SCREENSHOTS[@]}"; do
    screenshot_file_is_valid_png "$run_dir/$shot" || return 1
  done
  semantic_screenshot_frames_are_distinct "$run_dir" quiet >/dev/null 2>&1 || return 1
  screenshot_manifest_is_coherent "$run_dir" "$arch" || return 1
  firewall_live_toggle_proof_passes "$run_dir/$FIREWALL_LIVE_TOGGLE_PROOF" || return 1
  text_shortcuts_session_enable_proof_passes "$run_dir/$TEXT_SHORTCUTS_SESSION_ENABLE_PROOF" || return 1
  text_shortcuts_candidate_metadata_proof_passes "$run_dir/$TEXT_SHORTCUTS_CANDIDATE_METADATA_PROOF" || return 1
  text_shortcuts_overlay_intent_proof_passes "$run_dir/$TEXT_SHORTCUTS_OVERLAY_INTENT_PROOF" || return 1
  text_shortcuts_candidate_bubble_frame_proof_passes "$run_dir/$TEXT_SHORTCUTS_CANDIDATE_BUBBLE_FRAME_PROOF" || return 1
  text_shortcuts_candidate_bubble_layout_proof_passes "$run_dir/$TEXT_SHORTCUTS_CANDIDATE_BUBBLE_LAYOUT_PROOF" || return 1
  text_shortcuts_candidate_bubble_render_intent_proof_passes "$run_dir/$TEXT_SHORTCUTS_CANDIDATE_BUBBLE_RENDER_INTENT_PROOF" || return 1
  text_shortcuts_candidate_bubble_render_proof_passes "$run_dir/$TEXT_SHORTCUTS_CANDIDATE_BUBBLE_RENDER_PROOF" || return 1
  text_shortcuts_live_ibus_runtime_render_proof_passes "$run_dir/$TEXT_SHORTCUTS_LIVE_IBUS_RUNTIME_RENDER_PROOF" || return 1
  keyboard_shortcuts_roundtrip_proof_passes "$run_dir/$KEYBOARD_SHORTCUTS_ROUNDTRIP_PROOF" || return 1
  input_sources_roundtrip_proof_passes "$run_dir/$INPUT_SOURCES_ROUNDTRIP_PROOF" || return 1
  multi_display_apply_proof_passes "$run_dir/$MULTI_DISPLAY_APPLY_PROOF" || return 1
  focus_arm_roundtrip_proof_passes "$run_dir/$FOCUS_ARM_ROUNDTRIP_PROOF" || return 1
  app_privacy_revoke_proof_passes "$run_dir/$APP_PRIVACY_REVOKE_PROOF" || return 1
  preview_open_render_proof_passes "$run_dir/$PREVIEW_OPEN_RENDER_PROOF" || return 1
  audio_output_proof_passes "$run_dir/$AUDIO_OUTPUT_PROOF" || return 1
  runtime_build_proof_passes "$run_dir/$RUNTIME_BUILD_PROOF" || return 1
  accessibility_adaptivity_proof_passes "$run_dir/$ACCESSIBILITY_ADAPTIVITY_PROOF" || return 1
  return 0
}

screenshot_run_arch() {
  case "/$1/" in
    */os/screenshots/hardware-gate/aarch64/*)
      echo "aarch64"
      ;;
    *)
      echo ""
      ;;
  esac
}

screenshot_file_is_valid_png() {
  local file="$1"

  "$ROOT/os/hardware-gate/capture-harness/run-capture.sh" \
    --check-png "$file"
}

semantic_screenshot_frames_are_distinct() {
  "$ROOT/os/hardware-gate/capture-harness/run-capture.sh" \
    --check-semantic-screenshots "$1" "${2:-verbose}"
}

screenshot_manifest_iso_sha() {
  awk -F'"' '/"iso_sha256"/ { print $4; exit }' "$1" 2>/dev/null || true
}

manifest_candidate_commit() {
  awk -F'"' '/"candidate_commit"/ { print tolower($4); exit }' "$1" 2>/dev/null || true
}

manifest_image_ref() {
  awk -F'"' '/"image_ref"/ { print $4; exit }' "$1" 2>/dev/null || true
}

native_packaging_gate_workflow_run() {
  awk -F'"' '/"workflow_run"/ { print $4; exit }' "$1" 2>/dev/null || true
}

native_packaging_gate_workflow_run_attempt() {
  sed -nE 's/^[[:space:]]*"workflow_run_attempt"[[:space:]]*:[[:space:]]*([0-9]+),?$/\1/p' "$1" 2>/dev/null | head -n 1
}

iso_manifest_image_ref() {
  awk -F'"' '/"builder_source_image"/ { print $4; exit }' "$1" 2>/dev/null || true
}

image_ref_is_digest_pinned() {
  [[ "$1" =~ ^[^[:space:]@]+@sha256:[0-9a-f]{64}$ ]]
}

github_actions_run_is_successful() {
  local run_url="$1"
  local expected_commit="$2"
  local expected_attempt="$3"
  local expected_workflow_path="$4"
  local expected_repository="${5:-Joe-Simo/goblins-os}"
  local run_id

  case "$expected_repository" in
    Joe-Simo/goblins-os|Joe-Simo/goblins-os-publisher) ;;
    *) return 1 ;;
  esac
  [ "$run_url" = "https://github.com/$expected_repository/actions/runs/${run_url##*/}" ] || return 1
  [[ "${run_url##*/}" =~ ^[1-9][0-9]*$ ]] || return 1
  [[ "$expected_commit" =~ ^[0-9a-f]{40}$ ]] || return 1
  [[ "$expected_attempt" =~ ^[1-9][0-9]*$ ]] || return 1
  run_id="${run_url##*/}"
  python3 - "$run_id" "$run_url" "$expected_commit" "$expected_attempt" "$expected_workflow_path" "$expected_repository" <<'PY'
import json
import os
import sys
import urllib.request

run_id, run_url, expected_commit, expected_attempt, expected_workflow_paths, expected_repository = sys.argv[1:7]
request = urllib.request.Request(
    f"https://api.github.com/repos/{expected_repository}/actions/runs/{run_id}/attempts/{expected_attempt}",
    headers={
        "Accept": "application/vnd.github+json",
        "User-Agent": "goblins-os-release-verifier",
        "X-GitHub-Api-Version": "2022-11-28",
    },
)
token = os.environ.get("GH_TOKEN") or os.environ.get("GITHUB_TOKEN")
if token:
    request.add_header("Authorization", f"Bearer {token}")
try:
    with urllib.request.urlopen(request, timeout=20) as response:
        run = json.load(response)
except Exception:
    raise SystemExit(1)

expected = {
    "html_url": run_url,
    "status": "completed",
    "conclusion": "success",
    "head_sha": expected_commit,
    "run_attempt": int(expected_attempt),
    "event": "workflow_dispatch",
}
if run.get("repository", {}).get("full_name") != expected_repository:
    raise SystemExit(1)
if run.get("path") not in expected_workflow_paths.split(","):
    raise SystemExit(1)
raise SystemExit(0 if all(run.get(key) == value for key, value in expected.items()) else 1)
PY
}

github_actions_artifact_file_matches() {
  local run_url="$1"
  local artifact_name="$2"
  local local_file="$3"
  local expected_basename="$4"
  local run_id scratch_dir downloaded_file file_count result

  command -v gh >/dev/null 2>&1 || return 1
  [[ "$run_url" =~ ^https://github\.com/Joe-Simo/goblins-os/actions/runs/[0-9]+$ ]] || return 1
  [ -s "$local_file" ] && [ ! -L "$local_file" ] || return 1
  run_id="${run_url##*/}"
  scratch_dir="$(mktemp -d "${TMPDIR:-/tmp}/goblins-actions-artifact.XXXXXX")" || return 1
  if ! gh run download "$run_id" \
    --repo Joe-Simo/goblins-os \
    --name "$artifact_name" \
    --dir "$scratch_dir" >/dev/null 2>&1; then
    rm -rf "$scratch_dir"
    return 1
  fi
  file_count="$(find "$scratch_dir" -type f -name "$expected_basename" -print | awk 'END { print NR + 0 }')"
  downloaded_file="$(find "$scratch_dir" -type f -name "$expected_basename" -print -quit)"
  result=1
  if [ "$file_count" = "1" ] \
    && [ -n "$downloaded_file" ] \
    && python3 - "$local_file" "$downloaded_file" <<'PY'
import os
import stat
import sys

MAX_ARTIFACT_PROOF_BYTES = 16 * 1024 * 1024


def read_stable_regular_file(path: str) -> bytes:
    before = os.lstat(path)
    if (
        not stat.S_ISREG(before.st_mode)
        or before.st_nlink != 1
        or before.st_uid != os.getuid()
        or before.st_size < 1
        or before.st_size > MAX_ARTIFACT_PROOF_BYTES
    ):
        raise RuntimeError("artifact proof is not a bounded private regular file")
    descriptor = os.open(
        path,
        os.O_RDONLY | getattr(os, "O_NOFOLLOW", 0) | getattr(os, "O_CLOEXEC", 0),
    )
    try:
        opened = os.fstat(descriptor)
        identity = (before.st_dev, before.st_ino, before.st_mode, before.st_nlink)
        if identity != (opened.st_dev, opened.st_ino, opened.st_mode, opened.st_nlink):
            raise RuntimeError("artifact proof changed before it was opened")
        chunks = []
        remaining = before.st_size
        while remaining:
            chunk = os.read(descriptor, min(1024 * 1024, remaining))
            if not chunk:
                raise RuntimeError("artifact proof was truncated while reading")
            chunks.append(chunk)
            remaining -= len(chunk)
        if os.read(descriptor, 1):
            raise RuntimeError("artifact proof grew while reading")
        after_open = os.fstat(descriptor)
    finally:
        os.close(descriptor)
    after_path = os.lstat(path)
    stable_fields = ("st_dev", "st_ino", "st_mode", "st_nlink", "st_size", "st_mtime_ns", "st_ctime_ns")
    if any(getattr(before, field) != getattr(after_open, field) for field in stable_fields):
        raise RuntimeError("artifact proof changed while reading")
    if any(getattr(before, field) != getattr(after_path, field) for field in stable_fields):
        raise RuntimeError("artifact proof path changed while reading")
    return b"".join(chunks)


try:
    local_bytes = read_stable_regular_file(sys.argv[1])
    downloaded_bytes = read_stable_regular_file(sys.argv[2])
except (OSError, RuntimeError):
    raise SystemExit(1)
raise SystemExit(0 if local_bytes == downloaded_bytes else 1)
PY
  then
    result=0
  fi
  rm -rf "$scratch_dir"
  return "$result"
}

installer_branding_tool_source_handoff_contract_passes() {
  python3 - "$ROOT" <<'PY'
from pathlib import Path
import re
import sys
import tomllib

root = Path(sys.argv[1])
record = tomllib.loads(
    (root / "os/release/installer-branding-tool.toml").read_text(encoding="utf-8")
)
if record.get("schema") != 1:
    raise SystemExit(1)
if "anonymous_pull_verified" in record or "workflow_run_attempt" in record:
    raise SystemExit(1)

builder = (root / "os/iso/build-iso.sh").read_text(encoding="utf-8")
if "schema-1-bootstrap-diagnostic" not in builder:
    raise SystemExit(1)
if "GOBLINS_OS_INSTALLER_BRANDING_PUBLISHER_EVIDENCE" not in builder:
    raise SystemExit(1)

workflow = (root / ".github/workflows/branding-tool-image.yml").read_text(encoding="utf-8")
required = (
    'schema: "goblins-os-installer-branding-tool-handoff-v1"',
    'schema: "goblins-os-actions-artifact-envelope-v1"',
    'payload_schema: "goblins-os-installer-branding-tool-handoff-v1"',
    'split -n 4 -d -a 2',
    'source_repository_publish_authority: false',
    'non_promotional: true',
    'repository: "Joe-Simo/goblins-os-publisher"',
    'copy_mode: "preserve-digests"',
    'platforms: linux/arm64',
    'select(.platform.architecture == "amd64")',
    'artifacts/manifests/publisher-handoff/branding-tool/aarch64/handoff.json',
    'artifacts/manifests/publisher-handoff/branding-tool/aarch64/publisher-envelope.json',
    'artifacts/manifests/publisher-handoff/branding-tool/aarch64/SHA256SUMS',
    'artifacts/manifests/publisher-handoff/branding-tool/aarch64/rpm-packages.tsv',
)
if any(item not in workflow for item in required):
    raise SystemExit(1)
steps = re.findall(
    r"^      - name: Upload branding-tool OCI payload part (00|01|02|03)$",
    workflow,
    flags=re.MULTILINE,
)
if sorted(steps) != ["00", "01", "02", "03"]:
    raise SystemExit(1)
for suffix in ("00", "01", "02", "03"):
    upload_name = f"goblins-os-branding-tool-oci-${{{{ inputs.candidate_commit }}}}-aarch64-attempt-${{{{ github.run_attempt }}}}-part-{suffix}"
    envelope_name = f"goblins-os-branding-tool-oci-$CANDIDATE_COMMIT-aarch64-attempt-$GITHUB_RUN_ATTEMPT-part-{suffix}"
    if workflow.count(upload_name) != 1 or workflow.count(envelope_name) != 1:
        raise SystemExit(1)
if workflow.count("actions/upload-artifact@") != 5:
    raise SystemExit(1)
if workflow.count("- name: Upload publisher metadata and RPM inventory") != 1:
    raise SystemExit(1)
if "goblins-os-branding-tool-${{ inputs.candidate_commit }}-aarch64" in workflow:
    raise SystemExit(1)
if re.search(
    r"packages:\s*write|contents:\s*write|docker\s+login|docker\s+push|push:\s*true|gh\s+release|git\s+push",
    workflow,
):
    raise SystemExit(1)
PY
}

installer_branding_tool_publisher_gate_contract_passes() {
  python3 - "$ROOT" <<'PY'
from pathlib import Path
import sys

root = Path(sys.argv[1])
gate = (root / "os/hardware-gate/verify-shipping-status.sh").read_text(encoding="utf-8")
builder = (root / "os/iso/build-iso.sh").read_text(encoding="utf-8")
workflow = (root / ".github/workflows/aarch64-verification-iso.yml").read_text(encoding="utf-8")
contract = (root / "os/release/PUBLISHER-BOUNDARY.md").read_text(encoding="utf-8")

required_gate = (
    '"goblins-os-installer-branding-tool-publisher-evidence-v1"',
    '"goblins-os-iso-build-manifest-v2"',
    '"Joe-Simo/goblins-os-publisher"',
    '".github/workflows/publish-branding-tool-aarch64.yml"',
    'actions/artifacts/$publisher_artifact_id/zip',
    'publisher evidence artifact digest',
    'archive.read(member) != evidence_path.read_bytes()',
)
required_builder = (
    '"goblins-os-installer-branding-tool-publisher-evidence-v1"',
    '"goblins-os-iso-build-manifest-v2"',
    '"installer_branding_publisher_evidence_sha256"',
    '"installer_branding_publisher_workflow_run"',
)
required_workflow = (
    'branding_publisher_evidence_base64:',
    'branding_publisher_evidence_sha256:',
    'test "$(wc -c < "$evidence")" -le 32768',
)
required_contract = (
    "cannot truthfully contain its own post-upload digest",
    "verifies the downloaded ZIP against its API size and",
)
if (
    any(marker not in gate for marker in required_gate)
    or any(marker not in builder for marker in required_builder)
    or any(marker not in workflow for marker in required_workflow)
    or any(marker not in contract for marker in required_contract)
):
    raise SystemExit(1)
if (
    ('publisher["' + 'evidence_artifact"]') in gate
    or '"evidence_artifact"' in builder
    or workflow.count("GOBLINS_OS_ISO_CONFIG=os/iso/verify-config.toml") != 1
):
    raise SystemExit(1)
PY
}

installer_branding_tool_publisher_evidence_passes() {
  local evidence="$1"
  local manifest="$2"
  local evidence_size fields source_commit source_run source_attempt
  local publisher_commit publisher_run publisher_attempt
  local source_run_id publisher_run_id scratch_dir source_metadata_name
  local publisher_evidence_name source_inventory publisher_inventory
  local publisher_artifact_fields publisher_artifact_id publisher_artifact_digest
  local publisher_artifact_size publisher_artifact_zip

  [ -s "$evidence" ] && [ ! -L "$evidence" ] && [ -s "$manifest" ] || return 1
  evidence_size="$(wc -c < "$evidence" | tr -d '[:space:]')"
  if [[ ! "$evidence_size" =~ ^[1-9][0-9]*$ ]] || [ "$evidence_size" -gt 32768 ]; then
    return 1
  fi
  fields="$(python3 - "$ROOT" "$evidence" "$manifest" <<'PY'
import hashlib
import json
from pathlib import Path
import re
import sys

root, evidence_path, manifest_path = map(Path, sys.argv[1:4])

def reject_duplicate_keys(pairs):
    result = {}
    for key, value in pairs:
        if key in result:
            raise ValueError("duplicate JSON key")
        result[key] = value
    return result

def load(path):
    return json.loads(path.read_text(encoding="utf-8"), object_pairs_hook=reject_duplicate_keys)

def exact_keys(value, expected):
    if not isinstance(value, dict) or set(value) != set(expected):
        raise ValueError("JSON key set is not exact")

def positive(value):
    return type(value) is int and value > 0

try:
    evidence = load(evidence_path)
    manifest = load(manifest_path)
    exact_keys(evidence, {
        "schema", "product", "architecture", "oci_architecture",
        "source_repository", "source_workflow", "source_handoff", "publisher",
        "published_image", "verification", "source_repository_publish_authority",
        "non_promotional",
    })
    if evidence.get("schema") != "goblins-os-installer-branding-tool-publisher-evidence-v1":
        raise ValueError("evidence schema")
    if evidence.get("product") != "Goblins OS installer branding tool":
        raise ValueError("evidence product")
    if evidence.get("architecture") != "aarch64" or evidence.get("oci_architecture") != "arm64":
        raise ValueError("evidence architecture")
    if evidence.get("source_repository") != "https://github.com/Joe-Simo/goblins-os":
        raise ValueError("source repository")
    if evidence.get("source_repository_publish_authority") is not False or evidence.get("non_promotional") is not True:
        raise ValueError("source authority")

    source = evidence["source_workflow"]
    exact_keys(source, {
        "path", "run", "run_id", "run_attempt", "source_commit",
        "metadata_artifact", "payload_artifacts",
    })
    commit = source["source_commit"]
    if re.fullmatch(r"[0-9a-f]{40}", commit or "") is None:
        raise ValueError("source commit")
    if not positive(source["run_id"]) or not positive(source["run_attempt"]):
        raise ValueError("source run")
    if source["path"] != ".github/workflows/branding-tool-image.yml":
        raise ValueError("source path")
    if source["run"] != f"https://github.com/Joe-Simo/goblins-os/actions/runs/{source['run_id']}":
        raise ValueError("source run URL")

    artifact_keys = {"name", "id", "digest", "size_in_bytes"}
    metadata = source["metadata_artifact"]
    exact_keys(metadata, artifact_keys)
    source_attempt = source["run_attempt"]
    if metadata["name"] != f"goblins-os-branding-tool-oci-{commit}-aarch64-attempt-{source_attempt}-metadata":
        raise ValueError("metadata artifact")
    artifacts = source["payload_artifacts"]
    if not isinstance(artifacts, list) or len(artifacts) != 4:
        raise ValueError("payload artifact count")
    suffixes = set()
    for artifact in artifacts:
        exact_keys(artifact, artifact_keys | {"suffix"})
        suffix = artifact["suffix"]
        if suffix not in {"00", "01", "02", "03"} or suffix in suffixes:
            raise ValueError("payload suffix")
        suffixes.add(suffix)
        if artifact["name"] != f"goblins-os-branding-tool-oci-{commit}-aarch64-attempt-{source_attempt}-part-{suffix}":
            raise ValueError("payload name")
    for artifact in [metadata, *artifacts]:
        if not positive(artifact["id"]) or not positive(artifact["size_in_bytes"]):
            raise ValueError("artifact numeric identity")
        if re.fullmatch(r"sha256:[0-9a-f]{64}", artifact["digest"] or "") is None:
            raise ValueError("artifact digest")

    handoff = evidence["source_handoff"]
    exact_keys(handoff, {
        "schema", "envelope_schema", "handoff_sha256", "envelope_sha256",
        "checksums_sha256", "rpm_inventory_sha256", "rpm_package_count",
        "oci_archive_sha256", "oci_archive_size_bytes", "oci_image_digest",
        "intended_immutable_image_ref", "base_image", "containerfile_sha256",
    })
    if handoff["schema"] != "goblins-os-installer-branding-tool-handoff-v1":
        raise ValueError("handoff schema")
    if handoff["envelope_schema"] != "goblins-os-actions-artifact-envelope-v1":
        raise ValueError("envelope schema")
    for key in (
        "handoff_sha256", "envelope_sha256", "checksums_sha256",
        "rpm_inventory_sha256", "oci_archive_sha256", "containerfile_sha256",
    ):
        if re.fullmatch(r"[0-9a-f]{64}", handoff.get(key, "")) is None:
            raise ValueError("handoff hash")
    if not positive(handoff["rpm_package_count"]) or not positive(handoff["oci_archive_size_bytes"]):
        raise ValueError("handoff numeric identity")
    if handoff["oci_archive_size_bytes"] > 34359738368:
        raise ValueError("handoff archive bound")
    if re.fullmatch(r"sha256:[0-9a-f]{64}", handoff["oci_image_digest"] or "") is None:
        raise ValueError("OCI digest")
    image_ref = handoff["intended_immutable_image_ref"]
    if image_ref != f"ghcr.io/joe-simo/goblins-os-installer-branding-tool@{handoff['oci_image_digest']}":
        raise ValueError("intended image ref")

    publisher = evidence["publisher"]
    exact_keys(publisher, {
        "repository", "workflow_path", "workflow_commit", "run", "run_id",
        "run_attempt", "environment", "native_runner",
    })
    if re.fullmatch(r"[0-9a-f]{40}", publisher["workflow_commit"] or "") is None:
        raise ValueError("publisher commit")
    if not positive(publisher["run_id"]) or not positive(publisher["run_attempt"]):
        raise ValueError("publisher run")
    if publisher["repository"] != "Joe-Simo/goblins-os-publisher":
        raise ValueError("publisher repository")
    if publisher["workflow_path"] != ".github/workflows/publish-branding-tool-aarch64.yml":
        raise ValueError("publisher workflow")
    if publisher["run"] != f"https://github.com/Joe-Simo/goblins-os-publisher/actions/runs/{publisher['run_id']}":
        raise ValueError("publisher run URL")
    if publisher["environment"] != "candidate" or publisher["native_runner"] != "aarch64":
        raise ValueError("publisher environment")

    published = evidence["published_image"]
    exact_keys(published, {
        "immutable_ref", "manifest_digest", "digest_preserved",
        "public_readback_verified", "os", "architecture", "revision", "base_image",
        "containerfile_sha256", "rpm_inventory_sha256", "rpm_package_count",
    })
    if published["immutable_ref"] != image_ref or published["manifest_digest"] != handoff["oci_image_digest"]:
        raise ValueError("published image digest")
    if published["digest_preserved"] is not True or published["public_readback_verified"] is not True:
        raise ValueError("publisher read-back")
    if published["os"] != "linux" or published["architecture"] != "arm64" or published["revision"] != commit:
        raise ValueError("published image identity")
    for key in ("base_image", "containerfile_sha256", "rpm_inventory_sha256", "rpm_package_count"):
        if published[key] != handoff[key]:
            raise ValueError("published image provenance")

    verification = evidence["verification"]
    exact_keys(verification, {
        "source_run_authenticated", "metadata_artifact_digest_verified",
        "payload_artifact_digests_verified", "ordered_parts_verified",
        "oci_archive_verified", "required_tools_verified", "public_manifest_verified",
    })
    if any(value is not True for value in verification.values()):
        raise ValueError("publisher verification")

    if hashlib.sha256((root / "os/iso/branding-tool.Containerfile").read_bytes()).hexdigest() != handoff["containerfile_sha256"]:
        raise ValueError("Containerfile hash")
    evidence_sha = hashlib.sha256(evidence_path.read_bytes()).hexdigest()
    manifest_expected = {
        "schema": "goblins-os-iso-build-manifest-v2",
        "installer_branding_image": image_ref,
        "installer_branding_ownership_helper_image": image_ref,
        "installer_branding_provenance_kind": "protected-publisher-evidence-v1",
        "installer_branding_publisher_evidence": "installer-branding-publisher-evidence.json",
        "installer_branding_publisher_evidence_sha256": evidence_sha,
        "installer_branding_source_commit": commit,
        "installer_branding_source_workflow_run": source["run"],
        "installer_branding_source_workflow_run_attempt": source["run_attempt"],
        "installer_branding_publisher_workflow_commit": publisher["workflow_commit"],
        "installer_branding_publisher_workflow_run": publisher["run"],
        "installer_branding_publisher_workflow_run_attempt": publisher["run_attempt"],
        "installer_branding_handoff_sha256": handoff["handoff_sha256"],
        "installer_branding_envelope_sha256": handoff["envelope_sha256"],
        "installer_branding_oci_archive_sha256": handoff["oci_archive_sha256"],
    }
    if any(manifest.get(key) != value for key, value in manifest_expected.items()):
        raise ValueError("ISO manifest branding evidence binding")
except (KeyError, TypeError, ValueError, OSError, UnicodeError, json.JSONDecodeError):
    raise SystemExit(1)

print("\t".join((
    commit,
    source["run"],
    str(source["run_attempt"]),
    publisher["workflow_commit"],
    publisher["run"],
    str(publisher["run_attempt"]),
    metadata["name"],
    f"goblins-os-branding-tool-publisher-evidence-{commit}-aarch64",
)))
PY
)" || return 1
  IFS=$'\t' read -r \
    source_commit source_run source_attempt publisher_commit publisher_run \
    publisher_attempt source_metadata_name publisher_evidence_name <<< "$fields"

  github_actions_run_is_successful \
    "$source_run" "$source_commit" "$source_attempt" \
    ".github/workflows/branding-tool-image.yml" \
    "Joe-Simo/goblins-os" || return 1
  github_actions_run_is_successful \
    "$publisher_run" "$publisher_commit" "$publisher_attempt" \
    ".github/workflows/publish-branding-tool-aarch64.yml" \
    "Joe-Simo/goblins-os-publisher" || return 1

  command -v gh >/dev/null 2>&1 || return 1
  source_run_id="${source_run##*/}"
  publisher_run_id="${publisher_run##*/}"
  scratch_dir="$(mktemp -d "${TMPDIR:-/tmp}/goblins-branding-publisher-evidence.XXXXXX")" || return 1
  source_inventory="$scratch_dir/source-artifacts.json"
  publisher_inventory="$scratch_dir/publisher-artifacts.json"
  if ! gh api "repos/Joe-Simo/goblins-os/actions/runs/$source_run_id/artifacts?per_page=100" \
      > "$source_inventory" 2>/dev/null \
    || ! gh api "repos/Joe-Simo/goblins-os-publisher/actions/runs/$publisher_run_id/artifacts?per_page=100" \
      > "$publisher_inventory" 2>/dev/null; then
    rm -rf "$scratch_dir"
    return 1
  fi
  publisher_artifact_fields="$(python3 - \
    "$evidence" \
    "$source_inventory" \
    "$publisher_inventory" \
    "$publisher_evidence_name" <<'PY'
import json
import re
import sys

evidence, source_inventory, publisher_inventory = (
    json.load(open(path, encoding="utf-8")) for path in sys.argv[1:4]
)
publisher_evidence_name = sys.argv[4]

def matches(record, actual):
    return (
        all(actual.get(key) == record[key] for key in ("name", "id", "digest", "size_in_bytes"))
        and actual.get("expired") is False
    )

def positive(value):
    return type(value) is int and value > 0

source_expected = [
    evidence["source_workflow"]["metadata_artifact"],
    *evidence["source_workflow"]["payload_artifacts"],
]
source_actual = source_inventory.get("artifacts", [])
if any(len([actual for actual in source_actual if matches(expected, actual)]) != 1 for expected in source_expected):
    raise SystemExit(1)
publisher_actual = publisher_inventory.get("artifacts", [])
publisher_matches = [
    artifact
    for artifact in publisher_actual
    if artifact.get("name") == publisher_evidence_name
    and artifact.get("expired") is False
    and positive(artifact.get("id"))
    and positive(artifact.get("size_in_bytes"))
    and re.fullmatch(r"sha256:[0-9a-f]{64}", artifact.get("digest") or "") is not None
    and artifact.get("archive_download_url")
        == f"https://api.github.com/repos/Joe-Simo/goblins-os-publisher/actions/artifacts/{artifact['id']}/zip"
]
if len(publisher_matches) != 1:
    raise SystemExit(1)
publisher_artifact = publisher_matches[0]
print("\t".join((
    str(publisher_artifact["id"]),
    publisher_artifact["digest"],
    str(publisher_artifact["size_in_bytes"]),
)))
PY
  )" || {
    rm -rf "$scratch_dir"
    return 1
  }
  IFS=$'\t' read -r \
    publisher_artifact_id publisher_artifact_digest publisher_artifact_size \
    <<< "$publisher_artifact_fields"
  [[ "$publisher_artifact_id" =~ ^[1-9][0-9]*$ ]] \
    && [[ "$publisher_artifact_digest" =~ ^sha256:[0-9a-f]{64}$ ]] \
    && [[ "$publisher_artifact_size" =~ ^[1-9][0-9]*$ ]] || {
      rm -rf "$scratch_dir"
      return 1
    }

  publisher_artifact_zip="$scratch_dir/publisher-evidence.zip"
  if ! gh run download "$source_run_id" \
      --repo Joe-Simo/goblins-os \
      --name "$source_metadata_name" \
      --dir "$scratch_dir/source-metadata" >/dev/null 2>&1 \
    || ! gh api --method GET \
      "repos/Joe-Simo/goblins-os-publisher/actions/artifacts/$publisher_artifact_id/zip" \
      > "$publisher_artifact_zip" 2>/dev/null \
    || ! python3 - \
      "$evidence" \
      "$scratch_dir/source-metadata" \
      "$publisher_artifact_zip" \
      "$publisher_artifact_digest" \
      "$publisher_artifact_size" <<'PY'
import hashlib
import json
from pathlib import Path
import stat
import sys
import zipfile

evidence_path = Path(sys.argv[1])
source_root = Path(sys.argv[2])
publisher_zip = Path(sys.argv[3])
publisher_digest = sys.argv[4]
publisher_size = int(sys.argv[5])
evidence = json.loads(evidence_path.read_text(encoding="utf-8"))

def only_file(root, name):
    matches = list(root.rglob(name))
    if len(matches) != 1 or not matches[0].is_file() or matches[0].is_symlink():
        raise ValueError("artifact member identity")
    return matches[0]

try:
    publisher_zip_bytes = publisher_zip.read_bytes()
    if len(publisher_zip_bytes) != publisher_size:
        raise ValueError("publisher evidence artifact size")
    if f"sha256:{hashlib.sha256(publisher_zip_bytes).hexdigest()}" != publisher_digest:
        raise ValueError("publisher evidence artifact digest")
    with zipfile.ZipFile(publisher_zip) as archive:
        members = archive.infolist()
        if len(members) != 1:
            raise ValueError("publisher evidence member set")
        member = members[0]
        member_type = stat.S_IFMT(member.external_attr >> 16)
        if (
            member.filename != "installer-branding-publisher-evidence.json"
            or member.is_dir()
            or member_type not in (0, stat.S_IFREG)
            or member.file_size < 1
            or member.file_size > 32768
        ):
            raise ValueError("publisher evidence member identity")
        if archive.read(member) != evidence_path.read_bytes():
            raise ValueError("publisher evidence bytes")

    source_files = [path for path in source_root.rglob("*") if path.is_file() or path.is_symlink()]
    if len(source_files) != 4 or {path.name for path in source_files} != {
        "handoff.json", "publisher-envelope.json", "SHA256SUMS", "rpm-packages.tsv"
    }:
        raise ValueError("source metadata member set")
    handoff_path = only_file(source_root, "handoff.json")
    envelope_path = only_file(source_root, "publisher-envelope.json")
    checksums_path = only_file(source_root, "SHA256SUMS")
    inventory_path = only_file(source_root, "rpm-packages.tsv")
    handoff = json.loads(handoff_path.read_text(encoding="utf-8"))
    envelope = json.loads(envelope_path.read_text(encoding="utf-8"))
    source = evidence["source_workflow"]
    sealed = evidence["source_handoff"]
    hashes = {
        "handoff_sha256": hashlib.sha256(handoff_path.read_bytes()).hexdigest(),
        "envelope_sha256": hashlib.sha256(envelope_path.read_bytes()).hexdigest(),
        "checksums_sha256": hashlib.sha256(checksums_path.read_bytes()).hexdigest(),
        "rpm_inventory_sha256": hashlib.sha256(inventory_path.read_bytes()).hexdigest(),
    }
    if any(sealed[key] != value for key, value in hashes.items()):
        raise ValueError("source metadata hash")
    if handoff.get("schema") != sealed["schema"] or envelope.get("schema") != sealed["envelope_schema"]:
        raise ValueError("source metadata schema")
    if handoff.get("candidate_commit") != source["source_commit"]:
        raise ValueError("handoff commit")
    if handoff.get("workflow_run") != source["run"] or handoff.get("workflow_run_attempt") != source["run_attempt"]:
        raise ValueError("handoff run")
    if handoff.get("image_digest") != sealed["oci_image_digest"]:
        raise ValueError("handoff image digest")
    if handoff.get("intended_immutable_image_ref") != sealed["intended_immutable_image_ref"]:
        raise ValueError("handoff image ref")
    if handoff.get("oci_archive", {}).get("sha256") != sealed["oci_archive_sha256"]:
        raise ValueError("handoff archive hash")
    if handoff.get("oci_archive", {}).get("size_bytes") != sealed["oci_archive_size_bytes"]:
        raise ValueError("handoff archive size")
    parts = handoff.get("oci_archive", {}).get("parts")
    if not isinstance(parts, list) or len(parts) != 4:
        raise ValueError("handoff parts")
    expected_part_names = [
        f"parts/goblins-os-branding-tool-aarch64.oci.tar.part-{suffix}"
        for suffix in ("00", "01", "02", "03")
    ]
    if [part.get("name") for part in parts] != expected_part_names:
        raise ValueError("handoff part order")
    checksum_lines = checksums_path.read_text(encoding="utf-8").splitlines()
    expected_lines = [f"{part['sha256']}  {part['name']}" for part in parts]
    if checksum_lines != expected_lines:
        raise ValueError("ordered part checksums")
    if envelope.get("payload_schema") != sealed["schema"]:
        raise ValueError("envelope payload schema")
    if envelope.get("candidate_commit") != source["source_commit"]:
        raise ValueError("envelope commit")
    if envelope.get("handoff_sha256") != sealed["handoff_sha256"]:
        raise ValueError("envelope handoff hash")
    if envelope.get("checksums_sha256") != sealed["checksums_sha256"]:
        raise ValueError("envelope checksums hash")
    if envelope.get("rpm_inventory_sha256") != sealed["rpm_inventory_sha256"]:
        raise ValueError("envelope inventory hash")
    envelope_artifacts = envelope.get("payload_artifacts")
    if not isinstance(envelope_artifacts, list) or len(envelope_artifacts) != 4:
        raise ValueError("envelope artifacts")
    source_artifacts = sorted(source["payload_artifacts"], key=lambda item: item["suffix"])
    for expected, actual in zip(source_artifacts, envelope_artifacts):
        if any(actual.get(key) != expected[key] for key in ("name", "id", "digest")):
            raise ValueError("envelope artifact identity")
    inventory_rows = inventory_path.read_text(encoding="utf-8").splitlines()
    if not inventory_rows or inventory_rows[0] != "name\tevr\tarch\tlicense\tvendor":
        raise ValueError("RPM inventory header")
    if len(inventory_rows) - 1 != sealed["rpm_package_count"]:
        raise ValueError("RPM inventory count")

except (KeyError, TypeError, ValueError, OSError, UnicodeError, json.JSONDecodeError, zipfile.BadZipFile):
    raise SystemExit(1)
PY
  then
    rm -rf "$scratch_dir"
    return 1
  fi
  rm -rf "$scratch_dir"
}

iso_manifest_release_provenance_passes() {
  local manifest="$1"
  local arch="$2"
  local commit="$3"
  local image_ref="$4"
  local branding_evidence="$5"

  architecture_is_canonical "$arch" || return 1
  [ -s "$manifest" ] && [ -s "$branding_evidence" ] || return 1
  python3 - \
    "$manifest" \
    "$arch" \
    "$commit" \
    "$image_ref" \
    "$EXPECTED_BIB_IMAGE" \
    "$branding_evidence" <<'PY'
import json
import sys

manifest_path, arch, commit, image_ref, bib_image, evidence_path = sys.argv[1:7]
with open(manifest_path, encoding="utf-8") as handle:
    manifest = json.load(handle)
with open(evidence_path, encoding="utf-8") as handle:
    evidence = json.load(handle)
branding_image = evidence.get("published_image", {}).get("immutable_ref")
expected = {
    "schema": "goblins-os-iso-build-manifest-v2",
    "architecture": arch,
    "candidate_commit": commit,
    "image": image_ref,
    "native_host_os": "Linux",
    "native_host_arch": arch,
    "container_engine_arch": arch,
    "installer_config": "os/iso/config.toml",
    "installer_branding_applied": True,
    "installer_branding_image": branding_image,
    "installer_branding_ownership_helper_image": branding_image,
    "installer_branding_provenance_kind": "protected-publisher-evidence-v1",
    "installer_branding_publisher_evidence": "installer-branding-publisher-evidence.json",
    "builder_image": bib_image,
    "builder_output_ownership_helper_image": bib_image,
    "builder_source_image": image_ref,
    "installer_payload_source_kind": "release-registry",
    "installer_payload_source_local_only": False,
    "shippable_release": True,
}
raise SystemExit(0 if all(manifest.get(key) == value for key, value in expected.items()) else 1)
PY
}

candidate_artifact_metadata_passes() {
  local metadata="$1"
  local arch="$2"
  local commit="$3"
  local image_ref="$4"
  local iso_sha="$5"

  architecture_is_canonical "$arch" || return 1
  [ -s "$metadata" ] || return 1
  if ! python3 - "$metadata" "$arch" "$commit" "$image_ref" "$iso_sha" <<'PY'
import json
import hashlib
from pathlib import Path
import re
import sys

path, arch, commit, image_ref, iso_sha = sys.argv[1:6]
with open(path, encoding="utf-8") as handle:
    metadata = json.load(handle)
run_url = metadata.get("workflow_run", "")
source_repository = metadata.get("source_repository", "")
if not re.fullmatch(r"https://github\.com/Joe-Simo/goblins-os/actions/runs/[0-9]+", run_url):
    raise SystemExit(1)
if source_repository != "https://github.com/Joe-Simo/goblins-os":
    raise SystemExit(1)
if not re.fullmatch(r"ghcr\.io/joe-simo/goblins-os@sha256:[0-9a-f]{64}", image_ref):
    raise SystemExit(1)
if metadata.get("workflow_name") != "candidate-artifacts":
    raise SystemExit(1)
if not isinstance(metadata.get("workflow_run_attempt"), int) or metadata["workflow_run_attempt"] < 1:
    raise SystemExit(1)

def sha256(path):
    return hashlib.sha256(Path(path).read_bytes()).hexdigest()

iso_manifest = f"os/iso/output/{arch}/manifest-goblins-os-{arch}.json"
bib_manifest = f"os/iso/output/{arch}/manifest-anaconda-iso.json"
evidence_dir = Path(f"os/signoff-proofs/sbom/{arch}")
evidence_manifest = evidence_dir / "release-evidence-manifest.json"
cargo_tsv = evidence_dir / "cargo-lock-packages.tsv"
rpm_command = evidence_dir / "rpm-packages.command"
rpm_tsv = evidence_dir / "rpm-packages.tsv"
expected = {
    "schema": "goblins-os-candidate-image-ref-v3",
    "product": "Goblins OS",
    "architecture": arch,
    "candidate_commit": commit,
    "image_digest": image_ref.rsplit("@", 1)[1],
    "immutable_image_ref": image_ref,
    "oci_revision": commit,
    "iso_sha256": iso_sha,
    "iso_manifest_sha256": sha256(iso_manifest),
    "bib_manifest_sha256": sha256(bib_manifest),
    "release_evidence_manifest_sha256": sha256(evidence_manifest),
    "cargo_packages_sha256": sha256(cargo_tsv),
    "rpm_command_sha256": sha256(rpm_command),
    "rpm_packages_sha256": sha256(rpm_tsv),
    "installer_config": "os/iso/config.toml",
    "candidate_tag_authoritative": False,
    "non_promotional": True,
}
if not all(metadata.get(key) == value for key, value in expected.items()):
    raise SystemExit(1)
gates = metadata.get("exact_candidate_gates", {})
raise SystemExit(
    0
    if gates.get("source_verifier") == "pass"
    and gates.get("installed_root_verifier") == "pass"
    and gates.get("services_selftest") == "pass"
    else 1
)
PY
  then
    return 1
  fi
  github_actions_run_is_successful \
    "$(candidate_artifact_workflow_run "$metadata")" \
    "$commit" \
    "$(candidate_artifact_workflow_attempt "$metadata")" \
    ".github/workflows/candidate-artifacts.yml,.github/workflows/release.yml" \
    || return 1
  github_actions_artifact_file_matches \
    "$(candidate_artifact_workflow_run "$metadata")" \
    "goblins-os-candidate-ref-$commit-$arch" \
    "$metadata" \
    "image-ref.json"
}

candidate_artifact_workflow_run() {
  python3 - "$1" <<'PY'
import json
import sys

with open(sys.argv[1], encoding="utf-8") as handle:
    print(json.load(handle).get("workflow_run", ""))
PY
}

candidate_artifact_workflow_attempt() {
  python3 - "$1" <<'PY'
import json
import sys

with open(sys.argv[1], encoding="utf-8") as handle:
    print(json.load(handle).get("workflow_run_attempt", ""))
PY
}

native_packaging_gate_proof_passes() {
  local proof="$1"
  local arch="$2"
  local commit="$3"
  local image_ref="$4"
  local expected_run_url="${5:-}"
  local iso_sha="${6:-}"
  local iso_manifest_sha="${7:-}"
  local bib_manifest_sha="${8:-}"
  local evidence_manifest_sha="${9:-}"

  architecture_is_canonical "$arch" || return 1
  [ -s "$proof" ] || return 1
  python3 - \
    "$proof" \
    "$arch" \
    "$commit" \
    "$image_ref" \
    "$expected_run_url" \
    "$iso_sha" \
    "$iso_manifest_sha" \
    "$bib_manifest_sha" \
    "$evidence_manifest_sha" <<'PY'
import json
import re
import sys

(
    path,
    arch,
    commit,
    image_ref,
    expected_run_url,
    iso_sha,
    iso_manifest_sha,
    bib_manifest_sha,
    evidence_manifest_sha,
) = sys.argv[1:10]
with open(path, encoding="utf-8") as handle:
    proof = json.load(handle)
run_url = proof.get("workflow_run", "")
if not re.fullmatch(r"https://github\.com/Joe-Simo/goblins-os/actions/runs/[0-9]+", run_url):
    raise SystemExit(1)
if not isinstance(proof.get("workflow_run_attempt"), int) or proof["workflow_run_attempt"] < 1:
    raise SystemExit(1)
if expected_run_url and run_url != expected_run_url:
    raise SystemExit(1)
expected = {
    "schema": "goblins-os-native-packaging-gate-v1",
    "architecture": arch,
    "candidate_commit": commit,
    "image_ref": image_ref,
    "image_digest_pinned": True,
    "source_verifier": "pass",
    "installed_root_verifier": "pass",
    "services_selftest": "pass",
    "verification_iso_sha256": iso_sha,
    "iso_manifest_sha256": iso_manifest_sha,
    "bib_manifest_sha256": bib_manifest_sha,
    "release_evidence_manifest_sha256": evidence_manifest_sha,
    "runner_os": "Linux",
    "runner_architecture": arch,
    "native_runner": True,
    "source_repository": run_url.split("/actions/runs/", 1)[0],
}
raise SystemExit(0 if all(proof.get(key) == value for key, value in expected.items()) else 1)
PY
}

screenshot_manifest_is_coherent() {
  local run_dir="$1"
  local arch="$2"
  local manifest="$run_dir/proof-manifest.json"
  local iso_path="os/iso/output/$arch/bootiso/goblins-os-$arch.iso"
  local image_ref
  local verification_iso_manifest="$run_dir/verification-iso-manifest.json"
  local verification_bib_manifest="$run_dir/verification-bib-manifest.json"
  local verification_evidence_manifest="$run_dir/verification-release-evidence-manifest.json"
  local canonical_evidence_dir="os/signoff-proofs/sbom/$arch"
  local canonical_evidence_manifest="$canonical_evidence_dir/release-evidence-manifest.json"
  local iso_sha iso_manifest_sha bib_manifest_sha evidence_manifest_sha
  local recorded_evidence_manifest_sha
  local live_proof live_screenshot recorded_manifest_screenshot_sha
  local recorded_proof_screenshot_sha actual_screenshot_sha
  local native_run native_attempt
  local canonical_run_dir run_date bundle_digest artifact_name
  local attestation_fields authority_fingerprint authority_ca_fingerprint authority_iso_sha authority_screenshot_manifest_sha

  architecture_is_canonical "$arch" || return 1
  [ -s "$manifest" ] || return 1
  [ -s "$verification_iso_manifest" ] || return 1
  [ -s "$verification_bib_manifest" ] || return 1
  [ -s "$verification_evidence_manifest" ] || return 1
  goblins_os_release_evidence_hashes_match "$canonical_evidence_dir" || return 1
  [ "$(sha256_of_file "$verification_evidence_manifest")" = "$(sha256_of_file "$canonical_evidence_manifest")" ] \
    || return 1
  [ "$CANDIDATE_SELECTION_VALID" -eq 1 ] || return 1
  image_ref="$(iso_manifest_image_ref "os/iso/output/$arch/manifest-goblins-os-$arch.json")"
  image_ref_is_digest_pinned "$image_ref" || return 1
  canonical_run_dir="$(
    python3 "$ROOT/os/hardware-gate/capture-harness/proof_validation.py" \
      --run-directory "$run_dir" "$ROOT" "$arch"
  )" || return 1
  [ "$canonical_run_dir" = "$run_dir" ] || return 1
  python3 "$ROOT/os/hardware-gate/capture-harness/proof_validation.py" \
    --manifest "$manifest" "$arch" "$SELECTED_CANDIDATE_COMMIT" "$image_ref" \
    "$iso_path" "$run_dir" \
    || return 1
  bundle_digest="$(evidence_bundle_digest "$run_dir" "$arch" "$image_ref")" || return 1
  [[ "$bundle_digest" =~ ^[0-9a-f]{64}$ ]] || return 1
  [ "$(manifest_candidate_commit "$verification_iso_manifest")" = "$SELECTED_CANDIDATE_COMMIT" ] || return 1
  [ "$(iso_manifest_image_ref "$verification_iso_manifest")" = "$image_ref" ] || return 1
  rg -Fq '"installer_config": "os/iso/verify-config.toml"' "$verification_iso_manifest" || return 1
  goblins_os_bib_manifest_payload_ref "$verification_bib_manifest" | grep -Fxq "$image_ref" || return 1
  [ "$(manifest_candidate_commit "$verification_evidence_manifest")" = "$SELECTED_CANDIDATE_COMMIT" ] || return 1
  [ "$(manifest_image_ref "$verification_evidence_manifest")" = "$image_ref" ] || return 1
  grep -Fq '"image_ref": "'"$image_ref"'"' "$manifest" \
    && rg -q '"architecture"[[:space:]]*:[[:space:]]*"'"$arch"'"' "$manifest" \
    && rg -q '"candidate_commit"[[:space:]]*:[[:space:]]*"'"$SELECTED_CANDIDATE_COMMIT"'"' "$manifest" \
    && rg -q '"iso"[[:space:]]*:[[:space:]]*"'"$iso_path"'"' "$manifest" \
    && rg -q '"iso_sha256"[[:space:]]*:[[:space:]]*"[a-fA-F0-9]{64}"' "$manifest" \
    && rg -q '"captured_at"[[:space:]]*:[[:space:]]*"[^"]+"' "$manifest" \
    && rg -q '"screenshot_run_dir"[[:space:]]*:[[:space:]]*"'"$run_dir"'"' "$manifest" \
    && rg -q '"firewall_live_toggle_proof"[[:space:]]*:[[:space:]]*"'"$FIREWALL_LIVE_TOGGLE_PROOF"'"' "$manifest" \
    && rg -q '"text_shortcuts_session_enable_proof"[[:space:]]*:[[:space:]]*"'"$TEXT_SHORTCUTS_SESSION_ENABLE_PROOF"'"' "$manifest" \
    && rg -q '"text_shortcuts_candidate_metadata_proof"[[:space:]]*:[[:space:]]*"'"$TEXT_SHORTCUTS_CANDIDATE_METADATA_PROOF"'"' "$manifest" \
    && rg -q '"text_shortcuts_overlay_intent_proof"[[:space:]]*:[[:space:]]*"'"$TEXT_SHORTCUTS_OVERLAY_INTENT_PROOF"'"' "$manifest" \
    && rg -q '"text_shortcuts_candidate_bubble_frame_proof"[[:space:]]*:[[:space:]]*"'"$TEXT_SHORTCUTS_CANDIDATE_BUBBLE_FRAME_PROOF"'"' "$manifest" \
    && rg -q '"text_shortcuts_candidate_bubble_layout_proof"[[:space:]]*:[[:space:]]*"'"$TEXT_SHORTCUTS_CANDIDATE_BUBBLE_LAYOUT_PROOF"'"' "$manifest" \
    && rg -q '"text_shortcuts_candidate_bubble_render_intent_proof"[[:space:]]*:[[:space:]]*"'"$TEXT_SHORTCUTS_CANDIDATE_BUBBLE_RENDER_INTENT_PROOF"'"' "$manifest" \
    && rg -q '"text_shortcuts_candidate_bubble_render_proof"[[:space:]]*:[[:space:]]*"'"$TEXT_SHORTCUTS_CANDIDATE_BUBBLE_RENDER_PROOF"'"' "$manifest" \
    && rg -q '"text_shortcuts_live_ibus_runtime_render_proof"[[:space:]]*:[[:space:]]*"'"$TEXT_SHORTCUTS_LIVE_IBUS_RUNTIME_RENDER_PROOF"'"' "$manifest" \
    && rg -q '"text_shortcuts_live_ibus_runtime_render_screenshot_sha256"[[:space:]]*:[[:space:]]*"[0-9a-f]{64}"' "$manifest" \
    && rg -q '"keyboard_shortcuts_roundtrip_proof"[[:space:]]*:[[:space:]]*"'"$KEYBOARD_SHORTCUTS_ROUNDTRIP_PROOF"'"' "$manifest" \
    && rg -q '"input_sources_roundtrip_proof"[[:space:]]*:[[:space:]]*"'"$INPUT_SOURCES_ROUNDTRIP_PROOF"'"' "$manifest" \
    && rg -q '"multi_display_apply_proof"[[:space:]]*:[[:space:]]*"'"$MULTI_DISPLAY_APPLY_PROOF"'"' "$manifest" \
    && rg -q '"focus_arm_roundtrip_proof"[[:space:]]*:[[:space:]]*"'"$FOCUS_ARM_ROUNDTRIP_PROOF"'"' "$manifest" \
    && rg -q '"app_privacy_revoke_proof"[[:space:]]*:[[:space:]]*"'"$APP_PRIVACY_REVOKE_PROOF"'"' "$manifest" \
    && rg -q '"preview_open_render_proof"[[:space:]]*:[[:space:]]*"'"$PREVIEW_OPEN_RENDER_PROOF"'"' "$manifest" \
    && rg -q '"audio_output_proof"[[:space:]]*:[[:space:]]*"'"$AUDIO_OUTPUT_PROOF"'"' "$manifest" \
    && rg -q '"runtime_build_proof"[[:space:]]*:[[:space:]]*"'"$RUNTIME_BUILD_PROOF"'"' "$manifest" \
    && rg -q '"accessibility_adaptivity_proof"[[:space:]]*:[[:space:]]*"'"$ACCESSIBILITY_ADAPTIVITY_PROOF"'"' "$manifest" \
    && rg -q '"verification_iso_manifest"[[:space:]]*:[[:space:]]*"verification-iso-manifest[.]json"' "$manifest" \
    && rg -q '"verification_bib_manifest"[[:space:]]*:[[:space:]]*"verification-bib-manifest[.]json"' "$manifest" \
    && rg -q '"verification_release_evidence_manifest"[[:space:]]*:[[:space:]]*"verification-release-evidence-manifest[.]json"' "$manifest" \
    || return 1
  recorded_evidence_manifest_sha="$(awk -F'"' '/"verification_release_evidence_manifest_sha256"/ { print $4; exit }' "$manifest")"
  [[ "$recorded_evidence_manifest_sha" =~ ^[0-9a-f]{64}$ ]] || return 1
  [ "$recorded_evidence_manifest_sha" = "$(sha256_of_file "$verification_evidence_manifest")" ] || return 1
  [ "$(rg -c '"text_shortcuts_live_ibus_runtime_render_screenshot_sha256"[[:space:]]*:' "$manifest")" = "1" ] \
    || return 1
  live_proof="$run_dir/$TEXT_SHORTCUTS_LIVE_IBUS_RUNTIME_RENDER_PROOF"
  live_screenshot="$run_dir/32-text-shortcuts-live-ibus-runtime-render.png"
  recorded_manifest_screenshot_sha="$(awk -F'"' '/"text_shortcuts_live_ibus_runtime_render_screenshot_sha256"/ { print $4; exit }' "$manifest")"
  recorded_proof_screenshot_sha="$(awk -F'"' '/"screenshot_sha256"/ { print $4; exit }' "$live_proof")"
  actual_screenshot_sha="$(sha256_of_file "$live_screenshot")" || return 1
  [[ "$recorded_manifest_screenshot_sha" =~ ^[0-9a-f]{64}$ ]] \
    && [ "$recorded_manifest_screenshot_sha" = "$recorded_proof_screenshot_sha" ] \
    && [ "$recorded_manifest_screenshot_sha" = "$actual_screenshot_sha" ] \
    && screenshot_file_is_valid_png "$live_screenshot" \
    || return 1
    rg -Fq '"native_packaging_gate_proof": "'"$run_dir"'/native-packaging-gate.json"' "$manifest" \
      || return 1
    iso_sha="$(screenshot_manifest_iso_sha "$manifest" | tr '[:upper:]' '[:lower:]')"
    iso_manifest_sha="$(sha256_of_file "$verification_iso_manifest")" || return 1
    bib_manifest_sha="$(sha256_of_file "$verification_bib_manifest")" || return 1
    evidence_manifest_sha="$(sha256_of_file "$verification_evidence_manifest")" || return 1
    native_packaging_gate_proof_passes \
      "$run_dir/native-packaging-gate.json" \
      "$arch" \
      "$SELECTED_CANDIDATE_COMMIT" \
      "$image_ref" \
      "" \
      "$iso_sha" \
      "$iso_manifest_sha" \
      "$bib_manifest_sha" \
      "$evidence_manifest_sha" \
      || return 1
    native_run="$(native_packaging_gate_workflow_run "$run_dir/native-packaging-gate.json")"
    native_attempt="$(native_packaging_gate_workflow_run_attempt "$run_dir/native-packaging-gate.json")"
    github_actions_run_is_successful \
      "$native_run" \
      "$SELECTED_CANDIDATE_COMMIT" \
      "$native_attempt" \
      ".github/workflows/aarch64-verification-iso.yml" \
      || return 1
    run_date="${run_dir%/}"
    run_date="${run_date##*/}"
    artifact_name="goblins-os-aarch64-native-packaging-gate-$SELECTED_CANDIDATE_COMMIT-$run_date-attempt-$native_attempt"
    github_actions_artifact_file_matches \
      "$native_run" \
      "$artifact_name" \
      "$run_dir/native-packaging-gate.json" \
      "native-packaging-gate.json" \
      || return 1
    attestation_fields="$(aarch64_local_display_attestation_fields "$run_dir" "$image_ref")" \
      || return 1
    read -r authority_fingerprint authority_ca_fingerprint authority_iso_sha authority_screenshot_manifest_sha <<<"$attestation_fields"
    [[ "$authority_fingerprint" =~ ^[0-9a-f]{64}$ ]] || return 1
    [ "$authority_fingerprint" = "$(tr -d '[:space:]' < os/release/display-proof-authority2.sha256)" ] \
      || return 1
    [[ "$authority_ca_fingerprint" =~ ^[0-9a-f]{64}$ ]] || return 1
    [ "$authority_ca_fingerprint" = "$(tr -d '[:space:]' < os/release/display-proof-authority2-ca.sha256)" ] \
      || return 1
    [ "$authority_iso_sha" = "$iso_sha" ] || return 1
    [[ "$authority_screenshot_manifest_sha" =~ ^[0-9a-f]{64}$ ]] || return 1
  return 0
}

print_verification_and_public_release_iso_detail() {
  local run_dir="$1"
  local arch="$2"
  local manifest="$run_dir/proof-manifest.json"
  local sha_path="os/iso/output/$arch/bootiso/goblins-os-$arch.iso.sha256"
  local manifest_sha release_sha proof_commit

  manifest_sha="$(awk -F'"' '/"iso_sha256"/ { print $4; exit }' "$manifest" 2>/dev/null || true)"
  release_sha="$(awk '{ print $1; exit }' "$sha_path" 2>/dev/null || true)"
  proof_commit="$(manifest_candidate_commit "$manifest")"
  echo "[INFO] $arch candidate/source commit: ${proof_commit:-missing}"
  echo "[INFO] $arch verification proof ISO SHA256: ${manifest_sha:-missing}"
  echo "[INFO] $arch hydrated public release ISO SHA256: ${release_sha:-missing}"
  echo "[INFO] $arch automated screenshots intentionally use verification-only media; public release ISO artifacts are checked separately"
}

firewall_live_toggle_proof_passes() {
  local proof="$1"

  proof_json_passes "$proof" || return 1
  [ -s "$proof" ] \
    && rg -q '"status"[[:space:]]*:[[:space:]]*"pass"' "$proof" \
    && rg -q '"route"[[:space:]]*:[[:space:]]*"/v1/firewall/enabled"' "$proof" \
    && rg -q '"status_route"[[:space:]]*:[[:space:]]*"/v1/firewall/status"' "$proof" \
    && rg -q '"disable_http"[[:space:]]*:[[:space:]]*"200"' "$proof" \
    && rg -q '"disable_ok"[[:space:]]*:[[:space:]]*"true"' "$proof" \
    && rg -q '"disable_enabled"[[:space:]]*:[[:space:]]*"false"' "$proof" \
    && rg -q '"disable_active"[[:space:]]*:[[:space:]]*"false"' "$proof" \
    && rg -q '"enable_http"[[:space:]]*:[[:space:]]*"200"' "$proof" \
    && rg -q '"enable_ok"[[:space:]]*:[[:space:]]*"true"' "$proof" \
    && rg -q '"enable_enabled"[[:space:]]*:[[:space:]]*"true"' "$proof" \
    && rg -q '"enable_active"[[:space:]]*:[[:space:]]*"true"' "$proof" \
    && rg -q '"unit_template"[[:space:]]*:[[:space:]]*"goblins-os-firewall@\.service"' "$proof" \
    && rg -q '"polkit_rule"[[:space:]]*:[[:space:]]*"60-goblins-os-firewall.rules"' "$proof"
}

text_shortcuts_session_enable_proof_passes() {
  local proof="$1"

  proof_json_passes "$proof" || return 1
  [ -s "$proof" ] \
    && rg -q '"status"[[:space:]]*:[[:space:]]*"pass"' "$proof" \
    && rg -q '"route"[[:space:]]*:[[:space:]]*"/v1/text-shortcuts"' "$proof" \
    && rg -q '"proof_scope"[[:space:]]*:[[:space:]]*"session-plumbing"' "$proof" \
    && rg -q '"service"[[:space:]]*:[[:space:]]*"active"' "$proof" \
    && rg -q '"service_unit"[[:space:]]*:[[:space:]]*"org.freedesktop.IBus.session.GNOME.service"' "$proof" \
    && rg -q '"input_source_configured"[[:space:]]*:[[:space:]]*"true"' "$proof" \
    && rg -q '"preload_configured"[[:space:]]*:[[:space:]]*"true"' "$proof" \
    && rg -q '"engine_listed"[[:space:]]*:[[:space:]]*"true"' "$proof" \
    && rg -q '"active_engine"[[:space:]]*:[[:space:]]*"goblins-textshortcuts"' "$proof" \
    && rg -q '"adapter_self_test"[[:space:]]*:[[:space:]]*"pass"' "$proof" \
    && rg -q '"core_http"[[:space:]]*:[[:space:]]*"200"' "$proof" \
    && rg -q '"core_ibus_available"[[:space:]]*:[[:space:]]*"true"' "$proof" \
    && rg -q '"core_component_registered"[[:space:]]*:[[:space:]]*"true"' "$proof" \
    && rg -q '"core_engine_binary_available"[[:space:]]*:[[:space:]]*"true"' "$proof" \
    && rg -q '"core_input_source_configured"[[:space:]]*:[[:space:]]*"true"' "$proof" \
    && rg -q '"runtime_ready_claim"[[:space:]]*:[[:space:]]*"false"' "$proof" \
    && { { rg -q '"core_engine_available"[[:space:]]*:[[:space:]]*"true"' "$proof" \
           && rg -q '"core_runtime_loop_available"[[:space:]]*:[[:space:]]*"true"' "$proof"; } \
         || { rg -q '"core_engine_available"[[:space:]]*:[[:space:]]*"false"' "$proof" \
              && rg -q '"core_runtime_loop_available"[[:space:]]*:[[:space:]]*"false"' "$proof"; }; }
}

text_shortcuts_candidate_metadata_proof_passes() {
  local proof="$1"

  proof_json_passes "$proof" || return 1
  [ -s "$proof" ] \
    && rg -q '"status"[[:space:]]*:[[:space:]]*"pass"' "$proof" \
    && rg -q '"route"[[:space:]]*:[[:space:]]*"/v1/text-shortcuts"' "$proof" \
    && rg -q '"surface"[[:space:]]*:[[:space:]]*"goblins-os-shell-text-shortcuts-candidate-proof"' "$proof" \
    && rg -q '"candidate_replacement"[[:space:]]*:[[:space:]]*"on my way"' "$proof" \
    && rg -q '"candidate_accept_on"[[:space:]]*:[[:space:]]*"word-boundary"' "$proof" \
    && rg -q '"candidate_dismiss_key"[[:space:]]*:[[:space:]]*"Escape"' "$proof" \
    && rg -q '"rendered_bubble_ready_claim"[[:space:]]*:[[:space:]]*"false"' "$proof" \
    && rg -q '"live_overlay_claim"[[:space:]]*:[[:space:]]*"false"' "$proof" \
    && rg -q '"runtime_ready_claim"[[:space:]]*:[[:space:]]*"false"' "$proof"
}

text_shortcuts_overlay_intent_proof_passes() {
  local proof="$1"

  proof_json_passes "$proof" || return 1
  [ -s "$proof" ] \
    && rg -q '"status"[[:space:]]*:[[:space:]]*"pass"' "$proof" \
    && rg -q '"route"[[:space:]]*:[[:space:]]*"/v1/text-shortcuts"' "$proof" \
    && rg -q '"surface"[[:space:]]*:[[:space:]]*"goblins-textshortcuts-ibus-adapter-overlay-intent"' "$proof" \
    && rg -q '"adapter_self_test"[[:space:]]*:[[:space:]]*"pass"' "$proof" \
    && rg -q '"show_count"[[:space:]]*:[[:space:]]*"2"' "$proof" \
    && rg -q '"hide_count"[[:space:]]*:[[:space:]]*"2"' "$proof" \
    && rg -q '"dismissed_reason"[[:space:]]*:[[:space:]]*"true"' "$proof" \
    && rg -q '"committed_reason"[[:space:]]*:[[:space:]]*"true"' "$proof" \
    && rg -q '"rendered_bubble_ready_claim"[[:space:]]*:[[:space:]]*"false"' "$proof" \
    && rg -q '"live_overlay_claim"[[:space:]]*:[[:space:]]*"false"' "$proof" \
    && rg -q '"runtime_ready_claim"[[:space:]]*:[[:space:]]*"false"' "$proof"
}

text_shortcuts_candidate_bubble_frame_proof_passes() {
  local proof="$1"

  proof_json_passes "$proof" || return 1
  [ -s "$proof" ] \
    && rg -q '"status"[[:space:]]*:[[:space:]]*"pass"' "$proof" \
    && rg -q '"route"[[:space:]]*:[[:space:]]*"/v1/text-shortcuts"' "$proof" \
    && rg -q '"surface"[[:space:]]*:[[:space:]]*"goblins-textshortcuts-accept-bubble-frame"' "$proof" \
    && rg -q '"adapter_self_test"[[:space:]]*:[[:space:]]*"pass"' "$proof" \
    && rg -q '"show_frame_count"[[:space:]]*:[[:space:]]*"2"' "$proof" \
    && rg -q '"hide_frame_count"[[:space:]]*:[[:space:]]*"2"' "$proof" \
    && rg -q '"dismissed_frame"[[:space:]]*:[[:space:]]*"true"' "$proof" \
    && rg -q '"committed_frame"[[:space:]]*:[[:space:]]*"true"' "$proof" \
    && rg -q '"replacement"[[:space:]]*:[[:space:]]*"on my way"' "$proof" \
    && rg -q '"accept_on"[[:space:]]*:[[:space:]]*"word-boundary"' "$proof" \
    && rg -q '"accept_keys"[[:space:]]*:[[:space:]]*"Space,Return"' "$proof" \
    && rg -q '"dismiss_key"[[:space:]]*:[[:space:]]*"Escape"' "$proof" \
    && rg -q '"style_class"[[:space:]]*:[[:space:]]*"gos-text-shortcuts-candidate"' "$proof" \
    && rg -q '"text_style_class"[[:space:]]*:[[:space:]]*"gos-text-shortcuts-candidate-text"' "$proof" \
    && rg -q '"hint_style_class"[[:space:]]*:[[:space:]]*"gos-text-shortcuts-candidate-hint"' "$proof" \
    && rg -q '"font_family"[[:space:]]*:[[:space:]]*"Inter"' "$proof" \
    && rg -q '"sensitive_field_refusal"[[:space:]]*:[[:space:]]*"true"' "$proof" \
    && rg -q '"rendered_bubble_ready_claim"[[:space:]]*:[[:space:]]*"false"' "$proof" \
    && rg -q '"live_overlay_claim"[[:space:]]*:[[:space:]]*"false"' "$proof" \
    && rg -q '"runtime_ready_claim"[[:space:]]*:[[:space:]]*"false"' "$proof"
}

text_shortcuts_candidate_bubble_layout_proof_passes() {
  local proof="$1"

  proof_json_passes "$proof" || return 1
  [ -s "$proof" ] \
    && rg -q '"status"[[:space:]]*:[[:space:]]*"pass"' "$proof" \
    && rg -q '"route"[[:space:]]*:[[:space:]]*"/v1/text-shortcuts"' "$proof" \
    && rg -q '"surface"[[:space:]]*:[[:space:]]*"goblins-textshortcuts-accept-bubble-layout"' "$proof" \
    && rg -q '"adapter_self_test"[[:space:]]*:[[:space:]]*"pass"' "$proof" \
    && rg -q '"frame_surface"[[:space:]]*:[[:space:]]*"goblins-textshortcuts-accept-bubble-frame"' "$proof" \
    && rg -q '"layout_count"[[:space:]]*:[[:space:]]*"4"' "$proof" \
    && rg -q '"visible_layout_count"[[:space:]]*:[[:space:]]*"3"' "$proof" \
    && rg -q '"right_edge_clamped"[[:space:]]*:[[:space:]]*"true"' "$proof" \
    && rg -q '"bottom_edge_flipped"[[:space:]]*:[[:space:]]*"true"' "$proof" \
    && rg -q '"hidden_frame_collapses"[[:space:]]*:[[:space:]]*"true"' "$proof" \
    && rg -q '"style_class"[[:space:]]*:[[:space:]]*"gos-text-shortcuts-candidate"' "$proof" \
    && rg -q '"font_family"[[:space:]]*:[[:space:]]*"Inter"' "$proof" \
    && rg -q '"rendered_bubble_ready_claim"[[:space:]]*:[[:space:]]*"false"' "$proof" \
    && rg -q '"live_overlay_claim"[[:space:]]*:[[:space:]]*"false"' "$proof" \
    && rg -q '"runtime_ready_claim"[[:space:]]*:[[:space:]]*"false"' "$proof"
}

text_shortcuts_candidate_bubble_render_intent_proof_passes() {
  local proof="$1"

  proof_json_passes "$proof" || return 1
  [ -s "$proof" ] \
    && rg -q '"status"[[:space:]]*:[[:space:]]*"pass"' "$proof" \
    && rg -q '"route"[[:space:]]*:[[:space:]]*"/v1/text-shortcuts"' "$proof" \
    && rg -q '"surface"[[:space:]]*:[[:space:]]*"goblins-textshortcuts-accept-bubble-render-intent"' "$proof" \
    && rg -q '"adapter_self_test"[[:space:]]*:[[:space:]]*"pass"' "$proof" \
    && rg -q '"frame_surface"[[:space:]]*:[[:space:]]*"goblins-textshortcuts-accept-bubble-frame"' "$proof" \
    && rg -q '"layout_surface"[[:space:]]*:[[:space:]]*"goblins-textshortcuts-accept-bubble-layout"' "$proof" \
    && rg -q '"render_intent_count"[[:space:]]*:[[:space:]]*"8"' "$proof" \
    && rg -q '"show_intent_count"[[:space:]]*:[[:space:]]*"4"' "$proof" \
    && rg -q '"hide_intent_count"[[:space:]]*:[[:space:]]*"4"' "$proof" \
    && rg -q '"dismissed_intent"[[:space:]]*:[[:space:]]*"true"' "$proof" \
    && rg -q '"committed_intent"[[:space:]]*:[[:space:]]*"true"' "$proof" \
    && rg -q '"focus_out_hide"[[:space:]]*:[[:space:]]*"true"' "$proof" \
    && rg -q '"sensitive_hide"[[:space:]]*:[[:space:]]*"true"' "$proof" \
    && rg -q '"pass_through_unchanged"[[:space:]]*:[[:space:]]*"true"' "$proof" \
    && rg -q '"key_release_preserved_candidate"[[:space:]]*:[[:space:]]*"true"' "$proof" \
    && rg -q '"runtime_failure_cleanup"[[:space:]]*:[[:space:]]*"true"' "$proof" \
    && rg -q '"sink_failure_fail_open"[[:space:]]*:[[:space:]]*"true"' "$proof" \
    && rg -q '"style_class"[[:space:]]*:[[:space:]]*"gos-text-shortcuts-candidate"' "$proof" \
    && rg -q '"font_family"[[:space:]]*:[[:space:]]*"Inter"' "$proof" \
    && rg -q '"rendered_bubble_ready_claim"[[:space:]]*:[[:space:]]*"false"' "$proof" \
    && rg -q '"live_overlay_claim"[[:space:]]*:[[:space:]]*"false"' "$proof" \
	    && rg -q '"runtime_ready_claim"[[:space:]]*:[[:space:]]*"false"' "$proof"
}

text_shortcuts_candidate_bubble_render_proof_passes() {
  local proof="$1"

  proof_json_passes "$proof" || return 1
  [ -s "$proof" ] \
    && rg -q '"status"[[:space:]]*:[[:space:]]*"pass"' "$proof" \
    && rg -q '"route"[[:space:]]*:[[:space:]]*"/v1/text-shortcuts"' "$proof" \
    && rg -q '"surface"[[:space:]]*:[[:space:]]*"goblins-os-shell-text-shortcuts-candidate-bubble-render"' "$proof" \
    && rg -q '"render_intent_surface"[[:space:]]*:[[:space:]]*"goblins-textshortcuts-accept-bubble-render-intent"' "$proof" \
    && rg -q '"layout_surface"[[:space:]]*:[[:space:]]*"goblins-textshortcuts-accept-bubble-layout"' "$proof" \
    && rg -q '"frame_surface"[[:space:]]*:[[:space:]]*"goblins-textshortcuts-accept-bubble-frame"' "$proof" \
    && rg -q '"replacement"[[:space:]]*:[[:space:]]*"on my way"' "$proof" \
    && rg -q '"accept_on"[[:space:]]*:[[:space:]]*"word-boundary"' "$proof" \
    && rg -q '"dismiss_key"[[:space:]]*:[[:space:]]*"Escape"' "$proof" \
    && rg -q '"style_class"[[:space:]]*:[[:space:]]*"gos-text-shortcuts-candidate"' "$proof" \
    && rg -q '"text_style_class"[[:space:]]*:[[:space:]]*"gos-text-shortcuts-candidate-text"' "$proof" \
    && rg -q '"hint_style_class"[[:space:]]*:[[:space:]]*"gos-text-shortcuts-candidate-hint"' "$proof" \
    && rg -q '"font_family"[[:space:]]*:[[:space:]]*"Inter"' "$proof" \
    && rg -q '"screenshot"[[:space:]]*:[[:space:]]*"31-text-shortcuts-candidate-bubble-render\.png"' "$proof" \
    && rg -q '"rendered_candidate_surface"[[:space:]]*:[[:space:]]*"true"' "$proof" \
    && rg -q '"rendered_bubble_ready_claim"[[:space:]]*:[[:space:]]*"false"' "$proof" \
    && rg -q '"live_overlay_claim"[[:space:]]*:[[:space:]]*"false"' "$proof" \
    && rg -q '"runtime_ready_claim"[[:space:]]*:[[:space:]]*"false"' "$proof"
}

text_shortcuts_live_ibus_runtime_render_proof_passes() {
  local proof="$1"
  local screenshot recorded_sha actual_sha

  proof_json_passes "$proof" || return 1
  [ -s "$proof" ] \
    && rg -q '"status"[[:space:]]*:[[:space:]]*"pass"' "$proof" \
    && rg -q '"route"[[:space:]]*:[[:space:]]*"/v1/text-shortcuts"' "$proof" \
    && rg -q '"preview_route"[[:space:]]*:[[:space:]]*"/v1/text-shortcuts/preview"' "$proof" \
    && rg -q '"surface"[[:space:]]*:[[:space:]]*"goblins-textshortcuts-live-ibus-runtime-render"' "$proof" \
    && rg -q '"input_driver"[[:space:]]*:[[:space:]]*"qmp-keyboard"' "$proof" \
    && rg -q '"active_engine"[[:space:]]*:[[:space:]]*"goblins-textshortcuts"' "$proof" \
    && rg -q '"core_write_http"[[:space:]]*:[[:space:]]*"200"' "$proof" \
    && rg -q '"core_read_http"[[:space:]]*:[[:space:]]*"200"' "$proof" \
    && rg -q '"core_preview_http"[[:space:]]*:[[:space:]]*"200"' "$proof" \
    && rg -q '"file_contract_http"[[:space:]]*:[[:space:]]*"200"' "$proof" \
    && rg -q '"seed_write_http"[[:space:]]*:[[:space:]]*"200"' "$proof" \
    && rg -q '"seed_read_http"[[:space:]]*:[[:space:]]*"200"' "$proof" \
    && rg -q '"seed_roundtrip"[[:space:]]*:[[:space:]]*"true"' "$proof" \
    && rg -q '"seed_loaded"[[:space:]]*:[[:space:]]*"true"' "$proof" \
    && rg -q '"core_table_roundtrip"[[:space:]]*:[[:space:]]*"true"' "$proof" \
    && rg -q '"core_preview_roundtrip"[[:space:]]*:[[:space:]]*"true"' "$proof" \
    && rg -q '"desktop_file_contract"[[:space:]]*:[[:space:]]*"true"' "$proof" \
    && rg -q '"desktop_parent_contract"[[:space:]]*:[[:space:]]*"true"' "$proof" \
    && rg -q '"desktop_file_owner_mode"[[:space:]]*:[[:space:]]*"true"' "$proof" \
    && rg -q '"desktop_file_single_link"[[:space:]]*:[[:space:]]*"true"' "$proof" \
    && rg -q '"desktop_file_size_bounded"[[:space:]]*:[[:space:]]*"true"' "$proof" \
    && rg -q '"desktop_file_bounded_read"[[:space:]]*:[[:space:]]*"true"' "$proof" \
    && rg -q '"legacy_service_table_absent"[[:space:]]*:[[:space:]]*"true"' "$proof" \
    && rg -q '"live_watcher_reload"[[:space:]]*:[[:space:]]*"true"' "$proof" \
    && rg -q '"post_keystroke_read_http"[[:space:]]*:[[:space:]]*"200"' "$proof" \
    && rg -q '"post_keystroke_file_http"[[:space:]]*:[[:space:]]*"200"' "$proof" \
    && rg -q '"post_keystroke_roundtrip"[[:space:]]*:[[:space:]]*"true"' "$proof" \
    && rg -q '"normal_actual"[[:space:]]*:[[:space:]]*"on my way\."' "$proof" \
    && rg -q '"passthrough_actual"[[:space:]]*:[[:space:]]*"hello\."' "$proof" \
    && rg -q '"password_refusal"[[:space:]]*:[[:space:]]*"true"' "$proof" \
    && rg -q '"password_sensitive_purpose"[[:space:]]*:[[:space:]]*"true"' "$proof" \
    && rg -q '"password_process_key_callback"[[:space:]]*:[[:space:]]*"true"' "$proof" \
    && rg -q '"password_commit_absent"[[:space:]]*:[[:space:]]*"true"' "$proof" \
    && rg -q '"password_candidate_absent"[[:space:]]*:[[:space:]]*"true"' "$proof" \
    && rg -q '"password_popup_absent"[[:space:]]*:[[:space:]]*"true"' "$proof" \
    && rg -q '"normal_stage_ledger_scoped"[[:space:]]*:[[:space:]]*"true"' "$proof" \
    && rg -q '"focused_field_callback"[[:space:]]*:[[:space:]]*"true"' "$proof" \
    && rg -q '"process_key_event_callback"[[:space:]]*:[[:space:]]*"true"' "$proof" \
    && rg -q '"cursor_location_callback"[[:space:]]*:[[:space:]]*"true"' "$proof" \
    && rg -q '"pre_boundary_commit_absent"[[:space:]]*:[[:space:]]*"true"' "$proof" \
    && rg -q '"boundary_stage_ledger_scoped"[[:space:]]*:[[:space:]]*"true"' "$proof" \
    && rg -q '"boundary_stage_commit_count"[[:space:]]*:[[:space:]]*"1"' "$proof" \
    && rg -q '"normal_stage_commit"[[:space:]]*:[[:space:]]*"true"' "$proof" \
    && rg -q '"ibus_commit_operation"[[:space:]]*:[[:space:]]*"true"' "$proof" \
    && rg -q '"focused_entry_readback"[[:space:]]*:[[:space:]]*"true"' "$proof" \
    && rg -q '"ibus_commit_delivered"[[:space:]]*:[[:space:]]*"true"' "$proof" \
    && rg -q '"boundary_popup_action"[[:space:]]*:[[:space:]]*"hide-candidate"' "$proof" \
    && rg -q '"boundary_popup_reason"[[:space:]]*:[[:space:]]*"committed"' "$proof" \
    && rg -q '"candidate_intent_seen"[[:space:]]*:[[:space:]]*"true"' "$proof" \
    && rg -q '"native_ibus_candidate_published"[[:space:]]*:[[:space:]]*"true"' "$proof" \
    && rg -q '"native_popup_generation"[[:space:]]*:[[:space:]]*"[1-9][0-9]*"' "$proof" \
    && rg -q '"native_popup_record_ordinal"[[:space:]]*:[[:space:]]*"[1-9][0-9]*"' "$proof" \
    && rg -q '"native_popup_generation_current"[[:space:]]*:[[:space:]]*"true"' "$proof" \
    && rg -q '"native_popup_record_current_at_capture"[[:space:]]*:[[:space:]]*"true"' "$proof" \
    && rg -q '"native_popup_action"[[:space:]]*:[[:space:]]*"show-candidate"' "$proof" \
    && rg -q '"native_popup_has_cursor_rect"[[:space:]]*:[[:space:]]*"true"' "$proof" \
    && rg -q '"native_popup_expected_replacement"[[:space:]]*:[[:space:]]*"true"' "$proof" \
    && rg -q '"native_popup_hint_published"[[:space:]]*:[[:space:]]*"true"' "$proof" \
    && rg -q '"renderer"[[:space:]]*:[[:space:]]*"native-ibus-lookup-table"' "$proof" \
    && rg -q '"cursor_anchor"[[:space:]]*:[[:space:]]*"ibus-input-context"' "$proof" \
    && rg -q '"synthetic_overlay"[[:space:]]*:[[:space:]]*"false"' "$proof" \
    && rg -q '"screenshot"[[:space:]]*:[[:space:]]*"32-text-shortcuts-live-ibus-runtime-render\.png"' "$proof" \
    && rg -q '"screenshot_sha256"[[:space:]]*:[[:space:]]*"[0-9a-f]{64}"' "$proof" \
    && rg -q '"screenshot_capture_ack"[[:space:]]*:[[:space:]]*"true"' "$proof" \
    && rg -q '"native_candidate_popup_ready_claim"[[:space:]]*:[[:space:]]*"true"' "$proof" \
    && rg -q '"live_overlay_claim"[[:space:]]*:[[:space:]]*"true"' "$proof" \
    && rg -q '"runtime_ready_claim"[[:space:]]*:[[:space:]]*"true"' "$proof" \
    && rg -q '"core_readiness_flip"[[:space:]]*:[[:space:]]*"live"' "$proof" \
    || return 1

  [ "$(rg -c '"screenshot_sha256"[[:space:]]*:' "$proof")" = "1" ] || return 1
  screenshot="$(dirname "$proof")/32-text-shortcuts-live-ibus-runtime-render.png"
  recorded_sha="$(awk -F'"' '/"screenshot_sha256"/ { print $4; exit }' "$proof")"
  actual_sha="$(sha256_of_file "$screenshot")" || return 1
  [[ "$recorded_sha" =~ ^[0-9a-f]{64}$ ]] \
    && [ "$recorded_sha" = "$actual_sha" ] \
    && screenshot_file_is_valid_png "$screenshot"
}

keyboard_shortcuts_roundtrip_proof_passes() {
  local proof="$1"

  proof_json_passes "$proof" || return 1
  [ -s "$proof" ] \
    && rg -q '"status"[[:space:]]*:[[:space:]]*"pass"' "$proof" \
    && rg -q '"shortcut_route"[[:space:]]*:[[:space:]]*"/v1/keyboard/shortcuts/binding"' "$proof" \
    && rg -q '"modifier_route"[[:space:]]*:[[:space:]]*"/v1/keyboard/modifier-remap"' "$proof" \
    && rg -q '"shortcut_action"[[:space:]]*:[[:space:]]*"window-hud"' "$proof" \
    && rg -q '"shortcut_binding"[[:space:]]*:[[:space:]]*"<Super><Shift>H"' "$proof" \
    && rg -q '"shortcut_http"[[:space:]]*:[[:space:]]*"200"' "$proof" \
    && rg -q '"shortcut_gsettings_readback"[[:space:]]*:[[:space:]]*"true"' "$proof" \
    && rg -q '"shortcut_reset_http"[[:space:]]*:[[:space:]]*"200"' "$proof" \
    && rg -q '"shortcut_reset_binding"[[:space:]]*:[[:space:]]*"<Super>w"' "$proof" \
    && rg -q '"modifier_target"[[:space:]]*:[[:space:]]*"caps-lock"' "$proof" \
    && rg -q '"modifier_value"[[:space:]]*:[[:space:]]*"control"' "$proof" \
    && rg -q '"modifier_http"[[:space:]]*:[[:space:]]*"200"' "$proof" \
    && rg -q '"modifier_gsettings_readback"[[:space:]]*:[[:space:]]*"ctrl:nocaps"' "$proof" \
    && rg -q '"modifier_reset_http"[[:space:]]*:[[:space:]]*"200"' "$proof" \
    && rg -q '"modifier_restore"[[:space:]]*:[[:space:]]*"default"' "$proof" \
    && rg -q '"roundtrip_restored"[[:space:]]*:[[:space:]]*"true"' "$proof"
}

input_sources_roundtrip_proof_passes() {
  local proof="$1"

  proof_json_passes "$proof" || return 1
  [ -s "$proof" ] \
    && rg -q '"status"[[:space:]]*:[[:space:]]*"pass"' "$proof" \
    && rg -q '"source_route"[[:space:]]*:[[:space:]]*"/v1/input/sources"' "$proof" \
    && rg -q '"switch_route"[[:space:]]*:[[:space:]]*"/v1/input/switch-next"' "$proof" \
    && rg -q '"test_sources"[[:space:]]*:[[:space:]]*"xkb-us,xkb-gb"' "$proof" \
    && rg -q '"set_http"[[:space:]]*:[[:space:]]*"200"' "$proof" \
    && rg -q '"set_ok"[[:space:]]*:[[:space:]]*"true"' "$proof" \
    && rg -q '"sources_gsettings_readback"[[:space:]]*:[[:space:]]*"true"' "$proof" \
    && rg -q '"current_before_switch"[[:space:]]*:[[:space:]]*"0"' "$proof" \
    && rg -q '"switch_http"[[:space:]]*:[[:space:]]*"200"' "$proof" \
    && rg -q '"switch_ok"[[:space:]]*:[[:space:]]*"true"' "$proof" \
    && rg -q '"switch_switched"[[:space:]]*:[[:space:]]*"true"' "$proof" \
    && rg -q '"current_after_switch"[[:space:]]*:[[:space:]]*"1"' "$proof" \
    && rg -q '"restore_sources"[[:space:]]*:[[:space:]]*"true"' "$proof" \
    && rg -q '"restore_current"[[:space:]]*:[[:space:]]*"true"' "$proof" \
    && rg -q '"roundtrip_restored"[[:space:]]*:[[:space:]]*"true"' "$proof"
}

multi_display_apply_proof_passes() {
  local proof="$1"

  proof_json_passes "$proof" || return 1
  [ -s "$proof" ] \
    && rg -q '"status"[[:space:]]*:[[:space:]]*"pass"' "$proof" \
    && rg -q '"status_route"[[:space:]]*:[[:space:]]*"/v1/displays/status"' "$proof" \
    && rg -q '"apply_route"[[:space:]]*:[[:space:]]*"/v1/displays/apply"' "$proof" \
    && rg -q '"display_config"[[:space:]]*:[[:space:]]*"org.gnome.Mutter.DisplayConfig"' "$proof" \
    && rg -q '"connector"[[:space:]]*:[[:space:]]*"[^"]+"' "$proof" \
    && rg -q '"mode_id"[[:space:]]*:[[:space:]]*"[^"]+"' "$proof" \
    && rg -q '"serial"[[:space:]]*:[[:space:]]*"[0-9]+"' "$proof" \
    && rg -q '"verify_http"[[:space:]]*:[[:space:]]*"200"' "$proof" \
    && rg -q '"verify_ok"[[:space:]]*:[[:space:]]*"true"' "$proof" \
    && rg -q '"temporary_http"[[:space:]]*:[[:space:]]*"200"' "$proof" \
    && rg -q '"temporary_ok"[[:space:]]*:[[:space:]]*"true"' "$proof" \
    && rg -q '"persistent_guard_http"[[:space:]]*:[[:space:]]*"400"' "$proof" \
    && rg -q '"persistent_confirmation_required"[[:space:]]*:[[:space:]]*"true"' "$proof" \
    && rg -q '"stale_serial"[[:space:]]*:[[:space:]]*"[0-9]+"' "$proof" \
    && rg -q '"stale_serial_http"[[:space:]]*:[[:space:]]*"409"' "$proof" \
    && rg -q '"stale_serial_rejected"[[:space:]]*:[[:space:]]*"true"' "$proof" \
    && rg -q '"roundtrip_restored"[[:space:]]*:[[:space:]]*"true"' "$proof" \
    && rg -q '"persistent_keep_claim"[[:space:]]*:[[:space:]]*"false"' "$proof" \
    && rg -q '"same_layout_noop"[[:space:]]*:[[:space:]]*"true"' "$proof"
}

focus_arm_roundtrip_proof_passes() {
  local proof="$1"

  proof_json_passes "$proof" || return 1
  [ -s "$proof" ] \
    && rg -q '"status"[[:space:]]*:[[:space:]]*"pass"' "$proof" \
    && rg -q '"status_route"[[:space:]]*:[[:space:]]*"/v1/focus/status"' "$proof" \
    && rg -q '"activate_route"[[:space:]]*:[[:space:]]*"/v1/focus/activate"' "$proof" \
    && rg -q '"deactivate_route"[[:space:]]*:[[:space:]]*"/v1/focus/deactivate"' "$proof" \
    && rg -q '"test_mode"[[:space:]]*:[[:space:]]*"gate-work"' "$proof" \
    && rg -q '"test_mode_configured"[[:space:]]*:[[:space:]]*"true"' "$proof" \
    && rg -q '"activate_http"[[:space:]]*:[[:space:]]*"200"' "$proof" \
    && rg -q '"activate_ok"[[:space:]]*:[[:space:]]*"true"' "$proof" \
    && rg -q '"activate_active_mode"[[:space:]]*:[[:space:]]*"gate-work"' "$proof" \
    && rg -q '"active_mode_gsettings_readback"[[:space:]]*:[[:space:]]*"gate-work"' "$proof" \
    && rg -q '"armed_by_schedule_after_activate"[[:space:]]*:[[:space:]]*"false"' "$proof" \
    && rg -q '"restore_banners_after_activate"[[:space:]]*:[[:space:]]*"true"' "$proof" \
    && rg -q '"notification_banners_after_activate"[[:space:]]*:[[:space:]]*"false"' "$proof" \
    && rg -q '"deactivate_http"[[:space:]]*:[[:space:]]*"200"' "$proof" \
    && rg -q '"deactivate_ok"[[:space:]]*:[[:space:]]*"true"' "$proof" \
    && rg -q '"deactivate_active_mode"[[:space:]]*:[[:space:]]*""' "$proof" \
    && rg -q '"active_mode_after_deactivate"[[:space:]]*:[[:space:]]*""' "$proof" \
    && rg -q '"armed_by_schedule_after_deactivate"[[:space:]]*:[[:space:]]*"false"' "$proof" \
    && rg -q '"restore_banners_after_deactivate"[[:space:]]*:[[:space:]]*""' "$proof" \
    && rg -q '"notification_banners_after_deactivate"[[:space:]]*:[[:space:]]*"true"' "$proof" \
    && rg -q '"original_focus_state_restored"[[:space:]]*:[[:space:]]*"true"' "$proof" \
    && rg -q '"original_notification_banners_restored"[[:space:]]*:[[:space:]]*"true"' "$proof" \
    && rg -q '"roundtrip_restored"[[:space:]]*:[[:space:]]*"true"' "$proof" \
    && rg -q '"mode_crud_claim"[[:space:]]*:[[:space:]]*"false"' "$proof" \
    && rg -q '"schedule_claim"[[:space:]]*:[[:space:]]*"false"' "$proof" \
    && rg -q '"per_app_breakthroughs_claim"[[:space:]]*:[[:space:]]*"false"' "$proof"
}

app_privacy_revoke_proof_passes() {
  local proof="$1"

  proof_json_passes "$proof" || return 1
  [ -s "$proof" ] \
    && rg -q '"status"[[:space:]]*:[[:space:]]*"pass"' "$proof" \
    && rg -q '"route"[[:space:]]*:[[:space:]]*"/v1/app-privacy/revoke"' "$proof" \
    && rg -q '"table"[[:space:]]*:[[:space:]]*"location"' "$proof" \
    && rg -q '"app"[[:space:]]*:[[:space:]]*"org.goblins.GatePrivacyProof"' "$proof" \
    && rg -q '"seed_method"[[:space:]]*:[[:space:]]*"PermissionStore.SetPermission"' "$proof" \
    && rg -q '"revoke_method"[[:space:]]*:[[:space:]]*"PermissionStore.DeletePermission"' "$proof" \
    && rg -q '"readback_method"[[:space:]]*:[[:space:]]*"PermissionStore.GetPermission"' "$proof" \
    && rg -q '"seed_grant"[[:space:]]*:[[:space:]]*"yes"' "$proof" \
    && rg -q '"seed_readback"[[:space:]]*:[[:space:]]*"true"' "$proof" \
    && rg -q '"revoke_http"[[:space:]]*:[[:space:]]*"200"' "$proof" \
    && rg -q '"revoke_ok"[[:space:]]*:[[:space:]]*"true"' "$proof" \
    && rg -q '"post_revoke_absent"[[:space:]]*:[[:space:]]*"true"' "$proof" \
    && rg -q '"restore_prior_state"[[:space:]]*:[[:space:]]*"true"' "$proof" \
    && rg -q '"roundtrip_restored"[[:space:]]*:[[:space:]]*"true"' "$proof" \
    && rg -q '"resource_keyed_claim"[[:space:]]*:[[:space:]]*"false"' "$proof" \
    && rg -q '"device_revoke_claim"[[:space:]]*:[[:space:]]*"false"' "$proof"
}

preview_open_render_proof_passes() {
  local proof="$1"

  proof_json_passes "$proof" || return 1
  [ -s "$proof" ] \
    && rg -q '"status"[[:space:]]*:[[:space:]]*"pass"' "$proof" \
    && rg -q '"status_route"[[:space:]]*:[[:space:]]*"/v1/preview/status"' "$proof" \
    && rg -q '"route"[[:space:]]*:[[:space:]]*"/v1/preview/open"' "$proof" \
    && rg -q '"status_http"[[:space:]]*:[[:space:]]*"200"' "$proof" \
    && rg -q '"available"[[:space:]]*:[[:space:]]*"true"' "$proof" \
    && rg -q '"xdg_open"[[:space:]]*:[[:space:]]*"true"' "$proof" \
    && rg -q '"papers"[[:space:]]*:[[:space:]]*"true"' "$proof" \
    && rg -q '"loupe"[[:space:]]*:[[:space:]]*"true"' "$proof" \
    && rg -q '"pdf_default"[[:space:]]*:[[:space:]]*"org.gnome.Papers.desktop"' "$proof" \
    && rg -q '"image_default"[[:space:]]*:[[:space:]]*"org.gnome.Loupe.desktop"' "$proof" \
    && rg -q '"jpeg_default"[[:space:]]*:[[:space:]]*"org.gnome.Loupe.desktop"' "$proof" \
    && rg -q '"pdf_http"[[:space:]]*:[[:space:]]*"200"' "$proof" \
    && rg -q '"pdf_ok"[[:space:]]*:[[:space:]]*"true"' "$proof" \
    && rg -q '"pdf_kind"[[:space:]]*:[[:space:]]*"pdf"' "$proof" \
    && rg -q '"pdf_process"[[:space:]]*:[[:space:]]*"papers"' "$proof" \
    && rg -q '"pdf_screenshot"[[:space:]]*:[[:space:]]*"29-preview-pdf-open\.png"' "$proof" \
    && rg -q '"rendered_pdf_frame"[[:space:]]*:[[:space:]]*"true"' "$proof" \
    && rg -q '"image_http"[[:space:]]*:[[:space:]]*"200"' "$proof" \
    && rg -q '"image_ok"[[:space:]]*:[[:space:]]*"true"' "$proof" \
    && rg -q '"image_kind"[[:space:]]*:[[:space:]]*"image"' "$proof" \
    && rg -q '"image_process"[[:space:]]*:[[:space:]]*"loupe"' "$proof" \
    && rg -q '"image_screenshot"[[:space:]]*:[[:space:]]*"30-preview-image-open\.png"' "$proof" \
    && rg -q '"rendered_image_frame"[[:space:]]*:[[:space:]]*"true"' "$proof" \
    && rg -q '"unsupported_http"[[:space:]]*:[[:space:]]*"400"' "$proof" \
    && rg -q '"unsupported_ok"[[:space:]]*:[[:space:]]*"false"' "$proof" \
    && rg -q '"unsupported_rejected"[[:space:]]*:[[:space:]]*"true"' "$proof"
}

audio_output_proof_passes() {
  local proof="$1"

  proof_json_passes "$proof" || return 1
  [ -s "$proof" ] \
    && rg -q '"status"[[:space:]]*:[[:space:]]*"pass"' "$proof" \
    && rg -q '"status_route"[[:space:]]*:[[:space:]]*"/v1/audio/status"' "$proof" \
    && rg -q '"status_http"[[:space:]]*:[[:space:]]*"200"' "$proof" \
    && rg -q '"wireplumber_available"[[:space:]]*:[[:space:]]*"true"' "$proof" \
    && rg -q '"output_available"[[:space:]]*:[[:space:]]*"true"' "$proof" \
    && rg -q '"player"[[:space:]]*:[[:space:]]*"(pw-play|paplay)"' "$proof" \
    && rg -q '"test_tone_seconds"[[:space:]]*:[[:space:]]*"45"' "$proof" \
    && rg -q '"screenshot"[[:space:]]*:[[:space:]]*"24-audio-output\.png"' "$proof" \
    && rg -q '"rendered_sound_panel"[[:space:]]*:[[:space:]]*"true"' "$proof"
}

runtime_build_proof_passes() {
  local proof="$1"

  proof_json_passes "$proof" || return 1
  [ -s "$proof" ] \
    && rg -q '"status"[[:space:]]*:[[:space:]]*"pass"' "$proof" \
    && rg -q '"route"[[:space:]]*:[[:space:]]*"/v1/apps/builds"' "$proof" \
    && rg -q '"engine_mode"[[:space:]]*:[[:space:]]*"local-model"' "$proof" \
    && rg -q '"engine_source"[[:space:]]*:[[:space:]]*"[A-Za-z0-9._:-]+-built"' "$proof" \
    && rg -q '"built_artifact_id"[[:space:]]*:[[:space:]]*"[A-Za-z0-9._:-]+"' "$proof" \
    && rg -q '"built_artifact_name"[[:space:]]*:[[:space:]]*"[^"]+"' "$proof" \
    && rg -q '"intent"[[:space:]]*:[[:space:]]*"[^"]+"' "$proof"
}

print_missing_screenshot_paths() {
  local run_dir="$1"
  local missing=0
  local shot
  for shot in "${REQ_SCREENSHOTS[@]}"; do
    if ! screenshot_file_is_valid_png "$run_dir/$shot"; then
      echo "  $run_dir/$shot"
      missing=1
    fi
  done
  if ! screenshot_manifest_is_coherent "$run_dir" "$(screenshot_run_arch "$run_dir")"; then
    echo "  $run_dir/proof-manifest.json"
    missing=1
  fi
  if ! firewall_live_toggle_proof_passes "$run_dir/$FIREWALL_LIVE_TOGGLE_PROOF"; then
    echo "  $run_dir/$FIREWALL_LIVE_TOGGLE_PROOF"
    missing=1
  fi
  if ! text_shortcuts_session_enable_proof_passes "$run_dir/$TEXT_SHORTCUTS_SESSION_ENABLE_PROOF"; then
    echo "  $run_dir/$TEXT_SHORTCUTS_SESSION_ENABLE_PROOF"
    missing=1
  fi
  if ! text_shortcuts_candidate_metadata_proof_passes "$run_dir/$TEXT_SHORTCUTS_CANDIDATE_METADATA_PROOF"; then
    echo "  $run_dir/$TEXT_SHORTCUTS_CANDIDATE_METADATA_PROOF"
    missing=1
  fi
  if ! text_shortcuts_overlay_intent_proof_passes "$run_dir/$TEXT_SHORTCUTS_OVERLAY_INTENT_PROOF"; then
    echo "  $run_dir/$TEXT_SHORTCUTS_OVERLAY_INTENT_PROOF"
    missing=1
  fi
  if ! text_shortcuts_candidate_bubble_frame_proof_passes "$run_dir/$TEXT_SHORTCUTS_CANDIDATE_BUBBLE_FRAME_PROOF"; then
    echo "  $run_dir/$TEXT_SHORTCUTS_CANDIDATE_BUBBLE_FRAME_PROOF"
    missing=1
  fi
  if ! text_shortcuts_candidate_bubble_layout_proof_passes "$run_dir/$TEXT_SHORTCUTS_CANDIDATE_BUBBLE_LAYOUT_PROOF"; then
    echo "  $run_dir/$TEXT_SHORTCUTS_CANDIDATE_BUBBLE_LAYOUT_PROOF"
    missing=1
  fi
  if ! text_shortcuts_candidate_bubble_render_intent_proof_passes "$run_dir/$TEXT_SHORTCUTS_CANDIDATE_BUBBLE_RENDER_INTENT_PROOF"; then
    echo "  $run_dir/$TEXT_SHORTCUTS_CANDIDATE_BUBBLE_RENDER_INTENT_PROOF"
    missing=1
  fi
  if ! text_shortcuts_candidate_bubble_render_proof_passes "$run_dir/$TEXT_SHORTCUTS_CANDIDATE_BUBBLE_RENDER_PROOF"; then
    echo "  $run_dir/$TEXT_SHORTCUTS_CANDIDATE_BUBBLE_RENDER_PROOF"
    missing=1
  fi
  if ! text_shortcuts_live_ibus_runtime_render_proof_passes "$run_dir/$TEXT_SHORTCUTS_LIVE_IBUS_RUNTIME_RENDER_PROOF"; then
    echo "  $run_dir/$TEXT_SHORTCUTS_LIVE_IBUS_RUNTIME_RENDER_PROOF"
    missing=1
  fi
  if ! keyboard_shortcuts_roundtrip_proof_passes "$run_dir/$KEYBOARD_SHORTCUTS_ROUNDTRIP_PROOF"; then
    echo "  $run_dir/$KEYBOARD_SHORTCUTS_ROUNDTRIP_PROOF"
    missing=1
  fi
  if ! input_sources_roundtrip_proof_passes "$run_dir/$INPUT_SOURCES_ROUNDTRIP_PROOF"; then
    echo "  $run_dir/$INPUT_SOURCES_ROUNDTRIP_PROOF"
    missing=1
  fi
  if ! multi_display_apply_proof_passes "$run_dir/$MULTI_DISPLAY_APPLY_PROOF"; then
    echo "  $run_dir/$MULTI_DISPLAY_APPLY_PROOF"
    missing=1
  fi
  if ! focus_arm_roundtrip_proof_passes "$run_dir/$FOCUS_ARM_ROUNDTRIP_PROOF"; then
    echo "  $run_dir/$FOCUS_ARM_ROUNDTRIP_PROOF"
    missing=1
  fi
  if ! app_privacy_revoke_proof_passes "$run_dir/$APP_PRIVACY_REVOKE_PROOF"; then
    echo "  $run_dir/$APP_PRIVACY_REVOKE_PROOF"
    missing=1
  fi
  if ! preview_open_render_proof_passes "$run_dir/$PREVIEW_OPEN_RENDER_PROOF"; then
    echo "  $run_dir/$PREVIEW_OPEN_RENDER_PROOF"
    missing=1
  fi
  if ! audio_output_proof_passes "$run_dir/$AUDIO_OUTPUT_PROOF"; then
    echo "  $run_dir/$AUDIO_OUTPUT_PROOF"
    missing=1
  fi
  if ! runtime_build_proof_passes "$run_dir/$RUNTIME_BUILD_PROOF"; then
    echo "  $run_dir/$RUNTIME_BUILD_PROOF"
    missing=1
  fi
  if ! accessibility_adaptivity_proof_passes "$run_dir/$ACCESSIBILITY_ADAPTIVITY_PROOF"; then
    echo "  $run_dir/$ACCESSIBILITY_ADAPTIVITY_PROOF"
    missing=1
  fi
  return "$missing"
}

print_latest_incomplete_screenshot_run() {
  local root_dir="$1"
  local label="$2"
  local latest=""

  if [ ! -d "$root_dir" ]; then
    echo "[INFO] $label screenshot root is missing: $root_dir"
    echo "[INFO] Expected screenshot proof files:"
    print_missing_screenshot_paths "$root_dir/<date>" || true
    return 0
  fi

  latest="$(find "$root_dir" -mindepth 1 -maxdepth 1 -type d | sort -r | head -n 1 || true)"
  if [ -z "$latest" ]; then
    echo "[INFO] $label screenshot root has no dated run directories: $root_dir"
    echo "[INFO] Expected screenshot proof files:"
    print_missing_screenshot_paths "$root_dir/<date>" || true
    return 0
  fi

  echo "[INFO] Latest incomplete $label screenshot run: $latest"
  echo "[INFO] Missing screenshot proof files:"
  print_missing_screenshot_paths "$latest" || true
}

print_legacy_screenshot_roots() {
  local dir
  local base
  local count=0
  local shown=0

  [ -d "$SCREENSHOT_ROOT" ] || return 0

  while IFS= read -r dir; do
    base="$(basename "$dir")"
    case "$base" in
      aarch64)
        continue
        ;;
    esac

    if [ "$count" -eq 0 ]; then
      echo "[INFO] Legacy/non-shipping screenshot roots ignored by architecture proof gate:"
    fi

    count=$((count + 1))
    if [ "$shown" -lt 12 ]; then
      echo "  $dir"
      shown=$((shown + 1))
    fi
  done < <(find "$SCREENSHOT_ROOT" -mindepth 1 -maxdepth 1 -type d | sort)

  if [ "$count" -gt "$shown" ]; then
    echo "  ... $((count - shown)) more"
  fi
}

print_screenshot_run_checks() {
  local run_dir="$1"
  local arch
  local missing=0
  local shot
  arch="$(screenshot_run_arch "$run_dir")"
  for shot in "${REQ_SCREENSHOTS[@]}"; do
    if screenshot_file_is_valid_png "$run_dir/$shot"; then
      echo "[PASS] $shot"
    else
      echo "[FAIL] $shot (missing, empty, or not a PNG)"
      missing=1
    fi
  done
  if semantic_screenshot_frames_are_distinct "$run_dir"; then
    echo "[PASS] named login/Home and Studio semantic screenshot states are distinct"
  else
    echo "[FAIL] named login/Home or Studio semantic screenshot states reuse a central application crop"
    missing=1
  fi
  if [ -n "$arch" ] && screenshot_manifest_is_coherent "$run_dir" "$arch"; then
    echo "[PASS] proof-manifest.json coherent for verification ISO proof"
  else
    echo "[FAIL] proof-manifest.json (missing or incoherent verification ISO proof)"
    missing=1
  fi
  if firewall_live_toggle_proof_passes "$run_dir/$FIREWALL_LIVE_TOGGLE_PROOF"; then
    echo "[PASS] $FIREWALL_LIVE_TOGGLE_PROOF"
  else
    echo "[FAIL] $FIREWALL_LIVE_TOGGLE_PROOF (missing or live firewall toggle proof failed)"
    missing=1
  fi
  if text_shortcuts_session_enable_proof_passes "$run_dir/$TEXT_SHORTCUTS_SESSION_ENABLE_PROOF"; then
    echo "[PASS] $TEXT_SHORTCUTS_SESSION_ENABLE_PROOF"
  else
    echo "[FAIL] $TEXT_SHORTCUTS_SESSION_ENABLE_PROOF (missing or Text Shortcuts session-enable proof failed)"
    missing=1
  fi
  if text_shortcuts_candidate_metadata_proof_passes "$run_dir/$TEXT_SHORTCUTS_CANDIDATE_METADATA_PROOF"; then
    echo "[PASS] $TEXT_SHORTCUTS_CANDIDATE_METADATA_PROOF"
  else
    echo "[FAIL] $TEXT_SHORTCUTS_CANDIDATE_METADATA_PROOF (missing or Text Shortcuts candidate metadata proof failed)"
    missing=1
  fi
  if text_shortcuts_overlay_intent_proof_passes "$run_dir/$TEXT_SHORTCUTS_OVERLAY_INTENT_PROOF"; then
    echo "[PASS] $TEXT_SHORTCUTS_OVERLAY_INTENT_PROOF"
  else
    echo "[FAIL] $TEXT_SHORTCUTS_OVERLAY_INTENT_PROOF (missing or Text Shortcuts overlay-intent proof failed)"
    missing=1
  fi
  if text_shortcuts_candidate_bubble_frame_proof_passes "$run_dir/$TEXT_SHORTCUTS_CANDIDATE_BUBBLE_FRAME_PROOF"; then
    echo "[PASS] $TEXT_SHORTCUTS_CANDIDATE_BUBBLE_FRAME_PROOF"
  else
    echo "[FAIL] $TEXT_SHORTCUTS_CANDIDATE_BUBBLE_FRAME_PROOF (missing or Text Shortcuts candidate-bubble-frame proof failed)"
    missing=1
  fi
  if text_shortcuts_candidate_bubble_layout_proof_passes "$run_dir/$TEXT_SHORTCUTS_CANDIDATE_BUBBLE_LAYOUT_PROOF"; then
    echo "[PASS] $TEXT_SHORTCUTS_CANDIDATE_BUBBLE_LAYOUT_PROOF"
  else
    echo "[FAIL] $TEXT_SHORTCUTS_CANDIDATE_BUBBLE_LAYOUT_PROOF (missing or Text Shortcuts candidate-bubble-layout proof failed)"
    missing=1
  fi
  if text_shortcuts_candidate_bubble_render_intent_proof_passes "$run_dir/$TEXT_SHORTCUTS_CANDIDATE_BUBBLE_RENDER_INTENT_PROOF"; then
    echo "[PASS] $TEXT_SHORTCUTS_CANDIDATE_BUBBLE_RENDER_INTENT_PROOF"
  else
    echo "[FAIL] $TEXT_SHORTCUTS_CANDIDATE_BUBBLE_RENDER_INTENT_PROOF (missing or Text Shortcuts candidate-bubble-render-intent proof failed)"
    missing=1
  fi
  if text_shortcuts_candidate_bubble_render_proof_passes "$run_dir/$TEXT_SHORTCUTS_CANDIDATE_BUBBLE_RENDER_PROOF"; then
    echo "[PASS] $TEXT_SHORTCUTS_CANDIDATE_BUBBLE_RENDER_PROOF"
  else
    echo "[FAIL] $TEXT_SHORTCUTS_CANDIDATE_BUBBLE_RENDER_PROOF (missing or Text Shortcuts candidate-bubble-render screenshot proof failed)"
    missing=1
  fi
  if text_shortcuts_live_ibus_runtime_render_proof_passes "$run_dir/$TEXT_SHORTCUTS_LIVE_IBUS_RUNTIME_RENDER_PROOF"; then
    echo "[PASS] $TEXT_SHORTCUTS_LIVE_IBUS_RUNTIME_RENDER_PROOF"
  else
    echo "[FAIL] $TEXT_SHORTCUTS_LIVE_IBUS_RUNTIME_RENDER_PROOF (missing or Text Shortcuts live IBus runtime/render proof failed)"
    missing=1
  fi
  if keyboard_shortcuts_roundtrip_proof_passes "$run_dir/$KEYBOARD_SHORTCUTS_ROUNDTRIP_PROOF"; then
    echo "[PASS] $KEYBOARD_SHORTCUTS_ROUNDTRIP_PROOF"
  else
    echo "[FAIL] $KEYBOARD_SHORTCUTS_ROUNDTRIP_PROOF (missing or Keyboard shortcuts roundtrip proof failed)"
    missing=1
  fi
  if input_sources_roundtrip_proof_passes "$run_dir/$INPUT_SOURCES_ROUNDTRIP_PROOF"; then
    echo "[PASS] $INPUT_SOURCES_ROUNDTRIP_PROOF"
  else
    echo "[FAIL] $INPUT_SOURCES_ROUNDTRIP_PROOF (missing or Input sources roundtrip proof failed)"
    missing=1
  fi
  if multi_display_apply_proof_passes "$run_dir/$MULTI_DISPLAY_APPLY_PROOF"; then
    echo "[PASS] $MULTI_DISPLAY_APPLY_PROOF"
  else
    echo "[FAIL] $MULTI_DISPLAY_APPLY_PROOF (missing or Multi-display apply proof failed)"
    missing=1
  fi
  if focus_arm_roundtrip_proof_passes "$run_dir/$FOCUS_ARM_ROUNDTRIP_PROOF"; then
    echo "[PASS] $FOCUS_ARM_ROUNDTRIP_PROOF"
  else
    echo "[FAIL] $FOCUS_ARM_ROUNDTRIP_PROOF (missing or Focus arm roundtrip proof failed)"
    missing=1
  fi
  if app_privacy_revoke_proof_passes "$run_dir/$APP_PRIVACY_REVOKE_PROOF"; then
    echo "[PASS] $APP_PRIVACY_REVOKE_PROOF"
  else
    echo "[FAIL] $APP_PRIVACY_REVOKE_PROOF (missing or App privacy revoke proof failed)"
    missing=1
  fi
  if preview_open_render_proof_passes "$run_dir/$PREVIEW_OPEN_RENDER_PROOF"; then
    echo "[PASS] $PREVIEW_OPEN_RENDER_PROOF"
  else
    echo "[FAIL] $PREVIEW_OPEN_RENDER_PROOF (missing or Preview open/render proof failed)"
    missing=1
  fi
  if audio_output_proof_passes "$run_dir/$AUDIO_OUTPUT_PROOF"; then
    echo "[PASS] $AUDIO_OUTPUT_PROOF"
  else
    echo "[FAIL] $AUDIO_OUTPUT_PROOF (missing or audio output proof failed)"
    missing=1
  fi
  if runtime_build_proof_passes "$run_dir/$RUNTIME_BUILD_PROOF"; then
    echo "[PASS] $RUNTIME_BUILD_PROOF"
  else
    echo "[FAIL] $RUNTIME_BUILD_PROOF (missing or runtime app-build proof failed)"
    missing=1
  fi
  if accessibility_adaptivity_proof_passes "$run_dir/$ACCESSIBILITY_ADAPTIVITY_PROOF"; then
    echo "[PASS] $ACCESSIBILITY_ADAPTIVITY_PROOF and exact screenshot SHA256 bindings"
  else
    echo "[FAIL] $ACCESSIBILITY_ADAPTIVITY_PROOF (missing, invalid, or screenshot bindings failed)"
    missing=1
  fi
  return "$missing"
}

print_arch_next_steps() {
  local arch="$1"

  cat <<EOF

Next native Linux packaging command for $arch:
  GOBLINS_OS_CANDIDATE_COMMIT=$SELECTED_CANDIDATE_COMMIT \\
  GOBLINS_OS_ARCH=$arch \\
  GOBLINS_OS_CONTAINER_RUNTIME=docker \\
  RUN_QEMU=0 \\
  GOBLINS_OS_SHIPPABLE_RELEASE=1 \\
  GOBLINS_OS_BIB_SOURCE_IMAGE=<real release bootc image ref for $arch> \\
  GOBLINS_OS_INSTALLER_BRANDING_IMAGE=<protected-publisher branding image@sha256:digest> \\
  GOBLINS_OS_INSTALLER_BRANDING_PUBLISHER_EVIDENCE=<protected-publisher evidence JSON> \\
  REPO_ROOT="$ROOT" \\
  os/hardware-gate/run-external-gate.sh

Native runner preflight for $arch without building artifacts (with the same candidate selected):
  GOBLINS_OS_CANDIDATE_COMMIT=$SELECTED_CANDIDATE_COMMIT \\
  GOBLINS_OS_ARCH=$arch RUN_QEMU=0 GOBLINS_OS_SHIPPABLE_RELEASE=1 \
  GOBLINS_OS_BIB_SOURCE_IMAGE=<real release bootc image digest ref for $arch> \
  PREFLIGHT_ONLY=1 REPO_ROOT="$ROOT" os/hardware-gate/run-external-gate.sh

Diagnostic artifact/SBOM build for native $arch without release authority (with the same candidate selected):
  GOBLINS_OS_CANDIDATE_COMMIT=$SELECTED_CANDIDATE_COMMIT \\
  GOBLINS_OS_ARCH=$arch RUN_QEMU=0 GOBLINS_OS_SHIPPABLE_RELEASE=0 REPO_ROOT="$ROOT" os/hardware-gate/run-external-gate.sh

Runtime app-build proof for $arch, from inside a Goblins OS image/container joined to a real local model runtime:
  PROOF_PATH=os/screenshots/hardware-gate/$arch/<date>/$RUNTIME_BUILD_PROOF \\
  BUILD_RESPONSE_PATH=os/screenshots/hardware-gate/$arch/<date>/build-response.json \\
  os/runtime-gate/build-an-app-live-model.sh

Final signoff row after the display-backed screenshots and runtime-built app proof exist:
  GOBLINS_OS_CANDIDATE_COMMIT=$SELECTED_CANDIDATE_COMMIT \\
  GOBLINS_OS_ARCH=$arch \\
  SCREENSHOT_RUN_DIR=os/screenshots/hardware-gate/$arch/<date> \\
  RUNTIME_ENGINE_MODE=<real-mode> \\
  RUNTIME_ENGINE_SOURCE=<real-engine-source> \\
  RUNTIME_ENGINE_CONFIG=<config-or-artifact-path> \\
  BUILT_ARTIFACT_PATH_URL=<real-built-app-path-or-url> \\
  ./os/hardware-gate/close-signoff.sh

Expected $arch proof files:
  os/iso/output/$arch/bootiso/goblins-os-$arch.iso
  os/iso/output/$arch/bootiso/goblins-os-$arch.iso.sha256
  os/iso/output/$arch/manifest-goblins-os-$arch.json
  os/iso/output/$arch/installer-branding-publisher-evidence.json
  os/signoff-proofs/sbom/$arch/rpm-packages.tsv
  os/screenshots/hardware-gate/$arch/<date>/${REQ_SCREENSHOTS[0]} ... ${REQ_SCREENSHOTS[$((${#REQ_SCREENSHOTS[@]} - 1))]}
  os/screenshots/hardware-gate/$arch/<date>/proof-manifest.json
  os/screenshots/hardware-gate/$arch/<date>/$FIREWALL_LIVE_TOGGLE_PROOF
  os/screenshots/hardware-gate/$arch/<date>/$TEXT_SHORTCUTS_SESSION_ENABLE_PROOF
  os/screenshots/hardware-gate/$arch/<date>/$TEXT_SHORTCUTS_CANDIDATE_METADATA_PROOF
  os/screenshots/hardware-gate/$arch/<date>/$TEXT_SHORTCUTS_OVERLAY_INTENT_PROOF
  os/screenshots/hardware-gate/$arch/<date>/$TEXT_SHORTCUTS_CANDIDATE_BUBBLE_FRAME_PROOF
  os/screenshots/hardware-gate/$arch/<date>/$TEXT_SHORTCUTS_CANDIDATE_BUBBLE_LAYOUT_PROOF
  os/screenshots/hardware-gate/$arch/<date>/$TEXT_SHORTCUTS_CANDIDATE_BUBBLE_RENDER_INTENT_PROOF
  os/screenshots/hardware-gate/$arch/<date>/$TEXT_SHORTCUTS_CANDIDATE_BUBBLE_RENDER_PROOF
  os/screenshots/hardware-gate/$arch/<date>/$TEXT_SHORTCUTS_LIVE_IBUS_RUNTIME_RENDER_PROOF
  os/screenshots/hardware-gate/$arch/<date>/$KEYBOARD_SHORTCUTS_ROUNDTRIP_PROOF
  os/screenshots/hardware-gate/$arch/<date>/$INPUT_SOURCES_ROUNDTRIP_PROOF
  os/screenshots/hardware-gate/$arch/<date>/$MULTI_DISPLAY_APPLY_PROOF
  os/screenshots/hardware-gate/$arch/<date>/$FOCUS_ARM_ROUNDTRIP_PROOF
  os/screenshots/hardware-gate/$arch/<date>/$APP_PRIVACY_REVOKE_PROOF
  os/screenshots/hardware-gate/$arch/<date>/$PREVIEW_OPEN_RENDER_PROOF
  os/screenshots/hardware-gate/$arch/<date>/$AUDIO_OUTPUT_PROOF
  os/screenshots/hardware-gate/$arch/<date>/$RUNTIME_BUILD_PROOF
EOF

  if [ "$arch" = "aarch64" ]; then
    cat <<EOF

Apple Silicon/HVF capture after the verification ISO is present:
  RUN_DATE=<date> \
  GOBLINS_OS_ARCH=aarch64 \
  GOBLINS_OS_CAPTURE_EXPECTED_IMAGE_REF=<selected-aarch64-image@sha256:digest> \
  GOBLINS_OS_CAPTURE_NATIVE_PACKAGING_GATE_PROOF=<native-packaging-gate.json> \
  GOBLINS_OS_CAPTURE_NATIVE_PACKAGING_GATE_RUN_URL=<exact-actions-run-url> \
  REPO_ROOT="$ROOT" \
  os/hardware-gate/capture-harness/run-capture.sh
EOF
  fi
}

signoff_block_contains() {
  local block="$1"
  local pattern="$2"

  printf '%s\n' "$block" | rg -q "$pattern"
}

signoff_block_has_real_field() {
  local block="$1"
  local pattern="$2"
  local line

  line="$(printf '%s\n' "$block" | rg "$pattern" || true)"
  [ -n "$line" ] || return 1
  ! printf '%s\n' "$line" | rg -qi 'n/a|not provided|not configured|requires|external gate|not exercised|none|unknown|missing|no live engine'
}

signoff_block_top_level_labels_are_unique() {
  local block="$1"

  printf '%s\n' "$block" | awk '
    /^- [^:]+:/ {
      label = $0
      sub(/:.*/, "", label)
      if (seen[label]++) {
        duplicate = 1
      }
    }
    END { exit duplicate }
  '
}

signoff_block_verification_iso_binding_matches() {
  local block="$1"
  local artifact artifact_file verification_sha screenshot_dir recorded_screenshot_sha
  local manifest_sha native_attempt native_sha expected_artifact

  artifact="$(printf '%s\n' "$block" | sed -n 's/^- Verification ISO artifact: //p' | head -n 1)"
  artifact_file="$(printf '%s\n' "$block" | sed -n 's/^- Verification ISO artifact file: //p' | head -n 1)"
  verification_sha="$(printf '%s\n' "$block" | sed -n 's/^- Verification ISO SHA256: //p' | head -n 1)"
  recorded_screenshot_sha="$(printf '%s\n' "$block" | sed -n 's/^- Screenshot proof verification ISO SHA256: //p' | head -n 1)"
  screenshot_dir="$(printf '%s\n' "$block" | sed -n 's/^- Screenshot dir: //p' | head -n 1)"
  case "$screenshot_dir" in
    os/screenshots/hardware-gate/aarch64/*) ;;
    *) return 1 ;;
  esac
  native_attempt="$(printf '%s\n' "$block" | sed -n 's/^- Native packaging gate run attempt: //p' | head -n 1)"
  [[ "$native_attempt" =~ ^[1-9][0-9]*$ ]] || return 1
  expected_artifact="goblins-os-aarch64-verification-iso-$SELECTED_CANDIDATE_COMMIT-${screenshot_dir##*/}-attempt-$native_attempt"
  manifest_sha="$(screenshot_manifest_iso_sha "$screenshot_dir/proof-manifest.json" | tr '[:upper:]' '[:lower:]')"
  native_sha="$(awk -F'"' '/"verification_iso_sha256"/ { print $4; exit }' "$screenshot_dir/native-packaging-gate.json" | tr '[:upper:]' '[:lower:]')"
  [[ "$verification_sha" =~ ^[0-9a-f]{64}$ ]] \
    && [ "$artifact" = "$expected_artifact" ] \
    && [ "$artifact_file" = "$artifact/os/iso/output/aarch64/bootiso/goblins-os-aarch64.iso" ] \
    && [ "$recorded_screenshot_sha" = "$verification_sha" ] \
    && [ "$manifest_sha" = "$verification_sha" ] \
    && [ "$native_sha" = "$verification_sha" ]
}

signoff_block_public_release_iso_binding_matches() {
  local block="$1"
  local arch image_ref public_artifact public_artifact_file public_iso public_sha
  local metadata metadata_path source_repository workflow_run workflow_attempt actual_sha

  arch="$(printf '%s\n' "$block" | sed -n 's/^- Architecture: //p' | head -n 1)"
  architecture_is_canonical "$arch" || return 1
  image_ref="$(printf '%s\n' "$block" | sed -n 's/^- Image digest reference: //p' | head -n 1)"
  public_artifact="$(printf '%s\n' "$block" | sed -n 's/^- Public release artifact: //p' | head -n 1)"
  public_artifact_file="$(printf '%s\n' "$block" | sed -n 's/^- Public release ISO artifact file: //p' | head -n 1)"
  public_iso="$(printf '%s\n' "$block" | sed -n 's/^- Public release ISO: //p' | head -n 1)"
  public_sha="$(printf '%s\n' "$block" | sed -n 's/^- Public release ISO SHA256: //p' | head -n 1)"
  metadata_path="$(printf '%s\n' "$block" | sed -n 's/^- Public release candidate metadata: //p' | head -n 1)"
  source_repository="$(printf '%s\n' "$block" | sed -n 's/^- Public release source repository: //p' | head -n 1)"
  workflow_run="$(printf '%s\n' "$block" | sed -n 's/^- Public release workflow run: //p' | head -n 1)"
  workflow_attempt="$(printf '%s\n' "$block" | sed -n 's/^- Public release workflow run attempt: //p' | head -n 1)"
  metadata="os/signoff-proofs/candidate/$arch/image-ref.json"

  [[ "$public_sha" =~ ^[0-9a-f]{64}$ ]] || return 1
  [[ "$workflow_attempt" =~ ^[1-9][0-9]*$ ]] || return 1
  [ "$public_artifact" = "goblins-os-candidate-$SELECTED_CANDIDATE_COMMIT-$arch" ] \
    && [ "$public_artifact_file" = "$public_artifact/os/iso/output/$arch/bootiso/goblins-os-$arch.iso" ] \
    && [ "$public_iso" = "os/iso/output/$arch/bootiso/goblins-os-$arch.iso" ] \
    && [ "$metadata_path" = "$metadata" ] \
    && [ "$source_repository" = "https://github.com/Joe-Simo/goblins-os" ] \
    || return 1
  actual_sha="$(sha256_of_file "$public_iso")" || return 1
  [ "$actual_sha" = "$public_sha" ] \
    && [ "$workflow_run" = "$(candidate_artifact_workflow_run "$metadata")" ] \
    && [ "$workflow_attempt" = "$(candidate_artifact_workflow_attempt "$metadata")" ] \
    && candidate_artifact_metadata_passes \
      "$metadata" \
      "$arch" \
      "$SELECTED_CANDIDATE_COMMIT" \
      "$image_ref" \
      "$public_sha"
}

signoff_block_candidate_commit_matches() {
  local block="$1"
  local arch signoff_commit screenshot_dir proof_commit iso_commit evidence_commit

  [ "$CANDIDATE_SELECTION_VALID" -eq 1 ] || return 1
  arch="$(printf '%s\n' "$block" | sed -n 's/^- Architecture: //p' | head -n 1)"
  architecture_is_canonical "$arch" || return 1
  signoff_commit="$(printf '%s\n' "$block" | sed -n 's/^- Candidate\/source commit: //p' | head -n 1 | tr '[:upper:]' '[:lower:]')"
  screenshot_dir="$(printf '%s\n' "$block" | sed -n 's/^- Screenshot dir: //p' | head -n 1)"
  proof_commit="$(manifest_candidate_commit "$screenshot_dir/proof-manifest.json")"
  iso_commit="$(manifest_candidate_commit "os/iso/output/$arch/manifest-goblins-os-$arch.json")"
  evidence_commit="$(manifest_candidate_commit "os/signoff-proofs/sbom/$arch/release-evidence-manifest.json")"

  [ "$signoff_commit" = "$SELECTED_CANDIDATE_COMMIT" ] \
    && [ "$proof_commit" = "$SELECTED_CANDIDATE_COMMIT" ] \
    && [ "$iso_commit" = "$SELECTED_CANDIDATE_COMMIT" ] \
    && [ "$evidence_commit" = "$SELECTED_CANDIDATE_COMMIT" ]
}

signoff_block_image_ref_matches() {
  local block="$1"
  local arch signoff_ref tested_image selftest_image screenshot_dir proof_ref iso_ref evidence_ref

  arch="$(printf '%s\n' "$block" | sed -n 's/^- Architecture: //p' | head -n 1)"
  architecture_is_canonical "$arch" || return 1
  signoff_ref="$(printf '%s\n' "$block" | sed -n 's/^- Image digest reference: //p' | head -n 1)"
  tested_image="$(printf '%s\n' "$block" | sed -n 's/^- Image: //p' | head -n 1)"
  selftest_image="$(printf '%s\n' "$block" | sed -n 's/^- Self-test image: //p' | head -n 1)"
  screenshot_dir="$(printf '%s\n' "$block" | sed -n 's/^- Screenshot dir: //p' | head -n 1)"
  proof_ref="$(manifest_image_ref "$screenshot_dir/proof-manifest.json")"
  iso_ref="$(iso_manifest_image_ref "os/iso/output/$arch/manifest-goblins-os-$arch.json")"
  evidence_ref="$(manifest_image_ref "os/signoff-proofs/sbom/$arch/release-evidence-manifest.json")"

  image_ref_is_digest_pinned "$signoff_ref" \
    && [ "$tested_image" = "$signoff_ref" ] \
    && [ "$selftest_image" = "$signoff_ref" ] \
    && printf '%s\n' "$block" | grep -Fq -- "run --rm $signoff_ref /usr/libexec/goblins-os/goblins-os-verify --installed-root /" \
    && [ "$proof_ref" = "$signoff_ref" ] \
    && [ "$iso_ref" = "$signoff_ref" ] \
    && [ "$evidence_ref" = "$signoff_ref" ]
}

signoff_block_required_proof_is_complete() {
  local block="$1"
  local arch="${2:-}"
  local native_gate_path native_gate_run native_gate_attempt native_gate_artifact proof_native_gate_attempt signoff_ref screenshot_dir
  local native_iso_sha native_iso_manifest_sha native_bib_manifest_sha native_evidence_manifest_sha
  local evidence_bundle_path evidence_bundle_sha expected_bundle_sha image_ref
  local local_attestation_path local_attestation_signature local_authority_fingerprint local_authority_ca_fingerprint
  local local_authority_iso_sha local_authority_screenshot_manifest_sha
  local expected_attestation_fields expected_authority_fingerprint expected_authority_ca_fingerprint expected_authority_iso_sha
  local expected_authority_screenshot_manifest_sha

  signoff_block_top_level_labels_are_unique "$block" || return 1
  if [ -z "$arch" ]; then
    arch="$(printf '%s\n' "$block" | sed -n 's/^- Architecture: //p' | head -n 1)"
  fi
  architecture_is_canonical "$arch" || return 1

  signoff_block_contains "$block" "^- Runner: .+" || return 1
  signoff_block_contains "$block" "^- Architecture: aarch64$" || return 1
  signoff_block_contains "$block" "^- Verification ISO artifact: goblins-os-aarch64-verification-iso-[0-9a-f]{40}-[0-9]{4}-[0-9]{2}-[0-9]{2}-attempt-[1-9][0-9]*$" || return 1
  signoff_block_contains "$block" "^- Verification ISO artifact file: goblins-os-aarch64-verification-iso-[0-9a-f]{40}-[0-9]{4}-[0-9]{2}-[0-9]{2}-attempt-[1-9][0-9]*/os/iso/output/aarch64/bootiso/goblins-os-aarch64\\.iso$" || return 1
  signoff_block_contains "$block" "^- Verification ISO SHA256: [0-9a-f]{64}$" || return 1
  signoff_block_contains "$block" "^- Screenshot proof verification ISO SHA256: [0-9a-f]{64}$" || return 1
  signoff_block_contains "$block" "^- Public release artifact: goblins-os-candidate-[0-9a-f]{40}-aarch64$" || return 1
  signoff_block_contains "$block" "^- Public release ISO artifact file: goblins-os-candidate-[0-9a-f]{40}-aarch64/os/iso/output/aarch64/bootiso/goblins-os-aarch64\\.iso$" || return 1
  signoff_block_contains "$block" "^- Public release ISO: os/iso/output/aarch64/bootiso/goblins-os-aarch64\\.iso$" || return 1
  signoff_block_contains "$block" "^- Public release ISO SHA256: [0-9a-f]{64}$" || return 1
  signoff_block_contains "$block" "^- Public release candidate metadata: os/signoff-proofs/candidate/aarch64/image-ref\\.json$" || return 1
  signoff_block_contains "$block" "^- Public release source repository: https://github\\.com/Joe-Simo/goblins-os$" || return 1
  signoff_block_contains "$block" "^- Public release workflow run: https://github\\.com/Joe-Simo/goblins-os/actions/runs/[0-9]+$" || return 1
  signoff_block_contains "$block" "^- Public release workflow run attempt: [1-9][0-9]*$" || return 1
  signoff_block_contains "$block" "^- Verification ISO candidate binding checked: yes \\(" || return 1
  signoff_block_contains "$block" "^- Native packaging gate checked: yes \\(" || return 1
  signoff_block_contains "$block" "^- Native packaging gate run: https://github\\.com/[^/]+/[^/]+/actions/runs/[0-9]+$" || return 1
  native_gate_path="$(printf '%s\n' "$block" | sed -n 's/^- Native packaging gate proof: //p' | head -n 1)"
  native_gate_run="$(printf '%s\n' "$block" | sed -n 's/^- Native packaging gate run: //p' | head -n 1)"
  native_gate_attempt="$(printf '%s\n' "$block" | sed -n 's/^- Native packaging gate run attempt: //p' | head -n 1)"
  signoff_ref="$(printf '%s\n' "$block" | sed -n 's/^- Image digest reference: //p' | head -n 1)"
  screenshot_dir="$(printf '%s\n' "$block" | sed -n 's/^- Screenshot dir: //p' | head -n 1)"
  [ "$native_gate_path" = "$screenshot_dir/native-packaging-gate.json" ] || return 1
  native_iso_sha="$(screenshot_manifest_iso_sha "$screenshot_dir/proof-manifest.json" | tr '[:upper:]' '[:lower:]')"
  native_iso_manifest_sha="$(sha256_of_file "$screenshot_dir/verification-iso-manifest.json")" || return 1
  native_bib_manifest_sha="$(sha256_of_file "$screenshot_dir/verification-bib-manifest.json")" || return 1
  native_evidence_manifest_sha="$(sha256_of_file "$screenshot_dir/verification-release-evidence-manifest.json")" || return 1
  native_packaging_gate_proof_passes \
    "$native_gate_path" \
    "$arch" \
    "$SELECTED_CANDIDATE_COMMIT" \
    "$signoff_ref" \
    "$native_gate_run" \
    "$native_iso_sha" \
    "$native_iso_manifest_sha" \
    "$native_bib_manifest_sha" \
    "$native_evidence_manifest_sha" \
    || return 1
  proof_native_gate_attempt="$(native_packaging_gate_workflow_run_attempt "$native_gate_path")"
  [ "$native_gate_attempt" = "$proof_native_gate_attempt" ] || return 1
  [[ "$native_gate_attempt" =~ ^[1-9][0-9]*$ ]] || return 1
  github_actions_run_is_successful \
    "$native_gate_run" \
    "$SELECTED_CANDIDATE_COMMIT" \
    "$native_gate_attempt" \
    ".github/workflows/aarch64-verification-iso.yml" \
    || return 1
  native_gate_artifact="goblins-os-aarch64-native-packaging-gate-$SELECTED_CANDIDATE_COMMIT-${screenshot_dir##*/}-attempt-$native_gate_attempt"
  github_actions_artifact_file_matches \
    "$native_gate_run" \
    "$native_gate_artifact" \
    "$native_gate_path" \
    "native-packaging-gate.json" \
    || return 1
  signoff_block_contains "$block" "^- Candidate/source commit: [a-fA-F0-9]{40}$" || return 1
  signoff_block_candidate_commit_matches "$block" || return 1
  signoff_block_contains "$block" "^- Image digest reference: [^[:space:]]+@sha256:[a-fA-F0-9]{64}$" || return 1
  signoff_block_image_ref_matches "$block" || return 1
  signoff_block_contains "$block" "^- Image: [^[:space:]]+@sha256:[a-fA-F0-9]{64}$" || return 1
  signoff_block_contains "$block" "goblins-os-verify --installed-root /" || return 1
  signoff_block_contains "$block" "^- Verify result \\(blocked=0\\): pass" || return 1
  signoff_block_contains "$block" "^- Self-test image: [^[:space:]]+@sha256:[a-fA-F0-9]{64}$" || return 1
  signoff_block_contains "$block" "^- Self-test command: .+" || return 1
  signoff_block_contains "$block" "^- Self-test result: pass" || return 1
  signoff_block_contains "$block" "^- Release evidence/SBOM checked: yes" || return 1
  signoff_block_contains "$block" "^- Screenshot dir: .+" || return 1
  signoff_block_contains "$block" "^- Screenshot dir: os/screenshots/hardware-gate/aarch64/[^/[:space:]]+$" || return 1
  signoff_block_contains "$block" "^- Screenshot dir: .*not provided|stale screenshot|stale for this ISO|No fresh .*screenshots|missing current screenshot proof" && return 1
  screenshot_dir="$(printf '%s\n' "$block" | sed -n 's/^- Screenshot dir: //p' | head -n 1)"
  image_ref="$(printf '%s\n' "$block" | sed -n 's/^- Image digest reference: //p' | head -n 1)"
  evidence_bundle_path="$(printf '%s\n' "$block" | sed -n 's/^- Evidence bundle: //p' | head -n 1)"
  evidence_bundle_sha="$(printf '%s\n' "$block" | sed -n 's/^- Evidence bundle SHA256: //p' | head -n 1)"
  [ "$evidence_bundle_path" = "$screenshot_dir/evidence-bundle.json" ] || return 1
  [[ "$evidence_bundle_sha" =~ ^[0-9a-f]{64}$ ]] || return 1
  expected_bundle_sha="$(evidence_bundle_digest "$screenshot_dir" "$arch" "$image_ref")" || return 1
  [ "$evidence_bundle_sha" = "$expected_bundle_sha" ] || return 1
  signoff_block_contains "$block" "^- Evidence bundle integrity checked: yes \(" || return 1
  screenshot_run_is_complete "$screenshot_dir" || return 1
  signoff_block_contains "$block" "^- Capture workflow run: not provided$" || return 1
  signoff_block_contains "$block" "^- Capture workflow run attempt: 0$" || return 1
    local_attestation_path="$(printf '%s\n' "$block" | sed -n 's/^- Local display attestation: //p' | head -n 1)"
    local_attestation_signature="$(printf '%s\n' "$block" | sed -n 's/^- Local display attestation signature: //p' | head -n 1)"
    local_authority_fingerprint="$(printf '%s\n' "$block" | sed -n 's/^- Local display authority certificate SHA256: //p' | head -n 1)"
    local_authority_ca_fingerprint="$(printf '%s\n' "$block" | sed -n 's/^- Local display authority CA certificate SHA256: //p' | head -n 1)"
    local_authority_iso_sha="$(printf '%s\n' "$block" | sed -n 's/^- Local display authority verification ISO SHA256: //p' | head -n 1)"
    local_authority_screenshot_manifest_sha="$(printf '%s\n' "$block" | sed -n 's/^- Local display authority screenshot manifest SHA256: //p' | head -n 1)"
    [ "$local_attestation_path" = "$screenshot_dir/aarch64-local-display-attestation.json" ] || return 1
    [ "$local_attestation_signature" = "$screenshot_dir/aarch64-local-display-attestation.json.cms" ] || return 1
    expected_attestation_fields="$(aarch64_local_display_attestation_fields "$screenshot_dir" "$image_ref")" \
      || return 1
    read -r expected_authority_fingerprint expected_authority_ca_fingerprint expected_authority_iso_sha expected_authority_screenshot_manifest_sha <<<"$expected_attestation_fields"
    [ "$local_authority_fingerprint" = "$expected_authority_fingerprint" ] || return 1
    [ "$local_authority_ca_fingerprint" = "$expected_authority_ca_fingerprint" ] || return 1
    [ "$local_authority_iso_sha" = "$expected_authority_iso_sha" ] || return 1
    [ "$local_authority_screenshot_manifest_sha" = "$expected_authority_screenshot_manifest_sha" ] || return 1
    signoff_block_contains "$block" "^- Local display attestation checked: yes \(" || return 1
  signoff_block_verification_iso_binding_matches "$block" || return 1
  signoff_block_public_release_iso_binding_matches "$block" || return 1
  signoff_block_has_real_field "$block" "^  - mode: .+" || return 1
  signoff_block_has_real_field "$block" "^  - engine source: .+" || return 1
  signoff_block_has_real_field "$block" "^  - built artifact path/URL: .+" || return 1
  signoff_block_contains "$block" "^- Motion/interactions checked: yes" || return 1
  signoff_block_contains "$block" "^- Firewall live toggle checked: yes" || return 1
  signoff_block_contains "$block" "^- Text Shortcuts session enablement checked: yes" || return 1
  signoff_block_contains "$block" "^- Text Shortcuts live keystrokes checked: yes" || return 1
  signoff_block_contains "$block" "^- Text Shortcuts candidate metadata checked: yes" || return 1
  signoff_block_contains "$block" "^- Text Shortcuts overlay intent checked: yes" || return 1
  signoff_block_contains "$block" "^- Text Shortcuts candidate bubble frame checked: yes" || return 1
  signoff_block_contains "$block" "^- Text Shortcuts candidate bubble layout checked: yes" || return 1
  signoff_block_contains "$block" "^- Text Shortcuts candidate bubble render intent checked: yes" || return 1
  signoff_block_contains "$block" "^- Text Shortcuts candidate bubble render screenshot checked: yes" || return 1
  signoff_block_contains "$block" "^- Keyboard shortcuts roundtrip checked: yes" || return 1
  signoff_block_contains "$block" "^- Input sources roundtrip checked: yes" || return 1
  signoff_block_contains "$block" "^- Multi-display apply checked: yes" || return 1
  signoff_block_contains "$block" "^- Focus arm roundtrip checked: yes" || return 1
  signoff_block_contains "$block" "^- App privacy revoke checked: yes" || return 1
  signoff_block_contains "$block" "^- Preview open/render checked: yes" || return 1
  signoff_block_contains "$block" "^- Audio output checked: yes" || return 1
  signoff_block_contains "$block" "^- Accessibility/adaptivity checked: yes \(" || return 1
  signoff_block_contains "$block" "^- Hosted-context review light/dark checked: yes \(" || return 1
  signoff_block_contains "$block" "^- Gaming readiness checked: yes" || return 1
  signoff_block_contains "$block" "^- Install storage/bootloader/dual-boot checked: yes" || return 1
  return 0
}

signoff_block_from_line() {
  local start="$1"

  awk -v start="$start" 'NR < start { next } NR == start { print; next } /^## / { exit } { print }' "$SIGNOFF"
}

signoff_run_for_arch_is_complete() {
  [ -n "$(signoff_screenshot_run_for_arch "$1")" ]
}

signoff_screenshot_run_for_arch() {
  local arch="$1"
  local start block

  [ -f "$SIGNOFF" ] || return 1
  while IFS= read -r start; do
    block="$(signoff_block_from_line "$start")"

    signoff_block_required_proof_is_complete "$block" "$arch" || continue
    signoff_block_contains "$block" "^- Current project completion status: complete$" || continue
    printf '%s\n' "$block" | sed -n 's/^- Screenshot dir: //p' | head -n 1
    return 0
  done < <(rg -n "^## Manual Gate Run:" "$SIGNOFF" | cut -d: -f1)

  return 1
}

echo "# Shipping status check"
echo

if [ "$CANDIDATE_SELECTION_VALID" -eq 1 ]; then
  echo "[PASS] Exact candidate/source commit selected: $SELECTED_CANDIDATE_COMMIT"
else
  echo "[FAIL] Set GOBLINS_OS_CANDIDATE_COMMIT to the exact 40-hex source commit selected for the aarch64 proof track"
  FAIL_COUNT=$((FAIL_COUNT + 1))
fi

check "final gate parsers accept only canonical aarch64 evidence" "architecture_is_canonical aarch64 && ! architecture_is_canonical x86_64 && ! architecture_is_canonical amd64 && [ \"\$(screenshot_run_arch os/screenshots/hardware-gate/aarch64/2026-07-21)\" = aarch64 ] && [ -z \"\$(screenshot_run_arch os/screenshots/hardware-gate/x86_64/2026-07-21)\" ] && [ -z \"\$(screenshot_run_arch os/screenshots/hardware-gate/amd64/2026-07-21)\" ]"

check "SHIP.md declares Fedora bootc foundation" "rg -q 'Fedora bootc remains the OS foundation' \"$SHIP_DECL\""
check "SHIP.md declares no custom kernel ownership" "rg -q 'no custom kernel|custom kernel' \"$SHIP_DECL\""
check "SHIP.md declares Inter-only typography boundary" "rg -q 'Inter is the final shipped font stack' \"$SHIP_DECL\" && rg -q 'no non-Inter brand font dependency' \"$SHIP_DECL\""
check "No unused external brand font references in public docs" "! rg -qi --hidden --no-ignore-vcs --no-ignore 'OpenAI[ -]Sans|openai[ -]sans|openai-sans' README.md ROADMAP.md GO-LIVE.md SHIP.md CONTRIBUTING.md CLA.md NOTICE TRADEMARKS.md AGENTS.md apps/site/src apps/site/public --glob '!apps/site/.next/**' --glob '!apps/site/node_modules/**'"
check "No typography licensing TODOs in signing docs" "! rg -qi 'licensing\s+TODO|TODO.*licensing' \"$SHIP_DECL\" \"$RUNBOOK\" \"$SIGNOFF\""
check "Secret scanner rejects synthetic canaries without echoing matched content" "bash os/hardware-gate/secret-scan.sh --self-test >/dev/null"
check "Source package secret scan finds no live keys" "source_secret_scan"
check "Generated artifact/evidence secret scan finds no live keys" "goblins_os_artifact_secret_scan \"$ROOT\""
check "installed-root verifier enforces secret file and directory modes" "rg -q 'installed-openai-secret-file-mode-0600' crates/goblins-os-verify/src/main.rs && rg -q 'installed-openai-secret-file-owner-root' crates/goblins-os-verify/src/main.rs && rg -q 'installed-openai-secret-file-empty' crates/goblins-os-verify/src/main.rs && rg -q 'var/lib/goblins-os/secrets/openai' crates/goblins-os-verify/src/main.rs"
check "OpenAI account credential is confined to the goblins-os service user" "rg -q 'codex-home-owner-only-0700' crates/goblins-os-verify/src/main.rs && rg -q 'codex-login-user-not-in-service-group' crates/goblins-os-verify/src/main.rs && ! rg -q 'usermod -aG goblins-os goblin' os/bootc/Containerfile && rg -q 'd /var/lib/goblins-os/codex 0700 goblins-os goblins-os' os/tmpfiles/goblins-os-codex.conf"
check "immutable Arm image bundles checksum-enforced Codex CLI and upstream notices" "rg -Fq 'ARG CODEX_VERSION=0.144.4' os/bootc/Containerfile && rg -Fq 'ARG CODEX_AARCH64_SHA256=4d07243ef4ae6786b8b321d7aea3f9be4e1d2c597ae5407e7c1b9873334082b2' os/bootc/Containerfile && rg -Fq 'ARG CODEX_LICENSE_SHA256=d17f227e4df5da1600391338865ce0f3055211760a36688f816941d58232d8dc' os/bootc/Containerfile && rg -Fq 'ARG CODEX_NOTICE_SHA256=9d71575ecfd9a843fc1677b0efb08053c6ba9fd686a0de1a6f5382fd3c220915' os/bootc/Containerfile && rg -Fq 'codex-aarch64-unknown-linux-musl.tar.gz' os/bootc/Containerfile os/release/third-party-notices.toml && test \"$(rg -Fc 'sha256sum --check --strict -' os/bootc/Containerfile)\" -ge 3 && rg -Fq 'codex-release-pins-are-enforced-and-cross-file-consistent' crates/goblins-os-verify/src/main.rs && rg -Fq '/out/usr/bin/codex' os/bootc/Containerfile && rg -Fq '/usr/share/licenses/openai-codex/LICENSE' os/bootc/Containerfile os/release/third-party-notices.toml && rg -Fq '/usr/share/licenses/openai-codex/NOTICE' os/bootc/Containerfile os/release/third-party-notices.toml && rg -Fq 'usr/bin/codex' crates/goblins-os-verify/src/main.rs"
check "core secrets use systemd credentials and never enter generic child environments" "rg -Fq 'LoadCredential=openai-secrets.env:/etc/goblins-os/openai-secrets.env' os/systemd/goblins-os-core.service && ! rg -Fq 'EnvironmentFile=-/etc/goblins-os/openai-secrets.env' os/systemd/goblins-os-core.service && rg -Fq 'env::var_os(\"CREDENTIALS_DIRECTORY\")' crates/goblins-os-core/src/credentials.rs && rg -Fq 'command.env_clear();' crates/goblins-os-core/src/bounded.rs && rg -Fq 'const SESSION_ENV_ALLOWLIST' crates/goblins-os-core/src/bounded.rs"
check "hosted OpenAI direct path uses Responses API" "rg -q '/v1/responses' crates/goblins-os-core/src/resident.rs && ! rg -q '/v1/chat.?completions' crates/goblins-os-core/src/resident.rs"
check "OpenAI SDK bridge endpoints stay server-side" "rg -q 'GOBLINS_OS_AGENTS_SDK_RELAY_URL' os/etc/goblins-os/openai-secrets.env && rg -q 'GOBLINS_OS_CHATKIT_RELAY_URL' os/etc/goblins-os/openai-secrets.env && rg -q 'GOBLINS_OS_REALTIME_RELAY_URL' os/etc/goblins-os/openai-secrets.env && rg -q 'GOBLINS_OS_IMAGES_RELAY_URL' os/etc/goblins-os/openai-secrets.env && ! rg -q 'OPENAI_OS_' os/etc/goblins-os/openai-secrets.env && rg -q 'Official OpenAI Agents SDK' crates/goblins-os-core/src/service_catalog.rs && ! rg -q 'pub struct OpenAIService' crates/goblins-os-core/src/service_catalog.rs"
check "Build Studio uses only the authenticated user's explicitly selected core engine" "rg -Fq 'crate::resident::resident_generate_protected_context(' crates/goblins-os-core/src/app_builder.rs && ! rg -q 'GOBLINS_OS_AGENTS_SDK_RELAY_URL|OPENAI_OS_AGENTS_SDK_RELAY_URL|official-openai-agents-sdk' crates/goblins-os-core/src/app_builder.rs && rg -Fq 'let user_id = client.map(|client| client.user_id());' crates/goblins-os-core/src/resident.rs && rg -Fq 'let route = resolve_resident_route_for(user_id)' crates/goblins-os-core/src/resident.rs && rg -Fq '.map(crate::openai_key::selected_engine_for)' crates/goblins-os-core/src/resident.rs && rg -Fq 'assert_eq!(route.engine_label(), selection.as_id());' crates/goblins-os-core/src/resident.rs && rg -Fq 'always uses the explicitly selected Goblins AI engine' crates/goblins-os-core/src/service_catalog.rs && ! rg -Fq 'OpenAI-centered Linux OS' crates/goblins-os-core/src/app_builder.rs"
check "Codex local chat wire is loopback-only compatibility" "rg -q 'This compatibility wire is local-only' os/codex/config.toml && rg -q 'base_url = \"http://127.0.0.1:11434/v1\"' os/codex/config.toml && rg -q 'wire_api = \"chat\"' os/codex/config.toml"
check "bootc image declares OCI source and license labels" "rg -Fq 'org.opencontainers.image.title=\"Goblins OS\"' os/bootc/Containerfile && rg -Fq 'org.opencontainers.image.source=\"https://github.com/Joe-Simo/goblins-os\"' os/bootc/Containerfile && rg -Fq 'org.opencontainers.image.url=\"https://goblinsos.com\"' os/bootc/Containerfile && rg -Fq 'org.opencontainers.image.licenses=\"AGPL-3.0-or-later\"' os/bootc/Containerfile"
check "browser core URL is absent from the desktop session" "rg -Fq 'GOBLINS_OS_CORE_PORT=8787' os/etc/goblins-os/environment && ! rg -Fq 'GOBLINS_OS_CORE_URL=' os/etc/goblins-os/environment && ! rg -Fq 'OPENAI_OS_' os/etc/goblins-os/environment && ! rg -Fq 'GOBLINS_OS_CORE_URL' os/session/goblins-os-session && ! rg -Fq 'OPENAI_OS_CORE_URL' os/session/goblins-os-session && rg -Fq 'std::env::var(\"GOBLINS_OS_CORE_PORT\")' crates/goblins-os-core/src/main.rs && rg -Fq 'std::env::var(\"OPENAI_OS_CORE_PORT\")' crates/goblins-os-core/src/main.rs && rg -Fq 'tcp_surface_default_denies_every_native_api_route' crates/goblins-os-core/src/control_plane.rs"
check "session bridge is source-gated for desktop user operations" "rg -Fq 'GOBLINS_OS_SESSION_BRIDGE_SOCKET=/run/goblins-os-session/session-bridge.sock' os/etc/goblins-os/environment && rg -Fq 'COPY os/tmpfiles/goblins-os-session.conf /usr/lib/tmpfiles.d/goblins-os-session.conf' os/bootc/Containerfile && rg -Fq 'd /run/goblins-os-session 0770 goblin goblins-session-bridge -' os/tmpfiles/goblins-os-session.conf && rg -Fq 'goblins-os-session-bridge' os/bootc/Containerfile && rg -Fq 'COPY --from=rust-build /out/ /' os/bootc/Containerfile && rg -Fq 'COPY --from=os-assets / /' os/bootc/Containerfile && rg -Fq 'goblins-os-session-bridge --self-test' os/bootc/Containerfile && rg -Fq 'groupadd --system goblins-session-bridge' os/bootc/Containerfile && rg -Fq 'SupplementaryGroups=goblins-session-bridge' os/systemd/goblins-os-core.service && rg -Fq 'ExecStart=/usr/libexec/goblins-os/goblins-os-session-bridge' os/systemd-user/org.goblins.OS.SessionBridge.service && rg -Fq 'Wants=org.goblins.OS.SessionBridge.service' os/systemd-user/gnome-session@goblins-os.target.d/goblins-os.session.conf && rg -Fq 'Wants=org.goblins.OS.Shell.service' os/systemd-user/gnome-session@goblins-os.target.d/goblins-os.session.conf && ! rg -Fq 'Requires=org.goblins.OS.Shell.target' os/systemd-user/gnome-session@goblins-os.target.d/goblins-os.session.conf && ! rg -Fq 'org.goblins.OS.Shell' os/gnome-session/goblins-os.session && rg -Fq 'UnixStream::connect' crates/goblins-os-core/src/session_bridge.rs && rg -Fq 'permission_store_delete_permission' crates/goblins-os-core/src/session_bridge.rs && rg -Fq 'display_config_apply_monitors' crates/goblins-os-core/src/session_bridge.rs && rg -Fq 'display_config_get_current_state' crates/goblins-os-core/src/displays.rs && rg -Fq 'non-allowlisted schema was accepted' crates/goblins-os-session-bridge/src/main.rs && rg -Fq 'PermissionStoreDelete' crates/goblins-os-session-bridge/src/main.rs && rg -Fq 'PermissionStore deletes are limited to app-keyed tables' crates/goblins-os-session-bridge/src/main.rs && rg -Fq 'DisplayConfigApplyMonitors' crates/goblins-os-session-bridge/src/main.rs && rg -Fq 'validate_display_config_logical_monitors' crates/goblins-os-session-bridge/src/main.rs"
check "shell user service exports no browser core URL" "! rg -q 'Environment=(GOBLINS_OS|OPENAI_OS)_CORE_URL' os/systemd-user/org.goblins.OS.Shell.service"
check "desktop clients use the capability client without URL overrides" "rg -Fq 'goblins_os_core_client' crates/goblins-os-installer/src/main.rs crates/goblins-os-login/src/main.rs crates/goblins-os-shell/src/main.rs crates/goblins-os-settings/src/main.rs crates/goblins-os-launcher/src/main.rs crates/goblins-os-control-center/src/main.rs crates/goblins-os-open/src/main.rs crates/goblins-os-file-builder/src/main.rs crates/goblins-os-resident/src/main.rs && ! rg -q '(GOBLINS_OS|OPENAI_OS)_CORE_(URL|SOCKET)' crates/goblins-os-installer/src/main.rs crates/goblins-os-login/src/main.rs crates/goblins-os-shell/src/main.rs crates/goblins-os-settings/src/main.rs crates/goblins-os-launcher/src/main.rs crates/goblins-os-control-center/src/main.rs crates/goblins-os-open/src/main.rs crates/goblins-os-file-builder/src/main.rs crates/goblins-os-resident/src/main.rs"
check "release proof capability is server-only and root-operated" "rg -Fq 'const RELEASE_PROOF_PERMISSIONS' crates/goblins-os-core/src/control_plane.rs && rg -Fq 'Self::ReleaseProof => \"release-proof\"' crates/goblins-os-core/src/control_plane.rs && rg -Fq 'release_proof_is_server_only_and_default_denied_outside_its_manifest' crates/goblins-os-core/src/control_plane.rs && rg -Fq 'resolve_expected_group_id' crates/goblins-os-core/src/control_plane.rs && rg -Fq 'production_directory_validation_rejects_swapped_unique_group_ids' crates/goblins-os-core/src/control_plane.rs && rg -Fq 'libc::SO_PEERCRED' crates/goblins-os-core/src/control_plane.rs && rg -Fq 'libc::SO_PEERGROUPS' crates/goblins-os-core/src/control_plane.rs && rg -Fq 'peer_group_authorization_never_bypasses_for_shared_or_root_uid' crates/goblins-os-core/src/control_plane.rs && rg -Fq 'd /run/goblins-os-core/release-proof 2750 goblins-os goblins-core-release-proof -' os/tmpfiles/goblins-os-core.conf os/bootc/Containerfile && rg -Fq 'getent group goblins-core-release-proof' os/bootc/Containerfile && ! rg -Fq 'ReleaseProof' crates/goblins-os-core-client && ! rg -q 'release-proof:' os/bootc/Containerfile && ! rg -q '(usermod|useradd).*goblins-core-release-proof' os/bootc/Containerfile"
check "canonical proof scripts use the release-proof Unix capability" "(for script in os/bootc/render-screens.sh os/bootc/render-desktop.sh os/bootc/run-selftest.sh os/runtime-gate/build-an-app-live-model.sh os/hardware-gate/capture-harness/firstboot-unlock.sh os/hardware-gate/capture-harness/core-proof-operation.sh; do rg -Fq '/run/goblins-os-core/release-proof/control.sock' \"\$script\" && rg -Fq 'curl' \"\$script\" && rg -Fq -- '--unix-socket' \"\$script\" && rg -Fq 'setpriv --regid=goblins-core-release-proof --clear-groups --' \"\$script\" || exit 1; done) && ! rg -q '(GOBLINS_OS|OPENAI_OS)_CORE_URL|127[.]0[.]0[.]1:8788|localhost:8788|8787/v1' os/bootc/render-screens.sh os/bootc/render-desktop.sh os/bootc/run-selftest.sh os/runtime-gate/build-an-app-live-model.sh os/hardware-gate/capture-harness os/iso/verify-config.toml && rg -Fq 'CORE_HEALTH_URL=http://127.0.0.1:8787/health' os/hardware-gate/capture-harness/in-session-orchestrator.sh"
check "root proof launches preserve the core service identity" "(for script in os/bootc/render-screens.sh os/bootc/render-desktop.sh os/bootc/run-selftest.sh os/runtime-gate/build-an-app-live-model.sh; do rg -Fq 'systemd-tmpfiles --create /usr/lib/tmpfiles.d/goblins-os-core.conf' \"\$script\" && rg -Fq 'setpriv --reuid=goblins-os --regid=goblins-os --init-groups' \"\$script\" || exit 1; done)"
check "installer proof page override bypasses completed first-boot exit" "rg -Fq 'should_exit_after_first_boot(first_boot_completed, installer_page_override_requested())' crates/goblins-os-installer/src/main.rs && rg -Fq 'GOBLINS_OS_INSTALLER_PAGE' crates/goblins-os-installer/src/main.rs"

check "rust job checks fmt" "rg -q 'cargo fmt --all --check' \"$WORKFLOW\""
check "rust job checks clippy" "rg -q 'clippy --workspace' \"$WORKFLOW\""
check "rust job checks native desktop tests" 'rg -q --fixed-strings '\''cargo test --workspace --features "$NATIVE_FEATURES"'\'' "$WORKFLOW"'
check "native desktop feature inventory is identical across CI and local gates" "for feature in goblins-os-installer/native-desktop goblins-os-control-center/native-desktop goblins-os-consent-broker/native-desktop goblins-os-openai-key-broker/native-desktop goblins-os-launcher/native-desktop goblins-os-login/native-desktop goblins-os-markup/native-desktop goblins-os-settings/native-desktop goblins-os-shell/native-desktop goblins-os-today/native-desktop goblins-os-ui/native-desktop goblins-os-visual-lookup/native-desktop; do rg -Fq \"\$feature\" \"$WORKFLOW\" && rg -Fq \"\$feature\" os/bootc/gate.Dockerfile && rg -Fq \"\$feature\" os/bootc/Containerfile || exit 1; done"
check "rust job checks release" "rg -q 'cargo build --release --workspace' \"$WORKFLOW\""
check "image job has verify" "rg -q 'goblins-os-verify' \"$WORKFLOW\""
check "image job checks blocked=0" "rg -q 'blocked=0' \"$WORKFLOW\""
check "image job has selftest" "rg -q 'selftest.suffix.Dockerfile' \"$WORKFLOW\" && rg -q 'target: selftest' \"$WORKFLOW\""
check "image job renders settings interaction proof" "rg -Fq 'GOBLINS_OS_RENDER_SCOPE=settings-interactions' \"$WORKFLOW\" && rg -Fq 'goblins-os-settings-interactions-' \"$WORKFLOW\""
check "image job has explicit push marker trigger" "rg -Fq \"contains(github.event.head_commit.message, '[image]')\" \"$WORKFLOW\" && rg -Fq \"github.event_name == 'push' && contains(github.event.head_commit.message, '[image]')\" \"$WORKFLOW\""
check "CI suffixes avoid extra chmod run layers" "rg -Fq 'COPY --chmod=0755 os/bootc/run-selftest.sh' os/bootc/selftest.suffix.Dockerfile && ! rg -Fq 'RUN chmod +x /usr/libexec/goblins-os-ci/run-selftest' os/bootc/selftest.suffix.Dockerfile && rg -Fq 'COPY --chmod=0755 os/bootc/render-screens.sh' os/bootc/render.suffix.Dockerfile && ! rg -Fq 'RUN chmod +x /usr/libexec/goblins-os-ci/render-screens' os/bootc/render.suffix.Dockerfile && rg -Fq 'COPY --chmod=0755 os/bootc/render-desktop.sh' os/bootc/render-desktop.suffix.Dockerfile && ! rg -Fq 'RUN chmod +x /usr/libexec/goblins-os-ci/render-desktop' os/bootc/render-desktop.suffix.Dockerfile"
check "installer-iso job exists" "rg -q '^  installer-iso:' \"$WORKFLOW\""
check "installer-iso job generates and hash-seals release evidence" "rg -q -- '--release-evidence /out' \"$WORKFLOW\" && rg -q 'goblins_os_release_evidence_hashes_match' \"$WORKFLOW\""
check "installer-iso job scans generated evidence for secrets" "rg -q 'goblins_os_artifact_secret_scan' \"$WORKFLOW\""
check "installer-iso job uploads release evidence artifacts" "rg -q 'goblins-os-release-evidence-' \"$WORKFLOW\""
check "workflow declares only the canonical aarch64 runner" "rg -q 'ubuntu-24.04-arm' \"$WORKFLOW\" && rg -q 'expected_uname: aarch64' \"$WORKFLOW\" && ! rg -q 'x86_64|amd64|linux/amd64' \"$WORKFLOW\""
check "workflow asserts native aarch64 runner architecture" "rg -q --fixed-strings 'Assert native runner architecture' \"$WORKFLOW\" && rg -q --fixed-strings 'test \"\$(uname -m)\" = \"\${{ matrix.expected_uname }}\"' \"$WORKFLOW\" && rg -q --fixed-strings 'expected_uname: aarch64' \"$WORKFLOW\""

check "architecture contract selects exactly aarch64" "rg -Fxq 'supported = [\"aarch64\"]' os/release/architectures.toml && rg -q '^\[artifacts[.]aarch64\]$' os/release/architectures.toml && [ \"\$(rg -c '^\[artifacts[.]' os/release/architectures.toml)\" = 1 ]"
check "architecture contract records aarch64 artifact paths" "rg -q 'os/iso/output/aarch64/bootiso/goblins-os-aarch64\\.iso' os/release/architectures.toml && rg -q 'os/iso/output/aarch64/manifest-goblins-os-aarch64\\.json' os/release/architectures.toml"
check "architecture contract records the aarch64 SBOM path" "rg -q 'os/signoff-proofs/sbom/aarch64/rpm-packages\\.tsv' os/release/architectures.toml"
check "architecture contract records the aarch64 QEMU command" "rg -q 'qemu-system-aarch64' os/release/architectures.toml"
check "architecture contract records aarch64 UEFI pflash contract" "rg -q 'AARCH64_UEFI_CODE' os/release/architectures.toml && rg -q 'AARCH64_UEFI_VARS' os/release/architectures.toml"
check "architecture contract makes native aarch64 Linux authoritative for packaging" "rg -q '^\[authority[.]native_packaging\]$' os/release/architectures.toml && rg -q '^host_os = \"Linux\"$' os/release/architectures.toml && rg -q '^host_architecture = \"aarch64\"$' os/release/architectures.toml && rg -q '^execution = \"native\"$' os/release/architectures.toml && rg -q 'Native aarch64 Linux runner; emulated builds are not a release baseline' os/release/architectures.toml"
check "architecture contract confines Linux KVM to optional non-signoff diagnostics" "rg -q '^\[diagnostic[.]linux_kvm\]$' os/release/architectures.toml && rg -q '^qemu_machine = \"virt,accel=kvm,gic-version=max\"$' os/release/architectures.toml && rg -q '^optional = true$' os/release/architectures.toml && rg -q '^satisfies_display_signoff = false$' os/release/architectures.toml && rg -q 'KVM can never satisfy display signoff' os/release/architectures.toml"
check "architecture contract makes Darwin arm64 HVF the final display authority" "rg -q '^\[authority[.]display_signoff\]$' os/release/architectures.toml && rg -q '^host_os = \"Darwin\"$' os/release/architectures.toml && rg -q '^host_architecture = \"arm64\"$' os/release/architectures.toml && rg -q '^qemu_machine = \"virt,accel=hvf,gic-version=max\"$' os/release/architectures.toml && rg -q '^accelerator = \"hvf\"$' os/release/architectures.toml && rg -q 'Final display-backed capture and signoff' os/release/architectures.toml"
check "architecture contract rejects an x86 compatibility claim" "rg -q 'does not ship or claim an x86 compatibility layer' os/release/architectures.toml"
check "architecture contract limits compatibility to proved configurations" "rg -q '^\[compatibility\]$' os/release/architectures.toml && rg -q 'No bare-metal Arm device is supported until that exact model has current' os/release/architectures.toml && rg -q 'Apple Silicon is a local HVF proof host, not a claimed bare-metal Goblins OS install target' os/release/architectures.toml"

check "ISO builder supports GOBLINS_OS_ARCH" "rg -q 'GOBLINS_OS_ARCH' os/iso/build-iso.sh"
check "ISO builder writes architecture ISO names" "rg -q 'goblins-os-\\\$ARCH.iso' os/iso/build-iso.sh"
check "ISO builder host runtime is Docker-only" "rg -q \"expected docker\" os/iso/build-iso.sh && ! rg -q 'docker or podman' os/iso/build-iso.sh && ! rg -q 'GOBLINS_OS_PODMAN_SUDO' os/iso/build-iso.sh && ! rg -q 'run_podman_builder' os/iso/build-iso.sh"
check "ISO builder uses distinct labeled user-defined Docker bridges for registry isolation and BIB egress" "rg -Fq 'GOBLINS_OS_DOCKER_EGRESS_NETWORK' os/iso/build-iso.sh && rg -Fq 'must name distinct Docker networks' os/iso/build-iso.sh && rg -Fq 'must name a user-defined Docker network' os/iso/build-iso.sh && rg -Fq -- '--label org.goblins-os.purpose=installer-registry-handoff' os/iso/build-iso.sh && rg -Fq -- '--label org.goblins-os.purpose=installer-builder-egress' os/iso/build-iso.sh && rg -Fq 'dedicated internal registry bridge contract' os/iso/build-iso.sh && rg -Fq 'dedicated non-internal BIB egress bridge contract' os/iso/build-iso.sh && rg -Fq '{{.Scope}}' os/iso/build-iso.sh && rg -Fq 'assert_dedicated_registry_network_membership true' os/iso/build-iso.sh && rg -Fq 'assert_dedicated_egress_network_membership' os/iso/build-iso.sh"
check "ISO builder proves and cleans the exact two-user-defined-network capability" "rg -Fq -- '--label org.goblins-os.purpose=installer-network-preflight-egress' os/iso/build-iso.sh && rg -Fq -- '--label org.goblins-os.purpose=installer-network-preflight-registry' os/iso/build-iso.sh && rg -Fq -- '--network \"name=\$preflight_egress_network,gw-priority=1\"' os/iso/build-iso.sh && rg -Fq 'docker start -a \"\$preflight_container_id\"' os/iso/build-iso.sh && iso_builder_preflight_lifecycle_is_ordered && rg -Fq 'does not materialize endpoint NetworkIDs until the container starts' os/iso/build-iso.sh && rg -Fq 'egress_priority\" != \"1' os/iso/build-iso.sh && rg -Fq 'registry_priority\" != \"0' os/iso/build-iso.sh && rg -Fq 'egress_scope\" != \"local' os/iso/build-iso.sh && rg -Fq 'registry_scope\" != \"local' os/iso/build-iso.sh && rg -Fq 'expected_egress_network_id\" != \"\$preflight_egress_network_id' os/iso/build-iso.sh && rg -Fq 'expected_registry_network_id\" != \"\$preflight_registry_network_id' os/iso/build-iso.sh && rg -Fq 'bounded_docker_remove \"\$preflight_container_id\"' os/iso/build-iso.sh && rg -Fq 'bounded_docker_network_remove \"\$preflight_egress_network_id\"' os/iso/build-iso.sh && rg -Fq 'bounded_docker_network_remove \"\$preflight_registry_network_id\"' os/iso/build-iso.sh && rg -Fq 'preflight egress-network cleanup did not complete' os/iso/build-iso.sh && rg -Fq 'preflight registry-network cleanup did not complete' os/iso/build-iso.sh && ! rg -Fq -- '--network \"name=bridge,gw-priority=1\"' os/iso/build-iso.sh"
check "ISO builder applies one exact dual-network argument set to the probe and BIB" "rg -Fq -- '--network \"name=\$DOCKER_EGRESS_NETWORK,gw-priority=1\"' os/iso/build-iso.sh && rg -Fq -- '--network \"\$DOCKER_REGISTRY_NETWORK\"' os/iso/build-iso.sh && rg -Fq 'probe_docker_registry_from_builder_network \"\${bib_network_args[@]}\"' os/iso/build-iso.sh && rg -Fq '\"\${network_args[@]}\"' os/iso/build-iso.sh && rg -Fq '\${bib_network_args[@]+\"\${bib_network_args[@]}\"}' os/iso/build-iso.sh && rg -Fq 'Public remote images intentionally use Docker' os/iso/build-iso.sh && rg -Fq 'Recheck immediately before the privileged builder attaches' os/iso/build-iso.sh && rg -Fq -- '--rm must restore both managed networks' os/iso/build-iso.sh && rg -Fq -- '-p \"127.0.0.1:\$DOCKER_REGISTRY_PORT:5000\"' os/iso/build-iso.sh && rg -Fq -- '-p \"[::1]:\$DOCKER_REGISTRY_PORT:5000\"' os/iso/build-iso.sh && rg -Fq 'printf '\''127.0.0.1 %s\\n::1 %s\\n'\''' os/iso/build-iso.sh && rg -Fq 'registry_image=\"localhost:\$DOCKER_REGISTRY_PORT/goblins-os:\$ARCH\"' os/iso/build-iso.sh && rg -Fq 'builder_image=\"\$DOCKER_REGISTRY_NAME:5000/goblins-os:\$ARCH\"' os/iso/build-iso.sh && rg -Fq 'requires Docker 28 or newer on both client and server' os/iso/build-iso.sh && rg -Fq 'trap cleanup_registry_probe EXIT' os/iso/build-iso.sh && rg -Fq 'docker run --rm --pull=never' os/iso/build-iso.sh && rg -q 'docker push' os/iso/build-iso.sh && ! rg -q -- '--rm -it' os/iso/build-iso.sh"
check "ISO builder scopes local registry override routes without weakening remote pulls" "rg -Fq 'host-gateway is intentionally available only for this explicit override' os/iso/build-iso.sh && rg -Fq 'bib_host_args=(--add-host=host.docker.internal:host-gateway)' os/iso/build-iso.sh && rg -Fq 'uses container loopback and cannot reach a host registry from BIB' os/iso/build-iso.sh && rg -Fq 'GOBLINS_OS_DOCKER_REGISTRY_NAME cannot be localhost' os/iso/build-iso.sh && rg -Fq 'uses an unsupported local registry alias' os/iso/build-iso.sh && rg -Fq 'LOCAL_REGISTRY_IMAGE=\"registry:2\"' os/iso/build-iso.sh"
check "installer local-ref classifier rejects non-public routes without blocking public registries" "installer_local_ref_classifier_passes"
check "ISO builder separates local Docker handoff from digest-pinned shippable source" "rg -q 'GOBLINS_OS_BIB_SOURCE_IMAGE' os/iso/build-iso.sh && rg -q 'GOBLINS_OS_SHIPPABLE_RELEASE' os/iso/build-iso.sh && rg -q 'shippable release media requires a digest-pinned installer payload ref' os/iso/build-iso.sh"
check "ISO builder independently verifies the exact shippable ARM64 candidate image" "rg -q 'verify_shippable_candidate_image' os/iso/build-iso.sh && rg -q 'approved ghcr[.]io/joe-simo/goblins-os image repository' os/iso/build-iso.sh && rg -q 'docker pull --platform \"[$]DOCKER_PLATFORM\" \"[$]ref\"' os/iso/build-iso.sh && rg -q 'org[.]opencontainers[.]image[.]revision' os/iso/build-iso.sh && rg -q 'does not match selected commit' os/iso/build-iso.sh"
check "ISO builder can skip local image export for shippable registry source" "rg -q 'GOBLINS_OS_SKIP_LOCAL_IMAGE_BUILD' os/iso/build-iso.sh && rg -q 'Skipping local Docker image build' os/iso/build-iso.sh && rg -q 'requires GOBLINS_OS_BIB_SOURCE_IMAGE' os/iso/build-iso.sh"
check "ISO builder supports explicit installer config" "rg -q 'GOBLINS_OS_ISO_CONFIG' os/iso/build-iso.sh && rg -q '\"installer_config\": \"[$]CONFIG_LABEL\"' os/iso/build-iso.sh"
check "ISO builder records the explicit ARM64 Docker platform" "rg -q 'GOBLINS_OS_DOCKER_PLATFORM' os/iso/build-iso.sh && rg -q 'docker build --platform \"[$]DOCKER_PLATFORM\"' os/iso/build-iso.sh && rg -q -- '--platform \"[$]DOCKER_PLATFORM\"' os/iso/build-iso.sh && rg -q '\"docker_platform\": \"[$]DOCKER_PLATFORM\"' os/iso/build-iso.sh"
check "ISO builder rejects architecture emulation" "rg -q 'GOBLINS_OS_ALLOW_EMULATED_DOCKER is not supported' os/iso/build-iso.sh && rg -q 'requires a native aarch64 host' os/iso/build-iso.sh && rg -q 'requires a native [$]ARCH container engine' os/iso/build-iso.sh && ! rg -q 'verify_docker_emulation_runtime|emulation cannot run rustc' os/iso/build-iso.sh"
check "ISO builder restricts shippable packaging authority to native ARM64 Linux" "rg -q 'shippable [$]ARCH media requires a native aarch64 Linux host' os/iso/build-iso.sh && rg -q '\"native_host_os\": \"[$]HOST_OS\"' os/iso/build-iso.sh"
check "workflow installer ISO uses cached Buildx image and evidence steps" "rg -q --fixed-strings 'docker/build-push-action@53b7df96c91f9c12dcc8a07bcb9ccacbed38856a' \"$WORKFLOW\" && rg -q 'load: true' \"$WORKFLOW\" && rg -q 'docker run --rm' \"$WORKFLOW\" && rg -q 'GOBLINS_OS_CONTAINER_RUNTIME=docker' \"$WORKFLOW\""
check "workflow image builds use nonblocking BuildKit GHA cache" "rg -q --fixed-strings 'docker/setup-buildx-action@bb05f3f5519dd87d3ba754cc423b652a5edd6d2c' \"$WORKFLOW\" && rg -q --fixed-strings 'type=gha,scope=goblins-os-bootc-\${{ matrix.arch }}' \"$WORKFLOW\" && rg -q 'mode=max,ignore-error=true' \"$WORKFLOW\""
check "aarch64 verification consumes an immutable candidate image without channel promotion" "rg -q 'candidate_image_ref' .github/workflows/aarch64-verification-iso.yml && rg -q 'docker pull \"[$]GOBLINS_OS_CANDIDATE_IMAGE_REF\"' .github/workflows/aarch64-verification-iso.yml && rg -q 'GOBLINS_OS_SKIP_LOCAL_IMAGE_BUILD=1' .github/workflows/aarch64-verification-iso.yml && ! rg -q 'docker/build-push-action|push: true|goblins-os:(aarch64|latest|stable)' .github/workflows/aarch64-verification-iso.yml"
check "aarch64 proof input is restricted to this repository image" "rg -q 'expected_prefix=\"ghcr[.]io/[$]owner/goblins-os@sha256:\"' .github/workflows/aarch64-verification-iso.yml"
check "aarch64 verification uses the verification ISO config" "rg -q 'GOBLINS_OS_ISO_CONFIG=os/iso/verify-config.toml' .github/workflows/aarch64-verification-iso.yml"
check "aarch64 verification ISO workflow supports local HVF capture" "test -f .github/workflows/aarch64-verification-iso.yml && rg -q 'workflow_dispatch' .github/workflows/aarch64-verification-iso.yml && rg -q 'ubuntu-24.04-arm' .github/workflows/aarch64-verification-iso.yml && rg -q 'GOBLINS_OS_ISO_CONFIG=os/iso/verify-config.toml' .github/workflows/aarch64-verification-iso.yml && rg -q 'goblins-os-aarch64-verification-iso' .github/workflows/aarch64-verification-iso.yml && rg -q 'retention-days: 7' .github/workflows/aarch64-verification-iso.yml"
check "aarch64 verification generates and secret-scans release evidence" 'rg -q "Generate release evidence for the captured image" .github/workflows/aarch64-verification-iso.yml && rg -q -- "--release-evidence /out" .github/workflows/aarch64-verification-iso.yml && rg -q "rpm_sbom_arch_matches" .github/workflows/aarch64-verification-iso.yml && rg -q "Scan generated release evidence for secrets" .github/workflows/aarch64-verification-iso.yml'
check "retained aarch64 proof workflows are artifact-only and cannot write the repository" "rg -q --fixed-strings 'actions/upload-artifact@043fb46d1a93c77aae656e7c1c64a875d1fc6a0a' .github/workflows/aarch64-verification-iso.yml .github/workflows/aarch64-local-display-attestation.yml && rg -q 'contents: read' .github/workflows/aarch64-verification-iso.yml .github/workflows/aarch64-local-display-attestation.yml && ! rg -q 'contents: write|git push|persist_evidence' .github/workflows/aarch64-verification-iso.yml .github/workflows/aarch64-local-display-attestation.yml"
check "hardware evidence bundle is canonical, bounded, duplicate-safe, and uniform" "test -f os/hardware-gate/capture-harness/evidence_bundle.py && rg -Fq 'goblins-os-hardware-evidence-bundle-v5' os/hardware-gate/capture-harness/evidence_bundle.py && rg -Fq '40-accessibility-window-resize.png' os/hardware-gate/capture-harness/evidence_bundle.py os/hardware-gate/close-signoff.sh os/hardware-gate/verify-shipping-status.sh && rg -Fq '41-hosted-context-review.png' os/hardware-gate/capture-harness/evidence_bundle.py os/hardware-gate/close-signoff.sh os/hardware-gate/verify-shipping-status.sh && rg -Fq '42-hosted-context-review-dark.png' os/hardware-gate/capture-harness/evidence_bundle.py os/hardware-gate/close-signoff.sh os/hardware-gate/verify-shipping-status.sh && rg -Fq 'len(REQUIRED_PNGS) != 42' os/hardware-gate/capture-harness/evidence_bundle.py && rg -Fq 'reject_duplicate_keys' os/hardware-gate/capture-harness/evidence_bundle.py && rg -Fq 'O_NOFOLLOW' os/hardware-gate/capture-harness/evidence_bundle.py && rg -Fq 'evidence screenshots do not share one framebuffer size' os/hardware-gate/capture-harness/evidence_bundle.py"
check "hardware evidence seal binds Darwin ARM64 HVF, exact QEMU, ISO, and screenshots" "rg -Fq 'goblins-os-hardware-evidence-bundle-v5' os/hardware-gate/capture-harness/evidence_bundle.py && rg -Fq 'goblins-os-aarch64-local-display-authority-v2' os/hardware-gate/capture-harness/evidence_bundle.py && rg -Fq '\"authority_generation\": 2' os/hardware-gate/capture-harness/evidence_bundle.py && rg -q '^    runs-on: ubuntu-24[.]04-arm$' .github/workflows/aarch64-local-display-attestation.yml && rg -Fq 'test \"\$(uname -m)\" = aarch64' .github/workflows/aarch64-local-display-attestation.yml && rg -Fq 'capture_environment' os/hardware-gate/capture-harness/evidence_bundle.py os/hardware-gate/capture-harness/proof_validation.py os/hardware-gate/capture-harness/run-capture.sh && rg -Fq '\"host_os\": \"Darwin\"' os/hardware-gate/capture-harness/evidence_bundle.py && rg -Fq '\"accelerator\": \"hvf\"' os/hardware-gate/capture-harness/evidence_bundle.py && rg -Fq 'qemu_binary_sha256' os/hardware-gate/capture-harness/evidence_bundle.py os/hardware-gate/capture-harness/run-capture.sh && rg -Fq 'verification_iso_sha256' os/hardware-gate/capture-harness/evidence_bundle.py && rg -Fq 'screenshot_manifest_sha256' os/hardware-gate/capture-harness/evidence_bundle.py"
check "hardware proof validator adversarial self-test passes" "python3 os/hardware-gate/capture-harness/proof_validation.py --self-test >/dev/null"
check "visual secret scan binds sealed PNG bytes and uses pinned tiled Apple Vision OCR" "test -f os/hardware-gate/capture-harness/visual-secret-scan.swift && rg -Fq 'import Vision' os/hardware-gate/capture-harness/visual-secret-scan.swift && rg -Fq 'VNRecognizeTextRequestRevision3' os/hardware-gate/capture-harness/visual-secret-scan.swift && rg -Fq 'screenshot bytes do not match the signed canonical PNG entry' os/hardware-gate/capture-harness/visual-secret-scan.swift && rg -Fq -- '--seal \"\$RUN_DIR/evidence-bundle.json\"' os/hardware-gate/capture-harness/run-capture.sh && rg -Fq -- '--seal \"\$display_run/evidence-bundle.json\"' os/release/promote-stable.sh"
check "source workflows satisfy the protected publisher boundary" "os/release/verify-publisher-boundary.sh >/dev/null"
check "source workflows are read-only, secret-free, and non-promotional" "! rg -q --glob '*.yml' 'packages:[[:space:]]*write|contents:[[:space:]]*write|^[[:space:]]*environment:[[:space:]]*stable([[:space:]]|$)|[$][{][{][[:space:]]*secrets[.]' .github/workflows && rg -Fq 'source_repository_publish_authority: false' .github/workflows/candidate-artifacts.yml .github/workflows/branding-tool-image.yml .github/workflows/stable-promotion.yml && rg -Fq 'non_promotional: true' .github/workflows/candidate-artifacts.yml .github/workflows/branding-tool-image.yml .github/workflows/stable-promotion.yml"
check "source workflows contain no GHCR, tag, or Release write path" "! rg -q --glob '*.yml' 'docker[[:space:]]+login|docker/login-action|docker[[:space:]]+push|podman[[:space:]]+push|oras[[:space:]]+push|push:[[:space:]]*true|buildx[[:space:]]+imagetools[[:space:]]+create|gh[[:space:]]+release|git[[:space:]]+push|GHCR_TOKEN|write:packages' .github/workflows"
check "candidate and branding workflows seal attempt-scoped native ARM64 OCI handoffs" "rg -Fq 'schema: \"goblins-os-source-oci-handoff-v1\"' .github/workflows/candidate-artifacts.yml && rg -Fq 'schema: \"goblins-os-installer-branding-tool-handoff-v1\"' .github/workflows/branding-tool-image.yml && rg -Fq 'schema: \"goblins-os-actions-artifact-envelope-v1\"' .github/workflows/candidate-artifacts.yml .github/workflows/branding-tool-image.yml && rg -Fq 'platform: \"linux/arm64\"' .github/workflows/candidate-artifacts.yml && rg -Fq 'oci_architecture: \"arm64\"' .github/workflows/candidate-artifacts.yml .github/workflows/branding-tool-image.yml && rg -Fq 'split -n 4 -d -a 2' .github/workflows/candidate-artifacts.yml .github/workflows/branding-tool-image.yml && rg -Fq 'aarch64-attempt-\${{ github.run_attempt }}-part-' .github/workflows/candidate-artifacts.yml .github/workflows/branding-tool-image.yml && rg -Fq 'aarch64-attempt-\${{ github.run_attempt }}-metadata' .github/workflows/candidate-artifacts.yml .github/workflows/branding-tool-image.yml && [ \"\$(rg -c '^      - name: Upload OCI payload part (00|01|02|03)$' .github/workflows/candidate-artifacts.yml)\" = 4 ] && [ \"\$(rg -c '^      - name: Upload branding-tool OCI payload part (00|01|02|03)$' .github/workflows/branding-tool-image.yml)\" = 4 ]"
check "stable source workflow emits one attempt-scoped sealed publisher request without publication authority" "rg -Fq 'schema: \"goblins-os-publisher-request-v1\"' .github/workflows/stable-promotion.yml && rg -Fq 'repository: \"Joe-Simo/goblins-os-publisher\"' .github/workflows/stable-promotion.yml && rg -Fq 'authenticate-source-run-and-artifact-digests' .github/workflows/stable-promotion.yml && rg -Fq 'copy-with-preserved-digests' .github/workflows/stable-promotion.yml && rg -Fq 'aarch64-attempt-\$CANDIDATE_RUN_ATTEMPT-part-\$suffix' .github/workflows/stable-promotion.yml && rg -Fq 'aarch64-attempt-\$BRANDING_RUN_ATTEMPT-part-\$suffix' .github/workflows/stable-promotion.yml && rg -Fq 'build-pinned-zstd-1.5.7-from-sha256-verified-source' .github/workflows/stable-promotion.yml && rg -Fq 'move-aarch64-and-stable-aliases-last' .github/workflows/stable-promotion.yml && [ \"\$(rg -c '^      - name: Upload the publisher request$' .github/workflows/stable-promotion.yml)\" = 1 ]"
check "stable release replay rejects hidden USTAR and zstd bytes" "rg -Fq 'display-proof raw USTAR bytes are not canonical' os/release/stable-promotion.py && rg -Fq 'display-proof zstd bytes are not canonical' os/release/stable-promotion.py && rg -Fq 'release ISO zstd bytes are not canonical' os/release/stable-promotion.py && rg -Fq 'self-test accepted hidden bytes in raw USTAR padding' os/release/stable-promotion.py && rg -Fq 'self-test accepted a skippable frame in display-proof zstd bytes' os/release/stable-promotion.py && rg -Fq 'self-test accepted a skippable frame in release ISO zstd bytes' os/release/stable-promotion.py && rg -Fq -- '--zstd \"\$PINNED_ZSTD\"' os/release/promote-stable.sh && rg -Fq 'eb33e51f49a15e023950cd7825ca74a4a2b43db8354825ac24fc1b7ee09e6fa3' os/release/PUBLISHER-BOUNDARY.md"
check "signoff and shipping recompute the exact hardware evidence seal" "rg -Fq 'evidence_bundle.py\" verify' os/hardware-gate/close-signoff.sh os/hardware-gate/verify-shipping-status.sh && rg -q -- '--run-directory \"[$]SCREENSHOT_DIR\" \"[$]REPO_ROOT\" \"[$]ARCH\"' os/hardware-gate/close-signoff.sh && rg -q -- '--manifest \"[$]manifest\" \"[$]arch\" \"[$]SELECTED_CANDIDATE_COMMIT\"' os/hardware-gate/verify-shipping-status.sh"
check "aarch64 native packaging proof is exact-candidate/date/attempt artifact-bound" "rg -q 'goblins-os-aarch64-native-packaging-gate-[$][{][{] inputs[.]candidate_commit [}][}]-[$][{][{] inputs[.]run_date [}][}]-attempt-[$][{][{] github[.]run_attempt [}][}]' .github/workflows/aarch64-verification-iso.yml && rg -Fq 'NATIVE_PACKAGING_GATE_ARTIFACT' os/hardware-gate/close-signoff.sh && rg -Fq 'goblins-os-aarch64-native-packaging-gate-[$]SELECTED_CANDIDATE_COMMIT-[$]run_date-attempt-[$]native_attempt' os/hardware-gate/verify-shipping-status.sh"
check "aarch64 local HVF proof uses isolated Authority 2 and GitHub is verification-only" "python3 os/hardware-gate/capture-harness/evidence_bundle.py verify-authority-certificate --certificate os/release/display-proof-authority2.pem --certificate-sha256 os/release/display-proof-authority2.sha256 --ca-certificate os/release/display-proof-authority2-ca.pem --ca-certificate-sha256 os/release/display-proof-authority2-ca.sha256 >/dev/null && rg -Fq 'verify-authority-certificate' os/hardware-gate/capture-harness/run-capture.sh && rg -Fq 'finalize-display-proof.sh' os/hardware-gate/capture-harness/run-capture.sh && rg -Fq 'exit 75' os/hardware-gate/capture-harness/run-capture.sh && rg -Fq '\"cms\",' os/hardware-gate/capture-harness/display-authority2.py && rg -Fq '\"-S\",' os/hardware-gate/capture-harness/display-authority2.py && rg -Fq 'display-proof-authority2-ca.sha256' os/hardware-gate/capture-harness/finalize-display-proof.sh && rg -Fq 'openssl' os/hardware-gate/capture-harness/evidence_bundle.py && rg -Fq -- '-nointern' os/hardware-gate/capture-harness/evidence_bundle.py && rg -Fq -- '--signature \"\$run_dir/aarch64-local-display-attestation.json.cms\"' os/hardware-gate/close-signoff.sh os/hardware-gate/verify-shipping-status.sh && rg -Fq 'Verify the pinned Authority 2 signature without minting authority' .github/workflows/aarch64-local-display-attestation.yml && rg -Fq 'authority_signature_base64' .github/workflows/aarch64-local-display-attestation.yml && rg -q '^permissions:$' .github/workflows/aarch64-local-display-attestation.yml && rg -q '^  contents: read$' .github/workflows/aarch64-local-display-attestation.yml && ! rg -q 'actions/attest@|attestations: write|id-token: write|create-attestation' .github/workflows/aarch64-local-display-attestation.yml && ! rg -q 'gh[[:space:]]+attestation[[:space:]]+verify' os/hardware-gate/close-signoff.sh os/hardware-gate/verify-shipping-status.sh"
check "GitHub workflows reject mutable major-version action tags" "! rg -q 'uses:[[:space:]]+[^[:space:]#]+@v[0-9]+([[:space:]#]|$)' .github/workflows"
check "GitHub workflows use only the reviewed immutable Node 24 action pins" "release_workflow_action_pins_are_reviewed"
check "GitHub workflows contain none of the retired Node 20 action pins" "release_workflow_deprecated_action_pins_are_absent"
check "capture harness binds exact source tooling and safe date scope" "rg -q 'Capture tooling checkout .* does not match candidate' os/hardware-gate/capture-harness/run-capture.sh && rg -q 'Run the capture harness from the exact candidate checkout' os/hardware-gate/capture-harness/run-capture.sh && rg -q 'RUN_DATE must be a real calendar date in YYYY-MM-DD form' os/hardware-gate/capture-harness/run-capture.sh && rg -q '[$]RUN_DIR.*[$]RUN_ROOT/[$]DATE' os/hardware-gate/capture-harness/run-capture.sh"
check "capture harness verifies and canonicalizes external candidate proof" "rg -q 'Capture ISO checksum mismatch' os/hardware-gate/capture-harness/run-capture.sh && rg -q 'GOBLINS_OS_CAPTURE_BIB_MANIFEST' os/hardware-gate/capture-harness/run-capture.sh && rg -q 'GOBLINS_OS_CAPTURE_RELEASE_EVIDENCE_DIR' os/hardware-gate/capture-harness/run-capture.sh && rg -q 'CANONICAL_ISO=' os/hardware-gate/capture-harness/run-capture.sh"
check "aarch64 proof workflows cancel superseded runs" "rg -q 'cancel-in-progress: true' .github/workflows/aarch64-verification-iso.yml .github/workflows/aarch64-local-display-attestation.yml"
check "aarch64 shipping capture requires Darwin ARM64 with HVF" "rg -q 'expected Apple-Silicon Darwin/arm64 with HVF' os/hardware-gate/capture-harness/run-capture.sh && rg -q 'ACCEL=hvf' os/hardware-gate/capture-harness/run-capture.sh && rg -q 'capture_environment' os/hardware-gate/capture-harness/run-capture.sh && ! rg -q 'native Linux capture requires readable/writable /dev/kvm|need native Linux/KVM or macOS/HVF' os/hardware-gate/capture-harness/run-capture.sh"
check "external gate supports qemu-system-aarch64" "rg -q 'qemu-system-aarch64' os/hardware-gate/run-external-gate.sh"
check "external gate passes container runtime to ISO builder" "rg -q 'GOBLINS_OS_CONTAINER_RUNTIME=\"[$]CONTAINER_RUNTIME\"' os/hardware-gate/run-external-gate.sh"
check "external gate host runtime is Docker-only" "rg -q 'GOBLINS_OS_CONTAINER_RUNTIME must be docker' os/hardware-gate/run-external-gate.sh && ! rg -q 'docker or podman' os/hardware-gate/run-external-gate.sh && ! rg -q 'GOBLINS_OS_PODMAN_SUDO' os/hardware-gate/run-external-gate.sh && ! rg -q 'sudo podman' os/hardware-gate/run-external-gate.sh"
check "external gate requires immutable bootc source image for shippable packaging" "rg -q 'Shippable ARM64 packaging requires GOBLINS_OS_BIB_SOURCE_IMAGE to an immutable pullable bootc image digest ref' os/hardware-gate/run-external-gate.sh && rg -q 'GOBLINS_OS_BIB_SOURCE_IMAGE=\"[$]BIB_SOURCE_IMAGE\"' os/hardware-gate/run-external-gate.sh && rg -q 'GOBLINS_OS_SHIPPABLE_RELEASE=\"[$]SHIPPABLE_RELEASE\"' os/hardware-gate/run-external-gate.sh"
check "external gate runs shipping evidence from the exact candidate digest" "rg -q 'Pulling and verifying exact candidate image' os/hardware-gate/run-external-gate.sh && rg -q 'org[.]opencontainers[.]image[.]revision' os/hardware-gate/run-external-gate.sh && rg -q 'IMAGE_NAME=\"[$]BIB_SOURCE_IMAGE\"' os/hardware-gate/run-external-gate.sh && rg -q 'EVIDENCE_IMAGE_REF=\"[$]BIB_SOURCE_IMAGE\"' os/hardware-gate/run-external-gate.sh && rg -q 'GOBLINS_OS_SKIP_LOCAL_IMAGE_BUILD=\"[$]SKIP_LOCAL_IMAGE_BUILD\"' os/hardware-gate/run-external-gate.sh"
check "runbook documents digest-pinned release image source" "rg -q 'RELEASE_IMAGE=<registry>/<namespace>/goblins-os@sha256:<64-hex-digest>' os/hardware-gate/runbook.md && rg -q '\"installer_payload_source_local_only\": false' os/hardware-gate/runbook.md"
check "external gate confines KVM to optional native diagnostics" "rg -q 'QEMU_ACCEL must be kvm' os/hardware-gate/run-external-gate.sh && rg -q '/dev/kvm' os/hardware-gate/run-external-gate.sh && rg -q 'optional Linux/KVM diagnostic' os/hardware-gate/run-external-gate.sh && rg -q 'does not satisfy the Darwin/arm64/HVF display-proof gate' os/hardware-gate/run-external-gate.sh"
check "external gate uses aarch64 UEFI pflash code and vars" "rg -q 'if=pflash,format=raw,readonly=on,file=[$]AARCH64_UEFI_CODE' os/hardware-gate/run-external-gate.sh && rg -q 'if=pflash,format=raw,file=[$]AARCH64_UEFI_VARS' os/hardware-gate/run-external-gate.sh"
check "external gate copies aarch64 UEFI vars template" "rg -q 'AARCH64_UEFI_VARS_TEMPLATE' os/hardware-gate/run-external-gate.sh && rg -q 'cp \"[$]template\" \"[$]AARCH64_UEFI_VARS\"' os/hardware-gate/run-external-gate.sh"
check "external gate requires Linux host for shippable packaging" "rg -q 'Shippable ARM64 packaging requires a native aarch64 Linux host' os/hardware-gate/run-external-gate.sh"
check "external gate fails non-native architecture before build" "rg -q 'Requested [$]ARCH gate on [$]HOST_ARCH host' os/hardware-gate/run-external-gate.sh && rg -q 'release packaging must be produced on a native aarch64 Linux runner' os/hardware-gate/run-external-gate.sh"
check "external gate rejects Docker architecture emulation" "rg -q 'GOBLINS_OS_ALLOW_EMULATED_DOCKER is not supported by the ARM64 release gate' os/hardware-gate/run-external-gate.sh && ! rg -q 'Docker-emulated [$]ARCH artifact testing|Docker artifact testing on a non-native machine' os/hardware-gate/run-external-gate.sh os/hardware-gate/runbook.md"
check "external gate fails low disk before build" "rg -q 'MIN_HOST_FREE_GB' os/hardware-gate/run-external-gate.sh && rg -q 'Repository filesystem needs at least' os/hardware-gate/run-external-gate.sh && rg -q 'VM scratch filesystem needs at least' os/hardware-gate/run-external-gate.sh"
check "external gate checks container runtime health before build" "rg -q 'CONTAINER_RUNTIME_HEALTH_TIMEOUT_SECS' os/hardware-gate/run-external-gate.sh && rg -q 'Checking [$]CONTAINER_RUNTIME health' os/hardware-gate/run-external-gate.sh && rg -q 'did not answer within' os/hardware-gate/run-external-gate.sh"
check "external gate has fail-closed preflight-only mode" "rg -q 'PREFLIGHT_ONLY=1' os/hardware-gate/run-external-gate.sh && rg -q 'Native [$]ARCH Linux packaging preflight passed; no artifact was built' os/hardware-gate/run-external-gate.sh && rg -q 'Docker artifact-only preflight passed for [$]ARCH on [$]HOST_ARCH; not release proof' os/hardware-gate/run-external-gate.sh && rg -q 'Darwin/arm64/HVF display proof is still required before signoff' os/hardware-gate/run-external-gate.sh && rg -q 'No image, ISO, SBOM, screenshot, or signoff artifact was generated' os/hardware-gate/run-external-gate.sh"
check "runbook documents external preflight command" "rg -q 'PREFLIGHT_ONLY=1 RUN_QEMU=0 GOBLINS_OS_SHIPPABLE_RELEASE=1 GOBLINS_OS_ARCH=\"[$]ARCH\"' os/hardware-gate/runbook.md && rg -q 'does not create shipping artifacts or satisfy proof by itself' os/hardware-gate/runbook.md"
check "runbook documents the canonical Apple Silicon HVF display route" "rg -q 'aarch64 Apple Silicon/HVF capture route' os/hardware-gate/runbook.md && rg -q 'capture-harness/run-capture.sh' os/hardware-gate/runbook.md && rg -q 'already materialized verification-only hardware-gate ISO' os/hardware-gate/runbook.md && rg -q 'hydrated public release media' os/hardware-gate/runbook.md && rg -q 'gh workflow run aarch64-verification-iso.yml' os/hardware-gate/runbook.md && rg -Fq 'GOBLINS_OS_CAPTURE_ISO=\"\$AARCH64_VERIFICATION_ISO\"' os/hardware-gate/runbook.md && rg -q 'KVM can never satisfy the display signoff gate' os/hardware-gate/runbook.md && rg -q 'virt,accel=hvf,gic-version=max' os/hardware-gate/runbook.md && ! rg -q 'qemu-system-aarch64 -machine virt,accel=kvm.*-display' os/hardware-gate/runbook.md"
check "external gate allows artifact-only mode without pretending proof is complete" "rg -q 'RUN_QEMU=0: built and verified artifacts only' os/hardware-gate/run-external-gate.sh"
check "external gate verifies ISO SHA256" "rg -Fq 'verify_sha256_file()' os/hardware-gate/run-external-gate.sh && rg -Fq 'verify_sha256_file \"\$sha_path\" \"\$(basename \"\$iso_path\")\"' os/hardware-gate/run-external-gate.sh"
check "external gate generates release evidence" "rg -q -- '--release-evidence /out' os/hardware-gate/run-external-gate.sh"
check "external gate requires RPM SBOM TSV" "rg -q 'rpm-packages.tsv' os/hardware-gate/run-external-gate.sh"
check "installer policy exposes dual-boot preservation path" "rg -q 'dual_boot_preservation' crates/goblins-os-core/src/install_targets.rs"
check "installer policy exposes dual-boot preflight" "rg -q 'dual_boot_preflight' crates/goblins-os-core/src/install_targets.rs"
check "installer policy exposes safe dual-boot route" "rg -q 'dual_boot_safe_route' crates/goblins-os-core/src/install_targets.rs && rg -q 'DualBootSafeRoute' crates/goblins-os-core/src/install_targets.rs && rg -q 'Install beside an existing OS' crates/goblins-os-core/src/install_targets.rs && rg -q 'Install Goblins OS Beside Another OS' crates/goblins-os-core/src/install_targets.rs && rg -q 'every filesystem that will be formatted' crates/goblins-os-core/src/install_targets.rs"
check "installer policy exposes simple install erase scope" "rg -q 'simple_install_scope' crates/goblins-os-core/src/install_targets.rs && rg -q 'blank internal disk' crates/goblins-os-core/src/install_targets.rs && rg -q 'formats the new Goblins OS root filesystem' crates/goblins-os-core/src/install_targets.rs"
check "installer policy exposes bootloader recovery guidance" "rg -q 'bootloader_recovery' crates/goblins-os-core/src/install_targets.rs && rg -q 'firmware boot options' crates/goblins-os-core/src/install_targets.rs"
check "installer policy exposes storage review checklist" "rg -q 'storage_review_checklist' crates/goblins-os-core/src/install_targets.rs && rg -q 'StorageReviewItem' crates/goblins-os-core/src/install_targets.rs"
check "installer policy exposes recommended install path choices" "rg -q 'install_path_options' crates/goblins-os-core/src/install_targets.rs && rg -q 'Keep my current OS' crates/goblins-os-core/src/install_targets.rs && rg -q 'Replace one blank disk' crates/goblins-os-core/src/install_targets.rs && rg -q 'Advanced storage' crates/goblins-os-core/src/install_targets.rs"
check "installer policy exposes pre-write boot formatting plan" "rg -q 'pre_write_install_plan' crates/goblins-os-core/src/install_targets.rs && rg -q 'InstallPlanItem' crates/goblins-os-core/src/install_targets.rs && rg -q 'fresh GPT layout' crates/goblins-os-core/src/install_targets.rs && rg -q 'bootloader/EFI target' crates/goblins-os-core/src/install_targets.rs && rg -q 'Btrfs root' crates/goblins-os-core/src/install_targets.rs && rg -q 'TPM2 LUKS' crates/goblins-os-core/src/install_targets.rs"
check "installer policy exposes dual-boot readiness checklist" "rg -q 'dual_boot_readiness' crates/goblins-os-core/src/install_targets.rs && rg -q 'DualBootReadinessItem' crates/goblins-os-core/src/install_targets.rs && rg -q 'Windows readiness' crates/goblins-os-core/src/install_targets.rs && rg -q 'APFS data safety' crates/goblins-os-core/src/install_targets.rs && rg -q 'Linux readiness' crates/goblins-os-core/src/install_targets.rs && rg -q 'Other OS or data readiness' crates/goblins-os-core/src/install_targets.rs && rg -q 'Dedicated disk readiness' crates/goblins-os-core/src/install_targets.rs"
check "installer policy exposes dual-boot assistant choices" "rg -q 'dual_boot_choices' crates/goblins-os-core/src/install_targets.rs && rg -q 'DualBootChoice' crates/goblins-os-core/src/install_targets.rs && rg -q 'Keep Windows' crates/goblins-os-core/src/install_targets.rs && rg -q 'suspend BitLocker' crates/goblins-os-core/src/install_targets.rs && rg -q 'Protect APFS data' crates/goblins-os-core/src/install_targets.rs && rg -q 'Keep Linux' crates/goblins-os-core/src/install_targets.rs && rg -q 'Keep another OS or data' crates/goblins-os-core/src/install_targets.rs && rg -q 'Use a dedicated disk' crates/goblins-os-core/src/install_targets.rs"
check "installer policy exposes guided dual-boot steps" "rg -q 'dual_boot_guide' crates/goblins-os-core/src/install_targets.rs && rg -q 'Disk Management' crates/goblins-os-core/src/install_targets.rs && rg -q 'APFS data' crates/goblins-os-core/src/install_targets.rs && rg -q 'Startup menu' crates/goblins-os-core/src/install_targets.rs && rg -q 'Final storage review' crates/goblins-os-core/src/install_targets.rs"
check "installer policy exposes dual-boot decision map" "rg -q 'dual_boot_decision_map' crates/goblins-os-core/src/install_targets.rs && rg -q 'DualBootDecision' crates/goblins-os-core/src/install_targets.rs && rg -q 'Windows beside Goblins OS' crates/goblins-os-core/src/install_targets.rs && rg -q 'APFS or Apple-origin disk' crates/goblins-os-core/src/install_targets.rs && rg -q 'Linux beside Goblins OS' crates/goblins-os-core/src/install_targets.rs && rg -q 'Separate disk' crates/goblins-os-core/src/install_targets.rs"
check "installer policy exposes advanced storage handoff" "rg -q 'full_storage_installer' crates/goblins-os-core/src/install_targets.rs && rg -q '/usr/libexec/goblins-os/goblins-os-full-installer' crates/goblins-os-core/src/install_targets.rs && rg -q 'org.goblins.OS.FullInstaller.desktop' crates/goblins-os-core/src/install_targets.rs && rg -q 'Advanced storage' crates/goblins-os-core/src/install_targets.rs"
check "installer policy exposes dual-boot quick start" "rg -q 'dual_boot_quick_start' crates/goblins-os-core/src/install_targets.rs && rg -q 'Install beside another OS' crates/goblins-os-core/src/install_targets.rs && rg -q 'Confirm preserve, format, and bootloader' crates/goblins-os-core/src/install_targets.rs && rg -q 'Test every boot path' crates/goblins-os-core/src/install_targets.rs"
check "installer policy explains firmware startup picker" "rg -q 'firmware startup menu or boot picker' crates/goblins-os-core/src/install_targets.rs"
check "installer policy covers existing OS and data preservation" "rg -Fq 'Preserve an existing operating system or data layout.' crates/goblins-os-core/src/install_targets.rs"
check "installer policy protects APFS and rejects Apple bare-metal inference" "rg -q 'APFS data safety' crates/goblins-os-core/src/install_targets.rs && rg -q 'APFS detection does not mean this hardware can boot Goblins OS' crates/goblins-os-core/src/install_targets.rs && rg -q 'not a supported bare-metal install target' crates/goblins-os-core/src/install_targets.rs"
check "installer API explains blocked simple erase dual-boot handoff" "rg -q 'The simple erase flow will not install' crates/goblins-os-core/src/install_targets.rs && rg -q 'open advanced storage' crates/goblins-os-core/src/install_targets.rs && rg -q 'select only unallocated free space' crates/goblins-os-core/src/install_targets.rs"
check "installer scanner detects BitLocker Microsoft Reserved Apple HFS and Linux filesystems" "rg -q 'bitlocker' crates/goblins-os-core/src/install_targets.rs && rg -q 'e3c9e316-0b5c-4db8-817d-f92df00215ae' crates/goblins-os-core/src/install_targets.rs && rg -q '48465300-0000-11aa-aa11-00306543ecac' crates/goblins-os-core/src/install_targets.rs && rg -q 'f2fs' crates/goblins-os-core/src/install_targets.rs && rg -q 'bcachefs' crates/goblins-os-core/src/install_targets.rs"
check "installer scanner test covers Windows macOS Linux and data partitions" "rg -q 'scans_sys_block_and_routes_existing_operating_systems_to_manual_storage' crates/goblins-os-core/src/install_targets.rs && rg -q 'TYPE=ntfs' crates/goblins-os-core/src/install_targets.rs && rg -q 'TYPE=apfs' crates/goblins-os-core/src/install_targets.rs && rg -q 'TYPE=crypto_LUKS' crates/goblins-os-core/src/install_targets.rs && rg -q 'TYPE=zfs_member' crates/goblins-os-core/src/install_targets.rs"
check "installer render proof uses Docker fixture for Windows macOS Linux and data partitions" "rg -q 'TYPE=ntfs' os/bootc/render-screens.sh && rg -q 'TYPE=apfs' os/bootc/render-screens.sh && rg -q 'TYPE=crypto_LUKS' os/bootc/render-screens.sh && rg -q 'TYPE=zfs_member' os/bootc/render-screens.sh"
check "installer render proof captures full storage handoff screenshot" "rg -q 'Open advanced storage handoff' os/bootc/render-screens.sh && rg -q '27-dual-boot-preserve-existing-os\\.png' os/bootc/render-screens.sh"
check "desktop render proof documents Docker harness" "rg -q 'DOCKER_BUILDKIT=1 docker build' os/bootc/render-desktop.suffix.Dockerfile && ! rg -q 'podman build' os/bootc/render-desktop.suffix.Dockerfile"
check "render proofs do not use legacy demo or seeded app hooks" "rg -q 'GOBLINS_OS_RENDER_QUERY' os/bootc/render-screens.sh crates/goblins-os-launcher/src/main.rs && ! rg -q 'GOBLINS_OS_SHELL_DEMO|GOBLINS_OS_LAUNCHER_DEMO' os/bootc/render-screens.sh crates/goblins-os-shell/src/main.rs crates/goblins-os-launcher/src/main.rs && ! rg -q 'Render/design proof: seed' crates/goblins-os-launcher/src/main.rs"
check "installer UI shows best path for dual boot" "rg -q 'Best dual-boot path' crates/goblins-os-installer/src/main.rs"
check "installer UI shows simple path choice before disk erase" "rg -q 'Choose install path' crates/goblins-os-installer/src/main.rs && rg -q 'Replace one blank disk' crates/goblins-os-installer/src/main.rs"
check "installer UI makes dual boot the first storage choice" "rg -q 'Keeping another OS or data?' crates/goblins-os-installer/src/main.rs && rg -q 'start with advanced storage' crates/goblins-os-installer/src/main.rs"
check "installer UI renders recommended install paths" "rg -q 'append_install_path_options' crates/goblins-os-installer/src/main.rs && rg -q 'Recommended install paths' crates/goblins-os-installer/src/main.rs && rg -q 'install_path_options_summary' crates/goblins-os-installer/src/main.rs"
check "installer UI renders pre-write boot formatting plan" "rg -q 'append_pre_write_install_plan' crates/goblins-os-installer/src/main.rs && rg -q 'Before writing to disk' crates/goblins-os-installer/src/main.rs && rg -q 'pre_write_install_plan_summary' crates/goblins-os-installer/src/main.rs && rg -q 'dual boot and custom formatting stay in advanced storage' crates/goblins-os-installer/src/main.rs"
check "installer UI renders dual-boot quick start" "rg -q 'append_dual_boot_quick_start' crates/goblins-os-installer/src/main.rs && rg -q 'Dual-boot quick start' crates/goblins-os-installer/src/main.rs && rg -q 'final preserve, format, and bootloader summary' crates/goblins-os-installer/src/main.rs && rg -q 'dual_boot_quick_start_summary' crates/goblins-os-installer/src/main.rs"
check "installer UI renders dual-boot readiness checklist" "rg -q 'append_dual_boot_readiness' crates/goblins-os-installer/src/main.rs && rg -q 'Dual-boot readiness' crates/goblins-os-installer/src/main.rs && rg -q 'Use this checklist before writing storage changes' crates/goblins-os-installer/src/main.rs && rg -q 'dual_boot_readiness_summary' crates/goblins-os-installer/src/main.rs"
check "installer UI renders dual-boot assistant choices" "rg -q 'append_dual_boot_choices' crates/goblins-os-installer/src/main.rs && rg -q 'Dual-boot assistant' crates/goblins-os-installer/src/main.rs && rg -q 'Pick the operating system you are keeping' crates/goblins-os-installer/src/main.rs && rg -q 'dual_boot_choices_summary' crates/goblins-os-installer/src/main.rs"
check "installer UI renders dual-boot decision map" "rg -q 'append_dual_boot_decision_map' crates/goblins-os-installer/src/main.rs && rg -q 'Dual-boot decision map' crates/goblins-os-installer/src/main.rs && rg -q 'Best for:' crates/goblins-os-installer/src/main.rs && rg -q 'dual_boot_decision_map_summary' crates/goblins-os-installer/src/main.rs"
check "installer UI renders safe dual-boot route" "rg -q 'append_dual_boot_safe_route' crates/goblins-os-installer/src/main.rs && rg -q 'dual_boot_safe_route_summary' crates/goblins-os-installer/src/main.rs && rg -q 'Install beside an existing OS' crates/goblins-os-installer/src/main.rs && rg -q 'installer_dual_boot_safe_route_launch_error' crates/goblins-os-installer/src/main.rs"
check "installer UI exposes advanced storage button" "rg -q 'append_full_storage_installer_handoff' crates/goblins-os-installer/src/main.rs && rg -q 'Open advanced storage' crates/goblins-os-installer/src/main.rs && rg -q 'launch_full_storage_installer' crates/goblins-os-installer/src/main.rs && rg -q 'StorageInstallerCommand' crates/goblins-os-installer/src/main.rs"
check "installer UI turns detected existing OS disks into preservation actions" "rg -q 'Detected systems are actions' crates/goblins-os-installer/src/main.rs && rg -q 'Open advanced storage from detected disk' crates/goblins-os-installer/src/main.rs && rg -q 'installer_detected_disk_full_storage_launch_error' crates/goblins-os-installer/src/main.rs && rg -q 'row.set_sensitive(target.eligible || preservation_handoff)' crates/goblins-os-installer/src/main.rs"
check "installer wizard labels are title case and not shouted" "rg -q 'Step 1 of 3 · Choose disk' crates/goblins-os-installer/src/main.rs && rg -q 'Step 3 of 3 · Confirm' crates/goblins-os-installer/src/main.rs && rg -q 'Required Confirmation' crates/goblins-os-installer/src/main.rs && rg -q '.gos-onboarding-kicker' crates/goblins-os-design/src/lib.rs && rg -q 'text-transform: none;' crates/goblins-os-design/src/lib.rs && ! rg -q 'STEP ·|FINAL STEP|REQUIRED CONFIRMATION|WHAT HAPPENED|letter-spacing: 2\\.2px' crates/goblins-os-installer/src/main.rs crates/goblins-os-design/src/lib.rs"
check "installer UI shows detected OS preservation checklist" "rg -q 'Preservation checklist:' crates/goblins-os-installer/src/main.rs && rg -q 'Back up and save recovery keys' crates/goblins-os-installer/src/main.rs && rg -q 'detected_system_preparation_hint' crates/goblins-os-installer/src/main.rs && rg -q 'test every preserved system from the firmware boot picker' crates/goblins-os-installer/src/main.rs"
check "installer UI exposes guided install-beside launcher" "rg -q 'append_dual_boot_launcher' crates/goblins-os-installer/src/main.rs && rg -q 'Install beside another OS' crates/goblins-os-installer/src/main.rs && rg -q 'What are you keeping?' crates/goblins-os-installer/src/main.rs && rg -q 'installer_dual_boot_choice_launch_error' crates/goblins-os-installer/src/main.rs && rg -q '.gos-dual-boot-choice' crates/goblins-os-design/src/lib.rs"
check "installer UI shows erase scope and boot recovery" "rg -q 'Simple install scope' crates/goblins-os-installer/src/main.rs && rg -q 'Erase scope' crates/goblins-os-installer/src/main.rs && rg -q 'Startup recovery' crates/goblins-os-installer/src/main.rs && rg -q 'After reboot' crates/goblins-os-installer/src/main.rs"
check "installer UI renders storage review checklist" "rg -q 'append_storage_review_checklist' crates/goblins-os-installer/src/main.rs && rg -q 'Storage review checklist' crates/goblins-os-installer/src/main.rs"
check "installer UI renders guided dual-boot steps" "rg -q 'append_dual_boot_guide' crates/goblins-os-installer/src/main.rs && rg -q 'Dual-boot guide' crates/goblins-os-installer/src/main.rs"
check "installer UI labels keep existing OS path" "rg -q 'Keep an existing OS' crates/goblins-os-installer/src/main.rs"
check "installer network copy hides internal service wording" "rg -q 'The network service is not responding on this device' crates/goblins-os-installer/src/main.rs && rg -q 'Networking not ready' crates/goblins-os-installer/src/main.rs && ! rg -q 'NetworkManager isn.t responding|Networking unavailable' crates/goblins-os-installer/src/main.rs"
check "installer copy hides bootc and Anaconda implementation labels" "rg -q 'Install readiness' crates/goblins-os-installer/src/main.rs && ! rg -q 'Installer engine' crates/goblins-os-installer/src/main.rs && ! rg -q 'bootc installer' crates/goblins-os-installer/src/main.rs && ! rg -q 'bootc install command' crates/goblins-os-installer/src/main.rs && ! rg -q 'Fedora/Anaconda' crates/goblins-os-installer/src/bin/goblins-os-full-installer.rs && ! rg -q 'Anaconda;' os/applications/org.goblins.OS.FullInstaller.desktop"
check "native design system uses Goblins-native naming" "rg -q 'GOBLINS_NATIVE_CSS' crates/goblins-os-design/src/lib.rs && ! rg -q -e 'OPENAI_NATIVE_CSS' -e 'OpenAI-native' crates/goblins-os-design/src/lib.rs crates/goblins-os-installer/src/main.rs crates/goblins-os-login/src/main.rs crates/goblins-os-launcher/src/main.rs crates/goblins-os-control-center/src/main.rs os/plymouth/goblins-os/goblins-os.script os/iso/config.toml"
check "boot splash uses Goblins mark for OS identity" "rg -q 'brand/anaconda/sidebar-logo.png' os/bootc/Containerfile && rg -q 'Goblins OS boot splash.*Goblins mark' os/plymouth/goblins-os/goblins-os.plymouth && ! rg -q 'brand/OpenAI-white-monoblossom.png[[:space:]]*\\\\' os/bootc/Containerfile"
check "installer and login product copy uses Goblins desktop naming" "rg -q 'Goblins-native desktop' crates/goblins-os-installer/src/main.rs && rg -q 'Enter Goblins OS' crates/goblins-os-installer/src/main.rs && rg -q 'Unlock Goblins OS desktop' crates/goblins-os-login/src/main.rs && rg -q 'Goblins OS desktop unlock was rejected by local OS services' crates/goblins-os-login/src/main.rs && ! rg -q -e 'OpenAI-native desktop' -e 'Enter OpenAI desktop' -e 'Unlock OpenAI desktop' -e 'OpenAI desktop unlock' crates/goblins-os-installer/src/main.rs crates/goblins-os-login/src/main.rs"
check "desktop metadata uses Goblins identity for OS surfaces" "rg -q 'Comment=Native Goblins OS identity gate' os/applications/org.goblins.OS.Login.desktop && rg -q 'Comment=Native recovery checks for the boot image, services, models, and Goblins identity' os/applications/org.goblins.OS.Recovery.desktop && rg -q 'Comment=Native Goblins OS policy, enterprise controls, data boundaries, and permission gates' os/applications/org.goblins.OS.Policy.desktop"
check "OpenAI service launcher copy is Goblins-native" "rg -Fq 'unknown Goblins OS service id' crates/goblins-os-open/src/main.rs && rg -Fq 'Goblins OS service {service_id} is not allowed by the active policy or permission state' crates/goblins-os-open/src/main.rs && ! rg -Fq 'OpenAI OS service' crates/goblins-os-open/src/main.rs && rg -Fq 'Description=Goblins OS local AI service core' os/systemd/goblins-os-core.service"
check "core service owns policy state for permission grants" "rg -q '^StateDirectory=.*goblins-os/policy' os/systemd/goblins-os-core.service && rg -q '^StateDirectoryMode=0750$' os/systemd/goblins-os-core.service"
check "core service writable path allowlist is exact and narrow" "core_service_writable_paths_are_exact"
check "first boot proves the production core unit owns its socket and writable mounts" "firstboot_production_core_unit_proof_is_pinned"
check "hardware core proof unit has exact writable roots and a read-only desktop table view" "hardware_core_proof_unit_is_narrowly_sandboxed"
check "installer policy copy hides raw installer engine name" "rg -q 'advanced storage' crates/goblins-os-core/src/install_targets.rs && rg -q 'installer' crates/goblins-os-core/src/install_targets.rs && rg -q 'Goblins OS disk installer' crates/goblins-os-core/src/install_targets.rs && ! rg -q 'Anaconda' crates/goblins-os-core/src/install_targets.rs && ! rg -q 'bootc installer' crates/goblins-os-core/src/install_targets.rs && ! rg -q -e 'Ready for guarded bootc install preparation' -e 'bootc install was started by the Goblins OS core' -e 'could not spawn bootc install' -e 'core may spawn bootc install' crates/goblins-os-core/src/install_targets.rs"
check "installer UI copy uses advanced storage path" "rg -q 'open advanced storage' crates/goblins-os-core/src/install_targets.rs crates/goblins-os-installer/src/main.rs && rg -q 'advanced storage' crates/goblins-os-core/src/install_targets.rs crates/goblins-os-installer/src/main.rs && ! rg -q -e 'ISO manual storage' -e 'ISO Installation Destination' -e 'Installation Destination in the ISO' -e 'manual storage from the ISO' -e 'Use Installation Destination' crates/goblins-os-core/src/install_targets.rs crates/goblins-os-installer/src/main.rs"
check "installer docs use advanced storage language" "rg -q 'advanced storage Installation Destination' os/hardware-gate/runbook.md && rg -q 'advanced storage' \"$SHIP_DECL\" os/hardware-gate/runbook.md && rg -q 'advanced storage' os/iso/config.toml && ! rg -q -e 'uses Anaconda Installation Destination/manual storage' -e 'Anaconda manual storage summary' -e 'visible in Anaconda' -e 'to Anaconda manual storage' -e 'choose the disk/storage layout in Anaconda' \"$SHIP_DECL\" os/iso/config.toml os/hardware-gate/runbook.md"
check "settings detail copy hides raw setup state" "rg -Fq '(\"not configured\", \"not set up\")' crates/goblins-os-settings/src/main.rs && rg -Fq '(\"not available yet\", \"not ready yet\")' crates/goblins-os-settings/src/main.rs"
check "settings native app handoff uses image-owned copy" "rg -q 'Not Included' crates/goblins-os-settings/src/main.rs && rg -q 'included in the full Goblins OS image' crates/goblins-os-settings/src/main.rs && ! rg -q -e 'is not installed on this image' -e 'Not Installed' crates/goblins-os-settings/src/main.rs"
check "settings storage pressure plan is actionable" "rg -q 'append_storage_pressure_plan' crates/goblins-os-settings/src/main.rs && rg -q 'Storage pressure plan' crates/goblins-os-settings/src/main.rs && rg -q 'Open Disk Usage Analyzer' crates/goblins-os-settings/src/main.rs && rg -q 'Open Disks' crates/goblins-os-settings/src/main.rs && rg -q 'automatic removal of aged files' crates/goblins-os-settings/src/main.rs && ! rg -q 'needs GNOME' crates/goblins-os-settings/src/main.rs"
check "privacy cleanup copy uses aged wording" "rg -q 'Remove aged temporary files' crates/goblins-os-settings/src/main.rs crates/goblins-os-core/src/privacy.rs && ! rg -q 'Remove old temporary files' crates/goblins-os-settings/src/main.rs crates/goblins-os-core/src/privacy.rs"
check "settings built-in capability copy avoids install-manager wording" "rg -q 'Bluetooth support is not ready on this device' crates/goblins-os-settings/src/main.rs && rg -q 'Audio routing support is not ready in this build' crates/goblins-os-settings/src/main.rs && rg -q 'OpenAI account · Codex CLI not included' crates/goblins-os-settings/src/main.rs && rg -q 'Required service support is not included in this build' crates/goblins-os-settings/src/main.rs"
check "core built-in capability copy avoids install-manager wording" "rg -q 'Bluetooth support is not ready on this device' crates/goblins-os-core/src/bluetooth.rs && rg -q 'Audio routing controls are not ready' crates/goblins-os-core/src/audio.rs && rg -q 'OpenAI account access through the bundled Codex CLI is not included in this build' crates/goblins-os-core/src/codex.rs && ! rg -q -e 'Bluetooth support is not installed' -e 'WirePlumber control tooling is not installed' -e 'Codex CLI is not installed' crates/goblins-os-core/src/bluetooth.rs crates/goblins-os-core/src/audio.rs crates/goblins-os-core/src/codex.rs"
check "ISO/runbook document Custom or Reclaim Space dual boot" "rg -q 'Custom/manual storage or Reclaim Space' os/iso/config.toml os/hardware-gate/runbook.md"
check "ISO/runbook document advanced storage handoff" "rg -q 'Open advanced storage' os/iso/config.toml os/hardware-gate/runbook.md && rg -q 'Install Goblins OS Beside Another OS' os/hardware-gate/runbook.md"
check "runbook documents disk and Docker preflight" "rg -q '120 GiB free' os/hardware-gate/runbook.md && rg -q 'docker info' os/hardware-gate/runbook.md"
check "SHIP documents free-space or dedicated-disk dual boot" "rg -q 'unallocated free space or a dedicated disk' \"$SHIP_DECL\""
check "SHIP documents safe install-beside route" "rg -q 'Install beside an existing OS' \"$SHIP_DECL\" && rg -q 'every filesystem that will be formatted' \"$SHIP_DECL\""
check "SHIP documents dual-boot readiness checklist" "rg -q 'Dual-boot readiness' \"$SHIP_DECL\" && rg -q 'Windows/Linux/other OS or data' \"$SHIP_DECL\""
check "SHIP documents dual-boot assistant" "rg -q 'Dual-boot assistant' \"$SHIP_DECL\""
check "SHIP documents dual-boot decision map" "rg -q 'Dual-boot decision map' \"$SHIP_DECL\" && rg -q 'separate-disk rows' \"$SHIP_DECL\""
check "SHIP documents pre-write boot formatting plan" "rg -q 'Before writing to disk' \"$SHIP_DECL\" && rg -q 'fresh GPT layout' \"$SHIP_DECL\" && rg -q 'bootloader/EFI target' \"$SHIP_DECL\" && rg -q 'Btrfs root' \"$SHIP_DECL\""
check "SHIP documents advanced storage entry point" "rg -q 'Open advanced storage' \"$SHIP_DECL\" && rg -q 'Install Goblins OS Beside Another OS' \"$SHIP_DECL\""
check "external gate names preserved existing OS partitions without Apple support claims" "rg -q 'preserved existing-OS/APFS/data/recovery/vendor/EFI partitions' os/hardware-gate/run-external-gate.sh && rg -q 'APFS is preserve-only and does not claim Apple bare-metal support' os/hardware-gate/run-external-gate.sh"
check "external gate documents advanced storage entry point" "rg -q 'Open advanced storage' os/hardware-gate/run-external-gate.sh && rg -q 'Install Goblins OS Beside Another OS' os/hardware-gate/run-external-gate.sh"
check "bootc image includes advanced storage handoff" "rg -q 'anaconda-live' os/bootc/Containerfile && rg -q 'goblins-os-full-installer' os/bootc/Containerfile && rg -q 'org.goblins.OS.FullInstaller.desktop' os/bootc/Containerfile && rg -q 'desktop-file-validate /usr/share/applications/org.goblins.OS.FullInstaller.desktop' os/bootc/Containerfile"
check "core AI exposes notification context route" "rg -Fq '/v1/ai/notification-context' crates/goblins-os-core/src/main.rs && rg -Fq 'ask_notification_context' crates/goblins-os-core/src/main.rs"
check "core AI notification context is permission gated" "rg -Fq 'policy_state_for_control(\"notification-context\")' crates/goblins-os-core/src/ai.rs && rg -Fq 'Allow notification context in Privacy & Permissions' crates/goblins-os-core/src/ai.rs"
check "core AI notification context is bounded to one invoked notification" "rg -Fq 'Use only this invoked notification summary' crates/goblins-os-core/src/ai.rs && rg -Fq 'do not claim to inspect notification history, other notifications, files, screenshots, secrets, hidden windows, or background app data' crates/goblins-os-core/src/ai.rs"
check "core AI notification context audits registered action only" "rg -Fq 'audit_ai_action(\"answer-notification\"' crates/goblins-os-core/src/ai.rs && rg -Fq 'notification_context_prompt_is_invoked_and_bounded_to_one_notification' crates/goblins-os-core/src/ai.rs"
check "core AI runtime uses Goblins-native route with legacy compatibility" "rg -Fq '/v1/ai/runtime/status' crates/goblins-os-core/src/main.rs && rg -Fq '/v1/ai/runtime' crates/goblins-os-core/src/main.rs && rg -Fq '.route(\"/v1/codex/resident\", post(ai_runtime))' crates/goblins-os-core/src/main.rs"
check "desktop clients use Goblins-native AI runtime route" "rg -Fq '/v1/ai/runtime/status' crates/goblins-os-settings/src/main.rs crates/goblins-os-shell/src/main.rs && rg -Fq '/v1/ai/runtime' crates/goblins-os-launcher/src/main.rs && ! rg -Fq '\"/v1/codex/resident/status\"' crates/goblins-os-settings/src/main.rs crates/goblins-os-shell/src/main.rs && ! rg -Fq '\"/v1/codex/resident\"' crates/goblins-os-launcher/src/main.rs"
check "installed self-test checks AI runtime primary route and compatibility alias" "rg -Fq '/v1/ai/runtime/status' os/bootc/run-selftest.sh && rg -Fq '/v1/codex/resident/status' os/bootc/run-selftest.sh && rg -Fq 'Goblins AI runtime IPC socket live' os/bootc/run-selftest.sh"
check "hosted protected context is core-retained, uid-bound, and broker-claimed" "rg -Fq 'OsRng.fill_bytes' crates/goblins-os-core/src/context_consent.rs && rg -Fq 'REVIEW_TTL: Duration = Duration::from_secs(310)' crates/goblins-os-core/src/context_consent.rs && rg -Fq 'flow_serial().try_lock()' crates/goblins-os-core/src/context_consent.rs && rg -Fq 'fn claim_for_broker' crates/goblins-os-core/src/context_consent.rs && rg -Fq 'entry.intended_user_id == broker_user_id' crates/goblins-os-core/src/context_consent.rs && rg -Fq 'broker_cannot_claim_or_decide_another_users_review' crates/goblins-os-core/src/context_consent.rs && rg -Fq 'cross_user_broker_routes_return_gone_and_preserve_intended_review' crates/goblins-os-core/src/context_consent.rs && rg -Fq 'tokio::task::block_in_place' crates/goblins-os-core/src/context_consent.rs && rg -Fq 'broker_routes_progress_while_hosted_request_waits' crates/goblins-os-core/src/context_consent.rs && rg -Fq 'bounded_blocking_hosted_wait_does_not_nested_block_in_place' crates/goblins-os-core/src/context_consent.rs && rg -Fq 'current_thread_runtime_fails_closed_without_panic_or_residual_review' crates/goblins-os-core/src/context_consent.rs && rg -Fq 'Exact instruction and reviewed files Codex can access' crates/goblins-os-core/src/resident.rs && rg -Fq 'gtk::Label::new(Some(&review.content_label))' crates/goblins-os-consent-broker/src/main.rs && rg -Fq 'outbound_digest: outbound_digest(outbound_binding)' crates/goblins-os-core/src/context_consent.rs"
check "capability connections bind immutable peer uid to private requests" "rg -Fq 'struct CapabilityPeerAddress' crates/goblins-os-core/src/control_plane.rs && rg -Fq 'into_make_service_with_connect_info::<CapabilityPeerAddress>()' crates/goblins-os-core/src/control_plane.rs && rg -Fq 'user_id: peer.user_id' crates/goblins-os-core/src/control_plane.rs && rg -Fq 'authenticated_peer_uid_is_bound_to_every_private_request' crates/goblins-os-core/src/control_plane.rs"
check "protected context user bridge receives no consent authority" "rg -Fq 'consent_launch_request_exposes_no_review_capability' crates/goblins-os-core/src/session_bridge.rs && rg -Fq 'consent_launch_protocol_rejects_requester_supplied_capabilities' crates/goblins-os-session-bridge/src/main.rs && rg -Fq 'LaunchHostedConsentBroker {}' crates/goblins-os-session-bridge/src/main.rs"
check "installed protected-context self-test accepts only terminal broker outcomes" "rg -Fq 'protected_context_status_is_valid' os/bootc/run-selftest.sh && rg -Fq '200|403|408|429|503' os/bootc/run-selftest.sh"
check "settings exposes notification AI readiness" "rg -q 'append_notifications_ai_context' crates/goblins-os-settings/src/main.rs && rg -q 'Goblins AI for notifications' crates/goblins-os-settings/src/main.rs && rg -q 'answer-notification' crates/goblins-os-settings/src/main.rs"
check "voice assistant uses Goblin wake word truthfully" "rg -q 'VOICE_WAKE_WORD: &str = \"Goblin\"' crates/goblins-os-core/src/voice.rs && rg -q '\"Hey Goblin\"' crates/goblins-os-core/src/voice.rs && rg -q 'wake_listening' crates/goblins-os-core/src/voice.rs && rg -q 'Background wake listening is not ready' crates/goblins-os-core/src/voice.rs crates/goblins-os-settings/src/main.rs && rg -Fq 'Say {voice_word}' crates/goblins-os-shell/src/main.rs && rg -Fq 'Listening for {wake_word}…' crates/goblins-os-shell/src/main.rs && rg -q 'Goblin wake word' crates/goblins-os-settings/src/main.rs && rg -q 'Ask Goblin' crates/goblins-os-launcher/src/main.rs crates/goblins-os-settings/src/main.rs crates/goblins-os-control-center/src/main.rs crates/goblins-os-ai/src/lib.rs os/gnome-shell-extensions/goblins-menubar@goblins.os/extension.js && rg -Fq 'scripts/Ask Goblin about this' crates/goblins-os-verify/src/main.rs && test -f 'os/nautilus/scripts/Ask Goblin about this' && ! rg -q -e 'Talk[[:space:]]to[[:space:]]Goblins[[:space:]]OS' -e 'Ask[[:space:]]Goblins' -e 'Write[[:space:]]with[[:space:]]Goblins' -e 'Voice[[:space:]]model' crates/goblins-os-shell/src/main.rs crates/goblins-os-launcher/src/main.rs crates/goblins-os-settings/src/main.rs crates/goblins-os-ai/src/lib.rs os/gnome-shell-extensions/goblins-menubar@goblins.os/extension.js"
check "voice control dispatch is source-gated" "rg -q '/v1/voice/control' crates/goblins-os-core/src/main.rs && rg -q 'dispatch_voice_safe_setting_change' crates/goblins-os-core/src/voice_control.rs && rg -q 'fall_through_to_dictation: true' crates/goblins-os-core/src/voice_control.rs && rg -q 'id: \"voice-control\"' crates/goblins-os-ai/src/lib.rs && rg -Fq 'initialize(ClientKind::VoiceControl)' crates/goblins-os-session-tools/src/bin/goblins-os-voice-control.rs && rg -Fq 'const ROUTE: &str = \"/v1/voice/control\"' crates/goblins-os-session-tools/src/bin/goblins-os-voice-control.rs && ! test -e os/voice/goblins-os-voice-control && rg -q 'goblins-os-voice-control' os/bootc/Containerfile && rg -q 'Voice Control is source-gated' crates/goblins-os-settings/src/main.rs"
check "sound recognition decision contract is source-gated" "rg -q 'evaluate_sound_recognition_window' crates/goblins-os-core/src/sound_recognition.rs && rg -q 'sound_recognition_notification_payload' crates/goblins-os-core/src/sound_recognition.rs && rg -q 'sound_recognition_notification_delivery_plan' crates/goblins-os-core/src/sound_recognition.rs && rg -q 'listener_runtime_capabilities' crates/goblins-os-core/src/sound_recognition.rs && rg -Fq 'payload.runtime_ready_claim.unwrap_or(false)' crates/goblins-os-core/src/sound_recognition.rs && rg -q 'capture_runtime_ready' crates/goblins-os-core/src/sound_recognition.rs os/sound-recognition/goblins-os-sound-listener os/bootc/Containerfile && rg -q 'notification_delivery_contract_ready' os/sound-recognition/goblins-os-sound-listener os/bootc/Containerfile && rg -q 'delivery_ready_claim' os/sound-recognition/goblins-os-sound-listener crates/goblins-os-core/src/sound_recognition.rs && rg -q 'decision_contract_ready' os/sound-recognition/goblins-os-sound-listener && rg -q -- '--decision-self-test' os/sound-recognition/goblins-os-sound-listener os/bootc/Containerfile && rg -q -- '--notification-self-test' os/sound-recognition/goblins-os-sound-listener os/bootc/Containerfile"
check "live captions overlay is source-gated" "rg -q '/v1/captions/stream' crates/goblins-os-core/src/main.rs && rg -q 'text/event-stream' crates/goblins-os-core/src/live_captions.rs && rg -q 'pipewire_monitor_targets_from_dump' crates/goblins-os-core/src/live_captions.rs && rg -q 'caption_capture_args' crates/goblins-os-core/src/live_captions.rs && rg -q 'capture_runtime_ready: false' crates/goblins-os-core/src/live_captions.rs && rg -q 'transcription_ready_claim: false' crates/goblins-os-core/src/live_captions.rs && rg -q 'no live monitor target, capture stream, or transcription loop is claimed yet' crates/goblins-os-core/src/live_captions.rs && test -f os/gnome-shell-extensions/goblins-captions@goblins.os/extension.js && rg -q 'waiting for the local caption stream' os/gnome-shell-extensions/goblins-captions@goblins.os/extension.js && rg -q 'showWaitingRenderProof' os/gnome-shell-extensions/goblins-captions@goblins.os/extension.js && rg -q 'captureRuntimeReadyClaim: false' os/gnome-shell-extensions/goblins-captions@goblins.os/extension.js && rg -q 'transcriptionReadyClaim: false' os/gnome-shell-extensions/goblins-captions@goblins.os/extension.js && rg -Fq '58-live-captions-waiting-\$suffix.png' os/bootc/render-desktop.sh && rg -q 'font-family: \"Inter\"' os/gnome-shell-extensions/goblins-captions@goblins.os/stylesheet.css && rg -q 'goblins-captions@goblins.os' os/gnome-shell-modes/goblins-os.json"
check "settings live captions row is source-gated" "rg -q '/v1/live-captions/status' crates/goblins-os-settings/src/main.rs && rg -q 'append_live_captions_settings' crates/goblins-os-settings/src/main.rs && rg -q 'Toggle lives in Quick Settings' crates/goblins-os-settings/src/main.rs && rg -q 'Captioning stays local.' crates/goblins-os-settings/src/main.rs"
check "visual lookup launcher is source-gated" "test -f os/applications/org.goblins.OS.VisualLookup.desktop && rg -Fq 'Name=Visual Look Up' os/applications/org.goblins.OS.VisualLookup.desktop && rg -Fq 'Exec=/usr/libexec/goblins-os/goblins-os-visual-lookup' os/applications/org.goblins.OS.VisualLookup.desktop && rg -Fq 'NoDisplay=false' os/applications/org.goblins.OS.VisualLookup.desktop && rg -Fq 'StartupWMClass=org.goblins.OS.VisualLookup' os/applications/org.goblins.OS.VisualLookup.desktop && rg -Fq 'desktop-file-validate /usr/share/applications/org.goblins.OS.VisualLookup.desktop' os/bootc/Containerfile && rg -Fq 'goblins-os-visual-lookup/native-desktop' os/bootc/Containerfile"
check "today panel render hook is source-gated" "rg -Fq 'capture goblins-os-today' os/bootc/render-screens.sh && rg -Fq '122-today.png' os/bootc/render-screens.sh && rg -Fq '123-today-dark.png' os/bootc/render-screens.sh && rg -Fq 'run_desktop_app /usr/libexec/goblins-os/\"\$bin\"' os/bootc/render-screens.sh"
check "preview viewer packages and defaults are source-gated" "rg -q 'papers' os/bootc/Containerfile && rg -q 'loupe' os/bootc/Containerfile && rg -q 'command -v papers' os/bootc/Containerfile && rg -q 'command -v loupe' os/bootc/Containerfile && test -f os/applications/mimeapps.list && rg -q 'application/pdf=org.gnome.Papers.desktop' os/applications/mimeapps.list && rg -q 'image/png=org.gnome.Loupe.desktop' os/applications/mimeapps.list && rg -q 'image/jpeg=org.gnome.Loupe.desktop' os/applications/mimeapps.list"
check "preview open substrate is source-gated" "rg -q '/v1/preview/status' crates/goblins-os-core/src/main.rs && rg -q '/v1/preview/open' crates/goblins-os-core/src/main.rs && rg -Fq 'isolated_session_command(\"xdg-open\")' crates/goblins-os-core/src/preview.rs && rg -q 'Papers for PDFs and Loupe for images' crates/goblins-os-core/src/preview.rs && rg -q 'It never reads file contents or claims rendered proof.' crates/goblins-os-core/src/preview.rs"
check "preview open uses session bridge before direct fallback" "rg -Fq 'crate::session_bridge::open_preview' crates/goblins-os-core/src/preview.rs && rg -Fq 'OpenPreview' crates/goblins-os-session-bridge/src/main.rs && rg -Fq 'xdg-open' crates/goblins-os-session-bridge/src/main.rs"
check "preview installed-image open self-test is source-gated" "rg -Fq 'GET /v1/preview/status -> HTTP' os/bootc/run-selftest.sh && rg -Fq 'available=\$preview_available xdg-open=\$preview_xdg_open papers=\$preview_papers loupe=\$preview_loupe' os/bootc/run-selftest.sh && rg -Fq 'supported_extensions | index(\"pdf\") and index(\"png\")' os/bootc/run-selftest.sh && rg -Fq 'POST /v1/preview/open PDF -> HTTP' os/bootc/run-selftest.sh && rg -Fq 'POST /v1/preview/open image -> HTTP' os/bootc/run-selftest.sh && rg -Fq 'POST /v1/preview/open unsupported -> HTTP' os/bootc/run-selftest.sh"
check "fingerprint unlock substrate is source-gated" "rg -q '/v1/fingerprint/status' crates/goblins-os-core/src/main.rs && rg -q 'authselect_has_fingerprint' crates/goblins-os-core/src/fingerprint.rs && rg -q 'net.reactivated.Fprint.service' crates/goblins-os-core/src/fingerprint.rs && rg -q 'password remains available' crates/goblins-os-core/src/fingerprint.rs && rg -q 'authselect enable-feature with-fingerprint' os/bootc/Containerfile && rg -q 'pam_fprintd.so' os/bootc/Containerfile && rg -q 'fprintd-pam' os/bootc/Containerfile && rg -q 'Fingerprint unlock' crates/goblins-os-settings/src/main.rs"
check "keychain collection metadata is source-gated" "rg -q '/v1/keychain/collections' crates/goblins-os-core/src/main.rs crates/goblins-os-settings/src/main.rs && rg -q 'org.freedesktop.Secret.Service' crates/goblins-os-core/src/keychain.rs && rg -q 'Secret values are never returned by Goblins OS' crates/goblins-os-core/src/keychain.rs && rg -q 'Secret values are never displayed in Settings' crates/goblins-os-settings/src/main.rs && ! rg -q 'GetSecrets' crates/goblins-os-core/src/keychain.rs"
check "keychain manager handoff is source-gated" "rg -q 'Open Passwords & Keys' crates/goblins-os-settings/src/main.rs && rg -Fq 'SEAHORSE_PASSWORDS_AND_KEYS: &str = \"seahorse\"' crates/goblins-os-settings/src/main.rs && rg -Fq 'append_keychain_manager_handoff(panel)' crates/goblins-os-settings/src/main.rs && rg -q 'manage saved passwords, keys, and certificates in the system keyring' crates/goblins-os-settings/src/main.rs"
check "personal hotspot write substrate is source-gated" "rg -q '/v1/hotspot/enabled' crates/goblins-os-core/src/main.rs && rg -Fq 'policy_state_for_control(\"settings-control\")' crates/goblins-os-core/src/hotspot.rs && rg -q 'dnsmasq_present' crates/goblins-os-core/src/hotspot.rs && rg -q 'Connect to the internet over Ethernet to share it over Wi-Fi.' crates/goblins-os-core/src/hotspot.rs && rg -q 'sanitize_hotspot_error' crates/goblins-os-core/src/hotspot.rs && rg -q 'dnsmasq' os/bootc/Containerfile && rg -q 'command -v dnsmasq' os/bootc/Containerfile && rg -q 'append_hotspot_management' crates/goblins-os-settings/src/main.rs && rg -q 'hotspot_settings_inputs' crates/goblins-os-settings/src/main.rs && rg -q 'Passwords are used once to configure the hotspot and are never shown here.' crates/goblins-os-settings/src/main.rs && rg -q 'connected_clients_known' crates/goblins-os-core/src/hotspot.rs && rg -q 'parse_dnsmasq_leases' crates/goblins-os-core/src/hotspot.rs && rg -q 'Connected devices' crates/goblins-os-settings/src/main.rs"
check "switch control overlay is source-gated" "test -f os/gnome-shell-extensions/goblins-switch@goblins.os/extension.js && rg -q \"const SCHEMA_ID = 'org.goblins.os.a11y.switch-control';\" os/gnome-shell-extensions/goblins-switch@goblins.os/extension.js && rg -Fq \"import('gi://Atspi')\" os/gnome-shell-extensions/goblins-switch@goblins.os/extension.js && rg -q 'This window has no scannable controls - using point scan.' os/gnome-shell-extensions/goblins-switch@goblins.os/extension.js && rg -q 'Secure pointer control is not available yet.' os/gnome-shell-extensions/goblins-switch@goblins.os/extension.js && rg -q 'font-family: \"Inter\"' os/gnome-shell-extensions/goblins-switch@goblins.os/stylesheet.css && rg -q 'goblins-switch@goblins.os' os/gnome-shell-modes/goblins-os.json && rg -q 'goblins-switch@goblins.os' os/dconf/db/local.d/10-goblins-os-desktop"
check "switch control desktop render hook is source-gated" "rg -q 'showPointScanDemo' os/gnome-shell-extensions/goblins-switch@goblins.os/extension.js && rg -Fq '57-switch-control-point-\$suffix.png' os/bootc/render-desktop.sh && rg -q 'showPointScanDemo' os/bootc/render-desktop.sh"
check "IME menu-bar input source render hook is source-gated" "rg -Fq '59-menubar-input-source-\$suffix.png' os/bootc/render-desktop.sh && rg -Fq \"[('xkb', 'us'), ('xkb', 'gb')]\" os/bootc/render-desktop.sh && rg -q 'gsettings set org.gnome.desktop.input-sources current 1' os/bootc/render-desktop.sh"
check "Today menu-bar date button is source-gated" "rg -Fq '/usr/libexec/goblins-os/goblins-os-today' os/gnome-shell-extensions/goblins-menubar@goblins.os/extension.js && rg -Fq \"this._today = new PanelMenu.Button(0.0, 'Today', true);\" os/gnome-shell-extensions/goblins-menubar@goblins.os/extension.js && rg -Fq \"Main.panel.addToStatusArea('goblins-today', this._today, 1, 'right');\" os/gnome-shell-extensions/goblins-menubar@goblins.os/extension.js && rg -Fq 'GLib.DateTime.new_now_local().format' os/gnome-shell-extensions/goblins-menubar@goblins.os/extension.js && rg -Fq 'changed::clock-format' os/gnome-shell-extensions/goblins-menubar@goblins.os/extension.js && rg -Fq 'this._clearTodayClockTimer();' os/gnome-shell-extensions/goblins-menubar@goblins.os/extension.js && rg -Fq '.goblins-date-indicator' os/gnome-shell-extensions/goblins-menubar@goblins.os/stylesheet.css os/themes/GoblinsOS/gnome-shell/gnome-shell-light.css"
check "Today menu-bar render hook is source-gated" "rg -Fq '59c-menubar-today-\$suffix.png' os/bootc/render-desktop.sh && rg -q 'gsettings set org.gnome.desktop.interface clock-show-weekday true' os/bootc/render-desktop.sh && rg -q 'gsettings set org.gnome.desktop.interface clock-show-seconds false' os/bootc/render-desktop.sh"
check "settings notification AI copy preserves privacy boundary" "rg -Fq \"only that notification's exact title, body, app, chosen action label, and question\" crates/goblins-os-settings/src/main.rs && rg -Fq 'A hosted route requires a fresh review bound to the active engine before any of it leaves this device.' crates/goblins-os-settings/src/main.rs"
check "launcher search uses native accessible icon" "rg -Fq 'gtk::Image::from_icon_name(\"system-search-symbolic\")' crates/goblins-os-launcher/src/main.rs && rg -q 'Search Goblins OS' crates/goblins-os-launcher/src/main.rs && ! rg -q 'telephone-recorder' crates/goblins-os-launcher/src/main.rs"
check "control center controls use accessible title-case copy" "rg -q 'Connection & Appearance' crates/goblins-os-control-center/src/main.rs && rg -q 'Goblins AI' crates/goblins-os-control-center/src/main.rs && rg -q 'Sound' crates/goblins-os-control-center/src/main.rs && rg -q 'Display brightness' crates/goblins-os-control-center/src/main.rs && rg -q 'set_accessible_label_description' crates/goblins-os-control-center/src/main.rs && rg -q 'Use on-device GPT-OSS' crates/goblins-os-control-center/src/main.rs && ! rg -q -e 'CONNECTION & APPEARANCE' -e 'BUILD ENGINE' -e 'GOBLINS AI' -e 'SOUND' -e 'DISPLAY' crates/goblins-os-control-center/src/main.rs"
check "shell dock and window manager controls expose accessible names and focus states" "rg -q 'accessible_name: .*Open' os/gnome-shell-extensions/goblins-dock@goblins.os/extension.js && rg -q 'accessible_name: .*Activate' os/gnome-shell-extensions/goblins-wm@goblins.os/extension.js && rg -q \"accessible_name: 'Move to previous space'\" os/gnome-shell-extensions/goblins-wm@goblins.os/extension.js && rg -q '.goblins-dock-item:focus' os/gnome-shell-extensions/goblins-dock@goblins.os/stylesheet.css && rg -q '.goblins-wm-window-card:focus' os/gnome-shell-extensions/goblins-wm@goblins.os/stylesheet.css && rg -q '.goblins-wm-hud-button:focus' os/gnome-shell-extensions/goblins-wm@goblins.os/stylesheet.css"
check "core AI exposes confirmed safe setting route" "rg -q '/v1/ai/safe-setting-change' crates/goblins-os-core/src/main.rs && rg -q 'change_safe_setting' crates/goblins-os-core/src/main.rs"
check "core AI exposes open settings panel route" "rg -q '/v1/ai/open-settings-panel' crates/goblins-os-core/src/main.rs && rg -q 'open_settings_panel' crates/goblins-os-core/src/main.rs"
check "core AI open settings panel route is deterministic and offline" "rg -q 'OpenSettingsPanelRequest' crates/goblins-os-core/src/ai.rs && rg -q 'SETTINGS_PANEL_CANDIDATES' crates/goblins-os-core/src/ai.rs && rg -q 'resolve_open_settings_panel' crates/goblins-os-core/src/ai.rs && rg -q 'settings_panel_router_maps_exact_and_natural_language_requests' crates/goblins-os-core/src/ai.rs"
check "core AI open settings panel route uses policy and audit" "rg -Fq 'policy_state_for_control(\"resident-assistant\")' crates/goblins-os-core/src/ai.rs && rg -Fq 'audit_open_settings_panel' crates/goblins-os-core/src/ai.rs && rg -Fq 'launch_argument: format!(\"--panel={}\"' crates/goblins-os-core/src/ai.rs"
check "installed self-test checks open settings panel route" "rg -q '/v1/ai/open-settings-panel' os/bootc/run-selftest.sh && rg -q 'open wifi settings' os/bootc/run-selftest.sh"
check "core AI exposes system status route" "rg -q '/v1/ai/system-status' crates/goblins-os-core/src/main.rs && rg -q 'ask_system_status' crates/goblins-os-core/src/main.rs"
check "core AI system status route uses OS-owned bounded snapshot" "rg -q 'SystemStatusContextRequest' crates/goblins-os-core/src/ai.rs && rg -q 'bounded_system_status_snapshot' crates/goblins-os-core/src/ai.rs && rg -q 'Use only this OS-owned status snapshot' crates/goblins-os-core/src/ai.rs && rg -q 'system_status_prompt_uses_only_os_owned_snapshot' crates/goblins-os-core/src/ai.rs"
check "core AI system status route uses policy and audit" "rg -q 'system_troubleshooting_policy' crates/goblins-os-core/src/ai.rs && rg -Fq 'audit_ai_action(action_id, Some(\"troubleshooting\")' crates/goblins-os-core/src/ai.rs && rg -q 'system_status_action_id' crates/goblins-os-core/src/ai.rs"
check "installed self-test checks system status route" "rg -q '/v1/ai/system-status' os/bootc/run-selftest.sh && rg -q 'Summarize current system state' os/bootc/run-selftest.sh"
check "core input sources expose narrow write route and encoder" "rg -q '/v1/input/sources' crates/goblins-os-core/src/main.rs && rg -q 'normalize_input_sources' crates/goblins-os-core/src/input.rs && rg -q 'encode_input_sources' crates/goblins-os-core/src/input.rs"
check "settings input sources expose reorder and remove write controls" "rg -q 'input_source_action_button' crates/goblins-os-settings/src/main.rs && rg -q 'reordered_input_sources' crates/goblins-os-settings/src/main.rs && rg -q 'input_sources_without' crates/goblins-os-settings/src/main.rs && rg -q '/v1/input/sources' crates/goblins-os-settings/src/main.rs"
check "capture harness resets only the exact validated dated run dir" "rg -Fq 'rm -rf \"\$RUN_DIR\"' os/hardware-gate/capture-harness/run-capture.sh && rg -q 'refusing to reset unexpected hardware-gate run dir' os/hardware-gate/capture-harness/run-capture.sh && rg -Fq 'if [ \"\$RUN_DIR\" != \"\$RUN_ROOT/\$DATE\" ]' os/hardware-gate/capture-harness/run-capture.sh && rg -Fq '[ \"\$(dirname \"\$RUN_DIR\")\" != \"\$RUN_ROOT\" ]' os/hardware-gate/capture-harness/run-capture.sh"
check "capture harness rejects stale GDM screenshot sets" "rg -Fq 'stable_frame_hash' os/hardware-gate/capture-harness/run-capture.sh && rg -Fq 'cropping the top bar' os/hardware-gate/capture-harness/run-capture.sh && rg -Fq 'macOS sips' os/hardware-gate/capture-harness/run-capture.sh && rg -Fq 'Refusing stale screenshot signoff' os/hardware-gate/capture-harness/run-capture.sh"
check "capture harness waits for unique required screenshots" "rg -Fq 'REQUIRED_FRAME_SETTLE_SECONDS' os/hardware-gate/capture-harness/drive-capture.py && rg -Fq 'framebuffer stayed duplicate' os/hardware-gate/capture-harness/drive-capture.py && rg -Fq \"! -name '_debug-*'\" os/hardware-gate/capture-harness/run-capture.sh && rg -Fq 'required captured surfaces are distinct' os/hardware-gate/capture-harness/run-capture.sh"
check "capture driver uses progress-aware ready-signal timeouts" "rg -Fq 'GOS_CAPTURE_TOTAL_TIMEOUT_SECONDS' os/hardware-gate/capture-harness/drive-capture.py && rg -Fq 'GOS_CAPTURE_INACTIVITY_TIMEOUT_SECONDS' os/hardware-gate/capture-harness/drive-capture.py && rg -Fq 'last_progress = time.monotonic()' os/hardware-gate/capture-harness/drive-capture.py && rg -Fq 'EXPECTED_READY_SHOTS' os/hardware-gate/capture-harness/drive-capture.py && rg -Fq 'seconds_since_progress' os/hardware-gate/capture-harness/drive-capture.py"
check "capture harness launches current-session nonunique proof windows" "rg -Fq 'GOBLINS_OS_CAPTURE_NON_UNIQUE=1' os/hardware-gate/capture-harness/in-session-orchestrator.sh && rg -Fq 'GOS_SHOT_SETTLE_SECONDS' os/hardware-gate/capture-harness/in-session-orchestrator.sh && rg -Fq 'pkill -x \"\$base\"' os/hardware-gate/capture-harness/in-session-orchestrator.sh && rg -Fq 'pkill -f -- \"\$bin\"' os/hardware-gate/capture-harness/in-session-orchestrator.sh && ! rg -Fq 'dbus-run-session -- \"\$@\"' os/hardware-gate/capture-harness/in-session-orchestrator.sh"
check "capture harness bounds ready signals and shot helper cleanup" "rg -Fq 'GOS_READY_SIGNAL_TIMEOUT_SECONDS' os/hardware-gate/capture-harness/in-session-orchestrator.sh && rg -Fq 'GOS_SHOT_HELPER_TIMEOUT_SECONDS' os/hardware-gate/capture-harness/in-session-orchestrator.sh && rg -Fq 'GOBLINS_HWGATE_BOUNDED_COMMAND_TIMED_OUT' os/hardware-gate/capture-harness/in-session-orchestrator.sh && rg -Fq 'timeout -k 2s' os/hardware-gate/capture-harness/in-session-orchestrator.sh && rg -Fq 'GOBLINS_HWGATE_SHOT_SIGNALING' os/hardware-gate/capture-harness/in-session-orchestrator.sh"
check "capture harness routes installer screenshots through fixture core" "rg -Fq 'installer_shot()' os/hardware-gate/capture-harness/in-session-orchestrator.sh && rg -Fq 'GOBLINS_OS_INSTALLER_CORE_WAIT_SECS' os/hardware-gate/capture-harness/in-session-orchestrator.sh && rg -Fq 'GOS_INSTALLER_CAPTURE_CORE_WAIT_SECS' os/hardware-gate/capture-harness/in-session-orchestrator.sh && rg -Fq 'installer_shot welcome 06-onboarding' os/hardware-gate/capture-harness/in-session-orchestrator.sh && rg -Fq 'installer_shot network 02-install-network' os/hardware-gate/capture-harness/in-session-orchestrator.sh && rg -Fq 'Some(\"welcome\") => \"welcome\"' crates/goblins-os-installer/src/main.rs"
check "capture GTK apps support nonunique proof instances" "rg -Fq 'GOBLINS_OS_CAPTURE_NON_UNIQUE' crates/goblins-os-shell/src/main.rs && rg -Fq 'ApplicationFlags::NON_UNIQUE' crates/goblins-os-shell/src/main.rs && rg -Fq 'GOBLINS_OS_CAPTURE_NON_UNIQUE' crates/goblins-os-settings/src/main.rs && rg -Fq 'ApplicationFlags::NON_UNIQUE' crates/goblins-os-settings/src/main.rs && rg -Fq 'GOBLINS_OS_CAPTURE_NON_UNIQUE' crates/goblins-os-installer/src/main.rs && rg -Fq 'ApplicationFlags::NON_UNIQUE' crates/goblins-os-installer/src/main.rs"
check "capture fixture is a fail-safe root-controlled same-socket swap" "rg -Fq 'core_proof_request fixture-start' os/hardware-gate/capture-harness/in-session-orchestrator.sh && rg -Fq 'core_proof_request fixture-restore' os/hardware-gate/capture-harness/in-session-orchestrator.sh && rg -Fq 'FIXTURE_SWAP_IN_PROGRESS=true' os/hardware-gate/capture-harness/core-proof-operation.sh && rg -Fq 'wait_for_fixture_resident' os/hardware-gate/capture-harness/core-proof-operation.sh && rg -Fq 'fail_fixture_start' os/hardware-gate/capture-harness/core-proof-operation.sh && rg -Fq 'restore_production_services' os/hardware-gate/capture-harness/core-proof-operation.sh && rg -Fq 'ExecStopPost=-/etc/goblins-os/hardware-gate/goblins-hwgate-core-proof-operation %i-finished \${SERVICE_RESULT}' os/iso/verify-config.toml && rg -Fq 'StandardOutput=append:/run/goblins-hwgate-core-proof/fixture-core.log' os/iso/verify-config.toml && rg -Fq 'StandardOutput=append:/run/goblins-hwgate-core-proof/fixture-resident.log' os/iso/verify-config.toml && ! rg -q 'append:/tmp/fix(core|res)[.]log' os/iso/verify-config.toml && rg -Fq 'RuntimeMaxSec=1800' os/iso/verify-config.toml && rg -Fq 'ExecStopPost=-+/etc/goblins-os/hardware-gate/goblins-hwgate-core-proof-operation fixture-core-stopped' os/iso/verify-config.toml"
check "capture harness disables switch overlay before screenshots" "rg -Fq 'gnome-extensions disable goblins-switch@goblins.os' os/hardware-gate/capture-harness/in-session-orchestrator.sh && rg -Fq 'globalThis.goblinsSwitchControl' os/hardware-gate/capture-harness/in-session-orchestrator.sh"
check "hardware gate requires Input sources roundtrip proof" "rg -q 'input-sources-roundtrip-proof.json' os/hardware-gate/capture-harness/drive-capture.py os/hardware-gate/capture-harness/run-capture.sh os/hardware-gate/close-signoff.sh os/hardware-gate/verify-shipping-status.sh && rg -q '/proof/input-sources-roundtrip' os/hardware-gate/capture-harness/in-session-orchestrator.sh && rg -q '/v1/input/sources' os/hardware-gate/capture-harness/in-session-orchestrator.sh os/hardware-gate/capture-harness/run-capture.sh && rg -q '/v1/input/switch-next' os/hardware-gate/capture-harness/in-session-orchestrator.sh os/hardware-gate/capture-harness/run-capture.sh && rg -q 'test_sources=xkb-us,xkb-gb' os/hardware-gate/capture-harness/in-session-orchestrator.sh && rg -q 'sources_gsettings_readback=true' os/hardware-gate/capture-harness/in-session-orchestrator.sh os/hardware-gate/capture-harness/run-capture.sh && rg -q 'switch_switched=true' os/hardware-gate/capture-harness/in-session-orchestrator.sh os/hardware-gate/capture-harness/run-capture.sh && rg -q 'restore_sources=true' os/hardware-gate/capture-harness/in-session-orchestrator.sh os/hardware-gate/capture-harness/run-capture.sh && rg -q 'Input sources roundtrip checked' os/hardware-gate/close-signoff.sh os/hardware-gate/verify-shipping-status.sh"
check "hardware gate requires Focus arm roundtrip proof" "rg -q 'focus-arm-roundtrip-proof.json' os/hardware-gate/capture-harness/drive-capture.py os/hardware-gate/capture-harness/run-capture.sh os/hardware-gate/close-signoff.sh os/hardware-gate/verify-shipping-status.sh && rg -q '/proof/focus-arm-roundtrip' os/hardware-gate/capture-harness/in-session-orchestrator.sh && rg -q '/v1/focus/status' os/hardware-gate/capture-harness/in-session-orchestrator.sh os/hardware-gate/capture-harness/run-capture.sh && rg -q '/v1/focus/activate' os/hardware-gate/capture-harness/in-session-orchestrator.sh os/hardware-gate/capture-harness/run-capture.sh && rg -q '/v1/focus/deactivate' os/hardware-gate/capture-harness/in-session-orchestrator.sh os/hardware-gate/capture-harness/run-capture.sh && rg -q 'active_mode_gsettings_readback=gate-work' os/hardware-gate/capture-harness/in-session-orchestrator.sh os/hardware-gate/capture-harness/run-capture.sh && rg -q 'notification_banners_after_activate=false' os/hardware-gate/capture-harness/in-session-orchestrator.sh os/hardware-gate/capture-harness/run-capture.sh && rg -q 'original_notification_banners_restored=true' os/hardware-gate/capture-harness/in-session-orchestrator.sh os/hardware-gate/capture-harness/run-capture.sh && rg -q 'mode_crud_claim=false' os/hardware-gate/capture-harness/in-session-orchestrator.sh os/hardware-gate/capture-harness/run-capture.sh && rg -q 'Focus arm roundtrip checked' os/hardware-gate/close-signoff.sh os/hardware-gate/verify-shipping-status.sh"
check "hardware gate requires App privacy revoke proof" "rg -q 'app-privacy-revoke-proof.json' os/hardware-gate/capture-harness/drive-capture.py os/hardware-gate/capture-harness/run-capture.sh os/hardware-gate/close-signoff.sh os/hardware-gate/verify-shipping-status.sh && rg -q '/proof/app-privacy-revoke' os/hardware-gate/capture-harness/in-session-orchestrator.sh && rg -q '/v1/app-privacy/revoke' os/hardware-gate/capture-harness/in-session-orchestrator.sh os/hardware-gate/capture-harness/run-capture.sh && rg -q 'PermissionStore.SetPermission' os/hardware-gate/capture-harness/in-session-orchestrator.sh os/hardware-gate/capture-harness/run-capture.sh && rg -q 'PermissionStore.DeletePermission' os/hardware-gate/capture-harness/in-session-orchestrator.sh os/hardware-gate/capture-harness/run-capture.sh && rg -q 'PermissionStore.GetPermission' os/hardware-gate/capture-harness/in-session-orchestrator.sh os/hardware-gate/capture-harness/run-capture.sh && rg -q 'org.goblins.GatePrivacyProof' os/hardware-gate/capture-harness/in-session-orchestrator.sh os/hardware-gate/capture-harness/run-capture.sh && rg -q 'post_revoke_absent=true' os/hardware-gate/capture-harness/in-session-orchestrator.sh os/hardware-gate/capture-harness/run-capture.sh && rg -q 'restore_prior_state=true' os/hardware-gate/capture-harness/in-session-orchestrator.sh os/hardware-gate/capture-harness/run-capture.sh && rg -q 'resource_keyed_claim=false' os/hardware-gate/capture-harness/in-session-orchestrator.sh os/hardware-gate/capture-harness/run-capture.sh && rg -q 'App privacy revoke checked' os/hardware-gate/close-signoff.sh os/hardware-gate/verify-shipping-status.sh"
check "hardware gate records App privacy seed fallback diagnostics" "rg -q 'plain_permissions' os/hardware-gate/capture-harness/in-session-orchestrator.sh && rg -q 'seed_attempt=' os/hardware-gate/capture-harness/in-session-orchestrator.sh && rg -q 'seed_error=' os/hardware-gate/capture-harness/in-session-orchestrator.sh && rg -q 'proof_query_value' os/hardware-gate/capture-harness/in-session-orchestrator.sh && rg -q '/var/home/goblin/.local/share/flatpak/db' os/bootc/Containerfile && rg -q '.local/share/flatpak/db' os/hardware-gate/capture-harness/in-session-orchestrator.sh && rg -q 'permission-db-dir' os/hardware-gate/capture-harness/in-session-orchestrator.sh"
check "hardware gate requires Preview open/render proof" "rg -q 'preview-open-render-proof.json' os/hardware-gate/capture-harness/drive-capture.py os/hardware-gate/capture-harness/run-capture.sh os/hardware-gate/close-signoff.sh os/hardware-gate/verify-shipping-status.sh && rg -q '/proof/preview-open-render' os/hardware-gate/capture-harness/in-session-orchestrator.sh && rg -q '/v1/preview/status' os/hardware-gate/capture-harness/in-session-orchestrator.sh os/hardware-gate/capture-harness/run-capture.sh && rg -q '/v1/preview/open' os/hardware-gate/capture-harness/in-session-orchestrator.sh os/hardware-gate/capture-harness/run-capture.sh && rg -q 'org.gnome.Papers.desktop' os/hardware-gate/capture-harness/in-session-orchestrator.sh os/hardware-gate/capture-harness/run-capture.sh && rg -q 'org.gnome.Loupe.desktop' os/hardware-gate/capture-harness/in-session-orchestrator.sh os/hardware-gate/capture-harness/run-capture.sh && rg -q '29-preview-pdf-open.png' os/hardware-gate/capture-harness/in-session-orchestrator.sh os/hardware-gate/capture-harness/run-capture.sh os/hardware-gate/close-signoff.sh os/hardware-gate/verify-shipping-status.sh && rg -q '30-preview-image-open.png' os/hardware-gate/capture-harness/in-session-orchestrator.sh os/hardware-gate/capture-harness/run-capture.sh os/hardware-gate/close-signoff.sh os/hardware-gate/verify-shipping-status.sh && rg -q 'unsupported_rejected=true' os/hardware-gate/capture-harness/in-session-orchestrator.sh && rg -q 'Preview open/render checked' os/hardware-gate/close-signoff.sh os/hardware-gate/verify-shipping-status.sh"
check "hardware gate requires Audio output proof" "rg -q 'audio-output-proof.json' os/hardware-gate/capture-harness/drive-capture.py os/hardware-gate/capture-harness/run-capture.sh os/hardware-gate/close-signoff.sh os/hardware-gate/verify-shipping-status.sh os/hardware-gate/runbook.md && rg -q '/proof/audio-output' os/hardware-gate/capture-harness/in-session-orchestrator.sh && rg -Fq 'core_proof_request audio-status \"\$status_file\" || true' os/hardware-gate/capture-harness/in-session-orchestrator.sh && rg -Fq 'audio-status) request audio-status GET /v1/audio/status ;;' os/hardware-gate/capture-harness/core-proof-operation.sh && rg -q 'pw-play' os/hardware-gate/capture-harness/in-session-orchestrator.sh os/bootc/Containerfile && rg -Fq -- '-audiodev none,id=audio0' os/hardware-gate/capture-harness/run-capture.sh && rg -q 'ich9-intel-hda' os/hardware-gate/capture-harness/run-capture.sh && rg -q 'hda-output,audiodev=audio0' os/hardware-gate/capture-harness/run-capture.sh && rg -q 'GOBLINS_OS_CAPTURE_EXPECT_TITLE=\"Goblins OS Settings - Sound\"' os/hardware-gate/capture-harness/in-session-orchestrator.sh && rg -Fq 'GOS_SHOT_WINDOW_WAIT_ATTEMPTS=\"\${GOS_AUDIO_SHOT_WINDOW_WAIT_ATTEMPTS:-8}\"' os/hardware-gate/capture-harness/in-session-orchestrator.sh && rg -Fq 'GOS_SHOT_HELPER_TIMEOUT_SECONDS=\"\${GOS_AUDIO_SHOT_HELPER_TIMEOUT_SECONDS:-1}\"' os/hardware-gate/capture-harness/in-session-orchestrator.sh && rg -Fq 'one_second = bytearray()' os/hardware-gate/capture-harness/in-session-orchestrator.sh && rg -Fq 'for _ in range(seconds):' os/hardware-gate/capture-harness/in-session-orchestrator.sh && rg -Fq 'local timeout=30' os/hardware-gate/capture-harness/core-proof-operation.sh && rg -Fq -- '--connect-timeout 2' os/hardware-gate/capture-harness/core-proof-operation.sh && rg -q 'GOS_AUDIO_STATUS_ATTEMPTS' os/hardware-gate/capture-harness/in-session-orchestrator.sh && rg -q 'GOBLINS_HWGATE_AUDIO_WAV_GENERATION_TIMED_OUT' os/hardware-gate/capture-harness/in-session-orchestrator.sh && rg -Fq 'wav_generated=\$wav_generated' os/hardware-gate/capture-harness/in-session-orchestrator.sh && rg -q 'GOBLINS_OS_WPCTL_TIMEOUT_MS' crates/goblins-os-core/src/audio.rs && rg -q 'clamp_wpctl_timeout_ms' crates/goblins-os-core/src/audio.rs && rg -q 'audio_device_snapshot' crates/goblins-os-core/src/audio.rs && rg -q 'bounded_session_command_output' crates/goblins-os-core/src/audio.rs && rg -q 'try_wait()' crates/goblins-os-core/src/bounded.rs && rg -q 'WirePlumber did not answer before the audio status timeout.' crates/goblins-os-core/src/audio.rs && rg -q '24-audio-output.png' os/hardware-gate/capture-harness/in-session-orchestrator.sh os/hardware-gate/capture-harness/run-capture.sh os/hardware-gate/close-signoff.sh os/hardware-gate/verify-shipping-status.sh && rg -q 'Audio output checked' os/hardware-gate/close-signoff.sh os/hardware-gate/verify-shipping-status.sh"
check "hardware gate audio proof reports core service diagnostics" "rg -q 'core_probe_http' os/hardware-gate/capture-harness/in-session-orchestrator.sh && rg -q 'audio_core_service_diag' os/hardware-gate/capture-harness/in-session-orchestrator.sh && rg -q 'core_restarts=' os/hardware-gate/capture-harness/in-session-orchestrator.sh && rg -Fq 'Restart=always' os/systemd/goblins-os-core.service && rg -Fq 'StartLimitIntervalSec=0' os/systemd/goblins-os-core.service && rg -q 'GOBLINS_OS_CAPTURE_PRESENT_LEDGER' os/hardware-gate/capture-harness/in-session-orchestrator.sh && rg -q 'GOBLINS_OS_CAPTURE_PRESENT_LEDGER' crates/goblins-os-settings/src/main.rs && rg -Fq 'Restart=always' os/systemd-user/org.goblins.OS.SessionBridge.service && rg -Fq 'StartLimitIntervalSec=0' os/systemd-user/org.goblins.OS.SessionBridge.service"
check "IME CJK engine packages are source-gated" "rg -q 'ibus-libpinyin' os/bootc/Containerfile crates/goblins-os-core/src/input.rs && rg -q 'ibus-anthy' os/bootc/Containerfile crates/goblins-os-core/src/input.rs && rg -q 'ibus-hangul' os/bootc/Containerfile crates/goblins-os-core/src/input.rs && rg -q '/usr/share/ibus/component/libpinyin.xml' os/bootc/Containerfile crates/goblins-os-core/src/input.rs && rg -q '/usr/share/ibus/component/anthy.xml' os/bootc/Containerfile crates/goblins-os-core/src/input.rs && rg -q '/usr/share/ibus/component/hangul.xml' os/bootc/Containerfile crates/goblins-os-core/src/input.rs && rg -q '/usr/libexec/ibus-engine-libpinyin' os/bootc/Containerfile crates/goblins-os-core/src/input.rs && rg -q '/usr/libexec/ibus-engine-anthy' os/bootc/Containerfile crates/goblins-os-core/src/input.rs && rg -q '/usr/libexec/ibus-engine-hangul' os/bootc/Containerfile crates/goblins-os-core/src/input.rs && rg -q '/usr/lib64/gtk-4.0/4.0.0/immodules/libim-ibus.so' os/bootc/Containerfile && rg -q 'CJK engine packages' crates/goblins-os-settings/src/main.rs"
check "core audio probes WirePlumber through the session bridge" "rg -q 'Wpctl' crates/goblins-os-session-bridge/src/main.rs crates/goblins-os-core/src/session_bridge.rs && rg -q 'validate_wpctl_args' crates/goblins-os-session-bridge/src/main.rs && rg -q 'WirePlumber did not answer before the session bridge audio timeout.' crates/goblins-os-session-bridge/src/main.rs && rg -q 'org.gnome.desktop.sound' crates/goblins-os-session-bridge/src/main.rs && rg -q 'gsettings did not answer before the session bridge preference timeout.' crates/goblins-os-session-bridge/src/main.rs && rg -Fq 'pub(crate) fn wpctl' crates/goblins-os-core/src/session_bridge.rs && rg -q 'BRIDGE_IO_TIMEOUT' crates/goblins-os-core/src/session_bridge.rs && rg -q '"list-recursively"' crates/goblins-os-session-bridge/src/main.rs && rg -Fq 'session_bridge::wpctl(args)' crates/goblins-os-core/src/audio.rs && rg -q 'audio_endpoint_ready_without_volume_detail' crates/goblins-os-core/src/audio.rs && rg -Fq 'parse_wpctl_volume(suffix)' crates/goblins-os-core/src/audio.rs && rg -Fq 'session_bridge::gsettings(args)' crates/goblins-os-core/src/audio.rs && rg -Fq ', SOUND_SCHEMA])' crates/goblins-os-core/src/audio.rs && rg -q 'parse_sound_schema_snapshot' crates/goblins-os-core/src/audio.rs && rg -q 'audio_endpoint_default_volume_status' crates/goblins-os-core/src/audio.rs && rg -Fq 'wpctl(&[\"get-volume\", target.wpctl_id()])' crates/goblins-os-core/src/audio.rs && rg -q 'Audio device readiness does not wait for desktop sound preferences.' crates/goblins-os-core/src/audio.rs"
check "voice session bridge is typed exclusive private and fail-closed" "rg -Fq 'VoiceAudioStatus {}' crates/goblins-os-session-bridge/src/main.rs && rg -Fq 'VoiceCapture {}' crates/goblins-os-session-bridge/src/main.rs && rg -Fq 'bounded_input_output_of(command, wav, VOICE_PLAYBACK_TIMEOUT)' crates/goblins-os-session-bridge/src/main.rs && rg -Fq 'This operation requires the authenticated core service peer.' crates/goblins-os-session-bridge/src/main.rs && rg -Fq 'Some(CAPTURE_PCM_FORMAT)' crates/goblins-os-core/src/session_bridge.rs && rg -Fq 'run_voice_blocking' crates/goblins-os-core/src/voice.rs crates/goblins-os-core/src/voice_control.rs && rg -Fq 'play_audio(reply_wav.path())?' crates/goblins-os-core/src/voice.rs && rg -Fq 'voice::purge_stale_voice_workspaces()?;' crates/goblins-os-core/src/main.rs && rg -Fq 'd /var/lib/goblins-os/voice/work 0700 goblins-os goblins-os -' os/tmpfiles/goblins-os-core.conf && rg -Fxq 'UMask=0077' os/systemd/goblins-os-core.service && rg -Fxq 'UMask=0077' os/systemd-user/org.goblins.OS.SessionBridge.service"
check "core keyboard rebinding exposes allowlisted write routes" "rg -q '/v1/keyboard/shortcuts/binding' crates/goblins-os-core/src/main.rs && rg -q '/v1/keyboard/modifier-remap' crates/goblins-os-core/src/main.rs && rg -q 'shortcut_conflict' crates/goblins-os-core/src/shortcuts.rs && rg -q 'remap_caps_lock_options' crates/goblins-os-core/src/shortcuts.rs"
check "settings keyboard exposes protected shortcut editing" "rg -Fq '/v1/keyboard/shortcuts/binding' crates/goblins-os-core/src/main.rs crates/goblins-os-core/src/control_plane.rs crates/goblins-os-settings/src/main.rs && rg -Fq '/v1/keyboard/modifier-remap' crates/goblins-os-core/src/main.rs crates/goblins-os-core/src/control_plane.rs crates/goblins-os-settings/src/main.rs && rg -Fq 'Recording keyboard shortcut' crates/goblins-os-settings/src/main.rs && rg -Fq 'Caps Lock works as Control.' crates/goblins-os-settings/src/main.rs"
check "hardware gate requires Keyboard shortcuts roundtrip proof" "rg -q 'keyboard-shortcuts-roundtrip-proof.json' os/hardware-gate/capture-harness/drive-capture.py os/hardware-gate/capture-harness/run-capture.sh os/hardware-gate/close-signoff.sh os/hardware-gate/verify-shipping-status.sh && rg -q '/proof/keyboard-shortcuts-roundtrip' os/hardware-gate/capture-harness/in-session-orchestrator.sh && rg -q '/v1/keyboard/shortcuts/binding' os/hardware-gate/capture-harness/in-session-orchestrator.sh os/hardware-gate/capture-harness/run-capture.sh && rg -q '/v1/keyboard/modifier-remap' os/hardware-gate/capture-harness/in-session-orchestrator.sh os/hardware-gate/capture-harness/run-capture.sh && rg -q 'shortcut_binding=%3CSuper%3E%3CShift%3EH' os/hardware-gate/capture-harness/in-session-orchestrator.sh && rg -q 'shortcut_gsettings_readback=true' os/hardware-gate/capture-harness/in-session-orchestrator.sh os/hardware-gate/capture-harness/run-capture.sh && rg -q 'modifier_gsettings_readback=ctrl:nocaps' os/hardware-gate/capture-harness/in-session-orchestrator.sh && rg -q 'roundtrip_restored=true' os/hardware-gate/capture-harness/in-session-orchestrator.sh os/hardware-gate/capture-harness/run-capture.sh && rg -q 'Keyboard shortcuts roundtrip checked' os/hardware-gate/close-signoff.sh os/hardware-gate/verify-shipping-status.sh"
check "Migration source scan is source-gated" "rg -q '/v1/migration/sources' crates/goblins-os-core/src/main.rs && rg -q 'build_migration_sources' crates/goblins-os-core/src/migration.rs && rg -q 'scan_migration_source_partitions_in' crates/goblins-os-core/src/install_targets.rs && rg -q 'migration_filesystem_readability' crates/goblins-os-core/src/migration.rs && rg -q 'migration_sources_classify_sysfs_partitions_without_mounting' crates/goblins-os-core/src/migration.rs && rg -q '/proc/self/mountinfo' crates/goblins-os-core/src/migration.rs && rg -q 'scan_errors' crates/goblins-os-core/src/migration.rs && rg -q 'partial' crates/goblins-os-core/src/migration.rs && rg -Fq \"Goblins can't read this disk's format (APFS).\" crates/goblins-os-core/src/migration.rs && rg -q 'executes_live_mount: false' crates/goblins-os-core/src/migration.rs && rg -q 'executes_live_copy: false' crates/goblins-os-core/src/migration.rs && rg -Fq 'Migration source scan is ready. No disks were mounted and no files were copied by this source scan.' crates/goblins-os-core/src/migration.rs"
check "Migration copy plan and packages are source-gated" "rg -q '/v1/migration/copy-plan' crates/goblins-os-core/src/main.rs && rg -q 'build_migration_copy_plan' crates/goblins-os-core/src/migration.rs && rg -q -- '--info=progress2' crates/goblins-os-core/src/migration.rs && rg -q -- '--ignore-existing' crates/goblins-os-core/src/migration.rs && rg -q 'executes_live_copy: false' crates/goblins-os-core/src/migration.rs && rg -q 'ntfs-3g' os/bootc/Containerfile && rg -q 'exfatprogs' os/bootc/Containerfile && rg -q 'udisks2' os/bootc/Containerfile && rg -q 'rsync' os/bootc/Containerfile && rg -q 'command -v ntfs-3g' os/bootc/Containerfile && rg -q 'command -v mount.ntfs-3g' os/bootc/Containerfile && rg -q 'command -v fsck.exfat' os/bootc/Containerfile && rg -q 'command -v udisksctl' os/bootc/Containerfile && rg -q 'command -v rsync' os/bootc/Containerfile && rg -q '/usr/lib/systemd/system/udisks2.service' os/bootc/Containerfile"
check "Migration category sizing is source-gated" "rg -q '/v1/migration/estimate' crates/goblins-os-core/src/main.rs && rg -q 'build_migration_estimate' crates/goblins-os-core/src/migration.rs && rg -q 'file_type.is_symlink()' crates/goblins-os-core/src/migration.rs && rg -q 'No files were mounted or copied by this sizing step.' crates/goblins-os-core/src/migration.rs && rg -q 'executes_live_copy: false' crates/goblins-os-core/src/migration.rs"
check "Migration copy job progress substrate is source-gated" "rg -q '/v1/migration/start' crates/goblins-os-core/src/main.rs && rg -q '/v1/migration/progress' crates/goblins-os-core/src/main.rs && rg -q 'build_migration_start_response' crates/goblins-os-core/src/migration.rs && rg -Fq 'Migration copy job is planned. No files were copied by this start substrate.' crates/goblins-os-core/src/migration.rs && rg -Fq 'Live migration copy execution is CI/qemu-gated; this source substrate did not run rsync.' crates/goblins-os-core/src/migration.rs && rg -q 'StatusCode::PRECONDITION_REQUIRED' crates/goblins-os-core/src/migration.rs && rg -q 'OnceLock<Mutex<MigrationCopyProgress>>' crates/goblins-os-core/src/migration.rs && rg -q 'refresh_migration_copy_progress_from_logs' crates/goblins-os-core/src/migration.rs && rg -q 'progress.log' crates/goblins-os-core/src/migration.rs && rg -q 'parse_rsync_progress_line' crates/goblins-os-core/src/migration.rs && rg -q 'parse_migration_ledger_counts' crates/goblins-os-core/src/migration.rs && rg -q 'count_migration_skipped_ledger_entries' crates/goblins-os-core/src/migration.rs && rg -q 'executes_live_copy: false' crates/goblins-os-core/src/migration.rs"
check "Snapshots recovery is confirmation-gated and atomically non-overwriting" "rg -q '/v1/snapshots/status' crates/goblins-os-core/src/main.rs && rg -q '/v1/snapshots/browse' crates/goblins-os-core/src/main.rs && rg -q '/v1/snapshots/restore' crates/goblins-os-core/src/main.rs && rg -q 'ListSnapshots' crates/goblins-os-core/src/snapshots.rs crates/goblins-os-snapshot-broker/src/main.rs && rg -q '/proc/self/mountinfo' crates/goblins-os-core/src/snapshots.rs && rg -Fq 'RECOVER FILE COPY' crates/goblins-os-core/src/snapshots.rs crates/goblins-os-settings/src/main.rs && rg -q 'RecoverSnapshotFile' crates/goblins-os-core/src/session_bridge.rs crates/goblins-os-session-bridge/src/main.rs && rg -q 'open_dir_nofollow' crates/goblins-os-snapshot-broker/src/main.rs && rg -q 'libc::O_TMPFILE' crates/goblins-os-session-bridge/src/main.rs && rg -q 'libc::AT_SYMLINK_FOLLOW' crates/goblins-os-session-bridge/src/main.rs && rg -Fq 'recovered files are never overwritten' crates/goblins-os-session-bridge/src/main.rs && rg -q 'btrfs-progs' os/bootc/Containerfile && rg -q 'libbtrfsutil' os/bootc/Containerfile && rg -q 'command -v btrfs' os/bootc/Containerfile && rg -q 'append_storage_snapshots_status' crates/goblins-os-settings/src/main.rs && rg -q 'append_recovery_snapshots_status' crates/goblins-os-settings/src/main.rs && rg -q 'append_snapshot_recovery_controls' crates/goblins-os-settings/src/main.rs"
check "New installs create a dedicated Btrfs home subvolume before Snapper" "rg -Fq 'type = \"btrfs\"' os/bootc-install/00-goblins-os.toml && rg -Fq 'ROOTFS=\"\${GOBLINS_OS_ROOTFS:-btrfs}\"' os/iso/build-iso.sh && test -x os/bootc/goblins-os-snapshots-setup && test -f os/systemd-system/goblins-os-snapshots-setup.service && test -f os/systemd-system/gdm.service.d/10-goblins-os-snapshots.conf && test -f os/systemd-system/goblins-os-snapshot-broker.service && rg -Fq '#!/bin/sh' os/bootc/goblins-os-snapshots-setup && rg -Fq 'FRESH_MARKER=\$LAYOUT_STATE/initialize-home-v1' os/bootc/goblins-os-snapshots-setup && rg -Fq 'filesystem_probe=/var' os/bootc/goblins-os-snapshots-setup && rg -Fq \"stat -c '%u:%g:%a:%s'\" os/bootc/goblins-os-snapshots-setup && rg -Fq '/usr/sbin/matchpathcon -V \"\$HOME_PATH\"' os/bootc/goblins-os-snapshots-setup && ! rg -Fq 'restorecon -RF /etc/snapper \"\$subvolume/.snapshots\"' os/bootc/goblins-os-snapshots-setup && rg -Fq '\"\$BTRFS\" subvolume create \"\$HOME_PATH\"' os/bootc/goblins-os-snapshots-setup && rg -Fq '/usr/bin/cp -a --reflink=auto --one-file-system' os/bootc/goblins-os-snapshots-setup && rg -Fq '\"\$BTRFS\" subvolume show \"\$subvolume\"' os/bootc/goblins-os-snapshots-setup && rg -Fq 'leaving Recovery unavailable' os/bootc/goblins-os-snapshots-setup && rg -Fq '\"\$SNAPPER\" -c home create-config \"\$subvolume\"' os/bootc/goblins-os-snapshots-setup && rg -Fq 'ALLOW_GROUPS=' os/bootc/goblins-os-snapshots-setup && ! rg -Fq 'ALLOW_GROUPS=goblins-' os/bootc/goblins-os-snapshots-setup && rg -Fq 'SYNC_ACL=yes' os/bootc/goblins-os-snapshots-setup && rg -Fq 'Existing XFS installs remain supported and are never converted in place.' os/bootc/goblins-os-snapshots-setup && ! rg -q 'mkfs|btrfs-convert' os/bootc/goblins-os-snapshots-setup && rg -Fq 'ExecStart=/usr/libexec/goblins-os/goblins-os-snapshots-setup' os/systemd-system/goblins-os-snapshots-setup.service && rg -Fq -- '-/var/home.goblins-os-seed-v1' os/systemd-system/goblins-os-snapshots-setup.service && rg -Fxq 'Requires=goblins-os-snapshots-setup.service' os/systemd-system/gdm.service.d/10-goblins-os-snapshots.conf && rg -Fxq 'After=goblins-os-snapshots-setup.service' os/systemd-system/gdm.service.d/10-goblins-os-snapshots.conf && rg -Fxq 'Group=goblins-snapshot-readers' os/systemd-system/goblins-os-snapshot-broker.service && rg -Fxq 'SupplementaryGroups=goblins-snapshots' os/systemd-system/goblins-os-snapshot-broker.service && rg -Fq 'systemctl enable goblins-os-snapshots-setup.service' os/bootc/Containerfile && rg -Fq 'systemctl enable goblins-os-snapshot-broker.service' os/bootc/Containerfile && rg -Fq 'systemctl enable snapper-timeline.timer' os/bootc/Containerfile && rg -Fq 'systemctl enable snapper-cleanup.timer' os/bootc/Containerfile"
check "Immutable image actions use a fixed privileged bridge" "rg -q '/v1/system/image/action' crates/goblins-os-core/src/main.rs crates/goblins-os-core/src/control_plane.rs crates/goblins-os-settings/src/main.rs && rg -q 'APPLY UPDATE AND RESTART' crates/goblins-os-core/src/system_image.rs crates/goblins-os-settings/src/main.rs && test -x os/bootc/goblins-os-system-update && test -f os/systemd-system/goblins-os-system-update@.service && test -f os/systemd-system/goblins-os-system-reboot.service && test -f os/systemd-system/goblins-os-system-reboot.timer && test -f os/bootc/60-goblins-os-system-update.rules && rg -Fq '#!/bin/sh' os/bootc/goblins-os-system-update && rg -Fq 'exec \"\$BOOTC\" upgrade --apply' os/bootc/goblins-os-system-update && rg -Fq 'exec \"\$BOOTC\" upgrade --from-downloaded --apply' os/bootc/goblins-os-system-update && rg -Fq 'exec /usr/bin/systemctl start --no-block goblins-os-system-reboot.timer' os/bootc/goblins-os-system-update && rg -Fxq 'OnActiveSec=5s' os/systemd-system/goblins-os-system-reboot.timer && rg -Fq 'goblins-os-system-update@(check|download|apply|apply-downloaded|reboot|rollback)' os/bootc/60-goblins-os-system-update.rules && rg -q 'system_update_unit_for_status' crates/goblins-os-core/src/system_image.rs && rg -q 'systemd_system_image_operation' crates/goblins-os-core/src/system_image.rs"
check "Encryption posture substrate is source-gated" "rg -q '/v1/security/encryption' crates/goblins-os-core/src/main.rs && rg -q '/proc/self/mountinfo' crates/goblins-os-core/src/encryption.rs && rg -q '/etc/crypttab' crates/goblins-os-core/src/encryption.rs && rg -Fq 'cryptsetup\", &[\"status\"' crates/goblins-os-core/src/encryption.rs && rg -Fq 'systemd-cryptenroll\", &[\"--list\"' crates/goblins-os-core/src/encryption.rs && rg -q 'executes_enrollment: false' crates/goblins-os-core/src/encryption.rs && rg -Fq 'must not enable TPM-only install without escrow' crates/goblins-os-core/src/encryption.rs && rg -q 'cryptsetup' os/bootc/Containerfile && rg -q 'tpm2-tss' os/bootc/Containerfile && rg -q 'command -v cryptsetup' os/bootc/Containerfile && rg -q 'command -v systemd-cryptenroll' os/bootc/Containerfile && rg -q '/usr/lib64/libtss2-esys.so.0' os/bootc/Containerfile && ! rg -q 'systemd-cryptsetup' os/bootc/Containerfile && rg -q '/v1/security/encryption' crates/goblins-os-settings/src/main.rs && rg -q 'append_security_encryption_status' crates/goblins-os-settings/src/main.rs && rg -Fq 'Recovery-key minting and TPM enrollment remain installer and hardware-gated' crates/goblins-os-settings/src/main.rs"
check "Migration preference import plan is source-gated" "rg -q '/v1/migration/preference-plan' crates/goblins-os-core/src/main.rs && rg -q 'build_migration_preference_plan' crates/goblins-os-core/src/migration.rs && rg -Fq 'Migration preference import plan is ready. No preferences were written by this source substrate.' crates/goblins-os-core/src/migration.rs && rg -q 'parse_dconf_dump' crates/goblins-os-core/src/migration.rs && rg -q 'migration_preference_target' crates/goblins-os-core/src/migration.rs && rg -Fq 'Preference is not in the Goblins OS migration allowlist.' crates/goblins-os-core/src/migration.rs && rg -q 'wallpaper_destination_uri_from_copied_paths' crates/goblins-os-core/src/migration.rs && rg -Fq 'Wallpaper file was not present in the copied-path evidence.' crates/goblins-os-core/src/migration.rs && rg -q 'available_schemas: Option<Vec<String>>' crates/goblins-os-core/src/migration.rs && rg -q 'executes_preference_import: false' crates/goblins-os-core/src/migration.rs"
check "core Focus exposes arm disarm and tick routes" "rg -q '/v1/focus/activate' crates/goblins-os-core/src/main.rs && rg -q '/v1/focus/deactivate' crates/goblins-os-core/src/main.rs && rg -q '/v1/focus/tick' crates/goblins-os-core/src/main.rs"
check "core Focus mode and schedule CRUD is source-gated" "rg -q '/v1/focus/mode' crates/goblins-os-core/src/main.rs && rg -q '/v1/focus/schedule' crates/goblins-os-core/src/main.rs && rg -q 'Delete schedules that use this Focus mode before deleting the mode.' crates/goblins-os-core/src/focus.rs && rg -q 'Focus schedules must be saved with a configured mode.' crates/goblins-os-core/src/focus.rs"
check "settings Focus controls source-gated" "rg -q '/v1/focus/status' crates/goblins-os-settings/src/main.rs && rg -q '/v1/focus/activate' crates/goblins-os-settings/src/main.rs && rg -q '/v1/focus/deactivate' crates/goblins-os-settings/src/main.rs && rg -q 'append_focus_settings' crates/goblins-os-settings/src/main.rs"
check "menu-bar Focus indicator source-gated" "rg -q 'org.goblins.os.focus' os/gnome-shell-extensions/goblins-menubar@goblins.os/extension.js && rg -q 'changed::active-mode' os/gnome-shell-extensions/goblins-menubar@goblins.os/extension.js && rg -Fq 'modes.find(entry => entry.id === activeMode)' os/gnome-shell-extensions/goblins-menubar@goblins.os/extension.js && rg -q -- '--panel=notifications' os/gnome-shell-extensions/goblins-menubar@goblins.os/extension.js && rg -q '.goblins-focus-indicator' os/gnome-shell-extensions/goblins-menubar@goblins.os/stylesheet.css"
check "menu-bar Focus render hook is source-gated" "rg -Fq '59b-menubar-focus-\$suffix.png' os/bootc/render-desktop.sh && rg -Fq '[{\"id\":\"work\",\"name\":\"Deep Work\"}]' os/bootc/render-desktop.sh && rg -q 'gsettings set org.goblins.os.focus active-mode work' os/bootc/render-desktop.sh"
check "control center Focus tile source-gated" "rg -Fq '/v1/focus/status' crates/goblins-os-control-center/src/main.rs && ! rg -Fq '/v1/focus/activate' crates/goblins-os-control-center/src/main.rs && ! rg -Fq '/v1/focus/deactivate' crates/goblins-os-control-center/src/main.rs && rg -Fq -- '--panel=notifications' crates/goblins-os-control-center/src/main.rs && rg -Fq 'status.modes' crates/goblins-os-control-center/src/main.rs && rg -Fq 'No Focus modes are configured yet.' crates/goblins-os-control-center/src/main.rs && rg -Fq 'Focus status is unavailable because Goblins OS core did not respond.' crates/goblins-os-control-center/src/main.rs && rg -Fq 'focus_tile_copy' crates/goblins-os-control-center/src/main.rs"
check "control center Focus render hook is source-gated" "rg -Fq '37b-control-center-focus.png' os/bootc/render-screens.sh && rg -Fq '39b-control-center-focus-dark.png' os/bootc/render-screens.sh && rg -Fq '[{\"id\":\"work\",\"name\":\"Deep Work\"}]' os/bootc/render-screens.sh && rg -q 'gsettings set org.goblins.os.focus active-mode work' os/bootc/render-screens.sh && rg -q \"gsettings set org.goblins.os.focus active-mode ''\" os/bootc/render-screens.sh && rg -Fq \"gsettings set org.goblins.os.focus modes '[]'\" os/bootc/render-screens.sh"
check "core Focus snapshots notification banners through bridge" "rg -q 'restore-banners' crates/goblins-os-core/src/focus.rs os/glib-schemas/org.goblins.os.focus.gschema.xml && rg -q 'apply_notification_banners' crates/goblins-os-core/src/focus.rs crates/goblins-os-core/src/notifications.rs && rg -q 'read_notification_banners' crates/goblins-os-core/src/focus.rs crates/goblins-os-core/src/notifications.rs"
check "Focus schedule timer is source-gated" "rg -Fq 'initialize(ClientKind::FocusTick)' crates/goblins-os-session-tools/src/bin/goblins-os-focus-tick.rs && rg -Fq 'const ROUTE: &str = \"/v1/focus/tick\"' crates/goblins-os-session-tools/src/bin/goblins-os-focus-tick.rs && ! test -e os/focus/goblins-os-focus-tick && test -f os/systemd-user/org.goblins.OS.FocusTick.service && test -f os/systemd-user/org.goblins.OS.FocusTick.timer && rg -q 'ExecStart=/usr/libexec/goblins-os/goblins-os-focus-tick' os/systemd-user/org.goblins.OS.FocusTick.service && rg -q 'OnCalendar=minutely' os/systemd-user/org.goblins.OS.FocusTick.timer && rg -q 'Wants=org.goblins.OS.FocusTick.timer' os/systemd-user/gnome-session@goblins-os.target.d/goblins-os.session.conf && rg -q 'goblins-os-focus-tick' os/bootc/Containerfile && rg -q '\"crates/\"' os/release/source-tree-manifest.toml"
check "core app privacy exposes allowlisted revoke route" "rg -q '/v1/app-privacy/revoke' crates/goblins-os-core/src/main.rs && rg -q 'PermissionStore.DeletePermission' crates/goblins-os-core/src/app_permissions.rs && rg -q 'session_bridge::permission_store_delete_permission' crates/goblins-os-core/src/app_permissions.rs && rg -q 'permission_id_is_safe' crates/goblins-os-core/src/app_permissions.rs"
check "settings app privacy exposes revoke controls" "rg -q 'app_permission_revoke_row' crates/goblins-os-settings/src/main.rs && rg -q '/v1/app-privacy/revoke' crates/goblins-os-settings/src/main.rs"
check "core display apply exposes serial-gated Mutter route" "rg -q '/v1/displays/apply' crates/goblins-os-core/src/main.rs && rg -q 'ApplyMonitorsConfig' crates/goblins-os-core/src/displays.rs && rg -q 'validate_logical_monitors' crates/goblins-os-core/src/displays.rs && rg -q 'Display layout changed before apply' crates/goblins-os-core/src/displays.rs"
check "settings displays reports protected apply gate" "rg -q 'display_apply_detail' crates/goblins-os-settings/src/main.rs && rg -q 'Protected display apply is available' crates/goblins-os-settings/src/main.rs"
check "capture harness proves multi-display apply guarded route" "rg -q '/proof/multi-display-apply' os/hardware-gate/capture-harness/in-session-orchestrator.sh && rg -q '/v1/displays/status' os/hardware-gate/capture-harness/in-session-orchestrator.sh && rg -q '/v1/displays/apply' os/hardware-gate/capture-harness/in-session-orchestrator.sh && rg -q 'org.gnome.Mutter.DisplayConfig.GetCurrentState' os/hardware-gate/capture-harness/in-session-orchestrator.sh && rg -q 'persistent_confirmation_required=true' os/hardware-gate/capture-harness/in-session-orchestrator.sh && rg -q 'stale_serial_rejected=true' os/hardware-gate/capture-harness/in-session-orchestrator.sh && rg -q 'persistent_keep_claim=false' os/hardware-gate/capture-harness/in-session-orchestrator.sh"
check "capture driver persists multi-display apply proof" "rg -q 'multi-display-apply-proof.json' os/hardware-gate/capture-harness/drive-capture.py os/hardware-gate/capture-harness/run-capture.sh && rg -q 'multi-display-apply' os/hardware-gate/capture-harness/drive-capture.py && rg -q 'HONESTY GUARD: missing or failing multi-display apply proof' os/hardware-gate/capture-harness/run-capture.sh"
check "installed self-test checks firewall status and honest toggle route" "rg -q '/v1/firewall/status' os/bootc/run-selftest.sh && rg -q '/v1/firewall/enabled' os/bootc/run-selftest.sh && rg -Fq '502|503) [ \"\$firewall_toggle_ok\" != \"true\" ]' os/bootc/run-selftest.sh && rg -q 'firewall_toggle_body' os/bootc/run-selftest.sh"
check "settings interaction render captures firewall toggle failure" "rg -q 'capture_settings_firewall_toggle_interaction' os/bootc/render-screens.sh && rg -q '118-settings-firewall-before.png' os/bootc/render-screens.sh && rg -q '119-settings-firewall-toggle-failed.png' os/bootc/render-screens.sh"
check "firewall bridge rule is installed in image-owned polkit path" "rg -q '60-goblins-os-firewall.rules /usr/share/polkit-1/rules.d/60-goblins-os-firewall.rules' os/bootc/Containerfile && rg -q '/usr/share/polkit-1/rules.d/60-goblins-os-firewall.rules' crates/goblins-os-core/src/firewall.rs && rg -q '/usr/bin/systemctl' crates/goblins-os-core/src/firewall.rs"
check "firewall helper waits for firewalld and emits diagnostics" "rg -q 'systemctl reset-failed firewalld.service' os/bootc/goblins-os-firewall && rg -q 'systemctl unmask firewalld.service' os/bootc/goblins-os-firewall && rg -q 'systemctl daemon-reload' os/bootc/goblins-os-firewall && rg -q 'systemctl start firewalld.service || /usr/bin/systemctl restart firewalld.service' os/bootc/goblins-os-firewall && rg -Fq 'while [ \"\$i\" -lt 90 ]' os/bootc/goblins-os-firewall && rg -q 'firewall-cmd --state' os/bootc/goblins-os-firewall && rg -q 'firewalld did not report running after enable' os/bootc/goblins-os-firewall && rg -q 'systemctl --no-pager --full status firewalld.service' os/bootc/goblins-os-firewall && rg -Fq 'wait_for_firewall_state(enabled)' crates/goblins-os-core/src/firewall.rs && rg -Fq 'FIREWALL_STATE_POLL_WINDOW: Duration = Duration::from_secs(5)' crates/goblins-os-core/src/firewall.rs && rg -Fq 'remaining_probe_time(deadline)' crates/goblins-os-core/src/firewall.rs && rg -Fq 'post_start_state_poll_has_one_real_wall_clock_deadline' crates/goblins-os-core/src/firewall.rs && rg -q 'is-active\", \"--quiet\", \"firewalld.service' crates/goblins-os-core/src/firewall.rs"
check "capture harness proves live firewall polkit toggle path" "rg -q '/proof/firewall-live-toggle' os/hardware-gate/capture-harness/in-session-orchestrator.sh && rg -q '/v1/firewall/enabled' os/hardware-gate/capture-harness/in-session-orchestrator.sh && rg -q 'disable_active=false' os/hardware-gate/capture-harness/in-session-orchestrator.sh && rg -q 'enable_active=true' os/hardware-gate/capture-harness/in-session-orchestrator.sh && rg -Fq 'enable_text=\$(proof_query_value' os/hardware-gate/capture-harness/in-session-orchestrator.sh"
check "capture driver persists live firewall proof" "rg -q 'firewall-live-toggle-proof.json' os/hardware-gate/capture-harness/drive-capture.py os/hardware-gate/capture-harness/run-capture.sh && rg -q 'require_proofs' os/hardware-gate/capture-harness/drive-capture.py && rg -q 'HONESTY GUARD: missing or failing live firewall toggle proof' os/hardware-gate/capture-harness/run-capture.sh"
check "capture driver persists Preview open/render proof" "rg -q 'preview-open-render-proof.json' os/hardware-gate/capture-harness/drive-capture.py os/hardware-gate/capture-harness/run-capture.sh && rg -q 'preview-open-render' os/hardware-gate/capture-harness/drive-capture.py && rg -q 'HONESTY GUARD: missing or failing Preview open/render proof' os/hardware-gate/capture-harness/run-capture.sh"
check "capture driver persists Focus arm roundtrip proof" "rg -q 'focus-arm-roundtrip-proof.json' os/hardware-gate/capture-harness/drive-capture.py os/hardware-gate/capture-harness/run-capture.sh && rg -q 'focus-arm-roundtrip' os/hardware-gate/capture-harness/drive-capture.py && rg -q 'HONESTY GUARD: missing or failing Focus arm roundtrip proof' os/hardware-gate/capture-harness/run-capture.sh"
check "capture driver persists App privacy revoke proof" "rg -q 'app-privacy-revoke-proof.json' os/hardware-gate/capture-harness/drive-capture.py os/hardware-gate/capture-harness/run-capture.sh && rg -q 'app-privacy-revoke' os/hardware-gate/capture-harness/drive-capture.py && rg -q 'HONESTY GUARD: missing or failing App privacy revoke proof' os/hardware-gate/capture-harness/run-capture.sh"
check "capture harness proves Text Shortcuts session plumbing without runtime claim" "rg -q '/proof/text-shortcuts-session-enable' os/hardware-gate/capture-harness/in-session-orchestrator.sh && rg -q 'TEXT_SHORTCUTS_IBUS_SERVICE=org.freedesktop.IBus.session.GNOME.service' os/hardware-gate/capture-harness/in-session-orchestrator.sh && rg -Fq 'service_unit=\$TEXT_SHORTCUTS_IBUS_SERVICE' os/hardware-gate/capture-harness/in-session-orchestrator.sh && rg -Fq 'systemctl --user restart \"\$TEXT_SHORTCUTS_IBUS_SERVICE\"' os/hardware-gate/capture-harness/in-session-orchestrator.sh && rg -q 'ibus engine goblins-textshortcuts' os/hardware-gate/capture-harness/in-session-orchestrator.sh && rg -q 'ensure_textshortcuts_ibus_component' os/hardware-gate/capture-harness/in-session-orchestrator.sh && rg -q 'wait_ibus_cli_ready' os/hardware-gate/capture-harness/in-session-orchestrator.sh && rg -q 'wait_ibus_bus_owned' os/hardware-gate/capture-harness/in-session-orchestrator.sh && rg -q 'user_component_seeded=true' os/hardware-gate/capture-harness/in-session-orchestrator.sh && rg -q 'list_error=' os/hardware-gate/capture-harness/in-session-orchestrator.sh && rg -q 'bus_owner=' os/hardware-gate/capture-harness/in-session-orchestrator.sh && rg -q 'service_diag=' os/hardware-gate/capture-harness/in-session-orchestrator.sh && rg -q 'daemon_process=' os/hardware-gate/capture-harness/in-session-orchestrator.sh && rg -q 'session_env=' os/hardware-gate/capture-harness/in-session-orchestrator.sh && ! test -f os/systemd-user/org.goblins.OS.IBus.service && rg -q 'Wants=org.freedesktop.IBus.session.GNOME.service' os/systemd-user/gnome-session@goblins-os.target.d/goblins-os.session.conf && rg -q 'Before=org.freedesktop.IBus.session.GNOME.service' os/systemd-user/org.goblins.OS.InputSourcesSeed.service && ! rg -q 'org.goblins.OS.IBus.service' os/systemd-user/gnome-session@goblins-os.target.d/goblins-os.session.conf os/systemd-user/org.goblins.OS.InputSourcesSeed.service os/hardware-gate/capture-harness/in-session-orchestrator.sh && ! rg -Fq 'application.run_with_args(&[\"goblins-os-shell\", \"--text-shortcuts-proof\"]);' crates/goblins-os-shell/src/main.rs && rg -q 'systemctl --user import-environment' os/session/goblins-os-session && rg -q 'dbus-update-activation-environment --systemd' os/session/goblins-os-session && rg -q 'WAYLAND_DISPLAY' os/session/goblins-os-session && rg -q '/var/home/goblin/.local/share/ibus/component/goblins-textshortcuts.xml' os/bootc/Containerfile && rg -q 'proof_scope=session-plumbing' os/hardware-gate/capture-harness/in-session-orchestrator.sh && rg -q 'core_ibus_available=true' os/hardware-gate/capture-harness/in-session-orchestrator.sh && rg -q 'core_component_registered=true' os/hardware-gate/capture-harness/in-session-orchestrator.sh && rg -q 'core_engine_binary_available=true' os/hardware-gate/capture-harness/in-session-orchestrator.sh && rg -q 'core_input_source_configured=true' os/hardware-gate/capture-harness/in-session-orchestrator.sh && rg -Fq 'core_engine_available=\$core_engine_available&core_runtime_loop_available=\$core_runtime_loop&runtime_ready_claim=false' os/hardware-gate/capture-harness/in-session-orchestrator.sh && ! rg -Fq 'core_engine_available=true&core_runtime_loop_available=true&runtime_ready_claim=true' os/hardware-gate/capture-harness/in-session-orchestrator.sh"
check "capture driver persists Text Shortcuts session proof" "rg -q 'text-shortcuts-session-enable-proof.json' os/hardware-gate/capture-harness/drive-capture.py os/hardware-gate/capture-harness/run-capture.sh && rg -q 'text-shortcuts-session-enable' os/hardware-gate/capture-harness/drive-capture.py && rg -q 'HONESTY GUARD: missing or failing Text Shortcuts session-enable proof' os/hardware-gate/capture-harness/run-capture.sh"
check "Text Shortcuts use one bounded private desktop-user state contract" "text_shortcuts_desktop_state_contract_is_pinned"
check "Text Shortcuts one-shot input source seed is source-gated" "test -x os/input/goblins-os-input-source-seed && rg -q 'input-source-seeded' os/input/goblins-os-input-source-seed && rg -Fq '[ \"\$value\" = \"true\" ]' os/input/goblins-os-input-source-seed && ! rg -Fq '|| true' os/input/goblins-os-input-source-seed && rg -Fq 'parsed = ast.literal_eval(raw)' os/input/goblins-os-input-source-seed && rg -Fq 'annotation = \"@a(ss) \"' os/input/goblins-os-input-source-seed && rg -Fq 'annotation = \"@as \"' os/input/goblins-os-input-source-seed && rg -Fq 'input source settings were malformed; leaving every source unchanged' os/input/goblins-os-input-source-seed && rg -Fq 'sources.append((item[0], item[1]))' os/input/goblins-os-input-source-seed && rg -Fq 'engines.append(item)' os/input/goblins-os-input-source-seed && rg -Fq 'set_and_verify_input_sources()' os/input/goblins-os-input-source-seed && rg -Fq 'current_sources=\"\$(gsettings get org.gnome.desktop.input-sources sources 2>/dev/null)\"' os/input/goblins-os-input-source-seed && rg -Fq 'current_mru=\"\$(gsettings get org.gnome.desktop.input-sources mru-sources 2>/dev/null)\"' os/input/goblins-os-input-source-seed && rg -Fq 'current_preload=\"\$(gsettings get org.freedesktop.ibus.general preload-engines 2>/dev/null)\"' os/input/goblins-os-input-source-seed && rg -Fq 'rollback_originals()' os/input/goblins-os-input-source-seed && rg -Fq 'if [ \"\$touched\" != \"1\" ]' os/input/goblins-os-input-source-seed && rg -Fq '[ \"\$canonical\" = \"\$staged\" ] || return 1' os/input/goblins-os-input-source-seed && rg -Fq 'restore_input_sources_if_unchanged sources \"\$sources_touched\"' os/input/goblins-os-input-source-seed && rg -Fq 'restore_input_sources_if_unchanged mru-sources \"\$mru_touched\"' os/input/goblins-os-input-source-seed && rg -Fq 'restore_preload_if_unchanged \"\$preload_touched\"' os/input/goblins-os-input-source-seed && rg -Fq 'set_and_verify_input_sources sources \"\$canonical_sources\" \"\$next_sources\"' os/input/goblins-os-input-source-seed && rg -Fq 'set_and_verify_input_sources mru-sources \"\$canonical_mru\" \"\$next_mru\"' os/input/goblins-os-input-source-seed && rg -Fq 'set_and_verify_preload \"\$canonical_preload\" \"\$next_preload\"' os/input/goblins-os-input-source-seed && rg -Fq \"printf 'seeded %s/%s\\\\n'\" os/input/goblins-os-input-source-seed && rg -q 'COPY --chmod=0755 os/input/goblins-os-input-source-seed /usr/libexec/goblins-os/goblins-os-input-source-seed' os/bootc/Containerfile && rg -q 'bash -n /usr/libexec/goblins-os/goblins-os-input-source-seed' os/bootc/Containerfile && rg -q 'Wants=org.goblins.OS.InputSourcesSeed.service' os/systemd-user/gnome-session@goblins-os.target.d/goblins-os.session.conf && rg -q 'Wants=org.freedesktop.IBus.session.GNOME.service' os/systemd-user/gnome-session@goblins-os.target.d/goblins-os.session.conf && rg -q 'Before=org.freedesktop.IBus.session.GNOME.service' os/systemd-user/org.goblins.OS.InputSourcesSeed.service"
check "capture harness retired superseded Text Shortcuts live keystroke proof" "! rg -q '/proof/text-shortcuts-live-keystroke|text_shortcuts_live_keystroke_proof|proof_text_shortcuts_live[[:space:]]*\\(\\)' os/hardware-gate/capture-harness/in-session-orchestrator.sh && ! rg -q 'text-shortcuts-live-keystroke|TEXT_SHORTCUTS_LIVE_PROOF|HONESTY GUARD: missing or failing Text Shortcuts live keystroke proof' os/hardware-gate/capture-harness/drive-capture.py os/hardware-gate/capture-harness/run-capture.sh"
check "Text Shortcuts live keystrokes are covered by secure storage, native IBus, and focused readback proof" "rg -q '/proof/text-shortcuts-live-ibus-runtime-render' os/hardware-gate/capture-harness/in-session-orchestrator.sh && rg -q 'normal_actual=on%20my%20way\\.' os/hardware-gate/capture-harness/in-session-orchestrator.sh && rg -q 'passthrough_actual=hello\\.' os/hardware-gate/capture-harness/in-session-orchestrator.sh && rg -q 'desktop_parent_contract=true' os/hardware-gate/capture-harness/in-session-orchestrator.sh && rg -q 'live_watcher_reload=true' os/hardware-gate/capture-harness/in-session-orchestrator.sh && rg -q 'post_keystroke_roundtrip=true' os/hardware-gate/capture-harness/in-session-orchestrator.sh && rg -q 'password_sensitive_purpose=true' os/hardware-gate/capture-harness/in-session-orchestrator.sh && rg -q 'password_popup_absent=true' os/hardware-gate/capture-harness/in-session-orchestrator.sh && rg -q 'ibus_commit_operation=true' os/hardware-gate/capture-harness/in-session-orchestrator.sh && rg -q 'focused_entry_readback=true' os/hardware-gate/capture-harness/in-session-orchestrator.sh && rg -q 'ibus_commit_delivered=true' os/hardware-gate/capture-harness/in-session-orchestrator.sh && rg -q 'native_ibus_candidate_published=true' os/hardware-gate/capture-harness/in-session-orchestrator.sh && rg -q 'renderer=native-ibus-lookup-table' os/hardware-gate/capture-harness/in-session-orchestrator.sh && rg -q 'synthetic_overlay=false' os/hardware-gate/capture-harness/in-session-orchestrator.sh && rg -q '32-text-shortcuts-live-ibus-runtime-render.png' os/hardware-gate/capture-harness/in-session-orchestrator.sh os/hardware-gate/capture-harness/run-capture.sh"
check "capture harness proves Text Shortcuts candidate metadata without live overlay claim" "rg -q '/proof/text-shortcuts-candidate-metadata' os/hardware-gate/capture-harness/in-session-orchestrator.sh && rg -q 'goblins-os-shell\" --text-shortcuts-proof candidate' os/hardware-gate/capture-harness/in-session-orchestrator.sh && rg -q 'replacement=on my way' os/hardware-gate/capture-harness/in-session-orchestrator.sh && rg -q 'accept_on=word-boundary' os/hardware-gate/capture-harness/in-session-orchestrator.sh && rg -q 'dismiss_key=Escape' os/hardware-gate/capture-harness/in-session-orchestrator.sh && rg -q 'rendered_bubble_ready_claim=false' os/hardware-gate/capture-harness/in-session-orchestrator.sh && rg -q 'live_overlay_claim=false' os/hardware-gate/capture-harness/in-session-orchestrator.sh && rg -q 'runtime_ready_claim=false' os/hardware-gate/capture-harness/in-session-orchestrator.sh"
check "capture driver persists Text Shortcuts candidate metadata proof" "rg -q 'text-shortcuts-candidate-metadata-proof.json' os/hardware-gate/capture-harness/drive-capture.py os/hardware-gate/capture-harness/run-capture.sh && rg -q 'text-shortcuts-candidate-metadata' os/hardware-gate/capture-harness/drive-capture.py && rg -q 'HONESTY GUARD: missing or failing Text Shortcuts candidate metadata proof' os/hardware-gate/capture-harness/run-capture.sh"
check "capture harness proves Text Shortcuts overlay intent without live overlay claim" "rg -q '/proof/text-shortcuts-overlay-intent' os/hardware-gate/capture-harness/in-session-orchestrator.sh && rg -q -- '--overlay-intent-self-test' os/hardware-gate/capture-harness/in-session-orchestrator.sh && rg -q 'goblins-textshortcuts-ibus-adapter-overlay-intent' os/hardware-gate/capture-harness/in-session-orchestrator.sh && rg -q 'show_count=2' os/hardware-gate/capture-harness/in-session-orchestrator.sh && rg -q 'hide_count=2' os/hardware-gate/capture-harness/in-session-orchestrator.sh && rg -q 'dismissed_reason=true' os/hardware-gate/capture-harness/in-session-orchestrator.sh && rg -q 'committed_reason=true' os/hardware-gate/capture-harness/in-session-orchestrator.sh && rg -q 'live_overlay_claim=false' os/hardware-gate/capture-harness/in-session-orchestrator.sh && rg -q 'runtime_ready_claim=false' os/hardware-gate/capture-harness/in-session-orchestrator.sh"
check "capture driver persists Text Shortcuts overlay intent proof" "rg -q 'text-shortcuts-overlay-intent-proof.json' os/hardware-gate/capture-harness/drive-capture.py os/hardware-gate/capture-harness/run-capture.sh && rg -q 'text-shortcuts-overlay-intent' os/hardware-gate/capture-harness/drive-capture.py && rg -q 'HONESTY GUARD: missing or failing Text Shortcuts overlay-intent proof' os/hardware-gate/capture-harness/run-capture.sh"
check "capture harness proves Text Shortcuts candidate bubble frame without live render claim" "rg -q '/proof/text-shortcuts-candidate-bubble-frame' os/hardware-gate/capture-harness/in-session-orchestrator.sh && rg -q -- '--candidate-bubble-frame-self-test' os/hardware-gate/capture-harness/in-session-orchestrator.sh && rg -q 'goblins-textshortcuts-accept-bubble-frame' os/hardware-gate/capture-harness/in-session-orchestrator.sh && rg -q 'show_frame_count=2' os/hardware-gate/capture-harness/in-session-orchestrator.sh && rg -q 'hide_frame_count=2' os/hardware-gate/capture-harness/in-session-orchestrator.sh && rg -q 'style_class=gos-text-shortcuts-candidate' os/hardware-gate/capture-harness/in-session-orchestrator.sh && rg -q 'font_family=Inter' os/hardware-gate/capture-harness/in-session-orchestrator.sh && rg -q 'sensitive_field_refusal=true' os/hardware-gate/capture-harness/in-session-orchestrator.sh && rg -q 'rendered_bubble_ready_claim=false' os/hardware-gate/capture-harness/in-session-orchestrator.sh && rg -q 'live_overlay_claim=false' os/hardware-gate/capture-harness/in-session-orchestrator.sh && rg -q 'runtime_ready_claim=false' os/hardware-gate/capture-harness/in-session-orchestrator.sh"
check "capture driver persists Text Shortcuts candidate bubble frame proof" "rg -q 'text-shortcuts-candidate-bubble-frame-proof.json' os/hardware-gate/capture-harness/drive-capture.py os/hardware-gate/capture-harness/run-capture.sh && rg -q 'text-shortcuts-candidate-bubble-frame' os/hardware-gate/capture-harness/drive-capture.py && rg -q 'HONESTY GUARD: missing or failing Text Shortcuts candidate-bubble-frame proof' os/hardware-gate/capture-harness/run-capture.sh"
check "capture harness proves Text Shortcuts candidate bubble layout without live render claim" "rg -q '/proof/text-shortcuts-candidate-bubble-layout' os/hardware-gate/capture-harness/in-session-orchestrator.sh && rg -q -- '--candidate-bubble-layout-self-test' os/hardware-gate/capture-harness/in-session-orchestrator.sh && rg -q 'goblins-textshortcuts-accept-bubble-layout' os/hardware-gate/capture-harness/in-session-orchestrator.sh && rg -q 'frame_surface=goblins-textshortcuts-accept-bubble-frame' os/hardware-gate/capture-harness/in-session-orchestrator.sh && rg -q 'layout_count=4' os/hardware-gate/capture-harness/in-session-orchestrator.sh && rg -q 'visible_layout_count=3' os/hardware-gate/capture-harness/in-session-orchestrator.sh && rg -q 'right_edge_clamped=true' os/hardware-gate/capture-harness/in-session-orchestrator.sh && rg -q 'bottom_edge_flipped=true' os/hardware-gate/capture-harness/in-session-orchestrator.sh && rg -q 'hidden_frame_collapses=true' os/hardware-gate/capture-harness/in-session-orchestrator.sh && rg -q 'style_class=gos-text-shortcuts-candidate' os/hardware-gate/capture-harness/in-session-orchestrator.sh && rg -q 'font_family=Inter' os/hardware-gate/capture-harness/in-session-orchestrator.sh && rg -q 'rendered_bubble_ready_claim=false' os/hardware-gate/capture-harness/in-session-orchestrator.sh && rg -q 'live_overlay_claim=false' os/hardware-gate/capture-harness/in-session-orchestrator.sh && rg -q 'runtime_ready_claim=false' os/hardware-gate/capture-harness/in-session-orchestrator.sh"
check "capture driver persists Text Shortcuts candidate bubble layout proof" "rg -q 'text-shortcuts-candidate-bubble-layout-proof.json' os/hardware-gate/capture-harness/drive-capture.py os/hardware-gate/capture-harness/run-capture.sh && rg -q 'text-shortcuts-candidate-bubble-layout' os/hardware-gate/capture-harness/drive-capture.py && rg -q 'HONESTY GUARD: missing or failing Text Shortcuts candidate-bubble-layout proof' os/hardware-gate/capture-harness/run-capture.sh"
check "capture harness proves Text Shortcuts candidate bubble render intent without live render claim" "rg -q '/proof/text-shortcuts-candidate-bubble-render-intent' os/hardware-gate/capture-harness/in-session-orchestrator.sh && rg -q -- '--candidate-bubble-render-intent-self-test' os/hardware-gate/capture-harness/in-session-orchestrator.sh && rg -q 'goblins-textshortcuts-accept-bubble-render-intent' os/hardware-gate/capture-harness/in-session-orchestrator.sh && rg -q 'frame_surface=goblins-textshortcuts-accept-bubble-frame' os/hardware-gate/capture-harness/in-session-orchestrator.sh && rg -q 'layout_surface=goblins-textshortcuts-accept-bubble-layout' os/hardware-gate/capture-harness/in-session-orchestrator.sh && rg -q 'render_intent_count=8' os/hardware-gate/capture-harness/in-session-orchestrator.sh && rg -q 'show_intent_count=4' os/hardware-gate/capture-harness/in-session-orchestrator.sh && rg -q 'hide_intent_count=4' os/hardware-gate/capture-harness/in-session-orchestrator.sh && rg -q 'focus_out_hide=true' os/hardware-gate/capture-harness/in-session-orchestrator.sh && rg -q 'sensitive_hide=true' os/hardware-gate/capture-harness/in-session-orchestrator.sh && rg -q 'pass_through_unchanged=true' os/hardware-gate/capture-harness/in-session-orchestrator.sh && rg -q 'key_release_preserved_candidate=true' os/hardware-gate/capture-harness/in-session-orchestrator.sh && rg -q 'runtime_failure_cleanup=true' os/hardware-gate/capture-harness/in-session-orchestrator.sh && rg -q 'sink_failure_fail_open=true' os/hardware-gate/capture-harness/in-session-orchestrator.sh && rg -q 'style_class=gos-text-shortcuts-candidate' os/hardware-gate/capture-harness/in-session-orchestrator.sh && rg -q 'font_family=Inter' os/hardware-gate/capture-harness/in-session-orchestrator.sh && rg -q 'rendered_bubble_ready_claim=false' os/hardware-gate/capture-harness/in-session-orchestrator.sh && rg -q 'live_overlay_claim=false' os/hardware-gate/capture-harness/in-session-orchestrator.sh && rg -q 'runtime_ready_claim=false' os/hardware-gate/capture-harness/in-session-orchestrator.sh"
check "capture driver persists Text Shortcuts candidate bubble render intent proof" "rg -q 'text-shortcuts-candidate-bubble-render-intent-proof.json' os/hardware-gate/capture-harness/drive-capture.py os/hardware-gate/capture-harness/run-capture.sh && rg -q 'text-shortcuts-candidate-bubble-render-intent' os/hardware-gate/capture-harness/drive-capture.py && rg -q 'HONESTY GUARD: missing or failing Text Shortcuts candidate-bubble-render-intent proof' os/hardware-gate/capture-harness/run-capture.sh"
check "capture harness proves Text Shortcuts candidate bubble rendered screenshot without live claim" "rg -q '/proof/text-shortcuts-candidate-bubble-render' os/hardware-gate/capture-harness/in-session-orchestrator.sh && rg -q -- '--text-shortcuts-proof candidate-render' os/hardware-gate/capture-harness/in-session-orchestrator.sh && rg -q '31-text-shortcuts-candidate-bubble-render' os/hardware-gate/capture-harness/in-session-orchestrator.sh os/hardware-gate/capture-harness/run-capture.sh os/hardware-gate/close-signoff.sh os/hardware-gate/verify-shipping-status.sh && rg -q 'render_intent_surface=goblins-textshortcuts-accept-bubble-render-intent' os/hardware-gate/capture-harness/in-session-orchestrator.sh && rg -q 'rendered_candidate_surface=true' os/hardware-gate/capture-harness/in-session-orchestrator.sh os/hardware-gate/capture-harness/run-capture.sh && rg -q 'style_class=gos-text-shortcuts-candidate' os/hardware-gate/capture-harness/in-session-orchestrator.sh os/hardware-gate/capture-harness/run-capture.sh && rg -q 'font_family=Inter' os/hardware-gate/capture-harness/in-session-orchestrator.sh os/hardware-gate/capture-harness/run-capture.sh && rg -q 'rendered_bubble_ready_claim=false' os/hardware-gate/capture-harness/in-session-orchestrator.sh os/hardware-gate/capture-harness/run-capture.sh && rg -q 'live_overlay_claim=false' os/hardware-gate/capture-harness/in-session-orchestrator.sh os/hardware-gate/capture-harness/run-capture.sh && rg -q 'runtime_ready_claim=false' os/hardware-gate/capture-harness/in-session-orchestrator.sh os/hardware-gate/capture-harness/run-capture.sh"
check "capture driver persists Text Shortcuts candidate bubble rendered screenshot proof" "rg -q 'text-shortcuts-candidate-bubble-render-proof.json' os/hardware-gate/capture-harness/drive-capture.py os/hardware-gate/capture-harness/run-capture.sh && rg -q 'text-shortcuts-candidate-bubble-render' os/hardware-gate/capture-harness/drive-capture.py && rg -q 'HONESTY GUARD: missing or failing Text Shortcuts candidate-bubble-render screenshot proof' os/hardware-gate/capture-harness/run-capture.sh"
check "capture harness drives the native Text Shortcuts IBus runtime/render proof" "rg -q '/proof/text-shortcuts-live-ibus-runtime-render' os/hardware-gate/capture-harness/in-session-orchestrator.sh && rg -q -- '--text-shortcuts-proof normal' os/hardware-gate/capture-harness/in-session-orchestrator.sh && ! rg -q -- '--text-shortcuts-proof live-runtime-render' os/hardware-gate/capture-harness/in-session-orchestrator.sh && rg -q 'GOBLINS_TEXTSHORTCUTS_PROOF_EVENTS' os/hardware-gate/capture-harness/in-session-orchestrator.sh && rg -q 'systemctl --user set-environment GOBLINS_TEXTSHORTCUTS_PROOF_EVENTS' os/hardware-gate/capture-harness/in-session-orchestrator.sh && rg -q 'host_focus_text_shortcuts_field runtime-render-focus' os/hardware-gate/capture-harness/in-session-orchestrator.sh && rg -Fq '| last) as \$latest_popup' os/hardware-gate/capture-harness/in-session-orchestrator.sh && ! rg -Fq 'max_by(.generation // -1)' os/hardware-gate/capture-harness/in-session-orchestrator.sh && rg -q 'stage=native-popup-settle' os/hardware-gate/capture-harness/in-session-orchestrator.sh && rg -q 'normal_ledger_file=/tmp/gate-text-shortcuts-normal-stage-events.jsonl' os/hardware-gate/capture-harness/in-session-orchestrator.sh && rg -q 'pre_boundary_ledger_file=/tmp/gate-text-shortcuts-pre-boundary-events.jsonl' os/hardware-gate/capture-harness/in-session-orchestrator.sh && rg -q 'boundary_ledger_file=/tmp/gate-text-shortcuts-boundary-stage-events.jsonl' os/hardware-gate/capture-harness/in-session-orchestrator.sh && rg -Fq 'wait_capture_ack \"\$1\"' os/hardware-gate/capture-harness/in-session-orchestrator.sh && rg -q 'sig 32-text-shortcuts-live-ibus-runtime-render' os/hardware-gate/capture-harness/in-session-orchestrator.sh && rg -q 'captured_generation' os/hardware-gate/capture-harness/in-session-orchestrator.sh && rg -q 'captured_ordinal' os/hardware-gate/capture-harness/in-session-orchestrator.sh && rg -q 'native_popup_record_ordinal' os/hardware-gate/capture-harness/in-session-orchestrator.sh && rg -q 'boundary_ledger_start=' os/hardware-gate/capture-harness/in-session-orchestrator.sh && rg -q 'pre_boundary_commit_absent=true' os/hardware-gate/capture-harness/in-session-orchestrator.sh && rg -q 'boundary_stage_ledger_scoped=true' os/hardware-gate/capture-harness/in-session-orchestrator.sh && rg -q 'boundary_stage_commit_count=1' os/hardware-gate/capture-harness/in-session-orchestrator.sh && rg -q 'boundary_popup_action=hide-candidate' os/hardware-gate/capture-harness/in-session-orchestrator.sh && rg -q 'boundary_popup_reason=committed' os/hardware-gate/capture-harness/in-session-orchestrator.sh && rg -q 'stage=boundary-ledger' os/hardware-gate/capture-harness/in-session-orchestrator.sh && rg -Fq '.callback == \"process-key-event\"' os/hardware-gate/capture-harness/in-session-orchestrator.sh && rg -Fq '(\$commit_operations | length) == 1' os/hardware-gate/capture-harness/in-session-orchestrator.sh && rg -q 'os.replace[(]temporary_ack, ack[)]' os/hardware-gate/capture-harness/drive-capture.py && rg -q 'goblins-textshortcuts-live-ibus-runtime-render' os/hardware-gate/capture-harness/in-session-orchestrator.sh os/hardware-gate/capture-harness/run-capture.sh && rg -q '32-text-shortcuts-live-ibus-runtime-render.png' os/hardware-gate/capture-harness/in-session-orchestrator.sh os/hardware-gate/capture-harness/run-capture.sh os/hardware-gate/close-signoff.sh os/hardware-gate/verify-shipping-status.sh && rg -q 'focused_entry_readback' os/hardware-gate/capture-harness/in-session-orchestrator.sh os/hardware-gate/capture-harness/run-capture.sh && rg -q 'ibus_commit_delivered' os/hardware-gate/capture-harness/in-session-orchestrator.sh os/hardware-gate/capture-harness/run-capture.sh && rg -q 'native_ibus_candidate_published' os/hardware-gate/capture-harness/in-session-orchestrator.sh os/hardware-gate/capture-harness/run-capture.sh && rg -q 'renderer=native-ibus-lookup-table' os/hardware-gate/capture-harness/in-session-orchestrator.sh && rg -q 'synthetic_overlay=false' os/hardware-gate/capture-harness/in-session-orchestrator.sh && rg -q 'screenshot_capture_ack=true' os/hardware-gate/capture-harness/in-session-orchestrator.sh && rg -q 'native_candidate_popup_ready_claim=true' os/hardware-gate/capture-harness/in-session-orchestrator.sh && rg -q 'live_overlay_claim=true' os/hardware-gate/capture-harness/in-session-orchestrator.sh && rg -q 'runtime_ready_claim=true' os/hardware-gate/capture-harness/in-session-orchestrator.sh && rg -q 'core_readiness_flip=live' os/hardware-gate/capture-harness/in-session-orchestrator.sh && rg -q '\"core_readiness_flip\": \"live\"' os/hardware-gate/capture-harness/run-capture.sh && ! rg -q 'text_input_v3_commit|rendered_accept_bubble|rendered_bubble_ready_claim=true|live-ibus-runtime-render-not-implemented' os/hardware-gate/capture-harness/in-session-orchestrator.sh os/hardware-gate/capture-harness/run-capture.sh"
check "capture driver persists Text Shortcuts live IBus runtime/render proof" "rg -q 'text-shortcuts-live-ibus-runtime-render-proof.json' os/hardware-gate/capture-harness/drive-capture.py os/hardware-gate/capture-harness/run-capture.sh && rg -q 'text-shortcuts-live-ibus-runtime-render' os/hardware-gate/capture-harness/drive-capture.py && rg -q 'HONESTY GUARD: missing or failing Text Shortcuts live IBus runtime/render proof' os/hardware-gate/capture-harness/run-capture.sh"
check "capture harness turns Switch Control off before ordinary screenshots" "rg -Fq 'switch_control_off(){' os/hardware-gate/capture-harness/in-session-orchestrator.sh && rg -Fq 'gsettings set org.goblins.os.a11y.switch-control enabled false' os/hardware-gate/capture-harness/in-session-orchestrator.sh && rg -Fq 'goblinsSwitchControl.hide' os/hardware-gate/capture-harness/in-session-orchestrator.sh && rg -Fq 'this._stopScanner();' os/gnome-shell-extensions/goblins-switch@goblins.os/extension.js && rg -Fq 'switch_control_off' os/hardware-gate/capture-harness/in-session-orchestrator.sh"
check "Text Shortcuts accept-bubble frame contract is source-gated" "rg -q -- '--candidate-bubble-frame-self-test' os/goblins-os-textshortcuts/goblins-textshortcuts-ibus os/bootc/Containerfile && rg -q 'goblins-textshortcuts-accept-bubble-frame' os/goblins-os-textshortcuts/goblins-textshortcuts-ibus os/bootc/Containerfile && rg -q 'goblins-textshortcuts-candidate-bubble-frame.json' os/bootc/Containerfile && rg -q 'show_frame_count' os/goblins-os-textshortcuts/goblins-textshortcuts-ibus os/bootc/Containerfile && rg -q 'hide_frame_count' os/goblins-os-textshortcuts/goblins-textshortcuts-ibus os/bootc/Containerfile && rg -q 'dismissed_frame' os/goblins-os-textshortcuts/goblins-textshortcuts-ibus os/bootc/Containerfile && rg -q 'committed_frame' os/goblins-os-textshortcuts/goblins-textshortcuts-ibus os/bootc/Containerfile && rg -q 'sensitive_field_refusal' os/goblins-os-textshortcuts/goblins-textshortcuts-ibus os/bootc/Containerfile && rg -q 'gos-text-shortcuts-candidate' os/goblins-os-textshortcuts/goblins-textshortcuts-ibus os/bootc/Containerfile && rg -q 'rendered_bubble_ready_claim' os/goblins-os-textshortcuts/goblins-textshortcuts-ibus os/bootc/Containerfile && rg -q 'live_overlay_claim' os/goblins-os-textshortcuts/goblins-textshortcuts-ibus os/bootc/Containerfile && rg -q 'runtime_ready_claim' os/goblins-os-textshortcuts/goblins-textshortcuts-ibus os/bootc/Containerfile"
check "Text Shortcuts accept-bubble layout contract is source-gated" "rg -q -- '--candidate-bubble-layout-self-test' os/goblins-os-textshortcuts/goblins-textshortcuts-ibus os/bootc/Containerfile && rg -q 'goblins-textshortcuts-accept-bubble-layout' os/goblins-os-textshortcuts/goblins-textshortcuts-ibus os/bootc/Containerfile && rg -q 'goblins-textshortcuts-candidate-bubble-layout.json' os/bootc/Containerfile && rg -q 'layout_count' os/goblins-os-textshortcuts/goblins-textshortcuts-ibus os/bootc/Containerfile && rg -q 'visible_layout_count' os/goblins-os-textshortcuts/goblins-textshortcuts-ibus os/bootc/Containerfile && rg -q 'right_edge_clamped' os/goblins-os-textshortcuts/goblins-textshortcuts-ibus os/bootc/Containerfile && rg -q 'bottom_edge_flipped' os/goblins-os-textshortcuts/goblins-textshortcuts-ibus os/bootc/Containerfile && rg -q 'hidden_frame_collapses' os/goblins-os-textshortcuts/goblins-textshortcuts-ibus os/bootc/Containerfile && rg -q 'gos-text-shortcuts-candidate' os/goblins-os-textshortcuts/goblins-textshortcuts-ibus os/bootc/Containerfile && rg -q 'font_family' os/goblins-os-textshortcuts/goblins-textshortcuts-ibus os/bootc/Containerfile && rg -q 'rendered_bubble_ready_claim' os/goblins-os-textshortcuts/goblins-textshortcuts-ibus os/bootc/Containerfile && rg -q 'live_overlay_claim' os/goblins-os-textshortcuts/goblins-textshortcuts-ibus os/bootc/Containerfile && rg -q 'runtime_ready_claim' os/goblins-os-textshortcuts/goblins-textshortcuts-ibus os/bootc/Containerfile"
check "Text Shortcuts native IBus render-intent bridge is source-gated" "rg -q -- '--candidate-bubble-render-intent-self-test' os/goblins-os-textshortcuts/goblins-textshortcuts-ibus os/bootc/Containerfile && rg -q -- '--native-ibus-lookup-renderer-self-test' os/goblins-os-textshortcuts/goblins-textshortcuts-ibus os/bootc/Containerfile && rg -q 'NativeIbusLookupRenderer' os/goblins-os-textshortcuts/goblins-textshortcuts-ibus && rg -Fq 'self._ibus.LookupTable.new' os/goblins-os-textshortcuts/goblins-textshortcuts-ibus && rg -q 'update_lookup_table' os/goblins-os-textshortcuts/goblins-textshortcuts-ibus && rg -q 'do_candidate_clicked' os/goblins-os-textshortcuts/goblins-textshortcuts-ibus && rg -q 'accept-candidate' os/goblins-os-textshortcuts/goblins-textshortcuts-ibus crates/goblins-os-textshortcuts-engine/src/lib.rs && rg -q 'key_release_preserved_candidate' os/goblins-os-textshortcuts/goblins-textshortcuts-ibus os/bootc/Containerfile && rg -q 'runtime_failure_cleanup' os/goblins-os-textshortcuts/goblins-textshortcuts-ibus os/bootc/Containerfile && rg -q 'synthetic_overlay' os/goblins-os-textshortcuts/goblins-textshortcuts-ibus os/bootc/Containerfile"
check "capture harness prints qemu diagnostics on startup failure" "rg -q 'QEMU startup diagnostics' os/hardware-gate/capture-harness/run-capture.sh && rg -q 'qemu.log' os/hardware-gate/capture-harness/run-capture.sh && rg -q 'serial.log' os/hardware-gate/capture-harness/run-capture.sh && rg -q 'last connection error' os/hardware-gate/capture-harness/drive-capture.py"
check "capture harness refuses human-safe release ISO for automated proof" "rg -q 'require_verification_iso' os/hardware-gate/capture-harness/run-capture.sh && rg -q 'GOBLINS_OS_CAPTURE_ISO' os/hardware-gate/capture-harness/run-capture.sh && rg -q 'GOBLINS_OS_ISO_CONFIG=os/iso/verify-config.toml' os/hardware-gate/capture-harness/run-capture.sh && rg -q 'public release ISO is intentionally human-safe' os/hardware-gate/capture-harness/run-capture.sh && rg -q 'GOBLINS_VERIFY_INSTALL_DONE' os/hardware-gate/capture-harness/run-capture.sh && rg -q 'goblins-hwgate-session-orchestrator' os/hardware-gate/capture-harness/run-capture.sh && rg -q 'CAPTURE_STARTED' os/hardware-gate/capture-harness/run-capture.sh"
check "capture driver fail-closes on serial VM stages and diagnostic frames" "rg -q 'GOS_SERIALLOG' os/hardware-gate/capture-harness/run-capture.sh os/hardware-gate/capture-harness/drive-capture.py && rg -Fq 'wait_serial_contains(\"ISO boot menu\"' os/hardware-gate/capture-harness/drive-capture.py && rg -Fq 'observe_serial_contains(\"ISO boot handoff\"' os/hardware-gate/capture-harness/drive-capture.py && rg -q 'continuing to framebuffer stages' os/hardware-gate/capture-harness/drive-capture.py && rg -Fq 'key(\"ret\")' os/hardware-gate/capture-harness/drive-capture.py && rg -q 'Anaconda automated kickstart progress' os/hardware-gate/capture-harness/drive-capture.py && rg -Fq '\"kickstart install post\"' os/hardware-gate/capture-harness/drive-capture.py && rg -q 'GOBLINS_VERIFY_INSTALL_DONE' os/hardware-gate/capture-harness/drive-capture.py os/iso/verify-config.toml && rg -Fq 'wait_stage(\"first boot desktop\"' os/hardware-gate/capture-harness/drive-capture.py && rg -q 'diagnostic framebuffer samples' os/hardware-gate/capture-harness/drive-capture.py && rg -q '_debug-' os/hardware-gate/capture-harness/drive-capture.py && ! rg -q 'Anaconda destination disk selected' os/hardware-gate/capture-harness/drive-capture.py && ! rg -q 'click(0.937, 0.895)' os/hardware-gate/capture-harness/drive-capture.py && ! rg -q 'require_frame\\(' os/hardware-gate/capture-harness/drive-capture.py && ! rg -q 'wait_frame\\(' os/hardware-gate/capture-harness/drive-capture.py"
check "capture harness retries transient install boot hangs with fresh VM state" "rg -Fq 'GOS_INSTALL_POST_TIMEOUT' os/hardware-gate/capture-harness/drive-capture.py && rg -Fq 'exit_code=INSTALL_POST_TIMEOUT_EXIT' os/hardware-gate/capture-harness/drive-capture.py && rg -Fq 'GOS_CAPTURE_MAX_ATTEMPTS' os/hardware-gate/capture-harness/run-capture.sh && rg -Fq 'prepare_vm_state \"\$attempt\"' os/hardware-gate/capture-harness/run-capture.sh && rg -Fq 'copy_capture_logs \"attempt-\$attempt\"' os/hardware-gate/capture-harness/run-capture.sh && rg -Fq 'stalled before kickstart marker; retrying with fresh VM state' os/hardware-gate/capture-harness/run-capture.sh && rg -Fq 'driver_rc\" -eq \"\$INSTALL_TIMEOUT_RC' os/hardware-gate/capture-harness/run-capture.sh"
check "capture harness uses release-sized sparse scratch disk" "rg -q 'GOBLINS_OS_CAPTURE_DISK_SIZE:-80G' os/hardware-gate/capture-harness/run-capture.sh && rg -q 'SCRATCH_DISK_SIZE' os/hardware-gate/capture-harness/run-capture.sh && rg -q '80G sparse scratch disk' os/hardware-gate/runbook.md"
check "capture harness restarts aarch64 disk-only after install marker" "rg -q 'aarch64:install' os/hardware-gate/capture-harness/run-capture.sh && rg -q 'usb-storage,drive=install_iso,bootindex=1' os/hardware-gate/capture-harness/run-capture.sh && rg -q -- '-no-reboot' os/hardware-gate/capture-harness/run-capture.sh && rg -q 'aarch64:firstboot' os/hardware-gate/capture-harness/run-capture.sh && rg -q 'GOS_EXIT_AFTER_INSTALL_MARKER' os/hardware-gate/capture-harness/run-capture.sh os/hardware-gate/capture-harness/drive-capture.py && rg -q 'GOS_SKIP_INSTALL_PHASE' os/hardware-gate/capture-harness/run-capture.sh os/hardware-gate/capture-harness/drive-capture.py && rg -q 'install ISO is presented as USB' os/hardware-gate/runbook.md && rg -q 'scratch disk remains virtio vda' os/hardware-gate/runbook.md"
check "capture driver completes first boot through the root release-proof capability" "rg -q 'first boot setup: completing offline path through the root release-proof capability' os/hardware-gate/capture-harness/drive-capture.py && rg -q 'post first boot release-proof unlock' os/hardware-gate/capture-harness/drive-capture.py && rg -q 'firstboot-unlock.sh' os/hardware-gate/capture-harness/run-capture.sh && rg -q 'core-proof-operation.sh' os/hardware-gate/capture-harness/run-capture.sh && rg -Fq '/run/goblins-os-core/release-proof/control.sock' os/hardware-gate/capture-harness/firstboot-unlock.sh && rg -Fq 'curl --unix-socket' os/hardware-gate/capture-harness/firstboot-unlock.sh && rg -q '/v1/privacy' os/hardware-gate/capture-harness/firstboot-unlock.sh && rg -q '/v1/installer/complete' os/hardware-gate/capture-harness/firstboot-unlock.sh && rg -q '/v1/session/unlock' os/hardware-gate/capture-harness/firstboot-unlock.sh && rg -q '/ready/FIRSTBOOT_UNLOCK' os/hardware-gate/capture-harness/firstboot-unlock.sh && rg -Fq '/failed/FIRSTBOOT_UNLOCK?stage=\$CURRENT_STAGE&rc=\$rc' os/hardware-gate/capture-harness/firstboot-unlock.sh && rg -Fq 'event.get(\"kind\") == \"failed\"' os/hardware-gate/capture-harness/drive-capture.py && rg -q 'GOBLINS_HWGATE_FIRSTBOOT_STAGE' os/hardware-gate/capture-harness/firstboot-unlock.sh && rg -q 'GOBLINS_HWGATE_CORE_UNIT_STATE' os/hardware-gate/capture-harness/firstboot-unlock.sh && ! rg -q 'journalctl' os/hardware-gate/capture-harness/firstboot-unlock.sh && rg -q 'first boot release-proof unlock callback' os/hardware-gate/capture-harness/drive-capture.py && rg -Fq 'verification first boot -> privacy=\$firstboot_privacy_code installer=\$firstboot_installer_code session=\$firstboot_session_code' os/bootc/run-selftest.sh && rg -q 'persisted_installer_mode' os/bootc/run-selftest.sh && rg -q 'persisted_session_mode' os/bootc/run-selftest.sh"
check "first boot signal delivery is authenticated, current-attempt, two-way, and fail-closed" "rg -Fq 'hmac.compare_digest(authorization, expected)' os/hardware-gate/capture-harness/drive-capture.py && rg -Fq 'self.expected_sequence' os/hardware-gate/capture-harness/drive-capture.py && rg -Fq 'event.get(\"kind\") == \"failed\"' os/hardware-gate/capture-harness/drive-capture.py && rg -Fq 'success_event_seen = event.get(\"values\") == {\"status\": \"pass\"}' os/hardware-gate/capture-harness/drive-capture.py && rg -Fq 'success_serial_marker = \"GOBLINS_HWGATE_FIRSTBOOT_UNLOCK_DONE\"' os/hardware-gate/capture-harness/drive-capture.py && rg -Fq 'firstboot_serial_start_pos = safe_file_size(SERIALLOG, SERIAL_MAX_BYTES)' os/hardware-gate/capture-harness/drive-capture.py && rg -Fq 'start_offset=serial_start_pos' os/hardware-gate/capture-harness/drive-capture.py && rg -Fq 'def _capture_channel_self_test():' os/hardware-gate/capture-harness/drive-capture.py && rg -Fq -- '-fw_cfg \"name=opt/goblins/capture-token,file=\$TOKEN_FILE\"' os/hardware-gate/capture-harness/run-capture.sh && ! rg -Fq 'http.server' os/hardware-gate/capture-harness/run-capture.sh"
check "hardware proof exposes only finite root operations to the desktop session" "rg -Fq 'subject.user !== \"goblin\"' os/iso/verify-config.toml && rg -Fq 'unit.match(/^goblins-hwgate-core-proof@(' os/iso/verify-config.toml && rg -Fq 'goblins-hwgate-core-proof@' os/hardware-gate/capture-harness/in-session-orchestrator.sh && rg -Fq 'unsupported proof operation' os/hardware-gate/capture-harness/core-proof-operation.sh && rg -Fq 'OPERATION=\"\${1:-}\"' os/hardware-gate/capture-harness/core-proof-operation.sh && rg -Fq 'download_with_wait core-proof-operation.sh /run/goblins-hwgate-root/core-proof-operation 15' os/iso/verify-config.toml && ! rg -q '(GOBLINS_OS|OPENAI_OS)_CORE_URL|127[.]0[.]0[.]1:8788|localhost:8788' os/iso/verify-config.toml os/hardware-gate/capture-harness"
check "hardware gate session automation uses verification-only service not Alt+F2 injection" "rg -q 'goblins-hwgate-session-orchestrator.service' os/iso/verify-config.toml && rg -q '99-goblins-hwgate-session-orchestrator.conf' os/iso/verify-config.toml && rg -q 'WantedBy=default.target' os/iso/verify-config.toml && rg -q 'systemctl --global enable goblins-hwgate-session-orchestrator.service' os/iso/verify-config.toml && rg -q '/etc/xdg/autostart/goblins-hwgate-session-orchestrator.desktop' os/iso/verify-config.toml && rg -q 'Exec=/etc/goblins-os/hardware-gate/goblins-hwgate-session-orchestrator' os/iso/verify-config.toml && rg -q '/etc/goblins-os/hardware-gate/goblins-hwgate-start-session-orchestrator' os/iso/verify-config.toml && rg -q 'GOBLINS_HWGATE_ETC_HELPERS_INSTALLED' os/iso/verify-config.toml && ! rg -q '/usr/libexec/goblins-hwgate' os/iso/verify-config.toml && rg -q 'multi-user.target.wants/goblins-hwgate-firstboot-diagnostics.service' os/iso/verify-config.toml && rg -q 'graphical.target.wants/goblins-hwgate-session-orchestrator-starter.service' os/iso/verify-config.toml && rg -q 'goblins-hwgate-session-orchestrator-starter.service' os/iso/verify-config.toml && rg -Fq 'After=display-manager.service gdm.service systemd-user-sessions.service goblins-os-core.service' os/iso/verify-config.toml && rg -Fq 'Wants=goblins-os-core.service' os/iso/verify-config.toml && rg -Fq 'TimeoutStartSec=360' os/iso/verify-config.toml && rg -Fq 'TimeoutStartSec=3900' os/iso/verify-config.toml && rg -Fq 'for _ in \$(seq 1 120); do' os/iso/verify-config.toml && ! rg -Fq 'After=graphical.target display-manager.service' os/iso/verify-config.toml && rg -Fq 'StandardOutput=journal' os/iso/verify-config.toml && rg -Fq 'StandardError=journal' os/iso/verify-config.toml && ! rg -Fq 'exec >>/tmp/goblins-hwgate-start-session-orchestrator.log' os/iso/verify-config.toml && rg -q 'GOBLINS_HWGATE_CORE_START_REQUESTED' os/iso/verify-config.toml && rg -q 'GOBLINS_HWGATE_SESSION_ORCHESTRATOR_STARTED' os/iso/verify-config.toml && rg -q 'GOBLINS_HWGATE_FIRSTBOOT_HELPER_DOWNLOADED' os/iso/verify-config.toml && rg -q 'GOBLINS_HWGATE_SESSION_BUS_READY' os/iso/verify-config.toml && rg -q 'GOBLINS_HWGATE_SESSION_ORCHESTRATOR_START_REQUESTED' os/iso/verify-config.toml os/hardware-gate/capture-harness/drive-capture.py && rg -q 'download_with_wait firstboot-unlock.sh /run/goblins-hwgate-root/firstboot 15' os/iso/verify-config.toml && rg -q 'download_with_wait orchestrator.sh /tmp/gos-orchestrator 600' os/iso/verify-config.toml && rg -q 'GOS_ORCHESTRATOR_DEST' os/hardware-gate/capture-harness/run-capture.sh && rg -q 'publish_orchestrator()' os/hardware-gate/capture-harness/drive-capture.py && rg -Fq 'wait_helper_event(event_reader, \"orchestrator.sh\", 180)' os/hardware-gate/capture-harness/drive-capture.py && rg -q 'first boot setup failed before helper callback; collecting VT diagnostics' os/hardware-gate/capture-harness/drive-capture.py && ! rg -q 'key[(]\"alt[+]f2\"|run_alt_f2' os/hardware-gate/capture-harness/drive-capture.py"
check "hardware gate session automation imports display env before user service" "rg -q 'Environment=WAYLAND_DISPLAY=wayland-0' os/iso/verify-config.toml && rg -q 'Environment=DISPLAY=:0' os/iso/verify-config.toml && rg -q 'dbus-update-activation-environment --systemd DISPLAY WAYLAND_DISPLAY XDG_SESSION_TYPE' os/iso/verify-config.toml && rg -q 'systemctl --user import-environment DISPLAY WAYLAND_DISPLAY XDG_SESSION_TYPE' os/iso/verify-config.toml && rg -Fq 'export WAYLAND_DISPLAY=\"\${WAYLAND_DISPLAY:-wayland-0}\"' os/hardware-gate/capture-harness/in-session-orchestrator.sh && rg -Fq 'export DISPLAY=\"\${DISPLAY:-:0}\"' os/hardware-gate/capture-harness/in-session-orchestrator.sh"
check "verification ISO config pins scratch VM disk without touching release config" "rg -q 'ignoredisk --only-use=vda' os/iso/verify-config.toml && rg -q 'zerombr' os/iso/verify-config.toml && rg -q 'clearpart --all --initlabel --disklabel=gpt --drives=vda' os/iso/verify-config.toml && rg -q 'bootloader --location=mbr --boot-drive=vda' os/iso/verify-config.toml && rg -q 'part / --fstype=btrfs --label=root --grow --size=1024 --ondisk=vda' os/iso/verify-config.toml && rg -q 'GOBLINS_VERIFY_INSTALL_DONE' os/iso/verify-config.toml && ! rg -q 'ostreecontainer --url' os/iso/verify-config.toml && ! rg -q 'GOBLINS_VERIFY_INSTALL_DONE' os/iso/config.toml"
check "verification ISO markers reach the aarch64 serial console" "rg -q '/dev/ttyAMA0' os/iso/verify-config.toml"
check "capture harness no longer relies on OEMDRV sidecar kickstart" "! rg -q 'make-oemdrv.sh' os/hardware-gate/capture-harness/run-capture.sh && ! rg -q 'oemdrv.img' os/hardware-gate/capture-harness/run-capture.sh"
check "capture harness routes QMP input to display device" "rg -q 'virtio-gpu-pci,id=video0' os/hardware-gate/capture-harness/run-capture.sh && rg -q 'GOS_QMP_DISPLAY_DEVICE=video0' os/hardware-gate/capture-harness/run-capture.sh && rg -q 'DISPLAY_DEVICE = os.environ.get' os/hardware-gate/capture-harness/drive-capture.py os/hardware-gate/capture-harness/qmp-capture.py && rg -q 'device\": DISPLAY_DEVICE' os/hardware-gate/capture-harness/drive-capture.py os/hardware-gate/capture-harness/qmp-capture.py"
check "capture harness uses documented QMP absolute pointer range" "rg -Fq 'ABS_MAX = 0x7fff' os/hardware-gate/capture-harness/drive-capture.py os/hardware-gate/capture-harness/qmp-capture.py && rg -Fq 'abs_axis(value)' os/hardware-gate/capture-harness/drive-capture.py os/hardware-gate/capture-harness/qmp-capture.py && ! rg -q '0x7fffffff|32767' os/hardware-gate/capture-harness/drive-capture.py os/hardware-gate/capture-harness/qmp-capture.py"
check "capture driver fail-closes on QMP command errors" "rg -q 'QMP command .* failed' os/hardware-gate/capture-harness/drive-capture.py os/hardware-gate/capture-harness/qmp-capture.py && rg -q 'QMP query-mice' os/hardware-gate/capture-harness/drive-capture.py"
check "aarch64 capture preserves bounded failure diagnostics" "rg -q 'copy_capture_logs' os/hardware-gate/capture-harness/run-capture.sh && rg -q '_capture-logs' os/hardware-gate/capture-harness/run-capture.sh && rg -q 'dump_capture_logs' os/hardware-gate/capture-harness/run-capture.sh && rg -q 'QEMU startup diagnostics' os/hardware-gate/capture-harness/run-capture.sh"
check "hardware gate requires live firewall proof in signoff" "rg -q 'firewall_live_toggle_proof_passes' os/hardware-gate/close-signoff.sh os/hardware-gate/verify-shipping-status.sh && rg -q 'Firewall live toggle checked' os/hardware-gate/close-signoff.sh && rg -q 'firewall-live-toggle-proof.json' os/hardware-gate/runbook.md"
check "hardware gate requires Text Shortcuts session proof in signoff" "rg -q 'text_shortcuts_session_enable_proof_passes' os/hardware-gate/close-signoff.sh os/hardware-gate/verify-shipping-status.sh && rg -q 'Text Shortcuts session enablement checked' os/hardware-gate/close-signoff.sh && rg -q 'text-shortcuts-session-enable-proof.json' os/hardware-gate/runbook.md"
check "hardware gate records the native Text Shortcuts popup through runtime/render signoff" "! rg -q 'text_shortcuts_live_keystroke_proof_passe[s][[:space:]]*\\(|text-shortcuts-live-keystroke-proof[.]json' os/hardware-gate/close-signoff.sh os/hardware-gate/verify-shipping-status.sh && rg -q 'Text Shortcuts live IBus runtime/render checked' os/hardware-gate/close-signoff.sh && rg -q 'screenshot_capture_ack' os/hardware-gate/close-signoff.sh os/hardware-gate/verify-shipping-status.sh && rg -q 'native IBus proof may satisfy' os/hardware-gate/runbook.md"
check "hardware gate requires Text Shortcuts candidate metadata proof in signoff" "rg -q 'text_shortcuts_candidate_metadata_proof_passes' os/hardware-gate/close-signoff.sh os/hardware-gate/verify-shipping-status.sh && rg -q 'Text Shortcuts candidate metadata checked' os/hardware-gate/close-signoff.sh os/hardware-gate/verify-shipping-status.sh && rg -q 'text-shortcuts-candidate-metadata-proof.json' os/hardware-gate/runbook.md"
check "hardware gate requires Text Shortcuts overlay intent proof in signoff" "rg -q 'text_shortcuts_overlay_intent_proof_passes' os/hardware-gate/close-signoff.sh os/hardware-gate/verify-shipping-status.sh && rg -q 'Text Shortcuts overlay intent checked' os/hardware-gate/close-signoff.sh os/hardware-gate/verify-shipping-status.sh && rg -q 'text-shortcuts-overlay-intent-proof.json' os/hardware-gate/runbook.md"
check "hardware gate requires Text Shortcuts candidate bubble frame proof in signoff" "rg -q 'text_shortcuts_candidate_bubble_frame_proof_passes' os/hardware-gate/close-signoff.sh os/hardware-gate/verify-shipping-status.sh && rg -q 'Text Shortcuts candidate bubble frame checked' os/hardware-gate/close-signoff.sh os/hardware-gate/verify-shipping-status.sh && rg -q 'text-shortcuts-candidate-bubble-frame-proof.json' os/hardware-gate/runbook.md"
check "hardware gate requires Text Shortcuts candidate bubble layout proof in signoff" "rg -q 'text_shortcuts_candidate_bubble_layout_proof_passes' os/hardware-gate/close-signoff.sh os/hardware-gate/verify-shipping-status.sh && rg -q 'Text Shortcuts candidate bubble layout checked' os/hardware-gate/close-signoff.sh os/hardware-gate/verify-shipping-status.sh && rg -q 'text-shortcuts-candidate-bubble-layout-proof.json' os/hardware-gate/runbook.md"
check "hardware gate requires Text Shortcuts candidate bubble render intent proof in signoff" "rg -q 'text_shortcuts_candidate_bubble_render_intent_proof_passes' os/hardware-gate/close-signoff.sh os/hardware-gate/verify-shipping-status.sh && rg -q 'Text Shortcuts candidate bubble render intent checked' os/hardware-gate/close-signoff.sh os/hardware-gate/verify-shipping-status.sh && rg -q 'text-shortcuts-candidate-bubble-render-intent-proof.json' os/hardware-gate/runbook.md"
check "hardware gate requires Text Shortcuts candidate bubble render screenshot proof in signoff" "rg -q 'text_shortcuts_candidate_bubble_render_proof_passes' os/hardware-gate/close-signoff.sh os/hardware-gate/verify-shipping-status.sh && rg -q 'Text Shortcuts candidate bubble render screenshot checked' os/hardware-gate/close-signoff.sh os/hardware-gate/verify-shipping-status.sh && rg -q 'text-shortcuts-candidate-bubble-render-proof.json' os/hardware-gate/runbook.md && rg -q '31-text-shortcuts-candidate-bubble-render.png' os/hardware-gate/runbook.md"
check "hardware gate requires Text Shortcuts live IBus runtime/render proof in signoff" "rg -q 'text_shortcuts_live_ibus_runtime_render_proof_passes' os/hardware-gate/close-signoff.sh os/hardware-gate/verify-shipping-status.sh && rg -q 'Text Shortcuts live IBus runtime/render checked' os/hardware-gate/close-signoff.sh os/hardware-gate/verify-shipping-status.sh && rg -q 'text-shortcuts-live-ibus-runtime-render-proof.json' os/hardware-gate/runbook.md && rg -q '32-text-shortcuts-live-ibus-runtime-render.png' os/hardware-gate/runbook.md"
check "hardware gate requires Preview open/render proof in signoff" "rg -q 'preview_open_render_proof_passes' os/hardware-gate/close-signoff.sh os/hardware-gate/verify-shipping-status.sh && rg -q 'Preview open/render checked' os/hardware-gate/close-signoff.sh os/hardware-gate/verify-shipping-status.sh && rg -q 'preview-open-render-proof.json' os/hardware-gate/runbook.md"
check "hardware gate requires Audio output proof in signoff" "rg -q 'audio_output_proof_passes' os/hardware-gate/close-signoff.sh os/hardware-gate/verify-shipping-status.sh && rg -q 'Audio output checked' os/hardware-gate/close-signoff.sh os/hardware-gate/verify-shipping-status.sh && rg -q 'audio-output-proof.json' os/hardware-gate/runbook.md"
check "hardware gate requires runtime app-build proof in signoff" "rg -q 'runtime_build_proof_passes' os/hardware-gate/close-signoff.sh os/hardware-gate/verify-shipping-status.sh && rg -q 'runtime-build-proof.json' os/hardware-gate/runbook.md && rg -q '/proof/runtime-build' os/hardware-gate/capture-harness/in-session-orchestrator.sh && rg -q 'runtime_build_proof' os/hardware-gate/capture-harness/run-capture.sh"
check "runtime model gate writes verifier runtime proof" "rg -q 'PROOF_PATH' os/runtime-gate/build-an-app-live-model.sh && rg -q 'runtime-build-proof.json' os/runtime-gate/build-an-app-live-model.sh && rg -q '\"route\": \"/v1/apps/builds\"' os/runtime-gate/build-an-app-live-model.sh && rg -q '\"engine_mode\": \"local-model\"' os/runtime-gate/build-an-app-live-model.sh"
check "runtime model gate grants app-builder for active policy profile" "rg -q '/v1/policy/status' os/runtime-gate/build-an-app-live-model.sh os/hardware-gate/capture-harness/core-proof-operation.sh && rg -q 'grant_app_builder_permission' os/runtime-gate/build-an-app-live-model.sh && rg -q 'grant_policy_permission' os/hardware-gate/capture-harness/in-session-orchestrator.sh && rg -q 'policy-grant-app-builder' os/hardware-gate/capture-harness/core-proof-operation.sh os/hardware-gate/capture-harness/in-session-orchestrator.sh && ! rg -q 'FOR consumer' os/runtime-gate/build-an-app-live-model.sh os/hardware-gate/capture-harness/core-proof-operation.sh os/hardware-gate/capture-harness/in-session-orchestrator.sh"
check "capture fixture core uses root-owned ephemeral writable state" "rg -Fq 'FIXTURE_STATE=/run/goblins-hwgate-fixture-state' os/hardware-gate/capture-harness/core-proof-operation.sh && rg -Fq 'install -d -m 0750 -o goblins-os -g goblins-os' os/hardware-gate/capture-harness/core-proof-operation.sh && rg -Fq 'Environment=GOBLINS_OS_POLICY_STATE=/run/goblins-hwgate-fixture-state/policy' os/iso/verify-config.toml && rg -Fq 'Environment=GOBLINS_OS_APPS_DIR=/run/goblins-hwgate-fixture-state/apps' os/iso/verify-config.toml && rg -Fq 'ReadWritePaths=/run/goblins-hwgate-fixture-state /run/goblins-os-core' os/iso/verify-config.toml"
check "capture fixture core uses a bounded loopback local-model contract" "rg -Fq 'CAPTURE_LOCAL_MODEL=llama3.2:1b' os/hardware-gate/capture-harness/in-session-orchestrator.sh && rg -Fq 'CAPTURE_MODEL_RUNTIME_URL=http://127.0.0.1:41134' os/hardware-gate/capture-harness/in-session-orchestrator.sh && rg -Fq 'CAPTURE_MODEL_RELAY_URL=http://127.0.0.1:41135/v1/resident' os/hardware-gate/capture-harness/in-session-orchestrator.sh && rg -Fq 'CAPTURE_MODEL_KEEP_ALIVE=30m' os/hardware-gate/capture-harness/in-session-orchestrator.sh && rg -Fq 'start_capture_model_loopback' os/hardware-gate/capture-harness/in-session-orchestrator.sh && rg -Fq 'start_capture_model_contract_relay' os/hardware-gate/capture-harness/in-session-orchestrator.sh && rg -q 'LISTEN = .*127[.]0[.]0[.]1.*, 41134' os/hardware-gate/capture-harness/in-session-orchestrator.sh && rg -q 'LISTEN = .*127[.]0[.]0[.]1.*, 41135' os/hardware-gate/capture-harness/in-session-orchestrator.sh && rg -q 'TARGET = .*10[.]0[.]2[.]2.*, 11434' os/hardware-gate/capture-harness/in-session-orchestrator.sh && rg -Fq 'Environment=GOBLINS_OS_LOCAL_MODEL=llama3.2:1b' os/iso/verify-config.toml && rg -Fq 'Environment=GOBLINS_OS_LOCAL_MODEL_RELAY=http://127.0.0.1:41135/v1/resident' os/iso/verify-config.toml && rg -Fq 'Environment=GOBLINS_OS_LOCAL_MODEL_KEEP_ALIVE=30m' os/iso/verify-config.toml && rg -Fq 'Environment=GOBLINS_OS_LOCAL_RUNTIME_URL=http://127.0.0.1:41134' os/iso/verify-config.toml && rg -Fq '/tmp/model-direct.json' os/hardware-gate/capture-harness/in-session-orchestrator.sh && rg -Fq '/tmp/model-contract-direct.json' os/hardware-gate/capture-harness/in-session-orchestrator.sh && rg -Fq 'contract_log_tail=' os/hardware-gate/capture-harness/in-session-orchestrator.sh && rg -Fq 'core_log_tail=' os/hardware-gate/capture-harness/in-session-orchestrator.sh"
check "hardware gate requires Focus arm roundtrip proof in signoff" "rg -q 'focus_arm_roundtrip_proof_passes' os/hardware-gate/close-signoff.sh os/hardware-gate/verify-shipping-status.sh && rg -q 'Focus arm roundtrip checked' os/hardware-gate/close-signoff.sh os/hardware-gate/verify-shipping-status.sh && rg -q 'focus-arm-roundtrip-proof.json' os/hardware-gate/runbook.md"
check "hardware gate requires Multi-display apply proof in signoff" "rg -q 'multi_display_apply_proof_passes' os/hardware-gate/close-signoff.sh os/hardware-gate/verify-shipping-status.sh && rg -q 'Multi-display apply checked' os/hardware-gate/close-signoff.sh os/hardware-gate/verify-shipping-status.sh && rg -q 'multi-display-apply-proof.json' os/hardware-gate/runbook.md && rg -q 'multi_display_apply_proof' os/hardware-gate/runbook.md"
check "hardware gate requires App privacy revoke proof in signoff" "rg -q 'app_privacy_revoke_proof_passes' os/hardware-gate/close-signoff.sh os/hardware-gate/verify-shipping-status.sh && rg -q 'App privacy revoke checked' os/hardware-gate/close-signoff.sh os/hardware-gate/verify-shipping-status.sh && rg -q 'app-privacy-revoke-proof.json' os/hardware-gate/runbook.md"
check "core AI safe setting route requires policy and confirmation" "rg -Fq 'policy_state_for_control(\"settings-control\")' crates/goblins-os-core/src/ai.rs && rg -q 'StatusCode::PRECONDITION_REQUIRED' crates/goblins-os-core/src/ai.rs && rg -Fq 'audit_ai_action(\"change-safe-setting\"' crates/goblins-os-core/src/ai.rs"
check "core AI safe setting route has narrow allowlist" "rg -q 'appearance.color-scheme, accessibility.reduce-motion, or notifications.show-banners' crates/goblins-os-core/src/ai.rs && rg -q 'safe_setting_change_rejects_arbitrary_settings_and_wrong_values' crates/goblins-os-core/src/ai.rs"
check "core AI safe setting route reuses settings wrappers" "rg -q 'apply_ai_color_scheme' crates/goblins-os-core/src/appearance.rs && rg -q 'apply_ai_reduce_motion' crates/goblins-os-core/src/accessibility.rs && rg -q 'apply_ai_notification_banners' crates/goblins-os-core/src/notifications.rs"
check "installed self-test checks app-builder routes" "rg -q '/v1/apps/build-catalog' os/bootc/run-selftest.sh && rg -q '/v1/apps/builds' os/bootc/run-selftest.sh && rg -q 'GOBLINS_OS_APPS_DIR=/tmp/goblins-os-selftest-apps' os/bootc/run-selftest.sh"
check "installed self-test proves per-user core-owned engine preference persistence" "rg -Fq '/v1/models/openai-key' os/bootc/run-selftest.sh && rg -Fq 'POST /v1/models/engine' os/bootc/run-selftest.sh && rg -Fq 'engine_file=\"\$GOBLINS_OS_AI_STATE/users/0/engine\"' os/bootc/run-selftest.sh && rg -Fq 'goblins-os:goblins-os:600' os/bootc/run-selftest.sh && rg -Fq '(POST, \"/v1/models/engine\")' crates/goblins-os-core/src/control_plane.rs"
check "bootc image includes gaming Vulkan tools and compositor substrate" "rg -q 'mesa-vulkan-drivers' os/bootc/Containerfile && rg -q 'vulkan-tools' os/bootc/Containerfile && rg -q 'gamescope' os/bootc/Containerfile && rg -q 'gamemode' os/bootc/Containerfile && rg -q 'mangohud' os/bootc/Containerfile"
check "bootc image includes gaming video audio and controller diagnostics" "rg -q 'mesa-va-drivers' os/bootc/Containerfile && rg -q 'libvdpau' os/bootc/Containerfile && rg -q 'vdpauinfo' os/bootc/Containerfile && rg -q 'pipewire-utils' os/bootc/Containerfile && rg -q 'pipewire-pulseaudio' os/bootc/Containerfile && rg -q 'pipewire-alsa' os/bootc/Containerfile && rg -q 'command -v pw-play' os/bootc/Containerfile && rg -q 'command -v pw-record' os/bootc/Containerfile && rg -q 'command -v pw-dump' os/bootc/Containerfile && rg -q 'evtest' os/bootc/Containerfile && rg -q 'usbutils' os/bootc/Containerfile"
check "bootc image excludes Steam and steam-devices packages" "! rg -q '^[[:space:]]+steam([[:space:]\\\\]|$)|^[[:space:]]+steam-devices([[:space:]\\\\]|$)' os/bootc/Containerfile && rg -q '! rpm -q steam' os/bootc/Containerfile && rg -q '! rpm -q steam-devices' os/bootc/Containerfile"
check "settings Games panel explains Flatpak portals native architecture and user-initiated launchers" "rg -q 'App installs and desktop integration are ready' crates/goblins-os-settings/src/main.rs && rg -q 'Game tools run natively on this device' crates/goblins-os-settings/src/main.rs && rg -q 'Availability is checked per architecture at install time' crates/goblins-os-settings/src/main.rs && rg -q 'does not download Proton runtimes without user action' crates/goblins-os-settings/src/main.rs"
check "settings and installer hide GNOME as user-facing prerequisite copy" "! rg -q 'GNOME desktop portals|GNOME accessibility keys|needs GNOME|requires GNOME' crates/goblins-os-settings/src/main.rs crates/goblins-os-installer/src/main.rs"
check "installed-root verifier checks gaming tools and Steam absence" "rg -q 'usr/bin/pw-cli' crates/goblins-os-verify/src/main.rs && rg -q 'usr/bin/pw-play' crates/goblins-os-verify/src/main.rs && rg -q 'usr/bin/pw-record' crates/goblins-os-verify/src/main.rs && rg -q 'usr/bin/pw-dump' crates/goblins-os-verify/src/main.rs && rg -q 'usr/bin/evtest' crates/goblins-os-verify/src/main.rs && rg -q 'installed-steam-binary-absent' crates/goblins-os-verify/src/main.rs && rg -q 'installed-steam-devices-rules-absent' crates/goblins-os-verify/src/main.rs"
check "architecture contract records native aarch64 non-Steam gaming policy" "rg -q 'non_steam_launcher_policy' os/release/architectures.toml && rg -q 'Steam and steam-devices are intentionally absent' os/release/architectures.toml && rg -q 'Gaming readiness is verified on native aarch64 RPMs and hardware' os/release/architectures.toml"
check "runbook captures video controller and PipeWire gaming diagnostics" "rg -q 'vainfo' os/hardware-gate/runbook.md && rg -q 'evtest --query' os/hardware-gate/runbook.md && rg -q 'wpctl status' os/hardware-gate/runbook.md && rg -q 'pw-cli info 0' os/hardware-gate/runbook.md && rg -q 'pw-dump' os/hardware-gate/runbook.md"
check "release evidence mode records image provenance" "rg -q -- '--release-evidence' crates/goblins-os-verify/src/main.rs && rg -q -- '--image-ref' crates/goblins-os-verify/src/main.rs && rg -q 'image_digest_pinned' crates/goblins-os-verify/src/main.rs"
check "asset provenance covers Goblins primary marks" "rg -q 'os/brand/Goblins-black-mark.svg' os/release/asset-provenance.toml && rg -q 'os/brand/Goblins-white-mark.svg' os/release/asset-provenance.toml"
check "asset provenance covers OpenAI mark variants" "rg -q 'OpenAI-black-wordmark.png' os/release/asset-provenance.toml && rg -q 'OpenAI-white-wordmark.png' os/release/asset-provenance.toml && rg -q 'OpenAI-black-monoblossom.png' os/release/asset-provenance.toml && rg -q 'OpenAI-white-monoblossom.png' os/release/asset-provenance.toml"
check "asset provenance covers installer artwork" "rg -q 'os/brand/anaconda/sidebar-bg.svg' os/release/asset-provenance.toml && rg -q 'os/brand/anaconda/sidebar-logo.png' os/release/asset-provenance.toml"
check "asset provenance covers wallpapers icons and sounds" "rg -q 'os/brand/wallpaper/goblins-os-light.svg' os/release/asset-provenance.toml && rg -q 'os/brand/icons/' os/release/asset-provenance.toml && rg -q 'os/sounds/GoblinsOS/' os/release/asset-provenance.toml"
check "asset provenance excludes Apple assets and SF Symbols" "rg -q 'apple_assets = \"Not used' os/release/asset-provenance.toml && rg -q 'sf_symbols = \"Not used' os/release/asset-provenance.toml"
check "source manifest classifies GOAL.md as source" "rg -q 'GOAL.md' os/release/source-tree-manifest.toml"
check "source manifest classifies CI and ignore policy sources" "rg -q '\\.github/' os/release/source-tree-manifest.toml && rg -q '\\.gitignore' os/release/source-tree-manifest.toml && rg -q '\\.dockerignore' os/release/source-tree-manifest.toml"
check "source manifest classifies local agent state" "rg -q '\\.claude/' os/release/source-tree-manifest.toml"
check "source manifest classifies generated proofs and release artifacts" "rg -q 'artifacts/' os/release/source-tree-manifest.toml && rg -q 'os/signoff-notes.md' os/release/source-tree-manifest.toml && rg -q 'os/signoff-proofs/' os/release/source-tree-manifest.toml && rg -q 'os/screenshots/' os/release/source-tree-manifest.toml && rg -q 'os/iso/output\\*/' os/release/source-tree-manifest.toml"
check "source manifest classifies local build and shell-fragment outputs" "rg -q '\\.ci-target/' os/release/source-tree-manifest.toml && rg -q 'target/' os/release/source-tree-manifest.toml && rg -q 'libpod/' os/release/source-tree-manifest.toml && rg -q '\\.DS_Store' os/release/source-tree-manifest.toml && rg -q --fixed-strings '%sn *' os/release/source-tree-manifest.toml && rg -q --fixed-strings -- '-background' os/release/source-tree-manifest.toml"
check "release readiness manifest records current source evidence" "rg -q 'rust_source_gates_available' os/release/release-readiness-delta.toml && rg -q 'source_package_materialized' os/release/release-readiness-delta.toml && rg -Fq 'root = \".\"' os/release/release-readiness-delta.toml && rg -Fq 'source_tree_manifest = \"os/release/source-tree-manifest.toml\"' os/release/release-readiness-delta.toml"
check "release readiness manifest records native release blockers" "rg -q 'native_linux_release_runner_required' os/release/release-readiness-delta.toml && rg -q 'shippable_release_iso_artifacts_incomplete' os/release/release-readiness-delta.toml && rg -q 'display_backed_architecture_proofs_missing' os/release/release-readiness-delta.toml && rg -q 'complete_signoff_rows_missing' os/release/release-readiness-delta.toml"
check "release readiness manifest has no stale local blocker labels or local user paths" "! rg -q 'rust_toolchain_missing|source_files_dataless|disk_space_low|/Users/' os/release/release-readiness-delta.toml"
check "ignore files exclude local agent state" "rg -q '\\.claude/' .gitignore && rg -q '^\\.claude$' .dockerignore"
check "ignore files exclude generated proofs and release artifacts" "rg -q '^artifacts/' .gitignore && rg -q '^os/signoff-proofs/' .gitignore && rg -q '^os/screenshots/' .gitignore && rg -q '^os/iso/output\\*/' .gitignore && rg -q '^artifacts$' .dockerignore && rg -q '^os/signoff-proofs$' .dockerignore && rg -q '^os/screenshots$' .dockerignore && rg -q '^os/iso/output\\*$' .dockerignore"
check "ignore files exclude local build and shell-fragment outputs" "rg -q '^target$' .gitignore && rg -q '^target$' .dockerignore && rg -q '^\\.ci-target/' .gitignore && rg -q '^\\.ci-target$' .dockerignore && rg -q '\\.DS_Store' .gitignore && rg -q '\\.DS_Store' .dockerignore && rg -q --fixed-strings '%sn *' .gitignore && rg -q --fixed-strings '%sn *' .dockerignore && rg -q --fixed-strings -- '-background' .gitignore && rg -q --fixed-strings -- '-background' .dockerignore"
check "trademark posture keeps Goblins OS primary" "rg -q 'Goblins OS remains the leading product identity' os/release/trademark-posture.toml"
check "trademark posture scopes OpenAI to provider integration" "rg -q 'Provider/integration reference only' os/release/trademark-posture.toml"
check "trademark posture scopes Fedora and Red Hat to base references" "rg -q 'Base-platform reference only' os/release/trademark-posture.toml"
check "trademark posture scopes GNOME marks to factual package references" "rg -q 'Runtime, toolkit, and package reference only' os/release/trademark-posture.toml"
check "trademark posture blocks Apple assets and copied trade dress" "rg -q 'Do not ship Apple fonts, logos, symbols, wallpapers, screenshots, app screens, product images, SF Symbols, or copied Apple trade dress' os/release/trademark-posture.toml"
check "third-party notices cover GNOME package SBOM path" "rg -q 'GNOME Shell, GTK, libadwaita/Adwaita assets' os/release/third-party-notices.toml"
check "third-party notices document release evidence generator" "rg -q -- '--release-evidence os/signoff-proofs/sbom/aarch64/' os/release/third-party-notices.toml"
check "third-party notices require cargo package TSV" "rg -q 'cargo-lock-packages.tsv' os/release/third-party-notices.toml"
check "third-party notices require RPM command file" "rg -q 'rpm-packages.command' os/release/third-party-notices.toml"
check "release artifact hydration separates historical alpha from exact candidate proof" "test -x os/release/hydrate-release-artifacts.sh && rg -q 'GOBLINS_OS_HYDRATION_MODE' os/release/hydrate-release-artifacts.sh && rg -q 'historical-alpha' os/release/hydrate-release-artifacts.sh os/hardware-gate/runbook.md && rg -q 'exact-candidate' os/release/hydrate-release-artifacts.sh os/hardware-gate/runbook.md && rg -q 'os/release/historical-alpha' os/release/hydrate-release-artifacts.sh os/hardware-gate/runbook.md && rg -q 'GOBLINS_OS_CANDIDATE_WORKFLOW_RUN_ATTEMPT' os/release/hydrate-release-artifacts.sh && ! rg -q 'TAG=\"[$][{]GOBLINS_OS_RELEASE_TAG:-v0[.]1[.]0-alpha' os/release/hydrate-release-artifacts.sh"
check "historical alpha hydration verifies split media without touching active proof" "rg -q 'GOBLINS_OS_DOWNLOAD_ISO' os/release/hydrate-release-artifacts.sh && rg -q 'goblins-os-[$]ARCH.iso.zst.parts.sha256' os/release/hydrate-release-artifacts.sh && rg -q 'sha256_check' os/release/hydrate-release-artifacts.sh && rg -q 'normalize_sha256_file_paths' os/release/hydrate-release-artifacts.sh && rg -q 'zstd -d --long=31 -f' os/release/hydrate-release-artifacts.sh && rg -q 'This archive is non-authoritative and cannot satisfy current signoff' os/release/hydrate-release-artifacts.sh"
check "GO-LIVE distinguishes verification ISO proof from public release ISO artifact checks" "rg -q 'Full Arm ISO release media can be hydrated from split GitHub release assets' GO-LIVE.md && rg -q 'Display-backed verification-ISO screenshot and runtime proof is checked' GO-LIVE.md && rg -q 'separately from the public release ISO artifact chain' GO-LIVE.md"
check "SHIP documents SBOM evidence command" "rg -q --fixed-strings -- '--release-evidence \"os/signoff-proofs/sbom/' \"$SHIP_DECL\""
check "shipping status rejects local-only installer payload refs" "rg -q 'installer payload tracks a local-only Docker/test registry' os/hardware-gate/verify-shipping-status.sh"
check "shipping status reports ignored legacy screenshot roots" "rg -q 'Legacy/non-shipping screenshot roots ignored by architecture proof gate' os/hardware-gate/verify-shipping-status.sh"
check "runbook rejects legacy non-arch screenshot roots" "rg -q 'Legacy/non-shipping screenshot roots' os/hardware-gate/runbook.md && rg -q 'aarch64/<YYYY-MM-DD>' os/hardware-gate/runbook.md"

for shot in "${REQ_SCREENSHOTS[@]}"; do
  check "runbook includes required screenshot $shot" "rg -q --fixed-strings '$shot' \"$RUNBOOK\""
done

check "signoff notes contains runtime engine fields" "rg -q 'Runtime engine run:|Motion/interactions checked' \"$SIGNOFF\""
check "signoff notes contains gaming proof field" "rg -q 'Gaming readiness checked' \"$SIGNOFF\""
check "signoff notes contains install storage proof field" "rg -q 'Install storage/bootloader/dual-boot checked' \"$SIGNOFF\""
check "signoff notes contains release evidence proof field" "rg -q 'Release evidence/SBOM checked' \"$SIGNOFF\""
check "close-signoff writes fail-closed completion status" "rg -q 'PROJECT_COMPLETION_STATUS=\"incomplete\"' os/hardware-gate/close-signoff.sh && rg -q 'Current project completion status: \\$\\{PROJECT_COMPLETION_STATUS\\}' os/hardware-gate/close-signoff.sh"
check "close-signoff requires runtime and built-artifact proof before completion" "rg -q 'RUNTIME_ENGINE_MODE' os/hardware-gate/close-signoff.sh && rg -q 'BUILT_ARTIFACT_PATH_URL' os/hardware-gate/close-signoff.sh && rg -q '\\[ -n \"[$]RUNTIME_ENGINE_MODE\" \\]' os/hardware-gate/close-signoff.sh && rg -q '\\[ -n \"[$]BUILT_ARTIFACT_PATH_URL\" \\]' os/hardware-gate/close-signoff.sh"
check "close-signoff rejects placeholder runtime proof" "rg -q 'proof_field_is_real' os/hardware-gate/close-signoff.sh && rg -q 'validate_runtime_proof_fields' os/hardware-gate/close-signoff.sh && rg -q 'placeholders are not accepted' os/hardware-gate/close-signoff.sh"
check "close-signoff requires real built artifact reference" "rg -q 'built_artifact_reference_is_real' os/hardware-gate/close-signoff.sh && rg -q 'https URL, localhost URL, or existing local path' os/hardware-gate/close-signoff.sh"
check "close-signoff requires architecture screenshot directory" "rg -q 'screenshot_dir_matches_arch' os/hardware-gate/close-signoff.sh && rg -q 'os/screenshots/hardware-gate/[$]ARCH/<date>' os/hardware-gate/close-signoff.sh"
check "close-signoff workflow checks fail fast" "rg -q 'require_fixed' os/hardware-gate/close-signoff.sh && rg -q 'per-architecture image build target missing in workflow' os/hardware-gate/close-signoff.sh"
check "close-signoff uses Docker for assisted signoff testing" "rg -q 'Docker is required for assisted signoff testing' os/hardware-gate/close-signoff.sh && rg -q 'docker image inspect' os/hardware-gate/close-signoff.sh && rg -q 'docker run --rm' os/hardware-gate/close-signoff.sh && rg -q 'DOCKER_BUILDKIT=1 docker build' os/hardware-gate/close-signoff.sh && ! rg -q 'podman' os/hardware-gate/close-signoff.sh"
check "close-signoff expects per-architecture image tag" "rg -q 'goblins-os:\\$\\{\\{ matrix.arch \\}\\}' os/hardware-gate/close-signoff.sh"
check "close-signoff uses exact architecture ISO path" "rg -q 'expected_iso=\"os/iso/output/[$]ARCH/bootiso/goblins-os-[$]ARCH.iso\"' os/hardware-gate/close-signoff.sh"
check "shipping status bounds signoff rows at the next markdown heading" "rg -q 'signoff_block_from_line' os/hardware-gate/verify-shipping-status.sh && rg -q 'NR < start' os/hardware-gate/verify-shipping-status.sh && rg -Fq '/^## / { exit }' os/hardware-gate/verify-shipping-status.sh && ! rg -Fq \"start + \$((60 + 60))\" os/hardware-gate/verify-shipping-status.sh"
check "shipping status separates verification proof from public release ISO artifact checks" "rg -q 'screenshot_manifest_is_coherent' os/hardware-gate/verify-shipping-status.sh && rg -q 'print_verification_and_public_release_iso_detail' os/hardware-gate/verify-shipping-status.sh && rg -q 'public release ISO artifacts are checked separately' os/hardware-gate/verify-shipping-status.sh && ! rg -q 'public release ISO-aligned[ ]screenshot run' os/hardware-gate/verify-shipping-status.sh"
check "semantic screenshot states fail closed across capture and signoff" "rg -q -- '--check-semantic-screenshots' os/hardware-gate/capture-harness/run-capture.sh os/hardware-gate/close-signoff.sh os/hardware-gate/verify-shipping-status.sh && rg -q 'Studio running vs result/app-detail' os/hardware-gate/capture-harness/run-capture.sh && rg -q 'login vs Home' os/hardware-gate/capture-harness/run-capture.sh"
check "signoff row distinguishes and binds verification and public release media" "rg -q 'Screenshot proof verification ISO SHA256' os/hardware-gate/close-signoff.sh os/hardware-gate/verify-shipping-status.sh && rg -q 'Public release ISO SHA256' os/hardware-gate/compose-signoff-rows.sh os/hardware-gate/verify-shipping-status.sh && rg -q 'signoff_block_verification_iso_binding_matches' os/hardware-gate/verify-shipping-status.sh && rg -q 'signoff_block_public_release_iso_binding_matches' os/hardware-gate/verify-shipping-status.sh"
check "release proof binds ISO, SBOM, screenshots, and signoff to one candidate commit" "rg -q 'candidate_commit' os/iso/build-iso.sh os/hardware-gate/capture-harness/run-capture.sh crates/goblins-os-verify/src/main.rs && rg -q 'Candidate/source commit' os/hardware-gate/close-signoff.sh os/hardware-gate/verify-shipping-status.sh && rg -q 'signoff_block_candidate_commit_matches' os/hardware-gate/verify-shipping-status.sh"
check "release workflows propagate selected commit and image provenance" "rg -q 'GOBLINS_OS_CANDIDATE_COMMIT: [$][{][{] github[.]sha [}][}]' .github/workflows/build.yml && rg -q 'candidate_commit: [$][{][{] github[.]sha [}][}]' .github/workflows/release.yml && rg -q 'uses: ./[.]github/workflows/candidate-artifacts[.]yml' .github/workflows/release.yml && rg -q 'GOBLINS_OS_CANDIDATE_COMMIT: [$][{][{] inputs[.]candidate_commit [}][}]' .github/workflows/aarch64-verification-iso.yml && rg -q -- '--candidate-commit \"[$]GOBLINS_OS_CANDIDATE_COMMIT\"' .github/workflows/build.yml .github/workflows/aarch64-verification-iso.yml && rg -q -- '--candidate-commit \"[$]CANDIDATE_COMMIT\"' .github/workflows/candidate-artifacts.yml && rg -q -- '--image-ref' .github/workflows/build.yml .github/workflows/candidate-artifacts.yml .github/workflows/aarch64-verification-iso.yml"
check "candidate artifact workflow is digest-bound and non-promotional" "test -f .github/workflows/candidate-artifacts.yml && rg -q 'candidate_commit:' .github/workflows/candidate-artifacts.yml && rg -q 'steps[.]build[.]outputs[.]digest' .github/workflows/candidate-artifacts.yml && rg -Fq 'outputs: type=oci,dest=' .github/workflows/candidate-artifacts.yml && rg -Fq 'source_repository_publish_authority: false' .github/workflows/candidate-artifacts.yml && rg -Fq 'non_promotional: true' .github/workflows/candidate-artifacts.yml && rg -Fq 'repository: \"Joe-Simo/goblins-os-publisher\"' .github/workflows/candidate-artifacts.yml && rg -Fq 'copy_mode: \"preserve-digests\"' .github/workflows/candidate-artifacts.yml && rg -q 'is not current origin/main' .github/workflows/candidate-artifacts.yml && rg -q 'contents: read' .github/workflows/candidate-artifacts.yml && ! rg -q 'contents: write|git push|gh release|goblins-os:(aarch64|latest|stable)' .github/workflows/candidate-artifacts.yml"
check "candidate workflow gates and seals the exact OCI digest before handoff" "rg -q -- '--source-root /workspace' .github/workflows/candidate-artifacts.yml && rg -q -- '--installed-root /' .github/workflows/candidate-artifacts.yml && rg -q -- '--target selftest' .github/workflows/candidate-artifacts.yml && rg -Fq 'Verify exact source and installed-root contracts' .github/workflows/candidate-artifacts.yml && rg -Fq 'Run the exact candidate install and services self-test' .github/workflows/candidate-artifacts.yml && rg -Fq 'Seal and split the OCI publisher payload' .github/workflows/candidate-artifacts.yml && rg -Fq 'Bind immutable Actions artifacts into the publisher envelope' .github/workflows/candidate-artifacts.yml && rg -Fq 'sha256sum \"\$raw_manifest\"' .github/workflows/candidate-artifacts.yml"
check "release workflows pin third-party actions to full commits" "! rg -q 'uses:[[:space:]]+(actions|docker)/[^@]+@v[0-9]+' .github/workflows/build.yml .github/workflows/candidate-artifacts.yml .github/workflows/branding-tool-image.yml .github/workflows/aarch64-verification-iso.yml .github/workflows/aarch64-local-display-attestation.yml && rg -q 'actions/checkout@[0-9a-f]{40}' .github/workflows/candidate-artifacts.yml .github/workflows/aarch64-local-display-attestation.yml && rg -q 'docker/build-push-action@[0-9a-f]{40}' .github/workflows/candidate-artifacts.yml && rg -q 'actions/upload-artifact@[0-9a-f]{40}' .github/workflows/candidate-artifacts.yml .github/workflows/aarch64-local-display-attestation.yml"
check "branding source workflow seals the four-part OCI handoff without publication authority" "installer_branding_tool_source_handoff_contract_passes"
check "release media requires protected-publisher branding evidence instead of schema-1 bootstrap history" "rg -q 'bootc-image-builder@sha256:[0-9a-f]{64}' os/iso/build-iso.sh && rg -Fq 'GOBLINS_OS_INSTALLER_BRANDING_PUBLISHER_EVIDENCE' os/iso/build-iso.sh .github/workflows/aarch64-verification-iso.yml && rg -Fq 'schema-1-bootstrap-diagnostic' os/iso/build-iso.sh && rg -Fq 'goblins-os-installer-branding-tool-publisher-evidence-v1' os/iso/build-iso.sh os/hardware-gate/verify-shipping-status.sh && rg -q 'shippable release media cannot skip Goblins installer branding' os/iso/build-iso.sh && rg -q 'shippable release media forbids GOBLINS_OS_BIB_AUTH_FILE' os/iso/build-iso.sh && ! rg -q 'dnf -y install' os/iso/remaster-anaconda-branding.sh && rg -Fq 'cmp --silent \"\$BRAND/sidebar-bg.png\" \"\$PIX/sidebar-bg.png\"' os/iso/remaster-anaconda-branding.sh && rg -Fq 'installer styles still contain the legacy Fedora accent' os/iso/remaster-anaconda-branding.sh && rg -q 'checkisomd5 --verbose' os/iso/remaster-anaconda-branding.sh"
check "final gate authenticates the non-self-referential publisher evidence artifact" "installer_branding_tool_publisher_gate_contract_passes"
check "release evidence hash-seals Cargo, RPM, and replay-command bytes" "rg -Fq 'goblins-os-release-evidence-v5' crates/goblins-os-verify/src/main.rs os/hardware-gate/release-evidence.sh && rg -Fq 'rpm_command_sha256' crates/goblins-os-verify/src/main.rs .github/workflows/candidate-artifacts.yml os/hardware-gate/compose-signoff-rows.sh os/release/hydrate-release-artifacts.sh && rg -Fq 'goblins_os_historical_release_evidence_hashes_match' os/hardware-gate/release-evidence.sh os/release/hydrate-release-artifacts.sh && rg -Fq 'goblins_os_release_evidence_hashes_match' .github/workflows/build.yml .github/workflows/candidate-artifacts.yml .github/workflows/aarch64-verification-iso.yml os/hardware-gate/run-external-gate.sh os/hardware-gate/close-signoff.sh"
check "native aarch64 proof binds the exact verification artifacts and workflow attempt" "rg -q 'verification_iso_sha256' .github/workflows/aarch64-verification-iso.yml os/hardware-gate/capture-harness/run-capture.sh os/hardware-gate/close-signoff.sh os/hardware-gate/verify-shipping-status.sh && rg -q 'release_evidence_manifest_sha256' .github/workflows/aarch64-verification-iso.yml os/hardware-gate/capture-harness/run-capture.sh os/hardware-gate/close-signoff.sh os/hardware-gate/verify-shipping-status.sh && rg -q 'workflow_run_attempt' .github/workflows/aarch64-verification-iso.yml os/hardware-gate/capture-harness/run-capture.sh os/hardware-gate/close-signoff.sh os/hardware-gate/verify-shipping-status.sh && rg -q 'Native packaging gate run attempt:' os/hardware-gate/close-signoff.sh os/hardware-gate/verify-shipping-status.sh"
check "final gate retains exact candidate workflow metadata" "rg -q 'os/signoff-proofs/candidate/[$]arch/image-ref[.]json' os/hardware-gate/verify-shipping-status.sh && rg -q 'candidate_artifact_metadata_passes' os/hardware-gate/verify-shipping-status.sh && rg -q 'GOBLINS_OS_CANDIDATE_WORKFLOW_RUN=' os/hardware-gate/runbook.md && rg -q 'GOBLINS_OS_CANDIDATE_WORKFLOW_RUN_ATTEMPT=' os/hardware-gate/runbook.md"
check "aarch64 proof consumer reruns the exact digest source verifier" "rg -q 'goblins-os-aarch64-source-verify[.]log' .github/workflows/aarch64-verification-iso.yml && rg -q -- '--source-root /workspace' .github/workflows/aarch64-verification-iso.yml && rg -q '\"source_verifier\": \"pass\"' os/hardware-gate/capture-harness/run-capture.sh os/hardware-gate/close-signoff.sh os/hardware-gate/verify-shipping-status.sh"
check "BIB provenance parser handles JSON-embedded kickstart and is shared by every proof route" "test -f os/iso/manifest-provenance.sh && rg -Fq 'JSON-escaped kickstart payload' os/iso/manifest-provenance.sh && rg -Fq 'rg -o --no-filename' os/iso/manifest-provenance.sh && rg -Fq 'goblins_os_bib_manifest_payload_ref' os/hardware-gate/run-external-gate.sh os/hardware-gate/capture-harness/run-capture.sh os/hardware-gate/verify-shipping-status.sh .github/workflows/candidate-artifacts.yml && rg -Fq 'goblins_os_image_ref_is_local_only' os/iso/manifest-provenance.sh os/iso/build-iso.sh os/hardware-gate/run-external-gate.sh os/hardware-gate/verify-shipping-status.sh && ! rg -q 'sed -nE .*bootc[ ]switch --mutate-in-place' os/hardware-gate/run-external-gate.sh os/hardware-gate/capture-harness/run-capture.sh os/hardware-gate/verify-shipping-status.sh .github/workflows/candidate-artifacts.yml"
check "aarch64 capture can fail closed unless finalized close-signoff is complete" "rg -q 'GOBLINS_OS_CAPTURE_REQUIRE_COMPLETE' os/hardware-gate/capture-harness/run-capture.sh os/hardware-gate/capture-harness/finalize-display-proof.sh && rg -q 'REQUIRE_COMPLETE=\"[$]CAPTURE_REQUIRE_COMPLETE\"' os/hardware-gate/capture-harness/finalize-display-proof.sh && rg -q 'requires a complete signoff row' os/hardware-gate/close-signoff.sh"
check "aarch64 local capture imports exact native Linux packaging proof" "rg -q 'goblins-os-native-packaging-gate-v1' .github/workflows/aarch64-verification-iso.yml os/hardware-gate/capture-harness/run-capture.sh os/hardware-gate/close-signoff.sh os/hardware-gate/verify-shipping-status.sh && rg -q 'native_packaging_gate_proof_passes' os/hardware-gate/close-signoff.sh os/hardware-gate/verify-shipping-status.sh && rg -q 'native-packaging-gate.json' .github/workflows/aarch64-verification-iso.yml os/hardware-gate/capture-harness/run-capture.sh"
check "one canonical aarch64 signoff row is staged, fully verified, and atomically composed" "test -f os/hardware-gate/compose-signoff-rows.sh && rg -q '<aarch64-signoff-row[.]md>' os/hardware-gate/compose-signoff-rows.sh && rg -q 'architecture = \"aarch64\"' os/hardware-gate/compose-signoff-rows.sh && ! rg -q 'x86_64|amd64' os/hardware-gate/compose-signoff-rows.sh && rg -q 'SIGNOFF_ROW_OUTPUT' os/hardware-gate/close-signoff.sh os/hardware-gate/capture-harness/run-capture.sh && rg -q 'GOBLINS_OS_SIGNOFF_STAGING_VALIDATE=1' os/hardware-gate/compose-signoff-rows.sh && rg -q 'verify-shipping-status[.]sh' os/hardware-gate/compose-signoff-rows.sh && rg -q 'mv -f -- \"[$]STAGED_SIGNOFF\" \"[$]SIGNOFF_NOTES\"' os/hardware-gate/compose-signoff-rows.sh && rg -q 'notes stayed byte-identical before atomic replacement' os/hardware-gate/compose-signoff-rows.sh"
check "complete close-signoff writes only a staged row and leaves notes untouched" "rg -q 'Complete proof must be staged with SIGNOFF_ROW_OUTPUT' os/hardware-gate/close-signoff.sh && rg -q 'Staged architecture-scoped verification row' os/hardware-gate/close-signoff.sh && rg -q 'Signoff notes remain untouched until compose-signoff-rows validates public media' os/hardware-gate/close-signoff.sh"

AARCH64_CANDIDATE_BOUND=1
CANDIDATE_RUN_AARCH64=""
CANDIDATE_RUN_ATTEMPT_AARCH64=""
for arch in "${ARCHES[@]}"; do
  ISO_PATH="os/iso/output/$arch/bootiso/goblins-os-$arch.iso"
  SHA_PATH="$ISO_PATH.sha256"
  MANIFEST_PATH="os/iso/output/$arch/manifest-goblins-os-$arch.json"
  BIB_MANIFEST_PATH="os/iso/output/$arch/manifest-anaconda-iso.json"
  BRANDING_PUBLISHER_EVIDENCE="os/iso/output/$arch/installer-branding-publisher-evidence.json"
  SBOM_DIR="os/signoff-proofs/sbom/$arch"
  SBOM_MANIFEST="$SBOM_DIR/release-evidence-manifest.json"
  CARGO_TSV="$SBOM_DIR/cargo-lock-packages.tsv"
  RPM_COMMAND="$SBOM_DIR/rpm-packages.command"
  RPM_TSV="$SBOM_DIR/rpm-packages.tsv"
  CANDIDATE_METADATA="os/signoff-proofs/candidate/$arch/image-ref.json"
  IMAGE_REF=""
  PUBLIC_ISO_SHA=""
  ARCH_MISSING=()

  check_file "$arch ISO artifact exists" "$ISO_PATH" || ARCH_MISSING+=("ISO")
  check_file "$arch ISO SHA256 exists" "$SHA_PATH" || ARCH_MISSING+=("SHA256")
  if [ -f "$ISO_PATH" ] && [ -f "$SHA_PATH" ]; then
    check_sha256_file "$arch ISO SHA256 verifies the canonical public ISO" "$SHA_PATH" "$(basename "$ISO_PATH")" || ARCH_MISSING+=("SHA256 verification")
    PUBLIC_ISO_SHA="$(sha256_of_file "$ISO_PATH" 2>/dev/null || true)"
  fi
  check_file "$arch ISO manifest exists" "$MANIFEST_PATH" || ARCH_MISSING+=("ISO manifest")
  check_file "$arch protected-publisher branding evidence exists" "$BRANDING_PUBLISHER_EVIDENCE" || ARCH_MISSING+=("branding publisher evidence")
  check_file_contains "$arch ISO manifest records architecture" "$MANIFEST_PATH" "\"architecture\": \"$arch\"" || ARCH_MISSING+=("ISO manifest architecture")
  if ! check_file_contains "$arch ISO manifest records selected candidate commit" "$MANIFEST_PATH" "\"candidate_commit\": \"$SELECTED_CANDIDATE_COMMIT\""; then
    ARCH_MISSING+=("ISO candidate commit")
    AARCH64_CANDIDATE_BOUND=0
  fi
  check_file_contains "$arch ISO manifest records ISO name" "$MANIFEST_PATH" "\"iso\": \"bootiso/goblins-os-$arch.iso\"" || ARCH_MISSING+=("ISO manifest artifact")
  check_file_contains "$arch ISO manifest records SHA file" "$MANIFEST_PATH" "\"sha256_file\": \"bootiso/goblins-os-$arch.iso.sha256\"" || ARCH_MISSING+=("ISO manifest SHA")
  check_file_contains "$arch canonical public ISO uses the human-safe installer config" "$MANIFEST_PATH" "\"installer_config\": \"os/iso/config.toml\"" || ARCH_MISSING+=("public installer config")
  IMAGE_REF="$(iso_manifest_image_ref "$MANIFEST_PATH")"
  if image_ref_is_digest_pinned "$IMAGE_REF"; then
    echo "[PASS] $arch ISO manifest pins the builder source image digest"
  else
    echo "[FAIL] $arch ISO manifest builder source image must end in @sha256:<64-hex-digest>"
    FAIL_COUNT=$((FAIL_COUNT + 1))
    ARCH_MISSING+=("ISO manifest digest-pinned builder source")
  fi
  if iso_manifest_release_provenance_passes \
    "$MANIFEST_PATH" \
    "$arch" \
    "$SELECTED_CANDIDATE_COMMIT" \
    "$IMAGE_REF" \
    "$BRANDING_PUBLISHER_EVIDENCE"; then
    echo "[PASS] $arch public ISO records the exact native, pinned builder and Goblins-branding provenance"
  else
    echo "[FAIL] $arch public ISO provenance must use native builders, reviewed tool digests, Goblins branding, and the human-safe config"
    FAIL_COUNT=$((FAIL_COUNT + 1))
    ARCH_MISSING+=("public ISO builder/branding provenance")
  fi
  if installer_branding_tool_publisher_evidence_passes \
    "$BRANDING_PUBLISHER_EVIDENCE" \
    "$MANIFEST_PATH"; then
    echo "[PASS] $arch ISO branding is bound to authenticated source handoff and protected-publisher evidence"
  else
    echo "[FAIL] $arch ISO branding requires exact four-part source metadata, protected-publisher import proof, and matching published bytes"
    FAIL_COUNT=$((FAIL_COUNT + 1))
    ARCH_MISSING+=("authenticated branding publisher evidence")
  fi
  check_file_contains "$arch ISO manifest records installer payload source kind" "$MANIFEST_PATH" "\"installer_payload_source_kind\":" || ARCH_MISSING+=("ISO manifest payload source kind")
  check_file_contains "$arch ISO manifest records nonlocal installer payload source" "$MANIFEST_PATH" "\"installer_payload_source_local_only\": false" || ARCH_MISSING+=("ISO manifest nonlocal payload source")
  check_file_contains "$arch ISO manifest records shippable release mode" "$MANIFEST_PATH" "\"shippable_release\": true" || ARCH_MISSING+=("ISO manifest shippable release")
  check_bib_manifest_payload_ref "$arch BIB manifest uses the exact ISO image digest" "$BIB_MANIFEST_PATH" "$IMAGE_REF" || ARCH_MISSING+=("shippable installer payload ref")
  check_file "$arch release evidence manifest exists" "$SBOM_MANIFEST" || ARCH_MISSING+=("release evidence manifest")
  check_file_contains "$arch release evidence manifest records architecture" "$SBOM_MANIFEST" "\"architecture\": \"$arch\"" || ARCH_MISSING+=("release evidence architecture")
  if ! check_file_contains "$arch release evidence manifest records selected candidate commit" "$SBOM_MANIFEST" "\"candidate_commit\": \"$SELECTED_CANDIDATE_COMMIT\""; then
    ARCH_MISSING+=("release evidence candidate commit")
    AARCH64_CANDIDATE_BOUND=0
  fi
  check_file_contains "$arch release evidence manifest records ISO image digest" "$SBOM_MANIFEST" "\"image_ref\": \"$IMAGE_REF\"" || ARCH_MISSING+=("release evidence image digest")
  check_file_contains "$arch release evidence manifest records digest pin" "$SBOM_MANIFEST" "\"image_digest_pinned\": true" || ARCH_MISSING+=("release evidence digest pin")
  check_file_contains "$arch release evidence manifest records asset provenance" "$SBOM_MANIFEST" "\"asset_provenance\": \"os/release/asset-provenance.toml\"" || ARCH_MISSING+=("release evidence asset provenance")
  check_file_contains "$arch release evidence manifest records third-party notices" "$SBOM_MANIFEST" "\"third_party_notices\": \"os/release/third-party-notices.toml\"" || ARCH_MISSING+=("release evidence third-party notices")
  check_file_contains "$arch release evidence manifest records trademark posture" "$SBOM_MANIFEST" "\"trademark_posture\": \"os/release/trademark-posture.toml\"" || ARCH_MISSING+=("release evidence trademark posture")
  check_file_contains "$arch release evidence manifest records source tree manifest" "$SBOM_MANIFEST" "\"source_tree_manifest\": \"os/release/source-tree-manifest.toml\"" || ARCH_MISSING+=("release evidence source tree manifest")
  check_file "$arch Cargo SBOM package TSV exists" "$CARGO_TSV" || ARCH_MISSING+=("Cargo SBOM TSV")
  check_file "$arch RPM replay command exists" "$RPM_COMMAND" || ARCH_MISSING+=("RPM replay command")
  check_file "$arch RPM SBOM package TSV exists" "$RPM_TSV" || ARCH_MISSING+=("RPM SBOM TSV")
  if goblins_os_release_evidence_hashes_match "$SBOM_DIR"; then
    echo "[PASS] $arch release evidence seals Cargo, RPM, and replay-command bytes with matching SHA256 values"
  else
    echo "[FAIL] $arch release evidence Cargo/RPM inventories or replay-command bytes do not match their manifest SHA256 values"
    FAIL_COUNT=$((FAIL_COUNT + 1))
    ARCH_MISSING+=("sealed release evidence hashes")
  fi
  if [ -f "$RPM_TSV" ]; then
    if rpm_sbom_arch_matches "$RPM_TSV" "$arch"; then
      echo "[PASS] $arch RPM SBOM package architectures match $arch or noarch"
    else
      echo "[FAIL] $arch RPM SBOM package architectures must match $arch or noarch"
      FAIL_COUNT=$((FAIL_COUNT + 1))
      ARCH_MISSING+=("RPM SBOM architecture")
    fi
  fi
  check_file "$arch candidate workflow metadata exists" "$CANDIDATE_METADATA" || ARCH_MISSING+=("candidate workflow metadata")
  if candidate_artifact_metadata_passes \
    "$CANDIDATE_METADATA" \
    "$arch" \
    "$SELECTED_CANDIDATE_COMMIT" \
    "$IMAGE_REF" \
    "$PUBLIC_ISO_SHA"; then
    echo "[PASS] $arch public ISO SHA is bound to an exact successful candidate workflow"
    CANDIDATE_RUN_AARCH64="$(candidate_artifact_workflow_run "$CANDIDATE_METADATA")"
    CANDIDATE_RUN_ATTEMPT_AARCH64="$(candidate_artifact_workflow_attempt "$CANDIDATE_METADATA")"
  else
    echo "[FAIL] $arch candidate metadata must bind this public ISO SHA, commit, digest, and exact-candidate gates to one workflow run"
    FAIL_COUNT=$((FAIL_COUNT + 1))
    ARCH_MISSING+=("candidate workflow provenance")
    AARCH64_CANDIDATE_BOUND=0
  fi

  SIGNED_SCREENSHOT_RUN="$(signoff_screenshot_run_for_arch "$arch" || true)"
  if [ -n "$SIGNED_SCREENSHOT_RUN" ]; then
    echo "[PASS] $arch has complete hardware-gate screenshots bound to its signoff row: $SIGNED_SCREENSHOT_RUN"
    print_verification_and_public_release_iso_detail "$SIGNED_SCREENSHOT_RUN" "$arch"
    echo "[PASS] $arch has complete signoff row bound to that screenshot run"
  else
    echo "[FAIL] $arch has no complete hardware-gate screenshot run bound to a complete signoff row"
    print_latest_incomplete_screenshot_run "$SCREENSHOT_ROOT/$arch" "$arch"
    FAIL_COUNT=$((FAIL_COUNT + 1))
    ARCH_MISSING+=("signoff-bound complete screenshot run")
    echo "[FAIL] $arch has no complete signoff row bound to candidate $SELECTED_CANDIDATE_COMMIT with runner, ISO, verify/self-test, SBOM, runtime, gaming, and install-storage proof"
    FAIL_COUNT=$((FAIL_COUNT + 1))
    ARCH_MISSING+=("complete signoff row")
    AARCH64_CANDIDATE_BOUND=0
  fi

  if [ "${#ARCH_MISSING[@]}" -eq 0 ]; then
    echo "[PASS] $arch architecture track complete"
  else
    echo "[FAIL] $arch architecture track missing: ${ARCH_MISSING[*]}"
    FAIL_COUNT=$((FAIL_COUNT + 1))
  fi
done

if [ -n "$CANDIDATE_RUN_AARCH64" ] \
  && [[ "$CANDIDATE_RUN_ATTEMPT_AARCH64" =~ ^[1-9][0-9]*$ ]]; then
  echo "[PASS] The aarch64 public media comes from candidate run $CANDIDATE_RUN_AARCH64 attempt $CANDIDATE_RUN_ATTEMPT_AARCH64"
else
  echo "[FAIL] The aarch64 public media must come from one exact candidate workflow run and attempt"
  FAIL_COUNT=$((FAIL_COUNT + 1))
  AARCH64_CANDIDATE_BOUND=0
fi

if [ "$CANDIDATE_SELECTION_VALID" -eq 1 ] && [ "$AARCH64_CANDIDATE_BOUND" -eq 1 ]; then
  echo "[PASS] The aarch64 proof track is bound to candidate commit $SELECTED_CANDIDATE_COMMIT"
else
  echo "[FAIL] Stable promotion requires the aarch64 ISO, release evidence, screenshot proof, and signoff row all bound to one selected 40-hex candidate commit"
  FAIL_COUNT=$((FAIL_COUNT + 1))
fi

print_legacy_screenshot_roots

if [ -f "$SIGNOFF" ]; then
  RUN_LINE="$(rg -n "^## Manual Gate Run:" "$SIGNOFF" | tail -n1 | cut -d: -f1 || true)"
  if [ -n "$RUN_LINE" ]; then
    LATEST_RUN_BLOCK="$(awk -v start="$RUN_LINE" 'NR < start { next } NR == start { print; next } /^## / { exit } { print }' "$SIGNOFF")"
    if echo "$LATEST_RUN_BLOCK" | rg -q "^- Runner: .+"; then
      echo "[PASS] Latest signoff run has Runner"
    else
      echo "[FAIL] Latest signoff run missing Runner"
      FAIL_COUNT=$((FAIL_COUNT + 1))
    fi
    if echo "$LATEST_RUN_BLOCK" | rg -q "^- Architecture: aarch64$"; then
      echo "[PASS] Latest signoff run has canonical aarch64 architecture"
    else
      echo "[FAIL] Latest signoff run missing architecture"
      FAIL_COUNT=$((FAIL_COUNT + 1))
    fi
    if signoff_block_candidate_commit_matches "$LATEST_RUN_BLOCK"; then
      echo "[PASS] Latest signoff run and its release evidence are bound to candidate $SELECTED_CANDIDATE_COMMIT"
    else
      echo "[FAIL] Latest signoff run, ISO manifest, SBOM manifest, and screenshot proof do not all match the selected candidate commit"
      FAIL_COUNT=$((FAIL_COUNT + 1))
    fi
    if signoff_block_image_ref_matches "$LATEST_RUN_BLOCK"; then
      echo "[PASS] Latest signoff run, ISO, SBOM, and screenshot proof share one immutable image digest"
    else
      echo "[FAIL] Latest signoff run, ISO, SBOM, and screenshot proof do not share one immutable image digest"
      FAIL_COUNT=$((FAIL_COUNT + 1))
    fi
    if echo "$LATEST_RUN_BLOCK" | rg -q "^- Verify result \(blocked=0\): pass"; then
      echo "[PASS] Latest signoff run recorded blocked=0 pass"
    else
      echo "[FAIL] Latest signoff run missing/does not record blocked=0 pass"
      FAIL_COUNT=$((FAIL_COUNT + 1))
    fi
    if echo "$LATEST_RUN_BLOCK" | rg -q "^- Self-test result: pass"; then
      echo "[PASS] Latest signoff run recorded self-test pass"
    else
      echo "[FAIL] Latest signoff run missing/does not record self-test pass"
      FAIL_COUNT=$((FAIL_COUNT + 1))
    fi
    if signoff_block_verification_iso_binding_matches "$LATEST_RUN_BLOCK"; then
      echo "[PASS] Latest signoff run binds the verification ISO artifact and SHA to its screenshot proof"
    else
      echo "[FAIL] Latest signoff run does not bind the verification ISO artifact and SHA to its screenshot proof"
      FAIL_COUNT=$((FAIL_COUNT + 1))
    fi
    if signoff_block_public_release_iso_binding_matches "$LATEST_RUN_BLOCK"; then
      echo "[PASS] Latest signoff run binds the public release ISO to exact candidate artifact metadata"
    else
      echo "[FAIL] Latest signoff run does not bind the public release ISO to exact candidate artifact metadata"
      FAIL_COUNT=$((FAIL_COUNT + 1))
    fi
    if signoff_block_has_real_field "$LATEST_RUN_BLOCK" "^  - mode: .+"; then
      echo "[PASS] Latest signoff run records real runtime engine mode"
    else
      echo "[FAIL] Latest signoff run missing real runtime engine mode"
      FAIL_COUNT=$((FAIL_COUNT + 1))
    fi
    if signoff_block_has_real_field "$LATEST_RUN_BLOCK" "^  - engine source: .+"; then
      echo "[PASS] Latest signoff run records real runtime engine source"
    else
      echo "[FAIL] Latest signoff run missing real runtime engine source"
      FAIL_COUNT=$((FAIL_COUNT + 1))
    fi
    if signoff_block_has_real_field "$LATEST_RUN_BLOCK" "^  - built artifact path/URL: .+"; then
      echo "[PASS] Latest signoff run has real built artifact proof"
    else
      echo "[FAIL] Latest signoff run missing real built artifact proof"
      FAIL_COUNT=$((FAIL_COUNT + 1))
    fi
    if echo "$LATEST_RUN_BLOCK" | rg -q "^- Motion/interactions checked: yes"; then
      echo "[PASS] Latest signoff run records motion/interaction proof"
    else
      echo "[FAIL] Latest signoff run missing motion/interaction proof"
      FAIL_COUNT=$((FAIL_COUNT + 1))
    fi
    if echo "$LATEST_RUN_BLOCK" | rg -q "^- Hosted-context review light/dark checked: yes"; then
      echo "[PASS] Latest signoff run records hosted-context review light/dark display proof"
    else
      echo "[FAIL] Latest signoff run missing hosted-context review light/dark display proof"
      FAIL_COUNT=$((FAIL_COUNT + 1))
    fi
    if echo "$LATEST_RUN_BLOCK" | rg -q "^- Preview open/render checked: yes"; then
      echo "[PASS] Latest signoff run records Preview open/render proof"
    else
      echo "[FAIL] Latest signoff run missing Preview open/render proof"
      FAIL_COUNT=$((FAIL_COUNT + 1))
    fi
    if echo "$LATEST_RUN_BLOCK" | rg -q "^- Text Shortcuts candidate bubble render screenshot checked: yes"; then
      echo "[PASS] Latest signoff run records Text Shortcuts candidate bubble render screenshot proof"
    else
      echo "[FAIL] Latest signoff run missing Text Shortcuts candidate bubble render screenshot proof"
      FAIL_COUNT=$((FAIL_COUNT + 1))
    fi
    if echo "$LATEST_RUN_BLOCK" | rg -q "^- Focus arm roundtrip checked: yes"; then
      echo "[PASS] Latest signoff run records Focus arm roundtrip proof"
    else
      echo "[FAIL] Latest signoff run missing Focus arm roundtrip proof"
      FAIL_COUNT=$((FAIL_COUNT + 1))
    fi
    if echo "$LATEST_RUN_BLOCK" | rg -q "^- App privacy revoke checked: yes"; then
      echo "[PASS] Latest signoff run records App privacy revoke proof"
    else
      echo "[FAIL] Latest signoff run missing App privacy revoke proof"
      FAIL_COUNT=$((FAIL_COUNT + 1))
    fi
    if echo "$LATEST_RUN_BLOCK" | rg -q "^- Gaming readiness checked: yes"; then
      echo "[PASS] Latest signoff run records gaming readiness proof"
    else
      echo "[FAIL] Latest signoff run missing gaming readiness proof"
      FAIL_COUNT=$((FAIL_COUNT + 1))
    fi
    if echo "$LATEST_RUN_BLOCK" | rg -q "^- Install storage/bootloader/dual-boot checked: yes"; then
      echo "[PASS] Latest signoff run records install storage/dual-boot proof"
    else
      echo "[FAIL] Latest signoff run missing install storage/dual-boot proof"
      FAIL_COUNT=$((FAIL_COUNT + 1))
    fi
    if echo "$LATEST_RUN_BLOCK" | rg -q "^- Release evidence/SBOM checked: yes"; then
      echo "[PASS] Latest signoff run records release evidence/SBOM proof"
    else
      echo "[FAIL] Latest signoff run missing release evidence/SBOM proof"
      FAIL_COUNT=$((FAIL_COUNT + 1))
    fi
    if echo "$LATEST_RUN_BLOCK" | rg -qi "Screenshot dir: no fresh|stale screenshot|stale for this ISO|No fresh .*screenshots"; then
      echo "[FAIL] Latest signoff run records stale or missing current screenshot proof"
      FAIL_COUNT=$((FAIL_COUNT + 1))
    else
      echo "[PASS] Latest signoff run does not record stale/missing current screenshot proof"
    fi
    if signoff_block_required_proof_is_complete "$LATEST_RUN_BLOCK"; then
      if echo "$LATEST_RUN_BLOCK" | rg -q "^- Current project completion status: complete$"; then
        echo "[PASS] Latest signoff run completion status matches complete proof"
      else
        echo "[FAIL] Latest signoff run has complete proof but does not declare completion"
        FAIL_COUNT=$((FAIL_COUNT + 1))
      fi
    elif echo "$LATEST_RUN_BLOCK" | rg -q "^- Current project completion status: complete"; then
      echo "[FAIL] Latest signoff run declares completion before required proof is present"
      FAIL_COUNT=$((FAIL_COUNT + 1))
    else
      echo "[PASS] Latest signoff run does not claim completion with incomplete proof"
    fi
  else
    echo "[FAIL] No Manual Gate Run sections found in signoff notes"
    FAIL_COUNT=$((FAIL_COUNT + 1))
  fi
else
  echo "[FAIL] Signoff notes file missing"
  FAIL_COUNT=$((FAIL_COUNT + 1))
fi

if [ -n "$SCREENSHOT_RUN_DIR" ]; then
  if [ -d "$SCREENSHOT_RUN_DIR" ]; then
    LATEST_RUN="$SCREENSHOT_RUN_DIR"
    echo "Verifying provided screenshot run: $LATEST_RUN"
    if ! print_screenshot_run_checks "$LATEST_RUN"; then
      fail_check "Hardware-gate screenshot proof is incomplete"
    fi
  else
    fail_check "Provided SCREENSHOT_RUN_DIR not found: $SCREENSHOT_RUN_DIR"
  fi
else
  if [ -d "$SCREENSHOT_ROOT" ]; then
    LATEST_RUN=""
    while IFS= read -r candidate; do
      if screenshot_run_is_complete "$candidate"; then
        LATEST_RUN="$candidate"
        break
      fi
    done < <(find "$SCREENSHOT_ROOT" -mindepth 2 -maxdepth 2 -type d | sort -r)

    if [ -n "$LATEST_RUN" ]; then
      echo "Latest complete hardware-gate screenshot run: $LATEST_RUN"
      if ! print_screenshot_run_checks "$LATEST_RUN"; then
        fail_check "Hardware-gate screenshot proof is incomplete"
      fi
    else
      fail_check "No complete hardware-gate screenshot run found under $SCREENSHOT_ROOT"
      for arch in "${ARCHES[@]}"; do
        print_latest_incomplete_screenshot_run "$SCREENSHOT_ROOT/$arch" "$arch"
      done
    fi
  else
    fail_check "Screenshot root missing: $SCREENSHOT_ROOT"
    print_latest_incomplete_screenshot_run "$SCREENSHOT_ROOT" "hardware-gate"
  fi
fi

echo
for arch in "${ARCHES[@]}"; do
  print_arch_next_steps "$arch"
done

echo
echo "Run ./os/hardware-gate/close-signoff.sh on Linux to generate a full verified status row with verify/self-test results."
echo "Use SCREENSHOT_RUN_DIR or SCREENSHOT_DIR to validate screenshot completeness."

if [ "${FAIL_COUNT:-0}" -ne 0 ]; then
  echo "Shipping status gate: FAIL"
  exit 1
fi

echo "Shipping status gate: PASS"
exit 0
