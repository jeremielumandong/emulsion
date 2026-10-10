# Photo editing: tools, layers and effects

Photo is the layered workspace for retouching, compositing and grading an
image. It holds pixel, text, path, smart, fill and adjustment layers, masks,
editable filters and layer styles, and it saves them as an OpenRaster (`.ora`)
file.

For a guided first edit, follow the
[first photo edit tutorial](tutorials/photo-first-edit.md).

## RAW files go through Library first

Opening a camera RAW file opens it in Library Develop, not in Photo. Library
owns the RAW recipe, its sidecar and its history. See
[Library and Develop](library-develop.md).

To bring a developed photo into Photo, choose **Edit in Photo…** in Develop.
The button is available once Library has saved the current edits. Photo
receives the developed pixels in a new document. Later Library edits do not
change that document; choose **Edit in Photo…** again to make a new version.

Older Photo projects that embed a RAW recipe show **RAW development is in
Library** with a **Develop in Library…** button. It creates an independent
Library copy and leaves the Photo layers unchanged.

JPEG, PNG, TIFF, WebP, PSD, XCF and the other formats in
[Opening files](../../README.md#opening-files) open directly in Photo. Use
**File → Open images…** in Photo, or **Open…** on Home.

## The toolbar

The left rail has 14 tools. Several rail slots hold a flyout group; press
Shift with the tool letter to step through the group. Settings lists and
rebinds every shortcut.

| Tool | Key | Flyout entries | What it does |
| --- | --- | --- | --- |
| Move | V | — | Drags a layer or group. Arrows nudge 1 px; Shift+arrows nudge 10 px. |
| Select | M, L, W | Rectangular marquee (M), Elliptical marquee (Shift+M), Lasso (L), Polygonal lasso (Shift+L), Magnetic lasso (Alt+L), Quick select (AI) (Shift+W), Magic wand (W) | Makes a selection. See [Selections](#selections). |
| Crop | C | — | Drags a crop frame; Enter applies it. |
| Eyedropper | I | — | Picks the foreground colour; Alt-click picks the background colour. |
| Heal | J | Heal (J), Remove (Shift+J) | Paints over a blemish to blend it away, or removes an object. |
| Brush | B, E, G | Brush (B), Eraser (E), Gradient (G), Paint bucket (Shift+G), Smudge (Shift+B), Liquify (Ctrl+Shift+X) | Paints, erases, fills, blends or pushes pixels. The kind appears in the bar above the canvas. |
| Clone stamp | S | — | Alt-click a source, then paint copies of it. |
| Pen | P | Pen (P), Free Pen (Shift+P), Curvature Pen (Shift+P), Add Anchor Point, Delete Anchor Point, Convert Point | Draws editable vector paths. |
| Type | T | Type Tool (T), Vertical Type Tool (Shift+T) | Click to add editable text. |
| Shape | U | Rectangle (U), Ellipse (Shift+U) | Drags a rectangle or ellipse. Hold Shift for a square or circle. |
| Mask | — | — | Paints what shows on the selected layer: reveal or hide. It has no default key. |
| Grade | Shift+Q | — | Adds colour and tone adjustment layers. See [Adjustments](#adjustments). |
| Hand | H | Hand (H), Rotate View (R) | Pans the view. Space+drag pans from any tool. |
| Zoom | Z | — | Click zooms in; Shift- or Alt-click zooms out; double-click shows 100%. |

The rail shows these tools in 19 slots with 34 entries. Brush, Eraser,
Gradient, Paint bucket, Smudge and Liquify are all kinds of the Brush tool.

In Photo, **M, L, W, G, J, P, T and U** recall the last-used member of
their group. Pressing the same bare letter again keeps that tool; **Shift**
with the letter continues to cycle. The initial defaults and existing
**Shift+B** Brush/Smudge shortcut are unchanged. **Alt-click** (Option-click
on macOS) cycles a rail group without opening its flyout; Pen anchor-editing
tools are excluded. Right-click or the corner mark opens the full flyout.
Paint, Design and Storyboard retain their existing shortcut behavior.

Other keys that work while the canvas has focus:

| Key | Action |
| --- | --- |
| Q | Toggle Quick Mask |
| D / X | Default colours / swap colours |
| [ and ] | Brush smaller / larger |
| Shift+[ and Shift+] | Brush softer / harder |
| 1 … 9, 0 | Opacity 10% … 90%, 100% (tool opacity with a painting tool, layer opacity otherwise) |
| Two numbers quickly, such as 4 then 5 | Photo painting-tool opacity 45% |
| Shift+number, or two Shift+numbers quickly | Photo brush flow, using the same percentages |
| Ctrl+Alt+Shift+D | Switch between Photo and Paint |

The full list is in [Editor controls](../../README.md#editor-controls).

Photo combines two numeric shortcut presses less than one second apart.
Changing tools, focusing a text field, clicking a control or canvas, or using
another shortcut ends the sequence. **0** means 100%. The brush engine's
supported opacity and flow range is **1–100%**, so **00** selects 1%, not
zero-strength painting. Heal and Remove do not change layer opacity when
number keys are pressed. Layer opacity keeps its existing single-digit,
undoable behavior.

Click a Photo **Size**, **Hardness**, **Opacity** or **Flow** value to type an
exact number. **Enter** applies it; **Escape**, clicking elsewhere or changing
tools discards the unfinished value. Supported values are **1–1000 px** for
Size, **0–100%** for Hardness and **1–100%** for Opacity/Flow. Photo’s bracket
shortcuts use the same size limit. Tool settings do
not create document history steps; subsequent strokes remain undoable.

On a Photo pixel layer with **Lock transparent pixels** enabled, Eraser blends
toward the captured background colour while keeping the existing transparency.
Opacity, flow and the selection still control its strength. Eraser on a mask
continues to hide coverage; Eraser in Quick Mask restores selected coverage.

### Alignment and layer controls

Select **Move (V)** for **Align & distribute** in the Options bar. Its menu
keeps the Canvas, pixel Selection and Selected layers targets, including the
existing distribution commands. At narrow widths, use the Options bar's
**More** button. Alignment edits still use ordinary Undo and Redo.

Photo's Properties panel no longer repeats an Align disclosure or the layer
blend mode, Opacity and Fill controls: those blending controls live in Layers.
Properties retains mask controls, Quick actions, live adjustment parameters
and smart-filter controls. The labelled **Flip horizontal** and **Flip vertical**
buttons remain available there while painting; Move exposes numeric geometry.
Paint keeps its existing inspector controls.

## Selections

The Select tool has seven modes. Choose them in the rail flyouts or in the
options bar above the canvas.

| Mode | Use it for |
| --- | --- |
| Rectangle | Rectangular areas. |
| Ellipse | Elliptical areas. |
| Lasso | Freehand outlines. |
| Polygon | Straight-edged outlines; click each corner. |
| Magnetic | Click anchors; the outline snaps to edges between them. |
| Wand | Areas of similar colour, with **tolerance** and **contiguous**. |
| Quick | Brush over an area; the selection grows through similar colour. Turn on **AI** to click an object or drag a box around it instead. |

The options bar combines each new shape with the current selection: **new**,
**add**, **subtract** or **intersect**. It also offers **invert**, **grow 5**,
**shrink 5**, **soften 10**, **from layer**, **content-aware fill** and **AI fill**.

**Select subject** selects the main subject with a local AI model. It appears
in the contextual bar for the Move and Select tools, and as **subject (AI)** in
the Select options. After an AI selection, the **refine** sliders (**in above**,
**out below**, **grow**, **feather**) tune its edge; choose **done** to keep it.

**Select → Edit in Quick Mask Mode** (Q) shows the selection as a red overlay
that you paint with the Brush. Press Q again to return to a selection.

With a selection active:

- **Create layer mask** in the contextual bar turns it into a mask on the
  selected layer.
- Any new adjustment layer uses the selection as its mask. Deselecting later
  does not widen the adjustment.
- **Generative fill** and the **generate** field fill the selection with a new
  layer from the image provider set in Settings. See
  [Image generation](../../README.md#image-generation).

Ctrl+D deselects, Ctrl+Shift+D reselects, and Ctrl+Shift+I inverts.

## Layers

The Layers panel lists every layer. Its footer adds a new layer, a group, a
layer mask, a layer style (**fx**) and a fill or adjustment layer.

| Layer kind | Holds |
| --- | --- |
| Pixel | Painted or opened pixels. |
| Text | Editable text from the Type tool. |
| Path | Editable vector paths and shapes from the Pen and Shape tools. |
| Group | Other layers. Groups can use Pass Through blending. |
| Adjustment | One of the 18 adjustments; it changes the layers below. A LUT is an adjustment layer. |
| Fill | A solid colour (**Solid Color…** in the fill or adjustment menu). |
| Smart | A protected source plus editable filters. See [smart object sources](smart-object-sources.md). |

Right-click a layer for **Convert to Smart Object**, **Convert to Layers (remove
Smart filters)** and **Rasterize**.

**Masks.** Add a mask with the footer button; Alt-click it for an inverted mask.
The Mask tool paints on the mask: **reveal** shows the layer, **hide** hides it.
**Layer → Layer Mask** can disable, invert, apply, link or delete the mask.

Photo's mask controls keep **Density (0–100%)** and **Feather (0–1000 px)** as
editable properties instead of overwriting the stored mask. Density 0% reveals
the layer; 100% uses the full mask strength. Feather is measured in intrinsic
mask pixels before its transform, so scaling the mask scales its visible feather.
Enter applies a typed value, Escape cancels a draft, and a slider drag is one
Undo step. Save/reopen retains the original mask and both settings. Replacing
a raster mask resets its geometry and these properties, while preserving the
layer's existing linked/unlinked choice.

**Vector masks.** **Layer → Vector Mask** adds Reveal All or Hide All, starts a
new mask path, or edits an existing one with the Pen tools. These paths belong
to the mask; they do not create visible artwork layers. Raster and vector masks
can coexist, and their enabled coverage multiplies. Select the appropriate
thumbnail before editing; Properties and the contextual controls identify the
active component. Open subpaths are implicitly closed for coverage while their
editable geometry remains open.

Vector masks support Density, Feather, inversion, enable/disable, linking,
selection loading, affine Free Transform, and native save/history. **Rasterize
Vector Mask** converts the component to a raster mask only when no raster mask
already exists; it does not flatten the layer's pixels. Rasterization fixes a
sampling grid, so later scaling can change edge antialiasing. Extreme path,
feather, or transform combinations that exceed the bounded rendering budget
are rejected with an explanation before changing the document. See
[native vector-mask details](../technical/native-vector-masks.md).

Keyboard Delete and Backspace follow the active mask target and never fall
through to deleting its artwork. Use the explicit **Delete Layer** command to
remove a layer. Mask-pixel clipboard deletion is not implemented; use the mask
painting and mask-management controls.

Native projects without newer mask features retain format 9. Persistent raster
mask properties or independent raw mask grids require format 10; vector masks
require format 11; Smart Filter stack masks require format 12. Saved history
can raise the required version even after the live feature is removed. Older readers reject unsupported versions. Native
files retain editability; flattened exports retain appearance. PSD and generic
layered-format fallbacks do not promise complete editable mask-feature roundtrips.
PSD/PSB now retains raster-mask pixels outside the layer and canvas, independent
mask bounds, link/enable state, Density and Feather when the mask grid needs only
whole-pixel translation. Density uses PSD's 8-bit precision. Unsupported mask
transforms are baked with an export warning; vector and Smart Filter masks still
use an appearance fallback. Feather values remain editable, but their rendering
can differ between applications. Keep a native copy for complete editability.
See [PSD raster-mask interchange](../technical/psd-raster-masks.md).

**Clipping.** Ctrl+Alt+G, or **Layer → Create Clipping Mask**, clips a layer to
the one below it. In Photo, releasing a clipping mask releases the selected
clipped layer and the contiguous clipped layers above it that share its base,
as one Undo step. Locks and group boundaries are respected. Paint and Design
retain their existing release behavior.

**Blend modes.** Photo has 28 blend modes: 27 for any layer plus Pass Through
for groups. Shift+= and Shift+- step through them. Shift+Alt+letter picks a mode
directly when the canvas or Layers panel has focus.

| Group | Modes (Shift+Alt key) |
| --- | --- |
| Normal | Normal (N), Dissolve (I) |
| Darken | Darken (K), Multiply (M), Color burn (B), Linear burn (A), Darker color |
| Lighten | Lighten (G), Screen (S), Color dodge (D), Linear dodge (W), Lighter color |
| Contrast | Overlay (O), Soft light (F), Hard light (H), Vivid light (V), Linear light (J), Pin light (Z), Hard mix (L) |
| Comparative | Difference (E), Exclusion (X), Subtract, Divide |
| Component | Hue (U), Saturation (T), Color (C), Luminosity (Y) |
| Groups only | Pass through |

**Image → Blend space** chooses where blending runs: **PSD-compatible sRGB** or
**Linear light**.

Layer Style's **Blend If** ranges include direct joined handles. Alt/Option-drag
splits a half; the numerical controls remain available for precision. Limits
stay ordered, Cancel restores the previous settings, and an unchanged click
preserves an imported fractional bound. Finish or cancel the Layer Style edit
before Undo, Redo, or a history jump. Closing the dialog restores editor
keyboard focus.

Layer shortcuts: Ctrl+Shift+N new layer, Ctrl+J duplicate, Ctrl+G group,
Ctrl+Shift+G ungroup, Ctrl+E merge, Ctrl+Shift+E merge visible, Alt+[ and Alt+]
select the layer below or above.

## Enhance panel

**Enhance** in the panel tabs gathers one-click photo tools, similar to the
AI tools in other photo editors. Open the tab, click a tool, and move its sliders
right there in the panel. Tools marked ● are on.

Enhance works on the selected pixel or smart layer. If none is selected, it
uses the topmost visible one, which is usually the photo itself. The layer's
name appears at the top of the panel.

Filter tools add an editable filter to that layer (see [Filters](#filters)).
You can tune them later in the panel or in **Properties**, or delete them
with **Remove**. The tools that use local models (sky replacement, relight,
depth fog, portrait bokeh) add new layers with masks. Everything Enhance adds
can be undone.

| Section | Tool | What it does |
| --- | --- | --- |
| Looks | Vivid, Landscape, Soft portrait, Dreamy, Golden, Moody | Each one adds a set of the filters below with chosen settings, as a single undo step. |
| Essentials | Enhance | Balances light, colour and depth from the photo's own statistics. **Sky** deepens blue skies. |
| Essentials | Structure | Adds local contrast that respects detail. Negative values soften. |
| Essentials | Denoise, Sharpen, Film grain | Reduce noise, Smart sharpen and Add noise. |
| Creative | Glow, Mystical, Sunrays, Golden hour, Dramatic | Bloom, the Orton look, light rays from a sun you place with **X** and **Y**, warm low-sun toning, and gritty local contrast with muted colour. |
| Portrait | Skin | Smooths skin tones only. **Detail** brings back texture. |
| Portrait | Face restore | Repairs faces with the face models. |
| Portrait | Portrait bokeh | Blurs a copy of the picture outside the subject. The subject comes from the subject model, or from the depth model if the subject model is not installed. |
| Landscape | Sky replacement | Finds the sky, masks a new sky into it, and adds a **Sky relight** Photo filter layer that tints the foreground toward the new sky's colour. Choose a built-in sky (Clear blue, Fair clouds, Golden hour, Sunset, Blue hour, Stormy) or **Your image…**. |
| Landscape | Atmosphere | Adds haze. Below zero it removes haze. |
| Landscape | Relight | Adds two Exposure layers with depth masks: **near** brightens the foreground and **far** darkens the background. |
| Landscape | Depth fog | Adds a fill layer masked by distance, so the fog gets thicker further away. |
| Erase and expand | Remove objects, Remove background, Upscale | The Remove tool, Remove background and AI upscale. |
| Erase and expand | Expand | Enlarges the canvas by 15% of the shorter side on every side, then fills the new edges with AI fill. If the inpainting model is not installed, it uses content-aware fill. |

Model-backed tools need their models, installed under **Settings › Local
models**: sky segmentation, depth, subject matte, face detection and
restore, inpainting, and upscale. Without the model, the tool names the one
it needs. Sky replacement, relight, depth fog and portrait bokeh work from
the flattened picture as it is when you click **Apply**.

## Adjustments

Adjustments are editable layers. Add one in any of these ways:

- Choose **Grade** (Shift+Q). The options bar offers Exposure, Curves, Color
  balance and Hue / Saturation, and **All adjustments** opens the full list.
- Choose **Image → Adjustments** and pick an entry.
- Use the footer's fill or adjustment button in the Layers panel.
- Press a shortcut: Ctrl+L Levels, Ctrl+M Curves, Ctrl+U Hue / Saturation,
  Ctrl+B Color balance, Ctrl+Alt+Shift+B Black & white, Ctrl+I Invert,
  Ctrl+Shift+U Desaturate (a Hue / Saturation layer at −100 saturation).

The new layer opens in **Properties**. Choosing Grade with an adjustment layer
selected opens that layer's controls.

| Group in the Adjustments panel | Adjustments |
| --- | --- |
| Light | Exposure, Brightness / Contrast, Levels, Curves |
| Colour | White balance, Hue / Saturation, Color balance, Selective color, Vibrance, Photo filter, Black & white, Gradient map |
| Effects | Grain, Vignette, Posterize, Threshold |

**LUT…** at the top of the Adjustments panel loads a `.cube` file as a LUT
layer. Invert is in **Image → Adjustments** and the Layers footer's fill or
adjustment menu.

The Grade contextual bar also has **Auto Tone** (Ctrl+Shift+L), **Auto Contrast**
(Ctrl+Alt+Shift+L) and **Auto Color** (Ctrl+Shift+B). **Check brightness**,
**Check saturation** and **Check color** add a temporary check layer, such as a
Black & white layer for judging brightness.

## Filters

Filters are editable. Applying a filter to a pixel layer makes it a smart
layer, and the filter's settings stay in **Properties**. A smart layer holds up
to 32 filters. Select an unlocked pixel or smart layer before choosing a filter.

### Smart Filter masks

A Smart Object's **Smart Filters** header has one mask for its complete filter
stack. White reveals the filters; black restores the original source; gray mixes
the two. Raster and vector layer masks remain independent and apply afterward.
Applying the first filter creates a white mask, or uses the current selection.
Removing the final filter retains the mask for later reuse.

Click the filter-mask thumbnail to edit, Shift-click to disable or enable it,
or Alt/Option-click for grayscale inspection. Brush, Eraser and Gradient edit
its coverage. The **Filter Mask** controls provide Reveal All, Hide All,
selection loading and replacement, inversion, Density (0–100%),
Feather (0–1000 intrinsic pixels), linking, deletion and re-adding. Linked masks
follow layer placement; unlink to transform the mask independently. Repainting
the mask does not recalculate the filter stack.

Native save/history retains the editable mask. PSD and generic OpenRaster
exports retain rendered appearance rather than editable Smart Filter records.
Selection-derived masks and automatic editing-plane growth are limited to
16 MP; excessive growth is rejected instead of clipped. Target switches cancel
unfinished background filter requests. Per-filter masks, Channels integration,
mask-copy transforms, arbitrary warps and applying filters or adjustments to
the mask itself are unavailable. See
[Smart Filter mask details](../technical/smart-filter-masks.md).

Save finishes an active synchronous Brush, Eraser or mask stroke before
snapshotting, including its pending samples and one Undo step. Finish or cancel
other unfinished edits, modal previews and current background work before
saving; a rejected Save leaves the destination unchanged. Save As checks again
after the file picker closes.

### Available filters

| Filter menu | Filters |
| --- | --- |
| Blur | Gaussian blur, Box blur, Motion blur, Lens blur |
| Sharpen | Unsharp mask, Smart sharpen |
| Noise | Add noise, Reduce noise |
| Distort | Pinch, Twirl, Wave |
| Stylize | Emboss, Find edges |
| Other | High pass |
| Enhance | Enhance, Structure, Atmosphere, Golden hour, Dramatic |
| Creative | Glow, Mystical, Sunrays |
| Portrait | Skin smoothing |
| Top level | Lens correction (Ctrl+Shift+R), Lens profile |

The first Filter menu entry repeats the last filter; Ctrl+Alt+F does the same.
The Adjustments panel also has a **Filters** row, including **Lens profile
(auto)**, which looks up the photo's camera and lens from EXIF. It needs the
lens database, installed under **Settings › Local models**.

## Layer styles

A layer holds up to 12 styles from 10 kinds: Drop shadow, Inner shadow, Outer
glow, Inner glow, Stroke, Color overlay, Gradient overlay, Pattern overlay,
Bevel and emboss, and Satin.

Add a style with **+ style** under **Styles** in Properties, or with the **fx**
button in the Layers footer. **Layer → Layer Effects** has **Blending Options…**,
**Clear Layer Effects**, **Copy Layer Style** and **Paste Layer Style**.

## Retouching

| Tool | How to use it |
| --- | --- |
| Heal (J) | Paint over a blemish. The stroke blends surrounding texture and colour into it. |
| Remove (Shift+J) | Paint over an object. Choose **Remove now**, or turn on **Remove after each stroke**. |
| Clone stamp (S) | Alt-click a source, then paint copies of it. **Reset source** clears the source. |
| Smudge (Shift+B) | Drag the colour already on the layer. |
| Liquify (Ctrl+Shift+X) | Choose **push**, **twirl ↻**, **twirl ↺**, **pinch**, **expand** or **restore**, then brush over the pixels. |

For whole-area removal, select the area and use **content-aware fill**
(Shift+Backspace) or **AI fill**.

## Transform and crop

**Edit** has **Free Transform** (Ctrl+T), **Scale**, **Rotate**, **Distort** and
**Warp**. In Photo, Free Transform is one modal operation: move, scale, rotate,
flip, and enter numeric values, then press **Enter** or **Apply** to commit one
Undo step. **Escape** or **Cancel** restores the entire operation, including any
lifted pixels or provisional copies. Releasing the mouse keeps the session open.
The ordinary Move tool still commits each gesture separately.

With **Move (V)**, square handles resize the selected artwork. Drag the round
handle on a short stem to rotate, or drag just outside a corner. Hovering shows
the resize direction or a rotation indicator. Rotation keeps the artwork's
center fixed; hold **Shift** to snap to 15° steps.

A plain click inside already-selected artwork switches between **Resize** and
**Rotate** handles when you release. In Rotate mode, the four round corner
handles also rotate. You can choose either mode in the Options bar (under **···**
in a narrow window). **Edit → Transform → Rotate** starts Free Transform with
rotation handles; **Scale** starts with resize handles. The mode switch itself
does not move artwork, mark the file changed, or add an Undo step. A drag, a
modified click, and the second click of a text-editing double-click do not switch
modes. Changing the layer, mask component, or tool restores resize handles.

While dragging in ordinary Move, **Escape** cancels that gesture. During modal
Free Transform, **Enter** applies the whole session as one Undo step and
**Escape** restores its starting state. The same Move handles and click-to-switch
interaction are available in Paint and Design; their normal Move gestures keep
their existing per-gesture history behavior.

- **Ctrl+Alt+T** starts **Duplicate and Free Transform**. Enter without moving
  intentionally commits one copy; Escape removes it.
- **Ctrl+Shift+T** applies **Transform Again** to the current artwork.
- **Ctrl+Alt+Shift+T** applies **Transform Again with Copy** and selects the new
  copy. Repeating this advances each copy by the same saved operation.
- macOS also accepts the corresponding Command/Option chords. These defaults
  apply to the Photo canvas and Layers panel. Explicit saved keymap assignments
  take precedence, including a saved Timeline assignment on Ctrl+Alt+T.

Again stores the last non-identity, committed Free Transform as a document-space
matrix with a fixed reference point. It does not store original layer IDs.
Undo/Redo leave that recipe available; reopening or changing pages resets it.
Canceled, invalid, and identity transforms retain the previous recipe. A committed
Warp, Distort, or text-frame reflow clears it. Save, export, page/tab/workspace
changes, and close require applying or canceling the session first.

This preserves editable raster, Smart Object, text, path, and group content.
Linked masks follow artwork; unlinked masks retain document-space placement.
Vector masks support their own modal affine Free Transform. Raster masks use
the ordinary Move mask controls; mask transform-copy and Again commands are
gated. Copying clipped roots, linked ancestor/descendant sets,
projective transforms inside the modal session, and anisotropic stroked paths
are gated. A repeated matrix that would introduce unsupported shear also fails
before creating a copy. These are bounded affine workflows, not a claim of
pixel-identical replay/pivot behavior of other editors.

Warp places a 3×3 lattice over one pixel layer. It does not work on a
group, a mask or several layers at once; rasterize a smart layer before warping
it. **Apply warp** and **Cancel warp** appear in the contextual bar.
When a pixel selection is lifted for Warp or Distort, canceling or applying an
unchanged transform restores the original pixels and selection. These projective
modes retain their legacy two-step history after a successful selected-pixel
transform: the first Undo reverses the resample; the second reverses the lift.
They do not yet have Free Transform's one-step operation-wide Undo workflow.

The Crop tool has eight modes: Free, Fixed ratio, Fixed size, Original ratio,
1:1, 4:3, 3:2 and 16:9. Its behaviour with hidden pixels is in
[Cropping](../../README.md#cropping).

**Image → Image size…** (Ctrl+Alt+I) scales the picture, and **Image → Canvas
size…** (Ctrl+Alt+C) grows or trims the canvas. See
[Image size and canvas size](../../README.md#image-size-and-canvas-size) and
[Rotating objects](../../README.md#rotating-objects).

For moving and aligning layers, see [artwork movement](artwork-movement.md) and
[artwork alignment](artwork-alignment.md).

## Text and paths

The Type tool (T) adds a text layer where you click. Type, then choose **Done**.
The text stays editable. Select the layer, then use the **Character** panel or
the contextual bar's **Toggle bold** and **Toggle italic**.

The Pen tool (P) places path points; drag to make curves. The flyout adds
anchor editing. **Window → Paths** lists the document's paths. Path and text
layers stay vector until you choose **Rasterize**.

## Save, export and print

- **File → Save** (Ctrl+S) writes a single-page document as `.ora`, keeping
  layers, masks, filters and styles.
- **File → Export…** (Ctrl+Alt+Shift+W) writes PNG, JPEG, WebP, TIFF, layered PSD
  or XCF, and more formats. See [Exporting](../../README.md#exporting).
- **File → Print…** (Ctrl+P) is described in [printing](printing.md).

## Related

- [First photo edit tutorial](tutorials/photo-first-edit.md)
- [Workspaces compared](workspaces.md)
- [Library and Develop](library-develop.md)
- [Smart object sources](smart-object-sources.md)
- [Artwork movement](artwork-movement.md) and [artwork alignment](artwork-alignment.md)
- [Printing](printing.md)
- [Opening files](../../README.md#opening-files) and
  [Exporting](../../README.md#exporting)
