//! Component-specific native vector-mask actions. These never replace artwork
//! or its independent raster component.
use super::*;
use emulsion_core::{EmptyVectorCoverage, VectorMask};

type VectorMaskAction = fn(&mut EditorView, &mut Context<EditorView>);

impl EditorView {
    fn vector_mask_action_target(&self, geometry: bool) -> Option<NodeId> {
        let id = self.selected?;
        (self.layer_menu_ready()
            && !self.photo_transform_active()
            && self.mask_component_ready(id, MaskEditTarget::VectorMask, geometry))
        .then_some(id)
    }

    pub(crate) fn add_vector_mask(&mut self, hide_all: bool, cx: &mut Context<Self>) {
        if !self.layer_menu_ready() || !self.photo_transform_ready(cx) {
            return;
        }
        let Some(id) = self.selected else {
            return;
        };
        if self.editor.doc.locked_ancestor(id).is_some()
            || self.editor.doc.layer_locks(id).position
            || self
                .editor
                .doc
                .node(id)
                .is_none_or(|n| n.vector_mask.is_some())
        {
            return;
        }
        self.execute(
            Command::SetVectorMask {
                id,
                mask: Some(VectorMask::empty(if hide_all {
                    EmptyVectorCoverage::HideAll
                } else {
                    EmptyVectorCoverage::RevealAll
                })),
            },
            cx,
        );
        if self
            .editor
            .doc
            .node(id)
            .is_some_and(|n| n.vector_mask.is_some())
        {
            self.set_mask_edit_target(MaskEditTarget::VectorMask, cx);
        }
    }

    pub(crate) fn draw_vector_mask(&mut self, cx: &mut Context<Self>) {
        if !self.layer_menu_ready() || !self.photo_transform_ready(cx) {
            return;
        }
        if self
            .selected
            .and_then(|id| self.editor.doc.node(id))
            .is_some_and(|n| n.vector_mask.is_none())
        {
            self.add_vector_mask(false, cx);
        }
        self.edit_vector_mask(cx);
    }

    pub(crate) fn edit_vector_mask(&mut self, cx: &mut Context<Self>) {
        if self.vector_mask_action_target(true).is_none() {
            return;
        }
        self.set_mask_edit_target(MaskEditTarget::VectorMask, cx);
        self.set_tool(Tool::Pen, cx);
        self.tools.pen.mode = pen::PenMode::Pen;
    }

    pub(crate) fn close_vector_mask_path(&mut self, cx: &mut Context<Self>) {
        let Some(id) = self.vector_mask_action_target(true) else {
            return;
        };
        if self.tools.mask_edit_target != MaskEditTarget::VectorMask {
            self.edit_vector_mask(cx);
        }
        if let Some(sp) = &mut self.tools.pen.building {
            sp.closed = true;
            self.pen_finish(cx);
            return;
        }
        let mut path = (*self
            .editor
            .doc
            .node(id)
            .unwrap()
            .vector_mask
            .as_ref()
            .unwrap()
            .path)
            .clone();
        let si = self
            .tools
            .pen
            .selected
            .map(|(si, _)| si)
            .unwrap_or(path.subpaths.len().saturating_sub(1));
        if let Some(sp) = path.subpaths.get_mut(si) {
            sp.closed = true;
            self.execute(
                Command::SetVectorMaskPath {
                    id,
                    path: Arc::new(path),
                },
                cx,
            );
        }
    }

