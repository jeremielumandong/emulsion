//! A fixed Layers dock and a separately scrolling panel for the current task.
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
        let tabs = div()
            .flex()
            .flex_none()
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
                        .py(px(10.))
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
                            chip(
                                "timelapse",
                                if self.anim.record {
                                    "Stop recording"
                                } else {
                                    "Record time-lapse"
                                },
                                self.anim.record,
                                p,
                            )
                            .on_click(cx.listener(|this, _, _, cx| this.toggle_timelapse(cx))),
                        )
                        .when(self.anim.captured > 0, |d| {
                            d.child(
                                chip("timelapse-gif", "Export time-lapse GIF", false, p).on_click(
                                    cx.listener(|this, _, _, cx| this.export_timelapse_gif(cx)),
                                ),
                            )
                        }),
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
            .child(div().flex_none().child(self.scene_graph(p, cx)))
            .when(!self.draw_mode, |d| {
                d.child(tabs).child(
                    div()
                        .id(("sidebar-content", self.sidebar_tab as usize))
                        .flex_1()
                        .min_h_0()
                        .overflow_y_scroll()
                        .child(content),
                )
            })
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
