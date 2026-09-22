//! Agent-facing editable typography validation and operations.

use crate::server::ToolResult;
use emulsion_core::text::{Align, AntiAliasMode, TextSpec, TextStyle};
use emulsion_core::text_effects::{TextPath, TextPathMode, WarpStyle};
use emulsion_core::{Command, Editor, NodeId, NodeKind};
use emulsion_raster::vector::Path;
use serde_json::{Value, json};

fn error(message: impl Into<String>) -> ToolResult {
    ToolResult::error(message)
}

fn number(args: &Value, key: &str, min: f32, max: f32) -> Result<Option<f32>, ToolResult> {
    let Some(value) = args.get(key) else {
        return Ok(None);
    };
    let value = value
        .as_f64()
        .filter(|value| value.is_finite() && *value >= min as f64 && *value <= max as f64)
        .ok_or_else(|| error(format!("{key} must be a number from {min} to {max}")))?;
    Ok(Some(value as f32))
}

fn boolean(args: &Value, key: &str) -> Result<Option<bool>, ToolResult> {
    match args.get(key) {
        None => Ok(None),
        Some(Value::Bool(value)) => Ok(Some(*value)),
        Some(_) => Err(error(format!("{key} must be a boolean"))),
    }
}

fn color(value: &Value, key: &str) -> Result<[u8; 4], ToolResult> {
    let value = value
        .as_str()
        .and_then(|value| value.strip_prefix('#'))
        .ok_or_else(|| error(format!("{key} must be #RRGGBB or #RRGGBBAA")))?;
    if !matches!(value.len(), 6 | 8) || !value.bytes().all(|byte| byte.is_ascii_hexdigit()) {
        return Err(error(format!("{key} must be #RRGGBB or #RRGGBBAA")));
    }
    let mut rgba = [255; 4];
    for (index, channel) in rgba.iter_mut().enumerate().take(value.len() / 2) {
        *channel = u8::from_str_radix(&value[index * 2..index * 2 + 2], 16).unwrap();
    }
    Ok(rgba)
}

#[derive(Default)]
struct StyleEdit {
    font: Option<String>,
    size: Option<f32>,
    color: Option<[u8; 4]>,
    bold: Option<bool>,
    italic: Option<bool>,
    letter_spacing: Option<f32>,
    baseline: Option<f32>,
}

impl StyleEdit {
    fn parse(args: &Value) -> Result<Self, ToolResult> {
        Ok(Self {
            font: match args.get("font") {
                None => None,
                Some(Value::String(value)) => Some(value.trim().to_string()),
                Some(_) => return Err(error("font must be a string")),
            },
            size: number(args, "size", 1.0, 4000.0)?,
            color: args
                .get("color")
                .map(|value| color(value, "color"))
                .transpose()?,
            bold: boolean(args, "bold")?,
            italic: boolean(args, "italic")?,
            letter_spacing: number(args, "letter_spacing", -50.0, 500.0)?,
            baseline: number(args, "baseline", -4000.0, 4000.0)?,
        })
    }

    fn is_empty(&self) -> bool {
        self.font.is_none()
            && self.size.is_none()
            && self.color.is_none()
            && self.bold.is_none()
            && self.italic.is_none()
            && self.letter_spacing.is_none()
            && self.baseline.is_none()
    }

    fn apply(&self, style: &mut TextStyle) {
        if let Some(value) = &self.font {
            style.font = value.clone();
        }
        if let Some(value) = self.size {
            style.size = value;
        }
        if let Some(value) = self.color {
            style.color = value;
        }
        if let Some(value) = self.bold {
            style.bold = value;
        }
        if let Some(value) = self.italic {
            style.italic = value;
        }
        if let Some(value) = self.letter_spacing {
            style.letter_spacing = value;
        }
        if let Some(value) = self.baseline {
            style.baseline = value;
        }
    }

