//! Templates are inspected in an isolated dialog before any page is changed.
use super::*;
use emulsion_core::{
    design::{
        Template,
        template_families::{self, Category, Selection},
    },
    project::{PageId, Project, ProjectEditor, ProjectKind, ProjectPage},
};
use gpui_kit::component::{
    Disableable, Selectable, Sizable, WindowExt,
    button::{Button, ButtonVariants},
};

#[derive(Clone)]
enum Source {
    Builtin(Template, (u32, u32)),
    Local(PathBuf),
    Family(Selection),
}
impl Source {
    fn load(self) -> Result<Project, String> {
        match self {
            Self::Builtin(template, (w, h)) => {
                let mut project =
                    ProjectEditor::new_project(ProjectKind::Design, template.create(w, h)?)?
                        .snapshot()
                        .ok_or_else(|| t!("editor.design_template_ui.unavailable"))?;
                project.pages[0].meta.name = template.label().into();
                Ok(project)
            }
            Self::Family(selection) => selection.create(),
            Self::Local(path) => {
                let project = emulsion_io::project::read(&path).map_err(|e| e.to_string())?;
                if project.kind != ProjectKind::Design {
                    return Err(t!("editor.design_template_ui.choose_design").into());
                }
                Ok(project)
            }
        }
    }
}

struct TemplatePreview {
    owner: WeakEntity<EditorView>,
    ticket: (u64, u64),
    target: PageId,
    project: Option<Project>,
    family_selection: Option<Selection>,
    project_generation: u64,
    index: usize,
    image: Option<Arc<RenderImage>>,
    image_generation: u64,
    loading: bool,
    error: Option<String>,
    applied: bool,
}

impl TemplatePreview {
    fn load_source(&mut self, source: Source, cx: &mut Context<Self>) {
        self.project_generation = self.project_generation.wrapping_add(1);
        let generation = self.project_generation;
        self.image_generation = self.image_generation.wrapping_add(1);
        self.loading = true;
        self.error = None;
        self.project = None;
        self.index = 0;
        if let Some(image) = self.image.take() {
            cx.defer(move |cx| cx.drop_image(image, None));
        }
        cx.spawn(async move |this, cx| {
            let result = cx.background_spawn(async move { source.load() }).await;
            this.update(cx, |this, cx| {
                if this.project_generation != generation || this.applied {
                    return;
                }
                this.loading = false;
                match result {
                    Ok(project) => {
                        this.project = Some(project);
                        this.load_image(cx);
                    }
                    Err(error) => this.error = Some(error),
                }
                cx.notify();
            })
            .ok();
        })
        .detach();
        cx.notify();
    }

    fn choose_family_selection(&mut self, selection: Selection, cx: &mut Context<Self>) {
        if self.applied || self.family_selection == Some(selection) {
            return;
        }
        self.family_selection = Some(selection);
        self.load_source(Source::Family(selection), cx);
    }

