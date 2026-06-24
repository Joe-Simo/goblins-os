#!/usr/bin/env bash

rpm_sbom_arch_matches() {
  local tsv="$1"
  local expected_arch="$2"

  [ -f "$tsv" ] || {
    echo "RPM SBOM missing: $tsv" >&2
    return 1
  }

  awk -F '\t' -v expected_arch="$expected_arch" '
    /^[[:space:]]*$/ {
      next
    }
    $1 == "name" && $3 == "arch" {
      next
    }
    NF < 3 {
      printf "line %d has fewer than 3 tab-separated fields: %s\n", NR, $0 > "/dev/stderr"
      bad = 1
      next
    }
    $1 == "gpg-pubkey" && $3 == "(none)" {
      rows += 1
      next
    }
    {
      rows += 1
      if ($3 != expected_arch && $3 != "noarch") {
        printf "line %d has RPM architecture %s; expected %s or noarch: %s\n", NR, $3, expected_arch, $0 > "/dev/stderr"
        bad = 1
      }
    }
    END {
      if (rows == 0) {
        print "RPM SBOM has no package rows" > "/dev/stderr"
        bad = 1
      }
      exit bad
    }
  ' "$tsv"
}
