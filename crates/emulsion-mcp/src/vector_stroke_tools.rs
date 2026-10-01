//! Vector stroke layers for agents: create a layer of editable pencil lines,
//! draw strokes and shapes whose width and opacity vary point by point, read
//! them back, and edit them (smooth, simplify, erase, retouch, recolour,
//! rewidth, transform, delete, convert to fills). Every call validates its
//! whole edit first and is one Undo step through `Command::SetStrokes`.
use crate::project_tools::validate_schema;
use crate::{ToolDef, ToolResult};
use emulsion_core::command::Slot;
use emulsion_core::{Command, Editor, Node, NodeId, NodeKind};
use emulsion_raster::strokes::{MAX_WIDTH, Retouch, Stroke, StrokePoint, StrokeSet};
use glam::{DAffine2, dvec2};
use serde::Deserialize;
use serde_json::{Value, json};
use std::sync::Arc;

pub(crate) const READ_ONLY: &[&str] = &["describe_vector_strokes"];
/// Most strokes or shapes one drawing call may add.
const MAX_BATCH: usize = 400;
/// Most points in one stroke, or one eraser/retouch path.
const MAX_PATH: usize = 5000;
/// Most brush dabs one erase or retouch call may make.
const MAX_DABS: usize = 20_000;
/// Most points one describe page returns when points are requested.
const PAGE_POINTS: usize = 5000;

fn def(name: &str, description: &str, properties: Value, required: &[&str]) -> ToolDef {
    ToolDef {
        name: name.into(),
        description: description.into(),
        input_schema: json!({"type":"object","additionalProperties":false,"properties":properties,"required":required}),
    }
}

fn node() -> Value {
    json!({"type":"integer","minimum":1,"description":"Vector stroke layer ID (kind \"strokes\" in describe_document)."})
}

fn coord() -> Value {
    json!({"type":"number","minimum":-1000000,"maximum":1000000})
}

fn xy() -> Value {
    json!({"type":"object","additionalProperties":false,"required":["x","y"],"properties":{"x":coord(),"y":coord()}})
}

fn point() -> Value {
    json!({"type":"object","additionalProperties":false,"required":["x","y"],"properties":{
        "x":coord(),"y":coord(),
        "width":{"type":"number","minimum":0,"maximum":8,"description":"Pressure: multiplies line_width at this point (default 1). Ramp it from ~0.1 to 1 and back for a tapered pencil line."},
        "opacity":{"type":"number","minimum":0,"maximum":1,"description":"Multiplies the colour's alpha at this point (default 1)."}
    }})
}

fn color() -> Value {
    json!({"type":"string","minLength":7,"maxLength":9,"description":"#RRGGBB or #RRGGBBAA (default #000000)."})
}

fn line_width() -> Value {
    json!({"type":"number","minimum":0.1,"maximum":MAX_WIDTH,"description":"Full line width in pixels where a point's width is 1 (default 4)."})
}

fn points(max: usize) -> Value {
    json!({"type":"array","minItems":1,"maxItems":max,"items":point()})
}

fn stroke() -> Value {
    json!({"type":"object","additionalProperties":false,"required":["points"],"properties":{
        "points":points(MAX_PATH),"color":color(),"line_width":line_width(),
        "closed":{"type":"boolean","description":"Join the last point back to the first (needs 3+ points)."}
    }})
}

fn strokes() -> Value {
    json!({"type":"array","minItems":1,"maxItems":MAX_BATCH,"items":stroke()})
}

fn indices(description: &str) -> Value {
    json!({"type":"array","minItems":1,"maxItems":20000,"uniqueItems":true,"items":{"type":"integer","minimum":0},"description":description})
}

