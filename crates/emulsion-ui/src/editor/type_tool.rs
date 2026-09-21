//! In-place text editing on the canvas, with native text input and IME.

use super::*;
use emulsion_core::text::{Align, TextSpec};
use std::ops::Range;
use unicode_segmentation::UnicodeSegmentation;

#[derive(Default)]
pub struct TypeState {
    pub spec: TextSpec,
    pub field: Option<TextSession>,
    pub font_chip: crate::widgets::TrackBounds,
}

pub struct TextSession {
    pub id: NodeId,
    pub anchor: usize,
    pub cursor: usize,
    pub marked: Option<Range<usize>>,
    pub selecting: bool,
    _blur: Subscription,
}

impl TextSession {
    fn range(&self) -> Range<usize> {
        self.anchor.min(self.cursor)..self.anchor.max(self.cursor)
    }
}

fn utf16_to_byte(text: &str, offset: usize) -> usize {
    let mut utf16 = 0;
    for (byte, ch) in text.char_indices() {
        if utf16 + ch.len_utf16() > offset {
            return byte;
        }
        utf16 += ch.len_utf16();
    }
    text.len()
}
fn byte_to_utf16(text: &str, byte: usize) -> usize {
    text[..floor_byte(text, byte)].encode_utf16().count()
}
fn floor_byte(text: &str, byte: usize) -> usize {
    let mut byte = byte.min(text.len());
    while !text.is_char_boundary(byte) {
        byte -= 1;
    }
    byte
}
fn word_range(text: &str, byte: usize) -> Range<usize> {
    text.split_word_bound_indices()
        .find(|(at, word)| *at <= byte && byte < *at + word.len())
        .map(|(at, word)| at..at + word.len())
        .unwrap_or(byte..byte)
}

impl EditorView {
    pub(crate) fn text_target(&self) -> Option<(NodeId, Arc<TextSpec>)> {
        let id = self.selected?;
        match &self.editor.doc.node(id)?.kind {
            NodeKind::Text { spec, .. } => Some((id, spec.clone())),
            _ => None,
        }
    }

    fn editing_text(&self) -> Option<Arc<TextSpec>> {
        let id = self.type_tool.field.as_ref()?.id;
        match &self.editor.doc.node(id)?.kind {
            NodeKind::Text { spec, .. } => Some(spec.clone()),
            _ => None,
        }
    }

    fn text_hit(&self, d: (f64, f64)) -> Option<NodeId> {
        self.editor.doc.nodes.iter().rev().find_map(|node| {
            let NodeKind::Text { spec, .. } = &node.kind else {
                return None;
            };
            if !node.visible || self.editor.doc.locked_ancestor(node.id).is_some() {
                return None;
            }
            let local = spec
                .transform()
                .inverse()
                .transform_point2(glam::dvec2(d.0, d.1));
            let rect = emulsion_core::text::layout(spec).bounds();
            let padding = 4. / self.view.zoom.max(0.01) as f32;
            (local.x as f32 >= rect.x - padding
                && local.y as f32 >= rect.y - padding
                && local.x as f32 <= rect.x + rect.width.max(spec.size * 0.5) + padding
                && local.y as f32 <= rect.y + rect.height.max(spec.size) + padding)
                .then_some(node.id)
        })
    }

    pub(crate) fn close_text_field(&mut self, cx: &mut Context<Self>) {
        if self.type_tool.field.take().is_some() {
            if self.editor.in_transaction() {
                self.editor.end();
            }
            cx.notify();
        }
    }

    fn cancel_text_field(&mut self, cx: &mut Context<Self>) {
        if self.type_tool.field.take().is_some() {
            self.editor.cancel();
            if self
                .selected
                .is_some_and(|id| self.editor.doc.node(id).is_none())
            {
                self.set_layer_selection(Vec::new(), None);
            }
            self.after_change(cx);
        }
    }

    pub(crate) fn type_down(&mut self, d: (f64, f64), window: &mut Window, cx: &mut Context<Self>) {
        self.type_pointer_down(d, 1, false, window, cx);
    }

    pub(crate) fn try_edit_text_at(
        &mut self,
        d: (f64, f64),
        clicks: usize,
        window: &mut Window,
        cx: &mut Context<Self>,
    ) -> bool {
        if self.text_hit(d).is_none() {
            return false;
        }
        self.set_tool(Tool::Type, cx);
        self.type_pointer_down(d, clicks, false, window, cx);
        true
    }

