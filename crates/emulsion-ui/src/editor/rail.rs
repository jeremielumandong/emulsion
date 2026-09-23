//! The left tool rail, arranged the way Photoshop and GIMP users expect:
//! Move · Marquee · Lasso · Quick select · Crop · Eyedropper · Heal ·
//! Brush · Clone · Eraser · Gradient · Smudge · Pen · Type · Shape · Mask ·
//! Grade · Hand · Zoom, with the foreground/background swatches at the
//! bottom, matching Photoshop's single-column Tools panel.
//! Related tools share one slot; the slot shows the member last used, and
//! a right-click (or the corner mark) opens a fly-out with the others.

use super::*;
use std::collections::HashMap;

/// One tool the rail can activate: a tool, plus the sub-mode it selects.
#[derive(Clone, Copy)]
pub struct RailItem {
    pub name: &'static str,
    /// Icon: a Lucide name (`icons/<name>.svg` in the UI kit's assets) or
    /// one of the `emulsion-*` drawings below. Photoshop's silhouettes, so
    /// the rail reads at a glance.
    pub glyph: &'static str,
    /// Default shortcut, for the tooltip and fly-out.
    pub key: &'static str,
    pub tool: Tool,
    pub paint: Option<PaintKind>,
    pub select: Option<SelectShape>,
    pub shape: Option<ShapeKind>,
    pub pen: Option<PenMode>,
    pub rotate_view: bool,
    pub vertical_type: bool,
    pub remove: bool,
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
        pen: None,
        rotate_view: false,
        vertical_type: false,
        remove: false,
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

const fn pen(name: &'static str, glyph: &'static str, mode: PenMode) -> RailItem {
    RailItem {
        pen: Some(mode),
        ..item(
            name,
            glyph,
            match mode {
                PenMode::Pen => "P",
                PenMode::Free | PenMode::Curvature => "Shift+P",
                _ => "",
            },
            Tool::Pen,
        )
    }
}

/// Rail slots top to bottom; each is a group of one or more items.
pub const GROUPS: &[&[RailItem]] = &[
    &[item("Move", "move", "V", Tool::Move)],
    &[
        select(
            "Rectangular marquee",
            "square-dashed",
            "M",
            SelectShape::Rect,
        ),
        select(
            "Elliptical marquee",
            "circle-dashed",
            "Shift+M",
            SelectShape::Ellipse,
        ),
    ],
    &[
        select("Lasso", "lasso", "L", SelectShape::Lasso),
        select(
            "Polygonal lasso",
            "lasso-select",
            "Shift+L",
            SelectShape::Polygon,
        ),
        select("Magnetic lasso", "magnet", "Alt+L", SelectShape::Magnetic),
    ],
    &[
        select(
            "Quick select (AI)",
            "wand-sparkles",
            "Shift+W",
            SelectShape::Quick,
        ),
        select("Magic wand", "wand", "W", SelectShape::Wand),
    ],
    &[item("Crop", "crop", "C", Tool::Crop)],
    &[item("Eyedropper", "pipette", "I", Tool::Eyedropper)],
    &[
        item("Heal", "bandage", "J", Tool::Heal),
        RailItem {
            remove: true,
            ..item("Remove", "bandage", "Shift+J", Tool::Heal)
        },
    ],
    &[paint("Brush", "brush", "B", PaintKind::Brush)],
    &[item("Clone stamp", "stamp", "S", Tool::Clone)],
    &[paint("Eraser", "eraser", "E", PaintKind::Eraser)],
    &[
        paint("Gradient", "emulsion-gradient", "G", PaintKind::Gradient),
        paint("Paint bucket", "paint-bucket", "Shift+G", PaintKind::Bucket),
    ],
    // Photoshop's Blur / Sharpen / Smudge slot.
    &[
        paint("Smudge", "pointer", "Shift+B", PaintKind::Smudge),
        paint(
            "Liquify",
            "emulsion-liquify",
            "Ctrl+Shift+X",
            PaintKind::Liquify,
        ),
    ],
    &[
        pen("Pen", "pen-tool", PenMode::Pen),
        pen("Free Pen", "pencil", PenMode::Free),
        pen("Curvature Pen", "spline", PenMode::Curvature),
        pen("Add Anchor Point", "plus", PenMode::AddAnchor),
        pen("Delete Anchor Point", "minus", PenMode::DeleteAnchor),
        pen("Convert Point", "corner-down-right", PenMode::ConvertPoint),
    ],
    &[
        item("Type Tool", "type", "T", Tool::Type),
        RailItem {
            vertical_type: true,
            ..item(
                "Vertical Type Tool",
                "emulsion-vertical-type",
                "Shift+T",
                Tool::Type,
            )
        },
    ],
    &[
        shape("Rectangle", "square", "U", ShapeKind::Rect),
        shape("Ellipse", "circle", "Shift+U", ShapeKind::Ellipse),
    ],
    &[item("Mask", "emulsion-mask", "", Tool::Mask)],
    &[item("Grade", "contrast", "Shift+Q", Tool::Grade)],
    &[
        item("Hand", "hand", "H", Tool::Hand),
        RailItem {
            rotate_view: true,
            ..item("Rotate View", "rotate-cw", "R", Tool::Hand)
        },
    ],
    &[item("Zoom", "zoom-in", "Z", Tool::Zoom)],
];

/// Small gaps after these `GROUPS` slots, Photoshop's clusters: move ·
/// selection · crop and sampling · retouch and paint · vector · Emulsion's
/// mask and grade · navigation.
pub const DIVIDERS: &[usize] = &[0, 3, 5, 11, 14, 16];

/// Draw mode: the painter's rail, in the order Procreate users reach for.
pub const DRAW_GROUPS: &[&[RailItem]] = &[
    &[
        paint("Brush", "brush", "B", PaintKind::Brush),
        paint(
            "Liquify",
            "emulsion-liquify",
            "Ctrl+Shift+X",
            PaintKind::Liquify,
        ),
    ],
    &[paint("Smudge", "pointer", "Shift+B", PaintKind::Smudge)],
    &[paint("Eraser", "eraser", "E", PaintKind::Eraser)],
    &[item("Eyedropper", "pipette", "I", Tool::Eyedropper)],
    &[
        paint("Paint bucket", "paint-bucket", "Shift+G", PaintKind::Bucket),
        paint("Gradient", "emulsion-gradient", "G", PaintKind::Gradient),
    ],
    &[
        select(
            "Rectangular marquee",
            "square-dashed",
            "M",
            SelectShape::Rect,
        ),
        select("Lasso", "lasso", "L", SelectShape::Lasso),
        select(
            "Quick select (AI)",
            "wand-sparkles",
            "Shift+W",
            SelectShape::Quick,
        ),
    ],
    &[item("Move", "move", "V", Tool::Move)],
    &[item("Mask", "emulsion-mask", "", Tool::Mask)],
    &[
        item("Hand", "hand", "H", Tool::Hand),
        RailItem {
            rotate_view: true,
            ..item("Rotate View", "rotate-cw", "R", Tool::Hand)
        },
    ],
    &[item("Zoom", "zoom-in", "Z", Tool::Zoom)],
];

/// Group spacing for `DRAW_GROUPS`: paint · fill · select and move · navigation.
pub const DRAW_DIVIDERS: &[usize] = &[3, 4, 7];

/// Tools Lucide has no icon for, drawn in its 24-grid, 2 px stroke style.
const GRADIENT_SVG: &str = r##"<svg xmlns="http://www.w3.org/2000/svg" viewBox="0 0 24 24"><defs><linearGradient id="g" x1="0" y1="0" x2="1" y2="0"><stop offset="0" stop-color="currentColor"/><stop offset="1" stop-color="currentColor" stop-opacity="0.05"/></linearGradient></defs><rect x="3" y="4" width="18" height="16" rx="1" fill="url(#g)"/><rect x="3" y="4" width="18" height="16" rx="1" fill="none" stroke="currentColor" stroke-width="2"/></svg>"##;
const MASK_SVG: &str = r##"<svg xmlns="http://www.w3.org/2000/svg" viewBox="0 0 24 24"><rect x="3" y="4" width="18" height="16" rx="1" fill="none" stroke="currentColor" stroke-width="2"/><circle cx="12" cy="12" r="4.5" fill="currentColor"/></svg>"##;
const VERTICAL_TYPE_SVG: &str = r##"<svg xmlns="http://www.w3.org/2000/svg" viewBox="0 0 24 24" fill="none" stroke="currentColor" stroke-width="2" stroke-linecap="round"><path d="M9 4h12M15 4v16M12 20h6M4 4v16M2 17l2 3 2-3"/></svg>"##;
const LIQUIFY_SVG: &str = r##"<svg xmlns="http://www.w3.org/2000/svg" viewBox="0 0 24 24" fill="none" stroke="currentColor" stroke-width="2" stroke-linecap="round"><path d="M3 8c2.5-3 5-3 7.5 0s5 3 7.5 0 3-3 3-3"/><path d="M3 14c2.5-3 5-3 7.5 0s5 3 7.5 0 3-3 3-3"/><path d="M3 20c2.5-3 5-3 7.5 0s5 3 7.5 0 3-3 3-3"/></svg>"##;

/// The bytes of a rail icon: Lucide's file, embedded from the vendored UI
/// kit at build time, or one of the drawings above.
fn icon_bytes(id: &str) -> &'static [u8] {
    match id {
        "rotate-cw" => {
            include_bytes!("../../../../vendor/gpui/gpui-kit-assets/assets/icons/rotate-cw.svg")
        }
        "pencil" => {
            include_bytes!("../../../../vendor/gpui/gpui-kit-assets/assets/icons/pencil.svg")
        }
        "spline" => {
            include_bytes!("../../../../vendor/gpui/gpui-kit-assets/assets/icons/spline.svg")
        }
        "plus" => include_bytes!("../../../../vendor/gpui/gpui-kit-assets/assets/icons/plus.svg"),
        "minus" => include_bytes!("../../../../vendor/gpui/gpui-kit-assets/assets/icons/minus.svg"),
        "corner-down-right" => include_bytes!(
            "../../../../vendor/gpui/gpui-kit-assets/assets/icons/corner-down-right.svg"
        ),
        "move" => include_bytes!("../../../../vendor/gpui/gpui-kit-assets/assets/icons/move.svg"),
        "square-dashed" => {
            include_bytes!("../../../../vendor/gpui/gpui-kit-assets/assets/icons/square-dashed.svg")
        }
        "circle-dashed" => {
            include_bytes!("../../../../vendor/gpui/gpui-kit-assets/assets/icons/circle-dashed.svg")
        }
        "lasso" => include_bytes!("../../../../vendor/gpui/gpui-kit-assets/assets/icons/lasso.svg"),
        "lasso-select" => {
            include_bytes!("../../../../vendor/gpui/gpui-kit-assets/assets/icons/lasso-select.svg")
        }
        "magnet" => {
            include_bytes!("../../../../vendor/gpui/gpui-kit-assets/assets/icons/magnet.svg")
        }
        "wand-sparkles" => {
            include_bytes!("../../../../vendor/gpui/gpui-kit-assets/assets/icons/wand-sparkles.svg")
        }
        "wand" => include_bytes!("../../../../vendor/gpui/gpui-kit-assets/assets/icons/wand.svg"),
        "crop" => include_bytes!("../../../../vendor/gpui/gpui-kit-assets/assets/icons/crop.svg"),
        "pipette" => {
            include_bytes!("../../../../vendor/gpui/gpui-kit-assets/assets/icons/pipette.svg")
        }
        "bandage" => {
            include_bytes!("../../../../vendor/gpui/gpui-kit-assets/assets/icons/bandage.svg")
        }
        "brush" => include_bytes!("../../../../vendor/gpui/gpui-kit-assets/assets/icons/brush.svg"),
        "pointer" => {
            include_bytes!("../../../../vendor/gpui/gpui-kit-assets/assets/icons/pointer.svg")
        }
        "stamp" => include_bytes!("../../../../vendor/gpui/gpui-kit-assets/assets/icons/stamp.svg"),
        "eraser" => {
            include_bytes!("../../../../vendor/gpui/gpui-kit-assets/assets/icons/eraser.svg")
        }
        "paint-bucket" => {
            include_bytes!("../../../../vendor/gpui/gpui-kit-assets/assets/icons/paint-bucket.svg")
        }
        "pen-tool" => {
            include_bytes!("../../../../vendor/gpui/gpui-kit-assets/assets/icons/pen-tool.svg")
        }
        "type" => include_bytes!("../../../../vendor/gpui/gpui-kit-assets/assets/icons/type.svg"),
        "square" => {
            include_bytes!("../../../../vendor/gpui/gpui-kit-assets/assets/icons/square.svg")
        }
        "circle" => {
            include_bytes!("../../../../vendor/gpui/gpui-kit-assets/assets/icons/circle.svg")
        }
        "contrast" => {
            include_bytes!("../../../../vendor/gpui/gpui-kit-assets/assets/icons/contrast.svg")
        }
        "hand" => include_bytes!("../../../../vendor/gpui/gpui-kit-assets/assets/icons/hand.svg"),
        "zoom-in" => {
            include_bytes!("../../../../vendor/gpui/gpui-kit-assets/assets/icons/zoom-in.svg")
        }
        "emulsion-gradient" => GRADIENT_SVG.as_bytes(),
        "emulsion-mask" => MASK_SVG.as_bytes(),
        "emulsion-vertical-type" => VERTICAL_TYPE_SVG.as_bytes(),
        "emulsion-liquify" => LIQUIFY_SVG.as_bytes(),
        "layers" => {
            include_bytes!("../../../../vendor/gpui/gpui-kit-assets/assets/icons/layers.svg")
        }
        "undo-2" => {
            include_bytes!("../../../../vendor/gpui/gpui-kit-assets/assets/icons/undo-2.svg")
        }
        "redo-2" => {
            include_bytes!("../../../../vendor/gpui/gpui-kit-assets/assets/icons/redo-2.svg")
        }
        "star" => {
            include_bytes!("../../../../vendor/gpui/gpui-kit-assets/assets/icons/star.svg")
        }
        "star-fill" => {
            include_bytes!("../../../../vendor/gpui/gpui-kit-assets/assets/icons/star-fill.svg")
        }
        "library" => {
            include_bytes!("../../../../vendor/gpui/gpui-kit-assets/assets/icons/library.svg")
        }
        "link" => include_bytes!("../../../../vendor/gpui/gpui-kit-assets/assets/icons/link.svg"),
        "file-plus" => {
            include_bytes!("../../../../vendor/gpui/gpui-kit-assets/assets/icons/file-plus.svg")
        }
        _ => include_bytes!(
            "../../../../vendor/gpui/gpui-kit-assets/assets/icons/circle-question-mark.svg"
        ),
    }
}

