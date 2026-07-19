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
# The caller must use the reviewed digest-pinned branding-tool image. This script
# never installs packages or reaches a network during release-media generation.
#
# NOTE ON SELINUX/XATTRS: the Docker LinuxKit VM has no SELinux LSM, so the squashfs
# is repacked without xattrs. This is safe: the Anaconda live environment runs SELinux
# permissive and as root, and the *installed* system's labels come from the embedded
# OCI image (relabeled on first boot), not from this installer rootfs.
set -euo pipefail

ISO_IN="${ISO_IN:-/iso/install.iso}"
ISO_OUT="${ISO_OUT:-/work/install-goblins.iso}"
BRAND="${BRAND:-/brand}"
GOBLINS_INK="#0b0b0f"
LEGACY_FEDORA_ACCENT="#51a2da"

for required_tool in checkisomd5 cmp implantisomd5 magick mksquashfs osirrox unsquashfs xorriso; do
  command -v "$required_tool" >/dev/null 2>&1 || {
    echo "missing immutable installer-branding tool: $required_tool" >&2
    exit 1
  }
done

for brand_asset in sidebar-bg.png sidebar-logo.png; do
  [ -s "$BRAND/$brand_asset" ] || {
    echo "missing required Goblins installer asset: $BRAND/$brand_asset" >&2
    exit 1
  }
done

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
  cp "$BRAND/sidebar-bg.png"   "$d/sidebar-bg.png"
  cp "$BRAND/sidebar-logo.png" "$d/sidebar-logo.png"
  [ -f "$d/topbar-bg.png" ] && cp "$PIX/topbar-bg.png" "$d/topbar-bg.png" || true
  for css in "$d"/*.css; do [ -f "$css" ] && sed -i 's/#51a2da/#0b0b0f/gI' "$css"; done
done

echo "==> verifying Goblins identity before repacking"
cmp --silent "$BRAND/sidebar-bg.png" "$PIX/sidebar-bg.png" || {
  echo "installer sidebar background does not match the reviewed Goblins asset" >&2
  exit 1
}
cmp --silent "$BRAND/sidebar-logo.png" "$PIX/sidebar-logo.png" || {
  echo "installer sidebar logo does not match the reviewed Goblins asset" >&2
  exit 1
}
grep -Fqi "$GOBLINS_INK" "$PIX/fedora.css" || {
  echo "installer stylesheet does not contain the required Goblins ink color" >&2
  exit 1
}
if grep -Fqi "$LEGACY_FEDORA_ACCENT" "$PIX/fedora.css"; then
  echo "installer stylesheet still contains the legacy Fedora accent" >&2
  exit 1
fi
for v in atomic cloud server; do
  d="$PIX/$v"; [ -d "$d" ] || continue
  cmp --silent "$BRAND/sidebar-bg.png" "$d/sidebar-bg.png" || {
    echo "$v installer sidebar background does not match the reviewed Goblins asset" >&2
    exit 1
  }
  cmp --silent "$BRAND/sidebar-logo.png" "$d/sidebar-logo.png" || {
    echo "$v installer sidebar logo does not match the reviewed Goblins asset" >&2
    exit 1
  }
  for css in "$d"/*.css; do
    [ -f "$css" ] || continue
    if grep -Fqi "$LEGACY_FEDORA_ACCENT" "$css"; then
      echo "$css still contains the legacy Fedora accent" >&2
      exit 1
    fi
  done
done
grep -nF "$GOBLINS_INK" "$PIX/fedora.css"

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
implantisomd5 "$ISO_OUT" >/dev/null
checkisomd5 --verbose "$ISO_OUT"

echo "==> done"; ls -la "$ISO_OUT"; sha256sum "$ISO_OUT"
