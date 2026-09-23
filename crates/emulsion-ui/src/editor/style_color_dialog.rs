//! A nested native dialog with an independent color cancellation boundary.
use super::color_picker::StyleColorPicker;
use super::*;
use gpui_kit::component::button::Button;
use std::cell::Cell;

impl EditorView {
    pub(crate) fn open_foreground_color_dialog(
        &mut self,
        window: &mut Window,
        cx: &mut Context<Self>,
    ) {
        use gpui_kit::component::color_picker::ColorPickerState;
        self.tools.picker = false;
        self.close_text_field(cx);
        let [r, g, b, a] = self.tools.fg.map(|v| v as f32 / 255.);
        let state =
            cx.new(|cx| ColorPickerState::new(window, cx).default_value(Rgba { r, g, b, a }));
        let picker = cx.new(|cx| StyleColorPicker::new(state.clone(), window, cx));
        let editor = cx.weak_entity();
        // The compact popup closes, so restore focus to the canvas on dismissal.
        window.focus(&self.canvas_focus, cx);
        let body = picker.clone();
        let confirm = Rc::new(move |window: &mut Window, cx: &mut App| {
            if !picker.update(cx, |picker, cx| picker.commit_pending(window, cx)) {
                return false;
            }
            if let Some(color) = state.read(cx).value() {
                let rgb = color.to_rgb();
                let bytes =
                    [rgb.r, rgb.g, rgb.b, rgb.a].map(|v| (v * 255.).round().clamp(0., 255.) as u8);
                let _ = editor.update(cx, |editor, cx| editor.set_fg(bytes, cx));
            }
            true
        });
        window.open_dialog(cx, move |dialog, _, _| {
            let ok = confirm.clone();
            let button_ok = confirm.clone();
            dialog
                .title("Foreground color")
                .width(px(590.))
                .child(body.clone())
                .on_ok(move |_, window, cx| ok(window, cx))
                .footer(
                    div()
                        .flex()
                        .justify_end()
                        .gap_2()
                        .child(
                            Button::new("foreground-color-cancel")
                                .label("Cancel")
                                .on_click(|_, window, cx| window.close_dialog(cx)),
                        )
                        .child(Button::new("foreground-color-ok").label("OK").on_click(
                            move |_, window, cx| {
                                if button_ok(window, cx) {
                                    window.close_dialog(cx);
                                }
                            },
                        )),
                )
        });
        cx.notify();
    }

