//! Board versions on the open project (see `crate::storyboard_versions`):
//! creating and deleting them, reading the board back at a version or at
//! the last save or export, and keeping removed panels' history while a
//! version shows them.
use super::{PageId, ProjectEditor, ProjectStamp};
use crate::storyboard::Storyboard;
use crate::storyboard_versions::{
    Baseline, BoardState, BoardVersion, BoardVersions, MAX_VERSIONS, Tracking, version_board,
};
use std::collections::{BTreeMap, HashSet};
use std::sync::Arc;

impl ProjectEditor {
    /// Saved board versions, oldest first.
    pub fn board_versions(&self) -> &[BoardVersion] {
        &self.tracking.versions.list
    }

    /// Record the whole board as a named version: each panel's drawing as a
    /// commit in its page history, and the board data beside them. Not an
    /// Undo step; the project needs saving afterwards.
    pub fn create_board_version(&mut self, name: &str) -> Result<u64, String> {
        let name = name.trim();
        crate::storyboard::check_name(name, "Version")?;
        let board = self
            .storyboard
            .as_deref()
            .ok_or("This is not a storyboard project.")?;
        if self.in_transaction() {
            return Err("Finish the current edit first.".into());
        }
        if self.tracking.versions.list.len() >= MAX_VERSIONS {
            return Err(format!(
                "A storyboard keeps at most {MAX_VERSIONS} versions. Delete one first."
            ));
        }
        let board = version_board(board);
        let mut pages = BTreeMap::new();
        for meta in &self.layout {
            let editor = self.pages.get_mut(&meta.id).expect("laid-out page");
            let commit = editor
                .graph
                .record(&editor.doc, name, false)
                .unwrap_or_else(|| editor.graph.head_branch().tip);
            pages.insert(meta.id, commit);
        }
        let versions = &mut self.tracking.versions;
        let id = versions.next_id.max(1);
        versions.next_id = id + 1;
        versions.list.push(BoardVersion {
            id,
            name: name.into(),
            time: crate::storyboard_review::now(),
            layout: self.layout.clone(),
            board,
            pages,
        });
        self.tracking.revision += 1;
        self.tracking.epoch += 1;
        Ok(id)
    }

    /// Forget a version. The page commits it used stay in page history.
    pub fn delete_board_version(&mut self, id: u64) -> Result<(), String> {
        let versions = &mut self.tracking.versions;
        let before = versions.list.len();
        versions.list.retain(|v| v.id != id);
        if versions.list.len() == before {
            return Err("That version does not exist.".into());
        }
        let used = versions.referenced();
        versions.retired.retain(|page, _| used.contains(page));
        self.tracking.revision += 1;
        self.tracking.epoch += 1;
        Ok(())
    }

    /// The board as it is now.
    pub fn current_board_state(&self) -> Option<BoardState> {
        Some(BoardState {
            label: "Current".into(),
            layout: self.layout.clone(),
            board: Storyboard::clone(self.storyboard.as_deref()?),
            docs: self
                .layout
                .iter()
                .map(|m| (m.id, self.pages[&m.id].doc.clone()))
                .collect(),
        })
    }

    /// The board at `baseline`, read from history without changing the
    /// open project.
    pub fn board_state(&self, baseline: Baseline) -> Result<BoardState, String> {
        let shared = |state: &Option<Arc<BoardState>>, what: &str| {
            state
                .as_deref()
                .cloned()
                .ok_or_else(|| format!("Nothing was {what} in this session yet."))
        };
        match baseline {
            Baseline::LastSave => shared(&self.tracking.last_save, "opened or saved"),
            Baseline::LastExport => shared(&self.tracking.last_export, "exported"),
            Baseline::Version(id) => {
                let version = self
                    .tracking
                    .versions
                    .get(id)
                    .ok_or("That version does not exist.")?;
                let docs = version
                    .pages
                    .iter()
                    .filter_map(|(page, commit)| {
                        let graph = self
                            .pages
                            .get(page)
                            .map(|e| &e.graph)
                            .or_else(|| self.tracking.versions.retired.get(page))?;
                        Some((*page, graph.commit(*commit)?.doc.clone()))
                    })
                    .collect();
                Ok(BoardState {
                    label: version.name.clone(),
                    layout: version.layout.clone(),
                    board: version.board.clone(),
                    docs,
                })
            }
        }
    }

    /// Remember the board as exported, for “Changes since last export”.
    pub fn mark_board_exported(&mut self) {
        self.tracking.last_export = self.current_board_state().map(Arc::new);
        self.tracking.epoch += 1;
    }

