# Diagram functionality and compatibility

Updated September 28, 2026. The supplied Emulsion UI handoff is the layout
reference; the editor remains native GPUI. Sample inputs are the user's
`drawio-diagrams` and `visioStencils` checkouts. Import success is distinct from
pixel-for-pixel compatibility with another application.

## Implemented workflows

| Area | Behavior |
| --- | --- |
| Drawing | Editable vector shapes and text; bound connectors; drag ports to connect; straight, orthogonal, curved and cyclical routing; custom ports, bends, reconnect and label offsets |
| Object controls | Right-click arrange/alignment, grouping, locking, copy/paste style, annotations and selection export; floating selection toolbar |
| Selection | Mouse marquee, Ctrl/Shift selection, group/ungroup, graph-aware copy/paste/delete, connected movement, undo/redo |
| Formatting | Fill/stroke/text color pickers, typography, thin dark default outlines, white/soft teal/soft blue palettes, line patterns, rounded elbows, filled/hollow connector markers, theme application |
| Library | Shapes, Templates, Containers, Themes and Stencil packs drawers; search; 68 original default stencils, 18 editable templates, nine themes |
| Diagram objects as stencils | Imported and existing shapes automatically appear in Shapes in this diagram; cached background previews, search and pagination; click/drag reuses editable artwork with fresh IDs and one undo step, excluding connections and container contents |
| Installed stencils | Per-entry vector-generated previews; click or drag an installed entry to the pointer position; a drop is one undo step |
| Offline vendor packs | AWS, Azure, Google Cloud, Kubernetes, Cisco, network devices, BPMN, flowchart, floor plans, electrical, wireframes and office; all available entries in each family, paginated 96 at a time |
| Pages/data | Page management and persistence, four layouts, container membership, layout locks, text/CSV/Mermaid/SQL generation, data refresh and conditional fills |
| Automation | Native graph/project MCP operations share the UI command, import and history implementations; see [MCP reference](mcp/mcp-diagrams.md) |

New process shapes have a white fill, one-pixel charcoal outline, a subtle four-pixel corner radius, and centered dark text. The Style tab offers white, soft teal, soft blue and charcoal presets; themes use the same thin outlines. Existing imported colors and artwork retain their source appearance.

Right-click an object to arrange, align/distribute, group, lock, copy/paste style, edit annotations or export the selection. Selecting a connector opens its dedicated floating toolbar: routing, color, width, dash patterns, arrowheads, reverse direction, endpoint size, corner radius, crossings and label editing. Corner rounding applies to straight/elbow routes. Crossing bridges also support curved/cyclical routes. Double lines and label backgrounds are native vector artwork, with undo and export support. Connectors can attach to other connectors; branches follow their parent route and dependency cycles are rejected.

The document toolbox is reconstructed from the active page on import/open and after edits. Basic shapes with matching artwork/style share an entry; complex symbols retain individual entries. A container stencil includes its own frame and artwork, excluding the nested diagram. Import also saves deduplicated, editable packs in the persistent library. “Save shapes to library” captures objects from an existing page. Pack generation runs in the background and reports failures without discarding the imported diagram.

## Sample-inspired templates

The Templates drawer now contains 18 offline, editable starters. Ten additions
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

Generate SVG, PNG, editable `.emu` projects and a visual review index with:

```sh
cargo run --offline -p emulsion-io --example diagram_template_preview -- target/diagram-template-review
```

The review command also checks native save/reopen and rejects raster export fallback.

## Scalable rendering and movement

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
Undo preparation avoids redundant document copies. Display-tree construction indexes the hierarchy once instead of scanning every node for each group. Pointer hit geometry is retained between events. Diagram projects bypass photographic suggestion analysis, which otherwise populates document-sized raster caches.
The measured runs are recorded in [the audit](../specs/reports/diagram-sample-audit.json).
SVG component timings exclude input dispatch, GPU upload and presentation. The GPU benchmark includes command execution, scene updates and GPU completion but excludes OS/compositor presentation. Neither is an end-to-end FPS guarantee.

```sh
cargo run --release --locked -p emulsion-io --example diagram_viewport_bench -- 1000
cargo run --release --locked -p emulsion-io --example diagram_viewport_bench -- 1000 --nested
cargo run --release --locked -p emulsion-engine --example diagram_gpu_bench -- 1000
EMULSION_BENCH_DIAGRAM=1000 cargo run --release --locked -p emulsion-app --features canvas-bench --example editor_canvas_bench
```

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
native vector implementations. AWS resource icons retain their colored/gradient
backgrounds and inset white artwork. Vendor geometry supports fixed aspect ratio,
direction, rotation and flips.

Icon captions overflow their shapes by default; explicit `whiteSpace=wrap` and `labelWidth` control wrapping. HTML labels retain editable UTF-8 style runs, font sizes/colors, bold/italic,
underline/strikethrough, line breaks, lists, and editable HTML table cells with row/column spans, backgrounds and borders. Table layout approximates browser CSS. Vertical labels use
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

## Verification and sample coverage

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

## Remaining source-format differences

Import notes identify approximations. Unsupported JavaScript-defined custom
shapes, exact draw.io routing, sketch effects, browser HTML-table sizing and infinite-canvas page
semantics are not fully reproduced. Some custom-shape caption placement and font substitutions still require visual review. Remote image URLs are not fetched. Complex table CSS, Visio formulas without evaluated values, foreign/OLE objects and source bitmap artwork retain the limitations reported in import notes. Imported image aspect ratio, rotation and flips are preserved for supported artwork.

A project is limited to 4,096 pages, 10,000 graph shapes and 20,000 connectors per
page, with 150,000 document nodes. Larger libraries must be split; empty/corrupt files cannot supply
artwork. The supplied Visio collection includes 701 zero-byte files. Loading a
file does not imply that its bitmap source imagery becomes vector geometry.
