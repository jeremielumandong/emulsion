# Editable vector masks in PSD and PSB

PSD/PSB interchange preserves editable vector geometry for a deliberately bounded
subset. Native projects remain the source of complete geometry, original affine
state, history and unsupported mask features.

## Editable subset

- Integer-translated raster layers, groups and the separately guarded source-only
  embedded PNG Smart Object subset can retain an independent vector mask alongside
  an independent raster `-2` mask channel. Smart placement uses the original source
  transform, never the cropped preview rectangle.
- A vector descriptor may be empty, with explicit Reveal All/Hide All coverage,
  or contain one simple cubic Bézier contour. Closed contours and open contours
  with implicit straight fill closure keep their authored open/closed state.
- Anchors, incoming/outgoing handles and linked/smooth knot state are retained.
  Layer link, disable and inversion flags remain separate from knot linkage.
  Initial fill controls the empty state; it does not replace a nonempty Combine
  contour with a full-coverage mask.
- Coordinates use the document dimensions independently on each axis, not the
  layer preview bounds. Import keeps document-space anchors with an explicit
  translation to layer-local coordinates. Export maps the native mask affine
  and layer placement into document-space points without changing the native
  source. Original raster pixels are not clipped to the path.
- Adobe's path range is normalized `[-16,16)`, despite the wider signed i32
  storage. Points are rounded to 8.24 precision, at most half a grid step per
  axis. Output is rejected from the editable route if rounding reaches 16 or if
  the resulting contour cannot be certified. No saturation or canvas clipping
  is used to manufacture a representable path.

The current Rust ag-psd dependency uses unit dimensions for path read/write.
Its exposed knot coordinates are therefore already normalized. Multiplying or
dividing by layer dimensions, or assuming its Rust API matches the TypeScript
pixel-coordinate API, produces incorrect paths.

Its bidirectional key-alias table also routes `vmsk` to an unregistered handler.
The importer therefore makes an in-memory decoding copy and changes only
structurally located layer `vmsk` keys to the equivalent `vsms` key that the
dependency actually reads. Payloads, resources and original file bytes remain
untouched. Original-byte guards and saved-composite validation run independently
of that copy. The writer continues to emit standard PSD/PSB keys.

## Fill and topology

Adobe documents even-odd path filling; native vector masks use nonzero winding.
The dependency's `NonZero` subpath field is not independent evidence of matching
Photoshop rendering. Multiple Combine components are a union, which also does
not implement arbitrary native global nonzero geometry.

The editable route writes one explicit Combine component with an even-odd fill
field. It accepts either recognized fill field on import only when a bounded
simplicity check establishes equivalence. Each cubic is split into coordinate-
monotone pieces; pairs of control hulls must be separated, except for a single
shared authored endpoint between neighbors. The implicit closing segment is
included. Per-curve subdivision is capped at eight levels across every stage,
keeping subdivision and endpoint comparisons exact for 8.24 inputs in the
supported range; separating projections use a conservative numerical margin.
There are additional piece/work limits. Inconclusive contact, complexity or
numerics use appearance fallback, not an approximate Boolean reconstruction.

Compound, self-crossing, subtract/intersect/exclude, standalone continuation
operation `-1`, degenerate one-knot and other unproven paths remain unsupported
for editable interchange. Empty descriptors are distinct from one-knot paths.
This is a conservative interchange certificate, not a new path Boolean engine.

## Parameters and independent masks

Density and Feather remain standard vector parameters only when there is an
independent raster mask to carry the shared PSD parameter header, and that
header remains shorter than 36 bytes. Density is quantized to the standard byte.
The layered merged preview and exact appearance/profile checks use that same
nearest-byte Density state. Export reports each rounded raster/vector Density
value, including hidden/disabled ones, without changing native source or history.
Byte-grid values are idempotent. No other parameter, source pixel or geometry
receives this exception, and unsupported headers are rejected before rounding
can remove a parameter. Whole-appearance fallback keeps the original native
rendering. See [raster-mask representation rules](psd-raster-masks.md).
Native feather is intrinsic-grid based; its editable route permits only integer
mask translations, so moving fractional coverage through the blur is not silently
reordered. Unfeathered paths can absorb any supported finite affine into points.

Two dependency constraints remain explicit:

1. A parameter header without a raster bitmap makes ag-psd synthesize an empty
   `-2` channel. The writer does not manufacture an extra raster mask solely to
   carry vector properties.
