# Diagram functionality and compatibility

Updated September 28, 2026. The supplied Emulsion UI handoff is the layout
reference; the editor remains native GPUI. Sample inputs are the user's
`drawio-diagrams` and `visioStencils` checkouts. Import success is distinct from
pixel-for-pixel compatibility with another application.

## Start here

Diagram makes editable flowcharts, org charts, process maps, schemas and cloud
architecture drawings. Every shape is native vector artwork with an editable
label, and connectors stay attached when shapes move. A diagram saves as a
multi-page `.emu` project and exports to draw.io, PDF, SVG, PNG or JPEG.

- To learn by doing, follow [Your first diagram](tutorials/diagram-first-diagram.md).
- To compare Diagram with the other workspaces, see [Emulsion workspaces compared](workspaces.md).
- To drive diagrams from an assistant, see the [MCP reference](mcp/mcp-diagrams.md).

Choose **Diagram** on Home, or **File → New diagram…** in an open diagram. The
**New document** dialog offers **Templates** with the 22 starters or **Blank
canvas**. If a diagram tab is already open, **Diagram** on Home switches to that
tab instead.

The drawer beside the canvas has five tabs: **Shapes**, **Templates**,
**Containers**, **Themes** and **Packs**. Below the tabs, a tool strip holds
**Connect shapes**, **Arrange diagram**, the grid and minimap toggles,
**Import diagram pages…**, **Export editable .drawio…**, **Generate from data**
and **Color shapes by data…**. The properties panel has **Style**, **Text**,
**Arrange** and **Data** tabs for the selected shape or connector.

## Implemented workflows

| Area | Behavior |
| --- | --- |
| Drawing | Editable vector shapes and text; bound connectors; drag ports to connect; straight, orthogonal, curved and cyclical routing; custom ports, bends, reconnect and label offsets |
| Object controls | Right-click arrange/alignment, grouping, locking, copy/paste style, annotations and selection export; floating selection toolbar |
| Selection | Mouse marquee, Ctrl/Shift selection, group/ungroup, graph-aware copy/paste/delete, connected movement, undo/redo |
| Formatting | Fill/stroke/text color pickers, typography, thin dark default outlines, white/soft teal/soft blue palettes, line patterns, rounded elbows, filled/hollow connector markers, theme application |
| Library | Shapes, Templates, Containers, Themes and Stencil packs drawers; search; 92 original built-in stencils, 22 editable templates, nine themes |
| Diagram objects as stencils | Imported and existing shapes automatically appear in the temporary Imported data group; cached background previews, search and pagination; click/drag reuses editable artwork with fresh IDs and one undo step, excluding connections and container contents |
| Installed stencils | Per-entry vector-generated previews; click or drag an installed entry to the pointer position; a drop is one undo step |
| Offline vendor packs | AWS, Azure, Google Cloud, Kubernetes, Cisco, network devices, BPMN, flowchart, floor plans, electrical, wireframes and office; all available entries in each family, paginated 96 at a time |
| Pages/data | Page management and persistence, four layouts, container membership, layout locks, text/CSV/Mermaid/SQL generation, data refresh and conditional fills |
| Automation | Native graph/project MCP operations share the UI command, import and history implementations; see [MCP reference](mcp/mcp-diagrams.md) |

New process shapes have a white fill, one-pixel charcoal outline, a subtle four-pixel corner radius, and centered dark text. The Style tab offers white, soft teal, soft blue and charcoal presets; themes use the same thin outlines. Existing imported colors and artwork retain their source appearance.

Right-click an object to arrange, align/distribute, group, lock, copy/paste style, edit annotations or export the selection. Selecting a connector opens its dedicated floating toolbar: routing, color, width, dash patterns, arrowheads, reverse direction, endpoint size, corner radius, crossings and label editing. Corner rounding applies to straight/elbow routes. Crossing bridges also support curved/cyclical routes. Double lines and label backgrounds are native vector artwork, with undo and export support. Connectors can attach to other connectors; branches follow their parent route and dependency cycles are rejected.

