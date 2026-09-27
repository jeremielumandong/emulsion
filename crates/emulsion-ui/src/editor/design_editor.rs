//! Design's focused canvas and selection controls from the UI handoff.
use super::*;
use emulsion_core::text::Align;
use gpui_kit::component::{
    Disableable, Selectable, Sizable,
    button::{Button, ButtonVariants},
    menu::{DropdownMenu, PopupMenuItem},
};

fn small_button(id: &'static str, label: impl Into<SharedString>) -> Button {
    let label = label.into();
    Button::new(id)
        .accessibility_label(label.clone())
        .xsmall()
        .ghost()
        .h(px(24.))
        .min_w(px(24.))
        .px(px(7.))
        .child(div().text_size(px(11.)).child(label))
}
impl EditorView {
    pub(super) fn design_editor(
        &mut self,
        p: &Palette,
        window: &mut Window,
        cx: &mut Context<Self>,
    ) -> AnyElement {
        if self.history.open {
            return div()
                .flex()
                .flex_col()
                .flex_1()
                .min_h_0()
                .track_focus(&self.focus)
                .child(self.history_page(p, cx))
                .into_any_element();
        }
        self.refresh_suggestions(cx);
        let tabs = self.document_tabs.clone();
        let toolbar = self.design_canvas_toolbar(p, window, cx);
        let canvas = self.canvas_region();
        let inspector = self.is_diagram() || self.design_ui.inspector;
        let tabs = tabs.map(|tabs| {
            div()
                .id("document-tab-bar")
                .test_support()
                .h(px(38.))
                .flex_none()
                .flex()
                .items_end()
                .min_w_0()
                .bg(p.paper)
                .border_b_1()
                .border_color(p.line)
                .child(tabs)
        });
        div()
            .id("design-editor")
            .test_support()
            .flex()
            .flex_col()
            .flex_1()
            .min_w_0()
            .min_h_0()
            .track_focus(&self.focus)
            .on_modifiers_changed(cx.listener(|this, event: &ModifiersChangedEvent, _, cx| {
                this.drag_shift = event.modifiers.shift;
                this.notify_canvas(cx);
            }))
            .children(self.size_panel_view(p, cx))
            .children(self.export_panel_view(p, cx))
            .children(self.ask_area(p, cx))
            .child(
                div()
                    .id("editor-work-area")
                    .test_support()
                    .flex()
                    .flex_1()
                    .min_w_0()
                    .min_h_0()
                    .children(self.design_drawer(p, window, cx))
                    .children(self.diagram_drawer(p, window, cx))
                    .child(
                        div()
                            .id("editor-canvas-column")
                            .test_support()
                            .flex()
                            .flex_col()
                            .flex_1()
                            .min_w_0()
                            .min_h_0()
                            .overflow_hidden()
                            .children(tabs)
                            .children(toolbar)
                            .children(self.diagram_canvas_toolbar(p, window, cx))
                            .when(
                                !matches!(
                                    self.tool,
                                    Tool::Move | Tool::Type | Tool::Hand | Tool::Zoom
                                ),
                                |column| column.child(self.context_bar(p, window, cx)),
                            )
                            .child(canvas)
                            .children(self.project_page_strip(p, cx)),
                    )
                    .when(inspector, |row| row.child(self.sidebar_region(window, cx))),
            )
            .children(self.picker(p, window, cx))
            .children(self.assistant_dock(p, cx))
            .child(self.status_strip(p, cx))
            .into_any_element()
    }
    pub(super) fn design_selection_toolbar(
        &mut self,
        p: &Palette,
        window: &mut Window,
        cx: &mut Context<Self>,
    ) -> Option<AnyElement> {
        if !self.is_design()
            || self.previewing()
            || self.drag.is_some()
            || self.selected_layer_ids().len() != 1
        {
            return None;
        }
        let id = self.selected?;
        let node = self.editor.doc.node(id)?;
        let locked = self.editor.doc.locked_ancestor(id).is_some();
        let bounds = emulsion_core::geometry::node_bounds(&self.editor.doc, id)?;
        let canvas = self.canvas_bounds()?;
        let points = [
            (bounds.x, bounds.y),
            (bounds.right(), bounds.y),
            (bounds.x, bounds.bottom()),
            (bounds.right(), bounds.bottom()),
        ]
        .map(|(x, y)| self.view.doc_to_screen((x as f64, y as f64), &canvas));
        let left = points.iter().map(|p| p.0).fold(f64::INFINITY, f64::min)
            - f64::from(f32::from(canvas.origin.x));
        let right = points.iter().map(|p| p.0).fold(f64::NEG_INFINITY, f64::max)
            - f64::from(f32::from(canvas.origin.x));
        let top = points.iter().map(|p| p.1).fold(f64::INFINITY, f64::min)
            - f64::from(f32::from(canvas.origin.y));
        let text = matches!(node.kind, NodeKind::Text { .. });
        let width = if text { 324. } else { 250. };
        let x = (((left + right) / 2.) as f32 - width / 2.)
            .clamp(8., (f32::from(canvas.size.width) - width - 8.).max(8.));
        let y = (top as f32 - 38.).clamp(8., (f32::from(canvas.size.height) - 38.).max(8.));
        let mut bar = div()
            .id("design-selection-toolbar")
            .test_support()
            .absolute()
            .left(px(x))
            .top(px(y))
            .w(px(width))
            .h(px(32.))
            .flex()
            .items_center()
            .gap(px(2.))
            .p(px(3.))
            .rounded(px(8.))
            .bg(p.panel)
            .border_1()
            .border_color(p.line)
            .shadow_md()
            .occlude()
            .on_mouse_down(MouseButton::Left, |_, _, cx| cx.stop_propagation());
        if let Some((_, spec)) = self.text_target() {
            let style = spec.style_at(self.text_style_range().map_or(0, |r| r.start));
            let owner = cx.weak_entity();
            let font = if style.font.is_empty() {
                "Default".into()
            } else {
                style.font.clone()
            };
            bar = bar
                .child(
                    small_button("design-text-font", font.clone())
                        .w(px(104.))
                        .disabled(locked)
                        .dropdown_menu(move |mut menu, _, _| {
                            let mut fonts = emulsion_core::text::font_families();
                            for bundled in ["Geist", "Geist Mono"] {
                                if !fonts.iter().any(|f| f == bundled) {
                                    fonts.push(bundled.into());
                                }
                            }
                            fonts.sort();
                            fonts.dedup();
                            for font in fonts {
                                let owner = owner.clone();
                                menu = menu.item(PopupMenuItem::new(font.clone()).on_click(
                                    move |_, _, cx| {
                                        owner
                                            .update(cx, |this, cx| {
                                                this.restyle_text(
                                                    |spec| spec.font = font.clone(),
                                                    cx,
                                                )
                                            })
                                            .ok();
                                    },
                                ));
                            }
                            menu
                        }),
                )
                .child(self.design_text_size_input(window, cx))
                .child(
                    small_button("design-text-bold", "B")
                        .selected(style.bold)
                        .disabled(locked)
                        .on_click(cx.listener(|this, _, _, cx| {
                            this.restyle_text(|s| s.bold = !s.bold, cx)
                        })),
                )
                .child(
                    small_button("design-text-italic", "I")
                        .selected(style.italic)
                        .disabled(locked)
                        .on_click(cx.listener(|this, _, _, cx| {
                            this.restyle_text(|s| s.italic = !s.italic, cx)
                        })),
                )
                .child(
                    small_button("design-text-properties", "Aa")
                        .tooltip("Character and paragraph")
                        .on_click(cx.listener(|this, _, _, cx| {
                            this.select_sidebar(SidebarTab::Properties, cx)
                        })),
                );
            let owner = cx.weak_entity();
            bar = bar.child(
                Button::new("design-text-align")
                    .accessibility_label("Text alignment")
                    .xsmall()
                    .ghost()
                    .size(px(24.))
                    .disabled(locked)
                    .child(
                        rail::tool_icon("align-left")
                            .text_color(p.ink)
                            .size(px(11.)),
                    )
                    .dropdown_menu(move |mut menu, _, _| {
                        for (label, align) in [
                            ("Left", Align::Left),
                            ("Center", Align::Center),
                            ("Right", Align::Right),
                            ("Justify", Align::Justify),
                        ] {
                            let owner = owner.clone();
                            menu =
                                menu.item(PopupMenuItem::new(label).on_click(move |_, _, cx| {
                                    owner
                                        .update(cx, |this, cx| {
                                            this.restyle_text(|s| s.align = align, cx)
                                        })
                                        .ok();
                                }));
                        }
                        menu
                    }),
            );
        } else {
            bar = bar
                .child(
                    small_button("design-object-properties", "Properties").on_click(cx.listener(
                        |this, _, _, cx| this.select_sidebar(SidebarTab::Properties, cx),
                    )),
                )
                .child(
                    small_button("design-object-duplicate", "Duplicate")
                        .disabled(locked)
                        .on_click(cx.listener(|this, _, _, cx| this.duplicate_selected(cx))),
                )
                .child(
                    small_button("design-object-delete", "Delete")
                        .disabled(locked)
                        .on_click(cx.listener(|this, _, _, cx| this.delete_selected(cx))),
                );
        }
        Some(
            bar.child(
                Button::new("design-selection-magic")
                    .accessibility_label("Ask about this selection")
                    .tooltip("Ask about this selection")
                    .xsmall()
                    .ghost()
                    .size(px(24.))
                    .child(rail::tool_icon("sparkles").text_color(p.ink).size(px(11.)))
                    .on_click(cx.listener(|this, _, window, cx| this.open_ask(window, cx))),
            )
            .into_any_element(),
        )
    }
}
