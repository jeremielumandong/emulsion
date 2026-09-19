//! The history graph: named commits on branches, and three-way merge.
//!
//! Steps (see [`crate::history`]) are the fine-grained undo stack of one
//! branch. Commits are the coarse points worth keeping: the opened file, a
//! save, an export, a branch or merge, and an autosave every few seconds.
//! A commit holds a whole [`Document`] snapshot; snapshots share pixel tiles
//! through `Arc`, so a commit costs little beyond the tiles it changed.
//!
//! A branch is a named pointer to its newest commit plus the commit it was
//! created from (its *base*), which is what before/after compares against.
//! Merging replays the other branch's node changes onto this one. A node
//! changed on both sides is a conflict, and conflicts are never resolved by
//! guessing: the caller passes an explicit choice for each, or gets the list
//! back to ask the person.

use crate::document::{Document, DocumentError};
use crate::node::{Node, NodeId, NodeKind};
use std::collections::{BTreeMap, BTreeSet, HashMap, HashSet};

pub type CommitId = u64;

pub const MAIN: &str = "main";
/// Autosave commits kept per graph. Older ones are folded away when they
/// are not a branch tip, a branch base or a merge point.
pub const KEEP_AUTO: usize = 30;
pub const MAX_COMMITS: usize = 2000;
pub const MAX_BRANCH_NAME: usize = 64;

#[derive(Clone, Debug)]
pub struct Commit {
    pub id: CommitId,
    /// One parent, two for a merge, none for the root.
    pub parents: Vec<CommitId>,
    pub name: String,
    /// Seconds since the Unix epoch.
    pub time: u64,
    /// Made by autosave rather than by the person.
    pub auto: bool,
    /// The branch this commit was made on, for laying out columns.
    pub branch: String,
    pub doc: Document,
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct Branch {
    /// Newest commit.
    pub tip: CommitId,
    /// The commit this branch was created from (the root for main).
    pub base: CommitId,
}

#[derive(Debug, thiserror::Error, PartialEq)]
pub enum GraphError {
    #[error("there is no branch named {0:?}")]
    NoBranch(String),
    #[error("a branch named {0:?} already exists")]
    BranchExists(String),
    #[error("branch names must be 1–64 characters of letters, digits, spaces, - _ . or /")]
    BadName,
    #[error("there is no commit {0}")]
    NoCommit(CommitId),
    #[error("main cannot be deleted")]
    DeleteMain,
    #[error("switch to another branch before deleting {0:?}")]
    DeleteHead(String),
    #[error("{0:?} has nothing new to merge")]
    NothingToMerge(String),
    #[error("a branch cannot be merged into itself")]
    SelfMerge,
    #[error("the history graph is invalid: {0}")]
    Invalid(String),
    #[error("the merged document is invalid: {0}")]
    Document(#[from] DocumentError),
}

fn now() -> u64 {
    std::time::SystemTime::now()
        .duration_since(std::time::UNIX_EPOCH)
        .map(|d| d.as_secs())
        .unwrap_or(0)
}

pub fn valid_branch_name(name: &str) -> bool {
    let n = name.trim();
    !n.is_empty()
        && n == name
        && n.chars().count() <= MAX_BRANCH_NAME
        && n.chars()
            .all(|c| c.is_alphanumeric() || matches!(c, ' ' | '-' | '_' | '.' | '/'))
}

#[derive(Clone, Debug)]
pub struct Graph {
    commits: BTreeMap<CommitId, Commit>,
    branches: BTreeMap<String, Branch>,
    head: String,
    next_id: CommitId,
}

impl Graph {
    /// A graph with one root commit on main.
    pub fn new(doc: Document, name: impl Into<String>) -> Self {
        let root = Commit {
            id: 1,
            parents: Vec::new(),
            name: name.into(),
            time: now(),
            auto: false,
            branch: MAIN.into(),
            doc,
        };
        Self {
            commits: BTreeMap::from([(1, root)]),
            branches: BTreeMap::from([(MAIN.to_string(), Branch { tip: 1, base: 1 })]),
            head: MAIN.into(),
            next_id: 2,
        }
    }

