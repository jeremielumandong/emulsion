//! Moving editable artwork without accumulating mask resampling during a drag.
use super::*;
use emulsion_raster::IRect;

#[derive(Clone, Copy)]
pub(crate) struct MoveGesture {
    id: NodeId,
    start_doc: (f64, f64),
    bounds: IRect,
    revision: u64,
    delta: (f64, f64),
    mask_start: Option<(glam::DAffine2, [f64; 6])>,
}

impl EditorView {
    fn move_target_for(&self, id: NodeId) -> Result<IRect, &'static str> {
        let node = self
            .editor
            .doc
            .node(id)
            .ok_or("That layer no longer exists.")?;
        if self.editor.doc.locked_ancestor(id).is_some()
            || self.editor.doc.layer_locks(id).position
            || self
                .editor
                .doc
                .nodes
                .iter()
                .any(|n| (n.locked || n.locks.position) && self.editor.doc.is_ancestor(id, n.id))
        {
            return Err("That layer, its group, or a layer inside it is locked.");
        }
        if matches!(node.kind, NodeKind::Fill { .. } | NodeKind::Adjust(_)) && node.mask.is_none() {
            return Err("This layer covers the canvas. Add a mask to give it an area to move.");
        }
        let bounds = match &node.kind {
            NodeKind::Raster { raster, placement } => {
                Some(placement.doc_bounds(raster.width(), raster.height()))
            }
            NodeKind::Smart {
                source, placement, ..
            } => Some(placement.doc_bounds(source.width(), source.height())),
            _ => emulsion_core::geometry::node_bounds(&self.editor.doc, id).or_else(|| {
                // Hidden-by-mask content still has editable geometry and can move.
                let mut unmasked = self.editor.doc.clone();
                for node in &mut unmasked.nodes {
                    node.mask_enabled = false;
                }
                emulsion_core::geometry::node_bounds(&unmasked, id)
            }),
        }
        .ok_or("That layer or group has no content to move.")?;
        Ok(bounds)
    }

    fn move_target(&self) -> Result<(NodeId, IRect), &'static str> {
        if let Some(target) = self.mask_transform_target() {
            return Ok(target);
        }
        let id = self.selected.ok_or("Select a layer or group to move.")?;
        let mut bounds: Option<IRect> = None;
        for member in self.movement_layer_roots() {
            let rect = self.move_target_for(member)?;
            bounds = Some(bounds.map_or(rect, |bounds| bounds.union(&rect)));
        }
        Ok((id, bounds.ok_or("Select a layer or group to move.")?))
    }

    pub(super) fn begin_move(&mut self, point: (f64, f64), cx: &mut Context<Self>) {
        if self.assistant.running
            || self.editor.in_transaction()
            || self.drag.is_some()
            || self.warp.is_some()
        {
            self.set_status("Finish the current edit before moving artwork.", false, cx);
            return;
        }
        let (id, bounds) = match self.move_target() {
            Ok(target) => target,
            Err(message) => {
                self.set_status(message, false, cx);
                return;
            }
        };
        let mask_start = self
            .mask_transform_target()
            .and_then(|(id, _)| self.editor.doc.node(id))
            .map(|node| {
                (
                    emulsion_core::transform::local_to_document(node),
                    node.mask_transform,
                )
            });
        self.editor.begin(if mask_start.is_some() {
            "Move layer mask"
        } else {
            "Move"
        });
        self.layer_selection.move_ids = self.movement_layer_roots();
        self.drag = Some(Drag::Move(MoveGesture {
            id,
            start_doc: point,
            bounds,
            revision: self.editor.revision,
            delta: (0., 0.),
            mask_start,
        }));
        cx.notify();
    }

    pub(super) fn move_drag(
        &mut self,
        gesture: MoveGesture,
        point: (f64, f64),
        cx: &mut Context<Self>,
    ) {
        if self.editor.revision != gesture.revision || !self.editor.in_transaction() {
            // A keyboard edit may have changed the document during the drag.
            // Keep that work; never restore the old preview over it.
            self.drag = None;
            self.snap_lines.clear();
            self.editor.end();
            self.set_status(
                "Move finished because another edit changed the document.",
                false,
                cx,
            );
            return;
        }
        let mut delta = (point.0 - gesture.start_doc.0, point.1 - gesture.start_doc.1);
        if self.drag_shift {
            if delta.0.abs() >= delta.1.abs() {
                delta.1 = 0.;
            } else {
                delta.0 = 0.;
            }
        }
        delta = self.snap_node_move(gesture.id, gesture.bounds, delta.0, delta.1);
        if self.drag_shift {
            if (point.0 - gesture.start_doc.0).abs() >= (point.1 - gesture.start_doc.1).abs() {
                delta.1 = 0.;
                self.snap_lines.retain(|(vertical, _)| *vertical);
            } else {
                delta.0 = 0.;
                self.snap_lines.retain(|(vertical, _)| !*vertical);
            }
        }
        delta = (delta.0.round(), delta.1.round());
        if delta == gesture.delta {
            return;
        }
        let command = if let Some((local_to_doc, initial)) = gesture.mask_start {
            let delta = glam::DAffine2::from_translation(glam::dvec2(delta.0, delta.1));
            Command::SetMaskTransform {
                id: gesture.id,
                transform: (local_to_doc.inverse()
                    * delta
                    * local_to_doc
                    * glam::DAffine2::from_cols_array(&initial))
                .to_cols_array(),
            }
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

    pub(super) fn cancel_move(&mut self, cx: &mut Context<Self>) -> bool {
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
        if self.tool != Tool::Move {
            return;
        }
        if self.assistant.running
            || self.drag.is_some()
            || self.editor.in_transaction()
            || self.warp.is_some()
        {
            self.set_status("Finish the current edit before nudging artwork.", false, cx);
            return;
        }
        match self.move_target() {
            Ok(_) => {}
            Err(message) => {
                self.set_status(message, false, cx);
                return;
            }
        }
        self.snap_lines.clear();
        let command = self
            .mask_transform_command(glam::DAffine2::from_translation(glam::dvec2(dx, dy)))
            .unwrap_or_else(|| Command::TranslateNodes {
                ids: self.movement_layer_roots(),
                dx,
                dy,
            });
        self.execute(command, cx);
    }
}
