//! Choose the actual editable document before asking for canvas settings.
use super::*;
use emulsion_core::{
    design::Template,
    diagram_library,
    project::{ProjectEditor, ProjectKind},
};
use gpui_kit::component::menu::{DropdownMenu, PopupMenuItem};
use std::collections::{HashSet, VecDeque};

const PAGE: usize = 12;
const LOCAL: usize = usize::MAX;
#[derive(Clone)]
pub(super) enum Source {
    Design(Template),
    Diagram(diagram_library::Template),
    Local(PathBuf),
}
#[derive(Clone)]
pub(super) struct Starter {
    id: String,
    name: String,
    description: String,
    category: Option<usize>,
    source: Source,
}
impl Starter {
    fn project(
        &self,
        preview: bool,
    ) -> Result<(ProjectEditor, emulsion_io::project::ProjectReadReport), String> {
        let project = match &self.source {
            Source::Design(template) => {
                let (mut w, mut h) = template.native_size();
                if preview && !matches!(template, Template::Responsive(_)) {
                    let scale = 480. / f64::from(w.max(h));
                    w = (f64::from(w) * scale).round().max(1.) as u32;
                    h = (f64::from(h) * scale).round().max(1.) as u32;
                }
                ProjectEditor::new_project(ProjectKind::Design, template.create(w, h)?)
            }
            Source::Diagram(template) => {
                ProjectEditor::new_project(ProjectKind::Diagram, template.build()?)
            }
            Source::Local(path) if preview => {
                // A thumbnail cannot convey recovery diagnostics and is never
                // admission evidence for the subsequent full open.
                ProjectEditor::open(
                    emulsion_io::project::read(path).map_err(|e| e.to_string())?,
                    None,
                )
            }
            Source::Local(path) => {
                let opened =
                    emulsion_io::project::read_with_report(path).map_err(|e| e.to_string())?;
                return Ok((ProjectEditor::open(opened.project, None)?, opened.report));
            }
        }?;
        Ok((project, emulsion_io::project::ProjectReadReport::default()))
    }
}
#[derive(Clone)]
struct Preview {
    image: Arc<RenderImage>,
    description: String,
}
#[derive(Default)]
pub(super) struct Gallery {
    pub enabled: bool,
    pub selected: Option<Starter>,
    pub category: Option<usize>,
    pub page: usize,
    previews: HashMap<String, Result<Preview, String>>,
    loading: HashSet<String>,
}

// Built-in templates are immutable during an app session. Keep only small rendered
// previews, never the full editable documents. User templates stay dialog-local.
#[derive(Default)]
struct TemplatePreviewCache {
    entries: HashMap<String, Preview>,
    order: VecDeque<String>,
}
impl Global for TemplatePreviewCache {}
impl TemplatePreviewCache {
    fn get(&mut self, id: &str) -> Option<Preview> {
        let preview = self.entries.get(id)?.clone();
        self.order.retain(|key| key != id);
        self.order.push_back(id.to_owned());
        Some(preview)
    }
    fn insert(&mut self, id: String, preview: Preview) {
        self.order.retain(|key| key != &id);
        self.order.push_back(id.clone());
        self.entries.insert(id, preview);
        while self.order.len() > 64 {
            if let Some(key) = self.order.pop_front() {
                self.entries.remove(&key);
            }
        }
    }
}

