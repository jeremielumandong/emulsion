//! Responsive layout, resize rules and image-frame controls for active pages.
use crate::{ToolDef, ToolResult};
use emulsion_core::{
    Command, Editor, NodeId, NodeKind, design, design_layout as layout,
    design_metadata::{Anchor, Constraint},
};
use serde_json::{Value, json};

pub(crate) const READ_ONLY: &[&str] = &["describe_design_layout"];
pub(crate) const DESTRUCTIVE: &[&str] = &[
    "remove_responsive_layout",
    "clear_resize_constraints",
    "place_image_in_frame",
];
fn def(name: &str, description: &str, properties: Value, required: &[&str]) -> ToolDef {
    ToolDef {
        name: name.into(),
        description: format!(
            "{description} Active document/page only. Mutations respect locks and create one Undo step."
        ),
        input_schema: json!({"type":"object","properties":properties,"required":required,"additionalProperties":false}),
    }
}
fn node() -> Value {
    json!({"type":"integer","minimum":1})
}
fn nodes() -> Value {
    json!({"type":"array","items":node(),"minItems":1,"maxItems":emulsion_core::document::MAX_NODES,"uniqueItems":true})
}
fn anchor() -> Value {
    json!({"type":"string","enum":["start","center","end","stretch","scale"]})
}
pub(crate) fn definitions() -> Vec<ToolDef> {
    vec![
        def(
            "describe_design_layout",
            "Read persisted responsive group layouts, current frame bounds and per-object page-resize constraints. Child settings not present in a frame default to in-layout/fixed-width. Missing resize constraints default to scale on both axes without text reflow.",
            json!({}),
            &[],
        ),
        def(
            "set_responsive_layout",
            "Enable or patch automatic layout on an existing group. Use group_nodes first for ungrouped objects. Creates a native rectangle boundary when first enabled, then reflows children in layer order. Omitted fields preserve existing settings; new layouts default to column, padding 24, gap 16, columns 2, wrap true, align start, fixed height, and 80% of the canvas dimensions.",
            json!({"group":node(),"size":{"type":"array","items":{"type":"number","minimum":1,"maximum":100000},"minItems":2,"maxItems":2},"flow":{"type":"string","enum":["row","column","grid"]},"padding":{"type":"array","items":{"type":"number","minimum":0,"maximum":10000},"minItems":4,"maxItems":4,"description":"Top,right,bottom,left in document pixels."},"gap":{"type":"number","minimum":0,"maximum":10000},"columns":{"type":"integer","minimum":1,"maximum":64},"wrap":{"type":"boolean"},"align":{"type":"string","enum":["start","center","end"]},"hug_height":{"type":"boolean"}}),
            &["group"],
        ),
        def(
            "set_layout_child",
            "Patch an immediate content child's responsive settings. absolute=true excludes it from automatic positioning; fill_width=true uses the available cell width. Text reflows while retaining font size. At least one setting is required; frame boundaries are excluded.",
            json!({"node":node(),"absolute":{"type":"boolean"},"fill_width":{"type":"boolean"}}),
            &["node"],
        ),
        def(
            "remove_responsive_layout",
            "Disable automatic reflow on a responsive group. Keeps the group, boundary rectangle and children at their current geometry.",
            json!({"group":node()}),
            &["group"],
        ),
        def(
            "set_resize_constraints",
            "Patch page-resize anchor rules on native objects. At least one rule is required. Start/end retain the corresponding edge offset, center retains center offset, stretch changes size and scale scales proportionally. reflow_text changes text wrapping instead of resizing glyphs when the native resize operation supports it.",
            json!({"nodes":nodes(),"horizontal":anchor(),"vertical":anchor(),"reflow_text":{"type":"boolean"}}),
            &["nodes"],
        ),
        def(
            "clear_resize_constraints",
            "Remove explicit page-resize constraints from objects, restoring default scale/scale behavior without changing current artwork.",
            json!({"nodes":nodes()}),
            &["nodes"],
        ),
        def(
            "place_image_in_frame",
            "Place or replace an image in an existing native vector frame from a Raster node already present in this document. Uses the source's embedded pixels, not its masks, effects or placement; leaves the source unchanged. Cover-fit is centered. Existing frame image keeps its ID. Replacing resets that image's mask and crop. No file import or network fetch is performed.",
            json!({"frame":node(),"source":node()}),
            &["frame", "source"],
        ),
        def(
            "fit_frame_image",
            "Set cover, contain or stretch and normalized focal point on an existing native frame image. Preserves source pixels, clipping and existing rotation/flips. focus defaults to [0.5,0.5]; it affects cover placement only.",
            json!({"node":node(),"fit":{"type":"string","enum":["cover","contain","stretch"]},"focus":{"type":"array","items":{"type":"number","minimum":0,"maximum":1},"minItems":2,"maxItems":2}}),
            &["node", "fit"],
        ),
    ]
}
fn id(args: &Value, key: &str) -> Result<NodeId, String> {
    args.get(key)
        .and_then(Value::as_u64)
        .filter(|n| *n > 0)
        .ok_or_else(|| format!("{key} must be a positive integer"))
}
fn ids(args: &Value) -> Result<Vec<NodeId>, String> {
    let values = args["nodes"]
        .as_array()
        .filter(|v| !v.is_empty() && v.len() <= emulsion_core::document::MAX_NODES)
        .ok_or("nodes must be a nonempty bounded array")?;
    let mut seen = std::collections::HashSet::new();
    values
        .iter()
        .map(|v| {
            let id = v
                .as_u64()
                .filter(|n| *n > 0)
                .ok_or("nodes must contain positive integers")?;
            if !seen.insert(id) {
                return Err("Duplicate node ID".into());
            }
            Ok(id)
        })
        .collect()
}
fn editable(editor: &Editor, id: NodeId) -> Result<(), String> {
    editor.doc.node(id).ok_or("Object does not exist")?;
    let locks = editor.doc.layer_locks(id);
    if editor.doc.locked_ancestor(id).is_some()
        || locks.position
        || locks.pixels
        || locks.transparency
    {
        return Err(format!("Unlock object {id} before changing its layout"));
    }
    Ok(())
}
fn field<T: serde::de::DeserializeOwned>(args: &Value, key: &str) -> Result<Option<T>, String> {
    args.get(key)
        .map(|v| serde_json::from_value(v.clone()).map_err(|e| format!("Invalid {key}: {e}")))
        .transpose()
}
fn pair(
    args: &Value,
    key: &str,
    default: [f64; 2],
    min: f64,
    max: f64,
) -> Result<[f64; 2], String> {
    let values = field(args, key)?.unwrap_or(default);
    if values
        .iter()
        .any(|v| !v.is_finite() || !(min..=max).contains(v))
    {
        return Err(format!(
            "{key} must contain two numbers from {min} to {max}"
        ));
    }
    Ok(values)
}
fn commit(editor: &mut Editor, label: &str, command: Command) -> Result<(), String> {
    let mut trial = editor.doc.clone();
    command.apply(&mut trial).map_err(|e| e.to_string())?;
    editor.begin(label);
    if let Err(e) = editor.execute(command) {
        editor.cancel();
        return Err(e.to_string());
    }
    editor.end();
    Ok(())
}
fn frame_result(editor: &Editor, selected: NodeId) -> Result<Value, String> {
    let (boundary, image) =
        design::frame_parts(&editor.doc, selected).ok_or("Object is not an image frame")?;
    Ok(
        json!({"boundary":boundary,"image":image,"placement":image.and_then(|id|editor.doc.node(id)).and_then(|n|match &n.kind{NodeKind::Raster{placement,..}=>Some(placement),_=>None})}),
    )
}
fn run(editor: &mut Editor, name: &str, args: &Value) -> Result<Value, String> {
    if !READ_ONLY.contains(&name) && editor.in_transaction() {
        return Err("Finish the current edit before changing layout".into());
    }
    match name {
        "describe_design_layout" => Ok(
            json!({"frames":editor.doc.design.frames,"constraints":editor.doc.design.constraints,"bounds":editor.doc.design.frames.keys().map(|id|json!({"group":id,"bounds":layout::bounds(&editor.doc,*id)})).collect::<Vec<_>>()}),
        ),
        "set_responsive_layout" => {
            let group = id(args, "group")?;
            editable(editor, group)?;
            let mut frame = editor
                .doc
                .design
                .frames
                .get(&group)
                .cloned()
                .unwrap_or_default();
            macro_rules! patch{($($field:ident),*)=>{$(if let Some(value)=field(args,stringify!($field))?{frame.$field=value;})*};}
            patch!(flow, padding, gap, columns, wrap, align, hug_height);
            let default = layout::bounds(&editor.doc, group).map_or(
                [
                    f64::from(editor.doc.width) * 0.8,
                    f64::from(editor.doc.height) * 0.8,
                ],
                |(_, _, w, h)| [w, h],
            );
            let size = pair(args, "size", default, 1., 100000.)?;
            editor.begin("Responsive layout");
            if let Err(error) = layout::enable(editor, group, frame, (size[0], size[1])) {
                editor.cancel();
                return Err(error);
            }
            editor.end();
            Ok(
                json!({"group":group,"frame":editor.doc.design.frames[&group],"bounds":layout::bounds(&editor.doc,group)}),
            )
        }
        "set_layout_child" => {
            let node = id(args, "node")?;
            editable(editor, node)?;
            let parent = editor
                .doc
                .node(node)
                .and_then(|n| n.parent)
                .ok_or("Choose an immediate child of a responsive frame")?;
            editable(editor, parent)?;
            let absolute = field::<bool>(args, "absolute")?;
            let fill = field::<bool>(args, "fill_width")?;
            if absolute.is_none() && fill.is_none() {
                return Err("Set absolute or fill_width".into());
            }
            let mut design = editor.doc.design.clone();
            let frame = design
                .frames
                .get_mut(&parent)
                .ok_or("Parent has no responsive layout")?;
            if frame.boundary == node {
                return Err("The frame boundary is not a layout child".into());
            }
            let child = frame.children.entry(node).or_default();
            if let Some(v) = absolute {
                child.absolute = v;
            }
            if let Some(v) = fill {
                child.fill_width = v;
            }
            commit(
                editor,
                "Layout child",
                Command::SetDesign {
                    design: Box::new(design),
                },
            )?;
            Ok(
                json!({"node":node,"group":parent,"settings":editor.doc.design.frames[&parent].children[&node]}),
            )
        }
        "remove_responsive_layout" => {
            let group = id(args, "group")?;
            editable(editor, group)?;
            let mut design = editor.doc.design.clone();
            design
                .frames
                .remove(&group)
                .ok_or("Group has no responsive layout")?;
            commit(
                editor,
                "Remove automatic layout",
                Command::SetDesign {
                    design: Box::new(design),
                },
            )?;
            Ok(json!({"group":group,"removed":true,"artwork_preserved":true}))
        }
        "set_resize_constraints" | "clear_resize_constraints" => {
            let nodes = ids(args)?;
            let horizontal = field::<Anchor>(args, "horizontal")?;
            let vertical = field::<Anchor>(args, "vertical")?;
            let reflow = field::<bool>(args, "reflow_text")?;
            let clear = name == "clear_resize_constraints";
            if !clear && horizontal.is_none() && vertical.is_none() && reflow.is_none() {
                return Err("Set horizontal, vertical or reflow_text".into());
            }
            let mut design = editor.doc.design.clone();
            for node in &nodes {
                editable(editor, *node)?;
                if clear {
                    design.constraints.remove(node);
                } else {
                    let rule = design
                        .constraints
                        .entry(*node)
                        .or_insert(Constraint::default());
                    if let Some(v) = horizontal {
                        rule.horizontal = v;
                    }
                    if let Some(v) = vertical {
                        rule.vertical = v;
                    }
                    if let Some(v) = reflow {
                        rule.reflow_text = v;
                    }
                }
            }
            commit(
                editor,
                "Resize constraints",
                Command::SetDesign {
                    design: Box::new(design),
                },
            )?;
            Ok(json!({"nodes":nodes,"cleared":clear,"constraints":editor.doc.design.constraints}))
        }
        "place_image_in_frame" => {
            let frame = id(args, "frame")?;
            editable(editor, frame)?;
            let (boundary, image) =
                design::frame_parts(&editor.doc, frame).ok_or("Select a native vector frame")?;
            editable(editor, boundary)?;
            if let Some(image) = image {
                editable(editor, image)?;
            }
            let source = id(args, "source")?;
            let raster=match &editor.doc.node(source).ok_or("Source image does not exist")?.kind{NodeKind::Raster{raster,..}=>raster.clone(),_=>return Err("source must be an existing Raster node; file import is not performed by this tool".into())};
            design::place_in_frame(editor, frame, raster)?;
            frame_result(editor, frame)
        }
        "fit_frame_image" => {
            let node = id(args, "node")?;
            editable(editor, node)?;
            let fit = match args["fit"].as_str() {
                Some("cover") => design::ImageFit::Cover,
                Some("contain") => design::ImageFit::Contain,
                Some("stretch") => design::ImageFit::Stretch,
                _ => return Err("fit must be cover, contain or stretch".into()),
            };
            let focus = pair(args, "focus", [0.5; 2], 0., 1.)?;
            let command = design::fit_frame_image(&editor.doc, node, fit, focus)?;
            commit(editor, "Fit frame image", command)?;
            frame_result(editor, node)
        }
        _ => Err("Unknown Design layout tool".into()),
    }
}
pub(crate) fn execute(editor: &mut Editor, name: &str, args: &Value) -> Option<ToolResult> {
    let definition = definitions().into_iter().find(|d| d.name == name)?;
    let result = (|| {
        let values = args.as_object().ok_or("Tool arguments must be an object")?;
        let fields = definition.input_schema["properties"].as_object().unwrap();
        for key in values.keys() {
            if !fields.contains_key(key) {
                return Err(format!("Unknown argument: {key}"));
            }
        }
        for required in definition.input_schema["required"].as_array().unwrap() {
            let key = required.as_str().unwrap();
            if !values.contains_key(key) {
                return Err(format!("Missing argument: {key}"));
            }
        }
        run(editor, name, args)
    })();
    Some(match result {
        Ok(v) => ToolResult::text(v.to_string()),
        Err(e) => ToolResult::error(e),
    })
}
#[cfg(test)]
#[path = "design_layout_tools_tests.rs"]
mod tests;
