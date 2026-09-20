# Making GPU brushes faster

The current GPU brush hook accelerates only final composition. CPU code still
builds every dab in `emulsion-raster/src/paint.rs::Stroke::stamp`, including up to
16 antialiasing samples per edge pixel. Moving only the cheap final blend to GPU
does not remove this work.

The original full-tile hook uploaded 24 bytes of accumulated paint, 8 bytes of
base color, and 4 bytes of selection coverage per pixel, then downloaded 8 bytes.
Four 256×256 tiles therefore transferred 11 MiB per update, excluding additional
CPU copies and GPUI image uploads. This also paid for pixels with no paint.

The transfer optimization in `emulsion-gpu/src/paint.rs` packs only pixels with
nonzero stroke coverage for sparse batches and reconstructs the result over the
original base. Dense batches retain contiguous packing to avoid scatter overhead.
Sparse uploads omit the unused thickness channel. Dense uploads retain the
original six-float layout to avoid extra per-pixel repacking.
Restoring the base, rather than the previous preview, matters when end taper or
QuickShape removes earlier paint. This remains an experimental final-blend path;
it does not move dab generation to GPU or eliminate synchronous readback.

On Linux/Intel Iris Plus, the final isolated release benchmark measured:

| Workload | CPU dab generation | CPU composition | GPU composition |
| --- | ---: | ---: | ---: |
| Sparse, four tiles | 0.49 ms | 1.34 ms | 2.61 ms |
| Dense, four tiles | 2.63 ms | 5.69 ms | 9.23 ms |
| Dense, sixteen tiles | 10.59 ms | 18.47 ms | 45.72 ms |

The earlier full-tile GPU baseline was 10.92, 10.14, and 47.66 ms respectively.
Sparse GPU composition improved about 4×, while CPU composition remains faster
in every measured case. Dense differences are small enough to treat cautiously
across separate runs. Keep the experimental opt-in; do not infer a whole-editor
speedup from this result. Numbers average six warm iterations, include packing,
transfer and tile replacement, and exclude GPUI presentation. CPU dab generation
is listed separately and is still required by both paths.

Hardware and Mesa software parity tests cover ordinary blending, erasing,
clipping, alpha lock, sparse reconstruction, all-zero coverage restoration and
invalid sparse coverage. Strict GPU-crate Clippy checks pass.

## Reusing compute buffers

The compute context now retains one reusable set of input, output and readback
buffers when requested, with a 64 MiB retention limit. Automatic reuse is limited
to dense four-tile brush batches: these improved repeatedly in A/B measurements,
while sparse and sixteen-tile cases did not improve consistently. GPU brushes
still require the existing experimental opt-in. Small growth fits rounded capacities;
large size changes or different binding counts replace the workspace. Small
uncached jobs may run between brush updates without evicting it; retained memory
counts against the next job's allocation budget. The next
dispatch rewrites all input data, binds only its logical ranges, clears output,
and reads only its own result range. Reuse occurs after the previous readback
has completed and its buffer is unmapped. This does not retain stroke pixels as
authoritative GPU state.

The matching bind group is also reused when shader and logical sizes are
unchanged. CPU packing and temporary upload staging still allocate; this change
does not claim a completely allocation-free brush loop. A dedicated regression
test checks changed inputs, shorter bindings, cleared output, pipeline changes,
growth, and eviction after shrinking.

The ignored `benchmark_stroke_buffer_reuse` compares CPU composition, fresh GPU
buffers and reused GPU buffers in the same executable, alternating order across
12 iterations. It changes stroke positions and colors, verifies actual dispatch
and pixel parity, and counts buffer allocations outside the timed comparisons.
Run it alone to avoid other GPU tests disturbing allocation counts or timings:

```sh
EMULSION_REQUIRE_GPU_TESTS=1 cargo test -p emulsion-gpu --release benchmark_stroke_buffer_reuse -- --ignored --nocapture
```

The final same-executable A/B run against the original exact-size allocation
policy measured these mean composition times on Intel Iris Plus:

| Changing-stroke workload | CPU | Fresh GPU buffers | Reused GPU buffers |
| --- | ---: | ---: | ---: |
| Sparse, four tiles | 4.35 ms | 7.63 ms | 7.77 ms |
| Dense, four tiles | 18.12 ms | 29.53 ms | 20.10 ms |
| Dense, sixteen tiles | 42.46 ms | 73.53 ms | 69.00 ms |

Dense four-tile GPU composition was about 32% faster with reuse; CPU remained
faster. Preliminary A/B runs also improved that workload, but sparse and larger
cases varied or regressed. Every reused case made zero new compute/readback
buffer allocations after warmup, compared with 72 for fresh buffers over 12
iterations. This excludes temporary upload staging and CPU allocations.
Compare timings within a run: these changing-stroke tests use different ordering
and warmup from the earlier fixed-stroke benchmark, and absolute times vary with
machine load. No whole-editor frame-rate improvement is claimed.

## Brush-specific routing

Start the app with `EMULSION_GPU_BRUSHES=persistent` to enable the experimental
per-stroke router. `Stroke` decides from actual brush settings, not preset names.
GPU initialization remains asynchronous, so strokes begun before it completes
stay on CPU. `EMULSION_GPU=cpu` always disables GPU compute.

