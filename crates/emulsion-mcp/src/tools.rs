//! Tool definitions. Names are prefixed `mcp__emulsion__` by the CLI.

use crate::server::ToolDef;
use serde_json::{Value, json};

/// Tools that only read; the CLI may run them without asking.
pub const READ_ONLY: &[&str] = &[
    "describe_document",
    "get_view",
    "get_reference_image",
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
    "rasterize",
];

/// Tools that compute for a while; hosts run them off the UI thread.
pub const HEAVY: &[&str] = &[
    "select_color",
    "content_aware_fill",
    "paint",
    "hatch",
    "liquify",
    "add_filter",
    "set_filter",
    "remove_filter",
    "download_model",
    "select_subject",
    "select_by_points",
    "remove_background",
    "inpaint",
    "generative_fill",
    "generate_image",
    "depth_map",
    "upscale",
    "restore_faces",
    "lens_profile",
    "import_recipe",
    "batch_export",
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

fn view_region() -> Value {
    json!({"type": "array", "items": {"type": "integer", "minimum": 0}, "minItems": 4, "maxItems": 4,
        "description": "[x, y, width, height] in document pixels. Positive size, entirely inside the canvas. Returned mapping converts preview coordinates to document coordinates."})
}

fn critique_context() -> Value {
    json!({"type": "object", "additionalProperties": false, "properties": {
        "medium": {"type": "string"}, "stage": {"type": "string"},
        "style": {"type": "string", "description": "Requested visual style, independently of medium. Free text accepts any named, custom or hybrid style; omitted means unknown."},
        "composition_intent": {"type": "string", "description": "Intended layout, focal placement and symmetry, including deliberate centring."},
        "user_constraints": {"type": "array", "items": {"type": "string"}}
    }})
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
             image, optionally a document-space region for close inspection. Returns image-to-document coordinate mapping. Use full views for composition and region views for details.",
            json!({
                "node": { "type": "integer", "description": "Render only this node (and its children)." },
                "region": view_region(),
                "max_size": { "type": "integer", "minimum": 64, "maximum": 1568, "description": "Longest side in pixels, default 1024." }
            }),
            &[],
        ),
        def(
            "get_reference_image",
            "Inspect the image attached by the person as a drawing or painting reference. Returns a bounded PNG and original reference dimensions with image-to-reference coordinate mapping. Reference coordinates are independent of the drawing canvas. This does not import pixels into the document or accept a file path. Returns an error when no reference is attached.",
            json!({}),
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
            "rotate_node",
            "Rotate a node or group by an additional number of degrees clockwise about the centre of its content. Negative degrees rotate counterclockwise. Groups rotate their contents together; paths and text remain editable and pixel source data is retained. The canvas size and selection stay unchanged. Locked content cannot be rotated.",
            json!({"node": node(), "degrees": {"type": "number", "description": "Incremental clockwise angle in degrees, for example 90, -90, or 15."}}),
            &["node", "degrees"],
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
            "Intent-aware metric observations with conditional suggestions, plus images for your visual review. Pass medium, stage, composition intent and constraints in context. Inspect the returned full view and optional detail region for anatomy, perspective and subject fidelity; metrics cannot establish those qualities. Jev optionally ranks measurements only.",
            json!({ "count": { "type": "integer", "minimum": 1, "maximum": 8 },
                "context": critique_context(), "region": view_region(),
                "include_images": {"type": "boolean", "default": true, "description": "Return full composition and optional region images for the calling assistant to review."} }),
            &[],
        ),
        def(
            "hatch",
            "Shade an area with parallel strokes: fill rect [x, y, width, height] (or the selection's bounds) with lines at angle (degrees, default 45) every spacing pixels (default 8), with a little jitter (0-1) so they look hand-made; cross=true adds a second direction. Uses a brush and color like paint. One undo step.",
            json!({ "node": node(), "brush": { "type": "string" }, "color": { "type": "string" }, "settings": { "type": "object" }, "rect": { "type": "array", "items": { "type": "number" }, "minItems": 4, "maxItems": 4 }, "angle": { "type": "number" }, "spacing": { "type": "number", "minimum": 1 }, "jitter": { "type": "number", "minimum": 0, "maximum": 1 }, "cross": { "type": "boolean" },
                "sample_merged": {"type": "boolean", "default": false, "description": "Opt into lower-layer colour pickup for wet/smudge brushes."} }),
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
            "generative_fill",
            "Paint the selection (or rect [x, y, width, height]) from a text prompt with the person's configured image provider (Local SD, OpenAI, or Google; Settings › Image generation), into a new labelled node. The picture around the area goes to that provider as context; cloud providers bill API usage. Use inpaint instead when the goal is only to remove something.",
            json!({
                "prompt": { "type": "string", "minLength": 1 },
                "negative": { "type": "string" },
                "rect": { "type": "array", "items": { "type": "number" }, "minItems": 4, "maxItems": 4 }
            }),
            &["prompt"],
        ),
        def(
            "generate_image",
            "Make a whole new canvas-sized layer from a text prompt with the person's configured image provider (Local SD, OpenAI, or Google; Settings › Image generation), for backgrounds and textures. The layer is labelled with its provenance.",
            json!({
                "prompt": { "type": "string", "minLength": 1 },
                "negative": { "type": "string" },
                "name": { "type": "string" }
            }),
            &["prompt"],
        ),
        def(
            "inpaint",
            "Fill the selection (or rect [x, y, width, height]) from its surroundings with the LaMa model into a new node: removes objects and repairs holes, better than content_aware_fill on structure and texture. Needs the fill model.",
            json!({ "rect": { "type": "array", "items": { "type": "number" }, "minItems": 4, "maxItems": 4 } }),
            &[],
        ),
        def(
            "depth_map",
            "Add a grey depth-map node of the picture (near is bright) with Depth Anything; use it as a mask for depth of field, fog or depth-aware grading. Needs the depth model.",
            json!({ "name": { "type": "string" } }),
            &[],
        ),
        def(
            "lens_profile",
            "Correct the lens of a pixel node from the picture's camera data: looks the camera and lens up in the lensfun database and adds an editable Lens profile filter (distortion and vignetting). Needs the database (download_model lensfun) and EXIF in the file; strength 0–150 percent.",
            json!({ "node": node(), "strength": { "type": "number", "minimum": 0, "maximum": 150 } }),
            &[],
        ),
        def(
            "restore_faces",
            "Find every face and restore it with GFPGAN into a new node on top (strength 0–1 blends the restored face over the original; default 1). Good for old, small or blurry portraits. Needs the face detector and GFPGAN.",
            json!({ "strength": { "type": "number", "minimum": 0, "maximum": 1 } }),
            &[],
        ),
        def(
            "upscale",
            "Enlarge the whole picture with the installed super-resolution model (×4, or ×2 with the lightweight model): the canvas grows by the factor and the result lands as the top node. Slow on a CPU (tens of seconds per megapixel). Needs an upscale model.",
            json!({}),
            &[],
        ),
        def(
            "set_lock",
            "Lock or unlock a node so it cannot be edited or moved.",
            json!({ "node": node(), "locked": { "type": "boolean" } }),
            &["node", "locked"],
        ),
        def(
            "set_clip",
            "Clip a node to the content of the node directly below it (clipping mask), or unclip with to = null.",
            json!({ "node": node(), "to": { "type": ["integer", "null"] } }),
            &["node"],
        ),
        def(
            "add_mask",
            "Give a node a layer mask: from = \"selection\" turns the current selection into the mask (white reveals), \"all\" makes a fully white mask to paint into.",
            json!({ "node": node(), "from": { "type": "string", "enum": ["selection", "all"] } }),
            &["node"],
        ),
        def(
            "remove_mask",
            "Remove a node's layer mask.",
            json!({ "node": node() }),
            &["node"],
        ),
        def(
            "set_mask_enabled",
            "Turn a node's layer mask on or off without removing it.",
            json!({ "node": node(), "enabled": { "type": "boolean" } }),
            &["node", "enabled"],
        ),
        def(
            "rasterize",
            "Bake a smart layer's filters into plain pixels (the filters stop being editable).",
            json!({ "node": node() }),
            &["node"],
        ),
        def(
            "save_document",
            "Save the document as OpenRaster (.ora) with its history: to path, or to the file it was opened from.",
            json!({ "path": { "type": "string" } }),
            &[],
        ),
        def(
            "export_image",
            "Write the flattened picture to path; the extension picks the format (.png, .jpg, .webp, .tif). quality 1–100 for JPEG.",
            json!({ "path": { "type": "string" }, "quality": { "type": "integer", "minimum": 1, "maximum": 100 } }),
            &["path"],
        ),
        def(
            "import_recipe",
            "Add a recipe to the library from text (a pasted settings block or .recipe.toml), a file path (.recipe.toml, Lightroom .xmp, Fujifilm .FP1, text) or a web page URL (a recipe page, or an index page whose recipe links are all followed). Returns what was saved.",
            json!({ "text": { "type": "string" }, "path": { "type": "string" }, "url": { "type": "string" } }),
            &[],
        ),
        def(
            "batch_export",
            "Apply a recipe to many pictures and write them out: folder (every picture in it) or paths, recipe by name (omit for none), out_dir, format jpg or png. Slow: seconds per picture.",
            json!({ "folder": { "type": "string" }, "paths": { "type": "array", "items": { "type": "string" } }, "recipe": { "type": "string" }, "out_dir": { "type": "string" }, "format": { "type": "string", "enum": ["jpg", "png"] } }),
            &["out_dir"],
        ),
        def(
            "add_layer",
            "Add an empty, transparent pixel layer (the canvas size) to paint on, above the given node or at the top. Returns its id.",
            json!({ "name": { "type": "string" }, "above": node() }),
            &[],
        ),
        def(
            "list_brushes",
            "Discover brushes by name or category, intended uses, actual preset settings and supported ranges. Returns a rendered swatch sheet of up to 12 rows; use offset for further swatches. Preset names are approximations, not proof of material simulation. Use names with paint.",
            json!({"query": {"type": "string", "description": "Case-insensitive name/category substring; default all."},
                "swatches": {"type": "boolean", "default": true}, "offset": {"type": "integer", "minimum": 0, "description": "First swatch row in the filtered list; metadata includes all matches."}}),
            &[],
        ),
        def(
            "paint",
            concat!(
                "Paint strokes on a pixel layer with a brush from list_brushes. Each stroke is a polyline in document pixels; ",
                "a stroke gives either points ([x, y] or [x, y, pressure 0-1]) or d (SVG path data: M L C Q Z, absolute or relative) for smooth curves. SVG subpaths preserve pen lifts; the optional pressure envelope [start, end] restarts for each subpath. ",
                "color is #RRGGBB (ignored by Eraser and Smudge brushes). settings overrides brush fields for the whole call, e.g. ",
                "{\"size\": 6, \"opacity\": 0.5, \"hardness\": 1, \"flow\": 0.3, \"wetness\": 0.5, \"taper_end\": 20, \"tilt\": 0.5}. ",
                "mirror / symmetry repeat every stroke across or around the canvas centre; alpha_lock keeps paint on existing pixels. ",
                "Everything in one call is a single undo step. Group marks by the chosen medium's current stage and inspection checkpoints; no fixed number of calls or universal paint order is required. ",
                "Work on your own layer (add_layer) so the person can hide or mask it."
            ),
            json!({
                "node": node(),
                "brush": { "type": "string" },
                "color": { "type": "string", "pattern": "^#[0-9a-fA-F]{6}$" },
                "settings": { "type": "object" },
                "sample_merged": {"type": "boolean", "default": false, "description": "Opt into frozen lower-layer colour pickup for wet/smudge brushes. Current and higher layers are excluded from the backdrop; current layer still supplies its own paint."},
                "alpha_lock": {"type": "boolean", "default": false, "description": "Paint only where the layer already has pixels (shading inside an existing shape)."},
                "mirror": {"type": "string", "enum": ["x", "y", "xy"], "description": "Also paint each stroke mirrored across the canvas centre: x = left/right, y = top/bottom, xy = both (quadrant symmetry)."},
                "symmetry": {"type": "integer", "minimum": 2, "maximum": 64, "description": "Radial symmetry: also paint each stroke rotated this many ways around the canvas centre (mandalas)."},
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
        def(
            "liquify",
            concat!(
                "Liquify a pixel layer along a path (document pixels): push drags the pixels under the brush with it; ",
                "twirl_cw / twirl_ccw rotate, pinch pulls in, expand pushes out, restore paints the original pixels back. ",
                "size is the brush diameter, strength 0-1. One undo step per call."
            ),
            json!({
                "node": node(),
                "mode": { "type": "string", "enum": ["push", "twirl_cw", "twirl_ccw", "pinch", "expand", "restore"], "default": "push" },
                "points": { "type": "array", "minItems": 1, "maxItems": 2000, "items": { "type": "array", "items": { "type": "number" }, "minItems": 2, "maxItems": 2 } },
                "size": { "type": "number", "minimum": 2, "maximum": 2000, "default": 80 },
                "strength": { "type": "number", "minimum": 0, "maximum": 1, "default": 0.6 }
            }),
            &["node", "points"],
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
