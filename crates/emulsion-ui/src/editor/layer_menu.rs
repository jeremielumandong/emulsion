//! Layer commands share the document selection and the same undoable actions as shortcuts.
use super::*;
use gpui_kit::component::button::{Button, ButtonVariants};
use gpui_kit::component::menu::DropdownMenu;
use gpui_kit::component::menu::{PopupMenu, PopupMenuItem};
use gpui_kit::component::{Sizable, WindowExt};

pub(super) fn item(
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
                    // A dialog opened from this item must retain a live editor
                    // focus handle, not the popup that disappears after click.
                    window.focus(&this.panel_focus, cx);
                    action(this, window, cx);
                    cx.defer_in(window, |this, window, cx| {
                        // Do not steal keyboard input from a modal just opened
                        // by the action. Closing it restores the stable panel.
                        if !window.has_active_dialog(cx) {
                            window.focus(&this.panel_focus, cx);
                        }
                    });
                })
                .ok();
        })
}

/// A layer colour label's name in the interface language.
pub(super) fn layer_color_label(color: emulsion_core::node::LayerColor) -> String {
    use emulsion_core::node::LayerColor;
    match color {
        LayerColor::None => t!("editor.layer_menu.color_none"),
        LayerColor::Red => t!("editor.layer_menu.color_red"),
        LayerColor::Orange => t!("editor.layer_menu.color_orange"),
        LayerColor::Yellow => t!("editor.layer_menu.color_yellow"),
        LayerColor::Green => t!("editor.layer_menu.color_green"),
        LayerColor::Blue => t!("editor.layer_menu.color_blue"),
        LayerColor::Violet => t!("editor.layer_menu.color_violet"),
        LayerColor::Gray => t!("editor.layer_menu.color_gray"),
    }
    .into_owned()
}

