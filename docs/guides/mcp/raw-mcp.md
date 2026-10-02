# RAW tools over MCP

These tools operate on the active editable RAW document. Open a supported RAW
photo in Emulsion first, and use the app-connected MCP server. Restart/reconnect
an existing assistant session after updating so it discovers the new catalog.

| Tool | Capability |
| --- | --- |
| `describe_raw` | Camera, sensor/compression metadata, decoder, original path/fingerprint, development settings and curve presets |
| `develop_raw` | Patch exposure, temperature/tint, highlights, shadow lift, black point, brightness, contrast, saturation, five-point curve, sampled WB gains; omitted fields stay unchanged |
| `auto_develop_raw` | Deterministic auto tone, keeping white balance |
| `analyze_raw` | Read-only photo measurements: tonal percentiles, key, clipping, colour cast, saturation, hue distribution, skin/sky/foliage share, scene tags, suggested white balance and ranked looks |
| `list_raw_looks` | Read-only catalog of mood looks with mood words, descriptions and the preset genre each resembles |
| `apply_raw_look` | Grade with an adaptive look chosen by key, mood words or `auto`; optional auto-balance (`correct`) and `strength` 0–1.5, in one undo step |
| `save_raw_preset` | Save the current look by name to the Develop preset bank and/or export an Adobe Camera Raw `.xmp` for Lightroom; never overwrites unless asked |
| `list_raw_presets` | Read-only list of saved and installed presets |
| `apply_raw_preset` | Apply a saved preset by name or `.xmp`/`.lrtemplate`/`.json` path, with `strength`, in one undo step |
| `mask_raw` | Add/update/remove/clear Lightroom-style local masks from subject, background, sky, face, eyes, teeth, radial, linear, brush, luminance and colour-range parts with add/subtract/intersect/invert |
| `auto_mask_raw` | Masking strategies: subject pop, background recede, sky, vignette focus, directional light, colour range, colour separation, tonal balance, eyes, teeth, or auto |
| `list_raw_masks` | Read-only mask list, with an optional tinted overlay image to check placement |
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

For mood grading, call `analyze_raw`, then `apply_raw_look`:

```json
{"look":"warm moody film","strength":0.8,"harmony":"complementary"}
```

`look` accepts `auto` (chosen from the analysis), a key such as
`portrait-film-warm`, `cinematic-teal-orange`, `bright-airy`, `moody-dark`,
`golden-hour`, `vivid-landscape`, `nordic-cool`, `vintage-faded`,
`rich-slide-film`, `urban-muted`, `night-neon`, `clean-portrait`,
`natural-pro`, `classic-bw` or `noir-bw`, or free mood words. With the default
`correct: true`, exposure, highlight recovery, shadow lift and a measured cast
of near-neutral tones are balanced first; casts are kept for golden-hour and
night scenes. The look is then fitted to the photo with common retouching
rules:

- **Colour harmony** (`harmony`: `auto`, `none`, `complementary`,
  `split_complementary`, `analogous`, `triadic`, `monochromatic`), anchored on
  the subject: skin hue when people are present, otherwise the dominant hue.
  Colours on the scheme gain saturation; colours off it shift up to 25° toward
  the nearest scheme hue and lose up to 40 saturation. The warmest scheme hue
  tones the highlights and the coolest tones the shadows, at conservative
  split-tone strengths. `auto` uses each look's own scheme (for example
  complementary for teal & orange, analogous for golden hour).
- **Skin tones** are protected and, on a clean measurement, steered toward a
  flattering 18–28° hue, with oversaturated or dull skin calmed or enriched and
  dark skin brightened — all in the HSL orange/red bands. Midtone grading stays
  light because it tints every face.
- **Landscapes** get darker, richer blues with aqua pulled toward blue, tamed
  neon yellow-greens, and a graduated filter that darkens the sky by 0.4 stop
  (with `correct: true`).
- **Black & white** looks choose a colour filter by scene through primary
  calibration before the monochrome conversion: orange for skies, green for
  foliage, orange-red for portraits, yellow otherwise.
