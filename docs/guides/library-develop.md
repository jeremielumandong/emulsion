# Library and Develop

Reference layout: `Emulsion Editor v2.dc.html`, Library screen.

## Layout and implemented workflow

- Library/Develop module bar, resizable GPUI Kit side panels, central grid/canvas,
  and full-width filmstrip. Develop places Navigator, presets, snapshots/history
  and collections on the left, histogram and adjustment accordions on the right.
  Tab toggles side panels; Filmstrip toggles the bottom strip.
- Library keeps Auto, B&W and Reset in compact Quick Develop so recipe browsing
  remains accessible. Switch to Develop for the complete adjustment panels.
- The Classic-style photographic workspace uses a neutral gray stage and contained
  thumbnails. Navigator appears in both modules; the Library folder list navigates
  imported catalog folders. Develop keeps its histogram, editing tools and Sync/Reset
  visible while adjustment panels scroll. Import/export sits above the filmstrip.
- Search names and keywords; sort by filename or EXIF capture time; grid, list,
  Develop/loupe, and RAW before/after comparison; bounded filmstrip navigation.
- Import local folders into the persistent catalog, create collections, and add
  selected photos to collections. Imported files remain at their original paths.
- GPUI Kit checkboxes select photos and control sidebar filters using pointer or keyboard.
  Click selects a photo; Shift extends a range; Ctrl/Cmd toggles individual photos.
  Ratings 0–5, pick/unflag/reject, five color labels, keywords, and matching filters.
- Keyboard culling while the grid has focus: 0–5 for ratings, P/U/X for flags,
  arrows to navigate, Shift+arrows for ranges, G for grid, E for Library loupe, D for Develop, Enter
  for Photo. Typing in search fields does not trigger culling shortcuts.
- Light: exposure ±5 EV, contrast, highlights, shadows, whites, blacks. Existing
  Photo documents retain their separate black clipping and brightness settings.
- Color: relative white balance/tint, as-shot restore, saturation, vibrance, B&W.
- Effects: texture, clarity, dehaze, vignette. Detail: sharpening and conventional
  edge-preserving noise reduction.
- Histogram, Auto tone, reset, undo and saved history, built-in RAW presets, portable preset
  files, and synchronization of all settings, tone/effects, white balance, or curve.
- Info and Keywords inspector tabs expose file and camera metadata and keyword editing.

## Persistence and consistency

Photo edits save automatically after a short pause to fingerprint-bound
`<original>.emulsion-raw.json` sidecars. Save edits is also available explicitly.
The original is never overwritten. Library preview, Photo, thumbnails with edits,
and batch export share the same development settings and engine. Loading older
settings defaults new controls to neutral and preserves the existing render path.

Unsaved or failed saves remain visible. Export and opening in Photo require saved
settings so they cannot silently use older pixels. Source replacement or externally
changed saved settings are detected; Reload saved explicitly discards the current
photo's in-memory draft and reads the saved recipe again. Adobe XMP preset translation
is described below; Emulsion sidecars use their own JSON format. Sampled camera-channel white balance cannot be synchronized across
unknown camera models; tone/curve groups remain available.

A single active RAW development worker and bounded thumbnail workers avoid
unlimited decode queues. Stale results cannot replace the active photo. Only the
active source mosaic is cached; per-photo drafts and undo store settings.

Import immediately starts a bounded pair of thumbnails before grid layout;
subsequent work follows the visible rows. Grid cards show loading feedback and a
Retry preview button when thumbnail
decoding fails; hovering the button explains the error. `get_library` includes
each photo’s thumbnail status and error. Sorting and returning from Photo retain
unchanged thumbnails; background file/sidecar checks refresh only changed files.
Autosave refreshes only saved photos and does not rerender an already current
Develop preview. Per-photo revisions reject older in-flight thumbnail results.
Setting the view through `set_library_view` retries previously failed previews.

