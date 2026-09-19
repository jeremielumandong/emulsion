//! Run tools against an open document.

use crate::server::ToolResult;
use base64::Engine as _;
use emulsion_core::command::Slot;
use emulsion_core::{Command, Document, Editor, Node, NodeId, NodeKind};
use emulsion_raster::composite::{flatten, level_size, region};
use emulsion_raster::select::{self, Combine};
use emulsion_raster::{Adjustment, BlendMode, Placement};
use emulsion_raster::{IRect, color, fill};
use serde_json::{Map, Value, json};
use std::sync::Arc;

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
    if crate::tools::HEAVY.contains(&name) {
        return match plan_heavy(&editor.doc, name, args) {
            Ok(planned) => apply(editor, planned),
            Err(e) => e,
        };
    }
    match run(editor, name, args) {
        Ok(r) | Err(r) => r,
    }
}

/// Commands computed for a heavy tool, and what to tell the model.
pub struct Planned {
    pub commands: Vec<Command>,
    pub message: String,
}

/// Apply planned commands on the thread that owns the document.
pub fn apply(editor: &mut Editor, p: Planned) -> ToolResult {
    for c in p.commands {
        if let Err(e) = editor.execute(c) {
            return err(e.to_string());
        }
    }
    ToolResult::text(p.message)
}

fn combine_arg(args: &Value) -> Combine {
    match args.get("mode").and_then(Value::as_str) {
        Some("add") => Combine::Add,
        Some("subtract") => Combine::Subtract,
        Some("intersect") => Combine::Intersect,
        _ => Combine::Replace,
    }
}

/// The command that combines `m` into the current selection.
fn selection_command(
    doc: &Document,
    m: emulsion_raster::Mask,
    combine: Combine,
    feather: f32,
) -> (Command, String) {
    let m = if feather > 0.5 {
        select::feather(&m, feather)
    } else {
        m
    };
    let combined = select::combine(doc.selection.as_deref(), &m, combine);
    let b = select::bounds(&combined);
    if b.is_empty() {
        (
            Command::SetSelection { selection: None },
            "The selection is now empty".into(),
        )
    } else {
        (
            Command::SetSelection {
                selection: Some(Arc::new(combined)),
            },
            format!("Selected {}×{} at {}, {}", b.w, b.h, b.x, b.y),
        )
    }
}

