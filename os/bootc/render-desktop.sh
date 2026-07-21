#!/usr/bin/env bash
# Render the WHOLE Goblins OS desktop, not just app windows. This boots a real
# headless GNOME Shell session (mutter headless backend + a virtual monitor) in
# the goblins-os session mode inside the actual OS image, applies the desktop
# chrome (wallpaper, shell theme, dock, panel/menu bar), unlocks the session, and
# screenshots the composited desktop via the org.gnome.Shell.Screenshot D-Bus API.
#
# This is the verification loop for the desktop environment (P3): the captured
# pixels are the genuine composited desktop — wallpaper + menu bar + dock + window
# decorations + the native shell — in Light and Dark, exactly as a person sees it
# on a display. It complements render-screens.sh (isolated app windows under Xvfb,
# no compositor).
#
# First-run note: headless GNOME Shell + a programmatic screenshot is finicky in a
# container; this script logs every step and never silently "succeeds" — a missing
# shell bus name or a failed screenshot is a hard, visible error.
set -euo pipefail

OUT=/out
mkdir -p "$OUT"
CORE_PROOF_SOCKET=/run/goblins-os-core/release-proof/control.sock
CORE_PROOF_URL=http://localhost

core_proof_curl() {
  setpriv --regid=goblins-core-release-proof --clear-groups -- \
    curl --unix-socket "$CORE_PROOF_SOCKET" "$@"
}

RENDER_STATE_DIR=${GOBLINS_OS_RENDER_STATE_DIR:-/tmp/goblins-os-render-state}
export GOBLINS_OS_SESSION=${GOBLINS_OS_SESSION:-gnome-native-desktop}
export GOBLINS_OS_GUI_PLATFORM=${GOBLINS_OS_GUI_PLATFORM:-gnome-session}
export GOBLINS_OS_SHELL_MODE=${GOBLINS_OS_SHELL_MODE:-native-desktop}

