# Library and Develop

Reference layout: `Emulsion Editor v2.dc.html`, Library screen. Workflow reference:
Lightroom Classic desktop Library and Develop. This is an implementation and
parity ledger, not a claim that Emulsion reproduces every Lightroom feature or
Adobe's proprietary rendering.

## Layout and implemented workflow

- Workspace navigation above a 220px collections/filter/presets sidebar, central
  photo grid, and 300px inspector. Narrow windows expose the inspector as a drawer.
- Search names and keywords; sort by filename or EXIF capture time; grid, list,
  Develop/loupe, and RAW before/after comparison; bounded filmstrip navigation.
- Import local folders into the persistent catalog, create collections, and add
  selected photos to collections. Imported files remain at their original paths.
- Click selects a photo; Shift extends a range; Ctrl/Cmd toggles individual photos.
  Ratings 0–5, pick/unflag/reject, five color labels, keywords, and matching filters.
- Keyboard culling while the grid has focus: 0–5 for ratings, P/U/X for flags,
  arrows to navigate, Shift+arrows for ranges, G for grid, D/E for Develop, Enter
  for Photo. Typing in search fields does not trigger culling shortcuts.
- Light: exposure ±5 EV, contrast, highlights, shadows, whites, blacks. Existing
  Photo documents retain their separate black clipping and brightness settings.
- Color: relative white balance/tint, as-shot restore, saturation, vibrance, B&W.
- Effects: texture, clarity, dehaze, vignette. Detail: sharpening and conventional
  edge-preserving noise reduction. These are Emulsion algorithms, not Adobe's.
- Histogram, Auto tone, reset, session undo, built-in RAW presets, portable preset
  files, and synchronization of all settings, tone/effects, white balance, or curve.
- Info and Keywords inspector tabs expose file and camera metadata and keyword editing.

## Persistence and consistency

RAW edits save automatically after a short pause to fingerprint-bound
`<original>.emulsion-raw.json` sidecars. Save edits is also available explicitly.
The original is never overwritten. Library preview, Photo, thumbnails with edits,
and batch export share the same development settings and engine. Loading older
settings defaults new controls to neutral and preserves the existing render path.

Unsaved or failed saves remain visible. Export and opening in Photo require saved
settings so they cannot silently use older pixels. Source replacement or externally
changed saved settings are detected; Reload saved explicitly discards the current
photo's in-memory draft and reads the saved recipe again. No Adobe XMP compatibility
is implied. Sampled camera-channel white balance cannot be synchronized across
unknown camera models; tone/curve groups remain available.

A single active RAW development worker and bounded thumbnail workers avoid
unlimited decode queues. Stale results cannot replace the active photo. Only the
active source mosaic is cached; per-photo drafts and undo store settings.

## Remaining Lightroom Classic parity work

The handoff is the layout target; full Classic parity remains broader than this
implementation. Outstanding areas must remain explicit:

| Area | Remaining work |
| --- | --- |
| Develop | Library-local crop/straighten, interactive tone-curve editor, HSL/color grading, calibrated Kelvin WB, RGB clipping overlays, local adjustment masks |
| Lens and geometry | Automatic lens-profile matching, chromatic aberration, perspective/upright in the Library Develop workflow |
| AI | Subject/sky masks, AI denoise, super resolution; conventional denoise and deterministic Auto tone are not substitutes |
| Catalog | Smart collections, virtual copies, stacks, import deduplication/relinking, persistent develop history/snapshots, catalog backup workflows |
| Other image formats | General JPEG/TIFF Develop controls; current shared RAW controls target camera originals, while other images retain recipes and Photo editing |
| Export | Named export presets, resize/output sharpening/metadata policy, watermarking and publish services |
| Interoperability | Adobe XMP/catalog import, proprietary camera/profile rendering equivalence |

Photo already provides other editing tools, but opening Photo does not constitute
Library/Develop parity for those tools. Do not mark these gaps complete without
working controls, persistence, and end-to-end validation.

References: [Adobe's Develop workflow](https://helpx.adobe.com/lightroom-classic/desktop/process-and-develop-photos/develop-module-tools.html)
and [Classic ratings, flags, and labels](https://helpx.adobe.com/lightroom-classic/desktop/organize-photos-in-lightroom-classic/flag-label-rate-photos.html).

## MCP coverage

Library tools use the live desktop workspace attached to an open document's
assistant relay. They share the visible selection, catalog, RAW drafts, sidecars,
preview renderer and export queue. Offline document hosts explicitly reject
Library requests rather than silently operating on a different catalog/session.

| Library / Develop operation | MCP integration |
| --- | --- |
| Inspect photos, selection, EXIF, collections, histogram, save/export status | `get_library` (paged, up to 200 visible items) |
| Import a local folder | `import_library` (nonrecursive; original files stay in place) |
| Search, filter, filename/capture-time sort, grid/list/Develop/before/compare, inspector and recipe | `set_library_view` |
| Select, multiselect, navigate active filmstrip photo | `select_library_photos` with explicit canonical paths |
| Ratings, pick/unflag/reject, color labels, replace/add keywords | `edit_library_metadata` |
| Create a collection and add photos | `library_collection` |
| Every RAW parameter, Auto, reset, as-shot WB, undo, reload, save | `develop_library` |
| Built-in presets, portable preset files, grouped synchronization | `develop_library` (`preset`, `save_preset`, `load_preset`, `sync`) |
| Inspect rendered edits and before/after images | `get_library_preview` (PNG content blocks) |
| Export selection and stop export | `export_library`, `cancel_library_export` |
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
slider uses the opposite sign. Temperature/tint remain relative offsets. New
controls and `smooth_curve` are exposed in both Photo and Library tool schemas.
This coverage describes implemented features, not the outstanding Classic parity
items above. Starting a relay from Library without any open document remains a
separate assistant-host integration task.
