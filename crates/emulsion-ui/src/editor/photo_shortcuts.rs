//! Canvas-side shortcuts and a single live panel, matching the Photo handoff.
use super::*;
use gpui_kit::component::{
    Sizable,
    button::{Button, ButtonVariants},
};

impl EditorView {
    pub(super) fn open_shared_brush_panel(&mut self, cx: &mut Context<Self>) {
        self.draw_ui.gallery_open = false;
        if self.sidebar_layout.flyout_open
            && self.sidebar_layout.flyout_tab == SidebarTab::BrushSettings
        {
            return;
        }
        if !self.sidebar_layout.flyout_open && self.sidebar_tab == SidebarTab::BrushSettings {
            self.show_sidebar_tab(SidebarTab::BrushSettings, cx);
        } else {
            self.toggle_canvas_panel(SidebarTab::BrushSettings, cx);
        }
    }

    fn toggle_canvas_panel(&mut self, tab: SidebarTab, cx: &mut Context<Self>) {
        self.draw_ui.gallery_open = false;
        let open = !(self.sidebar_layout.flyout_open && self.sidebar_layout.flyout_tab == tab);
        if self.shared_panel_mode() {
            // Keep each input/slider mounted once, while the dock stays useful.
            if open
                && (self.sidebar_tab == tab
                    || (matches!(tab, SidebarTab::Properties | SidebarTab::Character)
                        && matches!(
                            self.sidebar_tab,
                            SidebarTab::Properties | SidebarTab::Character
                        )
                        && self.text_target().is_some()))
            {
                let fallback = if self.editor.doc.raw.is_some() {
                    SidebarTab::Develop
                } else if self.draw_mode
                    && !matches!(tab, SidebarTab::Properties | SidebarTab::Character)
                {
                    SidebarTab::Properties
                } else {
                    SidebarTab::Adjustments
                };
                let collapsed = self.sidebar_layout.collapsed;
                let overlay = self.sidebar_layout.overlay_open;
                self.select_sidebar(fallback, cx);
                self.sidebar_layout.collapsed = collapsed;
                self.sidebar_layout.overlay_open = overlay;
            }
            if tab == SidebarTab::BrushSettings {
                self.prepare_presets(cx);
            }
            if tab == SidebarTab::Assistant {
                self.assistant.dock_open = true;
            }
        } else {
            let collapsed = self.sidebar_layout.collapsed;
            let overlay = self.sidebar_layout.overlay_open;
            self.select_sidebar(tab, cx);
            self.sidebar_layout.collapsed = collapsed;
            self.sidebar_layout.overlay_open = overlay;
        }
        self.sidebar_layout.flyout_tab = tab;
        self.sidebar_layout.flyout_open = open;
        cx.notify();
    }

