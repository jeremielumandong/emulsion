//! The Type tool: click to place a text layer, then type into the field in
//! the options bar; every keystroke re-shapes the layer. Click an existing
//! text layer to edit it. Size, weight, slant, alignment and wrap width
//! live in the options bar and apply to the selected text layer at once.

use super::*;
use emulsion_core::text::{Align, TextSpec};

#[derive(Default)]
pub struct TypeState {
    /// Style for the next text layer; follows the selected one.
    pub spec: TextSpec,
    /// The field editing `NodeId`'s text.
    pub field: Option<(NodeId, Entity<InputState>, Subscription)>,
    /// Where the font chip was laid out, so its list opens under it.
    pub font_chip: crate::widgets::TrackBounds,
}

impl EditorView {
    /// The selected text layer, if any.
    pub(crate) fn text_target(&self) -> Option<(NodeId, Arc<TextSpec>)> {
        let id = self.selected?;
        match &self.editor.doc.node(id)?.kind {
            NodeKind::Text { spec, .. } => Some((id, spec.clone())),
            _ => None,
        }
    }

    /// The topmost text layer under document point `d`.
    fn text_hit(&self, d: (f64, f64)) -> Option<NodeId> {
        let (w, h) = (self.editor.doc.width as f64, self.editor.doc.height as f64);
        let inside = d.0 >= 0.0 && d.1 >= 0.0 && d.0 < w && d.1 < h;
        self.editor
            .doc
            .nodes
            .iter()
            .rev()
            .find_map(|n| match &n.kind {
                NodeKind::Text { spec, cache }
                    if n.visible && self.editor.doc.locked_ancestor(n.id).is_none() =>
                {
                    let ink = inside && cache.get(d.0 as u32, d.1 as u32)[3] > 0;
                    let lines = spec.text.lines().count().max(1) as f64;
                    let est_w = spec.width.map(f64::from).unwrap_or(
                        spec.size as f64 * 0.6 * spec.text.chars().count().max(1) as f64,
                    );
                    let delta = glam::dvec2(d.0 - spec.x as f64, d.1 - spec.y as f64);
                    let local =
                        glam::DMat2::from_angle(-(spec.rotation as f64).to_radians()) * delta;
                    let bx = local.x >= 0.0
                        && local.x <= est_w
                        && local.y >= 0.0
                        && local.y <= spec.size as f64 * spec.line_height as f64 * lines;
                    (ink || bx).then_some(n.id)
                }
                _ => None,
            })
    }

    /// Drop the text field and close the typing history step.
    pub(crate) fn close_text_field(&mut self, cx: &mut Context<Self>) {
        if self.type_tool.field.take().is_some() {
            if self.editor.in_transaction() {
                self.editor.end();
            }
            cx.notify();
        }
    }

    pub(crate) fn type_down(&mut self, d: (f64, f64), window: &mut Window, cx: &mut Context<Self>) {
        self.close_text_field(cx);
        if let Some(id) = self.text_hit(d) {
            self.selected = Some(id);
            if let Some((_, spec)) = self.text_target() {
                self.type_tool.spec = (*spec).clone();
            }
            self.open_text_field(id, window, cx);
            return;
        }
        // A new layer where the click landed, with a placeholder to see.
        let mut spec = self.type_tool.spec.clone();
        spec.text = "Text".into();
        spec.x = d.0.round() as f32;
        spec.y = d.1.round() as f32;
        let fg = self.tools.fg;
        spec.color = fg;
        let (w, h) = (self.editor.doc.width, self.editor.doc.height);
        let node = Node::text(0, "Text", spec.clone(), w, h);
        let slot = self.insertion_slot();
        let Some(id) = self.execute(
            Command::AddNode {
                node: Box::new(node),
                slot,
            },
            cx,
        ) else {
            return;
        };
        self.selected = Some(id);
        self.type_tool.spec = spec;
        self.open_text_field(id, window, cx);
    }

    fn open_text_field(&mut self, id: NodeId, window: &mut Window, cx: &mut Context<Self>) {
        let Some((_, spec)) = self.text_target() else {
            return;
        };
        let text = spec.text.clone();
        let state = cx.new(|cx| {
            InputState::new(window, cx)
                .default_value(text)
                .placeholder("type here — Enter to finish")
        });
        // The canvas takes focus on this same mouse-down after our handler
        // runs, so hand it to the field once the event has finished.
        let st = state.clone();
        cx.defer_in(window, move |_, window, cx| {
            st.update(cx, |s, cx| {
                s.focus(window, cx);
                // Typing replaces the placeholder.
                s.select_all(window, cx);
            });
        });
        let sub = cx.subscribe_in(
            &state,
            window,
            move |this, st, ev: &InputEvent, window, cx| match ev {
                InputEvent::Change => {
                    let text = st.read(cx).value().to_string();
                    this.set_text_field(id, text, cx);
                }
                InputEvent::PressEnter { .. } => {
                    this.close_text_field(cx);
                    window.focus(&this.canvas_focus, cx);
                }
                InputEvent::Blur => this.close_text_field(cx),
                _ => {}
            },
        );
        self.type_tool.field = Some((id, state, sub));
        cx.notify();
    }

