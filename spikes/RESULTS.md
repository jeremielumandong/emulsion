# Phase 0 spike results

Machine: Arch Linux (kernel 7.2), Intel Iris Plus Graphics G7 (ICL GT2, Mesa, Vulkan 1.4),
Apple T2 Mac hardware, Wayland/Hyprland. Rust 1.98. Claude Code 2.1.273 on PATH.

Note for spike 4: this GPU has no CUDA/ROCm. `ort` will run on CPU or the OpenVINO execution
provider here; the GPU paths get measured on other hardware.

| # | Spike | Status | Result | Decision |
|---|---|---|---|---|
| 1 | Tiled `paint_image` viewport at 60 fps | partial | Pan and zoom repaint from cached tiles. Re-rendering a full 1440×900 view of a 6000×4000, 7-node document on the CPU compositor takes 52–75 ms for 24 tiles (8 threads, Iris Plus G7), about 15 fps during slider drags. Frame time of the GPUI present itself not yet instrumented. | Plan A (tiles + `paint_image`) with the CPU compositor (Plan C) for Phase 1. GPU compositor next. |
| 2 | gpui-kit widgets: gaps for color picker, curves | pending | | |
| 3 | Tablet pressure on Wayland via GPUI | pending | | |
| 4 | `ort` execution provider on this machine | pending | | |
| 5 | Local diffusion inpaint speed | pending | | |
| 6 | wgpu device alongside GPUI's renderer | started | gpui-pre-platform 0.3.5 on Linux uses `gpui-pre-wgpu` (wgpu 29.0.4), not Blade. Workspace pins wgpu 29 so one copy is linked. Device sharing not yet attempted. | |
| 7 | Claude Code + `emulsion mcp-serve` image content | done | Claude Code 2.1.273 runs with no built-in tools and only Emulsion's MCP server. It calls `describe_document`, reads `get_view` PNG image blocks, and describes the canvas correctly. Edits reach the host as confirmations only with `--permission-prompts host` plus the stdio prompt tool; `host` alone denies them. Edit turn 6.3 s / $0.035, look turn 3.6 s / $0.050. | CLI transport adopted. |
| 8 | `agentops-core` as a dependency | decided without spike | A public repository cannot depend on a local path, and the crate brings tokio, reqwest and PTY support. | Ported the stream-json protocol, argv conventions and process-group handling into `emulsion-assistant` (about 700 lines). |
| 9 | Jev accuracy on editor intents | blocked | No TypeSafe key on this machine. Client and question shapes are tested against a mock server. | Run with a key via Settings → test, then a request corpus. |
| 10 | Film-simulation base look fidelity | pending | | |
| 11 | Import analysis latency | pending | | |
| 12 | Two-stack before/after cost | pending | | |

## Findings

- GPUI 0.3.5 samples every image with linear filtering (`gpui-pre-wgpu`, `FilterMode::Linear`).
  Crisp pixels at 200 % and above, and view rotation, therefore use a CPU screen-space path:
  one device-sized image built from cached tiles with nearest sampling.
- Tiles replaced in the atlas must be evicted with `Window::drop_image`, or GPU memory grows
  with every edit. The tile cache queues replaced images and drops them during paint.
- Document revisions are not monotonic under undo, so render caches key on a separate,
  always-increasing generation counter.
- Pixels created inside Emulsion in an 8-bit document must be stored at 16 bits when saved;
  8-bit storage re-quantises them and the composite drifts by one code value.

## Headless timings (release, 6000×4000, 7 nodes)

| Operation | Time |
|---|---|
| Fit view, 24 tiles at level 2 | 58 ms |
| 100 % view, 24 tiles at level 0 | 52 ms |
| Full flatten, 24 MP | 0.9 s |
| Save ORA (121 MB) | 2.5 s |
| Open ORA | 0.46 s |

Reproduce: `cargo run --release -p emulsion-io --example sample -- spikes/out/sample.ora`

## Assistant findings

- Headless `claude -p` over stream-json only routes confirmations to the host when
  `--permission-prompt-tool stdio` is given together with `--permission-prompts host`.
- Assistant text arrives as whole messages; consecutive messages separated by tool calls need
  an explicit break in the UI.
- The offline planner must refuse compound or visual clauses. Without a guard, "the top node
  and the one under it should be invisible; the third should be called Sky" became a confident
  but wrong single hide.
- Closing an input overlay must return focus to the canvas, or every shortcut stops working
  until the next click (caught by a headless UI test).

Reproduce the live checks:

```sh
cargo build -p emulsion-app
cargo run -p emulsion-assistant --example smoke -- target/debug/emulsion
cargo test -p emulsion-ui assistant_turn_through_the_ui -- --ignored --nocapture
```

## Log

- 2026-09-18 — workspace scaffolded; editor shell zones; `mcp-serve` answers `initialize`,
  `tools/list`, `tools/call`, `ping` with an empty tool set.
- 2026-09-18 — `cargo tree -i wgpu@29` shows `gpui-pre-linux → gpui-pre-wgpu → wgpu 29`. GPUI's
  Linux renderer is wgpu, so the viewport fallback plan becomes "share GPUI's wgpu device"
  rather than importing an external Vulkan image.
