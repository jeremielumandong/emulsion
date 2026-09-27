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
| Text authoring and typography | Editable rich text, font controls, spacing, paragraph settings and existing warp/path tools. Audit direct controls for text effects, lists and mixed formatting; keep styled text crisp. |
| Reuse formatting | Copy/Paste style transfers text/shape formatting, opacity, blending and effects without replacing the target's content or geometry. One Undo restores a multi-object application. |
| Bulk creation | Local CSV substitutes authored `{{column}}` text fields and generates editable pages as one undoable batch. Image/data bindings, saved field mappings and multi-page record sets remain open. |
| Charts and tables | Native bar, line and pie charts and tables with direct cell editing, row/column addition and removal, CSV editing, chart-type conversion, titles, sizes and palettes; changes are undoable and saved in projects. Artwork can be detached for free editing. Cell merging, formulas, axis controls and additional chart types remain open. See [charts and tables](design-charts.md). |
| Photo and media editing in Design | Existing uploads, frame fitting and shared Photo tools. Audit crop, replacement, adjustments, background removal and effects from the Design selection controls. |
| Brand and assets | Local brand kits and package exchange exist. Need stronger font portability, reusable styles, asset organization and consumer updates. |
| Pages and presentation | Add/remove/reorder pages, resize, a direct Present button, manual slide navigation, fullscreen, optional automatic advance and basic motion exist. Speaker notes, presenter display and PowerPoint interchange remain open. |
| Video, audio and animation | Editable YouTube link/poster objects and system web-player integration are implemented; Linux system-WebKit playback, audio, pause/resume and resize have been verified; Windows/macOS runtime verification is pending. The current Flatpak runtime lacks WebKitGTK, so that package supports video authoring but not playback. Basic object entry/exit and GIF export exist. Local video/audio tracks, trimming, transitions and property animation remain open. See [Design video](design-video.md). |
| Export and print | Existing local image/vector/PDF/native output. Audit selected pages/objects, transparency, bleed, crop marks, sizing and diagnostics per format. |
| AI-assisted design | Existing local assistant/provider integration remains available. Generation, extraction and editing actions require explicit capability checks; buttons alone do not establish parity. |

“Partial” means a foundation exists; it does not claim equivalent behavior or
quality. Evidence paths below refer to Emulsion source.

| Capability | Emulsion foundation | Remaining work and acceptance |
| --- | --- | --- |
| Editable vector authoring | Shared path, shape, text, transform, clipboard, layer and alignment tools | Audit their reachability in Design. Cover point/handle/segment editing, path continuation/join/split, object and node multiselection, precise transforms, compound operations and reversible stroke expansion. |
| Object appearance | Editable fills/strokes, blend modes, vector effects and shape properties | Verify multiple-stop paint on fill and stroke, alpha, gradient geometry/types, reusable appearance, selecting matching objects and export fidelity. Keep original type/shape data editable whenever the operation allows it. |
| Precision and reshaping | Rulers/guides, snapping, transforms and path operations | Add any missing object guides, ruler origins/units, spacing feedback, vector mesh/perspective/skew and bitmap tracing. Validate geometry at rotated/scaled views and one-step Undo. |
| Responsive frames | Persistent nested row/column/grid layouts, wrapping, padding/gaps, alignment, shared row fill width, content-sized height, absolute children and text reflow | Add hug-width/height-fill sizing, min/max, automatic clipping, aspect locks and broader constraints. Measure large scenes. Diagram auto-layout is a separate capability. |
| Breakpoints | Page resize with anchors and optional text reflow | Add authored width-based overrides, inheritance and non-destructive preview widths. Desktop → phone → desktop must recover base values. |
| Components | Editable templates and ordinary groups | Add document-local definitions, linked instances, families/variants, property overrides, propagation, reset and detach. Detect dependency cycles. Save, recovery, clipboard, duplicate/delete and Undo must preserve identity and links. |
| Design variables | Local brand colors/fonts in `creative_ui.rs` | Add named color/number variables and property bindings. Editing a variable updates consumers; unlink/delete retain resolved appearance; imported fragments remap IDs. |
| Image fills | Embedded frame media, replacement, crop editing, Cover/Contain/Stretch and a nine-point crop-focus control | Manual fitting preserves source pixels, rotation, flips, clipping and Undo. Fitted placements persist in native projects. Automatic refitting during responsive frame layout remains part of semantic frames. |
| Interactive presentation | Page presentation and motion preview in `design_motion_ui.rs` | Add object interactions for pointer triggers, navigation/back, overlays and instance variant changes, with supported transitions and a preview-only history. Returning to editing must preserve the authored document. |
| Frame export | Native projects and PNG/JPEG/SVG/PDF export | Add subtree export and standalone responsive HTML with embedded assets and supported interactions. Compare native, image, SVG and browser output across widths. Export is local, not deployment. |
| Motion | Fade/slide/zoom entry/exit, timing and GIF export | Add editable property keyframes, easing, retiming, reveal channels, more original presets and animated SVG/Lottie interchange. Unsupported export content must produce explicit diagnostics. |
| Templates and assets | 110 user-supplied editable starters across 11 searchable categories at authored sizes, two earlier starters, local assets, `.emutemplate` packs and GitHub installation | Expand the catalog and connect more templates to responsive layout. Preserve editable source artwork and local exchange. |
| Palettes and brand typography | Brand colors/font selection and kit import/export | Add named palette collections with transparency, selection extraction and fill/stroke targeting; project asset folders; locally embedded TTF/OTF families and named typography roles. Verify a portable project on a machine without those fonts installed. |
| Format interoperability | Existing layered/vector/page imports and exports | Maintain a feature-level fixture matrix for groups, clipping, masks, fonts, effects and editability. Evaluate missing proprietary-format bridges separately; no unsupported interchange claims. |
| Extensions and automation | Existing assistant/MCP editing and data-only template/stencil packages | Plan extensible authoring workflows separately. Data-only GitHub imports must never become implicit executable plugin installation. Any scripting/plugin runtime needs its own design and permission model. |

