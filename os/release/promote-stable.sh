#!/usr/bin/env bash
# Deterministic, fail-closed aarch64-only stable promotion.
set -euo pipefail

REPO_ROOT="$(cd "$(dirname "$0")/../.." && pwd -P)"
HELPER="$REPO_ROOT/os/release/stable-promotion.py"
ADAPTER="$REPO_ROOT/os/release/stable-promotion-gh-adapter.sh"
REPOSITORY="Joe-Simo/goblins-os"
SOURCE_REPOSITORY="https://github.com/$REPOSITORY"
PINNED_ZSTD_VERSION='*** Zstandard CLI (64-bit) v1.5.7, by Yann Collet ***'
PINNED_ZSTD=''

log() {
  printf '[goblins-stable] %s\n' "$*"
}

die() {
  printf '[goblins-stable][error] %s\n' "$*" >&2
  exit 1
}

require_command() {
  command -v "$1" >/dev/null 2>&1 || die "missing required command: $1"
}

require_env() {
  local name="$1"
  [ -n "${!name:-}" ] || die "missing required environment variable: $name"
}

assert_exact_zstd() {
  PINNED_ZSTD="$(command -v zstd)"
  case "$PINNED_ZSTD" in
    /*) ;;
    *) die "stable asset creation requires an absolute zstd executable path" ;;
  esac
  [ -x "$PINNED_ZSTD" ] \
    || die "stable asset creation requires an executable zstd path"
  [ "$("$PINNED_ZSTD" --version)" = "$PINNED_ZSTD_VERSION" ] \
    || die "stable asset creation requires the exact reviewed zstd 1.5.7 compressor"
}

scan_promotion_files() {
  local target="$1"
  local output file_list artifact_file
  local -a promotion_scan_files=()
  output="$(mktemp "${TMPDIR:-/tmp}/goblins-promotion-secret-scan.XXXXXX")"
  file_list="$(mktemp "${TMPDIR:-/tmp}/goblins-promotion-secret-files.XXXXXX")"
  : > "$output"
  : > "$file_list"
  if [ -f "$target" ] && [ ! -L "$target" ]; then
    printf '%s\n' "$target" > "$file_list"
  elif [ -d "$target" ] && [ ! -L "$target" ]; then
    find "$target" -type f \( \
      -name '*.command' -o \
      -name '*.conf' -o \
      -name '*.csv' -o \
      -name '*.env' -o \
      -name '*.json' -o \
      -name '*.log' -o \
      -name '*.md' -o \
      -name '*.sha256' -o \
      -name '*.toml' -o \
      -name '*.tsv' -o \
      -name '*.txt' -o \
      -name '*.yaml' -o \
      -name '*.yml' \
    \) -print > "$file_list"
  else
    rm -f -- "$output" "$file_list"
    die "secret-scan target is not a regular file or directory: $target"
  fi
  if [ -s "$file_list" ]; then
    while IFS= read -r artifact_file; do
      [ -f "$artifact_file" ] || continue
      promotion_scan_files+=("$artifact_file")
      if [ "${#promotion_scan_files[@]}" -ge 128 ]; then
        goblins_os_scan_artifact_secret_batch "$output" "${promotion_scan_files[@]}"
        promotion_scan_files=()
      fi
    done < "$file_list"
    goblins_os_scan_artifact_secret_batch "$output" "${promotion_scan_files[@]}"
  fi
  if [ -s "$output" ]; then
    sed -n '1,20p' "$output" >&2
    rm -f -- "$output" "$file_list"
    die "possible live secret found in promotion evidence"
  fi
  rm -f -- "$output" "$file_list"
}

validate_common_inputs() {
  require_env GOBLINS_OS_CANDIDATE_COMMIT
  require_env GOBLINS_OS_CANDIDATE_WORKFLOW_RUN_ID
  require_env GOBLINS_OS_CANDIDATE_WORKFLOW_RUN_ATTEMPT
  require_env GOBLINS_OS_DISPLAY_VERIFICATION_RUN_ID
  require_env GOBLINS_OS_DISPLAY_VERIFICATION_RUN_ATTEMPT
  require_env GOBLINS_OS_STABLE_TAG
  require_env GOBLINS_OS_STABLE_CONFIRMATION

  python3 "$HELPER" validate-inputs \
    --candidate-commit "$GOBLINS_OS_CANDIDATE_COMMIT" \
    --candidate-run-id "$GOBLINS_OS_CANDIDATE_WORKFLOW_RUN_ID" \
    --candidate-run-attempt "$GOBLINS_OS_CANDIDATE_WORKFLOW_RUN_ATTEMPT" \
    --display-run-id "$GOBLINS_OS_DISPLAY_VERIFICATION_RUN_ID" \
    --display-run-attempt "$GOBLINS_OS_DISPLAY_VERIFICATION_RUN_ATTEMPT" \
    --stable-tag "$GOBLINS_OS_STABLE_TAG" \
    --confirmation "$GOBLINS_OS_STABLE_CONFIRMATION"
}

assert_exact_checkout() {
  local head main_commit unexpected

  [ "$(git -C "$REPO_ROOT" rev-parse --is-inside-work-tree)" = "true" ] \
    || die "stable promotion requires a Git checkout"
  head="$(git -C "$REPO_ROOT" rev-parse HEAD | tr '[:upper:]' '[:lower:]')"
  [ "$head" = "$GOBLINS_OS_CANDIDATE_COMMIT" ] \
    || die "checkout HEAD $head is not candidate $GOBLINS_OS_CANDIDATE_COMMIT"
  git -C "$REPO_ROOT" fetch --no-tags --depth=1 origin main
  main_commit="$(git -C "$REPO_ROOT" rev-parse FETCH_HEAD | tr '[:upper:]' '[:lower:]')"
  [ "$main_commit" = "$GOBLINS_OS_CANDIDATE_COMMIT" ] \
    || die "candidate is not the current pushed origin/main commit ($main_commit)"
  unexpected="$({
    git -C "$REPO_ROOT" -c core.quotepath=false diff --name-only --no-ext-diff
    git -C "$REPO_ROOT" -c core.quotepath=false diff --cached --name-only --no-ext-diff
    git -C "$REPO_ROOT" -c core.quotepath=false ls-files --others --exclude-standard
  } | sed '/^$/d' | sort -u | grep -Ev '^os/(iso/output/|signoff-proofs/|screenshots/hardware-gate/)|^os/signoff-notes[.]md$' || true)"
  [ -z "$unexpected" ] || {
    printf '%s\n' "$unexpected" >&2
    die "checkout has changes outside the generated proof roots"
  }
}

assert_credential_free() {
  [ "${GOBLINS_OS_PROMOTION_CREDENTIAL_FREE:-}" = "1" ] \
    || die "prepare must run inside the workflow's credential-free environment"
  local name
  for name in \
    GH_TOKEN GITHUB_TOKEN GITHUB_ENTERPRISE_TOKEN \
    ACTIONS_ID_TOKEN_REQUEST_TOKEN DOCKER_AUTH_CONFIG \
    AWS_ACCESS_KEY_ID AWS_SECRET_ACCESS_KEY AZURE_CLIENT_SECRET \
    GOOGLE_APPLICATION_CREDENTIALS SSH_AUTH_SOCK; do
    [ -z "${!name:-}" ] || die "credential-bearing variable is forbidden during prepare: $name"
  done
  if git -C "$REPO_ROOT" config --local --get-regexp 'http\..*\.extraheader|credential\.' >/dev/null 2>&1; then
    die "checkout contains a persisted Git HTTP credential"
  fi
}

prepare() {
  validate_common_inputs
  assert_credential_free
  require_env GOBLINS_OS_PROMOTION_INPUT_DIR
  require_env GOBLINS_OS_LOCAL_DISPLAY_RUN_DIR
  require_env GOBLINS_OS_PROMOTION_OUTPUT_DIR
  require_env GOBLINS_OS_PROMOTION_TIMESTAMP
  require_env GOBLINS_OS_PROMOTION_WORKFLOW_RUN
  require_env GOBLINS_OS_PROMOTION_WORKFLOW_RUN_ATTEMPT
  require_command git
  require_command jq
  require_command python3
  [ -x /usr/bin/swift ] || die "missing required Apple Swift toolchain: /usr/bin/swift"
  require_command zstd
  assert_exact_zstd
  assert_exact_checkout

  local input_dir output_dir validation image_ref display_artifact adapter_dir
  local candidate_artifact_digest display_artifact_digest
  local verified_display display_run run_date final_gate_log source_epoch
  input_dir="$(cd "$GOBLINS_OS_PROMOTION_INPUT_DIR" && pwd -P)"
  output_dir="$GOBLINS_OS_PROMOTION_OUTPUT_DIR"
  validation="$input_dir/remote-validation.json"

  for path in \
    "$input_dir/candidate-run.json" \
    "$input_dir/candidate-artifacts.json" \
    "$input_dir/candidate-artifact.zip" \
    "$input_dir/display-run.json" \
    "$input_dir/display-artifacts.json" \
    "$input_dir/display-artifact.zip"; do
    [ -s "$path" ] && [ -f "$path" ] && [ ! -L "$path" ] \
      || die "missing or unsafe fetched promotion input: $path"
  done
  [ ! -e "$validation" ] || die "remote validation output already exists"

  log "Validating exact candidate and display-verification workflow artifacts"
  image_ref="$(python3 "$HELPER" validate-remote \
    --candidate-commit "$GOBLINS_OS_CANDIDATE_COMMIT" \
    --candidate-run-id "$GOBLINS_OS_CANDIDATE_WORKFLOW_RUN_ID" \
    --candidate-run-attempt "$GOBLINS_OS_CANDIDATE_WORKFLOW_RUN_ATTEMPT" \
    --display-run-id "$GOBLINS_OS_DISPLAY_VERIFICATION_RUN_ID" \
    --display-run-attempt "$GOBLINS_OS_DISPLAY_VERIFICATION_RUN_ATTEMPT" \
    --stable-tag "$GOBLINS_OS_STABLE_TAG" \
    --confirmation "$GOBLINS_OS_STABLE_CONFIRMATION" \
    --candidate-run-json "$input_dir/candidate-run.json" \
    --candidate-artifacts-json "$input_dir/candidate-artifacts.json" \
    --candidate-archive "$input_dir/candidate-artifact.zip" \
    --display-run-json "$input_dir/display-run.json" \
    --display-artifacts-json "$input_dir/display-artifacts.json" \
    --display-archive "$input_dir/display-artifact.zip" \
    --output "$validation")"
  [[ "$image_ref" =~ ^ghcr\.io/joe-simo/goblins-os@sha256:[0-9a-f]{64}$ ]] \
    || die "remote validation returned an invalid immutable image"
  candidate_artifact_digest="$(jq -er '.candidate_artifact_digest' "$validation")"
  display_artifact_digest="$(jq -er '.display_artifact_digest' "$validation")"
  [[ "$candidate_artifact_digest" =~ ^sha256:[0-9a-f]{64}$ ]] \
    || die "remote validation returned an invalid candidate artifact digest"
  [[ "$display_artifact_digest" =~ ^sha256:[0-9a-f]{64}$ ]] \
    || die "remote validation returned an invalid display artifact digest"

  log "Hydrating the exact candidate through the existing validator without credentials"
  adapter_dir="$(mktemp -d "${TMPDIR:-/tmp}/goblins-promotion-gh.XXXXXX")"
  cleanup_adapter() {
    rm -f -- "$adapter_dir/gh"
    rmdir "$adapter_dir" 2>/dev/null || true
  }
  trap cleanup_adapter EXIT HUP INT TERM
  ln -s "$ADAPTER" "$adapter_dir/gh"
  GOBLINS_OS_PROMOTION_CANDIDATE_RUN_ID="$GOBLINS_OS_CANDIDATE_WORKFLOW_RUN_ID" \
  GOBLINS_OS_PROMOTION_CANDIDATE_RUN_ATTEMPT="$GOBLINS_OS_CANDIDATE_WORKFLOW_RUN_ATTEMPT" \
  GOBLINS_OS_PROMOTION_CANDIDATE_COMMIT="$GOBLINS_OS_CANDIDATE_COMMIT" \
  GOBLINS_OS_PROMOTION_CANDIDATE_RUN_JSON="$input_dir/candidate-run.json" \
  GOBLINS_OS_PROMOTION_CANDIDATE_ARTIFACTS_JSON="$input_dir/candidate-artifacts.json" \
  GOBLINS_OS_PROMOTION_CANDIDATE_ARCHIVE="$input_dir/candidate-artifact.zip" \
  GOBLINS_OS_PROMOTION_CANDIDATE_ARTIFACT_DIGEST="$candidate_artifact_digest" \
  GOBLINS_OS_PROMOTION_HELPER="$HELPER" \
  GOBLINS_OS_HYDRATION_MODE=exact-candidate \
  GOBLINS_OS_ARCH=aarch64 \
  GOBLINS_OS_CANDIDATE_COMMIT="$GOBLINS_OS_CANDIDATE_COMMIT" \
  GOBLINS_OS_CANDIDATE_IMAGE_REF="$image_ref" \
  GOBLINS_OS_CANDIDATE_WORKFLOW_RUN="$SOURCE_REPOSITORY/actions/runs/$GOBLINS_OS_CANDIDATE_WORKFLOW_RUN_ID" \
  GOBLINS_OS_CANDIDATE_WORKFLOW_RUN_ATTEMPT="$GOBLINS_OS_CANDIDATE_WORKFLOW_RUN_ATTEMPT" \
  PATH="$adapter_dir:$PATH" \
    "$REPO_ROOT/os/release/hydrate-release-artifacts.sh"
  cleanup_adapter
  trap - EXIT HUP INT TERM

  display_artifact="$input_dir/display-artifact"
  [ ! -e "$display_artifact" ] || die "display artifact extraction path already exists"
  python3 "$HELPER" extract-display \
    --archive "$input_dir/display-artifact.zip" \
    --expected-digest "$display_artifact_digest" \
    --destination "$display_artifact"

  log "Importing and matching the complete canonical local signed proof"
  display_run="$(python3 "$HELPER" install-display \
    --source "$GOBLINS_OS_LOCAL_DISPLAY_RUN_DIR" \
    --verified-display "$display_artifact" \
    --repository "$REPO_ROOT" \
    --remote-inputs "$validation")"
  run_date="$(jq -er '.run_date' "$validation")"
  [ "$display_run" = "$REPO_ROOT/os/screenshots/hardware-gate/aarch64/$run_date" ] \
    || die "display importer returned a non-canonical proof path"

  python3 "$REPO_ROOT/os/hardware-gate/capture-harness/evidence_bundle.py" verify \
    --repository "$REPO_ROOT" \
    --run-dir "$display_run" \
    --architecture aarch64 \
    --candidate-commit "$GOBLINS_OS_CANDIDATE_COMMIT" \
    --image-ref "$image_ref" \
    --run-date "$run_date"
  python3 "$REPO_ROOT/os/hardware-gate/capture-harness/evidence_bundle.py" verify-attestation \
    --seal "$display_run/evidence-bundle.json" \
    --record "$display_run/aarch64-local-display-attestation.json" \
    --signature "$display_run/aarch64-local-display-attestation.json.cms" \
    --certificate "$display_run/display-proof-authority2.pem" \
    --certificate-sha256 "$display_run/display-proof-authority2.sha256" \
    --ca-certificate "$display_run/display-proof-authority2-ca.pem" \
    --ca-certificate-sha256 "$display_run/display-proof-authority2-ca.sha256" \
    --candidate-commit "$GOBLINS_OS_CANDIDATE_COMMIT" \
    --image-ref "$image_ref" \
    --run-date "$run_date"

  local -a screenshot_paths=("$display_run"/[0-9][0-9]-*.png)
  [ "${#screenshot_paths[@]}" -eq 42 ] \
    || die "visual secret scan requires the exact 42 signed screenshots"
  log "Scanning every signed screenshot locally with Apple Vision before promotion packaging"
  /usr/bin/swift "$REPO_ROOT/os/hardware-gate/capture-harness/visual-secret-scan.swift" \
    --seal "$display_run/evidence-bundle.json" \
    "${screenshot_paths[@]}"

  log "Composing the canonical aarch64 signoff row and running the final gate"
  GOBLINS_OS_CANDIDATE_COMMIT="$GOBLINS_OS_CANDIDATE_COMMIT" \
    "$REPO_ROOT/os/hardware-gate/compose-signoff-rows.sh" "$display_run/signoff-row.md"
  final_gate_log="$(mktemp "${TMPDIR:-/tmp}/goblins-final-gate.XXXXXX")"
  if ! GOBLINS_OS_CANDIDATE_COMMIT="$GOBLINS_OS_CANDIDATE_COMMIT" \
    SCREENSHOT_RUN_DIR="os/screenshots/hardware-gate/aarch64/$run_date" \
    "$REPO_ROOT/os/hardware-gate/verify-shipping-status.sh" \
      2>&1 | tee "$final_gate_log"; then
    die "final shipping gate did not pass"
  fi
  rg -q '^Shipping status gate: PASS$' "$final_gate_log" \
    || die "final shipping gate log has no canonical success marker"

  . "$REPO_ROOT/os/hardware-gate/secret-scan.sh"
  scan_promotion_files "$display_run"
  scan_promotion_files "$final_gate_log"

  source_epoch="$(git -C "$REPO_ROOT" show -s --format=%ct "$GOBLINS_OS_CANDIDATE_COMMIT")"
  log "Creating deterministic compressed media and signed-proof release assets"
  python3 "$HELPER" build-assets \
    --repository "$REPO_ROOT" \
    --display-run "$display_run" \
    --output "$output_dir" \
    --remote-inputs "$validation" \
    --candidate-commit "$GOBLINS_OS_CANDIDATE_COMMIT" \
    --stable-tag "$GOBLINS_OS_STABLE_TAG" \
    --promotion-timestamp "$GOBLINS_OS_PROMOTION_TIMESTAMP" \
    --promotion-workflow-run "$GOBLINS_OS_PROMOTION_WORKFLOW_RUN" \
    --promotion-workflow-run-attempt "$GOBLINS_OS_PROMOTION_WORKFLOW_RUN_ATTEMPT" \
    --source-date-epoch "$source_epoch" \
    --final-gate-log "$final_gate_log" \
    --zstd "$PINNED_ZSTD"
  scan_promotion_files "$output_dir"
  python3 "$HELPER" verify-payload \
    --payload "$output_dir" \
    --candidate-commit "$GOBLINS_OS_CANDIDATE_COMMIT" \
    --stable-tag "$GOBLINS_OS_STABLE_TAG" \
    --candidate-run-id "$GOBLINS_OS_CANDIDATE_WORKFLOW_RUN_ID" \
    --candidate-run-attempt "$GOBLINS_OS_CANDIDATE_WORKFLOW_RUN_ATTEMPT" \
    --display-run-id "$GOBLINS_OS_DISPLAY_VERIFICATION_RUN_ID" \
    --display-run-attempt "$GOBLINS_OS_DISPLAY_VERIFICATION_RUN_ATTEMPT" \
    --promotion-run-id "${GOBLINS_OS_PROMOTION_WORKFLOW_RUN##*/}" \
    --promotion-run-attempt "$GOBLINS_OS_PROMOTION_WORKFLOW_RUN_ATTEMPT" \
    --run-date "$run_date" \
    --image-ref "$image_ref" \
    --repository "$REPO_ROOT" \
    --zstd "$PINNED_ZSTD"
  rm -f -- "$final_gate_log"
  log "Prepared exact stable payload at $output_dir"
}

