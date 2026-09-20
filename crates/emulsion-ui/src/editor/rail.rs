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
        select("Elliptical marquee", "◯", "M", SelectShape::Ellipse),
    ],
    &[
        select("Lasso", "〰", "L", SelectShape::Lasso),
        select("Polygonal lasso", "⬠", "L", SelectShape::Polygon),
        select("Magnetic lasso", "⌇", "L", SelectShape::Magnetic),
    ],
    &[
        select("Quick select (AI)", "✦", "W", SelectShape::Quick),
        select("Magic wand", "⚚", "W", SelectShape::Wand),
    ],
    &[item("Crop", "⌗", "C", Tool::Crop)],
    &[item("Eyedropper", "◔", "I", Tool::Eyedropper)],
    &[item("Heal", "✚", "J", Tool::Heal)],
    &[
        paint("Brush", "✎", "B", PaintKind::Brush),
        paint("Smudge", "☁", "B", PaintKind::Smudge),
        paint("Liquify", "≈", "B", PaintKind::Liquify),
    ],
    &[item("Clone stamp", "◎", "S", Tool::Clone)],
    &[paint("Eraser", "◻", "E", PaintKind::Eraser)],
    &[
        paint("Gradient", "▤", "G", PaintKind::Gradient),
        paint("Paint bucket", "◍", "G", PaintKind::Bucket),
    ],
    &[item("Pen", "✒", "P", Tool::Pen)],
    &[item("Type", "T", "T", Tool::Type)],
    &[
        shape("Rectangle", "◇", "U", ShapeKind::Rect),
        shape("Ellipse", "○", "U", ShapeKind::Ellipse),
    ],
    &[item("Mask", "◐", "", Tool::Mask)],
    &[item("Grade", "◑", "", Tool::Grade)],
    &[item("Hand", "✋", "H", Tool::Hand)],
    &[item("Zoom", "⌕", "Z", Tool::Zoom)],
];

/// Draw mode: the painter's rail, in the order Procreate users reach for.
pub const DRAW_GROUPS: &[&[RailItem]] = &[
    &[
        paint("Brush", "✎", "B", PaintKind::Brush),
        paint("Liquify", "≈", "B", PaintKind::Liquify),
    ],
    &[paint("Smudge", "☁", "B", PaintKind::Smudge)],
    &[paint("Eraser", "◻", "E", PaintKind::Eraser)],
    &[item("Eyedropper", "◔", "I", Tool::Eyedropper)],
    &[
        paint("Paint bucket", "◍", "G", PaintKind::Bucket),
        paint("Gradient", "▤", "G", PaintKind::Gradient),
    ],
    &[
        select("Lasso", "〰", "L", SelectShape::Lasso),
        select("Rectangular marquee", "▭", "M", SelectShape::Rect),
        select("Quick select (AI)", "✦", "W", SelectShape::Quick),
    ],
    &[item("Move", "✥", "V", Tool::Move)],
    &[item("Mask", "◐", "", Tool::Mask)],
    &[item("Hand", "✋", "H", Tool::Hand)],
    &[item("Zoom", "⌕", "Z", Tool::Zoom)],
];

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
        let (ink, paper, accent, panel, line, muted) =
            (p.ink, p.paper, p.accent, p.panel, p.line, p.muted);
        let flyout = self.rail.flyout;
        let mut rail = div()
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
                    .py(px(3.))
                    .font_family(MONO_FONT)
                    .text_size(px(11.))
                    .children(group.iter().enumerate().map(|(i, m)| {
                        let active = self.rail_item_active(m);
                        div()
                            .id(("rail-flyout-item", g * 16 + i))
                            .flex()
                            .items_center()
                            .gap(px(8.))
                            .px(px(10.))
                            .py(px(4.))
                            .cursor_pointer()
                            .when(active, |d| d.bg(ink).text_color(paper))
                            .hover(move |s| s.bg(accent).text_color(paper))
                            .on_click(cx.listener(move |this, _, _, cx| {
                                this.activate_rail_item(g, i, cx);
                            }))
                            .child(div().w(px(16.)).text_size(px(13.)).child(m.glyph))
                            .child(div().flex_1().child(m.name))
                            .child(div().text_color(muted).child(m.key))
                    }))
            });
            rail = rail.child(
                div()
                    .id(SharedString::from(it.name))
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
                    .on_click(cx.listener(move |this, _, _, cx| {
                        this.activate_rail_item(g, shown, cx);
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
                                .absolute()
                                .right(px(1.))
                                .bottom(px(0.))
                                .text_size(px(7.))
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
                                .child("◢"),
                        )
                    })
                    .children(list)
                    .test_support(),
            );
        }
        rail.child(div().flex_1()).child(self.swatches(p, cx))
    }
}
