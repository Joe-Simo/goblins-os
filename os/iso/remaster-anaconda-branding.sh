#!/bin/bash
# Goblins OS — Anaconda installer rebrand (ISO post-process).
#
# WHY THIS EXISTS
#   bootc-image-builder builds the anaconda-iso from two independent parts:
#     /container        -> the Goblins OS image (the system that gets installed)
#     /images/install.img-> the Anaconda *installer runtime* squashfs, built from
#                           stock Fedora anaconda packages (incl. fedora-logos).
#   The installer's product title reads from the deployed image's os-release (so it
#   already says "GOBLINS OS 44"), but the sidebar art + accent color live in
#   fedora-logos *inside install.img* and cannot be reached by editing the image.
#   This step rebuilds install.img with the Goblins identity and re-masters the ISO,
#   preserving the UEFI El Torito boot records and re-implanting the media checksum.
#
# WHAT IT DOES
#   - swaps  /usr/share/anaconda/pixmaps/sidebar-bg.png  -> Goblins dark sidebar
#   - swaps  /usr/share/anaconda/pixmaps/sidebar-logo.png-> white Goblins mark
#   - regenerates topbar-bg.png as a dark gradient (spoke nav bars go dark)
#   - recolors fedora.css accent  #51a2da -> #0b0b0f (Goblins ink)
#   - repacks the squashfs (zstd, 128K) and re-masters the ISO with xorriso replay
#
# RUN (Docker on macOS; the host has no native squashfs/xorriso). From the repo root:
#   docker run --rm \
#     -v "$PWD/os/brand/anaconda":/brand:ro \
#     -v "$PWD/os/iso":/scripts:ro \
#     -v /tmp/goblins-os-bib-output/bootiso:/iso:ro \
#     -v /tmp/goblins-os-iso-out:/work \
#     fedora:44 bash /scripts/remaster-anaconda-branding.sh
#   -> output: /work/install-goblins.iso  (keep under /tmp, NOT in the iCloud repo)
#
# NOTE ON SELINUX/XATTRS: the Docker LinuxKit VM has no SELinux LSM, so the squashfs
# is repacked without xattrs. This is safe: the Anaconda live environment runs SELinux
# permissive and as root, and the *installed* system's labels come from the embedded
# OCI image (relabeled on first boot), not from this installer rootfs.
set -euo pipefail

ISO_IN="${ISO_IN:-/iso/install.iso}"
ISO_OUT="${ISO_OUT:-/work/install-goblins.iso}"
BRAND="${BRAND:-/brand}"

echo "==> installing tools"
dnf -y install squashfs-tools xorriso isomd5sum ImageMagick >/dev/null 2>&1

mkdir -p /build && cd /build
echo "==> extracting install.img from $ISO_IN"
osirrox -indev "$ISO_IN" -extract /images/install.img /build/install.img 2>/dev/null
echo "==> extracting installer squashfs (no xattrs)"
unsquashfs -no-xattrs -d /build/sqroot /build/install.img >/dev/null
PIX=/build/sqroot/usr/share/anaconda/pixmaps

echo "==> applying Goblins identity"
cp "$BRAND/sidebar-bg.png"   "$PIX/sidebar-bg.png"
cp "$BRAND/sidebar-logo.png" "$PIX/sidebar-logo.png"
magick -size 1040x132 gradient:'#15151b'-'#0b0b0f' "$PIX/topbar-bg.png"
sed -i 's/#51a2da/#0b0b0f/gI' "$PIX/fedora.css"
for v in atomic cloud server; do
  d="$PIX/$v"; [ -d "$d" ] || continue
  cp "$BRAND/sidebar-bg.png"   "$d/sidebar-bg.png"   2>/dev/null || true
  cp "$BRAND/sidebar-logo.png" "$d/sidebar-logo.png" 2>/dev/null || true
  [ -f "$d/topbar-bg.png" ] && cp "$PIX/topbar-bg.png" "$d/topbar-bg.png" || true
  for css in "$d"/*.css; do [ -f "$css" ] && sed -i 's/#51a2da/#0b0b0f/gI' "$css"; done
done
grep -n "define-color fedora" "$PIX/fedora.css"

echo "==> repacking squashfs"
mksquashfs /build/sqroot /build/install-new.img -comp zstd -b 131072 -noappend -no-xattrs >/dev/null

echo "==> re-mastering ISO (preserving UEFI boot + the source volume label)"
# Do NOT force the volume id: GRUB's `inst.stage2=hd:LABEL=<volid>` is baked against
# the source ISO's label (GOBLINS_OS when built with os/iso/config.toml, but e.g.
# "Fedora-S-dvd-aarch64-44" for a no-config build). Overriding it here would desync
# the label from that cmdline and leave dracut unable to find stage2. Cloning
# indev->outdev without -volid preserves whatever the source used, which always matches.
xorriso -indev "$ISO_IN" -outdev "$ISO_OUT" \
        -boot_image any replay -overwrite on \
        -map /build/install-new.img /images/install.img \
        -commit -end 2>&1 | tail -n 5
implantisomd5 "$ISO_OUT" >/dev/null 2>&1 || echo "  (implantisomd5 warning, non-fatal)"

echo "==> done"; ls -la "$ISO_OUT"; sha256sum "$ISO_OUT"
