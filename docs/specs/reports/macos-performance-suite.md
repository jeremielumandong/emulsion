# macOS performance baseline — latest code, 2026-09-28

Measured **`782da4d`**, pulled before rebuilding. Benchmarked package sources matched that revision. The local compact UI edit does not participate in these targets; the proposed layout optimization was set aside for this baseline.

Apple M1 MacBook Pro, 8 CPU cores, 16 GB RAM, macOS 26.3 (25D125), arm64, Rust 1.98.1. Locked release builds use thin LTO and one codegen unit. Workloads ran sequentially after compilation finished, with Emulsion closed and normal desktop services still active. GPU workloads used the Apple M1 Metal adapter; GPU initialization was required, so unavailable hardware could not silently skip tests.

**41 invocations passed**, including warmups, repeated comparisons and the GPU correctness suite. All six manual GPU benchmarks executed; the separate correctness suite passed 23 tests (the six manual benchmarks were ignored in that particular invocation).

These are component workloads, not OS input-to-screen latency or native-window FPS. Each benchmark keeps its existing sampling method; CPU/GPU means, per-run medians, and per-edit timings below are deliberately distinguished.

## Photo

The existing read-only `raw_workflow_bench` processed the three hash-verified files in [the RAW corpus manifest](../../../crates/emulsion-io/tests/fixtures/raw-corpus.json). Each run measures one decode, full development, a first fit preview and eight cached exposure edits. Every source remained unchanged, and edited previews differed from the neutral preview. Default development uses the latest demosaicing; AI sensor denoise is off.

| Camera | Developed dimensions | Decode | Full develop | First fit preview | Median cached edit | Process peak RSS |
| --- | --- | ---: | ---: | ---: | ---: | ---: |
| Nikon D50 | 3008 × 2000 | 119.67 ms | 279.45 ms | 199.78 ms | 14.47 ms | 494 MiB |
| Fujifilm X-Pro1 | 4896 × 3264 | 57.18 ms | 805.55 ms | 528.13 ms | 12.58 ms | 1207 MiB |
| Canon EOS M50 | 6000 × 4000 | 165.67 ms | 1189.30 ms | 810.55 ms | 14.73 ms | 1844 MiB |

Peak RSS is for the entire process, including full-resolution development and three reopens. It is not the memory of a single preview or evidence of a leak. The benchmark’s `/proc` fields are empty on macOS; `/usr/bin/time -l` supplies the process maximum instead.

The 6030 × 4030 CPU Gaussian workload (radius 5, eight Rayon threads) took **139.61 ms** median across three processes; all checksums matched.

At 2048 × 2048, three repeated CPU/Metal comparisons showed:

| Filter | CPU median range | GPU median range | Finding |
| --- | ---: | ---: | --- |
| Gaussian blur, radius 5 | 19.89–23.35 ms | 71.03–73.85 ms | CPU faster |
| Gaussian blur, radius 20 | 67.18–88.29 ms | 94.87–96.44 ms | CPU faster |
| Twirl | 15.93–20.28 ms | 32.30–33.54 ms | CPU faster |
| Add noise | 32.53–35.78 ms | 33.14–34.96 ms | Similar; varies by run |
| Reduce noise | 101.46–102.06 ms | 40.96–43.99 ms | GPU faster |

GPU filter timings include upload, synchronization and readback. These comparisons do not measure the current UI’s backend-selection policy.

## Diagram

Existing command, SVG viewport and offscreen GPU benchmarks all passed at 100 and 1,000 shapes. The nested 1,000-shape viewport case passed as well. These fixtures use 99/999 connectors. Commands use orthogonal connectors; the viewport and GPU fixtures use straight connectors, so their edit times are not interchangeable.

| Workload | Result |
| --- | ---: |
| 1,000-shape command benchmark: move | 7.33 ms p50 / 8.16 ms p95 |
| 1,000-shape SVG viewport: edit | 6.86 ms p50 / 7.81 ms p95 |
| SVG scene update | 4.58 ms p50 / 4.83 ms p95 |
| SVG dirty-region patch | 0.99 ms p50 / 1.08 ms p95 |
| Cold SVG scene creation | 1,103 ms |
| SVG roots rebuilt per move | At most 3, including nested case |
| 100-shape command through GPU completion | 12.96 ms p50 / 13.79 ms p95 |
| 1,000-shape command through GPU completion, three runs | 27.71–28.68 ms p50; 29.27–34.49 ms p95 |
| 1,000-shape GPU render/completion stage, three runs | 17.54–17.60 ms p50 |

The GPU test asserted that the move changed visible pixels and that rendering used no unexpected raster fallback. GPU completion excludes OS input delivery and compositor presentation. The render/completion stage is the largest measured part of this M1 workload.

