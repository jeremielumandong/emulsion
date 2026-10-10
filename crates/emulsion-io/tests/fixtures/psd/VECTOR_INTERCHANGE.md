# Independent editable vector-mask verification

These generated fixtures are self-authored test artwork. No image from ag-psd's
image-excluding license, or additional third-party PSD, is vendored here.
The separately documented CS4-era contributor fixture in `README.md`
remains an appearance-fallback regression, not evidence of editable parity.

Generate into a new or empty directory:

```sh
cargo run -p emulsion-io --example psd_vector_interchange -- /tmp/psd-vector-qa
PYTHONPATH=../dependency-source/psd-tools-1.23.0 \
  python3 scripts/verify_psd_vector_interchange.py /tmp/psd-vector-qa \
  --report /tmp/psd-vector-qa/verification.json
PYTHONPATH=../dependency-source/psd-tools-1.23.0 \
  python3 scripts/verify_psd_density_interchange.py \
  /tmp/psd-vector-qa/density-boundaries \
  --report /tmp/psd-vector-qa/density-boundaries/verification.json
```

The verifier needs Pillow, numpy and psd-tools 1.23.0 or later. Its report records
the reader version and actual imported module path. Schema 2 records the complete
original scene, original cases in admitted contexts, and a separate precision
control. Generation fails if any case's actual admission differs from its pinned
expectation or if independent masks are baked. The first verifier also runs the
unchanged density verifier; the second command emits its standalone report.

All original scenes and the density supplement are authored explicitly in
`PhotoshopSrgbV1`. The original fixture bodies, source colors, pixels, geometry,
flags and parameters remain unchanged. The canonical control is a separate,
explicitly disclosed geometry variant. Each export records and asserts its actual
`WriteReport`, including zero rounded density fields in every vector case and
eight in the boundary supplement.
The independent verifier checks the selected profile in the native archives;
the supplement also checks it in both history states. No third-party application
rendering claim follows from selecting this native profile.

## What is tested

- PSD and PSB, unequal 128 by 80 document axes, translated raster layers and a
  masked group, with exact hierarchy and sibling order.
- All thirteen original vector descriptors remain in the native source inventory.
  Twelve are proved editable in admitted contexts; a separately authored
  canonical-precision control proves the off-canvas cubic's standard record
  geometry while the unchanged original cubic is an exact fallback case.
  Coverage includes a golden rectangle, negative and off-canvas cubic
  anchors/handles, mixed linked/corner knots, an open contour
  whose fill closes implicitly, affine-baked geometry, independent inversion,
  disabled and unlinked states, empty reveal/hide and inverted-empty coverage,
  and separate density, feather, and combined raster/vector cases.
- A minimal Python `struct` record walker reads file bytes without either PSD
  library. It checks exact mask-header bytes, `vmsk`/`vsms` version/flags, 26-byte
  path records, subpath length, explicit Combine=1, EvenOdd=1, knot selectors,
  8.24 coordinates in y/x order, and reserved/padding bytes. A separately
  specified golden rectangle packet uses exact 1/8 and 3/8 coordinates and is
  independent of the generated manifest's encoder.
- psd-tools separately decodes the path geometry and checks source RGBA channels,
  independent raw `-2` raster pixels and outside fill, short parameter headers,
  and independent raster versus vector density/feather fields. There must be no
  synthetic raster carrier and no combined `-3` channel.
- Every positive packet's descriptor is freshly rasterized by
  `psd_tools.composite.vector.draw_vector_mask`, including its off-canvas viewport.
  The native component PNG is a comparison target, never the input to that
  rasterization. Inversion, density and disable are applied explicitly because
  the function itself does not implement those properties. Raster/vector
  multiplication is compared separately, with feather set to zero on both sides.
- Each native rasterization uses 4 by 4 subpixel samples, while psd-tools uses
  area coverage. The verifier therefore checks all interior pixels within one
  byte, and reports the independently derived one-pixel boundary band separately
  (maximum 64 bytes and mean 16 bytes). A large interior failure cannot be hidden
  by a whole-image mean. Integer-aligned and empty cases have exact interiors.
- Native source immutability is checked both by Rust document equality after
  each export and by byte-for-byte equality of every ZIP member in before/after
  native archives, including geometry resources and original raster pixels.

## Schema 2 case contract

The root manifest requires exactly fifteen case directories, each containing
both PSD and PSB, actual write reports, component references, exact native CPU
reopen references, and before/after native archives:

- `original-complete-scene`: the original fifteen-node scene with all thirteen
  masks. The masked PassThrough group above preceding roots is deliberately
  checked as `UnsupportedFeatures` appearance fallback.
- `original-root-02` through `original-root-13`: each unchanged original raster
  root over the unchanged opaque backdrop. Eleven must retain editable layers;
  `original-root-03` retains the authored off-canvas cubic and must report
  `BlendSpaceDifference` appearance fallback.
- `original-root-14`: the unchanged group and child alone, where the masked
  PassThrough group is admitted. Its transparent native appearance remains
  separate from the saved merged image's white-matte storage representation.
