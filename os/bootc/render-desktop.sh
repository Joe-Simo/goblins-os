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

RENDER_STATE_DIR=${GOBLINS_OS_RENDER_STATE_DIR:-/tmp/goblins-os-render-state}
export GOBLINS_OS_SESSION=${GOBLINS_OS_SESSION:-gnome-native-desktop}
export GOBLINS_OS_GUI_PLATFORM=${GOBLINS_OS_GUI_PLATFORM:-gnome-session}
export GOBLINS_OS_SHELL_MODE=${GOBLINS_OS_SHELL_MODE:-native-desktop}

if [ "$(id -u)" -eq 0 ] && [ -z "${GOBLINS_RENDER_USER_SESSION:-}" ]; then
  export GOBLINS_OS_RENDER_STATE_DIR="$RENDER_STATE_DIR"
  export GOBLINS_OS_INSTALLER_STATE="$RENDER_STATE_DIR/installer"
  export GOBLINS_OS_SESSION_STATE="$RENDER_STATE_DIR/session"
  rm -rf "$GOBLINS_OS_RENDER_STATE_DIR"
  mkdir -p "$GOBLINS_OS_INSTALLER_STATE" "$GOBLINS_OS_SESSION_STATE"
  printf '{"mode":"local-gpt-oss","completed_at":"%s"}\n' \
    "$(date -u +%Y-%m-%dT%H:%M:%SZ)" > "$GOBLINS_OS_INSTALLER_STATE/first-boot.json"
  chown -R goblin:goblin "$GOBLINS_OS_RENDER_STATE_DIR"

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

  /usr/libexec/goblins-os/goblins-os-core & CORE_PID=$!
  /usr/libexec/goblins-os/goblins-os-resident & RES_PID=$!
  for _ in $(seq 1 50); do
    curl -sf http://127.0.0.1:8787/health >/dev/null 2>&1 && { echo "core healthy"; break; }
    sleep 0.2
  done
  curl -s -X POST http://127.0.0.1:8787/v1/session/unlock \
    -H 'content-type: application/json' -d '{"mode":"local-gpt-oss"}' >/dev/null 2>&1 || true

  runuser -u goblin -- env \
    GOBLINS_RENDER_USER_SESSION=1 \
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
# the lock hero). Same OS-owned JSON contract the core reads.
export GOBLINS_OS_RENDER_STATE_DIR="$RENDER_STATE_DIR"
export GOBLINS_OS_INSTALLER_STATE="$GOBLINS_OS_RENDER_STATE_DIR/installer"
export GOBLINS_OS_SESSION_STATE="$GOBLINS_OS_RENDER_STATE_DIR/session"
export GOBLINS_OS_SESSION=gnome-native-desktop
export GOBLINS_OS_GUI_PLATFORM=gnome-session
export GOBLINS_OS_SHELL_MODE=native-desktop
mkdir -p "$GOBLINS_OS_INSTALLER_STATE" "$GOBLINS_OS_SESSION_STATE"
if [ ! -f "$GOBLINS_OS_INSTALLER_STATE/first-boot.json" ]; then
  printf '{"mode":"local-gpt-oss","completed_at":"%s"}\n' \
    "$(date -u +%Y-%m-%dT%H:%M:%SZ)" > "$GOBLINS_OS_INSTALLER_STATE/first-boot.json"
fi
export GOBLINS_OS_RAM_GB=32
export GOBLINS_OS_LOCAL_MODEL_RUNTIME=os-managed-runtime

WALLPAPER_DIR=/usr/share/goblins-os/brand/wallpaper
SHELL_PID=""; CORE_PID=""; RES_PID=""

cleanup() {
  pkill -f "/usr/libexec/goblins-os/goblins-os-shell" 2>/dev/null || true
  pkill -f "/usr/libexec/goblins-os/goblins-os-settings" 2>/dev/null || true
  [ -n "$SHELL_PID" ] && kill "$SHELL_PID" 2>/dev/null || true
  [ -n "$CORE_PID" ] && kill "$CORE_PID" 2>/dev/null || true
  [ -n "$RES_PID" ] && kill "$RES_PID" 2>/dev/null || true
}
trap cleanup EXIT

# Capture the full composited stage via the GNOME Shell Screenshot D-Bus API.
# Screenshot(in b include_cursor, in b flash, in s filename)
#   -> (out b success, out s filename_used). Some versions write to a temp path
# and report it in filename_used, so we honor that and copy into /out.
shoot() {
  local name="$1" res used
  res=$(gdbus call --session --dest org.gnome.Shell \
    --object-path /org/gnome/Shell/Screenshot \
    --method org.gnome.Shell.Screenshot.Screenshot \
    false false "$OUT/$name" 2>&1) || {
      echo "RENDER-FAILED $name: screenshot D-Bus call errored: $res" >&2; return 1; }
  used=$(printf '%s' "$res" | sed -n "s/.*, '\\(.*\\)')/\\1/p")
  if [ -n "$used" ] && [ "$used" != "$OUT/$name" ] && [ -f "$used" ]; then
    cp "$used" "$OUT/$name"
  fi
  if [ -f "$OUT/$name" ]; then echo "RENDERED $name"; else
    echo "RENDER-FAILED $name: no file produced ($res)" >&2; return 1; fi
}

