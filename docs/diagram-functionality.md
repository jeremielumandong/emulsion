# Diagram functionality and draw.io compatibility

Audit date: September 27, 2026. Visual reference: the supplied **Emulsion Editor
v2.dc.html** and **Emulsion UI Spec.dc.html** from the Modern AI-Powered Design App
handoff. Implementation remains native GPUI; the prototype is the layout reference.

The sample checkout is `drawio-diagrams` at
`0e70e69107de5a663d35468cbca8e6bd870fe975`. Loading a file is **not** certification
that every shape, label, or connector matches draw.io visually.

## Measured sample coverage

The audit considers 622 `.xml`, `.drawio`, and `.svg` files. Other assets in the
checkout (CSV, PNG, Gliffy, and Visio) are separate formats.

| Measurement | Before | After |
| --- | ---: | ---: |
| Successfully imported files | 428 | 620 |
| Imported pages, including library entries | — | 820 |
| Total importer time in this local run, excluding compilation | 112.8 s | 1.2 s |

These are single local corpus runs, not UI frame-time or cross-platform benchmarks.

The two remaining candidates are:

- `blog/template-index.xml`: a catalog of remote template URLs, not a drawing.
- `blog/er-diagram-library.drawio`: connector `UNJGccIc8EjJBR6IzSFc-151` references
  missing source `UNJGccIc8EjJBR6IzSFc-147`. Import fails before installing any pages;
  repairing the source drawing is required. The importer does not invent a shape.

The remaining 620 files load, but many contain compatibility warnings. Read them
using **Import notes** in the Shapes drawer. The warnings are session-local;
keep the original draw.io file alongside the native project.

Reproduce the audit with:

```sh
cargo run --locked -p emulsion-io --example drawio_audit -- /path/to/drawio-diagrams > /tmp/drawio-audit.jsonl
cargo run --locked -p emulsion-io --example drawio_preview -- /path/to/example.drawio /tmp/example.png
```

The audit emits per-file page, shape and connector counts, warnings, errors, and
elapsed time. Internal loose-endpoint handles are excluded from its shape count.
The preview example renders a chosen page through the native export pipeline;
pass a zero-based page index as its third argument.

## Implemented functionality

| Area | Working behavior |
| --- | --- |
| Handoff shell | Searchable 250 px stencil drawer, grid canvas, canvas toolbar, page tabs, navigator; Style/Text/Arrange/Data inspector tabs |
| Creation | Twelve native shape kinds, containers/swimlanes, connected quick-create, text editing |
| Connections | Bound/custom ports, straight/orthogonal routing, reconnect, editable waypoints, label offsets, start/end triangle arrows, dashed lines |
| Editing | Native paths/text, graph-aware copy/cut/delete, connected movement, undo/redo, fill swatches, native paint and text controls |
| Layout | Four automatic layouts, layout locks, align/distribute controls, container membership |
| Pages and persistence | Multiple pages, native project save/reopen, page operations and history |
| Data | Text/CSV/Mermaid/SQL generation, shape data, CSV refresh, conditional fills |
| Libraries | Native stencils and portable local/GitHub packs; draw.io XML libraries open as named editable pages |
| Interchange | Editable common draw.io graph export, Visio XML/OPC and Lucid import, native/SVG/PDF alternatives |

## Improvements in this change

- Normal Open recognizes draw.io `.xml` and SVG files with embedded drawing data;
  ordinary SVG artwork retains its existing import path.
- Decode raw and compressed pages, SVG `content` attributes (including URI encoding),
  and `mxlibrary` JSON entries. Decoding remains bounded; SVG external DTDs are not
  fetched, and internal entity declarations remain rejected.
- Construct pages without per-attribute undo snapshots. Validate the completed
  graph before installation and create a single initial history state.
- Retain loose connectors with invisible editable endpoint handles owned by their
  connector. Moving, copying and deleting a loose connector includes its handles;
  draw.io export writes endpoint coordinates rather than synthetic shapes.
- Preserve relative child coordinates, geometry offsets, parent-relative bends,
  hidden-layer visibility, and transparent text/group bodies. Keep common source
  geometry hints across draw.io round trips.
- Render ellipses, rounded rectangles, hexagons, triangles, partial rectangles,
  actors, double ellipses, crosses and lines as native paths. Generic unsupported
  symbols remain explicit rectangle fallbacks.
- Retain embedded bitmap/SVG data images as native image layers, including draw.io's
  SVG base64 syntax without a `;base64` marker. External image URLs are reported;
  the importer does not fetch them. Embedded images have decoded size limits.
- Accept off-perimeter port coordinates, recover invalid optional port values with
  automatic attachment and a warning, and retain labels up to the native text limit.
  Invalid geometry and missing graph references still fail validation.
- Expand the canvas to include content outside the paper size, translating negative
  coordinates together so object spacing and connections remain consistent.
- Expose import notes in a scrollable dialog. Unsupported arrowheads, curves,
  connector jumps and other appearance differences are reported.
- Follow the handoff's four inspector tabs and expose undoable fill swatches plus
  native paint/text controls directly on selected graph objects.
- Refuse draw.io export when extra artwork, images or additional connector-label
  objects would otherwise be discarded; save `.emu` to retain these objects.

## Still incomplete

| Gap | Current result / required work |
| --- | --- |
| Vendor/custom stencils | Most `mxgraph.*` libraries and inline `stencil(...)` geometry still use rectangle fallbacks. Implement a bounded stencil interpreter and versioned library definitions, with licensing and per-family render fixtures. |
| Rich HTML labels | Converted to editable plain text. Preserve runs, links, tables, font metrics and precise vertical placement before claiming visual parity. |
| Advanced edges | Curves, ERD crow's feet, BPMN/open/diamond markers, line jumps and exact draw.io routing are incomplete. Additional edge labels retain text but need relative-position review. |
| Edge-to-edge attachment | Imported as positioned endpoints with a warning; does not dynamically track another connector path. |
| Layers | Inherited visibility is retained, but draw.io's named layer organization and layer UI are not fully represented. |
| Appearance | Rotation support varies by primitive; shadow, sketch, gradients, symbolic colors and advanced image fitting require further compatibility work. |
| External images | Retain a warning/placeholder; local asset replacement is required. Embedded SVG images are image layers, not editable SVG internals. |
| Page semantics | Content fits within the native canvas limit, but draw.io infinite-canvas paper tiling/background-page references are not fully reproduced. |
| Stencil installation | XML libraries open as pages; a dedicated catalog installation/management workflow for these libraries remains to be added. |
| Handoff polish | Advanced controls, pointer-drag port authoring, accessibility, narrow-window visual review and native screenshots still need acceptance review. |
| AI and collaboration | Existing assistant is available; diagram-specific reviewable AI actions and collaboration remain separate work. |

Priority for the next compatibility pass: stencil geometry/library definitions,
rich labels, advanced connector geometry/markers, then layer/page semantics. Use
rendered comparisons against draw.io for representative files from every sample
family; import counts alone must not close the compatibility milestone.

## Validation

- 18 focused importer tests pass, including compression, libraries, embedded SVG,
  loose endpoint round trips/movement/copying, relative geometry, hidden layers,
  canvas expansion, long labels, malformed references and native project persistence.
- Three Diagram UI tests pass, including inspector navigation, undoable fills,
  connected shape copy/cut restoration and text-generated diagrams.
- The full supplied XML/draw.io/SVG corpus was exercised with the audit example.

The broader workspace contains concurrent changes outside this diagram work;
these checks do not claim validation of every unrelated subsystem.