The earlier [Linux diagram report](diagram-completion-results.json) recorded 13.00 ms median command-to-GPU completion on an AMD RX 7700 XT. This M1 result is slower in that fixture, but hardware, driver, revision and workload conditions differ; it is not evidence of an operating-system-only difference.

## Paint

`brush_benchmark` measures CPU dab generation and composition on a 4096 × 4096 canvas with 64 px brushes: 65 samples per stroke and three strokes per case. It excludes input and display latency.

| Brush | Update p50 | Update p95 |
| --- | ---: | ---: |
| dry | 0.424 ms | 1.062 ms |
| grain | 0.559 ms | 1.002 ms |
| image-tip | 0.203 ms | 0.442 ms |
| scatter | 0.435 ms | 0.746 ms |
| wet | 0.277 ms | 0.518 ms |
| smudge | 0.273 ms | 0.508 ms |
| erase | 0.516 ms | 0.907 ms |
| dual | 1.212 ms | 2.802 ms |

The integrated persistent-brush comparison uses a 512 × 512 raster and includes setup, eight updates, raster results and stroke finishing. Across three runs:

| Brush size | CPU mean per stroke | Routed mean per stroke |
| --- | ---: | ---: |
| 40 px (both routes stay on CPU) | 4.87–5.21 ms | 4.88–5.21 ms |
| 400 px | 38.69–38.93 ms | 31.18–31.35 ms |

The large-brush route used about **19% less time** in this fixture. Pixel-parity and routing assertions passed. It does not imply the same gain for other brush features or larger rasters.

Dense four-tile buffer reuse reduced GPU composition from 5.60–5.65 ms to 3.96–4.01 ms (about 29%) and eliminated retained-buffer reallocations after warmup. CPU composition still took only 2.32–2.34 ms. The sparse and dense fixed-stroke GPU composition paths were also slower than CPU.

The compositor benchmark showed workload-dependent results: for 16 tiles, one normal layer took 2.06 ms on CPU versus 32.66 ms on GPU, while eight blended layers took 563.54 ms versus 135.94 ms. These isolated forced-backend tests include readback and do not describe automatic routing in a live canvas.

## Responsive Design layout

The existing spacing-edit workload ran five measured processes per size after one discarded warmup process. Each process measured 20 edits and passed document validation and repeated-layout idempotence. These are latest-revision baselines before the local optimization.

| Objects / frames | Median of run medians | Range of run medians | Worst edit |
| --- | ---: | ---: | ---: |
| 1,000 / 10 | 16.31 ms | 16.18–17.10 ms | 18.10 ms |
| 4,000 / 40 | 81.25 ms | 80.79–84.01 ms | 101.25 ms |

The 20-sample layout benchmark’s reported p95 equals its maximum. No pooled percentile is inferred from the per-process summaries.

A subsequent local movement-link optimization was measured separately against
this exact revision: 1,000-object edits fell from 16.20 to 3.77 ms (76.8% less
time), and 4,000-object edits from 80.79 to 30.84 ms (61.8% less time). The baseline
tables above remain the unmodified latest-code measurements. See the
[paired comparison and validation](../../technical/design-layout-performance.md#single-object-movement-optimization-on-latest-code).

## Coverage and reproduction

Existing benchmark entry points:

- Photo: `emulsion-io --example raw_workflow_bench`, `emulsion-filters --example gaussian_perf`, and GPU `benchmark_filters` / `benchmark_tile_compositing`.
- Diagram: `emulsion-core --example diagram_bench`, `emulsion-io --example diagram_viewport_bench`, `emulsion-engine --example diagram_gpu_bench`.
- Paint: `emulsion-io --example brush_benchmark` and GPU `benchmark_stroke_compositing`, `benchmark_stroke_buffer_reuse`, `benchmark_persistent_application_brush`, `benchmark_routed_persistent_brush`.
- Layout: `emulsion-io --example design_layout_bench`.

Build examples with `cargo build --locked --release -p PACKAGE --example NAME`, finish all compilation, then run `target/release/examples/NAME` with the arguments recorded in the raw results. GPU benchmarks use `EMULSION_REQUIRE_GPU_TESTS=1 WGPU_BACKEND=metal cargo test --locked --release -p emulsion-gpu NAME -- --ignored --nocapture --test-threads=1`; run each benchmark separately after the test binary is built. RAW samples come from the project’s existing CC0 corpus manifest and are verified against its hashes.

Not covered here: native-window presentation/input latency, tablet latency, catalog scrolling, Photo/Draw switching, HDR, panorama, focus stacking, depth effects and AI sensor denoising. These results cover available representative workloads in each requested domain, not every feature or benchmark in the repository.

[Raw results, exact commands, environment and all outputs](macos-performance-suite-results.json).
