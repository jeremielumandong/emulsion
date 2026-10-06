//! Page backgrounds use ordinary document objects and the existing isolated preview.
use super::*;
use crate::file_prompt::FilePrompts;
use emulsion_core::design_background;
use gpui_kit::component::{
    Sizable,
    button::{Button, ButtonVariants},
    color_picker::{ColorPicker, ColorPickerEvent, ColorPickerState},
    menu::{DropdownMenu, PopupMenuItem},
};
use rust_i18n::t;

impl EditorView {
    /// This prefix never scrolls away with the selected object's controls.
    pub(super) fn design_page_background_controls(
        &self,
        p: &Palette,
        cx: &Context<Self>,
    ) -> AnyElement {
        let color = design_background::color(&self.editor.doc);
        let [r, g, b, a] = color.map(|v| v as f32 / 255.);
        let has_image =
            design_background::parts(&self.editor.doc).is_some_and(|b| b.image.is_some());
        let editor = cx.entity();
        div()
            .id("design-page-background-controls")
            .test_support()
            .flex()
            .flex_none()
            .items_center()
            .gap_1()
            .child(
                Button::new("design-page-background-color")
                    .accessibility_label(t!("design.background.color").to_string())
                    .tooltip(t!("design.background.color").to_string())
                    .xsmall()
                    .outline()
                    .h(px(25.))
                    .px_2()
                    .child(
                        div()
                            .size(px(12.))
                            .border_1()
                            .border_color(p.line)
                            .bg(Rgba { r, g, b, a }),
                    )
                    .child(t!("design.background.page").to_string())
                    .on_click(cx.listener(|this, _, window, cx| {
                        this.preview_page_background_color(window, cx)
                    })),
            )
            .child(
                Button::new("design-page-background-photo")
                    .label(t!("design.background.photo").to_string())
                    .xsmall()
                    .outline()
                    .h(px(25.))
                    .dropdown_menu(move |menu, _, _| {
                        let choose = editor.clone();
                        let crop = editor.clone();
                        let remove = editor.clone();
                        menu.item(
                            PopupMenuItem::new(if has_image {
                                t!("design.background.replace").to_string()
                            } else {
                                t!("design.background.choose").to_string()
                            })
                            .on_click(move |_, window, cx| {
                                choose.update(cx, |this, cx| {
                                    this.choose_page_background_image(window, cx)
                                })
                            }),
                        )
                        .item(
                            PopupMenuItem::new(t!("design.background.crop").to_string())
                                .disabled(!has_image)
                                .on_click(move |_, window, cx| {
                                    crop.update(cx, |this, cx| {
                                        this.preview_page_background_crop(window, cx)
                                    })
                                }),
                        )
                        .item(
                            PopupMenuItem::new(t!("design.background.remove").to_string())
                                .disabled(!has_image)
                                .on_click(move |_, _, cx| {
                                    remove.update(cx, |this, cx| {
                                        this.remove_page_background_image(cx)
                                    })
                                }),
                        )
                    }),
            )
            .into_any_element()
    }

    pub(super) fn preview_page_background_color(
        &mut self,
        window: &mut Window,
        cx: &mut Context<Self>,
    ) {
        if !self.is_design() || !self.prepare_page_action(cx) {
            return;
        }
        let color = design_background::color(&self.editor.doc);
        let [r, g, b, a] = color.map(|v| v as f32 / 255.);
        let state =
            cx.new(|cx| ColorPickerState::new(window, cx).default_value(Rgba { r, g, b, a }));
        self.begin_background_preview(
            self.editor.doc.clone(),
            None,
            t!("design.background.color").to_string(),
            window,
            cx,
        );
        let subscription = cx.subscribe(&state, |this, source, event, cx| {
            // A delayed event from a dismissed picker cannot recolor a newer
            // preview, even on the same page.
            if !this
                .design_ui
                .frame_crop
                .as_ref()
                .and_then(|p| p.color_picker.as_ref())
                .is_some_and(|active| active.entity_id() == source.entity_id())
            {
                return;
            }
            if let ColorPickerEvent::Change(Some(color)) = event {
                let color = color.to_rgb();
                this.update_page_background_color(
                    [color.r, color.g, color.b, color.a]
                        .map(|v| (v * 255.).round().clamp(0., 255.) as u8),
                    cx,
                );
            }
        });
        let preview = self.design_ui.frame_crop.as_mut().unwrap();
        preview.color_picker = Some(state);
        preview.color_subscription = Some(subscription);
        cx.notify();
    }

