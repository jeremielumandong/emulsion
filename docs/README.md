# Emulsion documentation

The top-level [README](../README.md) is the product overview and feature
reference: what Emulsion does, how to build and install it, and what every
editor feature covers. The website's illustrated getting-started guide lives in
[site/index.html](../site/index.html) (the `#guide-start` section); the
[website source](../site/README.md) explains how the site is built. This folder
holds everything else:

```text
docs/
├── guides/      how to use Emulsion
│   └── mcp/     driving Emulsion through MCP tools
├── technical/   architecture, internals and contributor reference
│   └── adr/     architecture decision records
└── specs/       plans, specifications and checklists
    └── reports/ dated snapshots, audits and raw measurement data
```

## Guides

### Getting set up

- [Files, folders and environment](guides/files-and-environment.md): the command
  line, where settings, shortcuts, autosaves and downloads live, how to capture
  logs, and every environment variable and installer flag.
- [Rendering and virtual machines](guides/rendering.md): how the window renderer
  picks an adapter and how to run on a machine without a GPU.
- [Troubleshooting](guides/troubleshooting.md): symptom-first fixes for start-up,
  rendering, converters, RAW, assistant and theme problems.
- [Cloud accounts and developer registrations](guides/cloud-setup.md):
  experimental project sync, selected-photo imports, and local OAuth
  registrations for open-source builds.

### Photos and RAW

- [Library and Develop](guides/library-develop.md): the Library and Develop
  layout, catalog workflow, RAW persistence, preset interchange and MCP coverage.
- [RAW development controls and limits](guides/raw-development.md): the RAW
  controls, how processing and ownership work, interoperability, and current
  limits.
- [Experimental Nikon HE/HE★ support](guides/nikon-he.md): the pinned decoder,
  its limits, and the opt-in test.
- [Smart Object source editing and links](guides/smart-object-sources.md):
  editing a layered Smart source, external links, and resource limits.
- [Printing](guides/printing.md): shared print preview, paper and layout
  controls, native printer connections and validation limits.

### Drawing and brushes

- [Brush Library and Brush Studio](guides/brush-workflow.md): organising,
  editing, importing and exporting brushes.
- [Brush files and conversion limits](guides/brush-import-formats.md): which
  brush package formats import and what is lost in conversion.
- [Moving artwork](guides/artwork-movement.md): the Move tool, dragging,
  constraining and nudging.
- [Aligning artwork](guides/artwork-alignment.md): the Align controls for canvas
  and selection targets.

### Design

- [Design starters, responsive layout and canvas shortcuts](guides/design-starters-and-layout.md):
  the supplied template library, responsive frames, reusable formatting and bulk
  creation.
- [Responsive starter layouts](guides/design-responsive-starters.md): the native
  responsive starter category.
- [Canvas width breakpoints](guides/design-layout-breakpoints.md): previewing a
  page at desktop, tablet, phone or custom widths.
- [Design appearance controls](guides/design-appearance.md): fill, gradient,
  stroke and effect controls in the Appearance row.
- [Reusable appearance styles](guides/design-styles.md): saved styles and their
  linked objects.
- [Reusable components](guides/design-components.md): components, linked
  instances, variants and preserved properties.
- [Design variables](guides/design-variables.md): typed colors and numbers bound
  to native object properties.
- [Native text lists, paragraph spacing and decorations](guides/design-text-formatting.md):
  underline, strikethrough, lists and paragraph spacing.
- [Native vector editing and precision](guides/design-vector-editing.md): point
  editing, Boolean operations, skew and envelope.
- [Native charts and tables](guides/design-charts.md): local data authoring and
  editable chart artwork.
- [Local CSV design generation](guides/design-data-bindings.md): binding CSV
  columns to text and images and generating record sets.
- [Portable typography, palettes and asset folders](guides/design-portable-brands.md):
  the Brand drawer, typography roles and palette collections.
- [Design AI capabilities and availability](guides/design-ai-capabilities.md):
  which assistant and image workflows apply to Design and how generated pixels
  stay separate.
- [Interactive presentations](guides/design-interactions.md): click actions,
  navigation, overlays and component states.
- [Design presentations](guides/design-presentation.md): presenter window,
  speaker notes and transitions.
- [Local video, audio and property keyframes](guides/design-local-media-keyframes.md):
  embedded media, trimming and property animation.
- [Advanced motion and interchange](guides/design-advanced-motion.md): motion
  presets, retiming and animation export.
- [YouTube in Design presentations](guides/design-video.md): video objects,
  presentation playback and system runtime requirements.
- [Selection and frame export](guides/design-selection-export.md): exporting
  selected objects or frames as SVG, PDF or PNG.
- [Standalone HTML presentations](guides/design-html-export.md): interactive
  responsive HTML export.
- [Editable PowerPoint presentations](guides/design-pptx.md): `.pptx` import and
  export and their supported subset.
- [Editable Lottie interchange](guides/lottie-interchange.md): Lottie import,
  editable vector export and rendered-frame export.
- [Portable Design templates and Diagram stencils](guides/template-pack-format.md):
  authoring, exporting and installing template and stencil packs.

