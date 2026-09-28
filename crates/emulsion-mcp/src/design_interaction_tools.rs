//! Native interactive presentation authoring; live execution uses the UI host.
use crate::{ToolDef, ToolResult};
use emulsion_core::{
    Editor,
    design_interactions::{self, Action},
};
use serde_json::{Value, json};
pub(crate) const READ_ONLY: &[&str] = &["get_presentation_actions"];
pub(crate) const DESTRUCTIVE: &[&str] = &["set_presentation_actions", "set_presentation_overlay"];
fn definition(name: &str, description: &str, properties: Value, required: &[&str]) -> ToolDef {
    ToolDef {
        name: name.into(),
        description: description.into(),
        input_schema: json!({"type":"object","properties":properties,"required":required,"additionalProperties":false}),
    }
}
fn action_schema() -> Value {
    let mut options = vec![];
    for kind in ["next", "previous", "back", "close_overlay"] {
        options.push(json!({"type":"object","properties":{"type":{"const":kind}},"required":["type"],"additionalProperties":false}));
    }
    options.push(json!({"type":"object","properties":{"type":{"const":"slide"},"page":{"type":"integer","minimum":1}},"required":["type","page"],"additionalProperties":false}));
    options.push(json!({"type":"object","properties":{"type":{"const":"overlay"},"target":{"type":"integer","minimum":1},"operation":{"enum":["show","hide","toggle"]}},"required":["type","target","operation"],"additionalProperties":false}));
    options.push(json!({"type":"object","properties":{"type":{"const":"variant"},"target":{"type":"integer","minimum":1},"variant":{"type":"string","minLength":1,"maxLength":80}},"required":["type","target","variant"],"additionalProperties":false}));
    json!({"oneOf":options})
}
fn parse_actions(value: Value) -> Result<Vec<Action>, String> {
    let list = value.as_array().ok_or("actions must be an array")?;
    for value in list {
        let object = value.as_object().ok_or("Each action must be an object")?;
        let allowed: &[&str] = match value["type"].as_str() {
            Some("slide") => &["type", "page"],
            Some("overlay") => &["type", "target", "operation"],
            Some("variant") => &["type", "target", "variant"],
            _ => &["type"],
        };
        if object.keys().any(|key| !allowed.contains(&key.as_str())) {
            return Err("Unknown action property".into());
        }
    }
    serde_json::from_value(value).map_err(|e| format!("Invalid actions: {e}"))
}
pub(crate) fn definitions() -> Vec<ToolDef> {
    vec![
        definition(
            "get_presentation_actions",
            "Read active-page click actions and overlay groups. These run in presentation only; authored artwork is unchanged by playback.",
            json!({}),
            &[],
        ),
        definition(
            "set_presentation_actions",
            "Replace one object's click actions atomically with native Undo. [] removes actions. Supports next/previous/back/specific slide, show/hide/toggle overlay, close top overlay and component variant. A navigation action must be last. Mark overlay targets first with set_presentation_overlay. Slide page IDs resolve within the project at runtime.",
            json!({"node":{"type":"integer","minimum":1},"actions":{"type":"array","maxItems":8,"items":action_schema()},"trigger":{"enum":["click","hover","drag_end"]}}),
            &["node", "actions"],
        ),
        definition(
            "set_presentation_overlay",
            "Mark a top-level group as a modal presentation overlay, hidden until opened. Disabling also removes incoming overlay actions on this page. Preserves authored visibility and other masks. One Undo; respects authoring locks.",
            json!({"node":{"type":"integer","minimum":1},"enabled":{"type":"boolean"}}),
            &["node", "enabled"],
        ),
    ]
}
pub(crate) fn execute(editor: &mut Editor, name: &str, args: &Value) -> Option<ToolResult> {
    let definition = definitions().into_iter().find(|d| d.name == name)?;
    let result = (|| -> Result<Value, String> {
        let values = args.as_object().ok_or("Arguments must be an object")?;
        if values
            .keys()
            .any(|key| definition.input_schema["properties"].get(key).is_none())
        {
            return Err("Unknown argument".into());
        }
        if name == "get_presentation_actions" {
            return Ok(
                json!({"actions":editor.doc.design.interactions,"triggers":editor.doc.design.interaction_triggers,"overlays":editor.doc.design.overlays}),
            );
        }
        let node = args["node"]
            .as_u64()
            .filter(|n| *n > 0)
            .ok_or("node must be a positive integer")?;
        let (actions, overlay) = if name == "set_presentation_actions" {
            (
                parse_actions(args.get("actions").cloned().ok_or("Missing actions")?)?,
                None,
            )
        } else {
            (
                editor
                    .doc
                    .design
                    .interactions
                    .get(&node)
                    .cloned()
                    .unwrap_or_default(),
                Some(args["enabled"].as_bool().ok_or("enabled must be boolean")?),
            )
        };
        let trigger = args
            .get("trigger")
            .map(|v| serde_json::from_value(v.clone()).map_err(|e| format!("Invalid trigger: {e}")))
            .transpose()?;
        design_interactions::author_with_trigger(editor, node, actions, overlay, trigger)?;
        Ok(
            json!({"node":node,"actions":editor.doc.design.interactions.get(&node),"overlay":editor.doc.design.overlays.contains(&node)}),
        )
    })();
    Some(match result {
        Ok(value) => ToolResult::text(value.to_string()),
        Err(error) => ToolResult::error(error),
    })
}

#[cfg(test)]
mod tests {
    use super::*;
    use emulsion_core::{Command, Document, Node, command::Slot};
    #[test]
    fn interaction_tools_validate_author_inspect_clear_and_undo() {
        let mut editor = Editor::new(Document::new(100, 100), None);
        let node = editor
            .execute(Command::AddNode {
                node: Box::new(Node::text(0, "Action", Default::default(), 100, 100)),
                slot: Slot::TOP,
            })
            .unwrap()
            .unwrap();
        let before = editor.doc.clone();
        let result = crate::exec::execute(
            &mut editor,
            "set_presentation_actions",
            &json!({"node":node,"actions":[{"type":"slide","page":2}]}),
        );
        assert!(!result.is_error, "{:?}", result.content);
        let installed = editor.doc.clone();
        let described = crate::exec::execute(&mut editor, "get_presentation_actions", &json!({}));
        assert!(!described.is_error);
        for args in [
            json!({"node":node,"actions":[{"type":"slide","page":0}]}),
            json!({"node":node,"actions":[{"type":"next","extra":1}]}),
            json!({"node":node,"actions":[{"type":"next"},{"type":"back"}]}),
            json!({"node":node,"actions":[],"typo":true}),
        ] {
            assert!(crate::exec::execute(&mut editor, "set_presentation_actions", &args).is_error);
            assert_eq!(editor.doc, installed);
        }
        assert!(
            !crate::exec::execute(
                &mut editor,
                "set_presentation_actions",
                &json!({"node":node,"actions":[]})
            )
            .is_error
        );
        assert!(editor.undo());
        assert_eq!(editor.doc, installed);
        assert!(editor.undo());
        assert_eq!(editor.doc, before);
        assert!(
            crate::exec::execute(
                &mut editor,
                "trigger_presentation_object",
                &json!({"node":node})
            )
            .is_error
        );
    }
}
