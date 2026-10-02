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
    menu::PopupMenu,
};

/// Display name for an image fit; `ImageFit::label()` stays the English id.
fn fit_label(fit: ImageFit) -> SharedString {
    match fit {
        ImageFit::Cover => t!("editor.design_controls.fit_cover"),
        ImageFit::Contain => t!("editor.design_controls.fit_contain"),
        ImageFit::Stretch => t!("editor.design_controls.fit_stretch"),
    }
    .into()
}

impl EditorView {
    pub(super) fn copy_design_appearance(&mut self, cx: &mut Context<Self>) {
        if !self.prepare_page_action(cx) {
            return;
        }
        if self.selected_layer_roots().len() != 1 {
            self.set_status(t!("editor.design_controls.copy_style_select"), true, cx);
            return;
        }
        if let Some(node) = self.selected.and_then(|id| self.editor.doc.node(id)) {
            self.design_ui.copied_appearance =
                Some(emulsion_core::design_appearance::Appearance::capture(node));
            self.set_status(t!("editor.design_controls.style_copied"), false, cx);
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
        let button = |id: &'static str, label: SharedString| {
            Button::new(id)
                .accessibility_label(label.clone())
                .when(id == "design-resize", |button| {
                    button.child(rail::tool_icon("sparkles").text_color(p.ink).size(px(11.)))
                })
                .child(div().text_size(px(11.)).child(label))
                .xsmall()
                .outline()
                .h(px(24.))
        };
        let tool_button = |id: &'static str,
                           icon: &'static str,
                           label: SharedString,
                           tooltip: SharedString,
                           active: bool| {
            Button::new(id)
                .xsmall()
                .outline()
                .size(px(24.))
                .tooltip(tooltip)
                .accessibility_label(label)
                .when(active, |button| button.bg(p.accent))
                .child(
                    rail::tool_icon(icon)
                        .text_color(if active { p.accent_fg } else { p.ink })
                        .size(px(12.)),
                )
        };
        let hand = self.tool == Tool::Hand && !self.tools.rotate_view;
        // Keep the labeled editing actions inside the narrow canvas toolbar.
        // Present has a familiar play icon and retains its accessible name.
        let present = if compact {
            tool_button(
                "design-present-now",
                "play",
                t!("editor.design_controls.present").into(),
                t!("editor.design_controls.present").into(),
                false,
            )
        } else {
            button(
                "design-present-now",
                t!("editor.design_controls.present").into(),
            )
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
                    div()
                        .flex()
                        .flex_none()
                        .items_center()
                        .gap(px(4.))
                        .child(
                            tool_button(
                                "design-select",
                                "mouse-pointer-2",
                                t!("editor.design_controls.select_objects").into(),
                                t!("editor.design_controls.select_objects_tip").into(),
                                self.tool == Tool::Move,
                            )
                            .on_click(cx.listener(|this, _, _, cx| this.set_tool(Tool::Move, cx))),
                        )
                        .child(
                            tool_button(
                                "design-hand",
                                "hand",
                                t!("editor.design_controls.hand").into(),
                                t!("editor.design_controls.hand_tip").into(),
                                hand,
                            )
                            .on_click(cx.listener(|this, _, _, cx| this.set_hand_mode(false, cx))),
                        ),
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
                                .tooltip(t!("edit.undo"))
                                .accessibility_label(t!("edit.undo"))
                                .disabled(!self.editor.can_undo())
                                .child(rail::tool_icon("undo-2").text_color(p.ink).size(px(11.)))
                                .on_click(cx.listener(|this, _, _, cx| this.undo(cx))),
                        )
                        .child(
                            Button::new("design-redo")
                                .xsmall()
                                .outline()
                                .size(px(24.))
                                .tooltip(t!("edit.redo"))
                                .accessibility_label(t!("edit.redo"))
                                .disabled(!self.editor.can_redo())
                                .child(rail::tool_icon("redo-2").text_color(p.ink).size(px(11.)))
                                .on_click(cx.listener(|this, _, _, cx| this.redo(cx))),
                        )
                        .child(
                            Button::new("design-inspector-toggle")
                                .accessibility_label(t!("editor.design_controls.layers_properties"))
                                .tooltip(t!("editor.design_controls.layers_properties"))
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
                        .child(
                            button(
                                "design-position",
                                t!("editor.design_controls.position").into(),
                            )
                            .on_click(cx.listener(|this, _, _, cx| {
                                this.show_design_section(Section::Position, cx)
                            })),
                        )
                        .child(
                            button(
                                "design-animate",
                                t!("editor.design_controls.animate").into(),
                            )
                            .on_click(cx.listener(|this, _, _, cx| {
                                this.show_design_section(Section::Motion, cx)
                            })),
                        )
                        .child(present.on_click(cx.listener(|this, _, window, cx| {
                            this.start_motion(true, cx);
                            window.focus(&this.canvas_focus, cx);
                        })))
                        .child(
                            button("design-resize", t!("design.resize.open").to_string().into())
                                .tooltip(t!("design.resize.uses_anchors").to_string())
                                .on_click(cx.listener(|this, _, window, cx| {
                                    this.resize_variant_dialog(window, cx)
                                })),
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
                    .label(t!("editor.design_controls.rulers"))
                    .small()
                    .outline()
                    .on_click(
                        cx.listener(|this, _, window, cx| this.show_design_precision(window, cx)),
                    ),
            )
            .child(
                div()
                    .text_color(p.muted)
                    .child(t!("editor.design_controls.arrange_order")),
            )
            .child(
                div().grid().grid_cols(2).gap(px(6.)).children(
                    [
                        ("design-front", t!("design.direct.front"), true, true),
                        ("design-back", t!("design.direct.back"), false, true),
                        ("design-forward", t!("design.direct.forward"), true, false),
                        (
                            "design-backward",
                            t!("design.direct.backward"),
                            false,
                            false,
                        ),
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
                    .label(t!("editor.design_controls.set_layer_index_menu"))
                    .small()
                    .outline()
                    .disabled(self.selected_layer_roots().len() != 1)
                    .on_click(cx.listener(|this, _, window, cx| {
                        this.design_layer_index_dialog(window, cx)
                    })),
            )
            .child(
                div()
                    .text_color(p.muted)
                    .child(t!("editor.design_controls.align_page")),
            )
            .child(
                div().grid().grid_cols(2).gap(px(6.)).children(
                    [
                        (t!("editor.design_editor.align_left"), Alignment::Left),
                        (t!("editor.design_editor.align_right"), Alignment::Right),
                        (t!("editor.design_controls.top"), Alignment::Top),
                        (t!("editor.design_controls.bottom"), Alignment::Bottom),
                        (
                            t!("editor.design_controls.centre_x"),
                            Alignment::HorizontalCenter,
                        ),
                        (
                            t!("editor.design_controls.centre_y"),
                            Alignment::VerticalCenter,
                        ),
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
                            .label(t!("editor.design_controls.copy_style"))
                            .small()
                            .outline()
                            .disabled(self.selected_layer_roots().len() != 1)
                            .on_click(
                                cx.listener(|this, _, _, cx| this.copy_design_appearance(cx)),
                            ),
                    )
                    .child(
                        Button::new("design-paste-style")
                            .label(t!("editor.design_controls.paste_style"))
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
                            .label(t!("design.direct.group"))
                            .small()
                            .outline()
                            .disabled(disabled)
                            .on_click(cx.listener(|this, _, _, cx| this.group_selected(cx))),
                    )
                    .child(
                        Button::new("design-ungroup")
                            .label(t!("design.direct.ungroup"))
                            .small()
                            .outline()
                            .disabled(disabled)
                            .on_click(cx.listener(|this, _, _, cx| this.ungroup_selected(cx))),
                    ),
            )
            .child(
                Button::new("design-duplicate")
                    .label(t!("design.direct.duplicate"))
                    .small()
                    .outline()
                    .disabled(disabled)
                    .on_click(cx.listener(|this, _, _, cx| this.duplicate_selected(cx))),
            )
            .child(
                Button::new("design-position-tools")
                    .label(t!("editor.design_controls.all_tools"))
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
        let editor = editor.clone();
        menu.separator().submenu(
            t!("design.direct.arrange").to_string(),
            window,
            cx,
            move |menu, _, cx| Self::design_arrange_items(menu, &editor, cx),
        )
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
        let offset = if parent.is_none() {
            emulsion_core::design_background::foreground_start(&self.editor.doc)
        } else {
            0
        };
        let count = siblings.len().saturating_sub(offset);
        let Some(index) = siblings.iter().position(|&other| other == id) else {
            return;
        };
        let Some(index) = index.checked_sub(offset) else {
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
            dialog
                .title(t!("editor.design_controls.set_layer_index"))
                .width(px(400.))
                .child(t!("editor.design_controls.layer_index_body", count = count))
                .child(Input::new(&input).id("design-layer-index-value"))
                .when(!error.read(cx).is_empty(), |dialog| {
                    dialog.child(error.read(cx).clone())
                })
                .footer(crate::widgets::form_dialog_footer(t!(
                    "editor.design_controls.apply"
                )))
                .on_ok(move |_, window, cx| {
                    let index = input
                        .read(cx)
                        .value()
                        .trim()
                        .parse::<usize>()
                        .ok()
                        .filter(|index| (1..=count).contains(index));
                    let result = index
                        .ok_or_else(|| {
                            t!("editor.design_controls.whole_number", count = count).into_owned()
                        })
                        .and_then(|index| {
                            owner
                                .update(cx, |this, cx| {
                                    if this.edit_ticket() != ticket
                                        || this.editor.stamp() != stamp
                                        || this.selected_layer_roots() != [id]
                                    {
                                        return Err(t!("editor.design_controls.selection_changed")
                                            .into_owned());
                                    }
                                    if this.editor.doc.children(parent).get(index - 1 + offset)
                                        == Some(&id)
                                    {
                                        return Ok(());
                                    }
                                    let command = Command::MoveNode {
                                        id,
                                        slot: Slot {
                                            parent,
                                            index: index - 1 + offset,
                                        },
                                    };
                                    command
                                        .clone()
                                        .apply(&mut this.editor.doc.clone())
                                        .map_err(|e| e.to_string())?;
                                    this.execute_layer_commands(
                                        "Set layer index",
                                        vec![command],
                                        cx,
                                    )
                                    .ok_or_else(|| {
                                        t!("editor.design_controls.reorder_failed").into_owned()
                                    })?;
                                    Ok(())
                                })
                                .unwrap_or_else(|_| {
                                    Err(t!("editor.design_controls.editor_closed").into_owned())
                                })
                        });
                    match result {
                        Ok(()) => true,
                        Err(message) => {
                            error_apply.update(cx, |error, cx| {
                                *error = message;
                                cx.notify();
                            });
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
            .ok_or_else(|| t!("editor.design_controls.select_frame_image").into_owned())
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
        div()
            .id("design-frame-controls")
            .test_support()
            .flex()
            .flex_col()
            .gap_2()
            .text_size(px(11.))
            .child(
                div()
                    .text_color(p.muted)
                    .child(t!("editor.design_controls.image_fit")),
            )
            .child(
                div()
                    .flex()
                    .gap_1()
                    .children(ImageFit::ALL.into_iter().enumerate().map(|(i, fit)| {
                        Button::new(("design-frame-fit", i))
                            .label(fit_label(fit))
                            .small()
                            .outline()
                            .disabled(disabled)
                            .on_click(cx.listener(move |this, _, _, cx| {
                                this.fit_design_image(fit, [0.5; 2], cx)
                            }))
                    })),
            )
            .child(
                div()
                    .text_color(p.muted)
                    .child(t!("editor.design_controls.crop_focus")),
            )
            .child(
                div().grid().grid_cols(3).gap_1().children(
                    [
                        (t!("editor.design_controls.top_left"), "↖"),
                        (t!("editor.design_controls.top"), "↑"),
                        (t!("editor.design_controls.top_right"), "↗"),
                        (t!("editor.design_controls.left"), "←"),
                        (t!("editor.design_controls.centre"), "•"),
                        (t!("editor.design_controls.right"), "→"),
                        (t!("editor.design_controls.bottom_left"), "↙"),
                        (t!("editor.design_controls.bottom"), "↓"),
                        (t!("editor.design_controls.bottom_right"), "↘"),
                    ]
                    .into_iter()
                    .enumerate()
                    .map(|(i, (label, glyph))| {
                        Button::new(("design-frame-focus", i))
                            .label(glyph)
                            .tooltip(t!("editor.design_controls.cover_focus", focus = label))
                            .accessibility_label(label)
                            .small()
                            .outline()
                            .disabled(disabled)
                            .on_click(cx.listener(move |this, _, _, cx| {
                                this.fit_design_image(
                                    ImageFit::Cover,
                                    [(i % 3) as f64 / 2., (i / 3) as f64 / 2.],
                                    cx,
                                )
                            }))
                    }),
                ),
            )
            .child(
                div()
                    .text_color(p.muted)
                    .child(t!("editor.design_controls.fit_note")),
            )
            .into_any_element()
    }
}
