//! Searchable vector previews for offline diagram starters, containers and themes.
use super::*;
#[path = "diagram_library_picker.rs"]
mod picker;
use emulsion_core::diagram_library::{TEMPLATES, THEMES};
use gpui_kit::component::{
    Selectable, Sizable,
    button::{Button, ButtonVariants},
};

impl EditorView {
    pub(super) fn diagram_pack_cards(
        &self,
        _query: &str,
        p: &Palette,
        cx: &Context<Self>,
    ) -> AnyElement {
        div()
            .flex()
            .flex_col()
            .gap_3()
            .child(t!("editor.diagram_library_ui.choose_libraries"))
            .child(
                div()
                    .text_size(px(12.))
                    .text_color(p.muted)
                    .child(t!("editor.diagram_library_ui.libraries_body")),
            )
            .child(
                Button::new("diagram-browse-libraries")
                    .label(t!("editor.diagram_library_ui.add_shapes"))
                    .on_click(
                        cx.listener(|this, _, window, cx| this.diagram_library_dialog(window, cx)),
                    ),
            )
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
            if !template.matches(query) {
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
                                        t!("editor.diagram_library_ui.added", name = template.name),
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
                    .child(t!("editor.diagram_library_ui.choose_starter")),
            )
            .child(grid)
            .when(matches == 0, |d| {
                d.child(t!("editor.diagram_library_ui.no_templates"))
            })
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
                    [
                        (false, t!("editor.diagram_library_ui.entire_page")),
                        (true, t!("editor.diagram_library_ui.selection")),
                    ]
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
                        .on_click(cx.listener(move |this, _, _, cx| {
                            this.diagram_ui.theme_selection = selected;
                            cx.notify();
                        }))
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
                                    t!("editor.diagram_library_ui.select_first"),
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
            .child(
                div()
                    .text_size(px(11.))
                    .text_color(p.muted)
                    .child(t!("editor.diagram_library_ui.themes_note")),
            )
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
        move |bounds, _, window, cx| {
            let scale = (f32::from(bounds.size.width) / doc.width as f32)
                .min(f32::from(bounds.size.height) / doc.height as f32);
            let dx = f32::from(bounds.left())
                + (f32::from(bounds.size.width) - doc.width as f32 * scale) / 2.;
            let dy = f32::from(bounds.top())
                + (f32::from(bounds.size.height) - doc.height as f32 * scale) / 2.;
            for node in &doc.nodes {
                if let NodeKind::Text { spec, .. } = &node.kind {
                    let mut font = gpui::font(spec.font.clone());
                    if spec.bold {
                        font.weight = FontWeight::BOLD;
                    }
                    for (index, text) in spec.text.lines().enumerate() {
                        let line = window.text_system().shape_line(
                            text.to_owned().into(),
                            px(spec.size * scale),
                            &[gpui::TextRun {
                                len: text.len(),
                                font: font.clone(),
                                color: gpui::rgba(u32::from_be_bytes(spec.color)).into(),
                                background_color: None,
                                underline: None,
                                strikethrough: None,
                            }],
                            None,
                        );
                        let width = spec.width.unwrap_or(0.) * scale;
                        let offset = match spec.align {
                            emulsion_core::text::Align::Center => {
                                (width - f32::from(line.width)) / 2.
                            }
                            emulsion_core::text::Align::Right => width - f32::from(line.width),
                            _ => 0.,
                        };
                        let at = point(
                            px(dx + spec.x * scale + offset),
                            px(dy + (spec.y + index as f32 * spec.size * spec.line_height) * scale),
                        );
                        let _ = line.paint(
                            at,
                            px(spec.size * spec.line_height * scale),
                            gpui::TextAlign::Left,
                            None,
                            window,
                            cx,
                        );
                    }
                    continue;
                }
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
