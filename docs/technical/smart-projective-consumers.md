# Smart projective consumer boundaries

This is the consumer portion of the native Smart mapping cutover. It depends on
core's authoritative `SmartPlacement` / `Mapping2`, checked support admission and
fallible document preparation, and on IO's native v16 adapters. It does not expose
a projective-transform MCP tool, Distort gesture, projective Again recipe, mesh
Warp, rational vector paths, mask-copy semantics or Photoshop projective parity.

## Rendering and publication

Production document rendering uses `Document::try_composite_tree`. Engine
compilation, signatures and rebakes propagate preparation errors. The existing
checked `ProjectivePixels` source, CPU bake, GPU admission and CPU fallback routes
remain authoritative. Mixed Legacy placement plus projective mask metadata still
produces the ordinary affine pixel content branch. A projective identity placement
retains the projective branch.

The workspace prepares both a checked editor and its first complete scene before
hiding the previous tab. Synchronous and asynchronous scene rebuild failures keep
the last valid scene and surface the error. Thumbnail, inspection, AI input,
classification, animation, batch and MCP render producers propagate errors to their
existing result/status surfaces; they do not construct empty scenes or remove
failed layers. Checked storyboard/design-motion evaluation errors remain errors
before scene preparation: they retain the Stage/player picture and the last
accepted design preview/transition, rather than selecting the authored document
or cutting away a failed outgoing transition. No-motion and thumbnail-loading
absence remain separate successful states. GIF frame errors flow through the existing staging-file encoder,
which publishes its destination only after every frame succeeds.

Finite UI inspection frames remain optional. Unavailable bounds suppress the
corresponding read-only outline, hit-test or affordance. Bounds needed to copy or
import multiple roots are collected fallibly before publication, so an invalid
root is not omitted from an otherwise successful operation. IO retains ownership
of native/strict-vector export admission and selection/bleed crop publication.

## Existing editing tools

Affine numeric fields, Grab/preview, Warp, affine mask initialization/painting,
mask movement and affine mask inspection refuse `Node::has_projective_metadata`.
That predicate includes identity-projective descriptors, disabled filter masks,
mixed states and plane-absent raster mappings. Refusal precedes the tool's history,
selection or transaction changes. Mask transform preparation cannot fall back to
moving the owner's artwork. Rasterize, Apply Layer Mask and Convert to Layers
perform the same check before finishing other UI interactions.

Whole-content move and existing affine Again continue through the generalized
core commands. Legacy branches retain their original `Placement` arithmetic;
projective outlines use checked full-source corners. Existing mask Copy and Again
with Copy refusals remain. Relative affine commands do not author a projective
repeat recipe.

Smart filter jobs preflight the prospective footprint and masks before rendering.
Initial selection-based mask creation remains an affine tool and refuses retained
projective owners. Completion checks include source and raw-cache identity,
placement, exact mapping coefficient keys, descriptors and grid. MCP filter plans
capture these inputs internally and reject stale publication before commands or
deferred effects. These snapshots are not serialized into replies or logs.

## Readback

MCP returns projective placement as an exclusive nine-coefficient object rather
than invented x/y/scale/rotation values. It includes full raster/filter component
maps, latent raster-plane presence, linked/enabled state, checked source corners,
and finite-bounds errors. Legacy readback retains its existing numeric values.
The native runtime geometry types are not made serializable for this purpose.

## Remaining legacy-only entry points

The production callsite sweep found no remaining direct `composite_tree()` call
outside explicit test fixtures and synthetic examples. Remaining `Editor::new`
uses initialize fresh affine documents before any later commands:

- `emulsion-mcp/src/storyboard_tools/stage.rs`: new empty stage document
- `emulsion-ui/src/editor/design_ui.rs`: new typography-pair preview
- `emulsion-ui/src/editor/diagram_library_picker.rs`: new stencil-preview canvas
- `emulsion-ui/src/editor/recipes.rs`: new raster-only recipe thumbnail document

`design_video_ui.rs` also constructs a fixed new document inside its Linux-only
`host_control_tests` module; it is a test fixture, not a production caller.
`ChartEditor::new` in `design_charts_ui.rs` is a chart form constructor, not a core
editor. `EditorView::new` remains an explicitly legacy-only fixture convenience;
the only non-test caller is `navigation_benchmark.rs`, whose `document()` is a
synthetic affine fixture. The file-loading canvas benchmark and project navigation
example use checked preparation. AI's legacy-only compatibility wrappers are used
only by their affine fixtures; runtime callers use the fallible variants.

## Verification status

The source migration was formatted and checked for whitespace errors. Authored
regressions cover retained-model engine routing, typed MCP projective/latent mask
readback, stale same-size filter inputs, P_ANY tool refusal before UI transaction
changes, and inactive-cache mask inspection. Compilation, Clippy, tests and GPU/GUI
execution are not claimed for this isolated consumer branch. They require the
coordinated core, IO and consumer integration and its exact-commit validation.