| Stroke features | Backend in persistent mode |
| --- | --- |
| Dry, round, normal-color brush, diameter at least 400 layer pixels, raster at most 1024×1024 | Persistent GPU when available |
| Small brush; eraser, clone or smudge; selection or alpha lock | CPU |
| Grain, texture, wetness, relief, edge darkening, other blends, ellipse or rotated tip | CPU |
| Pressure size/flow, speed thinning, taper, tilt, jitter, scatter, mirror or radial symmetry | CPU |
| GPU unavailable, busy with another persistent stroke, or initialization failure | CPU |

The 400 px threshold is deliberately conservative, based on the measured test
workload rather than a universal crossover. Larger documents remain on CPU until
the engine supports tiled persistent allocation. The opt-in is necessary while
other adapters and real GPUI presentation latency are evaluated.

GPU sessions begin lazily on the first eligible dab. CPU code resolves the path
and records ordered dab geometry, flow and color; the backend receives batches
at preview boundaries. One factory retains at most one GPU stroke (about 40 MiB
maximum), and the recovery journal is capped at 65,536 dabs. Once the cap is
reached, the stroke materializes on CPU and continues there.

An append/readback failure or malformed output discards the backend and replays
the complete resolved journal into CPU accumulation from the original base.
This includes commands already submitted and those awaiting submission. Changes
to brush settings or stroke options similarly switch to CPU. QuickShape replays
on CPU, restoring previously painted tiles; coverage requests reconstruct a CPU
mask. Existing CPU raster transactions continue to own undo, redo and cancel.
Explicit `render_with_compositor` calls select the old composition path and
materialize any persistent stroke first, keeping benchmark control explicit.

The integrated release benchmark (`benchmark_routed_persistent_brush`) includes
lazy GPU setup, CPU path generation, eight incremental previews, immutable
raster updates and finishing the stroke. On the tested Intel adapter, 400 px
strokes averaged 56.65 ms with routing versus 83.08 ms on CPU (about 32% less
time). The 40 px case stayed on CPU in both modes: 15.43 versus 16.43 ms, ordinary
run variance. This is still not a GPUI presentation benchmark or a measurement
on Windows/macOS hardware.

Verification after integration: 86 raster tests and 49 core tests passed;
21 GPU tests passed on hardware and Mesa software rendering (six manual
benchmarks ignored). App/UI compile checks and strict raster/GPU Clippy pass.
New tests cover selection, incremental GPU/CPU pixel parity, partial append and
preview failures, malformed outputs, coverage, brush mutation, QuickShape
restoration and the one-session allocation limit. Device-failure recovery is
injected through the backend contract; no physical GPU reset is required.

The prior `EMULSION_GPU_BRUSHES=1` final-composition experiment remains a separate
mode. Neither experimental mode is enabled by default.

## ArmorPaint-inspired persistent brush experiment

`crates/emulsion-gpu/src/persistent_paint.rs` and its WGSL kernel implement a
persistent paint engine. This is original MIT Emulsion code inspired by the
persistent-paint architecture, not copied ArmorPaint code or an ArmorPaint port.

The session uploads its original base once and keeps accumulated premultiplied
paint and final output on the GPU. Each update uploads only ordered, resolved
48-byte dab descriptors. The GPU calculates circular brush coverage, including
sixteen-sample sharp-edge antialiasing, accumulates pigment in stroke order, and
blends against the original base. Preview downloads RGBA16 pixels. Dimensions
are bounded to 1024×1024, with at most 1024 dabs per submission and about 40 MiB
of GPU buffers per session. The editor adapter allows only one active session
per factory and declines additional strokes to CPU.

The GPU kernel supports dry circular normal-color brushes only. Unsupported
features use CPU through the router above. GPU undo and direct GPUI texture
presentation are not implemented. Invalid input is rejected before dispatch; a
GPU error invalidates the session and the router reconstructs the stroke on CPU.

`persistent_dabs_match_application_brush` compares the kernel against the actual
`Stroke` CPU implementation, covering soft and sharp brushes, subpixel centers,
small tips, clipping at the image boundary, and transparent or translucent bases.
The ignored `benchmark_persistent_application_brush` compares CPU `Stroke`, the
existing optional GPU composition route, and persistent GPU accumulation across
eight successive previews. All paths include dab pixel calculations; GPU paths
include transfer and readback. The existing route can decline small batches.
Persistent setup is measured separately. GPUI presentation is excluded, and the
prototype returns a flat pixel vector rather than updating immutable raster
tiles, so this is an architecture experiment, not an editor latency claim.

```sh
EMULSION_REQUIRE_GPU_TESTS=1 cargo test -p emulsion-gpu --release benchmark_persistent_application_brush -- --ignored --nocapture
```

Two release runs on the Intel hardware adapter measured these mean update
latencies (six measured strokes per run, eight previews per stroke, rotating
backend order):

