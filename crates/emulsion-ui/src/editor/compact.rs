//! Canvas-owned toolbars. Docking changes presentation, never document history.
use super::*;
use gpui_kit::component::{
    Sizable,
    button::{Button, ButtonVariants},
    popover::Popover,
};
use std::cell::Cell;

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub(super) enum Edge {
    Left,
    Right,
    Top,
    Bottom,
    Floating,
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
enum Bar {
    Tools,
    Options,
    View,
    Color,
}

impl Bar {
    const ALL: [Self; 4] = [Self::Tools, Self::Options, Self::View, Self::Color];

    fn name(self) -> &'static str {
        match self {
            Self::Tools => "tools",
            Self::Options => "options",
            Self::View => "view",
            Self::Color => "color",
        }
    }
}

struct Toolbar {
    focus: FocusHandle,
    edge: Edge,
    open: bool,
    position: Point<Pixels>,
    bounds: Rc<Cell<Option<Bounds<Pixels>>>>,
}

pub(super) struct CompactLayout {
    bars: [Toolbar; 4],
    area: Rc<Cell<Option<Bounds<Pixels>>>>,
    drop_edge: Option<Edge>,
}

impl CompactLayout {
    pub(super) fn new(cx: &App) -> Self {
        Self {
            bars: [Edge::Left, Edge::Top, Edge::Bottom, Edge::Bottom].map(|edge| Toolbar {
                focus: cx.focus_handle(),
                edge,
                open: true,
                position: point(px(20.), px(60.)),
                bounds: Default::default(),
            }),
            area: Default::default(),
            drop_edge: None,
        }
    }
}

#[derive(Clone, Copy)]
pub(super) struct ToolbarDrag {
    bar: Bar,
    offset: Point<Pixels>,
    original_edge: Edge,
    original_position: Point<Pixels>,
}

fn control(id: impl Into<ElementId>, label: impl Into<SharedString>) -> Button {
    Button::new(id).label(label).xsmall().ghost()
}

impl EditorView {
    pub(crate) fn compact_header(
        &mut self,
        navigation: AnyElement,
        tabs: AnyElement,
        theme_controls: AnyElement,
        p: &Palette,
        window: &mut Window,
        cx: &mut Context<Self>,
    ) -> AnyElement {
        let wide =
            f32::from(window.viewport_size().width) / f32::from(window.rem_size()) * 16. >= 1500.;
        let editor = cx.entity().downgrade();
        let layout = if wide {
            self.layout_controls(p, cx)
        } else {
            Popover::new("compact-layout-menu")
                .trigger(
                    control("compact-layout-trigger", "⋯")
                        .tooltip("Workspace presets and toolbars"),
                )
                .content(move |_, _, cx| {
                    editor
                        .update(cx, |this, cx| {
                            let p = theme::palette(cx);
                            div()
                                .id("compact-layout-menu-content")
                                .test_support()
                                .flex()
                                .flex_col()
                                .gap_2()
                                .w(rems(11.875))
                                .p_2()
                                .child(label("Workspace", &p))
                                .child(this.workspace_presets(&p, cx))
                                .child(label("Toolbars", &p))
                                .child(this.toolbar_toggles(&p, cx))
                                .into_any_element()
                        })
                        .unwrap_or_else(|_| div().into_any_element())
                })
                .into_any_element()
        };
        let d = &self.editor.doc;
        let dimensions = format!(
            "{}×{} · {} bit",
            d.width,
            d.height,
            if d.source_depth == 16 { 16 } else { 8 }
        );
        let head = self.editor.graph.head().to_string();
        div()
            .id("editor-document-bar")
            .test_support()
            .flex()
            .items_center()
            .w_full()
            .min_w_0()
            .h(rems(2.25))
            .flex_none()
            .gap_1()
            .px_2()
            .bg(p.paper)
            .border_b_1()
            .border_color(p.line)
            .child(div().flex().items_center().child(navigation))
            .child(div().flex().items_center().child(self.effect_menus(p, cx)))
            .child(
                div()
                    .id("compact-tab-leading-drag")
                    .test_support()
                    .w(rems(1.5))
                    .h_full()
                    .flex_none()
                    .window_control_area(WindowControlArea::Drag),
            )
            .child(tabs)
            .child(
                div()
                    .id("compact-window-drag")
                    .test_support()
                    .flex_1()
                    .min_w(rems(1.5))
                    .h_full()
                    .window_control_area(WindowControlArea::Drag),
            )
            .child(
                control("doc-size", dimensions)
                    .tooltip("Image and canvas size")
                    .on_click(
                        cx.listener(|this, _, window, cx| this.toggle_size_panel(window, cx)),
                    ),
            )
            .child(
                control("branch-badge", head)
                    .tooltip("Branches and saved versions")
                    .on_click(cx.listener(|this, _, window, cx| {
                        if this.history.open {
                            this.close_history(window, cx);
                        } else {
                            this.open_history(cx);
                        }
                    })),
            )
            .child(div().flex().items_center().child(layout))
            .child(
                control("save", "Save")
                    .outline()
                    .on_click(cx.listener(|_, _, window, cx| {
                        window.dispatch_action(Box::new(crate::actions::Save), cx)
                    })),
            )
            .child(
                control("export", "Export")
                    .outline()
                    .on_click(cx.listener(|this, _, _, cx| this.toggle_export_panel(cx))),
            )
            .child(theme_controls)
            .into_any_element()
    }