impl NewCanvas {
    fn starters(&self, cx: &App) -> Vec<Starter> {
        let mut entries = if self.spec.kind == CanvasKind::Design {
            Template::catalog()
                .chain(Template::ADDITIONAL)
                .enumerate()
                .map(|(i, t)| {
                    let size = t.native_size();
                    Starter {
                        id: format!("design-{i}"),
                        name: catalog_label("template", t.label()),
                        description: t!(
                            "new_canvas.design_size",
                            category = t.category().map_or_else(
                                || kind_label(CanvasKind::Design).into_owned(),
                                |c| catalog_label(
                                    "template_category",
                                    Template::CATEGORIES[c].label
                                )
                            ),
                            width = size.0,
                            height = size.1
                        )
                        .into_owned(),
                        category: t.category(),
                        source: Source::Design(t),
                    }
                })
                .collect::<Vec<_>>()
        } else if self.spec.kind == CanvasKind::Diagram {
            diagram_library::TEMPLATES
                .iter()
                .copied()
                .enumerate()
                .map(|(i, t)| Starter {
                    id: format!("diagram-{i}"),
                    name: catalog_label("diagram", t.name),
                    description: catalog_text("diagram_desc", t.name, t.description),
                    category: None,
                    source: Source::Diagram(t),
                })
                .collect()
        } else {
            // Storyboards start from the templates people save.
            Vec::new()
        };
        let local = if self.spec.kind == CanvasKind::Storyboard {
            emulsion_io::creative_library::AssetKind::StoryboardTemplate
        } else {
            emulsion_io::creative_library::AssetKind::Template
        };
        if let Some(workspace) = self.workspace.upgrade() {
            entries.extend(
                workspace
                    .read(cx)
                    .home_state
                    .projects
                    .catalog
                    .assets
                    .iter()
                    .filter(|a| a.kind == local)
                    .map(|a| Starter {
                        id: format!("local-{}", a.id),
                        name: a.name.clone(),
                        description: t!("new_canvas.saved_template").into_owned(),
                        category: Some(LOCAL),
                        source: Source::Local(a.path.clone()),
                    }),
            );
        }
        let query = self.search.read(cx).value().trim().to_lowercase();
        entries.retain(|t| {
            self.templates
                .category
                .is_none_or(|c| t.category == Some(c))
                && format!("{} {}", t.name, t.description)
                    .to_lowercase()
                    .contains(&query)
        });
        entries
    }

    pub(super) fn creation_mode(&self, cx: &Context<Self>) -> AnyElement {
        crate::widgets::command_bar("new-canvas-mode-toolbar", "Document starting point")
            .children(
                [
                    (true, t!("new_canvas.templates"), "new-canvas-templates"),
                    (false, t!("new_canvas.blank_canvas"), "new-canvas-blank"),
                ]
                .into_iter()
                .filter(|_| {
                    matches!(
                        self.spec.kind,
                        CanvasKind::Design | CanvasKind::Diagram | CanvasKind::Storyboard
                    )
                })
                .map(|(enabled, label, id)| {
                    Button::new(id)
                        .label(label)
                        .small()
                        .outline()
                        .selected(self.templates.enabled == enabled)
                        .disabled(self.submitted)
                        .on_click(cx.listener(move |this, _, _, cx| {
                            this.templates.enabled = enabled;
                            this.notice = None;
                            cx.notify();
                        }))
                }),
            )
            .child(
                Button::new("new-canvas-import-photo")
                    .label(t!("new_canvas.import_photo"))
                    .small()
                    .outline()
                    .disabled(self.submitted)
                    .on_click(cx.listener(|this, _, window, cx| {
                        this.cancelled = true;
                        window.close_dialog(cx);
                        this.workspace
                            .update(cx, |workspace, cx| {
                                workspace.prompt_open_named(
                                    t!("new_canvas.import_photo_title").into_owned(),
                                    true,
                                    window,
                                    cx,
                                );
                            })
                            .ok();
                    })),
            )
            .into_any_element()
    }

    fn load_starter_previews(&mut self, entries: &[Starter], cx: &mut Context<Self>) {
        if !cx.has_global::<TemplatePreviewCache>() {
            cx.set_global(TemplatePreviewCache::default());
        }
        // The selected preview comes first even when the category/page changes.
        let candidates: Vec<_> = self
            .templates
            .selected
            .iter()
            .chain(entries.iter().skip(self.templates.page * PAGE).take(PAGE))
            .cloned()
            .collect();
        let visible: HashSet<_> = candidates.iter().map(|t| t.id.clone()).collect();
        self.templates
            .previews
            .retain(|key, _| visible.contains(key));
        for t in &candidates {
            if !self.templates.previews.contains_key(&t.id)
                && let Some(preview) = cx.global_mut::<TemplatePreviewCache>().get(&t.id)
            {
                self.templates.previews.insert(t.id.clone(), Ok(preview));
            }
        }
        for t in candidates {
            if self.templates.previews.contains_key(&t.id) || self.templates.loading.contains(&t.id)
            {
                continue;
            }
            // Deliver each preview immediately; don't wait for a slow batch to finish.
            if self.templates.loading.len() >= 2 {
                break;
            }
            self.templates.loading.insert(t.id.clone());
            let cacheable = !matches!(t.source, Source::Local(_));
            let id = t.id.clone();
            cx.spawn(async move |this, cx| {
                let preview = cx
                    .background_spawn(async move {
                        t.project(true).and_then(|(project, _)| {
                            let size = match &t.source {
                                Source::Design(t) => t.native_size(),
                                _ => (project.doc.width, project.doc.height),
                            };
                            let kind = if project.kind() == Some(ProjectKind::Diagram) {
                                CanvasKind::Diagram
                            } else {
                                CanvasKind::Design
                            };
                            let description = t!(
                                "new_canvas.preview_size",
                                kind = kind_label(kind),
                                width = size.0,
                                height = size.1,
                                pages = project.page_list().len()
                            )
                            .into_owned();
                            let (w, h, bytes) = crate::editor::doc_thumb(&project.doc, 216)?;
                            Ok(Preview {
                                image: Arc::new(crate::viewport::bgra_image(w, h, bytes)),
                                description,
                            })
                        })
                    })
                    .await;
                this.update(cx, |this, cx| {
                    if cacheable && let Ok(preview) = &preview {
                        cx.global_mut::<TemplatePreviewCache>()
                            .insert(id.clone(), preview.clone());
                    }
                    this.templates.loading.remove(&id);
                    this.templates.previews.insert(id, preview);
                    cx.notify();
                })
                .ok();
            })
            .detach();
        }
    }