    pub(super) fn update_page_background_color(&mut self, color: [u8; 4], cx: &mut Context<Self>) {
        let Some(preview) = self
            .design_ui
            .frame_crop
            .as_mut()
            .filter(|p| p.color_picker.is_some())
        else {
            return;
        };
        let mut trial = match Editor::try_new(preview.preview.clone(), None) {
            Ok(editor) => editor,
            Err(error) => {
                self.set_status(error.to_string(), true, cx);
                return;
            }
        };
        match design_background::set_color(&mut trial, color) {
            Ok(_) => {
                preview.preview = trial.doc;
                self.frame_crop_changed(cx);
            }
            Err(error) => self.set_status(error, true, cx),
        }
    }

    pub(super) fn preview_selected_page_background(
        &mut self,
        window: &mut Window,
        cx: &mut Context<Self>,
    ) {
        if !self.is_design() || !self.prepare_page_action(cx) {
            return;
        }
        let Some(id) = self
            .selected
            .filter(|_| self.selected_layer_ids().len() == 1)
        else {
            return;
        };
        let mut trial = match Editor::try_new(self.editor.doc.clone(), None) {
            Ok(editor) => editor,
            Err(error) => {
                self.set_status(error.to_string(), true, cx);
                return;
            }
        };
        match design_background::set_image(&mut trial, id) {
            Ok(image) => self.begin_background_preview(
                trial.doc,
                Some(image),
                t!("design.background.set").to_string(),
                window,
                cx,
            ),
            Err(error) => self.set_status(error, false, cx),
        }
    }

    pub(super) fn preview_page_background_crop(
        &mut self,
        window: &mut Window,
        cx: &mut Context<Self>,
    ) {
        if !self.is_design() || !self.prepare_page_action(cx) {
            return;
        }
        let Some(image) = design_background::parts(&self.editor.doc).and_then(|p| p.image) else {
            return;
        };
        if let Err(error) =
            emulsion_core::design::frame_image_editable(&self.editor.doc, image.image)
        {
            self.set_status(error, false, cx);
            return;
        }
        self.begin_background_preview(
            self.editor.doc.clone(),
            Some(image.image),
            t!("design.background.crop").to_string(),
            window,
            cx,
        );
    }

    pub(super) fn remove_page_background_image(&mut self, cx: &mut Context<Self>) {
        if !self.is_design() || !self.prepare_page_action(cx) {
            return;
        }
        match design_background::remove_image(&mut self.editor) {
            Ok(()) => self.after_change(cx),
            Err(error) => self.set_status(error, true, cx),
        }
    }

