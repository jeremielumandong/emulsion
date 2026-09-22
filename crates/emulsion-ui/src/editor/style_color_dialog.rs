//! A nested native dialog with an independent color cancellation boundary.
use super::color_picker::StyleColorPicker;
use super::*;
use gpui_kit::component::button::Button;
use std::cell::Cell;

impl EditorView {
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
