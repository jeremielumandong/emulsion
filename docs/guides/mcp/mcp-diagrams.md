# MCP: native diagrams

These tools work against the active document's structured diagram graph. Shapes and connectors retain editable paths, text, attachments, ports, conditional fills and container metadata. They use the same core operations as the native editor.

| Tool | Purpose |
| --- | --- |
| `describe_diagram` | Read shape/connector IDs, labels, bounds and stored graph metadata. |
| `list_diagram_stencils` | Discover bundled stencils, with optional text query and exact category filter. |
| `add_diagram_shape` | Add a native shape using a kind, `[x,y,width,height]` bounds and optional label. |
| `insert_diagram_stencil` | Insert a bundled stencil by its discovered ID and bounds. |
| `add_diagram_connector` | Attach two existing shapes with ports, routing and optional label. |
| `set_diagram_connector` | Patch endpoints, routing, waypoints, relative label position/offset, arrowheads, width, RGBA color, dash pattern, double lines, label backgrounds, corner radius, crossings and reversal. |
| `set_diagram_object_details` | Set/clear a shape’s note, alt text and HTTP(S) link. |
| `copy_diagram_style` | Copy appearance from a source shape/connector to target objects without changing geometry or captions. |
| `list_document_stencils` | Discover reusable imported/current-page shapes and their source IDs. |
| `insert_document_stencil` | Place a reusable source object at `center: [x,y]`, preserving artwork/style and excluding attached edges/container contents. |
| `set_diagram_shape` | Patch label, replace data/conditional rules, toggle layout lock, or change container. |
| `layout_diagram` | Arrange unlocked top-level shapes in vertical, horizontal, grid or mind-map layout. |

Create an attached connector with:

```json
{"source":{"shape":12,"port":"east"},"target":{"shape":19,"port":{"custom":{"x":0,"y":0.5}}},"label":"Approved","routing":"orthogonal"}
```

Ports are `auto`, `north`, `east`, `south`, `west`, or a `custom` object with coordinates relative to the shape bounds. Normalized coordinates usually range from 0 to 1; the native model also supports bounded outside ports for imported diagrams. Moving a connected shape through existing movement tools reroutes its attached edges.

A shape-data and conditional-fill update can use:

```json
{"node":12,"data":{"status":"complete"},"conditions":[{"field":"status","equals":"complete","color":[20,180,80,255]}],"layout_locked":true}
```

`data` replaces user fields while preserving read-only `emulsion_*`/`drawio_*` metadata; `conditions` replaces the rules; omitted properties are unchanged. Set `container` to a container shape ID, or `null` to remove containment. Cycles and non-container targets are rejected. Automatic layout respects layout/position locks and moves containers with their children.

Each mutation uses one Undo step and rejects an in-progress interactive transaction. Property edits preflight protected descendants, labels and graph validation before committing; invalid arguments do not partially update the diagram. Mutating property/layout tools carry the host's destructive classification. The discovery tools are read-only. Library package import/export, custom stencil authoring and page navigation are handled by the project/library host tools, rather than these bundled-stencil operations.


## Page-aware diagram workflows

| Tool | Purpose |
| --- | --- |
| `quick_create_diagram` | Add a connected neighbor in a cardinal direction. |
| `generate_diagram` | Generate a new page from text, CSV, Mermaid or SQL; `refresh=true` updates linked data on the active page. |
| `import_diagram` | Import every page from a supported local file or supplied draw.io XML, returning warnings and saved stencil pack IDs. `save_stencils` defaults to false; permanence requires explicit opt-in. |
| `save_document_stencils` | Save the open diagram’s reusable objects into deduplicated persistent packs; optional `name`. |
| `export_diagram` | Export all pages as editable draw.io; omit `path` to return XML. Existing files require `overwrite=true`. |

These tools require an open Diagram project. Existing project tools handle page
selection, add/duplicate/delete/reorder/rename, native saving, PDF and image
archives, and template/stencil packs. See the [functionality audit](../diagram-functionality.md)
for sample coverage, the default catalog and remaining compatibility limits.


## Templates, themes and stencil packs

| Tool | Purpose |
| --- | --- |
| `list_diagram_library` | Discover the 18 bundled templates and nine theme IDs. |
| `insert_diagram_template` | Insert a discovered template as a new editable page, with one undo step. |
| `apply_diagram_theme` | Apply a theme to the whole diagram or supplied `nodes`, atomically and with undo. |
| `list_diagram_stencil_packs` | Inspect installed offline stencil packs and their entry paths. |
| `insert_diagram_pack_entry` | Place an installed entry on the active canvas, preserving editable objects and connections, with one undo step. |
| `install_diagram_stencil_pack` | Install a supported local draw.io/Visio library, SVG file/folder or native pack; returns import warnings and refreshes the UI catalog. |

Installed entries use the `insert_diagram_pack_entry` tool. The color picker uses
native path/text operations (`set_path`, `set_text`); grouping uses `group_nodes`
and `ungroup`. Template/theme mutations share core commands with the gallery.
Stencil installation changes the local library rather than document history.

## Curves, marker styles and bundled vendor packs

