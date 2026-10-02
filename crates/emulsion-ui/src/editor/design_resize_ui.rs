//! Resizing is prepared on an isolated page clone and committed only after review.
use super::*;
use emulsion_core::{
    creation::{CanvasKind, CanvasSpec, Unit},
    design_resize::{self, ResizePlan, ResizePreset},
    project::PageId,
};
use gpui_kit::component::{
    Disableable, Sizable, WindowExt,
    button::{Button, ButtonVariants},
    menu::{DropdownMenu, PopupMenuItem},
};

fn preset_label(preset: &ResizePreset) -> String {
    let key = format!("design.resize.{}", preset.key);
    t!(key.as_str()).to_string()
}

struct ResizePreview {
    owner: WeakEntity<EditorView>,
    ticket: (u64, u64),
    page: PageId,
    name: String,
    source: Document,
    fields: [Entity<InputState>; 3],
    unit: Unit,
    _subscriptions: Vec<Subscription>,
    plan: Option<ResizePlan>,
    image: Option<Arc<RenderImage>>,
    generation: u64,
    loading: bool,
    error: Option<String>,
    applied: bool,
}

impl ResizePreview {
    fn target(&self, cx: &App) -> Result<(u32, u32, f32), String> {
        let values = self
            .fields
            .each_ref()
            .map(|field| field.read(cx).value().trim().parse::<f64>());
        let [Ok(width), Ok(height), Ok(resolution)] = values else {
            return Err(t!("design.resize.invalid").to_string());
        };
        let spec = CanvasSpec {
            kind: CanvasKind::Design,
            width,
            height,
            resolution,
            unit: self.unit,
            depth: self.source.source_depth,
            ..Default::default()
        };
        let (width, height) = spec.pixel_size()?;
        Ok((width, height, resolution as f32))
    }

    fn preview_matches(&self, cx: &App) -> bool {
        self.plan.as_ref().is_some_and(|plan| {
            self.target(cx).ok() == Some((plan.doc.width, plan.doc.height, plan.doc.resolution))
        })
    }

    fn changed(&self, cx: &App) -> bool {
        self.target(cx).is_ok_and(|target| {
            target
                != (
                    self.source.width,
                    self.source.height,
                    self.source.resolution,
                )
        })
    }

    fn request_preview(&mut self, cx: &mut Context<Self>) {
        self.generation = self.generation.wrapping_add(1);
        let generation = self.generation;
        self.error = None;
        let target = match self.target(cx) {
            Ok(target) => target,
            Err(error) => {
                self.loading = false;
                self.error = Some(error);
                cx.notify();
                return;
            }
        };
        self.loading = true;
        let source = self.source.clone();
        cx.spawn(async move |this, cx| {
            let result = cx
                .background_spawn(async move {
                    let plan = design_resize::prepare(&source, target.0, target.1, target.2)?;
                    let (w, h, bytes) = doc_thumb(&plan.doc, 720);
                    Ok::<_, String>((plan, Arc::new(viewport::bgra_image(w, h, bytes))))
                })
                .await;
            this.update(cx, |this, cx| {
                if this.generation != generation {
                    return;
                }
                this.loading = false;
                match result {
                    Ok((plan, image)) => {
                        if let Some(old) = this.image.replace(image) {
                            cx.defer(move |cx| cx.drop_image(old, None));
                        }
                        this.plan = Some(plan);
                    }
                    Err(error) => {
                        this.plan = None;
                        this.error = Some(error);
                    }
                }
                cx.notify();
            })
            .ok();
        })
        .detach();
        cx.notify();
    }

    fn preset(&mut self, preset: ResizePreset, window: &mut Window, cx: &mut Context<Self>) {
        self.unit = preset.unit;
        for (field, value) in
            self.fields
                .iter()
                .zip([preset.width, preset.height, f64::from(preset.resolution)])
        {
            field.update(cx, |field, cx| {
                field.set_value(value.to_string(), window, cx)
            });
        }
        self.request_preview(cx);
    }

