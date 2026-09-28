//! Native portable typography and explicit palette targets.
use crate::{ToolResult, server::ToolDef};
use emulsion_core::{
    Editor, NodeId,
    design_brand_assets::{self as brand, ColorTarget},
};
use serde::Deserialize;
use serde_json::{Value, json};
pub const NAMES: &[&str] = &[
    "get_embedded_fonts",
    "embed_design_font",
    "extract_selection_colors",
    "apply_palette_color",
    "apply_brand_typography",
    "apply_brand_kit",
];
pub const READ_ONLY: &[&str] = &["get_embedded_fonts", "extract_selection_colors"];
pub const DESTRUCTIVE: &[&str] = &[
    "embed_design_font",
    "apply_palette_color",
    "apply_brand_typography",
    "apply_brand_kit",
];
#[derive(Deserialize)]
#[serde(deny_unknown_fields)]
struct Selection {
    ids: Vec<NodeId>,
}
#[derive(Deserialize)]
#[serde(deny_unknown_fields)]
struct Font {
    ids: Vec<NodeId>,
    path: std::path::PathBuf,
}
#[derive(Deserialize)]
#[serde(deny_unknown_fields)]
struct Color {
    ids: Vec<NodeId>,
    color: [u8; 4],
    target: ColorTarget,
}
#[derive(Deserialize)]
#[serde(deny_unknown_fields)]
struct Apply {
    ids: Vec<NodeId>,
    brand_id: u64,
    role: Option<String>,
}
fn parse<T: serde::de::DeserializeOwned>(v: &Value) -> Result<T, String> {
    serde_json::from_value(v.clone()).map_err(|e| e.to_string())
}
pub fn load_font(
    path: &std::path::Path,
) -> Result<emulsion_core::design_fonts::EmbeddedFont, String> {
    use std::io::Read;
    if !path.is_absolute() {
        return Err("Use an absolute local TTF/OTF path.".into());
    }
    let mut bytes = Vec::new();
    std::fs::File::open(path)
        .map_err(|e| e.to_string())?
        .take((emulsion_core::design_fonts::MAX_FONT_BYTES + 1) as u64)
        .read_to_end(&mut bytes)
        .map_err(|e| e.to_string())?;
    emulsion_core::design_fonts::EmbeddedFont::from_bytes(bytes)
}
pub fn execute(editor: &mut Editor, name: &str, args: &Value) -> Option<ToolResult> {
    if !NAMES.contains(&name) {
        return None;
    }
    Some(match run(editor, name, args) {
        Ok(v) => ToolResult::text(v.to_string()),
        Err(e) => ToolResult::error(e),
    })
}
fn run(editor: &mut Editor, name: &str, args: &Value) -> Result<Value, String> {
    match name {
        "get_embedded_fonts" => {
            if !args.as_object().is_some_and(|v| v.is_empty()) {
                return Err("get_embedded_fonts takes no arguments".into());
            }
            Ok(
                json!({"fonts":editor.doc.design.fonts.values().map(|f|json!({"alias":f.alias(),"family":f.family(),"bytes":f.bytes().len()})).collect::<Vec<_>>()}),
            )
        }
        "embed_design_font" => {
            let a: Font = parse(args)?;
            let font = load_font(&a.path)?;
            let out =
                json!({"alias":font.alias(),"family":font.family(),"bytes":font.bytes().len()});
            emulsion_core::design_fonts::embed(editor, &a.ids, font)?;
            Ok(out)
        }
        "extract_selection_colors" => {
            let a: Selection = parse(args)?;
            Ok(
                json!({"colors":brand::extract_colors(&editor.doc,&a.ids)?,"format":"RGBA","maximum":32}),
            )
        }
        "apply_palette_color" => {
            let a: Color = parse(args)?;
            brand::apply_color(editor, &a.ids, a.color, a.target)?;
            Ok(json!({"applied":true,"target":a.target}))
        }
        "apply_brand_typography" | "apply_brand_kit" => {
            let a: Apply = parse(args)?;
            let catalog =
                emulsion_io::creative_library::load(&emulsion_io::creative_library::root())
                    .map_err(|e| e.to_string())?;
            let kit = catalog
                .brands
                .iter()
                .find(|b| b.id == a.brand_id)
                .ok_or("Brand kit no longer exists.")?;
            if name == "apply_brand_typography" {
                let role = a
                    .role
                    .as_ref()
                    .and_then(|name| kit.typography.get(name))
                    .ok_or("Choose an existing named typography role.")?;
                brand::apply_role(editor, &a.ids, role, &kit.fonts)?;
            } else {
                if a.role.is_some() {
                    return Err("Use apply_brand_typography for a named role.".into());
                }
                if editor.in_transaction() {
                    return Err("Finish the current edit first.".into());
                }
                let mut trial = Editor::new(editor.doc.clone(), None);
                let mut design = trial.doc.design.clone();
                if let Some(font)=kit.fonts.get(&kit.font){design.fonts.insert(kit.font.clone(),font.clone());}
                trial
                    .execute(emulsion_core::Command::SetDesign {
                        design: Box::new(design),
                    })
                    .map_err(|e| e.to_string())?;
                emulsion_core::design::brand::apply(&mut trial, &a.ids, &kit.font, &kit.colors)?;
                editor.commit_design_document(trial.doc, "Apply portable brand kit")?;
            }
            Ok(json!({"applied":true,"brand_id":a.brand_id}))
        }
        _ => Err("Unknown brand tool".into()),
    }
}
pub fn definitions() -> Vec<ToolDef> {
    let ids = json!({"type":"array","minItems":1,"maxItems":10000,"items":{"type":"integer","minimum":1}});
    let id = json!({"type":"integer","minimum":1});
    let def = |name: &str, description: &str, properties: Value, required: &[&str]| ToolDef {
        name: name.into(),
        description: description.into(),
        input_schema: json!({"type":"object","additionalProperties":false,"properties":properties,"required":required}),
    };
    vec![
        def(
            NAMES[0],
            "Inspect embedded font aliases, original families and byte sizes without returning font binaries.",
            json!({}),
            &[],
        ),
        def(
            NAMES[1],
            "Embed a local editable-embedding TTF/OTF (at most 8 MiB) and apply its private family alias to selected native text. Font bytes travel with the project/clipboard/templates; one Undo, no font download or system installation.",
            json!({"ids":ids,"path":{"type":"string","minLength":1}}),
            &["ids", "path"],
        ),
        def(
            NAMES[2],
            "Extract up to 32 unique RGBA colors from selected native text, fills, paths, gradient endpoints and patterns. Does not sample raster photos or modify artwork.",
            json!({"ids":ids}),
            &["ids"],
        ),
        def(
            NAMES[3],
            "Apply a solid RGBA palette color explicitly to fill, stroke, text or all paints, preserving other property targets and native geometry. Alpha zero is transparent. Locked selections reject atomically.",
            json!({"ids":ids,"color":{"type":"array","minItems":4,"maxItems":4,"items":{"type":"integer","minimum":0,"maximum":255}},"target":{"type":"string","enum":["fill","stroke","text","all"]}}),
            &["ids", "color", "target"],
        ),
        def(
            NAMES[4],
            "Apply a saved brand typography role to selected text with portable font resources. Preserves content and geometry; one Undo.",
            json!({"ids":ids,"brand_id":id,"role":{"type":"string","minLength":1}}),
            &["ids", "brand_id", "role"],
        ),
        def(
            NAMES[5],
            "Apply the native brand font/palette to selected text and paths, carrying embedded font resources. One atomic Undo; protected objects reject the complete change.",
            json!({"ids":ids,"brand_id":id}),
            &["ids", "brand_id"],
        ),
    ]
}
#[cfg(test)]
mod tests {
    use super::*;
    #[test]
    fn palette_tools_validate_targets_and_do_not_mutate_on_invalid_input() {
        let mut e = Editor::new(emulsion_core::Document::new(100, 100), None);
        let before = e.doc.clone();
        for a in [
            json!({"ids":[],"color":[0,0,0,0],"target":"fill"}),
            json!({"ids":[1],"color":[0,0,0,256],"target":"fill"}),
            json!({"ids":[1],"color":[0,0,0,0],"target":"other"}),
        ] {
            assert!(run(&mut e, "apply_palette_color", &a).is_err());
            assert_eq!(e.doc, before);
        }
        assert!(run(&mut e, "get_embedded_fonts", &json!({"path":"/tmp/no"})).is_err());
    }
}