`set_diagram_connector` accepts `routing: "curved"` and partial `start_marker` /
`end_marker` objects. Omitted marker fields retain their values. Supported kinds
are `none`, `block`, `classic`, `open`, `diamond`, `oval`, `circle_plus`, `many`,
`one`, `mandatory_one`, `zero_to_one`, `zero_to_many`, and `one_to_many`.
`size` is 1–100 document pixels; `filled` is boolean. `arrow_start` and
`arrow_end` enable each endpoint's marker independently. Invalid patches leave
the document and undo history unchanged.

```json
{"node":42,"routing":"curved","arrow_start":true,"start_marker":{"kind":"diamond","filled":false,"size":16},"end_marker":{"kind":"zero_to_many","size":18}}
```

`list_diagram_stencil_packs` also returns the available offline vendor packs.
Install one using `install_diagram_stencil_pack` with `{"pack":"aws4"}`. Supply
exactly one of `pack` (bundled ID) or `path` (local stencil source). Installation
returns compatibility notes. Native entries are then placed with
`insert_diagram_pack_entry`, which preserves connections and supports undo.
Visio conversion and vector-source preservation use the same importer as the UI.

## Crossing bridges and larger libraries

`set_diagram_connector` accepts `jump_style: "none" | "arc" | "gap" | "sharp"`
and `jump_size` from 1 to 100 document pixels. Bridges are recalculated when
connected objects move; straight, orthogonal, curved and cyclical routes are supported. Imported
additional labels appear in each edge's `labels` metadata and remain editable
through normal text commands. Invalid patches are atomic and undo restores the
previous route and crossing style.

```json
{"node":42,"routing":"orthogonal","jump_style":"arc","jump_size":12}
```

Bundled families expose all available stencil definitions. The UI pages results
in groups of 96; MCP listing and insertion use the same complete catalog. Limits
are 4,096 project pages, 10,000 shapes and 20,000 edges per diagram page.

## Floating connector controls and document stencils

The floating toolbar shares native connector commands with MCP. `routing` accepts `straight`, `orthogonal`, `curved`, or `cyclical`. `width` is 0.25–100 px; `color` is RGBA; `dash` is an array of up to six alternating lengths (empty resets to solid); `corner_radius` is 0–100 px. `reverse: true` swaps attachments and reverses manual bends while retaining endpoint marker roles. Label positions remain attached to the route. Curved/cyclical routes do not use corner rounding or crossing bridges.

```json
{"node":42,"routing":"orthogonal","corner_radius":6,"width":1,"color":[75,81,89,255],"dash":[8,5],"reverse":true,"end_marker":{"kind":"block","size":10}}
```

After importing a diagram, call `list_document_stencils`, then `insert_document_stencil` with a returned source ID:

```json
{"source":12,"center":[600,400]}
```

The inserted object has independent IDs, editable vector/text content and no copied external attachments. One Undo removes the insertion. Original embedded bitmap imagery remains bitmap imagery.

## Connector attachments, label placement and persistent shapes

An endpoint's `shape` may also identify a connector. For these attachments,
`port: {"custom":{"x":0.25,"y":0}}` attaches at 25% of its route length;
`auto` uses the midpoint. Connector dependencies follow rerouting; cycles fail
atomically. Deleting a parent connector also removes dependent branches.

`set_diagram_connector` accepts `double_line` (boolean), `label_background`
(RGBA array or `null`), `label_position` (-1 source, 0 midpoint, 1 target), and
`label_normal` (perpendicular displacement in document units). Decorations remain
editable vector nodes and survive native persistence and SVG export.

```json
{"node":42,"double_line":true,"label":"Traffic","label_background":[238,244,255,255],"label_position":0.25,"label_normal":16}
```

Live-editor imports and stencil pack generation perform file parsing and library
serialization in background work. The import checks that the target document has
not changed before insertion. Library persistence does not add document undo steps;
imports only expose temporary page shapes by default. Use `save_stencils: true`,
`save_document_stencils`, or `install_diagram_stencil_pack` to keep a permanent pack.
Use `remove_creative_asset` to remove a saved pack from the library.


## Structured objects, defaults and local review

- `get_diagram_structure {node}` reads fields; `set_diagram_structure` accepts
  `{node, kind: "class" | "entity", fields: {title, attributes: [], methods: []}}`.
  Methods apply only to classes. The operation reflows editable compartments in one undo step.
- `get_diagram_review {}` reads saved page defaults, thumbnail IDs and comment threads.
- `set_diagram_default_style {source, connector?: false}` captures appearance;
  omit `source` to reset that default. It affects subsequent insertions.
- `set_diagram_thumbnail {nodes: [...]}` selects preview bounds; an empty array resets it.
- `add_diagram_comment {node, author, text, thread?}` starts a local thread or replies.
  `resolve_diagram_comment {thread, resolved}` resolves/reopens it;
  `delete_diagram_comment {thread}` removes it. Mutations support undo.
- `create_diagram_link {nodes?: [...], view?: [center_x, center_y, zoom, rotation]}`
  returns a reference to the current saved project/page without changing history.
  `open_diagram_link {link}` checks the matching project and switches pages. The
  live editor also applies the selection/camera; a headless host receives those values.

`describe_diagram` includes each connector's optional `routing_warning`. A warning
means automatic routing found no clear corridor within its bounded search; add
manual waypoints or move overlapping objects. Clearing the obstacle lets routing
clear the warning on the next geometry update.
