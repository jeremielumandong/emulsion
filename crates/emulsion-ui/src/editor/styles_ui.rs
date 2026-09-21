//! Layer styles in the node panel: add, tune, recolour and remove.

use super::*;
use emulsion_core::styles::LayerStyle;
use emulsion_raster::composite::{BlendIfChannel, BlendingOptions};

#[derive(Default)]
pub(crate) struct StylesUi {
    pub menu_for: Option<NodeId>,
    pub blend_if_open: bool,
}

impl EditorView {
    pub(crate) fn set_blending(
        &mut self,
        id: NodeId,
        change: impl FnOnce(&mut BlendingOptions),
        cx: &mut Context<Self>,
    ) {
        let Some(node) = self.editor.doc.node(id) else {
            return;
        };
        let mut options = node.blending;
        change(&mut options);
        self.execute(Command::SetBlendingOptions { id, options }, cx);
    }

    pub(crate) fn set_blend_range(
        &mut self,
        id: NodeId,
        backdrop: bool,
        index: usize,
        value: f32,
        cx: &mut Context<Self>,
    ) {
        self.set_blending(
            id,
            |options| {
                let range = if backdrop {
                    &mut options.blend_if.backdrop
                } else {
                    &mut options.blend_if.source
                };
                let mut points = [range.black, range.black_fade, range.white_fade, range.white];
                points[index] = value.clamp(0., 1.);
                for i in (0..index).rev() {
                    points[i] = points[i].min(points[i + 1]);
                }
                for i in index + 1..4 {
                    points[i] = points[i].max(points[i - 1]);
                }
                [range.black, range.black_fade, range.white_fade, range.white] = points;
            },
            cx,
        );
    }

    pub(crate) fn open_blending_options(&mut self, id: NodeId, cx: &mut Context<Self>) {
        if self.editor.doc.node(id).is_none() {
            return;
        }
        self.selected = Some(id);
        self.styles_ui.menu_for = None;
        self.select_sidebar(SidebarTab::BlendingOptions, cx);
    }

