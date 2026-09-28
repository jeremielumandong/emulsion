//! Live workspace lifecycle; tab IDs identify entities, never array positions.
use crate::server::ToolDef;
use emulsion_core::creation::{CanvasKind, CanvasSpec};
use serde::Deserialize;
use serde_json::{Value, json};
pub const NAMES: &[&str] = &[
    "get_workspace_tabs",
    "create_design_project",
    "select_workspace_tab",
    "close_workspace_tab",
];
pub const READ_ONLY: &[&str] = &["get_workspace_tabs"];
pub const DESTRUCTIVE: &[&str] = &[
    "create_design_project",
    "select_workspace_tab",
    "close_workspace_tab",
];
#[derive(Debug)]
pub enum Action {
    List,
    Create(CanvasSpec),
    Select(u64),
    Close(u64),
}
#[derive(Deserialize)]
#[serde(deny_unknown_fields)]
struct Tab {
    tab_id: u64,
}
#[derive(Deserialize)]
#[serde(deny_unknown_fields)]
struct Create {
    kind: String,
    name: String,
    width: u32,
    height: u32,
    #[serde(default = "one")]
    pages: usize,
    #[serde(default)]
    bleed_mm: f64,
}
fn one() -> usize {
    1
}
pub fn parse(name: &str, args: &Value) -> Result<Action, String> {
    match name {
        "get_workspace_tabs" if args.as_object().is_some_and(|a| a.is_empty()) => Ok(Action::List),
        "create_design_project" => {
            let a: Create = serde_json::from_value(args.clone()).map_err(|e| e.to_string())?;
            let kind = match a.kind.as_str() {
                "design" => CanvasKind::Design,
                "diagram" => CanvasKind::Diagram,
                _ => return Err("kind must be design or diagram".into()),
            };
            let spec = CanvasSpec {
                kind,
                name: a.name,
                width: a.width as f64,
                height: a.height as f64,
                pages: a.pages,
                bleed_mm: a.bleed_mm,
                ..Default::default()
            };
            spec.validate()?;
            Ok(Action::Create(spec))
        }
        "select_workspace_tab" | "close_workspace_tab" => {
            let a: Tab = serde_json::from_value(args.clone()).map_err(|e| e.to_string())?;
            if a.tab_id == 0 {
                return Err("Choose a nonzero tab_id returned by get_workspace_tabs".into());
            }
            Ok(if name == "select_workspace_tab" {
                Action::Select(a.tab_id)
            } else {
                Action::Close(a.tab_id)
            })
        }
        _ => Err("Unknown workspace tool or unsupported arguments".into()),
    }
}
pub fn definitions() -> Vec<ToolDef> {
    let def = |name: &str, description: &str, properties: Value, required: &[&str]| ToolDef {
        name: name.into(),
        description: description.into(),
        input_schema: json!({"type":"object","additionalProperties":false,"properties":properties,"required":required}),
    };
    let tab = json!({"tab_id":{"type":"integer","minimum":1}});
    vec![
        def(
            NAMES[0],
            "List stable workspace tab IDs, current tab, originating relay tab, project kinds and unsaved state. Selecting another tab never retargets the originating relay.",
            json!({}),
            &[],
        ),
        def(
            NAMES[1],
            "Create a native Design or Diagram project in a new tab. Existing unsaved tabs remain open. The originating MCP relay stays bound to its original document.",
            json!({"kind":{"type":"string","enum":["design","diagram"]},"name":{"type":"string","minLength":1,"maxLength":200},"width":{"type":"integer","minimum":1,"maximum":30000},"height":{"type":"integer","minimum":1,"maximum":30000},"pages":{"type":"integer","minimum":1,"maximum":100,"default":1},"bleed_mm":{"type":"number","minimum":0,"maximum":100}}),
            &["kind", "name", "width", "height"],
        ),
        def(
            NAMES[2],
            "Select a workspace tab by stable ID. Does not redirect this relay's future editing calls.",
            tab.clone(),
            &["tab_id"],
        ),
        def(
            NAMES[3],
            "Close a saved, idle tab. Unsaved changes, active edits and pending work are rejected; save in the owning editor first. Never discards artwork or accepts a force flag.",
            tab,
            &["tab_id"],
        ),
    ]
}
#[cfg(test)]
mod tests {
    use super::*;
    #[test]
    fn workspace_arguments_are_bounded_and_never_allow_discard() {
        assert!(parse("close_workspace_tab", &json!({"tab_id":1,"force":true})).is_err());
        assert!(parse("get_workspace_tabs", &json!({"tab_id":1})).is_err());
        for args in [
            json!({"kind":"photo","name":"x","width":10,"height":10}),
            json!({"kind":"design","name":"x","width":0,"height":10}),
            json!({"kind":"design","name":"x","width":10,"height":10,"pages":101}),
        ] {
            assert!(parse("create_design_project", &args).is_err());
        }
        assert!(matches!(
            parse(
                "create_design_project",
                &json!({"kind":"diagram","name":"Flow","width":800,"height":600})
            )
            .unwrap(),
            Action::Create(_)
        ));
    }
}
