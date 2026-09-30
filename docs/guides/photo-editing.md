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

Other keys that work while the canvas has focus:

| Key | Action |
| --- | --- |
| Q | Toggle Quick Mask |
| D / X | Default colours / swap colours |
| [ and ] | Brush smaller / larger |
| Shift+[ and Shift+] | Brush softer / harder |
| 1 … 9, 0 | Opacity 10% … 90%, 100% (tool opacity with a painting tool, layer opacity otherwise) |
| Ctrl+Alt+Shift+D | Switch between Photo and Paint |

The full list is in [Editor controls](../../README.md#editor-controls).

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

**Clipping.** Ctrl+Alt+G, or **Layer → Create Clipping Mask**, clips a layer to
the one below it.

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

**Image → Blend space** chooses where blending runs: **Photoshop / sRGB** or
**Linear light**.

Layer shortcuts: Ctrl+Shift+N new layer, Ctrl+J duplicate, Ctrl+G group,
Ctrl+Shift+G ungroup, Ctrl+E merge, Ctrl+Shift+E merge visible, Alt+[ and Alt+]
select the layer below or above.

## Enhance panel

**Enhance** in the panel tabs gathers one-click photo tools, similar to the
AI tools in Luminar. Open the tab, click a tool, and move its sliders
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
**Warp**. Warp places a 3×3 lattice over one pixel layer. It does not work on a
group, a mask or several layers at once; rasterize a smart layer before warping
it. **Apply warp** and **Cancel warp** appear in the contextual bar.

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
