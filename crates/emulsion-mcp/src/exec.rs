//! Run tools against an open document.

use crate::server::ToolResult;
#[cfg(test)]
use base64::Engine as _;
use emulsion_core::command::{AlignTarget, Alignment, Slot};
use emulsion_core::{Command, Document, Editor, Node, NodeId, NodeKind};
use emulsion_raster::composite::region;
use emulsion_raster::paint::{Brush, Ink, Stroke};
use emulsion_raster::select::{self, Combine};
use emulsion_raster::{Adjustment, Placement};
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
/// Command API for document edits; brush tools commit the independent catalog.
pub fn execute(editor: &mut Editor, name: &str, args: &Value) -> ToolResult {
    if crate::brush_tools::is_tool(name) {
        return crate::brush_tools::execute(name, args);
    }
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
    /// A preview computed with the planned pixels, delivered only after apply succeeds.
    pub(crate) feedback: Option<ToolResult>,
    pub(crate) deferred: Option<crate::raw_tools::SettingsWrite>,
}

/// Apply planned commands on the thread that owns the document.
pub fn apply(editor: &mut Editor, p: Planned) -> ToolResult {
    if let Some(effect) = p.deferred {
        return effect.apply(&editor.doc);
    }
    for c in p.commands {
        if let Err(e) = editor.execute(c) {
            return err(e.to_string());
        }
    }
    p.feedback.unwrap_or_else(|| ToolResult::text(p.message))
}

/// Save captured adjustments without touching the editor or process-global paths.
fn save_recipe(
    doc: &Document,
    args: &Value,
    dir: &std::path::Path,
) -> Result<ToolResult, ToolResult> {
    let id = id_arg(args, "node")?;
    let name = args
        .get("name")
        .and_then(Value::as_str)
        .map(str::trim)
        .filter(|name| !name.is_empty())
        .ok_or_else(|| err("name must be a nonempty string"))?;
    let overwrite = match args.get("overwrite") {
        None => false,
        Some(v) => v
            .as_bool()
            .ok_or_else(|| err("overwrite must be a boolean"))?,
    };
    let excluded = match args.get("exclude_nodes") {
        None => Vec::new(),
        Some(v) => v
            .as_array()
            .ok_or_else(|| err("exclude_nodes must be an array of node ids"))?
            .iter()
            .map(|v| {
                v.as_u64()
                    .ok_or_else(|| err("exclude_nodes must contain integer node ids"))
            })
            .collect::<Result<Vec<_>, _>>()?,
    };
    let mut recipe = emulsion_recipes::capture_adjustments(doc, id, name, &excluded)
        .map_err(|e| err(e.to_string()))?;
    if let Some(tags) = args.get("tags") {
        recipe.tags = tags
            .as_array()
            .ok_or_else(|| err("tags must be an array of strings"))?
            .iter()
            .map(|v| {
                v.as_str()
                    .map(str::to_string)
                    .ok_or_else(|| err("tags must contain strings"))
            })
            .collect::<Result<Vec<_>, _>>()?;
    }
    if let Some(notes) = args.get("notes") {
        recipe.notes = notes
            .as_str()
            .ok_or_else(|| err("notes must be a string"))?
            .to_string();
    }
    let path = if overwrite {
        emulsion_recipes::store::update(dir, name, &recipe)
    } else {
        emulsion_recipes::store::save_new(dir, &recipe)
    }
    .map_err(|e| err(e.to_string()))?;
    let stages = recipe.workflow.as_ref().map_or(0, |w| w.stages.len());
    Ok(ToolResult::text(format!(
        "Saved adjustment recipe {:?} with {stages} stages to {}. The document is unchanged; use apply_recipe or batch_export with this name.",
        recipe.name,
        path.display()
    )))
}

fn recipe_summary(r: &emulsion_recipes::Recipe, origin: &emulsion_recipes::store::Origin) -> Value {
    let saved = matches!(origin, emulsion_recipes::store::Origin::Saved(_));
    let mut summary = json!({
        "name": r.name, "author": r.author, "tags": r.tags, "notes": r.notes,
        "saved": saved, "collection": origin.collection(), "limitations": r.limitations(),
    });
    if let Some(workflow) = &r.workflow {
        summary["kind"] = json!("adjustment_workflow");
        summary["workflow"] = json!({
            "version": workflow.version, "group": workflow.group,
            "stages": workflow.stages.iter().map(|stage| json!({
                "settings": stage.settings, "adjustment": stage.adjustment.label(),
            })).collect::<Vec<_>>(),
            "order": "bottom to top"
        });
    } else {
        summary["kind"] = json!("film_recipe");
        summary["film_simulation"] = json!(r.film_simulation);
        summary["settings"] = json!({
            "dynamic_range": format!("{:?}", r.dynamic_range),
            "grain": format!("{:?} {:?}", r.grain.strength, r.grain.size),
            "color_chrome_effect": format!("{:?}", r.color_chrome_effect),
            "white_balance": format!("{} R{:+} B{:+}", r.white_balance.preset, r.white_balance.red, r.white_balance.blue),
            "highlight": r.highlight, "shadow": r.shadow, "color": r.color,
            "exposure_compensation": r.exposure_compensation,
        });
    }
    summary
}

fn recipe_limitations(recipe: &emulsion_recipes::Recipe) -> String {
    let limitations = recipe.limitations();
    if limitations.is_empty() {
        String::new()
    } else {
        format!("; recipe limitations: {}", limitations.join("; "))
    }
}

/// Reserve a private encoding target; publication below never replaces artwork.
struct BatchExportStage(std::path::PathBuf);

impl BatchExportStage {
    fn new(dir: &std::path::Path, ext: &str) -> std::io::Result<Self> {
        static NEXT: std::sync::atomic::AtomicU64 = std::sync::atomic::AtomicU64::new(0);
        loop {
            let serial = NEXT.fetch_add(1, std::sync::atomic::Ordering::Relaxed);
            let path = dir.join(format!(
                ".emulsion-mcp-batch-{}-{serial}.{ext}",
                std::process::id()
            ));
            match std::fs::OpenOptions::new()
                .write(true)
                .create_new(true)
                .open(&path)
            {
                Ok(_) => return Ok(Self(path)),
                Err(e) if e.kind() == std::io::ErrorKind::AlreadyExists => continue,
                Err(e) => return Err(e),
            }
        }
    }
}

impl Drop for BatchExportStage {
    fn drop(&mut self) {
        let _ = std::fs::remove_file(&self.0);
    }
}

