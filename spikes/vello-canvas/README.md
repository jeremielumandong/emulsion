# Spike: owned wgpu + Vello canvas vs GPUI canvas

Results: [RESULTS.md](RESULTS.md). The brief comes first, then what was
built and how to run it.

## Brief

### Context

Emulsion is a Rust/GPUI Photoshop-style editor. The canvas currently renders
through GPUI, with GPU compute for selected image operations; persistent GPU
brush painting is experimental and opt-in. Brush performance is already decent,
but performance is meant to be the key advantage of Emulsion and of two planned
sibling apps (a Canva-style design editor and a draw.io-style diagram app for
Omarchy). Before committing those apps to a stack, measure whether a render loop
we own (winit + wgpu, Vello for vector content) beats GPUI painting.

The question being answered is **"does an owned wgpu pipeline beat GPUI painting
for our workloads?"**, not "Vello vs GPUI". Vello is a vector renderer; raster
layers go through our own compositor.

### Scope

A standalone binary under `spikes/vello-canvas/`. No GPUI, no changes to shipping
crates. Reuse existing crates as libraries: `emulsion-io` (loading),
`emulsion-core` (document model), `emulsion-raster` (brush stamping), and
existing blend/colour code where possible.

### Build

1. **Window + device:** winit window, one wgpu device/queue/surface, frame loop
   with vsync off option.
2. **Load real documents:** open `.ora` / `.psd` via `emulsion-io`. Include a 4K,
   20+ layer test file.
3. **Raster compositor:** each layer as tiled GPU textures (e.g. 256x256).
   Composite with our own wgpu compute or fragment pass. Blend modes and
   linear-light math must match Emulsion's CPU path.
4. **Vector layer:** draw Emulsion's editable paths and text with Vello into the
   same frame/surface. Cache a Vello scene fragment per object; re-encode only
   edited objects; cull via R-tree (`rstar`).
5. **Brush test A:** stamp dabs on CPU with existing brush code; track dirty
   tiles; upload only those. Render once per frame regardless of input event
   rate.
6. **Brush test B:** stamp dabs directly on GPU into the layer texture; async
   readback at stroke end.
7. **HUD + tracing:** on-screen frame time (p50/p99) and `tracing` +
   `tracing-tracy` spans around input, stamp, composite, upload, encode, present.

### Measure (same files, same machine, spike vs current GPUI build)

- Pan/zoom frame time on 4K, 20+ layer document
- Input-to-pixel latency during a large soft-brush stroke
- Frame time with ~500 vector paths + text boxes
- GPU memory and upload bytes per frame
- Pixel diff of both renders (colour and blend fidelity)

Record results in `spikes/vello-canvas/RESULTS.md` with hardware, driver, and
commit hash.

### Traps to check first

- **Vello image updates:** verify how the pinned Vello version handles per-frame
  changing image data. If it re-uploads whole images, keep raster layers entirely
  in our own textures.
- **Blend colour space:** confirm Vello's blending space vs Emulsion's linear
  compositing; vector content must not visibly shift.
- **Precision:** document raster is 16-bit linear RGB; pick texture formats that
  preserve it (e.g. Rgba16Float) and note the cost.
- **GPUI embedding** is out of scope. Answer "is it faster?" first.

### Decision criteria

- Clearly faster (e.g. >=2x headroom on pan/zoom or brush latency) with matching
  fidelity: plan a shared engine crate (wgpu compositor + Vello vector layer) for
  Emulsion, the design editor, and the diagram app; evaluate egui for UI chrome.
- Marginal: stay on GPUI; port the wins (dirty tiles, below/above caches,
  per-frame render) back.

### Constraints

Follow repo conventions: `cargo fmt`, `cargo clippy -D warnings`, pinned
dependencies in `Cargo.lock`, licence entries in THIRD_PARTY_CRATES.md for any
new crate.

## What was built

A workspace member binary, `vello-canvas-spike`, that depends on
`emulsion-core`, `emulsion-io` and `emulsion-raster` only. It links the same
wgpu (29.0.4) as GPUI; Vello 0.10 is the first release on wgpu 29.

