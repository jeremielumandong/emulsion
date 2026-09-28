//! Diagram review and navigation controls backed by the shared model.
use super::*;
use emulsion_core::diagram::workspace as review;
use gpui_kit::component::{
    button::{Button, ButtonVariants},
    input::{Textarea, TextareaState},
};
impl EditorView {
    pub(crate) fn diagram_default_style(&mut self, reset: bool, cx: &mut Context<Self>) {
        let Some(id) = self.diagram_object() else {
            return;
        };
        let connector = self
            .editor
            .doc
            .diagram
            .as_ref()
            .is_some_and(|m| m.edges.contains_key(&id));
        let result = review::set_default_style(&mut self.editor, (!reset).then_some(id), connector);
        self.diagram_review_result(
            result,
            if reset {
                "Default style reset."
            } else {
                "Default style saved for new objects on this page."
            },
            cx,
        );
    }
    fn diagram_review_result(
        &mut self,
        result: Result<(), String>,
        success: &str,
        cx: &mut Context<Self>,
    ) -> bool {
        match result {
            Ok(()) => {
                self.after_change(cx);
                self.set_status(success, false, cx);
                true
            }
            Err(e) => {
                self.set_status(e, true, cx);
                false
            }
        }
    }
    pub(crate) fn diagram_thumbnail(&mut self, reset: bool, cx: &mut Context<Self>) {
        let ids = if reset {
            vec![]
        } else {
            self.selected_layer_roots()
        };
        let result = review::set_thumbnail(&mut self.editor, ids);
        self.diagram_review_result(result, "Page thumbnail updated.", cx);
    }
    pub(crate) fn diagram_copy_link(&mut self, selection: bool, cx: &mut Context<Self>) {
        let Some(path) = &self.editor.path else {
            self.set_status("Save the project before copying a diagram link.", false, cx);
            return;
        };
        let link = review::Link {
            project: Some(path.to_string_lossy().into()),
            page: self.editor.active_page(),
            nodes: if selection {
                self.selected_layer_roots()
            } else {
                vec![]
            },
            view: (!selection).then_some([
                self.view.center.0,
                self.view.center.1,
                self.view.zoom,
                self.view.rotation,
            ]),
        };
        match link.encode() {
            Ok(link) => {
                cx.write_to_clipboard(ClipboardItem::new_string(link));
                self.set_status(
                    "Link copied. Use Open diagram link in the matching project.",
                    false,
                    cx,
                );
            }
            Err(e) => self.set_status(e, true, cx),
        }
    }
    pub(crate) fn diagram_open_link(&mut self, window: &mut Window, cx: &mut Context<Self>) {
        let input = cx.new(|cx| InputState::new(window, cx).placeholder("emulsion://diagram/…"));
        let owner = cx.weak_entity();
        window.open_dialog(cx, move |dialog, _, _| {
            let input = input.clone();
            let owner = owner.clone();
            dialog
                .title("Open diagram link")
                .width(px(520.))
                .child(Input::new(&input).id("diagram-link-input"))
                .on_ok(move |_, _, cx| {
                    let text = input.read(cx).value().to_string();
                    owner
                        .update(cx, |v, cx| match v.diagram_follow_link(&text, cx) {
                            Ok(()) => true,
                            Err(e) => {
                                v.set_status(e, true, cx);
                                false
                            }
                        })
                        .unwrap_or(false)
                })
        });
    }
    pub(crate) fn diagram_follow_link(
        &mut self,
        text: &str,
        cx: &mut Context<Self>,
    ) -> Result<(), String> {
        if !self.prepare_page_action(cx) {
            return Err("Finish the current edit first".into());
        }
        let link = review::Link::decode(text)?;
        link.check_project(&self.editor)?;
        self.editor.set_active_page(link.page)?;
        self.after_change(cx);
        self.set_layer_selection(link.nodes.clone(), link.nodes.first().copied());
        if let Some([x, y, zoom, rotation]) = link.view {
            self.view = viewport::View {
                center: (x, y),
                zoom,
                rotation,
            };
            self.fit_pending = false;
        } else if !link.nodes.is_empty() {
            let bounds = link
                .nodes
                .iter()
                .filter_map(|id| emulsion_core::geometry::node_bounds(&self.editor.doc, *id))
                .reduce(|a, b| a.union(&b));
            if let (Some(b), Some(canvas)) = (bounds, self.canvas_bounds()) {
                self.view.fit(b.w.max(1) as u32, b.h.max(1) as u32, &canvas);
                self.view.center = (b.x as f64 + b.w as f64 / 2., b.y as f64 + b.h as f64 / 2.);
                self.fit_pending = false;
            }
        }
        cx.notify();
        Ok(())
    }
    pub(crate) fn diagram_edit_fields(
        &mut self,
        kind: diagram::ShapeKind,
        window: &mut Window,
        cx: &mut Context<Self>,
    ) {
        if !self.prepare_page_action(cx) {
            return;
        }
        let Some(id) = self.diagram_object() else {
            return;
        };
        let Some(shape) = self
            .editor
            .doc
            .diagram
            .as_ref()
            .and_then(|m| m.shapes.get(&id))
        else {
            return;
        };
        let value = diagram::structure::get(&self.editor.doc, id).unwrap_or_else(|| {
            let text = match &self.editor.doc.node(shape.label).unwrap().kind {
                NodeKind::Text { spec, .. } => spec.text.as_str(),
                _ => "Object",
            };
            diagram::structure::StructuredObject::from_text(text)
        });
        let title = cx.new(|cx| InputState::new(window, cx).default_value(value.title));
        let fields = cx.new(|cx| {
            TextareaState::new(window, cx)
                .rows(5)
                .default_value(value.attributes.join("\n"))
        });
        let methods = cx.new(|cx| {
            TextareaState::new(window, cx)
                .rows(4)
                .default_value(value.methods.join("\n"))
        });
        let owner = cx.weak_entity();
        let ticket = self.edit_ticket();
        window.open_dialog(cx, move |dialog, _, _| {
            let title = title.clone();
            let fields = fields.clone();
            let methods = methods.clone();
            let owner = owner.clone();
            let mut dialog = dialog
                .title(if kind == diagram::ShapeKind::Class {
                    "UML class"
                } else {
                    "ER entity"
                })
                .width(px(520.))
                .child("Name")
                .child(Input::new(&title).id("diagram-structure-title"))
                .child("Fields — one per line")
                .child(
                    div()
                        .id("diagram-structure-fields")
                        .test_support()
                        .child(Textarea::new(&fields)),
                );
            if kind == diagram::ShapeKind::Class {
                dialog = dialog.child("Methods — one per line").child(
                    div()
                        .id("diagram-structure-methods")
                        .test_support()
                        .child(Textarea::new(&methods)),
                );
            }
            dialog.on_ok(move |_, _, cx| {
                let lines = |text: String| {
                    text.lines()
                        .filter(|s| !s.trim().is_empty())
                        .map(str::to_owned)
                        .collect()
                };
                let value = diagram::structure::StructuredObject {
                    title: title.read(cx).value().to_string(),
                    attributes: lines(fields.read(cx).value().to_string()),
                    methods: if kind == diagram::ShapeKind::Class {
                        lines(methods.read(cx).value().to_string())
                    } else {
                        vec![]
                    },
                };
                owner
                    .update(cx, |v, cx| {
                        if v.edit_ticket() != ticket {
                            return false;
                        }
                        let result = diagram::structure::set(&mut v.editor, id, kind, value);
                        v.diagram_review_result(result, "Object fields updated.", cx)
                    })
                    .unwrap_or(false)
            })
        });
    }
    pub(crate) fn diagram_comments(&mut self, window: &mut Window, cx: &mut Context<Self>) {
        if let Some(id) = self.diagram_object() {
            self.diagram_comment_dialog(id, None, window, cx);
        }
    }
    fn diagram_comment_dialog(
        &mut self,
        id: NodeId,
        reply: Option<u64>,
        window: &mut Window,
        cx: &mut Context<Self>,
    ) {
        if !self.prepare_page_action(cx) {
            return;
        }
        let threads = self
            .editor
            .doc
            .diagram
            .as_ref()
            .map(|m| {
                m.settings
                    .threads
                    .iter()
                    .filter(|(_, t)| t.object == id)
                    .map(|(id, t)| (*id, t.clone()))
                    .collect::<Vec<_>>()
            })
            .unwrap_or_default();
        let input = cx.new(|cx| {
            TextareaState::new(window, cx)
                .rows(3)
                .placeholder(if reply.is_some() {
                    "Write a reply"
                } else {
                    "Start a comment thread"
                })
        });
        let owner = cx.weak_entity();
        let ticket = self.edit_ticket();
        window.open_dialog(cx, move |dialog, _, _| {
            let mut list = div()
                .id("diagram-comment-list")
                .max_h(px(320.))
                .overflow_y_scroll()
                .flex()
                .flex_col()
                .gap_3();
            for (thread, t) in &threads {
                let thread = *thread;
                let resolved = t.resolved;
                let mut card = div().p_2().flex().flex_col().gap_1().child(if resolved {
                    "Resolved"
                } else {
                    "Open"
                });
                for m in &t.messages {
                    card = card.child(div().text_sm().child(format!("{}: {}", m.author, m.text)));
                }
                let resolve_owner = owner.clone();
                let reply_owner = owner.clone();
                let delete_owner = owner.clone();
                card = card.child(
                    div()
                        .flex()
                        .gap_2()
                        .child(
                            Button::new(("diagram-comment-reply", thread))
                                .label("Reply")
                                .on_click(move |_, window, cx| {
                                    reply_owner
                                        .update(cx, |v, cx| {
                                            if v.edit_ticket() == ticket {
                                                window.close_dialog(cx);
                                                v.diagram_comment_dialog(
                                                    id,
                                                    Some(thread),
                                                    window,
                                                    cx,
                                                );
                                            }
                                        })
                                        .ok();
                                }),
                        )
                        .child(
                            Button::new(("diagram-comment-resolve", thread))
                                .label(if resolved { "Reopen" } else { "Resolve" })
                                .on_click(move |_, window, cx| {
                                    resolve_owner
                                        .update(cx, |v, cx| {
                                            if v.edit_ticket() == ticket {
                                                let result = review::resolve_comment(
                                                    &mut v.editor,
                                                    thread,
                                                    !resolved,
                                                );
                                                if v.diagram_review_result(
                                                    result,
                                                    "Comment updated.",
                                                    cx,
                                                ) {
                                                    window.close_dialog(cx);
                                                }
                                            }
                                        })
                                        .ok();
                                }),
                        )
                        .child(
                            Button::new(("diagram-comment-delete", thread))
                                .label("Delete")
                                .ghost()
                                .on_click(move |_, window, cx| {
                                    delete_owner
                                        .update(cx, |v, cx| {
                                            if v.edit_ticket() == ticket {
                                                let result = review::delete_comment_thread(
                                                    &mut v.editor,
                                                    thread,
                                                );
                                                if v.diagram_review_result(
                                                    result,
                                                    "Comment removed. Undo restores it.",
                                                    cx,
                                                ) {
                                                    window.close_dialog(cx);
                                                }
                                            }
                                        })
                                        .ok();
                                }),
                        ),
                );
                list = list.child(card);
            }
            let input = input.clone();
            let owner = owner.clone();
            dialog
                .title(if reply.is_some() {
                    "Reply to comment"
                } else {
                    "Object comments"
                })
                .width(px(560.))
                .child(list)
                .child(
                    div()
                        .id("diagram-comment-input")
                        .test_support()
                        .child(Textarea::new(&input)),
                )
                .on_ok(move |_, _, cx| {
                    let text = input.read(cx).value().to_string();
                    owner
                        .update(cx, |v, cx| {
                            if v.edit_ticket() != ticket {
                                return false;
                            }
                            let result =
                                review::add_comment(&mut v.editor, id, reply, "You", &text)
                                    .map(|_| ());
                            v.diagram_review_result(result, "Comment saved locally.", cx)
                        })
                        .unwrap_or(false)
                })
        });
    }
}
