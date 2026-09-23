//! History and task controls above the Layers, Channels and Paths dock.
use super::*;
use gpui_kit::component::{
    Sizable,
    button::{Button, ButtonVariants},
};

#[derive(Default)]
pub(crate) struct SidebarState {
    pub width: Option<f32>,
    pub collapsed: bool,
}

impl SidebarState {
    fn width_for_viewport(&self, viewport_width: f32, rem_size: f32) -> Option<f32> {
        let available = (viewport_width - 280.).max(0.);
        let minimum = 13.75 * rem_size;
        if self.collapsed || available < minimum {
            return None;
        }
        Some(
            self.width
                .unwrap_or(20. * rem_size)
                .clamp(minimum, 35. * rem_size)
                .min(available),
        )
    }

    fn snapped_width(width: f32, rem_size: f32) -> f32 {
        if width / rem_size < 21.25 {
            25. * rem_size
        } else {
            18.75 * rem_size
        }
    }
}

#[cfg(test)]
mod tests {
    use super::SidebarState;

    #[test]
    fn sidebar_preserves_canvas_and_restores_requested_width() {
        let state = SidebarState {
            width: Some(560.),
            collapsed: false,
        };
        assert_eq!(state.width_for_viewport(600., 16.), Some(320.));
        assert_eq!(state.width_for_viewport(499., 16.), None);
        assert_eq!(state.width_for_viewport(1200., 16.), Some(560.));
    }

    #[test]
    fn sidebar_geometry_and_snap_follow_interface_zoom() {
        let state = SidebarState::default();
        assert_eq!(state.width_for_viewport(1200., 20.), Some(400.));
        assert_eq!(SidebarState::snapped_width(400., 20.), 500.);
        assert_eq!(SidebarState::snapped_width(500., 20.), 375.);
        let collapsed = SidebarState {
            collapsed: true,
            ..Default::default()
        };
        assert_eq!(collapsed.width_for_viewport(1200., 20.), None);
    }
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub(crate) enum DockTab {
    Layers,
    Channels,
    Paths,
}

#[derive(Clone, Copy, PartialEq, Eq)]
pub(crate) enum SidebarTab {
    Properties,
    Adjustments,
    Reference,
    Navigator,
    Info,
    Recipes,
    Timeline,
    History,
    Histogram,
    BrushSettings,
    BrushPresets,
    BlendingOptions,
}

impl EditorView {
    /// Open the panel dock on Layers, Channels or Paths.
    pub(crate) fn show_dock_tab(
        &mut self,
        tab: DockTab,
        window: &mut Window,
        cx: &mut Context<Self>,
    ) {
        self.sidebar_layout.collapsed = false;
        self.dock_tab = tab;
        self.menu = None;
        window.focus(&self.panel_focus, cx);
        cx.notify();
    }

    /// Photoshop's Tab: hide or show the panel dock.
    pub(crate) fn toggle_panel_dock(&mut self, cx: &mut Context<Self>) {
        self.sidebar_layout.collapsed = !self.sidebar_layout.collapsed;
        cx.notify();
    }

    /// Photoshop's F7: the Layers panel.
    pub(crate) fn show_layers_panel(&mut self, window: &mut Window, cx: &mut Context<Self>) {
        self.show_dock_tab(DockTab::Layers, window, cx)
    }

    /// Photoshop's F8: the Info panel.
    pub(crate) fn show_info_panel(&mut self, cx: &mut Context<Self>) {
        self.show_sidebar_tab(SidebarTab::Info, cx)
    }

    /// Photoshop's F5: the Brush Settings panel.
    pub(crate) fn show_brush_settings(&mut self, cx: &mut Context<Self>) {
        self.show_sidebar_tab(SidebarTab::BrushSettings, cx)
    }

    /// Open the panel dock on one of its upper tabs.
    pub(crate) fn show_sidebar_tab(&mut self, tab: SidebarTab, cx: &mut Context<Self>) {
        self.sidebar_layout.collapsed = false;
        self.select_sidebar(tab, cx);
    }

