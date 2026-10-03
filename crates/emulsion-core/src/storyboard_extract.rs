//! Extract and merge: hand a run of scenes to another artist as a project
//! of its own, then take their work back.
//!
//! **Extract** copies whole scenes (choose scenes, or the sequences and
//! acts that hold them) into a new storyboard project: their panels with
//! captions, keyframes and comps, the scenes' cameras, the audio and
//! reference video under them (cut to the range and moved to start at
//! frame 0), the sounds and videos those clips play, and the project
//! library. Panels keep their page IDs, and the new project carries an
//! [`ExtractRecord`]: the source project's ID, the scenes and panels taken
//! with each panel's [fingerprint](crate::storyboard_fingerprint) at the
//! time, and when it was made.
//!
//! **Merge** replaces the range in the source project with the extract's
//! panels as one Undo step. Panels the extract added, changed or removed
//! come across; panels changed here since extraction, deleted on either
//! side, or added here inside the range are reported as conflicts, each
//! resolved by taking theirs or keeping mine. Sound and reference video
//! in the range come from the extract, and everything after the range
//! moves by the change in running time, so it stays in sync. A file
//! extracted from another project is refused unless merging anyway is
//! asked for.
use crate::project::{PageId, Project, ProjectEditor, ProjectPage, adopt_fields};
use crate::storyboard::{GroupId, Panel, Storyboard};
use crate::storyboard_fingerprint::panel_fingerprint;
use crate::storyboard_merge::{Edit, edit, merge_order};
use crate::{Document, graph::Graph};
use serde::{Deserialize, Serialize};
use std::collections::{BTreeMap, HashMap, HashSet};

/// A new random-looking project ID in UUID form (version 4 layout), from
/// the clock, the process and a counter.
pub fn new_project_id() -> String {
    use sha2::{Digest, Sha256};
    use std::sync::atomic::{AtomicU64, Ordering};
    static COUNTER: AtomicU64 = AtomicU64::new(0);
    let now = std::time::SystemTime::now()
        .duration_since(std::time::UNIX_EPOCH)
        .unwrap_or_default();
    let mut h = Sha256::new();
    h.update(now.as_nanos().to_le_bytes());
    h.update(std::process::id().to_le_bytes());
    h.update(COUNTER.fetch_add(1, Ordering::Relaxed).to_le_bytes());
    let local = 0u8;
    h.update((&local as *const u8 as usize).to_le_bytes());
    let mut b: [u8; 16] = h.finalize()[..16].try_into().unwrap();
    b[6] = (b[6] & 0x0f) | 0x40;
    b[8] = (b[8] & 0x3f) | 0x80;
    let hex: String = b.iter().map(|x| format!("{x:02x}")).collect();
    format!(
        "{}-{}-{}-{}-{}",
        &hex[..8],
        &hex[8..12],
        &hex[12..16],
        &hex[16..20],
        &hex[20..]
    )
}

/// Where an extracted project came from.
#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
pub struct ExtractRecord {
    /// The source storyboard's `project_id`.
    pub source_project: String,
    /// The source's name when it was extracted, for messages.
    #[serde(default, skip_serializing_if = "String::is_empty")]
    pub source_name: String,
    /// Source scene IDs in board order.
    pub scenes: Vec<GroupId>,
    /// Source panels in board order with their fingerprints then.
    pub panels: Vec<ExtractedPanel>,
    /// The source panel just before the range, if any, so an emptied range
    /// can be put back in place.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub after: Option<PageId>,
    /// The range's first frame and length in the source's running time.
    pub start_frame: u64,
    pub frames: u64,
    /// Seconds since 1970 (UTC) when the extract was made.
    pub extracted_at: u64,
}

#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
pub struct ExtractedPanel {
    pub id: PageId,
    pub fingerprint: String,
}

