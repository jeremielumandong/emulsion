# Native editor, photo and print workflows

MCP editor controls always address the relay's originating tab. Selecting another
visible tab does not retarget a request. `get_editor_state` reports tool, channel,
quick mask, ruler/snapping preferences, saved guides, selection, workspace layout,
page and revision. `set_editor_state` patches validated fields; null and unknown
fields are rejected. Quick Mask starts with the brush and uses native selection
history. `set_document_guides` replaces up to 1000 finite document-pixel guides
as one Undo. `editor_clipboard` invokes native copy/cut/paste, retaining editable
objects when Emulsion owns the image clipboard. It does not inspect clipboard
text. Active gestures and previews reject mutating host controls.

Design image selection's Object actions menu now exposes image replacement,
source-pixel crop, clipped Curves/Hue–Saturation adjustments, editable blur/sharpen
filters and the complete effects/blending dialog. Background removal is available
for selected raster images when the local matte model is installed. Smart layers
can restore their editable original source. Crop intersects an enabled,
untransformed source-space layer mask; it preserves the original pixels and has
Undo. A transformed or disabled mask requires explicit reset/enable first.
Repeated crop operations intersect the previous crop; Undo restores earlier
coverage. It does not crop the whole design page.

`get_photo_source`, `replace_photo_source` and `crop_photo_source` use those native
models. Replacement decodes local image files on a worker and checks the source
stamp before committing. Displayed bounds, rotation, filters and effects remain;
Smart text/path/SVG sources explicitly become the chosen raster source. Existing
identity masks resize with source dimensions. A transformed mask must first be
reset when replacement dimensions differ. `restore_smart_source` restores the
original editable source and removes its smart filter stack; Undo reverses it.
These replacement/restoration tools edit embedded sources. Nested source tabs and external links use the dedicated Smart source tools described below.

Printing tools discover installed queues and supported paper/media/tray/quality/
duplex choices through the existing platform adapters. Preview and submission
share the native physical sheet compositor (document-size PDF, single, contact, repeat, poster;
fit, fill or actual placement). `preview_print_job` returns a bounded PNG and
layout warnings without submitting. `open_print_dialog` opens the normal setup
and print-preview UI. `submit_print_job` is an explicit print action and returns
OS queue acceptance, not physical completion. The request can include
`expected_revision`; replies identify the immutable originating page/revision.
Device discovery, rendering and submission run off the UI thread. No arbitrary
shell strings, JavaScript or device configuration commands are accepted. The
existing Windows printing adapter and macOS/Linux CUPS backend remain responsible
for platform integration. Linux Flatpak access depends on its native print portal
or available queue permissions; the tool does not escape the sandbox.

Creative options include a physical artwork box (`artwork_width_mm` and
`artwork_height_mm` together), `rows`, `columns`, `gutter_mm`, crop position
(`crop_x_percent`, `crop_y_percent`), `bleed_mm`, and `crop_marks`. Bleed uses
existing off-page artwork, not generated edge pixels. Marks reserve printable
space and single-artwork PDF pages include trim/bleed boxes.

`list_print_presets`, `save_print_preset` and `delete_print_preset` manage named
local layout presets. Save/delete require `preset_name`. Preview/submission can
use that name with explicit setting overrides. Presets exclude destination,
copies, ranges and device-specific options; incompatible printer paper reports an
error. Preset writes are atomic and serialized across processes. The normal UI
also provides preset loading, saving/replacing and deletion.

`preview_print_job`, `submit_print_job` and `export_print_pdf` share additional
source options. Choose one of:

- `photos: [{path, params?, expected_digest?}]`: 1–200 local photos, optionally
  with native DevelopParams snapshots; drafts are rendered without saving them.
- `frame_nodes: [id, ...]`: responsive Design frames on `source_page` (zero-based,
  defaults to 0), preserving each frame's physical size.
- `frame_times_ms: [0, 1000, 2500]`: samples page animation on `source_page`.
- `video_path` plus `frame_times_ms`: extracts local video frames using installed
  FFmpeg, with no network inputs. Up to 100 frames within 24 hours, at most 4096
  pixels per side without upscaling. Out-of-duration frames fail.

Omitting these uses the originating project's pages. `pages` selects from the
prepared sources; frame timestamps preserve requested order and duplicates.
Use `layout: "contact"` and `labels: "name"` or `"number_and_name"` for a labeled
storyboard/contact sheet (`"none"` disables labels).

`export_print_pdf` requires `output_path` and no printer. Writes are atomic and
cannot replace a source photo/video or the ICC profile. Like printer submission,
this is an explicit external action in the MCP tool policy. It accepts the same
physical page layouts as preview. For example:

```json
{
  "output_path": "/path/to/press.pdf",
  "layout": "document",
  "bleed_mm": 3,
  "crop_marks": true,
  "production": {
    "standard": "pdf_x1a2001",
    "profile": "/path/to/press-output-v2.icc",
    "intent": 1,
    "dpi": 300,
    "condition": "Provider-supplied press and paper condition"
  }
}
```

Production `standard` is `pdf`, `pdf_x1a2001`, or `pdf_x32002`. PDF/X enables
conversion implicitly and requires a CMYK ICC v2 output-device profile. Ordinary
PDF can use `managed: true` with RGB/CMYK profiles. Intent 0/1/2/3 means perceptual,
relative, saturation or absolute; `dpi` is 150–600. Managed output flattens the
sheet at that PPI and embeds its output profile. Preview is an sRGB proof
simulation. Native printer output requires an RGB printer profile and explicit
`driver_color_disabled: true` after disabling correction in the driver; the
portal rejects managed output. See [Printing](../printing.md) for production
limits and platform acceptance. Presets retain profile paths and labels but
never the driver-correction acknowledgment.

Tests cover strict parsing, device-free native print rendering, crop/replacement
source preservation and Undo, actual crop dialog validation, and native editor
state/guide/quick-mask history. No physical print job is submitted by tests.

## Live Smart Object sources

Nested source-editor tabs and persistent local external links are now available through the native source controls and nine matching MCP tools. See [Smart Object source editing and links](../smart-object-sources.md) for Apply/Undo, stale-source protection, bounded background refresh and explicit file-write semantics. Replacement/restoration tools remain available for their existing workflows.
