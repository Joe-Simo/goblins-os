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
#   - replaces every org.fedoraproject.AnacondaInstaller compatibility icon with
#     reviewed Goblins regular/symbolic artwork while retaining the icon names
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
BRAND_ROOT="${BRAND_ROOT:-/brand}"
BRAND="${BRAND:-$BRAND_ROOT/anaconda}"
INSTALLER_ICON_ROOT="${INSTALLER_ICON_ROOT:-/installer-icons}"
INSTALLER_ICON_REGULAR="${INSTALLER_ICON_REGULAR:-$INSTALLER_ICON_ROOT/org.goblins.OS.Installer.svg}"
INSTALLER_ICON_SYMBOLIC="${INSTALLER_ICON_SYMBOLIC:-$INSTALLER_ICON_ROOT/org.goblins.OS.Installer-symbolic.svg}"
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
for installer_icon_asset in "$INSTALLER_ICON_REGULAR" "$INSTALLER_ICON_SYMBOLIC"; do
  [ -s "$installer_icon_asset" ] || {
    echo "missing required Goblins installer icon asset: $installer_icon_asset" >&2
    exit 1
  }
done

installer_icon_paths() {
  find "$1" \( -type f -o -type l \) \
    \( -name 'org.fedoraproject.AnacondaInstaller.*' \
       -o -name 'org.fedoraproject.AnacondaInstaller-symbolic.*' \) \
    -print0 | LC_ALL=C sort -z
}

installer_icon_source() {
  case "${1##*/}" in
    org.fedoraproject.AnacondaInstaller-symbolic.*)
      printf '%s\n' "$INSTALLER_ICON_SYMBOLIC"
      ;;
    org.fedoraproject.AnacondaInstaller.*)
      printf '%s\n' "$INSTALLER_ICON_REGULAR"
      ;;
    *)
      return 1
      ;;
  esac
}

render_installer_png() {
  local source="$1" dimensions="$2" output="$3"
  [[ "$dimensions" =~ ^[1-9][0-9]*x[1-9][0-9]*$ ]] || {
    echo "invalid installer icon dimensions: $dimensions" >&2
    return 1
  }
  magick -background none "$source" \
    -resize "$dimensions" -gravity center -extent "$dimensions" -strip \
    "PNG32:$output"
}

brand_installer_icons() {
  local squash_root="$1" asset source dimensions expected difference
  local regular_count=0 symbolic_count=0 index=0
  local -a before=() after=()

  while IFS= read -r -d '' asset; do
    before[${#before[@]}]="$asset"
  done < <(installer_icon_paths "$squash_root")
  [ "${#before[@]}" -gt 0 ] || {
    echo "installer runtime contains no Anaconda installer compatibility icons" >&2
    return 1
  }

  for asset in "${before[@]}"; do
    source="$(installer_icon_source "$asset")" || {
      echo "unsupported Anaconda installer icon path: $asset" >&2
      return 1
    }
    case "${asset##*/}" in
      org.fedoraproject.AnacondaInstaller-symbolic.*)
        symbolic_count=$((symbolic_count + 1))
        ;;
      *)
        regular_count=$((regular_count + 1))
        ;;
    esac

    case "$asset" in
      *.svg)
        rm -f -- "$asset"
        install -m 0644 "$source" "$asset"
        ;;
      *.png)
        dimensions="$(magick identify -format '%wx%h' "$asset")"
        expected="/build/goblins-installer-icon-$index.png"
        render_installer_png "$source" "$dimensions" "$expected"
        rm -f -- "$asset"
        install -m 0644 "$expected" "$asset"
        rm -f -- "$expected"
        ;;
      *)
        echo "unsupported Anaconda installer icon format: $asset" >&2
        return 1
        ;;
    esac
    index=$((index + 1))
  done

  [ "$regular_count" -gt 0 ] || {
    echo "installer runtime contains no regular Anaconda installer compatibility icon" >&2
    return 1
  }
  [ "$symbolic_count" -gt 0 ] || {
    echo "installer runtime contains no symbolic Anaconda installer compatibility icon" >&2
    return 1
  }

  while IFS= read -r -d '' asset; do
    after[${#after[@]}]="$asset"
  done < <(installer_icon_paths "$squash_root")
  [ "${#after[@]}" -eq "${#before[@]}" ] || {
    echo "installer icon path set changed during branding" >&2
    return 1
  }
  for index in "${!before[@]}"; do
    [ "${before[$index]}" = "${after[$index]}" ] || {
      echo "installer icon compatibility filename changed during branding" >&2
      return 1
    }
  done

  index=0
  for asset in "${after[@]}"; do
    [ -f "$asset" ] && [ ! -L "$asset" ] || {
      echo "installer icon compatibility path is not a branded regular file: $asset" >&2
      return 1
    }
    source="$(installer_icon_source "$asset")"
    case "$asset" in
      *.svg)
        cmp --silent "$source" "$asset" || {
          echo "$asset does not match the reviewed Goblins installer icon" >&2
          return 1
        }
        ;;
      *.png)
        dimensions="$(magick identify -format '%wx%h' "$asset")"
        expected="/build/goblins-installer-icon-verify-$index.png"
        render_installer_png "$source" "$dimensions" "$expected"
        if ! difference="$(magick compare -metric AE "$expected" "$asset" null: 2>&1)"; then
          echo "$asset does not match the reviewed Goblins installer icon: $difference" >&2
          return 1
        fi
        rm -f -- "$expected"
        [ "$difference" = "0" ] || {
          echo "$asset differs from the reviewed Goblins installer icon by $difference pixels" >&2
          return 1
        }
        ;;
    esac
    echo "verified Goblins installer compatibility icon: $asset"
    index=$((index + 1))
  done
}