pub(crate) fn definitions() -> Vec<ToolDef> {
    vec![
        def(
            "add_vector_layer",
            "Add a vector stroke layer: editable pencil line work whose strokes keep their points, width (pressure) and opacity, so they can be smoothed, retouched, recoloured or moved later. Optionally draw first strokes into it (same format as draw_vector_strokes; color and line_width are defaults for those strokes only, not stored on the layer). Placed above the given node or at the top. Returns its node ID. One Undo step.",
            json!({"name":{"type":"string","maxLength":200},"above":{"type":"integer","minimum":1},"strokes":strokes(),"color":color(),"line_width":line_width()}),
            &[],
        ),
        def(
            "draw_vector_strokes",
            "Draw centreline strokes on a vector stroke layer, in document pixels. Each stroke is a list of points {x,y,width?,opacity?}; width multiplies line_width (pressure, 0–8) and opacity the colour's alpha (0–1), interpolated between points, so varying them tapers and fades the line like a pencil. color and line_width set call defaults that each stroke may override. Several strokes per call, one Undo step. Returns the new stroke indices.",
            json!({"node":node(),"strokes":strokes(),"color":color(),"line_width":line_width()}),
            &["node", "strokes"],
        ),
        def(
            "draw_vector_shapes",
            "Draw lines, rectangles, ellipses and polylines as editable strokes on a vector stroke layer. line takes exactly 2 points and polyline 2 or more (points may carry width/opacity like draw_vector_strokes; closed joins a polyline); rectangle and ellipse take x, y, width, height bounds and are closed strokes. color and line_width set call defaults. One Undo step. Returns the new stroke indices.",
            json!({"node":node(),"color":color(),"line_width":line_width(),"shapes":{"type":"array","minItems":1,"maxItems":MAX_BATCH,"items":{"type":"object","additionalProperties":false,"required":["shape"],"properties":{
                "shape":{"type":"string","enum":["line","rectangle","ellipse","polyline"]},
                "points":points(MAX_PATH),
                "x":coord(),"y":coord(),
                "width":{"type":"number","minimum":0,"maximum":1000000,"description":"Rectangle/ellipse bounds width."},
                "height":{"type":"number","minimum":0,"maximum":1000000,"description":"Rectangle/ellipse bounds height."},
                "closed":{"type":"boolean"},"color":color(),"line_width":line_width()
            }}}}),
            &["node", "shapes"],
        ),
        def(
            "describe_vector_strokes",
            "Read a vector stroke layer's strokes a page at a time: index, colour, line_width, closed, point count, drawn bounds [x,y,w,h], and the min/max of point width and opacity. include_points adds each stroke's points {x,y,width,opacity} (pages then stop early at about 5000 points). Use next_offset for the next page. Read-only.",
            json!({"node":node(),"offset":{"type":"integer","minimum":0},"limit":{"type":"integer","minimum":1,"maximum":200,"description":"Strokes per page (default 50)."},"include_points":{"type":"boolean"}}),
            &["node"],
        ),
        def(
            "edit_vector_strokes",
            "Edit chosen strokes (indices from describe_vector_strokes; omit for all) on a vector stroke layer. Any of, applied in this order: color and line_width replace those; smooth (0–1 strength, smooth_iterations passes, ends of open strokes stay put) evens out wobbles; simplify removes points within that many pixels of the line (optimize); scale and rotation (degrees clockwise) about origin (default: the chosen strokes' centre), then dx/dy move them. Scaling also scales line widths. One Undo step.",
            json!({"node":node(),"strokes":indices("Stroke indices; omit for every stroke."),"color":color(),"line_width":line_width(),
                "smooth":{"type":"number","minimum":0,"maximum":1},"smooth_iterations":{"type":"integer","minimum":1,"maximum":50,"description":"Default 2."},
                "simplify":{"type":"number","minimum":0.01,"maximum":100,"description":"Tolerance in pixels."},
                "dx":coord(),"dy":coord(),"scale":{"type":"number","minimum":0.01,"maximum":100},"rotation":{"type":"number","minimum":-360,"maximum":360},"origin":xy()}),
            &["node"],
        ),
        def(
            "delete_vector_strokes",
            "Delete strokes by index from a vector stroke layer; later strokes move down to fill the gaps. One Undo step.",
            json!({"node":node(),"strokes":indices("Stroke indices to delete.")}),
            &["node", "strokes"],
        ),
        def(
            "erase_vector_strokes",
            "Erase the parts of strokes within radius pixels of a path of points (one point erases a circle), cutting strokes where the eraser crosses them; closed strokes open at the cut. Fills are untouched. One Undo step; reports how many strokes there are now.",
            json!({"node":node(),"points":{"type":"array","minItems":1,"maxItems":MAX_PATH,"items":xy()},"radius":{"type":"number","minimum":0.5,"maximum":2000}}),
            &["node", "points", "radius"],
        ),
        def(
            "retouch_vector_strokes",
            "Pencil retouch: brush along a path of points over existing strokes to change their line weight or opacity, or smooth them, strongest at the brush centre and fading to nothing at radius. mode is thicker, thinner, opaquer, fainter or smooth; amount (0–1) is applied by each dab, spaced every half radius along the path. Points are added under the brush where a stroke has too few. strokes limits it to those indices. One Undo step.",
            json!({"node":node(),"points":{"type":"array","minItems":1,"maxItems":MAX_PATH,"items":xy()},"mode":{"type":"string","enum":["thicker","thinner","opaquer","fainter","smooth"]},
                "radius":{"type":"number","minimum":0.5,"maximum":2000},"amount":{"type":"number","minimum":0,"maximum":1,"description":"Default 0.5."},"strokes":indices("Only retouch these stroke indices.")}),
            &["node", "points", "mode", "radius"],
        ),
        def(
            "outline_vector_strokes",
            "Convert strokes to filled shapes of their drawn outline (pencil line to brush shape) on the same vector stroke layer, keeping their colour. The strokes are removed and fills are drawn under the remaining strokes. One Undo step.",
            json!({"node":node(),"strokes":indices("Stroke indices to convert.")}),
            &["node", "strokes"],
        ),
    ]
}

