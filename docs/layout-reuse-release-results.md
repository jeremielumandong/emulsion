# Release layout reuse measurements - 2026-09-23

*Snapshot from 2026-09-23. For current behavior see [layout-reuse-experiment.md](layout-reuse-experiment.md).*

Measured on 2026-09-23 comparing cold and retained layout in the same binary;
the setting is on by default, see [layout-reuse-experiment.md](layout-reuse-experiment.md).
The controlled release workloads show a substantial
benefit for fixed geometry. The real-editor panning trial did not demonstrate a
CPU saving. These results do not establish an application-wide improvement.

## Setup

- Windows 10.0.26200 x86_64; AMD Ryzen 7 8700G; 16 logical processors.
- Rust 1.98.1; release profile with thin LTO and one codegen unit.
- The same compiled benchmark executable ran twice: cold first, then retained first.
- Each of six cases used 50 Criterion samples, 3 seconds of warmup, and 10 seconds
  of requested measurement. No app or compiler processes were running during
  these measurements; the rest of the desktop was not isolated.
- Native text shaping; 256 synthetic rows. Includes CPU element construction,
  layout, and paint preparation. Excludes GPU submission. These fixtures are not
  the editor's virtualized Layers list.
- This was a dirty development checkout. Binary hashes, source revision, confidence
  intervals, and draw p50/p95/p99 values are preserved in the
  [machine-readable results](layout-reuse-release-results.json).

## Synthetic CPU rendering

Times are Criterion per-iteration point estimates in milliseconds, with 95%
confidence intervals. A negative duration change means less time with retention.

| Order | Workload | Cold ms (95% CI) | Retained ms (95% CI) | Duration change |
| --- | --- | --- | --- | --- |
| cold-first | fixed_geometry | 1.9059 (1.8953 to 1.9174) | 0.9197 (0.9153 to 0.9246) | -51.74% |
| cold-first | changing_text | 1.5911 (1.5839 to 1.5984) | 1.5731 (1.5642 to 1.5832) | -1.13% |
| cold-first | structural_churn | 0.9665 (0.9642 to 0.9689) | 0.9559 (0.9520 to 0.9611) | -1.10% |
| retained-first | fixed_geometry | 1.9238 (1.9175 to 1.9313) | 0.9235 (0.9191 to 0.9292) | -52.00% |
| retained-first | changing_text | 1.5779 (1.5723 to 1.5834) | 1.5689 (1.5558 to 1.5827) | -0.57% |
| retained-first | structural_churn | 0.9777 (0.9726 to 0.9843) | 0.9561 (0.9534 to 0.9592) | -2.22% |

The fixed-geometry benefit repeats with reversed ordering. The smaller differences
in changing text and structural churn should not be treated as meaningful editor
CPU savings. They are far smaller than the fixed-geometry effect, and the workload
is synthetic. Frame percentiles in the JSON come from GPUI's report and include
warmup/calibration; the table uses Criterion's measurement estimates.

## Manual release-editor observation

The user performed Space-drag canvas panning for about 30 seconds in each mode.
The same frozen release executable was used, with isolated settings and hardware
rendering on the AMD Radeon RX 7700 XT. The retained run reopened the user's saved
17-layer document. Sampling used `scripts/measure-editor-process.ps1` at roughly
one-second intervals. CPU is process CPU time divided by elapsed wall time and all
16 logical processors, matching the scale of whole-machine CPU utilization.

| Run | Average CPU | Peak sampled CPU | Final private MiB | Peak sampled private MiB | Final working set MiB |
| --- | --- | --- | --- | --- | --- |
| cold-canvas-pan-1 | 2.72% | 6.15% | 643.8 | 680.8 | 481.8 |
| retained-canvas-pan-1 | 2.92% | 6.35% | 519.2 | 520.1 | 350.4 |

This is one human-driven observation per mode, not a controlled performance result.
Mouse rate, exact viewport, FPS, and frame times were not recorded. The cold process
had prior editing/open tabs, while the retained process reopened the saved file;
memory/history/cache state was not matched. The lower retained memory reading
therefore cannot be attributed to layout reuse. The roughly 0.20 percentage-point
CPU difference is insufficient to conclude a regression or improvement.

The earlier Layers-scroll sample is excluded: the user had not identified the
panel, so the intended workload was not established. During preparation the app
also logged a missing `icons/trash.svg` asset repeatedly. That is an existing app
render/logging issue to investigate separately; it was not fixed mid-comparison.

## Reproduce

Run from the repository root in PowerShell, with other builds and the editor closed:

```powershell
$env:EMULSION_LAYOUT_BENCH_RETAINED_FIRST = '0'
cargo bench -p emulsion-ui --features layout-bench --bench layout_reuse --profile release -- --sample-size 50 --warm-up-time 3 --measurement-time 10 --noplot --save-baseline release-cold-first
$env:EMULSION_LAYOUT_BENCH_RETAINED_FIRST = '1'
cargo bench -p emulsion-ui --features layout-bench --bench layout_reuse --profile release -- --sample-size 50 --warm-up-time 3 --measurement-time 10 --noplot --save-baseline release-retained-first
Remove-Item Env:EMULSION_LAYOUT_BENCH_RETAINED_FIRST
```

For an existing editor process, replace `12345` with its PID, arrange the workload,
and start the sampler when the person performing it is ready:

```powershell
powershell.exe -NoProfile -ExecutionPolicy Bypass -File scripts/measure-editor-process.ps1 -ProcessId 12345 -Label cold-canvas-pan -Seconds 30
```

The sampler writes per-interval CSV and a JSON summary under `target/performance`.
It does not drive input, change application state, or capture GPU memory. Raw logs
for this run are `target/performance/layout-release-{cold-first,retained-first}-20260923.log`;
Criterion estimates are under each corresponding named baseline in
`target/criterion/layout_reuse`. Both release benchmark orders exited successfully.

Next validation should restart both modes into identical saved documents/tabs,
use the same viewport and a repeatable input trace, and record frame count and
latency alongside CPU. At measurement time this evidence supported keeping reuse
off while testing more representative layout workloads; the setting is on by
default, see [layout-reuse-experiment.md](layout-reuse-experiment.md).