    pub(crate) fn type_pointer_down(
        &mut self,
        d: (f64, f64),
        clicks: usize,
        shift: bool,
        window: &mut Window,
        cx: &mut Context<Self>,
    ) {
        let hit = self.text_hit(d);
        let editing = self.type_tool.field.as_ref().map(|field| field.id);
        if hit != editing || editing.is_none() {
            self.close_text_field(cx);
            self.editor.begin("Type");
            let (id, created) = if let Some(id) = hit {
                (id, false)
            } else {
                let mut spec = self.type_tool.spec.clone();
                spec.text = "Text".into();
                spec.x = d.0.round() as f32;
                spec.y = d.1.round() as f32;
                spec.color = self.tools.fg;
                let node = Node::text(
                    0,
                    "Text",
                    spec,
                    self.editor.doc.width,
                    self.editor.doc.height,
                );
                let Some(id) = self.execute(
                    Command::AddNode {
                        node: Box::new(node),
                        slot: self.insertion_slot(),
                    },
                    cx,
                ) else {
                    self.editor.cancel();
                    return;
                };
                (id, true)
            };
            self.set_layer_selection(vec![id], Some(id));
            let Some((_, spec)) = self.text_target() else {
                return;
            };
            self.type_tool.spec = (*spec).clone();
            let blur = cx.on_blur(&self.canvas_focus, window, |this, _, cx| {
                this.close_text_field(cx)
            });
            self.type_tool.field = Some(TextSession {
                id,
                anchor: 0,
                cursor: spec.text.len(),
                marked: None,
                selecting: !created,
                _blur: blur,
            });
            if !created {
                self.place_text_cursor(d, clicks, shift, cx);
            }
        } else {
            self.place_text_cursor(d, clicks, shift, cx);
        }
        window.focus(&self.canvas_focus, cx);
        cx.notify();
    }

    fn place_text_cursor(
        &mut self,
        d: (f64, f64),
        clicks: usize,
        shift: bool,
        cx: &mut Context<Self>,
    ) {
        let Some(spec) = self.editing_text() else {
            return;
        };
        let local = spec
            .transform()
            .inverse()
            .transform_point2(glam::dvec2(d.0, d.1));
        let byte = emulsion_core::text::layout(&spec).hit(local.x as f32, local.y as f32);
        let Some(field) = &mut self.type_tool.field else {
            return;
        };
        field.marked = None;
        field.selecting = true;
        if clicks >= 3 {
            field.anchor = 0;
            field.cursor = spec.text.len();
        } else if clicks == 2 {
            let word = word_range(&spec.text, byte);
            field.anchor = word.start;
            field.cursor = word.end;
        } else {
            if !shift {
                field.anchor = byte;
            }
            field.cursor = byte;
        }
        cx.notify();
    }

    pub(crate) fn text_pointer_move(&mut self, pos: Point<Pixels>, cx: &mut Context<Self>) -> bool {
        if !self
            .type_tool
            .field
            .as_ref()
            .is_some_and(|field| field.selecting)
        {
            return false;
        }
        if let Some(d) = self.doc_point(pos) {
            self.place_text_cursor(d, 1, true, cx);
        }
        true
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
        let mut updated = (**spec).clone();
        updated.text = text;
        if !self.editor.in_transaction() {
            self.editor.begin("Type");
        }
        self.execute(
            Command::SetText {
                id,
                spec: Box::new(updated),
            },
            cx,
        );
        if let Some(n) = self.editor.doc.node(id)
            && let NodeKind::Text { spec, .. } = &n.kind
            && (n.name == "Text" || n.name == old_label)
        {
            let label = spec.label();
            if label != n.name {
                self.execute(Command::Rename { id, name: label }, cx);
            }
        }
    }

    fn replace_canvas_text(&mut self, range: Range<usize>, text: &str, cx: &mut Context<Self>) {
        let Some(spec) = self.editing_text() else {
            return;
        };
        let start = floor_byte(&spec.text, range.start);
        let end = floor_byte(&spec.text, range.end).max(start);
        let range = start..end;
        let Some(field) = &mut self.type_tool.field else {
            return;
        };
        let id = field.id;
        let mut updated = spec.text.clone();
        updated.replace_range(range.clone(), text);
        field.cursor = range.start + text.len();
        field.anchor = field.cursor;
        field.marked = None;
        self.set_text_field(id, updated, cx);
        self.normalize_text_cursor();
        cx.notify();
    }

    fn normalize_text_cursor(&mut self) {
        let Some(spec) = self.editing_text() else {
            return;
        };
        if let Some(field) = &mut self.type_tool.field {
            field.cursor = floor_byte(&spec.text, field.cursor);
            field.anchor = floor_byte(&spec.text, field.anchor);
            field.marked = field.marked.take().and_then(|range| {
                let start = floor_byte(&spec.text, range.start);
                let end = floor_byte(&spec.text, range.end).max(start);
                (start < end).then_some(start..end)
            });
        }
    }