    fn update_base(&self, spec: &mut TextSpec) {
        if let Some(value) = &self.font {
            spec.font = value.clone();
        }
        if let Some(value) = self.size {
            spec.size = value;
        }
        if let Some(value) = self.color {
            spec.color = value;
        }
        if let Some(value) = self.bold {
            spec.bold = value;
        }
        if let Some(value) = self.italic {
            spec.italic = value;
        }
        if let Some(value) = self.letter_spacing {
            spec.letter_spacing = value;
        }
    }
}

/// Parse all fields shared by `add_text` and `set_text` before a command is created.
pub(crate) fn parse_text_args(args: &Value, mut spec: TextSpec) -> Result<TextSpec, ToolResult> {
    if !args.is_object() {
        return Err(error("text arguments must be an object"));
    }
    if let Some(value) = args.get("text") {
        let value = value
            .as_str()
            .ok_or_else(|| error("text must be a string"))?;
        let end = spec.text.len();
        spec.replace_range(0..end, value);
    }
    let style = StyleEdit::parse(args)?;
    style.update_base(&mut spec);
    if !style.is_empty() && !spec.text.is_empty() {
        let end = spec.text.len();
        spec.apply_style(0..end, |current| style.apply(current));
    }
    for (key, destination) in [("x", &mut spec.x), ("y", &mut spec.y)] {
        if let Some(value) = number(args, key, -1_000_000_000.0, 1_000_000_000.0)? {
            *destination = value;
        }
    }
    if let Some(value) = number(args, "line_height", 0.5, 4.0)? {
        spec.line_height = value;
    }
    if let Some(value) = boolean(args, "vertical")? {
        spec.vertical = value;
    }
    if let Some(value) = args.get("align") {
        let key = value
            .as_str()
            .ok_or_else(|| error("align must be left, center, right or justify"))?;
        spec.align = Align::parse(key)
            .ok_or_else(|| error("align must be left, center, right or justify"))?;
    }
    for (key, destination) in [("width", &mut spec.width), ("height", &mut spec.height)] {
        match args.get(key) {
            None => {}
            Some(Value::Null) => *destination = None,
            Some(_) => *destination = number(args, key, 1.0, 30_000.0)?,
        }
    }
    if let Some(value) = number(args, "rotation", -360_000.0, 360_000.0)? {
        spec.rotation = value;
    }
    for (key, destination) in [
        ("scale_x", &mut spec.scale_x),
        ("scale_y", &mut spec.scale_y),
    ] {
        if let Some(value) = number(args, key, -1000.0, 1000.0)? {
            if value == 0.0 {
                return Err(error(format!("{key} cannot be zero")));
            }
            *destination = value;
        }
    }
    if let Some(value) = args.get("anti_alias") {
        spec.anti_alias = match value.as_str() {
            Some("smooth") => AntiAliasMode::Smooth,
            Some("crisp") => AntiAliasMode::Crisp,
            Some("strong") => AntiAliasMode::Strong,
            Some("none") => AntiAliasMode::None,
            _ => return Err(error("anti_alias must be smooth, crisp, strong or none")),
        };
    }
    if let Some(value) = args.get("warp") {
        let value = value
            .as_object()
            .ok_or_else(|| error("warp must be an object"))?;
        if let Some(style) = value.get("style") {
            spec.warp.style = match style.as_str() {
                Some("none") => WarpStyle::None,
                Some("arc") => WarpStyle::Arc,
                Some("bulge") => WarpStyle::Bulge,
                Some("flag") => WarpStyle::Flag,
                _ => return Err(error("warp.style must be none, arc, bulge or flag")),
            };
        }
        for (key, destination) in [
            ("bend", &mut spec.warp.bend),
            ("horizontal", &mut spec.warp.horizontal),
            ("vertical", &mut spec.warp.vertical),
        ] {
            if let Some(raw) = value.get(key) {
                let parsed = raw
                    .as_f64()
                    .filter(|number| number.is_finite() && (-100.0..=100.0).contains(number))
                    .ok_or_else(|| error(format!("warp.{key} must be from -100 to 100")))?;
                *destination = parsed as f32;
            }
        }
    }
    Ok(spec.sanitized())
}