Select a connector and drag either circular endpoint handle to change its attachment. You can slide the attachment anywhere on its current object, including through overlapping artwork, or leave that object to attach elsewhere. Drag a straight line segment to move its bends, or drag the visible curved route to move/add a curve waypoint, while keeping both endpoints attached. The drag preview uses the same routing and curve construction as the committed line (arrow insets and crossing decorations appear on release); releasing makes one undoable edit. Escape or a drop outside the canvas cancels. Dropping an endpoint on empty canvas retains its previous attachment. Endpoint drags preserve existing manual bends and connector formatting. Selection follows paint order; an explicitly selected object remains editable through overlapping layers, and Ctrl/Shift selection still uses the object under the pointer.

The document toolbox is reconstructed from the active page on import/open and after edits. Basic shapes with matching artwork/style share an entry; complex symbols retain individual entries. A container stencil includes its own frame and artwork, excluding the nested diagram. Ordinary import/open never saves permanent packs. The temporary Imported data group can be collapsed, cleared, and restored without changing the canvas. “Keep as stencil pack” explicitly saves deduplicated objects from the page; Import stencils installs permanent packs. Saved packs are grouped and removable. A cleanup action removes packs collected by earlier versions from the library, retaining source files and placed objects. Pack generation runs in the background and reports failures without discarding the imported diagram.

The permanent **Web systems** category supplies 14 colored cards matching the browser/server templates: browser, edge, gateway, authentication, application server, database, cache, queue, worker, response, error/retry, WebSocket, outbox and object storage. Cards preserve editable text, vector icons and accent colors. Click/drag inserts a 288 × 148 card; MCP `list_diagram_stencils` and `insert_diagram_stencil` expose the same stable `web-*` IDs. These built-in entries are independent of temporary imported stencils. The portable Emulsion Web Systems pack supports existing application builds.

## Structured objects and review

New UML Class and Entity stencils and the UML template have structured names,
fields and methods. Right-click **Edit UML fields** or **Edit ER fields** to edit
one row per line. Compartments grow with content, follow movement/rotation, and
remain editable paths and text. Direct label edits update the field metadata.
Native saves and Emulsion's draw.io round trip retain the structured type.

The context menu also provides **Set default style**, **Reset default style**,
**Comments**, selection/view links and thumbnail selection. Defaults apply to new
shapes or connectors on the active page. Comments are local threads with replies,
resolve/reopen and deletion; changes support undo and native save/reopen. Copying
an object copies its comments with fresh thread/object IDs. Comments are not sent
to an external service. The selected thumbnail bounds are used for page/project
previews without cropping the document itself.

Save the project before copying a link. **Open diagram link** navigates to its
page and selection or camera in the matching open project. These links do not
open arbitrary files or launch an external URL handler. Review settings are
native project metadata; third-party formats do not provide a matching review model.
All these controls have MCP operations described in the MCP guide.

Dense orthogonal routing uses an indexed visibility grid and A* search instead
of rejecting every fallback above 64 obstacles. Grid allocation is bounded at
one million cells. If overlap or complexity prevents finding a corridor, the
connector toolbar shows a warning and MCP returns `routing_warning`. Move the
obstacles or add manual waypoints to resolve it.

## Browser and server system templates

Four detailed 1920 × 1240 boards are available in Templates and through the shared MCP template catalog. Search for `browser` or use these template IDs:

| Template | ID | Contents |
| --- | --- | --- |
| Browser to first paint | `web-page-journey` | Navigation, DNS, HTTPS, cache shortcuts, origin processing and browser rendering |
| Authenticated API platform | `web-api-platform` | Gateway, identity, authorization, database, cache, queue, background workers and error paths |
| Asynchronous checkout | `web-async-checkout` | Idempotency, transactional outbox, payment processing, retries, dead letters and recovery |
| Real-time browser updates | `web-realtime-updates` | Authorized subscriptions, event fan-out, heartbeats, reconnect and durable catch-up |

The boards use seven semantic colors, native stencil icons, grouped containers, editable typography, labeled connectors, rounded bends and crossing bridges. Purple identifies clients, blue requests and application logic, cyan network components, green responses, amber asynchronous work, rose failures and slate storage. Titles, legends and notes explain how to read and adapt each sample. These are conceptual reference designs, not measurements or deployed infrastructure.

## Sample-inspired templates

