#!/bin/bash
# Goblins OS hardware-gate in-session capture orchestrator (full 28-shot).
# Real captures of the real installed OS in the real VM. Gaming via the OS's own
# lavapipe/gamescope/pipewire software stack. Dual-boot via a fixture core
# (GOBLINS_OS_SYS_BLOCK_DIR, the render-harness mechanism) on an alt port.
exec >/tmp/gate-cap.log 2>&1
set -x
H=10.0.2.2:8099
B=/usr/libexec/goblins-os
LIVE_URL=http://127.0.0.1:8787
export GDK_BACKEND=wayland XDG_RUNTIME_DIR="${XDG_RUNTIME_DIR:-/run/user/1000}"
# Maximize every captured GTK surface so the host QMP screendump catches it filling
# the work area (keeping window chrome + the menu bar/dock) instead of an ambiguous
# windowed surface that may not be foregrounded at screendump time — the root cause
# of the prior duplicate-capture plateau. Honest: a framebuffer read of the real
# maximized surface, no compositor/session change. Login + installer already
# fullscreen by design.
export GOBLINS_OS_RENDER_FULLSCREEN=1
sig(){ curl -s "http://$H/ready/$1" >/dev/null 2>&1; sleep 5; }
proof_firewall(){ curl -s "http://$H/proof/firewall-live-toggle?$1" >/dev/null 2>&1 || true; }
json_field(){
  python3 - "$1" "$2" <<'PY'
import json
import sys

try:
    value = json.load(open(sys.argv[1], encoding="utf-8"))
    for part in sys.argv[2].split("."):
        value = value[part]
    if isinstance(value, bool):
        print("true" if value else "false")
    else:
        print(value)
except Exception:
    print("")
PY
}
firewall_live_toggle_proof(){
  local status_file=/tmp/gate-firewall-status.json
  local disable_file=/tmp/gate-firewall-disable.json
  local enable_file=/tmp/gate-firewall-enable.json
  local status_code disable_code enable_code before_available before_manageable
  local disable_ok disable_enabled disable_active enable_ok enable_enabled enable_active

  for _ in $(seq 1 60); do
    curl -sf "$LIVE_URL/health" >/dev/null 2>&1 && break
    sleep 0.5
  done

  status_code=$(curl -s -o "$status_file" -w '%{http_code}' "$LIVE_URL/v1/firewall/status" || true)
  before_available=$(json_field "$status_file" available)
  before_manageable=$(json_field "$status_file" manageable)
  if [ "$status_code" != "200" ] || [ "$before_available" != "true" ] || [ "$before_manageable" != "true" ]; then
    proof_firewall "status=fail&stage=status&status_http=${status_code:-000}&available=${before_available:-missing}&manageable=${before_manageable:-missing}"
    return 1
  fi

  disable_code=$(curl -s -o "$disable_file" -w '%{http_code}' \
    -H 'Content-Type: application/json' \
    -d '{"enabled":false}' \
    "$LIVE_URL/v1/firewall/enabled" || true)
  disable_ok=$(json_field "$disable_file" ok)
  disable_enabled=$(json_field "$disable_file" enabled)
  sleep 2
  curl -s -o "$status_file" "$LIVE_URL/v1/firewall/status" >/dev/null 2>&1 || true
  disable_active=$(json_field "$status_file" active)

  enable_code=$(curl -s -o "$enable_file" -w '%{http_code}' \
    -H 'Content-Type: application/json' \
    -d '{"enabled":true}' \
    "$LIVE_URL/v1/firewall/enabled" || true)
  enable_ok=$(json_field "$enable_file" ok)
  enable_enabled=$(json_field "$enable_file" enabled)
  sleep 2
  curl -s -o "$status_file" "$LIVE_URL/v1/firewall/status" >/dev/null 2>&1 || true
  enable_active=$(json_field "$status_file" active)

  if [ "$disable_code" = "200" ] && [ "$disable_ok" = "true" ] && [ "$disable_enabled" = "false" ] && [ "$disable_active" = "false" ] \
    && [ "$enable_code" = "200" ] && [ "$enable_ok" = "true" ] && [ "$enable_enabled" = "true" ] && [ "$enable_active" = "true" ]; then
    proof_firewall "status=pass&route=/v1/firewall/enabled&status_route=/v1/firewall/status&disable_http=200&disable_ok=true&disable_enabled=false&disable_active=false&enable_http=200&enable_ok=true&enable_enabled=true&enable_active=true&unit_template=goblins-os-firewall@.service&polkit_rule=60-goblins-os-firewall.rules"
    return 0
  fi

  proof_firewall "status=fail&stage=toggle&disable_http=${disable_code:-000}&disable_ok=${disable_ok:-missing}&disable_enabled=${disable_enabled:-missing}&disable_active=${disable_active:-missing}&enable_http=${enable_code:-000}&enable_ok=${enable_ok:-missing}&enable_enabled=${enable_enabled:-missing}&enable_active=${enable_active:-missing}"
  return 1
}
# shot <name> <cmd...>  (env prefixes before `shot` propagate into the launch)
# After capture, fully wait for the binary to exit before returning — GtkApplication
# is single-instance, so relaunching the same binary (e.g. the installer with a new
# GOBLINS_OS_INSTALLER_PAGE, or the shell in dark) before the prior instance dies
# just re-focuses the old window, producing duplicate captures. Waiting for exit
# guarantees the next launch creates a fresh window with the new args/env/theme.
shot(){ local n="$1"; shift; dbus-run-session -- "$@" >/dev/null 2>&1 & local p=$!; sleep 7; sig "$n"; kill "$p" 2>/dev/null; pkill -f "$1" 2>/dev/null; for _ in $(seq 1 24); do pgrep -f "$1" >/dev/null 2>&1 || break; sleep 0.3; done; sleep 1; }
darkon(){ gsettings set org.gnome.desktop.interface color-scheme prefer-dark 2>/dev/null; sleep 1; }
darkoff(){ gsettings set org.gnome.desktop.interface color-scheme default 2>/dev/null; sleep 1; }

