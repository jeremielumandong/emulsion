//! Native path authoring and matching with validated, atomic history operations.
use crate::{ToolDef, ToolResult};
use emulsion_core::{Editor, NodeKind, design_vectors as vectors};
use serde::Deserialize;
use serde_json::{Value, json};
pub(crate) const READ_ONLY: &[&str] = &[
    "inspect_vector_path",
    "find_matching_objects",
    "get_design_precision",
];
pub(crate) const DESTRUCTIVE: &[&str] = &["combine_vector_paths", "warp_vector_mesh"];
fn def(name: &str, description: &str, properties: Value, required: &[&str]) -> ToolDef {
    ToolDef {
        name: name.into(),
        description: description.into(),
        input_schema: json!({"type":"object","additionalProperties":false,"properties":properties,"required":required}),
    }
}
pub(crate) fn definitions() -> Vec<ToolDef> {
    let node = json!({"type":"integer","minimum":1});
    let index = json!({"type":"integer","minimum":0});
    let point = json!({"type":"array","items":{"type":"number","minimum":-1000000000,"maximum":1000000000},"minItems":2,"maxItems":2});
    vec![
        def(
            "get_design_precision",
            "Read page ruler units, origin in pixels and print DPI used for conversion.",
            json!({}),
            &[],
        ),
        def(
            "set_design_precision",
            "Patch persisted ruler units or origin without moving artwork. Omitted values preserve settings. One Undo step.",
            json!({"unit":{"enum":["pixels","millimeters","inches","points"]},"origin":point}),
            &[],
        ),
        def(
            "position_design_object",
            "Set an object's geometric/artwork top-left in page units relative to the ruler origin, preserving native sources and linked masks.",
            json!({"node":node,"x":{"type":"number"},"y":{"type":"number"}}),
            &["node", "x", "y"],
        ),
        def(
            "space_design_objects",
            "Arrange independent objects by geometric order with an exact edge-to-edge gap in page units; first object stays fixed. All dependent moves are preflighted before one Undo step.",
            json!({"nodes":{"type":"array","items":node,"minItems":2,"maxItems":1000,"uniqueItems":true},"vertical":{"type":"boolean","default":false},"gap":{"type":"number","minimum":-100000,"maximum":100000}}),
            &["nodes", "gap"],
        ),
        def(
            "perspective_vector_path",
            "Apply a true projective transform to a vector path using clockwise top-left/top-right/bottom-right/bottom-left corners. Curves become native sampled contours; Undo restores exact original geometry.",
            json!({"node":node,"corners":{"type":"array","items":point,"minItems":4,"maxItems":4},"tolerance":{"type":"number","minimum":0.05,"maximum":10}}),
            &["node", "corners"],
        ),
        def(
            "trace_bitmap_to_vector",
            "Create a local monochrome or alpha-silhouette vector trace from an image. Source remains intact. Samples at a bounded resolution and preserves holes; returns a native editable path. Layer masks and blending effects are excluded.",
            json!({"node":node,"options":{"type":"object","additionalProperties":false,"properties":{"resolution":{"type":"integer","minimum":16,"maximum":512},"threshold":{"type":"number","minimum":0.01,"maximum":0.99},"alpha_only":{"type":"boolean"},"invert":{"type":"boolean"},"color":{"type":"array","items":{"type":"integer","minimum":0,"maximum":255},"minItems":4,"maxItems":4}}}}),
            &["node"],
        ),
        def(
            "inspect_vector_path",
            "Read native subpaths, cubic handles, smooth flags and styling.",
            json!({"node":node}),
            &["node"],
        ),
        def(
            "set_vector_point",
            "Move an anchor and optionally its incoming/outgoing Bezier handles; omitted handles move with the point. Coordinates are document pixels. One Undo step.",
            json!({"node":node,"subpath":index,"anchor":index,"position":point,"incoming":point,"outgoing":point,"smooth":{"type":"boolean","default":false}}),
            &["node", "subpath", "anchor", "position"],
        ),
        def(
            "join_vector_subpaths",
            "Join the end of first open subpath to the start of second in one native path.",
            json!({"node":node,"first":index,"second":index}),
            &["node", "first", "second"],
        ),
        def(
            "split_vector_subpath",
            "Split at an interior anchor, or open a closed contour at an anchor while retaining the closing curve.",
            json!({"node":node,"subpath":index,"anchor":index}),
            &["node", "subpath", "anchor"],
        ),
        def(
            "combine_vector_paths",
            "Combine sibling paths into first node using its style. component preserves curves; Boolean operations approximate curves. Removes other operands in the same Undo step; clipping stacks are rejected.",
            json!({"nodes":{"type":"array","items":node,"minItems":2,"maxItems":64,"uniqueItems":true},"operation":{"enum":["component","union","subtract","intersect","exclude"]}}),
            &["nodes", "operation"],
        ),
        def(
            "skew_vector_path",
            "Apply native affine skew around a document-space origin, retaining editable handles.",
            json!({"node":node,"x_degrees":{"type":"number","exclusiveMinimum":-85,"exclusiveMaximum":85},"y_degrees":{"type":"number","exclusiveMinimum":-85,"exclusiveMaximum":85},"origin":point}),
            &["node", "x_degrees", "y_degrees", "origin"],
        ),
        def(
            "warp_vector_mesh",
            "Warp geometry through a row-major rectangular control mesh. Curves become editable sampled contours. 2x2 is a four-corner bilinear envelope; this is not a projective perspective transform. Folded cells are rejected.",
            json!({"node":node,"columns":{"type":"integer","minimum":2,"maximum":16},"rows":{"type":"integer","minimum":2,"maximum":16},"points":{"type":"array","items":point,"minItems":4,"maxItems":256},"tolerance":{"type":"number","minimum":0.05,"maximum":10,"default":0.25}}),
            &["node", "columns", "rows", "points"],
        ),
        def(
            "create_stroke_outline",
            "Create a separate editable outline of a solid vector stroke, including caps, joins, dashes and alignment. Keeps the original source unchanged for later stroke edits. Returns the new node ID.",
            json!({"node":node}),
            &["node"],
        ),
        def(
            "find_matching_objects",
            "Return visible nodes with the same kind, fill, stroke, opacity, or text font attributes. Hidden ancestor/library content is excluded. Does not mutate selection or history.",
            json!({"node":node,"property":{"enum":["kind","fill","stroke","opacity","font"]}}),
            &["node", "property"],
        ),
    ]
}
#[derive(Deserialize)]
#[serde(deny_unknown_fields)]
struct Node {
    node: u64,
}
#[derive(Deserialize)]
#[serde(deny_unknown_fields)]
struct Point {
    node: u64,
    subpath: usize,
    anchor: usize,
    position: (f64, f64),
    incoming: Option<(f64, f64)>,
    outgoing: Option<(f64, f64)>,
    #[serde(default)]
    smooth: bool,
}
#[derive(Deserialize)]
#[serde(deny_unknown_fields)]
struct Join {
    node: u64,
    first: usize,
    second: usize,
}
#[derive(Deserialize)]
#[serde(deny_unknown_fields)]
struct Split {
    node: u64,
    subpath: usize,
    anchor: usize,
}
#[derive(Deserialize)]
#[serde(deny_unknown_fields)]
struct Combine {
    nodes: Vec<u64>,
    operation: vectors::Combine,
}
#[derive(Deserialize)]
#[serde(deny_unknown_fields)]
struct Skew {
    node: u64,
    x_degrees: f64,
    y_degrees: f64,
    origin: (f64, f64),
}
#[derive(Deserialize)]
#[serde(deny_unknown_fields)]
struct Mesh {
    node: u64,
    columns: usize,
    rows: usize,
    points: Vec<(f64, f64)>,
    #[serde(default = "tolerance")]
    tolerance: f64,
}
fn tolerance() -> f64 {
    0.25
}
#[derive(Deserialize)]
#[serde(deny_unknown_fields)]
struct Matching {
    node: u64,
    property: vectors::MatchProperty,
}
fn parse<T: serde::de::DeserializeOwned>(args: &Value) -> Result<T, String> {
    serde_json::from_value(args.clone()).map_err(|e| e.to_string())
}
pub(crate) fn execute(editor: &mut Editor, name: &str, args: &Value) -> Option<ToolResult> {
    if !definitions().iter().any(|d| d.name == name) {
        return None;
    }
    Some(match run(editor, name, args) {
        Ok(v) => ToolResult::text(v.to_string()),
        Err(e) => ToolResult::error(e),
    })
}
#[derive(Deserialize)]
#[serde(deny_unknown_fields)]
struct Trace {
    node: u64,
    #[serde(default)]
    options: vectors::trace::Options,
}
#[derive(Deserialize)]
#[serde(deny_unknown_fields)]
struct Perspective {
    node: u64,
    corners: [(f64, f64); 4],
    #[serde(default = "tolerance")]
    tolerance: f64,
}
#[derive(Deserialize)]
#[serde(deny_unknown_fields)]
struct Precision {
    unit: Option<emulsion_core::design_precision::Unit>,
    origin: Option<[f64; 2]>,
}
#[derive(Deserialize)]
#[serde(deny_unknown_fields)]
struct Position {
    node: u64,
    x: f64,
    y: f64,
}
#[derive(Deserialize)]
#[serde(deny_unknown_fields)]
struct Spacing {
    nodes: Vec<u64>,
    #[serde(default)]
    vertical: bool,
    gap: f64,
}
fn run(e: &mut Editor, name: &str, args: &Value) -> Result<Value, String> {
    let mut added = None;
    match name {
        "get_design_precision" => {
            if args.as_object().is_none_or(|o| !o.is_empty()) {
                return Err("Expected empty arguments.".into());
            }
            return Ok(
                json!({"settings":e.doc.design.precision,"dpi":e.doc.resolution,"pixels_per_unit":e.doc.design.precision.unit.factor(e.doc.resolution)}),
            );
        }
        "set_design_precision" => {
            let a: Precision = parse(args)?;
            if args
                .as_object()
                .is_some_and(|o| o.values().any(Value::is_null))
            {
                return Err("Precision fields cannot be null.".into());
            }
            let mut settings = e.doc.design.precision;
            if let Some(unit) = a.unit {
                settings.unit = unit;
            }
            if let Some(origin) = a.origin {
                settings.origin = origin;
            }
            emulsion_core::design_precision::set(e, settings)?;
        }
        "position_design_object" => {
            let a: Position = parse(args)?;
            emulsion_core::design_precision::position(e, a.node, a.x, a.y)?;
        }
        "space_design_objects" => {
            let a: Spacing = parse(args)?;
            emulsion_core::design_precision::spacing(e, &a.nodes, a.vertical, a.gap)?;
        }

        "perspective_vector_path" => {
            let a: Perspective = parse(args)?;
            vectors::perspective(e, a.node, a.corners, a.tolerance)?;
        }
        "trace_bitmap_to_vector" => {
            let a: Trace = parse(args)?;
            added = Some(vectors::trace::apply(e, a.node, a.options)?);
        }
        "inspect_vector_path" => {
            let a: Node = parse(args)?;
            let Some(node) = e.doc.node(a.node) else {
                return Err("Object does not exist.".into());
            };
            let NodeKind::Path { path, style, .. } = &node.kind else {
                return Err("Select a native vector path.".into());
            };
            return Ok(
                json!({"node":a.node,"path":path,"style":crate::shape_style::style_json(style)}),
            );
        }
        "find_matching_objects" => {
            let a: Matching = parse(args)?;
            return Ok(json!({"nodes":vectors::matching(&e.doc,a.node,a.property)?}));
        }
        "set_vector_point" => {
            let a: Point = parse(args)?;
            vectors::point(
                e, a.node, a.subpath, a.anchor, a.position, a.incoming, a.outgoing, a.smooth,
            )?;
        }
        "join_vector_subpaths" => {
            let a: Join = parse(args)?;
            vectors::join(e, a.node, a.first, a.second)?;
        }
        "split_vector_subpath" => {
            let a: Split = parse(args)?;
            vectors::split(e, a.node, a.subpath, a.anchor)?;
        }
        "combine_vector_paths" => {
            let a: Combine = parse(args)?;
            added = Some(vectors::combine(e, &a.nodes, a.operation)?);
        }
        "skew_vector_path" => {
            let a: Skew = parse(args)?;
            vectors::skew(e, a.node, a.x_degrees, a.y_degrees, a.origin)?;
        }
        "warp_vector_mesh" => {
            let a: Mesh = parse(args)?;
            vectors::mesh(e, a.node, a.columns, a.rows, &a.points, a.tolerance)?;
        }
        "create_stroke_outline" => {
            let a: Node = parse(args)?;
            added = Some(vectors::outline_stroke(e, a.node)?);
        }
        _ => return Err("Unknown vector action.".into()),
    }
    Ok(json!({"ok":true,"node":added}))
}

