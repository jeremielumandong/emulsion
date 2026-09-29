//! Responsive layout, resize rules and image-frame controls for active pages.
use crate::{ToolDef, ToolResult};
use emulsion_core::{
    Command, Editor, NodeId, NodeKind, design, design_layout as layout, design_metadata::Anchor,
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
fn optional_size() -> Value {
    json!({"type":["number","null"],"minimum":1,"maximum":100000,"description":"Dimension bound in pixels; null clears this bound. Omission preserves it."})
}
fn optional_ratio() -> Value {
    json!({"type":["number","null"],"minimum":0.001,"maximum":1000,"description":"Width divided by height; null clears the ratio. Omission preserves it."})
}
fn breakpoint_schema() -> Value {
    json!({"type":"object","additionalProperties":false,"required":["min_width"],"properties":{
        "min_width":{"type":"number","minimum":1,"maximum":100000},
        "overrides":{"type":"object","additionalProperties":false,"properties":{
            "flow":{"type":["string","null"],"enum":["row","column","grid",null]},
            "padding":{"type":["array","null"],"items":{"type":"number","minimum":0,"maximum":10000},"minItems":4,"maxItems":4},
            "gap":{"type":["number","null"],"minimum":0,"maximum":10000},
            "columns":{"type":["integer","null"],"minimum":1,"maximum":64},
            "wrap":{"type":["boolean","null"]},"align":{"type":["string","null"],"enum":["start","center","end",null]},
            "hug_width":{"type":["boolean","null"]},"hug_height":{"type":["boolean","null"]},"clip_content":{"type":["boolean","null"]},
            "limits":{"type":["object","null"],"additionalProperties":false,"properties":{"min_width":optional_size(),"max_width":optional_size(),"min_height":optional_size(),"max_height":optional_size()},"description":"Omit/null to inherit base limits; an object replaces all four bounds, with missing/null bounds unrestricted."},
            "children":{"type":"object","patternProperties":{"^[1-9][0-9]*$":{"type":"object","additionalProperties":false,"properties":{"absolute":{"type":"boolean"},"fill_width":{"type":"boolean"},"fill_height":{"type":"boolean"},"min_width":optional_size(),"max_width":optional_size(),"min_height":optional_size(),"max_height":optional_size(),"aspect_ratio":optional_ratio()}}},"additionalProperties":false,"description":"Immediate child ID to complete sizing settings; omitted children inherit base sizing."}
        }}
    }})
}
pub(crate) fn definitions() -> Vec<ToolDef> {
    vec![
        def(
            "describe_design_layout",
            "Read persisted responsive group layouts, current frame bounds, active canvas-width breakpoints, effective settings and per-object page-resize constraints. Child settings not present in a frame default to in-layout/fixed dimensions without explicit bounds or ratio. Missing resize constraints default to scale on both axes without text reflow.",
            json!({}),
            &[],
        ),
        def(
            "set_responsive_layout",
            "Enable or patch automatic layout, native content clipping and canvas-width breakpoints on an existing group. Use group_nodes first for ungrouped objects. Creates a native rectangle boundary when first enabled, then reflows children in layer order. Omitted fields preserve existing settings; new layouts default to column, padding 24, gap 16, columns 2, wrap true, align start, fixed width/height without bounds, and 80% of the canvas dimensions.",
            json!({"group":node(),"size":{"type":"array","items":{"type":"number","minimum":1,"maximum":100000},"minItems":2,"maxItems":2},"flow":{"type":"string","enum":["row","column","grid"]},"padding":{"type":"array","items":{"type":"number","minimum":0,"maximum":10000},"minItems":4,"maxItems":4,"description":"Top,right,bottom,left in document pixels."},"gap":{"type":"number","minimum":0,"maximum":10000},"columns":{"type":"integer","minimum":1,"maximum":64},"wrap":{"type":"boolean"},"align":{"type":"string","enum":["start","center","end"]},"hug_height":{"type":"boolean"},"hug_width":{"type":"boolean"},"min_width":optional_size(),"max_width":optional_size(),"min_height":optional_size(),"max_height":optional_size(),"clip_content":{"type":"boolean"},"breakpoint_reference":{"enum":["canvas","container"],"description":"Container queries use immediate responsive parent content width; top-level frames use canvas. Content-sized query ancestors reject to avoid cycles."},"breakpoints":{"type":"array","maxItems":16,"items":breakpoint_schema(),"description":"Replace all canvas-width breakpoints; [] clears. Highest matching min_width inherits directly from base. Missing/null override fields inherit base. Unique widths required."}}),
            &["group"],
        ),
        def(
            "set_layout_child",
            "Patch an immediate content child's responsive settings. absolute=true excludes it from automatic positioning; fill_width/fill_height use available cell dimensions. Optional min/max dimensions and width-to-height aspect_ratio constrain sizing; null clears a bound or ratio. Text reflows while retaining font size. At least one setting is required; frame boundaries are excluded.",
            json!({"node":node(),"absolute":{"type":"boolean"},"fill_width":{"type":"boolean"},"fill_height":{"type":"boolean"},"min_width":optional_size(),"max_width":optional_size(),"min_height":optional_size(),"max_height":optional_size(),"aspect_ratio":optional_ratio()}),
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
            json!({"frames":editor.doc.design.frames,"constraints":editor.doc.design.constraints,"bounds":editor.doc.design.frames.keys().map(|id|json!({"group":id,"bounds":layout::bounds(&editor.doc,*id),"active_breakpoint":layout::active_breakpoint(&editor.doc,*id),"reference_width":layout::reference_width(&editor.doc,*id),"effective_frame":layout::effective_frame(&editor.doc,*id)})).collect::<Vec<_>>()}),
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
            patch!(
                flow,
                padding,
                gap,
                columns,
                wrap,
                align,
                hug_height,
                hug_width,
                min_width,
                max_width,
                min_height,
                max_height,
                clip_content,
                breakpoint_reference,
                breakpoints
            );
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
                json!({"group":group,"frame":editor.doc.design.frames[&group],"bounds":layout::bounds(&editor.doc,group),"active_breakpoint":layout::active_breakpoint(&editor.doc,group),"effective_frame":layout::effective_frame(&editor.doc,group)}),
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
            if args.as_object().is_none_or(|fields| fields.len() <= 1) {
                return Err("Set at least one child sizing option".into());
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
            macro_rules! patch{($($field:ident),*)=>{$(if let Some(value)=field(args,stringify!($field))?{child.$field=value;})*};}
            patch!(
                absolute,
                fill_width,
                fill_height,
                min_width,
                max_width,
                min_height,
                max_height,
                aspect_ratio
            );
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
                    let rule = design.constraints.entry(*node).or_default();
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

#[cfg(test)]
mod sizing_tests {
    use super::*;
    use emulsion_core::{Document, Node, command::Slot};
    use emulsion_raster::{vector::PathStyle, vector_geometry};
    use std::sync::Arc;

    fn fixture() -> (Editor, NodeId, NodeId) {
        let mut doc = Document::new(640, 480);
        let child = Command::AddNode {
            node: Box::new(Node::path(
                0,
                "Card",
                Arc::new(vector_geometry::rectangle(20., 30., 80., 40.)),
                PathStyle::default(),
                640,
                480,
            )),
            slot: Slot::TOP,
        }
        .apply(&mut doc)
        .unwrap()
        .unwrap();
        let group = Command::Group {
            ids: vec![child],
            name: "Layout".into(),
        }
        .apply(&mut doc)
        .unwrap()
        .unwrap();
        (Editor::new(doc, None), group, child)
    }
    fn call(editor: &mut Editor, name: &str, args: Value) -> Value {
        let result = crate::exec::execute(editor, name, &args);
        assert!(!result.is_error, "{name} {args}: {:?}", result.content);
        serde_json::from_str(result.content[0]["text"].as_str().unwrap()).unwrap()
    }
    fn reject(editor: &mut Editor, name: &str, args: Value) {
        let before = editor.doc.clone();
        let history = editor.history.len();
        let revision = editor.revision;
        assert!(
            crate::exec::execute(editor, name, &args).is_error,
            "Accepted {name}: {args}"
        );
        assert_eq!(editor.doc, before);
        assert_eq!(editor.history.len(), history);
        assert_eq!(editor.revision, revision);
        assert!(!editor.in_transaction());
    }
    #[test]
    fn responsive_sizing_mcp_patch_clear_inspect_and_undo() {
        let (mut editor, group, child) = fixture();
        let original = editor.doc.clone();
        call(
            &mut editor,
            "set_responsive_layout",
            json!({"group":group,"size":[320,240],"hug_width":true,"min_width":220,"max_width":500,"min_height":80,"max_height":300}),
        );
        let configured = editor.doc.clone();
        assert!(configured.design.frames[&group].hug_width);
        assert!(editor.undo());
        assert_eq!(editor.doc, original);
        assert!(editor.redo());
        call(
            &mut editor,
            "set_layout_child",
            json!({"node":child,"fill_height":true,"min_width":60,"max_width":140,"min_height":30,"max_height":70,"aspect_ratio":2}),
        );
        let sized = editor.doc.clone();
        let settings = sized.design.frames[&group].children[&child];
        assert!(settings.fill_height);
        assert_eq!(settings.aspect_ratio, Some(2.));
        let (width, height) = layout::item_dimensions(&editor.doc, child).unwrap();
        assert!((width / height - 2.).abs() < 0.001);
        assert!(editor.undo());
        assert_eq!(editor.doc, configured);
        assert!(editor.redo());
        call(
            &mut editor,
            "set_layout_child",
            json!({"node":child,"min_width":null,"max_width":null,"min_height":null,"max_height":null,"aspect_ratio":null}),
        );
        let cleared = editor.doc.design.frames[&group].children[&child];
        assert!(cleared.fill_height, "omitted boolean must stay unchanged");
        assert_eq!(
            (
                cleared.min_width,
                cleared.max_width,
                cleared.min_height,
                cleared.max_height,
                cleared.aspect_ratio
            ),
            (None, None, None, None, None)
        );
        assert!(editor.undo());
        assert_eq!(editor.doc, sized);
        call(
            &mut editor,
            "set_responsive_layout",
            json!({"group":group,"min_width":null,"max_width":null,"min_height":null,"max_height":null}),
        );
        let frame = &editor.doc.design.frames[&group];
        assert!(frame.hug_width);
        assert_eq!(
            (
                frame.min_width,
                frame.max_width,
                frame.min_height,
                frame.max_height
            ),
            (None, None, None, None)
        );
        let inspected = call(&mut editor, "describe_design_layout", json!({}));
        let frame = &inspected["frames"][group.to_string()];
        assert_eq!(frame["hug_width"], true);
        assert_eq!(frame["children"][child.to_string()]["aspect_ratio"], 2.);
        assert!(editor.undo());
        assert_eq!(editor.doc, sized);
    }
    #[test]
    fn responsive_sizing_mcp_rejects_invalid_types_bounds_and_conflicts_atomically() {
        let (mut editor, group, child) = fixture();
        for patch in [
            json!({"min_width":0}),
            json!({"max_height":100001}),
            json!({"min_width":200,"max_width":100}),
            json!({"min_height":"40"}),
            json!({"hug_width":null}),
            json!({"hug_width":1}),
        ] {
            let mut args = patch;
            args["group"] = json!(group);
            reject(&mut editor, "set_responsive_layout", args);
            assert!(
                editor.doc.design.frames.is_empty(),
                "failed enable must not leave metadata or boundary"
            );
        }
        call(
            &mut editor,
            "set_responsive_layout",
            json!({"group":group,"size":[320,240],"hug_width":true}),
        );
        for patch in [
            json!({"fill_height":null}),
            json!({"fill_height":"true"}),
            json!({"min_width":false}),
            json!({"min_height":0}),
            json!({"max_width":100001}),
            json!({"min_height":90,"max_height":40}),
            json!({"aspect_ratio":0}),
            json!({"aspect_ratio":0.0001}),
            json!({"aspect_ratio":1001}),
            json!({"aspect_ratio":"2"}),
            json!({"aspect_ratio":2,"min_width":100,"max_width":100,"min_height":100,"max_height":100}),
            json!({"fill_width":true}),
            json!({}),
        ] {
            let mut args = patch;
            args["node"] = json!(child);
            reject(&mut editor, "set_layout_child", args);
        }
        call(
            &mut editor,
            "set_layout_child",
            json!({"node":child,"min_width":70,"max_width":100}),
        );
        reject(
            &mut editor,
            "set_layout_child",
            json!({"node":child,"min_width":101}),
        );
        editor
            .execute(Command::SetLocked {
                id: child,
                locked: true,
            })
            .unwrap();
        reject(
            &mut editor,
            "set_layout_child",
            json!({"node":child,"fill_height":true}),
        );
        reject(
            &mut editor,
            "set_responsive_layout",
            json!({"group":group,"padding":[40,24,24,24]}),
        );
    }
    #[test]
    fn responsive_breakpoints_mcp_roundtrip_clear_inherit_and_atomic_errors() {
        let (mut editor, group, _) = fixture();
        let before = editor.doc.clone();
        let result = call(
            &mut editor,
            "set_responsive_layout",
            json!({"group":group,"flow":"column","gap":7,"clip_content":false,"breakpoints":[{"min_width":500,"overrides":{"flow":"row","gap":20,"clip_content":true}},{"min_width":900,"overrides":{"flow":"grid"}}]}),
        );
        assert_eq!(result["active_breakpoint"], 500.);
        assert_eq!(result["effective_frame"]["flow"], "row");
        assert_eq!(result["effective_frame"]["clip_content"], true);
        let described = call(&mut editor, "describe_design_layout", json!({}));
        assert_eq!(described["bounds"][0]["active_breakpoint"], 500.);
        let installed = editor.doc.clone();
        for invalid in [
            json!([{"min_width":500},{"min_width":500}]),
            json!([{"min_width":-1}]),
            json!([{"min_width":2000,"overrides":{"gap":-1}}]),
            json!([{"min_width":500,"overrides":{"unknown":true}}]),
        ] {
            reject(
                &mut editor,
                "set_responsive_layout",
                json!({"group":group,"breakpoints":invalid}),
            );
        }
        call(
            &mut editor,
            "set_responsive_layout",
            json!({"group":group,"breakpoints":[]}),
        );
        assert!(editor.doc.design.frames[&group].breakpoints.is_empty());
        assert!(
            !layout::effective_frame(&editor.doc, group)
                .unwrap()
                .clip_content
        );
        assert!(editor.undo());
        assert_eq!(editor.doc, installed);
        assert!(editor.undo());
        assert_eq!(editor.doc, before);
    }
    #[test]
    fn responsive_container_limits_child_overrides_mcp_are_native_and_atomic() {
        let (mut e, group, child) = fixture();
        let result = call(
            &mut e,
            "set_responsive_layout",
            json!({"group":group,"size":[300,200],"breakpoint_reference":"container","breakpoints":[{"min_width":500,"overrides":{"limits":{"max_width":250},"children":{child.to_string():{"fill_width":true,"min_width":120,"max_width":180}}}}]}),
        );
        assert_eq!(result["frame"]["breakpoint_reference"], "container");
        assert_eq!(result["effective_frame"]["max_width"], 250.);
        assert!(layout::item_dimensions(&e.doc, child).unwrap().0 <= 180.01);
        let original = e.doc.clone();
        for overrides in [
            json!({"limits":{"min_width":200,"max_width":100}}),
            json!({"children":{child.to_string():{"unknown":true}}}),
            json!({"children":{"99999":{"fill_width":true}}}),
        ] {
            reject(
                &mut e,
                "set_responsive_layout",
                json!({"group":group,"breakpoints":[{"min_width":9999,"overrides":overrides}]}),
            );
        }
        assert_eq!(e.doc, original);
        call(
            &mut e,
            "set_responsive_layout",
            json!({"group":group,"breakpoints":[{"min_width":500,"overrides":{"limits":{}}}]}),
        );
        assert_eq!(
            layout::effective_frame(&e.doc, group).unwrap().max_width,
            None
        );
        e.undo();
        assert_eq!(e.doc, original);
    }
}
