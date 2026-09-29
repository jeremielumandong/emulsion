//! In-window confirmation prompts. Every `window.prompt(..)` renders here in
//! the Emulsion style instead of as a native OS message box.
use crate::theme;
use gpui_kit::component::button::{Button, ButtonVariants};
use gpui_kit::prelude::FluentBuilder;
use gpui_kit::*;

/// Route all window prompts through [`MessageBox`].
pub fn install(cx: &mut App) {
    cx.set_prompt_builder(|level, message, detail, actions, handle, window, cx| {
        let view = cx.new(|cx| MessageBox {
            level,
            message: message.to_string().into(),
            detail: detail.map(|d| d.to_string().into()),
            actions: actions.to_vec(),
            focus: cx.focus_handle(),
        });
        handle.with_view(view, window, cx)
    });
}

struct MessageBox {
    level: PromptLevel,
    message: SharedString,
    detail: Option<SharedString>,
    actions: Vec<PromptButton>,
    focus: FocusHandle,
}

impl MessageBox {
    /// Escape answers with the cancel button, or the last one when none is marked.
    fn cancel_index(&self) -> usize {
        self.actions
            .iter()
            .position(|a| a.is_cancel() || a.label().eq_ignore_ascii_case("cancel"))
            .unwrap_or(self.actions.len().saturating_sub(1))
    }

    fn on_key_down(&mut self, event: &KeyDownEvent, _: &mut Window, cx: &mut Context<Self>) {
        let answer = match event.keystroke.key.as_str() {
            "enter" => 0,
            "escape" => self.cancel_index(),
            _ => return,
        };
        cx.stop_propagation();
        cx.emit(PromptResponse(answer));
    }
}

impl EventEmitter<PromptResponse> for MessageBox {}

impl Focusable for MessageBox {
    fn focus_handle(&self, _: &App) -> FocusHandle {
        self.focus.clone()
    }
}

impl Render for MessageBox {
    fn render(&mut self, _: &mut Window, cx: &mut Context<Self>) -> impl IntoElement {
        let p = theme::palette(cx);
        let destructive = matches!(self.level, PromptLevel::Warning | PromptLevel::Critical);
        // The first answer is the affirmative one; it sits last, where the eye ends.
        let buttons = self
            .actions
            .iter()
            .enumerate()
            .rev()
            .map(|(ix, action)| {
                Button::new(("message-box-action", ix))
                    .label(action.label().clone())
                    .when(ix == 0 && destructive, |b| b.danger())
                    .when(ix == 0 && !destructive, |b| b.primary())
                    .on_click(cx.listener(move |_, _, _, cx| {
                        cx.stop_propagation();
                        cx.emit(PromptResponse(ix));
                    }))
            })
            .collect::<Vec<_>>();

        div()
            .id("message-box")
            .test_support()
            .track_focus(&self.focus)
            .on_key_down(cx.listener(Self::on_key_down))
            .size_full()
            .flex()
            .items_center()
            .justify_center()
            .bg(hsla(0., 0., 0., 0.45))
            .occlude()
            .child(
                div()
                    .flex()
                    .flex_col()
                    .gap(px(16.))
                    .w(px(380.))
                    .max_w_full()
                    .p(px(20.))
                    .rounded_lg()
                    .border_1()
                    .border_color(p.line)
                    .bg(p.panel)
                    .shadow_lg()
                    .child(
                        div()
                            .flex()
                            .flex_col()
                            .gap(px(6.))
                            .child(
                                div()
                                    .text_size(px(14.))
                                    .font_weight(FontWeight::SEMIBOLD)
                                    .text_color(p.ink)
                                    .child(self.message.clone()),
                            )
                            .when_some(self.detail.clone(), |d, detail| {
                                d.child(
                                    div()
                                        .text_size(px(12.))
                                        .text_color(p.muted)
                                        .child(detail),
                                )
                            }),
                    )
                    .child(div().flex().justify_end().gap_2().children(buttons)),
            )
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use core::prelude::v1::test;

    fn open(cx: &mut TestAppContext) -> &mut VisualTestContext {
        cx.update(|cx| {
            gpui_kit::init(cx);
            theme::install(cx);
            install(cx);
        });
        cx.add_empty_window()
    }

    #[gpui_kit::test]
    fn prompts_render_in_window_and_answer_from_keys(cx: &mut TestAppContext) {
        let cx = open(cx);
        for (key, expected) in [("escape", 1), ("enter", 0)] {
            let mut answer = cx.update(|window, cx| {
                let answer = window.prompt(
                    PromptLevel::Warning,
                    "Close without saving?",
                    Some("Your unsaved changes will be lost."),
                    &["Close", "Cancel"],
                    cx,
                );
                window.refresh();
                answer
            });
            cx.run_until_parked();
            cx.update(|window, _| assert!(window.has_active_prompt()));
            cx.simulate_keystrokes(key);
            cx.run_until_parked();
            cx.update(|window, _| assert!(!window.has_active_prompt()));
            assert_eq!(answer.try_recv().unwrap(), Some(expected), "{key}");
        }
    }
}
