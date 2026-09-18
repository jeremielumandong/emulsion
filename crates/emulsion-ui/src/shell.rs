//! Editor shell: top bar, tool rail, stage with context bar and suggestion
//! strip, node panel. Content is hard-coded until Phase 1 brings the document
//! model.

use crate::theme::{self, MONO_FONT, Palette, UI_FONT, dim};
use gpui_kit::*;

const TOOLS: &[(&str, &str)] = &[
    ("move", "✥"),
    ("select", "▢"),
    ("mask", "◐"),
    ("brush", "✎"),
    ("heal", "✚"),
    ("clone", "◎"),
    ("grade", "◑"),
    ("type", "T"),
    ("crop", "⌗"),
    ("shape", "◇"),
];

const NODES: &[(&str, &str, u32)] = &[
    ("Base · hero_shot.raw", "raw", 0xB4B1A9),
    ("Relight subject", "ai", 0xE2DDD2),
    ("Sky replace", "ai", 0x9FB6C4),
    ("Filmic grade", "lut", 0xC9A27A),
    ("Grain 35mm", "fx", 0x8E8B84),
    ("Headline set", "txt", theme::ACCENT),
];

const SUGGESTIONS: &[(&str, &str)] = &[
    ("⌥1", "Lift the shadows 0.3"),
    ("⌥2", "Straighten horizon 1.4°"),
    ("⌥3", "Remove sensor dust ×7"),
    ("⌥4", "Match grade to v3"),
];

pub struct EditorShell {
    active_tool: usize,
    selected_node: usize,
}

impl EditorShell {
    pub fn new(_cx: &mut Context<Self>) -> Self {
        Self {
            active_tool: 1,
            selected_node: 2,
        }
    }

    fn mono(text: impl Into<SharedString>, size: f32, color: Hsla) -> Div {
        div()
            .font_family(MONO_FONT)
            .text_size(px(size))
            .text_color(color)
            .child(text.into())
    }

    fn top_bar(&self, p: &Palette) -> Div {
        div()
            .flex()
            .flex_none()
            .items_stretch()
            .h(dim::TOP_BAR_H)
            .bg(p.chrome)
            .text_color(p.chrome_fg)
            .child(
                div()
                    .flex()
                    .items_center()
                    .gap(px(11.))
                    .px(px(18.))
                    .child(div().size(px(14.)).bg(p.accent))
                    .child(
                        div()
                            .text_size(px(16.))
                            .font_weight(FontWeight::SEMIBOLD)
                            .child("Emulsion"),
                    ),
            )
            .child(div().flex_1())
            .child(
                div()
                    .flex()
                    .items_center()
                    .px(px(14.))
                    .border_l_1()
                    .border_color(p.chrome_line)
                    .child(Self::mono("light", 11., p.chrome_fg)),
            )
    }

    fn doc_bar(&self, p: &Palette) -> Div {
        div()
            .flex()
            .flex_none()
            .items_center()
            .gap(px(13.))
            .px(px(16.))
            .py(px(10.))
            .border_b_1()
            .border_color(p.line)
            .child(
                div()
                    .flex()
                    .items_baseline()
                    .gap(px(9.))
                    .child(
                        div()
                            .text_size(px(15.))
                            .font_weight(FontWeight::SEMIBOLD)
                            .child("campaign_hero"),
                    )
                    .child(Self::mono("4096×2560 · 16 bit", 10.5, p.muted)),
            )
            .child(
                div()
                    .flex()
                    .items_center()
                    .gap(px(7.))
                    .px(px(9.))
                    .py(px(4.))
                    .border_1()
                    .border_color(p.ink)
                    .bg(p.panel)
                    .child(div().size(px(6.)).rounded_full().bg(p.accent))
                    .child(Self::mono("branch / warm-grade", 10., p.ink))
                    .child(Self::mono("3 ahead", 10., p.muted)),
            )
            .child(div().flex_1())
            .child(
                div()
                    .px(px(16.))
                    .py(px(8.))
                    .bg(p.ink)
                    .text_color(p.paper)
                    .text_size(px(12.5))
                    .font_weight(FontWeight::MEDIUM)
                    .child("Export"),
            )
    }

