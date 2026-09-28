//! Structured native diagram editing, including ports, graph data and stencils.
use crate::{ToolDef, ToolResult};
use emulsion_core::{
    Command, Document, Editor, NodeId, NodeKind,
    command::Slot,
    diagram::{self, Endpoint, Layout, Port, Routing, ShapeKind},
};
use serde_json::{Value, json};
use std::sync::Arc;
pub(crate) const READ_ONLY: &[&str] = &[
    "list_document_stencils",
    "describe_diagram",
    "list_diagram_stencils",
    "list_diagram_library",
    "list_diagram_stencil_packs",
];
pub(crate) const DESTRUCTIVE: &[&str] = &[
    "insert_document_stencil",
    "set_diagram_shape",
    "set_diagram_object_details",
    "copy_diagram_style",
    "set_diagram_connector",
    "layout_diagram",
    "apply_diagram_theme",
];
fn node() -> Value {
    json!({"type":"integer","minimum":1})
}
fn port() -> Value {
    json!({"oneOf":[{"type":"string","enum":["auto","north","east","south","west"]},{"type":"object","additionalProperties":false,"required":["custom"],"properties":{"custom":{"type":"object","additionalProperties":false,"required":["x","y"],"properties":{"x":{"type":"number","minimum":-100,"maximum":100},"y":{"type":"number","minimum":-100,"maximum":100}}}}}],"description":"Cardinal port or {custom:{x,y}} relative to shape bounds; usual range0–1."})
}
fn endpoint_schema() -> Value {
    json!({"type":"object","additionalProperties":false,"properties":{"shape":node(),"port":port()},"required":["shape"]})
}
fn bounds() -> Value {
    json!({"type":"array","minItems":4,"maxItems":4,"items":{"type":"number"},"description":"[x,y,width,height] in document pixels; positive dimensions and coordinates bounded to1e6."})
}
fn marker() -> Value {
    json!({"type":"object","additionalProperties":false,"properties":{"kind":{"type":"string","enum":["none","block","classic","open","diamond","oval","circle_plus","many","one","mandatory_one","zero_to_one","zero_to_many","one_to_many"]},"filled":{"type":"boolean"},"size":{"type":"number","minimum":1,"maximum":100}}})
}
fn marker_patch(current: diagram::Marker, value: &Value) -> Result<diagram::Marker, String> {
    let patch = value.as_object().ok_or("Marker must be an object")?;
    let mut merged = serde_json::to_value(current).map_err(|e| e.to_string())?;
    for (key, value) in patch {
        merged
            .as_object_mut()
            .unwrap()
            .insert(key.clone(), value.clone());
    }
    decode(&merged, "marker")
}
fn label() -> Value {
    json!({"type":"string","maxLength":emulsion_core::text::MAX_CHARS})
}
fn routing() -> Value {
    json!({"type":"string","enum":["straight","orthogonal","curved","cyclical"]})
}
fn def(name: &str, description: &str, properties: Value, required: &[&str]) -> ToolDef {
    ToolDef {
        name: name.into(),
        description: format!(
            "{description} Acts on the active native document; edits preserve attachment metadata and Undo."
        ),
        input_schema: json!({"type":"object","additionalProperties":false,"properties":properties,"required":required}),
    }
}
pub(crate) fn definitions() -> Vec<ToolDef> {
    vec![
        def("list_document_stencils", "List reusable shapes automatically available from the active diagram, including imported artwork.",json!({}),&[]),
        def("insert_document_stencil", "Place a reusable copy of a shape from this diagram at a document-space center. Preserves native vector artwork and styling, remaps IDs and excludes connections and container contents. One undo step.",json!({"source":node(),"center":{"type":"array","minItems":2,"maxItems":2,"items":{"type":"number"}}}),&["source","center"]),
        def(
            "list_diagram_stencil_packs",
            "List installed local stencil packs and their reusable entries.",
            json!({}),
            &[],
        ),
        def(
            "list_diagram_library",
            "List offline diagram templates and themes with stable IDs.",
            json!({}),
            &[],
        ),
        def(
            "apply_diagram_theme",
            "Apply a discovered theme to specific object IDs, or the entire active page when nodes is omitted. One undo step; locked content rejects the edit.",
            json!({"theme":{"type":"string"},"nodes":{"type":"array","minItems":1,"items":node()}}),
            &["theme"],
        ),
        def("set_diagram_object_details", "Patch a shape's note, alternative text and HTTP(S) link. Empty values remove fields; omitted fields remain unchanged. One undo step, shared with the object context menu.", json!({"node":node(),"note":{"type":"string","maxLength":4096},"alt_text":{"type":"string","maxLength":4096},"link":{"type":"string","maxLength":4096}}), &["node"]),
        def("copy_diagram_style", "Copy native body/connector paint and label typography to diagram objects or groups. Content, geometry, IDs and connections remain unchanged; one undo step.", json!({"source":node(),"nodes":{"type":"array","minItems":1,"maxItems":10000,"items":node()}}), &["source","nodes"]),
        def(
            "describe_diagram",
            "Inspect shape/connector IDs, geometry, labels, ports, container memberships, conditional styles and data without changing the document.",
            json!({}),
            &[],
        ),
        def(
            "list_diagram_stencils",
            "Discover bundled original editable stencils by optional text query and exact category. This does not import files or access the network.",
            json!({"query":{"type":"string"},"category":{"type":"string"}}),
            &[],
        ),
        def(
            "add_diagram_shape",
            "Create an attached-graph shape with native vector body and editable text label.",
            json!({"kind":{"type":"string","enum":["process","decision","terminator","data","database","document","note","class","entity","container","swimlane","cloud"]},"bounds":bounds(),"label":label()}),
            &["kind", "bounds"],
        ),
        def(
            "insert_diagram_stencil",
            "Insert a bundled stencil by exact ID returned by list_diagram_stencils.",
            json!({"stencil":{"type":"string"},"bounds":bounds()}),
            &["stencil", "bounds"],
        ),
        def(
            "add_diagram_connector",
            "Connect existing shapes with attached ports, an editable label and automatic routing. Port defaults auto; routing defaults orthogonal.",
            json!({"source":endpoint_schema(),"target":endpoint_schema(),"label":label(),"routing":routing()}),
            &["source", "target"],
        ),
        def(
            "set_diagram_connector",
            "Patch a native connector's attachments, routing, waypoints, label, label offset and arrowheads; omitted fields stay unchanged.",
            json!({"node":node(),"source":endpoint_schema(),"target":endpoint_schema(),"routing":routing(),"waypoints":{"type":"array","maxItems":128,"items":{"type":"array","minItems":2,"maxItems":2,"items":{"type":"number"}}},"label":label(),"label_offset":{"type":"array","minItems":2,"maxItems":2,"items":{"type":"number"}},"arrow_start":{"type":"boolean"},"arrow_end":{"type":"boolean"},"start_marker":marker(),"end_marker":marker(),"jump_style":{"enum":["none","arc","gap","sharp"]},"jump_size":{"type":"number","minimum":1,"maximum":100},"corner_radius":{"type":"number","minimum":0,"maximum":100},"reverse":{"type":"boolean"},"width":{"type":"number","minimum":0.25,"maximum":100},"dash":{"type":"array","maxItems":6,"items":{"type":"number","minimum":0,"maximum":1000}},"color":{"type":"array","minItems":4,"maxItems":4,"items":{"type":"integer","minimum":0,"maximum":255}}}),
            &["node"],
        ),
        def(
            "set_diagram_shape",
            "Patch label, replace shape data or conditional fill rules, set automatic-layout lock, or move into a container (null removes containment). Object IDs and native artwork remain editable.",
            json!({"node":node(),"label":label(),"data":{"type":"object","maxProperties":64,"additionalProperties":{"type":"string","maxLength":4096}},"layout_locked":{"type":"boolean"},"container":{"type":["integer","null"],"minimum":1},"conditions":{"type":"array","maxItems":32,"items":{"type":"object","additionalProperties":false,"required":["field","equals","color"],"properties":{"field":{"type":"string","minLength":1,"maxLength":128},"equals":{"type":"string","maxLength":4096},"color":{"type":"array","minItems":4,"maxItems":4,"items":{"type":"integer","minimum":0,"maximum":255}}}}}}),
            &["node"],
        ),
        def(
            "layout_diagram",
            "Arrange top-level unlocked shapes and reroute their connections. Containers move with children; native layout/position locks are respected.",
            json!({"layout":{"type":"string","enum":["vertical","horizontal","grid","mind_map"]}}),
            &["layout"],
        ),
    ]
}
fn object<'a>(
    value: &'a Value,
    allowed: &[&str],
    label: &str,
) -> Result<&'a serde_json::Map<String, Value>, String> {
    let map = value
        .as_object()
        .ok_or_else(|| format!("{label} must be an object"))?;
    if let Some(key) = map.keys().find(|key| !allowed.contains(&key.as_str())) {
        return Err(format!("Unknown {label} field: {key}"));
    }
    Ok(map)
}
fn decode<T: serde::de::DeserializeOwned>(value: &Value, label: &str) -> Result<T, String> {
    serde_json::from_value(value.clone()).map_err(|e| format!("Invalid {label}: {e}"))
}
fn text<'a>(args: &'a Value, key: &str, default: Option<&'a str>) -> Result<&'a str, String> {
    match args.get(key) {
        None => default.ok_or_else(|| format!("Missing {key}")),
        Some(value) => value.as_str().ok_or_else(|| format!("{key} must be text")),
    }
}
fn id(value: &Value) -> Result<NodeId, String> {
    value
        .as_u64()
        .filter(|id| *id > 0)
        .ok_or_else(|| "Object ID must be a positive integer".into())
}
fn endpoint(value: &Value) -> Result<Endpoint, String> {
    let map = object(value, &["shape", "port"], "endpoint")?;
    let shape = id(map.get("shape").ok_or("Endpoint needs a shape ID")?)?;
    let port = if let Some(value) = map.get("port") {
        if value.is_object() {
            let custom = object(value, &["custom"], "port")?
                .get("custom")
                .ok_or("Custom port needs x/y")?;
            object(custom, &["x", "y"], "custom port")?;
        }
        decode::<Port>(value, "port")?
    } else {
        Port::Auto
    };
    Ok(Endpoint { shape, port })
}
fn editable(editor: &Editor, id: NodeId) -> Result<(), String> {
    if editor.doc.node(id).is_none() {
        return Err("Object does not exist".into());
    }
    for node in editor.doc.subtree(id) {
        let locks = editor.doc.layer_locks(node);
        if editor.doc.locked_ancestor(node).is_some()
            || locks.pixels
            || locks.position
            || locks.transparency
        {
            return Err("Unlock all affected diagram objects before editing".into());
        }
    }
    Ok(())
}
fn commit(editor: &mut Editor, commands: Vec<Command>) -> Result<(), String> {
    let mut trial = editor.doc.clone();
    for command in &commands {
        command.apply(&mut trial).map_err(|e| e.to_string())?;
    }
    editor.begin("Edit diagram properties");
    for command in commands {
        if let Err(e) = editor.execute(command) {
            editor.cancel();
            return Err(e.to_string());
        }
    }
    editor.end();
    Ok(())
}
fn label_command(editor: &Editor, id: NodeId, label: &str) -> Result<Command, String> {
    if label.chars().count() > emulsion_core::text::MAX_CHARS {
        return Err("Label is too long".into());
    }
    let Some(NodeKind::Text { spec, .. }) = editor.doc.node(id).map(|n| &n.kind) else {
        return Err("Label text is missing".into());
    };
    let mut spec = (**spec).clone();
    spec.text = label.into();
    spec.runs.clear();
    Ok(Command::SetText {
        id,
        spec: Box::new(spec),
    })
}
fn read(doc: &Document, name: &str, args: &Value) -> Result<Value, String> {
    match name {
        "list_document_stencils" => Ok(json!({"stencils":diagram::document_stencils(doc)})),
        "describe_diagram" => {
            let model = doc.diagram.as_deref().cloned().unwrap_or_default();
            let label = |id| match doc.node(id).map(|n| &n.kind) {
                Some(NodeKind::Text { spec, .. }) => spec.text.clone(),
                _ => String::new(),
            };
            Ok(
                json!({"scope":"active_page","shapes":model.shapes.iter().map(|(id,s)|json!({"node":id,"shape":s,"bounds":diagram::shape_bounds(doc,s),"label":label(s.label)})).collect::<Vec<_>>(),"connectors":model.edges.iter().map(|(id,e)|json!({"node":id,"connector":e,"label":label(e.label)})).collect::<Vec<_>>()}),
            )
        }
        "list_diagram_stencil_packs" => {
            let catalog =
                emulsion_io::creative_library::load(&emulsion_io::creative_library::root())
                    .map_err(|e| e.to_string())?;
            Ok(
                json!({"available":emulsion_io::diagram_packs::PACKS.iter().map(|(id,name,category)|json!({"id":id,"name":name,"category":category,"entries":emulsion_io::diagram_packs::entries(id).len()})).collect::<Vec<_>>(),"packs":catalog.assets.iter().filter(|a|a.kind==emulsion_io::creative_library::AssetKind::Stencil).map(|a|json!({"id":a.id,"name":a.name,"entries":a.variants,"path":a.path})).collect::<Vec<_>>()}),
            )
        }
        "list_diagram_library" => {
            use emulsion_core::diagram_library::{TEMPLATES, THEMES};
            Ok(
                json!({"templates":TEMPLATES.iter().map(|t|json!({"id":t.id,"name":t.name,"description":t.description})).collect::<Vec<_>>(),
                "themes":THEMES.iter().map(|t|json!({"id":t.id,"name":t.name,"fill":t.fill,"line":t.line,"text":t.text})).collect::<Vec<_>>()}),
            )
        }
        "list_diagram_stencils" => {
            let query = text(args, "query", Some(""))?;
            let category = if args.get("category").is_some() {
                Some(text(args, "category", None)?)
            } else {
                None
            };
            Ok(
                json!({"categories":diagram::stencils::CATEGORIES,"stencils":diagram::stencils::STENCILS.iter().filter(|s|s.matches(query)&&category.is_none_or(|c|c==s.category)).map(|s|json!({"id":s.id,"label":s.label,"category":s.category,"kind":s.kind,"keywords":s.keywords})).collect::<Vec<_>>()}),
            )
        }
        _ => Err(format!("Not a diagram inspection tool: {name}")),
    }
}

