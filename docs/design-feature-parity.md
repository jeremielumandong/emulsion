# Design feature parity reference

User-selected reference: [Omadesign](https://github.com/michaelmonetized/omadesign),
local checkout `/home/arkane/Projects/omadesign`, revision
`0469c5b4df4a7525d172cf0dce95d38f0c055805` (reviewed September 27, 2026).
The comparison uses its README and documentation: `docs/MANUAL.md`,
`docs/layout.md`, `docs/format-support.md`, and `docs/plugins.md`. Release labels
in those documents differ; the revision above defines this comparison.

This is a capability target, not a source of implementation, templates, assets,
copy, or styling. Emulsion keeps the supplied UI handoff, native document model,
Vello renderer, existing functionality and its own implementations. Design parity
is not complete. A visible button, a core API, or a similar Photo tool alone does
not count as an end-to-end Design workflow.

Collaboration, accounts, public publishing, remote shared libraries and cloud
review remain deferred by the user's local-editing-only requirement. Local
prototype presentation and export are included. Existing template/stencil file
and GitHub-URL exchange remain included.

## Comparison and acceptance targets

“Partial” means a foundation exists; it does not claim equivalent behavior or
quality. Evidence paths below refer to Emulsion source.

| Capability | Emulsion foundation | Remaining work and acceptance |
| --- | --- | --- |
| Editable vector authoring | Shared path, shape, text, transform, clipboard, layer and alignment tools | Audit their reachability in Design. Cover point/handle/segment editing, path continuation/join/split, object and node multiselection, precise transforms, compound operations and reversible stroke expansion. |
| Object appearance | Editable fills/strokes, blend modes, vector effects and shape properties | Verify multiple-stop paint on fill and stroke, alpha, gradient geometry/types, reusable appearance, selecting matching objects and export fidelity. Keep original type/shape data editable whenever the operation allows it. |
| Precision and reshaping | Rulers/guides, snapping, transforms and path operations | Add any missing object guides, ruler origins/units, spacing feedback, vector mesh/perspective/skew and bitmap tracing. Validate geometry at rotated/scaled views and one-step Undo. |
| Responsive frames | Clipping frames and page resize anchors in `design.rs` and `design_metadata.rs` | Add semantic nested frames with stack/wrap/grid, independent padding/gaps, alignment/distribution, fixed/hug/fill sizing, min/max, absolute children, constraints, clipping and aspect locks. Diagram auto-layout is a separate capability. |
| Breakpoints | Page resize with anchors and optional text reflow | Add authored width-based overrides, inheritance and non-destructive preview widths. Desktop → phone → desktop must recover base values. |
| Components | Editable templates and ordinary groups | Add document-local definitions, linked instances, families/variants, property overrides, propagation, reset and detach. Detect dependency cycles. Save, recovery, clipboard, duplicate/delete and Undo must preserve identity and links. |
| Design variables | Local brand colors/fonts in `creative_ui.rs` | Add named color/number variables and property bindings. Editing a variable updates consumers; unlink/delete retain resolved appearance; imported fragments remap IDs. |
| Image fills | Embedded frame media, replacement and crop editing | Add explicit cover/contain/stretch and focal-point controls. Keep destination hierarchy stable during asynchronous placement. Source-file removal after save must not break images. |
| Interactive presentation | Page presentation and motion preview in `design_motion_ui.rs` | Add object interactions for pointer triggers, navigation/back, overlays and instance variant changes, with supported transitions and a preview-only history. Returning to editing must preserve the authored document. |
| Frame export | Native projects and PNG/JPEG/SVG/PDF export | Add subtree export and standalone responsive HTML with embedded assets and supported interactions. Compare native, image, SVG and browser output across widths. Export is local, not deployment. |
| Motion | Fade/slide/zoom entry/exit, timing and GIF export | Add editable property keyframes, easing, retiming, reveal channels, more original presets and animated SVG/Lottie interchange. Unsupported export content must produce explicit diagnostics. |
| Templates and assets | Six original editable starters, local assets, `.emutemplate` packs and GitHub installation | Expand the original catalog across document types/proportions, with search/filter/preview and scalable layouts. Never import the reference's artwork, templates or text as our built-ins. |
| Palettes and brand typography | Brand colors/font selection and kit import/export | Add named palette collections with transparency, selection extraction and fill/stroke targeting; project asset folders; locally embedded TTF/OTF families and named typography roles. Verify a portable project on a machine without those fonts installed. |
| Format interoperability | Existing layered/vector/page imports and exports | Maintain a feature-level fixture matrix for groups, clipping, masks, fonts, effects and editability. Evaluate missing proprietary-format bridges separately; no unsupported interchange claims. |
| Extensions and automation | Existing assistant/MCP editing and data-only template/stencil packages | Compare extensible authoring workflows separately. Data-only GitHub imports must never become implicit executable plugin installation. Any scripting/plugin runtime needs its own design and permission model. |

## Implementation order

1. Finish and validate the shared shell and standalone image viewer already in
   progress. Preserve all established editing and file workflows.
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
row. Re-run the comparison when the reference revision changes; additions do not
silently expand the accepted target.
