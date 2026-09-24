# Emulsion changes to gpui-pre-windows 0.3.5

The published package remains the baseline. Its upstream license and provenance
files are retained. Modified source files carry an Emulsion notice.

- `src/directx_devices.rs`: attempt non-software DXGI adapters first, then obtain
  WARP explicitly through `EnumWarpAdapter` after enumeration ends or compatible
  hardware creation fails. Unreadable adapter descriptions are skipped; an
  enumeration error is logged and still permits WARP fallback. Preserve
  upstream D3D feature checks and recovery.
  `GPUI_FORCE_SOFTWARE_RENDERING=1` skips hardware for reproducible diagnostics;
  every other value keeps automatic hardware-first selection.
- `src/directx_renderer.rs`: diagnostics use the same software-device classifier
  as adapter selection, including Microsoft Basic adapter names.
- `src/shaders.hlsl`: clamp polychrome image samples to their atlas tile's outer
  texel centers. Linear filtering previously sampled neighboring allocations
  along enlarged image edges, exposing faint seams between canvas tiles.
  Clamping after interpolation preserves the interior image scale and applies
  to cropped image tiles as well. Glyph and path sampling remain unchanged.
  `src/directx_image_sampling_tests.rs` renders the shipping image shaders on
  WARP and reads pixels back, covering magnified edges, unchanged interior
  interpolation, single-texel crops and alpha at the atlas boundary. An
  in-memory shader with the old sampling behavior verifies the regression.
- `src/rendering_policy.rs`, `src/vsync.rs`, `src/gpui_windows.rs`: software
  rendering uses a sleeping scheduler capped at 30 Hz. Hardware keeps upstream
  DwmFlush behavior. Selection/recovery updates the policy. Forced OS paints
  are not rate-limited; this is not a total CPU or frame-rate guarantee.

The software scheduler adapts the Apache-2.0 GPUI changes in AgentOps
`src-gpui/vendor/gpui/src/rendering_policy.rs` and
`src-gpui/vendor/gpui/src/platform/windows/vsync.rs`. No AgentOps application
source was copied. The software adapter name checks also account for Microsoft's
Basic adapters whose DXGI software flag may be absent. Virtual GPU names alone
are not treated as software.

Six dependency-free policy tests cover hardware retry/early success, empty or
failed hardware fallback, preserved fallback errors, forced-software bypass of
enumeration, adapter classification and pacing. They can run on any host:

```
rustc --edition=2024 --test vendor/gpui/gpui-pre-windows/src/rendering_policy.rs -o /tmp/emulsion-windows-render-policy-tests
/tmp/emulsion-windows-render-policy-tests
```

Native validation must additionally cover WARP startup, hardware-first startup,
forced software startup, resize, window/input responsiveness and device recovery
on Windows. These changes do not provide a software Metal implementation on
macOS and do not port AgentOps' larger DirectX shader/path-cache optimizations.

Image sampling validation (2026-09-23): the native WARP regression passed with
shipping vertex/pixel shaders. Its unclamped control reproduced atlas bleed
(corner RGBA `[1, 0, 0.68359375, 0.31640625]`); the fixed corner was `[1, 0, 0, 1]`.
Interior interpolation, single-texel crops, and translucent atlas-edge crops also
passed. Optimized VS/PS 4.1 compilation, backend clippy with tests, and the app
release build passed. Logs: `target/windows-tile-seams-{tests,clippy,release-build}.log`.
This is native offscreen validation, not a captured before/after of the user's document.
