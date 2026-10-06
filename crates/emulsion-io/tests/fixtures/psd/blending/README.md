# Independent Photoshop blending references

These small references separate raw layer inputs from independently saved output.
`photoshop_blend_goldens.rs` builds new native `CompositeTree` nodes from numeric
layer data, calls `render_tile_cpu`, and compares the entire result with the
reference PNG at zero tolerance. The four knockout gates explicitly select
`BlendSpace::PhotoshopSrgbV1` (`photoshop-srgb-v1`) and set the verified Background
target; the two existing Fill/clipping gates retain legacy `BlendSpace::Srgb`.
It never opens a PSD through Emulsion, consumes a whole-image
appearance fallback, or uses reference pixels as native input. It does not test
GPU rendering, vector coverage, layer-style generation, or Smart Filters.

## Source, license, and authorship

All original PSDs and `group-clipping-photoshop.png` come from
[psd-tools revision d68bf46c7140a1f8c74be9c10b4e21103e820761](https://github.com/psd-tools/psd-tools/tree/d68bf46c7140a1f8c74be9c10b4e21103e820761/tests/psd_files).
The repository's MIT license, copyright 2019 Kota Yamaguchi, is retained at
[`../LICENSE.psd-tools`](../LICENSE.psd-tools). Its pinned upstream SHA-256 is
`79f7e019a30b97542cce6f6482a75ba26771b4204c374fec305dc8cf558d137f`.
That license has no image/fixture exclusion. No ag-psd artwork is included.

[`manifest.json`](manifest.json) records each exact upstream-relative path,
source SHA-256, writer/reader strings, raw input pixel hashes, and expected RGBA
hashes. Schema 2 also records each node’s source-record index, physical channel
IDs, flags byte, `lspf`, `lnsr`, and explicit Photoshop Background role, plus the
case’s root target index. The verification script independently pins the source hashes; changing
only the manifest cannot silently authorize new source material. Upstream raw
URLs have the form:

`https://raw.githubusercontent.com/psd-tools/psd-tools/d68bf46c7140a1f8c74be9c10b4e21103e820761/tests/psd_files/<source_path>`

The MIT license establishes redistribution terms; the following separate
evidence establishes the limited Photoshop provenance:

- **opacity-fill.psd:** embedded VersionInfo says writer `Adobe Photoshop`,
  reader `Adobe Photoshop CC 2019`. Its XMP history identifies creation in CC
  2018 and a final save in CC 2019 on June 5, 2019. It was added by
  [commit ab6e6e48099312c65b89a184afacf7cae16e3341](https://github.com/psd-tools/psd-tools/commit/ab6e6e48099312c65b89a184afacf7cae16e3341).
- **knockout fixtures:** embedded VersionInfo identifies Photoshop 2026.
  [Authoring commit 0a58ab02ef2230a98fbd09b68046c2f911e6c4d6](https://github.com/psd-tools/psd-tools/commit/0a58ab02ef2230a98fbd09b68046c2f911e6c4d6)
  explicitly documents Photoshop-authored fixtures and verifies their expected
  pixel values against Photoshop. The native harness does not use psd-tools'
  implementation of knockout. Its upstream tests initially marked the knockout
  cases as strict expected failures of that implementation.
- **group-clipping:** [commit 4847388b1ddb551f2cdaad5efdd65d90a6d9f903](https://github.com/psd-tools/psd-tools/commit/4847388b1ddb551f2cdaad5efdd65d90a6d9f903),
  dated September 24, 2025, adds separately named Photoshop and Clip Studio PNG
  references and tests each against the corresponding compatibility mode.
  Photoshop's exact version is **not recorded**. The PSD itself has no XMP or
  VersionInfo; it must not be described as Photoshop-authored. Upstream also
  marks this PSD as a known broken read/write fixture. Only the separately
  supplied Photoshop PNG is used as this case's output oracle.

Except for the untouched upstream group PNG, the PNGs here are lossless
extractions of existing PSD merged channels using
`PSDImage.topil(apply_icc=False)`. They are **not** new psd-tools composites.
Their dimensions, exact RGBA bytes, and constant control colors are reverified
from the pinned PSDs. The other sources contain an sRGB ICC profile, but both
raw layer channels and expected merged channels are deliberately decoded with
ICC conversion disabled. Applying the profile during extraction changes, for
example, the knockout sources' raw `(0,255,0)` green to `(1,255,0)` in this
environment. That extra conversion is not part of this blend proof.

## Exact controlled cases

All lists are bottom to top, in document coordinates. All sources are RGB8,
visible, with integer placement, neutral Blend If, no active effects, and no
vector masks. All modes are Normal except the explicitly named pass-through
outer group. Native scene input is decoded from raw layer channels and metadata,
not sampled from merged output. There is no alpha or color fitting.

### Fill and Opacity, independently retained

`opacity-fill`: 32×32, one full opaque red `(255,0,0,255)` raw layer. Opacity and
Fill are each **171/255**, `clbl=1`, `infx=0`, `knko=0`. Its empty zero-area raster
mask has outside value 255 and no channel samples, so it is an identity gate.
The saved Photoshop output is **`(255,0,0,115)` at every pixel**. This tests the
native independent Fill and Opacity envelopes on transparency; it says nothing
about the eight special-Fill modes, styles, or blending over a colored backdrop.

### Clipped pixel member inside an isolated group

`group-clipping`: 627×510, all Fill and Opacity 255, all RGB values below have
alpha 255 unless otherwise stated:

1. White background, bounds `(0,0,627,510)`.
2. Black pixel base, bounds `(162,99,465,388)`.
3. Normal isolated group, containing:
   - Red `(255,6,0)` pixel base, bounds `(258,44,483,468)`.
   - Blue `(0,23,255)` clipped pixel member, bounds `(175,80,400,504)`.
     Its source is 225×424. Its alpha is zero only in local half-open rectangle
     `(191,0,225,423)` and is 255 everywhere else. The verifier checks every raw
     source byte against this numeric description.

The stored group clipping byte is 1, but the independent Photoshop PNG displays
the group unclipped; only blue clips to red. The manifest records both stored
and effective clipping explicitly. The native scene thus uses an unclipped
group with a clipped pixel member. This is not evidence that a native group
which itself clips to a lower sibling will roundtrip to Photoshop. It also
does not exercise a **group as clipping base**, fractional base alpha, or a
non-Normal clipping envelope.

The PSD's own merged image is different: it clips the group to black, with
35,577 pixels different from the separate Photoshop PNG. The fixture verifier
asserts that those two images are not interchangeable.

### Photoshop 2026 knockout boundary matrix

All are 32×32. Start with an explicit opaque white Photoshop Background, then
an ordinary opaque red layer named BG. A group
`Outer` contains opaque green, then an `Inner` group containing opaque blue.
Inner Fill is **128/255**, Opacity 255, and `infx=0`, `clbl=1`. All raw source
colors are the exact primary colors, decoded without ICC conversion.

| Fixture | Outer mode | Inner Knockout | Every expected RGBA pixel |
| --- | --- | --- | --- |
| knockout-none-nested | Normal | None (0) | `(0,127,128,255)` |
| knockout-shallow-nested | Normal | Shallow (1) | `(127,0,128,255)` |
| knockout-deep-nested | Normal | Deep (2) | `(127,0,128,255)` |
| knockout-deep-nested-pt | Pass Through | Deep (2) | `(127,127,255,255)` |

The control separates ordinary color interpolation from knockout behavior.
The deep isolated/pass-through pair distinguishes the isolation boundary.
The PSDs contain disabled style presets: each effect descriptor has `enab=false`
and `present=false`, and each legacy `lrFX` effect has enabled=0. The decoded
active effect lists are empty, but the raw style records are not empty. These
are not styled-knockout tests and do not authorize ignoring unknown descriptors.

Background identity is established by the combination of a bottom-root pixel
record, no clipping, exactly RGB channels `[0,1,2]` without a transparency
channel, layer flags `0x09`, protected flags `lspf=0x0d`, and `lnsr=bgnd` on that
same record. The verifier walks length-delimited records and tags directly and
cross-checks them against psd-tools' hierarchy by source-record identity. It
never searches all bytes for `bgnd` or uses a layer name as evidence. In all
four sources the target is raw record 0 and native root index 0. The red BG
control has channels `[-1,0,1,2]`, flags `0x08`, `lspf=0`, and no `lnsr`; its
opacity and name do not make it a Background. Neither other fixture has a target.
The Rust helper checks the complete evidence again before mapping the explicit
root target index to `CompositeTree.knockout_background`. A rename regression
also confirms that giving the ordinary red layer the name Background does not
transfer the role.

All four cases are normal, non-ignored tests in `photoshop_blend_goldens.rs`.
They use the explicitly selected `PhotoshopSrgbV1` profile and independent
numeric layer inputs, with no PSD import selection or saved-appearance fallback.
In addition to the independent RGBA8 PNG comparison, the production conversion
path is pinned to these premultiplied-linear RGBA16 contract values:

- None/isolated: `[0,13909,14146,65535]`
- Shallow/isolated and Deep/isolated: `[13909,0,14146,65535]`
- Deep/pass-through: `[13909,13909,65535,65535]`

These raw16 assertions are native storage-contract checks for the fixed RGB8
references; they are not independent 16-bit Photoshop exports. Legacy Normal
interpolation remains on its existing arithmetic and is not changed by these
new-profile assertions.

The separate diagnostic explicitly prints `photoshop-srgb-v1` and reports all
RGBA8 mismatches, pixel counts, maximum byte errors, and the first actual/expected
pixel. It exits nonzero if any mismatch remains. Its expected values and
zero-tolerance comparison must never be changed to fit the renderer. A clean
run establishes only these four fixed cases under the selected profile, not
parity for other blend modes or source families.

## Reproduce and verify

With psd-tools 1.23.0 and Pillow available:

```sh
python scripts/verify_photoshop_blend_goldens.py
# Metadata-only refresh; does not rewrite expected PNGs:
python scripts/verify_photoshop_blend_goldens.py --write-manifest
# Only to regenerate manifest and extracted saved PNGs from pinned inputs:
python scripts/verify_photoshop_blend_goldens.py --write
cargo test -p emulsion-io --test photoshop_blend_goldens
cargo run -p emulsion-io --example photoshop_blend_diagnostics
```

The script makes no network requests, installs nothing, and invokes no renderer.
It checks original source hashes, sizes/depth, input colors/alpha rectangles,
flags, record-scoped Background roles, neutral gates, hierarchy, saved pixels,
and the canonical manifest.
The Rust test also hashes original PSDs, expected decoded pixels, and every
reconstructed raw source before using the native CPU compositor. PNG encoding
differences between Pillow versions do not matter; decoded pixels do.

## Interchange implications and remaining evidence gaps

The clipped-group fixture is concrete evidence of a format compatibility risk.
A conservative appearance fallback for **a group that itself has clipping** is
defensible on native export, and for a source group carrying clipping=1 on
import. That inference must not be broadened to groups serving as clipping
bases. Import fallback would preserve the source file's saved merged appearance,
which in this fixture is different from the separate Photoshop application
output; name that distinction rather than claiming both are identical.

[Adobe's clipping guide](https://helpx.adobe.com/photoshop/using/revealing-layers-clipping-masks.html)
(page updated September 25, 2023) documents successive clipped layers and base
semantics but does not settle group-node compatibility. The fixture does not
establish a universal rule for current Photoshop versions. Before relaxing a
conservative export guard, obtain a controlled current Photoshop save/open test
for a group which is itself clipped, separately from a group used as a base.

The separate [Patchy Blend If fixture](../blend-if/README.md) remains metadata
evidence for Gray/R/G/B ordering and its exact 40-byte record with an identity
fourth per-channel pair. It is not part of this rendering gate and does not
establish broad Blend If renderer parity.

Nothing here establishes fractional knockout mask/Opacity, root-level Shallow,
nondefault pass-through envelopes, complex styled clipping, Interior Effects As Group
toggles, mask-hides-effects behavior, knockout with styles, the eight special
Fill formulas, or Fill/Opacity with Blend Clipped Layers As Group off. Those
families still need independently authored controlled input/output pairs.
Existing CPU/GPU self-agreement and whole-image preview preservation cannot
substitute for those pairs. No current Photoshop application was run as part of
this harness, and no editable vector/filter recomputation is claimed.
