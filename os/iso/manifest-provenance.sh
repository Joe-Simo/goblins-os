#!/usr/bin/env bash

# Extract the one image reference embedded in bootc-image-builder's
# JSON-escaped kickstart payload. The command normally appears inside a larger
# JSON string, so the token must stop at JSON escapes, quotes, or whitespace.
goblins_os_bib_manifest_payload_ref() {
  local manifest="${1:-}"
  local matches refs count

  [ -f "$manifest" ] || return 1

  matches="$(
    LC_ALL=C rg -o --no-filename \
      'bootc switch --mutate-in-place --transport registry [^"\\[:space:]]+' \
      "$manifest" 2>/dev/null || true
  )"
  refs="$(
    printf '%s\n' "$matches" \
      | sed 's/^bootc switch --mutate-in-place --transport registry //' \
      | sed '/^$/d' \
      | sort -u
  )"
  count="$(printf '%s\n' "$refs" | sed '/^$/d' | wc -l | tr -d ' ')"

  [ "$count" = "1" ] || return 1
  printf '%s\n' "$refs"
}

goblins_os_ip_literal_is_non_global() {
  local host="$1"
  local ip_status=0

  # Release-proof consumers already require Python. If it is unavailable,
  # fail closed for an IP-looking authority instead of treating it as public.
  command -v python3 >/dev/null 2>&1 || return 0
  python3 - "$host" <<'PY' || ip_status=$?
import ipaddress
import sys

try:
    address = ipaddress.ip_address(sys.argv[1])
except ValueError:
    raise SystemExit(2)
raise SystemExit(0 if not address.is_global else 1)
PY
  case "$ip_status" in
    0) return 0 ;;
    1) return 1 ;;
    *) return 0 ;;
  esac
}

# Return success when an image reference cannot be a publicly pullable release
# source. This follows Docker's registry-authority rule closely enough to allow
# real registries (including digest-pinned Docker Hub namespaces and registries
# with ports) without treating container-loopback or Docker-local DNS as public.
goblins_os_image_ref_is_local_only() {
  local ref="${1:-}"
  local authority first_component host normalized_host
  local bracketed_ip=0

  [ -n "$ref" ] || return 0
  [[ ! "$ref" =~ [[:space:]] ]] || return 0
  [[ "$ref" != -* ]] || return 0

  if [[ "$ref" != */* ]]; then
    return 0
  fi
  first_component="${ref%%/*}"
  case "$first_component" in
    *.*|*:*|localhost)
      authority="$first_component"
      ;;
    *)
      # A namespace/image reference without an explicit registry authority is
      # a real Docker Hub route, not a local alias.
      return 1
      ;;
  esac

  case "$authority" in
    \[*\]|\[*\]:*)
      host="${authority#\[}"
      host="${host%%\]*}"
      bracketed_ip=1
      ;;
    *:*:*)
      # Docker requires IPv6 registry literals to be bracketed. Treat an
      # unbracketed multi-colon authority as local/invalid instead of public.
      return 0
      ;;
    *)
      host="${authority%%:*}"
      ;;
  esac
  normalized_host="$(printf '%s' "$host" | tr '[:upper:]' '[:lower:]')"
  normalized_host="${normalized_host%.}"
  [ -n "$normalized_host" ] || return 0

  case "$normalized_host" in
    localhost|*.localhost|host.docker.internal|host.containers.internal|gateway.docker.internal|*.local|*.docker.internal)
      return 0
      ;;
  esac

  if [ "$bracketed_ip" = "1" ] \
    || [[ "$normalized_host" =~ ^[0-9]+([.][0-9]+){3}$ ]]; then
    if goblins_os_ip_literal_is_non_global "$normalized_host"; then
      return 0
    fi
  fi
  if [[ "$authority" =~ ^[A-Za-z0-9_-]+:[0-9]+$ ]]; then
    return 0
  fi
  return 1
}