/// Compute a heavy tool against a document snapshot, on any thread.
pub fn plan_heavy(doc: &Document, name: &str, args: &Value) -> Result<Planned, ToolResult> {
    let (w, h) = (doc.width, doc.height);
    match name {
        "select_color" => {
            let x = args
                .get("x")
                .and_then(Value::as_f64)
                .ok_or_else(|| err("missing number 'x'"))?;
            let y = args
                .get("y")
                .and_then(Value::as_f64)
                .ok_or_else(|| err("missing number 'y'"))?;
            if x < 0.0 || y < 0.0 || x >= w as f64 || y >= h as f64 {
                return Err(err("(x, y) is outside the canvas"));
            }
            let tol = args
                .get("tolerance")
                .and_then(Value::as_u64)
                .unwrap_or(32)
                .min(255) as u8;
            let contiguous = args
                .get("contiguous")
                .and_then(Value::as_bool)
                .unwrap_or(true);
            let img: Vec<u8> = region(&doc.composite_tree(), IRect::new(0, 0, w as i32, h as i32))
                .into_iter()
                .flat_map(color::premul_to_srgba8)
                .collect();
            let m = select::by_color(&img, w, h, x as u32, y as u32, tol, contiguous);
            let (c, message) = selection_command(doc, m, combine_arg(args), 0.0);
            Ok(Planned {
                commands: vec![c],
                message,
            })
        }
        "content_aware_fill" => {
            let sel = doc
                .selection
                .clone()
                .ok_or_else(|| err("select the area to fill first"))?;
            let (raster, reg) = fill::content_aware_layer(&doc.composite_tree(), &sel)
                .ok_or_else(|| err("the selection is empty"))?;
            let node = Node::raster(
                0,
                "Content-aware fill",
                Arc::new(raster),
                Placement::at(reg.x as f64, reg.y as f64),
            );
            Ok(Planned {
                commands: vec![Command::AddNode { node: Box::new(node), slot: Slot::TOP }],
                message: "Filled the selection into a new node \"Content-aware fill\" at the top of the stack".into(),
            })
        }
        other => Err(err(format!("{other} is not a heavy tool"))),
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
        "select_rect" | "select_ellipse" => {
            let num = |k: &str| {
                args.get(k)
                    .and_then(Value::as_f64)
                    .ok_or_else(|| err(format!("missing number '{k}'")))
            };
            let (x, y, rw, rh) = (
                num("x")? as f32,
                num("y")? as f32,
                num("width")? as f32,
                num("height")? as f32,
            );
            let (w, h) = (editor.doc.width, editor.doc.height);
            let m = if name == "select_rect" {
                select::rect(w, h, x, y, rw, rh)
            } else {
                select::ellipse(w, h, x, y, rw, rh)
            };
            let feather = args.get("feather").and_then(Value::as_f64).unwrap_or(0.0) as f32;
            let (c, msg) = selection_command(&editor.doc, m, combine_arg(args), feather);
            exec(editor, c)?;
            Ok(ToolResult::text(msg))
        }
        "select_all" => {
            let (w, h) = (editor.doc.width, editor.doc.height);
            exec(
                editor,
                Command::SetSelection {
                    selection: Some(Arc::new(select::all(w, h))),
                },
            )?;
            Ok(ToolResult::text("Selected everything"))
        }
        "deselect" => {
            exec(editor, Command::SetSelection { selection: None })?;
            Ok(ToolResult::text("Nothing is selected"))
        }
        "invert_selection" => {
            let (w, h) = (editor.doc.width, editor.doc.height);
            let inv = match &editor.doc.selection {
                Some(s) => select::invert(s),
                None => select::all(w, h),
            };
            let (c, msg) = selection_command(&editor.doc, inv, Combine::Replace, 0.0);
            exec(editor, c)?;
            Ok(ToolResult::text(msg))
        }
        "modify_selection" => {
            let s = editor
                .doc
                .selection
                .clone()
                .ok_or_else(|| err("nothing is selected"))?;
            let mut m = (*s).clone();
            if let Some(g) = args.get("grow").and_then(Value::as_i64) {
                m = select::grow(&m, g.clamp(-500, 500) as i32);
            }
            if let Some(f) = args.get("feather").and_then(Value::as_f64) {
                m = select::feather(&m, f.clamp(0.0, 500.0) as f32);
            }
            let (c, msg) = selection_command(&editor.doc, m, Combine::Replace, 0.0);
            exec(editor, c)?;
            Ok(ToolResult::text(msg))
        }
        "fill_selection" => {
            let id = id_arg(args, "node")?;
            let hex = args
                .get("color")
                .and_then(Value::as_str)
                .ok_or_else(|| err("missing string 'color'"))?;
            let v = hex
                .strip_prefix('#')
                .filter(|h| h.len() == 6)
                .and_then(|h| u32::from_str_radix(h, 16).ok())
                .ok_or_else(|| err("color must be #RRGGBB"))?;
            let premul = color::srgba8_to_premul([(v >> 16) as u8, (v >> 8) as u8, v as u8, 255]);
            let node = editor
                .doc
                .node(id)
                .ok_or_else(|| err(format!("no node {id}")))?;
            let NodeKind::Raster { raster, placement } = &node.kind else {
                return Err(err(format!(
                    "{} has no pixels to fill",
                    node_label(&editor.doc, id)
                )));
            };
            let to_doc = placement.to_doc(raster.width(), raster.height());
            let sel = editor.doc.selection.clone();
            let cov = move |x: i32, y: i32| -> f32 {
                let Some(s) = &sel else { return 1.0 };
                let p = to_doc.transform_point2(glam::dvec2(x as f64 + 0.5, y as f64 + 0.5));
                if p.x < 0.0 || p.y < 0.0 || p.x >= s.width() as f64 || p.y >= s.height() as f64 {
                    return 0.0;
                }
                s.get(p.x as u32, p.y as u32) as f32 / 255.0
            };
            let (r, dirty) =
                emulsion_raster::paint::fill_color(raster, raster.bounds(), &cov, premul);
            exec(
                editor,
                Command::ReplacePixels {
                    id,
                    raster: Arc::new(r),
                    dirty,
                    label: "Fill".into(),
                },
            )?;
            Ok(ToolResult::text(format!(
                "Filled {} with {hex}",
                node_label(&editor.doc, id)
            )))
        }
        "crop" => {
            let int = |k: &str| {
                args.get(k)
                    .and_then(Value::as_i64)
                    .ok_or_else(|| err(format!("missing integer '{k}'")))
            };
            let (x, y, w, h) = (int("x")?, int("y")?, int("width")?, int("height")?);
            if w < 1 || h < 1 || w > 30000 || h > 30000 {
                return Err(err("width and height must be 1 to 30000"));
            }
            let rect = IRect::new(x as i32, y as i32, w as i32, h as i32);
            let rotation = args
                .get("rotation")
                .and_then(Value::as_f64)
                .unwrap_or(0.0)
                .clamp(-45.0, 45.0);
            exec(editor, Command::Crop { rect, rotation })?;
            Ok(ToolResult::text(format!(
                "Canvas is now {}×{}",
                editor.doc.width, editor.doc.height
            )))
        }
        "image_size" => {
            let width = args
                .get("width")
                .and_then(Value::as_u64)
                .ok_or_else(|| err("missing integer 'width'"))?
                .clamp(1, 30000) as u32;
            let height = ((width as f64 * editor.doc.height as f64 / editor.doc.width as f64)
                .round() as u32)
                .clamp(1, 30000);
            exec(editor, Command::ImageSize { width, height })?;
            Ok(ToolResult::text(format!("Image is now {width}×{height}")))
        }
        "list_history" => Ok(ToolResult::text(
            serde_json::to_string_pretty(&history_json(editor)).unwrap_or_default(),
        )),
        "create_branch" => {
            let name = args
                .get("name")
                .and_then(Value::as_str)
                .ok_or_else(|| err("missing string 'name'"))?;
            let r = match args.get("from_commit").and_then(Value::as_u64) {
                Some(c) => editor.branch_at(name, c),
                None => editor.branch(name),
            };
            r.map_err(|e| err(e.to_string()))?;
            Ok(ToolResult::text(format!(
                "Created branch {name} and switched to it"
            )))
        }
        "switch_branch" => {
            let name = args
                .get("name")
                .and_then(Value::as_str)
                .ok_or_else(|| err("missing string 'name'"))?;
            editor.checkout(name).map_err(|e| err(e.to_string()))?;
            Ok(ToolResult::text(format!("Switched to {name}")))
        }
        "compare" => {
            let a = point(editor, args.get("a"), true)?;
            let b = point(editor, args.get("b"), false)?;
            let rows: Vec<Value> = emulsion_core::graph::compare(&a, &b)
                .into_iter()
                .map(|r| json!({ "what": r.label, "a": r.a, "b": r.b }))
                .collect();
            Ok(ToolResult::text(if rows.is_empty() {
                "They are identical".to_string()
            } else {
                serde_json::to_string_pretty(&rows).unwrap_or_default()
            }))
        }
        "merge_branch" => {
            use emulsion_core::graph::{ConflictKey, MergeOutcome, Side};
            let from = args
                .get("branch")
                .and_then(Value::as_str)
                .ok_or_else(|| err("missing string 'branch'"))?;
            let mut choices = std::collections::HashMap::new();
            if let Some(map) = args.get("choices").and_then(Value::as_object) {
                for (k, v) in map {
                    let key = if k == "canvas" {
                        ConflictKey::Canvas
                    } else {
                        ConflictKey::Node(
                            k.parse()
                                .map_err(|_| err(format!("bad conflict key {k:?}")))?,
                        )
                    };
                    let side = match v.as_str() {
                        Some("ours") => Side::Ours,
                        Some("theirs") => Side::Theirs,
                        _ => {
                            return Err(err(format!(
                                "choice for {k} must be \"ours\" or \"theirs\""
                            )));
                        }
                    };
                    choices.insert(key, side);
                }
            }
            match editor
                .merge(from, &choices)
                .map_err(|e| err(e.to_string()))?
            {
                MergeOutcome::Merged(_) => Ok(ToolResult::text(format!(
                    "Merged {from} into {}. One undo reverts it.",
                    editor.graph.head()
                ))),
                MergeOutcome::Conflicts(c) => {
                    let list: Vec<Value> = c
                        .iter()
                        .map(|c| {
                            let key = match c.key {
                                ConflictKey::Canvas => "canvas".to_string(),
                                ConflictKey::Node(id) => id.to_string(),
                            };
                            json!({ "key": key, "what": c.what, "ours": c.ours, "theirs": c.theirs })
                        })
                        .collect();
                    Err(err(format!(
                        "Nothing was merged: both branches changed the same things. Ask the person which to keep, then call merge_branch again with choices.\n{}",
                        serde_json::to_string_pretty(&list).unwrap_or_default()
                    )))
                }
            }
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
/// A document at a branch name or commit id; `None` means the branch base
/// (when `base`) or the current document.
fn point(editor: &Editor, v: Option<&Value>, base: bool) -> Result<Document, ToolResult> {
    let g = &editor.graph;
    match v {
        None if base => Ok(editor.committed.clone()),
        None => Ok(editor.doc.clone()),
        Some(Value::Number(n)) => {
            let id = n
                .as_u64()
                .ok_or_else(|| err("commit ids are positive integers"))?;
            g.commit(id)
                .map(|c| c.doc.clone())
                .ok_or_else(|| err(format!("no commit {id}")))
        }
        Some(Value::String(name)) if *name == g.head() => Ok(editor.doc.clone()),
        Some(Value::String(name)) => {
            let b = g.branch(name).map_err(|e| err(e.to_string()))?;
            Ok(g.commit(b.tip).expect("tip").doc.clone())
        }
        Some(_) => Err(err("a and b are branch names or commit ids")),
    }
}

/// Branches and recent commits, for `list_history`.
pub fn history_json(editor: &Editor) -> Value {
    let g = &editor.graph;
    let branches: Vec<Value> = g
        .branches()
        .iter()
        .map(|(name, b)| {
            json!({
                "name": name,
                "current": name == g.head(),
                "tip": b.tip,
                "base": b.base,
                "ahead_of_main": g.ahead(name, emulsion_core::graph::MAIN),
            })
        })
        .collect();
    let commits: Vec<Value> = g
        .commits()
        .rev()
        .filter(|c| !c.auto)
        .take(30)
        .map(|c| json!({ "id": c.id, "name": c.name, "branch": c.branch, "parents": c.parents }))
        .collect();
    json!({
        "branches": branches,
        "uncommitted_changes": editor.uncommitted(),
        "recent_commits": commits,
        "note": "autosave commits are omitted",
    })
}

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
    let selection = doc.selection.as_deref().map(|s| {
        let b = select::bounds(s);
        json!({ "x": b.x, "y": b.y, "width": b.w, "height": b.h })
    });
    json!({
        "canvas": { "width": doc.width, "height": doc.height },
        "selection": selection,
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
    fn selection_fill_and_canvas_tools() {
        let mut e = editor();
        let r = execute(
            &mut e,
            "select_rect",
            &json!({ "x": 50, "y": 20, "width": 40, "height": 30 }),
        );
        assert!(!r.is_error, "{}", text(&r));
        assert_eq!(describe(&e)["selection"]["width"], 40);
        let r = execute(
            &mut e,
            "select_ellipse",
            &json!({ "x": 60, "y": 25, "width": 10, "height": 10, "mode": "subtract" }),
        );
        assert!(!r.is_error, "{}", text(&r));
        let before = e.doc.nodes.len();
        let r = execute(&mut e, "content_aware_fill", &json!({}));
        assert!(!r.is_error, "{}", text(&r));
        assert_eq!(e.doc.nodes.len(), before + 1);
        let r = execute(&mut e, "select_color", &json!({ "x": 5, "y": 5 }));
        assert!(
            !r.is_error && text(&r).starts_with("Selected"),
            "{}",
            text(&r)
        );
        let r = execute(
            &mut e,
            "fill_selection",
            &json!({ "node": 1, "color": "red" }),
        );
        assert!(r.is_error);
        let r = execute(&mut e, "deselect", &json!({}));
        assert!(!r.is_error && describe(&e)["selection"].is_null());
        let r = execute(
            &mut e,
            "crop",
            &json!({ "x": 10, "y": 10, "width": 100, "height": 50 }),
        );
        assert!(!r.is_error, "{}", text(&r));
        assert_eq!((e.doc.width, e.doc.height), (100, 50));
        let r = execute(&mut e, "image_size", &json!({ "width": 50 }));
        assert_eq!(text(&r), "Image is now 50×25");
    }

    #[test]
    fn branch_compare_and_merge_tools() {
        let mut e = editor();
        let ok = |r: ToolResult| {
            assert!(!r.is_error, "{}", text(&r));
            text(&r)
        };
        ok(execute(
            &mut e,
            "create_branch",
            &json!({ "name": "retouch" }),
        ));
        ok(execute(
            &mut e,
            "set_opacity",
            &json!({ "node": 2, "opacity": 40 }),
        ));
        let diff = ok(execute(&mut e, "compare", &json!({})));
        assert!(diff.contains("opacity"), "{diff}");
        ok(execute(&mut e, "switch_branch", &json!({ "name": "main" })));
        assert_eq!(e.doc.node(2).unwrap().opacity, 1.0);
        ok(execute(
            &mut e,
            "set_opacity",
            &json!({ "node": 2, "opacity": 70 }),
        ));
        let r = execute(&mut e, "merge_branch", &json!({ "branch": "retouch" }));
        assert!(
            r.is_error && text(&r).contains("\"key\": \"2\""),
            "{}",
            text(&r)
        );
        ok(execute(
            &mut e,
            "merge_branch",
            &json!({ "branch": "retouch", "choices": { "2": "theirs" } }),
        ));
        assert_eq!(e.doc.node(2).unwrap().opacity, 0.4);
        let h: Value =
            serde_json::from_str(&ok(execute(&mut e, "list_history", &json!({})))).unwrap();
        assert_eq!(h["branches"].as_array().unwrap().len(), 2);
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
