# Windows performance suite — provisional latest-main run, 2026-09-29

Fetched and fast-forwarded `main` to **`befd6e8`** before rebuilding and measuring. All 19 pre-existing local files were preserved byte-for-byte during the update. This measurement includes the local Windows device-cache, lazy vector initialization and DXC bundling changes described in [the canvas-loading report](windows-canvas-loading.md). It is not a pristine-commit baseline. Earlier runs made before fetching main were set aside and are not used here.

**All 41 invocations passed**, including warmups, repeated comparisons and all six manual GPU benchmarks. The separate GPU correctness invocation passed 23 tests, with six manual benchmarks intentionally ignored in that invocation. Source and RAW fixture hashes were unchanged during the suite.

**Provisional timings:** Cargo/rustc processes from another project were observed during 39 of the 41 invocations, detected by one-second process sampling. The runner explicitly allowed and recorded this activity. CPU and shared-resource timings can be affected; these results are a latest-main functional/performance snapshot, not a clean Windows–Mac speed comparison. A quiet-machine rerun is needed before diagnosing a regression.

## Machine and comparison conditions

Windows 11 build 26200, x64; AMD Ryzen 7 8700G, 8 physical cores / 16 logical CPUs; 61.61 GiB OS-reported usable physical RAM; AMD Radeon RX 7700 XT; Rust 1.98.1. Locked release builds use thin LTO and one codegen unit. The benchmark binaries finished compiling before sequential measurement; Emulsion was closed. Normal desktop services remained active. No thermal or power controls were imposed.

The [Mac reference](macos-performance-suite.md) used an Apple M1 MacBook Pro (8 CPU cores, 16 GB RAM), macOS 26.3 and revision `782da4d`. Arguments, fixtures, process order and repetition counts match that 41-run suite. Only the Gaussian workload pins Rayon to eight threads; other workloads use the machine default, as on Mac. These are comparisons between two machines and code revisions, **not an operating-system-only comparison**.

Actual adapter diagnostics identify two Windows paths:

- Diagram canvas: **DirectX 12**, driver `32.0.31041.1004`, bundled Microsoft DXC 1.8.2505.1. The example now uses the engine instance factory so `WGPU_BACKEND=dx12` takes effect.
- Image compute / GPU tests: **Vulkan**, AMD driver `26.8.1 (LLPC)`. The existing compute selector ignores `WGPU_BACKEND` and chooses its own adapter; its routing was not changed. A test-only diagnostic records that choice.

The Mac paths both used Metal. GPU filter/brush/compositor timings below must not be described as DirectX timings.

## Photo

Each hash-verified CC0 RAW runs one decode, full development, first fit preview and eight cached exposure edits. Every edited preview differs from neutral. AI sensor denoise is off. “First fit” concerns application preview caching; the OS file cache was not flushed. Times are milliseconds, with Windows / Mac pairs.

| Camera | Dimensions | Decode W / M | Full develop W / M | First fit W / M | Median cached edit W / M |
| --- | --- | --- | --- | --- | --- |
| Nikon D50 | 3008 × 2000 | 95.10 / 119.67 | 288.95 / 279.45 | 227.64 / 199.78 | 9.76 / 14.47 |
| Fujifilm X-Pro1 | 4896 × 3264 | 94.58 / 57.18 | 750.88 / 805.55 | 543.16 / 528.13 | 9.67 / 12.58 |
| Canon EOS M50 | 6000 × 4000 | 199.93 / 165.67 | 1601.31 / 1189.30 | 1265.64 / 810.55 | 59.09 / 14.73 |

Observed Windows process peak working sets: D50 320 MiB, X-Pro1 821 MiB, M50 1232 MiB. These include full development and three reopens. They use `GetProcessMemoryInfo.PeakWorkingSetSize`, sampled every 20 ms; a final short-lived increase can be missed. Windows working set and the Mac report’s peak RSS have different accounting and are not compared as equivalent metrics.

The 6030 × 4030 CPU Gaussian workload (radius 5, eight Rayon threads) took **274.39 ms** median across three Windows processes (range 256.43–280.44), versus **139.61 ms** on Mac. All six checksums match.

2048 × 2048 filters, ranges of three per-process medians in milliseconds:

| Filter | Windows CPU | Windows Vulkan | Mac CPU | Mac Metal |
| --- | --- | --- | --- | --- |
| Gaussian radius 5 | 22.77–24.38 | 101.83–105.42 | 19.89–23.35 | 71.03–73.85 |
| Gaussian radius 20 | 62.05–66.86 | 104.57–110.35 | 67.17–88.29 | 94.87–96.44 |
| Twirl | 15.24–15.62 | 50.62–52.51 | 15.93–20.28 | 32.30–33.54 |
| Add noise | 23.45–25.98 | 51.62–53.64 | 32.53–35.77 | 33.14–34.95 |
| Reduce noise | 74.20–84.76 | 53.58–59.99 | 101.46–102.06 | 40.96–43.99 |

