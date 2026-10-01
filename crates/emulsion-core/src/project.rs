//! Multi-page projects reuse the existing document editor on every page.
//! Page structure and content share chronological undo without copying the
//! pixel/history buffers of every page for each edit.
use crate::storyboard::Storyboard;
use crate::{Document, Editor, graph::Graph, history::edit_order};
use serde::{Deserialize, Serialize};
use std::collections::{BTreeMap, HashSet};
use std::ops::{Deref, DerefMut};
use std::path::PathBuf;
use std::sync::Arc;

pub type PageId = u64;
mod storyboard_ops;
pub use storyboard_ops::{ClipPanel, GroupStart, PanelClip};

pub const MAX_PAGES: usize = 4096;
pub const MAX_PROJECT_PIXELS: u64 = 1_000_000_000;
const MAX_PAGE_STEPS: usize = 100;

#[derive(Clone, Copy, Debug, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum ProjectKind {
    Design,
    Diagram,
    Storyboard,
}

impl ProjectKind {
    pub fn label(self) -> &'static str {
        match self {
            Self::Design => "Design",
            Self::Diagram => "Diagram",
            Self::Storyboard => "Storyboard",
        }
    }
}

#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
pub struct PageMeta {
    pub id: PageId,
    pub name: String,
    #[serde(default)]
    pub bleed_mm: f64,
}

impl PageMeta {
    pub fn validate(&self) -> Result<(), String> {
        if self.id == 0
            || self.name.trim().is_empty()
            || self.name.chars().count() > 200
            || self.name.chars().any(char::is_control)
        {
            return Err("Pages need a nonzero ID and a name of 1–200 characters.".into());
        }
        if !self.bleed_mm.is_finite() || !(0. ..=100.).contains(&self.bleed_mm) {
            return Err("Page bleed must be between 0 and 100 mm.".into());
        }
        Ok(())
    }
}

#[derive(Clone)]
pub struct ProjectPage {
    pub meta: PageMeta,
    pub doc: Document,
    pub graph: Graph,
}

#[derive(Clone)]
pub struct Project {
    pub kind: ProjectKind,
    pub pages: Vec<ProjectPage>,
    pub active: PageId,
    pub next_page_id: PageId,
    /// Present exactly when `kind` is Storyboard.
    pub storyboard: Option<Storyboard>,
}

impl Project {
    pub fn validate(&self) -> Result<(), String> {
        if self.pages.is_empty() || self.pages.len() > MAX_PAGES {
            return Err(format!("A project must contain 1–{MAX_PAGES} pages."));
        }
        let mut ids = HashSet::new();
        let mut pixels = 0u64;
        for page in &self.pages {
            page.meta.validate()?;
            page.doc.validate().map_err(|e| e.to_string())?;
            if !ids.insert(page.meta.id) {
                return Err("Duplicate page ID.".into());
            }
            pixels += u64::from(page.doc.width) * u64::from(page.doc.height);
        }
        if pixels > MAX_PROJECT_PIXELS {
            return Err("Project exceeds the total page area limit.".into());
        }
        if !ids.contains(&self.active)
            || self.next_page_id <= *ids.iter().max().unwrap()
            || self.next_page_id == u64::MAX
        {
            return Err("Invalid active page or page ID allocator.".into());
        }
        match (self.kind, &self.storyboard) {
            (ProjectKind::Storyboard, Some(board)) => {
                let layout: Vec<_> = self.pages.iter().map(|p| p.meta.id).collect();
                board.validate(&layout)?;
                let size = (board.settings.width, board.settings.height);
                if self
                    .pages
                    .iter()
                    .any(|p| (p.doc.width, p.doc.height) != size)
                {
                    return Err("Every storyboard panel must use the project resolution.".into());
                }
            }
            (ProjectKind::Storyboard, None) => {
                return Err("A storyboard project is missing its storyboard data.".into());
            }
            (_, Some(_)) => return Err("Only storyboard projects carry storyboard data.".into()),
            (_, None) => {}
        }
        Ok(())
    }
}

#[derive(Clone, Debug, PartialEq)]
pub struct ProjectStamp {
    layout: Vec<PageMeta>,
    revisions: Vec<(PageId, u64)>,
    storyboard: Option<Arc<Storyboard>>,
}

struct PageStep {
    layout: Vec<PageMeta>,
    active: PageId,
    order: u64,
    storyboard: Option<Arc<Storyboard>>,
}

/// The active page dereferences to Editor, preserving the existing editing API.
/// File operations must use `snapshot` for a project, never only its active doc.
pub struct ProjectEditor {
    kind: Option<ProjectKind>,
    layout: Vec<PageMeta>,
    pages: BTreeMap<PageId, Editor>,
    active: PageId,
    next_page_id: PageId,
    saved_layout: Option<Vec<PageMeta>>,
    undo_pages: Vec<PageStep>,
    redo_pages: Vec<PageStep>,
    last_page_edit: u64,
    history_groups: BTreeMap<u64, Vec<PageId>>,
    /// Shared with undo steps; replaced, never mutated in place.
    storyboard: Option<Arc<Storyboard>>,
    saved_storyboard: Option<Arc<Storyboard>>,
}

