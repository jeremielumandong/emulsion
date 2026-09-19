//! Run tools against an open document.

use crate::server::ToolResult;
use base64::Engine as _;
use emulsion_core::command::Slot;
use emulsion_core::{Command, Document, Editor, Node, NodeId, NodeKind};
use emulsion_raster::composite::{flatten, level_size, region};
use emulsion_raster::paint::{Brush, Ink, Stroke};
use emulsion_raster::select::{self, Combine};
use emulsion_raster::{Adjustment, BlendMode, Placement};
use emulsion_raster::{IRect, Raster, color, fill, library};
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

fn hex_color(v: &Value) -> Result<[f32; 4], ToolResult> {
    let hex = v
        .as_str()
        .ok_or_else(|| err("color must be a string like #RRGGBB"))?;
    let n = hex
        .strip_prefix('#')
        .filter(|h| h.len() == 6)
        .and_then(|h| u32::from_str_radix(h, 16).ok())
        .ok_or_else(|| err(format!("bad color {hex:?}; use #RRGGBB")))?;
    Ok(color::srgba8_to_premul([
        (n >> 16) as u8,
        (n >> 8) as u8,
        n as u8,
        255,
    ]))
}

/// A brush by name with `settings` laid over it. Returns the brush and
/// its library category ("" when built from settings alone).
fn resolve_brush(
    name: Option<&Value>,
    settings: Option<&Value>,
    fallback: &Brush,
) -> Result<(Brush, String), ToolResult> {
    let (mut brush, category) = match name.and_then(Value::as_str) {
        Some(n) => {
            let p = library::find(n)
                .ok_or_else(|| err(format!("no brush named {n:?}; call list_brushes")))?;
            (p.brush, p.category)
        }
        None => (*fallback, String::new()),
    };
    if let Some(s) = settings {
        let obj = s
            .as_object()
            .ok_or_else(|| err("settings must be an object"))?;
        // Merge over the brush's JSON so any field can be set.
        let mut base = serde_json::to_value(brush).map_err(|e| err(e.to_string()))?;
        for (k, v) in obj {
            if base.get(k).is_none() {
                return Err(err(format!("unknown brush setting {k:?}")));
            }
            base[k] = v.clone();
        }
        brush = serde_json::from_value::<Brush>(base)
            .map_err(|e| err(format!("bad settings: {e}")))?
            .sanitized();
    }
    Ok((brush, category))
}

/// One resolved stroke of a `paint` call, in layer pixels.
pub struct ScriptStroke {
    pub brush: Brush,
    pub ink: Ink,
    /// (x, y, pressure).
    pub points: Vec<(f32, f32, Option<f32>)>,
}

/// A `paint` call resolved against a document: everything needed to lay
/// the strokes down, at once or one point at a time.
pub struct PaintScript {
    pub id: NodeId,
    pub strokes: Vec<ScriptStroke>,
    pub clip: Option<emulsion_raster::paint::Clip>,
    /// Layer pixels → document pixels.
    pub to_doc: glam::DAffine2,
    pub label: String,
    pub message: String,
}

impl PaintScript {
    /// Total path length in layer pixels, for pacing a playback.
    pub fn length(&self) -> f32 {
        self.strokes
            .iter()
            .map(|s| {
                s.points
                    .windows(2)
                    .map(|w| (w[1].0 - w[0].0).hypot(w[1].1 - w[0].1))
                    .sum::<f32>()
            })
            .sum()
    }

    /// Lay every stroke down on `base`. Returns the new layer and what changed.
    pub fn render(&self, base: &Raster) -> (Raster, IRect) {
        let mut current = base.clone();
        let mut dirty = IRect::default();
        for s in &self.strokes {
            let mut stroke = Stroke::new(
                Arc::new(current.clone()),
                s.brush,
                s.ink.clone(),
                self.clip.clone(),
            );
            for (x, y, p) in &s.points {
                stroke.point_at(*x, *y, *p, None);
            }
            stroke.finish();
            let (r, d) = stroke.render(&current);
            current = r;
            dirty = dirty.union(&d);
        }
        let dirty = dirty.intersect(&current.bounds());
        (current, dirty)
    }
}