/// Run a vector stroke tool, or `None` when `name` is not one.
pub(crate) fn execute(editor: &mut Editor, name: &str, args: &Value) -> Option<ToolResult> {
    let def = definitions().into_iter().find(|d| d.name == name)?;
    let result = validate_schema(&def.input_schema, args).and_then(|()| run(editor, name, args));
    Some(match result {
        Ok(value) => ToolResult::text(value.to_string()),
        Err(error) => ToolResult::error(error),
    })
}

#[derive(Deserialize)]
struct PointArg {
    x: f64,
    y: f64,
    width: Option<f32>,
    opacity: Option<f32>,
}

impl PointArg {
    fn point(&self) -> StrokePoint {
        StrokePoint {
            width: self.width.unwrap_or(1.),
            opacity: self.opacity.unwrap_or(1.),
            ..StrokePoint::new(self.x, self.y)
        }
    }
}

#[derive(Deserialize)]
struct XY {
    x: f64,
    y: f64,
}

#[derive(Deserialize)]
struct StrokeArg {
    points: Vec<PointArg>,
    color: Option<String>,
    line_width: Option<f32>,
    #[serde(default)]
    closed: bool,
}

#[derive(Deserialize)]
struct ShapeArg {
    shape: String,
    points: Option<Vec<PointArg>>,
    x: Option<f64>,
    y: Option<f64>,
    width: Option<f64>,
    height: Option<f64>,
    #[serde(default)]
    closed: bool,
    color: Option<String>,
    line_width: Option<f32>,
}

fn parse<T: for<'de> Deserialize<'de>>(value: &Value, what: &str) -> Result<T, String> {
    serde_json::from_value(value.clone()).map_err(|e| format!("Invalid {what}: {e}"))
}

/// `#RRGGBB` or `#RRGGBBAA` as straight sRGB.
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

fn hex(c: [u8; 4]) -> String {
    format!("#{:02x}{:02x}{:02x}{:02x}", c[0], c[1], c[2], c[3])
}

/// Call-level colour and line width that strokes may override.
struct Defaults {
    color: [u8; 4],
    width: f32,
}

