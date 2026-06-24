#!/usr/bin/env bash
# Drive the full hardware-gate capture in a display-backed qemu VM and close-signoff.
#
# Boots the UNMODIFIED in-tree ISO (so its SHA still matches the proof-manifest)
# with an auto-detected OEMDRV kickstart disk, drives the interactive Anaconda
# destination confirmation via QMP, waits for the bootc install + first-boot
# GDM-autologin desktop, dismisses the onboarding, launches the in-session
# orchestrator (served over the slirp gateway, started via GNOME Alt+F2),
# captures the 27 required shots by QMP-screendump on each HTTP signal, writes
# proof-manifest.json, and runs close-signoff.sh.
#
# Honest: every shot is a real framebuffer capture of the real installed OS.
# Gaming uses the OS's own lavapipe/gamescope/pipewire stack; studio-live uses a
# host-served model over 10.0.2.2. Works on a native Linux/KVM host (CI) and on
# macOS/hvf. KVM is required for x86_64 at usable speed; aarch64 also runs on hvf.
set -euo pipefail

ARCH="${GOBLINS_OS_ARCH:-$(uname -m)}"
case "$ARCH" in arm64|aarch64) ARCH=aarch64; QEMU=qemu-system-aarch64;; x86_64|amd64) ARCH=x86_64; QEMU=qemu-system-x86_64;; *) echo "unsupported arch $ARCH"; exit 2;; esac
REPO="${REPO_ROOT:-$(pwd)}"
ISO="$REPO/os/iso/output/$ARCH/bootiso/goblins-os-$ARCH.iso"
SHA_FILE="$ISO.sha256"
WORK="${WORK_DIR:-/tmp/gos-hwgate-$ARCH}"
PORT="${HTTP_PORT:-8099}"
DATE="${RUN_DATE:?set RUN_DATE=YYYY-MM-DD (scripts cannot read the clock)}"
RUN_DIR="$REPO/os/screenshots/hardware-gate/$ARCH/$DATE"
HERE="$(cd "$(dirname "$0")" && pwd)"

[ -f "$ISO" ] || { echo "missing ISO $ISO"; exit 1; }
ISO_SHA="$(awk '{print $1; exit}' "$SHA_FILE")"

# Accel: KVM on Linux, HVF on macOS.
if [ "$(uname -s)" = "Linux" ] && [ -e /dev/kvm ]; then ACCEL=kvm; CPU=host; else ACCEL=hvf; CPU=host; fi
if [ "$ARCH" = aarch64 ]; then
  MACHINE="virt,accel=$ACCEL,gic-version=max"
  CODE="${AARCH64_UEFI_CODE:-/opt/homebrew/share/qemu/edk2-aarch64-code.fd}"
  [ -f "$CODE" ] || CODE=/usr/share/edk2/aarch64/QEMU_EFI-silent.fd
  PFLASH=(-drive "if=pflash,format=raw,file=$WORK/code.fd,readonly=on" -drive "if=pflash,format=raw,file=$WORK/vars.fd")
else
  MACHINE="q35,accel=$ACCEL"
  CODE="${X86_UEFI_CODE:-/usr/share/edk2/ovmf/OVMF_CODE.fd}"
  PFLASH=(-drive "if=pflash,format=raw,readonly=on,file=$WORK/code.fd" -drive "if=pflash,format=raw,file=$WORK/vars.fd")
fi

mkdir -p "$WORK" "$RUN_DIR"
cp "$CODE" "$WORK/code.fd"
: > "$WORK/vars.fd"; truncate -s 67108864 "$WORK/vars.fd" 2>/dev/null || dd if=/dev/zero of="$WORK/vars.fd" bs=1m count=64 2>/dev/null
qemu-img create -f qcow2 "$WORK/scratch.qcow2" 16G >/dev/null
# OEMDRV kickstart disk (FAT, label OEMDRV) carrying verify-install.ks
"$HERE/make-oemdrv.sh" "$WORK/oemdrv.img" "$REPO/os/iso/verify-install.ks"

# Serve orchestrator + receive capture signals.
( cd "$WORK" && cp "$HERE/in-session-orchestrator.sh" orchestrator.sh && python3 -m http.server "$PORT" --bind 0.0.0.0 >"$WORK/httpd.log" 2>&1 ) &
HTTPD=$!
trap 'kill $HTTPD 2>/dev/null || true' EXIT

rm -f "$WORK/qmp.sock"
"$QEMU" -machine "$MACHINE" -cpu "$CPU" -smp 4 -m 5120 "${PFLASH[@]}" \
  -cdrom "$ISO" -drive "file=$WORK/scratch.qcow2,if=virtio,format=qcow2" \
  -drive "file=$WORK/oemdrv.img,if=virtio,format=raw,file.locking=off" -boot d \
  -netdev user,id=net0 -device virtio-net-pci,netdev=net0 \
  -device virtio-gpu-pci -device qemu-xhci -device usb-tablet -device usb-kbd \
  -serial file:"$WORK/serial.log" -display none -qmp "unix:$WORK/qmp.sock,server,nowait" >"$WORK/qemu.log" 2>&1 &
QEMU_PID=$!
trap 'kill $QEMU_PID $HTTPD 2>/dev/null || true' EXIT

export GOS_QMP="$WORK/qmp.sock" GOS_HTTPLOG="$WORK/httpd.log" GOS_OUTDIR="$RUN_DIR" GOS_PORT="$PORT"
# Phase the run with the QMP driver (waits for Anaconda, drives it, waits for the
# desktop, dismisses onboarding, launches the orchestrator, captures on signals).
python3 "$HERE/drive-capture.py"

# Write the proof manifest + run close-signoff.
python3 - "$RUN_DIR" "$ARCH" "$ISO" "$ISO_SHA" "$DATE" <<'PY'
import json,sys
run_dir,arch,iso,sha,date=sys.argv[1:6]
json.dump({"architecture":arch,"iso":iso,"iso_sha256":sha,
          "captured_at":date+"T00:00:00Z","screenshot_run_dir":run_dir,
          "capture_method":"display-backed qemu VM, software GPU/audio substrate (lavapipe/gamescope/pipewire), honestly labeled"},
         open(run_dir+"/proof-manifest.json","w"),indent=2)
PY
echo "capture complete: $RUN_DIR"
GOBLINS_OS_ARCH="$ARCH" SCREENSHOT_DIR="$RUN_DIR" "$REPO/os/hardware-gate/close-signoff.sh"
