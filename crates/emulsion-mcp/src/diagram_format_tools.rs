//! Diagram-specific formatting maps semantic groups to native paths and labels.
use crate::{ToolDef, ToolResult};
use emulsion_core::{Command, Editor, NodeKind};
use serde::Deserialize;
use serde_json::{Value, json};
use std::collections::BTreeSet;
pub(crate) const READ_ONLY: &[&str] = &["get_diagram_formatting"];
pub(crate) const DESTRUCTIVE: &[&str] = &["set_diagram_formatting"];
pub(crate) fn definitions() -> Vec<ToolDef> {
    let nodes = json!({"type":"array","minItems":1,"maxItems":1024,"uniqueItems":true,"items":{"type":"integer","minimum":1}});
    let color = json!({"type":"array","minItems":4,"maxItems":4,"items":{"type":"integer","minimum":0,"maximum":255}});
    vec![ToolDef{name:"get_diagram_formatting".into(),description:"Inspect native path paint, connector dash/width and editable label typography by semantic diagram shape/connector group IDs.".into(),input_schema:json!({"type":"object","additionalProperties":false,"properties":{"nodes":nodes},"required":["nodes"]})},ToolDef{name:"set_diagram_formatting".into(),description:"Patch selected diagram shapes/connectors with native fill/line/text colors, stroke width/dashes and label typography. Omitted fields stay unchanged; fill:null/stroke:null remove paint. Connector fills are untouched and arrowheads follow the native line style. One atomic Undo; locked descendants reject the batch.".into(),input_schema:json!({"type":"object","additionalProperties":false,"properties":{"nodes":nodes,"fill":{"oneOf":[color.clone(),{"type":"null"}]},"stroke":{"oneOf":[color.clone(),{"type":"null"}]},"text_color":color,"stroke_width":{"type":"number","minimum":0,"maximum":1000},"dash":{"type":"array","maxItems":6,"items":{"type":"number","exclusiveMinimum":0,"maximum":10000}},"dash_offset":{"type":"number","minimum":-100000,"maximum":100000},"font":{"type":"string","maxLength":512},"font_size":{"type":"number","minimum":1,"maximum":4000},"bold":{"type":"boolean"},"italic":{"type":"boolean"},"align":{"enum":["left","center","right","justify"]}},"required":["nodes"]})}]
}
#[derive(Deserialize)]
#[serde(deny_unknown_fields)]
struct Request {
    nodes: Vec<u64>,
    fill: Option<[u8; 4]>,
    stroke: Option<[u8; 4]>,
    text_color: Option<[u8; 4]>,
    stroke_width: Option<f32>,
    dash: Option<Vec<f32>>,
    dash_offset: Option<f32>,
    font: Option<String>,
    font_size: Option<f32>,
    bold: Option<bool>,
    italic: Option<bool>,
    align: Option<String>,
}
pub(crate) fn execute(editor: &mut Editor, name: &str, args: &Value) -> Option<ToolResult> {
    if !READ_ONLY.contains(&name) && !DESTRUCTIVE.contains(&name) {
        return None;
    }
    Some(match run(editor, name, args) {
        Ok(v) => ToolResult::text(v.to_string()),
        Err(e) => ToolResult::error(e),
    })
}
fn run(editor: &mut Editor, name: &str, args: &Value) -> Result<Value, String> {
    let request: Request = serde_json::from_value(args.clone()).map_err(|e| e.to_string())?;
    if args.as_object().is_some_and(|o| {
        o.iter()
            .any(|(k, v)| v.is_null() && k != "fill" && k != "stroke")
    }) {
        return Err("Omit unchanged fields; only fill and stroke may be null.".into());
    }
    if request.nodes.is_empty()
        || request.nodes.len() > 1024
        || request.nodes.contains(&0)
        || request.nodes.iter().collect::<BTreeSet<_>>().len() != request.nodes.len()
    {
        return Err("Choose 1–1024 distinct diagram IDs.".into());
    }
    let model = editor
        .doc
        .diagram
        .as_ref()
        .ok_or("Open a native diagram.")?;
    let mut paths = BTreeSet::new();
    let mut labels = BTreeSet::new();
    let mut connector_paths = BTreeSet::new();
    for id in &request.nodes {
        if let Some(shape) = model.shapes.get(id) {
            paths.insert(shape.body);
            labels.insert(shape.label);
        } else if let Some(edge) = model.edges.get(id) {
            paths.insert(edge.path);
            connector_paths.insert(edge.path);
            labels.insert(edge.label);
        } else {
            return Err("Node is not a diagram shape or connector group.".into());
        }
    }
    if name == "get_diagram_formatting" {
        if args.as_object().unwrap().keys().any(|k| k != "nodes") {
            return Err("Formatting inspection accepts only nodes.".into());
        }
    } else {
        if request
            .stroke_width
            .is_some_and(|v| !v.is_finite() || !(0. ..=1000.).contains(&v))
            || request
                .font_size
                .is_some_and(|v| !v.is_finite() || !(1. ..=4000.).contains(&v))
            || request
                .dash_offset
                .is_some_and(|v| !v.is_finite() || v.abs() > 100000.)
            || request.dash.as_ref().is_some_and(|d| {
                d.len() > 6 || d.iter().any(|v| !v.is_finite() || *v <= 0. || *v > 10000.)
            })
            || request
                .font
                .as_ref()
                .is_some_and(|v| v.len() > 512 || v.chars().any(char::is_control))
        {
            return Err("Diagram formatting values are outside native limits.".into());
        }
        let align = match request.align.as_deref() {
            None => None,
            Some("left") => Some(emulsion_core::text::Align::Left),
            Some("center") => Some(emulsion_core::text::Align::Center),
            Some("right") => Some(emulsion_core::text::Align::Right),
            Some("justify") => Some(emulsion_core::text::Align::Justify),
            _ => return Err("Unknown label alignment.".into()),
        };
        let fill = args.get("fill").map(|_| request.fill);
        let stroke = args.get("stroke").map(|_| request.stroke);
        let mut trial = Editor::try_new(editor.doc.clone(), None).map_err(|e| e.to_string())?;
        for id in &paths {
            let Some(NodeKind::Path { path, style, .. }) = trial.doc.node(*id).map(|n| &n.kind)
            else {
                continue;
            };
            let path = path.clone();
            let mut style = *style;
            if let Some(fill) = fill
                && !connector_paths.contains(id)
            {
                style.fill = fill;
                style.fill_paint = emulsion_raster::vector::PathPaint::Solid;
            }
            if let Some(stroke) = stroke {
                style.stroke = stroke;
                style.stroke_paint = emulsion_raster::vector::PathPaint::Solid;
            }
            if let Some(w) = request.stroke_width {
                style.width = w;
            }
            if let Some(dash) = &request.dash {
                style.dash = [0.; 6];
                style.dash[..dash.len()].copy_from_slice(dash);
                style.dash_count = dash.len() as u8;
            }
            if let Some(offset) = request.dash_offset {
                style.dash_offset = offset;
            }
            trial
                .execute(Command::SetPath {
                    id: *id,
                    path,
                    style,
                })
                .map_err(|e| e.to_string())?;
        }
        for id in &labels {
            let Some(NodeKind::Text { spec, .. }) = trial.doc.node(*id).map(|n| &n.kind) else {
                continue;
            };
            let mut spec = (**spec).clone();
            if let Some(c) = request.text_color {
                spec.color = c;
            }
            if let Some(font) = &request.font {
                spec.font = font.clone();
            }
            if let Some(size) = request.font_size {
                spec.size = size;
            }
            if let Some(bold) = request.bold {
                spec.bold = bold;
            }
            if let Some(italic) = request.italic {
                spec.italic = italic;
            }
            if let Some(align) = align {
                spec.align = align;
            }
            spec.apply_style(0..spec.text.len(), |style| {
                if let Some(c) = request.text_color {
                    style.color = c;
                }
                if let Some(font) = &request.font {
                    style.font = font.clone();
                }
                if let Some(size) = request.font_size {
                    style.size = size;
                }
                if let Some(bold) = request.bold {
                    style.bold = bold;
                }
                if let Some(italic) = request.italic {
                    style.italic = italic;
                }
            });
            trial
                .execute(Command::SetText {
                    id: *id,
                    spec: Box::new(spec),
                })
                .map_err(|e| e.to_string())?;
        }
        editor.commit_design_document(trial.doc, "Format diagram selection")?;
    }
    Ok(
        json!({"paths":paths.into_iter().filter_map(|id|match &editor.doc.node(id)?.kind{NodeKind::Path{style,..}=>Some(json!({"node":id,"style":style})),_=>None}).collect::<Vec<_>>(),"labels":labels.into_iter().filter_map(|id|match &editor.doc.node(id)?.kind{NodeKind::Text{spec,..}=>Some(json!({"node":id,"font":spec.font,"size":spec.size,"color":spec.color,"bold":spec.bold,"italic":spec.italic,"align":spec.align})),_=>None}).collect::<Vec<_>>()}),
    )
}
#[cfg(test)]
mod tests {
    use super::*;
    use emulsion_core::{
        Document,
        diagram::{self, ShapeKind},
    };
    #[test]
    fn diagram_formatting_preserves_connections_and_atomic_undo() {
        let mut e = Editor::new(Document::new(500, 300), None);
        let a = diagram::add_shape(&mut e, ShapeKind::Process, [10., 10., 100., 60.], "A").unwrap();
        let b =
            diagram::add_shape(&mut e, ShapeKind::Process, [220., 10., 100., 60.], "B").unwrap();
        let before = e.doc.clone();
        let result=execute(&mut e,"set_diagram_formatting",&json!({"nodes":[a,b],"fill":[10,20,30,128],"stroke_width":3,"dash":[8,5],"font_size":22,"bold":true})).unwrap();
        assert!(!result.is_error, "{:?}", result.content);
        let body = e.doc.diagram.as_ref().unwrap().shapes[&a].body;
        assert!(
            matches!(&e.doc.node(body).unwrap().kind,NodeKind::Path{style,..}if style.fill==Some([10,20,30,128])&&style.dash_count==2)
        );
        e.undo();
        assert_eq!(e.doc, before);
        e.execute(Command::SetLocked {
            id: b,
            locked: true,
        })
        .unwrap();
        let before = e.doc.clone();
        assert!(
            execute(
                &mut e,
                "set_diagram_formatting",
                &json!({"nodes":[a,b],"fill":null})
            )
            .unwrap()
            .is_error
        );
        assert_eq!(e.doc, before);
        assert!(
            execute(
                &mut e,
                "set_diagram_formatting",
                &json!({"nodes":[a],"stroke_width":null})
            )
            .unwrap()
            .is_error
        );
    }
}