    fn layout_controls(&self, p: &Palette, cx: &mut Context<Self>) -> AnyElement {
        div()
            .flex()
            .items_center()
            .gap_1()
            .child(self.workspace_presets(p, cx))
            .child(div().w_px().h_4().bg(p.line))
            .child(self.toolbar_toggles(p, cx))
            .into_any_element()
    }

    fn workspace_presets(&self, p: &Palette, cx: &mut Context<Self>) -> AnyElement {
        let minimal = !self.compact.bars[Bar::Options as usize].open
            && !self.compact.bars[Bar::View as usize].open;
        div()
            .flex()
            .flex_wrap()
            .items_center()
            .gap_1()
            .children(
                [("photo", "Photo"), ("draw", "Draw"), ("minimal", "Minimal")]
                    .into_iter()
                    .map(|(id, name)| {
                        let selected = match id {
                            "minimal" => minimal,
                            "draw" => self.draw_mode && !minimal,
                            _ => !self.draw_mode && !minimal,
                        };
                        control(SharedString::from(format!("layout-preset-{id}")), name)
                            .when(selected, |b| b.bg(p.ink).text_color(p.paper))
                            .on_click(cx.listener(move |this, _, _, cx| {
                                this.compact = CompactLayout::new(cx);
                                this.sidebar_layout.collapsed = id == "minimal";
                                if id == "minimal" {
                                    for bar in [Bar::Options, Bar::View, Bar::Color] {
                                        this.compact.bars[bar as usize].open = false;
                                    }
                                } else if this.draw_mode != (id == "draw") {
                                    this.toggle_draw_mode(cx);
                                }
                                cx.notify();
                            }))
                    }),
            )
            .into_any_element()
    }

    fn toolbar_toggles(&self, p: &Palette, cx: &mut Context<Self>) -> AnyElement {
        div()
            .flex()
            .flex_wrap()
            .items_center()
            .gap_1()
            .children(Bar::ALL.into_iter().map(|bar| {
                let open = self.compact.bars[bar as usize].open;
                control(
                    SharedString::from(format!("toolbar-toggle-{}", bar.name())),
                    bar.name(),
                )
                .tooltip(format!("Show or hide the {} toolbar", bar.name()))
                .when(open, |b| b.bg(p.soft_bg).border_1().border_color(p.line))
                .on_click(cx.listener(move |this, _, _, cx| {
                    this.compact.bars[bar as usize].open = !open;
                    if bar == Bar::Tools {
                        this.rail.flyout = None;
                    }
                    cx.notify();
                }))
            }))
            .child(
                control("layout-reset", "Reset")
                    .tooltip("Restore default toolbar positions and panel width")
                    .on_click(cx.listener(|this, _, _, cx| {
                        this.compact = CompactLayout::new(cx);
                        this.sidebar_layout = Default::default();
                        cx.notify();
                    })),
            )
            .into_any_element()
    }