fn text_target(editor: &Editor, id: NodeId) -> Result<TextSpec, ToolResult> {
    match &editor
        .doc
        .node(id)
        .ok_or_else(|| error(format!("no node {id}")))?
        .kind
    {
        NodeKind::Text { spec, .. } => Ok((**spec).clone()),
        _ => Err(error(format!("node {id} is not editable text"))),
    }
}

fn char_to_byte(text: &str, index: usize) -> Option<usize> {
    if index == text.chars().count() {
        Some(text.len())
    } else {
        text.char_indices().nth(index).map(|(byte, _)| byte)
    }
}

fn byte_to_char(text: &str, byte: usize) -> usize {
    text[..byte.min(text.len())].chars().count()
}

pub(crate) fn execute(
    editor: &mut Editor,
    name: &str,
    args: &Value,
) -> Result<ToolResult, ToolResult> {
    let id = args
        .get("node")
        .and_then(Value::as_u64)
        .ok_or_else(|| error("node must be an integer"))?;
    let mut spec = text_target(editor, id)?;
    match name {
        "format_text_range" => {
            let style = StyleEdit::parse(args)?;
            if style.is_empty() {
                return Err(error("provide at least one character style to change"));
            }
            let range = match (args.get("start"), args.get("end")) {
                (None, None) => 0..spec.text.len(),
                (Some(start), Some(end)) => {
                    let start = start
                        .as_u64()
                        .and_then(|value| usize::try_from(value).ok())
                        .ok_or_else(|| error("start must be a non-negative character offset"))?;
                    let end = end
                        .as_u64()
                        .and_then(|value| usize::try_from(value).ok())
                        .ok_or_else(|| error("end must be a non-negative character offset"))?;
                    if start >= end {
                        return Err(error("start must be less than end"));
                    }
                    let start = char_to_byte(&spec.text, start)
                        .ok_or_else(|| error("start exceeds the text length"))?;
                    let end = char_to_byte(&spec.text, end)
                        .ok_or_else(|| error("end exceeds the text length"))?;
                    start..end
                }
                _ => return Err(error("provide both start and end, or omit both")),
            };
            spec.apply_style(range, |current| style.apply(current));
        }
        "set_text_path" => {
            let mode = args
                .get("mode")
                .and_then(Value::as_str)
                .ok_or_else(|| error("mode must be follow, inside or none"))?;
            if mode == "none" {
                spec.text_path = None;
            } else {
                let from_node = args.get("path_node");
                let from_svg = args.get("d");
                if from_node.is_some() == from_svg.is_some() {
                    return Err(error("provide exactly one of path_node or d"));
                }
                let mut path = if let Some(value) = from_node {
                    let path_id = value
                        .as_u64()
                        .ok_or_else(|| error("path_node must be an integer"))?;
                    match &editor
                        .doc
                        .node(path_id)
                        .ok_or_else(|| error(format!("no node {path_id}")))?
                        .kind
                    {
                        NodeKind::Path { path, .. } => (**path).clone(),
                        _ => return Err(error("path_node must identify an editable vector path")),
                    }
                } else {
                    Path::from_svg(
                        from_svg
                            .and_then(Value::as_str)
                            .ok_or_else(|| error("d must be SVG path data"))?,
                    )
                    .map_err(|message| error(format!("bad path data: {message}")))?
                };
                if path.anchor_count() == 0 {
                    return Err(error("the text path is empty"));
                }
                let path_mode = match mode {
                    "follow" => TextPathMode::Follow,
                    "inside" => TextPathMode::Inside,
                    _ => return Err(error("mode must be follow, inside or none")),
                };
                if path_mode == TextPathMode::Inside
                    && !path.subpaths.iter().any(|subpath| subpath.closed)
                {
                    return Err(error("inside text requires a closed path"));
                }
                path.transform(spec.transform().inverse());
                spec.text_path = Some(
                    TextPath {
                        path,
                        mode: path_mode,
                        offset: number(args, "offset", -1_000_000.0, 1_000_000.0)?.unwrap_or(0.0),
                        flip: boolean(args, "flip")?.unwrap_or(false),
                        inset: number(args, "inset", 0.0, 10_000.0)?.unwrap_or(0.0),
                    }
                    .sanitized(),
                );
            }
        }
        _ => return Err(error(format!("unknown text tool {name}"))),
    }
    let before = text_target(editor, id)?;
    if before == spec {
        return Ok(ToolResult::text("Nothing to change"));
    }
    editor
        .execute(Command::SetText {
            id,
            spec: Box::new(spec),
        })
        .map_err(|message| error(message.to_string()))?;
    Ok(ToolResult::text(format!("Updated editable text node {id}")))
}