    /// Rebuild a graph read from a file, checking every reference.
    pub fn from_parts(
        commits: Vec<Commit>,
        branches: BTreeMap<String, Branch>,
        head: String,
    ) -> Result<Self, GraphError> {
        let bad = |m: String| Err(GraphError::Invalid(m));
        if commits.is_empty() || commits.len() > MAX_COMMITS {
            return bad(format!(
                "{} commits (1–{MAX_COMMITS} allowed)",
                commits.len()
            ));
        }
        let mut map = BTreeMap::new();
        for c in commits {
            // Parents must be older, which also rules out cycles.
            if c.parents.len() > 2 || c.parents.iter().any(|p| *p >= c.id || !map.contains_key(p)) {
                return bad(format!("commit {} has bad parents", c.id));
            }
            c.doc.validate()?;
            if map.insert(c.id, c).is_some() {
                return bad("duplicate commit id".into());
            }
        }
        if !branches.contains_key(MAIN) || !branches.contains_key(&head) {
            return bad("main or head branch missing".into());
        }
        for (name, b) in &branches {
            if !valid_branch_name(name) {
                return bad(format!("bad branch name {name:?}"));
            }
            if !map.contains_key(&b.tip) || !map.contains_key(&b.base) {
                return bad(format!("branch {name:?} points at a missing commit"));
            }
        }
        let next_id = map.keys().next_back().copied().unwrap_or(0) + 1;
        Ok(Self {
            commits: map,
            branches,
            head,
            next_id,
        })
    }

    pub fn head(&self) -> &str {
        &self.head
    }
    pub fn branches(&self) -> &BTreeMap<String, Branch> {
        &self.branches
    }
    pub fn branch(&self, name: &str) -> Result<Branch, GraphError> {
        self.branches
            .get(name)
            .copied()
            .ok_or_else(|| GraphError::NoBranch(name.into()))
    }
    pub fn head_branch(&self) -> Branch {
        self.branches[&self.head]
    }
    /// Oldest first.
    pub fn commits(&self) -> impl DoubleEndedIterator<Item = &Commit> {
        self.commits.values()
    }
    pub fn commit(&self, id: CommitId) -> Option<&Commit> {
        self.commits.get(&id)
    }
    pub fn len(&self) -> usize {
        self.commits.len()
    }
    pub fn is_empty(&self) -> bool {
        self.commits.is_empty()
    }

    /// Record `doc` on the head branch. Returns None when it equals the tip.
    pub fn record(
        &mut self,
        doc: &Document,
        name: impl Into<String>,
        auto: bool,
    ) -> Option<CommitId> {
        let tip = self.head_branch().tip;
        if self.commits[&tip].doc == *doc {
            return None;
        }
        let id = self.push(vec![tip], name.into(), auto, doc.clone());
        self.branches.get_mut(&self.head).expect("head").tip = id;
        if auto {
            self.prune();
        }
        Some(id)
    }

    fn push(
        &mut self,
        parents: Vec<CommitId>,
        name: String,
        auto: bool,
        doc: Document,
    ) -> CommitId {
        let id = self.next_id;
        self.next_id += 1;
        self.commits.insert(
            id,
            Commit {
                id,
                parents,
                name,
                time: now(),
                auto,
                branch: self.head.clone(),
                doc,
            },
        );
        id
    }

    /// Create `name` at commit `at` without switching to it.
    pub fn create_branch(&mut self, name: &str, at: CommitId) -> Result<(), GraphError> {
        if !valid_branch_name(name) {
            return Err(GraphError::BadName);
        }
        if self.branches.contains_key(name) {
            return Err(GraphError::BranchExists(name.into()));
        }
        if !self.commits.contains_key(&at) {
            return Err(GraphError::NoCommit(at));
        }
        self.branches
            .insert(name.into(), Branch { tip: at, base: at });
        Ok(())
    }

    /// Make `name` the head branch.
    pub fn set_head(&mut self, name: &str) -> Result<(), GraphError> {
        self.branch(name)?;
        self.head = name.into();
        Ok(())
    }

    pub fn delete_branch(&mut self, name: &str) -> Result<(), GraphError> {
        if name == MAIN {
            return Err(GraphError::DeleteMain);
        }
        if name == self.head {
            return Err(GraphError::DeleteHead(name.into()));
        }
        self.branches
            .remove(name)
            .ok_or_else(|| GraphError::NoBranch(name.into()))?;
        self.gc();
        Ok(())
    }

