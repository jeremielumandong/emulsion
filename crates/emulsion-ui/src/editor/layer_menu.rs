//! Layer commands share the document selection and the same undoable actions as shortcuts.
use super::*;
use gpui_kit::component::Sizable;
use gpui_kit::component::button::{Button, ButtonVariants};
use gpui_kit::component::menu::DropdownMenu;
use gpui_kit::component::menu::{PopupMenu, PopupMenuItem};

fn item(
    editor: &Entity<EditorView>,
    label: impl Into<SharedString>,
    enabled: bool,
    action: impl Fn(&mut EditorView, &mut Window, &mut Context<EditorView>) + 'static,
) -> PopupMenuItem {
    let editor = editor.downgrade();
    PopupMenuItem::new(label)
        .disabled(!enabled)
        .on_click(move |_, window, cx| {
            editor
                .update(cx, |this, cx| {
                    if !this.layer_menu_ready() {
                        return;
                    }
                    action(this, window, cx);
                    cx.defer_in(window, |this, window, cx| {
                        window.focus(&this.panel_focus, cx)
                    });
                })
                .ok();
        })
}

pub(super) fn layer_context_menu(
    menu: PopupMenu,
    editor: &Entity<EditorView>,
    id: NodeId,
    focus: FocusHandle,
    window: &mut Window,
    cx: &mut Context<PopupMenu>,
) -> PopupMenu {
    editor.update(cx, |e, cx| e.select_layer_context(id, cx));
    let menu = editor.update(cx, |e, cx| e.clipboard_menu(menu, focus.clone(), cx));
    let menu = clipboard::transform_menu(menu, editor, focus.clone(), window, cx);
    let e = editor.read(cx);
    let ready = e.layer_menu_ready();
    let ids = e.selected_layer_ids();
    let single = ids.len() == 1;
    let editable = ready
        && !ids.is_empty()
        && ids
            .iter()
            .all(|id| e.editor.doc.locked_ancestor(*id).is_none());
    let structural = editable
        && ids
            .iter()
            .flat_map(|id| e.editor.doc.subtree(*id))
            .all(|id| e.editor.doc.locked_ancestor(id).is_none());
    let node = e.editor.doc.node(id);
    let group = node.is_some_and(Node::is_group);
    let mask = node.is_some_and(|n| n.mask.is_some());
    let mask_enabled = node.is_some_and(|n| n.mask_enabled);
    let clipped = node.is_some_and(|n| n.clip_to.is_some());
    let has_styles = node.is_some_and(|n| !n.styles.is_empty());
    let below = e.layer_below(id);
    let coverage = single
        && node.is_some_and(|n| {
            !matches!(n.kind, NodeKind::Adjust(_) | NodeKind::Group { .. }) || n.mask.is_some()
        });
    let merge = e.merge_layer_ids().is_some();
    let merge_visible = e.merge_visible_ids().is_some();
    let flatten = e.can_flatten_image();
    let link = e.can_link_layers(true);
    let unlink = e.can_link_layers(false);
    let mask_linked = node.is_some_and(|node| node.mask_linked);
    let paste_style = editable && e.can_paste_layer_style(cx);
    let apply_mask = ready && e.can_apply_layer_mask();
    let roots = e.selected_layer_roots();
    let parent = roots
        .first()
        .and_then(|id| e.editor.doc.node(*id))
        .map(|n| n.parent);
    let same_parent = roots
        .iter()
        .filter_map(|id| e.editor.doc.node(*id))
        .all(|n| Some(n.parent) == parent);
    let color = node.map(|n| n.color_label);
    let menu = menu
        .separator()
        .item(item(
            editor,
            "Blending Options…",
            single && ready,
            move |e, _, cx| e.open_blending_options(id, cx),
        ))
        .separator()
        .menu_with_disabled(
            if single {
                "Duplicate Layer"
            } else {
                "Duplicate Layers"
            },
            Box::new(crate::actions::DuplicateNode),
            !structural,
        )
        .menu_with_disabled(
            "Group Layers",
            Box::new(crate::actions::GroupNodes),
            !structural || !same_parent,
        )
        .menu_with_disabled(
            "Ungroup Layers",
            Box::new(crate::actions::Ungroup),
            !structural || !single || !group,
        )
        .menu_with_disabled(
            "Rename Layer…",
            Box::new(crate::actions::RenameLayer),
            !editable || !single,
        )
        .menu_with_disabled(
            if single {
                "Delete Layer"
            } else {
                "Delete Layers"
            },
            Box::new(crate::actions::DeleteNode),
            !structural,
        )
        .separator()
        .menu_with_disabled(
            if single { "Merge Down" } else { "Merge Layers" },
            Box::new(crate::actions::MergeLayers),
            !merge,
        )
        .menu_with_disabled(
            "Merge Visible",
            Box::new(crate::actions::MergeVisible),
            !merge_visible,
        )
        .separator()
        .item(item(
            editor,
            "Select Pixels",
            ready && coverage,
            |e, _, cx| {
                if let Some(mask) = e.selected.and_then(|id| e.editor.doc.node_coverage(id)) {
                    e.execute(
                        Command::SetSelection {
                            selection: Some(Arc::new(mask)),
                        },
                        cx,
                    );
                }
            },
        ))
        .item(item(
            editor,
            if clipped {
                "Release Clipping Mask"
            } else {
                "Create Clipping Mask"
            },
            editable && single && (clipped || below.is_some()),
            move |e, _, cx| {
                e.execute(
                    Command::SetClip {
                        id,
                        clip_to: if clipped { None } else { below },
                    },
                    cx,
                );
            },
        ));
    let target = editor.clone();
    let menu = menu.submenu("Layer Mask", window, cx, move |menu, _, _| {
        menu.item(item(
            &target,
            "Add Layer Mask",
            editable && single && !mask,
            |e, _, cx| e.add_mask(cx),
        ))
        .item(item(
            &target,
            if mask_enabled {
                "Disable Layer Mask"
            } else {
                "Enable Layer Mask"
            },
            editable && single && mask,
            move |e, _, cx| {
                e.execute(
                    Command::SetMaskEnabled {
                        id,
                        enabled: !mask_enabled,
                    },
                    cx,
                );
            },
        ))
        .item(item(
            &target,
            "Invert Layer Mask",
            editable && single && mask,
            |e, _, cx| e.invert_mask(cx),
        ))
        .item(item(
            &target,
            "Select Layer Mask",
            ready && single && mask,
            |e, _, cx| e.mask_to_selection(cx),
        ))
        .separator()
        .item(item(
            &target,
            "Delete Layer Mask",
            editable && single && mask,
            |e, _, cx| e.remove_mask(cx),
        ))
        .item(item(&target, "Apply Layer Mask", apply_mask, |e, _, cx| {
            e.apply_layer_mask(cx)
        }))
        .item(item(
            &target,
            if mask_linked {
                "Unlink Layer Mask"
            } else {
                "Link Layer Mask"
            },
            editable && single && mask,
            move |e, _, cx| {
                e.execute(
                    Command::SetMaskLinked {
                        id,
                        linked: !mask_linked,
                    },
                    cx,
                );
            },
        ))
    });
    let target = editor.clone();
    let menu = menu.submenu("Layer Effects", window, cx, move |menu, _, _| {
        menu.item(item(
            &target,
            "Blending Options…",
            single && ready,
            move |e, _, cx| e.open_blending_options(id, cx),
        ))
        .item(item(
            &target,
            "Clear Layer Effects",
            editable && single && has_styles,
            move |e, _, cx| {
                e.execute(
                    Command::SetStyles {
                        id,
                        styles: Vec::new(),
                    },
                    cx,
                );
            },
        ))
        .item(item(
            &target,
            "Copy Layer Style",
            single && ready,
            |e, _, cx| e.copy_layer_style(cx),
        ))
        .item(item(
            &target,
            "Paste Layer Style",
            paste_style,
            |e, _, cx| e.paste_layer_style(cx),
        ))
    });
    let target = editor.clone();
    let menu = menu.submenu("Color", window, cx, move |mut menu, _, _| {
        for label in emulsion_core::node::LayerColor::ALL {
            let entry = item(&target, label.label(), editable, move |e, _, cx| {
                let commands = e
                    .selected_layer_ids()
                    .into_iter()
                    .map(|id| Command::SetColorLabel { id, color: label })
                    .collect();
                e.execute_layer_commands("Layer color", commands, cx);
            })
            .checked(color == Some(label));
            menu = menu.item(entry);
        }
        menu
    });
    menu.separator()
        .menu_with_disabled("Link Layers", Box::new(crate::actions::LinkLayers), !link)
        .menu_with_disabled(
            "Unlink Layers",
            Box::new(crate::actions::UnlinkLayers),
            !unlink,
        )
        .menu_with_disabled(
            "Flatten Image",
            Box::new(crate::actions::FlattenImage),
            !flatten,
        )
}

