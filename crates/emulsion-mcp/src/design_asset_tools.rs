//! Active-page MCP access to native components, styles, charts and tables.
use crate::{ToolDef, ToolResult};
use emulsion_core::{
    Editor, NodeId,
    design_charts::{self as charts, Chart, Kind},
    design_components as components, design_styles as styles,
};
use serde_json::{Value, json};

pub(crate) const READ_ONLY: &[&str] = &[
    "list_design_components",
    "list_design_styles",
    "list_design_charts",
];
pub(crate) const DESTRUCTIVE: &[&str] = &[
    "update_design_component",
    "reset_design_component",
    "switch_design_component",
    "detach_design_component",
    "set_design_component_overrides",
    "update_design_style",
    "reset_design_style",
    "detach_design_style",
    "remove_design_style",
    "update_design_chart",
    "detach_design_chart",
];

fn def(name: &str, description: &str, properties: Value, required: &[&str]) -> ToolDef {
    ToolDef {
        name: name.into(),
        description: format!(
            "{description} Active page only; native editable data, local libraries, and Undo are preserved."
        ),
        input_schema: json!({"type":"object","properties":properties,"required":required,"additionalProperties":false}),
    }
}
fn node() -> Value {
    json!({"type":"integer","minimum":1,"description":"Existing native object/group ID."})
}
fn nodes() -> Value {
    json!({"type":"array","items":node(),"minItems":1,"maxItems":emulsion_core::document::MAX_NODES,"uniqueItems":true})
}
fn name() -> Value {
    json!({"type":"string","minLength":1,"maxLength":80})
}
fn pair() -> Value {
    json!({"type":"array","items":{"type":"number"},"minItems":2,"maxItems":2})
}
pub(crate) fn definitions() -> Vec<ToolDef> {
    let mut defs = vec![
        def(
            "list_design_components",
            "List component definitions, named variants, source IDs, and linked instances; does not change the document.",
            json!({}),
            &[],
        ),
        def(
            "create_design_component",
            "Create a Default component from selected node IDs and link the resulting group. Nested instances retain their links; dependency cycles are rejected.",
            json!({"nodes":nodes(),"name":name()}),
            &["nodes", "name"],
        ),
        def(
            "insert_design_component",
            "Insert a linked instance of a saved component variant. offset is a translation from the stored source, default [24,24].",
            json!({"name":name(),"variant":name(),"offset":pair()}),
            &["name"],
        ),
        def(
            "update_design_component",
            "Publish an edited instance to its variant and all linked instances on this page. Preserves stable member IDs, placement and explicit property overrides. Refreshes nested dependent components on this page.",
            json!({"node":node()}),
            &["node"],
        ),
        def(
            "save_design_component_variant",
            "Save the edited instance as a new named variant and link that instance to it; existing variants remain unchanged.",
            json!({"node":node(),"variant":name()}),
            &["node", "variant"],
        ),
        def(
            "reset_design_component",
            "Reset selected instance artwork from its saved variant and clear its explicit property overrides.",
            json!({"node":node()}),
            &["node"],
        ),
        def(
            "switch_design_component",
            "Switch an instance to another saved variant, replacing child overrides while retaining its group ID and top-left position.",
            json!({"node":node(),"variant":name()}),
            &["node", "variant"],
        ),
        def(
            "detach_design_component",
            "Remove the instance link and keep its editable artwork.",
            json!({"node":node()}),
            &["node"],
        ),
        def(
            "set_design_component_overrides",
            "Choose which current member properties survive component publishing. Omitted booleans are false; {} clears all flags. Appearance groups paint, typography, blend and effects; opacity/visibility are separate. Content supports text and raster images; geometry supports text, paths and images. Reset instance restores source values and clears flags. instance defaults to the innermost owning linked group.",
            json!({"node":node(),"instance":node(),"auto_overrides":{"type":"boolean","description":"Automatically preserve subsequent native edits; new instances default true, legacy instances false. Omission preserves policy."},"overrides":{"type":"object","properties":{"content":{"type":"boolean"},"appearance":{"type":"boolean"},"geometry":{"type":"boolean"},"opacity":{"type":"boolean"},"visibility":{"type":"boolean"},"fill":{"type":"boolean"},"stroke":{"type":"boolean"},"stroke_width":{"type":"boolean"},"font_family":{"type":"boolean"},"font_size":{"type":"boolean"},"text_color":{"type":"boolean"},"position":{"type":"boolean"},"size":{"type":"boolean"},"effects":{"type":"boolean"}},"additionalProperties":false}}),
            &["node", "overrides"],
        ),
        def(
            "list_design_styles",
            "List named appearance styles and node links without modifying the document.",
            json!({}),
            &[],
        ),
        def(
            "create_design_style",
            "Capture one node's appearance under a unique name and link the source. Typography samples the first character.",
            json!({"node":node(),"name":name()}),
            &["node", "name"],
        ),
        def(
            "apply_design_style",
            "Apply an existing named style to node IDs and link them. Keeps content and geometry; typography applies uniformly.",
            json!({"nodes":nodes(),"name":name()}),
            &["nodes", "name"],
        ),
        def(
            "update_design_style",
            "Publish source appearance to every linked consumer on this page. Source formatting remains unchanged; consumers receive the sampled appearance.",
            json!({"node":node(),"name":name()}),
            &["node", "name"],
        ),
        def(
            "reset_design_style",
            "Restore saved appearance on linked targets, replacing local formatting overrides.",
            json!({"nodes":nodes()}),
            &["nodes"],
        ),
        def(
            "detach_design_style",
            "Remove target style links, keeping the current appearance.",
            json!({"nodes":nodes()}),
            &["nodes"],
        ),
        def(
            "rename_design_style",
            "Rename a library style and update every reference to it on this page.",
            json!({"name":name(),"new_name":name()}),
            &["name", "new_name"],
        ),
        def(
            "remove_design_style",
            "Remove the library style and detach its consumers, keeping their appearance.",
            json!({"name":name()}),
            &["name"],
        ),
        def(
            "list_design_charts",
            "List chart/table group IDs, editable data models, and current object bounds.",
            json!({}),
            &[],
        ),
        def(
            "detach_design_chart",
            "Remove the chart data association while preserving all editable native artwork.",
            json!({"node":node()}),
            &["node"],
        ),
    ];
    let properties = json!({
        "kind":{"type":"string","enum":["bar","line","pie","table","area","scatter","stacked_bar","donut"]},
        "title":{"type":"string","maxLength":200},
        "rows":{"type":"array","minItems":2,"maxItems":51,"items":{"type":"array","minItems":2,"maxItems":9,"items":{"type":"string","maxLength":1000}},"description":"Header row followed by 1–50 data rows. Numeric series use decimal strings. All rows must have equal width; pie needs exactly one nonnegative series."},
        "colors":{"type":"array","minItems":1,"maxItems":16,"items":{"type":"array","minItems":4,"maxItems":4,"items":{"type":"integer","minimum":0,"maximum":255}}},
        "size":{"type":"array","items":{"type":"number","minimum":160,"maximum":10000},"minItems":2,"maxItems":2,"description":"[width,height] in document pixels."},
        "origin":pair(),
        "x_axis":chart_axis_schema(),"y_axis":chart_axis_schema(),
        "formulas":{"type":"boolean","description":"Evaluate = expressions with A1 references, arithmetic, SUM/AVERAGE/MIN/MAX/COUNT/ABS/ROUND. Source formulas remain editable. Default false preserves literal cells."},
        "merges":{"type":"array","maxItems":459,"description":"Table merged ranges. Zero-based row/column; row0 is the header. Covered data is retained; [] unmerges all. Regions may not overlap.","items":{"type":"object","additionalProperties":false,"required":["row","column","rows","columns"],"properties":{"row":{"type":"integer","minimum":0,"maximum":50},"column":{"type":"integer","minimum":0,"maximum":8},"rows":{"type":"integer","minimum":1,"maximum":51},"columns":{"type":"integer","minimum":1,"maximum":9}}}}
    });
    defs.push(def("add_design_chart","Create a native editable chart or table. Omitted fields use the chosen kind's example data; default kind bar, size[600,400], origin[0,0].",properties.clone(),&[]));
    let mut properties = properties;
    properties
        .as_object_mut()
        .unwrap()
        .insert("node".into(), node());
    defs.push(def("update_design_chart","Patch the selected data-backed chart/table. Omitted fields remain unchanged. Rebuilds native children, keeps group identity; omitted origin preserves current bounds position.",properties,&["node"]));
    defs
}

