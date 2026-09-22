//! Agent-facing shape geometry. Mutations use the same commands as the canvas.
use crate::server::ToolResult;
use emulsion_core::{Command, Editor, Node, NodeId, NodeKind, command::Slot};
use emulsion_raster::{
    Placement,
    vector::{Path, PathStyle},
    vector_geometry::{self as geometry, BooleanOp},
};
use serde_json::{Value, json};
use std::sync::Arc;

fn error(message: impl Into<String>) -> ToolResult {
    ToolResult::error(message)
}
fn number(args: &Value, key: &str) -> Result<f64, ToolResult> {
    args.get(key)
        .and_then(Value::as_f64)
        .filter(|v| v.is_finite() && v.abs() <= 1e9)
        .ok_or_else(|| error(format!("{key} must be a finite number within +/-1e9")))
}
fn dimension(args: &Value, key: &str) -> Result<f64, ToolResult> {
    let value = number(args, key)?;
    if value <= 0. || value > 1e6 {
        return Err(error(format!(
            "{key} must be greater than zero and at most 1000000"
        )));
    }
    Ok(value)
}
fn flag(args: &Value, key: &str) -> Result<bool, ToolResult> {
    match args.get(key) {
        None => Ok(false),
        Some(v) => v
            .as_bool()
            .ok_or_else(|| error(format!("{key} must be a boolean"))),
    }
}
pub(crate) fn ensure_canvas_bounds(
    editor: &Editor,
    args: &Value,
    path: &Path,
) -> Result<(), ToolResult> {
    if flag(args, "allow_outside_canvas")? {
        return Ok(());
    }
    let Some((x, y, width, height)) = geometry::bounds(path) else {
        return Ok(());
    };
    let epsilon = 1e-6;
    if x < -epsilon
        || y < -epsilon
        || x + width > editor.doc.width as f64 + epsilon
        || y + height > editor.doc.height as f64 + epsilon
    {
        return Err(error(format!(
            "geometry bounds [{x:.2}, {y:.2}, {width:.2}, {height:.2}] extend outside the {}x{} canvas; keep editable geometry inside the document or explicitly set allow_outside_canvas to true",
            editor.doc.width, editor.doc.height
        )));
    }
    Ok(())
}
fn target(editor: &Editor, args: &Value) -> Result<(NodeId, Arc<Path>, PathStyle), ToolResult> {
    let id = args
        .get("node")
        .and_then(Value::as_u64)
        .ok_or_else(|| error("node must be an integer id"))?;
    let node = editor
        .doc
        .node(id)
        .ok_or_else(|| error(format!("no node {id}")))?;
    let locks = editor.doc.layer_locks(id);
    if editor.doc.locked_ancestor(id).is_some()
        || locks.pixels
        || locks.position
        || locks.transparency
    {
        return Err(error(format!("node {id} is locked")));
    }
    match &node.kind {
        NodeKind::Path { path, style, .. } => Ok((id, path.clone(), *style)),
        _ => Err(error(format!("node {id} is not a path"))),
    }
}
fn update(
    editor: &mut Editor,
    id: NodeId,
    path: Path,
    style: PathStyle,
) -> Result<ToolResult, ToolResult> {
    editor
        .execute(Command::SetPath {
            id,
            path: Arc::new(path),
            style,
        })
        .map_err(|e| error(e.to_string()))?;
    Ok(ToolResult::text(format!("Updated path node {id}")))
}

