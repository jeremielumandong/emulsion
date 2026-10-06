# Checked retained-pixel projective rendering

`NodeContent::projective_pixels` adds a render-only source family. It retains the
original immutable `LazyRaster` and projects pixels while rendering requested
output tiles. It does not change Smart placement, commands, persistence, masks'
authored coordinate types, PSD import, or the Distort UI.

## Admission and source identity

`ProjectivePixels` owns a private source/mapping pair. Construction checks positive
source dimensions, 30,000 pixels per side and 400 million pixels, then resolves a
deferred source once and checks its decoded dimensions. A mismatch is an explicit
`ProjectivePixelError`, before a caller publishes the node. Legacy lazy sources
and Placement behavior are unchanged. Reads retain the source Arc and content ID;
only the existing Plane-owned mip cache can grow.

`ProjectivePixelMapping` stores checked forward and inverse geometry, its intrinsic
rectangle and conservative integer bounds. It additionally validates the union
of the bilinear support rectangles for all source mip levels. This includes half
a mip pixel outside each edge and rounded-up dimensions at odd mip sizes. A pole
in that expanded forward support is conservatively rejected, even if the exact
source rectangle alone is finite. Bounds approve culling, never allocation.

Inverse construction rebases the forward map before inversion. Admission then
independently certifies G * Translate(-anchor) * F against the authoritative
original forward coefficients. Compensated matrix products carry coefficient
error intervals through both products. The inverse round-trip residual has
quadratic numerators: every x², xy and y² term is bounded over the full source
support, with the denominator bounded away from zero. Derivatives of that
residual bound the inverse-Jacobian relative error in operator norm.

The inverse sampling domain uses interval-evaluated original translated-forward
corners, not just the rounded rebased intermediate or the enclosing AABB. That
AABB may cross the inverse horizon. Admission combines matrix construction,
document-point subtraction, FMA/division and differential evaluation error in
one 1e-6 source-pixel budget and one 1e-6 relative Jacobian-norm budget. A valid
map whose domain cannot meet these conservative checks is explicitly rejected.
There is no pointwise precision-failure-to-transparent path. Exterior support
and an inverse horizon outside the quad return transparent pixels or mask fill.
An unexpected zero differential defensively uses the coarsest mip, never mip zero
or a transparent hole; checked admission makes that fallback unnecessary for
supported descriptors.

### External mask admission and the low-level API boundary

The private payload certifies source/mapping pairing only. CompositeNode.mask is
a separately mutable, public field; an independent mask can have dimensions
unrelated to the source. Before rendering a newly assembled projective scene,
use try_render_tile_cpu / try_flatten, or call
CompositeTree::validate_projective_resources before the existing low-level
rendering functions. This fallible helper enforces positive mask dimensions,
30,000 pixels per side and 400 MP on all reachable mask planes, including hidden
nodes, group baselines, styled clip sources and effect-mask sources. Repeat
admission after changing external masks or derived input trees. Invalid masks
are rejected before mip work; they are never cropped, ignored or mip-clamped.

As with existing Plane and affine composite descriptors, raw public fields and
direct low-level render_tile/flatten calls retain caller responsibility. This is
not an immutable checked-tree API. Legacy-only trees retain their existing
producer-validation behavior. Document::validate already enforces authored mask
limits; future native projective producers must keep that real validation and
also admit their derived render inputs. Engine compile/recompile and the GPU
entry validate projective resources before preparing sources.

## Sampling and resource limits

For each document-space output center, the sampler uses
`floor(log2(max(1, sigma_max(J_inverse) * document_pixels_per_output_pixel)))`,
clamped to the source plane's available levels. Logarithms are added instead of
multiplying potentially large footprints. This deterministic isotropic policy
can blur anisotropic directions; it makes no Photoshop interpolation-equivalence
claim. Legacy Pixels retain their determinant-based mip rule and all exact-copy
fast paths.

Bilinear interpolation uses linear premultiplied storage. Pixel exterior is
transparent; masks use their authored fill. Both follow the same inverse map,
even if mask dimensions differ. Knockout's full-source shape follows the same
projected rectangle, including exact integer mip reductions at odd image edges,
without allocating a source-sized opaque raster. Masks and rectangle clips apply
once to the shape and once to paint.

Scratch consists of one fixed TILE_PX sample grid, tile-sized color/coverage
buffers, and eight source tile handles per active plane. Cache keys include mip
level. There are at most four tile lookup attempts per output pixel per plane;
finite support is checked before every integer conversion and fetch. No inverse
source window or forward document AABB is allocated. Plane's existing lazy mip
pyramid remains source-sized, with its own recursive work and retention bounded
by each admitted source or independent mask plane's own limits; the eight-entry
lookup cache does not bound that pyramid.

## Renderer integration

All three CPU blend profiles use the same geometry sampler and preserve their
existing blend arithmetic. Ordinary clipping bases and styled shape/mask sources
recognize projected content. The hybrid GPU compositor prepares it through its
CPU-reference source path. PhotoshopSrgbV1 continues to explicitly decline GPU
compositing.

The engine canvas always uses a document-bounded CPU source bake. Projective
content never takes an identity-source or Vello shortcut. Both bake keys and
node signatures include the full checked mapping and source identity. Unchanged
inputs reuse the bake; changed projective pixels currently request a full bounded
document bake instead of affine incremental-damage inference.

## Validation

Focused tests in `projective_sample_tests.rs` cover independent slow inverse
sampling, perspective/reflection, every blend profile, mask fill, bilinear halo,
local mip changes, odd-sized knockout mips, inverse-horizon AABBs, large off-canvas
bounds, bounded cache access, unchanged source identity, clipping, styled masks,
legacy affine exact-copy behavior, visible large-offset anisotropic
minification against a separate analytic inverse, real output seams at reduced
levels, and rejection of oversized external masks. GPU routing/device tests and engine
bake/signature tests cover the separate viewport routes. Authored tests are not
execution evidence: run the repository's required checks before native editing
can use this render-only foundation.
