//! History and task controls above the Layers, Channels and Paths dock.
use super::*;
use gpui_kit::component::{
    Sizable,
    button::{Button, ButtonVariants},
    menu::{DropdownMenu, PopupMenuItem},
};

pub(crate) struct SidebarState {
    pub width: Option<f32>,
    pub collapsed: bool,
    pub overlay_open: bool,
    pub flyout_open: bool,
    pub flyout_tab: SidebarTab,
    pub photo: super::photo_panels::PhotoPanelState,
    pub upper_collapsed: bool,
    pub layers_collapsed: bool,
    pub colors_collapsed: bool,
    pub color_tab: bool,
    pub colors_height: f32,
}
impl Default for SidebarState {
    fn default() -> Self {
        Self {
            width: None,
            collapsed: false,
            overlay_open: false,
            flyout_open: false,
            flyout_tab: SidebarTab::Properties,
            photo: Default::default(),
            upper_collapsed: false,
            layers_collapsed: false,
            colors_collapsed: false,
            color_tab: false,
            colors_height: 64.,
        }
    }
}

impl SidebarState {
    pub(super) fn width_for_viewport(&self, viewport_width: f32, rem_size: f32) -> Option<f32> {
        let available = (viewport_width - 280.).max(0.);
        let minimum = 13.75 * rem_size;
        if self.collapsed || available < minimum {
            return None;
        }
        Some(
            self.width
                .unwrap_or(18.75 * rem_size)
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
            ..Default::default()
        };
        assert_eq!(state.width_for_viewport(600., 16.), Some(320.));
        assert_eq!(state.width_for_viewport(499., 16.), None);
        assert_eq!(state.width_for_viewport(1200., 16.), Some(560.));
    }