/// The scenes, in board order, of `groups`: scenes themselves, or every
/// scene of a sequence or act. They must be one run of neighbouring scenes.
pub fn range_scenes(
    board: &Storyboard,
    layout: &[PageId],
    groups: &[GroupId],
) -> Result<Vec<GroupId>, String> {
    if groups.is_empty() {
        return Err("Choose scenes to extract.".into());
    }
    for group in groups {
        if !board.scenes.contains_key(group)
            && !board.sequences.contains_key(group)
            && !board.acts.contains_key(group)
        {
            return Err(format!("No act, sequence or scene has ID {group}."));
        }
    }
    let wanted: HashSet<_> = groups.iter().collect();
    let outline = board.outline(layout);
    let picked: Vec<usize> = outline
        .iter()
        .enumerate()
        .filter(|(_, s)| {
            wanted.contains(&s.scene) || wanted.contains(&s.sequence) || wanted.contains(&s.act)
        })
        .map(|(i, _)| i)
        .collect();
    if picked.windows(2).any(|w| w[1] != w[0] + 1) {
        return Err("Choose neighbouring scenes: an extract is one run of the board.".into());
    }
    Ok(picked.into_iter().map(|i| outline[i].scene).collect())
}

/// First frame and length on the running time of the panels at layout
/// positions `range` (thumbnail sheets take no time).
fn span(board: &Storyboard, layout: &[PageId], range: std::ops::Range<usize>) -> (u64, u64) {
    let frames = |ids: &[PageId]| -> u64 {
        ids.iter()
            .filter_map(|id| board.panels.get(id))
            .filter(|p| p.thumbnails.is_none())
            .map(|p| u64::from(p.frames))
            .sum()
    };
    (frames(&layout[..range.start]), frames(&layout[range]))
}

/// The fingerprint of each page of a storyboard project.
fn fingerprints<'a>(
    board: &Storyboard,
    pages: impl Iterator<Item = (PageId, &'a str, &'a Document)>,
) -> HashMap<PageId, String> {
    pages
        .filter_map(|(id, name, doc)| {
            let panel = board.panels.get(&id)?;
            Some((id, panel_fingerprint(name, doc, panel)))
        })
        .collect()
}

/// A new project holding the scenes `groups` names (see [`range_scenes`])
/// of `project`, with an extract record made at `now` (seconds since 1970).
/// `source_name` names the source in messages.
pub fn extract_scenes(
    project: &Project,
    groups: &[GroupId],
    source_name: &str,
    now: u64,
) -> Result<Project, String> {
    let board = project
        .storyboard
        .as_ref()
        .ok_or("This is not a storyboard project.")?;
    let layout: Vec<_> = project.pages.iter().map(|p| p.meta.id).collect();
    let scenes = range_scenes(board, &layout, groups)?;
    let chosen: HashSet<_> = scenes.iter().copied().collect();
    let first = layout
        .iter()
        .position(|id| chosen.contains(&board.panels[id].scene))
        .ok_or("Choose scenes to extract.")?;
    let count = layout[first..]
        .iter()
        .take_while(|id| chosen.contains(&board.panels[id].scene))
        .count();
    let ids = &layout[first..first + count];
    let pages: Vec<ProjectPage> = project.pages[first..first + count]
        .iter()
        .map(|page| ProjectPage {
            meta: page.meta.clone(),
            doc: page.doc.clone(),
            graph: Graph::new(page.doc.clone(), "Extracted"),
        })
        .collect();
    let prints = fingerprints(
        board,
        pages
            .iter()
            .map(|p| (p.meta.id, p.meta.name.as_str(), &p.doc)),
    );
    let (start, frames) = span(board, &layout, first..first + count);
    let mut next = board.clone();
    // Board versions name the source's page history, which stays behind.
    next.versions = Default::default();
    // The extract is a project of its own: no cloud merge history.
    next.sharing.merged_revision = None;
    next.reconcile(ids);
    // Only the 3D models the extracted sets use travel with it.
    next.prune_models();
    next.timeline = board
        .timeline
        .excerpt(start, start + frames, board.settings.frame_rate);
    next.project_id = new_project_id();
    next.extract = Some(ExtractRecord {
        source_project: board.project_id.clone(),
        source_name: source_name.trim().chars().take(200).collect(),
        scenes,
        panels: ids
            .iter()
            .map(|id| ExtractedPanel {
                id: *id,
                fingerprint: prints[id].clone(),
            })
            .collect(),
        after: first.checked_sub(1).map(|i| layout[i]),
        start_frame: start,
        frames,
        extracted_at: now,
    });
    let out = Project {
        kind: project.kind,
        active: ids[0],
        next_page_id: project.next_page_id,
        pages,
        storyboard: Some(next),
    };
    out.validate()?;
    Ok(out)
}