    fn set_text_field(&mut self, id: NodeId, text: String, cx: &mut Context<Self>) {
        let Some(n) = self.editor.doc.node(id) else {
            return;
        };
        let NodeKind::Text { spec, .. } = &n.kind else {
            return;
        };
        if spec.text == text {
            return;
        }
        let old_label = spec.label();
        let mut s = (**spec).clone();
        s.text = text;
        // Keep the whole typing session one history step.
        if !self.editor.in_transaction() {
            self.editor.begin("Type");
        }
        self.execute(
            Command::SetText {
                id,
                spec: Box::new(s),
            },
            cx,
        );
        if let Some(n) = self.editor.doc.node(id)
            && let NodeKind::Text { spec, .. } = &n.kind
            && (n.name == "Text" || n.name == old_label)
        {
            // Layers named after their text follow it.
            let label = spec.label();
            if label != n.name {
                self.execute(Command::Rename { id, name: label }, cx);
            }
        }
    }

    /// Change one aspect of the selected text layer and the tool defaults.
    pub(crate) fn restyle_text(&mut self, f: impl Fn(&mut TextSpec), cx: &mut Context<Self>) {
        f(&mut self.type_tool.spec);
        if let Some((id, spec)) = self.text_target() {
            let mut s = (*spec).clone();
            f(&mut s);
            if s != *spec {
                self.execute(
                    Command::SetText {
                        id,
                        spec: Box::new(s),
                    },
                    cx,
                );
            }
        }
        cx.notify();
    }

    pub(crate) fn type_options(
        &mut self,
        v: &mut Vec<AnyElement>,
        p: &Palette,
        cx: &mut Context<Self>,
    ) {
        let cur = self
            .text_target()
            .map(|(_, s)| (*s).clone())
            .unwrap_or_else(|| self.type_tool.spec.clone());
        if let Some((_, state, _)) = &self.type_tool.field {
            v.push(
                div()
                    .w(px(260.))
                    .child(Input::new(state).appearance(false).bordered(false))
                    .into_any_element(),
            );
        } else {
            v.push(
                mono(
                    if self.text_target().is_some() {
                        "click the text to edit it · drag with Move (V)"
                    } else {
                        "click the canvas to place text"
                    },
                    10.,
                    p.muted,
                )
                .into_any_element(),
            );
        }
        v.push(self.opt_slider(
            SliderKey::TextSize,
            "size",
            format!("{:.0}px", cur.size),
            (cur.size / 400.0).sqrt(),
            (4.0, 400.0, 1.0),
            p,
            cx,
        ));
        v.push(
            chip("type-bold", "bold", cur.bold, p)
                .on_click(
                    cx.listener(move |this, _, _, cx| this.restyle_text(|s| s.bold = !s.bold, cx)),
                )
                .into_any_element(),
        );
        v.push(
            chip("type-italic", "italic", cur.italic, p)
                .on_click(cx.listener(move |this, _, _, cx| {
                    this.restyle_text(|s| s.italic = !s.italic, cx)
                }))
                .into_any_element(),
        );
        for (i, (a, t)) in [
            (Align::Left, "left"),
            (Align::Center, "centre"),
            (Align::Right, "right"),
        ]
        .into_iter()
        .enumerate()
        {
            v.push(
                chip(("type-align", i), t, cur.align == a, p)
                    .on_click(
                        cx.listener(move |this, _, _, cx| this.restyle_text(|s| s.align = a, cx)),
                    )
                    .into_any_element(),
            );
        }
        let wrapped = cur.width.is_some();
        v.push(
            chip(
                "type-wrap",
                if wrapped { "wrap ✓" } else { "wrap" },
                wrapped,
                p,
            )
            .on_click(cx.listener(move |this, _, _, cx| {
                let dw = this.editor.doc.width as f32;
                this.restyle_text(
                    move |s| {
                        s.width = if s.width.is_some() {
                            None
                        } else {
                            Some(((dw - s.x) * 0.6).clamp(40.0, dw))
                        }
                    },
                    cx,
                )
            }))
            .into_any_element(),
        );
        v.push(
            chip("type-colour", "use colour", false, p)
                .on_click(cx.listener(move |this, _, _, cx| {
                    let fg = this.tools.fg;
                    this.restyle_text(move |s| s.color = fg, cx)
                }))
                .into_any_element(),
        );
        if let Some((id, _)) = self.text_target() {
            v.push(
                chip("type-raster", "rasterize", false, p)
                    .on_click(cx.listener(move |this, _, _, cx| this.rasterize_text(id, cx)))
                    .into_any_element(),
            );
        }
        let open = self.menu == Some(super::Menu::Font);
        let label = if cur.font.is_empty() {
            "font: default ▾".to_string()
        } else {
            format!("font: {} ▾", cur.font)
        };
        let chip_bounds = self.type_tool.font_chip.clone();
        v.push(
            crate::widgets::tip(
                chip("type-font", label, open, p)
                    .relative()
                    .when(!cur.font.is_empty(), |c| c.font_family(cur.font.clone()))
                    .child(
                        canvas(move |b, _, _| chip_bounds.set(Some(b)), |_, _, _, _| {})
                            .absolute()
                            .size_full(),
                    )
                    .on_click(cx.listener(move |this, _, _, cx| {
                        this.menu = if open { None } else { Some(super::Menu::Font) };
                        cx.notify();
                    })),
                "Choose a font; every family is shown in itself",
            )
            .into_any_element(),
        );
    }

