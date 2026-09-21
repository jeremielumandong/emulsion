//! Layer commands share the document selection and the same undoable actions as shortcuts.
use super::*;
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
    });
    let target = editor.clone();
    menu.submenu("Color", window, cx, move |mut menu, _, _| {
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
    })
}

impl EditorView {
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

    /// Partial merges must be independent of the remaining backdrop. More
    /// complex blend stacks can be baked faithfully using Merge Visible.
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
        let siblings = self.editor.doc.children(first.parent);
        let start = siblings.iter().position(|id| *id == ids[0])?;
        if siblings.get(start..start + ids.len()) != Some(ids.as_slice()) {
            return None;
        }
        for id in &ids {
            let n = self.editor.doc.node(*id)?;
            if self.editor.doc.locked_ancestor(*id).is_some()
                || self.editor.doc.layer_locks(*id) != Default::default()
                || !n.visible
                || matches!(n.kind, NodeKind::Group { .. } | NodeKind::Adjust(_))
                || n.blend != BlendMode::Normal
                || n.blending != Default::default()
                || n.clip_to.is_some()
                || !n.styles.is_empty()
            {
                return None;
            }
        }
        if self
            .editor
            .doc
            .nodes
            .iter()
            .any(|n| !ids.contains(&n.id) && n.clip_to.is_some_and(|id| ids.contains(&id)))
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
        let index = doc
            .children(parent)
            .iter()
            .position(|id| *id == ids[0])
            .unwrap();
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
            source.nodes = ids
                .iter()
                .filter_map(|id| doc.node(*id).cloned())
                .map(|mut n| {
                    n.parent = None;
                    n
                })
                .collect();
        }
        let raster = emulsion_raster::composite::flatten(&source.composite_tree(), 0);
        let mut commands: Vec<_> = ids
            .iter()
            .map(|id| Command::RemoveNode { id: *id })
            .collect();
        commands.push(Command::AddNode {
            node: Box::new(Node::raster(
                0,
                name,
                Arc::new(raster),
                Placement::default(),
            )),
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
}
