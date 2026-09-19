//! Run tools against an open document.

use crate::server::ToolResult;
#[cfg(test)]
use base64::Engine as _;
use emulsion_core::command::Slot;
use emulsion_core::{Command, Document, Editor, Node, NodeId, NodeKind};
use emulsion_raster::composite::region;
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
                .or_else(|| {
                    let want = n.trim().to_lowercase();
                    saved_brushes()
                        .into_iter()
                        .find(|p| p.name.to_lowercase() == want)
                })
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
    /// Optional snapshot of visible lower layers, sampled in layer coordinates.
    pub backdrop: Option<emulsion_raster::paint::Backdrop>,
    /// Layer pixels → document pixels.
    pub to_doc: glam::DAffine2,
    /// Mirror axes through the canvas centre, in layer pixels.
    pub mirror: (Option<f32>, Option<f32>),
    /// Rotational symmetry about the canvas centre (layer pixels), copies.
    pub radial: Option<((f32, f32), u32)>,
    pub label: String,
    pub message: String,
}

impl PaintScript {
    /// Shared stroke setup for immediate rendering and animated UI playback.
    pub fn start_stroke(&self, base: Arc<Raster>, s: &ScriptStroke) -> Stroke {
        let mut stroke = Stroke::new(base, s.brush, s.ink.clone(), self.clip.clone());
        if let Some(backdrop) = &self.backdrop {
            stroke.set_backdrop(backdrop.clone());
        }
        stroke.set_mirror(self.mirror.0, self.mirror.1);
        if let Some((c, n)) = self.radial {
            stroke.set_radial(c, n);
        }
        stroke
    }

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
            let mut stroke = self.start_stroke(Arc::new(current.clone()), s);
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

/// Turn a `hatch` call into `paint` arguments: parallel strokes across a
/// rectangle (or the selection's bounds) at an angle and spacing.
pub fn hatch_to_paint(doc: &Document, args: &Value) -> Result<Value, ToolResult> {
    let rect = match args.get("rect").and_then(Value::as_array) {
        Some(r) if r.len() == 4 => {
            let v: Vec<f64> = r.iter().map(|x| x.as_f64().unwrap_or(f64::NAN)).collect();
            if v.iter().any(|x| !x.is_finite()) || v[2] <= 0.0 || v[3] <= 0.0 {
                return Err(err("rect must be [x, y, width, height]"));
            }
            (v[0], v[1], v[2], v[3])
        }
        _ => match &doc.selection {
            Some(sel) => {
                let b = select::bounds(sel);
                (b.x as f64, b.y as f64, b.w as f64, b.h as f64)
            }
            None => {
                return Err(err(
                    "give rect [x, y, width, height] or make a selection first",
                ));
            }
        },
    };
    let angle = args
        .get("angle")
        .and_then(Value::as_f64)
        .unwrap_or(45.0)
        .to_radians();
    let spacing = args
        .get("spacing")
        .and_then(Value::as_f64)
        .unwrap_or(8.0)
        .clamp(1.0, 200.0);
    let jitter = args
        .get("jitter")
        .and_then(Value::as_f64)
        .unwrap_or(0.15)
        .clamp(0.0, 1.0);
    let cross = args.get("cross").and_then(Value::as_bool).unwrap_or(false);
    let (cx, cy) = (rect.0 + rect.2 / 2.0, rect.1 + rect.3 / 2.0);
    let half = (rect.2 * rect.2 + rect.3 * rect.3).sqrt() / 2.0;
    let mut strokes = Vec::new();
    let mut seed: u64 = 0x9E37_79B9_7F4A_7C15;
    let mut rnd = || {
        seed ^= seed << 13;
        seed ^= seed >> 7;
        seed ^= seed << 17;
        (seed >> 40) as f64 / (1u64 << 24) as f64 - 0.5
    };
    let angles: Vec<f64> = if cross {
        vec![angle, angle + std::f64::consts::FRAC_PI_2]
    } else {
        vec![angle]
    };
    for a in angles {
        let (dx, dy) = (a.cos(), a.sin());
        let (nx, ny) = (-dy, dx);
        let n = (2.0 * half / spacing).ceil() as i64;
        for k in -n..=n {
            let off = k as f64 * spacing + rnd() * spacing * jitter;
            let (mx, my) = (cx + nx * off, cy + ny * off);
            // Clip the infinite line to the rectangle.
            let mut ts: Vec<f64> = Vec::new();
            for (edge, dir) in [(rect.0, dx), (rect.0 + rect.2, dx)] {
                if dir.abs() > 1e-9 {
                    ts.push((edge - mx) / dir);
                }
            }
            for (edge, dir) in [(rect.1, dy), (rect.1 + rect.3, dy)] {
                if dir.abs() > 1e-9 {
                    ts.push((edge - my) / dir);
                }
            }
            let inside = |t: f64| {
                let (x, y) = (mx + dx * t, my + dy * t);
                x >= rect.0 - 1e-6
                    && x <= rect.0 + rect.2 + 1e-6
                    && y >= rect.1 - 1e-6
                    && y <= rect.1 + rect.3 + 1e-6
            };
            let mut hits: Vec<f64> = ts.into_iter().filter(|t| inside(*t)).collect();
            hits.sort_by(|p, q| p.total_cmp(q));
            if hits.len() < 2 || hits[hits.len() - 1] - hits[0] < 2.0 {
                continue;
            }
            let (t0, t1) = (
                hits[0] + rnd() * spacing * jitter,
                hits[hits.len() - 1] + rnd() * spacing * jitter,
            );
            let wobble = rnd() * spacing * jitter * 0.5;
            let (x0, y0, x1, y1) = (mx + dx * t0, my + dy * t0, mx + dx * t1, my + dy * t1);
            let (xm, ym) = ((x0 + x1) / 2.0 + nx * wobble, (y0 + y1) / 2.0 + ny * wobble);
            strokes
                .push(json!({ "points": [[x0, y0], [xm, ym], [x1, y1]], "pressure": [0.6, 1.0] }));
            if strokes.len() > 400 {
                return Err(err(
                    "that would take more than 400 strokes; use a wider spacing or smaller area",
                ));
            }
        }
    }
    let mut paint =
        json!({ "node": args.get("node").cloned().unwrap_or(Value::Null), "strokes": strokes });
    for k in ["brush", "color", "settings", "sample_merged"] {
        if let Some(v) = args.get(k) {
            paint[k] = v.clone();
        }
    }
    Ok(paint)
}

/// A paint script for `paint` or `hatch`.
pub fn paint_script_for(
    doc: &Document,
    name: &str,
    args: &Value,
) -> Result<PaintScript, ToolResult> {
    if name == "hatch" {
        let a = hatch_to_paint(doc, args)?;
        let mut script = paint_script(doc, &a)?;
        script.label = format!("Hatch ({} strokes)", script.strokes.len());
        script.message = format!(
            "Hatched with {} strokes on {}",
            script.strokes.len(),
            node_label(doc, script.id)
        );
        Ok(script)
    } else {
        paint_script(doc, args)
    }
}

/// Resolve a `paint` call against `doc` without painting anything.
pub fn paint_script(doc: &Document, args: &Value) -> Result<PaintScript, ToolResult> {
    let sample_merged = match args.get("sample_merged") {
        None => false,
        Some(Value::Bool(value)) => *value,
        _ => return Err(err("sample_merged must be a boolean")),
    };
    let alpha_lock = match args.get("alpha_lock") {
        None => false,
        Some(Value::Bool(value)) => *value,
        _ => return Err(err("alpha_lock must be a boolean")),
    };
    let (mirror_x, mirror_y) = match args.get("mirror") {
        None | Some(Value::Null) => (false, false),
        Some(Value::String(m)) => match m.as_str() {
            "x" => (true, false),
            "y" => (false, true),
            "xy" | "both" => (true, true),
            "none" | "" => (false, false),
            _ => return Err(err("mirror must be \"x\", \"y\" or \"xy\"")),
        },
        _ => return Err(err("mirror must be \"x\", \"y\" or \"xy\"")),
    };
    let symmetry = match args.get("symmetry") {
        None | Some(Value::Null) => 0,
        Some(v) => v
            .as_u64()
            .filter(|n| (2..=64).contains(n) || *n == 0 || *n == 1)
            .ok_or_else(|| err("symmetry must be an integer 2–64"))? as u32,
    };
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
    let clip = if alpha_lock {
        // Paint only where the layer already has pixels, at their coverage.
        let base = raster.clone();
        let sel = clip;
        Some(Arc::new(move |x: i32, y: i32| {
            if x < 0 || y < 0 || x >= base.width() as i32 || y >= base.height() as i32 {
                return 0.0;
            }
            let a = base.get(x as u32, y as u32)[3] as f32 / 65535.0;
            a * sel.as_ref().map_or(1.0, |c| c(x, y))
        }) as emulsion_raster::paint::Clip)
    } else {
        clip
    };
    let centre =
        to_local.transform_point2(glam::dvec2(doc.width as f64 / 2.0, doc.height as f64 / 2.0));
    let centre = (centre.x as f32, centre.y as f32);
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
        // Points come as [x, y, pressure?] lists or as SVG path data.
        let mut subpaths: Vec<Vec<(f64, f64, Option<f32>)>> = Vec::new();
        if let Some(d) = s.get("d").and_then(Value::as_str) {
            let path = emulsion_raster::vector::Path::from_svg(d)
                .map_err(|e| err(format!("stroke {i}: bad path data: {e}")))?;
            for (pts, closed) in path.flatten(0.75) {
                let mut pts: Vec<(f64, f64, Option<f32>)> =
                    pts.into_iter().map(|(x, y)| (x, y, None)).collect();
                if closed && let Some(f) = pts.first().copied() {
                    pts.push(f);
                }
                subpaths.push(pts);
            }
        } else {
            let pts = s
                .get("points")
                .and_then(Value::as_array)
                .ok_or_else(|| err(format!("stroke {i} has neither points nor d")))?;
            let mut doc_pts = Vec::with_capacity(pts.len());
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
                doc_pts.push((
                    x,
                    y,
                    a.get(2)
                        .and_then(Value::as_f64)
                        .map(|p| p.clamp(0.0, 1.0) as f32),
                ));
            }
            subpaths.push(doc_pts);
        }
        if subpaths.iter().map(Vec::len).sum::<usize>() > 4000 {
            return Err(err(format!("stroke {i} has more than 4000 points")));
        }
        // SVG moveto lifts the pen: pressure and taper restart independently.
        for mut doc_pts in subpaths {
            // A pressure envelope [start, end] fills in points without their own.
            if let Some(env) = s
                .get("pressure")
                .and_then(Value::as_array)
                .filter(|e| e.len() == 2)
            {
                let (p0, p1) = (
                    env[0].as_f64().unwrap_or(1.0).clamp(0.0, 1.0) as f32,
                    env[1].as_f64().unwrap_or(1.0).clamp(0.0, 1.0) as f32,
                );
                let total: f64 = doc_pts
                    .windows(2)
                    .map(|w| (w[1].0 - w[0].0).hypot(w[1].1 - w[0].1))
                    .sum();
                let mut run = 0.0;
                for k in 0..doc_pts.len() {
                    if k > 0 {
                        run += (doc_pts[k].0 - doc_pts[k - 1].0)
                            .hypot(doc_pts[k].1 - doc_pts[k - 1].1);
                    }
                    if doc_pts[k].2.is_none() {
                        let t = if total > 0.0 {
                            (run / total) as f32
                        } else {
                            0.0
                        };
                        doc_pts[k].2 = Some(p0 + (p1 - p0) * t);
                    }
                }
            }
            let points: Vec<(f32, f32, Option<f32>)> = doc_pts
                .into_iter()
                .map(|(x, y, p)| {
                    let l = to_local.transform_point2(glam::dvec2(x, y));
                    (l.x as f32, l.y as f32, p)
                })
                .collect();
            out.push(ScriptStroke {
                brush,
                ink: ink.clone(),
                points,
            });
        }
    }
    let count = out.len();
    let plural = if count == 1 { "" } else { "s" };
    Ok(PaintScript {
        id,
        strokes: out,
        clip,
        backdrop: sample_merged.then(|| lower_layer_backdrop(doc, id, to_doc)),
        to_doc,
        mirror: (mirror_x.then_some(centre.0), mirror_y.then_some(centre.1)),
        radial: (symmetry >= 2).then_some((centre, symmetry)),
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

/// Keep the document hierarchy, visibility, masks and blending, but omit the
/// target and all higher content. Ancestors remain to composite lower siblings.
fn lower_layer_backdrop(
    doc: &Document,
    id: NodeId,
    to_doc: glam::DAffine2,
) -> emulsion_raster::paint::Backdrop {
    use emulsion_raster::TileCoord;
    use emulsion_raster::composite::render_tile;
    use emulsion_raster::tile::{FTile, TILE};
    let mut lower = doc.clone();
    let index = doc.nodes.iter().position(|n| n.id == id).expect("checked");
    for node in &mut lower.nodes[index..] {
        if !doc.is_ancestor(node.id, id) {
            node.visible = false;
        }
    }
    let tree = lower.composite_tree();
    // A small bounded cache avoids recompositing a tile for every brush sample.
    let cache = std::sync::Mutex::new(std::collections::HashMap::<TileCoord, FTile>::new());
    Arc::new(move |x, y| {
        let p = to_doc.transform_point2(glam::dvec2(x as f64 + 0.5, y as f64 + 0.5));
        if p.x < 0.0 || p.y < 0.0 || p.x >= tree.width as f64 || p.y >= tree.height as f64 {
            return [0.0; 4];
        }
        let (dx, dy) = (p.x.floor() as u32, p.y.floor() as u32);
        let coord = TileCoord::new((dx / TILE) as i32, (dy / TILE) as i32);
        let mut cache = cache.lock().unwrap_or_else(|e| e.into_inner());
        if cache.len() >= 16 && !cache.contains_key(&coord) {
            cache.clear();
        }
        let tile = cache
            .entry(coord)
            .or_insert_with(|| render_tile(&tree, 0, coord));
        tile[((dy % TILE) * TILE + dx % TILE) as usize]
    })
}

/// Paint strokes onto a copy of a layer; runs off the UI thread.
fn plan_paint(doc: &Document, args: &Value) -> Result<Planned, ToolResult> {
    let script = paint_script(doc, args)?;
    plan_from_script(doc, script)
}

fn plan_from_script(doc: &Document, script: PaintScript) -> Result<Planned, ToolResult> {
    let NodeKind::Raster { raster, .. } = &doc.node(script.id).expect("checked").kind else {
        unreachable!()
    };
    let (current, dirty) = script.render(raster);
    // A quick look at the result, so the model corrects course.
    let mut after = doc.clone();
    if let Some(NodeKind::Raster { raster: r, .. }) = after.node_mut(script.id).map(|n| &mut n.kind)
    {
        *r = Arc::new(current.clone());
    }
    let note = critique_note(&after, std::env::var("TYPESAFE_API_KEY").ok().as_deref());
    Ok(Planned {
        commands: vec![Command::ReplacePixels {
            id: script.id,
            raster: Arc::new(current),
            dirty,
            label: script.label,
        }],
        message: format!("{}{note}", script.message),
    })
}

/// "\nCritique: …" for the top two issues, ranked by Jev when a key is given.
pub fn critique_note(doc: &Document, jev_key: Option<&str>) -> String {
    let mut c = emulsion_ai::critique::analyze(doc);
    if let Some(k) = jev_key.filter(|k| !k.is_empty()) {
        let _ = emulsion_ai::critique::rank_with_jev(&emulsion_ai::jev::Jev::new(k), &mut c);
    }
    let lines = c.lines(2);
    if lines.is_empty() {
        String::new()
    } else {
        format!("\nCritique ({}): {}", c.ranked_by, lines.join(" "))
    }
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

/// Apply text/x/y/size/color/font/bold/italic/align/width/line_height/
/// letter_spacing from `args` onto `spec`; absent keys leave it alone.
fn text_args(spec: &mut emulsion_core::text::TextSpec, args: &Value) -> Result<(), ToolResult> {
    if let Some(t) = args.get("text").and_then(Value::as_str) {
        spec.text = t.to_string();
    }
    let num = |k: &str| args.get(k).and_then(Value::as_f64).map(|v| v as f32);
    if let Some(v) = num("x") {
        spec.x = v;
    }
    if let Some(v) = num("y") {
        spec.y = v;
    }
    if let Some(v) = num("size") {
        spec.size = v;
    }
    if let Some(v) = num("line_height") {
        spec.line_height = v;
    }
    if let Some(v) = num("letter_spacing") {
        spec.letter_spacing = v;
    }
    if let Some(c) = args.get("color") {
        spec.color = rgba_arg(Some(c))?.unwrap_or([0, 0, 0, 255]);
    }
    if let Some(f) = args.get("font").and_then(Value::as_str) {
        spec.font = f.trim().to_string();
    }
    if let Some(b) = args.get("bold").and_then(Value::as_bool) {
        spec.bold = b;
    }
    if let Some(b) = args.get("italic").and_then(Value::as_bool) {
        spec.italic = b;
    }
    if let Some(a) = args.get("align").and_then(Value::as_str) {
        spec.align = emulsion_core::text::Align::parse(a).ok_or_else(|| {
            err(format!(
                "unknown align {a:?}: left, center, right or justify"
            ))
        })?;
    }
    match args.get("width") {
        None => {}
        Some(Value::Null) => spec.width = None,
        Some(v) => spec.width = v.as_f64().map(|w| w as f32),
    }
    Ok(())
}

fn filter_by_kind(kind: &str) -> Option<emulsion_filters::Filter> {
    let k = kind.trim().to_lowercase().replace(['-', ' '], "_");
    let k = match k.as_str() {
        "gaussian" | "blur" => "gaussian_blur",
        "sharpen" | "unsharp" => "unsharp_mask",
        "noise" => "add_noise",
        "denoise" => "reduce_noise",
        other => other,
    };
    emulsion_filters::Filter::catalogue()
        .into_iter()
        .find(|f| f.key() == k)
}

fn apply_filter_params(
    f: &mut emulsion_filters::Filter,
    params: &Map<String, Value>,
) -> Result<(), ToolResult> {
    for (k, v) in params {
        let v = v
            .as_f64()
            .ok_or_else(|| err(format!("parameter '{k}' must be a number")))?;
        if !f.set_param(k, v as f32) {
            let keys: Vec<&str> = f.params().iter().map(|s| s.key).collect();
            return Err(err(format!(
                "{} has no parameter '{k}' (valid: {})",
                f.label(),
                keys.join(", ")
            )));
        }
    }
    Ok(())
}

/// The filter stack of a smart node.
fn smart_filters(doc: &Document, id: NodeId) -> Result<Vec<emulsion_filters::Filter>, ToolResult> {
    match &doc
        .node(id)
        .ok_or_else(|| err(format!("no node {id}")))?
        .kind
    {
        NodeKind::Smart { filters, .. } => Ok(filters.clone()),
        _ => Err(err(format!(
            "{} is not a smart layer; call convert_to_smart first",
            node_label(doc, id)
        ))),
    }
}

/// Compute a heavy tool against a document snapshot, on any thread.
/// Brushes the person saved or imported (`<data dir>/brush-presets.json`).
pub fn saved_brushes() -> Vec<library::BrushPreset> {
    std::fs::read(emulsion_io::recent::data_dir().join("brush-presets.json"))
        .ok()
        .and_then(|b| serde_json::from_slice(&b).ok())
        .unwrap_or_default()
}

/// The flattened document as a raster.
fn doc_raster(doc: &Document) -> Raster {
    let (w, h) = (doc.width, doc.height);
    let px: Vec<[u16; 4]> = region(&doc.composite_tree(), IRect::new(0, 0, w as i32, h as i32))
        .into_iter()
        .map(color::f_to_px)
        .collect();
    Raster::from_pixels(w, h, [0; 4], &px)
}

pub fn plan_heavy(doc: &Document, name: &str, args: &Value) -> Result<Planned, ToolResult> {
    let (w, h) = (doc.width, doc.height);
    match name {
        "inpaint" => {
            if emulsion_ai::inpaint::available().is_none() {
                return Err(err(
                    "the fill model is not installed; download_model lama first",
                ));
            }
            let hole = match args.get("rect").and_then(Value::as_array) {
                Some(r) if r.len() == 4 => {
                    let v: Vec<i32> = r
                        .iter()
                        .map(|x| x.as_f64().unwrap_or(0.0).round() as i32)
                        .collect();
                    let rect = IRect::new(v[0], v[1], v[2].max(1), v[3].max(1));
                    emulsion_raster::Mask::from_fn(w, h, 0, move |x, y| {
                        let (x, y) = (x as i32, y as i32);
                        if x >= rect.x && y >= rect.y && x < rect.right() && y < rect.bottom() {
                            255
                        } else {
                            0
                        }
                    })
                }
                _ => match &doc.selection {
                    Some(s) => (**s).clone(),
                    None => return Err(err("select the area to fill, or pass rect")),
                },
            };
            let img = doc_raster(doc);
            let job = emulsion_ai::jobs::Job::new();
            let (layer, reg) =
                emulsion_ai::inpaint::fill(&img, &hole, &job).map_err(|e| err(e.to_string()))?;
            Ok(Planned {
                commands: vec![Command::AddNode {
                    node: Box::new(
                        Node::raster(
                            0,
                            "AI fill",
                            Arc::new(layer),
                            Placement::at(reg.x as f64, reg.y as f64),
                        )
                        .from_model(
                            emulsion_ai::inpaint::available()
                                .map(|m| m.id)
                                .unwrap_or("lama"),
                        ),
                    ),
                    slot: Slot::TOP,
                }],
                message: format!(
                    "Filled {}×{} at {}, {} into a new node \"AI fill\"",
                    reg.w, reg.h, reg.x, reg.y
                ),
            })
        }
        "depth_map" => {
            if emulsion_ai::depth::available().is_none() {
                return Err(err(
                    "the depth model is not installed; download_model depth-anything-v2-small first",
                ));
            }
            let img = doc_raster(doc);
            let job = emulsion_ai::jobs::Job::new();
            let m = emulsion_ai::depth::estimate(&img, &job).map_err(|e| err(e.to_string()))?;
            let name = args
                .get("name")
                .and_then(Value::as_str)
                .unwrap_or("Depth (AI)")
                .to_string();
            Ok(Planned {
                commands: vec![Command::AddNode {
                    node: Box::new(
                        Node::raster(
                            0,
                            name.clone(),
                            Arc::new(m.to_grey_raster()),
                            Placement::default(),
                        )
                        .from_model(
                            emulsion_ai::depth::available()
                                .map(|m| m.id)
                                .unwrap_or("depth"),
                        ),
                    ),
                    slot: Slot::TOP,
                }],
                message: format!("Added depth map {name:?}: near is bright, far is dark"),
            })
        }
        "restore_faces" => {
            if emulsion_ai::face::detector_available().is_none()
                || emulsion_ai::face::available().is_none()
            {
                return Err(err(
                    "face restore needs yoloface and gfpgan; download_model both first",
                ));
            }
            let strength = args.get("strength").and_then(Value::as_f64).unwrap_or(1.0) as f32;
            let img = doc_raster(doc);
            let job = emulsion_ai::jobs::Job::new();
            let (restored, n) =
                emulsion_ai::face::restore(&img, strength, &job).map_err(|e| err(e.to_string()))?;
            Ok(Planned {
                commands: vec![Command::AddNode {
                    node: Box::new(
                        Node::raster(
                            0,
                            "Faces restored (AI)",
                            Arc::new(restored),
                            Placement::default(),
                        )
                        .from_model(
                            emulsion_ai::face::available()
                                .map(|m| m.id)
                                .unwrap_or("gfpgan"),
                        ),
                    ),
                    slot: Slot::TOP,
                }],
                message: format!("Restored {n} face(s) into a new node on top"),
            })
        }
        "upscale" => {
            if emulsion_ai::upscale::available().is_none() {
                return Err(err(
                    "no upscale model is installed; download_model swin2sr-realworld-x4 first",
                ));
            }
            let f = emulsion_ai::upscale::factor();
            if w.saturating_mul(f) > 16_384 || h.saturating_mul(f) > 16_384 {
                return Err(err(
                    "too large to upscale in one go; crop or downsize first",
                ));
            }
            let img = doc_raster(doc);
            let job = emulsion_ai::jobs::Job::new();
            let big = emulsion_ai::upscale::upscale(&img, &job).map_err(|e| err(e.to_string()))?;
            Ok(Planned {
                commands: vec![
                    Command::ImageSize {
                        width: w * f,
                        height: h * f,
                    },
                    Command::AddNode {
                        node: Box::new(
                            Node::raster(
                                0,
                                format!("Upscaled ×{f} (AI)"),
                                Arc::new(big),
                                Placement::default(),
                            )
                            .from_model(
                                emulsion_ai::upscale::available()
                                    .map(|m| m.id)
                                    .unwrap_or("upscale"),
                            ),
                        ),
                        slot: Slot::TOP,
                    },
                ],
                message: format!(
                    "Upscaled ×{f} to {}×{}; the result is the top node",
                    w * f,
                    h * f
                ),
            })
        }
        "import_recipe" => {
            use emulsion_recipes::{Recipe, import, store};
            let dir = emulsion_io::recent::data_dir().join("recipes");
            let mut saved: Vec<String> = Vec::new();
            let mut skipped: Vec<String> = Vec::new();
            let mut keep = |r: Recipe, unknown: Vec<String>| -> Result<(), ToolResult> {
                r.validate().map_err(|e| err(e.to_string()))?;
                store::save(&dir, &r).map_err(|e| err(e.to_string()))?;
                saved.push(r.name.clone());
                skipped.extend(unknown);
                Ok(())
            };
            if let Some(t) = args.get("text").and_then(Value::as_str) {
                let trimmed = t.trim();
                let (r, unknown) = match Recipe::from_toml(t) {
                    Ok(r) => (r, Vec::new()),
                    Err(_) if trimmed.starts_with('<') && trimmed.contains("crs:") => {
                        import::from_xmp(t)
                    }
                    Err(_) if trimmed.starts_with('<') => import::from_fp1(t),
                    Err(_) => import::parse_text(t),
                };
                keep(r, unknown)?;
            } else if let Some(p) = args.get("path").and_then(Value::as_str) {
                let (r, unknown) = import::from_file(std::path::Path::new(p)).map_err(err)?;
                keep(r, unknown)?;
            } else if let Some(url) = args.get("url").and_then(Value::as_str) {
                let html = import::fetch(url).map_err(err)?;
                let (single, unknown) = import::from_html(&html, url);
                let links = import::recipe_links(&html, url);
                if single.validate().is_ok()
                    && single.film_simulation != Recipe::default().film_simulation
                    || links.len() < 2
                {
                    keep(single, unknown)?;
                } else {
                    for link in links.iter().take(400) {
                        if let Ok(h) = import::fetch(link) {
                            let (r, unknown) = import::from_html(&h, link);
                            if r.validate().is_ok() && store::save(&dir, &r).is_ok() {
                                saved.push(r.name.clone());
                                skipped.extend(unknown);
                            }
                        }
                    }
                }
            } else {
                return Err(err("give text, path or url"));
            }
            skipped.sort();
            skipped.dedup();
            Ok(Planned {
                commands: vec![],
                message: format!(
                    "Saved {} recipe(s): {}{}",
                    saved.len(),
                    saved.join(", "),
                    if skipped.is_empty() {
                        String::new()
                    } else {
                        format!(" (not mapped: {})", skipped.join(", "))
                    }
                ),
            })
        }
        "batch_export" => {
            use emulsion_recipes::store;
            let out_dir = std::path::PathBuf::from(
                args.get("out_dir")
                    .and_then(Value::as_str)
                    .ok_or_else(|| err("missing string 'out_dir'"))?,
            );
            let mut paths: Vec<std::path::PathBuf> = args
                .get("paths")
                .and_then(Value::as_array)
                .map(|a| {
                    a.iter()
                        .filter_map(|v| v.as_str())
                        .map(std::path::PathBuf::from)
                        .collect()
                })
                .unwrap_or_default();
            if let Some(folder) = args.get("folder").and_then(Value::as_str) {
                let mut listed: Vec<std::path::PathBuf> = std::fs::read_dir(folder)
                    .map_err(|e| err(format!("{folder}: {e}")))?
                    .flatten()
                    .map(|e| e.path())
                    .filter(|p| {
                        p.is_file()
                            && p.extension()
                                .map(|e| e.to_string_lossy().to_ascii_lowercase())
                                .is_some_and(|e| {
                                    emulsion_io::OPEN_EXTENSIONS.contains(&e.as_str()) && e != "svg"
                                })
                    })
                    .collect();
                listed.sort();
                paths.extend(listed);
            }
            if paths.is_empty() {
                return Err(err("no pictures: give folder or paths"));
            }
            let recipe = match args.get("recipe").and_then(Value::as_str) {
                Some(name) => Some(
                    store::find(&emulsion_io::recent::data_dir().join("recipes"), name)
                        .ok_or_else(|| {
                            err(format!("no recipe named {name:?}; call list_recipes"))
                        })?,
                ),
                None => None,
            };
            let ext = if args.get("format").and_then(Value::as_str) == Some("png") {
                "png"
            } else {
                "jpg"
            };
            std::fs::create_dir_all(&out_dir).map_err(|e| err(e.to_string()))?;
            let mut written = Vec::new();
            let mut failed = Vec::new();
            for p in &paths {
                let result = (|| -> Result<std::path::PathBuf, String> {
                    let d = emulsion_io::open(p).map_err(|e| e.to_string())?;
                    let mut ed = Editor::new(d, None);
                    if let Some(r) = &recipe {
                        let compiled =
                            emulsion_recipes::compile_sized(r, ed.doc.width, ed.doc.height)
                                .map_err(|e| e.to_string())?;
                        store::add_to(&mut ed, compiled, Slot::TOP).map_err(|e| e.to_string())?;
                    }
                    let stem = p
                        .file_stem()
                        .map(|s| s.to_string_lossy().to_string())
                        .unwrap_or_else(|| "picture".into());
                    let suffix = recipe
                        .as_ref()
                        .map(|r| {
                            format!(
                                "-{}",
                                r.name
                                    .to_lowercase()
                                    .replace(|c: char| !c.is_ascii_alphanumeric(), "-")
                            )
                        })
                        .unwrap_or_default();
                    let out = out_dir.join(format!("{stem}{suffix}.{ext}"));
                    emulsion_io::export::export(
                        &ed.doc,
                        &out,
                        emulsion_io::export::ExportOptions::for_doc(&ed.doc),
                    )
                    .map_err(|e| e.to_string())?;
                    Ok(out)
                })();
                match result {
                    Ok(o) => written.push(o.display().to_string()),
                    Err(e) => failed.push(format!("{}: {e}", p.display())),
                }
            }
            Ok(Planned {
                commands: vec![],
                message: format!(
                    "Exported {} of {} to {}{}",
                    written.len(),
                    paths.len(),
                    out_dir.display(),
                    if failed.is_empty() {
                        String::new()
                    } else {
                        format!("; failed: {}", failed.join("; "))
                    }
                ),
            })
        }
        "lens_profile" => {
            let id = match args.get("node").and_then(Value::as_u64) {
                Some(id) => id,
                None => doc
                    .nodes
                    .iter()
                    .rev()
                    .find(|n| matches!(n.kind, NodeKind::Raster { .. } | NodeKind::Smart { .. }))
                    .map(|n| n.id)
                    .ok_or_else(|| err("no pixel node to correct"))?,
            };
            let n = doc.node(id).ok_or_else(|| err(format!("no node {id}")))?;
            let (is_pixels, mut filters) = match &n.kind {
                NodeKind::Raster { .. } => (true, Vec::new()),
                NodeKind::Smart { filters, .. } => (false, filters.clone()),
                _ => return Err(err(format!("{} is not a pixel node", node_label(doc, id)))),
            };
            let info = doc
                .info
                .clone()
                .ok_or_else(|| err("this picture carries no camera data (EXIF)"))?;
            if !emulsion_io::lensfun::installed() {
                return Err(err(
                    "the lens database is not installed; download_model lensfun (5 MB) first",
                ));
            }
            let db = emulsion_io::lensfun::Database::load().map_err(|e| err(e.to_string()))?;
            let p = emulsion_io::lensfun::profile_for(
                &db,
                &info.make,
                &info.model,
                &info.lens,
                info.focal_mm,
                info.f_number,
            )
            .ok_or_else(|| {
                err(format!(
                    "no profile for {:?} on {} {}",
                    info.lens, info.make, info.model
                ))
            })?;
            let strength = args
                .get("strength")
                .and_then(Value::as_f64)
                .unwrap_or(100.0) as f32;
            let [a, b, c] = p.distortion.unwrap_or([0.0; 3]);
            let [k1, k2, k3] = p.vignetting.unwrap_or([0.0; 3]);
            filters.push(emulsion_filters::Filter::LensProfile {
                a,
                b,
                c,
                k1,
                k2,
                k3,
                scale: p.scale,
                distortion: strength,
                vignette: strength,
            });
            let mut commands = Vec::new();
            if is_pixels {
                commands.push(Command::ConvertToSmart { id });
            }
            commands.push(Command::SetFilters { id, filters });
            Ok(Planned {
                commands,
                message: format!(
                    "Applied the {} profile ({}{}) to {}",
                    p.lens,
                    if p.distortion.is_some() {
                        "distortion"
                    } else {
                        ""
                    },
                    if p.vignetting.is_some() {
                        if p.distortion.is_some() {
                            " + vignetting"
                        } else {
                            "vignetting"
                        }
                    } else {
                        ""
                    },
                    node_label(doc, id)
                ),
            })
        }
        "download_model" => {
            let id = args
                .get("id")
                .and_then(Value::as_str)
                .ok_or_else(|| err("missing string 'id'"))?;
            if id == "lensfun" {
                let cancel = std::sync::atomic::AtomicBool::new(false);
                emulsion_io::lensfun::install(&|_, _| {}, &cancel)
                    .map_err(|e| err(e.to_string()))?;
                return Ok(Planned {
                    commands: vec![],
                    message: "Installed the lensfun lens database".into(),
                });
            }
            let spec = emulsion_ai::models::spec(id)
                .ok_or_else(|| err(format!("unknown model {id:?}; see list_models")))?;
            let cancel = std::sync::atomic::AtomicBool::new(false);
            emulsion_ai::models::download(spec, &|_, _| {}, &cancel)
                .map_err(|e| err(e.to_string()))?;
            Ok(Planned {
                commands: vec![],
                message: format!(
                    "Installed {} ({})",
                    spec.name,
                    emulsion_ai::models::human_bytes(spec.total_bytes())
                ),
            })
        }
        "select_subject" => {
            if emulsion_ai::matte::available().is_none() {
                return Err(err(
                    "no subject matte model is installed; download_model rmbg14 (or isnet) first",
                ));
            }
            let img = doc_raster(doc);
            let job = emulsion_ai::jobs::Job::new();
            let m = emulsion_ai::matte::matte(&img, &Default::default(), &job)
                .map_err(|e| err(e.to_string()))?;
            let m = emulsion_ai::matte::harden(&m, 20, 235);
            let (c, msg) = selection_command(doc, m, combine_arg(args), 0.0);
            Ok(Planned {
                commands: vec![c],
                message: msg,
            })
        }
        "select_by_points" => {
            if emulsion_ai::sam::available().is_none() {
                return Err(err(
                    "SlimSAM is not installed; download_model slimsam first",
                ));
            }
            let pts = |k: &str, positive: bool| -> Vec<emulsion_ai::sam::Point> {
                args.get(k)
                    .and_then(Value::as_array)
                    .map(|a| {
                        a.iter()
                            .filter_map(|p| {
                                let p = p.as_array()?;
                                Some(emulsion_ai::sam::Point {
                                    x: p.first()?.as_f64()? as f32,
                                    y: p.get(1)?.as_f64()? as f32,
                                    positive,
                                })
                            })
                            .collect()
                    })
                    .unwrap_or_default()
            };
            let mut points = pts("points", true);
            points.extend(pts("negative", false));
            let bbox = args.get("box").and_then(Value::as_array).and_then(|b| {
                if b.len() == 4 {
                    Some((
                        b[0].as_f64()? as f32,
                        b[1].as_f64()? as f32,
                        b[2].as_f64()? as f32,
                        b[3].as_f64()? as f32,
                    ))
                } else {
                    None
                }
            });
            if points.is_empty() && bbox.is_none() {
                return Err(err("give points and/or a box"));
            }
            let img = doc_raster(doc);
            let job = emulsion_ai::jobs::Job::new();
            let emb = emulsion_ai::sam::encode(&img, &job).map_err(|e| err(e.to_string()))?;
            let (m, score) =
                emulsion_ai::sam::decode(&emb, &points, bbox).map_err(|e| err(e.to_string()))?;
            let m = emulsion_ai::matte::harden(&m, 96, 160);
            let (c, msg) = selection_command(doc, m, combine_arg(args), 0.0);
            Ok(Planned {
                commands: vec![c],
                message: format!("{msg} (confidence {:.0} %)", score * 100.0),
            })
        }
        "remove_background" => {
            if emulsion_ai::matte::available().is_none() {
                return Err(err(
                    "no subject matte model is installed; download_model rmbg14 (or isnet) first",
                ));
            }
            let source = match args.get("node").and_then(Value::as_u64) {
                Some(id) => {
                    let n = doc.node(id).ok_or_else(|| err(format!("no node {id}")))?;
                    match &n.kind {
                        NodeKind::Raster { raster, placement } => {
                            Some((id, n.name.clone(), raster.clone(), *placement, n.parent))
                        }
                        _ => {
                            return Err(err(format!(
                                "{} is not a pixel node",
                                node_label(doc, id)
                            )));
                        }
                    }
                }
                None => None,
            };
            let img: Arc<Raster> = match &source {
                Some((_, _, r, _, _)) => r.clone(),
                None => Arc::new(doc_raster(doc)),
            };
            let job = emulsion_ai::jobs::Job::new();
            let m = emulsion_ai::matte::matte(&img, &Default::default(), &job)
                .map_err(|e| err(e.to_string()))?;
            let cut = emulsion_ai::matte::cut_out(&img, &emulsion_ai::matte::harden(&m, 12, 240));
            let (name, placement, slot, hide) = match &source {
                Some((id, name, _, pl, parent)) => {
                    let sib = doc.children(*parent);
                    let idx = sib.iter().position(|s| s == id).unwrap_or(0) + 1;
                    (
                        format!("{name} cut-out"),
                        *pl,
                        Slot {
                            parent: *parent,
                            index: idx,
                        },
                        Some(*id),
                    )
                }
                None => ("Cut-out".to_string(), Placement::default(), Slot::TOP, None),
            };
            let model_id = emulsion_ai::matte::available()
                .map(|m| m.id)
                .unwrap_or("matte");
            let mut commands = vec![Command::AddNode {
                node: Box::new(
                    Node::raster(0, name.clone(), Arc::new(cut), placement).from_model(model_id),
                ),
                slot,
            }];
            if let Some(id) = hide {
                commands.push(Command::SetVisible { id, visible: false });
            }
            Ok(Planned {
                commands,
                message: format!(
                    "Cut the subject out into {name:?}{}",
                    if hide.is_some() {
                        "; the original is hidden"
                    } else {
                        ""
                    }
                ),
            })
        }

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
        "hatch" => {
            let script = paint_script_for(doc, "hatch", args)?;
            plan_from_script(doc, script)
        }
        "add_filter" | "set_filter" | "remove_filter" => {
            let id = id_arg(args, "node")?;
            let mut filters = smart_filters(doc, id)?;
            match name {
                "add_filter" => {
                    let kind = args
                        .get("kind")
                        .and_then(Value::as_str)
                        .ok_or_else(|| err("missing 'kind'"))?;
                    let mut f = filter_by_kind(kind).ok_or_else(|| {
                        err(format!(
                            "unknown filter {kind:?}; one of: {}",
                            emulsion_filters::Filter::catalogue()
                                .iter()
                                .map(|f| f.key())
                                .collect::<Vec<_>>()
                                .join(", ")
                        ))
                    })?;
                    if let Some(p) = args.get("params").and_then(Value::as_object) {
                        apply_filter_params(&mut f, p)?;
                    }
                    filters.push(f);
                }
                "set_filter" => {
                    let i = args
                        .get("index")
                        .and_then(Value::as_u64)
                        .ok_or_else(|| err("missing integer 'index'"))?
                        as usize;
                    let f = filters
                        .get_mut(i)
                        .ok_or_else(|| err(format!("no filter at index {i}")))?;
                    let p = args
                        .get("params")
                        .and_then(Value::as_object)
                        .ok_or_else(|| err("missing object 'params'"))?;
                    apply_filter_params(f, p)?;
                }
                _ => {
                    let i = args
                        .get("index")
                        .and_then(Value::as_u64)
                        .ok_or_else(|| err("missing integer 'index'"))?
                        as usize;
                    if i >= filters.len() {
                        return Err(err(format!("no filter at index {i}")));
                    }
                    filters.remove(i);
                }
            }
            if filters.len() > 32 {
                return Err(err("at most 32 filters on a layer"));
            }
            let n = filters.len();
            Ok(Planned {
                commands: vec![Command::SetFilters { id, filters }],
                message: format!(
                    "{} now has {n} filter{}",
                    node_label(doc, id),
                    if n == 1 { "" } else { "s" }
                ),
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
            let compiled =
                emulsion_recipes::compile_sized(&recipe, editor.doc.width, editor.doc.height)
                    .map_err(|e| err(e.to_string()))?;
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
        "add_style" | "set_style" | "remove_style" => {
            use emulsion_core::styles::LayerStyle;
            let id = id_arg(args, "node")?;
            let node = editor
                .doc
                .node(id)
                .ok_or_else(|| err(format!("no node {id}")))?;
            let mut styles = node.styles.clone();
            let apply = |s: &mut LayerStyle, args: &Value| -> Result<(), ToolResult> {
                if let Some(p) = args.get("params").and_then(Value::as_object) {
                    for (k, v) in p {
                        let v = v
                            .as_f64()
                            .ok_or_else(|| err(format!("parameter '{k}' must be a number")))?;
                        if !s.set_param(k, v as f32) {
                            return Err(err(format!("{} has no parameter '{k}'", s.label())));
                        }
                    }
                }
                if let Some(c) = args.get("color") {
                    let c = color::premul_to_srgba8(hex_color(c)?);
                    s.set_color([c[0], c[1], c[2]], false);
                }
                if let Some(c) = args.get("color2") {
                    let c = color::premul_to_srgba8(hex_color(c)?);
                    s.set_color([c[0], c[1], c[2]], true);
                }
                Ok(())
            };
            match name {
                "add_style" => {
                    let kind = args
                        .get("kind")
                        .and_then(Value::as_str)
                        .ok_or_else(|| err("missing 'kind'"))?;
                    let k = kind.trim().to_lowercase().replace(['-', ' '], "_");
                    let mut s = LayerStyle::catalogue()
                        .into_iter()
                        .find(|s| s.key() == k)
                        .ok_or_else(|| err(format!("unknown style {kind:?}; one of drop_shadow, inner_shadow, outer_glow, stroke, color_overlay, gradient_overlay")))?;
                    apply(&mut s, args)?;
                    styles.push(s);
                }
                "set_style" => {
                    let i = args
                        .get("index")
                        .and_then(Value::as_u64)
                        .ok_or_else(|| err("missing integer 'index'"))?
                        as usize;
                    let s = styles
                        .get_mut(i)
                        .ok_or_else(|| err(format!("no style at index {i}")))?;
                    apply(s, args)?;
                }
                _ => {
                    let i = args
                        .get("index")
                        .and_then(Value::as_u64)
                        .ok_or_else(|| err("missing integer 'index'"))?
                        as usize;
                    if i >= styles.len() {
                        return Err(err(format!("no style at index {i}")));
                    }
                    styles.remove(i);
                }
            }
            let n = styles.len();
            exec(editor, Command::SetStyles { id, styles })?;
            Ok(ToolResult::text(format!(
                "{} now has {n} style{}",
                node_label(&editor.doc, id),
                if n == 1 { "" } else { "s" }
            )))
        }
        "convert_to_smart" => {
            let id = id_arg(args, "node")?;
            let kind = editor
                .doc
                .node(id)
                .map(|n| n.kind.tag())
                .ok_or_else(|| err(format!("no node {id}")))?;
            match kind {
                "px" => {
                    exec(editor, Command::ConvertToSmart { id })?;
                    Ok(ToolResult::text(format!(
                        "{} is now a smart layer; add filters with add_filter",
                        node_label(&editor.doc, id)
                    )))
                }
                "smart" => {
                    exec(editor, Command::Rasterize { id })?;
                    Ok(ToolResult::text(format!(
                        "{} was rasterized; its filters are baked in",
                        node_label(&editor.doc, id)
                    )))
                }
                _ => Err(err(format!(
                    "{} has no pixels to filter",
                    node_label(&editor.doc, id)
                ))),
            }
        }
        "add_text" => {
            let (w, h) = (editor.doc.width, editor.doc.height);
            let mut spec = emulsion_core::text::TextSpec {
                color: [10, 10, 11, 255],
                ..Default::default()
            };
            text_args(&mut spec, args)?;
            if spec.text.trim().is_empty() {
                return Err(err("missing string 'text'"));
            }
            let name = args
                .get("name")
                .and_then(Value::as_str)
                .map(str::to_string)
                .unwrap_or_else(|| spec.label());
            let node = Node::text(0, name.clone(), spec, w, h);
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
                "Added text {name:?} as node {id}"
            )))
        }
        "set_text" => {
            let id = id_arg(args, "node")?;
            let spec = match &editor
                .doc
                .node(id)
                .ok_or_else(|| err(format!("no node {id}")))?
                .kind
            {
                NodeKind::Text { spec, .. } => (**spec).clone(),
                _ => {
                    return Err(err(format!(
                        "{} is not a text layer",
                        node_label(&editor.doc, id)
                    )));
                }
            };
            let mut new = spec.clone();
            text_args(&mut new, args)?;
            if new == spec {
                return Ok(ToolResult::text("Nothing to change"));
            }
            exec(
                editor,
                Command::SetText {
                    id,
                    spec: Box::new(new),
                },
            )?;
            Ok(ToolResult::text(format!(
                "Updated text {}",
                node_label(&editor.doc, id)
            )))
        }
        "list_fonts" => {
            let fonts = emulsion_core::text::font_families();
            Ok(ToolResult::text(
                serde_json::to_string_pretty(&json!({ "count": fonts.len(), "fonts": fonts }))
                    .unwrap_or_default(),
            ))
        }
        "list_models" => {
            let list: Vec<Value> = emulsion_ai::models::MANIFEST
                .iter()
                .map(|m| {
                    json!({
                        "id": m.id,
                        "name": m.name,
                        "task": m.task.label(),
                        "installed": emulsion_ai::models::status(m) == emulsion_ai::models::Status::Installed,
                        "bytes": m.total_bytes(),
                        "license": m.license,
                        "note": m.note,
                    })
                })
                .collect();
            Ok(ToolResult::text(
                serde_json::to_string_pretty(&json!({
                    "provider": emulsion_ai::runner::provider().label(),
                    "models": list,
                }))
                .unwrap_or_default(),
            ))
        }
        "set_lock" => {
            let id = id_arg(args, "node")?;
            let locked = args
                .get("locked")
                .and_then(Value::as_bool)
                .ok_or_else(|| err("missing boolean 'locked'"))?;
            exec(editor, Command::SetLocked { id, locked })?;
            Ok(ToolResult::text(format!(
                "{} {}",
                node_label(&editor.doc, id),
                if locked { "locked" } else { "unlocked" }
            )))
        }
        "set_clip" => {
            let id = id_arg(args, "node")?;
            let clip_to = match args.get("to") {
                None | Some(Value::Null) => None,
                Some(v) => Some(
                    v.as_u64()
                        .ok_or_else(|| err("'to' must be a node id or null"))?,
                ),
            };
            exec(editor, Command::SetClip { id, clip_to })?;
            Ok(ToolResult::text(match clip_to {
                Some(t) => format!(
                    "{} now clips to {}",
                    node_label(&editor.doc, id),
                    node_label(&editor.doc, t)
                ),
                None => format!("{} unclipped", node_label(&editor.doc, id)),
            }))
        }
        "add_mask" => {
            let id = id_arg(args, "node")?;
            let from = args
                .get("from")
                .and_then(Value::as_str)
                .unwrap_or("selection");
            let n = editor
                .doc
                .node(id)
                .ok_or_else(|| err(format!("no node {id}")))?;
            let (w, h, to_doc) = match &n.kind {
                NodeKind::Raster { raster, placement } => (
                    raster.width(),
                    raster.height(),
                    Some(placement.to_doc(raster.width(), raster.height())),
                ),
                _ => (editor.doc.width, editor.doc.height, None),
            };
            let mask = match (from, editor.doc.selection.clone()) {
                ("all", _) | (_, None) => emulsion_raster::Mask::white(w, h),
                (_, Some(sel)) => match to_doc {
                    Some(td) => emulsion_raster::Mask::from_fn(w, h, 0, |x, y| {
                        let p = td.transform_point2(glam::dvec2(x as f64 + 0.5, y as f64 + 0.5));
                        if p.x < 0.0
                            || p.y < 0.0
                            || p.x >= sel.width() as f64
                            || p.y >= sel.height() as f64
                        {
                            0
                        } else {
                            sel.get(p.x as u32, p.y as u32)
                        }
                    }),
                    None => (*sel).clone(),
                },
            };
            exec(
                editor,
                Command::SetMask {
                    id,
                    mask: Some(Arc::new(mask)),
                },
            )?;
            Ok(ToolResult::text(format!(
                "Mask added to {} (white reveals, black hides)",
                node_label(&editor.doc, id)
            )))
        }
        "remove_mask" => {
            let id = id_arg(args, "node")?;
            exec(editor, Command::SetMask { id, mask: None })?;
            Ok(ToolResult::text(format!(
                "Mask removed from {}",
                node_label(&editor.doc, id)
            )))
        }
        "set_mask_enabled" => {
            let id = id_arg(args, "node")?;
            let enabled = args
                .get("enabled")
                .and_then(Value::as_bool)
                .ok_or_else(|| err("missing boolean 'enabled'"))?;
            exec(editor, Command::SetMaskEnabled { id, enabled })?;
            Ok(ToolResult::text(format!(
                "Mask on {} {}",
                node_label(&editor.doc, id),
                if enabled { "enabled" } else { "disabled" }
            )))
        }
        "rasterize" => {
            let id = id_arg(args, "node")?;
            exec(editor, Command::Rasterize { id })?;
            Ok(ToolResult::text(format!(
                "Rasterized {}",
                node_label(&editor.doc, id)
            )))
        }
        "save_document" => {
            let path = match args.get("path").and_then(Value::as_str) {
                Some(p) => std::path::PathBuf::from(p),
                None => editor.path.clone().ok_or_else(|| {
                    err("the document has no file yet; pass a path ending in .ora")
                })?,
            };
            emulsion_io::save(&editor.doc, &path).map_err(|e| err(e.to_string()))?;
            editor.path = Some(path.clone());
            Ok(ToolResult::text(format!("Saved {}", path.display())))
        }
        "export_image" => {
            let path = args
                .get("path")
                .and_then(Value::as_str)
                .ok_or_else(|| err("missing string 'path'"))?;
            let mut opts = emulsion_io::export::ExportOptions::for_doc(&editor.doc);
            if let Some(q) = args.get("quality").and_then(Value::as_u64) {
                opts.jpeg_quality = q.clamp(1, 100) as u8;
            }
            emulsion_io::export::export(&editor.doc, std::path::Path::new(path), opts)
                .map_err(|e| err(e.to_string()))?;
            Ok(ToolResult::text(format!("Exported {path}")))
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
        "list_brushes" => crate::brush_discovery::list(args),
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
        "critique" => crate::review::critique(&editor.doc, args),
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
                let given = v
                    .as_str()
                    .ok_or_else(|| err("lut_file must be a path or URL of a .cube file"))?;
                // A URL is fetched once into the LUT library.
                let owned: String;
                let path: &str = if given.starts_with("http://") || given.starts_with("https://") {
                    let name = given
                        .rsplit('/')
                        .next()
                        .filter(|n| !n.is_empty())
                        .unwrap_or("lut.cube");
                    let dir = emulsion_io::recent::data_dir().join("luts");
                    std::fs::create_dir_all(&dir).map_err(|e| err(e.to_string()))?;
                    let dest = dir.join(name);
                    if !dest.exists() {
                        let body = emulsion_recipes::import::fetch(given).map_err(err)?;
                        std::fs::write(&dest, body).map_err(|e| err(e.to_string()))?;
                    }
                    owned = dest.display().to_string();
                    &owned
                } else {
                    given
                };
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
                "origin": n.origin,
                "kind": match &n.kind {
                    NodeKind::Raster { .. } => "pixels",
                    NodeKind::Group { .. } => "group",
                    NodeKind::Adjust(_) => "adjustment",
                    NodeKind::Fill { .. } => "fill",
                    NodeKind::Path { .. } => "path",
                    NodeKind::Text { .. } => "text",
                    NodeKind::Smart { .. } => "smart",
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
            if !n.styles.is_empty() {
                let st: Vec<Value> = n
                    .styles
                    .iter()
                    .enumerate()
                    .map(|(i, s)| {
                        let params: Map<String, Value> = s.params().into_iter().map(|p| (p.key.to_string(), json!(p.value))).collect();
                        json!({ "index": i, "kind": s.key(), "colors": s.colors().iter().map(|c| format!("#{:02X}{:02X}{:02X}", c[0], c[1], c[2])).collect::<Vec<_>>(), "params": params })
                    })
                    .collect();
                o.insert("styles".into(), Value::Array(st));
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
                NodeKind::Smart { source, filters, placement, .. } => {
                    o.insert("pixels".into(), json!(format!("{}×{}", source.width(), source.height())));
                    o.insert("placement".into(), json!({ "x": placement.x, "y": placement.y, "scale": placement.scale_x.abs() * 100.0, "rotation": placement.rotation }));
                    let fs: Vec<Value> = filters
                        .iter()
                        .enumerate()
                        .map(|(i, f)| {
                            let params: Map<String, Value> = f.params().into_iter().map(|s| (s.key.to_string(), json!(s.value))).collect();
                            json!({ "index": i, "kind": f.key(), "params": params })
                        })
                        .collect();
                    o.insert("filters".into(), Value::Array(fs));
                }
                NodeKind::Path { path, style, .. } => {
                    o.insert("d".into(), json!(path.to_svg()));
                    o.insert("anchors".into(), json!(path.anchor_count()));
                    o.insert("stroke".into(), json!(style.stroke.map(hex)));
                    o.insert("stroke_width".into(), json!(style.width));
                    o.insert("fill".into(), json!(style.fill.map(hex)));
                }
                NodeKind::Text { spec, .. } => {
                    o.insert("text".into(), json!(spec.text));
                    o.insert("x".into(), json!(spec.x));
                    o.insert("y".into(), json!(spec.y));
                    o.insert("size".into(), json!(spec.size));
                    o.insert("color".into(), json!(hex(spec.color)));
                    o.insert("font".into(), json!(spec.font));
                    o.insert("bold".into(), json!(spec.bold));
                    o.insert("italic".into(), json!(spec.italic));
                    o.insert("align".into(), json!(spec.align.key()));
                    o.insert("width".into(), json!(spec.width));
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
    let looks_like = {
        let k = emulsion_ai::kind::classify(doc);
        json!({ "kind": k.kind.label(), "confidence": k.confidence, "evidence": k.evidence })
    };
    json!({
        "canvas": { "width": doc.width, "height": doc.height },
        "camera": doc.info.as_ref().map(|i| i.summary()),
        "looks_like": looks_like,
        "selection": selection,
        "rows": "row 1 is the top of the stack; depth > 0 means inside the group listed above it",
        "nodes": nodes,
        "recent_history": history,
    })
}

/// Render the composite (or one node) as a base64 PNG image block.
pub fn view(doc: &Document, args: &Value) -> Result<ToolResult, ToolResult> {
    crate::preview::view(doc, args)
}

/// Image-bearing read tools can run against a snapshot off the UI thread.
pub fn inspect(doc: &Document, name: &str, args: &Value) -> Result<ToolResult, ToolResult> {
    match name {
        "get_view" => view(doc, args),
        "critique" => crate::review::critique(doc, args),
        "list_brushes" => crate::brush_discovery::list(args),
        _ => Err(err(format!("not an inspection tool: {name}"))),
    }
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
    fn paint_symmetry_and_alpha_lock() {
        let mut d = Document::new(200, 200);
        Command::AddNode {
            node: Box::new(Node::raster(
                0,
                "paint",
                Arc::new(Raster::transparent(200, 200)),
                Placement::default(),
            )),
            slot: Slot::TOP,
        }
        .apply(&mut d)
        .unwrap();
        let mut e = Editor::new(d, None);
        let r = execute(
            &mut e,
            "paint",
            &json!({ "node": 1, "brush": "Maru pen", "color": "#ff0000", "symmetry": 4,
                     "settings": {"size": 8, "hardness": 1.0},
                     "strokes": [{ "points": [[100, 40], [100, 60]] }] }),
        );
        assert!(!r.is_error, "{}", text(&r));
        let px = |e: &Editor, x: u32, y: u32| match &e.doc.node(1).unwrap().kind {
            NodeKind::Raster { raster, .. } => raster.get(x, y)[3],
            _ => 0,
        };
        assert!(px(&e, 100, 50) > 0);
        assert!(px(&e, 150, 100) > 0, "rotated copy");
        assert!(px(&e, 50, 100) > 0);
        assert_eq!(px(&e, 140, 140), 0);
        // Alpha lock: a blue wash over the whole canvas only lands on the red.
        let r = execute(
            &mut e,
            "paint",
            &json!({ "node": 1, "brush": "Maru pen", "color": "#0000ff", "alpha_lock": true,
                     "settings": {"size": 400, "hardness": 1.0},
                     "strokes": [{ "points": [[100, 100], [101, 100]] }] }),
        );
        assert!(!r.is_error, "{}", text(&r));
        assert_eq!(px(&e, 140, 140), 0, "empty stays empty");
        assert!(px(&e, 100, 50) > 0);
        let r = execute(
            &mut e,
            "paint",
            &json!({ "node": 1, "brush": "Maru pen", "color": "#0000ff", "mirror": "z", "strokes": [{ "points": [[1, 1]] }] }),
        );
        assert!(r.is_error);
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
    fn model_tools_list_and_validate_before_running() {
        let mut e = editor();
        let r = execute(&mut e, "list_models", &json!({}));
        assert!(!r.is_error);
        let v: Value = serde_json::from_str(&text(&r)).unwrap();
        assert!(v["models"].as_array().unwrap().len() >= 5);
        assert!(
            v["models"]
                .as_array()
                .unwrap()
                .iter()
                .any(|m| m["id"] == "slimsam")
        );
        // Argument checks come before any model is touched.
        let failed = |r: Result<Planned, ToolResult>| text(&r.err().expect("an error"));
        let r = plan_heavy(&e.doc, "download_model", &json!({ "id": "nope" }));
        assert!(failed(r).contains("unknown model"));
        if emulsion_ai::sam::available().is_some() {
            let r = plan_heavy(&e.doc, "select_by_points", &json!({}));
            assert!(failed(r).contains("points and/or a box"));
        } else {
            let r = plan_heavy(&e.doc, "select_by_points", &json!({ "points": [[1, 1]] }));
            assert!(failed(r).contains("not installed"));
        }
        for t in [
            "select_subject",
            "select_by_points",
            "remove_background",
            "inpaint",
            "depth_map",
            "upscale",
            "restore_faces",
            "lens_profile",
            "download_model",
        ] {
            assert!(crate::tools::HEAVY.contains(&t), "{t} is heavy");
        }
        assert!(crate::tools::READ_ONLY.contains(&"list_models"));
    }

    #[test]
    fn masks_clips_locks_save_and_export_tools() {
        let mut e = editor();
        let id = e.doc.nodes[0].id;
        let r = execute(&mut e, "set_lock", &json!({ "node": id, "locked": true }));
        assert!(!r.is_error && e.doc.node(id).unwrap().locked);
        execute(&mut e, "set_lock", &json!({ "node": id, "locked": false }));
        let r = execute(&mut e, "add_layer", &json!({ "name": "Top" }));
        assert!(!r.is_error);
        let top = e.doc.nodes.last().unwrap().id;
        let r = execute(&mut e, "set_clip", &json!({ "node": top, "to": id }));
        assert!(!r.is_error && e.doc.node(top).unwrap().clip_to == Some(id));
        let r = execute(&mut e, "set_clip", &json!({ "node": top, "to": null }));
        assert!(!r.is_error && e.doc.node(top).unwrap().clip_to.is_none());
        execute(
            &mut e,
            "select_rect",
            &json!({ "x": 10, "y": 10, "width": 50, "height": 30 }),
        );
        let r = execute(
            &mut e,
            "add_mask",
            &json!({ "node": id, "from": "selection" }),
        );
        assert!(!r.is_error, "{}", text(&r));
        let m = e.doc.node(id).unwrap().mask.clone().expect("mask");
        assert_eq!(m.get(20, 20), 255);
        assert_eq!(m.get(150, 50), 0);
        let r = execute(
            &mut e,
            "set_mask_enabled",
            &json!({ "node": id, "enabled": false }),
        );
        assert!(!r.is_error && !e.doc.node(id).unwrap().mask_enabled);
        let r = execute(&mut e, "remove_mask", &json!({ "node": id }));
        assert!(!r.is_error && e.doc.node(id).unwrap().mask.is_none());
        let r = execute(&mut e, "rasterize", &json!({ "node": id }));
        assert!(r.is_error, "plain pixels cannot be rasterized");
        let dir = std::env::temp_dir().join(format!("emulsion-mcp-{}", std::process::id()));
        std::fs::create_dir_all(&dir).unwrap();
        let ora = dir.join("doc.ora");
        let r = execute(
            &mut e,
            "save_document",
            &json!({ "path": ora.to_string_lossy() }),
        );
        assert!(!r.is_error, "{}", text(&r));
        assert!(ora.exists() && e.path.as_deref() == Some(ora.as_path()));
        let png = dir.join("out.png");
        let r = execute(
            &mut e,
            "export_image",
            &json!({ "path": png.to_string_lossy() }),
        );
        assert!(!r.is_error, "{}", text(&r));
        assert!(png.metadata().unwrap().len() > 100);
        let r = plan_heavy(
            &e.doc,
            "import_recipe",
            &json!({ "text": "Film Simulation: Velvia\nGrain Effect: Weak, Small\nHighlight: +1" }),
        );
        assert!(
            r.is_ok() || text(&r.err().unwrap()).contains("name"),
            "text import plans"
        );
        let r = plan_heavy(
            &e.doc,
            "batch_export",
            &json!({ "out_dir": dir.to_string_lossy() }),
        );
        assert!(text(&r.err().expect("needs pictures")).contains("no pictures"));
        std::fs::remove_dir_all(&dir).ok();
        for t in [
            "set_lock",
            "set_clip",
            "add_mask",
            "remove_mask",
            "set_mask_enabled",
            "rasterize",
            "save_document",
            "export_image",
            "import_recipe",
            "batch_export",
        ] {
            assert!(
                crate::tools::definitions().iter().any(|d| d.name == t),
                "{t} is defined"
            );
        }
    }

    #[test]
    fn text_layers_add_edit_and_describe() {
        let mut e = editor();
        let r = execute(
            &mut e,
            "add_text",
            &json!({ "text": "Hello", "x": 10, "y": 5, "size": 30, "color": "#ff0000", "bold": true }),
        );
        assert!(!r.is_error, "{}", text(&r));
        let id = e.doc.nodes.last().unwrap().id;
        assert_eq!(e.doc.nodes.last().unwrap().name, "Hello");
        let NodeKind::Text { spec, cache } = &e.doc.node(id).unwrap().kind else {
            panic!()
        };
        assert!(spec.bold && spec.size == 30.0 && spec.color == [255, 0, 0, 255]);
        let inked = (0..64)
            .flat_map(|y| (0..200).map(move |x| (x, y)))
            .filter(|&(x, y)| cache.get(x, y)[3] > 0)
            .count();
        assert!(inked > 50, "{inked}");
        let d = describe(&e).to_string();
        assert!(d.contains("\"text\":\"Hello\""), "{d}");
        let r = execute(
            &mut e,
            "set_text",
            &json!({ "node": id, "text": "Hello\nworld", "align": "center", "width": 120 }),
        );
        assert!(!r.is_error, "{}", text(&r));
        let NodeKind::Text { spec, .. } = &e.doc.node(id).unwrap().kind else {
            panic!()
        };
        assert_eq!(spec.text, "Hello\nworld");
        assert_eq!(spec.width, Some(120.0));
        let r = execute(
            &mut e,
            "set_text",
            &json!({ "node": id, "align": "sideways" }),
        );
        assert!(r.is_error);
        let r = execute(&mut e, "set_text", &json!({ "node": 1, "text": "x" }));
        assert!(r.is_error && text(&r).contains("not a text layer"));
        let r = execute(&mut e, "list_fonts", &json!({}));
        assert!(!r.is_error && text(&r).contains("fonts"));
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
    fn smart_layers_and_filters() {
        let mut e = editor();
        let r = execute(&mut e, "convert_to_smart", &json!({ "node": 1 }));
        assert!(!r.is_error, "{}", text(&r));
        let r = execute(
            &mut e,
            "add_filter",
            &json!({ "node": 1, "kind": "gaussian blur", "params": { "radius": 6 } }),
        );
        assert!(!r.is_error, "{}", text(&r));
        let r = execute(
            &mut e,
            "add_filter",
            &json!({ "node": 1, "kind": "add_noise" }),
        );
        assert!(!r.is_error);
        let d = describe(&e);
        let me = d["nodes"]
            .as_array()
            .unwrap()
            .iter()
            .find(|n| n["id"] == 1)
            .unwrap()
            .clone();
        assert_eq!(me["kind"], "smart");
        assert_eq!(me["filters"].as_array().unwrap().len(), 2);
        assert_eq!(me["filters"][0]["params"]["radius"], 6.0);
        let r = execute(
            &mut e,
            "set_filter",
            &json!({ "node": 1, "index": 0, "params": { "radius": 2 } }),
        );
        assert!(!r.is_error, "{}", text(&r));
        let r = execute(&mut e, "remove_filter", &json!({ "node": 1, "index": 1 }));
        assert!(!r.is_error);
        let NodeKind::Smart {
            filters,
            cache,
            source,
            ..
        } = &e.doc.node(1).unwrap().kind
        else {
            panic!()
        };
        assert_eq!(filters.len(), 1);
        assert!(cache.width() > source.width(), "the blur spread");
        let r = execute(&mut e, "add_filter", &json!({ "node": 1, "kind": "sepia" }));
        assert!(r.is_error && text(&r).contains("unknown filter"));
        let r = execute(&mut e, "convert_to_smart", &json!({ "node": 1 }));
        assert!(
            !r.is_error && e.doc.node(1).unwrap().kind.tag() == "px",
            "rasterize back"
        );
    }

    #[test]
    fn layer_styles_tools() {
        let mut e = editor();
        let r = execute(
            &mut e,
            "add_style",
            &json!({ "node": 3, "kind": "drop shadow", "params": { "distance": 20, "size": 4 }, "color": "#0000ff" }),
        );
        assert!(!r.is_error, "{}", text(&r));
        let r = execute(&mut e, "add_style", &json!({ "node": 3, "kind": "stroke" }));
        assert!(!r.is_error);
        let d = describe(&e);
        let me = d["nodes"]
            .as_array()
            .unwrap()
            .iter()
            .find(|n| n["id"] == 3)
            .unwrap()
            .clone();
        assert_eq!(me["styles"].as_array().unwrap().len(), 2);
        assert_eq!(me["styles"][0]["colors"][0], "#0000FF");
        let r = execute(
            &mut e,
            "set_style",
            &json!({ "node": 3, "index": 1, "params": { "size": 9 } }),
        );
        assert!(!r.is_error);
        let r = execute(&mut e, "remove_style", &json!({ "node": 3, "index": 0 }));
        assert!(!r.is_error && e.doc.node(3).unwrap().styles.len() == 1);
        let r = execute(&mut e, "add_style", &json!({ "node": 3, "kind": "bevel" }));
        assert!(r.is_error);
    }

    #[test]
    fn bezier_strokes_pressure_envelopes_and_hatch() {
        let mut e = editor();
        let r = execute(&mut e, "add_layer", &json!({ "name": "Ink" }));
        assert!(!r.is_error);
        let id = e.doc.nodes.last().unwrap().id;
        let r = execute(
            &mut e,
            "paint",
            &json!({ "node": id, "brush": "G-pen", "color": "#000000", "strokes": [{ "d": "M 20 20 C 80 0 120 100 180 80", "pressure": [0.2, 1.0] }] }),
        );
        assert!(!r.is_error, "{}", text(&r));
        let NodeKind::Raster { raster, .. } = &e.doc.node(id).unwrap().kind else {
            panic!()
        };
        // A tapered pen starts as a hairline, so probe a little way in.
        let at = |t: f64| {
            emulsion_raster::vector::cubic_at(
                (20.0, 20.0),
                (80.0, 0.0),
                (120.0, 100.0),
                (180.0, 80.0),
                t,
            )
        };
        for t in [0.15, 0.5, 0.85] {
            let p = at(t);
            assert!(
                raster.get(p.0 as u32, p.1 as u32)[3] > 0,
                "the curve is inked at t={t}"
            );
        }
        assert_eq!(raster.get(100, 20)[3], 0, "nothing away from the curve");
        let r = execute(
            &mut e,
            "hatch",
            &json!({ "node": id, "brush": "HB pencil", "color": "#000000", "rect": [20, 60, 60, 35], "angle": 45, "spacing": 6 }),
        );
        assert!(
            !r.is_error && text(&r).starts_with("Hatched"),
            "{}",
            text(&r)
        );
        let NodeKind::Raster { raster, .. } = &e.doc.node(id).unwrap().kind else {
            panic!()
        };
        let inside = (22..78)
            .step_by(2)
            .flat_map(|x| (62..93).step_by(2).map(move |y| (x, y)))
            .filter(|(x, y)| raster.get(*x, *y)[3] > 2000)
            .count();
        assert!(inside > 80, "hatching covers the rectangle: {inside}");
        assert_eq!(raster.get(150, 95)[3], 0, "nothing outside it");
        let r = execute(&mut e, "hatch", &json!({ "node": id, "color": "#000000" }));
        assert!(r.is_error, "no rect and no selection");
    }

    #[test]
    fn paint_regression_svg_pen_lifts() {
        let mut doc = Document::new(100, 60);
        doc.nodes.push(Node::raster(
            1,
            "Ink",
            Arc::new(Raster::solid(100, 60, [0.0; 4])),
            Placement::default(),
        ));
        let args = json!({"node": 1, "color": "#000000", "settings": {"size": 4},
            "strokes": [{"d": "M 10 20 L 30 20 M 70 20 L 90 20", "pressure": [0.2, 1.0]}]});
        let script = paint_script(&doc, &args).unwrap_or_else(|e| panic!("{}", text(&e)));
        let (raster, _) = script.render(&Raster::solid(100, 60, [0.0; 4]));
        assert_eq!(
            raster.get(50, 20)[3],
            0,
            "a pen lift must not paint a connecting line"
        );
        assert!(raster.get(20, 20)[3] > 0 && raster.get(80, 20)[3] > 0);
        assert_eq!(script.strokes.len(), 2);
        assert_eq!(
            script.length(),
            40.0,
            "playback distance excludes the pen lift"
        );
        for stroke in &script.strokes {
            assert_eq!(stroke.points.first().unwrap().2, Some(0.2));
            assert_eq!(stroke.points.last().unwrap().2, Some(1.0));
        }
    }

    #[test]
    fn paint_regression_sample_merged_is_opt_in_and_excludes_upper_layers() {
        let mut doc = Document::new(80, 60);
        doc.nodes = vec![
            Node::raster(
                1,
                "Red below",
                Arc::new(Raster::solid(80, 60, [1.0, 0.0, 0.0, 1.0])),
                Placement::default(),
            ),
            Node::raster(
                2,
                "Paint",
                Arc::new(Raster::solid(80, 60, [0.0; 4])),
                Placement::default(),
            ),
            Node::raster(
                3,
                "Green above",
                Arc::new(Raster::solid(80, 60, [0.0, 1.0, 0.0, 1.0])),
                Placement::default(),
            ),
        ];
        let mut args = json!({"node": 2, "color": "#0000ff", "settings": {"size": 8, "wetness": 1.0},
            "strokes": [{"points": [[20, 30], [60, 30]]}]});
        let render = |args: &Value| {
            let script = paint_script(&doc, args).unwrap_or_else(|e| panic!("{}", text(&e)));
            script
                .render(&Raster::solid(80, 60, [0.0; 4]))
                .0
                .get(40, 30)
        };
        let local = render(&args);
        assert!(
            local[2] > 50000 && local[0] == 0,
            "default samples the target layer only: {local:?}"
        );
        args["sample_merged"] = json!(true);
        let merged = render(&args);
        assert!(
            merged[0] > 50000 && merged[1] == 0 && merged[2] == 0,
            "wet paint must sample red below, not green above: {merged:?}"
        );
    }

    #[test]
    fn paint_regression_closed_subpaths_keep_closure_and_lifts() {
        let mut doc = Document::new(100, 80);
        doc.nodes.push(Node::raster(
            1,
            "Ink",
            Arc::new(Raster::solid(100, 80, [0.0; 4])),
            Placement::default(),
        ));
        let script = paint_script(
            &doc,
            &json!({"node": 1, "color": "#000000", "settings": {"size": 3},
            "strokes": [{"d": "M 10 10 L 30 10 L 30 30 Z M 70 50 L 90 50 L 90 70 Z"}]}),
        )
        .unwrap_or_else(|e| panic!("{}", text(&e)));
        assert_eq!(script.strokes.len(), 2);
        for stroke in &script.strokes {
            assert_eq!(
                stroke.points.first(),
                stroke.points.last(),
                "each closed subpath closes itself"
            );
        }
        let (raster, _) = script.render(&Raster::solid(100, 80, [0.0; 4]));
        assert_eq!(
            raster.get(50, 37)[3],
            0,
            "no segment from the first closed contour to the second"
        );
        assert!(raster.get(20, 20)[3] > 0 && raster.get(80, 60)[3] > 0);
    }

    #[test]
    fn paint_regression_backdrop_hierarchy_masks_and_target_exclusion() {
        let mut doc = Document::new(80, 60);
        let mut red = Node::raster(
            1,
            "Lower red",
            Arc::new(Raster::solid(80, 60, [1.0, 0.0, 0.0, 1.0])),
            Placement::default(),
        );
        red.parent = Some(5);
        red.opacity = 0.5;
        red.mask = Some(Arc::new(select::rect(80, 60, 0.0, 0.0, 40.0, 60.0)));
        let mut hidden = Node::raster(
            2,
            "Hidden blue",
            Arc::new(Raster::solid(80, 60, [0.0, 0.0, 1.0, 1.0])),
            Placement::default(),
        );
        hidden.parent = Some(5);
        hidden.visible = false;
        let mut target = Node::raster(
            3,
            "Current yellow",
            Arc::new(Raster::solid(80, 60, [0.5, 0.5, 0.0, 0.5])),
            Placement::default(),
        );
        target.parent = Some(5);
        let mut upper = Node::raster(
            4,
            "Upper green",
            Arc::new(Raster::solid(80, 60, [0.0, 1.0, 0.0, 1.0])),
            Placement::default(),
        );
        upper.parent = Some(5);
        let mut group = Node::new(5, "Group", NodeKind::Group { collapsed: false });
        group.opacity = 0.5;
        doc.nodes = vec![red, hidden, target, upper, group];
        let backdrop = lower_layer_backdrop(&doc, 3, glam::DAffine2::IDENTITY);
        let pixel = backdrop(20, 20);
        assert!(
            (pixel[0] - 0.25).abs() < 0.001 && pixel[1] == 0.0 && pixel[2] == 0.0,
            "only masked lower red through its group: {pixel:?}"
        );
        assert_eq!(backdrop(60, 20), [0.0; 4], "lower layer mask is respected");
        assert_eq!(backdrop(-1, 20), [0.0; 4]);
    }

    #[test]
    fn paint_regression_sampling_and_selection_use_document_coordinates() {
        let mut doc = Document::new(100, 80);
        doc.nodes.push(Node::raster(
            1,
            "Red below",
            Arc::new(Raster::solid(100, 80, [1.0, 0.0, 0.0, 1.0])),
            Placement::default(),
        ));
        let placement = Placement {
            x: 10.0,
            y: 10.0,
            scale_x: 2.0,
            scale_y: 2.0,
            ..Placement::default()
        };
        doc.nodes.push(Node::raster(
            2,
            "Scaled paint",
            Arc::new(Raster::solid(40, 30, [0.0; 4])),
            placement,
        ));
        doc.selection = Some(Arc::new(select::rect(100, 80, 10.0, 10.0, 40.0, 60.0)));
        let args = json!({"node": 2, "color": "#0000ff", "sample_merged": true,
            "settings": {"size": 8, "wetness": 1.0}, "strokes": [{"points": [[20, 30], [80, 30]]}]});
        let script = paint_script(&doc, &args).unwrap_or_else(|e| panic!("{}", text(&e)));
        let (raster, _) = script.render(&Raster::solid(40, 30, [0.0; 4]));
        assert!(
            raster.get(10, 10)[0] > 50000,
            "layer (10,10) maps to selected document (30,30)"
        );
        assert_eq!(
            raster.get(30, 10)[3],
            0,
            "layer (30,10) maps outside selection"
        );
        let mapped = lower_layer_backdrop(
            &doc,
            2,
            Placement {
                x: -30.0,
                ..placement
            }
            .to_doc(40, 30),
        );
        assert_eq!(
            mapped(5, 10),
            [0.0; 4],
            "negative mapped document point is outside the canvas"
        );
        assert_eq!(mapped(20, 10), [1.0, 0.0, 0.0, 1.0]);
        let hatch = hatch_to_paint(
            &doc,
            &json!({"node": 2, "sample_merged": true, "rect": [10, 10, 40, 40]}),
        )
        .unwrap_or_else(|e| panic!("{}", text(&e)));
        assert_eq!(hatch["sample_merged"], true);
        assert!(paint_script(&doc, &json!({"sample_merged": "true"})).is_err());
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
