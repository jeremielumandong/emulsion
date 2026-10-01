//! The storyboard library: reusable drawings such as characters, props and
//! backgrounds. A **layer item** holds one or more layers taken from a panel,
//! at their positions in the frame; placing it puts copies on top of the
//! active panel. A **panel item** holds a whole panel; placing it adds a new
//! panel after the active one.
//!
//! The project library lives on the `Storyboard`, so it travels with the
//! `.emu` file and its changes share project Undo like every other storyboard
//! edit. The personal library, shared by every storyboard, is kept on disk by
//! `emulsion-io` and holds the same two kinds of item.
use crate::command::Slot;
use crate::project::{MAX_PROJECT_PIXELS, PageId, ProjectEditor};
use crate::{Document, Editor, NodeId, fragment::Fragment};
use serde::{Deserialize, Serialize};
use std::collections::HashSet;
use std::sync::Arc;

pub type ItemId = u64;

/// Most items one project library holds.
pub const MAX_ITEMS: usize = 500;
/// Most tags on one item.
pub const MAX_TAGS: usize = 50;

#[derive(Clone, Copy, Debug, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum ItemKind {
    /// Layers placed on top of the active panel.
    Layers,
    /// A whole panel, placed as a new panel.
    Panel,
}

impl ItemKind {
    pub fn label(self) -> &'static str {
        match self {
            Self::Layers => "Layers",
            Self::Panel => "Panel",
        }
    }
}

/// One item of a project library. The drawing is stored in the `.emu`
/// package next to the storyboard data, not in its JSON.
#[derive(Clone, Debug, Serialize, Deserialize)]
pub struct LibraryItem {
    pub id: ItemId,
    pub name: String,
    #[serde(default)]
    pub tags: Vec<String>,
    pub kind: ItemKind,
    #[serde(skip, default = "unloaded")]
    pub doc: Arc<Document>,
}

/// Stands in for a drawing until the package reader fills it in; it never
/// validates, so a drawing missing from a package is an error.
fn unloaded() -> Arc<Document> {
    Arc::new(Document::new(0, 0))
}

impl PartialEq for LibraryItem {
    fn eq(&self, other: &Self) -> bool {
        self.id == other.id
            && self.name == other.name
            && self.tags == other.tags
            && self.kind == other.kind
            && (Arc::ptr_eq(&self.doc, &other.doc) || self.doc == other.doc)
    }
}

impl LibraryItem {
    pub fn matches(&self, query: &str) -> bool {
        matches(&self.name, &self.tags, query)
    }
}

/// Whether every word of `query` appears in the name or a tag, ignoring case.
/// Shared by both libraries' search.
pub fn matches(name: &str, tags: &[String], query: &str) -> bool {
    let name = name.to_lowercase();
    let tags: Vec<_> = tags.iter().map(|t| t.to_lowercase()).collect();
    query.split_whitespace().all(|word| {
        let word = word.to_lowercase();
        name.contains(&word) || tags.iter().any(|t| t.contains(&word))
    })
}

/// Item names are 1–200 characters without control characters.
pub fn check_name(name: &str) -> Result<(), String> {
    crate::storyboard::check_name(name, "Library item")
}

/// Trimmed, de-duplicated tags, checked against the library limits.
pub fn clean_tags(tags: &[String]) -> Result<Vec<String>, String> {
    let mut seen = HashSet::new();
    let tags: Vec<String> = tags
        .iter()
        .map(|t| t.trim().to_string())
        .filter(|t| !t.is_empty() && seen.insert(t.to_lowercase()))
        .collect();
    if tags.len() > MAX_TAGS
        || tags
            .iter()
            .any(|t| t.chars().count() > 200 || t.chars().any(char::is_control))
    {
        return Err(format!(
            "Use at most {MAX_TAGS} tags of 1–200 characters each."
        ));
    }
    Ok(tags)
}

/// Copies of `ids` (and everything inside them) in a transparent document of
/// `doc`'s size, at their positions in the frame.
pub fn capture_layers(doc: &Document, ids: &[NodeId]) -> Result<Document, String> {
    let fragment = Fragment::capture(doc, ids)?;
    let mut blank = Document::new(doc.width, doc.height);
    blank.resolution = doc.resolution;
    let mut editor = Editor::new(blank, None);
    fragment.paste(&mut editor, Slot::TOP, (0., 0.))?;
    Ok(editor.doc)
}

#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
pub struct Library {
    pub items: Vec<LibraryItem>,
    pub next_id: ItemId,
}

impl Default for Library {
    fn default() -> Self {
        Self {
            items: Vec::new(),
            next_id: 1,
        }
    }
}

impl Library {
    pub fn is_empty(&self) -> bool {
        self.items.is_empty()
    }

    pub fn item(&self, id: ItemId) -> Option<&LibraryItem> {
        self.items.iter().find(|i| i.id == id)
    }

