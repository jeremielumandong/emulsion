//! Rotation for selected drawing objects, independent of the active tool.

use super::*;
use gpui_kit::component::Sizable as _;

pub(crate) struct RotationFields {
    node: NodeId,
    angle: Entity<InputState>,
    _sub: Subscription,
}

impl EditorView {
    pub(crate) fn rotates_photo_canvas(&self, id: NodeId) -> bool {
        if self.draw_mode
            || self.editor.doc.selection.is_some()
            || self.selected_layer_ids().len() > 1
            || self.mask_transform_target().is_some()
        {
            return false;
        }
        let doc = &self.editor.doc;
        if doc.raw.as_ref().is_some_and(|raw| raw.node_id == id) {
            return true;
        }
        doc.nodes.len() == 1
            && doc
                .node(id)
                .is_some_and(|node| matches!(node.kind, NodeKind::Raster { .. }))
            && emulsion_core::geometry::node_bounds(doc, id)
                == Some(emulsion_raster::IRect::new(
                    0,
                    0,
                    doc.width as i32,
                    doc.height as i32,
                ))
    }

    pub(crate) fn sync_rotation_fields(&mut self, window: &mut Window, cx: &mut Context<Self>) {
        let Some(id) = self
            .selected
            .filter(|id| self.editor.doc.node(*id).is_some())
        else {
            self.rotation_fields = None;
            return;
        };
        if self.rotation_fields.as_ref().is_some_and(|f| f.node == id) {
            return;
        }
        let angle = cx.new(|cx| InputState::new(window, cx).default_value("15"));
        let sub = cx.subscribe_in(&angle, window, |this, _, ev: &InputEvent, _, cx| {
            if matches!(ev, InputEvent::PressEnter { .. }) {
                this.apply_node_rotation(cx);
            }
        });
        self.rotation_fields = Some(RotationFields {
            node: id,
            angle,
            _sub: sub,
        });
    }

    fn apply_node_rotation(&mut self, cx: &mut Context<Self>) {
        let Some(fields) = &self.rotation_fields else {
            return;
        };
        if self.selected != Some(fields.node) {
            return;
        }
        let degrees = fields
            .angle
            .read(cx)
            .value()
            .trim()
            .trim_end_matches('°')
            .parse::<f64>()
            .ok()
            .filter(|angle| angle.is_finite());
        match degrees {
            Some(degrees) => self.rotate_selected_node(degrees, cx),
            None => self.set_status("Enter a rotation angle in degrees.", true, cx),
        }
    }

    pub(crate) fn rotate_selected_node(&mut self, degrees: f64, cx: &mut Context<Self>) {
        let Some(id) = self.selected else { return };
        if self.warp.is_some() {
            self.set_status(
                "Apply or cancel the warp before rotating this layer.",
                false,
                cx,
            );
            return;
        }
        if self.assistant.running || self.drag.is_some() {
            self.set_status(
                "Finish the current drawing before rotating this layer.",
                false,
                cx,
            );
            return;
        }
        if degrees.is_finite() && degrees.rem_euclid(360.0).abs() < 1e-9 {
            return;
        }
        self.close_text_field(cx);
        if self.rotates_photo_canvas(id) {
            self.execute(Command::RotateImage { degrees }, cx);
            self.fit_pending = true;
        } else {
            self.execute(Command::RotateNode { id, degrees }, cx);
        }
    }

    pub(crate) fn rotation_controls(
        &self,
        p: &Palette,
        cx: &mut Context<Self>,
    ) -> Option<AnyElement> {
        let fields = self.rotation_fields.as_ref()?;
        let node = self.editor.doc.node(fields.node)?;
        // Whole-canvas colour/adjustment nodes have no spatial object to turn.
        // Masked shapes do, and keep the same controls as paths and pixels.
        if matches!(node.kind, NodeKind::Adjust(_) | NodeKind::Fill { .. }) && node.mask.is_none() {
            return None;
        }
        if self.selected != Some(fields.node) {
            return None;
        }
        Some(
            div()
                .flex()
                .flex_col()
                .gap(px(6.))
                .child(mono(
                    if self.rotates_photo_canvas(fields.node) {
                        "ROTATE IMAGE"
                    } else {
                        "ROTATE OBJECT"
                    },
                    9.5,
                    p.muted,
                ))
                .when(node.locked, |d| {
                    d.child(mono("Unlock this layer to rotate it.", 10., p.muted))
                })
                .when(!node.locked, |d| {
                    d.child(
                        div()
                            .flex()
                            .items_center()
                            .gap(px(6.))
                            .child(mono("by", 10., p.muted))
                            .child(div().w(px(58.)).child(Input::new(&fields.angle).small()))
                            .child(mono("°", 10., p.muted))
                            .child(chip("node-rotate-apply", "Apply", false, p).on_click(
                                cx.listener(|this, _, _, cx| this.apply_node_rotation(cx)),
                            )),
                    )
                    .child(
                        div()
                            .flex()
                            .gap(px(6.))
                            .child(chip("node-rotate-left", "↶ 90°", false, p).on_click(
                                cx.listener(|this, _, _, cx| this.rotate_selected_node(-90.0, cx)),
                            ))
                            .child(chip("node-rotate-right", "↷ 90°", false, p).on_click(
                                cx.listener(|this, _, _, cx| this.rotate_selected_node(90.0, cx)),
                            )),
                    )
                })
                .into_any_element(),
        )
    }
}
