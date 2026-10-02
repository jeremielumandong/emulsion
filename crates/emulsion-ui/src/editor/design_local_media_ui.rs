use super::*;
impl EditorView {
    pub(crate) fn import_design_media(&mut self, cx: &mut Context<Self>) {
        if !self.prepare_page_action(cx) {
            return;
        }
        let ticket = self.edit_ticket();
        let rx = cx.prompt_for_paths(PathPromptOptions {
            files: true,
            directories: false,
            multiple: false,
            prompt: Some(t!("editor.design_local_media_ui.import_prompt").into()),
        });
        cx.spawn(async move |this, cx| {
            let Ok(Ok(Some(paths))) = rx.await else {
                return;
            };
            let Some(path) = paths.into_iter().next() else {
                return;
            };
            let result = cx
                .background_spawn(async move { emulsion_io::design_media::read_local(&path) })
                .await;
            this.update(cx, |this, cx| {
                if this.edit_ticket() != ticket {
                    this.set_status(
                        t!("editor.design_local_media_ui.page_changed_import"),
                        true,
                        cx,
                    );
                    return;
                }
                let result = result.and_then(|media| {
                    let w = f64::from(this.editor.doc.width).min(640.);
                    let h = (w * 9. / 16.).min(f64::from(this.editor.doc.height));
                    let origin = (
                        (f64::from(this.editor.doc.width) - w) / 2.,
                        (f64::from(this.editor.doc.height) - h) / 2.,
                    );
                    media::insert_local(&mut this.editor, media, origin, (w, h))
                });
                match result {
                    Ok(id) => {
                        this.set_layer_selection(vec![id], Some(id));
                        this.after_change(cx);
                        this.set_tool(Tool::Move, cx)
                    }
                    Err(e) => this.set_status(e, true, cx),
                }
            })
            .ok();
        })
        .detach();
    }
    pub(super) fn local_media_dialog(&mut self, window: &mut Window, cx: &mut Context<Self>) {
        if !self.prepare_page_action(cx) {
            return;
        }
        let Some(id) = self.selected else { return };
        let Some(media) = self.editor.doc.design.local_media.get(&id) else {
            return;
        };
        let values = [
            media.trim_start_ms.to_string(),
            media.trim_end_ms.map(|v| v.to_string()).unwrap_or_default(),
            (media.volume * 100.).to_string(),
        ];
        let inputs = values.map(|v| cx.new(|cx| InputState::new(window, cx).default_value(v)));
        let looping = Rc::new(std::cell::Cell::new(media.looping));
        let error = Rc::new(RefCell::new(String::new()));
        let owner = cx.weak_entity();
        let ticket = self.edit_ticket();
        window.open_dialog(cx, move |dialog, _, _| {
            let mut body = div().flex().flex_col().gap_2();
            for (index, label) in [
                t!("editor.design_local_media_ui.trim_start"),
                t!("editor.design_local_media_ui.trim_end"),
                t!("editor.design_local_media_ui.volume"),
            ]
            .into_iter()
            .enumerate()
            {
                body = body
                    .child(label)
                    .child(Input::new(&inputs[index]).id(("design-media-field", index)));
            }
            let repeat = looping.clone();
            body = body
                .child(
                    Button::new("design-media-loop")
                        .label(if looping.get() {
                            t!("editor.design_local_media_ui.loop_on")
                        } else {
                            t!("editor.design_local_media_ui.loop_off")
                        })
                        .outline()
                        .on_click(move |_, _, cx| {
                            repeat.set(!repeat.get());
                            cx.refresh_windows();
                        }),
                )
                .child(t!("editor.design_local_media_ui.formats"))
                .child(error.borrow().clone());
            let fields = inputs.clone();
            let owner = owner.clone();
            let error = error.clone();
            let looping = looping.clone();
            dialog
                .title(t!("editor.design_local_media_ui.title"))
                .width(px(460.))
                .child(body)
                .footer(crate::widgets::form_dialog_footer(t!(
                    "editor.design_local_media_ui.apply"
                )))
                .on_ok(move |_, _, cx| {
                    let parsed = (|| -> Result<_, String> {
                        let start = fields[0]
                            .read(cx)
                            .value()
                            .parse::<u32>()
                            .map_err(|_| t!("editor.design_local_media_ui.start_error"))?;
                        let end = fields[1].read(cx).value().to_string();
                        let end = if end.trim().is_empty() {
                            None
                        } else {
                            Some(
                                end.parse::<u32>()
                                    .map_err(|_| t!("editor.design_local_media_ui.end_error"))?,
                            )
                        };
                        let volume = fields[2]
                            .read(cx)
                            .value()
                            .parse::<f32>()
                            .map_err(|_| t!("editor.design_local_media_ui.volume_error"))?
                            / 100.;
                        Ok((start, end, volume))
                    })();
                    let result = parsed.and_then(|(start, end, volume)| {
                        owner
                            .update(cx, |this, cx| {
                                if this.edit_ticket() != ticket {
                                    return Err(
                                        t!("editor.design_local_media_ui.page_changed").into()
                                    );
                                }
                                media::update_local(
                                    &mut this.editor,
                                    id,
                                    start,
                                    end,
                                    volume,
                                    looping.get(),
                                )?;
                                this.after_change(cx);
                                Ok(())
                            })
                            .unwrap_or_else(|_| {
                                Err(t!("editor.design_local_media_ui.editor_closed").into())
                            })
                    });
                    match result {
                        Ok(()) => true,
                        Err(e) => {
                            *error.borrow_mut() = e;
                            cx.refresh_windows();
                            false
                        }
                    }
                })
        });
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use ::core::prelude::v1::test;
    use gpui::TestAppContext;
    use gpui_kit::test::TestWindowExt;
    #[gpui_kit::test]
    fn design_local_media_playback_form_cancel_invalid_and_undo(cx: &mut TestAppContext) {
        let mut editor = emulsion_core::Editor::new(Document::new(640, 480), None);
        let media =
            media::LocalMedia::from_bytes("tone.wav".into(), b"RIFF\0\0\0\0WAVEdata".to_vec())
                .unwrap();
        let id = media::insert_local(&mut editor, media, (10., 20.), (400., 225.)).unwrap();
        let original = editor.doc.clone();
        let (workspace, cx) = crate::tests::open(cx, editor.doc);
        let view = cx.update(|_, cx| workspace.read(cx).editor.clone().unwrap());
        cx.update(|window, cx| {
            view.update(cx, |this, cx| {
                this.set_layer_selection(vec![id], Some(id));
                this.local_media_dialog(window, cx)
            })
        });
        cx.run_until_parked();
        cx.simulate_keystrokes("escape");
        cx.run_until_parked();
        cx.update(|window, cx| {
            assert_eq!(view.read(cx).editor.doc, original);
            view.update(cx, |this, cx| this.local_media_dialog(window, cx));
        });
        cx.run_until_parked();
        cx.update(|window, cx| window.click(("design-media-field", 2usize), cx));
        cx.simulate_keystrokes(if cfg!(target_os = "macos") {
            "cmd-a"
        } else {
            "ctrl-a"
        });
        cx.simulate_input("200");
        cx.run_until_parked();
        cx.update(|window, cx| window.click("ok", cx));
        cx.run_until_parked();
        cx.update(|window, cx| {
            assert_eq!(view.read(cx).editor.doc, original);
            window.click(("design-media-field", 2usize), cx);
        });
        cx.simulate_keystrokes(if cfg!(target_os = "macos") {
            "cmd-a"
        } else {
            "ctrl-a"
        });
        cx.simulate_input("25");
        cx.run_until_parked();
        cx.update(|window, cx| window.click("design-media-loop", cx));
        cx.run_until_parked();
        cx.update(|window, cx| window.click("ok", cx));
        cx.run_until_parked();
        cx.update(|_, cx| {
            view.update(cx, |this, cx| {
                assert_eq!(this.editor.doc.design.local_media[&id].volume, 0.25);
                assert!(this.editor.doc.design.local_media[&id].looping);
                this.undo(cx);
                assert_eq!(this.editor.doc, original);
            })
        });
    }
}