    pub(super) fn submit_template(&mut self, window: &mut Window, cx: &mut Context<Self>) {
        if self.submitted {
            return;
        }
        let Some(template) = self.templates.selected.clone() else {
            self.notice = Some(t!("new_canvas.choose_template").into_owned());
            cx.notify();
            return;
        };
        let name = self.fields[0].read(cx).value().trim().to_string();
        if name.is_empty() || name.chars().count() > 200 || name.chars().any(char::is_control) {
            self.notice = Some(t!("new_canvas.err_name").into_owned());
            cx.notify();
            return;
        }
        self.submitted = true;
        cx.notify();
        cx.spawn_in(window, async move |this, cx| {
            let result = cx
                .background_spawn(async move { template.project(false) })
                .await;
            this.update_in(cx, |this, window, cx| {
                if this.cancelled {
                    return;
                }
                match result {
                    Ok((project, report)) => {
                        // A copy of the template, unsaved, with fresh history.
                        let spec = CanvasSpec {
                            name,
                            kind: match project.kind() {
                                Some(ProjectKind::Diagram) => CanvasKind::Diagram,
                                Some(ProjectKind::Storyboard) => CanvasKind::Storyboard,
                                _ => CanvasKind::Design,
                            },
                            width: f64::from(project.doc.width),
                            height: f64::from(project.doc.height),
                            resolution: f64::from(project.doc.resolution),
                            pages: project.page_list().len(),
                            ..Default::default()
                        };
                        if this.install_created(
                            spec,
                            project.doc.clone(),
                            Some(project),
                            window,
                            cx,
                        ) {
                            this.workspace
                                .update(cx, |workspace, cx| {
                                    workspace.show_project_open_notes(report.warnings(), false, cx);
                                })
                                .ok();
                            window.close_dialog(cx);
                        }
                    }
                    Err(error) => {
                        this.submitted = false;
                        this.notice = Some(
                            t!("new_canvas.template_failed", error = core_error(error))
                                .into_owned(),
                        );
                        cx.notify();
                    }
                }
            })
            .ok();
        })
        .detach();
    }

