# RAW Library performance passes

The workload is all 22 RAW files supplied in `LandscapeRaw` and `PortraitRaw`: 15 Canon CR2 files at about 20 MP, one 24 MP DNG, four Canon CR3 files (three about 45 MP and one 24 MP), and two 24 MP Nikon NEF files. One CR2 is a duplicate. Benchmarks operate on workspace copies; no originals, sidecars, or saved edits are written. SHA-256 checks verify the copies against the originals.

## Changes

1. **Loading and first development:** reuse one mapped RAW container and decoder for metadata and sensor decoding. Preserve both content-verification hashes, pre-decode dimension checks, memory reservations, and panic containment. Reduce camera samples into the fit-preview cache in parallel by output row, preserving each pixel's accumulation order. Parallelize full-resolution camera-to-working-color conversion.
2. **Sensor and edit processing:** parallelize sensor normalization by row with cancellation and finite-value checks. Prepare recipe powers and inactive color/calibration flags once per render, preserving the existing scalar arithmetic and full-resolution export path.
3. **Library display and cancellation:** send already-developed opaque display pixels directly to GPUI after RGBA-to-BGRA conversion. Retain the existing compositor path for recipes and transparency. Keep a successfully decoded source when a newer slider revision cancels its preview, provided the same photo is still selected. Stale images and settings remain rejected.
4. **Demosaic neighborhoods:** precompute the ordered, color-filtered neighborhood for each repeating CFA phase. Interior pixels reuse offsets, spatial distances, and same-color gradient eligibility. Image borders retain the original clamping logic; accumulation order and pixel math are unchanged. Exact scalar comparisons cover all four Bayer phases, a 6×6 CFA, odd dimensions, and small images.

These are normal production paths, with no experimental feature flag or reduction in preview resolution.

## Results

Measured on Windows with an AMD Ryzen 7 8700G (8 cores / 16 logical processors), Rust 1.98.1, and the normal release profile (`opt-level=3`, thin LTO, one codegen unit). Final validation includes three alternating baseline/optimized runs for every file and 21 timed samples per file/edit case, following warmups.

| CPU stage | Before | After | Median paired time reduction |
| --- | ---: | ---: | ---: |
| Load + first fit development | 886.7 ms | 498.7 ms | 44.5% |
| First fit development alone | 684.3 ms | 287.6 ms | 57.5% |
| Cached exposure edit | 8.92 ms | 6.28 ms | 29.8% |
| Cached white-balance edit | 8.93 ms | 6.33 ms | 30.3% |
| Cached tone edit | 19.84 ms | 17.32 ms | 13.9% |
| Cached color edit | 9.17 ms | 6.65 ms | 26.9% |
| Cached rotation | 12.91 ms | 10.02 ms | 20.9% |
| Cached neutral development | 9.09 ms | 6.28 ms | 31.3% |
| Full-resolution development | 932.7 ms | 368.7 ms | 61.2% |
| Opaque Library display handoff | 7.25 ms | 0.57 ms | 92.2% |

Before/after columns are medians across per-file medians. Percentages are medians of paired per-file reductions, so they need not equal the ratio of the group medians. File loading alone was essentially unchanged (211.7 → 209.6 ms; paired reduction 0.3%). The opening improvement comes from faster development and avoiding repeated loads after cancellation.

| Folder | Load + first fit, before → after | Full development, before → after |
| --- | ---: | ---: |
| LandscapeRaw (16 files) | 879.3 → 490.7 ms | 925.3 → 365.1 ms |
| PortraitRaw (6 files) | 1,423.6 → 782.8 ms | 1,549.6 → 606.4 ms |

The three approximately 45 MP portraits improve from a median 1,849.6 ms to 1,029.7 ms for loading plus first fit development. All 22 files improve in first-preview and full-resolution processing. In the main sweep, 131 of 132 cached file/edit combinations improve; `P1060804.dng`'s tone case measures 18.03 → 19.25 ms. The recorded main sweep is retained unchanged.

A focused follow-up of that DNG used five additional alternating runs with 15 timed samples per edit (75 samples per case/build). Its tone case measured 18.98 → 16.05 ms, a 15.4% reduction, so the small regression did not reproduce. All six edit cases improved in this follow-up and all pixel hashes matched. Both datasets are preserved; the follow-up does not replace any main-sweep results.