## Implementation order

1. Shared shell and standalone image viewer: implemented and regression-tested.
   Preserve these workflows throughout the remaining work.
2. Audit Design's context tools and precision/appearance operations; expose
   existing working tools and fill gaps with original implementations.
3. Build and validate semantic responsive frames and breakpoints in the core;
   add native inspectors and manipulation controls only after the model works.
4. Add variables, components, variants and overrides as persistent document data,
   with migration, validation and complete undo/recovery/clipboard coverage.
5. Add local interactive presentation and responsive HTML/subtree export.
6. Expand brand typography/palettes, original template breadth and motion
   authoring/interchange. Validate interoperability and extension gaps separately.

Each milestone must include a complete user workflow, existing regression tests,
round-trip/recovery tests for new data, and performance measurements on realistic
projects. A core-only implementation or a panel of placeholders does not close a
row. Update this plan as implementation and acceptance checks establish progress.

The shared-tool quality pass fixes exclusive selection across Photo/Draw rails
and their grouped tools, and keeps eligible styled text in Vello when adding a
shadow. Effects retain their existing raster representation; advanced blending
still requires the compatibility canvas. The new application artwork is used by
Home and Linux/Windows packaging, with the full-resolution source used to build
the macOS icon. These fixes do not complete the responsive-frame, component or
advanced motion milestones above. Windows and macOS runtime checks remain with
the separate platform machines.

The Design drawer now follows the handoff's seven-item rail, 250 px library,
38 px heading and canvas action bar, two-column preview tiles, format chips,
and text/font-combination cards. Position, Animate and Magic resize use native
editing commands; Frames and the complete tool set remain reachable through
Elements. Instagram, Story, Poster and Presentation chips choose the size of
new editable template pages. Font combinations insert grouped native text.
Layout and interaction tests cover both chrome modes at narrow and wide widths,
plus page creation, alignment, frame fitting, save/reopen and Undo/Redo.
This pass does not close semantic responsive frames, breakpoints or components.

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
complete the responsive-layout and component milestones.

The [template fidelity audit](template-fidelity.md) records the completed layout
follow-up, including native icon delivery, project-editor chrome, narrow overlays,
file cards/list actions and proportional creation presets. Feature milestones
above remain open independently of that layout work.

The [starter and layout follow-up](design-starters-and-layout.md) implements the
updated 110-template library across 11 categories, visible page removal, responsive group layout and
the Photo/Paint canvas shortcut flyouts. It does not close the full advanced
Design milestone; the remaining frame capabilities and other rows above are open.
