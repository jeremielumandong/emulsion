# Independent stored-encoded-RGB Smart stack-mask oracle

These four arrays (20,800 bytes) come from two independently authored PSD
captures in [Patchy at 20f95a201c395213ce3e212f13d912e418d1cba6](https://github.com/SethRobinson/Patchy/tree/20f95a201c395213ce3e212f13d912e418d1cba6).
`manifest.json` pins both source URLs/SHA-256s and every derived array SHA-256.
The original PSDs are not redistributed here.

## Provenance and redistribution

The repository's [MIT license](https://github.com/SethRobinson/Patchy/blob/20f95a201c395213ce3e212f13d912e418d1cba6/LICENSE)
is copyright 2026 Seth A. Robinson, with no fixture/image exclusion. The exact
notice is `LICENSE.Patchy`. The [legal constraints](https://github.com/SethRobinson/Patchy/blob/20f95a201c395213ce3e212f13d912e418d1cba6/docs/legal-constraints.md)
identify these regression assets as self-authored, not vendor-supplied artwork.
[Capture introduction 52091ed6681326e93dae41fd7c4bafc3b3c2832e](https://github.com/SethRobinson/Patchy/commit/52091ed6681326e93dae41fd7c4bafc3b3c2832e)
says the Smart Filter captures were authored/reopened in the reference PSD editor, version 27.8;
independent XMP reads report creator tool 27.8 (Windows). This is upstream
provenance, not a new local reference-editor acceptance run.

## What is measured

Every array is unsigned 8-bit, row-major, in a 40×40 document grid at (0,0).
RGBA arrays are channel-interleaved; the mask has one channel.

- `projected-unfiltered-rgba8.bin`: instance B's placed **FEid unfiltered
  cache**, matched by SoLd `placed` UUID `efc62d75-9b6e-174a-a70c-f543171a472b`.
  This is **not the editable embedded PNG source**. Its occupied bounds are
  (15,18)–(33,28); alpha includes 225,238,241,244,255.
- `unmasked-filtered-rgba8.bin`: base instance B's actual saved RGBA layer
  channels, with layer bounds (4,6)–(40,40). These are unmasked filter inputs,
  **not final rendered output**.
- `shared-mask-u8.bin`: document-x bands 0,64,128,192,255, each eight pixels wide.
  FEid row lengths are 32-bit even though the containing file is PSD version 1.
- `photoshop-rasterized-target-rgba8.bin`: instance B's actual final rasterized
  pixel-layer channels from the separate rasterized capture, placed at its
  (8,6)–(40,40) layer bounds. This ordinary pixel layer has no Smart descriptor
  or raster mask. It is not a PSD merged preview, fallback, or product render.
  Unchanged instance A serves as a separate-file control.

The raw encoded-premultiplied crossfade matches the final target within one
8-bit code in RGB where target alpha is nonzero, and within one code in alpha
everywhere. Applying sRGB linearization before crossfade misses by up to 34 RGB
codes. Wrong source-local mask-origin controls also disagree. The fixed
acceptance bound is **one 8-bit code**, allowing separately quantized saved
stages. It is not exact byte identity or a fitted filter kernel.

## Deliberate limits

Both PSDs are explicitly untagged (resource 1041 = 1), with no ICC profile;
the embedded PNG has no ICC/sRGB/gAMA tag. This establishes interpolation of
**stored encoded RGB values**, not the original document's colorimetry, ICC
conversion, or gamma setting. The native `PhotoshopSrgbV1` test represents those
numeric inputs using its explicitly defined sRGB transfer and linear-premul-u16
storage. Its input/storage/output quantization stays within the declared bound.

Native Gaussian rendering, nonuniform resize, arbitrary filter blend/opacity,
linked mask movement, density/feather, outside-canvas mask semantics, editable
PSD Smart Filter import/export, and newly authored reference-editor acceptance are
not established by this test. The native test deliberately injects both cache
inputs rather than regenerating them with an unverified filter or resize.

## Reproduce

With existing psd-tools 1.23.0, Pillow and numpy, obtain the two pinned PSDs from
`manifest.json`, then run:

```sh
python3 extract.py --inputs /path/to/pinned-psds --output /tmp/mask-oracle
```

The independent reader checks source hashes, placed UUID association, untagged
profile, actual unmasked raster target, and all four derived hashes. It does
not invoke Emulsion or Patchy's parser/renderer, the authoring editor, a Gaussian kernel,
a resizer, or a downloader. Rust tests embed the checked-in arrays and need no
Python dependency or network. See `smart_filter_mask_space_tests.rs` for the
final-pixel comparisons, negative controls and separate native micro-oracles.
