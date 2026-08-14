#!/usr/bin/env bash

goblins_os_secret_scan_hasher_available() {
  command -v sha256sum >/dev/null 2>&1 || command -v shasum >/dev/null 2>&1
}

goblins_os_secret_sha256() {
  local value="$1"

  if command -v sha256sum >/dev/null 2>&1; then
    printf '%s' "$value" | sha256sum | awk '{print $1}'
  else
    printf '%s' "$value" | shasum -a 256 | awk '{print $1}'
  fi
}

goblins_os_secret_is_exact_fixture() {
  # Keep this allowlist literal and reviewable. A generic `<...>` exemption can
  # accidentally bless a real opaque account token merely because it was
  # wrapped in angle brackets.
  case "$1" in
    '<placeholder>' \
      | '<account-token>' \
      | '<only-for-confidential-clients>' \
      | '<server-side-only-gateway-key>' \
      | '<test-openai-key>' \
      | '<test-gateway-key>' \
      | '<credential-test-value>' \
      | '<credential-test-value>\n' \
      | abcdefghijklmnopqrstuvwxyz012345 \
      | sk- \
      | sk-abcdefghijklmnopqrstuvwxyz \
      | sk-proj-abcdefghijklmnopqrstuvwxyz \
      | sk-proj-abcdefghijklmnopqrstuvwxyz012345)
      return 0
      ;;
  esac
  return 1
}

goblins_os_secret_is_indirect_reference() {
  # Shell, GitHub Actions, and template references do not embed the credential
  # bytes in the repository or artifact being scanned. Only a leading `$` is
  # accepted here; literal prefixes followed by a reference are still scanned.
  case "$1" in
    '$'*)
      return 0
      ;;
  esac
  return 1
}

goblins_os_secret_assignment_name_is_known_provider() {
  case "$1" in
    OPENAI_API_KEY \
      | AI_GATEWAY_API_KEY \
      | AZURE_OPENAI_API_KEY \
      | OPENAI_ACCOUNT_CLIENT_SECRET \
      | OPENAI_ACCOUNT_ACCESS_TOKEN \
      | OPENAI_ACCOUNT_AUTH_TOKEN \
      | GOBLINS_OS_RESIDENT_RELAY_TOKEN \
      | GITHUB_TOKEN \
      | GH_TOKEN \
      | AWS_ACCESS_KEY_ID \
      | AWS_SECRET_ACCESS_KEY \
      | AWS_SESSION_TOKEN \
      | ANTHROPIC_API_KEY \
      | GOOGLE_API_KEY \
      | GEMINI_API_KEY \
      | STRIPE_SECRET_KEY \
      | SLACK_BOT_TOKEN \
      | SLACK_APP_TOKEN \
      | NPM_TOKEN \
      | NODE_AUTH_TOKEN \
      | GITLAB_TOKEN \
      | GITLAB_ACCESS_TOKEN \
      | HUGGINGFACE_TOKEN \
      | HF_TOKEN \
      | CLOUDFLARE_API_TOKEN \
      | SENTRY_AUTH_TOKEN \
      | TWILIO_AUTH_TOKEN \
      | SENDGRID_API_KEY \
      | VERCEL_TOKEN \
      | DIGITALOCEAN_ACCESS_TOKEN \
      | DOCKER_AUTH_TOKEN)
      return 0
      ;;
  esac
  return 1
}

goblins_os_secret_generic_value_looks_live() {
  local value="$1"

  # Generic suffix matching is intentionally conservative so constants such as
  # `*_PASSWORD: u32` are not treated as credentials. Known provider names and
  # provider-specific token formats do not use this heuristic.
  [ "${#value}" -ge 16 ] || return 1
  [[ "$value" =~ [[:alpha:]] ]] || return 1
  [[ "$value" =~ [[:digit:]] ]] || return 1
}