    #[test]
    fn sidebar_geometry_and_snap_follow_interface_zoom() {
        let state = SidebarState::default();
        assert_eq!(state.width_for_viewport(1200., 20.), Some(375.));
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
    Assistant,
    Character,
    Develop,
}

impl SidebarTab {
    pub(super) fn key(self) -> &'static str {
        match self {
            Self::Properties => "properties",
            Self::Adjustments => "adjustments",
            Self::Reference => "reference",
            Self::Navigator => "navigator",
            Self::Info => "info",
            Self::Recipes => "recipes",
            Self::Timeline => "timeline",
            Self::History => "history",
            Self::Histogram => "histogram",
            Self::BrushSettings => "brush-settings",
            Self::BrushPresets => "brush-presets",
            Self::BlendingOptions => "blending-options",
            Self::Assistant => "assistant",
            Self::Character => "character",
            Self::Develop => "develop",
        }
    }
    pub(super) fn from_key(key: &str) -> Self {
        [
            Self::Properties,
            Self::Adjustments,
            Self::Reference,
            Self::Navigator,
            Self::Info,
            Self::Recipes,
            Self::Timeline,
            Self::History,
            Self::Histogram,
            Self::BrushSettings,
            Self::BrushPresets,
            Self::BlendingOptions,
            Self::Assistant,
            Self::Character,
            Self::Develop,
        ]
        .into_iter()
        .find(|t| t.key() == key)
        .unwrap_or(Self::Properties)
    }
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
        self.sidebar_layout.overlay_open = true;
        self.dock_tab = tab;
        self.sidebar_layout.layers_collapsed = false;
        self.menu = None;
        window.focus(&self.panel_focus, cx);
        cx.notify();
    }

    /// Photoshop's Tab: hide or show the panel dock.
    pub(crate) fn toggle_panel_dock(&mut self, window: &Window, cx: &mut Context<Self>) {
        let show = !self.sidebar_content_visible(window, cx);
        self.sidebar_layout.collapsed = !show;
        self.sidebar_layout.overlay_open = show;
        cx.notify();
    }

    /// Photoshop's F7: the Layers panel.
    pub(crate) fn show_layers_panel(&mut self, window: &mut Window, cx: &mut Context<Self>) {
        self.show_dock_tab(DockTab::Layers, window, cx)
    }

    /// Ctrl+F: the Layers panel with its search field focused.
    pub(crate) fn find_layers(&mut self, window: &mut Window, cx: &mut Context<Self>) {
        self.show_layers_panel(window, cx);
        self.layer_panel.controls_open = true;
        self.ensure_layer_search(window, cx);
        if let Some((state, _)) = &self.layer_panel.search {
            state.update(cx, |state, cx| state.focus(window, cx));
        }
        cx.notify();
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
        let tab = if self.draw_mode && tab == SidebarTab::BrushPresets { SidebarTab::BrushSettings } else { tab };
        self.sidebar_layout.flyout_open = false;
        self.draw_ui.gallery_open = false;
        if self.is_design() {
            self.design_ui.inspector = true;
        }
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
        self.sidebar_layout.upper_collapsed = false;
        self.sidebar_layout.overlay_open = true;
        if tab == SidebarTab::Assistant {
            self.assistant.dock_open = true;
        }
        self.sidebar_layout.collapsed = false;
        if tab == SidebarTab::Info {
            self.panels.info = true;
        }
        if tab == SidebarTab::Navigator {
            self.panels.navigator = true;
        }
        self.presets.open = tab == SidebarTab::BrushPresets;
        self.sidebar_menu = false;
        self.menu = None;
        cx.notify();
    }

    pub(super) fn sidebar_content(
        &mut self,
        p: &Palette,
        window: &mut Window,
        cx: &mut Context<Self>,
    ) -> AnyElement {
        self.sidebar_content_for(self.sidebar_tab, p, window, cx)
    }

    pub(super) fn sidebar_content_for(
        &mut self, tab: SidebarTab, p: &Palette, window: &mut Window, cx: &mut Context<Self>,
    ) -> AnyElement {
        if self.shared_panel_mode() {
            match tab {
                SidebarTab::Properties => return self.photo_properties(p, window, cx),
                SidebarTab::BrushSettings => return self.photo_brushes(p, window, cx),
                SidebarTab::Assistant => return self.photo_assistant(p, window, cx),
                SidebarTab::History => return self.photo_history(p,cx),
                _ => {}
            }
        }
        match tab {
            SidebarTab::Develop => {
                if let Some(id) = self.editor.doc.raw.as_ref().map(|raw|raw.node_id) {
                    div().p_3().children(self.raw_panel(id,p,cx)).into_any_element()
                } else { self.quick_adjust_view(p,cx).into_any_element() }
            },
            SidebarTab::Character => {
                if let Some(properties) = self.text_properties(window, cx) {
                    div().id("sidebar-character-content").test_support().child(properties).into_any_element()
                } else {
                    div().id("sidebar-character-content").test_support().p_3().text_size(px(11.))
                        .child("Select a text layer to edit its character and paragraph settings.")
                        .child(Button::new("sidebar-character-tool").label("Add text").small().outline()
                            .on_click(cx.listener(|this, _, _, cx| this.set_tool(Tool::Type, cx))))
                        .into_any_element()
                }
            }
            SidebarTab::Assistant => div().id("sidebar-assistant-content").test_support().flex().flex_col().gap_2().p_2()
                .child(Button::new("sidebar-assistant-prompt").label("Ask about this document…").small().outline().on_click(cx.listener(|this,_,window,cx|this.open_ask(window,cx))))
                .children(self.assistant_dock(p,cx))
                .into_any_element(),
            SidebarTab::BlendingOptions => self.blending_options_panel(p, cx),
            SidebarTab::BrushSettings => self.brush_settings_panel(p, cx),
            SidebarTab::BrushPresets => div().children(self.presets_view(p, cx)).into_any_element(),
            SidebarTab::Properties if self.is_diagram() => div()
                .id("sidebar-properties-content").test_support()
                .child(self.diagram_inspector(p, window, cx)).into_any_element(),
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
        }
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
        if visible_width.is_some() {
            self.sidebar_layout.overlay_open = false;
        }
        let popup = visible_width.is_none()
            && !self.sidebar_layout.collapsed
            && self.sidebar_layout.overlay_open;
        let sidebar_width = visible_width.unwrap_or(
            (18.75 * rem_size)
                .min(f32::from(window.viewport_size().width) - 40.)
                .max(160.),
        );
        self.layer_panel.compact = compact || f32::from(window.viewport_size().height) < 700.;
        if visible_width.is_none() && !popup {
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
                            this.sidebar_layout.overlay_open = true;
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
        let owner = cx.weak_entity();
        let tabs = div()
            .id("sidebar-primary-tabs")
            .test_support()
            .flex()
            .flex_none()
            .h(rems(2.))
            .items_center()
            .border_b_1()
            .border_color(p.line)
            .when(self.shared_panel_mode() && self.editor.doc.raw.is_some(), |tabs| tabs.child(
                Button::new("sidebar-develop").label("Develop").xsmall().ghost()
                    .when(self.sidebar_tab == SidebarTab::Develop, |b| b.bg(p.soft_bg).text_color(p.accent))
                    .on_click(cx.listener(|this, _, _, cx| this.select_sidebar(SidebarTab::Develop, cx)))
            ))
            .children(
                [
                    (SidebarTab::Properties, "sidebar-properties", "Properties"),
                    (SidebarTab::Adjustments, "sidebar-adjustments", "Adjust"),
                    (SidebarTab::History, "sidebar-history-top", "History"),
                    (SidebarTab::Assistant, "sidebar-assistant", "Assistant"),
                ]
                .into_iter()
                .map(|(tab, id, title)| {
                    Button::new(id)
                        .label(title)
                        .xsmall()
                        .ghost()
                        .flex_1()
                        .min_w_0()
                        .when(self.sidebar_tab == tab, |b| {
                            b.bg(p.soft_bg).text_color(p.accent)
                        })
                        .on_click(cx.listener(move |this, _, _, cx| this.select_sidebar(tab, cx)))
                }),
            )
            .child(
                Button::new("sidebar-more")
                    .label("⋯")
                    .accessibility_label("More panels")
                    .tooltip("All panels")
                    .xsmall()
                    .ghost()
                    .dropdown_menu(move |mut menu, _, _| {
                        for (tab, title) in [
                            (SidebarTab::Character, "Character"),
                            (SidebarTab::Info, "Info"),
                            (SidebarTab::Reference, "Reference"),
                            (SidebarTab::Navigator, "Navigator"),
                            (SidebarTab::Histogram, "Histogram"),
                            (SidebarTab::BrushSettings, "Brush settings"),
                            (SidebarTab::BrushPresets, "Brush presets"),
                            (SidebarTab::Recipes, "Recipes"),
                            (SidebarTab::Timeline, "Timeline"),
                            (SidebarTab::BlendingOptions, "Blending options"),
                        ] {
                            let owner = owner.clone();
                            menu =
                                menu.item(PopupMenuItem::new(title).on_click(move |_, _, cx| {
                                    owner
                                        .update(cx, |this, cx| this.select_sidebar(tab, cx))
                                        .ok();
                                }));
                        }
                        menu
                    }),
            )
            .child(
                Button::new("sidebar-section-toggle")
                    .label(if self.sidebar_layout.upper_collapsed {
                        "⌄"
                    } else {
                        "⌃"
                    })
                    .accessibility_label("Toggle properties section")
                    .xsmall()
                    .ghost()
                    .on_click(cx.listener(|this, _, _, cx| {
                        this.sidebar_layout.upper_collapsed = !this.sidebar_layout.upper_collapsed;
                        cx.notify();
                    })),
            )
            .child(
                Button::new("sidebar-collapse")
                    .label("›")
                    .accessibility_label("Collapse sidebar")
                    .tooltip("Collapse sidebar")
                    .xsmall()
                    .ghost()
                    .on_click(cx.listener(|this, _, _, cx| {
                        this.sidebar_layout.collapsed = true;
                        this.sidebar_layout.overlay_open = false;
                        cx.notify();
                    })),
            );
        let content = if self.sidebar_layout.flyout_open && !self.shared_panel_mode() {
            div().p_3().child("Panel open beside canvas").into_any_element()
        } else { self.sidebar_content(p, window, cx) };
        let swatches = (compact && !self.compact.bars[super::compact::Bar::Color as usize].open)
            .then(|| {
                let color_content = if self.sidebar_layout.color_tab {
                    div()
                        .id("sidebar-color-content")
                        .test_support()
                        .flex()
                        .flex_wrap()
                        .items_center()
                        .gap_2()
                        .p_2()
                        .child(self.swatches(p, cx))
                        .child(mono(
                            format!(
                                "#{:02X}{:02X}{:02X}",
                                self.tools.fg[0], self.tools.fg[1], self.tools.fg[2]
                            ),
                            11.,
                            p.ink,
                        ))
                        .child(
                            Button::new("sidebar-edit-color")
                                .label("Edit foreground…")
                                .xsmall()
                                .ghost()
                                .on_click(cx.listener(|this, _, _, cx| {
                                    this.tools.picker = true;
                                    cx.notify();
                                })),
                        )
                        .into_any_element()
                } else {
                    div()
                        .id("sidebar-swatches")
                        .test_support()
                        .p_2()
                        .child(self.project_colors(false, p, cx))
                        .into_any_element()
                };
                div()
                    .id("sidebar-color-group")
                    .test_support()
                    .flex()
                    .flex_col()
                    .flex_none()
                    .border_t_1()
                    .border_color(p.line)
                    .child(
                        div()
                            .flex()
                            .items_center()
                            .h(rems(2.))
                            .flex_none()
                            .children(
                                [
                                    (false, "sidebar-swatches-tab", "Swatches"),
                                    (true, "sidebar-color-tab", "Color"),
                                ]
                                .map(|(tab, id, label)| {
                                    Button::new(id)
                                        .label(label)
                                        .xsmall()
                                        .ghost()
                                        .when(self.sidebar_layout.color_tab == tab, |b| {
                                            b.text_color(p.accent)
                                        })
                                        .on_click(cx.listener(move |this, _, _, cx| {
                                            this.sidebar_layout.color_tab = tab;
                                            this.sidebar_layout.colors_collapsed = false;
                                            cx.notify();
                                        }))
                                }),
                            )
                            .child(div().flex_1())
                            .child(
                                Button::new("sidebar-color-toggle")
                                    .label(if self.sidebar_layout.colors_collapsed {
                                        "⌄"
                                    } else {
                                        "⌃"
                                    })
                                    .accessibility_label("Toggle color section")
                                    .xsmall()
                                    .ghost()
                                    .on_click(cx.listener(|this, _, _, cx| {
                                        this.sidebar_layout.colors_collapsed =
                                            !this.sidebar_layout.colors_collapsed;
                                        cx.notify();
                                    })),
                            ),
                    )
                    .when(!self.sidebar_layout.colors_collapsed, |d| {
                        d.child(
                            div()
                                .id("sidebar-color-scroll")
                                .h(px(self
                                    .sidebar_layout
                                    .colors_height
                                    .min(f32::from(window.viewport_size().height) * 0.18)))
                                .overflow_y_scroll()
                                .child(color_content),
                        )
                    })
                    .child(
                        Button::new("sidebar-color-resize")
                            .label("─")
                            .accessibility_label("Resize color section; use Up or Down")
                            .xsmall()
                            .ghost()
                            .h(px(7.))
                            .w_full()
                            .cursor(CursorStyle::ResizeUpDown)
                            .on_mouse_down(
                                MouseButton::Left,
                                cx.listener(|this, e: &MouseDownEvent, _, cx| {
                                    this.drag = Some(Drag::ColorSplit {
                                        start_y: e.position.y,
                                        start_h: this.sidebar_layout.colors_height,
                                    });
                                    cx.stop_propagation();
                                    cx.notify();
                                }),
                            )
                            .on_key_down(cx.listener(|this, e: &KeyDownEvent, _, cx| {
                                let delta = match e.keystroke.key.as_str() {
                                    "up" => -16.,
                                    "down" => 16.,
                                    _ => return,
                                };
                                this.sidebar_layout.colors_height =
                                    (this.sidebar_layout.colors_height + delta).clamp(48., 240.);
                                cx.stop_propagation();
                                cx.notify();
                            })),
                    )
            });
        let dock_tabs = div()
            .flex()
            .flex_none()
            .h(rems(2.))
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
                    let active = self.dock_tab == tab;
                    Button::new(id)
                        .label(title)
                        .accessibility_label(title)
                        .small()
                        .ghost()
                        .h_full()
                        .when(active, |b| b.text_color(p.accent).bg(p.soft_bg))
                        .on_click(cx.listener(move |this, _, window, cx| {
                            this.show_dock_tab(tab, window, cx)
                        }))
                }),
            );
        let dock_tabs = dock_tabs.child(div().flex_1()).child(
            Button::new("sidebar-layers-toggle")
                .label(if self.sidebar_layout.layers_collapsed {
                    "⌄"
                } else {
                    "⌃"
                })
                .accessibility_label("Toggle layers section")
                .xsmall()
                .ghost()
                .on_click(cx.listener(|this, _, _, cx| {
                    this.sidebar_layout.layers_collapsed = !this.sidebar_layout.layers_collapsed;
                    cx.notify();
                })),
        );
        let dock_content = match self.dock_tab {
            DockTab::Layers => self.scene_graph(p, window, cx).into_any_element(),
            DockTab::Channels => self.channels_panel(p, cx),
            DockTab::Paths => self.paths_panel(p, cx),
        };
        let panel = div()
            .id("node-panel")
            .relative()
            .flex()
            .flex_none()
            .flex_col()
            .w(px(sidebar_width))
            .bg(p.panel)
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
                    .flex()
                    .items_center()
                    .flex_none()
                    .h(rems(1.5))
                    .child(
                        Button::new("sidebar-info-top")
                            .label("Info")
                            .xsmall()
                            .ghost()
                            .on_click(cx.listener(|this, _, _, cx| {
                                this.select_sidebar(SidebarTab::Info, cx)
                            })),
                    )
                    .child(
                        Button::new("sidebar-reference")
                            .label("Reference")
                            .xsmall()
                            .ghost()
                            .on_click(cx.listener(|this, _, _, cx| {
                                this.select_sidebar(SidebarTab::Reference, cx)
                            })),
                    )
                    .when(self.draw_mode, |d| {
                        d.child(
                            Button::new("sidebar-brush-settings")
                                .label("Brush")
                                .xsmall()
                                .ghost()
                                .on_click(cx.listener(|this, _, _, cx| {
                                    this.select_sidebar(SidebarTab::BrushSettings, cx)
                                })),
                        )
                    }),
            )
            .when(!self.sidebar_layout.upper_collapsed, |d| {
                d.child(
                    div()
                        .id(("sidebar-content", self.sidebar_tab as usize))
                        .flex_1()
                        .min_h_0()
                        .overflow_y_scroll()
                        .child(content)
                        .test_support(),
                )
            })
            .children(swatches)
            .map(|d| {
                let line = p.line;
                let accent = p.accent;
                d.child(crate::widgets::tip(
                    div()
                        .id("layers-resize")
                        .focusable()
                        .tab_index(0)
                        .aria_label("Resize layers section; use Up or Down")
                        .on_key_down(cx.listener(|this, e: &KeyDownEvent, _, cx| {
                            let delta = match e.keystroke.key.as_str() {
                                "up" => 16.,
                                "down" => -16.,
                                _ => return,
                            };
                            this.sidebar_layout.layers_collapsed = false;
                            if this.layer_panel.compact && !this.layer_panel.controls_open {
                                let height = this
                                    .layer_panel
                                    .dock_bounds
                                    .get()
                                    .map(|b| f32::from(b.size.height))
                                    .unwrap_or(240.);
                                this.layer_panel.compact_height =
                                    Some((height + delta).clamp(LAYERS_MIN_H, LAYERS_MAX_H));
                            } else {
                                this.layers_h =
                                    (this.layers_h + delta).clamp(LAYERS_MIN_H, LAYERS_MAX_H);
                            }
                            cx.stop_propagation();
                            cx.notify();
                        }))
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
                                this.sidebar_layout.layers_collapsed = false;
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
                    .h(px(if self.sidebar_layout.layers_collapsed {
                        2. * rem_size
                    } else if self.layer_panel.compact && !self.layer_panel.controls_open {
                        self.layer_panel
                            .compact_height
                            .unwrap_or(if compact { 240. } else { 236. })
                    } else {
                        self.layers_h
                    }))
                    .max_h(relative(if compact {
                        0.55
                    } else if self.layer_panel.compact && !self.layer_panel.controls_open {
                        0.50
                    } else {
                        0.80
                    }))
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
                    .when(!self.sidebar_layout.layers_collapsed, |d| {
                        d.child(dock_content)
                    })
                    .test_support(),
            )
            .map(|d| {
                let accent = p.accent;
                d.child(crate::widgets::tip(
                    div()
                        .id("sidebar-width-resize")
                        .focusable()
                        .tab_index(0)
                        .aria_label("Resize sidebar; use Left or Right")
                        .on_key_down(cx.listener(move |this, e: &KeyDownEvent, _, cx| {
                            let delta = match e.keystroke.key.as_str() {
                                "left" => 16.,
                                "right" => -16.,
                                _ => return,
                            };
                            this.sidebar_layout.width = Some(
                                (sidebar_width + delta).clamp(13.75 * rem_size, 35. * rem_size),
                            );
                            cx.stop_propagation();
                            cx.notify();
                        }))
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
            .test_support();
        if popup {
            deferred(
                anchored()
                    .position(point(
                        window.viewport_size().width - px(sidebar_width) - px(8.),
                        px(76.),
                    ))
                    .snap_to_window()
                    .child(
                        panel
                            .h((window.viewport_size().height - px(108.)).max(px(120.)))
                            .occlude()
                            .shadow_lg()
                            .on_mouse_down_out(cx.listener(|this, _, _, cx| {
                                this.sidebar_layout.overlay_open = false;
                                cx.notify();
                            })),
                    ),
            )
            .into_any_element()
        } else {
            panel.into_any_element()
        }
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