    pub(super) fn open_style_color_dialog(
        &mut self,
        key: (NodeId, u64, usize),
        title: String,
        window: &mut Window,
        cx: &mut Context<Self>,
    ) {
        if self.styles_ui.dialog_for != Some(key.0) || self.styles_ui.color_dialog_for.is_some() {
            return;
        }
        let Some(draft) = self.styles_ui.colors.get(&key) else {
            return;
        };
        let state = draft.state.clone();
        let title = self
            .editor
            .doc
            .node(key.0)
            .and_then(|node| {
                controls::effect_options(node)
                    .iter()
                    .position(|option| option.id == key.1)
                    .map(|index| format!("{} — {title}", node.styles[index].label()))
            })
            .unwrap_or(title);
        let original = state.read(cx).value();
        let picker = cx.new(|cx| StyleColorPicker::new(state.clone(), window, cx));
        let body = picker.clone();
        let resolved = Rc::new(Cell::new(false));
        self.styles_ui.color_dialog_for = Some(key);
        let weak = cx.entity().downgrade();
        let finish = Rc::new(
            move |commit: bool, window: &mut Window, cx: &mut App| -> bool {
                if resolved.get() {
                    return true;
                }
                if commit && !picker.update(cx, |picker, cx| picker.commit_pending(window, cx)) {
                    return false;
                }
                resolved.set(true);
                if let Some(editor) = weak.upgrade() {
                    let active = editor.read(cx).styles_ui.color_dialog_for == Some(key);
                    if active {
                        if !commit && let Some(original) = original {
                            state.update(cx, |state, cx| state.update_color(original, window, cx));
                        }
                        editor.update(cx, |editor, cx| {
                            editor.styles_ui.color_dialog_for = None;
                            cx.notify();
                        });
                    }
                }
                true
            },
        );
        window.open_dialog(cx, move |dialog, _, _| {
            let ok = finish.clone();
            let cancel = finish.clone();
            let close = finish.clone();
            let ok_button = finish.clone();
            let cancel_button = finish.clone();
            dialog
                .title(format!("Color Picker — {title}"))
                .width(px(590.))
                .movable(true)
                .overlay(false)
                .overlay_closable(false)
                .on_ok(move |_, window, cx| ok(true, window, cx))
                .on_cancel(move |_, window, cx| cancel(false, window, cx))
                .on_close(move |_, window, cx| {
                    close(false, window, cx);
                })
                .child(body.clone())
                .footer(
                    div()
                        .flex()
                        .justify_end()
                        .gap_2()
                        .child(
                            div().id("style-color-cancel").test_support().child(
                                Button::new("style-color-cancel-button")
                                    .label("Cancel")
                                    .on_click(move |_, window, cx| {
                                        if cancel_button(false, window, cx) {
                                            window.close_dialog(cx);
                                        }
                                    }),
                            ),
                        )
                        .child(div().id("style-color-ok").test_support().child(
                            Button::new("style-color-ok-button").label("OK").on_click(
                                move |_, window, cx| {
                                    if ok_button(true, window, cx) {
                                        window.close_dialog(cx);
                                    }
                                },
                            ),
                        )),
                )
        });
        cx.notify();
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use ::core::prelude::v1::test;
    use gpui_kit::component::Root;
    use gpui_kit::test::TestWindowExt;

    struct Host;
    impl Render for Host {
        fn render(&mut self, window: &mut Window, cx: &mut Context<Self>) -> impl IntoElement {
            div()
                .size_full()
                .children(Root::render_dialog_layer(window, cx))
        }
    }

    #[gpui_kit::test]
    fn foreground_cmyk_dialog_commits_and_escape_cancels(cx: &mut TestAppContext) {
        let editor = cx.update(|cx| {
            gpui_kit::init(cx);
            theme::install(cx);
            cx.set_reduce_motion(true);
            cx.set_global(crate::app_state::AppSettings(Default::default()));
            cx.new(|cx| EditorView::new(Document::new(64, 64), None, None, None, "CMYK".into(), cx))
        });
        let (_, cx) = cx.add_window_view(|window, cx| {
            let host = cx.new(|_| Host);
            Root::new(host, window, cx)
        });
        cx.simulate_resize(size(px(900.), px(800.)));
        cx.update(|window, cx| {
            editor.update(cx, |editor, cx| {
                editor.set_fg([255, 255, 255, 102], cx);
                editor.open_foreground_color_dialog(window, cx);
            });
        });
        cx.run_until_parked();
        cx.update(|window, cx| window.render_frame(cx));
        cx.update(|window, cx| {
            let bounds = window.find("style-color-cyan").bounds();
            window.click_at(
                "style-color-cyan",
                point(bounds.size.width - px(20.), bounds.size.height / 2.),
                cx,
            );
            window.press("ctrl-a", cx);
            window.input("100", cx);
        });
        cx.run_until_parked();
        cx.update(|window, cx| window.click("foreground-color-ok", cx));
        cx.run_until_parked();
        cx.update(|window, cx| window.render_frame(cx));
        cx.update(|window, cx| {
            assert_eq!(editor.read(cx).tools.fg, [0, 255, 255, 102]);
            editor.update(cx, |editor, cx| {
                editor.open_foreground_color_dialog(window, cx)
            });
        });
        cx.run_until_parked();
        cx.update(|window, cx| window.render_frame(cx));
        cx.update(|window, cx| {
            window.click("style-color-swatch-3", cx);
            window.press("escape", cx);
        });
        cx.run_until_parked();
        cx.update(|_, cx| assert_eq!(editor.read(cx).tools.fg, [0, 255, 255, 102]));
    }
}