    pub(super) fn template_gallery(
        &mut self,
        _window: &mut Window,
        cx: &mut Context<Self>,
    ) -> AnyElement {
        let p = theme::palette(cx);
        let entries = self.starters(cx);
        self.templates.page = self
            .templates
            .page
            .min(entries.len().saturating_sub(1) / PAGE);
        self.load_starter_previews(&entries, cx);
        let selected = self.templates.selected.as_ref();
        let labels: Vec<(Option<usize>, String)> =
            std::iter::once((None, t!("new_canvas.all_templates").into_owned()))
                .chain(
                    Template::CATEGORIES
                        .iter()
                        .enumerate()
                        .filter(|_| self.spec.kind == CanvasKind::Design)
                        .map(|(i, c)| (Some(i), catalog_label("template_category", c.label))),
                )
                .chain(std::iter::once((
                    Some(LOCAL),
                    t!("new_canvas.my_templates").into_owned(),
                )))
                .collect();
        let category = self.templates.category;
        let label = labels.iter().find(|(id, _)| *id == category).map_or_else(
            || t!("new_canvas.all_templates").into_owned(),
            |(_, label)| label.clone(),
        );
        let owner = cx.weak_entity();
        let categories = Button::new("new-template-category-select")
            .label(format!("{label} ▾"))
            .accessibility_label(t!("new_canvas.category_label", category = label.as_str()))
            .small()
            .outline()
            .disabled(self.submitted)
            .dropdown_menu(move |mut menu, _, _| {
                for (id, label) in &labels {
                    let (id, owner) = (*id, owner.clone());
                    menu = menu.item(
                        PopupMenuItem::new(label.clone())
                            .checked(id == category)
                            .on_click(move |_, _, cx| {
                                owner
                                    .update(cx, |this, cx| {
                                        this.templates.category = id;
                                        this.templates.page = 0;
                                        cx.notify();
                                    })
                                    .ok();
                            }),
                    );
                }
                menu
            });
        let blank = Button::new("new-template-blank")
            .accessibility_label(t!(
                "new_canvas.blank_a11y",
                kind = kind_label(self.spec.kind)
            ))
            .outline()
            .w_full()
            .h(px(72.))
            .p_3()
            .disabled(self.submitted)
            .child(
                div()
                    .flex()
                    .items_center()
                    .gap_3()
                    .w_full()
                    .child(
                        div()
                            .w(px(30.))
                            .h(px(38.))
                            .flex_none()
                            .border_1()
                            .border_color(p.line)
                            .bg(p.paper),
                    )
                    .child(
                        div()
                            .flex()
                            .flex_col()
                            .items_start()
                            .gap_1()
                            .child(div().child(t!("new_canvas.blank_document")))
                            .child(
                                div()
                                    .text_xs()
                                    .text_color(p.muted)
                                    .child(t!("new_canvas.blank_hint")),
                            ),
                    ),
            )
            .on_click(cx.listener(|this, _, window, cx| {
                this.templates.enabled = false;
                this.templates.selected = None;
                this.notice = None;
                let name = untitled(this.spec.kind);
                this.fields[0].update(cx, |field, cx| field.set_value(name, window, cx));
                cx.notify();
            }));
        let mut grid = div()
            .id("new-template-grid")
            .test_support()
            .grid()
            .grid_cols(2)
            .gap_2();
        for template in entries.iter().skip(self.templates.page * PAGE).take(PAGE) {
            let pick = template.clone();
            let preview = self.templates.previews.get(&template.id);
            grid = grid.child(
                Button::new(SharedString::from(format!("new-template-{}", template.id)))
                    .accessibility_label(template.name.clone())
                    .tooltip(template.description.clone())
                    .outline()
                    .w_full()
                    .h(px(174.))
                    .p_2()
                    .selected(selected.is_some_and(|s| s.id == template.id))
                    .disabled(self.submitted)
                    .child(
                        div()
                            .flex()
                            .flex_col()
                            .min_w_0()
                            .w_full()
                            .gap_1()
                            .child(
                                div()
                                    .h(px(110.))
                                    .w_full()
                                    .flex()
                                    .items_center()
                                    .justify_center()
                                    .bg(p.soft_bg)
                                    .child(match preview {
                                        Some(Ok(preview)) => img(preview.image.clone())
                                            .size_full()
                                            .object_fit(ObjectFit::Contain)
                                            .into_any_element(),
                                        Some(Err(_)) => div()
                                            .text_color(p.muted)
                                            .child(t!("new_canvas.preview_unavailable"))
                                            .into_any_element(),
                                        None => div()
                                            .text_color(p.muted)
                                            .child(t!("new_canvas.loading_preview"))
                                            .into_any_element(),
                                    }),
                            )
                            .child(div().text_ellipsis().child(template.name.clone()))
                            .child(
                                div()
                                    .text_size(px(10.))
                                    .text_color(p.muted)
                                    .child(template.description.clone()),
                            ),
                    )
                    .on_click(cx.listener(move |this, _, window, cx| {
                        if this.submitted {
                            return;
                        }
                        this.fields[0].update(cx, |input, cx| {
                            input.set_value(pick.name.clone(), window, cx)
                        });
                        this.templates.selected = Some(pick.clone());
                        this.notice = None;
                        cx.notify();
                    })),
            );
        }
        let mut details = div().w(px(220.)).flex_none().flex().flex_col().gap_3();
        if let Some(template) = selected {
            if let Some(Ok(preview)) = self.templates.previews.get(&template.id) {
                details = details
                    .child(
                        img(preview.image.clone())
                            .w_full()
                            .h(px(150.))
                            .object_fit(ObjectFit::Contain),
                    )
                    .child(
                        div()
                            .text_size(px(11.))
                            .text_color(p.muted)
                            .child(preview.description.clone()),
                    );
            }
            details = details
                .child(editable_field("Name", &self.fields[0], self.submitted))
                .child(self.project_destination(cx))
                .child(
                    div()
                        .text_color(p.muted)
                        .child(t!("new_canvas.template_sets_size")),
                );
        } else {
            details = details
                .child(div().text_lg().child(t!("new_canvas.how_start")))
                .child(
                    div()
                        .text_color(p.muted)
                        .child(t!("new_canvas.how_start_body")),
                );
        }
        div()
            .id("new-canvas-form")
            .test_support()
            .flex()
            .flex_col()
            .gap_3()
            .text_size(px(12.))
            .text_color(p.ink)
            .child(self.creation_mode(cx))
            .child(
                div()
                    .id("new-canvas-types")
                    .test_support()
                    .flex()
                    .flex_wrap()
                    .gap_2()
                    .children(CanvasKind::ALL.map(|kind| {
                        Button::new(("new-canvas-kind", kind as usize))
                            .label(kind_label(kind))
                            .small()
                            .ghost()
                            .selected(self.spec.kind == kind)
                            .disabled(self.submitted)
                            .on_click(cx.listener(move |this, _, window, cx| {
                                this.pick_kind(kind, window, cx)
                            }))
                    })),
            )
            .child(
                div().id("new-canvas-scroll").child(
                    div()
                        .flex()
                        .flex_wrap()
                        .gap_4()
                        .child(
                            div()
                                .flex_1()
                                .min_w(px(240.))
                                .flex()
                                .flex_col()
                                .gap_3()
                                .child(
                                    div()
                                        .id("new-canvas-search")
                                        .test_support()
                                        .child(Input::new(&self.search).small()),
                                )
                                .child(blank)
                                .child(categories)
                                .child(grid)
                                .when(entries.is_empty(), |d| d.child(t!("new_canvas.no_matches")))
                                .when(entries.len() > PAGE, |d| {
                                    d.child(
                                        div()
                                            .flex()
                                            .items_center()
                                            .gap_2()
                                            .child(
                                                Button::new("new-template-prev")
                                                    .label(t!("new_canvas.previous"))
                                                    .small()
                                                    .disabled(
                                                        self.templates.page == 0 || self.submitted,
                                                    )
                                                    .on_click(cx.listener(|this, _, _, cx| {
                                                        this.templates.page =
                                                            this.templates.page.saturating_sub(1);
                                                        cx.notify();
                                                    })),
                                            )
                                            .child(t!(
                                                "new_canvas.page_range",
                                                from = self.templates.page * PAGE + 1,
                                                to = ((self.templates.page + 1) * PAGE)
                                                    .min(entries.len()),
                                                total = entries.len()
                                            ))
                                            .child(
                                                Button::new("new-template-next")
                                                    .label(t!("new_canvas.next"))
                                                    .small()
                                                    .disabled(
                                                        (self.templates.page + 1) * PAGE
                                                            >= entries.len()
                                                            || self.submitted,
                                                    )
                                                    .on_click(cx.listener(|this, _, _, cx| {
                                                        this.templates.page += 1;
                                                        cx.notify();
                                                    })),
                                            ),
                                    )
                                }),
                        )
                        .child(details),
                ),
            )
            .into_any_element()
    }

