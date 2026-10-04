//! Local image/smart source replacement and non-destructive crop authoring.
use crate::{ToolDef, ToolResult};
use emulsion_core::{Editor, NodeKind, photo_source};
use serde_json::{Value, json};
pub const READ_ONLY: &[&str] = &["get_photo_source"];
pub const DESTRUCTIVE: &[&str] = &["replace_photo_source", "crop_photo_source"];
pub fn is_tool(name: &str) -> bool {
    READ_ONLY.contains(&name) || DESTRUCTIVE.contains(&name)
}
pub fn definitions() -> Vec<ToolDef> {
    let node = json!({"type":"integer","minimum":1});
    [
 ("get_photo_source","Inspect image or Smart Object source dimensions, placement, editable source kind and filter count without rasterizing vectors.",json!({"node":node}),vec!["node"]),
 ("replace_photo_source","Replace a raster image or Smart Object from a local decoded image. Retains displayed bounds, rotation, effects and smart filters; editable Smart sources become the chosen pixels. Existing masks scale to source dimensions unless their transform requires manual reset. One Undo restores source; no external file is modified.",json!({"node":node,"path":{"type":"string","minLength":1},"expected_revision":{"type":"integer","minimum":0}}),vec!["node","path"]),
 ("crop_photo_source","Crop an image or Smart Object with a non-destructive source-pixel rectangular layer mask. Intersects an existing enabled untransformed mask; original pixels remain editable. Rect is [x,y,width,height] within source dimensions. Undo restores the prior mask.",json!({"node":node,"rect":{"type":"array","items":{"type":"number"},"minItems":4,"maxItems":4},"expected_revision":{"type":"integer","minimum":0}}),vec!["node","rect"]),
].into_iter().map(|(name,description,properties,required)|ToolDef{name:name.into(),description:description.into(),input_schema:json!({"type":"object","properties":properties,"required":required,"additionalProperties":false})}).collect()
}
pub fn execute(editor: &mut Editor, name: &str, args: &Value) -> Option<ToolResult> {
    if !is_tool(name) {
        return None;
    }
    Some(match run(editor, name, args) {
        Ok(v) => ToolResult::text(v.to_string()),
        Err(e) => ToolResult::error(e),
    })
}
fn run(editor: &mut Editor, name: &str, args: &Value) -> Result<Value, String> {
    let object = args.as_object().ok_or("Expected an argument object.")?;
    let allowed: &[&str] = match name {
        "get_photo_source" => &["node"],
        "replace_photo_source" => &["node", "path", "expected_revision"],
        _ => &["node", "rect", "expected_revision"],
    };
    if object.keys().any(|k| !allowed.contains(&k.as_str())) {
        return Err("Unknown photo-source argument.".into());
    }
    let id = args
        .get("node")
        .and_then(Value::as_u64)
        .filter(|id| *id > 0)
        .ok_or("node must be a positive image ID.")?;
    if let Some(revision) = args.get("expected_revision")
        && revision.as_u64() != Some(editor.revision)
    {
        return Err("The document revision changed. Inspect the source and retry.".into());
    }
    match name {
        "replace_photo_source" => {
            let path = args
                .get("path")
                .and_then(Value::as_str)
                .filter(|v| !v.is_empty())
                .ok_or("Select a local image path.")?;
            photo_source::dimensions(&editor.doc, id).ok_or("Select an image or Smart Object.")?;
            let decoded = emulsion_io::import::decode(std::path::Path::new(path))
                .map_err(|e| e.to_string())?;
            photo_source::replace(editor, id, std::sync::Arc::new(decoded.raster))?;
        }
        "crop_photo_source" => {
            let rect =
                serde_json::from_value(args.get("rect").ok_or("Missing crop rectangle.")?.clone())
                    .map_err(|e| e.to_string())?;
            photo_source::crop(editor, id, rect)?;
        }
        _ => {}
    }
    let node = editor.doc.node(id).ok_or("Missing source object.")?;
    let (dimensions, placement, filters, editable) = match &node.kind {
        NodeKind::Raster { raster, placement } => {
            ([raster.width(), raster.height()], placement, 0, "raster")
        }
        NodeKind::Smart {
            source,
            placement,
            filters,
            editable,
            ..
        } => (
            [source.width(), source.height()],
            placement,
            filters.len(),
            match editable {
                Some(emulsion_core::node::SmartEditable::Text { .. }) => "text",
                Some(emulsion_core::node::SmartEditable::Path { .. }) => "path",
                Some(emulsion_core::node::SmartEditable::Svg { .. }) => "svg",
                Some(emulsion_core::node::SmartEditable::Document { .. }) => "document",
                None => "raster",
            },
        ),
        _ => return Err("Select an image or Smart Object.".into()),
    };
    Ok(
        json!({"node":id,"dimensions":dimensions,"placement":[placement.x,placement.y,placement.scale_x,placement.scale_y,placement.rotation],"filters":filters,"editable_source":editable,"has_mask":node.has_mask(),"has_raster_mask":node.mask.is_some(),"has_vector_mask":node.vector_mask.is_some(),"revision":editor.revision}),
    )
}
#[cfg(test)]
mod tests {
    use super::*;
    #[test]
    fn photo_source_rejects_bad_targets_before_file_io() {
        let mut e = Editor::new(emulsion_core::Document::new(100, 100), None);
        let before = e.doc.clone();
        assert!(
            execute(
                &mut e,
                "replace_photo_source",
                &json!({"node":99,"path":"missing.png"})
            )
            .unwrap()
            .is_error
        );
        assert_eq!(e.doc, before);
        assert!(
            execute(
                &mut e,
                "crop_photo_source",
                &json!({"node":99,"rect":[0,0,10,10],"shell":"x"})
            )
            .unwrap()
            .is_error
        );
    }
}
