# Emulsion changes to gpui-pre-wgpu 0.3.5

These changes remain under Apache-2.0. Original source notices are retained.

- `src/gpui_wgpu.rs`, `src/adapter_policy.rs`, `src/wgpu_context.rs`: prefer
  hardware to a software compositor's adapter, preserve explicit device-ID
  selection, and add `GPUI_FORCE_SOFTWARE_RENDERING=1` for software-only testing.
  Report missing software-driver support instead of silently using hardware.
- `src/wgpu_renderer.rs`: device-loss recovery uses the same hardware-first,
  software-capable selection as startup. Software-only VMs can recover instead
  of repeatedly rejecting their only available adapter.

Linux still needs a functioning display server and Vulkan/OpenGL implementation.
For CPU-only Vulkan rendering, install Mesa Lavapipe. This does not implement a
graphics-driver-free raster UI backend. The native macOS Metal renderer is unchanged.

Archive and upstream revision information are in `../UPSTREAM.json`.
