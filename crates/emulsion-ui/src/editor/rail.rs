//! The left tool rail, arranged the way Photoshop and GIMP users expect:
//! Move · Marquee · Lasso · Quick select · Crop · Eyedropper · Heal ·
//! Brush · Clone · Eraser · Gradient · Pen · Type · Shape · Mask · Grade ·
//! Hand · Zoom, with the foreground/background swatches at the bottom.
//! Related tools share one slot; the slot shows the member last used, and
//! a right-click (or the corner mark) opens a fly-out with the others.

use super::*;
use std::collections::HashMap;

/// One tool the rail can activate: a tool, plus the sub-mode it selects.
#[derive(Clone, Copy)]
pub struct RailItem {
    pub name: &'static str,
    pub glyph: &'static str,
    /// Default shortcut, for the tooltip and fly-out.
    pub key: &'static str,
    pub tool: Tool,
    pub paint: Option<PaintKind>,
    pub select: Option<SelectShape>,
    pub shape: Option<ShapeKind>,
}

const fn item(name: &'static str, glyph: &'static str, key: &'static str, tool: Tool) -> RailItem {
    RailItem {
        name,
        glyph,
        key,
        tool,
        paint: None,
        select: None,
        shape: None,
    }
}

const fn paint(
    name: &'static str,
    glyph: &'static str,
    key: &'static str,
    k: PaintKind,
) -> RailItem {
    RailItem {
        paint: Some(k),
        ..item(name, glyph, key, Tool::Brush)
    }
}

const fn select(
    name: &'static str,
    glyph: &'static str,
    key: &'static str,
    s: SelectShape,
) -> RailItem {
    RailItem {
        select: Some(s),
        ..item(name, glyph, key, Tool::Select)
    }
}

const fn shape(
    name: &'static str,
    glyph: &'static str,
    key: &'static str,
    s: ShapeKind,
) -> RailItem {
    RailItem {
        shape: Some(s),
        ..item(name, glyph, key, Tool::Shape)
    }
}

/// Rail slots top to bottom; each is a group of one or more items.
pub const GROUPS: &[&[RailItem]] = &[
    &[item("Move", "✥", "V", Tool::Move)],
    &[
        select("Rectangular marquee", "▭", "M", SelectShape::Rect),
        select("Elliptical marquee", "◯", "Shift+M", SelectShape::Ellipse),
    ],
    &[
        select("Lasso", "〰", "L", SelectShape::Lasso),
        select("Polygonal lasso", "⬠", "Shift+L", SelectShape::Polygon),
        select("Magnetic lasso", "⌇", "Alt+L", SelectShape::Magnetic),
    ],
    &[
        select("Quick select (AI)", "✦", "Shift+W", SelectShape::Quick),
        select("Magic wand", "⚚", "W", SelectShape::Wand),
    ],
    &[item("Crop", "⌗", "C", Tool::Crop)],
    &[item("Eyedropper", "◔", "I", Tool::Eyedropper)],
    &[item("Heal", "✚", "J", Tool::Heal)],
    &[
        paint("Brush", "✎", "B", PaintKind::Brush),
        paint("Smudge", "☁", "Shift+B", PaintKind::Smudge),
        paint("Liquify", "≈", "Shift+J", PaintKind::Liquify),
    ],
    &[item("Clone stamp", "◎", "S", Tool::Clone)],
    &[paint("Eraser", "◻", "E", PaintKind::Eraser)],
    &[
        paint("Gradient", "▤", "Shift+G", PaintKind::Gradient),
        paint("Paint bucket", "◍", "G", PaintKind::Bucket),
    ],
    &[item("Pen", "✒", "P", Tool::Pen)],
    &[item("Type", "T", "T", Tool::Type)],
    &[
        shape("Rectangle", "◇", "U", ShapeKind::Rect),
        shape("Ellipse", "○", "Shift+U", ShapeKind::Ellipse),
    ],
    &[item("Mask", "◐", "Q", Tool::Mask)],
    &[item("Grade", "◑", "Shift+Q", Tool::Grade)],
    &[item("Hand", "✋", "H", Tool::Hand)],
    &[item("Zoom", "⌕", "Z", Tool::Zoom)],
];