fn color_json(color: [u8; 4]) -> String {
    let [r, g, b, a] = color;
    if a == 255 {
        format!("#{r:02x}{g:02x}{b:02x}")
    } else {
        format!("#{r:02x}{g:02x}{b:02x}{a:02x}")
    }
}

fn style_json(style: &TextStyle) -> Value {
    json!({
        "font": style.font, "size": style.size, "color": color_json(style.color),
        "bold": style.bold, "italic": style.italic,
        "letter_spacing": style.letter_spacing, "baseline": style.baseline
    })
}

pub(crate) fn text_json(spec: &TextSpec) -> Value {
    let mut value = json!({
        "text": spec.text, "x": spec.x, "y": spec.y,
        "vertical": spec.vertical, "rotation": spec.rotation,
        "scale_x": spec.scale_x, "scale_y": spec.scale_y,
        "align": spec.align.key(), "width": spec.width, "height": spec.height,
        "line_height": spec.line_height,
        "anti_alias": match spec.anti_alias { AntiAliasMode::Smooth=>"smooth", AntiAliasMode::Crisp=>"crisp", AntiAliasMode::Strong=>"strong", AntiAliasMode::None=>"none" },
        "style": style_json(&spec.base_style()),
        "runs": spec.runs.iter().map(|run| json!({
            "start":byte_to_char(&spec.text,run.start), "end":byte_to_char(&spec.text,run.end),
            "style":style_json(&run.style)
        })).collect::<Vec<_>>(),
        "warp": {"style":match spec.warp.style {WarpStyle::None=>"none",WarpStyle::Arc=>"arc",WarpStyle::Bulge=>"bulge",WarpStyle::Flag=>"flag"},
            "bend":spec.warp.bend,"horizontal":spec.warp.horizontal,"vertical":spec.warp.vertical}
    });
    if let Some(path) = &spec.text_path {
        value["text_path"] = json!({
            "mode":if path.mode==TextPathMode::Follow{"follow"}else{"inside"},
            "d":path.path.to_svg(),"offset":path.offset,"flip":path.flip,"inset":path.inset
        });
    } else {
        value["text_path"] = Value::Null;
    }
    value
}

#[cfg(test)]
mod tests {
    use super::*;
    use emulsion_core::Document;

    fn add_text(editor: &mut Editor, text: &str) -> NodeId {
        let result = crate::exec::execute(
            editor,
            "add_text",
            &json!({
                "text":text,"x":20,"y":30,"width":180,"height":90,
                "vertical":false,"align":"justify","anti_alias":"crisp",
                "warp":{"style":"arc","bend":20,"horizontal":5,"vertical":-5}
            }),
        );
        assert!(!result.is_error, "{result:?}");
        editor.doc.nodes.last().unwrap().id
    }

