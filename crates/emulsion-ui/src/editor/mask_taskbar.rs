//! Contextual painting commands for the selected layer mask.
use super::*;
use gpui_kit::component::button::{Button, ButtonVariants};
use gpui_kit::component::{Disableable, Selectable, Sizable};

impl EditorView {
    pub(super) fn mask_taskbar_actions(&self, cx: &Context<Self>) -> Vec<AnyElement> {
        if !self.tools.mask_edit || self.tools.quick_mask {
            return Vec::new();
        }
        let Some(id) = self.selected.filter(|id| {
            self.editor
                .doc
                .node(*id)
                .is_some_and(|node| node.mask.is_some())
        }) else {
            return Vec::new();
        };
        let disabled = !self.layer_menu_ready() || self.editor.doc.locked_ancestor(id).is_some();
        let painting = self.tool == Tool::Mask
            || (self.tool == Tool::Brush && self.tools.paint == PaintKind::Brush);
        let viewing = self.mask_view.layer == Some(id);
        let mut actions = Vec::new();
        for (name, label, color) in [
            ("mask-add-paint", "Add to mask", [255; 4]),
            ("mask-subtract-paint", "Subtract from mask", [0, 0, 0, 255]),
        ] {
            actions.push(
                Button::new(name)
                    .label(label)
                    .small()
                    .ghost()
                    .selected(painting && self.tools.fg == color)
                    .disabled(disabled)
                    .tooltip(if color[0] == 255 {
                        "Paint white to reveal the layer"
                    } else {
                        "Paint black to hide the layer"
                    })
                    .on_click(cx.listener(move |this, _, window, cx| {
                        this.close_text_field(cx);
                        this.set_paint(PaintKind::Brush, cx);
                        this.set_mask_edit(true, cx);
                        this.set_fg(color, cx);
                        window.focus(&this.canvas_focus, cx);
                    }))
                    .into_any_element(),
            );
        }
        actions.push(
            Button::new("mask-taskbar-invert")
                .label("Invert mask")
                .small()
                .ghost()
                .disabled(disabled)
                .on_click(cx.listener(|this, _, window, cx| {
                    this.close_text_field(cx);
                    this.invert_mask(cx);
                    window.focus(&this.canvas_focus, cx);
                }))
                .into_any_element(),
        );
        actions.push(
            Button::new("mask-taskbar-view")
                .label("View mask")
                .small()
                .ghost()
                .selected(viewing)
                .disabled(!self.layer_menu_ready())
                .on_click(cx.listener(move |this, _, window, cx| {
                    this.mask_view.layer = if viewing { None } else { Some(id) };
                    window.focus(&this.canvas_focus, cx);
                    cx.notify();
                }))
                .into_any_element(),
        );
        actions
    }
}
