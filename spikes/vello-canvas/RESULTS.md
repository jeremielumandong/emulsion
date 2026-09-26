# Results: owned wgpu + Vello canvas vs GPUI canvas

*2026-09-26. Status: built; fidelity measured; **hardware timings not yet recorded**.*

**No decision yet.** The only GPU available for these runs was Mesa lavapipe, a
software Vulkan driver, on the same 4 vCPUs as the CPU baseline. Its timings
measure a shader emulator and cannot show whether an owned pipeline beats
GPUI. The findings that don't depend on hardware are fidelity, bytes moved,
memory, CPU cost per frame and the structure of each path. The
[hardware run](#hardware-run-to-do) section lists the commands for a real GPU.

## Machine for these runs

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

## Hardware run: to do

Measure on the Iris Plus G7 machine (and any discrete GPU), same files, same
window size, vsync off:

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

For the GPUI build, open the same files in `emulsion-app` (release) and record
frame time during the same gestures. The vendored GPUI has Tracy spans behind
`--cfg ztracing` (`vendor/gpui/gpui-pre-ztracing`), and `editor_navigation`
covers the CPU side of panning. Neither was run for this report. Fill in:

| Measure | Spike | GPUI build | Ratio |
|---|---|---|---|
| Pan 100%, layers-4k, p50 / p99 frame | | | |
| Zoom sweep, layers-4k, p50 / p99 frame | | | |
| Brush A input-to-pixel p50 / p99 | | | |
| Brush B input-to-pixel p50 / p99 | | | |
| vectors-500 navigate p50 / p99 frame | | | |
| Vector edit p50 / p99 frame | | | |
| GPU memory (allocator report) | | | |
| Upload bytes per brush frame | | | |

## Reading so far (not the decision)

- **Fidelity:** the raster criterion is met; results are indistinguishable at 8
  bits, zoomed out included. Vello meets it only for opaque vector content.
  Translucent overlaps shift by up to about 10 codes.
- **Speed:** the case for ≥2× lies in cache misses, zoom to new levels,
  and painting. There, the owned path does GPU work instead of CPU compositing and
  per-frame uploads. Warm-cache panning is a textured blit on both paths.
  Only hardware p99 numbers can show whether that clears the 2× bar.
- **Worth porting to GPUI either way:**
  - GPU-filled composite tiles: `emulsion-gpu` already composites tiles on
    the GPU, but reads them back.
  - Dirty-tile invalidation across mip levels.
  - Per-tile mip chains that match CPU rounding.
  - Brush B's resident dab stamping, which removes the per-frame uploads of
    test A.
