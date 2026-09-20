# Vendored GPUI licensing

This directory contains the 29 GPUI-family packages recorded in `UPSTREAM.json`,
copied from their exact crates.io release archives. That manifest records archive
SHA-256 checksums, declared licenses, available VCS metadata, and the license
files present in the original archives. Missing VCS metadata is not inferred.

The original licenses, copyright headers, and embedded sources remain intact.
Most packages are Apache-2.0. `gpui-pre-reqwest` is MIT OR Apache-2.0; both
upstream license texts are retained. These dependencies keep their own licenses
regardless of Emulsion's root license.

## Supplemental license files

The following files were added by Emulsion to complete the packaged notices:

- `gpui-kit/LICENSE-APACHE` and `gpui-kit-assets/LICENSE-APACHE` are exact copies
  of `gpui-base/LICENSE-APACHE` from version 0.6.4. All three packages report
  the same upstream revision, `3c387ae0a3e9b14ee39fe98be2b51a882800aa16`, in
  `.cargo_vcs_info.json`. The kit and assets release archives omitted the text.
  The notice attributes Copyright 2024–2026 to Longbridge.
- `licenses/MICROSOFT-TERMINAL-MIT.txt` is the unchanged license retrieved from
  [Microsoft Terminal at revision 1283c0f5b99a2961673249fa77c6b986efb5086c](https://github.com/microsoft/terminal/blob/1283c0f5b99a2961673249fa77c6b986efb5086c/LICENSE).
  The existing source headers identify adaptations from that revision in
  `gpui-pre/src/platform.rs`,
  `gpui-pre-apple/vendor/gpui/src/platform.rs`, and
  `gpui-pre-wgpu/src/shaders.wgsl`. Their original attribution comments are
  retained. This supplemental license covers those Microsoft-derived portions.

`gpui-kit-assets/LICENSE-LUCIDE` is an original packaged notice covering Lucide
icons under ISC and listed Feather-derived icons under MIT. Preserve the entire
file. No font binaries are included in these 29 package archives.

The Arboard-derived clipboard code in
`gpui-pre-linux/src/linux/x11/clipboard.rs` retains its upstream dual-license
and copyright header; its Apache-2.0 option is covered by that crate's included
`LICENSE-APACHE`.

## Maintaining this fork

When modifying upstream files, add a prominent comment identifying the Emulsion
change, retain existing notices, and record the patch in the vendor documentation.
Apache-2.0 section 4 requires modified-file notices in addition to retaining
license and attribution texts. Do not replace upstream notices with Emulsion's
copyright or license.

Distribute these license texts and attribution documentation with source and
binary releases containing this code. `UPSTREAM.json` describes original archive
contents; the supplemental files above intentionally are not presented as files
from those archives. This inventory covers the vendored GPUI family, not every
transitive dependency or externally installed graphics driver.
