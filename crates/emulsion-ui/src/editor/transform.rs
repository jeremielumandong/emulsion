//! Free Transform and Distort for the Move tool.
//!
//! The selected pixel node shows its box with eight handles. Corners scale
//! proportionally (Shift frees the aspect), edges scale one axis, and
//! dragging just outside a corner rotates (Shift snaps to 15°). The
//! opposite handle stays put. Ctrl-dragging a corner distorts: the corner
//! moves on its own and the pixels are re-projected into a new, larger
//! buffer when you let go — the one transform that resamples, so it is a
//! single undo step. X, Y, W, H and angle can also be typed in.

use super::*;
use emulsion_raster::warp;
use glam::dvec2;
use gpui_kit::component::Sizable as _;

/// Handle size and grab distances, in screen pixels.
const HANDLE_PX: f32 = 7.0;
const GRAB_PX: f64 = 7.0;
const ROTATE_PX: f64 = 26.0;

#[derive(Clone, Copy, Debug, PartialEq)]
pub(crate) enum Handle {
    /// 0 top-left, 1 top-right, 2 bottom-right, 3 bottom-left.
    Corner(usize),
    /// 0 top, 1 right, 2 bottom, 3 left.
    Edge(usize),
    Rotate,
}

/// A handle being dragged.
#[derive(Clone, Copy, Debug)]
pub(crate) struct Grab {
    pub id: NodeId,
    pub start: Placement,
    pub handle: Handle,
    pub start_doc: (f64, f64),
    pub size: (u32, u32),
}

/// Typed-in transform values for the selected node.
pub(crate) struct TransformFields {
    node: NodeId,
    rev: u64,
    x: Entity<InputState>,
    y: Entity<InputState>,
    w: Entity<InputState>,
    h: Entity<InputState>,
    angle: Entity<InputState>,
    _subs: Vec<Subscription>,
}

/// A Warp in progress: the lattice's current document-space positions.
pub(crate) struct WarpState {
    pub id: NodeId,
    pub cols: usize,
    pub rows: usize,
    pub grid: Vec<(f64, f64)>,
}

fn local_corners(w: f64, h: f64) -> [(f64, f64); 4] {
    [(0.0, 0.0), (w, 0.0), (w, h), (0.0, h)]
}

fn local_edges(w: f64, h: f64) -> [(f64, f64); 4] {
    [(w / 2.0, 0.0), (w, h / 2.0), (w / 2.0, h), (0.0, h / 2.0)]
}

fn fmt(v: f64) -> String {
    let r = (v * 10.0).round() / 10.0;
    if r.fract() == 0.0 {
        format!("{r:.0}")
    } else {
        format!("{r:.1}")
    }
}

/// Text uses an anchor rotation while pixel placements rotate about the box
/// centre. Translate between them without rasterizing the editable glyphs.
fn text_transform_frame(spec: &emulsion_core::text::TextSpec) -> (u32, u32, Placement) {
    let bounds = emulsion_core::text::layout(spec).bounds();
    let w = (bounds.x + bounds.width).ceil().max(1.0) as u32;
    let h = (bounds.y + bounds.height).ceil().max(1.0) as u32;
    let mut placement = Placement {
        scale_x: f64::from(spec.scale_x.abs()),
        scale_y: f64::from(spec.scale_y.abs()),
        rotation: f64::from(spec.rotation),
        flip_x: spec.scale_x < 0.0,
        flip_y: spec.scale_y < 0.0,
        ..Default::default()
    };
    let offset = placement.to_doc(w, h).transform_point2(dvec2(0., 0.));
    placement.x = f64::from(spec.x) - offset.x;
    placement.y = f64::from(spec.y) - offset.y;
    (w, h, placement)
}

