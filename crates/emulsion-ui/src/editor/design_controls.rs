//! The handoff's canvas header and object controls, backed by native commands.
use super::design_ui::Section;
use super::*;
use emulsion_core::{
    command::{AlignTarget, Alignment},
    design::ImageFit,
};
use gpui_kit::component::{
    Disableable, Sizable, WindowExt,
    button::{Button, ButtonVariants},
    menu::{PopupMenu, PopupMenuItem},
};

impl EditorView {
    pub(super) fn copy_design_appearance(&mut self, cx: &mut Context<Self>) {
        if !self.prepare_page_action(cx) {
            return;
        }
        if self.selected_layer_roots().len() != 1 {
            self.set_status("Select one object to copy its style.", true, cx);
            return;
        }
        if let Some(node) = self.selected.and_then(|id| self.editor.doc.node(id)) {
            self.design_ui.copied_appearance =
                Some(emulsion_core::design_appearance::Appearance::capture(node));
            self.set_status(
                "Style copied. Select objects and choose Paste style.",
                false,
                cx,
            );
        }
    }

    pub(super) fn paste_design_appearance(&mut self, cx: &mut Context<Self>) {
        if !self.prepare_page_action(cx) {
            return;
        }
        let Some(appearance) = &self.design_ui.copied_appearance else {
            return;
        };
        let commands = self
            .selected_layer_roots()
            .iter()
            .filter_map(|id| self.editor.doc.node(*id))
            .flat_map(|node| appearance.commands(node))
            .collect();
        self.execute_layer_commands("Paste style", commands, cx);
    }

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
                    button.child(rail::tool_icon("sparkles").text_color(p.ink).size(px(11.)))
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
                .child(
                    Button::new("design-select")
                        .xsmall()
                        .outline()
                        .size(px(24.))
                        .tooltip("Select objects (V); double-click text to edit")
                        .accessibility_label("Select objects")
                        .when(self.tool == Tool::Move, |button| button.bg(p.accent))
                        .child(
                            rail::tool_icon("mouse-pointer-2")
                                .text_color(if self.tool == Tool::Move {
                                    p.accent_fg
                                } else {
                                    p.ink
                                })
                                .size(px(12.)),
                        )
                        .on_click(cx.listener(|this, _, _, cx| this.set_tool(Tool::Move, cx))),
                )
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
                                .child(rail::tool_icon("undo-2").text_color(p.ink).size(px(11.)))
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
                                .child(rail::tool_icon("redo-2").text_color(p.ink).size(px(11.)))
                                .on_click(cx.listener(|this, _, _, cx| this.redo(cx))),
                        )
                        .child(
                            Button::new("design-inspector-toggle")
                                .accessibility_label("Layers and properties")
                                .tooltip("Layers and properties")
                                .xsmall()
                                .outline()
                                .size(px(24.))
                                .child(
                                    rail::tool_icon("sliders-horizontal")
                                        .text_color(p.ink)
                                        .size(px(11.)),
                                )
                                .on_click(cx.listener(|this, _, _, cx| {
                                    this.design_ui.inspector = !this.design_ui.inspector;
                                    cx.notify();
                                })),
                        )
                        .child(button("design-position", "Position").on_click(cx.listener(
                            |this, _, _, cx| this.show_design_section(Section::Position, cx),
                        )))
                        .child(button("design-animate", "Animate").on_click(cx.listener(
                            |this, _, _, cx| this.show_design_section(Section::Motion, cx),
                        )))
                        .child(
                            button("design-present-now", "Present").on_click(cx.listener(
                                |this, _, window, cx| {
                                    this.start_motion(true, cx);
                                    window.focus(&this.canvas_focus, cx);
                                },
                            )),
                        )
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
            .child(
                Button::new("design-precision-open")
                    .label("Rulers, units and spacing…")
                    .small()
                    .outline()
                    .on_click(
                        cx.listener(|this, _, window, cx| this.show_design_precision(window, cx)),
                    ),
            )
            .child(div().text_color(p.muted).child("Arrange · layer order"))
            .child(
                div().grid().grid_cols(2).gap(px(6.)).children(
                    [
                        ("design-front", "Bring to front", true, true),
                        ("design-back", "Send to back", false, true),
                        ("design-forward", "Bring forward", true, false),
                        ("design-backward", "Send backward", false, false),
                    ]
                    .into_iter()
                    .map(|(id, label, up, end)| {
                        Button::new(id)
                            .label(label)
                            .small()
                            .outline()
                            .disabled(disabled)
                            .on_click(cx.listener(move |this, _, _, cx| {
                                if !this.prepare_page_action(cx) {
                                    return;
                                }
                                if end {
                                    this.shift_selected_to_end(up, cx);
                                } else {
                                    this.shift_selected(up, cx);
                                }
                            }))
                    }),
                ),
            )
            .child(
                Button::new("design-layer-index")
                    .label("Set layer index…")
                    .small()
                    .outline()
                    .disabled(self.selected_layer_roots().len() != 1)
                    .on_click(cx.listener(|this, _, window, cx| {
                        this.design_layer_index_dialog(window, cx)
                    })),
            )
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
            .child(
                div()
                    .flex()
                    .gap_1()
                    .child(
                        Button::new("design-copy-style")
                            .label("Copy style")
                            .small()
                            .outline()
                            .disabled(self.selected_layer_roots().len() != 1)
                            .on_click(
                                cx.listener(|this, _, _, cx| this.copy_design_appearance(cx)),
                            ),
                    )
                    .child(
                        Button::new("design-paste-style")
                            .label("Paste style")
                            .small()
                            .outline()
                            .disabled(disabled || self.design_ui.copied_appearance.is_none())
                            .on_click(
                                cx.listener(|this, _, _, cx| this.paste_design_appearance(cx)),
                            ),
                    ),
            )
            .child(self.design_layout_controls(p, cx))
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

    pub(super) fn design_arrange_menu(
        menu: PopupMenu,
        editor: &Entity<Self>,
        window: &mut Window,
        cx: &mut Context<PopupMenu>,
    ) -> PopupMenu {
        let view = editor.read(cx);
        let ids = view.selected_layer_roots();
        let disabled = ids.is_empty() || view.drag.is_some() || view.editor.in_transaction();
        let single = ids.len() == 1 && !disabled;
        let focus = view.canvas_focus.clone();
        let owner = editor.downgrade();
        menu.separator()
            .submenu("Arrange", window, cx, move |menu, _, _| {
                menu.action_context(focus.clone())
                    .menu_with_disabled(
                        "Bring to front",
                        Box::new(crate::actions::BringToFront),
                        disabled,
                    )
                    .menu_with_disabled(
                        "Send to back",
                        Box::new(crate::actions::SendToBack),
                        disabled,
                    )
                    .menu_with_disabled(
                        "Bring forward",
                        Box::new(crate::actions::MoveNodeUp),
                        disabled,
                    )
                    .menu_with_disabled(
                        "Send backward",
                        Box::new(crate::actions::MoveNodeDown),
                        disabled,
                    )
                    .separator()
                    .item(
                        PopupMenuItem::new("Set layer index…")
                            .disabled(!single)
                            .on_click({
                                let owner = owner.clone();
                                move |_, window, cx| {
                                    owner
                                        .update(cx, |view, cx| {
                                            view.design_layer_index_dialog(window, cx)
                                        })
                                        .ok();
                                }
                            }),
                    )
            })
    }

    pub(super) fn design_layer_index_dialog(
        &mut self,
        window: &mut Window,
        cx: &mut Context<Self>,
    ) {
        if !self.prepare_page_action(cx) {
            return;
        }
        let ids = self.selected_layer_roots();
        let [id] = ids.as_slice() else { return };
        let id = *id;
        let Some(node) = self.editor.doc.node(id) else {
            return;
        };
        let parent = node.parent;
        let siblings = self.editor.doc.children(parent);
        let count = siblings.len();
        let Some(index) = siblings.iter().position(|&other| other == id) else {
            return;
        };
        let input = cx.new(|cx| InputState::new(window, cx).default_value((index + 1).to_string()));
        let owner = cx.weak_entity();
        let ticket = self.edit_ticket();
        let stamp = self.editor.stamp();
        let error = cx.new(|_| String::new());
        window.open_dialog(cx, move |dialog, _, cx| {
            let input = input.clone();
            let owner = owner.clone();
            let error_apply = error.clone();
            let stamp = stamp.clone();
            dialog.title("Set layer index").width(px(400.))
                .child(format!("Position among {count} objects in this group or page. 1 is the back; {count} is the front."))
                .child(Input::new(&input).id("design-layer-index-value"))
                .when(!error.read(cx).is_empty(), |dialog| dialog.child(error.read(cx).clone()))
                .footer(crate::widgets::form_dialog_footer("Apply"))
                .on_ok(move |_, window, cx| {
                    let index = input.read(cx).value().trim().parse::<usize>().ok()
                        .filter(|index| (1..=count).contains(index));
                    let result = index.ok_or_else(|| format!("Enter a whole number from 1 to {count}."))
                        .and_then(|index| owner.update(cx, |this, cx| {
                            if this.edit_ticket() != ticket || this.editor.stamp() != stamp
                                || this.selected_layer_roots() != [id] {
                                return Err("The selection or page changed. Reopen Set layer index.".into());
                            }
                            if this.editor.doc.children(parent).get(index - 1) == Some(&id) {
                                return Ok(());
                            }
                            let command = Command::MoveNode { id, slot: Slot { parent, index: index - 1 } };
                            command.clone().apply(&mut this.editor.doc.clone()).map_err(|e| e.to_string())?;
                            this.execute_layer_commands("Set layer index", vec![command], cx)
                                .ok_or_else(|| "Could not reorder this object.".to_string())?;
                            Ok(())
                        }).unwrap_or_else(|_| Err("The editor closed.".into())));
                    match result {
                        Ok(()) => true,
                        Err(message) => {
                            error_apply.update(cx, |error, cx| { *error = message; cx.notify(); });
                            window.refresh();
                            false
                        }
                    }
                })
        });
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
