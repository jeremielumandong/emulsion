//! Immutable selection export; the live host runs this through its heavy-tool worker.
use crate::{ToolDef, ToolResult};
use emulsion_core::Document;
use emulsion_io::selection_export::{self, Format, Options};
use serde_json::{Value, json};
pub(crate) const NAME: &str = "export_design_selection";
pub(crate) fn definitions() -> Vec<ToolDef> {
    vec![ToolDef{name:NAME.into(),description:"Export selected native objects or a responsive frame as one SVG/PDF/PNG file. Keeps source and history unchanged. Includes ancestor masks/clips, excludes unrelated siblings; incomplete clipping stacks are rejected. Returns crop origin/size and rasterization diagnostics. Existing files require overwrite:true.".into(),input_schema:json!({"type":"object","properties":{
    "nodes":{"type":"array","items":{"type":"integer","minimum":1},"minItems":1,"maxItems":emulsion_core::document::MAX_NODES,"uniqueItems":true},
    "path":{"type":"string","minLength":1},"format":{"enum":["svg","pdf","png"]},
    "bounds":{"enum":["content","frame","canvas"],"default":"content"},"padding":{"type":"integer","minimum":0,"maximum":1000,"default":0},
    "transparent":{"type":"boolean","default":true},"strict_vectors":{"type":"boolean","default":false},"overwrite":{"type":"boolean","default":false}
},"required":["nodes","path","format"],"additionalProperties":false})}]
}
pub(crate) fn execute(doc: &Document, name: &str, args: &Value) -> Option<ToolResult> {
    if name != NAME {
        return None;
    }
    Some(match run(doc, args) {
        Ok(report) => ToolResult::text(report.to_string()),
        Err(e) => ToolResult::error(e),
    })
}
fn run(doc: &Document, args: &Value) -> Result<Value, String> {
    let object = args.as_object().ok_or("Expected an argument object.")?;
    if object.keys().any(|k| {
        ![
            "nodes",
            "path",
            "format",
            "bounds",
            "padding",
            "transparent",
            "strict_vectors",
            "overwrite",
        ]
        .contains(&k.as_str())
    }) {
        return Err("Unknown selection export argument.".into());
    }
    let nodes: Vec<u64> =
        serde_json::from_value(args.get("nodes").ok_or("Missing nodes.")?.clone())
            .map_err(|e| e.to_string())?;
    let format: Format =
        serde_json::from_value(args.get("format").ok_or("Missing format.")?.clone())
            .map_err(|e| e.to_string())?;
    let path = args
        .get("path")
        .and_then(Value::as_str)
        .filter(|s| !s.trim().is_empty())
        .ok_or("path must be a nonempty string.")?;
    let options: serde_json::Map<_, _> = object
        .iter()
        .filter(|(k, _)| !["nodes", "path", "format"].contains(&k.as_str()))
        .map(|(k, v)| (k.clone(), v.clone()))
        .collect();
    let options: Options =
        serde_json::from_value(Value::Object(options)).map_err(|e| e.to_string())?;
    let report = selection_export::write(doc, &nodes, format, &options, std::path::Path::new(path))
        .map_err(|e| e.to_string())?;
    Ok(json!({"path":path,"format":format,"report":report}))
}

#[cfg(test)]
mod tests {
    use super::*;
    use emulsion_core::{Command, Editor, Node, command::Slot, text::TextSpec};
    #[test]
    fn selection_export_mcp_writes_atomic_artwork_without_history_and_validates_json() {
        let mut e = Editor::new(Document::new(300, 200), None);
        let id = e
            .execute(Command::AddNode {
                node: Box::new(Node::text(
                    0,
                    "Title",
                    TextSpec {
                        text: "Native export".into(),
                        x: 20.,
                        y: 20.,
                        size: 24.,
                        underline: true,
                        ..Default::default()
                    },
                    300,
                    200,
                )),
                slot: Slot::TOP,
            })
            .unwrap()
            .unwrap();
        let file = std::env::temp_dir().join(format!(
            "emulsion-selection-mcp-{}-{}.svg",
            std::process::id(),
            std::time::SystemTime::now()
                .duration_since(std::time::UNIX_EPOCH)
                .unwrap()
                .as_nanos()
        ));
        let before = e.doc.clone();
        let history = e.history.len();
        let result = crate::exec::execute(
            &mut e,
            NAME,
            &json!({"nodes":[id],"path":file,"format":"svg","padding":4}),
        );
        assert!(!result.is_error, "{result:?}");
        let data: Value =
            serde_json::from_str(result.content[0]["text"].as_str().unwrap()).unwrap();
        assert_eq!(data["report"]["rasterized"], false);
        let saved = std::fs::read(&file).unwrap();
        assert!(String::from_utf8_lossy(&saved).contains("<path"));
        assert_eq!(e.doc, before);
        assert_eq!(e.history.len(), history);
        for patch in [
            json!({"overwrite":false}),
            json!({"overwrite":true,"padding":-1}),
            json!({"overwrite":true,"transparent":"yes"}),
            json!({"overwrite":true,"bounds":"invalid"}),
            json!({"overwrite":true,"nodes":[id,id]}),
            json!({"overwrite":true,"surprise":1}),
        ] {
            let mut args = json!({"nodes":[id],"path":file,"format":"svg"});
            for (k, v) in patch.as_object().unwrap() {
                args[k] = v.clone();
            }
            assert!(crate::exec::execute(&mut e, NAME, &args).is_error, "{args}");
            assert_eq!(std::fs::read(&file).unwrap(), saved);
            assert_eq!(e.doc, before);
            assert_eq!(e.history.len(), history);
        }
        std::fs::remove_file(file).unwrap();
        assert!(crate::tools::HEAVY.contains(&NAME));
        assert!(!crate::tools::is_read_only(NAME));
        assert!(crate::tools::is_destructive(NAME));
    }
}
