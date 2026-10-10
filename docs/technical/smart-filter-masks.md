# Native Smart Filter stack masks

A Smart Object can retain one editable raster `SmartFilterMask` for its complete
filter stack. It is independent of the ordinary raster and vector layer masks.
It has raw coverage pixels, enabled/link flags, an intrinsic-to-source affine,
and the shared density/feather properties. No individual filter owns a mask.

## Native rendering rule

`NodeKind::Smart.cache` remains the enabled, unmasked styled stack result F.
[Independent root and item visibility](smart-filter-enabled-state.md) bypasses
disabled stages while retaining their descriptors. For each pixel
in that cache's expanded grid, S is the original source pixel at
`(x + offset.x, y + offset.y)`, or transparent outside the finite source. Each
linear-premultiplied RGBA16 component in `Linear` and legacy `Srgb` becomes:

`(S * (255 - coverage) + F * coverage + 127) / 255`

Black restores the source, white reveals the filters, and gray mixes them.
Alpha participates in the same integer operation in **all** profiles. Under
explicit `PhotoshopSrgbV1`, straight RGB is sRGB-encoded, premultiplied by its
endpoint alpha, crossfaded with coverage, then unpremultiplied and decoded to
native linear storage using that unchanged rounded alpha. Zero/white coverage
retains exact endpoint pixels. Legacy profiles retain their byte-identical
integer mix. This is neither source-over nor a layer-alpha mask. The ordinary masks, effects, blend/clip/adjustment pipeline,
and layer opacity follow the effective pixels in their existing order.
With an active filter stack, missing/disabled masks, zero density and constant
white planes return the raw cache directly. Empty or root-disabled stacks, and
stacks with no enabled stages, return the source Arc, retaining their dormant
masks. Enabled zero-opacity stages remain active and preserve the existing
cache extent. These read paths never rewrite the source, raw cache, offset or
authored metadata.

The mask is always source-local. The canonical raster-mask projection composes
`T(-cache_offset) * mask_affine` before inversion. Density and intrinsic-pixel
feather are derived through the shared raster-mask implementation before affine
sampling. Inspection ignores enabled state; it never changes export appearance.
The old integer mix and every filter kernel remain native contracts. The
versioned RGB mask mix has bounded [independent final-pixel evidence](../../crates/emulsion-core/tests/fixtures/photoshop-smart-filter-mask/README.md)
from application-authored PSD, untagged captures: stored-encoded-RGB interpolation
matches within one 8-bit code, whereas linear interpolation misses by up to 34.
This does not establish source colorimetry, Gaussian/resize parity, per-filter
blend/opacity semantics, or editable PSD Smart Filter interchange.

## Cache and resource boundaries

Mask edits never rerun the filters. A separate 32 MiB warm cache retains stable
effective-raster identities and weak references for larger live results. It
keys source, full-stack cache and raw-mask identity/content IDs, dimensions,
affine, offset, properties and document blend profile. Weak references guard against pointer reuse.
All possible tiled mip allocations count toward the warm budget. Expensive
projection and mixing happen outside the cache mutex. The shared 32 MiB
raster/vector coverage cache remains separate.

Document appearance uses `effective_pixels_with_space`; the node-only
`effective_pixels` API explicitly retains legacy linear behavior. The document
composite tree, bounds/tracing, Rasterize/Apply Layer Mask, ORA layer resources
and PPTX pictures pass the profile. Styles' solo documents and cache keys already
carry it. Canvas/source-only lowering, previews/thumbnails, clipboard/selection
capture and other flattened exports consume the same document tree. Profile
changes dirty the document and choose a distinct effective cache entry; the
raw source, full filter cache, mask and OriginalImage are never rewritten.

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

Applying a fractional raster layer mask rounds each covered linear-premultiplied
channel to native RGBA16 storage before regenerating styles. A live mask retains
fractional float coverage through compositing. The initial storage difference is
at most 127/255 of one 16-bit code; later effect generation and encoded-sRGB
compositing can amplify it. Exact straight-RGBA8 equality near transparent pixels
is therefore not the mask-bake contract. Tests require exact independently
rounded source storage and reference rendering, exact representable cases, and
an independent float compositing model with opaque-backdrop appearance checks.
Rasterize retains the ordinary mask and its exact rendering contract. The
independent reference-application rasterized-target tolerance is unchanged.

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
12, including disabled, white and dormant states. The separately versioned
`PhotoshopSrgbV1` document profile requires version 14; this RGB correction adds
no further schema or mask resource version. Vector-only state remains
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

`smart_filter_mask_tests`, `smart_filter_mask_space_tests`,
`profiled_smart_filter_masks`, `smart_filter_mask_cache::tests`,
`native_smart_filter_masks`, the RAW substitution/recipe regressions and the UI
filter-mask tests cover the source contracts. The engine parity matrix covers
cache on/off, mask edits, independent layer masks, clipping, effects and reload.
GPU verification requires an actual adapter; set `EMULSION_REQUIRE_GPU_TESTS=1`.
Native brush/gradient/target/transform/save/reopen workflows require separate
serial UI testing. Test source is not evidence that those gates have run.