    /// Record a merge commit on the head branch with the other tip as second parent.
    pub fn record_merge(
        &mut self,
        doc: &Document,
        theirs: CommitId,
        name: impl Into<String>,
    ) -> CommitId {
        let tip = self.head_branch().tip;
        let id = self.push(vec![tip, theirs], name.into(), false, doc.clone());
        self.branches.get_mut(&self.head).expect("head").tip = id;
        id
    }

    /// Every commit reachable from `id`, including itself.
    pub fn ancestors(&self, id: CommitId) -> HashSet<CommitId> {
        let mut seen = HashSet::new();
        let mut stack = vec![id];
        while let Some(c) = stack.pop() {
            if seen.insert(c)
                && let Some(commit) = self.commits.get(&c)
            {
                stack.extend(&commit.parents);
            }
        }
        seen
    }

    /// The newest common ancestor of two commits. Ids grow with time, so
    /// the largest common id is never an ancestor of another common one.
    pub fn merge_base(&self, a: CommitId, b: CommitId) -> Option<CommitId> {
        let aa = self.ancestors(a);
        self.ancestors(b)
            .into_iter()
            .filter(|c| aa.contains(c))
            .max()
    }

    /// Commits on `name` since it last shared history with `other`.
    pub fn ahead(&self, name: &str, other: &str) -> usize {
        let (Ok(a), Ok(b)) = (self.branch(name), self.branch(other)) else {
            return 0;
        };
        let theirs = self.ancestors(b.tip);
        self.ancestors(a.tip)
            .iter()
            .filter(|c| !theirs.contains(c))
            .count()
    }

    /// Fold away old autosave commits and drop unreachable ones.
    fn prune(&mut self) {
        let protected: HashSet<CommitId> = self
            .branches
            .values()
            .flat_map(|b| [b.tip, b.base])
            .collect();
        let mut children: HashMap<CommitId, usize> = HashMap::new();
        for c in self.commits.values() {
            for p in &c.parents {
                *children.entry(*p).or_default() += 1;
            }
        }
        let removable: Vec<CommitId> = self
            .commits
            .values()
            .filter(|c| {
                c.auto
                    && c.parents.len() == 1
                    && children.get(&c.id).copied().unwrap_or(0) <= 1
                    && !protected.contains(&c.id)
            })
            .map(|c| c.id)
            .collect();
        let excess = removable.len().saturating_sub(KEEP_AUTO);
        for id in removable.into_iter().take(excess) {
            let parent = self.commits[&id].parents[0];
            self.commits.remove(&id);
            for c in self.commits.values_mut() {
                for p in &mut c.parents {
                    if *p == id {
                        *p = parent;
                    }
                }
            }
        }
        self.gc();
    }

