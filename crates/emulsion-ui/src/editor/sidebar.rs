//! Layers occupy the main right dock; task controls stay in a compact lower panel.
use super::*;

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
        // Selecting a task panel or an adjustment is an explicit request to
        // edit its controls. Give that task room without hiding the layers.
        let expanded = self.sidebar_tab != SidebarTab::Properties
            || self
                .selected
                .and_then(|id| self.editor.doc.node(id))
                .is_some_and(|node| {
                    matches!(node.kind, NodeKind::Adjust(_) | NodeKind::Smart { .. })
                });
        let tabs = div()
            .flex()
            .flex_none()
            .h(px(34.))
            .border_t_1()
            .border_b_1()
            .border_color(p.line)
            .children(
                [
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
                                .child("Replay plays the picture back from its history: every step still undoable, and every save."),
                        ),
                )
                .into_any_element(),
            SidebarTab::History => div()
                .id("sidebar-history-content")
                .child(self.history_list(p, cx))
                .test_support()
                .into_any_element(),
            SidebarTab::Histogram => div()
                .p(px(15.))
                .child(self.histogram_view(p, cx))
                .into_any_element(),
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
            .child(
                div()
                    .id("sidebar-layers-dock")
                    .min_h_0()
                    .when(expanded && !self.draw_mode, |d| {
                        d.flex_none().h(relative(0.25)).max_h(px(160.))
                    })
                    .when(!expanded || self.draw_mode, |d| d.flex_1())
                    .child(self.scene_graph(p, cx))
                    .test_support(),
            )
            .when(!self.draw_mode, |d| {
                d.child(tabs).child(
                    div()
                        .id(("sidebar-content", self.sidebar_tab as usize))
                        // Keep Layers dominant on tall windows while making
                        // the inspector proportional on shorter displays.
                        .when(expanded, |d| d.flex_1())
                        .when(!expanded, |d| {
                            d.flex_none().h(relative(0.38)).max_h(px(260.))
                        })
                        .min_h_0()
                        .overflow_y_scroll()
                        .child(content)
                        .test_support(),
                )
            })
            .test_support()
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
