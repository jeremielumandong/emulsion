//! Shared, lossless native shape style schema and validation for MCP tools.
use crate::server::ToolResult;
use emulsion_raster::vector::{
    PathPaint, PathStyle, PatternKind, StrokeAlignment, StrokeCap, StrokeJoin,
};
use serde_json::{Value, json};

fn error(message: impl Into<String>) -> ToolResult {
    ToolResult::error(message)
}

fn number(value: &Value, name: &str, min: f32, max: f32) -> Result<f32, ToolResult> {
    let n = value
        .as_f64()
        .ok_or_else(|| error(format!("{name} must be a number")))?;
    if !n.is_finite() || n < min as f64 || n > max as f64 {
        return Err(error(format!("{name} must be between {min} and {max}")));
    }
    Ok(n as f32)
}

fn color(value: &Value, name: &str) -> Result<[u8; 4], ToolResult> {
    let value = value
        .as_str()
        .ok_or_else(|| error(format!("{name} must be a hex color")))?;
    let hex = value
        .strip_prefix('#')
        .ok_or_else(|| error(format!("{name} must be #RRGGBB or #RRGGBBAA")))?;
    if !matches!(hex.len(), 6 | 8) || !hex.bytes().all(|b| b.is_ascii_hexdigit()) {
        return Err(error(format!("{name} must be #RRGGBB or #RRGGBBAA")));
    }
    let mut rgba = [255; 4];
    for (i, channel) in rgba.iter_mut().enumerate().take(hex.len() / 2) {
        *channel = u8::from_str_radix(&hex[i * 2..i * 2 + 2], 16).unwrap();
    }
    Ok(rgba)
}

fn paint(value: &Value, name: &str) -> Result<PathPaint, ToolResult> {
    let fields = value
        .as_object()
        .ok_or_else(|| error(format!("{name} must be a paint object")))?;
    let kind = value
        .get("kind")
        .and_then(Value::as_str)
        .ok_or_else(|| error(format!("{name}.kind is required")))?;
    let allowed: &[&str] = match kind {
        "solid" => &["kind"],
        "linear_gradient" => &["kind", "end", "angle"],
        "radial_gradient" => &["kind", "end"],
        "pattern" => &["kind", "end", "pattern", "size"],
        _ => {
            return Err(error(format!(
                "{name}.kind must be solid, linear_gradient, radial_gradient, or pattern"
            )));
        }
    };
    if let Some(key) = fields.keys().find(|key| !allowed.contains(&key.as_str())) {
        return Err(error(format!("{name}.{key} is not valid for {kind}")));
    }
    if kind == "solid" {
        return Ok(PathPaint::Solid);
    }
    let end = color(
        value
            .get("end")
            .ok_or_else(|| error(format!("{name}.end is required")))?,
        &format!("{name}.end"),
    )?;
    Ok(match kind {
        "linear_gradient" => PathPaint::LinearGradient {
            end,
            angle: value
                .get("angle")
                .map(|v| number(v, &format!("{name}.angle"), -360000.0, 360000.0))
                .transpose()?
                .unwrap_or(0.0)
                .rem_euclid(360.0),
        },
        "radial_gradient" => PathPaint::RadialGradient { end },
        _ => PathPaint::Pattern {
            kind: match value.get("pattern").and_then(Value::as_str) {
                Some("checker") => PatternKind::Checker,
                Some("stripes") => PatternKind::Stripes,
                Some("dots") => PatternKind::Dots,
                _ => {
                    return Err(error(format!(
                        "{name}.pattern must be checker, stripes, or dots"
                    )));
                }
            },
            secondary: end,
            size: value
                .get("size")
                .map(|v| number(v, &format!("{name}.size"), 1.0, 4096.0))
                .transpose()?
                .unwrap_or(16.0),
        },
    })
}

