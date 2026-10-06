//! Moving editable artwork without accumulating mask resampling during a drag.
use super::*;
use emulsion_raster::IRect;

#[derive(Clone, Copy)]
pub(crate) struct MoveGesture {
    pub(super) toggle_controls: bool,
    id: NodeId,
    start_doc: (f64, f64),
    bounds: IRect,
    revision: u64,
    delta: (f64, f64),
    horizontal_axis: Option<bool>,
    mask_start: Option<(MaskEditTarget, glam::DAffine2, [f64; 6])>,
}

impl EditorView {
    /// Errors are catalog keys for the status line.
    fn move_target_for(&self, id: NodeId) -> Result<IRect, &'static str> {
        let node = self.editor.doc.node(id).ok_or("editor.movement.gone")?;
        if self.editor.doc.locked_ancestor(id).is_some()
            || self.editor.doc.layer_locks(id).position
            || self
                .editor
                .doc
                .nodes
                .iter()
                .any(|n| (n.locked || n.locks.position) && self.editor.doc.is_ancestor(id, n.id))
        {
            return Err("editor.movement.locked");
        }
        if matches!(node.kind, NodeKind::Fill { .. } | NodeKind::Adjust(_)) && !node.has_mask() {
            return Err("editor.movement.covers_canvas");
        }
        let bounds = match &node.kind {
            NodeKind::Raster { raster, placement } => {
                Some(placement.doc_bounds(raster.width(), raster.height()))
            }
            NodeKind::Smart {
                source, placement, ..
            } => Some(match placement {
                emulsion_core::SmartPlacement::Legacy(placement) => {
                    placement.doc_bounds(source.width(), source.height())
                }
                emulsion_core::SmartPlacement::Projective(_) => placement
                    .source_to_document((source.width(), source.height()))
                    .and_then(|mapping| {
                        mapping.bounds(emulsion_core::mapping::source_rect((
                            source.width(),
                            source.height(),
                        ))?)
                    })
                    .and_then(|bounds| bounds.to_irect())
                    .map_err(|_| "Projective source bounds are unavailable")?,
            }),
            _ => emulsion_core::geometry::node_bounds(&self.editor.doc, id)
                .map_err(|_| "Layer bounds are unavailable")?
                .or_else(|| {
                    // Hidden-by-mask content still has editable geometry and can move.
                    let mut unmasked = self.editor.doc.clone();
                    for node in &mut unmasked.nodes {
                        node.mask_enabled = false;
                        if let Some(mask) = &mut node.vector_mask {
                            mask.enabled = false;
                        }
                    }
                    emulsion_core::geometry::node_bounds(&unmasked, id)
                        .ok()
                        .flatten()
                }),
        }
        .ok_or("editor.movement.no_content")?;
        Ok(bounds)
    }

    fn move_target(&self) -> Result<(NodeId, IRect), &'static str> {
        if let Some(target) = self.mask_transform_target() {
            return Ok(target);
        }
        if self.tools.mask_edit_target.is_mask() {
            return Err("editor.movement.locked");
        }
        let id = self.selected.ok_or("editor.movement.select_layer")?;
        let mut bounds: Option<IRect> = None;
        for member in self.movement_layer_roots() {
            let rect = self.move_target_for(member)?;
            bounds = Some(bounds.map_or(rect, |bounds| bounds.union(&rect)));
        }
        Ok((id, bounds.ok_or("editor.movement.select_layer")?))
    }

    pub(super) fn begin_move(&mut self, point: (f64, f64), cx: &mut Context<Self>) {
        if self.tools.mask_edit_target.is_mask() && self.refuse_projective_tool("Mask movement", cx)
        {
            return;
        }
        if self.assistant.running
            || (self.editor.in_transaction() && !self.photo_transform_active())
            || self.drag.is_some()
            || self.warp.is_some()
        {
            self.set_status(t!("editor.movement.finish_move"), false, cx);
            return;
        }
        let (id, bounds) = match self.move_target() {
            Ok(target) => target,
            Err(message) => {
                self.set_status(t!(message), false, cx);
                return;
            }
        };
        let mask_start = if self.tools.mask_edit_target.is_mask() {
            let Some(node) = self.editor.doc.node(id) else {
                return;
            };
            let mapping = match super::transform::affine_tool_mapping(node) {
                Ok(mapping) => mapping,
                Err(error) => {
                    self.set_status(error.to_string(), true, cx);
                    return;
                }
            };
            let Some(affine) = self.tools.mask_edit_target.affine(node) else {
                self.set_status(
                    "The selected mask has no supported affine editing frame.",
                    true,
                    cx,
                );
                return;
            };
            Some((self.tools.mask_edit_target, mapping, affine))
        } else {
            None
        };
        self.photo_transform_begin_gesture();
        if !self.photo_transform_active() {
            self.editor.begin(if mask_start.is_some() {
                "Move layer mask"
            } else {
                "Move"
            });
        }
        self.layer_selection.move_ids = self.movement_layer_roots();
        self.drag = Some(Drag::Move(MoveGesture {
            toggle_controls: false,
            id,
            start_doc: point,
            bounds,
            revision: self.editor.revision,
            delta: (0., 0.),
            horizontal_axis: None,
            mask_start,
        }));
        cx.notify();
    }

    pub(super) fn move_drag(
        &mut self,
        mut gesture: MoveGesture,
        point: (f64, f64),
        cx: &mut Context<Self>,
    ) {
        if self.editor.revision != gesture.revision || !self.editor.in_transaction() {
            // A keyboard edit may have changed the document during the drag.
            // Keep that work; never restore the old preview over it.
            self.drag = None;
            self.snap_lines.clear();
            self.editor.end();
            self.set_status(t!("editor.movement.interrupted"), false, cx);
            return;
        }
        let mut delta = (point.0 - gesture.start_doc.0, point.1 - gesture.start_doc.1);
        if gesture.toggle_controls {
            if delta.0.hypot(delta.1) * self.view.zoom <= super::transform_controls::CLICK_SLOP_PX {
                self.drag = Some(Drag::Move(gesture));
                return;
            }
            gesture.toggle_controls = false;
        }
        if self.drag_shift {
            if gesture.horizontal_axis.is_none() {
                if delta.0.hypot(delta.1) * self.view.zoom < 3. {
                    return;
                }
                gesture.horizontal_axis = Some(delta.0.abs() >= delta.1.abs());
            }
        } else {
            gesture.horizontal_axis = None;
        }
        if let Some(horizontal) = gesture.horizontal_axis {
            if horizontal {
                delta.1 = 0.;
            } else {
                delta.0 = 0.;
            }
        }
        delta = self.snap_node_move(gesture.id, gesture.bounds, delta.0, delta.1);
        if let Some(horizontal) = gesture.horizontal_axis {
            if horizontal {
                delta.1 = 0.;
            } else {
                delta.0 = 0.;
            }
            self.snap_lines
                .retain(|(vertical, _)| *vertical == horizontal);
        }
        delta = (delta.0.round(), delta.1.round());
        if delta == gesture.delta {
            self.drag = Some(Drag::Move(gesture));
            return;
        }
        if self.photo_transform_active() {
            let transformed = self.photo_transform_gesture(
                glam::DAffine2::from_translation(glam::dvec2(delta.0, delta.1)),
                cx,
            );
            self.drag = Some(Drag::Move(MoveGesture {
                revision: self.editor.revision,
                delta: if transformed { delta } else { gesture.delta },
                ..gesture
            }));
            return;
        }
        let command = if let Some((target, local_to_doc, initial)) = gesture.mask_start {
            let delta = glam::DAffine2::from_translation(glam::dvec2(delta.0, delta.1));
            target.transform_command(
                gesture.id,
                if delta == glam::DAffine2::IDENTITY {
                    initial
                } else {
                    (local_to_doc.inverse()
                        * delta
                        * local_to_doc
                        * glam::DAffine2::from_cols_array(&initial))
                    .to_cols_array()
                },
            )
        } else {
            Command::TranslateNodes {
                ids: self.layer_selection.move_ids.clone(),
                dx: delta.0,
                dy: delta.1,
            }
        };
        match self.editor.preview(command) {
            Ok(_) => {
                self.status = None;
                self.drag = Some(Drag::Move(MoveGesture {
                    revision: self.editor.revision,
                    delta,
                    ..gesture
                }));
                self.after_change(cx);
            }
            Err(error) => self.set_status(error.to_string(), true, cx),
        }
    }

    /// Only a real, unmodified release completes a mode-switch click. Lost
    /// releases, focus changes and tool/selection changes merely retire it.
    pub(super) fn transform_click_released(&self, event: &MouseUpEvent) -> bool {
        let Some(Drag::Move(gesture)) = &self.drag else {
            return false;
        };
        if !gesture.toggle_controls
            || event.button != MouseButton::Left
            || event.modifiers.shift
            || event.modifiers.control
            || event.modifiers.alt
            || event.modifiers.platform
            || self.editor.revision != gesture.revision
            || !self
                .canvas_bounds()
                .is_some_and(|b| b.contains(&event.position))
        {
            return false;
        }
        self.doc_point(event.position).is_some_and(|point| {
            let point = self.layer_motion_point(point);
            (point.0 - gesture.start_doc.0).hypot(point.1 - gesture.start_doc.1) * self.view.zoom
                <= super::transform_controls::CLICK_SLOP_PX
        })
    }

    pub(super) fn cancel_move(&mut self, cx: &mut Context<Self>) -> bool {
        if self.photo_transform_active() {
            return false;
        }
        let Some(Drag::Move(gesture)) = self.drag else {
            return false;
        };
        self.drag = None;
        self.snap_lines.clear();
        if self.editor.revision == gesture.revision {
            self.editor.cancel();
            self.after_change(cx);
        } else {
            self.editor.end();
            cx.notify();
        }
        true
    }

    pub fn nudge_selected(&mut self, dx: f64, dy: f64, cx: &mut Context<Self>) {
        if self.tools.mask_edit_target.is_mask() && self.refuse_projective_tool("Mask nudge", cx) {
            return;
        }
        if self.tool != Tool::Move {
            return;
        }
        if self.assistant.running
            || self.drag.is_some()
            || (self.editor.in_transaction() && !self.photo_transform_active())
            || self.warp.is_some()
        {
            self.set_status(t!("editor.movement.finish_nudge"), false, cx);
            return;
        }
        match self.move_target() {
            Ok(_) => {}
            Err(message) => {
                self.set_status(t!(message), false, cx);
                return;
            }
        }
        self.snap_lines.clear();
        if self.photo_transform_active() {
            self.photo_transform_delta(glam::DAffine2::from_translation(glam::dvec2(dx, dy)), cx);
            return;
        }
        let command = if self.tools.mask_edit_target.is_mask() {
            let Some(command) =
                self.mask_transform_command(glam::DAffine2::from_translation(glam::dvec2(dx, dy)))
            else {
                self.set_status(
                    "The selected mask has no supported affine editing frame.",
                    true,
                    cx,
                );
                return;
            };
            command
        } else {
            Command::TranslateNodes {
                ids: self.movement_layer_roots(),
                dx,
                dy,
            }
        };
        self.execute(command, cx);
    }
}