pub(crate) fn execute(
    editor: &mut Editor,
    name: &str,
    args: &Value,
) -> Result<ToolResult, ToolResult> {
    match name {
        "draw_shape" => draw(editor, args),
        "combine_path" => {
            let (id, original, style) = target(editor, args)?;
            let d = args
                .get("d")
                .and_then(Value::as_str)
                .ok_or_else(|| error("d must be SVG path data"))?;
            let other = Path::from_svg(d).map_err(|e| error(format!("bad path data: {e}")))?;
            let path = match args.get("operation").and_then(Value::as_str) {
                Some("component") => {
                    let mut path = (*original).clone();
                    path.subpaths.extend(other.subpaths);
                    path
                }
                Some(op @ ("add" | "subtract" | "intersect" | "exclude")) => {
                    let op = match op {
                        "add" => BooleanOp::Add,
                        "subtract" => BooleanOp::Subtract,
                        "intersect" => BooleanOp::Intersect,
                        _ => BooleanOp::Exclude,
                    };
                    geometry::boolean(&original, &other, op).map_err(error)?
                }
                _ => {
                    return Err(error(
                        "operation must be component, add, subtract, intersect or exclude",
                    ));
                }
            };
            ensure_canvas_bounds(editor, args, &path)?;
            update(editor, id, path, style)
        }
        "resize_path" => resize(editor, args),
        "align_path_components" => align(editor, args),
        _ => Err(error(format!("unknown shape tool {name}"))),
    }
}

fn draw(editor: &mut Editor, args: &Value) -> Result<ToolResult, ToolResult> {
    let (mut x, mut y, mut w, mut h) = (
        number(args, "x")?,
        number(args, "y")?,
        dimension(args, "width")?,
        dimension(args, "height")?,
    );
    if flag(args, "align_edges")? {
        x = x.round();
        y = y.round();
        w = w.round();
        h = h.round();
    }
    if w <= 0. || h <= 0. {
        return Err(error("snapped dimensions must be at least one pixel"));
    }
    let shape = args
        .get("shape")
        .and_then(Value::as_str)
        .ok_or_else(|| error("shape must be rectangle or ellipse"))?;
    let path = match shape {
        "rectangle" => geometry::rectangle(x, y, w, h),
        "ellipse" => geometry::ellipse(x, y, w, h),
        _ => return Err(error("shape must be rectangle or ellipse")),
    };
    if path.anchor_count() == 0 {
        return Err(error("shape coordinates exceed supported bounds"));
    }
    ensure_canvas_bounds(editor, args, &path)?;
    let style_args = args.get("style").cloned().unwrap_or_else(|| json!({}));
    let mut style = crate::shape_style::parse_style(
        &style_args,
        PathStyle {
            fill: Some([0, 0, 0, 255]),
            stroke: None,
            ..Default::default()
        },
    )?;
    let mode = match args.get("mode") {
        None => "shape",
        Some(v) => v
            .as_str()
            .ok_or_else(|| error("mode must be shape, path or pixels"))?,
    };
    if !["shape", "path", "pixels"].contains(&mode) {
        return Err(error("mode must be shape, path or pixels"));
    }
    if mode == "path" {
        style.fill = None;
        style.stroke = None;
    }
    let name = match args.get("name") {
        None => shape,
        Some(v) => v.as_str().ok_or_else(|| error("name must be a string"))?,
    };
    let slot = match args.get("above") {
        None => Slot::TOP,
        Some(v) => {
            let id = v
                .as_u64()
                .ok_or_else(|| error("above must be an integer node id"))?;
            let node = editor
                .doc
                .node(id)
                .ok_or_else(|| error(format!("no node {id}")))?;
            Slot {
                parent: node.parent,
                index: editor
                    .doc
                    .children(node.parent)
                    .iter()
                    .position(|n| *n == id)
                    .unwrap_or(0)
                    + 1,
            }
        }
    };
    let node = if mode == "pixels" {
        Node::raster(
            0,
            name,
            Arc::new(path.rasterize(&style, editor.doc.width, editor.doc.height)),
            Placement::default(),
        )
    } else {
        Node::path(
            0,
            name,
            Arc::new(path),
            style,
            editor.doc.width,
            editor.doc.height,
        )
    };
    let id = editor
        .execute(Command::AddNode {
            node: Box::new(node),
            slot,
        })
        .map_err(|e| error(e.to_string()))?
        .ok_or_else(|| error("no node created"))?;
    Ok(ToolResult::text(format!(
        "Added {shape} {name:?} as node {id} ({mode})"
    )))
}