/// The SVG element for a rail icon id.
pub(crate) fn tool_icon(id: &'static str) -> Svg {
    svg().data(icon_bytes(id))
}

#[cfg(test)]
mod icon_tests {
    #[test]
    fn every_rail_icon_is_a_real_drawing() {
        for group in super::GROUPS.iter().chain(super::DRAW_GROUPS.iter()) {
            for it in group.iter() {
                let bytes = super::icon_bytes(it.glyph);
                assert!(
                    std::str::from_utf8(bytes).is_ok_and(|s| s.contains("<svg")),
                    "{} has no icon",
                    it.name
                );
                assert_ne!(
                    bytes,
                    super::icon_bytes("nothing-like-this"),
                    "{} falls back to the placeholder",
                    it.name
                );
            }
        }
    }
}

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
    pub(super) fn rail_item_active(&self, it: &RailItem) -> bool {
        if self.tool != it.tool {
            return false;
        }
        if it.tool == Tool::Hand {
            return self.tools.rotate_view == it.rotate_view;
        }
        if it.tool == Tool::Type {
            return self.type_tool.spec.vertical == it.vertical_type;
        }
        if it.tool == Tool::Heal {
            return self.tools.remove.enabled == it.remove;
        }
        if let Some(mode) = it.pen {
            return self.tools.pen.mode == mode;
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
        self.activate_tool_item(it, cx);
    }

    pub(super) fn activate_tool_item(&mut self, it: RailItem, cx: &mut Context<Self>) {
        self.rail.flyout = None;
        if it.tool == Tool::Heal {
            self.set_remove_mode(it.remove, cx);
            return;
        }
        if it.tool == Tool::Hand {
            self.set_hand_mode(it.rotate_view, cx);
            return;
        }
        if it.tool == Tool::Type {
            self.set_type_mode(it.vertical_type, cx);
            return;
        }
        if let Some(mode) = it.pen {
            self.set_pen_mode(mode, cx);
            return;
        }
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
        self.render_tool_rail(None, p, cx)
    }

    /// Compact tools wrap within the available length, expressed in rem units.
    pub(crate) fn compact_tool_rail(
        &mut self,
        horizontal: bool,
        available_length: f32,
        p: &Palette,
        cx: &mut Context<Self>,
    ) -> impl IntoElement + use<> {
        self.render_tool_rail(Some((horizontal, available_length)), p, cx)
    }

    fn render_tool_rail(
        &mut self,
        compact_layout: Option<(bool, f32)>,
        p: &Palette,
        cx: &mut Context<Self>,
    ) -> impl IntoElement + use<> {
        let compact = compact_layout.is_some();
        let horizontal = compact_layout.is_some_and(|(horizontal, _)| horizontal);
        let (ink, accent, panel, line) = (p.ink, p.accent, p.panel, p.line);
        let selected_bg = ink.opacity(0.10);
        let hover_bg = ink.opacity(0.06);
        let accent_fg = p.accent_fg;
        let flyout = self.rail.flyout;
        let rail = div()
            .id("tool-rail")
            .tab_group()
            .aria_label("Tools")
            .flex()
            .flex_none()
            .flex_col()
            .min_h_0()
            .items_center()
            .when(!compact, |d| d.w(dim::TOOL_RAIL_W).border_r_1())
            .border_color(p.line);
        // Keep every tool at its normal target size on short windows. Flyouts
        // are deferred, so they paint outside this scrolling content mask.
        let mut tools = div()
            .id("tool-rail-scroll")
            .flex()
            .min_h_0()
            .items_center()
            .when(!compact, |d| {
                d.flex_col()
                    .flex_1()
                    .w_full()
                    .py(px(8.))
                    .gap(px(2.))
                    .overflow_y_scroll()
            });
        let groups = self.rail_groups();
        if let Some((horizontal, available_length)) = compact_layout {
            let mut slots =
                (((available_length + 0.125) / 1.875).floor() as usize).clamp(1, groups.len());
            if self.compact.tool_columns >= 2 {
                slots = slots.min(groups.len().div_ceil(2));
            }
            let tracks = groups.len().div_ceil(slots);
            let length = rems(slots as f32 * 1.875 - 0.125);
            let breadth = rems(tracks as f32 * 1.875 - 0.125);
            tools = tools
                .flex_none()
                .flex_wrap()
                .gap(rems(0.125))
                .when(horizontal, |d| d.flex_row().w(length).h(breadth))
                .when(!horizontal, |d| d.flex_col().h(length).w(breadth));
        }
        for (g, group) in groups.iter().enumerate() {
            let shown = self.rail_shown(g);
            let it = group[shown];
            let on = self.rail_item_active(&it) || (self.tool == it.tool && group.len() == 1);
            let has_more = group.len() > 1;
            let tool_label = match it.select {
                Some(SelectShape::Rect) => "Rectangle selection (Rectangular marquee)",
                Some(SelectShape::Ellipse) => "Ellipse selection (Elliptical marquee)",
                _ => it.name,
            };
            let help = if it.rotate_view {
                "Drag to rotate the view. Shift snaps to 15°; Reset view restores the angle."
            } else {
                it.pen
                    .map(PenMode::help)
                    .unwrap_or_else(|| tool_help(it.tool))
            };
            let tip_text = if it.key.is_empty() {
                format!("{} — {}", tool_label, help)
            } else {
                format!("{} ({}) — {}", tool_label, it.key, help)
            };
            let tip: SharedString = tip_text.into();
            let open = flyout == Some(g);
            let trigger_bounds = std::rc::Rc::new(std::cell::Cell::new(None::<Bounds<Pixels>>));
            let dismiss_trigger = trigger_bounds.clone();
            let list = open.then(|| {
                div()
                    .id(("rail-flyout", g))
                    .test_support()
                    // Dismiss against the menu's bounds, not the narrow rail:
                    // menu items deliberately extend onto the canvas.
                    .on_mouse_down_out(cx.listener(move |this, event: &MouseDownEvent, _, cx| {
                        // Let the trigger's own click toggle the menu, rather
                        // than dismissing on down and reopening on click.
                        if !dismiss_trigger
                            .get()
                            .is_some_and(|bounds| bounds.contains(&event.position))
                        {
                            this.rail.flyout = None;
                            cx.notify();
                        }
                    }))
                    .when(!compact, |d| d.absolute().left(dim::TOOL_BTN_W).top_0())
                    .flex()
                    .flex_col()
                    .min_w(px(200.))
                    .border_1()
                    .border_color(line)
                    .rounded_md()
                    .bg(panel)
                    .text_color(ink)
                    .py(px(3.))
                    .font_family(MONO_FONT)
                    .text_size(px(11.))
                    .children(group.iter().enumerate().map(|(i, m)| {
                        let active = self.rail_item_active(m);
                        div()
                            .id(("rail-flyout-item", g * 16 + i))
                            .test_support()
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
                            .when(active, |d| d.bg(selected_bg))
                            .hover(move |s| s.bg(accent).text_color(accent_fg))
                            .on_click(cx.listener(move |this, _, window, cx| {
                                this.activate_rail_item(g, i, cx);
                                window.focus(&this.canvas_focus, cx);
                                cx.stop_propagation();
                            }))
                            .child(
                                div()
                                    .w(px(16.))
                                    .flex()
                                    .items_center()
                                    .child(tool_icon(m.glyph).size(px(14.)).text_color(ink)),
                            )
                            .child(div().flex_1().child(m.name))
                            .child(div().child(m.key))
                    }))
            });
            tools = tools.child(
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
                    .when(compact, |d| d.size(rems(1.75)))
                    .when(!compact, |d| d.w(dim::TOOL_BTN_W).h(BTN_H))
                    .flex_none()
                    .border_1()
                    // The active tool in the accent, as the nav's active
                    // tab: unmistakable in any theme.
                    .border_color(if on { accent } else { transparent_black() })
                    .rounded_md()
                    .bg(if on { accent } else { transparent_black() })
                    .text_color(ink)
                    .font_family(MONO_FONT)
                    .text_size(px(14.))
                    .when(!on, |d| d.hover(move |s| s.bg(hover_bg)))
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
                    .child(
                        tool_icon(it.glyph)
                            .when(compact, |icon| icon.size(rems(1.0625)))
                            .when(!compact, |icon| icon.size(px(18.)))
                            .text_color(if on { accent_fg } else { ink }),
                    )
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
                                .when(compact, |d| d.size(rems(0.75)).text_size(rems(0.375)))
                                .text_color(ink.opacity(0.65))
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
                    .children(open.then(|| {
                        canvas(
                            move |bounds, _, _| trigger_bounds.set(Some(bounds)),
                            |_, _, _, _| {},
                        )
                        .absolute()
                        .size_full()
                    }))
                    // Painted after the canvas, so the list is not covered.
                    .children(list.map(|list| {
                        let list = if compact {
                            div()
                                .absolute()
                                .when(horizontal, |d| d.left_0().top(rems(1.75)))
                                .when(!horizontal, |d| d.left(rems(1.75)).top_0())
                                .child(anchored().snap_to_window().child(list))
                                .into_any_element()
                        } else {
                            list.into_any_element()
                        };
                        deferred(list).with_priority(1)
                    }))
                    .test_support(),
            );
            let dividers = if self.draw_mode {
                DRAW_DIVIDERS
            } else {
                DIVIDERS
            };
            if !compact && dividers.contains(&g) && g + 1 < groups.len() {
                tools = tools.child(div().h(px(3.)).flex_none());
            }
        }
        rail.child(tools.test_support())
            .when(!compact, |d| {
                d.child(
                    div()
                        .id("tool-rail-swatches")
                        .flex_none()
                        .flex()
                        .flex_col()
                        .items_center()
                        .gap_1()
                        .py(px(8.))
                        .child(self.swatches(p, cx))
                        .child(self.quick_mask_button(p, cx))
                        .test_support(),
                )
            })
            .test_support()
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
        let [size, opacity] = self.brush_vsliders(170., p, cx)?;
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
                .child(size)
                .child(opacity)
                .into_any_element(),
        )
    }

    /// Tall size and opacity sliders for the active brush, or `None` when
    /// the current tool paints no brush strokes.
    pub(crate) fn brush_vsliders(
        &mut self,
        height: f32,
        p: &Palette,
        cx: &mut Context<Self>,
    ) -> Option<[AnyElement; 2]> {
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
                .h(px(height))
                .child(div().flex_1().min_h_0().child(el))
                .child(mono(value, 9.5, p.ink))
                .child(mono(label, 9., p.muted))
        };
        Some([
            column(
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
            )
            .into_any_element(),
            column(
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
            )
            .into_any_element(),
        ])
    }
}
