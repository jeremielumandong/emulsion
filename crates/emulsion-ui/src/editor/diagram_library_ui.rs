//! Searchable vector previews for offline diagram starters, containers and themes.
use super::*;
use emulsion_core::diagram_library::{TEMPLATES, THEMES};
use gpui_kit::component::{
    Selectable, Sizable,
    button::{Button, ButtonVariants},
};

impl EditorView {
    pub(super) fn diagram_pack_cards(
        &self,
        query: &str,
        p: &Palette,
        cx: &Context<Self>,
    ) -> AnyElement {
        let mut panel = div()
            .id("diagram-stencil-packs")
            .test_support()
            .flex()
            .flex_col()
            .gap_2();
        panel = panel.child(
            div().flex().flex_wrap().gap_1().children(
                ["All", "Cloud", "UML", "Network", "UI", "Business"]
                    .into_iter()
                    .enumerate()
                    .map(|(index, label)| {
                        Button::new(("diagram-pack-filter", index))
                            .label(label)
                            .xsmall()
                            .ghost()
                            .selected(self.diagram_ui.pack_filter == index)
                            .on_click(cx.listener(move |this, _, _, cx| {
                                this.diagram_ui.pack_filter = index;
                                cx.notify();
                            }))
                    }),
            ),
        );
        for (index, category) in emulsion_core::diagram::stencils::CATEGORIES
            .iter()
            .enumerate()
        {
            let category_match = match self.diagram_ui.pack_filter {
                1 => category.contains("Cloud"),
                2 => category.contains("UML"),
                3 => category.contains("Network"),
                4 => category.contains("Wireframe"),
                5 => {
                    category.contains("Business")
                        || category.contains("Planning")
                        || category.contains("Entity")
                }
                _ => true,
            };
            if !category_match || !category.to_lowercase().contains(query) {
                continue;
            }

            let count = emulsion_core::diagram::stencils::STENCILS
                .iter()
                .filter(|s| s.category == *category)
                .count();
            panel = panel.child(
                div()
                    .border_1()
                    .border_color(p.line)
                    .rounded(px(8.))
                    .p_2()
                    .flex()
                    .items_center()
                    .gap_2()
                    .child(
                        div()
                            .size(px(30.))
                            .rounded(px(7.))
                            .bg(p.accent.opacity(0.15))
                            .flex()
                            .items_center()
                            .justify_center()
                            .text_color(p.accent)
                            .child("◇"),
                    )
                    .child(
                        div()
                            .flex_1()
                            .min_w_0()
                            .flex()
                            .flex_col()
                            .gap_1()
                            .child(*category)
                            .child(
                                div()
                                    .text_size(px(10.))
                                    .text_color(p.muted)
                                    .child(format!("Emulsion · {count} shapes")),
                            ),
                    )
                    .child(
                        Button::new(("diagram-pack-added", index))
                            .label("Added")
                            .xsmall()
                            .ghost()
                            .on_click(cx.listener(move |this, _, window, cx| {
                                this.diagram_ui.library_tab = 0;
                                this.diagram_ui.collapsed_categories.remove(category);
                                if let Some(search) = &this.diagram_ui.search {
                                    search.update(cx, |s, cx| s.set_value(*category, window, cx));
                                }
                                cx.notify();
                            })),
                    ),
            );
        }
        for (index, (id, name, category)) in emulsion_io::diagram_packs::PACKS.iter().enumerate() {
            let matches = match self.diagram_ui.pack_filter {
                1 => *category == "Cloud",
                2 => *category == "UML",
                3 => *category == "Network",
                4 => *category == "UI",
                5 => *category == "Business",
                _ => true,
            };
            if !matches || !format!("{name} {category}").to_lowercase().contains(query) {
                continue;
            }
            let installed = self
                .creative
                .catalog
                .assets
                .iter()
                .any(|a| a.tags.contains(&format!("drawio:{id}")));
            let count = emulsion_io::diagram_packs::entries(id).len();
            let id = id.to_string();
            panel = panel.child(
                div()
                    .p_2()
                    .rounded(px(8.))
                    .border_1()
                    .border_color(p.line)
                    .flex()
                    .items_center()
                    .gap_2()
                    .child(
                        div().flex_1().child(*name).child(
                            div()
                                .text_size(px(10.))
                                .text_color(p.muted)
                                .child(format!("draw.io · {count} curated shapes")),
                        ),
                    )
                    .child(
                        Button::new(("diagram-vendor-pack", index))
                            .label(if installed { "Added" } else { "Add" })
                            .xsmall()
                            .outline()
                            .on_click(cx.listener(move |this, _, window, cx| {
                                if !installed {
                                    this.install_bundled_diagram_pack(id.clone(), cx);
                                } else {
                                    this.diagram_ui.library_tab=0;
                                    if let Some(search)=&this.diagram_ui.search {search.update(cx,|s,cx|s.set_value(*name,window,cx));}
                                    cx.notify();
                                }
                            })),
                    ),
            );
        }
        panel.child(Button::new("diagram-import-stencils").label("Import .vssx / .xml / SVG folder…")
            .small().outline().on_click(cx.listener(|this,_,_,cx|this.install_diagram_stencils(cx))))
            .child(div().text_size(px(11.)).text_color(p.muted).child("Bundled packs are available offline. Add a pack or import a local library; drag an installed entry onto the canvas."))
            .into_any_element()
    }

