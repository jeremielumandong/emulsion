//! Native per-paragraph list, indentation and spacing controls.
use crate::{ToolDef, ToolResult};
use emulsion_core::{
    Command, Editor, NodeKind,
    text::{self, ParagraphFormat},
};
use serde_json::{Value, json};
pub(crate) const READ_ONLY: &[&str] = &["inspect_text_paragraphs"];
pub(crate) const NAMES: &[&str] = &["inspect_text_paragraphs", "format_text_paragraphs"];
pub(crate) fn definitions() -> Vec<ToolDef> {
    let node = json!({"type":"integer","minimum":1});
    let index = json!({"type":"integer","minimum":0,"maximum":999});
    let spacing = json!({"type":"number","minimum":0,"maximum":10000});
    vec![ToolDef{name:NAMES[0].into(),description:"Read editable paragraph list metadata and zero-based paragraph indices. Source list markers remain normal text.".into(),input_schema:json!({"type":"object","additionalProperties":false,"properties":{"node":node},"required":["node"]})},ToolDef{name:NAMES[1].into(),description:"Patch native paragraph formatting for an inclusive zero-based paragraph range (default all paragraphs). Supports nested bullet/decimal lists, numbering restart, hanging indents, before/after spacing and alignment. Omitted format fields inherit the first targeted paragraph; restart:null continues numbering and align:null inherits text-layer alignment. One Undo step; source markers and rich character formatting remain editable.".into(),input_schema:json!({"type":"object","additionalProperties":false,"properties":{"node":node,"first":index,"last":index,"format":{"type":"object","additionalProperties":false,"properties":{"list":{"enum":["none","bullet","numbered"]},"level":{"type":"integer","minimum":0,"maximum":8},"indent":spacing,"hanging":spacing,"space_before":spacing,"space_after":spacing,"align":{"enum":["left","center","right","justify",null]},"restart":{"type":["integer","null"],"minimum":1,"maximum":1000000}}}},"required":["node","format"]})}]
}
pub(crate) fn execute(e: &mut Editor, name: &str, args: &Value) -> Option<ToolResult> {
    if !NAMES.contains(&name) {
        return None;
    }
    Some(match run(e, name, args) {
        Ok(v) => ToolResult::text(v.to_string()),
        Err(e) => ToolResult::error(e),
    })
}
fn run(e: &mut Editor, name: &str, args: &Value) -> Result<Value, String> {
    let object = args.as_object().ok_or("Expected an argument object.")?;
    let allowed = if name == NAMES[0] {
        &["node"][..]
    } else {
        &["node", "first", "last", "format"][..]
    };
    if object.keys().any(|k| !allowed.contains(&k.as_str())) {
        return Err("Unknown paragraph argument.".into());
    }
    let id = args["node"].as_u64().ok_or("node must be an integer.")?;
    let NodeKind::Text { spec, .. } = &e.doc.node(id).ok_or("Text node does not exist.")?.kind
    else {
        return Err("Select editable text.".into());
    };
    let starts = std::iter::once(0)
        .chain(spec.text.match_indices('\n').map(|(i, _)| i + 1))
        .collect::<Vec<_>>();
    if name == NAMES[0] {
        return Ok(
            json!({"node":id,"paragraphs":starts.iter().enumerate().map(|(i,start)|json!({"index":i,"start_byte":start,"text":spec.text[*start..starts.get(i+1).copied().unwrap_or(spec.text.len())].trim_end_matches(['\n','\r']),"format":spec.paragraphs.iter().find(|p|p.start==*start).map(|p|p.format).unwrap_or_default()})).collect::<Vec<_>>()}),
        );
    }
    let index = |key: &str, fallback: usize| -> Result<usize, String> {
        match args.get(key) {
            None => Ok(fallback),
            Some(v) => v
                .as_u64()
                .and_then(|v| usize::try_from(v).ok())
                .ok_or_else(|| format!("{key} must be an integer.")),
        }
    };
    let first = index("first", 0)?;
    let last = index("last", starts.len() - 1)?;
    if first > last || last >= starts.len() {
        return Err("Choose an existing inclusive paragraph range.".into());
    }
    let current = spec
        .paragraphs
        .iter()
        .find(|p| p.start == starts[first])
        .map(|p| p.format)
        .unwrap_or_default();
    let mut format = serde_json::to_value(current).map_err(|e| e.to_string())?;
    let patch = args
        .get("format")
        .and_then(Value::as_object)
        .ok_or("format must be an object.")?;
    for (k, v) in patch {
        format.as_object_mut().unwrap().insert(k.clone(), v.clone());
    }
    let mut format: ParagraphFormat = serde_json::from_value(format).map_err(|e| e.to_string())?;
    if current.list == text::ParagraphList::None && format.list != text::ParagraphList::None {
        if !patch.contains_key("indent") {
            format.indent = spec.size * 1.5;
        }
        if !patch.contains_key("hanging") {
            format.hanging = spec.size * 1.2;
        }
    }
    let end = starts.get(last + 1).copied().unwrap_or(spec.text.len());
    let result = text::apply_paragraphs(spec, starts[first]..end, format)?;
    if e.in_transaction() {
        return Err("Finish the current text edit first.".into());
    }
    e.execute(Command::SetText {
        id,
        spec: Box::new(result),
    })
    .map_err(|e| e.to_string())?;
    Ok(json!({"node":id,"first":first,"last":last}))
}

#[cfg(test)]
mod tests {
    use super::*;
    use emulsion_core::{Document, Node, command::Slot, text::TextSpec};
    #[test]
    fn mcp_paragraph_ranges_unicode_nesting_and_invalid_changes_are_atomic() {
        let mut e = Editor::new(Document::new(400, 300), None);
        let id = e
            .execute(Command::AddNode {
                node: Box::new(Node::text(
                    0,
                    "List",
                    TextSpec {
                        text: "Alpha 😀\nBeta\nGamma".into(),
                        size: 18.,
                        width: Some(170.),
                        ..Default::default()
                    },
                    400,
                    300,
                )),
                slot: Slot::TOP,
            })
            .unwrap()
            .unwrap();
        let call = |e: &mut Editor, args| crate::exec::execute(e, "format_text_paragraphs", &args);
        let original = e.doc.clone();
        assert!(!call(&mut e, json!({"node":id,"format":{"list":"numbered"}})).is_error);
        assert!(
            !call(
                &mut e,
                json!({"node":id,"first":1,"last":1,"format":{"level":1,"space_after":12}})
            )
            .is_error
        );
        let NodeKind::Text { spec, .. } = &e.doc.node(id).unwrap().kind else {
            panic!()
        };
        assert_eq!(spec.text, "1. Alpha 😀\n1.1. Beta\n2. Gamma");
        let before = e.doc.clone();
        for args in [
            json!({"node":id,"first":-1,"format":{}}),
            json!({"node":id,"format":{"level":9}}),
            json!({"node":id,"format":{"indent":"20"}}),
            json!({"node":id,"format":{"unknown":true}}),
        ] {
            assert!(call(&mut e, args).is_error);
            assert_eq!(e.doc, before);
        }
        assert!(crate::tools::is_read_only("inspect_text_paragraphs"));
        e.undo();
        e.undo();
        assert_eq!(e.doc, original);
    }
}
