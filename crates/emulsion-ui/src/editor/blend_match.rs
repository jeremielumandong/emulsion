//! Editable correction stacks and temporary checks for matching a subject to its scene.
use super::*;
use emulsion_raster::Mask;

impl EditorView {
    pub(crate) fn can_match_subject(&self) -> bool {
        self.layer_menu_ready()
            && self.selected.is_some_and(|id| {
                self.editor.doc.locked_ancestor(id).is_none()
                    && self
                        .editor
                        .doc
                        .node(id)
                        .is_some_and(|node| !matches!(node.kind, NodeKind::Adjust(_)))
            })
    }

    pub(crate) fn match_subject_stack(&mut self, cx: &mut Context<Self>) {
        if !self.can_match_subject() {
            return;
        }
        let subject = self.selected.unwrap();
        let slot = self.insertion_slot();
        let mut commands = Vec::new();
        for (offset, (key, name)) in [
            ("levels", "Match brightness"),
            ("hue_saturation", "Match saturation"),
            ("curves", "Match color"),
        ]
        .into_iter()
        .enumerate()
        {
            let adjustment = Adjustment::catalogue()
                .into_iter()
                .find(|a| a.key() == key)
                .unwrap();
            let mut node = Node::adjust(0, adjustment);
            node.name = name.into();
            node.mask = Some(Arc::new(Mask::empty(
                self.editor.doc.width,
                self.editor.doc.height,
                255,
            )));
            commands.push(Command::AddNode {
                node: Box::new(node),
                slot: Slot {
                    parent: slot.parent,
                    index: slot.index + offset,
                },
            });
            commands.push(Command::SetClip {
                id: self.editor.doc.next_id + offset as u64,
                clip_to: Some(subject),
            });
        }
        if let Some(ids) = self.execute_layer_commands("Set up subject matching", commands, cx) {
            if let Some(id) = ids.first().copied() {
                self.set_layer_selection(vec![id], Some(id));
            }
            self.select_sidebar(SidebarTab::Properties, cx);
            self.set_status("Adjust brightness, saturation, then color. All three layers are clipped to the subject; their masks control where each correction applies.", false, cx);
        }
    }

    pub(crate) fn add_blending_check(&mut self, kind: &str, cx: &mut Context<Self>) {
        if !self.layer_menu_ready() {
            return;
        }
        let mut node = match kind {
            "brightness" => {
                let adjustment = Adjustment::catalogue()
                    .into_iter()
                    .find(|a| a.key() == "black_and_white")
                    .unwrap();
                Node::adjust(0, adjustment)
            }
            "saturation" => Node::adjust(0, Adjustment::selective_color_saturation_check()),
            "color" => {
                let mut node = Node::new(
                    0,
                    "Check color",
                    NodeKind::Fill {
                        rgba: [128, 128, 128, 255],
                    },
                );
                node.blend = BlendMode::Luminosity;
                node
            }
            _ => return,
        };
        node.name = format!("Check {kind} (hide when finished)");
        // A diagnostic must cover the whole composite even if a subject selection is active.
        if let Some(id) = self.execute(
            Command::AddNode {
                node: Box::new(node),
                slot: Slot::TOP,
            },
            cx,
        ) {
            self.set_layer_selection(vec![id], Some(id));
            self.select_sidebar(SidebarTab::Properties, cx);
            self.set_status("Temporary check layer added above the whole image. Hide other check layers while using this one, and hide it when finished.", false, cx);
        }
    }
}
