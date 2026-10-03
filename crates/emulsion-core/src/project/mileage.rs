//! Line mileage (SB4): the length of every stroke drawn on a storyboard
//! panel, added up per panel and for the project.
//!
//! Mileage counts ink that is on the page. A stroke counts once its Undo
//! step lands; undoing it takes its length off again and redoing it puts it
//! back, so Undo never counts a stroke twice and a stroke thrown away by a
//! new edit after Undo never counts. The ledger lives outside Undo: each
//! stroke remembers the panel revision it produced, and a stroke counts
//! while that revision is part of the panel's present (the current
//! revision or one an Undo step returns to). Strokes older than the oldest
//! Undo step settle into the panel's total, as does everything read from
//! the file. Saving writes the totals to `Storyboard::mileage`.
use super::{PageId, ProjectEditor};
use crate::Editor;
use std::collections::{BTreeMap, HashSet};

/// Mileage at 72 pixels per inch when a panel records no resolution.
const DEFAULT_PPI: f64 = 72.;
/// A football pitch, in millimetres (105 m).
const PITCH_MM: f64 = 105_000.;

/// A stroke length in panel pixels and in millimetres on paper at the
/// panel's resolution.
#[derive(Clone, Copy, Debug, Default, PartialEq)]
pub struct Mileage {
    pub px: f64,
    pub mm: f64,
}

impl Mileage {
    /// `px` panel pixels at `ppi` pixels per inch (72 when unknown).
    pub fn from_px(px: f64, ppi: f32) -> Self {
        let ppi = if ppi.is_finite() && ppi > 0. {
            f64::from(ppi)
        } else {
            DEFAULT_PPI
        };
        Self {
            px,
            mm: px / ppi * 25.4,
        }
    }

    /// "12.4 m", "84 cm" or "6 mm".
    pub fn label(&self) -> String {
        let mm = self.mm.max(0.);
        if mm >= 1000. {
            format!("{:.1} m", mm / 1000.)
        } else if mm >= 10. {
            format!("{:.0} cm", mm / 10.)
        } else {
            format!("{mm:.0} mm")
        }
    }

    /// "≈ 1.3 football pitches", once the line is longer than a metre.
    pub fn comparison(&self) -> Option<String> {
        if self.mm < 1000. {
            return None;
        }
        let pitches = self.mm / PITCH_MM;
        Some(if pitches < 0.95 {
            format!("≈ {:.0}% of a football pitch", pitches * 100.)
        } else if pitches < 1.05 {
            "≈ 1 football pitch".into()
        } else if pitches < 10. {
            format!("≈ {pitches:.1} football pitches")
        } else {
            format!("≈ {pitches:.0} football pitches")
        })
    }
}

impl std::ops::Add for Mileage {
    type Output = Self;
    fn add(self, other: Self) -> Self {
        Self {
            px: self.px + other.px,
            mm: self.mm + other.mm,
        }
    }
}

/// The length of a polyline.
pub fn path_length(points: impl IntoIterator<Item = (f64, f64)>) -> f64 {
    let mut points = points.into_iter();
    let Some(mut last) = points.next() else {
        return 0.;
    };
    let mut length = 0.;
    for p in points {
        let step = (p.0 - last.0).hypot(p.1 - last.1);
        if step.is_finite() {
            length += step;
        }
        last = p;
    }
    length
}

/// One counted stroke: the panel revision it produced and its length.
#[derive(Clone, Copy, Debug, PartialEq)]
struct Stroke {
    revision: u64,
    px: f64,
}

/// Where a stroke stands against its panel's history.
#[derive(Clone, Copy, Debug, PartialEq)]
enum Standing {
    /// On the page now.
    Present,
    /// Undone; Redo can bring it back.
    Undone,
    /// Older than every Undo step: on the page for good.
    Settled,
    /// Undone and then replaced by another edit.
    Discarded,
}

struct Lineage {
    present: HashSet<u64>,
    undone: HashSet<u64>,
    oldest: u64,
}

impl Lineage {
    fn of(editor: &Editor) -> Self {
        let present: HashSet<u64> = editor
            .history
            .revisions(false)
            .chain([editor.revision])
            .collect();
        let oldest = present.iter().copied().min().unwrap_or(editor.revision);
        Self {
            present,
            undone: editor.history.revisions(true).collect(),
            oldest,
        }
    }

    fn standing(&self, stroke: &Stroke) -> Standing {
        if self.present.contains(&stroke.revision) {
            Standing::Present
        } else if self.undone.contains(&stroke.revision) {
            Standing::Undone
        } else if stroke.revision < self.oldest {
            Standing::Settled
        } else {
            Standing::Discarded
        }
    }
}