Nikon files whose decoder omits sensor dimensions use the RAW TIFF directory
dimensions for memory accounting, never the camera JPEG size. The memory limit
remains unchanged, and selecting a different RAW releases the prior cached mosaic.

## Expanded Develop and catalog workflow

The Library Develop inspector has Basic, Crop / lens, Curve, Mixer, Grading,
Masks, Kelvin, History, and Enhance sections. Its bounded scroll region keeps
recipe controls accessible. All sections use the same persisted `DevelopParams`
as Photo, thumbnails and export; neutral defaults preserve existing recipes.

- Normalized crop edges, aspect ratios, straighten, horizontal/vertical
  perspective, automatic line-based leveling/perspective, manual distortion and red/blue fringe correction. Match lens
  profile resolves Lensfun EXIF calibration, downloading the database on first
  explicit use. Measured coefficients are stored in the recipe.
- Interactive composite and RGB channel curves with up to 32 control points per channel, eight-channel HSL mixer, three-zone
  color grading, absolute illuminant Kelvin control, RGB histogram and display
  clipping overlay. Overlays never change exported pixels.
- Eight radial/linear local-adjustment slots, including inversion, feathering,
  exposure, saturation and temperature. AI subject masks, automatic semantic sky masks and click-guided sky
  masks use installed local models; mask PNG assets are content-addressed in
  the application data directory. Automatic sky uses SkySeg U-2-Net; guided sky uses SAM and requires a sky point.
- JPEG, TIFF, PNG and WebP use fingerprint-bound nondestructive sidecars and the
  same tonal/color/geometry pipeline. Source depth and transparency are retained;
  this does not restore sensor information absent from rendered photos.
- Saved history (up to 100 previous settings) and named snapshots survive restart.
  Virtual copies are small `.emuphoto` references plus independent sidecars;
  they share the protected original rather than copying full-resolution pixels.
  Verified relinking updates catalog virtual references and rebinds their independent histories.
- Smart collections evaluate rating, flags, color label, RAW status and text.
  Stacks group catalog IDs and can collapse. Optional SHA-256 deduplication and
  fingerprint-verified relinking preserve originals. Portable `.emulibrary` backups include photo originals, virtual references,
  sidecars, presets and mask assets with hashes. Restore places photos in a new
  directory, rebinds references and creates a safety catalog backup. Legacy JSON
  catalog backups retain references only. Conflicting shared resources are rejected.
- Export presets hold long-edge sizing (no enlargement), JPEG quality, source/
  8-/16-bit depth, output sharpening and image watermark settings. Export omits source metadata by default; optional copyright, camera or
  camera-and-location retention uses an allowlist. Serial numbers and maker notes
  are always excluded. Named presets use Emulsion JSON. Optional WebDAV publishing
  uses content-versioned filenames and conditional writes, preserving remote files.
- AI restoration runs installed Real-ESRGAN and combines its output per tile at
  the original resolution. This is RGB restoration after development, not Bayer
  or X-Trans mosaic denoising. Super resolution uses the installed upscaler.
  Both write new 16-bit PNG derivatives into the local catalog. A separate RAW-only
  Sensor denoise slider applies CFA-aware bilateral reduction before demosaicing;
  it is conventional sensor processing, not neural RAW denoise. Jobs can be
  canceled; originals and previously completed derivatives are retained.
- Ask Library owns an assistant relay without creating a Photo tab. Its host
  accepts Library tools and explains how to open a Photo document for other tools.

## Lightroom and VSCO interoperability

Import preset pack accepts individual `.xmp`, `.lrtemplate`, Emulsion `.json`,
and ZIP packs. ZIP members are validated as inert preset data; member paths are
never extracted directly, and Lua code is never executed. Saved presets appear
in the inspector and preserve parameters omitted by imported Adobe presets.
A compatibility report lists translated adjustments and unsupported active settings.
Known disabled controls and descriptive preset metadata do not generate warnings;
unknown settings remain visible even when their value is zero. Related limitations
are grouped in expandable, bounded compatibility notes. Import preset pack installs
presets; choose a listed preset to apply it to the selected photo. The `get_library`
MCP state retains the complete notes in its `preset_import` and `preset_import_notes` fields.

