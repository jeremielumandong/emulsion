# RAW Library performance passes

The workload is all 22 RAW files supplied in `LandscapeRaw` and `PortraitRaw`: 15 Canon CR2 files at about 20 MP, one 24 MP DNG, four Canon CR3 files (three about 45 MP and one 24 MP), and two 24 MP Nikon NEF files. One CR2 is a duplicate. Benchmarks operate on workspace copies; no originals, sidecars, or saved edits are written. SHA-256 checks verify the copies against the originals.

## Changes

1. **Loading and first development:** reuse one mapped RAW container and decoder for metadata and sensor decoding. Preserve both content-verification hashes, pre-decode dimension checks, memory reservations, and panic containment. Reduce camera samples into the fit-preview cache in parallel by output row, preserving each pixel's accumulation order. Parallelize full-resolution camera-to-working-color conversion.
2. **Sensor and edit processing:** parallelize sensor normalization by row with cancellation and finite-value checks. Prepare recipe powers and inactive color/calibration flags once per render, preserving the existing scalar arithmetic and full-resolution export path.
3. **Library display and cancellation:** send already-developed opaque display pixels directly to GPUI after RGBA-to-BGRA conversion. Retain the existing compositor path for recipes and transparency. Keep a successfully decoded source when a newer slider revision cancels its preview, provided the same photo is still selected. Stale images and settings remain rejected.
4. **Demosaic neighborhoods:** precompute the ordered, color-filtered neighborhood for each repeating CFA phase. Interior pixels reuse offsets, spatial distances, and same-color gradient eligibility. Image borders retain the original clamping logic; accumulation order and pixel math are unchanged. Exact scalar comparisons cover all four Bayer phases, a 6×6 CFA, odd dimensions, and small images.

These are normal production paths, with no experimental feature flag or reduction in preview resolution.

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
