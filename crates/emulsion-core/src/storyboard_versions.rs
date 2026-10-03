//! Board versions: named points in a storyboard's history that change
//! tracking and Compare read back.
//!
//! Panel drawings already have history: every page keeps its own graph of
//! commits (see [`crate::graph`]). A board version records, for each panel,
//! the commit holding its drawing at that moment, plus a copy of the board
//! data (order, names, captions, timing, cameras and layer keys), which
//! has no history of its own. Pixels are shared with the page graphs, so a
//! version costs little more than the board data.
//!
//! Versions sit outside Undo, like page versions. A removed panel's graph
//! is kept while a version still shows it ([`BoardVersions::retired`]) and
//! saved in the package beside the pages.
use crate::Document;
use crate::graph::{CommitId, Graph};
use crate::project::{PageId, PageMeta, Project, ProjectKind, ProjectPage};
use crate::storyboard::Storyboard;
use serde::{Deserialize, Serialize};
use std::collections::{BTreeMap, BTreeSet, HashSet};

pub const MAX_VERSIONS: usize = 200;

#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
pub struct BoardVersion {
    pub id: u64,
    pub name: String,
    /// Seconds since the Unix epoch.
    pub time: u64,
    /// Panels in board order, with their names.
    pub layout: Vec<PageMeta>,
    /// The board data then, without its library, sounds or versions.
    pub board: Storyboard,
    /// Each panel's drawing, as a commit of that page's history graph.
    pub pages: BTreeMap<PageId, CommitId>,
}

#[derive(Clone, Debug, Default, Serialize, Deserialize)]
pub struct BoardVersions {
    /// Oldest first.
    #[serde(default)]
    pub list: Vec<BoardVersion>,
    #[serde(default)]
    pub next_id: u64,
    /// History graphs of removed panels that a version still shows. The
    /// package stores them beside the pages, not in the board data.
    #[serde(skip)]
    pub retired: BTreeMap<PageId, Graph>,
}

impl PartialEq for BoardVersions {
    fn eq(&self, other: &Self) -> bool {
        self.list == other.list && self.next_id == other.next_id
    }
}

impl BoardVersions {
    pub fn is_empty(&self) -> bool {
        self.list.is_empty()
    }
    pub fn get(&self, id: u64) -> Option<&BoardVersion> {
        self.list.iter().find(|v| v.id == id)
    }
    /// Every page some version shows.
    pub fn referenced(&self) -> BTreeSet<PageId> {
        self.list
            .iter()
            .flat_map(|v| v.pages.keys().copied())
            .collect()
    }
    pub(crate) fn validate(&self) -> Result<(), String> {
        if self.list.len() > MAX_VERSIONS {
            return Err(format!(
                "A storyboard keeps at most {MAX_VERSIONS} versions."
            ));
        }
        let mut ids = HashSet::new();
        for v in &self.list {
            if v.id == 0 || v.id >= self.next_id || !ids.insert(v.id) {
                return Err("Board version IDs must be unique and allocated.".into());
            }
            crate::storyboard::check_name(&v.name, "Version")?;
            let layout: Vec<_> = v.layout.iter().map(|m| m.id).collect();
            for meta in &v.layout {
                meta.validate()?;
            }
            if v.pages.len() != layout.len() || layout.iter().any(|id| !v.pages.contains_key(id)) {
                return Err(format!("Version “{}” does not list every panel.", v.name));
            }
            if !v.board.versions.is_empty() {
                return Err("Board versions cannot hold versions.".into());
            }
            v.board.validate(&layout)?;
        }
        Ok(())
    }
}

/// The board data a version keeps: drawings live in page graphs, and the
/// library and sounds are not compared.
pub(crate) fn version_board(board: &Storyboard) -> Storyboard {
    Storyboard {
        library: Default::default(),
        timeline: Default::default(),
        versions: Default::default(),
        ..board.clone()
    }
}

/// A storyboard as it is now or was at a version, read-only.
#[derive(Clone, Debug)]
pub struct BoardState {
    /// What this is, for lists: “Current”, a version name, “Last save”.
    pub label: String,
    pub layout: Vec<PageMeta>,
    pub board: Storyboard,
    /// Panel drawings. A version's drawing is missing when its commit was
    /// dropped with a deleted branch.
    pub docs: BTreeMap<PageId, Document>,
}

impl BoardState {
    /// A storyboard project's board as a state, for change tracking.
    pub fn of_project(project: &Project, label: &str) -> Self {
        Self {
            label: label.into(),
            layout: project.pages.iter().map(|p| p.meta.clone()).collect(),
            board: project
                .storyboard
                .clone()
                .unwrap_or_else(|| Storyboard::new(crate::storyboard::Settings::new(1, 1), &[])),
            docs: project
                .pages
                .iter()
                .map(|p| (p.meta.id, p.doc.clone()))
                .collect(),
        }
    }
    pub fn order(&self) -> Vec<PageId> {
        self.layout.iter().map(|m| m.id).collect()
    }
    pub fn name(&self, id: PageId) -> Option<&str> {
        self.layout
            .iter()
            .find(|m| m.id == id)
            .map(|m| m.name.as_str())
    }
    pub fn doc(&self, id: PageId) -> Option<&Document> {
        self.docs.get(&id)
    }
    /// A separate read-only project of this state, for reading or exporting
    /// an old version without touching the open one. None when a drawing is
    /// missing.
    pub fn to_project(&self) -> Option<Project> {
        let pages = self
            .layout
            .iter()
            .map(|meta| {
                let doc = self.docs.get(&meta.id)?.clone();
                Some(ProjectPage {
                    meta: meta.clone(),
                    graph: Graph::new(doc.clone(), "Version"),
                    doc,
                })
            })
            .collect::<Option<Vec<_>>>()?;
        Some(Project {
            kind: ProjectKind::Storyboard,
            active: pages.first()?.meta.id,
            next_page_id: self.layout.iter().map(|m| m.id).max()? + 1,
            pages,
            storyboard: Some(self.board.clone()),
        })
    }
}

/// What change tracking compares the board against.
#[derive(Clone, Copy, Debug, PartialEq, Eq, Hash)]
pub enum Baseline {
    Version(u64),
    /// The board as last opened or saved.
    LastSave,
    /// The board as last exported or printed in this session.
    LastExport,
}

/// The open project's versions and the session's save and export points.
#[derive(Clone, Debug, Default)]
pub(crate) struct Tracking {
    pub versions: BoardVersions,
    /// Bumped by every change to `versions`, to know when they need saving.
    pub revision: u64,
    pub saved_revision: u64,
    pub last_save: Option<std::sync::Arc<BoardState>>,
    pub last_export: Option<std::sync::Arc<BoardState>>,
    /// Bumped whenever a version, the last save or the last export
    /// changes, so views know to compare again.
    pub epoch: u64,
}