impl From<Editor> for ProjectEditor {
    fn from(editor: Editor) -> Self {
        let meta = PageMeta {
            id: 1,
            name: "Page 1".into(),
            bleed_mm: 0.,
        };
        Self {
            kind: None,
            layout: vec![meta.clone()],
            pages: BTreeMap::from([(1, editor)]),
            active: 1,
            next_page_id: 2,
            saved_layout: Some(vec![meta]),
            undo_pages: Vec::new(),
            redo_pages: Vec::new(),
            last_page_edit: 0,
            history_groups: BTreeMap::new(),
            storyboard: None,
            saved_storyboard: None,
        }
    }
}

impl Deref for ProjectEditor {
    type Target = Editor;
    fn deref(&self) -> &Editor {
        &self.pages[&self.active]
    }
}
impl DerefMut for ProjectEditor {
    fn deref_mut(&mut self) -> &mut Editor {
        self.pages
            .get_mut(&self.active)
            .expect("validated active page")
    }
}

impl ProjectEditor {
    pub fn execute(
        &mut self,
        command: crate::Command,
    ) -> Result<Option<crate::NodeId>, crate::CommandError> {
        self.pages.get_mut(&self.active).unwrap().execute(command)
    }
    pub fn new_project(kind: ProjectKind, doc: Document) -> Result<Self, String> {
        doc.validate().map_err(|e| e.to_string())?;
        let settings = crate::storyboard::Settings::new(doc.width, doc.height);
        let mut session: Self = Editor::new(doc, None).into();
        session.kind = Some(kind);
        session.saved_layout = None;
        if kind == ProjectKind::Storyboard {
            session.storyboard = Some(Arc::new(Storyboard::new(settings, &[1])));
        }
        Ok(session)
    }

    pub fn open(project: Project, path: Option<PathBuf>) -> Result<Self, String> {
        project.validate()?;
        let layout = project
            .pages
            .iter()
            .map(|page| page.meta.clone())
            .collect::<Vec<_>>();
        let pages = project
            .pages
            .into_iter()
            .map(|page| {
                (
                    page.meta.id,
                    Editor::with_graph(page.doc, path.clone(), page.graph),
                )
            })
            .collect();
        let storyboard = project.storyboard.map(Arc::new);
        let mut editor = Self {
            kind: Some(project.kind),
            saved_storyboard: path.as_ref().and(storyboard.clone()),
            storyboard,
            saved_layout: path.as_ref().map(|_| layout.clone()),
            layout,
            pages,
            active: project.active,
            next_page_id: project.next_page_id,
            undo_pages: Vec::new(),
            redo_pages: Vec::new(),
            last_page_edit: 0,
            history_groups: BTreeMap::new(),
        };
        editor.refresh_locks();
        Ok(editor)
    }

