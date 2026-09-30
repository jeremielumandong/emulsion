//! Native Home layout; every card refers to a local project or a working action.
use super::*;
use emulsion_core::{
    creation::CanvasKind,
    project::{ProjectEditor, ProjectKind},
};
use gpui_kit::component::Icon;

/// Keep the card's natural layout, but avoid preparing hitboxes, text and image
/// drawing for cards completely outside the scroll viewport.
pub(super) struct VisibleCard(pub AnyElement);

impl IntoElement for VisibleCard {
    type Element = Self;

    fn into_element(self) -> Self {
        self
    }
}

impl Element for VisibleCard {
    type RequestLayoutState = ();
    type PrepaintState = bool;

    fn id(&self) -> Option<ElementId> {
        None
    }

    fn source_location(&self) -> Option<&'static std::panic::Location<'static>> {
        None
    }

    fn request_layout(
        &mut self,
        _: Option<&GlobalElementId>,
        _: Option<&InspectorElementId>,
        window: &mut Window,
        cx: &mut App,
    ) -> (LayoutId, ()) {
        (self.0.request_layout(window, cx), ())
    }

    fn prepaint(
        &mut self,
        _: Option<&GlobalElementId>,
        _: Option<&InspectorElementId>,
        bounds: Bounds<Pixels>,
        _: &mut (),
        window: &mut Window,
        cx: &mut App,
    ) -> bool {
        let visible = bounds.intersects(&window.content_mask().bounds);
        if visible {
            self.0.prepaint(window, cx);
        }
        visible
    }

    fn paint(
        &mut self,
        _: Option<&GlobalElementId>,
        _: Option<&InspectorElementId>,
        _: Bounds<Pixels>,
        _: &mut (),
        visible: &mut bool,
        window: &mut Window,
        cx: &mut App,
    ) {
        if *visible {
            self.0.paint(window, cx);
        }
    }
}

pub(super) fn icon(name: &str, size: f32) -> Icon {
    Icon::empty()
        .path(format!("icons/{name}.svg"))
        .size(px(size))
}
pub(super) fn destination_icon(destination: Destination) -> &'static str {
    match destination {
        Destination::Home => "house",
        Destination::Photo => "image",
        Destination::Paint => "brush",
        Destination::Design => "layout-template",
        Destination::Diagram => "workflow",
        Destination::Library => "library",
    }
}
pub(super) fn folder_color(id: u64) -> Hsla {
    rgb([0xd93a1e, 0x7a5cf5, 0x2c9ca3, 0xc59a48, 0x5086c1][id as usize % 5]).into()
}
fn button(id: impl Into<ElementId>, label: impl Into<SharedString>) -> Button {
    let label = label.into();
    Button::new(id)
        .accessibility_label(label.clone())
        .small()
        .outline()
        .h(px(30.))
        .child(div().text_size(px(12.)).child(label))
}

impl Workspace {
    pub(super) fn home_empty(&self, cx: &Context<Self>) -> AnyElement {
        let filtered = !self.home_search_query(cx).trim().is_empty()
            || self.home_state.projects.kind.is_some()
            || self.home_state.filter != HomeFilter::All
            || self.home_state.folder.is_some();
        let trash = self.home_state.projects.trash;
        let (title, description, glyph) = if filtered {
            (
                "No matching files",
                "Try another search or clear filters in this project.",
                "search",
            )
        } else if trash {
            (
                "Trash is empty",
                "Files moved to Trash will appear here.",
                "trash",
            )
        } else if self.home_state.projects.folder.is_some() {
            (
                "This project is ready",
                "Open a file or create something new to add work to this project.",
                "folder-open",
            )
        } else {
            (
                "Your work starts here",
                "Open a photo or start a new document. Saved work will appear here.",
                "image",
            )
        };
        let mut state = crate::widgets::empty_state(glyph, title, description);
        if filtered {
            state = state.child(
                Button::new("home-empty-clear")
                    .label("Clear filters")
                    .small()
                    .outline()
                    .on_click(cx.listener(|this, _, window, cx| {
                        this.home_state.projects.kind = None;
                        this.home_state.filter = HomeFilter::All;
                        this.home_state.folder = None;
                        this.home_state.page = 0;
                        if let Some((input, _)) = &this.home_state.search {
                            input.update(cx, |input, cx| input.set_value("", window, cx));
                        }
                        cx.notify();
                    })),
            );
        } else if !trash {
            state = state.child(
                crate::widgets::command_bar("home-empty-actions", "Start a document")
                    .justify_center()
                    .child(
                        Button::new("home-empty-open")
                            .label("Open a file…")
                            .small()
                            .primary()
                            .on_click(|_, window, cx| {
                                window.dispatch_action(Box::new(crate::actions::Open), cx)
                            }),
                    )
                    .child(
                        Button::new("home-empty-new")
                            .label("New document…")
                            .small()
                            .outline()
                            .on_click(cx.listener(|this, _, window, cx| {
                                this.open_new_canvas_kind(CanvasKind::Photo, window, cx)
                            })),
                    ),
            );
        }
        div()
            .id("home-empty")
            .test_support()
            .w_full()
            .child(state)
            .into_any_element()
    }

