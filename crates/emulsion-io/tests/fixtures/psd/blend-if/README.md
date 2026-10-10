# PSD Blend If metadata evidence

This directory contains one untouched, independently authored RGB8 PSD
fixture. It establishes logical channel ordering and a neutral fourth
per-channel range. It is **not** a native Emulsion rendering golden.

## Source and redistribution license

- Repository: [SethRobinson/Patchy](https://github.com/SethRobinson/Patchy).
- Pinned revision: `20f95a201c395213ce3e212f13d912e418d1cba6`.
- [Original PSD](https://github.com/SethRobinson/Patchy/blob/20f95a201c395213ce3e212f13d912e418d1cba6/test-fixtures/psd/photoshop-blend-if-4b-roundtrip.psd).
- [Exact raw download](https://raw.githubusercontent.com/SethRobinson/Patchy/20f95a201c395213ce3e212f13d912e418d1cba6/test-fixtures/psd/photoshop-blend-if-4b-roundtrip.psd).
- Size: 26,210 bytes. SHA-256:
  `ba61934630531a9fb7943d66872d16c9a60804a0cb9471b04a4a389d8c98e881`.
- The repository's [MIT license](https://github.com/SethRobinson/Patchy/blob/20f95a201c395213ce3e212f13d912e418d1cba6/LICENSE),
  copyright 2026 Seth A. Robinson, is preserved verbatim in `LICENSE.Patchy`.
  License SHA-256:
  `bbc50c8c376e0e5980939be7df6769feed1a30289c7efc6391b204dfb15de88d`.
  It contains no image/fixture exclusion. The pinned
  [third-party notice](https://github.com/SethRobinson/Patchy/blob/20f95a201c395213ce3e212f13d912e418d1cba6/NOTICE-THIRD-PARTY.md#test-fixtures-not-distributed-with-the-application)
  lists separately sourced fixtures, but gives no separate license or exclusion
  for this PSD. This is a test-suite fixture; no application packaging is needed.
  No ag-psd artwork is included.

## Authoring provenance, separately from licensing

- The PSD's VersionInfo identifies the original authoring application as
  writer and its 2026 release as reader, file version 1. Its `has_composite` flag is false;
  this fixture must not be assumed to supply an enabled compatibility composite.
- Embedded XMP identifies authoring-application build 27.8 (Windows), creation at
  `2026-07-12T01:30:33+09:00`, and modification at
  `2026-07-12T01:30:40+09:00`.
- [The upstream calibration record](https://github.com/SethRobinson/Patchy/blob/20f95a201c395213ce3e212f13d912e418d1cba6/docs/layer-effects-render.md#per-effect-calibration-notes)
  attributes these fixtures and their reference renders to automated captures from that authoring
  application. The [fixture's introducing commit](https://github.com/SethRobinson/Patchy/commit/6e256930b9b2aed5bf76268d299b8709f9c00990)
  is also public; its short commit message alone does not prove authorship.
- [Upstream fixture regression](https://github.com/SethRobinson/Patchy/blob/20f95a201c395213ce3e212f13d912e418d1cba6/tests/core/psd_structure_tests.cpp#L555-L655)
  pins the exact original bytes below and compares its own independent renderer
  to a separately supplied reference BMP with a stated tolerance. That test was
  inspected, not executed here. Its source SHA-256 is
  `b57bc298c4f68489c7801db4656e3522f8f43c59afb8cde6b7925e27b643b889`.

The separate upstream `photoshop-blend-if-4b-render.bmp` was inspected for its
identity, but is not included or consumed by this metadata regression. Its
SHA-256 is `4fd8a4ca65a60f28eff1fd9bd94b3787a183754eb2c5fe9b0176c4978e069ed6`.
No third-party application was run for this integration.

## What establishes the channel order

The [PSD specification](https://www.adobe.com/devnet-apps/photoshop/fileformatashtml/)
defines the composite-gray pair followed by a variable list of source/destination
channel pairs, each range containing two black and two white bytes. Its table
does **not** explicitly name the order of those per-channel pairs.
[The vendor's user guide](https://helpx.adobe.com/photoshop/using/layer-opacity-blending.html#specify_a_tonal_range_for_blending_layers)
separately describes Gray and the Red/Green/Blue controls and their 0–255 values;
it is not proof of binary ordering either.

Independent implementation evidence supplies the missing association:

- [Patchy channel enum and model](https://github.com/SethRobinson/Patchy/blob/20f95a201c395213ce3e212f13d912e418d1cba6/src/core/layer.hpp#L149-L158)
  use Gray, Red, Green, Blue in native record order. Source SHA-256:
  `66f46e58a9f6f292579b9db9d0606dd7f6b9f37a4d54df213e43148dea8a3451`.
- [Its decoder and writer](https://github.com/SethRobinson/Patchy/blob/20f95a201c395213ce3e212f13d912e418d1cba6/src/core/layer.cpp#L67-L132)
  map successive eight-byte pairs into that model and require a final identity
  pair for the supported 40-byte RGB shape. Source SHA-256:
  `39a5bdd1d17a72595292d233ce688d8f0de7d7ed144e6dea10eb305b44819251`.
- The upstream fixture regression linked above labels each distinct original
  pair Gray/R/G/B and verifies the authoring application's reference render.

The 16×16 RGB8 PSD's `Blend If Normal` layer has these exact pairs. Each row is
four This Layer bytes, then four Underlying Layer bytes:

| Logical range | This Layer | Underlying Layer |
| --- | --- | --- |
| Composite Gray | 11, 37, 201, 239 | 19, 53, 187, 227 |
| Red | 3, 33, 203, 233 | 13, 43, 193, 223 |
| Green | 5, 35, 205, 235 | 15, 45, 195, 225 |
| Blue | 7, 37, 207, 237 | 17, 47, 197, 227 |
| Fourth per-channel pair | 0, 0, 255, 255 | 0, 0, 255, 255 |

The range payload starts at byte 23,464 and has length 40. Channel records start
at byte 23,416 and have physical IDs `[-1, 0, 1, 2]`: transparency is physically
first, while the logical blending ranges put the neutral extra pair last.
These offsets were also checked by a small direct length-delimited byte walk,
independently of psd-tools 1.23.0's matching parse. Never derive Blend If ordering
from physical channel-record ordering. The final pair is identity regardless of
its channel semantics; an active/unknown tail is not supported by this evidence.

## Regression scope and conservative boundaries

`src/psd/blend_photoshop_fixture_tests.rs` pins the original PSD's SHA-256,
header, raw range bytes, physical IDs, and decoded Gray/R/G/B/tail values. The
original `Blend If Normal` has four active gates and must remain unsupported by
the one-gate native model. The original Background has four neutral per-channel
pairs and must remain identity.

The mapping tests make temporary, in-memory byte edits that neutralize all but
one selected pair. They check Gray, Red, Green, and Blue import independently of
Emulsion's exporter. These are **synthetic metadata derivatives**. The authoring
application did not author or render them, and their untouched previews are not reference output.
No derivative is stored or treated as a third-party rendering golden.

Keep rejection of multiple active gates, active/malformed fourth per-channel
pairs, further pairs, and active non-RGB channel ranges. This fixture supports
the RGB index association and exact neutral tail; it does not establish all
possible channel counts or acceptance of newly exported files by third-party editors.
In particular, its 40-byte shape does not directly validate an export omitting
the last identity pair. Actual third-party open/edit/save is a separate check.

Remaining renderer semantics are recorded separately in
`RENDERING-EVIDENCE.md`; the metadata regression does not change legacy rendering.