fn placement_command(node: &Node, placement: Placement) -> Command {
    if let NodeKind::Text { spec, .. } = &node.kind {
        let (w, h, _) = text_transform_frame(spec);
        let origin = placement.to_doc(w, h).transform_point2(dvec2(0., 0.));
        let mut spec = (**spec).clone();
        spec.x = origin.x as f32;
        spec.y = origin.y as f32;
        spec.rotation = placement.rotation as f32;
        spec.scale_x = (placement.scale_x * if placement.flip_x { -1. } else { 1. }) as f32;
        spec.scale_y = (placement.scale_y * if placement.flip_y { -1. } else { 1. }) as f32;
        Command::SetText {
            id: node.id,
            spec: Box::new(spec),
        }
    } else {
        Command::SetPlacement {
            id: node.id,
            placement,
        }
    }
}

impl EditorView {
    pub(crate) fn begin_transform_action(&mut self, mode: &str, cx: &mut Context<Self>) {
        self.close_text_field(cx);
        if self.drag.is_some()
            || self.warp.is_some()
            || self.editor.in_transaction()
            || self.assistant.running
        {
            self.set_status("Finish the current edit before transforming.", true, cx);
            return;
        }
        self.transform_pixels(cx);
        if self.editor.doc.selection.is_some() || self.transformable().is_none() {
            return;
        }
        if mode == "warp" {
            self.start_warp(cx);
            return;
        }
        self.set_status(
            match mode {
                "scale" => "Drag a corner to scale; drag an edge to change one dimension.",
                "rotate" => "Drag just outside a corner to rotate. Hold Shift to snap to 15°.",
                _ => "Hold Ctrl and drag a corner to distort the selected pixels.",
            },
            false,
            cx,
        );
    }

    pub(crate) fn rotate_transform_selection(&mut self, degrees: f64, cx: &mut Context<Self>) {
        self.transform_pixels_with(|id, _| Some(Command::RotateNode { id, degrees }), cx);
    }

    pub(crate) fn flip_transform_selection(&mut self, horizontal: bool, cx: &mut Context<Self>) {
        if !self
            .selected
            .and_then(|id| self.editor.doc.node(id))
            .is_some_and(|node| {
                matches!(
                    node.kind,
                    NodeKind::Raster { .. } | NodeKind::Smart { .. } | NodeKind::Text { .. }
                )
            })
        {
            self.set_status("Select a pixel, text or Smart layer to flip.", true, cx);
            return;
        }
        self.transform_pixels_with(
            |id, doc| {
                let node = doc.node(id)?;
                let mut placement = match &node.kind {
                    NodeKind::Raster { placement, .. } | NodeKind::Smart { placement, .. } => {
                        *placement
                    }
                    NodeKind::Text { spec, .. } => text_transform_frame(spec).2,
                    _ => return None,
                };
                if horizontal {
                    placement.flip_x = !placement.flip_x;
                } else {
                    placement.flip_y = !placement.flip_y;
                }
                Some(placement_command(node, placement))
            },
            cx,
        );
    }

    /// The selected node when it is a pixel node the Move tool can transform.
    pub(crate) fn transformable(&self) -> Option<(NodeId, u32, u32, Placement)> {
        if self.tool != Tool::Move {
            return None;
        }
        let id = self.selected?;
        let n = self.editor.doc.node(id)?;
        if self.editor.doc.locked_ancestor(id).is_some() || !n.visible {
            return None;
        }
        match &n.kind {
            NodeKind::Raster { raster, placement } => {
                Some((id, raster.width(), raster.height(), *placement))
            }
            NodeKind::Smart {
                source, placement, ..
            } => Some((id, source.width(), source.height(), *placement)),
            NodeKind::Text { spec, .. } => {
                let (w, h, placement) = text_transform_frame(spec);
                Some((id, w, h, placement))
            }
            _ => None,
        }
    }

