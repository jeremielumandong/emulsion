//! Obvious ways into the AI assistant: an "Ask AI" button in the document
//! bar, and a welcome hint above the canvas until the user closes it.
use super::*;

/// The Ask shortcut: F1, the Help key, on every platform.
pub(crate) const ASK_SHORTCUT: &str = "F1";

fn open_ask(window: &mut Window, cx: &mut App) {
    window.dispatch_action(Box::new(crate::actions::Ask), cx);
}

fn shortcut_badge(color: Hsla, border: Hsla) -> Div {
    div()
        .px(px(4.))
        .border_1()
        .border_color(border)
        .rounded_sm()
        .font_family(MONO_FONT)
        .text_size(px(9.5))
        .text_color(color)
        .child(ASK_SHORTCUT)
}

impl EditorView {
    /// A filled accent button, so the assistant is easy to spot.
    pub(super) fn ask_ai_button(&self, p: &Palette, cx: &mut Context<Self>) -> AnyElement {
        let open = self.ask.is_some();
        let (fg, bg) = (p.accent_fg, p.accent);
        crate::widgets::tip(
            div()
                .id("ask-ai-button")
                .test_support()
                .flex()
                .flex_none()
                .items_center()
                .gap(px(6.))
                .px(px(9.))
                .py(px(3.))
                .rounded_md()
                .cursor_pointer()
                .bg(bg)
                .text_color(fg)
                .text_size(px(12.))
                .font_weight(FontWeight::SEMIBOLD)
                .when(open, |d| d.opacity(0.8))
                .hover(|s| s.opacity(0.9))
                .child("✦ Ask AI")
                .child(shortcut_badge(fg, fg.opacity(0.6)))
                .on_click(cx.listener(|this, _, window, cx| {
                    if this.ask.is_some() {
                        this.close_ask(window, cx);
                    } else {
                        open_ask(window, cx);
                    }
                })),
            format!("AI Assistance: describe an edit or an image in plain words ({ASK_SHORTCUT})"),
        )
        .into_any_element()
    }

    /// The Ask bar when it is open; otherwise, until dismissed, a hint
    /// telling the user how to open it.
    pub(super) fn ask_area(&mut self, p: &Palette, cx: &mut Context<Self>) -> Option<AnyElement> {
        if self.ask.is_some() {
            return self.ask_bar(p, cx);
        }
        if crate::app_state::settings(cx).ai_hint_dismissed {
            return None;
        }
        let dismiss =
            |cx: &mut App| crate::app_state::update_settings(cx, |s| s.ai_hint_dismissed = true);
        Some(
            div()
                .id("ask-ai-hint")
                .test_support()
                .flex()
                .flex_none()
                .items_center()
                .gap(px(10.))
                .px(px(16.))
                .py(px(7.))
                .border_b_1()
                .border_color(p.accent)
                .bg(p.accent.opacity(0.12))
                .text_size(px(12.))
                .text_color(p.ink)
                .child(
                    div()
                        .text_color(p.accent)
                        .font_weight(FontWeight::BOLD)
                        .child("✦ Need help?"),
                )
                .child(
                    div()
                        .flex_1()
                        .min_w_0()
                        .overflow_hidden()
                        .whitespace_nowrap()
                        .text_ellipsis()
                        .child(
                            "Ask the AI assistant to edit, retouch or generate: just describe it.",
                        ),
                )
                .child(
                    div()
                        .id("ask-ai-hint-open")
                        .test_support()
                        .flex()
                        .flex_none()
                        .items_center()
                        .gap(px(6.))
                        .px(px(9.))
                        .py(px(3.))
                        .rounded_md()
                        .cursor_pointer()
                        .bg(p.accent)
                        .text_color(p.accent_fg)
                        .font_weight(FontWeight::SEMIBOLD)
                        .hover(|s| s.opacity(0.9))
                        .child("Click here to ask AI")
                        .on_click(cx.listener(move |_, _, window, cx| {
                            dismiss(cx);
                            open_ask(window, cx);
                        })),
                )
                .child(
                    div()
                        .flex()
                        .flex_none()
                        .items_center()
                        .gap(px(5.))
                        .text_color(p.muted)
                        .child("or press")
                        .child(shortcut_badge(p.ink, p.line)),
                )
                .child(crate::widgets::tip(
                    div()
                        .id("ask-ai-hint-close")
                        .test_support()
                        .flex_none()
                        .px(px(5.))
                        .cursor_pointer()
                        .text_color(p.muted)
                        .hover(|s| s.text_color(p.ink))
                        .child("✕")
                        .on_click(cx.listener(move |_, _, _, cx| dismiss(cx))),
                    format!("Hide this tip. The Ask AI button and {ASK_SHORTCUT} still work."),
                ))
                .into_any_element(),
        )
    }
}