    pub(crate) fn toggle_vector_mask(&mut self, cx: &mut Context<Self>) {
        let Some(id) = self.vector_mask_action_target(false) else {
            return;
        };
        let enabled = !self
            .editor
            .doc
            .node(id)
            .unwrap()
            .vector_mask
            .as_ref()
            .unwrap()
            .enabled;
        self.execute(Command::SetVectorMaskEnabled { id, enabled }, cx);
    }
    pub(crate) fn invert_vector_mask(&mut self, cx: &mut Context<Self>) {
        let Some(id) = self.vector_mask_action_target(false) else {
            return;
        };
        let inverted = !self
            .editor
            .doc
            .node(id)
            .unwrap()
            .vector_mask
            .as_ref()
            .unwrap()
            .inverted;
        self.execute(Command::SetVectorMaskInverted { id, inverted }, cx);
    }
    pub(crate) fn toggle_vector_mask_link(&mut self, cx: &mut Context<Self>) {
        let Some(id) = self.vector_mask_action_target(true) else {
            return;
        };
        let linked = !self
            .editor
            .doc
            .node(id)
            .unwrap()
            .vector_mask
            .as_ref()
            .unwrap()
            .linked;
        self.execute(Command::SetVectorMaskLinked { id, linked }, cx);
    }
    pub(crate) fn remove_vector_mask(&mut self, cx: &mut Context<Self>) {
        let Some(id) = self.vector_mask_action_target(true) else {
            return;
        };
        self.finish_mask_properties();
        self.pen_cancel();
        self.execute(Command::SetVectorMask { id, mask: None }, cx);
        if self.tools.mask_edit_target == MaskEditTarget::VectorMask {
            self.set_mask_edit_target(MaskEditTarget::Content, cx);
        }
    }
    pub(crate) fn vector_mask_to_selection(&mut self, cx: &mut Context<Self>) {
        // Coverage inspection is read-only for the layer. A full/ancestor lock
        // must not prevent loading its independent coverage as a selection.
        if !self.layer_menu_ready() || self.photo_transform_active() {
            return;
        }
        let Some(id) = self.selected.filter(|id| {
            self.editor
                .doc
                .node(*id)
                .is_some_and(|n| n.vector_mask.is_some())
        }) else {
            return;
        };
        self.component_mask_to_selection(id, MaskEditTarget::VectorMask, cx);
    }
    pub(crate) fn rasterize_vector_mask(&mut self, cx: &mut Context<Self>) {
        let Some(id) = self.vector_mask_action_target(true) else {
            return;
        };
        if self.editor.doc.node(id).is_some_and(|n| n.mask.is_some()) {
            self.set_status(
                "Remove the raster mask before rasterizing this vector mask.",
                false,
                cx,
            );
            return;
        }
        self.finish_mask_properties();
        self.pen_cancel();
        self.execute(Command::RasterizeVectorMask { id }, cx);
        if self
            .editor
            .doc
            .node(id)
            .is_some_and(|n| n.vector_mask.is_none() && n.mask.is_some())
        {
            self.set_mask_edit_target(MaskEditTarget::RasterMask, cx);
        }
    }

    pub(super) fn select_layer_vector_mask(&mut self, id: NodeId, cx: &mut Context<Self>) {
        if !self.photo_transform_ready(cx) || !self.layer_menu_ready() {
            return;
        }
        if self
            .editor
            .doc
            .node(id)
            .is_none_or(|n| n.vector_mask.is_none())
        {
            return;
        }
        self.select_layer_row(id, false, false, cx);
        self.set_mask_edit_target(MaskEditTarget::VectorMask, cx);
        self.set_tool(Tool::Pen, cx);
    }
}

impl EditorView {
    pub(super) fn vector_mask_thumbnail(
        &mut self,
        id: NodeId,
        p: &Palette,
        cx: &mut Context<Self>,
    ) -> Option<AnyElement> {
        use gpui_kit::component::button::{Button, ButtonVariants};
        use gpui_kit::component::{Disableable, Sizable};
        let node = self.editor.doc.node(id)?.clone();
        let mask = node.vector_mask.as_ref()?;
        let enabled = mask.enabled;
        let linked = mask.linked;
        let coverage = self.editor.doc.vector_mask_for_inspection(&node)?;
        let image = self.mask_thumbnail(id, MaskEditTarget::VectorMask, &coverage);
        let active =
            self.selected == Some(id) && self.tools.mask_edit_target == MaskEditTarget::VectorMask;
        let locked = self.editor.doc.locked_ancestor(id).is_some();
        Some(div().flex().items_center().gap_1()
            .child(Button::new(("vector-mask-link", id)).label(if linked { "↔" } else { "·" }).xsmall().ghost()
                .disabled(locked || self.editor.doc.layer_locks(id).position)
                .tooltip(if linked { "Unlink vector mask" } else { "Link vector mask" })
                .on_click(cx.listener(move |this, _, _, cx| {
                    cx.stop_propagation();
                    if this.layer_menu_ready() { this.execute(Command::SetVectorMaskLinked { id, linked: !linked }, cx); }
                })))
            .child(div().id(("layer-vector-mask", id)).test_support().relative().flex_none().p_0p5().border_1()
                .border_color(if active { p.accent } else { p.line })
                .opacity(if enabled { 1. } else { 0.45 })
                .on_click(cx.listener(move |this, event: &ClickEvent, window, cx| {
                    cx.stop_propagation();
                    if !this.layer_menu_ready() || !this.photo_transform_ready(cx) { return; }
                    if event.modifiers().shift {
                        if !locked { this.execute(Command::SetVectorMaskEnabled { id, enabled: !enabled }, cx); }
                        return;
                    }
                    let target = (id, MaskEditTarget::VectorMask);
                    let show = event.modifiers().alt && this.mask_view.target != Some(target);
                    this.select_layer_vector_mask(id, cx);
                    this.mask_view.target = show.then_some(target);
                    window.focus(&this.panel_focus, cx);
                    cx.notify();
                }))
                .child(img(ImageSource::Render(image)).size_5().object_fit(ObjectFit::Contain))
                .child(div().absolute().bottom_0().right_0().text_size(px(8.)).text_color(p.accent).child("V"))
                .when(!enabled, |el| el.child(div().id(("layer-vector-mask-disabled", id)).test_support().absolute().inset_0().flex().items_center().justify_center().text_color(p.accent).text_lg().child("×")))
                .tooltip(|window, cx| gpui_kit::component::tooltip::Tooltip::new("Vector mask: click to edit; Alt to inspect; Shift to enable or disable").build(window, cx)))
            .into_any_element())
    }

