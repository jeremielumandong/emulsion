# Staged Smart and component mapping primitives

`emulsion_core::mapping` provides runtime geometry types for the next native
Smart placement migration. It is not wired into Node fields, commands, masks,
rendering, IO, history or UI. It does not enable Smart Distort. There is no
serialization, secondary active placement, Deref or default-affine fallback.

## Representation and admission

- `SmartPlacement::Legacy(Placement)` retains the exact seven fields.
  `source_to_document((width, height))` evaluates the existing `Placement::to_doc`
  with **source** dimensions. Rotation centers and flips depend on these sizes.
- `SmartPlacement::Projective(Projective2)` retains a checked, source-pixel to
  document map independent of source and filter-cache dimensions.
- `Mapping2::Affine(DAffine2)` retains the six original columns `(a,d,b,e,c,f)`;
  `Mapping2::Projective(Projective2)` retains the checked matrix. Equality is
  exact representation equality, never an epsilon-based equivalence test.
- `from_affine`, `from_affine_columns`, `validate_representation` and affine
  `try_affine` require only finite coefficients. They deliberately do not
  replace existing native compatibility validation, certify nonsingularity or
  apply projective precision limits to an old file. Public affine variants can
  bypass constructors, so fallible operations recheck their inputs.
- `validate_for_operation` and explicit `to_projective` use existing
  `Projective2` validation. A retained legacy representation can fail this
  stronger operation admission. Nontrivial checked affine/mixed compositions,
  inversion and domain evaluation can also return an explicit error. Never
  apply these stronger gates wholesale when decoding legacy documents.
- Representation validity, finite geometric bounds and uniform numerical
  sampling admission are separate. `map_rect` checks corners and conservative
  bounds; its projective branch also excludes a horizon throughout the chosen
  rectangle. Success **does not guarantee that every interior `map_point`
  succeeds**. `bounds().to_irect()` separately checks integer representability.
  None of these provides a uniform numerical-domain certificate, admits a
  sampler or approves an allocation. Candidate admission must still enforce
  source/cache sizes, 30,000-side/400-MP limits, rendering support and bounded
  work. `source_rect` only checks positive geometry.

## Operations and exact legacy arithmetic

`left.compose(right)` means `left * right`, applying right first. Two affine
operands return the existing `DAffine2` product; inversion and point/corner
evaluation also use the original affine arithmetic. Checks never replace these
results with normalized/projective answers. New checked affine point evaluation
rejects overflow, underflow to zero and severe cancellation using a conservative
error budget of `1e-9 + 64*EPSILON*abs(result)` per coordinate. Its rectangle
bounds include rounding allowances and can be larger than old corner AABBs.
Existing affine render/bounds producers remain untouched.

Corner checks alone cannot establish the relative error budget throughout a
rectangle. For example, `x' = 2^20*x - 2^21, y' = y` on `[0,4] x [0,4]` passes
`map_rect`, but `map_point((2,2))` returns `PrecisionLoss`: cancellation makes
the interior x result zero, with a stricter absolute budget than the corners.
The regression preserves this explicit refusal; no tolerance or affine
arithmetic is changed. Uniform numerical admission remains a separate consumer
requirement. The retained-pixel renderer has its own checked sampling-domain
descriptor; these core bounds are not a substitute for that admission.

Mixed nonidentity composition stays projective, even if the result is exactly
affine. `try_affine` only accepts exactly zero perspective coefficients and does
not demote stored state. Canonical affine promotion may round coefficients, so
it is explicit and must not rewrite a legacy representation.

Exact identity composition retains the other operand; if both are identities,
the right-hand baseline wins. Zero cache offsets retain the receiver exactly.
An identity `left_compose_projective` returns the original Smart placement
before evaluating new geometry, including legacy fields outside new numerical
admission. This is no-op retention, not complete candidate validation.
Nonidentity projective deltas compute `Hnew = delta * Hold` and promote Legacy.
Tiny nonzero deltas are not suppressed. Subsequent **legacy affine editing**
must continue using its existing Placement operations; this helper is not a
replacement decomposition/reconstruction path. Projected content can explicitly
promote a document-space affine delta and left-compose it onto H.

## Source, cache and mask bases

Let H map source pixels to document pixels, C map intrinsic mask pixels to source
pixels, and o be the filter-cache origin in source coordinates:

- `H.with_source_offset(o)` is `H * Translate(o)`, cache to document.
- `C.in_cache(o)` is `Translate(-o) * C`, intrinsic mask to cache.
- `mask_to_document(H, C)` is `H * C`, with no cache offset.
- `preserve_mask_world(Hold, Hnew, C)` computes
  `inverse(Hnew) * (Hold * C)`, retaining existing affine world-first grouping.
  If the source mappings are exactly unchanged, C is returned unchanged.

The generalized cache helper is **not** permission to replace the existing
legacy `smart::cache_placement` rendering arithmetic. Integer offsets convert to
f64 before negation, including `i32::MIN`.

Only a content-only change compensates unlinked raster/filter masks. Linked
masks retain C exactly. Whole-document coordinate changes retain C regardless
of link state. Disabled/dormant descriptors obey the same geometry contracts;
these helpers neither inspect flags nor touch planes, filters, caches or source
provenance. The guarantee concerns world geometry, not byte-identical results
from the existing two-stage coverage interpolation.

A relative C may cross a horizon over intrinsic detail even when H*C is finite.
Do not reject it merely for that reason or clip stored detail. Validate the
relevant composed world/sampling domain. Undefined inverse samples must later
be handled safely by the mask sampler; painting/padding needs separate finite
frame and allocation checks. These primitives return errors, never mask fill,
NaN indices or an identity fallback.

## Verification and integration gate

Focused tests are in `crates/emulsion-core/src/mapping_tests.rs`. They cover
bitwise affine retention/arithmetic, explicit operation admission, exact no-ops,
source sizes, cache offsets, repeated left-composition, mask-world invariance,
relative horizons, expanded-cache refusal and extreme numerical inputs.
The new Rust files were formatted directly with the existing official rustfmt.
Tests were authored without running Cargo/builds/tests under this task's
execution restriction. The coordinator must run focused tests and repository
checks in `CONTRIBUTING.md` before integration claims.

Model/lifecycle cutover and strict v16 native/history adapters remain gated on
renderer execution and API review. The future central support validator must
not treat `map_rect` or `bounds` success as blanket numerical/sampling admission;
each consuming operation needs its own uniform certificate or explicit checked
pointwise failure contract. Vector-mask generalization is not included.