### Diagrams

- [Diagram functionality and compatibility](guides/diagram-functionality.md):
  diagram editing, scalable rendering, draw.io and Visio import/export, local
  packs and sample coverage.

## MCP and the assistant

The MCP catalog is assembled in `crates/emulsion-mcp/src/tools.rs` from the
`*_tools.rs` and related modules beside it; a connected client lists the live
catalog with `tools/list`. The README section
[MCP tools for editing and automation](../README.md#mcp-tools-for-editing-and-automation)
summarises what the tools cover.

- [Native editor, photo and print workflows](guides/mcp/mcp-editor-print-photo.md):
  editor state, photo and print tools addressed to the originating tab.
- [Native workspace and canvas controls](guides/mcp/mcp-native-controls.md):
  toolbar, menu, sidebar and layout presets.
- [Creative catalog and workspace](guides/mcp/mcp-creative-workspace.md): the
  local asset catalog, brand kits and collections.
- [RAW tools over MCP](guides/mcp/raw-mcp.md): the RAW development tools, their
  arguments and a worked session.
- [Brush Library and Brush Studio through MCP](guides/mcp/brush-mcp.md): the
  brush catalog and asset tools with `tools/call` examples.
- [Design assets](guides/mcp/mcp-design-assets.md): components, saved styles,
  charts and tables.
- [Design appearance and layout](guides/mcp/mcp-design-appearance.md): appearance
  and responsive layout commands.
- [Native vectors and precision](guides/mcp/mcp-design-vectors.md): point
  editing and path operations.
- [Design presentation tools](guides/mcp/mcp-design-presentation.md): notes,
  transitions, animations and video.
- [Native diagrams](guides/mcp/mcp-diagrams.md): shapes, connectors, ports and
  containers in the structured diagram graph.

## Technical

### Contributing

- [CONTRIBUTING](../CONTRIBUTING.md): bug reports, pull requests and the local
  validation commands that mirror CI.
- [Tool behavior tests](technical/tool-testing.md): the headless GPUI test suite
  and which test covers which editor behavior.
- [Releases](technical/releases.md): preparing a version, Linux downloads, and
  signed Windows and macOS builds.
- [RAW corpus tests](../crates/emulsion-io/tests/fixtures/RAW-CORPUS.md): the
  opt-in decode and export tests against public CC0 RAW samples.
- [Vendored GPUI](../vendor/gpui/README.md): why GPUI is vendored, what is
  patched, and the license and renderer-policy checks.

### Rendering and performance

- [GPU image processing](technical/gpu-rendering.md): what `emulsion-gpu`
  accelerates, the `EMULSION_GPU` and `EMULSION_GPU_BRUSHES` controls, and how
  it is verified.
- [Drawing preview and tile boundaries](technical/drawing-preview.md): how
  pointer samples become published brush tiles during a stroke.
- [Making GPU brushes faster](technical/gpu-brush-performance.md): the current
  GPU brush hook, its measured cost, and the persistent dab pipeline.
- [Cross-frame layout reuse experiment](technical/layout-reuse-experiment.md):
  how the retained-layout path works and why it is on by default.
- [Performance strategy](technical/performance-strategy.md): the open-document
  lifecycle and the ordered performance backlog.
- [Responsive layout performance](technical/design-layout-performance.md): which
  Design edits reflow only affected frames and which need a full layout pass.

### Interchange

- [Design interchange acceptance matrix](technical/design-interchange-matrix.md):
  what each export format carries from a native project and how losses are
  reported.

### Decisions

- [Architecture decision records](technical/adr/0000-template.md): the ADR
  template. No records exist yet.

## Specs and plans

Specs describe intended behaviour at their date; guides describe current
behaviour.

| Page | Date | Topic |
| --- | --- | --- |
| [Artist workflow evaluation](specs/artist-evaluation.md) | 2026-09-19 | Evaluation protocol for artistic output (protocol, not results) |
| [Adaptable artist workflow](specs/artist-workflow/shape.md) | 2026-09-19 | Contract for the style-expansion feature work |
| [Adaptable artist workflow report](specs/artist-workflow/report.md) | 2026-09-19 | Outcome and verification evidence for that work |
| [Tool repair plan](specs/tool-repair-plan.md) | 2026-09-19 | Fixes and remaining manual checks following the common-tool audit |
| [Photographer workflow recipes](specs/photography-workflow-plan.md) | 2026-09-20 | Plan for capturing and reusing adjustments as recipes |
| [Brush Library and Brush Studio plan](specs/brush-library-studio-plan.md) | 2026-09-22 | Gap assessment and implementation plan for the brush system |
| [Multi-vendor RAW pipeline backlog](specs/raw-pipeline-backlog.md) | 2026-09-22 | RAW pipeline audit with prioritised backlog and implementation updates |
| [Local Design, Diagram and editor redesign](specs/design-and-diagram-plan.md) | 2026-09-27 | Scope and decisions for the Design and Diagram workspaces and editor redesign |
| [Emulsion Design functionality plan](specs/design-functionality-plan.md) | 2026-09-27 | Remaining local Design capabilities and what counts as a complete workflow |
| [Print dialog design](specs/print-dialog-plan.md) · [interactive concept](specs/print-dialog-prototype.html) | 2026-09-27 | Shared photo/design print flow, printer connections, physical layout and delivery plan |
| [Cloud project sync and photo sources](specs/cloud-sync-plan.md) | 2026-09-27 | Feasibility and phased plan for Google Drive, Google Photos, Dropbox and OneDrive |
| [Cloud integration specification](specs/cloud-sync-spec.md) | 2026-09-27 | Implementation contract, acceptance criteria and release gates |
| [Local Design completion worklist](specs/design-completion-worklist.md) | 2026-09-28 | Remaining Design roadmap items and Linux acceptance results |
| [Local extension workflow design](specs/design-extension-workflows.md) | 2026-09-28 | Validated template, stencil, brand and font packages without executable content |
| [Design platform acceptance](specs/design-platform-acceptance.md) | 2026-09-28 | Windows/macOS runtime and file-exchange checklist |

## Reports

Latest cross-workload measurements: [macOS photo, diagram, paint and layout baseline](specs/reports/macos-performance-suite.md) (2026-09-28, revision `782da4d`, Apple M1).

Each report records the state on its date and is not updated afterwards; most
open with a banner naming the page that describes current behaviour.

| Page | Date | Topic |
| --- | --- | --- |
| [Phase 0 spike results](../spikes/RESULTS.md) | 2026-09-18 | Twelve feasibility spikes (tiled viewport, wgpu beside GPUI, assistant over MCP, dependencies) |
| [Common-tool audit](specs/reports/tool-audit-2026-09-19.md) | 2026-09-19 | Baseline audit of the twelve toolbar tools |
| [Brush implementation validation](specs/reports/brush-validation.md) | 2026-09-22 | Stroke timing measurements for the brush engine |
| [Assistant composition workflow](specs/reports/assistant-composition-workflow.md) | 2026-09-23 | Workflow ideas for assistant-driven composition |
| [Assistant drawing and MCP reliability pass](specs/reports/assistant-drawing-pass.md) | 2026-09-23 | Rendering and tool-use defects fixed in the assistant drawing path |
| [GPUI CPU review](specs/reports/gpui-cpu-review.md) | 2026-09-23 | Source audit of vendored GPUI CPU cost |
| [Photo/Draw switch pauses](specs/reports/photo-draw-switch-performance.md) | 2026-09-23 | Analysis of the workspace switch stall |
| [Release layout reuse measurements](specs/reports/layout-reuse-release-results.md) | 2026-09-23 | Cold versus retained layout benchmarks |
| [Intrinsic text release measurements](specs/reports/intrinsic-text-release-results.md) | 2026-09-23 | Text layout specialisation benchmarks |
| [Canvas navigation release measurements](specs/reports/canvas-navigation-release-results.md) | 2026-09-23 | Pan and zoom notification benchmarks |
| [Live layout setting comparison](specs/reports/live-layout-toggle-results.md) | 2026-09-23 | Manual sample of a running editor with layout reuse off and on |
| [Native UI handoff fidelity](specs/reports/template-fidelity.md) | 2026-09-27 | Surface contract, fidelity fixes and native adaptations against the UI handoff |
| [MCP functionality coverage](specs/reports/mcp-coverage.md) | 2026-09-27 | Editing, Design, diagram and presentation coverage, host behavior and remaining gaps |
| [GPUI Kit UI audit](specs/reports/gpui-kit-ui-audit.md) | 2026-09-28 | Findings, backports and prioritised next improvements for the UI kit |
| [Diagram visual and performance review](specs/reports/diagram-visual-review.md) | 2026-09-28 | Representative draw.io sample inspection, verified fixes and measured performance |
| [Library and Develop feature audit](specs/reports/library-develop-feature-audit.md) | 2026-09-28 | Static audit of Library and Develop features and priorities |

Raw data beside the reports:

- [layout-reuse-release-results.json](specs/reports/layout-reuse-release-results.json),
  [intrinsic-text-release-results.json](specs/reports/intrinsic-text-release-results.json),
  [canvas-navigation-release-results.json](specs/reports/canvas-navigation-release-results.json)
  and [live-layout-toggle-results.json](specs/reports/live-layout-toggle-results.json):
  benchmark output for the measurement report of the same name.
- [design-completion-results.json](specs/reports/design-completion-results.json):
  Linux source, rendering, browser and package acceptance for the
  [Design completion worklist](specs/design-completion-worklist.md).
- [design-interchange-results.json](specs/reports/design-interchange-results.json):
  editable-interchange and native-host acceptance, cited by the Design
  completion worklist and the MCP coverage report.
- [diagram-sample-audit.json](specs/reports/diagram-sample-audit.json),
  [drawio-sample-results.jsonl.gz](specs/reports/drawio-sample-results.jsonl.gz)
  and [visio-sample-results.jsonl.gz](specs/reports/visio-sample-results.jsonl.gz):
  sample-corpus runs summarised in
  [Diagram functionality and compatibility](guides/diagram-functionality.md).