    pub fn kind(&self) -> Option<ProjectKind> {
        self.kind
    }
    pub fn active_page(&self) -> PageId {
        self.active
    }
    pub fn page_list(&self) -> &[PageMeta] {
        &self.layout
    }
    pub fn page(&self, id: PageId) -> Option<&Editor> {
        self.layout
            .iter()
            .any(|p| p.id == id)
            .then(|| self.pages.get(&id))
            .flatten()
    }
    pub fn set_active_page(&mut self, id: PageId) -> Result<(), String> {
        if self.in_transaction() {
            return Err("Finish the current edit before switching pages.".into());
        }
        if self.page(id).is_none() {
            return Err("Page does not exist.".into());
        }
        self.active = id;
        Ok(())
    }
    pub fn snapshot(&self) -> Option<Project> {
        Some(Project {
            kind: self.kind?,
            active: self.active,
            next_page_id: self.next_page_id,
            pages: self
                .layout
                .iter()
                .map(|meta| {
                    let editor = &self.pages[&meta.id];
                    ProjectPage {
                        meta: meta.clone(),
                        doc: editor.doc.clone(),
                        graph: editor.graph.clone(),
                    }
                })
                .collect(),
            storyboard: self.storyboard.as_deref().cloned(),
        })
    }
    /// Storyboard data, for Storyboard projects.
    pub fn storyboard(&self) -> Option<&Storyboard> {
        self.storyboard.as_deref()
    }
    /// Change storyboard data as one undoable step. The change is validated
    /// against the page layout before it lands; a no-op records nothing.
    pub fn edit_storyboard(
        &mut self,
        edit: impl FnOnce(&mut Storyboard) -> Result<(), String>,
    ) -> Result<(), String> {
        let current = self
            .storyboard
            .as_ref()
            .ok_or("This is not a storyboard project.")?;
        let mut next = Storyboard::clone(current);
        edit(&mut next)?;
        if next == **current {
            return Ok(());
        }
        // A panel locked before and after keeps its data; only its grouping
        // may change.
        for (id, before) in &current.panels {
            if let Some(after) = next.panels.get(id) {
                let unchanged = crate::storyboard::Panel {
                    scene: before.scene,
                    ..after.clone()
                } == *before;
                if current.is_locked(*id) && next.is_locked(*id) && !unchanged {
                    return Err("That panel is locked. Unlock it to change it.".into());
                }
            }
        }
        let layout: Vec<_> = self.layout.iter().map(|m| m.id).collect();
        next.validate(&layout)?;
        let size = (next.settings.width, next.settings.height);
        if self
            .layout
            .iter()
            .any(|m| (self.pages[&m.id].doc.width, self.pages[&m.id].doc.height) != size)
        {
            return Err("Storyboard resolution must match every panel.".into());
        }
        self.record_pages()?;
        self.storyboard = Some(Arc::new(next));
        self.refresh_locks();
        Ok(())
    }
    /// Insert storyboard panels after `after` (or first when `None`) as one
    /// undoable step. Every new panel starts as `blank`, which shares its pixel
    /// buffers until painted. `start` begins a new scene, sequence or act
    /// with the first inserted panel. The first new panel becomes active.
    pub fn insert_panels(
        &mut self,
        after: Option<PageId>,
        blank: &Document,
        panels: Vec<(String, crate::storyboard::Panel)>,
        start: Option<(crate::storyboard::Level, Option<&str>)>,
    ) -> Result<Vec<PageId>, String> {
        let starts: Vec<_> = start
            .map(|(level, name)| GroupStart {
                at: 0,
                level,
                name: name.map(str::to_string),
            })
            .into_iter()
            .collect();
        let items = panels
            .into_iter()
            .map(|(name, panel)| (name, blank.clone(), panel))
            .collect();
        self.insert_panel_documents(after, items, &starts, None)
    }
    /// Keep storyboard membership in step with the page layout.
    fn sync_storyboard(&mut self) {
        if let Some(board) = &self.storyboard {
            let layout: Vec<_> = self.layout.iter().map(|m| m.id).collect();
            let mut next = Storyboard::clone(board);
            next.reconcile(&layout);
            if next != **board {
                self.storyboard = Some(Arc::new(next));
            }
        }
        self.refresh_locks();
    }
    fn check_panel_size(&self, doc: &Document) -> Result<(), String> {
        match &self.storyboard {
            Some(board)
                if (doc.width, doc.height) != (board.settings.width, board.settings.height) =>
            {
                Err("Storyboard panels must use the project resolution.".into())
            }
            _ => Ok(()),
        }
    }
    pub fn stamp(&self) -> ProjectStamp {
        ProjectStamp {
            storyboard: self.storyboard.clone(),
            layout: self.layout.clone(),
            revisions: self
                .layout
                .iter()
                .map(|p| (p.id, self.pages[&p.id].revision))
                .collect(),
        }
    }
    pub fn is_modified(&self) -> bool {
        (self.kind.is_some()
            && (self.saved_layout.as_ref() != Some(&self.layout)
                || self.saved_storyboard != self.storyboard))
            || self.layout.iter().any(|p| self.pages[&p.id].is_modified())
    }
    /// Mark exactly the saved revisions, allowing continued editing during IO.
    pub fn mark_project_saved(&mut self, path: PathBuf, stamp: &ProjectStamp) {
        self.saved_layout = Some(stamp.layout.clone());
        self.saved_storyboard = stamp.storyboard.clone();
        for (id, editor) in &mut self.pages {
            editor.path = Some(path.clone());
            if let Some((_, revision)) = stamp.revisions.iter().find(|(saved, _)| saved == id) {
                editor.mark_saved(path.clone(), *revision);
            }
        }
    }

