# Bundled fonts

These unmodified variable TTFs come from the official Google Fonts repository,
pinned to commit `718e1db4deb9e4d9d85a0ead1b9f5fde2761ccfd`:

| Bundled file | Exact family | Upstream path |
|---|---|---|
| `Geist.ttf` | Geist | `ofl/geist/Geist[wght].ttf` |
| `GeistMono.ttf` | Geist Mono | `ofl/geistmono/GeistMono[wght].ttf` |
| `CormorantGaramond.ttf` | Cormorant Garamond | `ofl/cormorantgaramond/CormorantGaramond[wght].ttf` |
| `CormorantGaramond-Italic.ttf` | Cormorant Garamond | `ofl/cormorantgaramond/CormorantGaramond-Italic[wght].ttf` |
| `Fraunces.ttf` | Fraunces | `ofl/fraunces/Fraunces[SOFT,WONK,opsz,wght].ttf` |
| `Fraunces-Italic.ttf` | Fraunces | `ofl/fraunces/Fraunces-Italic[SOFT,WONK,opsz,wght].ttf` |

Source: https://github.com/google/fonts/tree/718e1db4deb9e4d9d85a0ead1b9f5fde2761ccfd/ofl

## Attribution and licenses

- Geist and Geist Mono: copyright 2024 The Geist Project Authors
  (https://github.com/vercel/geist-font.git). Original licenses:
  `Geist-OFL.txt`, `GeistMono-OFL.txt`.
- Cormorant Garamond: copyright 2015 the Cormorant Project Authors
  (https://github.com/CatharsisFonts/Cormorant). Original license:
  `CormorantGaramond-OFL.txt`.
- Fraunces: copyright 2018 The Fraunces Project Authors
  (https://github.com/undercasetype/Fraunces). Original license:
  `Fraunces-OFL.txt`.

All are licensed under the SIL Open Font License 1.1. The original license
texts are included beside the fonts and staged in every application package.
Only filenames are simplified; font bytes are unchanged. `UPSTREAM.json`
records the source paths and SHA-256 hashes for fonts and their license texts.

## Offline rendering

`emulsion_core::text::BUNDLED_FONTS` is the shared face list for canvas shaping,
raster/vector output, the font chooser, GPUI and SVG import. Cormorant Garamond
adds an elegant wedding serif; Fraunces adds an expressive celebratory display
face. Both include upright and real italic faces, with variable weight support.
Geist and Geist Mono remain the UI and mono defaults. No runtime font download,
account or system-font installation is needed.

Native documents store the editable family names. SVG, PDF and HTML export
preserve text appearance through the existing glyph-outline path, so exported
invitations do not depend on a recipient having these fonts installed. Raster
exports use the same shaping source. Translated text outside each font's glyph
coverage still uses the application's normal script fallback.

## Validation

Run `python3 scripts/test-license-staging.py` to verify checksums, attribution
coverage and staged notices. Rust regression tests cover font selection without
system fonts, real italics, raster/vector output, chooser discovery, SVG import
and self-contained HTML outlines.