/// Thin lines after these `GROUPS` slots, Photoshop's clusters: move ·
/// selection · crop and sampling · retouch and paint · vector · Emulsion's
/// mask and grade · navigation.
pub const DIVIDERS: &[usize] = &[0, 3, 5, 10, 13, 15];

/// Draw mode: the painter's rail, in the order Procreate users reach for.
pub const DRAW_GROUPS: &[&[RailItem]] = &[
    &[
        paint("Brush", "✎", "B", PaintKind::Brush),
        paint("Liquify", "≈", "Shift+J", PaintKind::Liquify),
    ],
    &[paint("Smudge", "☁", "Shift+B", PaintKind::Smudge)],
    &[paint("Eraser", "◻", "E", PaintKind::Eraser)],
    &[item("Eyedropper", "◔", "I", Tool::Eyedropper)],
    &[
        paint("Paint bucket", "◍", "G", PaintKind::Bucket),
        paint("Gradient", "▤", "Shift+G", PaintKind::Gradient),
    ],
    &[
        select("Lasso", "〰", "L", SelectShape::Lasso),
        select("Rectangular marquee", "▭", "M", SelectShape::Rect),
        select("Quick select (AI)", "✦", "Shift+W", SelectShape::Quick),
    ],
    &[item("Move", "✥", "V", Tool::Move)],
    &[item("Mask", "◐", "Q", Tool::Mask)],
    &[item("Hand", "✋", "H", Tool::Hand)],
    &[item("Zoom", "⌕", "Z", Tool::Zoom)],
];

/// Dividers for `DRAW_GROUPS`: paint · fill · select and move · navigation.
pub const DRAW_DIVIDERS: &[usize] = &[3, 4, 7];

/// Rail button height; a little tighter than the old rail so eighteen
/// slots and the swatches fit a 720 px window.
const BTN_H: Pixels = px(32.);

#[derive(Default)]
pub struct RailState {
    /// Group whose fly-out is open.
    pub flyout: Option<usize>,
    /// Last-used member per group, so the slot shows what you picked.
    pick: HashMap<usize, usize>,
}

/// The rail's name for a tool, for the options bar heading.
pub fn tool_name(tool: Tool) -> &'static str {
    GROUPS
        .iter()
        .flat_map(|g| g.iter())
        .find(|i| i.tool == tool)
        .map(|i| match tool {
            Tool::Select => "Select",
            Tool::Brush => "Brush",
            Tool::Shape => "Shape",
            _ => i.name,
        })
        .unwrap_or("Move")
}

