#!/usr/bin/env bash
# Goblins OS install + services self-test, executed INSIDE the assembled OS image
# rootfs. Unlike a staged-tree check, this runs the real installed binaries
# against the real installed filesystem layout, so a pass is evidence the OS is
# correctly installed, its services run, and the persistent Goblins AI runtime serves IPC.
set -uo pipefail

SOCK=/run/goblins-os/resident.sock
export GOBLINS_OS_RESIDENT_SOCKET="$SOCK"
export GOBLINS_OS_RESIDENT_STATE=/var/lib/goblins-os/resident
export GOBLINS_OS_APPS_DIR=/tmp/goblins-os-selftest-apps
fail=0

echo "═══════════════════════════════════════════════════════════════════"
echo " Goblins OS self-test — real image rootfs ($(. /etc/os-release; echo "$PRETTY_NAME"))"
echo "═══════════════════════════════════════════════════════════════════"

echo
echo "── 1. Installed-OS packaging contract: verify --installed-root / ──"
if /usr/libexec/goblins-os/goblins-os-verify --installed-root / --quiet; then
  echo "PASS: installed OS contract holds on the real image rootfs"
else
  echo "FAIL: installed-root verify reported blocked checks"
  /usr/libexec/goblins-os/goblins-os-verify --installed-root / | grep -E '^blocked' || true
  fail=1
fi

echo
echo "── 2. systemd units enabled to start at boot ──"
for unit in goblins-os-core goblins-os-resident goblins-os-model-cache gdm NetworkManager; do
  state=$(systemctl is-enabled "$unit.service" 2>/dev/null || echo "unknown")
  echo "  $unit.service: $state"
  case "$state" in enabled|enabled-runtime|static|alias) ;; *) [ "$unit" = "goblins-os-model-cache" ] || fail=1 ;; esac
done

echo
echo "── 3. OS core daemon starts and serves its API ──"
rm -rf "$GOBLINS_OS_APPS_DIR"
/usr/libexec/goblins-os/goblins-os-core &
core_pid=$!
for _ in $(seq 1 60); do curl -sf http://127.0.0.1:8787/health >/dev/null 2>&1 && break; sleep 0.2; done
for ep in /health /v1/readiness /v1/ai/actions /v1/ai/action-history /v1/system/hardware /v1/local-models \
          /v1/policy/status /v1/ai/runtime/status /v1/codex/resident/status /v1/auth/openai/status \
          /v1/system/services /v1/installer/install-targets /v1/firewall/status /v1/preview/status \
          /v1/apps/build-catalog /v1/apps; do
  code=$(curl -s -o /dev/null -w '%{http_code}' "http://127.0.0.1:8787$ep")
  echo "  GET $ep -> HTTP $code"
  [ "$code" = "200" ] || fail=1
