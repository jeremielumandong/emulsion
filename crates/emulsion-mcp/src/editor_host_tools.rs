//! Native editor workflow schemas; execution remains on the originating host.
use crate::ToolDef;
use serde::Deserialize;
use serde_json::{Value, json};
pub const READ_ONLY: &[&str] = &[
    "get_editor_state",
    "get_editor_controls",
    "get_playback_setup",
];
pub const DESTRUCTIVE: &[&str] = &[
    "set_editor_state",
    "set_document_guides",
    "editor_clipboard",
    "restore_smart_source",
    "open_print_dialog",
    "set_editor_layout",
    "manage_editor_workspace",
    "canvas_gesture",
    "open_playback_setup",
    "install_playback_runtime",
];
pub fn is_tool(name: &str) -> bool {
    READ_ONLY.contains(&name) || DESTRUCTIVE.contains(&name)
}
#[derive(Clone, Debug, Default, Deserialize)]
#[serde(default, deny_unknown_fields)]
pub struct StatePatch {
    pub rulers: Option<bool>,
    pub snapping: Option<bool>,
    pub channel: Option<String>,
    pub quick_mask: Option<bool>,
    pub tool: Option<String>,
}
#[derive(Debug)]
pub enum Action {
    Inspect,
    State(StatePatch),
    Guides(Vec<emulsion_core::document::Guide>),
    Clipboard(String),
    Restore(u64),
    Print,
    Controls,
    Layout(Value),
    Workspace {
        operation: String,
        name: Option<String>,
    },
    Gesture(crate::editor_layout_tools::Gesture),
    PlaybackSetup,
    PlaybackInspect,
    PlaybackInstall,
}
pub fn definitions() -> Vec<ToolDef> {
    [
 ("get_editor_state","Inspect originating editor tool, channel, quick mask, rulers, snapping, guides, selection and workspace layout. Does not read clipboard data.",json!({}),vec![]),
 ("set_editor_state","Patch native tool/view controls without changing unmentioned settings. Quick Mask follows the native selection workflow. Active gestures must finish first.",json!({"rulers":{"type":"boolean"},"snapping":{"type":"boolean"},"channel":{"enum":["rgb","red","green","blue"]},"quick_mask":{"type":"boolean"},"tool":{"enum":["hand","move","select","mask","brush","heal","clone","grade","type","crop","shape","pen","eyedropper","zoom"]}}),vec![]),
 ("set_document_guides","Replace saved document guides in one Undo; an empty array removes all guides. Positions are finite document pixels.",json!({"guides":{"type":"array","maxItems":1000,"items":{"type":"object","properties":{"vertical":{"type":"boolean"},"pos":{"type":"number","minimum":-100000,"maximum":100000}},"required":["vertical","pos"],"additionalProperties":false}}}),vec!["guides"]),
 ("editor_clipboard","Copy/cut selected native objects or pixels, or paste the system image clipboard using the existing native workflow. Returns after the native clipboard operation completes.",json!({"action":{"enum":["copy","cut","paste"]}}),vec!["action"]),
 ("restore_smart_source","Convert a Smart Object back to its editable original text/path/raster source, removing the smart filter stack. Preserves placement using native conversion; one Undo restores the Smart Object.",json!({"node":{"type":"integer","minimum":1}}),vec!["node"]),
 ("open_print_dialog","Open the native shared print preview/setup dialog for the originating editor's document or project. Printing occurs through its normal Print button.",json!({}),vec![]),
 ].into_iter().map(|(name,description,properties,required)|ToolDef{name:name.into(),description:description.into(),input_schema:json!({"type":"object","properties":properties,"required":required,"additionalProperties":false})}).chain(crate::editor_layout_tools::definitions()).collect()
}
pub fn parse(name: &str, args: &Value) -> Result<Action, String> {
    if let Some(def) = crate::editor_layout_tools::definitions()
        .into_iter()
        .find(|d| d.name == name)
    {
        crate::editor_layout_tools::validate(&def.input_schema, args)?;
        return Ok(match name {
            "get_editor_controls" => Action::Controls,
            "set_editor_layout" => Action::Layout(args["layout"].clone()),
            "manage_editor_workspace" => {
                let operation = args["operation"].as_str().unwrap().to_owned();
                let name = args["name"].as_str().map(|s| s.trim().to_owned());
                if matches!(operation.as_str(), "save" | "apply" | "remove")
                    && name.as_ref().is_none_or(|s| s.is_empty())
                {
                    return Err("A preset name is required.".into());
                }
                if !matches!(operation.as_str(), "save" | "apply" | "remove") && name.is_some() {
                    return Err("This operation does not accept a name.".into());
                }
                Action::Workspace { operation, name }
            }
            "canvas_gesture" => {
                Action::Gesture(serde_json::from_value(args.clone()).map_err(|e| e.to_string())?)
            }
            "get_playback_setup" => Action::PlaybackInspect,
            "open_playback_setup" => Action::PlaybackSetup,
            _ => Action::PlaybackInstall,
        });
    }
    let object = args.as_object().ok_or("Expected an argument object.")?;
    let allowed: &[&str] = match name {
        "get_editor_state" | "open_print_dialog" => &[],
        "set_editor_state" => &["rulers", "snapping", "channel", "quick_mask", "tool"],
        "set_document_guides" => &["guides"],
        "editor_clipboard" => &["action"],
        "restore_smart_source" => &["node"],
        _ => return Err("Unknown editor host tool.".into()),
    };
    if object.keys().any(|k| !allowed.contains(&k.as_str())) {
        return Err("Unknown editor argument.".into());
    }
    Ok(match name {
        "get_editor_state" => Action::Inspect,
        "open_print_dialog" => Action::Print,
        "set_editor_state" => {
            if object.values().any(Value::is_null) {
                return Err("State fields cannot be null; omit unchanged fields.".into());
            }
            let patch: StatePatch =
                serde_json::from_value(args.clone()).map_err(|e| e.to_string())?;
            if patch
                .channel
                .as_deref()
                .is_some_and(|v| !["rgb", "red", "green", "blue"].contains(&v))
                || patch.tool.as_deref().is_some_and(|v| {
                    ![
                        "hand",
                        "move",
                        "select",
                        "mask",
                        "brush",
                        "heal",
                        "clone",
                        "grade",
                        "type",
                        "crop",
                        "shape",
                        "pen",
                        "eyedropper",
                        "zoom",
                    ]
                    .contains(&v)
                })
            {
                return Err("Unknown channel or native tool.".into());
            }
            if patch.quick_mask == Some(true) && patch.tool.as_deref().is_some_and(|v| v != "brush")
            {
                return Err(
                    "Quick Mask starts with the brush tool; omit tool or choose brush.".into(),
                );
            }
            Action::State(patch)
        }
        "set_document_guides" => {
            #[derive(Deserialize)]
            #[serde(deny_unknown_fields)]
            struct Guide {
                vertical: bool,
                pos: f64,
            }
            let guides: Vec<Guide> =
                serde_json::from_value(args.get("guides").ok_or("Missing guides.")?.clone())
                    .map_err(|e| e.to_string())?;
            if guides.len() > 1000
                || guides
                    .iter()
                    .any(|g| !g.pos.is_finite() || g.pos.abs() > 100000.)
            {
                return Err(
                    "Use at most 1000 finite guide positions within ±100000 pixels.".into(),
                );
            }
            Action::Guides(
                guides
                    .into_iter()
                    .map(|g| emulsion_core::document::Guide {
                        vertical: g.vertical,
                        pos: g.pos,
                    })
                    .collect(),
            )
        }
        "editor_clipboard" => {
            let action = args
                .get("action")
                .and_then(Value::as_str)
                .filter(|a| ["copy", "cut", "paste"].contains(a))
                .ok_or("Choose copy, cut or paste.")?;
            Action::Clipboard(action.into())
        }
        _ => Action::Restore(
            args.get("node")
                .and_then(Value::as_u64)
                .filter(|id| *id > 0)
                .ok_or("node must be positive.")?,
        ),
    })
}
#[cfg(test)]
mod tests {
    use super::*;
    #[test]
    fn host_state_rejects_unknown_null_and_conflicting_controls() {
        assert!(parse("set_editor_state", &json!({"tool":"shell"})).is_err());
        assert!(parse("set_editor_state", &json!({"rulers":null})).is_err());
        assert!(
            parse(
                "set_editor_state",
                &json!({"quick_mask":true,"tool":"move"})
            )
            .is_err()
        );
        assert!(
            parse(
                "set_document_guides",
                &json!({"guides":[{"vertical":true,"pos":10,"extra":true}]})
            )
            .is_err()
        );
        assert!(parse("editor_clipboard", &json!({"action":"copy"})).is_ok());
    }
}
