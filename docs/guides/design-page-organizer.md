# Organize Design pages

Choose **Pages** beside the thumbnail strip to see the whole design in a
scrollable grid. Page previews keep their original proportions; each card shows
its name, dimensions and whether it is the page currently being edited.

## Select and open pages

- Click a card to select one page. Click its circle/checkmark, or Ctrl-click
  (Command-click on macOS), to add or remove it from the selection.
- Shift-click selects a range from the last selection anchor. Combine it with
  Ctrl/Command to add a range without clearing the other selected pages.
- **Select all** and **Clear** change only the page selection.
- Arrow keys move the selection; Shift+arrows extend it. Space toggles the
  focused page. Enter or a double-click opens that page on the canvas.
- **Back to canvas** or Escape closes the grid without changing page content.

## Reorder, duplicate and delete

Drag a selected card before another card to move the selected pages together.
Selected pages keep their document order, even when selected in a different
order. Drop on **Drop here to move to the end** after the last row to append the
group. Dragging an unselected page moves only that page. Escape cancels a drag;
dropping outside a target does nothing.

**Move earlier** and **Move later**, or Alt+Left and Alt+Right, move the selection
past the neighboring page. Reordering retains the current editing page's ID.

**Duplicate** (Ctrl/Command+D) inserts copies as a group after the last selected
original. The copy of the current page becomes active when that page was selected;
otherwise the first copy becomes active. Dimensions, artwork, bleed, brand data,
styles and saved graph history are preserved. Links among copied pages target
their copies; links to pages outside the group keep their original targets.

**Delete** removes the selected pages together. If the editing page is removed,
the next surviving page becomes active, or the previous one if none follows it.
Deleting every page is blocked: keep at least one page or close the document tab.
Each duplicate, delete or reorder operation is a single Undo/Redo step. Deleted
page content is retained for Undo.

## Export selected pages

Use **PNG ZIP · N selected** or **PDF · N selected** in the grid. The header's
**Export N selected** menu also supports JPEG and SVG page archives. The count
shows the actual selection; nothing exports when it is empty. **Include bleed**
controls whether the page bleed is included.

Exports follow the document's order, never the order in which cards were
selected. PNG/JPEG/SVG produce ZIP archives; filenames retain the original page
positions. PDF produces a single multipage file. The exporter captures the pages
and their content when the file chooser opens, so later edits do not silently
change that export. Canceling the chooser writes nothing. Export does not change
the editable `.emu` project; save the project normally to preserve its new order.
