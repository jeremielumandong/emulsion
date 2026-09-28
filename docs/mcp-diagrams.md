# MCP: native diagrams

These tools work against the active document's structured diagram graph. Shapes and connectors retain editable paths, text, attachments, ports, conditional fills and container metadata. They use the same core operations as the native editor.

| Tool | Purpose |
| --- | --- |
| `describe_diagram` | Read shape/connector IDs, labels, bounds and stored graph metadata. |
| `list_diagram_stencils` | Discover bundled stencils, with optional text query and exact category filter. |
| `add_diagram_shape` | Add a native shape using a kind, `[x,y,width,height]` bounds and optional label. |
| `insert_diagram_stencil` | Insert a bundled stencil by its discovered ID and bounds. |
| `add_diagram_connector` | Attach two existing shapes with ports, routing and optional label. |
| `set_diagram_connector` | Patch endpoints, routing, waypoints, label offsets and arrowheads. |
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

Each mutation uses one Undo step and rejects an in-progress interactive transaction. Property edits preflight protected descendants, labels and graph validation before committing; invalid arguments do not partially update the diagram. Mutating property/layout tools carry the host's destructive classification. The two discovery tools are read-only. Library package import/export, custom stencil authoring and page navigation are handled by the project/library host tools, rather than these bundled-stencil operations.


## Page-aware diagram workflows

| Tool | Purpose |
| --- | --- |
| `quick_create_diagram` | Add a connected neighbor in a cardinal direction. |
| `generate_diagram` | Generate a new page from text, CSV, Mermaid or SQL; `refresh=true` updates linked data on the active page. |
| `import_diagram` | Import every page from a supported local file or supplied draw.io XML, returning warnings. |
| `export_diagram` | Export all pages as editable draw.io; omit `path` to return XML. Existing files require `overwrite=true`. |

These tools require an open Diagram project. Existing project tools handle page
selection, add/duplicate/delete/reorder/rename, native saving, PDF and image
archives, and template/stencil packs. See the [functionality audit](diagram-functionality.md)
for sample coverage, the default catalog and remaining compatibility limits.
