//! Layer selection is per document; `selected` remains the active inspector target.
use super::*;

#[derive(Default)]
pub(crate) struct LayerSelection {
    active: Option<NodeId>,
    anchor: Option<NodeId>,
    ids: Vec<NodeId>,
    pub(super) move_ids: Vec<NodeId>,
}

impl EditorView {
    pub(crate) fn selected_layer_ids(&self) -> Vec<NodeId> {
        let Some(active) = self.selected else {
            return Vec::new();
        };
        self.editor
            .doc
            .nodes
            .iter()
            .filter(|node| {
                node.id == active
                    || (self.layer_selection.active == Some(active)
                        && self.layer_selection.ids.contains(&node.id))
            })
            .map(|node| node.id)
            .collect()
    }

    pub(crate) fn layer_is_selected(&self, id: NodeId) -> bool {
        self.selected == Some(id)
            || (self.selected.is_some()
                && self.layer_selection.active == self.selected
                && self.layer_selection.ids.contains(&id))
    }

    pub(crate) fn set_layer_selection(&mut self, ids: Vec<NodeId>, active: Option<NodeId>) {
        // The modal target is frozen. Start/commit/cancel resolve ownership before
        // changing selection, while unrelated callers cannot retarget its handles.
        if self.photo_transform_active() {
            return;
        }
        self.transform_control_mode = TransformControlMode::Resize;
        self.finish_mask_properties();
        // Even a same-node thumbnail change must retire asynchronous filters.
        // Their conservative whole-node snapshot must not survive retargeting.
        self.smart.cancel_pending();
        // Selection changed by something other than a click on a layer row --
        // opening a document, a tool, the keyboard -- so the boundary goes.
        // `select_layer_row` re-arms it after calling through here.
        self.layer_outline_shown = false;
        self.mask_view.target = None;
        self.tools.mask_edit_target = MaskEditTarget::Content;
        self.pen_cancel();
        self.commit_shape_color_edit();
        self.type_tool.selection = None;
        self.selected = active;
        self.layer_selection = LayerSelection {
            active,
            anchor: active,
            ids,
            move_ids: Vec::new(),
        };
    }

    pub(crate) fn select_layer_row(
        &mut self,
        id: NodeId,
        toggle: bool,
        range: bool,
        cx: &mut Context<Self>,
    ) {
        if !self.photo_transform_ready(cx) {
            return;
        }
        if self.editor.doc.node(id).is_none() {
            return;
        }
        self.finish_tool_interaction(cx);
        self.cancel_move(cx);
        self.close_text_field(cx);
        let mut ids = self.selected_layer_ids();
        let anchor = if self.layer_selection.active == self.selected {
            self.layer_selection.anchor.or(self.selected)
        } else {
            self.selected
        };
        if range {
            let rows = self.filtered_layer_rows();
            if let Some(a) = anchor.and_then(|a| rows.iter().position(|r| r.id == a)) {
                if let Some(b) = rows.iter().position(|r| r.id == id) {
                    if !toggle {
                        ids.clear();
                    }
                    for row in &rows[a.min(b)..=a.max(b)] {
                        if !ids.contains(&row.id) {
                            ids.push(row.id);
                        }
                    }
                }
            } else {
                ids = vec![id];
            }
        } else if toggle {
            if ids.contains(&id) {
                ids.retain(|other| *other != id);
            } else {
                ids.push(id);
            }
        } else {
            ids = vec![id];
        }
        let active = if ids.contains(&id) {
            Some(id)
        } else {
            ids.last().copied()
        };
        self.set_layer_selection(ids, active);
        // An explicit click on a layer row: this is the one case that shows
        // the layer's dashed boundary.
        self.layer_outline_shown = true;
        if range {
            self.layer_selection.anchor = anchor.or(Some(id));
        }
        if !self.sidebar_layout.flyout_open
            && !matches!(
                self.sidebar_tab,
                SidebarTab::Develop
                    | SidebarTab::Reference
                    | SidebarTab::History
                    | SidebarTab::BrushSettings
                    | SidebarTab::BrushPresets
                    | SidebarTab::BlendingOptions
            )
        {
            self.select_sidebar(SidebarTab::Properties, cx);
        }
        cx.notify();
    }

    pub(crate) fn select_layer_context(&mut self, id: NodeId, cx: &mut Context<Self>) {
        self.cancel_move(cx);
        self.close_text_field(cx);
        if self.layer_is_selected(id) {
            let ids = self.selected_layer_ids();
            let anchor = self.layer_selection.anchor;
            self.set_layer_selection(ids, Some(id));
            self.layer_selection.anchor = anchor;
            cx.notify();
        } else {
            self.select_layer_row(id, false, false, cx);
        }
    }

