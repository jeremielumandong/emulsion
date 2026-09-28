# Design appearance and layout through MCP

MCP exposes the same native commands as the Design editor. These tools operate on the active document/page; they preserve editable objects and create one Undo step per successful mutation. They reject changes while another edit transaction is open. Multi-target appearance and arrangement operations preflight every command, so a locked or invalid later target cannot leave an earlier target partially changed.

## Existing controls already exposed

| Design control | Existing MCP tools |
| --- | --- |
| Text content, font, size, color, paragraph alignment and line spacing | `add_text`, `set_text` |
| Rich-text character ranges and letter spacing | `format_text_range` |
| Editable curves and distortion | `set_text` with native warp settings |
| Text following or flowing inside a path | `set_text_path` |
| Shape fill/stroke, alpha, linear/radial gradients, patterns and stroke alignment | `draw_shape`, `draw_path`, `set_path` |
| Object opacity and blending | `set_opacity`, `set_blend`, advanced blending tools |
| Shadow, outline and other layer effects | `add_style`, `set_style`, `remove_style`, `set_style_blending` |
| Grouping and single-object alignment | `group_nodes`, `ungroup`, `align_node` |

Inspect the registered tool schemas for their current field names. These handlers were reused instead of adding a second implementation of text warp, paint or layer effects.

## Appearance gaps now covered

- `describe_object_appearance` returns a versioned native appearance envelope, rectangular corner geometry and recognized text-backdrop information. It does not edit the document.
- `copy_object_appearance` copies source formatting to target IDs without a clipboard operation. Target content, geometry, masks and IDs are retained. Typography is sampled from the source's first character and applied uniformly; it does not copy mixed character runs or a text warp.
- `apply_object_appearance` applies the complete version-1 envelope returned by the read tool. Partial property patches belong in `set_text`, `set_path`, `set_style` or `set_opacity`. Existing named-style links remain attached as local overrides; these operations do not create or update library definitions.
- `set_text_background` creates or updates a rounded vector rectangle grouped behind editable text. It accepts hex RGBA color, horizontal/vertical padding and radius, fits the current text bounds, and follows text rotation/scale.
- `refit_text_background` resizes an existing backdrop after text edits. Padding is inferred from its current text-local top-left inset; use `set_text_background` when exact new padding is required. Backgrounds do not automatically refit when text changes.
- `remove_text_background` removes the recognized rectangle and ungroups the text. It rejects unrelated groups, and text/background locks prevent modification.
- `set_corner_radius` rounds native axis-aligned rectangles while retaining paint. It clamps to half the shorter side and rejects arbitrary artwork and responsive-layout boundaries.
- `arrange_nodes` exposes multi-object alignment, edge/center distribution and equal-gap spacing against selected objects, canvas or the active pixel selection. Distribution needs at least three selected roots. Parent/child selections are normalized by the native command.

Examples (IDs must exist on the active page):

```json
{"tool":"copy_object_appearance","arguments":{"source":12,"nodes":[23,24]}}
{"tool":"set_text_background","arguments":{"node":23,"color":"#FFF0AACC","padding":[20,12],"radius":8}}
{"tool":"arrange_nodes","arguments":{"nodes":[23,24,25],"operation":"distribute_horizontal_gap","target":"selected_nodes"}}
```

Plain supported curved text uses the same vector-outline/Vello rendering path as UI edits. Unsupported layouts and some SVG layer effects still use appearance-preserving raster fallback. MCP does not introduce a separate rendering pipeline or stronger export guarantees.

## Responsive layout and page constraints

