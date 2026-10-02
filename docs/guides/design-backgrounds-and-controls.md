# Page backgrounds and direct Design controls

Design keeps the page background available above the canvas, even when artwork
covers the whole page. These controls are separate from the selected object.
Photo keeps its existing detailed workspace.

## Page color

Choose **Page background**, then **Choose color**. The canvas previews changes
immediately. **Done** saves one undoable edit; **Cancel** or Escape restores the
original. A page color sits behind its background photo, so remove that photo
if you want the color to show through everywhere.

## Background photos

- Select an ordinary image and choose **Set as background**, or use
  **Photo → Choose background photo…** to choose a local file
- The photo covers the page. Drag to reposition it; scroll or use +/− to zoom
- **Done** saves the result; **Cancel** keeps both the page and selected image
  unchanged
- Use **Photo → Replace background photo…** for a new source, or
  **Crop / reposition background** to adjust the existing photo
- **Remove background photo** removes only the page's background image and its
  clipping frame. The page color and other objects remain

The source pixels remain embedded at their original resolution. Crop and zoom
change placement rather than resampling the source. The background remains
behind ordinary objects, including after Send to back. Page duplication,
undo/redo, native saves, and exports all use the same document objects.

Images with masks, effects, group membership, links, or locks may need those
settings resolved before they can become a background; the editor does not
silently discard their appearance. Backgrounds are not selected by clicking the
canvas. Use their dedicated controls.

## Style and arrange objects

Select an object to see its type, name, and relevant controls above the canvas.
Text exposes the font picker, size, color, and spacing. Shapes expose fill and
border controls. Images expose replace, crop, and opacity. Advanced effects
remain available separately.

**Arrange** exposes Forward, Backward, To front, To back, Group, Ungroup, Lock,
and Unlock. Shift-click to select multiple objects. Locked objects can be
unlocked through the same Design controls without opening Layers. At narrow
window widths the page controls stay accessible while object controls can scroll.

Layers and the full tool workspace remain available for advanced editing, but
are not required for this create, style, arrange, and export workflow.

## Export notes

Background photo frames use the native compositor when SVG/PDF cannot represent
their clipping and transparency exactly. The exporter reports these rendered
pages; the saved project still retains editable text, vectors, and full-resolution
photo sources. PNG and the rendered PDF appearance use the same page compositor.
Background photos stop at the trim edge; enabling bleed currently extends the
page color rather than automatically extending the photograph into bleed.
