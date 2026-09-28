# Library orientation, curves, HDR and profiles

This follow-up keeps the Library/Develop layout and controls while using Emulsion's
application palette. Gray thumbnail mats and forced grayscale controls were removed;
contained photos retain their full image in rounded, accent-selected cards.

## Shared editing behavior

- Camera metadata controls initial orientation. Manual left/right quarter turns are
  persisted in Develop settings and shared by RAW/RGB/HDR previews and exports.
  Quarter turns preserve pixels and alpha, swap dimensions, refit the preview and
  update only the affected thumbnail. Crop/mask gestures map back to source coordinates.
- Library point-curve edits use the same monotone cubic evaluator as Photo. Graph
  and image evaluation share one method. Legacy linear settings remain linear until
  edited; dragging adds one undo step. The manual Save action lives in the fixed footer,
  so the graph no longer jumps when the first edit marks the photo dirty. Per-channel smoothing and rotation are exposed
  through the existing Photo and Library MCP settings schemas.
- DCP profile browsing renders the current source with each profile. Compatible-camera
  filtering, search, persistent favorites and a virtualized 32-thumbnail cache are
  included. Rendering runs sequentially off the UI thread. Profile import and apply
  use existing operations; `library_profiles` supplies list/preview/favorite automation.

## HDR workflow and limits

`HDR Merge…` handles 2–9 original exposures, EXIF/manual EV, translation alignment,
auto display exposure, reference-based deghosting, overlay preview and cancellation.
The new output is a no-clobber RGB32 float TIFF with linear sRGB ICC and a source/hash
manifest. Library retains float highlight data when adjusting exposure. The Photo
workspace imports a tone-mapped 16-bit raster. RAW brackets share the reference
camera white balance and enter merging before tone curves and integer quantization.
Existing Develop edits are not included in a bracket merge.

Input is limited to matching oriented dimensions, same-camera RAW or opaque RGB,
and 24 megapixels per exposure. RGB import assumes linearized sRGB rather than
estimating a camera response. Alignment estimates translation only; deghosting uses
one reference and cannot reconstruct every moving/clipped region. Reduced-resolution
previews can differ from full-resolution alignment and deghosting. No equivalence to
Adobe's rendering engine or proprietary plug-ins is claimed.

`merge_library_hdr` provides a PNG preview or a new output file plus catalog insertion.
`cancel_library_hdr` remains callable during processing. `get_library` exposes HDR
busy state and profile favorites. The shared renderer drives GUI and automation.

## Validation

- 10 core RAW tests passed, including Photo/RAW curve equivalence, continuous tangents,
  legacy settings compatibility and rotation validation.
- 348 IO unit tests passed; two optional tests were ignored. Coverage includes exact
  pixel/alpha rotation, all EXIF orientations, rotated mask overlays, HDR highlight
  recovery, alignment, deghosting, cancellation, float TIFF round trips and no-clobber saves.
- Two generated-camera integration tests passed: EXIF plus manual rotation through
  HDR, and actual DCP-rendered previews with persisted favorites.
- Six Library MCP contract tests passed, including every persisted setting and the
  new HDR/profile request validation.
- 19 Library UI tests passed; the ordinary suite skips the local-camera test. Tests
  exercise controls, rotation/thumbnail updates, stable curve bounds, one undo step
  per drag, HDR preview/merge/catalog insertion, profile browsing and existing workflows.

The broader IO run exposed an existing test using 100,000 layers to exceed a limit
that is now 150,000. Its fixture now derives its size from `MAX_NODES + 1`; production
limits and import behavior are unchanged.

Validation uses a separate source snapshot/target and temporary application data.
Unrelated uncommitted editor/diagram/GPUI work is excluded. Original photos and the
running application are preserved. Headless UI tests are not a visual screenshot review.

The explicit GPUI BGRA channel-order regression test passed. The local Nikon check
loaded all 12 NEF files from the supplied folder in 8.48 seconds; SHA-256 fingerprints
of all originals were unchanged. The complete 19-test Library suite also passed again
with the final preview conversion.

The offline release build passed without warnings. Package:
`target/library-hdr-profiles-appimage/Emulsion-0.0.3-x86_64.AppImage` (54 MiB).
Extraction-mode `--version` returned `emulsion 0.0.3` successfully.
SHA-256: `6742ca549a3cb6f7f6804421fff25a20fe208fb747e49af49f84b3907e1dbd4c`.

The new rotation/curve/profile test and HDR dialog/MCP test also passed individually
with fresh application data. Their fixtures initialize the dedicated test catalog
before rendering Library, avoiding an unrelated global-catalog startup race in tests.
