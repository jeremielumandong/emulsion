# Native editable vector masks

A node can retain one raster layer mask and one independent editable vector
layer mask. `VectorMask` stores an `Arc<Path>`, enabled/link/inversion flags, an
intrinsic-to-layer-local affine, density/feather properties, and an explicit
empty-path coverage state. Derived coverage pixels are disposable and are never
the authoritative vector geometry.

## Coverage and coordinates

- Nonempty geometry uses nonzero winding with implicit fill closure. Open
  subpaths remain open in the editable data. Per-subpath Boolean operation
  records and a global Paths panel are outside this implementation.
- Empty Reveal All and Hide All are infinite coverage states, not canvas-sized
  rectangle paths. Adding an anchor uses ordinary path fill; deleting the last
  anchor restores the stored empty state. Inversion complements either case.
- Path anchors and handles remain finite within ±1 billion, with at most 20,000
  anchors and 20,000 subpaths. Nothing trims the stored path to the canvas.
- Affines have no separate translation cap. Their entries, inverse, determinant,
  and forward/inverse mappings of the intrinsic ±1 billion coordinate envelope
  must remain finite, with determinant magnitude at least 1e-12. Thus a stored
  translation of ±10 billion is supported; a finite matrix whose mapped envelope
  overflows is rejected. Rendering must also pass the work limits below.
- Raster/Smart layer-local coordinates are source pixels. Other layer kinds
  use document pixels. A Smart cache's expanded origin is applied only when
  deriving its output grid, never by moving the stored vector geometry.
- Unfeathered geometry is freshly rasterized in the requested output grid using
  4×4 nonzero fill samples with direct cubic scanline intersections. Cubics
  split at their vertical extrema and use bounded monotonic root solving; a
  fixed polyline-segment cap cannot flatten away a locally visible off-canvas
  curve. This is not a GPU/device-resolution vector-mask renderer. Layer placement and canvas zoom still use the existing
  source-grid compositor and its resampling.
- Feather uses the same native three-box approximation as raster masks, in
  intrinsic mask pixels before affine sampling. Density is
  `255 - density * (255 - coverage)`, rounded to a byte. Feather below 0.5 pixels
  is the existing kernel's no-blur range. Feathered geometry uses the intrinsic
  pixel kernel, rather than silently changing the radius under affine scale.
- Enabled components multiply independently using `(raster * vector + 127) /
  255` in integer arithmetic. Absent, disabled, and zero-density components
  contribute full coverage. Inspection and mask-to-selection helpers expose
  one component, ignoring its enabled flag. Applying Raster Mask bakes/removes
  only that component, preserving the vector mask.
- The existing Layer Mask Hides Effects option controls the combined coverage;
  there is no independent vector-mask-hides-effects setting.

## Resources and cache

Rasterization uses bounded output blocks. Feathered windows derive inverse
output sample footprints plus the complete intrinsic kernel support, splitting
only when the temporary coverage window exceeds 16 million pixels. If no edge
control hull touches the complete support rectangle, its winding is provably
constant; that window requires no intrinsic rasterization or blur. This handles
extreme minification of large nonempty interior/exterior regions without
repeating enormous feather halos.

Memory and cumulative work are separate limits. A preflight plan permits at most
4096 windows and 256 million conservative work units across coverage visits,
scanline edge checks, bounded cubic solves, sorting and the actual repeated
halo work of the 64-column blur kernel. The output grid remains bounded by the
native 30,000-pixel side/400MP limits. Commands and decoded live/history documents
reject an over-budget vector state before publishing or rendering it, with a
message to reduce feather, path complexity or extreme mask scale. The 1000 px
numeric feather maximum does not promise that every geometry/affine combination
fits this work budget. Full-extent vector rasterization uses the same work gate.
No substituted mask or silently reduced feather is returned on rejection.

Far-off-canvas path bounds do not determine allocation size. The 16M window limit
is not a total-process-memory guarantee: output tiles, bounded authored curve
pieces, scanline coverage and blur buffers have separate storage.

Raster, vector, and combined derived masks share one 32 MiB warm cache. Larger
live results are weakly reusable. Keys distinguish source identity, parameters,
affine, requested dimensions/origin, inversion, and empty state. Weak source
references prevent reused addresses from returning stale results. Rasterization,
feathering, and combination happen outside the cache mutex. Editable path
allocations are counted once across shared node/history references.

## Transform and conversion behavior

Linked components follow content transforms; unlinked components preserve their
world mapping. Image-wide crop/resize/rotate transforms every component while
retaining raw path anchors and mask data. Source-size replacement and Smart
cache/source conversions compensate the local origin without double-transforming
coverage.

Rasterize Vector Mask requires an empty raster-mask slot. It stores the complete
bounded intrinsic path coverage, compensates its origin, and retains the affine,
properties, enabled state, and link state. It rejects conversion above 30,000
pixels per intrinsic side or 16 Mi pixels in total rather than clipping geometry.
Rasterization fixes intrinsic pixel resolution: transformed unfeathered edge
antialiasing may change when the resulting pixels are resampled. Feather retains
the same intrinsic kernel and is not applied twice. The command is undoable.
Arbitrary projective/mesh vector-mask warp and destructive combination of two
independent components are explicitly unsupported.

## Persistence and interchange

Native live/history state containing vector masks requires version 11, including
disabled or empty descriptors. Raster property/independent-grid features alone
require version 10; default legacy documents continue writing version 9. Paths
share the existing lossless path-blob pool across content, masks, and history.
Opaque nested Smart-source archives retain their own independent native version
and exact bytes; a vector-free host need not advertise its child's version.
Opening an unsupported child version fails without modifying the host.

Merged ORA and pixel exports use combined appearance. PSD/PSB import/export
retains a bounded editable subset: empty masks and single certified-simple cubic
contours on supported raster/group layers, with explicit flags and coordinate
mapping. Compound paths, unproven fill operations, parameter layouts and other
unsupported features retain an explicitly labelled appearance fallback. See
[PSD vector-mask interchange](psd-vector-masks.md) for exact supported boundaries
and independent-reader proof limits. SVG export uses the rendered appearance
route. Unsupported editable interchange must diagnose the mask rather than
silently emit hidden base artwork.

## Verification

Focused regression suites include `vector_mask_tests`, `vector_geometry_tests`,
`composite_mask_cache::tests`, `native_vector_masks`, and
`vector_mask_export_guards`. Engine tests cover stale-bake invalidation and
independent masks with styles, clipping, and reload. GPU tests only count as
verified when an adapter ran; use `EMULSION_REQUIRE_GPU_TESTS=1` for that gate.
Native Pen/target/property/transform workflows and save/reopen require separate
serial UI/native validation. These contracts do not claim reference-application-identical
antialiasing, blur kernels, or complete feature parity.
