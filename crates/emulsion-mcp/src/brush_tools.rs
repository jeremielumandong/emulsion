//! Document-independent brush operations. Hosts should execute these off the UI thread.
use crate::{ToolResult, brush_assets, brush_catalog};
use serde_json::Value;

pub fn is_tool(name: &str) -> bool {
    brush_catalog::NAMES.contains(&name) || brush_assets::NAMES.contains(&name)
}

pub fn is_read_only(name: &str) -> bool {
    brush_catalog::READ_ONLY.contains(&name) || brush_assets::READ_ONLY.contains(&name)
}

/// Catalog writes use optimistic catalog revisions, independent of document history.
pub fn execute(name: &str, args: &Value) -> ToolResult {
    if brush_catalog::NAMES.contains(&name) {
        brush_catalog::execute(name, args)
    } else if brush_assets::NAMES.contains(&name) {
        brush_assets::execute(name, args)
    } else {
        ToolResult::error(format!("Unknown brush tool: {name}"))
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use serde_json::json;

    const EXPECTED: &[&str] = &[
        "describe_brush_library",
        "manage_brush_library",
        "edit_brush",
        "brush_memories",
        "brush_source",
        "import_brushes",
        "export_brushes",
        "preview_brush",
    ];

    #[test]
    fn complete_brush_workflow_is_registered_once_and_permissions_match_effects() {
        let definitions = crate::tools::definitions();
        for name in EXPECTED {
            let matching: Vec<_> = definitions.iter().filter(|d| d.name == *name).collect();
            assert_eq!(
                matching.len(),
                1,
                "{name} must be discoverable exactly once"
            );
            assert!(
                is_tool(name),
                "{name} must reach the document-independent dispatcher"
            );
            assert_eq!(matching[0].input_schema["type"], "object");
            assert_eq!(matching[0].input_schema["additionalProperties"], false);
            let reads_only = ["describe_brush_library", "preview_brush"].contains(name);
            assert_eq!(is_read_only(name), reads_only, "{name} facade permissions");
            assert_eq!(
                crate::tools::READ_ONLY.contains(name),
                reads_only,
                "{name} host permissions"
            );
        }
        assert!(!is_tool("paint"));
        assert!(!is_read_only("edit_brush"));
        assert!(!is_read_only("unknown_brush_tool"));
    }

    #[test]
    fn authoring_discovery_exposes_revision_and_complete_edit_contracts() {
        let definitions = crate::tools::definitions();
        for name in ["manage_brush_library", "edit_brush", "brush_source"] {
            let schema = &definitions
                .iter()
                .find(|d| d.name == name)
                .unwrap()
                .input_schema;
            assert!(
                schema["required"]
                    .as_array()
                    .unwrap()
                    .contains(&json!("expected_revision")),
                "{name} must require stale-write protection"
            );
        }
        let edit = &definitions
            .iter()
            .find(|d| d.name == "edit_brush")
            .unwrap()
            .input_schema["properties"];
        for field in [
            "brush_id",
            "patch",
            "component",
            "combine_mode",
            "name",
            "note",
            "author",
            "action",
        ] {
            assert!(
                edit.get(field).is_some(),
                "persistent Studio edit {field} is discoverable"
            );
        }
        let memories = &definitions
            .iter()
            .find(|d| d.name == "brush_memories")
            .unwrap()
            .input_schema["properties"];
        for action in [
            "inspect",
            "save",
            "save_mark",
            "recall_mark",
            "clear_mark",
            "clear",
            "transfer",
        ] {
            assert!(
                memories["action"]["enum"]
                    .as_array()
                    .unwrap()
                    .contains(&json!(action)),
                "memory action {action}"
            );
        }
        let result = execute("unknown_brush_tool", &json!({}));
        assert!(result.is_error);
        assert!(
            result.content[0]["text"]
                .as_str()
                .unwrap()
                .contains("Unknown brush tool")
        );
    }
}
