//! Narrow conversation surface with the same approval and cancellation paths
//! as the full Assistant dock.
use super::*;
use gpui_kit::component::{
    Disableable, Sizable,
    button::{Button, ButtonVariants},
};

impl EditorView {
    pub(crate) fn photo_assistant(
        &mut self,
        p: &Palette,
        window: &mut Window,
        cx: &mut Context<Self>,
    ) -> AnyElement {
        if self.sidebar_layout.photo.composer.is_none() {
            let input = cx.new(|cx| {
                InputState::new(window, cx).placeholder(t!("assistant.photo_panel.placeholder"))
            });
            let sub = cx.subscribe_in(&input, window, |this, _, event: &InputEvent, window, cx| {
                if matches!(event, InputEvent::Change) {
                    cx.notify();
                }
                if matches!(event, InputEvent::PressEnter { .. }) {
                    this.submit_photo_prompt(window, cx);
                }
            });
            self.sidebar_layout.photo.composer = Some((input, sub));
        }
        let input = self
            .sidebar_layout
            .photo
            .composer
            .as_ref()
            .unwrap()
            .0
            .clone();
        let mut body = div()
            .id("sidebar-assistant-content")
            .test_support()
            .p_3()
            .flex()
            .flex_col()
            .gap_3()
            .text_size(px(12.));
        // Older turns are available from the full transcript. Keep this panel
        // bounded even after a long editing session.
        for turn in self
            .assistant
            .history
            .iter()
            .rev()
            .take(4)
            .collect::<Vec<_>>()
            .into_iter()
            .rev()
            .chain(self.assistant.turn.iter())
        {
            body = body
                .child(
                    div().flex().justify_end().child(
                        div()
                            .max_w(relative(0.92))
                            .p_2()
                            .rounded(px(8.))
                            .bg(p.soft_bg)
                            .child(turn.prompt.clone()),
                    ),
                )
                .when(!turn.text.is_empty(), |body| {
                    body.child(div().text_color(p.ink).child(turn.text.clone()))
                })
                .when(turn.error.is_some(), |body| {
                    body.child(
                        div()
                            .text_color(p.accent)
                            .child(turn.error.clone().unwrap_or_default()),
                    )
                });
        }
        if self.assistant.turn.is_none() {
            body = body.child(
                div()
                    .text_color(p.muted)
                    .child(t!("assistant.photo_panel.empty")),
            );
        }
        if let Some(turn) = &self.assistant.turn {
            for (i, pending) in turn.pending.iter().enumerate() {
                let summary = turn
                    .cards
                    .iter()
                    .find(|card| card.id == pending.tool_use_id)
                    .map(|card| card.summary.clone())
                    .unwrap_or_else(|| t!("assistant.photo_panel.apply_proposed").into());
                body = body.child(
                    div()
                        .flex()
                        .flex_col()
                        .gap_2()
                        .p_2()
                        .rounded(px(6.))
                        .border_1()
                        .border_color(p.accent)
                        .child(summary)
                        .child(
                            div()
                                .flex()
                                .gap_2()
                                .child(
                                    Button::new(("photo-assistant-apply", i))
                                        .label(t!("assistant.photo_panel.apply"))
                                        .small()
                                        .primary()
                                        .on_click(cx.listener(move |this, _, _, cx| {
                                            this.answer(Some(i), true, cx)
                                        })),
                                )
                                .child(
                                    Button::new(("photo-assistant-skip", i))
                                        .label(t!("assistant.photo_panel.skip"))
                                        .small()
                                        .outline()
                                        .on_click(cx.listener(move |this, _, _, cx| {
                                            this.answer(Some(i), false, cx)
                                        })),
                                ),
                        ),
                );
            }
            for card in turn
                .cards
                .iter()
                .filter(|card| matches!(card.status, CardStatus::Running | CardStatus::Failed(_)))
            {
                body = body.child(div().text_color(p.muted).child(card.summary.clone()));
            }
        }
        if self.assistant.running {
            body = body.child(
                div()
                    .flex()
                    .items_center()
                    .justify_between()
                    .child(mono(t!("assistant.photo_panel.working"), 10., p.muted))
                    .child(
                        Button::new("photo-assistant-stop")
                            .label(t!("assistant.photo_panel.stop"))
                            .small()
                            .outline()
                            .on_click(cx.listener(|this, _, _, cx| this.stop_assistant(cx))),
                    ),
            );
        }
        body.child(
            div()
                .flex()
                .gap_1()
                .items_center()
                .child(
                    div()
                        .flex_1()
                        .min_w_0()
                        .child(Input::new(&input).id("photo-assistant-input").small()),
                )
                .child(
                    Button::new("photo-assistant-send")
                        .label("↑")
                        .accessibility_label(t!("assistant.photo_panel.send"))
                        .small()
                        .primary()
                        .disabled(
                            self.assistant.running
                                || self.assistant.reference_loading
                                || input.read(cx).value().trim().is_empty(),
                        )
                        .on_click(
                            cx.listener(|this, _, window, cx| this.submit_photo_prompt(window, cx)),
                        ),
                ),
        )
        .child(
            Button::new("sidebar-assistant-prompt")
                .label(t!("assistant.photo_panel.more_options"))
                .small()
                .ghost()
                .on_click(cx.listener(|this, _, window, cx| this.open_ask(window, cx))),
        )
        .into_any_element()
    }

    fn submit_photo_prompt(&mut self, window: &mut Window, cx: &mut Context<Self>) {
        if self.assistant.running
            || self.assistant.reference_loading
            || self.editor.in_transaction()
            || self.generate.busy
        {
            return;
        }
        let Some((input, _)) = &self.sidebar_layout.photo.composer else {
            return;
        };
        let input = input.clone();
        let text = input.read(cx).value().trim().to_string();
        if text.is_empty() {
            return;
        }
        self.submit_ask(text, cx);
        input.update(cx, |input, cx| input.set_value("", window, cx));
    }
}
