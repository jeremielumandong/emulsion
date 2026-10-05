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
    pub mask: Option<(MaskEditTarget, glam::DAffine2, [f64; 6])>,
}

/// Typed-in transform values for the selected node.
pub(crate) struct TransformFields {
    node: NodeId,
    selection: Vec<NodeId>,
    mask_target: MaskEditTarget,
    ticket: (u64, u64),
    modal: bool,
    frame: (u32, u32, Placement),
    applied_values: [u64; 5],
    invalid: bool,
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
    let w = spec
        .width
        .unwrap_or(bounds.x + bounds.width)
        .ceil()
        .max(1.0) as u32;
    let h = spec
        .height
        .unwrap_or(bounds.y + bounds.height)
        .ceil()
        .max(1.0) as u32;
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

/// Alpha below which a pixel cannot change the picture by even one 8-bit
/// code, so it should not push the layer boundary outwards. A soft brush's
/// falloff trails off far past anything visible, and counting every non-zero
/// step puts the dashed box well outside the content people can see.
const OUTLINE_MIN_ALPHA: u16 = u16::MAX / 255;

fn raster_frame(
    raster: &emulsion_raster::Raster,
    placement: Placement,
    mask: Option<&emulsion_raster::Mask>,
) -> (emulsion_raster::IRect, Placement) {
    let bounds = match mask {
        // A masked layer still needs the exact masked coverage.
        Some(_) => emulsion_core::geometry::ink_bounds(raster, mask),
        None => raster.coverage_bounds_above(OUTLINE_MIN_ALPHA),
    };
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

fn placement_command(doc: &Document, node: &Node, mut placement: Placement) -> Command {
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
            let (bounds, _) = raster_frame(raster, *old, doc.composite_mask(node).as_deref());
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

fn stored_composite_mask(doc: &Document, node: &Node) -> Option<Arc<emulsion_raster::Mask>> {
    doc.mask_for_inspection(node)
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
        let target = self.tools.mask_edit_target;
        if !target.is_mask() || self.selected_layer_ids().len() != 1 {
            return None;
        }
        let id = self.selected?;
        let node = self.editor.doc.node(id)?;
        if !self.mask_component_ready(id, target, true)
            || (target == MaskEditTarget::RasterMask && node.mask_linked)
        {
            return None;
        }
        Some((id, target.bounds(&self.editor.doc, node)?))
    }

    pub(crate) fn mask_transform_command(&self, delta: glam::DAffine2) -> Option<Command> {
        let (id, _) = self.mask_transform_target()?;
        let node = self.editor.doc.node(id)?;
        let target = self.tools.mask_edit_target;
        Some(
            target.transform_command(
                id,
                (emulsion_core::transform::local_to_document(node).inverse()
                    * delta
                    * target.to_document(node)?)
                .to_cols_array(),
            ),
        )
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
                    if matches!(n.kind, NodeKind::Text { .. }) && n.has_mask() {
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
        if self.is_photo_workflow() && matches!(mode, "scale" | "rotate") {
            if !self.photo_transform_active() {
                self.begin_photo_transform(false, cx);
            } else if self.drag.is_some() {
                self.set_status(t!("editor.transform.finish_edit"), true, cx);
                return;
            }
            if self.photo_transform_active() {
                self.transform_control_mode = if mode == "rotate" {
                    TransformControlMode::Rotate
                } else {
                    TransformControlMode::Resize
                };
                cx.notify();
            }
            return;
        }
        if !self.photo_transform_ready(cx) {
            return;
        }
        self.close_text_field(cx);
        if self.drag.is_some()
            || self.warp.is_some()
            || self.editor.in_transaction()
            || self.assistant.running
        {
            self.set_status(t!("editor.transform.finish_edit"), true, cx);
            return;
        }
        // Keep the lift's cancellation owner when entering a non-affine mode.
        // Calling transform_pixels here would start Photo's affine modal session.
        self.prepare_transform_pixels(cx);
        if self.editor.doc.selection.is_some() || self.transformable().is_none() {
            return;
        }
        // Non-affine modes keep their original corner/lattice interaction,
        // even when a preceding click selected rotation handles.
        self.transform_control_mode = if self.has_transform_controls() && mode == "rotate" {
            TransformControlMode::Rotate
        } else {
            TransformControlMode::Resize
        };
        if mode == "warp" {
            self.start_warp(cx);
            return;
        }
        self.set_status(
            match mode {
                "scale" => t!("editor.transform.scale_hint"),
                "rotate" if self.has_transform_controls() => {
                    t!("editor.transform.rotation_controls_hint")
                }
                "rotate" => t!("editor.transform.rotate_hint"),
                _ => t!("editor.transform.distort_hint"),
            },
            false,
            cx,
        );
    }

    pub(crate) fn rotate_transform_selection(&mut self, degrees: f64, cx: &mut Context<Self>) {
        if self.photo_transform_active() {
            if let Some((_, w, h, p)) = self.transformable() {
                let center = p
                    .to_doc(w, h)
                    .transform_point2(dvec2(w as f64 / 2., h as f64 / 2.));
                self.photo_transform_delta(
                    glam::DAffine2::from_translation(center)
                        * glam::DAffine2::from_angle(degrees.to_radians())
                        * glam::DAffine2::from_translation(-center),
                    cx,
                );
            }
            return;
        }

        if !self.tools.mask_edit_target.is_mask()
            && self
                .selected
                .is_some_and(|id| self.rotates_photo_canvas(id))
        {
            self.rotate_selected_node(degrees, cx);
            return;
        }
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
        if self.photo_transform_active() {
            if let Some((_, w, h, p)) = self.transformable() {
                let center = p
                    .to_doc(w, h)
                    .transform_point2(dvec2(w as f64 / 2., h as f64 / 2.));
                let scale = if horizontal {
                    dvec2(-1., 1.)
                } else {
                    dvec2(1., -1.)
                };
                self.photo_transform_delta(
                    glam::DAffine2::from_translation(center)
                        * glam::DAffine2::from_scale(scale)
                        * glam::DAffine2::from_translation(-center),
                    cx,
                );
            }
            return;
        }

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
            self.set_status(t!("editor.transform.flip_needs"), true, cx);
            return;
        }
        self.transform_pixels_with(
            |id, doc| {
                let node = doc.node(id)?;
                let mut placement = match &node.kind {
                    NodeKind::Raster { raster, placement } => {
                        raster_frame(raster, *placement, doc.composite_mask(node).as_deref()).1
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
                Some(placement_command(doc, node, placement))
            },
            cx,
        );
    }

    /// The selected node when it is a pixel node the Move tool can transform.
    pub(crate) fn transformable(&self) -> Option<(NodeId, u32, u32, Placement)> {
        if self.single_selected_connector() {
            return None;
        }
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
        if self.tools.mask_edit_target.is_mask() {
            return None;
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
                    self.editor.doc.composite_mask(n).as_deref(),
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
        if self.single_selected_connector() {
            return None;
        }
        // Only after the user clicks a layer row, not for whatever happens to
        // be selected when a document opens.
        if !self.layer_outline_shown {
            return None;
        }
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
                    self.editor.doc.composite_mask(n).as_deref(),
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
            NodeKind::Path { path, style, .. } => {
                // The path's own extent: tighter than the rasterised tile
                // bounds, which are 256-aligned, and it needs no pixels.
                let b = path.bounds(style);
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
        // An animated storyboard layer's box follows its keys.
        let m = self.layer_motion_matrix() * p.to_doc(w, h);
        Some(local_corners(w as f64, h as f64).map(|c| {
            let q = m.transform_point2(dvec2(c.0, c.1));
            (q.x, q.y)
        }))
    }

    pub(super) fn diagram_corner_down(&mut self, event: &MouseDownEvent) -> bool {
        // A nonrectangular stencil's bounding-box corner can be empty canvas.
        // Its visible resize handle must win before diagram selection starts a marquee.
        self.is_diagram()
            && self.tool == Tool::Move
            && !self.diagram_ui.connecting
            && matches!(self.handle_hit(event.position), Some(Handle::Corner(_)))
            && self.transform_down(event)
    }

    pub(super) fn handle_hit(&self, pos: Point<Pixels>) -> Option<Handle> {
        let (_, w, h, p) = self.transformable()?;
        let b = self.canvas_bounds()?;
        let m = self.layer_motion_matrix() * p.to_doc(w, h);
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
        let rotate_mode = self.has_transform_controls()
            && self.transform_control_mode == TransformControlMode::Rotate;
        if let Some(i) = (0..4).find(|&i| dist(local_corners(wf, hf)[i]) <= GRAB_PX) {
            return Some(if rotate_mode {
                Handle::Rotate
            } else {
                Handle::Corner(i)
            });
        }
        if !rotate_mode && let Some(i) = (0..4).find(|&i| dist(local_edges(wf, hf)[i]) <= GRAB_PX) {
            return Some(Handle::Edge(i));
        }
        if self.has_transform_controls()
            && let Some((_, handle)) =
                self.rotation_handle_for_frame(local_corners(wf, hf).map(screen), b)
            && (handle.0 - sx).hypot(handle.1 - sy) <= GRAB_PX + 2.
        {
            return Some(Handle::Rotate);
        }
        // Just outside a corner: rotate.
        let d = self.doc_point(pos)?;
        let q = m.inverse().transform_point2(dvec2(d.0, d.1));
        // A screen point rounds to f32 before mapping back through a rotated
        // frame. Treat a half-pixel boundary as on the frame, not outside it.
        let epsilon_x =
            0.5 / (m.transform_vector2(dvec2(1., 0.)).length() * self.view.zoom).max(1e-9);
        let epsilon_y =
            0.5 / (m.transform_vector2(dvec2(0., 1.)).length() * self.view.zoom).max(1e-9);
        let outside =
            q.x < -epsilon_x || q.y < -epsilon_y || q.x > wf + epsilon_x || q.y > hf + epsilon_y;
        if outside && local_corners(wf, hf).iter().any(|c| dist(*c) <= ROTATE_PX) {
            return Some(Handle::Rotate);
        }
        None
    }

    /// Begin warping the selected node: a regular 3×3 lattice over it.
    pub(crate) fn start_warp(&mut self, cx: &mut Context<Self>) {
        if !self.photo_transform_ready(cx) {
            return;
        }
        if self.collective_transform() || self.mask_transform_target().is_some() {
            self.set_status(t!("editor.transform.warp_one"), true, cx);
            return;
        }
        let Some((id, w, h, p)) = self.transformable() else {
            self.set_status(t!("editor.transform.warp_select"), true, cx);
            return;
        };
        if !matches!(
            self.editor.doc.node(id).map(|n| &n.kind),
            Some(NodeKind::Raster { .. })
        ) {
            self.set_status(t!("editor.transform.warp_smart"), true, cx);
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
        self.set_status(t!("editor.transform.warp_hint"), false, cx);
        cx.notify();
    }

    pub(crate) fn cancel_warp(&mut self, cx: &mut Context<Self>) {
        self.warp = None;
        self.cancel_transform_lift(cx);
        cx.notify();
    }

    /// Resample the node through the warped lattice.
    pub(crate) fn finish_warp(&mut self, cx: &mut Context<Self>) {
        if !self.photo_transform_ready(cx) {
            return;
        }
        let Some(wst) = self.warp.take() else {
            return;
        };
        let Some(n) = self.editor.doc.node(wst.id) else {
            return;
        };
        let NodeKind::Raster { raster, placement } = &n.kind else {
            return;
        };
        let matrix = placement.to_doc(raster.width(), raster.height());
        let unchanged = wst.grid.iter().enumerate().all(|(index, point)| {
            let column = index % (wst.cols + 1);
            let row = index / (wst.cols + 1);
            let original = matrix.transform_point2(dvec2(
                column as f64 * raster.width() as f64 / wst.cols as f64,
                row as f64 * raster.height() as f64 / wst.rows as f64,
            ));
            (point.0 - original.x).abs() < 1e-8 && (point.1 - original.y).abs() < 1e-8
        });
        if unchanged {
            // An untouched lattice is not an unsupported committed operation:
            // avoid resampling, an Undo step, and erasing the affine recipe.
            if self.is_photo_workflow() {
                self.cancel_transform_lift(cx);
            }
            self.status = None;
            cx.notify();
            return;
        }
        let stationary = stationary_mask(n);
        let (raster, mask, id) = (
            raster.clone(),
            stored_composite_mask(&self.editor.doc, n),
            wst.id,
        );
        self.set_status(t!("editor.transform.warping"), false, cx);
        let Some(ticket) = self.begin_edit_job() else {
            self.photo_transform_ready(cx);
            return;
        };
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
                    if this.is_photo_workflow() {
                        this.cancel_transform_lift(cx);
                    }
                    this.set_status(t!("editor.transform.warp_folds"), true, cx);
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
        let Some(start_doc) = self
            .doc_point(e.position)
            .map(|d| self.layer_motion_point(d))
        else {
            return false;
        };
        if e.modifiers.control
            && let Handle::Corner(corner) = handle
        {
            if self.photo_transform_active() {
                self.status = Some(("Distort is not supported inside an affine Free Transform. Apply or cancel first.".into(), true));
                return true;
            }
            if self.collective_transform()
                || self.mask_transform_target().is_some()
                || !matches!(
                    self.editor.doc.node(id).map(|n| &n.kind),
                    Some(NodeKind::Raster { .. })
                )
            {
                self.status = Some((t!("editor.transform.distort_smart").into(), true));
                return true;
            }
            let quad = self.transform_box().expect("transformable");
            self.drag = Some(Drag::Distort { id, corner, quad });
            return true;
        }
        self.photo_transform_begin_gesture();
        if !self.photo_transform_active() {
            self.editor.begin(if handle == Handle::Rotate {
                "Rotate"
            } else {
                "Transform"
            });
        }
        self.drag = Some(Drag::Transform(Grab {
            id,
            start,
            handle,
            start_doc,
            size: (w, h),
            // A moving unlinked mask changes the visible bounds. Keep the
            // initial frame/source snapshot for every preview, never measure
            // the previous preview again when applying the next pointer delta.
            collective: self.photo_transform_active()
                || self.collective_transform()
                || self.editor.doc.node(id).is_some_and(|n| n.has_mask()),
            current: start,
            mask: self
                .mask_transform_target()
                .and_then(|(id, _)| self.editor.doc.node(id))
                .map(|n| {
                    (
                        self.tools.mask_edit_target,
                        emulsion_core::transform::local_to_document(n),
                        self.tools.mask_edit_target.affine(n).expect("mask target"),
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
        // Edge handles reflow editable text inside its frame. Corners retain
        // the usual scale gesture. Measure from the original frame so repeated
        // pointer updates cannot accumulate drift, even for rotated text.
        if !self.photo_transform_active()
            && !g.collective
            && g.mask.is_none()
            && let Handle::Edge(edge) = handle
            && let Some(Node {
                kind: NodeKind::Text { spec, .. },
                ..
            }) = self.editor.doc.node(id)
            && spec.text_path.is_none()
        {
            let original = spec.clone();
            let mut spec = (**spec).clone();
            self.note_photo_reflow(id, original);
            let pointer = m0.inverse().transform_point2(dvec2(d.0, d.1));
            let mut origin = dvec2(0., 0.);
            match edge {
                0 => {
                    let height = (hf - pointer.y).clamp(1., 30000.);
                    origin.y = hf - height;
                    spec.height = Some(height as f32);
                }
                1 => spec.width = Some(pointer.x.clamp(1., 30000.) as f32),
                2 => spec.height = Some(pointer.y.clamp(1., 30000.) as f32),
                _ => {
                    let width = (wf - pointer.x).clamp(1., 30000.);
                    origin.x = wf - width;
                    spec.width = Some(width as f32);
                }
            }
            let origin = m0.transform_point2(origin);
            spec.x = origin.x as f32;
            spec.y = origin.y as f32;
            self.execute(
                Command::SetText {
                    id,
                    spec: Box::new(spec),
                },
                cx,
            );
            return;
        }
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
        if self.photo_transform_active() {
            if self.photo_transform_gesture(p.to_doc(w, h) * m0.inverse(), cx) {
                self.drag = Some(Drag::Transform(Grab { current: p, ..g }));
            }
        } else if g.collective || g.mask.is_some() {
            let delta = p.to_doc(w, h) * m0.inverse();
            let command = if let Some((target, local, original)) = g.mask {
                target.transform_command(
                    id,
                    if p == start || d == start_doc {
                        original
                    } else {
                        (local.inverse()
                            * delta
                            * local
                            * glam::DAffine2::from_cols_array(&original))
                        .to_cols_array()
                    },
                )
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
            self.execute(placement_command(&self.editor.doc, node, p), cx);
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
            self.editor.doc.composite_mask(n).as_deref(),
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
            if self.is_photo_workflow() {
                self.cancel_transform_lift(cx);
            }
            return;
        }
        // Extend the tight handle mapping to the full source rectangle. The
        // original pixel and mask buffers must share exactly the same mapping.
        let source = local_corners(w as f64, h as f64)
            .map(|(x, y)| (x + bounds.x as f64, y + bounds.y as f64));
        let Some(mapping) = warp::homography(source, quad) else {
            if self.is_photo_workflow() {
                self.cancel_transform_lift(cx);
            }
            return;
        };
        let full_source = local_corners(raster.width() as f64, raster.height() as f64);
        let denominators = full_source.map(|(x, y)| mapping[6] * x + mapping[7] * y + mapping[8]);
        if !denominators.iter().all(|v| v.is_finite() && *v > 1e-9)
            && !denominators.iter().all(|v| v.is_finite() && *v < -1e-9)
        {
            if self.is_photo_workflow() {
                self.cancel_transform_lift(cx);
            }
            self.set_status(t!("editor.transform.distort_horizon"), true, cx);
            return;
        }
        let quad = full_source.map(|point| warp::apply(&mapping, point));
        let stationary = stationary_mask(n);
        let (raster, mask) = (raster.clone(), stored_composite_mask(&self.editor.doc, n));
        self.set_status(t!("editor.transform.distorting"), false, cx);
        let Some(ticket) = self.begin_edit_job() else {
            self.photo_transform_ready(cx);
            return;
        };
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
                    if this.is_photo_workflow() {
                        this.cancel_transform_lift(cx);
                    }
                    this.set_status(t!("editor.transform.distort_invalid"), true, cx);
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
        let applied_values = values.each_ref().map(|value| {
            value
                .parse::<f64>()
                .expect("formatted transform number")
                .to_bits()
        });
        let ticket = self.edit_ticket();
        let modal = self.photo_transform_active();
        let selection = self.selected_layer_ids();
        let mask_target = self.tools.mask_edit_target;
        match &mut self.transform_fields {
            Some(f) if f.node == id && f.selection == selection && f.mask_target == mask_target => {
                if f.ticket == ticket && f.modal == modal {
                    return;
                }
                let fields = [&f.x, &f.y, &f.w, &f.h, &f.angle];
                if fields
                    .iter()
                    .any(|e| e.read(cx).focus_handle(cx).is_focused(window))
                {
                    return;
                }
                f.ticket = ticket;
                f.modal = modal;
                f.frame = (w, h, p);
                f.applied_values = applied_values;
                f.invalid = false;
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
                        cx.subscribe_in(e, window, |this, input, ev: &InputEvent, _, cx| {
                            if matches!(ev, InputEvent::PressEnter { .. } | InputEvent::Blur) {
                                this.apply_transform_fields(input, cx);
                            }
                        })
                    })
                    .collect();
                self.transform_fields = Some(TransformFields {
                    node: id,
                    selection,
                    mask_target,
                    ticket,
                    modal,
                    frame: (w, h, p),
                    applied_values,
                    invalid: false,
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

    fn apply_transform_fields(&mut self, input: &Entity<InputState>, cx: &mut Context<Self>) {
        let Some(f) = &self.transform_fields else {
            return;
        };
        if ![&f.x, &f.y, &f.w, &f.h, &f.angle].contains(&input) {
            return;
        }
        let Some((id, _, _, _)) = self.transformable() else {
            return;
        };
        if f.node != id
            || f.mask_target != self.tools.mask_edit_target
            || f.selection != self.selected_layer_ids()
        {
            return;
        }
        if f.ticket != self.edit_ticket()
            || f.modal != self.photo_transform_active()
            || (self.editor.in_transaction() && !f.modal)
        {
            // A focused old draft cannot replay over another edit/session.
            self.transform_fields = None;
            self.set_status(
                "The transform changed. Edit the refreshed values.",
                false,
                cx,
            );
            return;
        }
        let (w, h, start) = f.frame;
        let previous_values = f.applied_values;
        let invalid = f.invalid;
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
            self.transform_fields.as_mut().unwrap().invalid = true;
            self.invalidate_photo_transform_preview();
            self.set_status(t!("editor.transform.numbers"), true, cx);
            return;
        };
        if nw < 1.0 || nh < 1.0 || nw > 60_000.0 || nh > 60_000.0 || x.abs() > 1e6 || y.abs() > 1e6
        {
            self.transform_fields.as_mut().unwrap().invalid = true;
            self.invalidate_photo_transform_preview();
            self.set_status(t!("editor.transform.size_range"), true, cx);
            return;
        }
        let values = [x, y, nw, nh, angle].map(f64::to_bits);
        if previous_values == values && !invalid {
            return;
        }
        let mut p = start;
        // Unedited displayed values can be rounded. Preserve the accepted
        // frame rather than reapplying stale AABB/rounded fields wholesale.
        if values[0] != previous_values[0] {
            p.x = x;
        }
        if values[1] != previous_values[1] {
            p.y = y;
        }
        if values[2] != previous_values[2] {
            p.scale_x = nw / w as f64;
        }
        if values[3] != previous_values[3] {
            p.scale_y = nh / h as f64;
        }
        if values[4] != previous_values[4] {
            p.rotation = (angle + 180.0).rem_euclid(360.0) - 180.0;
        }
        let before = self.edit_ticket();
        let delta = if p == start {
            glam::DAffine2::IDENTITY
        } else {
            p.to_doc(w, h) * start.to_doc(w, h).inverse()
        };
        let applied = if self.photo_transform_active() {
            self.photo_transform_delta(delta, cx)
        } else if p != start
            && (self.collective_transform() || self.mask_transform_target().is_some())
        {
            self.execute(
                self.frame_transform_command(p.to_doc(w, h) * start.to_doc(w, h).inverse()),
                cx,
            );
            self.edit_ticket() != before
        } else if p != start {
            if let Some(node) = self.editor.doc.node(id) {
                self.execute(placement_command(&self.editor.doc, node, p), cx);
            }
            self.edit_ticket() != before
        } else {
            true
        };
        if applied {
            let ticket = self.edit_ticket();
            let modal = self.photo_transform_active();
            if let Some(fields) = &mut self.transform_fields {
                fields.ticket = ticket;
                fields.modal = modal;
                fields.frame = (w, h, p);
                fields.applied_values = values;
                fields.invalid = false;
            }
        } else if let Some(fields) = &mut self.transform_fields {
            // Command-level rejection also invalidates the modal session.
            // Re-entering the last accepted numbers must revalidate it rather
            // than taking the repeated-value fast path.
            fields.invalid = true;
        }
    }

    /// The fields for the Move tool's context bar.
    pub(super) fn photo_transform_fields(&self, p: &Palette, cx: &App) -> Vec<AnyElement> {
        let Some(fields) = &self.transform_fields else {
            return Vec::new();
        };
        [
            ("W", t!("editor.transform.w"), &fields.w),
            ("H", t!("editor.transform.h"), &fields.h),
            ("X", t!("editor.transform.x"), &fields.x),
            ("Y", t!("editor.transform.y"), &fields.y),
            ("Angle", t!("editor.transform.angle"), &fields.angle),
        ]
        .into_iter()
        .map(|(id, label, input)| {
            let focus = input.read(cx).focus_handle(cx);
            div()
                .id(SharedString::from(format!("photo-transform-{id}")))
                .test_support()
                .min_w_0()
                .on_mouse_down(MouseButton::Left, move |_, window, cx| {
                    window.focus(&focus, cx)
                })
                .child(
                    Input::new(input)
                        .aria_label(label.clone())
                        .small()
                        .h(px(26.))
                        .prefix(div().text_size(px(11.)).text_color(p.muted).child(label))
                        .font_family(MONO_FONT)
                        .text_size(px(11.))
                        .text_align(TextAlign::Right),
                )
                .into_any_element()
        })
        .collect()
    }

    pub(crate) fn transform_field_views(&self, p: &Palette) -> Vec<AnyElement> {
        let Some(f) = &self.transform_fields else {
            return Vec::new();
        };
        [
            (t!("editor.transform.x"), &f.x),
            (t!("editor.transform.y"), &f.y),
            (t!("editor.transform.w"), &f.w),
            (t!("editor.transform.h"), &f.h),
            ("∠".into(), &f.angle),
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
    controls: Option<super::transform_controls::BoxControls>,
    ink: Hsla,
    window: &mut Window,
) {
    let mode = controls.map(|controls| controls.mode);
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
    let mids = if mode == Some(TransformControlMode::Rotate) {
        Vec::new()
    } else {
        mids
    };
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
            .border_color(ink)
            .corner_radii(if mode == Some(TransformControlMode::Rotate) {
                px(HANDLE_PX / 2.)
            } else {
                px(0.)
            }),
        );
    }
    let screen_quad = quad.map(|p| view.doc_to_screen(p, &bounds));
    let rotate = controls.and_then(|controls| {
        super::transform_controls::rotation_handle_with_obstacle(
            screen_quad,
            bounds,
            controls.side_handle,
            controls.obstacle,
        )
    });
    if mode.is_some()
        && let Some((mid, handle)) = rotate
    {
        let mut path = PathBuilder::stroke(px(1.));
        path.move_to(point(px(mid.0 as f32), px(mid.1 as f32)));
        path.line_to(point(px(handle.0 as f32), px(handle.1 as f32)));
        if let Ok(path) = path.build() {
            window.paint_path(path, ink);
        }
        window.paint_quad(
            fill(
                Bounds::new(
                    point(px(handle.0 as f32 - 5.), px(handle.1 as f32 - 5.)),
                    size(px(10.), px(10.)),
                ),
                gpui_kit::white(),
            )
            .border_widths(px(2.))
            .border_color(ink)
            .corner_radii(px(5.)),
        );
    }
}

#[cfg(test)]
mod sparse_frame_tests {
    use super::{local_corners, placement_command, raster_frame};
    use emulsion_core::{Command, Document, Node};
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
            let Command::SetPlacement { placement, .. } =
                placement_command(&Document::new(300, 200), &node, change)
            else {
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
        let Command::SetPlacement { placement, .. } =
            placement_command(&Document::new(300, 200), &node, frame)
        else {
            panic!()
        };
        assert!((placement.x - original.x).abs() < 1e-8);
        assert!((placement.y - original.y).abs() < 1e-8);
    }
}
