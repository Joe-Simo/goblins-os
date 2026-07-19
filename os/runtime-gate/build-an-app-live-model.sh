#!/bin/bash
# Goblins OS — runtime-model gate: build an app from intent with a LIVE on-device
# open-weight model, end to end, headless. This exercises the REAL shipped path:
#   create_app_build (POST /v1/apps/builds) -> resident_generate -> resident_relay
#   -> Ollama /api/generate (the on-device GPT-OSS engine) -> persisted BuiltApp.
# No mocks: a real daemon, a real local model doing real inference, a real artifact.
#
# WHY A STAND-IN MODEL IN-SANDBOX: gpt-oss-20b/120b are the shipped defaults but need
# ~16GB / ~80GB RAM; the CI/dev VM here has 8GB, so we verify the (model-agnostic)
# path with a small real open-weight model served over the same Ollama protocol. On
# real hardware, set GOBLINS_OS_LOCAL_MODEL=gpt-oss:20b (or :120b) — same code path.
#
# RUN (Docker on macOS/Linux), from the repo root:
#   docker rm -f gos-ollama 2>/dev/null
#   docker run -d --name gos-ollama -v /tmp/ollama:/root/.ollama ollama/ollama
#   docker exec gos-ollama ollama pull "${MODEL:-llama3.2:3b}"
#   docker run --rm --network=container:gos-ollama \
#     -e GOBLINS_OS_LOCAL_RUNTIME_URL=http://127.0.0.1:11434 \
#     -e GOBLINS_OS_LOCAL_MODEL="${MODEL:-llama3.2:3b}" \
#     -e GOBLINS_OS_APPS_DIR=/work/apps -e GOBLINS_OS_POLICY_STATE=/work/policy \
#     -e GOBLINS_OS_RESIDENT_STATE=/work/resident -v /tmp/gos-e2e:/work \
#     goblins-os:latest bash /work/build-an-app-live-model.sh
#   (copy this script to /tmp/gos-e2e first, or bind the repo and point at os/runtime-gate/)
set -uo pipefail
CORE=/usr/libexec/goblins-os/goblins-os-core
CORE_PROOF_SOCKET=/run/goblins-os-core/release-proof/control.sock
B=http://localhost
INTENT="${INTENT:-A calm pomodoro focus timer that logs each finished session and shows a weekly streak.}"
BUILD_RESPONSE_PATH="${BUILD_RESPONSE_PATH:-/work/build-response.json}"
PROOF_PATH="${PROOF_PATH:-/work/runtime-build-proof.json}"

core_proof_curl() {
  curl --unix-socket "$CORE_PROOF_SOCKET" "$@"
}

write_runtime_build_proof() {
  local http_status="$1"

  python3 - "$BUILD_RESPONSE_PATH" "$PROOF_PATH" "$INTENT" "$http_status" <<'PY'
import json
import os
import sys

response_path, proof_path, intent, http_status = sys.argv[1:5]
data = {}
error = ""

try:
    with open(response_path, encoding="utf-8") as response_file:
        data = json.load(response_file)
except Exception as exc:
    error = str(exc)

app = data.get("app") if isinstance(data, dict) else None
if not isinstance(app, dict):
    app = {}

built_artifact_id = str(app.get("id") or "")
built_artifact_name = str(app.get("name") or "")
engine_source = str(app.get("source") or "")
app_intent = str(app.get("intent") or intent)
response_bytes = os.path.getsize(response_path) if os.path.exists(response_path) else 0

passed = (
    http_status == "200"
    and data.get("ok") is True
    and bool(built_artifact_id)
    and bool(built_artifact_name)
    and bool(engine_source)
)

proof = {
    "status": "pass" if passed else "fail",
    "route": "/v1/apps/builds",
    "intent": app_intent,
    "engine_mode": "local-model",
    "engine_source": engine_source or "missing",
    "built_artifact_id": built_artifact_id or "missing",
    "built_artifact_name": built_artifact_name or "missing",
    "response_bytes": str(response_bytes),
    "http_status": str(http_status),
}

if not passed:
    proof["stage"] = "response"
    if error:
        proof["error"] = error[-240:]
    if isinstance(data, dict):
        text = str(data.get("text") or "")
        if text:
            proof["response_text"] = text[:240]

os.makedirs(os.path.dirname(proof_path) or ".", exist_ok=True)
with open(proof_path, "w", encoding="utf-8") as proof_file:
    json.dump(proof, proof_file, indent=2)
    proof_file.write("\n")

manifest_path = os.path.join(os.path.dirname(proof_path) or ".", "proof-manifest.json")
if passed and os.path.basename(proof_path) == "runtime-build-proof.json" and os.path.exists(manifest_path):
    try:
        with open(manifest_path, encoding="utf-8") as manifest_file:
            manifest = json.load(manifest_file)
        if isinstance(manifest, dict):
            manifest["runtime_build_proof"] = "runtime-build-proof.json"
            with open(manifest_path, "w", encoding="utf-8") as manifest_file:
                json.dump(manifest, manifest_file, indent=2)
                manifest_file.write("\n")
    except Exception:
        pass
PY
}