    /// Where the selected layer sits, as a document-space quad, so the
    /// canvas shows what a click in the Layers panel picked. The Move tool
    /// draws its own box instead.
    pub(crate) fn layer_outline(&self) -> Option<[(f64, f64); 4]> {
        if self.tool == Tool::Move || self.warp.is_some() {
            return None;
        }
        let id = self.selected?;
        let n = self.editor.doc.node(id)?;
        let quad = |m: glam::DAffine2, w: f64, h: f64| {
            local_corners(w, h).map(|c| {
                let q = m.transform_point2(dvec2(c.0, c.1));
                (q.x, q.y)
            })
        };
        match &n.kind {
            NodeKind::Raster { raster, placement } => Some(quad(
                placement.to_doc(raster.width(), raster.height()),
                raster.width() as f64,
                raster.height() as f64,
            )),
            NodeKind::Smart {
                source, placement, ..
            } => Some(quad(
                placement.to_doc(source.width(), source.height()),
                source.width() as f64,
                source.height() as f64,
            )),
            NodeKind::Text { spec, .. } => {
                let (w, h, placement) = text_transform_frame(spec);
                Some(quad(placement.to_doc(w, h), w as f64, h as f64))
            }
            NodeKind::Path { cache, .. } => {
                let b = cache.tile_bounds();
                (!b.is_empty()).then(|| {
                    let (x, y, w, h) = (b.x as f64, b.y as f64, b.w as f64, b.h as f64);
                    [(x, y), (x + w, y), (x + w, y + h), (x, y + h)]
                })
            }
            _ => None,
        }
    }

    /// The node's corners in document space, for the overlay.
    pub(crate) fn transform_box(&self) -> Option<[(f64, f64); 4]> {
        if self.warp.is_some() {
            return None;
        }
        if let Some(Drag::Distort { quad, .. }) = &self.drag {
            return Some(*quad);
        }
        let (_, w, h, p) = self.transformable()?;
        let m = p.to_doc(w, h);
        Some(local_corners(w as f64, h as f64).map(|c| {
            let q = m.transform_point2(dvec2(c.0, c.1));
            (q.x, q.y)
        }))
    }

    fn handle_hit(&self, pos: Point<Pixels>) -> Option<Handle> {
        let (_, w, h, p) = self.transformable()?;
        let b = self.canvas_bounds()?;
        let m = p.to_doc(w, h);
        let (sx, sy) = (f32::from(pos.x) as f64, f32::from(pos.y) as f64);
        let screen = |c: (f64, f64)| {
            let q = m.transform_point2(dvec2(c.0, c.1));
            self.view.doc_to_screen((q.x, q.y), &b)
        };
        let dist = |c: (f64, f64)| {
            let s = screen(c);
            (s.0 - sx).hypot(s.1 - sy)
        };
        let (wf, hf) = (w as f64, h as f64);
        if let Some(i) = (0..4).find(|&i| dist(local_corners(wf, hf)[i]) <= GRAB_PX) {
            return Some(Handle::Corner(i));
        }
        if let Some(i) = (0..4).find(|&i| dist(local_edges(wf, hf)[i]) <= GRAB_PX) {
            return Some(Handle::Edge(i));
        }
        // Just outside a corner: rotate.
        let d = self.doc_point(pos)?;
        let q = m.inverse().transform_point2(dvec2(d.0, d.1));
        let outside = q.x < 0.0 || q.y < 0.0 || q.x > wf || q.y > hf;
        if outside && local_corners(wf, hf).iter().any(|c| dist(*c) <= ROTATE_PX) {
            return Some(Handle::Rotate);
        }
        None
    }

