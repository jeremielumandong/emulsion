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
    pub collective: bool,
    pub current: Placement,
    pub mask: Option<(glam::DAffine2, [f64; 6])>,
}

/// Typed-in transform values for the selected node.
pub(crate) struct TransformFields {
    node: NodeId,
    selection: Vec<NodeId>,
    mask_target: bool,
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

fn raster_frame(
    raster: &emulsion_raster::Raster,
    placement: Placement,
    mask: Option<&emulsion_raster::Mask>,
) -> (emulsion_raster::IRect, Placement) {
    let bounds = emulsion_core::geometry::ink_bounds(raster, mask);
    let bounds = if bounds.is_empty() {
        raster.bounds()
    } else {
        bounds
    };
    let origin = placement
        .to_doc(raster.width(), raster.height())
        .transform_point2(dvec2(bounds.x as f64, bounds.y as f64));
    let mut frame = placement;
    let local = frame
        .to_doc(bounds.w as u32, bounds.h as u32)
        .transform_point2(dvec2(0., 0.));
    frame.x += origin.x - local.x;
    frame.y += origin.y - local.y;
    (bounds, frame)
}

fn placement_command(node: &Node, mut placement: Placement) -> Command {
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
        if let NodeKind::Raster {
            raster,
            placement: old,
        } = &node.kind
        {
            let (bounds, _) = raster_frame(
                raster,
                *old,
                emulsion_core::Document::composite_mask(node).as_deref(),
            );
            let origin = placement
                .to_doc(bounds.w as u32, bounds.h as u32)
                .transform_point2(dvec2(0., 0.));
            let source_origin = placement
                .to_doc(raster.width(), raster.height())
                .transform_point2(dvec2(bounds.x as f64, bounds.y as f64));
            placement.x += origin.x - source_origin.x;
            placement.y += origin.y - source_origin.y;
        }
        Command::SetPlacement {
            id: node.id,
            placement,
        }
    }
}

fn stored_composite_mask(node: &Node) -> Option<Arc<emulsion_raster::Mask>> {
    let mut visible = node.clone();
    visible.mask_enabled = true;
    emulsion_core::Document::composite_mask(&visible)
}

fn stationary_mask(node: &Node) -> Option<(Arc<emulsion_raster::Mask>, glam::DAffine2)> {
    (!node.mask_linked)
        .then(|| {
            node.mask.clone().map(|mask| {
                (
                    mask,
                    emulsion_core::transform::mask_to_document(node).inverse(),
                )
            })
        })
        .flatten()
}

fn place_stationary_mask(
    mask: &emulsion_raster::Mask,
    inverse: glam::DAffine2,
    bounds: emulsion_raster::IRect,
) -> Arc<emulsion_raster::Mask> {
    Arc::new(emulsion_raster::Mask::from_fn(
        bounds.w as u32,
        bounds.h as u32,
        mask.fill(),
        |x, y| {
            emulsion_core::transform::sample_mask(
                mask,
                inverse.transform_point2(dvec2(
                    bounds.x as f64 + x as f64 + 0.5,
                    bounds.y as f64 + y as f64 + 0.5,
                )),
            )
        },
    ))
}

impl EditorView {
    pub(crate) fn mask_transform_target(&self) -> Option<(NodeId, emulsion_raster::IRect)> {
        if !self.tools.mask_edit || self.selected_layer_ids().len() != 1 {
            return None;
        }
        let id = self.selected?;
        let node = self.editor.doc.node(id)?;
        if node.mask_linked
            || node.locked
            || self.editor.doc.locked_ancestor(id).is_some()
            || self.editor.doc.layer_locks(id).position
        {
            return None;
        }
        Some((id, emulsion_core::transform::mask_bounds(node)?))
    }

    pub(crate) fn mask_transform_command(&self, delta: glam::DAffine2) -> Option<Command> {
        let (id, _) = self.mask_transform_target()?;
        let node = self.editor.doc.node(id)?;
        Some(Command::SetMaskTransform {
            id,
            transform: (emulsion_core::transform::local_to_document(node).inverse()
                * delta
                * emulsion_core::transform::mask_to_document(node))
            .to_cols_array(),
        })
    }

