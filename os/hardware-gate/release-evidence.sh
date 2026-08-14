#!/usr/bin/env bash

goblins_os_release_evidence_sha256() {
  if command -v sha256sum >/dev/null 2>&1; then
    sha256sum "$1" | awk '{print $1}'
  elif command -v shasum >/dev/null 2>&1; then
    shasum -a 256 "$1" | awk '{print $1}'
  else
    return 1
  fi
}

_goblins_os_release_evidence_manifest_string() {
  local manifest="$1"
  local field="$2"

  awk -F'"' -v field="$field" '
    $2 == field { value = $4; count += 1 }
    END {
      if (count != 1) {
        exit 1
      }
      print value
    }
  ' "$manifest"
}

_goblins_os_release_evidence_hashes_match() {
  local evidence_dir="$1"
  local expected_schema="$2"
  local bind_rpm_command="$3"
  local manifest="$evidence_dir/release-evidence-manifest.json"
  local cargo_tsv="$evidence_dir/cargo-lock-packages.tsv"
  local rpm_command="$evidence_dir/rpm-packages.command"
  local rpm_tsv="$evidence_dir/rpm-packages.tsv"
  local path
  local schema expected_cargo expected_command expected_rpm
  local actual_cargo actual_command actual_rpm

  for path in "$manifest" "$cargo_tsv" "$rpm_command" "$rpm_tsv"; do
    [ -s "$path" ] || return 1
    [ ! -L "$path" ] || return 1
  done
  [ ! -e "$evidence_dir/rpm-packages.not-generated.txt" ] || return 1
  schema="$(_goblins_os_release_evidence_manifest_string "$manifest" schema)" || return 1
  [ "$schema" = "$expected_schema" ] || return 1
  [ "$(_goblins_os_release_evidence_manifest_string "$manifest" cargo_packages_tsv)" = "cargo-lock-packages.tsv" ] || return 1
  [ "$(_goblins_os_release_evidence_manifest_string "$manifest" rpm_packages_tsv)" = "rpm-packages.tsv" ] || return 1
  [ "$(_goblins_os_release_evidence_manifest_string "$manifest" rpm_command_file)" = "rpm-packages.command" ] || return 1
  [ "$(_goblins_os_release_evidence_manifest_string "$manifest" rpm_status)" = "generated from rpm database" ] || return 1
  grep -Fq '"image_digest_pinned": true' "$manifest" || return 1
  expected_cargo="$(_goblins_os_release_evidence_manifest_string "$manifest" cargo_packages_sha256)" || return 1
  expected_rpm="$(_goblins_os_release_evidence_manifest_string "$manifest" rpm_packages_sha256)" || return 1
  [[ "$expected_cargo" =~ ^[0-9a-f]{64}$ ]] || return 1
  [[ "$expected_rpm" =~ ^[0-9a-f]{64}$ ]] || return 1
  actual_cargo="$(goblins_os_release_evidence_sha256 "$cargo_tsv")" || return 1
  actual_rpm="$(goblins_os_release_evidence_sha256 "$rpm_tsv")" || return 1
  [ "$actual_cargo" = "$expected_cargo" ] || return 1
  [ "$actual_rpm" = "$expected_rpm" ] || return 1

  if [ "$bind_rpm_command" = "1" ]; then
    expected_command="$(_goblins_os_release_evidence_manifest_string "$manifest" rpm_command_sha256)" || return 1
    [[ "$expected_command" =~ ^[0-9a-f]{64}$ ]] || return 1
    actual_command="$(goblins_os_release_evidence_sha256 "$rpm_command")" || return 1
    [ "$actual_command" = "$expected_command" ] || return 1
  else
    ! grep -Fq '"rpm_command_sha256"' "$manifest" || return 1
  fi
}

# Current candidate gates accept only v5 evidence, which binds all three
# generated payloads including the diagnostic RPM replay command.
goblins_os_release_evidence_hashes_match() {
  _goblins_os_release_evidence_hashes_match \
    "$1" \
    "goblins-os-release-evidence-v5" \
    "1"
}

# Read-only historical-alpha hydration may inspect archived v4 evidence. This
# compatibility path is deliberately separate and must never gate a current
# candidate because v4 did not bind rpm-packages.command.
goblins_os_historical_release_evidence_hashes_match() {
  _goblins_os_release_evidence_hashes_match \
    "$1" \
    "goblins-os-release-evidence-v4" \
    "0"
}
