//! Draft library selection: browsing and Cancel never modify the toolbox.
use super::*;
use emulsion_core::diagram;
use emulsion_io::{
    creative_library::{self as library, AssetKind},
    template_pack,
};
use gpui_kit::component::{Disableable, WindowExt, checkbox::Checkbox};

#[derive(Clone, Copy)]
enum Source {
    Native(&'static str),
    Bundled(&'static str),
    Installed,
}
struct Choice {
    source: Source,
    name: String,
    category: String,
    count: usize,
    installed: Option<u64>,
    path: Option<PathBuf>,
    selected: bool,
}
struct LibraryPicker {
    choices: Vec<Choice>,
    search: Entity<InputState>,
    _search_subscription: Subscription,
    active: usize,
    preview_page: usize,
    preview_scroll: ScrollHandle,
    previews: Vec<(String, Arc<Document>)>,
}
impl LibraryPicker {
    fn load_previews(&mut self) {
        self.preview_scroll.set_offset(point(px(0.), px(0.)));
        self.previews.clear();
        match self.choices[self.active].source {
            Source::Native(category) => {
                for stencil in diagram::stencils::STENCILS
                    .iter()
                    .filter(|s| s.category == category)
                    .skip(self.preview_page * 24)
                    .take(24)
                {
                    let mut editor = emulsion_core::Editor::new(Document::new(340, 220), None);
                    let (w, h) = stencil.default_size();
                    let scale = (280. / w).min(160. / h);
                    if stencil
                        .insert(&mut editor, [30., 30., w * scale, h * scale])
                        .is_ok()
                    {
                        self.previews
                            .push((stencil.label.into(), Arc::new(editor.doc)));
                    }
                }
            }
            Source::Bundled(id) => {
                for key in emulsion_io::diagram_packs::entries(id)
                    .into_iter()
                    .skip(self.preview_page * 24)
                    .take(24)
                {
                    if let Ok(doc) = emulsion_io::diagram_packs::document(key) {
                        self.previews
                            .push((emulsion_io::diagram_packs::entry_name(key), Arc::new(doc)));
                    }
                }
            }
            Source::Installed => {}
        }
    }
}
impl Render for LibraryPicker {
    fn render(&mut self, window: &mut Window, cx: &mut Context<Self>) -> impl IntoElement {
        let p = theme::palette(cx);
        let width = (f32::from(window.viewport_size().width) - 88.).min(992.);
        let narrow = width < 640.;
        let query = self.search.read(cx).value().trim().to_lowercase();
        let active = &self.choices[self.active];
        let selected = self.choices.iter().filter(|choice| choice.selected).count();
        let mut libraries = div()
            .id("shape-library-list")
            .test_support()
            .overflow_y_scroll()
            .flex_1()
            .min_h_0()
            .flex()
            .flex_col()
            .gap_1();
        let mut group = "";
        let mut visible = 0;
        for (i, choice) in self.choices.iter().enumerate() {
            if !format!("{} {}", choice.name, choice.category)
                .to_lowercase()
                .contains(&query)
            {
                continue;
            }
            visible += 1;
            let heading = match choice.source {
                Source::Native(_) => "Built-in",
                Source::Bundled(id) if emulsion_io::diagram_packs::is_builtin(id) => "Built-in",
                Source::Bundled(_) => "More libraries",
                Source::Installed => "Imported",
            };
            if heading != group {
                group = heading;
                libraries = libraries.child(
                    div()
                        .px_2()
                        .pt_3()
                        .pb_1()
                        .text_size(px(11.))
                        .text_color(p.muted)
                        .child(heading),
                );
            }
            libraries = libraries.child(
                div()
                    .flex()
                    .flex_shrink_0()
                    .items_center()
                    .gap_2()
                    .px_2()
                    .rounded(px(7.))
                    .border_1()
                    .border_color(if self.active == i {
                        p.accent.opacity(0.45)
                    } else {
                        transparent_black()
                    })
                    .bg(if self.active == i {
                        p.accent.opacity(0.10)
                    } else {
                        transparent_black()
                    })
                    .child(
                        Checkbox::new(("shape-library-check", i))
                            .small()
                            .checked(choice.selected)
                            .accessibility_label(format!("Include {}", choice.name))
                            .on_click(cx.listener(move |this, checked, _, cx| {
                                this.choices[i].selected = *checked;
                                cx.notify();
                            })),
                    )
                    .child(
                        Button::new(("shape-library-open", i))
                            .ghost()
                            .flex_1()
                            .min_w_0()
                            .h(px(44.))
                            .justify_start()
                            .px_1()
                            .accessibility_label(format!("Preview {}", choice.name))
                            .child(
                                div()
                                    .flex()
                                    .flex_col()
                                    .min_w_0()
                                    .w_full()
                                    .text_left()
                                    .child(
                                        div()
                                            .text_size(px(12.))
                                            .text_ellipsis()
                                            .child(choice.name.clone()),
                                    )
                                    .child(
                                        div()
                                            .text_size(px(10.))
                                            .text_color(p.muted)
                                            .child(format!("{} shapes", choice.count)),
                                    ),
                            )
                            .on_click(cx.listener(move |this, _, _, cx| {
                                this.active = i;
                                this.preview_page = 0;
                                this.load_previews();
                                cx.notify();
                            })),
                    ),
            );
        }
        if visible == 0 {
            libraries = libraries.child(
                div()
                    .id("shape-library-empty")
                    .test_support()
                    .p_3()
                    .text_size(px(12.))
                    .text_color(p.muted)
                    .child("No libraries found. Try a name or category such as network."),
            );
        }
        let columns = if narrow {
            2
        } else if width < 880. {
            3
        } else {
            4
        };
        let mut grid = div()
            .id("shape-library-grid")
            .test_support()
            .grid()
            .grid_cols(columns)
            .gap_2()
            .flex_shrink_0();
        let mut cards = 0;
        for (name, doc) in &self.previews {
            cards += 1;
            grid = grid.child(
                div()
                    .min_w_0()
                    .h(px(126.))
                    .border_1()
                    .border_color(p.line)
                    .rounded(px(8.))
                    .overflow_hidden()
                    .bg(p.panel)
                    .flex()
                    .flex_col()
                    .child(
                        div()
                            .h(px(90.))
                            .flex_shrink_0()
                            .p_3()
                            .bg(rgb(0xf1f3f7))
                            .child(template_preview(doc.clone()).size_full()),
                    )
                    .child(
                        div()
                            .px_2()
                            .py_2()
                            .text_size(px(11.))
                            .text_color(p.ink)
                            .text_ellipsis()
                            .child(name.clone()),
                    ),
            );
        }
        if self.previews.is_empty()
            && let Some(path) = &active.path
        {
            for index in (self.preview_page * 24)..active.count.min((self.preview_page + 1) * 24) {
                let thumbnail = path
                    .parent()
                    .unwrap_or(path)
                    .join(format!("entry-{index}.png"));
                if thumbnail.exists() {
                    cards += 1;
                    grid = grid.child(
                        div()
                            .h(px(126.))
                            .p_3()
                            .rounded(px(8.))
                            .bg(rgb(0xf1f3f7))
                            .child(img(thumbnail).size_full().object_fit(ObjectFit::Contain)),
                    );
                }
            }
        }
        let index = self.active;
        let pages = active.count.div_ceil(24).max(1);
        let details = div().id("shape-library-details").test_support().flex_1().min_w_0().min_h_0()
            .flex().flex_col().gap_3()
            .child(div().flex().flex_wrap().items_center().justify_between().gap_2().flex_shrink_0()
                .child(div().flex().flex_col().min_w_0().gap_1()
                    .child(div().text_size(px(20.)).font_weight(FontWeight::SEMIBOLD).child(active.name.clone()))
                    .child(div().text_size(px(12.)).text_color(p.muted).child(format!("{} · {} shapes", active.category, active.count))))
                .child(Button::new("shape-library-toggle")
                    .label(if active.selected { "Selected" } else { "Select library" }).outline().selected(active.selected)
                    .on_click(cx.listener(move |this, _, _, cx| {
                        this.choices[index].selected = !this.choices[index].selected;
                        cx.notify();
                    }))))
            .child(div().id("shape-library-pagination").test_support().flex().items_center().justify_between().gap_2().flex_shrink_0()
                .child(div().text_size(px(11.)).text_color(p.muted).child(if active.count == 0 {
                    "No shapes".into()
                } else {
                    format!("Shapes {}–{} of {}", self.preview_page * 24 + 1, ((self.preview_page + 1) * 24).min(active.count), active.count)
                }))
                .child(div().flex().items_center().gap_1()
                    .child(Button::new("shape-library-prev").label("‹").accessibility_label("Previous preview page").small().ghost().disabled(self.preview_page == 0)
                        .on_click(cx.listener(|this, _, _, cx| { this.preview_page = this.preview_page.saturating_sub(1); this.load_previews(); cx.notify(); })))
                    .child(div().text_size(px(11.)).text_color(p.muted).child(format!("{} / {pages}", self.preview_page + 1)))
                    .child(Button::new("shape-library-next").label("›").accessibility_label("Next preview page").small().ghost().disabled(self.preview_page + 1 >= pages)
                        .on_click(cx.listener(|this, _, _, cx| { this.preview_page += 1; this.load_previews(); cx.notify(); })))))
            .child(div().id("shape-library-preview-scroll").test_support().track_scroll(&self.preview_scroll)
                .overflow_y_scroll().flex_1().min_h_0().pr_2()
                .child(grid)
                .when(cards == 0, |d| d.child(div().p_4().text_color(p.muted).text_size(px(12.))
                    .child("No previews available. You can still select this library to use its shapes."))));
        div().id("shape-library-picker").test_support()
            .h(px((f32::from(window.viewport_size().height) - 220.).clamp(200., 680.)))
            .flex().flex_col().gap_3()
            .child(div().flex().flex_wrap().justify_between().gap_2().text_size(px(12.)).flex_shrink_0()
                .child(div().text_color(p.muted).child("Choose the libraries to show in your Shapes panel."))
                .child(div().id("shape-library-selection-count").test_support().text_color(p.accent)
                    .child(format!("{selected} selected"))))
            .child(div().flex().when(narrow, |d| d.flex_col()).flex_1().min_h_0().gap_4()
                .child(div().flex().flex_col().gap_2().flex_shrink_0().min_h_0()
                    .when(!narrow, |d| d.w(px(252.)).pr_3().border_r_1().border_color(p.line))
                    .when(narrow, |d| d.h(px(160.)).pb_2().border_b_1().border_color(p.line))
                    .child(Input::new(&self.search))
                    .child(libraries))
                .child(details))
            .child(div().text_size(px(11.)).text_color(p.muted).flex_shrink_0()
                .child("Unchecking a library hides it from the panel. Shapes already on your canvas stay unchanged."))
    }
}
impl EditorView {
    pub(crate) fn diagram_library_dialog(&mut self, window: &mut Window, cx: &mut Context<Self>) {
        if self.diagram_ui.library_installing {
            self.set_status("Shape libraries are still being prepared.", false, cx);
            return;
        }
        self.load_creative_library(cx);
        let settings = crate::app_state::settings(cx);
        let mut choices = Vec::new();
        let mut categories = diagram::stencils::CATEGORIES.to_vec();
        categories.sort_by_key(|c| match *c {
            "General" => 0,
            "Flowchart" => 1,
            _ => 2,
        });
        for category in categories {
            choices.push(Choice {
                source: Source::Native(category),
                name: if category == "General" {
                    "Standard".into()
                } else {
                    category.into()
                },
                category: "Built-in".into(),
                count: diagram::stencils::STENCILS
                    .iter()
                    .filter(|s| s.category == category)
                    .count(),
                installed: None,
                path: None,
                selected: settings
                    .diagram_shape_libraries
                    .iter()
                    .any(|c| c == category),
            });
        }
        for &(id, name, category) in emulsion_io::diagram_packs::packs() {
            let count = emulsion_io::diagram_packs::entries(id).len();
            if count == 0 {
                continue;
            }
            let asset = self.creative.catalog.assets.iter().find(|a| {
                a.kind == AssetKind::Stencil
                    && a.tags.contains(&emulsion_io::diagram_packs::tag(id))
            });
            choices.push(Choice {
                source: Source::Bundled(id),
                name: name.into(),
                category: category.into(),
                count,
                installed: asset.map(|a| a.id),
                path: asset.map(|a| a.path.clone()),
                selected: asset.is_some_and(|a| settings.diagram_stencil_packs.contains(&a.id)),
            });
        }
        for asset in self.creative.catalog.assets.iter().filter(|a| {
            a.kind == AssetKind::Stencil
                && !a
                    .tags
                    .iter()
                    .any(|t| emulsion_io::diagram_packs::is_bundled_tag(t))
        }) {
            choices.push(Choice {
                source: Source::Installed,
                name: asset.name.clone(),
                category: "My libraries".into(),
                count: asset.variants.len(),
                installed: Some(asset.id),
                path: Some(asset.path.clone()),
                selected: settings.diagram_stencil_packs.contains(&asset.id),
            });
        }
        let picker = cx.new(|cx| {
            let search =
                cx.new(|cx| InputState::new(window, cx).placeholder("Search shape libraries…"));
            let subscription = cx.subscribe(&search, |_, _, event, cx| {
                if matches!(event, InputEvent::Change) {
                    cx.notify();
                }
            });
            let mut picker = LibraryPicker {
                choices,
                search,
                _search_subscription: subscription,
                active: 0,
                preview_page: 0,
                preview_scroll: ScrollHandle::new(),
                previews: Vec::new(),
            };
            picker.load_previews();
            picker
        });
        let owner = cx.weak_entity();
        window.open_dialog(cx, move |dialog, window, _| {
            let state = picker.clone();
            let owner = owner.clone();
            let import_owner = owner.clone();
            dialog
                .title("Shape libraries")
                .width(px(
                    (f32::from(window.viewport_size().width) - 40.).clamp(320., 1040.)
                ))
                .child(picker.clone())
                .footer(
                    div()
                        .flex()
                        .items_center()
                        .justify_between()
                        .flex_wrap()
                        .gap_2()
                        .child(
                            Button::new("shape-library-import")
                                .label("Import library…")
                                .ghost()
                                .on_click(move |_, window, cx| {
                                    window.close_dialog(cx);
                                    import_owner
                                        .update(cx, |this, cx| this.install_diagram_stencils(cx))
                                        .ok();
                                }),
                        )
                        .child(
                            div()
                                .flex()
                                .gap_2()
                                .child(
                                    Button::new("shape-library-cancel")
                                        .label("Cancel")
                                        .on_click(|_, window, cx| window.close_dialog(cx)),
                                )
                                .child(
                                    Button::new("shape-library-apply")
                                        .label("Apply libraries")
                                        .primary()
                                        .on_click(move |_, window, cx| {
                                            let choices = &state.read(cx).choices;
                                            let native = choices
                                                .iter()
                                                .filter(|c| c.selected)
                                                .filter_map(|c| {
                                                    if let Source::Native(id) = c.source {
                                                        Some(id.to_string())
                                                    } else {
                                                        None
                                                    }
                                                })
                                                .collect::<Vec<_>>();
                                            let installed = choices
                                                .iter()
                                                .filter(|c| c.selected)
                                                .filter_map(|c| c.installed)
                                                .collect::<Vec<_>>();
                                            let pending = choices
                                                .iter()
                                                .filter(|c| c.selected && c.installed.is_none())
                                                .filter_map(|c| {
                                                    if let Source::Bundled(id) = c.source {
                                                        Some(id.to_string())
                                                    } else {
                                                        None
                                                    }
                                                })
                                                .collect::<Vec<_>>();
                                            owner
                                                .update(cx, |this, cx| {
                                                    this.apply_diagram_libraries(
                                                        native, installed, pending, cx,
                                                    )
                                                })
                                                .ok();
                                            window.close_dialog(cx);
                                        }),
                                ),
                        ),
                )
        });
    }
    fn apply_diagram_libraries(
        &mut self,
        native: Vec<String>,
        installed: Vec<u64>,
        pending: Vec<String>,
        cx: &mut Context<Self>,
    ) {
        crate::app_state::update_settings(cx, |s| {
            s.diagram_shape_libraries = native;
            s.diagram_stencil_packs = installed.clone();
        });
        self.diagram_ui.collapsed_categories.clear();
        self.diagram_ui.expanded_stencil_packs = installed.into_iter().collect();
        self.diagram_ui.library_tab = 0;
        cx.notify();
        if pending.is_empty() {
            return;
        }
        self.diagram_ui.library_installing = true;
        self.set_status("Preparing selected shape libraries…", false, cx);
        cx.spawn(async move |this, cx| {
            let (catalog, ids, notes) = cx
                .background_spawn(async move {
                    let mut catalog = None;
                    let mut ids = Vec::new();
                    let mut notes = Vec::new();
                    for id in pending {
                        match emulsion_io::diagram_packs::build(&id).and_then(|(pack, warnings)| {
                            notes.extend(warnings);
                            template_pack::install(&library::root(), pack)
                        }) {
                            Ok((updated, asset)) => {
                                catalog = Some(updated);
                                ids.push(asset);
                            }
                            Err(error) => notes.push(format!("{id}: {error}")),
                        }
                    }
                    (catalog, ids, notes)
                })
                .await;
            this.update(cx, |this, cx| {
                this.diagram_ui.library_installing = false;
                if let Some(catalog) = catalog {
                    this.install_catalog(catalog);
                }
                crate::app_state::update_settings(cx, |s| {
                    s.diagram_stencil_packs.extend(ids.iter().copied())
                });
                this.diagram_ui
                    .expanded_stencil_packs
                    .extend(ids.iter().copied());
                this.diagram_import_notes(notes.clone());
                this.set_status(
                    format!(
                        "Added {} shape libraries · {} import notes",
                        ids.len(),
                        notes.len()
                    ),
                    !notes.is_empty(),
                    cx,
                );
                cx.notify();
            })
            .ok();
        })
        .detach();
    }
}