pub(super) fn mask_context_menu(
    menu: PopupMenu,
    editor: &Entity<EditorView>,
    id: NodeId,
    cx: &mut Context<PopupMenu>,
) -> PopupMenu {
    editor.update(cx, |e, cx| e.select_layer_mask(id, cx));
    let e = editor.read(cx);
    let enabled = e.layer_menu_ready() && e.editor.doc.locked_ancestor(id).is_none();
    let mask_enabled = e.editor.doc.node(id).is_some_and(|node| node.mask_enabled);
    menu.item(item(
        editor,
        t!("editor.layer_menu.delete_mask"),
        enabled,
        |e, _, cx| e.remove_mask(cx),
    ))
    .item(item(
        editor,
        t!("editor.layer_menu.invert_mask"),
        enabled,
        |e, _, cx| e.invert_mask(cx),
    ))
    .item(item(
        editor,
        if mask_enabled {
            t!("editor.layer_menu.disable_mask")
        } else {
            t!("editor.layer_menu.enable_mask")
        },
        enabled,
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
        editor,
        t!("editor.layer_menu.apply_mask"),
        e.can_apply_layer_mask(),
        |e, _, cx| e.apply_layer_mask(cx),
    ))
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
    let vector = node.is_some_and(|n| n.vector_mask.is_some());
    let vector_enabled = node
        .and_then(|n| n.vector_mask.as_ref())
        .is_some_and(|m| m.enabled);
    let vector_linked = node
        .and_then(|n| n.vector_mask.as_ref())
        .is_some_and(|m| m.linked);
    let vector_editable = editable && single && !e.editor.doc.layer_locks(id).position;
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
            t!("editor.layer_menu.blending_options"),
            single && ready,
            move |e, window, cx| e.open_blending_options(id, window, cx),
        ))
        .separator()
        .menu_with_disabled(
            if single {
                t!("editor.layer_menu.duplicate_layer")
            } else {
                t!("editor.layer_menu.duplicate_layers")
            },
            Box::new(crate::actions::DuplicateNode),
            !structural,
        )
        .menu_with_disabled(
            t!("editor.layer_menu.group_layers"),
            Box::new(crate::actions::GroupNodes),
            !structural || !same_parent,
        )
        .menu_with_disabled(
            t!("editor.layer_menu.ungroup_layers"),
            Box::new(crate::actions::Ungroup),
            !structural || !single || !group,
        )
        .menu_with_disabled(
            t!("editor.layer_menu.rename_layer"),
            Box::new(crate::actions::RenameLayer),
            !editable || !single,
        )
        .menu_with_disabled(
            if single {
                t!("editor.layer_menu.delete_layer")
            } else {
                t!("editor.layer_menu.delete_layers")
            },
            Box::new(crate::actions::DeleteNode),
            !structural,
        )
        .item(item(editor, "New Vector Layer", ready, |e, _, cx| {
            e.new_vector_layer(cx);
        }))
        .separator()
        .menu_with_disabled(
            if single {
                t!("editor.layer_menu.merge_down")
            } else {
                t!("editor.layer_menu.merge_layers")
            },
            Box::new(crate::actions::MergeLayers),
            !merge,
        )
        .menu_with_disabled(
            t!("editor.layer_menu.merge_visible"),
            Box::new(crate::actions::MergeVisible),
            !merge_visible,
        )
        .separator()
        .item(item(
            editor,
            t!("editor.layer_menu.select_pixels"),
            ready && coverage,
            |e, _, cx| {
                let Some(id) = e.selected else {
                    return;
                };
                let mask = match e.editor.doc.node_coverage(id) {
                    Ok(Some(mask)) => mask,
                    Ok(None) => return,
                    Err(error) => {
                        e.set_status(error.to_string(), true, cx);
                        return;
                    }
                };
                {
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
                t!("editor.layer_menu.release_clip")
            } else {
                t!("editor.layer_menu.create_clip")
            },
            editable && single && e.clipping_mask_commands(id).is_some(),
            move |e, _, cx| e.toggle_clipping_mask_for(id, cx),
        ));
    let target = editor.clone();
    let menu = menu.submenu(
        t!("editor.layer_menu.layer_mask"),
        window,
        cx,
        move |menu, _, _| {
            menu.item(item(
                &target,
                t!("editor.layer_menu.add_mask"),
                editable && single && !mask,
                |e, _, cx| e.add_mask(cx),
            ))
            .item(item(
                &target,
                if mask_enabled {
                    t!("editor.layer_menu.disable_mask")
                } else {
                    t!("editor.layer_menu.enable_mask")
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
                t!("editor.layer_menu.invert_mask"),
                editable && single && mask,
                |e, _, cx| e.invert_mask(cx),
            ))
            .item(item(
                &target,
                t!("editor.layer_menu.select_mask"),
                ready && single && mask,
                move |e, _, cx| e.component_mask_to_selection(id, MaskEditTarget::RasterMask, cx),
            ))
            .separator()
            .item(item(
                &target,
                t!("editor.layer_menu.delete_mask"),
                editable && single && mask,
                |e, _, cx| e.remove_mask(cx),
            ))
            .item(item(
                &target,
                t!("editor.layer_menu.apply_mask"),
                apply_mask,
                |e, _, cx| e.apply_layer_mask(cx),
            ))
            .item(item(
                &target,
                if mask_linked {
                    t!("editor.layer_menu.unlink_mask")
                } else {
                    t!("editor.layer_menu.link_mask")
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
        },
    );
    let target = editor.clone();
    let menu = menu.submenu("Vector Mask", window, cx, move |menu, _, _| {
        menu.item(item(
            &target,
            "Reveal All",
            vector_editable && !vector,
            |e, _, cx| e.add_vector_mask(false, cx),
        ))
        .item(item(
            &target,
            "Hide All",
            vector_editable && !vector,
            |e, _, cx| e.add_vector_mask(true, cx),
        ))
        .item(item(
            &target,
            "Draw / Append Vector Mask",
            vector_editable,
            |e, _, cx| e.draw_vector_mask(cx),
        ))
        .item(item(
            &target,
            "Close Path",
            vector_editable && vector,
            |e, _, cx| e.close_vector_mask_path(cx),
        ))
        .item(item(
            &target,
            if vector_enabled {
                "Disable Vector Mask"
            } else {
                "Enable Vector Mask"
            },
            editable && single && vector,
            |e, _, cx| e.toggle_vector_mask(cx),
        ))
        .item(item(
            &target,
            "Invert Vector Mask",
            editable && single && vector,
            |e, _, cx| e.invert_vector_mask(cx),
        ))
        .item(item(
            &target,
            if vector_linked {
                "Unlink Vector Mask"
            } else {
                "Link Vector Mask"
            },
            vector_editable && vector,
            |e, _, cx| e.toggle_vector_mask_link(cx),
        ))
        .item(item(
            &target,
            "Vector Mask to Selection",
            ready && single && vector,
            |e, _, cx| e.vector_mask_to_selection(cx),
        ))
        .item(item(
            &target,
            "Remove Vector Mask",
            vector_editable && vector,
            |e, _, cx| e.remove_vector_mask(cx),
        ))
        .item(item(
            &target,
            "Rasterize Vector Mask",
            vector_editable && vector && !mask,
            |e, _, cx| e.rasterize_vector_mask(cx),
        ))
    });
    let target = editor.clone();
    let menu = menu.submenu(
        t!("editor.layer_menu.layer_effects"),
        window,
        cx,
        move |menu, _, _| {
            menu.item(item(
                &target,
                t!("editor.layer_menu.blending_options"),
                single && ready,
                move |e, window, cx| e.open_blending_options(id, window, cx),
            ))
            .item(item(
                &target,
                t!("editor.layer_menu.clear_effects"),
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
                t!("editor.layer_menu.copy_style"),
                single && ready,
                |e, _, cx| e.copy_layer_style(cx),
            ))
            .item(item(
                &target,
                t!("editor.layer_menu.paste_style"),
                paste_style,
                |e, _, cx| e.paste_layer_style(cx),
            ))
        },
    );
    let target = editor.clone();
    let menu = menu.submenu(
        t!("editor.layer_menu.color"),
        window,
        cx,
        move |mut menu, _, _| {
            for label in emulsion_core::node::LayerColor::ALL {
                let entry = item(
                    &target,
                    layer_color_label(label),
                    editable,
                    move |e, _, cx| {
                        let commands = e
                            .selected_layer_ids()
                            .into_iter()
                            .map(|id| Command::SetColorLabel { id, color: label })
                            .collect();
                        e.execute_layer_commands("Layer color", commands, cx);
                    },
                )
                .checked(color == Some(label));
                menu = menu.item(entry);
            }
            menu
        },
    );
    menu.separator()
        .menu_with_disabled(
            t!("editor.layer_menu.link_layers"),
            Box::new(crate::actions::LinkLayers),
            !link,
        )
        .menu_with_disabled(
            t!("editor.layer_menu.unlink_layers"),
            Box::new(crate::actions::UnlinkLayers),
            !unlink,
        )
        .menu_with_disabled(
            t!("editor.layer_menu.flatten_image"),
            Box::new(crate::actions::FlattenImage),
            !flatten,
        )
}

impl EditorView {
    pub(super) fn layer_menu_button(&self, cx: &Context<Self>) -> AnyElement {
        let editor = cx.entity().downgrade();
        Button::new("layer-menu-button")
            .label(super::menu_bar::menu_name("layer"))
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
                    let ready = e.layer_menu_ready();
                    menu.menu(
                        t!("editor.layer_menu.new_layer"),
                        Box::new(crate::actions::NewLayer),
                    )
                    .item(item(
                        &editor,
                        "New Vector Layer",
                        ready,
                        |e, _, cx| {
                            e.new_vector_layer(cx);
                        },
                    ))
                }
            })
            .into_any_element()
    }
    pub(super) fn layer_menu_ready(&self) -> bool {
        !self.assistant.running
            && !self.frame_crop_active()
            && self.drag.is_none()
            && !self.editor.in_transaction()
            && self.warp.is_none()
    }

    /// In Photo, Ctrl+Alt+G releases the selected Photo layer and the
    /// consecutive clipped siblings above it that share the same base.
    /// Explicit clip links in Paint/Design retain their single-node semantics.
    pub(crate) fn toggle_clipping_mask(&mut self, cx: &mut Context<Self>) {
        if let Some(id) = self.selected {
            self.toggle_clipping_mask_for(id, cx);
        }
    }

    fn clipping_mask_commands(&self, id: NodeId) -> Option<Vec<Command>> {
        if !self.layer_menu_ready()
            || self.selected != Some(id)
            || (self.is_photo_workflow() && self.selected_layer_ids().len() != 1)
        {
            return None;
        }
        let node = self.editor.doc.node(id)?;
        let commands = if node.clip_to.is_some() {
            let ids = if self.is_photo_workflow() {
                photo_clip_release_ids(&self.editor.doc, id)
            } else {
                vec![id]
            };
            ids.into_iter()
                .map(|id| Command::SetClip { id, clip_to: None })
                .collect::<Vec<_>>()
        } else {
            vec![Command::SetClip {
                id,
                clip_to: Some(self.layer_below(id)?),
            }]
        };
        // Preflight the complete group before the UI enables release. A locked
        // upper member cannot leave a partially released chain.
        (!commands.is_empty()
            && commands.iter().all(|command| match command {
                Command::SetClip { id, .. } => self
                    .editor
                    .doc
                    .subtree(*id)
                    .into_iter()
                    .all(|id| self.editor.doc.locked_ancestor(id).is_none()),
                _ => false,
            }))
        .then_some(commands)
    }

    fn toggle_clipping_mask_for(&mut self, id: NodeId, cx: &mut Context<Self>) {
        let Some(commands) = self.clipping_mask_commands(id) else {
            return;
        };
        self.close_text_field(cx);
        self.execute_layer_commands("Clipping mask", commands, cx);
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
        source.global_light = doc.global_light;
        source.blend_space = doc.blend_space;
        if visible {
            source = doc.clone();
        } else {
            let members: Vec<_> = ids.iter().flat_map(|id| doc.subtree(*id)).collect();
            // A selected subset is an isolated source, not a document scope.
            source.psd_background = None;
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
        source.prune_psd_background();
        let tree = match source.try_composite_tree() {
            Ok(tree) => tree,
            Err(error) => {
                self.set_status(error.to_string(), true, cx);
                return;
            }
        };
        let raster = emulsion_raster::composite::flatten(&tree, 0);
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
            self.tools.mask_edit_target = crate::editor::MaskEditTarget::Content;
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
        let raster = match flatten_on_white(doc) {
            Ok(raster) => raster,
            Err(error) => {
                self.set_status(error.to_string(), true, cx);
                return;
            }
        };
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
            self.tools.mask_edit_target = crate::editor::MaskEditTarget::Content;
            cx.notify();
        }
    }
}

/// Preserve the new profile's scoped appearance before adding the final matte.
/// Inserting white below its Background would invalidate that explicit stop.
pub(super) fn flatten_on_white(doc: &Document) -> Result<Raster, emulsion_core::DocumentError> {
    let mut source = doc.clone();
    if doc.blend_space == emulsion_raster::blend::BlendSpace::PhotoshopSrgbV1 {
        let appearance = emulsion_raster::composite::flatten(&doc.try_composite_tree()?, 0);
        source = Document::new(doc.width, doc.height);
        source.blend_space = doc.blend_space;
        source.nodes = vec![Node::raster(
            1,
            "Composite",
            Arc::new(appearance),
            Placement::default(),
        )];
        source.psd_background = None;
        source.next_id = 2;
    }
    let mut background = Node::new(
        source.next_id,
        "Background",
        NodeKind::Fill { rgba: [255; 4] },
    );
    background.parent = None;
    source.nodes.insert(0, background);
    // Legacy rendering is unchanged; dormant role metadata is not applicable
    // to this temporary white-matted source document.
    source.psd_background = None;
    Ok(emulsion_raster::composite::flatten(
        &source.try_composite_tree()?,
        0,
    ))
}

/// Resolve both immediate-below chains and imported direct-to-base links.
/// Stop at sibling/group boundaries and at another clipping group; a document
/// may contain explicit nonconsecutive links, which this UI must not rewrite.
fn photo_clip_release_ids(doc: &Document, id: NodeId) -> Vec<NodeId> {
    let Some(selected) = doc.node(id).filter(|node| node.clip_to.is_some()) else {
        return Vec::new();
    };
    let mut bases = std::collections::HashMap::new();
    let mut selected_base = None;
    let mut released = Vec::new();
    // Document order is bottom-to-top. Resolve every sibling's base once,
    // rather than following each chain again (which is quadratic on long stacks).
    for node in doc
        .nodes
        .iter()
        .filter(|node| node.parent == selected.parent)
    {
        let base = match node.clip_to {
            Some(base) => bases.get(&base).copied(),
            None => Some(node.id),
        };
        if let Some(base) = base {
            bases.insert(node.id, base);
        }
        if node.id == id {
            selected_base = base;
            if selected_base.is_none() {
                return Vec::new();
            }
        }
        if let Some(root) = selected_base {
            if node.clip_to.is_none() || base != Some(root) {
                break;
            }
            released.push(node.id);
        }
    }
    released
}

#[cfg(test)]
#[path = "photo_clipping_release_tests.rs"]
mod photo_clipping_release_tests;
