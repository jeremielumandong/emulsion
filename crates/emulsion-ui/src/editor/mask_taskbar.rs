//! Contextual painting commands for the selected layer mask.
use super::*;
use gpui_kit::component::button::{Button, ButtonVariants};
use gpui_kit::component::{Disableable, Selectable, Sizable};

impl EditorView {
    pub(super) fn mask_taskbar_actions(&mut self, cx: &mut Context<Self>) -> Vec<AnyElement> {
        if !self.tools.mask_edit_target.is_mask() || self.tools.quick_mask {
            return Vec::new();
        }
        let target = self.tools.mask_edit_target;
        let Some(id) = self.selected.filter(|id| {
            self.editor
                .doc
                .node(*id)
                .is_some_and(|node| target.exists(node))
        }) else {
            return Vec::new();
        };
        let disabled = !self.layer_menu_ready() || self.editor.doc.locked_ancestor(id).is_some();
        let painting = self.tool == Tool::Mask
            || (self.tool == Tool::Brush && self.tools.paint == PaintKind::Brush);
        let viewing = self.mask_view.target == Some((id, target));
        let mut actions = Vec::new();
        if target == MaskEditTarget::SmartFilterMask {
            let palette = self.workspace_palette(cx);
            actions
                .push(mono(t!("editor.filter_mask.title"), 11., palette.muted).into_any_element());
        }
        if matches!(
            target,
            MaskEditTarget::RasterMask | MaskEditTarget::SmartFilterMask
        ) {
            for (name, label, color) in [
                ("mask-add-paint", t!("editor.mask_taskbar.add"), [255; 4]),
                (
                    "mask-subtract-paint",
                    t!("editor.mask_taskbar.subtract"),
                    [0, 0, 0, 255],
                ),
            ] {
                actions.push(
                    Button::new(name)
                        .label(label)
                        .small()
                        .ghost()
                        .selected(painting && self.tools.fg == color)
                        .disabled(disabled)
                        .tooltip(if target == MaskEditTarget::SmartFilterMask {
                            if color[0] == 255 {
                                t!("editor.filter_mask.reveal_tip")
                            } else {
                                t!("editor.filter_mask.hide_tip")
                            }
                        } else if color[0] == 255 {
                            t!("editor.mask_taskbar.reveal_tip")
                        } else {
                            t!("editor.mask_taskbar.hide_tip")
                        })
                        .on_click(cx.listener(move |this, _, window, cx| {
                            this.close_text_field(cx);
                            this.set_paint(PaintKind::Brush, cx);
                            this.set_mask_edit_target(target, cx);
                            this.set_fg(color, cx);
                            window.focus(&this.canvas_focus, cx);
                        }))
                        .into_any_element(),
                );
            }
        }
        if target == MaskEditTarget::VectorMask {
            actions.push(
                Button::new("vector-mask-taskbar-edit")
                    .label("Edit / Append Path")
                    .small()
                    .ghost()
                    .disabled(disabled)
                    .on_click(cx.listener(|this, _, _, cx| this.edit_vector_mask(cx)))
                    .into_any_element(),
            );
            actions.push(
                Button::new("vector-mask-taskbar-close")
                    .label("Close Path")
                    .small()
                    .ghost()
                    .disabled(disabled)
                    .on_click(cx.listener(|this, _, _, cx| this.close_vector_mask_path(cx)))
                    .into_any_element(),
            );
        }
        actions.push(
            Button::new("mask-taskbar-invert")
                .label(t!("editor.mask_taskbar.invert"))
                .small()
                .ghost()
                .disabled(disabled)
                .on_click(cx.listener(move |this, _, window, cx| {
                    this.close_text_field(cx);
                    if target == MaskEditTarget::VectorMask {
                        this.invert_vector_mask(cx);
                    } else if target == MaskEditTarget::SmartFilterMask {
                        this.invert_smart_filter_mask(id, cx);
                    } else {
                        this.invert_mask(cx);
                    }
                    window.focus(&this.canvas_focus, cx);
                }))
                .into_any_element(),
        );
        actions.push(
            Button::new("mask-taskbar-view")
                .label(t!("editor.mask_taskbar.view"))
                .small()
                .ghost()
                .selected(viewing)
                .disabled(!self.layer_menu_ready())
                .on_click(cx.listener(move |this, _, window, cx| {
                    if !viewing && this.refuse_projective_tool("Mask inspection frame", cx) {
                        return;
                    }
                    this.mask_view.target = if viewing { None } else { Some((id, target)) };
                    window.focus(&this.canvas_focus, cx);
                    cx.notify();
                }))
                .into_any_element(),
        );
        let palette = self.workspace_palette(cx);
        actions.push(self.mask_property_controls(
            id,
            photo_masks::MaskControlSurface::Taskbar,
            &palette,
            cx,
        ));
        actions
    }
}