    /// Add a drawing; returns its ID.
    pub fn add(
        &mut self,
        name: &str,
        tags: &[String],
        kind: ItemKind,
        doc: Document,
    ) -> Result<ItemId, String> {
        if self.items.len() >= MAX_ITEMS {
            return Err(format!("A library holds at most {MAX_ITEMS} items."));
        }
        let id = self.next_id;
        self.items.push(LibraryItem {
            id,
            name: name.trim().into(),
            tags: clean_tags(tags)?,
            kind,
            doc: Arc::new(doc),
        });
        self.next_id += 1;
        Ok(id)
    }

    /// Rename an item and, when given, replace its tags.
    pub fn rename(
        &mut self,
        id: ItemId,
        name: &str,
        tags: Option<&[String]>,
    ) -> Result<(), String> {
        let tags = tags.map(clean_tags).transpose()?;
        let item = self
            .items
            .iter_mut()
            .find(|i| i.id == id)
            .ok_or("No library item has that ID.")?;
        item.name = name.trim().into();
        if let Some(tags) = tags {
            item.tags = tags;
        }
        Ok(())
    }

    pub fn remove(&mut self, id: ItemId) -> Result<LibraryItem, String> {
        let at = self
            .items
            .iter()
            .position(|i| i.id == id)
            .ok_or("No library item has that ID.")?;
        Ok(self.items.remove(at))
    }

    pub fn validate(&self) -> Result<(), String> {
        if self.items.len() > MAX_ITEMS {
            return Err(format!("A library holds at most {MAX_ITEMS} items."));
        }
        let mut ids = HashSet::new();
        let mut pixels = 0u64;
        for item in &self.items {
            if item.id == 0 || item.id >= self.next_id || !ids.insert(item.id) {
                return Err("Library item IDs must be unique and allocated.".into());
            }
            check_name(&item.name)?;
            if clean_tags(&item.tags)? != item.tags {
                return Err("Library tags must be trimmed and unique.".into());
            }
            item.doc.validate().map_err(|e| e.to_string())?;
            if item.doc.width == 0 || item.doc.height == 0 {
                return Err(format!("Library item {} has no drawing.", item.name));
            }
            if !item.doc.nodes.iter().any(|n| n.parent.is_none()) {
                return Err(format!("Library item {} has no layers.", item.name));
            }
            pixels += u64::from(item.doc.width) * u64::from(item.doc.height);
        }
        if pixels > MAX_PROJECT_PIXELS || self.next_id == u64::MAX {
            return Err("The library exceeds its size limit.".into());
        }
        Ok(())
    }
}

/// What placing a library drawing made.
#[derive(Clone, Debug, PartialEq, Eq)]
pub enum Placed {
    /// New top-level layers on the active panel.
    Layers(Vec<NodeId>),
    /// A new panel, now active.
    Panel(PageId),
}

impl ProjectEditor {
    /// Add layers of a panel to the project library, one Undo step.
    pub fn add_library_layers(
        &mut self,
        panel: PageId,
        layers: &[NodeId],
        name: &str,
        tags: &[String],
    ) -> Result<ItemId, String> {
        let page = self.page(panel).ok_or("Panel does not exist.")?;
        let doc = capture_layers(&page.doc, layers)?;
        self.add_library_item(name, tags, ItemKind::Layers, doc)
    }

    /// Add a whole panel to the project library, one Undo step.
    pub fn add_library_panel(
        &mut self,
        panel: PageId,
        name: &str,
        tags: &[String],
    ) -> Result<ItemId, String> {
        let doc = self.page(panel).ok_or("Panel does not exist.")?.doc.clone();
        self.add_library_item(name, tags, ItemKind::Panel, doc)
    }

    /// Add a drawing to the project library, one Undo step.
    pub fn add_library_item(
        &mut self,
        name: &str,
        tags: &[String],
        kind: ItemKind,
        mut doc: Document,
    ) -> Result<ItemId, String> {
        // A drawing is not a selection.
        doc.selection = None;
        let mut id = 0;
        self.edit_storyboard(|b| {
            id = b.library.add(name, tags, kind, doc)?;
            Ok(())
        })?;
        Ok(id)
    }

    /// Place a drawing from either library: layers go on top of the active
    /// panel at their positions in the frame; a panel becomes a new panel
    /// after the active one. Drawings at another resolution are fitted to the
    /// frame. One Undo step.
    pub fn place_drawing(&mut self, kind: ItemKind, doc: &Document) -> Result<Placed, String> {
        match kind {
            ItemKind::Layers => self.place_layers(doc).map(Placed::Layers),
            ItemKind::Panel => {
                let after = self.active_page();
                let name = self.next_panel_name(after);
                let ids = self.import_panels(Some(after), vec![(name, doc.clone())])?;
                Ok(Placed::Panel(ids[0]))
            }
        }
    }