    /// Begin warping the selected node: a regular 3×3 lattice over it.
    pub(crate) fn start_warp(&mut self, cx: &mut Context<Self>) {
        let Some((id, w, h, p)) = self.transformable() else {
            self.set_status("Select a pixel layer to warp.", true, cx);
            return;
        };
        if !matches!(
            self.editor.doc.node(id).map(|n| &n.kind),
            Some(NodeKind::Raster { .. })
        ) {
            self.set_status("Rasterize this Smart layer before using Warp.", true, cx);
            return;
        }
        let m = p.to_doc(w, h);
        let (cols, rows) = (3usize, 3usize);
        let mut grid = Vec::with_capacity((cols + 1) * (rows + 1));
        for r in 0..=rows {
            for c in 0..=cols {
                let q = m.transform_point2(dvec2(
                    c as f64 * w as f64 / cols as f64,
                    r as f64 * h as f64 / rows as f64,
                ));
                grid.push((q.x, q.y));
            }
        }
        self.warp = Some(WarpState {
            id,
            cols,
            rows,
            grid,
        });
        self.set_status("Warp: drag the grid points, then apply.", false, cx);
        cx.notify();
    }

    pub(crate) fn cancel_warp(&mut self, cx: &mut Context<Self>) {
        self.warp = None;
        self.cancel_transform_lift(cx);
        cx.notify();
    }

    /// Resample the node through the warped lattice.
    pub(crate) fn finish_warp(&mut self, cx: &mut Context<Self>) {
        let Some(wst) = self.warp.take() else {
            return;
        };
        let Some(n) = self.editor.doc.node(wst.id) else {
            return;
        };
        let NodeKind::Raster { raster, .. } = &n.kind else {
            return;
        };
        let (raster, mask, id) = (raster.clone(), n.mask.clone(), wst.id);
        self.set_status("Warping…", false, cx);
        let ticket = self.begin_edit_job();
        cx.spawn(async move |this, cx| {
            let result = cx
                .background_spawn(async move {
                    let (r, b) = warp::warp_mesh(&raster, &wst.grid, wst.cols, wst.rows, [0; 4])?;
                    let m = match mask {
                        Some(m) => Some(Arc::new(
                            warp::warp_mesh(&m, &wst.grid, wst.cols, wst.rows, 0)?.0,
                        )),
                        None => None,
                    };
                    Some((r, m, b))
                })
                .await;
            this.update(cx, |this, cx| {
                if !this.accept_edit_result(ticket, "Transform", cx) {
                    return;
                }
                this.status = None;
                let Some((raster, mask, b)) = result else {
                    this.set_status("That warp folds the image over itself.", true, cx);
                    return;
                };
                this.execute(
                    Command::ReplaceContent {
                        id,
                        raster: Arc::new(raster),
                        mask,
                        placement: Placement::at(b.x as f64, b.y as f64),
                        label: "Warp".into(),
                    },
                    cx,
                );
            })
            .ok();
        })
        .detach();
    }

    /// Lattice lines and points for the overlay, in document pixels.
    pub(crate) fn warp_overlay(&self) -> (Vec<super::guides::Polyline>, Vec<(f64, f64)>) {
        let Some(w) = &self.warp else {
            return (Vec::new(), Vec::new());
        };
        let mut lines = Vec::new();
        for r in 0..=w.rows {
            lines.push((0..=w.cols).map(|c| w.grid[r * (w.cols + 1) + c]).collect());
        }
        for c in 0..=w.cols {
            lines.push((0..=w.rows).map(|r| w.grid[r * (w.cols + 1) + c]).collect());
        }
        (lines, w.grid.clone())
    }