impl EditorView {
    pub(super) fn layer_menu_button(&self, cx: &Context<Self>) -> AnyElement {
        let editor = cx.entity().downgrade();
        Button::new("layer-menu-button")
            .label("Layer")
            .small()
            .ghost()
            .dropdown_menu(move |menu, window, cx| {
                let Some(editor) = editor.upgrade() else {
                    return menu;
                };
                let e = editor.read(cx);
                let focus = e.panel_focus.clone();
                if let Some(id) = e.selected {
                    layer_context_menu(menu, &editor, id, focus, window, cx)
                } else {
                    menu.menu("New Layer", Box::new(crate::actions::NewLayer))
                }
            })
            .into_any_element()
    }
    fn layer_menu_ready(&self) -> bool {
        !self.assistant.running
            && self.drag.is_none()
            && !self.editor.in_transaction()
            && self.warp.is_none()
    }

    fn layer_below(&self, id: NodeId) -> Option<NodeId> {
        let node = self.editor.doc.node(id)?;
        let siblings = self.editor.doc.children(node.parent);
        let index = siblings.iter().position(|other| *other == id)?;
        index.checked_sub(1).map(|i| siblings[i])
    }

    /// Merge complete sibling subtrees. Clipping relationships crossing the
    /// merge boundary cannot be represented by the resulting pixel layer.
    fn merge_layer_ids(&self) -> Option<Vec<NodeId>> {
        if !self.layer_menu_ready() {
            return None;
        }
        let mut ids = self.selected_layer_roots();
        if ids.len() == 1 {
            ids.insert(0, self.layer_below(ids[0])?);
        }
        if ids.len() < 2 {
            return None;
        }
        let first = self.editor.doc.node(ids[0])?;
        if ids.iter().any(|id| {
            self.editor
                .doc
                .node(*id)
                .is_none_or(|node| node.parent != first.parent)
        }) {
            return None;
        }
        let members: Vec<_> = ids
            .iter()
            .flat_map(|id| self.editor.doc.subtree(*id))
            .collect();
        for id in &members {
            let n = self.editor.doc.node(*id)?;
            if self.editor.doc.locked_ancestor(*id).is_some()
                || self.editor.doc.layer_locks(*id) != Default::default()
                || n.clip_to.is_some_and(|base| !members.contains(&base))
            {
                return None;
            }
        }
        if self
            .editor
            .doc
            .nodes
            .iter()
            .any(|n| !members.contains(&n.id) && n.clip_to.is_some_and(|id| members.contains(&id)))
        {
            return None;
        }
        Some(ids)
    }

