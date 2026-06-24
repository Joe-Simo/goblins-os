#!/bin/bash
# Goblins OS hardware-gate in-session capture orchestrator.
# Runs in the booted GNOME session (user goblin). Launches each gate surface and
# signals the host over the serial console; the host QMP-screendumps on each
# CAPREADY. Honest display-backed-VM capture from the real installed OS.
exec >/tmp/gate-cap.log 2>&1
set -x
H=10.0.2.2:8099
B=/usr/libexec/goblins-os
export GDK_BACKEND=wayland
# Signal host over HTTP: ready to capture <name>; host screendumps on this request.
sig(){ curl -s "http://$H/ready/$1" >/dev/null 2>&1; sleep 5; }
# Launch an app, let it render, signal, then kill it.
shot(){ local n="$1"; shift; "$@" >/dev/null 2>&1 & local p=$!; sleep 6; sig "$n"; kill "$p" 2>/dev/null; pkill -f "$1" 2>/dev/null; sleep 2; }

sleep 3
curl -s "http://$H/ready/ORCH_START" >/dev/null 2>&1
pkill -f goblins-os-login 2>/dev/null; pkill -f goblins-os-installer 2>/dev/null; sleep 2

# --- login + onboarding (launched explicitly, not relying on autostart) ---
shot 03-login         "$B/goblins-os-login"
shot 06-onboarding    "$B/goblins-os-installer"
sig 04-desktop

# --- session apps ---
shot 07-home          "$B/goblins-os-shell"
shot 08-shell-home    "$B/goblins-os-shell"
shot 10-settings      "$B/goblins-os-settings"
shot 11-settings-models "$B/goblins-os-settings" --panel=models
shot 13-studio-before "$B/goblins-os-shell"

# --- installer pages (real scratch disk) ---
GOBLINS_OS_INSTALLER_PAGE=appearance    shot 01-installer "$B/goblins-os-installer"
GOBLINS_OS_INSTALLER_PAGE=network       shot 02-install-network "$B/goblins-os-installer"
GOBLINS_OS_INSTALLER_PAGE=install-disk  shot 25-install-destination "$B/goblins-os-installer"
GOBLINS_OS_INSTALLER_PAGE=install-review shot 26-install-storage-summary "$B/goblins-os-installer"
GOBLINS_OS_INSTALLER_PAGE=details       shot 28-bootloader-efi-summary "$B/goblins-os-installer"

# --- gaming stack (real software substrate: lavapipe Vulkan, gamescope, pipewire) ---
shot 19-vulkan-vkcube  vkcube
( gamemoderun true >/dev/null 2>&1; gamemoded -t >/tmp/gm.txt 2>&1 ) &
shot 20-gamemode-active sh -c 'gamemoded -t 2>&1 | head -40 > /tmp/gm.txt; xterm -e "cat /tmp/gm.txt; sleep 8" 2>/dev/null || sleep 8'
shot 21-gamescope-session gamescope -W 1024 -H 640 --headless -- vkcube
shot 22-mangohud-overlay mangohud vkcube
shot 24-audio-output sh -c 'wpctl status > /tmp/wp.txt 2>&1; sleep 8'

curl -s "http://$H/ready/ORCH_ALLDONE" >/dev/null 2>&1
sleep 2
