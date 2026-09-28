# Rendering and virtual machines

The app prefers a working hardware graphics adapter. On Linux, GPUI tries
surface-compatible adapters and can fall back to a CPU software driver. On
Windows it tries hardware adapters, then explicitly requests Windows WARP.
Software-only Linux device recovery now follows the same policy as startup.

This renderer draws the GPUI interface and presents image tiles. A separate
wgpu compute device now accelerates selected document compositing and filters,
with the CPU engine retained for fallback, document storage, and undo. See
[GPU image processing](gpu-rendering.md) for coverage, performance routing, and
experimental brush/viewport paths. GPUI software rendering and image-processing
CPU fallback are independent.

## Systems without a hardware GPU

- **Linux:** a working X11 or Wayland display server, Vulkan loader, and a
  software implementation such as Mesa Lavapipe are required. On Ubuntu/Debian
  the driver is provided by `mesa-vulkan-drivers`; on Arch it is `vulkan-swrast`.
  Install drivers through the host/distribution, not by copying arbitrary shared
  libraries into the application package.
- **Windows:** WARP is the software fallback. Its device must pass the same
  feature checks as hardware. Software rendering uses a sleeping 30 Hz scheduler;
  hardware keeps upstream pacing. Windows graphics APIs are still required.
- **macOS:** the native Metal backend is unchanged. A VM without a working Metal
  device is not covered by the Linux/Windows software fallback. No macOS CPU
  rendering backend is introduced here.

References: [Microsoft WARP documentation](https://learn.microsoft.com/en-us/windows/win32/direct3darticles/directx-warp),
[Arch's vulkan-swrast package](https://archlinux.org/packages/extra/x86_64/vulkan-swrast/).

Software windows avoid continuously presenting unchanged scenes after rapid
pointer input. Dirty scenes and required presentation requests are still drawn.
The app logs the selected adapter name and whether it is software-rendered.

## Verify the software renderer

Build the real-window smoke program:

```sh
cargo build --locked -p emulsion-app --example renderer_smoke
```

In a graphical Linux session with Lavapipe installed:

```sh
GPUI_FORCE_SOFTWARE_RENDERING=1 target/debug/examples/renderer_smoke --require-software
```

Mesa's OpenGL software driver is another option when available: add
`LIBGL_ALWAYS_SOFTWARE=1` to that command to select llvmpipe. This path was
validated locally alongside normal Intel Vulkan rendering; both completed the
two-frame smoke test. Lavapipe is selected explicitly by the Linux CI test.

In an interactive Windows PowerShell session:

```powershell
$env:GPUI_FORCE_SOFTWARE_RENDERING = '1'
.\target\debug\examples\renderer_smoke.exe --require-software
Remove-Item Env:GPUI_FORCE_SOFTWARE_RENDERING
```

The override is active only for the exact value `1`. It deliberately fails when
no compatible software adapter exists instead of silently selecting hardware.
It applies to Linux/wgpu and Windows, not native macOS Metal. Unset it for normal
hardware-first operation. Existing `ZED_DEVICE_ID` selection remains an explicit
priority override on the wgpu path, subject to the software-only diagnostic flag.

The smoke program creates a real window, verifies the reported adapter type,
draws text and styled geometry over two frames, and exits. A watchdog fails hangs.
It exercises device setup and drawing/presentation, not screenshot pixel parity
or simulated device loss. CI runs it under Xvfb with the Lavapipe ICD selected
explicitly; portable policy tests cover selection order and frame decisions.