goblins_os_record_secret_finding() {
  local output="$1"
  local path="$2"
  local line_number="$3"
  local detector="$4"
  local matched_value="$5"
  local digest
  local safe_path
  local LC_ALL=C

  # Bind the finding to the matched bytes without ever writing those bytes to
  # stdout, stderr, or the report file.
  digest="$(goblins_os_secret_sha256 "$matched_value")" || return 2
  safe_path="${path//$'\n'/\\n}"
  safe_path="${safe_path//$'\r'/\\r}"
  printf 'path=%s line=%s detector_class=%s length=%s sha256=%s\n' \
    "$safe_path" "$line_number" "$detector" "${#matched_value}" "$digest" >> "$output"
}

goblins_os_scan_secret_line() {
  local output="$1"
  local path="$2"
  local line_number="$3"
  local line="$4"
  local scan_context="${5:-artifact}"
  local matched_value
  local assignment_name
  local github_token_pattern='(^|[^A-Za-z0-9_])(github_pat_[A-Za-z0-9_]{40,}|gh[pousr]_[A-Za-z0-9]{36,})'
  local openai_key_pattern='(^|[^A-Za-z0-9_-])(sk-proj-[A-Za-z0-9_-]{24,}|sk-[A-Za-z0-9_-]{29,})'
  local aws_access_key_pattern='(^|[^A-Z0-9])((AKIA|ASIA)[A-Z0-9]{16})($|[^A-Z0-9])'
  local common_provider_token_pattern='(^|[^A-Za-z0-9_-])(AIza[A-Za-z0-9_-]{35}|npm_[A-Za-z0-9]{36}|glpat-[A-Za-z0-9_-]{20,}|hf_[A-Za-z0-9]{30,}|xox[baprs]-[A-Za-z0-9-]{20,}|sk_live_[A-Za-z0-9]{16,}|ya29[.][A-Za-z0-9_-]{20,}|SG[.][A-Za-z0-9_-]{16,}[.][A-Za-z0-9_-]{16,})'
  local sensitive_assignment_name_pattern='[A-Z][A-Z0-9_]*_API_KEY|[A-Z][A-Z0-9_]*_ACCESS_TOKEN|[A-Z][A-Z0-9_]*_AUTH_TOKEN|[A-Z][A-Z0-9_]*_CLIENT_SECRET|[A-Z][A-Z0-9_]*_PRIVATE_KEY|[A-Z][A-Z0-9_]*_SECRET_KEY|[A-Z][A-Z0-9_]*_PASSWORD|[A-Z][A-Z0-9_]*_PASSWD|[A-Z][A-Z0-9_]*_SECRET|[A-Z][A-Z0-9_]*_TOKEN|AWS_ACCESS_KEY_ID|AWS_SECRET_ACCESS_KEY'
  local provider_assignment_pattern
  local source_declaration_assignment_pattern
  local source_env_call_pattern
  local provider_name_group
  local provider_value_group

  if [ "$scan_context" = "source" ]; then
    provider_assignment_pattern="^[[:space:]]*((export|ENV)[[:space:]]+)?\"?($sensitive_assignment_name_pattern)\"?[[:space:]]*[:=][[:space:]]*\"?([^\"[:space:]#,};]+)"
    source_declaration_assignment_pattern="^[[:space:]]*((pub|pub[(][A-Za-z0-9_:]+[)])[[:space:]]+)?(const|static|let)[[:space:]]+($sensitive_assignment_name_pattern)[[:space:]]*(:[^=]{0,128})?=[[:space:]]*(b?r#*)?\"?([^\"[:space:]#,};]+)"
    source_env_call_pattern="(^|[^A-Za-z0-9_])(set_var|env)[[:space:]]*[(][[:space:]]*\"($sensitive_assignment_name_pattern)\"[[:space:]]*,[[:space:]]*(b?r#*)?\"?([^\"[:space:]#,};]+)"
    provider_name_group=3
    provider_value_group=4
  else
    provider_assignment_pattern="(^|[^A-Za-z0-9_])(export[[:space:]]+)?\"?($sensitive_assignment_name_pattern)\"?[[:space:]]*[:=][[:space:]]*\"?([^\"[:space:]#,};]+)"
    provider_name_group=3
    provider_value_group=4
  fi

  if [[ "$line" =~ $github_token_pattern ]]; then
    matched_value="${BASH_REMATCH[2]}"
    if ! goblins_os_secret_is_exact_fixture "$matched_value"; then
      goblins_os_record_secret_finding \
        "$output" "$path" "$line_number" "github-token" "$matched_value" || return
    fi
    return 0
  fi

  if [[ "$line" =~ $openai_key_pattern ]]; then
    matched_value="${BASH_REMATCH[2]}"
    if ! goblins_os_secret_is_exact_fixture "$matched_value"; then
      goblins_os_record_secret_finding \
        "$output" "$path" "$line_number" "openai-key" "$matched_value" || return
    fi
    return 0
  fi

  if [[ "$line" =~ $aws_access_key_pattern ]]; then
    matched_value="${BASH_REMATCH[2]}"
    if ! goblins_os_secret_is_exact_fixture "$matched_value"; then
      goblins_os_record_secret_finding \
        "$output" "$path" "$line_number" "aws-access-key-id" "$matched_value" || return
    fi
    return 0
  fi

  if [[ "$line" =~ $common_provider_token_pattern ]]; then
    matched_value="${BASH_REMATCH[2]}"
    if ! goblins_os_secret_is_exact_fixture "$matched_value"; then
      goblins_os_record_secret_finding \
        "$output" "$path" "$line_number" "common-provider-token" "$matched_value" || return
    fi
    return 0
  fi

  if [[ "$line" =~ $provider_assignment_pattern ]]; then
    assignment_name="${BASH_REMATCH[$provider_name_group]}"
    matched_value="${BASH_REMATCH[$provider_value_group]}"
  elif [ "$scan_context" = "source" ] \
    && [[ "$line" =~ $source_declaration_assignment_pattern ]]; then
    assignment_name="${BASH_REMATCH[4]}"
    matched_value="${BASH_REMATCH[7]}"
  elif [ "$scan_context" = "source" ] \
    && [[ "$line" =~ $source_env_call_pattern ]]; then
    assignment_name="${BASH_REMATCH[3]}"
    matched_value="${BASH_REMATCH[5]}"
  else
    return 0
  fi

  if [ -n "$assignment_name" ]; then
    case "$matched_value" in
      \'*\')
        matched_value="${matched_value#\'}"
        matched_value="${matched_value%\'}"
        ;;
    esac
    if goblins_os_secret_is_exact_fixture "$matched_value" \
      || goblins_os_secret_is_indirect_reference "$matched_value"; then
      return 0
    fi
    if goblins_os_secret_assignment_name_is_known_provider "$assignment_name"; then
      goblins_os_record_secret_finding \
        "$output" "$path" "$line_number" "provider-assignment" "$matched_value" || return
    elif goblins_os_secret_generic_value_looks_live "$matched_value"; then
      goblins_os_record_secret_finding \
        "$output" "$path" "$line_number" "generic-sensitive-assignment" "$matched_value" || return
    fi
  fi
  return 0
}

