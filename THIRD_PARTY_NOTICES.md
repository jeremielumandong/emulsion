# Third-party notices

Emulsion's original code is licensed under MIT. The third-party code it uses
retains its own licenses and ownership; the root MIT license does not relicense
the vendored GPUI code. Changes to this GPUI fork remain under its upstream
Apache-2.0 terms unless a file specifies another applicable license.

The GPUI family is included under `vendor/gpui/`. Its exact package versions,
release checksums, upstream repositories, and declared licenses are recorded in
[`UPSTREAM.json`](vendor/gpui/UPSTREAM.json). Most of these packages are
Apache-2.0, with copyright notices identifying Zed Industries or Longbridge.
`gpui-pre-reqwest` retains its MIT OR Apache-2.0 license choice.

Additional notices apply to Lucide and Feather icons (ISC and MIT),
Microsoft Terminal-derived rendering code (MIT), and Arboard-derived clipboard
code (Apache-2.0 OR MIT). Their license texts and original source attributions
are retained; see [`vendor/gpui/LICENSING.md`](vendor/gpui/LICENSING.md) for exact
locations and the provenance of supplemental license files.

This notice inventories the vendored GPUI family. Other dependencies retain
their own licenses; this is not an exhaustive inventory of the entire Cargo
dependency graph. Release packages must also retain notices required by their
other bundled dependencies and assets.

## Bundled fonts (SIL OFL-1.1)

