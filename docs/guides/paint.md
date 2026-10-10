# Paint workspace

Paint is Emulsion's workspace for drawing and painting on a blank canvas. It
puts brushes, colour, and paint controls up front, and it saves layered work as
OpenRaster (`.ora`). For a first result, follow the
[first painting tutorial](tutorials/paint-first-painting.md).

## Start and switch

- Choose **Paint** on Home, or choose **File → New painting…**. The **New
  document** dialog asks for a name, width, height, resolution, and background.
  Choose **Create** to open the canvas.
- Choose **File → Open artwork…** to open OpenRaster, PSD, XCF, or image files
  in Paint.
- Press Ctrl+Alt+Shift+D to switch between Paint and Photo on the same
  document. On macOS, Cmd+Alt+Shift+D also works. The document and its history
  stay the same; only the tools and toolbars change.
- Choose **Window → Layout → Paint layout** or **Photo layout** for the same
  switch from the menu. **Minimal layout** hides the options, view, colour,
  and brush toolbars and collapses the sidebar.

Paint and Photo share one editor. Paint shows a shorter tool rail: it has no
Pen, Type, Shape, Clone, Heal, Crop, or Grade tool. Switch to Photo for those
tools, then switch back.

## Paint toolbar

The Paint rail has 12 slots and 21 tools. Slots with more than one tool show
the others in a flyout.

| Slot | Tools | Key |
| --- | --- | --- |
| 1 | Brush, Liquify | B, Ctrl+Shift+X |
| 2 | Smudge | Shift+B |
| 3 | Eraser | E |
| 4 | Eyedropper | I |
| 5 | Line, Rectangle, Ellipse, Polyline | N (press again for the next) |
| 6 | Contour editor, Pencil retouch | A, Shift+A |
| 7 | Paint bucket, Gradient | Shift+G, G |
| 8 | Rectangular marquee, Lasso, Quick select (AI) | M, L, Shift+W |
| 9 | Move | V |
| 10 | Mask | — |
| 11 | Hand, Rotate View | H, R |
| 12 | Zoom | Z |

Press `[` and `]` to make the brush smaller or larger. Click the active tool of
a slot with more than one tool, or right-click the slot, to open its flyout.
Choose **Browse ▾** on the shelf, or **Brush settings** from the sidebar's
**⋯** menu, to open the brush panel.

## Move, resize and rotate artwork

Choose **Move (V)** to transform the selected layer or group. Square handles
resize; the round handle on a short stem rotates. Dragging just outside a corner
also rotates, with a curved-arrow hover cue. Hold **Shift** while rotating to
snap to 15° steps.

A plain click inside already-selected artwork switches between **Resize** and
**Rotate** handles without editing the document. In Rotate mode, the four round
corners also rotate. The Options bar offers the same two modes; narrow toolbars
keep them under **···**. **Edit → Transform → Rotate** selects rotation handles.
A real drag moves the artwork instead of toggling, and each completed gesture
is one Undo step. **Escape** during a drag restores its starting state.

Switching back to Brush restores painting behavior. These controls do not change
brush clicks, strokes, or the separate **Rotate View (R)** tool.

## Brushes, the shelf, and memories

The **Quick brushes** shelf shows four brushes: the current brush first, then
pinned, recent, and current-set brushes. Click a brush on the shelf to paint
with it. Click the star to pin or unpin the current brush. Choose **Browse ▾**
to open the brush panel.

The brush panel (**Brush settings**) has five tabs:

| Tab | Contents |
| --- | --- |
| Brushes | The brush list by library and set, and **Brush library…** |
| Tip | Flow, spacing, roundness, angle, follow path |
| Texture | Grain kind, grain size and strength, wetness |
| Dynamics | Stabilizer, taper, pressure to size and flow, speed thinning, scatter, jitter, tilt, pressure curve, and the pen status |
| Drawing | Symmetry, drawing guide, Drawing Assist, alpha lock, QuickShape |