    fn set_unit(&mut self, unit: Unit, window: &mut Window, cx: &mut Context<Self>) {
        if self.unit == unit {
            return;
        }
        if let Err(error) = self.target(cx) {
            self.error = Some(error);
            cx.notify();
            return;
        }
        let resolution = self.fields[2]
            .read(cx)
            .value()
            .trim()
            .parse::<f64>()
            .expect("validated target");
        let old_scale = self.unit.pixels_per_unit(resolution);
        let new_scale = unit.pixels_per_unit(resolution);
        for field in &self.fields[..2] {
            let value = field
                .read(cx)
                .value()
                .trim()
                .parse::<f64>()
                .expect("validated target");
            field.update(cx, |field, cx| {
                field.set_value((value * old_scale / new_scale).to_string(), window, cx)
            });
        }
        self.error = None;
        self.unit = unit;
        cx.notify();
    }

    fn close(&mut self, window: &mut Window, cx: &mut Context<Self>) {
        self.generation = self.generation.wrapping_add(1);
        window.close_dialog(cx);
        self.owner
            .update(cx, |view, cx| window.focus(&view.canvas_focus, cx))
            .ok();
    }

    fn apply(&mut self, copy: bool, window: &mut Window, cx: &mut Context<Self>) {
        if self.applied || self.loading || !self.preview_matches(cx) || !self.changed(cx) {
            return;
        }
        let plan = self.plan.as_ref().unwrap();
        let mut review = plan.overflow.clone();
        review.extend(&plan.text_reflow);
        review.extend(&plan.text_overflow);
        review.extend(&plan.unchecked);
        review.extend(&plan.photo_coverage);
        review.sort_unstable();
        review.dedup();
        let doc = plan.doc.clone();
        let result = self
            .owner
            .update(cx, |view, cx| {
                if view.edit_ticket() != self.ticket || view.editor.active_page() != self.page {
                    return Err(t!("design.resize.stale").to_string());
                }
                if !view.prepare_page_action(cx) {
                    return Err(t!("design.resize.finish_edit").to_string());
                }
                let name = copy.then(|| {
                    format!(
                        "{} · {} × {}",
                        self.name.chars().take(160).collect::<String>(),
                        doc.width,
                        doc.height
                    )
                });
                view.editor.apply_resized_page(doc, copy, name)?;
                view.after_change(cx);
                view.cache.borrow_mut().clear();
                view.thumbs.clear();
                view.fit_pending = true;
                review.retain(|id| {
                    !emulsion_core::design_background::is_background_node(&view.editor.doc, *id)
                });
                view.set_layer_selection(review.clone(), review.first().copied());
                view.set_tool(Tool::Move, cx);
                let mut message = if copy {
                    t!("design.resize.copy_done")
                } else {
                    t!("design.resize.current_done")
                }
                .to_string();
                if !review.is_empty() {
                    message.push(' ');
                    message.push_str(&t!("design.resize.review"));
                }
                view.set_status(message, !review.is_empty(), cx);
                Ok::<_, String>(())
            })
            .unwrap_or_else(|_| Err(t!("design.resize.unavailable").to_string()));
        match result {
            Ok(()) => {
                self.applied = true;
                self.close(window, cx);
            }
            Err(error) => {
                self.error = Some(error);
                cx.notify();
            }
        }
    }

    fn warning(&self, key: &'static str, ids: &[NodeId], cx: &App) -> Option<AnyElement> {
        if ids.is_empty() {
            return None;
        }
        let p = theme::palette(cx);
        let names = ids
            .iter()
            .take(4)
            .filter_map(|id| self.source.node(*id))
            .map(|node| node.name.clone())
            .collect::<Vec<_>>()
            .join(", ");
        Some(
            div()
                .text_size(px(11.))
                .text_color(p.ink)
                .child(format!(
                    "{}: {names}{}",
                    t!(key, count = ids.len()),
                    if ids.len() > 4 { "…" } else { "" }
                ))
                .into_any_element(),
        )
    }
}

