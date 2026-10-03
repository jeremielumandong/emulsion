//! Change tracking and Compare: how a storyboard differs from an earlier
//! state of itself, panel by panel.
//!
//! Panels are matched by panel ID, then unmatched ones by name (a panel cut
//! and pasted back gets a new ID but keeps its name). Each matched pair is
//! compared part by part with content fingerprints
//! ([`crate::storyboard_fingerprint`]), so a change says what changed: the
//! drawing, captions, timing, camera, layer keys, shot details, name or
//! review. A panel is *moved* when its place among the panels both states
//! share changed: inserting or deleting panels moves nothing else.
use crate::Document;
use crate::project::PageId;
use crate::storyboard::{Panel, Storyboard};
use crate::storyboard_fingerprint::{data_fingerprint, document_fingerprint};
use crate::storyboard_versions::BoardState;
use serde::Serialize;
use std::collections::{BTreeMap, HashMap, HashSet};

/// What about a panel changed.
#[derive(Clone, Copy, Debug, PartialEq, Eq, PartialOrd, Ord, Hash, Serialize)]
#[serde(rename_all = "snake_case")]
pub enum Aspect {
    Drawing,
    Caption,
    Timing,
    Camera,
    LayerKeys,
    Details,
    Name,
    Review,
}

pub const ASPECTS: [(Aspect, &str); 8] = [
    (Aspect::Drawing, "drawing"),
    (Aspect::Caption, "captions"),
    (Aspect::Timing, "timing"),
    (Aspect::Camera, "camera"),
    (Aspect::LayerKeys, "layer keys"),
    (Aspect::Details, "shot details"),
    (Aspect::Name, "name"),
    (Aspect::Review, "review"),
];

impl Aspect {
    pub fn label(self) -> &'static str {
        ASPECTS
            .iter()
            .find(|(a, _)| *a == self)
            .map_or("", |(_, l)| l)
    }
}

#[derive(Clone, Copy, Debug, PartialEq, Eq, Hash, Serialize)]
#[serde(rename_all = "snake_case")]
pub enum ChangeKind {
    New,
    Changed,
    Moved,
    Deleted,
    Unchanged,
}

impl ChangeKind {
    pub fn label(self) -> &'static str {
        match self {
            Self::New => "New",
            Self::Changed => "Changed",
            Self::Moved => "Moved",
            Self::Deleted => "Deleted",
            Self::Unchanged => "Unchanged",
        }
    }
}

/// One panel of either state, in aligned order.
#[derive(Clone, Debug, PartialEq, Serialize)]
pub struct PanelChange {
    /// The panel in the older state; None for a new panel.
    pub old: Option<PageId>,
    /// The panel now; None for a deleted panel.
    pub new: Option<PageId>,
    /// Its name now, or then for a deleted panel.
    pub name: String,
    /// New, Deleted, or Changed when any aspect changed (then `moved` may
    /// also be set), else Moved or Unchanged.
    pub kind: ChangeKind,
    pub moved: bool,
    /// What changed, for Changed panels.
    pub aspects: Vec<Aspect>,
}

impl PanelChange {
    pub fn is_change(&self) -> bool {
        self.kind != ChangeKind::Unchanged
    }
    /// The panel this row is about: the current one, or the deleted one.
    pub fn panel(&self) -> PageId {
        self.new.or(self.old).unwrap_or_default()
    }
    /// “Changed: drawing, captions · moved”.
    pub fn summary(&self) -> String {
        let mut out = self.kind.label().to_string();
        if !self.aspects.is_empty() {
            let parts: Vec<_> = self.aspects.iter().map(|a| a.label()).collect();
            out.push_str(": ");
            out.push_str(&parts.join(", "));
        }
        if self.moved && self.kind == ChangeKind::Changed {
            out.push_str(" · moved");
        }
        out
    }
}