    fn collective_transform(&self) -> bool {
        if self.mask_transform_target().is_some() {
            return false;
        }
        let roots = emulsion_core::layer_links::movement_roots(
            &self.editor.doc,
            &self.selected_layer_roots(),
        )
        .unwrap_or_default();
        roots.len() > 1
            || self
                .selected
                .and_then(|id| self.editor.doc.node(id))
                .is_some_and(|n| {
                    if matches!(n.kind, NodeKind::Text { .. }) && n.mask.is_some() {
                        return true;
                    }
                    !matches!(
                        n.kind,
                        NodeKind::Raster { .. } | NodeKind::Smart { .. } | NodeKind::Text { .. }
                    )
                })
    }

    fn frame_transform_command(&self, delta: glam::DAffine2) -> Command {
        self.mask_transform_command(delta)
            .unwrap_or_else(|| Command::TransformNodes {
                ids: self.selected_layer_roots(),
                transform: delta.to_cols_array(),
            })
    }

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
        if self.collective_transform() || self.mask_transform_target().is_some() {
            self.set_tool(Tool::Move, cx);
            if let Some((_, w, h, p)) = self.transformable() {
                let center = p
                    .to_doc(w, h)
                    .transform_point2(dvec2(w as f64 / 2., h as f64 / 2.));
                let delta = glam::DAffine2::from_translation(center)
                    * glam::DAffine2::from_angle(degrees.to_radians())
                    * glam::DAffine2::from_translation(-center);
                self.execute(self.frame_transform_command(delta), cx);
            }
            return;
        }
        self.transform_pixels_with(|id, _| Some(Command::RotateNode { id, degrees }), cx);
    }

    pub(crate) fn flip_transform_selection(&mut self, horizontal: bool, cx: &mut Context<Self>) {
        if self.collective_transform() || self.mask_transform_target().is_some() {
            self.set_tool(Tool::Move, cx);
            if let Some((_, w, h, p)) = self.transformable() {
                let center = p
                    .to_doc(w, h)
                    .transform_point2(dvec2(w as f64 / 2., h as f64 / 2.));
                let scale = if horizontal {
                    dvec2(-1., 1.)
                } else {
                    dvec2(1., -1.)
                };
                let delta = glam::DAffine2::from_translation(center)
                    * glam::DAffine2::from_scale(scale)
                    * glam::DAffine2::from_translation(-center);
                self.execute(self.frame_transform_command(delta), cx);
            }
            return;
        }
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
                    NodeKind::Raster { raster, placement } => {
                        raster_frame(
                            raster,
                            *placement,
                            emulsion_core::Document::composite_mask(node).as_deref(),
                        )
                        .1
                    }
                    NodeKind::Smart { placement, .. } => *placement,
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
        if let Some((id, b)) = self.mask_transform_target() {
            return Some((
                id,
                b.w as u32,
                b.h as u32,
                Placement::at(b.x as f64, b.y as f64),
            ));
        }
        if self.collective_transform() {
            let roots = emulsion_core::layer_links::movement_roots(
                &self.editor.doc,
                &self.selected_layer_roots(),
            )
            .ok()?;
            for id in &roots {
                for member in self.editor.doc.subtree(*id) {
                    if self.editor.doc.locked_ancestor(member).is_some()
                        || self.editor.doc.layer_locks(member).position
                    {
                        return None;
                    }
                }
            }
            let bounds = roots
                .into_iter()
                .filter_map(|id| emulsion_core::geometry::node_bounds(&self.editor.doc, id))
                .reduce(|a, b| a.union(&b))?;
            return Some((
                self.selected?,
                bounds.w as u32,
                bounds.h as u32,
                Placement::at(bounds.x as f64, bounds.y as f64),
            ));
        }
        let id = self.selected?;
        let n = self.editor.doc.node(id)?;
        if self.editor.doc.locked_ancestor(id).is_some()
            || self.editor.doc.layer_locks(id).position
            || !n.visible
        {
            return None;
        }
        match &n.kind {
            NodeKind::Raster { raster, placement } => {
                let (bounds, frame) = raster_frame(
                    raster,
                    *placement,
                    emulsion_core::Document::composite_mask(n).as_deref(),
                );
                Some((id, bounds.w as u32, bounds.h as u32, frame))
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
        if self.selected_layer_ids().len() == 1
            && self.warp.is_none()
            && let Some(id) = self.selected
            && self
                .editor
                .doc
                .node(id)
                .is_some_and(|node| matches!(node.kind, NodeKind::Fill { .. }))
        {
            let b = emulsion_core::geometry::node_bounds(&self.editor.doc, id)?;
            return Some([
                (b.x as f64, b.y as f64),
                (b.right() as f64, b.y as f64),
                (b.right() as f64, b.bottom() as f64),
                (b.x as f64, b.bottom() as f64),
            ]);
        }
        if self.selected_layer_ids().len() > 1 && self.warp.is_none() {
            let bounds = self
                .selected_layer_roots()
                .into_iter()
                .filter_map(|id| emulsion_core::geometry::node_bounds(&self.editor.doc, id))
                .reduce(|a, b| a.union(&b))?;
            let (x, y, right, bottom) = (
                bounds.x as f64,
                bounds.y as f64,
                bounds.right() as f64,
                bounds.bottom() as f64,
            );
            return Some([(x, y), (right, y), (right, bottom), (x, bottom)]);
        }
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
            NodeKind::Raster { raster, placement } => {
                let (bounds, frame) = raster_frame(
                    raster,
                    *placement,
                    emulsion_core::Document::composite_mask(n).as_deref(),
                );
                Some(quad(
                    frame.to_doc(bounds.w as u32, bounds.h as u32),
                    bounds.w as f64,
                    bounds.h as f64,
                ))
            }
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
        if let Some(Drag::Transform(grab)) = &self.drag
            && (grab.collective || grab.mask.is_some())
        {
            let (w, h) = grab.size;
            let m = grab.current.to_doc(w, h);
            return Some(local_corners(w as f64, h as f64).map(|(x, y)| {
                let p = m.transform_point2(dvec2(x, y));
                (p.x, p.y)
            }));
        }
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
        if self.collective_transform() || self.mask_transform_target().is_some() {
            self.set_status("Warp requires one raster layer's content.", true, cx);
            return;
        }
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
        // Mesh resampling consumes the full stored source, including its mask.
        // Keep this lattice in that source frame rather than the tight handles.
        let (w, h, p) = match &self.editor.doc.node(id).expect("selected raster").kind {
            NodeKind::Raster { raster, placement } => (raster.width(), raster.height(), *placement),
            _ => (w, h, p),
        };
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
        let stationary = stationary_mask(n);
        let (raster, mask, id) = (raster.clone(), stored_composite_mask(n), wst.id);
        self.set_status("Warping…", false, cx);
        let ticket = self.begin_edit_job();
        cx.spawn(async move |this, cx| {
            let result = cx
                .background_spawn(async move {
                    let (r, b) = warp::warp_mesh(&raster, &wst.grid, wst.cols, wst.rows, [0; 4])?;
                    let m = if let Some((mask, inverse)) = stationary {
                        Some(place_stationary_mask(&mask, inverse, b))
                    } else {
                        match mask {
                            Some(m) => Some(Arc::new(
                                warp::warp_mesh(&m, &wst.grid, wst.cols, wst.rows, 0)?.0,
                            )),
                            None => None,
                        }
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
            if self.collective_transform()
                || self.mask_transform_target().is_some()
                || !matches!(
                    self.editor.doc.node(id).map(|n| &n.kind),
                    Some(NodeKind::Raster { .. })
                )
            {
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
            // A moving unlinked mask changes the visible bounds. Keep the
            // initial frame/source snapshot for every preview, never measure
            // the previous preview again when applying the next pointer delta.
            collective: self.collective_transform()
                || self.editor.doc.node(id).is_some_and(|n| n.mask.is_some()),
            current: start,
            mask: self
                .mask_transform_target()
                .and_then(|(id, _)| self.editor.doc.node(id))
                .map(|n| {
                    (
                        emulsion_core::transform::local_to_document(n),
                        n.mask_transform,
                    )
                }),
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
            ..
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
        if g.collective || g.mask.is_some() {
            let delta = p.to_doc(w, h) * m0.inverse();
            let command = if let Some((local, original)) = g.mask {
                Command::SetMaskTransform {
                    id,
                    transform: (local.inverse()
                        * delta
                        * local
                        * glam::DAffine2::from_cols_array(&original))
                    .to_cols_array(),
                }
            } else {
                Command::TransformNodes {
                    ids: self.selected_layer_roots(),
                    transform: delta.to_cols_array(),
                }
            };
            match self.editor.preview(command) {
                Ok(_) => {
                    self.drag = Some(Drag::Transform(Grab { current: p, ..g }));
                    self.after_change(cx);
                }
                Err(error) => self.set_status(error.to_string(), true, cx),
            }
        } else if let Some(node) = self.editor.doc.node(id) {
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
        let (bounds, frame) = raster_frame(
            raster,
            *placement,
            emulsion_core::Document::composite_mask(n).as_deref(),
        );
        let (w, h) = (bounds.w as u32, bounds.h as u32);
        // The quad is in document space; map it back through the placement's
        // rotation/scale-free frame by warping the source directly.
        let untouched = frame.to_doc(w, h);
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
        // Extend the tight handle mapping to the full source rectangle. The
        // original pixel and mask buffers must share exactly the same mapping.
        let source = local_corners(w as f64, h as f64)
            .map(|(x, y)| (x + bounds.x as f64, y + bounds.y as f64));
        let Some(mapping) = warp::homography(source, quad) else {
            return;
        };
        let full_source = local_corners(raster.width() as f64, raster.height() as f64);
        let denominators = full_source.map(|(x, y)| mapping[6] * x + mapping[7] * y + mapping[8]);
        if !denominators.iter().all(|v| v.is_finite() && *v > 1e-9)
            && !denominators.iter().all(|v| v.is_finite() && *v < -1e-9)
        {
            self.set_status(
                "That distortion crosses the source image's perspective horizon.",
                true,
                cx,
            );
            return;
        }
        let quad = full_source.map(|point| warp::apply(&mapping, point));
        let stationary = stationary_mask(n);
        let (raster, mask) = (raster.clone(), stored_composite_mask(n));
        self.set_status("Distorting…", false, cx);
        let ticket = self.begin_edit_job();
        cx.spawn(async move |this, cx| {
            let result = cx
                .background_spawn(async move {
                    let (r, b) = warp::warp(&raster, quad, [0; 4])?;
                    let m = if let Some((mask, inverse)) = stationary {
                        Some(place_stationary_mask(&mask, inverse, b))
                    } else {
                        match mask {
                            Some(m) => Some(Arc::new(warp::warp(&m, quad, 0)?.0)),
                            None => None,
                        }
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
        let selection = self.selected_layer_ids();
        let mask_target = self.mask_transform_target().is_some();
        match &mut self.transform_fields {
            Some(f) if f.node == id && f.selection == selection && f.mask_target == mask_target => {
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
                    selection,
                    mask_target,
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
        if p != start && (self.collective_transform() || self.mask_transform_target().is_some()) {
            self.execute(
                self.frame_transform_command(p.to_doc(w, h) * start.to_doc(w, h).inverse()),
                cx,
            );
        } else if p != start
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

#[cfg(test)]
mod sparse_frame_tests {
    use super::{local_corners, placement_command, raster_frame};
    use emulsion_core::{Command, Node};
    use emulsion_raster::Placement;
    use glam::dvec2;
    use std::sync::Arc;

    #[test]
    fn tight_frame_maps_flipped_rotated_source_without_losing_pixels() {
        let raster = Arc::new(emulsion_raster::Raster::from_fn(
            300,
            200,
            [0; 4],
            |x, y| {
                if (40..90).contains(&x) && (60..80).contains(&y) {
                    [65535; 4]
                } else {
                    [0; 4]
                }
            },
        ));
        let original = Placement {
            x: 25.,
            y: 13.,
            scale_x: 1.5,
            scale_y: 2.,
            rotation: 37.,
            flip_x: true,
            ..Default::default()
        };
        let node = Node::raster(1, "Sparse", raster.clone(), original);
        let (bounds, frame) = raster_frame(&raster, original, None);
        assert_eq!((bounds.x, bounds.y, bounds.w, bounds.h), (40, 60, 50, 20));
        for change in [
            frame,
            Placement {
                x: frame.x + 17.,
                y: frame.y - 10.,
                rotation: 85.,
                scale_x: 3.,
                flip_x: false,
                flip_y: true,
                ..frame
            },
        ] {
            let Command::SetPlacement { placement, .. } = placement_command(&node, change) else {
                panic!()
            };
            for (x, y) in local_corners(50., 20.) {
                let actual = placement
                    .to_doc(300, 200)
                    .transform_point2(dvec2(x + 40., y + 60.));
                let expected = change.to_doc(50, 20).transform_point2(dvec2(x, y));
                assert!((actual - expected).length() < 1e-8);
            }
        }
        let Command::SetPlacement { placement, .. } = placement_command(&node, frame) else {
            panic!()
        };
        assert!((placement.x - original.x).abs() < 1e-8);
        assert!((placement.y - original.y).abs() < 1e-8);
    }
}