    pub(super) fn blending_options_panel(
        &mut self,
        p: &Palette,
        cx: &mut Context<Self>,
    ) -> AnyElement {
        let mut body = div()
            .id("layer-blending-panel")
            .flex()
            .flex_col()
            .gap_3()
            .p_3()
            .child(
                div()
                    .flex()
                    .items_center()
                    .gap_2()
                    .child(label("Blending Options", p))
                    .child(div().flex_1())
                    .child(
                        chip("layer-blending-close", "Close", false, p)
                            .on_click(cx.listener(|this, _, window, cx| {
                                this.select_sidebar(SidebarTab::History, cx);
                                window.focus(&this.panel_focus, cx);
                            }))
                            .test_support(),
                    ),
            );
        let Some(n) = self
            .selected
            .and_then(|id| self.editor.doc.node(id))
            .cloned()
        else {
            return body
                .child(label("Select a layer to edit its blending options.", p))
                .test_support()
                .into_any_element();
        };
        let id = n.id;
        body = body.child(label(n.name.clone(), p));
        if self.editor.doc.locked_ancestor(id).is_some() {
            return body
                .child(label(
                    "This layer or its group is locked. Unlock it to edit blending options.",
                    p,
                ))
                .test_support()
                .into_any_element();
        }
        let blend_open = self.menu == Some(Menu::Blend);
        body = body.child(
            div()
                .flex()
                .items_center()
                .justify_between()
                .child(label("Blend mode", p))
                .child(
                    chip("layer-blend", n.blend.label(), blend_open, p)
                        .on_click(cx.listener(|this, _, _, cx| {
                            this.menu = if this.menu == Some(Menu::Blend) {
                                None
                            } else {
                                Some(Menu::Blend)
                            };
                            cx.notify();
                        }))
                        .test_support(),
                ),
        );
        if blend_open {
            let modes = n
                .is_group()
                .then_some(BlendMode::PassThrough)
                .into_iter()
                .chain(BlendMode::MENU.iter().flatten().copied())
                .map(|m| (SharedString::from(m.label()), MenuAction::Blend(id, m)))
                .collect();
            body = body.child(self.menu_list("blending-mode-menu", modes, p, cx));
        }
        body = body.child(self.param_slider(
            SliderKey::Opacity(id),
            "Opacity",
            format!("{:.0}%", n.opacity * 100.),
            n.opacity,
            (0., 100., 1.),
            p,
            cx,
        ));
        body = body
            .child(self.param_slider(
                SliderKey::FillOpacity(id),
                "Fill",
                format!("{:.0}%", n.blending.fill_opacity * 100.),
                n.blending.fill_opacity,
                (0., 100., 1.),
                p,
                cx,
            ))
            .child(mono(
                "Fill changes layer content while preserving its effects.",
                10.,
                p.muted,
            ));
        let mut channels = div()
            .flex()
            .items_center()
            .gap_2()
            .child(label("Channels", p));
        for (index, name) in ["Red", "Green", "Blue"].into_iter().enumerate() {
            channels = channels.child(
                chip(
                    ("blend-channel", index),
                    name,
                    n.blending.channels[index],
                    p,
                )
                .on_click(cx.listener(move |this, _, _, cx| {
                    this.set_blending(
                        id,
                        |options| options.channels[index] = !options.channels[index],
                        cx,
                    )
                }))
                .test_support(),
            );
        }
        body = body.child(channels).child(
            chip(
                "blend-if-toggle",
                "Blend If",
                self.styles_ui.blend_if_open,
                p,
            )
            .on_click(cx.listener(|this, _, _, cx| {
                this.styles_ui.blend_if_open = !this.styles_ui.blend_if_open;
                cx.notify();
            }))
            .test_support(),
        );
        if self.styles_ui.blend_if_open {
            let mut channels = div().flex().flex_wrap().gap_1();
            for (index, (channel, name)) in [
                (BlendIfChannel::Gray, "Gray"),
                (BlendIfChannel::Red, "Red"),
                (BlendIfChannel::Green, "Green"),
                (BlendIfChannel::Blue, "Blue"),
            ]
            .into_iter()
            .enumerate()
            {
                channels = channels.child(
                    chip(
                        ("blend-if-channel", index),
                        name,
                        n.blending.blend_if.channel == channel,
                        p,
                    )
                    .on_click(cx.listener(move |this, _, _, cx| {
                        this.set_blending(id, |options| options.blend_if.channel = channel, cx)
                    }))
                    .test_support(),
                );
            }
            body = body.child(channels);
            for (backdrop, title, range) in [
                (false, "This layer", n.blending.blend_if.source),
                (true, "Underlying layers", n.blending.blend_if.backdrop),
            ] {
                body = body.child(label(title, p));
                for (index, (name, value)) in [
                    ("Black cutoff", range.black),
                    ("Black fade end", range.black_fade),
                    ("White fade start", range.white_fade),
                    ("White cutoff", range.white),
                ]
                .into_iter()
                .enumerate()
                {
                    body = body.child(self.param_slider(
                        SliderKey::BlendRange(id, backdrop, index),
                        name,
                        format!("{:.0}", value * 255.),
                        value,
                        (0., 255., 1.),
                        p,
                        cx,
                    ));
                }
            }
            body = body.child(
                chip("blend-if-reset", "Reset Blend If", false, p)
                    .on_click(cx.listener(move |this, _, _, cx| {
                        this.set_blending(id, |options| options.blend_if = Default::default(), cx)
                    }))
                    .test_support(),
            );
        }
        if matches!(
            n.kind,
            NodeKind::Raster { .. }
                | NodeKind::Smart { .. }
                | NodeKind::Path { .. }
                | NodeKind::Text { .. }
        ) {
            body = body.children(self.styles_panel_with_catalogue(id, &n.styles, true, p, cx));
        }
        body.child(mono(
            "Changes apply immediately. Undo restores the previous setting.",
            10.,
            p.muted,
        ))
        .test_support()
        .into_any_element()
    }

    fn set_styles(&mut self, id: NodeId, styles: Vec<LayerStyle>, cx: &mut Context<Self>) {
        self.execute(Command::SetStyles { id, styles }, cx);
    }

    pub fn add_style(&mut self, id: NodeId, mut s: LayerStyle, cx: &mut Context<Self>) {
        let Some(n) = self.editor.doc.node(id) else {
            return;
        };
        // New effects take the foreground colour, except shadows.
        if !matches!(
            s,
            LayerStyle::DropShadow { .. }
                | LayerStyle::InnerShadow { .. }
                | LayerStyle::BevelEmboss { .. }
                | LayerStyle::Satin { .. }
        ) {
            let fg = self.tools.fg;
            s.set_color([fg[0], fg[1], fg[2]], false);
        }
        let mut styles = n.styles.clone();
        styles.push(s);
        self.set_styles(id, styles, cx);
        self.styles_ui.menu_for = None;
    }

