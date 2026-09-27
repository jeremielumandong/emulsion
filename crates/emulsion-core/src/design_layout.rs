//! Persistent responsive group layouts. Reflow is part of the originating edit.
use crate::{Document, NodeId, NodeKind};
use serde::{Deserialize, Serialize};
use std::collections::BTreeMap;

#[derive(Clone, Copy, Debug, Default, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum Flow {
    Row,
    #[default]
    Column,
    Grid,
}
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum Align {
    #[default]
    Start,
    Center,
    End,
}
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq, Serialize, Deserialize)]
#[serde(default)]
pub struct Child {
    pub absolute: bool,
    pub fill_width: bool,
}
#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
#[serde(default)]
pub struct Frame {
    pub boundary: NodeId,
    pub flow: Flow,
    /// Top, right, bottom, left, in document pixels.
    pub padding: [f64; 4],
    pub gap: f64,
    pub columns: u32,
    pub wrap: bool,
    pub align: Align,
    /// Resize the boundary to the laid-out content plus vertical padding.
    pub hug_height: bool,
    pub children: BTreeMap<NodeId, Child>,
}
impl Default for Frame {
    fn default() -> Self {
        Self {
            boundary: 0,
            flow: Flow::Column,
            padding: [24.; 4],
            gap: 16.,
            columns: 2,
            wrap: true,
            align: Align::Start,
            hug_height: false,
            children: BTreeMap::new(),
        }
    }
}
pub fn validate(frames: &BTreeMap<NodeId, Frame>, doc: &Document) -> Result<(), String> {
    if frames.len() > 256 {
        return Err("A page supports up to 256 responsive layouts.".into());
    }
    for (id, frame) in frames {
        if !doc.node(*id).is_some_and(|n| n.is_group())
            || !doc
                .node(frame.boundary)
                .is_some_and(|n| n.parent == Some(*id) && matches!(n.kind, NodeKind::Path { .. }))
        {
            return Err("A responsive layout needs a group and its own frame boundary.".into());
        }
        if let NodeKind::Path { path, .. } = &doc.node(frame.boundary).unwrap().kind {
            let rectangular = path.subpaths.len() == 1
                && path.subpaths[0].closed
                && path.subpaths[0].anchors.len() == 4
                && path.subpaths[0].anchors.iter().enumerate().all(|(i, a)| {
                    let b = &path.subpaths[0].anchors[(i + 1) % 4];
                    a.p == a.h_in
                        && a.p == a.h_out
                        && path.subpaths[0].anchors[..i]
                            .iter()
                            .all(|previous| previous.p != a.p)
                        && ((a.p.0 - b.p.0).abs() < 0.001 || (a.p.1 - b.p.1).abs() < 0.001)
                });
            let dimensions = emulsion_raster::vector_geometry::bounds(path);
            if !rectangular
                || !dimensions.is_some_and(|(x, y, w, h)| {
                    x.is_finite()
                        && y.is_finite()
                        && (1. ..=100000.).contains(&w)
                        && (1. ..=100000.).contains(&h)
                })
            {
                return Err(
                    "Remove automatic layout before rotating or reshaping its rectangular frame."
                        .into(),
                );
            }
        }
        if !frame
            .padding
            .iter()
            .chain([&frame.gap])
            .all(|v| v.is_finite() && (0. ..=10000.).contains(v))
            || !(1..=64).contains(&frame.columns)
        {
            return Err("Layout spacing must be 0–10000 px and columns 1–64.".into());
        }
        if doc.children(Some(*id)).len() > 512 {
            return Err("A responsive layout supports up to 511 content objects.".into());
        }
        for child in frame.children.keys() {
            if *child == frame.boundary || !doc.node(*child).is_some_and(|n| n.parent == Some(*id))
            {
                return Err("Layout sizing refers to an object outside the frame.".into());
            }
        }
    }
    Ok(())
}

