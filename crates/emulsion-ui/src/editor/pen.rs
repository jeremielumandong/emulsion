//! The Pen tool: draw and edit vector paths, Inkscape-style.
//!
//! Click to place corner anchors, drag to pull out smooth handles, click
//! the first anchor (or press Enter) to finish. The result is a Path node:
//! a vector shape that composites like pixels but stays editable. With a
//! Path node selected, the pen edits it: drag anchors and handles, Alt-click
//! an anchor to switch corner and smooth, click the outline to add an
//! anchor, Backspace to remove one. A path can also become a selection, or
//! be painted along with the current brush.

use super::tools::{ToolDrag, local_clip, premul};
use super::*;
use emulsion_raster::paint::{Ink, Stroke};
use emulsion_raster::vector::{Anchor, Hit, Path, PathStyle, Pt, SubPath};
use glam::dvec2;

/// Grab distance in screen pixels.
const GRAB_PX: f64 = 7.0;

#[derive(Clone, Copy, Debug, Default, PartialEq, Eq)]
pub enum PenMode {
    #[default]
    Pen,
    Free,
    Curvature,
    AddAnchor,
    DeleteAnchor,
    ConvertPoint,
}

impl PenMode {
    pub(crate) fn help(self) -> &'static str {
        match self {
            Self::Pen => {
                "Click to place anchors; drag to set curve handles. Enter finishes; Escape cancels."
            }
            Self::Free => "Drag to draw a freehand path. Release to finish; Escape cancels.",
            Self::Curvature => {
                "Click to place points; curves flow smoothly through them. Enter finishes; Escape cancels."
            }
            Self::AddAnchor => {
                "Select a path layer, then click its outline to add an anchor point."
            }
            Self::DeleteAnchor => "Select a path layer, then click an anchor point to remove it.",
            Self::ConvertPoint => {
                "Select a path layer, then click an anchor to switch between a corner and a smooth curve."
            }
        }
    }
}

#[derive(Default)]
pub struct PenState {
    pub mode: PenMode,
    /// The subpath being drawn, in document pixels.
    pub building: Option<SubPath>,
    /// Anchor picked on the selected Path node: (subpath, index).
    pub selected: Option<(usize, usize)>,
    pub width: f32,
    pub stroke_on: bool,
    pub fill_on: bool,
}

impl PenState {
    pub(crate) fn fresh() -> Self {
        Self {
            mode: PenMode::Pen,
            building: None,
            selected: None,
            width: 3.0,
            stroke_on: true,
            fill_on: false,
        }
    }
}

/// Pen drags.
#[derive(Clone, Debug)]
pub enum PenDrag {
    /// A continuous freehand path, committed on release.
    Free,
    /// Pulling handles out of the anchor just placed.
    New { idx: usize, start: Pt },
    /// Moving an anchor of the selected node with its handles.
    Anchor { si: usize, ai: usize, last: Pt },
    /// Moving one handle.
    Handle {
        si: usize,
        ai: usize,
        out: bool,
        alt: bool,
    },
}

/// What the overlay draws for the pen.
#[derive(Clone, Default)]
pub struct PenOverlay {
    /// Flattened curves with their closed flag.
    pub curves: Vec<(Vec<Pt>, bool)>,
    /// (position, selected, smooth).
    pub anchors: Vec<(Pt, bool, bool)>,
    /// Handle lines: anchor → handle.
    pub handles: Vec<(Pt, Pt)>,
}

fn mirror(center: Pt, p: Pt) -> Pt {
    (2.0 * center.0 - p.0, 2.0 * center.1 - p.1)
}

fn along(center: Pt, dir_from: Pt, len: f64) -> Pt {
    let (dx, dy) = (center.0 - dir_from.0, center.1 - dir_from.1);
    let d = (dx * dx + dy * dy).sqrt();
    if d < 1e-9 {
        return center;
    }
    (center.0 + dx / d * len, center.1 + dy / d * len)
}

