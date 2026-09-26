# Results: owned wgpu + Vello canvas vs GPUI canvas

*2026-09-26. Status: measured on Apple M1 (Metal) and Mesa lavapipe; Intel Iris
Plus run in progress; GPUI build not yet measured directly.*

On the M1, against the CPU work the GPUI canvas does for the same scripted
input:

- **GPU brush stamping (test B) clears the brief's ≥2× bar on input-to-pixel
  latency:** 7.1 / 19.4 ms p50 / p99 against 17.4 / 39.0 ms.
- **Stamping on the CPU and uploading dirty tiles (test A) gains only
  1.2–1.3×.**
- **Navigation doesn't clear 2×.** A warm tile cache makes panning cheap on
  both paths; the spike's p99 is 1.7× better at 100% pan.
- **Raster fidelity matches** on Metal as on lavapipe.

The GPUI column leaves out GPUI's own layout, atlas upload and present, so it
understates GPUI's frame times. Where the spike is faster, the ratios are lower
bounds.

## Machines

The M1 and Iris Plus sections list their own hardware. The lavapipe smoke run
and fidelity baseline ran here:

| | |
|---|---|
| CPU | Intel Xeon @ 2.80 GHz, 4 vCPU, 15 GiB (cloud container) |
| GPU / driver | llvmpipe (LLVM 20.1.2, 256 bits), Mesa 25.2.8 lavapipe, Vulkan backend, wgpu 29.0.4 |
| Display | Xvfb 21.1.12 for windowed runs; otherwise headless |
| OS / toolchain | Ubuntu 24.04, kernel 6.18, Rust 1.98.1, release profile |
| Code | base `cce707c` plus this spike (the JSON records the base hash); the committed code is what ran, apart from RESULTS.md and README.md |
| Raw data | [`results/bench-lavapipe.jsonl`](results/bench-lavapipe.jsonl), [`results/fidelity-lavapipe.jsonl`](results/fidelity-lavapipe.jsonl) |

Test files come from `vello-canvas-spike gen`:

- `layers-4k.ora`: 3840×2160 with 25 nodes: a background, 8 full-canvas
  washes, and 12 soft blobs in 12 blend modes. It also has an isolated group
  at 80%, a pass-through group, a clipped layer, a masked layer and a Multiply
  fill.
- `vectors-500.ora`: 480 editable paths and 20 text boxes over a paper
  raster.
- `fidelity-{linear,srgb}.ora`: 1024×768, every blend mode, a group with a
  clip, a mask and a placed layer. It has 15 opaque and 15 translucent paths
  and 6 text boxes, in each blend space.

## Traps

### Vello image updates: keep raster layers out of Vello

Checked in the pinned `vello 0.10.0` / `vello_encoding 0.10.0` source:

- Images live in a single `Rgba8` atlas. It starts at 1024², grows up to 8192²,
  and is allocated with guillotiere. An `ImageData` is cached by blob id, so
  changing pixels means a new blob and a whole-image `write_texture`.
- `Renderer::register_texture` / `override_image` avoid the CPU copy but
  require `Rgba8Unorm` with straight alpha. The *whole* texture is copied into
  the atlas each time it is marked dirty.