- `describe_design_layout` lists persisted responsive frames, their current bounds and page-resize constraints.
- `set_responsive_layout` enables or patches layout on an existing group. Use `group_nodes` first for ungrouped objects. It supports row/column/grid, frame dimensions, four-sided padding, gap, columns, wrapping, cross-axis alignment, content-driven width/height (`hug_width`, `hug_height`), optional minimum/maximum frame dimensions (`min_width`, `max_width`, `min_height`, `max_height`), `clip_content`, and [canvas width breakpoints](design-layout-breakpoints.md). Omitted fields retain existing settings; new groups use documented defaults.
- `set_layout_child` patches immediate content children: absolute positioning, fill-width/fill-height, optional minimum/maximum dimensions, and `aspect_ratio` (width divided by height). Frame boundaries are excluded. Text width changes reflow editable text instead of resizing its font when native layout supports it.
- `remove_responsive_layout` removes automatic reflow while retaining the group's current artwork and boundary rectangle.
- `set_resize_constraints` patches horizontal/vertical start, center, end, stretch or scale rules, plus text reflow. `clear_resize_constraints` restores default scale behavior. These rules affect later native page resizing; changing them alone does not move current artwork.

Absolute children retain their sizing settings but remain excluded from parent reflow. Minimum-size overflow is permitted rather than shrinking objects below their declared limits. Wrapped-row fill-height uses the frame’s available height for each row; grid fill-height uses equal rows.

Sizing patches preserve omitted fields. Set an optional dimension limit or aspect ratio to JSON `null` to clear it; booleans require `true` or `false`. Dimension limits are 1–100000 px, with each minimum no greater than its maximum. Aspect ratios range from 0.001 to 1000; incompatible ratios and dimension limits are rejected atomically. Content-driven sizing and fill cannot depend on each other on the same axis, including nested frames. A nested frame that hugs its content cannot also have an explicit child aspect ratio.

For example, enable bounded content width, then constrain a child on the other axis:

```json
{"tool":"set_responsive_layout","arguments":{"group":12,"hug_width":true,"min_width":220,"max_width":500}}
{"tool":"set_layout_child","arguments":{"node":23,"fill_height":true,"max_height":140,"aspect_ratio":2}}
{"tool":"set_layout_child","arguments":{"node":23,"aspect_ratio":null,"max_height":null}}
```

`describe_design_layout` returns persisted settings alongside native geometry, the active canvas-width threshold and the effective frame settings. `set_responsive_layout` accepts `clip_content` and a complete `breakpoints` array; `[]` clears entries, while omission preserves them. Breakpoint overrides inherit omitted or `null` fields directly from the base. The highest matching `min_width` wins, with canvas width as the stable reference. See [breakpoint parameters and examples](design-layout-breakpoints.md). The same changes are available through the native UI and remain editable and undoable. These are native layout rules, not a browser/CSS runtime.

Layout metadata is saved in native projects. Layouts retain the core limits: 256 frames per page, 511 content children per frame, 1–64 columns, spacing/padding 0–10000 px and rectangular frame dimensions 1–100000 px. Rotating or reshaping a responsive boundary requires removing its automatic layout first. Layouts use document-axis geometry, layer order and the existing core reflow behavior; they are not a separate CSS layout engine.

## Native image frames

`place_image_in_frame` accepts a native frame and an existing Raster source node. It shares embedded source pixels without changing the source node; source masks, effects and placement are not copied. Creation uses centered cover fitting. Replacement preserves the existing frame-image ID and resets that image's mask/crop, matching the native replacement command. Locked frame content is rejected.

`fit_frame_image` provides cover, contain and stretch plus normalized focal point. It changes native placement without resampling pixels, preserves clipping and the image's rotation/flips, and can be undone. Focal point affects cover fitting only.

These handlers perform no file import, network fetch or image decoding. Use the separate image-import workflow to bring an image into the document first, then pass its Raster node ID. A general multi-shape illustration group is not automatically treated as an unambiguous image frame. Existing native vector frames, frame boundaries and their clipped images are supported.

## Verification

`design_appearance_tools_tests.rs` covers capture/copy/apply, invalid payloads, locks, active transactions, native background creation/refit/removal, rectangular corners, equal-gap arrangement and one-step Undo. `design_layout_tools_tests.rs` covers enable/update/remove, child settings, invalid or locked reflow without orphan boundaries, constraints, frame image replacement/fitting and unchanged source pixels. Inline sizing regressions in `design_layout_tools.rs` cover patch/clear semantics, JSON inspection, bounded aspect ratios, protected targets, invalid types, conflicting rules, and one-step Undo. UI controls and MCP share `emulsion_core::design_formatting` for corner and text-backdrop operations.
