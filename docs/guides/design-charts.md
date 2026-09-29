# Native charts and tables

In Design, open **Elements → Charts and tables** and choose **Bar chart**, **Line chart**, **Pie chart**, **Table**, **Area chart**, **Scatter plot**, **Stacked bar**, or **Donut chart**. The data dialog sets the title, artwork dimensions, palette, and data. Edit values directly in **Cells**, or switch to **CSV** to paste a complete dataset. The first row contains column headings. Column A contains category labels; the remaining columns contain numeric series, or arbitrary text for tables. Pie and donut charts accept one numeric series with a positive total and no negative values.

Use **Add row** and **Add column** to extend the grid. Each data row and column has a **Remove** control; the header, at least one data row and at least two columns are retained. Larger datasets scroll within the grid. Cells preserve Unicode, quotation marks, commas and multiline text when switching between the grid and CSV. Invalid CSV stays in the draft with an inline error until corrected.

The chart-type buttons also convert existing artwork between all eight chart/table types, preserving the entered data. Conversion never silently drops extra series or replaces table text with numbers. Incompatible data produces an inline error: for example, remove extra value columns explicitly before applying a pie chart. **Apply data** commits all changes as one Undo step; **Cancel** discards the draft. Changing the chart type preserves the chart group's identity, title and palette.

Cartesian charts expose numeric Y minimum/maximum (blank means automatic), 2–20 evenly spaced ticks, an axis title, and tick-label visibility. Scatter plots additionally use numeric X data in column A with their own bounds/ticks; other charts keep category labels on X. Scatter inputs must be numeric on both axes. Area charts fill each series to zero (or the nearest visible range boundary). Stacked bars accumulate positive and negative values separately. Donut charts use native ring segments with a transparent center. Data beyond explicit axis bounds is clipped from the plot geometry.

Tables support **Merge range** and **Unmerge at cell**. Enter a 1-based start row/column and row/column spans; row 1 is the header. Only the top-left cell is displayed in a merged region, while all covered values remain stored and reappear after unmerging. Overlapping or out-of-bounds regions are rejected. Removing rows/columns adjusts affected regions. CSV contains cell values, not merge metadata; project files retain both.

Charts are ordinary groups of native paths and text. They remain sharp when zoomed, can be moved and resized with the existing selection tools, and remain editable after saving and reopening an `.emu` project. Selecting the group exposes **Edit data** in the floating selection toolbar, and **Edit selected data…** remains available in Elements. Applying changes rebuilds the generated artwork as one undoable transaction. The group keeps its identity so existing references to the group continue to work. Individual generated children are replaced.

Use **Detach from data** before making manual changes that should survive future artwork edits. This removes the data association while keeping all paths and text. Undo restores the association. Editing data intentionally replaces manual edits to generated children. A rotated or otherwise transformed chart is regenerated as axis-aligned artwork within the selection bounds when its data is edited.

Duplicating or copying a whole chart preserves its data association. Copying an individual child copies that artwork alone. A locked group, locked ancestor, or protected descendant prevents data replacement and detachment, including independent pixel, position, and transparency locks. Invalid data leaves the document unchanged and keeps the dialog open for correction.

Native project files retain chart data. SVG and PDF exports preserve the supported vector artwork; they do not carry an editable Emulsion data association. Raster exports render the chart at the requested output resolution.

## Current limits

- Up to 50 data rows and 8 numeric series or table value columns, plus the header and category column.
- Up to 1000 characters per cell, a 200-character title, and 16 palette colors.
- Artwork dimensions from 160 to 10000 pixels; numeric magnitudes up to one trillion.
- CSV input is limited to 512 KiB. Quoted commas, escaped quotes, Unicode, and multiline cells are supported.
- Up to 256 chart/table groups per page, subject to the document's normal object limit.
- Dense charts and tables may need larger dimensions to make labels readable.

Horizontal bars, radar charts, configurable gridline styling, spreadsheet formulas, live data connections, and animated chart playback remain future work. The cell grid is a data editor, not a full spreadsheet.

## Local formulas

Enable **Formulas** in the chart/table data editor to interpret cells beginning
with `=`. Existing documents default to literal cells. The grid keeps formula
source text; Apply creates editable native text/path artwork from calculated
values. Changing an input recalculates dependent cells in the same Undo step.
MCP add/update chart tools accept `formulas:true` and the same source rows.

References use A1–I51 (row 1 is the header), optional `$` address markers, numeric
constants, arithmetic `+ - * / ^`, parentheses, and SUM/AVERAGE/MIN/MAX/COUNT/ABS/
ROUND. Aggregates accept ranges such as `=SUM(B2:B4)` and ignore text in ranges.
`ROUND(value, decimals)` accepts -12 through 12. Commas inside CSV formulas must
follow normal CSV quoting. No expression can call scripts, URLs or external files.

Invalid references, cycles, division by zero, excessive nesting/calculation cost,
and nonfinite results reject Apply atomically with a cell diagnostic. Results are
bounded to ±1e12. Addresses refer to current grid positions; Emulsion
does not rewrite cell references when rows or columns move. Native saves retain source
formulas and calculated editable artwork.