/// The open project's mileage, outside Undo.
#[derive(Clone, Debug, Default)]
pub(super) struct Ink {
    /// Pixels no Undo step can take back: read from the file, or strokes
    /// older than the oldest Undo step.
    settled: BTreeMap<PageId, f64>,
    /// Strokes Undo or Redo can still reach.
    strokes: BTreeMap<PageId, Vec<Stroke>>,
}

impl Ink {
    /// Mileage read from a file. Values that are not finite or negative
    /// are dropped.
    pub(super) fn loaded(saved: BTreeMap<PageId, f64>) -> Self {
        Self {
            settled: saved
                .into_iter()
                .filter(|(_, px)| px.is_finite() && *px > 0.)
                .collect(),
            strokes: BTreeMap::new(),
        }
    }
}

impl ProjectEditor {
    /// Count a stroke of `length` panel pixels just committed on `page`
    /// (brush, eraser, vector line or shape). Call it after the stroke's
    /// Undo step lands. Only storyboard panels keep mileage.
    pub fn record_ink(&mut self, page: PageId, length: f64) {
        if self.storyboard.is_none() || !length.is_finite() || length <= 0. {
            return;
        }
        let Some(editor) = self.page(page) else {
            return;
        };
        let revision = editor.revision;
        self.settle_ink();
        let strokes = self.ink.strokes.entry(page).or_default();
        // A stroke that changed nothing shares its revision with the edit
        // before it; that edit already holds the place.
        if !strokes.iter().any(|s| s.revision == revision) {
            strokes.push(Stroke {
                revision,
                px: length,
            });
        }
    }

    /// Move strokes Undo can no longer reach into the totals and forget the
    /// ones a new edit replaced.
    fn settle_ink(&mut self) {
        let Self { ink, pages, .. } = self;
        for (page, strokes) in &mut ink.strokes {
            let Some(editor) = pages.get(page) else {
                continue;
            };
            let lineage = Lineage::of(editor);
            strokes.retain(|stroke| match lineage.standing(stroke) {
                Standing::Present | Standing::Undone => true,
                Standing::Settled => {
                    *ink.settled.entry(*page).or_default() += stroke.px;
                    false
                }
                Standing::Discarded => false,
            });
        }
        ink.strokes.retain(|_, strokes| !strokes.is_empty());
    }

    fn panel_ink_px(&self, page: PageId) -> f64 {
        let settled = self.ink.settled.get(&page).copied().unwrap_or(0.);
        let (Some(strokes), Some(editor)) = (self.ink.strokes.get(&page), self.pages.get(&page))
        else {
            return settled;
        };
        let lineage = Lineage::of(editor);
        settled
            + strokes
                .iter()
                .filter(|s| matches!(lineage.standing(s), Standing::Present | Standing::Settled))
                .map(|s| s.px)
                .sum::<f64>()
    }

    /// The stroke length drawn on `page` that is still on the page.
    pub fn panel_mileage(&self, page: PageId) -> Mileage {
        let ppi = self.pages.get(&page).map_or(0., |e| e.doc.resolution);
        Mileage::from_px(self.panel_ink_px(page), ppi)
    }

    /// Every panel's mileage added up.
    pub fn project_mileage(&self) -> Mileage {
        self.layout
            .iter()
            .map(|m| self.panel_mileage(m.id))
            .fold(Mileage::default(), |a, b| a + b)
    }

    /// Start counting again from zero on `panels`, or on every panel. This
    /// is not an Undo step: Undo does not bring the old count back.
    pub fn reset_mileage(&mut self, panels: Option<&[PageId]>) {
        match panels {
            Some(ids) => {
                for id in ids {
                    self.ink.settled.remove(id);
                    self.ink.strokes.remove(id);
                }
            }
            None => self.ink = Ink::default(),
        }
        self.mark_storyboard_unsaved();
    }