impl Render for ResizePreview {
    fn render(&mut self, window: &mut Window, cx: &mut Context<Self>) -> impl IntoElement {
        let p = theme::palette(cx);
        let matches = self.preview_matches(cx);
        let preview_height = (f32::from(window.viewport_size().height) - 455.).clamp(80., 310.);
        let selected_preset = design_resize::PRESETS
            .iter()
            .find(|preset| {
                preset.pixel_size().ok().is_some_and(|(width, height)| {
                    self.target(cx).ok() == Some((width, height, preset.resolution))
                })
            })
            .map(preset_label)
            .unwrap_or_else(|| t!("design.resize.custom").to_string());
        let owner = cx.entity();
        let units_owner = cx.entity();
        let mut checks = Vec::new();
        if matches && let Some(plan) = &self.plan {
            for (key, ids) in [
                ("design.resize.overflow", &plan.overflow),
                ("design.resize.text_wrap", &plan.text_reflow),
                ("design.resize.text_overflow", &plan.text_overflow),
                ("design.resize.photo_gap", &plan.photo_coverage),
                ("design.resize.unchecked", &plan.unchecked),
            ] {
                checks.extend(self.warning(key, ids, cx));
            }
            if plan.background_recropped {
                checks.push(
                    div()
                        .text_size(px(11.))
                        .child(t!("design.resize.background_crop").to_string())
                        .into_any_element(),
                );
            }
            if checks.is_empty() {
                checks.push(
                    div()
                        .text_size(px(11.))
                        .child(t!("design.resize.check_clear").to_string())
                        .into_any_element(),
                );
            }
        }
        div()
            .id("design-resize-dialog")
            .test_support()
            .flex()
            .flex_col()
            .gap_2()
            .min_w_0()
            .child(
                div().text_size(px(11.)).text_color(p.muted).child(
                    t!(
                        "design.resize.source",
                        name = &self.name,
                        width = self.source.width,
                        height = self.source.height
                    )
                    .to_string(),
                ),
            )
            .child(
                Button::new("design-resize-presets")
                    .label(format!("{}: {selected_preset}", t!("design.resize.preset")))
                    .accessibility_label(t!("design.resize.preset").to_string())
                    .small()
                    .outline()
                    .dropdown_menu(move |mut menu, _, _| {
                        for preset in design_resize::PRESETS {
                            let preset = *preset;
                            let target = owner.clone();
                            menu = menu.item(
                                PopupMenuItem::new(format!(
                                    "{} · {}",
                                    preset_label(&preset),
                                    preset.dimensions_label()
                                ))
                                .on_click(move |_, window, cx| {
                                    target.update(cx, |this, cx| this.preset(preset, window, cx))
                                }),
                            );
                        }
                        menu
                    }),
            )
            .child(
                div()
                    .flex()
                    .items_end()
                    .gap_2()
                    .children(
                        [
                            (0, "design.resize.width"),
                            (1, "design.resize.height"),
                            (2, "design.resize.resolution"),
                        ]
                        .map(|(i, key)| {
                            div()
                                .flex()
                                .flex_col()
                                .flex_1()
                                .min_w_0()
                                .gap_1()
                                .child(div().text_size(px(11.)).child(t!(key).to_string()))
                                .child(
                                    Input::new(&self.fields[i])
                                        .id(("design-resize-field", i))
                                        .small(),
                                )
                        }),
                    )
                    .child(
                        Button::new("design-resize-units")
                            .label(self.unit.label())
                            .small()
                            .outline()
                            .accessibility_label(t!("design.resize.unit").to_string())
                            .dropdown_menu(move |mut menu, _, _| {
                                for unit in Unit::ALL {
                                    let target = units_owner.clone();
                                    menu = menu.item(PopupMenuItem::new(unit.label()).on_click(
                                        move |_, window, cx| {
                                            target.update(cx, |this, cx| {
                                                this.set_unit(unit, window, cx)
                                            })
                                        },
                                    ));
                                }
                                menu
                            }),
                    )
                    .child(
                        Button::new("design-resize-preview")
                            .label(t!("design.resize.preview").to_string())
                            .small()
                            .outline()
                            .on_click(cx.listener(|this, _, _, cx| this.request_preview(cx))),
                    ),
            )
            .child(
                div().text_size(px(11.)).text_color(p.muted).child(
                    if self.unit == Unit::Pixels {
                        t!("design.resize.screen_hint")
                    } else {
                        t!("design.resize.print_hint")
                    }
                    .to_string(),
                ),
            )
            .child(
                div()
                    .id("design-resize-large-preview")
                    .test_support()
                    .w_full()
                    .h(px(preview_height))
                    .flex()
                    .items_center()
                    .justify_center()
                    .bg(p.soft_bg)
                    .rounded(px(6.))
                    .overflow_hidden()
                    .child(if self.loading {
                        div()
                            .text_color(p.muted)
                            .child(t!("design.resize.rendering").to_string())
                            .into_any_element()
                    } else if let Some(image) = &self.image {
                        img(image.clone())
                            .size_full()
                            .object_fit(ObjectFit::Contain)
                            .into_any_element()
                    } else {
                        div()
                            .child(t!("design.resize.unchanged").to_string())
                            .into_any_element()
                    }),
            )
            .when_some(self.plan.as_ref(), |d, plan| {
                d.child(
                    div().text_size(px(11.)).text_color(p.muted).child(
                        t!(
                            "design.resize.target",
                            width = plan.doc.width,
                            height = plan.doc.height,
                            resolution = plan.doc.resolution
                        )
                        .to_string(),
                    ),
                )
            })
            .when(!matches && !self.loading, |d| {
                d.child(
                    div()
                        .id("design-resize-dirty")
                        .test_support()
                        .text_size(px(11.))
                        .child(t!("design.resize.dirty").to_string()),
                )
            })
            .child(
                div()
                    .id("design-resize-checks")
                    .test_support()
                    .max_h(px(90.))
                    .overflow_y_scroll()
                    .flex()
                    .flex_col()
                    .gap_1()
                    .children(checks),
            )
            .child(
                div()
                    .text_size(px(10.))
                    .text_color(p.muted)
                    .child(t!("design.resize.check_scope").to_string()),
            )
            .when_some(self.error.clone(), |d, error| {
                d.child(
                    div()
                        .id("design-resize-error")
                        .test_support()
                        .text_size(px(11.))
                        .child(error),
                )
            })
            .child(
                div()
                    .text_size(px(11.))
                    .text_color(p.muted)
                    .child(t!("design.resize.uses_anchors").to_string()),
            )
    }
}

