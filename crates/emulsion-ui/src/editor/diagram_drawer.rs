//! Shapes drawer chrome: section headings, shape tiles, the tools strip and tips.
use super::*;
use gpui_kit::{assets::IconName, component::Icon};

/// Square tile edge for a five-column grid inside the 250px drawer.
pub(crate) const TILE: f32 = 41.;

/// Uppercase disclosure heading; attach `on_click` to toggle the section.
pub(crate) fn section_header(
    id: impl Into<ElementId>,
    name: &str,
    count: Option<usize>,
    open: bool,
    p: &Palette,
) -> Stateful<Div> {
    let ink = p.ink;
    div()
        .id(id)
        .flex()
        .flex_1()
        .min_w_0()
        .items_center()
        .gap(px(6.))
        .h(px(24.))
        .cursor_pointer()
        .text_color(p.muted)
        .hover(move |d| d.text_color(ink))
        .child(
            Icon::new(if open {
                IconName::ChevronDown
            } else {
                IconName::ChevronRight
            })
            .size(px(10.))
            .text_color(p.muted),
        )
        .child(
            div()
                .min_w_0()
                .overflow_hidden()
                .text_ellipsis()
                .whitespace_nowrap()
                .text_size(px(11.))
                .font_weight(FontWeight::SEMIBOLD)
                .child(name.to_uppercase()),
        )
        .when_some(count, |d, count| {
            d.child(pill(count.to_string(), p.soft_bg, p.muted))
        })
}

pub(crate) fn pill(text: impl Into<SharedString>, bg: Hsla, fg: Hsla) -> Div {
    div()
        .flex_none()
        .px(px(6.))
        .py(px(1.))
        .rounded(px(99.))
        .bg(bg)
        .text_color(fg)
        .text_size(px(10.))
        .font_weight(FontWeight::SEMIBOLD)
        .child(text.into())
}

pub(crate) fn tile_grid(id: impl Into<ElementId>, columns: u16) -> Stateful<Div> {
    div().id(id).grid().grid_cols(columns).gap(px(6.))
}

/// A shape tile with the accent hover ring.
pub(crate) fn tile(id: impl Into<ElementId>, bg: Hsla, p: &Palette) -> Stateful<Div> {
    let accent = p.accent;
    div()
        .id(id)
        .w_full()
        .min_w_0()
        .h(px(TILE))
        .rounded(px(8.))
        .bg(bg)
        .border_1()
        .border_color(transparent_black())
        .cursor_pointer()
        .hover(move |d| d.border_color(accent).bg(accent.opacity(0.10)))
}

/// Borderless icon button used in the drawer chrome.
pub(crate) fn icon_button(
    id: impl Into<ElementId>,
    icon: IconName,
    tip: impl Into<SharedString>,
) -> Button {
    Button::new(id)
        .icon(icon)
        .ghost()
        .with_size(px(24.))
        .tooltip(tip)
}

/// Icon button that opens a menu, marked by a small caret.
fn menu_button(
    id: impl Into<ElementId>,
    icon: IconName,
    tip: impl Into<SharedString>,
    p: &Palette,
) -> Button {
    Button::new(id)
        .ghost()
        .compact()
        .h(px(24.))
        .px(px(3.))
        .tooltip(tip)
        .child(
            div()
                .flex()
                .items_center()
                .gap(px(1.))
                .child(Icon::new(icon).size(px(16.)).text_color(p.ink))
                .child(
                    Icon::new(IconName::ChevronDown)
                        .size(px(8.))
                        .text_color(p.ink),
                ),
        )
}

