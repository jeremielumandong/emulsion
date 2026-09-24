# RAW tools over MCP

These tools operate on the active editable RAW document. Open a supported RAW
photo in Emulsion first, and use the app-connected MCP server. Restart/reconnect
an existing assistant session after updating so it discovers the new catalog.

| Tool | Capability |
| --- | --- |
| `describe_raw` | Camera, sensor/compression metadata, decoder, original path/fingerprint, development settings and curve presets |
| `develop_raw` | Patch exposure, temperature/tint, highlights, shadow lift, black point, brightness, contrast, saturation, five-point curve, sampled WB gains; omitted fields stay unchanged |
| `auto_develop_raw` | Deterministic auto tone, keeping white balance |
| `pick_raw_white_balance` | Sample a neutral patch using oriented/cropped source-raster `x`, `y` coordinates |
| `reset_raw` | Reset `all`, `white_balance`, `tone`, or `curve` |
| `raw_settings` | Save/load sidecars and presets; save/apply/reset camera-model defaults |
| `relink_raw` | Reconnect an identical original at a new path, verified by SHA-256 |
| `get_raw_preview` | Read-only PNG of edited, split, without-tone, without-curve or clipping output |
| `set_raw_comparison` | Control the live draggable divider or diagnostic view without changing saved edits |
| `list_raw_documents` | Discover open RAW tabs and their session IDs |
| `synchronize_raw` | Apply selected settings groups to explicit destination tab IDs, with separate undo histories |

`export_image` and `batch_export` also accept `bit_depth` (8/16),
`color_space` (`srgb`/`adobe_rgb`), `scale` (`full`/`half`/`quarter`), and `dpi`
(1–1200). Sixteen-bit output requires PNG or TIFF. Resolution metadata requires
PNG/JPEG/TIFF; WebP rejects DPI. Unsupported option/format combinations fail
rather than silently ignoring the requested workflow. Adobe RGB is an export
conversion, not a wider-gamut working document.

## Examples

Tool arguments are JSON; names may have the host's `mcp__emulsion__` prefix.

```json
{"settings":{"exposure":0.75,"temperature":0.15,"saturation":0.1},"curve_preset":"medium"}
```

Pass that to `develop_raw`. Temperature/tint are relative offsets, not Kelvin;
use `describe_raw` and the tool schema for ranges. Curve presets are `linear`,
`medium`, and `strong`; do not combine a preset with explicit `tone_curve`.

For `set_raw_comparison` or `get_raw_preview`:

```json
{"mode":"split","position":0.35}
```

Left is as-shot development; right is the current edited photo. `position` is
0–1. Preview additionally accepts `max_size` (64–1568). Use `mode: "edited"` to
close the live comparison. Other modes are `without_tone`, `without_curve`, and
`clipping`. Clipping shows output clipping, not sensor highlight recoverability.

For `raw_settings`:

```json
{"action":"save_sidecar","path":"C:/Photos/photo.emulsion-raw.json"}
```

Actions: `save_sidecar`, `load_sidecar`, `save_preset`, `load_preset`,
`save_camera_defaults`, `apply_camera_defaults`, `reset_camera_defaults`.
Sidecars may omit `path` to use the original's adjacent settings filename;
presets require it. Camera defaults use Emulsion's model-specific store, not an
arbitrary path. Loading/applying accepts a `group`; saving always saves all
settings. These files are versioned Emulsion JSON, not Adobe XMP.

For normal Save behavior, call `save_document` with `{}`. A directly opened RAW
with only RAW development edits saves `photo.NEF.emulsion-raw.json` beside its
original and marks those edits saved. Reopening the original restores the
sidecar automatically; the NEF itself is unchanged. Added layers, painting,
transforms, saved versions/branches, and other project edits require an explicit
`.ora` path instead.
An existing `.ora` project continues saving to its project path. Sidecars store
the current development settings, not the project's undo/version history.
`raw_settings` remains available for explicitly importing/exporting settings;
that operation alone does not mark the whole document saved.

After `list_raw_documents`, call `synchronize_raw` with chosen IDs:

```json
{"targets":[123,456],"group":"tone"}
```

IDs are session-local, not file indexes. The source is never implicitly included.
Sampled WB gains cannot be copied across camera models. Work is sequential to
bound decoded memory. Busy/stale/closed targets are rejected; the response
reports individual outcomes, so a batch can be partially successful. Undo in
each destination tab to revert its synchronized edit.

For `export_image`:

```json
{"path":"C:/Photos/output.tif","bit_depth":16,"color_space":"adobe_rgb","scale":"full","dpi":300}
```

## Safety and lifecycle

RAW development updates pixels and settings together through normal undoable
commands. In the live assistant, its existing per-turn undo transaction still
groups source edits. Expensive development runs in the background; generation,
operation-ticket and revision checks reject obsolete results. Pending manual RAW
edits must finish before RAW tools or save/export calls proceed. Exports write
the requested document snapshot; later edits do not change that output.

Comparison changes neither recipes nor export pixels. Settings writes occur
only after stale-result validation. Original-image overwrite protection and
camera-bound preset checks use the same IO implementation as the editor.

The MCP exposes currently implemented RAW functionality; it does not add Adobe
XMP interoperability, DNG writing, arbitrary curve knots, or new camera support.
See the [Camera Raw gap report](camera-raw-3-gap.md) for those boundaries.