/// Keep metadata valid after deleting/reparenting a child or ungrouping a frame.
pub(crate) fn prune(doc: &mut Document) {
    let parents: BTreeMap<_, _> = doc.nodes.iter().map(|n| (n.id, n.parent)).collect();
    doc.design
        .frames
        .retain(|id, frame| parents.get(&frame.boundary) == Some(&Some(*id)));
    for (id, frame) in &mut doc.design.frames {
        frame
            .children
            .retain(|child, _| parents.get(child) == Some(&Some(*id)));
    }
}

/// Return a group's boundary dimensions in document coordinates.
pub fn bounds(doc: &Document, id: NodeId) -> Option<(f64, f64, f64, f64)> {
    let boundary = doc.design.frames.get(&id)?.boundary;
    let NodeKind::Path { path, .. } = &doc.node(boundary)?.kind else {
        return None;
    };
    emulsion_raster::vector_geometry::bounds(path)
}

/// Resize the semantic frame without treating its decorative stroke as content
/// geometry or changing that stroke width during repeated responsive reflow.
fn resize_frame(doc: &mut Document, id: NodeId, width: f64, height: f64) -> Result<(), String> {
    let (x, y, w, h) = bounds(doc, id).ok_or("Missing layout boundary")?;
    if (w - width).abs() <= 0.01 && (h - height).abs() <= 0.01 {
        return Ok(());
    }
    let boundary = doc.design.frames[&id].boundary;
    let NodeKind::Path { style, .. } = doc.node(boundary).ok_or("Missing layout boundary")?.kind
    else {
        return Err("Missing layout boundary".into());
    };
    crate::transform::transform_nodes(
        doc,
        &[boundary],
        [
            width / w,
            0.,
            0.,
            height / h,
            x * (1. - width / w),
            y * (1. - height / h),
        ],
    )
    .map_err(|e| e.to_string())?;
    let kind = crate::Node::path(
        boundary,
        "Layout frame",
        std::sync::Arc::new(emulsion_raster::vector_geometry::rectangle(
            x, y, width, height,
        )),
        style,
        doc.width,
        doc.height,
    )
    .kind;
    doc.node_mut(boundary).unwrap().kind = kind;
    Ok(())
}

fn place(doc: &mut Document, id: NodeId, x: f64, y: f64, width: Option<f64>) -> Result<(), String> {
    if let Some(width) = width {
        if let Some((_, _, _, height)) = bounds(doc, id) {
            resize_frame(doc, id, width.max(1.), height)?;
        } else {
            let target = doc
                .design
                .frames
                .get(&id)
                .map_or(id, |frame| frame.boundary);
            let node = doc.node(target).ok_or("Missing layout child")?;
            if let NodeKind::Text { spec, .. } = &node.kind {
                if spec.rotation == 0. && spec.scale_x == 1. && spec.scale_y == 1. {
                    let mut spec = (**spec).clone();
                    spec.width = Some(width.max(1.) as f32);
                    if let NodeKind::Text { spec: current, .. } = &node.kind
                        && **current != spec
                    {
                        if doc.locked_ancestor(id).is_some()
                            || (doc.layer_locks(id).pixels || doc.layer_locks(id).transparency)
                        {
                            return Err("Unlock the text before changing layout.".into());
                        }
                        let kind =
                            crate::Node::text(id, &node.name, spec, doc.width, doc.height).kind;
                        doc.node_mut(id).unwrap().kind = kind;
                    }
                }
            } else if let Some(b) = crate::geometry::node_bounds(doc, target)
                && b.w > 0
                && b.w as f64 != width.max(1.).ceil()
            {
                let sx = width.max(1.) / b.w as f64;
                crate::transform::transform_nodes(
                    doc,
                    &[target],
                    [sx, 0., 0., 1., b.x as f64 * (1. - sx), 0.],
                )
                .map_err(|e| e.to_string())?;
            }
        }
    }
    let Some(b) = item_bounds(doc, id) else {
        return Ok(());
    };
    // Snap cells once to prevent fractional bounds from drifting on reflow.
    let dx = x.round() - b.x as f64;
    let dy = y.round() - b.y as f64;
    if dx.abs() > 0.01 || dy.abs() > 0.01 {
        crate::transform::transform_nodes(doc, &[id], [1., 0., 0., 1., dx, dy])
            .map_err(|e| e.to_string())?;
    }
    Ok(())
}

