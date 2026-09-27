# Native charts and tables

In Design, open **Elements → Charts and tables** and choose **Bar chart**, **Line chart**, **Pie chart**, or **Table**. The data dialog sets the title, artwork dimensions, palette, and CSV data. The first CSV row contains column headings. The first column contains category labels; the remaining columns contain numeric series, or arbitrary text for tables. Pie charts accept one numeric series with a positive total and no negative values.

Charts are ordinary groups of native paths and text. They remain sharp when zoomed, can be moved and resized with the existing selection tools, and remain editable after saving and reopening an `.emu` project. Selecting the group exposes **Edit selected data…**. Applying changes rebuilds the generated artwork as one undoable transaction. The group keeps its identity so existing references to the group continue to work. Individual generated children are replaced.

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

This is the initial native data-chart workflow. Chart-type conversion controls, stacked and horizontal bars, scatter/radar/area charts, axis formatting, configurable gridlines, cell merging, spreadsheet formulas, live data connections, and animated chart playback remain future work. It is not a claim of full Canva chart or spreadsheet parity.
