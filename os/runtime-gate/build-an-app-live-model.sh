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
B=http://127.0.0.1:8787
INTENT="${INTENT:-A calm pomodoro focus timer that logs each finished session and shows a weekly streak.}"

echo "==> engine: $GOBLINS_OS_LOCAL_MODEL @ $GOBLINS_OS_LOCAL_RUNTIME_URL"
"$CORE" >/work/core.log 2>&1 & CORE_PID=$!
n=0; until curl -sf "$B/health" >/dev/null 2>&1 || [ $n -ge 30 ]; do n=$((n+1)); sleep 1; done
echo "==> core /health: $(curl -s "$B/health")"
echo "==> codex installed (expect false): $(curl -s "$B/v1/codex/status" | grep -o '"installed":[a-z]*')"
echo "==> app-builder before grant: $(curl -s "$B/v1/apps/build-catalog" | grep -o '"builder":"[a-z-]*"')"
curl -s -X POST "$B/v1/policy/permissions/grant" -H 'content-type: application/json' \
  -d '{"control_id":"app-builder","acknowledgement":"GRANT GOBLINS OS PERMISSION app-builder FOR consumer"}' \
  | grep -o '"ok":[a-z]*' | sed 's/^/==> grant /'
echo "==> building app from intent (live inference): $INTENT"
curl -s -X POST "$B/v1/apps/builds" -H 'content-type: application/json' \
  -d "{\"intent\":\"$INTENT\"}" > /work/build-response.json
grep -o '"ok":[a-z]*\|"text":"[^"]*"' /work/build-response.json | sed 's/^/==> /'
echo "==> app store count: $(curl -s "$B/v1/apps" | grep -o '"count":[0-9]*')"
echo "==> persisted artifact:"; ls -la "$GOBLINS_OS_APPS_DIR"
kill $CORE_PID 2>/dev/null
echo "==> done"