impl EditorView {
    /// The rail's slots for the current mode.
    fn rail_groups(&self) -> &'static [&'static [RailItem]] {
        if self.draw_mode { DRAW_GROUPS } else { GROUPS }
    }

    /// The selected subtype, used for contextual headings and tool help.
    pub(crate) fn active_tool_name(&self) -> &'static str {
        GROUPS
            .iter()
            .flat_map(|group| group.iter())
            .find(|item| self.rail_item_active(item))
            .map(|item| item.name)
            .unwrap_or_else(|| tool_name(self.tool))
    }

    /// Does the current tool state match this item?
    fn rail_item_active(&self, it: &RailItem) -> bool {
        if self.tool != it.tool {
            return false;
        }
        match (it.paint, it.select, it.shape) {
            (Some(k), _, _) => self.tools.paint == k,
            (_, Some(s), _) => self.tools.select == s,
            (_, _, Some(s)) => self.tools.shape == s,
            _ => true,
        }
    }

    /// Which member a group slot shows.
    fn rail_shown(&self, g: usize) -> usize {
        let group = self.rail_groups()[g];
        if let Some(i) = group.iter().position(|it| self.rail_item_active(it)) {
            return i;
        }
        self.rail
            .pick
            .get(&g)
            .copied()
            .unwrap_or(0)
            .min(group.len() - 1)
    }

    pub(crate) fn activate_rail_item(&mut self, g: usize, i: usize, cx: &mut Context<Self>) {
        let it = self.rail_groups()[g][i];
        self.rail.pick.insert(g, i);
        self.rail.flyout = None;
        match (it.paint, it.select, it.shape) {
            (Some(k), _, _) => self.set_paint(k, cx),
            (_, Some(s), _) => self.set_select(s, cx),
            (_, _, Some(s)) => {
                self.set_tool(Tool::Shape, cx);
                self.tools.shape = s;
            }
            _ => self.set_tool(it.tool, cx),
        }
        cx.notify();
    }

    pub(crate) fn tool_rail(
        &mut self,
        p: &Palette,
        cx: &mut Context<Self>,
    ) -> impl IntoElement + use<> {
        let (ink, paper, accent, panel, line) = (p.ink, p.paper, p.accent, p.panel, p.line);
        let accent_fg = p.accent_fg;
        let flyout = self.rail.flyout;
        let mut rail = div()
            .id("tool-rail")
            .tab_group()
            .aria_label("Tools")
            .flex()
            .flex_none()
            .flex_col()
            .items_center()
            .w(dim::TOOL_RAIL_W)
            .py(px(6.))
            .gap(px(1.))
            .border_r_1()
            .border_color(p.line)
            .on_mouse_down_out(cx.listener(|this, _, _, cx| {
                if this.rail.flyout.is_some() {
                    this.rail.flyout = None;
                    cx.notify();
                }
            }));
        let groups = self.rail_groups();
        for (g, group) in groups.iter().enumerate() {
            let shown = self.rail_shown(g);
            let it = group[shown];
            let on = self.rail_item_active(&it) || (self.tool == it.tool && group.len() == 1);
            let has_more = group.len() > 1;
            let tip_text = if it.key.is_empty() {
                format!("{} — {}", it.name, tool_help(it.tool))
            } else {
                format!("{} ({}) — {}", it.name, it.key, tool_help(it.tool))
            };
            let tip: SharedString = tip_text.into();
            let open = flyout == Some(g);
            let list = open.then(|| {
                div()
                    .id(("rail-flyout", g))
                    .absolute()
                    .left(dim::TOOL_BTN_W)
                    .top_0()
                    .flex()
                    .flex_col()
                    .min_w(px(200.))
                    .border_1()
                    .border_color(ink)
                    .bg(panel)
                    .text_color(ink)
                    .py(px(3.))
                    .font_family(MONO_FONT)
                    .text_size(px(11.))
                    .children(group.iter().enumerate().map(|(i, m)| {
                        let active = self.rail_item_active(m);
                        div()
                            .id(("rail-flyout-item", g * 16 + i))
                            .focusable()
                            .tab_index(0)
                            .aria_label(m.name)
                            .aria_keyshortcuts(m.key)
                            .aria_selected(active)
                            .focus(move |s| s.bg(accent).text_color(accent_fg))
                            .on_key_down(cx.listener(move |this, e: &KeyDownEvent, window, cx| {
                                if e.keystroke.key == "escape" {
                                    this.rail.flyout = None;
                                    window.focus(&this.canvas_focus, cx);
                                    cx.notify();
                                    cx.stop_propagation();
                                }
                            }))
                            .flex()
                            .items_center()
                            .gap(px(8.))
                            .px(px(10.))
                            .py(px(4.))
                            .cursor_pointer()
                            .when(active, |d| d.bg(ink).text_color(paper))
                            .hover(move |s| s.bg(accent).text_color(accent_fg))
                            .on_click(cx.listener(move |this, _, window, cx| {
                                this.activate_rail_item(g, i, cx);
                                window.focus(&this.canvas_focus, cx);
                                cx.stop_propagation();
                            }))
                            .child(div().w(px(16.)).text_size(px(13.)).child(m.glyph))
                            .child(div().flex_1().child(m.name))
                            .child(div().child(m.key))
                    }))
            });
            rail = rail.child(
                div()
                    .id(SharedString::from(it.name))
                    .focusable()
                    .tab_index(0)
                    .aria_label(it.name)
                    .aria_keyshortcuts(it.key)
                    .aria_selected(on)
                    .focus(move |s| s.border_color(accent))
                    .on_key_down(cx.listener(move |this, e: &KeyDownEvent, window, cx| {
                        match e.keystroke.key.as_str() {
                            "down" if has_more => {
                                this.rail.flyout = Some(g);
                                cx.notify();
                                cx.stop_propagation();
                            }
                            "escape" => {
                                this.rail.flyout = None;
                                window.focus(&this.canvas_focus, cx);
                                cx.notify();
                                cx.stop_propagation();
                            }
                            _ => {}
                        }
                    }))
                    .relative()
                    .flex()
                    .items_center()
                    .justify_center()
                    .w(dim::TOOL_BTN_W)
                    .h(BTN_H)
                    .border_1()
                    .border_color(if on { ink } else { transparent_black() })
                    .bg(if on { ink } else { transparent_black() })
                    .text_color(if on { paper } else { ink })
                    .font_family(MONO_FONT)
                    .text_size(px(14.))
                    .when(!on, |d| d.hover(move |s| s.border_color(ink)))
                    .cursor(CursorStyle::PointingHand)
                    .on_click(cx.listener(move |this, _, window, cx| {
                        // Clicking the tool you already hold opens its group,
                        // the way click-and-hold does in Photoshop.
                        if has_more && on {
                            this.rail.flyout = if this.rail.flyout == Some(g) {
                                None
                            } else {
                                Some(g)
                            };
                            cx.notify();
                        } else {
                            this.activate_rail_item(g, shown, cx);
                            window.focus(&this.canvas_focus, cx);
                        }
                    }))
                    .on_mouse_down(
                        MouseButton::Right,
                        cx.listener(move |this, _, _, cx| {
                            if has_more {
                                this.rail.flyout = if this.rail.flyout == Some(g) {
                                    None
                                } else {
                                    Some(g)
                                };
                                cx.notify();
                            }
                        }),
                    )
                    .tooltip(move |w, cx| {
                        gpui_kit::component::tooltip::Tooltip::new(tip.clone()).build(w, cx)
                    })
                    .child(it.glyph)
                    .when(has_more, |d| {
                        // Corner mark: this slot holds more tools (right-click).
                        d.child(
                            div()
                                .id(("rail-more", g))
                                .aria_label("More tools")
                                .absolute()
                                .right(px(0.))
                                .bottom(px(0.))
                                .flex()
                                .items_center()
                                .justify_center()
                                .w(px(16.))
                                .h(px(16.))
                                .text_size(px(8.))
                                .text_color(if on { paper } else { line })
                                .cursor_pointer()
                                .on_click(cx.listener(move |this, _, _, cx| {
                                    this.rail.flyout = if this.rail.flyout == Some(g) {
                                        None
                                    } else {
                                        Some(g)
                                    };
                                    cx.stop_propagation();
                                    cx.notify();
                                }))
                                .tooltip(|window, cx| {
                                    gpui_kit::component::tooltip::Tooltip::new(
                                        "More tools (right-click or focus the tool and press Down)",
                                    )
                                    .build(window, cx)
                                })
                                .child("◢"),
                        )
                    })
                    // Painted after the canvas, so the list is not covered.
                    .children(list.map(|l| deferred(l).with_priority(1)))
                    .test_support(),
            );
            let dividers = if self.draw_mode {
                DRAW_DIVIDERS
            } else {
                DIVIDERS
            };
            if dividers.contains(&g) && g + 1 < groups.len() {
                rail = rail.child(div().w(px(18.)).h(px(1.)).my(px(3.)).bg(line.opacity(0.9)));
            }
        }
        rail.child(div().flex_1()).child(self.swatches(p, cx))
    }
}