Lightroom catalog import recognizes the common SQLite file-reference/rating/flag
schema via the local `sqlite3` command in read-only mode. It reports missing files
and unsupported formats. Collections and recognized readable JSON/Lua Develop
history records migrate into Emulsion history/snapshots. Private binary history
formats are reported as unsupported; existing Emulsion sidecars are preserved.

The companion [Lightroom plugin](../../integrations/lightroom/README.md) runs inside
Lightroom and exports originals, settings, collections and 16-bit TIFF references.
Import its `handoff.emulr.json` through the Lightroom catalog importer. Rendered
references retain the Lightroom/VSCO appearance; translated RAW settings remain
approximations. The companion has syntax and mocked-SDK contract tests; live
Lightroom host validation remains outstanding.

**Import and interoperability limits:**

- Native `.lrplugin` execution requires the Adobe Lua SDK host; it is not provided.
- DCP/LCP/Adobe Look profile payloads and proprietary VSCO camera rendering are
  not reproduced. Profile-only presets are rejected, and partially supported
  presets report the omitted profile. No VSCO assets are bundled.
- RGB curves preserve their control points; unsupported Adobe adjustments still
  produce compatibility warnings. Adobe DCP/Look color science is not reproduced.
- Automatic sky, conventional sensor denoise and line-based perspective use
  Emulsion's own algorithms and models.
- WebDAV publishing is available; Adobe Publish Service plugins and vendor-specific
  cloud services are not hosted.

### WebDAV setup

Load a destination JSON from the export panel, or pass `settings.publish` to
`export_library`. The destination collection must already exist:

```json
{"url":"https://dav.example.com/photos/","authorization_env":"EMULSION_DAV_AUTH"}
```

Set the named environment variable to the complete Authorization header before
starting Emulsion. Credentials are never written into presets. HTTPS is required
except for local loopback testing. An export remains local if publishing fails.
Cancellation stops subsequent files; a running request has a 120-second timeout.
Local contract tests cover conditional upload and repeat-publish hash verification;
no external account or user photos were used for testing.