    fn tool_rail(&self, p: &Palette) -> Div {
        let active = self.active_tool;
        div()
            .flex()
            .flex_none()
            .flex_col()
            .items_center()
            .w(dim::TOOL_RAIL_W)
            .py(px(8.))
            .gap(px(2.))
            .border_r_1()
            .border_color(p.line)
            .children(TOOLS.iter().enumerate().map(|(i, (name, glyph))| {
                let on = i == active;
                div()
                    .id(SharedString::from(*name))
                    .flex()
                    .items_center()
                    .justify_center()
                    .w(dim::TOOL_BTN_W)
                    .h(dim::TOOL_BTN_H)
                    .border_1()
                    .border_color(if on { p.ink } else { transparent_black() })
                    .bg(if on { p.ink } else { transparent_black() })
                    .text_color(if on { p.paper } else { p.ink })
                    .font_family(MONO_FONT)
                    .text_size(px(14.))
                    .child(*glyph)
            }))
            .child(div().flex_1())
            .child(
                div()
                    .size(px(30.))
                    .border_1()
                    .border_color(p.ink)
                    .bg(rgb(0xC07A4A)),
            )
    }

    fn context_bar(&self, p: &Palette) -> Div {
        let (tool, _) = TOOLS[self.active_tool];
        div()
            .flex()
            .flex_none()
            .overflow_hidden()
            .items_center()
            .gap(px(13.))
            .px(px(16.))
            .py(px(9.))
            .border_b_1()
            .border_color(p.line)
            .font_family(MONO_FONT)
            .text_size(px(10.5))
            .text_color(p.muted)
            .child(div().text_color(p.ink).child(tool.to_uppercase()))
            .child(div().child("mode semantic"))
            .child(div().child("feather 0.6 px"))
            .child(div().child("edge auto-refine"))
            .child(div().flex_1())
            .child(div().child("before / after"))
            .child(div().w(dim::COMPARE_SLIDER_W).h(px(2.)).bg(p.line))
            .child(div().child("34%"))
    }

    fn canvas(&self, p: &Palette) -> Div {
        div()
            .flex_1()
            .min_h_0()
            .overflow_hidden()
            .flex()
            .items_center()
            .justify_center()
            .p(px(30.))
            .bg(p.stage)
            .child(
                div()
                    .w_full()
                    .max_w(px(760.))
                    .aspect_ratio(16. / 10.)
                    .bg(rgb(0xC9C7C0))
                    .border_1()
                    .border_color(p.line)
                    .relative()
                    .child(
                        div()
                            .absolute()
                            .top(px(11.))
                            .left(px(11.))
                            .child(Self::mono("hero_shot.raw", 10., rgb(0x3A3A3C).into())),
                    ),
            )
    }

    fn suggestion_strip(&self, p: &Palette) -> Div {
        div()
            .flex()
            .flex_none()
            .flex_wrap()
            .overflow_hidden()
            .items_center()
            .gap(px(9.))
            .px(px(16.))
            .py(px(10.))
            .border_t_1()
            .border_color(p.line)
            .child(Self::mono("IT NOTICED", 9.5, p.muted))
            .children(SUGGESTIONS.iter().map(|(key, label)| {
                div()
                    .flex()
                    .items_center()
                    .gap(px(8.))
                    .px(px(11.))
                    .py(px(6.))
                    .border_1()
                    .border_color(p.line)
                    .bg(p.soft_bg)
                    .text_size(px(12.))
                    .text_color(p.ink)
                    .child(Self::mono(*key, 10., p.muted))
                    .child(*label)
            }))
            .child(div().flex_1())
            .child(Self::mono(
                "non-destructive · 6 nodes · autosaved 12s ago",
                10.,
                p.muted,
            ))
    }

