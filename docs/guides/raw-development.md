# RAW panel controls and limits

This reference describes the RAW panel's recipe controls, how their settings
are stored and processed, and their limits. Library hosts RAW development:
opening a RAW file enters Library Develop, and the full workflow is in
[Library and Develop](library-develop.md). Photo documents do not show these
controls. A Photo project with an embedded RAW recipe shows **RAW development is
in Library** and a **Develop in Library…** button. The legacy RAW MCP tools
(`develop_raw` and related tools) use the same recipe for existing projects.

## Controls

| Control | Behaviour | Limits |
| --- | --- | --- |
| RAW-derived previews | Editor and export develop sensor data. | Embedded thumbnails are a fast initial interpretation, not authoritative edited previews. |
| Auto tone and camera defaults | Auto tone produces concrete, editable values. Save/Apply/Reset defaults are keyed by camera make/model. | Camera defaults are applied explicitly, never silently on import. |
| Read-only originals and portable settings | Fingerprint-bound Emulsion JSON sidecars, reusable presets, camera defaults, and native projects. Saving settings never modifies the image. | Sidecars apply only to the photo whose fingerprint they record. |
| Exposure, temperature, tint | Float exposure (EV) and white balance, a sensor-derived Pick neutral tool, and an independent As-shot WB reset. | The RAW panel temperature slider is a relative warmer/cooler control. An absolute Kelvin illuminant is set in the Library Develop white-balance panel. |
| Tone | Highlights, Shadow lift, Black clipping, Brightness, Contrast, Saturation, Whites, Blacks, Vibrance, Texture, Clarity, Dehaze, Vignette, Sharpening, and Noise reduction, applied before raster conversion. | Highlights is a roll-off. Partially clipped sensor colours are estimated by **Reconstruct RAW highlights** in the Library Develop Detail section. |
| Clipping view | Show clipping marks output clipping: red for highlights, blue for black. | Display-only result after RAW development; not a sensor saturation or recoverability map, and never saved or exported. |
| Tone curve | Gamma-2.2-interface luminance curve after tonal controls, with Linear / Medium contrast / Strong contrast presets and five editable fixed input levels. Optional smooth (monotone cubic) interpolation. | The RAW panel curve has fixed input positions. Arbitrary per-channel point curves (composite, red, green, blue; up to 32 points) are edited in the Library Develop curve panel. |
| Section comparison | Compare without tone / Compare without curve render transiently; Before / after shows a draggable as-shot vs edited split. | Comparisons never change saved pixels, recipes, history, or export. Escape or Show edited photo restores the edited view. |
| Settings synchronization | Copy All / White balance / Tone / Curve groups to selected open RAW photos. Each destination is redeveloped and independently undoable. | Document tabs act as the navigator; there is no dedicated thumbnail filmstrip or live multi-selection editing in the editor. |
| Output space, size, resolution | Photo export converts to sRGB or Adobe RGB, full/half/quarter size, and optional resolution metadata (1–1200 ppi) for PNG, JPEG, TIFF, and WebP. | These are export conversions from the bounded linear-sRGB document, not a wide-gamut working document. |
| Rendered output | Existing export formats; the photo workflow tags profiles and preserves 16-bit PNG/TIFF output. | JPEG is 8-bit. |
| DNG output | Not provided. Native ORA plus the linked original is the lossless editing archive. | An exported TIFF renamed to DNG is not a substitute. |

## Ownership and processing

Recipes belong to `emulsion-core::raw::DevelopParams`. New fields have neutral
serde defaults, so previous native projects reopen without changing appearance.
All controls commit pixels and recipe together through `Command::DevelopRaw`.
Camera normalization, demosaicing, white balance and camera color conversion
precede float tonal development and final linear-RGBA16 quantization.

UI requests are debounced, background-developed, cancellation-aware and guarded
by request generation plus document revision. Auto tone and neutral sampling use
the same queue. Section comparison and clipping images never replace document
pixels. Saving settings rejects pending development and stale file-dialog results.

Settings files are versioned and bounded to 4 MiB. Sidecars validate the source
fingerprint; generic presets intentionally apply across photos. Sampled WB gains
are camera-channel values: use them only with the same camera model. Camera
defaults are isolated by normalized make/model and are not keyed by extension.

## Interoperability

- Emulsion sidecars, presets, and camera defaults are Emulsion JSON, not
  XMP. XMP sidecars are neither read nor written.
- XMP (`.xmp`) and `.lrtemplate` develop presets can be imported as preset
  data in the Library; plug-in code is not executed.
- DNG files are read as RAW sources; Emulsion does not write DNG.
- Adobe RGB is available as an export color space alongside sRGB.
- Supported cameras and compression modes are listed in the
  [sample matrix](../../crates/emulsion-io/tests/fixtures/RAW-CORPUS.md).

## Current limits

- Photo working documents remain bounded linear sRGB. Library has an opt-in
  linear-ProPhoto RAW development/export path; see [Library and Develop](library-develop.md).
- Highlight reconstruction is set in Library Develop (**Detail → Reconstruct RAW
  highlights**), not in this panel.
- The panel has no filmstrip or live group editing. Library has a filmstrip
  and copies settings to other photos with **Sync to selected photos**.
- XMP sidecar interoperability and DNG writing are not implemented.

## Verification

RAW decoding and development are covered by the RAW corpus tests described in
[RAW-CORPUS.md](../../crates/emulsion-io/tests/fixtures/RAW-CORPUS.md). Planned
work is tracked in the [RAW pipeline backlog](../specs/raw-pipeline-backlog.md).
