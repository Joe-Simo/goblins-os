#!/usr/bin/env bash
# Render the native Goblins OS GTK apps headlessly inside the real OS image and
# capture a screenshot of each window. This is a packaging-time design proof:
# it runs the actual installed binaries against the actual OS daemon, so the
# captured pixels are the installed first-boot UI, not a storyboard.
set -euo pipefail

OUT=/out
mkdir -p "$OUT"
RENDER_SCOPE="${GOBLINS_OS_RENDER_SCOPE:-all}"
case "$RENDER_SCOPE" in
  all|chrome|installer|settings|settings-interactions) ;;
  *)
    echo "RENDER-FAILED unsupported GOBLINS_OS_RENDER_SCOPE=$RENDER_SCOPE (expected all, chrome, installer, settings, or settings-interactions)" >&2
    exit 2
    ;;
esac

export XDG_RUNTIME_DIR=/tmp/xdg
mkdir -p "$XDG_RUNTIME_DIR"
chmod 700 "$XDG_RUNTIME_DIR"
# GTK4 has no GPU under Xvfb; force the software/cairo path.
export GDK_BACKEND=x11
export GSK_RENDERER=cairo
export LIBGL_ALWAYS_SOFTWARE=1

# Represent a capable workstation for the render so the installer can show the
# eligible-vs-greyed model contrast: the RAM override (for VMs/containers whose
# auto-detected RAM is wrong) reports 32GB, and an OS-selected inference runtime
# is present. gpt-oss-20b then becomes installable while gpt-oss-120b stays
# greyed out (it needs an 80GB-class GPU that is not present).
export GOBLINS_OS_RAM_GB=32
export GOBLINS_OS_LOCAL_MODEL_RUNTIME=os-managed-runtime

# Seed three block-device fixtures so the native live installer renders the same
# preservation states it reads from /sys/block on hardware:
# one protected NVMe with EFI, Windows, macOS/APFS, Linux/LUKS, and data-style
# partitions so the dual-boot preservation route is visible, one blank eligible
# NVMe for the whole-disk review/confirmation screenshots, and one blocked USB stick
# (removable + under the 32GB minimum). The core scans this directory in
# place of /sys/block via GOBLINS_OS_SYS_BLOCK_DIR. Eligibility still requires
# bootc present + root (both true in this build), and the destructive execute
# stays gated behind GOBLINS_OS_ENABLE_DESTRUCTIVE_INSTALL=1, which is never set —
# so these captures prove the UI without ever risking a real wipe. The env must be
# exported BEFORE the core launches, since the daemon inherits it at start.
export GOBLINS_OS_SYS_BLOCK_DIR=/tmp/goblins-os-render-sys-block
export GOBLINS_OS_BOOTC_IMAGE=localhost/goblins-os:render
export GOBLINS_OS_SESSION=gnome-native-desktop
export GOBLINS_OS_GUI_PLATFORM=gnome-session
export GOBLINS_OS_SHELL_MODE=native-desktop
export GOBLINS_OS_RENDER_STATE_DIR=/tmp/goblins-os-render-state
export GOBLINS_OS_INSTALLER_STATE="$GOBLINS_OS_RENDER_STATE_DIR/installer"
export GOBLINS_OS_SESSION_STATE="$GOBLINS_OS_RENDER_STATE_DIR/session"
rm -rf "$GOBLINS_OS_RENDER_STATE_DIR"
mkdir -p "$GOBLINS_OS_SYS_BLOCK_DIR/nvme0n1/queue" "$GOBLINS_OS_SYS_BLOCK_DIR/nvme0n1/device"
printf '536870912\n'        > "$GOBLINS_OS_SYS_BLOCK_DIR/nvme0n1/size"
printf '0\n'                > "$GOBLINS_OS_SYS_BLOCK_DIR/nvme0n1/removable"
printf '0\n'                > "$GOBLINS_OS_SYS_BLOCK_DIR/nvme0n1/queue/rotational"
printf 'Goblins NVMe SSD\n' > "$GOBLINS_OS_SYS_BLOCK_DIR/nvme0n1/device/model"
mkdir -p "$GOBLINS_OS_SYS_BLOCK_DIR/nvme0n1/nvme0n1p1"
printf '1\n' > "$GOBLINS_OS_SYS_BLOCK_DIR/nvme0n1/nvme0n1p1/partition"
printf 'DEVNAME=nvme0n1p1\nDEVTYPE=partition\nPARTNAME=EFI System Partition\nPART_ENTRY_TYPE=c12a7328-f81f-11d2-ba4b-00a0c93ec93b\n' > "$GOBLINS_OS_SYS_BLOCK_DIR/nvme0n1/nvme0n1p1/uevent"
mkdir -p "$GOBLINS_OS_SYS_BLOCK_DIR/nvme0n1/nvme0n1p2"
printf '2\n' > "$GOBLINS_OS_SYS_BLOCK_DIR/nvme0n1/nvme0n1p2/partition"
printf 'DEVNAME=nvme0n1p2\nDEVTYPE=partition\nTYPE=ntfs\nPARTLABEL=Windows\n' > "$GOBLINS_OS_SYS_BLOCK_DIR/nvme0n1/nvme0n1p2/uevent"
mkdir -p "$GOBLINS_OS_SYS_BLOCK_DIR/nvme0n1/nvme0n1p3"
printf '3\n' > "$GOBLINS_OS_SYS_BLOCK_DIR/nvme0n1/nvme0n1p3/partition"
printf 'DEVNAME=nvme0n1p3\nDEVTYPE=partition\nTYPE=apfs\nPARTLABEL=Macintosh HD\n' > "$GOBLINS_OS_SYS_BLOCK_DIR/nvme0n1/nvme0n1p3/uevent"
mkdir -p "$GOBLINS_OS_SYS_BLOCK_DIR/nvme0n1/nvme0n1p4"
printf '4\n' > "$GOBLINS_OS_SYS_BLOCK_DIR/nvme0n1/nvme0n1p4/partition"
printf 'DEVNAME=nvme0n1p4\nDEVTYPE=partition\nTYPE=crypto_LUKS\nPARTLABEL=Linux encrypted root\n' > "$GOBLINS_OS_SYS_BLOCK_DIR/nvme0n1/nvme0n1p4/uevent"
mkdir -p "$GOBLINS_OS_SYS_BLOCK_DIR/nvme0n1/nvme0n1p5"
printf '5\n' > "$GOBLINS_OS_SYS_BLOCK_DIR/nvme0n1/nvme0n1p5/partition"
printf 'DEVNAME=nvme0n1p5\nDEVTYPE=partition\nTYPE=zfs_member\nPARTLABEL=Shared data\n' > "$GOBLINS_OS_SYS_BLOCK_DIR/nvme0n1/nvme0n1p5/uevent"
mkdir -p "$GOBLINS_OS_SYS_BLOCK_DIR/nvme1n1/queue" "$GOBLINS_OS_SYS_BLOCK_DIR/nvme1n1/device"
printf '536870912\n'        > "$GOBLINS_OS_SYS_BLOCK_DIR/nvme1n1/size"
printf '0\n'                > "$GOBLINS_OS_SYS_BLOCK_DIR/nvme1n1/removable"
printf '0\n'                > "$GOBLINS_OS_SYS_BLOCK_DIR/nvme1n1/queue/rotational"
printf 'Goblins Blank SSD\n' > "$GOBLINS_OS_SYS_BLOCK_DIR/nvme1n1/device/model"
mkdir -p "$GOBLINS_OS_SYS_BLOCK_DIR/sdb/queue" "$GOBLINS_OS_SYS_BLOCK_DIR/sdb/device"
printf '33554432\n'          > "$GOBLINS_OS_SYS_BLOCK_DIR/sdb/size"
printf '1\n'                 > "$GOBLINS_OS_SYS_BLOCK_DIR/sdb/removable"
printf '0\n'                 > "$GOBLINS_OS_SYS_BLOCK_DIR/sdb/queue/rotational"
printf 'Generic USB Stick\n' > "$GOBLINS_OS_SYS_BLOCK_DIR/sdb/device/model"

