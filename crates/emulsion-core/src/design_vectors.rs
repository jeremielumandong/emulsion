//! Native vector editing operations. Geometry remains editable and uses document history.
use crate::{Command, Document, Editor, Node, NodeId, NodeKind, command::Slot};
use emulsion_raster::{
    vector::{Anchor, MAX_ANCHORS, Path, PathStyle, SubPath},
    vector_geometry::{self, BooleanOp},
};
use serde::{Deserialize, Serialize};
use std::{collections::HashSet, sync::Arc};
fn editable(doc: &Document, id: NodeId) -> Result<(), String> {
    if doc.node(id).is_none() {
        return Err("Object no longer exists.".into());
    }
    let l = doc.layer_locks(id);
    if doc.locked_ancestor(id).is_some() || l.pixels || l.position || l.transparency {
        return Err("Unlock the selected object before editing its geometry.".into());
    }
    if doc.design.frames.values().any(|f| f.boundary == id) {
        return Err("Resize the responsive frame rather than editing its boundary path.".into());
    }
    Ok(())
}
fn target(doc: &Document, id: NodeId) -> Result<(Path, PathStyle), String> {
    editable(doc, id)?;
    match &doc.node(id).unwrap().kind {
        NodeKind::Path { path, style, .. } => Ok(((**path).clone(), *style)),
        _ => Err("Select a native vector path.".into()),
    }
}
fn validate(path: &Path) -> Result<(), String> {
    if path.anchor_count() > MAX_ANCHORS {
        return Err(
            "This result exceeds 20000 editable anchors. Reduce the operation complexity.".into(),
        );
    }
    if path
        .subpaths
        .iter()
        .flat_map(|s| &s.anchors)
        .flat_map(|a| [a.p, a.h_in, a.h_out])
        .any(|p| !p.0.is_finite() || !p.1.is_finite() || p.0.abs() > 1e9 || p.1.abs() > 1e9)
    {
        return Err("Path coordinates must be finite within ±1 billion.".into());
    }
    Ok(())
}
fn apply(editor: &mut Editor, id: NodeId, path: Path, style: PathStyle) -> Result<(), String> {
    if editor.in_transaction() {
        return Err("Finish the current edit before editing vector geometry.".into());
    }
    validate(&path)?;
    editor
        .execute(Command::SetPath {
            id,
            path: Arc::new(path),
            style,
        })
        .map_err(|e| e.to_string())?;
    Ok(())
}
/// Update one anchor and its independent handles. Omitted handles translate with the anchor.
// Explicit anchor/handle arguments mirror the native point editor and MCP action.
#[allow(clippy::too_many_arguments)]
pub fn point(
    editor: &mut Editor,
    id: NodeId,
    subpath: usize,
    index: usize,
    position: (f64, f64),
    incoming: Option<(f64, f64)>,
    outgoing: Option<(f64, f64)>,
    smooth: bool,
) -> Result<(), String> {
    let (mut path, style) = target(&editor.doc, id)?;
    let a = path
        .subpaths
        .get_mut(subpath)
        .and_then(|s| s.anchors.get_mut(index))
        .ok_or("Choose an existing subpath and anchor.")?;
    let delta = (position.0 - a.p.0, position.1 - a.p.1);
    a.h_in = incoming.unwrap_or((a.h_in.0 + delta.0, a.h_in.1 + delta.1));
    a.h_out = outgoing.unwrap_or((a.h_out.0 + delta.0, a.h_out.1 + delta.1));
    if smooth {
        let u = (a.h_in.0 - position.0, a.h_in.1 - position.1);
        let v = (a.h_out.0 - position.0, a.h_out.1 - position.1);
        let product = u.0.hypot(u.1) * v.0.hypot(v.1);
        if product > 1e-10
            && ((u.0 * v.1 - u.1 * v.0).abs() > product * 1e-6 || u.0 * v.0 + u.1 * v.1 > 0.)
        {
            return Err("Smooth handles must point in opposite directions along one line.".into());
        }
    }
    a.p = position;
    a.smooth = smooth;
    apply(editor, id, path, style)
}
/// Join the end of one open subpath to the start of another, retaining Bezier controls.
pub fn join(editor: &mut Editor, id: NodeId, first: usize, second: usize) -> Result<(), String> {
    let (mut path, style) = target(&editor.doc, id)?;
    if first == second {
        return Err("Choose two different open subpaths.".into());
    }
    let a = path
        .subpaths
        .get(first)
        .ok_or("First subpath does not exist.")?;
    let b = path
        .subpaths
        .get(second)
        .ok_or("Second subpath does not exist.")?;
    if a.closed || b.closed || a.anchors.is_empty() || b.anchors.is_empty() {
        return Err("Join two nonempty open subpaths.".into());
    }
    let mut joined = a.clone();
    let mut tail = b.anchors.clone();
    let end = joined.anchors.last_mut().unwrap();
    if (end.p.0 - tail[0].p.0).hypot(end.p.1 - tail[0].p.1) < 1e-8 {
        end.h_out = tail[0].h_out;
        tail.remove(0);
    } else {
        end.h_out = end.p;
        tail[0].h_in = tail[0].p;
    }
    joined.anchors.extend(tail);
    path.subpaths[first] = joined;
    path.subpaths.remove(second);
    apply(editor, id, path, style)
}
/// Split at an existing anchor. Closed contours open at the point without losing their closing curve.
pub fn split(editor: &mut Editor, id: NodeId, subpath: usize, index: usize) -> Result<(), String> {
    let (mut path, style) = target(&editor.doc, id)?;
    let s = path
        .subpaths
        .get_mut(subpath)
        .ok_or("Subpath does not exist.")?;
    if index >= s.anchors.len() {
        return Err("Anchor does not exist.".into());
    }
    if s.closed {
        s.anchors.rotate_left(index);
        s.anchors.push(s.anchors[0]);
        s.closed = false;
    } else {
        if index == 0 || index + 1 >= s.anchors.len() {
            return Err("Split an open path at an interior anchor.".into());
        }
        let tail = s.anchors.split_off(index);
        s.anchors.push(tail[0]);
        let next = SubPath {
            anchors: tail,
            closed: false,
        };
        path.subpaths.insert(subpath + 1, next);
    }
    apply(editor, id, path, style)
}
#[derive(Clone, Copy, Debug, Deserialize, Serialize)]
#[serde(rename_all = "snake_case")]
pub enum Combine {
    Component,
    Union,
    Subtract,
    Intersect,
    Exclude,
}
/// Combine selected paths on the first path. Other source paths are removed in the same Undo step.
pub fn combine(editor: &mut Editor, ids: &[NodeId], operation: Combine) -> Result<NodeId, String> {
    if editor.in_transaction() {
        return Err("Finish the current edit first.".into());
    }
    if ids.len() < 2 || ids.len() > 64 || ids.iter().collect::<HashSet<_>>().len() != ids.len() {
        return Err("Choose 2–64 unique vector paths.".into());
    }
    let (mut path, style) = target(&editor.doc, ids[0])?;
    let parent = editor.doc.node(ids[0]).unwrap().parent;
    for id in ids {
        target(&editor.doc, *id)?;
        let node = editor.doc.node(*id).unwrap();
        if node.parent != parent
            || node.clip_to.is_some()
            || editor.doc.nodes.iter().any(|n| n.clip_to == Some(*id))
        {
            return Err("Combine sibling paths outside a clipping stack.".into());
        }
    }
    for id in &ids[1..] {
        let (other, _) = target(&editor.doc, *id)?;
        if matches!(operation, Combine::Component) {
            path.subpaths.extend(other.subpaths);
        } else {
            path = vector_geometry::boolean(
                &path,
                &other,
                match operation {
                    Combine::Union => BooleanOp::Add,
                    Combine::Subtract => BooleanOp::Subtract,
                    Combine::Intersect => BooleanOp::Intersect,
                    _ => BooleanOp::Exclude,
                },
            )?;
        }
        validate(&path)?;
    }
    editor.begin("Combine vector paths");
    let result = (|| {
        editor
            .execute(Command::SetPath {
                id: ids[0],
                path: Arc::new(path),
                style,
            })
            .map_err(|e| e.to_string())?;
        for id in &ids[1..] {
            editor
                .execute(Command::RemoveNode { id: *id })
                .map_err(|e| e.to_string())?;
        }
        Ok(ids[0])
    })();
    if result.is_ok() {
        editor.end();
    } else {
        editor.cancel();
    }
    result
}
/// Apply an editable affine skew around a chosen document-space origin.
pub fn skew(
    editor: &mut Editor,
    id: NodeId,
    x_degrees: f64,
    y_degrees: f64,
    origin: (f64, f64),
) -> Result<(), String> {
    if ![x_degrees, y_degrees]
        .iter()
        .all(|v| v.is_finite() && v.abs() < 85.)
        || ![origin.0, origin.1]
            .iter()
            .all(|v| v.is_finite() && v.abs() <= 1e9)
    {
        return Err("Skew angles must be between -85 and 85 degrees with a finite origin.".into());
    }
    let (mut path, style) = target(&editor.doc, id)?;
    let x = x_degrees.to_radians().tan();
    let y = y_degrees.to_radians().tan();
    if (1. - x * y).abs() < 1e-6 {
        return Err("That skew collapses the path.".into());
    }
    let m = glam::DAffine2::from_cols_array(&[1., y, x, 1., -x * origin.1, -y * origin.0]);
    path.transform(m);
    apply(editor, id, path, style)
}
#[derive(Clone, Copy, Debug, Deserialize, Serialize)]
#[serde(rename_all = "snake_case")]
pub enum MatchProperty {
    Kind,
    Fill,
    Stroke,
    Opacity,
    Font,
}
/// Pure matching selection, excluding hidden library masters and hidden ancestor content.
pub fn matching(
    doc: &Document,
    source: NodeId,
    property: MatchProperty,
) -> Result<Vec<NodeId>, String> {
    let source = doc
        .node(source)
        .ok_or("Choose an existing reference object.")?;
    let paint = |n: &Node, stroke: bool| match &n.kind {
        NodeKind::Path { style, .. } => Some(if stroke {
            (style.stroke, style.stroke_paint)
        } else {
            (style.fill, style.fill_paint)
        }),
        NodeKind::Fill { rgba } if !stroke => Some((Some(*rgba), Default::default())),
        NodeKind::Text { spec, .. } if !stroke => Some((Some(spec.color), Default::default())),
        _ => None,
    };
    if matches!(property, MatchProperty::Fill | MatchProperty::Stroke)
        && paint(source, matches!(property, MatchProperty::Stroke)).is_none()
    {
        return Err("This object has no matching paint property.".into());
    }
    if matches!(property, MatchProperty::Font) && !matches!(source.kind, NodeKind::Text { .. }) {
        return Err("Choose a text object for font matching.".into());
    }
    Ok(doc
        .nodes
        .iter()
        .filter(|n| {
            let mut cursor = Some(n.id);
            while let Some(id) = cursor {
                let node = doc.node(id).unwrap();
                if !node.visible {
                    return false;
                }
                cursor = node.parent;
            }
            match property {
                MatchProperty::Kind => n.kind.tag() == source.kind.tag(),
                MatchProperty::Opacity => (n.opacity - source.opacity).abs() < 1e-6,
                MatchProperty::Fill => paint(n, false) == paint(source, false),
                MatchProperty::Stroke => paint(n, true) == paint(source, true),
                MatchProperty::Font => match (&n.kind, &source.kind) {
                    (NodeKind::Text { spec: a, .. }, NodeKind::Text { spec: b, .. }) => {
                        a.font == b.font && a.bold == b.bold && a.italic == b.italic
                    }
                    _ => false,
                },
            }
        })
        .map(|n| n.id)
        .collect())
}