    /// Changes whenever a version, the last save or the last export does.
    pub fn board_tracking_epoch(&self) -> u64 {
        self.tracking.epoch
    }

    pub(super) fn open_tracking(&mut self, versions: BoardVersions, opened: bool) {
        self.tracking = Tracking {
            versions,
            ..Tracking::default()
        };
        if opened {
            self.tracking.last_save = self.current_board_state().map(Arc::new);
        }
    }

    /// The board with its versions, for saving: removed panels that a
    /// version shows bring their history graphs.
    pub(super) fn board_to_save(&self, board: &Storyboard) -> Storyboard {
        let mut versions = self.tracking.versions.clone();
        let laid_out: HashSet<_> = self.layout.iter().map(|m| m.id).collect();
        versions.retired = versions
            .referenced()
            .into_iter()
            .filter(|id| !laid_out.contains(id))
            .filter_map(|id| {
                let graph = self
                    .pages
                    .get(&id)
                    .map(|e| e.graph.clone())
                    .or_else(|| self.tracking.versions.retired.get(&id).cloned())?;
                Some((id, graph))
            })
            .collect();
        Storyboard {
            versions,
            ..board.clone()
        }
    }

    /// Keep the history of pages about to be dropped while a version shows
    /// them.
    pub(super) fn retire_pages(&mut self, used: &HashSet<PageId>) {
        let shown = self.tracking.versions.referenced();
        for (id, editor) in &self.pages {
            if !used.contains(id) && shown.contains(id) {
                self.tracking
                    .versions
                    .retired
                    .insert(*id, editor.graph.clone());
            }
        }
    }

    pub(super) fn versions_modified(&self) -> bool {
        self.tracking.revision != self.tracking.saved_revision
    }

    /// After a save of `stamp`: versions are saved, and when nothing changed
    /// during the save the board is the new “last save”.
    pub(super) fn tracking_saved(&mut self, stamp: &ProjectStamp) {
        self.tracking.saved_revision = stamp.versions;
        if self.storyboard.is_some() && *stamp == self.stamp() {
            self.tracking.last_save = self.current_board_state().map(Arc::new);
            self.tracking.epoch += 1;
        }
    }
}

#[cfg(test)]
mod tests {
    use crate::project::{ProjectEditor, ProjectKind};
    use crate::storyboard_versions::Baseline;
    use crate::{Command, Document, Node, NodeKind, command::Slot};

    #[test]
    fn versions_read_back_removed_panels_and_survive_a_snapshot() {
        let mut p =
            ProjectEditor::new_project(ProjectKind::Storyboard, Document::new(16, 9)).unwrap();
        let blank = p.storyboard().unwrap().blank_panel().unwrap();
        let panel = emulsion_panel(&p);
        let second = p
            .insert_panels(Some(1), &blank, vec![("Two".into(), panel)], None)
            .unwrap()[0];
        p.set_active_page(second).unwrap();
        p.execute(Command::AddNode {
            node: Box::new(Node::new(0, "Ink", NodeKind::Fill { rgba: [0; 4] })),
            slot: Slot::TOP,
        })
        .unwrap();
        assert!(!p.versions_modified());
        let v = p.create_board_version("First pass").unwrap();
        assert!(p.is_modified() && p.versions_modified());
        assert!(!p.can_redo());
        p.remove_page(second).unwrap();
        let state = p.board_state(Baseline::Version(v)).unwrap();
        assert_eq!(state.order(), vec![1, second]);
        assert_eq!(state.doc(second).unwrap().nodes.len(), 2);
        let snapshot = p.snapshot().unwrap();
        let saved = snapshot.storyboard.as_ref().unwrap();
        assert_eq!(saved.versions.list.len(), 1);
        assert!(saved.versions.retired.contains_key(&second));
        snapshot.validate().unwrap();
        let reopened = ProjectEditor::open(snapshot, Some("b.emu".into())).unwrap();
        assert!(reopened.storyboard().unwrap().versions.is_empty());
        let state = reopened.board_state(Baseline::Version(v)).unwrap();
        assert_eq!(state.doc(second).unwrap().nodes.len(), 2);
        assert!(reopened.board_state(Baseline::LastSave).is_ok());
        assert!(reopened.board_state(Baseline::LastExport).is_err());
        assert!(state.to_project().unwrap().validate().is_ok());
    }

    fn emulsion_panel(p: &ProjectEditor) -> crate::storyboard::Panel {
        let board = p.storyboard().unwrap();
        crate::storyboard::Panel::new(0, board.settings.panel_frames)
    }
}