    pub(super) fn move_toolbar(
        &mut self,
        drag: ToolbarDrag,
        pos: Point<Pixels>,
        cx: &mut Context<Self>,
    ) {
        let Some(area) = self.compact.area.get() else {
            return;
        };
        let bar = &mut self.compact.bars[drag.bar as usize];
        let size = bar.bounds.get().map(|b| b.size).unwrap_or_default();
        let position = pos - area.origin - drag.offset;
        bar.position = point(
            position
                .x
                .max(px(0.))
                .min((area.size.width - size.width).max(px(0.))),
            position
                .y
                .max(px(0.))
                .min((area.size.height - size.height - px(24.)).max(px(0.))),
        );
        bar.edge = Edge::Floating;
        let local = pos - area.origin;
        self.compact.drop_edge = if local.x < px(48.) {
            Some(Edge::Left)
        } else if local.x > area.size.width - px(48.) {
            Some(Edge::Right)
        } else if local.y < px(48.) {
            Some(Edge::Top)
        } else if local.y > area.size.height - px(64.) {
            Some(Edge::Bottom)
        } else {
            None
        };
        cx.notify();
    }

    pub(super) fn finish_toolbar(&mut self, drag: ToolbarDrag, cx: &mut Context<Self>) {
        if let Some(edge) = self.compact.drop_edge.take() {
            self.compact.bars[drag.bar as usize].edge = edge;
        }
        cx.notify();
    }

    fn toolbar_shell(
        &self,
        bar: Bar,
        content: AnyElement,
        p: &Palette,
        cx: &mut Context<Self>,
    ) -> AnyElement {
        let state = &self.compact.bars[bar as usize];
        let vertical = matches!(bar, Bar::Tools | Bar::Color)
            && !matches!(state.edge, Edge::Top | Edge::Bottom);
        let bounds = state.bounds.clone();
        let measure = bounds.clone();
        let edge = state.edge;
        let position = state.position;
        let lane: f32 = Bar::ALL
            .into_iter()
            .take(bar as usize)
            .filter(|other| {
                let other = &self.compact.bars[*other as usize];
                other.open && other.edge == edge
            })
            .map(|other| {
                self.compact.bars[other as usize]
                    .bounds
                    .get()
                    .map(|bounds| {
                        f32::from(if matches!(edge, Edge::Left | Edge::Right) {
                            bounds.size.width
                        } else {
                            bounds.size.height
                        }) + 8.
                    })
                    .unwrap_or(48.)
            })
            .sum();
        let focus = state.focus.clone();
        let shell = div().id(SharedString::from(format!("canvas-toolbar-{}", bar.name()))).test_support()
            .occlude().relative().flex().items_center().gap_1().p_1()
            .track_focus(&state.focus)
            .on_key_down(cx.listener(move |this, e: &KeyDownEvent, _, cx| {
                let next = match e.keystroke.key.as_str() { "left" => Edge::Left, "right" => Edge::Right, "up" => Edge::Top, "down" => Edge::Bottom, _ => return };
                this.compact.bars[bar as usize].edge = next;
                cx.stop_propagation(); cx.notify();
            }))
            .when(vertical, |d| d.flex_col())
            .bg(p.panel).border_1().border_color(p.line).shadow_md()
            .on_mouse_down(MouseButton::Left, |_, _, cx| cx.stop_propagation())
            .child(control(SharedString::from(format!("toolbar-grip-{}", bar.name())), "⠿")
                .accessibility_label(format!("Move {} toolbar", bar.name()))
                .tooltip("Drag to move; release near an edge to dock. Arrow keys dock; Escape cancels dragging.")
                .cursor(CursorStyle::OpenHand)
                .on_mouse_down(MouseButton::Left, cx.listener(move |this, e: &MouseDownEvent, window, cx| {
                    if let Some(bounds) = bounds.get() {
                        window.focus(&focus, cx);
                        this.drag = Some(Drag::Toolbar(ToolbarDrag { bar, offset: e.position - bounds.origin, original_edge: edge, original_position: position }));
                        cx.stop_propagation();
                        cx.notify();
                    }
                }))
                .on_key_down(cx.listener(move |this, e: &KeyDownEvent, _, cx| {
                    let next = match e.keystroke.key.as_str() { "left" => Edge::Left, "right" => Edge::Right, "up" => Edge::Top, "down" => Edge::Bottom, _ => return };
                    this.compact.bars[bar as usize].edge = next;
                    cx.stop_propagation(); cx.notify();
                })))
            .child(content)
            .child(control(SharedString::from(format!("toolbar-close-{}", bar.name())), "×").tooltip(format!("Hide {} toolbar", bar.name()))
                .accessibility_label(format!("Hide {} toolbar", bar.name()))
                .on_click(cx.listener(move |this, _, window, cx| {
                    this.compact.bars[bar as usize].open = false;
                    this.rail.flyout = None;
                    window.focus(&this.canvas_focus, cx);
                    cx.notify();
                })))
            .child(canvas(move |bounds, _, _| measure.set(Some(bounds)), |_, _, _, _| {}).absolute().size_full());
        let offset = px(8. + lane);
        let wrapper = div().absolute().flex();
        match edge {
            Edge::Left => wrapper
                .left(offset)
                .top(rems(3.))
                .bottom(rems(6.))
                .items_center()
                .child(shell),
            Edge::Right => wrapper
                .right(offset)
                .top(rems(3.))
                .bottom(rems(6.))
                .items_center()
                .child(shell),
            Edge::Top => wrapper
                .top(offset)
                .left(rems(3.5))
                .right_2()
                .justify_center()
                .child(shell),
            Edge::Bottom => wrapper
                .bottom(px(32. + lane))
                .left(rems(3.5))
                .right_2()
                .justify_center()
                .child(shell),
            Edge::Floating => {
                let area = self.compact.area.get();
                let size = state.bounds.get().map(|b| b.size).unwrap_or_default();
                let x = area
                    .map(|a| position.x.min((a.size.width - size.width).max(px(0.))))
                    .unwrap_or(position.x);
                let y = area
                    .map(|a| {
                        position
                            .y
                            .min((a.size.height - size.height - px(24.)).max(px(0.)))
                    })
                    .unwrap_or(position.y);
                wrapper.left(x).top(y).child(shell)
            }
        }
        .into_any_element()
    }