impl Defaults {
    fn from(args: &Value) -> Result<Self, String> {
        Ok(Self {
            color: match args.get("color").and_then(Value::as_str) {
                Some(c) => parse_color(c)?,
                None => [0, 0, 0, 255],
            },
            width: args
                .get("line_width")
                .and_then(Value::as_f64)
                .map_or(4., |w| w as f32),
        })
    }
    fn stroke(
        &self,
        points: Vec<StrokePoint>,
        color: Option<&str>,
        width: Option<f32>,
        closed: bool,
    ) -> Result<Stroke, String> {
        if closed && points.len() < 3 {
            return Err("A closed stroke needs at least 3 points.".into());
        }
        Ok(Stroke {
            points,
            color: color.map_or(Ok(self.color), parse_color)?,
            width: width.unwrap_or(self.width),
            closed,
        })
    }
}

fn parse_strokes(args: &Value) -> Result<Vec<Stroke>, String> {
    let defaults = Defaults::from(args)?;
    let Some(list) = args.get("strokes") else {
        return Ok(Vec::new());
    };
    parse::<Vec<StrokeArg>>(list, "strokes")?
        .into_iter()
        .map(|s| {
            defaults.stroke(
                s.points.iter().map(PointArg::point).collect(),
                s.color.as_deref(),
                s.line_width,
                s.closed,
            )
        })
        .collect()
}

fn ellipse(x: f64, y: f64, w: f64, h: f64) -> Vec<StrokePoint> {
    let (rx, ry) = (w / 2., h / 2.);
    let around = std::f64::consts::TAU * ((rx * rx + ry * ry) / 2.).sqrt();
    let n = (around / 6.).ceil().clamp(16., 360.) as usize;
    (0..n)
        .map(|i| {
            let a = std::f64::consts::TAU * i as f64 / n as f64;
            StrokePoint::new(x + rx + rx * a.cos(), y + ry + ry * a.sin())
        })
        .collect()
}

fn parse_shapes(args: &Value) -> Result<Vec<Stroke>, String> {
    let defaults = Defaults::from(args)?;
    parse::<Vec<ShapeArg>>(&args["shapes"], "shapes")?
        .into_iter()
        .enumerate()
        .map(|(i, s)| {
            let bounds = || match (s.x, s.y, s.width, s.height) {
                (Some(x), Some(y), Some(w), Some(h)) if s.points.is_none() => Ok((x, y, w, h)),
                _ => Err(format!(
                    "Shape {i}: a {} takes x, y, width and height, not points.",
                    s.shape
                )),
            };
            let path = |min: usize, max: usize| match &s.points {
                Some(p)
                    if (min..=max).contains(&p.len())
                        && s.x.is_none()
                        && s.y.is_none()
                        && s.width.is_none()
                        && s.height.is_none() =>
                {
                    Ok(p.iter().map(PointArg::point).collect::<Vec<_>>())
                }
                _ => Err(format!(
                    "Shape {i}: a {} takes {} points and no bounds.",
                    s.shape,
                    if min == max {
                        min.to_string()
                    } else {
                        format!("{min} or more")
                    }
                )),
            };
            let (points, closed) = match s.shape.as_str() {
                "line" => (path(2, 2)?, false),
                "polyline" => (path(2, MAX_PATH)?, s.closed),
                "rectangle" => {
                    let (x, y, w, h) = bounds()?;
                    let corners = [(x, y), (x + w, y), (x + w, y + h), (x, y + h)];
                    (
                        corners
                            .iter()
                            .map(|&(x, y)| StrokePoint::new(x, y))
                            .collect(),
                        true,
                    )
                }
                _ => {
                    let (x, y, w, h) = bounds()?;
                    (ellipse(x, y, w, h), true)
                }
            };
            defaults.stroke(points, s.color.as_deref(), s.line_width, closed)
        })
        .collect()
}

fn layer(editor: &Editor, id: NodeId) -> Result<StrokeSet, String> {
    if editor.is_read_only() {
        return Err(
            "This document is read-only (a locked storyboard panel); unlock it first.".into(),
        );
    }
    let n = editor.doc.node(id).ok_or_else(|| format!("No node {id}"))?;
    let NodeKind::Strokes { strokes, .. } = &n.kind else {
        return Err(format!(
            "{} (#{id}) is not a vector stroke layer; add_vector_layer makes one.",
            n.name
        ));
    };
    if n.locked {
        return Err(format!("{} (#{id}) is locked.", n.name));
    }
    Ok((**strokes).clone())
}