/// Validate a complete edit before the caller creates any command or history entry.
pub(crate) fn parse_style(args: &Value, mut style: PathStyle) -> Result<PathStyle, ToolResult> {
    if !args.is_object() {
        return Err(error("shape arguments must be an object"));
    }
    for (name, channel) in [("fill", &mut style.fill), ("stroke", &mut style.stroke)] {
        if let Some(v) = args.get(name) {
            *channel = if v.as_str() == Some("none") {
                None
            } else {
                Some(color(v, name)?)
            };
        }
    }
    for (name, destination) in [
        ("fill_paint", &mut style.fill_paint),
        ("stroke_paint", &mut style.stroke_paint),
    ] {
        if let Some(v) = args.get(name) {
            *destination = paint(v, name)?;
        }
    }
    for (name, destination, min, max) in [
        ("width", &mut style.width, 0.0, 500.0),
        ("miter_limit", &mut style.miter_limit, 1.0, 100.0),
        ("dash_offset", &mut style.dash_offset, -100000.0, 100000.0),
    ] {
        if let Some(v) = args.get(name) {
            *destination = number(v, name, min, max)?;
        }
    }
    if let Some(v) = args.get("stroke_alignment") {
        style.alignment = match v.as_str() {
            Some("center") => StrokeAlignment::Center,
            Some("inside") => StrokeAlignment::Inside,
            Some("outside") => StrokeAlignment::Outside,
            _ => return Err(error("stroke_alignment must be center, inside, or outside")),
        };
    }
    if let Some(v) = args.get("cap") {
        style.cap = match v.as_str() {
            Some("butt") => StrokeCap::Butt,
            Some("round") => StrokeCap::Round,
            Some("square") => StrokeCap::Square,
            _ => return Err(error("cap must be butt, round, or square")),
        };
    }
    if let Some(v) = args.get("join") {
        style.join = match v.as_str() {
            Some("miter") => StrokeJoin::Miter,
            Some("round") => StrokeJoin::Round,
            Some("bevel") => StrokeJoin::Bevel,
            _ => return Err(error("join must be miter, round, or bevel")),
        };
    }
    if let Some(v) = args.get("dashes") {
        let values = v
            .as_array()
            .ok_or_else(|| error("dashes must be an array"))?;
        if values.len() > 6 {
            return Err(error("dashes accepts at most six dash/gap lengths"));
        }
        style.dash = [0.0; 6];
        style.dash_count = values.len() as u8;
        for (i, v) in values.iter().enumerate() {
            let n = number(v, "dash/gap length", 0.0, 10000.0)?;
            if n > 0.0 && n < 0.25 {
                return Err(error(
                    "dash/gap lengths must be zero or at least 0.25 pixels",
                ));
            }
            style.dash[i] = n;
        }
        if !values.is_empty() && style.dash.iter().all(|n| *n == 0.0) {
            return Err(error("a nonempty dashes array needs a positive length"));
        }
    }
    Ok(style)
}

fn color_json(color: Option<[u8; 4]>) -> Value {
    match color {
        Some([r, g, b, 255]) => json!(format!("#{r:02x}{g:02x}{b:02x}")),
        Some([r, g, b, a]) => json!(format!("#{r:02x}{g:02x}{b:02x}{a:02x}")),
        None => json!("none"),
    }
}

fn paint_json(paint: PathPaint) -> Value {
    match paint {
        PathPaint::Solid => json!({"kind":"solid"}),
        PathPaint::LinearGradient { end, angle } => {
            json!({"kind":"linear_gradient","end":color_json(Some(end)),"angle":angle})
        }
        PathPaint::RadialGradient { end } => {
            json!({"kind":"radial_gradient","end":color_json(Some(end))})
        }
        PathPaint::Pattern {
            kind,
            secondary,
            size,
        } => {
            json!({"kind":"pattern","end":color_json(Some(secondary)),"pattern":match kind {PatternKind::Checker=>"checker",PatternKind::Stripes=>"stripes",PatternKind::Dots=>"dots"},"size":size})
        }
    }
}