- The 4K test document holds 22 raster layers × 8.3 MP, far more than one
  8192² atlas (67 MP). Oversized images silently draw nothing ("failed to
  allocate a slot").

So raster content cannot go through Vello at 16 bits, per tile, or at this
size. The spike keeps every raster layer in its own `Rgba16Unorm` tile atlas,
and Vello draws only paths and text.

### Blend colour space: Vello shifts translucent vector content

Vello anti-aliases and blends in whatever numeric space its colours are in. It
encodes solid colours as premultiplied 8-bit and writes straight-alpha
`Rgba8Unorm`. Emulsion blends in linear light at 16 bits. With sRGB-encoded
input, which Vello expects, a single opaque object decodes to exactly
Emulsion's `coverage × linear(colour)`. Anything that blends *inside* a Vello
run happens in sRGB space instead: a stroke over its own translucent fill, or
translucent objects overlapping. Passing linear values (`--vectors linear`)
moves those blends into linear light, but stores colour at 8-bit linear.

| Vector content (level 0) | Mode | >1 code | >3 codes off-edge |
|---|---|---|---|
| 480 opaque paths + 20 text boxes | sRGB-encoded | 3.27% (edges) | **0.008%** |
| 480 opaque paths + 20 text boxes | linear 8-bit | 14.19% | 3.23% (dark tones band) |
| Fidelity sheet, half translucent | sRGB-encoded | 5.74% | 1.40% |
| Fidelity sheet, half translucent | linear 8-bit | 7.74% | 0.98% |

At an overlap of two translucent paths, one probe reads 0.162 linear red from
the GPU against 0.199 from the CPU in sRGB mode, about 10 display codes. Linear
mode reads 0.196. The interiors of single translucent fills are off by 1–2 codes
from Vello's 8-bit premultiplied colour. Glyph and path edges differ by up to a
full contrast step on a pixel's partial coverage. That's expected when tiny-skia
or swash is compared with Vello's analytic coverage, and the edge column separates
those pixels out. One dashed path also starts its dash pattern at a different
phase (kurbo vs tiny-skia).

**Conclusion.** Keep sRGB-encoded input: opaque vector content doesn't visibly
shift. Translucent or overlapping vector content does shift. A shared engine
would need one of these:
- accept sRGB-space blending within vector runs, as most design tools do;
- give each translucent object its own run (one Vello render each);
- wait for Vello to render into 16-bit float targets.

### Precision: `Rgba16Unorm`, not `Rgba16Float`

Rendering and blending into `Rgba16Unorm` requires `TEXTURE_FORMAT_16BIT_NORM`
plus `TEXTURE_ADAPTER_SPECIFIC_FORMAT_FEATURES`. Lavapipe reports both, and
Vulkan, Metal and DX12 desktop drivers generally do. `Rgba16Unorm` holds the
CPU's `[u16; 4]` tiles bit-exact, uploads them with a plain memcpy, and gives
mips the CPU's `(a+b+c+d+2)>>2` rounding. `Rgba16Float` costs the same 8 B/px
but loses precision near 1.0 and needs a CPU conversion on every upload and
readback:

| Case | `Rgba16Unorm` | `Rgba16Float` |
|---|---|---|
| Fidelity sheet, linear blending, levels 0 / 1 / 2 | max 1 / 1 / 1 code | max 1 / 39 / 71 codes |
| Fidelity sheet, sRGB blending, level 0 | max 1 code | max 107 codes on 0.009% of pixels (burn/dodge amplify f16 steps near 1.0) |
| GPU dabs vs CPU `Stroke` | max 5/65535, ≤1 code | max 242/65535, 2 codes |

The spike picks `Rgba16Unorm` when the adapter allows it (`--tiles` overrides).

**Memory cost.** The 4K test document is 1454 unique 256² tiles. Its atlas is
26 pages of 2048² with per-tile mips: 1115 MiB, of which 727 MiB is level-0
pixels. The composite cache adds 192 MiB (352 tiles). The CPU keeps its own copy
of the document. On an integrated GPU like the Iris Plus, video memory is system
memory, so a GPU-resident document roughly doubles its memory footprint.

### GPUI embedding

Out of scope, as briefed. The spike links the same wgpu 29.0.4 as GPUI, so
sharing GPUI's device remains possible.

## Fidelity against the CPU compositor

This compares the spike's composite, read back as premultiplied linear f32,
with `emulsion_raster::composite::render_tile_cpu`, the pixels the GPUI canvas
presents. Display codes are 8-bit sRGB over mid grey. "Edge px" counts pixels
where the CPU image varies by more than 24 codes in a 3×3 neighbourhood.
"Off-edge" counts the remaining pixels that differ by more than 3 codes from
every CPU pixel within one pixel.

| Case | Max linear | Max code | >1 code | >3 codes off-edge |
|---|---|---|---|---|
| layers-4k, raster, direct, levels 0 / 1 / 2 | 7.6e-6 / 1.2e-3 / 1.7e-3 | 1 | 0% | 0% |
| layers-4k, raster, via tile cache, levels 0 / 1 / 2 | 1.5e-5 / 1.2e-3 / 1.7e-3 | 1 | 0% | 0% |
| fidelity-linear, raster, direct and cached, levels 0–2 | ≤ 2.5e-3 | 1 | 0% | 0% |
| fidelity-srgb, raster, direct and cached, levels 0–2 | ≤ 2.5e-3 | 1 | 0% | 0% |
| vectors-500 background via cache, levels 0–2 | 7.7e-6 | 1 | 0% | 0% |

Raster compositing matches at every level: every blend mode in both blend
spaces, groups, pass-through, clipping, masks and a placed layer. The spike
splices in `emulsion-gpu`'s parity-tested blend kernels, and its per-tile mip
chains round like the CPU. Zoomed-out differences up to 2.5e-3 linear come from
baked masks and placements being mipped after baking, and stay within 1 code.
`cargo test -p vello-canvas-spike` asserts these bounds and runs in CI on
lavapipe. The layer-style, adjustment-layer and advanced-blending paths are not
implemented and are not in the test files.

## Apple M1 (Metal)

Jeremie's MacBook Pro: Apple M1, Metal backend, `Rgba16Unorm` tiles. Windowed
runs at 1600×1000, vsync off, commit `024a410`. Raw data:
[`results/bench-Jeremies-MacBook-Pro.local.jsonl`](results/bench-Jeremies-MacBook-Pro.local.jsonl),
[`results/fidelity-Jeremies-MacBook-Pro.local.jsonl`](results/fidelity-Jeremies-MacBook-Pro.local.jsonl).
All three GPU tests pass with `EMULSION_REQUIRE_GPU_TESTS=1`.

Frame time is serialised: CPU record, GPU, present, then a wait for the GPU.
The GPUI column is the same script through the GPUI canvas's CPU raster work.

| Workload | Spike p50 / p99 | GPUI-path CPU work p50 / p99 (mean) | Spike advantage |
|---|---|---|---|
| Navigate layers-4k, all 600 frames | 3.61 / 11.50 ms | 0.00 / 17.12 ms (0.67) | p99 1.5× |
| — pan 100% | 2.55 / 12.30 ms | 0.00 / 20.62 ms (1.25) | p99 1.7× |
| — pan 50% | 2.65 / 10.39 ms | 0.00 / 6.57 ms (0.75) | none |
| — zoom sweep fit↔400% | 3.99 / 8.65 ms | 0.00 / 0.00 ms (0.05) | none |
| Navigate layers-4k, `--no-cache` | 19.00 / 20.50 ms | – | – |
| **Brush A input-to-pixel** (CPU stamp, dirty tiles) | 14.90 / 29.21 ms | 17.38 / 39.00 ms | 1.17× / 1.34× |
| **Brush B input-to-pixel** (GPU dabs) | **7.14 / 19.36 ms** | 17.38 / 39.00 ms | **2.4× / 2.0×** |
| Navigate vectors-500 (Vello every frame) | 7.34 / 10.62 ms | not run | – |
| Edit one vector object per frame | 6.81 / 10.10 ms | 12.23 / 16.07 ms | 1.8× / 1.6× |

The brush comparison uses the same 1.5 s S-curve at 1 kHz, a 300 px soft brush,
a 25-node 4K document, and the view at 100%. Latency runs from each input
event's scheduled time to GPU completion of the frame that shows it. Details:

- **Brush A:** `Stroke::render` took 1.2 ms of CPU per frame (p50) and
  uploaded 2.2 tiles per frame on average (1.1 MiB, up to 3 MiB).
- **Brush B:** no uploads during the stroke. The stroke end was read back in
  4.2 ms (13 MiB, 26 tiles). The result matches the CPU `Stroke` within 15/65535,
  at most 1 display code.
- **Baseline brush timing:** the GPUI baseline's brush run predates `9f8800d`
  and spun between input events. Its latency is valid; its frame-time stats are
  not, so they are left out.

CPU record and submit cost 0.2–0.5 ms per frame; 1.4 ms with Vello drawing
500 objects (1.1 ms of it in Vello). The Metal adapter gives no allocator
report. Texture sizes are the same as on lavapipe: 1115 MiB atlas, 192 MiB tile
cache, and 6 MiB per Vello run target.

Fidelity on Metal matches lavapipe: raster at most 1 code at levels 0–2, direct
and cached, in both blend spaces. Opaque vectors (vectors-500, sRGB mode) are
0.002% off-edge; the translucent sheet is 1.32%.

## Intel Iris Plus (Vulkan)

In progress.

## Timings on lavapipe (smoke run only)

1600×1000 view. Frame time covers CPU record plus the GPU, serialised. The GPUI
columns replay the same camera path and input script through the CPU raster work
the GPUI canvas does: a 480-entry tile cache, `render_tile` and `tile_to_bgra8`
for missing or dirty tiles. They exclude GPUI's layout, atlas upload and present.

| Workload | Spike p50 / p99 | GPUI-path CPU work p50 / p99 (mean) |
|---|---|---|
| Navigate layers-4k, tile cache (600 frames) | 17.6 / 56.6 ms | 0.0 / 57.1 ms (2.3) |
| — pan 100% | 18.2 / 68.1 ms | 0.0 / 65.3 ms (4.2) |
| — pan 50% | 16.0 / 21.5 ms | 0.0 / 20.1 ms (2.7) |
| — zoom sweep fit↔400% | 17.3 / 21.6 ms | 0.0 / 0.0 ms (0.15) |
| Navigate layers-4k, `--no-cache` | 132.5 / 167.8 ms | – |
| Brush A (CPU stamp, dirty tiles), input-to-pixel | 123.8 / 197.0 ms | 116.8 / 205.4 ms |
| Brush B (GPU dabs), input-to-pixel | 95.7 / 153.3 ms | – |
| Navigate vectors-500 | 45.6 / 66.1 ms | 0.0 / 30.8 ms (1.7) |
| Edit one vector object per frame | 51.0 / 64.7 ms | 28.5 / 49.1 ms |

Windowed runs under Xvfb gave the same picture: brush A 128 ms and brush B
105 ms p50 input-to-pixel; navigate 18.1 ms p50.

What carries over to real hardware:

- **CPU cost of the owned loop is 0.22–0.45 ms per frame** for record and
  submit on the 25-node document, whatever the camera does. Vello scene
  building plus its `render_to_texture` call takes 0.8–1.0 ms for 500 objects.
  On lavapipe the Vello frames block about 45 ms in submit, which is
  lavapipe executing Vello's compute pipeline, not CPU encoding.
- **A tile cache is not optional.** Recompositing all 25 layers per pixel every
  frame was 7.5× slower than sampling cached composite tiles. The GPU path needs
  the same cache the GPUI canvas already has. The `--no-cache` numbers are the
  per-frame cost that cache avoids.
- **Warm-cache navigation is equally cheap on both paths.** The 4K document's
  tiles at levels 0–2 (187) fit the 480-tile cache, so after the first pass
  GPUI does no raster work (mean 0.3 tiles per frame). The spike samples one
  texel per pixel. The difference is in misses: the GPUI path composites new
  tiles on the CPU (p99 57–65 ms here; 52–75 ms for 24 tiles on the Iris Plus
  in spike 1), while the spike fills them on the GPU.
- **Upload bytes.** Brush A uploads 5.7 tiles per frame on average (2.8 MiB,
  up to 4 MiB) and spends 11 ms of CPU in `Stroke::render`. Brush B uploads
  nothing during the stroke, then reads 13 MiB (26 tiles) back in 54 ms,
  asynchronously, at stroke end. Its result matches the CPU `Stroke` within
  5/65535.
- **Vector edits** re-encode one object. Re-encoding, scene assembly and the
  Vello render call together take about 1 ms of CPU. The GPUI path re-rasterizes the edited path into its document-size
  cache and recomposites 4 tiles (28.5 ms here).

## Reproducing on other hardware

Same files, same window size, vsync off:

```sh
cargo run --release -p vello-canvas-spike -- gen --out spikes/out
S=target/release/vello-canvas-spike; J=spikes/vello-canvas/results/bench-$(hostname).jsonl
for s in navigate brush-a brush-b; do $S bench $s spikes/out/layers-4k.ora --json $J; done
$S bench navigate spikes/out/layers-4k.ora --no-cache --json $J
$S bench navigate spikes/out/vectors-500.ora --json $J
$S bench vector-edit spikes/out/vectors-500.ora --json $J
for s in navigate brush-a vector-edit; do
  f=spikes/out/layers-4k.ora; [ $s = vector-edit ] && f=spikes/out/vectors-500.ora
  $S bench $s $f --baseline --json $J
done
$S fidelity spikes/out/*.ora --json spikes/vello-canvas/results/fidelity-$(hostname).jsonl
```

Since `9f8800d`, each JSON line records its options (`baseline`, `cache`,
…). The GPUI build itself hasn't been measured. The vendored GPUI has Tracy
spans behind `--cfg ztracing` (`vendor/gpui/gpui-pre-ztracing`). One Tracy
capture of a brush stroke in `emulsion-app` would replace the lower bound above
with a direct number.

## Reading against the decision criteria

**Fidelity:**
- **Raster:** met on both drivers. At most 1 display code at every zoom
  level, every blend mode, groups, clipping and masks.
- **Vector:** met for opaque content. Translucent overlaps inside a Vello run
  shift by up to about 10 codes (sRGB-space blending).

**Brush latency: ≥2× only with GPU-resident stamping (B).**
- **Test A** keeps Emulsion's CPU brush engine and gains 1.2–1.3×.
- **Test B covers only round, dry brushes at stroke opacity 1.** Wet, textured,
  dual and dynamic brushes would have to move to the GPU to get the same win.
- **B's win needs the GPU layer shown without a readback.** That's what an
  owned render loop provides. The GPUI canvas presents CPU images, and GPUI's
  macOS renderer is native Metal rather than wgpu. Getting B inside GPUI
  means embedding external textures, the out-of-scope question.

**Pan/zoom: not ≥2×.**
- With a warm cache, both paths show cached tiles, and GPUI's zoom sweep does no
  raster work at all.
- The spike only wins on cache misses: p99 1.7× when panning at 100%.
- Without a cache, the spike was 5× slower (19 ms vs 3.6 ms), so the GPU path
  needs the same tile cache GPUI already has.

**Vector edits:** 1.6–1.8× before GPUI's present is counted. Vello re-renders
500 objects every frame in about 7 ms on the M1. Editing one object costs
about 1 ms of CPU.

**Costs:**
- The GPU-resident document roughly doubles memory, since the CPU copy stays;
  on unified-memory machines that's system RAM.
- The spike implements neither layer styles nor adjustment layers.

By the brief's criteria this points to the shared engine crate, built around
GPU-resident painting, a GPU tile cache and Vello for vectors, and not to
faster pan/zoom. The deciding cost is moving the brush engine to the GPU. Before
committing, do the Iris Plus run (in progress) and one direct GPUI-build
latency capture. If the answer is "stay on GPUI", what's worth porting back:
- GPU-filled composite tiles (`emulsion-gpu` composites on the GPU but reads
  tiles back);
- dirty-tile invalidation across mip levels;
- per-tile mip chains that match CPU rounding.