/// Catmull–Rom tangents expressed as editable cubic Bezier handles.
fn smooth_subpath(path: &mut SubPath) {
    let positions: Vec<_> = path.anchors.iter().map(|anchor| anchor.p).collect();
    let len = positions.len();
    if len < 2 {
        return;
    }
    for (index, anchor) in path.anchors.iter_mut().enumerate() {
        let previous = if index > 0 {
            positions[index - 1]
        } else if path.closed {
            positions[len - 1]
        } else {
            positions[0]
        };
        let next = if index + 1 < len {
            positions[index + 1]
        } else if path.closed {
            positions[0]
        } else {
            positions[len - 1]
        };
        let tangent = ((next.0 - previous.0) / 6.0, (next.1 - previous.1) / 6.0);
        anchor.h_in = (anchor.p.0 - tangent.0, anchor.p.1 - tangent.1);
        anchor.h_out = (anchor.p.0 + tangent.0, anchor.p.1 + tangent.1);
        anchor.smooth = true;
    }
}

impl EditorView {
    pub(crate) fn set_pen_mode(&mut self, mode: PenMode, cx: &mut Context<Self>) {
        if self.tools.pen.mode != mode {
            self.finish_tool_interaction(cx);
        }
        self.set_tool(Tool::Pen, cx);
        self.tools.pen.mode = mode;
        self.set_status(mode.help(), false, cx);
        cx.notify();
    }

    /// The Path node the pen is editing, when one is selected.
    pub(crate) fn pen_target(&self) -> Option<(NodeId, Arc<Path>, PathStyle)> {
        let id = self.selected?;
        let n = self.editor.doc.node(id)?;
        if self.editor.doc.locked_ancestor(id).is_some() {
            return None;
        }
        match &n.kind {
            NodeKind::Path { path, style, .. } => Some((id, path.clone(), *style)),
            _ => None,
        }
    }

    fn pen_style(&self) -> PathStyle {
        let pen = &self.tools.pen;
        PathStyle {
            stroke: pen.stroke_on.then_some(self.tools.fg),
            width: pen.width,
            fill: pen.fill_on.then_some(self.tools.bg),
            ..Default::default()
        }
        .sanitized()
    }

    fn grab_tol(&self) -> f64 {
        GRAB_PX / self.view.zoom.max(0.01)
    }

    fn set_path(&mut self, id: NodeId, path: Path, style: PathStyle, cx: &mut Context<Self>) {
        self.execute(
            Command::SetPath {
                id,
                path: Arc::new(path),
                style,
            },
            cx,
        );
    }

