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
        self.retire_page_thumbnails(window);
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
        let organizing = self.pages_ui.organizer.open && self.is_design();
        let canvas = if organizing {
            self.page_organizer(p, window, cx)
        } else {
            self.canvas_region().into_any_element()
        };
        let inspector = !organizing && (self.is_diagram() || self.design_ui.inspector);
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
            .key_context(if self.design_ui.asset_job.is_some() {
                "DesignAssetLoading"
            } else {
                "DesignEditor"
            })
            .capture_key_down(cx.listener(|this, event: &KeyDownEvent, _, cx| {
                if event.keystroke.key == "escape" && this.cancel_design_asset_load(cx) {
                    cx.stop_propagation();
                }
            }))
            .on_modifiers_changed(cx.listener(|this, event: &ModifiersChangedEvent, _, cx| {
                this.drag_shift = event.modifiers.shift;
                this.notify_canvas(cx);
            }))
            .children(self.size_panel_view(p, cx))
            .children(self.ask_area(p, cx))
            .children(self.design_asset_loading_controls(cx))
            .child(
                div()
                    .id("editor-work-area")
                    .test_support()
                    .flex()
                    .flex_1()
                    .min_w_0()
                    .min_h_0()
                    .when(!organizing, |row| {
                        row.children(self.design_library_region(p, window, cx))
                    })
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
                            .when(!organizing, |column| column.children(toolbar))
                            .when(!organizing, |column| {
                                column.children(self.design_direct_controls(p, window, cx))
                            })
                            .children(self.diagram_canvas_toolbar(p, window, cx))
                            .when(
                                !organizing
                                    && !matches!(
                                        self.tool,
                                        Tool::Move | Tool::Type | Tool::Hand | Tool::Zoom
                                    ),
                                |column| column.child(self.context_bar(p, window, cx)),
                            )
                            .child(canvas)
                            .when(!organizing, |column| {
                                column.children(self.project_page_strip(p, cx))
                            }),
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
        if !self.design_full_tools() {
            return None;
        }
        self.design_selection_toolbar_content(p, window, cx, true)
    }

    pub(super) fn design_selection_toolbar_content(
        &mut self,
        p: &Palette,
        window: &mut Window,
        cx: &mut Context<Self>,
        floating: bool,
    ) -> Option<AnyElement> {
        if !self.is_design() || self.previewing() {
            return None;
        }
        let ids = self.selected_layer_roots();
        if ids.is_empty() {
            if floating {
                return None;
            }
            return Some(
                div()
                    .text_size(px(11.))
                    .text_color(p.muted)
                    .child(t!("design.direct.select_hint").to_string())
                    .into_any_element(),
            );
        }
        if ids.len() > 1 {
            if floating {
                return None;
            }
            return Some(
                div()
                    .id("design-multiple-selection-toolbar")
                    .test_support()
                    .flex()
                    .flex_none()
                    .items_center()
                    .gap_1()
                    .child(self.design_selection_target(p, cx))
                    .children(self.design_direct_appearance_actions(cx))
                    .children(self.design_selection_actions(cx))
                    .child(self.design_advanced_appearance_button(cx))
                    .into_any_element(),
            );
        }
        if floating && self.drag.is_some() {
            return None;
        }
        let id = self.selected?;
        let node = self.editor.doc.node(id)?;
        let locked =
            self.editor.doc.locked_ancestor(id).is_some() || self.editor.doc.layer_locks(id).pixels;
        let text = matches!(node.kind, NodeKind::Text { .. });
        let raster = matches!(node.kind, NodeKind::Raster { .. } | NodeKind::Smart { .. });
        let plain_image = matches!(node.kind, NodeKind::Raster { .. });
        let smart_image = matches!(node.kind, NodeKind::Smart { .. });
        let frame_parts = emulsion_core::design::frame_parts(&self.editor.doc, id);
        let chart = self.editor.doc.design.charts.contains_key(&id);
        let placement = if floating {
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
            let bottom = points.iter().map(|p| p.1).fold(f64::NEG_INFINITY, f64::max)
                - f64::from(f32::from(canvas.origin.y));
            let width = if text {
                f32::from(window.rem_size()) * 27.5
            } else if chart {
                350.
            } else {
                276.
            };
            let width = width.min((f32::from(canvas.size.width) - 16.).max(1.));
            let x = (((left + right) / 2.) as f32 - width / 2.)
                .clamp(8., (f32::from(canvas.size.width) - width - 8.).max(8.));
            let y = if top >= 46. {
                top as f32 - 38.
            } else {
                bottom as f32 + 8.
            }
            .clamp(8., (f32::from(canvas.size.height) - 38.).max(8.));
            Some((x, y, width))
        } else {
            None
        };
        let mut bar = div()
            .id("design-selection-toolbar")
            .test_support()
            .flex()
            .flex_none()
            .items_center()
            .gap(px(2.))
            .when_some(placement, |bar, (x, y, width)| {
                bar.absolute()
                    .left(px(x))
                    .top(px(y))
                    .w(px(width))
                    .min_h(px(32.))
                    .flex_wrap()
                    .p(px(3.))
                    .rounded(px(8.))
                    .bg(p.panel)
                    .border_1()
                    .border_color(p.line)
                    .shadow_md()
                    .occlude()
            })
            .when(!floating, |bar| {
                bar.child(self.design_selection_target(p, cx))
            })
            .on_mouse_down(MouseButton::Left, |_, _, cx| cx.stop_propagation());
        // Keep the everyday photo actions first in the scrolling selection row.
        // Properties and advanced appearance controls must not push them offscreen.
        if let Some((_, image)) = frame_parts {
            let replaceable =
                emulsion_core::design::frame_image_replaceable(&self.editor.doc, id).is_ok();
            let croppable = image.is_some()
                && emulsion_core::design::frame_image_editable(&self.editor.doc, id).is_ok();
            bar = bar
                .child(
                    small_button(
                        "design-selection-replace-frame",
                        t!("design.direct.replace"),
                    )
                    .disabled(!replaceable)
                    .on_click(cx.listener(|this, _, _, cx| this.choose_frame_image(cx))),
                )
                .child(
                    small_button("design-selection-crop-frame", t!("design.direct.crop"))
                        .disabled(!croppable)
                        .on_click(
                            cx.listener(|this, _, window, cx| this.start_frame_crop(window, cx)),
                        ),
                );
        }
        if let Some((_, spec)) = self.text_target() {
            let style = spec.style_at(self.text_style_range().map_or(0, |r| r.start));
            let font = self.font_label(&style.font);
            let font_bounds = TrackBounds::default();
            let anchor = font_bounds.clone();
            bar = bar
                .child(
                    small_button("design-text-font", font)
                        .w(px(104.))
                        .min_w_0()
                        .overflow_hidden()
                        .relative()
                        .disabled(locked)
                        .tooltip(t!("design.direct.font_search").to_string())
                        .child(
                            gpui_kit::canvas(
                                move |bounds, _, _| font_bounds.set(Some(bounds)),
                                |_, _, _, _| {},
                            )
                            .absolute()
                            .size_full(),
                        )
                        .on_click(cx.listener(move |this, _, window, cx| {
                            this.toggle_font_picker(anchor.get(), window, cx)
                        })),
                )
                .child(self.design_text_size_input(window, cx))
                .when(!floating, |bar| {
                    bar.children(self.design_direct_appearance_actions(cx))
                })
                .child(
                    small_button("design-text-bold", "B")
                        .accessibility_label(t!("design.direct.bold").to_string())
                        .tooltip(t!("design.direct.bold").to_string())
                        .selected(style.bold)
                        .disabled(locked)
                        .on_click(cx.listener(|this, _, _, cx| {
                            this.restyle_text(|s| s.bold = !s.bold, cx)
                        })),
                )
                .child(
                    small_button("design-text-italic", "I")
                        .accessibility_label(t!("design.direct.italic").to_string())
                        .tooltip(t!("design.direct.italic").to_string())
                        .selected(style.italic)
                        .disabled(locked)
                        .on_click(cx.listener(|this, _, _, cx| {
                            this.restyle_text(|s| s.italic = !s.italic, cx)
                        })),
                )
                .when(floating, |bar| {
                    bar.child(
                        small_button(
                            "design-text-auto-width",
                            t!("editor.design_editor.auto_width"),
                        )
                        .tooltip(t!("editor.design_editor.auto_width_tip"))
                        .disabled(locked || spec.text_path.is_some())
                        .on_click(cx.listener(|this, _, _, cx| this.auto_width_text(cx))),
                    )
                })
                .child(
                    small_button(
                        "design-text-properties",
                        t!("design.direct.text_options").to_string(),
                    )
                    .tooltip(t!("editor.design_editor.character_paragraph"))
                    .on_click(
                        cx.listener(|this, _, _, cx| {
                            this.select_sidebar(SidebarTab::Properties, cx)
                        }),
                    ),
                );
            let owner = cx.weak_entity();
            bar = bar.child(
                Button::new("design-text-align")
                    .accessibility_label(t!("editor.design_editor.text_alignment"))
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
                            (t!("editor.design_editor.align_left"), Align::Left),
                            (t!("editor.design_editor.align_center"), Align::Center),
                            (t!("editor.design_editor.align_right"), Align::Right),
                            (t!("editor.design_editor.align_justify"), Align::Justify),
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
            if !floating {
                bar = bar.children(self.design_direct_appearance_actions(cx));
            }
            // Deep-picking a frame's image must expose the same visual crop and
            // Cover replacement as selecting its boundary or group. The source
            // pixel operations remain available in More for advanced edits.
            if !floating && raster && frame_parts.is_none() {
                bar = bar
                    .child(
                        small_button("design-image-crop", t!("design.direct.crop").to_string())
                            .disabled(locked)
                            .on_click(cx.listener(move |this, _, window, cx| {
                                this.crop_photo_source_dialog(id, window, cx)
                            })),
                    )
                    .child(
                        small_button(
                            "design-image-replace",
                            t!("design.direct.replace").to_string(),
                        )
                        .disabled(locked)
                        .on_click(cx.listener(move |this, _, _, cx| {
                            this.replace_photo_source_dialog(id, cx)
                        })),
                    )
                    .when(plain_image, |bar| {
                        let error =
                            emulsion_core::design_background::can_set_image(&self.editor.doc, id)
                                .err();
                        bar.child(
                            small_button(
                                "design-image-set-background",
                                t!("design.direct.set_background").to_string(),
                            )
                            .disabled(error.is_some())
                            .tooltip(
                                error.unwrap_or_else(|| {
                                    t!("design.direct.set_background").to_string()
                                }),
                            )
                            .on_click(cx.listener(
                                |this, _, window, cx| {
                                    this.preview_selected_page_background(window, cx)
                                },
                            )),
                        )
                    });
            }
            bar = bar
                .when(chart, |bar| {
                    bar.child(
                        small_button(
                            "design-chart-edit-selection",
                            t!("editor.design_editor.edit_data"),
                        )
                        .disabled(locked)
                        .on_click(cx.listener(|this, _, window, cx| {
                            this.design_chart_dialog(
                                emulsion_core::design_charts::Kind::Bar,
                                true,
                                window,
                                cx,
                            )
                        })),
                    )
                })
                .child(
                    small_button("design-object-properties", t!("window.properties")).on_click(
                        cx.listener(|this, _, _, cx| {
                            this.select_sidebar(SidebarTab::Properties, cx)
                        }),
                    ),
                )
                .child(
                    small_button("design-object-duplicate", t!("design.direct.duplicate"))
                        .disabled(locked)
                        .on_click(cx.listener(|this, _, _, cx| this.duplicate_selected(cx))),
                )
                .child(
                    small_button("design-object-delete", t!("design.direct.delete"))
                        .disabled(locked)
                        .on_click(cx.listener(|this, _, _, cx| this.delete_selected(cx))),
                );
        }
        if !floating {
            bar = bar.child(self.design_advanced_appearance_button(cx));
        }
        let editor = cx.entity();
        let can_paste = self.design_ui.copied_appearance.is_some() && !locked;
        let can_background =
            emulsion_core::design_background::can_set_image(&self.editor.doc, id).is_ok();
        bar = bar.child(
            Button::new("design-selection-more")
                .accessibility_label(t!("editor.design_editor.object_actions"))
                .tooltip(t!("editor.design_editor.object_actions"))
                .xsmall()
                .ghost()
                .size(px(24.))
                .child(rail::tool_icon("ellipsis").text_color(p.ink).size(px(12.)))
                .dropdown_menu(move |menu, _, _| {
                    use super::layer_menu::item;
                    menu.when(plain_image, |menu| {
                        menu.item(item(
                            &editor,
                            t!("design.direct.set_background").to_string(),
                            can_background,
                            |e, window, cx| e.preview_selected_page_background(window, cx),
                        ))
                    })
                    .item(item(
                        &editor,
                        t!("editor.design_editor.layer_style"),
                        !locked,
                        move |e, window, cx| e.open_layer_styles_dialog(id, window, cx),
                    ))
                    .item(item(
                        &editor,
                        t!("editor.design_controls.copy_style"),
                        true,
                        |e, _, cx| e.copy_design_appearance(cx),
                    ))
                    .item(item(
                        &editor,
                        t!("editor.design_controls.paste_style"),
                        can_paste,
                        |e, _, cx| e.paste_design_appearance(cx),
                    ))
                    .item(item(
                        &editor,
                        t!("editor.design_editor.save_style"),
                        true,
                        |e, window, cx| e.save_design_style_dialog(None, window, cx),
                    ))
                    .item(item(
                        &editor,
                        t!("editor.design_editor.saved_styles"),
                        true,
                        |e, _, cx| e.show_design_section(super::design_ui::Section::Brand, cx),
                    ))
                    .item(item(
                        &editor,
                        t!("editor.design_editor.bind_csv"),
                        !locked,
                        |e, window, cx| e.design_data_binding_dialog(window, cx),
                    ))
                    .when(raster, |menu| {
                        menu.item(item(
                            &editor,
                            t!("editor.design_editor.trace_vector"),
                            !locked,
                            move |e, window, cx| e.show_bitmap_trace(id, window, cx),
                        ))
                    })
                    .when(raster, |menu| {
                        menu.item(item(
                            &editor,
                            t!("editor.design_editor.replace_source"),
                            !locked,
                            move |e, _, cx| e.replace_photo_source_dialog(id, cx),
                        ))
                        .item(item(
                            &editor,
                            t!("editor.design_editor.crop_image"),
                            !locked,
                            move |e, window, cx| e.crop_photo_source_dialog(id, window, cx),
                        ))
                        .item(item(
                            &editor,
                            t!("editor.design_editor.adjust_curves"),
                            !locked,
                            move |e, _, cx| e.adjust_design_photo(id, "curves", cx),
                        ))
                        .item(item(
                            &editor,
                            t!("editor.design_editor.adjust_hue"),
                            !locked,
                            move |e, _, cx| e.adjust_design_photo(id, "hue_saturation", cx),
                        ))
                        .item(item(
                            &editor,
                            t!("editor.design_editor.filter_blur"),
                            !locked,
                            |e, _, cx| e.apply_filter_key("gaussian_blur", cx),
                        ))
                        .item(item(
                            &editor,
                            t!("editor.design_editor.filter_sharpen"),
                            !locked,
                            |e, _, cx| e.apply_filter_key("unsharp_mask", cx),
                        ))
                        .item(item(
                            &editor,
                            t!("editor.design_editor.effects_blending"),
                            !locked,
                            move |e, window, cx| e.open_blending_options(id, window, cx),
                        ))
                    })
                    .when(plain_image, |menu| {
                        menu.item(item(
                            &editor,
                            t!("editor.design_editor.remove_bg"),
                            !locked,
                            |e, _, cx| e.remove_background(cx),
                        ))
                    })
                    .when(smart_image, |menu| {
                        menu.item(item(
                            &editor,
                            t!("editor.design_editor.edit_smart"),
                            !locked,
                            move |e, _, cx| {
                                e.dispatch_smart_source(
                                    emulsion_mcp::smart_source_tools::Action::Open { node: id },
                                    cx,
                                )
                            },
                        ))
                        .item(item(
                            &editor,
                            t!("editor.design_editor.link_smart"),
                            !locked,
                            move |e, _, cx| e.smart_link_dialog(id, cx),
                        ))
                        .item(item(
                            &editor,
                            t!("editor.design_editor.restore_smart"),
                            !locked,
                            |e, _, cx| e.convert_smart_to_layers(cx),
                        ))
                    })
                    .item(item(
                        &editor,
                        t!("editor.design_editor.flip_horizontal"),
                        !locked,
                        |e, _, cx| e.flip_transform_selection(true, cx),
                    ))
                    .item(item(
                        &editor,
                        t!("editor.design_editor.flip_vertical"),
                        !locked,
                        |e, _, cx| e.flip_transform_selection(false, cx),
                    ))
                    .item(item(
                        &editor,
                        if locked {
                            t!("design.direct.unlock")
                        } else {
                            t!("design.direct.lock")
                        },
                        true,
                        move |e, _, cx| {
                            e.execute(
                                Command::SetLocked {
                                    id,
                                    locked: !locked,
                                },
                                cx,
                            );
                        },
                    ))
                    .item(item(
                        &editor,
                        t!("design.direct.duplicate"),
                        !locked,
                        |e, _, cx| e.duplicate_selected(cx),
                    ))
                    .item(item(
                        &editor,
                        t!("design.direct.delete"),
                        !locked,
                        |e, _, cx| e.delete_selected(cx),
                    ))
                }),
        );
        Some(
            bar.child(
                Button::new("design-selection-magic")
                    .accessibility_label(t!("editor.design_editor.ask_selection"))
                    .tooltip(t!("editor.design_editor.ask_selection"))
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