if [ "$(id -u)" -eq 0 ] && [ -z "${GOBLINS_RENDER_USER_SESSION:-}" ]; then
  export GOBLINS_OS_RENDER_STATE_DIR="$RENDER_STATE_DIR"
  export GOBLINS_OS_INSTALLER_STATE="$RENDER_STATE_DIR/installer"
  export GOBLINS_OS_SESSION_STATE="$RENDER_STATE_DIR/session"
  rm -rf "$GOBLINS_OS_RENDER_STATE_DIR"
  install -d -m 0750 -o goblins-os -g goblins-os \
    "$GOBLINS_OS_INSTALLER_STATE" "$GOBLINS_OS_SESSION_STATE"
  printf '{"mode":"local-gpt-oss","completed_at":"%s"}\n' \
    "$(date -u +%Y-%m-%dT%H:%M:%SZ)" > "$GOBLINS_OS_INSTALLER_STATE/first-boot.json"
  chown -R goblins-os:goblins-os "$GOBLINS_OS_RENDER_STATE_DIR"
  if runuser -u goblin -- test -r "$GOBLINS_OS_INSTALLER_STATE/first-boot.json"; then
    echo "RENDER-FAILED core-owned installer state is readable by the desktop user" >&2
    exit 1
  fi

  export GOBLINS_OS_RAM_GB=32
  export GOBLINS_OS_LOCAL_MODEL_RUNTIME=os-managed-runtime
  install -d -o goblin -g goblin -m 0700 /run/user/1000
  install -d -o goblin -g goblin -m 0700 /var/home/goblin
  install -d -o goblin -g goblin -m 0700 /var/home/goblin/.config /var/home/goblin/.cache
  install -d -o goblin -g goblin -m 0700 /var/home/goblin/.local /var/home/goblin/.local/share
  chown -R goblin:goblin /var/home/goblin "$OUT"
  chmod 0755 "$OUT"

  mkdir -p /run/dbus
  dbus-daemon --system --fork 2>/dev/null || true

  CORE_PID=""; RES_PID=""
  cleanup_root() {
    [ -n "$CORE_PID" ] && kill "$CORE_PID" 2>/dev/null || true
    [ -n "$RES_PID" ] && kill "$RES_PID" 2>/dev/null || true
  }
  trap cleanup_root EXIT

  systemd-tmpfiles --create /usr/lib/tmpfiles.d/goblins-os-core.conf
  install -d -m 0750 -o goblins-resident -g goblins-core-resident \
    /run/goblins-os /var/lib/goblins-os/resident
  setpriv --reuid=goblins-os --regid=goblins-os --init-groups -- \
    /usr/libexec/goblins-os/goblins-os-core & CORE_PID=$!
  GOBLINS_OS_RESIDENT_STATE=/var/lib/goblins-os/resident \
  GOBLINS_OS_RESIDENT_SOCKET=/run/goblins-os/resident.sock \
    setpriv --reuid=goblins-resident --regid=goblins-core-resident --init-groups -- \
      /usr/libexec/goblins-os/goblins-os-resident & RES_PID=$!
  for _ in $(seq 1 50); do
    core_proof_curl -sf "$CORE_PROOF_URL/health" >/dev/null 2>&1 && { echo "core healthy"; break; }
    sleep 0.2
  done
  core_proof_curl -s -X POST "$CORE_PROOF_URL/v1/session/unlock" \
    -H 'content-type: application/json' -d '{"mode":"local-gpt-oss"}' >/dev/null 2>&1 || true

  runuser -u goblin -- env \
    GOBLINS_RENDER_USER_SESSION=1 \
    GOBLINS_OS_RENDER_ROOT_CORE_READY=1 \
    GOBLINS_OS_RENDER_STATE_DIR="$GOBLINS_OS_RENDER_STATE_DIR" \
    GOBLINS_OS_INSTALLER_STATE="$GOBLINS_OS_INSTALLER_STATE" \
    GOBLINS_OS_SESSION_STATE="$GOBLINS_OS_SESSION_STATE" \
    GOBLINS_OS_SESSION="$GOBLINS_OS_SESSION" \
    GOBLINS_OS_GUI_PLATFORM="$GOBLINS_OS_GUI_PLATFORM" \
    GOBLINS_OS_SHELL_MODE="$GOBLINS_OS_SHELL_MODE" \
    GOBLINS_OS_RAM_GB="$GOBLINS_OS_RAM_GB" \
    GOBLINS_OS_LOCAL_MODEL_RUNTIME="$GOBLINS_OS_LOCAL_MODEL_RUNTIME" \
    XDG_RUNTIME_DIR=/run/user/1000 \
    HOME=/var/home/goblin \
    PATH="$PATH" \
    dbus-run-session -- "$0" "$@"
  exit $?
fi

# Re-exec under one private session bus so gnome-shell, gsettings, and the
# screenshot D-Bus calls all share the same org.gnome.Shell instance.
if [ -z "${DBUS_SESSION_BUS_ADDRESS:-}" ]; then
  exec dbus-run-session -- "$0" "$@"
fi

# Headless, software-rendered GNOME Shell: mutter's headless backend renders
# through EGL surfaceless on llvmpipe — no GPU, no seat, no logind.
export XDG_RUNTIME_DIR=${XDG_RUNTIME_DIR:-/run/user/$(id -u)}
mkdir -p "$XDG_RUNTIME_DIR"; chmod 700 "$XDG_RUNTIME_DIR"
# gnome-shell connects to the SYSTEM bus (logind/UPower) during init; without it
# main.js throws "Could not connect" and the shell exits, dropping the bus name.
mkdir -p /run/dbus; dbus-daemon --system --fork 2>/dev/null || true
export LIBGL_ALWAYS_SOFTWARE=1
export GALLIUM_DRIVER=llvmpipe
WAYLAND_SOCK=wayland-goblins
RES=1600x1000

