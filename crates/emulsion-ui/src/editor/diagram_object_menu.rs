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
    label: impl Into<SharedString>,
    enabled: bool,
    action: impl Fn(&mut EditorView, &mut Window, &mut Context<EditorView>) + 'static,
) -> PopupMenuItem {
    let owner = editor.downgrade();
    PopupMenuItem::new(label.into())
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
        if !self.is_diagram() || self.drag.is_some() || self.diagram_ui.endpoint_drag.is_some() {
            return;
        }
        let Some(point) = self.doc_point(position) else {
            return;
        };
        let id = self.diagram_active_hit(point).or_else(|| {
            self.diagram_hit(point)
                .map(|e| self.diagram_selection_root(e.shape))
        });
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
                self.set_status(t!("editor.diagram_object_menu.style_copied"), false, cx);
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
        title: SharedString,
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
                .title(title.clone())
                .width(px(720.))
                .child(
                    div()
                        .id("diagram-object-detail-input")
                        .test_support()
                        .flex_shrink_0()
                        .child(Textarea::new(&input).h(rems(12.)).aria_label(title.clone())),
                )
                .footer(crate::widgets::form_dialog_footer(t!("file.save")))
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
            .menu_with_disabled(
                t!("edit.cut"),
                Box::new(crate::actions::CutPixels),
                !editable,
            )
            .menu_with_disabled(
                t!("edit.copy"),
                Box::new(crate::actions::CopyPixels),
                !ready,
            )
            .menu_with_disabled(
                t!("edit.paste"),
                Box::new(crate::actions::PastePixels),
                !paste,
            )
            .item(item(
                editor,
                t!("design.direct.duplicate"),
                editable,
                |v, _, cx| v.duplicate_selected(cx),
            ))
            .item(item(
                editor,
                t!("design.direct.delete"),
                editable,
                |v, _, cx| v.delete_selected(cx),
            ))
            .separator()
            .item(item(
                editor,
                t!("editor.diagram_object_menu.copy_png"),
                ready,
                |v, _, cx| v.copy_pixels(cx),
            ));
        let e = editor.clone();
        menu = menu.separator().submenu(
            t!("design.direct.arrange"),
            window,
            cx,
            move |menu, _, _| {
                menu.item(item(&e, t!("design.direct.front"), editable, |v, _, cx| {
                    v.diagram_order_to_end(true, cx)
                }))
                .item(item(&e, t!("design.direct.back"), editable, |v, _, cx| {
                    v.diagram_order_to_end(false, cx)
                }))
                .item(item(
                    &e,
                    t!("design.direct.forward"),
                    editable,
                    |v, _, cx| v.shift_selected(true, cx),
                ))
                .item(item(
                    &e,
                    t!("design.direct.backward"),
                    editable,
                    |v, _, cx| v.shift_selected(false, cx),
                ))
            },
        );
        let e = editor.clone();
        let count = ids.len();
        menu = menu.submenu(
            t!("editor.diagram_object_menu.align_distribute"),
            window,
            cx,
            move |mut menu, _, _| {
                for (name, alignment) in [
                    ("editor.diagram_object_menu.align_left", Alignment::Left),
                    (
                        "editor.diagram_object_menu.center_horizontally",
                        Alignment::HorizontalCenter,
                    ),
                    ("editor.diagram_object_menu.align_right", Alignment::Right),
                    ("editor.diagram_object_menu.align_top", Alignment::Top),
                    (
                        "editor.diagram_object_menu.center_vertically",
                        Alignment::VerticalCenter,
                    ),
                    ("editor.diagram_object_menu.align_bottom", Alignment::Bottom),
                ] {
                    menu = menu.item(item(&e, t!(name), editable, move |v, _, cx| {
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
                        t!("editor.diagram_object_menu.distribute_horizontally"),
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
                        t!("editor.diagram_object_menu.distribute_vertically"),
                        editable && count >= 3,
                        |v, _, cx| {
                            v.arrange_selected(
                                Arrange::Distribute(Distribution::VerticalGap),
                                ArrangeTarget::SelectedLayers,
                                cx,
                            )
                        },
                    ))
            },
        );
        menu = menu
            .item(item(
                editor,
                t!("design.direct.group"),
                editable && ids.len() > 1,
                |v, _, cx| v.group_selected(cx),
            ))
            .item(item(
                editor,
                t!("design.direct.ungroup"),
                editable && ungroup,
                |v, _, cx| v.ungroup_selected(cx),
            ))
            .item(item(
                editor,
                if unlock {
                    t!("design.direct.unlock")
                } else {
                    t!("design.direct.lock")
                },
                ready,
                |v, _, cx| v.diagram_toggle_lock(cx),
            ))
            .separator()
            .item(item(
                editor,
                t!("editor.diagram_object_menu.copy_style"),
                ready && object,
                |v, _, cx| v.diagram_copy_style(cx),
            ))
            .item(item(
                editor,
                t!("editor.diagram_object_menu.set_default_style"),
                ready && object,
                |v, _, cx| v.diagram_default_style(false, cx),
            ))
            .item(item(
                editor,
                t!("editor.diagram_object_menu.reset_default_style"),
                ready && object,
                |v, _, cx| v.diagram_default_style(true, cx),
            ))
            .item(item(
                editor,
                t!("editor.diagram_object_menu.paste_style"),
                editable && paste_style,
                |v, _, cx| v.diagram_paste_style(cx),
            ))
            .item(item(
                editor,
                t!("editor.diagram_object_menu.fill_color_menu"),
                editable,
                |v, w, cx| v.diagram_color_dialog("fill", w, cx),
            ))
            .item(item(
                editor,
                t!("editor.diagram_object_menu.line_color_menu"),
                editable,
                |v, w, cx| v.diagram_color_dialog("stroke", w, cx),
            ))
            .item(item(
                editor,
                t!("editor.diagram_object_menu.text_color_menu"),
                editable,
                |v, w, cx| v.diagram_color_dialog("text", w, cx),
            ))
            .item(item(
                editor,
                t!("editor.diagram_object_menu.edit_text_properties"),
                editable && object,
                |v, w, cx| v.diagram_edit_caption(w, cx),
            ))
            .separator()
            .item(item(
                editor,
                t!("editor.diagram_object_menu.edit_note"),
                editable && shape,
                |v, w, cx| {
                    v.diagram_annotation_dialog(
                        "note",
                        t!("editor.diagram_object_menu.note_title").into(),
                        w,
                        cx,
                    )
                },
            ))
            .item(item(
                editor,
                t!("editor.diagram_object_menu.edit_link_menu"),
                editable && shape,
                |v, w, cx| {
                    v.diagram_annotation_dialog(
                        "drawio_link",
                        t!("editor.diagram_object_menu.link_title").into(),
                        w,
                        cx,
                    )
                },
            ))
            .item(item(
                editor,
                t!("editor.diagram_object_menu.edit_alt_text"),
                editable && shape,
                |v, w, cx| {
                    v.diagram_annotation_dialog(
                        "alt_text",
                        t!("editor.diagram_object_menu.alt_text_title").into(),
                        w,
                        cx,
                    )
                },
            ))
            .separator()
            .item(item(
                editor,
                t!("editor.diagram_object_menu.edit_uml"),
                ready && object,
                |v, w, cx| v.diagram_edit_fields(diagram::ShapeKind::Class, w, cx),
            ))
            .item(item(
                editor,
                t!("editor.diagram_object_menu.edit_er"),
                ready && object,
                |v, w, cx| v.diagram_edit_fields(diagram::ShapeKind::Entity, w, cx),
            ))
            .item(item(
                editor,
                t!("editor.diagram_object_menu.comments"),
                ready && object,
                |v, w, cx| v.diagram_comments(w, cx),
            ))
            .item(item(
                editor,
                t!("editor.diagram_object_menu.copy_link_selection"),
                ready,
                |v, _, cx| v.diagram_copy_link(true, cx),
            ))
            .item(item(
                editor,
                t!("editor.diagram_object_menu.copy_link_view"),
                true,
                |v, _, cx| v.diagram_copy_link(false, cx),
            ))
            .item(item(
                editor,
                t!("editor.diagram_object_menu.open_link"),
                true,
                |v, w, cx| v.diagram_open_link(w, cx),
            ))
            .item(item(
                editor,
                t!("editor.diagram_object_menu.set_thumbnail"),
                ready,
                |v, _, cx| v.diagram_thumbnail(false, cx),
            ))
            .item(item(
                editor,
                t!("editor.diagram_object_menu.reset_thumbnail"),
                true,
                |v, _, cx| v.diagram_thumbnail(true, cx),
            ))
            .item(item(
                editor,
                t!("editor.diagram_object_menu.export_selection"),
                ready,
                |v, w, cx| v.show_selection_export(w, cx),
            ));
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
            .filter_map(|id| {
                emulsion_core::geometry::node_bounds(&self.editor.doc, *id)
                    .ok()
                    .flatten()
            })
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
        let width = if connector.is_some() { 495. } else { 295. };
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
        let button = |id, label: SharedString, icon| {
            Button::new(id)
                .accessibility_label(label.clone())
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
                    button(
                        "diagram-object-fill",
                        t!("editor.diagram_object_menu.fill_color").into(),
                        "paint-bucket",
                    )
                    .disabled(locked)
                    .on_click(cx.listener(|v, _, w, cx| v.diagram_color_dialog("fill", w, cx))),
                )
                .child(
                    button(
                        "diagram-object-stroke",
                        t!("editor.diagram_object_menu.line_color").into(),
                        "circle",
                    )
                    .disabled(locked)
                    .on_click(cx.listener(|v, _, w, cx| v.diagram_color_dialog("stroke", w, cx))),
                )
                .child(
                    Button::new("diagram-object-text-color")
                        .label("A̲")
                        .accessibility_label(t!("editor.diagram_object_menu.text_color"))
                        .tooltip(t!("editor.diagram_object_menu.text_color"))
                        .xsmall()
                        .ghost()
                        .size(px(32.))
                        .disabled(locked)
                        .on_click(cx.listener(|v, _, w, cx| v.diagram_color_dialog("text", w, cx))),
                )
                .child(
                    button(
                        "diagram-object-text",
                        t!("editor.diagram_object_menu.edit_text").into(),
                        "type",
                    )
                    .disabled(locked || !object)
                    .on_click(cx.listener(|v, _, w, cx| v.diagram_edit_caption(w, cx))),
                )
                .child(
                    button(
                        "diagram-object-lock",
                        if locked {
                            t!("editor.diagram_object_menu.unlock_objects")
                        } else {
                            t!("editor.diagram_object_menu.lock_objects")
                        }
                        .into(),
                        if locked { "lock" } else { "unlock" },
                    )
                    .on_click(cx.listener(|v, _, _, cx| v.diagram_toggle_lock(cx))),
                )
                .child(
                    button(
                        "diagram-object-link",
                        t!("editor.diagram_object_menu.edit_link").into(),
                        "link",
                    )
                    .disabled(locked || !shape)
                    .on_click(cx.listener(|v, _, w, cx| {
                        v.diagram_annotation_dialog(
                            "drawio_link",
                            t!("editor.diagram_object_menu.link_title").into(),
                            w,
                            cx,
                        )
                    })),
                )
                .child(
                    button(
                        "diagram-object-duplicate",
                        t!("design.direct.duplicate").into(),
                        "copy",
                    )
                    .disabled(locked)
                    .on_click(cx.listener(|v, _, _, cx| v.duplicate_selected(cx))),
                )
                .child(
                    button(
                        "diagram-object-more",
                        t!("editor.diagram_object_menu.more_actions").into(),
                        "ellipsis",
                    )
                    .dropdown_menu(move |menu, w, cx| {
                        let Some(editor) = owner.upgrade() else {
                            return menu;
                        };
                        Self::diagram_object_menu(menu, &editor, w, cx)
                    }),
                )
                .into_any_element(),
        )
    }
}