    /// Place a project library item; see `place_drawing`.
    pub fn place_library_item(&mut self, id: ItemId) -> Result<Placed, String> {
        let item = self
            .storyboard()
            .ok_or("This is not a storyboard project.")?
            .library
            .item(id)
            .ok_or("No library item has that ID.")?
            .clone();
        self.place_drawing(item.kind, &item.doc)
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::creation::{CanvasKind, CanvasSpec};
    use crate::{Command, Node, NodeKind};

    fn board() -> ProjectEditor {
        let mut p = CanvasSpec {
            name: "Board".into(),
            kind: CanvasKind::Storyboard,
            width: 64.,
            height: 36.,
            pages: 2,
            ..Default::default()
        }
        .create_project()
        .unwrap();
        p.set_active_page(1).unwrap();
        p.execute(Command::AddNode {
            node: Box::new(Node::new(
                0,
                "Hero",
                NodeKind::Fill {
                    rgba: [200, 40, 40, 255],
                },
            )),
            slot: Slot::TOP,
        })
        .unwrap();
        p
    }

    fn hero(p: &ProjectEditor) -> NodeId {
        p.doc.nodes.iter().find(|n| n.name == "Hero").unwrap().id
    }

    #[test]
    fn layer_items_place_on_the_active_panel_as_one_undo_step() {
        let mut p = board();
        let id = p
            .add_library_layers(1, &[hero(&p)], " Hero ", &["character".into()])
            .unwrap();
        let item = p.storyboard().unwrap().library.item(id).unwrap().clone();
        assert_eq!(item.name, "Hero");
        assert_eq!(item.kind, ItemKind::Layers);
        assert_eq!(item.doc.nodes.len(), 1);
        assert!(item.matches("CHAR hero"));
        assert!(!item.matches("prop"));
        // Adding is itself one step.
        assert!(p.undo());
        assert!(p.storyboard().unwrap().library.is_empty());
        assert!(p.redo());

        p.set_active_page(2).unwrap();
        let before = p.doc.nodes.len();
        let Placed::Layers(new) = p.place_library_item(id).unwrap() else {
            panic!()
        };
        assert_eq!(new.len(), 1);
        assert_eq!(p.doc.nodes.len(), before + 1);
        assert_eq!(p.doc.nodes.last().unwrap().name, "Hero");
        assert!(p.undo());
        assert_eq!(p.doc.nodes.len(), before);
        // The library keeps its item through the placement's undo.
        assert!(p.storyboard().unwrap().library.item(id).is_some());
    }

    #[test]
    fn panel_items_become_a_panel_after_the_active_one() {
        let mut p = board();
        let id = p.add_library_panel(1, "Castle", &[]).unwrap();
        p.set_active_page(1).unwrap();
        let Placed::Panel(page) = p.place_library_item(id).unwrap() else {
            panic!()
        };
        let order: Vec<_> = p.page_list().iter().map(|m| m.id).collect();
        assert_eq!(order, [1, page, 2]);
        assert_eq!(p.active_page(), page);
        assert!(p.doc.nodes.iter().any(|n| n.name == "Hero"));
        p.snapshot().unwrap().validate().unwrap();
        assert!(p.undo());
        assert_eq!(p.page_list().len(), 2);
    }

    #[test]
    fn library_edits_are_validated_and_undoable() {
        let mut p = board();
        let id = p.add_library_panel(1, "Castle", &[]).unwrap();
        let stamp = p.stamp();
        assert!(p.add_library_layers(1, &[], "x", &[]).is_err());
        assert!(p.add_library_panel(1, " ", &[]).is_err());
        assert!(p.add_library_panel(9, "x", &[]).is_err());
        assert!(
            p.edit_storyboard(|b| b.library.rename(99, "x", None))
                .is_err()
        );
        assert!(
            p.edit_storyboard(|b| b.library.rename(id, "", None))
                .is_err()
        );
        assert_eq!(p.stamp(), stamp);
        p.edit_storyboard(|b| {
            b.library
                .rename(id, "Keep", Some(&["set".into(), " set ".into()]))
        })
        .unwrap();
        assert_eq!(p.storyboard().unwrap().library.items[0].tags, ["set"]);
        p.edit_storyboard(|b| b.library.remove(id).map(|_| ()))
            .unwrap();
        assert!(p.storyboard().unwrap().library.is_empty());
        assert!(p.undo());
        assert_eq!(p.storyboard().unwrap().library.items[0].name, "Keep");
        assert!(p.is_modified());
    }

    #[test]
    fn locked_panels_refuse_placed_layers() {
        let mut p = board();
        let id = p.add_library_layers(1, &[hero(&p)], "Hero", &[]).unwrap();
        p.edit_storyboard(|b| {
            b.panels.get_mut(&2).unwrap().locked = true;
            Ok(())
        })
        .unwrap();
        p.set_active_page(2).unwrap();
        let stamp = p.stamp();
        assert!(p.place_library_item(id).is_err());
        assert!(p.place_library_item(99).is_err());
        assert_eq!(p.stamp(), stamp);
    }
}
