# Emulsion changes to gpui-pre-wgpu 0.3.5

These changes remain under Apache-2.0. Original source notices are retained.

- `src/gpui_wgpu.rs`, `src/adapter_policy.rs`, `src/wgpu_context.rs`: prefer
  hardware to a software compositor's adapter, preserve explicit device-ID
  selection, and add `GPUI_FORCE_SOFTWARE_RENDERING=1` for software-only testing.
  Report missing software-driver support instead of silently using hardware.
- `src/wgpu_renderer.rs`: device-loss recovery uses the same hardware-first,
  software-capable selection as startup. Software-only VMs can recover instead
  of repeatedly rejecting their only available adapter.
- `src/wgpu_context.rs`: request WebGPU default limits when the adapter has them
  (otherwise the previous downlevel limits), and `TEXTURE_FORMAT_16BIT_NORM` /
  `TEXTURE_ADAPTER_SPECIFIC_FORMAT_FEATURES` when the adapter has them, so an
  application canvas can run on GPUI's device. Publish that device through
  `shared_gpu()` with a generation that changes on device-loss recovery.
- `src/wgpu_renderer.rs`, `src/shaders.wgsl`: draw `PaintSurface` external
  textures (a `wgpu::TextureView` from GPUI's device) with the existing surface
  layout and a new `fs_external` fragment entry. Used by `spikes/vello-canvas`
  (Linux embedding spike); the application does not paint external textures yet.

Linux still needs a functioning display server and Vulkan/OpenGL implementation.
For CPU-only Vulkan rendering, install Mesa Lavapipe. This does not implement a
graphics-driver-free raster UI backend. The native macOS Metal renderer is unchanged.

Archive and upstream revision information are in `../UPSTREAM.json`.
