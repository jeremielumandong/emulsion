//! History and task controls above the Layers, Channels and Paths dock.
use super::*;

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
    /// Resolve temporary previews before hiding their Apply/Cancel controls.
    pub(crate) fn select_sidebar(&mut self, tab: SidebarTab, cx: &mut Context<Self>) {
        if tab == SidebarTab::Recipes {
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
    ) -> impl IntoElement + use<> {
        let tabs = div()
            .flex()
            .flex_none()
            .h(px(34.))
            .border_t_1()
            .border_b_1()
            .border_color(p.line)
            .children(
                [
                    (SidebarTab::History, "sidebar-history-top", "History"),
                    (SidebarTab::Properties, "sidebar-properties", "Properties"),
                    (
                        SidebarTab::Adjustments,
                        "sidebar-adjustments",
                        "Adjustments",
                    ),
                    (SidebarTab::Reference, "sidebar-reference", "Reference"),
                ]
                .into_iter()
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
                        .child(title)
                        .on_click(cx.listener(move |this, _, _, cx| {
                            this.select_sidebar(tab, cx);
                        }))
                        .test_support()
                }),
            );
        let content = match self.sidebar_tab {
            SidebarTab::BlendingOptions => self.blending_options_panel(p, cx),
            SidebarTab::BrushSettings => self.brush_settings_panel(p, cx),
            SidebarTab::BrushPresets => div().children(self.presets_view(p, cx)).into_any_element(),
            SidebarTab::Properties => div()
                .id("sidebar-properties-content")
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
        let dock_tabs = div()
            .flex()
            .flex_none()
            .gap_1()
            .p_1()
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
                    chip(id, title, self.dock_tab == tab, p)
                        .aria_selected(self.dock_tab == tab)
                        .flex_1()
                        .on_click(cx.listener(move |this, _, window, cx| {
                            this.dock_tab = tab;
                            this.menu = None;
                            window.focus(&this.panel_focus, cx);
                            cx.notify();
                        }))
                        .test_support()
                }),
            );
        let dock_content = match self.dock_tab {
            DockTab::Layers => self.scene_graph(p, cx).into_any_element(),
            DockTab::Channels => self.channels_panel(p, cx),
            DockTab::Paths => self.paths_panel(p, cx),
        };
        div()
            .id("node-panel")
            .flex()
            .flex_none()
            .flex_col()
            .w(dim::NODE_PANEL_W)
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
                                    start_h: this.layers_h,
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
                    .flex()
                    .flex_col()
                    .flex_none()
                    .h(px(self.layers_h))
                    .min_h_0()
                    .child(dock_tabs)
                    .child(dock_content)
                    .test_support(),
            )
            .test_support()
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
                        this.selected = Some(id);
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
