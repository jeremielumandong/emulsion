# Multi-vendor RAW pipeline audit and backlog

*Snapshot from 2026-09-22. For current behavior see [README › Developing RAW photos](../README.md#developing-raw-photos).*

Status: original assessment with implementation update, 2026-09-22  
Scope: import -> decode/develop -> edits -> preview -> save/reopen -> export

## Implementation update

2026-09-24: a pinned rawler branch now provides [experimental Nikon HE/HE★
decoding](nikon-he.md), verified to open and develop one local Z9 HE★ file.
The baseline unsupported-format statements below describe released decoders;
the experimental path is not a reference-validated support guarantee.

The assessment below records the pre-implementation baseline, not the current
behavior. The first verified slice is now implemented with the existing rawler
0.8 decoder: content probing, recorded camera/variant metadata with explicit
unknowns, floating-point RAW development, versioned linked-source recipes,
undo/reopen/relink, cancellation/stale-result guards, embedded thumbnails, and
full-resolution source-verified export. PNG/JPEG/WebP/TIFF exports carry sRGB
profiles. Protected source paths survive recipe detachment and history changes.

The [measured support matrix](../crates/emulsion-io/tests/fixtures/RAW-CORPUS.md)
records successful real Nikon D50 NEF, Fujifilm X-Pro1 RAF, and Canon EOS M50 CRAW
CR3 roundtrips, alongside exact CC0 fixture provenance. Synthetic Bayer/X-Trans
DNG fixtures run in ordinary CI. This does not verify all listed vendors or modes.

Remaining backlog includes broader camera/compression coverage, calibrated color
and quality reference images, monitor color conversion, wide-gamut/HDR document
storage, lens correction profiles, advanced noise/highlight processing, and batch
workflow polish. Decoded RAW sources share a 128MP reservation and heavy stages are
serialized; this is not a hard allocator/process-memory cap. Individual upstream
decoder stages are not interruptible. The source is linked rather than embedded.

Validation of this implementation: core library suite passed (139 tests, one
ignored); IO library suite passed (73); UI suite passed (320, one ignored);
synthetic RAW development and save/reopen/export integration tests passed.
Strict Clippy passed for core/IO all targets and UI library. The broader formats
integration suite passed 10 tests but failed its converter-backed AVIF case
because the installed `convert` command rejected `-quality`; this is not a RAW
decoder failure. The real-camera matrix above was exercised separately.

## Outcome

Emulsion has a useful first RAW path, but it does not yet have a support contract that is safe to publish by vendor. It currently routes files by extension, decodes with `rawler` 0.8, develops immediately to a 16-bit sRGB raster, and remembers camera make/model as ordinary EXIF. It does **not** record the detected RAW container, camera variant, sensor/CFA, compression mode, bit depth/RAW size, decoder version, or whether that exact combination was tested.

The first production milestone should therefore be a verified slice, not a longer extension list:

> A fixture from each initial vendor can be identified by content, opened from sensor data, adjusted in a high-precision RAW stage, saved and reopened with the same settings, then exported at full resolution with preview/export agreement. The test and published support matrix identify the exact camera model and compression/mode that passed.

Do not claim support from a filename suffix. The support key is:

```text
(canonical camera model, firmware when relevant, sensor/CFA,
 compression/mode, bit depth, RAW size, image index)
```

## Current pipeline

| Stage | Current implementation | Assessment |
|---|---|---|
| Discovery/routing | `emulsion-io/src/raw.rs::RAW_EXTENSIONS`, duplicated by `emulsion-io/src/lib.rs::OPEN_EXTENSIONS`; `is_raw` checks only the suffix. | A recognized suffix is currently indistinguishable from a verified camera/mode. Content probing and one canonical registry are needed. |
| Decode | `emulsion-io/src/raw.rs::RawSource::load` calls `rawler::decode_file`; workspace dependency is `rawler = 0.8` with default features disabled. | Required families are advertised, but no real RAW fixture proves them. `rawler` rejects Nikon HE/HE-star and some DNG compression values. |
| Format variations | Source bit depth, compression and CFA layout are not represented in the persisted RAW support contract. | Verify distinct bit depths, uncompressed/lossless/lossy modes, and Bayer/X-Trans development per camera. Explicitly report unsupported variants and keep untested combinations visible. |
| Identity/metadata | `RawInfo` retains make, model, WB coefficients and dimensions transiently. `Document::info` / `ImageInfo` persists make, model, lens and exposure EXIF through the ORA manifest/history. | Camera model exists, but format, compression/mode, sensor, decoder/version and support result do not. RAW metadata is decoded twice (`raw.rs` and `exif.rs`). |
| Development precision | `RawDevelop::develop_intermediate` produces float intermediates. Exposure, WB, highlight and shadow operations happen before display gamma in `raw.rs::develop_with`; the result becomes the raster crate's 16-bit linear storage through `Raster::from_srgba16`. | Better than editing an embedded JPEG, but the developed raster is clipped to `[0,1]`. Sensor values/headroom and the RAW recipe are not part of the document, so later sessions cannot reproduce a new development. |
| RAW development | Development delegates sensor processing to rawler, adjusts camera WB coefficients, and applies exposure and a highlight roll-off in `raw.rs`. | Validate black/white levels, as-shot/manual WB, Bayer/X-Trans demosaicing, highlight handling and camera-to-working-space conversion independently. A successful decode or a highlight roll-off alone does not establish development correctness. |
| Color | RAW development uses rawler's camera-to-sRGB path. Non-RAW ICC import converts to sRGB in `emulsion-io/src/icc.rs`; the raster working representation is linear sRGB. | There is no persisted input/camera profile identity, selectable working space, display-profile conversion, render intent, or export-profile option. Export files are not consistently tagged. |
| Orientation/corrections | `raw.rs::finish` applies orientation. Lensfun lookup and editable distortion/vignetting filters exist in `emulsion-io/src/lensfun.rs` and `emulsion-ui/src/editor/lens.rs`. Generic noise reduction and sharpening filters exist. | Crop/active-area semantics need fixtures. Lensfun TCA is parsed but chromatic-aberration coefficients are not applied by `Filter::LensProfile`. Corrections are not an integrated RAW-stage policy. |
| Nondestructive behavior | `emulsion-ui/src/editor/raw_panel.rs` keeps an `Arc<RawSource>` and `DevelopParams` in session, then replaces the bottom raster with an undoable command. | The original on disk is not overwritten, but RAW source linkage and parameters are not in `Document`, ORA, or history. Reopening an ORA loses the RAW panel and future re-development. |
| Preview/performance | Opening and redeveloping use GPUI background tasks. A generation counter avoids leaving an old result as the final result, but `develop_now` still commits a completed stale raster before starting the newer request. | Work is neither truly cancelled nor prevented from briefly landing/adding history. Every RAW gallery thumbnail performs a full development because `thumb.rs` does not read embedded previews. Decoded mosaics are unbounded per tab and cache keys do not include a RAW recipe. |
| Export | `export.rs` flattens the current document at level 0; PNG/TIFF/EXR/Farbfeld can retain more than 8-bit output. | Export uses the already-developed raster rather than re-developing the RAW from persisted settings. ICC output tagging and explicit gamut conversion are missing. |
| Compatibility tests | `raw.rs::raw_extensions_are_recognised` checks ARW/DNG/RAF names and a missing file. | There is no legal sample corpus, model/mode manifest, corrupt-input suite, pixel/reference assertion, memory budget, or generated support matrix. |

One packaging mismatch should be fixed with the registry work: the Linux desktop MIME list advertises Samsung SRW, while `RAW_EXTENSIONS` and `OPEN_EXTENSIONS` omit `srw`.

## Decoder decision

### Recommendation

Put RAW decoding behind an internal `RawDecoder` interface and run the corpus spike with **LibRaw 0.22.x as the production candidate** and the existing **rawler 0.8 path as the baseline**. Do not replace rawler or publish a broader support claim until the same fixtures pass both the decoder and Emulsion's complete develop/export path.

LibRaw is the stronger default candidate because its supported-camera program and format breadth cover the requested families, including modern CR3/CRAW, while rawler calls itself alpha, does not promise SemVer stability, warns against hostile inputs, and currently rejects Nikon HE/HE-star. LibRaw is not a complete imaging pipeline: use it to identify/unpack sensor data and metadata, then own development/color policy in Emulsion. Nikon Z HE/HE-star remains a known gap even in LibRaw and must produce a specific unsupported-compression error.

The spike must settle three release concerns before adoption:

1. Pin the exact LibRaw version and build features on Windows, macOS, Linux, AppImage and Flatpak; verify CR3 and optional DNG codecs in the shipped artifacts.
2. Select and document one of LibRaw's LGPL-2.1 or CDDL-1.0 licensing modes, including notices and relink/source obligations. This is an engineering recommendation, not legal advice.
3. Decide whether decode runs in a bounded worker process. LibRaw has recent malformed-file hardening; rawler explicitly warns that malformed input may panic/abort. A worker gives real cancellation and contains decoder failures.

Rejected as the sole decoder:

- **RawSpeed** is a fast, fuzzed sensor decoder with useful per-camera modes, but deliberately omits general metadata, demosaic, color processing and thumbnails, and is not a clearly complete CR3 solution.
- **rawloader** does not meet the mandatory CR3 requirement.

Primary upstream evidence: [LibRaw scope, licenses and update policy](https://github.com/LibRaw/LibRaw), [LibRaw 0.22 supported cameras](https://www.libraw.org/supported-cameras), [LibRaw 0.22 change log](https://github.com/LibRaw/LibRaw/blob/master/Changelog.txt), [DNGLab/rawler status and format table](https://github.com/dnglab/dnglab/blob/main/README.md), [DNGLab sample-mode taxonomy](https://github.com/dnglab/dnglab/blob/main/CONTRIBUTE_SAMPLES.md), [RawSpeed scope](https://github.com/darktable-org/rawspeed), and [RawSpeed camera/mode schema](https://github.com/darktable-org/rawspeed/blob/develop/data/README.md).

## Required persisted model

Add a decoder-neutral model to `emulsion-core/src/document.rs` and serialize it with defaults in `emulsion-io/src/ora.rs` and `history.rs`. Keep the original vendor values as well as normalized values so a future classifier can improve without losing evidence.

```rust
struct RawProvenance {
    decoder: DecoderIdentity,       // name, exact version, build/features
    container: RawContainer,        // CR2, CR3, NEF, NRW, ARW, RAF, RW2, ORF, PEF, DNG, ...
    reported_make: String,
    reported_model: String,
    canonical_make: String,
    canonical_model: String,
    firmware: Option<String>,
    compression: RawCompression,    // normalized enum with Unknown/Other
    vendor_mode: Option<String>,    // original code/name, never discarded
    bits_per_sample: Option<u8>,
    raw_size: RawSize,              // Full/L/M/S/Unknown
    sensor: SensorLayout,           // Bayer pattern, X-Trans, monochrome, Foveon, ...
    image_index: u32,
    image_count: u32,
    flags: RawFlags,                // CRAW/sRAW, dual pixel, pixel shift/high-res, crop/aspect
    source: RawSourceRef,            // content digest + linked/embedded/relocatable reference
    warning: Option<RawWarning>,
}

struct RawRecipe {
    schema_version: u32,
    exposure_ev: f32,
    white_balance: WhiteBalance,    // as-shot or explicit multipliers/temperature+tint
    highlight_recovery: ...,
    demosaic: ...,
    camera_profile: ...,
    corrections: ...,
    output_transform: ...,
}
```

`Unknown` is a valid, visible state. It must never silently collapse to “uncompressed” or “supported.” Persist a digest and file facts, not only an absolute path. For portable ORA files, offer an explicit linked-original policy and an opt-in embedded-original policy; reopening a missing link must keep the developed proxy visible and offer relinking.

## Prioritized backlog

Estimates are focused engineering time for one experienced Rust engineer and include unit/integration tests, not legal review or the waiting time to acquire samples. The P0 verified end-to-end slice is approximately **5-8 engineer-weeks**. The complete backlog below is approximately **9-13 engineer-weeks**, plus corpus acquisition and legal/packaging validation.

### P0.1 — Define the support contract and corpus manifest (3-5 days)

**Touches:** new `crates/emulsion-io/tests/raw/` harness and fixture manifest; `docs/`; CI fixture-fetch/staging script.

**Work**

- Define the support key and normalized enums above before changing decoders.
- Create a manifest that records provenance/license, SHA-256, make/model, firmware, container, compression/mode, bit depth, size, sensor/layout, orientation, expected dimensions and expected outcome.
- Start with at least one legally redistributable/CI-accessible sample for Canon CR2 and CR3/CRAW, Nikon NEF and NRW plus an HE negative case, Sony ARW, Fujifilm RAF compressed and uncompressed, Panasonic RW2, OM/Olympus ORF, Pentax PEF, and DNG.
- Cover 12-, 14- and 16-bit sources where available, and uncompressed, lossless-compressed and lossy-compressed modes. Record packed storage separately from effective sample depth; the editor's 16-bit output is not evidence of source bit depth.
- Include both Bayer and X-Trans sensor fixtures, recording the actual CFA pattern and validating demosaic output. Test the combinations a camera actually produces; mark unavailable combinations as not applicable.
- Store large fixtures outside git or in LFS/object storage; CI downloads pinned hashes. Keep a tiny corrupt/truncated mutation set in-repo when licensing permits.

**Acceptance**

- Every fixture has a recorded license and digest.
- Tests select cases by model/mode, not extension.
- Each tested bit-depth/compression/sensor combination has its own result. A passing Bayer or uncompressed file cannot certify X-Trans or compressed variants. Distinguish verified, unsupported and untested combinations.
- Supported variants pass dimension, black/white-level normalization, CFA/color and full-resolution output checks against fixture expectations. Unsupported bit depths, compression modes and sensor layouts identify the detected variant and reason; unknown metadata remains unknown. Never silently substitute an embedded JPEG for RAW editing.
- The harness distinguishes unsupported camera, unsupported compression, corrupt/truncated input, resource-limit rejection and decoder failure.
- A generated support matrix can only mark a tuple supported after the full pipeline test passes.

**Depends on:** none. This gates every support claim.

### P0.2 — Add decoder-neutral probing and provenance (4-6 days)

**Touches:** `emulsion-core/src/document.rs`, `emulsion-io/src/raw.rs` (split into probe/decoder/develop modules), `emulsion-io/src/exif.rs`, `emulsion-io/src/lib.rs`, `emulsion-io/src/ora.rs`, `emulsion-io/src/history.rs`, Linux packaging metadata.

**Work**

- Introduce `RawDecoder::{probe, decode}` and `RawProbe`/`RawProvenance`; keep rawler behind the first adapter.
- Probe content before suffix routing. Use suffix only as a discovery hint.
- Consolidate `RAW_EXTENSIONS` and `OPEN_EXTENSIONS` into one registry and reconcile SRW.
- Decode RAW metadata once and map make/model plus compression/mode into normalized and original fields.
- Version the ORA manifest and preserve backward-compatible serde defaults.
- Return actionable errors containing detected model/mode and the unsupported dimension.

**Acceptance**

- A renamed RAW still probes correctly; a JPEG renamed `.nef` does not enter the RAW decoder.
- Make/model/compression/mode survive save/reopen and history round-trips.
- Unknown compression is displayed as unknown.
- Existing ORA files still open; newer files are rejected safely by older format-version logic.

**Depends on:** P0.1 manifest schema.

### P0.3 — LibRaw packaging and corpus spike (5-8 days)

**Touches:** workspace/build scripts, a new decoder adapter or helper crate/process, `THIRD_PARTY_*`, packaging manifests and CI.

**Work**

- Pin LibRaw 0.22.x and its features; implement `RawDecoder` without exposing FFI types outside the adapter.
- Build/package on all release targets and confirm runtime discovery/loading from installed artifacts.
- Run LibRaw and rawler over the same corpus; record decode outcome, dimensions, metadata, black/white levels and deterministic image hashes/statistics.
- Enforce input pixel/allocation/time limits. Prototype process isolation and cooperative termination.
- Record the decoder/build identity in every result.

**Acceptance**

- Each initially supported tuple passes on Windows, macOS and Linux release packages.
- Unsupported Nikon HE/HE-star and any unavailable DNG codec fail with a specific message, never a generic extension error or embedded-JPEG fallback.
- Corrupt fixtures cannot crash the application process or exceed the agreed memory/time limit.
- Licensing/notices packaging checks pass.

**Depends on:** P0.1, P0.2. Decoder selection is finalized only here.

### P0.3a — Validate the RAW development stages (required milestone gate)

**Touches:** `emulsion-io/src/raw.rs`, decoder adapter, `emulsion-color`, RAW corpus tests and the high-precision boundary in `emulsion-raster`.

**Work**

- Preserve decoder calibration metadata and normalize sensor samples using the applicable black and white levels, including per-channel or spatial corrections when supplied. Record the metadata source and any fallback; reject invalid ranges rather than silently using the integer type's maximum.
- Apply as-shot or explicit white balance in the sensor/linear development stages. Validate finite, positive coefficients, define a visible fallback for missing calibration, and apply exposure before display rendering.
- Select demosaicing from the actual CFA layout, including Bayer pattern offsets and X-Trans. Record the algorithm/version in the recipe and avoid demosaicing already-linear RGB data.
- Preserve scene-linear headroom through development. Distinguish sensor saturation from display clipping, and highlight roll-off from reconstruction of clipped channels. Define behavior for partial and complete channel saturation without claiming recovery of unrecorded detail.
- Convert camera RGB to the declared linear working space using the selected camera profile/matrix, its illuminant and the required white-point adaptation. Record the transform identity and apply display encoding only at the output boundary.

**Acceptance**

- Synthetic calibration ramps verify black subtraction, white-level scaling and channel-specific calibration within declared numerical tolerances; invalid or missing calibration produces the documented error/fallback.
- Neutral-target fixtures verify as-shot and manual WB; a +1 EV setting doubles unsaturated linear values within tolerance before the output transform.
- Bayer and X-Trans fixtures verify CFA alignment, edge detail and color reconstruction against approved reference outputs, with numerical tolerances and visual crops.
- Highlight ramps retain values above display white until output rendering. Partially and fully saturated fixtures exercise the documented reconstruction/roll-off policy and produce finite output without uncontrolled hue shifts.
- Color-chart fixtures verify camera-to-working-space conversion against the selected profile/reference. These checks run before display conversion, so a plausible preview cannot conceal an incorrect input transform.
- The same calibrated development stages and versioned settings feed preview and full-resolution export.

**Depends on:** P0.1-P0.3; persistence integrates with P0.4. This gate makes the RAW-stage portion of P1.1 a first-milestone requirement; monitor conversion and additional export color options remain in P1.1. Estimate this work within the existing development/color allocation, then revise the total after the decoder spike establishes how much processing can be reused.

### P0.4 — Persist a real nondestructive RAW source and recipe (8-12 days)

**Touches:** `emulsion-core` document/node/commands/history, `emulsion-io/src/ora.rs`, `emulsion-ui/src/editor/raw_panel.rs`, workspace save/reopen and relink UI.

**Work**

- Replace session-only `RawState.params`/source ownership with a versioned document RAW recipe and source reference.
- Keep a developed proxy for immediate rendering while retaining access to sensor data for redevelopment and export.
- Make parameter edits update one RAW-development history operation and invalidate only dependent caches.
- Define linked versus embedded original behavior, digest validation, missing-source relinking and changed-source refusal.

**Acceptance**

- Open -> edit RAW settings -> save ORA -> restart -> reopen reproduces the same pixels and editable settings.
- The camera original is byte-identical and never overwritten.
- Moving an ORA plus embedded source remains editable; a missing linked source retains the proxy and clearly requests relinking.
- Undo/redo and history reopening restore both settings and pixels without duplicating full mosaics per slider move.

**Depends on:** P0.2. Can proceed while P0.3 finishes behind the decoder interface.

### P0.5 — Full-resolution develop/export parity (5-7 days)

**Touches:** `emulsion-io/src/export.rs`, batch/MCP export paths, RAW proxy/develop service, golden-image tests.

**Work**

- Make export request a level-0 development from the persisted source/recipe when available, then composite the document.
- Share one render specification between viewport, batch and export; differences may be resolution/quality only.
- Prevent a fast embedded preview or stale proxy from ever becoming the full-resolution export source.
- Add tagged PNG/TIFF/JPEG behavior once the color contract in P1.1 lands.

**Acceptance**

- Export dimensions equal the selected RAW image's intended full-resolution crop/orientation.
- Preview and export sampled colors agree within a documented tolerance for the same transform.
- Changing a RAW setting invalidates export and preview caches; an older background result cannot overwrite a newer recipe.
- Batch and interactive export produce identical pixels for identical settings.

**Depends on:** P0.3a and P0.4; color-tagging portion depends on P1.1.

### P1.1 — Explicit camera, working, display and export color pipeline (8-12 days)

**Touches:** `emulsion-color`, `emulsion-io/src/icc.rs`, RAW develop stage, viewport/GPU output, export options/UI.

**Work**

- Represent the camera matrix/DCP identity and chosen illuminant/profile in the recipe.
- Choose and document a high-precision linear working space; avoid repeated gamma round trips.
- Add monitor-profile conversion at display output and selectable output conversion/tagging at export.
- Define tone mapping/gamut mapping and rendering intent so preview and export share policy.

**Acceptance**

- Color-chart fixtures have measured error thresholds against the selected reference developer/profile.
- Wide-gamut values are not clipped merely by opening the RAW.
- Exports carry the selected ICC profile and are interpreted consistently by an independent color-managed viewer.
- A no-profile/invalid-profile path is explicit and covered by tests.

**Depends on:** P0.3 and P0.4.

### P1.2 — Integrate RAW crop/orientation and optical/image corrections (5-8 days)

**Touches:** RAW recipe/develop stages, `emulsion-io/src/lensfun.rs`, `emulsion-filters`, `emulsion-ui/src/editor/lens.rs`.

**Work**

- Preserve active area, default crop, pixel aspect and all orientation cases as metadata/recipe operations.
- Apply Lensfun distortion, vignetting and TCA in a defined order; add chromatic-aberration execution rather than only parsing it.
- Define RAW-aware defaults for hot/dead pixels, noise reduction and capture sharpening while keeping all choices editable/off.

**Acceptance**

- Orientation/default-crop fixtures match expected dimensions and corner markers.
- Lens grid and flat-field fixtures meet distortion/vignetting thresholds; TCA alignment is measurably improved.
- Noise/sharpen controls round-trip in the recipe and can be disabled to reproduce the uncorrected baseline.

**Depends on:** P0.4 and P1.1.

### P1.3 — Embedded preview, bounded caches and real cancellation (5-8 days)

**Touches:** `emulsion-io/src/thumb.rs`, decoder worker, GPUI background task orchestration in `raw_panel.rs`, workspace and batch caches.

**Work**

- Extract the best suitable embedded preview for gallery/initial paint, with orientation and provenance, then replace it with developed output.
- Key caches by source digest, decoder/build, image index, recipe version/content, scale and output transform.
- Bound decoded-mosaic, developed-tile and thumbnail caches globally; expose hit/miss/bytes telemetry in tests/logs.
- Cancel/terminate superseded decoder work, not merely suppress its result. Keep GPUI entity updates on the UI context.

**Acceptance**

- Gallery thumbnails do not perform a full sensor development when a valid embedded preview exists.
- Rapid slider movement leaves at most the newest job running/committable.
- A stress suite stays below agreed peak RSS and returns to the cache budget after closing tabs.
- Editing, source replacement and decoder upgrades cannot return stale cache entries.

**Depends on:** P0.2-P0.4. Use the existing generation guards as the starting invariant.

### P1.4 — Publish support from test evidence and add regression gates (4-7 days initially, ongoing)

**Touches:** raw corpus tests, CI, generated documentation/README, release checklist.

**Work**

- Generate the public matrix from passing manifest rows; include decoder/version and known failures.
- Add decode/develop/export golden statistics, corrupt/truncated tests, fuzz targets for probe/metadata boundaries and performance budgets.
- Add new camera/model/mode support only with a licensed fixture and passing row.
- Replace README's broad vendor claim with a link to the generated matrix until coverage justifies it.

**Acceptance**

- CI fails if documentation claims an untested tuple or a formerly passing tuple regresses.
- Each required family has at least one passing tuple, while unsupported modes remain visible.
- Release artifacts run a smoke corpus through their packaged decoder, not only the development machine's library.

**Depends on:** begins at P0.1; publication follows P0.3 and expands continuously.

## Milestone exit checklist

The first milestone is complete only when all of these are demonstrated by automated tests and release-package smoke tests:

- Representative CR2, CR3, NEF, NRW, ARW, RAF, RW2, ORF, PEF and DNG tuples identify their camera model and compression/mode.
- The corpus verifies multiple source bit depths, uncompressed/lossless/lossy modes, and both Bayer and X-Trans development, with results recorded for each actual camera/variant combination.
- Unsupported bit depth, compression and sensor variants are actionable and distinct from corrupt data; untested variants remain explicitly unverified.
- RAW exposure and white balance operate before display rendering from sensor-derived data.
- Black/white-level normalization, as-shot/manual WB, Bayer/X-Trans demosaicing, highlight handling and camera-to-working-space conversion pass the P0.3a calibration/reference checks.
- Save/reopen preserves the original relationship, versioned recipe and rendered result.
- Full-resolution export redevelops from the RAW recipe and agrees with the preview color contract.
- Embedded previews accelerate browsing but are never substituted for sensor data in editing/export.
- Cancellation, memory bounds and cache invalidation have measured tests.
- The published matrix is generated from the exact passing corpus, with licensing provenance for every sample.