# The bootc image has no writable /root, so gnome-shell/gjs/gsettings fail to make
# their config/cache dirs and main.js throws (the shell then exits and drops the
# org.gnome.Shell bus name). Give the render a real, writable HOME + XDG dirs.
export HOME=${HOME:-/var/home/goblin}
mkdir -p "$HOME/.config" "$HOME/.cache" "$HOME/.local/share"
export XDG_CONFIG_HOME="$HOME/.config"
export XDG_CACHE_HOME="$HOME/.cache"
export XDG_DATA_HOME="$HOME/.local/share"

# Render unlocked and past first boot, so the shell shows the real desktop (not
# the lock hero). The root half seeded this OS-owned JSON for the core.
# Native clients consume only their fixed capability sockets. The desktop user
# deliberately does not read or rewrite that private state.
export GOBLINS_OS_RENDER_STATE_DIR="$RENDER_STATE_DIR"
export GOBLINS_OS_INSTALLER_STATE="$GOBLINS_OS_RENDER_STATE_DIR/installer"
export GOBLINS_OS_SESSION_STATE="$GOBLINS_OS_RENDER_STATE_DIR/session"
export GOBLINS_OS_SESSION=gnome-native-desktop
export GOBLINS_OS_GUI_PLATFORM=gnome-session
export GOBLINS_OS_SHELL_MODE=native-desktop
export GOBLINS_OS_RAM_GB=32
export GOBLINS_OS_LOCAL_MODEL_RUNTIME=os-managed-runtime

WALLPAPER_DIR=/usr/share/goblins-os/brand/wallpaper
SHELL_PID=""; CORE_PID=""; RES_PID=""

cleanup() {
  cleanup_scheme
  [ -n "$CORE_PID" ] && kill "$CORE_PID" 2>/dev/null || true
  [ -n "$RES_PID" ] && kill "$RES_PID" 2>/dev/null || true
}
trap cleanup EXIT

cleanup_scheme() {
  pkill -f "/usr/libexec/goblins-os/goblins-os-shell" 2>/dev/null || true
  pkill -f "/usr/libexec/goblins-os/goblins-os-settings" 2>/dev/null || true
  if [ -n "$SHELL_PID" ]; then
    kill "$SHELL_PID" 2>/dev/null || true
    for _ in $(seq 1 40); do
      kill -0 "$SHELL_PID" 2>/dev/null || break
      sleep 0.1
    done
    if kill -0 "$SHELL_PID" 2>/dev/null; then
      kill -KILL "$SHELL_PID" 2>/dev/null || true
    fi
    wait "$SHELL_PID" 2>/dev/null || true
    SHELL_PID=""
  fi
}

# Capture the full composited stage via the GNOME Shell Screenshot D-Bus API.
# Screenshot(in b include_cursor, in b flash, in s filename)
#   -> (out b success, out s filename_used). Some versions write to a temp path
# and report it in filename_used, so we honor that and copy into /out.
shoot() {
  local name="$1" res used image_info
  rm -f "$OUT/$name"
  res=$(gdbus call --timeout 15 --session --dest org.gnome.Shell \
    --object-path /org/gnome/Shell/Screenshot \
    --method org.gnome.Shell.Screenshot.Screenshot \
    false false "$OUT/$name" 2>&1) || {
      echo "RENDER-FAILED $name: screenshot D-Bus call errored: $res" >&2; return 1; }
  printf '%s' "$res" | grep -q "^(true," || {
    echo "RENDER-FAILED $name: screenshot D-Bus call returned failure: $res" >&2
    rm -f "$OUT/$name"
    return 1
  }
  used=$(printf '%s' "$res" | sed -n "s/.*, '\\(.*\\)')/\\1/p")
  if [ -n "$used" ] && [ "$used" != "$OUT/$name" ] && [ -f "$used" ]; then
    cp "$used" "$OUT/$name"
  fi
  if [ ! -s "$OUT/$name" ]; then
    echo "RENDER-FAILED $name: screenshot is missing or empty ($res)" >&2
    rm -f "$OUT/$name"
    return 1
  fi
  image_info=$(timeout 10s magick identify -format '%m %wx%h' "$OUT/$name" 2>&1) || {
    echo "RENDER-FAILED $name: screenshot is not a decodable PNG: $image_info" >&2
    rm -f "$OUT/$name"
    return 1
  }
  if [ "$image_info" != "PNG ${RES}" ]; then
    echo "RENDER-FAILED $name: expected PNG ${RES}, got $image_info" >&2
    rm -f "$OUT/$name"
    return 1
  fi
  chmod 0644 "$OUT/$name" || {
    echo "RENDER-FAILED $name: could not make screenshot exportable" >&2
    rm -f "$OUT/$name"
    return 1
  }
  echo "RENDERED $name ($image_info)"
}