GPU filter timings include upload, synchronization and readback. Background load prevents attributing cross-machine differences to GPU hardware or backend. In this sample CPU is faster for blur, twirl and add-noise on Windows; reduce-noise benefits from GPU. These forced comparisons do not measure automatic UI routing.

## Diagram and vector rendering

100- and 1,000-shape fixtures and the nested 1,000-shape SVG fixture all passed. Each has 99/999 connectors. Commands use orthogonal connectors; SVG and GPU fixtures use straight connectors, so their edit timings are different workloads.

| 1,000 shapes, milliseconds | Windows p50 / p95 | Mac p50 / p95 |
| --- | --- | --- |
| Move command | 16.36 / 18.28 | 7.33 / 8.15 |
| SVG edit | 9.89 / 12.03 | 6.86 / 7.81 |
| SVG scene update | 7.52 / 10.98 | 4.58 / 4.83 |
| SVG dirty-region patch | 1.64 / 2.23 | 0.99 / 1.08 |

Cold SVG scene creation: **2188.30 ms Windows / 1103.02 ms Mac**. At most three SVG roots were rebuilt per move, including the nested case. This is scene construction, not end-to-end file opening.

| Command through GPU completion, ms | Windows p50 / p95 | Mac p50 / p95 |
| --- | --- | --- |
| 100 shapes | 4.02 / 4.99 | 12.96 / 13.79 |
| 1,000 shapes, three runs | 13.58–14.62 / 14.81–15.84 | 27.71–28.68 / 29.27–34.49 |

For 1,000 shapes, render/GPU completion alone took 1.82–1.87 ms p50 on Windows versus 17.54–17.60 ms on Mac. CPU edit/reload work is a larger part of the Windows total. Pixel-change and unexpected-raster-fallback assertions passed. These offscreen timings exclude physical display and OS input latency; stage medians cannot be summed to derive the total median.

## Paint

CPU brush fixture: 4096 × 4096 canvas, 64 px brushes, 65 samples per stroke and three strokes per case. Update times in milliseconds:

| Brush | Windows p50 / p95 | Mac p50 / p95 |
| --- | --- | --- |
| dry | 0.581 / 1.609 | 0.424 / 1.062 |
| grain | 0.745 / 1.934 | 0.559 / 1.002 |
| image-tip | 0.379 / 1.249 | 0.203 / 0.442 |
| scatter | 0.856 / 1.805 | 0.435 / 0.746 |
| wet | 0.666 / 1.623 | 0.277 / 0.518 |
| smudge | 0.462 / 0.921 | 0.273 / 0.508 |
| erase | 0.751 / 1.625 | 0.516 / 0.907 |
| dual | 2.692 / 8.082 | 1.212 / 2.802 |

Integrated persistent-brush fixture: 512 × 512 raster, setup, eight updates, raster results and stroke finishing included. Ranges of three per-process means, milliseconds per stroke:

| Brush | Windows CPU | Windows routed | Mac CPU | Mac routed |
| --- | --- | --- | --- | --- |
| 40 px | 8.24–8.30 | 8.05–8.29 | 4.87–5.21 | 4.88–5.21 |
| 400 px | 56.79–59.23 | 17.66–18.55 | 38.69–38.93 | 31.18–31.35 |

The 40 px route stays on CPU. The 400 px route uses persistent GPU composition and is faster than CPU on both machines; pixel parity and routing assertions passed. This gain does not apply to unsupported brush features.

Dense four-tile changing strokes, ranges of three means in milliseconds:

| Machine | CPU | Fresh GPU | Reused GPU |
| --- | --- | --- | --- |
| Windows | 5.46–5.92 | 8.36–10.81 | 6.41–8.54 |
| Mac | 2.32–2.34 | 5.60–5.64 | 3.96–4.01 |

Reused buffers made no retained-buffer allocations after warmup. The fixed-stroke composition and isolated persistent-application brush benchmarks also passed; their complete timings are in the raw results.

16-tile compositor examples, milliseconds:

| Fixture | Windows CPU | Windows Vulkan | Mac CPU | Mac Metal |
| --- | --- | --- | --- | --- |
| One normal layer | 5.50 | 28.19 | 2.06 | 32.66 |
| Eight blended layers | 764.27 | 222.89 | 563.54 | 135.94 |

These are forced-backend composition tests including readback, not the automatic routing policy of a live canvas.

## Responsive layout

