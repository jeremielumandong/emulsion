# Results: owned wgpu + Vello canvas vs GPUI canvas

*2026-09-26. Status: measured on Intel Iris Plus G7 (Vulkan), Apple M1 (Metal),
AMD Radeon RX 7700 XT (Windows; Vulkan and D3D12) and Mesa lavapipe, in a
standalone window and embedded in GPUI on all three machines. The GPUI build
itself not yet benchmarked directly. Stage 2 Windows editor functionality is
validated below.*

The GPUI column below is the CPU work the GPUI canvas does for the same scripted
input. It leaves out GPUI's own layout, atlas upload and present, so where the
spike is faster the ratios are lower bounds.

| Spike advantage (p50 / p99) | Intel Iris Plus G7 | Apple M1 | RX 7700 XT |
|---|---|---|---|
| Brush input-to-pixel, CPU stamping (test A) | **2.6× / 2.7×** | 1.2× / 1.3× | **43× / 1.7×** |
| Brush input-to-pixel, GPU dabs (test B) | **8.3× / 5.3×** | **2.1×** / 1.4× | **63× / 10×** |
| Pan at 100%, p99 frame | **4.0×** | 1.2× | **3.0×** |
| Edit one vector object per frame | **6.5× / 6.0×** | 1.7× / 1.2× | **10× / 4.3×** |
| Warm-cache pan/zoom p50 | no gain (both show cached tiles) | no gain | no gain |

**On the Iris Plus, which Emulsion targets, the brief's ≥2× bar is cleared in a
standalone window on brush latency with both brush tests, on pan p99 and on
vector edits.** On the M1 only GPU dabs clear it, at p50; its much faster CPU
shrinks the GPUI path's cost. Raster fidelity matches the CPU compositor within
one 8-bit code on all four drivers. On the desktop RX 7700 XT the spike's
frames take well under a millisecond, so its ratios are large, but they are
set by the GPUI path's CPU work (21 ms per brush frame) rather than by anything
the display could show at 165 Hz.

Embedded in a GPUI window, the canvas is paced to the 60 Hz display, and the
GPUI column is still unpaced CPU work, so these ratios are lower bounds:

| Inside GPUI, advantage at least (p50 / p99) | Intel Iris Plus G7 | Apple M1 | RX 7700 XT (165 Hz) |
|---|---|---|---|
| Brush input-to-pixel, CPU stamping (test A) | 1.2× / 1.0× | none shown | **3.6× / 3.5×** |
| Brush input-to-pixel, GPU dabs (test B) | **2.5×** / 1.3× | none shown | **3.7× / 4.3×** |
| Pan at 100%, p99 frame | 1.4× | 1.1× | **2.9×** |
| Edit one vector object per frame | **2.2× / 2.8×** | none shown | 1.95× / **2.8×** |

