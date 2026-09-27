# Stage 2: the engine in `emulsion-ui`, and what is left

*State as of 2026-09-26, measured on an Intel Iris Plus G7 (ICL GT2), Vulkan
via Mesa 26.2.2, Wayland. Read `README.md` and `RESULTS.md` first; this
covers only the integration into the shipping app.*

*Windows follow-up, 2026-09-26: the build, embedded scenarios, live editor
workflows and frame-pacing checks below passed on the RX 7700 XT at `96bc42a`.
See [Stage 2 Windows validation](RESULTS.md#stage-2-windows-validation).
The other engineering tasks remain open.*

Stage 1 asked whether an owned wgpu + Vello canvas beats GPUI painting.
Stage 2 puts that engine behind the real canvas. It runs, it draws, and it
edits; the list below is what stands between that and a canvas anyone can be
handed.

## What landed

- **`crates/emulsion-engine`** — the spike's engine as a library: `gpu`,
  `atlas`, `canvas`, `compositor`, `cache`, `vector`, `brush`, `engine`.
  The spike depends on it, so its benchmarks remain the regression test.
- **`emulsion_engine::host`** (feature `gpui`) — the three hosting backends,
  shared by the spike and the app: GPUI's own wgpu device on Linux, an
  IOSurface on macOS, a shared D3D12 resource on Windows.
- **`crates/emulsion-ui/src/viewport_gpu.rs`** — the canvas path. It owns the
  engine, paints its texture into the GPUI scene, and refuses whenever it
  cannot reproduce the CPU result, in which case `viewport` takes over. A
  refusal is sticky, so a fallback costs one frame.
- **Document sync by tile identity** — `Canvas::signature` separates the
  document's structure from its pixel content. When only pixels in direct
  sources changed, `Engine::reload` swaps those rasters instead of rebuilding
  the program.

**On by default** where a hosting backend exists. `EMULSION_GPU_CANVAS=0`
forces the CPU tile path.

### Measured, on the 4K 25-layer test document

| | |
|---|---|
| Reload after a stroke commit | 1.5 ms (was 74 ms) |
| Reload, nothing changed | 0.04 ms |
| Full compile from scratch | 210–250 ms |
| Reload on a masked or placed layer | ~67 ms — see task 2 |
| Document-sized text rasterise | 12.1 ms — see task 5 |
| GPU textures, 4K document | 1301 MiB |

## Pending

### Blocking

**1. Canvas chrome is missing while the engine paints.** `viewport::paint` is
skipped entirely, so the pixel grid, the compare wipe and the stage plate and
hairline do not draw. Either teach `viewport::paint` to draw chrome without
its tile images, or move that chrome into the overlay pass that already runs
after the canvas element. Most visible regression; hit first.

**2. Painting on a masked or placed layer costs ~67 ms per frame.** Those
compile to baked sources, whose pixels are a function of the mask and
placement rather than the raster alone, so `Canvas::replace_raster` cannot
swap them and the whole program rebuilds. Fix: re-bake the one changed node
and replace that source. `BakeKey` in `canvas.rs` already records what a bake
depends on. Ordinary pixel layers take the 1.5 ms path.

**3. macOS still needs validation; Windows is validated.** The extracted
engine builds on Windows with `--features gpui`, as do the spike and app.
The RX 7700 XT passes the embedded scenarios and live editor checks below.
WARP also renders the saved test project through a matching D3D12 software
adapter on this machine. This does not establish support for every Windows
driver or complete the macOS checks.

**4. Device loss is not handled.** `gpui_wgpu::shared_gpu().generation` exists
for it and nothing reads it. A GPU reset leaves the canvas dead until
restart. One was triggered during stage 1 by a long Vello dispatch, so this
is not hypothetical.

### Performance

**5. Building the composite tree rasterises vector layers.** *(done)*

The eager half is fixed: `VectorRaster` in `emulsion-core` renders a text or
path layer's pixels on first use, so a transform only records what they
should be. Measured on a 3840×2160 document with one text layer, transforming
went from **12.1 ms to 0.00 ms**.

`NodeContent::Pixels` now holds a `LazyRaster` rather than an `Arc<Raster>`,
so building a composite tree records how to make a vector layer's pixels
instead of making them. It carries the size and an identity, so a caller can
measure and key on content it is not going to draw. The CPU compositor
renders them where it samples them, in `composite`; the GPU canvas, drawing
the vector with Vello, never asks.

Measured end to end on a 3840x2160 document with one text layer: transform
**12.1 ms -> 0.00 ms**, composite tree after a transform **10.94 ms ->
0.00 ms**.

What the lazy cache already buys: transforms whose result is never displayed
cost nothing -- MCP and batch operations, undo and redo chains, documents
loaded but not shown, and every intermediate step of a multi-command edit.
Loading an ORA no longer rasterises its vector layers up front either.

**5b. The original diagnosis, for reference.**
`emulsion-core/src/transform.rs:189` calls `text::rasterize(&updated, w, h)`
on every transform, where `w, h` are the document's dimensions; paths do the
same a few lines above. Measured 12.1 ms for modest text at 3840×2160, rising
with glyph coverage, which is why enlarging makes a drag worse. **Pre-existing
and shared with the CPU canvas** — the GPU canvas does not cause it, but it
pays it for nothing, since Vello draws from the spec and never reads that
raster. Agreed fix is to make the cache lazy: invalidate on transform,
rasterise on first read. Roughly 177 call sites read `cache: Arc<Raster>`
across `emulsion-core`, `emulsion-io`, `emulsion-mcp` and `emulsion-ui`, so
it needs its own session and tests over save, load and export — a missed
reader renders blank rather than failing.

**6. Structural vector changes re-encode every object.**
`VectorLayer::resync` keeps the Vello renderer and reuses unchanged
fragments, so a move re-encodes one object. A structural change still walks
them all; `VectorLayer::edit` exists for finer updates.

**7. The GPU brush is not wired in.** Strokes still stamp on the CPU and
upload dirty tiles. Brush B — dabs drawn straight into the atlas, read back
once at stroke end — measured 2.3–2.4× better input-to-pixel in both hosts
and is the largest interactive win still unclaimed.

### Correctness

**8. Unsupported features fall back silently.** Adjustment layers, layer
styles, advanced blending, Dissolve and masked vector nodes are not
implemented; `Canvas::unsupported` lists them and the canvas refuses, so the
document renders on the CPU path. Correct, but a document can quietly get the
other renderer with no indication.

**9. Translucent overlapping vector content shifts colour.** Vello blends
within a run in sRGB while Emulsion blends in linear light: 3.25% of pixels
over one 8-bit code on `vectors-500`, reproduced on Intel to within 0.02
points of lavapipe. `RESULTS.md` lists the three options; none chosen.

**10. Memory.** 1301 MiB of textures for a 4K document, on an integrated GPU
where that is system memory, on top of the CPU's own copy.

**11. Stage-2 results — Windows report added.** `RESULTS.md` now records the
Windows validation and paced comparisons, with raw data and an editor
screenshot. A complete shipping-editor latency benchmark is still outstanding;
the reported frame/latency numbers are from the spike hosts.

## For the Windows session

Completed on 2026-09-26 at `96bc42a`:

- [x] Release-build `emulsion-engine` independently with `--features gpui`.
- [x] Release-build the spike and editor.
- [x] Run navigate, brush A, brush B and vector-edit inside GPUI and with
  standalone D3D12 vsync at an actual 1600×1000 surface.
- [x] Start the editor with `EMULSION_GPU_CANVAS` unset and confirm the engine
  activates on the RX 7700 XT.
- [x] Exercise brush, eraser, undo/redo, bucket fill, pen and committed text
  in a disposable document; save the result as an ORA project.
- [x] Run 29 targeted headless UI tests for painting, pen, text and canvas
  invalidation. These test editing behavior separately from GPU presentation.
- [x] Reopen the project under Windows WARP and check the explicit CPU-canvas
  override (`EMULSION_GPU_CANVAS=0`).
- [x] Record measurements and limits in `RESULTS.md`.

Both brush pipelines are tested in the **spike**. The editor still uses CPU
stamping; task 7 remains open. No Windows backend changes were needed.

The backend is `mod backend` under `#[cfg(target_os = "windows")]` in
`crates/emulsion-engine/src/host.rs`: the engine renders into a shared D3D12
resource that GPUI's DirectX renderer paints, via
`gpui_windows::SharedTexture`.

```powershell
cargo build --release -p emulsion-engine --features gpui
cargo build --release -p vello-canvas-spike
cargo build --release -p emulsion-app

# The engine is on by default; this forces the CPU canvas for comparison.
$env:EMULSION_GPU_CANVAS = "0"
```

What to establish, in order:

1. **It compiles.** `emulsion-engine` with `--features gpui` is the new thing;
   the backend code itself is unchanged from when it was in the spike.
2. **The spike still runs**: `vello-canvas-spike view <file> --gpui`, and
   `bench navigate|brush-a|brush-b|vector-edit ... --gpui --json`.
3. **The app runs with the engine**, and the four scenarios behave: paint with
   both brushes, bucket fill, pen, and the type tool. Each of those broke on
   Linux for a different reason and each is now covered by a test that fails
   without its fix — if one misbehaves on Windows, it is a new bug, not a
   regression of those.
4. **Frame pacing.** On Linux, GPUI drives the canvas through
   `request_animation_frame`, so frame time quantises to the refresh interval
   and sub-refresh headroom is not observable. Worth checking whether DirectX
   presents the same way, since it changes how any Windows timing is read.

Run the standalone benchmarks with `--vsync` as well: paced against paced is
the only like-for-like comparison, and the Iris numbers in `RESULTS.md` show
why the free-running ones mislead.

## Watch out

- `--size` is a request. A tiling window manager overrides it and the engine
  renders at the laid-out bounds, so reports record the size actually
  rendered. Compare those, not the flag.
- Reload logs which path it took and its timings at `debug`; the UI logs any
  engine frame over 8 ms with a full breakdown. `RUST_LOG=emulsion_ui=debug,emulsion_engine=debug`.
- A stale installed build cost an hour of false debugging on Linux. Check the
  binary you are running is the one you just built.