    pub(crate) fn text_key_down(
        &mut self,
        event: &KeyDownEvent,
        window: &mut Window,
        cx: &mut Context<Self>,
    ) -> bool {
        let Some(spec) = self.editing_text() else {
            return false;
        };
        let Some(field) = &self.type_tool.field else {
            return false;
        };
        let text = &spec.text;
        let (cursor, range) = (field.cursor, field.range());
        let modifiers = event.keystroke.modifiers;
        let command = modifiers.control || modifiers.platform;
        let key = event.keystroke.key.as_str();
        let previous = || {
            text.grapheme_indices(true)
                .map(|(i, _)| i)
                .take_while(|i| *i < cursor)
                .last()
                .unwrap_or(0)
        };
        let next = || {
            text.grapheme_indices(true)
                .map(|(i, _)| i)
                .find(|i| *i > cursor)
                .unwrap_or(text.len())
        };
        match key {
            "escape" => self.cancel_text_field(cx),
            "enter" if command => self.close_text_field(cx),
            "t" if command => {
                self.close_text_field(cx);
                self.transform_pixels(cx);
            }
            "enter" => self.replace_canvas_text(range, "\n", cx),
            "a" if command => {
                let field = self.type_tool.field.as_mut().unwrap();
                field.anchor = 0;
                field.cursor = text.len();
                cx.notify();
            }
            "c" | "x" if command => {
                if !range.is_empty() {
                    cx.write_to_clipboard(ClipboardItem::new_string(
                        text[range.clone()].to_string(),
                    ));
                    if key == "x" {
                        self.replace_canvas_text(range, "", cx);
                    }
                }
            }
            "v" if command => {
                if let Some(text) = cx.read_from_clipboard().and_then(|item| item.text()) {
                    self.replace_canvas_text(range, &text, cx);
                }
            }
            "backspace" | "delete" => {
                let range = if range.is_empty() {
                    if key == "backspace" {
                        previous()..cursor
                    } else {
                        cursor..next()
                    }
                } else {
                    range
                };
                self.replace_canvas_text(range, "", cx);
            }
            "left" | "right" | "up" | "down" | "home" | "end" => {
                let backwards = key == "left" || key == "up";
                let inline = if spec.vertical {
                    key == "up" || key == "down"
                } else {
                    key == "left" || key == "right"
                };
                let target = if key == "home" {
                    if command {
                        0
                    } else {
                        text[..cursor].rfind('\n').map_or(0, |i| i + 1)
                    }
                } else if key == "end" {
                    if command {
                        text.len()
                    } else {
                        text[cursor..].find('\n').map_or(text.len(), |i| cursor + i)
                    }
                } else if inline {
                    if !modifiers.shift && !range.is_empty() {
                        if backwards { range.start } else { range.end }
                    } else if command {
                        if backwards {
                            text[..cursor]
                                .unicode_word_indices()
                                .next_back()
                                .map_or(0, |(i, _)| i)
                        } else {
                            text[cursor..]
                                .unicode_word_indices()
                                .next()
                                .map_or(text.len(), |(i, word)| cursor + i + word.len())
                        }
                    } else if backwards {
                        previous()
                    } else {
                        next()
                    }
                } else {
                    let layout = emulsion_core::text::layout(&spec);
                    let caret = layout.caret(cursor);
                    let step = spec.size * spec.line_height.max(0.1);
                    let (x, y) = if spec.vertical {
                        (caret.x + if key == "left" { -step } else { step }, caret.y)
                    } else {
                        (
                            caret.x,
                            caret.y + caret.height * 0.5 + if key == "up" { -step } else { step },
                        )
                    };
                    layout.hit(x, y)
                };
                let field = self.type_tool.field.as_mut().unwrap();
                if !modifiers.shift {
                    field.anchor = target;
                }
                field.cursor = target;
                field.marked = None;
                cx.notify();
            }
            _ => return false,
        }
        let _ = window;
        true
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
        if self.type_tool.field.is_some() {
            for (id, title, cancel) in [
                ("type-done", "Done", false),
                ("type-cancel", "Cancel", true),
            ] {
                v.push(
                    chip(id, title, false, p)
                        .test_support()
                        // Keep canvas focus until click chooses commit or cancel.
                        .capture_any_mouse_down(|event, window, _| {
                            if event.button == MouseButton::Left {
                                window.prevent_default();
                            }
                        })
                        .on_click(cx.listener(move |this, _, window, cx| {
                            if cancel {
                                this.cancel_text_field(cx);
                            } else {
                                this.close_text_field(cx);
                            }
                            window.focus(&this.canvas_focus, cx);
                        }))
                        .into_any_element(),
                );
            }
        }
        v.push(
            mono(
                if self.type_tool.field.is_some() {
                    "Type on canvas ? Ctrl+Enter to finish ? Esc to cancel"
                } else if self.text_target().is_some() {
                    "Click text to edit ? double-click a word to select"
                } else {
                    "Click the canvas to place text"
                },
                10.,
                p.muted,
            )
            .into_any_element(),
        );
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
            chip("type-colour", "Text colour", self.tools.picker, p)
                .test_support()
                .on_click(cx.listener(move |this, _, window, cx| this.open_text_colour(window, cx)))
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
        self.close_text_field(cx);
        self.execute(Command::Rasterize { id }, cx);
    }
}

