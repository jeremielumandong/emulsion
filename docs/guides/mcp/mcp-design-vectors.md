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

## Vector stroke layers (pencil lines)

A vector stroke layer (kind `strokes` in `describe_document`, with `strokes`, `points`, `fills` and `stroke_bounds`) keeps pencil lines as editable centrelines. Each point carries `width`, a pressure multiplier of the stroke's `line_width` (0–8), and `opacity` (0–1), both interpolated along the line, so a stroke can taper and fade like a pencil and still be smoothed, retouched, recoloured or moved later. Fills (from `outline_vector_strokes`) draw under the strokes. These tools work on any page, respect layer locks and read-only (locked storyboard) panels, validate the whole call before changing anything and are one Undo step each. Coordinates are document pixels; colours are `#RRGGBB` or `#RRGGBBAA`.

| Tool | Arguments and behavior |
| --- | --- |
| `add_vector_layer` | Optional `name`, `above` (node), and first `strokes` with call defaults `color`/`line_width` (defaults for those strokes only; the layer stores none). Returns the new node ID. |
| `draw_vector_strokes` | `node`, `strokes` (1–400): each `{points:[{x,y,width?,opacity?}], color?, line_width?, closed?}` with up to 5000 points; call-level `color` and `line_width` (default black, 4 px). Returns the new stroke indices. |
| `draw_vector_shapes` | `node`, `shapes` (1–400): `line` (exactly 2 `points`), `polyline` (2+ `points`, optional `closed`), `rectangle`/`ellipse` (`x`, `y`, `width`, `height`, closed strokes). Points may carry width and opacity. |
| `describe_vector_strokes` | `node`, `offset`, `limit` (1–200, default 50), `include_points`. Returns each stroke's `index`, `color`, `line_width`, `closed`, `point_count`, `bounds`, and `width`/`opacity` min–max, and `next_offset`. With points, a page stops at about 5000 points. Read-only. |
| `edit_vector_strokes` | `node`, optional `strokes` indices (default all), then any of `color`, `line_width`, `smooth` (0–1) with `smooth_iterations` (default 2), `simplify` (tolerance in px), `scale` and `rotation` (degrees clockwise) about `origin` `{x,y}` (default the strokes' centre), `dx`/`dy`. Applied in that order; scaling scales line widths too. |
| `delete_vector_strokes` | `node`, `strokes` indices. Later strokes move down. |
| `erase_vector_strokes` | `node`, a path of `points` `{x,y}` and `radius`. Cuts strokes exactly where the eraser crosses them; closed strokes open at the cut. Fills are untouched. Returns `changed`; a miss changes nothing. |
| `retouch_vector_strokes` | `node`, `points` path, `mode` (`thicker`, `thinner`, `opaquer`, `fainter`, `smooth`), `radius`, `amount` (0–1, default 0.5, per dab; dabs every half radius), optional `strokes`. Strongest at the brush centre, nothing at the radius; adds points under the brush where a segment is long. |
| `outline_vector_strokes` | `node`, `strokes` indices; replaces them with fills of their drawn outline in the same colour. |

A tapered line, thin at both ends:

```json
{"name":"draw_vector_strokes","arguments":{"node":3,"line_width":8,"strokes":[{"points":[{"x":100,"y":400,"width":0.1},{"x":300,"y":380,"width":1},{"x":500,"y":405,"width":0.1}]}]}}
```

Thicken its middle later without redrawing:

```json
{"name":"retouch_vector_strokes","arguments":{"node":3,"points":[{"x":260,"y":382},{"x":340,"y":382}],"mode":"thicker","radius":40,"amount":0.6}}
```

Use vector strokes for clean line work that should stay editable; use `paint` with raster brushes for tone, texture and rough marks.