CORE_PID=""
RES_PID=""
XVFB_PID=""

cleanup_app_windows() {
  pkill -f "/usr/libexec/goblins-os/goblins-os-installer" 2>/dev/null || true
  pkill -f "/usr/libexec/goblins-os/goblins-os-login" 2>/dev/null || true
  pkill -f "/usr/libexec/goblins-os/goblins-os-settings" 2>/dev/null || true
  pkill -f "/usr/libexec/goblins-os/goblins-os-shell" 2>/dev/null || true
  pkill -f "/usr/libexec/goblins-os/goblins-os-launcher" 2>/dev/null || true
  pkill -f "/usr/libexec/goblins-os/goblins-os-control-center" 2>/dev/null || true
  pkill -f "/usr/libexec/goblins-os/goblins-os-today" 2>/dev/null || true
  pkill -f "dbus-run-session -- /usr/libexec/goblins-os/" 2>/dev/null || true
  sleep 0.4
}

shutdown_render_services() {
  cleanup_app_windows || true
  [ -n "$XVFB_PID" ] && kill "$XVFB_PID" 2>/dev/null || true
  [ -n "$CORE_PID" ] && kill "$CORE_PID" 2>/dev/null || true
  [ -n "$RES_PID" ] && kill "$RES_PID" 2>/dev/null || true
}

seed_first_boot_profile() {
  local mode="$1"

  mkdir -p "$GOBLINS_OS_INSTALLER_STATE" "$GOBLINS_OS_SESSION_STATE"
  printf '{"mode":"%s","completed_at":"%s"}\n' \
    "$mode" "$(date -u +%Y-%m-%dT%H:%M:%SZ)" \
    > "$GOBLINS_OS_INSTALLER_STATE/first-boot.json"
  rm -f "$GOBLINS_OS_SESSION_STATE/gate.json"
}

clear_first_boot_profile() {
  rm -f "$GOBLINS_OS_INSTALLER_STATE/first-boot.json"
}

trap shutdown_render_services EXIT

# Bring the OS daemon and persistent resident online so the native apps render
# with real system state (hardware scan, model eligibility, resident status).
/usr/libexec/goblins-os/goblins-os-core &
CORE_PID=$!
/usr/libexec/goblins-os/goblins-os-resident &
RES_PID=$!

for _ in $(seq 1 50); do
  if curl -sf http://127.0.0.1:8787/health >/dev/null 2>&1; then
    echo "core daemon is healthy"
    break
  fi
  sleep 0.2
done

Xvfb :99 -screen 0 1440x900x24 -nolisten tcp &
XVFB_PID=$!
export DISPLAY=:99
sleep 1.5

