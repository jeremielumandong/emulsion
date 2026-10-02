use super::*;
use gpui_kit::component::{Disableable, Sizable, WindowExt, button::Button};

impl EditorView {
    pub(crate) fn playback_setup_dialog(&mut self, window: &mut Window, cx: &mut Context<Self>) {
        let setup = crate::playback_setup::current();
        let owner = cx.weak_entity();
        window.open_dialog(cx, move |dialog, _, _| {
            let owner = owner.clone();
            dialog
                .title(t!("editor.playback_setup_ui.title"))
                .width(px(520.))
                .child(
                    div()
                        .flex()
                        .flex_col()
                        .gap_3()
                        .child(setup.message.clone())
                        .when_some(setup.install.clone(), |panel, plan| {
                            panel.child(
                                Button::new("playback-install-dependencies")
                                    .label(t!("editor.playback_setup_ui.install"))
                                    .small()
                                    .outline()
                                    .disabled(crate::playback_setup::installing())
                                    .on_click(move |_, window, cx| {
                                        let plan = plan.clone();
                                        owner
                                            .update(cx, |this, cx| {
                                                this.stop_design_video(cx);
                                                this.set_status(
                                                    t!("editor.playback_setup_ui.waiting"),
                                                    false,
                                                    cx,
                                                );
                                                cx.spawn(async move |this, cx| {
                                                    let result = cx
                                                        .background_spawn(async move {
                                                            crate::playback_setup::install(plan)
                                                        })
                                                        .await;
                                                    this.update(cx, |this, cx| match result {
                                                        Ok(message) => {
                                                            this.set_status(message, false, cx)
                                                        }
                                                        Err(error) => {
                                                            this.set_status(error, true, cx)
                                                        }
                                                    })
                                                    .ok();
                                                })
                                                .detach();
                                            })
                                            .ok();
                                        window.close_dialog(cx);
                                    }),
                            )
                        })
                        .when_some(setup.website, |panel, url| {
                            panel.child(
                                Button::new("playback-runtime-website")
                                    .label(t!("editor.playback_setup_ui.website"))
                                    .small()
                                    .outline()
                                    .on_click(move |_, _, cx| cx.open_url(url)),
                            )
                        }),
                )
        });
    }
}
