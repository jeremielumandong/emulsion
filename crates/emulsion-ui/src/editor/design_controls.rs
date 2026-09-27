//! The handoff's canvas header and object controls, backed by native commands.
use super::design_ui::Section;
use super::*;
use emulsion_core::{
    command::{AlignTarget, Alignment},
    design::ImageFit,
};
use gpui_kit::component::{
    Disableable, Sizable,
    button::{Button, ButtonVariants},
};

impl EditorView {
    pub(super) fn design_canvas_toolbar(
        &self,
        p: &Palette,
        window: &Window,
        cx: &Context<Self>,
    ) -> Option<AnyElement> {
        if !self.is_design() {
            return None;
        }
        let compact = window.viewport_size().width < px(1100.);
        let name = self
            .editor
            .page_list()
            .iter()
            .find(|page| page.id == self.editor.active_page())
            .map(|page| page.name.clone())
            .unwrap_or_else(|| self.name.clone());
        let button = |id: &'static str, label: &'static str| {
            Button::new(id)
                .accessibility_label(label)
                .when(id == "design-resize", |button| {
                    button.child(rail::tool_icon("sparkles").size(px(11.)))
                })
                .child(div().text_size(px(11.)).child(label))
                .xsmall()
                .outline()
                .h(px(24.))
        };
        Some(
            div()
                .id("design-canvas-toolbar")
                .test_support()
                .flex()
                .items_center()
                .gap(px(8.))
                .h(px(38.))
                .flex_none()
                .min_w_0()
                .px(px(12.))
                .bg(p.panel)
                .border_b_1()
                .border_color(p.line)
                .when(!compact, |bar| {
                    bar.child(
                        div()
                            .max_w(px(240.))
                            .min_w_0()
                            .overflow_hidden()
                            .text_size(px(12.))
                            .font_weight(FontWeight::MEDIUM)
                            .child(name),
                    )
                    .child(
                        div()
                            .font_family(MONO_FONT)
                            .text_size(px(10.))
                            .text_color(p.muted)
                            .child(format!(
                                "{} × {}",
                                self.editor.doc.width, self.editor.doc.height
                            )),
                    )
                })
                .child(div().flex_1())
                .child(
                    div()
                        .flex()
                        .flex_none()
                        .items_center()
                        .gap(px(4.))
                        .child(
                            Button::new("design-undo")
                                .xsmall()
                                .outline()
                                .size(px(24.))
                                .tooltip("Undo")
                                .accessibility_label("Undo")
                                .disabled(!self.editor.can_undo())
                                .child(rail::tool_icon("undo-2").size(px(11.)))
                                .on_click(cx.listener(|this, _, _, cx| this.undo(cx))),
                        )
                        .child(
                            Button::new("design-redo")
                                .xsmall()
                                .outline()
                                .size(px(24.))
                                .tooltip("Redo")
                                .accessibility_label("Redo")
                                .disabled(!self.editor.can_redo())
                                .child(rail::tool_icon("redo-2").size(px(11.)))
                                .on_click(cx.listener(|this, _, _, cx| this.redo(cx))),
                        )
                        .child(button("design-position", "Position").on_click(cx.listener(
                            |this, _, _, cx| this.show_design_section(Section::Position, cx),
                        )))
                        .child(button("design-animate", "Animate").on_click(cx.listener(
                            |this, _, _, cx| this.show_design_section(Section::Motion, cx),
                        )))
                        .child(
                            button(
                                "design-resize",
                                if compact { "Resize" } else { "Magic resize" },
                            )
                            .tooltip("Copy and resize page using object anchors")
                            .on_click(cx.listener(
                                |this, _, window, cx| this.resize_variant_dialog(window, cx),
                            )),
                        ),
                )
                .into_any_element(),
        )
    }

