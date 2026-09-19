//! Snapshot history and the editing session.
//!
//! Every recorded step keeps the whole document as it was before the step.
//! Snapshots share pixel buffers through `Arc`, so a step costs a few
//! kilobytes unless it replaced pixels. Undo swaps snapshots; there are no
//! inverse commands to get wrong.

use crate::command::{Command, CommandError, Dirty};
use crate::document::Document;
use crate::graph::{CommitId, ConflictKey, Graph, GraphError, MergeOutcome, Side};
use crate::node::NodeId;
use std::collections::{HashMap, HashSet};
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

#[derive(Default, Clone)]
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
    /// The head branch's base commit, which before/after compares against.
    pub committed: Document,
    /// Changes whenever `committed` does, so views can re-render it.
    pub committed_revision: u64,
    /// Commits and branches.
    pub graph: Graph,
    /// Undo stacks of the branches that are not checked out.
    stashed: HashMap<String, History>,
    txn: Option<(String, Document, u64, u32)>,
    /// What changed on screen since the last `take_dirty`.
    dirty: Dirty,
}

impl Editor {
    pub fn new(doc: Document, path: Option<PathBuf>) -> Self {
        let name = if path.is_some() { "Opened" } else { "New" };
        let graph = Graph::new(doc.clone(), name);
        Self::with_graph(doc, path, graph)
    }

