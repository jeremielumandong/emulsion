//! Property keyframes share the native motion inspector and Undo history.
use super::*;
use emulsion_core::design_keyframes::{self as keyframes, Easing, Keyframe, Property};
fn property_label(v: Property) -> SharedString {
    match v {
        Property::TranslationX => t!("editor.design_keyframes_ui.prop_translation_x"),
        Property::TranslationY => t!("editor.design_keyframes_ui.prop_translation_y"),
        Property::ScaleX => t!("editor.design_keyframes_ui.prop_scale_x"),
        Property::ScaleY => t!("editor.design_keyframes_ui.prop_scale_y"),
        Property::Rotation => t!("editor.design_keyframes_ui.prop_rotation"),
        Property::Opacity => t!("editor.design_keyframes_ui.prop_opacity"),
        Property::Visibility => t!("editor.design_keyframes_ui.prop_visibility"),
        Property::TextReveal => t!("editor.design_keyframes_ui.prop_text_reveal"),
    }
    .into()
}
fn easing_label(v: Easing) -> SharedString {
    match v {
        Easing::Linear => t!("editor.design_keyframes_ui.ease_linear"),
        Easing::EaseIn => t!("editor.design_keyframes_ui.ease_in"),
        Easing::EaseOut => t!("editor.design_keyframes_ui.ease_out"),
        Easing::EaseInOut => t!("editor.design_keyframes_ui.ease_in_out"),
        Easing::Step => t!("editor.design_keyframes_ui.ease_hold"),
    }
    .into()
}
impl EditorView {
    pub(super) fn retime_motion_dialog(&mut self, window: &mut Window, cx: &mut Context<Self>) {
        if !self.prepare_page_action(cx) {
            return;
        }
        let ids = self.selected_layer_ids();
        if ids.is_empty() {
            return;
        }
        let duration = self.editor.doc.design.duration_ms;
        let inputs = ["1".to_owned(), "0".to_owned(), duration.to_string()]
            .map(|v| cx.new(|cx| InputState::new(window, cx).default_value(v)));
        let owner = cx.weak_entity();
        let ticket = self.edit_ticket();
        let error = Rc::new(RefCell::new(String::new()));
        window.open_dialog(cx, move |dialog, _, _| {
            let mut body = div().flex().flex_col().gap_2();
            for (index, label) in [
                t!("editor.design_keyframes_ui.time_scale"),
                t!("editor.design_keyframes_ui.offset_ms"),
                t!("editor.design_keyframes_ui.page_duration_ms"),
            ]
            .into_iter()
            .enumerate()
            {
                body = body
                    .child(label)
                    .child(Input::new(&inputs[index]).id(("design-retime-field", index)));
            }
            body = body
                .child(t!("editor.design_keyframes_ui.retime_note"))
                .child(error.borrow().clone());
            let fields = inputs.clone();
            let owner = owner.clone();
            let ids = ids.clone();
            let error = error.clone();
            dialog
                .title(t!("editor.design_keyframes_ui.retime_title"))
                .width(px(460.))
                .child(body)
                .footer(crate::widgets::form_dialog_footer(t!(
                    "editor.design_keyframes_ui.apply_timing"
                )))
                .on_ok(move |_, _, cx| {
                    let result = (|| -> Result<_, String> {
                        Ok((
                            fields[0]
                                .read(cx)
                                .value()
                                .parse::<f64>()
                                .map_err(|_| t!("editor.design_keyframes_ui.numeric_scale"))?,
                            fields[1]
                                .read(cx)
                                .value()
                                .parse::<i64>()
                                .map_err(|_| t!("editor.design_keyframes_ui.whole_offset"))?,
                            fields[2]
                                .read(cx)
                                .value()
                                .parse::<u32>()
                                .map_err(|_| t!("editor.design_keyframes_ui.whole_duration"))?,
                        ))
                    })();
                    let result = result.and_then(|(scale, offset, duration)| {
                        owner
                            .update(cx, |this, cx| {
                                if this.edit_ticket() != ticket {
                                    return Err(t!(
                                        "editor.design_keyframes_ui.page_changed_timing"
                                    )
                                    .into());
                                }
                                keyframes::retime(
                                    &mut this.editor,
                                    &ids,
                                    scale,
                                    offset,
                                    Some(duration),
                                )?;
                                this.after_change(cx);
                                Ok(())
                            })
                            .unwrap_or_else(|_| {
                                Err(t!("editor.design_keyframes_ui.editor_closed").into())
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
    pub(super) fn property_keyframes_dialog(
        &mut self,
        window: &mut Window,
        cx: &mut Context<Self>,
    ) {
        if !self.prepare_page_action(cx) {
            return;
        }
        let Some(id) = self.selected else { return };
        let saved = self
            .editor
            .doc
            .design
            .keyframes
            .get(&id)
            .cloned()
            .unwrap_or_default();
        let property = Rc::new(std::cell::Cell::new(Property::TranslationX));
        let easing = Rc::new(std::cell::Cell::new(Easing::Linear));
        let remove = Rc::new(std::cell::Cell::new(false));
        let time = cx.new(|cx| InputState::new(window, cx).default_value("0"));
        let value = cx.new(|cx| InputState::new(window, cx).default_value("0"));
        let error = Rc::new(RefCell::new(String::new()));
        let owner = cx.weak_entity();
        let ticket = self.edit_ticket();
        let duration = self.editor.doc.design.duration_ms;
        window.open_dialog(cx, move |dialog, _, _| {
            let mut body = div()
                .id("design-keyframes-body")
                .flex()
                .flex_col()
                .gap_2()
                .max_h(px(430.))
                .overflow_y_scroll();
            let pick = property.clone();
            let field = value.clone();
            body = body.child(
                Button::new("design-keyframe-property")
                    .label(format!("{} ▾", property_label(property.get())))
                    .outline()
                    .dropdown_menu(move |mut menu, _, _| {
                        for p in Property::ALL {
                            let pick = pick.clone();
                            let field = field.clone();
                            menu = menu.item(PopupMenuItem::new(property_label(p)).on_click(
                                move |_, window, cx| {
                                    pick.set(p);
                                    field.update(cx, |state, cx| {
                                        state.set_value(p.initial().to_string(), window, cx)
                                    });
                                    cx.refresh_windows();
                                },
                            ));
                        }
                        menu
                    }),
            );
            let pick = easing.clone();
            body = body.child(
                Button::new("design-keyframe-easing")
                    .label(t!(
                        "editor.design_keyframes_ui.easing_menu",
                        name = easing_label(easing.get())
                    ))
                    .outline()
                    .dropdown_menu(move |mut menu, _, _| {
                        for e in Easing::ALL {
                            let pick = pick.clone();
                            menu = menu.item(PopupMenuItem::new(easing_label(e)).on_click(
                                move |_, _, cx| {
                                    pick.set(e);
                                    cx.refresh_windows();
                                },
                            ));
                        }
                        menu
                    }),
            );
            body = body
                .child(t!(
                    "editor.design_keyframes_ui.time_ms",
                    duration = duration
                ))
                .child(Input::new(&time).id("design-keyframe-time"))
                .child(t!("editor.design_keyframes_ui.value_hint"))
                .child(Input::new(&value).id("design-keyframe-value"));
            let deleting = remove.clone();
            body = body.child(
                Button::new("design-keyframe-action")
                    .label(if remove.get() {
                        t!("editor.design_keyframes_ui.action_delete")
                    } else {
                        t!("editor.design_keyframes_ui.action_save")
                    })
                    .outline()
                    .on_click(move |_, _, cx| {
                        deleting.set(!deleting.get());
                        cx.refresh_windows();
                    }),
            );
            for track in &saved {
                for frame in &track.frames {
                    let p = track.property;
                    let f = *frame;
                    let property = property.clone();
                    let easing = easing.clone();
                    let time = time.clone();
                    let value = value.clone();
                    body = body.child(
                        Button::new(SharedString::from(format!(
                            "design-saved-keyframe-{p:?}-{}",
                            f.time_ms
                        )))
                        .label(format!(
                            "{} · {} ms → {}",
                            property_label(p),
                            f.time_ms,
                            f.value
                        ))
                        .small()
                        .ghost()
                        .on_click(move |_, window, cx| {
                            property.set(p);
                            easing.set(f.easing);
                            time.update(cx, |s, cx| s.set_value(f.time_ms.to_string(), window, cx));
                            value.update(cx, |s, cx| s.set_value(f.value.to_string(), window, cx));
                            cx.refresh_windows();
                        }),
                    );
                }
            }
            body = body
                .child(t!("editor.design_keyframes_ui.keyframes_note"))
                .child(error.borrow().clone());
            let time = time.clone();
            let value = value.clone();
            let owner = owner.clone();
            let property = property.clone();
            let easing = easing.clone();
            let remove = remove.clone();
            let error = error.clone();
            dialog
                .title(t!("editor.design_keyframes_ui.keyframes_title"))
                .width(px(500.))
                .child(body)
                .footer(crate::widgets::form_dialog_footer(t!(
                    "editor.design_keyframes_ui.apply_keyframe"
                )))
                .on_ok(move |_, _, cx| {
                    let result = (|| -> Result<_, String> {
                        let time = time
                            .read(cx)
                            .value()
                            .parse::<u32>()
                            .map_err(|_| t!("editor.design_keyframes_ui.whole_time"))?;
                        let value = value
                            .read(cx)
                            .value()
                            .parse::<f64>()
                            .map_err(|_| t!("editor.design_keyframes_ui.numeric_value"))?;
                        Ok((time, value))
                    })();
                    let result = result.and_then(|(time, value)| {
                        owner
                            .update(cx, |this, cx| {
                                if this.edit_ticket() != ticket {
                                    return Err(t!(
                                        "editor.design_keyframes_ui.page_changed_keyframes"
                                    )
                                    .into());
                                }
                                if remove.get() {
                                    keyframes::remove_keyframe(
                                        &mut this.editor,
                                        id,
                                        property.get(),
                                        time,
                                    )?
                                } else {
                                    keyframes::set_keyframe(
                                        &mut this.editor,
                                        id,
                                        property.get(),
                                        Keyframe {
                                            time_ms: time,
                                            value,
                                            easing: easing.get(),
                                        },
                                    )?
                                }
                                this.after_change(cx);
                                Ok(())
                            })
                            .unwrap_or_else(|_| {
                                Err(t!("editor.design_keyframes_ui.editor_closed").into())
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
    fn design_property_keyframe_authoring_cancel_save_and_undo(cx: &mut TestAppContext) {
        let mut editor = emulsion_core::Editor::new(Document::new(640, 480), None);
        let id = emulsion_core::design::media::insert_youtube(
            &mut editor,
            "https://youtu.be/M7lc1UVf-VE",
            (10., 20.),
            (400., 225.),
        )
        .unwrap();
        let original = editor.doc.clone();
        let (workspace, cx) = crate::tests::open(cx, editor.doc);
        let view = cx.update(|_, cx| workspace.read(cx).editor.clone().unwrap());
        cx.update(|window, cx| {
            view.update(cx, |this, cx| {
                this.set_layer_selection(vec![id], Some(id));
                this.property_keyframes_dialog(window, cx)
            })
        });
        cx.run_until_parked();
        cx.simulate_keystrokes("escape");
        cx.run_until_parked();
        cx.update(|window, cx| {
            assert_eq!(view.read(cx).editor.doc, original);
            view.update(cx, |this, cx| this.property_keyframes_dialog(window, cx));
        });
        cx.run_until_parked();
        cx.update(|window, cx| window.click("design-keyframe-value", cx));
        cx.simulate_keystrokes(if cfg!(target_os = "macos") {
            "cmd-a"
        } else {
            "ctrl-a"
        });
        cx.simulate_input("42.5");
        cx.run_until_parked();
        cx.update(|window, cx| window.click("ok", cx));
        cx.run_until_parked();
        cx.update(|_, cx| {
            view.update(cx, |this, cx| {
                assert_eq!(
                    this.editor.doc.design.keyframes[&id][0].frames[0].value,
                    42.5
                );
                assert_eq!(this.editor.doc.nodes, original.nodes);
                let preview = emulsion_core::design_metadata::at_time(&this.editor.doc, 0).unwrap();
                assert_eq!(
                    emulsion_core::design::media::bounds(&preview, id)
                        .unwrap()
                        .0,
                    52.5
                );
                this.undo(cx);
                assert_eq!(this.editor.doc, original);
            })
        });
    }
}

#[cfg(test)]
mod retime_tests {
    use super::*;
    use ::core::prelude::v1::test;
    use gpui::TestAppContext;
    use gpui_kit::test::TestWindowExt;
    #[gpui_kit::test]
    fn design_bulk_retime_form_rejects_overflow_and_undoes_as_one_step(cx: &mut TestAppContext) {
        let mut editor = emulsion_core::Editor::new(Document::new(640, 480), None);
        let id = emulsion_core::design::media::insert_youtube(
            &mut editor,
            "https://youtu.be/M7lc1UVf-VE",
            (10., 20.),
            (400., 225.),
        )
        .unwrap();
        keyframes::apply_preset(&mut editor, &[id], keyframes::Preset::SlideUp, 0, 1000).unwrap();
        let before = editor.doc.clone();
        let (workspace, cx) = crate::tests::open(cx, editor.doc);
        let view = cx.update(|_, cx| workspace.read(cx).editor.clone().unwrap());
        cx.update(|window, cx| {
            view.update(cx, |this, cx| {
                this.set_layer_selection(vec![id], Some(id));
                this.retime_motion_dialog(window, cx)
            })
        });
        cx.run_until_parked();
        for (index, value) in [(0usize, "2"), (1usize, "100"), (2usize, "2000")] {
            cx.update(|window, cx| window.click(("design-retime-field", index), cx));
            cx.simulate_keystrokes(if cfg!(target_os = "macos") {
                "cmd-a"
            } else {
                "ctrl-a"
            });
            cx.simulate_input(value);
            cx.run_until_parked();
        }
        cx.update(|window, cx| window.click("ok", cx));
        cx.run_until_parked();
        cx.update(|window, cx| {
            assert_eq!(view.read(cx).editor.doc, before);
            window.click(("design-retime-field", 2usize), cx);
        });
        cx.simulate_keystrokes(if cfg!(target_os = "macos") {
            "cmd-a"
        } else {
            "ctrl-a"
        });
        cx.simulate_input("3000");
        cx.run_until_parked();
        cx.update(|window, cx| window.click("ok", cx));
        cx.run_until_parked();
        cx.update(|_, cx| {
            view.update(cx, |this, cx| {
                assert_eq!(
                    this.editor.doc.design.keyframes[&id][0].frames[1].time_ms,
                    2100
                );
                this.undo(cx);
                assert_eq!(this.editor.doc, before);
            })
        });
    }
}