The Templates drawer now contains 22 offline, editable starters. Ten additions
use the layouts in the local `Downloads/Diagram` reference collection: Business
process, Purchase approval, Family tree, Cause and effect, Team directory,
Strategy tree, Improvement cycle, Project roadmap, Relationship map and Cloud
architecture. The existing Network diagram also uses the bundled infrastructure
stencils. Search accepts multiple words, such as `credit workflow` or `cloud storage`.

These are original native vector compositions with editable labels, one-pixel
dark outlines, soft fills and bound connectors. Family/person placeholders can
be renamed; no stock portraits, website scripts or reference branding are bundled.
Swimlane objects retain their container membership. Fishbone and relationship
branches attach to their parent connector and follow edits. Template shapes carry
the same catalog identity as toolbox stencils and appear in Shapes in this diagram.
Each template opens as a new page with one-step undo/redo, through either UI or MCP.

## Generate a diagram from text or data

Open **Generate from data** in the Diagram tool strip and choose **Text flow**,
**CSV**, **Mermaid flowchart** or **SQL schema**. The dialog starts with a short
sample of that format. Replace it with your input and choose **Apply**. Emulsion
builds a new page of editable shapes and connections and leaves the current page
unchanged.

To read a file instead, choose **Import local data file…** in the same menu. The
file extension selects the parser: `.txt` for text flow, `.csv` for CSV, `.mmd`
or `.mermaid` for Mermaid, and `.sql` for SQL. Input is limited to 1 MiB. A
generated page accepts up to 10,000 shapes and 20,000 connections. Shape IDs are
1–200 bytes and labels up to 2,000 characters. Malformed or unsupported input is
rejected before anything is added to the project. Parsing never runs scripts,
formulas or SQL.

### Text flow

Write one step per line. Each line becomes a Process shape connected to the
previous line. Write `A -> B -> C` on one line to connect an explicit chain; a
line with arrows does not connect to the line before it. Repeating a name reuses
its shape, which creates branches and loops. Text flow has no connector labels.

Illustrative input:

```text
Request -> Review
Review -> Approve
Review -> Reject
```

### CSV

The first row names the columns. The `id` column is required. Optional columns
describe shapes and connections:

- `label`: the shape text. An empty label uses the ID.
- `type`: `process` (the default), `decision`, `start`, `end`, `terminator`,
  `database`, `entity` or `note`.
- `next`: target IDs separated by semicolons.
- `edge_label`: the label for every connection that leaves this row.

Every other column becomes a local data field on the shape, shown on the
**Data** tab. Quoted fields may contain commas, doubled quotes and line breaks.
Each generated shape stores its row ID as `source_id`, so that column name is
reserved.

This input is checked by a regression test:

```csv
id,label,type,next,edge_label,owner
a,"Review, then approve",decision,b;c,Yes,Pat
b,Publish,process,,,Lee
c,"Wait
for changes",note,,,Sam
```

To update a generated page, choose **Refresh mapped labels and data from CSV…**.
Refresh matches rows to shapes by `source_id` and updates their labels and data
fields. Positions and connections stay as they are. A row with no matching shape
rejects the refresh; generate a new page to change the structure. Undo reverts
the refresh.

Choose **Color shapes by data…** to fill shapes whose data field equals a value.
The rule is saved on the selected shapes, or on every shape when nothing is
selected, and it follows later refreshes.

### Mermaid flowchart

Emulsion reads a subset of Mermaid flowcharts:

- The first line is `flowchart TD`, `flowchart TB` or `flowchart LR`, or the
  same forms with `graph`. TD and TB lay out top to bottom; LR lays out left to
  right.
- A node ID uses letters, digits and underscores. `A[Label]` makes a rectangle,
  `A{Label}` a decision, and `A(Label)` or `A((Label))` a rounded start/end
  shape.
- `-->` draws an arrow, `---` draws a line without an arrowhead, and
  `-->|Label|` labels the connection. One line may chain several nodes.
- Semicolons separate statements, a statement that starts with `%%` is a
  comment, and a surrounding
  `mermaid` code fence is accepted.

`subgraph`, `style`, `class`, `click` and `linkStyle` are rejected, as are other
node shapes and directions. Import draw.io XML for styled diagrams.

This input is checked by a regression test:

```text
flowchart LR
A[Start] --> B{Ready?}
B -->|Yes| C((Done))
B -->|No| A
```