fn resize(editor: &mut Editor, args: &Value) -> Result<ToolResult, ToolResult> {
    let (id, original, style) = target(editor, args)?;
    let (x, y, w, h) = geometry::bounds(&original).ok_or_else(|| error("path is empty"))?;
    if w <= 0. || h <= 0. {
        return Err(error("path must have nonzero width and height"));
    }
    let width = args
        .get("width")
        .map(|_| dimension(args, "width"))
        .transpose()?;
    let height = args
        .get("height")
        .map(|_| dimension(args, "height"))
        .transpose()?;
    if width.is_none() && height.is_none() {
        return Err(error("provide width or height"));
    }
    let linked = flag(args, "linked")?;
    let (mut nw, mut nh) = match (width, height, linked) {
        (Some(a), Some(b), true) if (a / w - b / h).abs() > 1e-8 => {
            return Err(error(
                "linked width and height must preserve the current aspect ratio",
            ));
        }
        (Some(a), None, true) => (a, h * a / w),
        (None, Some(b), true) => (w * b / h, b),
        _ => (width.unwrap_or(w), height.unwrap_or(h)),
    };
    let snap = flag(args, "align_edges")?;
    if snap {
        nw = nw.round();
        nh = nh.round();
    }
    if nw <= 0. || nh <= 0. || nw > 1e6 || nh > 1e6 {
        return Err(error(
            "resulting dimensions must be positive and at most 1000000",
        ));
    }
    let (tx, ty) = if snap { (x.round(), y.round()) } else { (x, y) };
    let mut path = (*original).clone();
    path.transform(
        glam::DAffine2::from_translation(glam::dvec2(tx, ty))
            * glam::DAffine2::from_scale(glam::dvec2(nw / w, nh / h))
            * glam::DAffine2::from_translation(glam::dvec2(-x, -y)),
    );
    update(editor, id, path, style)
}

