//! Saved CSV bindings share native validation and history.
use crate::{ToolDef, ToolResult};
use emulsion_core::{Editor, design_data};
use serde_json::{Value, json};
pub(crate) const READ_ONLY: &[&str] = &["list_design_data_bindings"];
pub(crate) const DESTRUCTIVE: &[&str] = &["set_design_data_binding"];
pub(crate) fn definitions() -> Vec<ToolDef> {
    let column = json!({"type":"string","minLength":1,"maxLength":200});
    let binding = json!({"oneOf":[{"type":"null"},{"type":"object","properties":{"kind":{"const":"text"},"column":column},"required":["kind","column"],"additionalProperties":false},{"type":"object","properties":{"kind":{"const":"image"},"column":column,"fit":{"enum":["cover","contain","stretch"]},"focus":{"type":"array","items":{"type":"number","minimum":0,"maximum":1},"minItems":2,"maxItems":2}},"required":["kind","column"],"additionalProperties":false}]});
    [
        ("list_design_data_bindings", "Inspect saved CSV text/image column mappings on the active page.", json!({}), vec![]),
        ("set_design_data_binding", "Bind a text or raster image to a CSV column. Frame targets resolve to their image; null removes a binding. Image values are local paths resolved during generation. Undoable.", json!({"node":{"type":"integer","minimum":1},"binding":binding}), vec!["node","binding"]),
    ].into_iter().map(|(name,description,properties,required)|ToolDef {name:name.into(),description:description.into(),input_schema:json!({"type":"object","properties":properties,"required":required,"additionalProperties":false})}).collect()
}
pub(crate) fn execute(editor: &mut Editor, name: &str, args: &Value) -> Option<ToolResult> {
    if !READ_ONLY.contains(&name) && !DESTRUCTIVE.contains(&name) {
        return None;
    }
    Some(match run(editor, name, args) {
        Ok(value) => ToolResult::text(value.to_string()),
        Err(error) => ToolResult::error(error),
    })
}
fn run(editor: &mut Editor, name: &str, args: &Value) -> Result<Value, String> {
    let object = args.as_object().ok_or("Expected an argument object.")?;
    let allowed: &[&str] = if name == READ_ONLY[0] {
        &[]
    } else {
        &["node", "binding"]
    };
    if object.keys().any(|key| !allowed.contains(&key.as_str())) {
        return Err("Unknown data binding argument.".into());
    }
    let mut target = None;
    if name == DESTRUCTIVE[0] {
        let id = args["node"]
            .as_u64()
            .filter(|id| *id > 0)
            .ok_or("node must be a positive object ID.")?;
        let value = object
            .get("binding")
            .ok_or("Missing binding; use null to remove it.")?;
        let binding = serde_json::from_value(value.clone()).map_err(|e| e.to_string())?;
        target = Some(design_data::set(editor, id, binding)?);
    }
    Ok(json!({"node":target,"bindings":editor.doc.design.data_bindings}))
}

#[cfg(test)]
mod tests {
    use super::*;
    use emulsion_core::{
        Command, Document, Node,
        command::Slot,
        project::{ProjectEditor, ProjectKind},
        text::TextSpec,
    };
    #[test]
    fn design_data_mcp_strict_bindings_and_atomic_record_sets() {
        let mut project =
            ProjectEditor::new_project(ProjectKind::Design, Document::new(300, 200)).unwrap();
        let id = project
            .execute(Command::AddNode {
                node: Box::new(Node::text(
                    0,
                    "Name",
                    TextSpec {
                        text: "Default".into(),
                        ..Default::default()
                    },
                    300,
                    200,
                )),
                slot: Slot::TOP,
            })
            .unwrap()
            .unwrap();
        let result = execute(
            &mut project,
            "set_design_data_binding",
            &json!({"node":id,"binding":{"kind":"text","column":"name"}}),
        )
        .unwrap();
        assert!(!result.is_error, "{:?}", result.content);
        let original = project.doc.clone();
        let history = project.history.len();
        for args in [
            json!({"node":id}),
            json!({"node":id,"binding":{"kind":"text","column":"name","extra":true}}),
            json!({"node":id,"binding":{"kind":"image","column":"photo"}}),
            json!({"node":id,"binding":null,"extra":1}),
        ] {
            assert!(
                execute(&mut project, "set_design_data_binding", &args)
                    .unwrap()
                    .is_error
            );
            assert_eq!(project.doc, original);
            assert_eq!(project.history.len(), history);
        }
        assert!(crate::tools::is_read_only("list_design_data_bindings"));
        assert!(crate::tools::uses_native_history("set_design_data_binding"));
        let result = crate::project_tools::execute(
            &mut project,
            "generate_design_pages",
            &json!({"csv":"name\nAlice\nBob","template_pages":[1]}),
        );
        assert!(!result.is_error, "{:?}", result.content);
        assert_eq!(project.page_list().len(), 3);
        assert!(project.undo());
        assert_eq!(project.page_list().len(), 1);
        assert_eq!(project.doc, original);
        for args in [
            json!({"csv":"name\nAlice","template_pages":[1,1]}),
            json!({"csv":"name\nAlice","template_pages":[999]}),
            json!({"csv":"name\nAlice","base_directory":true}),
            json!({"csv":"name\nAlice","unknown":true}),
        ] {
            assert!(
                crate::project_tools::execute(&mut project, "generate_design_pages", &args)
                    .is_error
            );
            assert_eq!(project.page_list().len(), 1);
            assert_eq!(project.doc, original);
        }
    }
}