### SQL schema

SQL input accepts `CREATE TABLE` and `CREATE TABLE IF NOT EXISTS` statements
only; any other statement rejects the input. Each table becomes an Entity shape
that lists its columns as `name: type`. A column with `REFERENCES other(...)`
adds a connection labelled with the column name. A table-level
`FOREIGN KEY ... REFERENCES` constraint adds a connection labelled
`foreign key`. `--` comments and quoted identifiers are accepted.

This input is checked by a regression test:

```sql
CREATE TABLE users (id INTEGER PRIMARY KEY, name VARCHAR(100));
CREATE TABLE orders (id INT, user_id INT REFERENCES users(id), amount DECIMAL(10,2), CONSTRAINT fk_user FOREIGN KEY (user_id) REFERENCES users(id));
```

## Import from Lucid

Emulsion imports the Lucid Standard Import format, version 1: a `.lucid`
package that contains `document.json`, or a `.lucidjson` file. Choose
**File → Import → Lucid (.lucid, .lucidjson)…** to add its pages to the current
diagram. Choose **File → Open diagram…** to open the file as a new project.
Lucid cloud backups and infrastructure JSON exports are different formats and
are not read.

The importer rejects a file in these cases:

- The file is not Standard Import version 1. For a Lucidchart document, export
  VSDX or VDX from Lucid and import that instead.
- `document.json` exceeds 2 MiB, or the pages, shapes or lines exceed the
  project limits.
- A line has a free endpoint or ends on another line. Export VSDX to keep the
  drawn geometry.
- A page contains data-backed shapes. Expand them in Lucid or export VSDX.
- The file uses an unknown paper size or stroke style.

Other differences are reported as import notes. Open them with the
**Import notes** button in the drawer:

- Unrecognised shape types become editable rectangles that keep their label and
  data.
- HTML labels keep editable plain text; inline formatting needs review.
- Curved lines become editable straight segments.
- Endpoint styles other than a plain arrow use a triangular arrowhead.
- Infinite canvases become finite pages, and objects keep their coordinates.
- External images are not fetched; an editable placeholder keeps the reference.
- Lucid actions are not run, and data collection bindings are not refreshed.

## Themes and auto-layout

Open the **Themes** tab to restyle a diagram. Choose **Entire page** or
**Selection**, then click a theme card. The nine themes are Charcoal, Soft teal,
Soft blue, Monochrome grey, Monochrome blue, Monochrome teal, Monochrome earth,
Chalk and Emulsion default. Undo reverts a theme in one step.

Open **Arrange diagram** in the tool strip and choose **Top to bottom**,
**Left to right**, **Grid** or **Mind map**. Layout ranks shapes by their
connections; cycles and disconnected groups stay visible. Undo reverts the whole
arrangement in one step.

Layout leaves these shapes in place:

- Shapes with **Keep position during layout** turned on. Select a shape and turn
  it on in the properties panel.
- Locked objects and objects on position-locked layers.
- Shapes inside a container. The container moves as one unit with its children.

Layout fails with a message when no unlocked shapes remain, or when the result
would overlap fixed objects. After layout, the status bar reports any shapes
that extend outside the page.

## draw.io fidelity

The reader accepts raw/compressed pages, embedded SVG drawing data, libraries,
relative vertices and containers, inherited visibility, custom ports and loose
endpoints. Shapes, labels, graph attachments and editable artwork survive movement
and native persistence. Missing graph references fail explicitly rather than
silently replacing an intended connection.

8,954 pinned upstream XML stencil definitions are available offline. The
interpreter handles independent paints, path segments/arcs, save/restore, nested
stencils, text and opacity without executing JavaScript. Common parameterized
floor-plan walls, windows, doors and stairs, parameterized arcs/pies, infographic ribbons/cylinders, BPMN events/tasks/gateways, and arrows have
native vector implementations. Additional native geometry covers mockup icons/grids/pie charts, curly braces, browser windows, invisible anchors, dimension lines, top buttons, phone frames, download bars, U-turn arrows, folded banners and shaded cubes. AWS resource icons retain their colored/gradient
backgrounds and inset white artwork. Vendor geometry supports fixed aspect ratio,
direction, rotation and flips.

