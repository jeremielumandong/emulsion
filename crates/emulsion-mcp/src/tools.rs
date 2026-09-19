//! Tool definitions. Names are prefixed `mcp__emulsion__` by the CLI.

use crate::server::ToolDef;
use serde_json::{Value, json};

/// Tools that only read; the CLI may run them without asking.
pub const READ_ONLY: &[&str] = &["describe_document", "get_view", "list_history", "compare"];

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
pub const HEAVY: &[&str] = &["select_color", "content_aware_fill"];

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
            "Add an adjustment node. It changes everything below it in the same parent. Parameters: \
             exposure {exposure -5..5 ev, offset -0.5..0.5, gamma 0.1..3}; brightness_contrast \
             {brightness -150..150, contrast -50..100}; levels {in_black 0..253, in_white 2..255, gamma \
             0.1..9.99, out_black, out_white 0..255}; hue_saturation {hue -180..180, saturation \
             -100..100, lightness -100..100}; white_balance {temperature -100..100 (warmth), tint \
             -100..100}; invert {}.",
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