wait_for_exact_title() {
  local title="$1"
  local wid=""

  for _ in $(seq 1 60); do
    wid=$(xdotool search --onlyvisible --name "^${title}$" 2>/dev/null | tail -1 || true)
    if [ -n "$wid" ]; then
      printf '%s\n' "$wid"
      return 0
    fi
    sleep 0.3
  done

  return 1
}

capture() {
  local bin="$1" title="$2" out="$3"
  shift 3

  cleanup_app_windows
  dbus-run-session -- /usr/libexec/goblins-os/"$bin" "$@" &
  local wid=""
  wid=$(wait_for_exact_title "$title" || true)
  # Let the widget tree, CSS, and network-backed data settle before capture.
  sleep 2.5
  if [ -z "$wid" ]; then
    echo "RENDER-FAILED $out ($title) no visible exact-title window" >&2
    cleanup_app_windows
    return 1
  fi

  import -window "$wid" "$OUT/$out" && echo "RENDERED $out ($title) wid=$wid"
  cleanup_app_windows
}

capture_existing_window() {
  local wid="$1" out="$2" title="$3"

  import -window "$wid" "$OUT/$out" && echo "RENDERED $out ($title) wid=$wid"
}

capture_settings_panel() {
  local slug="$1" title="$2" out="$3"
  capture goblins-os-settings "Goblins OS Settings - ${title}" "$out" --panel="$slug"
}

capture_settings_light_surface() {
  capture goblins-os-settings  "Goblins OS Settings" 03-settings.png
  # The Models panel is the focused home for the GPT-OSS engine and the optional
  # bring-your-own OpenAI API key; render it as its own design proof. Its window
  # carries a panel-specific title so this capture never matches the overview.
  capture goblins-os-settings  "Goblins OS Settings - AI & Models" 05-settings-models.png --panel=models
  # The Policy and Recovery panels — the OS's permission/consent surface and its
  # rollback/recovery surface; rendered so every Settings panel is a design proof.
  capture goblins-os-settings  "Goblins OS Settings - Policy"   19-settings-policy.png --panel=policy
  capture goblins-os-settings  "Goblins OS Settings - Recovery" 20-settings-recovery.png --panel=recovery
  # Expanded System Settings surface. These panels are functional where backed by
  # GNOME/core state and explicitly read-only where the required backend route is
  # not available yet.
  capture_settings_panel appearance          "Appearance"            46-settings-appearance.png
  capture_settings_panel network             "Network"               47-settings-network.png
  capture_settings_panel bluetooth           "Bluetooth"             48-settings-bluetooth.png
  capture_settings_panel displays            "Displays"              49-settings-displays.png
  capture_settings_panel sound               "Sound"                 50-settings-sound.png
  capture_settings_panel keyboard            "Keyboard"              51-settings-keyboard.png
  capture_settings_panel mouse-trackpad      "Mouse & Trackpad"      52-settings-mouse-trackpad.png
  capture_settings_panel accessibility       "Accessibility"         53-settings-accessibility.png
  capture_settings_panel desktop-wallpaper   "Desktop & Wallpaper"   54-settings-desktop-wallpaper.png
  capture_settings_panel notifications       "Notifications"         55-settings-notifications.png
  capture_settings_panel lock-screen         "Lock Screen"           110-settings-lock-screen.png
  capture_settings_panel users-accounts      "Users & Accounts"      56-settings-users-accounts.png
  capture_settings_panel privacy-permissions "Privacy & Permissions" 57-settings-privacy-permissions.png
  capture_settings_panel storage             "Storage"               58-settings-storage.png
  capture_settings_panel updates-about       "Updates & About"       59-settings-updates-about.png
  capture_settings_panel developer           "Diagnostics"           60-settings-developer.png
  capture_settings_panel applications        "Applications"          76-settings-applications.png
  capture_settings_panel network-services    "Wired & VPN"           77-settings-wired-vpn.png
  capture_settings_panel mobile-broadband    "Mobile Broadband"      78-settings-mobile-broadband.png
  capture_settings_panel sharing             "Sharing"               79-settings-sharing.png
  capture_settings_panel color-management    "Color"                 80-settings-color.png
  capture_settings_panel drawing-tablet      "Drawing Tablet"        81-settings-drawing-tablet.png
  capture_settings_panel search              "Search"                82-settings-search.png
  capture_settings_panel multitasking        "Multitasking"          83-settings-multitasking.png
  capture_settings_panel power-battery       "Power & Battery"       84-settings-power-battery.png
  capture_settings_panel games               "Games"                 116-settings-games.png
  capture_settings_panel printers-scanners   "Printers & Scanners"   85-settings-printers-scanners.png
  capture_settings_panel date-time           "Date & Time"           111-settings-date-time.png
  capture_settings_panel language-region     "Language & Region"     112-settings-language-region.png
  capture_settings_panel online-accounts     "Online Accounts"       86-settings-online-accounts.png
  capture_settings_panel wellbeing           "Wellbeing"             87-settings-wellbeing.png
  capture_settings_panel security                "Security"                   104-settings-security.png
  capture_settings_panel desktop-dock            "Desktop & Dock"             105-settings-desktop-dock.png
  capture_settings_panel menu-bar-control-center "Menu Bar & Control Center"  106-settings-menu-bar.png
}

