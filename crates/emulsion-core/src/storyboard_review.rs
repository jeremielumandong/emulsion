//! Storyboard review: each panel's review status and notes, and review
//! layers, which draw on the Stage but never print.
//!
//! Every export reads panels through [`printable`], which drops review
//! layers (and anything inside a review group) from a copy of the panel;
//! documents without review layers pass through untouched.
use crate::node::{LayerColor, Node, NodeId, NodeKind};
use crate::project::PageId;
use crate::storyboard::Storyboard;
use crate::{Command, Document};
use serde::{Deserialize, Serialize};
use std::borrow::Cow;
use std::sync::Arc;

pub const MAX_NOTES: usize = 200;
pub const MAX_NOTE_CHARS: usize = 4000;
pub const MAX_AUTHOR_CHARS: usize = 100;
/// The colour label new review layers get, so they stand out in the Layers
/// panel.
pub const REVIEW_LAYER_COLOR: LayerColor = LayerColor::Violet;
/// The tint review marks use on the Board and Stage.
pub const REVIEW_RGB: u32 = 0xD6409F;

#[derive(Clone, Copy, Debug, Default, PartialEq, Eq, Hash, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum ReviewStatus {
    #[default]
    None,
    ToDo,
    InReview,
    Approved,
    NeedsChanges,
}

/// Display names and badge colours (RGB).
pub const REVIEW_STATUSES: [(ReviewStatus, &str, u32); 5] = [
    (ReviewStatus::None, "No review", 0x8B8D98),
    (ReviewStatus::ToDo, "To do", 0x0090FF),
    (ReviewStatus::InReview, "In review", 0xF76B15),
    (ReviewStatus::Approved, "Approved", 0x30A46C),
    (ReviewStatus::NeedsChanges, "Needs changes", 0xE5484D),
];

impl ReviewStatus {
    pub fn label(self) -> &'static str {
        REVIEW_STATUSES
            .iter()
            .find(|(s, ..)| *s == self)
            .map_or("", |(_, label, _)| label)
    }
    pub fn rgb(self) -> u32 {
        REVIEW_STATUSES
            .iter()
            .find(|(s, ..)| *s == self)
            .map_or(0, |(.., rgb)| *rgb)
    }
    fn is_none(&self) -> bool {
        *self == Self::None
    }
}

#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
pub struct ReviewNote {
    /// Unique within its panel.
    pub id: u64,
    pub author: String,
    /// Seconds since the Unix epoch.
    pub time: u64,
    pub text: String,
    #[serde(default, skip_serializing_if = "std::ops::Not::not")]
    pub resolved: bool,
}

/// A panel's review state. Empty on panels nobody reviewed, and then not
/// written to the file.
#[derive(Clone, Debug, Default, PartialEq, Serialize, Deserialize)]
pub struct PanelReview {
    #[serde(default, skip_serializing_if = "ReviewStatus::is_none")]
    pub status: ReviewStatus,
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    pub notes: Vec<ReviewNote>,
}

impl PanelReview {
    pub fn is_empty(&self) -> bool {
        self.status.is_none() && self.notes.is_empty()
    }
    pub fn open_notes(&self) -> impl Iterator<Item = &ReviewNote> {
        self.notes.iter().filter(|n| !n.resolved)
    }
    pub(crate) fn validate(&self) -> Result<(), String> {
        if self.notes.len() > MAX_NOTES {
            return Err(format!("A panel holds at most {MAX_NOTES} review notes."));
        }
        let mut ids = std::collections::HashSet::new();
        for note in &self.notes {
            if note.id == 0 || !ids.insert(note.id) {
                return Err("Review note IDs must be unique.".into());
            }
            check_note(&note.author, &note.text)?;
        }
        Ok(())
    }
    /// The printed Review column: the status, then each open note.
    pub fn summary(&self) -> Option<String> {
        if self.is_empty() {
            return None;
        }
        let mut out = String::new();
        if !self.status.is_none() {
            out.push_str(self.status.label());
        }
        for note in self.open_notes() {
            if !out.is_empty() {
                out.push('\n');
            }
            if note.author.is_empty() {
                out.push_str(&format!("• {}", note.text));
            } else {
                out.push_str(&format!("• {}: {}", note.author, note.text));
            }
        }
        (!out.is_empty()).then_some(out)
    }
}

fn check_note(author: &str, text: &str) -> Result<(), String> {
    if text.trim().is_empty() || text.chars().count() > MAX_NOTE_CHARS {
        return Err(format!(
            "Review notes must be 1–{MAX_NOTE_CHARS} characters."
        ));
    }
    if author.chars().count() > MAX_AUTHOR_CHARS || author.chars().any(char::is_control) {
        return Err(format!(
            "Review authors are at most {MAX_AUTHOR_CHARS} characters."
        ));
    }
    if text
        .chars()
        .any(|c| c.is_control() && !matches!(c, '\n' | '\t'))
    {
        return Err("Review notes cannot contain control characters.".into());
    }
    Ok(())
}

pub fn now() -> u64 {
    std::time::SystemTime::now()
        .duration_since(std::time::UNIX_EPOCH)
        .map_or(0, |d| d.as_secs())
}

impl Storyboard {
    fn review_mut(&mut self, panel: PageId) -> Result<&mut PanelReview, String> {
        Ok(&mut self
            .panels
            .get_mut(&panel)
            .ok_or("Panel does not exist.")?
            .review)
    }