    pub(super) fn choose_page_background_image(
        &mut self,
        window: &mut Window,
        cx: &mut Context<Self>,
    ) {
        if !self.is_design() || !self.prepare_page_action(cx) {
            return;
        }
        let Some(ticket) = self.begin_design_asset_request() else {
            self.photo_transform_ready(cx);
            return;
        };
        let page = self.editor.active_page();
        let paths = cx.prompt_open_paths(PathPromptOptions {
            files: true,
            directories: false,
            multiple: false,
            prompt: Some(t!("design.background.choose").to_string().into()),
        });
        cx.spawn_in(window, async move |this, cx| {
            let paths = paths.await;
            let path = match paths {
                Ok(Ok(Some(paths))) => paths.into_iter().next(),
                _ => None,
            };
            let Some(path) = path else {
                this.update_in(cx, |this, _, cx| {
                    this.accept_design_asset_result(ticket, page, cx);
                })
                .ok();
                return;
            };
            let source = path.clone();
            let result = cx
                .background_spawn(async move {
                    super::design_asset_ui::frame_asset_raster_with_report(&source)
                })
                .await;
            this.update_in(cx, |this, window, cx| {
                if !this.accept_design_asset_result(ticket, page, cx) {
                    return;
                }
                let notice = result.as_ref().ok().and_then(|(_, _, report)| {
                    crate::workspace::import_report::source_notice(&path, None, *report)
                });
                let result = result
                    .map_err(|e| e.to_string())
                    .and_then(|(raster, _, _)| {
                        let mut trial = Editor::try_new(this.editor.doc.clone(), None)
                            .map_err(|e| e.to_string())?;
                        let image = design_background::replace_image(&mut trial, raster)?;
                        Ok((trial.doc, image))
                    });
                match result {
                    Ok((doc, image)) => {
                        this.begin_background_preview(
                            doc,
                            Some(image),
                            t!("design.background.replace").to_string(),
                            window,
                            cx,
                        );
                        if let Some((message, warning)) = notice {
                            this.set_status(message, warning, cx);
                        }
                    }
                    Err(error) => this.set_status(error, true, cx),
                }
            })
            .ok();
        })
        .detach();
    }