/// What to do with one conflict: the shared merge engine's choice. An
/// extract merge offers Take theirs and Keep mine.
pub use crate::storyboard_merge::Resolution;

#[derive(Clone, Copy, Debug, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum ConflictKind {
    /// The panel changed here since extraction.
    ChangedHere,
    /// The panel was deleted here; the extract still has it.
    DeletedHere,
    /// The extract deleted the panel; it is still here.
    DeletedThere,
    /// The panel was added here, inside the extracted scenes.
    AddedHere,
}

impl ConflictKind {
    pub fn label(self) -> &'static str {
        match self {
            Self::ChangedHere => "Changed here since the extract was made",
            Self::DeletedHere => "Deleted here, still in the extract",
            Self::DeletedThere => "Deleted in the extract, still here",
            Self::AddedHere => "Added here since the extract was made",
        }
    }
}

#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
pub struct Conflict {
    /// The panel's page ID (in this project, or in the extract when it was
    /// deleted here).
    pub panel: PageId,
    pub name: String,
    pub kind: ConflictKind,
    /// Whether the extract changed the panel too.
    pub changed_there: bool,
    /// The suggested choice: keep whichever side did any work.
    pub default: Resolution,
}

/// What merging an extract would do, before it is applied.
#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
pub struct MergeReport {
    pub source_project: String,
    pub this_project: String,
    /// The extract was made from this project.
    pub same_project: bool,
    pub source_name: String,
    pub extracted_at: u64,
    /// Panels of the range here, and in the extract.
    pub panels_here: usize,
    pub panels_there: usize,
    /// Running time of the range here, and in the extract.
    pub frames_here: u64,
    pub frames_there: u64,
    pub conflicts: Vec<Conflict>,
}

/// Choices for a merge: a resolution per conflict (by panel; conflicts
/// left out take their default), and whether to merge an extract of
/// another project.
#[derive(Clone, Debug, Default, PartialEq)]
pub struct MergeOptions {
    pub resolutions: BTreeMap<PageId, Resolution>,
    pub allow_other_project: bool,
}

/// What a merge did.
#[derive(Clone, Debug, PartialEq)]
pub struct MergeSummary {
    /// The range's panels after the merge, in board order.
    pub panels: Vec<PageId>,
    pub kept_mine: usize,
    pub took_theirs: usize,
    /// Change in running time; everything after the range moved by it.
    pub frames_delta: i64,
}

/// One panel of the merged range.
#[derive(Clone, Copy, Debug, PartialEq)]
enum Side {
    Theirs(PageId),
    Mine(PageId),
}

struct Analysis {
    report: MergeReport,
    /// Layout positions of the range here.
    range: std::ops::Range<usize>,
    /// Where the range goes when it is empty here.
    after: Option<PageId>,
}