2. ag-psd misinterprets headers of 36 bytes or more as real-mask metadata. A
   simultaneous raster/vector Feather combination can reach that size and uses
   appearance fallback until a verified parameter decoder is available.

A rendered-from-vector `-2` channel is a cached appearance, not an independent
raster mask. Such input, real/combined `-3` channels and ambiguous long headers
retain the existing saved-appearance route. They are never multiplied into the
editable path a second time. PSD vector fill/stroke shape layers, effects,
unsupported clipping and filter records also retain explicit appearance
fallback. Disabled features are still checked, because their state must remain
meaningful when re-enabled.

## Strict input and loss reporting

A length-delimited original-byte guard validates versions, flags, selectors,
declared counts, knot closure, reserved state and supported coordinate bounds
before the permissive dependency can discard evidence. Only exact optional
zero alignment is accepted after 26-byte path records. Truncation and mismatched
knot counts are errors; structurally valid but unsupported features can use a
validated genuine saved composite. Missing/fake composites do not produce
invented artwork. See [raster-mask recovery](psd-raster-masks.md).

Unsupported exports use an explicitly named flattened appearance and the UI's
existing warning to keep the native project. The actual export job returns a
`WriteReport`; warning decisions are not inferred from a separate foreground
render. A native Linear-blend document additionally compares its current 8-bit
pixels with the importer's sRGB convention where a blend kernel can differ.
A visible difference uses native appearance fallback; an identical current
result keeps supported layers. This does not assert equivalent future edits or
Photoshop's application-wide gamma behavior.

## Verification and limits

- Unit tests cover raw golden packets, PSD/PSB, negative/off-canvas coordinates,
  independent pixels, flags, empty/open geometry, parameter boundaries,
  transformed geometry, malformed input and explicit fallback.
- Imported paths remain native-editable with Undo/Redo and native save/reopen.
  Export does not mutate source pixels, descriptors or native history.
- `psd_vector_interchange` generates schema-2 self-authored evidence packets
  while retaining the original thirteen-mask fixture verbatim. Twelve original
  masks are checked in admitted contexts; the complete original scene and its
  unchanged off-canvas cubic are explicit, exact appearance-fallback cases.
  A separately labeled canonical-8.24 cubic control proves the expected format
  geometry, with its changed native coverage and appearance disclosed.
  `scripts/verify_psd_vector_interchange.py` independently parses positive path
  packets and checks fresh psd-tools coverage separately from saved composites.
  Every case is bound to the original native descriptors/resources, and both
  PSD/PSB actual write reports and exact native reopen appearance are checked.
  The transparent group-only saved preview uses an exact white-matte storage
  check with genuine alpha metadata; existing opaque checks remain strict.
  Flags/properties are asserted independently; matching Feather metadata is not
  a matching-kernel claim. Existing schema-1 verification remains supported.
- Its separate `density-boundaries/` packet and
  `scripts/verify_psd_density_interchange.py` retain the original thirteen-case
  coverage while checking the adjacent native f32 values around the 229/230
  density boundary, raster-only and independently carried vector fields, exact
  native source/history preservation, and actual export rounding counts. The
  Python oracle derives expectations from pinned f32 bits, and rejects both an
  incorrect f32-multiply manifest byte and mutated raw density bytes. See the
  [generation and verification instructions](../../crates/emulsion-io/tests/fixtures/psd/VECTOR_INTERCHANGE.md).
- Third-party fixture provenance and exact proof limits are recorded in
  `crates/emulsion-io/tests/fixtures/psd/`. A Photoshop-authored fallback fixture
  does not prove editable import of every Photoshop path layout.

No actual Photoshop application session or Photoshop-identical antialiasing,
feather, Boolean topology, Smart Filter masks or full editable PSD parity is
claimed by these tests. Native-window acceptance and a full final source test
run are separate checks recorded for each delivered batch.

Primary format reference: [Adobe Photoshop file format](https://www.adobe.com/devnet-apps/photoshop/fileformatashtml/).
Independent parser/rasterizer: [psd-tools paths](https://github.com/psd-tools/psd-tools/blob/main/src/psd_tools/psd/vector.py)
and [vector compositor](https://github.com/psd-tools/psd-tools/blob/main/src/psd_tools/composite/vector.py).
