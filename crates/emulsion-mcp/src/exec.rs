//! Run tools against an open document.

use crate::server::ToolResult;
use base64::Engine as _;
use emulsion_core::command::Slot;
use emulsion_core::{Command, Document, Editor, Node, NodeId, NodeKind};
use emulsion_raster::composite::{flatten, level_size};
use emulsion_raster::{Adjustment, BlendMode, Placement};
use serde_json::{Map, Value, json};

fn err(msg: impl Into<String>) -> ToolResult {
    ToolResult::error(msg)
}

fn id_arg(args: &Value, key: &str) -> Result<NodeId, ToolResult> {
    args.get(key)
        .and_then(Value::as_u64)
        .ok_or_else(|| err(format!("missing integer argument '{key}'")))
}

fn node_label(doc: &Document, id: NodeId) -> String {
    doc.node(id)
        .map(|n| format!("{} (#{id})", n.name))
        .unwrap_or_else(|| format!("#{id}"))
}

/// Run `name` with `args` against `editor`. Every change goes through the
/// Command API, so it lands in history like a person's edit.
pub fn execute(editor: &mut Editor, name: &str, args: &Value) -> ToolResult {
    match run(editor, name, args) {
        Ok(r) | Err(r) => r,
    }
}

fn exec(editor: &mut Editor, cmd: Command) -> Result<Option<NodeId>, ToolResult> {
    editor.execute(cmd).map_err(|e| err(e.to_string()))
}