goblins_os_scan_secret_file() {
  local output="$1"
  local path="$2"
  local scan_context="${3:-artifact}"
  local line=""
  local line_number=0

  while IFS= read -r line || [ -n "$line" ]; do
    line_number=$((line_number + 1))
    goblins_os_scan_secret_line \
      "$output" "$path" "$line_number" "$line" "$scan_context" || return
  done < "$path" || return 2
  return 0
}

goblins_os_scan_artifact_secret_batch() {
  local output="$1"
  local artifact_file
  shift

  [ "$#" -gt 0 ] || return 0
  for artifact_file in "$@"; do
    goblins_os_scan_secret_file "$output" "$artifact_file" "artifact" || return
  done
  return 0
}

goblins_os_scan_source_secret_batch() {
  local output="$1"
  local source_file
  shift

  [ "$#" -gt 0 ] || return 0
  for source_file in "$@"; do
    goblins_os_scan_secret_file "$output" "$source_file" "source" || return
  done
  return 0
}

goblins_os_artifact_secret_scan() {
  local repo_root="${1:-.}"
  local output
  local file_list
  local artifact_file
  local artifact_root
  local batch=()

  if ! goblins_os_secret_scan_hasher_available; then
    printf '%s\n' "Secret scan requires sha256sum or shasum." >&2
    return 2
  fi

  output="$(mktemp "${TMPDIR:-/tmp}/goblins-os-artifact-secret-scan.XXXXXX")" || return 2
  file_list="$(mktemp "${TMPDIR:-/tmp}/goblins-os-artifact-secret-files.XXXXXX")" || {
    rm -f "$output"
    return 2
  }

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
  while IFS= read -r artifact_file; do
    [ -f "$artifact_file" ] || continue
    batch+=("$artifact_file")
    if [ "${#batch[@]}" -ge 128 ]; then
      if ! goblins_os_scan_artifact_secret_batch "$output" "${batch[@]}"; then
        rm -f "$output" "$file_list"
        return 2
      fi
      batch=()
    fi
  done < "$file_list"
  if ! goblins_os_scan_artifact_secret_batch "$output" "${batch[@]}"; then
    rm -f "$output" "$file_list"
    return 2
  fi

  if [ -s "$output" ]; then
    printf '%s\n' "Possible live secrets found in generated artifacts/evidence; matched content is suppressed:"
    sed -n '1,20p' "$output"
    rm -f "$output" "$file_list"
    return 1
  fi

  rm -f "$output" "$file_list"
  return 0
}

