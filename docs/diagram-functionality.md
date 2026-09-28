# Diagram functionality and draw.io compatibility

Audit date: September 27, 2026. Visual reference: the supplied **Emulsion Editor
v2.dc.html** and **Emulsion UI Spec.dc.html** from the Modern AI-Powered Design App
handoff. Implementation remains native GPUI; the prototype is the layout reference.

The sample checkout is `drawio-diagrams` at
`0e70e69107de5a663d35468cbca8e6bd870fe975`. Loading a file is **not** certification
that every shape, label, or connector matches draw.io visually.

## Measured sample coverage

The XML/draw.io/SVG baseline considers 622 files. The audit tool now also scans
Visio extensions: this checkout adds one `.vsdx` candidate whose unresolved
`FillPattern` value prevents import. CSV, PNG and Gliffy are separate formats.

| Measurement | Before | After |
| --- | ---: | ---: |
| Successfully imported files | 428 | 620 |
| Imported pages, including library entries | — | 820 |
| Total importer time in this local run, excluding compilation | 112.8 s | 1.4 s |

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
| Creation | 68 default stencils across 11 searchable/collapsible categories, twelve underlying shape kinds, containers/swimlanes, connected quick-create, text editing |
| Connections | Bound/custom ports, straight/orthogonal routing, reconnect, editable waypoints, label offsets, start/end triangle arrows, dashed lines |
| Editing | Native paths/text, mouse box selection, Ctrl/Shift multi-selection, grouping/ungrouping, graph-aware copy/cut/delete, connected movement, undo/redo, fill/stroke/text color pickers |
| Layout | Four automatic layouts, layout locks, align/distribute controls, container membership |
| Pages and persistence | Multiple pages, native project save/reopen, page operations and history |
| Data | Text/CSV/Mermaid/SQL generation, shape data, CSV refresh, conditional fills |
| Libraries | Shapes, Templates, Containers, Themes and Stencil packs tabs; 8 editable templates, 6 themes, drag-to-canvas bundled stencils, local SVG folder/draw.io/Visio stencil installation |
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
- Preserve sibling stacking so background shapes do not hide connectors.
- Decode bounded inline stencil geometry; malformed decorations use a reported
  placeholder without losing the surrounding valid graph.
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
| Vendor/custom stencils | Most named `mxgraph.*` libraries still use rectangle fallbacks. Inline `stencil(...)` paths, lines, cubic/quadratic curves, arcs, rectangles and ellipses are decoded; per-part paints, nested includes, text and other instructions need further support. Versioned vendor definitions and per-family render fixtures remain necessary. |
| Rich HTML labels | Converted to editable plain text. Preserve runs, links, tables, font metrics and precise vertical placement before claiming visual parity. |
| Advanced edges | Curves, ERD crow's feet, BPMN/open/diamond markers, line jumps and exact draw.io routing are incomplete. Additional edge labels retain text but need relative-position review. |
| Edge-to-edge attachment | Imported as positioned endpoints with a warning; does not dynamically track another connector path. |
| Layers | Inherited visibility is retained, but draw.io's named layer organization and layer UI are not fully represented. |
| Appearance | Rotation support varies by primitive; shadow, sketch, gradients, symbolic colors and advanced image fitting require further compatibility work. |
| External images | Retain a warning/placeholder; local asset replacement is required. Embedded SVG images are image layers, not editable SVG internals. |
| Page semantics | Content fits within the native canvas limit, but draw.io infinite-canvas paper tiling/background-page references are not fully reproduced. |
| Stencil installation | Supported XML libraries, Visio and SVG folders install into the offline catalog. Vendor-family bundling, pack removal controls and legacy Visio conversion remain incomplete. |
| Handoff polish | Advanced controls, custom port authoring, accessibility, narrow-window visual review and native screenshots still need acceptance review. |
| Collaboration | Local MCP graph/project actions are available; multi-user collaboration remains separate work. |

Priority for the next compatibility pass: stencil geometry/library definitions,
rich labels, advanced connector geometry/markers, then layer/page semantics. Use
rendered comparisons against draw.io for representative files from every sample
family; import counts alone must not close the compatibility milestone.

## Canvas text and interactive connections

Supported diagram pages now display through an SVG scene containing vector glyph
outlines. Labels remain editable native text. The scene is rendered at the current
zoom and physical display resolution, avoiding enlargement of document-sized text
bitmaps. Pan, zoom and rotation reuse the parsed scene. A bounded glyph-outline cache avoids
reshaping unchanged labels. Document edits rebuild the scene in the background,
with one build in flight; the previous scene stays visible until it is ready.
Selection-only clicks reuse the rendered frame. Ordinary vector moves redraw the
union of the old/new object and connector bounds; structural and effect edits
fall back to a full viewport render. This is a native SVG scene rasterized at
physical screen resolution, not a browser SVG DOM or a GPU-only SVG renderer.
Pages with unsupported SVG effects retain the existing compositor.

Hover over or select a shape to expose circular connection ports. Drag a port onto
another shape or port to create an attached connector, with a live routing preview.
The toolbar Connect tool also retains its two-click workflow. Escape or a drag
released on empty space cancels; creating a connector is one undo step. MCP uses
the existing `add_diagram_connector` and `set_diagram_connector` graph operations.

## Validation