impl EditorView {
    /// Labeled source insertion above the compact canvas-wide action strip.
    pub(super) fn diagram_tools_strip(&self, p: &Palette, cx: &mut Context<Self>) -> AnyElement {
        let connecting = self.diagram_ui.connecting;
        let layout_owner = cx.weak_entity();
        let data_owner = cx.weak_entity();
        let has_rules = self
            .editor
            .doc
            .diagram
            .as_ref()
            .is_some_and(|d| d.shapes.values().any(|s| !s.conditions.is_empty()));
        let strip = div()
            .id("diagram-tools")
            .test_support()
            .flex()
            .flex_wrap()
            .items_center()
            .gap(px(1.))
            .p(px(3.))
            .rounded(px(8.))
            .bg(p.soft_bg)
            .child(
                icon_button(
                    "diagram-connect",
                    IconName::Spline,
                    if connecting {
                        t!("editor.diagram_drawer.cancel_connection")
                    } else {
                        t!("editor.diagram_drawer.connect_shapes")
                    },
                )
                .selected(connecting)
                .on_click(cx.listener(|this, _, _, cx| {
                    let active = this.diagram_ui.connecting;
                    this.set_tool(Tool::Move, cx);
                    this.diagram_cancel_connection();
                    this.diagram_ui.connecting = !active;
                    this.set_status(
                        if active {
                            t!("editor.diagram_drawer.connection_cancelled")
                        } else {
                            t!("editor.diagram_drawer.connect_hint")
                        },
                        false,
                        cx,
                    );
                })),
            )
            .child(
                menu_button(
                    "diagram-layout",
                    IconName::LayoutGrid,
                    t!("editor.diagram_drawer.arrange"),
                    p,
                )
                .dropdown_menu(move |mut menu, _, _| {
                    for layout in Layout::ALL {
                        let owner = layout_owner.clone();
                        menu = menu.item(PopupMenuItem::new(super::layout_label(layout)).on_click(
                            move |_, _, cx| {
                                owner
                                    .update(cx, |this, cx| this.layout_diagram(layout, cx))
                                    .ok();
                            },
                        ));
                    }
                    menu
                }),
            )
            .child(
                icon_button(
                    "diagram-grid",
                    IconName::Grid3x3,
                    if self.diagram_ui.grid {
                        t!("editor.diagram_drawer.hide_grid")
                    } else {
                        t!("editor.diagram_drawer.show_grid")
                    },
                )
                .selected(self.diagram_ui.grid)
                .on_click(cx.listener(|this, _, _, cx| {
                    this.diagram_ui.grid = !this.diagram_ui.grid;
                    cx.notify();
                })),
            )
            .child(
                icon_button(
                    "diagram-minimap",
                    IconName::Map,
                    t!("editor.diagram_drawer.minimap"),
                )
                .on_click(cx.listener(|this, _, _, cx| this.toggle_navigator(cx))),
            )
            .child(div().w(px(1.)).h(px(18.)).mx(px(3.)).bg(p.line))
            .child(
                icon_button(
                    "diagram-import-file",
                    IconName::Download,
                    t!("editor.diagram_drawer.import_pages"),
                )
                .on_click(cx.listener(|this, _, _, cx| this.import_diagram_file(cx))),
            )
            .child(
                icon_button(
                    "diagram-export-file",
                    IconName::Upload,
                    t!("editor.diagram_drawer.export_drawio"),
                )
                .on_click(cx.listener(|this, _, _, cx| this.export_drawio_file(cx))),
            )
            .child(
                icon_button(
                    "diagram-conditional-fill",
                    IconName::Droplet,
                    t!("editor.diagram_drawer.color_by_data"),
                )
                .on_click(
                    cx.listener(|this, _, window, cx| this.diagram_conditional_fill(window, cx)),
                ),
            )
            .when(has_rules, |strip| {
                strip.child(
                    icon_button(
                        "diagram-clear-conditions",
                        IconName::Eraser,
                        t!("editor.diagram_drawer.clear_rules"),
                    )
                    .on_click(cx.listener(|this, _, _, cx| this.clear_diagram_conditions(cx))),
                )
            });
        div()
            .flex()
            .flex_col()
            .flex_none()
            .gap(px(6.))
            .child(
                Button::new("diagram-generate")
                    .label(t!("editor.diagram_drawer.insert_from_code"))
                    .accessibility_label(t!("editor.diagram_drawer.insert_from_code"))
                    .tooltip(t!("editor.diagram_drawer.insert_tooltip"))
                    .icon(IconName::Table)
                    .dropdown_caret(true)
                    .small()
                    .outline()
                    .w_full()
                    .dropdown_menu(move |mut menu, _, _| {
                        for format in emulsion_io::diagram_data::Format::ALL {
                            let owner = data_owner.clone();
                            menu = menu.item(
                                PopupMenuItem::new(super::super::diagram_data_ui::format_label(
                                    format,
                                ))
                                .on_click(move |_, window, cx| {
                                    owner
                                        .update(cx, |this, cx| {
                                            this.diagram_data_dialog(format, false, window, cx)
                                        })
                                        .ok();
                                }),
                            );
                        }
                        let import = data_owner.clone();
                        let refresh = data_owner.clone();
                        menu.separator()
                            .item(
                                PopupMenuItem::new(t!("editor.diagram_drawer.import_data_file"))
                                    .on_click(move |_, _, cx| {
                                        import
                                            .update(cx, |this, cx| this.import_diagram_data(cx))
                                            .ok();
                                    }),
                            )
                            .item(
                                PopupMenuItem::new(t!("editor.diagram_drawer.refresh_from_csv"))
                                    .on_click(move |_, window, cx| {
                                        refresh
                                            .update(cx, |this, cx| {
                                                this.diagram_data_dialog(
                                                    emulsion_io::diagram_data::Format::Csv,
                                                    true,
                                                    window,
                                                    cx,
                                                )
                                            })
                                            .ok();
                                    }),
                            )
                    }),
            )
            .child(strip)
            .into_any_element()
    }