    pub(crate) fn pen_down(&mut self, d: Pt, e: &MouseDownEvent, cx: &mut Context<Self>) {
        let tol = self.grab_tol();
        let mode = self.tools.pen.mode;
        if matches!(
            mode,
            PenMode::AddAnchor | PenMode::DeleteAnchor | PenMode::ConvertPoint
        ) {
            let Some((id, path, style)) = self.pen_target() else {
                self.set_status(
                    "Select an unlocked path layer to edit its anchor points.",
                    true,
                    cx,
                );
                return;
            };
            if mode == PenMode::AddAnchor {
                if matches!(path.hit(d, tol), Some(Hit::Anchor(..))) {
                    return;
                }
                if let Some((si, segment, t)) = path.nearest_on_curve(d, tol) {
                    let mut path = (*path).clone();
                    if let Some(ai) = path.insert_at(si, segment, t) {
                        self.editor.begin("Add anchor");
                        self.set_path(id, path, style, cx);
                        self.tools.pen.selected = Some((si, ai));
                        self.drag = Some(Drag::Tool(ToolDrag::Pen(PenDrag::Anchor {
                            si,
                            ai,
                            last: d,
                        })));
                    }
                }
            } else if let Some(Hit::Anchor(si, ai)) = path.hit(d, tol) {
                self.tools.pen.selected = Some((si, ai));
                if mode == PenMode::DeleteAnchor {
                    self.pen_delete(cx);
                } else {
                    self.pen_toggle_smooth(id, &path, style, si, ai, cx);
                }
            }
            return;
        }
        if mode == PenMode::Free {
            self.tools.pen.selected = None;
            self.tools.pen.building = Some(SubPath {
                anchors: vec![Anchor::corner(d)],
                closed: false,
            });
            self.drag = Some(Drag::Tool(ToolDrag::Pen(PenDrag::Free)));
            cx.notify();
            return;
        }
        if let Some(sp) = &mut self.tools.pen.building {
            let first = sp.anchors.first().map(|a| a.p);
            if sp.anchors.len() >= 2 && first.is_some_and(|f| (f.0 - d.0).hypot(f.1 - d.1) <= tol) {
                sp.closed = true;
                if mode == PenMode::Curvature {
                    smooth_subpath(sp);
                }
                self.pen_finish(cx);
                return;
            }
            sp.anchors.push(Anchor::corner(d));
            if mode == PenMode::Curvature {
                smooth_subpath(sp);
                cx.notify();
                return;
            }
            let idx = sp.anchors.len() - 1;
            self.drag = Some(Drag::Tool(ToolDrag::Pen(PenDrag::New { idx, start: d })));
            cx.notify();
            return;
        }
        if let Some((id, path, style)) = self.pen_target() {
            match path.hit(d, tol) {
                Some(Hit::Anchor(si, ai)) => {
                    self.tools.pen.selected = Some((si, ai));
                    if e.modifiers.alt {
                        self.pen_toggle_smooth(id, &path, style, si, ai, cx);
                        return;
                    }
                    self.editor.begin("Move anchor");
                    self.drag = Some(Drag::Tool(ToolDrag::Pen(PenDrag::Anchor {
                        si,
                        ai,
                        last: d,
                    })));
                    cx.notify();
                    return;
                }
                Some(Hit::HandleIn(si, ai)) | Some(Hit::HandleOut(si, ai)) => {
                    let out = matches!(path.hit(d, tol), Some(Hit::HandleOut(..)));
                    self.tools.pen.selected = Some((si, ai));
                    self.editor.begin("Adjust handle");
                    self.drag = Some(Drag::Tool(ToolDrag::Pen(PenDrag::Handle {
                        si,
                        ai,
                        out,
                        alt: e.modifiers.alt,
                    })));
                    cx.notify();
                    return;
                }
                None => {}
            }
            if let Some((si, seg, t)) = path.nearest_on_curve(d, tol) {
                let mut p = (*path).clone();
                if let Some(ai) = p.insert_at(si, seg, t) {
                    self.editor.begin("Add anchor");
                    self.set_path(id, p, style, cx);
                    self.tools.pen.selected = Some((si, ai));
                    self.drag = Some(Drag::Tool(ToolDrag::Pen(PenDrag::Anchor {
                        si,
                        ai,
                        last: d,
                    })));
                }
                return;
            }
        }
        // Empty space: start a new path.
        self.tools.pen.selected = None;
        self.tools.pen.building = Some(SubPath {
            anchors: vec![Anchor::corner(d)],
            closed: false,
        });
        self.drag = Some(Drag::Tool(ToolDrag::Pen(PenDrag::New { idx: 0, start: d })));
        cx.notify();
    }