capture_settings_dark_surface() {
  local had_theme=0
  local previous_theme=""
  if [ "${GOBLINS_OS_THEME+x}" = x ]; then
    had_theme=1
    previous_theme="$GOBLINS_OS_THEME"
  fi

  export GOBLINS_OS_THEME=dark
  capture goblins-os-settings  "Goblins OS Settings" 11-settings-dark.png
  # Models panel in Dark — proves the BYO OpenAI API-key field (the one text input
  # in the OS) themes like every other surface instead of rendering a white slab.
  capture goblins-os-settings  "Goblins OS Settings - AI & Models" 23-settings-models-dark.png --panel=models
  # The remaining Settings panels in Dark (login-dark is captured earlier, pre-unlock):
  capture goblins-os-settings  "Goblins OS Settings - Policy"   33-settings-policy-dark.png --panel=policy
  capture goblins-os-settings  "Goblins OS Settings - Recovery" 34-settings-recovery-dark.png --panel=recovery
  capture_settings_panel appearance          "Appearance"            61-settings-appearance-dark.png
  capture_settings_panel network             "Network"               62-settings-network-dark.png
  capture_settings_panel bluetooth           "Bluetooth"             63-settings-bluetooth-dark.png
  capture_settings_panel displays            "Displays"              64-settings-displays-dark.png
  capture_settings_panel sound               "Sound"                 65-settings-sound-dark.png
  capture_settings_panel keyboard            "Keyboard"              66-settings-keyboard-dark.png
  capture_settings_panel mouse-trackpad      "Mouse & Trackpad"      67-settings-mouse-trackpad-dark.png
  capture_settings_panel accessibility       "Accessibility"         68-settings-accessibility-dark.png
  capture_settings_panel desktop-wallpaper   "Desktop & Wallpaper"   69-settings-desktop-wallpaper-dark.png
  capture_settings_panel notifications       "Notifications"         70-settings-notifications-dark.png
  capture_settings_panel lock-screen         "Lock Screen"           113-settings-lock-screen-dark.png
  capture_settings_panel users-accounts      "Users & Accounts"      71-settings-users-accounts-dark.png
  capture_settings_panel privacy-permissions "Privacy & Permissions" 72-settings-privacy-permissions-dark.png
  capture_settings_panel storage             "Storage"               73-settings-storage-dark.png
  capture_settings_panel updates-about       "Updates & About"       74-settings-updates-about-dark.png
  capture_settings_panel developer           "Diagnostics"           75-settings-developer-dark.png
  capture_settings_panel applications        "Applications"          88-settings-applications-dark.png
  capture_settings_panel network-services    "Wired & VPN"           89-settings-wired-vpn-dark.png
  capture_settings_panel mobile-broadband    "Mobile Broadband"      90-settings-mobile-broadband-dark.png
  capture_settings_panel sharing             "Sharing"               91-settings-sharing-dark.png
  capture_settings_panel color-management    "Color"                 92-settings-color-dark.png
  capture_settings_panel drawing-tablet      "Drawing Tablet"        93-settings-drawing-tablet-dark.png
  capture_settings_panel search              "Search"                94-settings-search-dark.png
  capture_settings_panel multitasking        "Multitasking"          95-settings-multitasking-dark.png
  capture_settings_panel power-battery       "Power & Battery"       96-settings-power-battery-dark.png
  capture_settings_panel games               "Games"                 117-settings-games-dark.png
  capture_settings_panel printers-scanners   "Printers & Scanners"   97-settings-printers-scanners-dark.png
  capture_settings_panel date-time           "Date & Time"           114-settings-date-time-dark.png
  capture_settings_panel language-region     "Language & Region"     115-settings-language-region-dark.png
  capture_settings_panel online-accounts     "Online Accounts"       98-settings-online-accounts-dark.png
  capture_settings_panel wellbeing           "Wellbeing"             99-settings-wellbeing-dark.png
  capture_settings_panel security                "Security"                   107-settings-security-dark.png
  capture_settings_panel desktop-dock            "Desktop & Dock"             108-settings-desktop-dock-dark.png
  capture_settings_panel menu-bar-control-center "Menu Bar & Control Center"  109-settings-menu-bar-dark.png

  if [ "$had_theme" -eq 1 ]; then
    export GOBLINS_OS_THEME="$previous_theme"
  else
    unset GOBLINS_OS_THEME
  fi
}

capture_settings_surface() {
  seed_first_boot_profile cloud-openai
  capture_settings_light_surface
  capture_settings_dark_surface
}

