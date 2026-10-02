//! The history graph: named commits on branches, and three-way merge.
//!
//! Steps (see [`crate::history`]) are the fine-grained undo stack of one
//! branch. Commits are the coarse points worth keeping: the opened file,
//! explicit named versions, and branch or merge checkpoints. Recovery saves
//! preserve the working document separately without recording new commits.
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
    pub(crate) fn remap_pages(&mut self, pages: &BTreeMap<u64, u64>) {
        for commit in self.commits.values_mut() {
            commit.doc.design.remap_pages(pages);
        }
    }
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
        "global light",
        format!("{:?}", a.global_light),
        format!("{:?}", b.global_light),
    );
    row(
        "design",
        format!("{:?}", a.design),
        format!("{:?}", b.design),
    );
    row(
        "diagram",
        format!("{:?}", a.diagram),
        format!("{:?}", b.diagram),
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
    if x.blending != y.blending {
        out.push((
            "blending options",
            format!("{:?}", x.blending),
            format!("{:?}", y.blending),
        ));
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
    if !mask_same
        || x.mask_enabled != y.mask_enabled
        || x.mask_linked != y.mask_linked
        || x.mask_transform != y.mask_transform
    {
        out.push(("mask", "before".into(), "edited".into()));
    }
    if x.styles != y.styles
        || x.style_options != y.style_options
        || x.effects_enabled != y.effects_enabled
    {
        out.push(("styles", "before".into(), "changed".into()));
    }
    if x.clip_to != y.clip_to {
        out.push(("clipping", "before".into(), "changed".into()));
    }
    if x.locks != y.locks {
        out.push((
            "layer locks",
            format!("{:?}", x.locks),
            format!("{:?}", y.locks),
        ));
    }
    if x.link_group != y.link_group {
        out.push((
            "linked layers",
            format!("{:?}", x.link_group),
            format!("{:?}", y.link_group),
        ));
    }
    if x.color_label != y.color_label {
        out.push((
            "layer color",
            x.color_label.label().into(),
            y.color_label.label().into(),
        ));
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
        (NodeKind::Text { spec: a, .. }, NodeKind::Text { spec: b, .. }) => {
            if a.text != b.text {
                out.push(("text", a.label(), b.label()));
            } else if a != b {
                out.push(("text style", "before".into(), "changed".into()));
            }
        }
        (NodeKind::Strokes { strokes: a, .. }, NodeKind::Strokes { strokes: b, .. }) => {
            if a != b {
                out.push(("drawing", "before".into(), "edited".into()));
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
        (
            NodeKind::Smart {
                source: a,
                filters: fa,
                placement: pa,
                ..
            },
            NodeKind::Smart {
                source: b,
                filters: fb,
                placement: pb,
                ..
            },
        ) => {
            if !std::sync::Arc::ptr_eq(a, b) {
                out.push(("pixels", "before".into(), "edited".into()));
            }
            if fa != fb {
                out.push(("filters", "before".into(), "changed".into()));
            }
            if pa != pb {
                out.push(("placement", "before".into(), "moved".into()));
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
    Diagram,
    Design,
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
    Merged(Box<Document>),
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
    let (styles, style_options) = pick(
        &(b.styles.clone(), b.style_options.clone()),
        &(o.styles.clone(), o.style_options.clone()),
        &(t.styles.clone(), t.style_options.clone()),
        |x, y| x == y,
    )?;
    Some(Node {
        id: o.id,
        name: pick(&b.name, &o.name, &t.name, |x, y| x == y)?,
        parent: pick(&b.parent, &o.parent, &t.parent, |x, y| x == y)?,
        visible: pick(&b.visible, &o.visible, &t.visible, |x, y| x == y)?,
        locked: pick(&b.locked, &o.locked, &t.locked, |x, y| x == y)?,
        locks: pick(&b.locks, &o.locks, &t.locks, |x, y| x == y)?,
        color_label: pick(&b.color_label, &o.color_label, &t.color_label, |x, y| {
            x == y
        })?,
        link_group: pick(&b.link_group, &o.link_group, &t.link_group, |x, y| x == y)?,
        mask_linked: pick(&b.mask_linked, &o.mask_linked, &t.mask_linked, |x, y| {
            x == y
        })?,
        mask_transform: pick(
            &b.mask_transform,
            &o.mask_transform,
            &t.mask_transform,
            |x, y| x == y,
        )?,
        opacity: pick(&b.opacity, &o.opacity, &t.opacity, |x, y| x == y)?,
        blending: pick(&b.blending, &o.blending, &t.blending, |x, y| x == y)?,
        blend: pick(&b.blend, &o.blend, &t.blend, |x, y| x == y)?,
        clip_to: pick(&b.clip_to, &o.clip_to, &t.clip_to, |x, y| x == y)?,
        mask,
        mask_enabled,
        styles,
        style_options,
        effects_enabled: pick(
            &b.effects_enabled,
            &o.effects_enabled,
            &t.effects_enabled,
            |x, y| x == y,
        )?,
        origin: pick(&b.origin, &o.origin, &t.origin, |x, y| x == y)?,
        review: pick(&b.review, &o.review, &t.review, |x, y| x == y)?,
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
    // Link tokens have their own namespace. Independently created link sets
    // on two branches must not become one set merely because tokens coincide.
    let base_links: std::collections::HashSet<_> =
        base.nodes.iter().filter_map(|n| n.link_group).collect();
    let mut used: std::collections::HashSet<_> = ours
        .nodes
        .iter()
        .chain(theirs.nodes.iter())
        .filter_map(|n| n.link_group)
        .collect();
    let mut link_map = HashMap::new();
    for token in theirs.nodes.iter().filter_map(|n| n.link_group) {
        if base_links.contains(&token) || link_map.contains_key(&token) {
            continue;
        }
        let ours_members: std::collections::HashSet<_> = ours
            .nodes
            .iter()
            .filter(|n| n.link_group == Some(token))
            .map(|n| n.id)
            .collect();
        let theirs_members: std::collections::HashSet<_> = theirs
            .nodes
            .iter()
            .filter(|n| n.link_group == Some(token))
            .map(|n| map.get(&n.id).copied().unwrap_or(n.id))
            .collect();
        if !ours_members.is_empty() && ours_members != theirs_members {
            let fresh = (0..)
                .find(|id| !used.contains(id))
                .expect("available link token");
            used.insert(fresh);
            link_map.insert(token, fresh);
        }
    }
    let fix = |id: &mut NodeId| {
        if let Some(n) = map.get(id) {
            *id = *n;
        }
    };
    if let Some(raw) = &mut t.raw {
        fix(&mut raw.node_id);
    }
    for n in &mut t.nodes {
        fix(&mut n.id);
        if let Some(p) = &mut n.parent {
            fix(p);
        }
        if let Some(c) = &mut n.clip_to {
            fix(c);
        }
        if let Some(group) = &mut n.link_group
            && let Some(fresh) = link_map.get(group)
        {
            *group = *fresh;
        }
    }
    t.diagram = t
        .diagram
        .as_ref()
        .map(|d| std::sync::Arc::new(d.remap(&map)));
    t.design = t.design.remap(&map);
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
    for doc in [base, &theirs] {
        for path in &doc.raw_originals {
            if !out.raw_originals.contains(path) {
                out.raw_originals.push(path.clone());
            }
        }
    }

    // Canvas: size, resolution and blend space move together.
    let canvas = |d: &Document| {
        (
            d.width,
            d.height,
            d.resolution.to_bits(),
            d.blend_space,
            d.global_light,
        )
    };
    let (cb, co, ct) = (canvas(base), canvas(ours), canvas(&theirs));
    if co == cb && ct != cb {
        (
            out.width,
            out.height,
            out.resolution,
            out.blend_space,
            out.global_light,
        ) = (
            theirs.width,
            theirs.height,
            theirs.resolution,
            theirs.blend_space,
            theirs.global_light,
        );
    } else if co != cb && ct != cb && co != ct {
        match choices.get(&ConflictKey::Canvas) {
            Some(Side::Ours) => {}
            Some(Side::Theirs) => {
                (
                    out.width,
                    out.height,
                    out.resolution,
                    out.blend_space,
                    out.global_light,
                ) = (
                    theirs.width,
                    theirs.height,
                    theirs.resolution,
                    theirs.blend_space,
                    theirs.global_light,
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

    // Merge independent diagram additions and edits by object identity.
    fn merge_metadata<K: Clone + Ord, T: Clone + PartialEq>(
        base: &std::collections::BTreeMap<K, T>,
        ours: &std::collections::BTreeMap<K, T>,
        theirs: &std::collections::BTreeMap<K, T>,
        side: Option<&Side>,
        conflict: &mut bool,
    ) -> std::collections::BTreeMap<K, T> {
        base.keys()
            .chain(ours.keys())
            .chain(theirs.keys())
            .cloned()
            .collect::<BTreeSet<_>>()
            .into_iter()
            .filter_map(|id| {
                let (b, o, t) = (base.get(&id), ours.get(&id), theirs.get(&id));
                let pick = if o == b {
                    t
                } else if t == b || o == t {
                    o
                } else {
                    *conflict = true;
                    if side == Some(&Side::Theirs) { t } else { o }
                };
                pick.cloned().map(|v| (id, v))
            })
            .collect()
    }
    if base.diagram.is_some() || ours.diagram.is_some() || theirs.diagram.is_some() {
        let empty = crate::diagram::Diagram::default();
        let (b, o, t) = (
            base.diagram.as_deref().unwrap_or(&empty),
            ours.diagram.as_deref().unwrap_or(&empty),
            theirs.diagram.as_deref().unwrap_or(&empty),
        );
        let mut conflict = false;
        let side = choices.get(&ConflictKey::Diagram);
        let shapes = merge_metadata(&b.shapes, &o.shapes, &t.shapes, side, &mut conflict);
        let edges = merge_metadata(&b.edges, &o.edges, &t.edges, side, &mut conflict);
        let settings = merge_metadata(
            &std::collections::BTreeMap::from([(0, b.settings.clone())]),
            &std::collections::BTreeMap::from([(0, o.settings.clone())]),
            &std::collections::BTreeMap::from([(0, t.settings.clone())]),
            side,
            &mut conflict,
        )
        .remove(&0)
        .unwrap_or_default();
        if conflict && side.is_none() {
            conflicts.push(Conflict {
                key: ConflictKey::Diagram,
                what: "Diagram connections and shape data".into(),
                ours: "Our diagram properties".into(),
                theirs: "Their diagram properties".into(),
            });
        }
        out.diagram = Some(std::sync::Arc::new(crate::diagram::Diagram {
            shapes,
            edges,
            settings,
        }));
    }

    let mut design_conflict = false;
    let side = choices.get(&ConflictKey::Design);
    out.design.data_bindings = merge_metadata(
        &base.design.data_bindings,
        &ours.design.data_bindings,
        &theirs.design.data_bindings,
        side,
        &mut design_conflict,
    );
    out.design.variable_libraries = merge_metadata(
        &base.design.variable_libraries,
        &ours.design.variable_libraries,
        &theirs.design.variable_libraries,
        side,
        &mut design_conflict,
    );
    out.design.variables = merge_metadata(
        &base.design.variables,
        &ours.design.variables,
        &theirs.design.variables,
        side,
        &mut design_conflict,
    );
    out.design.variable_bindings = merge_metadata(
        &base.design.variable_bindings,
        &ours.design.variable_bindings,
        &theirs.design.variable_bindings,
        side,
        &mut design_conflict,
    );
    out.design.interaction_triggers = merge_metadata(
        &base.design.interaction_triggers,
        &ours.design.interaction_triggers,
        &theirs.design.interaction_triggers,
        side,
        &mut design_conflict,
    );
    out.design.interactions = merge_metadata(
        &base.design.interactions,
        &ours.design.interactions,
        &theirs.design.interactions,
        side,
        &mut design_conflict,
    );
    out.design.fonts = merge_metadata(
        &base.design.fonts,
        &ours.design.fonts,
        &theirs.design.fonts,
        side,
        &mut design_conflict,
    );
    out.design.local_media = merge_metadata(
        &base.design.local_media,
        &ours.design.local_media,
        &theirs.design.local_media,
        side,
        &mut design_conflict,
    );
    out.design.keyframes = merge_metadata(
        &base.design.keyframes,
        &ours.design.keyframes,
        &theirs.design.keyframes,
        side,
        &mut design_conflict,
    );
    let overlay_map = |d: &Document| d.design.overlays.iter().map(|id| (*id, ())).collect();
    out.design.overlays = merge_metadata(
        &overlay_map(base),
        &overlay_map(ours),
        &overlay_map(&theirs),
        side,
        &mut design_conflict,
    )
    .into_keys()
    .collect();
    out.design.saved_styles = merge_metadata(
        &base.design.saved_styles,
        &ours.design.saved_styles,
        &theirs.design.saved_styles,
        side,
        &mut design_conflict,
    );
    out.design.style_links = merge_metadata(
        &base.design.style_links,
        &ours.design.style_links,
        &theirs.design.style_links,
        side,
        &mut design_conflict,
    );
    out.design.components = merge_metadata(
        &base.design.components,
        &ours.design.components,
        &theirs.design.components,
        side,
        &mut design_conflict,
    );
    out.design.component_links = merge_metadata(
        &base.design.component_links,
        &ours.design.component_links,
        &theirs.design.component_links,
        side,
        &mut design_conflict,
    );
    out.design.media = merge_metadata(
        &base.design.media,
        &ours.design.media,
        &theirs.design.media,
        side,
        &mut design_conflict,
    );
    out.design.charts = merge_metadata(
        &base.design.charts,
        &ours.design.charts,
        &theirs.design.charts,
        side,
        &mut design_conflict,
    );
    out.design.frames = merge_metadata(
        &base.design.frames,
        &ours.design.frames,
        &theirs.design.frames,
        side,
        &mut design_conflict,
    );
    out.design.constraints = merge_metadata(
        &base.design.constraints,
        &ours.design.constraints,
        &theirs.design.constraints,
        side,
        &mut design_conflict,
    );
    out.design.motion = merge_metadata(
        &base.design.motion,
        &ours.design.motion,
        &theirs.design.motion,
        side,
        &mut design_conflict,
    );
    if ours.design.page_background == base.design.page_background {
        out.design.page_background = theirs.design.page_background;
    } else if theirs.design.page_background != base.design.page_background
        && theirs.design.page_background != ours.design.page_background
    {
        design_conflict = true;
        if side == Some(&Side::Theirs) {
            out.design.page_background = theirs.design.page_background;
        }
    }
    if ours.design.precision == base.design.precision {
        out.design.precision = theirs.design.precision;
    } else if theirs.design.precision != base.design.precision
        && theirs.design.precision != ours.design.precision
    {
        design_conflict = true;
        if side == Some(&Side::Theirs) {
            out.design.precision = theirs.design.precision;
        }
    }
    let presentation = |d: &Document| {
        (
            d.design.speaker_notes.clone(),
            d.design.page_transition,
            d.design.transition_ms,
        )
    };
    if presentation(ours) == presentation(base)
        || (presentation(&theirs) != presentation(base)
            && presentation(&theirs) != presentation(ours)
            && side == Some(&Side::Theirs))
    {
        out.design.speaker_notes = theirs.design.speaker_notes.clone();
        out.design.page_transition = theirs.design.page_transition;
        out.design.transition_ms = theirs.design.transition_ms;
    }
    if presentation(ours) != presentation(base)
        && presentation(&theirs) != presentation(base)
        && presentation(&theirs) != presentation(ours)
    {
        design_conflict = true;
    }
    let timing = |d: &Document| (d.design.duration_ms, d.design.fps);
    if timing(ours) == timing(base) {
        out.design.duration_ms = theirs.design.duration_ms;
        out.design.fps = theirs.design.fps;
    } else if timing(&theirs) != timing(base) && timing(&theirs) != timing(ours) {
        design_conflict = true;
        if side == Some(&Side::Theirs) {
            out.design.duration_ms = theirs.design.duration_ms;
            out.design.fps = theirs.design.fps;
        }
    }
    if design_conflict && side.is_none() {
        conflicts.push(Conflict {
            key: ConflictKey::Design,
            what: "Resize constraints and animation".into(),
            ours: "Our design settings".into(),
            theirs: "Their design settings".into(),
        });
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
    // A library deletion/rename and a new consumer on the other branch are
    // disjoint key edits, but their references must be merged together. Keep
    // only definitions still needed by surviving consumers; unused deletions
    // remain deletions. Component definitions also own hidden native sources.
    let libraries = if side == Some(&Side::Ours) {
        [ours, &theirs, base]
    } else {
        [&theirs, ours, base]
    };
    let mut required_components: std::collections::BTreeMap<String, BTreeSet<String>> =
        Default::default();
    for (id, link) in &out.design.component_links {
        if result.contains_key(id) {
            required_components
                .entry(link.component.clone())
                .or_default()
                .insert(link.variant.clone());
        }
    }
    for (name, variants) in required_components {
        if !out.design.components.contains_key(&name)
            && let Some(definition) = libraries
                .iter()
                .find_map(|doc| doc.design.components.get(&name))
        {
            out.design
                .components
                .insert(name.clone(), definition.clone());
        }
        if let Some(definition) = out.design.components.get_mut(&name) {
            for variant in variants {
                if !definition.variants.contains_key(&variant)
                    && let Some(root) = libraries
                        .iter()
                        .find_map(|doc| doc.design.components.get(&name)?.variants.get(&variant))
                {
                    definition.variants.insert(variant, *root);
                }
            }
        }
        let Some(definition) = out.design.components.get(&name).cloned() else {
            continue;
        };
        for root in definition.variants.values() {
            if result.contains_key(root) {
                continue;
            }
            let Some(source) = libraries.iter().find(|doc| doc.node(*root).is_some()) else {
                continue;
            };
            let ids: HashSet<_> = source.subtree(*root).into_iter().collect();
            for node in source.nodes.iter().filter(|node| ids.contains(&node.id)) {
                result.entry(node.id).or_insert_with(|| node.clone());
            }
            let settings = source.design.fragment(&ids);
            for (key, value) in &settings.data_bindings {
                out.design
                    .data_bindings
                    .entry(*key)
                    .or_insert_with(|| value.clone());
            }
            for (key, value) in settings.variable_libraries {
                out.design.variable_libraries.entry(key).or_insert(value);
            }
            for (key, value) in settings.variables {
                out.design.variables.entry(key).or_insert(value);
            }
            for (key, value) in settings.variable_bindings {
                out.design.variable_bindings.entry(key).or_insert(value);
            }
            for (key, value) in settings.interaction_triggers {
                out.design.interaction_triggers.entry(key).or_insert(value);
            }
            for (key, value) in settings.interactions {
                out.design.interactions.entry(key).or_insert(value);
            }
            for (key, value) in settings.fonts {
                out.design.fonts.entry(key).or_insert(value);
            }
            for (key, value) in settings.local_media {
                out.design.local_media.entry(key).or_insert(value);
            }
            for (key, value) in settings.keyframes {
                for track in &value {
                    if let Some(frame) = track.frames.last() {
                        out.design.duration_ms = out.design.duration_ms.max(frame.time_ms);
                    }
                }
                out.design.keyframes.entry(key).or_insert(value);
            }
            out.design.overlays.extend(settings.overlays);
            for (key, value) in settings.saved_styles {
                out.design.saved_styles.entry(key).or_insert(value);
            }
            for (key, value) in settings.style_links {
                out.design.style_links.entry(key).or_insert(value);
            }
            for (key, value) in settings.media {
                out.design.media.entry(key).or_insert(value);
            }
            for (key, value) in settings.charts {
                out.design.charts.entry(key).or_insert(value);
            }
            for (key, value) in settings.frames {
                out.design.frames.entry(key).or_insert(value);
            }
            for (key, value) in settings.constraints {
                out.design.constraints.entry(key).or_insert(value);
            }
            for (key, value) in settings.motion {
                out.design.duration_ms = out.design.duration_ms.max(value.end_ms);
                out.design.motion.entry(key).or_insert(value);
            }
            if let Some(diagram) = &source.diagram {
                let settings = diagram.fragment(&ids);
                let diagram =
                    std::sync::Arc::make_mut(out.diagram.get_or_insert_with(Default::default));
                for (key, value) in settings.shapes {
                    diagram.shapes.entry(key).or_insert(value);
                }
                for (key, value) in settings.edges {
                    diagram.edges.entry(key).or_insert(value);
                }
            }
        }
    }
    let required_styles: BTreeSet<_> = out
        .design
        .style_links
        .iter()
        .filter(|(id, _)| result.contains_key(id))
        .map(|(_, name)| name.clone())
        .collect();
    for name in required_styles {
        if !out.design.saved_styles.contains_key(&name)
            && let Some(style) = libraries
                .iter()
                .find_map(|doc| doc.design.saved_styles.get(&name))
        {
            out.design.saved_styles.insert(name, style.clone());
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

    // Development settings belong to the exact source pixels they produced.
    // A node merge can choose another branch's pixels or bake the source;
    // never retain a recipe from a different rendering in either case.
    fn raw_pixels(doc: &Document, id: NodeId) -> Option<&std::sync::Arc<emulsion_raster::Raster>> {
        match &doc.node(id)?.kind {
            NodeKind::Raster { raster, .. } => Some(raster),
            NodeKind::Smart {
                source,
                editable: None,
                ..
            } => Some(source),
            _ => None,
        }
    }
    let matching_recipe = |candidate: &Document| {
        let raw = candidate.raw.as_ref()?;
        let source = raw_pixels(candidate, raw.node_id)?;
        let merged = raw_pixels(&out, raw.node_id)?;
        std::sync::Arc::ptr_eq(source, merged).then(|| raw.clone())
    };
    out.raw = if ours.raw == base.raw && theirs.raw != base.raw {
        matching_recipe(&theirs).or_else(|| matching_recipe(ours))
    } else {
        matching_recipe(ours).or_else(|| matching_recipe(&theirs))
    };

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
    crate::diagram::synchronize(&out.clone(), &mut out)
        .map_err(crate::DocumentError::BadDiagram)?;
    out.design
        .retain_nodes(&out.nodes.iter().map(|n| n.id).collect());
    out.normalize();
    crate::design_background::pin(&mut out);
    out.validate()?;
    Ok(MergeOutcome::Merged(Box::new(out)))
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
            MergeOutcome::Merged(d) => *d,
            MergeOutcome::Conflicts(c) => panic!("unexpected conflicts {c:?}"),
        }
    }

    #[test]
    fn style_rename_or_delete_keeps_other_branch_new_consumers_valid() {
        use crate::{Editor, design_styles};
        for rename in [false, true] {
            let mut source = Editor::new(doc(), None);
            let id = source.doc.nodes[0].id;
            design_styles::create(&mut source, id, "Shared").unwrap();
            let base = source.doc.clone();
            let saved = base.design.saved_styles["Shared"].clone();
            let mut ours = Editor::new(base.clone(), None);
            if rename {
                design_styles::rename(&mut ours, "Shared", "Renamed").unwrap();
            } else {
                design_styles::remove(&mut ours, "Shared").unwrap();
            }
            let mut theirs = Editor::new(base.clone(), None);
            let added = theirs
                .execute(Command::DuplicateNode { id })
                .unwrap()
                .unwrap();
            for (left, right) in [(&ours.doc, &theirs.doc), (&theirs.doc, &ours.doc)] {
                let result = merged(merge(&base, left, right, &HashMap::new()).unwrap());
                assert_eq!(result.design.style_links[&added], "Shared");
                assert_eq!(result.design.saved_styles["Shared"], saved);
                assert_eq!(result.design.saved_styles.contains_key("Renamed"), rename);
                result.validate().unwrap();
            }
        }
    }

    #[test]
    fn component_source_deletion_keeps_other_branch_new_instance_editable() {
        use crate::{Editor, design_components as components};
        let mut source = Editor::new(doc(), None);
        let id = source.doc.nodes[0].id;
        components::create(&mut source, &[id], "Reusable").unwrap();
        let base = source.doc.clone();
        let mut ours = Editor::new(base.clone(), None);
        for root in components::source_roots(&base.design) {
            ours.execute(Command::RemoveNode { id: root }).unwrap();
        }
        let mut theirs = Editor::new(base.clone(), None);
        let inserted = components::insert(&mut theirs, "Reusable", "Default", (10., 0.)).unwrap();
        for (left, right) in [(&ours.doc, &theirs.doc), (&theirs.doc, &ours.doc)] {
            let result = merged(merge(&base, left, right, &HashMap::new()).unwrap());
            assert_eq!(
                result.design.component_links[&inserted].component,
                "Reusable"
            );
            for root in components::source_roots(&result.design) {
                assert!(!result.node(root).unwrap().visible);
                assert!(!result.children(Some(root)).is_empty());
            }
            let mut editor = Editor::new(result, None);
            components::reset(&mut editor, inserted, None).unwrap();
            editor.doc.validate().unwrap();
        }
    }

    #[test]
    fn deleted_component_variant_is_retained_for_new_branch_consumer() {
        use crate::{Editor, design_components as components};
        let mut source = Editor::new(doc(), None);
        let id = source.doc.nodes[0].id;
        let instance = components::create(&mut source, &[id], "Reusable").unwrap();
        components::update(&mut source, instance, Some("Alternate")).unwrap();
        let base = source.doc.clone();
        let mut ours = Editor::new(base.clone(), None);
        let default_root = base.design.components["Reusable"].variants["Default"];
        ours.execute(Command::RemoveNode { id: default_root })
            .unwrap();
        let mut theirs = Editor::new(base.clone(), None);
        let inserted = components::insert(&mut theirs, "Reusable", "Default", (10., 0.)).unwrap();
        let result = merged(merge(&base, &ours.doc, &theirs.doc, &HashMap::new()).unwrap());
        assert_eq!(result.design.components["Reusable"].variants.len(), 2);
        assert_eq!(result.design.component_links[&inserted].variant, "Default");
        let mut editor = Editor::new(result, None);
        components::reset(&mut editor, inserted, None).unwrap();
    }

    #[test]
    fn independent_branch_link_tokens_remain_separate() {
        let base = doc();
        let mut ours = base.clone();
        let mut theirs = base.clone();
        for branch in [&mut ours, &mut theirs] {
            let first = Command::AddNode {
                node: Box::new(Node::raster(
                    0,
                    "one",
                    Arc::new(Raster::transparent(2, 2)),
                    Placement::default(),
                )),
                slot: Slot::TOP,
            }
            .apply(branch)
            .unwrap()
            .unwrap();
            let second = Command::AddNode {
                node: Box::new(Node::raster(
                    0,
                    "two",
                    Arc::new(Raster::transparent(2, 2)),
                    Placement::default(),
                )),
                slot: Slot::TOP,
            }
            .apply(branch)
            .unwrap()
            .unwrap();
            Command::SetLayerLinks {
                ids: vec![first, second],
                linked: true,
            }
            .apply(branch)
            .unwrap();
        }
        let result = merged(merge(&base, &ours, &theirs, &HashMap::new()).unwrap());
        let mut groups = HashMap::new();
        for node in result.nodes {
            if let Some(token) = node.link_group {
                *groups.entry(token).or_insert(0) += 1;
            }
        }
        assert_eq!(groups.len(), 2);
        assert!(groups.values().all(|n| *n == 2));
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

    #[test]
    fn layer_lock_and_color_edits_merge_as_independent_properties() {
        use crate::node::{LayerColor, LayerLocks};
        let base = doc();
        let locks = LayerLocks {
            position: true,
            ..Default::default()
        };
        let ours = apply(&base, Command::SetLayerLocks { id: 1, locks });
        let theirs = apply(
            &base,
            Command::SetColorLabel {
                id: 1,
                color: LayerColor::Green,
            },
        );
        let merged = merged(merge(&base, &ours, &theirs, &HashMap::new()).unwrap());
        assert_eq!(merged.node(1).unwrap().locks, locks);
        assert_eq!(merged.node(1).unwrap().color_label, LayerColor::Green);
    }
}