    pub(crate) fn pen_move(&mut self, d: Pt, drag: PenDrag, cx: &mut Context<Self>) {
        match drag {
            PenDrag::Free => {
                if let Some(path) = &mut self.tools.pen.building
                    && path.anchors.last().is_some_and(|anchor| {
                        (anchor.p.0 - d.0).hypot(anchor.p.1 - d.1) * self.view.zoom >= 2.0
                    })
                {
                    path.anchors.push(Anchor::corner(d));
                    cx.notify();
                }
            }
            PenDrag::New { idx, start } => {
                let far = (d.0 - start.0).hypot(d.1 - start.1) * self.view.zoom > 3.0;
                if let Some(sp) = &mut self.tools.pen.building
                    && let Some(a) = sp.anchors.get_mut(idx)
                    && far
                {
                    a.h_out = d;
                    a.h_in = mirror(a.p, d);
                    a.smooth = true;
                    cx.notify();
                }
            }
            PenDrag::Anchor { si, ai, last } => {
                let Some((id, path, style)) = self.pen_target() else {
                    return;
                };
                let (dx, dy) = (d.0 - last.0, d.1 - last.1);
                let mut p = (*path).clone();
                if let Some(a) = p.subpaths.get_mut(si).and_then(|s| s.anchors.get_mut(ai)) {
                    for q in [&mut a.p, &mut a.h_in, &mut a.h_out] {
                        *q = (q.0 + dx, q.1 + dy);
                    }
                }
                self.set_path(id, p, style, cx);
                if let Some(Drag::Tool(ToolDrag::Pen(PenDrag::Anchor { last, .. }))) =
                    &mut self.drag
                {
                    *last = d;
                }
            }
            PenDrag::Handle { si, ai, out, alt } => {
                let Some((id, path, style)) = self.pen_target() else {
                    return;
                };
                let mut p = (*path).clone();
                if let Some(a) = p.subpaths.get_mut(si).and_then(|s| s.anchors.get_mut(ai)) {
                    if alt {
                        a.smooth = false;
                    }
                    let (this, other) = if out {
                        (&mut a.h_out, &mut a.h_in)
                    } else {
                        (&mut a.h_in, &mut a.h_out)
                    };
                    *this = d;
                    if a.smooth {
                        let len = (other.0 - a.p.0).hypot(other.1 - a.p.1);
                        *other = along(a.p, d, len);
                    }
                }
                self.set_path(id, p, style, cx);
            }
        }
    }

    pub(crate) fn pen_up(&mut self, drag: PenDrag, cx: &mut Context<Self>) {
        match drag {
            PenDrag::Free => {
                if let Some(path) = &mut self.tools.pen.building {
                    smooth_subpath(path);
                }
                self.pen_finish(cx);
                self.pen_cancel();
            }
            PenDrag::New { .. } => {}
            PenDrag::Anchor { .. } | PenDrag::Handle { .. } => {
                if self.editor.in_transaction() {
                    self.editor.end();
                }
            }
        }
    }

    fn pen_toggle_smooth(
        &mut self,
        id: NodeId,
        path: &Path,
        style: PathStyle,
        si: usize,
        ai: usize,
        cx: &mut Context<Self>,
    ) {
        let mut p = path.clone();
        let Some(sp) = p.subpaths.get_mut(si) else {
            return;
        };
        let n = sp.anchors.len();
        let Some(a) = sp.anchors.get(ai).copied() else {
            return;
        };
        let new = if a.smooth || a.has_handles() {
            Anchor::corner(a.p)
        } else {
            // Handles a third of the way towards the neighbours.
            let prev = if ai > 0 {
                Some(sp.anchors[ai - 1].p)
            } else if sp.closed && n > 1 {
                Some(sp.anchors[n - 1].p)
            } else {
                None
            };
            let next = if ai + 1 < n {
                Some(sp.anchors[ai + 1].p)
            } else if sp.closed && n > 1 {
                Some(sp.anchors[0].p)
            } else {
                None
            };
            let dir = match (prev, next) {
                (Some(p), Some(q)) => ((q.0 - p.0) / 6.0, (q.1 - p.1) / 6.0),
                (None, Some(q)) => ((q.0 - a.p.0) / 3.0, (q.1 - a.p.1) / 3.0),
                (Some(p), None) => ((a.p.0 - p.0) / 3.0, (a.p.1 - p.1) / 3.0),
                (None, None) => (10.0, 0.0),
            };
            Anchor {
                p: a.p,
                h_in: (a.p.0 - dir.0, a.p.1 - dir.1),
                h_out: (a.p.0 + dir.0, a.p.1 + dir.1),
                smooth: true,
            }
        };
        sp.anchors[ai] = new;
        self.set_path(id, p, style, cx);
    }

