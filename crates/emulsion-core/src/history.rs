//! Snapshot history and the editing session.
//!
//! Every recorded step keeps the whole document as it was before the step.
//! Snapshots share pixel buffers through `Arc`, so a step costs a few
//! kilobytes unless it replaced pixels. Undo swaps snapshots; there are no
//! inverse commands to get wrong.

use crate::command::{Command, CommandError};
use crate::document::Document;
use crate::node::NodeId;
use std::collections::HashSet;
use std::path::PathBuf;

const MAX_STEPS: usize = 100;
const MAX_RETAINED_BYTES: usize = 2 << 30;

#[derive(Clone)]
pub struct Step {
    pub name: String,
    /// Document before the step.
    pub before: Document,
    pub revision_before: u64,
}

#[derive(Default)]
pub struct History {
    undo: Vec<Step>,
    redo: Vec<Step>,
}

impl History {
    pub fn can_undo(&self) -> bool {
        !self.undo.is_empty()
    }
    pub fn can_redo(&self) -> bool {
        !self.redo.is_empty()
    }
    /// Most recent first.
    pub fn steps(&self) -> impl Iterator<Item = &Step> {
        self.undo.iter().rev()
    }
    pub fn len(&self) -> usize {
        self.undo.len()
    }
    pub fn is_empty(&self) -> bool {
        self.undo.is_empty()
    }
}

/// An open document with its history and save state.
pub struct Editor {
    pub doc: Document,
    pub history: History,
    /// Bumped on every change, including undo and redo. Render caches key on it.
    pub revision: u64,
    next_revision: u64,
    saved_revision: u64,
    pub path: Option<PathBuf>,
    /// Snapshot at the last commit (open or save), for before/after.
    pub committed: Document,
    pub committed_revision: u64,
    txn: Option<(String, Document, u64, u32)>,
}

impl Editor {
    pub fn new(doc: Document, path: Option<PathBuf>) -> Self {
        Self {
            committed: doc.clone(),
            doc,
            history: History::default(),
            revision: 1,
            next_revision: 2,
            saved_revision: 1,
            path,
            committed_revision: 1,
            txn: None,
        }
    }

    pub fn is_modified(&self) -> bool {
        self.revision != self.saved_revision
    }

    pub fn title(&self) -> String {
        self.path
            .as_ref()
            .and_then(|p| p.file_stem())
            .map(|s| s.to_string_lossy().into_owned())
            .unwrap_or_else(|| "untitled".into())
    }

    fn bump(&mut self) {
        self.revision = self.next_revision;
        self.next_revision += 1;
    }

    /// Run one command as its own history step (or inside the open
    /// transaction). View-only commands change the document without
    /// recording a step.
    pub fn execute(&mut self, cmd: Command) -> Result<Option<NodeId>, CommandError> {
        if self.txn.is_some() || cmd.is_view_only() {
            let before = self.doc.clone();
            let out = cmd.apply(&mut self.doc)?;
            if self.doc != before {
                self.bump();
            }
            return Ok(out);
        }
        let name = cmd.label();
        let before = self.doc.clone();
        let rev = self.revision;
        let out = cmd.apply(&mut self.doc)?;
        if self.doc != before {
            self.bump();
            self.push(Step { name, before, revision_before: rev });
        }
        Ok(out)
    }

    /// Start a transaction: every command until the matching `end` becomes
    /// one step. Nests.
    pub fn begin(&mut self, name: impl Into<String>) {
        match &mut self.txn {
            Some((_, _, _, depth)) => *depth += 1,
            None => self.txn = Some((name.into(), self.doc.clone(), self.revision, 1)),
        }
    }

    pub fn end(&mut self) {
        let Some((name, before, rev, depth)) = self.txn.take() else { return };
        if depth > 1 {
            self.txn = Some((name, before, rev, depth - 1));
            return;
        }
        if self.doc != before {
            self.push(Step { name, before, revision_before: rev });
        }
    }

    pub fn in_transaction(&self) -> bool {
        self.txn.is_some()
    }

    fn push(&mut self, step: Step) {
        self.history.undo.push(step);
        self.history.redo.clear();
        self.trim();
    }

