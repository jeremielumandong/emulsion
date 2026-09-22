//! Native layer-style dialog: a persistent effect checklist and one settings pane.
use super::*;
use gpui_kit::component::button::Button;
use gpui_kit::component::checkbox::Checkbox;
use gpui_kit::component::dialog::DialogButtonProps;
use gpui_kit::component::{Selectable, Sizable, WindowExt};

struct StyleDialogView {
    editor: WeakEntity<EditorView>,
    _observe: Subscription,
}
impl Render for StyleDialogView {
    fn render(&mut self, window: &mut Window, cx: &mut Context<Self>) -> impl IntoElement {
        let Some(editor) = self.editor.upgrade() else {
            return div().into_any_element();
        };
        editor.update(cx, |editor, cx| {
            editor.sync_style_color_pickers(window, cx);
            editor.layer_style_dialog_content(window, cx)
        })
    }
}
impl EditorView {
    pub(crate) fn open_layer_effect(
        &mut self,
        id: NodeId,
        effect: u64,
        window: &mut Window,
        cx: &mut Context<Self>,
    ) {
        let index = self.editor.doc.node(id).and_then(|node| {
            controls::effect_options(node)
                .iter()
                .position(|option| option.id == effect)
        });
        self.styles_ui.expanded = index.map(|index| (id, index));
        self.open_layer_styles_dialog(id, window, cx);
    }
    pub(crate) fn open_layer_styles_dialog(
        &mut self,
        id: NodeId,
        window: &mut Window,
        cx: &mut Context<Self>,
    ) {
        if self.styles_ui.dialog_for == Some(id) {
            cx.notify();
            return;
        }
        self.close_text_field(cx);
        if self.editor.in_transaction() || self.drag.is_some() || self.assistant.running {
            self.set_status(
                "Finish the current edit before opening Layer Style.",
                false,
                cx,
            );
            return;
        }
        if self.editor.doc.node(id).is_none() {
            return;
        }
        self.set_layer_selection(vec![id], Some(id));
        self.styles_ui.dialog_for = Some(id);
        self.editor.begin("Layer style");
        let editor = cx.entity();
        let weak = editor.downgrade();
        let view = cx.new(|cx| StyleDialogView {
            editor: weak.clone(),
            _observe: cx.observe(&editor, |_, _, cx| cx.notify()),
        });
        window.open_dialog(cx, move |dialog, _, _| {
            let ok = weak.clone();
            let cancel = weak.clone();
            let close = weak.clone();
            let ok_button = weak.clone();
            let cancel_button = weak.clone();
            dialog
                .title("Layer Style")
                .width(px(880.))
                .movable(true)
                .overlay(false)
                .overlay_closable(false)
                .button_props(
                    DialogButtonProps::default()
                        .ok_text("OK")
                        .show_cancel(true)
                        .cancel_text("Cancel"),
                )
                .on_ok(move |_, _, cx| {
                    if let Some(editor) = ok.upgrade() {
                        editor.update(cx, |e, cx| e.close_style_dialog(true, cx));
                    }
                    true
                })
                .on_cancel(move |_, _, cx| {
                    if let Some(editor) = cancel.upgrade() {
                        editor.update(cx, |e, cx| e.close_style_dialog(false, cx));
                    }
                    true
                })
                .on_close(move |_, _, cx| {
                    if let Some(editor) = close.upgrade() {
                        editor.update(cx, |e, cx| e.close_style_dialog(false, cx));
                    }
                })
                .footer(
                    div()
                        .flex()
                        .justify_end()
                        .gap_2()
                        .child(
                            div()
                                .id("style-dialog-cancel")
                                .child(
                                    Button::new("style-dialog-cancel-button")
                                        .label("Cancel")
                                        .on_click(move |_, window, cx| {
                                            if let Some(editor) = cancel_button.upgrade() {
                                                editor.update(cx, |e, cx| {
                                                    e.close_style_dialog(false, cx)
                                                });
                                            }
                                            window.close_dialog(cx);
                                        }),
                                )
                                .test_support(),
                        )
                        .child(
                            div()
                                .id("style-dialog-ok")
                                .child(Button::new("style-dialog-ok-button").label("OK").on_click(
                                    move |_, window, cx| {
                                        if let Some(editor) = ok_button.upgrade() {
                                            editor
                                                .update(cx, |e, cx| e.close_style_dialog(true, cx));
                                        }
                                        window.close_dialog(cx);
                                    },
                                ))
                                .test_support(),
                        ),
                )
                .child(view.clone())
        });
        cx.notify();
    }
    pub(crate) fn close_style_dialog(&mut self, commit: bool, cx: &mut Context<Self>) {
        if self.styles_ui.dialog_for.take().is_none() {
            return;
        }
        self.styles_ui.colors.clear();
        self.styles_ui.color_dialog_for = None;
        self.styles_ui.expanded = None;
        self.styles_ui.advanced = None;
        self.menu = None;
        self.invalidate_pending_edits();
        if matches!(self.drag, Some(Drag::Slider { .. })) {
            self.drag = None;
            self.editor.end();
        }
        if commit {
            self.editor.end();
        } else {
            self.editor.cancel();
        }
        self.after_change(cx);
    }
    fn apply_style_dialog(&mut self, cx: &mut Context<Self>) {
        if self.styles_ui.dialog_for.is_none() {
            return;
        }
        self.editor.end();
        self.editor.begin("Layer style");
        self.after_change(cx);
    }
    fn select_dialog_style(&mut self, id: NodeId, index: usize, cx: &mut Context<Self>) {
        self.styles_ui.expanded = Some((id, index));
        self.styles_ui.advanced = None;
        cx.notify();
    }
    fn layer_style_dialog_content(
        &mut self,
        window: &mut Window,
        cx: &mut Context<Self>,
    ) -> AnyElement {
        let Some(id) = self.styles_ui.dialog_for else {
            return div().into_any_element();
        };
        let Some(node) = self.editor.doc.node(id).cloned() else {
            return div().into_any_element();
        };
        let options = controls::effect_options(&node);
        let p = theme::palette(cx);
        let mut list = div()
            .id("style-dialog-effects")
            .w_56()
            .flex_none()
            .min_h_0()
            .overflow_y_scroll()
            .flex()
            .flex_col()
            .gap_1()
            .pr_3()
            .border_r_1()
            .border_color(p.line)
            .child(
                Button::new("style-dialog-blending")
                    .label("Blending Options")
                    .selected(self.styles_ui.expanded.is_none())
                    .small()
                    .on_click(cx.listener(|e, _, _, cx| {
                        e.styles_ui.expanded = None;
                        e.styles_ui.advanced = None;
                        cx.notify();
                    })),
            )
            .child(
                Checkbox::new("style-dialog-master")
                    .label("Layer effects")
                    .checked(node.effects_enabled)
                    .on_click(cx.listener(move |e, checked: &bool, _, cx| {
                        e.execute(
                            Command::SetEffectsEnabled {
                                id,
                                enabled: *checked,
                            },
                            cx,
                        );
                    })),
            );
        for (index, style) in node.styles.iter().enumerate() {
            let effect = options[index].id;
            let selected = self.styles_ui.expanded == Some((id, index));
            list = list.child(
                div()
                    .flex()
                    .items_center()
                    .gap_1()
                    .child(
                        div()
                            .id(SharedString::from(format!("style-dialog-enabled-{effect}")))
                            .child(
                                Checkbox::new(("style-dialog-check", effect))
                                    .accessibility_label(format!("Enable {}", style.label()))
                                    .checked(options[index].enabled)
                                    .on_click(cx.listener(move |e, checked: &bool, _, cx| {
                                        e.update_style_option(
                                            id,
                                            index,
                                            |o| o.enabled = *checked,
                                            cx,
                                        )
                                    })),
                            )
                            .test_support(),
                    )
                    .child(
                        div()
                            .id(SharedString::from(format!("style-dialog-effect-{effect}")))
                            .flex_1()
                            .child(
                                Button::new(("style-dialog-select", effect))
                                    .label(style.label())
                                    .selected(selected)
                                    .small()
                                    .on_click(cx.listener(move |e, _, _, cx| {
                                        e.select_dialog_style(id, index, cx)
                                    })),
                            )
                            .test_support(),
                    ),
            );
        }
        for (catalogue, style) in LayerStyle::catalogue().into_iter().enumerate() {
            if node.styles.iter().any(|s| s.key() == style.key()) {
                continue;
            }
            let name = style.label();
            list = list.child(
                div()
                    .id(("style-kind", catalogue))
                    .child(
                        Checkbox::new(("style-dialog-add", catalogue))
                            .label(name)
                            .checked(false)
                            .on_click(
                                cx.listener(move |e, _, _, cx| e.add_style(id, style.clone(), cx)),
                            ),
                    )
                    .test_support(),
            );
        }
        let mut settings = div()
            .id("style-dialog-settings")
            .flex_1()
            .min_w_0()
            .min_h_0()
            .overflow_y_scroll()
            .flex()
            .flex_col()
            .gap_2()
            .pl_3();
        if let Some((selected, index)) = self
            .styles_ui
            .expanded
            .filter(|(selected, index)| *selected == id && *index < node.styles.len())
        {
            let style = &node.styles[index];
            settings = settings.child(label(format!("{} · {}", node.name, style.label()), &p));
            settings = settings.children(self.effect_controls(
                (selected, index),
                style,
                &options[index],
                node.styles.len(),
                &p,
                cx,
            ));
        } else {
            settings = settings.child(self.blending_options_panel(&p, cx));
        }
        let height = (f32::from(window.viewport_size().height) - 190.).clamp(240., 680.);
        div()
            .id("layer-style-dialog")
            .flex()
            .flex_col()
            .gap_3()
            .child(
                div()
                    .flex()
                    .min_h_0()
                    .h(px(height))
                    .child(list.test_support())
                    .child(settings.test_support()),
            )
            .child(
                div()
                    .flex()
                    .items_center()
                    .gap_2()
                    .child(mono(
                        "Preview updates on the canvas. OK keeps changes; Cancel restores them.",
                        10.,
                        p.muted,
                    ))
                    .child(div().flex_1())
                    .child(
                        div()
                            .id("style-dialog-apply")
                            .child(
                                Button::new("style-dialog-apply-button")
                                    .label("Apply")
                                    .small()
                                    .on_click(cx.listener(|e, _, _, cx| e.apply_style_dialog(cx))),
                            )
                            .test_support(),
                    ),
            )
            .test_support()
            .into_any_element()
    }
}