    pub(super) fn photo_shortcuts(
        &mut self,
        p: &Palette,
        window: &mut Window,
        cx: &mut Context<Self>,
    ) -> AnyElement {
        let paint_controls = self.draw_mode
            && crate::app_state::settings(cx).compact_chrome
            && self.compact.bars[super::compact::Bar::Dock as usize].open;
        let quick_controls = paint_controls.then(|| {
            div()
                .flex_none()
                .pt_2()
                .border_t_1()
                .border_color(p.line)
                .child(self.draw_dock(false, p, cx))
        });
        // (tab, catalog key for its title, icon)
        let tabs = [
            (
                SidebarTab::Properties,
                "window.properties",
                "sliders-horizontal",
            ),
            (
                SidebarTab::BrushSettings,
                "editor.photo_shortcuts.brushes",
                "brush",
            ),
            (SidebarTab::History, "window.history", "history"),
            (
                SidebarTab::Character,
                "editor.photo_shortcuts.character",
                "type",
            ),
            (
                SidebarTab::Assistant,
                "editor.photo_shortcuts.assistant",
                "sparkles",
            ),
        ];
        let mut overlay = div()
            .id("photo-shortcuts-overlay")
            .test_support()
            .absolute()
            .size_full();
        if self.sidebar_layout.flyout_open {
            let (title, icon) = tabs
                .iter()
                .find(|(tab, _, _)| *tab == self.sidebar_layout.flyout_tab)
                .map(|(_, title, icon)| (*title, *icon))
                .unwrap_or(("editor.photo_shortcuts.panel", "sliders-horizontal"));
            let content = self.sidebar_content_for(self.sidebar_layout.flyout_tab, p, window, cx);
            overlay = overlay.child(
                div()
                    .id("photo-shortcut-panel")
                    .test_support()
                    .absolute()
                    .top(px(18.))
                    .bottom(px(12.))
                    .right(px(if paint_controls { 68. } else { 52. }))
                    .w(rems(18.75))
                    .max_w(relative(if paint_controls { 0.70 } else { 0.78 }))
                    .flex()
                    .flex_col()
                    .bg(p.panel)
                    .border_1()
                    .border_color(p.line)
                    .rounded(px(crate::app_state::settings(cx).corners.radius() + 4.))
                    .shadow_lg()
                    .occlude()
                    .overflow_hidden()
                    .child(
                        div()
                            .h(rems(2.5))
                            .flex_none()
                            .flex()
                            .items_center()
                            .gap(px(8.))
                            .px(px(10.))
                            .border_b_1()
                            .border_color(p.line)
                            .child(rail::tool_icon(icon).text_color(p.ink).size(px(13.)))
                            .child(
                                div()
                                    .flex_1()
                                    .text_size(px(12.))
                                    .child(SharedString::from(t!(title))),
                            )
                            .when(
                                self.sidebar_layout.flyout_tab == SidebarTab::History,
                                |header| {
                                    header.child(mono(
                                        t!(
                                            "editor.photo_shortcuts.states",
                                            count = self.editor.history.len() + 1
                                        ),
                                        10.,
                                        p.muted,
                                    ))
                                },
                            )
                            .child(
                                Button::new("photo-shortcut-dock")
                                    .accessibility_label(t!("editor.photo_shortcuts.move_to_dock"))
                                    .tooltip(t!("editor.photo_shortcuts.move_to_dock"))
                                    .ghost()
                                    .xsmall()
                                    .size(px(26.))
                                    .child(
                                        rail::tool_icon("panel-right")
                                            .text_color(p.ink)
                                            .size(px(13.)),
                                    )
                                    .on_click(cx.listener(|this, _, _, cx| {
                                        this.select_sidebar(this.sidebar_layout.flyout_tab, cx);
                                        this.sidebar_layout.collapsed = false;
                                        this.sidebar_layout.overlay_open = true;
                                        this.sidebar_layout.upper_collapsed = false;
                                        cx.notify();
                                    })),
                            )
                            .child(
                                Button::new("photo-shortcut-close")
                                    .accessibility_label(t!("editor.photo_shortcuts.close_panel"))
                                    .tooltip(t!("editor.photo_shortcuts.close_panel"))
                                    .ghost()
                                    .xsmall()
                                    .size(px(26.))
                                    .label("×")
                                    .on_click(cx.listener(|this, _, _, cx| {
                                        this.sidebar_layout.flyout_open = false;
                                        cx.notify();
                                    })),
                            ),
                    )
                    .child(
                        div()
                            .id("photo-shortcut-content")
                            .flex_1()
                            .min_h_0()
                            .overflow_y_scroll()
                            .child(content),
                    ),
            );
        }
        overlay = overlay.child(
            div()
                .id("photo-shortcut-strip")
                .test_support()
                .absolute()
                .top(px(28.))
                .right(px(10.))
                .when(paint_controls, |strip| {
                    strip
                        .bottom(px(12.))
                        .w(px(52.))
                        .items_center()
                        .overflow_y_scroll()
                })
                .flex()
                .flex_col()
                .gap(px(2.))
                .p(px(4.))
                .rounded(px(8.))
                .bg(p.panel)
                .border_1()
                .border_color(p.line)
                .shadow_md()
                .occlude()
                .child(
                    Button::new("photo-shortcut-toggle-dock")
                        .accessibility_label(t!("editor.photo_shortcuts.toggle_dock"))
                        .tooltip(t!("editor.photo_shortcuts.toggle_dock"))
                        .ghost()
                        .xsmall()
                        .w(px(30.))
                        .h(px(28.))
                        .flex_none()
                        .label(if self.sidebar_content_visible(window, cx) {
                            "»"
                        } else {
                            "«"
                        })
                        .on_click(
                            cx.listener(|this, _, window, cx| this.toggle_panel_dock(window, cx)),
                        ),
                )
                .child(div().w(px(22.)).h(px(1.)).my(px(2.)).bg(p.line))
                .children(tabs.into_iter().enumerate().map(|(i, (tab, title, icon))| {
                    let selected =
                        self.sidebar_layout.flyout_open && self.sidebar_layout.flyout_tab == tab;
                    Button::new(("photo-shortcut", i))
                        .accessibility_label(t!(title))
                        .tooltip(t!(title))
                        .ghost()
                        .xsmall()
                        .w(px(30.))
                        .h(px(28.))
                        .flex_none()
                        .border_1()
                        .border_color(if selected {
                            p.accent
                        } else {
                            gpui_kit::transparent_black()
                        })
                        .when(selected, |b| b.bg(p.accent.opacity(0.12)))
                        .child(
                            rail::tool_icon(icon)
                                .text_color(if selected { p.accent } else { p.ink })
                                .size(px(14.)),
                        )
                        .on_click(cx.listener(move |this, _, _, cx| {
                            this.toggle_canvas_panel(tab, cx);
                        }))
                }))
                .children(quick_controls),
        );
        deferred(overlay).with_priority(1).into_any_element()
    }
}