capture_settings_search_interaction() {
  seed_first_boot_profile cloud-openai
  cleanup_app_windows
  dbus-run-session -- /usr/libexec/goblins-os/goblins-os-settings &

  local wid=""
  wid=$(wait_for_exact_title "Goblins OS Settings" || true)
  if [ -z "$wid" ]; then
    echo "RENDER-FAILED 100-settings-search-wifi-filter.png (Goblins OS Settings) no visible exact-title window" >&2
    cleanup_app_windows
    return 1
  fi

  xdotool windowactivate "$wid" 2>/dev/null || true
  xdotool windowfocus "$wid" 2>/dev/null || true
  sleep 0.8
  xdotool key --clearmodifiers --window "$wid" ctrl+f
  sleep 0.3
  xdotool type --clearmodifiers --window "$wid" "wi fi"
  sleep 0.8
  capture_existing_window "$wid" 100-settings-search-wifi-filter.png "Goblins OS Settings search wi fi"

  xdotool windowactivate "$wid" 2>/dev/null || true
  xdotool windowfocus "$wid" 2>/dev/null || true
  sleep 0.4
  # The search bar is already open and focused with the "wi fi" query from the
  # capture above (a passive screenshot doesn't steal focus). A second ctrl+f would
  # TOGGLE the search bar closed, so just press Enter to activate the entry, which
  # navigates to the strongest result (Network) and retitles the window in place.
  xdotool key --clearmodifiers --window "$wid" Return
  # Navigation happens IN PLACE: the same window's title and body switch to Network.
  # Capture that window directly (robust under Xvfb focus quirks) and report whether
  # the title confirms the switch — without aborting the whole interactions scope.
  local network_wid=""
  network_wid=$(wait_for_exact_title "Goblins OS Settings - Network" || true)
  sleep 1.0
  if [ -n "$network_wid" ]; then
    capture_existing_window "$network_wid" 101-settings-search-enter-network.png "Goblins OS Settings - Network from search Enter"
  else
    echo "RENDER-WARNING 101-settings-search-enter-network.png: window title did not confirm Network; capturing the in-place navigated window" >&2
    capture_existing_window "$wid" 101-settings-search-enter-network.png "Goblins OS Settings search Enter (in place)"
  fi
  cleanup_app_windows
}

capture_settings_search_recovery_interaction() {
  seed_first_boot_profile cloud-openai
  cleanup_app_windows
  dbus-run-session -- /usr/libexec/goblins-os/goblins-os-settings &

  local wid=""
  wid=$(wait_for_exact_title "Goblins OS Settings" || true)
  if [ -z "$wid" ]; then
    echo "RENDER-FAILED 102-settings-search-no-results.png (Goblins OS Settings) no visible exact-title window" >&2
    cleanup_app_windows
    return 1
  fi

  xdotool windowactivate "$wid" 2>/dev/null || true
  xdotool windowfocus "$wid" 2>/dev/null || true
  sleep 0.8
  xdotool key --clearmodifiers --window "$wid" ctrl+f
  sleep 0.6
  xdotool type --clearmodifiers --window "$wid" "zzzz unmatched"
  sleep 0.8
  capture_existing_window "$wid" 102-settings-search-no-results.png "Goblins OS Settings search no results"

  xdotool key --clearmodifiers --window "$wid" Escape
  sleep 0.8
  capture_existing_window "$wid" 103-settings-search-cleared.png "Goblins OS Settings search cleared"
  cleanup_app_windows
}

capture_settings_firewall_toggle_interaction() {
  seed_first_boot_profile cloud-openai
  cleanup_app_windows
  dbus-run-session -- /usr/libexec/goblins-os/goblins-os-settings --panel=security &

  local wid=""
  wid=$(wait_for_exact_title "Goblins OS Settings - Security" || true)
  if [ -z "$wid" ]; then
    echo "RENDER-FAILED 118-settings-firewall-before.png (Goblins OS Settings - Security) no visible exact-title window" >&2
    cleanup_app_windows
    return 1
  fi

  xdotool windowactivate "$wid" 2>/dev/null || true
  xdotool windowfocus "$wid" 2>/dev/null || true
  local geometry
  local width=1180
  local height=760
  geometry="$(xdotool getwindowgeometry --shell "$wid" 2>/dev/null || true)"
  if [ -n "$geometry" ]; then
    eval "$geometry"
    width="${WIDTH:-$width}"
    height="${HEIGHT:-$height}"
  fi

  local content_x=$((width - 360))
  local content_y=$((height / 2))
  xdotool mousemove --window "$wid" "$content_x" "$content_y"
  for _ in $(seq 1 4); do
    xdotool click 5
    sleep 0.1
  done
  sleep 1.0
  capture_existing_window "$wid" 118-settings-firewall-before.png "Goblins OS Settings firewall toggle ready"

  # The Security panel is rendered at a fixed size in Xvfb. After scrolling the
  # main pane to Network protection, click the switch and capture the honest
  # non-systemd failure and switch reversion from the real /v1/firewall/enabled
  # route.
  local click_x=$((width - 132))
  local click_y=$((height / 2 + 130))
  xdotool mousemove --window "$wid" "$click_x" "$click_y"
  sleep 0.2
  xdotool click 1
  sleep 1.2
  capture_existing_window "$wid" 119-settings-firewall-toggle-failed.png "Goblins OS Settings firewall toggle failure"
  cleanup_app_windows
}

capture_settings_interactions() {
  capture_settings_search_interaction
  capture_settings_search_recovery_interaction
  capture_settings_firewall_toggle_interaction
}

seed_focus_render_state() {
  gsettings set org.goblins.os.focus modes '[{"id":"work","name":"Deep Work"}]'
  gsettings set org.goblins.os.focus active-mode work
}

clear_focus_render_state() {
  gsettings set org.goblins.os.focus active-mode '' 2>/dev/null || true
  gsettings set org.goblins.os.focus modes '[]' 2>/dev/null || true
}