# Execute a tiny statement in the live GNOME Shell process. The render container
# runs gnome-shell with --unsafe-mode specifically for deterministic compositor
# proofs, so this is available here; the shipped OS does not depend on Eval.
shell_eval() {
  local script="$1" res
  res=$(gdbus call --session --dest org.gnome.Shell \
    --object-path /org/gnome/Shell \
    --method org.gnome.Shell.Eval "$script" 2>&1) || {
      echo "RENDER-FAILED shell-eval: $res" >&2; return 1; }
  printf '%s\n' "$res"
  printf '%s' "$res" | grep -q "^(true," || {
    echo "RENDER-FAILED shell-eval returned failure: $res" >&2; return 1; }
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
    if gdbus introspect --session --dest org.gnome.Shell \
        --object-path /org/gnome/Shell/Screenshot 2>/dev/null \
        | grep -q "interface org.gnome.Shell.Screenshot"; then
      up=1; echo "gnome-shell ($suffix) is up (Screenshot ready)"; break
    fi
    sleep 0.25
  done
  if [ -z "$up" ]; then
    echo "RENDER-FAILED desktop-$suffix: gnome-shell never claimed org.gnome.Shell" >&2
    kill "$SHELL_PID" 2>/dev/null || true; SHELL_PID=""; return 1
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

  # Bare desktop: wallpaper + menu bar + dock, no window.
  shoot "50-desktop-$suffix.png" || true

  # The native shell composited into the live desktop.
  GOBLINS_OS_THEME="$app_theme" \
  WAYLAND_DISPLAY="$WAYLAND_SOCK" GDK_BACKEND=wayland GSK_RENDERER=cairo \
    /usr/libexec/goblins-os/goblins-os-shell &
  GOBLINS_OS_THEME="$app_theme" \
  WAYLAND_DISPLAY="$WAYLAND_SOCK" GDK_BACKEND=wayland GSK_RENDERER=cairo \
    /usr/libexec/goblins-os/goblins-os-settings &
  sleep 4.0
  shoot "51-desktop-shell-$suffix.png" || true

  # Goblins window-management surfaces: actual Shell actors and real window
  # clones over real native windows, not isolated app screenshots.
  shell_eval "globalThis.goblinsWindowManager.showMissionControlDemo(); 'mission-control';" || return 1
  sleep 0.9
  shoot "52-wm-mission-control-$suffix.png"
  shell_eval "globalThis.goblinsWindowManager.hide(); 'hidden';" || true
  sleep 0.3

  shell_eval "globalThis.goblinsWindowManager.showAppExposeDemo(); 'app-expose';" || return 1
  sleep 0.9
  shoot "52b-wm-app-expose-$suffix.png"
  shell_eval "globalThis.goblinsWindowManager.hide(); 'hidden';" || true
  sleep 0.3

  shell_eval "globalThis.goblinsWindowManager.showSpacesDemo(); 'spaces';" || return 1
  sleep 0.9
  shoot "53-wm-spaces-$suffix.png"
  shell_eval "globalThis.goblinsWindowManager.hide(); 'hidden';" || true
  sleep 0.3

  shell_eval "globalThis.goblinsWindowManager.showSwitcherDemo(); 'switcher';" || return 1
  sleep 0.9
  shoot "54-wm-switcher-$suffix.png"
  shell_eval "globalThis.goblinsWindowManager.hide(); 'hidden';" || true
  sleep 0.3

  shell_eval "globalThis.goblinsWindowManager.showSnapPreviewDemo(); 'snap';" || return 1
  sleep 0.12
  shoot "55-wm-snap-assist-$suffix.png"
  sleep 0.5

  shell_eval "globalThis.goblinsWindowManager.showHudDemo(); 'hud';" || return 1
  sleep 0.9
  shoot "56-wm-hud-$suffix.png"
  shell_eval "globalThis.goblinsWindowManager.hide(); 'hidden';" || true

  pkill -f "/usr/libexec/goblins-os/goblins-os-shell" 2>/dev/null || true
  pkill -f "/usr/libexec/goblins-os/goblins-os-settings" 2>/dev/null || true
  kill "$SHELL_PID" 2>/dev/null || true; SHELL_PID=""
  sleep 1.0
}

# Use the root wrapper's OS daemon + resident when available; if the script is
# invoked directly, try to bring them online in the current user context.
if ! curl -sf http://127.0.0.1:8787/health >/dev/null 2>&1; then
  /usr/libexec/goblins-os/goblins-os-core & CORE_PID=$!
  /usr/libexec/goblins-os/goblins-os-resident & RES_PID=$!
  for _ in $(seq 1 50); do
    curl -sf http://127.0.0.1:8787/health >/dev/null 2>&1 && { echo "core healthy"; break; }
    sleep 0.2
  done
fi
curl -s -X POST http://127.0.0.1:8787/v1/session/unlock \
  -H 'content-type: application/json' -d '{"mode":"local-gpt-oss"}' >/dev/null 2>&1 || true

# Never let one scheme's failure abort the run — always export whatever rendered.
RENDER_FAILED=0
render_scheme 'default'     'light' 'light' || { echo "render_scheme light failed" >&2; RENDER_FAILED=1; }
render_scheme 'prefer-dark' 'dark'  'dark'  || { echo "render_scheme dark failed" >&2; RENDER_FAILED=1; }

echo "=== captured desktop artifacts ==="
ls -la "$OUT"
exit "$RENDER_FAILED"