fn export_batch_new(
    doc: &Document,
    dir: &std::path::Path,
    stem: &str,
    ext: &str,
    export: crate::export_tools::ExportRequest,
) -> Result<std::path::PathBuf, String> {
    let stage = BatchExportStage::new(dir, ext).map_err(|e| e.to_string())?;
    export.write(doc, &stage.0)?;
    for serial in 0u64.. {
        let name = if serial == 0 {
            format!("{stem}.{ext}")
        } else {
            format!("{stem}-{serial}.{ext}")
        };
        let path = dir.join(name);
        match std::fs::hard_link(&stage.0, &path) {
            Ok(()) => return Ok(path),
            Err(e) if e.kind() == std::io::ErrorKind::AlreadyExists => continue,
            Err(_) => {
                // FAT/exFAT may not support links. Exclusive creation still
                // protects existing files; remove only our partial output.
                let mut out = match std::fs::OpenOptions::new()
                    .write(true)
                    .create_new(true)
                    .open(&path)
                {
                    Ok(out) => out,
                    Err(e) if e.kind() == std::io::ErrorKind::AlreadyExists => continue,
                    Err(e) => return Err(e.to_string()),
                };
                if let Err(e) = std::fs::File::open(&stage.0)
                    .and_then(|mut input| std::io::copy(&mut input, &mut out))
                    .and_then(|_| out.sync_all())
                {
                    drop(out);
                    let _ = std::fs::remove_file(&path);
                    return Err(e.to_string());
                }
                return Ok(path);
            }
        }
    }
    unreachable!()
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

type ResolvedBrush = (
    Brush,
    String,
    Option<(Brush, emulsion_raster::paint::DualBlend)>,
);

fn merge_known_brush_settings(
    base: &mut Value,
    patch: &Value,
    path: &str,
) -> Result<(), ToolResult> {
    if let Some(fields) = patch.as_object() {
        let target = base
            .as_object_mut()
            .ok_or_else(|| err(format!("{path} does not accept nested settings")))?;
        for (key, value) in fields {
            let next = format!("{path}.{key}");
            let original = target
                .get_mut(key)
                .ok_or_else(|| err(format!("unknown brush setting {next:?}")))?;
            merge_known_brush_settings(original, value, &next)?;
        }
    } else {
        *base = patch.clone();
    }
    Ok(())
}

pub(crate) fn apply_brush_settings(brush: Brush, s: &Value) -> Result<Brush, ToolResult> {
    let obj = s
        .as_object()
        .ok_or_else(|| err("settings must be an object"))?;
    // Merge over the brush's JSON so any field can be set.
    let mut base = serde_json::to_value(brush).map_err(|e| err(e.to_string()))?;
    for (k, v) in obj {
        if base.get(k).is_none() {
            return Err(err(format!("unknown brush setting {k:?}")));
        }
        base[k] = if k == "blend" {
            let label = v
                .as_str()
                .ok_or_else(|| err("brush blend must be a mode name"))?;
            let blend = emulsion_raster::paint::BrushBlend::parse(label)
                .ok_or_else(|| err(format!("unknown brush blend mode {label:?}")))?;
            serde_json::to_value(blend).map_err(|e| err(e.to_string()))?
        } else if v.is_object() {
            let mut merged = base[k].clone();
            merge_known_brush_settings(&mut merged, v, k)?;
            merged
        } else {
            v.clone()
        };
    }
    let brush = serde_json::from_value::<Brush>(base)
        .map_err(|e| err(format!("bad settings: {e}")))?
        .sanitized();
    Ok(brush)
}

/// A brush by stable ID or name with overrides, its library category, and
/// optional secondary component. The catalog is loaded once per paint call.
fn resolve_brush(
    name: Option<&Value>,
    settings: Option<&Value>,
    fallback: &Brush,
    catalog: Option<&emulsion_io::brush_library::Catalog>,
) -> Result<ResolvedBrush, ToolResult> {
    let name = name
        .map(|v| {
            v.as_str()
                .filter(|s| !s.trim().is_empty())
                .ok_or_else(|| err("brush must be a nonempty ID or name from list_brushes"))
        })
        .transpose()?;
    let (mut brush, category, secondary) = match name {
        Some(n) => {
            let catalog = catalog.ok_or_else(|| err("Brush catalog was not loaded"))?;
            let definition = if let Some(definition) = catalog.brush(n) {
                definition
            } else {
                let want = n.trim().to_lowercase();
                let found: Vec<_> = catalog
                    .brushes
                    .iter()
                    .filter(|b| b.name.to_lowercase() == want)
                    .collect();
                if found.len() > 1 {
                    return Err(err(format!(
                        "Several brushes are named {n:?}; use a brush ID from list_brushes"
                    )));
                }
                found
                    .first()
                    .copied()
                    .ok_or_else(|| err(format!("no brush named {n:?}; call list_brushes")))?
            };
            let p = catalog.preset(&definition.id).expect("validated catalog");
            (
                p.brush,
                p.category,
                definition.secondary.map(|b| (b, definition.combine_mode)),
            )
        }
        None => (*fallback, String::new(), None),
    };
    if let Some(settings) = settings {
        brush = apply_brush_settings(brush, settings)?;
    }
    Ok((brush, category, secondary))
}

/// One resolved stroke of a `paint` call, in layer pixels.
pub struct ScriptStroke {
    pub brush: Brush,
    pub secondary: Option<(Brush, emulsion_raster::paint::DualBlend)>,
    pub ink: Ink,
    /// (x, y, pressure).
    pub points: Vec<(f32, f32, Option<f32>)>,
    pub samples: Option<Vec<emulsion_raster::preview::StrokeSample>>,
    pub seed: Option<u64>,
}

/// Rich input shared by canvas paint and portable brush previews.
pub(crate) fn parse_brush_samples(
    value: &Value,
) -> Result<Vec<emulsion_raster::preview::StrokeSample>, ToolResult> {
    let values = value
        .as_array()
        .filter(|v| !v.is_empty() && v.len() <= 2000)
        .ok_or_else(|| err("samples must contain 1 to 2000 sample objects"))?;
    let mut out = Vec::with_capacity(values.len());
    let mut previous_time = 0.0;
    for (index, value) in values.iter().enumerate() {
        let object = value
            .as_object()
            .ok_or_else(|| err(format!("sample {index} must be an object")))?;
        if let Some(key) = object
            .keys()
            .find(|key| !matches!(key.as_str(), "x" | "y" | "pressure" | "tilt" | "time_ms"))
        {
            return Err(err(format!("sample {index} has unknown field {key:?}")));
        }
        let number = |key: &str| -> Result<f64, ToolResult> {
            value
                .get(key)
                .and_then(Value::as_f64)
                .filter(|v| v.is_finite())
                .ok_or_else(|| err(format!("sample {index} {key} must be a finite number")))
        };
        let x = number("x")? as f32;
        let y = number("y")? as f32;
        if !x.is_finite() || !y.is_finite() {
            return Err(err(format!(
                "sample {index} coordinates exceed the finite range"
            )));
        }
        let pressure = if value.get("pressure").is_some() {
            let p = number("pressure")?;
            if !(0.0..=1.0).contains(&p) {
                return Err(err(format!("sample {index} pressure must be from 0 to 1")));
            }
            Some(p as f32)
        } else {
            None
        };
        let tilt = value
            .get("tilt")
            .map(|v| {
                let a = v
                    .as_array()
                    .filter(|a| a.len() == 2)
                    .ok_or_else(|| err(format!("sample {index} tilt must be [x, y] degrees")))?;
                let axis = |v: &Value| {
                    v.as_f64()
                        .filter(|v| v.is_finite() && (-90.0..=90.0).contains(v))
                        .map(|v| v as f32)
                        .ok_or_else(|| {
                            err(format!(
                                "sample {index} tilt axes must be from -90 to 90 degrees"
                            ))
                        })
                };
                Ok::<_, ToolResult>((axis(&a[0])?, axis(&a[1])?))
            })
            .transpose()?;
        let time_ms = if value.get("time_ms").is_some() {
            number("time_ms")?
        } else if index == 0 {
            0.0
        } else {
            previous_time + 16.0
        };
        if time_ms < 0.0 || time_ms < previous_time || !time_ms.is_finite() {
            return Err(err(format!(
                "sample {index} time_ms must be nonnegative and monotonic"
            )));
        }
        previous_time = time_ms;
        out.push(emulsion_raster::preview::StrokeSample {
            x,
            y,
            pressure,
            tilt,
            time_ms,
        });
    }
    Ok(out)
}

pub(crate) fn parse_dual_blend(
    value: &Value,
) -> Result<emulsion_raster::paint::DualBlend, ToolResult> {
    use emulsion_raster::paint::DualBlend;
    match value.as_str().map(str::to_ascii_lowercase).as_deref() {
        Some("normal") => Ok(DualBlend::Normal),
        Some("multiply") => Ok(DualBlend::Multiply),
        Some("screen") => Ok(DualBlend::Screen),
        _ => Err(err("combine_mode must be Normal, Multiply or Screen")),
    }
}

fn secondary_overrides(
    mut secondary: Option<(Brush, emulsion_raster::paint::DualBlend)>,
    args: &Value,
) -> Result<Option<(Brush, emulsion_raster::paint::DualBlend)>, ToolResult> {
    if let Some(settings) = args.get("secondary_settings") {
        if settings.is_null() {
            secondary = None;
        } else {
            let (brush, blend) = secondary.unwrap_or_default();
            secondary = Some((apply_brush_settings(brush, settings)?, blend));
        }
    }
    if let Some(value) = args.get("combine_mode") {
        let blend = parse_dual_blend(value)?;
        let Some((_, mode)) = &mut secondary else {
            return Err(err("combine_mode requires a secondary brush"));
        };
        *mode = blend;
    }
    Ok(secondary)
}

impl ScriptStroke {
    /// Feed the same resolved input during immediate and animated rendering.
    pub fn feed_point(&self, stroke: &mut Stroke, index: usize) {
        if let Some(samples) = &self.samples {
            let sample = samples[index];
            stroke.point_full(
                sample.x,
                sample.y,
                sample.pressure,
                sample.tilt,
                Some(sample.time_ms),
            );
        } else {
            let (x, y, pressure) = self.points[index];
            stroke.point_at(x, y, pressure, None);
        }
    }
}

/// A `paint` call resolved against a document: everything needed to lay
/// the strokes down, at once or one point at a time.
pub struct PaintScript {
    pub id: NodeId,
    pub strokes: Vec<ScriptStroke>,
    pub clip: Option<emulsion_raster::paint::Clip>,
    /// Keep original alpha while changing colour, including translucent edges.
    pub alpha_lock: bool,
    /// Optional snapshot of visible lower layers, sampled in layer coordinates.
    pub backdrop: Option<emulsion_raster::paint::Backdrop>,
    /// Layer pixels → document pixels.
    pub to_doc: glam::DAffine2,
    /// Mirror axes through the canvas centre, in document pixels.
    pub mirror: (Option<f32>, Option<f32>),
    /// Rotational symmetry about the canvas centre (document pixels), copies.
    pub radial: Option<((f32, f32), u32)>,
    pub label: String,
    pub message: String,
}

impl PaintScript {
    /// Shared stroke setup for immediate rendering and animated UI playback.
    pub fn start_stroke(&self, base: Arc<Raster>, s: &ScriptStroke) -> Stroke {
        let mut stroke = Stroke::new(base, s.brush, s.ink.clone(), self.clip.clone());
        if let Some((secondary, blend)) = s.secondary {
            stroke.set_secondary(secondary, blend);
        }
        if let Some(seed) = s.seed {
            stroke.set_seed(seed);
        }
        stroke.set_alpha_lock(self.alpha_lock);
        stroke.set_symmetry_space(self.to_doc);
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
            for index in 0..s.points.len() {
                s.feed_point(&mut stroke, index);
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
    for k in [
        "brush",
        "color",
        "settings",
        "secondary_settings",
        "combine_mode",
        "seed",
        "sample_merged",
        "mode",
        "alpha_lock",
        "mirror",
        "symmetry",
    ] {
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
    let catalog = if args.get("brush").is_some() || strokes.iter().any(|s| s.get("brush").is_some())
    {
        Some(emulsion_io::brush_library::load().map_err(|e| err(e.to_string()))?)
    } else {
        None
    };
    let (default_brush, default_cat, default_secondary) = resolve_brush(
        args.get("brush"),
        args.get("settings"),
        &Brush::default(),
        catalog.as_ref(),
    )?;
    let default_color = match args.get("color") {
        Some(c) => Some(hex_color(c)?),
        None => None,
    };
    let parse_seed = |value: &Value| {
        value
            .as_u64()
            .ok_or_else(|| err("seed must be an unsigned 64-bit integer"))
    };
    let default_seed = args.get("seed").map(&parse_seed).transpose()?;
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
    let centre = (doc.width as f32 / 2.0, doc.height as f32 / 2.0);
    let mut out = Vec::with_capacity(strokes.len());
    for (i, s) in strokes.iter().enumerate() {
        if !s.is_object() {
            return Err(err(format!("stroke {i} must be an object")));
        }
        let (brush, cat, mut secondary) = if s.get("brush").is_some() {
            resolve_brush(
                s.get("brush"),
                args.get("settings"),
                &default_brush,
                catalog.as_ref(),
            )?
        } else {
            (default_brush, default_cat.clone(), default_secondary)
        };
        secondary = secondary_overrides(secondary, args)?;
        secondary = secondary_overrides(secondary, s)?;
        let seed = s.get("seed").map(&parse_seed).transpose()?.or(default_seed);
        let rich = s.get("samples").map(parse_brush_samples).transpose()?;
        // Named preset, then call settings, then this stroke's overrides.
        let mut brush = resolve_brush(None, s.get("settings"), &brush, catalog.as_ref())?.0;
        brush.size = (brush.size as f64 / scale) as f32;
        // Stabilizing suits a hand, not computed points.
        if rich.is_none() {
            brush.stabilizer = 0.0;
            brush.advanced.stabilization = Default::default();
        }
        if let Some((brush, _)) = &mut secondary {
            brush.size = (brush.size as f64 / scale) as f32;
            if rich.is_none() {
                brush.stabilizer = 0.0;
                brush.advanced.stabilization = Default::default();
            }
        }
        let operation = s
            .get("mode")
            .or_else(|| args.get("mode"))
            .map(|mode| {
                mode.as_str()
                    .filter(|mode| matches!(*mode, "paint" | "erase" | "smudge"))
                    .ok_or_else(|| err("paint mode must be paint, erase or smudge"))
            })
            .transpose()?;
        let operation = operation.unwrap_or(match cat.as_str() {
            "Eraser" => "erase",
            "Smudge" => "smudge",
            _ => "paint",
        });
        let ink = match operation {
            "erase" => Ink::Erase,
            "smudge" => Ink::Smudge,
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
        if ["d", "points", "samples"]
            .iter()
            .filter(|key| s.get(**key).is_some())
            .count()
            != 1
        {
            return Err(err(format!(
                "stroke {i} must give exactly one of d, points or samples"
            )));
        }
        let pressure = |v: &Value| -> Result<f32, ToolResult> {
            v.as_f64()
                .filter(|p| p.is_finite() && (0.0..=1.0).contains(p))
                .map(|p| p as f32)
                .ok_or_else(|| err(format!("stroke {i} pressure must be a number from 0 to 1")))
        };
        let envelope = s
            .get("pressure")
            .map(|v| {
                let env = v.as_array().filter(|a| a.len() == 2).ok_or_else(|| {
                    err(format!("stroke {i} pressure envelope must be [start, end]"))
                })?;
                Ok::<_, ToolResult>((pressure(&env[0])?, pressure(&env[1])?))
            })
            .transpose()?;
        let mut subpaths: Vec<Vec<(f64, f64, Option<f32>)>> = Vec::new();
        if let Some(samples) = &rich {
            subpaths.push(
                samples
                    .iter()
                    .map(|sample| (sample.x as f64, sample.y as f64, sample.pressure))
                    .collect(),
            );
        } else if let Some(d) = s.get("d") {
            let d = d
                .as_str()
                .filter(|d| !d.trim().is_empty())
                .ok_or_else(|| err(format!("stroke {i} d must be nonempty SVG path data")))?;
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
                .filter(|p| !p.is_empty() && p.len() <= 2000)
                .ok_or_else(|| err(format!("stroke {i} points must contain 1 to 2000 points")))?;
            let mut doc_pts = Vec::with_capacity(pts.len());
            for (j, p) in pts.iter().enumerate() {
                let a = p
                    .as_array()
                    .filter(|a| (2..=3).contains(&a.len()))
                    .ok_or_else(|| {
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
                doc_pts.push((x, y, a.get(2).map(&pressure).transpose()?));
            }
            subpaths.push(doc_pts);
        }
        let count = subpaths.iter().map(Vec::len).sum::<usize>();
        if count == 0 {
            return Err(err(format!("stroke {i} has no usable points")));
        }
        if count > 4000 {
            return Err(err(format!("stroke {i} has more than 4000 points")));
        }
        // SVG moveto lifts the pen: pressure and taper restart independently.
        for mut doc_pts in subpaths {
            // A pressure envelope [start, end] fills in points without their own.
            if let Some((p0, p1)) = envelope {
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
            if points
                .iter()
                .any(|(x, y, _)| !x.is_finite() || !y.is_finite())
            {
                return Err(err(format!(
                    "stroke {i} coordinates exceed the layer's finite range"
                )));
            }
            let samples = rich.as_ref().map(|samples| {
                samples
                    .iter()
                    .zip(&points)
                    .map(
                        |(sample, &(x, y, pressure))| emulsion_raster::preview::StrokeSample {
                            x,
                            y,
                            pressure,
                            ..*sample
                        },
                    )
                    .collect()
            });
            out.push(ScriptStroke {
                samples,
                seed,
                secondary,
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
        alpha_lock,
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

/// The `liquify` tool: dabs along a document-space path.
fn plan_liquify(doc: &Document, args: &Value) -> Result<Planned, ToolResult> {
    use emulsion_raster::liquify::{Mode, dab};
    let id = id_arg(args, "node")?;
    let node = doc.node(id).ok_or_else(|| err(format!("no node {id}")))?;
    let NodeKind::Raster { raster, placement } = &node.kind else {
        return Err(err(format!("{} is not a pixel layer", node_label(doc, id))));
    };
    if node.locked {
        return Err(err(format!("{} is locked", node_label(doc, id))));
    }
    let mode = match args.get("mode") {
        None | Some(Value::Null) => Mode::Push,
        Some(Value::String(m)) => Mode::parse(m).ok_or_else(|| {
            err("mode must be push, twirl_cw, twirl_ccw, pinch, expand or restore")
        })?,
        _ => return Err(err("mode must be a string")),
    };
    let size = args.get("size").and_then(Value::as_f64).unwrap_or(80.0);
    let strength = args.get("strength").and_then(Value::as_f64).unwrap_or(0.6);
    if !(2.0..=2000.0).contains(&size) || !(0.0..=1.0).contains(&strength) {
        return Err(err("size must be 2–2000 and strength 0–1"));
    }
    let pts = args
        .get("points")
        .and_then(Value::as_array)
        .filter(|p| !p.is_empty() && p.len() <= 2000)
        .ok_or_else(|| err("points must be 1–2000 [x, y] pairs"))?;
    let to_doc = placement.to_doc(raster.width(), raster.height());
    let to_local = to_doc.inverse();
    let scale = to_local.matrix2.determinant().abs().sqrt().max(1e-6) as f32;
    let mut path: Vec<(f32, f32)> = Vec::with_capacity(pts.len());
    for p in pts {
        let (Some(x), Some(y)) = (
            p.get(0).and_then(Value::as_f64),
            p.get(1).and_then(Value::as_f64),
        ) else {
            return Err(err("each point is [x, y]"));
        };
        let l = to_local.transform_point2(glam::dvec2(x, y));
        path.push((l.x as f32, l.y as f32));
    }
    let radius = size as f32 * scale / 2.0;
    // Dab every quarter radius along the path so pushes stay smooth.
    let step = (radius * 0.25).max(1.0);
    let mut dabs: Vec<((f32, f32), (f32, f32))> = vec![(path[0], (0.0, 0.0))];
    for w in path.windows(2) {
        let (a, b) = (w[0], w[1]);
        let len = (b.0 - a.0).hypot(b.1 - a.1);
        let n = (len / step).ceil().max(1.0) as usize;
        for i in 1..=n {
            let f = i as f32 / n as f32;
            let p = (a.0 + (b.0 - a.0) * f, a.1 + (b.1 - a.1) * f);
            let prev = dabs.last().unwrap().0;
            dabs.push((p, (p.0 - prev.0, p.1 - prev.1)));
        }
    }
    if dabs.len() > 20_000 {
        return Err(err("path too long for one call; split it"));
    }
    let mut current = (**raster).clone();
    let mut dirty = emulsion_raster::IRect::default();
    for (c, delta) in dabs {
        let (r, d) = dab(&current, raster, mode, c, radius, strength as f32, delta);
        current = r;
        dirty = dirty.union(&d);
    }
    if dirty.is_empty() {
        return Err(err("the path missed the layer"));
    }
    Ok(Planned {
        deferred: None,
        feedback: None,
        commands: vec![Command::ReplacePixels {
            id,
            raster: Arc::new(current),
            dirty,
            label: "Liquify".into(),
        }],
        message: format!(
            "Liquified ({}) {} along {} point{}",
            mode.label(),
            node_label(doc, id),
            pts.len(),
            if pts.len() == 1 { "" } else { "s" }
        ),
    })
}

fn plan_from_script(doc: &Document, script: PaintScript) -> Result<Planned, ToolResult> {
    let NodeKind::Raster { raster, .. } = &doc.node(script.id).expect("checked").kind else {
        unreachable!()
    };
    let (current, dirty) = script.render(raster);
    if dirty.is_empty() {
        let message = format!(
            "No pixels changed on {}. Check the selection, alpha lock, stroke location, or brush coverage before adjusting the stroke.",
            node_label(doc, script.id)
        );
        return Ok(Planned {
            deferred: None,
            feedback: Some(paint_feedback(doc, message.clone())),
            commands: Vec::new(),
            message,
        });
    }
    // Render feedback on the same background thread as the paint computation.
    let mut after = doc.clone();
    if let Some(NodeKind::Raster { raster: r, .. }) = after.node_mut(script.id).map(|n| &mut n.kind)
    {
        *r = Arc::new(current.clone());
    }
    let feedback = paint_feedback(&after, script.message.clone());
    Ok(Planned {
        deferred: None,
        feedback: Some(feedback),
        commands: vec![Command::ReplacePixels {
            id: script.id,
            raster: Arc::new(current),
            dirty,
            label: script.label,
        }],
        message: script.message,
    })
}

/// Show the actual composite after a completed paint/hatch call. A missing
/// preview must not turn an already-applied edit into a retryable tool error.
/// Call on a background thread when used from the UI.
pub fn paint_feedback(doc: &Document, message: impl Into<String>) -> ToolResult {
    let mut result = ToolResult::text(message);
    match crate::preview::view(doc, &json!({"max_size": 800})) {
        Ok(preview) => result.content.extend(preview.content),
        Err(error) => result.content.push(json!({
            "type": "text",
            "text": format!("The operation completed, but its preview is unavailable: {}. Inspect with get_view before making visual judgments; do not repeat the paint call.",
                error.content.first().and_then(|b| b["text"].as_str()).unwrap_or("preview failed"))
        })),
    }
    result
}

fn hex(c: [u8; 4]) -> String {
    format!("#{:02X}{:02X}{:02X}", c[0], c[1], c[2])
}

/// Apply validated editable text attributes; absent keys leave them unchanged.
fn text_args(spec: &mut emulsion_core::text::TextSpec, args: &Value) -> Result<(), ToolResult> {
    *spec = crate::text_tools::parse_text_args(args, spec.clone())?;
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

fn smart_filter_styles(
    doc: &Document,
    id: NodeId,
) -> Result<Vec<emulsion_filters::FilterStyle>, ToolResult> {
    match &doc
        .node(id)
        .ok_or_else(|| err(format!("no node {id}")))?
        .kind
    {
        NodeKind::Smart {
            filters,
            filter_styles,
            ..
        } => {
            let mut styles = filter_styles.clone();
            styles.resize(filters.len(), Default::default());
            Ok(styles)
        }
        _ => Err(err(format!("{} is not a smart layer", node_label(doc, id)))),
    }
}

/// Compute a heavy tool against a document snapshot, on any thread.
/// Brushes the person saved or imported, including legacy migration.
pub fn saved_brushes() -> Vec<library::BrushPreset> {
    match emulsion_io::brush_library::load() {
        Ok(catalog) => catalog
            .brushes
            .iter()
            .filter(|b| !b.builtin)
            .filter_map(|b| catalog.preset(&b.id))
            .collect(),
        Err(error) => {
            tracing::warn!(%error, "Could not read brush library");
            Vec::new()
        }
    }
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
    if crate::raw_tools::HEAVY.contains(&name) {
        return crate::raw_tools::plan(doc, name, args);
    }
    let (w, h) = (doc.width, doc.height);
    match name {
        "get_reference_image" => Err(crate::reference::missing_reference()),
        "save_recipe" => {
            let result = save_recipe(doc, args, &emulsion_io::recent::data_dir().join("recipes"))?;
            let message = result
                .content
                .first()
                .and_then(|block| block["text"].as_str())
                .unwrap_or("Recipe saved")
                .to_string();
            Ok(Planned {
                commands: Vec::new(),
                message,
                deferred: None,
                feedback: Some(result),
            })
        }
        "generative_fill" | "generate_image" => {
            let settings = emulsion_io::settings::Settings::load();
            let provider = emulsion_ai::generate::Provider::parse(&settings.image_provider)
                .ok_or_else(|| {
                    err("no image server is set up; the person chooses one under Settings › Image generation")
                })?;
            let cfg = emulsion_ai::generate::Config {
                provider,
                endpoint: (provider == emulsion_ai::generate::Provider::A1111)
                    .then(|| settings.image_endpoint.clone())
                    .flatten(),
                model: match provider {
                    emulsion_ai::generate::Provider::A1111 => settings.image_model.clone(),
                    emulsion_ai::generate::Provider::OpenAi => settings.openai_image_model.clone(),
                    emulsion_ai::generate::Provider::Google => settings.google_image_model.clone(),
                },
                api_key: settings.image_key(provider.id()).map(|(key, _)| key),
            };
            let prompt = args
                .get("prompt")
                .and_then(Value::as_str)
                .map(str::trim)
                .filter(|p| !p.is_empty())
                .ok_or_else(|| err("missing 'prompt'"))?;
            let negative = args.get("negative").and_then(Value::as_str);
            let job = emulsion_ai::jobs::Job::new();
            let model_id = cfg.model_id();
            if name == "generate_image" {
                let layer =
                    emulsion_ai::generate::text_to_image(&cfg, prompt, negative, w, h, &job)
                        .map_err(|e| err(e.to_string()))?;
                let label = args
                    .get("name")
                    .and_then(Value::as_str)
                    .map(str::to_string)
                    .unwrap_or_else(|| {
                        format!("Generated: {}", prompt.chars().take(40).collect::<String>())
                    });
                return Ok(Planned {
                    deferred: None,
                    feedback: None,
                    commands: vec![Command::AddNode {
                        node: Box::new(
                            Node::raster(0, label.clone(), Arc::new(layer), Placement::default())
                                .from_model(&model_id),
                        ),
                        slot: Slot::TOP,
                    }],
                    message: format!("Generated a {w}×{h} layer \"{label}\" from the prompt"),
                });
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
            let (layer, reg) =
                emulsion_ai::generate::fill(&cfg, &img, &hole, prompt, negative, &job)
                    .map_err(|e| err(e.to_string()))?;
            let label = format!("Generated: {}", prompt.chars().take(40).collect::<String>());
            Ok(Planned {
                deferred: None,
                feedback: None,
                commands: vec![Command::AddNode {
                    node: Box::new(
                        Node::raster(
                            0,
                            label.clone(),
                            Arc::new(layer),
                            Placement::at(reg.x as f64, reg.y as f64),
                        )
                        .from_model(&model_id),
                    ),
                    slot: Slot::TOP,
                }],
                message: format!(
                    "Generated {}×{} at {}, {} into a new node \"{label}\"",
                    reg.w, reg.h, reg.x, reg.y
                ),
            })
        }
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
                deferred: None,
                feedback: None,
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
                deferred: None,
                feedback: None,
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
                deferred: None,
                feedback: None,
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
                deferred: None,
                feedback: None,
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
                let (r, unknown) = if trimmed.starts_with('<') && trimmed.contains("crs:") {
                    import::from_xmp(t)
                } else if trimmed.starts_with('<') {
                    import::from_fp1(t)
                } else {
                    import::from_text(t).map_err(err)?
                };
                keep(r, unknown)?;
            } else if let Some(p) = args.get("path").and_then(Value::as_str) {
                for (r, unknown) in import::from_file_many(std::path::Path::new(p)).map_err(err)? {
                    keep(r, unknown)?;
                }
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
                deferred: None,
                feedback: None,
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
            let export = crate::export_tools::ExportRequest::parse(args).map_err(err)?;
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
                        p.is_file() && !emulsion_io::is_svg(p) && emulsion_io::is_openable(p)
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
            let ext = {
                let want = args
                    .get("format")
                    .and_then(Value::as_str)
                    .unwrap_or("jpg")
                    .trim_start_matches('.')
                    .to_ascii_lowercase();
                let want = match want.as_str() {
                    "jpeg" => "jpg".to_string(),
                    "tiff" => "tif".to_string(),
                    _ => want,
                };
                emulsion_io::export::ExportFormat::exportable_extensions()
                    .into_iter()
                    .filter(|e| !matches!(*e, "psd" | "xcf" | "pdf"))
                    .find(|e| *e == want)
                    .ok_or_else(|| {
                        err(format!("format {want:?} is not one this machine can write"))
                    })?
            };
            export
                .validate_path(&out_dir.join(format!("output.{ext}")))
                .map_err(err)?;
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
                    export_batch_new(&ed.doc, &out_dir, &format!("{stem}{suffix}"), ext, export)
                })();
                match result {
                    Ok(o) => written.push(o.display().to_string()),
                    Err(e) => failed.push(format!("{}: {e}", p.display())),
                }
            }
            Ok(Planned {
                deferred: None,
                feedback: None,
                commands: vec![],
                message: format!(
                    "Exported {} of {} to {}{}{}",
                    written.len(),
                    paths.len(),
                    out_dir.display(),
                    if failed.is_empty() {
                        String::new()
                    } else {
                        format!("; failed: {}", failed.join("; "))
                    },
                    recipe.as_ref().map(recipe_limitations).unwrap_or_default(),
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
                deferred: None,
                feedback: None,
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
                    deferred: None,
                    feedback: None,
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
                deferred: None,
                feedback: None,
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
                deferred: None,
                feedback: None,
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
                deferred: None,
                feedback: None,
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
                deferred: None,
                feedback: None,
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
                deferred: None,
                feedback: None,
                commands: vec![c],
                message,
            })
        }
        "paint" => plan_paint(doc, args),
        "liquify" => plan_liquify(doc, args),
        "hatch" => {
            let script = paint_script_for(doc, "hatch", args)?;
            plan_from_script(doc, script)
        }
        "add_filter" | "set_filter" | "remove_filter" => {
            let id = id_arg(args, "node")?;
            let mut filters = smart_filters(doc, id)?;
            let mut styles = smart_filter_styles(doc, id)?;
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
                    styles.push(Default::default());
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
                    if let Some(p) = args.get("params").and_then(Value::as_object) {
                        apply_filter_params(f, p)?;
                    }
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
                    styles.remove(i);
                }
            }
            if name != "remove_filter" {
                let index = if name == "add_filter" {
                    styles.len() - 1
                } else {
                    args.get("index").and_then(Value::as_u64).unwrap() as usize
                };
                if let Some(opacity) = args.get("opacity").and_then(Value::as_f64) {
                    if !(0.0..=1.0).contains(&opacity) {
                        return Err(err("opacity must be 0–1"));
                    }
                    styles[index].opacity = opacity as f32;
                }
                if let Some(mode) = args.get("blend").and_then(Value::as_str) {
                    let blend = crate::blending::mode(&Value::String(mode.to_string()), false)?;
                    styles[index].blend = blend;
                }
            }
            if filters.len() > 32 {
                return Err(err("at most 32 filters on a layer"));
            }
            let n = filters.len();
            Ok(Planned {
                deferred: None,
                feedback: None,
                commands: vec![Command::SetFilterStack {
                    id,
                    filters,
                    styles,
                }],
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
                deferred: None,
                feedback: None,
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
        "describe_raw" => crate::raw_tools::describe(&editor.doc, args),
        "get_raw_preview" => crate::raw_preview::preview(&editor.doc, args),
        "list_raw_documents" | "set_raw_comparison" | "synchronize_raw" => {
            Err(err("This tool needs the live Emulsion workspace host"))
        }
        "format_text_range" | "set_text_path" => crate::text_tools::execute(editor, name, args),
        "draw_shape" | "combine_path" | "resize_path" | "align_path_components" => {
            crate::shape_geometry::execute(editor, name, args)
        }
        "list_shape_stroke_presets" | "save_shape_stroke_preset" | "apply_shape_stroke_preset" => {
            let mut settings = emulsion_io::settings::Settings::load();
            Ok(crate::shape_presets::execute(
                editor,
                name,
                args,
                &mut settings,
            ))
        }
        "describe_document" => Ok(ToolResult::text(
            serde_json::to_string_pretty(&describe(editor)).unwrap_or_default(),
        )),
        "get_view" => view(&editor.doc, args),
        "get_reference_image" => Err(crate::reference::missing_reference()),
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
        "set_blending_options"
        | "set_style_blending"
        | "set_effects_enabled"
        | "set_blend_space" => crate::blending::execute(editor, name, args),
        "set_blend_mode" => {
            let id = id_arg(args, "node")?;
            let m = args
                .get("mode")
                .and_then(Value::as_str)
                .ok_or_else(|| err("missing string 'mode'"))?;
            let node = editor
                .doc
                .node(id)
                .ok_or_else(|| err(format!("no node {id}")))?;
            let blend = crate::blending::mode(&Value::String(m.to_string()), node.is_group())?;
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
        "align_node" => {
            let id = id_arg(args, "node")?;
            let alignment_name = args
                .get("alignment")
                .and_then(Value::as_str)
                .ok_or_else(|| err("missing string 'alignment'"))?;
            let alignment = match alignment_name {
                "left" => Alignment::Left,
                "horizontal_center" => Alignment::HorizontalCenter,
                "right" => Alignment::Right,
                "top" => Alignment::Top,
                "vertical_center" => Alignment::VerticalCenter,
                "bottom" => Alignment::Bottom,
                _ => return Err(err(format!("unknown alignment '{alignment_name}'"))),
            };
            let target_name = match args.get("target") {
                None => "canvas",
                Some(value) => value
                    .as_str()
                    .ok_or_else(|| err("target must be a string"))?,
            };
            let target = match target_name {
                "canvas" => AlignTarget::Canvas,
                "selection" => AlignTarget::Selection,
                _ => return Err(err(format!("unknown alignment target '{target_name}'"))),
            };
            exec(
                editor,
                Command::AlignNode {
                    id,
                    alignment,
                    target,
                },
            )?;
            Ok(ToolResult::text(format!(
                "Aligned {} {alignment_name} to {target_name}",
                node_label(&editor.doc, id)
            )))
        }
        "translate_node" => {
            let id = id_arg(args, "node")?;
            let offset = |key| {
                args.get(key)
                    .and_then(Value::as_f64)
                    .filter(|v| v.is_finite())
                    .ok_or_else(|| err(format!("missing finite number '{key}'")))
            };
            let (dx, dy) = (offset("dx")?, offset("dy")?);
            exec(editor, Command::TranslateNode { id, dx, dy })?;
            Ok(ToolResult::text(format!(
                "Moved {} by ({dx}, {dy}) document pixels",
                node_label(&editor.doc, id)
            )))
        }
        "rotate_node" => {
            let id = id_arg(args, "node")?;
            let degrees = args
                .get("degrees")
                .and_then(Value::as_f64)
                .ok_or_else(|| err("missing number 'degrees'"))?;
            exec(editor, Command::RotateNode { id, degrees })?;
            Ok(ToolResult::text(format!(
                "Rotated {} by {degrees} degrees clockwise",
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
            crate::shape_geometry::ensure_canvas_bounds(editor, args, &path)?;
            let style = crate::shape_style::parse_style(args, Default::default())?;
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
            crate::shape_geometry::ensure_canvas_bounds(editor, args, &path)?;
            style = crate::shape_style::parse_style(args, style)?;
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
                .map(|(r, origin)| recipe_summary(&r, &origin))
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
                let (r, _) = emulsion_recipes::import::from_text(t).map_err(err)?;
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
                "Applied recipe {:?} as group {gid} with {n} adjustment stages{}",
                recipe.name,
                recipe_limitations(&recipe)
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
                        .ok_or_else(|| err(format!("unknown style {kind:?}; one of drop_shadow, inner_shadow, outer_glow, inner_glow, stroke, color_overlay, gradient_overlay, bevel_emboss, satin, pattern_overlay")))?;
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
            match args.get("refresh") {
                None | Some(Value::Bool(false)) => {}
                Some(Value::Bool(true)) => emulsion_core::text::refresh_fonts(),
                Some(_) => return Err(err("refresh must be a boolean")),
            }
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
            if args.get("path").is_none()
                && editor.path.is_none()
                && emulsion_io::raw_settings::sidecar_only(&editor.doc)
                && editor.graph.commits().count() == 1
                && editor.graph.branches().len() == 1
            {
                let path = emulsion_io::raw_settings::suggested_sidecar_path(&editor.doc)
                    .map_err(|e| err(e.to_string()))?;
                emulsion_io::raw_settings::save_sidecar(&editor.doc, &path)
                    .map_err(|e| err(e.to_string()))?;
                editor.mark_sidecar_saved(editor.revision);
                return Ok(ToolResult::text(format!(
                    "Saved RAW settings to {}; original RAW unchanged. Reopening the original restores these settings.",
                    path.display()
                )));
            }
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
            crate::export_tools::ExportRequest::parse(args)
                .and_then(|export| export.write(&editor.doc, std::path::Path::new(path)))
                .map_err(err)?;
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
            if args.get("delete_pixels").and_then(Value::as_bool) == Some(true) {
                exec(editor, Command::TrimToCanvas)?;
            }
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

fn describe_source_geometry(o: &mut Map<String, Value>, w: u32, h: u32, p: &Placement) {
    let bounds = p.doc_bounds(w, h);
    o.insert("source_size".into(), json!({ "width": w, "height": h }));
    o.insert(
        "placement".into(),
        json!({
            "x": p.x, "y": p.y, "scale": p.scale_x.abs() * 100.0,
            "scale_x": p.scale_x, "scale_y": p.scale_y,
            "rotation": p.rotation, "flip_x": p.flip_x, "flip_y": p.flip_y,
        }),
    );
    o.insert(
        "source_bounds".into(),
        json!({ "x": bounds.x, "y": bounds.y, "width": bounds.w, "height": bounds.h }),
    );
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
                "blending": crate::blending::describe(&n.blending),
                "effects_enabled": n.effects_enabled,
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
                        json!({ "index": i, "kind": s.key(), "options": crate::blending::describe_style(&n.style_options.get(i).cloned().unwrap_or_else(|| emulsion_core::style_options::StyleOptions::for_style(s))), "colors": s.colors().iter().map(|c| format!("#{:02X}{:02X}{:02X}", c[0], c[1], c[2])).collect::<Vec<_>>(), "params": params })
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
                    describe_source_geometry(o, raster.width(), raster.height(), placement);
                }
                NodeKind::Fill { rgba } => {
                    o.insert("color".into(), json!(format!("#{:02X}{:02X}{:02X}", rgba[0], rgba[1], rgba[2])));
                }
                NodeKind::Smart { source, filters, filter_styles, placement, .. } => {
                    o.insert("pixels".into(), json!(format!("{}×{}", source.width(), source.height())));
                    describe_source_geometry(o, source.width(), source.height(), placement);
                    let fs: Vec<Value> = filters
                        .iter()
                        .enumerate()
                        .map(|(i, f)| {
                            let params: Map<String, Value> = f.params().into_iter().map(|s| (s.key.to_string(), json!(s.value))).collect();
                            let style = filter_styles.get(i).copied().unwrap_or_default().sanitized();
                            json!({ "index": i, "kind": f.key(), "params": params, "opacity": style.opacity, "blend": style.blend.label() })
                        })
                        .collect();
                    o.insert("filters".into(), Value::Array(fs));
                }
                NodeKind::Path { path, style, .. } => {
                    o.insert("d".into(), json!(path.to_svg()));
                    o.insert("anchors".into(), json!(path.anchor_count()));
                    o.insert("path_style".into(), crate::shape_style::style_json(style));
                    o.insert("path_bounds".into(), json!(emulsion_raster::vector_geometry::bounds(path)));
                    o.insert("components".into(), json!(path.subpaths.iter().enumerate().map(|(index, sub)| {
                        let p = emulsion_raster::vector::Path { subpaths: vec![sub.clone()] };
                        json!({"index":index,"d":p.to_svg(),"bounds":emulsion_raster::vector_geometry::bounds(&p)})
                    }).collect::<Vec<_>>()));
                    o.insert("stroke".into(), json!(style.stroke.map(hex)));
                    o.insert("stroke_width".into(), json!(style.width));
                    o.insert("fill".into(), json!(style.fill.map(hex)));
                }
                NodeKind::Text { spec, .. } => {
                    o.extend(
                        crate::text_tools::text_json(spec)
                            .as_object()
                            .expect("text description is an object")
                            .clone(),
                    );
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
        "blend_space": doc.blend_space,
        "camera": doc.info.as_ref().map(|i| i.summary()),
        "raw": doc.raw.as_ref().map(|raw| json!({
            "node_id": raw.node_id,
            "settings": raw.params,
            "workflow": "Inspect describe_raw and get_view; prefer develop_raw on this source for supported global photo edits before adding layers. RAW controls do not clip to the selection.",
        })),
        "looks_like": looks_like,
        "selection": selection,
        "rows": "row 1 is the top of the stack; depth > 0 means inside the group listed above it",
        "geometry": "source_size is in source pixels; placement x/y are document pixels, scale_x/scale_y are signed factors (1 = 100%), scale is legacy absolute x percent, rotation is clockwise degrees, flip_x/flip_y are additional source flips. source_bounds is the axis-aligned source rectangle in document pixels, rounded outward; it ignores alpha, masks, filters and effects, and is not clipped to the canvas",
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
    if crate::brush_tools::is_read_only(name) {
        let result = crate::brush_tools::execute(name, args);
        return if result.is_error {
            Err(result)
        } else {
            Ok(result)
        };
    }
    match name {
        "get_raw_preview" => crate::raw_preview::preview(doc, args),
        "describe_raw" => crate::raw_tools::describe(doc, args),
        "get_view" => view(doc, args),
        "get_reference_image" => Err(crate::reference::missing_reference()),
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

    fn recipe_test_dir(label: &str) -> std::path::PathBuf {
        let nonce = std::time::SystemTime::now()
            .duration_since(std::time::UNIX_EPOCH)
            .unwrap()
            .as_nanos();
        let dir = std::env::temp_dir().join(format!(
            "emulsion-mcp-{label}-{}-{nonce}",
            std::process::id()
        ));
        std::fs::create_dir_all(&dir).unwrap();
        dir
    }

    #[test]
    fn captured_recipe_saves_updates_and_applies_exact_stages_without_changing_source() {
        let dir = recipe_test_dir("saved-workflow");
        let mut doc = Document::new(8, 8);
        let add = |doc: &mut Document, node: Node, parent| {
            Command::AddNode {
                node: Box::new(node),
                slot: Slot::top_of(parent),
            }
            .apply(doc)
            .unwrap()
            .unwrap()
        };
        add(
            &mut doc,
            Node::raster(
                0,
                "Picture",
                Arc::new(Raster::solid(8, 8, [0.1, 0.2, 0.3, 1.0])),
                Placement::default(),
            ),
            None,
        );
        let picture = doc.clone();
        let group = add(&mut doc, Node::group(0, "Custom tone"), None);
        let mut exposure = Node::adjust(
            0,
            Adjustment::Exposure {
                exposure: 0.7,
                offset: 0.0,
                gamma: 1.0,
            },
        );
        exposure.name = "Lift".into();
        exposure.opacity = 0.65;
        add(&mut doc, exposure, Some(group));
        let mut contrast = Node::adjust(
            0,
            Adjustment::BrightnessContrast {
                brightness: 5.0,
                contrast: 12.0,
            },
        );
        contrast.visible = false;
        let excluded = add(&mut doc, contrast, Some(group));
        let e = Editor::new(doc, None);
        let before = e.doc.clone();
        let revision = e.revision;
        let steps = e.history.len();
        let args = json!({"node": group,"name": "My exact tone", "tags": ["portrait"],"notes": "Two editable stages"});
        let result = save_recipe(&e.doc, &args, &dir).unwrap();
        assert!(text(&result).contains("2 stages"));
        let saved = emulsion_recipes::store::find(&dir, "My exact tone").unwrap();
        assert_eq!(saved.tags, vec!["portrait"]);
        assert_eq!(saved.notes, "Two editable stages");
        assert_eq!(saved.workflow.as_ref().unwrap().stages.len(), 2);
        let origin = emulsion_recipes::store::Origin::Saved(dir.join("x"));
        assert_eq!(
            recipe_summary(&saved, &origin)["kind"],
            "adjustment_workflow"
        );
        assert!(recipe_summary(&saved, &origin)["workflow"]["stages"][0]["adjustment"].is_string());
        let mut target = Editor::new(picture, None);
        let result = execute(
            &mut target,
            "apply_recipe",
            &json!({"text":saved.to_toml()}),
        );
        assert!(!result.is_error, "{}", text(&result));
        assert_eq!(
            region(&target.doc.composite_tree(), IRect::new(0, 0, 8, 8)),
            region(&e.doc.composite_tree(), IRect::new(0, 0, 8, 8))
        );
        assert_eq!(target.history.len(), 1);
        assert!(target.undo());
        assert!(
            save_recipe(&e.doc, &args, &dir).is_err(),
            "new save must not replace existing recipe"
        );
        assert_eq!(
            emulsion_recipes::store::find(&dir, "My exact tone").unwrap(),
            saved
        );
        let mut updated = args.clone();
        updated["overwrite"] = json!(true);
        updated["exclude_nodes"] = json!([excluded]);
        updated["notes"] = json!("Keep visible exposure only");
        save_recipe(&e.doc, &updated, &dir).unwrap();
        let saved = emulsion_recipes::store::find(&dir, "My exact tone").unwrap();
        assert_eq!(saved.workflow.as_ref().unwrap().stages.len(), 1);
        assert_eq!(saved.notes, "Keep visible exposure only");
        for patch in [
            json!({"tags":[1]}),
            json!({"notes":false}),
            json!({"overwrite":"true"}),
            json!({"exclude_nodes":["bad"]}),
            json!({"exclude_nodes":[9999]}),
            json!({"name":" "}),
            json!({"node":9999}),
        ] {
            let mut invalid = updated.clone();
            invalid
                .as_object_mut()
                .unwrap()
                .extend(patch.as_object().unwrap().clone());
            assert!(save_recipe(&e.doc, &invalid, &dir).is_err());
            assert_eq!(
                emulsion_recipes::store::find(&dir, "My exact tone").unwrap(),
                saved
            );
        }
        assert_eq!(e.doc, before);
        assert_eq!(e.revision, revision);
        assert_eq!(e.history.len(), steps);
        assert!(
            crate::tools::definitions()
                .iter()
                .any(|d| d.name == "save_recipe")
        );
        assert!(!crate::tools::READ_ONLY.contains(&"save_recipe"));
        assert!(crate::tools::HEAVY.contains(&"save_recipe"));
        std::fs::remove_dir_all(dir).unwrap();
    }

    #[test]
    fn apply_recipe_rejects_malformed_workflow_text_without_document_changes() {
        let mut e = editor();
        let before = e.doc.clone();
        let revision = e.revision;
        let steps = e.history.len();
        for input in [
            "name = \"Broken\"\n[workflow]\nversion = 999",
            "name = \"Broken\"\nworkflow = \"unsupported\"",
            "name = \"Broken\"\nunknown_stage = true",
            "name = \"Broken\"\nhighlight = nan",
            "name = \"unterminated",
            "[workflow",
        ] {
            let result = execute(&mut e, "apply_recipe", &json!({"text": input}));
            assert!(result.is_error, "must reject {input}: {}", text(&result));
            assert_eq!(e.doc, before);
            assert_eq!(e.revision, revision);
            assert_eq!(e.history.len(), steps);
        }
    }

    #[test]
    fn recipe_tools_report_legacy_settings_that_are_not_applied() {
        let recipe = emulsion_recipes::Recipe {
            name: "Legacy notes".into(),
            sharpness: 2.0,
            noise_reduction: -2.0,
            color_chrome_fx_blue: emulsion_recipes::Strength::Weak,
            ..Default::default()
        };
        let summary = recipe_summary(&recipe, &emulsion_recipes::store::Origin::Starter);
        assert_eq!(summary["kind"], "film_recipe");
        assert_eq!(summary["limitations"].as_array().unwrap().len(), 3);
        let mut e = editor();
        let result = execute(&mut e, "apply_recipe", &json!({"toml":recipe.to_toml()}));
        assert!(!result.is_error, "{}", text(&result));
        assert!(text(&result).contains("stored but not applied"));
    }

    #[test]
    fn batch_export_never_overwrites_sources_or_same_named_outputs() {
        let dir = recipe_test_dir("batch-collisions");
        let a = dir.join("a");
        let b = dir.join("b");
        std::fs::create_dir_all(&a).unwrap();
        std::fs::create_dir_all(&b).unwrap();
        let first = a.join("photo.png");
        let second = b.join("photo.png");
        for (path, color) in [(&first, [255, 0, 0, 255]), (&second, [0, 0, 255, 255])] {
            let image = image::RgbaImage::from_pixel(8, 8, image::Rgba(color));
            std::fs::write(
                path,
                emulsion_io::export::png8(8, 8, image.as_raw()).unwrap(),
            )
            .unwrap();
        }
        let original = std::fs::read(&first).unwrap();
        let existing = a.join("photo-1.png");
        std::fs::write(&existing, b"existing artwork").unwrap();
        let mut e = editor();
        let before = e.doc.clone();
        let revision = e.revision;
        let result = execute(
            &mut e,
            "batch_export",
            &json!({"paths":[first,second],"out_dir":a,"format":"png"}),
        );
        assert!(!result.is_error, "{}", text(&result));
        assert!(
            text(&result).contains("Exported 2 of 2"),
            "{}",
            text(&result)
        );
        assert_eq!(std::fs::read(&first).unwrap(), original);
        assert_eq!(std::fs::read(&existing).unwrap(), b"existing artwork");
        assert_eq!(
            image::open(a.join("photo-2.png"))
                .unwrap()
                .into_rgba8()
                .get_pixel(0, 0)
                .0,
            [255, 0, 0, 255]
        );
        assert_eq!(
            image::open(a.join("photo-3.png"))
                .unwrap()
                .into_rgba8()
                .get_pixel(0, 0)
                .0,
            [0, 0, 255, 255]
        );
        assert_eq!(
            std::fs::read_dir(&a).unwrap().count(),
            4,
            "no private stage files remain"
        );
        assert_eq!(e.doc, before);
        assert_eq!(e.revision, revision);
        assert!(e.history.is_empty());
        std::fs::remove_dir_all(dir).unwrap();
    }

    #[test]
    fn paint_and_hatch_return_the_completed_composite_and_preserve_layers() {
        for (name, args) in [
            (
                "paint",
                json!({"node": 3, "brush": "Maru pen", "color": "#ff0000",
                "settings": {"size": 12}, "strokes": [{"points": [[20,50],[180,50]]}]}),
            ),
            (
                "hatch",
                json!({"node": 3, "brush": "Maru pen", "color": "#ff0000",
                "rect": [20,20,160,60], "spacing": 12}),
            ),
        ] {
            let mut e = editor();
            let before = e.doc.clone();
            // Exercise both entry points used by the headless server and UI.
            let result = if name == "paint" {
                execute(&mut e, name, &args)
            } else {
                let planned = plan_heavy(&e.doc, name, &args)
                    .unwrap_or_else(|error| panic!("{}", text(&error)));
                apply(&mut e, planned)
            };
            assert!(!result.is_error, "{}", text(&result));
            assert!(!text(&result).contains("Critique"));
            let expected = crate::preview::view(&e.doc, &json!({"max_size": 800})).unwrap();
            assert_eq!(result.content[1..], expected.content);
            assert_eq!(result.content[1]["type"], "image");
            let mapping: Value =
                serde_json::from_str(result.content[2]["text"].as_str().unwrap()).unwrap();
            assert_eq!(mapping["document_size"], json!([200, 100]));
            assert_eq!(mapping["image_to_document"]["scale"], json!([1.0, 1.0]));
            assert_eq!(e.doc.nodes.len(), before.nodes.len());
            for id in [1, 2] {
                match (
                    &before.node(id).unwrap().kind,
                    &e.doc.node(id).unwrap().kind,
                ) {
                    (
                        NodeKind::Raster { raster: old, .. },
                        NodeKind::Raster { raster: new, .. },
                    ) => assert!(Arc::ptr_eq(old, new)),
                    _ => panic!("lower layer changed kind"),
                }
            }
            let previous_view = crate::preview::view(&before, &json!({"max_size": 800})).unwrap();
            assert_ne!(
                result.content[1], previous_view.content[0],
                "painting must visibly change the result"
            );
            assert_eq!(e.history.len(), 1, "feedback must not add document edits");
        }
    }

    #[test]
    fn paint_feedback_is_bounded_and_preview_failure_is_not_a_paint_failure() {
        let doc = Document::new(1600, 1000);
        let result = paint_feedback(&doc, "Painted");
        assert!(!result.is_error);
        let bytes = base64::engine::general_purpose::STANDARD
            .decode(result.content[1]["data"].as_str().unwrap())
            .unwrap();
        let image = image::load_from_memory(&bytes).unwrap();
        assert_eq!((image.width(), image.height()), (800, 500));
        let mapping: Value =
            serde_json::from_str(result.content[2]["text"].as_str().unwrap()).unwrap();
        assert_eq!(mapping["image_to_document"]["scale"], json!([2.0, 2.0]));

        let unavailable = paint_feedback(&Document::new(0, 0), "Painted");
        assert!(!unavailable.is_error);
        assert_eq!(text(&unavailable), "Painted");
        assert!(
            unavailable.content[1]["text"]
                .as_str()
                .unwrap()
                .contains("do not repeat")
        );
    }

    #[test]
    fn failed_paint_does_not_return_a_success_preview() {
        let mut e = editor();
        let args = json!({"node": 3, "brush": "Maru pen", "color": "#ff0000",
            "strokes": [{"points": [[20,50],[180,50]]}]});
        let planned =
            plan_heavy(&e.doc, "paint", &args).unwrap_or_else(|error| panic!("{}", text(&error)));
        e.doc.nodes.retain(|node| node.id != 3);
        let failed_apply = apply(&mut e, planned);
        assert!(failed_apply.is_error);
        assert!(
            failed_apply
                .content
                .iter()
                .all(|block| block["type"] != "image")
        );
        let failed_plan = execute(&mut e, "paint", &args);
        assert!(failed_plan.is_error);
        assert!(
            failed_plan
                .content
                .iter()
                .all(|block| block["type"] != "image")
        );
        assert!(e.history.is_empty());
    }

    #[test]
    fn liquify_tool_moves_pixels() {
        let mut d = Document::new(100, 100);
        let mut px = vec![[0u16, 0, 0, 65535]; 100 * 100];
        for (i, p) in px.iter_mut().enumerate() {
            if i % 100 >= 50 {
                *p = [65535, 65535, 65535, 65535];
            }
        }
        let r = Raster::transparent(100, 100)
            .write_rect(emulsion_raster::IRect::new(0, 0, 100, 100), &px);
        Command::AddNode {
            node: Box::new(Node::raster(0, "pic", Arc::new(r), Placement::default())),
            slot: Slot::TOP,
        }
        .apply(&mut d)
        .unwrap();
        let mut e = Editor::new(d, None);
        let r = execute(
            &mut e,
            "liquify",
            &json!({ "node": 1, "mode": "push", "size": 40, "strength": 1.0,
                     "points": [[40, 50], [70, 50]] }),
        );
        assert!(!r.is_error, "{}", text(&r));
        let px = match &e.doc.node(1).unwrap().kind {
            NodeKind::Raster { raster, .. } => raster.get(58, 50)[0],
            _ => 65535,
        };
        assert!(px < 30000, "edge pushed right: {px}");
        let r = execute(
            &mut e,
            "liquify",
            &json!({ "node": 1, "mode": "melt", "points": [[1, 1]] }),
        );
        assert!(r.is_error);
    }

    #[test]
    fn brush_ids_dual_and_nested_overrides_preserve_other_fields() {
        let mut catalog = emulsion_io::brush_library::Catalog::builtin();
        let first = catalog
            .add_brush(
                emulsion_io::brush_library::USER_SET,
                "Duplicate",
                Brush::default(),
            )
            .unwrap();
        let second = catalog
            .add_brush(
                emulsion_io::brush_library::USER_SET,
                "Duplicate",
                Brush {
                    size: 17.,
                    ..Brush::default()
                },
            )
            .unwrap();
        assert!(
            resolve_brush(
                Some(&json!("Duplicate")),
                None,
                &Brush::default(),
                Some(&catalog)
            )
            .is_err()
        );
        let combined = catalog.combine_brushes(&first, &second).unwrap();
        catalog
            .brush_mut(&combined)
            .unwrap()
            .brush
            .advanced
            .shape
            .count = 4;
        catalog
            .brush_mut(&combined)
            .unwrap()
            .brush
            .advanced
            .grain
            .brightness = 0.5;
        let (brush, _, secondary) = resolve_brush(
            Some(&json!(combined)),
            Some(&json!({"advanced":{"shape":{"flip_x":true}}})),
            &Brush::default(),
            Some(&catalog),
        )
        .unwrap_or_else(|e| panic!("{}", text(&e)));
        assert_eq!(secondary.unwrap().0.size, 17.);
        assert_eq!(brush.advanced.shape.count, 4);
        assert!(brush.advanced.shape.flip_x);
        assert_eq!(brush.advanced.grain.brightness, 0.5);
        assert!(
            resolve_brush(
                Some(&json!(first)),
                Some(&json!({"advanced":{"shape":{"nonsense":1}}})),
                &Brush::default(),
                Some(&catalog)
            )
            .is_err()
        );
    }

    #[test]
    fn paint_operation_is_independent_of_brush_category() {
        let e = editor();
        let hatch = hatch_to_paint(&e.doc, &json!({"node":1,"rect":[0,0,20,20],"mode":"erase"}))
            .unwrap_or_else(|e| panic!("{}", text(&e)));
        assert_eq!(hatch["mode"], json!("erase"));
        let args = json!({"node":1,"brush":"Sketch pencil","mode":"erase","strokes":[{"points":[[5,5],[10,10]]},{"mode":"smudge","points":[[5,5],[10,10]]}]});
        let script = paint_script(&e.doc, &args).unwrap_or_else(|e| panic!("{}", text(&e)));
        assert!(matches!(script.strokes[0].ink, Ink::Erase));
        assert!(matches!(script.strokes[1].ink, Ink::Smudge));
        let mut invalid = args;
        invalid["mode"] = json!("unknown");
        assert!(paint_script(&e.doc, &invalid).is_err());
    }

    #[test]
    fn paint_settings_apply_preset_then_call_then_stroke() {
        let e = editor();
        let args = json!({"node": 1, "brush": "Sketch pencil", "color": "#000000",
        "settings": {"size": 6, "flow": 0.2, "opacity": 0.8},
        "strokes": [
            {"brush": "G-pen", "settings": {"opacity": 0.5}, "points": [[10,10]]},
            {"settings": {"size": 9}, "points": [[20,10]]},
            {"brush": "Hard eraser", "settings": {"opacity": 0.4}, "points": [[30,10]]}
        ]});
        let script = paint_script(&e.doc, &args).unwrap();
        assert_eq!(script.strokes[0].brush.size, 6.0);
        assert_eq!(script.strokes[0].brush.flow, 0.2);
        assert_eq!(script.strokes[0].brush.opacity, 0.5);
        assert_eq!(
            script.strokes[0].brush.grain,
            library::find("G-pen").unwrap().brush.grain
        );
        assert_eq!(script.strokes[1].brush.size, 9.0);
        assert_eq!(script.strokes[1].brush.opacity, 0.8);
        assert_eq!(script.strokes[2].brush.size, 6.0);
        assert_eq!(script.strokes[2].brush.opacity, 0.4);
        assert!(matches!(script.strokes[2].ink, Ink::Erase));

        let args = json!({"node": 1, "color": "#000000", "settings": {"blend": "soft light"},
            "strokes": [{"settings": {"blend": "clear"}, "points": [[10,10]]}]});
        let script = paint_script(&e.doc, &args).unwrap();
        assert_eq!(
            script.strokes[0].brush.blend,
            emulsion_raster::paint::BrushBlend::Clear
        );
    }

    #[test]
    fn invalid_paint_arguments_fail_before_any_stroke_or_history_change() {
        let mut e = editor();
        let before = e.doc.clone();
        let revision = e.revision;
        let steps = e.history.len();
        let mut invalid = vec![
            json!(null),
            json!({}),
            json!({"points": []}),
            json!({"points": null}),
            json!({"points": [[1,2,0.5,4]]}),
            json!({"points": [[1]]}),
            json!({"points": [[1,2,"light"]]}),
            json!({"points": [[1,2,null]]}),
            json!({"points": [[1,2,-0.1]]}),
            json!({"points": [[1,2,1.1]]}),
            json!({"points": [["1",2]]}),
            json!({"points": [[1e300,2]]}),
            json!({"d": ""}),
            json!({"d": "   "}),
            json!({"d": 1}),
            json!({"d": "M 1 2 L 3 4", "points": [[1,2]]}),
            json!({"points": [[1,2]], "pressure": [0.5]}),
            json!({"points": [[1,2]], "pressure": [0,1,0]}),
            json!({"points": [[1,2]], "pressure": ["light",1]}),
            json!({"points": [[1,2]], "pressure": [0,1.1]}),
            json!({"points": [[1,2]], "pressure": null}),
            json!({"points": [[1,2]], "brush": 5}),
            json!({"points": [[1,2]], "brush": null}),
            json!({"points": [[1,2]], "brush": " "}),
            json!({"points": [[1,2]], "brush": "nonexistent brush"}),
            json!({"points": vec![[1,2]; 2001]}),
            json!({"samples": []}),
            json!({"samples": [[1,2]]}),
            json!({"samples": [{"x":1,"y":2}], "points":[[1,2]]}),
            json!({"samples": [{"x":1,"y":2}], "d":"M1 2L3 4"}),
            json!({"samples": [{"x":1e300,"y":2}]}),
            json!({"samples": [{"x":1,"y":2,"pressure":1.1}]}),
            json!({"samples": [{"x":1,"y":2,"tilt":[0,91]}]}),
            json!({"samples": [{"x":1,"y":2,"tilt":[0]}]}),
            json!({"samples": [{"x":1,"y":2,"time_ms":-1}]}),
            json!({"samples": [{"x":1,"y":2,"time_ms":20},{"x":3,"y":4,"time_ms":19}]}),
            json!({"samples": [{"x":1,"y":2,"unexpected":true}]}),
            json!({"samples": vec![json!({"x":1,"y":2}); 2001]}),
            json!({"points":[[1,2]],"secondary_settings":{"unknown":2}}),
            json!({"points":[[1,2]],"combine_mode":"Multiply"}),
            json!({"points":[[1,2]],"secondary_settings":{},"combine_mode":"invalid"}),
            json!({"points":[[1,2]],"seed":-1}),
        ];
        let long_svg = format!(
            "M 0 0 {}",
            (1..=4000)
                .map(|i| format!("L {} {}", i % 100, i / 100))
                .collect::<Vec<_>>()
                .join(" ")
        );
        invalid.push(json!({"d": long_svg}));
        for stroke in invalid {
            let result = execute(
                &mut e,
                "paint",
                &json!({"node": 1, "color": "#000000",
                "strokes": [{"points": [[10,10],[20,10]]}, stroke]}),
            );
            assert!(result.is_error, "{}", text(&result));
            assert_eq!(e.doc, before);
            assert_eq!(e.revision, revision);
            assert_eq!(e.history.len(), steps);
        }
        for brush in [json!(null), json!(true), json!("")] {
            let result = execute(
                &mut e,
                "paint",
                &json!({"node": 1, "brush": brush,
                "color": "#000000", "strokes": [{"points": [[10,10]]}]}),
            );
            assert!(result.is_error);
            assert_eq!(e.doc, before);
            assert_eq!(e.revision, revision);
            assert_eq!(e.history.len(), steps);
        }
        assert!(
            paint_script(
                &e.doc,
                &json!({"node": 1, "color": "#000000",
            "strokes": [{"points": vec![[1,2]; 2000], "pressure": [0,1]}]})
            )
            .is_ok()
        );
    }

    #[test]
    fn paint_rich_samples_match_dual_preview_and_incremental_feed() {
        use emulsion_raster::preview::{PreviewMode, render_dual_stroke};
        let base = Arc::new(Raster::solid(100, 60, [0.0; 4]));
        let mut doc = Document::new(100, 60);
        doc.nodes
            .push(Node::raster(1, "Ink", base.clone(), Placement::default()));
        let args = json!({"node":1,"color":"#000000","seed":87,
            "settings":{"size":14,"size_jitter":0.3,"stabilizer":0.5,"tilt":0.8,"advanced":{"dynamics":{"speed_opacity":0.5},"stabilization":{"amount":0.4,"stages":2}}},
            "secondary_settings":{"size":7,"scatter":0.2},"combine_mode":"Screen",
            "strokes":[{"samples":[{"x":10,"y":20,"pressure":0.5,"tilt":[20,30],"time_ms":0},{"x":30,"y":30,"pressure":0.9,"tilt":[50,30],"time_ms":25},{"x":60,"y":15,"tilt":[40,20],"time_ms":100},{"x":85,"y":30}]}]});
        let script = paint_script(&doc, &args).unwrap_or_else(|e| panic!("{}", text(&e)));
        let resolved = &script.strokes[0];
        assert_eq!(resolved.brush.stabilizer, 0.5);
        let samples = resolved.samples.as_ref().unwrap();
        assert_eq!(samples[3].time_ms, 116.);
        let (secondary, mode) = resolved.secondary.unwrap();
        let expected = render_dual_stroke(
            base.clone(),
            resolved.brush,
            secondary,
            mode,
            PreviewMode::Paint([0., 0., 0., 1.]),
            samples,
            87,
        );
        let (actual, _) = script.render(&base);
        assert_eq!(actual.to_srgba8(), expected.to_srgba8());
        let mut other_seed = args.clone();
        other_seed["seed"] = json!(88);
        let changed = paint_script(&doc, &other_seed).unwrap().render(&base).0;
        assert_ne!(changed.to_srgba8(), actual.to_srgba8());
        let mut no_dynamics = args.clone();
        for sample in no_dynamics["strokes"][0]["samples"].as_array_mut().unwrap() {
            sample.as_object_mut().unwrap().remove("tilt");
            sample["time_ms"] = json!(0);
        }
        let changed = paint_script(&doc, &no_dynamics).unwrap().render(&base).0;
        assert_ne!(changed.to_srgba8(), actual.to_srgba8());
        let mut incremental = script.start_stroke(base.clone(), resolved);
        for index in 0..resolved.points.len() {
            resolved.feed_point(&mut incremental, index);
            let _ = incremental.render(&base);
        }
        incremental.finish();
        assert_eq!(incremental.render(&base).0.to_srgba8(), actual.to_srgba8());
        let mut legacy = args.clone();
        legacy["strokes"] = json!([{"points":[[10,20],[30,30],[60,15],[85,30]], "secondary_settings":null,"seed":9}]);
        let legacy = paint_script(&doc, &legacy).unwrap();
        assert!(legacy.strokes[0].samples.is_none());
        assert!(legacy.strokes[0].secondary.is_none());
        assert_eq!(legacy.strokes[0].seed, Some(9));
        assert_eq!(legacy.strokes[0].brush.stabilizer, 0.);
        assert_eq!(legacy.strokes[0].brush.advanced.stabilization.amount, 0.);
    }

    #[test]
    fn paint_alpha_lock_preserves_translucent_coverage_and_selection_strength() {
        let base = Arc::new(Raster::from_fn(32, 32, [0; 4], |x, _| {
            if x < 4 { [0; 4] } else { [32768, 0, 0, 32768] }
        }));
        let mut doc = Document::new(32, 32);
        let id = Command::AddNode {
            node: Box::new(Node::raster(
                0,
                "Translucent",
                base.clone(),
                Placement::default(),
            )),
            slot: Slot::TOP,
        }
        .apply(&mut doc)
        .unwrap()
        .unwrap();
        doc.selection = Some(Arc::new(emulsion_raster::Mask::from_fn(
            32,
            32,
            0,
            |x, _| {
                if x < 4 {
                    255
                } else if x < 8 {
                    0
                } else if x < 16 {
                    128
                } else {
                    255
                }
            },
        )));
        let mut e = Editor::new(doc, None);
        let before = e.doc.clone();
        let args = json!({"node": id, "color": "#0000ff", "alpha_lock": true,
            "settings": {"size": 128, "hardness": 1, "flow": 1, "opacity": 1},
            "strokes": [{"points": [[16,16]]}]});
        let result = execute(&mut e, "paint", &args);
        assert!(!result.is_error, "{}", text(&result));
        let NodeKind::Raster { raster, .. } = &e.doc.node(id).unwrap().kind else {
            panic!()
        };
        for y in 0..32 {
            for x in 0..32 {
                assert_eq!(raster.get(x, y)[3], base.get(x, y)[3], "alpha at {x},{y}");
            }
        }
        assert_eq!(
            raster.get(2, 16),
            [0; 4],
            "empty selected pixels stay empty"
        );
        assert_eq!(
            raster.get(6, 16),
            base.get(6, 16),
            "unselected colour stays untouched"
        );
        let half = raster.get(12, 16);
        assert!(
            (half[0] as i32 - 16320).abs() < 3 && (half[2] as i32 - 16448).abs() < 3,
            "selection blends colour once without multiplying by base alpha: {half:?}"
        );
        assert_eq!(
            raster.get(24, 16),
            [0, 0, 32768, 32768],
            "full selection replaces straight colour"
        );
        assert!(e.undo());
        assert_eq!(e.doc, before);
        let mut erase = args;
        erase["brush"] = json!("Hard eraser");
        let revision = e.revision;
        let steps = e.history.len();
        let result = execute(&mut e, "paint", &erase);
        assert!(!result.is_error, "{}", text(&result));
        assert!(text(&result).starts_with("No pixels changed"));
        assert_eq!(e.doc, before, "erasing cannot alter alpha-locked content");
        assert_eq!(e.revision, revision);
        assert_eq!(e.history.len(), steps);
    }

    #[test]
    fn paint_symmetry_uses_document_axes_on_rotated_nonuniform_layers() {
        let placement = Placement {
            x: 0.0,
            y: 64.0,
            scale_x: 2.0,
            scale_y: 1.0,
            rotation: 90.0,
            ..Default::default()
        };
        let mut doc = Document::new(256, 256);
        let base = Arc::new(Raster::transparent(128, 128));
        let id = Command::AddNode {
            node: Box::new(Node::raster(0, "Transformed", base.clone(), placement)),
            slot: Slot::TOP,
        }
        .apply(&mut doc)
        .unwrap()
        .unwrap();
        let to_local = placement.to_doc(128, 128).inverse();
        for (option, expected, absent) in [
            (
                json!({"mirror": "x"}),
                vec![(100.0, 116.0), (156.0, 116.0)],
                (100.0, 140.0),
            ),
            (
                json!({"symmetry": 4}),
                vec![
                    (100.0, 116.0),
                    (140.0, 100.0),
                    (156.0, 140.0),
                    (116.0, 156.0),
                ],
                (156.0, 116.0),
            ),
        ] {
            let mut args = json!({"node": id, "color": "#000000", "settings": {"size": 8, "hardness": 1},
                "strokes": [{"points": [[100,116]]}]});
            args.as_object_mut()
                .unwrap()
                .extend(option.as_object().unwrap().clone());
            let script = paint_script(&doc, &args).unwrap();
            let (painted, _) = script.render(&base);
            let coverage = |x, y| {
                let p = to_local.transform_point2(glam::dvec2(x, y));
                painted.get(p.x.floor() as u32, p.y.floor() as u32)[3]
            };
            for (x, y) in expected {
                assert!(
                    coverage(x, y) > 20000,
                    "{option}: missing document-space copy at {x},{y}"
                );
            }
            assert_eq!(
                coverage(absent.0, absent.1),
                0,
                "{option}: unexpected layer-axis copy"
            );
        }
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
    fn describe_document_preserves_transformed_source_geometry_without_edits() {
        let source = Arc::new(Raster::solid(8, 4, [0.0; 4]));
        let raster_placement = Placement {
            x: 0.25,
            y: 0.5,
            scale_x: -2.0,
            scale_y: 0.5,
            rotation: 90.0,
            flip_x: true,
            flip_y: false,
        };
        let smart_placement = Placement {
            x: 30.25,
            y: 20.5,
            scale_x: 0.5,
            scale_y: -3.0,
            rotation: 90.0,
            flip_x: false,
            flip_y: true,
        };
        let mut doc = Document::new(100, 80);
        doc.nodes.push(Node::raster(
            1,
            "Transparent raster",
            source.clone(),
            raster_placement,
        ));
        doc.nodes.push(Node::smart(
            2,
            "Expanded smart cache",
            source,
            vec![emulsion_filters::Filter::GaussianBlur { radius: 2.0 }],
            smart_placement,
        ));
        let NodeKind::Smart { cache, offset, .. } = &doc.node(2).unwrap().kind else {
            panic!("smart fixture");
        };
        assert!(cache.width() > 8 && cache.height() > 4 && offset.0 < 0);
        let mut e = Editor::new(doc, None);
        let before = e.doc.clone();
        let history = e.history.len();
        let uncommitted = e.uncommitted();
        let result = execute(&mut e, "describe_document", &json!({}));
        assert!(!result.is_error, "{}", text(&result));
        let description: Value = serde_json::from_str(&text(&result)).unwrap();
        let nodes = description["nodes"].as_array().unwrap();
        for (id, placement, bounds) in [
            (
                1,
                raster_placement,
                json!({ "x": -9, "y": -7, "width": 3, "height": 17 }),
            ),
            (
                2,
                smart_placement,
                json!({ "x": 26, "y": 12, "width": 13, "height": 5 }),
            ),
        ] {
            let node = nodes.iter().find(|n| n["id"] == id).unwrap();
            assert_eq!(node["pixels"], "8×4");
            assert_eq!(node["source_size"], json!({ "width": 8, "height": 4 }));
            assert_eq!(node["placement"]["x"], placement.x);
            assert_eq!(node["placement"]["y"], placement.y);
            assert_eq!(node["placement"]["scale"], placement.scale_x.abs() * 100.0);
            assert_eq!(node["placement"]["scale_x"], placement.scale_x);
            assert_eq!(node["placement"]["scale_y"], placement.scale_y);
            assert_eq!(node["placement"]["rotation"], placement.rotation);
            assert_eq!(node["placement"]["flip_x"], placement.flip_x);
            assert_eq!(node["placement"]["flip_y"], placement.flip_y);
            assert_eq!(node["source_bounds"], bounds);
        }
        assert_eq!(e.doc, before);
        assert_eq!(e.history.len(), history);
        assert_eq!(e.uncommitted(), uncommitted);
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
    fn align_node_moves_group_to_canvas_or_selection_with_one_undo() {
        let mut e = Editor::new(Document::new(128, 128), None);
        let source = Arc::new(Raster::solid(8, 6, [1.0, 0.0, 0.0, 1.0]));
        let raster_id = e
            .execute(Command::AddNode {
                node: Box::new(Node::raster(
                    0,
                    "Pixels",
                    source.clone(),
                    Placement::at(50.0, 60.0),
                )),
                slot: Slot::TOP,
            })
            .unwrap()
            .unwrap();
        let result = execute(
            &mut e,
            "draw_path",
            &json!({
                "name": "Editable", "d": "M 20 20 L 40 20 L 40 60 Z", "stroke": "none", "fill": "#000000"
            }),
        );
        assert!(!result.is_error, "{}", text(&result));
        let path_id = e
            .doc
            .nodes
            .iter()
            .find(|n| n.name == "Editable")
            .unwrap()
            .id;
        let result = execute(
            &mut e,
            "group_nodes",
            &json!({"nodes": [path_id, raster_id]}),
        );
        assert!(!result.is_error, "{}", text(&result));
        let group = e.doc.node(path_id).unwrap().parent.unwrap();
        assert_eq!(
            emulsion_core::geometry::node_bounds(&e.doc, group),
            Some(IRect::new(20, 20, 38, 46))
        );
        for target in ["canvas", "selection"] {
            if target == "selection" {
                let result = execute(
                    &mut e,
                    "select_rect",
                    &json!({"x": 10, "y": 12, "width": 61, "height": 71}),
                );
                assert!(!result.is_error, "{}", text(&result));
            }
            let before = e.doc.clone();
            // Center ties round to the nearest even whole-pixel offset.
            let expected = if target == "canvas" {
                [(0, 20), (45, 20), (90, 20), (20, 0), (20, 41), (20, 82)]
            } else {
                [(10, 20), (22, 20), (33, 20), (20, 12), (20, 24), (20, 37)]
            };
            for (alignment, (x, y)) in [
                "left",
                "horizontal_center",
                "right",
                "top",
                "vertical_center",
                "bottom",
            ]
            .into_iter()
            .zip(expected)
            {
                let steps = e.history.len();
                let mut args = json!({"node": group, "alignment": alignment});
                if target == "selection" {
                    args["target"] = json!(target);
                }
                let result = execute(&mut e, "align_node", &args);
                assert!(!result.is_error, "{args}: {}", text(&result));
                assert_eq!(
                    emulsion_core::geometry::node_bounds(&e.doc, group),
                    Some(IRect::new(x, y, 38, 46))
                );
                let NodeKind::Path { path, .. } = &e.doc.node(path_id).unwrap().kind else {
                    panic!("path flattened")
                };
                assert_eq!(path.subpaths[0].anchors[0].p, (x as f64, y as f64));
                let NodeKind::Raster { raster, placement } = &e.doc.node(raster_id).unwrap().kind
                else {
                    panic!("raster missing")
                };
                assert!(Arc::ptr_eq(raster, &source));
                assert_eq!(
                    (placement.x, placement.y),
                    (x as f64 + 30.0, y as f64 + 40.0)
                );
                assert!(match (&e.doc.selection, &before.selection) {
                    (None, None) => true,
                    (Some(a), Some(b)) => Arc::ptr_eq(a, b),
                    _ => false,
                });
                assert_eq!(e.doc.children(Some(group)), before.children(Some(group)));
                assert_eq!(e.history.len(), steps + 1);
                assert!(e.undo());
                assert_eq!(e.doc, before);
            }
        }
        for args in [
            json!({"node": group}),
            json!({"node": group, "alignment": "center"}),
            json!({"node": group, "alignment": 1}),
            json!({"node": group, "alignment": "left", "target": "layer"}),
            json!({"node": group, "alignment": "left", "target": null}),
            json!({"node": 99999, "alignment": "left"}),
        ] {
            let before = e.doc.clone();
            let revision = e.revision;
            let steps = e.history.len();
            let result = execute(&mut e, "align_node", &args);
            assert!(result.is_error, "{args}: {}", text(&result));
            assert_eq!(e.doc, before);
            assert_eq!(e.revision, revision);
            assert_eq!(e.history.len(), steps);
        }
        e.execute(Command::SetSelection { selection: None })
            .unwrap();
        for locked in [false, true] {
            if locked {
                e.execute(Command::SetLocked {
                    id: group,
                    locked: true,
                })
                .unwrap();
            }
            let before = e.doc.clone();
            let revision = e.revision;
            let steps = e.history.len();
            let target = if locked { "canvas" } else { "selection" };
            let result = execute(
                &mut e,
                "align_node",
                &json!({"node": group, "alignment": "left", "target": target}),
            );
            assert!(result.is_error, "{}", text(&result));
            assert_eq!(e.doc, before);
            assert_eq!(e.revision, revision);
            assert_eq!(e.history.len(), steps);
        }
    }

    #[test]
    fn translate_node_moves_mixed_group_with_one_undo_and_rejects_errors_atomically() {
        let mut e = Editor::new(Document::new(128, 128), None);
        let result = execute(
            &mut e,
            "draw_path",
            &json!({"d": "M 20 20 L 40 20 L 40 60 Z", "fill": "#000000"}),
        );
        assert!(!result.is_error, "{}", text(&result));
        let path_id = e.doc.nodes[0].id;
        let source = Arc::new(Raster::solid(8, 6, [1.0, 0.0, 0.0, 1.0]));
        let raster_id = e
            .execute(Command::AddNode {
                node: Box::new(Node::raster(
                    0,
                    "Pixels",
                    source.clone(),
                    Placement::at(50.0, 60.0),
                )),
                slot: Slot::TOP,
            })
            .unwrap()
            .unwrap();
        let result = execute(
            &mut e,
            "group_nodes",
            &json!({"nodes": [path_id, raster_id], "name": "Mixed"}),
        );
        assert!(!result.is_error, "{}", text(&result));
        let group = e.doc.node(path_id).unwrap().parent.unwrap();
        let before = e.doc.clone();
        let steps = e.history.len();
        let result = execute(
            &mut e,
            "translate_node",
            &json!({"node": group, "dx": 3.5, "dy": -4}),
        );
        assert!(!result.is_error, "{}", text(&result));
        let NodeKind::Path { path, .. } = &e.doc.node(path_id).unwrap().kind else {
            panic!("path flattened")
        };
        assert_eq!(path.subpaths[0].anchors[0].p, (23.5, 16.0));
        let NodeKind::Raster { raster, placement } = &e.doc.node(raster_id).unwrap().kind else {
            panic!("raster missing")
        };
        assert!(
            Arc::ptr_eq(raster, &source),
            "translation must retain source pixels"
        );
        assert_eq!((placement.x, placement.y), (53.5, 56.0));
        assert_eq!(e.doc.children(Some(group)), before.children(Some(group)));
        assert_eq!((e.doc.width, e.doc.height), (128, 128));
        assert!(match (&e.doc.selection, &before.selection) {
            (None, None) => true,
            (Some(a), Some(b)) => Arc::ptr_eq(a, b),
            _ => false,
        });
        assert_eq!(e.history.len(), steps + 1);
        assert!(e.undo());
        assert_eq!(e.doc, before);

        for args in [
            json!({"node": group, "dx": 1}),
            json!({"node": group, "dy": 1}),
            json!({"node": group, "dx": "1", "dy": 0}),
            json!({"node": group, "dx": 0, "dy": null}),
            json!({"node": 99999, "dx": 1, "dy": 0}),
        ] {
            let revision = e.revision;
            let steps = e.history.len();
            let result = execute(&mut e, "translate_node", &args);
            assert!(result.is_error, "{args}: {}", text(&result));
            assert_eq!(e.doc, before);
            assert_eq!(e.revision, revision);
            assert_eq!(e.history.len(), steps);
        }
        for locked_id in [group, path_id] {
            e.execute(Command::SetLocked {
                id: locked_id,
                locked: true,
            })
            .unwrap();
            let locked = e.doc.clone();
            let revision = e.revision;
            let steps = e.history.len();
            for target in [group, path_id] {
                let result = execute(
                    &mut e,
                    "translate_node",
                    &json!({"node": target, "dx": 1, "dy": 2}),
                );
                assert!(
                    result.is_error,
                    "locked {locked_id}, target {target}: {}",
                    text(&result)
                );
                assert_eq!(e.doc, locked);
                assert_eq!(e.revision, revision);
                assert_eq!(e.history.len(), steps);
            }
            assert!(e.undo());
            assert_eq!(e.doc, before);
        }
    }

    #[test]
    fn rotate_node_rotates_editable_paths_incrementally_with_one_undo_step() {
        let mut e = Editor::new(Document::new(128, 128), None);
        let result = execute(
            &mut e,
            "draw_path",
            &json!({
                "name": "Rectangle", "d": "M 20 20 L 40 20 L 40 60 L 20 60 Z",
                "stroke": "none", "fill": "#000000"
            }),
        );
        assert!(!result.is_error, "{}", text(&result));
        let id = e.doc.nodes[0].id;
        let before = e.doc.clone();
        let steps = e.history.len();
        let point = |doc: &Document| {
            let NodeKind::Path { path, .. } = &doc.node(id).unwrap().kind else {
                panic!("rotation must preserve editable path geometry");
            };
            path.subpaths[0].anchors[0].p
        };
        for expected in [(50.0, 30.0), (40.0, 60.0)] {
            let result = execute(&mut e, "rotate_node", &json!({"node": id, "degrees": 90}));
            assert!(!result.is_error, "{}", text(&result));
            let actual = point(&e.doc);
            assert!(
                (actual.0 - expected.0).abs() < 1e-5 && (actual.1 - expected.1).abs() < 1e-5,
                "clockwise incremental rotation: {actual:?} vs {expected:?}"
            );
        }
        assert_eq!(e.history.len(), steps + 2);
        assert_eq!((e.doc.width, e.doc.height), (128, 128));
        assert!(e.undo());
        assert!(e.undo());
        assert_eq!(e.doc, before);
    }

    #[test]
    fn rotate_node_rotates_groups_together_and_rejects_invalid_input_atomically() {
        let mut e = Editor::new(Document::new(128, 128), None);
        let mut ids = Vec::new();
        for d in [
            "M 10 40 L 30 40 L 30 60 L 10 60 Z",
            "M 90 40 L 110 40 L 110 60 L 90 60 Z",
        ] {
            let result = execute(
                &mut e,
                "draw_path",
                &json!({"d": d, "stroke": "none", "fill": "#000000"}),
            );
            assert!(!result.is_error, "{}", text(&result));
            ids.push(e.doc.nodes.last().unwrap().id);
        }
        let result = execute(
            &mut e,
            "group_nodes",
            &json!({"nodes": ids, "name": "Pair"}),
        );
        assert!(!result.is_error, "{}", text(&result));
        let group = e
            .doc
            .nodes
            .iter()
            .find(|node| node.name == "Pair")
            .unwrap()
            .id;
        let before = e.doc.clone();
        let steps = e.history.len();
        let result = execute(
            &mut e,
            "rotate_node",
            &json!({"node": group, "degrees": 90}),
        );
        assert!(!result.is_error, "{}", text(&result));
        for (id, expected) in ids.iter().zip([(70.0, 0.0), (70.0, 80.0)]) {
            let node = e.doc.node(*id).unwrap();
            assert_eq!(node.parent, Some(group));
            let NodeKind::Path { path, .. } = &node.kind else {
                panic!("path flattened")
            };
            let actual = path.subpaths[0].anchors[0].p;
            assert!(
                (actual.0 - expected.0).abs() < 1e-5 && (actual.1 - expected.1).abs() < 1e-5,
                "children rotate around one group pivot: {actual:?} vs {expected:?}"
            );
        }
        assert_eq!(e.history.len(), steps + 1);
        assert!(e.undo());
        assert_eq!(e.doc, before);
        for args in [
            json!({"node": group}),
            json!({"node": group, "degrees": "90"}),
            json!({"node": 9999, "degrees": 90}),
        ] {
            let revision = e.revision;
            let result = execute(&mut e, "rotate_node", &args);
            assert!(result.is_error, "{args}: {}", text(&result));
            assert_eq!(e.doc, before);
            assert_eq!(e.revision, revision);
        }
        e.execute(Command::SetLocked {
            id: ids[0],
            locked: true,
        })
        .unwrap();
        let locked = e.doc.clone();
        let revision = e.revision;
        let result = execute(
            &mut e,
            "rotate_node",
            &json!({"node": group, "degrees": -15}),
        );
        assert!(result.is_error);
        assert_eq!(e.doc, locked);
        assert_eq!(e.revision, revision);
    }

    #[test]
    fn rotate_node_keeps_text_rotation_when_content_changes() {
        let mut e = Editor::new(Document::new(200, 160), None);
        let result = execute(
            &mut e,
            "add_text",
            &json!({"text": "Hello", "x": 60, "y": 60, "size": 24}),
        );
        assert!(!result.is_error, "{}", text(&result));
        let id = e.doc.nodes[0].id;
        for (tool, args) in [
            ("rotate_node", json!({"node": id, "degrees": 25})),
            ("set_text", json!({"node": id, "text": "Changed"})),
        ] {
            let result = execute(&mut e, tool, &args);
            assert!(!result.is_error, "{}", text(&result));
        }
        let NodeKind::Text { spec, .. } = &e.doc.node(id).unwrap().kind else {
            panic!("text flattened")
        };
        assert_eq!(spec.text, "Changed");
        assert!((spec.rotation - 25.0).abs() < 1e-5);
        let described = describe(&e);
        assert_eq!(described["nodes"][0]["rotation"], json!(25.0));
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
        let before_add = e.history.len();
        let r = execute(
            &mut e,
            "add_filter",
            &json!({ "node": 1, "kind": "gaussian blur", "params": { "radius": 6 },
                "opacity": 0.5, "blend": "soft light" }),
        );
        assert!(!r.is_error, "{}", text(&r));
        assert_eq!(
            e.history.len(),
            before_add + 1,
            "filter plus blending options are one undo step"
        );
        let NodeKind::Smart {
            source,
            filters,
            filter_styles,
            cache,
            offset,
            ..
        } = &e.doc.node(1).unwrap().kind
        else {
            panic!()
        };
        assert_eq!(filter_styles[0].opacity, 0.5);
        assert_eq!(
            filter_styles[0].blend,
            emulsion_raster::BlendMode::SoftLight
        );
        let (expected, expected_offset) =
            emulsion_core::smart::render_styled(source, filters, filter_styles);
        assert_eq!(cache.to_srgba8(), expected.to_srgba8());
        assert_eq!(*offset, expected_offset);
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
