//! Structured native diagram editing, including ports, graph data and stencils.
use crate::{ToolDef, ToolResult};
use emulsion_core::{
    Command, Document, Editor, NodeId, NodeKind,
    command::Slot,
    diagram::{self, Endpoint, Layout, Port, Routing, ShapeKind},
};
use serde_json::{Value, json};
use std::sync::Arc;
pub(crate) const READ_ONLY: &[&str] = &["describe_diagram", "list_diagram_stencils"];
pub(crate) const DESTRUCTIVE: &[&str] = &[
    "set_diagram_shape",
    "set_diagram_connector",
    "layout_diagram",
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
fn label() -> Value {
    json!({"type":"string","maxLength":emulsion_core::text::MAX_CHARS})
}
fn routing() -> Value {
    json!({"type":"string","enum":["straight","orthogonal"]})
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
            json!({"node":node(),"source":endpoint_schema(),"target":endpoint_schema(),"routing":routing(),"waypoints":{"type":"array","maxItems":128,"items":{"type":"array","minItems":2,"maxItems":2,"items":{"type":"number"}}},"label":label(),"label_offset":{"type":"array","minItems":2,"maxItems":2,"items":{"type":"number"}},"arrow_start":{"type":"boolean"},"arrow_end":{"type":"boolean"}}),
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
        "describe_diagram" | "list_diagram_stencils" => read(&editor.doc, name, args),
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
                    shape.data = decode(data, "data")?;
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
                if let Some(value) = args.get("routing") {
                    edge.routing = decode(value, "routing")?;
                }
                if let Some(value) = args.get("waypoints") {
                    edge.waypoints = decode(value, "waypoints")?;
                }
                if let Some(value) = args.get("label_offset") {
                    edge.label_offset = decode(value, "label_offset")?;
                }
                if let Some(value) = args.get("arrow_start") {
                    edge.arrow_start = decode(value, "arrow_start")?;
                }
                if let Some(value) = args.get("arrow_end") {
                    edge.arrow_end = decode(value, "arrow_end")?;
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
}