- 23 focused draw.io tests pass, including compressed pages, libraries, geometry,
  measured wrapped-label placement, connector defaults and round trips.
- 11 Diagram UI tests pass: selection/grouping/movement, actual toolbox drag/drop,
  connection gestures, color apply/cancel/undo, gallery navigation and SVG caching.
- Three SVG viewport tests pass: editable vector text, 64× glyph zoom, and partial
  redraw versus full rendering after movement at zoom and rotation.
- Five SVG/PDF export tests and five portable-pack tests pass, including SVG folder
  installation and editable stencil connections after persistence.
- Nine MCP diagram tests and sixteen core diagram/library tests pass.
- The nested tile-scratch regression test passes. The captured crash was a
  reentrant `RefCell` borrow during nested rendering; scratch storage now releases
  its borrow before entering the renderer.
- The full supplied XML/draw.io/SVG corpus was exercised with the audit example.

The local 100-shape benchmark measured roughly 27 ms for a complete 1440×1080
viewport redraw versus 0.57 ms (median) / 0.60 ms (95th percentile) for a movement
patch in the final run. Background scene preparation was 10.03 ms median /
10.16 ms at the 95th percentile; initial cold preparation was 94.02 ms.
These are CPU timing samples, not end-to-end UI FPS;
image upload, input handling and compositor costs are additional. Reproduce with:

```sh
cargo run --release --locked -p emulsion-io --example diagram_viewport_bench -- 100
```

The broader workspace contains concurrent changes outside this diagram work;
these checks do not claim validation of every unrelated subsystem.


## Default stencil catalog

68 original, editable vector stencils ship offline in 11 categories: Flowchart,
General, UML / Software, Entity relationship, BPMN / Business, Network /
Infrastructure, Cloud / Architecture, Wireframe / UX, Office / Floor plan,
Electrical, and Planning. Search matches names, category names and keywords.
Each insertion is one undo step and supports attached connectors. Vendor artwork
from the supplied collection is not bundled. Custom native stencil paths export
as compressed inline draw.io stencils rather than plain rectangles.

## MCP coverage

The graph and project tools use the same commands and parsers as the native UI.
The live host closes the assistant's outer transaction before native operations,
so graph/page tools keep their own undo boundaries. Page changes refresh the
canvas and clear stale selections through the existing page lifecycle.

| Functionality | MCP entry points |
| --- | --- |
| Discover and inspect | `list_diagram_stencils`, `list_diagram_library`, `list_diagram_stencil_packs`, `describe_diagram`, `describe_project`, `get_view` |
| Templates, themes and packs | `insert_diagram_template`, `apply_diagram_theme`, `install_diagram_stencil_pack`; installed entries use `insert_diagram_pack_entry` |
| Create and connect | `add_diagram_shape`, `insert_diagram_stencil`, `add_diagram_connector`, `quick_create_diagram` |
| Ports, reconnect, routing, bends, labels, arrows | `set_diagram_connector` |
| Labels, data, conditional fills, layout locks, containment | `set_diagram_shape` |
| Appearance and typography | `set_path` on returned body/path ID; `set_text` on returned label ID |
| Geometry and layer editing | Existing `set_transform`, `resize_path`, `move_node`, `align_node`, `duplicate_node`, `delete_node` |
| Automatic layouts | `layout_diagram` (vertical, horizontal, grid, mind map) |
| Data generation and refresh | `generate_diagram` (text, CSV, Mermaid, SQL; refresh existing linked data) |
| Pages | `select_project_page`, `add_project_page`, `duplicate_project_page`, `delete_project_page`, `set_project_page`, `move_project_page` |
| File and XML interchange | `import_diagram`, `export_diagram`; import returns compatibility warnings |
| Native save and other exports | `save_project`, `export_project`, `export_template_pack`, `import_project_pages` |
| History | `undo`, `redo` through the project-aware host |

This covers the implemented diagram editing workflows, not every UI-only action
or every unsupported feature of draw.io/Visio. Supported draw.io/Visio libraries can open as pages or install into the offline
stencil catalog. Grouping uses the existing `group_nodes`/`ungroup` tools; color
pickers correspond to `set_path`/`set_text`. Mouse selection, pointer gestures and
viewport rasterization are UI mechanics, not separate MCP tools. Multi-user
editing is not claimed as covered by these diagram tools.


## Supplied Visio collection

The scanner exercised 4,481 diagram/stencil candidates in `visioStencils`.
The initial importer loaded 129; after selecting stencil masters instead of
placeholder document pages it loads **318 files / 2,029 pages**. This is a parsing
and native graph validation count, not a visual-fidelity score.

- 3,506 candidates use legacy binary `.vsd`, `.vss` or `.vst` extensions and require
  conversion to actual Visio XML/OPC files.
- 701 source files are empty (including 253 with modern ZIP-based extensions) and
  cannot contain an importable drawing. The legacy-extension count above includes
  the remaining empty files.
- Three files with modern extensions also contain the legacy OLE binary signature.
  The reader now identifies that content and explains that renaming is insufficient.
- Remaining blockers include missing evaluated theme/style values, unsupported or
  incomplete geometry, XML complexity limits, and stencils exceeding the native
  100-page project limit. Unsupported/corrupt archives also remain rejected.
- Default stencils are bundled independently of these source files, so the catalog
  works offline even when a vendor file cannot be imported.
