//! Choose the actual editable document before asking for canvas settings.
use super::*;
use emulsion_core::{
    design::Template,
    diagram_library,
    project::{ProjectEditor, ProjectKind},
};

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
    fn project(&self, preview: bool) -> Result<ProjectEditor, String> {
        match &self.source {
            Source::Design(template) => {
                let (mut w, mut h) = template.native_size();
                if preview {
                    let scale = 480. / f64::from(w.max(h));
                    w = (f64::from(w) * scale).round().max(1.) as u32;
                    h = (f64::from(h) * scale).round().max(1.) as u32;
                }
                ProjectEditor::new_project(ProjectKind::Design, template.create(w, h)?)
            }
            Source::Diagram(template) => {
                ProjectEditor::new_project(ProjectKind::Diagram, template.build()?)
            }
            Source::Local(path) => ProjectEditor::open(
                emulsion_io::project::read(path).map_err(|e| e.to_string())?,
                None,
            ),
        }
    }
}
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
    loading: bool,
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
                        name: t.label().into(),
                        description: format!(
                            "{} · {} × {} px",
                            t.category()
                                .map_or("Design", |c| Template::CATEGORIES[c].label),
                            size.0,
                            size.1
                        ),
                        category: t.category(),
                        source: Source::Design(t),
                    }
                })
                .collect::<Vec<_>>()
        } else {
            diagram_library::TEMPLATES
                .iter()
                .copied()
                .enumerate()
                .map(|(i, t)| Starter {
                    id: format!("diagram-{i}"),
                    name: t.name.into(),
                    description: t.description.into(),
                    category: None,
                    source: Source::Diagram(t),
                })
                .collect()
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
                    .filter(|a| a.kind == emulsion_io::creative_library::AssetKind::Template)
                    .map(|a| Starter {
                        id: format!("local-{}", a.id),
                        name: a.name.clone(),
                        description: "Saved template".into(),
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
        div()
            .flex()
            .gap_2()
            .children(
                [
                    (true, "Templates", "new-canvas-templates"),
                    (false, "Blank canvas", "new-canvas-blank"),
                ]
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
            .into_any_element()
    }

    fn load_starter_previews(&mut self, entries: &[Starter], cx: &mut Context<Self>) {
        if self.templates.loading {
            return;
        }
        let visible: std::collections::HashSet<_> = entries
            .iter()
            .skip(self.templates.page * PAGE)
            .take(PAGE)
            .map(|t| t.id.clone())
            .chain(self.templates.selected.iter().map(|t| t.id.clone()))
            .collect();
        self.templates
            .previews
            .retain(|key, _| visible.contains(key));
        let batch: Vec<_> = entries
            .iter()
            .skip(self.templates.page * PAGE)
            .take(PAGE)
            .filter(|t| !self.templates.previews.contains_key(&t.id))
            .take(4)
            .cloned()
            .collect();
        if batch.is_empty() {
            return;
        }
        self.templates.loading = true;
        cx.spawn(async move |this, cx| {
            let results = cx
                .background_spawn(async move {
                    batch
                        .into_iter()
                        .map(|t| {
                            let preview = t.project(true).map(|project| {
                                let size = match &t.source {
                                    Source::Design(t) => t.native_size(),
                                    _ => (project.doc.width, project.doc.height),
                                };
                                let description = format!(
                                    "{} · {} × {} px · {} page(s)",
                                    project.kind().unwrap().label(),
                                    size.0,
                                    size.1,
                                    project.page_list().len()
                                );
                                let (w, h, bytes) = crate::editor::doc_thumb(&project.doc, 216);
                                Preview {
                                    image: Arc::new(crate::viewport::bgra_image(w, h, bytes)),
                                    description,
                                }
                            });
                            (t.id, preview)
                        })
                        .collect::<Vec<_>>()
                })
                .await;
            this.update(cx, |this, cx| {
                this.templates.previews.extend(results);
                this.templates.loading = false;
                cx.notify();
            })
            .ok();
        })
        .detach();
    }

    pub(super) fn submit_template(&mut self, window: &mut Window, cx: &mut Context<Self>) {
        if self.submitted {
            return;
        }
        let Some(template) = self.templates.selected.clone() else {
            self.notice = Some("Choose a template to start from.".into());
            cx.notify();
            return;
        };
        let name = self.fields[0].read(cx).value().trim().to_string();
        if name.is_empty() || name.chars().count() > 200 || name.chars().any(char::is_control) {
            self.notice = Some("Enter a document name of 1–200 characters.".into());
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
                    Ok(project) => {
                        let spec = CanvasSpec {
                            name,
                            kind: if project.kind() == Some(ProjectKind::Diagram) {
                                CanvasKind::Diagram
                            } else {
                                CanvasKind::Design
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
                            window.close_dialog(cx);
                        }
                    }
                    Err(error) => {
                        this.submitted = false;
                        this.notice = Some(format!("Could not create template: {error}"));
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
        window: &mut Window,
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
        let mut categories = div().flex().flex_wrap().gap_1();
        let labels = std::iter::once((None, "All templates"))
            .chain(
                Template::CATEGORIES
                    .iter()
                    .enumerate()
                    .filter(|_| self.spec.kind == CanvasKind::Design)
                    .map(|(i, c)| (Some(i), c.label)),
            )
            .chain(std::iter::once((Some(LOCAL), "My templates")));
        for (i, (category, label)) in labels.enumerate() {
            categories = categories.child(
                Button::new(("new-template-category", i))
                    .label(label)
                    .xsmall()
                    .ghost()
                    .selected(self.templates.category == category)
                    .disabled(self.submitted)
                    .on_click(cx.listener(move |this, _, _, cx| {
                        this.templates.category = category;
                        this.templates.page = 0;
                        cx.notify();
                    })),
            );
        }
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
                                            .child("Preview unavailable")
                                            .into_any_element(),
                                        None => div()
                                            .text_color(p.muted)
                                            .child("Loading preview…")
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
                        .child("The template sets the canvas size. Everything remains editable."),
                );
        } else {
            details = details
                .child(div().text_lg().child("Start with a template"))
                .child(div().text_color(p.muted).child(
                    "Choose a layout to preview it. Its pages, size and content come with it.",
                ));
        }
        div().id("new-canvas-form").test_support().flex().flex_col().gap_3().text_size(px(12.)).text_color(p.ink)
            .child(self.creation_mode(cx))
            .child(div().id("new-canvas-types").test_support().flex().flex_wrap().gap_2().children(CanvasKind::ALL.map(|kind| {
                Button::new(("new-canvas-kind", kind as usize)).label(kind.label()).small().ghost().selected(self.spec.kind == kind).disabled(self.submitted)
                    .on_click(cx.listener(move |this, _, window, cx| this.pick_kind(kind, window, cx)))
            })))
            .child(div().id("new-canvas-scroll").h((window.viewport_size().height - px(360.)).clamp(px(150.), px(520.))).flex_none().overflow_y_scroll()
                .child(div().flex().flex_wrap().gap_4()
                    .child(div().flex_1().min_w(px(240.)).flex().flex_col().gap_3()
                        .child(div().id("new-canvas-search").test_support().child(Input::new(&self.search).small()))
                        .child(categories).child(grid)
                        .when(entries.is_empty(), |d| d.child("No matching templates. Try another search or category."))
                        .when(entries.len() > PAGE, |d| d.child(div().flex().items_center().gap_2()
                            .child(Button::new("new-template-prev").label("Previous").small().disabled(self.templates.page == 0 || self.submitted)
                                .on_click(cx.listener(|this, _, _, cx| { this.templates.page = this.templates.page.saturating_sub(1); cx.notify(); })))
                            .child(format!("{}–{} of {}", self.templates.page * PAGE + 1, ((self.templates.page + 1) * PAGE).min(entries.len()), entries.len()))
                            .child(Button::new("new-template-next").label("Next").small().disabled((self.templates.page + 1) * PAGE >= entries.len() || self.submitted)
                                .on_click(cx.listener(|this, _, _, cx| { this.templates.page += 1; cx.notify(); }))))))
                    .child(details)))
            .when_some(self.notice.clone(), |d, notice| d.child(div().text_color(p.muted).child(notice)))
            .child(div().flex().flex_none().items_center().justify_end().gap_2()
                .child(Button::new("new-canvas-cancel").label("Cancel").small().on_click(cx.listener(|this, _, window, cx| { this.cancelled = true; window.close_dialog(cx); })))
                .child(Button::new("new-canvas-create").label(if self.submitted { "Creating…" } else { "Use template" }).small().primary().disabled(selected.is_none() || self.submitted)
                    .on_click(cx.listener(|this, _, window, cx| this.submit_template(window, cx)))))
            .into_any_element()
    }
}
