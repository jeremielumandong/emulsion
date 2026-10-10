# PSD mask fixtures

## mask-parameters-no-real-channel.psd

- Source: [psd-tools fixture](https://github.com/psd-tools/psd-tools/blob/main/tests/psd_files/mask-parameters-no-real-channel.psd)
- Upstream Git blob: `db774029f8d7ba63c7345fa11e1b63cafeeab507`
- SHA-256: `71bff054dbb4e1749192ba7d99034c689c0337c46c9fdcac86d63dbce9ca299b`
- License: psd-tools MIT license, included as `LICENSE.psd-tools`.
- [Contributor provenance and test-suite permission](https://github.com/psd-tools/psd-tools/issues/693#issuecomment-5874881734): rebuilt in a 2008-era (CS4) PSD editor, no copyrighted artwork.
- [Upstream issue #693](https://github.com/psd-tools/psd-tools/issues/693), addressed by upstream PR #900.

The document is 256×256, 8-bit RGB, with three composite channels. The `heart`
layer has a `-2` raster channel, a vector mask, raster feather 3, vector density
191 and vector feather 6; it has no `-3` channel. Its 36-byte mask block exposes
the length-only real-mask-header heuristic in ag-psd 0.3.

`mask-parameters-no-real-channel.png` contains the PSD's existing saved merged
RGB pixels, extracted with psd-tools 1.23.0 `PSDImage.open(path).topil().save(...)`.
It is not a new psd-tools compositing render. The test checks that Emulsion's
explicit appearance fallback preserves those stored pixels. It does not claim
editable vector interchange or independent present-day third-party validation.

## smartobject-layer.psd

- Source: [psd-tools minimal Smart Object fixture](https://github.com/psd-tools/psd-tools/blob/d68bf46c7140a1f8c74be9c10b4e21103e820761/tests/psd_files/layers/smartobject-layer.psd)
- Upstream repository revision: `d68bf46c7140a1f8c74be9c10b4e21103e820761`.
- SHA-256: `a34abf773a64f21b59d5a854f7d394e7ac41e402d0ce546ddd86d19c764a63a2`.
- License: psd-tools repository MIT license, included as `LICENSE.psd-tools`.
  Its license has no fixture/image exclusion; no additional authorship or
  present-day third-party-application verification is inferred from that license.

This is a 32×32 embedded RGBA8 PNG Smart Object, using an identity placed-layer
transform and an empty external-linked-data block. It has no Smart Filter and
no filter-effect mask. Tests may independently inspect its embedded source and
UUID mapping; it is not evidence for editable Smart Filter-mask interchange.