    fn compact_options(
        &mut self,
        p: &Palette,
        window: &Window,
        cx: &mut Context<Self>,
    ) -> AnyElement {
        let available = self
            .compact
            .area
            .get()
            .map(|b| f32::from(b.size.width))
            .unwrap_or(f32::from(window.viewport_size().width) - 320.);
        // Reserve the tool name, grip and disclosure before exposing controls.
        // The remainder stays in a keyboard-accessible popover at every width.
        let count = if self.brushy() {
            if available > 780. {
                3
            } else if available > 610. {
                2
            } else if available > 420. {
                1
            } else {
                0
            }
        } else {
            0
        };
        let options = self.tool_options(p, cx);
        let has_more = options.len() > count;
        let editor = cx.entity().downgrade();
        div()
            .id("editor-tool-options")
            .test_support()
            .flex()
            .items_center()
            .gap_1()
            .min_w_0()
            .text_size(rems(0.625))
            .text_color(p.muted)
            .child(
                div()
                    .text_color(p.ink)
                    .whitespace_nowrap()
                    .child(self.active_tool_name()),
            )
            .children(options.into_iter().take(count))
            .when(has_more, |d| {
                d.child(
                    Popover::new("tool-options-overflow")
                        .trigger(control("tool-options-more", "···").tooltip("More tool options"))
                        .content(move |_, window, cx| {
                            editor
                                .update(cx, |this, cx| {
                                    let p = theme::palette(cx);
                                    div()
                                        .id("tool-options-overflow-content")
                                        .test_support()
                                        .flex()
                                        .flex_col()
                                        .gap_2()
                                        .w(rems(20.))
                                        .max_h(window.viewport_size().height - px(80.))
                                        .overflow_y_scroll()
                                        .p_2()
                                        .children(this.tool_options(&p, cx).into_iter().skip(count))
                                        .children(this.font_picker(&p, cx))
                                        .into_any_element()
                                })
                                .unwrap_or_else(|_| div().into_any_element())
                        }),
                )
            })
            .into_any_element()
    }