    fn family_choices(&self, selection: Selection, cx: &mut Context<Self>) -> impl IntoElement {
        let family = template_families::family(selection.family);
        let p = theme::palette(cx);
        div()
            .id("design-family-choices")
            .test_support()
            .flex()
            .flex_col()
            .gap_2()
            .child(
                div()
                    .flex()
                    .flex_wrap()
                    .items_center()
                    .gap_2()
                    .child(
                        div()
                            .text_size(px(12.))
                            .child(t!("design.invitation.layout")),
                    )
                    .children(family.variants.iter().enumerate().map(|(index, variant)| {
                        let variant_id = variant.id;
                        Button::new(("design-family-layout", index))
                            .label(variant.label)
                            .tooltip(variant.description)
                            .small()
                            .outline()
                            .selected(selection.variant == variant.id)
                            .on_click(cx.listener(move |this, _, _, cx| {
                                if let Some(current) = this.family_selection {
                                    this.choose_family_selection(
                                        Selection {
                                            variant: variant_id,
                                            ..current
                                        },
                                        cx,
                                    );
                                }
                            }))
                    })),
            )
            .child(
                div()
                    .flex()
                    .flex_wrap()
                    .items_center()
                    .gap_2()
                    .child(
                        div()
                            .text_size(px(12.))
                            .child(t!("design.invitation.palette")),
                    )
                    .children(family.palettes.iter().enumerate().map(|(index, palette)| {
                        Button::new(("design-family-palette", index))
                            .accessibility_label(palette.label)
                            .tooltip(palette.label)
                            .small()
                            .outline()
                            .selected(selection.palette == index)
                            .child(
                                div()
                                    .flex()
                                    .items_center()
                                    .gap_1()
                                    .children(
                                        [palette.background, palette.ink, palette.accent]
                                            .into_iter()
                                            .map(|color| {
                                                div()
                                                    .size(px(13.))
                                                    .rounded_full()
                                                    .border_1()
                                                    .border_color(p.line)
                                                    .bg(rgba(
                                                        (u32::from(color[0]) << 24)
                                                            | (u32::from(color[1]) << 16)
                                                            | (u32::from(color[2]) << 8)
                                                            | u32::from(color[3]),
                                                    ))
                                            }),
                                    )
                                    .child(div().ml_1().text_size(px(11.)).child(palette.label)),
                            )
                            .on_click(cx.listener(move |this, _, _, cx| {
                                if let Some(current) = this.family_selection {
                                    this.choose_family_selection(
                                        Selection {
                                            palette: index,
                                            ..current
                                        },
                                        cx,
                                    );
                                }
                            }))
                    })),
            )
            .child(
                div()
                    .text_size(px(11.))
                    .text_color(p.muted)
                    .child(match family.occasion {
                        Category::Wedding | Category::Birthday => t!("design.invitation.set_hint"),
                        Category::Social => t!("design.family.social_hint"),
                        Category::Posters => t!("design.family.poster_hint"),
                        Category::Presentations => t!("design.family.presentation_hint"),
                    }),
            )
    }

    fn load_image(&mut self, cx: &mut Context<Self>) {
        let Some(page) = self.project.as_ref().and_then(|p| p.pages.get(self.index)) else {
            return;
        };
        let doc = page.doc.clone();
        self.image_generation = self.image_generation.wrapping_add(1);
        let generation = self.image_generation;
        if let Some(image) = self.image.take() {
            cx.defer(move |cx| cx.drop_image(image, None));
        }
        cx.spawn(async move |this, cx| {
            let image = cx
                .background_spawn(async move {
                    let (w, h, bytes) = doc_thumb(&doc, 720);
                    Arc::new(viewport::bgra_image(w, h, bytes))
                })
                .await;
            this.update(cx, |this, cx| {
                if this.image_generation == generation {
                    this.image = Some(image);
                    cx.notify();
                }
            })
            .ok();
        })
        .detach();
    }

    fn change_page(&mut self, next: bool, cx: &mut Context<Self>) {
        if self.loading || self.applied {
            return;
        }
        let Some(project) = &self.project else {
            return;
        };
        let index = if next {
            self.index
                .saturating_add(1)
                .min(project.pages.len().saturating_sub(1))
        } else {
            self.index.saturating_sub(1)
        };
        if index != self.index {
            self.index = index;
            self.load_image(cx);
            cx.notify();
        }
    }

    fn ready_to_apply(&self) -> bool {
        !self.loading
            && !self.applied
            && self.image.is_some()
            && self
                .project
                .as_ref()
                .is_some_and(|project| project.pages.get(self.index).is_some())
    }