    /// Start a transform or distort if the press is on a handle.
    pub(crate) fn transform_down(&mut self, e: &MouseDownEvent) -> bool {
        if let Some(w) = &self.warp {
            let Some(b) = self.canvas_bounds() else {
                return true;
            };
            let (sx, sy) = (
                f32::from(e.position.x) as f64,
                f32::from(e.position.y) as f64,
            );
            if let Some(i) = w.grid.iter().position(|g| {
                let s = self.view.doc_to_screen(*g, &b);
                (s.0 - sx).hypot(s.1 - sy) <= GRAB_PX * 1.5
            }) {
                self.drag = Some(Drag::Warp(i));
            }
            // While warping, the node itself does not move.
            return true;
        }
        let Some(handle) = self.handle_hit(e.position) else {
            return false;
        };
        let Some((id, w, h, start)) = self.transformable() else {
            return false;
        };
        let Some(start_doc) = self.doc_point(e.position) else {
            return false;
        };
        if e.modifiers.control
            && let Handle::Corner(corner) = handle
        {
            if !matches!(
                self.editor.doc.node(id).map(|n| &n.kind),
                Some(NodeKind::Raster { .. })
            ) {
                self.status = Some((
                    "Rasterize this Smart layer before using Distort.".into(),
                    true,
                ));
                return true;
            }
            let quad = self.transform_box().expect("transformable");
            self.drag = Some(Drag::Distort { id, corner, quad });
            return true;
        }
        self.editor.begin(if handle == Handle::Rotate {
            "Rotate"
        } else {
            "Transform"
        });
        self.drag = Some(Drag::Transform(Grab {
            id,
            start,
            handle,
            start_doc,
            size: (w, h),
        }));
        true
    }

    pub(crate) fn transform_move(&mut self, g: Grab, d: (f64, f64), cx: &mut Context<Self>) {
        let Grab {
            id,
            start,
            handle,
            start_doc,
            size,
        } = g;
        let (w, h) = size;
        let (wf, hf) = (w as f64, h as f64);
        let m0 = start.to_doc(w, h);
        let mut p = start;
        match handle {
            Handle::Rotate => {
                let c = m0.transform_point2(dvec2(wf / 2.0, hf / 2.0));
                let a0 = (start_doc.1 - c.y).atan2(start_doc.0 - c.x);
                let a1 = (d.1 - c.y).atan2(d.0 - c.x);
                let mut deg = start.rotation + (a1 - a0).to_degrees();
                if self.drag_shift {
                    deg = (deg / 15.0).round() * 15.0;
                }
                p.rotation = (deg + 180.0).rem_euclid(360.0) - 180.0;
            }
            Handle::Corner(i) | Handle::Edge(i) => {
                let (hp, op) = match handle {
                    Handle::Corner(_) => {
                        (local_corners(wf, hf)[i], local_corners(wf, hf)[(i + 2) % 4])
                    }
                    _ => (local_edges(wf, hf)[i], local_edges(wf, hf)[(i + 2) % 4]),
                };
                let q = m0.inverse().transform_point2(dvec2(d.0, d.1));
                let ratio = |qv: f64, h: f64, o: f64| {
                    if (h - o).abs() < 1e-9 {
                        1.0
                    } else {
                        (qv - o) / (h - o)
                    }
                };
                let (mut rx, mut ry) = (ratio(q.x, hp.0, op.0), ratio(q.y, hp.1, op.1));
                match handle {
                    Handle::Edge(0) | Handle::Edge(2) => rx = 1.0,
                    Handle::Edge(_) => ry = 1.0,
                    _ if !self.drag_shift => {
                        let r = if rx.abs() > ry.abs() { rx } else { ry };
                        (rx, ry) = (r, r);
                    }
                    _ => {}
                }
                p.scale_x = start.scale_x * rx.max(0.01);
                p.scale_y = start.scale_y * ry.max(0.01);
                // Keep the opposite handle where it was.
                let fixed = m0.transform_point2(dvec2(op.0, op.1));
                let moved = p.to_doc(w, h).transform_point2(dvec2(op.0, op.1));
                p.x += fixed.x - moved.x;
                p.y += fixed.y - moved.y;
            }
        }
        if let Some(node) = self.editor.doc.node(id) {
            self.execute(placement_command(node, p), cx);
        }
    }