    pub fn set_review_status(&mut self, panel: PageId, status: ReviewStatus) -> Result<(), String> {
        self.review_mut(panel)?.status = status;
        Ok(())
    }

    /// Add a note to `panel`; returns its ID.
    pub fn add_review_note(
        &mut self,
        panel: PageId,
        author: &str,
        text: &str,
        time: u64,
    ) -> Result<u64, String> {
        let (author, text) = (author.trim(), text.trim());
        check_note(author, text)?;
        let review = self.review_mut(panel)?;
        if review.notes.len() >= MAX_NOTES {
            return Err(format!("A panel holds at most {MAX_NOTES} review notes."));
        }
        let id = review.notes.iter().map(|n| n.id).max().unwrap_or(0) + 1;
        review.notes.push(ReviewNote {
            id,
            author: author.into(),
            time,
            text: text.into(),
            resolved: false,
        });
        Ok(id)
    }

    pub fn resolve_review_note(
        &mut self,
        panel: PageId,
        note: u64,
        resolved: bool,
    ) -> Result<(), String> {
        self.review_mut(panel)?
            .notes
            .iter_mut()
            .find(|n| n.id == note)
            .ok_or("That review note does not exist.")?
            .resolved = resolved;
        Ok(())
    }

    pub fn remove_review_note(&mut self, panel: PageId, note: u64) -> Result<(), String> {
        let review = self.review_mut(panel)?;
        let before = review.notes.len();
        review.notes.retain(|n| n.id != note);
        if review.notes.len() == before {
            return Err("That review note does not exist.".into());
        }
        Ok(())
    }
}

/// Whether `id` is a review layer or sits inside one.
pub fn is_review(doc: &Document, id: NodeId) -> bool {
    let mut at = doc.node(id);
    while let Some(node) = at {
        if node.review {
            return true;
        }
        at = node.parent.and_then(|p| doc.node(p));
    }
    false
}

pub fn has_review_layers(doc: &Document) -> bool {
    doc.nodes.iter().any(|n| n.review)
}

/// The document every export draws: `doc` without its review layers. The
/// one place exports leave review layers out, so PDF sheets, panel images,
/// movies, GIFs, layered exports and (optionally) thumbnails agree.
pub fn printable(doc: &Document) -> Cow<'_, Document> {
    if !has_review_layers(doc) {
        return Cow::Borrowed(doc);
    }
    let mut out = doc.clone();
    // Outermost review layers first; their subtrees go with them.
    let tops: Vec<NodeId> = doc
        .nodes
        .iter()
        .filter(|n| n.review && !n.parent.is_some_and(|p| is_review(doc, p)))
        .map(|n| n.id)
        .collect();
    for id in tops {
        // Removal cannot fail for a node that exists; locks do not apply here.
        let _ = Command::RemoveNode { id }.apply(&mut out);
    }
    Cow::Owned(out)
}

/// A new, empty review layer the size of `doc`, named `name`.
pub fn review_layer(doc: &Document, name: &str) -> Node {
    let mut node = Node::new(
        0,
        name,
        NodeKind::Raster {
            raster: Arc::new(emulsion_raster::Raster::transparent(doc.width, doc.height)),
            placement: Default::default(),
        },
    );
    node.review = true;
    node.color_label = REVIEW_LAYER_COLOR;
    node
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::command::Slot;

    pub(crate) fn doc_with_review() -> (Document, NodeId, NodeId) {
        let mut doc = Document::new(32, 18);
        let ink = Command::AddNode {
            node: Box::new(Node::new(
                0,
                "Ink",
                NodeKind::Fill {
                    rgba: [0, 0, 0, 255],
                },
            )),
            slot: Slot::TOP,
        }
        .apply(&mut doc)
        .unwrap()
        .unwrap();
        let review = Command::AddNode {
            node: Box::new(review_layer(&doc, "Review")),
            slot: Slot::TOP,
        }
        .apply(&mut doc)
        .unwrap()
        .unwrap();
        (doc, ink, review)
    }

    #[test]
    fn printable_drops_review_layers_and_borrows_clean_documents() {
        let (doc, ink, review) = doc_with_review();
        assert!(is_review(&doc, review) && !is_review(&doc, ink));
        let print = printable(&doc);
        assert!(matches!(print, Cow::Owned(_)));
        assert!(print.node(review).is_none() && print.node(ink).is_some());
        assert!(matches!(printable(&print), Cow::Borrowed(_)));
    }

    #[test]
    fn notes_validate_and_summarise_open_notes() {
        let mut review = PanelReview::default();
        assert!(review.is_empty() && review.summary().is_none());
        review.status = ReviewStatus::NeedsChanges;
        review.notes.push(ReviewNote {
            id: 1,
            author: "Ana".into(),
            time: 1,
            text: "Bigger".into(),
            resolved: false,
        });
        review.notes.push(ReviewNote {
            id: 2,
            author: String::new(),
            time: 2,
            text: "Done".into(),
            resolved: true,
        });
        review.validate().unwrap();
        assert_eq!(review.summary().unwrap(), "Needs changes\n• Ana: Bigger");
        review.notes[1].id = 1;
        assert!(review.validate().is_err());
    }
}
