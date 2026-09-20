# GPU image processing

Emulsion uses two graphics paths: GPUI draws the interface and presents images;
`emulsion-gpu` optionally computes document pixels using wgpu. The compute device
starts in the background. Editing remains available before initialization and
when no compatible compute device exists.
The desktop editor and MCP server both initialize this optional backend.

This is a hybrid pipeline, not a fully GPU-resident editor. Canonical document
tiles, undo, saving, layer source sampling, and brush dab accumulation remain on
the CPU. Compute jobs upload inputs and download results before committing them.

## Coverage

| Operation | Implementation and default |
| --- | --- |
| Interface and tile presentation | GPUI graphics backend; existing software fallback on Linux/Windows |
| Layer/group compositing | GPU kernels for every blend mode, masks, clipping, and prepared adjustments; automatic routing selects expensive compositions |
| Filters | GPU kernels for all current filter variants; automatic routing selects positive-strength Reduce Noise at 512×512 pixels or larger |
| Brush final composition | Experimental GPU normal/behind/multiply, erasing, clipping, alpha lock; CPU by default |
| Rotated/magnified nearest-pixel viewport | Experimental GPU sampling with exact CPU correction near pixel boundaries; CPU by default |
| Brush dabs, wet sampling, clone, complex tips/grain, selections, transforms, import/export, history | Existing CPU algorithms |

Kernels cover filter families including blur, sharpening, edge detection, noise,
emboss, distortion, and lens correction. Large or malformed jobs still fall back:
for example Lens Blur radii above 32, Motion Blur distance above 500, and jobs
exceeding buffer or memory limits. Compositing is bounded to 64 nodes, depth 16,
512 commands, and 64 MiB of prepared source data. Each dispatch is bounded to
256 MiB for input/output/readback buffers and 384 MiB including transient upload
staging, subject to tighter device limits. Busy tile
preparation uses the parallel CPU renderer instead of queuing unbounded uploads.
For measured dense four-tile brush jobs, the context can retain one completed
compute workspace of up to 64 MiB for reuse;
larger jobs are released after completion. Retained buffers do not accumulate
per shader or per stroke, and count against the memory budget when another job
runs between brush updates.

## Performance routing

Release benchmarks on Intel Iris Plus Graphics measured complete preparation,
dispatch, and readback, with CPU/GPU output comparisons:

| Workload | Result |
| --- | --- |
| Eight complex blend layers | GPU about 2.2–2.4× faster |
| Eight adjustments | GPU about 6–11× faster |
| One Hue/Saturation adjustment | GPU about 1.5–2× faster |
| Reduce Noise, 512² and 2048² | GPU about 2× faster |
| Normal-only layers, single exposure LUT, simple filters | CPU generally faster |
| Brush final composition, 4–16 tiles | CPU remains faster; see subsequent [brush timings](gpu-brush-performance.md) |

These measurements inform conservative defaults; they are not guarantees for
other hardware. Normal/LUT-only compositions use CPU. GPU eligibility considers
costly adjustments and the density of expensive blends relative to source
layers. Cheap filtering stays on CPU. The viewport kernel is opt-in until
end-to-end UI latency is established. Brush accumulation and direct GPU texture
presentation need further work to remove repeated transfers before GPU painting
can become the default.
See [brush performance investigation](gpu-brush-performance.md) for subsequent
transfer optimization and the proposed persistent dab-accumulation pipeline.

## Controls

Set before starting Emulsion:

| Variable | Effect |
| --- | --- |
| Unset `EMULSION_GPU` | Select hardware compute and use performance routing |
| `EMULSION_GPU=cpu` | Disable image compute; GPUI presentation is unaffected |
| `EMULSION_GPU=force` | Prefer available hardware compute for supported compositions, filters, and screen sampling, regardless of measured cost |
| `EMULSION_GPU=software` | Force a CPU graphics adapter for shader validation; fails compute initialization if none exists, leaving ordinary CPU algorithms available |
| `EMULSION_GPU_BRUSHES=1` | Also enable experimental final brush composition for eligible batches of 4–32 tiles |

Overrides never bypass correctness checks or device/memory limits. Unsupported
jobs use their complete CPU reference operation. A failed shader is disabled for
the session; a lost device disables compute until restart. CPU document data
remains available. See [VM rendering](rendering.md) for GPUI's independent
`GPUI_FORCE_SOFTWARE_RENDERING` diagnostic.

The wgpu compute code is portable across Linux, macOS, and Windows. Local hardware
validation was on Linux/Intel, with software compute also tested on Mesa llvmpipe;
macOS/Windows runtime performance and pixel parity
still require testing on those platforms. The native macOS GPUI renderer remains
unchanged; this does not add a macOS software interface renderer.

## Verification

```sh
cargo test -p emulsion-raster -p emulsion-filters --lib
EMULSION_REQUIRE_GPU_TESTS=1 cargo test -p emulsion-gpu --lib
EMULSION_GPU=force cargo run -p emulsion-gpu --example backend_smoke
EMULSION_GPU=cpu cargo run -p emulsion-gpu --example backend_smoke
```

GPU tests otherwise skip when no adapter is available. `EMULSION_REQUIRE_GPU_TESTS=1`
turns that into failure. CI explicitly selects Mesa Lavapipe and runs required
software-compute tests and production-hook smoke checks. Tests cover transparent
pixels, blend spaces, groups, masks, clipping, mip levels, deterministic noise,
all filter variants, viewport sampling, rejected jobs, and device loss. GPU/CPU
results have small floating-point differences; nearest viewport pixels use CPU
correction where necessary rather than moving pixel boundaries.

For repeatable performance measurements, run the ignored release benchmarks
without other GPU workloads:

```sh
EMULSION_REQUIRE_GPU_TESTS=1 cargo test -p emulsion-gpu --release benchmark_ -- --ignored --nocapture --test-threads=1
```


The [brush-specific persistent router](gpu-brush-performance.md#brush-specific-routing)
is available with `EMULSION_GPU_BRUSHES=persistent`. It selects persistent GPU
accumulation for supported large dry round brushes on rasters up to 1024×1024;
small brushes and unsupported settings use CPU. Backend failure replays the
resolved dab journal on CPU, preserving the current raster/history contract.
This mode remains experimental and off by default. Direct GPU presentation and
broader brush support remain future work.