    /// Finish a distort: re-project the pixels (and mask) onto the quad.
    pub(crate) fn finish_distort(
        &mut self,
        id: NodeId,
        quad: [(f64, f64); 4],
        cx: &mut Context<Self>,
    ) {
        let Some(n) = self.editor.doc.node(id) else {
            return;
        };
        let NodeKind::Raster { raster, placement } = &n.kind else {
            return;
        };
        let (w, h) = (raster.width(), raster.height());
        // The quad is in document space; map it back through the placement's
        // rotation/scale-free frame by warping the source directly.
        let untouched = placement.to_doc(w, h);
        let same = local_corners(w as f64, h as f64)
            .iter()
            .zip(&quad)
            .all(|(c, q)| {
                let p = untouched.transform_point2(dvec2(c.0, c.1));
                (p.x - q.0).abs() < 0.01 && (p.y - q.1).abs() < 0.01
            });
        if same {
            return;
        }
        let (raster, mask) = (raster.clone(), n.mask.clone());
        self.set_status("Distorting…", false, cx);
        let ticket = self.begin_edit_job();
        cx.spawn(async move |this, cx| {
            let result = cx
                .background_spawn(async move {
                    let (r, b) = warp::warp(&raster, quad, [0; 4])?;
                    let m = match mask {
                        Some(m) => Some(Arc::new(warp::warp(&m, quad, 0)?.0)),
                        None => None,
                    };
                    Some((r, m, b))
                })
                .await;
            this.update(cx, |this, cx| {
                if !this.accept_edit_result(ticket, "Transform", cx) {
                    return;
                }
                this.status = None;
                let Some((raster, mask, b)) = result else {
                    this.set_status("That shape cannot be distorted to.", true, cx);
                    return;
                };
                this.execute(
                    Command::ReplaceContent {
                        id,
                        raster: Arc::new(raster),
                        mask,
                        placement: Placement::at(b.x as f64, b.y as f64),
                        label: "Distort".into(),
                    },
                    cx,
                );
            })
            .ok();
        })
        .detach();
    }

    pub(crate) fn distort_move(&mut self, corner: usize, d: (f64, f64), cx: &mut Context<Self>) {
        if let Some(Drag::Distort { quad, .. }) = &mut self.drag {
            quad[corner] = d;
            cx.notify();
        }
    }

    // ── Numeric fields ──────────────────────────────────────────────────

    /// Keep the typed-in fields in step with the selected node.
    pub(crate) fn sync_transform_fields(&mut self, window: &mut Window, cx: &mut Context<Self>) {
        let Some((id, w, h, p)) = self.transformable() else {
            self.transform_fields = None;
            return;
        };
        let values = [
            fmt(p.x),
            fmt(p.y),
            fmt(w as f64 * p.scale_x),
            fmt(h as f64 * p.scale_y),
            fmt(p.rotation),
        ];
        let rev = self.editor.revision;
        match &mut self.transform_fields {
            Some(f) if f.node == id => {
                if f.rev == rev {
                    return;
                }
                let fields = [&f.x, &f.y, &f.w, &f.h, &f.angle];
                if fields
                    .iter()
                    .any(|e| e.read(cx).focus_handle(cx).is_focused(window))
                {
                    return;
                }
                f.rev = rev;
                for (e, v) in fields.into_iter().zip(values) {
                    e.update(cx, |s, cx| s.set_value(v, window, cx));
                }
            }
            _ => {
                let mk = |v: &String, window: &mut Window, cx: &mut Context<Self>| {
                    cx.new(|cx| InputState::new(window, cx).default_value(v.clone()))
                };
                let (x, y, wi, hi, angle) = (
                    mk(&values[0], window, cx),
                    mk(&values[1], window, cx),
                    mk(&values[2], window, cx),
                    mk(&values[3], window, cx),
                    mk(&values[4], window, cx),
                );
                let subs = [&x, &y, &wi, &hi, &angle]
                    .into_iter()
                    .map(|e| {
                        cx.subscribe_in(e, window, |this, _, ev: &InputEvent, _, cx| {
                            if matches!(ev, InputEvent::PressEnter { .. } | InputEvent::Blur) {
                                this.apply_transform_fields(cx);
                            }
                        })
                    })
                    .collect();
                self.transform_fields = Some(TransformFields {
                    node: id,
                    rev,
                    x,
                    y,
                    w: wi,
                    h: hi,
                    angle,
                    _subs: subs,
                });
            }
        }
    }