capture_chrome_surface() {
  seed_first_boot_profile cloud-openai
  curl -s -X POST http://127.0.0.1:8787/v1/session/unlock \
    -H 'content-type: application/json' -d '{"mode":"local-gpt-oss"}' >/dev/null 2>&1 || true
  sleep 0.5

  export GOBLINS_OS_RENDER_HOLD_WINDOW=1
  export GOBLINS_OS_RENDER_QUERY="time"
  capture goblins-os-launcher  "Goblins OS Launcher"        35-launcher.png
  export GOBLINS_OS_RENDER_QUERY="240 / 4"
  capture goblins-os-launcher  "Goblins OS Launcher"        36-launcher-answer.png
  unset GOBLINS_OS_RENDER_QUERY
  unset GOBLINS_OS_RENDER_HOLD_WINDOW

  export GOBLINS_OS_RENDER_HOLD_WINDOW=1
  capture goblins-os-control-center "Goblins OS Control Center" 37-control-center.png
  unset GOBLINS_OS_RENDER_HOLD_WINDOW
  seed_focus_render_state
  export GOBLINS_OS_RENDER_HOLD_WINDOW=1
  capture goblins-os-control-center "Goblins OS Control Center" 37b-control-center-focus.png
  unset GOBLINS_OS_RENDER_HOLD_WINDOW
  clear_focus_render_state

  export GOBLINS_OS_THEME=dark
  export GOBLINS_OS_RENDER_HOLD_WINDOW=1
  export GOBLINS_OS_RENDER_QUERY="time"
  capture goblins-os-launcher  "Goblins OS Launcher"        38-launcher-dark.png
  unset GOBLINS_OS_RENDER_QUERY
  unset GOBLINS_OS_RENDER_HOLD_WINDOW
  export GOBLINS_OS_RENDER_HOLD_WINDOW=1
  capture goblins-os-control-center "Goblins OS Control Center" 39-control-center-dark.png
  unset GOBLINS_OS_RENDER_HOLD_WINDOW
  seed_focus_render_state
  export GOBLINS_OS_RENDER_HOLD_WINDOW=1
  capture goblins-os-control-center "Goblins OS Control Center" 39b-control-center-focus-dark.png
  unset GOBLINS_OS_RENDER_HOLD_WINDOW
  clear_focus_render_state
  unset GOBLINS_OS_THEME
}

if [ "$RENDER_SCOPE" = "settings" ]; then
  capture_settings_surface
  kill "$XVFB_PID" "$CORE_PID" "$RES_PID" 2>/dev/null || true
  echo "=== captured artifacts ==="
  ls -la "$OUT"
  exit 0
fi

if [ "$RENDER_SCOPE" = "settings-interactions" ]; then
  capture_settings_interactions
  kill "$XVFB_PID" "$CORE_PID" "$RES_PID" 2>/dev/null || true
  echo "=== captured artifacts ==="
  ls -la "$OUT"
  exit 0
fi

if [ "$RENDER_SCOPE" = "chrome" ]; then
  capture_chrome_surface
  kill "$XVFB_PID" "$CORE_PID" "$RES_PID" 2>/dev/null || true
  echo "=== captured artifacts ==="
  ls -la "$OUT"
  exit 0
fi

if [ "$RENDER_SCOPE" = "installer" ]; then
  capture goblins-os-installer "Goblins OS Setup"    01-installer.png
  export GOBLINS_OS_INSTALLER_PAGE=appearance
  capture goblins-os-installer "Goblins OS Setup"    40-setup-appearance.png
  export GOBLINS_OS_INSTALLER_PAGE=accessibility
  capture goblins-os-installer "Goblins OS Setup"    41-setup-accessibility.png
  export GOBLINS_OS_INSTALLER_PAGE=first-app
  capture goblins-os-installer "Goblins OS Setup"    42-setup-first-app.png
  export GOBLINS_OS_INSTALLER_PAGE=network
  capture goblins-os-installer "Goblins OS Setup"    06-network.png
  export GOBLINS_OS_INSTALLER_PAGE=install-disk
  capture goblins-os-installer "Goblins OS Setup"    12-install-disk.png
  capture goblins-os-installer "Goblins OS Setup"    27-dual-boot-preserve-existing-os.png
  export GOBLINS_OS_INSTALLER_PAGE=install-review
  capture goblins-os-installer "Goblins OS Setup"    13-install-review.png
  export GOBLINS_OS_INSTALLER_PAGE=install-confirm
  capture goblins-os-installer "Goblins OS Setup"    14-install-confirm.png
  export GOBLINS_OS_INSTALLER_PAGE=install-progress
  capture goblins-os-installer "Goblins OS Setup"    15-install-progress.png
  export GOBLINS_OS_INSTALLER_PAGE=install-done
  capture goblins-os-installer "Goblins OS Setup"    16-install-done.png
  export GOBLINS_OS_INSTALLER_PAGE=details
  capture goblins-os-installer "Goblins OS Setup"    18-installer-details.png

  export GOBLINS_OS_THEME=dark
  unset GOBLINS_OS_INSTALLER_PAGE
  capture goblins-os-installer "Goblins OS Setup"    24-installer-dark.png
  export GOBLINS_OS_INSTALLER_PAGE=appearance
  capture goblins-os-installer "Goblins OS Setup"    43-setup-appearance-dark.png
  export GOBLINS_OS_INSTALLER_PAGE=accessibility
  capture goblins-os-installer "Goblins OS Setup"    44-setup-accessibility-dark.png
  export GOBLINS_OS_INSTALLER_PAGE=first-app
  capture goblins-os-installer "Goblins OS Setup"    45-setup-first-app-dark.png
  export GOBLINS_OS_INSTALLER_PAGE=network
  capture goblins-os-installer "Goblins OS Setup"    26-network-dark.png
  export GOBLINS_OS_INSTALLER_PAGE=install-disk
  capture goblins-os-installer "Goblins OS Setup"    28-install-disk-dark.png
  export GOBLINS_OS_INSTALLER_PAGE=install-review
  capture goblins-os-installer "Goblins OS Setup"    29-install-review-dark.png
  export GOBLINS_OS_INSTALLER_PAGE=install-confirm
  capture goblins-os-installer "Goblins OS Setup"    17-install-confirm-dark.png
  export GOBLINS_OS_INSTALLER_PAGE=install-progress
  capture goblins-os-installer "Goblins OS Setup"    30-install-progress-dark.png
  export GOBLINS_OS_INSTALLER_PAGE=install-done
  capture goblins-os-installer "Goblins OS Setup"    31-install-done-dark.png
  export GOBLINS_OS_INSTALLER_PAGE=details
  capture goblins-os-installer "Goblins OS Setup"    32-installer-details-dark.png
  unset GOBLINS_OS_INSTALLER_PAGE
  unset GOBLINS_OS_THEME

  kill "$XVFB_PID" "$CORE_PID" "$RES_PID" 2>/dev/null || true
  echo "=== captured artifacts ==="
  ls -la "$OUT"
  exit 0
