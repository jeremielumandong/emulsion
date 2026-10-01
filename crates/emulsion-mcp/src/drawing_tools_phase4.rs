//! Storyboard drawing tools for agents: the gap-closing paint bucket with
//! fill modes, the cutter (selection or lasso to a new layer), perspective
//! and envelope distortion, and Drawing Assist guides (including 4- and
//! 5-point curvilinear perspective, the ruler and saved guide sets). Each
//! edit is one Undo step; guides are drawing aids outside Undo.
use crate::project_tools::validate_schema;
use crate::{ToolDef, ToolResult};
use emulsion_core::bucket::{BucketOptions, bucket_fill};
use emulsion_core::cutter::cut_to_new_layer;
use emulsion_core::distort::{
    DistortKind, Distortion, MAX_ENVELOPE, distort_area, distort_command,
};
use emulsion_core::drawing_guides::{DrawingGuides, GuideKind, MAX_GUIDES, Ruler};
use emulsion_core::{Editor, NodeId};
use emulsion_raster::gap_fill::{FillMode, MAX_GAP};
use emulsion_raster::select;
use serde::Deserialize;
use serde_json::{Value, json};

pub(crate) const READ_ONLY: &[&str] = &["describe_drawing_guides"];

fn def(name: &str, description: &str, properties: Value, required: &[&str]) -> ToolDef {
    ToolDef {
        name: name.into(),
        description: description.into(),
        input_schema: json!({"type":"object","additionalProperties":false,"properties":properties,"required":required}),
    }
}

fn node() -> Value {
    json!({"type":"integer","minimum":1,"description":"Pixel or vector stroke layer ID (from describe_document)."})
}

fn coord() -> Value {
    json!({"type":"number","minimum":-1000000,"maximum":1000000})
}

fn xy() -> Value {
    json!({"type":"object","additionalProperties":false,"required":["x","y"],"properties":{"x":coord(),"y":coord()}})
}

fn guide() -> Value {
    json!({"type":"object","additionalProperties":false,"required":["kind"],"properties":{
        "kind":{"type":"string","enum":["grid","isometric","perspective","curvilinear"]},
        "size":{"type":"number","exclusiveMinimum":0,"maximum":100000,"description":"grid/isometric: spacing in pixels."},
        "points":{"type":"array","minItems":1,"maxItems":3,"items":xy(),"description":"perspective: 1–3 vanishing points (may lie off the canvas)."},
        "center":xy(),
        "radius":{"type":"number","exclusiveMinimum":0,"maximum":1000000,"description":"curvilinear: distance from the centre to the four vanishing points."},
        "five":{"type":"boolean","description":"curvilinear: add the fifth (central) point; default false (4-point)."}
    }})
}