Geist and Geist Mono are bundled unmodified in `assets/fonts/`, copyright
2024 The Geist Project Authors (https://github.com/vercel/geist-font.git).
Their SIL Open Font License 1.1 texts are `assets/fonts/Geist-OFL.txt` and
`assets/fonts/GeistMono-OFL.txt`. Pinned source and checksums are recorded in
`assets/fonts/README.md`.

Cormorant Garamond (upright and italic) is bundled unmodified, copyright
2015 the Cormorant Project Authors (https://github.com/CatharsisFonts/Cormorant).
Fraunces (upright and italic) is bundled unmodified, copyright 2018 The Fraunces
Project Authors (https://github.com/undercasetype/Fraunces). Their original
SIL Open Font License 1.1 texts are `assets/fonts/CormorantGaramond-OFL.txt` and
`assets/fonts/Fraunces-OFL.txt`. These display faces support offline invitation
typography. All bundled font source paths, licenses and checksums are recorded
in `assets/fonts/UPSTREAM.json`; fonts are embedded without runtime downloads.

## English spelling dictionary (MIT AND BSD)

The en_US Hunspell dictionary derived from SCOWL (Kevin Atkinson and
contributors), as packaged by `dictionary-en` 4.0.0, is bundled unmodified in
`assets/dictionaries/` for caption spell checking. Its license text, covering
SCOWL and the word lists it draws on, is `assets/dictionaries/en_US-LICENSE.txt`;
the source and checksums are recorded in `assets/dictionaries/README.md`.

## Film preset library (MIT)

The 451 Adobe Camera Raw `.xmp` presets in `assets/develop-presets/film-library/`
are copied unmodified from peva3/Lightroom-Presets
(https://github.com/peva3/Lightroom-Presets), copyright 2026 peva3, under the
MIT License in `assets/develop-presets/film-library/LICENSE`. The source commit
is recorded in `SOURCE.md`; `scripts/sync-film-presets.py` refreshes them. The
editing limits in `crates/emulsion-mcp/src/raw_looks.rs` follow that project's
`STYLEGUIDE.md`. Film and brand names describe the looks the presets emulate;
they are not affiliated with or endorsed by the film manufacturers.

## Windows DirectX Shader Compiler

Windows builds stage Microsoft's unmodified DirectX Shader Compiler release
`v1.8.2505.1` (`dxc_2025_07_14.zip`) beside the executable. Its pinned download
and SHA-256 are recorded in `scripts/lib/windows-dxc.ps1`. The compiler and
validator ship with the release's `LICENSE-LLVM.txt`, `LICENSE-MIT.txt`, and
`LICENSE-MS.txt` under `licenses/dxc/` in the Windows package.

Source and release: https://github.com/microsoft/DirectXShaderCompiler/releases/tag/v1.8.2505.1

## RAW decoding (LGPL-2.1)

Camera RAW files are decoded by the `rawler` crate (https://github.com/dnglab/dnglab),
licensed under the GNU Lesser General Public License v2.1. Emulsion uses it unmodified
and links it into the binary. Under LGPL §6, you may relink Emulsion against a modified
`rawler`: the crate's source is available from its repository at the version recorded in
`THIRD_PARTY_CRATES.md`, and Emulsion's own source is available under MIT so a modified
build can be produced. The LGPL text ships with release packages.

The current build pins Nicolai Buchwitz's experimental Nikon HE/HE* branch at
`0f044c2c30d78c4ed5fcede6ab4e8db893d566f6`:
https://github.com/nbuchwitz/dnglab/tree/0f044c2c30d78c4ed5fcede6ab4e8db893d566f6/rawler.
This is the unmodified source used by the Cargo dependency, including its
LGPL-2.1 JPEG XS decoder. See `docs/guides/nikon-he.md` for validation limits.

## Community recipe library

The film recipes under `crates/emulsion-recipes/library/` are camera settings
shared publicly by Fujifilm photographers. They were collected from two public
repositories and are regenerated by `scripts/gen-recipe-library.js`:

- Open Fuji Recipes (https://github.com/matthieurobin/open-fuji-recipes,
  https://openfujirecipes.com/), a community list compiled by Matthieu Robin.
  Each recipe names its creator (for example Ritchie Roesch of Fuji X Weekly)
  in its `author` field.
- fujifilm-recipes (https://github.com/akirichev/fujifilm-recipes), X RAW STUDIO
  profiles collected by Alexander Kirichev from Fuji X Weekly and F16, provided
  "as is".

The recipes are lists of camera settings; Emulsion's rendering of them is its
own approximation. FUJIFILM and X RAW STUDIO are trademarks of FUJIFILM
Corporation.

## Everything else

The third-party PSD regression fixtures and saved-preview PNG under
`crates/emulsion-io/tests/fixtures/psd/`, apart from the separately credited
`blend-if/` fixture below, come from the MIT-licensed
[psd-tools project](https://github.com/psd-tools/psd-tools). The directory includes
the license and per-fixture pinned source/checksum. The mask-parameter fixture
also records the contributor's explicit Photoshop-authorship and test-suite
permission; this additional statement is not attributed to the other fixtures.
Self-authored vector interchange outputs are generated separately by the example
program. These test assets are not application artwork.

`crates/emulsion-io/tests/fixtures/psd/blend-if/` contains one Photoshop Blend If
metadata fixture from [Patchy](https://github.com/SethRobinson/Patchy), copyright
2026 Seth A. Robinson, under MIT. The directory preserves `LICENSE.Patchy`, the
exact upstream revision, original checksums, provenance and proof limitations.
This test-suite fixture is not application artwork.

`crates/emulsion-core/tests/fixtures/photoshop-smart-filter-mask/` contains
four derived raw arrays from two Photoshop-authored Smart Filter captures in
[Patchy](https://github.com/SethRobinson/Patchy), copyright 2026 Seth A. Robinson,
under MIT. The directory preserves `LICENSE.Patchy`, pinned source and derived
checksums, independent extraction code and narrowly scoped rendering evidence.
These test inputs are not application artwork or editable imported sources.

`THIRD_PARTY_CRATES.md` lists every crate in the build with its declared licence; it is
generated from `cargo metadata` by `scripts/gen-third-party.py`. The About screen in the
application shows the same information.

## draw.io stencil artwork

`assets/diagram-stencils/drawio.json.gz` contains XML stencil definitions from
[jgraph/drawio](https://github.com/jgraph/drawio), pinned to the commit and
per-file SHA-256 checksums in `assets/diagram-stencils/UPSTREAM.json`. Definitions
are interpreted locally as vector drawing instructions. The original Apache
license and additional stencil asset terms are distributed as `LICENSE-APACHE`
and `LICENSE-STENCILS` in that directory. Vendor names and trademarks remain
the property of their respective owners.

## AWS and Azure architecture icons

`assets/diagram-stencils/cloud-icons.json.gz` contains the official AWS
Architecture Icons (Amazon Web Services) and Azure Public Service Icons
(Microsoft), packaged as SVG by `scripts/refresh-cloud-stencils.py`. They are
provided for creating architecture diagrams under each vendor's icon usage
terms. AWS, Azure and related names and marks are trademarks of their owners.

Additional native translations of draw.io dynamic geometry are documented in
`assets/diagram-stencils/UPSTREAM-DYNAMIC.json` with pinned source checksums.
Copyright (c) 2006–2010, JGraph Holdings Ltd; distributed under Apache-2.0.