    fn apply_all(&mut self, window: &mut Window, cx: &mut Context<Self>) {
        if !self.ready_to_apply() {
            return;
        }
        let Some(project) = self.project.clone() else {
            return;
        };
        let result = self
            .owner
            .update(cx, |owner, cx| {
                owner.check_template_target(self.ticket, self.target, cx)?;
                owner.editor.import_pages(project)?;
                owner.finish_template_application(false, cx);
                Ok::<_, String>(())
            })
            .unwrap_or_else(|_| Err(t!("editor.design_template_ui.editor_closed").into()));
        match result {
            Ok(()) => {
                self.applied = true;
                window.close_dialog(cx);
                self.owner
                    .update(cx, |owner, cx| window.focus(&owner.canvas_focus, cx))
                    .ok();
            }
            Err(error) => {
                self.error = Some(error);
                cx.notify();
            }
        }
    }

    fn apply(&mut self, replace: bool, window: &mut Window, cx: &mut Context<Self>) {
        if !self.ready_to_apply() {
            return;
        }
        let Some(page) = self
            .project
            .as_ref()
            .and_then(|p| p.pages.get(self.index))
            .cloned()
        else {
            return;
        };
        let result = self
            .owner
            .update(cx, |owner, cx| {
                owner.apply_previewed_template(&page, replace, self.ticket, self.target, cx)
            })
            .unwrap_or_else(|_| Err(t!("editor.design_template_ui.editor_closed").into()));
        match result {
            Ok(()) => {
                self.applied = true;
                window.close_dialog(cx);
                self.owner
                    .update(cx, |owner, cx| window.focus(&owner.canvas_focus, cx))
                    .ok();
            }
            Err(error) => {
                self.error = Some(error);
                cx.notify();
            }
        }
    }
}