impl ProjectEditor {
    fn merge_analysis(&self, extract: &Project) -> Result<Analysis, String> {
        let board = self
            .storyboard()
            .ok_or("This is not a storyboard project.")?;
        let theirs = extract
            .storyboard
            .as_ref()
            .ok_or("That file is not a storyboard.")?;
        let record = theirs
            .extract
            .as_ref()
            .ok_or("That storyboard is not an extract: it was not made with Extract Scenes.")?;
        if theirs.settings.frame_rate != board.settings.frame_rate {
            return Err(format!(
                "The extract plays at {}, this storyboard at {}. Set the same frame rate first.",
                theirs.settings.frame_rate.label(),
                board.settings.frame_rate.label()
            ));
        }
        if (theirs.settings.width, theirs.settings.height)
            != (board.settings.width, board.settings.height)
        {
            return Err(format!(
                "The extract is {} × {}, this storyboard {} × {}. Merge needs the same resolution.",
                theirs.settings.width,
                theirs.settings.height,
                board.settings.width,
                board.settings.height
            ));
        }
        let layout: Vec<_> = self.page_list().iter().map(|m| m.id).collect();
        let recorded: HashMap<PageId, &str> = record
            .panels
            .iter()
            .map(|p| (p.id, p.fingerprint.as_str()))
            .collect();
        let scenes: HashSet<_> = record.scenes.iter().copied().collect();
        let inside: Vec<usize> = layout
            .iter()
            .enumerate()
            .filter(|(_, id)| {
                recorded.contains_key(id) || scenes.contains(&board.panels[*id].scene)
            })
            .map(|(i, _)| i)
            .collect();
        if inside.windows(2).any(|w| w[1] != w[0] + 1) {
            return Err("Panels of the extracted scenes are no longer next to each other here. Move them back together, then merge.".into());
        }
        let (range, after) = match (inside.first(), inside.last()) {
            (Some(a), Some(b)) => (*a..*b + 1, a.checked_sub(1).map(|i| layout[i])),
            _ => {
                // The whole range is gone: put it back after the panel that
                // was before it, or at the end when that is gone too.
                let after = match record.after {
                    Some(id) if layout.contains(&id) => Some(id),
                    Some(_) => layout.last().copied(),
                    None => None,
                };
                let at = after.map_or(0, |id| layout.iter().position(|x| *x == id).unwrap() + 1);
                (at..at, after)
            }
        };
        let names: HashMap<PageId, &str> = self
            .page_list()
            .iter()
            .map(|m| (m.id, m.name.as_str()))
            .collect();
        let here = fingerprints(
            board,
            layout[range.clone()]
                .iter()
                .map(|id| (*id, names[id], &self.page(*id).unwrap().doc)),
        );
        let there = fingerprints(
            theirs,
            extract
                .pages
                .iter()
                .map(|p| (p.meta.id, p.meta.name.as_str(), &p.doc)),
        );
        let mut conflicts = Vec::new();
        for item in &record.panels {
            // The extract record is the base: fingerprints when it was made.
            let base = Some(item.fingerprint.as_str());
            let mine = edit(base, here.get(&item.id).map(String::as_str));
            let yours = edit(base, there.get(&item.id).map(String::as_str));
            let changed_there = yours == Edit::Changed;
            let pick = |prefer_theirs: bool| {
                if prefer_theirs {
                    Resolution::Theirs
                } else {
                    Resolution::Mine
                }
            };
            // The extract replaces the range, so every change here is
            // confirmed, not only the ones both sides made.
            let conflict = match (mine, yours) {
                (Edit::Changed, Edit::Changed | Edit::Unchanged) => {
                    Some((ConflictKind::ChangedHere, pick(changed_there)))
                }
                (Edit::Deleted, Edit::Changed | Edit::Unchanged) => {
                    Some((ConflictKind::DeletedHere, pick(changed_there)))
                }
                (Edit::Changed | Edit::Unchanged, Edit::Deleted) => {
                    Some((ConflictKind::DeletedThere, pick(mine != Edit::Changed)))
                }
                _ => None,
            };
            if let Some((kind, default)) = conflict {
                let name = names.get(&item.id).copied().or_else(|| {
                    extract
                        .pages
                        .iter()
                        .find(|p| p.meta.id == item.id)
                        .map(|p| p.meta.name.as_str())
                });
                conflicts.push(Conflict {
                    panel: item.id,
                    name: name.unwrap_or_default().to_string(),
                    kind,
                    changed_there,
                    default,
                });
            }
        }
        for id in &layout[range.clone()] {
            if !recorded.contains_key(id) {
                conflicts.push(Conflict {
                    panel: *id,
                    name: names[id].to_string(),
                    kind: ConflictKind::AddedHere,
                    changed_there: false,
                    default: Resolution::Mine,
                });
            }
        }
        let their_layout: Vec<_> = extract.pages.iter().map(|p| p.meta.id).collect();
        let report = MergeReport {
            source_project: record.source_project.clone(),
            this_project: board.project_id.clone(),
            same_project: record.source_project == board.project_id,
            source_name: record.source_name.clone(),
            extracted_at: record.extracted_at,
            panels_here: range.len(),
            panels_there: their_layout.len(),
            frames_here: span(board, &layout, range.clone()).1,
            frames_there: theirs.animatic_frames(&their_layout),
            conflicts,
        };
        Ok(Analysis {
            report,
            range,
            after,
        })
    }

