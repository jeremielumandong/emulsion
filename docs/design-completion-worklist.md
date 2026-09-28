# Local Design completion worklist

User scope: finish the full remaining Design roadmap, preserving the supplied
native UI, existing editing functions, crisp editable text, local file exchange,
cross-platform code and the small application footprint. Collaboration, accounts
and remote publishing stay deferred. Windows/macOS runtime acceptance is performed
on the user's separate machines. This checklist records delivery, not a claim of
Canva/PowerPoint/Lucid parity.

## Current implementation and validation

- [x] Typed page variables with color/number bindings, UI/MCP and persistence.
- [x] Presentation click actions, navigation/back, modal overlays and component states.
- [x] Cross-page component publication and finer explicit overrides.
- [x] Responsive width preview without changing authored pages.
- [x] Embedded local audio/video, trim/volume/loop and transform/opacity keyframes.
- [x] Area/scatter/stacked/donut charts, axis controls and merged table cells.
- [x] Native underline/strikethrough and whole-layer bullet/numbered lists.
- [x] Finish combined UI/MCP/core regression, lint and release acceptance for these additions.

## Remaining delivery batches

- [x] Selection/frame output: cropped PNG/SVG/PDF, transparency and diagnostics.
- [x] Standalone responsive HTML with embedded artwork/media and supported interactions.
- [x] Live MCP media controls and presenter timer.
- [x] Creative catalog MCP: brand kits, logos, collections and installed templates.
- [x] Workspace lifecycle MCP without silently retargeting originating relays.
- [x] Guides/snapping/channel/quick-mask/clipboard and tool-state MCP audit.
- [x] Smart-source relink/edit workflows and specialized diagram-formatting audit.
- [x] Printer preview/setup/job workflow MCP with platform validation.
- [x] Vector editing reachability: points/handles, joins/splits, compound paths and stroke expansion.
- [x] Appearance: multiple gradient stops and matching-object selection.
- [x] Precision: ruler origins/units, spacing, mesh/perspective/skew and bitmap tracing.
- [x] Responsive constraints: container rules and child breakpoint sizing; final large-scene measurements remain in acceptance.
- [x] Component automatic override inference; project variable libraries.
- [x] Data-driven image fields, saved CSV mappings and multi-page record sets.
- [x] Rich paragraph lists, nested numbering/hanging indents and table formulas.
- [x] Design photo crop/replacement/adjustment/effect reachability audit.
- [x] Portable embedded fonts, typography roles, palette extraction/targeting and asset folders.
- [x] Hover/drag presentation triggers and expanded transition controls.
- [x] Bulk keyframe retiming, reveal channels, additional presets, sampled animated SVG and rendered-frame Lottie export with explicit fidelity diagnostics.
- [x] Template breadth and responsive catalog integration.
- [x] Feature-level interchange matrix and PowerPoint evaluation; no editable `.pptx` bridge is advertised.
- [x] AI capability availability and native authoring workflow audit; runtime model/provider requirements documented.
- [x] Extension workflow design; data-only packs must not silently execute scripts.
- [x] Final source/package smoke, Linux playback and user-side Windows/macOS checklist.

OS registration, codec installation, credentials and cloud configuration retain
explicit user-facing setup. No service subscription or browser bundle is added
implicitly. Unsupported exports must return diagnostics rather than silently
claiming fidelity.

## Acceptance boundaries

- Windows/macOS runtime acceptance remains on the user's separate machines; see
  [platform checklist](design-platform-acceptance.md).
- PowerPoint file support was evaluated; no editable `.pptx` bridge is implemented.
- Lottie export uses rendered image layers; general Lottie import and editable
  vector Lottie interchange are not advertised.
- Cloud collaboration, remote publishing and executable plugin runtimes remain
  outside this local-editing delivery.

## Completed Linux acceptance, 2026-09-28

- 311 core, 247 IO and 188 MCP tests passed; one core test remains intentionally ignored.
- 64 focused native UI checks passed, including the inspector scroll regression.
- Required-host-GPU checks passed for crisp shadowed text and multistop gradients.
- Browser HTML checks passed for hover/drag/click actions, overlays, component states,
  responsive width selection, safe labels and hidden fullscreen controls.
- Animated SVG browser sampling confirmed movement, visibility and native text reveal.
- Linux system media helper passed WAV/H.264 playback, trim/loop and live command checks.
- Clippy passed with warnings denied for core, IO, MCP, UI and engine library/test targets.
- Release MCP inventory contains 315 unique tools with valid schemas/annotations.
- AppImage version smoke passed; no browser runtime was bundled.

Package: `target/appimage/Emulsion-0.0.3-advanced-design-x86_64.AppImage`.
Exact size, checksum, test counts and benchmark results are recorded in
[completion results](design-completion-results.json). The responsive 1,000-object
workload improved from 40.47 ms to 16.29 ms median; see
[measurement boundaries](design-layout-performance.md).
