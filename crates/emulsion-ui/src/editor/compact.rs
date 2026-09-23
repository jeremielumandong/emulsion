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
pub(super) enum Bar {
    Tools,
    Options,
    View,
    Color,
    /// One-click brush shelf: pinned and recent brushes.
    Brushes,
    /// Draw mode's large paint, smudge, erase, layers and colour controls
    /// with size and opacity sliders, like Procreate's side bar.
    Dock,
}

impl Bar {
    pub(super) const ALL: [Self; 6] = [
        Self::Tools,
        Self::Options,
        Self::View,
        Self::Color,
        Self::Brushes,
        Self::Dock,
    ];

    pub(super) fn name(self) -> &'static str {
        match self {
            Self::Tools => "tools",
            Self::Options => "options",
            Self::View => "view",
            Self::Color => "color",
            Self::Brushes => "brushes",
            Self::Dock => "dock",
        }
    }

    pub(super) fn label(self) -> &'static str {
        match self {
            Self::Tools => "Tools",
            Self::Options => "Options",
            Self::View => "View",
            Self::Color => "Colors",
            Self::Brushes => "Brushes",
            Self::Dock => "Draw dock",
        }
    }
}

/// Toolbar sizes offered in the workspace customizer: (label, scale).
pub(super) const SCALES: [(&str, f32); 4] = [("S", 0.85), ("M", 1.0), ("L", 1.25), ("XL", 1.5)];
pub(super) const MIN_SCALE: f32 = 0.75;
pub(super) const MAX_SCALE: f32 = 2.0;

pub(super) struct Toolbar {
    focus: FocusHandle,
    pub(super) edge: Edge,
    pub(super) open: bool,
    /// Multiplies the toolbar's rem-based sizes; 1 is standard.
    pub(super) scale: f32,
    pub(super) position: Point<Pixels>,
    bounds: Rc<Cell<Option<Bounds<Pixels>>>>,
}

pub(super) struct CompactLayout {
    pub(super) bars: [Toolbar; 6],
    area: Rc<Cell<Option<Bounds<Pixels>>>>,
    drop_edge: Option<Edge>,
    pub(super) tool_ids: Vec<String>,
    pub(super) hidden_menu_ids: Vec<String>,
    /// Docked toolbars float over the canvas (Procreate) instead of
    /// taking their own space beside it (Photoshop).
    pub(super) overlay: bool,
}