    /// Collapsible shortcut help pinned to the bottom of the drawer.
    pub(super) fn diagram_tips(&self, p: &Palette, cx: &mut Context<Self>) -> AnyElement {
        let open = self.diagram_ui.tips_open;
        let ink = p.ink;
        let tip = |key: std::borrow::Cow<'static, str>, rest: std::borrow::Cow<'static, str>| {
            div()
                .flex()
                .flex_wrap()
                .gap(px(3.))
                .child(div().text_color(ink).child(key))
                .child(rest)
        };
        div()
            .id("diagram-tips")
            .test_support()
            .flex_none()
            .flex()
            .flex_col()
            .gap(px(6.))
            .mx(px(10.))
            .mb(px(12.))
            .pt(px(8.))
            .border_t_1()
            .border_color(p.line)
            .child(
                div()
                    .id("diagram-tips-toggle")
                    .test_support()
                    .flex()
                    .items_center()
                    .gap(px(6.))
                    .h(px(22.))
                    .cursor_pointer()
                    .text_size(px(11.))
                    .text_color(p.muted)
                    .hover(move |d| d.text_color(ink))
                    .child(
                        Icon::new(IconName::CircleQuestionMark)
                            .size(px(13.))
                            .text_color(p.muted),
                    )
                    .child(div().flex_1().child(t!("editor.diagram_drawer.tips")))
                    .child(
                        Icon::new(if open {
                            IconName::ChevronUp
                        } else {
                            IconName::ChevronDown
                        })
                        .size(px(10.))
                        .text_color(p.muted),
                    )
                    .on_click(cx.listener(|this, _, _, cx| {
                        this.diagram_ui.tips_open = !this.diagram_ui.tips_open;
                        cx.notify();
                    })),
            )
            .when(open, |d| {
                d.child(
                    div()
                        .flex()
                        .flex_col()
                        .gap(px(5.))
                        .pl(px(19.))
                        .text_size(px(11.))
                        .line_height(relative(1.4))
                        .text_color(p.muted)
                        .child(tip(
                            t!("editor.diagram_drawer.tip_drag_key"),
                            t!("editor.diagram_drawer.tip_drag"),
                        ))
                        .child(tip(
                            t!("editor.diagram_drawer.tip_click_key"),
                            t!("editor.diagram_drawer.tip_click"),
                        ))
                        .child(tip("Ctrl+G".into(), t!("editor.diagram_drawer.tip_group")))
                        .child(tip(
                            t!("editor.diagram_drawer.tip_double_key"),
                            t!("editor.diagram_drawer.tip_double"),
                        ))
                        .child(t!("editor.diagram_drawer.tip_connectors")),
                )
            })
            .into_any_element()
    }
}