goblins_os_secret_scan_self_test_fail() {
  local work_dir="$1"
  local message="$2"

  rm -rf -- "$work_dir"
  printf 'secret scanner self-test failed: %s\n' "$message" >&2
  return 1
}

goblins_os_secret_scan_self_test() {
  local work_dir
  local fixture
  local source_fixture
  local source_github_fixture
  local source_generic_fixture
  local stdout_file
  local stderr_file
  local provider_name="OPENAI_API""_KEY"
  local gateway_name="AI_GATEWAY_API""_KEY"
  local account_token_name="OPENAI_ACCOUNT_ACCESS""_TOKEN"
  local github_token_name="GITHUB_""TOKEN"
  local gh_token_name="GH_""TOKEN"
  local generic_provider_name="ACME_CLOUD_API""_KEY"
  local example_canary="sk-proj-""exampleA9f4kQ2mZ7tR1bX8nL3vC6dW0pY5hJ"
  local placeholder_canary="placeholder-""A9f4kQ2mZ7tR1bX8nL3vC6dW0pY5hJ"
  local github_pat_canary="ghp_""A9f4kQ2mZ7tR1bX8nL3vC6dW0pY5hJK8mN2q"
  local github_assignment_canary="github-runtime-""A9f4kQ2mZ7tR1bX8nL3vC6dW0pY5hJ"
  local gh_assignment_canary="gh-runtime-""Q2mZ7tR1bX8nL3vC6dW0pY5hJA9f4k"
  local generic_provider_canary="acme-live-""R1bX8nL3vC6dW0pY5hJA9f4kQ2mZ7t"
  local source_github_canary="github-source-""N2qR1bX8nL3vC6dW0pY5hJA9f4kQ2mZ7t"
  local source_generic_canary="acme-source-""C6dW0pY5hJA9f4kQ2mZ7tR1bX8nL3v"
  local expected_digest
  local expected_length
  local github_expected_digest
  local github_expected_length
  local generic_expected_digest
  local generic_expected_length
  local finding_count
  local LC_ALL=C

  work_dir="$(mktemp -d "${TMPDIR:-/tmp}/goblins-os-secret-scan-self-test.XXXXXX")" || return 2
  fixture="$work_dir/artifacts/release/canary.env"
  source_fixture="$work_dir/source-fixture.rs"
  source_github_fixture="$work_dir/source-github.rs"
  source_generic_fixture="$work_dir/source-generic.rs"
  stdout_file="$work_dir/stdout"
  stderr_file="$work_dir/stderr"
  mkdir -p "$(dirname "$fixture")"
  printf '%s=%s\ncommand --set %s=%s\n%s=%s\nNEUTRAL_VALUE=%s\n%s=%s\n%s=%s\n%s=%s\n' \
    "$provider_name" "$example_canary" \
    "$gateway_name" "$placeholder_canary" \
    "$account_token_name" "$placeholder_canary" \
    "$github_pat_canary" \
    "$github_token_name" "$github_assignment_canary" \
    "$gh_token_name" "$gh_assignment_canary" \
    "$generic_provider_name" "$generic_provider_canary" > "$fixture"

  if goblins_os_artifact_secret_scan "$work_dir" > "$stdout_file" 2> "$stderr_file"; then
    goblins_os_secret_scan_self_test_fail "$work_dir" "synthetic canaries were accepted"
    return 1
  fi
  if grep -Fq -- "$example_canary" "$stdout_file" "$stderr_file" \
    || grep -Fq -- "$placeholder_canary" "$stdout_file" "$stderr_file" \
    || grep -Fq -- "$github_pat_canary" "$stdout_file" "$stderr_file" \
    || grep -Fq -- "$github_assignment_canary" "$stdout_file" "$stderr_file" \
    || grep -Fq -- "$gh_assignment_canary" "$stdout_file" "$stderr_file" \
    || grep -Fq -- "$generic_provider_canary" "$stdout_file" "$stderr_file"; then
    goblins_os_secret_scan_self_test_fail "$work_dir" "matched content reached stdout or stderr"
    return 1
  fi
  if [ -s "$stderr_file" ]; then
    goblins_os_secret_scan_self_test_fail "$work_dir" "scanner emitted unexpected stderr"
    return 1
  fi

  finding_count="$(grep -c '^path=' "$stdout_file" || true)"
  if [ "$finding_count" -ne 7 ] \
    || ! grep -Eq '^path=.* line=1 detector_class=openai-key length=[0-9]+ sha256=[0-9a-f]{64}$' "$stdout_file" \
    || ! grep -Eq '^path=.* line=2 detector_class=provider-assignment length=[0-9]+ sha256=[0-9a-f]{64}$' "$stdout_file" \
    || ! grep -Eq '^path=.* line=3 detector_class=provider-assignment length=[0-9]+ sha256=[0-9a-f]{64}$' "$stdout_file" \
    || ! grep -Eq '^path=.* line=4 detector_class=github-token length=[0-9]+ sha256=[0-9a-f]{64}$' "$stdout_file" \
    || ! grep -Eq '^path=.* line=5 detector_class=provider-assignment length=[0-9]+ sha256=[0-9a-f]{64}$' "$stdout_file" \
    || ! grep -Eq '^path=.* line=6 detector_class=provider-assignment length=[0-9]+ sha256=[0-9a-f]{64}$' "$stdout_file" \
    || ! grep -Eq '^path=.* line=7 detector_class=generic-sensitive-assignment length=[0-9]+ sha256=[0-9a-f]{64}$' "$stdout_file"; then
    goblins_os_secret_scan_self_test_fail "$work_dir" "metadata-only findings were incomplete"
    return 1
  fi

  expected_digest="$(goblins_os_secret_sha256 "$example_canary")" || {
    goblins_os_secret_scan_self_test_fail "$work_dir" "could not hash synthetic canary"
    return 1
  }
  expected_length="${#example_canary}"
  if ! grep -Fq "detector_class=openai-key length=$expected_length sha256=$expected_digest" "$stdout_file"; then
    goblins_os_secret_scan_self_test_fail "$work_dir" "finding digest did not bind the matched bytes"
    return 1
  fi

  github_expected_digest="$(goblins_os_secret_sha256 "$github_pat_canary")" || {
    goblins_os_secret_scan_self_test_fail "$work_dir" "could not hash GitHub synthetic canary"
    return 1
  }
  github_expected_length="${#github_pat_canary}"
  if ! grep -Fq "detector_class=github-token length=$github_expected_length sha256=$github_expected_digest" "$stdout_file"; then
    goblins_os_secret_scan_self_test_fail "$work_dir" "GitHub finding digest did not bind the matched bytes"
    return 1
  fi

  generic_expected_digest="$(goblins_os_secret_sha256 "$generic_provider_canary")" || {
    goblins_os_secret_scan_self_test_fail "$work_dir" "could not hash generic provider synthetic canary"
    return 1
  }
  generic_expected_length="${#generic_provider_canary}"
  if ! grep -Fq "detector_class=generic-sensitive-assignment length=$generic_expected_length sha256=$generic_expected_digest" "$stdout_file"; then
    goblins_os_secret_scan_self_test_fail "$work_dir" "generic provider finding digest did not bind the matched bytes"
    return 1
  fi

  printf 'NORMAL_SETTING=enabled\n%s=%s\n%s=%s\n%s=%s\n%s=%s\n%s=%s\n%s\n' \
    "$provider_name" '<placeholder>' \
    "$account_token_name" '<account-token>' \
    "$github_token_name" '${{ github.token }}' \
    "$gh_token_name" '$RUNTIME_GITHUB_TOKEN' \
    "$generic_provider_name" '<credential-test-value>' \
    'sk-proj-abcdefghijklmnopqrstuvwxyz' > "$fixture"
  if ! goblins_os_artifact_secret_scan "$work_dir" > "$stdout_file" 2> "$stderr_file"; then
    goblins_os_secret_scan_self_test_fail "$work_dir" "exact known fixtures were rejected"
    return 1
  fi
  if [ -s "$stdout_file" ] || [ -s "$stderr_file" ]; then
    goblins_os_secret_scan_self_test_fail "$work_dir" "clean scan emitted unexpected output"
    return 1
  fi

  printf 'const %s: &str = "%s";\n' \
    "$github_token_name" "$source_github_canary" > "$source_github_fixture"
  printf 'command.env("%s", "%s");\n' \
    "$generic_provider_name" "$source_generic_canary" > "$source_generic_fixture"
  : > "$stdout_file"
  : > "$stderr_file"
  if ! goblins_os_scan_source_secret_batch \
    "$stdout_file" "$source_github_fixture" "$source_generic_fixture" \
    2> "$stderr_file"; then
    goblins_os_secret_scan_self_test_fail "$work_dir" "source canary scan failed"
    return 1
  fi
  if grep -Fq -- "$source_github_canary" "$stdout_file" "$stderr_file" \
    || grep -Fq -- "$source_generic_canary" "$stdout_file" "$stderr_file"; then
    goblins_os_secret_scan_self_test_fail "$work_dir" "source matched content reached stdout or stderr"
    return 1
  fi
  if [ -s "$stderr_file" ] \
    || [ "$(grep -c '^path=' "$stdout_file" || true)" -ne 2 ] \
    || ! grep -Eq '^path=.*source-github[.]rs line=1 detector_class=provider-assignment length=[0-9]+ sha256=[0-9a-f]{64}$' "$stdout_file" \
    || ! grep -Eq '^path=.*source-generic[.]rs line=1 detector_class=generic-sensitive-assignment length=[0-9]+ sha256=[0-9a-f]{64}$' "$stdout_file"; then
    goblins_os_secret_scan_self_test_fail "$work_dir" "source metadata-only findings were incomplete"
    return 1
  fi

  printf 'fn clean_multiline_fixture() {\n            "%s=%s\\n",\n}\n' \
    "$gateway_name" '<credential-test-value>' > "$source_fixture"
  : > "$stdout_file"
  : > "$stderr_file"
  if ! goblins_os_scan_source_secret_batch "$stdout_file" "$source_fixture" 2> "$stderr_file"; then
    goblins_os_secret_scan_self_test_fail "$work_dir" "source fixture scan failed"
    return 1
  fi
  if [ -s "$stdout_file" ] || [ -s "$stderr_file" ]; then
    goblins_os_secret_scan_self_test_fail "$work_dir" "exact escaped source fixture was rejected"
    return 1
  fi

  rm -rf -- "$work_dir"
  printf '%s\n' "secret scanner self-test passed"
}

if [ "${BASH_SOURCE[0]}" = "$0" ]; then
  case "${1:-}" in
    --self-test)
      goblins_os_secret_scan_self_test
      ;;
    *)
      printf '%s\n' "usage: $0 --self-test" >&2
      exit 2
      ;;
  esac
fi