fn run(editor: &mut Editor, name: &str, args: &Value) -> Result<ToolResult, ToolResult> {
    match name {
        "describe_document" => Ok(ToolResult::text(
            serde_json::to_string_pretty(&describe(editor)).unwrap_or_default(),
        )),
        "get_view" => view(&editor.doc, args),
        "set_visibility" => {
            let id = id_arg(args, "node")?;
            let visible = args
                .get("visible")
                .and_then(Value::as_bool)
                .ok_or_else(|| err("missing boolean 'visible'"))?;
            exec(editor, Command::SetVisible { id, visible })?;
            Ok(ToolResult::text(format!(
                "{} {}",
                if visible { "Showed" } else { "Hid" },
                node_label(&editor.doc, id)
            )))
        }
        "rename_node" => {
            let id = id_arg(args, "node")?;
            let new = args
                .get("name")
                .and_then(Value::as_str)
                .ok_or_else(|| err("missing string 'name'"))?;
            let old = node_label(&editor.doc, id);
            exec(
                editor,
                Command::Rename {
                    id,
                    name: new.to_string(),
                },
            )?;
            Ok(ToolResult::text(format!("Renamed {old} to {new}")))
        }
        "set_opacity" => {
            let id = id_arg(args, "node")?;
            let o = args
                .get("opacity")
                .and_then(Value::as_f64)
                .ok_or_else(|| err("missing number 'opacity'"))?;
            exec(
                editor,
                Command::SetOpacity {
                    id,
                    opacity: (o / 100.0) as f32,
                },
            )?;
            Ok(ToolResult::text(format!(
                "Set {} opacity to {o:.0}%",
                node_label(&editor.doc, id)
            )))
        }
        "set_blend_mode" => {
            let id = id_arg(args, "node")?;
            let m = args
                .get("mode")
                .and_then(Value::as_str)
                .ok_or_else(|| err("missing string 'mode'"))?;
            let blend = parse_blend(m).ok_or_else(|| err(format!("unknown blend mode '{m}'")))?;
            exec(editor, Command::SetBlend { id, blend })?;
            Ok(ToolResult::text(format!(
                "Set {} blend mode to {}",
                node_label(&editor.doc, id),
                blend.label()
            )))
        }
        "move_node" => {
            let id = id_arg(args, "node")?;
            let n = editor
                .doc
                .node(id)
                .ok_or_else(|| err(format!("no node {id}")))?
                .clone();
            let sib_without = |doc: &Document, parent: Option<NodeId>| -> Vec<NodeId> {
                doc.children(parent)
                    .into_iter()
                    .filter(|s| *s != id)
                    .collect()
            };
            let slot = if let Some(a) = args.get("above").and_then(Value::as_u64) {
                let t = editor
                    .doc
                    .node(a)
                    .ok_or_else(|| err(format!("no node {a}")))?;
                let sib = sib_without(&editor.doc, t.parent);
                Slot {
                    parent: t.parent,
                    index: sib.iter().position(|s| *s == a).unwrap_or(0) + 1,
                }
            } else if let Some(b) = args.get("below").and_then(Value::as_u64) {
                let t = editor
                    .doc
                    .node(b)
                    .ok_or_else(|| err(format!("no node {b}")))?;
                let sib = sib_without(&editor.doc, t.parent);
                Slot {
                    parent: t.parent,
                    index: sib.iter().position(|s| *s == b).unwrap_or(0),
                }
            } else if let Some(g) = args.get("into_group").and_then(Value::as_u64) {
                Slot::top_of(Some(g))
            } else {
                match args.get("to").and_then(Value::as_str) {
                    Some("top") => Slot::top_of(n.parent),
                    Some("bottom") => Slot {
                        parent: n.parent,
                        index: 0,
                    },
                    _ => return Err(err("give one of above, below, into_group, or to")),
                }
            };
            exec(editor, Command::MoveNode { id, slot })?;
            Ok(ToolResult::text(format!(
                "Moved {}",
                node_label(&editor.doc, id)
            )))
        }
        "group_nodes" => {
            let ids: Vec<NodeId> = args
                .get("nodes")
                .and_then(Value::as_array)
                .map(|a| a.iter().filter_map(Value::as_u64).collect())
                .unwrap_or_default();
            let name = args
                .get("name")
                .and_then(Value::as_str)
                .unwrap_or("Group")
                .to_string();
            let g = exec(editor, Command::Group { ids, name })?.unwrap_or_default();
            Ok(ToolResult::text(format!(
                "Created group {}",
                node_label(&editor.doc, g)
            )))
        }
        "ungroup" => {
            let id = id_arg(args, "node")?;
            let l = node_label(&editor.doc, id);
            exec(editor, Command::Ungroup { id })?;
            Ok(ToolResult::text(format!("Ungrouped {l}")))
        }
        "delete_node" => {
            let id = id_arg(args, "node")?;
            let l = node_label(&editor.doc, id);
            exec(editor, Command::RemoveNode { id })?;
            Ok(ToolResult::text(format!("Deleted {l}")))
        }
        "duplicate_node" => {
            let id = id_arg(args, "node")?;
            let new = exec(editor, Command::DuplicateNode { id })?.unwrap_or_default();
            Ok(ToolResult::text(format!(
                "Duplicated as {}",
                node_label(&editor.doc, new)
            )))
        }
        "add_adjustment" => {
            let kind = args
                .get("kind")
                .and_then(Value::as_str)
                .ok_or_else(|| err("missing 'kind'"))?;
            let mut adj =
                adjustment(kind).ok_or_else(|| err(format!("unknown adjustment '{kind}'")))?;
            if let Some(p) = args.get("params").and_then(Value::as_object) {
                apply_params(&mut adj, p)?;
            }
            let mut node = Node::adjust(0, adj);
            if let Some(n) = args.get("name").and_then(Value::as_str) {
                node.name = n.to_string();
            }
            let slot = match args.get("above").and_then(Value::as_u64) {
                Some(a) => {
                    let t = editor
                        .doc
                        .node(a)
                        .ok_or_else(|| err(format!("no node {a}")))?;
                    let sib = editor.doc.children(t.parent);
                    Slot {
                        parent: t.parent,
                        index: sib.iter().position(|s| *s == a).unwrap_or(0) + 1,
                    }
                }
                None => Slot::TOP,
            };
            let id = exec(
                editor,
                Command::AddNode {
                    node: Box::new(node),
                    slot,
                },
            )?
            .unwrap_or_default();
            Ok(ToolResult::text(format!(
                "Added {}",
                node_label(&editor.doc, id)
            )))
        }
        "set_adjustment" => {
            let id = id_arg(args, "node")?;
            let params = args
                .get("params")
                .and_then(Value::as_object)
                .ok_or_else(|| err("missing object 'params'"))?;
            let NodeKind::Adjust(a) = &editor
                .doc
                .node(id)
                .ok_or_else(|| err(format!("no node {id}")))?
                .kind
            else {
                return Err(err(format!("node {id} is not an adjustment")));
            };
            let mut a = a.clone();
            apply_params(&mut a, params)?;
            exec(editor, Command::SetAdjustment { id, adjustment: a })?;
            Ok(ToolResult::text(format!(
                "Updated {}",
                node_label(&editor.doc, id)
            )))
        }
        "set_transform" => {
            let id = id_arg(args, "node")?;
            let NodeKind::Raster { raster, placement } = &editor
                .doc
                .node(id)
                .ok_or_else(|| err(format!("no node {id}")))?
                .kind
            else {
                return Err(err(format!("node {id} has no pixels to place")));
            };
            let (w, h) = (raster.width() as f64, raster.height() as f64);
            let mut p: Placement = *placement;
            if let Some(s) = args.get("scale").and_then(Value::as_f64) {
                let s = (s / 100.0).max(0.0001);
                p.scale_x = s * p.scale_x.signum();
                p.scale_y = s * p.scale_y.signum();
            }
            let _ = (w, h);
            if let Some(x) = args.get("x").and_then(Value::as_f64) {
                p.x = x;
            }
            if let Some(y) = args.get("y").and_then(Value::as_f64) {
                p.y = y;
            }
            if let Some(r) = args.get("rotation").and_then(Value::as_f64) {
                p.rotation = r;
            }
            if let Some(f) = args.get("flip_x").and_then(Value::as_bool) {
                p.flip_x = f;
            }
            if let Some(f) = args.get("flip_y").and_then(Value::as_bool) {
                p.flip_y = f;
            }
            exec(editor, Command::SetPlacement { id, placement: p })?;
            Ok(ToolResult::text(format!(
                "Placed {}",
                node_label(&editor.doc, id)
            )))
        }
        "undo" => Ok(ToolResult::text(if editor.undo() {
            "Undid the last step"
        } else {
            "Nothing to undo"
        })),
        "redo" => Ok(ToolResult::text(if editor.redo() {
            "Redid the last step"
        } else {
            "Nothing to redo"
        })),
        other => Err(err(format!("unknown tool '{other}'"))),
    }
}