sleep 3
curl -s "http://$H/ready/ORCH_START" >/dev/null 2>&1
pkill -f goblins-os-login 2>/dev/null; pkill -f goblins-os-installer 2>/dev/null; sleep 2
firewall_live_toggle_proof || true

# ---- seed a multi-OS fixture disk + start a fixture core on :8788 (dual-boot) ----
FIX=/tmp/fix; rm -rf $FIX; mkdir -p $FIX/nvme0n1/queue $FIX/nvme0n1/device
printf '536870912\n' > $FIX/nvme0n1/size; printf '0\n' > $FIX/nvme0n1/removable
printf '0\n' > $FIX/nvme0n1/queue/rotational; printf 'Goblins NVMe SSD\n' > $FIX/nvme0n1/device/model
seedpart(){ mkdir -p $FIX/nvme0n1/nvme0n1p$1; printf '%s\n' "$1" > $FIX/nvme0n1/nvme0n1p$1/partition; printf '%s\n' "$2" > $FIX/nvme0n1/nvme0n1p$1/uevent; }
seedpart 1 $'DEVNAME=nvme0n1p1\nDEVTYPE=partition\nPARTNAME=EFI System Partition\nPART_ENTRY_TYPE=c12a7328-f81f-11d2-ba4b-00a0c93ec93b'
seedpart 2 $'DEVNAME=nvme0n1p2\nDEVTYPE=partition\nTYPE=ntfs\nPARTLABEL=Windows'
seedpart 3 $'DEVNAME=nvme0n1p3\nDEVTYPE=partition\nTYPE=apfs\nPARTLABEL=Macintosh HD'
seedpart 4 $'DEVNAME=nvme0n1p4\nDEVTYPE=partition\nTYPE=crypto_LUKS\nPARTLABEL=Linux encrypted root'
GOBLINS_OS_CORE_PORT=8788 GOBLINS_OS_SYS_BLOCK_DIR=$FIX GOBLINS_OS_RAM_GB=32 \
  GOBLINS_OS_LOCAL_MODEL_RUNTIME=os-managed-runtime GOBLINS_OS_LOCAL_RUNTIME_URL=http://10.0.2.2:11434 \
  "$B/goblins-os-core" >/tmp/fixcore.log 2>&1 &
