//! Layer styles in the node panel: add, tune, recolour and remove.

use super::*;
use emulsion_core::styles::LayerStyle;

#[derive(Default)]
pub(crate) struct StylesUi {
    pub menu_for: Option<NodeId>,
}

impl EditorView {
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
            LayerStyle::DropShadow { .. } | LayerStyle::InnerShadow { .. }
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
        let mut v: Vec<AnyElement> = Vec::new();
        v.push(
            div()
                .flex()
                .items_center()
                .gap(px(6.))
                .pt(px(6.))
                .child(label("Styles", p))
                .child(div().flex_1())
                .child(
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
                    })),
                )
                .into_any_element(),
        );
        if self.styles_ui.menu_for == Some(id) {
            let mut menu = div().flex().flex_wrap().gap(px(4.));
            for (i, s) in LayerStyle::catalogue().into_iter().enumerate() {
                let text = s.label();
                menu = menu.child(chip(("style-kind", i), text, false, p).on_click(
                    cx.listener(move |this, _, _, cx| this.add_style(id, s.clone(), cx)),
                ));
            }
            v.push(menu.into_any_element());
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
