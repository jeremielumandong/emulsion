//! Tool definitions. Names are prefixed `mcp__emulsion__` by the CLI.

use crate::server::ToolDef;
use serde_json::{Value, json};

/// Tools that only read; the CLI may run them without asking.
pub const READ_ONLY: &[&str] = &[
    "describe_document",
    "get_view",
    "list_history",
    "compare",
    "list_brushes",
    "list_recipes",
    "critique",
    "list_fonts",
    "list_models",
];

/// Tools whose effect is hard to see or undo at a glance; always confirmed.
pub const DESTRUCTIVE: &[&str] = &[
    "delete_node",
    "ungroup",
    "undo",
    "crop",
    "image_size",
    "canvas_size",
    "merge_branch",
];

/// Tools that compute for a while; hosts run them off the UI thread.
pub const HEAVY: &[&str] = &[
    "select_color",
    "content_aware_fill",
    "paint",
    "hatch",
    "add_filter",
    "set_filter",
    "remove_filter",
    "download_model",
    "select_subject",
    "select_by_points",
    "remove_background",
];

const BLEND_MODES: &[&str] = &[
    "normal",
    "dissolve",
    "darken",
    "multiply",
    "color burn",
    "linear burn",
    "darker color",
    "lighten",
    "screen",
    "color dodge",
    "linear dodge",
    "lighter color",
    "overlay",
    "soft light",
    "hard light",
    "vivid light",
    "linear light",
    "pin light",
    "hard mix",
    "difference",
    "exclusion",
    "subtract",
    "divide",
    "hue",
    "saturation",
    "color",
    "luminosity",
    "pass through",
];

pub const ADJUSTMENTS: &[&str] = &[
    "exposure",
    "brightness_contrast",
    "levels",
    "hue_saturation",
    "white_balance",
    "invert",
];

fn node() -> Value {
    json!({ "type": "integer", "description": "Node id from describe_document." })
}

fn mode() -> Value {
    json!({ "type": "string", "enum": ["replace", "add", "subtract", "intersect"] })
}

fn def(name: &str, description: &str, properties: Value, required: &[&str]) -> ToolDef {
    ToolDef {
        name: name.into(),
        description: description.into(),
        input_schema: json!({
            "type": "object",
            "properties": properties,
            "required": required,
            "additionalProperties": false,
        }),
    }
}

