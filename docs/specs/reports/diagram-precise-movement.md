# Diagram precision movement

Modifier selection previously consumed Ctrl/Shift mouse-down before a move could start. Selected-object deselection is now deferred until mouse-up: a click toggles selection, while a drag preserves and moves it. Dragging an unselected object with a selection modifier adds it and moves the selection. Passing the movement threshold clears the pending click even if the pointer later returns to its origin.

Shift movement locks the first deliberate axis for the gesture instead of recalculating the dominant axis on every pointer event. Ctrl bypasses snapping, including when held before mouse-down. Ctrl+Shift combines axis lock and unsnapped, whole-document-pixel translation. Existing arrow nudges remain 1 px, or 10 px with Shift.

Connector subtrees are excluded from snapping bounds. Their paths and labels change during routing as attached objects move, so including them fed preview geometry back into snap targeting and could make connected text jump vertically. Other shapes, guides and canvas alignment targets remain available.

Regression checks exercise actual modifier mouse events, crossing the drag diagonal, exact coordinates, undo and click toggling. A connected-shape fixture verifies snap targets stay identical after moving the connected object.

Validation: 23 diagram workflow tests, 7 movement tests and 4 snap tests passed. `scripts/lint.sh` passed with workspace/all-target Clippy warnings denied. The AppImage was rebuilt, passed its version smoke check and installed with a backup of the preceding version.