fn parse_blend(s: &str) -> Option<BlendMode> {
    let s = s.trim().to_ascii_lowercase().replace(['_', '-'], " ");
    if s == "pass through" {
        return Some(BlendMode::PassThrough);
    }
    BlendMode::MENU
        .iter()
        .flatten()
        .copied()
        .find(|m| m.label() == s)
}

pub fn adjustment(kind: &str) -> Option<Adjustment> {
    let want = match kind {
        "exposure" => "Exposure",
        "brightness_contrast" => "Brightness / Contrast",
        "levels" => "Levels",
        "hue_saturation" => "Hue / Saturation",
        "white_balance" => "White balance",
        "invert" => "Invert",
        _ => return None,
    };
    Adjustment::catalogue()
        .into_iter()
        .find(|a| a.label() == want)
}

fn apply_params(adj: &mut Adjustment, params: &Map<String, Value>) -> Result<(), ToolResult> {
    for (k, v) in params {
        let v = v
            .as_f64()
            .ok_or_else(|| err(format!("parameter '{k}' must be a number")))?;
        // Accept "warmth" for white balance temperature.
        let key = if k == "warmth" {
            "temperature"
        } else {
            k.as_str()
        };
        if !adj.set_param(key, v as f32) {
            let keys: Vec<&str> = adj.params().iter().map(|s| s.key).collect();
            return Err(err(format!(
                "{} has no parameter '{k}' (valid: {})",
                adj.label(),
                keys.join(", ")
            )));
        }
    }
    Ok(())
}

