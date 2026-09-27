#!/usr/bin/env bash
# Run in an interactive desktop session. Writes reports only to a new temp dir.
set -euo pipefail
cd "$(dirname "$0")/.."
report_dir=$(mktemp -d "${TMPDIR:-/tmp}/emulsion-vello-validation.XXXXXX")
printf 'Writing Vello validation reports to %s\n' "$report_dir"
uname -a > "$report_dir/platform.txt"
cargo test --locked -p emulsion-core -p emulsion-raster --lib -- --test-threads=1 > "$report_dir/core.log" 2>&1
EMULSION_REQUIRE_GPU_TESTS=1 cargo test --locked -p emulsion-engine --lib -- --test-threads=1 > "$report_dir/engine.log" 2>&1
EMULSION_REQUIRE_GPU_TESTS=1 cargo test --locked -p vello-canvas-spike -- --test-threads=1 > "$report_dir/fidelity.log" 2>&1
cargo test --locked -p emulsion-ui --lib -- --test-threads=1 > "$report_dir/ui.log" 2>&1
cargo build --locked --release -p emulsion-app --features canvas-bench --example editor_canvas_bench
EMULSION_GPU_CANVAS=1 target/release/examples/editor_canvas_bench "$@" > "$report_dir/editor-gpu.json" 2> "$report_dir/editor-gpu.log"
EMULSION_GPU_CANVAS=0 target/release/examples/editor_canvas_bench "$@" > "$report_dir/editor-cpu.json" 2> "$report_dir/editor-cpu.log"
printf 'Automated checks passed. Reports: %s\n' "$report_dir"
printf '%s\n' 'Also complete the manual platform checks in spikes/vello-canvas/PLATFORM_VALIDATION.md.'
