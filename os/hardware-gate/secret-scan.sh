#!/usr/bin/env bash

goblins_os_scan_artifact_secret_batch() {
  local output="$1"
  shift

  [ "$#" -gt 0 ] || return 0
  awk '
    function allowed(line) {
      line = tolower(line)
      return line ~ /placeholder|example|secretvalue|abcdefghijklmnopqrstuvwxyz|server-side-only-gateway-key|not set|redacted|dummy|sample|template|your[-_ ]/
    }
    function has_active_secret_assignment(line) {
      return line ~ /"?(OPENAI_API_KEY|AI_GATEWAY_API_KEY|OPENAI_ACCOUNT_CLIENT_SECRET)"?[[:space:]]*[:=][[:space:]]*"?[^"<[:space:]#]/
    }
    function has_openai_key(line, pos, tail) {
      pos = index(line, "sk-proj-")
      if (pos > 0) {
        tail = substr(line, pos)
        if (match(tail, /^sk-proj-[A-Za-z0-9_-]+/) && RLENGTH >= 32) {
          return 1
        }
      }
      pos = index(line, "sk-")
      if (pos > 0) {
        tail = substr(line, pos)
        if (match(tail, /^sk-[A-Za-z0-9_-]+/) && RLENGTH >= 32) {
          return 1
        }
      }
      return 0
    }
    {
      if (allowed($0)) {
        next
      }
      if (has_active_secret_assignment($0) || has_openai_key($0)) {
        print FILENAME ":" FNR ":" $0
      }
    }
  ' "$@" >> "$output"
}

goblins_os_artifact_secret_scan() {
  local repo_root="${1:-.}"
  local output="${TMPDIR:-/tmp}/goblins_os_artifact_secret_scan.$$"
  local file_list="${TMPDIR:-/tmp}/goblins_os_artifact_secret_files.$$"
  : > "$output"
  : > "$file_list"

  [ -f "$repo_root/os/signoff-notes.md" ] && printf '%s\n' "$repo_root/os/signoff-notes.md" >> "$file_list"

  for artifact_root in \
    "$repo_root/artifacts/release" \
    "$repo_root/artifacts/sbom" \
    "$repo_root/artifacts/manifests" \
    "$repo_root/os/signoff-proofs/sbom"; do
    [ -d "$artifact_root" ] || continue
    find "$artifact_root" -type f \( \
      -name '*.command' -o \
      -name '*.conf' -o \
      -name '*.csv' -o \
      -name '*.env' -o \
      -name '*.json' -o \
      -name '*.md' -o \
      -name '*.sha256' -o \
      -name '*.sha256sum' -o \
      -name '*.toml' -o \
      -name '*.tsv' -o \
      -name '*.txt' -o \
      -name '*.yaml' -o \
      -name '*.yml' \
    \) -print >> "$file_list"
  done

  [ -d "$repo_root/os/iso" ] && find "$repo_root/os/iso" -path "$repo_root/os/iso/output*" -type f \( \
    -name '*.json' -o \
    -name '*.sha256' -o \
    -name '*.sha256sum' -o \
    -name '*.txt' \
  \) -print >> "$file_list"

  if [ ! -s "$file_list" ]; then
    rm -f "$output" "$file_list"
    return 0
  fi

  sort -u "$file_list" -o "$file_list"
  local batch=()
  while IFS= read -r artifact_file; do
    [ -f "$artifact_file" ] || continue
    batch+=("$artifact_file")
    if [ "${#batch[@]}" -ge 128 ]; then
      goblins_os_scan_artifact_secret_batch "$output" "${batch[@]}"
      batch=()
    fi
  done < "$file_list"
  goblins_os_scan_artifact_secret_batch "$output" "${batch[@]}"

  if [ -s "$output" ]; then
    echo "Possible live secrets found in generated artifacts/evidence:"
    sed -n '1,20p' "$output"
    rm -f "$output" "$file_list"
    return 1
  fi

  rm -f "$output" "$file_list"
  return 0
}