- `canonical-cubic-control`: the cubic's exact expected 8.24 reconstruction,
  derived into a clone while preserving all other node fields and raster sources.
  It must retain editable layers and reopen with the exact control descriptor
  and native appearance. It does not claim the original cubic appearance is
  editable.

The verifier binds every extracted node's complete native descriptor, resource
bytes, relative order, canvas and profile to the complete source archive. Only
the control's path bytes and vector transform may differ, and its exact
reconstructed world coordinates are independently checked against the original.
The original and control must produce identical expected signed 8.24 words and
full path records. Nine Y fields differ by at most 0.0000019073486328125 document
pixels; the actual coverage and native RGB differences are reported separately.
The references must expose a nonzero difference. No geometry adjustment is
applied to the original fixture or to production admission.

Both original fallback scenes must contain exactly one explicitly named
appearance layer, no editable vector blocks, and exact original pixels in its
source, saved merged preview and reopened native rendering. Their complete
native source archives retain all original editable geometry and source pixels.
A fallback's saved pixels are never counted as fresh editable-path evidence.

The transparent group preview has an explicit, bounded storage check. The raw
header must declare RGB8 with four channels and a negative layer count marking
merged transparency. Alpha is exact. For fractional alpha the expected stored
RGB uses the encoder's separate f64 operations `a=A/255`, `r=255*(1-a)`, then
truncates `C*a+r` to a byte. Alpha-zero and alpha-255 RGB bytes are untouched.
Every decompressed plane must match exactly. This proves white-matted storage;
exact native layered reopen appearance is reported separately. No alpha
tolerance or decoded straight-color equality is substituted for this check.
The existing opaque schema-1 and density preview checks remain unchanged.

Existing schema-1 packets retain their original thirteen-editable-descriptor
contract. New schema-2 orchestration does not reinterpret a schema-1 failure,
remove a source case, change golden bytes, or relax raw/shape tolerances.

## Separate density-boundary packet

The same generator also writes `density-boundaries/`. This supplement does not
replace any of the thirteen descriptors or relax their coverage assertions.
Run both verifiers on a freshly generated packet. Older generated manifests
without exact `density_f32_bits` must be regenerated.

Its six named cases contain two raster-only layers, independently crossed
raster/vector densities on two carried-vector layers, an exact 0.5 tie on both
fields, and an already representable byte-grid control on both fields. The exact
IEEE-754 inputs `0x3f666666` (`0.9f32`) and its successor `0x3f666667` straddle the
229.5-byte boundary. Widening the stored f32 to f64 before multiplying by 255
gives bytes 229 and 230 respectively. Multiplication in f32 would invent a 229.5
tie for the lower input and incorrectly emit 230. Both PSD and PSB must preserve
the separately serialized raster and vector fields, original source RGBA, raw
independent `-2` samples and editable vector records.

The Python oracle starts with the pinned native f32 bits, reconstructs their
exact value and uses nonnegative half-away rounding, independently of the Rust
helper and manifest byte. It also derives the represented f32 and compares the
number of changed fields with the actual `write_with_report` result: eight
rounded fields, no rounding of the byte-grid control, no fallback and no baking.
The generator records the real report returned by each export, not a prediction.

Native preservation has two different history states, before the densities
were authored and after. Before/after native ZIP members must be identical.
The independent verifier additionally reads the live and both historical f32
values, exact editable path resources, full RGBA16 source words and raw raster
mask samples. The archived history must bind the live document and retain the
two-commit edge. Rust separately asserts document/source identities and history
metadata/document equality after each export.

Component references and the saved merged preview are rendered from the
represented byte-grid document. The unrounded native preview is retained and
must differ, so a stale native preview cannot silently satisfy the check. Fresh
vector geometry remains a separate psd-tools rasterization, with the original
interior and edge-band tolerances unchanged.

Mandatory negative controls run for both PSD and PSB, separately for raster and
carried-vector density. One changes a copy of the manifest byte to the incorrect
f32-multiply result. Another changes byte 229 to 230 in a copy of the actual raw
mask header and passes it through the same raw-header validator against the
original independently decoded layer. Both must fail for the intended density
assertion. These are in-memory mutations; packet files are never modified. The
JSON report records each rejection. The density verifier creates its report
exclusively and will not overwrite an existing report.

These are reproducible verification tools, not a claim that a particular source
revision or generated packet has passed. Record generator/verifier executions
and their results separately when validating a delivered batch.

## Deliberately separate claims

Feather is checked as exact stored metadata. The native feathered reference is
also verified to change pixels in the feather cases, but no comparison between
different feather kernels is claimed. The PSD/PSB saved merged pixels are checked
against the native preview in a separate report field. Saved-preview equality
is not fresh compositing and is never accepted as vector-geometry proof.

This suite establishes the bounded editable interchange subset with an independent
reader. It does not claim a present-day third-party application open/edit/save test,
third-party rendering parity, general Boolean contour support, or editable fallback for
unrecognized path metadata. Core tests separately exercise unsupported compound,
self-intersecting, unknown-operation/record, out-of-range coordinate, long mask
parameter, derived-raster (`from_vector_data`), and `-3` layouts.