    /// Resolve temporary previews before hiding their Apply/Cancel controls.
    pub(crate) fn select_sidebar(&mut self, tab: SidebarTab, cx: &mut Context<Self>) {
        if tab != self.sidebar_tab {
            self.finish_shape_color_edit(cx);
        }
        if tab == SidebarTab::Recipes {
            self.recipes.open = true;
            self.reload_recipes();
        }
        if tab != SidebarTab::Recipes && self.recipes.preview.is_some() {
            self.cancel_preview(cx);
            self.set_status("Unapplied recipe preview canceled.", false, cx);
        }
        if tab != SidebarTab::Timeline && self.anim.open {
            self.anim.open = false;
            self.anim.playing = false;
            self.seen_rev = u64::MAX;
            self.tree_dirty = emulsion_core::Dirty::All;
        }
        if tab != SidebarTab::Timeline {
            self.anim.replay = None;
        }
        self.sidebar_tab = tab;
        self.sidebar_layout.collapsed = false;
        if tab == SidebarTab::Info {
            self.panels.info = true;
        }
        self.presets.open = tab == SidebarTab::BrushPresets;
        self.sidebar_menu = false;
        self.menu = None;
        cx.notify();
    }

    pub(super) fn sidebar(
        &mut self,
        p: &Palette,
        window: &mut Window,
        cx: &mut Context<Self>,
    ) -> AnyElement {
        let compact = crate::app_state::settings(cx).compact_chrome;
        let rem_size = f32::from(window.rem_size());
        let visible_width = self
            .sidebar_layout
            .width_for_viewport(f32::from(window.viewport_size().width), rem_size);
        let sidebar_width = visible_width.unwrap_or(20. * rem_size);
        self.layer_panel.compact = compact || f32::from(window.viewport_size().height) < 700.;
        if compact && visible_width.is_none() {
            return div()
                .id("sidebar-collapsed")
                .flex()
                .flex_col()
                .flex_none()
                .w(rems(1.875))
                .h_full()
                .border_l_1()
                .border_color(p.line)
                .bg(p.panel)
                .child(
                    Button::new("sidebar-expand")
                        .ghost()
                        .xsmall()
                        .label("‹")
                        .accessibility_label("Expand sidebar")
                        .tooltip("Expand sidebar")
                        .on_click(cx.listener(|this, _, _, cx| {
                            this.sidebar_layout.collapsed = false;
                            cx.notify();
                        })),
                )
                .children(
                    [
                        (SidebarTab::Info, "I", "Info"),
                        (SidebarTab::Properties, "P", "Properties"),
                        (SidebarTab::Adjustments, "A", "Adjustments"),
                        (SidebarTab::History, "H", "History"),
                        (SidebarTab::Reference, "R", "Reference"),
                    ]
                    .into_iter()
                    .map(|(tab, label, title)| {
                        Button::new(("sidebar-rail", tab as usize))
                            .ghost()
                            .xsmall()
                            .label(label)
                            .accessibility_label(title)
                            .tooltip(title)
                            .on_click(
                                cx.listener(move |this, _, _, cx| this.select_sidebar(tab, cx)),
                            )
                    }),
                )
                // Photoshop's collapsed dock lists the Layers group too.
                .child(div().h(px(1.)).mx_1().my_1().bg(p.line))
                .children(
                    [
                        (DockTab::Layers, "L", "Layers"),
                        (DockTab::Channels, "C", "Channels"),
                        (DockTab::Paths, "Pa", "Paths"),
                    ]
                    .into_iter()
                    .map(|(tab, label, title)| {
                        Button::new(("sidebar-rail-dock", tab as usize))
                            .ghost()
                            .xsmall()
                            .label(label)
                            .accessibility_label(title)
                            .tooltip(title)
                            .on_click(cx.listener(move |this, _, window, cx| {
                                this.show_dock_tab(tab, window, cx)
                            }))
                    }),
                )
                .test_support()
                .into_any_element();
        }
        let dock_bounds = self.layer_panel.dock_bounds.clone();
        let tabs = div()
            .flex()
            .flex_none()
            .h(if compact { rems(1.625) } else { rems(2.125) })
            .border_t_1()
            .border_b_1()
            .border_color(p.line)
            .children(
                [
                    (SidebarTab::Info, "sidebar-info-top", "Info"),
                    (SidebarTab::Properties, "sidebar-properties", "Properties"),
                    (
                        SidebarTab::Adjustments,
                        "sidebar-adjustments",
                        "Adjustments",
                    ),
                    (SidebarTab::History, "sidebar-history-top", "History"),
                    (SidebarTab::Reference, "sidebar-reference", "Reference"),
                ]
                .into_iter()
                .filter(|(tab, _, _)| compact || *tab != SidebarTab::Info)
                .map(|(tab, id, title)| {
                    let active = self.sidebar_tab == tab;
                    div()
                        .id(id)
                        .role(gpui_kit::Role::Button)
                        .aria_label(title)
                        .aria_selected(active)
                        .focusable()
                        .tab_index(0)
                        .flex_1()
                        .min_w_0()
                        .flex()
                        .items_center()
                        .justify_center()
                        .h_full()
                        .text_center()
                        .text_size(px(11.))
                        .text_color(if active { p.ink } else { p.muted })
                        .border_b_2()
                        .border_color(if active {
                            p.accent
                        } else {
                            transparent_black()
                        })
                        .cursor_pointer()
                        .child(if compact {
                            match tab {
                                SidebarTab::Properties => "Props",
                                SidebarTab::Adjustments => "Adjust",
                                SidebarTab::Reference => "Ref",
                                _ => title,
                            }
                        } else {
                            title
                        })
                        .on_click(cx.listener(move |this, _, _, cx| {
                            this.select_sidebar(tab, cx);
                        }))
                        .test_support()
                }),
            )
            .when(compact, |d| {
                d.child(
                    Button::new("sidebar-collapse")
                        .ghost()
                        .xsmall()
                        .label("›")
                        .accessibility_label("Collapse sidebar")
                        .tooltip("Collapse sidebar")
                        .on_click(cx.listener(|this, _, _, cx| {
                            this.sidebar_layout.collapsed = true;
                            cx.notify();
                        })),
                )
            });
        let content = match self.sidebar_tab {
            SidebarTab::BlendingOptions => self.blending_options_panel(p, cx),
            SidebarTab::BrushSettings => self.brush_settings_panel(p, cx),
            SidebarTab::BrushPresets => div().children(self.presets_view(p, cx)).into_any_element(),
            SidebarTab::Properties => div()
                .id("sidebar-properties-content")
                .children(self.shape_properties(window, cx))
                .children(self.text_properties(window, cx))
                .child(self.inspector(p, window, cx))
                .test_support()
                .into_any_element(),
            SidebarTab::Adjustments => div()
                .id("sidebar-adjustments-content")
                .child(self.quick_adjust_view(p, cx))
                .test_support()
                .into_any_element(),
            SidebarTab::Reference => self.reference_panel(p, cx),
            SidebarTab::Navigator => div()
                .children(self.navigator_view(p, cx))
                .into_any_element(),
            SidebarTab::Info => div().children(self.info_view(p)).into_any_element(),
            SidebarTab::Recipes => div()
                .children(self.recipes_view(p, window, cx))
                .into_any_element(),
            SidebarTab::Timeline => div()
                .children(self.animation_panel(p, cx))
                .child(
                    div()
                        .p(px(12.))
                        .flex()
                        .flex_wrap()
                        .gap(px(6.))
                        .child(
                            chip("replay", "Replay drawing", self.anim.replay.is_some(), p)
                                .on_click(cx.listener(|this, _, _, cx| this.replay_start(cx)))
                                .test_support(),
                        )
                        .child(
                            chip("replay-export", "Export replay GIF", false, p).on_click(
                                cx.listener(|this, _, _, cx| this.export_replay_gif(cx)),
                            ),
                        )
                        .child(
                            div()
                                .w_full()
                                .text_size(px(10.5))
                                .text_color(p.muted)
                                .child("Replay shows saved versions and the editing steps still available in Undo."),
                        ),
                )
                .into_any_element(),
            SidebarTab::History => div()
                .id("sidebar-history-content")
                .child(self.compact_history(p, cx))
                .test_support()
                .into_any_element(),
            SidebarTab::Histogram => div()
                .p(px(15.))
                .child(self.histogram_view(p, cx))
                .into_any_element(),
        };
        // Photoshop's Color | Swatches group heads the panel dock, unless the
        // Colors toolbar already shows them.
        let swatches = (compact && !self.compact.bars[super::compact::Bar::Color as usize].open)
            .then(|| {
                div()
                    .id("sidebar-swatches")
                    .test_support()
                    .flex()
                    .flex_col()
                    .flex_none()
                    .gap_1()
                    .p_2()
                    .border_t_1()
                    .border_color(p.line)
                    .child(label("Swatches", p))
                    .child(self.project_colors(false, p, cx))
            });
        let dock_tabs = div()
            .flex()
            .flex_none()
            .h(if compact { rems(1.625) } else { rems(2.125) })
            .border_b_1()
            .border_color(p.line)
            .children(
                [
                    (DockTab::Layers, "dock-layers", "Layers"),
                    (DockTab::Channels, "dock-channels", "Channels"),
                    (DockTab::Paths, "dock-paths", "Paths"),
                ]
                .into_iter()
                .map(|(tab, id, title)| {
                    // Photoshop's panel-group tabs: a label with an accent
                    // underline, sized to its text.
                    let active = self.dock_tab == tab;
                    div()
                        .id(id)
                        .role(gpui_kit::Role::Tab)
                        .aria_label(title)
                        .aria_selected(active)
                        .focusable()
                        .tab_index(0)
                        .flex()
                        .items_center()
                        .h_full()
                        .px_3()
                        .text_size(px(11.))
                        .text_color(if active { p.ink } else { p.muted })
                        .border_b_2()
                        .border_color(if active {
                            p.accent
                        } else {
                            transparent_black()
                        })
                        .cursor_pointer()
                        .hover(|s| s.text_color(p.ink))
                        .child(title)
                        .on_click(cx.listener(move |this, _, window, cx| {
                            this.show_dock_tab(tab, window, cx);
                        }))
                        .test_support()
                }),
            );
        let dock_content = match self.dock_tab {
            DockTab::Layers => self.scene_graph(p, window, cx).into_any_element(),
            DockTab::Channels => self.channels_panel(p, cx),
            DockTab::Paths => self.paths_panel(p, cx),
        };
        div()
            .id("node-panel")
            .relative()
            .flex()
            .flex_none()
            .flex_col()
            .w(dim::NODE_PANEL_W)
            .when(compact, |d| d.w(px(sidebar_width)))
            .min_h_0()
            .border_l_1()
            .border_color(p.line)
            .track_focus(&self.panel_focus)
            .key_context("NodePanel")
            .on_key_down(cx.listener(|this, e: &KeyDownEvent, _, cx| {
                if e.keystroke.key == "escape" && this.selected.is_some() {
                    this.deselect_layer(cx);
                    cx.stop_propagation();
                }
            }))
            .children(swatches)
            .child(tabs)
            .child(
                div()
                    .id(("sidebar-content", self.sidebar_tab as usize))
                    .flex_1()
                    .min_h_0()
                    .overflow_y_scroll()
                    .child(content)
                    .test_support(),
            )
            .map(|d| {
                let line = p.line;
                let accent = p.accent;
                d.child(crate::widgets::tip(
                    div()
                        .id("layers-resize")
                        .test_support()
                        .flex_none()
                        .h(px(7.))
                        .w_full()
                        .flex()
                        .items_center()
                        .justify_center()
                        .cursor(CursorStyle::ResizeUpDown)
                        .hover(move |s| s.bg(accent.opacity(0.25)))
                        .on_mouse_down(
                            MouseButton::Left,
                            cx.listener(|this, e: &MouseDownEvent, _, cx| {
                                this.drag = Some(Drag::LayersSplit {
                                    start_y: e.position.y,
                                    start_h: this
                                        .layer_panel
                                        .dock_bounds
                                        .get()
                                        .map(|bounds| f32::from(bounds.size.height))
                                        .unwrap_or(this.layers_h),
                                });
                                cx.stop_propagation();
                                cx.notify();
                            }),
                        )
                        .child(div().w(px(36.)).h(px(2.)).bg(line)),
                    "Drag to give the Layers list more or less room",
                ))
            })
            .child(
                div()
                    .id("sidebar-layers-dock")
                    .relative()
                    .flex()
                    .flex_col()
                    .flex_none()
                    .h(px(
                        if self.layer_panel.compact && !self.layer_panel.controls_open {
                            self.layer_panel.compact_height.unwrap_or(if compact {
                                240.
                            } else {
                                260.
                            })
                        } else {
                            self.layers_h
                        },
                    ))
                    .max_h(relative(
                        if self.layer_panel.compact && !self.layer_panel.controls_open {
                            0.60
                        } else {
                            0.80
                        },
                    ))
                    .min_h_0()
                    .child(
                        canvas(
                            move |bounds, _, _| dock_bounds.set(Some(bounds)),
                            |_, _, _, _| {},
                        )
                        .absolute()
                        .size_full(),
                    )
                    .child(dock_tabs)
                    .child(dock_content)
                    .test_support(),
            )
            .when(compact, |d| {
                let accent = p.accent;
                d.child(crate::widgets::tip(
                    div()
                        .id("sidebar-width-resize")
                        .absolute()
                        .left_0()
                        .top_0()
                        .bottom_0()
                        .w(rems(0.25))
                        .cursor(CursorStyle::ResizeLeftRight)
                        .hover(move |s| s.bg(accent.opacity(0.25)))
                        .on_mouse_down(
                            MouseButton::Left,
                            cx.listener(move |this, e: &MouseDownEvent, _, cx| {
                                if e.click_count == 2 {
                                    this.sidebar_layout.width =
                                        Some(SidebarState::snapped_width(sidebar_width, rem_size));
                                    this.drag = None;
                                } else {
                                    this.drag = Some(Drag::SidebarResize {
                                        start_x: e.position.x,
                                        start_w: sidebar_width,
                                    });
                                }
                                cx.stop_propagation();
                                cx.notify();
                            }),
                        )
                        .test_support(),
                    "Drag to resize sidebar; double-click to snap between narrow and wide",
                ))
            })
            .test_support()
            .into_any_element()
    }