/// Resolve a `paint` call against `doc` without painting anything.
pub fn paint_script(doc: &Document, args: &Value) -> Result<PaintScript, ToolResult> {
    let id = id_arg(args, "node")?;
    let node = doc.node(id).ok_or_else(|| err(format!("no node {id}")))?;
    let NodeKind::Raster { raster, placement } = &node.kind else {
        return Err(err(format!(
            "{} is not a pixel layer; add_layer makes one",
            node_label(doc, id)
        )));
    };
    if node.locked {
        return Err(err(format!("{} is locked", node_label(doc, id))));
    }
    let strokes = args
        .get("strokes")
        .and_then(Value::as_array)
        .filter(|s| !s.is_empty())
        .ok_or_else(|| err("strokes must be a non-empty array"))?;
    if strokes.len() > 400 {
        return Err(err("at most 400 strokes per call"));
    }
    let (default_brush, default_cat) =
        resolve_brush(args.get("brush"), args.get("settings"), &Brush::default())?;
    let default_color = match args.get("color") {
        Some(c) => Some(hex_color(c)?),
        None => None,
    };
    let to_doc = placement.to_doc(raster.width(), raster.height());
    let to_local = to_doc.inverse();
    let scale = to_doc.matrix2.determinant().abs().sqrt().max(1e-6);
    let clip = doc
        .selection
        .clone()
        .map(|sel| -> emulsion_raster::paint::Clip {
            Arc::new(move |x: i32, y: i32| {
                let p = to_doc.transform_point2(glam::dvec2(x as f64 + 0.5, y as f64 + 0.5));
                if p.x < 0.0 || p.y < 0.0 || p.x >= sel.width() as f64 || p.y >= sel.height() as f64
                {
                    0.0
                } else {
                    sel.get(p.x as u32, p.y as u32) as f32 / 255.0
                }
            })
        });
    let mut out = Vec::with_capacity(strokes.len());
    for (i, s) in strokes.iter().enumerate() {
        let settings = s.get("settings").or(args.get("settings"));
        let (mut brush, cat) = if s.get("brush").is_some() {
            resolve_brush(s.get("brush"), settings, &default_brush)?
        } else {
            (
                resolve_brush(None, s.get("settings"), &default_brush)?.0,
                default_cat.clone(),
            )
        };
        brush.size = (brush.size as f64 / scale) as f32;
        // Stabilizing suits a hand, not computed points.
        brush.stabilizer = 0.0;
        let ink = match cat.as_str() {
            "Eraser" => Ink::Erase,
            "Smudge" => Ink::Smudge,
            _ => {
                let c = match s.get("color") {
                    Some(c) => hex_color(c)?,
                    None => default_color.ok_or_else(|| {
                        err(format!(
                            "stroke {i} has no color and no color was given for the call"
                        ))
                    })?,
                };
                Ink::Color(c)
            }
        };
        let pts = s
            .get("points")
            .and_then(Value::as_array)
            .ok_or_else(|| err(format!("stroke {i} has no points")))?;
        if pts.len() > 2000 {
            return Err(err(format!("stroke {i} has more than 2000 points")));
        }
        let mut points = Vec::with_capacity(pts.len());
        for (j, p) in pts.iter().enumerate() {
            let a = p.as_array().filter(|a| a.len() >= 2).ok_or_else(|| {
                err(format!(
                    "stroke {i} point {j} must be [x, y] or [x, y, pressure]"
                ))
            })?;
            let (x, y) = (
                a[0].as_f64().unwrap_or(f64::NAN),
                a[1].as_f64().unwrap_or(f64::NAN),
            );
            if !x.is_finite() || !y.is_finite() {
                return Err(err(format!("stroke {i} point {j} is not a number")));
            }
            let pressure = a
                .get(2)
                .and_then(Value::as_f64)
                .map(|p| p.clamp(0.0, 1.0) as f32);
            let l = to_local.transform_point2(glam::dvec2(x, y));
            points.push((l.x as f32, l.y as f32, pressure));
        }
        out.push(ScriptStroke { brush, ink, points });
    }
    let count = out.len();
    let plural = if count == 1 { "" } else { "s" };
    Ok(PaintScript {
        id,
        strokes: out,
        clip,
        to_doc,
        label: format!("Paint ({count} stroke{plural})"),
        message: format!(
            "Painted {count} stroke{plural} on {} with {}",
            node_label(doc, id),
            args.get("brush")
                .and_then(Value::as_str)
                .unwrap_or("the given settings")
        ),
    })
}

