#!/usr/bin/env bash
# make-oemdrv.sh <output.img> <kickstart.ks>
# Build an 8 MiB FAT image labelled OEMDRV containing ks.cfg at its root, which
# Anaconda auto-detects and loads. Cross-platform (Linux CI + macOS/hvf).
set -euo pipefail
OUT="$1"; KS="$2"
rm -f "$OUT"
if [ "$(uname -s)" = "Linux" ]; then
  dd if=/dev/zero of="$OUT" bs=1M count=8 status=none
  mkfs.fat -n OEMDRV "$OUT" >/dev/null
  mcopy -i "$OUT" "$KS" ::ks.cfg            # mtools; install with: dnf/apt install mtools dosfstools
else
  TMP="${OUT%.img}"
  rm -f "$TMP.dmg"
  hdiutil create -size 8m -fs "MS-DOS FAT12" -volname OEMDRV -layout NONE "$TMP" >/dev/null
  mv "$TMP.dmg" "$OUT"
  for d in $(hdiutil info 2>/dev/null | grep -i oemdrv | awk '{print $1}' | grep '^/dev/disk'); do hdiutil detach "$d" 2>/dev/null || true; done
  MP="$(hdiutil attach "$OUT" -nomount | head -1 | awk '{print $1}')"
  diskutil mount "$MP" >/dev/null
  VOL="$(diskutil info "$MP" | awk -F': *' '/Mount Point/{print $2}')"
  cp "$KS" "$VOL/ks.cfg"
  diskutil unmount "$MP" >/dev/null
  hdiutil detach "$MP" >/dev/null 2>&1 || true
fi
echo "OEMDRV image ready: $OUT"