impl EditorView {
    fn text_screen_point(&self, spec: &TextSpec, local: (f32, f32)) -> Option<Point<Pixels>> {
        let bounds = self.canvas_bounds()?;
        let doc = spec
            .transform()
            .transform_point2(glam::dvec2(local.0 as f64, local.1 as f64));
        let screen = self.view.doc_to_screen((doc.x, doc.y), &bounds);
        Some(point(px(screen.0 as f32), px(screen.1 as f32)))
    }

    pub(crate) fn paint_text_editing(
        &self,
        bounds: Bounds<Pixels>,
        window: &mut Window,
        cx: &App,
        entity: Entity<Self>,
    ) {
        let Some(spec) = self.editing_text() else {
            return;
        };
        let Some(field) = &self.type_tool.field else {
            return;
        };
        window.handle_input(
            &self.canvas_focus,
            ElementInputHandler::new(bounds, entity),
            cx,
        );
        let layout = emulsion_core::text::layout(&spec);
        let accent = theme::palette(cx).accent;
        let paint_rect = |rect: emulsion_core::text::TextRect, color: Hsla, window: &mut Window| {
            let corners = [
                (rect.x, rect.y),
                (rect.x + rect.width, rect.y),
                (rect.x + rect.width, rect.y + rect.height),
                (rect.x, rect.y + rect.height),
            ];
            let points: Vec<_> = corners
                .into_iter()
                .filter_map(|p| self.text_screen_point(&spec, p))
                .collect();
            if points.len() != 4 {
                return;
            }
            let mut path = PathBuilder::fill();
            path.move_to(points[0]);
            for point in &points[1..] {
                path.line_to(*point);
            }
            path.close();
            if let Ok(path) = path.build() {
                window.paint_path(path, color);
            }
        };
        for rect in layout.selection(field.range()) {
            paint_rect(rect, accent.opacity(0.3), window);
        }
        if let Some(marked) = &field.marked {
            for mut rect in layout.selection(marked.clone()) {
                rect.y += rect.height - 1.;
                rect.height = 1.;
                paint_rect(rect, accent, window);
            }
        }
        let mut caret = layout.caret(field.cursor);
        let local_pixel = (1.5 / self.view.zoom.max(0.01)) as f32;
        if spec.vertical {
            caret.height = local_pixel / spec.scale_y.abs().max(0.01);
        } else {
            caret.width = local_pixel / spec.scale_x.abs().max(0.01);
        }
        paint_rect(caret, accent, window);
    }
}

impl EntityInputHandler for EditorView {
    fn text_for_range(
        &mut self,
        range: Range<usize>,
        adjusted: &mut Option<Range<usize>>,
        _: &mut Window,
        _: &mut Context<Self>,
    ) -> Option<String> {
        let spec = self.editing_text()?;
        let bytes = utf16_to_byte(&spec.text, range.start)..utf16_to_byte(&spec.text, range.end);
        *adjusted =
            Some(byte_to_utf16(&spec.text, bytes.start)..byte_to_utf16(&spec.text, bytes.end));
        Some(spec.text[bytes].to_string())
    }

    fn selected_text_range(
        &mut self,
        _: bool,
        _: &mut Window,
        _: &mut Context<Self>,
    ) -> Option<UTF16Selection> {
        let spec = self.editing_text()?;
        let field = self.type_tool.field.as_ref()?;
        let range = field.range();
        Some(UTF16Selection {
            range: byte_to_utf16(&spec.text, range.start)..byte_to_utf16(&spec.text, range.end),
            reversed: field.cursor < field.anchor,
        })
    }

