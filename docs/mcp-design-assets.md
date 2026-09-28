# MCP: Design assets

MCP exposes the same native Design component, saved-style, chart and table operations as the editor. Each tool acts on the active page. Mutation results return JSON text with affected native node IDs; list results expose stored definitions and links. Invalid requests and protected changes leave the page unchanged. Each change forms one Undo step. Mutations refuse to interrupt an active interactive edit.

| Feature | Tools |
| --- | --- |
| Components | `list_design_components`, `create_design_component`, `insert_design_component`, `update_design_component`, `save_design_component_variant`, `reset_design_component`, `switch_design_component`, `detach_design_component`, `set_design_component_overrides` |
| Saved styles | `list_design_styles`, `create_design_style`, `apply_design_style`, `update_design_style`, `reset_design_style`, `detach_design_style`, `rename_design_style`, `remove_design_style` |
| Charts and tables | `list_design_charts`, `add_design_chart`, `update_design_chart`, `detach_design_chart` |

For example, `create_design_component` accepts `{"nodes":[12,13],"name":"Card"}` and returns its native group ID. Insert it with `{"name":"Card","variant":"Default","offset":[200,0]}`. Component publishing propagates within the page through an acyclic nested dependency graph. Matched member IDs and explicit property overrides survive updates. Unrelated variants remain unchanged.

`set_design_component_overrides` accepts `{"node":13,"overrides":{"content":true,"opacity":true}}`. It targets the innermost owning instance by default; supply `instance` to name an enclosing linked group. Flags replace the previous set; omitted booleans are false and `{}` clears all flags. Available groups are `content`, `appearance`, `geometry`, `opacity`, and `visibility`. Appearance groups paint, typography, blend and effects; content supports text/raster objects, and geometry supports text/path/image objects. Reset or variant switching restores the saved source and clears flags. See [component behavior and limits](design-components.md).

Save appearance with `create_design_style` using `{"node":12,"name":"Heading"}`, then apply it using `{"nodes":[19,24],"name":"Heading"}`. Updates publish to linked consumers. Content and geometry remain independent. Text styles sample the source's first character and apply uniform typography to consumers; publishing does not flatten the source's rich-text runs.

`add_design_chart` and `update_design_chart` accept `kind` (`bar`, `line`, `pie`, `table`), `title`, `rows`, `colors` (RGBA byte arrays), `size` and `origin`. An update also requires `node`; omitted fields are retained. Rows contain a header followed by data, with numeric values encoded as strings. For example:

```json
{"kind":"bar","title":"Revenue","rows":[["Region","Revenue"],["North","125.5"],["South","98"]],"size":[600,400],"origin":[20,40]}
```

Charts use native editable paths and text. Updating data replaces chart children, while detaching keeps the artwork and removes its data link. Validation includes rectangular data, supported chart types, numeric ranges, nonnegative pie values, bounded sizes and color channels.

The three list tools are read-only. Publish/reset/switch, detachment, style removal and chart-data replacement have destructive annotations because they replace overrides or remove associations; they remain undoable. Cross-page asset import and project navigation belong to the project host API, outside these active-page tools. This module does not imply that every application feature has MCP coverage.