impl Render for TemplatePreview {
    fn render(&mut self, window: &mut Window, cx: &mut Context<Self>) -> impl IntoElement {
        let p = theme::palette(cx);
        let current = self.project.as_ref().and_then(|p| p.pages.get(self.index));
        // Keep known family controls in place while a new choice renders.
        // Otherwise the dialog recenters and moves the next click's target.
        let total = self.project.as_ref().map_or_else(
            || {
                self.family_selection.map_or(0, |selection| {
                    template_families::family(selection.family)
                        .page_labels()
                        .len()
                })
            },
            |p| p.pages.len(),
        );
        let page_info = current
            .map(|page| (page.meta.name.clone(), page.doc.width, page.doc.height))
            .or_else(|| {
                self.family_selection.map(|selection| {
                    let family = template_families::family(selection.family);
                    let (width, height) = family.native_size();
                    (
                        format!("{} · {}", family.label, family.page_labels()[0]),
                        width,
                        height,
                    )
                })
            });
        let ready = self.ready_to_apply();
        let controls_height = if self.family_selection.is_some() {
            if total > 1 { 470. } else { 420. }
        } else {
            330.
        };
        let height = (f32::from(window.viewport_size().height) - controls_height).clamp(80., 430.);
        div()
            .id("design-template-dialog")
            .test_support()
            .flex()
            .flex_col()
            .gap_3()
            .min_w_0()
            .child(
                div()
                    .text_size(px(12.))
                    .text_color(p.muted)
                    .child(t!("editor.design_template_ui.preview_only")),
            )
            .when_some(self.family_selection, |d, selection| {
                d.child(self.family_choices(selection, cx))
            })
            .child(
                div()
                    .id("design-template-large-preview")
                    .test_support()
                    .w_full()
                    .h(px(height))
                    .flex()
                    .items_center()
                    .justify_center()
                    .bg(p.soft_bg)
                    .rounded(px(8.))
                    .overflow_hidden()
                    .child(match &self.image {
                        Some(image) => img(image.clone())
                            .size_full()
                            .object_fit(ObjectFit::Contain)
                            .into_any_element(),
                        None => div()
                            .text_color(p.muted)
                            .child(if self.loading || current.is_some() {
                                t!("editor.design_template_ui.rendering")
                            } else {
                                t!("editor.design_template_ui.unavailable_preview")
                            })
                            .into_any_element(),
                    }),
            )
            .when_some(page_info, |d, (name, width, height)| {
                d.child(
                    div()
                        .flex()
                        .flex_col()
                        .gap_1()
                        .child(div().font_weight(FontWeight::SEMIBOLD).child(name))
                        .child(div().text_size(px(12.)).text_color(p.muted).child(t!(
                            "editor.design_template_ui.page_info",
                            width = width,
                            height = height,
                            page = self.index + 1,
                            total = total
                        ))),
                )
            })
            .when(total > 1, |d| {
                d.child(
                    div()
                        .flex()
                        .items_center()
                        .gap_2()
                        .child(
                            Button::new("design-template-previous")
                                .label(t!("editor.design_template_ui.previous_page"))
                                .small()
                                .outline()
                                .disabled(self.loading || self.index == 0)
                                .on_click(cx.listener(|this, _, _, cx| {
                                    this.change_page(false, cx);
                                })),
                        )
                        .child(
                            Button::new("design-template-next")
                                .label(t!("editor.design_template_ui.next_page"))
                                .small()
                                .outline()
                                .disabled(self.loading || self.index + 1 >= total)
                                .on_click(cx.listener(|this, _, _, cx| {
                                    this.change_page(true, cx);
                                })),
                        )
                        .child(
                            div()
                                .text_size(px(11.))
                                .text_color(p.muted)
                                .child(t!("editor.design_template_ui.applies_previewed")),
                        ),
                )
            })
            .when_some(self.error.clone(), |d, error| {
                d.child(
                    div()
                        .id("design-template-error")
                        .test_support()
                        .text_size(px(12.))
                        .child(error),
                )
            })
            .child(
                div()
                    .flex()
                    .flex_wrap()
                    .gap_2()
                    .justify_end()
                    .when(total > 1, |d| {
                        d.child(
                            Button::new("design-template-add-all")
                                .label(
                                    match self.family_selection.map(|selection| {
                                        template_families::family(selection.family).occasion
                                    }) {
                                        Some(Category::Social) => {
                                            t!("design.family.add_carousel", total = total)
                                        }
                                        Some(Category::Presentations) => {
                                            t!("design.family.add_slides", total = total)
                                        }
                                        Some(_) => t!("design.invitation.add_set", total = total),
                                        None => {
                                            t!("editor.design_template_ui.add_all", total = total)
                                        }
                                    },
                                )
                                .outline()
                                .disabled(!ready)
                                .on_click(
                                    cx.listener(|this, _, window, cx| this.apply_all(window, cx)),
                                ),
                        )
                    })
                    .child(
                        Button::new("design-template-cancel")
                            .label(t!("shell.cancel"))
                            .outline()
                            .on_click(|_, window, cx| window.close_dialog(cx)),
                    )
                    .child(
                        Button::new("design-template-replace")
                            .label(t!("editor.design_template_ui.replace"))
                            .outline()
                            .disabled(!ready)
                            .on_click(
                                cx.listener(|this, _, window, cx| this.apply(true, window, cx)),
                            ),
                    )
                    .child(
                        Button::new("design-template-add")
                            .label(t!("editor.design_template_ui.add"))
                            .primary()
                            .disabled(!ready)
                            .on_click(
                                cx.listener(|this, _, window, cx| this.apply(false, window, cx)),
                            ),
                    ),
            )
            .child(
                div()
                    .text_size(px(11.))
                    .text_color(p.muted)
                    .child(t!("editor.design_template_ui.replace_note")),
            )
    }
}

impl EditorView {
    pub(super) fn preview_template_family(
        &mut self,
        selection: Selection,
        window: &mut Window,
        cx: &mut Context<Self>,
    ) {
        self.open_template_preview(Source::Family(selection), window, cx);
    }