verify_payload() {
  require_env GOBLINS_OS_CANDIDATE_COMMIT
  require_env GOBLINS_OS_CANDIDATE_WORKFLOW_RUN_ID
  require_env GOBLINS_OS_CANDIDATE_WORKFLOW_RUN_ATTEMPT
  require_env GOBLINS_OS_DISPLAY_VERIFICATION_RUN_ID
  require_env GOBLINS_OS_DISPLAY_VERIFICATION_RUN_ATTEMPT
  require_env GOBLINS_OS_PROMOTION_WORKFLOW_RUN_ID
  require_env GOBLINS_OS_PROMOTION_WORKFLOW_RUN_ATTEMPT
  require_env GOBLINS_OS_DISPLAY_RUN_DATE
  require_env GOBLINS_OS_CANDIDATE_IMAGE_REF
  require_env GOBLINS_OS_STABLE_TAG
  require_env GOBLINS_OS_PROMOTION_PAYLOAD_DIR
  require_command python3
  require_command zstd
  assert_exact_zstd
  python3 "$HELPER" verify-payload \
    --payload "$GOBLINS_OS_PROMOTION_PAYLOAD_DIR" \
    --candidate-commit "$GOBLINS_OS_CANDIDATE_COMMIT" \
    --stable-tag "$GOBLINS_OS_STABLE_TAG" \
    --candidate-run-id "$GOBLINS_OS_CANDIDATE_WORKFLOW_RUN_ID" \
    --candidate-run-attempt "$GOBLINS_OS_CANDIDATE_WORKFLOW_RUN_ATTEMPT" \
    --display-run-id "$GOBLINS_OS_DISPLAY_VERIFICATION_RUN_ID" \
    --display-run-attempt "$GOBLINS_OS_DISPLAY_VERIFICATION_RUN_ATTEMPT" \
    --promotion-run-id "$GOBLINS_OS_PROMOTION_WORKFLOW_RUN_ID" \
    --promotion-run-attempt "$GOBLINS_OS_PROMOTION_WORKFLOW_RUN_ATTEMPT" \
    --run-date "$GOBLINS_OS_DISPLAY_RUN_DATE" \
    --image-ref "$GOBLINS_OS_CANDIDATE_IMAGE_REF" \
    --repository "$REPO_ROOT" \
    --zstd "$PINNED_ZSTD"
}

usage() {
  cat >&2 <<'EOF'
Usage: os/release/promote-stable.sh prepare|verify-payload

prepare runs without credentials in the reviewed Apple-Silicon capture
workspace. verify-payload performs the native Linux/aarch64 replay without
credentials. Publication executes no checked-out repository script with a write
token; the protected workflow YAML and stable environment are the publication
trust boundary.
EOF
  exit 2
}

case "${1:-}" in
  prepare) [ "$#" -eq 1 ] || usage; prepare ;;
  verify-payload) [ "$#" -eq 1 ] || usage; verify_payload ;;
  *) usage ;;
esac