    fn paths_panel(&self, p: &Palette, cx: &mut Context<Self>) -> AnyElement {
        let paths: Vec<_> = self
            .editor
            .doc
            .nodes
            .iter()
            .rev()
            .filter(|node| matches!(node.kind, NodeKind::Path { .. }))
            .map(|node| (node.id, node.name.clone()))
            .collect();
        div()
            .id("sidebar-paths-content")
            .flex()
            .flex_col()
            .flex_1()
            .min_h_0()
            .p_2()
            .gap_1()
            .overflow_y_scroll()
            .when(paths.is_empty(), |d| {
                d.child(mono(
                    "No paths. Draw with the Pen tool to create one.",
                    11.,
                    p.muted,
                ))
            })
            .children(paths.into_iter().map(|(id, name)| {
                chip(("path-row", id), name, self.selected == Some(id), p)
                    .aria_selected(self.selected == Some(id))
                    .on_click(cx.listener(move |this, _, window, cx| {
                        this.set_layer_selection(vec![id], Some(id));
                        this.set_pen_mode(PenMode::Pen, cx);
                        window.focus(&this.canvas_focus, cx);
                        cx.notify();
                    }))
                    .test_support()
            }))
            .test_support()
            .into_any_element()
    }

    pub(super) fn sidebar_panel_menu(
        &self,
        p: &Palette,
        cx: &mut Context<Self>,
    ) -> Option<AnyElement> {
        self.sidebar_menu.then(|| {
            div()
                .id("sidebar-panel-menu")
                .flex()
                .flex_wrap()
                .gap(px(6.))
                .py(px(8.))
                .children(
                    [
                        (SidebarTab::Navigator, "sidebar-navigator", "Navigator"),
                        (SidebarTab::Info, "sidebar-info", "Info"),
                        (SidebarTab::Recipes, "sidebar-recipes", "Recipes"),
                        (SidebarTab::Timeline, "sidebar-timeline", "Timeline"),
                        (SidebarTab::History, "sidebar-history", "History"),
                        (SidebarTab::Histogram, "sidebar-histogram", "Histogram"),
                    ]
                    .into_iter()
                    .map(|(tab, id, title)| {
                        chip(id, title, self.sidebar_tab == tab, p)
                            .on_click(cx.listener(move |this, _, _, cx| {
                                match tab {
                                    SidebarTab::Navigator => this.panels.navigator = true,
                                    SidebarTab::Info => this.panels.info = true,
                                    SidebarTab::Recipes if !this.recipes.open => {
                                        this.toggle_recipes(cx)
                                    }
                                    SidebarTab::Timeline if !this.anim.open => {
                                        this.toggle_animation(cx)
                                    }
                                    _ => {}
                                }
                                this.select_sidebar(tab, cx);
                            }))
                            .test_support()
                    }),
                )
                .test_support()
                .into_any_element()
        })
    }
}