/// Make a separate editable outline; retain the authored source path for future stroke edits.
pub fn outline_stroke(editor: &mut Editor, id: NodeId) -> Result<NodeId, String> {
    use emulsion_raster::vector::{PathPaint, StrokeAlignment};
    if editor.in_transaction() {
        return Err("Finish the current edit first.".into());
    }
    let (path, style) = target(&editor.doc, id)?;
    let color = style.stroke.ok_or("The selected path has no stroke.")?;
    if !matches!(style.stroke_paint, PathPaint::Solid) {
        return Err("Stroke outlining currently requires a solid stroke paint.".into());
    }
    let mut outline = emulsion_raster::vector::stroke_outline(&path, &style)?;
    if style.alignment != StrokeAlignment::Center && path.subpaths.iter().any(|s| s.closed) {
        let closed = Path {
            subpaths: path.subpaths.iter().filter(|s| s.closed).cloned().collect(),
        };
        let open = Path {
            subpaths: path
                .subpaths
                .iter()
                .filter(|s| !s.closed)
                .cloned()
                .collect(),
        };
        let wide = emulsion_raster::vector::stroke_outline_scaled(&closed, &style, 2.)?;
        outline = vector_geometry::boolean(
            &wide,
            &closed,
            if style.alignment == StrokeAlignment::Inside {
                BooleanOp::Intersect
            } else {
                BooleanOp::Subtract
            },
        )?;
        if !open.is_empty() {
            outline
                .subpaths
                .extend(emulsion_raster::vector::stroke_outline(&open, &style)?.subpaths);
        }
    }
    validate(&outline)?;
    let source = editor.doc.node(id).unwrap();
    let parent = source.parent;
    let index = editor
        .doc
        .children(parent)
        .iter()
        .position(|n| *n == id)
        .unwrap()
        + 1;
    let node = Node::path(
        0,
        format!("{} stroke outline", source.name),
        Arc::new(outline),
        PathStyle {
            fill: Some(color),
            stroke: None,
            ..Default::default()
        },
        editor.doc.width,
        editor.doc.height,
    );
    let result = editor
        .execute(Command::AddNode {
            node: Box::new(node),
            slot: Slot { parent, index },
        })
        .map_err(|e| e.to_string())?;
    result.ok_or("No outline was created.".into())
}