    pub(super) fn design_position_controls(&self, p: &Palette, cx: &Context<Self>) -> AnyElement {
        let disabled = self.selected.is_none();
        div()
            .id("design-position-controls")
            .test_support()
            .flex()
            .flex_col()
            .gap_2()
            .text_size(px(11.))
            .child(div().text_color(p.muted).child("Align to page"))
            .child(
                div().grid().grid_cols(2).gap(px(6.)).children(
                    [
                        ("Left", Alignment::Left),
                        ("Right", Alignment::Right),
                        ("Top", Alignment::Top),
                        ("Bottom", Alignment::Bottom),
                        ("Centre X", Alignment::HorizontalCenter),
                        ("Centre Y", Alignment::VerticalCenter),
                    ]
                    .into_iter()
                    .enumerate()
                    .map(|(i, (label, alignment))| {
                        Button::new(("design-align", i))
                            .label(label)
                            .small()
                            .outline()
                            .disabled(disabled)
                            .on_click(cx.listener(move |this, _, _, cx| {
                                this.align_selected(alignment, AlignTarget::Canvas, cx)
                            }))
                    }),
                ),
            )
            .child(self.alignment_controls(p, cx))
            .child(div().text_color(p.muted).child("Layer order"))
            .child(
                div()
                    .flex()
                    .gap_1()
                    .child(
                        Button::new("design-backward")
                            .label("Backward")
                            .small()
                            .outline()
                            .disabled(disabled)
                            .on_click(cx.listener(|this, _, _, cx| this.shift_selected(false, cx))),
                    )
                    .child(
                        Button::new("design-forward")
                            .label("Forward")
                            .small()
                            .outline()
                            .disabled(disabled)
                            .on_click(cx.listener(|this, _, _, cx| this.shift_selected(true, cx))),
                    ),
            )
            .child(
                div()
                    .flex()
                    .gap_1()
                    .child(
                        Button::new("design-group")
                            .label("Group")
                            .small()
                            .outline()
                            .disabled(disabled)
                            .on_click(cx.listener(|this, _, _, cx| this.group_selected(cx))),
                    )
                    .child(
                        Button::new("design-ungroup")
                            .label("Ungroup")
                            .small()
                            .outline()
                            .disabled(disabled)
                            .on_click(cx.listener(|this, _, _, cx| this.ungroup_selected(cx))),
                    ),
            )
            .child(
                Button::new("design-duplicate")
                    .label("Duplicate")
                    .small()
                    .outline()
                    .disabled(disabled)
                    .on_click(cx.listener(|this, _, _, cx| this.duplicate_selected(cx))),
            )
            .child(
                Button::new("design-position-tools")
                    .label("All editing tools")
                    .small()
                    .ghost()
                    .on_click(
                        cx.listener(|this, _, _, cx| this.show_design_section(Section::Tools, cx)),
                    ),
            )
            .into_any_element()
    }

    fn fit_design_image(&mut self, fit: ImageFit, focus: [f64; 2], cx: &mut Context<Self>) {
        if !self.prepare_page_action(cx) {
            return;
        }
        let result = self
            .selected
            .ok_or_else(|| "Select a frame containing an image first.".to_owned())
            .and_then(|id| {
                emulsion_core::design::fit_frame_image(&self.editor.doc, id, fit, focus)
            });
        match result {
            Ok(command) => {
                self.execute(command, cx);
            }
            Err(error) => self.set_status(error, false, cx),
        }
    }

    pub(super) fn design_frame_controls(&self, p: &Palette, cx: &Context<Self>) -> AnyElement {
        let image = self
            .selected
            .and_then(|id| emulsion_core::design::frame_parts(&self.editor.doc, id))
            .and_then(|(_, image)| image);
        let disabled = image.is_none_or(|id| {
            self.editor.doc.locked_ancestor(id).is_some()
                || self.editor.doc.layer_locks(id).position
        });
        div().id("design-frame-controls").test_support().flex().flex_col().gap_2().text_size(px(11.))
            .child(div().text_color(p.muted).child("Image fit"))
            .child(div().flex().gap_1().children(ImageFit::ALL.into_iter().enumerate().map(|(i, fit)|
                Button::new(("design-frame-fit", i)).label(fit.label()).small().outline().disabled(disabled)
                    .on_click(cx.listener(move |this, _, _, cx| this.fit_design_image(fit, [0.5; 2], cx))))))
            .child(div().text_color(p.muted).child("Crop focus"))
            .child(div().grid().grid_cols(3).gap_1().children([
                ("Top left", "↖"), ("Top", "↑"), ("Top right", "↗"),
                ("Left", "←"), ("Centre", "•"), ("Right", "→"),
                ("Bottom left", "↙"), ("Bottom", "↓"), ("Bottom right", "↘"),
            ].into_iter().enumerate().map(|(i, (label, glyph))| Button::new(("design-frame-focus", i)).label(glyph).tooltip(format!("Cover frame, focus {label}")).accessibility_label(label).small().outline().disabled(disabled)
                .on_click(cx.listener(move |this, _, _, cx| this.fit_design_image(ImageFit::Cover, [(i % 3) as f64 / 2., (i / 3) as f64 / 2.], cx))))))
            .child(div().text_color(p.muted).child("Fit keeps rotation, flips and original pixels. Crop focus uses Cover; move the image for finer adjustment."))
            .into_any_element()
    }
}