| Brief item | Where | Notes |
|---|---|---|
| 1. Window + device | `app.rs`, `gpu.rs` | winit 0.30, one device/queue/surface; `Immediate` or `Mailbox` present when vsync is off. |
| 2. Real documents | `main.rs`, `testdocs.rs` | Opens anything `emulsion_io::open` does (ORA, PSD, …). `gen` writes the test files below as ORA so the GPUI build opens the same bytes. |
| 3. Raster compositor | `atlas.rs`, `canvas.rs`, `compositor.rs`, `cache.rs`, `composite.wgsl`, `mips.wgsl` | 256² tiles in 2048² array-texture pages with per-tile mip chains; the document runs as an op program, into a GPU tile cache and then one full-screen pass. Blend kernels are spliced in at build time from `emulsion-gpu/src/composite.wgsl`, the GPU path already parity-tested against the CPU. |
| 4. Vector layer | `vector.rs` | One Vello scene fragment per path/text node, encoded once in document space; R-tree per run; edits re-encode one object. |
| 5. Brush test A | `brush.rs` (`CpuStroke`) | Emulsion's own `paint::Stroke`; uploads only tiles whose `Arc` identity changed. |
| 6. Brush test B | `brush.rs` (`GpuStroke`), `brush.wgsl` | Dabs drawn straight into the layer's atlas tiles with hardware blending; async readback into the CPU raster at stroke end. |
| 7. HUD + tracing | `app.rs`, `vector.rs` (`Hud`), `main.rs` | HUD drawn with Vello, refreshed at 4 Hz (off in scripted runs); spans `input`, `stamp`, `upload`, `mips`, `cache_fill`, `encode`, `vello_render`, `composite`, `submit`, `present`, `gpu_wait`. `--features tracy` streams them to Tracy. |

### Compositor

Each document tile (256×256 RGBA16, premultiplied linear) occupies one slot of a
`Rgba16Unorm` 2D array texture: 2048×2048 pages, 64 slots each. A page's mip
chain is a per-tile mip chain for levels 0–8 because tiles are 256-aligned, and
the mip pass rounds exactly like the CPU's `(a + b + c + d + 2) >> 2`. Zoomed-out
views therefore sample the same values as Emulsion's level-k tiles. Tiles are
deduplicated by `Arc` identity, as the CPU planes share them. A shared slot is
copied before GPU painting.

`canvas.rs` turns `Document::composite_tree()` into the op model of
`emulsion-gpu`'s tile compositor: layer, fill, isolated and pass-through groups,
clipping, group masks. Masks and non-identity placements of pixel layers are
baked once on the CPU into document-aligned rasters. Adjustment layers, layer
styles, advanced blending options and Dissolve are not implemented. The spike
lists any it meets as unsupported rather than approximating them. The test files
use none of them.

The screen pass maps each screen pixel to a document pixel. It runs the program
with nearest sampling at the camera's mip level, then applies the checkerboard,
sRGB encoding and HUD.

A GPU composite cache (`cache.rs`) mirrors the GPUI canvas's tile cache. It
holds flattened 256² tiles per level for the program's cacheable prefix: the
leading ops, up to the first Vello run, that don't split a group or a clipping
pair. It fills only visible tiles that are missing. Painting invalidates the
tiles it touched at every level. The screen pass then reads one cache texel and
runs only the remaining ops, usually the Vello runs. `--no-cache` recomposites
every layer every frame, for comparison.

### Vector layer

Consecutive Normal, 100%, unclipped path/text nodes form a run. Vello renders
each run into its own screen-sized `Rgba8Unorm` target. The compositor blends
that target at the run's stack position in linear light, with the node's blend
mode and opacity. Text is shaped with cosmic-text the way `emulsion-core` shapes
it and drawn as Vello glyph runs. Supported: solid fills and centred strokes
(caps, joins, miter limit, dashes); single-style horizontal text without warp,
path or frame height. Anything else composites from its CPU cache raster.