# Execute a tiny statement in the live GNOME Shell process. The render container
# runs gnome-shell with --unsafe-mode specifically for deterministic compositor
# proofs, so this is available here; the shipped OS does not depend on Eval.
shell_eval() {
  local script="$1" res
  res=$(gdbus call --timeout 5 --session --dest org.gnome.Shell \
    --object-path /org/gnome/Shell \
    --method org.gnome.Shell.Eval "$script" 2>&1) || {
      echo "RENDER-FAILED shell-eval: $res" >&2; return 1; }
  printf '%s\n' "$res"
  printf '%s' "$res" | grep -q "^(true," || {
    echo "RENDER-FAILED shell-eval returned failure: $res" >&2; return 1; }
}

assert_switch_control_inactive() {
  shell_eval "if (!globalThis.goblinsSwitchControl.renderProofInactive()) throw new Error('Switch Control surfaced while disabled'); 'switch-control-inactive';" >/dev/null
}

assert_live_captions_inactive() {
  shell_eval "if (!globalThis.goblinsLiveCaptions.renderProofInactive()) throw new Error('Live Captions surfaced while disabled'); 'live-captions-inactive';" >/dev/null
}

wait_for_two_native_windows() {
  local res="" deadline=$((SECONDS + 20))
  while [ "$SECONDS" -lt "$deadline" ]; do
    res=$(gdbus call --timeout 1 --session --dest org.gnome.Shell \
      --object-path /org/gnome/Shell \
      --method org.gnome.Shell.Eval \
      "globalThis.goblinsWindowManager.renderWindowCount() >= 2;" 2>/dev/null || true)
    if printf '%s' "$res" | grep -q "^(true, 'true')"; then
      echo "two native windows are mapped"
      return 0
    fi
    sleep 0.25
  done
  echo "RENDER-FAILED snap assist needs two mapped native windows ($res)" >&2
  return 1
}

