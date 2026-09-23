# Brush Library and Brush Studio implementation plan

Implementation update (2026-09-22): the library, Studio, expanded CPU engine, dual brushes, Windows input adapter, memories, and package/import workflows are implemented. See [usage](brush-workflow.md), [validation and remaining hardware gates](brush-validation.md), and [format boundaries](brush-import-formats.md). The audit below records the original baseline; its test-status notes are historical.

Audit date: 2026-09-22. Scope: the current working tree, including the uncommitted Brush Settings sidebar and quick-controls work. This is a gap assessment and implementation plan; no application code was changed.

Emulsion has a useful brush engine already. The largest missing pieces are a durable library model, a safe brush-authoring workflow, Windows pen input, and deeper rendering semantics. Build on the existing engine rather than replacing it or adding a screen full of unsupported settings.

## Reference baseline

The current handbook describes shared brush use across Paint, Smudge, and Erase, tool-setting transfer, and per-brush size/opacity memories. These are the interaction targets for Emulsion's three painting modes. [Paint, Smudge, and Erase](https://help.procreate.com/procreate/handbook/brushes/paint-smudge-erase)

The library target includes libraries containing ordered sets and brushes, stroke previews, search, recent/pinned brushes, organization commands, and file-backed storage. Desktop menus and keyboard commands should provide the equivalent workflows. [Brush Libraries](https://help.procreate.com/procreate/handbook/brushes/brush-library)

Studio supplies category navigation, settings, and a drawing pad that updates existing test strokes as settings change. Edits have explicit Done/Cancel behavior; numerical entry and input-response editing supplement sliders. [Brush Studio](https://help.procreate.com/procreate/handbook/brushes/brush-studio)

The detailed reference has 14 categories, including Preview and 3D Materials. Use that page as the inventory; the overview still contains an older reference to eleven attributes. [Brush Studio Settings](https://help.procreate.com/procreate/handbook/brushes/brush-studio-settings)

The supplied [Part One — The Fundamentals PDF](<C:/Users/jerem/Downloads/part1-the-fundamentals.pdf>) is a 38-page beginner workbook made with Procreate 5.2 in July 2022 (PDF p.2). It supplies workflow acceptance cases, not brush algorithms or source assets. Page references below use PDF page numbers, not the offset printed numbers.

The proposed target is comparable 2D brush workflows and expressive behavior. Reproducing Procreate's exact pixels or bundled brushes is not established by these documents. Preserve imported originals, report conversion limits, and develop Emulsion's own curated presets. Full 3D material painting and cloud synchronization are separate later projects, explicitly outside the first 2D release.

## What exists and where the gaps are

“Partial” means real functionality exists but does not complete the intended workflow. This is a source audit, not a claim that all current behavior has passed testing on this machine.

| Area | Current Emulsion evidence | Gap / next action |
| --- | --- | --- |
| Built-in brushes | 51 presets across 10 categories; `BrushPreset` has name/category/note/brush. [library.rs](../crates/emulsion-raster/src/library.rs) | Stable identities, editable sets and libraries, curated assets, richer metadata. Preserve existing presets during migration. |
| Library interface | Category chips, preset names, Mine, save/delete/import. [presets.rs](../crates/emulsion-ui/src/editor/presets.rs), `preset_panel` area around line 385 | Two-pane browsing, stroke thumbnails, search, pins/recents, rename/duplicate/move/reorder, multiselection, library switching. |
| Shared tool behavior | `ToolState::kits` and `switch_slot` already retain brush values and preset name separately for painting modes and other brush tools. [tools.rs](../crates/emulsion-ui/src/editor/tools.rs), lines 63, 510 | `apply_preset` at presets.rs:166 derives mode from category; an ordinary brush resets Smudge/Erase to Paint. Browsing a category also applies its first brush. Separate browsing, brush selection, and mode activation. |
| Quick adjustments | Current working tree has size/hardness/opacity/flow popup and a Brush Settings sidebar. [brush_quick.rs](../crates/emulsion-ui/src/editor/brush_quick.rs), tools.rs:4133 | Retain quick access; add per-brush/per-mode memories and explicit transfer-current-brush command. Full Studio must not replace quick controls. |
| Studio | Existing Tip/Texture/Dynamics/Drawing/Presets controls edit current tool state directly. tools.rs:4220, 4360 | Dedicated draft session, Done/Cancel, reset points, drawing pad, source editors, author metadata, shared swatch rendering. Drawing guides/symmetry remain document controls. |
| Storage | Flat JSON, 400-preset cap, corrupt-file backup, defaulted legacy fields, persisted PNG textures. presets.rs:8–51; [brushset.rs](../crates/emulsion-io/src/brushset.rs):275 | Versioned hierarchy, transactional updates, consistent shared state across documents, portable packages, asset ownership. Save/delete currently mutate memory before successful persistence. |
| Imports | `.brush`/`.brushset` scalar subset plus shape/grain PNGs. brushset.rs:26, 100, 176 | Unsupported properties are lost; malformed archives can fall back to defaults. Add compatibility reports and failure handling; preserve set structure. No `.brushlibrary`, ABR or brush export implementation found. |
| Engine | Real shared paint/smudge/erase path, image tips, grain, pressure/tilt parameters, wetness, taper, smoothing, blend modes. [paint.rs](../crates/emulsion-raster/src/paint.rs):202, 376, 957 | Keep this working baseline. Expand semantics in independently testable increments. |
| Pen input | Linux/macOS adapters; Windows branch only marks scanning complete. [tablet.rs](../crates/emulsion-ui/src/tablet.rs):126 | Windows pressure/tilt input is a priority dependency. Current samples lack a unified position/time/input record and barrel rotation. |
| GPU | Optional, narrowly eligible persistent strokes; otherwise CPU or final-composition acceleration. paint.rs:775; [performance notes](gpu-brush-performance.md) | Benchmark textured/wet brushes on representative canvases. Existing GPU paths do not establish broad brush acceleration. |
| Assistant integration | MCP reads legacy JSON separately and already renders engine-based swatches. [exec.rs](../crates/emulsion-mcp/src/exec.rs):1078; [brush_discovery.rs](../crates/emulsion-mcp/src/brush_discovery.rs):75 | Migrate discovery and selection with the UI; extract reusable preview rendering below both consumers. |

The Studio comparison below is deliberately at category level. Detailed parameter ranges, dependencies and conversion formulas must be specified with each implementation slice, against the [settings reference](https://help.procreate.com/procreate/handbook/brushes/brush-studio-settings). Similar control names are not evidence of matching algorithms.

| Category | Current engine / UI | Required development |
| --- | --- | --- |
| Stroke Path | Spacing and scatter | Directional/spacing jitter; distance falloff. |
| Stabilization | One position smoother | Separate smoothing behaviors; cadence-independent sampling. |
| Taper | Distance-based size taper; end replay | Opacity and input-specific profiles; response controls. |
| Shape | Ellipse/image tip, angle, path following | Stamp multiplicity, flips, rotation dynamics, source editing. |
| Grain | Procedural patterns and tiled images | Moving grain, richer transforms/depth, seamless source editing. |
| Rendering | Flow, stroke opacity, blend modes | Distinct accumulation modes, edge controls, luminance/threshold behavior. |
| Wet Mix | Simple pickup recurrence, wetness | Independent pigment load, dilution, pull and mixing controls. |
| Color Dynamics | Combined per-dab hue/lightness jitter | Independent color channels, stroke variation and input mappings. |
| Dynamics | Size jitter; speed substitutes for pressure | Independent speed response and opacity/spacing dynamics. |
| Apple Pencil → Stylus | Pressure exponent, size/flow mapping, tilt shaping | Windows input, editable curves, capability-aware tilt/rotation mappings. |
| Properties | Global sanitization limits | Per-brush limits, orientation and smudge behavior. |
| Materials | No 3D painting model | Deferred: needs a separate material/document/rendering design. |
| Preview | MCP swatches only | Shared configurable thumbnails and cached preview renderer. |
| About this Brush | Name and note only | Author, provenance, baseline and reset metadata. |

Dual brushes are another missing engine and authoring feature: two independently editable components with a combine operation and an uncombine workflow. They should follow the single-brush model and preview infrastructure. [Dual Brush](https://help.procreate.com/procreate/handbook/brushes/dual-brush)

## Proposed ownership and data model

These are proposed types and boundaries, not existing APIs.

1. Add a coherent `emulsion-brushes` feature crate for the pure library model, validation, commands and store contract. It may depend on `emulsion-raster` for brush definitions; it must not depend on UI, MCP, or `emulsion-io`. Implement storage/archive adapters in `emulsion-io`. Both UI and MCP consume this shared contract. This avoids putting persistent library ownership in `EditorView` or making the raster engine depend on application services.
2. Model `BrushLibrary`, `BrushSet`, `BrushDefinition` with stable IDs and explicit ordering. Definitions carry schema version, engine settings, source-asset references, provenance, preview settings, factory/import baseline and optional user reset point. Define duplicate, move, delete, restore, and missing-reference behavior once.
3. Keep immutable engine configuration separate from `ToolBrushState`: active brush ID, size/opacity overrides, and memories per brush and mode. Preserve current independent tool slots. Ordinary selection keeps the current mode; transfer is explicit. Named convenience commands such as choosing an eraser may still activate a mode, through a separate action.
4. One application-owned library entity coordinates all open editors. Documents own active tool state and artwork history. Library changes and Studio drafts have separate undo/commit semantics; browsing and test strokes never enter document undo.
5. Introduce versioned grouped settings without immediately rewriting the proven raster pipeline. Resolve them into an immutable runtime configuration at stroke start. Legacy flat `Brush` values must retain their existing appearance through an explicit compatibility mapping. New definitions must not silently lose unsupported settings when saved.
6. Store textures with collision-resistant content IDs and bounded decoded caches. Existing 32-bit registry IDs are runtime handles, not durable identities. Preserve shared assets during duplicate/delete/reset; garbage-collect only unreachable assets after a successful store transaction. Include reset baselines when determining reachability.
7. Put shared stroke-sample/replay and thumbnail rendering beside the raster engine. Record time, position, pressure, tilt, available rotation, and deterministic random seed. The canvas, Studio pad and library previews use the same rendering semantics, with explicit preview background and sampling policy.

Migration must import every legacy preset and its texture references, retain a backup, preserve duplicate names using IDs, and avoid the current 400-entry truncation. Write assets before publishing a manifest, then atomically commit a versioned manifest; failures keep the previous library and live state usable. Update MCP in the same release. Missing assets and unsupported schema versions must produce visible recoverable states.

## Fabric reference: concrete reuse opportunities

Reviewed the user-provided [Fabric-Brush-like-Procreate repository](https://github.com/DevGambles/Fabric-Brush-like-Procreate) through its local checkout at `C:\development\github\Fabric-Brush-like-Procreate`, commit `4ee583a0db30d419af1ad465cbc350d6ecda80db`. The source review supersedes the initial web-only access limitation. The checkout contains 32 brush modules and 23 PNGs, plus a Fabric.js demo and a modified Croquis engine. Source was inspected but not executed.

| Reference implementation | What it contributes | Application to Emulsion |
| --- | --- | --- |
| `dist/brushes/Brush/Watercolor.js`, `WetAcrylic.js` | Tinted image stamps, size-relative spacing, Canvas XOR composition | Image-tip preset experiments and comparison fixtures. These implementations do not provide wet pigment pickup or mixing; retain our existing wet engine and phase 5b. |
| `dist/brushes/Roller/GrungyLinen.js:46` | Repeated recolored texture with randomized origin, circular coverage and slight blur | Grain-origin variation and textured preset trials; our tiled grain already provides part of this foundation. |
| `dist/croquis.js:1035` | A cascade that smooths position and pressure, including finishing behavior | Prototype a separate smoothing mode; adapt timing to recorded input instead of copying timer-dependent behavior. |
| `dist/croquis.js:1548`, `1593`, `1636` | Transformed image stamps, canvas-anchored grain, tangent/normal scatter, directional rotation and scale interpolation | Concrete references for phase 5a directional jitter and stamp/grain coordinate tests. Validate units, bounds and interpolation before porting. |
| `dist/brushes/Marker/HandstylerOne.js:35`, `149` | Integration of Croquis image/grain rendering, smoothing and browser pen pressure | A useful combined brush fixture; browser input plumbing does not replace a native Windows adapter. |
| `dist/brushes/ribbonBrush.js:27` | Multiple eased follower paths | Optional procedural-brush experiment after the core Studio. It requires retained simulation state, not merely another preset value. |

Several apparent advanced features in Croquis are only stored settings: `taperAmount` (1269), `shapeCount` (1312), `grainMoveScale` (1362), and `dynamicSpeedSize` (1427) have getters/setters but no renderer reads. Do not count those as implementations to port or expose their controls as functional.

The demo's brush output is committed as cropped Fabric image objects (`dist/fabric.brushes.js:145`), and randomness uses `Math.random` (155). Emulsion should retain tiled document strokes, transaction undo and seeded replay. The demo has brush-selection buttons and basic controls; it does not close our library persistence, Studio draft, reset, or source-editor gaps. No test/spec files or package test manifest were found in the checkout.

The root `LICENSE` contains the MIT license and Arjan Haverkamp's 2021 notice. Retain applicable notices for adapted code and record source attribution, including the existing per-file credits. Record provenance for any selected images; the checkout does not contain a per-image source manifest. Asset selection and code adaptation are explicit implementation tasks, not a dependency on shipping the JavaScript runtime.

Add a small reference-adaptation track to phases 0 and 5a:

1. Select four representative fixtures: textured marker, linen roller, scattered spray, and image-stamped paint. Record the source module, asset, parameter values and input path.
2. Produce comparison swatches with the existing Emulsion engine first; classify each difference as preset tuning, asset interpretation, missing dynamics or different compositing semantics.
3. Implement only the missing shared capabilities, starting with directional scatter, rotation variation and grain-origin control. Prototype multi-stage position/pressure smoothing separately, and test first-stamp orientation. Keep per-stroke seeds and explicit coordinate conventions. In particular, fix the reference's random-rotation unit mismatch before adapting that behavior: `croquis.js:1599` generates 0–360 values while surrounding trigonometry uses radians.
4. Compare repeated strokes, isolated taps, zoom/transformed layers, transparent backgrounds and overlapping dabs. Add performance measurements and regression fixtures before adding curated presets.

This reference can accelerate brush variety and selected engine work. The handbook remains the workflow specification and the PDF remains the end-to-end drawing acceptance reference.

## Desktop interaction plan

The primary library task is choosing a brush for the active Paint, Smudge, or Erase mode. Use a library selector and set list beside a virtualized brush list with real stroke previews. Show the current mode and selected brush. Search results show their library/set; Recent and Pinned remain identifiable collections. Browsing leaves the active brush untouched. Selecting a brush applies it; double-click or `Edit brush…` opens Studio.

Use standard GPUI list, menu, button, input and resizable-region components through the existing toolkit. Domain IDs drive selection and element identity. Provide keyboard equivalents for drag/reorder/move, visible import/create commands, and context menus for secondary operations. Repeated activation of the active paint tool opens its library. Dismissal restores canvas focus without painting through the surface.

Studio is a retained editing view with category navigation, a scrollable settings form, and a resizable drawing pad. Keep original and draft definitions separate. Done publishes once after validation and persistence; Cancel drops the draft. Failed saves keep the draft open. Reset modifies the draft until Done. Concurrent library edits must be detected by revision rather than overwritten.

The pad records strokes and replays them when settings change. It supports clear, preview color/background/size, and representative smudge/erase content. Background jobs carry draft revisions and discard stale results. Thumbnails are lazy and bounded, keyed by brush/asset/engine/preview revisions. Keep all preview rendering outside GPUI's render method.

Use theme tokens and relative sizing; define minimum/default pane sizes during the UI slice. At narrow sizes, retain navigation and a reachable pad without clipping settings. Test keyboard paths, focus restoration, light/dark themes, UI zoom and display scaling. Unsupported settings are omitted from the initial authoring UI; import reports still disclose them. Hardware-dependent controls explain capability availability.

## Delivery sequence and acceptance gates

Effort labels are relative: S = localized, M = one feature slice, L = several connected slices, XL = substantial engine or platform work. They are not calendar estimates. Each row should be broken into reviewable changes with its own test gate.

| Phase | Deliverable and primary owners | Dependencies | Acceptance gate | Effort |
| --- | --- | --- | --- | --- |
| 0 | Baseline fixtures, capability inventory, import diagnostics spike, Windows input spike. `raster`, `io`, `ui/tablet` | None | Capture representative dry, textured, wet, erase and smudge output; identify unsupported archive fields; verify an actual Windows pen sample path. | M |
| 1 | Versioned library domain/store, stable IDs, asset lifecycle, legacy migration, shared UI/MCP access. New `brushes`, `io`, UI/MCP adapters | 0 | Restart preserves presets/assets/order; duplicate names are safe; corrupt/missing assets recover; failed writes cannot change live state; multiple editors see coherent changes. | L |
| 2 | Mode-preserving selection and library UI: sets/libraries, thumbnails, search, recent/pinned, create/rename/duplicate/move/delete/reorder, multiselect. `ui`, `brushes` | 1 | Select the same textured brush in all three modes; browse without switching brush; organize and restart; keyboard workflow works with a large imported collection. | L |
| 3 | Windows pen backend, unified sample records, quick size/opacity memories and transfer action. `ui/tablet`, platform adapter, `raster`, `brushes` | 0; memories depend on 1 | Real pen changes pressure/tilt output on Windows; mouse fallback remains usable; switching modes preserves settings; memories survive restart and remain scoped to brush/mode. | L |
| 4 | Studio draft lifecycle, settings for existing engine features, deterministic pad, source selection/import, previews, metadata and reset. `ui`, `brushes`, `raster`, `io` | 1–2; unified samples from 3 for full stylus validation | Create/edit/test/save/reopen a brush; Cancel leaves library and artwork unchanged; pad replays existing marks; reset/duplicate retain assets; preview and canvas agree on identical samples. | L |
| 5a | Stroke, stabilization, taper, shape and grain expansion; shape/grain source editors and original source library. `raster`, Studio UI, assets | 3–4 | Each exposed parameter changes output in a controlled fixture; transformed layers, sparse samples and single dabs work; legacy brush fixtures remain compatible. | XL |
| 5b | Rendering, wet mix, color and independent speed/input dynamics; per-brush properties and full curve editing. `raster`, Studio UI | 5a | Define and test parameter interactions, layer sampling, transparent pixels, opacity accumulation and lift-off; no controls with unimplemented behavior. | XL |
| 6 | Dual brush editing/composition; native portable brush/set/library packages; broader external imports. `raster`, `brushes`, `io`, Studio | 4; dual/render fidelity depends on 5 | Combine/edit/uncombine without losing originals; native export/import preserves settings/assets/baselines; external import provides per-brush compatibility results. | L–XL |
| 7 | Performance work, final preset curation, usability pass and documentation. `gpu`, `raster`, UI/assets | Benchmarks start at 0; release gate after relevant features | Measured latency/memory targets, CPU/GPU parity and failure fallback; workbook scenario passes using original Emulsion brushes. | L |

Phases 1–2 can proceed alongside the Windows input work after the initial spike. Native package export can follow phase 4 without waiting for advanced physics. Benchmark and curate continuously; phase 7 is the release gate rather than the first performance check.

The first usable milestone is phases 1–4: an organized persistent library and a real Studio for the engine we already have, including Windows pen behavior. That is an initial release, not full handbook parity. The first 2D parity target additionally requires 5a, 5b and the agreed phase-6 features. 3D Materials remains explicitly deferred.

## Import fidelity, rendering and performance decisions

Keep native lossless exchange separate from external conversion. The handbook lists `.brush`, `.brushset`, `.brushlibrary` and `.abr` import workflows; that is a format/workflow reference, not a public specification of archive internals. [Import and Share](https://help.procreate.com/procreate/handbook/brushes/brushes-share)

For external files, classify each setting/asset as supported, approximated, unsupported or invalid. Preserve originals/provenance, show warnings before committing, and reject malformed brushes rather than inventing successful default imports. Add archive entry/decompression/image limits and deterministic partial-failure behavior. Use a fixture corpus across format versions before promising fidelity. `.brushlibrary` and ABR need discovery spikes; native export does not imply Procreate-readable export.

Define wet sampling explicitly: interactive painting currently supplies a composited backdrop, while MCP advertises current-layer sampling by default. Unify the policy contract and make any intended difference explicit. Test transport of color and transparency on isolated versus visible-layer samples.

Keep CPU rendering as the reference. Extend GPU eligibility only after feature parity tests; snapshot brush parameters at stroke start and preserve cancellation/replay/fallback. Benchmark 4K and larger canvases with dry, image-tip, dense-grain, wet, smudge, erase and dual workloads. Record p50/p95 stroke-update time, input-to-visible latency, commit time, frame stalls and asset memory. An initial engineering target is 60 Hz interaction for representative brushes; choose concrete hardware/brush-size limits in phase 0 before turning that into a release promise.

## Validation plan and audit limitations

Extend existing tests rather than treating UI presence as completion:

- Raster tests in `paint.rs`: pressure/tilt interpolation (2441), taper/spacing (2480), erase/selection (2546), alpha lock (2612), taper finish (2690), wet/smudge (2790), and persistent GPU recovery (2019 onward).
- UI tests in `paint_functionality_tests.rs`, `painting_tests.rs`, and `tool_usability_tests.rs`: actual pixels, mode state, undo/cancel, preset selection, quick controls, and focus. The existing `preset_tool_switches_preserve_the_previous_brush_and_leave_liquify` test intentionally checks category-triggered erasing; revise it to distinguish explicit tool activation from ordinary library selection.
- Storage/import tests in `editor/presets.rs` and `io/brushset.rs`: migration, round trips, corrupt data and archives. Add failure rollback, cross-document updates, durable ordering, reset asset retention and portable package tests.
- Preview tests: canvas/pad equality for identical samples and seed; cache invalidation after settings/source changes; stale async result rejection; closing Studio never mutates artwork.
- Platform/manual checks: real Windows tablet, pen lift/cancel, pressure and tilt, mouse fallback, keyboard access, min-size layout, themes and zoom. Headless tests cannot prove physical driver behavior.

The PDF's end-to-end acceptance exercise is: browse a pencil-like brush (pp.9–10); vary pressure/tilt/size/opacity (p.11); use the same brush through all three modes (p.12); paint on separate layers (pp.22–23); sample color (pp.29–30); add pressure-sensitive outlines and large, light-pressure texture taps (pp.32–34); undo, reorder and export (pp.27, 36). Its named brushes are behavior examples, not supplied reproducible presets.

Recommended implementation checks:

```powershell
cargo test -p emulsion-raster -p emulsion-core -p emulsion-io -p emulsion-mcp -p emulsion-ui --lib --locked -- --test-threads=1
cargo clippy -p emulsion-raster -p emulsion-io -p emulsion-mcp -p emulsion-ui --all-targets --locked
```

Include the proposed feature crate once added. Run targeted tests per slice, GPU-specific tests on supported hardware, and the broader workspace checks at integration milestones.

During this audit, the raster paint suite and the IO synthetic-brushset test were attempted but both waited on an existing Cargo build-directory lock. Only these waiting audit commands were canceled. No test pass is claimed. The current source, existing test coverage, all 38 PDF pages, the linked handbook pages, and the local Fabric reference were reviewed; application behavior and performance were not newly validated in this planning pass. Document local-link and trailing-whitespace checks passed.