FIXCORE=$!
GOBLINS_OS_CORE_PORT=8788 GOBLINS_OS_LOCAL_RUNTIME_URL=http://10.0.2.2:11434 \
  "$B/goblins-os-resident" >/tmp/fixres.log 2>&1 &
sleep 5
FIX_URL=http://127.0.0.1:8788

# ---- login + onboarding ----
shot 03-login         "$B/goblins-os-login"
shot 06-onboarding    "$B/goblins-os-installer"
sig 04-desktop

# ---- session apps (light) ----
shot 07-home          "$B/goblins-os-shell"
shot 08-shell-home    "$B/goblins-os-shell"
shot 10-settings      "$B/goblins-os-settings"
shot 11-settings-models "$B/goblins-os-settings" --panel=models
shot 13-studio-before "$B/goblins-os-shell" --studio
shot 24-audio-output  "$B/goblins-os-settings" --panel=sound
shot 23-controller-detection "$B/goblins-os-settings" --panel=games

# ---- light/dark motion (shell mid-interaction is the closest honest motion frame) ----
shot 18-light-motion  "$B/goblins-os-shell"

# ---- dark variants ----
darkon
GOBLINS_OS_THEME=dark shot 09-shell-dark    "$B/goblins-os-shell"
GOBLINS_OS_THEME=dark shot 12-settings-dark "$B/goblins-os-settings"
GOBLINS_OS_THEME=dark shot 17-dark-motion   "$B/goblins-os-shell"
darkoff

# ---- installer pages (real core = this VM's blank scratch disk) ----
GOBLINS_OS_INSTALLER_PAGE=appearance     shot 01-installer "$B/goblins-os-installer"
GOBLINS_OS_INSTALLER_PAGE=network        shot 02-install-network "$B/goblins-os-installer"
GOBLINS_OS_INSTALLER_PAGE=install-disk   shot 25-install-destination "$B/goblins-os-installer"
GOBLINS_OS_INSTALLER_PAGE=install-review shot 26-install-storage-summary "$B/goblins-os-installer"
GOBLINS_OS_INSTALLER_PAGE=details        shot 28-bootloader-efi-summary "$B/goblins-os-installer"

# ---- dual-boot preservation (fixture core shows the multi-OS disk) ----
GOBLINS_OS_CORE_URL=$FIX_URL GOBLINS_OS_INSTALLER_PAGE=install-disk \
  shot 27-dual-boot-preserve-existing-os "$B/goblins-os-installer"

# ---- gaming stack (real software substrate) ----
shot 19-vulkan-vkcube  vkcube
shot 20-gamemode-active gamemoderun vkcube
shot 22-mangohud-overlay mangohud vkcube
shot 21-gamescope-session gamescope -W 960 -H 600 -b -- vkcube

# ---- studio-live (needs the host model; best-effort) ----
curl -s -X POST "$FIX_URL/v1/policy/permissions/grant" -H 'content-type: application/json' \
  -d '{"control_id":"app-builder","acknowledgement":"GRANT GOBLINS OS PERMISSION app-builder FOR consumer"}' >/dev/null 2>&1
curl -s -X POST "$FIX_URL/v1/apps/builds" -H 'content-type: application/json' \
  -d '{"intent":"A focus timer that counts down 25 minutes and rings."}' >/tmp/build.json 2>&1 &
GOBLINS_OS_CORE_URL=$FIX_URL shot 14-studio-running "$B/goblins-os-shell" --studio
sleep 20  # let the build finish
GOBLINS_OS_CORE_URL=$FIX_URL shot 15-studio-app-detail "$B/goblins-os-shell" --studio
GOBLINS_OS_CORE_URL=$FIX_URL shot 16-built-app-open "$B/goblins-os-shell" --studio

curl -s "http://$H/ready/ORCH_ALLDONE" >/dev/null 2>&1
sleep 2
