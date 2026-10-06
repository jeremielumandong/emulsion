//! Local raster placement plans decode off the UI thread and retain source pixels.
use crate::{ToolDef, ToolResult, exec::Planned};
use emulsion_core::{Command, Document, Node, command::Slot};
use emulsion_raster::Placement;
use serde::Deserialize;
use serde_json::{Value, json};
use std::{path::PathBuf, sync::Arc};

#[derive(Deserialize)]
#[serde(deny_unknown_fields)]
struct Request {
    path: PathBuf,
    #[serde(default)]
    x: f64,
    #[serde(default)]
    y: f64,
    width: Option<f64>,
    name: Option<String>,
}
pub(crate) fn definitions() -> Vec<ToolDef> {
    vec![ToolDef {
        name:"import_image".into(),
        description:"Place a local raster image (PNG/JPEG/WebP/TIFF and other native image-decoder formats) into the active page as a new editable pixel layer. Applies EXIF orientation, retains original pixel resolution and bit depth, and uses lossless placement scaling. Width defaults to source width; height follows aspect ratio. Does not replace the project or modify the source file. Decode runs in the background and a changed target rejects the result.".into(),
        input_schema:json!({"type":"object","additionalProperties":false,"required":["path"],"properties":{
            "path":{"type":"string","minLength":1},"x":{"type":"number","minimum":-1000000,"maximum":1000000},"y":{"type":"number","minimum":-1000000,"maximum":1000000},"width":{"type":"number","exclusiveMinimum":0,"maximum":1000000},"name":{"type":"string","minLength":1,"maxLength":200}}}),
    }]
}
pub(crate) fn plan(doc: &Document, args: &Value) -> Result<Planned, ToolResult> {
    let request: Request =
        serde_json::from_value(args.clone()).map_err(|e| ToolResult::error(e.to_string()))?;
    if request.path.as_os_str().is_empty()
        || [request.x, request.y]
            .iter()
            .any(|v| !v.is_finite() || v.abs() > 1_000_000.)
        || request
            .width
            .is_some_and(|w| !w.is_finite() || w <= 0. || w > 1_000_000.)
        || request.name.as_ref().is_some_and(|s| {
            s.trim().is_empty() || s.chars().count() > 200 || s.chars().any(char::is_control)
        })
    {
        return Err(ToolResult::error(
            "Invalid image path, placement, width, or layer name",
        ));
    }
    let decoded =
        emulsion_io::import::decode(&request.path).map_err(|e| ToolResult::error(e.to_string()))?;
    let size = (decoded.raster.width(), decoded.raster.height());
    let scale = request.width.map_or(1., |w| w / f64::from(size.0));
    let placement = Placement {
        x: request.x,
        y: request.y,
        scale_x: scale,
        scale_y: scale,
        ..Default::default()
    };
    let label = request.name.unwrap_or_else(|| {
        request
            .path
            .file_name()
            .unwrap_or_default()
            .to_string_lossy()
            .into_owned()
    });
    let command = Command::AddNode {
        node: Box::new(Node::raster(0, label, Arc::new(decoded.raster), placement)),
        slot: Slot::TOP,
    };
    let mut trial = doc.clone();
    let id = command
        .apply(&mut trial)
        .map_err(|e| ToolResult::error(e.to_string()))?;
    Ok(Planned {
        smart_input: None,
        commands: vec![command],
        message: json!({"node":id,"source_size":size,"source_bit_depth":decoded.depth,"placement":placement}).to_string(),
        feedback: None,
        deferred: None,
    })
}

#[cfg(test)]
mod tests {
    use super::*;
    #[test]
    fn imported_pixels_keep_source_size_and_one_undo() {
        let path =
            std::env::temp_dir().join(format!("emulsion-mcp-image-{}.png", std::process::id()));
        image::RgbaImage::from_pixel(8, 4, image::Rgba([220, 30, 10, 255]))
            .save(&path)
            .unwrap();
        let mut editor = emulsion_core::Editor::new(Document::new(64, 64), None);
        let before = editor.doc.clone();
        let result = crate::exec::execute(
            &mut editor,
            "import_image",
            &json!({"path":path,"x":3,"width":32}),
        );
        assert!(!result.is_error, "{result:?}");
        let node = editor.doc.nodes.last().unwrap();
        if let emulsion_core::NodeKind::Raster {
            raster, placement, ..
        } = &node.kind
        {
            assert_eq!(raster.width(), 8);
            assert_eq!(placement.scale_x, 4.);
            assert_eq!(placement.x, 3.);
        } else {
            panic!("pixel source")
        }
        assert!(editor.undo());
        assert_eq!(editor.doc, before);
        let result =
            crate::exec::execute(&mut editor, "import_image", &json!({"path":path,"width":0}));
        assert!(result.is_error);
        assert_eq!(editor.doc, before);
        std::fs::remove_file(path).unwrap();
    }
}