Icon captions overflow their shapes by default; explicit `whiteSpace=wrap` and `labelWidth` control wrapping. HTML labels retain editable UTF-8 style runs, font sizes/colors, bold/italic,
underline/strikethrough, line breaks, lists, and editable HTML table cells with row/column spans, backgrounds and borders. Explicit table/column/cell widths (pixels or percentages), cell padding, minimum row/cell heights, vertical alignment and row backgrounds are supported. Other table CSS still approximates browser layout. Vertical labels use
rotated text. Rich runs survive draw.io export and reopen. Swimlanes retain header
artwork and header label placement. Connector paths never inherit an arrowhead's
fill, preventing the black polygons previously visible on bent routes.

Connector markers include block, classic, open, diamond, oval, circle-plus and
ER cardinalities. Curves, marker size and filled/hollow state round-trip through
draw.io. Hollow closed markers shorten the visible line beneath the marker. Straight, orthogonal and curved connectors support arc, gap and sharp crossing bridges. Primary and extra connector labels retain their route-relative position and perpendicular offset and follow rerouting. Named layers, HTTP(S) links and hard-edged vector shadows survive import/export.

Compound artwork exports as an embedded **SVG** draw.io shape with a separate
editable label and graph connections. Native projects preserve individually
editable paths and richer source artwork. Export rejects additional connector
artwork that it cannot encode rather than silently discarding it.