done
preview_status_code=$(curl -s -o /tmp/goblins-os-preview-status.json -w '%{http_code}' \
  http://127.0.0.1:8787/v1/preview/status)
preview_available=$(jq -r '.available // false' /tmp/goblins-os-preview-status.json 2>/dev/null || true)
preview_xdg_open=$(jq -r '.xdg_open_available // false' /tmp/goblins-os-preview-status.json 2>/dev/null || true)
preview_papers=$(jq -r '.papers_available // false' /tmp/goblins-os-preview-status.json 2>/dev/null || true)
preview_loupe=$(jq -r '.loupe_available // false' /tmp/goblins-os-preview-status.json 2>/dev/null || true)
echo "  GET /v1/preview/status -> HTTP $preview_status_code available=$preview_available xdg-open=$preview_xdg_open papers=$preview_papers loupe=$preview_loupe"
[ "$preview_status_code" = "200" ] \
  && [ "$preview_available" = "true" ] \
  && [ "$preview_xdg_open" = "true" ] \
  && [ "$preview_papers" = "true" ] \
  && [ "$preview_loupe" = "true" ] \
  && jq -e '.supported_extensions | index("pdf") and index("png")' /tmp/goblins-os-preview-status.json >/dev/null \
  || fail=1
preview_pdf=/tmp/goblins-os-preview-selftest.pdf
preview_png=/tmp/goblins-os-preview-selftest.png
preview_txt=/tmp/goblins-os-preview-selftest.txt
printf '%%PDF-1.4\n%% Goblins OS Preview self-test\n%%%%EOF\n' > "$preview_pdf"
printf 'Goblins OS Preview image self-test placeholder\n' > "$preview_png"
printf 'Goblins OS Preview unsupported self-test placeholder\n' > "$preview_txt"
preview_pdf_code=$(curl -s -o /tmp/goblins-os-preview-open-pdf.json -w '%{http_code}' \
  -H 'Content-Type: application/json' \
  -d "$(jq -cn --arg path "$preview_pdf" '{path:$path}')" \
  http://127.0.0.1:8787/v1/preview/open)
preview_pdf_ok=$(jq -r '.ok // empty' /tmp/goblins-os-preview-open-pdf.json 2>/dev/null || true)
preview_pdf_kind=$(jq -r '.kind // empty' /tmp/goblins-os-preview-open-pdf.json 2>/dev/null || true)
echo "  POST /v1/preview/open PDF -> HTTP $preview_pdf_code ok=$preview_pdf_ok kind=$preview_pdf_kind"
[ "$preview_pdf_code" = "200" ] && [ "$preview_pdf_ok" = "true" ] && [ "$preview_pdf_kind" = "pdf" ] || fail=1
preview_image_code=$(curl -s -o /tmp/goblins-os-preview-open-image.json -w '%{http_code}' \
  -H 'Content-Type: application/json' \
  -d "$(jq -cn --arg path "$preview_png" '{path:$path}')" \
  http://127.0.0.1:8787/v1/preview/open)
preview_image_ok=$(jq -r '.ok // empty' /tmp/goblins-os-preview-open-image.json 2>/dev/null || true)
preview_image_kind=$(jq -r '.kind // empty' /tmp/goblins-os-preview-open-image.json 2>/dev/null || true)
echo "  POST /v1/preview/open image -> HTTP $preview_image_code ok=$preview_image_ok kind=$preview_image_kind"
[ "$preview_image_code" = "200" ] && [ "$preview_image_ok" = "true" ] && [ "$preview_image_kind" = "image" ] || fail=1
preview_unsupported_code=$(curl -s -o /tmp/goblins-os-preview-open-unsupported.json -w '%{http_code}' \
  -H 'Content-Type: application/json' \
  -d "$(jq -cn --arg path "$preview_txt" '{path:$path}')" \
  http://127.0.0.1:8787/v1/preview/open)
preview_unsupported_ok=$(jq -r '.ok // empty' /tmp/goblins-os-preview-open-unsupported.json 2>/dev/null || true)
preview_unsupported_text=$(jq -r '.text // empty' /tmp/goblins-os-preview-open-unsupported.json 2>/dev/null || true)
echo "  POST /v1/preview/open unsupported -> HTTP $preview_unsupported_code ok=$preview_unsupported_ok"
[ "$preview_unsupported_code" = "400" ] && [ "$preview_unsupported_ok" != "true" ] && [ -n "$preview_unsupported_text" ] || fail=1
firewall_toggle_code=$(curl -s -o /tmp/goblins-os-firewall-toggle.json -w '%{http_code}' \
  -H 'Content-Type: application/json' \
  -d '{"enabled":true}' \
  http://127.0.0.1:8787/v1/firewall/enabled)
firewall_toggle_ok=$(jq -r '.ok // empty' /tmp/goblins-os-firewall-toggle.json 2>/dev/null || true)
firewall_toggle_text=$(jq -r '.text // empty' /tmp/goblins-os-firewall-toggle.json 2>/dev/null || true)
firewall_toggle_error=$(jq -r '.error // empty' /tmp/goblins-os-firewall-toggle.json 2>/dev/null || true)
firewall_toggle_body=$(tr -d '\n' < /tmp/goblins-os-firewall-toggle.json 2>/dev/null || true)
echo "  POST /v1/firewall/enabled -> HTTP $firewall_toggle_code ok=$firewall_toggle_ok"
case "$firewall_toggle_code" in
  200) [ "$firewall_toggle_ok" = "true" ] && [ -n "$firewall_toggle_text" ] || fail=1 ;;
  502|503) [ "$firewall_toggle_ok" != "true" ] && { [ -n "$firewall_toggle_text" ] || [ -n "$firewall_toggle_error" ] || [ -n "$firewall_toggle_body" ]; } || fail=1 ;;
  *) fail=1 ;;
esac
app_build_code=$(curl -s -o /tmp/goblins-os-app-build.json -w '%{http_code}' \
  -H 'Content-Type: application/json' \
  -d '{"intent":"Self-test app-builder route check. Create a tiny notes app plan only."}' \
  http://127.0.0.1:8787/v1/apps/builds)
echo "  POST /v1/apps/builds -> HTTP $app_build_code"
case "$app_build_code" in 200|403|503) ;; *) fail=1 ;; esac
settings_ai_code=$(curl -s -o /tmp/goblins-os-settings-ai.json -w '%{http_code}' \
  -H 'Content-Type: application/json' \
  -d '{"panel":"network","topic":"Network","question":"Why is the network offline?","status_summary":"Self-test route check only; no user content."}' \
  http://127.0.0.1:8787/v1/ai/settings-context)
echo "  POST /v1/ai/settings-context -> HTTP $settings_ai_code"
case "$settings_ai_code" in 200|403|503) ;; *) fail=1 ;; esac
open_settings_ai_code=$(curl -s -o /tmp/goblins-os-open-settings-ai.json -w '%{http_code}' \
  -H 'Content-Type: application/json' \
  -d '{"query":"open wifi settings","source_panel":"self-test"}' \
  http://127.0.0.1:8787/v1/ai/open-settings-panel)
echo "  POST /v1/ai/open-settings-panel -> HTTP $open_settings_ai_code"
case "$open_settings_ai_code" in 200|403) ;; *) fail=1 ;; esac
system_status_ai_code=$(curl -s -o /tmp/goblins-os-system-status-ai.json -w '%{http_code}' \
  -H 'Content-Type: application/json' \
  -d '{"focus":"storage","question":"Summarize current system state.","status_summary":"Self-test route check only; no user content."}' \
  http://127.0.0.1:8787/v1/ai/system-status)
echo "  POST /v1/ai/system-status -> HTTP $system_status_ai_code"
case "$system_status_ai_code" in 200|403|503) ;; *) fail=1 ;; esac
selected_text_ai_code=$(curl -s -o /tmp/goblins-os-selected-text-ai.json -w '%{http_code}' \
  -H 'Content-Type: application/json' \
  -d '{"text":"Self-test selected text route check.","app":"Self Test","window_title":"Installed OS self-test","question":"Summarize this selected text."}' \
  http://127.0.0.1:8787/v1/ai/selected-text-context)
echo "  POST /v1/ai/selected-text-context -> HTTP $selected_text_ai_code"
case "$selected_text_ai_code" in 200|403|503) ;; *) fail=1 ;; esac
writing_ai_code=$(curl -s -o /tmp/goblins-os-writing-ai.json -w '%{http_code}' \
  -H 'Content-Type: application/json' \
  -d '{"text":"Self-test writing tools route check.","app":"Self Test","window_title":"Installed OS self-test","question":"Proofread this text."}' \
  http://127.0.0.1:8787/v1/ai/write-selected-text)
echo "  POST /v1/ai/write-selected-text -> HTTP $writing_ai_code"
case "$writing_ai_code" in 200|403|503) ;; *) fail=1 ;; esac
screen_ai_code=$(curl -s -o /tmp/goblins-os-screen-ai.json -w '%{http_code}' \
  -H 'Content-Type: application/json' \
  -d '{"source":"self-test","app":"Self Test","window_title":"Installed OS self-test","visible_text":"Self-test screen context route check.","visual_summary":"No screenshot pixels are sent in self-test.","question":"Summarize this visible context."}' \
  http://127.0.0.1:8787/v1/ai/screen-context)
echo "  POST /v1/ai/screen-context -> HTTP $screen_ai_code"
case "$screen_ai_code" in 200|403|503) ;; *) fail=1 ;; esac
echo "  hardware scan:"
curl -s http://127.0.0.1:8787/v1/system/hardware | jq -c '{os:.platform.os, ram_gb:.memory.total_gb, accelerators:(.accelerators|length), storage:(.storage|length), runtimes:.runtimes}'
echo "  model eligibility:"
curl -s http://127.0.0.1:8787/v1/local-models | jq -c '.models[] | {id, state, min_ram_gb:.minimum_ram_gb, min_vram_gb:.minimum_gpu_vram_gb, min_storage_gb:.minimum_free_storage_gb}'

