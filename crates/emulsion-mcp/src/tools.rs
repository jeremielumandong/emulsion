//! Tool definitions. Names are prefixed `mcp__emulsion__` by the CLI.

use crate::server::ToolDef;
use serde_json::{Value, json};

/// Tools that only read; the CLI may run them without asking.
pub const READ_ONLY: &[&str] = &["describe_document", "get_view"];

/// Tools whose effect is hard to see or undo at a glance; always confirmed.
pub const DESTRUCTIVE: &[&str] = &["delete_node", "ungroup", "undo"];

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