mkdir -p /build && cd /build
echo "==> extracting install.img from $ISO_IN"
osirrox -indev "$ISO_IN" -extract /images/install.img /build/install.img 2>/dev/null
echo "==> extracting installer squashfs (no xattrs)"
unsquashfs -no-xattrs -d /build/sqroot /build/install.img >/dev/null
SQUASH_ROOT=/build/sqroot
PIX=/build/sqroot/usr/share/anaconda/pixmaps

echo "==> applying Goblins identity"
brand_installer_icons "$SQUASH_ROOT"
cp "$BRAND/sidebar-bg.png"   "$PIX/sidebar-bg.png"
cp "$BRAND/sidebar-logo.png" "$PIX/sidebar-logo.png"
magick -size 1040x132 gradient:'#15151b'-'#0b0b0f' "$PIX/topbar-bg.png"
sed -i 's/#51a2da/#0b0b0f/gI' "$PIX/fedora.css"
find "$PIX" -mindepth 1 -type d -print0 | while IFS= read -r -d '' d; do
  [ -f "$d/sidebar-bg.png" ] && cp "$BRAND/sidebar-bg.png" "$d/sidebar-bg.png"
  [ -f "$d/sidebar-logo.png" ] && cp "$BRAND/sidebar-logo.png" "$d/sidebar-logo.png"
  [ -f "$d/topbar-bg.png" ] && cp "$PIX/topbar-bg.png" "$d/topbar-bg.png"
done
find "$PIX" -type f -name '*.css' -print0 | while IFS= read -r -d '' css; do
  sed -i 's/#51a2da/#0b0b0f/gI' "$css"
done
# The stock release-notes artwork is optional installer chrome and can carry
# upstream distribution identity. Goblins OS does not ship that visual surface.
rm -rf -- "$PIX/rnotes"

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
  echo "installer styles still contain the legacy Fedora accent" >&2
  exit 1
fi
find "$PIX" -mindepth 1 -type f -name 'sidebar-bg.png' -print0 | while IFS= read -r -d '' asset; do
  cmp --silent "$BRAND/sidebar-bg.png" "$asset" || {
    echo "$asset does not match the reviewed Goblins sidebar background" >&2
    exit 1
  }
done
find "$PIX" -mindepth 1 -type f -name 'sidebar-logo.png' -print0 | while IFS= read -r -d '' asset; do
  cmp --silent "$BRAND/sidebar-logo.png" "$asset" || {
    echo "$asset does not match the reviewed Goblins sidebar logo" >&2
    exit 1
  }
done
if grep -RFqi --include='*.css' "$LEGACY_FEDORA_ACCENT" "$PIX"; then
  echo "installer styles still contain the legacy Fedora accent" >&2
  exit 1
fi
if [ -e "$PIX/rnotes" ]; then
  echo "installer release-note artwork was not removed" >&2
  exit 1
fi
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
