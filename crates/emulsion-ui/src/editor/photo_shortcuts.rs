//! Canvas-side shortcuts and a single live panel, matching the Photo handoff.
use super::*;
use gpui_kit::component::{
    Sizable,
    button::{Button, ButtonVariants},
};

impl EditorView {
    pub(super) fn photo_shortcuts(
        &mut self,
        p: &Palette,
        window: &mut Window,
        cx: &mut Context<Self>,
    ) -> AnyElement {
        let tabs = [
            (SidebarTab::Properties, "Properties", "sliders-horizontal"),
            (SidebarTab::BrushSettings, "Brushes", "brush"),
            (SidebarTab::History, "History", "history"),
            (SidebarTab::Character, "Character", "type"),
            (SidebarTab::Assistant, "Assistant", "sparkles"),
        ];
        let mut overlay = div()
            .id("photo-shortcuts-overlay")
            .test_support()
            .absolute()
            .size_full();
        if self.sidebar_layout.flyout_open {
            let (title, icon) = tabs
                .iter()
                .find(|(tab, _, _)| *tab == self.sidebar_tab)
                .map(|(_, title, icon)| (*title, *icon))
                .unwrap_or(("Panel", "sliders-horizontal"));
            let content = self.sidebar_content(p, window, cx);
            overlay = overlay.child(
                div()
                    .id("photo-shortcut-panel")
                    .test_support()
                    .absolute()
                    .top(px(18.))
                    .bottom(px(12.))
                    .right(px(52.))
                    .w(px(300.))
                    .max_w(relative(0.78))
                    .flex()
                    .flex_col()
                    .bg(p.panel)
                    .border_1()
                    .border_color(p.line)
                    .rounded(px(10.))
                    .shadow_lg()
                    .occlude()
                    .overflow_hidden()
                    .child(
                        div()
                            .h(px(40.))
                            .flex_none()
                            .flex()
                            .items_center()
                            .gap(px(8.))
                            .px(px(10.))
                            .border_b_1()
                            .border_color(p.line)
                            .child(rail::tool_icon(icon).text_color(p.ink).size(px(13.)))
                            .child(div().flex_1().text_size(px(12.)).child(title))
                            .child(
                                Button::new("photo-shortcut-dock")
                                    .accessibility_label("Move to dock")
                                    .tooltip("Move to dock")
                                    .ghost()
                                    .xsmall()
                                    .size(px(26.))
                                    .child(
                                        rail::tool_icon("panel-right")
                                            .text_color(p.ink)
                                            .size(px(13.)),
                                    )
                                    .on_click(cx.listener(|this, _, _, cx| {
                                        this.sidebar_layout.flyout_open = false;
                                        this.sidebar_layout.collapsed = false;
                                        this.sidebar_layout.overlay_open = true;
                                        this.sidebar_layout.upper_collapsed = false;
                                        cx.notify();
                                    })),
                            )
                            .child(
                                Button::new("photo-shortcut-close")
                                    .accessibility_label("Close panel")
                                    .tooltip("Close panel")
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
                        .accessibility_label("Toggle panel dock")
                        .tooltip("Toggle panel dock")
                        .ghost()
                        .xsmall()
                        .w(px(30.))
                        .h(px(28.))
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
                    let selected = self.sidebar_layout.flyout_open && self.sidebar_tab == tab;
                    Button::new(("photo-shortcut", i))
                        .accessibility_label(title)
                        .tooltip(title)
                        .ghost()
                        .xsmall()
                        .w(px(30.))
                        .h(px(28.))
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
                            let open =
                                !(this.sidebar_layout.flyout_open && this.sidebar_tab == tab);
                            // Selecting a shortcut must not expand/collapse the dock.
                            let collapsed = this.sidebar_layout.collapsed;
                            let overlay = this.sidebar_layout.overlay_open;
                            this.select_sidebar(tab, cx);
                            this.sidebar_layout.collapsed = collapsed;
                            this.sidebar_layout.overlay_open = overlay;
                            this.sidebar_layout.flyout_open = open;
                            cx.notify();
                        }))
                })),
        );
        deferred(overlay).with_priority(1).into_any_element()
    }
}