fn string<'a>(args: &'a Value, key: &str) -> Result<&'a str, String> {
    args.get(key)
        .and_then(Value::as_str)
        .filter(|s| !s.trim().is_empty())
        .ok_or_else(|| format!("{key} must be a nonempty string"))
}
fn id(args: &Value) -> Result<NodeId, String> {
    args.get("node")
        .and_then(Value::as_u64)
        .filter(|id| *id > 0)
        .ok_or_else(|| "node must be a positive integer".into())
}
fn ids(args: &Value) -> Result<Vec<NodeId>, String> {
    let values = args
        .get("nodes")
        .and_then(Value::as_array)
        .filter(|v| !v.is_empty() && v.len() <= emulsion_core::document::MAX_NODES)
        .ok_or("nodes must be a nonempty bounded array of IDs")?;
    let mut ids = Vec::with_capacity(values.len());
    let mut seen = std::collections::HashSet::new();
    for value in values {
        let id = value
            .as_u64()
            .filter(|id| *id > 0)
            .ok_or("nodes must contain positive integers")?;
        if !seen.insert(id) {
            return Err("nodes must not contain duplicate IDs".into());
        }
        ids.push(id);
    }
    Ok(ids)
}
fn optional_pair(args: &Value, key: &str, default: (f64, f64)) -> Result<(f64, f64), String> {
    match args.get(key) {
        None => Ok(default),
        Some(value) => {
            let pair: (f64, f64) = serde_json::from_value(value.clone())
                .map_err(|_| format!("{key} must contain exactly two numbers"))?;
            if !pair.0.is_finite() || !pair.1.is_finite() {
                return Err(format!("{key} must contain finite numbers"));
            }
            Ok(pair)
        }
    }
}
fn chart_axis_schema() -> Value {
    json!({"type":"object","additionalProperties":false,"description":"Patch axis settings; omitted fields stay unchanged, null min/max restores automatic range. X numeric bounds/ticks affect scatter; other Cartesian charts use category labels.","properties":{"min":{"type":["number","null"],"minimum":-1e12,"maximum":1e12},"max":{"type":["number","null"],"minimum":-1e12,"maximum":1e12},"ticks":{"type":"integer","minimum":2,"maximum":20},"label":{"type":"string","maxLength":100},"show_labels":{"type":"boolean"}}})
}
fn patch_chart_axis(
    axis: &mut emulsion_core::design_charts::Axis,
    value: &Value,
) -> Result<(), String> {
    let patch = value.as_object().ok_or("Axis settings must be an object")?;
    let mut merged = serde_json::to_value(&*axis).unwrap();
    for (key, value) in patch {
        merged
            .as_object_mut()
            .unwrap()
            .insert(key.clone(), value.clone());
    }
    *axis = serde_json::from_value(merged).map_err(|e| format!("Invalid axis settings: {e}"))?;
    Ok(())
}
fn patch_chart(chart: &mut Chart, args: &Value) -> Result<(), String> {
    macro_rules! field {
        ($field:ident) => {
            if let Some(value) = args.get(stringify!($field)) {
                chart.$field = serde_json::from_value(value.clone())
                    .map_err(|e| format!("Invalid {}: {e}", stringify!($field)))?;
            }
        };
    }
    field!(kind);
    field!(title);
    field!(rows);
    field!(colors);
    field!(size);
    field!(merges);
    field!(formulas);
    if let Some(value) = args.get("x_axis") {
        patch_chart_axis(&mut chart.x_axis, value)?;
    }
    if let Some(value) = args.get("y_axis") {
        patch_chart_axis(&mut chart.y_axis, value)?;
    }
    chart.validate()
}
fn run(editor: &mut Editor, name: &str, args: &Value) -> Result<Value, String> {
    if !READ_ONLY.contains(&name) && editor.in_transaction() {
        return Err("Finish the current edit before changing Design assets.".into());
    }
    match name {
        "list_design_components" => Ok(
            json!({"scope":"active_page","definitions":editor.doc.design.components,"instances":editor.doc.design.component_links,"nested_components":true}),
        ),
        "list_design_styles" => Ok(
            json!({"scope":"active_page","styles":editor.doc.design.saved_styles,"links":editor.doc.design.style_links}),
        ),
        "list_design_charts" => Ok(
            json!({"scope":"active_page","charts":editor.doc.design.charts.iter().map(|(id,chart)|json!({"node":id,"chart":chart,"bounds":match emulsion_core::geometry::node_bounds(&editor.doc,*id) { Ok(bounds) => json!(bounds.map(|b|[b.x,b.y,b.w,b.h])), Err(error) => json!({"error":error.to_string()}) }})).collect::<Vec<_>>()}),
        ),
        "set_design_component_overrides" => {
            let node = id(args)?;
            let instance = if let Some(value) = args.get("instance") {
                value
                    .as_u64()
                    .filter(|id| *id > 0)
                    .ok_or("instance must be a positive native group ID")?
            } else {
                components::owner(&editor.doc, node)
                    .ok_or("Select a member of a linked component")?
            };
            let flags: components::Overrides = serde_json::from_value(args["overrides"].clone())
                .map_err(|e| format!("Invalid overrides: {e}"))?;
            let auto = args
                .get("auto_overrides")
                .map(|v| v.as_bool().ok_or("auto_overrides must be boolean"))
                .transpose()?;
            components::configure_overrides(editor, instance, node, flags, auto)?;
            Ok(json!({"instance":instance,"node":node,"overrides":flags}))
        }
        "create_design_component" => {
            let root = components::create(editor, &ids(args)?, string(args, "name")?)?;
            Ok(json!({"node":root,"instance":editor.doc.design.component_links[&root]}))
        }
        "insert_design_component" => {
            let variant = if args.get("variant").is_some() {
                string(args, "variant")?
            } else {
                "Default"
            };
            let root = components::insert(
                editor,
                string(args, "name")?,
                variant,
                optional_pair(args, "offset", (24., 24.))?,
            )?;
            Ok(json!({"node":root,"instance":editor.doc.design.component_links[&root]}))
        }
        "update_design_component"
        | "save_design_component_variant"
        | "reset_design_component"
        | "switch_design_component"
        | "detach_design_component" => {
            let node = id(args)?;
            let nodes = if name == "update_design_component" {
                let link = editor
                    .doc
                    .design
                    .component_links
                    .get(&node)
                    .ok_or("Node is not a linked component")?;
                editor
                    .doc
                    .design
                    .component_links
                    .iter()
                    .filter(|(_, other)| {
                        other.component == link.component && other.variant == link.variant
                    })
                    .map(|(id, _)| *id)
                    .collect::<Vec<_>>()
            } else {
                vec![node]
            };
            match name {
                "update_design_component" => components::update(editor, node, None)?,
                "save_design_component_variant" => {
                    components::update(editor, node, Some(string(args, "variant")?))?
                }
                "reset_design_component" => components::reset(editor, node, None)?,
                "switch_design_component" => {
                    components::reset(editor, node, Some(string(args, "variant")?))?
                }
                _ => components::detach(editor, node)?,
            };
            Ok(
                json!({"node":node,"nodes":nodes,"instance":editor.doc.design.component_links.get(&node)}),
            )
        }
        "create_design_style" => {
            let node = id(args)?;
            let name = string(args, "name")?;
            styles::create(editor, node, name)?;
            Ok(json!({"node":node,"name":editor.doc.design.style_links[&node]}))
        }
        "apply_design_style" => {
            let nodes = ids(args)?;
            let name = string(args, "name")?;
            let saved = editor
                .doc
                .design
                .saved_styles
                .get(name)
                .cloned()
                .ok_or("Saved style does not exist on this page")?;
            let name = styles::apply(editor, &nodes, name, &saved)?;
            Ok(json!({"nodes":nodes,"name":name}))
        }
        "update_design_style" => {
            let node = id(args)?;
            let name = string(args, "name")?;
            styles::update(editor, name, node)?;
            let nodes: Vec<_> = editor
                .doc
                .design
                .style_links
                .iter()
                .filter(|(_, link)| *link == name)
                .map(|(id, _)| *id)
                .collect();
            Ok(json!({"node":node,"nodes":nodes,"name":name}))
        }
        "reset_design_style" | "detach_design_style" => {
            let nodes = ids(args)?;
            for node in &nodes {
                if !editor.doc.design.style_links.contains_key(node) {
                    return Err(format!("Node {node} is not linked to a saved style"));
                }
            }
            if name == "reset_design_style" {
                styles::reset(editor, &nodes)?;
            } else {
                styles::detach(editor, &nodes)?;
            }
            Ok(json!({"nodes":nodes}))
        }
        "rename_design_style" => {
            let old = string(args, "name")?;
            let new = string(args, "new_name")?;
            if !editor.doc.design.saved_styles.contains_key(old) {
                return Err("Saved style does not exist on this page".into());
            }
            styles::rename(editor, old, new)?;
            Ok(json!({"name":new.trim()}))
        }
        "remove_design_style" => {
            let name = string(args, "name")?;
            styles::remove(editor, name)?;
            Ok(json!({"removed":name}))
        }
        "add_design_chart" | "update_design_chart" => {
            let existing = if name == "update_design_chart" {
                Some(id(args)?)
            } else {
                None
            };
            let mut chart = if let Some(node) = existing {
                editor
                    .doc
                    .design
                    .charts
                    .get(&node)
                    .cloned()
                    .ok_or("Node is not a data-backed chart or table")?
            } else {
                let kind = match args.get("kind") {
                    Some(value) => serde_json::from_value(value.clone())
                        .map_err(|_| "kind must be bar, line, pie or table")?,
                    None => Kind::Bar,
                };
                Chart::example(kind)
            };
            patch_chart(&mut chart, args)?;
            let position = existing
                .map(|id| emulsion_core::geometry::node_bounds(&editor.doc, id))
                .transpose()
                .map_err(|e| e.to_string())?
                .flatten()
                .map_or((0., 0.), |b| (b.x as f64, b.y as f64));
            let node = charts::apply(
                editor,
                existing,
                chart,
                optional_pair(args, "origin", position)?,
            )?;
            Ok(json!({"node":node,"chart":editor.doc.design.charts[&node]}))
        }
        "detach_design_chart" => {
            let node = id(args)?;
            if !editor.doc.design.charts.contains_key(&node) {
                return Err("Node is not a data-backed chart or table".into());
            }
            charts::detach(editor, node)?;
            Ok(json!({"node":node,"detached":true}))
        }
        _ => Err("Unknown Design asset tool".into()),
    }
}

