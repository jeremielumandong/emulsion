# Emulsion Design functionality plan

This plan defines the remaining local Design capabilities. Emulsion uses its
own implementation, native document model, Vello renderer and supplied UI
handoff. Existing editing functionality must be preserved. A visible control or
a core API alone does not count as a complete Design workflow.

Collaboration, accounts, public publishing, remote shared libraries and cloud
review remain deferred by the user's local-editing-only requirement. Local
prototype presentation and export are included. Existing template/stencil file
and GitHub-URL exchange remain included.

## Capabilities and acceptance targets

The product target includes Canva-style day-to-day authoring as well as advanced
layout. The reference inventory is Canva's [editing and designing help](https://www.canva.com/help/editing-designing/)
and [feature catalog](https://www.canva.com/features/), reviewed on 2026-09-27.
Those references define user workflows; Emulsion keeps its supplied UI template
and implements its own local document behavior. Cloud collaboration remains a
separate phase. A feature is complete only when its controls, canvas interaction,
Undo, persistence and applicable export work together.

| Everyday workflow | Current status and next acceptance |
| --- | --- |
| Select, multi-select, group, arrange, duplicate, lock, flip and delete | Shared native commands; Design has Select, Position and object actions. Continue auditing pointer, keyboard and narrow-window reachability. |
| Text authoring and typography | Editable rich text, native backgrounds, curved text, shadow/outline, underline and strikethrough. Paragraph-range bullets/numbering, nesting, restart, hanging indents, paragraph alignment/spacing and Enter continuation are implemented in native UI/MCP. Supported glyphs/decorations remain Vello outlines; horizontal-text paragraph limits and effect fallbacks are documented in [text formatting](design-text-formatting.md) and [appearance](design-appearance.md). |
| Reuse formatting | Copy/Paste style plus named reusable styles with linked consumers, explicit publish/reset, detach, rename and removal. Updates are atomic and undoable; native saves and clipboard preserve links. Propagation is page-local; cross-page use imports an independent definition. See [saved styles](design-styles.md). |
| Bulk creation | Local CSV supports inline fields, saved whole-text/local-image mappings and ordered multi-page record sets with remapped presentation links, native UI/MCP, and one-step Undo. See [data bindings](design-data-bindings.md). |
| Charts and tables | Native bar, line and pie charts and tables with direct cell editing, row/column addition and removal, CSV editing, chart-type conversion, titles, sizes and palettes; changes are undoable and saved in projects. Artwork can be detached for free editing. Area, scatter, stacked bar and donut charts, axis controls and merged table cells are implemented. Bounded spreadsheet formulas with cell/range references are implemented. See [charts and tables](design-charts.md). |
| Photo and media editing in Design | Design image Object actions expose source replacement, source-pixel mask crop, clipped Curves/Hue–Saturation adjustments, editable filters, blending/effects and local-model background removal. Smart sources can restore editable originals. Source stamps, locking and Undo use shared native models; transformed-mask and model/runtime limits remain explicit. See [photo workflows](mcp-editor-print-photo.md). |
| Brand and assets | Named typography roles, RGBA palette collections/extraction with explicit paint targets, nested creative asset folders, portable TTF/OTF resources and brand JSON v2 exchange are implemented in UI/MCP. Component families and variable identities support explicit project publication. Logos remain local references; remote libraries are deferred. See [portable brands](design-portable-brands.md). |
| Pages and presentation | Add/remove/reorder pages, resize, manual navigation, clean fullscreen, automatic advance, saved speaker notes, a separate presenter display with timer and slide previews, and Fade/Slide/Zoom page transitions are implemented. Fullscreen hides application controls. Object click navigation, modal overlays and preview-only component variant changes are implemented. PowerPoint interchange remains open. See [interactions](design-interactions.md). See [presentations](design-presentation.md). |
| Video, audio and animation | Editable YouTube link/poster objects and system web-player integration are implemented; Linux system-WebKit playback, audio, pause/resume and resize have been verified; Windows/macOS runtime verification is pending. The current Flatpak runtime lacks WebKitGTK, so that package supports video authoring but not playback. Basic object entry/exit and GIF export exist. Page transitions and an in-app system playback setup helper are implemented. Embedded local video/audio, trim/volume/loop controls and property keyframes with easing are implemented. See [local media and keyframes](design-local-media-keyframes.md). See [Design video](design-video.md). |
| Export and print | Native projects; selected-page image/vector/PDF output; bounded object/frame PNG/SVG/PDF exports; standalone responsive HTML; GIF and sampled animated SVG/Lottie exports with diagnostics. Native print discovery/capabilities, preview/setup and explicit OS-queue submission have host tools. Physical device and separate-platform acceptance remain distinct. See [selection export](design-selection-export.md), [HTML](design-html-export.md), [motion](design-advanced-motion.md) and [printing](mcp-editor-print-photo.md). |
| AI-assisted design | Existing local assistant/provider integration remains available. Generation, extraction and editing actions require explicit capability checks; buttons alone do not establish parity. |
| MCP authoring and automation | Native tools cover authoring, layouts, project assets/variables, catalog administration, workspace lifecycle, image sources, diagram formatting, editor/clipboard state, presentations/media and printing. Source identity, ordered host execution and native Undo remain enforced. Actual remaining setup/gesture boundaries are listed in the [MCP coverage audit](mcp-coverage.md). |

“Partial” means a foundation exists; it does not claim equivalent behavior or
quality. Evidence paths below refer to Emulsion source.

| Capability | Emulsion foundation | Remaining work and acceptance |
| --- | --- | --- |
| Editable vector authoring | Native point/handle editing, join/split, compounds and Boolean operations, skew/envelope/perspective, precision placement and reversible stroke-outline copies; direct canvas Pen editing and UI/MCP controls | Boolean/warp operations can produce sampled contours; solid-stroke expansion retains the original but does not reproduce its masks/effects. Mesh grids have MCP authoring; bespoke on-canvas mesh handles are not implied. See [vector editing](design-vector-editing.md). |
| Object appearance | Direct fill/stroke/alpha/opacity/corners, native text curve/background/effects, 2–16-stop linear/radial gradients with per-stop alpha and hard transitions, matching-object selection and reusable styles | Translucent gradient stops and unsupported compositing use explicit renderer/export fallbacks. Matching ignores hidden objects. See [appearance controls](design-appearance.md) and [vector editing](design-vector-editing.md). |
| Precision and reshaping | Saved ruler units/origins, exact placement and edge spacing, guides/snapping, native transforms, skew/perspective/envelope and monochrome bitmap tracing with preview | Physical ruler units do not reinterpret existing pixel-valued controls. Tracing samples threshold contours rather than reconstructing multicolor illustrations; warped curves may become sampled points. See [precision and tracing](design-vector-editing.md). |
| Responsive frames | Persistent nested row/column/grid layouts, wrapping, padding/gaps, alignment, content sizing on both axes, fill width/height, frame and object min/max limits, object aspect ratios, absolute children, native text reflow and optional content clipping that preserves frame borders and editable sources. UI and MCP share validated, undoable sizing rules. | Add broader constraints. Measure large scenes. Diagram auto-layout is a separate capability. |
| Breakpoints | Authored canvas-width presets select layout and clipping overrides with base-value inheritance; native editor and MCP support editing, inspection, persistence and Undo. See [breakpoints](design-layout-breakpoints.md). | Desktop/tablet/phone/custom preview widths use isolated documents and restore the editing view on exit. Container width queries and per-child/frame-limit overrides are implemented; cyclic content sizing rejects atomically. |
| Components | Local native definitions, named variants, linked instances, explicit publish/reset, switching and detach; acyclic nested dependencies, stable member identities and explicit content/appearance/geometry/opacity/visibility overrides; project library browsing with cross-page import; UI, MCP, save, recovery, clipboard and Undo coverage | Explicit project publishing updates linked families across pages. Nine finer property flags supplement the five override groups. New instances track fine property overrides automatically; legacy files retain explicit behavior. Cross-size imports containing document-sized masks currently reject safely. See [components](design-components.md). |
| Design variables | Named color/number variables with native property bindings, editor controls, MCP, persistence and Undo | Page-local consumers update atomically; unlink/delete retain appearance and imports resolve name collisions. Explicit project variable publication/import and grouped updates preserve local aliases and stable library identities. See [variables](design-variables.md). |
| Image fills | Embedded frame media, replacement, crop editing, Cover/Contain/Stretch and a nine-point crop-focus control | Manual fitting preserves source pixels, rotation, flips, clipping and Undo. Fitted placements persist in native projects. Automatic refitting during responsive frame layout remains part of semantic frames. |
| Interactive presentation | Fullscreen audience, separate notes/timer presenter, transitions, media control and native motion preview; click, pointer-entry and drag-release actions | Navigation/back, modal overlays and variant changes affect isolated preview state. Drag triggers do not move authored objects; modal visibility gates hit-testing/media. See [interactions](design-interactions.md). |
| Frame export | Selection/frame PNG/SVG/PDF export and standalone HTML with embedded artwork/media, sampled responsive widths and supported interactions | Incomplete clipping stacks reject. Static unsupported appearances report raster fallback; strict-vector and HTML paths reject unsupported output. HTML samples native layouts at chosen widths rather than adding a continuous browser layout engine. See [selection export](design-selection-export.md) and [HTML](design-html-export.md). |
| Motion | Transform/opacity/visibility/text-reveal keyframes, easing, presets, bulk retiming, GIF, sampled animated SVG and rendered-frame Lottie export | SVG exports scalable sampled frames where supported; Lottie contains PNG image layers, not editable vector/text interchange. Both omit slide transitions/actions and use silent media posters; no general Lottie importer. Text reveal currently rejects HTML export. See [advanced motion](design-advanced-motion.md). |
| Templates and assets | 110 user-supplied editable starters across 11 searchable categories at authored sizes, ten additional responsive starters in their own category, two earlier starters, local assets, `.emutemplate` packs and GitHub installation | Expand the catalog and connect more templates to responsive layout. Preserve editable source artwork and local exchange. |
| Palettes and brand typography | Named transparent palettes, vector/text color extraction, fill/stroke/text targeting, named typography roles, nested asset folders and bounded portable TTF/OTF resources with private content-derived font identities | Native projects/history/templates/clipboard/component/style imports carry font bytes; SVG/PDF outline glyphs. Single-face editable-embedding fonts only; logos remain external catalog references. Separate-machine font/runtime acceptance remains explicit. See [portable typography](design-portable-brands.md). |
| Format interoperability | Existing layered/vector/page imports and exports | Maintain a feature-level fixture matrix for groups, clipping, masks, fonts, effects and editability. Evaluate missing proprietary-format bridges separately; no unsupported interchange claims. |
| Extensions and automation | Existing assistant/MCP editing and data-only template/stencil packages | Plan extensible authoring workflows separately. Data-only GitHub imports must never become implicit executable plugin installation. Any scripting/plugin runtime needs its own design and permission model. |

## Implementation order

1. Shared shell and standalone image viewer: implemented and regression-tested.
   Preserve these workflows throughout the remaining work.
2. Audit Design's context tools and precision/appearance operations; expose
   existing working tools and fill gaps with original implementations.
3. Semantic responsive frames, optional content clipping and canvas-width
   breakpoints now include native inspectors, MCP and round-trip coverage.
   Non-mutating preview widths are implemented; broader constraints remain separate.
4. Local components, variants and named appearance styles now have persistent
   native data, validation, Undo/recovery/clipboard and consumer updates. Named color/number variables, finer per-field overrides, nested components and
   explicit cross-page component publishing, automatic local overrides and project
   variable libraries are implemented.
5. Clean fullscreen, speaker notes, presenter display and slide transitions are
   implemented, including click/hover/drag interactions. Responsive HTML and object/frame export are implemented with documented format boundaries.
6. Portable brand typography/palettes, asset folders, responsive starters and
   advanced motion authoring/export are implemented. Continue format/runtime
   acceptance and catalog expansion; evaluate unsupported proprietary bridges
   and executable extensions separately.

Each milestone must include a complete user workflow, existing regression tests,
round-trip/recovery tests for new data, and performance measurements on realistic
projects. A core-only implementation or a panel of placeholders does not close a
row. Update this plan as implementation and acceptance checks establish progress.

The shared-tool quality pass fixes exclusive selection across Photo/Draw rails
and their grouped tools, and keeps eligible styled text in Vello when adding a
shadow. Effects retain their existing raster representation; advanced blending
still requires the compatibility canvas. The new application artwork is used by
Home and Linux/Windows packaging, with the full-resolution source used to build
the macOS icon. The capability table above distinguishes implemented workflows from remaining
runtime, format and fidelity limits. Windows and macOS runtime checks remain with
the separate platform machines.

The Design drawer now follows the handoff's seven-item rail, 250 px library,
38 px heading and canvas action bar, two-column preview tiles, format chips,
and text/font-combination cards. Position, Animate and Magic resize use native
editing commands; Frames and the complete tool set remain reachable through
Elements. Instagram, Story, Poster and Presentation chips choose the size of
new editable template pages. Font combinations insert grouped native text.
Layout and interaction tests cover both chrome modes at narrow and wide widths,
plus page creation, alignment, frame fitting, save/reopen and Undo/Redo.
Subsequent semantic frame, breakpoint and nested component implementations
are tracked above.

The Home follow-up replaces the photographic hero and permanent inspector with
project navigation, five workspace launch cards, local project previews and a
spaced recent-files grid/list. Project and workspace filters, name/opened sorting,
search, pinned files, Trash and file details use local records. Extra file actions,
folder imports, saved presets and project management remain available from More
file actions. Photo opens a file picker; switching back to an existing Photo tab
reuses it. New still supports intentionally creating a blank document.

Design now has a focused editor in both chrome settings. The properties/layers
dock opens explicitly, and specialized tool options appear when those tools are
chosen. The selection toolbar provides native text font, size, bold, italic,
alignment and character/paragraph controls; shapes retain properties, duplicate
and delete. The page strip includes thumbnails, page menus, Add, page count and
zoom/fit controls. The native document model, clipboard and history are shared
with the other editing workspaces. These changes do not add collaboration or
complete every remaining advanced Design capability listed above.

The [template fidelity audit](template-fidelity.md) records the completed layout
follow-up, including native icon delivery, project-editor chrome, narrow overlays,
file cards/list actions and proportional creation presets. Feature milestones
above remain open independently of that layout work.

The [starter and layout follow-up](design-starters-and-layout.md) implements the
updated 110-template library across 11 categories, visible page removal, responsive group layout and
the Photo/Paint canvas shortcut flyouts. It does not close the full advanced
Design milestone; the remaining frame capabilities and other rows above are open.