fi

# Order matters: capture the exact-title apps before the bare "Goblins OS" shell
# so the substring match never grabs the wrong (already-closed) window.
capture goblins-os-installer "Goblins OS Setup"    01-installer.png
export GOBLINS_OS_INSTALLER_PAGE=appearance
capture goblins-os-installer "Goblins OS Setup"    40-setup-appearance.png
export GOBLINS_OS_INSTALLER_PAGE=accessibility
capture goblins-os-installer "Goblins OS Setup"    41-setup-accessibility.png
export GOBLINS_OS_INSTALLER_PAGE=first-app
capture goblins-os-installer "Goblins OS Setup"    42-setup-first-app.png
unset GOBLINS_OS_INSTALLER_PAGE
# The first-boot network step (a Stack page); ask the installer to open on it.
export GOBLINS_OS_INSTALLER_PAGE=network
capture goblins-os-installer "Goblins OS Setup"    06-network.png
unset GOBLINS_OS_INSTALLER_PAGE

# The native "Install Goblins OS to this computer" flow — the Anaconda replacement.
# Each page is forced via GOBLINS_OS_INSTALLER_PAGE; pages past disk preselect the
# first eligible disk so the captured device path is real. Nothing is clicked and
# the destructive env gate is unset, so no disk is ever touched.
export GOBLINS_OS_INSTALLER_PAGE=install-disk
capture goblins-os-installer "Goblins OS Setup"    12-install-disk.png
# Packaging-time dual-boot proof: the Docker sys-block fixture makes the
# Open advanced storage handoff visible. The hardware gate still requires
# a separate display-backed screenshot under os/screenshots/hardware-gate/<arch>/.
export GOBLINS_OS_INSTALLER_PAGE=install-disk
capture goblins-os-installer "Goblins OS Setup"    27-dual-boot-preserve-existing-os.png
export GOBLINS_OS_INSTALLER_PAGE=install-review
capture goblins-os-installer "Goblins OS Setup"    13-install-review.png
export GOBLINS_OS_INSTALLER_PAGE=install-confirm
capture goblins-os-installer "Goblins OS Setup"    14-install-confirm.png
export GOBLINS_OS_INSTALLER_PAGE=install-progress
capture goblins-os-installer "Goblins OS Setup"    15-install-progress.png
export GOBLINS_OS_INSTALLER_PAGE=install-done
capture goblins-os-installer "Goblins OS Setup"    16-install-done.png
# The "Advanced setup" advanced page — a denser surface (account, readiness,
# install target, local models) that the calm welcome hides behind one link.
export GOBLINS_OS_INSTALLER_PAGE=details
capture goblins-os-installer "Goblins OS Setup"    18-installer-details.png
unset GOBLINS_OS_INSTALLER_PAGE

# Render the real login gate after first boot exists but before any session gate
# unlock is persisted. The profile is the same OS-owned JSON contract that
# goblins-os-core reads from its supported GOBLINS_OS_INSTALLER_STATE path.
seed_first_boot_profile cloud-openai
capture goblins-os-login     "Goblins OS Login"    02-login.png
# Login in Dark — captured here, while the session is still LOCKED. The dark block
# below runs after the session unlock, where the login gate has nothing to render.
export GOBLINS_OS_THEME=dark
capture goblins-os-login     "Goblins OS Login"    25-login-dark.png
unset GOBLINS_OS_THEME
capture_settings_light_surface

# Unlock the session in local-only mode so the shell renders the real desktop
# (launcher + workspace + resident strip), not just the first-boot lock screen.
# This also exercises the OS session-unlock path end to end during the render.
curl -s -X POST http://127.0.0.1:8787/v1/session/unlock \
  -H 'content-type: application/json' -d '{"mode":"local-gpt-oss"}' >/dev/null 2>&1 || true
sleep 0.5

capture goblins-os-shell     "Goblins OS"          04-shell.png
capture goblins-os-shell     "Goblins OS Text Shortcuts Proof" 120-text-shortcuts-candidate.png --text-shortcuts-proof candidate

# Build Studio through the real deep-link entrypoint. If the core has no saved
# build sessions this truthfully renders the empty new-build state.
capture goblins-os-shell     "Goblins OS"          08-studio.png --studio