pub(crate) fn execute(editor: &mut Editor, name: &str, args: &Value) -> Option<ToolResult> {
    let definition = definitions().into_iter().find(|tool| tool.name == name)?;
    let result = (|| {
        let values = args
            .as_object()
            .ok_or("Tool arguments must be a JSON object")?;
        let properties = definition.input_schema["properties"].as_object().unwrap();
        for key in values.keys() {
            if !properties.contains_key(key) {
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
mod tests {
    use super::*;
    use emulsion_core::{Command, Document, Node, NodeKind, command::Slot, text::TextSpec};
    fn editor() -> Editor {
        Editor::new(Document::new(800, 600), None)
    }
    fn text(editor: &mut Editor, value: &str) -> NodeId {
        editor
            .execute(Command::AddNode {
                node: Box::new(Node::text(
                    0,
                    "Text",
                    TextSpec {
                        text: value.into(),
                        x: 30.,
                        y: 40.,
                        ..Default::default()
                    },
                    800,
                    600,
                )),
                slot: Slot::TOP,
            })
            .unwrap()
            .unwrap()
    }
    fn call(editor: &mut Editor, name: &str, args: Value) -> Value {
        let result = crate::exec::execute(editor, name, &args);
        assert!(!result.is_error, "{name}: {:?}", result.content);
        serde_json::from_str(result.content[0]["text"].as_str().unwrap()).unwrap()
    }
    fn rejected(editor: &mut Editor, name: &str, args: Value) {
        let before = editor.doc.clone();
        let history = editor.history.len();
        let result = crate::exec::execute(editor, name, &args);
        assert!(result.is_error, "{name} should reject {args}");
        assert_eq!(editor.doc, before);
        assert_eq!(editor.history.len(), history);
    }
    #[test]
    fn registered_asset_contracts_are_unique_and_permissions_match() {
        let definitions = crate::tools::definitions();
        let mut e = editor();
        for tool in super::definitions() {
            assert_eq!(
                definitions.iter().filter(|d| d.name == tool.name).count(),
                1,
                "{} registration",
                tool.name
            );
            assert_eq!(tool.input_schema["additionalProperties"], false);
            assert_eq!(
                crate::tools::is_read_only(&tool.name),
                READ_ONLY.contains(&tool.name.as_str()),
                "{} permissions",
                tool.name
            );
            if DESTRUCTIVE.contains(&tool.name.as_str()) {
                assert!(crate::tools::is_destructive(&tool.name));
            }
            assert!(
                super::execute(&mut e, &tool.name, &json!({"unexpected":1}))
                    .unwrap()
                    .is_error
            );
        }
        assert!(super::execute(&mut e, "other_tool", &json!({})).is_none());
        let before = e.doc.clone();
        for tool in READ_ONLY {
            let value = call(&mut e, tool, json!({}));
            assert_eq!(value["scope"], "active_page");
        }
        assert_eq!(e.doc, before);
        assert!(e.history.is_empty());
    }
    #[test]
    fn component_mcp_workflow_preserves_native_identity_variants_and_undo() {
        let mut e = editor();
        let child = text(&mut e, "Native text");
        let root = call(
            &mut e,
            "create_design_component",
            json!({"nodes":[child],"name":"Card"}),
        )["node"]
            .as_u64()
            .unwrap();
        let second = call(
            &mut e,
            "insert_design_component",
            json!({"name":"Card","offset":[200,0]}),
        )["node"]
            .as_u64()
            .unwrap();
        e.execute(Command::SetOpacity {
            id: child,
            opacity: 0.4,
        })
        .unwrap();
        let before = e.doc.clone();
        let history = e.history.len();
        call(&mut e, "update_design_component", json!({"node":root}));
        assert_eq!(e.history.len(), history + 1);
        assert!(
            e.doc
                .subtree(second)
                .iter()
                .any(|id| e
                    .doc
                    .node(*id)
                    .is_some_and(|n| matches!(n.kind, NodeKind::Text { .. }) && n.opacity == 0.4))
        );
        e.undo();
        assert_eq!(e.doc, before);
        e.redo();
        call(
            &mut e,
            "save_design_component_variant",
            json!({"node":root,"variant":"Alternate"}),
        );
        call(
            &mut e,
            "switch_design_component",
            json!({"node":second,"variant":"Alternate"}),
        );
        assert_eq!(e.doc.design.component_links[&second].variant, "Alternate");
        call(&mut e, "reset_design_component", json!({"node":second}));
        call(&mut e, "detach_design_component", json!({"node":second}));
        assert!(e.doc.node(second).is_some());
        assert!(!e.doc.design.component_links.contains_key(&second));
        call(
            &mut e,
            "create_design_component",
            json!({"nodes":[root],"name":"Nested"}),
        );
        let listing = call(&mut e, "list_design_components", json!({}));
        assert_eq!(
            listing["definitions"]["Card"]["variants"]
                .as_object()
                .unwrap()
                .len(),
            2
        );
    }
    #[test]
    fn styles_mcp_workflow_updates_every_consumer_and_preserves_locked_targets() {
        let mut e = editor();
        let a = text(&mut e, "Heading");
        let b = text(&mut e, "Other words");
        call(
            &mut e,
            "create_design_style",
            json!({"node":a,"name":"Heading"}),
        );
        call(
            &mut e,
            "apply_design_style",
            json!({"nodes":[b],"name":"Heading"}),
        );
        e.execute(Command::SetOpacity {
            id: a,
            opacity: 0.3,
        })
        .unwrap();
        e.execute(Command::SetLocked {
            id: b,
            locked: true,
        })
        .unwrap();
        rejected(
            &mut e,
            "update_design_style",
            json!({"node":a,"name":"Heading"}),
        );
        e.execute(Command::SetLocked {
            id: b,
            locked: false,
        })
        .unwrap();
        let before = e.doc.clone();
        let history = e.history.len();
        let result = call(
            &mut e,
            "update_design_style",
            json!({"node":a,"name":"Heading"}),
        );
        assert_eq!(result["nodes"].as_array().unwrap().len(), 2);
        assert_eq!(e.doc.node(b).unwrap().opacity, 0.3);
        assert_eq!(e.history.len(), history + 1);
        e.undo();
        assert_eq!(e.doc, before);
        e.redo();
        e.execute(Command::SetOpacity {
            id: b,
            opacity: 0.8,
        })
        .unwrap();
        call(&mut e, "reset_design_style", json!({"nodes":[b]}));
        assert_eq!(e.doc.node(b).unwrap().opacity, 0.3);
        call(
            &mut e,
            "rename_design_style",
            json!({"name":"Heading","new_name":"Title"}),
        );
        assert_eq!(e.doc.design.style_links[&a], "Title");
        call(&mut e, "detach_design_style", json!({"nodes":[b]}));
        call(&mut e, "remove_design_style", json!({"name":"Title"}));
        assert!(e.doc.design.saved_styles.is_empty());
        assert!(
            matches!(&e.doc.node(b).unwrap().kind,NodeKind::Text{spec,..} if spec.text=="Other words")
        );
    }
    #[test]
    fn chart_mcp_converts_data_atomically_and_keeps_editable_artwork() {
        let mut e = editor();
        for kind in ["bar", "line", "pie", "table"] {
            call(
                &mut e,
                "add_design_chart",
                json!({"kind":kind,"origin":[20,30]}),
            );
        }
        let id = *e.doc.design.charts.keys().next().unwrap();
        let before = e.doc.clone();
        let history = e.history.len();
        call(
            &mut e,
            "update_design_chart",
            json!({"node":id,"kind":"table","title":"Revenue","rows":[["Region","Value"],["North","12"]]}),
        );
        assert_eq!(e.doc.design.charts[&id].kind, Kind::Table);
        assert_eq!(e.history.len(), history + 1);
        assert!(e.doc.node(id).is_some());
        e.undo();
        assert_eq!(e.doc, before);
        e.redo();
        rejected(
            &mut e,
            "update_design_chart",
            json!({"node":id,"kind":"pie","rows":[["Region","Value"],["North","-1"]]}),
        );
        let children = e.doc.subtree(id);
        let pixels = e.doc.nodes.clone();
        call(&mut e, "detach_design_chart", json!({"node":id}));
        assert_eq!(e.doc.nodes, pixels);
        assert_eq!(e.doc.subtree(id), children);
        assert!(!e.doc.design.charts.contains_key(&id));
        rejected(&mut e, "detach_design_chart", json!({"node":id}));
    }
    #[test]
    fn malformed_json_and_active_transactions_cannot_partially_mutate_assets() {
        let mut e = editor();
        let id = text(&mut e, "Safe");
        for (tool, args) in [
            (
                "create_design_component",
                json!({"nodes":[id,id],"name":"Duplicate"}),
            ),
            ("create_design_style", json!({"node":true,"name":"Wrong"})),
            (
                "add_design_chart",
                json!({"kind":"table","rows":[["A","B"],["R",12]]}),
            ),
            ("add_design_chart", json!({"colors":[[256,0,0,255]]})),
            ("add_design_chart", json!({"origin":[1,2,3]})),
            ("add_design_chart", json!({"title":null})),
            ("add_design_chart", json!({"rows":[["A","B"],["R"]]})),
            ("list_design_styles", json!(null)),
            (
                "rename_design_style",
                json!({"name":"missing","new_name":"missing"}),
            ),
            ("reset_design_style", json!({"nodes":[id]})),
        ] {
            rejected(&mut e, tool, args);
        }
        e.begin("Interactive edit");
        rejected(&mut e, "add_design_chart", json!({"kind":"bar"}));
        assert!(e.in_transaction());
        call(&mut e, "list_design_charts", json!({}));
        e.cancel();
    }
}

#[cfg(test)]
mod component_override_tests {
    use super::*;
    use emulsion_core::{Command, Document, Node, NodeKind, command::Slot, text::TextSpec};
    fn call(e: &mut Editor, name: &str, args: Value) -> Value {
        let result = execute(e, name, &args).unwrap();
        assert!(!result.is_error, "{:?}", result.content);
        serde_json::from_str(result.content[0]["text"].as_str().unwrap()).unwrap()
    }
    #[test]
    fn nested_component_override_tool_uses_native_history_and_strict_flags() {
        let mut e = Editor::new(Document::new(800, 600), None);
        let text = e
            .execute(Command::AddNode {
                node: Box::new(Node::text(
                    0,
                    "Title",
                    TextSpec {
                        text: "Original".into(),
                        ..Default::default()
                    },
                    800,
                    600,
                )),
                slot: Slot::TOP,
            })
            .unwrap()
            .unwrap();
        let first = call(
            &mut e,
            "create_design_component",
            json!({"nodes":[text],"name":"Badge"}),
        )["node"]
            .as_u64()
            .unwrap();
        let outer = call(
            &mut e,
            "create_design_component",
            json!({"nodes":[first],"name":"Panel"}),
        )["node"]
            .as_u64()
            .unwrap();
        let before = e.doc.clone();
        let history = e.history.len();
        call(
            &mut e,
            "set_design_component_overrides",
            json!({"node":text,"overrides":{"content":true,"opacity":true}}),
        );
        assert_eq!(e.history.len(), history + 1);
        assert!(components::overrides_for(&e.doc, first, text).content);
        e.undo();
        assert_eq!(e.doc, before);
        e.redo();
        let listing = call(&mut e, "list_design_components", json!({}));
        assert_eq!(listing["nested_components"], true);
        assert!(
            listing["instances"][first.to_string()]["members"]
                .as_object()
                .is_some()
        );
        let before = e.doc.clone();
        assert!(
            execute(
                &mut e,
                "set_design_component_overrides",
                &json!({"node":text,"overrides":{"unknown":true}})
            )
            .unwrap()
            .is_error
        );
        assert!(
            execute(
                &mut e,
                "set_design_component_overrides",
                &json!({"node":outer,"overrides":{"content":true}})
            )
            .unwrap()
            .is_error
        );
        assert_eq!(e.doc, before);
        assert!(matches!(
            e.doc.node(text).unwrap().kind,
            NodeKind::Text { .. }
        ));
    }
}

#[cfg(test)]
mod advanced_chart_tests {
    use super::*;
    use emulsion_core::Document;
    fn call(editor: &mut Editor, name: &str, args: Value) -> Value {
        let result = execute(editor, name, &args).unwrap();
        assert!(!result.is_error, "{:?}", result.content);
        serde_json::from_str(result.content[0]["text"].as_str().unwrap()).unwrap()
    }
    #[test]
    fn advanced_charts_partial_axes_clear_merges_and_undo_are_native() {
        let mut e = Editor::new(Document::new(800, 600), None);
        for kind in ["area", "scatter", "stacked_bar", "donut"] {
            let id = call(
                &mut e,
                "add_design_chart",
                json!({"kind":kind,"y_axis":{"min":0,"max":100,"ticks":6,"label":"Amount"}}),
            )["node"]
                .as_u64()
                .unwrap();
            let before = e.doc.clone();
            let history = e.history.len();
            call(
                &mut e,
                "update_design_chart",
                json!({"node":id,"y_axis":{"min":null,"show_labels":false}}),
            );
            let axis = &e.doc.design.charts[&id].y_axis;
            assert_eq!(axis.min, None);
            assert_eq!(axis.max, Some(100.));
            assert_eq!(axis.ticks, 6);
            assert!(!axis.show_labels);
            assert_eq!(e.history.len(), history + 1);
            e.undo();
            assert_eq!(e.doc, before);
            for axis in [
                json!({"ticks":1}),
                json!({"ticks":"5"}),
                json!({"min":120}),
                json!({"show_labels":null}),
                json!({"unknown":1}),
            ] {
                assert!(
                    execute(
                        &mut e,
                        "update_design_chart",
                        &json!({"node":id,"y_axis":axis})
                    )
                    .unwrap()
                    .is_error
                );
                assert_eq!(e.doc, before);
            }
        }
        let id = call(
            &mut e,
            "add_design_chart",
            json!({"kind":"table","merges":[{"row":0,"column":0,"rows":1,"columns":2}]}),
        )["node"]
            .as_u64()
            .unwrap();
        let before = e.doc.clone();
        call(
            &mut e,
            "update_design_chart",
            json!({"node":id,"merges":[]}),
        );
        assert!(e.doc.design.charts[&id].merges.is_empty());
        e.undo();
        assert_eq!(e.doc, before);
        assert!(
            execute(
                &mut e,
                "update_design_chart",
                &json!({"node":id,"merges":[{"row":0,"column":0,"rows":99,"columns":2}]})
            )
            .unwrap()
            .is_error
        );
        assert_eq!(e.doc, before);
    }
}

#[cfg(test)]
mod formula_tests {
    use super::*;
    #[test]
    fn design_chart_formulas_mcp_retains_sources_recalculates_and_rejects_cycles() {
        let mut e = Editor::new(emulsion_core::Document::new(600, 400), None);
        let result = crate::exec::execute(
            &mut e,
            "add_design_chart",
            &json!({"kind":"bar","formulas":true,"rows":[["Item","Value"],["A","10"],["B","=B2*2"]]}),
        );
        assert!(!result.is_error, "{result:?}");
        let id = *e.doc.design.charts.keys().next().unwrap();
        let before = e.doc.clone();
        assert_eq!(
            e.doc.design.charts[&id].resolved_rows().unwrap()[2][1],
            "20"
        );
        for args in [
            json!({"node":id,"formulas":"yes"}),
            json!({"node":id,"rows":[["A","B"],["A","=B2"]]}),
        ] {
            assert!(crate::exec::execute(&mut e, "update_design_chart", &args).is_error);
            assert_eq!(e.doc, before);
        }
        let result = crate::exec::execute(
            &mut e,
            "update_design_chart",
            &json!({"node":id,"rows":[["Item","Value"],["A","15"],["B","=B2*2"]]}),
        );
        assert!(!result.is_error, "{result:?}");
        assert_eq!(
            e.doc.design.charts[&id].resolved_rows().unwrap()[2][1],
            "30"
        );
        e.undo();
        assert_eq!(e.doc, before);
    }
}
