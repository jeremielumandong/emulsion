# Emulsion: local Design, Diagram, and editor redesign

## Scope and decisions

The September 26 handoff (`Modern AI-Powered Design App-handoff.zip`, primary
`Emulsion Editor v2.dc.html` and accompanying UI spec) is the visual target.
Implement missing controls and tools as working features. Preserve every existing
Photo, Paint, RAW, brush, vector, text, AI, file, recovery, and history workflow.
This is a native GPUI/Vello application; the HTML is a reference, not an embedded
replacement for the editor.

**Local editing first. Collaboration later**, as requested. Documents, templates,
projects, brands, asset libraries, search, and history must work without accounts
or a network. Existing optional AI providers remain available; manual editing
never requires them. Do not reproduce the prototype's fictional collaborators,
storage quota, projects, images, AI responses, or save status. Share becomes local
export/package until collaboration exists.

The intended product combines Canva-style page composition with draw.io/Lucid-style
structured diagrams. Canva's documented elements, frames, page settings, guides,
and layers inform Design; draw.io's pages, shape libraries, connected objects, and
editable exports inform Diagram. Lucid's page operations and data-driven diagrams
inform the advanced graph workflow. These are workflow references, not a promise
of compatibility with proprietary cloud services or access to their asset catalogs.
References: [Canva editing](https://www.canva.com/help/editing-designing/),
[Canva frames](https://www.canva.com/en_gb/help/using-frames-variantb/),
[draw.io features](https://www.drawio.com/docs/features/),
[Lucid pages](https://help.lucid.co/hc/en-us/articles/11970952773652-Welcome-to-Lucidchart),
[Lucid CSV import](https://www.lucidchart.com/blog/introducing-process-diagrams-from-csv-import).

## September 27 implementation checkpoint and package plan

The next deliverable is portable local templates and stencils, with file exchange
and GitHub installation. See [the format and user workflow](template-pack-format.md).

- [x] Versioned `.emutemplate` / `.emustencil` ZIP manifest with name, author,
  license, tags, editable native project, and preview.
- [x] Export current Design pages or one Diagram stencil per page; omit local
  version history, preserve native text, vectors, images, and graph metadata.
- [x] Validate and install local files into the persistent library; preserve
  source files; reinstall the same version without duplicate entries.
- [x] Public GitHub repository / pinned directory installation, bounded HTTPS
  downloads, data-only archives, atomic catalog updates, offline installed copies.
- [x] Native export/install forms, per-page stencil placement, normal Open routing,
  search and existing library metadata/removal controls.
- [x] Editable Visio OPC/VDX and Lucid Standard Import readers; warnings/errors for
  unsupported features. Lucidchart's VSDX/VDX export is an additional import route.
- [x] File/package tests for editable round trips, metadata, multiple pages,
  connections, reinstallation, repository layout, and invalid archives.
- [x] Native interaction regression for export dialog, opening/installing a pack,
  editable stencil placement, source preservation, and one-step undo.
- [ ] Representative real-world vendor-file compatibility corpus and visual review.

Other implemented work now includes persistent Home folders/trash references and
workspace filters, local Library collections/metadata, Design resize constraints,
brand kits, motion preview/presentation/GIF export, diagram quick-create, arrow
styling, bend/label editing, bulk layout, text/CSV/Mermaid/SQL generation, CSV refresh,
and conditional fills. These need the final preservation/interaction review below;
implementation does not mean every handoff layout acceptance criterion is complete.

Vello work removed history-accounting calls that unnecessarily rasterized editable
text/path caches. A 1,000-shape/999-connector core benchmark improved single-node
move p50 from 65.337 ms to 9.042 ms on this machine. That measures command latency,
not frame presentation. Native clipboard retains source text/vector objects.

Final validation for this checkpoint (September 27): `cargo test --workspace
--locked -- --test-threads=1` passed **1,270 tests**, with 12 ignored by their test
annotations, including **490 native UI tests** and **153 I/O tests**. The separate
ignored Vello glyph-coverage test passed on the available GPU for regular, bold,
italic, and bold-italic Geist. Workspace Clippy (`--all-targets --locked -- -D
warnings`), formatting, GPUI vendor validation, license-staging tests, renderer
policy tests, and whitespace checks passed. GitHub URL/selected-directory archive
handling is covered by local fixtures; a published Emulsion-pack repository has
not yet been used for a live download/install acceptance test.

Remaining release gates include the complete handoff control/layout audit,
accessibility and narrow-window review, advanced vendor shape/library compatibility,
context-specific AI proposals, broader display/performance measurements, and
platform checks requiring macOS/Windows hardware. Collaboration remains deferred.
The migration is not marked complete while those gates remain open.

## Visual contract

Use the handoff's six destinations: Home, Photo, Paint, Library, Design, Diagram.
Paint reuses Draw; old settings, shortcuts, saved workspaces, and Minimal remain
supported. Workspace changes never convert or flatten document contents.

| Surface | Handoff target |
| --- | --- |
| Frame | 36 px menu, 34 px tool options, 38 px tabs, 24 px status |
| Photo/Paint | 48 px tool rail, canvas, 300 px dock; resizable sections |
| Dock | Properties/Adjust/History/Assistant; Swatches/Color; Layers/Channels/Paths |
| Home/Library | 220 px navigation; Home content max 1240 px |
| Design | 68 px rail, 250 px asset drawer, canvas, 88 px page strip |
| Diagram | 250 px stencil drawer, grid canvas, properties, page tabs/minimap |
| New document | 880 px dialog: types, presets, editable preview/settings |
| Typography | Bundled Geist UI and Geist Mono values; system fallback |
| Dark | backgrounds #141416 / #1b1b1e / #232327 / #2d2d32; stage #0e0e10 |
| Light | backgrounds #f2f1ee / #fafaf9 / #eeede9 / #e2e1dc; stage #dedcd7 |
| Appearance | Ember, Halo, Tide, Moss, Amber, Rose; Square 3 / Soft 8 / Round 12 px |

All menus remain reachable at narrow widths through overflow. Keyboard navigation,
focus indicators, tooltips, display scaling, reduced motion, and readable light and
dark themes are acceptance criteria, not finishing extras. Preserve external-theme
following. The prototype's HTML sliders are examples; real controls must validate
values, expose units, and participate in undo where they edit a document.

## Functional inventory

“Reuse” means a working existing subsystem, not that the new UI is finished.

| Area | Reuse | Implement or extend |
| --- | --- | --- |
| Home | Recent files, thumbnails, stars, grid/list, recovery, RAW gallery | Project folders, workspace filters, project creation/move/rename, trash/restore, five start cards, local search |
| New document | Document validation, background layers, tabs | Type/category/preset chooser, custom dimensions/units/DPI, orientation, name, recent/custom presets, project, page count/bleed, diagram setup |
| Photo | Selections, crop, transform, masks, adjustments, effects, RAW, export | Handoff dock/rail/options, selection action bar, review each template option against tool audit |
| Paint | Brushes/imports, pressure, stabilizer, symmetry, clone/heal/smudge, palette | Handoff brush shelf/properties/preview; expose every existing brush control |
| Text | Editable text/runs, paragraphs, fonts, warp/path text, vector cache | Design presets/combinations; inspect copied text at multiple scales, cross-document and native clipboard fidelity |
| Library | Ratings/flags, RAW sidecars, batch/sync/export | Collections, folder import, filmstrip, keyword search, multi-selection Develop and batch controls |
| Design pages | Native Document and editable Node types | Stable page IDs, add/duplicate/delete/reorder/rename, thumbnails, page sizes/backgrounds/bleed, page-aware undo/save/export |
| Design elements | Paths/text/raster/smart layers/groups/transforms, align/distribute | Searchable element library, semantic frames/cropping, editable templates, snapping/spacing, reusable components |
| Design assets | File import, brushes, fonts | Local uploads/photos index, replace/relink missing assets, metadata/license attribution, SVG vector preservation |
| Design brand | Project colors and text styles | Named brand kits, logos/fonts/colors, apply styles across selected pages, import/export kits |
| Design resize | Canvas/image transforms | Copy-and-resize variants, per-object anchors/constraints, text reflow, review overflow; preserve source |
| Design animation | None confirmed | Page/object duration, simple entrance/exit/motion, preview/playback, deterministic animation export; separate from static page export |
| Diagram graph | Vector paths, text, transforms, selections | Shape IDs, named ports, bound endpoints, labels, routing, waypoints, arrowheads, container membership |
| Diagram tools | Selection, pan, shapes, text | Connect/reconnect ports, quick-create, notes, containers/swimlanes, grid/snap, minimap, graph-aware copy/delete/group |
| Diagram libraries | Basic geometric shapes | General/Flowchart/UML, ERD/BPMN/wireframe/network/cloud packs; search, install/remove locally, user libraries |
| Diagram layout | Align/distribute | Directed/orthogonal/tree layout, mind maps, cycle/disconnected handling, preserve manually locked placements |
| Diagram data | Existing assistant/command architecture | Validated text/CSV/SQL/Mermaid import, local data fields, refresh mappings and conditional styles |
| Diagram interchange | Raster/SVG import and image export | Editable .drawio XML import/export, library XML, SVG stencil import, VSSX parser with explicit unsupported-feature reports |
| AI surfaces | Existing provider adapters, task preview, commands, generation | Context-specific actions, variants/prompts, design generation/rewrite/resize, diagram generation/tidy/explain/error-path proposals |
| Save/history | ORA, atomic saves, recovery, branches/versions | Versioned multi-page package; page/graph metadata in snapshots; whole-project recovery and dirty checks |

Existing feature-preservation references: `tool-audit-2026-09-19.md`,
`tool-repair-plan.md`, `tool-testing.md`, `photography-workflow-plan.md`, and
`raw-pipeline-backlog.md`. The new layout must provide a reachable location for
every existing action, including uncommon settings, plug-in/provider setup,
branch history, custom toolbars, masks/channels, and advanced brush controls.

## Data and command design

1. Keep `Document` as the raster/vector composition engine for a page. Text and
   paths remain editable source objects; previews and GPU caches are disposable.
   Do not implement pages as giant raster canvases or rasterize copied objects.
2. A project contains stable page IDs, ordered page records, metadata, assets and
   schema version. Each page owns a Document and typed workspace metadata.
   Existing single-page ORA files open unchanged. New multi-page files must never
   silently save through a single-page writer or discard inactive pages.
3. Design metadata describes bleed, object anchors, frame/media relationships,
   reusable styles and animation. It references stable object IDs, not row indices.
4. Diagram metadata is a graph: shapes/containers, ports, connectors with endpoint
   references, labels, routing policy and optional waypoints. Rendering derives
   editable paths/text from that graph. Moving/resizing a node reroutes attached
   edges in the same undo transaction. Deleting/copying nodes handles dependent
   edges and remaps IDs. Containers move descendants exactly once.
5. All mutations use commands and transactions, with headless validation. Page
   operations and graph edits are undoable; switching page/tool/view is not an edit.
   Asset buffers remain shared through Arc to avoid duplicating large images.
6. Persistence uses a bounded, versioned manifest plus page documents and embedded
   assets, written atomically. Validate dimensions, object references, archive
   paths, total decoded sizes, and unsupported schema versions before installation.
   Recovery covers every page. External assets have a missing-file/relink workflow.
7. Preserve useful foundations for later collaboration: stable IDs, explicit
   transactions, deterministic serialization, and separation of document data
   from selection/view state. No account, server, CRDT or presence layer in this
   release. Do not promise collaboration can be added without further design.

## Implementation sequence and acceptance gates

### 1. Shared shell and creation

- [x] Handoff palette and persistent accent/corner controls, light/dark/external themes; bundled fonts.
- [x] Photo/Paint New document dialog with presets, units, dimensions, resolution, name, backgrounds, saved presets and recent sizes.
- [x] Shared handoff geometry, preset search and project destination.
- [x] Home navigation/start cards, workspace routing, responsive menu overflow.
- [x] Photo/Paint dock restructuring; preserve saved custom layouts and shortcuts.
- [x] Library workspace uses actual files and existing batch/develop operations.

Gate: create/open/save/reopen a photo and painting; all current UI regression tests
pass; old settings load; no visible control is a silent no-op.

Initial implementation (September 27): the shared appearance popover and native
New document dialog are wired to real operations. `CanvasSpec` validates creation
before allocation; physical-unit conversion, presets, and creation are headless
core APIs. Paint retains the existing Draw settings/shortcut IDs. Bundled font
licenses are embedded in About and included in the packaging manifest. Custom
presets can be loaded, replaced by name, and removed; reaching the preset limit
does not silently discard existing saved presets.

This first slice did not complete the shell redesign. The page and Design
implementation below followed it; Diagram still needs the structured graph milestone.

Validation for this slice: 480 UI tests passed (one ignored), 147 core tests passed
(one ignored), and 18 settings tests passed. Workspace/all-target Clippy with
warnings denied and Rust formatting passed. The UI suite needs permission to bind
local mock AI/MCP servers; the sandboxed run's 16 permission failures passed on
rerun outside the sandbox. The font license packaging manifest was staged
successfully. These are headless/regression checks, not cross-platform visual or
Vello latency validation.

### 2. Page and project foundation

- [x] Versioned project package, page IDs, cross-page clipboard and project history.
- [x] Add/duplicate/delete/reorder/rename pages, thumbnails, active-page persistence.
- [x] Save/recovery/close prompts include inactive pages; embedded assets round-trip.
- [x] New document pages/background/bleed and custom presets.

Gate: edit two pages, duplicate/reorder/delete/undo, save/reopen/recover; edits,
fonts, vectors, masks and branch history survive. Reject malformed packages without
partially replacing the active document.

Page/Design implementation (September 27): `.emu` packages retain a full native
ORA and branch/version graph for each page. Page structure and page content share
chronological undo during an editing session. Named versions remain per-page;
this is not a whole-project branch system. A failed import installs no pages.
Inactive-page edits participate in save, close, and recovery, and recovered copies
remain on disk until saved or explicitly discarded. Page switching invalidates
GPU/CPU caches and delayed document operations with page-local IDs.

Design now has a native 68 px rail, searchable 250 px drawer, and 88 px page strip
in both retained layouts. Six editable templates add pages, eight vector elements
and five text presets add native objects, and local asset placement embeds sources.
Frames use editable vector clipping boundaries and retain original image pixels
for replacement and cropping. Mixed/grouped text and vector clipboard fragments
preserve sources, hierarchy, and clipping relationships across pages/tabs.

Page exports write PNG/JPEG/SVG ZIPs or a multi-page PDF atomically, with optional
bleed and PDF trim/bleed boxes. SVG/PDF outline glyphs using the editor's shaping
and bundled fonts; unsupported effects use an explicitly reported raster appearance.
The native project remains editable. PDFs use RGB, not a PDF/X or CMYK workflow.
The converter API is documented at [svg2pdf](https://docs.rs/svg2pdf/0.13.0/svg2pdf/).
`cargo run -p emulsion-io --example project_fixture -- <new-output-directory>`
produces an editable project and a complete export fixture for review.

Validation at this checkpoint: 154 core tests passed (one ignored); 484 UI tests
passed (one ignored) with `--test-threads=1`. Three brush-library tests in the
parallel run conflicted on the existing shared test catalog; the isolated and
serial checks pass. The complete I/O suite passed before export additions; all
seven new project/package/export checks pass afterward. The generated two-page
PDF passes qpdf validation and Poppler reports the expected trim/bleed boxes;
the first page was rendered and inspected. Vello's bundled variable-font weight
and synthetic italic settings now match shaping; Linux offscreen GPU coverage
checks pass for regular/bold/italic/bold-italic. These checks do not settle the
remaining native frame-pacing, opaque-edge parity, or other-platform validation.

### 3. Canva-style Design

- [ ] Native Design rail/drawer, contextual toolbar and page strip from handoff.
- [x] Real text presets, editable shapes/elements, local uploads/photos, frames.
- [x] Editable bundled templates and local template save/import/search.
- [ ] Object ordering/grouping/locking/snapping/align/distribute and crop controls.
- [x] Local brand kits and anchored resize variants with overflow review.
- [ ] Apply brands across selected pages and include portable brand logos.
- [x] Selected/all-page PNG/JPEG, vector SVG and print PDF export, bleed options.
- [x] Basic entrance/exit timing, preview, GIF export and presentation playback.

Gate: create a three-page social campaign from a template, replace a photo inside
a frame, edit copied text, apply a brand, make another size, export and reopen
without flattening source objects or modifying the original design.

### 4. draw.io/Lucid-style Diagram

- [x] Graph model, node/port/connector commands and bounded orthogonal routing.
- [x] Native shape library, connector gesture, labels, properties and page tabs.
- [ ] General/flowchart/UML/ERD stencils, notes, containers and swimlanes.
- [x] Manual waypoints, reconnect, arrow/dash/style controls and graph clipboard.
- [x] Auto-layout, minimap, quick-create keyboard authoring.
- [ ] Large-scene render culling and dense-route fallback reporting.
- [x] .drawio interchange, Visio XML/VSSX import, portable local/GitHub packs.
- [ ] draw.io library XML and dedicated SVG stencil interchange.
- [x] Text/CSV/SQL/Mermaid generation and local data fields/conditional formatting.

Gate: build a branched flowchart, move/resize/duplicate nodes, reconnect and label
edges, use nested containers, undo/redo, save/reopen and export/import .drawio while
retaining connectivity. Test loops, disconnected graphs and thousands of shapes.

### 5. Remaining handoff controls and release validation

- [ ] Audit every menu, panel, quick action and option against the handoff and old UI.
- [ ] Implement missing Photo/Paint options without weakening existing tools.
- [ ] Connect Design/Diagram AI to validated commands and reviewable proposals.
- [ ] Keyboard/accessibility, display scale, narrow windows, theme contrast.
- [ ] Migration/performance/platform gates below; document measured limitations.

No milestone is complete because its buttons or screens exist. It needs working
editing, undo, persistence, export where relevant, and regression coverage.

## Vello and text quality remain release dependencies

The previous migration is committed on `feat/vello-migration-ui`. Its remaining
items are tracked in `../spikes/vello-canvas/STAGE2_PENDING.md`: platform validation,
opaque-vector edge/overlap fidelity, and native frame pacing. Do not mark these
complete as a side effect of the redesign.

Matched active-window Linux measurements now show lower GPU brush/text submission
and next-frame callback latency on the dense layered fixture. The older 38–41 ms
GPU callback delay did not recur with fixed viewport dimensions; these runs do
not establish a universal compositor fix or physical input-to-photon latency.
Wider display configurations and other platforms remain unvalidated. See the
September 27 matched follow-up in `spikes/vello-canvas/RESULTS.md`.

Text verification must include native copy/paste within/across documents, zoom
changes, non-integer placement, HiDPI, rotation, multiline/rich text, save/reopen,
SVG/PDF output, and external clipboard fallback. Preserve source font/runs and
transform; never resample a pasted text preview as the source of future renders.
Track GPU submission, frame presentation, input latency, memory, and fidelity
separately. Use representative photo, brush, multi-page design and dense-diagram
fixtures; do not hide fallback behavior to improve benchmark numbers.

## September 27 implementation checkpoint: diagram and creative catalog

The Diagram graph now owns bound endpoints, named/custom ports, orthogonal or
straight routes, manual waypoints, labels, and native shapes. Moving, deleting,
duplicating and copying graph objects update their connections in the same undo
operation. The drawer exposes twelve basic stencils, connection/reconnection,
container membership, per-shape data, layout locks, four automatic layouts, a
snap grid and the existing navigator. Bounded routing can fall back to a route
through obstacles in dense scenes; large-scene layout and router validation are
still release work. Basic Class/Entity shapes do not yet implement full UML/ERD
compartments or the requested external stencil packs.

Editable draw.io XML and compressed pages import together after validation;
exports preserve the common graph primitives. Unsupported style reports remain
visible. The interchange is not yet complete for every draw.io feature or native
effect. Project imports retain all pages and their individual version histories.

Home and the retained editor layouts now expose all six destinations. Routing
activates a compatible open document or opens the correct New document type.
The Library destination retains the existing batch/develop workspace. Its full
collection/keyword management and the Home project-folder redesign remain open.

A bounded, versioned local creative catalog stores asset metadata, searchable
names/tags, attribution/license, ratings, collections, and brand records. Updates
reload under a file lock and replace the catalog atomically. Design has local
asset properties/relink, template save/import/search, and brand font/color
creation, application, and import/export. Brand logos and Library collection
controls still need UI work. Small Design windows show the drawer as an overlay.
Magic opens the existing configured assistant workflow; it does not invent an
AI result or send anything without a submitted request.

Validation before the subsequent motion changes: 486 UI tests passed (one
ignored), including destination routing and graph-aware clipboard/undo. The full
core/I/O suites and all-target Clippy for core, I/O, UI and engine passed. Motion,
resize constraints and their persistence are being implemented after this
checkpoint and require their own tests before the related gates are checked.


## September 27 checkpoint: shared UI completion

The shared shell now uses the handoff's 48px attached tool rail, 38px document
strip and 300px default dock in Photo and Paint. Saved toolbar positions, scales,
floating layouts and the roomy chrome setting remain supported. The dock has
Properties/Adjust/History/Assistant, Swatches/Color and Layers/Channels/Paths
groups. Less common panels remain in More and the Window menu. Sections can
collapse, color/layer heights and dock width can be resized with pointer or
keyboard, and saved workspaces restore the selected panels and collapsed states.
Narrow windows reopen the dock as an overlay without reducing the canvas width.

Home uses the handoff's 220px project navigation and a content area capped at
1240px. Its dashboard contains workspace launch cards, local project previews,
and recent-file grid/list views. Library retains its own destination navigation.
Home details are optional and overlay at narrow widths; More file actions keeps
folder import, presets and project management reachable.
Library starts with a viewport-driven image grid, switches to its existing
preview/develop workflow on image selection, and has a Grid view return action.
Develop/export settings remain reachable through an overlay on narrow windows.
Folder imports, collections, ratings, tags, recipes, selection and batch export
continue to use the existing local file operations and bounded thumbnail loader.

New Document's Save to project picker defaults to the current Home project.
The first successful save records that folder in the persistent Home catalog;
subsequent saves preserve membership. Canceling creation preserves open documents.
This selects a Home organization folder; the normal Save dialog still chooses the
physical file path. Old workspace JSON supplies defaults for the new dock fields.

This completes the shared-shell slice. Advanced Design and Diagram tools,
contextual AI proposals, importer compatibility expansion, and the remaining
Vello performance/platform release gates remain separate milestones below/above;
this checkpoint does not mark the full migration complete.


Validation after the shared shell, standalone image viewer and new splash:
`cargo test --workspace --locked -- --test-threads=1` passed 1,282 tests
(12 ignored), including 500 UI tests (one ignored). Workspace all-target Clippy
with warnings denied, formatting, GPUI vendor validation, license staging,
renderer policy and Linux desktop-entry validation passed. The viewer skips
editor startup services and the splash until Edit in Photo is requested; the
handoff to Photo also skips the splash. Normal Home launch uses the supplied
Emulsion artwork with a version label from the app's build metadata.


## Design functionality plan

The remaining capabilities and delivery order are in
[Design functionality](design-feature-parity.md). The supplied handoff remains
the UI source. Local editing remains the scope; collaboration is deferred.


## September 27 follow-up: rich text and matched canvas measurements

Opaque character-style runs now remain Vello glyphs, sharing paragraph shaping
with CPU rendering and vector export. Text keeps fractional positions through
rendering, transforms and exports. Clipboard regressions cover rotated multiline
rich text with different colors, sizes, italic and raised baselines. GPU outline
coverage passes at 100%, 150% and 200% zoom without creating text raster caches.
Advanced text effects, bounded-height text and translucent paint retain explicit
compatibility rendering.

Validation: 1,284 workspace tests passed (13 ignored), all four explicit GPU font
tests passed, and workspace all-target Clippy including the native benchmark
feature passed with warnings denied. The benchmark comparison policy has three
passing tests. Matched active-window Linux runs passed for synthetic and dense
layered 4K documents, including GPU brush commit/undo/redo. See
[the measured results](../spikes/vello-canvas/RESULTS.md#september-27-matched-active-window-follow-up).
macOS, updated Windows behavior, opaque-vector edge fidelity and the remaining
Design/Diagram capabilities are still open.