/// A model-friendly snapshot of the document.
pub fn describe(editor: &Editor) -> Value {
    let doc = &editor.doc;
    let mut rows = Vec::new();
    fn walk(doc: &Document, parent: Option<NodeId>, depth: usize, rows: &mut Vec<(NodeId, usize)>) {
        for id in doc.children(parent).into_iter().rev() {
            rows.push((id, depth));
            walk(doc, Some(id), depth + 1, rows);
        }
    }
    walk(doc, None, 0, &mut rows);
    let nodes: Vec<Value> = rows
        .iter()
        .enumerate()
        .map(|(i, (id, depth))| {
            let n = doc.node(*id).expect("row");
            let mut v = json!({
                "id": n.id,
                "row": i + 1,
                "depth": depth,
                "name": n.name,
                "kind": match &n.kind {
                    NodeKind::Raster { .. } => "pixels",
                    NodeKind::Group { .. } => "group",
                    NodeKind::Adjust(_) => "adjustment",
                    NodeKind::Fill { .. } => "fill",
                },
                "visible": n.visible,
                "opacity": (n.opacity * 100.0).round(),
                "blend": n.blend.label(),
            });
            let o = v.as_object_mut().unwrap();
            if let Some(p) = n.parent {
                o.insert("parent".into(), json!(p));
            }
            if let Some(c) = n.clip_to {
                o.insert("clipped_to".into(), json!(c));
            }
            if n.mask.is_some() {
                o.insert("mask".into(), json!(if n.mask_enabled { "on" } else { "off" }));
            }
            if n.locked {
                o.insert("locked".into(), json!(true));
            }
            match &n.kind {
                NodeKind::Adjust(a) => {
                    o.insert("adjustment".into(), json!(a.label()));
                    let params: Map<String, Value> = a.params().into_iter().map(|s| (s.key.to_string(), json!(s.value))).collect();
                    o.insert("params".into(), Value::Object(params));
                }
                NodeKind::Raster { raster, placement } => {
                    o.insert("pixels".into(), json!(format!("{}×{}", raster.width(), raster.height())));
                    o.insert(
                        "placement".into(),
                        json!({ "x": placement.x, "y": placement.y, "scale": placement.scale_x.abs() * 100.0, "rotation": placement.rotation }),
                    );
                }
                NodeKind::Fill { rgba } => {
                    o.insert("color".into(), json!(format!("#{:02X}{:02X}{:02X}", rgba[0], rgba[1], rgba[2])));
                }
                NodeKind::Group { .. } => {}
            }
            v
        })
        .collect();
    let history: Vec<&str> = editor
        .history
        .steps()
        .take(5)
        .map(|s| s.name.as_str())
        .collect();
    json!({
        "canvas": { "width": doc.width, "height": doc.height },
        "rows": "row 1 is the top of the stack; depth > 0 means inside the group listed above it",
        "nodes": nodes,
        "recent_history": history,
    })
}

/// Render the composite (or one node) as a base64 PNG image block.
pub fn view(doc: &Document, args: &Value) -> Result<ToolResult, ToolResult> {
    let max = args
        .get("max_size")
        .and_then(Value::as_u64)
        .unwrap_or(1024)
        .clamp(64, 1568) as u32;
    let mut d = doc.clone();
    if let Some(id) = args.get("node").and_then(Value::as_u64) {
        if d.node(id).is_none() {
            return Err(err(format!("no node {id}")));
        }
        let keep = d.subtree(id);
        let snapshot = d.clone();
        for n in &mut d.nodes {
            if !(keep.contains(&n.id) || snapshot.is_ancestor(n.id, id)) {
                n.visible = false;
            }
        }
    }
    let tree = d.composite_tree();
    let mut level = 0;
    while {
        let (w, h) = level_size(d.width, d.height, level);
        w.max(h) > max * 2
    } {
        level += 1;
    }
    let flat = flatten(&tree, level);
    let img = image::RgbaImage::from_raw(flat.width(), flat.height(), flat.to_srgba8())
        .ok_or_else(|| err("render failed"))?;
    let s = (max as f64 / img.width().max(img.height()) as f64).min(1.0);
    let (w, h) = (
        ((img.width() as f64 * s).round() as u32).max(1),
        ((img.height() as f64 * s).round() as u32).max(1),
    );
    let img = if (w, h) == img.dimensions() {
        img
    } else {
        image::imageops::resize(&img, w, h, image::imageops::FilterType::Triangle)
    };
    let png = emulsion_io::export::png8(w, h, img.as_raw()).map_err(|e| err(e.to_string()))?;
    let data = base64::engine::general_purpose::STANDARD.encode(png);
    Ok(ToolResult {
        content: vec![
            json!({ "type": "image", "data": data, "mimeType": "image/png" }),
            json!({ "type": "text", "text": format!("{w}×{h} view of a {}×{} document", doc.width, doc.height) }),
        ],
        is_error: false,
    })
}