    fn merge_visible_ids(&self) -> Option<Vec<NodeId>> {
        if !self.layer_menu_ready() {
            return None;
        }
        let ids: Vec<_> = self
            .editor
            .doc
            .nodes
            .iter()
            .filter(|n| n.parent.is_none() && n.visible)
            .map(|n| n.id)
            .collect();
        if ids.is_empty() || (ids.len() == 1 && !self.editor.doc.node(ids[0])?.is_group()) {
            return None;
        }
        if ids
            .iter()
            .flat_map(|id| self.editor.doc.subtree(*id))
            .any(|id| {
                self.editor.doc.locked_ancestor(id).is_some()
                    || self.editor.doc.layer_locks(id) != Default::default()
            })
        {
            return None;
        }
        if self.editor.doc.nodes.iter().any(|n| {
            n.parent.is_none() && !n.visible && n.clip_to.is_some_and(|id| ids.contains(&id))
        }) {
            return None;
        }
        Some(ids)
    }

    pub(crate) fn rename_layer(&mut self, window: &mut Window, cx: &mut Context<Self>) {
        if self.layer_menu_ready()
            && self.selected_layer_ids().len() == 1
            && let Some(id) = self.selected
            && self.editor.doc.locked_ancestor(id).is_none()
        {
            self.start_rename(id, window, cx);
        }
    }

