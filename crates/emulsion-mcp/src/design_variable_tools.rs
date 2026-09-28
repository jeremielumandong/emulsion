//! Typed variable authoring uses the same native document history as the editor.
use crate::{ToolDef, ToolResult};
use emulsion_core::{Editor, design_variables as variables};
use serde_json::{Value, json};
pub(crate) const READ_ONLY: &[&str] = &["list_design_variables"];
pub(crate) const DESTRUCTIVE: &[&str] = &[
    "set_design_variable",
    "rename_design_variable",
    "remove_design_variable",
    "bind_design_variable",
];
pub(crate) fn definitions() -> Vec<ToolDef> {
    let name = json!({"type":"string","minLength":1,"maxLength":80});
    let property = json!({"type":"string","enum":["fill","stroke","text_color","opacity","font_size","stroke_width","frame_gap","frame_padding"]});
    [
        ("list_design_variables","Inspect active-page typed variables and property bindings without editing.",json!({}),vec![]),
        ("set_design_variable","Create or update a color/number variable and all bound consumers atomically. A locked consumer rejects the entire edit.",json!({"name":name,"value":{"oneOf":[{"type":"object","properties":{"type":{"const":"color"},"value":{"type":"array","items":{"type":"integer","minimum":0,"maximum":255},"minItems":4,"maxItems":4}},"required":["type","value"],"additionalProperties":false},{"type":"object","properties":{"type":{"const":"number"},"value":{"type":"number","minimum":-1e9,"maximum":1e9}},"required":["type","value"],"additionalProperties":false}]}}),vec!["name","value"]),
        ("rename_design_variable","Rename a variable and preserve all consumer bindings.",json!({"name":name,"new_name":name}),vec!["name","new_name"]),
        ("remove_design_variable","Remove a variable and unlink its consumers while keeping their resolved appearance.",json!({"name":name}),vec!["name"]),
        ("bind_design_variable","Bind selected object properties to a typed variable. name:null unlinks while retaining current values. Numeric limits follow the target property; all targets are validated before one undoable edit.",json!({"nodes":{"type":"array","items":{"type":"integer","minimum":1},"minItems":1,"maxItems":emulsion_core::document::MAX_NODES,"uniqueItems":true},"property":property,"name":{"type":["string","null"],"minLength":1,"maxLength":80}}),vec!["nodes","property","name"]),
    ].into_iter().map(|(name,description,properties,required)|ToolDef{name:name.into(),description:description.into(),input_schema:json!({"type":"object","properties":properties,"required":required,"additionalProperties":false})}).collect()
}
pub(crate) fn execute(editor: &mut Editor, name: &str, args: &Value) -> Option<ToolResult> {
    if !definitions().iter().any(|t| t.name == name) {
        return None;
    }
    Some(match run(editor, name, args) {
        Ok(v) => ToolResult::text(v.to_string()),
        Err(e) => ToolResult::error(e),
    })
}
fn run(editor: &mut Editor, name: &str, args: &Value) -> Result<Value, String> {
    let args = args.as_object().ok_or("Expected an argument object.")?;
    let allowed: &[&str] = match name {
        "list_design_variables" => &[],
        "set_design_variable" => &["name", "value"],
        "rename_design_variable" => &["name", "new_name"],
        "remove_design_variable" => &["name"],
        _ => &["nodes", "property", "name"],
    };
    if args.keys().any(|k| !allowed.contains(&k.as_str())) {
        return Err("Unknown variable argument.".into());
    }
    let string = |key| {
        args.get(key)
            .and_then(Value::as_str)
            .ok_or_else(|| format!("{key} must be a string."))
    };
    match name {
        "list_design_variables" => {}
        "set_design_variable" => variables::set(
            editor,
            string("name")?,
            serde_json::from_value(args.get("value").ok_or("Missing value.")?.clone())
                .map_err(|e| e.to_string())?,
        )?,
        "rename_design_variable" => {
            variables::rename(editor, string("name")?, string("new_name")?)?
        }
        "remove_design_variable" => variables::remove(editor, string("name")?)?,
        _ => {
            let nodes: Vec<u64> =
                serde_json::from_value(args.get("nodes").ok_or("Missing nodes.")?.clone())
                    .map_err(|e| e.to_string())?;
            let property =
                serde_json::from_value(args.get("property").ok_or("Missing property.")?.clone())
                    .map_err(|e| e.to_string())?;
            let variable = match args.get("name") {
                Some(Value::Null) => None,
                Some(Value::String(s)) => Some(s.as_str()),
                _ => return Err("name must be a variable name or null.".into()),
            };
            variables::bind(editor, &nodes, property, variable)?;
        }
    }
    Ok(
        json!({"variables":editor.doc.design.variables,"bindings":editor.doc.design.variable_bindings}),
    )
}

