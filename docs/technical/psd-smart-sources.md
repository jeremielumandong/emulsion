# Source-only embedded PNG Smart Objects in PSD/PSB

Emulsion supports a bounded **source-only** Smart Object interchange route in
PSD and PSB. This preserves an embedded PNG source and its placed-layer identity;
it does not implement editable PSD Smart Filters or establish current
reference-application/rendering parity. Keep the native project for full editability.

## Admitted source and placement

- RGB8 PSD documents, with no document ICC profile or an explicitly
  recognized sRGB profile. The embedded source must pass the strict,
  noninterlaced RGB8/RGBA8 PNG codec described in
  [native original images](native-original-images.md#bounded-png-admission).
- One raster source per placed instance, at its original dimensions with a
  whole-pixel translation. Negative/off-canvas placement is allowed within the
  signed record bounds. Scaling, rotation, flips, perspective, non-affine
  transforms, warps, animation, and nondefault layer-comp selection are outside
  this route.
- An empty native filter stack and filter-style list, no Smart Filter mask,
  zero cache offset, and exact native source/cache pixel equality on export.
  A disabled or dormant filter-mask descriptor still excludes editable export.

The placed descriptor's `Sz  ` and `Trnf` define source dimensions and placement.
Layer channels are a possibly trimmed/unrelated preview, never a substitute for
the source. The MIT `smartobject-layer.psd` fixture exercises a full 32×32 source
with smaller preview bounds.

### Source and instance IDs

`Idnt` associates a placed layer with the linked-source record UUID. `placed`
is the separate instance UUID. The adapter checks both associations, dimensions,
and duplicate IDs before constructing a native Smart layer.

Two PSD instances referring to one source UUID remain unsupported: that
relationship means shared future source edits, while native raster-backed source
editing is node-local. Independent native duplicates export distinct source and
instance UUIDv4s from operating-system entropy for each export, with bounded
collision checks. The IDs are captured in the export plan and checked against
the written descriptors. Entropy failure aborts without replacing the destination. Native deduplication of
identical PNG bytes does not turn those nodes into shared-edit instances.

## Structural records and strictness

The adapter supplements ag-psd 0.3's missing embedded-source byte handling; it
uses the dependency's descriptor and placed-layer encoding machinery.

- Document `lnk2`, `lnkD`, and `lnk3` blocks admit bounded `liFD` embedded records,
  versions 1–7, only with the understood optional fields and PNG payload.
- Layer `SoLd`/`SoLE` records admit `soLD` versions 4/5 and a whitelist of
  source-only descriptor fields. An accompanying legacy `PlLd`/`plcL` version-3
  record must agree with the modern source ID, transform, and no-warp state;
  legacy-only placement is unsupported.
- Export emits generated embedded `lnk2`/`liFD` data and verifies the writer's
  placed-layer IDs, dimensions, and positions before insertion. It refuses to
  append to an output already carrying linked-source/filter blocks.

Length/count/depth limits, reserved state, alignment, PNG CRC/order/profile,
exact bounded zlib output, and source budgets are checked before accepting pixels.
Malformed framing is an error.

External/alias records, nonempty `lnkE`, locked library sources, non-PNG or nested
PSD/PSB/native-document sources, unrecognized descriptor controls, and alternative
high-depth layer records are not admitted. `FEid`/`FXid`, filter descriptors, and
`FMsk` do not become native editable Smart Filters. In particular, `FMsk` display
metadata is not a grayscale filter-mask pixel plane, and an ordinary layer mask
is never reinterpreted as one. Native filter-mask editing is documented separately
in [Smart Filter masks](smart-filter-masks.md).

## Independent masks and original bytes

An admitted Smart source can carry independent ordinary raster and editable
vector masks. Coordinates use its placed origin; neither changes the retained
PNG. [PSD raster-mask](psd-raster-masks.md) baking/resampling gates and
[PSD vector-mask](psd-vector-masks.md) geometry/parameter gates still apply.
This extends the older raster-mask document's blanket Smart/vector exclusions
only for the jointly admitted subset.

Imported PNG bytes are retained exactly, including allowed metadata, RGB beneath
alpha zero, and low-alpha samples that cannot be reconstructed from native
premultiplied RGBA16. IO validates encoded and canonical decoded-source SHA-256
bindings before re-emitting them. Without retained originals, source-only export
still requires exact native → RGBA8 → native reconstruction.

Native/history version 13 shares a content-addressed PNG across live/history
references and uses it directly as the live source. Other documents keep versions
9–12; falsely lower declarations are rejected. Outer `.emu` remains version 1
unless protected retired history requires
[project format 2](native-project-envelope.md).
Replacement invalidates original bytes, Undo restores them, and no-op Apply
preserves them. Real source edits retain a layered native archive and use PSD
appearance fallback. See [native original images](native-original-images.md) for
budgets and lifecycle.

## Fallback and evidence

Unsupported imports may use only a validated genuine saved composite. A missing
or invalid composite is an error. Unsupported exports use the named whole-document
appearance layer. No retained PNG payload or placed-source record is appended to
that fallback.

`write_with_report` validates sources and checks current-pixel blend-space
compatibility in the export job. `appearance_fallback` reports unsupported
features or a blend-space difference; `baked_raster_masks` separately reports
baking on otherwise layered export. UI warnings use this completed-job report.
An 8-bit appearance match does not establish equivalent future edits or kernels.

The tests exercise native-original persistence and the bounded Smart adapter,
plus public PSD read → native save/reopen → PSD/PSB write/read with simultaneous
translated Smart source, vector mask and independent raster mask. The original
PNG bytes and canonical native source digest remain exact. The real RGB(A)
fixture also covers the PSD format's fourth exact-neutral Blend If pair; active or
malformed fourth ranges, and additional unknown ranges, still require fallback.
Source-byte preservation, saved-preview equality, independent mask semantics,
and reference-application acceptance are separate claims.

## Controlled reference-application fixtures still needed

Use self-authored, redistributable artwork and retain the untouched source PNG.
Request a small RGB8 sRGB document with an asymmetric source containing opaque
colors, alpha-zero hidden RGB, and alpha values 1–5, placed by integer translation.
Save a no-filter baseline in both PSD and PSB with a genuine compatibility
composite. Record the exact application name, version/build, OS, document/source sizes,
profile, placement, and SHA-256 of every supplied file.

From that baseline, save **one controlled change per file**:

1. A parameter-free Invert Smart Filter at Normal/100%, first without a mask;
   then a second controlled filter to establish stack order. Gaussian Blur at
   2 px is a separate later kernel/parameter fixture, not a prerequisite for
   the minimal Invert storage proof.
2. An asymmetric grayscale Smart Filter mask with known 0/64/128/192/255 samples,
   followed by separate disabled, Density, Feather, linked/unlinked movement,
   and dormant-mask cases. Supply the mask's original pixel plane and dimensions.
3. An ordinary raster layer mask and a simple vector mask, separately and together
   with the filter mask, to distinguish every mask's storage and coordinate space.
4. Shared-source duplicates, an independent duplicate, an external link, and a
   nested source as separate relationship/fallback fixtures.

Include fresh application-rendered flattened PNGs, source/filter-mask exports when
available, and screenshots of the Layers panel and exact filter/mask properties.
Also open an Emulsion-written source-only file in that recorded application build,
inspect/edit its source, save/reopen, and return the resulting files. These
fixtures are needed to establish filter pixel storage, source/filter association,
coordinate mapping, edit propagation, and actual application acceptance before
expanding the editable contract.

## Format references

- [PSD/PSB specification: Linked Layer and Placed Layer Data](https://www.adobe.com/devnet-apps/photoshop/fileformatashtml/).
- [psd-tools linked-layer parser at the pinned MIT revision](https://github.com/psd-tools/psd-tools/blob/d68bf46c7140a1f8c74be9c10b4e21103e820761/src/psd_tools/psd/linked_layer.py)
  and [Smart Object API](https://github.com/psd-tools/psd-tools/blob/d68bf46c7140a1f8c74be9c10b4e21103e820761/src/psd_tools/api/smart_object.py).
- [Vendored fixture provenance and limits](../../crates/emulsion-io/tests/fixtures/psd/README.md#smartobject-layerpsd).
