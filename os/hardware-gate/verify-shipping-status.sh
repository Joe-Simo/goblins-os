#!/usr/bin/env bash
set -euo pipefail

ROOT="${ROOT:-$(cd "$(dirname "$0")/../.." && pwd)}"
cd "$ROOT"
. "$ROOT/os/hardware-gate/secret-scan.sh"
. "$ROOT/os/hardware-gate/rpm-sbom-arch.sh"

SHIP_DECL="SHIP.md"
WORKFLOW=".github/workflows/build.yml"
SCREENSHOT_ROOT="os/screenshots/hardware-gate"
SIGNOFF="os/signoff-notes.md"
RUNBOOK="os/hardware-gate/runbook.md"
SCREENSHOT_RUN_DIR="${SCREENSHOT_RUN_DIR:-${SCREENSHOT_DIR:-}}"
FAIL_COUNT=0
ARCHES=(aarch64 x86_64)

REQ_SCREENSHOTS=(
  "01-installer.png"
  "02-install-network.png"
  "03-login.png"
  "04-desktop.png"
  "06-onboarding.png"
  "07-home.png"
  "08-shell-home.png"
  "09-shell-dark.png"
  "10-settings.png"
  "11-settings-models.png"
  "12-settings-dark.png"
  "13-studio-before.png"
  "14-studio-running.png"
  "15-studio-app-detail.png"
  "16-built-app-open.png"
  "17-dark-motion.png"
  "18-light-motion.png"
  "19-vulkan-vkcube.png"
  "20-gamemode-active.png"
  "21-gamescope-session.png"
  "22-mangohud-overlay.png"
  "23-controller-detection.png"
  "24-audio-output.png"
  "25-install-destination.png"
  "26-install-storage-summary.png"
  "27-dual-boot-preserve-existing-os.png"
  "28-bootloader-efi-summary.png"
)

check() {
  local label="$1"
  local test_cmd="$2"
  if eval "$test_cmd"; then
    echo "[PASS] $label"
  else
    echo "[FAIL] $label"
    FAIL_COUNT=$((FAIL_COUNT + 1))
  fi
}

fail_check() {
  local label="$1"
  echo "[FAIL] $label"
  FAIL_COUNT=$((FAIL_COUNT + 1))
}

check_file() {
  local label="$1"
  local path="$2"
  if [ -f "$path" ]; then
    echo "[PASS] $label"
    return 0
  fi
  echo "[FAIL] $label: missing $path"
  FAIL_COUNT=$((FAIL_COUNT + 1))
  return 1
}

check_file_contains() {
  local label="$1"
  local path="$2"
  local pattern="$3"
  if [ ! -f "$path" ]; then
    echo "[FAIL] $label: missing $path"
    FAIL_COUNT=$((FAIL_COUNT + 1))
    return 1
  fi
  if rg -q "$pattern" "$path"; then
    echo "[PASS] $label"
    return 0
  fi
  echo "[FAIL] $label: $path does not contain $pattern"
  FAIL_COUNT=$((FAIL_COUNT + 1))
  return 1
}

check_sha256_file() {
  local label="$1"
  local sha_path="$2"
  local expected actual artifact sha_dir sha_base

  if [ ! -f "$sha_path" ]; then
    echo "[FAIL] $label: missing $sha_path"
    FAIL_COUNT=$((FAIL_COUNT + 1))
    return 1
  fi

  # Verify from the checksum file's own directory so a portable, basename-relative
  # checksum resolves correctly (an absolute legacy path also still works).
  sha_dir="$(dirname "$sha_path")"
  sha_base="$(basename "$sha_path")"
  if command -v sha256sum >/dev/null 2>&1; then
    if (cd "$sha_dir" && sha256sum -c "$sha_base" >/dev/null 2>&1); then
      echo "[PASS] $label"
      return 0
    fi
  elif command -v shasum >/dev/null 2>&1; then
    read -r expected artifact < "$sha_path"
    if [ -n "$expected" ] && [ -n "$artifact" ] && (cd "$sha_dir" && [ -f "$artifact" ]); then
      actual="$(cd "$sha_dir" && shasum -a 256 "$artifact" | awk '{print $1}')"
      if [ "$actual" = "$expected" ]; then
        echo "[PASS] $label"
        return 0
      fi
    fi
  else
    echo "[FAIL] $label: no sha256sum or shasum command available"
    FAIL_COUNT=$((FAIL_COUNT + 1))
    return 1
  fi

  echo "[FAIL] $label: checksum verification failed for $sha_path"
  FAIL_COUNT=$((FAIL_COUNT + 1))
  return 1
}

check_bib_manifest_payload_ref() {
  local label="$1"
  local path="$2"

  if [ ! -f "$path" ]; then
    echo "[FAIL] $label: missing $path"
    FAIL_COUNT=$((FAIL_COUNT + 1))
    return 1
  fi
  if rg -q 'bootc switch --mutate-in-place --transport registry (host\.docker\.internal|localhost[:/]|127\.|0\.0\.0\.0[:/]|goblins-os:|docker\.io/library/goblins-os:)' "$path"; then
    echo "[FAIL] $label: installer payload tracks a local-only Docker/test registry"
    FAIL_COUNT=$((FAIL_COUNT + 1))
    return 1
  fi
  echo "[PASS] $label"
  return 0
}

source_secret_scan() {
  local output="${TMPDIR:-/tmp}/goblins_os_secret_scan.$$"
  : > "$output"

  rg -n --hidden --no-ignore-vcs --no-ignore \
    '^[[:space:]]*(export[[:space:]]+)?(OPENAI_API_KEY|AI_GATEWAY_API_KEY|OPENAI_ACCOUNT_CLIENT_SECRET)[[:space:]]*=[[:space:]]*([^<[:space:]#][^#]*)' \
    . \
    --glob '!.git/**' \
    --glob '!target/**' \
    --glob '!artifacts/**' \
    --glob '!libpod/**' \
    --glob '!os/signoff-proofs/**' \
    --glob '!os/screenshots/**' \
    --glob '!os/iso/output*/**' \
    --glob '!os/brand/*.png' \
    >> "$output" || true

  rg -n --hidden --no-ignore-vcs --no-ignore \
    '(^|[^A-Za-z0-9_-])(sk-proj-[A-Za-z0-9_-]{24,}|sk-[A-Za-z0-9_-]{29,})' \
    . \
    --glob '!.git/**' \
    --glob '!target/**' \
    --glob '!artifacts/**' \
    --glob '!libpod/**' \
    --glob '!os/signoff-proofs/**' \
    --glob '!os/screenshots/**' \
    --glob '!os/iso/output*/**' \
    --glob '!os/brand/*.png' \
    | rg -vi 'placeholder|example|secretvalue|abcdefghijklmnopqrstuvwxyz|server-side-only-gateway-key' \
    >> "$output" || true

  if [ -s "$output" ]; then
    echo "Possible live secrets found:"
    sed -n '1,20p' "$output"
    rm -f "$output"
    return 1
  fi

  rm -f "$output"
  return 0
}

screenshot_run_is_complete() {
  local run_dir="$1"
  local arch
  local shot
  arch="$(screenshot_run_arch "$run_dir")"
  [ -n "$arch" ] || return 1
  for shot in "${REQ_SCREENSHOTS[@]}"; do
    screenshot_file_is_valid_png "$run_dir/$shot" || return 1
  done
  screenshot_manifest_matches_iso "$run_dir" "$arch" || return 1
  return 0
}