    fn page_step(&self, order: u64) -> PageStep {
        PageStep {
            layout: self.layout.clone(),
            active: self.active,
            order,
            storyboard: self.storyboard.clone(),
        }
    }
    fn record_pages(&mut self) -> Result<(), String> {
        if self.kind.is_none() {
            return Err("Pages require a Design or Diagram project.".into());
        }
        if self.in_transaction() {
            return Err("Finish the current edit first.".into());
        }
        self.last_page_edit = edit_order();
        self.undo_pages.push(self.page_step(self.last_page_edit));
        self.redo_pages.clear();
        if self.undo_pages.len() > MAX_PAGE_STEPS {
            self.undo_pages.remove(0);
        }
        Ok(())
    }
    fn collect_pages(&mut self) {
        let used: HashSet<_> = self
            .layout
            .iter()
            .chain(
                self.undo_pages
                    .iter()
                    .chain(&self.redo_pages)
                    .flat_map(|s| &s.layout),
            )
            .map(|m| m.id)
            .collect();
        self.pages.retain(|id, _| used.contains(id));
    }
    pub fn add_page(
        &mut self,
        doc: Document,
        name: String,
        bleed_mm: f64,
    ) -> Result<PageId, String> {
        if self.layout.len() >= MAX_PAGES {
            return Err(format!("A project supports at most {MAX_PAGES} pages."));
        }
        let id = self.next_page_id;
        let meta = PageMeta {
            id,
            name: name.trim().into(),
            bleed_mm,
        };
        meta.validate()?;
        doc.validate().map_err(|e| e.to_string())?;
        self.check_panel_size(&doc)?;
        if id >= u64::MAX - 1 {
            return Err("Page ID limit reached.".into());
        }
        let pixels: u64 = self
            .layout
            .iter()
            .map(|m| {
                let d = &self.pages[&m.id].doc;
                u64::from(d.width) * u64::from(d.height)
            })
            .sum();
        if pixels + u64::from(doc.width) * u64::from(doc.height) > MAX_PROJECT_PIXELS {
            return Err("Project exceeds the total page area limit.".into());
        }
        self.record_pages()?;
        let editor = Editor::new(doc, self.path.clone());
        self.pages.insert(id, editor);
        let index = self
            .layout
            .iter()
            .position(|m| m.id == self.active)
            .unwrap()
            + 1;
        self.layout.insert(index, meta);
        self.active = id;
        self.next_page_id += 1;
        self.collect_pages();
        self.sync_storyboard();
        Ok(id)
    }
    /// Import every page as one undoable layout change, retaining page histories.
    pub fn import_pages(&mut self, mut project: Project) -> Result<Vec<PageId>, String> {
        project.validate()?;
        for page in &project.pages {
            self.check_panel_size(&page.doc)?;
        }
        if self.layout.len() + project.pages.len() > MAX_PAGES {
            return Err(format!("A project supports at most {MAX_PAGES} pages."));
        }
        let pixels: u64 = self
            .layout
            .iter()
            .map(|m| {
                let d = &self.pages[&m.id].doc;
                u64::from(d.width) * u64::from(d.height)
            })
            .chain(
                project
                    .pages
                    .iter()
                    .map(|p| u64::from(p.doc.width) * u64::from(p.doc.height)),
            )
            .sum();
        if pixels > MAX_PROJECT_PIXELS {
            return Err("Project exceeds the total page area limit.".into());
        }
        if self
            .next_page_id
            .checked_add(project.pages.len() as u64)
            .is_none_or(|id| id >= u64::MAX - 1)
        {
            return Err("Page ID limit reached.".into());
        }
        let page_ids = project
            .pages
            .iter()
            .enumerate()
            .map(|(offset, page)| (page.meta.id, self.next_page_id + offset as u64))
            .collect();
        for page in &mut project.pages {
            page.doc.design.remap_pages(&page_ids);
            page.graph.remap_pages(&page_ids);
        }
        self.record_pages()?;
        let index = self
            .layout
            .iter()
            .position(|m| m.id == self.active)
            .unwrap()
            + 1;
        let mut ids = Vec::new();
        for (offset, mut page) in project.pages.into_iter().enumerate() {
            let id = self.next_page_id;
            self.next_page_id += 1;
            page.meta.id = id;
            self.pages.insert(
                id,
                Editor::with_graph(page.doc, self.path.clone(), page.graph),
            );
            self.layout.insert(index + offset, page.meta);
            ids.push(id);
        }
        self.active = ids[0];
        self.collect_pages();
        self.sync_storyboard();
        Ok(ids)
    }
    pub fn duplicate_page(&mut self, id: PageId) -> Result<PageId, String> {
        let meta = self
            .layout
            .iter()
            .find(|m| m.id == id)
            .ok_or("Page does not exist.")?
            .clone();
        let doc = self.pages[&id].doc.clone();
        let previous = self.active;
        if self.storyboard.is_some() {
            // The next frame of a storyboard: right after its source, in the
            // same scene, keeping shot data, timing and captions.
            if self.in_transaction() {
                return Err("Finish the current edit first.".into());
            }
            self.active = id;
        }
        let copy = self
            .add_page(
                doc,
                format!("{} copy", meta.name.chars().take(195).collect::<String>()),
                meta.bleed_mm,
            )
            .inspect_err(|_| self.active = previous)?;
        if let Some(board) = &self.storyboard {
            let mut next = Storyboard::clone(board);
            let panel = crate::storyboard::Panel {
                locked: false,
                ..next.panels[&id].clone()
            };
            next.panels.insert(copy, panel);
            self.storyboard = Some(Arc::new(next));
            self.refresh_locks();
        }
        Ok(copy)
    }
    pub fn remove_page(&mut self, id: PageId) -> Result<(), String> {
        if self.layout.len() == 1 {
            return Err("Keep at least one page in the project.".into());
        }
        let index = self
            .layout
            .iter()
            .position(|m| m.id == id)
            .ok_or("Page does not exist.")?;
        if self.storyboard.as_ref().is_some_and(|b| b.is_locked(id)) {
            return Err("That panel is locked. Unlock it to remove it.".into());
        }
        self.record_pages()?;
        self.layout.remove(index);
        if self.active == id {
            self.active = self.layout[index.min(self.layout.len() - 1)].id;
        }
        self.collect_pages();
        self.sync_storyboard();
        Ok(())
    }
    pub fn rename_page(&mut self, id: PageId, name: String, bleed_mm: f64) -> Result<(), String> {
        let meta = PageMeta {
            id,
            name: name.trim().into(),
            bleed_mm,
        };
        meta.validate()?;
        let index = self
            .layout
            .iter()
            .position(|m| m.id == id)
            .ok_or("Page does not exist.")?;
        if self.layout[index] == meta {
            return Ok(());
        }
        self.record_pages()?;
        self.layout[index] = meta;
        self.collect_pages();
        Ok(())
    }
    pub fn move_page(&mut self, id: PageId, to: usize) -> Result<(), String> {
        let from = self
            .layout
            .iter()
            .position(|m| m.id == id)
            .ok_or("Page does not exist.")?;
        if to >= self.layout.len() {
            return Err("Page position is outside the project.".into());
        }
        if from == to {
            return Ok(());
        }
        self.record_pages()?;
        let meta = self.layout.remove(from);
        self.layout.insert(to, meta);
        self.collect_pages();
        self.sync_storyboard();
        Ok(())
    }
    /// Prepare all affected pages before committing any of them, as one Undo action.
    pub(crate) fn commit_documents(
        &mut self,
        mut documents: BTreeMap<PageId, Document>,
        label: &str,
    ) -> Result<(), String> {
        if self.kind.is_none() || self.in_transaction() {
            return Err("Finish the current edit in a project first.".into());
        }
        for (id, doc) in &mut documents {
            let editor = self.page(*id).ok_or("Project page no longer exists.")?;
            if editor.in_transaction() {
                return Err("Finish edits on every affected page first.".into());
            }
            if editor.is_read_only() {
                return Err(crate::CommandError::ReadOnly.to_string());
            }
            let mut trial = Editor::new(editor.doc.clone(), None);
            trial.commit_design_document(doc.clone(), label)?;
            *doc = trial.doc;
        }
        let order = edit_order();
        let affected: Vec<_> = documents
            .iter()
            .filter_map(|(id, doc)| (self.pages[id].doc != *doc).then_some(*id))
            .collect();
        for (id, doc) in documents {
            self.pages
                .get_mut(&id)
                .unwrap()
                .commit_project_document(doc, label, order);
        }
        if affected.len() > 1 {
            self.history_groups.insert(order, affected);
        }
        self.expire_incomplete_groups();
        Ok(())
    }
    fn expire_incomplete_groups(&mut self) {
        loop {
            let expired: Vec<_> = self
                .history_groups
                .iter()
                .filter_map(|(order, pages)| {
                    let complete = [false, true].into_iter().any(|redo| {
                        pages.iter().all(|id| {
                            self.pages
                                .get(id)
                                .is_some_and(|editor| editor.history.contains_order(redo, *order))
                        })
                    });
                    (!complete).then_some(*order)
                })
                .collect();
            if expired.is_empty() {
                break;
            }
            for order in expired {
                self.history_groups.remove(&order);
                for editor in self.pages.values_mut() {
                    editor.expire_group_history(order);
                }
            }
        }
    }
    fn candidate(&self, redo: bool) -> (PageId, u64) {
        self.layout
            .iter()
            .map(|p| {
                let h = &self.pages[&p.id].history;
                (p.id, if redo { h.redo_order() } else { h.undo_order() })
            })
            .max_by_key(|(_, order)| *order)
            .unwrap_or((self.active, 0))
    }
    fn last_fresh_edit(&self) -> u64 {
        self.pages
            .values()
            .map(|e| e.last_edit_order)
            .max()
            .unwrap_or(0)
            .max(self.last_page_edit)
    }
    pub fn can_undo(&self) -> bool {
        self.candidate(false).1 > 0 || !self.undo_pages.is_empty()
    }
    pub fn can_redo(&self) -> bool {
        if self.kind.is_none() {
            return self.history.can_redo();
        }
        self.candidate(true)
            .1
            .max(self.redo_pages.last().map_or(0, |s| s.order))
            > self.last_fresh_edit()
    }
    pub fn undo(&mut self) -> bool {
        self.travel(false)
    }
    pub fn redo(&mut self) -> bool {
        self.travel(true)
    }
    fn travel(&mut self, redo: bool) -> bool {
        if self.kind.is_none() {
            let editor = self.pages.get_mut(&self.active).unwrap();
            return if redo { editor.redo() } else { editor.undo() };
        }
        while self.in_transaction() {
            self.end();
        }
        self.expire_incomplete_groups();
        let (id, order) = self.candidate(redo);
        let page_order = if redo {
            &self.redo_pages
        } else {
            &self.undo_pages
        }
        .last()
        .map_or(0, |s| s.order);
        if order.max(page_order) == 0 || (redo && order.max(page_order) <= self.last_fresh_edit()) {
            return false;
        }
        if page_order > order {
            let current = self.page_step(edit_order());
            let previous = if redo {
                self.redo_pages.pop().unwrap()
            } else {
                self.undo_pages.pop().unwrap()
            };
            if redo {
                self.undo_pages.push(current);
            } else {
                self.redo_pages.push(current);
            }
            self.layout = previous.layout;
            self.active = previous.active;
            self.storyboard = previous.storyboard;
            self.refresh_locks();
            true
        } else {
            self.active = id;
            let affected: Vec<_> = self
                .layout
                .iter()
                .filter_map(|page| {
                    let h = &self.pages[&page.id].history;
                    ((if redo { h.redo_order() } else { h.undo_order() }) == order)
                        .then_some(page.id)
                })
                .collect();
            // A removed page must be restored by page history before a shared
            // content step can travel. Never apply a surviving subset.
            if self.history_groups.get(&order).is_some_and(|expected| {
                expected.len() != affected.len() || expected.iter().any(|id| !affected.contains(id))
            }) {
                return false;
            }
            let grouped_order = edit_order();
            if let Some(pages) = self.history_groups.remove(&order) {
                self.history_groups.insert(grouped_order, pages);
            }
            for page in affected {
                let editor = self.pages.get_mut(&page).unwrap();
                if redo {
                    editor.redo();
                } else {
                    editor.undo();
                }
                editor.group_history(!redo, grouped_order);
            }
            true
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::{Command, Node, NodeKind, command::Slot};
    fn edit(editor: &mut ProjectEditor, name: &str) {
        editor
            .execute(Command::AddNode {
                node: Box::new(Node::new(0, name, NodeKind::Fill { rgba: [255; 4] })),
                slot: Slot::TOP,
            })
            .unwrap();
    }
    #[test]
    fn page_structure_and_content_undo_in_global_order_and_branch_redo_is_invalidated() {
        let mut p = ProjectEditor::new_project(ProjectKind::Design, Document::new(64, 48)).unwrap();
        edit(&mut p, "First");
        let second = p
            .add_page(Document::new(80, 60), "Second".into(), 3.)
            .unwrap();
        edit(&mut p, "Second");
        p.set_active_page(1).unwrap();
        edit(&mut p, "Later on first");
        assert!(p.undo());
        assert_eq!(p.active_page(), 1);
        assert_eq!(p.doc.nodes.len(), 1);
        assert!(p.undo());
        assert_eq!(p.active_page(), second);
        assert!(p.doc.nodes.is_empty());
        assert!(p.undo());
        assert_eq!(p.page_list().len(), 1);
        assert!(p.redo());
        assert_eq!(p.page_list().len(), 2);
        assert!(p.redo());
        assert_eq!(p.doc.nodes[0].name, "Second");
        assert!(p.redo());
        assert_eq!(p.active_page(), 1);
        assert_eq!(p.doc.nodes.len(), 2);
        assert!(p.undo());
        p.set_active_page(second).unwrap();
        edit(&mut p, "New timeline");
        assert!(!p.can_redo());
        assert!(!p.redo());
    }
    #[test]
    fn deleted_pages_restore_edits_ids_and_dirty_state_and_save_races_stay_dirty() {
        let mut p = ProjectEditor::new_project(ProjectKind::Design, Document::new(64, 48)).unwrap();
        let second = p.duplicate_page(1).unwrap();
        edit(&mut p, "Keep me");
        let stamp = p.stamp();
        p.mark_project_saved("design.emu".into(), &stamp);
        assert!(!p.is_modified());
        p.remove_page(second).unwrap();
        assert!(p.is_modified());
        p.undo();
        assert_eq!(p.active_page(), second);
        assert_eq!(p.doc.nodes[0].name, "Keep me");
        assert!(!p.is_modified());
        p.rename_page(second, "Back cover".into(), 5.).unwrap();
        p.move_page(second, 0).unwrap();
        let snapshot = p.snapshot().unwrap();
        snapshot.validate().unwrap();
        let mut reopened = ProjectEditor::open(snapshot, Some("design.emu".into())).unwrap();
        assert_eq!(reopened.page_list()[0].name, "Back cover");
        let stamp = reopened.stamp();
        edit(&mut reopened, "During save");
        reopened.mark_project_saved("design.emu".into(), &stamp);
        assert!(reopened.is_modified());
        let next = p.add_page(Document::new(8, 8), "Third".into(), 0.).unwrap();
        p.undo();
        assert!(
            p.add_page(Document::new(8, 8), "Replacement".into(), 0.)
                .unwrap()
                > next
        );
    }
    #[test]
    fn invalid_page_operations_are_atomic() {
        let mut p = ProjectEditor::new_project(ProjectKind::Design, Document::new(64, 48)).unwrap();
        let stamp = p.stamp();
        assert!(p.remove_page(1).is_err());
        assert!(p.rename_page(1, "".into(), 0.).is_err());
        assert!(p.rename_page(1, "A".into(), f64::NAN).is_err());
        assert!(p.add_page(Document::new(0, 0), "Bad".into(), 0.).is_err());
        assert_eq!(p.stamp(), stamp);
        assert!(!p.can_undo());
    }
}

#[cfg(test)]
mod grouped_history_tests {
    use super::*;
    use crate::{Command, Node, NodeKind, command::Slot};
    fn fixture() -> ProjectEditor {
        let mut doc = Document::new(100, 100);
        Command::AddNode {
            node: Box::new(Node::new(0, "Before", NodeKind::Fill { rgba: [255; 4] })),
            slot: Slot::TOP,
        }
        .apply(&mut doc)
        .unwrap();
        ProjectEditor::open(
            Project {
                storyboard: None,
                kind: ProjectKind::Design,
                pages: (1..=2)
                    .map(|id| ProjectPage {
                        meta: PageMeta {
                            id,
                            name: format!("Page {id}"),
                            bleed_mm: 0.,
                        },
                        doc: doc.clone(),
                        graph: Graph::new(doc.clone(), "Opened"),
                    })
                    .collect(),
                active: 1,
                next_page_id: 3,
            },
            None,
        )
        .unwrap()
    }
    fn group(p: &mut ProjectEditor) {
        let documents = (1..=2)
            .map(|id| {
                let mut doc = p.page(id).unwrap().doc.clone();
                doc.nodes[0].name = "Published".into();
                (id, doc)
            })
            .collect();
        p.commit_documents(documents, "Publish").unwrap();
    }
    #[test]
    fn grouped_history_eviction_expires_whole_group_but_retains_newer_edits() {
        let mut p = fixture();
        group(&mut p);
        let id = p.doc.nodes[0].id;
        for i in 0..101 {
            p.execute(Command::Rename {
                id,
                name: format!("Later {i}"),
            })
            .unwrap();
        }
        // The first page has evicted the group; the second still has it.
        assert_eq!(p.page(2).unwrap().history.len(), 1);
        assert!(p.undo());
        assert_eq!(p.doc.node(id).unwrap().name, "Later 99");
        assert!(!p.page(2).unwrap().history.can_undo());
        for _ in 0..99 {
            assert!(p.undo());
        }
        assert!(!p.undo());
        assert_eq!(p.page(2).unwrap().doc.nodes[0].name, "Published");
        // Redo newer work remains available; expired publication stays applied.
        assert!(p.redo());
        assert_eq!(p.page(2).unwrap().doc.nodes[0].name, "Published");
    }
    #[test]
    fn grouped_history_new_branch_cannot_redo_only_one_page() {
        let mut p = fixture();
        group(&mut p);
        assert!(p.undo());
        p.set_active_page(1).unwrap();
        let id = p.doc.nodes[0].id;
        p.execute(Command::Rename {
            id,
            name: "Branch".into(),
        })
        .unwrap();
        assert!(!p.redo());
        assert_eq!(p.page(2).unwrap().doc.nodes[0].name, "Before");
        assert!(!p.page(2).unwrap().history.can_redo());
        assert!(p.undo());
        assert!(p.redo());
        assert_eq!(p.page(1).unwrap().doc.nodes[0].name, "Branch");
        assert_eq!(p.page(2).unwrap().doc.nodes[0].name, "Before");
    }

    #[test]
    fn storyboard_layout_changes_keep_panels_in_step_and_undo_together() {
        let mut p =
            ProjectEditor::new_project(ProjectKind::Storyboard, Document::new(32, 18)).unwrap();
        assert_eq!(p.storyboard().unwrap().panels.len(), 1);
        let second = p
            .add_page(Document::new(32, 18), "Panel 2".into(), 0.)
            .unwrap();
        assert!(
            p.add_page(Document::new(30, 18), "Wrong".into(), 0.)
                .is_err()
        );
        assert_eq!(p.storyboard().unwrap().panels.len(), 2);
        p.edit_storyboard(|b| {
            let action = b.caption("Action").unwrap();
            b.panels
                .get_mut(&second)
                .unwrap()
                .captions
                .insert(action, "Door opens".into());
            Ok(())
        })
        .unwrap();
        assert!(
            p.edit_storyboard(|b| {
                b.panels.get_mut(&second).unwrap().frames = 0;
                Ok(())
            })
            .is_err()
        );
        p.remove_page(1).unwrap();
        assert_eq!(p.storyboard().unwrap().panels.len(), 1);
        p.snapshot().unwrap().validate().unwrap();
        assert!(p.undo());
        assert_eq!(p.storyboard().unwrap().panels.len(), 2);
        assert!(p.undo());
        assert!(p.storyboard().unwrap().panels[&second].captions.is_empty());
        assert!(p.redo());
        assert!(!p.storyboard().unwrap().panels[&second].captions.is_empty());
        p.snapshot().unwrap().validate().unwrap();
    }

    #[test]
    fn duplicating_a_storyboard_panel_makes_the_next_frame() {
        let mut p =
            ProjectEditor::new_project(ProjectKind::Storyboard, Document::new(32, 18)).unwrap();
        p.execute(Command::AddNode {
            node: Box::new(crate::Node::new(
                0,
                "Hero",
                crate::NodeKind::Fill { rgba: [255; 4] },
            )),
            slot: crate::command::Slot::TOP,
        })
        .unwrap();
        let last = p
            .add_page(Document::new(32, 18), "Panel 2".into(), 0.)
            .unwrap();
        p.edit_storyboard(|b| {
            let action = b.caption("Action").unwrap();
            let panel = b.panels.get_mut(&1).unwrap();
            panel.captions.insert(action, "Hero turns".into());
            panel.frames = 12;
            Ok(())
        })
        .unwrap();
        let copy = p.duplicate_page(1).unwrap();
        let order: Vec<_> = p.page_list().iter().map(|m| m.id).collect();
        assert_eq!(order, [1, copy, last]);
        assert_eq!(p.doc.nodes[0].name, "Hero");
        let board = p.storyboard().unwrap();
        assert_eq!(board.panels[&copy], board.panels[&1]);
        p.snapshot().unwrap().validate().unwrap();
        assert!(p.undo());
        assert_eq!(p.page_list().len(), 2);
        p.snapshot().unwrap().validate().unwrap();
    }

    #[test]
    fn inserted_panels_take_their_data_and_can_start_a_scene_in_one_step() {
        use crate::storyboard::{Level, Panel, ShotSize};
        let mut p =
            ProjectEditor::new_project(ProjectKind::Storyboard, Document::new(32, 18)).unwrap();
        let blank = Document::new(32, 18);
        let mut wide = Panel::new(0, 24);
        wide.size = ShotSize::Wide;
        let ids = p
            .insert_panels(
                Some(1),
                &blank,
                vec![
                    ("Panel 2".into(), wide.clone()),
                    ("Panel 3".into(), Panel::new(0, 36)),
                ],
                Some((Level::Scene, Some("2"))),
            )
            .unwrap();
        let board = p.storyboard().unwrap();
        assert_eq!(board.panels[&ids[0]].size, ShotSize::Wide);
        assert_eq!(board.panels[&ids[1]].frames, 36);
        let layout: Vec<_> = p.page_list().iter().map(|m| m.id).collect();
        assert_eq!(board.outline(&layout).len(), 2);
        assert_eq!(p.active_page(), ids[0]);
        p.snapshot().unwrap().validate().unwrap();
        // Invalid input changes nothing.
        let stamp = p.stamp();
        assert!(
            p.insert_panels(None, &blank, vec![("x".into(), Panel::new(0, 0))], None)
                .is_err()
        );
        assert!(
            p.insert_panels(Some(99), &blank, vec![("x".into(), wide)], None)
                .is_err()
        );
        assert!(
            p.insert_panels(
                None,
                &Document::new(8, 8),
                vec![("x".into(), Panel::new(0, 1))],
                None
            )
            .is_err()
        );
        assert_eq!(p.stamp(), stamp);
        assert!(p.undo());
        assert_eq!(p.page_list().len(), 1);
        p.snapshot().unwrap().validate().unwrap();
    }

    #[test]
    fn storyboard_edits_count_as_unsaved_changes() {
        let mut p =
            ProjectEditor::new_project(ProjectKind::Storyboard, Document::new(16, 9)).unwrap();
        let stamp = p.stamp();
        p.mark_project_saved("board.emu".into(), &stamp);
        assert!(!p.is_modified());
        p.edit_storyboard(|b| {
            b.settings.panel_frames = 12;
            Ok(())
        })
        .unwrap();
        assert!(p.is_modified());
        p.undo();
        assert!(!p.is_modified());
        assert!(p.edit_storyboard(|_| Ok(())).is_ok());
        assert!(!p.can_redo() || p.redo());
    }

    #[test]
    fn design_projects_carry_no_storyboard_data() {
        let mut p = ProjectEditor::new_project(ProjectKind::Design, Document::new(16, 9)).unwrap();
        assert!(p.storyboard().is_none());
        assert!(p.edit_storyboard(|_| Ok(())).is_err());
        let mut snapshot = p.snapshot().unwrap();
        snapshot.storyboard = Some(crate::storyboard::Storyboard::new(
            crate::storyboard::Settings::new(16, 9),
            &[1],
        ));
        assert!(snapshot.validate().is_err());
    }
}
