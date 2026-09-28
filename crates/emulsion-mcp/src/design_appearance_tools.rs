//! Missing Design selection operations, using the same native commands as UI.
use crate::{ToolDef, ToolResult};
use emulsion_core::{
    Command, Editor, NodeId, NodeKind,
    command::Alignment,
    design_appearance::Appearance,
    design_formatting as formatting,
    layer_links::{Arrange, ArrangeTarget, Distribution},
};
use serde_json::{Value, json};

pub(crate) const READ_ONLY: &[&str] = &["describe_object_appearance"];
pub(crate) const DESTRUCTIVE: &[&str] = &["remove_text_background"];
fn def(name: &str, description: &str, properties: Value, required: &[&str]) -> ToolDef {
    ToolDef {
        name: name.into(),
        description: format!(
            "{description} Active document/page only. Mutations are atomic, respect locks, and form one Undo step."
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
fn scalar(min: f32, max: f32) -> Value {
    json!({"type":"number","minimum":min,"maximum":max})
}
pub(crate) fn definitions() -> Vec<ToolDef> {
    vec![
        def(
            "describe_object_appearance",
            "Read native appearance, rectangle radius, and any recognized native text background. The versioned appearance object can be passed unchanged to apply_object_appearance; text typography samples the first character.",
            json!({"node":node()}),
            &["node"],
        ),
        def(
            "copy_object_appearance",
            "Copy one source object's native appearance to target nodes without using the clipboard or creating style links. Keeps target text content, geometry, masks and identity. Text formatting is uniform, sampled from the source's first character. Existing named-style links stay attached as local overrides.",
            json!({"source":node(),"nodes":nodes()}),
            &["source", "nodes"],
        ),
        def(
            "apply_object_appearance",
            "Apply the complete versioned appearance object returned by describe_object_appearance to target nodes. Preserves content and geometry. This is a full appearance replacement, not a partial property patch; use existing set_text, set_path, set_style and set_opacity for patches.",
            json!({"nodes":nodes(),"appearance":{"type":"object","description":"Exact appearance envelope from describe_object_appearance: {version:1,data:{...native appearance...}}.","required":["version","data"],"properties":{"version":{"type":"integer","const":1},"data":{"type":"object"}},"additionalProperties":false}}),
            &["nodes", "appearance"],
        ),
        def(
            "set_text_background",
            "Create or update an editable rounded vector backdrop grouped behind text; node is text or its native backdrop group. Omitted fields preserve an existing backdrop or use yellow, padding [16,12], radius 8. Always refits to current text bounds, including rotation and scale. Returns text/group/background IDs. Text remains editable.",
            json!({"node":node(),"color":{"type":"string","pattern":"^#[0-9a-fA-F]{6}([0-9a-fA-F]{2})?$"},"padding":{"type":"array","items":scalar(0.,1000.),"minItems":2,"maxItems":2},"radius":scalar(0.,10000.)}),
            &["node"],
        ),
        def(
            "refit_text_background",
            "Refit an existing native text backdrop after editing text. Keeps color and radius; infers padding from the current text-local top-left inset. To specify new padding explicitly, use set_text_background. Does not create a missing backdrop.",
            json!({"node":node()}),
            &["node"],
        ),
        def(
            "remove_text_background",
            "Remove only the recognized native backdrop rectangle and ungroup its text. Existing unrelated groups are rejected. Returns the surviving editable text ID.",
            json!({"node":node()}),
            &["node"],
        ),
        def(
            "set_corner_radius",
            "Set round-rectangle corners on native axis-aligned rectangle paths. Radius is clamped to half the shorter side. Preserves fill, stroke and IDs; arbitrary paths and responsive frame boundaries are rejected.",
            json!({"nodes":nodes(),"radius":scalar(0.,100000.)}),
            &["nodes", "radius"],
        ),
        def(
            "arrange_nodes",
            "Align or distribute multiple native objects/groups. Operations use whole-pixel content bounds, preserve editable data, and normalize parent/child selections. Distribution needs at least three root objects; gap operations equalize spacing. target defaults to selected_nodes; selection means the nonempty pixel selection. Stack order stays unchanged.",
            json!({"nodes":nodes(),"operation":{"type":"string","enum":["align_left","align_horizontal_center","align_right","align_top","align_vertical_center","align_bottom","distribute_left","distribute_horizontal_center","distribute_right","distribute_top","distribute_vertical_center","distribute_bottom","distribute_horizontal_gap","distribute_vertical_gap"]},"target":{"type":"string","enum":["selected_nodes","canvas","selection"],"default":"selected_nodes"}}),
            &["nodes", "operation"],
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
    let list = args
        .get("nodes")
        .and_then(Value::as_array)
        .filter(|v| !v.is_empty() && v.len() <= emulsion_core::document::MAX_NODES)
        .ok_or("nodes must be a nonempty bounded array")?;
    let mut seen = std::collections::HashSet::new();
    list.iter()
        .map(|v| {
            let id = v
                .as_u64()
                .filter(|v| *v > 0)
                .ok_or("nodes must contain positive integers")?;
            if !seen.insert(id) {
                return Err("nodes must not contain duplicate IDs".into());
            }
            Ok(id)
        })
        .collect()
}
fn number(value: &Value, min: f32, max: f32, name: &str) -> Result<f32, String> {
    value
        .as_f64()
        .filter(|v| v.is_finite() && *v >= f64::from(min) && *v <= f64::from(max))
        .map(|v| v as f32)
        .ok_or_else(|| format!("{name} must be a number from {min} to {max}"))
}
fn color(value: &Value) -> Result<[u8; 4], String> {
    let h = value
        .as_str()
        .and_then(|v| v.strip_prefix('#'))
        .filter(|v| matches!(v.len(), 6 | 8) && v.bytes().all(|b| b.is_ascii_hexdigit()))
        .ok_or("color must be #RRGGBB or #RRGGBBAA")?;
    let mut rgba = [255; 4];
    for (i, v) in rgba.iter_mut().enumerate().take(h.len() / 2) {
        *v = u8::from_str_radix(&h[i * 2..i * 2 + 2], 16).map_err(|_| "Invalid color")?;
    }
    Ok(rgba)
}
fn commit(editor: &mut Editor, label: &str, commands: Vec<Command>) -> Result<(), String> {
    // Validate every command against the resulting state before touching history.
    let mut trial = editor.doc.clone();
    for command in &commands {
        command
            .clone()
            .apply(&mut trial)
            .map_err(|e| e.to_string())?;
    }
    editor.begin(label);
    for command in commands {
        if let Err(error) = editor.execute(command) {
            editor.cancel();
            return Err(error.to_string());
        }
    }
    editor.end();
    Ok(())
}
fn appearance_commands(
    editor: &Editor,
    appearance: &Appearance,
    nodes: &[NodeId],
) -> Result<Vec<Command>, String> {
    appearance.validate()?;
    let mut commands = Vec::new();
    for id in nodes {
        let node = editor
            .doc
            .node(*id)
            .ok_or_else(|| format!("Node {id} does not exist"))?;
        commands.extend(appearance.commands(node));
    }
    Ok(commands)
}
fn backdrop_settings(
    editor: &Editor,
    node: NodeId,
) -> Result<(NodeId, Option<(NodeId, NodeId)>, [u8; 4], [f32; 2], f32), String> {
    let (text, pair) = formatting::text_backdrop(&editor.doc, node)
        .ok_or("Choose editable text or its native background group")?;
    let (mut color, mut padding, mut radius) = ([255, 235, 120, 255], [16., 12.], 8.);
    if let Some((_, bg)) = pair {
        let NodeKind::Path { style, .. } = &editor.doc.node(bg).ok_or("Missing backdrop")?.kind
        else {
            return Err("Invalid backdrop".into());
        };
        color = style.fill.unwrap_or([0; 4]);
        let (x, y, _, _, r) = formatting::background_geometry(&editor.doc, text, bg)
            .ok_or("Backdrop geometry is not a rectangle")?;
        radius = r as f32;
        let NodeKind::Text { spec, .. } = &editor.doc.node(text).ok_or("Missing text")?.kind else {
            return Err("Invalid text".into());
        };
        let b = emulsion_core::text::layout(spec).bounds();
        padding = [
            (f64::from(b.x) - x).max(0.) as f32,
            (f64::from(b.y) - y).max(0.) as f32,
        ];
    }
    Ok((text, pair, color, padding, radius))
}
fn describe(editor: &Editor, node: NodeId) -> Result<Value, String> {
    let native = editor.doc.node(node).ok_or("Node does not exist")?;
    let rectangle = match &native.kind {
        NodeKind::Path { path, .. } => formatting::rectangle(path)
            .map(|(x, y, w, h, r)| json!({"bounds":[x,y,w,h],"radius":r})),
        _ => None,
    };
    let backdrop=backdrop_settings(editor,node).ok().and_then(|(text,pair,color,padding,radius)|pair.map(|(group,background)|json!({"text":text,"group":group,"background":background,"color":color,"padding":padding,"radius":radius,"requires_refit_after_text_edit":true})));
    Ok(
        json!({"node":node,"appearance":{"version":1,"data":Appearance::capture(native)},"rectangle":rectangle,"text_background":backdrop}),
    )
}
fn run(editor: &mut Editor, name: &str, args: &Value) -> Result<Value, String> {
    if !READ_ONLY.contains(&name) && editor.in_transaction() {
        return Err("Finish the current edit before changing appearance".into());
    }
    match name {
        "describe_object_appearance" => describe(editor, id(args, "node")?),
        "copy_object_appearance" | "apply_object_appearance" => {
            let nodes = ids(args)?;
            let appearance = if name == "copy_object_appearance" {
                Appearance::capture(
                    editor
                        .doc
                        .node(id(args, "source")?)
                        .ok_or("Source object does not exist")?,
                )
            } else {
                let envelope = &args["appearance"];
                let object = envelope
                    .as_object()
                    .ok_or("appearance must be a versioned object")?;
                if object.len() != 2
                    || envelope["version"].as_u64() != Some(1)
                    || !envelope["data"].is_object()
                {
                    return Err("appearance requires version 1 and native data from describe_object_appearance".into());
                }
                let data: Appearance = serde_json::from_value(envelope["data"].clone())
                    .map_err(|e| format!("Invalid native appearance: {e}"))?;
                // Prevent ignored/unknown native fields from turning an invalid
                // request into an apparently successful partial update.
                if serde_json::to_value(&data).map_err(|e| e.to_string())? != envelope["data"] {
                    return Err("Use the complete, unmodified native appearance returned by describe_object_appearance".into());
                }
                data
            };
            let commands = appearance_commands(editor, &appearance, &nodes)?;
            commit(editor, "Apply object appearance", commands)?;
            Ok(json!({"nodes":nodes,"applied":true,"style_links_changed":false}))
        }
        "set_corner_radius" => {
            let nodes = ids(args)?;
            let radius = number(&args["radius"], 0., 100000., "radius")?;
            let commands = formatting::corners(&editor.doc, &nodes, radius)?;
            commit(editor, "Corner radius", commands)?;
            Ok(json!({"nodes":nodes,"radius_requested":radius}))
        }
        "set_text_background" | "refit_text_background" | "remove_text_background" => {
            let node = id(args, "node")?;
            let (_, pair, mut rgba, mut padding, mut radius) = backdrop_settings(editor, node)?;
            if name != "set_text_background" && pair.is_none() {
                return Err("The text has no native background to refit or remove".into());
            }
            if let Some(value) = args.get("color") {
                rgba = color(value)?;
            }
            if let Some(value) = args.get("padding") {
                let values = value
                    .as_array()
                    .filter(|v| v.len() == 2)
                    .ok_or("padding must contain [horizontal, vertical]")?;
                padding = [
                    number(&values[0], 0., 1000., "horizontal padding")?,
                    number(&values[1], 0., 1000., "vertical padding")?,
                ];
            }
            if let Some(value) = args.get("radius") {
                radius = number(value, 0., 10000., "radius")?;
            }
            let remove = name == "remove_text_background";
            let (commands, result) =
                formatting::background(&editor.doc, node, rgba, padding, radius, remove)?;
            commit(
                editor,
                if remove {
                    "Remove text background"
                } else {
                    "Text background"
                },
                commands,
            )?;
            let mut info = describe(editor, result)?;
            info["selected_node"] = json!(result);
            info["removed"] = json!(remove);
            Ok(info)
        }
        "arrange_nodes" => {
            let nodes = ids(args)?;
            let operation = match args["operation"].as_str() {
                Some("align_left") => Arrange::Align(Alignment::Left),
                Some("align_horizontal_center") => Arrange::Align(Alignment::HorizontalCenter),
                Some("align_right") => Arrange::Align(Alignment::Right),
                Some("align_top") => Arrange::Align(Alignment::Top),
                Some("align_vertical_center") => Arrange::Align(Alignment::VerticalCenter),
                Some("align_bottom") => Arrange::Align(Alignment::Bottom),
                Some("distribute_left") => Arrange::Distribute(Distribution::Left),
                Some("distribute_horizontal_center") => {
                    Arrange::Distribute(Distribution::HorizontalCenter)
                }
                Some("distribute_right") => Arrange::Distribute(Distribution::Right),
                Some("distribute_top") => Arrange::Distribute(Distribution::Top),
                Some("distribute_vertical_center") => {
                    Arrange::Distribute(Distribution::VerticalCenter)
                }
                Some("distribute_bottom") => Arrange::Distribute(Distribution::Bottom),
                Some("distribute_horizontal_gap") => {
                    Arrange::Distribute(Distribution::HorizontalGap)
                }
                Some("distribute_vertical_gap") => Arrange::Distribute(Distribution::VerticalGap),
                _ => return Err("Unknown arrange operation".into()),
            };
            let target = match args.get("target").map(Value::as_str) {
                None | Some(Some("selected_nodes")) => ArrangeTarget::SelectedLayers,
                Some(Some("canvas")) => ArrangeTarget::Canvas,
                Some(Some("selection")) => ArrangeTarget::PixelSelection,
                _ => return Err("target must be selected_nodes, canvas or selection".into()),
            };
            commit(
                editor,
                operation.label(),
                vec![Command::ArrangeLayers {
                    ids: nodes.clone(),
                    operation,
                    target,
                }],
            )?;
            Ok(json!({"nodes":nodes,"arranged":true}))
        }
        _ => Err("Unknown Design appearance tool".into()),
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
        Ok(value) => ToolResult::text(value.to_string()),
        Err(error) => ToolResult::error(error),
    })
}

#[cfg(test)]
#[path = "design_appearance_tools_tests.rs"]
mod tests;