/// Warp native contours through a rectangular control mesh. Curves are sampled into editable
/// points with bounded complexity; affine edits should use ordinary transforms to retain handles.
pub fn mesh(
    editor: &mut Editor,
    id: NodeId,
    columns: usize,
    rows: usize,
    points: &[(f64, f64)],
    tolerance: f64,
) -> Result<(), String> {
    if !(2..=16).contains(&columns)
        || !(2..=16).contains(&rows)
        || points.len() != columns * rows
        || !(0.05..=10.).contains(&tolerance)
        || !tolerance.is_finite()
    {
        return Err(
            "Use a 2–16 by 2–16 mesh, matching control points, and tolerance 0.05–10 pixels."
                .into(),
        );
    }
    if points
        .iter()
        .any(|p| ![p.0, p.1].iter().all(|v| v.is_finite() && v.abs() <= 1e9))
    {
        return Err("Mesh points must be finite and bounded.".into());
    }
    // Folded cells have ambiguous fills and frequently create accidental flipped artwork.
    for y in 0..rows - 1 {
        for x in 0..columns - 1 {
            let p = [
                points[y * columns + x],
                points[y * columns + x + 1],
                points[(y + 1) * columns + x + 1],
                points[(y + 1) * columns + x],
            ];
            let mut sign = 0.;
            for i in 0..4 {
                let a = p[i];
                let b = p[(i + 1) % 4];
                let c = p[(i + 2) % 4];
                let cross = (b.0 - a.0) * (c.1 - b.1) - (b.1 - a.1) * (c.0 - b.0);
                if cross.abs() < 1e-8 || (sign != 0. && cross * sign < 0.) {
                    return Err("Mesh cells must be convex and must not fold.".into());
                }
                sign = cross;
            }
        }
    }
    let (path, style) = target(&editor.doc, id)?;
    let (x, y, w, h) = vector_geometry::bounds(&path).ok_or("Empty path")?;
    if w <= 1e-8 || h <= 1e-8 {
        return Err("Warp requires a path with nonzero width and height.".into());
    }
    let warped = Path {
        subpaths: path
            .flatten(tolerance)
            .into_iter()
            .map(|(vertices, closed)| SubPath {
                closed,
                anchors: vertices
                    .into_iter()
                    .map(|p| {
                        let u = ((p.0 - x) / w).clamp(0., 1.) * (columns - 1) as f64;
                        let v = ((p.1 - y) / h).clamp(0., 1.) * (rows - 1) as f64;
                        let cx = (u.floor() as usize).min(columns - 2);
                        let cy = (v.floor() as usize).min(rows - 2);
                        let a = u - cx as f64;
                        let b = v - cy as f64;
                        let ps = [
                            points[cy * columns + cx],
                            points[cy * columns + cx + 1],
                            points[(cy + 1) * columns + cx],
                            points[(cy + 1) * columns + cx + 1],
                        ];
                        let weights = [(1. - a) * (1. - b), a * (1. - b), (1. - a) * b, a * b];
                        let q = ps
                            .into_iter()
                            .zip(weights)
                            .fold((0., 0.), |r, (p, w)| (r.0 + p.0 * w, r.1 + p.1 * w));
                        Anchor::corner(q)
                    })
                    .collect(),
            })
            .collect(),
    };
    apply(editor, id, warped, style)
}

