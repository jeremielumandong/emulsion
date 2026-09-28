//! Object controls for diagrams; mutations use the shared command/history path.
use super::*;
#[path = "diagram_connector_menu.rs"]
mod connectors;
#[path = "diagram_workspace_ui.rs"]
mod workspace_ui;
use emulsion_core::{
    command::Alignment,
    layer_links::{Arrange, ArrangeTarget, Distribution},
};
use gpui_kit::component::{
    input::{Textarea, TextareaState},
    menu::PopupMenu,
};

pub(super) use emulsion_core::diagram::ObjectStyle;

fn item(
    editor: &Entity<EditorView>,
    label: &str,
    enabled: bool,
    action: impl Fn(&mut EditorView, &mut Window, &mut Context<EditorView>) + 'static,
) -> PopupMenuItem {
    let owner = editor.downgrade();
    PopupMenuItem::new(label.to_string())
        .disabled(!enabled)
        .on_click(move |_, window, cx| {
            owner
                .update(cx, |view, cx| {
                    if view.drag.is_none() && !view.editor.in_transaction() {
                        action(view, window, cx);
                    }
                })
                .ok();
        })
}

impl EditorView {
    pub(crate) fn diagram_context_select(
        &mut self,
        position: Point<Pixels>,
        cx: &mut Context<Self>,
    ) {
        if !self.is_diagram() || self.drag.is_some() {
            return;
        }
        let Some(point) = self.doc_point(position) else {
            return;
        };
        let id = self
            .diagram_hit(point)
            .map(|e| e.shape)
            .or_else(|| self.diagram_edge_hit(point))
            .map(|id| self.diagram_selection_root(id));
        if let Some(id) = id {
            if !self.layer_is_selected(id) {
                self.set_layer_selection(vec![id], Some(id));
            }
        } else {
            self.set_layer_selection(vec![], None);
        }
        cx.notify();
    }
    pub(crate) fn diagram_copy_style(&mut self, cx: &mut Context<Self>) {
        let Some(id) = self.diagram_object() else {
            return;
        };
        match ObjectStyle::capture(&self.editor.doc, id) {
            Ok(style) => {
                self.diagram_ui.copied_style = Some(style);
                self.set_status("Object style copied.", false, cx);
            }
            Err(error) => self.set_status(error, true, cx),
        }
    }
    pub(crate) fn diagram_paste_style(&mut self, cx: &mut Context<Self>) {
        if !self.prepare_page_action(cx) {
            return;
        }
        let Some(style) = &self.diagram_ui.copied_style else {
            return;
        };
        match style.commands(&self.editor.doc, &self.selected_layer_roots()) {
            Ok(commands) => {
                self.execute_layer_commands("Paste object style", commands, cx);
            }
            Err(error) => self.set_status(error, true, cx),
        }
    }
    pub(crate) fn diagram_toggle_lock(&mut self, cx: &mut Context<Self>) {
        let ids = self.selected_layer_roots();
        let unlock = ids
            .iter()
            .any(|id| self.editor.doc.node(*id).is_some_and(|n| n.locked));
        self.execute_layer_commands(
            if unlock {
                "Unlock objects"
            } else {
                "Lock objects"
            },
            ids.into_iter()
                .map(|id| Command::SetLocked {
                    id,
                    locked: !unlock,
                })
                .collect(),
            cx,
        );
    }
    pub(crate) fn diagram_order_to_end(&mut self, front: bool, cx: &mut Context<Self>) {
        // One move per selected root, rather than moving through every sibling.
        let mut ids = self.selected_layer_roots();
        if !front {
            ids.reverse();
        }
        let commands = ids
            .into_iter()
            .filter_map(|id| {
                self.editor.doc.node(id).map(|n| Command::MoveNode {
                    id,
                    slot: emulsion_core::command::Slot {
                        parent: n.parent,
                        index: if front { usize::MAX } else { 0 },
                    },
                })
            })
            .collect();
        self.execute_layer_commands("Arrange objects", commands, cx);
    }
    fn diagram_edit_caption(&mut self, window: &mut Window, cx: &mut Context<Self>) {
        self.diagram_properties(window, cx);
    }
    pub(crate) fn diagram_save_annotation(
        &mut self,
        id: NodeId,
        key: &str,
        value: String,
        cx: &mut Context<Self>,
    ) -> bool {
        let key = if key == "drawio_link" { "link" } else { key };
        match diagram::object_details_commands(
            &self.editor.doc,
            id,
            &std::collections::BTreeMap::from([(key.into(), value)]),
        ) {
            Ok(commands) => self
                .execute_layer_commands("Object details", commands, cx)
                .is_some(),
            Err(error) => {
                self.set_status(error, true, cx);
                false
            }
        }
    }
    fn diagram_annotation_dialog(
        &mut self,
        key: &'static str,
        title: &'static str,
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
            .and_then(|d| d.shapes.get(&id))
        else {
            return;
        };
        let value = shape.data.get(key).cloned().unwrap_or_default();
        let input = cx.new(|cx| TextareaState::new(window, cx).rows(8).default_value(value));
        let owner = cx.weak_entity();
        let ticket = self.edit_ticket();
        window.open_dialog(cx, move |dialog, _, _| {
            let input_ok = input.clone();
            let owner = owner.clone();
            dialog
                .title(title)
                .width(px(720.))
                .child(
                    div()
                        .id("diagram-object-detail-input")
                        .test_support()
                        .flex_shrink_0()
                        .child(Textarea::new(&input).h(rems(12.)).aria_label(title)),
                )
                .footer(crate::widgets::form_dialog_footer("Save"))
                .on_ok(move |_, _, cx| {
                    let value = input_ok.read(cx).value().to_string();
                    owner
                        .update(cx, |view, cx| {
                            if view.edit_ticket() != ticket {
                                return false;
                            }
                            view.diagram_save_annotation(id, key, value, cx)
                        })
                        .unwrap_or(false)
                })
        });
    }
    pub(crate) fn diagram_object_menu(
        menu: PopupMenu,
        editor: &Entity<Self>,
        window: &mut Window,
        cx: &mut Context<PopupMenu>,
    ) -> PopupMenu {
        let view = editor.read(cx);
        let ids = view.selected_layer_roots();
        let single = ids.len() == 1;
        let ready = !ids.is_empty()
            && view.drag.is_none()
            && !view.editor.in_transaction()
            && !view.assistant.running;
        let editable = ready
            && ids
                .iter()
                .all(|id| view.editor.doc.locked_ancestor(*id).is_none());
        let shape = single
            && view.diagram_object().is_some_and(|id| {
                view.editor
                    .doc
                    .diagram
                    .as_ref()
                    .unwrap()
                    .shapes
                    .contains_key(&id)
            });
        let object = single && view.diagram_object().is_some();
        let unlock = ids
            .iter()
            .any(|id| view.editor.doc.node(*id).is_some_and(|n| n.locked));
        let ungroup = single && view.editor.doc.node(ids[0]).is_some_and(Node::is_group) && !object;
        let paste_style = view.diagram_ui.copied_style.is_some();
        let focus = view.canvas_focus.clone();
        let paste_ready =
            !view.assistant.running && view.drag.is_none() && !view.editor.in_transaction();
        let paste = paste_ready
            && cx.read_from_clipboard().is_some_and(|c| {
                c.entries
                    .iter()
                    .any(|e| matches!(e, gpui_kit::ClipboardEntry::Image(_)))
            });
        let mut menu = menu
            .action_context(focus)
            .menu_with_disabled("Cut", Box::new(crate::actions::CutPixels), !editable)
            .menu_with_disabled("Copy", Box::new(crate::actions::CopyPixels), !ready)
            .menu_with_disabled("Paste", Box::new(crate::actions::PastePixels), !paste)
            .item(item(editor, "Duplicate", editable, |v, _, cx| {
                v.duplicate_selected(cx)
            }))
            .item(item(editor, "Delete", editable, |v, _, cx| {
                v.delete_selected(cx)
            }))
            .separator()
            .item(item(editor, "Copy as PNG", ready, |v, _, cx| {
                v.copy_pixels(cx)
            }));
        let e = editor.clone();
        menu = menu
            .separator()
            .submenu("Arrange", window, cx, move |menu, _, _| {
                menu.item(item(&e, "Bring to front", editable, |v, _, cx| {
                    v.diagram_order_to_end(true, cx)
                }))
                .item(item(&e, "Send to back", editable, |v, _, cx| {
                    v.diagram_order_to_end(false, cx)
                }))
                .item(item(&e, "Bring forward", editable, |v, _, cx| {
                    v.shift_selected(true, cx)
                }))
                .item(item(&e, "Send backward", editable, |v, _, cx| {
                    v.shift_selected(false, cx)
                }))
            });
        let e = editor.clone();
        let count = ids.len();
        menu = menu.submenu("Align and distribute", window, cx, move |mut menu, _, _| {
            for (name, alignment) in [
                ("Align left", Alignment::Left),
                ("Center horizontally", Alignment::HorizontalCenter),
                ("Align right", Alignment::Right),
                ("Align top", Alignment::Top),
                ("Center vertically", Alignment::VerticalCenter),
                ("Align bottom", Alignment::Bottom),
            ] {
                menu = menu.item(item(&e, name, editable, move |v, _, cx| {
                    v.arrange_selected(
                        Arrange::Align(alignment),
                        if count > 1 {
                            ArrangeTarget::SelectedLayers
                        } else {
                            ArrangeTarget::Canvas
                        },
                        cx,
                    )
                }));
            }
            menu.separator()
                .item(item(
                    &e,
                    "Distribute horizontally",
                    editable && count >= 3,
                    |v, _, cx| {
                        v.arrange_selected(
                            Arrange::Distribute(Distribution::HorizontalGap),
                            ArrangeTarget::SelectedLayers,
                            cx,
                        )
                    },
                ))
                .item(item(
                    &e,
                    "Distribute vertically",
                    editable && count >= 3,
                    |v, _, cx| {
                        v.arrange_selected(
                            Arrange::Distribute(Distribution::VerticalGap),
                            ArrangeTarget::SelectedLayers,
                            cx,
                        )
                    },
                ))
        });
        menu = menu
            .item(item(
                editor,
                "Group",
                editable && ids.len() > 1,
                |v, _, cx| v.group_selected(cx),
            ))
            .item(item(editor, "Ungroup", editable && ungroup, |v, _, cx| {
                v.ungroup_selected(cx)
            }))
            .item(item(
                editor,
                if unlock { "Unlock" } else { "Lock" },
                ready,
                |v, _, cx| v.diagram_toggle_lock(cx),
            ))
            .separator()
            .item(item(editor, "Copy style", ready && object, |v, _, cx| {
                v.diagram_copy_style(cx)
            }))
            .item(item(
                editor,
                "Set default style",
                ready && object,
                |v, _, cx| v.diagram_default_style(false, cx),
            ))
            .item(item(
                editor,
                "Reset default style",
                ready && object,
                |v, _, cx| v.diagram_default_style(true, cx),
            ))
            .item(item(
                editor,
                "Paste style",
                editable && paste_style,
                |v, _, cx| v.diagram_paste_style(cx),
            ))
            .item(item(editor, "Fill color…", editable, |v, w, cx| {
                v.diagram_color_dialog("fill", w, cx)
            }))
            .item(item(editor, "Line color…", editable, |v, w, cx| {
                v.diagram_color_dialog("stroke", w, cx)
            }))
            .item(item(editor, "Text color…", editable, |v, w, cx| {
                v.diagram_color_dialog("text", w, cx)
            }))
            .item(item(
                editor,
                "Edit text and properties…",
                editable && object,
                |v, w, cx| v.diagram_edit_caption(w, cx),
            ))
            .separator()
            .item(item(
                editor,
                "Add / edit note…",
                editable && shape,
                |v, w, cx| v.diagram_annotation_dialog("note", "Object note", w, cx),
            ))
            .item(item(
                editor,
                "Add / edit link…",
                editable && shape,
                |v, w, cx| v.diagram_annotation_dialog("drawio_link", "Object link", w, cx),
            ))
            .item(item(
                editor,
                "Add / edit alt text…",
                editable && shape,
                |v, w, cx| v.diagram_annotation_dialog("alt_text", "Alternative text", w, cx),
            ))
            .separator()
            .item(item(
                editor,
                "Edit UML fields…",
                ready && object,
                |v, w, cx| v.diagram_edit_fields(diagram::ShapeKind::Class, w, cx),
            ))
            .item(item(
                editor,
                "Edit ER fields…",
                ready && object,
                |v, w, cx| v.diagram_edit_fields(diagram::ShapeKind::Entity, w, cx),
            ))
            .item(item(editor, "Comments…", ready && object, |v, w, cx| {
                v.diagram_comments(w, cx)
            }))
            .item(item(editor, "Copy link to selection", ready, |v, _, cx| {
                v.diagram_copy_link(true, cx)
            }))
            .item(item(editor, "Copy link to view", true, |v, _, cx| {
                v.diagram_copy_link(false, cx)
            }))
            .item(item(editor, "Open diagram link…", true, |v, w, cx| {
                v.diagram_open_link(w, cx)
            }))
            .item(item(editor, "Set as thumbnail", ready, |v, _, cx| {
                v.diagram_thumbnail(false, cx)
            }))
            .item(item(editor, "Reset thumbnail", true, |v, _, cx| {
                v.diagram_thumbnail(true, cx)
            }))
            .item(item(editor, "Export selection…", ready, |v, w, cx| {
                v.show_selection_export(w, cx)
            }));
        menu
    }
    pub(crate) fn diagram_object_toolbar(
        &mut self,
        p: &Palette,
        cx: &mut Context<Self>,
    ) -> Option<AnyElement> {
        if !self.is_diagram()
            || self.previewing()
            || self.before_active()
            || self.drag.is_some()
            || self.tool != Tool::Move
            || self.diagram_ui.connecting
            || self.diagram_ui.marquee.is_some()
        {
            return None;
        }
        let ids = self.selected_layer_roots();
        if ids.is_empty() {
            return None;
        }
        let bounds = ids
            .iter()
            .filter_map(|id| emulsion_core::geometry::node_bounds(&self.editor.doc, *id))
            .reduce(|a, b| a.union(&b))?;
        let canvas = self.canvas_bounds()?;
        let points = [
            (bounds.x, bounds.y),
            (bounds.right(), bounds.y),
            (bounds.x, bounds.bottom()),
            (bounds.right(), bounds.bottom()),
        ]
        .map(|(x, y)| self.view.doc_to_screen((x as f64, y as f64), &canvas));
        let left = points.iter().map(|p| p.0).fold(f64::INFINITY, f64::min)
            - f32::from(canvas.origin.x) as f64;
        let right = points.iter().map(|p| p.0).fold(f64::NEG_INFINITY, f64::max)
            - f32::from(canvas.origin.x) as f64;
        let top = points.iter().map(|p| p.1).fold(f64::INFINITY, f64::min)
            - f32::from(canvas.origin.y) as f64;
        let bottom = points.iter().map(|p| p.1).fold(f64::NEG_INFINITY, f64::max)
            - f32::from(canvas.origin.y) as f64;
        if right < 0.
            || left > f32::from(canvas.size.width) as f64
            || bottom < 0.
            || top > f32::from(canvas.size.height) as f64
        {
            return None;
        }
        let connector = if ids.len() == 1 {
            self.diagram_object().and_then(|id| {
                self.editor
                    .doc
                    .diagram
                    .as_ref()?
                    .edges
                    .get(&id)
                    .cloned()
                    .map(|e| (id, e))
            })
        } else {
            None
        };
        let width = if connector.is_some() { 460. } else { 260. };
        let x = (((left + right) / 2.) as f32 - width / 2.)
            .clamp(8., (f32::from(canvas.size.width) - width - 8.).max(8.));
        let y = (top as f32 - 48.).clamp(8., (f32::from(canvas.size.height) - 44.).max(8.));
        let locked = ids
            .iter()
            .any(|id| self.editor.doc.locked_ancestor(*id).is_some());
        let object = ids.len() == 1 && self.diagram_object().is_some();
        let shape = object
            && self.diagram_object().is_some_and(|id| {
                self.editor
                    .doc
                    .diagram
                    .as_ref()
                    .unwrap()
                    .shapes
                    .contains_key(&id)
            });
        if let Some((id, edge)) = connector {
            return Some(self.diagram_connector_toolbar(id, edge, x, y, p, cx));
        }
        let button = |id, label, icon| {
            Button::new(id)
                .accessibility_label(label)
                .tooltip(label)
                .xsmall()
                .ghost()
                .size(px(32.))
                .child(rail::tool_icon(icon).size(px(16.)).text_color(p.ink))
        };
        let owner = cx.weak_entity();
        Some(
            div()
                .id("diagram-object-toolbar")
                .test_support()
                .absolute()
                .left(px(x))
                .top(px(y))
                .w(px(width))
                .h(px(40.))
                .flex()
                .items_center()
                .gap(px(3.))
                .p(px(4.))
                .rounded(px(8.))
                .bg(p.panel)
                .border_1()
                .border_color(p.line)
                .shadow_md()
                .occlude()
                .on_mouse_down(MouseButton::Left, |_, _, cx| cx.stop_propagation())
                .child(
                    button("diagram-object-fill", "Fill color", "paint-bucket")
                        .disabled(locked)
                        .on_click(cx.listener(|v, _, w, cx| v.diagram_color_dialog("fill", w, cx))),
                )
                .child(
                    button("diagram-object-stroke", "Line color", "circle")
                        .disabled(locked)
                        .on_click(
                            cx.listener(|v, _, w, cx| v.diagram_color_dialog("stroke", w, cx)),
                        ),
                )
                .child(
                    button("diagram-object-text", "Edit text", "type")
                        .disabled(locked || !object)
                        .on_click(cx.listener(|v, _, w, cx| v.diagram_edit_caption(w, cx))),
                )
                .child(
                    button(
                        "diagram-object-lock",
                        if locked {
                            "Unlock objects"
                        } else {
                            "Lock objects"
                        },
                        if locked { "lock" } else { "unlock" },
                    )
                    .on_click(cx.listener(|v, _, _, cx| v.diagram_toggle_lock(cx))),
                )
                .child(
                    button("diagram-object-link", "Edit link", "link")
                        .disabled(locked || !shape)
                        .on_click(cx.listener(|v, _, w, cx| {
                            v.diagram_annotation_dialog("drawio_link", "Object link", w, cx)
                        })),
                )
                .child(
                    button("diagram-object-duplicate", "Duplicate", "copy")
                        .disabled(locked)
                        .on_click(cx.listener(|v, _, _, cx| v.duplicate_selected(cx))),
                )
                .child(
                    button("diagram-object-more", "More object actions", "ellipsis").dropdown_menu(
                        move |menu, w, cx| {
                            let Some(editor) = owner.upgrade() else {
                                return menu;
                            };
                            Self::diagram_object_menu(menu, &editor, w, cx)
                        },
                    ),
                )
                .into_any_element(),
        )
    }
}