    /// The template gallery's notice and Cancel / Use template row, shown in
    /// the dialog footer.
    pub(super) fn template_actions(
        &mut self,
        _window: &mut Window,
        cx: &mut Context<Self>,
    ) -> AnyElement {
        let p = theme::palette(cx);
        let selected = self.templates.selected.as_ref();
        div()
            .flex()
            .flex_col()
            .gap_3()
            .text_size(px(12.))
            .text_color(p.ink)
            .when_some(self.notice.clone(), |d, notice| {
                d.child(div().text_color(p.muted).child(notice))
            })
            .child(
                div()
                    .flex()
                    .flex_none()
                    .items_center()
                    .justify_end()
                    .gap_2()
                    .child(
                        Button::new("new-canvas-cancel")
                            .label(t!("new_canvas.cancel"))
                            .small()
                            .on_click(cx.listener(|this, _, window, cx| {
                                this.cancelled = true;
                                window.close_dialog(cx);
                            })),
                    )
                    .child(
                        Button::new("new-canvas-create")
                            .label(if self.submitted {
                                t!("new_canvas.creating")
                            } else {
                                t!("new_canvas.use_template")
                            })
                            .small()
                            .primary()
                            .disabled(selected.is_none() || self.submitted)
                            .on_click(
                                cx.listener(|this, _, window, cx| this.submit_template(window, cx)),
                            ),
                    ),
            )
            .into_any_element()
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use core::prelude::v1::test;
    use gpui_kit::test::TestWindowExt;

    fn local_starter(path: PathBuf) -> Starter {
        Starter {
            id: "local-recovery-test".into(),
            name: "Recovered template".into(),
            description: String::new(),
            category: Some(LOCAL),
            source: Source::Local(path),
        }
    }

    #[test]
    fn builtins_and_clean_local_templates_have_empty_reports() {
        for source in [
            Source::Design(Template::Announcement),
            Source::Diagram(diagram_library::TEMPLATES[0]),
        ] {
            let starter = Starter {
                source,
                ..local_starter(PathBuf::new())
            };
            for preview in [false, true] {
                assert!(starter.project(preview).unwrap().1.is_empty());
            }
        }
        let dir = tempfile::tempdir().unwrap();
        let path = dir.path().join("clean.emu");
        let (project, _, _) = Workspace::native_recovery_fixture(false);
        emulsion_io::project::write(&project, &path).unwrap();
        let starter = local_starter(path);
        for preview in [false, true] {
            assert!(starter.project(preview).unwrap().1.is_empty());
        }
    }

    #[gpui_kit::test]
    fn local_template_recovery_warns_after_actual_creation_while_preview_is_strict(
        cx: &mut TestAppContext,
    ) {
        let dir = tempfile::tempdir().unwrap();
        let path = dir.path().join("recovering-template.emu");
        let (project, _, _) = Workspace::native_recovery_fixture(true);
        emulsion_io::project::write(&project, &path).unwrap();
        let original = std::fs::read(&path).unwrap();
        let warnings = emulsion_io::project::read_with_report(&path)
            .unwrap()
            .report
            .warnings();
        let starter = local_starter(path.clone());
        assert!(
            starter.project(true).is_err(),
            "preview must not silently recover"
        );
        let (ws, cx) = crate::tests::open(cx, Document::new(8, 8));
        let view = cx.update(|window, cx| {
            let view = cx.new(|cx| {
                let mut view = NewCanvas::new(ws.downgrade(), None, window, cx);
                view.pick_kind(CanvasKind::Storyboard, window, cx);
                view.templates.selected = Some(starter);
                view
            });
            view.update(cx, |view, cx| view.submit_template(window, cx));
            view
        });
        cx.run_until_parked();
        cx.update(|window, cx| {
            let message = window.find("editor-status-message");
            assert!(message.visible());
            assert!(view.read(cx).notice.is_none());
            let workspace = ws.read(cx);
            assert_eq!(workspace.tabs.len(), 2);
            let editor = workspace.editor.as_ref().unwrap().read(cx);
            assert!(
                editor.editor.path.is_none(),
                "a template is an unsaved copy"
            );
            assert_eq!(editor.editor.board_versions().len(), 1);
            let (message, warning) = editor.status.as_ref().unwrap();
            assert!(*warning);
            assert_eq!(message.matches(warnings[0].as_str()).count(), 1);
        });
        assert_eq!(std::fs::read(path).unwrap(), original);
    }

    #[test]
    fn template_cache_reuses_images_and_evicts_the_least_recent_preview() {
        let image = Arc::new(crate::viewport::bgra_image(1, 1, vec![0, 0, 0, 255]));
        let mut cache = TemplatePreviewCache::default();
        for i in 0..64 {
            cache.insert(
                format!("design-{i}"),
                Preview {
                    image: image.clone(),
                    description: String::new(),
                },
            );
        }
        assert!(Arc::ptr_eq(&cache.get("design-0").unwrap().image, &image));
        cache.insert(
            "design-64".into(),
            Preview {
                image,
                description: String::new(),
            },
        );
        assert_eq!(cache.entries.len(), 64);
        assert!(cache.get("design-0").is_some());
        assert!(cache.get("design-1").is_none());
    }
}