/// Actions stay in the dialog footer even when warnings make the preview scroll.
struct ResizeFooter {
    preview: Entity<ResizePreview>,
    _subscription: Subscription,
}
impl Render for ResizeFooter {
    fn render(&mut self, _: &mut Window, cx: &mut Context<Self>) -> impl IntoElement {
        let preview = self.preview.read(cx);
        let ready = preview.preview_matches(cx)
            && preview.changed(cx)
            && !preview.loading
            && !preview.applied;
        let cancel = self.preview.clone();
        let current = self.preview.clone();
        let copy = self.preview.clone();
        let p = theme::palette(cx);
        div()
            .flex()
            .flex_col()
            .gap_2()
            .w_full()
            .child(
                div()
                    .flex()
                    .flex_wrap()
                    .justify_end()
                    .gap_2()
                    .child(
                        Button::new("design-resize-cancel")
                            .label(t!("design.resize.cancel").to_string())
                            .outline()
                            .on_click(move |_, window, cx| {
                                cancel.update(cx, |preview, cx| preview.close(window, cx))
                            }),
                    )
                    .child(
                        Button::new("design-resize-current")
                            .label(t!("design.resize.current").to_string())
                            .outline()
                            .disabled(!ready)
                            .on_click(move |_, window, cx| {
                                current.update(cx, |preview, cx| preview.apply(false, window, cx))
                            }),
                    )
                    .child(
                        Button::new("design-resize-copy")
                            .label(t!("design.resize.copy").to_string())
                            .primary()
                            .disabled(!ready)
                            .on_click(move |_, window, cx| {
                                copy.update(cx, |preview, cx| preview.apply(true, window, cx))
                            }),
                    ),
            )
            .child(
                div()
                    .text_size(px(10.))
                    .text_color(p.muted)
                    .child(t!("design.resize.safe_default").to_string()),
            )
    }
}

impl EditorView {
    pub(super) fn resize_variant_dialog(&mut self, window: &mut Window, cx: &mut Context<Self>) {
        if !self.is_design() || !self.prepare_page_action(cx) {
            return;
        }
        self.open_resize_preview(window, cx);
    }

