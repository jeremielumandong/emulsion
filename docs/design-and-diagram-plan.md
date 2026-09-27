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
- [ ] Finish handoff geometry across existing custom controls; preset search and project destination.
- [ ] Home navigation/start cards, workspace routing, responsive menu overflow.
- [ ] Photo/Paint dock restructuring; preserve saved custom layouts and shortcuts.
- [ ] Library workspace uses actual files and existing batch/develop operations.

Gate: create/open/save/reopen a photo and painting; all current UI regression tests
pass; old settings load; no visible control is a silent no-op.

Initial implementation (September 27): the shared appearance popover and native
New document dialog are wired to real operations. `CanvasSpec` validates creation
before allocation; physical-unit conversion, presets, and creation are headless
core APIs. Paint retains the existing Draw settings/shortcut IDs. Bundled font
licenses are embedded in About and included in the packaging manifest. Custom
presets can be loaded, replaced by name, and removed; reaching the preset limit
does not silently discard existing saved presets.

This does **not** complete the shell redesign or add Design/Diagram pages yet.
Those workspace choices depend on the page/project and graph milestones below;
do not present ordinary single-page image tabs as completed Design/Diagram editors.

Validation for this slice: 480 UI tests passed (one ignored), 147 core tests passed
(one ignored), and 18 settings tests passed. Workspace/all-target Clippy with
warnings denied and Rust formatting passed. The UI suite needs permission to bind
local mock AI/MCP servers; the sandboxed run's 16 permission failures passed on
rerun outside the sandbox. The font license packaging manifest was staged
successfully. These are headless/regression checks, not cross-platform visual or
Vello latency validation.

### 2. Page and project foundation

- [ ] Versioned project package, page IDs, cross-page clipboard and project history.
- [ ] Add/duplicate/delete/reorder/rename pages, thumbnails, active-page persistence.
- [ ] Save/recovery/close prompts include inactive pages; embedded assets round-trip.
- [ ] New document pages/background/bleed and custom presets.

Gate: edit two pages, duplicate/reorder/delete/undo, save/reopen/recover; edits,
fonts, vectors, masks and branch history survive. Reject malformed packages without
partially replacing the active document.

### 3. Canva-style Design

- [ ] Native Design rail/drawer, contextual toolbar and page strip from handoff.
- [ ] Real text presets, editable shapes/elements, local uploads/photos, frames.
- [ ] Editable bundled templates and local template save/import/search.
- [ ] Object ordering/grouping/locking/snapping/align/distribute and crop controls.
- [ ] Local brand kits and anchored resize variants with overflow review.
- [ ] Selected/all-page PNG/JPEG, vector SVG and print PDF export, bleed options.
- [ ] Animation authoring/preview/export and presentation playback.

Gate: create a three-page social campaign from a template, replace a photo inside
a frame, edit copied text, apply a brand, make another size, export and reopen
without flattening source objects or modifying the original design.

### 4. draw.io/Lucid-style Diagram

- [ ] Graph model, node/port/connector commands and bounded orthogonal routing.
- [ ] Native shape library, connector gesture, labels, properties and page tabs.
- [ ] General/flowchart/UML/ERD stencils, notes, containers and swimlanes.
- [ ] Manual waypoints, reconnect, arrow/dash/style controls and graph clipboard.
- [ ] Auto-layout, minimap, large-scene culling and keyboard authoring.
- [ ] .drawio interchange, SVG/XML/VSSX stencil import, local pack management.
- [ ] Text/CSV/SQL/Mermaid generation and data fields/conditional formatting.

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

The corrected native benchmark shows lower brush/text submission time, but GPU
next-frame callbacks around 38–41 ms and unmatched viewport dimensions prevent a
claim of an overall latency win. Repeat matched-window tests after shell changes.

Text verification must include native copy/paste within/across documents, zoom
changes, non-integer placement, HiDPI, rotation, multiline/rich text, save/reopen,
SVG/PDF output, and external clipboard fallback. Preserve source font/runs and
transform; never resample a pasted text preview as the source of future renders.
Track GPU submission, frame presentation, input latency, memory, and fidelity
separately. Use representative photo, brush, multi-page design and dense-diagram
fixtures; do not hide fallback behavior to improve benchmark numbers.