    pub(crate) fn merge_layers(&mut self, visible: bool, cx: &mut Context<Self>) {
        let ids = if visible {
            self.merge_visible_ids()
        } else {
            self.merge_layer_ids()
        };
        let Some(ids) = ids else {
            return;
        };
        let doc = &self.editor.doc;
        let parent = doc.node(ids[0]).unwrap().parent;
        // Keep the merged result at the top selected layer's stack position.
        let siblings = doc.children(parent);
        let top = siblings
            .iter()
            .position(|id| Some(id) == ids.last())
            .unwrap();
        let index = siblings[..top]
            .iter()
            .filter(|id| !ids.contains(id))
            .count();
        let name = if visible {
            "Merged visible".into()
        } else {
            doc.node(*ids.last().unwrap()).unwrap().name.clone()
        };
        let mut source = Document::new(doc.width, doc.height);
        source.blend_space = doc.blend_space;
        if visible {
            source = doc.clone();
        } else {
            let members: Vec<_> = ids.iter().flat_map(|id| doc.subtree(*id)).collect();
            source.nodes = doc
                .nodes
                .iter()
                .filter(|node| members.contains(&node.id))
                .cloned()
                .map(|mut n| {
                    if ids.contains(&n.id) {
                        n.parent = None;
                    }
                    n
                })
                .collect();
        }
        let raster = emulsion_raster::composite::flatten(&source.composite_tree(), 0);
        let mut commands: Vec<_> = ids
            .iter()
            .map(|id| Command::RemoveNode { id: *id })
            .collect();
        let mut merged = Node::raster(0, name, Arc::new(raster), Placement::default());
        // Keep an existing link only when every merged root belongs to it.
        let common_link = doc.node(ids[0]).and_then(|node| node.link_group);
        let merged_members: Vec<_> = ids.iter().flat_map(|id| doc.subtree(*id)).collect();
        let has_external_partner = doc
            .nodes
            .iter()
            .any(|node| !merged_members.contains(&node.id) && node.link_group == common_link);
        if common_link.is_some()
            && has_external_partner
            && ids.iter().all(|id| {
                doc.node(*id)
                    .is_some_and(|node| node.link_group == common_link)
            })
        {
            merged.link_group = common_link;
        }
        commands.push(Command::AddNode {
            node: Box::new(merged),
            slot: Slot { parent, index },
        });
        if let Some(created) = self.execute_layer_commands(
            if visible {
                "Merge visible"
            } else {
                "Merge layers"
            },
            commands,
            cx,
        ) && let Some(id) = created.last().copied()
        {
            self.set_layer_selection(vec![id], Some(id));
            self.tools.mask_edit = false;
            cx.notify();
        }
    }

    pub(crate) fn can_flatten_image(&self) -> bool {
        self.layer_menu_ready()
            && !self.editor.doc.nodes.is_empty()
            && self.editor.doc.nodes.iter().all(|node| {
                self.editor.doc.locked_ancestor(node.id).is_none()
                    && self.editor.doc.layer_locks(node.id) == Default::default()
            })
    }

    /// Flatten is deliberately distinct from Merge Visible: hidden layers are
    /// discarded and transparency is composited on an opaque white background.
    pub(crate) fn flatten_image(&mut self, cx: &mut Context<Self>) {
        if !self.can_flatten_image() {
            return;
        }
        let doc = &self.editor.doc;
        let mut source = doc.clone();
        let mut background = Node::new(
            source.next_id,
            "Background",
            NodeKind::Fill { rgba: [255; 4] },
        );
        background.parent = None;
        source.nodes.insert(0, background);
        let raster = emulsion_raster::composite::flatten(&source.composite_tree(), 0);
        let mut commands: Vec<_> = doc
            .nodes
            .iter()
            .filter(|node| node.parent.is_none())
            .map(|node| Command::RemoveNode { id: node.id })
            .collect();
        commands.push(Command::AddNode {
            node: Box::new(Node::raster(
                0,
                "Background",
                Arc::new(raster),
                Placement::default(),
            )),
            slot: Slot::TOP,
        });
        if let Some(created) = self.execute_layer_commands("Flatten image", commands, cx)
            && let Some(id) = created.last().copied()
        {
            self.set_layer_selection(vec![id], Some(id));
            self.tools.mask_edit = false;
            cx.notify();
        }
    }
}