impl CompactLayout {
    /// The factory arrangement for Photo (`draw == false`) or Draw mode.
    /// Photo keeps every tool option in reach; Draw puts brushes, colours
    /// and large paint controls up front and hides the options bar.
    pub(super) fn for_mode(draw: bool, cx: &App) -> Self {
        let edges = [
            Edge::Left,
            Edge::Top,
            Edge::Bottom,
            Edge::Bottom,
            Edge::Top,
            Edge::Right,
        ];
        // Photo matches Photoshop's Essentials: Tools left with the colour
        // swatches at their foot, the options bar across the top.
        let open = [true, !draw, true, false, draw, draw];
        // Painting favours bigger targets: tools and colours at L size.
        let scale = if draw {
            [1.25, 1., 1., 1.25, 1., 1.]
        } else {
            [1.; 6]
        };
        Self {
            bars: std::array::from_fn(|i| Toolbar {
                focus: cx.focus_handle(),
                edge: edges[i],
                open: open[i],
                scale: scale[i],
                position: point(px(20.), px(60.)),
                bounds: Default::default(),
            }),
            area: Default::default(),
            drop_edge: None,
            tool_ids: Vec::new(),
            hidden_menu_ids: Vec::new(),
            overlay: draw,
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

/// Lays out and paints its child at another rem size, so a whole toolbar
/// (icons, text, padding) grows or shrinks together.
struct WithRemSize {
    rem: Pixels,
    child: AnyElement,
}

impl IntoElement for WithRemSize {
    type Element = Self;
    fn into_element(self) -> Self {
        self
    }
}

impl Element for WithRemSize {
    type RequestLayoutState = ();
    type PrepaintState = ();

    fn id(&self) -> Option<ElementId> {
        None
    }

    fn source_location(&self) -> Option<&'static std::panic::Location<'static>> {
        None
    }

    fn request_layout(
        &mut self,
        _: Option<&GlobalElementId>,
        _: Option<&InspectorElementId>,
        window: &mut Window,
        cx: &mut App,
    ) -> (LayoutId, ()) {
        let child = &mut self.child;
        let id = window.with_rem_size(Some(self.rem), |window| child.request_layout(window, cx));
        (id, ())
    }

    fn prepaint(
        &mut self,
        _: Option<&GlobalElementId>,
        _: Option<&InspectorElementId>,
        _: Bounds<Pixels>,
        _: &mut (),
        window: &mut Window,
        cx: &mut App,
    ) {
        let child = &mut self.child;
        window.with_rem_size(Some(self.rem), |window| {
            child.prepaint(window, cx);
        });
    }

    fn paint(
        &mut self,
        _: Option<&GlobalElementId>,
        _: Option<&InspectorElementId>,
        _: Bounds<Pixels>,
        _: &mut (),
        _: &mut (),
        window: &mut Window,
        cx: &mut App,
    ) {
        let child = &mut self.child;
        window.with_rem_size(Some(self.rem), |window| child.paint(window, cx));
    }
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
        _window: &mut Window,
        cx: &mut Context<Self>,
    ) -> AnyElement {
        let layout = control("compact-layout-trigger", "Workspace")
            .tooltip("Customize tools, menus and workspace presets")
            .on_click(
                cx.listener(|this, _, window, cx| this.toggle_workspace_customizer(window, cx)),
            );
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
            .child(self.mode_switch(p, cx))
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

    pub(super) fn workspace_presets(&self, p: &Palette, cx: &mut Context<Self>) -> AnyElement {
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
                                if id != "minimal" && this.draw_mode != (id == "draw") {
                                    this.toggle_draw_mode(cx);
                                }
                                this.compact = CompactLayout::for_mode(this.draw_mode, cx);
                                this.sidebar_layout.collapsed = id == "minimal";
                                if id == "minimal" {
                                    for bar in [Bar::Options, Bar::View, Bar::Color, Bar::Brushes] {
                                        this.compact.bars[bar as usize].open = false;
                                    }
                                }
                                cx.notify();
                            }))
                    }),
            )
            .into_any_element()
    }

    /// One row per toolbar: show or hide it, pick its size, and dock it to
    /// an edge or float it, all without dragging.
    pub(super) fn toolbar_toggles(&self, p: &Palette, cx: &mut Context<Self>) -> AnyElement {
        let chip = move |b: Button, on: bool| {
            b.when(on, |b| b.bg(p.soft_bg).border_1().border_color(p.accent))
        };
        let all = self.compact.bars[0].scale;
        let uniform = self
            .compact
            .bars
            .iter()
            .all(|b| (b.scale - all).abs() < 0.01);
        let mut all_sizes = div().flex().gap_1();
        for (name, scale) in SCALES {
            all_sizes = all_sizes.child(
                chip(
                    control(
                        SharedString::from(format!("toolbar-scale-all-{name}")),
                        name,
                    )
                    .tooltip(format!(
                        "Every toolbar at {name} size ({:.0}%)",
                        scale * 100.
                    )),
                    uniform && (all - scale).abs() < 0.01,
                )
                .on_click(cx.listener(move |this, _, _, cx| {
                    for bar in &mut this.compact.bars {
                        bar.scale = scale;
                    }
                    cx.notify();
                })),
            );
        }
        let mut rows = div().flex().flex_col().gap_1().child(
            div()
                .flex()
                .items_center()
                .gap_2()
                .child(
                    div()
                        .w(rems(6.))
                        .text_xs()
                        .text_color(p.muted)
                        .child("All toolbars"),
                )
                .child(all_sizes),
        );
        for bar in Bar::ALL {
            let state = &self.compact.bars[bar as usize];
            let (open, edge, scale) = (state.open, state.edge, state.scale);
            let name = bar.name();
            let mut row = div().flex().flex_wrap().items_center().gap_2().child(
                chip(
                    control(
                        SharedString::from(format!("toolbar-toggle-{name}")),
                        bar.label(),
                    )
                    .w(rems(6.))
                    .tooltip(format!("Show or hide the {} toolbar", bar.label())),
                    open,
                )
                .on_click(cx.listener(move |this, _, _, cx| {
                    this.compact.bars[bar as usize].open = !open;
                    if bar == Bar::Tools {
                        this.rail.flyout = None;
                    }
                    cx.notify();
                })),
            );
            let mut sizes = div().flex().gap_1();
            for (label, value) in SCALES {
                sizes = sizes.child(
                    chip(
                        control(
                            SharedString::from(format!("toolbar-scale-{name}-{label}")),
                            label,
                        )
                        .tooltip(format!("{} toolbar at {label} size", bar.label())),
                        (scale - value).abs() < 0.01,
                    )
                    .on_click(cx.listener(move |this, _, _, cx| {
                        this.compact.bars[bar as usize].scale = value;
                        this.compact.bars[bar as usize].open = true;
                        cx.notify();
                    })),
                );
            }
            let mut docks = div().flex().gap_1();
            for (label, tip, target) in [
                ("◧", "Dock left", Edge::Left),
                ("⬒", "Dock top", Edge::Top),
                ("◨", "Dock right", Edge::Right),
                ("⬓", "Dock bottom", Edge::Bottom),
                ("❐", "Float over the canvas", Edge::Floating),
            ] {
                docks = docks.child(
                    chip(
                        control(
                            SharedString::from(format!(
                                "toolbar-dock-{name}-{}",
                                tip.to_lowercase().replace(' ', "-")
                            )),
                            label,
                        )
                        .accessibility_label(format!("{tip}: {}", bar.label()))
                        .tooltip(format!("{tip}: {}", bar.label())),
                        edge == target,
                    )
                    .on_click(cx.listener(move |this, _, _, cx| {
                        let state = &mut this.compact.bars[bar as usize];
                        state.edge = target;
                        state.open = true;
                        cx.notify();
                    })),
                );
            }
            row = row.child(sizes).child(docks);
            rows = rows.child(row);
        }
        rows.child(
            div().child(
                control("layout-reset", "Reset")
                    .tooltip("Restore this mode's default toolbar positions, sizes and panel width")
                    .on_click(cx.listener(|this, _, _, cx| {
                        this.reset_workspace(cx);
                        cx.notify();
                    })),
            ),
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

    /// Move a toolbar one step through the size presets.
    pub(super) fn step_toolbar_scale(&mut self, bar: Bar, step: i32, cx: &mut Context<Self>) {
        let current = self.compact.bars[bar as usize].scale;
        let index = SCALES
            .iter()
            .position(|(_, s)| *s >= current - 0.001)
            .unwrap_or(SCALES.len() - 1) as i32;
        let next = (index + step).clamp(0, SCALES.len() as i32 - 1) as usize;
        self.compact.bars[bar as usize].scale = SCALES[next].1;
        cx.notify();
    }

    /// A toolbar's frame. Returns the edge it occupies when it is docked
    /// beside the canvas (placed in the layout by the caller), or `None`
    /// when it is positioned over the canvas.
    fn toolbar_shell(
        &self,
        bar: Bar,
        content: AnyElement,
        p: &Palette,
        window: &Window,
        cx: &mut Context<Self>,
    ) -> (Option<Edge>, AnyElement) {
        let state = &self.compact.bars[bar as usize];
        let attached = !self.compact.overlay && state.edge != Edge::Floating;
        let vertical = match bar {
            Bar::Tools | Bar::Color | Bar::Dock => !matches!(state.edge, Edge::Top | Edge::Bottom),
            Bar::Brushes => matches!(state.edge, Edge::Left | Edge::Right),
            _ => false,
        };
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
        let scale = state.scale;
        let shell = div().id(SharedString::from(format!("canvas-toolbar-{}", bar.name()))).test_support()
            .occlude().relative().flex().items_center().gap_1().p_1()
            .track_focus(&state.focus)
            .on_key_down(cx.listener(move |this, e: &KeyDownEvent, _, cx| {
                let next = match e.keystroke.key.as_str() { "left" => Edge::Left, "right" => Edge::Right, "up" => Edge::Top, "down" => Edge::Bottom, _ => return };
                this.compact.bars[bar as usize].edge = next;
                cx.stop_propagation(); cx.notify();
            }))
            .when(vertical, |d| d.flex_col())
            .bg(p.panel).border_color(p.line)
            .when(!attached, |d| d.border_1().shadow_md())
            .when(attached, |d| match edge {
                Edge::Left => d.h_full().border_r_1(),
                Edge::Right => d.h_full().border_l_1(),
                Edge::Top => d.w_full().border_b_1(),
                _ => d.w_full().border_t_1(),
            })
            .on_mouse_down(MouseButton::Left, |_, _, cx| cx.stop_propagation())
            .child(control(SharedString::from(format!("toolbar-grip-{}", bar.name())), "⠿")
                .accessibility_label(format!("Move {} toolbar", bar.name()))
                .tooltip("Drag to move; release near an edge to dock or anywhere to float. Arrow keys dock, + and - resize; Escape cancels dragging.")
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
                    let next = match e.keystroke.key.as_str() {
                        "left" => Edge::Left, "right" => Edge::Right, "up" => Edge::Top, "down" => Edge::Bottom,
                        "=" | "+" => { this.step_toolbar_scale(bar, 1, cx); cx.stop_propagation(); return }
                        "-" => { this.step_toolbar_scale(bar, -1, cx); cx.stop_propagation(); return }
                        _ => return,
                    };
                    this.compact.bars[bar as usize].edge = next;
                    cx.stop_propagation(); cx.notify();
                })))
            .child(content)
            .when(attached, |d| d.child(div().flex_1()))
            .child(control(SharedString::from(format!("toolbar-close-{}", bar.name())), "×").tooltip(format!("Hide {} toolbar", bar.name()))
                .accessibility_label(format!("Hide {} toolbar", bar.name()))
                .on_click(cx.listener(move |this, _, window, cx| {
                    this.compact.bars[bar as usize].open = false;
                    this.rail.flyout = None;
                    window.focus(&this.canvas_focus, cx);
                    cx.notify();
                })))
            .child(canvas(move |bounds, _, _| measure.set(Some(bounds)), |_, _, _, _| {}).absolute().size_full());
        let shell = WithRemSize {
            rem: window.rem_size() * scale,
            child: shell.into_any_element(),
        };
        if attached {
            return (Some(edge), shell.into_any_element());
        }
        let offset = px(8. + lane);
        let wrapper = div().absolute().flex();
        let wrapper = match edge {
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
        };
        (None, wrapper.into_any_element())
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
        let attached_row = !self.compact.overlay
            && matches!(
                self.compact.bars[Bar::Options as usize].edge,
                Edge::Top | Edge::Bottom
            );
        let count = if attached_row {
            // A full-width options bar, as in Photoshop: show what fits.
            ((available - 260.) / 150.).max(0.) as usize
        } else if self.brushy() {
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
            .bg(p.stage);
        let measure_area = canvas(
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
        .size_full();
        let mut docked: Vec<(Edge, AnyElement)> = Vec::new();
        let mut overlays: Vec<AnyElement> = Vec::new();
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
                    // Photoshop keeps the colour swatches at the foot of the
                    // Tools panel; they live here unless the Colors bar is shown.
                    let swatches = !self.compact.bars[Bar::Color as usize].open;
                    let available = (if horizontal {
                        f32::from(area.width) - 140.
                    } else {
                        f32::from(area.height) - 200.
                    } - if swatches { 56. } else { 0. })
                        / self.compact.bars[bar as usize].scale;
                    let rail = if !self.compact.tool_ids.is_empty() {
                        self.custom_tool_rail(
                            horizontal,
                            (available / f32::from(window.rem_size())).max(3.5),
                            p,
                            cx,
                        )
                    } else {
                        self.compact_tool_rail(
                            horizontal,
                            (available / f32::from(window.rem_size())).max(3.5),
                            p,
                            cx,
                        )
                        .into_any_element()
                    };
                    if swatches {
                        div()
                            .flex()
                            .items_center()
                            .gap_2()
                            .when(!horizontal, |d| d.flex_col())
                            .child(rail)
                            .child(
                                div()
                                    .id("tool-rail-swatches")
                                    .test_support()
                                    .child(self.swatches(p, cx)),
                            )
                            .into_any_element()
                    } else {
                        rail
                    }
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
                Bar::Color => {
                    let vertical = !matches!(
                        self.compact.bars[bar as usize].edge,
                        Edge::Top | Edge::Bottom
                    );
                    div()
                        .id("tool-rail-swatches")
                        .test_support()
                        .flex()
                        .items_center()
                        .gap_1()
                        .when(vertical, |d| d.flex_col())
                        .child(self.swatches(p, cx))
                        .child(mono(
                            format!(
                                "#{:02X}{:02X}{:02X}",
                                self.tools.fg[0], self.tools.fg[1], self.tools.fg[2]
                            ),
                            9.5,
                            p.muted,
                        ))
                        .child(self.project_colors(vertical, p, cx))
                        .into_any_element()
                }
                Bar::Brushes => {
                    let vertical = matches!(
                        self.compact.bars[bar as usize].edge,
                        Edge::Left | Edge::Right
                    );
                    self.brush_shelf(vertical, p, cx)
                }
                Bar::Dock => {
                    let horizontal = matches!(
                        self.compact.bars[bar as usize].edge,
                        Edge::Top | Edge::Bottom
                    );
                    self.draw_dock(horizontal, p, cx)
                }
            };
            match self.toolbar_shell(bar, content, p, window, cx) {
                (Some(edge), element) => docked.push((edge, element)),
                (None, element) => overlays.push(element),
            }
        }
        // Docked toolbars sit beside the canvas, Photoshop-style; the canvas
        // keeps whatever room is left.
        let mut sides: [Vec<AnyElement>; 4] = Default::default();
        for (edge, element) in docked {
            let side = match edge {
                Edge::Top => 0,
                Edge::Left => 1,
                Edge::Right => 2,
                _ => 3,
            };
            sides[side].push(element);
        }
        let [tops, lefts, rights, bottoms] = sides;
        let overlay = self.compact.overlay;
        let status = self.status_strip(p, cx);
        stage = stage
            .child(
                div()
                    .id("editor-dock-frame")
                    .test_support()
                    .flex()
                    .flex_col()
                    .flex_1()
                    .min_h_0()
                    .children(tops)
                    .child(
                        div()
                            .flex()
                            .flex_1()
                            .min_h_0()
                            .children(lefts)
                            .child(
                                div()
                                    .relative()
                                    .flex()
                                    .flex_col()
                                    .flex_1()
                                    .min_w_0()
                                    .min_h_0()
                                    .overflow_hidden()
                                    .child(canvas_view),
                            )
                            .children(rights),
                    )
                    .children(bottoms)
                    .when(!overlay, |d| d.child(div().flex_none().child(status))),
            )
            .child(measure_area)
            .children(overlays);
        stage = stage.children(self.brush_gallery(p, window, cx));
        stage = stage.children(self.workspace_customizer(p, window, cx));
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
        if overlay {
            stage = stage.child(
                div()
                    .absolute()
                    .bottom_0()
                    .left_0()
                    .right_0()
                    .child(self.status_strip(p, cx)),
            );
        }
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