| Brush diameter | CPU Stroke | Existing GPU route | Persistent GPU prototype |
| --- | ---: | ---: | ---: |
| 40 px | 1.21–1.29 ms | 1.48–1.72 ms | 2.51–2.87 ms |
| 400 px | 11.37–13.22 ms | 16.54–17.94 ms | 2.88–3.01 ms |

The large-brush prototype was 3.8–4.6× faster than CPU in this experiment;
small brushes remained faster on CPU. Persistent session setup additionally
cost 3.0–6.8 ms per stroke, including buffer allocation, pipeline creation and
initial base upload. These timings do not establish a production routing
threshold. Compare complete editor preview latency after integration before
enabling it automatically.

The initial prototype tests compare output within two RGBA16 channel units.
The integrated routing tests above now additionally exercise CPU recovery and
existing history checks; a physical device-loss event remains untested.

## Further persistent brush support

The opt-in router integrates the first bounded dry circular brush path.
Extending it to large documents and more brush types must preserve the current
UI contract and CPU document/history storage:

1. Keep smoothing, pressure, spacing, symmetry and randomness on CPU. Record
   resolved dab commands containing geometry, flow and color, in order.
2. Allocate paint accumulation, base and clipping buffers once per touched tile.
   Upload base and clipping once; upload only newly resolved dabs on later updates.
3. Assign GPU work per tile pixel and apply intersecting dabs in stroke order.
   Preserve the existing falloff and edge sampling; do not approximate brush
   appearance merely to improve timing.
4. Batch pending dabs for each preview rather than dispatching per dab. With
   frame scheduling, retain all input samples but submit at most one preview batch
   per frame; flush the final batch on pointer-up. Blend accumulated paint with
   the base on GPU and download only updated result tiles once per render. Reuse
   bounded staging/readback buffers across updates.
5. Retain the resolved dab journal until commit. On GPU failure, reconstruct CPU
   accumulation from the same base, clipping and commands, then continue editing.

This first stage still reads back RGBA16 tiles for each preview. It removes CPU
per-dab pixel math and repeated paint-buffer uploads without changing the
editor's synchronous `Stroke::render -> Raster -> ReplacePixels` contract.

Do not use this first stage for wet/smudge/clone, textured tips, grain/relief, or
healing. Wet pickup reads accumulated paint synchronously; healing needs CPU
coverage. These require explicit materialization or separate GPU kernels.
Replay and end taper must reset GPU accumulation and dirty previously touched
tiles so removed preview marks are restored. Existing history transactions can
still combine intermediate preview replacements into one undo step.

## Later: preview directly from GPU textures

The larger gain is to keep the preview on GPU and download changed tiles only
for commit, eviction, or CPU consumers. That needs a rendering API change:
GPUI's `Window::paint_image` accepts CPU-backed `RenderImage` frames and uploads
through its atlas. Its `paint_surface` API is macOS-only; it is not a portable
wgpu texture bridge. The compute backend also currently owns a separate device.

A portable solution needs shared rendering resources or explicit texture
interoperability for Linux, Windows, and macOS, with synchronization and device
loss handling. Preview must participate at the correct layer position, including
masks and blend modes; drawing an overlay above the finished document would be
incorrect. Commits must preserve revision checks and prevent stale asynchronous
readbacks from overwriting later edits.

## Evidence and acceptance criteria

Measure complete point-plus-render time, not just kernel execution. Include CPU
dab generation, packing, allocations, uploads, dispatch, downloads, tile
replacement and eventually GPUI presentation. Test sparse and dense strokes,
large overlapping dabs, long strokes, small brushes, selection and alpha lock,
taper/QuickShape restoration, cancel/undo/redo, and device-loss CPU replay.
Enable automatic GPU routing only for workloads with a repeatable end-to-end win.

The wgpu 29 [queue performance documentation](https://docs.rs/wgpu/29.0.0/wgpu/struct.Queue.html#method.write_buffer)
confirms that ordinary writes allocate temporary staging memory on native
platforms and describes reusable staging as an alternative. Persistent buffers
and batching address that overhead; they do not by themselves remove readback.
GPU painting research also uses GPU tile copies to avoid frequent CPU transfers
for undo: [Baxter's dissertation](https://www.billbaxter.com/dissertation/Baxter-dissertation.pdf).

## Open-source references

- GIMP's [3.2.6 release notes](https://www.gimp.org/news/2026/09/10/gimp-3-2-6-released/)
  describe updated GEGL OpenCL support but say it remains disabled by default.
- Krita documents GPU [canvas acceleration](https://docs.krita.org/en/reference_manual/preferences/display_settings.html)
  separately from CPU multithreading and vector optimizations in its
  [painting performance settings](https://docs.krita.org/en/reference_manual/preferences/performance_settings.html).
- ArmorPaint's [manual](https://armorpaint.org/manual) states that painting runs on
  GPU. Its [paint path](https://github.com/armory3d/armorpaint/blob/main/paint/sources/render/render_path_paint.c)
  uses persistent render targets for paint and coverage and binds the layer
  textures for display. Its [history implementation](https://github.com/armory3d/armorpaint/blob/main/paint/sources/history.c)
  uses GPU copies into undo targets and layer swaps. These are useful references
  for the later persistent-texture stage; no upstream code is copied here.
