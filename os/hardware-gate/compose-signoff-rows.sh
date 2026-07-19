#!/usr/bin/env bash
set -euo pipefail

REPO_ROOT="${REPO_ROOT:-$(cd "$(dirname "$0")/../.." && pwd -P)}"
cd "$REPO_ROOT"

CANDIDATE_COMMIT="${GOBLINS_OS_CANDIDATE_COMMIT:-}"
if [[ ! "$CANDIDATE_COMMIT" =~ ^[0-9a-f]{40}$ ]]; then
  echo "GOBLINS_OS_CANDIDATE_COMMIT must be the exact lowercase 40-hex candidate commit." >&2
  exit 2
fi
if [ "$(git rev-parse HEAD | tr '[:upper:]' '[:lower:]')" != "$CANDIDATE_COMMIT" ]; then
  echo "The composition checkout must be the exact candidate commit $CANDIDATE_COMMIT." >&2
  exit 2
fi
UNEXPECTED_SOURCE_CHANGES="$({
  git -c core.quotepath=false diff --name-only --no-ext-diff
  git -c core.quotepath=false diff --cached --name-only --no-ext-diff
  git -c core.quotepath=false ls-files --others --exclude-standard
} | sed '/^$/d' | sort -u | grep -Ev '^os/(iso/output/|signoff-proofs/|screenshots/hardware-gate/)|^os/signoff-notes[.]md$' || true)"
if [ -n "$UNEXPECTED_SOURCE_CHANGES" ]; then
  echo "Composition checkout has changes outside generated proof paths:" >&2
  printf '%s\n' "$UNEXPECTED_SOURCE_CHANGES" >&2
  exit 2
fi
if [ "$#" -ne 2 ]; then
  echo "Usage: GOBLINS_OS_CANDIDATE_COMMIT=<commit> $0 <x86_64-signoff-row.md> <aarch64-signoff-row.md>" >&2
  exit 2
fi

python3 - "os/signoff-notes.md" "$CANDIDATE_COMMIT" "$1" "$2" <<'PY'
from __future__ import annotations

import pathlib
import re
import sys

output_path = pathlib.Path(sys.argv[1])
candidate_commit = sys.argv[2]
row_paths = {
    "x86_64": pathlib.Path(sys.argv[3]),
    "aarch64": pathlib.Path(sys.argv[4]),
}

base = output_path.read_text(encoding="utf-8")
validated_rows: list[str] = []
for architecture, row_path in row_paths.items():
    row = row_path.read_text(encoding="utf-8").strip()
    if "\x00" in row:
        raise SystemExit(f"{row_path}: NUL byte is not allowed")
    headings = re.findall(r"^## .+$", row, flags=re.MULTILINE)
    if len(headings) != 1 or not headings[0].startswith("## Manual Gate Run: "):
        raise SystemExit(f"{row_path}: expected exactly one Manual Gate Run block")

    required_lines = (
        f"- Architecture: {architecture}",
        f"- Candidate/source commit: {candidate_commit}",
        "- Verify result (blocked=0): pass",
        "- Self-test result: pass",
        "- Current project completion status: complete",
    )
    for required in required_lines:
        if row.splitlines().count(required) != 1:
            raise SystemExit(f"{row_path}: missing unique required line {required!r}")

    digest_lines = [
        line for line in row.splitlines() if line.startswith("- Image digest reference: ")
    ]
    if len(digest_lines) != 1 or not re.fullmatch(
        r"- Image digest reference: [^\s@]+@sha256:[0-9a-f]{64}", digest_lines[0]
    ):
        raise SystemExit(f"{row_path}: image digest reference is missing or invalid")

    if architecture == "aarch64":
        if not any(
            line.startswith("- Native packaging gate checked: yes (")
            for line in row.splitlines()
        ):
            raise SystemExit(f"{row_path}: aarch64 row lacks accepted native Linux packaging proof")
        if not any(
            re.fullmatch(
                r"- Native packaging gate run: https://github\.com/[^/]+/[^/]+/actions/runs/[0-9]+",
                line,
            )
            for line in row.splitlines()
        ):
            raise SystemExit(f"{row_path}: aarch64 row lacks an exact native gate run URL")

    normalized = row + "\n"
    base = base.replace(normalized, "")
    validated_rows.append(normalized)

base = base.rstrip() + "\n\n" + "\n".join(validated_rows)
output_path.write_text(base, encoding="utf-8")
PY

echo "Composed complete x86_64 and aarch64 signoff rows into os/signoff-notes.md"