/// Replace only the chosen paint channel with a validated multistop native gradient.
pub fn gradient(
    editor: &mut Editor,
    id: NodeId,
    stroke: bool,
    stops: &[emulsion_raster::vector::GradientStop],
    radial: bool,
    angle: f32,
) -> Result<(), String> {
    let (path, mut style) = target(&editor.doc, id)?;
    let paint = emulsion_raster::vector::PathPaint::from_stops(stops, radial, angle)?;
    if stroke {
        style.stroke = Some(stops[0].color);
        style.stroke_paint = paint;
    } else {
        style.fill = Some(stops[0].color);
        style.fill_paint = paint;
    }
    apply(editor, id, path, style)
}

#[path = "design_vector_trace.rs"]
pub mod trace;

/// Apply a true projective transform from path bounds to clockwise TL/TR/BR/BL corners.
/// Cubic curves become sampled native contours because general projected cubics are rational.
pub fn perspective(
    editor: &mut Editor,
    id: NodeId,
    corners: [(f64, f64); 4],
    tolerance: f64,
) -> Result<(), String> {
    if !tolerance.is_finite()
        || !(0.05..=10.).contains(&tolerance)
        || corners
            .iter()
            .flat_map(|p| [p.0, p.1])
            .any(|v| !v.is_finite() || v.abs() > 1e9)
    {
        return Err("Use bounded finite corners and sampling tolerance 0.05–10.".into());
    }
    let mut sign = 0.;
    for i in 0..4 {
        let a = corners[i];
        let b = corners[(i + 1) % 4];
        let c = corners[(i + 2) % 4];
        let cross = (b.0 - a.0) * (c.1 - b.1) - (b.1 - a.1) * (c.0 - b.0);
        if cross.abs() < 1e-8 || (sign != 0. && sign * cross < 0.) {
            return Err(
                "Perspective corners must form a convex quadrilateral in TL/TR/BR/BL order.".into(),
            );
        }
        sign = cross;
    }
    let [p0, p1, p2, p3] = corners;
    let dx1 = p1.0 - p2.0;
    let dx2 = p3.0 - p2.0;
    let dx3 = p0.0 - p1.0 + p2.0 - p3.0;
    let dy1 = p1.1 - p2.1;
    let dy2 = p3.1 - p2.1;
    let dy3 = p0.1 - p1.1 + p2.1 - p3.1;
    let denom = dx1 * dy2 - dx2 * dy1;
    if denom.abs() < 1e-10 {
        return Err("Perspective corners are degenerate.".into());
    }
    let g = (dx3 * dy2 - dx2 * dy3) / denom;
    let h = (dx1 * dy3 - dx3 * dy1) / denom;
    if [1., 1. + g, 1. + h, 1. + g + h].iter().any(|d| *d <= 1e-8) {
        return Err("Perspective horizon crosses the path.".into());
    }
    let (path, style) = target(&editor.doc, id)?;
    let (x, y, w, height) = vector_geometry::bounds(&path).ok_or("Empty path")?;
    if w <= 1e-8 || height <= 1e-8 {
        return Err("Perspective requires a nonzero width and height.".into());
    }
    let a = p1.0 - p0.0 + g * p1.0;
    let b = p3.0 - p0.0 + h * p3.0;
    let d = p1.1 - p0.1 + g * p1.1;
    let e = p3.1 - p0.1 + h * p3.1;
    let result = Path {
        subpaths: path
            .flatten(tolerance)
            .into_iter()
            .map(|(vertices, closed)| SubPath {
                closed,
                anchors: vertices
                    .into_iter()
                    .map(|p| {
                        let u = (p.0 - x) / w;
                        let v = (p.1 - y) / height;
                        let den = g * u + h * v + 1.;
                        Anchor::corner(((a * u + b * v + p0.0) / den, (d * u + e * v + p0.1) / den))
                    })
                    .collect(),
            })
            .collect(),
    };
    apply(editor, id, result, style)
}
#[cfg(test)]
#[path = "design_vector_tests.rs"]
mod tests;