    fn node_panel(&self, p: &Palette) -> Stateful<Div> {
        let selected = self.selected_node;
        div()
            .id("node-panel")
            .flex()
            .flex_none()
            .flex_col()
            .w(dim::NODE_PANEL_W)
            .min_h_0()
            .border_l_1()
            .border_color(p.line)
            .overflow_y_scroll()
            .child(
                div()
                    .flex()
                    .flex_col()
                    .gap(px(2.))
                    .px(px(15.))
                    .pt(px(13.))
                    .pb(px(11.))
                    .border_b_1()
                    .border_color(p.line)
                    .child(Self::mono("SCENE GRAPH", 9.5, p.muted).pb(px(6.)))
                    .children(NODES.iter().enumerate().map(|(i, (name, meta, chip))| {
                        let on = i == selected;
                        div()
                            .flex()
                            .items_center()
                            .gap(px(9.))
                            .px(px(8.))
                            .py(px(7.))
                            .border_1()
                            .border_color(if on { p.ink } else { p.line })
                            .bg(if on { p.ink } else { transparent_black() })
                            .text_color(if on { p.paper } else { p.ink })
                            .child(div().size(px(20.)).flex_none().bg(rgb(*chip)))
                            .child(
                                div()
                                    .flex_1()
                                    .min_w_0()
                                    .overflow_hidden()
                                    .text_size(px(12.5))
                                    .child(*name),
                            )
                            .child(Self::mono(*meta, 9.5, if on { p.paper } else { p.muted }))
                    })),
            )
            .child(
                div()
                    .flex()
                    .flex_col()
                    .gap(px(10.))
                    .px(px(15.))
                    .py(px(13.))
                    .border_b_1()
                    .border_color(p.line)
                    .child(Self::mono(NODES[selected].0.to_uppercase(), 9.5, p.muted))
                    .children(
                        [
                            ("exposure", "-0.32 ev"),
                            ("warmth", "61%"),
                            ("depth blend", "28%"),
                            ("grain", "12%"),
                        ]
                        .into_iter()
                        .map(|(label, value)| {
                            div()
                                .flex()
                                .flex_col()
                                .gap(px(5.))
                                .child(
                                    div()
                                        .flex()
                                        .justify_between()
                                        .font_family(MONO_FONT)
                                        .text_size(px(10.5))
                                        .text_color(p.ink)
                                        .child(label)
                                        .child(div().text_color(p.muted).child(value)),
                                )
                                .child(div().w_full().h(px(2.)).bg(p.line))
                        }),
                    ),
            )
            .child(
                div()
                    .flex()
                    .flex_col()
                    .gap(px(8.))
                    .px(px(15.))
                    .py(px(13.))
                    .child(
                        div()
                            .flex()
                            .justify_between()
                            .child(Self::mono("HISTORY", 9.5, p.muted))
                            .child(Self::mono("graph →", 10., p.accent)),
                    )
                    .children(
                        [
                            ("Sky replace", "2m ago · you"),
                            ("Relight subject", "6m ago · auto"),
                            ("Filmic grade", "11m ago · you"),
                            ("Dust removal ×7", "18m ago · auto"),
                            ("Import raw", "32m ago"),
                        ]
                        .into_iter()
                        .enumerate()
                        .map(|(i, (label, meta))| {
                            div()
                                .flex()
                                .gap(px(10.))
                                .child(div().size(px(7.)).mt(px(4.)).rounded_full().bg(if i == 0 {
                                    p.accent
                                } else {
                                    p.muted
                                }))
                                .child(
                                    div()
                                        .flex()
                                        .flex_col()
                                        .child(
                                            div()
                                                .text_size(px(12.5))
                                                .text_color(p.ink)
                                                .child(label),
                                        )
                                        .child(Self::mono(meta, 9.5, p.muted)),
                                )
                        }),
                    ),
            )
    }
}

impl Render for EditorShell {
    fn render(&mut self, _window: &mut Window, cx: &mut Context<Self>) -> impl IntoElement {
        let p = theme::palette(cx);
        div()
            .flex()
            .flex_col()
            .size_full()
            .bg(p.paper)
            .text_color(p.ink)
            .font_family(UI_FONT)
            .text_size(px(13.))
            .child(self.top_bar(&p))
            .child(self.doc_bar(&p))
            .child(
                div()
                    .flex()
                    .flex_1()
                    .min_h_0()
                    .items_stretch()
                    .child(self.tool_rail(&p))
                    .child(
                        div()
                            .flex()
                            .flex_col()
                            .flex_1()
                            .min_w_0()
                            .min_h_0()
                            .overflow_hidden()
                            .child(self.context_bar(&p))
                            .child(self.canvas(&p))
                            .child(self.suggestion_strip(&p)),
                    )
                    .child(self.node_panel(&p)),
            )
    }
}
