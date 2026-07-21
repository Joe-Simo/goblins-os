#!/usr/bin/env bash
# Render the native Goblins OS GTK apps headlessly inside the real OS image and
# capture a screenshot of each window. This is a packaging-time design proof:
# it runs the actual installed binaries against the actual OS daemon, so the
# captured pixels are the installed first-boot UI, not a storyboard.
set -euo pipefail

OUT=/out
mkdir -p "$OUT"
CORE_PROOF_SOCKET=/run/goblins-os-core/release-proof/control.sock
CORE_PROOF_URL=http://localhost
RENDER_SCOPE="${GOBLINS_OS_RENDER_SCOPE:-all}"
case "$RENDER_SCOPE" in
  all|chrome|installer|settings|settings-interactions|polish-interactions) ;;
  *)
    echo "RENDER-FAILED unsupported GOBLINS_OS_RENDER_SCOPE=$RENDER_SCOPE (expected all, chrome, installer, settings, settings-interactions, or polish-interactions)" >&2
    exit 2
    ;;
esac

export XDG_RUNTIME_DIR=/tmp/xdg
install -d -m 0700 -o goblin -g goblin "$XDG_RUNTIME_DIR"
# A real boot initializes the deliberately empty image machine-id before any
# desktop D-Bus or dconf work. Reproduce that lifecycle inside this disposable
# render layer so preference-driven accessibility proof uses the real backend.
systemd-machine-id-setup >/dev/null
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
export GOBLINS_OS_POLICY_STATE="$GOBLINS_OS_RENDER_STATE_DIR/policy"
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
INTERACTION_WID=""

core_proof_curl() {
  setpriv --regid=goblins-core-release-proof --clear-groups -- \
    curl --unix-socket "$CORE_PROOF_SOCKET" "$@"
}

# Exercise the installed setgid capability boundary exactly as the real desktop
# session does. Running a UI binary as root is intentionally rejected because
# root owns writable socket ancestors; the human desktop user must consume the
# one-time per-app group capability before GTK initializes.
desktop_user_command() {
  setpriv --reuid=goblin --regid=goblin --init-groups -- \
    env HOME=/var/home/goblin USER=goblin LOGNAME=goblin \
      XDG_RUNTIME_DIR="$XDG_RUNTIME_DIR" "$@"
}

run_desktop_app() {
  desktop_user_command dbus-run-session -- "$@"
}

desktop_gsettings() {
  # The image intentionally does not ship dbus-launch. Use the supported
  # session runner so dconf writes the real per-user settings database, which
  # later application sessions read from the same home directory.
  desktop_user_command dbus-run-session -- gsettings "$@"
}

desktop_uid_has_live_processes() {
  local desktop_uid="$1"

  ps -eo uid=,stat= | awk -v desktop_uid="$desktop_uid" '
    $1 == desktop_uid && $2 !~ /^Z/ { live = 1 }
    END { exit(live ? 0 : 1) }
  '
}

cleanup_app_windows() {
  local desktop_uid
  desktop_uid="$(id -u goblin)"

  # Every capture owns a disposable desktop-user session: the app, its private
  # D-Bus daemon, accessibility services, and any portal processes activated by
  # GTK. Terminate that complete identity boundary between frames instead of
  # matching executable paths. This also follows the installed setgid launcher
  # when it re-execs the root-owned payload from /usr/libexec/goblins-os/ui.
  pkill -TERM -u "$desktop_uid" 2>/dev/null || true
  for _ in $(seq 1 20); do
    if ! desktop_uid_has_live_processes "$desktop_uid"; then
      sleep 0.1
      return 0
    fi
    sleep 0.05
  done

  # A wedged portal must not leak X11 clients into the next proof frame.
  pkill -KILL -u "$desktop_uid" 2>/dev/null || true
  for _ in $(seq 1 20); do
    if ! desktop_uid_has_live_processes "$desktop_uid"; then
      sleep 0.1
      return 0
    fi
    sleep 0.05
  done

  echo "RENDER-FAILED desktop session processes survived forced cleanup" >&2
  ps -u "$desktop_uid" -o pid=,ppid=,stat=,comm= >&2 || true
  return 1
}

shutdown_render_services() {
  cleanup_app_windows || true
  [ -n "$XVFB_PID" ] && kill "$XVFB_PID" 2>/dev/null || true
  [ -n "$CORE_PID" ] && kill "$CORE_PID" 2>/dev/null || true
  [ -n "$RES_PID" ] && kill "$RES_PID" 2>/dev/null || true
}

wait_for_core_health() {
  for _ in $(seq 1 50); do
    if core_proof_curl -sf "$CORE_PROOF_URL/health" >/dev/null 2>&1; then
      echo "core daemon is healthy"
      return 0
    fi
    sleep 0.2
  done

  echo "RENDER-FAILED core daemon did not become healthy" >&2
  return 1
}

start_core() {
  if [ -n "$CORE_PID" ] && kill -0 "$CORE_PID" 2>/dev/null; then
    return 0
  fi

  systemd-tmpfiles --create /usr/lib/tmpfiles.d/goblins-os-core.conf
  install -d -m 0750 -o goblins-os -g goblins-os "$GOBLINS_OS_RENDER_STATE_DIR"
  setpriv --reuid=goblins-os --regid=goblins-os --init-groups -- \
    /usr/libexec/goblins-os/goblins-os-core &
  CORE_PID=$!
  wait_for_core_health
}

