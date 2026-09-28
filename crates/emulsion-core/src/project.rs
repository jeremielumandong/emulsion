//! Multi-page projects reuse the existing document editor on every page.
//! Page structure and content share chronological undo without copying the
//! pixel/history buffers of every page for each edit.
use crate::{Document, Editor, graph::Graph, history::edit_order};
use serde::{Deserialize, Serialize};
use std::collections::{BTreeMap, HashSet};
use std::ops::{Deref, DerefMut};
use std::path::PathBuf;

pub type PageId = u64;
pub const MAX_PAGES: usize = 100;
pub const MAX_PROJECT_PIXELS: u64 = 1_000_000_000;
const MAX_PAGE_STEPS: usize = 100;

#[derive(Clone, Copy, Debug, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum ProjectKind {
    Design,
    Diagram,
}

impl ProjectKind {
    pub fn label(self) -> &'static str {
        match self {
            Self::Design => "Design",
            Self::Diagram => "Diagram",
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
        Ok(())
    }
}

#[derive(Clone, Debug, PartialEq)]
pub struct ProjectStamp {
    layout: Vec<PageMeta>,
    revisions: Vec<(PageId, u64)>,
}

struct PageStep {
    layout: Vec<PageMeta>,
    active: PageId,
    order: u64,
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
        let mut session: Self = Editor::new(doc, None).into();
        session.kind = Some(kind);
        session.saved_layout = None;
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
        Ok(Self {
            kind: Some(project.kind),
            saved_layout: path.as_ref().map(|_| layout.clone()),
            layout,
            pages,
            active: project.active,
            next_page_id: project.next_page_id,
            undo_pages: Vec::new(),
            redo_pages: Vec::new(),
            last_page_edit: 0,
            history_groups: BTreeMap::new(),
        })
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
        })
    }
    pub fn stamp(&self) -> ProjectStamp {
        ProjectStamp {
            layout: self.layout.clone(),
            revisions: self
                .layout
                .iter()
                .map(|p| (p.id, self.pages[&p.id].revision))
                .collect(),
        }
    }
    pub fn is_modified(&self) -> bool {
        (self.kind.is_some() && self.saved_layout.as_ref() != Some(&self.layout))
            || self.layout.iter().any(|p| self.pages[&p.id].is_modified())
    }
    /// Mark exactly the saved revisions, allowing continued editing during IO.
    pub fn mark_project_saved(&mut self, path: PathBuf, stamp: &ProjectStamp) {
        self.saved_layout = Some(stamp.layout.clone());
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
        Ok(id)
    }
    /// Import every page as one undoable layout change, retaining page histories.
    pub fn import_pages(&mut self, mut project: Project) -> Result<Vec<PageId>, String> {
        project.validate()?;
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
        let page_ids = project.pages.iter().enumerate().map(|(offset,page)|(page.meta.id,self.next_page_id+offset as u64)).collect();
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
        self.add_page(
            doc,
            format!("{} copy", meta.name.chars().take(195).collect::<String>()),
            meta.bleed_mm,
        )
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
        self.record_pages()?;
        self.layout.remove(index);
        if self.active == id {
            self.active = self.layout[index.min(self.layout.len() - 1)].id;
        }
        self.collect_pages();
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
}
