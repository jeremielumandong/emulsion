# PSD mask fixtures

## mask-parameters-no-real-channel.psd

- Source: [psd-tools fixture](https://github.com/psd-tools/psd-tools/blob/main/tests/psd_files/mask-parameters-no-real-channel.psd)
- Upstream Git blob: `db774029f8d7ba63c7345fa11e1b63cafeeab507`
- SHA-256: `71bff054dbb4e1749192ba7d99034c689c0337c46c9fdcac86d63dbce9ca299b`
- License: psd-tools MIT license, included as `LICENSE.psd-tools`.
- [Contributor provenance and test-suite permission](https://github.com/psd-tools/psd-tools/issues/693#issuecomment-5874881734): rebuilt in Photoshop CS4, no copyrighted artwork.
- [Upstream issue #693](https://github.com/psd-tools/psd-tools/issues/693), addressed by upstream PR #900.

The document is 256×256, 8-bit RGB, with three composite channels. The `heart`
layer has a `-2` raster channel, a vector mask, raster feather 3, vector density
191 and vector feather 6; it has no `-3` channel. Its 36-byte mask block exposes
the length-only real-mask-header heuristic in ag-psd 0.3.

`mask-parameters-no-real-channel.png` contains the PSD's existing saved merged
RGB pixels, extracted with psd-tools 1.23.0 `PSDImage.open(path).topil().save(...)`.
It is not a new psd-tools compositing render. The test checks that Emulsion's
explicit appearance fallback preserves those stored pixels. It does not claim
editable vector interchange or independent present-day Photoshop validation.