References: [Adobe Lightroom SDK](https://developer.adobe.com/lightroom-classic),
[Camera Raw XMP schema](https://developer.adobe.com/xmp/docs/xmp-namespaces/crs/),
[VSCO Lightroom preset support](https://support.vsco.co/en/articles/12698551-vsco-presets-for-adobe-lightroom-and-capture-one).

## MCP coverage

Library tools use the live desktop workspace through Ask Library or an open
document's assistant relay. They share the visible selection, catalog, RAW drafts, sidecars,
preview renderer and export queue. Offline document hosts explicitly reject
Library requests rather than silently operating on a different catalog/session.

| Library / Develop operation | MCP integration |
| --- | --- |
| Inspect photos, selection, EXIF, collections, histogram, save/export status | `get_library` (paged, up to 200 visible items) |
| Import a local folder | `import_library` (nonrecursive, optional content deduplication; original files stay in place) |
| Smart collections, stacks, virtual copies, verified relink, catalog backup/restore, preset packs, Lightroom catalog migration | `library_catalog` |
| Search, filter, filename/capture-time sort, grid/list/Develop/before/compare, inspector and recipe | `set_library_view` |
| Select, multiselect, navigate active filmstrip photo | `select_library_photos` with explicit canonical paths |
| Ratings, pick/unflag/reject, color labels, replace/add keywords | `edit_library_metadata` |
| Create a collection and add photos | `library_collection` |
| Every persisted Develop parameter, Auto, reset, as-shot WB, undo, reload, save | `develop_library` |
| Built-in presets, portable preset files, grouped synchronization | `develop_library` (`preset`, `save_preset`, `load_preset`, `sync`) |
| Inspect rendered edits and before/after images | `get_library_preview` (PNG content blocks) |
| Local AI mask/enhancement and matching lens profile | `develop_library` (`subject_mask`, `sky_mask` with point, `auto_sky`, `auto_perspective`, `denoise`, `super_resolution`, `match_lens`), `cancel_library_enhancement` |
| Named snapshots and restore | `develop_library` (`snapshot`, `restore_snapshot`) |
| Save/load output presets | `library_catalog` (`save_export_preset`, `load_export_preset`) |
| Export selection with output settings and stop export | `export_library`, `cancel_library_export` |
| Open the active saved photo | `open_library_photo` (returns the document ID) |
| Photo-side RAW editing, picker, source relinking, camera defaults, comparison and open-tab synchronization | Existing `describe_raw`, `develop_raw`, `auto_develop_raw`, `pick_raw_white_balance`, `reset_raw`, `relink_raw`, `raw_settings`, `get_raw_preview`, `set_raw_comparison`, `list_raw_documents`, `synchronize_raw` |
| Recipe discovery/import and additional export formats/options | Existing `list_recipes`, `import_recipe`, `batch_export` |

Mutating Library tools are serialized across document relays. Read-only inspection
and export cancellation remain available during a long operation. Calls wait for
persistence/export and return failures (including partial save/export failures).
Cancellation stops subsequent photos; a file already being encoded may complete.
RAW adjustments preserve omitted settings, reject unknown fields/out-of-range
values, and refuse to overwrite an externally changed source or sidecar. Failed
saves retain drafts; export and opening Photo require saved settings. Explicit
`reload` discards the active draft. Group sync applies the same conflict checks to
targets already inspected in Library.

MCP RAW `highlights` is recovery: positive darkens highlights; the Library UI's
slider uses the opposite sign. Temperature/tint remain relative offsets unless `kelvin` is set. New
controls and `smooth_curve` are exposed in both Photo and Library tool schemas.
This coverage describes implemented features, not the import limits above. Ask Library can now start its own relay without an open document.

Coverage tests check that both RAW schemas expose every persisted parameter,
reject malformed requests, and preserve omitted settings. Headless workspace
tests exercise catalog import/culling/filter/collection persistence and RAW
adjust/save/sync/preset/undo/comparison/export, including pixel agreement with
Photo, original-byte preservation and external-sidecar conflicts. A loopback
relay test checks workspace routing and ensures progress reads do not release
another tool's mutation queue reservation.


## Desktop development additions

- Click a numeric readout for exact entry, use arrow keys to nudge its focused control,
  or double-click the control label to reset it. Sliders create one undo step per drag.
- Process 1 preserves previous rendering. New tonal/detail controls use Process 2;
  upgrades are explicit settings changes and appear in history.
- Parametric tonal regions, primary calibration/shadow tint, global grading and
  grading balance/blending render through the same engine as imported presets.
- Detail separates luminance and chroma noise controls and sharpening amount,
  radius, detail and masking. The 100% button displays native pixels; click Navigator
  to choose another region, and Fit to return. Current region rendering evaluates the
  full image before extracting the region, preserving surrounding filter samples.
- Fit RAW previews reuse a bounded camera-channel cache. After 900 ms at rest in
  Develop, full processing refines the selected preview. New edits cancel obsolete
  work; the previous image remains visible during same-photo updates. Grid thumbnails
  retain their independent cache. Demosaicing itself cannot be interrupted mid-stage.
- Import compatible DCP camera profiles through Profile. The supported subset has
  three-channel matrices, two illuminants, tone curves and HSV tables. Unsupported
  operations are rejected. Missing or mismatched profiles remain errors. Dual-profile
  interpolation uses the selected Kelvin value, with D65 as the as-shot fallback.
- Masking supports up to 64 named masks, brush/erase, radial/linear, luminance/color
  ranges, add/subtract/intersect, and existing AI bitmap masks. Local exposure,
  contrast, saturation, temperature and tint are saved nondestructively. O toggles
  the selected mask overlay. Range and adjustment values are editable from each mask.
- Heal/Clone stores editable source/target pins and bounded freehand strokes. Alt-click chooses a source; drag pins
  to reposition them. Each spot has radius, feather and opacity controls. Visualize
  spots is a preview-only high-pass view. Healing currently blends source texture with
  a local color correction; it does not synthesize missing content.
- R activates direct crop handles with a grid; straightening and a perspective guide
  use dragged lines. Q selects healing, K brush, M linear and Shift+M radial masking.
- A toggles auto advance; Shift+rating/flag advances once; 6–9 apply color labels.
  Compare photos links zoom/pan for two selected photos; Survey shows up to eight.
  These start with cached thumbnails and refine to developed fit previews.
  Ctrl/Cmd+Z in Library undoes metadata with conflict checks.

## Offline, catalog and color workflows

Build proxies prepares bounded, verified 8-bit previews for disconnected originals.
Proxy edits are approximate, and the interface identifies them. Full-resolution
inspection and export require the matching original. Unwritable originals use
app-managed sidecars; originals remain unchanged. Portable full-library backups
include originals, settings, local edit assets, masks, profiles and presets. Reconnect
originals before creating a full portable backup; proxies are a local offline cache.

Catalog migration pages up to 100,000 photo references, retaining offline entries,
keyword hierarchies, labels and category counts. Relink folder root matches verified
originals and reports individual skips. Offline entries without fingerprints may bind
metadata to a replacement only when no saved edits depend on the unverified original.
The rebuildable photo index accelerates metadata filtering while retaining shared
creative IDs. The shared JSON catalog remains authoritative for writes. Thumbnail
maintenance caps generated disk previews at 2 GiB / 20,000 files and leaves originals
alone.

Soft Proofing / Display accepts RGB ICC profiles, with gamut warnings. Default display
handling remains with the system; manual display conversion is opt-in. Proofing affects
viewing only. CMYK proofing is not provided in Library.

Wide-gamut RAW working space is opt-in and saved in the recipe. Floating camera RGB
converts to linear ProPhoto primaries before tonal processing and quantization. Library
export can retain that gamut in ProPhoto RGB or convert to Adobe RGB/sRGB, embedding the
matching ICC profile. Preview and Photo documents remain display-oriented sRGB.
Additional Photo recipes currently require sRGB working space; Develop presets and
local edits are supported on the wide RAW path. RGB imports retain their existing
sRGB input path. Choose 16-bit TIFF/PNG for wide output; JPEG/WebP remain 8-bit.

## Automation for the additions

`develop_library` and `develop_raw` share the expanded settings schema, including
`process_version`, `wide_gamut`, `camera_profile`, tonal/detail families and local-edit
asset identity. `develop_library` action `local_edits` accepts the complete bounded
mask/spot object with stable IDs. All changes use the shared save and render path.

`set_library_view` adds `loupe`, `photo_compare`, `survey`, panel/filmstrip visibility,
auto advance, mask/dust overlays, `detail_region`, `fit_preview` and `color_view`.
`library_catalog` adds `import_profile`, `create_proxy`, `relink_root`, `undo_metadata`
and `maintain_cache`. `export_library.settings.color_space` accepts `srgb`, `adobe_rgb`
and `pro_photo`; output presets preserve that choice. `get_library` exposes profile,
proxy, settings/history and layout state. Unsupported dependencies remain visible.

These controls implement Emulsion's independent rendering and workflows. They do not
promise pixel-for-pixel Adobe processing or support executable Lightroom plug-ins,
proprietary adaptive profiles, sensor highlight reconstruction, AI sensor denoise,
HDR/panorama merge or depth-aware blur.