    pub(super) fn home_navigation_menu(&self, p: &Palette, cx: &Context<Self>) -> AnyElement {
        let owner = cx.weak_entity();
        div()
            .id("home-navigation-compact")
            .test_support()
            .w(px(56.))
            .flex_none()
            .bg(p.panel)
            .border_r_1()
            .border_color(p.line)
            .p(px(8.))
            .child(
                Popover::new("home-navigation-popover")
                    .trigger(
                        Button::new("home-navigation-open")
                            .accessibility_label("Projects and filters")
                            .tooltip("Projects and filters")
                            .ghost()
                            .size(px(36.))
                            .child(icon("panel-left", 16.)),
                    )
                    .content(move |_, window, cx| {
                        owner
                            .update(cx, |this, cx| {
                                div()
                                    .h((window.viewport_size().height - px(80.)).max(px(120.)))
                                    .child(this.home_dashboard_sidebar(&theme::palette(cx), cx))
                                    .into_any_element()
                            })
                            .unwrap_or_else(|_| div().into_any_element())
                    }),
            )
            .into_any_element()
    }
    pub(super) fn home_list_heading(&self, wide: bool, p: &Palette) -> AnyElement {
        let mut row = div()
            .id("home-list-heading")
            .test_support()
            .flex()
            .items_center()
            .gap(px(12.))
            .h(px(32.))
            .px(px(14.))
            .border_b_1()
            .border_color(p.line)
            .bg(p.panel)
            .font_family(theme::MONO_FONT)
            .text_size(px(9.5))
            .text_color(p.muted)
            .child(div().w(px(16.)).flex_none())
            .child(div().flex_1().min_w_0().child("NAME"));
        if wide {
            row = row.child(div().w(px(130.)).flex_none().child("PROJECT"));
        }
        row = row.child(div().w(px(100.)).flex_none().child("WORKSPACE"));
        if wide {
            row = row.child(div().w(px(90.)).flex_none().child("SIZE"));
        }
        row.child(div().w(px(100.)).flex_none().child("OPENED"))
            .child(div().w(px(24.)).flex_none())
            .into_any_element()
    }
    pub(super) fn home_workspace_label(&self, path: &Path) -> String {
        self.home_project_kind(path)
            .map(|kind| kind.label().to_string())
            .unwrap_or_else(|| file_kind(path))
    }
    fn pick_home_folder(&mut self, id: Option<u64>, unfiled: bool, cx: &mut Context<Self>) {
        self.home_state.cloud_files = false;
        self.home_state.page = 0;
        self.home_state.projects.folder = id;
        self.home_state.unfiled = unfiled;
        self.home_state.projects.trash = false;
        self.home_state.filter = HomeFilter::All;
        self.home_state.folder = None;
        self.home_state.selected = None;
        self.home_state.checked.clear();
        cx.notify();
    }
    pub(super) fn home_locations(&self, cx: &Context<Self>) -> AnyElement {
        let owner = cx.weak_entity();
        Popover::new("home-locations-popover")
            .trigger(button("home-locations", "Folders and imports…"))
            .content(move |_, _, cx| {
                owner
                    .update(cx, |this, cx| {
                        let p = theme::palette(cx);
                        div()
                            .w(px(250.))
                            .h(px(440.))
                            .child(this.home_library(13.75, &p, cx))
                            .into_any_element()
                    })
                    .unwrap_or_else(|_| div().into_any_element())
            })
            .into_any_element()
    }
    pub(super) fn home_welcome(&self, p: &Palette, cx: &Context<Self>) -> AnyElement {
        if self.home_state.cloud_files {
            return div()
                .id("home-welcome")
                .test_support()
                .flex()
                .flex_col()
                .gap_1()
                .child(
                    div()
                        .text_size(px(22.))
                        .font_weight(FontWeight::SEMIBOLD)
                        .child("Cloud files"),
                )
                .child(
                    div()
                        .text_sm()
                        .text_color(p.muted)
                        .child("Browse synced files. Open a file’s history to see older versions."),
                )
                .into_any_element();
        }
        let folder = self.home_state.projects.folder.and_then(|id| {
            self.home_state
                .projects
                .catalog
                .folders
                .iter()
                .find(|f| f.id == id)
        });
        let title = if self.home_state.cloud_files {
            "Cloud files".into()
        } else {
            folder.map(|f| f.name.clone()).unwrap_or_else(|| {
                if self.home_state.unfiled {
                    "Unfiled"
                } else if self.home_state.projects.trash {
                    "Trash"
                } else {
                    "Welcome back"
                }
                .into()
            })
        };
        div()
            .id("home-welcome")
            .test_support()
            .flex()
            .flex_wrap()
            .items_end()
            .justify_between()
            .gap(px(16.))
            .child(
                div()
                    .flex()
                    .flex_col()
                    .min_w_0()
                    .gap(px(4.))
                    .child(
                        div()
                            .text_size(px(22.))
                            .font_weight(FontWeight::SEMIBOLD)
                            .child(title),
                    )
                    .child(div().text_size(px(12.)).text_color(p.muted).child(
                        if self.home_state.cloud_files {
                            "Browse synced files. Open a file’s history to see older versions."
                        } else if folder.is_some() {
                            "New files here save to this project."
                        } else {
                            "Pick up where you left off, or start something new."
                        },
                    )),
            )
            .child(
                div()
                    .flex()
                    .gap(px(6.))
                    .child(
                        button("home-import-files", "Open…")
                            .icon(icon("folder-open", 12.))
                            .on_click(|_, window, cx| {
                                window.dispatch_action(Box::new(crate::actions::Open), cx)
                            }),
                    )
                    .child(
                        Button::new("home-start-prompt")
                            .accessibility_label("Start with a prompt")
                            .small()
                            .h(px(30.))
                            .primary()
                            .icon(icon("sparkles", 12.))
                            .child(div().text_size(px(12.)).child("Start with a prompt"))
                            .on_click(cx.listener(|this, _, window, cx| {
                                let project = ProjectEditor::new_project(
                                    ProjectKind::Design,
                                    Document::new(1080, 1080),
                                )
                                .expect("valid canvas");
                                this.install_project(project, "Untitled design".into(), window, cx);
                                if let Some(editor) = this.editor.clone() {
                                    editor.update(cx, |e, cx| e.open_ask(window, cx));
                                }
                            })),
                    ),
            )
            .into_any_element()
    }
    pub(super) fn home_dashboard_sidebar(&self, p: &Palette, cx: &mut Context<Self>) -> AnyElement {
        let search = self.home_state.search.as_ref().unwrap().0.clone();
        let state = &self.home_state.projects;
        let entries = self.home_project_entries();
        let trashed_paths: HashSet<_> = state
            .catalog
            .projects
            .iter()
            .filter(|p| p.trashed)
            .map(|p| &p.path)
            .collect();
        let mut list = div()
            .id("home-library-list")
            .flex()
            .flex_col()
            .flex_1()
            .min_h_0()
            .overflow_y_scroll()
            .gap(px(2.))
            .child(
                div().pb(px(10.)).child(
                    Styled::h(Input::new(&search).small(), px(30.))
                        .text_size(px(11.5))
                        .prefix(icon("search", 12.)),
                ),
            );
        for (id, label, glyph, filter, trash) in [
            ("home-filter-all", "Recent", "clock", HomeFilter::All, false),
            (
                "home-filter-today",
                "Today",
                "calendar",
                HomeFilter::Today,
                false,
            ),
            (
                "home-filter-starred",
                "Pinned",
                "pin",
                HomeFilter::Starred,
                false,
            ),
            (
                "home-filter-unfinished",
                "Unfinished",
                "file-pen-line",
                HomeFilter::Unfinished,
                false,
            ),
            ("home-trash", "Trash", "trash", HomeFilter::All, true),
        ] {
            let count = entries
                .iter()
                .filter(|entry| {
                    let trashed = trashed_paths.contains(&entry.path);
                    trashed == trash
                        && match filter {
                            HomeFilter::Starred => crate::app_state::settings(cx)
                                .starred_files
                                .contains(&entry.path),
                            HomeFilter::Unfinished => self.unfinished(&entry.path, cx),
                            HomeFilter::Today => {
                                recent::now().saturating_sub(entry.opened) < 86_400
                            }
                            _ => true,
                        }
                })
                .count();
            let active = !self.home_state.cloud_files
                && self.home_state.filter == filter
                && state.trash == trash
                && state.folder.is_none()
                && !self.home_state.unfiled;
            list = list.child(
                Button::new(id)
                    .accessibility_label(label)
                    .ghost()
                    .h(px(30.))
                    .w_full()
                    .selected(active)
                    .child(
                        div()
                            .flex()
                            .items_center()
                            .gap(px(9.))
                            .w_full()
                            .text_color(if active { p.ink } else { p.muted })
                            .child(icon(glyph, 13.))
                            .child(div().text_size(px(12.)).flex_1().child(label))
                            .child(
                                div()
                                    .font_family(theme::MONO_FONT)
                                    .text_size(px(10.))
                                    .child(count.to_string()),
                            ),
                    )
                    .on_click(cx.listener(move |this, _, _, cx| {
                        this.pick_home_folder(None, false, cx);
                        this.home_state.cloud_files = false;
                        this.home_state.page = 0;
                        this.home_state.filter = filter;
                        if filter == HomeFilter::All && !trash {
                            this.home_state.sort_name = false;
                        }
                        this.home_state.projects.trash = trash;
                        cx.notify();
                    })),
            );
        }
        list = list.child(
            div()
                .pt(px(16.))
                .pb(px(6.))
                .px(px(10.))
                .font_family(theme::MONO_FONT)
                .text_size(px(9.5))
                .text_color(p.muted)
                .child("PROJECTS"),
        );
        for folder in &state.catalog.folders {
            let id = folder.id;
            let count = state
                .catalog
                .projects
                .iter()
                .filter(|p| p.folder == Some(id) && !p.trashed)
                .count();
            list = list.child(
                Button::new(("home-project-nav", id))
                    .accessibility_label(folder.name.clone())
                    .ghost()
                    .h(px(28.))
                    .w_full()
                    .selected(!self.home_state.cloud_files && state.folder == Some(id))
                    .child(
                        div()
                            .flex()
                            .items_center()
                            .gap(px(9.))
                            .w_full()
                            .child(div().size(px(8.)).rounded(px(2.)).bg(folder_color(id)))
                            .child(
                                div()
                                    .flex_1()
                                    .min_w_0()
                                    .text_ellipsis()
                                    .text_size(px(12.))
                                    .child(folder.name.clone()),
                            )
                            .child(
                                div()
                                    .font_family(theme::MONO_FONT)
                                    .text_size(px(10.))
                                    .text_color(p.muted)
                                    .child(count.to_string()),
                            ),
                    )
                    .on_click(cx.listener(move |this, _, _, cx| {
                        this.pick_home_folder(Some(id), false, cx)
                    })),
            );
        }
        list = list
            .child(
                button("home-unfiled", "Unfiled")
                    .ghost()
                    .w_full()
                    .h(px(28.))
                    .selected(!self.home_state.cloud_files && self.home_state.unfiled)
                    .on_click(cx.listener(|this, _, _, cx| this.pick_home_folder(None, true, cx))),
            )
            .child(
                button("home-new-folder", "New project")
                    .w_full()
                    .h(px(28.))
                    .child(icon("folder-plus", 12.))
                    .on_click(cx.listener(|this, _, window, cx| {
                        this.home_project_name_dialog(None, None, window, cx)
                    })),
            );
        div()
            .id("home-library")
            .test_support()
            .w(px(220.))
            .flex_none()
            .min_h_0()
            .flex()
            .flex_col()
            .px(px(10.))
            .py(px(14.))
            .gap_2()
            .border_r_1()
            .border_color(p.line)
            .bg(p.panel)
            .child(list)
            .child(
                Button::new("home-cloud-sidebar")
                    .label("Cloud files")
                    .ghost()
                    .w_full()
                    .selected(self.home_state.cloud_files)
                    .on_click(cx.listener(|this, _, window, cx| this.open_cloud_home(window, cx))),
            )
            .child(
                button("home-more", "Project tools…")
                    .ghost()
                    .w_full()
                    .on_click(cx.listener(|this, _, _, cx| {
                        this.home_state.management = !this.home_state.management;
                        cx.notify();
                    })),
            )
            .child(
                Button::new("home-library-sidebar")
                    .label("Library")
                    .ghost()
                    .w_full()
                    .on_click(cx.listener(|this, _, window, cx| {
                        this.visit_destination(Destination::Library, window, cx)
                    })),
            )
            .child(
                div()
                    .p(px(10.))
                    .rounded(px(6.))
                    .border_1()
                    .border_color(p.line)
                    .flex()
                    .flex_col()
                    .gap(px(5.))
                    .child(
                        div()
                            .flex()
                            .justify_between()
                            .text_size(px(11.))
                            .child("Storage")
                            .child(
                                div()
                                    .font_family(theme::MONO_FONT)
                                    .text_size(px(10.))
                                    .text_color(p.muted)
                                    .child("Local files"),
                            ),
                    )
                    .child(
                        div()
                            .text_size(px(10.))
                            .text_color(p.muted)
                            .child("Saved in your chosen folders"),
                    ),
            )
            .into_any_element()
    }
    pub(super) fn home_project_cards(
        &self,
        width: f32,
        p: &Palette,
        cx: &Context<Self>,
    ) -> Option<AnyElement> {
        if self.home_state.projects.folder.is_some()
            || self.home_state.projects.trash
            || self.home_state.unfiled
            || self.home_state.filter != HomeFilter::All
        {
            return None;
        }
        let state = &self.home_state.projects;
        let mut grid = div()
            .id("home-project-grid")
            .test_support()
            .grid()
            .grid_cols(((width - 4.) / 12.5).floor().clamp(1., 5.) as u16)
            .gap(px(10.));
        for folder in &state.catalog.folders {
            let id = folder.id;
            let mut projects = state
                .catalog
                .projects
                .iter()
                .filter(|p| p.folder == Some(id) && !p.trashed)
                .collect::<Vec<_>>();
            projects.sort_by_key(|p| std::cmp::Reverse(p.opened));
            let mut thumbs = div().flex().gap(px(4.)).h(px(44.));
            for i in 0..3 {
                thumbs = thumbs.child(
                    div()
                        .flex_1()
                        .min_w_0()
                        .h_full()
                        .rounded(px(4.))
                        .overflow_hidden()
                        .bg(p.soft_bg)
                        .when_some(projects.get(i), |cell, project| {
                            cell.child(self.recent_thumbnail(&project.path, p))
                        }),
                );
            }
            grid = grid.child(
                Button::new(("home-project-card", id))
                    .accessibility_label(folder.name.clone())
                    .outline()
                    .h_auto()
                    .p(px(12.))
                    .bg(p.panel)
                    .child(
                        div()
                            .flex()
                            .flex_col()
                            .gap(px(10.))
                            .w_full()
                            .min_w_0()
                            .child(
                                div()
                                    .flex()
                                    .items_center()
                                    .gap(px(8.))
                                    .child(
                                        div()
                                            .size(px(22.))
                                            .flex_none()
                                            .rounded(px(6.))
                                            .bg(folder_color(id)),
                                    )
                                    .child(
                                        div()
                                            .text_size(px(12.5))
                                            .font_weight(FontWeight::MEDIUM)
                                            .text_ellipsis()
                                            .child(folder.name.clone()),
                                    ),
                            )
                            .child(thumbs)
                            .child(
                                div()
                                    .flex()
                                    .justify_between()
                                    .font_family(theme::MONO_FONT)
                                    .text_size(px(10.))
                                    .text_color(p.muted)
                                    .child(format!("{} files", projects.len()))
                                    .child(
                                        projects
                                            .first()
                                            .map(|p| recent::ago(p.opened))
                                            .unwrap_or_else(|| "Empty".into()),
                                    ),
                            ),
                    )
                    .on_click(cx.listener(move |this, _, _, cx| {
                        this.pick_home_folder(Some(id), false, cx)
                    })),
            );
        }
        Some(
            div()
                .id("home-projects")
                .test_support()
                .flex()
                .flex_col()
                .gap(px(12.))
                .child(
                    div()
                        .flex()
                        .items_center()
                        .gap(px(8.))
                        .child(
                            div()
                                .text_size(px(14.))
                                .font_weight(FontWeight::SEMIBOLD)
                                .child("Projects"),
                        )
                        .child(
                            div()
                                .font_family(theme::MONO_FONT)
                                .text_size(px(10.5))
                                .text_color(p.muted)
                                .child(format!("{} projects", state.catalog.folders.len())),
                        )
                        .child(div().flex_1())
                        .child(
                            button("home-projects-new", "+ New project")
                                .ghost()
                                .on_click(cx.listener(|this, _, window, cx| {
                                    this.home_project_name_dialog(None, None, window, cx)
                                })),
                        ),
                )
                .child(grid)
                .into_any_element(),
        )
    }
    pub(super) fn home_file_controls(
        &self,
        count: usize,
        p: &Palette,
        cx: &Context<Self>,
    ) -> AnyElement {
        let mut row = div()
            .id("home-actions")
            .test_support()
            .flex()
            .flex_wrap()
            .items_center()
            .gap(px(8.))
            .child(
                div()
                    .text_size(px(14.))
                    .font_weight(FontWeight::SEMIBOLD)
                    .child(if self.home_state.projects.trash {
                        "Trash"
                    } else if self.home_state.projects.folder.is_some() || self.home_state.unfiled {
                        "Files"
                    } else {
                        "Recent"
                    }),
            )
            .child(
                div()
                    .font_family(theme::MONO_FONT)
                    .text_size(px(10.5))
                    .text_color(p.muted)
                    .child(format!("{count} files")),
            )
            .child(div().flex_1())
            .child(
                Button::new("home-cloud")
                    .label("Cloud & Photos")
                    .xsmall()
                    .outline()
                    .on_click(cx.listener(|this, _, window, cx| {
                        this.open_cloud_home(window, cx);
                    })),
            );
        let mut filters =
            crate::widgets::command_bar("home-kind-toolbar", "Filter by document type");
        for (i, kind) in [
            None,
            Some(CanvasKind::Photo),
            Some(CanvasKind::Design),
            Some(CanvasKind::Diagram),
            Some(CanvasKind::Paint),
        ]
        .into_iter()
        .enumerate()
        {
            let label = kind.map_or("All", CanvasKind::label);
            filters = filters.child(
                Button::new(("home-kind", i))
                    .accessibility_label(label)
                    .xsmall()
                    .outline()
                    .h(px(24.))
                    .rounded_full()
                    .selected(self.home_state.projects.kind == kind)
                    .child(div().text_size(px(11.)).child(label))
                    .on_click(cx.listener(move |this, _, _, cx| {
                        this.home_state.page = 0;
                        this.home_state.projects.kind = kind;
                        cx.notify();
                    })),
            );
        }
        row = row.child(filters);
        let mut view = crate::widgets::command_bar("home-view-toolbar", "File view and sorting")
            .child(
                button(
                    "home-sort",
                    if self.home_state.sort_name {
                        "Name ↑"
                    } else {
                        "Last opened ↓"
                    },
                )
                .h(px(24.))
                .on_click(cx.listener(|this, _, _, cx| {
                    this.home_state.sort_name = !this.home_state.sort_name;
                    cx.notify();
                })),
            );
        for (id, glyph, rows) in [
            ("home-grid", "layout-grid", false),
            ("home-list", "list", true),
        ] {
            view = view.child(
                Button::new(id)
                    .accessibility_label(if rows { "List view" } else { "Grid view" })
                    .tooltip(if rows { "List view" } else { "Grid view" })
                    .xsmall()
                    .outline()
                    .h(px(24.))
                    .selected(self.home_state.rows == rows)
                    .child(icon(glyph, 12.))
                    .on_click(cx.listener(move |this, _, _, cx| this.set_home_rows(rows, cx))),
            );
        }
        view = view.child(
            button("home-details", "Details")
                .h(px(24.))
                .selected(self.home_state.details)
                .on_click(cx.listener(|this, _, _, cx| {
                    this.home_state.details = !this.home_state.details;
                    cx.notify();
                })),
        );
        row = row.child(view);
        if !self.home_state.checked.is_empty() {
            row = row
                .child(
                    button(
                        "home-batch",
                        format!("Batch {}…", self.home_state.checked.len()),
                    )
                    .disabled(self.batch.running.is_some())
                    .on_click(
                        cx.listener(|this, _, window, cx| this.batch_home_selection(window, cx)),
                    ),
                )
                .child(
                    button("home-clear-checked", "Clear selection").on_click(cx.listener(
                        |this, _, _, cx| {
                            this.home_state.checked.clear();
                            cx.notify();
                        },
                    )),
                );
        }
        row.into_any_element()
    }
}