pub(crate) fn style_json(style: &PathStyle) -> Value {
    json!({
        "fill":color_json(style.fill),"stroke":color_json(style.stroke),"width":style.width,
        "fill_paint":paint_json(style.fill_paint),"stroke_paint":paint_json(style.stroke_paint),
        "stroke_alignment":match style.alignment {StrokeAlignment::Center=>"center",StrokeAlignment::Inside=>"inside",StrokeAlignment::Outside=>"outside"},
        "cap":match style.cap {StrokeCap::Butt=>"butt",StrokeCap::Round=>"round",StrokeCap::Square=>"square"},
        "join":match style.join {StrokeJoin::Miter=>"miter",StrokeJoin::Round=>"round",StrokeJoin::Bevel=>"bevel"},
        "miter_limit":style.miter_limit,"dashes":style.dash[..usize::from(style.dash_count.min(6))],"dash_offset":style.dash_offset
    })
}

pub(crate) fn style_properties() -> Value {
    let color = json!({"type":"string","pattern":"^#[0-9a-fA-F]{6}([0-9a-fA-F]{2})?$"});
    let paint = json!({"oneOf":[
        {"type":"object","additionalProperties":false,"required":["kind"],"properties":{"kind":{"const":"solid"}}},
        {"type":"object","additionalProperties":false,"required":["kind","end"],"properties":{"kind":{"const":"linear_gradient"},"end":color,"angle":{"type":"number","minimum":-360000,"maximum":360000,"default":0}}},
        {"type":"object","additionalProperties":false,"required":["kind","end"],"properties":{"kind":{"const":"radial_gradient"},"end":color}},
        {"type":"object","additionalProperties":false,"required":["kind","end","pattern"],"properties":{"kind":{"const":"pattern"},"end":color,"pattern":{"enum":["checker","stripes","dots"]},"size":{"type":"number","minimum":1,"maximum":4096,"default":16}}}
    ],"description":"Native editable paint. Base color is fill/stroke; end is the second color. Replaces paint configuration; omitted paint preserves existing configuration."});
    json!({
        "fill":{"type":"string","pattern":"^(none|#[0-9a-fA-F]{6}([0-9a-fA-F]{2})?)$","description":"Fill base color, including optional alpha, or none to disable."},
        "stroke":{"type":"string","pattern":"^(none|#[0-9a-fA-F]{6}([0-9a-fA-F]{2})?)$","description":"Stroke base color, including optional alpha, or none to disable."},
        "width":{"type":"number","minimum":0,"maximum":500,"description":"Stroke width in document pixels (not shape width)."},
        "fill_paint":paint,"stroke_paint":paint,
        "stroke_alignment":{"enum":["center","inside","outside"]},
        "cap":{"enum":["butt","round","square"]},
        "join":{"enum":["miter","round","bevel"]},
        "miter_limit":{"type":"number","minimum":1,"maximum":100},
        "dashes":{"type":"array","maxItems":6,"items":{"anyOf":[{"const":0},{"type":"number","minimum":0.25,"maximum":10000}]},"description":"Alternating dash/gap lengths. Empty means solid; nonempty requires a positive length. Zero dashes with round caps make dots."},
        "dash_offset":{"type":"number","minimum":-100000,"maximum":100000}
    })
}

#[cfg(test)]
mod tests {
    use super::*;
    use emulsion_core::{Document, Editor, NodeKind};

    #[test]
    fn styles_round_trip_and_partial_color_edits_preserve_paint() {
        let base = PathStyle::default();
        assert_eq!(parse_style(&json!({}), base).unwrap(), base);
        for paint in [
            json!({"kind":"solid"}),
            json!({"kind":"linear_gradient","end":"#abcdef80","angle":32}),
            json!({"kind":"radial_gradient","end":"#00000000"}),
            json!({"kind":"pattern","pattern":"dots","end":"#ffffff","size":8}),
        ] {
            let style = parse_style(&json!({"fill":"#12345680","fill_paint":paint,"stroke_paint":paint,"stroke_alignment":"inside","cap":"butt","join":"bevel","dashes":[0,4],"dash_offset":-3}),base).unwrap();
            assert_eq!(parse_style(&style_json(&style), base).unwrap(), style);
            let edited = parse_style(&json!({"fill":"#ff0000"}), style).unwrap();
            assert_eq!(edited.fill_paint, style.fill_paint);
            assert_eq!(edited.fill, Some([255, 0, 0, 255]));
        }
    }

