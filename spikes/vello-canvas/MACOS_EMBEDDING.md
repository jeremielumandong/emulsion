# Brief: embed the vello-canvas engine in GPUI on macOS

*For a session on the Apple M1. It builds on the Linux embedding in `88ba943`;
read `README.md` ("Linux embedding in GPUI") and `src/gpui_host.rs` first.*

## Goal

`vello-canvas-spike … --gpui` should work on macOS the way it works on Linux:
the engine draws the canvas into a GPU texture, and GPUI's own Metal renderer
composites that texture beside ordinary GPUI chrome, with no CPU readback and
no `paint_image`. Then measure the four scenarios on the M1 inside GPUI, so
`RESULTS.md` can say whether the spike's M1 gains survive the embedding.

Keep it a spike: small, additive changes, the Linux path unchanged, and the
Emulsion app untouched.

## Approach

On Linux, GPUI renders with wgpu, so the engine simply borrows GPUI's device.
On macOS, GPUI renders with `metal-rs` and wgpu is a separate Metal client.
The two share memory through an **IOSurface**, the mechanism GPUI already
uses for video:

- `Window::paint_surface(bounds, CVPixelBuffer)` already exists on macOS
  (`vendor/gpui/gpui-pre/src/window.rs`). **No `gpui-pre` change is needed.**
- The Metal renderer's `draw_surfaces` currently asserts
  `kCVPixelFormatType_420YpCbCr8BiPlanarFullRange` (video). It has to learn
  single-plane BGRA.
- The engine renders, on its own wgpu device, into a wgpu texture made
  from the same IOSurface. On Apple silicon both devices are the one GPU.

## 1. GPUI patch (`vendor/gpui/gpui-pre-apple` only)

**BGRA surfaces**, in `src/metal_renderer.rs` and `src/shaders.metal`:

- Add `fragment float4 surface_bgra_fragment(SurfaceFragmentInput input
  [[stage_in]], texture2d<float> texture [[texture(SurfaceInputIndex_YTexture)]])`.
  It should sample with the same linear sampler and return the colour
  unchanged. Reuse the `YTexture` slot, so `SurfaceInputIndex` and the
  cbindgen header in `build.rs` stay as they are. `surface_vertex` already
  clips to the content mask.
- Build a second pipeline, `surfaces_bgra_pipeline_state`: `surface_vertex`
  and `surface_bgra_fragment`, `BGRA8Unorm`, the same way `surfaces_pipeline_state`
  is built.
- In `draw_surfaces`, replace the `assert_eq!` on the pixel format with a
  match:
  - YCbCr keeps today's path.
  - `kCVPixelFormatType_32BGRA` makes one texture:
    `core_video_texture_cache.create_texture_from_image(buf, None,
    MTLPixelFormat::BGRA8Unorm, w, h, 0)`. Bind it at `YTexture`, set the BGRA
    pipeline, and draw.
  - Anything else logs once and is skipped.

  Set the pipeline per surface, because video and canvas surfaces can mix.

**Frame completion**, so the canvas can wait for GPUI the way Linux does:

- Add two process-wide counters to `metal_renderer.rs`: frames **submitted**
  (incremented in `draw()` after `commit()`) and frames **completed**
  (incremented in the existing `add_completed_handler` block in
  `render_frame`). Put a `Mutex` + `Condvar` next to them.
- Export `pub fn wait_for_submitted_frames()` from `src/gpui_apple.rs`. It
  blocks until completed ≥ submitted. The windowed path is the only one that
  matters; headless renders need not count.

**Bookkeeping** (`python3 scripts/check-gpui-vendor.py` enforces some of it):

- Add a `// Modified by Emulsion: …` first line to each changed file, in the
  style of `gpui-pre-wgpu/src/wgpu_renderer.rs`.
- Create `vendor/gpui/gpui-pre-apple/EMULSION_CHANGES.md` in the style of
  `gpui-pre-wgpu/EMULSION_CHANGES.md`.
- In `vendor/gpui/README.md`, add it to "Local patches" and change the
  sentence "Native macOS Metal rendering is unchanged".

## 2. Spike changes (`spikes/vello-canvas`)

**Dependencies.** Add a `[target.'cfg(target_os = "macos")'.dependencies]`
block with `gpui-kit` (workspace) and `gpui-pre-apple = "=0.3.5"`, which is
patched to the vendored path in the root `Cargo.toml`. Add `core-video = "0.5.2"`,
`objc2`, `objc2-metal` 0.3 (with the `objc2-io-surface` feature) and
`objc2-io-surface`. Use only versions already in `Cargo.lock`, so no new
crates are pulled. If the lockfile changes, run
`python3 scripts/gen-third-party.py` and commit `THIRD_PARTY_CRATES.md` too.

**Make `gpui_host.rs` cross-platform.** Gate it (and `gpui_embedded` in
`main.rs`) on `any(target_os = "linux", target_os = "macos")`. Keep
everything shared: state, scripts, input, chrome, stats. Put the platform
differences behind a small `#[cfg]` backend with four operations:

| | Linux (today) | macOS |
|---|---|---|
| Device | `Gpu::from_shared(gpui_wgpu::shared_gpu())` | `Gpu::new(Metal instance, None, tiles)`: its own device |
| Wait for previous GPUI frame | `engine.gpu.wait()` | `gpui_apple::wait_for_submitted_frames()`, then `engine.gpu.wait()` |
| Target | one `Rgba8Unorm` wgpu texture | a ring of 3 IOSurface-backed BGRA textures (below) |
| Paint | `window.paint_external_texture(bounds, …)` | `window.paint_surface(bounds, pixel_buffer.clone())` |

Also on macOS:
- After `engine.render(…)` submits, call `device.poll(wgpu::PollType::Wait)`.
  The wgpu and GPUI command queues are not ordered, so without it GPUI can
  sample a half-drawn canvas. Note this in the report.
- Render with `wgpu::TextureFormat::Bgra8Unorm` and `Output::Encoded`. GPUI's
  layer is non-sRGB `BGRA8Unorm`, so the encoded values pass straight through.
- `presentation()` should return `"macOS, CAMetalLayer (display-linked)"`.

**The IOSurface target.** For each ring slot, at device-pixel canvas size:

1. Create the pixel buffer: `CVPixelBuffer::new(kCVPixelFormatType_32BGRA, w,
   h, Some(&options))`, where `options` has
   `kCVPixelBufferIOSurfacePropertiesKey → {}` (empty dictionary) and
   `kCVPixelBufferMetalCompatibilityKey → true`.
2. Get its IOSurface with the raw FFI `CVPixelBufferGetIOSurface` and **do not
   release it**. ⚠️ `core-video` 0.5.2's `get_io_surface()` wraps this *Get*
   function with `wrap_under_create_rule`, which over-releases and crashes
   later. Don't use it.
3. Get wgpu's `MTLDevice`:
   `unsafe { gpu.device.as_hal::<wgpu::hal::api::Metal>() }` → `.raw_device()`.
   It is an objc2 `ProtocolObject<dyn MTLDevice>`.
4. Make the Metal texture:
   `newTextureWithDescriptor_iosurface_plane(desc, iosurface, 0)`. The
   descriptor is `BGRA8Unorm`, `Type2D`, w×h, usage `RenderTarget | ShaderRead`,
   storage mode `Shared`. Cast the IOSurface pointer to objc2's `IOSurfaceRef`.
5. Wrap it for wgpu:
   `wgpu::hal::metal::Device::texture_from_raw(raw, Bgra8Unorm,
   MTLTextureType::Type2D, 1, 1, CopyExtent { width, height, depth: 1 })`,
   then `device.create_texture_from_hal::<wgpu::hal::api::Metal>(hal, &desc)`.
   The descriptor has usage `RENDER_ATTACHMENT | TEXTURE_BINDING`.
6. Keep the `CVPixelBuffer` together with its `wgpu::Texture` and view. Use the
   next ring slot each paint, and rebuild the ring when the size changes.

Timing stays as on Linux. A frame runs from the point after the wait to the
same point in the next paint. A stroke's latency ends at the wait that follows
the GPUI frame showing it. GPUI on macOS paints from the display link, so frame
p50 bottoms out at the refresh interval (16.7 ms at 60 Hz). That is the floor,
not a cost; latency and p99 are what matter.

## 3. Verify on the M1

1. `cargo build --release -p vello-canvas-spike`, `cargo fmt --all --check` and
   `cargo clippy --workspace --all-targets --locked -- -D warnings`, as CI runs them.
2. `python3 scripts/check-gpui-vendor.py` and
   `python3 scripts/test-license-staging.py`.
3. `cargo run --release -p emulsion-app`: open an image and check the app still
   draws normally. Its canvas does not use surfaces, but GPUI's renderer
   changed.
4. `$S view spikes/out/layers-4k.ora --gpui` (make test files with `gen` if
   `spikes/out` is missing). Paint with brush A, toggle to brush B and paint,
   right-drag to pan, wheel to zoom, and try Fit and 100%. Save a screenshot to
   `spikes/vello-canvas/results/gpui-embedded-m1.png`.

## 4. Measure on the M1

Close other apps and keep the Mac on power. `--vsync` gives the standalone
window the same display pacing, for a like-for-like comparison:

```sh
S=target/release/vello-canvas-spike; H=$(hostname)
G=spikes/vello-canvas/results/bench-$H-gpui.jsonl
V=spikes/vello-canvas/results/bench-$H-vsync.jsonl
for s in navigate brush-a brush-b; do
  $S bench $s spikes/out/layers-4k.ora --gpui --json $G
  $S bench $s spikes/out/layers-4k.ora --vsync --json $V
done
$S bench vector-edit spikes/out/vectors-500.ora --gpui --json $G
$S bench vector-edit spikes/out/vectors-500.ora --vsync --json $V
```

Commit the code, the two JSONL files and the screenshot, then push to
`claude/wgpu-vello-canvas-spike-ccv7cj`. `RESULTS.md` gets written up from
them afterwards.

## Out of scope

- Sharing one `MTLDevice` or command queue between wgpu and GPUI.
- `MTLSharedEvent` synchronisation instead of the CPU waits.
- Intel Macs with two GPUs.
- The Emulsion app's canvas.