    pub(super) fn diagram_template_cards(
        &self,
        query: &str,
        p: &Palette,
        cx: &Context<Self>,
    ) -> AnyElement {
        let mut grid = div()
            .id("diagram-template-gallery")
            .test_support()
            .grid()
            .grid_cols(2)
            .gap_2();
        let mut matches = 0;
        for (index, template) in TEMPLATES.iter().copied().enumerate() {
            if !format!("{} {}", template.name, template.description)
                .to_lowercase()
                .contains(query)
            {
                continue;
            }
            matches += 1;
            let doc = template_previews()[index].clone();
            grid = grid.child(
                div()
                    .id(("diagram-template", index))
                    .test_support()
                    .cursor_pointer()
                    .border_1()
                    .border_color(p.line)
                    .rounded(px(8.))
                    .bg(p.paper)
                    .p_2()
                    .h(px(135.))
                    .flex()
                    .flex_col()
                    .gap_2()
                    .hover(|d| d.border_color(p.accent))
                    .child(
                        div()
                            .text_size(px(11.))
                            .font_weight(FontWeight::MEDIUM)
                            .child(template.name),
                    )
                    .child(template_preview(doc).flex_1().w_full())
                    .on_mouse_down(
                        MouseButton::Left,
                        cx.listener(move |this, _, _, cx| {
                            if !this.prepare_page_action(cx) {
                                return;
                            }
                            match template.insert(&mut this.editor) {
                                Ok(_) => {
                                    this.set_layer_selection(Vec::new(), None);
                                    this.after_change(cx);
                                    this.fit_pending = true;
                                    this.set_status(
                                        format!("Added {} as a new editable page.", template.name),
                                        false,
                                        cx,
                                    );
                                }
                                Err(error) => this.set_status(error, true, cx),
                            }
                        }),
                    ),
            );
        }
        div()
            .flex()
            .flex_col()
            .gap_2()
            .child(
                div()
                    .text_size(px(11.))
                    .text_color(p.muted)
                    .child("Choose a starter to add a new page."),
            )
            .child(grid)
            .when(matches == 0, |d| d.child("No matching templates"))
            .into_any_element()
    }
    pub(super) fn diagram_theme_cards(
        &self,
        query: &str,
        p: &Palette,
        cx: &Context<Self>,
    ) -> AnyElement {
        let selection = self.diagram_ui.theme_selection;
        let mut panel = div()
            .id("diagram-theme-gallery")
            .test_support()
            .flex()
            .flex_col()
            .gap_3()
            .child(
                div().flex().gap_1().children(
                    [(false, "Entire page"), (true, "Selection")]
                        .into_iter()
                        .map(|(selected, label)| {
                            Button::new(if selected {
                                "diagram-theme-selection"
                            } else {
                                "diagram-theme-page"
                            })
                            .label(label)
                            .small()
                            .outline()
                            .selected(selection == selected)
                            .on_click(cx.listener(
                                move |this, _, _, cx| {
                                    this.diagram_ui.theme_selection = selected;
                                    cx.notify();
                                },
                            ))
                        }),
                ),
            );
        for (index, theme) in THEMES.iter().copied().enumerate() {
            if !theme.name.to_lowercase().contains(query) {
                continue;
            }
            let swatches = div().flex().gap_1().children(
                [theme.fill, theme.line, theme.text, [255, 255, 255, 255]].map(|color| {
                    div()
                        .w(px(36.))
                        .h(px(28.))
                        .rounded(px(4.))
                        .bg(gpui::rgba(u32::from_be_bytes(color)))
                        .border_1()
                        .border_color(p.line)
                }),
            );
            panel = panel.child(
                div()
                    .id(("diagram-theme", index))
                    .test_support()
                    .cursor_pointer()
                    .border_1()
                    .border_color(p.line)
                    .rounded(px(8.))
                    .bg(p.paper)
                    .p_3()
                    .flex()
                    .flex_col()
                    .gap_2()
                    .hover(|d| d.border_color(p.accent))
                    .child(theme.name)
                    .child(swatches)
                    .on_mouse_down(
                        MouseButton::Left,
                        cx.listener(move |this, _, _, cx| {
                            if !this.prepare_page_action(cx) {
                                return;
                            }
                            let roots = if this.diagram_ui.theme_selection {
                                this.selected_layer_roots()
                            } else {
                                this.editor.doc.children(None)
                            };
                            if roots.is_empty() {
                                this.set_status(
                                    "Select objects before applying a selection theme.",
                                    false,
                                    cx,
                                );
                                return;
                            }
                            match emulsion_core::diagram_library::theme_commands(
                                &this.editor.doc,
                                &roots,
                                theme,
                            ) {
                                Ok(commands) => {
                                    this.execute_layer_commands("Diagram theme", commands, cx);
                                }
                                Err(error) => this.set_status(error, true, cx),
                            }
                        }),
                    ),
            );
        }
        panel
            .child(div().text_size(px(11.)).text_color(p.muted).child(
                "Themes change colors and line weight. Undo restores your original styling.",
            ))
            .into_any_element()
    }
}
fn template_previews() -> &'static Vec<Arc<Document>> {
    static PREVIEWS: std::sync::OnceLock<Vec<Arc<Document>>> = std::sync::OnceLock::new();
    PREVIEWS.get_or_init(|| {
        TEMPLATES
            .iter()
            .map(|t| Arc::new(t.build().expect("bundled template")))
            .collect()
    })
}
fn template_preview(doc: Arc<Document>) -> impl IntoElement + Styled {
    canvas(
        |_, _, _| (),
        move |bounds, _, window, _| {
            let scale = (f32::from(bounds.size.width) / doc.width as f32)
                .min(f32::from(bounds.size.height) / doc.height as f32);
            let dx = f32::from(bounds.left())
                + (f32::from(bounds.size.width) - doc.width as f32 * scale) / 2.;
            let dy = f32::from(bounds.top())
                + (f32::from(bounds.size.height) - doc.height as f32 * scale) / 2.;
            for node in &doc.nodes {
                let NodeKind::Path { path, style, .. } = &node.kind else {
                    continue;
                };
                let paths = path.flatten(0.7);
                for (fill, color) in [(true, style.fill), (false, style.stroke)] {
                    let Some(color) = color else {
                        continue;
                    };
                    let mut builder = if fill {
                        PathBuilder::fill()
                    } else {
                        PathBuilder::stroke(px(0.8))
                    };
                    for (points, closed) in &paths {
                        if fill && !closed {
                            continue;
                        }
                        let screen = |p: (f64, f64)| {
                            point(px(dx + p.0 as f32 * scale), px(dy + p.1 as f32 * scale))
                        };
                        if let Some(first) = points.first() {
                            builder.move_to(screen(*first));
                        }
                        for p in points.iter().skip(1) {
                            builder.line_to(screen(*p));
                        }
                        if *closed {
                            builder.close();
                        }
                    }
                    if let Ok(path) = builder.build() {
                        window.paint_path(path, gpui::rgba(u32::from_be_bytes(color)));
                    }
                }
            }
        },
    )
}
