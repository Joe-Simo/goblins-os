#!/usr/bin/env bash

set -euo pipefail

repo_root="$(cd "$(dirname "${BASH_SOURCE[0]}")/../.." && pwd -P)"
workflow_root="$repo_root/.github/workflows"

fail() {
  printf 'publisher-boundary: %s\n' "$1" >&2
  exit 1
}

require_literal() {
  local path="$1"
  local literal="$2"
  local description="$3"
  rg -Fq -- "$literal" "$path" || fail "$description"
}

require_count() {
  local path="$1"
  local pattern="$2"
  local expected="$3"
  local description="$4"
  local actual
  actual="$(rg -c -- "$pattern" "$path" || true)"
  [ "$actual" = "$expected" ] || fail "$description (expected $expected, found $actual)"
}

[ -d "$workflow_root" ] || fail "workflow directory is missing"

if rg -n --glob '*.yml' \
  'packages:[[:space:]]*write|contents:[[:space:]]*write|^[[:space:]]*environment:[[:space:]]*stable([[:space:]]|$)' \
  "$workflow_root"; then
  fail "a source workflow grants publication or stable-environment authority"
fi

if rg -n --glob '*.yml' \
  'docker[[:space:]]+login|docker/login-action|docker[[:space:]]+push|podman[[:space:]]+push|oras[[:space:]]+push|push:[[:space:]]*true|buildx[[:space:]]+imagetools[[:space:]]+create|gh[[:space:]]+release|git[[:space:]]+push|GHCR_TOKEN|write:packages' \
  "$workflow_root"; then
  fail "a source workflow contains a registry, tag, or Release write path"
fi

if rg -n --glob '*.yml' '\$\{\{[[:space:]]*secrets\.' "$workflow_root"; then
  fail "source workflows must not consume repository or environment secrets"
fi

candidate="$workflow_root/candidate-artifacts.yml"
branding="$workflow_root/branding-tool-image.yml"
release="$workflow_root/release.yml"
handoff="$workflow_root/stable-promotion.yml"

for required in "$candidate" "$branding" "$release" "$handoff"; do
  [ -f "$required" ] || fail "missing release workflow: ${required#$repo_root/}"
done

require_literal "$candidate" 'runs-on: ubuntu-24.04-arm' \
  "candidate handoff must run natively on ARM64"
require_literal "$candidate" 'outputs: type=oci,dest=' \
  "candidate handoff must export an OCI archive"
require_literal "$candidate" 'schema: "goblins-os-source-oci-handoff-v1"' \
  "candidate handoff schema is missing"
require_literal "$candidate" 'schema: "goblins-os-actions-artifact-envelope-v1"' \
  "candidate Actions envelope is missing"
require_literal "$candidate" 'split -n 4 -d -a 2' \
  "candidate OCI archive must be split into four bounded payloads"
require_literal "$candidate" 'source_repository_publish_authority: false' \
  "candidate handoff must deny source publication authority"
require_literal "$candidate" 'repository: "Joe-Simo/goblins-os-publisher"' \
  "candidate handoff must target the separate publisher repository"
require_literal "$candidate" 'copy_mode: "preserve-digests"' \
  "candidate handoff must require digest-preserving publication"
require_literal "$candidate" 'and .config.Labels["org.goblins-os.supported-architectures"] == "aarch64"' \
  "candidate OCI verification must enforce the ARM-only image label"
require_literal "$candidate" 'select(.platform.architecture == "amd64")' \
  "candidate OCI verification must reject an amd64 child"
require_count "$candidate" \
  '^      - name: Upload OCI payload part (00|01|02|03)$' 4 \
  "candidate workflow must upload exactly four OCI payload artifacts"
require_literal "$candidate" \
  'aarch64-attempt-${{ github.run_attempt }}-part-' \
  "candidate payload artifact names must be run-attempt scoped"
require_literal "$candidate" \
  'aarch64-attempt-${{ github.run_attempt }}-metadata' \
  "candidate metadata artifact name must be run-attempt scoped"

require_literal "$branding" 'runs-on: ubuntu-24.04-arm' \
  "branding-tool handoff must run natively on ARM64"
require_literal "$branding" 'outputs: type=oci,dest=' \
  "branding-tool handoff must export an OCI archive"
require_literal "$branding" 'schema: "goblins-os-installer-branding-tool-handoff-v1"' \
  "branding-tool handoff schema is missing"
require_literal "$branding" 'schema: "goblins-os-actions-artifact-envelope-v1"' \
  "branding-tool Actions envelope is missing"
require_literal "$branding" 'split -n 4 -d -a 2' \
  "branding-tool OCI archive must be split into four bounded payloads"
require_literal "$branding" 'source_repository_publish_authority: false' \
  "branding-tool handoff must deny source publication authority"
require_literal "$branding" 'repository: "Joe-Simo/goblins-os-publisher"' \
  "branding-tool handoff must target the separate publisher repository"
require_literal "$branding" 'select(.platform.architecture == "amd64")' \
  "branding-tool OCI verification must reject an amd64 child"
require_count "$branding" \
  '^      - name: Upload branding-tool OCI payload part (00|01|02|03)$' 4 \
  "branding-tool workflow must upload exactly four OCI payload artifacts"
require_literal "$branding" \
  'aarch64-attempt-${{ github.run_attempt }}-part-' \
  "branding-tool payload artifact names must be run-attempt scoped"
require_literal "$branding" \
  'aarch64-attempt-${{ github.run_attempt }}-metadata' \
  "branding-tool metadata artifact name must be run-attempt scoped"

require_literal "$release" 'uses: ./.github/workflows/candidate-artifacts.yml' \
  "release compatibility workflow must delegate to the OCI handoff"
require_literal "$release" 'candidate_commit: ${{ github.sha }}' \
  "release compatibility workflow must bind the selected commit"

require_literal "$handoff" 'schema: "goblins-os-publisher-request-v1"' \
  "publisher request schema is missing"
require_literal "$handoff" 'repository: "Joe-Simo/goblins-os-publisher"' \
  "stable handoff must target the separate publisher repository"
require_literal "$handoff" 'source_repository_publish_authority: false' \
  "stable handoff must deny source publication authority"
require_literal "$handoff" 'authenticate-source-run-and-artifact-digests' \
  "stable handoff must require independent artifact authentication"
require_literal "$handoff" 'copy-with-preserved-digests' \
  "stable handoff must require digest-preserving registry import"
require_literal "$handoff" \
  'aarch64-attempt-$CANDIDATE_RUN_ATTEMPT-part-$suffix' \
  "stable handoff must select only candidate artifacts from the requested attempt"
require_literal "$handoff" \
  'aarch64-attempt-$BRANDING_RUN_ATTEMPT-part-$suffix' \
  "stable handoff must select only branding artifacts from the requested attempt"
require_literal "$handoff" 'move-aarch64-and-stable-aliases-last' \
  "stable handoff must require aliases to move last"

if rg -n \
  'GOBLINS_OS_SHIPPABLE_RELEASE|os/iso/build-iso[.]sh|goblins-os-aarch64[.]iso' \
  "$candidate" "$branding"; then
  fail "source OCI handoff workflows must not build or claim shippable media"
fi

if rg -n --glob '*.yml' \
  'uses:[[:space:]]+(actions|docker)/[^@[:space:]]+@v[0-9]+' \
  "$workflow_root"; then
  fail "third-party Actions must be pinned to full commit SHAs"
fi

printf 'publisher-boundary: pass (source workflows are read-only; ARM64 OCI publication is delegated)\n'
