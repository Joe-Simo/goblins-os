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

goblins_os_release_evidence_hashes_match() {
  local evidence_dir="$1"
  local manifest="$evidence_dir/release-evidence-manifest.json"
  local cargo_tsv="$evidence_dir/cargo-lock-packages.tsv"
  local rpm_command="$evidence_dir/rpm-packages.command"
  local rpm_tsv="$evidence_dir/rpm-packages.tsv"
  local expected_cargo expected_rpm actual_cargo actual_rpm

  for path in "$manifest" "$cargo_tsv" "$rpm_command" "$rpm_tsv"; do
    [ -s "$path" ] || return 1
    [ ! -L "$path" ] || return 1
  done
  [ ! -e "$evidence_dir/rpm-packages.not-generated.txt" ] || return 1
  grep -Fq '"schema": "goblins-os-release-evidence-v4"' "$manifest" || return 1
  grep -Fq '"cargo_packages_tsv": "cargo-lock-packages.tsv"' "$manifest" || return 1
  grep -Fq '"rpm_packages_tsv": "rpm-packages.tsv"' "$manifest" || return 1
  grep -Fq '"rpm_command_file": "rpm-packages.command"' "$manifest" || return 1
  grep -Fq '"rpm_status": "generated from rpm database"' "$manifest" || return 1
  grep -Fq '"image_digest_pinned": true' "$manifest" || return 1
  expected_cargo="$(awk -F'"' '/"cargo_packages_sha256"/ { print $4; exit }' "$manifest")"
  expected_rpm="$(awk -F'"' '/"rpm_packages_sha256"/ { print $4; exit }' "$manifest")"
  [[ "$expected_cargo" =~ ^[0-9a-f]{64}$ ]] || return 1
  [[ "$expected_rpm" =~ ^[0-9a-f]{64}$ ]] || return 1
  actual_cargo="$(goblins_os_release_evidence_sha256 "$cargo_tsv")" || return 1
  actual_rpm="$(goblins_os_release_evidence_sha256 "$rpm_tsv")" || return 1
  [ "$actual_cargo" = "$expected_cargo" ] && [ "$actual_rpm" = "$expected_rpm" ]
}