/// Pair the panels of `old` and `new`: by ID, then unmatched ones by name.
/// Rows follow the new order; a deleted panel follows the panel it came
/// after in the old order.
pub fn align(old: &BoardState, new: &BoardState) -> Vec<(Option<PageId>, Option<PageId>)> {
    let old_ids: HashSet<_> = old.layout.iter().map(|m| m.id).collect();
    let new_ids: HashSet<_> = new.layout.iter().map(|m| m.id).collect();
    // new → old
    let mut pairs: HashMap<PageId, PageId> = new
        .layout
        .iter()
        .filter(|m| old_ids.contains(&m.id))
        .map(|m| (m.id, m.id))
        .collect();
    let mut by_name: HashMap<&str, Vec<PageId>> = HashMap::new();
    for m in old.layout.iter().rev().filter(|m| !new_ids.contains(&m.id)) {
        by_name.entry(m.name.trim()).or_default().push(m.id);
    }
    for m in new.layout.iter().filter(|m| !old_ids.contains(&m.id)) {
        if let Some(id) = by_name.get_mut(m.name.trim()).and_then(Vec::pop) {
            pairs.insert(m.id, id);
        }
    }
    let matched_old: HashMap<PageId, PageId> = pairs.iter().map(|(n, o)| (*o, *n)).collect();
    // Deleted panels, keyed by the matched old panel before them.
    let mut after: BTreeMap<Option<PageId>, Vec<PageId>> = BTreeMap::new();
    let mut anchor = None;
    for m in &old.layout {
        match matched_old.get(&m.id) {
            Some(n) => anchor = Some(*n),
            None => after.entry(anchor).or_default().push(m.id),
        }
    }
    let mut rows: Vec<_> = after
        .remove(&None)
        .unwrap_or_default()
        .into_iter()
        .map(|o| (Some(o), None))
        .collect();
    for m in &new.layout {
        rows.push((pairs.get(&m.id).copied(), Some(m.id)));
        if let Some(deleted) = after.remove(&Some(m.id)) {
            rows.extend(deleted.into_iter().map(|o| (Some(o), None)));
        }
    }
    rows
}

/// Positions (in `seq`) of a longest strictly increasing subsequence.
fn increasing(seq: &[usize]) -> HashSet<usize> {
    let mut tails: Vec<usize> = Vec::new(); // index into seq
    let mut prev = vec![usize::MAX; seq.len()];
    for (i, &v) in seq.iter().enumerate() {
        let at = tails.partition_point(|&t| seq[t] < v);
        if at > 0 {
            prev[i] = tails[at - 1];
        }
        if at == tails.len() {
            tails.push(i);
        } else {
            tails[at] = i;
        }
    }
    let mut keep = HashSet::new();
    let mut at = tails.last().copied();
    while let Some(i) = at {
        keep.insert(i);
        at = (prev[i] != usize::MAX).then_some(prev[i]);
    }
    keep
}

/// Fingerprints of one panel's parts, other than its drawing.
#[derive(PartialEq)]
struct Parts {
    caption: String,
    timing: String,
    camera: String,
    layer_keys: String,
    details: String,
    name: String,
    review: String,
}

fn parts(state: &BoardState, id: PageId) -> Option<Parts> {
    let board = &state.board;
    let panel = board.panels.get(&id)?;
    let layout = state.order();
    // Captions by field name, so boards whose field IDs differ compare.
    let captions: BTreeMap<&str, _> = board
        .captions
        .iter()
        .filter_map(|f| Some((f.name.as_str(), panel.captions.get(&f.id)?)))
        .collect();
    let mut camera = crate::storyboard_library::panel_camera(board, &layout, id);
    camera.shake = board.cameras.get(&panel.scene).and_then(|c| c.shake);
    Some(Parts {
        caption: data_fingerprint(&captions),
        timing: data_fingerprint(&(panel.frames, &panel.transition, &panel.thumbnails)),
        camera: data_fingerprint(&camera),
        layer_keys: data_fingerprint(&(&panel.motion, &panel.comps)),
        details: data_fingerprint(&(panel.size, panel.angle, panel.status, panel.tag)),
        name: data_fingerprint(&state.name(id).unwrap_or_default().trim()),
        review: data_fingerprint(&panel.review),
    })
}

/// Whether the drawing changed, and whether only review layers did.
/// Unknown (None) when either drawing is missing.
fn drawing_change(old: Option<&Document>, new: Option<&Document>) -> Option<(bool, bool)> {
    let (old, new) = (old?, new?);
    // Shared buffers compare by identity: the usual case costs nothing.
    if old == new {
        return Some((false, false));
    }
    let printed = |d: &Document| document_fingerprint(&crate::storyboard_review::printable(d));
    if printed(old) != printed(new) {
        return Some((true, false));
    }
    Some((
        false,
        document_fingerprint(old) != document_fingerprint(new),
    ))
}