    fn marked_text_range(&self, _: &mut Window, _: &mut Context<Self>) -> Option<Range<usize>> {
        let spec = self.editing_text()?;
        let range = self.type_tool.field.as_ref()?.marked.as_ref()?;
        Some(byte_to_utf16(&spec.text, range.start)..byte_to_utf16(&spec.text, range.end))
    }

    fn unmark_text(&mut self, _: &mut Window, cx: &mut Context<Self>) {
        if let Some(field) = &mut self.type_tool.field {
            field.marked = None;
            cx.notify();
        }
    }

    fn replace_text_in_range(
        &mut self,
        range: Option<Range<usize>>,
        text: &str,
        _: &mut Window,
        cx: &mut Context<Self>,
    ) {
        let Some(spec) = self.editing_text() else {
            return;
        };
        let Some(field) = &self.type_tool.field else {
            return;
        };
        let range = range
            .map(|r| utf16_to_byte(&spec.text, r.start)..utf16_to_byte(&spec.text, r.end))
            .or(field.marked.clone())
            .unwrap_or_else(|| field.range());
        self.replace_canvas_text(range, text, cx);
    }

    fn replace_and_mark_text_in_range(
        &mut self,
        range: Option<Range<usize>>,
        text: &str,
        selected: Option<Range<usize>>,
        _: &mut Window,
        cx: &mut Context<Self>,
    ) {
        let Some(spec) = self.editing_text() else {
            return;
        };
        let Some(field) = &self.type_tool.field else {
            return;
        };
        let range = range
            .map(|r| utf16_to_byte(&spec.text, r.start)..utf16_to_byte(&spec.text, r.end))
            .or(field.marked.clone())
            .unwrap_or_else(|| field.range());
        let start = range.start;
        self.replace_canvas_text(range, text, cx);
        if let Some(field) = &mut self.type_tool.field {
            field.marked = (!text.is_empty()).then_some(start..start + text.len());
            if let Some(selected) = selected {
                field.anchor = start + utf16_to_byte(text, selected.start);
                field.cursor = start + utf16_to_byte(text, selected.end);
            }
        }
        self.normalize_text_cursor();
        cx.notify();
    }

    fn bounds_for_range(
        &mut self,
        range: Range<usize>,
        _: Bounds<Pixels>,
        _: &mut Window,
        _: &mut Context<Self>,
    ) -> Option<Bounds<Pixels>> {
        let spec = self.editing_text()?;
        let layout = emulsion_core::text::layout(&spec);
        let byte = utf16_to_byte(&spec.text, range.start);
        let rect = layout.caret(byte);
        let points: Vec<_> = [
            (rect.x, rect.y),
            (rect.x + rect.width, rect.y),
            (rect.x + rect.width, rect.y + rect.height),
            (rect.x, rect.y + rect.height),
        ]
        .into_iter()
        .filter_map(|p| self.text_screen_point(&spec, p))
        .collect();
        let first = *points.first()?;
        let (mut min, mut max) = (first, first);
        for p in points {
            min.x = min.x.min(p.x);
            min.y = min.y.min(p.y);
            max.x = max.x.max(p.x);
            max.y = max.y.max(p.y);
        }
        Some(Bounds::from_corners(min, max))
    }

    fn character_index_for_point(
        &mut self,
        point: Point<Pixels>,
        _: &mut Window,
        _: &mut Context<Self>,
    ) -> Option<usize> {
        let spec = self.editing_text()?;
        let doc = self.doc_point(point)?;
        let local = spec
            .transform()
            .inverse()
            .transform_point2(glam::dvec2(doc.0, doc.1));
        let byte = emulsion_core::text::layout(&spec).hit(local.x as f32, local.y as f32);
        Some(byte_to_utf16(&spec.text, byte))
    }

    fn set_selected_text_range(
        &mut self,
        range: Range<usize>,
        _: &mut Window,
        cx: &mut Context<Self>,
    ) {
        let Some(spec) = self.editing_text() else {
            return;
        };
        if let Some(field) = &mut self.type_tool.field {
            field.anchor = utf16_to_byte(&spec.text, range.start);
            field.cursor = utf16_to_byte(&spec.text, range.end);
            cx.notify();
        }
    }

    fn text_length_utf16(&mut self, _: &mut Window, _: &mut Context<Self>) -> Option<usize> {
        Some(self.editing_text()?.text.encode_utf16().count())
    }

    fn accepts_text_input(&self, _: &mut Window, _: &mut Context<Self>) -> bool {
        self.type_tool.field.is_some()
    }
}