    pub(crate) fn set_style_param(
        &mut self,
        id: NodeId,
        idx: usize,
        key: &'static str,
        v: f32,
        cx: &mut Context<Self>,
    ) {
        let Some(n) = self.editor.doc.node(id) else {
            return;
        };
        let mut styles = n.styles.clone();
        if let Some(s) = styles.get_mut(idx)
            && s.set_param(key, v)
        {
            self.set_styles(id, styles, cx);
        }
    }

    pub(crate) fn styles_panel(
        &mut self,
        id: NodeId,
        styles: &[LayerStyle],
        p: &Palette,
        cx: &mut Context<Self>,
    ) -> Vec<AnyElement> {
        self.styles_panel_with_catalogue(id, styles, false, p, cx)
    }

    fn styles_panel_with_catalogue(
        &mut self,
        id: NodeId,
        styles: &[LayerStyle],
        show_catalogue: bool,
        p: &Palette,
        cx: &mut Context<Self>,
    ) -> Vec<AnyElement> {
        let mut v: Vec<AnyElement> = Vec::new();
        v.push(
            div()
                .flex()
                .items_center()
                .gap(px(6.))
                .pt(px(6.))
                .child(label("Styles", p))
                .child(div().flex_1())
                .when(!show_catalogue, |row| {
                    row.child(
                        chip(
                            "style-add",
                            "+ style",
                            self.styles_ui.menu_for == Some(id),
                            p,
                        )
                        .on_click(cx.listener(move |this, _, _, cx| {
                            this.styles_ui.menu_for = if this.styles_ui.menu_for == Some(id) {
                                None
                            } else {
                                Some(id)
                            };
                            cx.notify();
                        }))
                        .test_support(),
                    )
                })
                .into_any_element(),
        );
        if show_catalogue || self.styles_ui.menu_for == Some(id) {
            let mut menu = div().flex().flex_wrap().gap(px(4.));
            for (i, s) in LayerStyle::catalogue().into_iter().enumerate() {
                let text = s.label();
                menu = menu.child(
                    chip(("style-kind", i), text, false, p)
                        .on_click(
                            cx.listener(move |this, _, _, cx| this.add_style(id, s.clone(), cx)),
                        )
                        .test_support(),
                );
            }
            v.push(menu.into_any_element());
        }
        if !styles.is_empty() {
            v.push(
                mono(
                    "Click an effect swatch to use the foreground color.",
                    9.5,
                    p.muted,
                )
                .into_any_element(),
            );
        }
        for (idx, s) in styles.iter().enumerate() {
            let mut row = div()
                .flex()
                .items_center()
                .gap(px(6.))
                .pt(px(4.))
                .child(mono(s.label(), 10.5, p.ink));
            for (ci, c) in s.colors().iter().enumerate() {
                let col: Hsla =
                    rgb(((c[0] as u32) << 16) | ((c[1] as u32) << 8) | c[2] as u32).into();
                row = row.child(
                    div()
                        .id(("style-col", idx * 2 + ci))
                        .size(px(14.))
                        .border_1()
                        .border_color(p.ink)
                        .bg(col)
                        .cursor_pointer()
                        .on_click(cx.listener(move |this, _, _, cx| {
                            // Click a swatch to give it the foreground colour.
                            let fg = this.tools.fg;
                            let Some(n) = this.editor.doc.node(id) else {
                                return;
                            };
                            let mut styles = n.styles.clone();
                            if let Some(s) = styles.get_mut(idx) {
                                s.set_color([fg[0], fg[1], fg[2]], ci == 1);
                                this.set_styles(id, styles, cx);
                            }
                        })),
                );
            }
            row =
                row.child(div().flex_1())
                    .child(
                        chip(("style-del", idx), "×", false, p).on_click(cx.listener(
                            move |this, _, _, cx| {
                                let Some(n) = this.editor.doc.node(id) else {
                                    return;
                                };
                                let mut styles = n.styles.clone();
                                if idx < styles.len() {
                                    styles.remove(idx);
                                    this.set_styles(id, styles, cx);
                                }
                            },
                        )),
                    );
            v.push(row.into_any_element());
            for spec in s.params() {
                let norm = (spec.value - spec.min) / (spec.max - spec.min).max(1e-6);
                v.push(
                    self.param_slider(
                        SliderKey::Style(id, idx, spec.key),
                        spec.label,
                        spec.display(),
                        norm,
                        (spec.min, spec.max, spec.step),
                        p,
                        cx,
                    )
                    .into_any_element(),
                );
            }
        }
        if styles.is_empty() {
            v.push(
                mono(
                    "swatches take the foreground colour when clicked",
                    9.5,
                    p.muted,
                )
                .into_any_element(),
            );
        }
        v
    }
}