    /// An editor on a document whose history graph was read from its file.
    pub fn with_graph(doc: Document, path: Option<PathBuf>, graph: Graph) -> Self {
        let base = graph.head_branch().base;
        let committed = graph
            .commit(base)
            .map_or_else(|| doc.clone(), |c| c.doc.clone());
        Self {
            committed,
            doc,
            history: History::default(),
            revision: 1,
            next_revision: 2,
            saved_revision: 1,
            path,
            committed_revision: 1,
            graph,
            stashed: HashMap::new(),
            txn: None,
            dirty: Dirty::All,
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
    /// Region changed since the last call (renderers re-render only this).
    pub fn take_dirty(&mut self) -> Dirty {
        std::mem::replace(&mut self.dirty, Dirty::Nothing)
    }

    pub fn execute(&mut self, cmd: Command) -> Result<Option<NodeId>, CommandError> {
        let d = cmd.dirty(&self.doc);
        let r = self.execute_inner(cmd);
        if r.is_ok() {
            self.dirty = self.dirty.union(d);
        }
        r
    }

    fn execute_inner(&mut self, cmd: Command) -> Result<Option<NodeId>, CommandError> {
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
            self.push(Step {
                name,
                before,
                revision_before: rev,
            });
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
        let Some((name, before, rev, depth)) = self.txn.take() else {
            return;
        };
        if depth > 1 {
            self.txn = Some((name, before, rev, depth - 1));
            return;
        }
        if self.doc != before {
            self.push(Step {
                name,
                before,
                revision_before: rev,
            });
        }
    }

    pub fn in_transaction(&self) -> bool {
        self.txn.is_some()
    }

    /// Abandon the open transaction: the document returns to how it was
    /// when the outermost `begin` ran, and nothing reaches the history.
    pub fn cancel(&mut self) {
        if let Some((_, before, rev, _)) = self.txn.take() {
            self.doc = before;
            self.revision = rev.max(self.revision) + 1;
        }
    }

    fn push(&mut self, step: Step) {
        self.history.undo.push(step);
        self.history.redo.clear();
        self.trim();
    }

    pub fn undo(&mut self) -> bool {
        self.end_all();
        let Some(step) = self.history.undo.pop() else {
            return false;
        };
        self.dirty = Dirty::All;
        let current = std::mem::replace(&mut self.doc, step.before);
        let rev = self.revision;
        self.revision = step.revision_before;
        self.history.redo.push(Step {
            name: step.name,
            before: current,
            revision_before: rev,
        });
        true
    }

    pub fn redo(&mut self) -> bool {
        self.end_all();
        let Some(step) = self.history.redo.pop() else {
            return false;
        };
        self.dirty = Dirty::All;
        let current = std::mem::replace(&mut self.doc, step.before);
        let rev = self.revision;
        self.revision = step.revision_before;
        self.history.undo.push(Step {
            name: step.name,
            before: current,
            revision_before: rev,
        });
        true
    }

    fn end_all(&mut self) {
        while self.txn.is_some() {
            self.end();
        }
    }

    /// Record that the document at `revision` was written to `path`.
    /// Edits made while the save ran stay unsaved.
    pub fn mark_saved(&mut self, path: PathBuf, revision: u64) {
        self.path = Some(path);
        self.saved_revision = revision;
    }

    /// Revision recorded at the last save (or open).
    pub fn saved_revision(&self) -> u64 {
        self.saved_revision
    }

    // ── History graph ───────────────────────────────────────────────────

    /// Record the document as a commit on the head branch. None when
    /// nothing changed since the branch's newest commit.
    pub fn commit(&mut self, name: impl Into<String>, auto: bool) -> Option<CommitId> {
        self.end_all();
        self.graph.record(&self.doc, name, auto)
    }

    /// Whether the document differs from the head branch's newest commit.
    pub fn uncommitted(&self) -> bool {
        self.graph
            .commit(self.graph.head_branch().tip)
            .is_none_or(|c| c.doc != self.doc)
    }

    /// Whether the document differs from what before/after compares against.
    pub fn differs_from_base(&self) -> bool {
        self.doc != self.committed
    }

    fn refresh_base(&mut self) {
        let base = self.graph.head_branch().base;
        if let Some(c) = self.graph.commit(base)
            && c.doc != self.committed
        {
            self.committed = c.doc.clone();
            self.committed_revision = self.next_revision;
            self.next_revision += 1;
        }
    }

    /// Start a branch from the current state and switch to it. The work so
    /// far is committed first, so nothing is left behind.
    pub fn branch(&mut self, name: &str) -> Result<(), GraphError> {
        self.end_all();
        if !crate::graph::valid_branch_name(name) {
            return Err(GraphError::BadName);
        }
        if self.graph.branches().contains_key(name) {
            return Err(GraphError::BranchExists(name.into()));
        }
        self.graph.record(&self.doc, "Before branching", false);
        let at = self.graph.head_branch().tip;
        self.graph.create_branch(name, at)?;
        let old = self.graph.head().to_string();
        self.graph.set_head(name)?;
        // The new branch keeps the undo stack it grew out of.
        self.stashed.insert(old, self.history.clone());
        self.refresh_base();
        Ok(())
    }

    /// Start a branch at an earlier commit and switch to it.
    pub fn branch_at(&mut self, name: &str, at: CommitId) -> Result<(), GraphError> {
        self.end_all();
        self.graph.record(&self.doc, "Work in progress", true);
        self.graph.create_branch(name, at)?;
        self.checkout(name)
    }

    /// Switch to another branch, committing the current one's work first.
    pub fn checkout(&mut self, name: &str) -> Result<(), GraphError> {
        let target = self.graph.branch(name)?;
        if name == self.graph.head() {
            return Ok(());
        }
        self.end_all();
        self.graph.record(&self.doc, "Work in progress", true);
        let old = self.graph.head().to_string();
        self.graph.set_head(name)?;
        let history = self.stashed.remove(name).unwrap_or_default();
        self.stashed
            .insert(old, std::mem::replace(&mut self.history, history));
        self.doc = self
            .graph
            .commit(target.tip)
            .ok_or(GraphError::NoCommit(target.tip))?
            .doc
            .clone();
        self.bump();
        self.dirty = Dirty::All;
        self.refresh_base();
        Ok(())
    }

    pub fn delete_branch(&mut self, name: &str) -> Result<(), GraphError> {
        self.graph.delete_branch(name)?;
        self.stashed.remove(name);
        Ok(())
    }

    /// Merge branch `from` into the head branch. Conflicts come back unless
    /// `choices` decides each one; the merge is one undo step and a commit.
    pub fn merge(
        &mut self,
        from: &str,
        choices: &HashMap<ConflictKey, Side>,
    ) -> Result<MergeOutcome, GraphError> {
        if from == self.graph.head() {
            return Err(GraphError::SelfMerge);
        }
        let theirs_tip = self.graph.branch(from)?.tip;
        self.end_all();
        self.graph.record(&self.doc, "Before merging", false);
        let ours_tip = self.graph.head_branch().tip;
        if self.graph.ancestors(ours_tip).contains(&theirs_tip) {
            return Err(GraphError::NothingToMerge(from.into()));
        }
        let base_id = self
            .graph
            .merge_base(ours_tip, theirs_tip)
            .ok_or_else(|| GraphError::Invalid("the branches share no history".into()))?;
        let (base, theirs) = (
            &self.graph.commit(base_id).expect("base").doc,
            &self.graph.commit(theirs_tip).expect("tip").doc,
        );
        let outcome = crate::graph::merge(base, &self.doc, theirs, choices)?;
        if let MergeOutcome::Merged(doc) = &outcome {
            let label = format!("Merge {from}");
            self.replace_document(doc.clone(), &label);
            self.graph.record_merge(
                doc,
                theirs_tip,
                format!("Merge {from} into {}", self.graph.head()),
            );
        }
        Ok(outcome)
    }

    /// Bring back the document as it was at `commit`, as one undo step.
    pub fn restore(&mut self, commit: CommitId) -> Result<(), GraphError> {
        let c = self
            .graph
            .commit(commit)
            .ok_or(GraphError::NoCommit(commit))?;
        let (doc, label) = (c.doc.clone(), format!("Restore “{}”", c.name));
        self.replace_document(doc, &label);
        Ok(())
    }

    /// Swap in a whole document as one undo step.
    fn replace_document(&mut self, doc: Document, label: &str) {
        self.end_all();
        if doc == self.doc {
            return;
        }
        let rev = self.revision;
        let before = std::mem::replace(&mut self.doc, doc);
        self.bump();
        self.dirty = Dirty::All;
        self.push(Step {
            name: label.into(),
            before,
            revision_before: rev,
        });
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
            node: Box::new(Node::raster(
                0,
                "a",
                Arc::new(Raster::transparent(32, 32)),
                Placement::default(),
            )),
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
    fn dirty_regions_accumulate_and_undo_is_everything() {
        let (mut e, id) = editor();
        let _ = e.take_dirty();
        e.execute(Command::Rename {
            id,
            name: "x".into(),
        })
        .unwrap();
        assert_eq!(
            e.take_dirty(),
            Dirty::Nothing,
            "renaming changes nothing on screen"
        );
        let r = std::sync::Arc::new(Raster::transparent(32, 32));
        e.execute(Command::ReplacePixels {
            id,
            raster: r,
            dirty: emulsion_raster::IRect::new(2, 3, 4, 5),
            label: "Paint".into(),
        })
        .unwrap();
        assert!(matches!(e.take_dirty(), Dirty::Rect(r) if r.x <= 2 && r.right() >= 6));
        e.undo();
        assert_eq!(e.take_dirty(), Dirty::All);
    }

    #[test]
    fn branch_switch_merge_and_restore() {
        let (mut e, id) = editor();
        e.execute(Command::SetOpacity { id, opacity: 0.8 }).unwrap();
        e.branch("retouch").unwrap();
        assert_eq!(e.graph.head(), "retouch");
        assert!(!e.differs_from_base(), "a new branch starts at its base");
        e.execute(Command::Rename {
            id,
            name: "retouched".into(),
        })
        .unwrap();
        assert!(e.differs_from_base());
        e.checkout("main").unwrap();
        assert_eq!(e.doc.node(id).unwrap().name, "a");
        assert_eq!(e.doc.node(id).unwrap().opacity, 0.8);
        e.execute(Command::SetVisible { id, visible: false })
            .unwrap();
        let MergeOutcome::Merged(_) = e.merge("retouch", &HashMap::new()).unwrap() else {
            panic!("clean merge expected");
        };
        let n = e.doc.node(id).unwrap();
        assert!(n.name == "retouched" && !n.visible);
        assert_eq!(
            e.merge("retouch", &HashMap::new()).unwrap_err(),
            GraphError::NothingToMerge("retouch".into())
        );
        e.undo();
        assert_eq!(e.doc.node(id).unwrap().name, "a", "merge is one undo step");
        let first = e.graph.commits().next().unwrap().id;
        e.restore(first).unwrap();
        assert_eq!(e.doc.node(id).unwrap().opacity, 1.0);
        e.checkout("retouch").unwrap();
        assert!(e.history.can_undo(), "each branch keeps its own undo stack");
    }

    #[test]
    fn new_edit_clears_redo() {
        let (mut e, id) = editor();
        e.execute(Command::SetVisible { id, visible: false })
            .unwrap();
        e.undo();
        assert!(e.history.can_redo());
        e.execute(Command::Rename {
            id,
            name: "b".into(),
        })
        .unwrap();
        assert!(!e.history.can_redo());
    }
}