render_scheme() {
  local color_scheme="$1" app_theme="$2" suffix="$3"
  echo "=== rendering desktop: $suffix ($color_scheme) ==="

  # Desktop chrome preferences for this scheme. Wallpaper + accent + fonts +
  # button layout are OS defaults; color-scheme drives Light/Dark everywhere.
  gsettings set org.gnome.desktop.interface color-scheme "$color_scheme" 2>/dev/null || true
  gsettings set org.gnome.desktop.interface font-name 'Inter 11' 2>/dev/null || true
  gsettings set org.gnome.shell.extensions.user-theme name 'GoblinsOS' 2>/dev/null || true
  # Render cleanliness: the only banner here is GNOME's "running as root" warning,
  # a render-only artifact (the shipped OS runs as the unprivileged goblin user).
  gsettings set org.gnome.desktop.notifications show-banners false 2>/dev/null || true
  # Force the scheme-appropriate wallpaper on picture-uri too (headless GNOME does
  # not always honour the color-scheme -> picture-uri-dark switch).
  gsettings set org.gnome.desktop.background picture-uri \
    "file://$WALLPAPER_DIR/goblins-os-$app_theme.png" 2>/dev/null || true
  gsettings set org.gnome.desktop.background picture-uri-dark \
    "file://$WALLPAPER_DIR/goblins-os-dark.png" 2>/dev/null || true
  gsettings set org.gnome.desktop.background picture-options 'zoom' 2>/dev/null || true

  GNOME_SHELL_SESSION_MODE=goblins-os \
  gnome-shell --headless --virtual-monitor "$RES" \
    --wayland --wayland-display "$WAYLAND_SOCK" --unsafe-mode &
  SHELL_PID=$!

  local up=""
  for _ in $(seq 1 160); do
    # The Screenshot INTERFACE registers only once main.js fully initializes, a few
    # seconds in. Grep for the interface (not just introspect exit code) so we don't
    # false-positive on the node existing before the interface is exported.
    if timeout -k 1s 2s gdbus introspect --session --dest org.gnome.Shell \
        --object-path /org/gnome/Shell/Screenshot 2>/dev/null \
        | grep -q "interface org.gnome.Shell.Screenshot"; then
      up=1; echo "gnome-shell ($suffix) is up (Screenshot ready)"; break
    fi
    sleep 0.25
  done
  if [ -z "$up" ]; then
    echo "RENDER-FAILED desktop-$suffix: gnome-shell never claimed org.gnome.Shell" >&2
    return 1
  fi
  sleep 2.0

  # Re-assert scheme + wallpaper on the LIVE shell. Headless mutter loads the
  # background once at startup from the system dconf db (light) and does not pick up
  # the user-db wallpaper set before launch; setting it again now delivers a
  # GSettings::changed signal to the running background actor so it reloads the
  # scheme-correct wallpaper. fedora-bootc has no gdk-pixbuf SVG loader, so the
  # shipped wallpaper is the rasterized PNG (matches the dconf default); the settle
  # lets it decode before we capture.
  gsettings set org.gnome.desktop.interface color-scheme "$color_scheme" 2>/dev/null || true
  gsettings set org.gnome.desktop.background picture-uri \
    "file://$WALLPAPER_DIR/goblins-os-$app_theme.png" 2>/dev/null || true
  gsettings set org.gnome.desktop.background picture-uri-dark \
    "file://$WALLPAPER_DIR/goblins-os-dark.png" 2>/dev/null || true
  sleep 3.0

  # Accessibility overlays are opt-in. Establish the disabled baseline once,
  # then assert it remains true after native apps map; opening an app must never
  # surface or focus Switch Control.
  gsettings set org.goblins.os.a11y.switch-control enabled false || return 1
  shell_eval "globalThis.goblinsSwitchControl.hide(); 'switch-control-disabled';" >/dev/null || return 1
  assert_switch_control_inactive || return 1
  gsettings set org.goblins.shell.extensions.captions enabled false || return 1
  shell_eval "globalThis.goblinsLiveCaptions.hide(); 'live-captions-disabled';" >/dev/null || return 1
  assert_live_captions_inactive || return 1

  # Bare desktop: wallpaper + menu bar + dock, no window.
  shoot "50-desktop-$suffix.png" || return 1

  # IME menu-bar proof: seed two stock XKB sources and switch to the second one
  # so the Goblins menu-bar input-source chip is visible. This proves only the
  # shell indicator render; live CJK engine switching remains hardware-gated.
  gsettings set org.gnome.desktop.input-sources sources "[('xkb', 'us'), ('xkb', 'gb')]" || return 1
  gsettings set org.gnome.desktop.input-sources current 1 || return 1
  sleep 0.8
  shoot "59-menubar-input-source-$suffix.png" || return 1
  gsettings set org.gnome.desktop.input-sources current 0 2>/dev/null || true
  gsettings set org.gnome.desktop.input-sources sources "[('xkb', 'us')]" 2>/dev/null || true
  sleep 0.2

  # Focus menu-bar proof: seed one configured mode and mark it active so the
  # Goblins Focus chip is visible. This proves only the shell indicator render;
  # live arm/disarm and notification writes remain hardware-gated.
  gsettings set org.goblins.os.focus modes '[{"id":"work","name":"Deep Work"}]' || return 1
  gsettings set org.goblins.os.focus active-mode work || return 1
  sleep 0.8
  shoot "59b-menubar-focus-$suffix.png" || return 1
  gsettings set org.goblins.os.focus active-mode '' 2>/dev/null || true
  gsettings set org.goblins.os.focus modes '[]' 2>/dev/null || true
  sleep 0.2

  # Today menu-bar proof: seed deterministic GNOME clock preferences so the
  # date/time button is visible and stable. This proves only the shell button
  # render + launcher affordance; edge-swipe and live widget data remain gated.
  clock_format="$(gsettings get org.gnome.desktop.interface clock-format 2>/dev/null || printf "'24h'")"
  clock_weekday="$(gsettings get org.gnome.desktop.interface clock-show-weekday 2>/dev/null || printf "false")"
  clock_seconds="$(gsettings get org.gnome.desktop.interface clock-show-seconds 2>/dev/null || printf "false")"
  gsettings set org.gnome.desktop.interface clock-format '24h' || return 1
  gsettings set org.gnome.desktop.interface clock-show-weekday true || return 1
  gsettings set org.gnome.desktop.interface clock-show-seconds false || return 1
  sleep 0.8
  shoot "59c-menubar-today-$suffix.png" || return 1
  gsettings set org.gnome.desktop.interface clock-format "$clock_format" 2>/dev/null || true
  gsettings set org.gnome.desktop.interface clock-show-weekday "$clock_weekday" 2>/dev/null || true
  gsettings set org.gnome.desktop.interface clock-show-seconds "$clock_seconds" 2>/dev/null || true
  sleep 0.2

  # The native shell composited into the live desktop.
  GOBLINS_OS_THEME="$app_theme" \
  WAYLAND_DISPLAY="$WAYLAND_SOCK" GDK_BACKEND=wayland GSK_RENDERER=cairo \
    /usr/libexec/goblins-os/goblins-os-shell &
  GOBLINS_OS_THEME="$app_theme" \
  WAYLAND_DISPLAY="$WAYLAND_SOCK" GDK_BACKEND=wayland GSK_RENDERER=cairo \
    /usr/libexec/goblins-os/goblins-os-settings &
  sleep 4.0
  assert_switch_control_inactive || return 1
  assert_live_captions_inactive || return 1
  shoot "51-desktop-shell-$suffix.png" || return 1

  # Goblins window-management surfaces: actual Shell actors and real window
  # clones over real native windows, not isolated app screenshots.
  shell_eval "globalThis.goblinsWindowManager.showWorkspaceOverviewDemo(); 'workspace-overview';" || return 1
  sleep 0.9
  shoot "52-wm-workspace-overview-$suffix.png" || return 1
  shell_eval "globalThis.goblinsWindowManager.hide(); 'hidden';" || true
  sleep 0.3

  shell_eval "globalThis.goblinsWindowManager.showFocusedAppWindowsDemo(); 'focused-app-windows';" || return 1
  sleep 0.9
  shoot "52b-wm-focused-app-windows-$suffix.png" || return 1
  shell_eval "globalThis.goblinsWindowManager.hide(); 'hidden';" || true
  sleep 0.3

  gsettings set org.goblins.shell.extensions.wm hot-corner-top-left 'app-expose' 2>/dev/null || true
  shell_eval "globalThis.goblinsWindowManager.showHotCornerDemo(); 'hot-corner';" || return 1
  sleep 0.9
  shoot "52c-wm-hot-corner-$suffix.png" || return 1
  shell_eval "globalThis.goblinsWindowManager.hide(); 'hidden';" || true
  gsettings set org.goblins.shell.extensions.wm hot-corner-top-left 'none' 2>/dev/null || true
  sleep 0.3

  shell_eval "globalThis.goblinsWindowManager.showWorkspacesDemo(); 'workspaces';" || return 1
  sleep 0.9
  shoot "53-wm-workspaces-$suffix.png" || return 1
  shell_eval "globalThis.goblinsWindowManager.hide(); 'hidden';" || true
  sleep 0.3

  shell_eval "globalThis.goblinsWindowManager.showSwitcherDemo(); 'switcher';" || return 1
  sleep 0.9
  shoot "54-wm-switcher-$suffix.png" || return 1
  shell_eval "globalThis.goblinsWindowManager.hide(); 'hidden';" || true
  sleep 0.3

  wait_for_two_native_windows || return 1
  shell_eval "if (!globalThis.goblinsWindowManager.showSnapAssistDemo()) throw new Error('Snap Assist needs two real native windows'); 'snap-assist';" || return 1
  sleep 0.9
  shoot "55-wm-snap-assist-$suffix.png" || return 1
  shell_eval "globalThis.goblinsWindowManager._clearSnapAssist(); 'snap-assist-hidden';" || true
  sleep 0.5

  shell_eval "globalThis.goblinsWindowManager.showHudDemo(); 'hud';" || return 1
  sleep 0.9
  shoot "56-wm-hud-$suffix.png" || return 1
  shell_eval "globalThis.goblinsWindowManager.hide(); 'hidden';" || true
  sleep 0.3

  # Switch Control proof: the extension is installed in the live shell, remains
  # disabled by default, and can render the point-scan overlay without enabling
  # unproved pointer injection.
  shell_eval "globalThis.goblinsSwitchControl.showPointScanDemo(); 'switch-control-point';" || return 1
  sleep 0.9
  shoot "57-switch-control-point-$suffix.png" || return 1
  shell_eval "globalThis.goblinsSwitchControl.hide(); 'switch-control-hidden';" || true
  sleep 0.3

  # Live Captions proof: render the honest waiting overlay only. This does not
  # start capture, stream transcription, or claim caption text.
  shell_eval "globalThis.goblinsLiveCaptions.showWaitingRenderProof(); 'live-captions-waiting';" || return 1
  sleep 0.9
  shell_eval "if (!globalThis.goblinsLiveCaptions.renderProofWaiting()) throw new Error('Live Captions waiting overlay is not mapped with honest copy'); 'live-captions-waiting-mapped';" >/dev/null || return 1
  shoot "58-live-captions-waiting-$suffix.png" || return 1
  shell_eval "globalThis.goblinsLiveCaptions.hide(); 'live-captions-hidden';" || true
  assert_live_captions_inactive || return 1

}

