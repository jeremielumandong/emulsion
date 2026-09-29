# Library and Develop desktop validation

2026-09-28. Implementation follow-up to the [feature audit](library-develop-feature-audit.md).
Usage and MCP contracts are in the [Library guide](../../guides/library-develop.md).

The desktop workflow now has Library and Develop modules, resizable side panels,
Navigator, presets and history, a central preview, adjustment sections and a full-width
filmstrip. Library keeps compact Quick Develop controls. GPUI components provide
checkboxes, sliders, dialogs and panel resizing. This is an independent implementation;
it is not a certification of complete Lightroom Classic equivalence.

## Validation environment

Tests and packaging use a separate source snapshot and Cargo target. Unrelated
in-progress changes in the shared workspace are excluded from that snapshot. Test
catalogs and settings use temporary data directories. The existing application is
not restarted, and the package has its own output directory.

The automated desktop test checks panel ordering, preview bounds and the full-width
filmstrip, then applies local edits through MCP and verifies persisted rendered pixels.
Other Library tests cover selection, grid/loupe navigation, GPUI checkboxes, thumbnail
reuse, metadata, presets, history, saved edits and export. These are headless interaction
checks; they do not establish pixel-for-pixel visual parity with another application's
screens.

Final targeted results:

| Test group | Passed | Failed | Ignored |
| --- | ---: | ---: | ---: |
| Core RAW/local-edit contracts | 12 | 0 | 0 |
| IO RAW, catalog, profiles, masks, color, proxy and thumbnails | 66 | 0 | 2 |
| MCP Library/RAW contracts | 10 | 0 | 0 |
| Final Library UI and recipe-browser regression tests | 17 | 0 | 1 |

The camera-sample and benchmark tests are opt-in and were run separately. The
broader UI selection also passed 14 other photo-related tests before the final
Library-only corrections. Library regressions found during validation were fixed:
compact Quick Develop restores recipe access, compatibility details remain visible,
and the recipe-browser test now checks scrolling to output controls with the
filmstrip present. No full-workspace pass is claimed.

The offline release build passed. The separate 54 MiB AppImage is
`target/library-develop-desktop-appimage/Emulsion-0.0.3-x86_64.AppImage`.
Extraction-mode `--version` returned `emulsion 0.0.3` without starting the editor.
SHA-256: `0427eed5d971c303e9747bc67b66f7710f5845f0022fc72faba5d088d3882cbb`.

## Measured workflows

The read-only RAW benchmark used the supplied Nikon `_DSC0019.NEF` (4288 × 2848).
Times are one local debug-profile run with optimized dependencies, not a cross-machine
performance guarantee or a before/after comparison.

| Operation | Measurement |
| --- | --- |
| Decode source | 208 ms |
| Cold fit preview, 1280 × 850 | 476 ms |
| Eight cached preview edits | 62–97 ms |
| Full-resolution development | 1117 ms |
| Resident memory after three reopens | 170504, 170516, 170556 KiB |
| Peak resident memory in benchmark | 604328 KiB |
| Global reusable camera-preview budget | 128 MiB |
| 100,000-photo index build | 267 ms |
| Indexed catalog query, 54 matches | 13.6 ms |

The source SHA-256 remained
`0e2f77a4d81d4c75e53115046f3c6b7c6a70d53e1b1c201d348306effc38de3d`.
The sample-folder UI test loaded thumbnails and selected each of 12 Nikon RAW files.
The supplied Chic preset imported 84 adjustments. Two compatibility notes remain:
independent rendering can differ, and its named Default Color profile is unavailable.
No matching proprietary profile is bundled or silently substituted.

## Scope and remaining limits

Follow-up implementation and validation are tracked in the
[remaining-work ledger](../implementation/library-remaining-work.md). Its newer
CMYK proofing, multi-guide geometry and content-aware healing entries supersede
the corresponding original limitations below; the test counts above describe
the original validation run.

- DCP support covers bounded three-channel matrix, illuminant, tone-curve and HSV-table
  profiles. Unsupported structures are rejected. As-shot dual-profile interpolation
  currently uses D65 when no explicit Kelvin value is selected.
- Native-pixel inspection currently renders the full frame before extracting the region;
  a tiled region renderer remains a performance improvement. Demosaicing cannot be
  interrupted inside its current processing stage.
- Healing blends source texture with local color correction. Guided perspective uses
  a single approximate guide; a multi-guide geometric solver and content-aware healing
  are not implemented.
- Wide-gamut processing is available for RAW Library development/export. RGB imports and
  Photo documents retain their existing sRGB path. Additional Photo recipes currently
  require sRGB working space. Library proofing accepts RGB ICC profiles, not CMYK.
- Proxies support approximate offline editing. Full-resolution export and portable full
  backups require verified, online originals. The shared JSON catalog remains the write
  authority; the photo index is a rebuildable read store.
- Sensor-channel highlight reconstruction, AI sensor denoise, HDR/panorama merge and
  depth-aware blur remain the separate research projects identified in the audit.
  Executable Lightroom plug-ins and proprietary adaptive profiles are not supported.

Broad camera/profile/illuminant coverage and photographic quality comparisons remain
ongoing validation work. Import coverage and synthetic pixel tests do not prove Adobe
rendering equivalence.