fn aspects(old: &BoardState, o: PageId, new: &BoardState, n: PageId) -> Vec<Aspect> {
    let mut out = Vec::new();
    if let Some((drawing, review)) = drawing_change(old.doc(o), new.doc(n)) {
        if drawing {
            out.push(Aspect::Drawing);
        }
        if review {
            out.push(Aspect::Review);
        }
    }
    if let (Some(a), Some(b)) = (parts(old, o), parts(new, n)) {
        for (aspect, x, y) in [
            (Aspect::Caption, &a.caption, &b.caption),
            (Aspect::Timing, &a.timing, &b.timing),
            (Aspect::Camera, &a.camera, &b.camera),
            (Aspect::LayerKeys, &a.layer_keys, &b.layer_keys),
            (Aspect::Details, &a.details, &b.details),
            (Aspect::Name, &a.name, &b.name),
            (Aspect::Review, &a.review, &b.review),
        ] {
            if x != y && !out.contains(&aspect) {
                out.push(aspect);
            }
        }
    }
    out.sort();
    out
}

/// Every panel of `old` and `new`, aligned, with how it changed.
pub fn describe_changes(old: &BoardState, new: &BoardState) -> Vec<PanelChange> {
    let rows = align(old, new);
    let old_index: HashMap<PageId, usize> = old
        .layout
        .iter()
        .enumerate()
        .map(|(i, m)| (m.id, i))
        .collect();
    let matched: Vec<(usize, usize)> = rows
        .iter()
        .enumerate()
        .filter_map(|(row, (o, n))| {
            n.as_ref()?;
            Some((row, old_index[&(*o)?]))
        })
        .collect();
    let keep = increasing(&matched.iter().map(|(_, i)| *i).collect::<Vec<_>>());
    let moved: HashSet<usize> = matched
        .iter()
        .enumerate()
        .filter(|(i, _)| !keep.contains(i))
        .map(|(_, (row, _))| *row)
        .collect();
    rows.iter()
        .enumerate()
        .map(|(row, &(o, n))| {
            let name = n
                .and_then(|n| new.name(n))
                .or_else(|| o.and_then(|o| old.name(o)))
                .unwrap_or_default()
                .to_string();
            let (kind, aspects, moved) = match (o, n) {
                (None, _) => (ChangeKind::New, Vec::new(), false),
                (_, None) => (ChangeKind::Deleted, Vec::new(), false),
                (Some(o), Some(n)) => {
                    let aspects = aspects(old, o, new, n);
                    let moved = moved.contains(&row);
                    let kind = if !aspects.is_empty() {
                        ChangeKind::Changed
                    } else if moved {
                        ChangeKind::Moved
                    } else {
                        ChangeKind::Unchanged
                    };
                    (kind, aspects, moved)
                }
            };
            PanelChange {
                old: o,
                new: n,
                name,
                kind,
                moved,
                aspects,
            }
        })
        .collect()
}

#[derive(Clone, Copy, Debug, PartialEq, Eq, Serialize)]
#[serde(rename_all = "snake_case")]
pub enum DiffOp {
    Same,
    Added,
    Removed,
}

/// Words (with the spaces after them) that `new` added to or removed from
/// `old`, in reading order. Long texts fall back to whole-text replacement.
pub fn word_diff(old: &str, new: &str) -> Vec<(DiffOp, String)> {
    fn words(s: &str) -> Vec<&str> {
        let mut out = Vec::new();
        let mut start = 0;
        let mut in_space = false;
        for (i, c) in s.char_indices() {
            let space = c.is_whitespace();
            if i > start && in_space && !space {
                out.push(&s[start..i]);
                start = i;
            }
            in_space = space;
        }
        if start < s.len() {
            out.push(&s[start..]);
        }
        out
    }
    let (a, b) = (words(old), words(new));
    let mut out: Vec<(DiffOp, String)> = Vec::new();
    let mut push = |op: DiffOp, word: &str| match out.last_mut() {
        Some((last, text)) if *last == op => text.push_str(word),
        _ => out.push((op, word.to_string())),
    };
    if a.len().saturating_mul(b.len()) > 4_000_000 {
        if old != new {
            push(DiffOp::Removed, old);
            push(DiffOp::Added, new);
        } else {
            push(DiffOp::Same, old);
        }
        return out;
    }
    // Longest common subsequence, filled from the end.
    let w = b.len() + 1;
    let mut lcs = vec![0u32; (a.len() + 1) * w];
    for i in (0..a.len()).rev() {
        for j in (0..b.len()).rev() {
            lcs[i * w + j] = if a[i].trim_end() == b[j].trim_end() {
                lcs[(i + 1) * w + j + 1] + 1
            } else {
                lcs[(i + 1) * w + j].max(lcs[i * w + j + 1])
            };
        }
    }
    let (mut i, mut j) = (0, 0);
    while i < a.len() || j < b.len() {
        if i < a.len() && j < b.len() && a[i].trim_end() == b[j].trim_end() {
            push(DiffOp::Same, b[j]);
            i += 1;
            j += 1;
        } else if i < a.len() && (j == b.len() || lcs[(i + 1) * w + j] >= lcs[i * w + j + 1]) {
            push(DiffOp::Removed, a[i]);
            i += 1;
        } else {
            push(DiffOp::Added, b[j]);
            j += 1;
        }
    }
    out
}

