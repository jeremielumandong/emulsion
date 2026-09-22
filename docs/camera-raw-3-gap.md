# Camera Raw 3 reference: gap assessment and implementation

Reference: Bruce Fraser, *Understanding Adobe Camera Raw 3* (2006), the user's
`C:\Users\jerem\Downloads\phscs2ip_camraw3.pdf`, all 14 pages. This is a functional
comparison, not a claim of Adobe rendering or file-format compatibility.

| Reference capability | Previous gap | Emulsion implementation / remaining difference |
| --- | --- | --- |
| RAW-derived previews (p.1) | Sensor development existed; embedded JPEG thumbnails could differ | Editor and export develop sensor data. Embedded thumbnails remain a fast initial interpretation, not authoritative edited previews. |
| Auto adjustments and camera defaults (pp.1–3) | No automatic tone or model defaults | Explicit Auto tone; Save/Apply/Reset defaults keyed by camera make/model. Defaults are deliberately applied explicitly, not silently on import. Auto results are concrete editable values. |
| Read-only originals, portable settings (p.3) | Only linked native projects | Fingerprint-bound Emulsion JSON sidecars, reusable presets, model defaults, and native projects. Images are never modified by saving settings. JSON is not Adobe XMP; there is no Adobe settings database interoperability. |
| Exposure, temperature, tint (pp.4–6) | Relative WB sliders only | Existing float exposure/WB plus sensor-derived neutral-point picker and independent As-shot WB reset. Temperature remains a relative warmer/cooler control, not a measured Kelvin readout. |
| Black clipping, brightness, contrast, saturation (pp.4–8) | Missing, except a differently defined shadow-lift slider | Added separate black clipping, midtone brightness, contrast, and global saturation before raster conversion. Shadow lift is retained under its correct name. Values/algorithms are Emulsion's, not Adobe slider-value equivalents. |
| Clipping diagnosis (pp.6,11) | No RAW diagnostic view | Explicit output-clipping view, red for white clipping and blue for black. This is a display-only result after RAW development, not a sensor saturation/recoverability map or Alt-drag gesture. |
| Curves and presets (pp.9–10) | No RAW-stage curve | Gamma-2.2-interface luminance curve after tonal controls, Linear/Medium/Strong presets, five editable fixed input knots. Numeric/keyboard slider editing is supported; arbitrary knot placement, multi-point selection, and Ctrl-click curve sampling are not yet implemented. |
| Section preview (p.9) | No adjustment-section comparison | Compare without tone / without curve renders transiently without changing saved pixels, recipes, history, or export. Escape or Show edited photo restores the edited view. |
| Filmstrip synchronization (pp.12–13) | No RAW setting synchronization | Select destination open RAW photos and copy All / WB / Tone / Curve groups. Each destination is redeveloped and independently undoable. Existing document tabs remain the navigator; a dedicated thumbnail filmstrip and live multi-selection editing are not implemented. |
| Workflow space, depth, size, resolution (pp.4,14) | Only depth and fixed sRGB | Photo export adds sRGB/Adobe RGB conversion, full/half/quarter size and optional resolution metadata. These are export conversions from the bounded linear-sRGB document, not a wide-gamut working document. |
| Save rendered photographs (p.14) | Existing full-resolution export | Existing formats retained; photo workflow tags profiles and preserves 16-bit PNG/TIFF output. JPEG stays 8-bit. |
| New DNG with original sensor data and metadata (pp.3,14) | No DNG writer | Not implemented: native ORA + linked original remains the lossless editing archive. An exported TIFF renamed to DNG is not a substitute. |

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

Settings files are versioned and bounded to 64 KiB. Sidecars validate the source
fingerprint; generic presets intentionally apply across photos. Sampled WB gains
are camera-channel values: use them only with the same camera model. Model
defaults are isolated by normalized make/model and are not keyed by extension.

## Scope boundaries

Adobe's algorithms, proprietary database/XMP semantics, Bridge/Photoshop plugin
hosting, and 2006 installation paths are not copied. The reference is not a
modern camera-support list. Actual decoder compatibility remains documented in
the [sample matrix](../crates/emulsion-io/tests/fixtures/RAW-CORPUS.md).

Remaining substantial gaps are true wide-gamut RAW working storage, saturated
channel highlight reconstruction, arbitrary-point curve tools, a dedicated
filmstrip/live group editing, Adobe XMP interoperability, and verified DNG writing.
This implementation does not claim those features are finished.

## Verification

Validated on 2026-09-22:

- Core: 140 tests passed, one ignored.
- IO: 85 library tests passed; three synthetic RAW integration tests passed,
  covering Bayer/X-Trans decoding, editable native-project reopening, missing
  originals, and exact 8/16-bit PNG preview/export agreement.
- UI: 325 tests passed, one ignored, including actual comparison/clipping
  buttons, Escape restoration, stale settings dialogs, and selected tone-group
  synchronization with independent undo.
- Real-file regression: Nikon D50 NEF, Fujifilm X-Pro1 RAF (X-Trans), and Canon
  EOS M50 compressed CR3 passed development/edit/reopen/export checks. This is
  representative coverage, not verification of every model or compression mode.
- Strict Clippy passed for core/IO all targets and the UI library. UI all-target
  Clippy remains blocked by the existing `items_after_test_module` warning in
  `crates/emulsion-ui/src/tablet.rs:68`.

GPUI tests exercise controls offscreen; this does not replace a manual visual
assessment on a calibrated monitor or establish Adobe rendering equivalence.
