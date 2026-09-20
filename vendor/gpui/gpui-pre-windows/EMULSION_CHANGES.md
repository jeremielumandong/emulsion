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