fn commit(editor: &mut Editor, id: NodeId, set: StrokeSet) -> Result<(), String> {
    set.validate()?;
    editor
        .execute(Command::SetStrokes {
            id,
            strokes: Arc::new(set),
        })
        .map(|_| ())
        .map_err(|e| e.to_string())
}

/// Sorted, in-range stroke indices, or every stroke when `key` is absent.
fn chosen(set: &StrokeSet, args: &Value, key: &str) -> Result<Vec<usize>, String> {
    let Some(list) = args.get(key) else {
        return Ok((0..set.strokes.len()).collect());
    };
    let mut out: Vec<usize> = parse(list, key)?;
    out.sort_unstable();
    out.dedup();
    if let Some(bad) = out.iter().find(|i| **i >= set.strokes.len()) {
        return Err(format!(
            "No stroke {bad}; the layer has {} strokes.",
            set.strokes.len()
        ));
    }
    Ok(out)
}

/// Run `edit` on just the chosen strokes, which keep their places.
fn on_subset(
    set: &mut StrokeSet,
    chosen: &[usize],
    edit: impl FnOnce(&mut StrokeSet) -> bool,
) -> bool {
    let mut subset = StrokeSet {
        strokes: chosen.iter().map(|i| set.strokes[*i].clone()).collect(),
        fills: Vec::new(),
    };
    let changed = edit(&mut subset);
    for (i, stroke) in chosen.iter().zip(subset.strokes) {
        set.strokes[*i] = stroke;
    }
    changed
}

fn num(args: &Value, key: &str) -> Option<f64> {
    args.get(key).and_then(Value::as_f64)
}

fn lerp_point(a: StrokePoint, b: StrokePoint, t: f64) -> StrokePoint {
    let tf = t as f32;
    StrokePoint {
        x: a.x + (b.x - a.x) * t,
        y: a.y + (b.y - a.y) * t,
        width: a.width + (b.width - a.width) * tf,
        opacity: a.opacity + (b.opacity - a.opacity) * tf,
    }
}

/// Where segment `a`–`b` runs inside the circle, as a parameter range.
fn chord(a: StrokePoint, b: StrokePoint, c: (f64, f64), r: f64) -> Option<(f64, f64, f64)> {
    let (dx, dy) = (b.x - a.x, b.y - a.y);
    let (fx, fy) = (a.x - c.0, a.y - c.1);
    let qa = dx * dx + dy * dy;
    if qa == 0. {
        return None;
    }
    let qb = 2. * (fx * dx + fy * dy);
    let qc = fx * fx + fy * fy - r * r;
    let disc = qb * qb - 4. * qa * qc;
    if disc <= 0. {
        return None;
    }
    let s = disc.sqrt();
    let (e0, e1) = ((-qb - s) / (2. * qa), (-qb + s) / (2. * qa));
    let (t0, t1) = (e0.max(0.), e1.min(1.));
    (t0 < t1).then_some((t0, t1, qa.sqrt()))
}

/// Add points to segments crossing the circle at `c`, as `inside` asks for
/// the parameter range inside it. Returns whether the stroke reaches it.
fn densify(
    stroke: &mut Stroke,
    c: (f64, f64),
    r: f64,
    inside: impl Fn(f64, f64, f64) -> Vec<f64>,
) -> bool {
    let n = stroke.points.len();
    let segments = if stroke.closed && n > 2 {
        n
    } else {
        n.saturating_sub(1)
    };
    let mut reached = stroke
        .points
        .iter()
        .any(|p| (p.x - c.0).hypot(p.y - c.1) <= r);
    let mut out = Vec::with_capacity(n);
    for i in 0..n {
        let a = stroke.points[i];
        out.push(a);
        if i < segments {
            let b = stroke.points[(i + 1) % n];
            if let Some((t0, t1, len)) = chord(a, b, c, r) {
                reached = true;
                out.extend(
                    inside(t0, t1, len)
                        .into_iter()
                        .filter(|t| *t > 1e-9 && *t < 1. - 1e-9)
                        .map(|t| lerp_point(a, b, t)),
                );
            }
        }
    }
    stroke.points = out;
    reached
}

