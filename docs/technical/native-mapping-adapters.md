# Preparatory native mapping adapters

`emulsion-io/src/mapping_data.rs` is an isolated source artifact for the
coordinated Smart model and native/history cutover. Its module registration is
**test-only**. It does not enable projective native files, alter any archive
schema, change required/maximum versions, change resource loading, or relax
feature/retention admission. Production integration remains pending.

The adapters use the existing runtime `SmartPlacement`, `Mapping2` and checked
`Projective2`; no serde implementation is added to those types. Adapter fields
are private, and all runtime-to-wire construction uses named checked methods.
There are no dead-code exemptions or new public runtime APIs.

## API and role boundary

| Wire adapter | Construct for writing | Convert after reading |
| --- | --- | --- |
| Smart placement | `PlacementData::from_smart(SmartPlacement)` | `into_smart() -> SmartPlacement` |
| Raster placement | `PlacementData::from_raster(Placement)` | `into_raster() -> Result<Placement>` |
| Raster component | `MappingData::from_raster_mask(Mapping2, ComponentOwner)` | `into_raster_mask(ComponentOwner) -> Result<Mapping2>` |
| Smart Filter component | `MappingData::from_filter_mask(Mapping2)` | `into_filter_mask() -> Result<Mapping2>` |
| Vector-mask component | `MappingData::from_vector_mask([f64; 6])` | `into_vector_mask() -> Result<[f64; 6]>` |

Every `from_*` method returns `Result`. `ComponentOwner` is `Smart` or `Other`;
it describes the actual node kind, not the presence/enabled state of a plane.
Raster placement and vector-mask conversions refuse every projective variant,
including identity and affine-valued matrices. Raster component conversions
permit projective metadata only on Smart owners, including latent mappings with
no plane. The filter methods are for the descriptor inside a Smart kind.

The shared `MappingData` parser can recognize both shapes before its enclosing
kind has been decoded. Parsing applies the legacy raster-component predicate to
affines and `Projective2` representation admission to projective matrices. The
caller **must** use the correct named conversion before resource loading or
candidate publication. Filter/vector conversions add their stronger legacy
affine predicate. A successful generic parse is not role admission. Placement
parsing similarly does not make a projective object usable as Raster placement.

These adapters do not certify resource dimensions, pixel/cache support, mask-grid
sampling, the coexistence of a vector mask and any projective metadata, or a
schema position/version. Those checks require the enclosing reader and the
coordinated model support validator. In particular, a relative projective mask
is not required to have finite forward bounds across its entire intrinsic
rectangle. No rectangle or sampling operation is performed by these adapters.

## Exact wire shapes

- Legacy placement is the existing object in writer order: `x`, `y`, `scale_x`,
  `scale_y`, `rotation`, `flip_x`, `flip_y`. All seven fields are required.
- An affine component is the existing six-number array in column order
  `(a, d, b, e, c, f)`. It is not row-major and gets no new wrapper/tag.
- A projective placement/component is exclusively
  `{"projective":[a,b,c,d,e,f,g,h,i]}`, nine row-major numbers.

Direct serde visitors reject unknown/duplicate/conflicting keys, including
escaped spellings of duplicates, before any fallback. There is no untagged enum,
intermediate `serde_json::Value`, approximate affine detection, or missing-value
fallback in the implementation. Lengths, scalar types, nulls, nonfinite values
and checked-constructor matrix failures are rejected. A legacy placement array
is rejected: the native contract is the seven-field object, even though generic
derived Rust struct serde may accept sequences in other contexts.

`PlacementData` deliberately has no `Default`: `Placement::default()` is an
explicit runtime construction choice, not the current placement missing-field
policy. `MappingData::default()` is affine identity solely for an enclosing
optional/defaulted raster-component field. An absent enclosing field can retain
the old identity default; a present null, empty object or short array cannot.
An enclosing required filter/vector transform must remain required. Unknown
unrelated document fields retain their existing schema policies.

## Legacy numeric compatibility

These are copied from the existing role predicates, not generalized through
projective operation admission:

- Placement: finite `x`, `y`, `scale_x`, `scale_y`, `rotation`, with
  `abs(scale_x) >= 1e-6` and `abs(scale_y) >= 1e-6`. Negative scales and all finite
  rotations/translations remain allowed. No `to_doc` evaluation is added.
- Raster component: finite stored coefficients and rejection when
  `determinant.abs() < 1e-12`, exactly as in `Document::validate`. This inherited
  predicate can accept a computed infinite/NaN determinant produced by huge
  finite coefficients; this preparatory change deliberately does not strengthen
  it. Such acceptance is legacy retention compatibility, not numerical safety
  for a new operation.
- Filter/vector component: finite stored coefficients, finite determinant with
  `abs(det) >= 1e-12`, finite inverse coefficients, and finite forward/inverse
  evaluations at all four corners of the existing `+/-1e9` envelope. This matches
  `FilterMaskData::validate` and `VectorMask::valid`'s transform predicate.

No legacy coefficient is normalized or round-tripped through `Projective2`.
Legacy placement serialization delegates to the original `Placement` writer;
affine component serialization delegates to the original `[f64; 6]` writer.
Both preserve coefficient/field order, float formatting and signed-zero bits.
The deliberate strictness change is limited to recognized mapping object
shapes; it must not become global unknown-field denial on document structures.

## Projective canonical bits

Reading calls `Projective2::from_row_major`; writing calls `to_row_major`. The
runtime API picks the first largest-magnitude coefficient as pivot, including
negative/tied pivots, and normalizes signed zeros. It does not divide by the
bottom-right coefficient. A valid matrix with `i == 0` is supported. Its variant
is retained even when `to_affine()` would succeed.

The workspace already enables `serde_json`'s `float_roundtrip` feature for native
geometry/keyframes. This change does not alter Cargo features or implement a
second number parser. Authored fixtures assert stable writer bytes and exact
coefficient bits over repeated serialization/deserialization for nontrivial
decimal coefficients, adjacent mantissas, inverse/composed matrices, negative
and tied pivots, tiny perspective terms and a zero bottom-right coefficient.
This is stronger than float `PartialEq` or checking only an integer identity.

## Integration duties and validation status

Before production registration, the coordinator must migrate the actual
native/history fields and their named role conversions, while retaining missing
field rules. It must coordinate `native_admission`'s schema-position/owner probe,
`native_features`' archive-wide version/preservation handling and lexical
fallback, maximum/required v16 selection by marker presence, full support
preflight, retained-history relation checking, and resource-order guarantees.
The current admission still rejects every projective marker, including escaped
ones; it has not been modified here. Native/history/project resources and outer
envelope policy are unchanged. No PSD parser or interchange work is included.

The focused source tests compare placement/raster admission against actual
`Document::validate`, filter admission against `FilterMaskData::validate`, and
vector admission against `VectorMask::valid`. Additional fixtures cover exact
legacy writer bytes/bits, defaults/missing values, adversarial shapes/duplicates,
nonfinite deserializers, role restrictions, marker retention and projective
canonical idempotence. Tests must be maintained against those original oracles
during the coordinated field migration rather than weakening expected behavior.

Standalone rustfmt and `git diff --check` are the only checks performed for this
source artifact. No Cargo, build, Rust test, runtime or GUI execution was run.
The coordinator owns focused test execution and the repository validation gates;
these fixtures are not evidence of a passing build or active native v16 support.
