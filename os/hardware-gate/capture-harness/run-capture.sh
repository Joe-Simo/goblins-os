#!/usr/bin/env bash
# Drive the full hardware-gate capture in a display-backed qemu VM and close-signoff.
#
# Boots the hardware-gate ISO built with os/iso/verify-config.toml (so the
# embedded /osbuild.ks, not a sidecar disk, drives Anaconda), waits for the bootc
# install + first-boot GDM-autologin desktop, completes first boot through the
# same core API contracts as the private/offline UI path, publishes the
# in-session orchestrator for the verification-only user service, captures
# the 32 required shots by QMP-screendump on each HTTP signal,
# writes proof-manifest.json, and runs close-signoff.sh.
#
# Honest: every shot is a real framebuffer capture of the real installed OS.
# Gaming uses the OS's own lavapipe/gamescope/pipewire stack; studio-live uses a
# host-served model over 10.0.2.2. Works on a native Linux/KVM host (CI) and on
# macOS/hvf. KVM is required for x86_64 at usable speed; aarch64 also runs on hvf.
set -euo pipefail

ARCH="${GOBLINS_OS_ARCH:-$(uname -m)}"
case "$ARCH" in arm64|aarch64) ARCH=aarch64; QEMU=qemu-system-aarch64;; x86_64|amd64) ARCH=x86_64; QEMU=qemu-system-x86_64;; *) echo "unsupported arch $ARCH"; exit 2;; esac
REPO="${REPO_ROOT:-$(pwd)}"
ISO="${GOBLINS_OS_CAPTURE_ISO:-$REPO/os/iso/output/$ARCH/bootiso/goblins-os-$ARCH.iso}"
SHA_FILE="${GOBLINS_OS_CAPTURE_ISO_SHA256:-$ISO.sha256}"
BASE_WORK="${WORK_DIR:-/tmp/gos-hwgate-$ARCH}"
WORK="$BASE_WORK"
PORT="${HTTP_PORT:-8099}"
DATE="${RUN_DATE:?set RUN_DATE=YYYY-MM-DD (scripts cannot read the clock)}"
RUN_DIR="$REPO/os/screenshots/hardware-gate/$ARCH/$DATE"
HERE="$(cd "$(dirname "$0")" && pwd)"
HTTPD=""
QEMU_PID=""
CAPTURE_STARTED=0

dump_file_tail() {
  local label="$1"
  local path="$2"
  if [ -s "$path" ]; then
    echo "---- $label: $path ----"
    tail -n 200 "$path" || true
  else
    echo "---- $label missing or empty: $path ----"
  fi
}

copy_capture_logs() {
  local suffix="${1:-}"
  local target="$RUN_DIR/_capture-logs"
  if [ -n "$suffix" ]; then
    target="$target/$suffix"
  fi
  mkdir -p "$target"
  local name
  for name in qemu.log serial.log httpd.log; do
    if [ -e "$WORK/$name" ]; then
      cp -f "$WORK/$name" "$target/$name" || true
    fi
  done
}

dump_capture_logs() {
  copy_capture_logs
  echo "QEMU startup diagnostics"
  command -v "$QEMU" >/dev/null 2>&1 && "$QEMU" --version | head -n 1 || true
  [ -e /dev/kvm ] && ls -l /dev/kvm || true
  [ -n "${QEMU_PID:-}" ] && ps -p "$QEMU_PID" -o pid,stat,etime,command || true
  [ -S "$WORK/qmp.sock" ] && echo "QMP socket exists: $WORK/qmp.sock" || echo "QMP socket missing: $WORK/qmp.sock"
  dump_file_tail "qemu.log" "$WORK/qemu.log"
  dump_file_tail "serial.log" "$WORK/serial.log"
  dump_file_tail "httpd.log" "$WORK/httpd.log"
}

cleanup() {
  local rc=$?
  if [ "$rc" -ne 0 ] && [ "${CAPTURE_STARTED:-0}" = "1" ]; then
    dump_capture_logs
  fi
  [ -n "${QEMU_PID:-}" ] && kill "$QEMU_PID" 2>/dev/null || true
  [ -n "${HTTPD:-}" ] && kill "$HTTPD" 2>/dev/null || true
}
trap cleanup EXIT

[ -f "$ISO" ] || { echo "missing ISO $ISO"; exit 1; }
[ -f "$SHA_FILE" ] || { echo "missing ISO SHA256 file $SHA_FILE"; exit 1; }

require_verification_iso() {
  local missing=0
  local needle
  for needle in \
    "GOBLINS_VERIFY_INSTALL_DONE" \
    "ignoredisk --only-use=vda" \
    "goblins-hwgate-session-orchestrator"; do
    if ! LC_ALL=C grep -aFq "$needle" "$ISO"; then
      echo "verification ISO guard: missing $needle in $ISO" >&2
      missing=1
    fi
  done
  if [ "$missing" -ne 0 ]; then
    cat >&2 <<EOF
capture harness requires the verification-only hardware-gate ISO built with:
  GOBLINS_OS_ISO_CONFIG=os/iso/verify-config.toml

The public release ISO is intentionally human-safe and leaves storage
interactive, so it cannot satisfy automated display-backed proof. Build or
point GOBLINS_OS_CAPTURE_ISO at the verification ISO generated from the real
pullable release bootc image.
EOF
    exit 2
  fi
}

require_verification_iso
ISO_SHA="$(awk '{print $1; exit}' "$SHA_FILE")"

# Accel: KVM on Linux, HVF on macOS.
case "$(uname -s)" in
  Linux)
    if [ ! -e /dev/kvm ] || [ ! -r /dev/kvm ] || [ ! -w /dev/kvm ]; then
      echo "native Linux capture requires readable/writable /dev/kvm for QEMU/KVM" >&2
      [ -e /dev/kvm ] && ls -l /dev/kvm >&2 || true
      exit 1
    fi
    ACCEL=kvm
    CPU=host
    ;;
  Darwin)
    ACCEL=hvf
    CPU=host
    ;;
  *)
    echo "unsupported capture host $(uname -s): need native Linux/KVM or macOS/HVF" >&2
    exit 1
    ;;
esac
QEMU_SMP="${GOBLINS_OS_QEMU_CPUS:-2}"
SCRATCH_DISK_SIZE="${GOBLINS_OS_CAPTURE_DISK_SIZE:-80G}"
pick() { for f in "$@"; do [ -n "$f" ] && [ -f "$f" ] && { echo "$f"; return 0; }; done; return 1; }
VARS_TEMPLATE=""
if [ "$ARCH" = aarch64 ]; then
  MACHINE="virt,accel=$ACCEL,gic-version=max"
  CODE="$(pick "${AARCH64_UEFI_CODE:-}" /opt/homebrew/share/qemu/edk2-aarch64-code.fd /usr/share/AAVMF/AAVMF_CODE.fd /usr/share/edk2/aarch64/QEMU_EFI-silent.fd)"
  VARS_TEMPLATE="$(pick "${AARCH64_UEFI_VARS:-}" /usr/share/AAVMF/AAVMF_VARS.fd || true)"  # empty 64M also works on edk2-aarch64
else
  MACHINE="q35,accel=$ACCEL"
  CODE="$(pick "${X86_UEFI_CODE:-}" /usr/share/OVMF/OVMF_CODE_4M.fd /usr/share/OVMF/OVMF_CODE.fd /usr/share/edk2/ovmf/OVMF_CODE.fd)"
  # x86_64 OVMF requires a real VARS template matching the code build (4M code -> 4M vars).
  case "$CODE" in
    *_4M.fd) VARS_TEMPLATE="$(pick /usr/share/OVMF/OVMF_VARS_4M.fd /usr/share/edk2/ovmf/OVMF_VARS.fd)";;
    *)       VARS_TEMPLATE="$(pick /usr/share/OVMF/OVMF_VARS.fd /usr/share/edk2/ovmf/OVMF_VARS.fd)";;
  esac