The first exploratory pass reduced median full-development time by 15.5%; the second CPU pass reached 17.0% less first-preview time and 30.1% less full-development time. The fourth pass's CFA neighborhood reuse provides the larger final gains. Intermediate runs were exploratory and included changing background load; the final result uses paired repeated runs.

All source hashes, dimensions, first-preview pixels, six edited outputs, and full-resolution RGBA16 outputs match the baseline exactly across the final runs. All 22 display comparisons also produce identical BGRA bytes. The display benchmark uses 20 measured old/new pairs per photo after two warmups.

The complete timings, comparison summaries, source/binary hashes, and original-file verification are saved in [the benchmark data](data/raw-library-performance-2026-09-29.json).

## Validation

- 24 RAW unit tests pass, including exact planned/scalar CFA comparisons, all Bayer phases, orientation, malformed input, cancellation, camera-preview reuse, sensor preservation, and tiled output.
- 66 Library/RAW UI tests pass, including the new display-equivalence and canceled-preview source-retention tests, recipes, transparency, save/reopen, undo, thumbnails, selection, and exports. The optional local-camera test remains ignored in this suite.
- Six targeted decoder/sidecar/export integration tests pass. The final UI suite also exercises export after the demosaic change.
- `cargo clippy --locked -p emulsion-io -p emulsion-ui --all-targets -- -D warnings -A dead_code` passes. The exception is for the existing platform-specific dead-code warnings.
- The Library import test initializes its isolated catalog before showing the workspace, avoiding a race with the default catalog loader. Its original assertions remain intact.

## Measurement method

[`raw_pipeline_bench`](../../../crates/emulsion-io/examples/raw_pipeline_bench.rs) uses the same `PhotoSource` APIs as the Library. It measures loading, the first fit preview, six cached edit cases (exposure, white balance, tone, color, rotation, neutral), display conversion, and full-resolution development. Every raster is hashed as dimensions plus exact linear RGBA16 pixels. Hashing happens outside the stage timers. Each edit has one warmup followed by timed samples. The original is hashed again after the run.

The preserved baseline executable comes from the working tree before these RAW changes, including the Library work already present. The first exploratory pass measures decoder reuse, preview reduction, and full-resolution conversion. Final paired runs alternate baseline and optimized executables per photo, reverse file order on alternate runs, and use three runs with seven timed samples per edit case. The runner waits for active Rust compilers before starting each timed process. Binary SHA-256 hashes identify the measured builds.

The OS file cache is warm, because source verification reads the file before the load timer. “First preview” means a cold in-memory camera-preview cache, not a cold disk. Figures cover CPU processing, not native GPU presentation, UI frame rate, disk-cold opening, thumbnail-grid scrolling, or end-to-end input latency.

[`library_preview_bench`](../../../crates/emulsion-io/examples/library_preview_bench.rs) compares the previous document/compositor roundtrip with the actual dependency-free display helper included from the UI source. It uses real developed previews, alternates old/new order, excludes buffer preparation, and asserts identical BGRA output. This isolates the UI's CPU display preparation, not GPUI rendering.

## Reproduction

Build and preserve the baseline before changing the implementation. Build the optimized version with:

```powershell
cargo build --locked --release -p emulsion-io --example raw_pipeline_bench --example library_preview_bench
Copy-Item target/release/examples/raw_pipeline_bench.exe target/performance/raw-pipeline/pass4.exe
python scripts/bench-raw-pipeline.py --binary target/performance/raw-pipeline/pass4.exe --reference target/performance/raw-pipeline/baseline.exe --photos target/performance/raw-pipeline/photos --output target/performance/raw-pipeline/pass4-paired --runs 3 --samples 7 --wait-for-builds
python scripts/compare-raw-pipeline.py target/performance/raw-pipeline/pass4-paired/reference target/performance/raw-pipeline/pass4-paired/candidate --output target/performance/raw-pipeline/final-comparison.json
```

The comparison command fails if photo sets, source hashes, dimensions, first-preview pixels, edited pixels, or full-resolution pixels differ. Raw photos and executable snapshots remain under ignored `target/` storage.

## Additional pass 5: RGB buffer ownership (2026-09-30)