    /// Photoshop's Alt+] / Alt+[ (and with Shift, add to the selection):
    /// the next layer up or down the Layers panel.
    pub(crate) fn select_adjacent_layer(&mut self, up: bool, extend: bool, cx: &mut Context<Self>) {
        let rows = self.filtered_layer_rows();
        let target = match self
            .selected
            .and_then(|id| rows.iter().position(|row| row.id == id))
        {
            // Rows run top to bottom.
            Some(i) if up => i.checked_sub(1),
            Some(i) => Some(i + 1).filter(|next| *next < rows.len()),
            None => (!rows.is_empty()).then_some(0),
        };
        if let Some(row) = target.and_then(|i| rows.get(i)) {
            let id = row.id;
            if extend && self.layer_is_selected(id) {
                return;
            }
            self.layer_panel.reveal = Some(id);
            self.select_layer_row(id, extend, false, cx);
        }
    }

    /// Photoshop's Alt+. / Alt+, : the top or bottom layer.
    pub(crate) fn select_edge_layer(&mut self, top: bool, cx: &mut Context<Self>) {
        let rows = self.filtered_layer_rows();
        let row = if top { rows.first() } else { rows.last() };
        if let Some(id) = row.map(|row| row.id) {
            self.layer_panel.reveal = Some(id);
            self.select_layer_row(id, false, false, cx);
        }
    }

    /// Photoshop's Ctrl+Alt+A: every layer in the panel.
    pub(crate) fn select_all_layers(&mut self, cx: &mut Context<Self>) {
        if !self.photo_transform_ready(cx) {
            return;
        }
        let ids: Vec<_> = self
            .filtered_layer_rows()
            .into_iter()
            .map(|row| row.id)
            .collect();
        let Some(first) = ids.first().copied() else {
            return;
        };
        let active = self.selected.filter(|id| ids.contains(id)).or(Some(first));
        self.cancel_move(cx);
        self.close_text_field(cx);
        self.set_layer_selection(ids, active);
        cx.notify();
    }

    /// Photoshop's Ctrl+Shift+] / Ctrl+Shift+[: move the selection to the
    /// top or bottom of its siblings, as one undo step.
    pub(crate) fn shift_selected_to_end(&mut self, up: bool, cx: &mut Context<Self>) {
        let selected = self.selected_layer_roots();
        let protect_background =
            self.is_design() || self.editor.doc.design.page_background.is_some();
        let mut trial = self.editor.doc.clone();
        let mut commands = Vec::new();
        loop {
            let mut moved = false;
            let mut ids = selected.clone();
            if up {
                ids.reverse();
            }
            for id in ids {
                if protect_background
                    && emulsion_core::design_background::is_background_node(&trial, id)
                {
                    continue;
                }
                let Some(node) = trial.node(id) else { continue };
                let parent = node.parent;
                let siblings = trial.children(parent);
                let Some(index) = siblings.iter().position(|other| *other == id) else {
                    continue;
                };
                let target = if up { index + 1 } else { index.wrapping_sub(1) };
                let floor = if protect_background && parent.is_none() {
                    emulsion_core::design_background::foreground_start(&trial)
                } else {
                    0
                };
                if target < floor
                    || target >= siblings.len()
                    || selected.contains(&siblings[target])
                {
                    continue;
                }
                let command = Command::MoveNode {
                    id,
                    slot: Slot {
                        parent,
                        index: target,
                    },
                };
                if let Err(error) = command.clone().apply(&mut trial) {
                    self.set_status(error.to_string(), true, cx);
                    return;
                }
                // Normalization may pin a protected object back into place.
                // Only a real order change makes progress toward the end.
                if trial.children(parent) != siblings {
                    commands.push(command);
                    moved = true;
                }
            }
            if !moved {
                break;
            }
        }
        if commands.is_empty() {
            return;
        }
        self.execute_layer_commands("Reorder layers", commands, cx);
    }

    /// Selected parents already include their descendants for structural edits.
    pub(crate) fn selected_layer_roots(&self) -> Vec<NodeId> {
        emulsion_core::layer_links::selected_roots(&self.editor.doc, &self.selected_layer_ids())
            .unwrap_or_default()
    }

    /// Validate the whole operation before changing anything, including inside
    /// a slider transaction. A locked member must never cause a partial edit.
    pub(crate) fn execute_layer_commands(
        &mut self,
        name: &str,
        commands: Vec<Command>,
        cx: &mut Context<Self>,
    ) -> Option<Vec<NodeId>> {
        if !self.photo_transform_ready(cx) {
            return None;
        }
        match self.editor.execute_commands(name, &commands) {
            Ok(created) => {
                self.after_change(cx);
                Some(created.into_iter().flatten().collect())
            }
            Err(error) => {
                self.set_status(error.to_string(), true, cx);
                None
            }
        }
    }
}