    /// What merging `extract` back would do: the range here and there, and
    /// every conflict with its suggested resolution. Changes nothing.
    pub fn plan_merge(&self, extract: &Project) -> Result<MergeReport, String> {
        Ok(self.merge_analysis(extract)?.report)
    }

    /// Replace the extracted range with `extract`'s panels as one Undo
    /// step, resolving conflicts by `options` (see the module docs). Sound
    /// and video in the range come from the extract; everything after it
    /// moves by the change in running time.
    pub fn merge_extract(
        &mut self,
        extract: &Project,
        options: &MergeOptions,
    ) -> Result<MergeSummary, String> {
        if self.in_transaction() {
            return Err("Finish the current edit before merging.".into());
        }
        let Analysis {
            report,
            range,
            after,
        } = self.merge_analysis(extract)?;
        if !report.same_project && !options.allow_other_project {
            return Err(format!(
                "This extract was made from another project{} (ID {}), not this one (ID {}). Merge anyway only if this project is a copy of that one.",
                if report.source_name.is_empty() {
                    String::new()
                } else {
                    format!(" “{}”", report.source_name)
                },
                report.source_project,
                report.this_project
            ));
        }
        if options.resolutions.values().any(|r| *r == Resolution::Both) {
            return Err("An extract merge takes theirs or keeps mine for each conflict.".into());
        }
        if let Some(id) = options
            .resolutions
            .keys()
            .find(|id| !report.conflicts.iter().any(|c| c.panel == **id))
        {
            return Err(format!("Panel {id} has no conflict to resolve."));
        }
        let choice = |id: PageId| -> Option<Resolution> {
            let conflict = report.conflicts.iter().find(|c| c.panel == id)?;
            Some(
                options
                    .resolutions
                    .get(&id)
                    .copied()
                    .unwrap_or(conflict.default),
            )
        };
        let board = self.storyboard().unwrap();
        let theirs = extract.storyboard.as_ref().unwrap();
        let record = theirs.extract.as_ref().unwrap();
        let recorded: HashSet<_> = record.panels.iter().map(|p| p.id).collect();
        let layout: Vec<_> = self.page_list().iter().map(|m| m.id).collect();
        let here_range = &layout[range.clone()];

        // The merged range, in order: the extract's panels, with kept
        // panels of mine slotted in after their neighbour (the shared
        // merge engine's ordering). New panels on either side may share an
        // ID, so items say which side a new panel is from.
        #[derive(Clone, Copy, PartialEq, Eq, Hash)]
        enum Item {
            Shared(PageId),
            TheirsNew(PageId),
            MineNew(PageId),
        }
        let in_extract: HashSet<_> = extract.pages.iter().map(|p| p.meta.id).collect();
        let tag = |id: PageId, new: fn(PageId) -> Item| {
            if recorded.contains(&id) {
                Item::Shared(id)
            } else {
                new(id)
            }
        };
        let primary: Vec<Item> = extract
            .pages
            .iter()
            .map(|p| tag(p.meta.id, Item::TheirsNew))
            .collect();
        let secondary: Vec<Item> = here_range
            .iter()
            .map(|id| tag(*id, Item::MineNew))
            .collect();
        let keep = |item: Item| match item {
            Item::TheirsNew(_) => true,
            Item::MineNew(id) => choice(id) == Some(Resolution::Mine),
            Item::Shared(id) if in_extract.contains(&id) => {
                here_range.contains(&id) || choice(id) != Some(Resolution::Mine)
            }
            Item::Shared(id) => choice(id) == Some(Resolution::Mine),
        };
        let sides: Vec<Side> = merge_order(&primary, &secondary, keep)
            .into_iter()
            .map(|item| match item {
                Item::TheirsNew(id) => Side::Theirs(id),
                Item::MineNew(id) => Side::Mine(id),
                Item::Shared(id)
                    if in_extract.contains(&id) && choice(id) != Some(Resolution::Mine) =>
                {
                    Side::Theirs(id)
                }
                Item::Shared(id) => Side::Mine(id),
            })
            .collect();
        if sides.is_empty() {
            return Err(
                "The merge would leave no panels in the extracted scenes. Delete them instead."
                    .into(),
            );
        }

        let mut next = Storyboard::clone(board);
        let used = sides.iter().filter_map(|s| match s {
            Side::Theirs(id) => theirs.panels.get(id),
            Side::Mine(_) => None,
        });
        let fields = adopt_fields(
            &mut next,
            &theirs.captions,
            used.flat_map(|p| p.captions.keys().copied())
                .collect::<Vec<_>>(),
        )?;
        // Their scenes: the same scene here when it came from here,
        // otherwise a new one beside the range.
        let sequence = here_range
            .first()
            .or(after.as_ref())
            .and_then(|id| board.panels.get(id))
            .map(|p| board.scenes[&p.scene].sequence)
            .or_else(|| next.sequences.keys().next().copied())
            .ok_or("The storyboard has no sequences.")?;
        let source_scenes: HashSet<_> = record.scenes.iter().copied().collect();
        let mut scenes: HashMap<GroupId, GroupId> = HashMap::new();
        for page in &extract.pages {
            let scene = theirs.panels[&page.meta.id].scene;
            if scenes.contains_key(&scene) {
                continue;
            }
            let here_scene = if source_scenes.contains(&scene) && board.scenes.contains_key(&scene)
            {
                scene
            } else {
                next.add_scene(sequence)
            };
            let theirs_scene = &theirs.scenes[&scene];
            let mine = next.scenes.get_mut(&here_scene).unwrap();
            mine.name = theirs_scene.name.clone();
            mine.locked = theirs_scene.locked;
            match theirs.cameras.get(&scene) {
                Some(camera) => next.cameras.insert(here_scene, camera.clone()),
                None => next.cameras.remove(&here_scene),
            };
            scenes.insert(scene, here_scene);
        }
        let names: HashMap<PageId, &str> = self
            .page_list()
            .iter()
            .map(|m| (m.id, m.name.as_str()))
            .collect();
        let mut items = Vec::new();
        let (mut kept_mine, mut took_theirs) = (0, 0);
        for side in &sides {
            items.push(match *side {
                Side::Theirs(id) => {
                    took_theirs += 1;
                    let page = extract.pages.iter().find(|p| p.meta.id == id).unwrap();
                    let panel = &theirs.panels[&id];
                    let captions = panel
                        .captions
                        .iter()
                        .filter_map(|(f, text)| Some((*fields.get(f)?, text.clone())))
                        .collect();
                    let panel = Panel {
                        scene: scenes[&panel.scene],
                        captions,
                        ..panel.clone()
                    };
                    (page.meta.name.clone(), page.doc.clone(), panel)
                }
                Side::Mine(id) => {
                    kept_mine += 1;
                    let doc = self.page(id).unwrap().doc.clone();
                    (names[&id].to_string(), doc, board.panels[&id].clone())
                }
            });
        }
        let new_frames: u64 = items
            .iter()
            .filter(|(_, _, p)| p.thumbnails.is_none())
            .map(|(_, _, p)| u64::from(p.frames))
            .sum();
        let (start, old_frames) = span(board, &layout, range.clone());
        next.timeline.splice(
            start,
            old_frames,
            &theirs.timeline,
            new_frames,
            board.settings.frame_rate,
        )?;
        for item in &theirs.library.items {
            let known = next
                .library
                .items
                .iter()
                .any(|i| i.id == item.id && i.name == item.name && i.kind == item.kind);
            if !known {
                next.library
                    .add_item(&item.name, &item.tags, item.clone())?;
            }
        }
        let ids = self.insert_into(next, after, items, &[], here_range)?;
        Ok(MergeSummary {
            panels: ids,
            kept_mine,
            took_theirs,
            frames_delta: new_frames as i64 - old_frames as i64,
        })
    }
}

#[cfg(test)]
#[path = "storyboard_extract_tests.rs"]
mod tests;