The additional pass removes redundant full-sensor copies after our demosaicer. It now produces owned RGB triplets directly and transfers them into the development pipeline. Previously the handoff flattened or repackaged the data through a cloned `RawImage`, and rawler cloned the RGB data and collected another RGB buffer before cropping. The common path now avoids that roundtrip, releases the normalized sensor buffer before cropping, and copies cropped rows in parallel with cancellation checks. An uncropped image transfers its existing allocation directly. The AI sensor-denoise result uses the same RGB handoff; model inference is unchanged.

Default-crop precedence, active-area fallback, sensor-relative offsets, and the existing dimension comparison are preserved. A regression test compares exact floating-point RGB output and dimensions against rawler's previous `CropDefault` implementation for no crop, a full-frame crop, offset active/default crops, and a single edge pixel. It also verifies allocation reuse and cancellation. Legacy demosaicing, rotated Fuji sensors, and other fallback formats continue through rawler.

Validation: all 25 RAW unit tests and six decoder, sidecar, saved-edit roundtrip, and export integration tests pass. The optional local-Nikon integration test remains ignored. `cargo clippy --locked -p emulsion-io --all-targets -- -D warnings -A dead_code` passes, retaining the existing platform-specific dead-code exception.

This comparison uses the preserved **pass 4 optimized executable** as the reference, so its gains are additional to the four passes above. The workload and measurement method are unchanged: all 22 files, three alternating runs per build, seven timed samples per cached edit after warmup, and exact RGBA16 output hashes. These are warm-file-cache CPU measurements; native UI frame rate and AI inference are not measured.

| CPU stage | Pass 4 | Pass 5 | Median paired time reduction |
| --- | ---: | ---: | ---: |
| Load + first fit development | 495.0 ms | 387.7 ms | 22.6% |
| First fit development alone | 279.6 ms | 175.8 ms | 38.1% |
| Full-resolution development | 357.0 ms | 251.9 ms | 30.9% |
| Cached exposure edit | 5.98 ms | 6.07 ms | -2.2% |
| Cached white-balance edit | 5.95 ms | 6.06 ms | -1.7% |
| Cached tone edit | 16.38 ms | 16.42 ms | -2.0% |
| Cached color edit | 6.37 ms | 6.32 ms | 0.4% |
| Cached rotation | 9.63 ms | 9.72 ms | -2.0% |
| Cached neutral development | 5.93 ms | 5.95 ms | -0.2% |

Before/after columns are medians across per-file medians; reductions are medians of paired per-file ratios. Negative reductions indicate measured slowdowns. Cached edits remain broadly flat and do not benefit from the changed sensor-to-RGB handoff. The small slowdowns are retained in the results rather than excluded. File loading alone is also essentially unchanged (212.3 → 209.4 ms, paired reduction 0.4%).

LandscapeRaw load + first fit improves from 486.5 to 380.7 ms; full development improves from 352.3 to 244.6 ms. PortraitRaw load + first fit improves from 799.2 to 605.5 ms; full development improves from 618.3 to 422.6 ms. All 22 photos improve in median first-fit and full-development time.

All 132 measured processes agree on source hashes, output dimensions, first-preview pixels, six cached edit outputs, and full-resolution pixels. Original photos and workspace copies were rehashed against the previously verified manifest. Full timings, executable and source hashes, and per-photo comparisons are preserved in [the pass 5 data](data/raw-library-performance-pass5-2026-09-30.json). The benchmark waited when unrelated Rust compilation was active; every run, including timing outliers, is retained.

```powershell
cargo build --locked --release -p emulsion-io --example raw_pipeline_bench
Copy-Item target/release/examples/raw_pipeline_bench.exe target/performance/raw-pipeline/pass5.exe
python scripts/bench-raw-pipeline.py --binary target/performance/raw-pipeline/pass5.exe --reference target/performance/raw-pipeline/pass4.exe --photos target/performance/raw-pipeline/photos --output target/performance/raw-pipeline/pass5-paired --runs 3 --samples 7 --wait-for-builds
python scripts/compare-raw-pipeline.py target/performance/raw-pipeline/pass5-paired/reference target/performance/raw-pipeline/pass5-paired/candidate --output target/performance/raw-pipeline/pass5-comparison.json
```