#[cfg(test)]
mod tests {
    use super::*;
    use emulsion_raster::Raster;
    use std::sync::Arc;

    fn editor() -> Editor {
        let mut d = Document::new(200, 100);
        for name in ["bottom", "middle", "top"] {
            Command::AddNode {
                node: Box::new(Node::raster(
                    0,
                    name,
                    Arc::new(Raster::solid(200, 100, [0.2, 0.3, 0.4, 1.0])),
                    Placement::default(),
                )),
                slot: Slot::TOP,
            }
            .apply(&mut d)
            .unwrap();
        }
        Editor::new(d, None)
    }

    fn text(r: &ToolResult) -> String {
        r.content[0]["text"]
            .as_str()
            .unwrap_or_default()
            .to_string()
    }

    #[test]
    fn describe_lists_top_first() {
        let e = editor();
        let v = describe(&e);
        assert_eq!(v["nodes"][0]["name"], "top");
        assert_eq!(v["nodes"][0]["row"], 1);
        assert_eq!(v["nodes"][2]["name"], "bottom");
    }

    #[test]
    fn deliverable_request_as_tool_calls() {
        // "hide the top two nodes and rename the third to Sky"
        let mut e = editor();
        let ids: Vec<u64> = describe(&e)["nodes"]
            .as_array()
            .unwrap()
            .iter()
            .map(|n| n["id"].as_u64().unwrap())
            .collect();
        e.begin("Assistant: hide the top two…");
        for id in &ids[..2] {
            let r = execute(
                &mut e,
                "set_visibility",
                &json!({ "node": id, "visible": false }),
            );
            assert!(!r.is_error, "{}", text(&r));
        }
        let r = execute(
            &mut e,
            "rename_node",
            &json!({ "node": ids[2], "name": "Sky" }),
        );
        assert!(!r.is_error);
        e.end();
        assert_eq!(e.history.len(), 1, "one undo step for the whole turn");
        assert_eq!(e.doc.node(ids[2]).unwrap().name, "Sky");
        e.undo();
        assert!(e.doc.nodes.iter().all(|n| n.visible));
        assert_eq!(e.doc.node(ids[2]).unwrap().name, "bottom");
    }

    #[test]
    fn adjustments_and_bad_params() {
        let mut e = editor();
        let r = execute(
            &mut e,
            "add_adjustment",
            &json!({ "kind": "white_balance", "params": { "warmth": 30 } }),
        );
        assert!(!r.is_error, "{}", text(&r));
        let id = e.doc.nodes.last().unwrap().id;
        let r = execute(
            &mut e,
            "set_adjustment",
            &json!({ "node": id, "params": { "nope": 1 } }),
        );
        assert!(r.is_error && text(&r).contains("valid: temperature, tint"));
    }

    #[test]
    fn view_returns_png_image_block() {
        let e = editor();
        let r = view(&e.doc, &json!({ "max_size": 128 })).unwrap();
        assert_eq!(r.content[0]["type"], "image");
        let png = base64::engine::general_purpose::STANDARD
            .decode(r.content[0]["data"].as_str().unwrap())
            .unwrap();
        let img = image::load_from_memory(&png).unwrap();
        assert_eq!((img.width(), img.height()), (128, 64));
    }

    #[test]
    fn errors_do_not_change_the_document() {
        let mut e = editor();
        let before = e.doc.clone();
        let r = execute(
            &mut e,
            "set_blend_mode",
            &json!({ "node": 999, "mode": "multiply" }),
        );
        assert!(r.is_error);
        assert_eq!(e.doc, before);
    }
}
