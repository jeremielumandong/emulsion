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

The Paint rail has 10 slots and 15 tools. Slots with two or three tools show
the others in a flyout.

| Slot | Tools | Key |
| --- | --- | --- |
| 1 | Brush, Liquify | B, Ctrl+Shift+X |
| 2 | Smudge | Shift+B |
| 3 | Eraser | E |
| 4 | Eyedropper | I |
| 5 | Paint bucket, Gradient | Shift+G, G |
| 6 | Rectangular marquee, Lasso, Quick select (AI) | M, L, Shift+W |
| 7 | Move | V |
| 8 | Mask | — |
| 9 | Hand, Rotate View | H, R |
| 10 | Zoom | Z |

Press `[` and `]` to make the brush smaller or larger. Click the active tool of
a slot with more than one tool, or right-click the slot, to open its flyout.
Choose **Browse ▾** on the shelf, or **Brush settings** from the sidebar's
**⋯** menu, to open the brush panel.

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

## Drawing guides and Drawing Assist

The guide chip in the **Drawing** tab draws a guide over the canvas. Click it
to cycle through **grid**, **isometric** (30°), **1-point**, **2-point**, and
**3-point** perspective, then off. Grid and isometric spacing is one twelfth of
the shorter canvas side. Drag a vanishing point on the canvas to move it; points
may lie outside the canvas.

With a guide on, turn on **assist** to snap strokes to the guide. Each stroke
locks to the nearest guide direction once the pointer has moved a few pixels:
horizontal and vertical for a grid, the three isometric axes, or the lines
towards each vanishing point. One- and two-point perspective also keep
vertical lines, and one-point keeps horizontal lines.

## QuickShape

QuickShape is on by default; the **QuickShape** chip in the **Drawing** tab
turns it off and on. Draw a stroke and hold the
pointer still at its end for about half a second. Emulsion replaces the stroke
with the shape it was aiming for: a line, polyline, triangle, quadrilateral,
polygon, circle, or ellipse. The status line names the shape. A stroke that
matches no shape stays as drawn.

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
