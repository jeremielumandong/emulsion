//! Link membership survives selection changes, saves, and undo.
use super::*;

impl EditorView {
    pub(crate) fn layer_is_linked(&self, id: NodeId) -> bool {
        let Some(group) = self.editor.doc.node(id).and_then(|node| node.link_group) else {
            return false;
        };
        self.editor
            .doc
            .nodes
            .iter()
            .any(|node| node.id != id && node.link_group == Some(group))
    }

    pub(crate) fn can_link_layers(&self, linked: bool) -> bool {
        if self.assistant.running
            || self.drag.is_some()
            || self.editor.in_transaction()
            || self.warp.is_some()
        {
            return false;
        }
        let mut trial = self.editor.doc.clone();
        Command::SetLayerLinks {
            ids: self.selected_layer_roots(),
            linked,
        }
        .apply(&mut trial)
        .is_ok()
            && trial != self.editor.doc
    }

    pub(crate) fn set_layer_links(&mut self, linked: bool, cx: &mut Context<Self>) {
        if !self.can_link_layers(linked) {
            return;
        }
        self.execute(
            Command::SetLayerLinks {
                ids: self.selected_layer_roots(),
                linked,
            },
            cx,
        );
    }

    pub(crate) fn movement_layer_roots(&self) -> Vec<NodeId> {
        emulsion_core::layer_links::movement_roots(&self.editor.doc, &self.selected_layer_roots())
            .unwrap_or_default()
    }
}
