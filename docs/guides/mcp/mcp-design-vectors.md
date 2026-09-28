# MCP native vectors and precision

These tools operate on the active page. Geometry edits use the same native core operations as the UI, respect locks, and create one Undo step. The source remains editable. Coordinates in vector tools are document pixels.

| Tool | Arguments and behavior |
| --- | --- |
| `inspect_vector_path` | `node`; reads subpaths, anchors, cubic handles, flags and style. |
| `set_vector_point` | `node`, `subpath`, `anchor`, `position:[x,y]`; optional incoming/outgoing handles and smooth flag. Omitted handles translate with the point. Smooth handles must be opposite and collinear. |
| `join_vector_subpaths` | `node`, `first`, `second`; joins two open subpaths. |
| `split_vector_subpath` | `node`, `subpath`, `anchor`; opens a closed contour or splits an open contour at an interior anchor. |
| `combine_vector_paths` | `nodes`, `operation`: component/union/subtract/intersect/exclude. Retains the first node and style, removes other operands atomically. |
| `skew_vector_path` | `node`, horizontal/vertical degrees, `origin:[x,y]`; preserves handles. |
| `warp_vector_mesh` | `node`, `columns`, `rows`, row-major `points`, optional sampling tolerance. 2–16 controls per axis; native sampled contours. |
| `perspective_vector_path` | `node`, four clockwise TL/TR/BR/BL corners, optional sampling tolerance; true projective mapping. |
| `create_stroke_outline` | `node`; creates a separate solid-stroke outline, retaining the source. Returns new node ID. |
| `trace_bitmap_to_vector` | `node`, optional options: resolution, threshold, alpha_only, invert, RGBA color. Local monochrome tracing, source retained. |
| `find_matching_objects` | `node`, property: kind/fill/stroke/opacity/font. Returns visible matching IDs without changing selection/history. |
| `get_design_precision` | No arguments; reads page measurement settings and document DPI. |
| `set_design_precision` | Optional unit and pixel-space origin. Omission preserves settings; null is invalid. |
| `position_design_object` | `node`, `x`, `y` in selected page units relative to ruler origin. |
| `space_design_objects` | `nodes`, `gap` in page units, optional vertical flag. First object in geometric order stays fixed. |

The existing shape fill/stroke paint schema accepts gradients with either an `end` color or `stops` (not both). Stops contain numeric `offset` from 0 to 1 and `color` in #RRGGBB or #RRGGBBAA form. Provide 2–16 ordered stops; equal positions create hard boundaries. Linear gradients accept an angle; radial gradients do not. Shape inspection returns all stops losslessly.

Warp tolerance is 0.05–10 document pixels; curves become sampled polylines. Stroke outline copies currently require solid paint and do not copy layer masks/effects. Bitmap trace uses bounded threshold contours, not multicolor vectorization. See [native UI behavior and limitations](../design-vector-editing.md).