screenshot_run_arch() {
  case "/$1/" in
    */os/screenshots/hardware-gate/aarch64/*)
      echo "aarch64"
      ;;
    */os/screenshots/hardware-gate/x86_64/*)
      echo "x86_64"
      ;;
    *)
      echo ""
      ;;
  esac
}

screenshot_file_is_valid_png() {
  local file="$1"
  local signature

  [ -s "$file" ] || return 1
  signature="$(od -An -tx1 -N8 "$file" 2>/dev/null | tr -d ' \n')"
  [ "$signature" = "89504e470d0a1a0a" ]
}

screenshot_manifest_matches_iso() {
  local run_dir="$1"
  local arch="$2"
  local manifest="$run_dir/proof-manifest.json"
  local iso_path="os/iso/output/$arch/bootiso/goblins-os-$arch.iso"
  local sha_path="$iso_path.sha256"
  local iso_sha

  [ -s "$manifest" ] || return 1
  [ -f "$iso_path" ] || return 1
  [ -f "$sha_path" ] || return 1
  iso_sha="$(awk '{print $1; exit}' "$sha_path")"
  [ -n "$iso_sha" ] || return 1
  rg -q '"architecture"[[:space:]]*:[[:space:]]*"'"$arch"'"' "$manifest" \
    && rg -q '"iso"[[:space:]]*:[[:space:]]*"'"$iso_path"'"' "$manifest" \
    && rg -q '"iso_sha256"[[:space:]]*:[[:space:]]*"'"$iso_sha"'"' "$manifest" \
    && rg -q '"captured_at"[[:space:]]*:[[:space:]]*"[^"]+"' "$manifest" \
    && rg -q '"screenshot_run_dir"[[:space:]]*:[[:space:]]*"'"$run_dir"'"' "$manifest"
}

print_missing_screenshot_paths() {
  local run_dir="$1"
  local missing=0
  local shot
  for shot in "${REQ_SCREENSHOTS[@]}"; do
    if ! screenshot_file_is_valid_png "$run_dir/$shot"; then
      echo "  $run_dir/$shot"
      missing=1
    fi
  done
  if [ ! -s "$run_dir/proof-manifest.json" ]; then
    echo "  $run_dir/proof-manifest.json"
    missing=1
  fi
  return "$missing"
}

print_latest_incomplete_screenshot_run() {
  local root_dir="$1"
  local label="$2"
  local latest=""

  if [ ! -d "$root_dir" ]; then
    echo "[INFO] $label screenshot root is missing: $root_dir"
    echo "[INFO] Expected screenshot proof files:"
    print_missing_screenshot_paths "$root_dir/<date>" || true
    return 0
  fi

  latest="$(find "$root_dir" -mindepth 1 -maxdepth 1 -type d | sort -r | head -n 1 || true)"
  if [ -z "$latest" ]; then
    echo "[INFO] $label screenshot root has no dated run directories: $root_dir"
    echo "[INFO] Expected screenshot proof files:"
    print_missing_screenshot_paths "$root_dir/<date>" || true
    return 0
  fi

  echo "[INFO] Latest incomplete $label screenshot run: $latest"
  echo "[INFO] Missing screenshot proof files:"
  print_missing_screenshot_paths "$latest" || true
}

print_legacy_screenshot_roots() {
  local dir
  local base
  local count=0
  local shown=0

  [ -d "$SCREENSHOT_ROOT" ] || return 0

  while IFS= read -r dir; do
    base="$(basename "$dir")"
    case "$base" in
      aarch64 | x86_64)
        continue
        ;;
    esac

    if [ "$count" -eq 0 ]; then
      echo "[INFO] Legacy/non-shipping screenshot roots ignored by architecture proof gate:"
    fi

    count=$((count + 1))
    if [ "$shown" -lt 12 ]; then
      echo "  $dir"
      shown=$((shown + 1))
    fi
  done < <(find "$SCREENSHOT_ROOT" -mindepth 1 -maxdepth 1 -type d | sort)

  if [ "$count" -gt "$shown" ]; then
    echo "  ... $((count - shown)) more"
  fi
}

print_screenshot_run_checks() {
  local run_dir="$1"
  local arch
  local missing=0
  local shot
  arch="$(screenshot_run_arch "$run_dir")"
  for shot in "${REQ_SCREENSHOTS[@]}"; do
    if screenshot_file_is_valid_png "$run_dir/$shot"; then
      echo "[PASS] $shot"
    else
      echo "[FAIL] $shot (missing, empty, or not a PNG)"
      missing=1
    fi
  done
  if [ -n "$arch" ] && screenshot_manifest_matches_iso "$run_dir" "$arch"; then
    echo "[PASS] proof-manifest.json"
  else
    echo "[FAIL] proof-manifest.json"
    missing=1
  fi
  return "$missing"
}

print_arch_next_steps() {
  local arch="$1"

  cat <<EOF

Next evidence command for $arch:
  GOBLINS_OS_ARCH=$arch \\
  GOBLINS_OS_CONTAINER_RUNTIME=docker \\
  RUN_QEMU=1 \\
  GOBLINS_OS_SHIPPABLE_RELEASE=1 \\
  GOBLINS_OS_BIB_SOURCE_IMAGE=<real release bootc image ref for $arch> \\
  REPO_ROOT="$ROOT" \\
  os/hardware-gate/run-external-gate.sh

Native runner preflight for $arch without building artifacts:
  GOBLINS_OS_ARCH=$arch PREFLIGHT_ONLY=1 REPO_ROOT="$ROOT" os/hardware-gate/run-external-gate.sh

Artifact/SBOM build for native $arch without display proof:
  GOBLINS_OS_ARCH=$arch RUN_QEMU=0 REPO_ROOT="$ROOT" os/hardware-gate/run-external-gate.sh

Docker-emulated artifact/SBOM build for non-native local testing:
  GOBLINS_OS_ARCH=$arch RUN_QEMU=0 GOBLINS_OS_ALLOW_EMULATED_DOCKER=1 REPO_ROOT="$ROOT" os/hardware-gate/run-external-gate.sh

Final signoff row after the display-backed screenshots and runtime-built app proof exist:
  GOBLINS_OS_ARCH=$arch \\
  SCREENSHOT_RUN_DIR=os/screenshots/hardware-gate/$arch/<date> \\
  RUNTIME_ENGINE_MODE=<real-mode> \\
  RUNTIME_ENGINE_SOURCE=<real-engine-source> \\
  RUNTIME_ENGINE_CONFIG=<config-or-artifact-path> \\
  BUILT_ARTIFACT_PATH_URL=<real-built-app-path-or-url> \\
  ./os/hardware-gate/close-signoff.sh

Expected $arch proof files:
  os/iso/output/$arch/bootiso/goblins-os-$arch.iso
  os/iso/output/$arch/bootiso/goblins-os-$arch.iso.sha256
  os/iso/output/$arch/manifest-goblins-os-$arch.json
  os/signoff-proofs/sbom/$arch/rpm-packages.tsv
  os/screenshots/hardware-gate/$arch/<date>/${REQ_SCREENSHOTS[0]} ... ${REQ_SCREENSHOTS[$((${#REQ_SCREENSHOTS[@]} - 1))]}
  os/screenshots/hardware-gate/$arch/<date>/proof-manifest.json
EOF
}

signoff_block_contains() {
  local block="$1"
  local pattern="$2"

  printf '%s\n' "$block" | rg -q "$pattern"
}

signoff_block_has_real_field() {
  local block="$1"
  local pattern="$2"
  local line

  line="$(printf '%s\n' "$block" | rg "$pattern" || true)"
  [ -n "$line" ] || return 1
  ! printf '%s\n' "$line" | rg -qi 'n/a|not provided|not configured|requires|external gate|not exercised|none|unknown|missing|no live engine'
}

signoff_block_required_proof_is_complete() {
  local block="$1"
  local arch="${2:-}"

  signoff_block_contains "$block" "^- Runner: .+" || return 1
  if [ -n "$arch" ]; then
    signoff_block_contains "$block" "^- Architecture: $arch$" || return 1
    signoff_block_contains "$block" "^- ISO: .*goblins-os-$arch\\.iso" || return 1
  else
    signoff_block_contains "$block" "^- Architecture: (aarch64|x86_64)$" || return 1
    signoff_block_contains "$block" "^- ISO: .*goblins-os-(aarch64|x86_64)\\.iso" || return 1
  fi
  signoff_block_contains "$block" "^- ISO SHA256: [a-fA-F0-9]{64}$" || return 1
  signoff_block_contains "$block" "goblins-os-verify --installed-root /" || return 1
  signoff_block_contains "$block" "^- Verify result \\(blocked=0\\): pass" || return 1
  signoff_block_contains "$block" "^- Self-test command: .+" || return 1
  signoff_block_contains "$block" "^- Self-test result: pass" || return 1
  signoff_block_contains "$block" "^- Release evidence/SBOM checked: yes" || return 1
  signoff_block_contains "$block" "^- Screenshot dir: .+" || return 1
  if [ -n "$arch" ]; then
    signoff_block_contains "$block" "^- Screenshot dir: .*os/screenshots/hardware-gate/$arch/[^[:space:]]+" || return 1
  else
    signoff_block_contains "$block" "^- Screenshot dir: .*os/screenshots/hardware-gate/(aarch64|x86_64)/[^[:space:]]+" || return 1
  fi
  signoff_block_contains "$block" "^- Screenshot dir: .*not provided|stale screenshot|stale for this ISO|No fresh .*screenshots|missing current screenshot proof" && return 1
  signoff_block_has_real_field "$block" "^  - mode: .+" || return 1
  signoff_block_has_real_field "$block" "^  - engine source: .+" || return 1
  signoff_block_has_real_field "$block" "^  - built artifact path/URL: .+" || return 1
  signoff_block_contains "$block" "^- Motion/interactions checked: yes" || return 1
  signoff_block_contains "$block" "^- Gaming readiness checked: yes" || return 1
  signoff_block_contains "$block" "^- Install storage/bootloader/dual-boot checked: yes" || return 1
  return 0
}

signoff_block_from_line() {
  local start="$1"

  awk -v start="$start" 'NR < start { next } NR == start { print; next } /^## / { exit } { print }' "$SIGNOFF"
}

signoff_run_for_arch_is_complete() {
  local arch="$1"
  local start block

  [ -f "$SIGNOFF" ] || return 1
  while IFS= read -r start; do
    block="$(signoff_block_from_line "$start")"

    signoff_block_required_proof_is_complete "$block" "$arch" || continue
    signoff_block_contains "$block" "^- Current project completion status: complete$" || continue
    return 0
  done < <(rg -n "^## Manual Gate Run:" "$SIGNOFF" | cut -d: -f1)

  return 1
}

echo "# Shipping status check"
echo

check "SHIP.md declares Fedora bootc foundation" "rg -q 'Fedora bootc remains the OS foundation' \"$SHIP_DECL\""
check "SHIP.md declares no custom kernel ownership" "rg -q 'no custom kernel|custom kernel' \"$SHIP_DECL\""
check "SHIP.md declares OpenAI Sans not used" "rg -q 'OpenAI Sans' \"$SHIP_DECL\""
check "No OpenAI Sans references outside SHIP" "rg -qi --hidden --no-ignore-vcs --no-ignore 'OpenAI Sans|openai sans|openai-sans' os .github --glob '!os/hardware-gate/verify-shipping-status.sh' --glob '!os/hardware-gate/close-signoff.sh' --glob '!os/iso/output*/**' --glob '!os/signoff-proofs/**' --glob '!os/screenshots/**' --glob '!os/brand/*.png' --glob '!SHIP.md' > /tmp/openai_sans_check.txt; [ ! -s /tmp/openai_sans_check.txt ]"
check "No typography licensing TODOs in signing docs" "! rg -qi 'licensing\s+TODO|TODO.*licensing' \"$SHIP_DECL\" \"$RUNBOOK\" \"$SIGNOFF\""
check "Source package secret scan finds no live keys" "source_secret_scan"
check "Generated artifact/evidence secret scan finds no live keys" "goblins_os_artifact_secret_scan \"$ROOT\""
check "installed-root verifier enforces secret file and directory modes" "rg -q 'installed-openai-secret-file-mode-0600' crates/goblins-os-verify/src/main.rs && rg -q 'installed-openai-secret-file-empty' crates/goblins-os-verify/src/main.rs && rg -q 'var/lib/goblins-os/secrets/openai' crates/goblins-os-verify/src/main.rs"
check "hosted OpenAI direct path uses Responses API" "rg -q '/v1/responses' crates/goblins-os-core/src/resident.rs && ! rg -q '/v1/chat.?completions' crates/goblins-os-core/src/resident.rs"
check "OpenAI SDK bridge endpoints stay server-side" "rg -q 'GOBLINS_OS_AGENTS_SDK_RELAY_URL' os/etc/goblins-os/openai-secrets.env && rg -q 'GOBLINS_OS_CHATKIT_RELAY_URL' os/etc/goblins-os/openai-secrets.env && rg -q 'GOBLINS_OS_REALTIME_RELAY_URL' os/etc/goblins-os/openai-secrets.env && rg -q 'GOBLINS_OS_IMAGES_RELAY_URL' os/etc/goblins-os/openai-secrets.env && ! rg -q 'OPENAI_OS_' os/etc/goblins-os/openai-secrets.env && rg -q 'Official OpenAI Agents SDK' crates/goblins-os-core/src/service_catalog.rs && ! rg -q 'pub struct OpenAIService' crates/goblins-os-core/src/service_catalog.rs"
check "Build Studio uses official Agents SDK relay only server-side" "rg -q 'GOBLINS_OS_AGENTS_SDK_RELAY_URL' crates/goblins-os-core/src/app_builder.rs && rg -q 'official-openai-agents-sdk' crates/goblins-os-core/src/app_builder.rs && rg -q 'handoffs' crates/goblins-os-core/src/app_builder.rs && rg -q 'guardrails' crates/goblins-os-core/src/app_builder.rs && rg -q 'tracing' crates/goblins-os-core/src/app_builder.rs && rg -q 'sandbox-execution' crates/goblins-os-core/src/app_builder.rs && rg -q 'Build Studio never receives raw API keys' crates/goblins-os-core/src/service_catalog.rs && ! rg -q 'OpenAI-centered Linux OS' crates/goblins-os-core/src/app_builder.rs"
check "Codex local chat wire is loopback-only compatibility" "rg -q 'This compatibility wire is local-only' os/codex/config.toml && rg -q 'base_url = \"http://127.0.0.1:11434/v1\"' os/codex/config.toml && rg -q 'wire_api = \"chat\"' os/codex/config.toml"
check "core URL env contract ships Goblins-native names with reader-side compatibility only" "rg -Fq 'GOBLINS_OS_CORE_URL=http://127.0.0.1:8787' os/etc/goblins-os/environment && rg -Fq 'GOBLINS_OS_CORE_PORT=8787' os/etc/goblins-os/environment && ! rg -Fq 'OPENAI_OS_' os/etc/goblins-os/environment && rg -Fq 'export GOBLINS_OS_CORE_URL=\"\${GOBLINS_OS_CORE_URL:-\${OPENAI_OS_CORE_URL:-http://127.0.0.1:8787}}\"' os/session/goblins-os-session && ! rg -Fq 'export OPENAI_OS_CORE_URL=' os/session/goblins-os-session && rg -Fq 'std::env::var(\"GOBLINS_OS_CORE_PORT\")' crates/goblins-os-core/src/main.rs && rg -Fq 'std::env::var(\"OPENAI_OS_CORE_PORT\")' crates/goblins-os-core/src/main.rs && rg -Fq 'GOBLINS_OS_CORE_URL must be a local http endpoint' crates/goblins-os-open/src/main.rs"
check "shell user service does not directly export legacy core URL" "! rg -q 'Environment=OPENAI_OS_CORE_URL' os/systemd-user/org.goblins.OS.Shell.service"
check "desktop clients prefer GOBLINS_OS_CORE_URL over legacy alias" "rg -Fq 'env::var(\"GOBLINS_OS_CORE_URL\")' crates/goblins-os-installer/src/main.rs crates/goblins-os-login/src/main.rs crates/goblins-os-shell/src/main.rs crates/goblins-os-settings/src/main.rs crates/goblins-os-launcher/src/main.rs crates/goblins-os-control-center/src/main.rs crates/goblins-os-open/src/main.rs crates/goblins-os-file-builder/src/main.rs crates/goblins-os-resident/src/main.rs && rg -Fq 'env::var(\"OPENAI_OS_CORE_URL\")' crates/goblins-os-installer/src/main.rs crates/goblins-os-login/src/main.rs crates/goblins-os-shell/src/main.rs crates/goblins-os-settings/src/main.rs crates/goblins-os-launcher/src/main.rs crates/goblins-os-control-center/src/main.rs crates/goblins-os-open/src/main.rs crates/goblins-os-file-builder/src/main.rs crates/goblins-os-resident/src/main.rs"

check "rust job checks fmt" "rg -q 'cargo fmt --all --check' \"$WORKFLOW\""
check "rust job checks clippy" "rg -q 'clippy --workspace' \"$WORKFLOW\""
check "rust job checks native desktop tests" 'rg -q --fixed-strings '\''cargo test --workspace --features "$NATIVE_FEATURES"'\'' "$WORKFLOW"'
check "rust job checks release" "rg -q 'cargo build --release --workspace' \"$WORKFLOW\""
check "image job has verify" "rg -q 'goblins-os-verify' \"$WORKFLOW\""
check "image job checks blocked=0" "rg -q 'blocked=0' \"$WORKFLOW\""
check "image job has selftest" "rg -q 'goblins-os:selftest' \"$WORKFLOW\""
check "installer-iso job exists" "rg -q '^  installer-iso:' \"$WORKFLOW\""
check "installer-iso job generates release evidence" "rg -q -- '--release-evidence /out' \"$WORKFLOW\" && rg -q 'rpm-packages.command' \"$WORKFLOW\""
check "installer-iso job scans generated evidence for secrets" "rg -q 'goblins_os_artifact_secret_scan' \"$WORKFLOW\""
check "installer-iso job uploads release evidence artifacts" "rg -q 'goblins-os-release-evidence-' \"$WORKFLOW\""
check "workflow declares aarch64 runner" "rg -q 'ubuntu-24.04-arm|aarch64' \"$WORKFLOW\""
check "workflow declares x86_64 runner" "rg -q 'ubuntu-24.04|x86_64' \"$WORKFLOW\""
check "workflow asserts native runner architecture" "rg -q --fixed-strings 'Assert native runner architecture' \"$WORKFLOW\" && rg -q --fixed-strings 'test \"\$(uname -m)\" = \"\${{ matrix.expected_uname }}\"' \"$WORKFLOW\" && rg -q --fixed-strings 'expected_uname: aarch64' \"$WORKFLOW\" && rg -q --fixed-strings 'expected_uname: x86_64' \"$WORKFLOW\""

check "architecture contract records aarch64 artifact paths" "rg -q 'os/iso/output/aarch64/bootiso/goblins-os-aarch64\\.iso' os/release/architectures.toml && rg -q 'os/iso/output/aarch64/manifest-goblins-os-aarch64\\.json' os/release/architectures.toml"
check "architecture contract records x86_64 artifact paths" "rg -q 'os/iso/output/x86_64/bootiso/goblins-os-x86_64\\.iso' os/release/architectures.toml && rg -q 'os/iso/output/x86_64/manifest-goblins-os-x86_64\\.json' os/release/architectures.toml"
check "architecture contract records per-architecture SBOM paths" "rg -q 'os/signoff-proofs/sbom/aarch64/rpm-packages\\.tsv' os/release/architectures.toml && rg -q 'os/signoff-proofs/sbom/x86_64/rpm-packages\\.tsv' os/release/architectures.toml"
check "architecture contract records per-architecture QEMU commands" "rg -q 'qemu-system-aarch64' os/release/architectures.toml && rg -q 'qemu-system-x86_64' os/release/architectures.toml"
check "architecture contract records aarch64 UEFI pflash contract" "rg -q 'virt,accel=kvm,gic-version=max' os/release/architectures.toml && rg -q 'AARCH64_UEFI_CODE' os/release/architectures.toml && rg -q 'AARCH64_UEFI_VARS' os/release/architectures.toml"
check "architecture contract records native KVM proof" "rg -q 'qemu_accel = \"kvm\"' os/release/architectures.toml"
check "architecture contract rejects aarch64 emulation baseline" "rg -q 'do not use x86_64 emulation as baseline' os/release/architectures.toml"

check "ISO builder supports GOBLINS_OS_ARCH" "rg -q 'GOBLINS_OS_ARCH' os/iso/build-iso.sh"
check "ISO builder writes architecture ISO names" "rg -q 'goblins-os-\\\$ARCH.iso' os/iso/build-iso.sh"
check "ISO builder host runtime is Docker-only" "rg -q \"expected docker\" os/iso/build-iso.sh && ! rg -q 'docker or podman' os/iso/build-iso.sh && ! rg -q 'GOBLINS_OS_PODMAN_SUDO' os/iso/build-iso.sh && ! rg -q 'run_podman_builder' os/iso/build-iso.sh"
check "ISO builder uses Docker local registry handoff" "rg -q 'GOBLINS_OS_CONTAINER_RUNTIME' os/iso/build-iso.sh && rg -q 'host.docker.internal' os/iso/build-iso.sh && rg -q 'docker push' os/iso/build-iso.sh && ! rg -q -- '--rm -it' os/iso/build-iso.sh"
check "ISO builder separates local Docker handoff from shippable release source" "rg -q 'GOBLINS_OS_BIB_SOURCE_IMAGE' os/iso/build-iso.sh && rg -q 'GOBLINS_OS_SHIPPABLE_RELEASE' os/iso/build-iso.sh && rg -q 'shippable release media cannot track local/test-only installer payload ref' os/iso/build-iso.sh"
check "ISO builder supports explicit Docker platform for non-release artifact testing" "rg -q 'GOBLINS_OS_DOCKER_PLATFORM' os/iso/build-iso.sh && rg -q 'docker build --platform \"[$]DOCKER_PLATFORM\"' os/iso/build-iso.sh && rg -q -- '--platform \"[$]DOCKER_PLATFORM\"' os/iso/build-iso.sh && rg -q '\"docker_platform\": \"[$]DOCKER_PLATFORM\"' os/iso/build-iso.sh"
check "ISO builder fails fast when Docker emulation cannot run rustc" "rg -q 'verify_docker_emulation_runtime' os/iso/build-iso.sh && rg -q 'emulation cannot run rustc' os/iso/build-iso.sh && rg -q 'use a native [$]ARCH runner' os/iso/build-iso.sh"
check "workflow installer ISO uses Docker image and evidence steps" "rg -q 'docker build -f os/bootc/Containerfile' \"$WORKFLOW\" && rg -q 'docker run --rm' \"$WORKFLOW\" && rg -q 'GOBLINS_OS_CONTAINER_RUNTIME=docker' \"$WORKFLOW\""
check "external gate supports qemu-system-aarch64" "rg -q 'qemu-system-aarch64' os/hardware-gate/run-external-gate.sh"
check "external gate supports qemu-system-x86_64" "rg -q 'qemu-system-x86_64' os/hardware-gate/run-external-gate.sh"
check "external gate passes container runtime to ISO builder" "rg -q 'GOBLINS_OS_CONTAINER_RUNTIME=\"[$]CONTAINER_RUNTIME\"' os/hardware-gate/run-external-gate.sh"
check "external gate host runtime is Docker-only" "rg -q 'GOBLINS_OS_CONTAINER_RUNTIME must be docker' os/hardware-gate/run-external-gate.sh && ! rg -q 'docker or podman' os/hardware-gate/run-external-gate.sh && ! rg -q 'GOBLINS_OS_PODMAN_SUDO' os/hardware-gate/run-external-gate.sh && ! rg -q 'sudo podman' os/hardware-gate/run-external-gate.sh"
check "external gate requires real bootc source image for display proof" "rg -q 'Display-backed shipping proof requires GOBLINS_OS_BIB_SOURCE_IMAGE' os/hardware-gate/run-external-gate.sh && rg -q 'GOBLINS_OS_BIB_SOURCE_IMAGE=\"[$]BIB_SOURCE_IMAGE\"' os/hardware-gate/run-external-gate.sh && rg -q 'GOBLINS_OS_SHIPPABLE_RELEASE=\"[$]SHIPPABLE_RELEASE\"' os/hardware-gate/run-external-gate.sh"
check "runbook documents real release image source" "rg -q 'RELEASE_IMAGE=<registry>/<namespace>/goblins-os:[$]ARCH' os/hardware-gate/runbook.md && rg -q '\"installer_payload_source_local_only\": false' os/hardware-gate/runbook.md"
check "external gate requires native KVM acceleration" "rg -q 'QEMU_ACCEL must be kvm' os/hardware-gate/run-external-gate.sh && rg -q '/dev/kvm' os/hardware-gate/run-external-gate.sh"
check "external gate uses aarch64 UEFI pflash code and vars" "rg -q 'if=pflash,format=raw,readonly=on,file=[$]AARCH64_UEFI_CODE' os/hardware-gate/run-external-gate.sh && rg -q 'if=pflash,format=raw,file=[$]AARCH64_UEFI_VARS' os/hardware-gate/run-external-gate.sh"
check "external gate copies aarch64 UEFI vars template" "rg -q 'AARCH64_UEFI_VARS_TEMPLATE' os/hardware-gate/run-external-gate.sh && rg -q 'cp \"[$]template\" \"[$]AARCH64_UEFI_VARS\"' os/hardware-gate/run-external-gate.sh"
check "external gate requires Linux host before display proof" "rg -q 'External display-backed gate requires a native Linux host with Docker and QEMU' os/hardware-gate/run-external-gate.sh"
check "external gate fails non-native architecture before build" "rg -q 'Requested [$]ARCH gate on [$]HOST_ARCH host' os/hardware-gate/run-external-gate.sh && rg -q 'must be produced on a native [$]ARCH Linux runner' os/hardware-gate/run-external-gate.sh"
check "external gate allows explicit Docker emulation for artifact testing only" "rg -q 'GOBLINS_OS_ALLOW_EMULATED_DOCKER' os/hardware-gate/run-external-gate.sh && rg -q 'Docker-emulated [$]ARCH artifact testing' os/hardware-gate/run-external-gate.sh && rg -q 'not release proof' os/hardware-gate/run-external-gate.sh && rg -q 'Docker artifact testing on a non-native machine' os/hardware-gate/runbook.md"
check "external gate fails low disk before build" "rg -q 'MIN_HOST_FREE_GB' os/hardware-gate/run-external-gate.sh && rg -q 'Repository filesystem needs at least' os/hardware-gate/run-external-gate.sh && rg -q 'VM scratch filesystem needs at least' os/hardware-gate/run-external-gate.sh"
check "external gate checks container runtime health before build" "rg -q 'CONTAINER_RUNTIME_HEALTH_TIMEOUT_SECS' os/hardware-gate/run-external-gate.sh && rg -q 'Checking [$]CONTAINER_RUNTIME health' os/hardware-gate/run-external-gate.sh && rg -q 'did not answer within' os/hardware-gate/run-external-gate.sh"
check "external gate has fail-closed preflight-only mode" "rg -q 'PREFLIGHT_ONLY=1' os/hardware-gate/run-external-gate.sh && rg -q 'Preflight passed for native [$]ARCH release runner' os/hardware-gate/run-external-gate.sh && rg -q 'Docker artifact-only preflight passed for [$]ARCH on [$]HOST_ARCH; not release proof' os/hardware-gate/run-external-gate.sh && rg -q 'No image, ISO, SBOM, screenshot, or signoff artifact was generated' os/hardware-gate/run-external-gate.sh"
check "runbook documents external preflight command" "rg -q 'PREFLIGHT_ONLY=1 GOBLINS_OS_ARCH' os/hardware-gate/runbook.md && rg -q 'does not create shipping artifacts or satisfy proof by itself' os/hardware-gate/runbook.md"
check "external gate allows artifact-only mode without pretending proof is complete" "rg -q 'RUN_QEMU=0: built and verified artifacts only' os/hardware-gate/run-external-gate.sh"
check "external gate verifies ISO SHA256" "rg -q 'sha256sum -c' os/hardware-gate/run-external-gate.sh"
check "external gate generates release evidence" "rg -q -- '--release-evidence /out' os/hardware-gate/run-external-gate.sh"
check "external gate requires RPM SBOM TSV" "rg -q 'rpm-packages.tsv' os/hardware-gate/run-external-gate.sh"
check "installer policy exposes dual-boot preservation path" "rg -q 'dual_boot_preservation' crates/goblins-os-core/src/install_targets.rs"
check "installer policy exposes dual-boot preflight" "rg -q 'dual_boot_preflight' crates/goblins-os-core/src/install_targets.rs"
check "installer policy exposes safe dual-boot route" "rg -q 'dual_boot_safe_route' crates/goblins-os-core/src/install_targets.rs && rg -q 'DualBootSafeRoute' crates/goblins-os-core/src/install_targets.rs && rg -q 'Install beside an existing OS' crates/goblins-os-core/src/install_targets.rs && rg -q 'Install Goblins OS Beside Another OS' crates/goblins-os-core/src/install_targets.rs && rg -q 'every filesystem that will be formatted' crates/goblins-os-core/src/install_targets.rs"
check "installer policy exposes simple install erase scope" "rg -q 'simple_install_scope' crates/goblins-os-core/src/install_targets.rs && rg -q 'blank internal disk' crates/goblins-os-core/src/install_targets.rs && rg -q 'formats the new Goblins OS root filesystem' crates/goblins-os-core/src/install_targets.rs"
check "installer policy exposes bootloader recovery guidance" "rg -q 'bootloader_recovery' crates/goblins-os-core/src/install_targets.rs && rg -q 'firmware boot options' crates/goblins-os-core/src/install_targets.rs"
check "installer policy exposes storage review checklist" "rg -q 'storage_review_checklist' crates/goblins-os-core/src/install_targets.rs && rg -q 'StorageReviewItem' crates/goblins-os-core/src/install_targets.rs"
check "installer policy exposes recommended install path choices" "rg -q 'install_path_options' crates/goblins-os-core/src/install_targets.rs && rg -q 'Keep my current OS' crates/goblins-os-core/src/install_targets.rs && rg -q 'Replace one blank disk' crates/goblins-os-core/src/install_targets.rs && rg -q 'Advanced storage' crates/goblins-os-core/src/install_targets.rs"
check "installer policy exposes pre-write boot formatting plan" "rg -q 'pre_write_install_plan' crates/goblins-os-core/src/install_targets.rs && rg -q 'InstallPlanItem' crates/goblins-os-core/src/install_targets.rs && rg -q 'fresh GPT layout' crates/goblins-os-core/src/install_targets.rs && rg -q 'bootloader/EFI target' crates/goblins-os-core/src/install_targets.rs && rg -q 'xfs root' crates/goblins-os-core/src/install_targets.rs && rg -q 'TPM2 LUKS' crates/goblins-os-core/src/install_targets.rs"
check "installer policy exposes dual-boot readiness checklist" "rg -q 'dual_boot_readiness' crates/goblins-os-core/src/install_targets.rs && rg -q 'DualBootReadinessItem' crates/goblins-os-core/src/install_targets.rs && rg -q 'Windows readiness' crates/goblins-os-core/src/install_targets.rs && rg -q 'macOS readiness' crates/goblins-os-core/src/install_targets.rs && rg -q 'Linux readiness' crates/goblins-os-core/src/install_targets.rs && rg -q 'Other OS or data readiness' crates/goblins-os-core/src/install_targets.rs && rg -q 'Dedicated disk readiness' crates/goblins-os-core/src/install_targets.rs"
check "installer policy exposes dual-boot assistant choices" "rg -q 'dual_boot_choices' crates/goblins-os-core/src/install_targets.rs && rg -q 'DualBootChoice' crates/goblins-os-core/src/install_targets.rs && rg -q 'Keep Windows' crates/goblins-os-core/src/install_targets.rs && rg -q 'suspend BitLocker' crates/goblins-os-core/src/install_targets.rs && rg -q 'Keep macOS' crates/goblins-os-core/src/install_targets.rs && rg -q 'Keep Linux' crates/goblins-os-core/src/install_targets.rs && rg -q 'Keep another OS or data' crates/goblins-os-core/src/install_targets.rs && rg -q 'Use a dedicated disk' crates/goblins-os-core/src/install_targets.rs"
check "installer policy exposes guided dual-boot steps" "rg -q 'dual_boot_guide' crates/goblins-os-core/src/install_targets.rs && rg -q 'Disk Management' crates/goblins-os-core/src/install_targets.rs && rg -q 'Disk Utility' crates/goblins-os-core/src/install_targets.rs && rg -q 'Startup menu' crates/goblins-os-core/src/install_targets.rs && rg -q 'Final storage review' crates/goblins-os-core/src/install_targets.rs"
check "installer policy exposes dual-boot decision map" "rg -q 'dual_boot_decision_map' crates/goblins-os-core/src/install_targets.rs && rg -q 'DualBootDecision' crates/goblins-os-core/src/install_targets.rs && rg -q 'Windows beside Goblins OS' crates/goblins-os-core/src/install_targets.rs && rg -q 'macOS beside Goblins OS' crates/goblins-os-core/src/install_targets.rs && rg -q 'Linux beside Goblins OS' crates/goblins-os-core/src/install_targets.rs && rg -q 'Separate disk' crates/goblins-os-core/src/install_targets.rs"
check "installer policy exposes advanced storage handoff" "rg -q 'full_storage_installer' crates/goblins-os-core/src/install_targets.rs && rg -q '/usr/libexec/goblins-os/goblins-os-full-installer' crates/goblins-os-core/src/install_targets.rs && rg -q 'org.goblins.OS.FullInstaller.desktop' crates/goblins-os-core/src/install_targets.rs && rg -q 'Advanced storage' crates/goblins-os-core/src/install_targets.rs"
check "installer policy exposes dual-boot quick start" "rg -q 'dual_boot_quick_start' crates/goblins-os-core/src/install_targets.rs && rg -q 'Install beside another OS' crates/goblins-os-core/src/install_targets.rs && rg -q 'Confirm preserve, format, and bootloader' crates/goblins-os-core/src/install_targets.rs && rg -q 'Test every boot path' crates/goblins-os-core/src/install_targets.rs"
check "installer policy explains firmware startup picker" "rg -q 'firmware startup menu or boot picker' crates/goblins-os-core/src/install_targets.rs"
check "installer policy covers Windows macOS Linux and other OS" "rg -q 'Windows, macOS, Linux, or another OS' crates/goblins-os-core/src/install_targets.rs"
check "installer policy protects APFS and EFI partitions" "rg -q 'macOS/APFS, Linux, other OS, recovery, and EFI partitions' crates/goblins-os-core/src/install_targets.rs"
check "installer API explains blocked simple erase dual-boot handoff" "rg -q 'The simple erase flow will not install' crates/goblins-os-core/src/install_targets.rs && rg -q 'open advanced storage' crates/goblins-os-core/src/install_targets.rs && rg -q 'select only unallocated free space' crates/goblins-os-core/src/install_targets.rs"
check "installer scanner detects BitLocker Microsoft Reserved Apple HFS and Linux filesystems" "rg -q 'bitlocker' crates/goblins-os-core/src/install_targets.rs && rg -q 'e3c9e316-0b5c-4db8-817d-f92df00215ae' crates/goblins-os-core/src/install_targets.rs && rg -q '48465300-0000-11aa-aa11-00306543ecac' crates/goblins-os-core/src/install_targets.rs && rg -q 'f2fs' crates/goblins-os-core/src/install_targets.rs && rg -q 'bcachefs' crates/goblins-os-core/src/install_targets.rs"
check "installer scanner test covers Windows macOS Linux and data partitions" "rg -q 'scans_sys_block_and_routes_existing_operating_systems_to_manual_storage' crates/goblins-os-core/src/install_targets.rs && rg -q 'TYPE=ntfs' crates/goblins-os-core/src/install_targets.rs && rg -q 'TYPE=apfs' crates/goblins-os-core/src/install_targets.rs && rg -q 'TYPE=crypto_LUKS' crates/goblins-os-core/src/install_targets.rs && rg -q 'TYPE=zfs_member' crates/goblins-os-core/src/install_targets.rs"
check "installer render proof uses Docker fixture for Windows macOS Linux and data partitions" "rg -q 'TYPE=ntfs' os/bootc/render-screens.sh && rg -q 'TYPE=apfs' os/bootc/render-screens.sh && rg -q 'TYPE=crypto_LUKS' os/bootc/render-screens.sh && rg -q 'TYPE=zfs_member' os/bootc/render-screens.sh"
check "installer render proof captures full storage handoff screenshot" "rg -q 'Open advanced storage handoff' os/bootc/render-screens.sh && rg -q '27-dual-boot-preserve-existing-os\\.png' os/bootc/render-screens.sh"
check "desktop render proof documents Docker harness" "rg -q 'DOCKER_BUILDKIT=1 docker build' os/bootc/render-desktop.suffix.Dockerfile && ! rg -q 'podman build' os/bootc/render-desktop.suffix.Dockerfile"
check "render proofs do not use legacy demo or seeded app hooks" "rg -q 'GOBLINS_OS_RENDER_QUERY' os/bootc/render-screens.sh crates/goblins-os-launcher/src/main.rs && ! rg -q 'GOBLINS_OS_SHELL_DEMO|GOBLINS_OS_LAUNCHER_DEMO' os/bootc/render-screens.sh crates/goblins-os-shell/src/main.rs crates/goblins-os-launcher/src/main.rs && ! rg -q 'Render/design proof: seed' crates/goblins-os-launcher/src/main.rs"
check "installer UI shows best path for dual boot" "rg -q 'Best dual-boot path' crates/goblins-os-installer/src/main.rs"
check "installer UI shows simple path choice before disk erase" "rg -q 'Choose install path' crates/goblins-os-installer/src/main.rs && rg -q 'Replace one blank disk' crates/goblins-os-installer/src/main.rs"
check "installer UI makes dual boot the first storage choice" "rg -q 'Keeping another OS or data?' crates/goblins-os-installer/src/main.rs && rg -q 'start with advanced storage' crates/goblins-os-installer/src/main.rs"
check "installer UI renders recommended install paths" "rg -q 'append_install_path_options' crates/goblins-os-installer/src/main.rs && rg -q 'Recommended install paths' crates/goblins-os-installer/src/main.rs && rg -q 'install_path_options_summary' crates/goblins-os-installer/src/main.rs"
check "installer UI renders pre-write boot formatting plan" "rg -q 'append_pre_write_install_plan' crates/goblins-os-installer/src/main.rs && rg -q 'Before writing to disk' crates/goblins-os-installer/src/main.rs && rg -q 'pre_write_install_plan_summary' crates/goblins-os-installer/src/main.rs && rg -q 'dual boot and custom formatting stay in advanced storage' crates/goblins-os-installer/src/main.rs"
check "installer UI renders dual-boot quick start" "rg -q 'append_dual_boot_quick_start' crates/goblins-os-installer/src/main.rs && rg -q 'Dual-boot quick start' crates/goblins-os-installer/src/main.rs && rg -q 'final preserve, format, and bootloader summary' crates/goblins-os-installer/src/main.rs && rg -q 'dual_boot_quick_start_summary' crates/goblins-os-installer/src/main.rs"
check "installer UI renders dual-boot readiness checklist" "rg -q 'append_dual_boot_readiness' crates/goblins-os-installer/src/main.rs && rg -q 'Dual-boot readiness' crates/goblins-os-installer/src/main.rs && rg -q 'Use this checklist before writing storage changes' crates/goblins-os-installer/src/main.rs && rg -q 'dual_boot_readiness_summary' crates/goblins-os-installer/src/main.rs"
check "installer UI renders dual-boot assistant choices" "rg -q 'append_dual_boot_choices' crates/goblins-os-installer/src/main.rs && rg -q 'Dual-boot assistant' crates/goblins-os-installer/src/main.rs && rg -q 'Pick the operating system you are keeping' crates/goblins-os-installer/src/main.rs && rg -q 'dual_boot_choices_summary' crates/goblins-os-installer/src/main.rs"
check "installer UI renders dual-boot decision map" "rg -q 'append_dual_boot_decision_map' crates/goblins-os-installer/src/main.rs && rg -q 'Dual-boot decision map' crates/goblins-os-installer/src/main.rs && rg -q 'Best for:' crates/goblins-os-installer/src/main.rs && rg -q 'dual_boot_decision_map_summary' crates/goblins-os-installer/src/main.rs"
check "installer UI renders safe dual-boot route" "rg -q 'append_dual_boot_safe_route' crates/goblins-os-installer/src/main.rs && rg -q 'dual_boot_safe_route_summary' crates/goblins-os-installer/src/main.rs && rg -q 'Install beside an existing OS' crates/goblins-os-installer/src/main.rs && rg -q 'installer_dual_boot_safe_route_launch_error' crates/goblins-os-installer/src/main.rs"
check "installer UI exposes advanced storage button" "rg -q 'append_full_storage_installer_handoff' crates/goblins-os-installer/src/main.rs && rg -q 'Open advanced storage' crates/goblins-os-installer/src/main.rs && rg -q 'launch_full_storage_installer' crates/goblins-os-installer/src/main.rs && rg -q 'StorageInstallerCommand' crates/goblins-os-installer/src/main.rs"
check "installer UI turns detected existing OS disks into preservation actions" "rg -q 'Detected systems are actions' crates/goblins-os-installer/src/main.rs && rg -q 'Open advanced storage from detected disk' crates/goblins-os-installer/src/main.rs && rg -q 'installer_detected_disk_full_storage_launch_error' crates/goblins-os-installer/src/main.rs && rg -q 'row.set_sensitive(target.eligible || preservation_handoff)' crates/goblins-os-installer/src/main.rs"
check "installer wizard labels are title case and not shouted" "rg -q 'Step · Install' crates/goblins-os-installer/src/main.rs && rg -q 'Final Step · Confirm' crates/goblins-os-installer/src/main.rs && rg -q 'Required Confirmation' crates/goblins-os-installer/src/main.rs && rg -q '.gos-onboarding-kicker' crates/goblins-os-design/src/lib.rs && rg -q 'text-transform: none;' crates/goblins-os-design/src/lib.rs && ! rg -q 'STEP ·|FINAL STEP|REQUIRED CONFIRMATION|WHAT HAPPENED|letter-spacing: 2\\.2px' crates/goblins-os-installer/src/main.rs crates/goblins-os-design/src/lib.rs"
check "installer UI shows detected OS preservation checklist" "rg -q 'Preservation checklist:' crates/goblins-os-installer/src/main.rs && rg -q 'Back up and save recovery keys' crates/goblins-os-installer/src/main.rs && rg -q 'detected_system_preparation_hint' crates/goblins-os-installer/src/main.rs && rg -q 'test every preserved system from the firmware boot picker' crates/goblins-os-installer/src/main.rs"
check "installer UI exposes guided install-beside launcher" "rg -q 'append_dual_boot_launcher' crates/goblins-os-installer/src/main.rs && rg -q 'Install beside another OS' crates/goblins-os-installer/src/main.rs && rg -q 'What are you keeping?' crates/goblins-os-installer/src/main.rs && rg -q 'installer_dual_boot_choice_launch_error' crates/goblins-os-installer/src/main.rs && rg -q '.gos-dual-boot-choice' crates/goblins-os-design/src/lib.rs"
check "installer UI shows erase scope and boot recovery" "rg -q 'Simple install scope' crates/goblins-os-installer/src/main.rs && rg -q 'Erase scope' crates/goblins-os-installer/src/main.rs && rg -q 'Startup recovery' crates/goblins-os-installer/src/main.rs && rg -q 'After reboot' crates/goblins-os-installer/src/main.rs"
check "installer UI renders storage review checklist" "rg -q 'append_storage_review_checklist' crates/goblins-os-installer/src/main.rs && rg -q 'Storage review checklist' crates/goblins-os-installer/src/main.rs"
check "installer UI renders guided dual-boot steps" "rg -q 'append_dual_boot_guide' crates/goblins-os-installer/src/main.rs && rg -q 'Dual-boot guide' crates/goblins-os-installer/src/main.rs"
check "installer UI labels keep existing OS path" "rg -q 'Keep an existing OS' crates/goblins-os-installer/src/main.rs"
check "installer network copy hides internal service wording" "rg -q 'The network service is not responding on this device' crates/goblins-os-installer/src/main.rs && rg -q 'Networking not ready' crates/goblins-os-installer/src/main.rs && ! rg -q 'NetworkManager isn.t responding|Networking unavailable' crates/goblins-os-installer/src/main.rs"
check "installer copy hides bootc and Anaconda implementation labels" "rg -q 'Install readiness' crates/goblins-os-installer/src/main.rs && ! rg -q 'Installer engine' crates/goblins-os-installer/src/main.rs && ! rg -q 'bootc installer' crates/goblins-os-installer/src/main.rs && ! rg -q 'bootc install command' crates/goblins-os-installer/src/main.rs && ! rg -q 'Fedora/Anaconda' crates/goblins-os-installer/src/bin/goblins-os-full-installer.rs && ! rg -q 'Anaconda;' os/applications/org.goblins.OS.FullInstaller.desktop"
check "native design system uses Goblins-native naming" "rg -q 'GOBLINS_NATIVE_CSS' crates/goblins-os-design/src/lib.rs && ! rg -q -e 'OPENAI_NATIVE_CSS' -e 'OpenAI-native' crates/goblins-os-design/src/lib.rs crates/goblins-os-installer/src/main.rs crates/goblins-os-login/src/main.rs crates/goblins-os-launcher/src/main.rs crates/goblins-os-control-center/src/main.rs os/plymouth/goblins-os/goblins-os.script os/iso/config.toml"
check "boot splash uses Goblins mark for OS identity" "rg -q 'brand/anaconda/sidebar-logo.png' os/bootc/Containerfile && rg -q 'Goblins OS boot splash.*Goblins mark' os/plymouth/goblins-os/goblins-os.plymouth && ! rg -q 'brand/OpenAI-white-monoblossom.png[[:space:]]*\\\\' os/bootc/Containerfile"
check "installer and login product copy uses Goblins desktop naming" "rg -q 'Goblins-native desktop' crates/goblins-os-installer/src/main.rs && rg -q 'Enter Goblins OS desktop' crates/goblins-os-installer/src/main.rs && rg -q 'Unlock Goblins OS desktop' crates/goblins-os-login/src/main.rs && rg -q 'Goblins OS desktop unlock was rejected by local OS services' crates/goblins-os-login/src/main.rs && ! rg -q -e 'OpenAI-native desktop' -e 'Enter OpenAI desktop' -e 'Unlock OpenAI desktop' -e 'OpenAI desktop unlock' crates/goblins-os-installer/src/main.rs crates/goblins-os-login/src/main.rs"
check "desktop metadata uses Goblins identity for OS surfaces" "rg -q 'Comment=Native Goblins OS identity gate' os/applications/org.goblins.OS.Login.desktop && rg -q 'Comment=Native recovery checks for the boot image, services, models, and Goblins identity' os/applications/org.goblins.OS.Recovery.desktop && rg -q 'Comment=Native Goblins OS policy, enterprise controls, data boundaries, and permission gates' os/applications/org.goblins.OS.Policy.desktop"
check "OpenAI service launcher copy is Goblins-native" "rg -Fq 'unknown Goblins OS service id' crates/goblins-os-open/src/main.rs && rg -Fq 'Goblins OS service {service_id} is blocked by the active Goblins OS policy' crates/goblins-os-open/src/main.rs && ! rg -Fq 'OpenAI OS service' crates/goblins-os-open/src/main.rs && rg -Fq 'Description=Goblins OS local AI service core' os/systemd/goblins-os-core.service"
check "installer policy copy hides raw installer engine name" "rg -q 'advanced storage' crates/goblins-os-core/src/install_targets.rs && rg -q 'installer' crates/goblins-os-core/src/install_targets.rs && rg -q 'Goblins OS disk installer' crates/goblins-os-core/src/install_targets.rs && ! rg -q 'Anaconda' crates/goblins-os-core/src/install_targets.rs && ! rg -q 'bootc installer' crates/goblins-os-core/src/install_targets.rs && ! rg -q -e 'Ready for guarded bootc install preparation' -e 'bootc install was started by the Goblins OS core' -e 'could not spawn bootc install' -e 'core may spawn bootc install' crates/goblins-os-core/src/install_targets.rs"
check "installer UI copy uses advanced storage path" "rg -q 'open advanced storage' crates/goblins-os-core/src/install_targets.rs crates/goblins-os-installer/src/main.rs && rg -q 'advanced storage' crates/goblins-os-core/src/install_targets.rs crates/goblins-os-installer/src/main.rs && ! rg -q -e 'ISO manual storage' -e 'ISO Installation Destination' -e 'Installation Destination in the ISO' -e 'manual storage from the ISO' -e 'Use Installation Destination' crates/goblins-os-core/src/install_targets.rs crates/goblins-os-installer/src/main.rs"
check "installer docs use advanced storage language" "rg -q 'advanced storage Installation Destination' os/hardware-gate/runbook.md && rg -q 'advanced storage' \"$SHIP_DECL\" os/hardware-gate/runbook.md && rg -q 'advanced storage' os/iso/config.toml && ! rg -q -e 'uses Anaconda Installation Destination/manual storage' -e 'Anaconda manual storage summary' -e 'visible in Anaconda' -e 'to Anaconda manual storage' -e 'choose the disk/storage layout in Anaconda' \"$SHIP_DECL\" os/iso/config.toml os/hardware-gate/runbook.md"
check "settings detail copy hides raw setup state" "rg -Fq '(\"not configured\", \"not set up\")' crates/goblins-os-settings/src/main.rs && rg -Fq '(\"not available yet\", \"not ready yet\")' crates/goblins-os-settings/src/main.rs"
check "settings native app handoff uses image-owned copy" "rg -q 'Not Included' crates/goblins-os-settings/src/main.rs && rg -q 'included in the full Goblins OS image' crates/goblins-os-settings/src/main.rs && ! rg -q -e 'is not installed on this image' -e 'Not Installed' crates/goblins-os-settings/src/main.rs"
check "settings storage pressure plan is actionable" "rg -q 'append_storage_pressure_plan' crates/goblins-os-settings/src/main.rs && rg -q 'Storage pressure plan' crates/goblins-os-settings/src/main.rs && rg -q 'Open Disk Usage Analyzer' crates/goblins-os-settings/src/main.rs && rg -q 'Open Disks' crates/goblins-os-settings/src/main.rs && rg -q 'automatic removal of aged files' crates/goblins-os-settings/src/main.rs && ! rg -q 'needs GNOME' crates/goblins-os-settings/src/main.rs"
check "privacy cleanup copy uses aged wording" "rg -q 'Remove aged temporary files' crates/goblins-os-settings/src/main.rs crates/goblins-os-core/src/privacy.rs && ! rg -q 'Remove old temporary files' crates/goblins-os-settings/src/main.rs crates/goblins-os-core/src/privacy.rs"
check "settings built-in capability copy avoids install-manager wording" "rg -q 'Bluetooth support is not ready on this device' crates/goblins-os-settings/src/main.rs && rg -q 'Audio routing support is not ready in this build' crates/goblins-os-settings/src/main.rs && rg -q 'Codex · not included' crates/goblins-os-settings/src/main.rs && rg -q 'Required service support is not included in this build' crates/goblins-os-settings/src/main.rs"
check "core built-in capability copy avoids install-manager wording" "rg -q 'Bluetooth support is not ready on this device' crates/goblins-os-core/src/bluetooth.rs && rg -q 'Audio routing controls are not ready' crates/goblins-os-core/src/audio.rs && rg -q 'Codex account support is not included in this build' crates/goblins-os-core/src/codex.rs && ! rg -q -e 'Bluetooth support is not installed' -e 'WirePlumber control tooling is not installed' -e 'Codex CLI is not installed' crates/goblins-os-core/src/bluetooth.rs crates/goblins-os-core/src/audio.rs crates/goblins-os-core/src/codex.rs"
check "ISO/runbook document Custom or Reclaim Space dual boot" "rg -q 'Custom/manual storage or Reclaim Space' os/iso/config.toml os/hardware-gate/runbook.md"
check "ISO/runbook document advanced storage handoff" "rg -q 'Open advanced storage' os/iso/config.toml os/hardware-gate/runbook.md && rg -q 'Install Goblins OS Beside Another OS' os/hardware-gate/runbook.md"
check "runbook documents disk and Docker preflight" "rg -q '120 GiB free' os/hardware-gate/runbook.md && rg -q 'docker info' os/hardware-gate/runbook.md"
check "SHIP documents free-space or dedicated-disk dual boot" "rg -q 'unallocated free space or a dedicated disk' \"$SHIP_DECL\""
check "SHIP documents safe install-beside route" "rg -q 'Install beside an existing OS' \"$SHIP_DECL\" && rg -q 'every filesystem that will be formatted' \"$SHIP_DECL\""
check "SHIP documents dual-boot readiness checklist" "rg -q 'Dual-boot readiness' \"$SHIP_DECL\" && rg -q 'Windows/macOS/Linux/other OS' \"$SHIP_DECL\""
check "SHIP documents dual-boot assistant" "rg -q 'Dual-boot assistant' \"$SHIP_DECL\""
check "SHIP documents dual-boot decision map" "rg -q 'Dual-boot decision map' \"$SHIP_DECL\" && rg -q 'separate-disk rows' \"$SHIP_DECL\""
check "SHIP documents pre-write boot formatting plan" "rg -q 'Before writing to disk' \"$SHIP_DECL\" && rg -q 'fresh GPT layout' \"$SHIP_DECL\" && rg -q 'bootloader/EFI target' \"$SHIP_DECL\" && rg -q 'xfs root' \"$SHIP_DECL\""
check "SHIP documents advanced storage entry point" "rg -q 'Open advanced storage' \"$SHIP_DECL\" && rg -q 'Install Goblins OS Beside Another OS' \"$SHIP_DECL\""
check "external gate names preserved existing OS partitions" "rg -q 'preserved Windows/macOS/APFS/Linux/other OS/recovery/EFI partitions' os/hardware-gate/run-external-gate.sh"
check "external gate documents advanced storage entry point" "rg -q 'Open advanced storage' os/hardware-gate/run-external-gate.sh && rg -q 'Install Goblins OS Beside Another OS' os/hardware-gate/run-external-gate.sh"
check "bootc image includes advanced storage handoff" "rg -q 'anaconda-live' os/bootc/Containerfile && rg -q 'goblins-os-full-installer' os/bootc/Containerfile && rg -q 'org.goblins.OS.FullInstaller.desktop' os/bootc/Containerfile && rg -q 'desktop-file-validate /usr/share/applications/org.goblins.OS.FullInstaller.desktop' os/bootc/Containerfile"
check "core AI exposes notification context route" "rg -Fq '/v1/ai/notification-context' crates/goblins-os-core/src/main.rs && rg -Fq 'ask_notification_context' crates/goblins-os-core/src/main.rs"
check "core AI notification context is permission gated" "rg -Fq 'policy_state_for_control(\"notification-context\")' crates/goblins-os-core/src/ai.rs && rg -Fq 'Allow notification context in Privacy & Permissions' crates/goblins-os-core/src/ai.rs"
check "core AI notification context is bounded to one invoked notification" "rg -Fq 'Use only this invoked notification summary' crates/goblins-os-core/src/ai.rs && rg -Fq 'do not claim to inspect notification history, other notifications, files, screenshots, secrets, hidden windows, or background app data' crates/goblins-os-core/src/ai.rs"
check "core AI notification context audits registered action only" "rg -Fq 'audit_ai_action(\"answer-notification\"' crates/goblins-os-core/src/ai.rs && rg -Fq 'notification_context_prompt_is_invoked_and_bounded_to_one_notification' crates/goblins-os-core/src/ai.rs"
check "core AI runtime uses Goblins-native route with legacy compatibility" "rg -Fq '/v1/ai/runtime/status' crates/goblins-os-core/src/main.rs && rg -Fq '/v1/ai/runtime' crates/goblins-os-core/src/main.rs && rg -Fq '.route(\"/v1/codex/resident\", post(ai_runtime))' crates/goblins-os-core/src/main.rs"
check "desktop clients use Goblins-native AI runtime route" "rg -Fq '/v1/ai/runtime/status' crates/goblins-os-settings/src/main.rs crates/goblins-os-shell/src/main.rs && rg -Fq '/v1/ai/runtime' crates/goblins-os-launcher/src/main.rs && ! rg -Fq '\"/v1/codex/resident/status\"' crates/goblins-os-settings/src/main.rs crates/goblins-os-shell/src/main.rs && ! rg -Fq '\"/v1/codex/resident\"' crates/goblins-os-launcher/src/main.rs"
check "installed self-test checks AI runtime primary route and compatibility alias" "rg -Fq '/v1/ai/runtime/status' os/bootc/run-selftest.sh && rg -Fq '/v1/codex/resident/status' os/bootc/run-selftest.sh && rg -Fq 'Goblins AI runtime IPC socket live' os/bootc/run-selftest.sh"
check "settings exposes notification AI readiness" "rg -q 'append_notifications_ai_context' crates/goblins-os-settings/src/main.rs && rg -q 'Goblins AI for notifications' crates/goblins-os-settings/src/main.rs && rg -q 'answer-notification' crates/goblins-os-settings/src/main.rs"
check "voice assistant uses Goblin wake word truthfully" "rg -q 'VOICE_WAKE_WORD: &str = \"Goblin\"' crates/goblins-os-core/src/voice.rs && rg -q '\"Hey Goblin\"' crates/goblins-os-core/src/voice.rs && rg -q 'wake_listening' crates/goblins-os-core/src/voice.rs && rg -q 'Background wake listening is not ready' crates/goblins-os-core/src/voice.rs crates/goblins-os-settings/src/main.rs && rg -Fq 'Say {voice_word}' crates/goblins-os-shell/src/main.rs && rg -Fq 'Listening for {wake_word}…' crates/goblins-os-shell/src/main.rs && rg -q 'Goblin wake word' crates/goblins-os-settings/src/main.rs && rg -q 'Ask Goblin' crates/goblins-os-launcher/src/main.rs crates/goblins-os-settings/src/main.rs crates/goblins-os-control-center/src/main.rs crates/goblins-os-ai/src/lib.rs os/gnome-shell-extensions/goblins-menubar@goblins.os/extension.js && rg -Fq 'scripts/Ask Goblin about this' crates/goblins-os-verify/src/main.rs && test -f 'os/nautilus/scripts/Ask Goblin about this' && ! rg -q -e 'Talk[[:space:]]to[[:space:]]Goblins[[:space:]]OS' -e 'Ask[[:space:]]Goblins' -e 'Write[[:space:]]with[[:space:]]Goblins' -e 'Voice[[:space:]]model' crates/goblins-os-shell/src/main.rs crates/goblins-os-launcher/src/main.rs crates/goblins-os-settings/src/main.rs crates/goblins-os-ai/src/lib.rs os/gnome-shell-extensions/goblins-menubar@goblins.os/extension.js"
check "settings notification AI copy preserves privacy boundary" "rg -q \"only that notification's title, body, app, and chosen action label\" crates/goblins-os-settings/src/main.rs"
check "launcher search uses native accessible icon" "rg -Fq 'gtk::Image::from_icon_name(\"system-search-symbolic\")' crates/goblins-os-launcher/src/main.rs && rg -q 'Search Goblins OS' crates/goblins-os-launcher/src/main.rs && ! rg -q 'telephone-recorder' crates/goblins-os-launcher/src/main.rs"
check "control center controls use accessible title-case copy" "rg -q 'Connection & Appearance' crates/goblins-os-control-center/src/main.rs && rg -q 'Goblins AI' crates/goblins-os-control-center/src/main.rs && rg -q 'Sound' crates/goblins-os-control-center/src/main.rs && rg -q 'Display brightness' crates/goblins-os-control-center/src/main.rs && rg -q 'set_accessible_label_description' crates/goblins-os-control-center/src/main.rs && rg -q 'Use on-device GPT-OSS' crates/goblins-os-control-center/src/main.rs && ! rg -q -e 'CONNECTION & APPEARANCE' -e 'BUILD ENGINE' -e 'GOBLINS AI' -e 'SOUND' -e 'DISPLAY' crates/goblins-os-control-center/src/main.rs"
check "shell dock and window manager controls expose accessible names and focus states" "rg -q 'accessible_name: .*Open' os/gnome-shell-extensions/goblins-dock@goblins.os/extension.js && rg -q 'accessible_name: .*Activate' os/gnome-shell-extensions/goblins-wm@goblins.os/extension.js && rg -q \"accessible_name: 'Move to previous space'\" os/gnome-shell-extensions/goblins-wm@goblins.os/extension.js && rg -q '.goblins-dock-item:focus' os/gnome-shell-extensions/goblins-dock@goblins.os/stylesheet.css && rg -q '.goblins-wm-window-card:focus' os/gnome-shell-extensions/goblins-wm@goblins.os/stylesheet.css && rg -q '.goblins-wm-hud-button:focus' os/gnome-shell-extensions/goblins-wm@goblins.os/stylesheet.css"
check "core AI exposes confirmed safe setting route" "rg -q '/v1/ai/safe-setting-change' crates/goblins-os-core/src/main.rs && rg -q 'change_safe_setting' crates/goblins-os-core/src/main.rs"
check "core AI exposes open settings panel route" "rg -q '/v1/ai/open-settings-panel' crates/goblins-os-core/src/main.rs && rg -q 'open_settings_panel' crates/goblins-os-core/src/main.rs"
check "core AI open settings panel route is deterministic and offline" "rg -q 'OpenSettingsPanelRequest' crates/goblins-os-core/src/ai.rs && rg -q 'SETTINGS_PANEL_CANDIDATES' crates/goblins-os-core/src/ai.rs && rg -q 'resolve_open_settings_panel' crates/goblins-os-core/src/ai.rs && rg -q 'settings_panel_router_maps_exact_and_natural_language_requests' crates/goblins-os-core/src/ai.rs"
check "core AI open settings panel route uses policy and audit" "rg -Fq 'policy_state_for_control(\"resident-assistant\")' crates/goblins-os-core/src/ai.rs && rg -Fq 'audit_open_settings_panel' crates/goblins-os-core/src/ai.rs && rg -Fq 'launch_argument: format!(\"--panel={}\"' crates/goblins-os-core/src/ai.rs"
check "installed self-test checks open settings panel route" "rg -q '/v1/ai/open-settings-panel' os/bootc/run-selftest.sh && rg -q 'open wifi settings' os/bootc/run-selftest.sh"
check "core AI exposes system status route" "rg -q '/v1/ai/system-status' crates/goblins-os-core/src/main.rs && rg -q 'ask_system_status' crates/goblins-os-core/src/main.rs"
check "core AI system status route uses OS-owned bounded snapshot" "rg -q 'SystemStatusContextRequest' crates/goblins-os-core/src/ai.rs && rg -q 'bounded_system_status_snapshot' crates/goblins-os-core/src/ai.rs && rg -q 'Use only this OS-owned status snapshot' crates/goblins-os-core/src/ai.rs && rg -q 'system_status_prompt_uses_only_os_owned_snapshot' crates/goblins-os-core/src/ai.rs"
check "core AI system status route uses policy and audit" "rg -q 'system_troubleshooting_policy' crates/goblins-os-core/src/ai.rs && rg -Fq 'audit_ai_action(action_id, Some(\"troubleshooting\")' crates/goblins-os-core/src/ai.rs && rg -q 'system_status_action_id' crates/goblins-os-core/src/ai.rs"
check "installed self-test checks system status route" "rg -q '/v1/ai/system-status' os/bootc/run-selftest.sh && rg -q 'Summarize current system state' os/bootc/run-selftest.sh"
check "core AI safe setting route requires policy and confirmation" "rg -Fq 'policy_state_for_control(\"settings-control\")' crates/goblins-os-core/src/ai.rs && rg -q 'StatusCode::PRECONDITION_REQUIRED' crates/goblins-os-core/src/ai.rs && rg -Fq 'audit_ai_action(\"change-safe-setting\"' crates/goblins-os-core/src/ai.rs"
check "core AI safe setting route has narrow allowlist" "rg -q 'appearance.color-scheme, accessibility.reduce-motion, or notifications.show-banners' crates/goblins-os-core/src/ai.rs && rg -q 'safe_setting_change_rejects_arbitrary_settings_and_wrong_values' crates/goblins-os-core/src/ai.rs"
check "core AI safe setting route reuses settings wrappers" "rg -q 'apply_ai_color_scheme' crates/goblins-os-core/src/appearance.rs && rg -q 'apply_ai_reduce_motion' crates/goblins-os-core/src/accessibility.rs && rg -q 'apply_ai_notification_banners' crates/goblins-os-core/src/notifications.rs"
check "installed self-test checks app-builder routes" "rg -q '/v1/apps/build-catalog' os/bootc/run-selftest.sh && rg -q '/v1/apps/builds' os/bootc/run-selftest.sh && rg -q 'GOBLINS_OS_APPS_DIR=/tmp/goblins-os-selftest-apps' os/bootc/run-selftest.sh"
check "bootc image includes gaming Vulkan tools and compositor substrate" "rg -q 'mesa-vulkan-drivers' os/bootc/Containerfile && rg -q 'vulkan-tools' os/bootc/Containerfile && rg -q 'gamescope' os/bootc/Containerfile && rg -q 'gamemode' os/bootc/Containerfile && rg -q 'mangohud' os/bootc/Containerfile"
check "bootc image includes gaming video audio and controller diagnostics" "rg -q 'mesa-va-drivers' os/bootc/Containerfile && rg -q 'libvdpau' os/bootc/Containerfile && rg -q 'vdpauinfo' os/bootc/Containerfile && rg -q 'pipewire-utils' os/bootc/Containerfile && rg -q 'pipewire-pulseaudio' os/bootc/Containerfile && rg -q 'pipewire-alsa' os/bootc/Containerfile && rg -q 'evtest' os/bootc/Containerfile && rg -q 'usbutils' os/bootc/Containerfile"
check "bootc image excludes Steam and steam-devices packages" "! rg -q '^[[:space:]]+steam([[:space:]\\\\]|$)|^[[:space:]]+steam-devices([[:space:]\\\\]|$)' os/bootc/Containerfile && rg -q '! rpm -q steam' os/bootc/Containerfile && rg -q '! rpm -q steam-devices' os/bootc/Containerfile"
check "settings Games panel explains Flatpak portals native architecture and user-initiated launchers" "rg -q 'Flatpak and Goblins OS desktop portals' crates/goblins-os-settings/src/main.rs && rg -q 'Release evidence is captured separately for aarch64 and x86_64 RPMs' crates/goblins-os-settings/src/main.rs && rg -q 'Availability is checked per architecture at install time' crates/goblins-os-settings/src/main.rs && rg -q 'does not download Proton runtimes without user action' crates/goblins-os-settings/src/main.rs"
check "settings and installer hide GNOME as user-facing prerequisite copy" "! rg -q 'GNOME desktop portals|GNOME accessibility keys|needs GNOME|requires GNOME' crates/goblins-os-settings/src/main.rs crates/goblins-os-installer/src/main.rs"
check "installed-root verifier checks gaming tools and Steam absence" "rg -q 'usr/bin/pw-cli' crates/goblins-os-verify/src/main.rs && rg -q 'usr/bin/evtest' crates/goblins-os-verify/src/main.rs && rg -q 'installed-steam-binary-absent' crates/goblins-os-verify/src/main.rs && rg -q 'installed-steam-devices-rules-absent' crates/goblins-os-verify/src/main.rs"
check "architecture contract records native non-Steam gaming policy" "rg -q 'non_steam_launcher_policy' os/release/architectures.toml && rg -q 'Steam and steam-devices are intentionally absent' os/release/architectures.toml && rg -q 'does not claim x86-only game runtimes work on Arm' os/release/architectures.toml"
check "runbook captures video controller and PipeWire gaming diagnostics" "rg -q 'vainfo' os/hardware-gate/runbook.md && rg -q 'evtest --query' os/hardware-gate/runbook.md && rg -q 'wpctl status' os/hardware-gate/runbook.md && rg -q 'pw-cli info 0' os/hardware-gate/runbook.md"
check "release evidence mode exists" "rg -q -- '--release-evidence' crates/goblins-os-verify/src/main.rs"
check "asset provenance covers Goblins primary marks" "rg -q 'os/brand/Goblins-black-mark.svg' os/release/asset-provenance.toml && rg -q 'os/brand/Goblins-white-mark.svg' os/release/asset-provenance.toml"
check "asset provenance covers OpenAI mark variants" "rg -q 'OpenAI-black-wordmark.png' os/release/asset-provenance.toml && rg -q 'OpenAI-white-wordmark.png' os/release/asset-provenance.toml && rg -q 'OpenAI-black-monoblossom.png' os/release/asset-provenance.toml && rg -q 'OpenAI-white-monoblossom.png' os/release/asset-provenance.toml"
check "asset provenance covers installer artwork" "rg -q 'os/brand/anaconda/sidebar-bg.svg' os/release/asset-provenance.toml && rg -q 'os/brand/anaconda/sidebar-logo.png' os/release/asset-provenance.toml"
check "asset provenance covers wallpapers icons and sounds" "rg -q 'os/brand/wallpaper/goblins-os-light.svg' os/release/asset-provenance.toml && rg -q 'os/brand/icons/' os/release/asset-provenance.toml && rg -q 'os/sounds/GoblinsOS/' os/release/asset-provenance.toml"
check "asset provenance excludes Apple assets and SF Symbols" "rg -q 'apple_assets = \"Not used' os/release/asset-provenance.toml && rg -q 'sf_symbols = \"Not used' os/release/asset-provenance.toml"
check "source manifest classifies GOAL.md as source" "rg -q 'GOAL.md' os/release/source-tree-manifest.toml"
check "source manifest classifies CI and ignore policy sources" "rg -q '\\.github/' os/release/source-tree-manifest.toml && rg -q '\\.gitignore' os/release/source-tree-manifest.toml && rg -q '\\.dockerignore' os/release/source-tree-manifest.toml"
check "source manifest classifies local agent state" "rg -q '\\.claude/' os/release/source-tree-manifest.toml"
check "source manifest classifies generated proofs and release artifacts" "rg -q 'artifacts/' os/release/source-tree-manifest.toml && rg -q 'os/signoff-notes.md' os/release/source-tree-manifest.toml && rg -q 'os/signoff-proofs/' os/release/source-tree-manifest.toml && rg -q 'os/screenshots/' os/release/source-tree-manifest.toml && rg -q 'os/iso/output\\*/' os/release/source-tree-manifest.toml"
check "source manifest classifies local build and shell-fragment outputs" "rg -q '\\.ci-target/' os/release/source-tree-manifest.toml && rg -q '\\.ci-target-amd64/' os/release/source-tree-manifest.toml && rg -q 'target/' os/release/source-tree-manifest.toml && rg -q 'libpod/' os/release/source-tree-manifest.toml && rg -q '\\.DS_Store' os/release/source-tree-manifest.toml && rg -q --fixed-strings '%sn *' os/release/source-tree-manifest.toml && rg -q --fixed-strings -- '-background' os/release/source-tree-manifest.toml"
check "acquisition delta records current source evidence" "rg -q 'rust_source_gates_available' os/release/acquisition-readiness-delta.toml && rg -q 'source_package_materialized' os/release/acquisition-readiness-delta.toml && rg -Fq 'root = \".\"' os/release/acquisition-readiness-delta.toml && rg -Fq 'source_tree_manifest = \"os/release/source-tree-manifest.toml\"' os/release/acquisition-readiness-delta.toml"
check "acquisition delta records native release blockers" "rg -q 'native_linux_release_runner_required' os/release/acquisition-readiness-delta.toml && rg -q 'shippable_release_iso_artifacts_incomplete' os/release/acquisition-readiness-delta.toml && rg -q 'display_backed_architecture_proofs_missing' os/release/acquisition-readiness-delta.toml && rg -q 'x86_64_rpm_sbom_present' os/release/acquisition-readiness-delta.toml && rg -q 'complete_signoff_rows_missing' os/release/acquisition-readiness-delta.toml"
check "acquisition delta has no stale local blocker labels or local user paths" "! rg -q 'rust_toolchain_missing|source_files_dataless|disk_space_low|x86_64_rpm_sbom_missing|/Users/' os/release/acquisition-readiness-delta.toml"
check "ignore files exclude local agent state" "rg -q '\\.claude/' .gitignore && rg -q '^\\.claude$' .dockerignore"
check "ignore files exclude generated proofs and release artifacts" "rg -q '^artifacts/' .gitignore && rg -q '^os/signoff-proofs/' .gitignore && rg -q '^os/screenshots/' .gitignore && rg -q '^os/iso/output\\*/' .gitignore && rg -q '^artifacts$' .dockerignore && rg -q '^os/signoff-proofs$' .dockerignore && rg -q '^os/screenshots$' .dockerignore && rg -q '^os/iso/output\\*$' .dockerignore"
check "ignore files exclude local build and shell-fragment outputs" "rg -q '^target$' .gitignore && rg -q '^target$' .dockerignore && rg -q '^\\.ci-target/' .gitignore && rg -q '^\\.ci-target$' .dockerignore && rg -q '^\\.ci-target-amd64/' .gitignore && rg -q '^\\.ci-target-amd64$' .dockerignore && rg -q '\\.DS_Store' .gitignore && rg -q '\\.DS_Store' .dockerignore && rg -q --fixed-strings '%sn *' .gitignore && rg -q --fixed-strings '%sn *' .dockerignore && rg -q --fixed-strings -- '-background' .gitignore && rg -q --fixed-strings -- '-background' .dockerignore"
check "trademark posture keeps Goblins OS primary" "rg -q 'Goblins OS remains the leading product identity' os/release/trademark-posture.toml"
check "trademark posture scopes OpenAI to provider integration" "rg -q 'Provider/integration reference only' os/release/trademark-posture.toml"
check "trademark posture scopes Fedora and Red Hat to base references" "rg -q 'Base-platform reference only' os/release/trademark-posture.toml"
check "trademark posture scopes GNOME marks to factual package references" "rg -q 'Runtime, toolkit, and package reference only' os/release/trademark-posture.toml"
check "trademark posture blocks Apple assets and copied trade dress" "rg -q 'Do not ship Apple fonts, logos, symbols, wallpapers, screenshots, app screens, product images, SF Symbols, or copied Apple trade dress' os/release/trademark-posture.toml"
check "third-party notices cover GNOME package SBOM path" "rg -q 'GNOME Shell, GTK, libadwaita/Adwaita assets' os/release/third-party-notices.toml"
check "third-party notices document release evidence generator" "rg -q -- '--release-evidence os/signoff-proofs/sbom/<arch>/' os/release/third-party-notices.toml"
check "third-party notices require cargo package TSV" "rg -q 'cargo-lock-packages.tsv' os/release/third-party-notices.toml"
check "third-party notices require RPM command file" "rg -q 'rpm-packages.command' os/release/third-party-notices.toml"
check "SHIP documents SBOM evidence command" "rg -q --fixed-strings -- '--release-evidence \"os/signoff-proofs/sbom/' \"$SHIP_DECL\""
check "shipping status rejects local-only installer payload refs" "rg -q 'installer payload tracks a local-only Docker/test registry' os/hardware-gate/verify-shipping-status.sh"
check "shipping status reports ignored legacy screenshot roots" "rg -q 'Legacy/non-shipping screenshot roots ignored by architecture proof gate' os/hardware-gate/verify-shipping-status.sh"
check "runbook rejects legacy non-arch screenshot roots" "rg -q 'Legacy/non-shipping screenshot roots' os/hardware-gate/runbook.md && rg -q '<arch>/<YYYY-MM-DD>' os/hardware-gate/runbook.md"

for shot in "${REQ_SCREENSHOTS[@]}"; do
  check "runbook includes required screenshot $shot" "rg -q --fixed-strings '$shot' \"$RUNBOOK\""
done

check "signoff notes contains runtime engine fields" "rg -q 'Runtime engine run:|Motion/interactions checked' \"$SIGNOFF\""
check "signoff notes contains gaming proof field" "rg -q 'Gaming readiness checked' \"$SIGNOFF\""
check "signoff notes contains install storage proof field" "rg -q 'Install storage/bootloader/dual-boot checked' \"$SIGNOFF\""
check "signoff notes contains release evidence proof field" "rg -q 'Release evidence/SBOM checked' \"$SIGNOFF\""
check "close-signoff writes fail-closed completion status" "rg -q 'PROJECT_COMPLETION_STATUS=\"incomplete\"' os/hardware-gate/close-signoff.sh && rg -q 'Current project completion status: \\$\\{PROJECT_COMPLETION_STATUS\\}' os/hardware-gate/close-signoff.sh"
check "close-signoff requires runtime and built-artifact proof before completion" "rg -q 'RUNTIME_ENGINE_MODE' os/hardware-gate/close-signoff.sh && rg -q 'BUILT_ARTIFACT_PATH_URL' os/hardware-gate/close-signoff.sh && rg -q '\\[ -n \"[$]RUNTIME_ENGINE_MODE\" \\]' os/hardware-gate/close-signoff.sh && rg -q '\\[ -n \"[$]BUILT_ARTIFACT_PATH_URL\" \\]' os/hardware-gate/close-signoff.sh"
check "close-signoff rejects placeholder runtime proof" "rg -q 'proof_field_is_real' os/hardware-gate/close-signoff.sh && rg -q 'validate_runtime_proof_fields' os/hardware-gate/close-signoff.sh && rg -q 'placeholders are not accepted' os/hardware-gate/close-signoff.sh"
check "close-signoff requires real built artifact reference" "rg -q 'built_artifact_reference_is_real' os/hardware-gate/close-signoff.sh && rg -q 'https URL, localhost URL, or existing local path' os/hardware-gate/close-signoff.sh"
check "close-signoff requires architecture screenshot directory" "rg -q 'screenshot_dir_matches_arch' os/hardware-gate/close-signoff.sh && rg -q 'os/screenshots/hardware-gate/[$]ARCH/<date>' os/hardware-gate/close-signoff.sh"
check "close-signoff workflow checks fail fast" "rg -q 'require_fixed' os/hardware-gate/close-signoff.sh && rg -q 'per-architecture image build target missing in workflow' os/hardware-gate/close-signoff.sh"
check "close-signoff uses Docker for assisted signoff testing" "rg -q 'Docker is required for assisted signoff testing' os/hardware-gate/close-signoff.sh && rg -q 'docker image inspect' os/hardware-gate/close-signoff.sh && rg -q 'docker run --rm' os/hardware-gate/close-signoff.sh && rg -q 'DOCKER_BUILDKIT=1 docker build' os/hardware-gate/close-signoff.sh && ! rg -q 'podman' os/hardware-gate/close-signoff.sh"
check "close-signoff expects per-architecture image tag" "rg -q 'goblins-os:\\$\\{\\{ matrix.arch \\}\\}' os/hardware-gate/close-signoff.sh"
check "close-signoff uses exact architecture ISO path" "rg -q 'expected_iso=\"os/iso/output/[$]ARCH/bootiso/goblins-os-[$]ARCH.iso\"' os/hardware-gate/close-signoff.sh"
check "shipping status bounds signoff rows at the next markdown heading" "rg -q 'signoff_block_from_line' os/hardware-gate/verify-shipping-status.sh && rg -q 'NR < start' os/hardware-gate/verify-shipping-status.sh && rg -Fq '/^## / { exit }' os/hardware-gate/verify-shipping-status.sh && ! rg -Fq \"start + \$((60 + 60))\" os/hardware-gate/verify-shipping-status.sh"

for arch in "${ARCHES[@]}"; do
  ISO_PATH="os/iso/output/$arch/bootiso/goblins-os-$arch.iso"
  SHA_PATH="$ISO_PATH.sha256"
  MANIFEST_PATH="os/iso/output/$arch/manifest-goblins-os-$arch.json"
  BIB_MANIFEST_PATH="os/iso/output/$arch/manifest-anaconda-iso.json"
  SBOM_DIR="os/signoff-proofs/sbom/$arch"
  SBOM_MANIFEST="$SBOM_DIR/release-evidence-manifest.json"
  CARGO_TSV="$SBOM_DIR/cargo-lock-packages.tsv"
  RPM_TSV="$SBOM_DIR/rpm-packages.tsv"
  ARCH_MISSING=()

  check_file "$arch ISO artifact exists" "$ISO_PATH" || ARCH_MISSING+=("ISO")
  check_file "$arch ISO SHA256 exists" "$SHA_PATH" || ARCH_MISSING+=("SHA256")
  if [ -f "$ISO_PATH" ] && [ -f "$SHA_PATH" ]; then
    check_sha256_file "$arch ISO SHA256 verifies" "$SHA_PATH" || ARCH_MISSING+=("SHA256 verification")
  fi
  check_file "$arch ISO manifest exists" "$MANIFEST_PATH" || ARCH_MISSING+=("ISO manifest")
  check_file_contains "$arch ISO manifest records architecture" "$MANIFEST_PATH" "\"architecture\": \"$arch\"" || ARCH_MISSING+=("ISO manifest architecture")
  check_file_contains "$arch ISO manifest records ISO name" "$MANIFEST_PATH" "\"iso\": \"bootiso/goblins-os-$arch.iso\"" || ARCH_MISSING+=("ISO manifest artifact")
  check_file_contains "$arch ISO manifest records SHA file" "$MANIFEST_PATH" "\"sha256_file\": \"bootiso/goblins-os-$arch.iso.sha256\"" || ARCH_MISSING+=("ISO manifest SHA")
  check_file_contains "$arch ISO manifest records builder source image" "$MANIFEST_PATH" "\"builder_source_image\":" || ARCH_MISSING+=("ISO manifest builder source")
  check_file_contains "$arch ISO manifest records installer payload source kind" "$MANIFEST_PATH" "\"installer_payload_source_kind\":" || ARCH_MISSING+=("ISO manifest payload source kind")
  check_file_contains "$arch ISO manifest records nonlocal installer payload source" "$MANIFEST_PATH" "\"installer_payload_source_local_only\": false" || ARCH_MISSING+=("ISO manifest nonlocal payload source")
  check_file_contains "$arch ISO manifest records shippable release mode" "$MANIFEST_PATH" "\"shippable_release\": true" || ARCH_MISSING+=("ISO manifest shippable release")
  check_bib_manifest_payload_ref "$arch BIB manifest uses shippable installer payload ref" "$BIB_MANIFEST_PATH" || ARCH_MISSING+=("shippable installer payload ref")
  check_file "$arch release evidence manifest exists" "$SBOM_MANIFEST" || ARCH_MISSING+=("release evidence manifest")
  check_file_contains "$arch release evidence manifest records architecture" "$SBOM_MANIFEST" "\"architecture\": \"$arch\"" || ARCH_MISSING+=("release evidence architecture")
  check_file_contains "$arch release evidence manifest records asset provenance" "$SBOM_MANIFEST" "\"asset_provenance\": \"os/release/asset-provenance.toml\"" || ARCH_MISSING+=("release evidence asset provenance")
  check_file_contains "$arch release evidence manifest records third-party notices" "$SBOM_MANIFEST" "\"third_party_notices\": \"os/release/third-party-notices.toml\"" || ARCH_MISSING+=("release evidence third-party notices")
  check_file_contains "$arch release evidence manifest records trademark posture" "$SBOM_MANIFEST" "\"trademark_posture\": \"os/release/trademark-posture.toml\"" || ARCH_MISSING+=("release evidence trademark posture")
  check_file_contains "$arch release evidence manifest records source tree manifest" "$SBOM_MANIFEST" "\"source_tree_manifest\": \"os/release/source-tree-manifest.toml\"" || ARCH_MISSING+=("release evidence source tree manifest")
  check_file "$arch Cargo SBOM package TSV exists" "$CARGO_TSV" || ARCH_MISSING+=("Cargo SBOM TSV")
  check_file "$arch RPM SBOM package TSV exists" "$RPM_TSV" || ARCH_MISSING+=("RPM SBOM TSV")
  if [ -f "$RPM_TSV" ]; then
    if rpm_sbom_arch_matches "$RPM_TSV" "$arch"; then
      echo "[PASS] $arch RPM SBOM package architectures match $arch or noarch"
    else
      echo "[FAIL] $arch RPM SBOM package architectures must match $arch or noarch"
      FAIL_COUNT=$((FAIL_COUNT + 1))
      ARCH_MISSING+=("RPM SBOM architecture")
    fi
  fi

  if [ -d "$SCREENSHOT_ROOT/$arch" ]; then
    LATEST_ARCH_RUN=""
    while IFS= read -r candidate; do
      if screenshot_run_is_complete "$candidate"; then
        LATEST_ARCH_RUN="$candidate"
        break
      fi
    done < <(find "$SCREENSHOT_ROOT/$arch" -mindepth 1 -maxdepth 1 -type d | sort -r)
    if [ -n "$LATEST_ARCH_RUN" ]; then
      echo "[PASS] $arch has complete hardware-gate screenshots: $LATEST_ARCH_RUN"
    else
      echo "[FAIL] $arch has no complete hardware-gate screenshot run under $SCREENSHOT_ROOT/$arch"
      print_latest_incomplete_screenshot_run "$SCREENSHOT_ROOT/$arch" "$arch"
      FAIL_COUNT=$((FAIL_COUNT + 1))
      ARCH_MISSING+=("complete screenshot run")
    fi
  else
    echo "[FAIL] $arch screenshot root missing: $SCREENSHOT_ROOT/$arch"
    print_latest_incomplete_screenshot_run "$SCREENSHOT_ROOT/$arch" "$arch"
    FAIL_COUNT=$((FAIL_COUNT + 1))
    ARCH_MISSING+=("screenshot root")
  fi

  if signoff_run_for_arch_is_complete "$arch"; then
    echo "[PASS] $arch has complete signoff row"
  else
    echo "[FAIL] $arch has no complete signoff row with runner, ISO, verify/self-test, SBOM, runtime, gaming, and install-storage proof"
    FAIL_COUNT=$((FAIL_COUNT + 1))
    ARCH_MISSING+=("complete signoff row")
  fi

  if [ "${#ARCH_MISSING[@]}" -eq 0 ]; then
    echo "[PASS] $arch architecture track complete"
  else
    echo "[FAIL] $arch architecture track missing: ${ARCH_MISSING[*]}"
    FAIL_COUNT=$((FAIL_COUNT + 1))
  fi
done

print_legacy_screenshot_roots

if [ -f "$SIGNOFF" ]; then
  RUN_LINE="$(rg -n "^## Manual Gate Run:" "$SIGNOFF" | tail -n1 | cut -d: -f1 || true)"
  if [ -n "$RUN_LINE" ]; then
    LATEST_RUN_BLOCK="$(awk -v start="$RUN_LINE" 'NR < start { next } NR == start { print; next } /^## / { exit } { print }' "$SIGNOFF")"
    if echo "$LATEST_RUN_BLOCK" | rg -q "^- Runner: .+"; then
      echo "[PASS] Latest signoff run has Runner"
    else
      echo "[FAIL] Latest signoff run missing Runner"
      FAIL_COUNT=$((FAIL_COUNT + 1))
    fi
    if echo "$LATEST_RUN_BLOCK" | rg -q "^- Architecture: (aarch64|x86_64)"; then
      echo "[PASS] Latest signoff run has architecture"
    else
      echo "[FAIL] Latest signoff run missing architecture"
      FAIL_COUNT=$((FAIL_COUNT + 1))
    fi
    if echo "$LATEST_RUN_BLOCK" | rg -q "^- Verify result \(blocked=0\): pass"; then
      echo "[PASS] Latest signoff run recorded blocked=0 pass"
    else
      echo "[FAIL] Latest signoff run missing/does not record blocked=0 pass"
      FAIL_COUNT=$((FAIL_COUNT + 1))
    fi
    if echo "$LATEST_RUN_BLOCK" | rg -q "^- Self-test result: pass"; then
      echo "[PASS] Latest signoff run recorded self-test pass"
    else
      echo "[FAIL] Latest signoff run missing/does not record self-test pass"
      FAIL_COUNT=$((FAIL_COUNT + 1))
    fi
    if signoff_block_has_real_field "$LATEST_RUN_BLOCK" "^  - mode: .+"; then
      echo "[PASS] Latest signoff run records real runtime engine mode"
    else
      echo "[FAIL] Latest signoff run missing real runtime engine mode"
      FAIL_COUNT=$((FAIL_COUNT + 1))
    fi
    if signoff_block_has_real_field "$LATEST_RUN_BLOCK" "^  - engine source: .+"; then
      echo "[PASS] Latest signoff run records real runtime engine source"
    else
      echo "[FAIL] Latest signoff run missing real runtime engine source"
      FAIL_COUNT=$((FAIL_COUNT + 1))
    fi
    if signoff_block_has_real_field "$LATEST_RUN_BLOCK" "^  - built artifact path/URL: .+"; then
      echo "[PASS] Latest signoff run has real built artifact proof"
    else
      echo "[FAIL] Latest signoff run missing real built artifact proof"
      FAIL_COUNT=$((FAIL_COUNT + 1))
    fi
    if echo "$LATEST_RUN_BLOCK" | rg -q "^- Motion/interactions checked: yes"; then
      echo "[PASS] Latest signoff run records motion/interaction proof"
    else
      echo "[FAIL] Latest signoff run missing motion/interaction proof"
      FAIL_COUNT=$((FAIL_COUNT + 1))
    fi
    if echo "$LATEST_RUN_BLOCK" | rg -q "^- Gaming readiness checked: yes"; then
      echo "[PASS] Latest signoff run records gaming readiness proof"
    else
      echo "[FAIL] Latest signoff run missing gaming readiness proof"
      FAIL_COUNT=$((FAIL_COUNT + 1))
    fi
    if echo "$LATEST_RUN_BLOCK" | rg -q "^- Install storage/bootloader/dual-boot checked: yes"; then
      echo "[PASS] Latest signoff run records install storage/dual-boot proof"
    else
      echo "[FAIL] Latest signoff run missing install storage/dual-boot proof"
      FAIL_COUNT=$((FAIL_COUNT + 1))
    fi
    if echo "$LATEST_RUN_BLOCK" | rg -q "^- Release evidence/SBOM checked: yes"; then
      echo "[PASS] Latest signoff run records release evidence/SBOM proof"
    else
      echo "[FAIL] Latest signoff run missing release evidence/SBOM proof"
      FAIL_COUNT=$((FAIL_COUNT + 1))
    fi
    if echo "$LATEST_RUN_BLOCK" | rg -qi "Screenshot dir: no fresh|stale screenshot|stale for this ISO|No fresh .*screenshots"; then
      echo "[FAIL] Latest signoff run records stale or missing current screenshot proof"
      FAIL_COUNT=$((FAIL_COUNT + 1))
    else
      echo "[PASS] Latest signoff run does not record stale/missing current screenshot proof"
    fi
    if signoff_block_required_proof_is_complete "$LATEST_RUN_BLOCK"; then
      if echo "$LATEST_RUN_BLOCK" | rg -q "^- Current project completion status: complete$"; then
        echo "[PASS] Latest signoff run completion status matches complete proof"
      else
        echo "[FAIL] Latest signoff run has complete proof but does not declare completion"
        FAIL_COUNT=$((FAIL_COUNT + 1))
      fi
    elif echo "$LATEST_RUN_BLOCK" | rg -q "^- Current project completion status: complete"; then
      echo "[FAIL] Latest signoff run declares completion before required proof is present"
      FAIL_COUNT=$((FAIL_COUNT + 1))
    else
      echo "[PASS] Latest signoff run does not claim completion with incomplete proof"
    fi
  else
    echo "[FAIL] No Manual Gate Run sections found in signoff notes"
    FAIL_COUNT=$((FAIL_COUNT + 1))
  fi
else
  echo "[FAIL] Signoff notes file missing"
  FAIL_COUNT=$((FAIL_COUNT + 1))
fi

if [ -n "$SCREENSHOT_RUN_DIR" ]; then
  if [ -d "$SCREENSHOT_RUN_DIR" ]; then
    LATEST_RUN="$SCREENSHOT_RUN_DIR"
    echo "Verifying provided screenshot run: $LATEST_RUN"
    if ! print_screenshot_run_checks "$LATEST_RUN"; then
      fail_check "Hardware-gate screenshot proof is incomplete"
    fi
  else
    fail_check "Provided SCREENSHOT_RUN_DIR not found: $SCREENSHOT_RUN_DIR"
  fi
else
  if [ -d "$SCREENSHOT_ROOT" ]; then
    LATEST_RUN=""
    while IFS= read -r candidate; do
      if screenshot_run_is_complete "$candidate"; then
        LATEST_RUN="$candidate"
        break
      fi
    done < <(find "$SCREENSHOT_ROOT" -mindepth 2 -maxdepth 2 -type d | sort -r)

    if [ -n "$LATEST_RUN" ]; then
      echo "Latest complete hardware-gate screenshot run: $LATEST_RUN"
      if ! print_screenshot_run_checks "$LATEST_RUN"; then
        fail_check "Hardware-gate screenshot proof is incomplete"
      fi
    else
      fail_check "No complete hardware-gate screenshot run found under $SCREENSHOT_ROOT"
      for arch in "${ARCHES[@]}"; do
        print_latest_incomplete_screenshot_run "$SCREENSHOT_ROOT/$arch" "$arch"
      done
    fi
  else
    fail_check "Screenshot root missing: $SCREENSHOT_ROOT"
    print_latest_incomplete_screenshot_run "$SCREENSHOT_ROOT" "hardware-gate"
  fi
fi

echo
for arch in "${ARCHES[@]}"; do
  print_arch_next_steps "$arch"
done

echo
echo "Run ./os/hardware-gate/close-signoff.sh on Linux to generate a full verified status row with verify/self-test results."
echo "Use SCREENSHOT_RUN_DIR or SCREENSHOT_DIR to validate screenshot completeness."

if [ "${FAIL_COUNT:-0}" -ne 0 ]; then
  echo "Shipping status gate: FAIL"
  exit 1
fi

echo "Shipping status gate: PASS"
exit 0