/// Whether this module implements the named active-document diagram tool.
pub fn is_tool(name: &str) -> bool {
    definitions().iter().any(|tool| tool.name == name)
}

fn validate_args(name: &str, args: &Value) -> Result<(), String> {
    let definition = definitions()
        .into_iter()
        .find(|tool| tool.name == name)
        .ok_or_else(|| format!("Unknown diagram tool: {name}"))?;
    let properties = definition.input_schema["properties"].as_object().unwrap();
    let allowed: Vec<_> = properties.keys().map(String::as_str).collect();
    let values = object(args, &allowed, "argument")?;
    for key in definition.input_schema["required"].as_array().unwrap() {
        let key = key.as_str().unwrap();
        if !values.contains_key(key) {
            return Err(format!("Missing argument: {key}"));
        }
    }
    Ok(())
}

/// Inspect a document snapshot without constructing or mutating an editor.
pub fn inspect(doc: &Document, name: &str, args: &Value) -> Result<ToolResult, ToolResult> {
    validate_args(name, args)
        .and_then(|()| read(doc, name, args))
        .map(|value| ToolResult::text(value.to_string()))
        .map_err(ToolResult::error)
}

fn run(editor: &mut Editor, name: &str, args: &Value) -> Result<Value, String> {
    if !READ_ONLY.contains(&name) && editor.in_transaction() {
        return Err("Finish the current edit before changing the diagram".into());
    }
    match name {
        "describe_diagram"
        | "list_document_stencils"
        | "list_diagram_stencils"
        | "list_diagram_library"
        | "list_diagram_stencil_packs" => read(&editor.doc, name, args),
        "insert_document_stencil" => {
            let source=id(&args["source"])?;
            let center:[f64;2]=decode(&args["center"],"center")?;
            let nodes=diagram::insert_document_stencil(editor,source,(center[0],center[1]))?;
            Ok(json!({"nodes":nodes}))
        }
        "set_diagram_object_details" => {
            let node=id(&args["node"])?;
            let mut fields=std::collections::BTreeMap::new();
            for key in ["note","alt_text","link"] {if let Some(value)=args.get(key){fields.insert(key.into(),value.as_str().ok_or("Details must be text")?.to_string());}}
            let commands=diagram::object_details_commands(&editor.doc,node,&fields)?;
            commit(editor,commands)?;
            Ok(json!({"node":node,"data":editor.doc.diagram.as_ref().unwrap().shapes[&node].data}))
        }
        "copy_diagram_style" => {
            let style=diagram::ObjectStyle::capture(&editor.doc,id(&args["source"])?)?;
            let targets=decode::<Vec<NodeId>>(&args["nodes"],"nodes")?;
            let commands=style.commands(&editor.doc,&targets)?;
            commit(editor,commands)?;
            Ok(json!({"nodes":targets}))
        }
        "apply_diagram_theme" => {
            let name = text(args, "theme", None)?;
            let theme = *emulsion_core::diagram_library::THEMES
                .iter()
                .find(|t| t.id == name)
                .ok_or("Unknown diagram theme")?;
            let roots = match args.get("nodes") {
                Some(v) => decode::<Vec<NodeId>>(v, "nodes")?,
                None => editor.doc.children(None),
            };
            if roots.is_empty() {
                return Err("Select at least one object".into());
            }
            let commands =
                emulsion_core::diagram_library::theme_commands(&editor.doc, &roots, theme)?;
            editor.begin("Diagram theme");
            for command in commands {
                if let Err(error) = editor.execute(command) {
                    editor.cancel();
                    return Err(error.to_string());
                }
            }
            editor.end();
            Ok(json!({"theme":theme.id,"nodes":roots}))
        }
        "add_diagram_shape" => {
            let kind: ShapeKind = decode(&args["kind"], "kind")?;
            let bounds = decode(&args["bounds"], "bounds")?;
            let node = diagram::add_shape(editor, kind, bounds, text(args, "label", Some(""))?)?;
            Ok(json!({"node":node,"shape":editor.doc.diagram.as_ref().unwrap().shapes[&node]}))
        }
        "insert_diagram_stencil" => {
            let stencil = text(args, "stencil", None)?;
            let stencil = diagram::stencils::STENCILS
                .iter()
                .find(|s| s.id == stencil)
                .ok_or("Unknown bundled stencil ID")?;
            let node = stencil.insert(editor, decode(&args["bounds"], "bounds")?)?;
            Ok(
                json!({"node":node,"stencil":stencil.id,"shape":editor.doc.diagram.as_ref().unwrap().shapes[&node]}),
            )
        }
        "add_diagram_connector" => {
            let source = endpoint(&args["source"])?;
            let target = endpoint(&args["target"])?;
            let routing = match args.get("routing") {
                Some(value) => decode(value, "routing")?,
                None => Routing::Orthogonal,
            };
            let node = diagram::connect(
                editor,
                source,
                target,
                text(args, "label", Some(""))?,
                routing,
            )?;
            Ok(json!({"node":node,"connector":editor.doc.diagram.as_ref().unwrap().edges[&node]}))
        }
        "layout_diagram" => {
            let layout = match text(args, "layout", None)? {
                "vertical" => Layout::Vertical,
                "horizontal" => Layout::Horizontal,
                "grid" => Layout::Grid,
                "mind_map" => Layout::MindMap,
                _ => return Err("Unsupported diagram layout".into()),
            };
            diagram::arrange(editor, layout)?;
            Ok(
                json!({"nodes":editor.doc.diagram.as_ref().unwrap().shapes.keys().collect::<Vec<_>>()}),
            )
        }
        "set_diagram_shape" | "set_diagram_connector" => {
            let node = id(&args["node"])?;
            editable(editor, node)?;
            let mut model = editor
                .doc
                .diagram
                .as_deref()
                .cloned()
                .ok_or("Document has no diagram")?;
            let mut commands = Vec::new();
            let label_id;
            if name == "set_diagram_shape" {
                let shape = model
                    .shapes
                    .get_mut(&node)
                    .ok_or("Node is not a diagram shape")?;
                label_id = shape.label;
                if let Some(data) = args.get("data") {
                    let mut data: std::collections::BTreeMap<String, String> =
                        decode(data, "data")?;
                    if data
                        .keys()
                        .any(|key| key.starts_with("emulsion_") || key.starts_with("drawio_"))
                    {
                        return Err("Internal stencil and interchange metadata is read-only".into());
                    }
                    data.extend(
                        shape
                            .data
                            .iter()
                            .filter(|(key, _)| {
                                key.starts_with("emulsion_") || key.starts_with("drawio_")
                            })
                            .map(|(key, value)| (key.clone(), value.clone())),
                    );
                    shape.data = data;
                }
                if let Some(value) = args.get("layout_locked") {
                    shape.layout_locked = decode(value, "layout_locked")?;
                }
                if let Some(value) = args.get("conditions") {
                    for rule in value.as_array().ok_or("conditions must be an array")? {
                        object(rule, &["field", "equals", "color"], "condition")?;
                    }
                    shape.conditions = decode(value, "conditions")?;
                }
                if let Some(value) = args.get("container") {
                    let parent = if value.is_null() {
                        None
                    } else {
                        Some(id(value)?)
                    };
                    if let Some(parent) = parent {
                        let target = model
                            .shapes
                            .get(&parent)
                            .ok_or("Container shape is missing")?;
                        if !target.kind.is_container() {
                            return Err("Target is not a diagram container".into());
                        }
                        editable(editor, parent)?;
                    }
                    commands.push(Command::MoveNode {
                        id: node,
                        slot: Slot::top_of(parent),
                    });
                    model.shapes.get_mut(&node).unwrap().container = parent;
                }
            } else {
                let edge = model
                    .edges
                    .get_mut(&node)
                    .ok_or("Node is not a diagram connector")?;
                label_id = edge.label;
                if let Some(value) = args.get("source") {
                    edge.source = endpoint(value)?;
                }
                if let Some(value) = args.get("target") {
                    edge.target = endpoint(value)?;
                }
                if let Some(value) = args.get("jump_style") { edge.jump_style=decode(value,"jump_style")?; }
                if let Some(value) = args.get("corner_radius") { edge.corner_radius=decode(value,"corner_radius")?; }
                if let Some(value) = args.get("jump_size") { edge.jump_size=decode(value,"jump_size")?; }
                if let Some(value) = args.get("routing") {
                    edge.routing = decode(value, "routing")?;
                }
                if let Some(value) = args.get("waypoints") {
                    edge.waypoints = decode(value, "waypoints")?;
                }
                if let Some(value) = args.get("label_offset") {
                    edge.label_offset = decode(value, "label_offset")?;
                }
                if let Some(value) = args.get("start_marker") {
                    edge.start_marker = marker_patch(edge.start_marker, value)?;
                }
                if let Some(value) = args.get("end_marker") {
                    edge.end_marker = marker_patch(edge.end_marker, value)?;
                }
                if let Some(value) = args.get("arrow_start") {
                    edge.arrow_start = decode(value, "arrow_start")?;
                }
                if let Some(value) = args.get("arrow_end") {
                    edge.arrow_end = decode(value, "arrow_end")?;
                }
                if let Some(value) = args.get("reverse") { if decode::<bool>(value,"reverse")? { edge.reverse(); } }
                if ["width", "dash", "color"].iter().any(|k| args.get(k).is_some()) {
                    let width = args.get("width").map(|v|decode(v,"width")).transpose()?;
                    let dash: Option<Vec<f32>> = args.get("dash").map(|v|decode(v,"dash")).transpose()?;
                    let color = args.get("color").map(|v|decode(v,"color")).transpose()?;
                    commands.push(diagram::connector_style_command(&editor.doc,node,width,dash.as_deref(),color)?);
                }
            }
            if args.get("label").is_some() {
                commands.push(label_command(editor, label_id, text(args, "label", None)?)?);
            }
            model.validate(&editor.doc)?;
            commands.push(Command::SetDiagram {
                diagram: Some(Arc::new(model)),
            });
            commit(editor, commands)?;
            Ok(if name == "set_diagram_shape" {
                json!({"node":node,"shape":editor.doc.diagram.as_ref().unwrap().shapes[&node]})
            } else {
                json!({"node":node,"connector":editor.doc.diagram.as_ref().unwrap().edges[&node]})
            })
        }
        _ => Err("Unknown diagram tool".into()),
    }
}
pub(crate) fn execute(editor: &mut Editor, name: &str, args: &Value) -> Option<ToolResult> {
    if !is_tool(name) {
        return None;
    }
    let result = (|| {
        validate_args(name, args)?;
        run(editor, name, args)
    })();
    Some(match result {
        Ok(value) => ToolResult::text(value.to_string()),
        Err(error) => ToolResult::error(error),
    })
}