/// Points spaced every `step` pixels along a path, starting at its first.
fn dabs(path: &[XY], step: f64) -> Result<Vec<(f64, f64)>, String> {
    let mut out = vec![(path[0].x, path[0].y)];
    for pair in path.windows(2) {
        let (a, b) = (&pair[0], &pair[1]);
        let len = (b.x - a.x).hypot(b.y - a.y);
        let k = (len / step).ceil() as usize;
        if out.len() + k > MAX_DABS {
            return Err(format!(
                "The path is too long for this radius (over {MAX_DABS} dabs); split it or use a bigger radius."
            ));
        }
        for j in 1..=k {
            let t = j as f64 / k as f64;
            out.push((a.x + (b.x - a.x) * t, a.y + (b.y - a.y) * t));
        }
    }
    Ok(out)
}

fn erase(set: &mut StrokeSet, c: (f64, f64), r: f64) -> bool {
    for stroke in &mut set.strokes {
        // A point just outside each edge of the cut, and one in the middle,
        // so long segments split exactly where the eraser crosses them.
        let reached = densify(stroke, c, r, |t0, t1, len| {
            let margin = 0.25 / len;
            vec![t0 - margin, (t0 + t1) / 2., t1 + margin]
        });
        let inside = |p: &StrokePoint| (p.x - c.0).hypot(p.y - c.1) <= r;
        if reached && stroke.closed {
            // Open the loop at the cut so the closing segment survives.
            if let Some(k) = stroke.points.iter().position(inside) {
                stroke.points.rotate_left(k);
                let first = stroke.points[0];
                stroke.points.push(first);
                stroke.closed = false;
            }
        }
    }
    set.erase(c, r)
}

fn retouch(set: &mut StrokeSet, c: (f64, f64), r: f64, mode: Retouch, amount: f32) -> bool {
    for stroke in &mut set.strokes {
        densify(stroke, c, r, |t0, t1, len| {
            let k = ((t1 - t0) * len / (r / 4.)).ceil().max(1.) as usize;
            (0..=k)
                .map(|j| t0 + (t1 - t0) * j as f64 / k as f64)
                .collect()
        });
    }
    set.retouch(c, r, mode, amount)
}

fn stroke_json(index: usize, s: &Stroke, with_points: bool) -> Value {
    let round = |v: f64| (v * 100.).round() / 100.;
    let range = |f: fn(&StrokePoint) -> f32| {
        let (lo, hi) = s
            .points
            .iter()
            .map(f)
            .fold((f32::INFINITY, f32::NEG_INFINITY), |(lo, hi), v| {
                (lo.min(v), hi.max(v))
            });
        json!([round(f64::from(lo)), round(f64::from(hi))])
    };
    let mut out = json!({
        "index": index,
        "color": hex(s.color),
        "line_width": round(f64::from(s.width)),
        "closed": s.closed,
        "point_count": s.points.len(),
        "bounds": s.bounds().map(|b| json!([b.x, b.y, b.w, b.h])),
        "width": range(|p| p.width),
        "opacity": range(|p| p.opacity),
    });
    if with_points {
        out["points"] = s
            .points
            .iter()
            .map(|p| {
                json!({"x":round(p.x),"y":round(p.y),"width":round(f64::from(p.width)),"opacity":round(f64::from(p.opacity))})
            })
            .collect();
    }
    out
}

