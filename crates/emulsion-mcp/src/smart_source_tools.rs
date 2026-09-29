//! Native live Smart Object source sessions and explicit linked-file operations.
use crate::ToolDef;
use serde::Deserialize;
use serde_json::{Value, json};
use std::path::PathBuf;
pub const READ_ONLY: &[&str] = &["inspect_smart_source"];
pub const DESTRUCTIVE: &[&str] = &[
    "open_smart_source",
    "apply_smart_source",
    "link_smart_source",
    "refresh_smart_source",
    "set_smart_source_auto_refresh",
    "unlink_smart_source",
    "save_smart_source_as",
    "write_linked_smart_source",
];
pub fn is_tool(name: &str) -> bool {
    READ_ONLY.contains(&name) || DESTRUCTIVE.contains(&name)
}
#[derive(Clone, Debug)]
pub enum Action {
    Inspect {
        node: u64,
    },
    Open {
        node: u64,
    },
    Apply,
    Link {
        node: u64,
        path: PathBuf,
        auto_refresh: bool,
    },
    Refresh {
        node: u64,
        discard_local: bool,
    },
    Auto {
        node: u64,
        enabled: bool,
    },
    Unlink {
        node: u64,
    },
    SaveAs {
        node: u64,
        path: PathBuf,
    },
    Write {
        node: u64,
    },
}
impl Action {
    pub fn node(&self) -> Option<u64> {
        match self {
            Self::Apply => None,
            Self::Inspect { node }
            | Self::Open { node }
            | Self::Link { node, .. }
            | Self::Refresh { node, .. }
            | Self::Auto { node, .. }
            | Self::Unlink { node }
            | Self::SaveAs { node, .. }
            | Self::Write { node } => Some(*node),
        }
    }
}
pub fn parse(name: &str, args: &Value) -> Result<Action, String> {
    #[derive(Deserialize)]
    #[serde(deny_unknown_fields)]
    struct Args {
        node: Option<u64>,
        path: Option<PathBuf>,
        auto_refresh: Option<bool>,
        discard_local: Option<bool>,
        enabled: Option<bool>,
    }
    let object = args.as_object().ok_or("Expected an argument object.")?;
    let allowed: &[&str] = match name {
        "apply_smart_source" => &[],
        "link_smart_source" => &["node", "path", "auto_refresh"],
        "refresh_smart_source" => &["node", "discard_local"],
        "set_smart_source_auto_refresh" => &["node", "enabled"],
        "save_smart_source_as" => &["node", "path"],
        _ => &["node"],
    };
    if object
        .iter()
        .any(|(key, value)| !allowed.contains(&key.as_str()) || value.is_null())
    {
        return Err("Unknown or null Smart source argument.".into());
    }
    let a: Args = serde_json::from_value(args.clone()).map_err(|e| e.to_string())?;
    if name == "apply_smart_source" {
        return Ok(Action::Apply);
    }
    let node = a
        .node
        .filter(|v| *v > 0)
        .ok_or("node must be a positive Smart Object ID.")?;
    let path = || {
        a.path
            .clone()
            .filter(|p| !p.as_os_str().is_empty() && p.as_os_str().len() <= 4096)
            .ok_or_else(|| "Choose a local source path.".to_owned())
    };
    Ok(match name {
        "inspect_smart_source" => Action::Inspect { node },
        "open_smart_source" => Action::Open { node },
        "link_smart_source" => Action::Link {
            node,
            path: path()?,
            auto_refresh: a.auto_refresh.unwrap_or(false),
        },
        "refresh_smart_source" => Action::Refresh {
            node,
            discard_local: a.discard_local.unwrap_or(false),
        },
        "set_smart_source_auto_refresh" => Action::Auto {
            node,
            enabled: a.enabled.ok_or("enabled is required.")?,
        },
        "unlink_smart_source" => Action::Unlink { node },
        "save_smart_source_as" => Action::SaveAs {
            node,
            path: path()?,
        },
        "write_linked_smart_source" => Action::Write { node },
        _ => return Err("Unknown Smart source tool.".into()),
    })
}
pub fn definitions() -> Vec<ToolDef> {
    let node = json!({"type":"integer","minimum":1});
    let path = json!({"type":"string","minLength":1,"maxLength":4096});
    [("inspect_smart_source","Inspect embedded source, external link and current nested editor session without file IO.",json!({"node":node}),vec!["node"]),
("open_smart_source","Open a live nested native source-editor tab for a Smart Object. Text/path/layers remain editable; save/apply updates the parent, never an external original. Up to eight nested sessions.",json!({"node":node}),vec!["node"]),
("apply_smart_source","Apply the originating source-editor tab to its parent Smart Object with one Undo. Rejects removed/changed/locked parents or stale sessions. Does not write external files.",json!({}),vec![]),
("link_smart_source","Explicitly link/relink a local native .ora/single-page .emu/SVG/image file. Replaces embedded source atomically, retaining parent geometry/filters. Optional auto_refresh polls while tab is open; external writes never occur automatically.",json!({"node":node,"path":path,"auto_refresh":{"type":"boolean"}}),vec!["node","path"]),
("refresh_smart_source","Read a linked source on a background worker; missing files retain embedded fallback. Conflicting local edits reject unless discard_local is explicitly true. One Undo restores the prior source.",json!({"node":node,"discard_local":{"type":"boolean"}}),vec!["node"]),
("set_smart_source_auto_refresh","Enable or disable bounded background polling of this linked source. Local edits block conflicting external refresh.",json!({"node":node,"enabled":{"type":"boolean"}}),vec!["node","enabled"]),
("unlink_smart_source","Keep the embedded editable source and remove its external link in one Undo.",json!({"node":node}),vec!["node"]),
("save_smart_source_as","Explicitly create a new layered .ora file and link it. Refuses existing paths; never overwrites an original.",json!({"node":node,"path":path}),vec!["node","path"]),
("write_linked_smart_source","Explicitly overwrite the linked .ora file with the embedded layered source, only when its stored SHA-256 still matches. Non-native image originals are protected: use Save Source As. External disk writes are not undone by document Undo.",json!({"node":node}),vec!["node"])]
.into_iter().map(|(name,description,properties,required)|ToolDef{name:name.into(),description:description.into(),input_schema:json!({"type":"object","properties":properties,"required":required,"additionalProperties":false})}).collect()
}
/// Worker-side operations; live host enforces original tab/page/revision stamps.
pub fn execute(editor: &mut emulsion_core::Editor, action: &Action) -> Result<(), String> {
    use emulsion_io::smart_source as io;
    match action {
        Action::Link {
            node,
            path,
            auto_refresh,
        } => io::relink(editor, *node, path, *auto_refresh).map_err(|e| e.to_string()),
        Action::Refresh {
            node,
            discard_local,
        } => io::refresh(editor, *node, *discard_local)
            .map(|_| ())
            .map_err(|e| e.to_string()),
        Action::SaveAs { node, path } => {
            io::save_as(editor, *node, path).map_err(|e| e.to_string())
        }
        Action::Write { node } => io::write_linked(editor, *node).map_err(|e| e.to_string()),
        Action::Unlink { node } => emulsion_core::smart_source::set_link(editor, *node, None),
        Action::Auto { node, enabled } => {
            let mut link = emulsion_core::smart_source::link(&editor.doc, *node)
                .cloned()
                .ok_or("Link a file first.")?;
            link.auto_refresh = *enabled;
            emulsion_core::smart_source::set_link(editor, *node, Some(link))
        }
        _ => Err("This operation requires its live native editor session.".into()),
    }
}
#[cfg(test)]
mod tests {
    use super::*;
    #[test]
    fn smart_source_schema_rejects_unknown_and_null() {
        for v in [
            json!({"node":1,"path":"x","discard_local":true}),
            json!({"node":1,"path":null}),
            json!({"node":0,"path":"x"}),
        ] {
            assert!(parse("link_smart_source", &v).is_err());
        }
        assert!(parse("apply_smart_source", &json!({"node":1})).is_err());
        assert!(
            parse(
                "refresh_smart_source",
                &json!({"node":1,"discard_local":true})
            )
            .is_ok()
        );
    }
}