fi
[ -n "$CODE" ] || { echo "no UEFI code firmware found for $ARCH"; exit 1; }
PFLASH=(-drive "if=pflash,format=raw,file=$WORK/code.fd,readonly=on" -drive "if=pflash,format=raw,file=$WORK/vars.fd")
QEMU_AUDIO=(-audiodev none,id=audio0 -device ich9-intel-hda -device hda-output,audiodev=audio0)

case "$RUN_DIR" in
  "$REPO"/os/screenshots/hardware-gate/"$ARCH"/*)
    rm -rf "$RUN_DIR"
    ;;
  *)
    echo "refusing to reset unexpected hardware-gate run dir: $RUN_DIR"
    exit 2
    ;;
esac
mkdir -p "$WORK" "$RUN_DIR"

stop_qemu() {
  if [ -n "${QEMU_PID:-}" ]; then
    kill "$QEMU_PID" 2>/dev/null || true
    wait "$QEMU_PID" 2>/dev/null || true
    QEMU_PID=""
  fi
}

prepare_vm_state() {
  local attempt="$1"
  echo "capture attempt $attempt: preparing fresh VM state"
  rm -f "$WORK/qmp.sock" "$WORK/serial.log" "$WORK/qemu.log" "$WORK/scratch.qcow2" "$WORK/orchestrator.sh"
  cp "$CODE" "$WORK/code.fd"
  if [ -n "$VARS_TEMPLATE" ]; then
    cp "$VARS_TEMPLATE" "$WORK/vars.fd"
  else
    : > "$WORK/vars.fd"; truncate -s 67108864 "$WORK/vars.fd" 2>/dev/null || dd if=/dev/zero of="$WORK/vars.fd" bs=1m count=64 2>/dev/null
  fi
  qemu-img create -f qcow2 "$WORK/scratch.qcow2" "$SCRATCH_DISK_SIZE" >/dev/null
}

start_qemu() {
  local attempt="$1"
  echo "capture attempt $attempt: starting QEMU"
  "$QEMU" -machine "$MACHINE" -cpu "$CPU" -smp "$QEMU_SMP" -m 5120 "${PFLASH[@]}" \
    -cdrom "$ISO" -drive "file=$WORK/scratch.qcow2,if=virtio,format=qcow2" \
    -boot d \
    -netdev user,id=net0 -device virtio-net-pci,netdev=net0 \
    -device virtio-gpu-pci,id=video0 -device qemu-xhci -device usb-tablet -device usb-kbd \
    "${QEMU_AUDIO[@]}" \
    -serial file:"$WORK/serial.log" -display none -qmp "unix:$WORK/qmp.sock,server,nowait" >"$WORK/qemu.log" 2>&1 &
  QEMU_PID=$!
  CAPTURE_STARTED=1
  export GOS_QMP="$WORK/qmp.sock" GOS_SERIALLOG="$WORK/serial.log" GOS_HTTPLOG="$WORK/httpd.log" GOS_OUTDIR="$RUN_DIR" GOS_PORT="$PORT" GOS_QMP_DISPLAY_DEVICE=video0
  export GOS_ORCHESTRATOR_SOURCE="$HERE/in-session-orchestrator.sh" GOS_ORCHESTRATOR_DEST="$WORK/orchestrator.sh"
}

run_driver() {
  set +e
  python3 "$HERE/drive-capture.py"
  local driver_rc=$?
  set -e
  return "$driver_rc"
}

# Serve first-boot helper and receive capture signals. The orchestrator is
# published by drive-capture.py only after the host has recorded the post-unlock
# log offset, so early /ready signals cannot race ahead of the screenshot tailer.
rm -f "$WORK/orchestrator.sh"
( cd "$WORK" \
    && sed "s/@GOS_PORT@/$PORT/g" "$HERE/firstboot-unlock.sh" > firstboot-unlock.sh \
    && python3 -m http.server "$PORT" --bind 0.0.0.0 >"$WORK/httpd.log" 2>&1 ) &
HTTPD=$!

# Phase the run with the QMP driver (waits for Anaconda, drives it, waits for the
# desktop, dismisses onboarding, launches the orchestrator, captures on signals).
MAX_ATTEMPTS="${GOS_CAPTURE_MAX_ATTEMPTS:-2}"
INSTALL_TIMEOUT_RC="${GOS_INSTALL_POST_TIMEOUT_EXIT:-70}"
attempt=1
while [ "$attempt" -le "$MAX_ATTEMPTS" ]; do
  prepare_vm_state "$attempt"
  start_qemu "$attempt"
  if run_driver; then
    break
  else
    driver_rc=$?
  fi
  copy_capture_logs "attempt-$attempt"
  if [ "$driver_rc" -eq "$INSTALL_TIMEOUT_RC" ] && [ "$attempt" -lt "$MAX_ATTEMPTS" ]; then
    echo "capture attempt $attempt stalled before kickstart marker; retrying with fresh VM state"
    stop_qemu
    attempt=$((attempt + 1))
    continue
  fi
  exit "$driver_rc"
done

FIREWALL_PROOF="$RUN_DIR/firewall-live-toggle-proof.json"
TEXT_SHORTCUTS_PROOF="$RUN_DIR/text-shortcuts-session-enable-proof.json"
TEXT_SHORTCUTS_CANDIDATE_PROOF="$RUN_DIR/text-shortcuts-candidate-metadata-proof.json"
TEXT_SHORTCUTS_OVERLAY_INTENT_PROOF="$RUN_DIR/text-shortcuts-overlay-intent-proof.json"
TEXT_SHORTCUTS_CANDIDATE_BUBBLE_FRAME_PROOF="$RUN_DIR/text-shortcuts-candidate-bubble-frame-proof.json"
TEXT_SHORTCUTS_CANDIDATE_BUBBLE_LAYOUT_PROOF="$RUN_DIR/text-shortcuts-candidate-bubble-layout-proof.json"
TEXT_SHORTCUTS_CANDIDATE_BUBBLE_RENDER_INTENT_PROOF="$RUN_DIR/text-shortcuts-candidate-bubble-render-intent-proof.json"
TEXT_SHORTCUTS_CANDIDATE_BUBBLE_RENDER_PROOF="$RUN_DIR/text-shortcuts-candidate-bubble-render-proof.json"
TEXT_SHORTCUTS_LIVE_IBUS_RUNTIME_RENDER_PROOF="$RUN_DIR/text-shortcuts-live-ibus-runtime-render-proof.json"
KEYBOARD_SHORTCUTS_ROUNDTRIP_PROOF="$RUN_DIR/keyboard-shortcuts-roundtrip-proof.json"
INPUT_SOURCES_ROUNDTRIP_PROOF="$RUN_DIR/input-sources-roundtrip-proof.json"
MULTI_DISPLAY_APPLY_PROOF="$RUN_DIR/multi-display-apply-proof.json"
FOCUS_ARM_ROUNDTRIP_PROOF="$RUN_DIR/focus-arm-roundtrip-proof.json"
APP_PRIVACY_REVOKE_PROOF="$RUN_DIR/app-privacy-revoke-proof.json"
PREVIEW_OPEN_RENDER_PROOF="$RUN_DIR/preview-open-render-proof.json"
RUNTIME_BUILD_PROOF="$RUN_DIR/runtime-build-proof.json"
if ! grep -Fq '"status": "pass"' "$FIREWALL_PROOF" \
  || ! grep -Fq '"disable_http": "200"' "$FIREWALL_PROOF" \
  || ! grep -Fq '"disable_active": "false"' "$FIREWALL_PROOF" \
  || ! grep -Fq '"enable_http": "200"' "$FIREWALL_PROOF" \
  || ! grep -Fq '"enable_active": "true"' "$FIREWALL_PROOF"; then
  echo "HONESTY GUARD: missing or failing live firewall toggle proof at $FIREWALL_PROOF"
  exit 4
fi
if ! grep -Fq '"status": "pass"' "$TEXT_SHORTCUTS_PROOF" \
  || ! grep -Fq '"service": "active"' "$TEXT_SHORTCUTS_PROOF" \
  || ! grep -Fq '"service_unit": "org.freedesktop.IBus.session.GNOME.service"' "$TEXT_SHORTCUTS_PROOF" \
  || ! grep -Fq '"input_source_configured": "true"' "$TEXT_SHORTCUTS_PROOF" \
  || ! grep -Fq '"preload_configured": "true"' "$TEXT_SHORTCUTS_PROOF" \
  || ! grep -Fq '"engine_listed": "true"' "$TEXT_SHORTCUTS_PROOF" \
  || ! grep -Fq '"active_engine": "goblins-textshortcuts"' "$TEXT_SHORTCUTS_PROOF" \
  || ! grep -Fq '"adapter_self_test": "pass"' "$TEXT_SHORTCUTS_PROOF" \
  || ! grep -Fq '"core_http": "200"' "$TEXT_SHORTCUTS_PROOF" \
  || ! grep -Fq '"core_engine_available": "true"' "$TEXT_SHORTCUTS_PROOF" \
  || ! grep -Fq '"core_runtime_loop_available": "true"' "$TEXT_SHORTCUTS_PROOF" \
  || ! grep -Fq '"runtime_ready_claim": "true"' "$TEXT_SHORTCUTS_PROOF"; then
  echo "HONESTY GUARD: missing or failing Text Shortcuts session-enable proof at $TEXT_SHORTCUTS_PROOF"
  exit 4
fi
if ! grep -Fq '"status": "pass"' "$RUNTIME_BUILD_PROOF" \
  || ! grep -Fq '"route": "/v1/apps/builds"' "$RUNTIME_BUILD_PROOF" \
  || ! grep -Fq '"engine_mode": "local-model"' "$RUNTIME_BUILD_PROOF" \
  || ! rg -q '"engine_source"[[:space:]]*:[[:space:]]*"[A-Za-z0-9._:-]+-built"' "$RUNTIME_BUILD_PROOF"; then
  echo "HONESTY GUARD: missing or failing runtime app-build proof at $RUNTIME_BUILD_PROOF"
  exit 4
fi
if ! grep -Fq '"status": "pass"' "$TEXT_SHORTCUTS_CANDIDATE_PROOF" \
  || ! grep -Fq '"surface": "goblins-os-shell-text-shortcuts-candidate-proof"' "$TEXT_SHORTCUTS_CANDIDATE_PROOF" \
  || ! grep -Fq '"candidate_replacement": "on my way"' "$TEXT_SHORTCUTS_CANDIDATE_PROOF" \
  || ! grep -Fq '"candidate_accept_on": "word-boundary"' "$TEXT_SHORTCUTS_CANDIDATE_PROOF" \
  || ! grep -Fq '"candidate_dismiss_key": "Escape"' "$TEXT_SHORTCUTS_CANDIDATE_PROOF" \
  || ! grep -Fq '"rendered_bubble_ready_claim": "false"' "$TEXT_SHORTCUTS_CANDIDATE_PROOF" \
  || ! grep -Fq '"live_overlay_claim": "false"' "$TEXT_SHORTCUTS_CANDIDATE_PROOF" \
  || ! grep -Fq '"runtime_ready_claim": "false"' "$TEXT_SHORTCUTS_CANDIDATE_PROOF"; then
  echo "HONESTY GUARD: missing or failing Text Shortcuts candidate metadata proof at $TEXT_SHORTCUTS_CANDIDATE_PROOF"
  exit 4
fi
if ! grep -Fq '"status": "pass"' "$TEXT_SHORTCUTS_OVERLAY_INTENT_PROOF" \
  || ! grep -Fq '"surface": "goblins-textshortcuts-ibus-adapter-overlay-intent"' "$TEXT_SHORTCUTS_OVERLAY_INTENT_PROOF" \
  || ! grep -Fq '"adapter_self_test": "pass"' "$TEXT_SHORTCUTS_OVERLAY_INTENT_PROOF" \
  || ! grep -Fq '"show_count": "2"' "$TEXT_SHORTCUTS_OVERLAY_INTENT_PROOF" \
  || ! grep -Fq '"hide_count": "2"' "$TEXT_SHORTCUTS_OVERLAY_INTENT_PROOF" \
  || ! grep -Fq '"dismissed_reason": "true"' "$TEXT_SHORTCUTS_OVERLAY_INTENT_PROOF" \
  || ! grep -Fq '"committed_reason": "true"' "$TEXT_SHORTCUTS_OVERLAY_INTENT_PROOF" \
  || ! grep -Fq '"rendered_bubble_ready_claim": "false"' "$TEXT_SHORTCUTS_OVERLAY_INTENT_PROOF" \
  || ! grep -Fq '"live_overlay_claim": "false"' "$TEXT_SHORTCUTS_OVERLAY_INTENT_PROOF" \
  || ! grep -Fq '"runtime_ready_claim": "false"' "$TEXT_SHORTCUTS_OVERLAY_INTENT_PROOF"; then
  echo "HONESTY GUARD: missing or failing Text Shortcuts overlay-intent proof at $TEXT_SHORTCUTS_OVERLAY_INTENT_PROOF"
  exit 4
fi
if ! grep -Fq '"status": "pass"' "$TEXT_SHORTCUTS_CANDIDATE_BUBBLE_FRAME_PROOF" \
  || ! grep -Fq '"surface": "goblins-textshortcuts-accept-bubble-frame"' "$TEXT_SHORTCUTS_CANDIDATE_BUBBLE_FRAME_PROOF" \
  || ! grep -Fq '"adapter_self_test": "pass"' "$TEXT_SHORTCUTS_CANDIDATE_BUBBLE_FRAME_PROOF" \
  || ! grep -Fq '"show_frame_count": "2"' "$TEXT_SHORTCUTS_CANDIDATE_BUBBLE_FRAME_PROOF" \
  || ! grep -Fq '"hide_frame_count": "2"' "$TEXT_SHORTCUTS_CANDIDATE_BUBBLE_FRAME_PROOF" \
  || ! grep -Fq '"dismissed_frame": "true"' "$TEXT_SHORTCUTS_CANDIDATE_BUBBLE_FRAME_PROOF" \
  || ! grep -Fq '"committed_frame": "true"' "$TEXT_SHORTCUTS_CANDIDATE_BUBBLE_FRAME_PROOF" \
  || ! grep -Fq '"replacement": "on my way"' "$TEXT_SHORTCUTS_CANDIDATE_BUBBLE_FRAME_PROOF" \
  || ! grep -Fq '"accept_on": "word-boundary"' "$TEXT_SHORTCUTS_CANDIDATE_BUBBLE_FRAME_PROOF" \
  || ! grep -Fq '"accept_keys": "Space,Return"' "$TEXT_SHORTCUTS_CANDIDATE_BUBBLE_FRAME_PROOF" \
  || ! grep -Fq '"dismiss_key": "Escape"' "$TEXT_SHORTCUTS_CANDIDATE_BUBBLE_FRAME_PROOF" \
  || ! grep -Fq '"style_class": "gos-text-shortcuts-candidate"' "$TEXT_SHORTCUTS_CANDIDATE_BUBBLE_FRAME_PROOF" \
  || ! grep -Fq '"text_style_class": "gos-text-shortcuts-candidate-text"' "$TEXT_SHORTCUTS_CANDIDATE_BUBBLE_FRAME_PROOF" \
  || ! grep -Fq '"hint_style_class": "gos-text-shortcuts-candidate-hint"' "$TEXT_SHORTCUTS_CANDIDATE_BUBBLE_FRAME_PROOF" \
  || ! grep -Fq '"font_family": "Inter"' "$TEXT_SHORTCUTS_CANDIDATE_BUBBLE_FRAME_PROOF" \
  || ! grep -Fq '"sensitive_field_refusal": "true"' "$TEXT_SHORTCUTS_CANDIDATE_BUBBLE_FRAME_PROOF" \
  || ! grep -Fq '"rendered_bubble_ready_claim": "false"' "$TEXT_SHORTCUTS_CANDIDATE_BUBBLE_FRAME_PROOF" \
  || ! grep -Fq '"live_overlay_claim": "false"' "$TEXT_SHORTCUTS_CANDIDATE_BUBBLE_FRAME_PROOF" \
  || ! grep -Fq '"runtime_ready_claim": "false"' "$TEXT_SHORTCUTS_CANDIDATE_BUBBLE_FRAME_PROOF"; then
  echo "HONESTY GUARD: missing or failing Text Shortcuts candidate-bubble-frame proof at $TEXT_SHORTCUTS_CANDIDATE_BUBBLE_FRAME_PROOF"
  exit 4
fi
if ! grep -Fq '"status": "pass"' "$TEXT_SHORTCUTS_CANDIDATE_BUBBLE_LAYOUT_PROOF" \
  || ! grep -Fq '"surface": "goblins-textshortcuts-accept-bubble-layout"' "$TEXT_SHORTCUTS_CANDIDATE_BUBBLE_LAYOUT_PROOF" \
  || ! grep -Fq '"adapter_self_test": "pass"' "$TEXT_SHORTCUTS_CANDIDATE_BUBBLE_LAYOUT_PROOF" \
  || ! grep -Fq '"frame_surface": "goblins-textshortcuts-accept-bubble-frame"' "$TEXT_SHORTCUTS_CANDIDATE_BUBBLE_LAYOUT_PROOF" \
  || ! grep -Fq '"layout_count": "4"' "$TEXT_SHORTCUTS_CANDIDATE_BUBBLE_LAYOUT_PROOF" \
  || ! grep -Fq '"visible_layout_count": "3"' "$TEXT_SHORTCUTS_CANDIDATE_BUBBLE_LAYOUT_PROOF" \
  || ! grep -Fq '"right_edge_clamped": "true"' "$TEXT_SHORTCUTS_CANDIDATE_BUBBLE_LAYOUT_PROOF" \
  || ! grep -Fq '"bottom_edge_flipped": "true"' "$TEXT_SHORTCUTS_CANDIDATE_BUBBLE_LAYOUT_PROOF" \
  || ! grep -Fq '"hidden_frame_collapses": "true"' "$TEXT_SHORTCUTS_CANDIDATE_BUBBLE_LAYOUT_PROOF" \
  || ! grep -Fq '"style_class": "gos-text-shortcuts-candidate"' "$TEXT_SHORTCUTS_CANDIDATE_BUBBLE_LAYOUT_PROOF" \
  || ! grep -Fq '"font_family": "Inter"' "$TEXT_SHORTCUTS_CANDIDATE_BUBBLE_LAYOUT_PROOF" \
  || ! grep -Fq '"rendered_bubble_ready_claim": "false"' "$TEXT_SHORTCUTS_CANDIDATE_BUBBLE_LAYOUT_PROOF" \
  || ! grep -Fq '"live_overlay_claim": "false"' "$TEXT_SHORTCUTS_CANDIDATE_BUBBLE_LAYOUT_PROOF" \
  || ! grep -Fq '"runtime_ready_claim": "false"' "$TEXT_SHORTCUTS_CANDIDATE_BUBBLE_LAYOUT_PROOF"; then
  echo "HONESTY GUARD: missing or failing Text Shortcuts candidate-bubble-layout proof at $TEXT_SHORTCUTS_CANDIDATE_BUBBLE_LAYOUT_PROOF"
  exit 4
fi
if ! grep -Fq '"status": "pass"' "$TEXT_SHORTCUTS_CANDIDATE_BUBBLE_RENDER_INTENT_PROOF" \
  || ! grep -Fq '"surface": "goblins-textshortcuts-accept-bubble-render-intent"' "$TEXT_SHORTCUTS_CANDIDATE_BUBBLE_RENDER_INTENT_PROOF" \
  || ! grep -Fq '"adapter_self_test": "pass"' "$TEXT_SHORTCUTS_CANDIDATE_BUBBLE_RENDER_INTENT_PROOF" \
  || ! grep -Fq '"frame_surface": "goblins-textshortcuts-accept-bubble-frame"' "$TEXT_SHORTCUTS_CANDIDATE_BUBBLE_RENDER_INTENT_PROOF" \
  || ! grep -Fq '"layout_surface": "goblins-textshortcuts-accept-bubble-layout"' "$TEXT_SHORTCUTS_CANDIDATE_BUBBLE_RENDER_INTENT_PROOF" \
  || ! grep -Fq '"render_intent_count": "8"' "$TEXT_SHORTCUTS_CANDIDATE_BUBBLE_RENDER_INTENT_PROOF" \
  || ! grep -Fq '"show_intent_count": "4"' "$TEXT_SHORTCUTS_CANDIDATE_BUBBLE_RENDER_INTENT_PROOF" \
  || ! grep -Fq '"hide_intent_count": "4"' "$TEXT_SHORTCUTS_CANDIDATE_BUBBLE_RENDER_INTENT_PROOF" \
  || ! grep -Fq '"dismissed_intent": "true"' "$TEXT_SHORTCUTS_CANDIDATE_BUBBLE_RENDER_INTENT_PROOF" \
  || ! grep -Fq '"committed_intent": "true"' "$TEXT_SHORTCUTS_CANDIDATE_BUBBLE_RENDER_INTENT_PROOF" \
  || ! grep -Fq '"focus_out_hide": "true"' "$TEXT_SHORTCUTS_CANDIDATE_BUBBLE_RENDER_INTENT_PROOF" \
  || ! grep -Fq '"sensitive_hide": "true"' "$TEXT_SHORTCUTS_CANDIDATE_BUBBLE_RENDER_INTENT_PROOF" \
  || ! grep -Fq '"pass_through_unchanged": "true"' "$TEXT_SHORTCUTS_CANDIDATE_BUBBLE_RENDER_INTENT_PROOF" \
  || ! grep -Fq '"sink_failure_fail_open": "true"' "$TEXT_SHORTCUTS_CANDIDATE_BUBBLE_RENDER_INTENT_PROOF" \
  || ! grep -Fq '"style_class": "gos-text-shortcuts-candidate"' "$TEXT_SHORTCUTS_CANDIDATE_BUBBLE_RENDER_INTENT_PROOF" \
  || ! grep -Fq '"font_family": "Inter"' "$TEXT_SHORTCUTS_CANDIDATE_BUBBLE_RENDER_INTENT_PROOF" \
  || ! grep -Fq '"rendered_bubble_ready_claim": "false"' "$TEXT_SHORTCUTS_CANDIDATE_BUBBLE_RENDER_INTENT_PROOF" \
  || ! grep -Fq '"live_overlay_claim": "false"' "$TEXT_SHORTCUTS_CANDIDATE_BUBBLE_RENDER_INTENT_PROOF" \
  || ! grep -Fq '"runtime_ready_claim": "false"' "$TEXT_SHORTCUTS_CANDIDATE_BUBBLE_RENDER_INTENT_PROOF"; then
  echo "HONESTY GUARD: missing or failing Text Shortcuts candidate-bubble-render-intent proof at $TEXT_SHORTCUTS_CANDIDATE_BUBBLE_RENDER_INTENT_PROOF"
  exit 4
fi
if ! grep -Fq '"status": "pass"' "$TEXT_SHORTCUTS_CANDIDATE_BUBBLE_RENDER_PROOF" \
  || ! grep -Fq '"surface": "goblins-os-shell-text-shortcuts-candidate-bubble-render"' "$TEXT_SHORTCUTS_CANDIDATE_BUBBLE_RENDER_PROOF" \
  || ! grep -Fq '"render_intent_surface": "goblins-textshortcuts-accept-bubble-render-intent"' "$TEXT_SHORTCUTS_CANDIDATE_BUBBLE_RENDER_PROOF" \
  || ! grep -Fq '"layout_surface": "goblins-textshortcuts-accept-bubble-layout"' "$TEXT_SHORTCUTS_CANDIDATE_BUBBLE_RENDER_PROOF" \
  || ! grep -Fq '"frame_surface": "goblins-textshortcuts-accept-bubble-frame"' "$TEXT_SHORTCUTS_CANDIDATE_BUBBLE_RENDER_PROOF" \
  || ! grep -Fq '"screenshot": "31-text-shortcuts-candidate-bubble-render.png"' "$TEXT_SHORTCUTS_CANDIDATE_BUBBLE_RENDER_PROOF" \
  || ! grep -Fq '"rendered_candidate_surface": "true"' "$TEXT_SHORTCUTS_CANDIDATE_BUBBLE_RENDER_PROOF" \
  || ! grep -Fq '"style_class": "gos-text-shortcuts-candidate"' "$TEXT_SHORTCUTS_CANDIDATE_BUBBLE_RENDER_PROOF" \
  || ! grep -Fq '"font_family": "Inter"' "$TEXT_SHORTCUTS_CANDIDATE_BUBBLE_RENDER_PROOF" \
  || ! grep -Fq '"rendered_bubble_ready_claim": "false"' "$TEXT_SHORTCUTS_CANDIDATE_BUBBLE_RENDER_PROOF" \
  || ! grep -Fq '"live_overlay_claim": "false"' "$TEXT_SHORTCUTS_CANDIDATE_BUBBLE_RENDER_PROOF" \
  || ! grep -Fq '"runtime_ready_claim": "false"' "$TEXT_SHORTCUTS_CANDIDATE_BUBBLE_RENDER_PROOF" \
  || [ ! -s "$RUN_DIR/31-text-shortcuts-candidate-bubble-render.png" ]; then
  echo "HONESTY GUARD: missing or failing Text Shortcuts candidate-bubble-render screenshot proof at $TEXT_SHORTCUTS_CANDIDATE_BUBBLE_RENDER_PROOF"
  exit 4
fi
if ! grep -Fq '"status": "pass"' "$TEXT_SHORTCUTS_LIVE_IBUS_RUNTIME_RENDER_PROOF" \
  || ! grep -Fq '"route": "/v1/text-shortcuts"' "$TEXT_SHORTCUTS_LIVE_IBUS_RUNTIME_RENDER_PROOF" \
  || ! grep -Fq '"surface": "goblins-textshortcuts-live-ibus-runtime-render"' "$TEXT_SHORTCUTS_LIVE_IBUS_RUNTIME_RENDER_PROOF" \
  || ! grep -Fq '"input_driver": "qmp-keyboard"' "$TEXT_SHORTCUTS_LIVE_IBUS_RUNTIME_RENDER_PROOF" \
  || ! grep -Fq '"active_engine": "goblins-textshortcuts"' "$TEXT_SHORTCUTS_LIVE_IBUS_RUNTIME_RENDER_PROOF" \
  || ! grep -Fq '"normal_actual": "onmyway."' "$TEXT_SHORTCUTS_LIVE_IBUS_RUNTIME_RENDER_PROOF" \
  || ! grep -Fq '"passthrough_actual": "hello."' "$TEXT_SHORTCUTS_LIVE_IBUS_RUNTIME_RENDER_PROOF" \
  || ! grep -Fq '"password_refusal": "true"' "$TEXT_SHORTCUTS_LIVE_IBUS_RUNTIME_RENDER_PROOF" \
  || ! grep -Fq '"focused_field_callback": "true"' "$TEXT_SHORTCUTS_LIVE_IBUS_RUNTIME_RENDER_PROOF" \
  || ! grep -Fq '"text_input_v3_commit": "true"' "$TEXT_SHORTCUTS_LIVE_IBUS_RUNTIME_RENDER_PROOF" \
  || ! grep -Fq '"rendered_accept_bubble": "true"' "$TEXT_SHORTCUTS_LIVE_IBUS_RUNTIME_RENDER_PROOF" \
  || ! grep -Fq '"screenshot": "32-text-shortcuts-live-ibus-runtime-render.png"' "$TEXT_SHORTCUTS_LIVE_IBUS_RUNTIME_RENDER_PROOF" \
  || ! grep -Fq '"style_class": "gos-text-shortcuts-candidate"' "$TEXT_SHORTCUTS_LIVE_IBUS_RUNTIME_RENDER_PROOF" \
  || ! grep -Fq '"font_family": "Inter"' "$TEXT_SHORTCUTS_LIVE_IBUS_RUNTIME_RENDER_PROOF" \
  || ! grep -Fq '"rendered_bubble_ready_claim": "true"' "$TEXT_SHORTCUTS_LIVE_IBUS_RUNTIME_RENDER_PROOF" \
  || ! grep -Fq '"live_overlay_claim": "true"' "$TEXT_SHORTCUTS_LIVE_IBUS_RUNTIME_RENDER_PROOF" \
  || ! grep -Fq '"runtime_ready_claim": "true"' "$TEXT_SHORTCUTS_LIVE_IBUS_RUNTIME_RENDER_PROOF" \
  || ! grep -Fq '"core_readiness_flip": "live"' "$TEXT_SHORTCUTS_LIVE_IBUS_RUNTIME_RENDER_PROOF" \
  || [ ! -s "$RUN_DIR/32-text-shortcuts-live-ibus-runtime-render.png" ]; then
  echo "HONESTY GUARD: missing or failing Text Shortcuts live IBus runtime/render proof at $TEXT_SHORTCUTS_LIVE_IBUS_RUNTIME_RENDER_PROOF"
  exit 4
fi
if ! grep -Fq '"status": "pass"' "$KEYBOARD_SHORTCUTS_ROUNDTRIP_PROOF" \
  || ! grep -Fq '"shortcut_route": "/v1/keyboard/shortcuts/binding"' "$KEYBOARD_SHORTCUTS_ROUNDTRIP_PROOF" \
  || ! grep -Fq '"modifier_route": "/v1/keyboard/modifier-remap"' "$KEYBOARD_SHORTCUTS_ROUNDTRIP_PROOF" \
  || ! grep -Fq '"shortcut_action": "window-hud"' "$KEYBOARD_SHORTCUTS_ROUNDTRIP_PROOF" \
  || ! grep -Fq '"shortcut_binding": "<Super><Shift>H"' "$KEYBOARD_SHORTCUTS_ROUNDTRIP_PROOF" \
  || ! grep -Fq '"shortcut_http": "200"' "$KEYBOARD_SHORTCUTS_ROUNDTRIP_PROOF" \
  || ! grep -Fq '"shortcut_gsettings_readback": "true"' "$KEYBOARD_SHORTCUTS_ROUNDTRIP_PROOF" \
  || ! grep -Fq '"shortcut_reset_http": "200"' "$KEYBOARD_SHORTCUTS_ROUNDTRIP_PROOF" \
  || ! grep -Fq '"shortcut_reset_binding": "<Super>w"' "$KEYBOARD_SHORTCUTS_ROUNDTRIP_PROOF" \
  || ! grep -Fq '"modifier_target": "caps-lock"' "$KEYBOARD_SHORTCUTS_ROUNDTRIP_PROOF" \
  || ! grep -Fq '"modifier_value": "control"' "$KEYBOARD_SHORTCUTS_ROUNDTRIP_PROOF" \
  || ! grep -Fq '"modifier_http": "200"' "$KEYBOARD_SHORTCUTS_ROUNDTRIP_PROOF" \
  || ! grep -Fq '"modifier_gsettings_readback": "ctrl:nocaps"' "$KEYBOARD_SHORTCUTS_ROUNDTRIP_PROOF" \
  || ! grep -Fq '"modifier_reset_http": "200"' "$KEYBOARD_SHORTCUTS_ROUNDTRIP_PROOF" \
  || ! grep -Fq '"modifier_restore": "default"' "$KEYBOARD_SHORTCUTS_ROUNDTRIP_PROOF" \
  || ! grep -Fq '"roundtrip_restored": "true"' "$KEYBOARD_SHORTCUTS_ROUNDTRIP_PROOF"; then
  echo "HONESTY GUARD: missing or failing Keyboard shortcuts roundtrip proof at $KEYBOARD_SHORTCUTS_ROUNDTRIP_PROOF"
  exit 4
fi
if ! grep -Fq '"status": "pass"' "$INPUT_SOURCES_ROUNDTRIP_PROOF" \
  || ! grep -Fq '"source_route": "/v1/input/sources"' "$INPUT_SOURCES_ROUNDTRIP_PROOF" \
  || ! grep -Fq '"switch_route": "/v1/input/switch-next"' "$INPUT_SOURCES_ROUNDTRIP_PROOF" \
  || ! grep -Fq '"test_sources": "xkb-us,xkb-gb"' "$INPUT_SOURCES_ROUNDTRIP_PROOF" \
  || ! grep -Fq '"set_http": "200"' "$INPUT_SOURCES_ROUNDTRIP_PROOF" \
  || ! grep -Fq '"set_ok": "true"' "$INPUT_SOURCES_ROUNDTRIP_PROOF" \
  || ! grep -Fq '"sources_gsettings_readback": "true"' "$INPUT_SOURCES_ROUNDTRIP_PROOF" \
  || ! grep -Fq '"current_before_switch": "0"' "$INPUT_SOURCES_ROUNDTRIP_PROOF" \
  || ! grep -Fq '"switch_http": "200"' "$INPUT_SOURCES_ROUNDTRIP_PROOF" \
  || ! grep -Fq '"switch_ok": "true"' "$INPUT_SOURCES_ROUNDTRIP_PROOF" \
  || ! grep -Fq '"switch_switched": "true"' "$INPUT_SOURCES_ROUNDTRIP_PROOF" \
  || ! grep -Fq '"current_after_switch": "1"' "$INPUT_SOURCES_ROUNDTRIP_PROOF" \
  || ! grep -Fq '"restore_sources": "true"' "$INPUT_SOURCES_ROUNDTRIP_PROOF" \
  || ! grep -Fq '"restore_current": "true"' "$INPUT_SOURCES_ROUNDTRIP_PROOF" \
  || ! grep -Fq '"roundtrip_restored": "true"' "$INPUT_SOURCES_ROUNDTRIP_PROOF"; then
  echo "HONESTY GUARD: missing or failing Input sources roundtrip proof at $INPUT_SOURCES_ROUNDTRIP_PROOF"
  exit 4
fi
if ! grep -Fq '"status": "pass"' "$MULTI_DISPLAY_APPLY_PROOF" \
  || ! grep -Fq '"status_route": "/v1/displays/status"' "$MULTI_DISPLAY_APPLY_PROOF" \
  || ! grep -Fq '"apply_route": "/v1/displays/apply"' "$MULTI_DISPLAY_APPLY_PROOF" \
  || ! grep -Fq '"display_config": "org.gnome.Mutter.DisplayConfig"' "$MULTI_DISPLAY_APPLY_PROOF" \
  || ! grep -Fq '"verify_http": "200"' "$MULTI_DISPLAY_APPLY_PROOF" \
  || ! grep -Fq '"verify_ok": "true"' "$MULTI_DISPLAY_APPLY_PROOF" \
  || ! grep -Fq '"temporary_http": "200"' "$MULTI_DISPLAY_APPLY_PROOF" \
  || ! grep -Fq '"temporary_ok": "true"' "$MULTI_DISPLAY_APPLY_PROOF" \
  || ! grep -Fq '"persistent_guard_http": "400"' "$MULTI_DISPLAY_APPLY_PROOF" \
  || ! grep -Fq '"persistent_confirmation_required": "true"' "$MULTI_DISPLAY_APPLY_PROOF" \
  || ! grep -Fq '"stale_serial_http": "409"' "$MULTI_DISPLAY_APPLY_PROOF" \
  || ! grep -Fq '"stale_serial_rejected": "true"' "$MULTI_DISPLAY_APPLY_PROOF" \
  || ! grep -Fq '"roundtrip_restored": "true"' "$MULTI_DISPLAY_APPLY_PROOF" \
  || ! grep -Fq '"persistent_keep_claim": "false"' "$MULTI_DISPLAY_APPLY_PROOF" \
  || ! grep -Fq '"same_layout_noop": "true"' "$MULTI_DISPLAY_APPLY_PROOF"; then
  echo "HONESTY GUARD: missing or failing multi-display apply proof at $MULTI_DISPLAY_APPLY_PROOF"
  exit 4
fi
if ! grep -Fq '"status": "pass"' "$FOCUS_ARM_ROUNDTRIP_PROOF" \
  || ! grep -Fq '"status_route": "/v1/focus/status"' "$FOCUS_ARM_ROUNDTRIP_PROOF" \
  || ! grep -Fq '"activate_route": "/v1/focus/activate"' "$FOCUS_ARM_ROUNDTRIP_PROOF" \
  || ! grep -Fq '"deactivate_route": "/v1/focus/deactivate"' "$FOCUS_ARM_ROUNDTRIP_PROOF" \
  || ! grep -Fq '"test_mode": "gate-work"' "$FOCUS_ARM_ROUNDTRIP_PROOF" \
  || ! grep -Fq '"test_mode_configured": "true"' "$FOCUS_ARM_ROUNDTRIP_PROOF" \
  || ! grep -Fq '"activate_http": "200"' "$FOCUS_ARM_ROUNDTRIP_PROOF" \
  || ! grep -Fq '"activate_ok": "true"' "$FOCUS_ARM_ROUNDTRIP_PROOF" \
  || ! grep -Fq '"activate_active_mode": "gate-work"' "$FOCUS_ARM_ROUNDTRIP_PROOF" \
  || ! grep -Fq '"active_mode_gsettings_readback": "gate-work"' "$FOCUS_ARM_ROUNDTRIP_PROOF" \
  || ! grep -Fq '"armed_by_schedule_after_activate": "false"' "$FOCUS_ARM_ROUNDTRIP_PROOF" \
  || ! grep -Fq '"restore_banners_after_activate": "true"' "$FOCUS_ARM_ROUNDTRIP_PROOF" \
  || ! grep -Fq '"notification_banners_after_activate": "false"' "$FOCUS_ARM_ROUNDTRIP_PROOF" \
  || ! grep -Fq '"deactivate_http": "200"' "$FOCUS_ARM_ROUNDTRIP_PROOF" \
  || ! grep -Fq '"deactivate_ok": "true"' "$FOCUS_ARM_ROUNDTRIP_PROOF" \
  || ! grep -Fq '"deactivate_active_mode": ""' "$FOCUS_ARM_ROUNDTRIP_PROOF" \
  || ! grep -Fq '"active_mode_after_deactivate": ""' "$FOCUS_ARM_ROUNDTRIP_PROOF" \
  || ! grep -Fq '"armed_by_schedule_after_deactivate": "false"' "$FOCUS_ARM_ROUNDTRIP_PROOF" \
  || ! grep -Fq '"restore_banners_after_deactivate": ""' "$FOCUS_ARM_ROUNDTRIP_PROOF" \
  || ! grep -Fq '"notification_banners_after_deactivate": "true"' "$FOCUS_ARM_ROUNDTRIP_PROOF" \
  || ! grep -Fq '"original_focus_state_restored": "true"' "$FOCUS_ARM_ROUNDTRIP_PROOF" \
  || ! grep -Fq '"original_notification_banners_restored": "true"' "$FOCUS_ARM_ROUNDTRIP_PROOF" \
  || ! grep -Fq '"roundtrip_restored": "true"' "$FOCUS_ARM_ROUNDTRIP_PROOF" \
  || ! grep -Fq '"mode_crud_claim": "false"' "$FOCUS_ARM_ROUNDTRIP_PROOF" \
  || ! grep -Fq '"schedule_claim": "false"' "$FOCUS_ARM_ROUNDTRIP_PROOF" \
  || ! grep -Fq '"per_app_breakthroughs_claim": "false"' "$FOCUS_ARM_ROUNDTRIP_PROOF"; then
  echo "HONESTY GUARD: missing or failing Focus arm roundtrip proof at $FOCUS_ARM_ROUNDTRIP_PROOF"
  exit 4
fi
if ! grep -Fq '"status": "pass"' "$APP_PRIVACY_REVOKE_PROOF" \
  || ! grep -Fq '"route": "/v1/app-privacy/revoke"' "$APP_PRIVACY_REVOKE_PROOF" \
  || ! grep -Fq '"table": "location"' "$APP_PRIVACY_REVOKE_PROOF" \
  || ! grep -Fq '"app": "org.goblins.GatePrivacyProof"' "$APP_PRIVACY_REVOKE_PROOF" \
  || ! grep -Fq '"seed_method": "PermissionStore.SetPermission"' "$APP_PRIVACY_REVOKE_PROOF" \
  || ! grep -Fq '"revoke_method": "PermissionStore.DeletePermission"' "$APP_PRIVACY_REVOKE_PROOF" \
  || ! grep -Fq '"readback_method": "PermissionStore.GetPermission"' "$APP_PRIVACY_REVOKE_PROOF" \
  || ! grep -Fq '"seed_grant": "yes"' "$APP_PRIVACY_REVOKE_PROOF" \
  || ! grep -Fq '"seed_readback": "true"' "$APP_PRIVACY_REVOKE_PROOF" \
  || ! grep -Fq '"revoke_http": "200"' "$APP_PRIVACY_REVOKE_PROOF" \
  || ! grep -Fq '"revoke_ok": "true"' "$APP_PRIVACY_REVOKE_PROOF" \
  || ! grep -Fq '"post_revoke_absent": "true"' "$APP_PRIVACY_REVOKE_PROOF" \
  || ! grep -Fq '"restore_prior_state": "true"' "$APP_PRIVACY_REVOKE_PROOF" \
  || ! grep -Fq '"roundtrip_restored": "true"' "$APP_PRIVACY_REVOKE_PROOF" \
  || ! grep -Fq '"resource_keyed_claim": "false"' "$APP_PRIVACY_REVOKE_PROOF" \
  || ! grep -Fq '"device_revoke_claim": "false"' "$APP_PRIVACY_REVOKE_PROOF"; then
  echo "HONESTY GUARD: missing or failing App privacy revoke proof at $APP_PRIVACY_REVOKE_PROOF"
  exit 4
fi
if ! grep -Fq '"status": "pass"' "$PREVIEW_OPEN_RENDER_PROOF" \
  || ! grep -Fq '"status_route": "/v1/preview/status"' "$PREVIEW_OPEN_RENDER_PROOF" \
  || ! grep -Fq '"route": "/v1/preview/open"' "$PREVIEW_OPEN_RENDER_PROOF" \
  || ! grep -Fq '"status_http": "200"' "$PREVIEW_OPEN_RENDER_PROOF" \
  || ! grep -Fq '"available": "true"' "$PREVIEW_OPEN_RENDER_PROOF" \
  || ! grep -Fq '"xdg_open": "true"' "$PREVIEW_OPEN_RENDER_PROOF" \
  || ! grep -Fq '"papers": "true"' "$PREVIEW_OPEN_RENDER_PROOF" \
  || ! grep -Fq '"loupe": "true"' "$PREVIEW_OPEN_RENDER_PROOF" \
  || ! grep -Fq '"pdf_default": "org.gnome.Papers.desktop"' "$PREVIEW_OPEN_RENDER_PROOF" \
  || ! grep -Fq '"image_default": "org.gnome.Loupe.desktop"' "$PREVIEW_OPEN_RENDER_PROOF" \
  || ! grep -Fq '"jpeg_default": "org.gnome.Loupe.desktop"' "$PREVIEW_OPEN_RENDER_PROOF" \
  || ! grep -Fq '"pdf_http": "200"' "$PREVIEW_OPEN_RENDER_PROOF" \
  || ! grep -Fq '"pdf_ok": "true"' "$PREVIEW_OPEN_RENDER_PROOF" \
  || ! grep -Fq '"pdf_kind": "pdf"' "$PREVIEW_OPEN_RENDER_PROOF" \
  || ! grep -Fq '"pdf_process": "papers"' "$PREVIEW_OPEN_RENDER_PROOF" \
  || ! grep -Fq '"pdf_screenshot": "29-preview-pdf-open.png"' "$PREVIEW_OPEN_RENDER_PROOF" \
  || ! grep -Fq '"rendered_pdf_frame": "true"' "$PREVIEW_OPEN_RENDER_PROOF" \
  || ! grep -Fq '"image_http": "200"' "$PREVIEW_OPEN_RENDER_PROOF" \
  || ! grep -Fq '"image_ok": "true"' "$PREVIEW_OPEN_RENDER_PROOF" \
  || ! grep -Fq '"image_kind": "image"' "$PREVIEW_OPEN_RENDER_PROOF" \
  || ! grep -Fq '"image_process": "loupe"' "$PREVIEW_OPEN_RENDER_PROOF" \
  || ! grep -Fq '"image_screenshot": "30-preview-image-open.png"' "$PREVIEW_OPEN_RENDER_PROOF" \
  || ! grep -Fq '"rendered_image_frame": "true"' "$PREVIEW_OPEN_RENDER_PROOF" \
  || ! grep -Fq '"unsupported_http": "400"' "$PREVIEW_OPEN_RENDER_PROOF" \
  || ! grep -Fq '"unsupported_ok": "false"' "$PREVIEW_OPEN_RENDER_PROOF" \
  || ! grep -Fq '"unsupported_rejected": "true"' "$PREVIEW_OPEN_RENDER_PROOF"; then
  echo "HONESTY GUARD: missing or failing Preview open/render proof at $PREVIEW_OPEN_RENDER_PROOF"
  exit 4
fi

# HONESTY GUARD: refuse to write a signoff for a run whose surfaces aren't all
# distinct. GNOME 42+ returns AccessDenied to scripted screenshots (org.gnome.
# Shell.Screenshot), so the only automation path is the host QMP framebuffer
# screendump, which collapses some surfaces to byte-identical duplicates when a
# window doesn't foreground in time. close-signoff.sh only checks PNG validity,
# not distinctness — so without this guard, duplicates would falsely read as
# distinct proof and the shipping gate would go green on a lie. A human operator
# (the gate's by-design path) visually confirms each surface; an unattended agent
# cannot. Fail loudly rather than commit a dishonest run.
md5cmd() { command -v md5sum >/dev/null && md5sum "$@" || md5 -r "$@"; }
_required_pngs=()
while IFS= read -r -d '' png; do
  _required_pngs+=("$png")
done < <(find "$RUN_DIR" -maxdepth 1 -type f -name '*.png' ! -name '_debug-*' -print0)
_total="${#_required_pngs[@]}"
if [ "${_total:-0}" -eq 0 ]; then
  echo "HONESTY GUARD: no required screenshot surfaces were captured."
  exit 3
fi
_distinct="$(md5cmd "${_required_pngs[@]}" 2>/dev/null | awk '{print $1}' | sort -u | wc -l | tr -d ' ')"
if [ "${_distinct:-0}" -lt "${_total:-1}" ]; then
  echo "HONESTY GUARD: only $_distinct/$_total required captured surfaces are distinct."
  echo "GNOME Wayland blocks scripted per-window capture (AccessDenied); duplicate"
  echo "surfaces cannot be passed off as distinct proof. This run requires a human"
  echo "operator at the display (run-external-gate.sh) — refusing to close-signoff."
  exit 3
fi

stable_frame_hash() {
  local file="$1"
  # Ignore the top shell/GDM bar: its clock changes can make one stale screen
  # look byte-distinct while the actual proof surface never foregrounded.
  convert "$file" -gravity South -crop '100%x86%+0+0' \
    -resize 64x64! -colorspace Gray -depth 8 gray:- 2>/dev/null | md5cmd | awk '{print $1}'
}

_surface_shots=(
  04-desktop.png
  07-home.png
  08-shell-home.png
  10-settings.png
  11-settings-models.png
  12-settings-dark.png
  13-studio-before.png
  14-studio-running.png
  15-studio-app-detail.png
  16-built-app-open.png
  29-preview-pdf-open.png
  30-preview-image-open.png
  31-text-shortcuts-candidate-bubble-render.png
)
_stable_hashes="$(
  for shot in "${_surface_shots[@]}"; do
    [ -f "$RUN_DIR/$shot" ] || continue
    stable_frame_hash "$RUN_DIR/$shot" || true
  done | sort -u
)"
_stable_distinct="$(printf '%s\n' "$_stable_hashes" | sed '/^$/d' | wc -l | tr -d ' ')"
if [ "${_stable_distinct:-0}" -lt 8 ]; then
  echo "HONESTY GUARD: only $_stable_distinct stable app-surface frame(s) after cropping the top bar."
  echo "This usually means the VM is still on GDM, the login session timed out, or"
  echo "foregrounded proof windows were not captured. Refusing stale screenshot signoff."
  exit 3
fi

# Write the proof manifest + run close-signoff. The manifest records the
# repo-relative ISO path: close-signoff and verify-shipping-status both match
# the exact string "os/iso/output/$ARCH/bootiso/goblins-os-$ARCH.iso", and the
# committed manifest must not leak runner-absolute paths.
python3 - "$RUN_DIR" "${RUN_DIR#"$REPO/"}" "$ARCH" "${ISO#"$REPO/"}" "$ISO_SHA" "$DATE" <<'PY'
import json,sys
run_dir,rel_run_dir,arch,iso,sha,date=sys.argv[1:7]
json.dump({"architecture":arch,"iso":iso,"iso_sha256":sha,
          "captured_at":date+"T00:00:00Z","screenshot_run_dir":rel_run_dir,
          "firewall_live_toggle_proof":"firewall-live-toggle-proof.json",
          "text_shortcuts_session_enable_proof":"text-shortcuts-session-enable-proof.json",
          "text_shortcuts_candidate_metadata_proof":"text-shortcuts-candidate-metadata-proof.json",
          "text_shortcuts_overlay_intent_proof":"text-shortcuts-overlay-intent-proof.json",
          "text_shortcuts_candidate_bubble_frame_proof":"text-shortcuts-candidate-bubble-frame-proof.json",
          "text_shortcuts_candidate_bubble_layout_proof":"text-shortcuts-candidate-bubble-layout-proof.json",
          "text_shortcuts_candidate_bubble_render_intent_proof":"text-shortcuts-candidate-bubble-render-intent-proof.json",
          "text_shortcuts_candidate_bubble_render_proof":"text-shortcuts-candidate-bubble-render-proof.json",
          "text_shortcuts_live_ibus_runtime_render_proof":"text-shortcuts-live-ibus-runtime-render-proof.json",
          "keyboard_shortcuts_roundtrip_proof":"keyboard-shortcuts-roundtrip-proof.json",
          "input_sources_roundtrip_proof":"input-sources-roundtrip-proof.json",
          "multi_display_apply_proof":"multi-display-apply-proof.json",
          "focus_arm_roundtrip_proof":"focus-arm-roundtrip-proof.json",
          "app_privacy_revoke_proof":"app-privacy-revoke-proof.json",
          "preview_open_render_proof":"preview-open-render-proof.json",
          "audio_output_proof":"audio-output-proof.json",
          "runtime_build_proof":"runtime-build-proof.json",
          "capture_method":"display-backed qemu VM, software GPU/audio substrate (lavapipe/gamescope/pipewire), honestly labeled"},
         open(run_dir+"/proof-manifest.json","w"),indent=2)
PY
echo "capture complete: $RUN_DIR"
# Close-signoff matches the committed repo-relative run dir and reads its own
# relative paths (ISO, workflow) from the repo root.
RUNTIME_ENGINE_SOURCE="$(python3 - "$RUNTIME_BUILD_PROOF" <<'PY'
import json
import sys

try:
    print(json.load(open(sys.argv[1], encoding="utf-8")).get("engine_source", ""))
except Exception:
    print("")
PY
)"
( cd "$REPO" \
  && GOBLINS_OS_ARCH="$ARCH" \
    SCREENSHOT_DIR="${RUN_DIR#"$REPO/"}" \
    RUNTIME_ENGINE_MODE="local-model" \
    RUNTIME_ENGINE_SOURCE="$RUNTIME_ENGINE_SOURCE" \
    RUNTIME_ENGINE_CONFIG="${RUN_DIR#"$REPO/"}/runtime-build-proof.json" \
    BUILT_ARTIFACT_PATH_URL="${RUN_DIR#"$REPO/"}/runtime-build-proof.json" \
    os/hardware-gate/close-signoff.sh )