fn describe(editor: &Editor, args: &Value) -> Result<Value, String> {
    let id = args["node"].as_u64().unwrap();
    let n = editor.doc.node(id).ok_or_else(|| format!("No node {id}"))?;
    let NodeKind::Strokes { strokes: set, .. } = &n.kind else {
        return Err(format!("{} (#{id}) is not a vector stroke layer.", n.name));
    };
    let offset = args.get("offset").and_then(Value::as_u64).unwrap_or(0) as usize;
    let limit = args.get("limit").and_then(Value::as_u64).unwrap_or(50) as usize;
    let with_points = args
        .get("include_points")
        .and_then(Value::as_bool)
        .unwrap_or(false);
    let mut items = Vec::new();
    let mut emitted = 0;
    let mut next = offset;
    for (i, s) in set.strokes.iter().enumerate().skip(offset).take(limit) {
        if with_points && !items.is_empty() && emitted + s.points.len() > PAGE_POINTS {
            break;
        }
        emitted += s.points.len();
        items.push(stroke_json(i, s, with_points));
        next = i + 1;
    }
    Ok(json!({
        "node": id,
        "name": n.name,
        "stroke_count": set.strokes.len(),
        "fill_count": set.fills.len(),
        "point_count": set.point_count(),
        "bounds": set.bounds().map(|b| json!([b.x, b.y, b.w, b.h])),
        "offset": offset,
        "next_offset": (next < set.strokes.len()).then_some(next),
        "strokes": items,
    }))
}

fn add(editor: &mut Editor, id: NodeId, new: Vec<Stroke>) -> Result<Value, String> {
    let mut set = layer(editor, id)?;
    let start = set.strokes.len();
    set.strokes.extend(new);
    let end = set.strokes.len();
    commit(editor, id, set)?;
    Ok(json!({"node":id,"added":(start..end).collect::<Vec<_>>(),"stroke_count":end}))
}

fn run(editor: &mut Editor, name: &str, args: &Value) -> Result<Value, String> {
    if name == "describe_vector_strokes" {
        return describe(editor, args);
    }
    if name == "add_vector_layer" {
        return add_layer(editor, args);
    }
    let id = args["node"].as_u64().unwrap();
    match name {
        "draw_vector_strokes" => {
            let strokes = parse_strokes(args)?;
            add(editor, id, strokes)
        }
        "draw_vector_shapes" => {
            let strokes = parse_shapes(args)?;
            add(editor, id, strokes)
        }
        "edit_vector_strokes" => edit(editor, id, args),
        "delete_vector_strokes" => {
            let mut set = layer(editor, id)?;
            let chosen = chosen(&set, args, "strokes")?;
            for i in chosen.iter().rev() {
                set.strokes.remove(*i);
            }
            let count = set.strokes.len();
            commit(editor, id, set)?;
            Ok(json!({"node":id,"deleted":chosen.len(),"stroke_count":count}))
        }
        "erase_vector_strokes" | "retouch_vector_strokes" => brush(editor, id, name, args),
        "outline_vector_strokes" => {
            let mut set = layer(editor, id)?;
            let chosen = chosen(&set, args, "strokes")?;
            set.outline_strokes(&chosen);
            let (strokes, fills) = (set.strokes.len(), set.fills.len());
            commit(editor, id, set)?;
            Ok(
                json!({"node":id,"converted":chosen.len(),"stroke_count":strokes,"fill_count":fills}),
            )
        }
        _ => Err(format!("Unknown vector stroke tool {name}")),
    }
}

fn add_layer(editor: &mut Editor, args: &Value) -> Result<Value, String> {
    if editor.is_read_only() {
        return Err(
            "This document is read-only (a locked storyboard panel); unlock it first.".into(),
        );
    }
    let set = StrokeSet {
        strokes: parse_strokes(args)?,
        fills: Vec::new(),
    };
    set.validate()?;
    let count = set.strokes.len();
    let slot = match args.get("above").and_then(Value::as_u64) {
        Some(above) => {
            let target = editor
                .doc
                .node(above)
                .ok_or_else(|| format!("No node {above}"))?;
            let siblings = editor.doc.children(target.parent);
            Slot {
                parent: target.parent,
                index: siblings.iter().position(|s| *s == above).unwrap_or(0) + 1,
            }
        }
        None => Slot::TOP,
    };
    let name = args.get("name").and_then(Value::as_str).unwrap_or("Vector");
    let (w, h) = (editor.doc.width, editor.doc.height);
    let id = editor
        .execute(Command::AddNode {
            node: Box::new(Node::strokes(0, name, Arc::new(set), w, h)),
            slot,
        })
        .map_err(|e| e.to_string())?
        .ok_or("No layer was created")?;
    Ok(json!({"node":id,"name":name,"stroke_count":count}))
}