impl EditorView {
    /// Draw mode: size and opacity as tall sliders beside the canvas,
    /// where a painter's off hand finds them (Procreate's side bar).
    pub(crate) fn draw_side_sliders(
        &mut self,
        p: &Palette,
        cx: &mut Context<Self>,
    ) -> Option<AnyElement> {
        if !self.draw_mode {
            return None;
        }
        let brushy = matches!(
            self.tool,
            Tool::Brush | Tool::Heal | Tool::Clone | Tool::Mask
        ) && !matches!(self.tools.paint, PaintKind::Bucket | PaintKind::Gradient);
        if !brushy {
            return None;
        }
        let b = self.tools.brush;
        let size_track = self.tracks.entry(SliderKey::SideSize).or_default().clone();
        let op_track = self
            .tracks
            .entry(SliderKey::SideOpacity)
            .or_default()
            .clone();
        let size_norm = ((b.size - 1.0) / 499.0).clamp(0.0, 1.0).sqrt();
        let column = |label: &'static str, value: String, el: AnyElement| {
            div()
                .flex()
                .flex_col()
                .items_center()
                .gap(px(4.))
                .h(px(170.))
                .child(div().flex_1().min_h_0().child(el))
                .child(mono(value, 9.5, p.ink))
                .child(mono(label, 9., p.muted))
        };
        Some(
            div()
                .flex()
                .flex_none()
                .flex_col()
                .justify_center()
                .gap(px(18.))
                .w(px(40.))
                .py(px(12.))
                .border_r_1()
                .border_color(p.line)
                .child(column(
                    "size",
                    format!("{:.0}", b.size),
                    crate::widgets::vslider(
                        "side-size",
                        size_norm,
                        size_track,
                        p,
                        cx.listener(|this, e: &MouseDownEvent, _, cx| {
                            this.vslider_down(SliderKey::SideSize, (1.0, 500.0, 1.0), e, cx)
                        }),
                    )
                    .tab_index(0)
                    .key_context("Slider")
                    .role(Role::Slider)
                    .aria_label("Brush size")
                    .aria_value(format!("{:.0} pixels", b.size))
                    .aria_orientation(Orientation::Vertical)
                    .focus_visible(|s| s.bg(p.accent.opacity(0.2)))
                    .on_key_down(cx.listener(move |this, e, _, cx| {
                        this.slider_key(SliderKey::SideSize, size_norm, (1., 500., 1.), e, cx);
                    }))
                    .into_any_element(),
                ))
                .child(column(
                    "opacity",
                    format!("{:.0}%", b.opacity * 100.0),
                    crate::widgets::vslider(
                        "side-opacity",
                        b.opacity,
                        op_track,
                        p,
                        cx.listener(|this, e: &MouseDownEvent, _, cx| {
                            this.vslider_down(SliderKey::SideOpacity, (1.0, 100.0, 1.0), e, cx)
                        }),
                    )
                    .tab_index(0)
                    .key_context("Slider")
                    .role(Role::Slider)
                    .aria_label("Brush opacity")
                    .aria_value(format!("{:.0} percent", b.opacity * 100.))
                    .aria_orientation(Orientation::Vertical)
                    .focus_visible(|s| s.bg(p.accent.opacity(0.2)))
                    .on_key_down(cx.listener(move |this, e, _, cx| {
                        this.slider_key(SliderKey::SideOpacity, b.opacity, (1., 100., 1.), e, cx);
                    }))
                    .into_any_element(),
                ))
                .into_any_element(),
        )
    }
}