    /// The font list under the options bar: every installed family, each
    /// name set in its own face so the choice can be made by eye.
    pub(crate) fn font_picker(&self, p: &Palette, cx: &mut Context<Self>) -> Option<AnyElement> {
        if self.menu != Some(super::Menu::Font) || self.tool != Tool::Type {
            return None;
        }
        let current = self.type_tool.spec.font.clone();
        let (accent, accent_fg, ink, paper) = (p.accent, p.accent_fg, p.ink, p.paper);
        let mut fonts = emulsion_core::text::font_families();
        fonts.insert(0, String::new());
        let rows = fonts.into_iter().enumerate().map(|(i, name)| {
            let on = name == current;
            let display: SharedString = if name.is_empty() {
                "default".into()
            } else {
                name.clone().into()
            };
            let choose = name.clone();
            div()
                .id(("font-row", i))
                .flex()
                .items_baseline()
                .justify_between()
                .gap(px(12.))
                .px(px(10.))
                .py(px(5.))
                .cursor_pointer()
                .when(on, |d| d.bg(ink).text_color(paper))
                .hover(move |s| s.bg(accent).text_color(accent_fg))
                .on_click(cx.listener(move |this, _, _, cx| {
                    let f = choose.clone();
                    this.restyle_text(move |s| s.font = f.clone(), cx);
                    this.menu = None;
                    cx.notify();
                }))
                .child(
                    div()
                        .text_size(px(15.))
                        .when(!name.is_empty(), |d| d.font_family(name.clone()))
                        .child(display),
                )
                .child(
                    div()
                        .font_family(MONO_FONT)
                        .text_size(px(9.5))
                        .text_color(if on { paper } else { p.muted })
                        .child(if name.is_empty() {
                            "system"
                        } else {
                            "Aa Bb 0123"
                        }),
                )
        });
        // Under the chip, in window coordinates; snapped inside the window.
        let at = self
            .type_tool
            .font_chip
            .get()
            .map(|b| point(b.left(), b.bottom() + px(4.)))
            .unwrap_or(point(px(90.), px(230.)));
        Some(
            deferred(
                anchored().position(at).snap_to_window().child(
                    div()
                        .id("font-picker")
                        // Wheel and clicks stop here instead of zooming
                        // the canvas underneath.
                        .occlude()
                        .w(px(340.))
                        .max_h(px(380.))
                        .flex()
                        .flex_col()
                        .border_1()
                        .border_color(p.ink)
                        .bg(p.panel)
                        .text_color(p.ink)
                        .overflow_y_scroll()
                        .on_mouse_down_out(cx.listener(|this, _, _, cx| {
                            if this.menu == Some(super::Menu::Font) {
                                this.menu = None;
                                cx.notify();
                            }
                        }))
                        .children(rows),
                ),
            )
            .with_priority(1)
            .into_any_element(),
        )
    }

    /// Bake the selected text layer into pixels.
    pub(crate) fn rasterize_text(&mut self, id: NodeId, cx: &mut Context<Self>) {
        let Some(n) = self.editor.doc.node(id) else {
            return;
        };
        let NodeKind::Text { cache, .. } = &n.kind else {
            return;
        };
        let (name, cache) = (n.name.clone(), cache.clone());
        let sib = self.editor.doc.children(n.parent);
        let slot = Slot {
            parent: n.parent,
            index: sib.iter().position(|s| *s == id).unwrap_or(0),
        };
        self.editor.begin("Rasterize text");
        self.execute(Command::RemoveNode { id }, cx);
        let node = Node::raster(0, name, cache, Placement::default());
        if let Some(new) = self.execute(
            Command::AddNode {
                node: Box::new(node),
                slot,
            },
            cx,
        ) {
            self.selected = Some(new);
        }
        self.editor.end();
        self.close_text_field(cx);
        cx.notify();
    }
}