fn edit(editor: &mut Editor, id: NodeId, args: &Value) -> Result<Value, String> {
    let mut set = layer(editor, id)?;
    let chosen = chosen(&set, args, "strokes")?;
    let edits = [
        "color",
        "line_width",
        "smooth",
        "simplify",
        "dx",
        "dy",
        "scale",
        "rotation",
    ];
    if !edits.iter().any(|k| args.get(*k).is_some()) {
        return Err(format!("Give at least one edit: {}.", edits.join(", ")));
    }
    if args.get("smooth_iterations").is_some() && args.get("smooth").is_none() {
        return Err("smooth_iterations needs smooth.".into());
    }
    if args.get("origin").is_some() && args.get("scale").is_none() && args.get("rotation").is_none()
    {
        return Err("origin needs scale or rotation.".into());
    }
    let color = args
        .get("color")
        .and_then(Value::as_str)
        .map(parse_color)
        .transpose()?;
    let origin = args
        .get("origin")
        .map(|o| parse::<XY>(o, "origin"))
        .transpose()?;
    for i in &chosen {
        let stroke = &mut set.strokes[*i];
        if let Some(c) = color {
            stroke.color = c;
        }
        if let Some(w) = num(args, "line_width") {
            stroke.width = w as f32;
        }
        if let Some(strength) = num(args, "smooth") {
            let passes = args
                .get("smooth_iterations")
                .and_then(Value::as_u64)
                .unwrap_or(2) as u32;
            stroke.smooth(strength, passes);
        }
        if let Some(tolerance) = num(args, "simplify") {
            stroke.simplify(tolerance);
        }
    }
    let scale = num(args, "scale").unwrap_or(1.);
    let rotation = num(args, "rotation").unwrap_or(0.).to_radians();
    let shift = dvec2(num(args, "dx").unwrap_or(0.), num(args, "dy").unwrap_or(0.));
    if scale != 1. || rotation != 0. || shift != glam::DVec2::ZERO {
        let pivot = match origin {
            Some(o) => dvec2(o.x, o.y),
            None => {
                let (lo, hi) = chosen.iter().flat_map(|i| &set.strokes[*i].points).fold(
                    (
                        dvec2(f64::INFINITY, f64::INFINITY),
                        dvec2(f64::NEG_INFINITY, f64::NEG_INFINITY),
                    ),
                    |(lo, hi), p| (lo.min(dvec2(p.x, p.y)), hi.max(dvec2(p.x, p.y))),
                );
                (lo + hi) / 2.
            }
        };
        let m = DAffine2::from_translation(pivot + shift)
            * DAffine2::from_angle(rotation)
            * DAffine2::from_scale(dvec2(scale, scale))
            * DAffine2::from_translation(-pivot);
        on_subset(&mut set, &chosen, |subset| {
            subset.transform(m);
            true
        });
    }
    commit(editor, id, set)?;
    Ok(json!({"node":id,"edited":chosen.len()}))
}

fn brush(editor: &mut Editor, id: NodeId, name: &str, args: &Value) -> Result<Value, String> {
    let mut set = layer(editor, id)?;
    let path: Vec<XY> = parse(&args["points"], "points")?;
    let radius = num(args, "radius").unwrap();
    let changed = if name == "erase_vector_strokes" {
        let mut changed = false;
        for c in dabs(&path, radius / 2.)? {
            changed |= erase(&mut set, c, radius);
        }
        changed
    } else {
        let chosen = chosen(&set, args, "strokes")?;
        let mode: Retouch = parse(&args["mode"], "mode")?;
        let amount = num(args, "amount").unwrap_or(0.5) as f32;
        let centres = dabs(&path, radius / 2.)?;
        on_subset(&mut set, &chosen, |subset| {
            let mut changed = false;
            for c in centres {
                changed |= retouch(subset, c, radius, mode, amount);
            }
            changed
        })
    };
    let count = set.strokes.len();
    if changed {
        commit(editor, id, set)?;
    }
    Ok(json!({"node":id,"changed":changed,"stroke_count":count}))
}

#[cfg(test)]
#[path = "vector_stroke_tools_tests.rs"]
mod tests;
