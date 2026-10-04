//! The Stage: the camera frame's guides and the board palette (read by
//! describe_storyboard, set by set_storyboard_settings), and outside pictures
//! (PSD, ORA, PNG, JPEG…) brought in as new panels or as layers on a panel.
use super::{def, insertion, panel_id, placement};
use crate::ToolDef;
use emulsion_core::command::Slot;
use emulsion_core::fragment::Fragment;
use emulsion_core::project::ProjectEditor;
use emulsion_core::storyboard::{Frame, StageGuides, Storyboard};
use emulsion_core::storyboard_naming::{centred_frame, fit_document};
use emulsion_core::storyboard_stage::{DEFAULT_PALETTE, MAX_PALETTE};
use emulsion_core::{Document, Editor, Node, NodeId};
use serde_json::{Value, json};
use std::collections::HashSet;
use std::path::Path;

/// Most files one import may read.
const MAX_FILES: usize = 50;
const EXTENSIONS: [&str; 9] = [
    "psd", "psb", "ora", "png", "jpg", "jpeg", "webp", "tif", "tiff",
];

/// `guides` for set_storyboard_settings.
pub(super) fn guide_fields() -> Value {
    let percent = json!({"type":"number","minimum":0,"maximum":100,"description":"Percentage of the camera frame; 0 hides it."});
    json!({
        "type":"object",
        "additionalProperties":false,
        "description":"Stage guides drawn over the camera frame. Omitted values stay.",
        "properties":{
            "action_safe":percent,
            "title_safe":percent,
            "field_guide":{"type":"boolean","description":"Show the field guide."},
            "fields":{"type":"integer","minimum":2,"maximum":24,"description":"Fields in the field guide (the frame is the outermost)."},
            "overscan":{"type":"number","minimum":0,"maximum":StageGuides::MAX_OVERSCAN,"description":"Space shown around the frame for art that runs outside the shot, as a percentage of the frame on each side."}
        }
    })
}

/// `palette` for set_storyboard_settings.
pub(super) fn palette_fields() -> Value {
    let colors = |min: usize, description: &str| json!({"type":"array","items":{"type":"string","maxLength":7},"minItems":min,"maxItems":MAX_PALETTE,"description":description});
    json!({
        "type":"object",
        "additionalProperties":false,
        "description":"Change the board's colour palette (#RRGGBB). Applied in order: reset or set, remove, add.",
        "properties":{
            "reset":{"type":"boolean","description":"Go back to the default storyboard palette."},
            "set":colors(0, "Replace the whole palette."),
            "remove":colors(1, "Colours to remove; each must be in the palette."),
            "add":colors(1, "Colours to append; ones already there are skipped.")
        }
    })
}

fn hex(color: [u8; 3]) -> String {
    format!("#{:02X}{:02X}{:02X}", color[0], color[1], color[2])
}

fn parse_hex(value: &Value) -> Result<[u8; 3], String> {
    let text = value.as_str().unwrap_or_default();
    let digits = text
        .strip_prefix('#')
        .filter(|d| d.len() == 6 && d.bytes().all(|b| b.is_ascii_hexdigit()))
        .ok_or_else(|| format!("Palette colours are #RRGGBB, not '{text}'."))?;
    let channel = |i: usize| u8::from_str_radix(&digits[i * 2..i * 2 + 2], 16).unwrap();
    Ok([channel(0), channel(1), channel(2)])
}

fn colors(spec: &Value, key: &str) -> Result<Vec<[u8; 3]>, String> {
    spec[key]
        .as_array()
        .into_iter()
        .flatten()
        .map(parse_hex)
        .collect()
}