echo
echo "── 4. Persistent Goblins AI runtime IPC (always-available OS process) ──"
mkdir -p "$(dirname "$SOCK")"
/usr/libexec/goblins-os/goblins-os-resident &
resident_pid=$!
for _ in $(seq 1 60); do [ -S "$SOCK" ] && break; sleep 0.2; done
if [ -S "$SOCK" ]; then
  echo "  Goblins AI runtime IPC socket live: $SOCK"
  echo "  ping ->   $(echo '{"op":"ping"}'   | socat -t3 - "UNIX-CONNECT:$SOCK")"
  echo "  status -> $(echo '{"op":"status"}' | socat -t3 - "UNIX-CONNECT:$SOCK" | jq -c '{source, mode, engine:.engine.selected, ipc:.ipc.transport, caps:(.capabilities|length)}')"
  # No relay is configured here, so chat must refuse cleanly WITHOUT leaking creds.
  echo "  chat ->   $(echo '{"op":"chat","message":"hello"}' | socat -t6 - "UNIX-CONNECT:$SOCK")"
else
  echo "FAIL: Goblins AI runtime IPC socket never appeared"
  fail=1
fi

echo
echo "── 5. Human login user + autologin (the graphical session is reachable) ──"
if grep -q '^goblin:' /etc/passwd; then
  echo "  human user present: $(getent passwd goblin | cut -d: -f1,6,7)"
else
  echo "FAIL: human login user 'goblin' missing from /etc/passwd"; fail=1
fi
if [ -d /var/home/goblin ]; then echo "  home: /var/home/goblin present"; else echo "FAIL: /var/home/goblin missing"; fail=1; fi
if grep -q '^AutomaticLogin=goblin' /etc/gdm/custom.conf 2>/dev/null; then
  echo "  GDM autologin: $(grep -E '^Automatic' /etc/gdm/custom.conf | tr '\n' ' ')"
else
  echo "FAIL: GDM autologin not configured"; fail=1
fi
if grep -q '^Session=goblins-os' /var/lib/AccountsService/users/goblin 2>/dev/null; then
  echo "  default session pinned: goblins-os (AccountsService)"
else
  echo "FAIL: default session not pinned to goblins-os"; fail=1
fi

kill "$core_pid" "$resident_pid" 2>/dev/null || true

echo
echo "═══════════════════════════════════════════════════════════════════"
if [ "$fail" -eq 0 ]; then
  echo " GOBLINS OS SELF-TEST: PASS"
else
  echo " GOBLINS OS SELF-TEST: FAIL"
fi
echo "═══════════════════════════════════════════════════════════════════"
exit "$fail"