    pub(super) fn compact_editor(
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
        let panel = self.node_panel(p, window, cx);
        let canvas_view = self.canvas_area(p, window, cx);
        let area_bounds = self.compact.area.clone();
        let editor = cx.entity().downgrade();
        let mut stage = div()
            .id("editor-canvas-column")
            .test_support()
            .relative()
            .flex()
            .flex_col()
            .flex_1()
            .min_w_0()
            .min_h_0()
            .overflow_hidden()
            .bg(p.stage)
            .child(canvas_view)
            .child(
                canvas(
                    move |bounds, _, cx| {
                        if area_bounds.replace(Some(bounds)) != Some(bounds) {
                            let editor = editor.clone();
                            cx.defer(move |cx| {
                                editor.update(cx, |_, cx| cx.notify()).ok();
                            });
                        }
                    },
                    |_, _, _, _| {},
                )
                .absolute()
                .size_full(),
            );
        for bar in Bar::ALL {
            if !self.compact.bars[bar as usize].open {
                continue;
            }
            let content = match bar {
                Bar::Tools => {
                    let horizontal = matches!(
                        self.compact.bars[bar as usize].edge,
                        Edge::Top | Edge::Bottom
                    );
                    let area = self
                        .compact
                        .area
                        .get()
                        .map(|b| b.size)
                        .unwrap_or(window.viewport_size());
                    let available = if horizontal {
                        f32::from(area.width) - 140.
                    } else {
                        f32::from(area.height) - 200.
                    };
                    self.compact_tool_rail(
                        horizontal,
                        (available / f32::from(window.rem_size())).max(3.5),
                        p,
                        cx,
                    )
                    .into_any_element()
                }
                Bar::Options => self.compact_options(p, window, cx),
                Bar::View => div()
                    .flex()
                    .flex_wrap()
                    .items_center()
                    .gap_1()
                    .max_w(px(self
                        .compact
                        .area
                        .get()
                        .map(|b| f32::from(b.size.width))
                        .unwrap_or(f32::from(window.viewport_size().width) - 320.)
                        - 150.))
                    .children(self.view_controls(p, cx))
                    .into_any_element(),
                Bar::Color => div()
                    .id("tool-rail-swatches")
                    .test_support()
                    .flex()
                    .items_center()
                    .gap_1()
                    .child(self.swatches(p, cx))
                    .child(mono(
                        format!(
                            "#{:02X}{:02X}{:02X}",
                            self.tools.fg[0], self.tools.fg[1], self.tools.fg[2]
                        ),
                        9.5,
                        p.muted,
                    ))
                    .into_any_element(),
            };
            stage = stage.child(self.toolbar_shell(bar, content, p, cx));
        }
        if let Some(edge) = self.compact.drop_edge {
            let guide = div()
                .absolute()
                .border_1()
                .border_color(p.accent)
                .bg(p.accent.opacity(0.12));
            stage = stage.child(match edge {
                Edge::Left => guide.left_0().top_0().bottom_0().w_8(),
                Edge::Right => guide.right_0().top_0().bottom_0().w_8(),
                Edge::Top => guide.left_0().top_0().right_0().h_8(),
                _ => guide.left_0().bottom_0().right_0().h_8(),
            });
        }
        stage = stage.child(
            div()
                .absolute()
                .bottom_0()
                .left_0()
                .right_0()
                .child(self.status_strip(p, cx)),
        );
        div()
            .flex()
            .flex_col()
            .flex_1()
            .min_h_0()
            .track_focus(&self.focus)
            .on_key_down(cx.listener(|this, e: &KeyDownEvent, _, cx| {
                if e.keystroke.key == "escape"
                    && let Some(Drag::Toolbar(drag)) = this.drag.take()
                {
                    let bar = &mut this.compact.bars[drag.bar as usize];
                    bar.edge = drag.original_edge;
                    bar.position = drag.original_position;
                    this.compact.drop_edge = None;
                    cx.stop_propagation();
                    cx.notify();
                }
            }))
            .on_modifiers_changed(cx.listener(|this, event: &ModifiersChangedEvent, _, cx| {
                this.drag_shift = event.modifiers.shift;
                if matches!(this.tool, Tool::Zoom | Tool::Shape)
                    || matches!(this.drag, Some(Drag::Transform(_)))
                {
                    cx.notify();
                }
            }))
            .children(self.size_panel_view(p, cx))
            .children(self.export_panel_view(p, cx))
            .children(self.ask_bar(p, cx))
            .child(
                div()
                    .id("editor-work-area")
                    .test_support()
                    .flex()
                    .flex_1()
                    .min_h_0()
                    .child(stage)
                    .child(panel),
            )
            .children(self.assistant_dock(p, cx))
            .children(self.picker(p, cx))
            .into_any_element()
    }
}