/// Apply `guides` and `palette` from set_storyboard_settings; the board
/// validates the result.
pub(super) fn apply_settings(board: &mut Storyboard, args: &Value) -> Result<(), String> {
    if let Some(changes) = args["guides"].as_object() {
        let mut guides = serde_json::to_value(&board.stage).map_err(|e| e.to_string())?;
        for (key, value) in changes {
            guides[key] = value.clone();
        }
        board.stage = serde_json::from_value(guides).map_err(|e| e.to_string())?;
    }
    let spec = &args["palette"];
    if !spec.is_object() {
        return Ok(());
    }
    let palette = &mut board.palette;
    match (spec["reset"] == true, spec.get("set")) {
        (true, Some(_)) => return Err("Use either reset or set for the palette.".into()),
        (true, None) => *palette = DEFAULT_PALETTE.to_vec(),
        (false, Some(_)) => {
            palette.clear();
            for color in colors(spec, "set")? {
                if !palette.contains(&color) {
                    palette.push(color);
                }
            }
        }
        (false, None) => {}
    }
    for color in colors(spec, "remove")? {
        let at = palette
            .iter()
            .position(|c| *c == color)
            .ok_or_else(|| format!("{} is not in the palette.", hex(color)))?;
        palette.remove(at);
    }
    for color in colors(spec, "add")? {
        if !palette.contains(&color) {
            palette.push(color);
        }
    }
    Ok(())
}

/// A rectangle in panel pixels, to a hundredth of a pixel.
fn rect(frame: Frame) -> Value {
    let round = |v: f64| (v * 100.).round() / 100.;
    json!({"x":round(frame.x),"y":round(frame.y),"width":round(frame.w),"height":round(frame.h)})
}

/// The guides with their rectangles in panel pixels, for describe_storyboard.
pub(super) fn guides_json(board: &Storyboard) -> Value {
    let (width, height) = (board.settings.width, board.settings.height);
    let guides = &board.stage;
    let safe = |percent: f64| (percent > 0.).then(|| rect(Frame::centred(width, height, percent)));
    let mut out = serde_json::to_value(guides).unwrap_or_default();
    out["action_safe_rect"] = safe(guides.action_safe).into();
    out["title_safe_rect"] = safe(guides.title_safe).into();
    out["field_rects"] = guides
        .field_rects(width, height)
        .into_iter()
        .map(rect)
        .collect();
    out["stage_area"] = rect(guides.stage_area(width, height));
    out
}

pub(super) fn palette_json(board: &Storyboard) -> Value {
    board.palette.iter().copied().map(hex).collect()
}

pub(super) fn definitions() -> Vec<ToolDef> {
    let mut fields = placement();
    fields["paths"] = json!({"type":"array","items":{"type":"string","maxLength":4096},"minItems":1,"maxItems":MAX_FILES,"description":"Absolute paths of PSD, PSB, ORA, PNG, JPEG, WebP or TIFF files."});
    fields["into"] = json!({"enum":["panels","layers"],"description":"panels (default): one new panel per file; layers: every file's layers on top of `panel`."});
    fields["panel"] = panel_id();
    vec![def(
        "import_storyboard_files",
        "Bring outside pictures into the storyboard: layouts, reference art or finished drawings. into \"panels\" (default) adds one panel per file, named after it, after a panel (default: the active panel) or at_start, in that panel's scene, with the default duration. into \"layers\" places every file's layers on top of `panel` (default: the active panel) and selects it. Pictures at another size or aspect are cropped to the centre and scaled to the panel resolution. Layers stay editable; PSD and ORA files keep their groups, opacity, masks, blend modes and clipping. Returns each new panel's or the placed layers with their blend mode and clipping. One Undo step.",
        fields,
        &["paths"],
    )]
}

/// Run a stage tool; `None` when `name` is not one.
pub(super) fn run(
    editor: &mut ProjectEditor,
    board: &Storyboard,
    name: &str,
    args: &Value,
) -> Option<Result<Value, String>> {
    match name {
        "import_storyboard_files" => Some(import_files(editor, board, args)),
        _ => None,
    }
}

