# Tutorial: your first painting

## What you'll make

A small layered painting: an ink sketch over a watercolour colour layer, saved
as an OpenRaster (`.ora`) file, exported as PNG, and played back as a replay
GIF.

## Prerequisites

- Emulsion is installed and open on Home.
- Optional: an image to use as a reference.
- Optional: a pen tablet. A mouse also works; stroke speed stands in for
  pressure. On Linux, pen pressure needs membership of the `input` group.

## Steps

1. **Start a canvas.** Choose **Paint** on Home. In the **New document**
   dialog, enter a name and set **Width** and **Height**, for example 2000 by
   1500. Choose **Create**.
   You should see a blank canvas, the Paint tool rail with Brush selected, and
   the **Quick brushes** shelf.

2. **Add a reference.** Open the sidebar's **⋯** menu and choose **Reference**.
   Choose **Attach files** and select an image. Skip this step if you have no
   reference.
   You should see the image preview in the sidebar, marked "reference only".

3. **Add a sketch layer.** Press Ctrl+Shift+N, or choose **Layer → New
   Layer**.
   You should see a new, empty layer selected in the Layers panel.

4. **Choose an ink brush.** Choose **Browse ▾** on the shelf to open the brush
   panel, and select its **Brushes** tab. Set **Library** to **Emulsion** and
   **Set** to **Ink**. Click **G-pen**.
   You should see G-pen highlighted in the list and added to the shelf.

5. **Turn on symmetry (optional).** Select the **Drawing** tab of the brush
   panel. Click **mirror ↔**.
   You should see the chip highlighted; strokes now repeat mirrored left to
   right.

6. **Sketch.** Draw the outline of your subject on the canvas. Press `[` or
   `]` to change the brush size. Click **mirror ↔** again when you no longer
   want symmetry.
   You should see ink lines on the sketch layer, thinner where you draw fast
   or press lightly.

7. **Add a colour layer beneath.** Press Ctrl+Shift+N for another layer. Press
   Ctrl+[ to move it below the sketch.
   You should see the new layer listed under the sketch layer.

8. **Paint with watercolour.** In the **Brushes** tab, set **Set** to
   **Watercolour** and click **Wash**. Pick a colour from the foreground
   swatch, and paint large areas under the ink. Switch to **Detail round** for
   smaller areas.
   You should see soft, transparent colour under the ink lines.

9. **Reuse your colours.** Look at the used-colour palette in the sidebar
   colour group. Click an earlier colour to paint with it again.
   You should see every colour you have painted with, newest first.

## Save and export

1. Press Ctrl+S, or choose **File → Save**. Choose a location and name.
   You should see the file saved as `.ora`, with both layers and your colour
   palette.
2. Choose **File → Export…** and choose **PNG**.
   You should see a flattened PNG of the painting at the chosen location.
3. Open the Layers panel's **Panels ▾** menu and choose **Timeline**. Choose
   **Replay drawing**.
   You should see the painting build up over the canvas, oldest step first.
4. Choose **export GIF** in the replay bar and choose a location. Choose
   **close** when the export finishes.
   You should see the status line report the exported GIF.

## Next steps

- Read the [Paint workspace guide](../paint.md) for drawing guides, QuickShape,
  animation frames, and the full brush catalogue.
- Tune a brush in Brush Studio with
  [Brush Library and Brush Studio](../brush-workflow.md).
- Compare Paint with the other workspaces in
  [Emulsion workspaces compared](../workspaces.md).