pub(crate) fn definitions() -> Vec<ToolDef> {
    vec![
        def(
            "bucket_fill",
            "Paint-bucket fill at a canvas point on a pixel or vector stroke layer, within the selection. close_gaps treats openings in line art up to that many pixels as closed so the fill does not leak. On a vector stroke layer the filled area becomes a vector fill under the strokes. One Undo step.",
            json!({
                "node":node(),"x":coord(),"y":coord(),
                "color":{"type":"string","minLength":7,"maxLength":9,"description":"#RRGGBB or #RRGGBBAA."},
                "tolerance":{"type":"integer","minimum":0,"maximum":255,"description":"Per-channel colour difference allowed (default 32)."},
                "contiguous":{"type":"boolean","description":"Only the connected area (default true)."},
                "close_gaps":{"type":"integer","minimum":0,"maximum":MAX_GAP,"description":"Gap size in pixels treated as closed (default 0)."},
                "mode":{"type":"string","enum":["normal","behind","unpainted"],"description":"normal paints over; behind paints under the layer's pixels; unpainted fills only pixels whose alpha is below threshold."},
                "threshold":{"type":"integer","minimum":1,"maximum":255,"description":"Alpha below which unpainted fills (default 128)."},
                "sample":{"type":"string","enum":["all","layer"],"description":"Find the area on every visible layer (default) or on this layer only."}
            }),
            &["node", "x", "y", "color"],
        ),
        def(
            "cut_to_new_layer",
            "The cutter: move (or with copy, copy) the selected part of a pixel or vector stroke layer into a new layer just above it at the same position. Uses the current selection, or the lasso polygon given here. Vector strokes split where they cross the edge and stay vector. One Undo step; returns the new layer's ID.",
            json!({
                "node":node(),
                "copy":{"type":"boolean","description":"Copy instead of cut (default false)."},
                "lasso":{"type":"array","minItems":3,"maxItems":5000,"items":xy(),"description":"Polygon to cut instead of the current selection."}
            }),
            &["node"],
        ),
        def(
            "distort_layer",
            "Distort the selected pixels (or the whole layer without a selection) of a pixel or vector stroke layer, in place. perspective drags the 4 corners; envelope bends a lattice of cells×cells cells. The lattice starts as a regular grid over the selection's bounds (or the layer's content); `moves` sets where lattice points go, by row-major index (perspective: 0 top-left, 1 top-right, 2 bottom-left, 3 bottom-right). Returns the source rectangle and final lattice. One Undo step.",
            json!({
                "node":node(),
                "kind":{"type":"string","enum":["perspective","envelope"]},
                "cells":{"type":"integer","minimum":1,"maximum":MAX_ENVELOPE,"description":"envelope: cells per side (default 3)."},
                "moves":{"type":"array","minItems":1,"maxItems":81,"items":{"type":"object","additionalProperties":false,"required":["index","x","y"],"properties":{"index":{"type":"integer","minimum":0,"maximum":80},"x":coord(),"y":coord()}}}
            }),
            &["node", "kind", "moves"],
        ),
        def(
            "describe_drawing_guides",
            "The page's Drawing Assist guides, ruler and saved guide sets.",
            json!({}),
            &[],
        ),
        def(
            "set_drawing_guides",
            "Change the page's Drawing Assist guides (strokes drawn with assist snap to them; each storyboard panel keeps its own). guides replaces those shown: grid, isometric, 1–3 point perspective, or 4-/5-point curvilinear (fish-eye). ruler places a straight edge strokes snap to (null removes it). save_set stores the guides shown under a name; switch_set shows a saved set; delete_set removes one. Not part of Undo.",
            json!({
                "guides":{"type":"array","minItems":0,"maxItems":MAX_GUIDES,"items":guide()},
                "ruler":{"oneOf":[{"type":"null"},{"type":"object","additionalProperties":false,"required":["a","b"],"properties":{"a":xy(),"b":xy(),"enabled":{"type":"boolean"}}}]},
                "save_set":{"type":"string","minLength":1,"maxLength":100},
                "switch_set":{"type":"string","minLength":1,"maxLength":100},
                "delete_set":{"type":"string","minLength":1,"maxLength":100}
            }),
            &[],
        ),
    ]
}

/// Run one of these tools, or `None` when `name` is not one.
pub(crate) fn execute(editor: &mut Editor, name: &str, args: &Value) -> Option<ToolResult> {
    let def = definitions().into_iter().find(|d| d.name == name)?;
    let result = validate_schema(&def.input_schema, args).and_then(|()| run(editor, name, args));
    Some(match result {
        Ok(value) => ToolResult::text(value.to_string()),
        Err(error) => ToolResult::error(error),
    })
}

#[derive(Deserialize)]
struct XY {
    x: f64,
    y: f64,
}

#[derive(Deserialize)]
struct GuideArg {
    kind: String,
    size: Option<f64>,
    points: Option<Vec<XY>>,
    center: Option<XY>,
    radius: Option<f64>,
    #[serde(default)]
    five: bool,
}

#[derive(Deserialize)]
struct MoveArg {
    index: usize,
    x: f64,
    y: f64,
}

fn parse<T: for<'de> Deserialize<'de>>(value: &Value, what: &str) -> Result<T, String> {
    serde_json::from_value(value.clone()).map_err(|e| format!("Invalid {what}: {e}"))
}