# The Command-Space launcher. The render query only pre-types the search field;
# it never fabricates apps, answers, or runtime state.
export GOBLINS_OS_RENDER_HOLD_WINDOW=1
export GOBLINS_OS_RENDER_QUERY="time"
capture goblins-os-launcher  "Goblins OS Launcher"        35-launcher.png
export GOBLINS_OS_RENDER_QUERY="240 / 4"
capture goblins-os-launcher  "Goblins OS Launcher"        36-launcher-answer.png
unset GOBLINS_OS_RENDER_QUERY
unset GOBLINS_OS_RENDER_HOLD_WINDOW

# The control center (menu-bar quick settings). Headless containers report
# unavailable hardware truthfully rather than showing representative values.
export GOBLINS_OS_RENDER_HOLD_WINDOW=1
capture goblins-os-control-center "Goblins OS Control Center" 37-control-center.png
unset GOBLINS_OS_RENDER_HOLD_WINDOW
seed_focus_render_state
export GOBLINS_OS_RENDER_HOLD_WINDOW=1
capture goblins-os-control-center "Goblins OS Control Center" 37b-control-center-focus.png
unset GOBLINS_OS_RENDER_HOLD_WINDOW
clear_focus_render_state

# The Today panel reads the installed core route and renders real local Date/Clock
# values plus honest empty states for weather, calendar, and the daily brief.
capture goblins-os-today "Today" 122-today.png

# Dark theme — the OS is not locked to one scheme. The same surfaces in Dark,
# proving Light/Dark/Auto themes the whole OS (chrome and the Build Studio).
export GOBLINS_OS_THEME=dark
capture goblins-os-shell     "Goblins OS"          09-shell-dark.png
capture goblins-os-shell     "Goblins OS Text Shortcuts Proof" 121-text-shortcuts-candidate-dark.png --text-shortcuts-proof candidate
capture goblins-os-shell     "Goblins OS"          10-studio-dark.png --studio
capture goblins-os-today     "Today"               123-today-dark.png
# The destructive confirmation in Dark — proving the install flow themes with the
# rest of the OS (the typed-acknowledgement hero in both schemes).
clear_first_boot_profile
export GOBLINS_OS_INSTALLER_PAGE=install-confirm
capture goblins-os-installer "Goblins OS Setup"    17-install-confirm-dark.png
unset GOBLINS_OS_INSTALLER_PAGE

# Full dark coverage of the remaining surfaces, so EVERY surface is a Light+Dark
# design proof (not just a representative subset). First-boot installer pages:
capture goblins-os-installer "Goblins OS Setup"    24-installer-dark.png
export GOBLINS_OS_INSTALLER_PAGE=appearance
capture goblins-os-installer "Goblins OS Setup"    43-setup-appearance-dark.png
export GOBLINS_OS_INSTALLER_PAGE=accessibility
capture goblins-os-installer "Goblins OS Setup"    44-setup-accessibility-dark.png
export GOBLINS_OS_INSTALLER_PAGE=first-app
capture goblins-os-installer "Goblins OS Setup"    45-setup-first-app-dark.png
unset GOBLINS_OS_INSTALLER_PAGE
export GOBLINS_OS_INSTALLER_PAGE=network
capture goblins-os-installer "Goblins OS Setup"    26-network-dark.png
export GOBLINS_OS_INSTALLER_PAGE=install-disk
capture goblins-os-installer "Goblins OS Setup"    28-install-disk-dark.png
export GOBLINS_OS_INSTALLER_PAGE=install-review
capture goblins-os-installer "Goblins OS Setup"    29-install-review-dark.png
export GOBLINS_OS_INSTALLER_PAGE=install-progress
capture goblins-os-installer "Goblins OS Setup"    30-install-progress-dark.png
export GOBLINS_OS_INSTALLER_PAGE=install-done
capture goblins-os-installer "Goblins OS Setup"    31-install-done-dark.png
export GOBLINS_OS_INSTALLER_PAGE=details
capture goblins-os-installer "Goblins OS Setup"    32-installer-details-dark.png
unset GOBLINS_OS_INSTALLER_PAGE

# Full dark Settings coverage (login-dark is captured earlier, pre-unlock):
capture_settings_dark_surface

# The launcher + control center in Dark — the bespoke chrome themes with the OS.
export GOBLINS_OS_RENDER_HOLD_WINDOW=1
export GOBLINS_OS_RENDER_QUERY="time"
capture goblins-os-launcher  "Goblins OS Launcher"        38-launcher-dark.png
unset GOBLINS_OS_RENDER_QUERY
unset GOBLINS_OS_RENDER_HOLD_WINDOW
export GOBLINS_OS_RENDER_HOLD_WINDOW=1
capture goblins-os-control-center "Goblins OS Control Center" 39-control-center-dark.png
unset GOBLINS_OS_RENDER_HOLD_WINDOW
seed_focus_render_state
export GOBLINS_OS_RENDER_HOLD_WINDOW=1
capture goblins-os-control-center "Goblins OS Control Center" 39b-control-center-focus-dark.png
unset GOBLINS_OS_RENDER_HOLD_WINDOW
clear_focus_render_state
unset GOBLINS_OS_THEME

kill "$XVFB_PID" "$CORE_PID" "$RES_PID" 2>/dev/null || true
echo "=== captured artifacts ==="
ls -la "$OUT"
