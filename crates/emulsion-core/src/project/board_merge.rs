//! Shared-project merges on the open project (see `crate::storyboard_merge`):
//! the merged board replaces the open one as one Undo step that changes the
//! page layout, the board data and the drawings of changed panels together.
//! Board versions brought in by the merge sit outside Undo, like every
//! board version.
use super::{PageId, ProjectEditor};
use crate::project::Project;
use crate::storyboard_merge::{BoardMergeReport, ConflictKey, Resolution, merge_boards};
use crate::{Editor, history::edit_order};
use std::collections::BTreeMap;
use std::sync::Arc;

impl ProjectEditor {
    /// What merging `theirs` (another copy of this board, descended from
    /// `base`) would do with these choices. Changes nothing.
    pub fn plan_board_merge(
        &self,
        base: &Project,
        theirs: &Project,
        resolutions: &BTreeMap<ConflictKey, Resolution>,
    ) -> Result<BoardMergeReport, String> {
        let ours = self
            .snapshot()
            .filter(|p| p.storyboard.is_some())
            .ok_or("This is not a storyboard project.")?;
        Ok(merge_boards(base, &ours, theirs, resolutions)?.report)
    }

    /// Merge `theirs` into the open board as one Undo step, resolving
    /// conflicts by `resolutions` (others keep mine). `revision` names the
    /// cloud revision merged, recorded on the board so its next upload has
    /// both heads as parents.
    pub fn merge_board(
        &mut self,
        base: &Project,
        theirs: &Project,
        resolutions: &BTreeMap<ConflictKey, Resolution>,
        revision: Option<&str>,
    ) -> Result<BoardMergeReport, String> {
        if self.in_transaction() {
            return Err("Finish the current edit before merging.".into());
        }
        let ours = self
            .snapshot()
            .filter(|p| p.storyboard.is_some())
            .ok_or("This is not a storyboard project.")?;
        let mut merged = merge_boards(base, &ours, theirs, resolutions)?;
        if let Some(revision) = revision {
            let board = merged.project.storyboard.as_mut().unwrap();
            board.sharing.merged_revision = Some(revision.to_string());
            board.validate(
                &merged
                    .project
                    .pages
                    .iter()
                    .map(|p| p.meta.id)
                    .collect::<Vec<_>>(),
            )?;
        }
        self.replace_project_state(merged.project, "Merge shared changes")?;
        Ok(merged.report)
    }

    /// Make `project` (a later state of this storyboard with the same page
    /// identities) the open state as one Undo step. Unchanged pages keep
    /// their editors; changed drawings are committed on their page with the
    /// same order as the layout step, so Undo and Redo travel together.
    fn replace_project_state(&mut self, mut project: Project, label: &str) -> Result<(), String> {
        self.check_page_edit()?;
        let current = self.board()?.clone();
        let mut board = project
            .storyboard
            .take()
            .ok_or("Missing storyboard data.")?;
        let versions = std::mem::take(&mut board.versions);
        current.check_locks_kept(&board)?;
        let layout: Vec<_> = project.pages.iter().map(|p| p.meta.clone()).collect();
        for page in &project.pages {
            page.meta.validate()?;
            if let Some(editor) = self.pages.get(&page.meta.id) {
                if editor.in_transaction() {
                    return Err("Finish edits on every affected page first.".into());
                }
                if editor.doc != page.doc && current.is_locked(page.meta.id) {
                    return Err(format!(
                        "Panel {} is locked here and changed in the other copy. Unlock it first.",
                        page.meta.name
                    ));
                }
            }
            self.check_panel_size(&page.doc)?;
        }
        let order = edit_order();
        self.last_page_edit = order;
        self.undo_pages.push(self.page_step(order));
        self.redo_pages.clear();
        if self.undo_pages.len() > super::MAX_PAGE_STEPS {
            self.undo_pages.remove(0);
        }
        let path = self.path.clone();
        let mut affected: Vec<PageId> = Vec::new();
        for page in project.pages {
            let id = page.meta.id;
            let laid_out = self.layout.iter().any(|m| m.id == id);
            match self.pages.get_mut(&id) {
                Some(editor) => {
                    // A page laid out here keeps its history plus the
                    // commits of versions the merge brought in.
                    if laid_out {
                        editor.graph = page.graph;
                    }
                    if editor.doc != page.doc {
                        editor.commit_project_document(page.doc, label, order);
                        affected.push(id);
                    }
                }
                None => {
                    self.pages
                        .insert(id, Editor::with_graph(page.doc, path.clone(), page.graph));
                }
            }
        }
        if affected.len() > 1 {
            self.history_groups.insert(order, affected);
        }
        self.layout = layout;
        self.next_page_id = self.next_page_id.max(project.next_page_id);
        if self.layout.iter().any(|m| m.id == project.active) {
            self.active = project.active;
        }
        self.storyboard = Some(Arc::new(board));
        if self.tracking.versions != versions {
            self.tracking.versions = versions;
            self.tracking.revision += 1;
            self.tracking.epoch += 1;
        }
        self.collect_pages();
        self.refresh_locks();
        Ok(())
    }
}