    pub(super) fn preview_design_template(
        &mut self,
        template: Template,
        size: (u32, u32),
        window: &mut Window,
        cx: &mut Context<Self>,
    ) {
        self.open_template_preview(Source::Builtin(template, size), window, cx);
    }
    pub(super) fn preview_local_template(
        &mut self,
        path: PathBuf,
        window: &mut Window,
        cx: &mut Context<Self>,
    ) {
        self.open_template_preview(Source::Local(path), window, cx);
    }
    fn open_template_preview(
        &mut self,
        source: Source,
        window: &mut Window,
        cx: &mut Context<Self>,
    ) -> Entity<TemplatePreview> {
        let family_selection = match &source {
            Source::Family(selection) => Some(*selection),
            _ => None,
        };
        let owner = cx.weak_entity();
        let ticket = self.edit_ticket();
        let target = self.editor.active_page();
        let preview = cx.new(|cx: &mut Context<TemplatePreview>| {
            cx.on_release(|this, cx| {
                if let Some(image) = this.image.take() {
                    cx.defer(move |cx| cx.drop_image(image, None));
                }
            })
            .detach();
            TemplatePreview {
                owner,
                ticket,
                target,
                project: None,
                family_selection,
                project_generation: 0,
                index: 0,
                image: None,
                image_generation: 0,
                loading: true,
                error: None,
                applied: false,
            }
        });
        preview.update(cx, |preview, cx| preview.load_source(source, cx));
        let dialog_preview = preview.clone();
        window.open_dialog(cx, move |dialog, _, _| {
            dialog
                .title(t!("editor.design_template_ui.title"))
                .width(px(760.))
                .child(dialog_preview.clone())
                .footer(div())
                .on_ok(|_, _, _| false)
        });
        preview
    }
    fn check_template_target(
        &mut self,
        ticket: (u64, u64),
        target: PageId,
        cx: &mut Context<Self>,
    ) -> Result<(), String> {
        if self.edit_ticket() != ticket || self.editor.active_page() != target {
            return Err(t!("editor.design_template_ui.page_changed").into());
        }
        if !self.prepare_page_action(cx) {
            return Err(t!("editor.design_template_ui.finish_edit").into());
        }
        Ok(())
    }
    fn finish_template_application(&mut self, replace: bool, cx: &mut Context<Self>) {
        // Template node IDs may collide with the old page's IDs. Never retain an
        // old selection that would now point to an unrelated template object.
        self.set_layer_selection(Vec::new(), None);
        self.after_change(cx);
        self.cache.borrow_mut().clear();
        self.thumbs.clear();
        self.fit_pending = true;
        self.set_tool(Tool::Move, cx);
        self.set_status(
            if replace {
                t!("editor.design_template_ui.replaced")
            } else {
                t!("editor.design_template_ui.added")
            },
            false,
            cx,
        );
    }
    fn apply_previewed_template(
        &mut self,
        page: &ProjectPage,
        replace: bool,
        ticket: (u64, u64),
        target: PageId,
        cx: &mut Context<Self>,
    ) -> Result<(), String> {
        self.check_template_target(ticket, target, cx)?;
        self.editor.apply_template_page(page, replace)?;
        self.finish_template_application(replace, cx);
        Ok(())
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use ::core::prelude::v1::test;
    use gpui_kit::test::TestWindowExt;

    pub(super) fn open_design(
        cx: &mut TestAppContext,
    ) -> (Entity<EditorView>, &mut VisualTestContext) {
        let doc = Template::Announcement.create(400, 300).unwrap();
        let (workspace, cx) = crate::tests::open(cx, doc.clone());
        cx.simulate_resize(size(px(1200.), px(900.)));
        let view = cx.update(|window, cx| {
            workspace.update(cx, |w, cx| {
                w.install_project(
                    ProjectEditor::new_project(ProjectKind::Design, doc).unwrap(),
                    "Templates".into(),
                    window,
                    cx,
                )
            });
            workspace.read(cx).editor.clone().unwrap()
        });
        // The newly installed editor loads its shared catalog asynchronously.
        // Snapshot only after it is ready, including in the complete UI suite.
        cx.run_until_parked();
        (view, cx)
    }

    #[gpui_kit::test]
    fn template_preview_cancel_add_replace_undo_and_save_export(cx: &mut TestAppContext) {
        let (view, cx) = open_design(cx);
        let (before, selected, ticket, catalog) = cx.update(|window, cx| {
            view.update(cx, |v, cx| {
                let selected = v.editor.doc.nodes.last().unwrap().id;
                v.set_layer_selection(vec![selected], Some(selected));
                let before = v.editor.doc.clone();
                let ticket = v.edit_ticket();
                let catalog =
                    serde_json::to_value((&v.creative.catalog.brands, &v.creative.catalog.assets))
                        .unwrap();
                v.preview_design_template(Template::Announcement, (320, 240), window, cx);
                (before, selected, ticket, catalog)
            })
        });
        cx.run_until_parked();
        cx.update(|window, cx| {
            assert!(window.find("design-template-large-preview").visible());
            let v = view.read(cx);
            assert_eq!(v.editor.doc, before);
            assert_eq!(v.selected, Some(selected));
            assert_eq!(v.edit_ticket(), ticket);
            assert!(!v.editor.can_undo());
            window.click("design-template-cancel", cx);
        });
        cx.run_until_parked();
        cx.update(|window, cx| {
            view.update(cx, |v, cx| {
                assert_eq!(v.editor.doc, before);
                assert_eq!(v.selected, Some(selected));
                v.preview_design_template(Template::Announcement, (320, 240), window, cx);
            })
        });
        cx.run_until_parked();
        cx.update(|window, cx| window.click("design-template-add", cx));
        cx.run_until_parked();
        cx.update(|window, cx| {
            view.update(cx, |v, cx| {
                assert_eq!(v.editor.page_list().len(), 2);
                assert_eq!(v.editor.page(1).unwrap().doc, before);
                assert_eq!((v.editor.doc.width, v.editor.doc.height), (320, 240));
                assert!(v.selected.is_none());
                v.undo(cx);
                assert_eq!(v.editor.page_list().len(), 1);
                assert_eq!(v.editor.active_page(), 1);
                assert_eq!(v.editor.doc, before);
                v.redo(cx);
                assert_eq!(v.editor.page_list().len(), 2);
                v.select_page(1, cx);
                v.preview_design_template(Template::Announcement, (280, 210), window, cx);
            })
        });
        cx.run_until_parked();
        cx.update(|window, cx| window.click("design-template-replace", cx));
        cx.run_until_parked();
        let project = cx.update(|_, cx| {
            view.update(cx, |v, cx| {
                assert_eq!(v.editor.page_list().len(), 2);
                assert_eq!(v.editor.active_page(), 1);
                assert_eq!(v.editor.page_list()[0].name, "Page 1");
                assert_eq!((v.editor.doc.width, v.editor.doc.height), (280, 210));
                assert!(v.selected.is_none());
                let replaced = v.editor.doc.clone();
                v.undo(cx);
                assert_eq!(v.editor.doc, before);
                v.redo(cx);
                assert_eq!(v.editor.doc, replaced);
                assert_eq!(
                    serde_json::to_value((&v.creative.catalog.brands, &v.creative.catalog.assets))
                        .unwrap(),
                    catalog
                );
                v.editor.snapshot().unwrap()
            })
        });
        let dir = std::env::temp_dir().join(format!(
            "emulsion-template-discovery-{}",
            std::process::id()
        ));
        std::fs::create_dir_all(&dir).unwrap();
        let path = dir.join("templates.emu");
        emulsion_io::project::write(&project, &path).unwrap();
        let reopened = emulsion_io::project::read(&path).unwrap();
        assert_eq!(reopened.pages.len(), 2);
        for (saved, original) in reopened.pages.iter().zip(&project.pages) {
            assert_eq!(saved.meta, original.meta);
            assert_eq!(saved.doc, original.doc);
        }
        let ids = reopened
            .pages
            .iter()
            .map(|page| page.meta.id)
            .collect::<Vec<_>>();
        for (format, name) in [
            (emulsion_io::project_export::Format::Png, "templates.zip"),
            (emulsion_io::project_export::Format::Pdf, "templates.pdf"),
        ] {
            let report =
                emulsion_io::project_export::write(&reopened, &ids, format, false, &dir.join(name))
                    .unwrap();
            assert_eq!(report.pages, 2);
            assert!(std::fs::metadata(dir.join(name)).unwrap().len() > 0);
        }
        std::fs::remove_dir_all(&dir).unwrap();
    }

    #[gpui_kit::test]
    fn template_preview_escape_narrow_layout_and_stale_apply_are_non_destructive(
        cx: &mut TestAppContext,
    ) {
        let (view, cx) = open_design(cx);
        cx.simulate_resize(size(px(900.), px(700.)));
        let before = cx.update(|window, cx| {
            view.update(cx, |v, cx| {
                v.preview_design_template(Template::Announcement, (240, 320), window, cx);
                v.editor.doc.clone()
            })
        });
        cx.run_until_parked();
        cx.update(|window, cx| {
            for id in [
                "design-template-cancel",
                "design-template-add",
                "design-template-replace",
            ] {
                let bounds = window.find(id).bounds();
                assert!(bounds.origin.x >= px(0.) && bounds.right() <= px(900.));
                assert!(bounds.origin.y >= px(0.) && bounds.bottom() <= px(700.));
            }
            view.update(cx, |v, _| v.operation_epoch += 1);
            window.click("design-template-replace", cx);
        });
        cx.run_until_parked();
        cx.update(|window, cx| {
            assert!(window.find("design-template-error").visible());
            assert_eq!(view.read(cx).editor.doc, before);
            assert!(!view.read(cx).editor.can_undo());
        });
        cx.simulate_keystrokes("escape");
        cx.run_until_parked();
        cx.update(|window, cx| {
            assert!(window.try_find("design-template-dialog").is_none());
            assert_eq!(view.read(cx).editor.doc, before);
        });
    }

    #[gpui_kit::test]
    fn template_application_keeps_existing_brand_kit_and_other_page(cx: &mut TestAppContext) {
        let (view, cx) = open_design(cx);
        cx.update(|_, cx| {
            view.update(cx, |v, cx| {
                v.creative
                    .catalog
                    .add_brand(
                        "Discovery brand".into(),
                        "Geist Mono".into(),
                        vec![[12, 34, 56, 255]],
                    )
                    .unwrap();
                let brand = serde_json::to_value(&v.creative.catalog.brands).unwrap();
                let original = v.editor.doc.clone();
                let project = Source::Builtin(Template::Announcement, (320, 240))
                    .load()
                    .unwrap();
                let ticket = v.edit_ticket();
                let target = v.editor.active_page();
                v.apply_previewed_template(&project.pages[0], false, ticket, target, cx)
                    .unwrap();
                assert_eq!(
                    serde_json::to_value(&v.creative.catalog.brands).unwrap(),
                    brand
                );
                assert_eq!(v.editor.page(target).unwrap().doc, original);
                let ticket = v.edit_ticket();
                let added = v.editor.active_page();
                v.apply_previewed_template(&project.pages[0], true, ticket, added, cx)
                    .unwrap();
                assert_eq!(
                    serde_json::to_value(&v.creative.catalog.brands).unwrap(),
                    brand
                );
                assert_eq!(v.editor.page(target).unwrap().doc, original);
            })
        });
    }

    #[gpui_kit::test]
    fn template_preview_releases_rasters_after_navigation_and_escape(cx: &mut TestAppContext) {
        let (view, cx) = open_design(cx);
        let preview = cx.update(|window, cx| {
            view.update(cx, |v, cx| {
                v.open_template_preview(
                    Source::Builtin(Template::Announcement, (320, 240)),
                    window,
                    cx,
                )
            })
        });
        cx.run_until_parked();
        let first = cx.update(|window, cx| {
            let image = preview.read(cx).image.clone().unwrap();
            assert!(window.has_image_atlas_entry(&image));
            preview.update(cx, |p, cx| p.load_image(cx));
            image
        });
        cx.run_until_parked();
        let last = cx.update(|window, cx| {
            assert!(!window.has_image_atlas_entry(&first));
            let image = preview.read(cx).image.clone().unwrap();
            assert!(window.has_image_atlas_entry(&image));
            image
        });
        drop(preview);
        cx.simulate_keystrokes("escape");
        cx.run_until_parked();
        cx.update(|window, _| assert!(!window.has_image_atlas_entry(&last)));
    }

    #[gpui_kit::test]
    fn local_template_preview_pages_and_cancel_while_loading(cx: &mut TestAppContext) {
        let mut local =
            ProjectEditor::new_project(ProjectKind::Design, Document::new(260, 180)).unwrap();
        local
            .add_page(
                Template::Announcement.create(320, 240).unwrap(),
                "Second design".into(),
                2.,
            )
            .unwrap();
        let dir = std::env::temp_dir().join(format!(
            "emulsion-local-template-discovery-{}",
            std::process::id()
        ));
        std::fs::create_dir_all(&dir).unwrap();
        let path = dir.join("local.emu");
        emulsion_io::project::write(&local.snapshot().unwrap(), &path).unwrap();
        let (view, cx) = open_design(cx);
        let before = cx.update(|window, cx| {
            view.update(cx, |v, cx| {
                v.preview_local_template(path.clone(), window, cx);
                window.close_dialog(cx);
                v.editor.doc.clone()
            })
        });
        cx.run_until_parked();
        cx.update(|window, cx| {
            view.update(cx, |v, cx| {
                assert_eq!(v.editor.doc, before);
                assert!(!v.editor.can_undo());
                v.preview_local_template(path.clone(), window, cx);
            })
        });
        cx.run_until_parked();
        cx.update(|window, cx| window.click("design-template-next", cx));
        cx.run_until_parked();
        cx.update(|window, cx| window.click("design-template-add", cx));
        cx.run_until_parked();
        cx.update(|_, cx| {
            view.update(cx, |v, _| {
                assert_eq!(v.editor.page_list().len(), 2);
                assert_eq!(v.editor.page_list()[1].name, "Second design");
                assert_eq!(v.editor.page_list()[1].bleed_mm, 2.);
                assert_eq!(v.editor.doc, local.doc);
                assert_eq!(v.editor.page(1).unwrap().doc, before);
            })
        });
        cx.update(|window, cx| {
            view.update(cx, |v, cx| {
                v.undo(cx);
                assert_eq!(v.editor.page_list().len(), 1);
                v.preview_local_template(path.clone(), window, cx);
            })
        });
        cx.run_until_parked();
        cx.update(|window, cx| window.click("design-template-add-all", cx));
        cx.run_until_parked();
        cx.update(|_, cx| {
            view.update(cx, |v, cx| {
                assert_eq!(v.editor.page_list().len(), 3);
                assert_eq!(v.editor.page(1).unwrap().doc, before);
                v.undo(cx);
                assert_eq!(v.editor.page_list().len(), 1);
                assert_eq!(v.editor.doc, before);
                v.redo(cx);
                assert_eq!(v.editor.page_list().len(), 3);
            })
        });
        std::fs::remove_dir_all(&dir).unwrap();
    }
}

#[cfg(test)]
#[path = "design_invitation_ui_tests.rs"]
mod invitation_tests;

#[cfg(test)]
#[path = "design_family_ui_tests.rs"]
mod family_tests;