pub fn definitions() -> Vec<ToolDef> {
    vec![
        def(
            "describe_document",
            "Describe the open document: canvas size, every node from the top of the stack down \
             (id, row, depth, name, kind, visibility, opacity, blend mode, clipping, adjustment \
             parameters, placement), and recent history. Call this before changing anything.",
            json!({}),
            &[],
        ),
        def(
            "get_view",
            "Render the current composite (or one node on its own) as a PNG so you can see the \
             image. Use it to ground decisions in what the picture actually shows.",
            json!({
                "node": { "type": "integer", "description": "Render only this node (and its children)." },
                "max_size": { "type": "integer", "minimum": 64, "maximum": 1568, "description": "Longest side in pixels, default 1024." }
            }),
            &[],
        ),
        def(
            "set_visibility",
            "Show or hide a node.",
            json!({ "node": node(), "visible": { "type": "boolean" } }),
            &["node", "visible"],
        ),
        def(
            "rename_node",
            "Rename a node.",
            json!({ "node": node(), "name": { "type": "string", "minLength": 1 } }),
            &["node", "name"],
        ),
        def(
            "set_opacity",
            "Set a node's opacity in percent.",
            json!({ "node": node(), "opacity": { "type": "number", "minimum": 0, "maximum": 100 } }),
            &["node", "opacity"],
        ),
        def(
            "set_blend_mode",
            "Set a node's blend mode. 'pass through' is only valid for groups.",
            json!({ "node": node(), "mode": { "type": "string", "enum": BLEND_MODES } }),
            &["node", "mode"],
        ),
        def(
            "move_node",
            "Move a node in the stack. Give exactly one of: above, below, into_group, or to.",
            json!({
                "node": node(),
                "above": { "type": "integer", "description": "Place directly above this node, in its parent." },
                "below": { "type": "integer", "description": "Place directly below this node, in its parent." },
                "into_group": { "type": "integer", "description": "Place at the top of this group." },
                "to": { "type": "string", "enum": ["top", "bottom"], "description": "Top or bottom of the node's current parent." }
            }),
            &["node"],
        ),
        def(
            "group_nodes",
            "Wrap nodes in a new group placed where the topmost of them was.",
            json!({ "nodes": { "type": "array", "items": { "type": "integer" }, "minItems": 1 }, "name": { "type": "string" } }),
            &["nodes"],
        ),
        def(
            "ungroup",
            "Replace a group by its children.",
            json!({ "node": node() }),
            &["node"],
        ),
        def(
            "delete_node",
            "Delete a node and everything inside it.",
            json!({ "node": node() }),
            &["node"],
        ),
        def(
            "duplicate_node",
            "Duplicate a node (and its children) directly above it.",
            json!({ "node": node() }),
            &["node"],
        ),
        def(
            "add_adjustment",
            "Add an adjustment node. It changes everything below it in the same parent. Kinds and parameters: \
             exposure {exposure -5..5 ev, offset -0.5..0.5, gamma 0.1..3}; brightness_contrast \
             {brightness -150..150, contrast -50..100}; levels {in_black 0..253, in_white 2..255, gamma \
             0.1..9.99, out_black, out_white 0..255}; curves {points: [[in,out],...] on 0..255 for the master \
             curve, plus red/green/blue arrays}; hue_saturation {hue -180..180, saturation -100..100, \
             lightness -100..100}; color_balance {shadows_cr, shadows_mg, shadows_yb, midtones_*, highlights_* \
             -100..100, preserve_luminosity 0/1}; vibrance {vibrance, saturation -100..100}; black_and_white \
             {reds, yellows, greens, cyans, blues, magentas -200..300, tint_hue 0..360, tint_strength 0..100}; \
             photo_filter {hue 0..360, saturation 0..100, density 0..100, preserve_luminosity 0/1}; gradient_map \
             {stops: [[0, \"#000000\"], [1, \"#ffffff\"]], reverse 0/1}; grain {amount 0..100, size 0.5..8, \
             monochrome 0/1}; white_balance {temperature -100..100 (warmth), tint -100..100}; threshold {level \
             1..255}; posterize {levels 2..256}; lut {lut_file: path to a .cube, strength 0..100}; invert {}.",
            json!({
                "kind": { "type": "string", "enum": ADJUSTMENTS },
                "params": { "type": "object", "additionalProperties": { "type": "number" } },
                "above": { "type": "integer", "description": "Insert directly above this node; default is the top of the stack." },
                "name": { "type": "string" }
            }),
            &["kind"],
        ),
        def(
            "set_adjustment",
            "Change parameters of an adjustment node. Keys as listed for add_adjustment.",
            json!({ "node": node(), "params": { "type": "object", "additionalProperties": { "type": "number" } } }),
            &["node", "params"],
        ),
        def(
            "set_transform",
            "Place a pixel node: position of its top-left corner in document pixels, uniform scale in \
             percent (source pixels are kept, so this is lossless), clockwise rotation in degrees, flips.",
            json!({
                "node": node(),
                "x": { "type": "number" }, "y": { "type": "number" },
                "scale": { "type": "number", "exclusiveMinimum": 0, "maximum": 10000 },
                "rotation": { "type": "number" },
                "flip_x": { "type": "boolean" }, "flip_y": { "type": "boolean" }
            }),
            &["node"],
        ),
        def(
            "select_rect",
            "Select a rectangle in document pixels. mode: replace (default), add, subtract, intersect. feather softens the edge by that many pixels.",
            json!({ "x": { "type": "number" }, "y": { "type": "number" }, "width": { "type": "number", "exclusiveMinimum": 0 }, "height": { "type": "number", "exclusiveMinimum": 0 }, "mode": mode(), "feather": { "type": "number", "minimum": 0 } }),
            &["x", "y", "width", "height"],
        ),
        def(
            "select_ellipse",
            "Select the ellipse inscribed in a rectangle. Same arguments as select_rect.",
            json!({ "x": { "type": "number" }, "y": { "type": "number" }, "width": { "type": "number", "exclusiveMinimum": 0 }, "height": { "type": "number", "exclusiveMinimum": 0 }, "mode": mode(), "feather": { "type": "number", "minimum": 0 } }),
            &["x", "y", "width", "height"],
        ),
        def(
            "select_color",
            "Magic wand on the visible image: select pixels within tolerance (0-255) of the colour at (x, y); contiguous (default true) keeps it to the connected area.",
            json!({ "x": { "type": "number" }, "y": { "type": "number" }, "tolerance": { "type": "integer", "minimum": 0, "maximum": 255 }, "contiguous": { "type": "boolean" }, "mode": mode() }),
            &["x", "y"],
        ),
        def(
            "select_node",
            "Select what a node covers: a pixel node's visible pixels (through its mask), or an adjustment's mask.",
            json!({ "node": node(), "mode": mode() }),
            &["node"],
        ),
        def(
            "transform_selection",
            "Move the selection by dx/dy pixels, scale it about its centre (1 = unchanged) and rotate it clockwise in degrees.",
            json!({ "dx": { "type": "number" }, "dy": { "type": "number" }, "scale": { "type": "number", "exclusiveMinimum": 0, "maximum": 20 }, "rotation": { "type": "number", "minimum": -360, "maximum": 360 } }),
            &[],
        ),
        def(
            "draw_path",
            "Add a vector Path node from SVG path data (M L H V C S Q T Z, absolute or relative; no arcs). It stays editable. stroke and fill are #RRGGBB or \"none\"; width is the stroke width in pixels. Use it for clean outlines, shapes, lettering and anything that should be crisp and adjustable; use paint for painterly marks.",
            json!({ "d": { "type": "string", "minLength": 3 }, "name": { "type": "string" }, "stroke": { "type": "string" }, "width": { "type": "number", "minimum": 0, "maximum": 500 }, "fill": { "type": "string" }, "above": node() }),
            &["d"],
        ),
        def(
            "set_path",
            "Change a Path node: new path data d, and/or stroke, width, fill (#RRGGBB or \"none\"). describe_document shows each path's current d.",
            json!({ "node": node(), "d": { "type": "string" }, "stroke": { "type": "string" }, "width": { "type": "number", "minimum": 0, "maximum": 500 }, "fill": { "type": "string" } }),
            &["node"],
        ),
        def(
            "path_to_selection",
            "Make the selection from the inside of a Path node (non-zero winding).",
            json!({ "node": node(), "mode": mode() }),
            &["node"],
        ),
        def(
            "list_recipes",
            "Film recipes available: the starter set and the person's saved ones, with their film simulation, tags and settings. Base looks that a recipe can name: see `looks`.",
            json!({}),
            &[],
        ),
        def(
            "apply_recipe",
            "Apply a film recipe as a group of adjustment nodes above the given node (or at the top). Give one of: name (from list_recipes), text (a pasted settings block like Fuji X Weekly's: 'Film Simulation: Classic Chrome', 'Grain Effect: Weak, Small', 'White Balance: Daylight, +2 Red & -4 Blue', 'Highlight: -1' …), or toml (a .recipe.toml). save=true also keeps a text or toml recipe for later.",
            json!({ "name": { "type": "string" }, "text": { "type": "string" }, "toml": { "type": "string" }, "save": { "type": "boolean" }, "above": node() }),
            &[],
        ),
        def(
            "add_style",
            "Add a layer style (an effect drawn from the node's shape) to a pixel, smart or path node. Kinds: drop_shadow {opacity 0..100, angle, distance 0..100, size 0..60}, inner_shadow {same}, outer_glow {opacity, size 0..80}, stroke {opacity, size 1..40}, color_overlay {opacity}, gradient_overlay {angle, opacity; color and color2 are its ends}. color is #RRGGBB.",
            json!({ "node": node(), "kind": { "type": "string" }, "params": { "type": "object" }, "color": { "type": "string" }, "color2": { "type": "string" } }),
            &["node", "kind"],
        ),
        def(
            "set_style",
            "Change a layer style at index (params, color, color2).",
            json!({ "node": node(), "index": { "type": "integer", "minimum": 0 }, "params": { "type": "object" }, "color": { "type": "string" }, "color2": { "type": "string" } }),
            &["node", "index"],
        ),
        def(
            "remove_style",
            "Remove the layer style at index.",
            json!({ "node": node(), "index": { "type": "integer", "minimum": 0 } }),
            &["node", "index"],
        ),
        def(
            "convert_to_smart",
            "Make a pixel layer a smart layer (filters stay editable; painting on it is not possible), or rasterize a smart layer back to pixels.",
            json!({ "node": node() }),
            &["node"],
        ),
        def(
            "add_filter",
            "Add a filter to a smart layer's stack. Kinds and parameters: gaussian_blur {radius 0..100}; box_blur {radius}; motion_blur {angle -180..180, distance 0..200}; lens_blur {radius 0..40}; unsharp_mask {amount 0..500 %, radius 0.1..50, threshold 0..255}; smart_sharpen {amount, radius}; add_noise {amount 0..100, monochrome 0/1}; reduce_noise {strength 0..10, detail 0..100}; high_pass {radius}; lens_correction {distortion -100..100, vignette -100..100}; emboss {angle, height 1..20, amount}; find_edges {}; pinch {amount -100..100}; twirl {angle}; wave {amplitude, wavelength}. Blurs spread past the layer's edges.",
            json!({ "node": node(), "kind": { "type": "string" }, "params": { "type": "object" } }),
            &["node", "kind"],
        ),
        def(
            "set_filter",
            "Change parameters of the filter at index on a smart layer (indices from describe_document).",
            json!({ "node": node(), "index": { "type": "integer", "minimum": 0 }, "params": { "type": "object" } }),
            &["node", "index", "params"],
        ),
        def(
            "remove_filter",
            "Remove the filter at index from a smart layer.",
            json!({ "node": node(), "index": { "type": "integer", "minimum": 0 } }),
            &["node", "index"],
        ),
        def(
            "critique",
            "A fast, measured critique of the picture as it stands: value range and grouping, where the detail sits, balance, edge character, colour temperature, symmetry, empty space, ranked by what to fix first (Jev ranks when configured). Free and instant; every paint and hatch result also carries its top two lines.",
            json!({ "count": { "type": "integer", "minimum": 1, "maximum": 8 } }),
            &[],
        ),
        def(
            "hatch",
            "Shade an area with parallel strokes: fill rect [x, y, width, height] (or the selection's bounds) with lines at angle (degrees, default 45) every spacing pixels (default 8), with a little jitter (0-1) so they look hand-made; cross=true adds a second direction. Uses a brush and color like paint. One undo step.",
            json!({ "node": node(), "brush": { "type": "string" }, "color": { "type": "string" }, "settings": { "type": "object" }, "rect": { "type": "array", "items": { "type": "number" }, "minItems": 4, "maxItems": 4 }, "angle": { "type": "number" }, "spacing": { "type": "number", "minimum": 1 }, "jitter": { "type": "number", "minimum": 0, "maximum": 1 }, "cross": { "type": "boolean" } }),
            &["node"],
        ),
        def(
            "add_text",
            "Add an editable text layer. text may contain newlines for paragraphs; x, y is the top-left of the text box in pixels; size is the font size in pixels; color is #RRGGBB; font is a family name from list_fonts (empty for the default sans). width wraps lines. Use it for titles, captions, speech-bubble lettering and any text that must stay editable. Returns the node id.",
            json!({ "text": { "type": "string" }, "x": { "type": "number" }, "y": { "type": "number" }, "size": { "type": "number", "minimum": 1, "maximum": 4000 }, "color": { "type": "string" }, "font": { "type": "string" }, "bold": { "type": "boolean" }, "italic": { "type": "boolean" }, "align": { "type": "string", "enum": ["left", "center", "right", "justify"] }, "width": { "type": ["number", "null"], "description": "Wrap width in pixels; null for a single line per paragraph." }, "line_height": { "type": "number", "minimum": 0.5, "maximum": 4 }, "letter_spacing": { "type": "number" }, "name": { "type": "string" }, "above": node() }),
            &["text"],
        ),
        def(
            "set_text",
            "Change a text layer: any of text, x, y, size, color, font, bold, italic, align, width, line_height, letter_spacing. Unmentioned settings stay as they are.",
            json!({ "node": node(), "text": { "type": "string" }, "x": { "type": "number" }, "y": { "type": "number" }, "size": { "type": "number", "minimum": 1, "maximum": 4000 }, "color": { "type": "string" }, "font": { "type": "string" }, "bold": { "type": "boolean" }, "italic": { "type": "boolean" }, "align": { "type": "string", "enum": ["left", "center", "right", "justify"] }, "width": { "type": ["number", "null"], "description": "Wrap width in pixels; null for a single line per paragraph." }, "line_height": { "type": "number", "minimum": 0.5, "maximum": 4 }, "letter_spacing": { "type": "number" } }),
            &["node"],
        ),
        def(
            "list_fonts",
            "Font families installed on this machine, usable as `font` in add_text and set_text.",
            json!({}),
            &[],
        ),
        def(
            "list_models",
            "Local AI models: which are installed (segmentation, subject matte, depth, fill, upscale), their size and licence. Tools that need a model say so when it is missing; download_model fetches one.",
            json!({}),
            &[],
        ),
        def(
            "download_model",
            "Download and install a local model by id from list_models (tens to hundreds of MB; takes a while). Ask before fetching anything large.",
            json!({ "id": { "type": "string" } }),
            &["id"],
        ),
        def(
            "select_subject",
            "Select the main subject of the picture with the local matte model (needs a matte model installed). mode combines with the current selection.",
            json!({ "mode": mode() }),
            &[],
        ),
        def(
            "select_by_points",
            "Select a thing by pointing at it with Segment Anything (needs SlimSAM installed): points are [[x, y], …] on the object, negative points [[x, y], …] mark what to leave out, and box [x0, y0, x1, y1] frames it. Returns the model's confidence.",
            json!({ "points": { "type": "array", "items": { "type": "array", "items": { "type": "number" }, "minItems": 2, "maxItems": 2 } }, "negative": { "type": "array", "items": { "type": "array", "items": { "type": "number" }, "minItems": 2, "maxItems": 2 } }, "box": { "type": "array", "items": { "type": "number" }, "minItems": 4, "maxItems": 4 }, "mode": mode() }),
            &[],
        ),
        def(
            "remove_background",
            "Cut the subject out of a pixel node (or the whole picture when node is omitted) into a new node with a transparent background, hiding the original. Needs a matte model.",
            json!({ "node": node() }),
            &[],
        ),
        def(
            "add_layer",
            "Add an empty, transparent pixel layer (the canvas size) to paint on, above the given node or at the top. Returns its id.",
            json!({ "name": { "type": "string" }, "above": node() }),
            &[],
        ),
        def(
            "list_brushes",
            "The brush library: every brush with its category (Ink, Pencil, Chalk, Marker, Watercolour, Oil, Airbrush, Eraser, Smudge) and what it is for. Use the names with paint.",
            json!({}),
            &[],
        ),
        def(
            "paint",
            concat!(
                "Paint strokes on a pixel layer with a brush from list_brushes. Each stroke is a polyline in document pixels; ",
                "a stroke gives either points ([x, y] or [x, y, pressure 0-1]) or d (SVG path data: M L C Q Z, absolute or relative) for smooth curves, plus an optional pressure envelope [start, end] applied along the stroke. ",
                "color is #RRGGBB (ignored by Eraser and Smudge brushes). settings overrides brush fields for the whole call, e.g. ",
                "{\"size\": 6, \"opacity\": 0.5, \"hardness\": 1, \"flow\": 0.3, \"wetness\": 0.5, \"taper_end\": 20}. ",
                "Everything in one call is a single undo step, so plan a drawing as a few calls: block-in, then lines, then shading. ",
                "Work on your own layer (add_layer) so the person can hide or mask it."
            ),
            json!({
                "node": node(),
                "brush": { "type": "string" },
                "color": { "type": "string", "pattern": "^#[0-9a-fA-F]{6}$" },
                "settings": { "type": "object" },
                "strokes": {
                    "type": "array", "minItems": 1, "maxItems": 400,
                    "items": {
                        "type": "object",
                        "properties": {
                            "d": { "type": "string" },
                            "pressure": { "type": "array", "items": { "type": "number" }, "minItems": 2, "maxItems": 2 },
                            "points": { "type": "array", "minItems": 1, "maxItems": 2000, "items": { "type": "array", "items": { "type": "number" }, "minItems": 2, "maxItems": 3 } },
                            "brush": { "type": "string" },
                            "color": { "type": "string", "pattern": "^#[0-9a-fA-F]{6}$" },
                            "settings": { "type": "object" }
                        },
                        "required": []
                    }
                }
            }),
            &["node", "strokes"],
        ),
        def("select_all", "Select the whole canvas.", json!({}), &[]),
        def("deselect", "Clear the selection.", json!({}), &[]),
        def("invert_selection", "Invert the selection.", json!({}), &[]),
        def(
            "modify_selection",
            "Grow or shrink the selection by pixels (negative shrinks), and/or feather it.",
            json!({ "grow": { "type": "integer" }, "feather": { "type": "number", "minimum": 0 } }),
            &[],
        ),
        def(
            "content_aware_fill",
            "Fill the selected area from its surroundings (object and blemish removal). The result goes into a new node above the selected node, so it can be hidden or masked.",
            json!({}),
            &[],
        ),
        def(
            "fill_selection",
            "Paint a solid colour into the selection (or the whole node) on a pixel node.",
            json!({ "node": node(), "color": { "type": "string", "pattern": "^#[0-9a-fA-F]{6}$" } }),
            &["node", "color"],
        ),
        def(
            "crop",
            "Crop the canvas to a rectangle, optionally straightening by rotating everything clockwise first. The rectangle may extend past the canvas to enlarge it. Layers move; they are never resampled.",
            json!({ "x": { "type": "integer" }, "y": { "type": "integer" }, "width": { "type": "integer", "minimum": 1 }, "height": { "type": "integer", "minimum": 1 }, "rotation": { "type": "number", "minimum": -45, "maximum": 45 } }),
            &["x", "y", "width", "height"],
        ),
        def(
            "canvas_size",
            "Change the canvas to width × height without scaling anything; the image stays pinned at the anchor. New canvas is transparent (select it and use content_aware_fill to fill it).",
            json!({ "width": { "type": "integer", "minimum": 1, "maximum": 30000 }, "height": { "type": "integer", "minimum": 1, "maximum": 30000 }, "anchor": { "type": "string", "enum": ["top-left", "top", "top-right", "left", "center", "right", "bottom-left", "bottom", "bottom-right"] } }),
            &["width", "height"],
        ),
        def(
            "image_size",
            "Scale the whole image to a new width (height follows the aspect ratio). Lossless: layers keep their source pixels.",
            json!({ "width": { "type": "integer", "minimum": 1, "maximum": 30000 } }),
            &["width"],
        ),
        def(
            "list_history",
            "List branches (with the current one marked) and the most recent commits. Commits are the saved points of the history graph; ids are for compare and branch.",
            json!({}),
            &[],
        ),
        def(
            "create_branch",
            "Start a new branch and switch to it, so edits can be tried without touching the current branch. from_commit starts it at an earlier commit instead of the current state.",
            json!({ "name": { "type": "string", "minLength": 1, "maxLength": 64 }, "from_commit": { "type": "integer" } }),
            &["name"],
        ),
        def(
            "switch_branch",
            "Switch to another branch. The current branch's work is committed first, so nothing is lost.",
            json!({ "name": { "type": "string" } }),
            &["name"],
        ),
        def(
            "compare",
            "What differs between two points: each of a and b is a branch name or a commit id; b defaults to the current document and a to the current branch's starting point.",
            json!({ "a": { "type": ["string", "integer"] }, "b": { "type": ["string", "integer"] } }),
            &[],
        ),
        def(
            "merge_branch",
            "Merge another branch into the current one. If both branches changed the same property of a node, nothing is merged and the conflicts are listed; call again with choices mapping each conflict key (a node id, or \"canvas\") to \"ours\" or \"theirs\". Only pass choices the person asked for.",
            json!({ "branch": { "type": "string" }, "choices": { "type": "object", "additionalProperties": { "type": "string", "enum": ["ours", "theirs"] } } }),
            &["branch"],
        ),
        def("undo", "Undo the most recent history step.", json!({}), &[]),
        def(
            "redo",
            "Redo the most recently undone step.",
            json!({}),
            &[],
        ),
    ]
}

/// Tool names as the CLI sees them.
pub fn qualified(name: &str) -> String {
    format!("mcp__{}__{name}", crate::SERVER_NAME)
}