    #[test]
    fn paragraph_effects_range_styles_and_describe_are_lossless_and_undoable() {
        let mut editor = Editor::new(Document::new(400, 300), None);
        let id = add_text(&mut editor, "A😀BC");
        let result = crate::exec::execute(
            &mut editor,
            "format_text_range",
            &json!({"node":id,"start":1,"end":2,"color":"#ff000080","size":72,"baseline":4}),
        );
        assert!(!result.is_error, "{result:?}");
        let NodeKind::Text { spec, .. } = &editor.doc.node(id).unwrap().kind else {
            panic!()
        };
        assert_eq!(spec.runs.len(), 1);
        assert_eq!(spec.runs[0].start..spec.runs[0].end, 1..5);
        assert_eq!(spec.runs[0].style.color, [255, 0, 0, 128]);
        let description = crate::exec::describe(&editor);
        let text = description["nodes"]
            .as_array()
            .unwrap()
            .iter()
            .find(|node| node["id"] == id)
            .unwrap();
        assert_eq!(text["runs"][0]["start"], 1);
        assert_eq!(text["runs"][0]["end"], 2);
        assert_eq!(text["height"], 90.0);
        assert_eq!(text["warp"]["style"], "arc");
        assert!(editor.undo());
        let NodeKind::Text { spec, .. } = &editor.doc.node(id).unwrap().kind else {
            panic!()
        };
        assert!(spec.runs.is_empty());
    }

    #[test]
    fn path_text_accepts_follow_and_closed_inside_and_rejects_invalid_atomically() {
        let mut editor = Editor::new(Document::new(400, 300), None);
        let text_id = add_text(&mut editor, "Follow this path");
        let result = crate::exec::execute(
            &mut editor,
            "draw_path",
            &json!({"name":"Open","d":"M 20 120 C 120 20 260 20 360 120","stroke":"none","fill":"none"}),
        );
        assert!(!result.is_error, "{result:?}");
        let open_id = editor.doc.nodes.last().unwrap().id;
        let result = crate::exec::execute(
            &mut editor,
            "set_text_path",
            &json!({"node":text_id,"mode":"follow","path_node":open_id,"offset":12,"flip":true}),
        );
        assert!(!result.is_error, "{result:?}");
        let before = editor.doc.clone();
        let history = editor.history.len();
        let result = crate::exec::execute(
            &mut editor,
            "set_text_path",
            &json!({"node":text_id,"mode":"inside","path_node":open_id}),
        );
        assert!(result.is_error);
        assert_eq!(editor.doc, before);
        assert_eq!(editor.history.len(), history);
        let result = crate::exec::execute(
            &mut editor,
            "set_text_path",
            &json!({"node":text_id,"mode":"inside","d":"M 50 50 L 350 50 L 350 250 L 50 250 Z","inset":10}),
        );
        assert!(!result.is_error, "{result:?}");
        let NodeKind::Text { spec, .. } = &editor.doc.node(text_id).unwrap().kind else {
            panic!()
        };
        assert_eq!(spec.text_path.as_ref().unwrap().mode, TextPathMode::Inside);
        assert!(editor.undo());
        assert!(editor.undo());
        let NodeKind::Text { spec, .. } = &editor.doc.node(text_id).unwrap().kind else {
            panic!()
        };
        assert!(spec.text_path.is_none());
    }

    #[test]
    fn invalid_or_locked_text_edits_never_change_history() {
        let mut editor = Editor::new(Document::new(200, 100), None);
        let id = add_text(&mut editor, "Locked");
        for args in [
            json!({"node":id,"vertical":"yes"}),
            json!({"node":id,"scale_x":0}),
            json!({"node":id,"anti_alias":"maybe"}),
            json!({"node":id,"warp":{"bend":101}}),
        ] {
            let before = editor.doc.clone();
            let history = editor.history.len();
            let result = crate::exec::execute(&mut editor, "set_text", &args);
            assert!(result.is_error, "{args}");
            assert_eq!(editor.doc, before);
            assert_eq!(editor.history.len(), history);
        }
        assert!(
            !crate::exec::execute(&mut editor, "set_lock", &json!({"node":id,"locked":true}))
                .is_error
        );
        let before = editor.doc.clone();
        let result = crate::exec::execute(
            &mut editor,
            "format_text_range",
            &json!({"node":id,"start":0,"end":1,"bold":true}),
        );
        assert!(result.is_error);
        assert_eq!(editor.doc, before);
    }
}