    /// The mileage to save: each laid-out panel's total.
    pub(super) fn mileage_to_save(&self) -> BTreeMap<PageId, f64> {
        self.layout
            .iter()
            .map(|m| (m.id, self.panel_ink_px(m.id)))
            .filter(|(_, px)| *px > 0.)
            .collect()
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::project::{Project, ProjectKind};
    use crate::{Command, Document, Node, NodeKind, command::Slot};

    fn board() -> ProjectEditor {
        ProjectEditor::new_project(ProjectKind::Storyboard, Document::new(72, 36)).unwrap()
    }

    /// An edit standing in for a stroke, with its length counted.
    fn stroke(p: &mut ProjectEditor, length: f64) {
        let n = p.doc.nodes.len();
        p.execute(Command::AddNode {
            node: Box::new(Node::new(
                0,
                format!("Stroke {n}"),
                NodeKind::Fill { rgba: [0; 4] },
            )),
            slot: Slot::TOP,
        })
        .unwrap();
        let page = p.active_page();
        p.record_ink(page, length);
    }

    #[test]
    fn path_length_adds_up_segments() {
        assert_eq!(path_length([(0., 0.), (3., 4.), (3., 10.)]), 11.);
        assert_eq!(path_length([(1., 1.)]), 0.);
        assert_eq!(path_length(std::iter::empty()), 0.);
    }

    #[test]
    fn strokes_add_up_and_undo_takes_them_back_once() {
        let mut p = board();
        stroke(&mut p, 100.);
        stroke(&mut p, 50.);
        assert_eq!(p.panel_mileage(1).px, 150.);
        assert!(p.undo());
        assert_eq!(p.panel_mileage(1).px, 100., "undo subtracts");
        assert!(p.redo());
        assert_eq!(p.panel_mileage(1).px, 150., "redo adds it back");
        assert!(p.undo());
        assert!(p.undo());
        assert_eq!(p.panel_mileage(1).px, 0.);
        assert!(p.redo());
        // A new stroke after Undo drops the undone one for good.
        stroke(&mut p, 7.);
        assert_eq!(p.panel_mileage(1).px, 107.);
        assert!(!p.redo());
        assert_eq!(p.project_mileage().px, 107.);
    }

    #[test]
    fn strokes_older_than_the_history_stay_counted() {
        let mut p = board();
        for _ in 0..130 {
            stroke(&mut p, 1.);
        }
        assert_eq!(p.panel_mileage(1).px, 130.);
        while p.undo() {}
        // Only the 100 strokes Undo still reached came off.
        assert_eq!(p.panel_mileage(1).px, 30.);
    }

    #[test]
    fn mileage_converts_through_the_panel_resolution() {
        let mut p = board();
        stroke(&mut p, 72. * 1000.);
        let m = p.panel_mileage(1);
        assert!((m.mm - 25_400.).abs() < 1e-6, "{m:?}");
        assert_eq!(m.label(), "25.4 m");
        assert_eq!(m.comparison().unwrap(), "≈ 24% of a football pitch");
        assert_eq!(Mileage::from_px(72. * 20., 72.).label(), "51 cm");
        assert_eq!(Mileage::from_px(20., 72.).label(), "7 mm");
        assert_eq!(
            Mileage::from_px(105_000. / 25.4 * 72. * 3., 72.)
                .comparison()
                .unwrap(),
            "≈ 3.0 football pitches"
        );
    }

    #[test]
    fn mileage_saves_reopens_and_resets() {
        let mut p = board();
        stroke(&mut p, 40.);
        let saved = p.snapshot().unwrap();
        assert_eq!(saved.storyboard.as_ref().unwrap().mileage[&1], 40.);
        let json = serde_json::to_string(saved.storyboard.as_ref().unwrap()).unwrap();
        assert!(json.contains("\"mileage\""));
        let mut opened = ProjectEditor::open(saved, None).unwrap();
        assert_eq!(opened.panel_mileage(1).px, 40.);
        assert!(
            opened.storyboard().unwrap().mileage.is_empty(),
            "kept outside Undo while open"
        );
        stroke(&mut opened, 2.);
        assert_eq!(opened.project_mileage().px, 42.);
        opened.reset_mileage(None);
        assert_eq!(opened.project_mileage().px, 0.);
        assert!(opened.is_modified());
        let snapshot = opened.snapshot().unwrap();
        assert!(snapshot.storyboard.unwrap().mileage.is_empty());
    }

    #[test]
    fn files_without_mileage_still_open() {
        let p = board();
        let mut value = serde_json::to_value(p.storyboard().unwrap()).unwrap();
        value.as_object_mut().unwrap().remove("mileage");
        let board: crate::storyboard::Storyboard = serde_json::from_value(value).unwrap();
        assert!(board.mileage.is_empty());
        let project = Project {
            storyboard: Some(board),
            ..p.snapshot().unwrap()
        };
        let opened = ProjectEditor::open(project, None).unwrap();
        assert_eq!(opened.project_mileage(), Mileage::default());
    }

    #[test]
    fn paint_projects_keep_no_mileage() {
        let mut p = ProjectEditor::new_project(ProjectKind::Design, Document::new(8, 8)).unwrap();
        stroke(&mut p, 10.);
        assert_eq!(p.panel_mileage(1).px, 0.);
    }
}