fn parse_color(hex: &str) -> Result<[u8; 4], String> {
    let bad = || format!("Bad colour {hex:?}; use #RRGGBB or #RRGGBBAA");
    let digits = hex.strip_prefix('#').ok_or_else(bad)?;
    if !matches!(digits.len(), 6 | 8) || !digits.chars().all(|c| c.is_ascii_hexdigit()) {
        return Err(bad());
    }
    let byte = |i: usize| u8::from_str_radix(&digits[i..i + 2], 16).map_err(|_| bad());
    Ok([
        byte(0)?,
        byte(2)?,
        byte(4)?,
        if digits.len() == 8 { byte(6)? } else { 255 },
    ])
}

fn node_id(args: &Value) -> Result<NodeId, String> {
    args["node"]
        .as_u64()
        .ok_or_else(|| "node is required".into())
}

fn guide_kind(g: GuideArg) -> Result<GuideKind, String> {
    let need = |v: Option<f64>, what: &str| v.ok_or_else(|| format!("{} needs {what}", g.kind));
    Ok(match g.kind.as_str() {
        "grid" => GuideKind::Grid {
            size: need(g.size, "size")?,
        },
        "isometric" => GuideKind::Isometric {
            size: need(g.size, "size")?,
        },
        "perspective" => GuideKind::Perspective {
            points: g
                .points
                .as_ref()
                .ok_or("perspective needs points")?
                .iter()
                .map(|p| (p.x, p.y))
                .collect(),
        },
        "curvilinear" => {
            let c = g.center.as_ref().ok_or("curvilinear needs center")?;
            GuideKind::Curvilinear {
                center: (c.x, c.y),
                radius: need(g.radius, "radius")?,
                five: g.five,
            }
        }
        other => return Err(format!("Unknown guide kind {other:?}")),
    })
}

fn describe_guides(g: &DrawingGuides) -> Value {
    json!({
        "guides": serde_json::to_value(&g.guides).unwrap_or_default(),
        "ruler": serde_json::to_value(g.ruler).unwrap_or_default(),
        "sets": g.sets.iter().map(|s| json!({"name":s.name,"guides":serde_json::to_value(&s.guides).unwrap_or_default()})).collect::<Vec<_>>(),
        "active_set": g.active_set.map(|i| g.sets[i].name.clone()),
    })
}