    pub(super) fn vector_mask_controls(
        &mut self,
        id: NodeId,
        p: &Palette,
        cx: &mut Context<Self>,
    ) -> AnyElement {
        use gpui_kit::component::button::Button;
        use gpui_kit::component::{Disableable, Sizable};
        let Some(node) = self.editor.doc.node(id) else {
            return div().into_any_element();
        };
        let exists = node.vector_mask.is_some();
        let enabled = node.vector_mask.as_ref().is_some_and(|m| m.enabled);
        let linked = node.vector_mask.as_ref().is_some_and(|m| m.linked);
        let has_raster = node.mask.is_some();
        let locked = self.editor.doc.locked_ancestor(id).is_some() || !self.layer_menu_ready();
        let geometry_locked = locked || self.editor.doc.layer_locks(id).position;
        let mut controls = div()
            .id("vector-mask-controls")
            .test_support()
            .flex()
            .flex_col()
            .gap_2()
            .child(mono("Vector mask", 11., p.muted));
        let actions: Vec<(&str, &str, bool, VectorMaskAction)> = vec![
            (
                "vector-mask-reveal",
                "Reveal All",
                geometry_locked || exists,
                |e, cx| e.add_vector_mask(false, cx),
            ),
            (
                "vector-mask-hide",
                "Hide All",
                geometry_locked || exists,
                |e, cx| e.add_vector_mask(true, cx),
            ),
            (
                "vector-mask-draw",
                if exists {
                    "Edit / Append Path"
                } else {
                    "Draw Vector Mask"
                },
                geometry_locked,
                Self::draw_vector_mask,
            ),
            (
                "vector-mask-close",
                "Close Path",
                geometry_locked || !exists,
                Self::close_vector_mask_path,
            ),
            (
                "vector-mask-enable",
                if enabled { "Disable" } else { "Enable" },
                locked || !exists,
                Self::toggle_vector_mask,
            ),
            (
                "vector-mask-invert",
                "Invert",
                locked || !exists,
                Self::invert_vector_mask,
            ),
            (
                "vector-mask-link-action",
                if linked { "Unlink" } else { "Link" },
                geometry_locked || !exists,
                Self::toggle_vector_mask_link,
            ),
            (
                "vector-mask-selection",
                "To Selection",
                !self.layer_menu_ready() || !exists,
                Self::vector_mask_to_selection,
            ),
            (
                "vector-mask-remove",
                "Remove Vector Mask",
                geometry_locked || !exists,
                Self::remove_vector_mask,
            ),
            (
                "vector-mask-rasterize",
                "Rasterize Vector Mask",
                geometry_locked || !exists || has_raster,
                Self::rasterize_vector_mask,
            ),
        ];
        let mut grid = div().grid().grid_cols(2).gap_1();
        for (key, label, disabled, action) in actions {
            grid = grid.child(
                Button::new(key)
                    .label(label)
                    .small()
                    .outline()
                    .disabled(disabled)
                    .on_click(cx.listener(move |this, _, _, cx| action(this, cx))),
            );
        }
        controls = controls.child(grid);
        controls.into_any_element()
    }
}
