//! Drawings brought back from paper worksheets: each lands on its panel as
//! a new layer and each new-panel slot becomes a panel, all as one project
//! Undo step.
use super::{PageId, ProjectEditor};
use crate::history::edit_order;
use crate::{Command, Document, Node, command::Slot};
use emulsion_raster::{Placement, Raster};
use std::collections::BTreeMap;
use std::sync::Arc;

/// Layers made from paper drawings start with this name.
pub const PAPER_LAYER_PREFIX: &str = "Paper drawing";

/// What placing paper drawings changed.
#[derive(Clone, Debug, Default, PartialEq, Eq)]
pub struct PaperPlaced {
    /// Existing panels that took a drawing, in panel order.
    pub changed: Vec<PageId>,
    /// Panels made for new-panel slots, in board order.
    pub added: Vec<PageId>,
}

/// The commands that put `raster` on `doc` as the top layer `name`; with
/// `replace`, earlier paper drawing layers are removed first.
fn drawing_commands(
    doc: &Document,
    raster: Arc<Raster>,
    name: &str,
    replace: bool,
) -> Vec<Command> {
    let mut commands: Vec<_> = if replace {
        doc.nodes
            .iter()
            .filter(|n| n.parent.is_none() && n.name.starts_with(PAPER_LAYER_PREFIX))
            .map(|n| Command::RemoveNode { id: n.id })
            .collect()
    } else {
        Vec::new()
    };
    commands.push(Command::AddNode {
        node: Box::new(Node::raster(0, name, raster, Placement::default())),
        slot: Slot::TOP,
    });
    commands
}

impl ProjectEditor {
    /// Put each drawing on its panel as a new top layer named `layer` (with
    /// `replace`, the panel's earlier paper drawing layers go), and add one
    /// blank panel per `new_panels` drawing after `after`, carrying it. One
    /// Undo step for everything; nothing changes when any panel is locked or
    /// missing. Drawings are at the board's resolution.
    pub fn place_paper_drawings(
        &mut self,
        drawings: Vec<(PageId, Arc<Raster>)>,
        new_panels: Vec<(String, Arc<Raster>)>,
        after: Option<PageId>,
        layer: &str,
        replace: bool,
    ) -> Result<PaperPlaced, String> {
        const LABEL: &str = "Import paper worksheets";
        if drawings.is_empty() && new_panels.is_empty() {
            return Err("There are no drawings to place.".into());
        }
        let board = self.board()?.clone();
        let size = (board.settings.width, board.settings.height);
        if drawings
            .iter()
            .map(|(_, r)| r)
            .chain(new_panels.iter().map(|(_, r)| r))
            .any(|r| (r.width(), r.height()) != size)
        {
            return Err("Paper drawings must match the panel size.".into());
        }
        let mut edits: BTreeMap<PageId, Vec<Command>> = BTreeMap::new();
        for (panel, raster) in drawings {
            let doc = &self.page(panel).ok_or("Panel does not exist.")?.doc;
            // A panel drawn on twice keeps both drawings; only layers from
            // earlier imports are replaced.
            let replace = replace && !edits.contains_key(&panel);
            let commands = drawing_commands(doc, raster, layer, replace);
            edits.entry(panel).or_default().extend(commands);
        }
        let documents = self.edited_panels(edits)?;
        let changed = self.changed_panels(&documents);
        let documents = self.prepare_documents(documents, LABEL)?;
        let (added, order) = if new_panels.is_empty() {
            (Vec::new(), edit_order())
        } else {
            let blank = board.blank_panel()?;
            let mut items = Vec::new();
            for (name, raster) in new_panels {
                let mut doc = blank.clone();
                for command in drawing_commands(&doc, raster, layer, false) {
                    command.apply(&mut doc).map_err(|e| e.to_string())?;
                }
                items.push((name, doc));
            }
            let added = self.import_panels(after, items)?;
            // The drawings share the new panels' step, so they undo together.
            (added, self.last_page_edit)
        };
        self.apply_documents(documents, LABEL, order);
        Ok(PaperPlaced { changed, added })
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::project::ProjectKind;
    use crate::storyboard::Panel;

    fn board() -> ProjectEditor {
        let mut p =
            ProjectEditor::new_project(ProjectKind::Storyboard, Document::new(16, 9)).unwrap();
        let blank = p.storyboard().unwrap().blank_panel().unwrap();
        p.insert_panels(
            Some(1),
            &blank,
            vec![("Panel 2".into(), Panel::new(0, 24))],
            None,
        )
        .unwrap();
        p
    }

    fn ink() -> Arc<Raster> {
        Arc::new(Raster::solid(16, 9, [0., 0., 0., 1.]))
    }

    fn layers(p: &ProjectEditor, id: PageId) -> Vec<String> {
        let doc = &p.page(id).unwrap().doc;
        doc.nodes
            .iter()
            .filter(|n| n.name.starts_with(PAPER_LAYER_PREFIX))
            .map(|n| n.name.clone())
            .collect()
    }

    #[test]
    fn drawings_and_new_panels_land_and_undo_as_one_step() {
        let mut p = board();
        let ids: Vec<_> = p.page_list().iter().map(|m| m.id).collect();
        let placed = p
            .place_paper_drawings(
                vec![(ids[0], ink()), (ids[1], ink())],
                vec![("Paper 1".into(), ink())],
                Some(ids[1]),
                "Paper drawing (2026-10-03)",
                false,
            )
            .unwrap();
        assert_eq!(placed.changed, ids);
        assert_eq!(placed.added.len(), 1);
        assert_eq!(p.page_list().len(), 3);
        assert_eq!(layers(&p, ids[0]), ["Paper drawing (2026-10-03)"]);
        assert_eq!(layers(&p, placed.added[0]).len(), 1);
        assert!(p.undo());
        assert_eq!(p.page_list().len(), 2, "one Undo removes the new panel");
        assert!(layers(&p, ids[0]).is_empty(), "and the drawings");
        assert!(layers(&p, ids[1]).is_empty());
        assert!(p.redo());
        assert_eq!(p.page_list().len(), 3);
        assert_eq!(layers(&p, ids[1]).len(), 1);
    }

    #[test]
    fn replace_removes_earlier_paper_layers_and_locks_refuse() {
        let mut p = board();
        let ids: Vec<_> = p.page_list().iter().map(|m| m.id).collect();
        for (name, replace) in [("Paper drawing (a)", false), ("Paper drawing (b)", true)] {
            p.place_paper_drawings(vec![(ids[0], ink())], vec![], None, name, replace)
                .unwrap();
        }
        assert_eq!(layers(&p, ids[0]), ["Paper drawing (b)"]);
        let wrong = Arc::new(Raster::solid(8, 8, [0., 0., 0., 1.]));
        assert!(
            p.place_paper_drawings(vec![(ids[0], wrong)], vec![], None, "x", false)
                .is_err()
        );
        p.edit_storyboard(|b| {
            b.panels.get_mut(&ids[1]).unwrap().locked = true;
            Ok(())
        })
        .unwrap();
        let error = p
            .place_paper_drawings(
                vec![(ids[1], ink())],
                vec![("New".into(), ink())],
                None,
                "Paper drawing (c)",
                false,
            )
            .unwrap_err();
        assert!(error.contains("locked"), "{error}");
        assert_eq!(p.page_list().len(), 2, "nothing was added");
    }
}
