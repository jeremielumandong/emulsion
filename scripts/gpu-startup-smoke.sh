#!/usr/bin/env bash
# Linux/Mesa acceptance. Build first; never prewarm a shader outside production.
# Keep every log and cache, including the first failure, under the printed path.
set -euo pipefail
umask 077

ROOT_DIR="$(cd "$(dirname "${BASH_SOURCE[0]}")/.." && pwd)"
cd "$ROOT_DIR"
if (( $# > 2 )); then
  printf 'Usage: bash scripts/gpu-startup-smoke.sh [built-backend-smoke] [built-gpu-test]\n' >&2
  exit 2
fi
for command in date find git mktemp python3 realpath sha256sum tee timeout; do
  command -v "$command" >/dev/null || { printf 'Missing command: %s\n' "$command" >&2; exit 2; }
done
SMOKE="$(realpath "${1:-target/debug/examples/backend_smoke}")"
if [[ ! -x "$SMOKE" ]]; then
  printf 'Build backend_smoke before running acceptance: %s\n' "$SMOKE" >&2
  exit 2
fi

# Resolve the already-built test executable from Cargo's recorded artifact
# messages, never guess among old target/debug/deps binaries or rebuild inside
# the cold-run watchdog. A direct explicit path is also supported.
GPU_TEST="${2:-}"
if [[ -z "$GPU_TEST" ]]; then
  if [[ -z "${EMULSION_GPU_TEST_ARTIFACTS:-}" || ! -f "$EMULSION_GPU_TEST_ARTIFACTS" ]]; then
    printf 'Pass the built GPU test executable, or set EMULSION_GPU_TEST_ARTIFACTS to saved cargo test --no-run --message-format=json output.\n' >&2
    exit 2
  fi
  GPU_TEST="$(python3 - "$EMULSION_GPU_TEST_ARTIFACTS" <<'PYARTIFACT'
import json
import sys
paths = set()
with open(sys.argv[1], encoding="utf-8") as source:
    for line in source:
        message = json.loads(line)
        target = message.get("target", {})
        if (message.get("reason") == "compiler-artifact"
                and target.get("name") == "emulsion_gpu"
                and "lib" in target.get("kind", [])
                and message.get("profile", {}).get("test")
                and message.get("executable")):
            paths.add(message["executable"])
if len(paths) != 1:
    sys.exit(f"Expected one built emulsion_gpu test executable, found {len(paths)}")
print(paths.pop())
PYARTIFACT
)"
fi
GPU_TEST="$(realpath "$GPU_TEST")"
if [[ ! -x "$GPU_TEST" ]]; then
  printf 'GPU test executable is not runnable: %s\n' "$GPU_TEST" >&2
  exit 2
fi

# An explicit path supports a separately installed Mesa. Never silently switch
# to another driver when that requested file is missing.
LAVAPIPE_ICD="${EMULSION_LAVAPIPE_ICD:-}"
if [[ -z "$LAVAPIPE_ICD" ]]; then
  for candidate in /usr/share/vulkan/icd.d/lvp_icd*.json /usr/local/share/vulkan/icd.d/lvp_icd*.json; do
    if [[ -f "$candidate" ]]; then
      LAVAPIPE_ICD="$candidate"
      break
    fi
  done
fi
if [[ -z "$LAVAPIPE_ICD" || ! -f "$LAVAPIPE_ICD" ]]; then
  printf 'Mesa Lavapipe ICD not found; set EMULSION_LAVAPIPE_ICD to its JSON file.\n' >&2
  exit 2
fi
LAVAPIPE_ICD="$(realpath "$LAVAPIPE_ICD")"

REPORT_ROOT="${RUNNER_TEMP:-${TMPDIR:-/tmp}}/emulsion-gpu-startup"
mkdir -p "$REPORT_ROOT"
RUN_DIR="$(mktemp -d "$REPORT_ROOT/run.XXXXXX")"
REPORTS="$RUN_DIR/reports"
mkdir "$REPORTS" "$RUN_DIR/caches" "$RUN_DIR/runtime"
trap 'result=$?; printf "Startup acceptance exit %s; reports and caches retained at %s\n" "$result" "$RUN_DIR"' EXIT
printf 'Startup acceptance reports and caches: %s\n' "$RUN_DIR"

{
  printf 'started_utc=%s\n' "$(date -u +%FT%TZ)"
  printf 'source_revision=%s\n' "$(git rev-parse HEAD)"
  printf 'source_worktree_status:\n'
  git status --short
  printf 'smoke_binary=%s\n' "$SMOKE"
  sha256sum "$SMOKE"
  printf 'gpu_test_binary=%s\n' "$GPU_TEST"
  sha256sum "$GPU_TEST"
  printf 'lavapipe_icd=%s\n' "$LAVAPIPE_ICD"
  sha256sum "$LAVAPIPE_ICD"
  cat "$LAVAPIPE_ICD"
  printf '\nplatform:\n'
  uname -a
  if command -v dpkg-query >/dev/null; then
    dpkg-query -W -f='${Package} ${Version}\n' mesa-vulkan-drivers 2>/dev/null || true
  fi
  printf 'smoke_watchdog_seconds=60\nsuite_watchdog_seconds=180\n'
} > "$REPORTS/identity.txt"
printf 'case\texit_status\telapsed_ms\n' > "$REPORTS/results.tsv"

assert_empty() {
  local entry
  entry="$(find "$1" -mindepth 1 -print -quit)" || return
  if [[ -n "$entry" ]]; then
    printf 'Expected an empty isolated cache: %s\n' "$1" >&2
    return 1
  fi
}

new_cache() {
  local cache="$RUN_DIR/caches/$1"
  # mkdir without -p rejects accidental reuse of a cold case.
  # Explicit propagation also works when command substitution clears errexit.
  mkdir "$cache" || return
  mkdir "$cache/mesa" "$cache/xdg" || return
  assert_empty "$cache/mesa" || return
  assert_empty "$cache/xdg" || return
  printf '%s\n' "$cache"
}

verify_cache_used() {
  local cache="$1" label="$2" entries="$1/mesa/mesa_shader_cache"
  # A driver-created index alone does not prove it cached any compiled shader.
  # Force Mesa's multi-file cache below and require a nonempty sharded entry.
  if [[ ! -d "$entries" ]] || [[ -z "$(find "$entries" -mindepth 2 -type f -size +0c -print -quit)" ]]; then
    printf '%s: isolated shader cache use was not verified at %s\n' "$label" "$cache" \
      | tee "$REPORTS/$label.cache-error.txt" >&2
    return 1
  fi
  printf '%s: nonempty shader entries verified in %s\n' "$label" "$entries" \
    | tee "$REPORTS/$label.cache-verified.txt"
}

run_case() (
  local label="$1" limit="$2" mode="$3" brushes="$4" cache="$5"
  shift 5
  export EMULSION_GPU="$mode" EMULSION_REQUIRE_GPU_TESTS=1
  unset EMULSION_GPU_BRUSHES
  if [[ "$brushes" != unset ]]; then
    export EMULSION_GPU_BRUSHES="$brushes"
  fi
  export VK_DRIVER_FILES="$LAVAPIPE_ICD" VK_ICD_FILENAMES="$LAVAPIPE_ICD"
  unset VK_ADD_DRIVER_FILES VK_LOADER_DRIVERS_SELECT VK_LOADER_DRIVERS_DISABLE
  export WGPU_BACKEND=vulkan XDG_RUNTIME_DIR="$RUN_DIR/runtime"
  export XDG_CACHE_HOME="$cache/xdg" MESA_SHADER_CACHE_DIR="$cache/mesa"
  # Do not read an inherited Fossilize cache or silently run cache-disabled.
  # Supported controls: https://docs.mesa3d.org/envvars.html
  unset MESA_DISK_CACHE_READ_ONLY_FOZ_DBS MESA_DISK_CACHE_READ_ONLY_FOZ_DBS_DYNAMIC_LIST
  export MESA_DISK_CACHE_COMBINE_RW_WITH_RO_FOZ=0 MESA_DISK_CACHE_SINGLE_FILE=0
  export MESA_DISK_CACHE_DATABASE=0 MESA_DISK_CACHE_MULTI_FILE=1
  export MESA_SHADER_CACHE_DISABLE=false MESA_SHADER_CACHE_MAX_SIZE=1G
  export MESA_SHADER_CACHE_SHOW_STATS=true
  {
    printf 'started_utc=%s\n' "$(date -u +%FT%TZ)"
    printf 'cache_root=%s\n' "$cache"
    printf 'EMULSION_GPU_BRUSHES=%s\n' "$brushes"
    # Record only relevant settings, never unrelated environment secrets.
    env | LC_ALL=C sort | grep -E '^(EMULSION_GPU=|EMULSION_REQUIRE_GPU_TESTS=|MESA_.*CACHE|VK_.*DRIVER|VK_ICD_FILENAMES=|WGPU_BACKEND=|XDG_CACHE_HOME=|XDG_RUNTIME_DIR=)'
    printf 'command='
    printf '%q ' timeout --signal=KILL "${limit}s" "$@"
    printf '\n'
  } > "$REPORTS/$label.environment.txt"
  find "$cache" -type f -printf '%P\t%s bytes\n' | LC_ALL=C sort > "$REPORTS/$label.cache-before.txt"
  printf '\n=== %s ===\n' "$label"
  local started finished status
  local -a pipeline_status
  started="$(date +%s%N)"
  set +e
  timeout --signal=KILL "${limit}s" "$@" 2>&1 | tee "$REPORTS/$label.log"
  pipeline_status=("${PIPESTATUS[@]}")
  set -e
  status="${pipeline_status[0]}"
  if (( pipeline_status[1] != 0 )); then
    printf 'Could not retain the complete log for %s\n' "$label" >&2
    status="${pipeline_status[1]}"
  fi
  finished="$(date +%s%N)"
  printf '%s\t%s\t%s\n' "$label" "$status" "$(( (finished - started) / 1000000 ))" \
    | tee -a "$REPORTS/results.tsv"
  find "$cache" -type f -printf '%P\t%s bytes\n' | LC_ALL=C sort > "$REPORTS/$label.cache-after.txt"
  return "$status"
)

# Each cold process is the first compute use of its own verified empty cache.
# The immediate warm process has the same gates and reuses only that case's cache.
for brushes in unset 1 persistent; do
  case "$brushes" in
    unset) label=software-default ;;
    1) label=software-final-brush ;;
    persistent) label=software-persistent-brush ;;
  esac
  cache="$(new_cache "$label")"
  run_case "$label-cold" 60 software "$brushes" "$cache" "$SMOKE" --require-vulkan
  verify_cache_used "$cache" "$label-cold"
  run_case "$label-warm" 60 software "$brushes" "$cache" "$SMOKE" --require-vulkan
  verify_cache_used "$cache" "$label-warm"
done

# CPU override must win over either optional brush flag. The binary verifies
# DisabledByConfiguration, no context/hooks/preparation, and complete CPU output.
for brushes in 1 persistent; do
  label="cpu-$brushes"
  cache="$(new_cache "$label")"
  run_case "$label" 60 cpu "$brushes" "$cache" "$SMOKE" --require-vulkan
  assert_empty "$cache/mesa"
  assert_empty "$cache/xdg"
done

# No preceding process has touched this cache. Required tests must initialize
# through production preparation rather than inherit a smoke or QA-helper cache.
cache="$(new_cache required-suite)"
run_case required-suite-cold 180 software unset "$cache" \
  "$GPU_TEST" --nocapture --test-threads=1
verify_cache_used "$cache" required-suite-cold
printf '\nSoftware compute startup matrix and required serial suite passed.\n'
