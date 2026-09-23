# Brush implementation validation ? 2026-09-22

4096 × 4096 canvas; 64 px brush; 65 timestamped pressure/tilt samples; three strokes per case. Update includes sampling and CPU compositing. No GPU or display/input latency measurement. Debug profile uses workspace optimization settings.

|Case|p50 update ms|p95 update ms|p95 finish ms|
|---|---:|---:|---:|
|dry|0.622|1.745|0.041|
|grain|1.212|2.653|0.010|
|image-tip|0.664|1.661|0.008|
|scatter|1.126|2.251|0.006|
|wet|0.724|1.963|0.012|
|smudge|0.567|1.593|0.003|
|erase|0.972|2.209|0.003|
|dual|3.340|10.027|0.005|

## Reproduce

```powershell
cargo run -p emulsion-io --example brush_benchmark --offline -- target/brush-validation
```

The example writes eight PNG swatches and a measurement report. The scatter, dual, and smudge images were visually inspected. The source image fixture is an original generated diamond; no Fabric repository images or code were copied. The new built-in Moving paper marker, Woven roller, Scattered pigment, and Turning petals presets exercise moving grain, staged smoothing, directional scatter, rotation variation and color dynamics.

The benchmark samples one moderate brush size, path, and canvas size, with three strokes per case. It is a reference measurement, not a latency service-level guarantee. GPU parity/performance, memory under a large asset catalog, live display stalls, transformed 8K workloads, native tablet latency and physical pen cancellation still require hardware validation. Advanced and dual settings deliberately use the tested CPU path.

## Compatibility boundaries

Native packages preserve selected brushes or a complete set/library hierarchy, including empty collections. External archive/ABR parsing is covered by synthetic fixtures; real packs across proprietary versions still need a compatibility corpus. See [format limits](brush-import-formats.md).

The implementation retains the existing visible-layer sampling default for interactive wet painting and adds a current-layer alternative. MCP's opt-in merged sampling excludes upper layers. The UI and guide state the distinction rather than silently changing existing artwork behavior.

A physical Windows pen and the PDF's complete drawing exercise have not been manually verified in this environment. 3D material brushes and cloud synchronization remain outside scope. Source images are durable and retained for reset/undo; automatic orphan-asset garbage collection is not performed.

## Automated checks

Strict Clippy passed with `-D warnings` across all targets of `emulsion-raster`, `emulsion-io`, `emulsion-mcp`, and `emulsion-ui`; the brush-domain library/tests passed its separate strict check. Final diff whitespace checks passed.

- Raster: 147 tests passed, including neutral-default compatibility, deterministic preview/canvas agreement, parameter pixel effects and dual composition.
- Brush domain: 5 tests passed for hierarchy, identity, migration, memories and combine/uncombine.
- IO: full 106-test suite passed; the final 11-test library subset passed after adding hierarchy export, builtin backfill and runtime-asset promotion regressions.
- MCP: full 96-test suite passed; final brush subset 6/6 passed after mode forwarding and swatch scaling changes.
- UI: the full run passed 355 tests, failed three, and ignored one. Two brush regressions found in that run were fixed. The final focused brush suite passed **24/24**, including numeric Studio save, active/inactive editor propagation, quick-override preservation, same-event draft freshness, import rebasing, dual/source preservation, sampling pixels, source transforms, letterbox coordinates and keyboard/document isolation.
- The remaining full-UI failure is unrelated to brushes: the landing-image test at `crates/emulsion-ui/src/tests.rs:1000` expects 2172 ? 724, while the existing image opens as 2508 ? 627. Neither that assertion nor the landing image was changed here.

Windows normalization/cancellation tests ran in the full UI suite, but physical device pressure and tilt remain unverified. No passing full-workspace test claim is made.

Relevant rerun commands:

```powershell
cargo test -p emulsion-ui --lib --offline brush -- --test-threads=1
cargo test -p emulsion-raster --lib --offline -- --test-threads=1
cargo clippy -p emulsion-raster -p emulsion-io -p emulsion-mcp -p emulsion-ui --all-targets --offline -- -D warnings
```

## MCP authoring extension

The MCP now registers eight document-independent brush tools; see [Brush MCP guide](brush-mcp.md). Validation after the extension:

- MCP: 110 unit tests and 1 drawing integration test passed, including atomic/stale catalog writes, metadata/reset points, sources, package hierarchy/dry-run, memories, tool registration, rich samples, seeds, and dual-preview parity.
- Assistant: 33 tests passed serially, including read-only permissions. An initial parallel run hit a Windows subprocess timing failure; its isolated rerun and the full serial run passed.
- UI: 25 brush-filtered tests and the shared-catalog publication test passed serially. Animated rich samples match immediate rendering pixel-for-pixel. Initial parallel brush tests contended over their shared catalog; use `--test-threads=1` as above.
- Strict Clippy passed for all MCP/UI targets and assistant library/tests. The broader assistant all-targets check hits an existing `collapsible_if` warning in `examples/windows_cli_smoke.rs:72`; that unrelated example was not changed.
- Formatting and changed-file whitespace checks passed.

Catalog transactions refresh the shared app library off the UI thread and retain independent revision protection. They do not create document undo entries; original/custom brush reset points remain available. The full UI suite was not rerun for this MCP extension.