**Scatter** (Dynamics tab, or Brush Studio's Stroke Path page) offsets each dab
at random by up to that fraction of the brush size. In Brush Studio's Shape
page, **Stamp count** stamps 1 to 16 dabs at each step and **Count jitter**
randomly drops some of them, the conventional stamp randomization. Both are
saved with the brush, and a stroke replays the same dabs every time.

Choose **Brush settings ▾** on the options bar, or right-click the canvas, for
quick size, hardness, opacity, and flow sliders. The same popup holds four
size-and-opacity memories for each brush and painting tool. Choose **Save** to
store a memory, click the memory to recall it, and choose **Clear** to empty
it. The **Paint**, **Smudge**, and **Erase** buttons copy the active brush to
another painting tool.

Choose **Brush library…** in the Brushes tab to organize, import, and export
brushes. Choose **File → Import → Import brushes…** to import brush files
directly. See [Brush Library and Brush Studio](brush-workflow.md) for the
library and [brush files and conversion limits](brush-import-formats.md) for
supported formats.

## Colour and the used-colour palette

Click the foreground colour swatch to pick a colour. Choose the Eyedropper (I)
to sample colour from the canvas.

Emulsion remembers the colours you paint with. A colour joins the palette when
the Brush, Paint bucket, or Gradient tool puts it on a layer. The newest colour
comes first, and the palette keeps the 32 most recent colours. Click a swatch
to paint with that colour again. The palette is saved in the `.ora` file.

The palette appears in the sidebar colour group while the Colors toolbar is
hidden. Choose **Window → Toolbars → Colors** to show it as a toolbar instead.

## Symmetry

Open the **Drawing** tab of the brush panel. Symmetry applies to Brush, Eraser,
and Smudge strokes.

- **mirror ↔** also paints each stroke mirrored left to right.
- **mirror ↕** also paints each stroke mirrored top to bottom.
- **radial** repeats each stroke around the canvas centre. Click it to cycle
  through 4, 6, 8, and 12 copies, then off.

Mirror and radial symmetry combine.

## Paint bucket

Choose the Paint bucket (Shift+G) and click an area to fill it with the
foreground colour, within the selection. The options bar holds:

| Option | What it does |
| --- | --- |
| **tolerance** | How different a colour may be from the clicked one and still fill |
| **close gaps** | Click to cycle **open**, 2, 4, 8, 16 and 32 px. Openings in line art up to that size count as closed, so the fill does not leak through a small gap. The fill still reaches the lines. |
| **normal** | Fill over the area |
| **behind** | Fill under the layer's pixels: the colour shows only where the layer is transparent or translucent |
| **unpainted** | Fill only pixels less opaque than the threshold; click **alpha <** to cycle 25, 50, 75 and 100 % |
| **sample** | **all layers** finds the area on everything visible; **layer** looks only at the current layer |

On a vector stroke layer, the bucket adds a vector fill under the strokes
instead of pixels: the filled area is traced into an outline, so it stays sharp
and editable. Each fill is one Undo step.

## Cut to a layer and distort a selection

With a selection (Rectangular marquee, Lasso, Quick select…), the Select
tool's options bar shows a **layer** group:

- **cut to layer** moves the selected part of the current layer into a new
  layer just above it, at the same position. **copy to layer** copies it
  instead. On a vector stroke layer, strokes are split where they cross the
  selection's edge and fills are clipped, so both layers stay vector. Each is
  one Undo step.
- **perspective** shows the selection's four corners; drag them to distort
  the selected pixels. **envelope 3×3** and **4×4** show a lattice whose
  points bend the selection. Without a selection, they distort the whole layer.
  The canvas shows the result as you drag. Choose **apply distort** to keep it
  as one Undo step, or **cancel**. On a vector stroke layer the stroke points
  move instead. Pixel layers must not be scaled, rotated or flipped, and need
  their mask applied first.

## Drawing guides and Drawing Assist

The guide chip in the **Drawing** tab draws a guide over the canvas. Click it
to cycle through **grid**, **isometric** (30°), **1-point**, **2-point** and
**3-point** perspective, **4-point** and **5-point** curvilinear (fish-eye)
perspective, then off. Grid and isometric spacing is one twelfth of the
shorter canvas side. Drag a vanishing point on the canvas to move it; points
may lie outside the canvas. A curvilinear guide has two handles: its centre,
and its right-hand vanishing point, which sets the radius.

With a guide on, turn on **assist** to snap strokes to the guide. Each stroke
locks to the nearest guide direction once the pointer has moved a few pixels:
horizontal and vertical for a grid, the three isometric axes, or the lines
towards each vanishing point. One- and two-point perspective also keep
vertical lines, and one-point keeps horizontal lines. With a curvilinear guide,
strokes bend along the arc through the left and right vanishing points or the
arc through the top and bottom ones; five-point also offers straight lines
from the centre.

Choose **+ keep** to keep the current guide on the canvas and cycle another
alongside it, for example a grid with a 2-point perspective. **clear guides**
removes them all.

### Guide sets

Choose **save set** to store the guides shown as a named set (**Guides 1**,
**Guides 2**, …). Each saved set appears as a chip; click it to switch to its
guides. **delete set** removes the set you switched to. Guides, sets and the
ruler are saved with the document, so each storyboard panel keeps its own. They
are drawing aids: Undo does not change them.

### Ruler

Click **ruler** to show a straight edge across the canvas, and drag its ends
to place it. A Brush, Eraser or pencil stroke that starts near the ruler
follows its edge, however the hand wanders, on pixel and vector layers. Strokes
that start away from it are free. Click **ruler** again to hide it; it keeps
its place.

## QuickShape

QuickShape is on by default; the **QuickShape** chip in the **Drawing** tab
turns it off and on. Draw a stroke and hold the
pointer still at its end for about half a second. Emulsion replaces the stroke
with the shape it was aiming for: a line, polyline, triangle, quadrilateral,
polygon, circle, or ellipse. The status line names the shape. A stroke that
matches no shape stays as drawn.

## Vector layers

A vector layer keeps pencil lines as editable strokes: each line is a
centreline whose width and opacity change point by point. Choose **Layer →
New Vector Layer**, the pencil button under the layers list, or **New Vector
Layer** in a layer's right-click menu. Each drawing tool below is one Undo
step per stroke or drag. A locked layer, a layer with locked pixels, or a
locked storyboard panel refuses the edit and says why on the status line.

- **Brush** on a vector layer draws a vector line through the same brush
  input as pixel strokes: stabilizer, pressure curve, tapers, speed and
  QuickShape (hold still to snap the line to a clean shape). The brush's
  **pressure → size** setting narrows the line where you press lightly. The
  options bar adds **vector opacity**: **pressure**, **tilt** and **speed**
  make the line fainter for a light touch, a tilted pen or a fast stroke, and
  **fade** fades the line out over that many pixels.
- **Eraser** cuts vector lines where it passes, even between two points, and
  leaves the rest as separate lines. Smudge, Gradient and Liquify work on
  pixel layers only; choose **Rasterize Layer** first.
- **Line**, **Rectangle**, **Ellipse** and **Polyline** (N) draw editable
  strokes on a vector layer and brush pixels on any other layer. Hold Shift to
  keep lines at 45° steps and boxes square. For a polyline, click each corner,
  then press Enter or double-click; click the first corner to close it. Set
  the line **width** on the options bar.
- **Contour editor** (A) selects lines: click a line (Shift-click adds or
  removes), or drag a marquee. Drag a selected line to move it (Shift keeps the
  move straight). Drag a corner of the box to scale (Shift frees the aspect),
  an edge to stretch, or just outside a corner to rotate (Shift snaps to 15°).
  Drag a point of a selected line to reshape it. Press Delete to delete the
  selection. **Smooth** and **Optimize** smooth the lines or remove points that
  change nothing, by **strength** and **tolerance**; **Lines → shapes**
  converts pencil lines into filled brush shapes. They work on the selection,
  or on the whole layer when nothing is selected.
- **Pencil retouch** (Shift+A) brushes over lines to make them **thicker**,
  **thinner**, **opaquer**, **fainter** or **smooth**, with a **size** and an
  **amount**.

Vector layers save in `.ora` and `.emu` files. **Rasterize Layer** turns one
into pixels.

The ruler and Drawing Assist guides hold vector Brush strokes as they do pixel
strokes. Mirror and radial symmetry apply to pixel strokes only.

## Reference images

Choose **Reference** from the sidebar's **⋯** menu. Choose **Attach files**,
**Paste**, or **Attach folder** to add a reference. Its preview stays beside the
canvas, marked "reference only". Choose **hide** or **show** to collapse the
preview, and **Remove** to detach it. References are not part of the artwork or
its exports. See [Drawing from a reference](../../README.md#drawing-from-a-reference)
for using a reference with the assistant.

## Animation frames and onion skin

Open the **Timeline** panel from the **Panels ▾** menu in the Layers panel. The
**Animation** strip treats each top-level layer or group as one frame.

- Choose **play** or **stop** to run the animation, and ◀ or ▶ to step.
- Click the frame rate to cycle through 4, 8, 12, and 24 fps.
- Turn on **onion skin** to fade in the previous frame while you draw.
- Choose **export GIF** to write every frame as an animated GIF.

The preview never changes the document. Exported frames fit within 800 px on
the longest side.

## Replay drawing

Replay plays the painting back from its history, so nothing needs to be
recorded in advance. It uses the file's saved versions and the steps still
available in Undo, oldest first.

1. Open the **Timeline** panel.
2. Choose **Replay drawing**. The replay opens over the canvas and plays at
   6 frames per second.
3. Choose **pause**, ◀, or ▶ to inspect a moment.
4. Choose **export GIF** in the replay bar, or **Export replay GIF** in the
   panel, to save the replay.
5. Choose **close** to return to the canvas.

A long history is thinned evenly to at most 240 moments, keeping the first and
last. Replay needs at least two moments, so draw or edit first.

## Pen tablets

Emulsion reads pen pressure and tilt on all three desktop platforms:

- **Windows:** Windows Ink pointer messages.
- **macOS:** AppKit tablet events.
- **Linux:** evdev devices under `/dev/input/event*`. Most distributions
  require membership of the `input` group to read them.

The pen status appears in the **Dynamics** tab and in Brush Studio's
**Stylus** group. It names the pen in use, reports "pen found but unreadable"
when Linux permissions block a tablet, or says that speed stands in for
pressure.

A mouse or trackpad still paints. Without pen pressure, stroke speed stands in
for pressure. Adjust this with **speed thins** in the Dynamics tab, or
**Mouse speed pressure** in Brush Studio.

## Built-in brushes

Emulsion ships 55 brushes in the **Emulsion** library, one set per category.
Your own brushes go in the **My brushes** library.

| Category | Count | Brushes |
| --- | --- | --- |
| Manga | 15 | Maru pen, Kabura pen, Milli pen 0.3, Milli pen 0.8, Fude brush, Speed lines, Screentone 20%, Screentone 40%, Screentone 60%, Hatching, Cross hatch, Blue pencil, Sketch pencil, White ink, Ink wash |
| Ink | 5 | Fine liner, G-pen, Brush pen, Technical pen, Dry ink |
| Pencil | 5 | HB pencil, 2B soft, 6B side, Mechanical, Coloured pencil |
| Chalk | 5 | Chalk, Charcoal stick, Conté, Pastel, Woven roller |
| Marker | 5 | Chisel marker, Soft marker, Highlighter, Fine marker, Moving paper marker |
| Watercolour | 4 | Wash, Wet blend, Dry brush, Detail round |
| Oil | 6 | Flat bristle, Round oil, Impasto, Acrylic, Fan blender, Turning petals |
| Airbrush | 4 | Airbrush, Fine spray, Speckle spray, Scattered pigment |
| Eraser | 3 | Hard eraser, Soft eraser, Kneaded eraser |
| Smudge | 3 | Blend, Smear, Bristle smudge |

To make a variant, duplicate a brush in the Brush Library and tune the copy in
Brush Studio. The
[Brush Studio settings reference](brush-workflow.md#brush-studio-settings-reference)
lists every setting.

## GPU brushes

Final brush composition runs on the CPU by default. The environment variable
`EMULSION_GPU_BRUSHES` opts in to experimental GPU brush paths: `1` for GPU
final composition, or `persistent` for the persistent GPU brush backend. Both
are ignored when GPU compute is unavailable. Advanced and dual brushes bypass
unsupported GPU paths and render on the CPU. See [GPU rendering](../technical/gpu-rendering.md) and
[files and environment](files-and-environment.md).

## Save and export

- Choose **File → Save** (Ctrl+S) to save the layers and the used-colour
  palette as `.ora`.
- Choose **File → Export…** to write a flattened PNG, JPEG, or other format.

## Related

- [First painting tutorial](tutorials/paint-first-painting.md)
- [Brush Library and Brush Studio](brush-workflow.md)
- [Brush files and conversion limits](brush-import-formats.md)
- [Brush MCP guide](mcp/brush-mcp.md)
- [Moving artwork](artwork-movement.md) and [aligning artwork](artwork-alignment.md)
- [Troubleshooting](troubleshooting.md)