    pub fn undo(&mut self) -> bool {
        self.end_all();
        let Some(step) = self.history.undo.pop() else { return false };
        let current = std::mem::replace(&mut self.doc, step.before);
        let rev = self.revision;
        self.revision = step.revision_before;
        self.history.redo.push(Step { name: step.name, before: current, revision_before: rev });
        true
    }

    pub fn redo(&mut self) -> bool {
        self.end_all();
        let Some(step) = self.history.redo.pop() else { return false };
        let current = std::mem::replace(&mut self.doc, step.before);
        let rev = self.revision;
        self.revision = step.revision_before;
        self.history.undo.push(Step { name: step.name, before: current, revision_before: rev });
        true
    }

    fn end_all(&mut self) {
        while self.txn.is_some() {
            self.end();
        }
    }

    /// Record that `snapshot`, which was the document at `revision`, was
    /// written to `path`, and make it the before/after reference point.
    /// Edits made while the save ran stay unsaved.
    pub fn mark_saved(&mut self, path: PathBuf, revision: u64, snapshot: Document) {
        self.path = Some(path);
        self.saved_revision = revision;
        self.committed = snapshot;
        self.committed_revision = revision;
    }

    /// Revision recorded at the last save (or open).
    pub fn saved_revision(&self) -> u64 {
        self.saved_revision
    }

    pub fn commit(&mut self) {
        self.committed = self.doc.clone();
        self.committed_revision = self.revision;
    }

    /// Drop the oldest steps beyond the count limit or while pixels kept alive
    /// only by history exceed the byte budget.
    fn trim(&mut self) {
        while self.history.undo.len() > MAX_STEPS {
            self.history.undo.remove(0);
        }
        loop {
            let live: HashSet<usize> = self.doc.buffers().into_iter().map(|(p, _)| p).collect();
            let mut seen = HashSet::new();
            let mut bytes = 0usize;
            for s in self.history.undo.iter().chain(&self.history.redo) {
                for (p, b) in s.before.buffers() {
                    if !live.contains(&p) && seen.insert(p) {
                        bytes += b;
                    }
                }
            }
            if bytes <= MAX_RETAINED_BYTES || self.history.undo.len() <= 1 {
                break;
            }
            self.history.undo.remove(0);
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::command::Slot;
    use crate::node::Node;
    use emulsion_raster::{Placement, Raster};
    use std::sync::Arc;

    fn editor() -> (Editor, NodeId) {
        let mut d = Document::new(32, 32);
        let id = Command::AddNode {
            node: Box::new(Node::raster(0, "a", Arc::new(Raster::transparent(32, 32)), Placement::default())),
            slot: Slot::TOP,
        }
        .apply(&mut d)
        .unwrap()
        .unwrap();
        (Editor::new(d, None), id)
    }

    #[test]
    fn undo_redo_restore_revisions() {
        let (mut e, id) = editor();
        let r0 = e.revision;
        e.execute(Command::SetOpacity { id, opacity: 0.5 }).unwrap();
        let r1 = e.revision;
        assert_ne!(r0, r1);
        assert!(e.is_modified());
        e.undo();
        assert_eq!(e.revision, r0);
        assert_eq!(e.doc.node(id).unwrap().opacity, 1.0);
        assert!(!e.is_modified());
        e.redo();
        assert_eq!(e.revision, r1);
        assert_eq!(e.doc.node(id).unwrap().opacity, 0.5);
    }

    #[test]
    fn transaction_is_one_step_and_noops_record_nothing() {
        let (mut e, id) = editor();
        e.begin("Opacity");
        for v in [0.9, 0.8, 0.7] {
            e.execute(Command::SetOpacity { id, opacity: v }).unwrap();
        }
        e.end();
        assert_eq!(e.history.len(), 1);
        e.execute(Command::SetOpacity { id, opacity: 0.7 }).unwrap();
        assert_eq!(e.history.len(), 1, "no-op is not a step");
        e.undo();
        assert_eq!(e.doc.node(id).unwrap().opacity, 1.0);
    }

    #[test]
    fn new_edit_clears_redo() {
        let (mut e, id) = editor();
        e.execute(Command::SetVisible { id, visible: false }).unwrap();
        e.undo();
        assert!(e.history.can_redo());
        e.execute(Command::Rename { id, name: "b".into() }).unwrap();
        assert!(!e.history.can_redo());
    }
}