#[cfg(test)]
mod tests {
    use super::*;
    use emulsion_core::{Command, Document, Node, NodeKind, command::Slot, text::TextSpec};
    fn call(e: &mut Editor, tool: &str, args: Value) -> Value {
        let result = execute(e, tool, &args).unwrap();
        assert!(!result.is_error, "{:?}", result.content);
        serde_json::from_str(result.content[0]["text"].as_str().unwrap()).unwrap()
    }
    fn text(e: &mut Editor) -> u64 {
        e.execute(Command::AddNode {
            node: Box::new(Node::text(
                0,
                "Bound text",
                TextSpec {
                    text: "Native".into(),
                    underline: true,
                    ..Default::default()
                },
                400,
                300,
            )),
            slot: Slot::TOP,
        })
        .unwrap()
        .unwrap()
    }
    #[test]
    fn design_variable_mcp_publish_rename_remove_and_locks_are_atomic() {
        let mut e = Editor::new(Document::new(400, 300), None);
        let a = text(&mut e);
        let b = text(&mut e);
        call(
            &mut e,
            "set_design_variable",
            json!({"name":"Accent","value":{"type":"color","value":[20,60,220,255]}}),
        );
        let before = e.doc.clone();
        let history = e.history.len();
        call(
            &mut e,
            "bind_design_variable",
            json!({"nodes":[a,b],"property":"text_color","name":"Accent"}),
        );
        assert_eq!(e.history.len(), history + 1);
        e.undo();
        assert_eq!(e.doc, before);
        e.redo();
        for id in [a, b] {
            let NodeKind::Text { spec, .. } = &e.doc.node(id).unwrap().kind else {
                panic!()
            };
            assert_eq!(spec.color, [20, 60, 220, 255]);
            assert!(spec.underline);
        }
        e.execute(Command::SetLocked {
            id: b,
            locked: true,
        })
        .unwrap();
        let protected = e.doc.clone();
        let history = e.history.len();
        assert!(
            execute(
                &mut e,
                "set_design_variable",
                &json!({"name":"Accent","value":{"type":"color","value":[220,60,20,255]}})
            )
            .unwrap()
            .is_error
        );
        assert_eq!(e.doc, protected);
        assert_eq!(e.history.len(), history);
        e.undo();
        call(
            &mut e,
            "rename_design_variable",
            json!({"name":"Accent","new_name":"Brand"}),
        );
        assert_eq!(
            e.doc.design.variable_bindings[&a][&variables::Property::TextColor],
            "Brand"
        );
        let linked = e.doc.clone();
        call(&mut e, "remove_design_variable", json!({"name":"Brand"}));
        assert!(e.doc.design.variable_bindings.is_empty());
        assert_eq!(e.doc.nodes, linked.nodes);
        e.undo();
        assert_eq!(e.doc, linked);
        call(
            &mut e,
            "set_design_variable",
            json!({"name":"Size","value":{"type":"number","value":36}}),
        );
        call(
            &mut e,
            "bind_design_variable",
            json!({"nodes":[a],"property":"font_size","name":"Size"}),
        );
        let NodeKind::Text { spec, .. } = &e.doc.node(a).unwrap().kind else {
            panic!()
        };
        assert_eq!(spec.size, 36.);
        assert!(spec.underline);
        call(
            &mut e,
            "bind_design_variable",
            json!({"nodes":[a],"property":"font_size","name":null}),
        );
        assert!(!e.doc.design.variable_bindings[&a].contains_key(&variables::Property::FontSize));
    }
    #[test]
    fn design_variable_mcp_strict_types_bounds_and_transactions_preserve_source() {
        let mut e = Editor::new(Document::new(400, 300), None);
        let id = text(&mut e);
        call(
            &mut e,
            "set_design_variable",
            json!({"name":"N","value":{"type":"number","value":20}}),
        );
        let before = e.doc.clone();
        let history = e.history.len();
        for (tool, args) in [
            (
                "set_design_variable",
                json!({"name":"N","value":{"type":"number","value":"20"}}),
            ),
            (
                "set_design_variable",
                json!({"name":"N","value":{"type":"color","value":[256,0,0,255]}}),
            ),
            (
                "set_design_variable",
                json!({"name":"N","value":{"type":"number","value":20,"extra":true}}),
            ),
            (
                "set_design_variable",
                json!({"name":" N ","value":{"type":"number","value":20}}),
            ),
            (
                "bind_design_variable",
                json!({"nodes":[id,id],"property":"font_size","name":"N"}),
            ),
            (
                "bind_design_variable",
                json!({"nodes":[id],"property":"opacity","name":"N"}),
            ),
            (
                "bind_design_variable",
                json!({"nodes":[id],"property":"text_color","name":"N"}),
            ),
            (
                "bind_design_variable",
                json!({"nodes":[id],"property":"font_size","name":"Missing"}),
            ),
            (
                "bind_design_variable",
                json!({"nodes":[id],"property":"font_size","name":true}),
            ),
            ("list_design_variables", json!({"unexpected":true})),
        ] {
            let result = execute(&mut e, tool, &args).unwrap();
            assert!(result.is_error, "{tool}: {args}");
            assert_eq!(e.doc, before);
            assert_eq!(e.history.len(), history);
        }
        e.begin("Unfinished edit");
        call(&mut e, "list_design_variables", json!({}));
        assert!(
            execute(
                &mut e,
                "set_design_variable",
                &json!({"name":"N","value":{"type":"number","value":21}})
            )
            .unwrap()
            .is_error
        );
        assert!(e.in_transaction());
        e.cancel();
        assert_eq!(e.doc, before);
    }
}