    #[test]
    fn draw_and_edit_native_paints_are_undoable_and_invalid_edits_are_atomic() {
        let mut editor = Editor::new(Document::new(64, 64), None);
        let result = crate::exec::execute(
            &mut editor,
            "draw_path",
            &json!({"d":"M 8 8 L 56 8 L 56 56 L 8 56 Z","fill":"#ff000080","fill_paint":{"kind":"linear_gradient","end":"#0000ff","angle":45},"stroke":"#ffffff","stroke_alignment":"outside","cap":"square","join":"miter","miter_limit":8,"dashes":[4,2],"dash_offset":2}),
        );
        assert!(!result.is_error, "{result:?}");
        let id = editor.doc.nodes.last().unwrap().id;
        let NodeKind::Path { style: before, .. } = editor.doc.node(id).unwrap().kind else {
            panic!()
        };
        assert_eq!(before.fill, Some([255, 0, 0, 128]));
        assert!(matches!(
            before.fill_paint,
            PathPaint::LinearGradient { .. }
        ));
        assert_eq!(before.alignment, StrokeAlignment::Outside);
        let result = crate::exec::execute(
            &mut editor,
            "set_path",
            &json!({"node":id,"stroke_paint":{"kind":"pattern","pattern":"stripes","end":"#00ff0080","size":12},"cap":"butt"}),
        );
        assert!(!result.is_error, "{result:?}");
        let NodeKind::Path { style: edited, .. } = editor.doc.node(id).unwrap().kind else {
            panic!()
        };
        assert_eq!(edited.fill_paint, before.fill_paint);
        assert!(matches!(
            edited.stroke_paint,
            PathPaint::Pattern {
                kind: PatternKind::Stripes,
                ..
            }
        ));
        let steps = editor.history.len();
        for invalid in [
            json!({"width":"wide"}),
            json!({"fill":"#😀😀"}),
            json!({"fill":null}),
            json!({"width":501}),
            json!({"cap":"projecting"}),
            json!({"join":"invalid"}),
            json!({"stroke_alignment":"bad"}),
            json!({"miter_limit":0}),
            json!({"dashes":[0,0]}),
            json!({"dashes":[0.01,2]}),
            json!({"dashes":[1,2,3,4,5,6,7]}),
            json!({"dash_offset":100001}),
            json!({"fill_paint":{"kind":"linear_gradient","end":"none"}}),
            json!({"fill_paint":{"kind":"pattern","pattern":"dots","end":"#ffffff","size":0}}),
            json!({"fill_paint":{"kind":"solid","angle":5}}),
        ] {
            let mut args = invalid.clone();
            args["node"] = json!(id);
            args["d"] = json!("M 1 1 L 2 2");
            let result = crate::exec::execute(&mut editor, "set_path", &args);
            assert!(result.is_error, "accepted {invalid}");
            assert_eq!(editor.history.len(), steps);
            let NodeKind::Path { style, .. } = editor.doc.node(id).unwrap().kind else {
                panic!()
            };
            assert_eq!(style, edited);
            args["d"] = json!("M 1 1 L 2 2");
            assert!(
                crate::exec::execute(&mut editor, "draw_path", &args).is_error,
                "draw accepted {invalid}"
            );
            assert_eq!(editor.history.len(), steps);
        }
        assert!(editor.undo());
        let NodeKind::Path { style, .. } = editor.doc.node(id).unwrap().kind else {
            panic!()
        };
        assert_eq!(style, before);
    }
}