#[cfg(test)]
mod tests {
    use super::*;
    use emulsion_core::{Command, Document, Node, command::Slot};
    use emulsion_raster::{
        vector::{Path, PathStyle},
        vector_geometry,
    };
    use std::sync::Arc;
    #[test]
    fn vector_mcp_validates_geometry_and_preserves_one_step_history() {
        let mut e = Editor::new(Document::new(120, 100), None);
        let id = e
            .execute(Command::AddNode {
                node: Box::new(Node::path(
                    0,
                    "Path",
                    Arc::new(Path::from_svg("M0 0 L50 0 M50 0 L100 10").unwrap()),
                    PathStyle::default(),
                    120,
                    100,
                )),
                slot: Slot::TOP,
            })
            .unwrap()
            .unwrap();
        let before = e.doc.clone();
        let call = |e: &mut Editor, name, args| crate::exec::execute(e, name, &args);
        assert!(!call(&mut e, "inspect_vector_path", json!({"node":id})).is_error);
        assert!(
            !call(
                &mut e,
                "join_vector_subpaths",
                json!({"node":id,"first":0,"second":1})
            )
            .is_error
        );
        e.undo();
        assert_eq!(e.doc, before);
        for args in [
            json!({"node":id,"subpath":-1,"anchor":0,"position":[0,0]}),
            json!({"node":id,"subpath":0,"anchor":0,"position":[0,"1"]}),
            json!({"node":id,"subpath":0,"anchor":999,"position":[0,0]}),
            json!({"node":id,"subpath":0,"anchor":0,"position":[0,0],"unexpected":true}),
        ] {
            assert!(call(&mut e, "set_vector_point", args).is_error);
            assert_eq!(e.doc, before);
        }
        assert!(crate::tools::is_read_only("find_matching_objects"));
        assert!(crate::tools::is_destructive("combine_vector_paths"));
        let path = vector_geometry::rectangle(0., 0., 20., 20.);
        e.execute(Command::SetPath {
            id,
            path: Arc::new(path),
            style: Default::default(),
        })
        .unwrap();
        let before = e.doc.clone();
        assert!(
            !call(
                &mut e,
                "perspective_vector_path",
                json!({"node":id,"corners":[[0,0],[30,0],[25,25],[5,20]]})
            )
            .is_error
        );
        e.undo();
        assert_eq!(e.doc, before);
    }
}