fn item_bounds(doc: &Document, id: NodeId) -> Option<emulsion_raster::IRect> {
    if let Some((x, y, w, h)) = bounds(doc, id) {
        return Some(emulsion_raster::IRect::new(
            x.floor() as i32,
            y.floor() as i32,
            ((x + w).ceil() - x.floor()) as i32,
            ((y + h).ceil() - y.floor()) as i32,
        ));
    }
    crate::geometry::node_bounds(doc, id)
}

pub(crate) fn reflow(doc: &mut Document) -> Result<(), String> {
    validate(&doc.design.frames, doc)?;
    for id in doc.children(None) {
        reflow_subtree(doc, id)?;
    }
    Ok(())
}

fn reflow_subtree(doc: &mut Document, id: NodeId) -> Result<(), String> {
    if let Some(frame) = doc.design.frames.get(&id).cloned() {
        reflow_frame(doc, id, &frame)
    } else {
        for child in doc.children(Some(id)) {
            reflow_subtree(doc, child)?;
        }
        Ok(())
    }
}

fn reflow_frame(doc: &mut Document, id: NodeId, frame: &Frame) -> Result<(), String> {
    let Some((x, y, w, h)) = bounds(doc, id) else {
        return Ok(());
    };
    let left = x + frame.padding[3];
    let top = y + frame.padding[0];
    let available = (w - frame.padding[1] - frame.padding[3]).max(1.);
    let children: Vec<_> = doc
        .children(Some(id))
        .into_iter()
        .filter(|child| {
            *child != frame.boundary && !frame.children.get(child).is_some_and(|s| s.absolute)
        })
        .collect();
    let mut cursor = (left, top);
    let mut row_height: f64 = 0.;
    let cell =
        ((available - frame.gap * (frame.columns - 1) as f64) / frame.columns as f64).max(1.);
    let flexible = children
        .iter()
        .filter(|id| frame.children.get(id).is_some_and(|s| s.fill_width))
        .count();
    let fixed: f64 = children
        .iter()
        .filter(|id| !frame.children.get(id).is_some_and(|s| s.fill_width))
        .filter_map(|id| item_bounds(doc, *id))
        .map(|b| b.w as f64)
        .sum();
    let row_fill = ((available - fixed - frame.gap * children.len().saturating_sub(1) as f64)
        / flexible.max(1) as f64)
        .max(1.);
    let mut bottom = top;
    for (index, child) in children.into_iter().enumerate() {
        let Some(b) = item_bounds(doc, child) else {
            continue;
        };
        let sizing = frame.children.get(&child).copied().unwrap_or_default();
        let width = if sizing.fill_width {
            Some(if frame.flow == Flow::Grid {
                cell
            } else if frame.flow == Flow::Row {
                row_fill
            } else {
                available
            })
        } else {
            None
        };
        let child_width = width.unwrap_or(b.w as f64);
        match frame.flow {
            Flow::Column => {
                let offset = match frame.align {
                    Align::Start => 0.,
                    Align::Center => (available - child_width) / 2.,
                    Align::End => available - child_width,
                };
                place(doc, child, left + offset.max(0.), cursor.1, width)?;
                reflow_subtree(doc, child)?;
                cursor.1 += item_bounds(doc, child).map_or(b.h, |b| b.h) as f64 + frame.gap;
            }
            Flow::Row => {
                if frame.wrap && cursor.0 > left && cursor.0 + child_width > left + available + 0.01
                {
                    cursor.0 = left;
                    cursor.1 += row_height + frame.gap;
                    row_height = 0.;
                }
                place(doc, child, cursor.0, cursor.1, width)?;
                reflow_subtree(doc, child)?;
                cursor.0 += child_width + frame.gap;
                row_height = row_height.max(item_bounds(doc, child).map_or(b.h, |b| b.h) as f64);
            }
            Flow::Grid => {
                let col = index % frame.columns as usize;
                if col == 0 && index > 0 {
                    cursor.1 += row_height + frame.gap;
                    row_height = 0.;
                }
                let offset = match frame.align {
                    Align::Start => 0.,
                    Align::Center => (cell - child_width) / 2.,
                    Align::End => cell - child_width,
                };
                place(
                    doc,
                    child,
                    left + col as f64 * (cell + frame.gap) + offset.max(0.),
                    cursor.1,
                    width,
                )?;
                reflow_subtree(doc, child)?;
                row_height = row_height.max(item_bounds(doc, child).map_or(b.h, |b| b.h) as f64);
            }
        }
        if let Some(b) = item_bounds(doc, child) {
            bottom = bottom.max(b.bottom() as f64);
        }
    }
    for child in doc.children(Some(id)) {
        if child != frame.boundary && frame.children.get(&child).is_some_and(|s| s.absolute) {
            reflow_subtree(doc, child)?;
        }
    }
    if frame.hug_height {
        let height = (bottom - y + frame.padding[2]).ceil().max(1.);
        if height > 100000. {
            return Err("Layout content exceeds the maximum frame height.".into());
        }
        if (h - height).abs() > 0.01 {
            resize_frame(doc, id, w, height)?;
        }
    }
    Ok(())
}