    fn apply_transform_fields(&mut self, cx: &mut Context<Self>) {
        let Some(f) = &self.transform_fields else {
            return;
        };
        let Some((id, w, h, start)) = self.transformable() else {
            return;
        };
        if f.node != id {
            return;
        }
        let num = |e: &Entity<InputState>| {
            e.read(cx)
                .value()
                .trim()
                .trim_end_matches(['°', '%'])
                .parse::<f64>()
                .ok()
                .filter(|v| v.is_finite())
        };
        let (Some(x), Some(y), Some(nw), Some(nh), Some(angle)) =
            (num(&f.x), num(&f.y), num(&f.w), num(&f.h), num(&f.angle))
        else {
            self.set_status("Transform values must be numbers.", true, cx);
            return;
        };
        if nw < 1.0 || nh < 1.0 || nw > 60_000.0 || nh > 60_000.0 || x.abs() > 1e6 || y.abs() > 1e6
        {
            self.set_status("Width and height must be 1 to 60000 px.", true, cx);
            return;
        }
        let mut p = start;
        p.x = x;
        p.y = y;
        p.scale_x = nw / w as f64;
        p.scale_y = nh / h as f64;
        p.rotation = (angle + 180.0).rem_euclid(360.0) - 180.0;
        if p != start
            && let Some(node) = self.editor.doc.node(id)
        {
            self.execute(placement_command(node, p), cx);
        }
    }

    /// The fields for the Move tool's context bar.
    pub(crate) fn transform_field_views(&self, p: &Palette) -> Vec<AnyElement> {
        let Some(f) = &self.transform_fields else {
            return Vec::new();
        };
        [
            ("X", &f.x),
            ("Y", &f.y),
            ("W", &f.w),
            ("H", &f.h),
            ("∠", &f.angle),
        ]
        .into_iter()
        .map(|(l, e)| {
            div()
                .flex()
                .flex_none()
                .items_center()
                .gap(px(4.))
                .child(mono(l, 10., p.muted))
                .child(div().w(px(64.)).child(Input::new(e).small()))
                .into_any_element()
        })
        .collect()
    }
}

/// Paint the transform box and its handles.
pub(crate) fn paint_box(
    quad: [(f64, f64); 4],
    view: &View,
    bounds: Bounds<Pixels>,
    ink: Hsla,
    window: &mut Window,
) {
    let s = |p: (f64, f64)| {
        let q = view.doc_to_screen(p, &bounds);
        point(px(q.0 as f32), px(q.1 as f32))
    };
    let mut path = PathBuilder::stroke(px(1.));
    path.move_to(s(quad[0]));
    for q in &quad[1..] {
        path.line_to(s(*q));
    }
    path.line_to(s(quad[0]));
    if let Ok(p) = path.build() {
        window.paint_path(p, ink);
    }
    let mids: Vec<(f64, f64)> = (0..4)
        .map(|i| {
            let (a, b) = (quad[i], quad[(i + 1) % 4]);
            ((a.0 + b.0) / 2.0, (a.1 + b.1) / 2.0)
        })
        .collect();
    for c in quad.iter().chain(&mids) {
        let c = s(*c);
        let half = px(HANDLE_PX / 2.0);
        window.paint_quad(
            fill(
                Bounds::new(
                    point(c.x - half, c.y - half),
                    size(px(HANDLE_PX), px(HANDLE_PX)),
                ),
                gpui_kit::white(),
            )
            .border_widths(px(1.))
            .border_color(ink),
        );
    }
}
