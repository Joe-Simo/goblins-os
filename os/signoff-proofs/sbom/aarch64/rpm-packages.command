#!/usr/bin/env sh
set -eu
tmp="${TMPDIR:-/tmp}/goblins-os-rpm-packages.$$"
trap 'rm -f "$tmp"' EXIT
rpm -qa --qf '%{NAME}\t%{VERSION}-%{RELEASE}\t%{ARCH}\t%{LICENSE}\n' | LC_ALL=C sort > "$tmp"
{
  printf 'name\tversion_release\tarch\tlicense\n'
  cat "$tmp"
} > rpm-packages.tsv