/// Attach responsive layout to an existing group. The caller owns the transaction.
pub fn enable(
    editor: &mut crate::Editor,
    group: NodeId,
    mut frame: Frame,
    size: (f64, f64),
) -> Result<(), String> {
    use crate::{Command, Node, command::Slot};
    use emulsion_raster::{vector::PathStyle, vector_geometry::rectangle};
    if !editor.doc.node(group).is_some_and(|n| n.is_group()) {
        return Err("Select a group to add responsive layout.".into());
    }
    if ![size.0, size.1]
        .into_iter()
        .all(|n| n.is_finite() && (1. ..=100000.).contains(&n))
    {
        return Err("Choose frame dimensions from 1–100000 px.".into());
    }
    let existing = editor.doc.design.frames.get(&group).map(|f| f.boundary);
    let origin = existing
        .and_then(|_| bounds(&editor.doc, group))
        .map(|(x, y, _, _)| (x, y))
        .unwrap_or_else(|| {
            let b = crate::geometry::node_bounds(&editor.doc, group).unwrap_or_default();
            (b.x as f64, b.y as f64)
        });
    let path = std::sync::Arc::new(rectangle(origin.0, origin.1, size.0, size.1));
    let style = PathStyle {
        fill: Some([255; 4]),
        stroke: None,
        ..Default::default()
    };
    let boundary_command = if let Some(id) = existing {
        let style = match &editor.doc.node(id).unwrap().kind {
            NodeKind::Path { style, .. } => *style,
            _ => style,
        };
        Command::SetPath { id, path, style }
    } else {
        Command::AddNode {
            node: Box::new(Node::path(
                0,
                "Layout frame",
                path,
                style,
                editor.doc.width,
                editor.doc.height,
            )),
            slot: Slot {
                parent: Some(group),
                index: 0,
            },
        }
    };
    // Validate both the geometry and dependent reflow before issuing either
    // command. Even callers without a transaction get no orphan boundary or
    // partial resize when a protected child or invalid setting rejects layout.
    let mut trial = editor.doc.clone();
    let created = boundary_command
        .apply(&mut trial)
        .map_err(|e| e.to_string())?;
    frame.boundary = existing.or(created).ok_or("Missing layout boundary")?;
    let mut design = trial.design.clone();
    design.frames.insert(group, frame);
    let settings_command = Command::SetDesign {
        design: Box::new(design),
    };
    settings_command
        .apply(&mut trial)
        .map_err(|e| e.to_string())?;
    editor
        .execute(boundary_command)
        .map_err(|e| e.to_string())?;
    editor
        .execute(settings_command)
        .map_err(|e| e.to_string())?;
    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::{Command, Editor, Node, command::Slot, fragment::Fragment};
    use emulsion_raster::{vector::PathStyle, vector_geometry::rectangle};
    use std::sync::Arc;
    fn fixture() -> (Editor, NodeId, Vec<NodeId>) {
        let mut editor = Editor::new(Document::new(600, 400), None);
        let ids = (0..3)
            .map(|i| {
                editor
                    .execute(Command::AddNode {
                        node: Box::new(Node::path(
                            0,
                            "Item",
                            Arc::new(rectangle(i as f64 * 100., 0., 50., 40.)),
                            PathStyle {
                                fill: Some([30, 40, 50, 255]),
                                stroke: None,
                                ..Default::default()
                            },
                            600,
                            400,
                        )),
                        slot: Slot::TOP,
                    })
                    .unwrap()
                    .unwrap()
            })
            .collect::<Vec<_>>();
        let group = editor
            .execute(Command::Group {
                ids: ids.clone(),
                name: "Layout".into(),
            })
            .unwrap()
            .unwrap();
        (editor, group, ids)
    }
    fn rect(editor: &Editor, id: NodeId) -> emulsion_raster::IRect {
        crate::geometry::node_bounds(&editor.doc, id).unwrap()
    }
    #[test]
    fn flexible_row_shares_space_and_hug_height_tracks_nested_content() {
        let (mut editor, inner, ids) = fixture();
        let mut frame = Frame {
            flow: Flow::Row,
            padding: [10.; 4],
            gap: 10.,
            hug_height: true,
            ..Default::default()
        };
        for id in &ids[1..] {
            frame.children.insert(
                *id,
                Child {
                    fill_width: true,
                    ..Default::default()
                },
            );
        }
        editor.begin("Layout");
        enable(&mut editor, inner, frame, (300., 300.)).unwrap();
        editor.end();
        assert_eq!(
            (
                rect(&editor, ids[0]).w,
                rect(&editor, ids[1]).w,
                rect(&editor, ids[2]).w
            ),
            (50, 105, 105)
        );
        assert_eq!(bounds(&editor.doc, inner).unwrap().3, 60.);
        let outer = editor
            .execute(Command::Group {
                ids: vec![inner],
                name: "Outer".into(),
            })
            .unwrap()
            .unwrap();
        editor.begin("Outer layout");
        enable(
            &mut editor,
            outer,
            Frame {
                padding: [10.; 4],
                hug_height: true,
                ..Default::default()
            },
            (340., 400.),
        )
        .unwrap();
        editor.end();
        assert_eq!(bounds(&editor.doc, outer).unwrap().3, 80.);
        let before = editor.doc.clone();
        let b = rect(&editor, ids[0]);
        let NodeKind::Path { style, .. } = editor.doc.node(ids[0]).unwrap().kind else {
            panic!()
        };
        editor
            .execute(Command::SetPath {
                id: ids[0],
                path: Arc::new(rectangle(b.x as f64, b.y as f64, 50., 90.)),
                style,
            })
            .unwrap();
        assert_eq!(bounds(&editor.doc, inner).unwrap().3, 110.);
        assert_eq!(bounds(&editor.doc, outer).unwrap().3, 130.);
        let after = editor.doc.clone();
        for _ in 0..3 {
            editor
                .execute(Command::SetDesign {
                    design: Box::new(editor.doc.design.clone()),
                })
                .unwrap();
        }
        assert_eq!(editor.doc, after);
        editor.undo();
        assert_eq!(editor.doc, before);
        let json = serde_json::to_string(&after.design).unwrap();
        let restored: crate::design_metadata::Design = serde_json::from_str(&json).unwrap();
        assert_eq!(restored, after.design);
    }
    #[test]
    fn frame_resize_wraps_in_the_same_undo_step_and_clipboard_remaps_layout() {
        let (mut editor, group, ids) = fixture();
        editor.begin("Layout");
        enable(
            &mut editor,
            group,
            Frame {
                flow: Flow::Row,
                padding: [10.; 4],
                gap: 10.,
                ..Default::default()
            },
            (200., 200.),
        )
        .unwrap();
        editor.end();
        assert_eq!(
            (rect(&editor, ids[0]).x, rect(&editor, ids[2]).x),
            (10, 130)
        );
        let original = editor.doc.clone();
        let boundary = editor.doc.design.frames[&group].boundary;
        editor
            .execute(Command::SetPath {
                id: boundary,
                path: Arc::new(rectangle(0., 0., 120., 200.)),
                style: PathStyle {
                    fill: Some([255; 4]),
                    stroke: None,
                    ..Default::default()
                },
            })
            .unwrap();
        assert_eq!((rect(&editor, ids[1]).x, rect(&editor, ids[1]).y), (10, 60));
        assert!(editor.undo());
        assert_eq!(editor.doc, original);
        let fragment = Fragment::capture(&editor.doc, &[group]).unwrap();
        let pasted = fragment.paste(&mut editor, Slot::TOP, (250., 30.)).unwrap()[0];
        assert_ne!(editor.doc.design.frames[&pasted].boundary, boundary);
        assert_eq!(editor.doc.design.frames.len(), 2);
        assert!(editor.undo());
        assert_eq!(editor.doc, original);
        editor
            .execute(Command::DuplicateNode { id: group })
            .unwrap();
        assert_eq!(editor.doc.design.frames.len(), 2);
        assert!(editor.undo());
        assert_eq!(editor.doc, original);
        // Re-evaluation without geometry changes must not drift fractional paths.
        for _ in 0..4 {
            editor
                .execute(Command::SetDesign {
                    design: Box::new(editor.doc.design.clone()),
                })
                .unwrap();
        }
        assert_eq!(editor.doc, original);
    }
    #[test]
    fn fill_text_reflows_without_scaling_font_and_locks_reject_dependent_moves() {
        let (mut editor, group, ids) = fixture();
        let text = editor
            .execute(Command::AddNode {
                node: Box::new(Node::text(
                    0,
                    "Text",
                    crate::text::TextSpec {
                        text: "One short line".into(),
                        font: "Geist".into(),
                        size: 20.,
                        width: Some(150.),
                        ..Default::default()
                    },
                    600,
                    400,
                )),
                slot: Slot {
                    parent: Some(group),
                    index: 0,
                },
            })
            .unwrap()
            .unwrap();
        let mut frame = Frame {
            padding: [10.; 4],
            ..Default::default()
        };
        frame.children.insert(
            text,
            Child {
                fill_width: true,
                ..Default::default()
            },
        );
        editor.begin("Layout");
        enable(&mut editor, group, frame, (200., 300.)).unwrap();
        editor.end();
        let NodeKind::Text { spec, .. } = &editor.doc.node(text).unwrap().kind else {
            panic!()
        };
        assert_eq!(spec.size, 20.);
        assert_eq!(spec.width, Some(180.));
        let mut spec = (**spec).clone();
        spec.text="Many more words now wrap over several lines without flattening or changing the original font size.".into();
        editor
            .execute(Command::SetLocked {
                id: ids[0],
                locked: true,
            })
            .unwrap();
        let original = editor.doc.clone();
        assert!(
            editor
                .execute(Command::SetText {
                    id: text,
                    spec: Box::new(spec.clone())
                })
                .is_err()
        );
        assert_eq!(editor.doc, original);
        editor
            .execute(Command::SetLocked {
                id: ids[0],
                locked: false,
            })
            .unwrap();
        let previous_y = rect(&editor, ids[0]).y;
        editor
            .execute(Command::SetText {
                id: text,
                spec: Box::new(spec),
            })
            .unwrap();
        assert!(rect(&editor, ids[0]).y > previous_y);
        assert!(editor.undo());
        assert_eq!(rect(&editor, ids[0]).y, previous_y);
    }
    #[test]
    fn grid_absolute_children_and_invalid_settings_are_safe() {
        let (mut editor, group, ids) = fixture();
        let original_absolute = editor.doc.node(ids[2]).unwrap().clone();
        let mut frame = Frame {
            flow: Flow::Grid,
            padding: [10.; 4],
            gap: 10.,
            ..Default::default()
        };
        frame.children.insert(
            ids[2],
            Child {
                absolute: true,
                ..Default::default()
            },
        );
        editor.begin("Layout");
        enable(&mut editor, group, frame, (200., 200.)).unwrap();
        editor.end();
        assert_eq!(rect(&editor, ids[1]).x, 105);
        assert_eq!(editor.doc.node(ids[2]).unwrap(), &original_absolute);
        let original = editor.doc.clone();
        let mut bad = editor.doc.design.clone();
        bad.frames.get_mut(&group).unwrap().gap = f64::NAN;
        assert!(
            editor
                .execute(Command::SetDesign {
                    design: Box::new(bad)
                })
                .is_err()
        );
        assert_eq!(editor.doc, original);
        editor.execute(Command::Ungroup { id: group }).unwrap();
        assert!(editor.doc.design.frames.is_empty());
        editor.undo();
        assert_eq!(editor.doc, original);
    }

    #[test]
    fn nested_fractional_grid_layout_is_stable_and_keeps_text_source_size() {
        let (mut editor, inner, _) = fixture();
        editor.begin("Inner layout");
        enable(&mut editor, inner, Frame::default(), (150., 180.)).unwrap();
        editor.end();
        let outer = editor
            .execute(Command::Group {
                ids: vec![inner],
                name: "Outer".into(),
            })
            .unwrap()
            .unwrap();
        let mut frame = Frame {
            flow: Flow::Grid,
            columns: 3,
            padding: [10.; 4],
            ..Default::default()
        };
        frame.children.insert(
            inner,
            Child {
                fill_width: true,
                ..Default::default()
            },
        );
        editor.begin("Outer layout");
        enable(&mut editor, outer, frame, (400., 300.)).unwrap();
        editor.end();
        editor
            .execute(Command::TranslateNode {
                id: outer,
                dx: 0.5,
                dy: 0.5,
            })
            .unwrap();
        let original = editor.doc.clone();
        for _ in 0..5 {
            editor
                .execute(Command::SetDesign {
                    design: Box::new(editor.doc.design.clone()),
                })
                .unwrap();
        }
        assert_eq!(editor.doc, original);
        let original = editor.doc.clone();
        assert!(
            editor
                .execute(Command::TransformNodes {
                    ids: vec![outer],
                    transform: [0.8, 0.6, -0.6, 0.8, 0., 0.]
                })
                .is_err()
        );
        assert_eq!(editor.doc, original);
    }

    #[test]
    fn photo_frames_remain_replaceable_inside_responsive_layout() {
        let mut editor = Editor::new(Document::new(400, 300), None);
        let photo = crate::design::frame(&editor.doc, crate::design::Element::Circle)
            .paste(&mut editor, Slot::TOP, (0., 0.))
            .unwrap()[0];
        let group = editor
            .execute(Command::Group {
                ids: vec![photo],
                name: "Photo layout".into(),
            })
            .unwrap()
            .unwrap();
        editor.begin("Layout");
        enable(&mut editor, group, Frame::default(), (300., 250.)).unwrap();
        editor.end();
        let original = editor.doc.clone();
        let image = crate::design::place_in_frame(
            &mut editor,
            photo,
            Arc::new(emulsion_raster::Raster::solid(20, 10, [1.; 4])),
        )
        .unwrap();
        assert!(editor.doc.node(image).unwrap().clip_to.is_some());
        editor.undo();
        assert_eq!(editor.doc, original);
    }
    #[test]
    fn rejected_enable_does_not_add_or_resize_a_boundary() {
        let (mut editor, group, ids) = fixture();
        let original = editor.doc.clone();
        assert!(
            enable(
                &mut editor,
                group,
                Frame {
                    gap: f64::NAN,
                    ..Default::default()
                },
                (200., 200.)
            )
            .is_err()
        );
        assert_eq!(editor.doc, original);
        editor
            .execute(Command::SetLocked {
                id: ids[1],
                locked: true,
            })
            .unwrap();
        let locked = editor.doc.clone();
        assert!(enable(&mut editor, group, Frame::default(), (200., 200.)).is_err());
        assert_eq!(editor.doc, locked);
        editor
            .execute(Command::SetLocked {
                id: ids[1],
                locked: false,
            })
            .unwrap();
        editor.begin("Layout");
        enable(&mut editor, group, Frame::default(), (200., 200.)).unwrap();
        editor.end();
        let original = editor.doc.clone();
        assert!(
            enable(
                &mut editor,
                group,
                Frame {
                    columns: 0,
                    ..Default::default()
                },
                (300., 300.)
            )
            .is_err()
        );
        assert_eq!(editor.doc, original);
        let boundary = editor.doc.design.frames[&group].boundary;
        let NodeKind::Path { style, .. } = editor.doc.node(boundary).unwrap().kind else {
            panic!()
        };
        assert!(
            editor
                .execute(Command::SetPath {
                    id: boundary,
                    path: Arc::new(rectangle(0., 0., 100., 0.)),
                    style
                })
                .is_err()
        );
        assert_eq!(editor.doc, original);
    }

    #[test]
    fn filling_protected_text_rejects_reflow_without_partial_changes() {
        let (mut editor, group, _) = fixture();
        let id = editor
            .execute(Command::AddNode {
                node: Box::new(Node::text(
                    0,
                    "Protected",
                    crate::text::TextSpec {
                        text: "Wrapping text".into(),
                        width: Some(100.),
                        ..Default::default()
                    },
                    600,
                    400,
                )),
                slot: Slot::top_of(Some(group)),
            })
            .unwrap()
            .unwrap();
        editor
            .execute(Command::SetLayerLocks {
                id,
                locks: crate::node::LayerLocks {
                    transparency: true,
                    ..Default::default()
                },
            })
            .unwrap();
        let original = editor.doc.clone();
        let mut frame = Frame::default();
        frame.children.insert(
            id,
            Child {
                fill_width: true,
                ..Default::default()
            },
        );
        assert!(enable(&mut editor, group, frame, (300., 300.)).is_err());
        assert_eq!(editor.doc, original);
    }
    #[test]
    fn nested_hug_frames_keep_decorative_strokes_out_of_layout_geometry() {
        let (mut editor, inner, _) = fixture();
        editor.begin("Inner");
        enable(
            &mut editor,
            inner,
            Frame {
                padding: [10.; 4],
                gap: 10.,
                hug_height: true,
                ..Default::default()
            },
            (200., 300.),
        )
        .unwrap();
        editor.end();
        let boundary = editor.doc.design.frames[&inner].boundary;
        let NodeKind::Path {
            path, mut style, ..
        } = editor.doc.node(boundary).unwrap().kind.clone()
        else {
            panic!()
        };
        style.stroke = Some([0, 0, 0, 255]);
        style.width = 10.;
        editor
            .execute(Command::SetPath {
                id: boundary,
                path,
                style,
            })
            .unwrap();
        let outer = editor
            .execute(Command::Group {
                ids: vec![inner],
                name: "Outer".into(),
            })
            .unwrap()
            .unwrap();
        let mut frame = Frame {
            padding: [10.; 4],
            hug_height: true,
            ..Default::default()
        };
        frame.children.insert(
            inner,
            Child {
                fill_width: true,
                ..Default::default()
            },
        );
        editor.begin("Outer");
        enable(&mut editor, outer, frame, (320., 400.)).unwrap();
        editor.end();
        assert_eq!(bounds(&editor.doc, inner).unwrap().2, 300.);
        assert_eq!(bounds(&editor.doc, inner).unwrap().3, 160.);
        assert_eq!(bounds(&editor.doc, outer).unwrap().3, 180.);
        let NodeKind::Path { style, .. } = editor.doc.node(boundary).unwrap().kind else {
            panic!()
        };
        assert_eq!(style.width, 10.);
        let before = editor.doc.clone();
        let frame = editor.doc.design.frames[&inner].clone();
        editor.begin("Keep frame");
        enable(&mut editor, inner, frame, (300., 160.)).unwrap();
        editor.end();
        assert_eq!(editor.doc, before);
        for _ in 0..4 {
            editor
                .execute(Command::SetDesign {
                    design: Box::new(editor.doc.design.clone()),
                })
                .unwrap();
        }
        assert_eq!(editor.doc, before);
    }
}