/// Every file read, named after it, before anything changes.
fn read_files(args: &Value) -> Result<Vec<(String, Document)>, String> {
    let mut out = Vec::new();
    for text in args["paths"]
        .as_array()
        .unwrap()
        .iter()
        .filter_map(Value::as_str)
    {
        let path = Path::new(text);
        if !path.is_absolute() {
            return Err(format!("Use an absolute path, not '{text}'."));
        }
        let extension = path
            .extension()
            .map(|e| e.to_string_lossy().to_ascii_lowercase())
            .unwrap_or_default();
        if !EXTENSIONS.contains(&extension.as_str()) {
            return Err(format!(
                "'{text}' is not a PSD, PSB, ORA, PNG, JPEG, WebP or TIFF file."
            ));
        }
        let doc = emulsion_io::open(path).map_err(|e| format!("{text}: {e}"))?;
        let name = path
            .file_stem()
            .map_or_else(|| "Import".into(), |s| s.to_string_lossy().into_owned());
        out.push((name, doc));
    }
    Ok(out)
}

/// Several pictures as one document at the panel size, later files on top,
/// so placing them is one Undo step.
fn stack(documents: Vec<(String, Document)>, width: u32, height: u32) -> Result<Document, String> {
    if documents.len() == 1 {
        return Ok(documents.into_iter().next().unwrap().1);
    }
    let mut sheet = Editor::new(Document::new(width, height), None);
    for (_, doc) in documents {
        let doc = if (doc.width, doc.height) == (width, height) {
            doc
        } else {
            fit_document(&doc, centred_frame(&doc, width, height), width, height)
        };
        let roots: Vec<_> = doc
            .nodes
            .iter()
            .filter(|n| n.parent.is_none())
            .map(|n| n.id)
            .collect();
        if !roots.is_empty() {
            Fragment::capture(&doc, &roots)?.paste(&mut sheet, Slot::TOP, (0., 0.))?;
        }
    }
    Ok(sheet.doc)
}

/// Layers with their blend mode, opacity, clipping and mask, in stack order.
fn layers_json(doc: &Document, keep: impl Fn(&Node) -> bool) -> Vec<Value> {
    doc.nodes
        .iter()
        .filter(|n| keep(n))
        .map(|n| {
            let mut layer = json!({
                "id":n.id,
                "name":n.name,
                "blend":n.blend.label(),
                "opacity":(n.opacity * 100.).round(),
            });
            if let Some(parent) = n.parent {
                layer["parent"] = parent.into();
            }
            if let Some(base) = n.clip_to {
                layer["clipped_to"] = base.into();
            }
            if n.has_mask() {
                layer["mask"] = true.into();
            }
            layer
        })
        .collect()
}

fn import_files(
    editor: &mut ProjectEditor,
    board: &Storyboard,
    args: &Value,
) -> Result<Value, String> {
    let into_layers = args["into"] == "layers";
    if into_layers && (args.get("after").is_some() || args.get("at_start").is_some()) {
        return Err("after and at_start place new panels; give panel with into \"layers\".".into());
    }
    if !into_layers && args.get("panel").is_some() {
        return Err(
            "panel is for into \"layers\"; new panels go after a panel or at_start.".into(),
        );
    }
    let documents = read_files(args)?;
    if !into_layers {
        let after = insertion(args, editor.active_page())?;
        let ids = editor.import_panels(after, documents)?;
        let panels: Vec<_> = ids
            .iter()
            .map(|id| {
                let name = editor.page_list().iter().find(|m| m.id == *id).unwrap();
                let doc = &editor.page(*id).unwrap().doc;
                json!({"panel":id,"name":name.name,"layers":layers_json(doc, |_| true)})
            })
            .collect();
        return Ok(json!({"panels":panels,"active_panel":editor.active_page()}));
    }
    let target = args["panel"].as_u64().unwrap_or(editor.active_page());
    if !board.panels.contains_key(&target) {
        return Err(format!("Panel {target} does not exist."));
    }
    let doc = stack(documents, board.settings.width, board.settings.height)?;
    let previous = editor.active_page();
    editor.set_active_page(target)?;
    let placed: Vec<NodeId> = editor.place_layers(&doc).inspect_err(|_| {
        let _ = editor.set_active_page(previous);
    })?;
    let included: HashSet<_> = placed
        .iter()
        .flat_map(|id| editor.doc.subtree(*id))
        .collect();
    Ok(json!({
        "panel":target,
        "layers":layers_json(&editor.doc, |n| included.contains(&n.id)),
        "active_panel":editor.active_page(),
    }))
}
