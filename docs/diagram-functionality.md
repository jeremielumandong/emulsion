# Diagram functionality and compatibility

Updated September 28, 2026. The supplied Emulsion UI handoff is the layout
reference; the editor remains native GPUI. Sample inputs are the user's
`drawio-diagrams` and `visioStencils` checkouts. Import success is distinct from
pixel-for-pixel compatibility with another application.

## Implemented workflows

| Area | Behavior |
| --- | --- |
| Drawing | Editable vector shapes and text; bound connectors; drag ports to connect; straight, orthogonal and curved routing; custom ports, bends, reconnect and label offsets |
| Selection | Mouse marquee, Ctrl/Shift selection, group/ungroup, graph-aware copy/paste/delete, connected movement, undo/redo |
| Formatting | Fill/stroke/text color pickers, typography, line style, filled/hollow connector markers, theme application |
| Library | Shapes, Templates, Containers, Themes and Stencil packs drawers; search; 68 original default stencils, eight editable templates, six themes |
| Installed stencils | Per-entry vector-generated previews; click or drag an installed entry to the pointer position; a drop is one undo step |
| Offline vendor packs | AWS, Azure, Google Cloud, Kubernetes, Cisco, network devices, BPMN, flowchart, floor plans, electrical, wireframes and office; up to 64 curated entries per pack |
| Pages/data | Page management and persistence, four layouts, container membership, layout locks, text/CSV/Mermaid/SQL generation, data refresh and conditional fills |
| Automation | Native graph/project MCP operations share the UI command, import and history implementations; see [MCP reference](mcp-diagrams.md) |

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
existing compositor. This is a native SVG scene rasterized at screen resolution,
not a browser SVG DOM or an entirely GPU-based SVG renderer.

The 500- and 1,000-shape benchmark includes an attached connector between adjacent
shapes, moves a visible shape, measures 21 edits, and samples five zoom levels.
The measured runs are recorded in [the audit](diagram-sample-audit.json).
Component timings exclude input dispatch, GPU upload and presentation; they are
not an end-to-end FPS guarantee.

```sh
cargo run --release --locked -p emulsion-io --example diagram_viewport_bench -- 1000
cargo run --release --locked -p emulsion-io --example diagram_viewport_bench -- 1000 --nested
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
floor-plan walls, windows and doors, BPMN events/tasks/gateways, and arrows have
native vector implementations. AWS resource icons retain their colored/gradient
backgrounds and inset white artwork. Vendor geometry supports fixed aspect ratio,
direction, rotation and flips.

HTML labels retain editable UTF-8 style runs, font sizes/colors, bold/italic,
underline/strikethrough, line breaks, basic tables and lists. Vertical labels use
rotated text. Rich runs survive draw.io export and reopen. Swimlanes retain header
artwork and header label placement. Connector paths never inherit an arrowhead's
fill, preventing the black polygons previously visible on bent routes.

Connector markers include block, classic, open, diamond, oval, circle-plus and
ER cardinalities. Curves, marker size and filled/hollow state round-trip through
draw.io. Hollow closed markers shorten the visible line beneath the marker.

Compound artwork exports as an embedded **SVG** draw.io shape with a separate
editable label and graph connections. Native projects preserve individually
editable paths and richer source artwork. Export rejects additional connector
artwork that it cannot encode rather than silently discarding it.

The pinned commit, per-file checksums, Apache license and stencil asset terms live
in `assets/diagram-stencils/`. Application packages include those notices.
`scripts/refresh-diagram-stencils.py` rebuilds the archive using the manifest's
pinned sources and verifies every checksum before replacing it.

## Visio and local packs

Modern Visio XML/OPC imports native evaluated geometry, text and supported graph
bindings. Legacy binary `.vss`, `.vsd` and `.vst` files use the installed librevisio
`vss2xhtml`/`vsd2xhtml` converter. Modern files can use the same fallback when their
native formulas cannot be evaluated. Conversion runs locally with bounded input,
output, runtime and page count. Empty converter placeholders are skipped.

Converted artwork retains its vector source or editable paths; each converted
page/master is a connectable graph object. Original Visio formulas and connector
bindings are unavailable in the converter's SVG output. Embedded source bitmaps
remain bitmaps. The converter is optional and must be installed on the host.

Supported Visio/draw.io libraries, SVG files/folders and portable `.emustencil`
packages install into the offline catalog. Empty/unrenderable vendor entries are
excluded with import notes. Installing a pack changes the library; placing an
entry changes the document and supports undo.

## Verification and sample coverage

[The machine-readable audit](diagram-sample-audit.json) records final corpus
counts, failures, movement/undo results and benchmark measurements. The draw.io
checkout contains 623 candidates; 621 import successfully. The two rejected
inputs are a URL catalog (`blog/template-index.xml`) and a drawing with a missing
referenced source node (`blog/er-diagram-library.drawio`).

The Visio audit loads **3,518 of 4,481 files / 37,475 pages** (previous native-only
baseline: 318 files / 2,029 pages). The compressed per-file reports are
`drawio-sample-results.jsonl.gz` and `visio-sample-results.jsonl.gz`.

Every imported page in both supplied collections is exercised by translating an
object, validating its graph, and verifying that undo restores the original
page. Representative AWS, Cisco, floor-plan and BPMN samples are also rendered for
visual review. These checks do not certify every pixel in every sample.

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
shapes, line jumps, exact draw.io routing, sketch/shadow effects, hyperlink actions,
browser HTML-table sizing, named-layer organization and infinite-canvas page
semantics are not fully reproduced. Edge-to-edge attachments become positioned
endpoints. Additional connector-label placement can need adjustment. Remote image
URLs are not fetched. Some rotations/image fitting remain approximate.

A project is limited to 100 pages, 1,000 graph shapes and 2,000 connectors per
page. Oversized libraries must be split; empty/corrupt files cannot supply
artwork. The supplied Visio collection includes 701 zero-byte files. Loading a
file does not imply that its bitmap source imagery becomes vector geometry.
