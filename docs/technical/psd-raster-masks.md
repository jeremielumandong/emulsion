# PSD raster-mask interchange

Emulsion's PSD/PSB adapter preserves a bounded editable raster-mask subset.
This is not a claim of full Photoshop compatibility or Photoshop-rendered
equivalence. Native projects remain the source format for complete editability.
PSD export remains 8-bit RGB, and the existing import adapter converts channel
samples to 8-bit. This batch does not add lossless 16/32-bit mask interchange.
PSD import also retains its existing sRGB blending convention; it does not
preserve a native linear-light blending setting as an editable PSD parameter.

## Editable subset

For ordinary raster layers with whole-pixel translation and for groups, a mask
whose affine is an integer translation is written as its original `-2` pixel
channel and independent document-space rectangle. The mask can extend beyond
the content layer or the document; import no longer crops those pixels away.
Its enabled state, linked state, black/white outside fill, Density and Feather
are retained. Disabled masks and density-zero masks keep their original pixels
and parameters so they can be edited or enabled later.

Density is serialized as the standard 0–255 byte, with nearest-byte rounding.
Feather is the standard double-precision parameter on disk and becomes the
native single-precision value on import. The supported native feather range is
0–1000 pixels. Pixel grids are unchanged, so feather units are not silently
rescaled. Native feather uses Emulsion's existing three-box approximation;
matching numeric values do not guarantee Photoshop-identical softened edges.
The layered PSD merged preview uses the same nearest-byte Density values as
the emitted editable masks. Its appearance may therefore differ from the native
composite by that documented representation change. Export reports the number
of independent raster/vector Density values rounded, including disabled or
hidden parameters; native pixels, mask state and history are not changed.
Byte-grid Density values are idempotent and do not count as rounded.

This exception applies only to independently serialized Density parameters.
Source pixels, layer Opacity/Fill and geometry still undergo exact emitted-input
appearance and profile checks, with no tolerance. Baked mask affines retain
their original native coverage rather than rounding Density first. Unsupported
state is checked before rounding, and any whole-appearance fallback uses the
original native rendering and reports zero rounded Density parameters.

The historical mask flag named `position_relative_to_layer` is interpreted as
unlinked when set, while mask rectangles remain in document coordinates. This
follows [GIMP's documented interoperability correction][gimp-link], rather than
the misleading flag name. Vector-mask link flags are a separate record.

## Conservative fallbacks

- Fractional translation, rotation, scaling and shear of a raster mask use
  baked coverage, with default PSD mask properties to avoid applying Density or
  Feather twice. The export status warns that those mask settings were baked.
- Transformed raster content and non-raster content retain their existing
  rendered-layer route. This does not create Photoshop Smart Objects or retain
  an editable content transform.
- Gray *raw* outside fill uses a named whole-appearance layer. Adobe specifies
  black/white background values, and readers disagree on intermediate bytes.
  A black raw fill with reduced Density is supported through the standard
  Density field; it is not mistaken for an unsupported gray raw fill.
- Baked mask channels cover the complete exported layer/document grid and use
  a binary outside fill. Original off-grid pixels remain in the native source,
  not in this appearance-only mask.
- Vector masks, Smart Filter stack masks, adjustment layers, layer effects and
  unsupported clipping relationships continue using their named appearance
  fallback. There is no editable vector/Smart Filter PSD round-trip claim.

Export is read-only with respect to the source document, masks, transforms,
filters and undo history. Import creates a new document; it does not modify the
input PSD. Saving the imported editable subset to a native archive preserves
its independent mask state through the existing native format/version rules.

## Parser boundary

The adapter uses ag-psd 0.3.0. Its raster mask fields are implemented, but the
reader's real-mask-header length heuristic is ambiguous for a mask containing
both raster and vector feather parameters without a `-3` channel. The older
inverted-raster-mask flag is also not retained by that dependency.

The mask guard scans ordinary and document-level high-depth layer records and
distinguishes these layouts from supported editable records.
Where safe, it decodes the file's existing merged composite and explicitly names
the result as flattened appearance. Raw/RLE completeness is checked even when
the dependency reports success, so
underfilled rows cannot become invented black pixels. The original merged-alpha
metadata is retained when the decoder succeeds. The guard never rewrites the
source file. Recovery after a layer-decoder failure is limited to 8-bit RGB
without additional channels or 8-bit grayscale without additional channels;
other layouts are rejected rather than losing merged-alpha semantics. A file
that declares it has no real merged image cannot use this recovery route.
Additional composite planes without an actual merged-alpha marker are rejected
rather than mistaken for transparency.

## Verification categories

Keep these evidence types distinct:

1. Synthetic byte-layout and malformed/truncated-record tests.
2. Emulsion PSD/PSB export/reopen tests for pixels, properties, independent
   bounds, group masks, link/enable state and density quantization.
3. Native save/reopen and mask-edit Undo/Redo tests after import.
4. Independently parsed Emulsion exports and externally authored PSD input.
5. Actual Photoshop open/edit/save and rendered comparisons.

The first four can be exercised without Photoshop. They do not establish the
fifth. The external `mask-parameters-no-real-channel.psd` fixture from psd-tools
has [reported Photoshop CS4 authorship and explicit test-suite permission][fixture].
It is useful for parser/fallback validation, not as proof of editable vector
mask support. The fixture license and source provenance accompany the fixture.

## Sources and deferred work

- [Adobe PSD/PSB file-format specification][adobe]: layer-mask channels,
  rectangles, flags and parameter fields.
- [ag-psd 0.3.0 source][ag-psd]: exact dependency version used by Emulsion.
- [psd-tools mask parser][psd-tools]: independent channel-aware real-mask parsing.
- [Known mask-layout issue #693][issue]: real-world parameter-layout example.

Editable vector interchange needs operation/fill-rule and fixed-point-coordinate
validation against external fixtures. Editable Smart Filter interchange requires
the complete Smart Object/filter descriptor graph, source and channel mapping;
`FMsk` display metadata alone is insufficient. The current dependency's filter-FX
subtree is incomplete. Neither gap is papered over with private or invented PSD
records.

[adobe]: https://www.adobe.com/devnet-apps/photoshop/fileformatashtml/
[gimp-link]: https://mail.gnome.org/archives/commits-list/2017-September/msg02005.html
[ag-psd]: https://github.com/Vasyanator/ag-psd-rs/tree/ff8754fb5dfb3b91f4e28dd37df98f6404f1dbcc
[psd-tools]: https://github.com/psd-tools/psd-tools/blob/main/src/psd_tools/psd/layer_and_mask.py
[issue]: https://github.com/psd-tools/psd-tools/issues/693
[fixture]: https://github.com/psd-tools/psd-tools/issues/693#issuecomment-5874881734