#[cfg(test)]
mod tests {
    use super::*;
    use emulsion_core::Document;
    fn editor() -> Editor {
        Editor::new(Document::new(1000, 700), None)
    }
    fn call(e: &mut Editor, name: &str, args: Value) -> Value {
        let result = crate::exec::execute(e, name, &args);
        assert!(!result.is_error, "{name}: {:?}", result.content);
        serde_json::from_str(result.content[0]["text"].as_str().unwrap()).unwrap()
    }
    fn rejected(e: &mut Editor, name: &str, args: Value) {
        let before = e.doc.clone();
        let history = e.history.len();
        assert!(
            crate::exec::execute(e, name, &args).is_error,
            "{name} should reject {args}"
        );
        assert_eq!(e.doc, before);
        assert_eq!(e.history.len(), history);
    }
    fn shape(e: &mut Editor, kind: &str, x: f64) -> NodeId {
        call(
            e,
            "add_diagram_shape",
            json!({"kind":kind,"bounds":[x,80,140,80],"label":"Editable"}),
        )["node"]
            .as_u64()
            .unwrap()
    }
    #[test]
    fn diagram_contracts_are_discoverable_and_read_tools_leave_history_unchanged() {
        let defs = crate::tools::definitions();
        let mut e = editor();
        for tool in definitions() {
            assert_eq!(
                defs.iter().filter(|d| d.name == tool.name).count(),
                1,
                "{} registration",
                tool.name
            );
            assert_eq!(
                crate::tools::is_read_only(&tool.name),
                READ_ONLY.contains(&tool.name.as_str())
            );
            if DESTRUCTIVE.contains(&tool.name.as_str()) {
                assert!(crate::tools::is_destructive(&tool.name));
            }
            rejected(&mut e, &tool.name, json!({"unknown":true}));
        }
        let before = e.doc.clone();
        let catalogue = call(&mut e, "list_diagram_stencils", json!({}));
        assert!(catalogue["stencils"].as_array().unwrap().len() > 12);
        let diagram = call(&mut e, "describe_diagram", json!({}));
        assert!(diagram["shapes"].as_array().unwrap().is_empty());
        assert_eq!(e.doc, before);
        assert!(e.history.is_empty());
        let snapshot = inspect(&e.doc, "describe_diagram", &json!({})).unwrap();
        let snapshot: Value =
            serde_json::from_str(snapshot.content[0]["text"].as_str().unwrap()).unwrap();
        assert_eq!(snapshot, diagram);
        assert!(
            inspect(
                &e.doc,
                "add_diagram_shape",
                &json!({"kind":"process","bounds":[0,0,40,40]})
            )
            .is_err()
        );
        assert!(inspect(&e.doc, "describe_diagram", &json!({"unknown":true})).is_err());
        assert_eq!(e.doc, before);
    }
    #[test]
    fn connector_mcp_preserves_ports_routing_editability_and_one_undo() {
        let mut e = editor();
        let a = shape(&mut e, "process", 30.);
        let b = shape(&mut e, "decision", 430.);
        let edge=call(&mut e,"add_diagram_connector",json!({"source":{"shape":a,"port":"east"},"target":{"shape":b,"port":"west"},"label":"Next"}))["node"].as_u64().unwrap();
        let before = e.doc.clone();
        let history = e.history.len();
        call(
            &mut e,
            "set_diagram_connector",
            json!({"node":edge,"routing":"straight","target":{"shape":b,"port":{"custom":{"x":0,"y":0.25}}},"waypoints":[[270,160]],"label":"Route","label_offset":[10,-12],"arrow_start":true,"arrow_end":false}),
        );
        assert_eq!(e.history.len(), history + 1);
        let model = e.doc.diagram.as_ref().unwrap();
        let connector = &model.edges[&edge];
        assert_eq!(connector.target.port, Port::Custom { x: 0., y: 0.25 });
        assert_eq!(connector.waypoints, vec![(270., 160.)]);
        assert_eq!(connector.label_offset, (10., -12.));
        assert!(connector.arrow_start);
        assert!(!connector.arrow_end);
        assert!(
            matches!(&e.doc.node(connector.label).unwrap().kind,NodeKind::Text{spec,..} if spec.text=="Route")
        );
        e.undo();
        assert_eq!(e.doc, before);
        e.redo();
        let path = e.doc.diagram.as_ref().unwrap().edges[&edge].path;
        let old = e.doc.node(path).unwrap().clone();
        e.execute(Command::TranslateNode {
            id: a,
            dx: 25.,
            dy: 15.,
        })
        .unwrap();
        assert_ne!(e.doc.node(path).unwrap(), &old);
        e.execute(Command::SetLocked {
            id: path,
            locked: true,
        })
        .unwrap();
        rejected(
            &mut e,
            "set_diagram_connector",
            json!({"node":edge,"arrow_end":true}),
        );
    }
    #[test]
    fn shape_metadata_conditions_containers_and_layout_are_native_and_atomic() {
        let mut e = editor();
        let a = shape(&mut e, "process", 20.);
        let b = shape(&mut e, "process", 420.);
        let before = e.doc.clone();
        let history = e.history.len();
        call(
            &mut e,
            "set_diagram_shape",
            json!({"node":a,"data":{"status":"done"},"layout_locked":true,"label":"Ready","conditions":[{"field":"status","equals":"done","color":[10,200,50,255]}]}),
        );
        assert_eq!(e.history.len(), history + 1);
        let model = e.doc.diagram.as_ref().unwrap();
        let body = model.shapes[&a].body;
        assert!(
            matches!(&e.doc.node(body).unwrap().kind,NodeKind::Path{style,..} if style.fill==Some([10,200,50,255]))
        );
        e.undo();
        assert_eq!(e.doc, before);
        e.redo();
        let bounds = diagram::shape_bounds(&e.doc, &e.doc.diagram.as_ref().unwrap().shapes[&a]);
        call(&mut e, "layout_diagram", json!({"layout":"horizontal"}));
        assert_eq!(
            diagram::shape_bounds(&e.doc, &e.doc.diagram.as_ref().unwrap().shapes[&a]),
            bounds
        );
        let container = shape(&mut e, "container", 700.);
        let before = e.doc.clone();
        call(
            &mut e,
            "set_diagram_shape",
            json!({"node":b,"container":container}),
        );
        assert_eq!(e.doc.node(b).unwrap().parent, Some(container));
        assert_eq!(
            e.doc.diagram.as_ref().unwrap().shapes[&b].container,
            Some(container)
        );
        e.undo();
        assert_eq!(e.doc, before);
        rejected(
            &mut e,
            "set_diagram_shape",
            json!({"node":container,"container":container}),
        );
        e.doc.validate().unwrap();
    }
    #[test]
    fn stencils_and_invalid_requests_preserve_undo_and_existing_transactions() {
        let mut e = editor();
        let initial = e.doc.clone();
        let stencil = diagram::stencils::STENCILS[0].id;
        let root = call(
            &mut e,
            "insert_diagram_stencil",
            json!({"stencil":stencil,"bounds":[40,60,120,80]}),
        )["node"]
            .as_u64()
            .unwrap();
        assert_eq!(
            e.doc.diagram.as_ref().unwrap().shapes[&root].data["emulsion_stencil"],
            stencil
        );
        assert_eq!(e.history.len(), 1);
        e.undo();
        assert_eq!(e.doc, initial);
        e.redo();
        for (tool, args) in [
            (
                "add_diagram_shape",
                json!({"kind":"process","bounds":[1,2,-3,4]}),
            ),
            (
                "add_diagram_connector",
                json!({"source":{"shape":root,"extra":1},"target":{"shape":root}}),
            ),
            (
                "add_diagram_connector",
                json!({"source":{"shape":root,"port":{"custom":{"x":0,"y":0,"extra":1}}},"target":{"shape":root}}),
            ),
            (
                "add_diagram_connector",
                json!({"source":{"shape":root},"target":{"shape":999999}}),
            ),
            (
                "set_diagram_shape",
                json!({"node":root,"label":"No partial label edit","data":{"invalid":10}}),
            ),
            (
                "set_diagram_shape",
                json!({"node":root,"conditions":[{"field":"a","equals":"b","color":[1,2,3,255],"unknown":true}]}),
            ),
            ("list_diagram_stencils", json!({"query":false})),
        ] {
            rejected(&mut e, tool, args);
        }
        e.begin("Gesture");
        rejected(
            &mut e,
            "add_diagram_shape",
            json!({"kind":"process","bounds":[1,2,30,40]}),
        );
        assert!(e.in_transaction());
        e.cancel();
    }
    #[test]
    fn stencil_metadata_survives_user_data_replacement() {
        let mut e = editor();
        let node = call(
            &mut e,
            "insert_diagram_stencil",
            json!({"stencil":"server","bounds":[20,20,120,90]}),
        )["node"]
            .as_u64()
            .unwrap();
        call(
            &mut e,
            "set_diagram_shape",
            json!({"node":node,"data":{"owner":"Operations"}}),
        );
        let data = &e.doc.diagram.as_ref().unwrap().shapes[&node].data;
        assert_eq!(data["emulsion_stencil"], "server");
        assert_eq!(data["owner"], "Operations");
        rejected(
            &mut e,
            "set_diagram_shape",
            json!({"node":node,"data":{"emulsion_stencil":"rectangle"}}),
        );
    }
    #[test]
    fn library_discovery_and_theme_share_core_and_undo() {
        let doc = emulsion_core::diagram_library::TEMPLATES[0]
            .build()
            .unwrap();
        let mut e = Editor::new(doc.clone(), None);
        let library = call(&mut e, "list_diagram_library", json!({}));
        assert_eq!(library["templates"].as_array().unwrap().len(), 8);
        let packs = call(&mut e, "list_diagram_stencil_packs", json!({}));
        assert_eq!(packs["available"].as_array().unwrap().len(), 12);
        assert!(
            packs["available"]
                .as_array()
                .unwrap()
                .iter()
                .all(|p| p["entries"].as_u64().unwrap() > 0)
        );
        call(&mut e, "apply_diagram_theme", json!({"theme":"blue"}));
        assert_ne!(e.doc, doc);
        e.undo();
        assert_eq!(e.doc, doc);
        assert!(
            execute(&mut e, "apply_diagram_theme", &json!({"theme":"missing"}))
                .unwrap()
                .is_error
        );
    }
    #[test]
    fn crossing_style_patch_is_discoverable_and_undoable() {
        let mut e=Editor::new(emulsion_core::diagram_library::TEMPLATES[0].build().unwrap(),None);
        let edge=*e.doc.diagram.as_ref().unwrap().edges.keys().next().unwrap();let before=e.doc.clone();
        call(&mut e,"set_diagram_connector",json!({"node":edge,"jump_style":"gap","jump_size":14}));
        assert_eq!(e.doc.diagram.as_ref().unwrap().edges[&edge].jump_style,diagram::JumpStyle::Gap);
        for value in [json!(0),json!(101),json!("large")] {rejected(&mut e,"set_diagram_connector",json!({"node":edge,"jump_size":value}));}
        e.undo();assert_eq!(e.doc,before);
        let defs=definitions();assert!(defs.iter().find(|d|d.name=="set_diagram_connector").unwrap().input_schema["properties"].get("jump_style").is_some());
    }
    #[test]
    fn curved_marker_patch_moves_and_rejects_invalid_values_atomically() {
        let doc = emulsion_core::diagram_library::TEMPLATES[0]
            .build()
            .unwrap();
        let mut e = Editor::new(doc, None);
        let edge = *e.doc.diagram.as_ref().unwrap().edges.keys().next().unwrap();
        call(
            &mut e,
            "set_diagram_connector",
            json!({"node":edge,"routing":"curved","start_marker":{"kind":"diamond","filled":false,"size":16},"end_marker":{"kind":"zero_to_many","size":18},"arrow_start":true}),
        );
        call(
            &mut e,
            "set_diagram_connector",
            json!({"node":edge,"start_marker":{"size":12}}),
        );
        let model = &e.doc.diagram.as_ref().unwrap().edges[&edge];
        assert_eq!(model.routing, Routing::Curved);
        assert_eq!(model.start_marker.kind, diagram::MarkerKind::Diamond);
        let source = model.source.shape;
        e.execute(Command::TranslateNode {
            id: source,
            dx: 27.,
            dy: 11.,
        })
        .unwrap();
        e.doc.validate().unwrap();
        for marker in [
            json!({"size":0}),
            json!({"size":101}),
            json!({"kind":"unknown"}),
            json!({"unexpected":true}),
        ] {
            rejected(
                &mut e,
                "set_diagram_connector",
                json!({"node":edge,"end_marker":marker}),
            );
        }
    }
    #[test]
    fn connector_style_rounding_reversal_is_atomic_and_persistent() {
        let mut e=editor();
        let a=shape(&mut e,"process",30.);
        let b=shape(&mut e,"decision",430.);
        let id=call(&mut e,"add_diagram_connector",json!({"source":{"shape":a,"port":"east"},"target":{"shape":b,"port":"west"}}))["node"].as_u64().unwrap();
        let before=e.doc.clone(); let history=e.history.len();
        call(&mut e,"set_diagram_connector",json!({"node":id,"waypoints":[[250,90],[250,220]],"corner_radius":6,"width":3,"dash":[8,4,0,4],"color":[250,130,30,255],"reverse":true,"end_marker":{"kind":"diamond","size":20,"filled":false}}));
        assert_eq!(e.history.len(),history+1);
        let edge=&e.doc.diagram.as_ref().unwrap().edges[&id];
        assert_eq!(edge.source.shape,b); assert_eq!(edge.target.shape,a);
        assert_eq!(edge.waypoints,vec![(250.,220.),(250.,90.)]);
        assert_eq!(edge.corner_radius,6.);
        assert!(matches!(&e.doc.node(edge.path).unwrap().kind,NodeKind::Path{path,style,..} if style.width==3. && style.dash_count==4 && style.stroke==Some([250,130,30,255]) && path.subpaths[0].anchors.iter().any(|a|a.h_in!=a.p)));
        let serialized=serde_json::to_value(e.doc.diagram.as_deref().unwrap()).unwrap();
        let restored:diagram::Diagram=serde_json::from_value(serialized).unwrap();
        assert_eq!(&restored,e.doc.diagram.as_deref().unwrap());
        e.undo(); assert_eq!(e.doc,before);
        for patch in [json!({"width":0}),json!({"dash":[0,0]}),json!({"dash":[1,2,3,4,5,6,7]}),json!({"corner_radius":-1}),json!({"reverse":"true"})] {
            let mut args=patch;args["node"]=json!(id);rejected(&mut e,"set_diagram_connector",args);
        }
        call(&mut e,"set_diagram_connector",json!({"node":id,"routing":"cyclical"}));
        e.execute(Command::TranslateNode{id:a,dx:20.,dy:30.}).unwrap();
        e.doc.validate().unwrap();
        assert_eq!(e.doc.diagram.as_ref().unwrap().edges[&id].routing,Routing::Cyclical);
    }
    #[test]
    fn object_style_and_details_keep_geometry_and_one_undo() {
        let mut e=editor();let a=shape(&mut e,"process",30.);let b=shape(&mut e,"decision",430.);
        let edge=call(&mut e,"add_diagram_connector",json!({"source":{"shape":a},"target":{"shape":b}}))["node"].as_u64().unwrap();
        let source=e.doc.diagram.as_ref().unwrap().shapes[&a].body;
        let target=e.doc.diagram.as_ref().unwrap().shapes[&b].body;
        let NodeKind::Path{path,style,..}=&e.doc.node(source).unwrap().kind else{panic!()};
        let mut style=*style;style.fill=Some([178,242,235,255]);
        e.execute(Command::SetPath{id:source,path:path.clone(),style}).unwrap();
        let before=e.doc.clone();let history=e.history.len();
        call(&mut e,"copy_diagram_style",json!({"source":a,"nodes":[b,edge]}));
        assert_eq!(e.history.len(),history+1);
        let NodeKind::Path{path:old,..}=&before.node(target).unwrap().kind else{panic!()};
        assert!(matches!(&e.doc.node(target).unwrap().kind,NodeKind::Path{path,style,..} if path==old && style.fill==Some([178,242,235,255])));
        let path=e.doc.diagram.as_ref().unwrap().edges[&edge].path;
        assert!(matches!(&e.doc.node(path).unwrap().kind,NodeKind::Path{style,..} if style.fill.is_none()));
        e.undo();assert_eq!(e.doc,before);
        call(&mut e,"set_diagram_object_details",json!({"node":a,"note":"Operations","alt_text":"Start of flow","link":"https://example.org/flow"}));
        let data=&e.doc.diagram.as_ref().unwrap().shapes[&a].data;
        assert_eq!(data["note"],"Operations");assert_eq!(data["drawio_link"],"https://example.org/flow");
        assert!(e.doc.design.interactions.contains_key(&a));
        e.undo();assert_eq!(e.doc,before);
        rejected(&mut e,"set_diagram_object_details",json!({"node":a,"note":"must roll back","link":"javascript:alert(1)"}));
        e.execute(Command::SetLocked{id:a,locked:true}).unwrap();
        rejected(&mut e,"set_diagram_object_details",json!({"node":a,"note":"locked"}));
        rejected(&mut e,"copy_diagram_style",json!({"source":b,"nodes":[a]}));
    }

    #[test]
    fn imported_object_toolbox_has_mcp_placement_and_atomic_undo() {
        let mut e=editor();let a=shape(&mut e,"process",30.);
        let entries=call(&mut e,"list_document_stencils",json!({}));assert_eq!(entries["stencils"][0]["source"],a);
        let before=e.doc.clone();let history=e.history.len();
        let placed=call(&mut e,"insert_document_stencil",json!({"source":a,"center":[500,400]}));
        let id=placed["nodes"][0].as_u64().unwrap();assert_ne!(id,a);
        assert_eq!(e.history.len(),history+1);assert_eq!(e.doc.diagram.as_ref().unwrap().shapes.len(),2);
        e.undo();assert_eq!(e.doc,before);
        rejected(&mut e,"insert_document_stencil",json!({"source":a,"center":[1e7,0]}));
    }

}