    /// Drop commits no branch can reach.
    fn gc(&mut self) {
        let mut live = HashSet::new();
        for b in self.branches.values() {
            live.extend(self.ancestors(b.tip));
            live.extend(self.ancestors(b.base));
        }
        self.commits.retain(|id, _| live.contains(id));
    }
}

// ── Compare ─────────────────────────────────────────────────────────────

/// One row of a comparison between two documents.
#[derive(Clone, Debug, PartialEq)]
pub struct DiffRow {
    pub label: String,
    pub a: String,
    pub b: String,
}

fn pct(v: f32) -> String {
    format!("{}%", (v * 100.0).round())
}

/// What differs between two documents, in the person's terms.
pub fn compare(a: &Document, b: &Document) -> Vec<DiffRow> {
    let mut rows = Vec::new();
    let mut row = |label: &str, x: String, y: String| {
        if x != y {
            rows.push(DiffRow {
                label: label.into(),
                a: x,
                b: y,
            });
        }
    };
    row(
        "canvas",
        format!("{}×{}", a.width, a.height),
        format!("{}×{}", b.width, b.height),
    );
    row(
        "nodes",
        a.nodes.len().to_string(),
        b.nodes.len().to_string(),
    );
    let ids: BTreeSet<NodeId> = a.nodes.iter().chain(&b.nodes).map(|n| n.id).collect();
    for id in ids {
        match (a.node(id), b.node(id)) {
            (Some(x), None) => row(&x.name, "present".into(), "deleted".into()),
            (None, Some(y)) => row(&y.name, "absent".into(), "added".into()),
            (Some(x), Some(y)) if x != y => {
                for (field, fx, fy) in node_fields(x, y) {
                    row(&format!("{} · {field}", y.name), fx, fy);
                }
            }
            _ => {}
        }
    }
    rows
}

/// Changed fields of a node, as (field, before, after).
fn node_fields(x: &Node, y: &Node) -> Vec<(&'static str, String, String)> {
    let mut out = Vec::new();
    if x.name != y.name {
        out.push(("name", x.name.clone(), y.name.clone()));
    }
    if x.visible != y.visible {
        let v = |b: bool| if b { "shown" } else { "hidden" }.to_string();
        out.push(("visibility", v(x.visible), v(y.visible)));
    }
    if x.opacity != y.opacity {
        out.push(("opacity", pct(x.opacity), pct(y.opacity)));
    }
    if x.blend != y.blend {
        out.push(("blend", format!("{:?}", x.blend), format!("{:?}", y.blend)));
    }
    if x.parent != y.parent {
        out.push(("group", "before".into(), "moved".into()));
    }
    let mask_same = match (&x.mask, &y.mask) {
        (None, None) => true,
        (Some(a), Some(b)) => std::sync::Arc::ptr_eq(a, b),
        _ => false,
    };
    if !mask_same || x.mask_enabled != y.mask_enabled {
        out.push(("mask", "before".into(), "edited".into()));
    }
    if x.clip_to != y.clip_to {
        out.push(("clipping", "before".into(), "changed".into()));
    }
    if x.locked != y.locked {
        out.push(("lock", x.locked.to_string(), y.locked.to_string()));
    }
    match (&x.kind, &y.kind) {
        (
            NodeKind::Raster {
                raster: a,
                placement: pa,
            },
            NodeKind::Raster {
                raster: b,
                placement: pb,
            },
        ) => {
            if !std::sync::Arc::ptr_eq(a, b) {
                out.push(("pixels", "before".into(), "edited".into()));
            }
            if pa != pb {
                out.push(("placement", "before".into(), "moved".into()));
            }
        }
        (NodeKind::Adjust(a), NodeKind::Adjust(b)) if a != b => {
            for (pa, pb) in a.params().iter().zip(b.params()) {
                if pa.value != pb.value {
                    out.push((
                        "parameter",
                        format!("{} {:.2}", pa.label, pa.value),
                        format!("{} {:.2}", pb.label, pb.value),
                    ));
                }
            }
            if out.is_empty() {
                out.push(("parameters", "before".into(), "changed".into()));
            }
        }
        (
            NodeKind::Path {
                path: a, style: sa, ..
            },
            NodeKind::Path {
                path: b, style: sb, ..
            },
        ) => {
            if a != b {
                out.push(("path", "before".into(), "edited".into()));
            }
            if sa != sb {
                out.push(("path style", "before".into(), "changed".into()));
            }
        }
        (a, b) if a != b => out.push(("content", "before".into(), "changed".into())),
        _ => {}
    }
    out
}

// ── Merge ───────────────────────────────────────────────────────────────

#[derive(Clone, Copy, Debug, PartialEq, Eq, Hash, PartialOrd, Ord)]
pub enum ConflictKey {
    Canvas,
    Node(NodeId),
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Side {
    Ours,
    Theirs,
}

/// Something both sides changed differently.
#[derive(Clone, Debug, PartialEq)]
pub struct Conflict {
    pub key: ConflictKey,
    /// What it is: a node name or "canvas".
    pub what: String,
    /// What this branch did, and what the other did.
    pub ours: String,
    pub theirs: String,
}

#[derive(Debug)]
pub enum MergeOutcome {
    Merged(Document),
    /// These need a choice before the merge can finish.
    Conflicts(Vec<Conflict>),
}

fn change_summary(base: Option<&Node>, now: Option<&Node>) -> String {
    match (base, now) {
        (Some(_), None) => "deleted it".into(),
        (None, Some(_)) => "added it".into(),
        (Some(b), Some(n)) => {
            let f: Vec<&str> = node_fields(b, n).into_iter().map(|(f, _, _)| f).collect();
            if f.is_empty() {
                "left it".into()
            } else {
                format!("changed {}", f.join(", "))
            }
        }
        (None, None) => "nothing".into(),
    }
}

/// Merge a node property by property. None when some property changed
/// differently on both sides.
fn merge_fields(b: &Node, o: &Node, t: &Node) -> Option<Node> {
    fn pick<T: Clone>(b: &T, o: &T, t: &T, eq: impl Fn(&T, &T) -> bool) -> Option<T> {
        if eq(o, b) {
            Some(t.clone())
        } else if eq(t, b) || eq(o, t) {
            Some(o.clone())
        } else {
            None
        }
    }
    let mask_eq = |x: &(Option<std::sync::Arc<emulsion_raster::Mask>>, bool),
                   y: &(Option<std::sync::Arc<emulsion_raster::Mask>>, bool)| {
        x.1 == y.1
            && match (&x.0, &y.0) {
                (None, None) => true,
                (Some(a), Some(b)) => std::sync::Arc::ptr_eq(a, b),
                _ => false,
            }
    };
    let (mask, mask_enabled) = pick(
        &(b.mask.clone(), b.mask_enabled),
        &(o.mask.clone(), o.mask_enabled),
        &(t.mask.clone(), t.mask_enabled),
        mask_eq,
    )?;
    Some(Node {
        id: o.id,
        name: pick(&b.name, &o.name, &t.name, |x, y| x == y)?,
        parent: pick(&b.parent, &o.parent, &t.parent, |x, y| x == y)?,
        visible: pick(&b.visible, &o.visible, &t.visible, |x, y| x == y)?,
        locked: pick(&b.locked, &o.locked, &t.locked, |x, y| x == y)?,
        opacity: pick(&b.opacity, &o.opacity, &t.opacity, |x, y| x == y)?,
        blend: pick(&b.blend, &o.blend, &t.blend, |x, y| x == y)?,
        clip_to: pick(&b.clip_to, &o.clip_to, &t.clip_to, |x, y| x == y)?,
        mask,
        mask_enabled,
        kind: pick(&b.kind, &o.kind, &t.kind, |x, y| x == y)?,
    })
}

/// Give nodes that `theirs` added fresh ids when `ours` added different
/// nodes under the same ids.
fn remap_collisions(base: &Document, ours: &Document, theirs: &Document) -> Document {
    let mut t = theirs.clone();
    let mut next = ours.next_id.max(theirs.next_id);
    let mut map = HashMap::new();
    for n in &theirs.nodes {
        if base.node(n.id).is_none() && ours.node(n.id).is_some() {
            map.insert(n.id, next);
            next += 1;
        }
    }
    if map.is_empty() {
        return t;
    }
    let fix = |id: &mut NodeId| {
        if let Some(n) = map.get(id) {
            *id = *n;
        }
    };
    for n in &mut t.nodes {
        fix(&mut n.id);
        if let Some(p) = &mut n.parent {
            fix(p);
        }
        if let Some(c) = &mut n.clip_to {
            fix(c);
        }
    }
    t.next_id = next;
    t
}

/// Three-way merge of node stacks. `choices` resolves conflicts; any
/// conflict without a choice is returned instead of a document.
pub fn merge(
    base: &Document,
    ours: &Document,
    theirs: &Document,
    choices: &HashMap<ConflictKey, Side>,
) -> Result<MergeOutcome, GraphError> {
    let theirs = remap_collisions(base, ours, theirs);
    let mut conflicts = Vec::new();
    let mut out = ours.clone();

    // Canvas: size, resolution and blend space move together.
    let canvas = |d: &Document| (d.width, d.height, d.resolution.to_bits(), d.blend_space);
    let (cb, co, ct) = (canvas(base), canvas(ours), canvas(&theirs));
    if co == cb && ct != cb {
        (out.width, out.height, out.resolution, out.blend_space) = (
            theirs.width,
            theirs.height,
            theirs.resolution,
            theirs.blend_space,
        );
    } else if co != cb && ct != cb && co != ct {
        match choices.get(&ConflictKey::Canvas) {
            Some(Side::Ours) => {}
            Some(Side::Theirs) => {
                (out.width, out.height, out.resolution, out.blend_space) = (
                    theirs.width,
                    theirs.height,
                    theirs.resolution,
                    theirs.blend_space,
                )
            }
            None => conflicts.push(Conflict {
                key: ConflictKey::Canvas,
                what: "canvas".into(),
                ours: format!("{}×{}", ours.width, ours.height),
                theirs: format!("{}×{}", theirs.width, theirs.height),
            }),
        }
    }
    // Selection is transient: keep ours unless only theirs changed it.
    let sel_eq = |a: &Document, b: &Document| match (&a.selection, &b.selection) {
        (None, None) => true,
        (Some(x), Some(y)) => std::sync::Arc::ptr_eq(x, y),
        _ => false,
    };
    if sel_eq(ours, base) && !sel_eq(&theirs, base) {
        out.selection = theirs.selection.clone();
    }
    // Guides: like the selection, a helper rather than content.
    if ours.guides == base.guides && theirs.guides != base.guides {
        out.guides = theirs.guides.clone();
    }

    // Nodes.
    let ids: BTreeSet<NodeId> = base
        .nodes
        .iter()
        .chain(&ours.nodes)
        .chain(&theirs.nodes)
        .map(|n| n.id)
        .collect();
    let mut result: HashMap<NodeId, Node> = HashMap::new();
    for id in ids {
        let (b, o, t) = (base.node(id), ours.node(id), theirs.node(id));
        let fielded = match (b, o, t) {
            (Some(b), Some(o), Some(t)) => merge_fields(b, o, t),
            _ => None,
        };
        let pick = if o == b {
            t
        } else if t == b || o == t {
            o
        } else if let Some(n) = &fielded {
            Some(n)
        } else {
            let key = ConflictKey::Node(id);
            match choices.get(&key) {
                Some(Side::Ours) => o,
                Some(Side::Theirs) => t,
                None => {
                    conflicts.push(Conflict {
                        key,
                        what: o.or(t).or(b).map(|n| n.name.clone()).unwrap_or_default(),
                        ours: change_summary(b, o),
                        theirs: change_summary(b, t),
                    });
                    o
                }
            }
        };
        if let Some(n) = pick {
            result.insert(id, n.clone());
        }
    }
    if !conflicts.is_empty() {
        return Ok(MergeOutcome::Conflicts(conflicts));
    }

    // Order: take the side that reordered shared nodes (ours wins if both
    // did), then slot in nodes only the other side has above the node they
    // sat on there.
    let order_of = |d: &Document| -> Vec<NodeId> {
        d.nodes
            .iter()
            .map(|n| n.id)
            .filter(|id| {
                base.node(*id).is_some() && ours.node(*id).is_some() && theirs.node(*id).is_some()
            })
            .collect()
    };
    let (primary, secondary) =
        if order_of(ours) == order_of(base) && order_of(&theirs) != order_of(base) {
            (&theirs, ours)
        } else {
            (ours, &theirs)
        };
    let mut order: Vec<NodeId> = primary
        .nodes
        .iter()
        .map(|n| n.id)
        .filter(|id| result.contains_key(id))
        .collect();
    for (i, n) in secondary.nodes.iter().enumerate() {
        if !result.contains_key(&n.id) || order.contains(&n.id) {
            continue;
        }
        let below = secondary.nodes[..i]
            .iter()
            .rev()
            .find_map(|m| order.iter().position(|x| *x == m.id));
        order.insert(below.map_or(0, |p| p + 1), n.id);
    }
    out.nodes = order
        .into_iter()
        .map(|id| result.remove(&id).expect("picked"))
        .collect();

    // References to nodes that no longer exist fall back to safe values.
    let present: HashSet<NodeId> = out.nodes.iter().map(|n| n.id).collect();
    let groups: HashSet<NodeId> = out
        .nodes
        .iter()
        .filter(|n| n.is_group())
        .map(|n| n.id)
        .collect();
    for n in &mut out.nodes {
        if n.parent.is_some_and(|p| !groups.contains(&p)) {
            n.parent = None;
        }
        if n.clip_to.is_some_and(|c| !present.contains(&c)) {
            n.clip_to = None;
        }
    }
    out.next_id = [base.next_id, ours.next_id, theirs.next_id]
        .into_iter()
        .chain(out.nodes.iter().map(|n| n.id + 1))
        .max()
        .unwrap_or(1);
    out.normalize();
    // A clip target must be a sibling below; drop any that normalizing broke.
    let snapshot = out.clone();
    for n in &mut out.nodes {
        if let Some(c) = n.clip_to {
            let (Some(i), Some(j)) = (snapshot.index_of(n.id), snapshot.index_of(c)) else {
                n.clip_to = None;
                continue;
            };
            if j >= i || snapshot.nodes[j].parent != n.parent {
                n.clip_to = None;
            }
        }
    }
    out.validate()?;
    Ok(MergeOutcome::Merged(out))
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::command::{Command, Slot};
    use emulsion_raster::{Placement, Raster};
    use std::sync::Arc;

    fn doc() -> Document {
        let mut d = Document::new(64, 64);
        for name in ["bg", "sky", "sun"] {
            Command::AddNode {
                node: Box::new(Node::raster(
                    0,
                    name,
                    Arc::new(Raster::solid(64, 64, [0.5, 0.5, 0.5, 1.0])),
                    Placement::default(),
                )),
                slot: Slot::TOP,
            }
            .apply(&mut d)
            .unwrap();
        }
        d
    }

    fn apply(d: &Document, c: Command) -> Document {
        let mut d = d.clone();
        c.apply(&mut d).unwrap();
        d
    }

    fn merged(o: MergeOutcome) -> Document {
        match o {
            MergeOutcome::Merged(d) => d,
            MergeOutcome::Conflicts(c) => panic!("unexpected conflicts {c:?}"),
        }
    }

    #[test]
    fn disjoint_changes_merge_cleanly() {
        let base = doc();
        let ours = apply(
            &base,
            Command::SetOpacity {
                id: 1,
                opacity: 0.5,
            },
        );
        let theirs = apply(
            &base,
            Command::SetVisible {
                id: 2,
                visible: false,
            },
        );
        let m = merged(merge(&base, &ours, &theirs, &HashMap::new()).unwrap());
        assert_eq!(m.node(1).unwrap().opacity, 0.5);
        assert!(!m.node(2).unwrap().visible);
    }

    #[test]
    fn different_properties_of_one_node_merge_cleanly() {
        let base = doc();
        let ours = apply(
            &base,
            Command::Rename {
                id: 2,
                name: "clouds".into(),
            },
        );
        let theirs = apply(
            &base,
            Command::SetOpacity {
                id: 2,
                opacity: 0.4,
            },
        );
        let m = merged(merge(&base, &ours, &theirs, &HashMap::new()).unwrap());
        let n = m.node(2).unwrap();
        assert!(n.name == "clouds" && n.opacity == 0.4);
    }

    #[test]
    fn same_node_both_sides_asks_then_obeys() {
        let base = doc();
        let ours = apply(
            &base,
            Command::SetOpacity {
                id: 2,
                opacity: 0.5,
            },
        );
        let theirs = apply(
            &base,
            Command::SetOpacity {
                id: 2,
                opacity: 0.2,
            },
        );
        let MergeOutcome::Conflicts(c) = merge(&base, &ours, &theirs, &HashMap::new()).unwrap()
        else {
            panic!("expected a conflict");
        };
        assert_eq!(c.len(), 1);
        assert_eq!(c[0].what, "sky");
        assert_eq!(c[0].ours, "changed opacity");
        let pick = HashMap::from([(c[0].key, Side::Theirs)]);
        let m = merged(merge(&base, &ours, &theirs, &pick).unwrap());
        assert_eq!(m.node(2).unwrap().opacity, 0.2);
    }

    #[test]
    fn both_sides_add_nodes_with_the_same_id() {
        let base = doc();
        let add = |name: &str| Command::AddNode {
            node: Box::new(Node::new(
                0,
                name,
                NodeKind::Fill {
                    rgba: [1, 2, 3, 255],
                },
            )),
            slot: Slot::TOP,
        };
        let ours = apply(&base, add("ours"));
        let theirs = apply(&base, add("theirs"));
        assert_eq!(
            ours.nodes.last().unwrap().id,
            theirs.nodes.last().unwrap().id
        );
        let m = merged(merge(&base, &ours, &theirs, &HashMap::new()).unwrap());
        let names: Vec<&str> = m.nodes.iter().map(|n| n.name.as_str()).collect();
        // This branch's new node stays on top; the other slots in above the node it sat on.
        assert_eq!(names, ["bg", "sky", "sun", "theirs", "ours"]);
        assert!(m.next_id > m.nodes.iter().map(|n| n.id).max().unwrap());
    }

    #[test]
    fn delete_versus_edit_is_a_conflict_and_delete_versus_nothing_is_not() {
        let base = doc();
        let ours = apply(&base, Command::RemoveNode { id: 3 });
        let theirs = apply(
            &base,
            Command::Rename {
                id: 3,
                name: "moon".into(),
            },
        );
        let MergeOutcome::Conflicts(c) = merge(&base, &ours, &theirs, &HashMap::new()).unwrap()
        else {
            panic!("expected a conflict");
        };
        assert_eq!(c[0].ours, "deleted it");
        let theirs = apply(
            &base,
            Command::SetVisible {
                id: 1,
                visible: false,
            },
        );
        let m = merged(merge(&base, &ours, &theirs, &HashMap::new()).unwrap());
        assert!(m.node(3).is_none() && !m.node(1).unwrap().visible);
    }

    #[test]
    fn reorder_on_one_side_is_kept() {
        let base = doc();
        let theirs = apply(
            &base,
            Command::MoveNode {
                id: 3,
                slot: Slot {
                    parent: None,
                    index: 0,
                },
            },
        );
        let ours = apply(
            &base,
            Command::SetOpacity {
                id: 1,
                opacity: 0.3,
            },
        );
        let m = merged(merge(&base, &ours, &theirs, &HashMap::new()).unwrap());
        assert_eq!(m.nodes[0].id, 3);
        assert_eq!(m.node(1).unwrap().opacity, 0.3);
    }

    #[test]
    fn graph_branches_merge_base_and_pruning() {
        let d = doc();
        let mut g = Graph::new(d.clone(), "Opened");
        let d1 = apply(
            &d,
            Command::SetOpacity {
                id: 1,
                opacity: 0.9,
            },
        );
        let c1 = g.record(&d1, "Edit", false).unwrap();
        assert!(
            g.record(&d1, "Same", false).is_none(),
            "unchanged is not a commit"
        );
        g.create_branch("retouch", c1).unwrap();
        assert_eq!(
            g.create_branch("retouch", c1),
            Err(GraphError::BranchExists("retouch".into()))
        );
        assert_eq!(g.create_branch(" x", c1), Err(GraphError::BadName));
        g.set_head("retouch").unwrap();
        let d2 = apply(
            &d1,
            Command::SetOpacity {
                id: 2,
                opacity: 0.4,
            },
        );
        let c2 = g.record(&d2, "Retouch", false).unwrap();
        assert_eq!(g.merge_base(c2, g.branch(MAIN).unwrap().tip), Some(c1));
        assert_eq!(g.ahead("retouch", MAIN), 1);
        // Autosaves beyond the cap fold away but tips survive.
        let mut cur = d2;
        for i in 0..(KEEP_AUTO + 10) {
            cur = apply(
                &cur,
                Command::SetOpacity {
                    id: 3,
                    opacity: 0.01 * (i + 1) as f32,
                },
            );
            g.record(&cur, "Autosave", true);
        }
        // The kept autosaves plus the tip, which is never folded.
        assert_eq!(g.commits().filter(|c| c.auto).count(), KEEP_AUTO + 1);
        assert_eq!(g.commit(g.head_branch().tip).unwrap().doc, cur);
        assert_eq!(
            g.merge_base(g.head_branch().tip, g.branch(MAIN).unwrap().tip),
            Some(c1)
        );
        g.set_head(MAIN).unwrap();
        g.delete_branch("retouch").unwrap();
        assert!(g.commit(c2).is_none(), "unreachable commits are dropped");
    }
}
