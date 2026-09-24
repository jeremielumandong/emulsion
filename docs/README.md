# Emulsion documentation

The top-level [README](../README.md) is the product guide: what Emulsion does,
how to build and install it, and a reference for every editor feature. The
website in [`site/`](../site/README.md) carries the illustrated getting-started
guide. This folder holds everything that does not fit on those two pages,
listed here by the reader it serves. Pages in the last section are dated
snapshots; each opens with a banner naming the page that describes current
behavior.

## Using Emulsion

- [Files, folders and environment](files-and-environment.md): the command line,
  where settings, shortcuts, autosaves and downloads live, how to capture logs,
  and every environment variable and installer flag.
- [Troubleshooting](troubleshooting.md): symptom-first fixes for start-up,
  rendering, converters, RAW, assistant and theme problems.
- [Rendering and virtual machines](rendering.md): how the window renderer picks
  an adapter and how to run on a machine without a GPU.
- [GPU image processing](gpu-rendering.md): what `emulsion-gpu` accelerates,
  the `EMULSION_GPU` and `EMULSION_GPU_BRUSHES` controls, and how it is verified.
- [Brush Library and Brush Studio](brush-workflow.md): organising, editing,
  importing and exporting brushes.
- [Brush files and conversion limits](brush-import-formats.md): which brush
  package formats import and what is lost in conversion.
- [Moving artwork](artwork-movement.md) and [Aligning artwork](artwork-alignment.md):
  the Move tool, nudging, and the Align controls.
- [Experimental Nikon HE/HE★ support](nikon-he.md): the pinned decoder, its
  limits, and the opt-in test.
- [Camera Raw 3 reference: gap assessment](camera-raw-3-gap.md): Emulsion's RAW
  controls compared feature by feature against a Camera Raw 3 reference.

## Assistant and MCP

The MCP tool catalog is defined in `crates/emulsion-mcp/src/tools.rs` together
with `brush_catalog.rs`, `brush_assets.rs`, `raw_tools.rs` and `raw_preview.rs`;
a connected client lists the current catalog with the MCP `tools/list` request.
The README section [MCP tools for editing and automation](../README.md#mcp-tools-for-editing-and-automation)
summarises what the tools cover.

- [RAW tools over MCP](raw-mcp.md): the RAW development tools, their arguments
  and a worked session.
- [Brush Library and Brush Studio through MCP](brush-mcp.md): the brush
  catalog and asset tools with `tools/call` examples.

## Contributing

- [CONTRIBUTING](../CONTRIBUTING.md): bug reports, pull requests and the local
  validation commands that mirror CI.
- [Tool behavior tests](tool-testing.md): the headless GPUI test suite and which
  test covers which editor behavior.
- [RAW corpus tests](../crates/emulsion-io/tests/fixtures/RAW-CORPUS.md): the
  opt-in decode and export tests against public CC0 RAW samples.
- [Vendored GPUI](../vendor/gpui/README.md): why GPUI is vendored, what is
  patched, and the license and renderer-policy checks.
- [Cross-frame layout reuse experiment](layout-reuse-experiment.md): how the
  retained-layout path works and why it is on by default.
- [Drawing preview and tile boundaries](drawing-preview.md): how pointer
  samples become published brush tiles during a stroke.
- [Making GPU brushes faster](gpu-brush-performance.md): the current GPU brush
  hook, its measured cost, and the persistent dab pipeline.
- [Performance strategy](performance-strategy.md): the open-document lifecycle
  and the ordered performance backlog.
- [Architecture decision records](adr/0000-template.md): the ADR template. No
  records exist yet; decisions to date are recorded in the design notes above
  and the reports below.

## Reports, audits and plans (dated snapshots)

Each page records the state on its date and is not updated afterwards.

| Page | Date | Topic |
| --- | --- | --- |
| [Phase 0 spike results](../spikes/RESULTS.md) | 2026-09-18 | Twelve feasibility spikes (tiled viewport, wgpu beside GPUI, assistant over MCP, dependencies); several still pending |
| [Common-tool audit](tool-audit-2026-09-19.md) | 2026-09-19 | Baseline audit of the twelve toolbar tools |
| [Tool repair plan](tool-repair-plan.md) | 2026-09-19 | Fixes and remaining manual checks following the tool audit |
| [Artist workflow evaluation](artist-evaluation.md) | 2026-09-19 | Evaluation protocol for artistic output (protocol, not results) |
| [Adaptable artist workflow](artist-workflow/shape.md) | 2026-09-19 | Contract for the style-expansion feature work |
| [Adaptable artist workflow report](artist-workflow/report.md) | 2026-09-19 | Outcome and verification evidence for that work |
| [Photographer workflow recipes](photography-workflow-plan.md) | 2026-09-20 | Plan for capturing and reusing adjustments as recipes |
| [Brush Library and Brush Studio plan](brush-library-studio-plan.md) | 2026-09-22 | Gap assessment and implementation plan for the brush system |
| [Brush implementation validation](brush-validation.md) | 2026-09-22 | Stroke timing measurements for the brush engine |
| [Multi-vendor RAW pipeline backlog](raw-pipeline-backlog.md) | 2026-09-22 | RAW pipeline audit with prioritised backlog and implementation updates |
| [Assistant composition workflow](assistant-composition-workflow.md) | 2026-09-23 | Workflow ideas for assistant-driven composition |
| [Assistant drawing and MCP reliability pass](assistant-drawing-pass.md) | 2026-09-23 | Rendering and tool-use defects fixed in the assistant drawing path |
| [GPUI CPU review](gpui-cpu-review.md) | 2026-09-23 | Source audit of vendored GPUI CPU cost |
| [Photo/Draw switch pauses](photo-draw-switch-performance.md) | 2026-09-23 | Analysis of the workspace switch stall |
| [Release layout reuse measurements](layout-reuse-release-results.md) | 2026-09-23 | Cold versus retained layout benchmarks |
| [Intrinsic text release measurements](intrinsic-text-release-results.md) | 2026-09-23 | Text layout specialisation benchmarks |
| [Canvas navigation release measurements](canvas-navigation-release-results.md) | 2026-09-23 | Pan and zoom notification benchmarks |
| [Live layout setting comparison](live-layout-toggle-results.md) | 2026-09-23 | Manual sample of a running editor with layout reuse off and on |

The `.json` files beside the measurement pages hold the raw benchmark output
those pages summarise.