/// Paint strokes onto a copy of a layer; runs off the UI thread.
fn plan_paint(doc: &Document, args: &Value) -> Result<Planned, ToolResult> {
    let script = paint_script(doc, args)?;
    let NodeKind::Raster { raster, .. } = &doc.node(script.id).expect("checked").kind else {
        unreachable!()
    };
    let (current, dirty) = script.render(raster);
    Ok(Planned {
        commands: vec![Command::ReplacePixels {
            id: script.id,
            raster: Arc::new(current),
            dirty,
            label: script.label,
        }],
        message: script.message,
    })
}

fn hex(c: [u8; 4]) -> String {
    format!("#{:02X}{:02X}{:02X}", c[0], c[1], c[2])
}

/// Colour argument as straight sRGB, or None for "none"/null.
fn rgba_arg(v: Option<&Value>) -> Result<Option<[u8; 4]>, ToolResult> {
    match v {
        None | Some(Value::Null) => Ok(None),
        Some(Value::String(s)) if s.eq_ignore_ascii_case("none") => Ok(None),
        Some(v) => {
            let p = hex_color(v)?;
            Ok(Some(color::premul_to_srgba8(p)))
        }
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
        "paint" => plan_paint(doc, args),
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
            if let Adjustment::Lut3D { cube, .. } = &adj
                && cube.name.is_empty()
                && cube.size == 2
            {
                return Err(err(
                    "a lut adjustment needs params.lut_file: the path to a .cube file",
                ));
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
        "select_node" => {
            let id = id_arg(args, "node")?;
            editor
                .doc
                .node(id)
                .ok_or_else(|| err(format!("no node {id}")))?;
            let m = editor
                .doc
                .node_coverage(id)
                .ok_or_else(|| err(format!("{} covers nothing", node_label(&editor.doc, id))))?;
            let (c, msg) = selection_command(&editor.doc, m, combine_arg(args), 0.0);
            exec(editor, c)?;
            Ok(ToolResult::text(msg))
        }
        "transform_selection" => {
            let sel = editor
                .doc
                .selection
                .clone()
                .ok_or_else(|| err("nothing is selected"))?;
            let num = |k: &str, d: f64| args.get(k).and_then(Value::as_f64).unwrap_or(d);
            let (dx, dy, scale, rot) = (
                num("dx", 0.0),
                num("dy", 0.0),
                num("scale", 1.0),
                num("rotation", 0.0),
            );
            if !(scale > 0.0 && scale <= 20.0) {
                return Err(err("scale must be above 0 and at most 20"));
            }
            let b = select::bounds(&sel);
            let c = glam::dvec2(b.x as f64 + b.w as f64 / 2.0, b.y as f64 + b.h as f64 / 2.0);
            let a = glam::DAffine2::from_translation(c + glam::dvec2(dx, dy))
                * glam::DAffine2::from_angle(rot.to_radians())
                * glam::DAffine2::from_scale(glam::dvec2(scale, scale))
                * glam::DAffine2::from_translation(-c);
            let m = select::transform(&sel, a);
            let (c, msg) = selection_command(&editor.doc, m, Combine::Replace, 0.0);
            exec(editor, c)?;
            Ok(ToolResult::text(msg))
        }
        "draw_path" => {
            let d = args
                .get("d")
                .and_then(Value::as_str)
                .ok_or_else(|| err("missing string 'd' (SVG path data)"))?;
            let path = emulsion_raster::vector::Path::from_svg(d)
                .map_err(|e| err(format!("bad path data: {e}")))?;
            let style = emulsion_raster::vector::PathStyle {
                stroke: if args.get("stroke").is_some() {
                    rgba_arg(args.get("stroke"))?
                } else {
                    Some([10, 10, 11, 255])
                },
                width: args.get("width").and_then(Value::as_f64).unwrap_or(3.0) as f32,
                fill: rgba_arg(args.get("fill"))?,
            }
            .sanitized();
            let (w, h) = (editor.doc.width, editor.doc.height);
            let name = args.get("name").and_then(Value::as_str).unwrap_or("Path");
            let node = Node::path(0, name, Arc::new(path), style, w, h);
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
            .ok_or_else(|| err("no node was created"))?;
            Ok(ToolResult::text(format!(
                "Added path {name:?} as node {id}"
            )))
        }
        "set_path" => {
            let id = id_arg(args, "node")?;
            let (path, mut style) = match &editor
                .doc
                .node(id)
                .ok_or_else(|| err(format!("no node {id}")))?
                .kind
            {
                NodeKind::Path { path, style, .. } => (path.clone(), *style),
                _ => {
                    return Err(err(format!(
                        "{} is not a path",
                        node_label(&editor.doc, id)
                    )));
                }
            };
            let path = match args.get("d").and_then(Value::as_str) {
                Some(d) => Arc::new(
                    emulsion_raster::vector::Path::from_svg(d)
                        .map_err(|e| err(format!("bad path data: {e}")))?,
                ),
                None => path,
            };
            if args.get("stroke").is_some() {
                style.stroke = rgba_arg(args.get("stroke"))?;
            }
            if args.get("fill").is_some() {
                style.fill = rgba_arg(args.get("fill"))?;
            }
            if let Some(w) = args.get("width").and_then(Value::as_f64) {
                style.width = w as f32;
            }
            exec(
                editor,
                Command::SetPath {
                    id,
                    path,
                    style: style.sanitized(),
                },
            )?;
            Ok(ToolResult::text(format!(
                "Updated path {}",
                node_label(&editor.doc, id)
            )))
        }
        "path_to_selection" => {
            let id = id_arg(args, "node")?;
            let NodeKind::Path { path, .. } = &editor
                .doc
                .node(id)
                .ok_or_else(|| err(format!("no node {id}")))?
                .kind
            else {
                return Err(err(format!(
                    "{} is not a path",
                    node_label(&editor.doc, id)
                )));
            };
            let m = path.fill_mask(editor.doc.width, editor.doc.height);
            let (c, msg) = selection_command(&editor.doc, m, combine_arg(args), 0.0);
            exec(editor, c)?;
            Ok(ToolResult::text(msg))
        }
        "list_recipes" => {
            let dir = emulsion_io::recent::data_dir().join("recipes");
            let list: Vec<Value> = emulsion_recipes::store::list(&dir)
                .into_iter()
                .map(|(r, origin)| {
                    json!({
                        "name": r.name,
                        "author": r.author,
                        "film_simulation": r.film_simulation,
                        "tags": r.tags,
                        "saved": matches!(origin, emulsion_recipes::store::Origin::Saved(_)),
                        "settings": {
                            "dynamic_range": format!("{:?}", r.dynamic_range),
                            "grain": format!("{:?} {:?}", r.grain.strength, r.grain.size),
                            "color_chrome_effect": format!("{:?}", r.color_chrome_effect),
                            "white_balance": format!("{} R{:+} B{:+}", r.white_balance.preset, r.white_balance.red, r.white_balance.blue),
                            "highlight": r.highlight, "shadow": r.shadow, "color": r.color,
                            "exposure_compensation": r.exposure_compensation,
                        }
                    })
                })
                .collect();
            let looks: Vec<Value> = emulsion_recipes::looks::LOOKS
                .iter()
                .map(|l| json!({ "key": l.key, "label": l.label }))
                .collect();
            Ok(ToolResult::text(
                serde_json::to_string_pretty(&json!({ "recipes": list, "looks": looks }))
                    .unwrap_or_default(),
            ))
        }
        "apply_recipe" => {
            let dir = emulsion_io::recent::data_dir().join("recipes");
            let (recipe, from_text) = if let Some(name) = args.get("name").and_then(Value::as_str) {
                (
                    emulsion_recipes::store::find(&dir, name).ok_or_else(|| {
                        err(format!("no recipe named {name:?}; call list_recipes"))
                    })?,
                    false,
                )
            } else if let Some(t) = args.get("toml").and_then(Value::as_str) {
                (
                    emulsion_recipes::Recipe::from_toml(t).map_err(|e| err(e.to_string()))?,
                    true,
                )
            } else if let Some(t) = args.get("text").and_then(Value::as_str) {
                let (r, unknown) = emulsion_recipes::import::parse_text(t);
                r.validate().map_err(|e| {
                    err(format!(
                        "{e}{}",
                        if unknown.is_empty() {
                            String::new()
                        } else {
                            format!(" (lines not understood: {unknown:?})")
                        }
                    ))
                })?;
                (r, true)
            } else {
                return Err(err("give name, text or toml"));
            };
            if from_text && args.get("save").and_then(Value::as_bool).unwrap_or(false) {
                emulsion_recipes::store::save(&dir, &recipe).map_err(|e| err(e.to_string()))?;
            }
            let compiled = emulsion_recipes::compile(&recipe).map_err(|e| err(e.to_string()))?;
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
            let n = compiled.1.len();
            let gid = emulsion_recipes::store::add_to(editor, compiled, slot)
                .map_err(|e| err(e.to_string()))?;
            Ok(ToolResult::text(format!(
                "Applied recipe {:?} as group {gid} with {n} adjustment stages",
                recipe.name
            )))
        }
        "add_layer" => {
            let (w, h) = (editor.doc.width, editor.doc.height);
            let name = args.get("name").and_then(Value::as_str).unwrap_or("Layer");
            let node = Node::raster(
                0,
                name,
                Arc::new(Raster::transparent(w, h)),
                Placement::default(),
            );
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
            .ok_or_else(|| err("no node was created"))?;
            Ok(ToolResult::text(format!(
                "Added layer {name:?} as node {id}"
            )))
        }
        "list_brushes" => {
            let list: Vec<Value> = library::library()
                .into_iter()
                .map(|b| json!({ "name": b.name, "category": b.category, "for": b.note, "size": b.brush.size }))
                .collect();
            Ok(ToolResult::text(serde_json::to_string_pretty(&json!({
                "brushes": list,
                "settings": "any Brush field: size, hardness, opacity, flow, spacing, roundness, angle, follow_path, grain (None|Paper|Canvas|Chalk|Speckle|Bristle|Halftone|Hatch|CrossHatch), grain_scale (dot or line pitch for tones), grain_strength (dot size for Halftone), edge_darken, size_pressure, flow_pressure, speed_thins, taper_start, taper_end, size_jitter, scatter, color_jitter, wetness, blend (Normal|Multiply|Behind)",
                "tips": "Manga: Maru/Kabura/Fude nibs for line work, Milli pens for borders, Screentone brushes lay dot tone fixed to the page (paint an area with one), Hatching/Cross hatch for shade, Speed lines flick from thick to hairline, Blue pencil for roughs (color #A4C8FF), White ink for highlights. Ink for lines (G-pen tapers), Pencil and Chalk show paper grain, Markers darken where they overlap, Watercolour and Oil mix with what is under them, Smudge drags colour, Eraser removes. Give pressure per point for thick-to-thin lines."
            })).unwrap_or_default()))
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
        "canvas_size" => {
            let int = |k: &str| {
                args.get(k)
                    .and_then(Value::as_u64)
                    .filter(|v| (1..=30_000).contains(v))
                    .ok_or_else(|| err(format!("'{k}' must be an integer from 1 to 30000")))
            };
            let (w, h) = (int("width")? as i64, int("height")? as i64);
            let (ow, oh) = (editor.doc.width as i64, editor.doc.height as i64);
            let (ax, ay) = match args
                .get("anchor")
                .and_then(Value::as_str)
                .unwrap_or("center")
            {
                "top-left" => (0, 0),
                "top" => (1, 0),
                "top-right" => (2, 0),
                "left" => (0, 1),
                "center" => (1, 1),
                "right" => (2, 1),
                "bottom-left" => (0, 2),
                "bottom" => (1, 2),
                "bottom-right" => (2, 2),
                other => return Err(err(format!("unknown anchor {other:?}"))),
            };
            let rect = IRect::new(
                (-((w - ow) * ax / 2)) as i32,
                (-((h - oh) * ay / 2)) as i32,
                w as i32,
                h as i32,
            );
            exec(
                editor,
                Command::Crop {
                    rect,
                    rotation: 0.0,
                },
            )?;
            Ok(ToolResult::text(format!("Canvas is now {w}×{h}")))
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
    let k = kind.trim().to_lowercase().replace(['-', ' '], "_");
    let k = match k.as_str() {
        "bw" | "black_white" | "monochrome" => "black_and_white",
        "curve" => "curves",
        "colour_balance" => "color_balance",
        "lut3d" | "cube" => "lut",
        other => other,
    };
    if k == "lut" {
        // A LUT needs a file; the caller fills it in from `lut_file`.
        return Some(Adjustment::Lut3D {
            cube: emulsion_raster::adjust::Cube {
                name: String::new(),
                size: 2,
                data: Arc::new(vec![
                    [0, 0, 0],
                    [65535, 0, 0],
                    [0, 65535, 0],
                    [65535, 65535, 0],
                    [0, 0, 65535],
                    [65535, 0, 65535],
                    [0, 65535, 65535],
                    [65535; 3],
                ]),
            },
            strength: 100.0,
        });
    }
    Adjustment::catalogue().into_iter().find(|a| a.key() == k)
}

fn curve_points(v: &Value, what: &str) -> Result<Vec<[f32; 2]>, ToolResult> {
    let arr = v.as_array().ok_or_else(|| {
        err(format!(
            "{what} must be an array of [input, output] pairs on 0–255"
        ))
    })?;
    let mut pts: Vec<[f32; 2]> = Vec::with_capacity(arr.len());
    for p in arr {
        let a = p
            .as_array()
            .filter(|a| a.len() == 2)
            .ok_or_else(|| err(format!("{what}: each point is [input, output]")))?;
        let (x, y) = (
            a[0].as_f64().unwrap_or(f64::NAN),
            a[1].as_f64().unwrap_or(f64::NAN),
        );
        if !x.is_finite() || !y.is_finite() {
            return Err(err(format!("{what}: points must be numbers")));
        }
        pts.push([x.clamp(0.0, 255.0) as f32, y.clamp(0.0, 255.0) as f32]);
    }
    if pts.len() < 2 || pts.len() > 32 {
        return Err(err(format!("{what} needs 2 to 32 points")));
    }
    pts.sort_by(|a, b| a[0].total_cmp(&b[0]));
    Ok(pts)
}

fn apply_params(adj: &mut Adjustment, params: &Map<String, Value>) -> Result<(), ToolResult> {
    for (k, v) in params {
        // Structured parameters first.
        match (&mut *adj, k.as_str()) {
            (Adjustment::Curves { master, .. }, "points" | "master") => {
                *master = curve_points(v, k)?;
                continue;
            }
            (Adjustment::Curves { red, .. }, "red") => {
                *red = curve_points(v, k)?;
                continue;
            }
            (Adjustment::Curves { green, .. }, "green") => {
                *green = curve_points(v, k)?;
                continue;
            }
            (Adjustment::Curves { blue, .. }, "blue") => {
                *blue = curve_points(v, k)?;
                continue;
            }
            (Adjustment::GradientMap { stops, .. }, "stops") => {
                let arr = v
                    .as_array()
                    .ok_or_else(|| err("stops must be an array of [position 0-1, \"#RRGGBB\"]"))?;
                let mut out = Vec::new();
                for s in arr {
                    let a = s
                        .as_array()
                        .filter(|a| a.len() == 2)
                        .ok_or_else(|| err("each stop is [position, \"#RRGGBB\"]"))?;
                    let pos = a[0]
                        .as_f64()
                        .ok_or_else(|| err("stop position must be a number"))?
                        .clamp(0.0, 1.0) as f32;
                    let c = color::premul_to_srgba8(hex_color(&a[1])?);
                    out.push(emulsion_raster::adjust::Stop {
                        pos,
                        color: [c[0], c[1], c[2]],
                    });
                }
                if out.len() < 2 || out.len() > 16 {
                    return Err(err("a gradient map needs 2 to 16 stops"));
                }
                *stops = out;
                continue;
            }
            (Adjustment::Lut3D { cube, .. }, "lut_file") => {
                let path = v
                    .as_str()
                    .ok_or_else(|| err("lut_file must be a path to a .cube file"))?;
                let text = std::fs::read_to_string(path)
                    .map_err(|e| err(format!("cannot read {path}: {e}")))?;
                *cube = emulsion_raster::adjust::Cube::parse(&text)
                    .map_err(|e| err(format!("{path}: {e}")))?;
                if cube.name.is_empty() {
                    cube.name = std::path::Path::new(path)
                        .file_stem()
                        .map(|s| s.to_string_lossy().into_owned())
                        .unwrap_or_default();
                }
                continue;
            }
            _ => {}
        }
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
                    NodeKind::Path { .. } => "path",
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
                NodeKind::Path { path, style, .. } => {
                    o.insert("d".into(), json!(path.to_svg()));
                    o.insert("anchors".into(), json!(path.anchor_count()));
                    o.insert("stroke".into(), json!(style.stroke.map(hex)));
                    o.insert("stroke_width".into(), json!(style.width));
                    o.insert("fill".into(), json!(style.fill.map(hex)));
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
        let r = execute(&mut e, "select_node", &json!({ "node": 1 }));
        assert!(
            !r.is_error && describe(&e)["selection"]["width"] == 200,
            "{}",
            text(&r)
        );
        let r = execute(
            &mut e,
            "select_rect",
            &json!({ "x": 10, "y": 10, "width": 20, "height": 20 }),
        );
        assert!(!r.is_error);
        let r = execute(
            &mut e,
            "transform_selection",
            &json!({ "dx": 30, "scale": 2 }),
        );
        assert!(!r.is_error, "{}", text(&r));
        let s = describe(&e)["selection"].clone();
        assert!(
            (s["width"].as_i64().unwrap() - 40).abs() <= 2
                && (s["x"].as_i64().unwrap() - 30).abs() <= 2,
            "{s}"
        );
        let r = execute(&mut e, "deselect", &json!({}));
        assert!(!r.is_error && describe(&e)["selection"].is_null());
        let r = execute(
            &mut e,
            "crop",
            &json!({ "x": 10, "y": 10, "width": 100, "height": 50 }),
        );
        assert!(!r.is_error, "{}", text(&r));
        assert_eq!((e.doc.width, e.doc.height), (100, 50));
        let r = execute(
            &mut e,
            "canvas_size",
            &json!({ "width": 120, "height": 60, "anchor": "top-left" }),
        );
        assert!(!r.is_error, "{}", text(&r));
        assert_eq!((e.doc.width, e.doc.height), (120, 60));
        let r = execute(
            &mut e,
            "canvas_size",
            &json!({ "width": 100, "height": 50, "anchor": "top-left" }),
        );
        assert!(!r.is_error);
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
    fn paint_tool_draws_with_library_brushes() {
        let mut e = editor();
        let r = execute(&mut e, "add_layer", &json!({ "name": "Sketch" }));
        assert!(!r.is_error, "{}", text(&r));
        let id = e.doc.nodes.last().unwrap().id;
        assert_eq!(e.doc.nodes.last().unwrap().name, "Sketch");
        let r = execute(&mut e, "list_brushes", &json!({}));
        assert!(text(&r).contains("G-pen"));
        let r = execute(
            &mut e,
            "paint",
            &json!({
                "node": id, "brush": "G-pen", "color": "#ff0000",
                "strokes": [
                    { "points": [[10, 50, 1.0], [190, 50, 0.2]] },
                    { "brush": "Chisel marker", "color": "#00ff00", "points": [[100, 10], [100, 90]] }
                ]
            }),
        );
        assert!(!r.is_error, "{}", text(&r));
        let NodeKind::Raster { raster, .. } = &e.doc.node(id).unwrap().kind else {
            panic!()
        };
        assert!(raster.get(50, 50)[0] > 60000, "red ink at the start");
        let g = raster.get(100, 30);
        assert!(
            g[1] > 30000 && g[0] < 2000,
            "green marker at 60% down the middle: {g:?}"
        );
        assert_eq!(e.history.len(), 2, "add_layer and one paint step");
        let r = execute(
            &mut e,
            "paint",
            &json!({ "node": id, "brush": "nope", "strokes": [{ "points": [[0, 0]] }] }),
        );
        assert!(r.is_error && text(&r).contains("list_brushes"));
        let r = execute(
            &mut e,
            "paint",
            &json!({ "node": id, "settings": { "sizes": 3 }, "color": "#000000", "strokes": [{ "points": [[0, 0]] }] }),
        );
        assert!(r.is_error && text(&r).contains("unknown brush setting"));
    }

    #[test]
    fn path_tools_draw_edit_and_select() {
        let mut e = editor();
        let r = execute(
            &mut e,
            "draw_path",
            &json!({ "name": "Leaf", "d": "M 20 20 C 60 0 100 40 80 80 Z", "fill": "#00ff00", "stroke": "none" }),
        );
        assert!(!r.is_error, "{}", text(&r));
        let id = e.doc.nodes.last().unwrap().id;
        let d = describe(&e);
        let me = d["nodes"]
            .as_array()
            .unwrap()
            .iter()
            .find(|n| n["id"] == id)
            .unwrap()
            .clone();
        assert_eq!(me["kind"], "path");
        assert!(me["d"].as_str().unwrap().starts_with("M 20 20 C"));
        let NodeKind::Path { cache, .. } = &e.doc.node(id).unwrap().kind else {
            panic!()
        };
        assert!(cache.get(60, 40)[1] > 60000, "filled green inside");
        let r = execute(
            &mut e,
            "set_path",
            &json!({ "node": id, "stroke": "#ff0000", "width": 6, "fill": "none" }),
        );
        assert!(!r.is_error, "{}", text(&r));
        let NodeKind::Path { cache, .. } = &e.doc.node(id).unwrap().kind else {
            panic!()
        };
        assert_eq!(cache.get(60, 40), [0; 4], "no fill now");
        let r = execute(&mut e, "path_to_selection", &json!({ "node": id }));
        assert!(
            !r.is_error && describe(&e)["selection"]["width"].as_i64().unwrap() > 40,
            "{}",
            text(&r)
        );
        let r = execute(
            &mut e,
            "draw_path",
            &json!({ "d": "M 0 0 A 5 5 0 0 1 1 1" }),
        );
        assert!(r.is_error);
    }

    #[test]
    fn new_adjustment_kinds_and_structured_params() {
        let mut e = editor();
        let r = execute(
            &mut e,
            "add_adjustment",
            &json!({ "kind": "curves", "params": { "points": [[0, 0], [64, 40], [192, 215], [255, 255]], "red": [[0, 10], [255, 255]] } }),
        );
        assert!(!r.is_error, "{}", text(&r));
        let NodeKind::Adjust(Adjustment::Curves { master, red, .. }) =
            &e.doc.nodes.last().unwrap().kind
        else {
            panic!()
        };
        assert_eq!((master.len(), red[0][1]), (4, 10.0));
        let r = execute(
            &mut e,
            "add_adjustment",
            &json!({ "kind": "gradient_map", "params": { "stops": [[0, "#000000"], [1, "#ffcc00"]], "reverse": 1 } }),
        );
        assert!(!r.is_error, "{}", text(&r));
        let NodeKind::Adjust(Adjustment::GradientMap { stops, reverse }) =
            &e.doc.nodes.last().unwrap().kind
        else {
            panic!()
        };
        assert!(*reverse && stops[1].color == [255, 204, 0]);
        for kind in [
            "color_balance",
            "vibrance",
            "black_and_white",
            "photo_filter",
            "grain",
            "threshold",
            "posterize",
            "Colour Balance",
        ] {
            let r = execute(&mut e, "add_adjustment", &json!({ "kind": kind }));
            assert!(!r.is_error, "{kind}: {}", text(&r));
        }
        let r = execute(
            &mut e,
            "add_adjustment",
            &json!({ "kind": "color_balance", "params": { "midtones_cr": 40, "preserve_luminosity": 0 } }),
        );
        assert!(!r.is_error, "{}", text(&r));
        let r = execute(&mut e, "add_adjustment", &json!({ "kind": "lut" }));
        assert!(r.is_error && text(&r).contains("lut_file"));
        let r = execute(
            &mut e,
            "add_adjustment",
            &json!({ "kind": "curves", "params": { "points": [[0, 0]] } }),
        );
        assert!(r.is_error);
    }

    #[test]
    fn recipes_list_and_apply_from_name_and_text() {
        let mut e = editor();
        let r = execute(&mut e, "list_recipes", &json!({}));
        assert!(
            !r.is_error && text(&r).contains("Chrome Street"),
            "{}",
            text(&r)
        );
        let before = e.doc.nodes.len();
        let r = execute(&mut e, "apply_recipe", &json!({ "name": "chrome street" }));
        assert!(!r.is_error, "{}", text(&r));
        let group = e.doc.nodes.iter().find(|n| n.is_group()).unwrap();
        assert!(group.name.contains("Chrome Street"));
        assert!(e.doc.nodes.len() > before + 5);
        let r = execute(
            &mut e,
            "apply_recipe",
            &json!({ "text": "Film Simulation: Acros+R\nGrain Effect: Strong, Large\nHighlight: +1" }),
        );
        assert!(!r.is_error, "{}", text(&r));
        let r = execute(
            &mut e,
            "apply_recipe",
            &json!({ "text": "Film Simulation: Kodachrome" }),
        );
        assert!(r.is_error);
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