- **Context:** auto selection keeps dark, moody grades away from portraits,
  and `analyze_raw` reports a `palette_mood` from colour psychology (warm
  reads energetic, cool reads calm, low saturation reads quiet) so the agent
  can match or deliberately shift the feeling.
- Added contrast is halved on contrasty scenes, added saturation is halved on
  already colourful ones, and clarity/dehaze are eased at night. Applying a new
look replaces the previous look's curves, HSL, grading, calibration and
presence settings instead of stacking; geometry, detail, lens and masks stay.
The result is ordinary RAW settings, editable with `develop_raw`. Looks are
Emulsion's own and are described by the genre they resemble, not copied from
any vendor's presets.

When you like an edit, save it as a preset with `save_raw_preset`:

```json
{"name":"Warm Film Portrait","export_xmp":true}
```

The look (tone, presence, curves, HSL, colour grading, calibration, detail
and vignette) goes to Emulsion's preset bank, where it appears in the Develop
panel. `export_xmp: true` also writes `Warm Film Portrait.xmp` to Emulsion's
`exported-presets` data folder; pass a path string instead to choose the
location. Import that file in Lightroom Classic, Lightroom or Adobe Camera Raw
(Presets → Import Presets). Emulsion-only controls such as depth blur and
sensor denoise have no Adobe field and are left out. Crop, geometry, lens,
masks, depth and sampled white balance always stay with the photo; set
`include_exposure` or `include_white_balance` to carry those. Existing presets
and files are never replaced unless `overwrite` is true.

Apply it later with `apply_raw_preset` (`{"name":"Warm Film Portrait"}` or a
`path`), optionally with `strength`. The target photo keeps its own crop,
geometry, lens, masks, exposure and white balance unless the preset carries
them.



```json
{"mode":"split","position":0.35}
```

Left is as-shot development; right is the current edited photo. `position` is
0–1. Preview additionally accepts `max_size` (64–1568). Use `mode: "edited"` to
close the live comparison. Other modes are `without_tone`, `without_curve`, and
`clipping`. Clipping shows output clipping, not sensor highlight recoverability.

### Local masks

`mask_raw` works like Lightroom's masking panel. A mask is a list of parts,
combined in order:

```json
{"action":"add","name":"Moody sky","components":[
  {"shape":"sky"},
  {"shape":"linear","start":[0.5,0.7],"end":[0.5,0.35],"operation":"intersect"}],
 "adjustments":{"highlights":-0.4,"saturation":0.15,"temperature":-0.05}}
```

Parts: `subject`, `background`, `sky` (local AI models), `face`, `eyes`,
`teeth` (face detector), `all`, `radial` (`center`, `radius`, `feather`),
`linear` (no effect at `start`, full at `end`), `brush` (`points`, `size`),
`luminance` (`range` of display brightness) and `color` (`color` as a name,
`#rrggbb` or `[r,g,b]`, with `tolerance`). Each part can `add`, `subtract` or
`intersect`, and `invert` flips the first part or a later subtract/intersect
part. Inside a mask you can adjust `exposure`, `contrast`, `highlights`,
`shadows`, `saturation`, `temperature` and `tint`. Coordinates are 0–1 of the
cropped, rotated photo as `get_view` shows it; pass `"space":"source"` for the
uncropped original. Without a subject or sky model, a labelled approximation
is used: a centred radial for the subject, and a top gradient intersected
with bright tones for the sky. Face, eye and teeth masks need the face
detector.

`auto_mask_raw` applies proven strategies: `subject_pop`,
`background_recede`, `sky`, `vignette_focus`, `directional_light` (warm the
lit side, cool the shadow side on flat light), `color_range` (warm the
yellows, deepen the reds, cool the blues), `color_separation` (warm
foreground, cool distance), `tonal_balance`, `eyes`, `teeth`, or `auto`.
Re-running a strategy replaces only its own masks. `list_raw_masks` with
`"overlay":true` returns the photo with masks tinted, so you can check edges.

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
See [RAW development controls and limits](../raw-development.md) for those boundaries.