`--vectors srgb` (default) gives Vello sRGB-encoded colours, which is what it
expects. `--vectors linear` gives it linear values, so overlaps blend like
Emulsion, at 8-bit linear precision. See RESULTS.md.

### Brush tests

Both use a 300 px soft round brush (hardness 0, flow 0.35, spacing 0.12), a
scripted 1.5 s S-curve at 1 kHz, and render once per frame whatever the input
rate. Input-to-pixel latency is measured from each input event's scheduled time
to GPU completion of the frame that shows it. Scanout is not included.

- **A** feeds `emulsion_raster::paint::Stroke` and calls `Stroke::render` once
  per frame. Only the tiles in the dirty rectangle whose `Arc` changed are
  uploaded, then their mips are rebuilt on the GPU.
- **B** places dabs with the CPU stroke's spacing rule and draws each dab ∩ tile
  as a quad into the atlas page. `PREMULTIPLIED_ALPHA_BLENDING` performs the
  CPU's `ink = colour·a + ink·(1 − a)` in draw order. The shape and 4×4
  supersampling come from `persistent_paint.wgsl`. At stroke opacity 1,
  stamping into the layer equals accumulating ink and compositing it once. Lower
  stroke opacity would need a stroke buffer and is not implemented. At stroke
  end the painted tiles are copied to a buffer and mapped asynchronously; the
  canvas adopts the result as its CPU raster without re-uploading.

## Running

```sh
# Test documents (git-ignored): layers-4k.ora (3840×2160, 25 nodes),
# vectors-500.ora (480 paths + 20 text boxes), fidelity-{linear,srgb}.ora.
cargo run --release -p vello-canvas-spike -- gen --out spikes/out

# Interactive: drag to paint (b toggles brush A/B), right/middle drag to pan,
# wheel to zoom, 1 = 100%, 0 = fit, e = animate vector edits, h = HUD.
cargo run --release -p vello-canvas-spike -- view spikes/out/layers-4k.ora

# Scripted runs in a window (vsync off). Add --headless to render offscreen,
# or --baseline for the CPU work the GPUI canvas does on the same script.
S=target/release/vello-canvas-spike
$S bench navigate    spikes/out/layers-4k.ora   --json spikes/out/results.jsonl
$S bench brush-a     spikes/out/layers-4k.ora   --json spikes/out/results.jsonl
$S bench brush-b     spikes/out/layers-4k.ora   --json spikes/out/results.jsonl
$S bench navigate    spikes/out/vectors-500.ora --json spikes/out/results.jsonl
$S bench vector-edit spikes/out/vectors-500.ora --json spikes/out/results.jsonl
$S bench navigate    spikes/out/layers-4k.ora --baseline

# Pixel diff against Emulsion's CPU compositor, with heatmaps.
$S fidelity spikes/out/fidelity-linear.ora spikes/out/fidelity-srgb.ora \
   spikes/out/layers-4k.ora --out spikes/out/fidelity

# Tracy: build with the feature and connect the Tracy profiler.
cargo run --release -p vello-canvas-spike --features tracy -- view spikes/out/layers-4k.ora
```

Other options: `--size WxH` (default 1600x1000), `--vsync`,
`--tiles unorm16|float16`, `--vectors srgb|linear`, `--no-vello`,
`--no-cache`, `--frames N`.

`navigate` runs 240 frames of fast panning at 100%, 120 at 50%, and 240 of a
zoom sweep between fit and 400%. `vector-edit` moves one object per frame for
300 frames at fit.

Windowed runs wait for the GPU after each present, so latency is attributable
and frames do not overlap. Headless runs do the same without a present. Both
report whole-frame time (CPU record + GPU) rather than throughput.

`cargo test -p vello-canvas-spike` checks raster composite parity with the CPU
at mip levels 0–2 in both blend spaces, with and without the tile cache. It also
checks GPU dabs against the CPU `Stroke`, and that a stroke invalidates cached
tiles.
The tests skip without an adapter; `EMULSION_REQUIRE_GPU_TESTS=1` makes that
a failure.