    fn open_resize_preview(
        &mut self,
        window: &mut Window,
        cx: &mut Context<Self>,
    ) -> Entity<ResizePreview> {
        let fields = [
            self.editor.doc.width.to_string(),
            self.editor.doc.height.to_string(),
            self.editor.doc.resolution.to_string(),
        ]
        .map(|value| cx.new(|cx| InputState::new(window, cx).default_value(value)));
        let source = self.editor.doc.clone();
        let owner = cx.weak_entity();
        let focus_owner = owner.clone();
        let ticket = self.edit_ticket();
        let page = self.editor.active_page();
        let name = self
            .editor
            .page_list()
            .iter()
            .find(|meta| meta.id == page)
            .map(|meta| meta.name.clone())
            .unwrap_or_else(|| self.name.clone());
        let preview = cx.new(|cx: &mut Context<ResizePreview>| {
            let subscriptions = fields
                .iter()
                .map(|field| {
                    cx.subscribe(field, |_, _, event, cx| {
                        if matches!(event, InputEvent::Change) {
                            cx.notify();
                        }
                    })
                })
                .collect();
            cx.on_release(|this, cx| {
                if let Some(image) = this.image.take() {
                    cx.defer(move |cx| cx.drop_image(image, None));
                }
            })
            .detach();
            ResizePreview {
                owner,
                ticket,
                page,
                name,
                source,
                fields,
                unit: Unit::Pixels,
                _subscriptions: subscriptions,
                plan: None,
                image: None,
                generation: 0,
                loading: false,
                error: None,
                applied: false,
            }
        });
        preview.update(cx, |this, cx| this.request_preview(cx));
        let footer = cx.new(|cx: &mut Context<ResizeFooter>| ResizeFooter {
            preview: preview.clone(),
            _subscription: cx.observe(&preview, |_, _, cx| cx.notify()),
        });
        let dialog_preview = preview.clone();
        let width = (f32::from(window.viewport_size().width) - 48.).clamp(300., 760.);
        window.open_dialog(cx, move |dialog, _, _| {
            dialog
                .title(t!("design.resize.title").to_string())
                .width(px(width))
                .child(dialog_preview.clone())
                .footer(footer.clone())
                .on_ok(|_, _, _| false)
                .on_close({
                    let focus_owner = focus_owner.clone();
                    move |_, window, cx| {
                        focus_owner
                            .update(cx, |view, cx| window.focus(&view.canvas_focus, cx))
                            .ok();
                    }
                })
        });
        preview
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use ::core::prelude::v1::test;
    use emulsion_core::{
        design::Template,
        project::{ProjectEditor, ProjectKind},
    };
    use gpui_kit::test::TestWindowExt;

    fn setup(cx: &mut TestAppContext) -> (Entity<EditorView>, &mut VisualTestContext) {
        let doc = Template::Announcement.create(400, 300).unwrap();
        let (workspace, cx) = crate::tests::open(cx, doc.clone());
        cx.simulate_resize(gpui_kit::size(px(1200.), px(900.)));
        let view = cx.update(|window, cx| {
            workspace.update(cx, |workspace, cx| {
                workspace.install_project(
                    ProjectEditor::new_project(ProjectKind::Design, doc).unwrap(),
                    "Resize workflow".into(),
                    window,
                    cx,
                )
            });
            workspace.read(cx).editor.clone().unwrap()
        });
        cx.run_until_parked();
        (view, cx)
    }

    fn open(view: &Entity<EditorView>, cx: &mut VisualTestContext) -> Entity<ResizePreview> {
        let preview = cx.update(|window, cx| {
            view.update(cx, |view, cx| {
                assert!(view.prepare_page_action(cx));
                view.open_resize_preview(window, cx)
            })
        });
        cx.run_until_parked();
        preview
    }

    fn size(
        preview: &Entity<ResizePreview>,
        width: &str,
        height: &str,
        cx: &mut VisualTestContext,
    ) {
        cx.update(|window, cx| {
            preview.update(cx, |preview, cx| {
                preview.unit = Unit::Pixels;
                for (field, value) in preview.fields.iter().zip([width, height, "72"]) {
                    field.update(cx, |field, cx| field.set_value(value, window, cx));
                }
                preview.request_preview(cx);
            })
        });
        cx.run_until_parked();
    }

    #[gpui_kit::test]
    fn resize_preview_cancel_escape_reopen_and_interrupt_leave_page_unchanged(
        cx: &mut TestAppContext,
    ) {
        let (view, cx) = setup(cx);
        let before = cx.update(|_, cx| view.read(cx).editor.doc.clone());
        for escape in [false, true] {
            let preview = open(&view, cx);
            size(&preview, "320", "500", cx);
            cx.update(|window, cx| {
                assert!(window.find("design-resize-large-preview").visible());
                assert_eq!(view.read(cx).editor.doc, before);
                assert!(!view.read(cx).editor.can_undo());
                if !escape {
                    window.click("design-resize-cancel", cx);
                }
            });
            if escape {
                cx.simulate_keystrokes("escape");
            }
            drop(preview);
            cx.run_until_parked();
            cx.update(|window, cx| {
                assert!(window.try_find("design-resize-dialog").is_none());
                assert_eq!(view.read(cx).editor.doc, before);
                assert!(view.read(cx).canvas_focus.is_focused(window));
            });
        }
        let preview = cx.update(|window, cx| {
            view.update(cx, |view, cx| {
                let preview = view.open_resize_preview(window, cx);
                window.close_dialog(cx);
                preview
            })
        });
        drop(preview);
        cx.run_until_parked();
        cx.update(|_, cx| assert_eq!(view.read(cx).editor.doc, before));
    }

    #[gpui_kit::test]
    fn resize_copy_is_safe_default_with_atomic_undo_and_repeated_apply_guard(
        cx: &mut TestAppContext,
    ) {
        let (view, cx) = setup(cx);
        let before = cx.update(|_, cx| view.read(cx).editor.doc.clone());
        let preview = open(&view, cx);
        size(&preview, "600", "450", cx);
        cx.update(|window, cx| window.click("design-resize-copy", cx));
        cx.run_until_parked();
        cx.update(|window, cx| {
            preview.update(cx, |preview, cx| preview.apply(true, window, cx));
            view.update(cx, |view, cx| {
                assert_eq!(view.editor.page_list().len(), 2);
                assert_eq!(view.editor.page(1).unwrap().doc, before);
                assert_eq!((view.editor.doc.width, view.editor.doc.height), (600, 450));
                let resized = view.editor.doc.clone();
                view.undo(cx);
                assert_eq!(view.editor.page_list().len(), 1);
                assert_eq!(view.editor.active_page(), 1);
                assert_eq!(view.editor.doc, before);
                view.redo(cx);
                assert_eq!(view.editor.page_list().len(), 2);
                assert_eq!(view.editor.doc, resized);
            });
        });
    }

    #[gpui_kit::test]
    fn resize_current_page_preserves_identity_then_saves_reopens_and_exports(
        cx: &mut TestAppContext,
    ) {
        let (view, cx) = setup(cx);
        let (before, page, meta) = cx.update(|_, cx| {
            let view = view.read(cx);
            (
                view.editor.doc.clone(),
                view.editor.active_page(),
                view.editor.page_list()[0].clone(),
            )
        });
        let preview = open(&view, cx);
        size(&preview, "500", "375", cx);
        cx.update(|window, cx| window.click("design-resize-current", cx));
        cx.run_until_parked();
        let project = cx.update(|_, cx| {
            view.update(cx, |view, cx| {
                assert_eq!(view.editor.active_page(), page);
                assert_eq!(view.editor.page_list(), &[meta]);
                let resized = view.editor.doc.clone();
                view.undo(cx);
                assert_eq!(view.editor.doc, before);
                view.redo(cx);
                assert_eq!(view.editor.doc, resized);
                view.editor.snapshot().unwrap()
            })
        });
        let dir =
            std::env::temp_dir().join(format!("emulsion-resize-export-{}", std::process::id()));
        std::fs::create_dir_all(&dir).unwrap();
        let path = dir.join("resized.emu");
        emulsion_io::project::write(&project, &path).unwrap();
        let reopened = emulsion_io::project::read(&path).unwrap();
        assert_eq!(reopened.pages[0].doc, project.pages[0].doc);
        assert_eq!(reopened.pages[0].meta.id, page);
        for (format, filename) in [
            (emulsion_io::project_export::Format::Png, "resized.zip"),
            (emulsion_io::project_export::Format::Pdf, "resized.pdf"),
        ] {
            let report = emulsion_io::project_export::write(
                &reopened,
                &[page],
                format,
                false,
                &dir.join(filename),
            )
            .unwrap();
            assert_eq!(report.pages, 1);
            assert!(std::fs::metadata(dir.join(filename)).unwrap().len() > 0);
        }
        std::fs::remove_dir_all(dir).unwrap();
    }

    #[gpui_kit::test]
    fn resize_dirty_invalid_and_stale_targets_cannot_apply(cx: &mut TestAppContext) {
        let (view, cx) = setup(cx);
        let before = cx.update(|_, cx| view.read(cx).editor.doc.clone());
        let preview = open(&view, cx);
        size(&preview, "320", "400", cx);
        cx.update(|window, cx| {
            preview.update(cx, |preview, cx| {
                preview.fields[0].update(cx, |field, cx| field.set_value("321", window, cx));
                preview.apply(true, window, cx);
                assert!(!preview.preview_matches(cx));
            })
        });
        cx.run_until_parked();
        cx.simulate_keystrokes("enter");
        cx.run_until_parked();
        cx.update(|window, cx| {
            assert!(window.find("design-resize-dirty").visible());
            assert_eq!(view.read(cx).editor.doc, before);
            preview.update(cx, |preview, cx| {
                preview.fields[0].update(cx, |field, cx| field.set_value("0", window, cx));
                preview.request_preview(cx);
            });
        });
        cx.run_until_parked();
        cx.update(|window, cx| {
            assert!(window.find("design-resize-error").visible());
            assert_eq!(view.read(cx).editor.doc, before);
        });
        size(&preview, "320", "400", cx);
        cx.update(|window, cx| {
            view.update(cx, |view, _| view.operation_epoch += 1);
            window.click("design-resize-current", cx);
        });
        cx.run_until_parked();
        cx.update(|window, cx| {
            assert!(window.find("design-resize-error").visible());
            assert_eq!(view.read(cx).editor.doc, before);
            assert!(!view.read(cx).editor.can_undo());
        });
    }

    #[gpui_kit::test]
    fn resize_repeated_preview_uses_latest_request_and_releases_images(cx: &mut TestAppContext) {
        let (view, cx) = setup(cx);
        let preview = open(&view, cx);
        let original = cx.update(|_, cx| view.read(cx).editor.doc.clone());
        let old_image = cx.update(|window, cx| {
            let image = preview.read(cx).image.clone().unwrap();
            assert!(window.has_image_atlas_entry(&image));
            preview.update(cx, |preview, cx| {
                for (width, height) in [("480", "320"), ("300", "500"), ("640", "360")] {
                    for (field, value) in preview.fields.iter().zip([width, height]) {
                        field.update(cx, |field, cx| field.set_value(value, window, cx));
                    }
                    preview.request_preview(cx);
                }
            });
            image
        });
        cx.run_until_parked();
        let last_image = cx.update(|window, cx| {
            assert!(!window.has_image_atlas_entry(&old_image));
            let latest = preview.read(cx);
            let plan = latest.plan.as_ref().unwrap();
            assert_eq!((plan.doc.width, plan.doc.height), (640, 360));
            assert!(latest.preview_matches(cx));
            assert_eq!(view.read(cx).editor.doc, original);
            let image = latest.image.clone().unwrap();
            assert!(window.has_image_atlas_entry(&image));
            image
        });
        drop(preview);
        cx.simulate_keystrokes("escape");
        cx.run_until_parked();
        cx.update(|window, cx| {
            assert!(!window.has_image_atlas_entry(&last_image));
            assert_eq!(view.read(cx).editor.doc, original);
            assert!(!view.read(cx).editor.can_undo());
        });
    }

    #[gpui_kit::test]
    fn resize_narrow_dialog_buttons_and_physical_units_are_reachable(cx: &mut TestAppContext) {
        let (view, cx) = setup(cx);
        cx.simulate_resize(gpui_kit::size(px(600.), px(700.)));
        let preview = open(&view, cx);
        size(&preview, "320", "480", cx);
        cx.update(|window, cx| {
            for id in [
                "design-resize-preview",
                "design-resize-copy",
                "design-resize-current",
                "design-resize-cancel",
            ] {
                let bounds = window.find(id).bounds();
                assert!(
                    bounds.origin.x >= px(0.) && bounds.right() <= px(600.),
                    "{id}: {bounds:?}"
                );
                assert!(
                    bounds.origin.y >= px(0.) && bounds.bottom() <= px(700.),
                    "{id}: {bounds:?}"
                );
            }
            preview.update(cx, |preview, cx| {
                for (field, value) in preview.fields.iter().zip(["320 ", " 480", "72 "]) {
                    field.update(cx, |field, cx| field.set_value(value, window, cx));
                }
                preview.set_unit(Unit::Inches, window, cx);
                assert_eq!(preview.target(cx).unwrap(), (320, 480, 72.));
                preview.set_unit(Unit::Millimeters, window, cx);
                assert_eq!(preview.target(cx).unwrap(), (320, 480, 72.));
                preview.set_unit(Unit::Pixels, window, cx);
                assert_eq!(preview.target(cx).unwrap(), (320, 480, 72.));
                preview.fields[2].update(cx, |field, cx| field.set_value("invalid", window, cx));
                preview.set_unit(Unit::Inches, window, cx);
                assert_eq!(preview.unit, Unit::Pixels);
                assert!(preview.error.is_some());
            });
        });
    }
}
