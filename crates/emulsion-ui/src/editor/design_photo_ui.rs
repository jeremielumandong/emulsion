//! Photo operations remain available on selected Design images without leaving the page.
use super::*;
use gpui_kit::component::{
    WindowExt,
    input::{Input, InputState},
};
impl EditorView {
    pub(crate) fn replace_photo_source_dialog(&mut self, id: NodeId, cx: &mut Context<Self>) {
        if !self.prepare_page_action(cx) {
            return;
        }
        if emulsion_core::photo_source::dimensions(&self.editor.doc, id).is_none() {
            return;
        }
        let ticket = self.edit_ticket();
        let doc = self.editor.doc.clone();
        let pick = cx.prompt_for_paths(PathPromptOptions {
            files: true,
            directories: false,
            multiple: false,
            prompt: Some(t!("editor.design_photo_ui.replace_prompt").into()),
        });
        cx.spawn(async move |this, cx| {
            let Ok(Ok(Some(paths))) = pick.await else {
                return;
            };
            let Some(path) = paths.into_iter().next() else {
                return;
            };
            let result = cx
                .background_spawn(async move {
                    let decoded = emulsion_io::import::decode(&path).map_err(|e| e.to_string())?;
                    let mut trial = emulsion_core::Editor::new(doc, None);
                    emulsion_core::photo_source::replace(&mut trial, id, Arc::new(decoded.raster))?;
                    Ok::<_, String>(trial.doc)
                })
                .await;
            this.update(cx, |this, cx| {
                if this.edit_ticket() != ticket {
                    this.set_status(t!("editor.design_photo_ui.source_changed"), true, cx);
                    return;
                }
                match result.and_then(|doc| {
                    this.editor
                        .commit_design_document(doc, "Replace image source")
                }) {
                    Ok(()) => {
                        this.after_change(cx);
                        this.set_status(t!("editor.design_photo_ui.replaced"), false, cx);
                    }
                    Err(e) => this.set_status(e, true, cx),
                }
            })
            .ok();
        })
        .detach();
    }
    pub(crate) fn crop_photo_source_dialog(
        &mut self,
        id: NodeId,
        window: &mut Window,
        cx: &mut Context<Self>,
    ) {
        if !self.prepare_page_action(cx) {
            return;
        }
        let Some((w, h)) = emulsion_core::photo_source::dimensions(&self.editor.doc, id) else {
            return;
        };
        let inputs = [0., 0., f64::from(w), f64::from(h)]
            .map(|v| cx.new(|cx| InputState::new(window, cx).default_value(v.to_string())));
        let owner = cx.weak_entity();
        let ticket = self.edit_ticket();
        let error = cx.new(|_| String::new());
        window.open_dialog(cx, move |dialog, _, cx| {
            let inputs = inputs.clone();
            let owner = owner.clone();
            let error_apply = error.clone();
            dialog
                .title(t!("editor.design_photo_ui.crop_title"))
                .width(px(420.))
                .child(t!(
                    "editor.design_photo_ui.crop_body",
                    width = w,
                    height = h
                ))
                .children(
                    [
                        ("Source X", t!("editor.design_photo_ui.source_x")),
                        ("Source Y", t!("editor.design_photo_ui.source_y")),
                        ("Crop width", t!("editor.design_photo_ui.crop_width")),
                        ("Crop height", t!("editor.design_photo_ui.crop_height")),
                    ]
                    .into_iter()
                    .zip(inputs.iter())
                    .map(|((id, label), input)| {
                        div()
                            .flex()
                            .flex_col()
                            .child(label)
                            .child(Input::new(input).id(id))
                    }),
                )
                .when(!error.read(cx).is_empty(), |d| {
                    d.child(error.read(cx).clone())
                })
                .footer(crate::widgets::form_dialog_footer(t!(
                    "editor.design_photo_ui.crop_title"
                )))
                .on_ok(move |_, window, cx| {
                    let values = inputs
                        .iter()
                        .map(|input| {
                            input
                                .read(cx)
                                .value()
                                .parse::<f64>()
                                .map_err(|_| t!("editor.design_photo_ui.numeric").into_owned())
                        })
                        .collect::<Result<Vec<_>, _>>();
                    let result = values.and_then(|values| {
                        owner
                            .update(cx, |this, cx| {
                                if this.edit_ticket() != ticket {
                                    return Err(
                                        t!("editor.design_photo_ui.page_changed").into_owned()
                                    );
                                }
                                emulsion_core::photo_source::crop(
                                    &mut this.editor,
                                    id,
                                    values.try_into().unwrap(),
                                )?;
                                this.after_change(cx);
                                Ok(())
                            })
                            .unwrap_or_else(|_| {
                                Err(t!("editor.design_controls.editor_closed").into_owned())
                            })
                    });
                    match result {
                        Ok(()) => true,
                        Err(e) => {
                            error_apply.update(cx, |v, cx| {
                                *v = e;
                                cx.notify();
                            });
                            window.refresh();
                            false
                        }
                    }
                })
        });
    }
    pub(crate) fn adjust_design_photo(&mut self, id: NodeId, key: &str, cx: &mut Context<Self>) {
        if !self.prepare_page_action(cx) {
            return;
        }
        let Some(adjustment) = Adjustment::catalogue().into_iter().find(|a| a.key() == key) else {
            return;
        };
        let Some(base) = self.editor.doc.node(id) else {
            return;
        };
        let parent = base.parent;
        let index = self
            .editor
            .doc
            .children(parent)
            .iter()
            .position(|node| *node == id)
            .unwrap_or(0)
            + 1;
        let mut trial = emulsion_core::Editor::new(self.editor.doc.clone(), None);
        let result = (|| {
            let added = trial
                .execute(Command::AddNode {
                    node: Box::new(Node::adjust(0, adjustment)),
                    slot: emulsion_core::command::Slot { parent, index },
                })
                .map_err(|e| e.to_string())?
                .ok_or_else(|| t!("editor.design_photo_ui.adjust_failed").into_owned())?;
            trial
                .execute(Command::SetClip {
                    id: added,
                    clip_to: Some(id),
                })
                .map_err(|e| e.to_string())?;
            self.editor
                .commit_design_document(trial.doc, "Adjust selected image")?;
            Ok::<_, String>(added)
        })();
        match result {
            Ok(added) => {
                self.set_layer_selection(vec![added], Some(added));
                self.select_sidebar(SidebarTab::Properties, cx);
                self.after_change(cx);
                self.set_status(t!("editor.design_photo_ui.adjusted"), false, cx);
            }
            Err(e) => self.set_status(e, true, cx),
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use ::core::prelude::v1::test;
    use gpui_kit::test::TestWindowExt;
    #[gpui_kit::test]
    fn design_photo_crop_dialog_validates_preserves_source_and_undo(cx: &mut TestAppContext) {
        let (ws, cx) = crate::tests::open(cx, Document::new(100, 100));
        cx.simulate_resize(size(px(1200.), px(900.)));
        let (view, id, before) = cx.update(|window, cx| {
            let view = ws.read(cx).editor.clone().unwrap();
            let (id, before) = view.update(cx, |v, cx| {
                let id = v
                    .editor
                    .execute(Command::AddNode {
                        node: Box::new(Node::raster(
                            0,
                            "Photo",
                            Arc::new(Raster::from_srgba8(20, 20, &vec![255; 1600])),
                            Placement::default(),
                        )),
                        slot: emulsion_core::command::Slot::TOP,
                    })
                    .unwrap()
                    .unwrap();
                let before = v.editor.doc.clone();
                v.crop_photo_source_dialog(id, window, cx);
                (id, before)
            });
            (view, id, before)
        });
        cx.run_until_parked();
        cx.update(|window, cx| window.click("Crop width", cx));
        cx.simulate_keystrokes("ctrl-a");
        cx.simulate_input("40");
        cx.update(|window, cx| window.click("ok", cx));
        cx.run_until_parked();
        cx.update(|window, cx| {
            assert_eq!(view.read(cx).editor.doc, before);
            assert!(window.find("ok").visible());
            window.click("Crop width", cx);
        });
        cx.simulate_keystrokes("ctrl-a");
        cx.simulate_input("10");
        cx.update(|window, cx| window.click("ok", cx));
        cx.run_until_parked();
        cx.update(|_, cx| {
            view.update(cx, |v, _| {
                assert_eq!(
                    v.editor
                        .doc
                        .node(id)
                        .unwrap()
                        .mask
                        .as_ref()
                        .unwrap()
                        .get(15, 5),
                    0
                );
                let NodeKind::Raster { raster, .. } = &v.editor.doc.node(id).unwrap().kind else {
                    panic!()
                };
                assert_eq!(raster.width(), 20);
                v.editor.undo();
                assert_eq!(v.editor.doc, before);
            })
        });
    }
}
