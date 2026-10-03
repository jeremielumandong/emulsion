//! Commands computed for several storyboard panels at once (AI over the
//! selected panels), committed together as one project Undo step.
use super::{PageId, ProjectEditor};
use crate::Command;
use std::collections::BTreeMap;

impl ProjectEditor {
    /// Apply each panel's commands to a copy of its drawing and commit all
    /// of them as one Undo step labelled `label`. Nothing changes when any
    /// panel is locked, missing or refuses a command. Returns the panels
    /// that changed.
    pub fn edit_panels(
        &mut self,
        edits: BTreeMap<PageId, Vec<Command>>,
        label: &str,
    ) -> Result<Vec<PageId>, String> {
        let board = self.board()?.clone();
        let mut documents = BTreeMap::new();
        for (id, commands) in edits {
            let name = || {
                self.layout
                    .iter()
                    .find(|m| m.id == id)
                    .map_or_else(|| format!("Panel {id}"), |m| m.name.clone())
            };
            let editor = self.page(id).ok_or("Panel does not exist.")?;
            if board.is_locked(id) {
                return Err(format!("{} is locked.", name()));
            }
            let mut doc = editor.doc.clone();
            for command in commands {
                command
                    .apply(&mut doc)
                    .map_err(|e| format!("{}: {e}", name()))?;
            }
            documents.insert(id, doc);
        }
        let changed: Vec<_> = documents
            .iter()
            .filter(|(id, doc)| self.pages[id].doc != **doc)
            .map(|(id, _)| *id)
            .collect();
        self.commit_documents(documents, label)?;
        Ok(changed)
    }
}

#[cfg(test)]
mod tests {
    use crate::command::Slot;
    use crate::project::{ProjectEditor, ProjectKind};
    use crate::storyboard::Panel;
    use crate::{Command, Document, Node, NodeKind};
    use std::collections::BTreeMap;

    fn board(panels: usize) -> ProjectEditor {
        let mut p =
            ProjectEditor::new_project(ProjectKind::Storyboard, Document::new(16, 9)).unwrap();
        let blank = p.storyboard().unwrap().blank_panel().unwrap();
        let items = (2..=panels)
            .map(|n| (format!("Panel {n}"), Panel::new(0, 24)))
            .collect();
        p.insert_panels(Some(1), &blank, items, None).unwrap();
        p
    }

    fn add(name: &str) -> Vec<Command> {
        vec![Command::AddNode {
            node: Box::new(Node::new(0, name, NodeKind::Fill { rgba: [255; 4] })),
            slot: Slot::TOP,
        }]
    }

    fn names(p: &ProjectEditor, id: u64) -> Vec<String> {
        p.page(id)
            .unwrap()
            .doc
            .nodes
            .iter()
            .map(|n| n.name.clone())
            .collect()
    }

    #[test]
    fn several_panels_change_and_undo_as_one_step() {
        let mut p = board(3);
        let ids: Vec<_> = p.page_list().iter().map(|m| m.id).collect();
        let before: Vec<_> = ids.iter().map(|id| names(&p, *id)).collect();
        let edits = BTreeMap::from([(ids[0], add("AI one")), (ids[2], add("AI three"))]);
        let changed = p.edit_panels(edits, "AI on panels").unwrap();
        assert_eq!(changed, vec![ids[0], ids[2]]);
        assert!(names(&p, ids[0]).contains(&"AI one".to_string()));
        assert!(names(&p, ids[2]).contains(&"AI three".to_string()));
        assert_eq!(names(&p, ids[1]), before[1]);
        assert!(p.undo());
        let after: Vec<_> = ids.iter().map(|id| names(&p, *id)).collect();
        assert_eq!(after, before, "one Undo reverts every panel");
        assert!(p.redo());
        assert!(names(&p, ids[2]).contains(&"AI three".to_string()));
    }

    #[test]
    fn a_locked_panel_refuses_the_whole_batch() {
        let mut p = board(2);
        let ids: Vec<_> = p.page_list().iter().map(|m| m.id).collect();
        p.edit_storyboard(|b| {
            b.panels.get_mut(&ids[1]).unwrap().locked = true;
            Ok(())
        })
        .unwrap();
        let edits = BTreeMap::from([(ids[0], add("A")), (ids[1], add("B"))]);
        let error = p.edit_panels(edits, "AI").unwrap_err();
        assert!(error.contains("locked"), "{error}");
        assert!(!names(&p, ids[0]).contains(&"A".to_string()));
    }
}
