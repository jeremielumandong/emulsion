# Phase 0 spike results

Machine: Arch Linux (kernel 7.2), Intel Iris Plus Graphics G7 (ICL GT2, Mesa, Vulkan 1.4),
Apple T2 Mac hardware, Wayland/Hyprland. Rust 1.98. Claude Code 2.1.273 on PATH.

Note for spike 4: this GPU has no CUDA/ROCm. `ort` will run on CPU or the OpenVINO execution
provider here; the GPU paths get measured on other hardware.

| # | Spike | Status | Result | Decision |
|---|---|---|---|---|
| 1 | Tiled `paint_image` viewport at 60 fps | pending | | |
| 2 | gpui-kit widgets: gaps for color picker, curves | pending | | |
| 3 | Tablet pressure on Wayland via GPUI | pending | | |
| 4 | `ort` execution provider on this machine | pending | | |
| 5 | Local diffusion inpaint speed | pending | | |
| 6 | wgpu device alongside GPUI's renderer | started | gpui-pre-platform 0.3.5 on Linux uses `gpui-pre-wgpu` (wgpu 29.0.4), not Blade. Workspace pins wgpu 29 so one copy is linked. Device sharing not yet attempted. | |
| 7 | Claude Code + `emulsion mcp-serve` image content | pending | | |
| 8 | `agentops-core` as a dependency | pending | | |
| 9 | Jev accuracy on editor intents | pending | | |
| 10 | Film-simulation base look fidelity | pending | | |
| 11 | Import analysis latency | pending | | |
| 12 | Two-stack before/after cost | pending | | |

## Log

- 2026-09-18 — workspace scaffolded; editor shell zones; `mcp-serve` answers `initialize`,
  `tools/list`, `tools/call`, `ping` with an empty tool set.
- 2026-09-18 — `cargo tree -i wgpu@29` shows `gpui-pre-linux → gpui-pre-wgpu → wgpu 29`. GPUI's
  Linux renderer is wgpu, so the viewport fallback plan becomes "share GPUI's wgpu device"
  rather than importing an external Vulkan image.