    /// Enter: finish the path being drawn as a Path node.
    pub(crate) fn pen_finish(&mut self, cx: &mut Context<Self>) {
        if self
            .tools
            .pen
            .building
            .as_ref()
            .is_none_or(|path| path.anchors.len() < 2)
        {
            return;
        }
        let Some(sp) = self.tools.pen.building.take() else {
            return;
        };
        let (w, h) = (self.editor.doc.width, self.editor.doc.height);
        let path = Path { subpaths: vec![sp] };
        if self.shape_ui.operation != super::shapes::ShapeOperation::NewLayer {
            self.apply_shape_operation(path, cx);
            return;
        }
        let style = self.pen_style();
        let node = Node::path(0, "Path", Arc::new(path), style, w, h);
        let slot = self.insertion_slot();
        if let Some(id) = self.execute(
            Command::AddNode {
                node: Box::new(node),
                slot,
            },
            cx,
        ) {
            self.set_layer_selection(vec![id], Some(id));
            self.tools.pen.selected = None;
            self.set_status(
                "Path added. Drag anchors to edit; alt-click one to make it a corner or a curve.",
                false,
                cx,
            );
        }
    }

    /// Escape: drop the path being drawn, or the anchor selection.
    pub(crate) fn pen_cancel(&mut self) -> bool {
        let building = self.tools.pen.building.take().is_some();
        let selected = self.tools.pen.selected.take().is_some();
        building || selected
    }

    /// Backspace: remove the last placed or the selected anchor. Returns
    /// whether the pen used the key.
    pub(crate) fn pen_delete(&mut self, cx: &mut Context<Self>) -> bool {
        if let Some(sp) = &mut self.tools.pen.building {
            sp.anchors.pop();
            if sp.anchors.is_empty() {
                self.tools.pen.building = None;
            }
            cx.notify();
            return true;
        }
        if let (Some((si, ai)), Some((id, path, style))) =
            (self.tools.pen.selected, self.pen_target())
        {
            let mut p = (*path).clone();
            p.remove_anchor(si, ai);
            self.tools.pen.selected = None;
            if p.is_empty() {
                self.execute(Command::RemoveNode { id }, cx);
            } else {
                self.set_path(id, p, style, cx);
            }
            return true;
        }
        false
    }

    /// Apply the pen's stroke/fill/width options to the selected path.
    pub(crate) fn pen_restyle(&mut self, cx: &mut Context<Self>) {
        if let Some((id, path, _)) = self.pen_target() {
            let style = self.pen_style();
            self.set_path(id, (*path).clone(), style, cx);
        }
    }

    /// Turn the selected path (or the one being drawn) into the selection.
    pub(crate) fn pen_to_selection(&mut self, cx: &mut Context<Self>) {
        let (w, h) = (self.editor.doc.width, self.editor.doc.height);
        let path = match (&self.tools.pen.building, self.pen_target()) {
            (Some(sp), _) if sp.anchors.len() >= 3 => Path {
                subpaths: vec![SubPath {
                    anchors: sp.anchors.clone(),
                    closed: true,
                }],
            },
            (_, Some((_, p, _))) => (*p).clone(),
            _ => {
                self.set_status("Draw or select a path first.", false, cx);
                return;
            }
        };
        let m = path.fill_mask(w, h);
        let combine = self.tools.combine;
        self.apply_selection(m, combine, cx);
    }

