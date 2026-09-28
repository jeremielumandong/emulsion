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
share the native physical sheet compositor (single, contact, repeat, poster;
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

Tests cover strict parsing, device-free native print rendering, crop/replacement
source preservation and Undo, actual crop dialog validation, and native editor
state/guide/quick-mask history. No physical print job is submitted by tests.

## Live Smart Object sources

Nested source-editor tabs and persistent local external links are now available through the native source controls and nine matching MCP tools. See [Smart Object source editing and links](smart-object-sources.md) for Apply/Undo, stale-source protection, bounded background refresh and explicit file-write semantics. Replacement/restoration tools remain available for their existing workflows.