The pinned commit, per-file checksums, Apache license and stencil asset terms live
in `assets/diagram-stencils/`. Application packages include those notices.
`scripts/refresh-diagram-stencils.py` rebuilds the archive using the manifest's
pinned sources and verifies every checksum before replacing it. Parameterized
geometry translations also follow the same pinned upstream
[mxBasic.js](https://github.com/jgraph/drawio/blob/0f419a92c769adb5fb20f2b18053a5ae8c7e4993/src/main/webapp/shapes/mxBasic.js),
[mxInfographic.js](https://github.com/jgraph/drawio/blob/0f419a92c769adb5fb20f2b18053a5ae8c7e4993/src/main/webapp/shapes/mxInfographic.js) and
[mxFloorplan.js](https://github.com/jgraph/drawio/blob/0f419a92c769adb5fb20f2b18053a5ae8c7e4993/src/main/webapp/shapes/mxFloorplan.js)
under the bundled Apache-2.0 license.

## Visio and local packs

Legacy Visio recovery is excluded from the current work at the user’s request. The following describes existing support and historical audit results.

Modern Visio XML/OPC imports native evaluated geometry, text and supported graph
bindings, including circular/elliptical arcs, polylines and evaluated numeric NURBS. Rational curves use adaptively fitted cubic segments. Themed scalar cells use inherited evaluated values when available. Visible converter output with invalid zero-size SVG roots is fitted to its ink bounds. Legacy binary `.vss`, `.vsd` and `.vst` files use the installed librevisio
`vss2xhtml`/`vsd2xhtml` converter. Modern files can use the same fallback when their
native formulas cannot be evaluated. Conversion runs locally with bounded input,
output, runtime and page count. Empty converter placeholders are skipped. Conversion streams entries from a temporary file (512 MiB total, 64 MiB per SVG entry, 60-second converter timeout) to avoid holding a large XHTML library in memory. Unusable entries are reported while valid entries remain available.

Converted artwork retains its vector source or editable paths; each converted
page/master is a connectable graph object. Original Visio formulas and connector
bindings are unavailable in the converter's SVG output. Embedded source bitmaps
remain bitmaps. The converter is optional and must be installed on the host.

Supported Visio/draw.io libraries, SVG files/folders and portable `.emustencil`
packages install into the offline catalog. Empty/unrenderable vendor entries are
excluded with import notes. Installing a pack changes the library; placing an
entry changes the document and supports undo.

## Remaining source-format differences

Import notes identify approximations. Unsupported JavaScript-defined custom
shapes, exact draw.io routing, sketch effects, browser HTML-table sizing and infinite-canvas page
semantics are not fully reproduced. Some custom-shape caption placement and font substitutions still require visual review. Remote image URLs are not fetched. Complex table CSS, Visio formulas outside the supported arithmetic subset, foreign/OLE objects and source bitmap artwork retain the limitations reported in import notes. Imported image aspect ratio, rotation and flips are preserved for supported artwork.

A project is limited to 4,096 pages, 10,000 graph shapes and 20,000 connectors per
page, with 150,000 document nodes. Larger libraries must be split; empty/corrupt files cannot supply
artwork. The supplied Visio collection includes 701 zero-byte files. Loading a
file does not imply that its bitmap source imagery becomes vector geometry.

Connector mode accepts any picked position on an object as a relative attachment. The point follows object movement, resizing, rotation and reflection; the visible midpoint handles remain available as shortcuts.

Visio loose lines import as native connectors with owned free endpoints and conservative outline-contact inference. Common numeric ShapeSheet formulas and explicit single-theme palette references are supported. See [follow-up validation](../specs/reports/diagram-gap-followup.md) for the supported subset and remaining format differences.


### Discovering shape libraries

The fixed **Add shapes…** button opens a searchable modal. Standard and Flowchart are the only default toolbox groups. Browse native libraries, 42 draw.io families and 55 AWS/Azure icon packs (1,473 icons), and installed personal packs; preview entries in pages of 24, check the desired groups and choose **Apply libraries**. Cancel discards the draft selection. Enabled groups persist across restarts. Unchecking a group hides it without deleting installed files or changing canvas objects.

The picker separates previewing from selection: the outlined row identifies the library being previewed, while native checkboxes and a live selection count identify the groups to apply. Libraries are grouped under **Built-in**, **More libraries** and **Imported**, with shape counts beside their names. The title and pagination stay above a separately scrolling preview grid; on narrow windows the library list moves above the previews. Preview cards use a light surface to keep dark diagram symbols legible.

Additional packs are built sequentially in the background only after Apply. The catalog includes Android/iOS mockups, AWS and Azure icon sets, Google Cloud, BPMN, geometric shapes, arrows, server racks, process engineering, value stream and network libraries. Original AI workflow symbols cover agents, language models, prompts, retrieval, tools, memory, guardrails and human review; they are not third-party provider logos.

Corner resize handles take precedence over blank-canvas selection on nonrectangular stencils, so circles, diamonds and imported vector artwork can be resized from their bounding-box corners.

### Precise positioning

Ctrl-drag bypasses grid and alignment snapping. Shift-drag locks the first deliberate movement to the horizontal or vertical axis until Shift is released; Ctrl+Shift combines both. Holding a selection modifier before dragging a selected object preserves the selection for the move. A modifier click without dragging still toggles selection. Arrow keys move selected objects by one document pixel, and Shift+arrow moves ten pixels, independently of zoom. Connector paths and labels are excluded from alignment snap targets because rerouting them during a move would otherwise make targets shift beneath the pointer.

### Sharp diagram export

PDF export uses native vector page export, preserving editable path/text artwork as scalable PDF contours rather than embedding a canvas-size screenshot. Embedded bitmap artwork retains its source resolution; unsupported compositing effects may still require a rendered appearance. The diagram export chooser initially selects 2× raster output and also offers 1× and 4×. Enlarged PNG output redraws vector paths and text at the requested dimensions rather than enlarging a canvas bitmap. DPI changes metadata only; use the size controls to increase pixel dimensions. MCP exports accept `scale: "double"` and `scale: "quadruple"` as well as the existing sizes. Raster output is limited to 64 megapixels for scaled/vector exports; use vector PDF for larger diagrams.

## Technical notes

The commands, benchmarks and corpus results below support development and
compatibility review. They are not needed for everyday diagramming.

### Template preview commands

Generate editable `.emu` pages, full vector SVGs, PNG previews and a combined project with:

```sh
cargo run -p emulsion-io --example diagram_template_preview -- /path/to/output web-
```

Generate SVG, PNG, editable `.emu` projects and a visual review index with:

```sh
cargo run --offline -p emulsion-io --example diagram_template_preview -- target/diagram-template-review
```

The review command also checks native save/reopen and rejects raster export fallback.

### Scalable rendering and movement

Diagram display uses retained SVG subtrees with vector glyph outlines. Text stays
editable. Rendering samples paths at the current zoom and physical display
resolution; it does not enlarge a document-resolution text bitmap. The 64× zoom
regression checks the curved edge of a letter against a scaled low-resolution
image. Ordinary SVG imports become editable paths where supported. Gradients,
filters, masks and other richer SVG artwork retain their original SVG source in
a Smart Object, including through native save/reopen and SVG export.

Unchanged SVG subtrees survive edits. Moving one shape rebuilds that shape and its
changed connectors, including objects inside ordinary groups and containers.
Offscreen roots are culled and only damaged screen pixels are redrawn. Selection clicks reuse the existing frame. Background scene
preparation retains the prior image until replacement is ready. Structural and
compositing changes may require a full redraw. Unsupported native effects use the
existing compositor. Compatible native diagrams use GPU vector paths and text, with ordinary groups batched into one vector pass. Moving a shape patches changed vectors without recompiling the compositing tree. Diagrams requiring richer SVG filters/masks retain the SVG scene rendered at screen resolution. This is a native renderer, not a browser SVG DOM.

The 500- and 1,000-shape benchmark includes an attached connector between adjacent
shapes, moves a visible shape, measures 21 edits, and samples five zoom levels.
A GPU benchmark additionally measures native command execution, scene updates and completed GPU rendering. A native-window benchmark exercises pan, object dragging and command movement through the editor. These measurements exclude physical display latency.
Undo preparation avoids redundant document copies. Display-tree construction indexes the hierarchy once instead of scanning every node for each group. Pointer hit geometry is retained between events. Movement reuses unchanged body bounds and label geometry, skips unrelated straight-connector rerouting and avoids rebuilding metadata target sets on translations. Diagram projects bypass photographic suggestion analysis, which otherwise populates document-sized raster caches.
The measured runs are recorded in [the audit](../specs/reports/diagram-sample-audit.json).
SVG component timings exclude input dispatch, GPU upload and presentation. The GPU benchmark includes command execution, scene updates and GPU completion but excludes OS/compositor presentation. Neither is an end-to-end FPS guarantee.

```sh
cargo run --release --locked -p emulsion-io --example diagram_viewport_bench -- 1000
cargo run --release --locked -p emulsion-io --example diagram_viewport_bench -- 1000 --nested
cargo run --release --locked -p emulsion-engine --example diagram_gpu_bench -- 1000
EMULSION_BENCH_DIAGRAM=1000 cargo run --release --locked -p emulsion-app --features canvas-bench --example editor_canvas_bench
```

### Verification and sample coverage

[The machine-readable audit](../specs/reports/diagram-sample-audit.json) records final corpus
counts, failures, movement/undo results and benchmark measurements. The draw.io
checkout contains 623 candidates; 621 import successfully. The two rejected
inputs are a URL catalog (`blog/template-index.xml`) and a drawing with a missing
referenced source node (`blog/er-diagram-library.drawio`).

The Visio audit loads **3,774 of 4,481 files / 67,128 pages** (79 more files than the preceding audit; previous native-only baseline: 318 files / 2,029 pages). The 707 rejected files comprise 701 empty files and six nonempty inputs that the available converters cannot read. The compressed per-file reports are
`drawio-sample-results.jsonl.gz` and `visio-sample-results.jsonl.gz`.

Every imported page in both supplied collections is exercised by translating an
object, validating its graph, and verifying that undo restores the original
page. Representative AWS, Cisco, floor-plan and BPMN samples are also rendered for
visual review. These checks do not certify every pixel in every sample. See the [visual and native-window performance review](../specs/reports/diagram-visual-review.md).

```sh
cargo run --release --locked -p emulsion-io --example drawio_audit -- /path/to/samples --exercise
cargo run --release --locked -p emulsion-io --example drawio_preview -- /path/to/sample.drawio /tmp/preview.png
cargo test --locked -p emulsion-io drawio --lib
cargo test --locked -p emulsion-io svg_viewport --lib
cargo test --locked -p emulsion-io diagram --lib
cargo test --locked -p emulsion-core diagram --lib
cargo test --locked -p emulsion-mcp diagram --lib
cargo test --locked -p emulsion-ui diagram_ --lib
```

Regressions cover import/export and native persistence, rich labels, embedded
SVG, vendor packs/previews, scalable zoom, retained compositing, damage redraw,
connector edits and rollback, grouping/selection, port gestures, built-in and
installed-entry mouse dragging, color application and undo.