    /// Paint along the selected path with the current brush and colour.
    pub(crate) fn pen_paint_along(&mut self, cx: &mut Context<Self>) {
        let path = match (&self.tools.pen.building, self.pen_target()) {
            (Some(sp), _) if sp.anchors.len() >= 2 => Path {
                subpaths: vec![sp.clone()],
            },
            (_, Some((_, p, _))) => (*p).clone(),
            _ => {
                self.set_status("Draw or select a path first.", false, cx);
                return;
            }
        };
        let Some(id) = self.paint_target(cx) else {
            return;
        };
        let Some((raster, to_doc)) = self.target_raster(id) else {
            return;
        };
        let to_local = to_doc.inverse();
        let scale = to_doc.matrix2.determinant().abs().sqrt().max(1e-6);
        let mut brush = self.tools.brush;
        brush.size = (brush.size as f64 / scale) as f32;
        brush.stabilizer = 0.0;
        let clip = self
            .editor
            .doc
            .selection
            .clone()
            .map(|m| local_clip(m, to_doc));
        let ink = if self.tools.paint == PaintKind::Eraser {
            Ink::Erase
        } else {
            Ink::Color(premul(self.tools.fg))
        };
        let mut any = false;
        let mut current = (*raster).clone();
        for (pts, closed) in path.flatten(0.5) {
            let mut s = Stroke::new(Arc::new(current.clone()), brush, ink.clone(), clip.clone());
            let mut all = pts.clone();
            if closed && let Some(f) = pts.first() {
                all.push(*f);
            }
            for p in &all {
                let l = to_local.transform_point2(dvec2(p.0, p.1));
                s.point_at(l.x as f32, l.y as f32, None, None);
            }
            s.finish();
            let (r, d) = s.render(&current);
            if !d.is_empty() {
                any = true;
                current = r;
            }
        }
        if any {
            let dirty = current.bounds();
            self.execute(
                Command::ReplacePixels {
                    id,
                    raster: Arc::new(current),
                    dirty,
                    label: "Paint along path".into(),
                },
                cx,
            );
        }
    }

    /// What the overlay shows for the pen: the path in progress or the
    /// selected node's path with its anchors and handles.
    pub(crate) fn pen_overlay(&self) -> Option<PenOverlay> {
        if self.tool != Tool::Pen
            && !(self.tool == Tool::Shape && self.shape_ui.component.is_some())
        {
            return None;
        }
        let mut o = PenOverlay::default();
        if let Some(sp) = &self.tools.pen.building {
            let mut pts = sp.flatten(0.5);
            if let Some(p) = self.tools.pointer.and_then(|p| self.doc_point(p))
                && self.drag.is_none()
                && let Some(last) = sp.anchors.last()
            {
                // Rubber band from the last anchor to the pointer.
                pts.extend(preview_segment(last, p));
            }
            o.curves.push((pts, false));
            let n = sp.anchors.len();
            for (i, a) in sp.anchors.iter().enumerate() {
                o.anchors.push((a.p, i + 1 == n, a.smooth));
                if a.has_handles() {
                    o.handles.push((a.p, a.h_in));
                    o.handles.push((a.p, a.h_out));
                }
            }
            return Some(o);
        }
        let (_, path, _) = self.pen_target()?;
        o.curves = path.flatten(0.5);
        for (si, sp) in path.subpaths.iter().enumerate() {
            for (ai, a) in sp.anchors.iter().enumerate() {
                let sel = self.tools.pen.selected == Some((si, ai));
                o.anchors.push((a.p, sel, a.smooth));
                if sel
                    || a.has_handles()
                        && self
                            .tools
                            .pen
                            .selected
                            .is_some_and(|(s, i)| s == si && (i as isize - ai as isize).abs() <= 1)
                {
                    o.handles.push((a.p, a.h_in));
                    o.handles.push((a.p, a.h_out));
                }
            }
        }
        Some(o)
    }
}

/// The curve the next click would make: from `last` towards `p`, with
/// `last`'s outgoing handle and a straight arrival.
fn preview_segment(last: &Anchor, p: Pt) -> Vec<Pt> {
    (1..=16)
        .map(|k| emulsion_raster::vector::cubic_at(last.p, last.h_out, p, p, k as f64 / 16.0))
        .collect()
}