fn run(editor: &mut Editor, name: &str, args: &Value) -> Result<Value, String> {
    match name {
        "bucket_fill" => {
            let id = node_id(args)?;
            let d = BucketOptions::default();
            let options = BucketOptions {
                tolerance: args["tolerance"].as_u64().map_or(d.tolerance, |v| v as u8),
                contiguous: args["contiguous"].as_bool().unwrap_or(d.contiguous),
                gap: args["close_gaps"].as_u64().map_or(0, |v| v as u32),
                mode: args
                    .get("mode")
                    .map(|m| parse::<FillMode>(m, "mode"))
                    .transpose()?
                    .unwrap_or_default(),
                threshold: args["threshold"].as_u64().map_or(d.threshold, |v| v as u8),
                sample_all: args["sample"].as_str() != Some("layer"),
            };
            let color = parse_color(args["color"].as_str().unwrap_or_default())?;
            let at = (
                args["x"].as_f64().unwrap_or_default(),
                args["y"].as_f64().unwrap_or_default(),
            );
            match bucket_fill(&editor.doc, id, at, color, &options)? {
                Some(command) => {
                    editor.execute(command).map_err(|e| e.to_string())?;
                    Ok(json!({"filled": true, "node": id}))
                }
                None => Ok(json!({"filled": false, "node": id})),
            }
        }
        "cut_to_new_layer" => {
            let id = node_id(args)?;
            let copy = args["copy"].as_bool().unwrap_or(false);
            let selection = match args.get("lasso") {
                Some(points) => {
                    let pts: Vec<XY> = parse(points, "lasso")?;
                    let pts: Vec<(f32, f32)> =
                        pts.iter().map(|p| (p.x as f32, p.y as f32)).collect();
                    select::polygon(editor.doc.width, editor.doc.height, &pts)
                }
                None => (**editor
                    .doc
                    .selection
                    .as_ref()
                    .ok_or("Nothing is selected; select an area or pass lasso")?)
                .clone(),
            };
            let commands = cut_to_new_layer(&editor.doc, id, &selection, copy)?;
            let mut trial = editor.doc.clone();
            for c in &commands {
                c.clone().apply(&mut trial).map_err(|e| e.to_string())?;
            }
            editor.begin(if copy {
                "Copy to new layer"
            } else {
                "Cut to new layer"
            });
            let mut created = None;
            for c in commands {
                match editor.execute(c) {
                    Ok(Some(n)) => created = created.or(Some(n)),
                    Ok(None) => {}
                    Err(e) => {
                        editor.cancel();
                        return Err(e.to_string());
                    }
                }
            }
            editor.end();
            Ok(json!({"node": created, "source": id, "copied": copy}))
        }
        "distort_layer" => {
            let id = node_id(args)?;
            let kind = match args["kind"].as_str() {
                Some("perspective") => DistortKind::Perspective,
                _ => DistortKind::Envelope(args["cells"].as_u64().unwrap_or(3) as usize),
            };
            let selection = editor.doc.selection.clone();
            let area = distort_area(&editor.doc, id, selection.as_deref())
                .ok_or("Nothing to distort on that layer")?;
            let mut d = Distortion::new(area, kind);
            let moves: Vec<MoveArg> = parse(&args["moves"], "moves")?;
            for m in moves {
                if m.index >= d.grid.len() {
                    return Err(format!(
                        "Lattice point {} does not exist; there are {}",
                        m.index,
                        d.grid.len()
                    ));
                }
                d.move_handle(m.index, (m.x, m.y));
            }
            let command = distort_command(&editor.doc, id, selection.as_deref(), &d)?;
            editor.execute(command).map_err(|e| e.to_string())?;
            Ok(json!({
                "node": id,
                "source": {"x": area.x, "y": area.y, "width": area.w, "height": area.h},
                "lattice": d.grid.iter().map(|p| json!({"x": p.0, "y": p.1})).collect::<Vec<_>>(),
            }))
        }
        "describe_drawing_guides" => Ok(describe_guides(&editor.doc.drawing_guides)),
        "set_drawing_guides" => {
            let mut g = editor.doc.drawing_guides.clone();
            if let Some(list) = args.get("guides") {
                let list: Vec<GuideArg> = parse(list, "guides")?;
                g.guides = list.into_iter().map(guide_kind).collect::<Result<_, _>>()?;
                g.active_set = None;
            }
            if let Some(r) = args.get("ruler") {
                g.ruler = if r.is_null() {
                    None
                } else {
                    let (a, b): (XY, XY) = (parse(&r["a"], "ruler.a")?, parse(&r["b"], "ruler.b")?);
                    Some(Ruler {
                        a: (a.x, a.y),
                        b: (b.x, b.y),
                        enabled: r["enabled"].as_bool().unwrap_or(true),
                    })
                };
            }
            if let Some(name) = args["delete_set"].as_str() {
                let i = g
                    .sets
                    .iter()
                    .position(|s| s.name == name)
                    .ok_or_else(|| format!("No guide set {name:?}"))?;
                g.delete_set(i);
            }
            if let Some(name) = args["save_set"].as_str() {
                g.save_set(name)?;
            }
            if let Some(name) = args["switch_set"].as_str() {
                let i = g
                    .sets
                    .iter()
                    .position(|s| s.name == name)
                    .ok_or_else(|| format!("No guide set {name:?}"))?;
                g.switch_to(i);
            }
            g.validate()?;
            editor.doc.drawing_guides = g;
            Ok(describe_guides(&editor.doc.drawing_guides))
        }
        _ => Err(format!("Unknown tool {name}")),
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use emulsion_core::command::Slot;
    use emulsion_core::{Command, Document, Node, NodeKind};
    use emulsion_raster::strokes::{Stroke, StrokePoint, StrokeSet};
    use std::sync::Arc;

    fn call(editor: &mut Editor, name: &str, args: Value) -> Result<Value, String> {
        let r = execute(editor, name, &args).expect("a phase 4 tool");
        let text = serde_json::to_value(&r).unwrap();
        if text["isError"].as_bool() == Some(true) {
            return Err(text.to_string());
        }
        let body = text["content"][0]["text"].as_str().unwrap().to_string();
        Ok(serde_json::from_str(&body).unwrap())
    }

    fn ink(editor: &mut Editor) -> NodeId {
        let pt = StrokePoint::new;
        let set = StrokeSet {
            strokes: vec![Stroke {
                points: vec![
                    pt(10., 10.),
                    pt(50., 10.),
                    pt(50., 50.),
                    pt(10., 50.),
                    pt(10., 16.),
                ],
                ..Stroke::new([0, 0, 0, 255], 2.)
            }],
            fills: Vec::new(),
        };
        editor
            .execute(Command::AddNode {
                node: Box::new(Node::strokes(0, "Ink", Arc::new(set), 60, 60)),
                slot: Slot::TOP,
            })
            .unwrap()
            .unwrap()
    }

    #[test]
    fn agents_fill_cut_distort_and_set_guides() {
        let mut e = Editor::new(Document::new(60, 60), None);
        let id = ink(&mut e);
        let filled = call(
            &mut e,
            "bucket_fill",
            json!({"node":id,"x":30,"y":30,"color":"#ff0000","close_gaps":6}),
        )
        .unwrap();
        assert_eq!(filled["filled"], true);
        let NodeKind::Strokes { strokes, .. } = &e.doc.node(id).unwrap().kind else {
            panic!()
        };
        assert_eq!(
            strokes.fills.len(),
            1,
            "the 4 px opening was closed and the fill is vector"
        );
        let steps = e.history.len();
        let cut = call(
            &mut e,
            "cut_to_new_layer",
            json!({"node":id,"lasso":[{"x":0,"y":0},{"x":30,"y":0},{"x":30,"y":60},{"x":0,"y":60}]}),
        )
        .unwrap();
        assert_eq!(e.history.len(), steps + 1);
        let new = cut["node"].as_u64().unwrap();
        assert!(matches!(
            e.doc.node(new).unwrap().kind,
            NodeKind::Strokes { .. }
        ));
        let moved = call(
            &mut e,
            "distort_layer",
            json!({"node":new,"kind":"perspective","moves":[{"index":1,"x":40,"y":0}]}),
        )
        .unwrap();
        assert_eq!(moved["lattice"].as_array().unwrap().len(), 4);
        assert!(
            call(
                &mut e,
                "distort_layer",
                json!({"node":new,"kind":"envelope","cells":2,"moves":[{"index":9,"x":1,"y":1}]})
            )
            .is_err()
        );
        let g = call(
            &mut e,
            "set_drawing_guides",
            json!({"guides":[{"kind":"curvilinear","center":{"x":30,"y":30},"radius":40,"five":true}],
                   "ruler":{"a":{"x":0,"y":5},"b":{"x":60,"y":5}},"save_set":"Fish-eye"}),
        )
        .unwrap();
        assert_eq!(g["active_set"], "Fish-eye");
        assert_eq!(e.doc.drawing_guides.primary().label(), "5-point");
        call(&mut e, "set_drawing_guides", json!({"guides":[]})).unwrap();
        call(
            &mut e,
            "set_drawing_guides",
            json!({"switch_set":"Fish-eye"}),
        )
        .unwrap();
        assert_eq!(e.doc.drawing_guides.guides.len(), 1);
        let described = call(&mut e, "describe_drawing_guides", json!({})).unwrap();
        assert_eq!(described["sets"][0]["name"], "Fish-eye");
        assert!(
            call(
                &mut e,
                "set_drawing_guides",
                json!({"guides":[{"kind":"grid"}]})
            )
            .is_err()
        );
    }
}