    pub(super) fn page_background_color_view(&self, p: &Palette, cx: &Context<Self>) -> AnyElement {
        let state = self
            .design_ui
            .frame_crop
            .as_ref()
            .and_then(|p| p.color_picker.clone())
            .unwrap();
        let has_image =
            design_background::parts(&self.editor.doc).is_some_and(|p| p.image.is_some());
        div()
            .id("design-background-color-editor")
            .test_support()
            .flex()
            .flex_col()
            .flex_1()
            .min_h_0()
            .min_w_0()
            .key_context("FrameCrop")
            .capture_key_down(cx.listener(|this, event: &KeyDownEvent, window, cx| {
                match event.keystroke.key.as_str() {
                    "escape" => {
                        this.cancel_frame_crop(cx);
                        window.focus(&this.canvas_focus, cx);
                    }
                    "enter" if this.canvas_focus.is_focused(window) => {
                        this.finish_frame_crop(cx);
                        window.focus(&this.canvas_focus, cx);
                    }
                    "tab" => return,
                    _ if !this.canvas_focus.is_focused(window) => return,
                    _ => {}
                }
                cx.stop_propagation();
            }))
            .child(
                div()
                    .flex()
                    .flex_wrap()
                    .items_center()
                    .gap_2()
                    .p_2()
                    .bg(p.panel)
                    .child(
                        div()
                            .flex_1()
                            .min_w_0()
                            .child(t!("design.background.color").to_string()),
                    )
                    .child(ColorPicker::new(&state).label(t!("design.background.pick").to_string()))
                    .child(
                        Button::new("background-color-cancel")
                            .label(t!("design.background.cancel").to_string())
                            .small()
                            .ghost()
                            .on_click(cx.listener(|this, _, window, cx| {
                                this.cancel_frame_crop(cx);
                                window.focus(&this.canvas_focus, cx);
                            })),
                    )
                    .child(
                        Button::new("background-color-done")
                            .label(t!("design.background.done").to_string())
                            .small()
                            .primary()
                            .on_click(cx.listener(|this, _, window, cx| {
                                this.finish_frame_crop(cx);
                                window.focus(&this.canvas_focus, cx);
                            })),
                    ),
            )
            .child(self.canvas_region())
            .child(
                div()
                    .p_2()
                    .text_size(px(11.))
                    .text_color(p.muted)
                    .child(if has_image {
                        t!("design.background.behind_photo").to_string()
                    } else {
                        t!("design.background.preview_hint").to_string()
                    }),
            )
            .into_any_element()
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use ::core::prelude::v1::test;
    use emulsion_core::{
        Node,
        project::{ProjectEditor, ProjectKind},
    };
    use gpui_kit::test::TestWindowExt;

    fn fixture() -> (Document, NodeId, NodeId) {
        let mut doc = Document::new(600, 400);
        Command::AddNode {
            node: Box::new(Node::new(
                0,
                "Custom color",
                NodeKind::Fill {
                    rgba: [240, 230, 210, 255],
                },
            )),
            slot: Slot::TOP,
        }
        .apply(&mut doc)
        .unwrap();
        let image = Command::AddNode {
            node: Box::new(Node::raster(
                0,
                "Landscape",
                Arc::new(Raster::solid(800, 400, [0.15, 0.35, 0.65, 1.])),
                Placement {
                    scale_x: 0.75,
                    scale_y: 1.,
                    ..Default::default()
                },
            )),
            slot: Slot::TOP,
        }
        .apply(&mut doc)
        .unwrap()
        .unwrap();
        let text = Command::AddNode {
            node: Box::new(Node::text(
                0,
                "Title",
                emulsion_core::text::TextSpec {
                    text: "A bright idea".into(),
                    size: 42.,
                    x: 80.,
                    y: 100.,
                    ..Default::default()
                },
                600,
                400,
            )),
            slot: Slot::TOP,
        }
        .apply(&mut doc)
        .unwrap()
        .unwrap();
        (doc, image, text)
    }

    #[gpui_kit::test]
    fn background_color_is_reachable_on_covered_page_previews_cancel_done_undo(
        cx: &mut TestAppContext,
    ) {
        let (authored, _, text) = fixture();
        let (workspace, cx) = crate::tests::open(cx, authored.clone());
        cx.simulate_resize(size(px(900.), px(700.)));
        let view = cx.update(|window, cx| {
            workspace.update(cx, |workspace, cx| {
                workspace.install_project(
                    ProjectEditor::new_project(ProjectKind::Design, authored.clone()).unwrap(),
                    "Background".into(),
                    window,
                    cx,
                )
            });
            let view = workspace.read(cx).editor.clone().unwrap();
            view.update(cx, |v, cx| {
                v.set_layer_selection(vec![text], Some(text));
                cx.notify();
            });
            view
        });
        cx.run_until_parked();
        cx.update(|window, cx| {
            assert!(window.find("design-page-background-color").visible());
            assert!(
                window.find("design-drawer").bounds().top()
                    >= window
                        .find("design-page-background-controls")
                        .bounds()
                        .bottom()
            );
            window.click("design-page-background-color", cx);
        });
        cx.run_until_parked();
        cx.update(|window, cx| {
            assert!(window.find("design-background-color-editor").visible());
            view.update(cx, |v, cx| {
                v.update_page_background_color([220, 30, 80, 255], cx);
                assert_eq!(v.editor.doc, authored);
                assert_eq!(v.editor.history.len(), 0);
                assert_eq!(
                    design_background::color(&v.render_doc().unwrap()),
                    [220, 30, 80, 255]
                );
                assert_eq!(v.selected, Some(text));
            });
            window.click("background-color-cancel", cx);
        });
        cx.run_until_parked();
        cx.update(|window, cx| {
            view.update(cx, |v, cx| {
                assert_eq!(v.editor.doc, authored);
                assert_eq!(v.render_doc().unwrap(), authored);
                v.preview_page_background_color(window, cx);
                v.update_page_background_color([220, 30, 80, 255], cx);
            });
        });
        cx.run_until_parked();
        cx.update(|window, cx| window.click("background-color-done", cx));
        cx.run_until_parked();
        cx.update(|_, cx| {
            view.update(cx, |v, cx| {
                assert_eq!(design_background::color(&v.editor.doc), [220, 30, 80, 255]);
                assert_eq!(v.editor.history.len(), 1);
                assert_eq!(v.selected, Some(text));
                v.undo(cx);
                assert_eq!(v.editor.doc, authored);
                v.redo(cx);
                assert_eq!(design_background::color(&v.editor.doc), [220, 30, 80, 255]);
            })
        });
    }

    #[gpui_kit::test]
    fn background_image_preview_crop_cancel_commit_remove_and_page_change(cx: &mut TestAppContext) {
        let (authored, image, text) = fixture();
        let (workspace, cx) = crate::tests::open(cx, authored.clone());
        cx.simulate_resize(size(px(1100.), px(800.)));
        let view = cx.update(|window, cx| {
            workspace.update(cx, |workspace, cx| {
                workspace.install_project(
                    ProjectEditor::new_project(ProjectKind::Design, authored.clone()).unwrap(),
                    "Background image".into(),
                    window,
                    cx,
                )
            });
            let view = workspace.read(cx).editor.clone().unwrap();
            view.update(cx, |v, cx| {
                v.set_layer_selection(vec![image], Some(image));
                v.preview_selected_page_background(window, cx);
                assert!(v.frame_crop_active());
                assert_eq!(v.editor.doc, authored);
                assert_eq!(
                    design_background::parts(&v.render_doc().unwrap())
                        .unwrap()
                        .image
                        .unwrap()
                        .image,
                    image
                );
                v.zoom_frame_crop(1.5, cx);
            });
            view
        });
        cx.run_until_parked();
        cx.simulate_keystrokes("escape");
        cx.run_until_parked();
        cx.update(|window, cx| {
            view.update(cx, |v, cx| {
                assert!(!v.frame_crop_active());
                assert_eq!(v.editor.doc, authored);
                assert_eq!(v.selected, Some(image));
                v.preview_selected_page_background(window, cx);
                v.zoom_frame_crop(1.5, cx);
                let preview = v.render_doc().unwrap();
                v.finish_frame_crop(cx);
                assert_eq!(v.editor.doc, preview);
                assert_eq!(v.editor.history.len(), 1);
                assert!(v.selected.is_none());
                let NodeKind::Raster { raster: before, .. } = &authored.node(image).unwrap().kind
                else {
                    panic!()
                };
                let NodeKind::Raster { raster: after, .. } =
                    &v.editor.doc.node(image).unwrap().kind
                else {
                    panic!()
                };
                assert!(Arc::ptr_eq(before, after));
                assert_eq!(v.editor.doc.node(text), authored.node(text));
                assert_eq!(v.design_hit((10., 10.), false), None);
                v.undo(cx);
                assert_eq!(v.editor.doc, authored);
                v.redo(cx);
                let with_image = v.editor.doc.clone();
                v.remove_page_background_image(cx);
                assert!(v.editor.doc.node(image).is_none());
                assert_eq!(v.editor.doc.node(text), authored.node(text));
                v.undo(cx);
                assert_eq!(v.editor.doc, with_image);
                v.preview_page_background_crop(window, cx);
                v.zoom_frame_crop(1.2, cx);
                v.add_project_page(false, cx);
                let next = v.editor.doc.clone();
                assert!(!v.frame_crop_active());
                v.finish_frame_crop(cx);
                assert_eq!(v.editor.doc, next);
            })
        });
    }

    #[gpui_kit::test]
    fn background_color_preview_blocks_edit_shortcuts_and_cancels_on_navigation(
        cx: &mut TestAppContext,
    ) {
        let (authored, _, text) = fixture();
        let (workspace, cx) = crate::tests::open(cx, authored.clone());
        cx.simulate_resize(size(px(900.), px(700.)));
        let view = cx.update(|window, cx| {
            workspace.update(cx, |workspace, cx| {
                workspace.install_project(
                    ProjectEditor::new_project(ProjectKind::Design, authored.clone()).unwrap(),
                    "Preview guards".into(),
                    window,
                    cx,
                )
            });
            let view = workspace.read(cx).editor.clone().unwrap();
            view.update(cx, |v, cx| {
                v.set_layer_selection(vec![text], Some(text));
                v.preview_page_background_color(window, cx);
                v.update_page_background_color([0, 0, 0, 255], cx);
            });
            view
        });
        cx.run_until_parked();
        cx.simulate_keystrokes("ctrl-j delete backspace ctrl-v right");
        cx.run_until_parked();
        cx.update(|_, cx| {
            view.update(cx, |v, cx| {
                assert_eq!(v.editor.doc, authored);
                assert!(v.frame_crop_active());
                v.add_project_page(false, cx);
                assert!(!v.frame_crop_active());
                let next = v.editor.doc.clone();
                v.finish_frame_crop(cx);
                assert_eq!(v.editor.doc, next);
            })
        });
    }
    #[gpui_kit::test]
    fn pasted_painted_background_frame_is_selectable_without_selecting_the_page_background(
        cx: &mut TestAppContext,
    ) {
        let (doc, image, _) = fixture();
        let mut editor = Editor::new(doc, None);
        design_background::set_image(&mut editor, image).unwrap();
        // Painted frames are portable; invisible shape-only page frames are
        // rejected by clipboard capture instead of losing their page role.
        let boundary = design_background::parts(&editor.doc)
            .unwrap()
            .image
            .unwrap()
            .boundary;
        editor.doc.node_mut(boundary).unwrap().opacity = 1.;
        let group = design_background::parts(&editor.doc)
            .unwrap()
            .image
            .unwrap()
            .group;
        let fragment = emulsion_core::fragment::Fragment::capture(&editor.doc, &[group]).unwrap();
        let view = cx.update(|cx| {
            gpui_kit::init(cx);
            theme::install(cx);
            cx.set_global(crate::app_state::AppSettings(Default::default()));
            cx.new(|cx| EditorView::new(editor.doc, None, None, None, "Background hit".into(), cx))
        });
        view.update(cx, |v, cx| {
            assert_eq!(v.design_hit((10., 10.), false), None);
            let copies = fragment
                .paste_into_project(&mut v.editor, Slot::TOP, (0., 0.))
                .unwrap();
            v.after_change(cx);
            assert_eq!(v.design_hit((10., 10.), false), Some(copies[0]));
            assert_ne!(copies[0], group);
            v.execute(
                Command::SetVisible {
                    id: copies[0],
                    visible: false,
                },
                cx,
            );
            assert_eq!(v.design_hit((10., 10.), false), None);
        });
    }
    #[gpui_kit::test]
    fn select_all_design_objects_excludes_background_and_locked_artwork(cx: &mut TestAppContext) {
        let (doc, image, text) = fixture();
        let mut editor = Editor::new(doc, None);
        design_background::set_image(&mut editor, image).unwrap();
        let authored = editor.doc.clone();
        let (workspace, cx) = crate::tests::open(cx, authored.clone());
        let view = cx.update(|window, cx| {
            workspace.update(cx, |workspace, cx| {
                workspace.install_project(
                    ProjectEditor::new_project(ProjectKind::Design, authored.clone()).unwrap(),
                    "Select objects".into(),
                    window,
                    cx,
                )
            });
            workspace.read(cx).editor.clone().unwrap()
        });
        cx.update(|_, cx| {
            view.update(cx, |v, cx| {
                v.select_all(cx);
                assert_eq!(v.selected_layer_ids(), vec![text]);
                assert!(v.editor.doc.selection.is_none());
                v.deselect(cx);
                assert!(v.selected_layer_ids().is_empty());
                v.execute(
                    Command::SetLocked {
                        id: text,
                        locked: true,
                    },
                    cx,
                );
                v.select_all(cx);
                assert!(v.selected_layer_ids().is_empty());
                v.undo(cx);
                v.select_all(cx);
                v.delete_selected(cx);
                assert!(v.editor.doc.node(text).is_none());
                assert!(v.editor.doc.node(image).is_some());
                assert!(
                    design_background::parts(&v.editor.doc)
                        .unwrap()
                        .image
                        .is_some()
                );
                v.undo(cx);
                assert_eq!(v.editor.doc, authored);
            })
        });
    }
}