stop_core() {
  if [ -n "$CORE_PID" ]; then
    kill "$CORE_PID" 2>/dev/null || true
    wait "$CORE_PID" 2>/dev/null || true
    CORE_PID=""
  fi

  for _ in $(seq 1 30); do
    if ! core_proof_curl -sf "$CORE_PROOF_URL/health" >/dev/null 2>&1; then
      return 0
    fi
    sleep 0.1
  done

  echo "RENDER-FAILED core daemon remained reachable after stop" >&2
  return 1
}

seed_first_boot_profile() {
  local mode="$1"

  install -d -m 0750 -o goblins-os -g goblins-os \
    "$GOBLINS_OS_INSTALLER_STATE" "$GOBLINS_OS_SESSION_STATE"
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
start_core
install -d -m 0750 -o goblins-resident -g goblins-core-resident \
  /run/goblins-os /var/lib/goblins-os/resident
GOBLINS_OS_RESIDENT_STATE=/var/lib/goblins-os/resident \
GOBLINS_OS_RESIDENT_SOCKET=/run/goblins-os/resident.sock \
  setpriv --reuid=goblins-resident --regid=goblins-core-resident --init-groups -- \
    /usr/libexec/goblins-os/goblins-os-resident &
RES_PID=$!

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
  run_desktop_app /usr/libexec/goblins-os/"$bin" "$@" &
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

capture_window_region_with_popovers() {
  local wid="$1" out="$2" title="$3" geometry x y width height
  local parent_pid parent_capture transient_capture transient_mask transient_alpha composited_capture
  local candidate candidate_geometry candidate_x candidate_y candidate_width candidate_height
  local relative_x relative_y relative_x_offset relative_y_offset alpha_state
  local transient_count=0
  local -a candidate_windows=()

  geometry="$(xdotool getwindowgeometry --shell "$wid" 2>/dev/null)" || {
    echo "RENDER-FAILED $out ($title) could not read window geometry" >&2
    return 1
  }
  x="$(printf '%s\n' "$geometry" | awk -F= '$1 == "X" { print $2; exit }')"
  y="$(printf '%s\n' "$geometry" | awk -F= '$1 == "Y" { print $2; exit }')"
  width="$(printf '%s\n' "$geometry" | awk -F= '$1 == "WIDTH" { print $2; exit }')"
  height="$(printf '%s\n' "$geometry" | awk -F= '$1 == "HEIGHT" { print $2; exit }')"
  if [[ ! "$x" =~ ^-?[0-9]+$ || ! "$y" =~ ^-?[0-9]+$ || ! "$width" =~ ^[1-9][0-9]*$ || ! "$height" =~ ^[1-9][0-9]*$ ]]; then
    echo "RENDER-FAILED $out ($title) returned invalid window geometry" >&2
    return 1
  fi
  parent_capture="$(mktemp --suffix=.png /tmp/goblins-render-parent.XXXXXX)"
  if ! import -window "$wid" "$parent_capture"; then
    rm -f "$parent_capture"
    echo "RENDER-FAILED $out ($title) could not capture parent window" >&2
    return 1
  fi

  parent_pid="$(xdotool getwindowpid "$wid" 2>/dev/null || true)"
  if [[ ! "$parent_pid" =~ ^[1-9][0-9]*$ ]]; then
    rm -f "$parent_capture"
    echo "RENDER-FAILED $out ($title) could not resolve parent window process" >&2
    return 1
  fi
  mapfile -t candidate_windows < <(
    xdotool search --onlyvisible --pid "$parent_pid" 2>/dev/null || true
  )

  # GTK popovers are separate ARGB X11 surfaces. Direct Xvfb root capture drops
  # their alpha channel and exposes the surface's contiguous black backing.
  # Capture each real transient separately, recover only that connected backing
  # as transparency, and composite the native pixels over the parent window.
  # This preserves rounded popover geometry without inventing UI pixels.
  for candidate in "${candidate_windows[@]}"; do
    [ "$candidate" = "$wid" ] && continue
    candidate_geometry="$(xdotool getwindowgeometry --shell "$candidate" 2>/dev/null || true)"
    candidate_x="$(printf '%s\n' "$candidate_geometry" | awk -F= '$1 == "X" { print $2; exit }')"
    candidate_y="$(printf '%s\n' "$candidate_geometry" | awk -F= '$1 == "Y" { print $2; exit }')"
    candidate_width="$(printf '%s\n' "$candidate_geometry" | awk -F= '$1 == "WIDTH" { print $2; exit }')"
    candidate_height="$(printf '%s\n' "$candidate_geometry" | awk -F= '$1 == "HEIGHT" { print $2; exit }')"
    if [[ ! "$candidate_x" =~ ^-?[0-9]+$ || ! "$candidate_y" =~ ^-?[0-9]+$ \
        || ! "$candidate_width" =~ ^[1-9][0-9]*$ || ! "$candidate_height" =~ ^[1-9][0-9]*$ ]]; then
      rm -f "$parent_capture"
      echo "RENDER-FAILED $out ($title) returned invalid transient geometry" >&2
      return 1
    fi
    if (( candidate_x + candidate_width <= x || candidate_x >= x + width \
          || candidate_y + candidate_height <= y || candidate_y >= y + height )); then
      continue
    fi

    transient_capture="$(mktemp --suffix=.png /tmp/goblins-render-transient.XXXXXX)"
    transient_mask="$(mktemp --suffix=.png /tmp/goblins-render-transient-mask.XXXXXX)"
    transient_alpha="$(mktemp --suffix=.png /tmp/goblins-render-transient-alpha.XXXXXX)"
    composited_capture="$(mktemp --suffix=.png /tmp/goblins-render-composited.XXXXXX)"
    if ! import -window "$candidate" "$transient_capture"; then
      rm -f "$parent_capture" "$transient_capture" "$transient_mask" "$transient_alpha" "$composited_capture"
      echo "RENDER-FAILED $out ($title) could not capture transient window" >&2
      return 1
    fi
    # Build a binary near-black map, mark only the region connected to the
    # surface corner, then convert that one region to alpha. Dark-theme cards
    # and dark text remain opaque because they are enclosed product pixels,
    # not connected Xvfb backing.
    magick "$transient_capture" \
      -colorspace gray -threshold 9% -fill gray50 \
      -draw 'color 0,0 floodfill' -fx 'abs(u-0.5)<0.02?0:1' \
      "$transient_mask"
    magick "$transient_capture" "$transient_mask" \
      -alpha off -compose CopyOpacity -composite "$transient_alpha"
    alpha_state="$(magick identify -format '%[opaque]' "$transient_alpha")"
    if [ "$alpha_state" != "False" ]; then
      rm -f "$parent_capture" "$transient_capture" "$transient_mask" "$transient_alpha" "$composited_capture"
      echo "RENDER-FAILED $out ($title) transient alpha reconstruction stayed opaque" >&2
      return 1
    fi

    relative_x=$((candidate_x - x))
    relative_y=$((candidate_y - y))
    if [ "$relative_x" -ge 0 ]; then relative_x_offset="+$relative_x"; else relative_x_offset="$relative_x"; fi
    if [ "$relative_y" -ge 0 ]; then relative_y_offset="+$relative_y"; else relative_y_offset="$relative_y"; fi
    magick "$parent_capture" "$transient_alpha" \
      -geometry "${relative_x_offset}${relative_y_offset}" \
      -compose over -composite "$composited_capture"
    mv "$composited_capture" "$parent_capture"
    rm -f "$transient_capture" "$transient_mask" "$transient_alpha"
    transient_count=$((transient_count + 1))
  done

  mv "$parent_capture" "$OUT/$out"
  chmod 0644 "$OUT/$out"
  echo "RENDERED $out ($title, composited_transients=$transient_count) wid=$wid"
}

start_interaction_window() {
  local bin="$1" title="$2"
  shift 2

  cleanup_app_windows
  run_desktop_app /usr/libexec/goblins-os/"$bin" "$@" &
  INTERACTION_WID="$(wait_for_exact_title "$title" || true)"
  sleep 2.5
  if [ -z "$INTERACTION_WID" ]; then
    echo "RENDER-FAILED interaction window ($title) was not visible" >&2
    cleanup_app_windows
    return 1
  fi
  xdotool windowactivate "$INTERACTION_WID" 2>/dev/null || true
  xdotool windowfocus "$INTERACTION_WID" 2>/dev/null || true
}

interaction_window_size() {
  local wid="$1" key="$2" fallback="$3"
  local geometry value

  geometry="$(xdotool getwindowgeometry --shell "$wid" 2>/dev/null || true)"
  value="$(printf '%s\n' "$geometry" | awk -F= -v key="$key" '$1 == key { print $2; exit }')"
  if [ -n "$value" ]; then
    printf '%s\n' "$value"
  else
    printf '%s\n' "$fallback"
  fi
}

capture_settings_panel() {
  local slug="$1" title="$2" out="$3"
  capture goblins-os-settings "Goblins OS Settings - ${title}" "$out" --panel="$slug"
}

capture_settings_light_surface() {
  capture goblins-os-settings  "Goblins OS Settings" 03-settings.png
  # The Models panel is the focused home for GPT-OSS, Codex, and the status of
  # administrator-provisioned hosted access; render it as its own design proof. Its window
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
  # Models panel in Dark — proves the administrator-provisioned hosted-access
  # disclosure themes like every other surface without exposing credential input.
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
  run_desktop_app /usr/libexec/goblins-os/goblins-os-settings &

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
  run_desktop_app /usr/libexec/goblins-os/goblins-os-settings &

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
  run_desktop_app /usr/libexec/goblins-os/goblins-os-settings --panel=security &

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

image_absolute_error_pixels() {
  local left="$1" right="$2" left_signature right_signature left_geometry right_geometry metric

  left_signature="$(magick identify -format '%wx%h|%[colorspace]|%[channels]|%[opaque]' "$left" 2>/dev/null)" || {
    echo "RENDER-FAILED could not identify comparison image $left" >&2
    return 1
  }
  right_signature="$(magick identify -format '%wx%h|%[colorspace]|%[channels]|%[opaque]' "$right" 2>/dev/null)" || {
    echo "RENDER-FAILED could not identify comparison image $right" >&2
    return 1
  }
  left_geometry="${left_signature%%|*}"
  right_geometry="${right_signature%%|*}"
  if [ "$left_geometry" != "$right_geometry" ]; then
    echo "RENDER-FAILED comparison image dimensions differ: $left=$left_geometry $right=$right_geometry" >&2
    return 1
  fi
  if [ "$left_signature" != "$right_signature" ] || [[ "$left_signature" != *'|True' ]]; then
    echo "RENDER-FAILED comparison images must use the same opaque color layout: $left=$left_signature $right=$right_signature" >&2
    return 1
  fi

  # ImageMagick's HDRI builds may emit the AE quantum-error sum as a decimal or
  # in scientific notation. Build an exact binary difference mask instead, then
  # count its non-zero pixels so the proof remains portable and semantically
  # matches the changed-pixel contract.
  metric="$(magick -precision 20 "$left" "$right" \
    -alpha off -compose difference -composite -colorspace gray -threshold 0 \
    -format '%[fx:round(mean*w*h)]' info: 2>/dev/null)" || {
    echo "RENDER-FAILED could not calculate pixel difference for $left and $right" >&2
    return 1
  }
  if [[ ! "$metric" =~ ^[0-9]+$ ]]; then
    echo "RENDER-FAILED could not calculate pixel difference for $left and $right: ${metric:-missing}" >&2
    return 1
  fi
  printf '%s\n' "$metric"
}

capture_settings_polish_interactions() {
  local width height content_x content_y click_x click_y expanded_difference dark_expanded_difference offline_difference

  export GOBLINS_OS_THEME=light
  seed_first_boot_profile cloud-openai
  start_interaction_window goblins-os-settings "Goblins OS Settings - AI & Models" --panel=models
  width="$(interaction_window_size "$INTERACTION_WID" WIDTH 1055)"
  height="$(interaction_window_size "$INTERACTION_WID" HEIGHT 840)"
  content_x=$((width - 220))
  content_y=$((height / 2))
  xdotool mousemove --window "$INTERACTION_WID" "$content_x" "$content_y"
  for _ in $(seq 1 20); do
    xdotool click 5
    sleep 0.06
  done
  sleep 0.8
  xdotool mousemove 2 2
  capture_existing_window "$INTERACTION_WID" 124-settings-models-advanced-collapsed.png "Settings Models advanced disclosure collapsed"

  # At the bottom of the Models pane the collapsed disclosure settles in the
  # upper quarter of the content column. Use the stable fixed-size Settings
  # window geometry rather than a render-only application hook.
  click_x=$((width / 2 + 65))
  click_y=$((height / 4))
  xdotool mousemove --window "$INTERACTION_WID" "$click_x" "$click_y"
  xdotool click 1
  sleep 1.0
  xdotool mousemove 2 2
  capture_existing_window "$INTERACTION_WID" 125-settings-models-advanced-expanded.png "Settings Models advanced disclosure expanded"
  expanded_difference="$(image_absolute_error_pixels \
    "$OUT/124-settings-models-advanced-collapsed.png" \
    "$OUT/125-settings-models-advanced-expanded.png")"
  if [ "$expanded_difference" -lt 100 ]; then
    echo "RENDER-FAILED Settings advanced disclosure did not produce a substantive expanded state (changed_pixels=$expanded_difference)" >&2
    return 1
  fi
  cleanup_app_windows

  # Repeat the real disclosure interaction under the shared dark-theme override.
  # The temporary collapsed frame is used only to fail closed if the menu did
  # not actually expand; the semantic artifact is the requested expanded state.
  export GOBLINS_OS_THEME=dark
  start_interaction_window goblins-os-settings "Goblins OS Settings - AI & Models" --panel=models
  width="$(interaction_window_size "$INTERACTION_WID" WIDTH 1055)"
  height="$(interaction_window_size "$INTERACTION_WID" HEIGHT 840)"
  content_x=$((width - 220))
  content_y=$((height / 2))
  xdotool mousemove --window "$INTERACTION_WID" "$content_x" "$content_y"
  for _ in $(seq 1 20); do
    xdotool click 5
    sleep 0.06
  done
  sleep 0.8
  xdotool mousemove 2 2
  capture_existing_window "$INTERACTION_WID" .settings-models-dark-collapsed.png "Settings Models dark disclosure collapsed comparison"
  click_x=$((width / 2 + 65))
  click_y=$((height / 4))
  xdotool mousemove --window "$INTERACTION_WID" "$click_x" "$click_y"
  xdotool click 1
  sleep 1.0
  xdotool mousemove 2 2
  capture_existing_window "$INTERACTION_WID" 136-settings-models-advanced-expanded-dark.png "Settings Models advanced disclosure expanded in dark mode"
  dark_expanded_difference="$(image_absolute_error_pixels \
    "$OUT/.settings-models-dark-collapsed.png" \
    "$OUT/136-settings-models-advanced-expanded-dark.png")"
  rm -f "$OUT/.settings-models-dark-collapsed.png"
  if [ "$dark_expanded_difference" -lt 100 ]; then
    echo "RENDER-FAILED Settings dark advanced disclosure did not produce a substantive expanded state (changed_pixels=$dark_expanded_difference)" >&2
    return 1
  fi
  cleanup_app_windows
  export GOBLINS_OS_THEME=light

  # Load authoritative engine state first, then stop the real core and invoke the
  # real selector. This proves the product error state without treating a missing
  # status response as evidence that GPT-OSS is active.
  start_interaction_window goblins-os-settings "Goblins OS Settings - AI & Models" --panel=models
  width="$(interaction_window_size "$INTERACTION_WID" WIDTH 1055)"
  height="$(interaction_window_size "$INTERACTION_WID" HEIGHT 840)"
  xdotool mousemove 2 2
  capture_existing_window "$INTERACTION_WID" .settings-models-engine-online.png "Settings Models online comparison"
  stop_core
  # The selected on-device segment occupies the left half of the selector near
  # the top of the Models pane. Click its stable center so the real action—not
  # nearby explanatory copy—is exercised after the core goes offline.
  xdotool mousemove --window "$INTERACTION_WID" $((width * 49 / 100)) $((height * 41 / 100))
  xdotool click 1
  sleep 1.0
  xdotool mousemove 2 2
  capture_existing_window "$INTERACTION_WID" 126-settings-models-engine-offline-error.png "Settings Models engine selection while core is offline"
  offline_difference="$(image_absolute_error_pixels \
    "$OUT/.settings-models-engine-online.png" \
    "$OUT/126-settings-models-engine-offline-error.png")"
  rm -f "$OUT/.settings-models-engine-online.png"
  if [ "$offline_difference" -lt 100 ]; then
    echo "RENDER-FAILED Settings offline engine selection did not produce a visible error state (changed_pixels=$offline_difference)" >&2
    return 1
  fi
  cleanup_app_windows
  start_core

  SETTINGS_EXPANDED_DIFFERENCE="$expanded_difference"
  SETTINGS_DARK_EXPANDED_DIFFERENCE="$dark_expanded_difference"
  SETTINGS_OFFLINE_ERROR_DIFFERENCE="$offline_difference"
}

capture_studio_polish_interactions() {
  local width height picker_x picker_y option_x option_y menu_difference closed_error_difference dark_menu_difference

  export GOBLINS_OS_THEME=light
  seed_first_boot_profile cloud-openai
  core_proof_curl -sSf -X POST "$CORE_PROOF_URL/v1/session/unlock" \
    -H 'content-type: application/json' -d '{"mode":"local-gpt-oss"}' >/dev/null
  start_interaction_window goblins-os-shell "Goblins OS" --studio
  width="$(interaction_window_size "$INTERACTION_WID" WIDTH 940)"
  height="$(interaction_window_size "$INTERACTION_WID" HEIGHT 700)"
  # The engine picker is anchored in the lower-left of the right-side composer.
  picker_x=$((width * 63 / 100))
  picker_y=$((height * 83 / 100))
  xdotool mousemove 2 2
  capture_window_region_with_popovers "$INTERACTION_WID" .studio-engine-menu-closed.png "Studio engine menu closed comparison"
  xdotool mousemove --window "$INTERACTION_WID" "$picker_x" "$picker_y"
  xdotool click 1
  sleep 0.8
  xdotool mousemove 2 2
  capture_window_region_with_popovers "$INTERACTION_WID" 127-studio-engine-menu.png "Studio explicit engine menu"

  stop_core
  # The popover opens above the lower composer. Exercise its first enabled,
  # on-device option directly; keyboard traversal from a MenuButton closes the
  # popover before entering this plain vertical button list on GTK 4.
  option_x="$picker_x"
  option_y=$((picker_y - height * 34 / 100))
  xdotool mousemove --window "$INTERACTION_WID" "$option_x" "$option_y"
  xdotool click 1
  sleep 1.0
  xdotool mousemove 2 2
  capture_window_region_with_popovers "$INTERACTION_WID" 128-studio-engine-offline-error.png "Studio engine switch while core is offline"
  menu_difference="$(image_absolute_error_pixels \
    "$OUT/127-studio-engine-menu.png" \
    "$OUT/128-studio-engine-offline-error.png")"
  closed_error_difference="$(image_absolute_error_pixels \
    "$OUT/.studio-engine-menu-closed.png" \
    "$OUT/128-studio-engine-offline-error.png")"
  rm -f "$OUT/.studio-engine-menu-closed.png"
  if [ "$menu_difference" -lt 100 ]; then
    echo "RENDER-FAILED Studio offline switch did not produce a visible error state (changed_pixels=$menu_difference)" >&2
    return 1
  fi
  if [ "$closed_error_difference" -lt 100 ]; then
    echo "RENDER-FAILED Studio offline switch collapsed to the ordinary closed state (changed_pixels=$closed_error_difference)" >&2
    return 1
  fi
  cleanup_app_windows
  start_core

  # Prove the same keyboard-focused engine menu with the actual shared dark
  # theme. Compare the closed and open real windows so a missed click fails CI.
  export GOBLINS_OS_THEME=dark
  start_interaction_window goblins-os-shell "Goblins OS" --studio
  width="$(interaction_window_size "$INTERACTION_WID" WIDTH 940)"
  height="$(interaction_window_size "$INTERACTION_WID" HEIGHT 700)"
  picker_x=$((width * 63 / 100))
  picker_y=$((height * 83 / 100))
  xdotool mousemove 2 2
  capture_window_region_with_popovers "$INTERACTION_WID" .studio-dark-engine-menu-closed.png "Studio dark engine menu closed comparison"
  xdotool mousemove --window "$INTERACTION_WID" "$picker_x" "$picker_y"
  xdotool click 1
  sleep 0.8
  xdotool mousemove 2 2
  capture_window_region_with_popovers "$INTERACTION_WID" 137-studio-engine-menu-dark.png "Studio explicit engine menu in dark mode"
  dark_menu_difference="$(image_absolute_error_pixels \
    "$OUT/.studio-dark-engine-menu-closed.png" \
    "$OUT/137-studio-engine-menu-dark.png")"
  rm -f "$OUT/.studio-dark-engine-menu-closed.png"
  if [ "$dark_menu_difference" -lt 100 ]; then
    echo "RENDER-FAILED Studio dark engine menu did not visibly open (changed_pixels=$dark_menu_difference)" >&2
    return 1
  fi
  cleanup_app_windows
  export GOBLINS_OS_THEME=light

  STUDIO_ERROR_DIFFERENCE="$menu_difference"
  STUDIO_ERROR_VS_CLOSED_DIFFERENCE="$closed_error_difference"
  STUDIO_DARK_MENU_DIFFERENCE="$dark_menu_difference"
}

grant_render_app_builder_permission() {
  local profile acknowledgement payload response

  profile="$(core_proof_curl -sSf "$CORE_PROOF_URL/v1/policy/status" | jq -er '.profile')"
  acknowledgement="GRANT GOBLINS OS PERMISSION app-builder FOR $profile"
  payload="$(jq -cn \
    --arg control_id app-builder \
    --arg acknowledgement "$acknowledgement" \
    '{control_id:$control_id, acknowledgement:$acknowledgement}')"
  response="$(core_proof_curl -sSf -X POST "$CORE_PROOF_URL/v1/policy/permissions/grant" \
    -H 'content-type: application/json' -d "$payload")"
  printf '%s\n' "$response" | jq -e --arg profile "$profile" \
    '.ok == true and .grant.control_id == "app-builder" and .grant.profile == $profile' >/dev/null
}

capture_first_app_polish_interactions() {
  local width height entry_x entry_y offline_difference

  rm -f "$GOBLINS_OS_POLICY_STATE/permissions.json"
  core_proof_curl -sSf "$CORE_PROOF_URL/v1/policy/status" | jq -e \
    'any(.controls[]; .id == "app-builder" and .profile_state == "permission-gated" and .state == "permission-gated" and .grant == null)' >/dev/null
  export GOBLINS_OS_INSTALLER_PAGE=first-app
  start_interaction_window goblins-os-installer "Goblins OS Setup"
  width="$(interaction_window_size "$INTERACTION_WID" WIDTH 1280)"
  height="$(interaction_window_size "$INTERACTION_WID" HEIGHT 820)"
  entry_x=$((width / 2))
  entry_y=$((height / 2))
  xdotool mousemove --window "$INTERACTION_WID" "$entry_x" "$entry_y"
  xdotool click 1
  xdotool type --clearmodifiers --window "$INTERACTION_WID" "A focus timer for writing sessions"
  sleep 0.5
  xdotool mousemove 2 2
  capture_existing_window "$INTERACTION_WID" 129-first-app-grant-required.png "First App explicit policy grant required"
  cleanup_app_windows

  grant_render_app_builder_permission
  core_proof_curl -sSf "$CORE_PROOF_URL/v1/policy/status" | jq -e \
    'any(.controls[]; .id == "app-builder" and .profile_state == "permission-gated" and .state == "allowed" and .grant != null)' >/dev/null
  start_interaction_window goblins-os-installer "Goblins OS Setup"
  capture_existing_window "$INTERACTION_WID" 130-first-app-policy-granted.png "First App policy grant persisted through the real core"

  width="$(interaction_window_size "$INTERACTION_WID" WIDTH 1280)"
  height="$(interaction_window_size "$INTERACTION_WID" HEIGHT 820)"
  entry_x=$((width / 2))
  entry_y=$((height / 2))
  xdotool mousemove --window "$INTERACTION_WID" "$entry_x" "$entry_y"
  xdotool click 1
  xdotool type --clearmodifiers --window "$INTERACTION_WID" "A focus timer for writing sessions"
  xdotool key --clearmodifiers --window "$INTERACTION_WID" Tab
  xdotool mousemove 2 2
  capture_existing_window "$INTERACTION_WID" .first-app-build-online.png "First App online build comparison"
  stop_core
  xdotool key --clearmodifiers --window "$INTERACTION_WID" Return
  sleep 1.2
  xdotool mousemove 2 2
  capture_existing_window "$INTERACTION_WID" 131-first-app-offline-error.png "First App build while core is offline"
  offline_difference="$(image_absolute_error_pixels \
    "$OUT/.first-app-build-online.png" \
    "$OUT/131-first-app-offline-error.png")"
  rm -f "$OUT/.first-app-build-online.png"
  if [ "$offline_difference" -lt 100 ]; then
    echo "RENDER-FAILED First App offline build did not produce a visible error state (changed_pixels=$offline_difference)" >&2
    return 1
  fi
  cleanup_app_windows
  start_core

  # A locked local-only profile is a real supported core configuration. Remove
  # the prior explicit grant, restart the actual core under that policy, and prove
  # the disabled product state without fabricating a policy response.
  rm -f "$GOBLINS_OS_POLICY_STATE/permissions.json"
  stop_core
  export GOBLINS_OS_POLICY_PROFILE=local-only
  export GOBLINS_OS_POLICY_LOCKED=1
  start_core
  core_proof_curl -sSf "$CORE_PROOF_URL/v1/policy/status" | jq -e \
    '.profile == "local-only" and .locked == true and any(.controls[]; .id == "app-builder" and .state != "allowed")' >/dev/null
  start_interaction_window goblins-os-installer "Goblins OS Setup"
  capture_existing_window "$INTERACTION_WID" 132-first-app-policy-blocked.png "First App blocked by locked policy"
  cleanup_app_windows
  stop_core
  unset GOBLINS_OS_POLICY_PROFILE
  unset GOBLINS_OS_POLICY_LOCKED
  start_core
  unset GOBLINS_OS_INSTALLER_PAGE

  FIRST_APP_OFFLINE_ERROR_DIFFERENCE="$offline_difference"
}

capture_reduced_motion_polish_interactions() {
  local previous_animations reduced_difference

  previous_animations="$(desktop_gsettings get org.gnome.desktop.interface enable-animations)"
  desktop_gsettings set org.gnome.desktop.interface enable-animations false
  if [ "$(desktop_gsettings get org.gnome.desktop.interface enable-animations)" != "false" ]; then
    echo "RENDER-FAILED could not activate the real reduced-motion preference" >&2
    return 1
  fi

  export GOBLINS_OS_INSTALLER_PAGE=accessibility
  capture goblins-os-installer "Goblins OS Setup" 133-setup-accessibility-reduced-motion.png

  export GOBLINS_OS_INSTALLER_PAGE=install-progress
  start_interaction_window goblins-os-installer "Goblins OS Setup"
  xdotool mousemove 2 2
  capture_existing_window "$INTERACTION_WID" 134-install-progress-reduced-motion-a.png "Reduced-motion install progress frame A"
  sleep 1.2
  xdotool mousemove 2 2
  capture_existing_window "$INTERACTION_WID" 135-install-progress-reduced-motion-b.png "Reduced-motion install progress frame B"
  reduced_difference="$(image_absolute_error_pixels \
    "$OUT/134-install-progress-reduced-motion-a.png" \
    "$OUT/135-install-progress-reduced-motion-b.png")"
  if [ "$reduced_difference" -ne 0 ]; then
    echo "RENDER-FAILED reduced-motion progress frames changed (changed_pixels=$reduced_difference)" >&2
    return 1
  fi
  cleanup_app_windows
  unset GOBLINS_OS_INSTALLER_PAGE
  desktop_gsettings set org.gnome.desktop.interface enable-animations "$previous_animations"

  REDUCED_MOTION_DIFFERENCE="$reduced_difference"
}

capture_first_boot_offline_codex_if_supported() {
  local codex_status network_status

  codex_status="$(core_proof_curl -sSf "$CORE_PROOF_URL/v1/codex/status")"
  network_status="$(core_proof_curl -sSf "$CORE_PROOF_URL/v1/network/status")"
  FIRST_BOOT_OFFLINE_CODEX_SUPPORTED=false
  FIRST_BOOT_OFFLINE_CODEX_CAPTURED=false

  # The welcome page only exposes the offline Codex reason when the installed
  # image really contains Codex, it is signed out, and NetworkManager reports
  # offline. Never forge either response just to make a screenshot appear.
  if printf '%s\n' "$codex_status" | jq -e \
      '.installed == true and .authenticated == false' >/dev/null \
    && printf '%s\n' "$network_status" | jq -e '.online == false' >/dev/null; then
    FIRST_BOOT_OFFLINE_CODEX_SUPPORTED=true
    export GOBLINS_OS_THEME=light
    export GOBLINS_OS_INSTALLER_PAGE=welcome
    capture goblins-os-installer "Goblins OS Setup" 138-first-boot-codex-offline.png
    unset GOBLINS_OS_INSTALLER_PAGE
    FIRST_BOOT_OFFLINE_CODEX_CAPTURED=true
  else
    echo "SKIPPED 138-first-boot-codex-offline.png: real render state is not signed-out Codex plus offline network"
  fi
}

write_polish_interactions_proof() {
  python3 - "$OUT" \
    "${SETTINGS_EXPANDED_DIFFERENCE:?}" \
    "${SETTINGS_DARK_EXPANDED_DIFFERENCE:?}" \
    "${SETTINGS_OFFLINE_ERROR_DIFFERENCE:?}" \
    "${STUDIO_ERROR_DIFFERENCE:?}" \
    "${STUDIO_ERROR_VS_CLOSED_DIFFERENCE:?}" \
    "${STUDIO_DARK_MENU_DIFFERENCE:?}" \
    "${FIRST_APP_OFFLINE_ERROR_DIFFERENCE:?}" \
    "${REDUCED_MOTION_DIFFERENCE:?}" \
    "${FIRST_BOOT_OFFLINE_CODEX_SUPPORTED:?}" \
    "${FIRST_BOOT_OFFLINE_CODEX_CAPTURED:?}" <<'PY'
import hashlib
import json
import struct
import sys
from pathlib import Path

output = Path(sys.argv[1])
settings_difference = int(sys.argv[2])
settings_dark_difference = int(sys.argv[3])
settings_offline_difference = int(sys.argv[4])
studio_difference = int(sys.argv[5])
studio_closed_difference = int(sys.argv[6])
studio_dark_difference = int(sys.argv[7])
first_app_offline_difference = int(sys.argv[8])
reduced_difference = int(sys.argv[9])
first_boot_offline_codex_supported = sys.argv[10] == "true"
first_boot_offline_codex_captured = sys.argv[11] == "true"
screenshots = [
    "124-settings-models-advanced-collapsed.png",
    "125-settings-models-advanced-expanded.png",
    "126-settings-models-engine-offline-error.png",
    "127-studio-engine-menu.png",
    "128-studio-engine-offline-error.png",
    "129-first-app-grant-required.png",
    "130-first-app-policy-granted.png",
    "131-first-app-offline-error.png",
    "132-first-app-policy-blocked.png",
    "133-setup-accessibility-reduced-motion.png",
    "134-install-progress-reduced-motion-a.png",
    "135-install-progress-reduced-motion-b.png",
    "136-settings-models-advanced-expanded-dark.png",
    "137-studio-engine-menu-dark.png",
]
if first_boot_offline_codex_captured:
    screenshots.append("138-first-boot-codex-offline.png")


def png_evidence(name):
    encoded = (output / name).read_bytes()
    if len(encoded) < 24 or encoded[:8] != b"\x89PNG\r\n\x1a\n":
        raise SystemExit(f"RENDER-FAILED {name} is not a valid PNG")
    width, height = struct.unpack(">II", encoded[16:24])
    return {
        "file": name,
        "sha256": hashlib.sha256(encoded).hexdigest(),
        "width": width,
        "height": height,
    }


proof = {
    "schema_version": 1,
    "scope": "polish-interactions",
    "installed_binaries": True,
    "synthetic_auth_or_policy_json": False,
    "authenticated_codex_claim": False,
    "policy": {
        "state": "isolated",
        "status_route": "/v1/policy/status",
        "grant_route": "/v1/policy/permissions/grant",
    },
    "offline_driver": "stop and restart the real goblins-os-core process",
    "dark_theme": {
        "driver": "GOBLINS_OS_THEME=dark",
        "settings_models_expanded": True,
        "studio_engine_menu": True,
    },
    "first_boot_offline_codex": {
        "status_routes": ["/v1/codex/status", "/v1/network/status"],
        "supported_by_real_render_state": first_boot_offline_codex_supported,
        "captured": first_boot_offline_codex_captured,
        "authenticated_claim": False,
    },
    "accessibility": {
        "proof": "source-gated",
        "account_backed_at_spi": "external",
    },
    "comparisons": {
        "settings_expanded_changed_pixels": settings_difference,
        "settings_dark_expanded_changed_pixels": settings_dark_difference,
        "settings_offline_error_changed_pixels": settings_offline_difference,
        "studio_offline_error_changed_pixels": studio_difference,
        "studio_offline_error_vs_closed_changed_pixels": studio_closed_difference,
        "studio_dark_menu_changed_pixels": studio_dark_difference,
        "first_app_offline_error_changed_pixels": first_app_offline_difference,
        "reduced_motion_changed_pixels": reduced_difference,
        "reduced_motion_zero_difference": reduced_difference == 0,
    },
    "screenshots": [png_evidence(name) for name in screenshots],
}
if not proof["comparisons"]["reduced_motion_zero_difference"]:
    raise SystemExit("RENDER-FAILED reduced-motion comparison was not zero")
if first_boot_offline_codex_supported != first_boot_offline_codex_captured:
    raise SystemExit("RENDER-FAILED supported first-boot offline Codex state was not captured")
(output / "139-polish-interactions-proof.json").write_text(
    json.dumps(proof, indent=2, sort_keys=True) + "\n", encoding="utf-8"
)
PY
}

capture_polish_interactions() {
  SETTINGS_EXPANDED_DIFFERENCE=""
  SETTINGS_DARK_EXPANDED_DIFFERENCE=""
  SETTINGS_OFFLINE_ERROR_DIFFERENCE=""
  STUDIO_ERROR_DIFFERENCE=""
  STUDIO_ERROR_VS_CLOSED_DIFFERENCE=""
  STUDIO_DARK_MENU_DIFFERENCE=""
  FIRST_APP_OFFLINE_ERROR_DIFFERENCE=""
  REDUCED_MOTION_DIFFERENCE=""
  FIRST_BOOT_OFFLINE_CODEX_SUPPORTED=""
  FIRST_BOOT_OFFLINE_CODEX_CAPTURED=""

  capture_settings_polish_interactions
  capture_studio_polish_interactions
  capture_first_app_polish_interactions
  capture_reduced_motion_polish_interactions
  capture_first_boot_offline_codex_if_supported
  write_polish_interactions_proof
  unset GOBLINS_OS_THEME
}

seed_focus_render_state() {
  desktop_gsettings set org.goblins.os.focus modes '[{"id":"work","name":"Deep Work"}]'
  desktop_gsettings set org.goblins.os.focus active-mode work
}

clear_focus_render_state() {
  desktop_gsettings set org.goblins.os.focus active-mode '' 2>/dev/null || true
  desktop_gsettings set org.goblins.os.focus modes '[]' 2>/dev/null || true
}

capture_chrome_surface() {
  seed_first_boot_profile cloud-openai
  core_proof_curl -s -X POST "$CORE_PROOF_URL/v1/session/unlock" \
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

if [ "$RENDER_SCOPE" = "polish-interactions" ]; then
  capture_polish_interactions
  kill "$XVFB_PID" "$CORE_PID" "$RES_PID" 2>/dev/null || true
  echo "=== captured artifacts ==="
  ls -la "$OUT"
  exit 0
fi

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
core_proof_curl -s -X POST "$CORE_PROOF_URL/v1/session/unlock" \
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