/// One caption field of a compared pair.
#[derive(Clone, Debug, PartialEq, Serialize)]
pub struct CaptionDiff {
    pub field: String,
    pub old: String,
    pub new: String,
    pub words: Vec<(DiffOp, String)>,
}

/// A Compare row: the change, the caption fields that differ, and both
/// durations.
#[derive(Clone, Debug, PartialEq, Serialize)]
pub struct CompareRow {
    pub change: PanelChange,
    pub captions: Vec<CaptionDiff>,
    pub old_frames: Option<u32>,
    pub new_frames: Option<u32>,
}

fn caption_text(board: &Storyboard, panel: &Panel, field: &str) -> String {
    board
        .captions
        .iter()
        .find(|f| f.name == field)
        .and_then(|f| panel.captions.get(&f.id))
        .map(|c| c.text.clone())
        .unwrap_or_default()
}

/// Two states panel by panel, for the Compare view.
pub fn compare(old: &BoardState, new: &BoardState) -> Vec<CompareRow> {
    let mut fields: Vec<String> = new.board.captions.iter().map(|f| f.name.clone()).collect();
    for f in &old.board.captions {
        if !fields.contains(&f.name) {
            fields.push(f.name.clone());
        }
    }
    describe_changes(old, new)
        .into_iter()
        .map(|change| {
            let o = change.old.and_then(|id| old.board.panels.get(&id));
            let n = change.new.and_then(|id| new.board.panels.get(&id));
            let captions = fields
                .iter()
                .filter_map(|field| {
                    let a = o
                        .map(|p| caption_text(&old.board, p, field))
                        .unwrap_or_default();
                    let b = n
                        .map(|p| caption_text(&new.board, p, field))
                        .unwrap_or_default();
                    (a != b).then(|| CaptionDiff {
                        field: field.clone(),
                        words: word_diff(&a, &b),
                        old: a,
                        new: b,
                    })
                })
                .collect();
            CompareRow {
                old_frames: o.map(|p| p.frames),
                new_frames: n.map(|p| p.frames),
                change,
                captions,
            }
        })
        .collect()
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::project::{ProjectEditor, ProjectKind};
    use crate::storyboard_versions::Baseline;
    use crate::{Command, Node, NodeKind, command::Slot};

    fn board(panels: usize) -> ProjectEditor {
        let mut p =
            ProjectEditor::new_project(ProjectKind::Storyboard, Document::new(16, 9)).unwrap();
        let blank = p.storyboard().unwrap().blank_panel().unwrap();
        let more = (1..panels)
            .map(|i| (format!("P{}", i + 1), crate::storyboard::Panel::new(0, 24)))
            .collect();
        if panels > 1 {
            p.insert_panels(Some(1), &blank, more, None).unwrap();
        }
        p
    }

    fn ids(p: &ProjectEditor) -> Vec<PageId> {
        p.page_list().iter().map(|m| m.id).collect()
    }

    #[test]
    fn panels_are_new_changed_deleted_or_moved_against_a_version() {
        let mut p = board(6);
        let id = ids(&p);
        let v = p.create_board_version("Pass 1").unwrap();
        // Draw on 2, caption 3, retime 4, delete 5, move 6 to the front, add one.
        p.set_active_page(id[1]).unwrap();
        p.execute(Command::AddNode {
            node: Box::new(Node::new(0, "Ink", NodeKind::Fill { rgba: [0; 4] })),
            slot: Slot::TOP,
        })
        .unwrap();
        let action = p.storyboard().unwrap().caption("Action").unwrap();
        p.edit_storyboard(|b| {
            b.panels
                .get_mut(&id[2])
                .unwrap()
                .captions
                .insert(action, "Runs".into());
            b.panels.get_mut(&id[3]).unwrap().frames = 10;
            Ok(())
        })
        .unwrap();
        p.remove_page(id[4]).unwrap();
        p.move_page(id[5], 0).unwrap();
        let blank = p.storyboard().unwrap().blank_panel().unwrap();
        let added = p
            .insert_panels(
                Some(id[0]),
                &blank,
                vec![("New".into(), crate::storyboard::Panel::new(0, 24))],
                None,
            )
            .unwrap()[0];
        let old = p.board_state(Baseline::Version(v)).unwrap();
        let now = p.current_board_state().unwrap();
        let changes = describe_changes(&old, &now);
        let of = |panel: PageId| changes.iter().find(|c| c.panel() == panel).unwrap();
        assert_eq!(of(id[0]).kind, ChangeKind::Unchanged);
        assert_eq!(of(id[1]).aspects, vec![Aspect::Drawing]);
        assert_eq!(of(id[2]).aspects, vec![Aspect::Caption]);
        assert_eq!(of(id[3]).aspects, vec![Aspect::Timing]);
        assert_eq!(of(id[4]).kind, ChangeKind::Deleted);
        assert_eq!(of(id[5]).kind, ChangeKind::Moved);
        assert_eq!(of(added).kind, ChangeKind::New);
        // The deleted panel sits after the panel it followed.
        let rows: Vec<_> = changes.iter().map(|c| c.panel()).collect();
        let at = |id| rows.iter().position(|r| *r == id).unwrap();
        assert_eq!(at(id[4]), at(id[3]) + 1);
        assert_eq!(changes.iter().filter(|c| c.is_change()).count(), 6);
        // Against itself nothing changed.
        assert!(describe_changes(&now, &now).iter().all(|c| !c.is_change()));
    }

    #[test]
    fn compare_aligns_by_id_then_name_and_diffs_caption_words() {
        let mut p = board(3);
        let id = ids(&p);
        let action = p.storyboard().unwrap().caption("Action").unwrap();
        p.edit_storyboard(|b| {
            b.panels
                .get_mut(&id[1])
                .unwrap()
                .captions
                .insert(action, "Mia runs home".into());
            Ok(())
        })
        .unwrap();
        let v = p.create_board_version("Before").unwrap();
        // Remove P3 and add a new panel with the same name: matched by name.
        p.remove_page(id[2]).unwrap();
        let blank = p.storyboard().unwrap().blank_panel().unwrap();
        let again = p
            .insert_panels(
                Some(id[1]),
                &blank,
                vec![("P3".into(), crate::storyboard::Panel::new(0, 30))],
                None,
            )
            .unwrap()[0];
        p.edit_storyboard(|b| {
            b.panels
                .get_mut(&id[1])
                .unwrap()
                .captions
                .insert(action, "Mia walks home".into());
            Ok(())
        })
        .unwrap();
        let rows = compare(
            &p.board_state(Baseline::Version(v)).unwrap(),
            &p.current_board_state().unwrap(),
        );
        assert_eq!(rows.len(), 3);
        assert_eq!(
            (rows[2].change.old, rows[2].change.new),
            (Some(id[2]), Some(again))
        );
        assert_eq!(
            (rows[2].old_frames, rows[2].new_frames),
            (Some(24), Some(30))
        );
        let caption = &rows[1].captions[0];
        assert_eq!(caption.field, "Action");
        assert_eq!(
            caption.words,
            vec![
                (DiffOp::Same, "Mia ".into()),
                (DiffOp::Removed, "runs ".into()),
                (DiffOp::Added, "walks ".into()),
                (DiffOp::Same, "home".into()),
            ]
        );
    }

    #[test]
    fn review_layers_count_as_review_not_drawing() {
        let mut p = board(1);
        let v = p.create_board_version("Clean").unwrap();
        let layer = crate::storyboard_review::review_layer(&p.doc, "Review");
        p.execute(Command::AddNode {
            node: Box::new(layer),
            slot: Slot::TOP,
        })
        .unwrap();
        let changes = describe_changes(
            &p.board_state(Baseline::Version(v)).unwrap(),
            &p.current_board_state().unwrap(),
        );
        assert_eq!(changes[0].aspects, vec![Aspect::Review]);
    }
}