fn align(editor: &mut Editor, args: &Value) -> Result<ToolResult, ToolResult> {
    let (id, original, style) = target(editor, args)?;
    let (axis, position, distribute) = match args.get("alignment").and_then(Value::as_str) {
        Some("left") => (0, 0., false),
        Some("center_x") => (0, 0.5, false),
        Some("right") => (0, 1., false),
        Some("top") => (1, 0., false),
        Some("center_y") => (1, 0.5, false),
        Some("bottom") => (1, 1., false),
        Some("distribute_x") => (0, 0.5, true),
        Some("distribute_y") => (1, 0.5, true),
        _ => return Err(error("unknown component alignment")),
    };
    let selected = args
        .get("component")
        .map(|v| {
            v.as_u64()
                .and_then(|i| usize::try_from(i).ok())
                .ok_or_else(|| error("component must be a zero-based integer index"))
        })
        .transpose()?;
    if selected.is_some_and(|i| i >= original.subpaths.len()) {
        return Err(error("component index out of range"));
    }
    if distribute && selected.is_some() {
        return Err(error(
            "distribution applies to all components; omit component",
        ));
    }
    let total = geometry::bounds(&original).ok_or_else(|| error("path is empty"))?;
    let at = |b: (f64, f64, f64, f64)| {
        if axis == 0 {
            b.0 + b.2 * position
        } else {
            b.1 + b.3 * position
        }
    };
    let mut items: Vec<_> = original
        .subpaths
        .iter()
        .enumerate()
        .filter_map(|(i, s)| {
            geometry::bounds(&Path {
                subpaths: vec![s.clone()],
            })
            .map(|b| (i, b))
        })
        .collect();
    let mut path = (*original).clone();
    if distribute {
        if items.len() < 3 {
            return Err(error("distribution needs at least three components"));
        }
        items.sort_by(|a, b| at(a.1).total_cmp(&at(b.1)));
        let first = at(items[0].1);
        let step = (at(items[items.len() - 1].1) - first) / (items.len() - 1) as f64;
        for (rank, (index, b)) in items.iter().enumerate() {
            translate(&mut path, *index, axis, first + rank as f64 * step - at(*b));
        }
    } else {
        for (index, b) in items {
            if selected.is_none_or(|i| i == index) {
                translate(&mut path, index, axis, at(total) - at(b));
            }
        }
    }
    update(editor, id, path, style)
}
fn translate(path: &mut Path, index: usize, axis: usize, delta: f64) {
    for anchor in &mut path.subpaths[index].anchors {
        for p in [&mut anchor.p, &mut anchor.h_in, &mut anchor.h_out] {
            if axis == 0 {
                p.0 += delta;
            } else {
                p.1 += delta;
            }
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use emulsion_core::Document;

    fn editor() -> Editor {
        Editor::new(Document::new(100, 100), None)
    }
    fn call(editor: &mut Editor, name: &str, args: Value) {
        let result = execute(editor, name, &args);
        assert!(result.is_ok(), "{result:?}");
    }
    fn add(editor: &mut Editor) -> NodeId {
        call(
            editor,
            "draw_shape",
            json!({"shape":"rectangle","x":10,"y":10,"width":30,"height":20}),
        );
        editor.doc.nodes.last().unwrap().id
    }
    fn path(editor: &Editor, id: NodeId) -> &Path {
        let NodeKind::Path { path, .. } = &editor.doc.node(id).unwrap().kind else {
            panic!("not a path")
        };
        path
    }
    fn alpha(editor: &Editor, id: NodeId, x: u32, y: u32) -> u16 {
        let NodeKind::Path { cache, .. } = &editor.doc.node(id).unwrap().kind else {
            panic!("not a path")
        };
        cache.get(x, y)[3]
    }

    #[test]
    fn primitive_modes_snapping_and_undo() {
        for mode in ["shape", "path", "pixels"] {
            let mut e = editor();
            call(
                &mut e,
                "draw_shape",
                json!({"shape":"ellipse","x":10.4,"y":10.4,"width":30.4,"height":20.4,"align_edges":true,"mode":mode}),
            );
            let node = e.doc.nodes.last().unwrap();
            if mode == "pixels" {
                assert!(matches!(node.kind, NodeKind::Raster { .. }));
            } else {
                assert_eq!(
                    geometry::bounds(path(&e, node.id)),
                    Some((10., 10., 30., 20.))
                );
                assert!(
                    path(&e, node.id).subpaths[0]
                        .anchors
                        .iter()
                        .all(|a| a.smooth)
                );
                assert_eq!(alpha(&e, node.id, 25, 20) > 0, mode == "shape");
            }
            assert_eq!(e.history.len(), 1);
            assert!(e.undo());
            assert!(e.doc.nodes.is_empty());
        }
    }

    #[test]
    fn all_boolean_operations_preserve_layer_and_undo() {
        for (operation, left, overlap, right) in [
            ("add", true, true, true),
            ("subtract", true, false, false),
            ("intersect", false, true, false),
            ("exclude", true, false, true),
        ] {
            let mut e = editor();
            let id = add(&mut e);
            let original = path(&e, id).clone();
            call(
                &mut e,
                "combine_path",
                json!({"node":id,"d":"M25 10H55V30H25Z","operation":operation}),
            );
            assert_eq!(e.doc.nodes.len(), 1);
            assert_eq!(alpha(&e, id, 15, 20) > 0, left);
            assert_eq!(alpha(&e, id, 30, 20) > 0, overlap);
            assert_eq!(alpha(&e, id, 50, 20) > 0, right);
            assert!(e.undo());
            assert_eq!(path(&e, id), &original);
        }
    }

    #[test]
    fn components_alignment_distribution_and_linked_resize() {
        let mut e = editor();
        let id = add(&mut e);
        call(
            &mut e,
            "combine_path",
            json!({"node":id,"operation":"component","d":"M45 40H55V50H45Z M80 60H90V70H80Z"}),
        );
        assert_eq!(path(&e, id).subpaths.len(), 3);
        call(
            &mut e,
            "align_path_components",
            json!({"node":id,"alignment":"left","component":1}),
        );
        assert_eq!(path(&e, id).subpaths[1].anchors[0].p.0, 10.);
        assert_eq!(path(&e, id).subpaths[2].anchors[0].p.0, 80.);
        assert!(e.undo());
        call(
            &mut e,
            "align_path_components",
            json!({"node":id,"alignment":"distribute_x"}),
        );
        assert_eq!(path(&e, id).subpaths[1].anchors[0].p.0, 50.);
        assert!(e.undo());
        let before = path(&e, id).clone();
        call(
            &mut e,
            "resize_path",
            json!({"node":id,"width":40,"linked":true}),
        );
        assert_eq!(geometry::bounds(path(&e, id)), Some((10., 10., 40., 30.)));
        assert!(e.undo());
        assert_eq!(path(&e, id), &before);
    }

    #[test]
    fn every_component_alignment_and_vertical_distribution() {
        for (alignment, expected) in [
            ("left", (10., 40.)),
            ("center_x", (45., 40.)),
            ("right", (80., 40.)),
            ("top", (45., 10.)),
            ("center_y", (45., 35.)),
            ("bottom", (45., 60.)),
            ("distribute_y", (45., 37.5)),
        ] {
            let mut e = editor();
            let id = add(&mut e);
            call(
                &mut e,
                "combine_path",
                json!({"node":id,"operation":"component","d":"M45 40H55V50H45Z M80 60H90V70H80Z"}),
            );
            let original = path(&e, id).clone();
            let history_len = e.history.len();
            let mut args = json!({"node":id,"alignment":alignment});
            if alignment != "distribute_y" {
                args["component"] = json!(1);
            }
            call(&mut e, "align_path_components", args);
            assert_eq!(
                path(&e, id).subpaths[1].anchors[0].p,
                expected,
                "{alignment}"
            );
            if alignment == "center_x" {
                assert_eq!(
                    e.history.len(),
                    history_len,
                    "already-centered component is a no-op"
                );
            } else {
                assert_eq!(e.history.len(), history_len + 1);
                assert!(e.undo());
            }
            assert_eq!(path(&e, id), &original);
        }
    }

    #[test]
    fn invalid_input_and_locks_do_not_mutate_document() {
        let mut e = editor();
        let id = add(&mut e);
        let initial = e.doc.clone();
        let history = e.history.len();
        for (tool, args) in [
            (
                "draw_shape",
                json!({"shape":"rectangle","x":0,"y":0,"width":-1,"height":10}),
            ),
            (
                "draw_shape",
                json!({"shape":"ellipse","x":0,"y":0,"width":10,"height":10,"mode":"bad"}),
            ),
            (
                "draw_shape",
                json!({"shape":"ellipse","x":-10,"y":0,"width":30,"height":30}),
            ),
            (
                "resize_path",
                json!({"node":id,"width":10,"height":10,"linked":true}),
            ),
            (
                "combine_path",
                json!({"node":id,"d":"bad","operation":"add"}),
            ),
            (
                "align_path_components",
                json!({"node":id,"alignment":"left","component":10}),
            ),
            (
                "align_path_components",
                json!({"node":id,"alignment":"distribute_x"}),
            ),
        ] {
            assert!(execute(&mut e, tool, &args).is_err());
            assert_eq!(e.doc.nodes, initial.nodes);
            assert_eq!(e.history.len(), history);
        }
        for lock in 0..4 {
            let n = e.doc.node_mut(id).unwrap();
            n.locked = lock == 0;
            n.locks.pixels = lock == 1;
            n.locks.position = lock == 2;
            n.locks.transparency = lock == 3;
            for (tool, args) in [
                ("resize_path", json!({"node":id,"width":50})),
                (
                    "combine_path",
                    json!({"node":id,"d":"M0 0H10V10H0Z","operation":"component"}),
                ),
                (
                    "align_path_components",
                    json!({"node":id,"alignment":"left"}),
                ),
            ] {
                assert!(execute(&mut e, tool, &args).is_err());
                assert_eq!(e.history.len(), history);
            }
        }
    }

    #[test]
    fn agent_shapes_stay_on_canvas_unless_explicitly_allowed() {
        let mut e = editor();
        assert!(
            execute(
                &mut e,
                "draw_shape",
                &json!({"shape":"ellipse","x":80,"y":80,"width":40,"height":40}),
            )
            .is_err()
        );
        assert!(e.doc.nodes.is_empty());
        call(
            &mut e,
            "draw_shape",
            json!({"shape":"ellipse","x":80,"y":80,"width":40,"height":40,"allow_outside_canvas":true}),
        );
        assert_eq!(e.doc.nodes.len(), 1);
    }
}
