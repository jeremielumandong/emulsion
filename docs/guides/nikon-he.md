# Experimental Nikon HE/HE★ support

Emulsion pins rawler's experimental Nikon decoder from
[DNGLab PR #835](https://github.com/dnglab/dnglab/pull/835), revision
`0f044c2c30d78c4ed5fcede6ab4e8db893d566f6`. This decodes sensor data directly;
it does not substitute the embedded JPEG or require Wine or an external converter.
The source remains LGPL-2.1 and is pinned in Cargo.toml and Cargo.lock.

The RAW panel identifies Nikon JPEG XS compression as HE/HE★ and shows an
experimental warning. The upstream implementation fits chroma gains and
decompanding constants from a single scene; they are approximate. Do not claim
pixel equivalence to Nikon or Adobe, or general camera compatibility based on
this integration. The exact HE versus HE★ rate is not distinguished by the probe.

Local validation on 2026-09-24: a Nikon Z9 HE★ `_DSC1914.NEF` successfully
decoded and developed to 8256 × 5504 in Emulsion. The resulting image was visually
inspected. The user's original is neither modified nor included in this repository.

Run the opt-in full-resolution regression against a local HE/HE★ file:

```sh
EMULSION_NIKON_HE_FILE=/path/to/photo.NEF cargo test -p emulsion-io \
  --test nikon_he -- --ignored
```

Before removing the experimental label, compare sensor samples with an independent
reference decoder across cameras, compression rates and crops; validate boundary
rows, tone reconstruction and malformed-input behavior. Upstream
[LibRaw PR #826](https://github.com/LibRaw/LibRaw/pull/826) documents related
reconstruction findings; its implementation is not used here.
