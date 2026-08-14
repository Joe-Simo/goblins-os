#!/usr/bin/env bash
# Offline, read-only adapter used to let the existing exact-candidate hydrator
# consume an artifact that the workflow fetched before dropping credentials.
set -euo pipefail

required_env=(
  GOBLINS_OS_PROMOTION_CANDIDATE_RUN_ID
  GOBLINS_OS_PROMOTION_CANDIDATE_RUN_ATTEMPT
  GOBLINS_OS_PROMOTION_CANDIDATE_COMMIT
  GOBLINS_OS_PROMOTION_CANDIDATE_RUN_JSON
  GOBLINS_OS_PROMOTION_CANDIDATE_ARTIFACTS_JSON
  GOBLINS_OS_PROMOTION_CANDIDATE_ARCHIVE
  GOBLINS_OS_PROMOTION_CANDIDATE_ARTIFACT_DIGEST
  GOBLINS_OS_PROMOTION_HELPER
)
for name in "${required_env[@]}"; do
  if [ -z "${!name:-}" ]; then
    echo "error: offline GitHub adapter is missing $name" >&2
    exit 2
  fi
done

run_endpoint="repos/Joe-Simo/goblins-os/actions/runs/${GOBLINS_OS_PROMOTION_CANDIDATE_RUN_ID}/attempts/${GOBLINS_OS_PROMOTION_CANDIDATE_RUN_ATTEMPT}"
artifacts_endpoint="repos/Joe-Simo/goblins-os/actions/runs/${GOBLINS_OS_PROMOTION_CANDIDATE_RUN_ID}/artifacts?per_page=100"
artifact_name="goblins-os-candidate-${GOBLINS_OS_PROMOTION_CANDIDATE_COMMIT}-aarch64"

case "${1:-}" in
  api)
    [ "$#" -eq 2 ] || {
      echo "error: offline GitHub adapter rejected unexpected api arguments" >&2
      exit 2
    }
    case "$2" in
      "$run_endpoint") exec /bin/cat -- "$GOBLINS_OS_PROMOTION_CANDIDATE_RUN_JSON" ;;
      "$artifacts_endpoint") exec /bin/cat -- "$GOBLINS_OS_PROMOTION_CANDIDATE_ARTIFACTS_JSON" ;;
      *) echo "error: offline GitHub adapter rejected endpoint: $2" >&2; exit 2 ;;
    esac
    ;;
  run)
    if [ "$#" -ne 9 ] \
      || [ "${2:-}" != "download" ] \
      || [ "${3:-}" != "$GOBLINS_OS_PROMOTION_CANDIDATE_RUN_ID" ] \
      || [ "${4:-}" != "--repo" ] \
      || [ "${5:-}" != "Joe-Simo/goblins-os" ] \
      || [ "${6:-}" != "--name" ] \
      || [ "${7:-}" != "$artifact_name" ] \
      || [ "${8:-}" != "--dir" ] \
      || [ -z "${9:-}" ]; then
      echo "error: offline GitHub adapter rejected unexpected run download arguments" >&2
      exit 2
    fi
    exec /usr/bin/env python3 "$GOBLINS_OS_PROMOTION_HELPER" extract-candidate \
      --archive "$GOBLINS_OS_PROMOTION_CANDIDATE_ARCHIVE" \
      --expected-digest "$GOBLINS_OS_PROMOTION_CANDIDATE_ARTIFACT_DIGEST" \
      --destination "$9"
    ;;
  *)
    echo "error: offline GitHub adapter permits only exact api reads and one artifact download" >&2
    exit 2
    ;;
esac
