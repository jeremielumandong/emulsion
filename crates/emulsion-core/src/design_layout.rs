//! Persistent responsive group layouts. Reflow is part of the originating edit.
use crate::{Document, NodeId, NodeKind};
use serde::{Deserialize, Serialize};
use std::collections::BTreeMap;

#[path = "design_layout_breakpoints.rs"]
mod breakpoints;
#[path = "design_layout_incremental.rs"]
mod incremental;
pub use breakpoints::{
    Breakpoint, BreakpointReference, FrameLimits, FrameOverrides, active_breakpoint,
    effective_frame, reference_width,
};
pub(crate) use incremental::reflow_after;

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
#[derive(Clone, Copy, Debug, Default, PartialEq, Serialize, Deserialize)]
#[serde(default, deny_unknown_fields)]
pub struct Child {
    pub absolute: bool,
    pub fill_width: bool,
    pub fill_height: bool,
    pub min_width: Option<f64>,
    pub max_width: Option<f64>,
    pub min_height: Option<f64>,
    pub max_height: Option<f64>,
    /// Explicit width / height; never rasterizes native source pixels or fonts.
    pub aspect_ratio: Option<f64>,
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
    pub hug_width: bool,
    pub min_width: Option<f64>,
    pub max_width: Option<f64>,
    pub min_height: Option<f64>,
    pub max_height: Option<f64>,
    pub children: BTreeMap<NodeId, Child>,
    /// Clip descendants to this native rectangular boundary.
    pub clip_content: bool,
    /// Canvas-width thresholds; highest matching entry overrides the base.
    pub breakpoints: Vec<Breakpoint>,
    pub breakpoint_reference: BreakpointReference,
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
            hug_width: false,
            min_width: None,
            max_width: None,
            min_height: None,
            max_height: None,
            children: BTreeMap::new(),
            clip_content: false,
            breakpoints: Vec::new(),
            breakpoint_reference: BreakpointReference::Canvas,
        }
    }
}
pub fn validate(frames: &BTreeMap<NodeId, Frame>, doc: &Document) -> Result<(), String> {
    if frames.len() > 256 {
        return Err("A page supports up to 256 responsive layouts.".into());
    }
    for frame in frames.values() {
        breakpoints::validate(&frame.breakpoints)?;
    }
    for (id, base) in frames {
        if base.breakpoint_reference == BreakpointReference::Container
            && !base.breakpoints.is_empty()
        {
            let mut parent = doc.node(*id).and_then(|n| n.parent);
            while let Some(id) = parent {
                if let Some(frame) = frames.get(&id)
                    && (frame.hug_width
                        || frame
                            .breakpoints
                            .iter()
                            .any(|b| b.overrides.hug_width == Some(true)))
                {
                    return Err("Container breakpoints cannot query inside content-sized width ancestors. Use fixed/fill width or canvas breakpoints.".into());
                }
                parent = doc.node(id).and_then(|n| n.parent);
            }
        }
        // Validate every reachable parent/child combination, including inactive
        // breakpoints. Only immediate child thresholds can affect this frame's
        // sizing constraints, keeping the work bounded for nested documents.
        let mut widths = vec![0.];
        widths.extend(base.breakpoints.iter().map(|b| b.min_width));
        for child in doc.children(Some(*id)) {
            if let Some(nested) = frames.get(&child) {
                widths.extend(nested.breakpoints.iter().map(|b| b.min_width));
            }
        }
        widths.sort_by(f64::total_cmp);
        widths.dedup();
        for canvas_width in widths {
            let resolved = breakpoints::resolve(base, canvas_width);
            let frame = &resolved;
            if !doc.node(*id).is_some_and(|n| n.is_group())
                || !doc.node(frame.boundary).is_some_and(|n| {
                    n.parent == Some(*id) && matches!(n.kind, NodeKind::Path { .. })
                })
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
            limits(
                frame.min_width,
                frame.max_width,
                frame.min_height,
                frame.max_height,
            )?;
            for (child, sizing) in &frame.children {
                let own = limits(
                    sizing.min_width,
                    sizing.max_width,
                    sizing.min_height,
                    sizing.max_height,
                )?;
                if let Some(ratio) = sizing.aspect_ratio {
                    if !ratio.is_finite() || !(0.001..=1000.).contains(&ratio) {
                        return Err(
                            "Child aspect ratio must be 0.001–1000 (width / height).".into()
                        );
                    }
                    ratio_limits(own, ratio)?;
                }
                if !sizing.absolute {
                    if (frame.hug_width && sizing.fill_width)
                        || (frame.hug_height && sizing.fill_height)
                    {
                        return Err("A content-sized frame cannot have children filling the same axis. Turn off content sizing or child fill.".into());
                    }
                    if let Some(base_nested) = frames.get(child) {
                        let independent = base.breakpoint_reference
                            == BreakpointReference::Container
                            || base_nested.breakpoint_reference == BreakpointReference::Container;
                        let queries = if independent {
                            std::iter::once(0.)
                                .chain(base_nested.breakpoints.iter().map(|b| b.min_width))
                                .collect()
                        } else {
                            vec![canvas_width]
                        };
                        for query_width in queries {
                            let nested = breakpoints::resolve(base_nested, query_width);
                            if (nested.hug_width && sizing.fill_width)
                                || (nested.hug_height && sizing.fill_height)
                            {
                                return Err(
                            "A nested frame cannot hug and receive parent fill on the same axis."
                                .into(),
                        );
                            }
                            if sizing.aspect_ratio.is_some()
                                && (nested.hug_width || nested.hug_height)
                            {
                                return Err("Turn off nested frame content sizing before assigning an aspect ratio.".into());
                            }
                            let combined = intersect_limits(
                                own,
                                limits(
                                    nested.min_width,
                                    nested.max_width,
                                    nested.min_height,
                                    nested.max_height,
                                )?,
                            )?;
                            if let Some(ratio) = sizing.aspect_ratio {
                                ratio_limits(combined, ratio)?;
                            }
                        }
                    }
                }
            }
            for child in frame.children.keys() {
                if *child == frame.boundary
                    || !doc.node(*child).is_some_and(|n| n.parent == Some(*id))
                {
                    return Err("Layout sizing refers to an object outside the frame.".into());
                }
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
        for entry in &mut frame.breakpoints {
            entry
                .overrides
                .children
                .retain(|child, _| parents.get(child) == Some(&Some(*id)));
        }
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
            } else if let Some(b) = crate::geometry::affine_capability_bounds(doc, target)
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
    crate::geometry::affine_capability_bounds(doc, id)
}

pub(crate) fn reflow(doc: &mut Document) -> Result<(), String> {
    if !doc.design.frames.is_empty() && doc.nodes.iter().any(crate::Node::has_projective_metadata) {
        return Err(
            crate::GeometryError::retained_projective("responsive layout reflow").to_string(),
        );
    }
    validate(&doc.design.frames, doc)?;
    for id in doc.children(None) {
        reflow_subtree(doc, id)?;
    }
    Ok(())
}

fn reflow_subtree(doc: &mut Document, id: NodeId) -> Result<(), String> {
    if let Some(frame) = effective_frame(doc, id) {
        reflow_frame(doc, id, &frame)
    } else {
        for child in doc.children(Some(id)) {
            reflow_subtree(doc, child)?;
        }
        Ok(())
    }
}

fn reflow_frame(doc: &mut Document, id: NodeId, frame: &Frame) -> Result<(), String> {
    if advanced_subtree(doc, id) {
        reflow_advanced_frame(doc, id, frame)
    } else {
        reflow_legacy_frame(doc, id, frame)
    }
}

fn reflow_legacy_frame(doc: &mut Document, id: NodeId, frame: &Frame) -> Result<(), String> {
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

// Advanced sizing is opt-in, so documents using only the original controls keep
// their existing integer content-bound layout exactly.
type SizeLimits = [f64; 4]; // min width, max width, min height, max height
fn limits(
    min_w: Option<f64>,
    max_w: Option<f64>,
    min_h: Option<f64>,
    max_h: Option<f64>,
) -> Result<SizeLimits, String> {
    if [min_w, max_w, min_h, max_h]
        .into_iter()
        .flatten()
        .any(|v| !v.is_finite() || !(1. ..=100000.).contains(&v))
    {
        return Err("Layout size limits must be finite values from 1–100000 px.".into());
    }
    let result = [
        min_w.unwrap_or(1.),
        max_w.unwrap_or(100000.),
        min_h.unwrap_or(1.),
        max_h.unwrap_or(100000.),
    ];
    if result[0] > result[1] || result[2] > result[3] {
        return Err("Layout minimum sizes cannot exceed maximum sizes.".into());
    }
    Ok(result)
}
fn intersect_limits(a: SizeLimits, b: SizeLimits) -> Result<SizeLimits, String> {
    let result = [
        a[0].max(b[0]),
        a[1].min(b[1]),
        a[2].max(b[2]),
        a[3].min(b[3]),
    ];
    if result[0] > result[1] || result[2] > result[3] {
        return Err("Parent child limits conflict with the nested frame's size limits.".into());
    }
    Ok(result)
}
fn ratio_limits(l: SizeLimits, r: f64) -> Result<(f64, f64), String> {
    let lo = l[0].max(l[2] * r);
    let hi = l[1].min(l[3] * r);
    if lo > hi + 1e-9 {
        return Err("Aspect ratio cannot satisfy the requested minimum and maximum sizes.".into());
    }
    Ok((lo, hi.max(lo)))
}
fn child_limits(doc: &Document, id: NodeId, s: Child) -> Result<SizeLimits, String> {
    let own = limits(s.min_width, s.max_width, s.min_height, s.max_height)?;
    if let Some(f) = effective_frame(doc, id) {
        intersect_limits(
            own,
            limits(f.min_width, f.max_width, f.min_height, f.max_height)?,
        )
    } else {
        Ok(own)
    }
}
fn advanced_frame(f: &Frame) -> bool {
    f.hug_width
        || [f.min_width, f.max_width, f.min_height, f.max_height]
            .iter()
            .any(Option::is_some)
        || f.children.values().any(|s| {
            s.fill_height
                || [
                    s.min_width,
                    s.max_width,
                    s.min_height,
                    s.max_height,
                    s.aspect_ratio,
                ]
                .iter()
                .any(Option::is_some)
        })
}
fn advanced_subtree(doc: &Document, id: NodeId) -> bool {
    doc.design.frames.keys().any(|child| {
        (*child == id || doc.is_ancestor(id, *child))
            && effective_frame(doc, *child).is_some_and(|f| advanced_frame(&f))
    })
}
#[derive(Clone, Copy)]
struct BoxRect {
    x: f64,
    y: f64,
    w: f64,
    h: f64,
}
fn measure(doc: &Document, id: NodeId) -> Option<BoxRect> {
    if let Some((x, y, w, h)) = bounds(doc, id) {
        return Some(BoxRect { x, y, w, h });
    }
    let node = doc.node(id)?;
    match &node.kind {
        NodeKind::Path { path, .. } => {
            let (x, y, w, h) = emulsion_raster::vector_geometry::bounds(path)?;
            Some(BoxRect {
                x,
                y,
                w: w.max(1.),
                h: h.max(1.),
            })
        }
        NodeKind::Text { spec, .. }
            if spec.rotation == 0. && spec.scale_x == 1. && spec.scale_y == 1. =>
        {
            let b = crate::text::layout(spec).bounds();
            Some(BoxRect {
                x: f64::from(spec.x),
                y: f64::from(spec.y),
                w: f64::from(spec.width.unwrap_or(b.width)).max(1.),
                h: f64::from(spec.height.unwrap_or(b.height)).max(1.),
            })
        }
        _ => {
            let b = crate::geometry::affine_capability_bounds(doc, id)?;
            Some(BoxRect {
                x: f64::from(b.x),
                y: f64::from(b.y),
                w: f64::from(b.w),
                h: f64::from(b.h),
            })
        }
    }
}
/// Semantic layout dimensions, excluding decorative frame/path strokes and
/// using a text object's paragraph box rather than just its visible glyph ink.
pub fn item_dimensions(doc: &Document, id: NodeId) -> Option<(f64, f64)> {
    measure(doc, id).map(|b| (b.w, b.h))
}
fn independent_move(doc: &Document, id: NodeId) -> Result<(), String> {
    let roots = crate::layer_links::movement_roots(doc, &[id]).map_err(|e| e.to_string())?;
    if roots.len() != 1 || roots[0] != id {
        return Err(
            "Unlink separately positioned objects before automatic layout moves or resizes them."
                .into(),
        );
    }
    Ok(())
}
fn size_item(
    doc: &mut Document,
    id: NodeId,
    width: Option<f64>,
    height: Option<f64>,
) -> Result<(), String> {
    let Some(b) = measure(doc, id) else {
        return Ok(());
    };
    if width.is_none() && height.is_none() {
        return Ok(());
    }
    let (w, h) = (width.unwrap_or(b.w).max(1.), height.unwrap_or(b.h).max(1.));
    let node = doc.node(id).ok_or("Missing layout child")?;
    if let NodeKind::Text { spec, .. } = &node.kind {
        if spec.rotation != 0. || spec.scale_x != 1. || spec.scale_y != 1. {
            return Err(
                "Reset text rotation/scale before assigning responsive paragraph dimensions."
                    .into(),
            );
        }
        let mut next = (**spec).clone();
        if let Some(w) = width {
            next.width = Some(w as f32);
        }
        if let Some(h) = height {
            next.height = Some(h as f32);
        }
        if next != **spec {
            let locks = doc.layer_locks(id);
            if doc.locked_ancestor(id).is_some()
                || locks.position
                || locks.pixels
                || locks.transparency
            {
                return Err("Unlock the text before changing layout dimensions.".into());
            }
            independent_move(doc, id)?;
            let kind = crate::Node::text(id, &node.name, next, doc.width, doc.height).kind;
            doc.node_mut(id).unwrap().kind = kind;
        }
        return Ok(());
    }
    if let NodeKind::Path { path, .. } = &node.kind
        && let Some((_, _, raw_w, raw_h)) = emulsion_raster::vector_geometry::bounds(path)
        && ((raw_w <= 1e-9 && (b.w - w).abs() > 0.01) || (raw_h <= 1e-9 && (b.h - h).abs() > 0.01))
    {
        return Err("A zero-width or zero-height path cannot fill that axis. Use a rectangle for a resizable rule.".into());
    }
    if (b.w - w).abs() <= 0.01 && (b.h - h).abs() <= 0.01 {
        return Ok(());
    }
    independent_move(doc, id)?;
    if doc.design.frames.contains_key(&id) {
        resize_frame(doc, id, w, h)
    } else {
        crate::transform::transform_nodes(
            doc,
            &[id],
            [
                w / b.w.max(1e-9),
                0.,
                0.,
                h / b.h.max(1e-9),
                b.x * (1. - w / b.w.max(1e-9)),
                b.y * (1. - h / b.h.max(1e-9)),
            ],
        )
        .map(|_| ())
        .map_err(|e| e.to_string())
    }
}
fn move_item(doc: &mut Document, id: NodeId, x: f64, y: f64) -> Result<(), String> {
    let Some(b) = measure(doc, id) else {
        return Ok(());
    };
    let (dx, dy) = (x.round() - b.x, y.round() - b.y);
    if dx.abs() > 0.01 || dy.abs() > 0.01 {
        independent_move(doc, id)?;
        crate::transform::transform_nodes(doc, &[id], [1., 0., 0., 1., dx, dy])
            .map_err(|e| e.to_string())?;
    }
    Ok(())
}
fn nested_reflow(doc: &mut Document, id: NodeId, s: Child) -> Result<(), String> {
    if let Some(mut f) = effective_frame(doc, id) {
        let l = child_limits(doc, id, s)?;
        // Resolve parent sizing limits without persisting them in the child's
        // own frame definition. Removing the parent rule restores its settings.
        if s.min_width.is_some() {
            f.min_width = Some(l[0]);
        }
        if s.max_width.is_some() {
            f.max_width = Some(l[1]);
        }
        if s.min_height.is_some() {
            f.min_height = Some(l[2]);
        }
        if s.max_height.is_some() {
            f.max_height = Some(l[3]);
        }
        reflow_advanced_frame(doc, id, &f)
    } else {
        reflow_subtree(doc, id)
    }
}
fn resolved_size(
    base: (f64, f64),
    s: Child,
    l: SizeLimits,
    w: Option<f64>,
    h: Option<f64>,
) -> Result<(Option<f64>, Option<f64>), String> {
    let (mut width, mut height) = (
        w.unwrap_or(base.0).clamp(l[0], l[1]),
        h.unwrap_or(base.1).clamp(l[2], l[3]),
    );
    if let Some(r) = s.aspect_ratio {
        let (lo, hi) = ratio_limits(l, r)?;
        width = match (w, h) {
            (Some(w), Some(h)) => w.min(h * r),
            (None, Some(h)) => h * r,
            _ => width,
        }
        .clamp(lo, hi);
        height = width / r;
        return Ok((Some(width), Some(height)));
    }
    Ok((
        (w.is_some() || (width - base.0).abs() > 0.01).then_some(width),
        (h.is_some() || (height - base.1).abs() > 0.01).then_some(height),
    ))
}
/// Bounded water-fill: clamp siblings at their minimum/maximum and redistribute
/// the remaining space. Minimum sizes may overflow instead of silently shrinking.
fn shares(total: f64, limits: &[(f64, f64)]) -> Vec<f64> {
    let minimum: f64 = limits.iter().map(|l| l.0).sum();
    let maximum: f64 = limits.iter().map(|l| l.1).sum();
    if total <= minimum {
        return limits.iter().map(|l| l.0).collect();
    }
    if total >= maximum {
        return limits.iter().map(|l| l.1).collect();
    }
    // The common flexible size is monotonic. Bisection gives equal shares,
    // clamps saturated siblings, and redistributes all spare space in at most
    // 64 passes, independent of the number of children hitting a limit.
    let (mut lo, mut hi) = (0., limits.iter().map(|l| l.1).fold(0., f64::max));
    for _ in 0..64 {
        let mid = (lo + hi) / 2.;
        let sum: f64 = limits.iter().map(|l| mid.clamp(l.0, l.1)).sum();
        if sum < total { lo = mid } else { hi = mid }
    }
    let level = (lo + hi) / 2.;
    limits.iter().map(|l| level.clamp(l.0, l.1)).collect()
}
fn hug_width(children: &[(NodeId, Child, SizeLimits)], doc: &Document, frame: &Frame) -> f64 {
    let widths: Vec<_> = children
        .iter()
        .map(|(id, _, _)| measure(doc, *id).map_or(0., |b| b.w))
        .collect();
    let gaps = frame.gap * widths.len().saturating_sub(1) as f64;
    let content = match frame.flow {
        Flow::Row => widths.iter().sum::<f64>() + gaps,
        Flow::Column => widths.iter().copied().fold(0., f64::max),
        Flow::Grid => {
            let cols = (frame.columns as usize).min(widths.len()).max(1);
            let cell = widths.iter().copied().fold(0., f64::max);
            cell * cols as f64 + frame.gap * cols.saturating_sub(1) as f64
        }
    };
    content + frame.padding[1] + frame.padding[3]
}
#[cfg(test)]
thread_local! {static ADVANCED_VISITS:std::cell::Cell<usize>=const {std::cell::Cell::new(0)};}

fn reflow_advanced_frame(doc: &mut Document, id: NodeId, frame: &Frame) -> Result<(), String> {
    #[cfg(test)]
    ADVANCED_VISITS.with(|visits| visits.set(visits.get() + 1));
    let Some((x, y, initial_w, initial_h)) = bounds(doc, id) else {
        return Ok(());
    };
    let frame_limits = limits(
        frame.min_width,
        frame.max_width,
        frame.min_height,
        frame.max_height,
    )?;
    let mut w = initial_w.clamp(frame_limits[0], frame_limits[1]);
    let h = initial_h.clamp(frame_limits[2], frame_limits[3]);
    if (w - initial_w).abs() > 0.01 || (h - initial_h).abs() > 0.01 {
        independent_move(doc, frame.boundary)?;
        resize_frame(doc, id, w, h)?;
    }
    let children: Vec<_> = doc
        .children(Some(id))
        .into_iter()
        .filter(|child| {
            *child != frame.boundary && !frame.children.get(child).is_some_and(|s| s.absolute)
        })
        .map(|child| {
            let sizing = frame.children.get(&child).copied().unwrap_or_default();
            Ok((child, sizing, child_limits(doc, child, sizing)?))
        })
        .collect::<Result<_, String>>()?;
    // Natural children are measured bottom-up once. Filled children defer
    // recursion until their parent has assigned both dimensions; this avoids
    // exponential visits through deep chains of responsive frames.
    let mut visited = std::collections::HashSet::new();
    for (child, s, l) in &children {
        if let Some(b) = measure(doc, *child) {
            let natural = Child {
                aspect_ratio: if s.fill_height || s.fill_width {
                    None
                } else {
                    s.aspect_ratio
                },
                ..*s
            };
            let (width, height) = resolved_size((b.w, b.h), natural, *l, None, None)?;
            size_item(doc, *child, width, height)?;
        }
        if !s.fill_width && !s.fill_height {
            nested_reflow(doc, *child, *s)?;
            visited.insert(*child);
        }
    }
    if frame.hug_width {
        w = hug_width(&children, doc, frame)
            .ceil()
            .clamp(frame_limits[0], frame_limits[1]);
    }
    let available_w = (w - frame.padding[1] - frame.padding[3]).max(1.);
    let available_h = (h - frame.padding[0] - frame.padding[2]).max(1.);
    let cols = if frame.hug_width {
        (frame.columns as usize).min(children.len()).max(1)
    } else {
        frame.columns as usize
    };
    let rows = children.len().div_ceil(cols).max(1);
    let cell_w = ((available_w - frame.gap * (cols - 1) as f64) / cols as f64).max(1.);
    let cell_h = ((available_h - frame.gap * (rows - 1) as f64) / rows as f64).max(1.);
    // Cross-axis sizing can change intrinsic width (including a nested frame's
    // content width). Resolve it before reserving room for flexible siblings.
    if frame.flow == Flow::Row {
        for (child, s, l) in &children {
            if s.fill_height
                && !s.fill_width
                && let Some(b) = measure(doc, *child)
            {
                let (width, height) = resolved_size((b.w, b.h), *s, *l, None, Some(available_h))?;
                size_item(doc, *child, width, height)?;
                nested_reflow(doc, *child, *s)?;
                visited.insert(*child);
            }
        }
    }
    let row_fixed = children
        .iter()
        .filter(|(_, s, _)| !s.fill_width)
        .filter_map(|(id, _, _)| measure(doc, *id))
        .map(|b| b.w)
        .sum::<f64>();
    let width_limits: Vec<_> = children
        .iter()
        .filter(|(_, s, _)| s.fill_width)
        .map(|(_, s, l)| {
            if let Some(r) = s.aspect_ratio {
                ratio_limits(*l, r).map(|(lo, hi)| {
                    (
                        lo,
                        if s.fill_height {
                            hi.min(available_h * r).max(lo)
                        } else {
                            hi
                        },
                    )
                })
            } else {
                Ok((l[0], l[1]))
            }
        })
        .collect::<Result<_, _>>()?;
    let row_shares = shares(
        available_w - row_fixed - frame.gap * children.len().saturating_sub(1) as f64,
        &width_limits,
    );
    let mut wi = 0;
    for (child, s, l) in &children {
        let Some(b) = measure(doc, *child) else {
            continue;
        };
        let width = if s.fill_width {
            Some(match frame.flow {
                Flow::Row => {
                    let v = row_shares[wi];
                    wi += 1;
                    v
                }
                Flow::Column => available_w,
                Flow::Grid => cell_w,
            })
        } else {
            None
        };
        let height =
            (s.fill_height && frame.flow != Flow::Column).then_some(if frame.flow == Flow::Grid {
                cell_h
            } else {
                available_h
            });
        let defer_height = s.fill_height && frame.flow == Flow::Column;
        let sizing = if defer_height && !s.fill_width {
            Child {
                aspect_ratio: None,
                ..*s
            }
        } else {
            *s
        };
        let (width, height) = resolved_size((b.w, b.h), sizing, *l, width, height)?;
        size_item(doc, *child, width, height)?;
        // A width change may produce new text wrapping; enforce height limits
        // now, in the same command, rather than waiting for the next reflow.
        if let Some(next) = measure(doc, *child) {
            let (width, height) = resolved_size((next.w, next.h), sizing, *l, None, None)?;
            size_item(doc, *child, width, height)?;
        }
        let changed = measure(doc, *child)
            .is_some_and(|next| (next.w - b.w).abs() > 0.01 || (next.h - b.h).abs() > 0.01);
        if !defer_height && (!visited.contains(child) || changed) {
            nested_reflow(doc, *child, *s)?;
            visited.insert(*child);
        }
    }
    if frame.flow == Flow::Column {
        let fixed = children
            .iter()
            .filter(|(_, s, _)| !s.fill_height)
            .filter_map(|(id, _, _)| measure(doc, *id))
            .map(|b| b.h)
            .sum::<f64>();
        let height_limits: Vec<_> = children
            .iter()
            .filter(|(_, s, _)| s.fill_height)
            .map(|(_, s, l)| {
                if let Some(r) = s.aspect_ratio {
                    ratio_limits(*l, r).map(|(lo, hi)| {
                        (
                            lo / r,
                            if s.fill_width {
                                hi.min(available_w).max(lo) / r
                            } else {
                                hi / r
                            },
                        )
                    })
                } else {
                    Ok((l[2], l[3]))
                }
            })
            .collect::<Result<_, _>>()?;
        let allocation = shares(
            available_h - fixed - frame.gap * children.len().saturating_sub(1) as f64,
            &height_limits,
        );
        let mut hi = 0;
        for (child, s, l) in &children {
            if !s.fill_height {
                continue;
            }
            let Some(b) = measure(doc, *child) else {
                continue;
            };
            let height = allocation[hi];
            hi += 1;
            let width = s.fill_width.then_some(available_w);
            let (width, height) = resolved_size((b.w, b.h), *s, *l, width, Some(height))?;
            size_item(doc, *child, width, height)?;
            nested_reflow(doc, *child, *s)?;
        }
    }
    // Height-driven aspect ratios can change intrinsic width; no child fills a
    // hugged width, so this final width cannot feed back into its own allocation.
    if frame.hug_width {
        w = hug_width(&children, doc, frame)
            .ceil()
            .clamp(frame_limits[0], frame_limits[1]);
    }
    let available = (w - frame.padding[1] - frame.padding[3]).max(1.);
    let cell = ((available - frame.gap * (cols - 1) as f64) / cols as f64).max(1.);
    let (left, top) = (x + frame.padding[3], y + frame.padding[0]);
    let (mut cx, mut cy, mut row_height) = (left, top, 0_f64);
    let mut bottom = top;
    let grid_fills_height = children.iter().any(|(_, s, _)| s.fill_height);
    for (index, (child, _, _)) in children.iter().enumerate() {
        let Some(b) = measure(doc, *child) else {
            continue;
        };
        let offset = |space: f64| match frame.align {
            Align::Start => 0.,
            Align::Center => ((space - b.w) / 2.).max(0.),
            Align::End => (space - b.w).max(0.),
        };
        match frame.flow {
            Flow::Column => {
                move_item(doc, *child, left + offset(available), cy)?;
                cy += b.h + frame.gap;
            }
            Flow::Row => {
                if frame.wrap && cx > left && cx + b.w > left + available + 0.01 {
                    cx = left;
                    cy += row_height + frame.gap;
                    row_height = 0.;
                }
                move_item(doc, *child, cx, cy)?;
                cx += b.w + frame.gap;
                row_height = row_height.max(b.h);
            }
            Flow::Grid => {
                let col = index % cols;
                if col == 0 && index > 0 {
                    cy += row_height + frame.gap;
                    row_height = 0.;
                }
                move_item(
                    doc,
                    *child,
                    left + col as f64 * (cell + frame.gap) + offset(cell),
                    cy,
                )?;
                row_height = row_height.max(if grid_fills_height {
                    cell_h.max(b.h)
                } else {
                    b.h
                });
            }
        }
        if let Some(b) = measure(doc, *child) {
            bottom = bottom.max(b.y + b.h);
        }
    }
    for child in doc.children(Some(id)) {
        if child != frame.boundary && frame.children.get(&child).is_some_and(|s| s.absolute) {
            reflow_subtree(doc, child)?;
        }
    }
    let height = if frame.hug_height {
        (bottom - y + frame.padding[2])
            .ceil()
            .clamp(frame_limits[2], frame_limits[3])
    } else {
        h
    };
    independent_move(doc, frame.boundary)?;
    resize_frame(doc, id, w, height)?;
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
            let b =
                crate::geometry::affine_capability_bounds(&editor.doc, group).unwrap_or_default();
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
        crate::geometry::affine_capability_bounds(&editor.doc, id).unwrap()
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

#[cfg(test)]
#[path = "design_layout_advanced_tests.rs"]
mod advanced_tests;
