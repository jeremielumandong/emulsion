# Native Smart Filter stack masks

A Smart Object can retain one editable raster `SmartFilterMask` for its complete
filter stack. It is independent of the ordinary raster and vector layer masks.
It has raw coverage pixels, enabled/link flags, an intrinsic-to-source affine,
and the shared density/feather properties. No individual filter owns a mask.

## Native rendering rule

`NodeKind::Smart.cache` remains the full styled stack result F. For each pixel
in that cache's expanded grid, S is the original source pixel at
`(x + offset.x, y + offset.y)`, or transparent outside the finite source. Each
linear-premultiplied RGBA16 component becomes:

`(S * (255 - coverage) + F * coverage + 127) / 255`

Black restores the source, white reveals the filters, and gray mixes them.
Alpha participates in the same operation. This is neither source-over nor a
layer-alpha mask. The ordinary masks, effects, blend/clip/adjustment pipeline,
and layer opacity follow the effective pixels in their existing order.
Missing/disabled masks, zero density, constant white planes and empty filter
stacks return the raw cache directly. An empty stack retains its dormant mask.

The mask is always source-local. The canonical raster-mask projection composes
`T(-cache_offset) * mask_affine` before inversion. Density and intrinsic-pixel
feather are derived through the shared raster-mask implementation before affine
sampling. Inspection ignores enabled state; it never changes export appearance.
The integer mix and blur kernel are native contracts, not Photoshop rendered
equivalence guarantees.

## Cache and resource boundaries

Mask edits never rerun the filters. A separate 32 MiB warm cache retains stable
effective-raster identities and weak references for larger live results. It
keys source, full-stack cache and raw-mask identity/content IDs, dimensions,
affine, offset and properties. Weak references guard against pointer reuse.
All possible tiled mip allocations count toward the warm budget. Expensive
projection and mixing happen outside the cache mutex. The shared 32 MiB
raster/vector coverage cache remains separate.

The mix and editing-plane padding materialize one tile at a time, without a
full-plane temporary. Stored descriptors and output grids use the native
30,000-pixel-side/400 MP limits. These bounds do not promise a fixed total
process-memory limit for large source, filter and output rasters.

Painting can pad the stored mask to cover the current expanded cache footprint.
It preserves old pixels and infinite fill, adjusts the affine by the new
intrinsic origin, and retains feather units. Interactive growth is limited to
30,000 pixels per side and 16 MP; an over-budget extent is rejected rather than
clipped. Existing larger planes can be used without growth. Padding plus paint
belongs to one gesture; position locks also protect affine changes introduced
by padding. Arbitrary mask warp is unsupported.

## Geometry and editing lifetime

Linked masks follow content placement; unlinked masks preserve their document
mapping. Image-wide crop/resize/rotation transforms every component, including
disabled and unlinked masks. Source replacement preserves the mask's world
mapping and raw pixels. Filter spread changes only the derived grid.

Rasterize bakes the effective result and consumes Smart/filter-mask state while
preserving ordinary layer-mask descriptors. Applying an ordinary raster layer
mask to a Smart Object also bakes the effective result and consumes the Smart
stack, preserving the independent vector component. Convert to Layers restores
the original editable source and intentionally discards the stack/filter mask.
Undo restores the original descriptor. Unsupported resampling rejects before
publication; trimming leaves Smart Objects untouched.

First-filter UI application captures source placement and selection before the
background render. Conversion, the rendered stack and initial mask publish as
one atomic edit. Later filter changes preserve the same mask; deleting the last
filter leaves it dormant. Deleting a mask restores the full filtered appearance.
A later parameter change does not create a new mask. Background publication
carries the exact rendered filter styles, and cannot enter a foreign modal or
mask transaction. Target switches cancel pending conservative node-snapshot
requests; canceled work cannot revive.

## Persistence and interchange

Any live or saved-history filter-mask descriptor requires native/history version
12, including disabled, white and dormant states. Vector-only state remains
version 11, raster-property state version 10, and default legacy state version 9.
Pre-12 declarations carrying a descriptor are malformed. PNG resources use a
separate `emulsion/filter-mask-<id>.png` path; history planes share the existing
8-bit tile pool. Raw F is preserved independently of disposable effective pixels.
Opaque nested source archives retain their exact bytes and independent version.

Native files retain editability. Generic ORA and PSD use conservative named
whole-appearance fallbacks when a filter-mask descriptor exists. Flattened pixel
exports use the native composite. PPTX Smart pictures use effective pixels and
cache placement, with existing warnings for nonportable ordinary effects/masks.
There is no editable PSD Smart Filter record round-trip or conversion of a PSD
ordinary layer mask into this component. Unsupported recipe/Lottie exports
reject mask-bearing nodes, including disabled/dormant descriptors.

Channels-panel integration, individual-filter masks, cross-object mask dragging,
arbitrary projective/mesh warps and mask-filter processing are outside this
implementation.

## Verification gates

`smart_filter_mask_tests`, `smart_filter_mask_cache::tests`,
`native_smart_filter_masks`, the RAW substitution/recipe regressions and the UI
filter-mask tests cover the source contracts. The engine parity matrix covers
cache on/off, mask edits, independent layer masks, clipping, effects and reload.
GPU verification requires an actual adapter; set `EMULSION_REQUIRE_GPU_TESTS=1`.
Native brush/gradient/target/transform/save/reopen workflows require separate
serial UI testing. Test source is not evidence that those gates have run.