run_render_scheme() {
  local status=0
  render_scheme "$@" || status=$?
  cleanup_scheme
  sleep 1.0
  return "$status"
}

# The outer root wrapper owns service startup and the proof-only capability.
# This user-session half only launches the installed setgid desktop clients,
# which connect to their own fixed capability sockets.
if [ "${GOBLINS_OS_RENDER_ROOT_CORE_READY:-}" != "1" ]; then
  echo "RENDER-FAILED render-desktop.sh must start through its root service wrapper" >&2
  exit 1
fi

# Never let one scheme's failure abort the run — always export whatever rendered.
RENDER_FAILED=0
run_render_scheme 'default'     'light' 'light' || { echo "render_scheme light failed" >&2; RENDER_FAILED=1; }
run_render_scheme 'prefer-dark' 'dark'  'dark'  || { echo "render_scheme dark failed" >&2; RENDER_FAILED=1; }

VALID_PNGS=$(find "$OUT" -maxdepth 1 -type f -name '*.png' -size +0c | wc -l)
if [ "$VALID_PNGS" -ne 28 ]; then
  echo "RENDER-FAILED desktop proof expected 28 valid PNGs, found $VALID_PNGS" >&2
  RENDER_FAILED=1
fi

echo "=== captured desktop artifacts ==="
ls -la "$OUT"
exit "$RENDER_FAILED"