grant_app_builder_permission() {
  local status_path="${1:-/tmp/goblins-os-policy-status.json}"
  local grant_path="${2:-/tmp/goblins-os-app-builder-grant.json}"
  local profile acknowledgement payload grant_http grant_ok

  core_proof_curl -s -o "$status_path" "$B/v1/policy/status" || true
  profile="$(python3 - "$status_path" <<'PY'
import json
import sys

try:
    print(json.load(open(sys.argv[1], encoding="utf-8")).get("profile", ""))
except Exception:
    print("")
PY
)"
  if [ -z "$profile" ]; then
    printf '{"ok":false,"text":"Could not read active policy profile from /v1/policy/status."}\n' > "$grant_path"
    echo "==> app-builder grant: missing policy profile"
    return 1
  fi

  acknowledgement="GRANT GOBLINS OS PERMISSION app-builder FOR $profile"
  payload="$(python3 - "$acknowledgement" <<'PY'
import json
import sys

print(json.dumps({"control_id": "app-builder", "acknowledgement": sys.argv[1]}))
PY
)"
  grant_http="$(core_proof_curl -s -o "$grant_path" -w '%{http_code}' -X POST "$B/v1/policy/permissions/grant" -H 'content-type: application/json' -d "$payload" || true)"
  grant_ok="$(python3 - "$grant_path" <<'PY'
import json
import sys

try:
    print("true" if json.load(open(sys.argv[1], encoding="utf-8")).get("ok") is True else "false")
except Exception:
    print("false")
PY
)"
  echo "==> app-builder grant: http=$grant_http ok=$grant_ok profile=$profile"
  [ "$grant_http" = "200" ] && [ "$grant_ok" = "true" ]
}

echo "==> engine: $GOBLINS_OS_LOCAL_MODEL @ $GOBLINS_OS_LOCAL_RUNTIME_URL"
systemd-tmpfiles --create /usr/lib/tmpfiles.d/goblins-os-core.conf
install -d -m 0750 -o goblins-os -g goblins-os \
  "$GOBLINS_OS_APPS_DIR" "$GOBLINS_OS_POLICY_STATE" "$GOBLINS_OS_RESIDENT_STATE"
setpriv --reuid=goblins-os --regid=goblins-os --init-groups -- \
  "$CORE" >/work/core.log 2>&1 & CORE_PID=$!
n=0; until core_proof_curl -sf "$B/health" >/dev/null 2>&1 || [ $n -ge 30 ]; do n=$((n+1)); sleep 1; done
echo "==> core /health: $(core_proof_curl -s "$B/health")"
echo "==> codex installed (expect false): $(core_proof_curl -s "$B/v1/codex/status" | grep -o '"installed":[a-z]*')"
echo "==> app-builder before grant: $(core_proof_curl -s "$B/v1/apps/build-catalog" | grep -o '"builder":"[a-z-]*"')"
grant_app_builder_permission /tmp/goblins-os-policy-status.json /tmp/goblins-os-app-builder-grant.json || true
echo "==> building app from intent (live inference): $INTENT"
build_payload="$(INTENT="$INTENT" python3 - <<'PY'
import json
import os

print(json.dumps({"intent": os.environ["INTENT"]}))
PY
)"
build_http="$(core_proof_curl -s -o "$BUILD_RESPONSE_PATH" -w '%{http_code}' -X POST "$B/v1/apps/builds" -H 'content-type: application/json' \
  -d "$build_payload")"
write_runtime_build_proof "$build_http"
grep -o '"ok":[a-z]*\|"text":"[^"]*"' "$BUILD_RESPONSE_PATH" | sed 's/^/==> /'
echo "==> runtime build proof: $PROOF_PATH"
echo "==> built app count: $(core_proof_curl -s "$B/v1/apps" | grep -o '"count":[0-9]*')"
echo "==> persisted artifact:"; ls -la "$GOBLINS_OS_APPS_DIR"
kill $CORE_PID 2>/dev/null
echo "==> done"
