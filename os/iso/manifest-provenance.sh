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
