# Windows canvas loading — 2026-09-29

Measured on an AMD Radeon RX 7700 XT, Windows x64, DirectX 12, driver
32.0.31041.1004, with release builds based on `7f6251d` plus these changes.

## Causes and changes

- Windows builds omitted `dxcompiler.dll`. wgpu's automatic compiler selection
  therefore used FXC. The Windows build script now downloads Microsoft's pinned
  DXC 1.8.2505.1 archive, verifies SHA-256, and stages the compiler, validator,
  and notices. CI tests staging; the release workflow requires the files before
  packaging. The installer already includes DLLs and the license directory.
- Every Windows canvas created a D3D12 device, losing wgpu's per-device compiled
  shader cache. The host now retains one device per hosting thread, keyed by
  GPUI's adapter LUID and requested tile format. Lost devices and changed keys
  replace the cached device. Document engines and textures remain independent.
- Raster-only canvases created Vello even without vector content. Vello now
  initializes on the first vector or nonempty HUD render. Adding text later and
  removing it still produces the expected image.

## Measurements

The `windows_canvas_startup_benchmark` constructs a 3840×2160 document and renders
its first 1280×720 frame, including device acquisition, engine initialization,
submission and GPU completion. Raster content is one solid photo layer; vector
content is editable text. This isolates startup costs, not complex SVG parsing,
file decoding, UI layout, or physical display latency.

Each workload uses the order fresh device, first cached device, warm cached
device, fresh device, warm cached device. Each render's pixels must match the
fresh-device reference. All times below are milliseconds; these are individual
samples, not statistically established application-wide speedups.

| Workload | FXC, fresh device | DXC, fresh device | DXC, first cached device | DXC, warm cached device |
| --- | --- | --- | --- | --- |
| Raster | 8530.62 / 6935.58 | 1449.24 / 631.64 | 1486.51 | 41.30 / 40.93 |
| Vector text | 11539.95 / 8805.38 | 2119.44 / 854.15 | 2006.35 | 63.55 / 64.08 |

FXC also benefits from device reuse: warm raster samples were 41.64 / 53.82 ms,
and warm vector samples were 61.71 / 62.16 ms. Bundling DXC addresses cold loading;
device reuse avoids repeatedly paying compilation costs as documents open.

A separate paired raster test explicitly restores eager Vello creation to compare
the work removed by lazy initialization. With the bundled DXC it measured
1734.34 / 1777.46 ms eager versus 1161.96 / 1154.08 ms lazy, using fresh devices.

## Validation and reproduction

The Windows device-cache regression passes for reuse across closed tabs, changed
tile-format requests, device destruction/recovery, and mismatched adapter IDs.
GPU regressions cover raster pixels, text added after raster initialization,
text removal, nonempty HUD initialization, and HUD removal. Both FXC and the
bundled DXC pass the startup comparison and pixel checks. DXC DLLs retain valid,
timestamped Microsoft signatures.

```powershell
powershell -NoProfile -ExecutionPolicy Bypass -File scripts/test-windows-dxc.ps1
$env:PATH = "$PWD/target/release;" + $env:PATH
$env:WGPU_BACKEND = 'dx12'
$env:WGPU_DX12_COMPILER = 'dxc' # Repeat with 'fxc' for the fallback comparison.
cargo test --release --locked -p emulsion-engine --features gpui canvas_ -- --ignored --test-threads=1 --nocapture
```

Build with `scripts/build-windows.ps1` first to stage DXC beside the application.
Local raw logs are `target/windows-canvas-{dxc,fxc}-validation.log` and
`target/windows-canvas-host-test-build.log`. The first document of each process
still needs shader initialization; device reuse is within one process, not a
persistent disk cache. Retaining the device may retain driver/allocator capacity;
the cache does not retain document engines or document texture references.
