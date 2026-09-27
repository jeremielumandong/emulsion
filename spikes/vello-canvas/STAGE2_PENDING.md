# Stage 2: the engine in `emulsion-ui`, and what is left

*State as of 2026-09-26, measured on an Intel Iris Plus G7 (ICL GT2), Vulkan
via Mesa 26.2.2, Wayland. Read `README.md` and `RESULTS.md` first; this
covers only the integration into the shipping app.*

*Windows follow-up, 2026-09-26: the build, embedded scenarios, live editor
workflows and frame-pacing checks below passed on the RX 7700 XT at `96bc42a`.
See [Stage 2 Windows validation](RESULTS.md#stage-2-windows-validation).
The engineering follow-up below was implemented on `feat/vello-migration-ui`;
these Windows results predate that follow-up.*

**Current follow-up:** tasks 1, 2, 4–8, 10 and 12 have implementation and Linux
regression coverage. Task 11 now has a native EditorView benchmark. Task 3
remains open for macOS validation. Task 9 retains opaque-vector edge differences;
task 13 tracks the frame-pacing issue exposed by native measurement;
the new brush/recovery code also needs a
Windows follow-up. See [platform validation](PLATFORM_VALIDATION.md).

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
  refusal is visible in the status strip and retries after document/device
  changes; transient failures use a two-second backoff on subsequent frames.
- **Document sync by tile identity** — `Canvas::signature` separates the
  document's structure from its pixel content. Pixel edits refresh direct or baked sources without
  rebuilding the program. Content-only vector edits update their fragments.

**On by default** where a hosting backend exists. `EMULSION_GPU_CANVAS=0`
forces the CPU tile path.

### Original baseline, on the 4K 25-layer test document

These figures predate the follow-up below; they are retained as historical context.

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

**1. Canvas chrome is missing while the engine paints.** *(done)*

`viewport::prepaint` takes an `images` flag. With it false it requests no
tiles and returns a plan carrying only chrome; `viewport::paint` splits into
`paint_under` (stage, plate hairline) and `paint_over` (pixel grid, compare
wipe, rulers), so the engine's pixels go between them in the order the tile
path draws them. Verified in the app: rulers, stage and plate render with the
engine drawing the document.

**2. Painting on a masked or placed layer costs ~67 ms per frame.** *(done)*

Pixel edits now refresh the affected baked source without rebuilding the
program or tile-table buffer. Bakes retain their input tiles: masked layers
rebake only document tiles overlapping changed source tiles, including
fractional translations, rotation, anisotropic scale and flips. Damage includes
the source mip footprint and bilinear filtering halo. Unchanged output tiles retain their identity,
atlas slots and composite-cache entries. Erasing and undo/redo use the same path.

Changes to the mask, placement itself, source size or implicit fill retain a
full bake because their damage is not restricted to changed source pixels.
The original ~67 ms figure above predates this change; it is not a current
shipping-editor latency measurement.

Local release microbenchmark on Ryzen 7 8700G (3840×2160 solid layer, uniform mask, one changed
256×256 source tile, seven iterations): full bake **85.316 ms** median;
incremental bake **1.319 ms** median. This isolates CPU baking; it does not
measure total frame time or input-to-pixel latency. Reproduce with:

```sh
cargo test --locked --release -p emulsion-engine benchmark_masked_tile_rebake -- --ignored --nocapture
```

Regression checks compare partial bakes to full CPU renders through edits,
erase, undo/redo, mask/fill/size changes and resampling fallbacks. GPU readback
on Linux/RADV, RX 7700 XT, also checks pixel parity, retained tile tables,
composite-cache invalidation and unchanged reloads.

**3. macOS still needs validation; Windows is validated.** The extracted
engine builds on Windows with `--features gpui`, as do the spike and app.
The RX 7700 XT passes the embedded scenarios and live editor checks below.
WARP also renders the saved test project through a matching D3D12 software
adapter on this machine. This does not establish support for every Windows
driver or complete the macOS checks.

**4. Device loss recovery.** *(implemented; hardware reset validation remains)*

The engine checks device-loss flags before work and rejects shared devices from
an older GPUI generation. Linux shares GPUI’s existing loss flag without replacing
its callback; owned devices install their own callback. The viewport discards
invalid resources, uses CPU rendering, and retries a replacement device.
Live brush points are journaled and replayed on CPU after failure, including
failure after the last input sample. Tests cover owned-device destruction and
recreation, retry/backoff decisions, and stroke replay; they do not simulate a
real OS/shared-device reset.

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

Follow-up: lazy-raster identities now survive rebuilding a composite tree.
Ready pixels identify the retained source raster; deferred vector pixels use
the vector cache's identity. Previously each wrapper allocation appeared to be
new content, defeating no-op reloads and bake reuse. Building signatures still
does not rasterise vector layers.

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

**6. Incremental vector updates.** *(done)*

Content-only edits call `VectorLayer::edit` for the affected objects. Structural
changes still walk the scene to rebuild ordering/culling but reuse unchanged
encoded fragments. Run boundaries are part of the vector signature, so opacity
or isolation changes cannot leave old run targets in use. Tests assert that a
single edit encodes one object and that run topology updates correctly.

**7. GPU brush in the shipping editor.** *(done for compatible brush settings)*

Opaque-color and eraser strokes with the supported round-dab settings stamp
straight into the atlas, flush at frame boundaries, and read back once on commit.
The normal document transaction supplies undo/redo. Save/export materializes
in-progress GPU strokes, and closing detects them as unsaved changes. Cancellation
discards the preview. QuickShape stays on GPU while moving and replays on CPU
when a hold needs shape fitting. Device failure replays the complete journal.

Dynamics, wet/secondary brushes, non-default opacity/blending, selections,
mask painting, transparency locks, symmetry and transformed paint targets retain
the CPU stroke implementation. GPU/CPU color and eraser parity, recovery,
QuickShape, save and cancel have regression coverage. Commit readback is currently
synchronous; it happens once per stroke, not for every input frame.

### Correctness

**8. Visible and recoverable fallback.** *(done)*

The status strip shows **CPU canvas** with the refusal reason, or **Compatibility
rendering** when individual vector appearances use CPU rasterization. Empty/offscreen CPU tile plans no longer accidentally retry the GPU or schedule
endless fallback redraws. Unsupported
features are checked after reload as well as initial compilation. Removing them
or replacing the GPU device permits retry; transient failures back off.

**9. Vector color fidelity.** *(translucent paints fixed; broader parity still open)*

Translucent path fill/stroke and text paints use the reference CPU rasterizer and
linear-light GPU compositor. Opaque supported content remains Vello-rendered.
This avoids Vello's sRGB blending shift without switching the whole document to
CPU compositing. The compatibility badge explains this choice. A GPU readback
test compares overlapping translucent colored vectors against the reference.
This is not a new linear-target Vello implementation: compatibility nodes use
raster pixels at document resolution.

The full original `vectors-500` fixture still differs at 3.250% of pixels by more
than one display code (0.008% off-edge by more than three codes). Inspection of
`testdocs::vectors` shows it generates **opaque** paints, so the earlier tracker
incorrectly attributed that entire figure to translucent paint. The new alpha
fallback cannot fix those opaque edge/overlap differences. Linear 8-bit Vello
targets remain worse (14.152% over one code); switching to that mode is not a
fidelity fix. This item stays open rather than masking the measured result.

**10. Texture memory.** *(allocation improvements and limits implemented)*

Implicit fills are counted once per distinct color when sizing the atlas;
composite cache capacity follows the visible mip-level tile grid (capped at
128 MiB, otherwise direct compositing); raster-only scenes use a 1×1 placeholder
instead of a full-size vector target. Vector targets are bounded to 128 MiB and
atlas textures to 1536 MiB, with device dimension/layer limits checked before
allocation. Oversized documents fall back to CPU rather than trigger a validation
panic. Texture figures exclude CPU document data and driver/Vello internal buffers.
A 30-layer implicit-fill 4K regression fixture stays below 256 MiB. Dense, unique
raster content still needs its atlas storage; this is not sparse GPU residency.

**11. Shipping-editor measurement.** *(native harness implemented)*

`editor_canvas_bench`, behind the `canvas-bench` feature, opens the real EditorView
and exercises pan, brush and editable-text updates. It records renderer/brush
routing, texture bytes, input-to-canvas-submission and the next platform frame
callback. CPU samples wait for current tiles. It uses isolated temporary app data.
These measurements are distinct from physical input-to-photon latency. See
[platform validation](PLATFORM_VALIDATION.md) and the follow-up in `RESULTS.md`.

**12. Copy/paste rasterises text and loses crispness when enlarged.** *(done
for one whole text layer copied within the running app)*

Copy retains the text node alongside the portable PNG clipboard image. When
that image still matches, Paste creates editable text with the original spec
and a destination-sized lazy raster cache, including across tabs. Same-document
paste preserves the exact position; cross-document paste centers it. A pixel
selection or a mixed/multiple-layer copy still copies pixels. External apps
receive the PNG. Native clipboard data does not persist across app restarts.

**13. Native editor frame pacing.** *(open, found by task 11)*

The corrected native benchmark shows substantially lower GPU submission cost on
the original layered fixture (brush p50 33.13 → 3.45 ms, p95 214.81 → 3.86 ms).
However GPU input-to-next-frame-callback remains roughly 38–41 ms, so median
end-to-end brush latency has not been shown to improve. Investigate the
GPUI/compositor scheduling delay and repeat with identical viewport dimensions;
the window manager gave these runs slightly different canvas widths. The raw
reports and limits are in `RESULTS.md`. Do not substitute the earlier undersized
harness numbers or spike-host latency for these shipping-editor results.

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

At that recorded commit, both brush pipelines were tested only in the **spike**.
The shipping-editor GPU brush added in this follow-up needs Windows revalidation.
No Windows backend changes were needed for the earlier validation.

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
- **The GPU canvas is off under `cfg!(test)`.** The unit tests drive GPUI
  headless, with no surface to present and no frame loop, and a canvas that
  renders on the GPU there hangs them: `splash_dismisses_and_the_landing_
  image_opens_for_editing` ran indefinitely instead of its usual 0.19 s. The
  guard is in `viewport_gpu::enabled`. If a test ever needs the engine, it
  needs a real surface, not that flag removed.
- The Linux follow-up full UI run passed 477 tests with one ignored, including
  GPU stroke replay/save/cancel and QuickShape lifecycle coverage. Do not treat failing
  brush-studio tests as automatically acceptable.
