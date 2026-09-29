# Emulsion changes to gpui-pre-apple 0.3.5

These changes remain under Apache-2.0. Original source notices are retained.

- `src/metal_renderer.rs`, `src/shaders.metal`: draw `PaintSurface`s whose
  `CVPixelBuffer` is single-plane `kCVPixelFormatType_32BGRA`, with a new
  `surface_bgra_fragment` entry and `surfaces_bgra` pipeline that pass the
  texture through unchanged. The pipeline is chosen per surface, so video
  (bi-planar YCbCr) and BGRA surfaces can share a frame; other pixel formats
  are logged once and skipped instead of asserting.
- `src/metal_renderer.rs`, `src/gpui_apple.rs`: count window frames committed
  to the GPU and frames the GPU has finished, and export
  `wait_for_submitted_frames()`, which blocks until every committed frame has
  finished. Headless renders are not counted.

Both are used by `spikes/vello-canvas` (macOS embedding spike), which renders
its canvas on its own wgpu device into an IOSurface-backed texture and paints
it with `Window::paint_surface`. The application does not paint BGRA surfaces
yet.

Archive and upstream revision information are in `../UPSTREAM.json`.