**Inside GPUI on the Iris, GPU dabs and vector edits keep a gain of 2× or more
at p50. Worst-case brush latency does not**, because of stalls inside GPUI that
the standalone window doesn't have; they are being traced. On the M1 the GPUI
path's work mostly fits in one 60 Hz frame, so the display pacing hides any
difference. **On Windows (RX 7700 XT, 165 Hz) every row keeps about 2× or more
inside GPUI, at p50 and p99, and the embedding costs nothing measurable:** it matches a
vsync'd standalone window on the same D3D12 backend. See
[Embedding in GPUI](#embedding-in-gpui).

### In short: is it really faster?

Yes for brushes and vector edits, on every machine. Not for panning over
content that is already cached, and not visibly on the M1 at 60 Hz.

- **Brush strokes.** On the Windows desktop the GPUI path does about 21 ms of
  CPU work per brush frame, and its input-to-pixel latency is 34 / 53 ms
  (p50 / p99). Inside GPUI the spike shows the stroke in 9 / 12–15 ms,
  about 3.5× lower, which is noticeable when painting.
- **Vector edits.** 12 ms of GPUI-path work against 6 ms inside GPUI: one
  165 Hz frame instead of two.
- **Panning into new areas.** p99 frames are about 3× better, because new
  tiles are filled on the GPU instead of the CPU.
- **Iris Plus, the target laptop.** GPU dabs and vector edits keep 2× or
  more at p50 inside GPUI.

Where it doesn't help or isn't proven yet:

- **Warm-cache pan/zoom:** both paths show cached tiles, so there is no gain.
- **Apple M1 at 60 Hz:** the GPUI path's work mostly fits in one frame, so the
  display hides the difference.
- **Standalone ratios** like 43× or 63× on the desktop are real, but the
  display caps what anyone sees. The GPUI-embedded numbers are what a user
  would feel.
- **The GPUI column is a lower bound.** It is CPU work only, without layout,
  upload or present. The GPUI build still needs a direct Tracy capture.
- **GPU dabs (test B)** cover only round, dry brushes at stroke opacity 1.
- **Stalls inside GPUI** on the M1 and Iris (80–130 ms p99) are unexplained.
  Windows has none, which points at GPUI's Linux/macOS frame scheduling.
- **Memory:** about 1.3 GiB of GPU memory for a 4K document, which is system
  RAM on integrated GPUs.
- **Vector translucency:** overlaps shift by a few codes (sRGB-space blending
  in Vello).

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

Out of scope in the brief; done afterwards as a follow-up. See
[Embedding in GPUI](#embedding-in-gpui). On Linux the engine shares GPUI's
wgpu 29.0.4 device. On macOS GPUI renders with Metal directly, so the canvas
crosses over through an IOSurface. On Windows GPUI renders with Direct3D 11, so
it crosses over through an NT-shared D3D12 texture.

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
runs at 1600×1000 device px, vsync off, commit `ffa48a4` (a full rerun of the
first `024a410` run, after the report-race and brush-baseline fixes; p50s
moved within noise). Raw data:
[`results/bench-Jeremies-MacBook-Pro.local.jsonl`](results/bench-Jeremies-MacBook-Pro.local.jsonl),
[`results/fidelity-Jeremies-MacBook-Pro.local.jsonl`](results/fidelity-Jeremies-MacBook-Pro.local.jsonl).
All three GPU tests pass with `EMULSION_REQUIRE_GPU_TESTS=1`.

Frame time is serialised: CPU record, GPU, present, then a wait for the GPU.
The GPUI column is the same script through the GPUI canvas's CPU raster work.

| Workload | Spike p50 / p99 | GPUI-path CPU work p50 / p99 (mean) | Spike advantage |
|---|---|---|---|
| Navigate layers-4k, all 600 frames | 3.96 / 17.28 ms | 0.00 / 17.25 ms (0.79) | none |
| — pan 100% | 4.16 / 17.28 ms | 0.00 / 20.24 ms | p99 1.2× |
| — pan 50% | 3.69 / 481.22 ms | 0.00 / 7.05 ms | none (two stalls) |
| — zoom sweep fit↔400% | 3.96 / 15.38 ms | 0.00 / 0.01 ms | none |
| Navigate layers-4k, `--no-cache` | 18.97 / 20.53 ms | – | – |
| **Brush A input-to-pixel** (CPU stamp, dirty tiles) | 14.10 / 31.09 ms | 17.00 / 39.03 ms | 1.2× / 1.3× |
| **Brush B input-to-pixel** (GPU dabs) | **8.28** / 27.04 ms | 17.00 / 39.03 ms | **2.1×** / 1.4× |
| Navigate vectors-500 (Vello every frame) | 7.26 / 11.72 ms | not run | – |
| Edit one vector object per frame | 7.29 / 12.21 ms | 12.19 / 14.73 ms | 1.7× / 1.2× |

The rerun's p99s are noisier than the first run's. Brush B's p99 was 19.4 ms in
the first run and 19.4–21.4 ms in three unrecorded reruns, against 27.0 ms
here. Pan 50% had two stalls of about half a second.

The brush comparison uses the same 1.5 s S-curve at 1 kHz, a 300 px soft brush,
a 25-node 4K document, and the view at 100%. Latency runs from each input
event's scheduled time to GPU completion of the frame that shows it. Details:

- **Brush A:** `Stroke::render` took 1.0 ms of CPU per frame on average and
  uploaded 2.1 tiles per frame on average (1.0 MiB, up to 3 MiB).
- **Brush B:** no uploads during the stroke. The stroke end was read back in
  4.2 ms (13 MiB, 26 tiles). The result matches the CPU `Stroke` within 15/65535,
  at most 1 display code.
- **Baseline brush timing:** since `9f8800d` the baseline records real frames
  (328 of them) instead of spinning between input events; its latency is
  unchanged at 17.0 / 39.0 ms.

CPU record and submit cost 0.3–0.5 ms per frame; 1.4 ms with Vello drawing
500 objects (1.2 ms of it in Vello). The Metal adapter gives no allocator
report. Texture sizes are the same as on lavapipe: 1115 MiB atlas, 192 MiB tile
cache, and 6 MiB per Vello run target.

Fidelity on Metal matches lavapipe: raster at most 1 code at levels 0–2, direct
and cached, in both blend spaces. Opaque vectors (vectors-500, sRGB mode) are
0.002% off-edge; the translucent sheet is 1.32%.

## Intel Iris Plus G7 (Vulkan)

Jeremie's Arch Linux machine: Intel Iris Plus Graphics G7 (ICL GT2), Vulkan via
Mesa 26.2.2, `Rgba16Unorm` tiles. Windowed runs at 1600×1000, vsync off, commit
`9f8800d`; the CPU baselines include that commit's fix for brush idling. Raw
data: [`results/bench-arkmac.jsonl`](results/bench-arkmac.jsonl),
[`results/fidelity-arkmac.jsonl`](results/fidelity-arkmac.jsonl). All three
GPU tests pass with `EMULSION_REQUIRE_GPU_TESTS=1`.

| Workload | Spike p50 / p99 | GPUI-path CPU work p50 / p99 (mean) | Spike advantage |
|---|---|---|---|
| Navigate layers-4k, all 600 frames | 1.91 / 9.16 ms | 0.00 / 34.70 ms (1.49) | p99 3.8× |
| — pan 100% | 1.96 / 11.87 ms | 0.00 / 47.71 ms (2.81) | p99 4.0× |
| — pan 50% | 1.78 / 6.35 ms | 0.00 / 16.03 ms (1.58) | p99 2.5× |
| — zoom sweep fit↔400% | 1.90 / 4.82 ms | 0.00 / 0.01 ms (0.12) | none |
| Navigate layers-4k, `--no-cache` | 31.36 / 34.62 ms | – | – |
| **Brush A input-to-pixel** (CPU stamp, dirty tiles) | **25.68 / 48.82 ms** | 65.88 / 133.32 ms | **2.6× / 2.7×** |
| **Brush B input-to-pixel** (GPU dabs) | **7.89 / 24.99 ms** | 65.88 / 133.32 ms | **8.3× / 5.3×** |
| Navigate vectors-500 (Vello every frame) | 5.67 / 8.00 ms | not run | – |
| Edit one vector object per frame | 5.71 / 8.16 ms | 37.36 / 48.88 ms | 6.5× / 6.0× |

- **Brush A frames.** On this CPU, `Stroke::render` takes 4.3 ms p50 (11.6
  ms p99). Each frame uploads 2.5 tiles on average (1.2 MiB, up to 3 MiB).
- **Brush A baseline.** The GPUI path spends 41 ms p50 (75 ms p99) per
  stroke frame recompositing about 5 dirty tiles on the CPU. That's where the
  latency difference comes from.
- **Brush B.** It uploads nothing during the stroke and reads the stroke end
  back in 8.7 ms (13 MiB). The result matches the CPU `Stroke` within 5/65535.
- **Vello.** Scene building and the render call cost 0.5–0.7 ms of CPU per
  frame for 500 objects. Total CPU record per frame is 0.2 ms for rasters and
  0.8 ms with vectors.
- **Tile cache.** Recompositing all 25 layers every frame took 31 ms; the
  tile cache brings that to 1.9 ms, 16×.
- **GPU memory.** The allocator reports 1451 MiB for the 4K document: 1308
  MiB of spike textures plus driver overhead.

Fidelity on the Iris matches the other drivers: raster at most 1 code at levels
0–2, direct and cached, in both blend spaces; the translucent fidelity sheet
is 1.39% off-edge in sRGB mode.

**GPU hang.** The first fidelity sweep hung the GPU on `vectors-500`. i915
logged "GPU HANG … Resetting rcs0 for preemption time out", then "device
wedged". The check had rendered the whole 3840×2160 document in one draw with
Vello off. That makes 501 raster ops per pixel over 8.3 MP, which outlasts the
preemption timeout. Since this commit, the fidelity check renders 512² chunks,
each submitted separately, and cache fills issue at most 4 tiles per draw. Those
eight entries need a rerun:

```sh
target/release/vello-canvas-spike fidelity spikes/out/vectors-500.ora \
  --json spikes/vello-canvas/results/fidelity-$(hostname).jsonl
```

Any production engine has the same constraint: work per draw has to be bounded
by tiles × ops, not by the view.

## AMD Radeon RX 7700 XT (Windows)

Jeremie's desktop: AMD Ryzen 7 8700G (with a Radeon 780M, unused here), 62 GiB,
AMD Radeon RX 7700 XT driving a 2560-wide display at 165 Hz, Windows 11 Home
(build 26200), Rust 1.98.1, `Rgba16Unorm` tiles. wgpu picked Vulkan (AMD
proprietary driver 26.8.1, LLPC) for standalone runs. Windowed runs at 1600×1000,
vsync off, commit `40a7217`. Raw data:
[`results/bench-PCDESKPC.jsonl`](results/bench-PCDESKPC.jsonl),
[`results/fidelity-PCDESKPC.jsonl`](results/fidelity-PCDESKPC.jsonl). All three
GPU tests pass on D3D12.

| Workload | Spike p50 / p99 | GPUI-path CPU work p50 / p99 (mean) | Spike advantage |
|---|---|---|---|
| Navigate layers-4k, all 600 frames | 0.31 / 0.79 ms | 0.00 / 19.57 ms | p99 25× |
| — pan 100% | 0.32 / 7.02 ms | 0.00 / 20.90 ms (1.50) | p99 3.0× |
| — pan 50% | 0.31 / 0.47 ms | 0.00 / 10.99 ms | p99 23× |
| — zoom sweep fit↔400% | 0.31 / 0.45 ms | 0.00 / 0.00 ms | none |
| Navigate layers-4k, `--no-cache` | 1.12 / 1.72 ms | – | – |
| **Brush A input-to-pixel** (CPU stamp, dirty tiles) | **0.78** / 31.35 ms | 33.61 / 52.86 ms | **43× / 1.7×** |
| **Brush B input-to-pixel** (GPU dabs) | **0.53 / 5.20** ms | 33.61 / 52.86 ms | **63× / 10×** |
| Navigate vectors-500 (Vello every frame) | 0.95 / 1.36 ms | not run | – |
| Edit one vector object per frame | 1.14 / 4.34 ms | 11.80 / 18.47 ms | **10× / 4.3×** |

- **The spike is GPU-bound far below a frame.** Unpaced, it renders at
  1–3 kHz, so the brush script's 1 kHz input reaches the screen in under a
  millisecond at p50. Brush A's p99 comes from frames that upload dirty tiles
  (up to 3 MiB).
- **The GPUI path's CPU work is the same order as on the M1 and Iris:** 21 ms
  per brush frame, 12 ms per vector edit. On this machine that is 3–4
  refreshes at 165 Hz, so unlike the M1, the gain survives display pacing (see
  [Embedding in GPUI](#embedding-in-gpui)).
- **Without the tile cache**, pan is 3.6× slower at p50 but still
  1.1 ms: the GPU has headroom the laptops don't.

Fidelity on AMD Vulkan matches the other drivers. Raster is at most 1 code at
levels 0–2, direct and cached, in both blend spaces. vectors-500 in sRGB mode is
0.003% off-edge. The translucent sheets are 1.34–1.35% (sRGB-encoded) and 1.0%
(linear 8-bit). The fidelity run wrote all 30 cases and then crashed at process
exit (`0xC0000409`) during Vulkan teardown. That doesn't affect the results.

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

## Embedding in GPUI

The engine runs inside a GPUI window, next to ordinary GPUI chrome (a toolbar
with live stats and a layer sidebar). GPUI's own renderer composites the canvas:
no readback, no `paint_image`. The README describes the patches.

- **Linux:** the engine adopts GPUI's wgpu device and paints an external
  texture.
- **macOS:** GPUI renders with Metal directly. The engine keeps its own wgpu
  Metal device and renders into a ring of IOSurface-backed BGRA textures. GPUI
  paints them with its existing `Window::paint_surface`, patched to accept
  single-plane BGRA. The two command queues aren't ordered, so each canvas
  frame waits for its own GPU work before GPUI samples it.
- **Windows:** GPUI renders with Direct3D 11. The engine keeps its own wgpu
  D3D12 device on GPUI's adapter and renders into a ring of NT-shared D3D12
  textures. GPUI's `draw_surfaces`, patched from a no-op, opens each handle once
  and draws it with its image shaders. As on macOS, each canvas frame waits for
  its own GPU work.

Verified:

- **Lavapipe under Xvfb:** the canvas composites in the GPUI window
  ([screenshot](results/gpui-embedded-lavapipe.png)). Mouse painting with
  both brushes, panning, zoom and the toolbar work, driven with `xdotool`.
  Every scenario completes, and the CI renderer smoke test passes with the
  patched GPUI.
- **Iris Plus (Hyprland):** all four scenarios complete, and brush B matches
  the CPU `Stroke` within 5/65535 ([screenshot](results/gpui-embedded-iris.png)).
- **M1:** release build, fmt, clippy, the vendor and licence checks and the
  GPU tests pass. `emulsion-app` still draws an open document. Painting with
  both brushes, panning, zoom, Fit and 100% work in the embedded view
  ([screenshot](results/gpui-embedded-m1.png)).
- **RX 7700 XT (Windows 11):** release build, fmt, clippy and the GPU tests
  on D3D12 pass. `emulsion-app` still draws an open document. All four
  scenarios complete inside GPUI, including brush B's readback, and the
  embedded view composites beside the chrome
  ([screenshot](results/gpui-embedded-windows.png)). GPUI and the engine both
  run on the 7700 XT, not the 780M. On a Windows checkout,
  `check-gpui-vendor.py` and `test-license-staging.py` fail on licence
  hashes, because `core.autocrlf` rewrites the vendored licence files. That
  is unrelated to this patch; CI checks them on Linux.

### Results

Commit `63d9642`. Raw data:
[`bench-arkmac-gpui.jsonl`](results/bench-arkmac-gpui.jsonl),
[`bench-Jeremies-MacBook-Pro.local-gpui.jsonl`](results/bench-Jeremies-MacBook-Pro.local-gpui.jsonl) and
[`bench-Jeremies-MacBook-Pro.local-vsync.jsonl`](results/bench-Jeremies-MacBook-Pro.local-vsync.jsonl).
Pan and vector edit rows are frame times; brush rows are input-to-pixel latency.

| Iris Plus G7, p50 / p99 | Standalone, vsync off | Inside GPUI | GPUI-path CPU work |
|---|---|---|---|
| Pan 100% | 1.96 / 11.87 ms | 16.66 / 33.89 ms | 0.00 / 47.71 ms |
| Brush A | 25.68 / 48.82 ms | 53.67 / 131.87 ms | 65.88 / 133.32 ms |
| Brush B | 7.89 / 24.99 ms | **26.76** / 105.78 ms | 65.88 / 133.32 ms |
| Edit one vector object | 5.71 / 8.16 ms | **16.67 / 17.72 ms** | 37.36 / 48.88 ms |

| Apple M1, p50 / p99 | Standalone, vsync off | Standalone, vsync | Inside GPUI | GPUI-path CPU work |
|---|---|---|---|---|
| Pan 100% | 4.16 / 17.28 ms | 16.43 / 25.98 ms | 16.65 / 18.97 ms | 0.00 / 20.24 ms |
| Brush A | 14.10 / 31.09 ms | 24.83 / 38.55 ms | 26.37 / 90.54 ms | 17.00 / 39.03 ms |
| Brush B | 8.28 / 27.04 ms | 25.13 / 46.31 ms | 25.42 / 78.30 ms | 17.00 / 39.03 ms |
| Edit one vector object | 7.29 / 12.21 ms | 16.47 / 18.26 ms | 16.67 / 18.50 ms | 12.19 / 14.73 ms |

How to read these tables:

- **Display pacing.** GPUI draws when the display asks for a frame: Wayland
  frame callbacks on Hyprland, the display link on macOS. So every embedded
  p50 sits at the 60 Hz interval (brush A on the Iris at twice it), and p99s
  land on two intervals.
  - **The engine's own cost doesn't change.** On the Iris, CPU record and
    submit is 0.24 ms p50 inside GPUI against 0.19 ms standalone, with the
    same upload and dab counts.
  - **The vsync'd M1 window shows the same medians as GPUI.** That's 16.4–16.7
    ms frames and about 25 ms brush latency, so the pacing, not the embedding,
    sets the median.
- **The GPUI column is a floor.** It is the GPUI canvas's CPU work alone,
  without pacing or present, and the real GPUI canvas is paced too. So the
  embedded number against it is a lower bound on the advantage.
  - **On the Iris, the GPUI path's work exceeds a frame,** so the gain shows
    through the pacing: brush B at least 2.5× at p50, vector edits at least
    2.2× / 2.8×.
  - **On the M1, that work mostly fits in a frame** (12 ms for a vector edit,
    17 ms brush latency), so 60 Hz pacing hides the difference. The M1 can't
    show a gain at this refresh rate.
- **Brush A runs at half rate inside GPUI on the Iris.** Its frames are
  33.3 ms p50, against 14.3 ms standalone. CPU stamping takes 6.7 ms p50 per
  frame inside GPUI (4.3 ms standalone) and runs in the canvas paint on GPUI's
  main thread. Which part of the frame misses the refresh is not yet known.
  Brush A keeps only 1.2× / 1.0×.
- **Stalls inside GPUI.** Brush p99 latency inside GPUI is 78–90 ms on the M1,
  against 39–46 ms in the vsync'd window, and 106–132 ms on the Iris.
  - **On the M1, the extra time comes in a few stalls.** In brush B, frame
    times total 60 ms over the median across 88 frames: about one stall of
    three refreshes. Brush A's total is 136 ms. The vsync'd standalone window
    has none.
  - **The cause isn't known yet.** Since `f6cde2b`, reports list slow frames
    and split each into three parts: the wait for GPUI's previous frame, the
    canvas paint, and GPUI's own share (`slow_frames` in the JSON).
- **Canvas sizes differ.** `--size` is logical pixels in GPUI and device
  pixels in the standalone window, and Hyprland tiles windows, so the rendered
  canvases differ:
  - Iris: 808×1465 embedded (1.18 Mpx) and 1224×1523 standalone (1.86 Mpx).
  - M1: 2360×1494 embedded (3.5 Mpx) and 1600×1000 standalone.
  - The CPU-work column rendered 1600×1000.

  Brush cost follows dirty tiles, not view size, so the brush rows compare
  fairly; the pan rows less so. Standalone reports now record the rendered
  size.

| RX 7700 XT (165 Hz), p50 / p99 | Standalone, vsync off (Vulkan) | Standalone, vsync (D3D12) | Inside GPUI (D3D12) | GPUI-path CPU work |
|---|---|---|---|---|
| Pan 100% | 0.32 / 7.02 ms | 5.99 / 9.32 ms | 6.04 / 7.24 ms | 0.00 / 20.90 ms |
| Brush A | 0.78 / 31.35 ms | 9.06 / 14.44 ms | **9.23 / 15.19 ms** | 33.61 / 52.86 ms |
| Brush B | 0.53 / 5.20 ms | 9.06 / 18.03 ms | **9.06 / 12.31 ms** | 33.61 / 52.86 ms |
| Edit one vector object | 1.14 / 4.34 ms | 6.01 / 7.00 ms | **6.04 / 6.63 ms** | 11.80 / 18.47 ms |

Windows commit `40a7217`. Raw data:
[`bench-PCDESKPC-gpui.jsonl`](results/bench-PCDESKPC-gpui.jsonl),
[`bench-PCDESKPC-vsync-dx12.jsonl`](results/bench-PCDESKPC-vsync-dx12.jsonl),
and a vsync'd Vulkan run,
[`bench-PCDESKPC-vsync.jsonl`](results/bench-PCDESKPC-vsync.jsonl). It has the
same medians as D3D12; brush p99s vary run to run (brush A 37.8 ms there).

- **Embedding costs nothing measurable on Windows.** Against a vsync'd
  standalone window on the same D3D12 backend, frames and latency match within
  1 ms at p50 and p99. The engine's CPU record and submit is 0.15–0.63 ms p50
  inside GPUI and 0.29–1.03 ms standalone.
- **No stalls like the M1's and Iris's.** Brush A inside GPUI has four frames
  over 1.5× the median in 245, and brush p99 latency is 12–15 ms, about two
  refreshes. Brush A runs at full rate.
- **Pacing.** GPUI on Windows presents with `Present(0)` from a
  DwmFlush-paced loop, so embedded frames sit at the 165 Hz interval (6.0 ms),
  and brush latency at p50 is one and a half refreshes.
- **The gain survives the pacing.** The GPUI path's CPU work, 21 ms per brush
  frame and 12 ms per vector edit, spans several 6 ms refreshes. So inside GPUI
  every row keeps about 2× or more at both p50 and p99: brush A 3.6× / 3.5×,
  brush B 3.7× / 4.3×, vector edits 1.95× / 2.8×, pan p99 2.9×.

**Next:** find the stalls and brush A's half rate before building on this.
Rerun the brush tests inside GPUI on both laptops with the slow-frame
breakdown. On the Iris, also run the standalone window with `--vsync`, the
like-for-like baseline the M1 already has:

```sh
S=target/release/vello-canvas-spike; H=$(hostname); R=spikes/vello-canvas/results
for i in 1 2 3; do for s in brush-a brush-b; do
  $S bench $s spikes/out/layers-4k.ora --gpui --json $R/bench-$H-gpui-slow.jsonl
done; done
# Iris only:
for s in navigate brush-a brush-b; do $S bench $s spikes/out/layers-4k.ora --vsync --json $R/bench-$H-vsync.jsonl; done
$S bench vector-edit spikes/out/vectors-500.ora --vsync --json $R/bench-$H-vsync.jsonl
```

Stage 2 now embeds the engine in `emulsion-ui` and enables it by default on
supported platforms. The laptop stalls remain relevant to further integration.

## Stage 2 Windows validation

2026-09-26, source `96bc42a`, on the same RX 7700 XT desktop and 165 Hz display
described above. D3D12 driver `32.0.31041.1004`, Rust 1.98.1,
`Rgba16Unorm` tiles. The extracted engine builds independently with
`cargo build --locked --release -p emulsion-engine --features gpui`; the
release spike and editor also build. No Windows backend changes were needed.

The shipping editor starts with `EMULSION_GPU_CANVAS` **unset** and reports
`gpu canvas active` on the RX 7700 XT. A disposable 800×600 document was edited
through Windows keyboard and mouse input: brush, eraser, undo/redo, bucket fill
and undo, a committed freeform pen path, and committed text. Raster edits logged
`reload: pixels only`; new vector content logged a successful vector resync.
The pen stroke remained visible after leaving the pen tool, and text remained
visible after committing the text session. The resulting ORA was saved and
reopened. [Editor screenshot](results/stage2-windows-editor.png).

The following command passed **29 headless UI tests** (no skipped tests),
covering the editing operations, history and canvas invalidation independently
of real GPU presentation:

```powershell
cargo test --locked -p emulsion-ui --lib -- paint_functionality_tests pen_workflow_tests on_canvas_text_tests canvas_invalidation_tests --test-threads=1
```

Windows WARP also reopened and rendered the saved raster/path/text project with
the engine active: `Microsoft Basic Render Driver`, D3D12 driver
`10.0.26100.9549`. Thus a matching D3D12 software adapter **does** exist on this
Windows build; the older blanket statement that WARP cannot host the engine was
incorrect. This is a functional check, not a software-rendering performance
claim. Launching with `EMULSION_GPU_CANVAS=0` also preserves the CPU canvas.
Device-loss recovery has not been validated or implemented by these checks.

### Paced host comparison

All four scenarios completed in both hosts. Both used D3D12 and actual
1600×1000 device-pixel canvases, with no concurrent build running. The
standalone runs used `--vsync`. Navigation covers all 600 frames and vector
editing all 300; brush scripts deliver the full 1.5-second, 1 kHz input stream.
Times below are milliseconds, p50 / p99, from one run per scenario and host.

| Workload / measure | GPUI embedded | Standalone, vsync |
|---|---|---|
| Navigation frame | 6.07 / 7.90 | 5.94 / 8.53 |
| Brush A frame | 6.21 / 10.56 | 6.01 / 9.42 |
| Brush A input-to-pixel | 9.49 / 17.36 | 9.16 / 37.22 |
| Brush B frame | 6.05 / 7.74 | 5.98 / 12.08 |
| Brush B input-to-pixel | 9.07 / 12.86 | 9.15 / 30.92 |
| Vector-edit frame | 6.10 / 7.51 | 5.98 / 7.33 |

Raw data: [GPUI](results/stage2-PCDESKPC-gpui-dx12.jsonl),
[standalone vsync](results/stage2-PCDESKPC-vsync-dx12.jsonl),
[standalone brush B rerun](results/stage2-PCDESKPC-vsync-brush-b-rerun.jsonl).
The first standalone brush B sample reported a final 1×1 surface and is
excluded from the table; its replacement explicitly reports 1600×1000.
A hidden-window attempt produced no report and was also excluded. Presentation
benchmarks must run with an onscreen window and verify the reported size.

The medians sit near the 165 Hz refresh interval (6.06 ms), consistent with
GPUI's DwmFlush-paced `Present(0)` loop. They do not reveal sub-refresh GPU
headroom. Brush B readback differs from the CPU stroke by at most 4/65535 per
channel and one 8-bit display code; its 13 MiB readback completed in 2.9 ms
embedded and 1.9 ms in the standalone rerun. Both hosts allocated about
1307 MiB of textures for the 4K document at this viewport size.

Standalone brush tail latency was worse in this sample; repeated measurements
are needed before attributing that difference to a host or driver. These are
still **spike-host** timings, not end-to-end shipping-editor latency. The editor
does not yet route strokes through brush B. Missing canvas chrome, masked/placed
layer update cost, device-loss handling, lazy text/path caches, vector blending
and memory costs remain listed in [STAGE2_PENDING.md](STAGE2_PENDING.md).

## Linux migration follow-up, 2026-09-26

Branch `feat/vello-migration-ui`, uncommitted working tree based on `80f7712`.
Ryzen 7 8700G, RX 7700 XT, RADV Mesa 26.2.2, Linux/Wayland, display scale 1.25.
These checks cover the shipping editor integration; earlier spike and Windows
measurements above remain historical results from their recorded revisions.

The follow-up adds incremental transformed/masked pixel rebakes, per-object vector
updates, GPU brush/eraser routing with CPU replay recovery, device-generation/loss
checks, fallback indicators, texture allocation limits, and editable in-process
text copy/paste. Unsupported brush features retain CPU handling. Translucent vector
paint uses the CPU reference rasterizer inside the GPU composite.

Validation: **477 UI tests**, **144 core tests**, **161 raster tests**, **6 engine
tests**, and **15 spike GPU tests** passed; two UI/core tests and one engine
microbenchmark are intentionally ignored. Clippy with warnings denied, formatting,
and vendored-GPUI validation passed. GPU checks used the Vulkan hardware adapter.
Native-window checks additionally assert GPU brush activation, committed pixels,
and stroke undo/redo. Device-loss coverage destroys an owned engine device; a real
shared GPUI/OS device reset is still a manual platform check.

### Native EditorView measurements

The opt-in `editor_canvas_bench` example wraps the real editor in the same flex
sizing contract used by Workspace. It calls the pan and brush input handlers and
executes editable-text translations, using isolated temporary app data. Each case
has 8 warm-up inputs and 40 measured inputs. The CPU path waits for current tile
work. The default synthetic 4K fixture has 24 raster layers plus text and a paint
layer; the original `layers-4k.ora` fixture gets an added text and paint layer.

JSON records actual canvas dimensions, display scale, adapter, GPU brush routing,
texture bytes, and commit/readback time. **Input-to-canvas-submission** ends after
painting the current canvas into the GPUI scene; the second metric ends at the
following platform frame callback. Neither is physical display latency. Compilation,
opening the document, and stroke-end readback are outside the input samples.
Earlier exploratory runs with an undersized standalone parent were discarded.

Raw data: [native editor results](results/editor-linux-rx7700xt-migration.json).
Use the matched CPU/GPU canvas dimensions in that report when comparing results.

### Fidelity and remaining work

[Full fixture comparisons](results/fidelity-linux-rx7700xt-migration.md) preserve
all results, including differences. `layers-4k` remains within one display code
at mip levels 0–2 with direct and cached GPU composition. The targeted translucent
colored-overlap regression also matches the reference within one display code.

`vectors-500` remains at **3.250%** of pixels over one display code, with **0.008%**
off-edge over three codes. Its generator uses opaque paints: the previous pending
tracker's attribution of that whole number to translucent paint was incorrect.
The translucent-node fix does not resolve the opaque vector edge/overlap differences.
The linear 8-bit Vello mode is worse (**14.152%** over one code), so it was not
selected as the shipping fix. Broader vector parity remains open.

macOS integration and Windows revalidation of the new brush/recovery work remain
open. Reproduce with [the validation script and manual checks](PLATFORM_VALIDATION.md).

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
- **Raster:** met on lavapipe, Metal and Iris Vulkan. At most 1 display code
  at every zoom level, every blend mode, groups, clipping and masks.
- **Vector:** met for opaque content. Translucent overlaps inside a Vello run
  shift by up to about 10 codes (sRGB-space blending).

**Brush latency: ≥2× on the Iris with either brush test; on the M1 only with
GPU dabs.**
- **Where the gain comes from:** the GPUI path recomposites dirty tiles on the
  CPU every stroke frame (41 ms on the Iris machine). The owned path moves that
  to the GPU. Uploading dirty tiles from the unchanged CPU brush engine (test
  A) is enough for 2.6× on the Iris.
- **Test B's limits:** it covers only round, dry brushes at stroke opacity 1.
  Wet, textured, dual and dynamic brushes would have to move to the GPU to get
  its larger gain.
- **B needs the GPU layer shown without a readback.** The GPUI canvas
  presents CPU images. Embedding does this on all three platforms (see
  [Embedding in GPUI](#embedding-in-gpui)): on the Iris, B keeps at least
  2.5× at p50 inside GPUI, but not yet at p99; on the Windows desktop at
  165 Hz, at least 3.7× / 4.3×.

**Pan/zoom:**
- **Warm cache:** no gain at p50. Both paths show cached tiles, and GPUI's zoom
  sweep does no raster work.
- **Cache misses:** the spike fills new tiles on the GPU, giving p99 4.0× at
  100% pan on the Iris (1.2× on the M1).
- **The tile cache is required:** without it the spike was 16× slower on the
  Iris.

**Vector edits:** 6.5× on the Iris (at least 2.2× inside GPUI), 1.7× on the
M1. Vello re-renders 500 objects every frame in 5.7 ms on the Iris.

**Costs:**
- **Memory:** a GPU-resident 4K document took 1.45 GiB on the Iris, on top of
  the CPU copy; on integrated GPUs that's system RAM.
- **Not implemented:** layer styles and adjustment layers.
- **Per-draw work:** must be bounded (the i915 hang above).

**By the brief's criteria (≥2× on brush latency or pan/zoom, with matching
fidelity), this is "clearly faster".** The brief's next step is a shared engine
crate: wgpu tile atlas, GPU tile cache, dirty-tile uploads and GPU dabs for
brushes, and Vello for vectors, with egui evaluated for UI chrome. Open items
before committing to it:

- **Stalls inside GPUI.** On the Iris, GPU dabs and vector edits keep at least
  2× at p50 embedded in GPUI, so GPUI could keep the chrome, with no UI toolkit
  change. That holds only if the brush p99 stalls and brush A's half rate
  inside GPUI can be fixed (see [Embedding in GPUI](#embedding-in-gpui)).
  GPUI's Windows renderer shows neither, which points at the Linux and macOS
  frame scheduling rather than at the embedding itself.
- **One direct GPUI-build capture** of a brush stroke and a pan, via Tracy
  (`--cfg ztracing`). The GPUI numbers here are lower bounds, so this can only
  widen the gaps, but it replaces an estimate with a measurement.
- **Vector translucency:** accept sRGB-space blending inside Vello runs, give
  each translucent object its own run, or wait for 16-bit float targets in
  Vello.
- **Memory budget** for large documents: evict atlas pages, or keep
  high-resolution mips only for visible tiles.

If the answer is still "stay on GPUI", the wins worth porting are:
- GPU-filled composite tiles (`emulsion-gpu` composites on the GPU but reads
  tiles back);
- dirty-tile invalidation across mip levels;
- per-tile mip chains that match CPU rounding.