One discarded warmup process per size, followed by five measured processes of 20 edits each. Every process validated its document and repeated-layout idempotence. Current Windows code includes the single-unlinked-object movement optimization, so the later [optimized Mac measurement](../../technical/design-layout-performance.md#single-object-movement-optimization-on-latest-code) is the closer reference.

| Objects | Windows median | Windows median range | Windows worst edit | Mac optimized median | Mac original baseline |
| --- | --- | --- | --- | --- | --- |
| 1,000 | 4.09 | 3.87–4.21 | 4.78 | 3.77 | 16.31 |
| 4,000 | 31.41 | 30.65–35.59 | 44.96 | 30.84 | 81.25 |

Times are milliseconds. Medians are medians of five process medians. With 20 samples the benchmark’s p95 equals its maximum; no pooled percentile is inferred. Differences from the original Mac layout baseline include a code optimization and cannot be credited to Windows.

## Loading and remaining limits

This suite separates steady-state costs from the earlier Windows startup problem. The [separate startup measurements](windows-canvas-loading.md) found multi-second FXC/device initialization and measured the DXC, device-reuse and lazy-vector changes. Those samples were collected on `7f6251d` plus the fixes and were not rerun or relabeled as `befd6e8` measurements here.

The suite does not establish end-to-end time to open an arbitrary raster/SVG file, native-window FPS, input/tablet latency, catalog scrolling or workspace-switch latency. It also excludes HDR, panorama, focus stacking, depth effects and AI denoising. Faster offscreen vector rendering does not rule out a Windows file-loading or presentation bottleneck.

## Reproduction and artifacts

Use [the Windows runner](../../../scripts/run-windows-performance.py) after fetching/updating `main`, closing Emulsion and finishing release compilation. Download the three originals in [the RAW corpus manifest](../../../crates/emulsion-io/tests/fixtures/raw-corpus.json) into `target/raw-corpus`; the runner verifies their hashes before and after measurement. Stage the pinned DXC runtime with `scripts/build-windows.ps1` (or its `Install-WindowsDxc` helper) into `target/release`.

```powershell
# Build all examples and the GPU test binary; finish both before measuring.
cargo build --release --locked -p emulsion-io -p emulsion-core -p emulsion-filters -p emulsion-engine --example raw_workflow_bench --example gaussian_perf --example diagram_bench --example diagram_viewport_bench --example diagram_gpu_bench --example brush_benchmark --example design_layout_bench
if ($LASTEXITCODE -ne 0) { throw "Example build failed" }
$artifacts = cargo test --release --locked -p emulsion-gpu --lib --no-run --message-format=json
if ($LASTEXITCODE -ne 0) { throw "GPU test build failed" }
$gpuBinary = $artifacts | ForEach-Object { $_ | ConvertFrom-Json } | Where-Object { $_.reason -eq "compiler-artifact" -and $_.profile.test -and $_.executable } | Select-Object -ExpandProperty executable
python scripts/run-windows-performance.py --gpu-test-binary $gpuBinary
if ($LASTEXITCODE -ne 0) { throw "Performance suite failed" }
```

The measured builds additionally used `--offline` with dependencies already cached. The runner normally refuses to start during Rust compilation and samples competing build/app processes once per second. This recorded run added `--allow-background-activity` because another project kept building; omit that flag for the quiet rerun. The runner requires GPU tests, sets DXC on the DLL search path, and records source/binary/runtime hashes, local source diff, exact commands, environment overrides, hardware, complete outputs and validation. This is a manual hardware measurement; hosted CI machines do not provide this GPU comparison.

Invocations with observed competing processes: `photo-raw-nikon-d50`, `photo-raw-fujifilm-x-pro1`, `photo-raw-canon-eos-m50`, `photo-gaussian-1`, `photo-gaussian-2`, `photo-gaussian-3`, `diagram-command-100`, `diagram-viewport-100`, `diagram-gpu-100`, `diagram-command-1000`, `diagram-viewport-1000`, `diagram-gpu-1000`, `diagram-viewport-nested-1000`, `paint-cpu`, `benchmark_filters`, `benchmark_stroke_compositing`, `benchmark_stroke_buffer_reuse`, `benchmark_persistent_application_brush`, `benchmark_routed_persistent_brush`, `benchmark_tile_compositing`, `layout-1000-latest-warmup`, `layout-1000-latest-1`, `layout-1000-latest-2`, `layout-1000-latest-3`, `layout-1000-latest-4`, `layout-1000-latest-5`, `layout-4000-latest-warmup`, `layout-4000-latest-1`, `layout-4000-latest-2`, `layout-4000-latest-5`, `diagram-gpu-1000-repeat-2`, `benchmark_filters-repeat-2`, `benchmark_stroke_buffer_reuse-repeat-2`, `benchmark_routed_persistent_brush-repeat-2`, `diagram-gpu-1000-repeat-3`, `benchmark_filters-repeat-3`, `benchmark_stroke_buffer_reuse-repeat-3`, `benchmark_routed_persistent_brush-repeat-3`, `gpu-correctness`.

[Raw results and provenance](windows-performance-suite-results.json). Per-process logs are also retained locally in `target/performance-windows`.
