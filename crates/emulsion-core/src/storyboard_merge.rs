//! The storyboard merge engine: two copies of a board that went their own
//! ways since a common ancestor (the *base*) are combined part by part.
//!
//! Every part is compared three ways ([`pick`]): what only one side changed
//! is taken from that side, what both changed alike is kept, and what both
//! changed differently is a **conflict**, resolved by a per-conflict choice
//! ([`Resolution`]: keep mine, take theirs, or for panels keep both). Nothing
//! is resolved destructively behind the person's back: a conflict without a
//! choice keeps this copy's version, and the other copy stays available.
//!
//! - **Panels** match by page ID. A panel changed on one side takes that
//!   side's change; changed on both sides, each aspect (drawing, name,
//!   timing, each caption field, shot details, layer keys, review) merges
//!   on its own and only aspects both sides changed differently conflict.
//!   Added panels on either side are kept, in place beside the neighbours
//!   they had ([`merge_order`]); a panel deleted on one side and unchanged
//!   on the other is deleted; deleted against changed is a conflict.
//! - **Order**: when only one side reordered panels its order is used and
//!   the other side's new panels are slotted in after their neighbours;
//!   when both reordered, the choice is a conflict.
//! - **Board data**: acts, sequences and scenes (names, parents, locks),
//!   caption fields, scene cameras, sounds and videos, audio and video
//!   tracks (clip by clip; a track whose merged clips would overlap is a
//!   conflict), library items and board settings merge the same way.
//!   Review notes and board versions are unions; scene claims are a union
//!   where the latest word on each scene wins.
//!
//! IDs that both copies allocated independently (new panels, groups,
//! caption fields, sounds, library items) are renumbered on their side
//! first, so two different new panels never pass for one. The same inputs
//! always give the same output.
//!
//! [`merge_boards`] is pure: it returns the merged project and a report.
//! `ProjectEditor::merge_board` applies it to the open project as one Undo
//! step. Extract and merge (`storyboard_extract`) uses the same panel
//! classification ([`edit`]) and ordering.
use crate::graph::Graph;
use crate::project::{PageId, PageMeta, Project, ProjectPage};
use crate::storyboard::{CaptionField, CaptionId, GroupId, Panel, Storyboard};
use crate::storyboard_changes::{Aspect, PanelChange, describe_changes};
use crate::storyboard_fingerprint::document_fingerprint;
use crate::storyboard_library::{ItemId, LibraryItem};
use crate::storyboard_versions::{BoardState, MAX_VERSIONS};
use crate::timeline::{AssetId, AudioTrack, Timeline, VideoTrack};
use crate::{Document, NodeId};
use serde::{Deserialize, Serialize};
use std::collections::{BTreeMap, BTreeSet, HashMap, HashSet};
use std::hash::Hash;

/// What to do with one conflict.
#[derive(Clone, Copy, Debug, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum Resolution {
    /// Use the other copy's version (or its deletion).
    Theirs,
    /// Keep this copy's version (or its deletion).
    Mine,
    /// Panels only: keep mine, and theirs as a new panel after it.
    Both,
}

/// How one side changed a thing since the base.
#[derive(Clone, Copy, Debug, PartialEq, Eq, Serialize)]
#[serde(rename_all = "snake_case")]
pub enum Edit {
    Unchanged,
    Changed,
    Added,
    Deleted,
    /// In neither the base nor this side.
    Absent,
}

impl Edit {
    pub fn is_change(self) -> bool {
        matches!(self, Self::Changed | Self::Added | Self::Deleted)
    }
}

/// How `side` differs from `base` (`None` where the thing is missing).
pub fn edit<T: PartialEq + ?Sized>(base: Option<&T>, side: Option<&T>) -> Edit {
    match (base, side) {
        (Some(b), Some(s)) if b == s => Edit::Unchanged,
        (Some(_), Some(_)) => Edit::Changed,
        (None, Some(_)) => Edit::Added,
        (Some(_), None) => Edit::Deleted,
        (None, None) => Edit::Absent,
    }
}

/// Which side's value a three-way comparison takes.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Pick {
    /// Ours: unchanged on their side, changed only here, or alike on both.
    Ours,
    /// Theirs: changed only on their side.
    Theirs,
    /// Both sides changed it differently.
    Conflict,
}

/// The three-way rule for one value (`None` where it is missing).
pub fn pick<T: PartialEq + ?Sized>(base: Option<&T>, ours: Option<&T>, theirs: Option<&T>) -> Pick {
    if ours == theirs || theirs == base {
        Pick::Ours
    } else if ours == base {
        Pick::Theirs
    } else {
        Pick::Conflict
    }
}

/// The items of `primary` that `keep` keeps, in its order, with the kept
/// items only `secondary` has slotted in after the nearest item before
/// them in `secondary` that is already placed (or first when none is) and
/// after primary's own new items there, so runs of new items stay
/// together.
pub fn merge_order<I: Copy + Eq + Hash>(
    primary: &[I],
    secondary: &[I],
    keep: impl Fn(I) -> bool,
) -> Vec<I> {
    let mut out: Vec<I> = primary.iter().copied().filter(|i| keep(*i)).collect();
    let mut placed: HashSet<I> = out.iter().copied().collect();
    let in_secondary: HashSet<I> = secondary.iter().copied().collect();
    for (n, &item) in secondary.iter().enumerate() {
        if placed.contains(&item) || !keep(item) {
            continue;
        }
        let mut at = secondary[..n]
            .iter()
            .rev()
            .find_map(|before| out.iter().position(|x| x == before))
            .map_or(0, |p| p + 1);
        // Primary's own new items at that spot come first.
        while at < out.len() && !in_secondary.contains(&out[at]) {
            at += 1;
        }
        out.insert(at, item);
        placed.insert(item);
    }
    out
}

/// Whether `side` put the items all three lists share in another order
/// than `base`.
pub fn reordered<I: Copy + Eq + Hash>(base: &[I], side: &[I], other: &[I]) -> bool {
    let sets: [HashSet<I>; 3] = [base, side, other].map(|l| l.iter().copied().collect());
    let shared = |list: &[I]| -> Vec<I> {
        list.iter()
            .copied()
            .filter(|i| sets.iter().all(|s| s.contains(i)))
            .collect()
    };
    shared(base) != shared(side)
}

/// What a conflict is about.
#[derive(Clone, Debug, PartialEq, Eq, PartialOrd, Ord, Hash)]
pub enum ConflictKey {
    Panel(PageId),
    /// Both sides reordered the panels.
    Order,
    /// An act, sequence or scene.
    Group(GroupId),
    /// A scene's camera.
    Camera(GroupId),
    CaptionField(CaptionId),
    AudioTrack(String),
    VideoTrack(String),
    /// A sound or video of the timeline's library.
    Sound(AssetId),
    Library(ItemId),
    /// A board setting, by name (settings, naming, palette…).
    Board(String),
}

impl ConflictKey {
    /// The key as text: `panel:12`, `order`, `group:4`, `camera:4`,
    /// `field:2`, `audio:FX`, `video:Ref`, `sound:3`, `library:5` or
    /// `board:settings`.
    pub fn key(&self) -> String {
        match self {
            Self::Panel(id) => format!("panel:{id}"),
            Self::Order => "order".into(),
            Self::Group(id) => format!("group:{id}"),
            Self::Camera(id) => format!("camera:{id}"),
            Self::CaptionField(id) => format!("field:{id}"),
            Self::AudioTrack(name) => format!("audio:{name}"),
            Self::VideoTrack(name) => format!("video:{name}"),
            Self::Sound(id) => format!("sound:{id}"),
            Self::Library(id) => format!("library:{id}"),
            Self::Board(name) => format!("board:{name}"),
        }
    }

    pub fn parse(text: &str) -> Option<Self> {
        if text == "order" {
            return Some(Self::Order);
        }
        let (kind, rest) = text.split_once(':')?;
        let id = || rest.parse::<u64>().ok();
        Some(match kind {
            "panel" => Self::Panel(id()?),
            "group" => Self::Group(id()?),
            "camera" => Self::Camera(id()?),
            "field" => Self::CaptionField(id()?),
            "audio" => Self::AudioTrack(rest.into()),
            "video" => Self::VideoTrack(rest.into()),
            "sound" => Self::Sound(id()?),
            "library" => Self::Library(id()?),
            "board" => Self::Board(rest.into()),
            _ => return None,
        })
    }
}

impl Serialize for ConflictKey {
    fn serialize<S: serde::Serializer>(&self, s: S) -> Result<S::Ok, S::Error> {
        s.serialize_str(&self.key())
    }
}

impl<'de> Deserialize<'de> for ConflictKey {
    fn deserialize<D: serde::Deserializer<'de>>(d: D) -> Result<Self, D::Error> {
        let text = String::deserialize(d)?;
        Self::parse(&text).ok_or_else(|| serde::de::Error::custom("unknown conflict key"))
    }
}

/// One thing both sides changed differently.
#[derive(Clone, Debug, PartialEq, Serialize)]
pub struct BoardConflict {
    pub key: ConflictKey,
    /// What it is: “Panel 3”, “Scene 4”, “Audio track FX”.
    pub what: String,
    /// How the sides differ: “Both changed: drawing, captions”.
    pub detail: String,
    /// For panels changed on both sides: the aspects both changed.
    #[serde(skip_serializing_if = "Vec::is_empty")]
    pub aspects: Vec<Aspect>,
    /// Whether Keep both is offered (panels changed on both sides).
    pub keep_both: bool,
    /// The choice used when none is given: keep mine.
    pub default: Resolution,
    /// The choice this merge used.
    pub chosen: Resolution,
}

/// What a merge does, before or after it is applied.
#[derive(Clone, Debug, PartialEq, Serialize)]
pub struct BoardMergeReport {
    pub conflicts: Vec<BoardConflict>,
    /// Their changes since the base, panel by panel (change tracking's
    /// classification; panel IDs are as merged).
    pub theirs: Vec<PanelChange>,
    /// This copy's changes since the base.
    pub ours: Vec<PanelChange>,
    /// Panels and running time after the merge.
    pub panels: usize,
    pub frames: u64,
    /// Panels whose result includes a change of theirs.
    pub took_theirs: usize,
    /// Their new panels given new IDs because this copy used theirs.
    pub renumbered: usize,
    /// Their board versions added to this board.
    pub versions_added: usize,
}

/// A merged project and what the merge did.
#[derive(Clone)]
pub struct BoardMerge {
    pub project: Project,
    pub report: BoardMergeReport,
}

/// Merge `ours` and `theirs`, two storyboard projects descended from
/// `base`, resolving conflicts by `resolutions` (keyed by conflict; the rest
/// keep mine). See the module docs. Errors when an input is not a
/// storyboard, a resolution names no conflict, or the result is invalid.
pub fn merge_boards(
    base: &Project,
    ours: &Project,
    theirs: &Project,
    resolutions: &BTreeMap<ConflictKey, Resolution>,
) -> Result<BoardMerge, String> {
    let board = |p: &Project, what: &str| -> Result<(), String> {
        p.storyboard
            .as_ref()
            .map(|_| ())
            .ok_or_else(|| format!("The {what} copy is not a storyboard."))
    };
    board(base, "common")?;
    board(ours, "open")?;
    board(theirs, "other")?;
    let (theirs, renumbered) = renumber_theirs(base, ours, theirs)?;
    let mut m = Merger::new(base, ours, &theirs, resolutions);
    m.merge()?;
    let mut merged = m.finish()?;
    merged.report.renumbered = renumbered;
    for key in resolutions.keys() {
        let Some(conflict) = merged.report.conflicts.iter().find(|c| &c.key == key) else {
            return Err(format!("{} has no conflict to resolve.", key.key()));
        };
        if resolutions[key] == Resolution::Both && !conflict.keep_both {
            return Err(format!("{} cannot keep both versions.", conflict.what));
        }
    }
    Ok(merged)
}

// ── Renumbering their new IDs ─────────────────────────────────────────────

#[derive(Default)]
struct Renumber {
    pages: BTreeMap<PageId, PageId>,
    groups: BTreeMap<GroupId, GroupId>,
    assets: BTreeMap<AssetId, AssetId>,
    items: BTreeMap<ItemId, ItemId>,
}

impl Renumber {
    fn page(&self, id: PageId) -> PageId {
        self.pages.get(&id).copied().unwrap_or(id)
    }
    fn group(&self, id: GroupId) -> GroupId {
        self.groups.get(&id).copied().unwrap_or(id)
    }
    fn asset(&self, id: AssetId) -> AssetId {
        self.assets.get(&id).copied().unwrap_or(id)
    }
}

fn sb(p: &Project) -> &Storyboard {
    p.storyboard.as_ref().unwrap()
}

/// Their copy with the IDs it allocated since the base, which this copy
/// may have allocated too, moved past both copies' allocators. A new panel
/// both copies hold identically keeps its ID; caption fields match by name.
fn renumber_theirs(
    base: &Project,
    ours: &Project,
    theirs: &Project,
) -> Result<(Project, usize), String> {
    let (b, o, t) = (sb(base), sb(ours), sb(theirs));
    let mut map = Renumber::default();
    let base_pages: HashSet<_> = base.pages.iter().map(|p| p.meta.id).collect();
    let mut next_page = ours.next_page_id.max(theirs.next_page_id);
    for page in &theirs.pages {
        let id = page.meta.id;
        if base_pages.contains(&id) || id >= ours.next_page_id {
            continue;
        }
        let same = ours
            .pages
            .iter()
            .find(|p| p.meta.id == id)
            .is_some_and(|mine| {
                mine.meta.name == page.meta.name
                    && Panel {
                        scene: 0,
                        ..o.panels[&id].clone()
                    } == Panel {
                        scene: 0,
                        ..t.panels[&id].clone()
                    }
                    && document_fingerprint(&mine.doc) == document_fingerprint(&page.doc)
            });
        if !same {
            map.pages.insert(id, next_page);
            next_page += 1;
        }
    }
    let base_groups: HashSet<u64> = b
        .acts
        .keys()
        .chain(b.sequences.keys())
        .chain(b.scenes.keys())
        .chain(b.captions.iter().map(|c| &c.id))
        .copied()
        .collect();
    let mut next_group = o.next_id.max(t.next_id);
    for field in &t.captions {
        if base_groups.contains(&field.id) {
            continue;
        }
        if let Some(mine) = o.caption(&field.name) {
            map.groups.insert(field.id, mine);
        } else if field.id < o.next_id {
            map.groups.insert(field.id, next_group);
            next_group += 1;
        }
    }
    for id in t
        .acts
        .keys()
        .chain(t.sequences.keys())
        .chain(t.scenes.keys())
    {
        if !base_groups.contains(id) && *id < o.next_id {
            map.groups.insert(*id, next_group);
            next_group += 1;
        }
    }
    let base_assets: HashSet<_> = b
        .timeline
        .assets
        .keys()
        .chain(b.timeline.videos.keys())
        .collect();
    let mut next_asset = o.timeline.next_asset.max(t.timeline.next_asset);
    for (id, asset) in &t.timeline.assets {
        if base_assets.contains(id) || *id >= o.timeline.next_asset {
            continue;
        }
        if o.timeline
            .assets
            .get(id)
            .is_some_and(|mine| crate::timeline::same_sound(mine, asset))
        {
            continue;
        }
        map.assets.insert(*id, next_asset);
        next_asset += 1;
    }
    for (id, video) in &t.timeline.videos {
        if base_assets.contains(id) || *id >= o.timeline.next_asset {
            continue;
        }
        if o.timeline.videos.get(id).is_some_and(|mine| {
            mine.name == video.name
                && mine.format == video.format
                && mine.duration_ms == video.duration_ms
        }) {
            continue;
        }
        map.assets.insert(*id, next_asset);
        next_asset += 1;
    }
    let base_items: HashSet<_> = b.library.items.iter().map(|i| i.id).collect();
    let mut next_item = o.library.next_id.max(t.library.next_id);
    for item in &t.library.items {
        if base_items.contains(&item.id) || item.id >= o.library.next_id {
            continue;
        }
        if o.library.item(item.id).is_some_and(|mine| mine == item) {
            continue;
        }
        map.items.insert(item.id, next_item);
        next_item += 1;
    }
    let renumbered = map.pages.len();

    let mut out = theirs.clone();
    for page in &mut out.pages {
        page.meta.id = map.page(page.meta.id);
    }
    out.active = map.page(out.active);
    out.next_page_id = next_page;
    let board = out.storyboard.as_mut().unwrap();
    board.next_id = next_group;
    board.panels = std::mem::take(&mut board.panels)
        .into_iter()
        .map(|(id, mut panel)| {
            panel.scene = map.group(panel.scene);
            panel.captions = std::mem::take(&mut panel.captions)
                .into_iter()
                .map(|(f, c)| (map.group(f), c))
                .collect();
            (map.page(id), panel)
        })
        .collect();
    for field in &mut board.captions {
        field.id = map.group(field.id);
    }
    board.acts = std::mem::take(&mut board.acts)
        .into_iter()
        .map(|(id, a)| (map.group(id), a))
        .collect();
    board.sequences = std::mem::take(&mut board.sequences)
        .into_iter()
        .map(|(id, mut s)| {
            s.act = map.group(s.act);
            (map.group(id), s)
        })
        .collect();
    board.scenes = std::mem::take(&mut board.scenes)
        .into_iter()
        .map(|(id, mut s)| {
            s.sequence = map.group(s.sequence);
            (map.group(id), s)
        })
        .collect();
    board.cameras = std::mem::take(&mut board.cameras)
        .into_iter()
        .map(|(id, c)| (map.group(id), c))
        .collect();
    for claim in &mut board.sharing.claims {
        claim.scene = map.group(claim.scene);
    }
    let timeline = &mut board.timeline;
    timeline.next_asset = next_asset;
    timeline.assets = std::mem::take(&mut timeline.assets)
        .into_iter()
        .map(|(id, a)| (map.asset(id), a))
        .collect();
    timeline.videos = std::mem::take(&mut timeline.videos)
        .into_iter()
        .map(|(id, a)| (map.asset(id), a))
        .collect();
    for clip in timeline.tracks.iter_mut().flat_map(|t| &mut t.clips) {
        clip.asset = map.asset(clip.asset);
    }
    for clip in timeline.video.iter_mut().flat_map(|t| &mut t.clips) {
        clip.asset = map.asset(clip.asset);
    }
    board.voices.lines = std::mem::take(&mut board.voices.lines)
        .into_iter()
        .map(|(id, mut line)| {
            line.panel = map.page(line.panel);
            (map.asset(id), line)
        })
        .collect();
    board.library.next_id = next_item;
    for item in &mut board.library.items {
        item.id = map.items.get(&item.id).copied().unwrap_or(item.id);
    }
    for version in &mut board.versions.list {
        for meta in &mut version.layout {
            meta.id = map.page(meta.id);
        }
        version.pages = std::mem::take(&mut version.pages)
            .into_iter()
            .map(|(id, c)| (map.page(id), c))
            .collect();
        version.board.panels = std::mem::take(&mut version.board.panels)
            .into_iter()
            .map(|(id, p)| (map.page(id), p))
            .collect();
        for line in version.board.voices.lines.values_mut() {
            line.panel = map.page(line.panel);
        }
    }
    board.versions.retired = std::mem::take(&mut board.versions.retired)
        .into_iter()
        .map(|(id, g)| (map.page(id), g))
        .collect();
    Ok((out, renumbered))
}

// ── Merging ───────────────────────────────────────────────────────────────

/// One side's view of a panel.
struct View<'a> {
    meta: &'a PageMeta,
    drawing: String,
    panel: &'a Panel,
}

fn views(p: &Project) -> BTreeMap<PageId, View<'_>> {
    let board = sb(p);
    p.pages
        .iter()
        .filter_map(|page| {
            Some((
                page.meta.id,
                View {
                    meta: &page.meta,
                    drawing: document_fingerprint(&page.doc),
                    panel: board.panels.get(&page.meta.id)?,
                },
            ))
        })
        .collect()
}

/// Which side a merged panel's drawing and page come from.
#[derive(Clone, Copy, PartialEq, Eq)]
enum From {
    Ours,
    Theirs,
}

struct MergedPanel {
    meta: PageMeta,
    drawing: From,
    panel: Panel,
}

struct Merger<'a> {
    base: &'a Project,
    ours: &'a Project,
    theirs: &'a Project,
    resolutions: &'a BTreeMap<ConflictKey, Resolution>,
    conflicts: Vec<BoardConflict>,
    board: Storyboard,
    panels: BTreeMap<PageId, MergedPanel>,
    /// Kept-both copies of their version, after the panel they copy.
    duplicates: Vec<(PageId, MergedPanel)>,
    /// Each kept-both copy's new ID and the panel it copies.
    copies: BTreeMap<PageId, PageId>,
    order: Vec<PageId>,
    took_theirs: usize,
}

/// Merge one value present on all sides, noting a conflict.
fn take<T: PartialEq + Clone>(b: &T, o: &T, t: &T, theirs: bool, conflict: &mut bool) -> T {
    match pick(Some(b), Some(o), Some(t)) {
        Pick::Ours => o.clone(),
        Pick::Theirs => t.clone(),
        Pick::Conflict => {
            *conflict = true;
            if theirs { t.clone() } else { o.clone() }
        }
    }
}

/// Merge maps key by key (a missing key is a deletion). Returns the
/// merged map and the keys both sides changed differently, which take
/// theirs when `theirs` says so.
fn merge_map<K: Ord + Clone, V: PartialEq + Clone>(
    b: &BTreeMap<K, V>,
    o: &BTreeMap<K, V>,
    t: &BTreeMap<K, V>,
    theirs: impl Fn(&K) -> bool,
) -> (BTreeMap<K, V>, Vec<K>) {
    let keys: BTreeSet<&K> = b.keys().chain(o.keys()).chain(t.keys()).collect();
    let mut out = BTreeMap::new();
    let mut conflicts = Vec::new();
    for k in keys {
        let value = match pick(b.get(k), o.get(k), t.get(k)) {
            Pick::Ours => o.get(k),
            Pick::Theirs => t.get(k),
            Pick::Conflict => {
                conflicts.push(k.clone());
                if theirs(k) { t.get(k) } else { o.get(k) }
            }
        };
        if let Some(v) = value {
            out.insert(k.clone(), v.clone());
        }
    }
    (out, conflicts)
}

/// Merge lists as sets: ours, less what theirs removed, plus what theirs added.
fn merge_set<T: PartialEq + Clone>(b: &[T], o: &[T], t: &[T]) -> Vec<T> {
    let mut out: Vec<T> = o
        .iter()
        .filter(|x| !(b.contains(x) && !t.contains(x)))
        .cloned()
        .collect();
    for x in t {
        if !b.contains(x) && !out.contains(x) {
            out.push(x.clone());
        }
    }
    out
}

impl<'a> Merger<'a> {
    fn new(
        base: &'a Project,
        ours: &'a Project,
        theirs: &'a Project,
        resolutions: &'a BTreeMap<ConflictKey, Resolution>,
    ) -> Self {
        Self {
            base,
            ours,
            theirs,
            resolutions,
            conflicts: Vec::new(),
            board: sb(ours).clone(),
            panels: BTreeMap::new(),
            duplicates: Vec::new(),
            copies: BTreeMap::new(),
            order: Vec::new(),
            took_theirs: 0,
        }
    }

    fn choice(&self, key: &ConflictKey) -> Resolution {
        self.resolutions
            .get(key)
            .copied()
            .unwrap_or(Resolution::Mine)
    }

    fn theirs_wins(&self, key: &ConflictKey) -> bool {
        self.choice(key) == Resolution::Theirs
    }

    fn conflict(
        &mut self,
        key: ConflictKey,
        what: String,
        detail: impl Into<String>,
        aspects: Vec<Aspect>,
        keep_both: bool,
    ) {
        let chosen = self.choice(&key);
        self.conflicts.push(BoardConflict {
            key,
            what,
            detail: detail.into(),
            aspects,
            keep_both,
            default: Resolution::Mine,
            chosen,
        });
    }

    fn merge(&mut self) -> Result<(), String> {
        self.merge_settings();
        self.merge_groups();
        self.merge_fields();
        self.merge_panels();
        self.merge_order();
        self.merge_cameras();
        self.merge_timeline();
        self.merge_library();
        let (o, t) = (sb(self.ours), sb(self.theirs));
        self.board.sharing.claims =
            crate::storyboard_sharing::merge_claims(&o.sharing.claims, &t.sharing.claims);
        self.board.next_id = o.next_id.max(t.next_id);
        Ok(())
    }

    /// Board-wide settings, each merged whole.
    fn merge_settings(&mut self) {
        let (b, o, t) = (sb(self.base), sb(self.ours), sb(self.theirs));
        macro_rules! field {
            ($name:literal, $label:literal, $field:ident) => {{
                let key = ConflictKey::Board($name.into());
                let mut clash = false;
                let theirs = self.theirs_wins(&key);
                self.board.$field = take(&b.$field, &o.$field, &t.$field, theirs, &mut clash);
                if clash {
                    self.conflict(key, $label.into(), "Both changed it", Vec::new(), false);
                }
            }};
        }
        field!(
            "settings",
            "Board settings (resolution, frame rate, panel length)",
            settings
        );
        field!("naming", "Naming rules", naming);
        field!("smart_add", "Smart add layers", smart_add_layers);
        field!("stage", "Stage guides", stage);
        field!("palette", "Board palette", palette);
        field!("keyframe_sync", "Keyframe sync", keyframe_sync);
        field!("colorspace", "Working colour space", working_colorspace);
        field!("voices", "Voice cast and scratch dialogue", voices);
        field!("extract", "Extract record", extract);
        field!("project_id", "Project ID", project_id);
    }

    fn merge_groups(&mut self) {
        let (b, o, t) = (sb(self.base), sb(self.ours), sb(self.theirs));
        // Acts, sequences and scenes as (name, parent, locked).
        type Row = (String, GroupId, bool);
        let rows = |s: &Storyboard| -> BTreeMap<GroupId, (u8, Row)> {
            let mut out = BTreeMap::new();
            for (id, a) in &s.acts {
                out.insert(*id, (0, (a.name.clone(), 0, false)));
            }
            for (id, q) in &s.sequences {
                out.insert(*id, (1, (q.name.clone(), q.act, false)));
            }
            for (id, c) in &s.scenes {
                out.insert(*id, (2, (c.name.clone(), c.sequence, c.locked)));
            }
            out
        };
        let (rb, ro, rt) = (rows(b), rows(o), rows(t));
        let ids: BTreeSet<GroupId> = rb
            .keys()
            .chain(ro.keys())
            .chain(rt.keys())
            .copied()
            .collect();
        let mut merged: BTreeMap<GroupId, (u8, Row)> = BTreeMap::new();
        for id in ids {
            let key = ConflictKey::Group(id);
            let level = |l: u8| ["Act", "Sequence", "Scene"][usize::from(l)];
            let value = match (rb.get(&id), ro.get(&id), rt.get(&id)) {
                (Some(vb), Some(vo), Some(vt)) => {
                    let theirs = self.theirs_wins(&key);
                    let mut clash = false;
                    let name = take(&vb.1.0, &vo.1.0, &vt.1.0, theirs, &mut clash);
                    let parent = take(&vb.1.1, &vo.1.1, &vt.1.1, theirs, &mut clash);
                    let locked = take(&vb.1.2, &vo.1.2, &vt.1.2, theirs, &mut clash);
                    if clash {
                        self.conflict(
                            key,
                            format!("{} {}", level(vo.0), vo.1.0),
                            format!("Renamed, moved or locked differently (theirs: {})", vt.1.0),
                            Vec::new(),
                            false,
                        );
                    }
                    Some((vo.0, (name, parent, locked)))
                }
                // Deleted on a side: groups vanish with their last panel, so
                // keep it for now; regrouping drops it if nothing uses it.
                (_, Some(v), _) | (_, None, Some(v)) => Some(v.clone()),
                (_, None, None) => None,
            };
            if let Some(v) = value {
                merged.insert(id, v);
            }
        }
        let board = &mut self.board;
        board.acts.clear();
        board.sequences.clear();
        board.scenes.clear();
        for (id, (level, (name, parent, locked))) in merged {
            match level {
                0 => {
                    board.acts.insert(id, crate::storyboard::Act { name });
                }
                1 => {
                    board
                        .sequences
                        .insert(id, crate::storyboard::Sequence { act: parent, name });
                }
                _ => {
                    board.scenes.insert(
                        id,
                        crate::storyboard::Scene {
                            sequence: parent,
                            name,
                            locked,
                        },
                    );
                }
            }
        }
    }

    fn merge_fields(&mut self) {
        let (b, o, t) = (sb(self.base), sb(self.ours), sb(self.theirs));
        let map = |s: &Storyboard| -> BTreeMap<CaptionId, CaptionField> {
            s.captions.iter().map(|c| (c.id, c.clone())).collect()
        };
        let (mb, mo, mt) = (map(b), map(o), map(t));
        let resolutions = self.resolutions;
        let (fields, clashes) = merge_map(&mb, &mo, &mt, |id| {
            resolutions.get(&ConflictKey::CaptionField(*id)) == Some(&Resolution::Theirs)
        });
        for id in clashes {
            let name = mo
                .get(&id)
                .or(mt.get(&id))
                .map_or("?", |f| f.name.as_str())
                .to_string();
            self.conflict(
                ConflictKey::CaptionField(id),
                format!("Caption field {name}"),
                "Changed differently, or deleted on one side and changed on the other",
                Vec::new(),
                false,
            );
        }
        let ids = |s: &Storyboard| s.captions.iter().map(|c| c.id).collect::<Vec<_>>();
        let (lb, lo, lt) = (ids(b), ids(o), ids(t));
        let primary_theirs = !reordered(&lb, &lo, &lt) && reordered(&lb, &lt, &lo);
        let (first, second) = if primary_theirs {
            (&lt, &lo)
        } else {
            (&lo, &lt)
        };
        self.board.captions = merge_order(first, second, |id| fields.contains_key(&id))
            .into_iter()
            .map(|id| fields[&id].clone())
            .collect();
    }

    /// Merge one panel present on all three sides.
    fn merge_panel(&mut self, id: PageId, vb: &View, vo: &View, vt: &View) -> MergedPanel {
        let key = ConflictKey::Panel(id);
        let choice = self.choice(&key);
        let theirs = choice == Resolution::Theirs;
        let mut aspects = BTreeSet::new();
        let mut aspect = |a: Aspect, clash: bool| {
            if clash {
                aspects.insert(a);
            }
        };
        let (pb, po, pt) = (vb.panel, vo.panel, vt.panel);
        let mut c = false;
        let name = take(&vb.meta.name, &vo.meta.name, &vt.meta.name, theirs, &mut c);
        let bleed = take(
            &vb.meta.bleed_mm,
            &vo.meta.bleed_mm,
            &vt.meta.bleed_mm,
            theirs,
            &mut c,
        );
        aspect(Aspect::Name, c);
        let drawing = match pick(Some(&vb.drawing), Some(&vo.drawing), Some(&vt.drawing)) {
            Pick::Ours => From::Ours,
            Pick::Theirs => From::Theirs,
            Pick::Conflict => {
                aspect(Aspect::Drawing, true);
                if theirs { From::Theirs } else { From::Ours }
            }
        };
        let mut c = false;
        let frames = take(&pb.frames, &po.frames, &pt.frames, theirs, &mut c);
        let transition = take(
            &pb.transition,
            &po.transition,
            &pt.transition,
            theirs,
            &mut c,
        );
        aspect(Aspect::Timing, c);
        let (captions, clash) = merge_map(&pb.captions, &po.captions, &pt.captions, |_| theirs);
        aspect(Aspect::Caption, !clash.is_empty());
        let mut c = false;
        let scene = take(&pb.scene, &po.scene, &pt.scene, theirs, &mut c);
        let size = take(&pb.size, &po.size, &pt.size, theirs, &mut c);
        let angle = take(&pb.angle, &po.angle, &pt.angle, theirs, &mut c);
        let status = take(&pb.status, &po.status, &pt.status, theirs, &mut c);
        let tag = take(&pb.tag, &po.tag, &pt.tag, theirs, &mut c);
        let locked = take(&pb.locked, &po.locked, &pt.locked, theirs, &mut c);
        let thumbnails = take(
            &pb.thumbnails,
            &po.thumbnails,
            &pt.thumbnails,
            theirs,
            &mut c,
        );
        aspect(Aspect::Details, c);
        let (motion, clash) =
            merge_map::<NodeId, _>(&pb.motion, &po.motion, &pt.motion, |_| theirs);
        let mut c = !clash.is_empty();
        let comps = take(&pb.comps, &po.comps, &pt.comps, theirs, &mut c);
        aspect(Aspect::LayerKeys, c);
        let mut c = false;
        let review = merge_review(&pb.review, &po.review, &pt.review, theirs, &mut c);
        aspect(Aspect::Review, c);
        let merged = MergedPanel {
            meta: PageMeta {
                id,
                name,
                bleed_mm: bleed,
            },
            drawing,
            panel: Panel {
                scene,
                frames,
                captions,
                size,
                angle,
                status,
                tag,
                locked,
                thumbnails,
                transition,
                motion,
                comps,
                review,
            },
        };
        let aspects: Vec<Aspect> = aspects.into_iter().collect();
        if !aspects.is_empty() {
            let labels: Vec<_> = aspects.iter().map(|a| a.label()).collect();
            self.conflict(
                key,
                vo.meta.name.clone(),
                format!("Both changed: {}", labels.join(", ")),
                aspects,
                true,
            );
            if choice == Resolution::Both {
                self.duplicates.push((
                    id,
                    MergedPanel {
                        meta: vt.meta.clone(),
                        drawing: From::Theirs,
                        panel: vt.panel.clone(),
                    },
                ));
            }
        }
        let theirs_part = merged.drawing == From::Theirs
            || merged.meta.name != vo.meta.name
            || merged.panel != *po;
        if theirs_part {
            self.took_theirs += 1;
        }
        merged
    }

    fn merge_panels(&mut self) {
        let (vb, vo, vt) = (views(self.base), views(self.ours), views(self.theirs));
        let ids: BTreeSet<PageId> = vb
            .keys()
            .chain(vo.keys())
            .chain(vt.keys())
            .copied()
            .collect();
        let same = |a: &View, b: &View| {
            a.meta.name == b.meta.name
                && a.meta.bleed_mm == b.meta.bleed_mm
                && a.drawing == b.drawing
                && a.panel == b.panel
        };
        for id in ids {
            let key = ConflictKey::Panel(id);
            let kept = match (vb.get(&id), vo.get(&id), vt.get(&id)) {
                (Some(b), Some(o), Some(t)) => Some(self.merge_panel(id, b, o, t)),
                (Some(b), Some(o), None) => {
                    if same(b, o) {
                        None
                    } else {
                        self.conflict(
                            key.clone(),
                            o.meta.name.clone(),
                            "Changed here, deleted in theirs",
                            Vec::new(),
                            false,
                        );
                        (!self.theirs_wins(&key)).then(|| ours_panel(o))
                    }
                }
                (Some(b), None, Some(t)) => {
                    if same(b, t) {
                        None
                    } else {
                        self.conflict(
                            key.clone(),
                            t.meta.name.clone(),
                            "Deleted here, changed in theirs",
                            Vec::new(),
                            false,
                        );
                        self.theirs_wins(&key).then(|| {
                            self.took_theirs += 1;
                            theirs_panel(t)
                        })
                    }
                }
                (Some(_), None, None) => None,
                (None, Some(o), _) => Some(ours_panel(o)),
                (None, None, Some(t)) => {
                    self.took_theirs += 1;
                    Some(theirs_panel(t))
                }
                (None, None, None) => None,
            };
            if let Some(panel) = kept {
                self.panels.insert(id, panel);
            }
        }
    }

    fn merge_order(&mut self) {
        let ids = |p: &Project| p.pages.iter().map(|p| p.meta.id).collect::<Vec<_>>();
        let (lb, lo, lt) = (ids(self.base), ids(self.ours), ids(self.theirs));
        let (ours_moved, theirs_moved) = (reordered(&lb, &lo, &lt), reordered(&lb, &lt, &lo));
        let mut use_theirs = theirs_moved && !ours_moved;
        if ours_moved && theirs_moved {
            self.conflict(
                ConflictKey::Order,
                "Panel order".into(),
                "Both sides reordered panels",
                Vec::new(),
                false,
            );
            use_theirs = self.theirs_wins(&ConflictKey::Order);
        }
        let (first, second) = if use_theirs { (&lt, &lo) } else { (&lo, &lt) };
        let panels = &self.panels;
        let mut order = merge_order(first, second, |id| panels.contains_key(&id));
        // Kept-both copies go right after the panel they copy.
        let first_new = self.ours.next_page_id.max(self.theirs.next_page_id);
        let copies = std::mem::take(&mut self.duplicates);
        for (id, (source, mut copy)) in (first_new..).zip(copies) {
            copy.meta.id = id;
            let name: String = copy.meta.name.chars().take(190).collect();
            copy.meta.name = format!("{name} (theirs)");
            self.copies.insert(id, source);
            let at = order
                .iter()
                .position(|x| *x == source)
                .map_or(order.len(), |p| p + 1);
            order.insert(at, id);
            self.panels.insert(id, copy);
            self.took_theirs += 1;
        }
        self.order = order;
    }

    fn merge_cameras(&mut self) {
        let (b, o, t) = (sb(self.base), sb(self.ours), sb(self.theirs));
        let resolutions = self.resolutions;
        let (cameras, clashes) = merge_map(&b.cameras, &o.cameras, &t.cameras, |scene| {
            resolutions.get(&ConflictKey::Camera(*scene)) == Some(&Resolution::Theirs)
        });
        for scene in clashes {
            let name = self
                .board
                .scenes
                .get(&scene)
                .map_or_else(String::new, |s| s.name.clone());
            self.conflict(
                ConflictKey::Camera(scene),
                format!("Camera of scene {name}"),
                "Both changed the camera moves",
                Vec::new(),
                false,
            );
        }
        self.board.cameras = cameras;
    }

    fn merge_timeline(&mut self) {
        let (b, o, t) = (
            &sb(self.base).timeline,
            &sb(self.ours).timeline,
            &sb(self.theirs).timeline,
        );
        let resolutions = self.resolutions;
        let wins =
            |id: &AssetId| resolutions.get(&ConflictKey::Sound(*id)) == Some(&Resolution::Theirs);
        let (mut assets, clashes) = merge_map(&b.assets, &o.assets, &t.assets, wins);
        for id in clashes {
            let name = o
                .assets
                .get(&id)
                .or(t.assets.get(&id))
                .map_or_else(String::new, |a| a.name.clone());
            self.conflict(
                ConflictKey::Sound(id),
                format!("Sound {name}"),
                "Changed differently on both sides",
                Vec::new(),
                false,
            );
        }
        let (mut videos, clashes) = merge_map(&b.videos, &o.videos, &t.videos, wins);
        for id in clashes {
            let name = o
                .videos
                .get(&id)
                .or(t.videos.get(&id))
                .map_or_else(String::new, |a| a.name.clone());
            self.conflict(
                ConflictKey::Sound(id),
                format!("Video {name}"),
                "Changed differently on both sides",
                Vec::new(),
                false,
            );
        }
        let tracks = self.merge_tracks(
            &b.tracks,
            &o.tracks,
            &t.tracks,
            |t: &AudioTrack| &t.name,
            ConflictKey::AudioTrack,
            "Audio track",
            |b, o, t, theirs, clash| {
                let mut track = AudioTrack {
                    name: o.name.clone(),
                    volume_db: take(&b.volume_db, &o.volume_db, &t.volume_db, theirs, clash),
                    muted: take(&b.muted, &o.muted, &t.muted, theirs, clash),
                    solo: take(&b.solo, &o.solo, &t.solo, theirs, clash),
                    clips: merge_set(&b.clips, &o.clips, &t.clips),
                    markers: merge_set(&b.markers, &o.markers, &t.markers),
                };
                track.clips.sort_by_key(|c| c.start);
                track.markers.sort_by_key(|m| m.frame);
                if track.clips.windows(2).any(|w| w[1].start < w[0].end()) {
                    *clash = true;
                    return if theirs { t.clone() } else { o.clone() };
                }
                track
            },
        );
        let video = self.merge_tracks(
            &b.video,
            &o.video,
            &t.video,
            |t: &VideoTrack| &t.name,
            ConflictKey::VideoTrack,
            "Video track",
            |b, o, t, theirs, clash| {
                let mut track = VideoTrack {
                    name: o.name.clone(),
                    clips: merge_set(&b.clips, &o.clips, &t.clips),
                };
                track.clips.sort_by_key(|c| c.start);
                if track.clips.windows(2).any(|w| w[1].start < w[0].end()) {
                    *clash = true;
                    return if theirs { t.clone() } else { o.clone() };
                }
                track
            },
        );
        // Sounds and videos a kept clip plays stay, from whichever side has them.
        for clip in tracks.iter().flat_map(|t| &t.clips) {
            if let Some(a) = o.assets.get(&clip.asset).or(t.assets.get(&clip.asset)) {
                assets.entry(clip.asset).or_insert_with(|| a.clone());
            }
        }
        for clip in video.iter().flat_map(|t| &t.clips) {
            if let Some(a) = o.videos.get(&clip.asset).or(t.videos.get(&clip.asset)) {
                videos.entry(clip.asset).or_insert_with(|| a.clone());
            }
        }
        // Scratch dialogue lines play sounds too.
        for id in self.board.voices.lines.keys() {
            if let Some(a) = o.assets.get(id).or(t.assets.get(id)) {
                assets.entry(*id).or_insert_with(|| a.clone());
            }
        }
        self.board.timeline = Timeline {
            tracks,
            assets,
            next_asset: o.next_asset.max(t.next_asset),
            video,
            videos,
        };
    }

    /// Tracks match by name (and position among tracks of that name).
    #[allow(clippy::too_many_arguments)]
    fn merge_tracks<T: Clone + PartialEq>(
        &mut self,
        b: &[T],
        o: &[T],
        t: &[T],
        name: impl Fn(&T) -> &String,
        key: impl Fn(String) -> ConflictKey,
        label: &str,
        both: impl Fn(&T, &T, &T, bool, &mut bool) -> T,
    ) -> Vec<T> {
        let keyed = |list: &[T]| -> Vec<((String, usize), T)> {
            let mut seen: HashMap<String, usize> = HashMap::new();
            list.iter()
                .map(|track| {
                    let n = seen.entry(name(track).clone()).or_default();
                    *n += 1;
                    ((name(track).clone(), *n - 1), track.clone())
                })
                .collect()
        };
        let (kb, ko, kt) = (keyed(b), keyed(o), keyed(t));
        let find = |list: &[((String, usize), T)], k: &(String, usize)| {
            list.iter().find(|(x, _)| x == k).map(|(_, v)| v.clone())
        };
        let keys: BTreeSet<(String, usize)> = kb
            .iter()
            .chain(&ko)
            .chain(&kt)
            .map(|(k, _)| k.clone())
            .collect();
        let mut merged: HashMap<(String, usize), T> = HashMap::new();
        for k in &keys {
            let conflict_key = key(k.0.clone());
            let theirs = self.theirs_wins(&conflict_key);
            let value = match (find(&kb, k), find(&ko, k), find(&kt, k)) {
                (Some(vb), Some(vo), Some(vt)) => {
                    let mut clash = false;
                    let v = both(&vb, &vo, &vt, theirs, &mut clash);
                    if clash {
                        self.conflict(
                            conflict_key,
                            format!("{label} {}", k.0),
                            "Both changed it in ways that do not fit together",
                            Vec::new(),
                            false,
                        );
                    }
                    Some(v)
                }
                (Some(vb), Some(vo), None) if vb != vo => {
                    self.conflict(
                        conflict_key,
                        format!("{label} {}", k.0),
                        "Changed here, deleted in theirs",
                        Vec::new(),
                        false,
                    );
                    (!theirs).then_some(vo)
                }
                (Some(vb), None, Some(vt)) if vb != vt => {
                    self.conflict(
                        conflict_key,
                        format!("{label} {}", k.0),
                        "Deleted here, changed in theirs",
                        Vec::new(),
                        false,
                    );
                    theirs.then_some(vt)
                }
                (Some(_), _, _) => None,
                (None, Some(v), _) | (None, None, Some(v)) => Some(v),
                (None, None, None) => None,
            };
            if let Some(v) = value {
                merged.insert(k.clone(), v);
            }
        }
        // Order by position in `keys`: `merge_order` needs Copy items.
        let all: Vec<&(String, usize)> = keys.iter().collect();
        let order = |list: &[((String, usize), T)]| -> Vec<usize> {
            list.iter()
                .map(|(k, _)| all.iter().position(|x| *x == k).unwrap())
                .collect()
        };
        let (lb, lo, lt) = (order(&kb), order(&ko), order(&kt));
        let use_theirs = !reordered(&lb, &lo, &lt) && reordered(&lb, &lt, &lo);
        let (first, second) = if use_theirs { (&lt, &lo) } else { (&lo, &lt) };
        merge_order(first, second, |i| merged.contains_key(all[i]))
            .into_iter()
            .map(|i| merged.remove(all[i]).unwrap())
            .collect()
    }

    fn merge_library(&mut self) {
        let (b, o, t) = (
            &sb(self.base).library,
            &sb(self.ours).library,
            &sb(self.theirs).library,
        );
        let map = |l: &crate::storyboard_library::Library| -> BTreeMap<ItemId, LibraryItem> {
            l.items.iter().map(|i| (i.id, i.clone())).collect()
        };
        let resolutions = self.resolutions;
        let (items, clashes) = merge_map(&map(b), &map(o), &map(t), |id| {
            resolutions.get(&ConflictKey::Library(*id)) == Some(&Resolution::Theirs)
        });
        for id in clashes {
            let name = o
                .item(id)
                .or(t.item(id))
                .map_or_else(String::new, |i| i.name.clone());
            self.conflict(
                ConflictKey::Library(id),
                format!("Library item {name}"),
                "Changed differently, or deleted and changed",
                Vec::new(),
                false,
            );
        }
        let ids = |l: &crate::storyboard_library::Library| {
            l.items.iter().map(|i| i.id).collect::<Vec<_>>()
        };
        let (lb, lo, lt) = (ids(b), ids(o), ids(t));
        let use_theirs = !reordered(&lb, &lo, &lt) && reordered(&lb, &lt, &lo);
        let (first, second) = if use_theirs { (&lt, &lo) } else { (&lo, &lt) };
        self.board.library.items = merge_order(first, second, |id| items.contains_key(&id))
            .into_iter()
            .map(|id| items[&id].clone())
            .collect();
        self.board.library.next_id = o.next_id.max(t.next_id);
    }

    fn finish(mut self) -> Result<BoardMerge, String> {
        let ours_pages: HashMap<PageId, &ProjectPage> =
            self.ours.pages.iter().map(|p| (p.meta.id, p)).collect();
        let theirs_pages: HashMap<PageId, &ProjectPage> =
            self.theirs.pages.iter().map(|p| (p.meta.id, p)).collect();
        let fields: HashSet<CaptionId> = self.board.captions.iter().map(|c| c.id).collect();
        let mut pages = Vec::with_capacity(self.order.len());
        let mut panels = BTreeMap::new();
        for id in &self.order {
            let merged = self.panels.remove(id).unwrap();
            let source = |from: From| match from {
                From::Ours => ours_pages.get(id).or(theirs_pages.get(id)),
                From::Theirs => theirs_pages.get(id).or(ours_pages.get(id)),
            };
            // A kept-both copy has a new ID; its drawing is theirs.
            let original = self.copies.get(id).copied();
            let page = match original {
                Some(src) => theirs_pages[&src],
                None => source(merged.drawing)
                    .copied()
                    .ok_or("A merged panel lost its drawing.")?,
            };
            let graph = match ours_pages.get(id) {
                Some(mine) if original.is_none() => mine.graph.clone(),
                _ => page.graph.clone(),
            };
            let mut panel = merged.panel;
            panel.captions.retain(|f, _| fields.contains(f));
            panels.insert(*id, panel);
            pages.push(ProjectPage {
                meta: merged.meta,
                doc: page.doc.clone(),
                graph,
            });
        }
        if pages.is_empty() {
            return Err("The merge would leave no panels.".into());
        }
        let mut board = self.board;
        board.panels = panels;
        let order: Vec<PageId> = pages.iter().map(|p| p.meta.id).collect();
        board.reconcile(&order);
        let next_page_id = self
            .ours
            .next_page_id
            .max(self.theirs.next_page_id)
            .max(order.iter().max().map_or(0, |m| m + 1));
        let (theirs_board, ours_board) = (sb(self.theirs), sb(self.ours));
        board.versions = ours_board.versions.clone();
        let versions_added = import_versions(&mut board, &mut pages, theirs_board, &theirs_pages);
        let size = (board.settings.width, board.settings.height);
        if pages.iter().any(|p| (p.doc.width, p.doc.height) != size) {
            return Err("The copies use different panel resolutions. Set the same resolution on both, then merge.".into());
        }
        board.validate(&order)?;
        let active = if order.contains(&self.ours.active) {
            self.ours.active
        } else {
            order[0]
        };
        let project = Project {
            kind: self.ours.kind,
            pages,
            active,
            next_page_id,
            storyboard: Some(board),
        };
        project.validate()?;
        let state = |p: &Project, label: &str| BoardState::of_project(p, label);
        let base_state = state(self.base, "Common version");
        let report = BoardMergeReport {
            conflicts: self.conflicts,
            theirs: describe_changes(&base_state, &state(self.theirs, "Theirs")),
            ours: describe_changes(&base_state, &state(self.ours, "Mine")),
            panels: project.pages.len(),
            frames: sb(&project).total_frames(),
            took_theirs: self.took_theirs,
            renumbered: 0,
            versions_added,
        };
        Ok(BoardMerge { project, report })
    }
}

fn ours_panel(v: &View) -> MergedPanel {
    MergedPanel {
        meta: v.meta.clone(),
        drawing: From::Ours,
        panel: v.panel.clone(),
    }
}

fn theirs_panel(v: &View) -> MergedPanel {
    MergedPanel {
        meta: v.meta.clone(),
        drawing: From::Theirs,
        panel: v.panel.clone(),
    }
}

/// Review status merges three ways; notes are a union (matching by ID,
/// author and time), and a note both sides edited differently conflicts.
fn merge_review(
    b: &crate::storyboard::PanelReview,
    o: &crate::storyboard::PanelReview,
    t: &crate::storyboard::PanelReview,
    theirs: bool,
    clash: &mut bool,
) -> crate::storyboard::PanelReview {
    let status = take(&b.status, &o.status, &t.status, theirs, clash);
    let key = |n: &crate::storyboard::ReviewNote| (n.time, n.author.clone(), n.id);
    let map = |r: &crate::storyboard::PanelReview| -> BTreeMap<_, _> {
        r.notes.iter().map(|n| (key(n), n.clone())).collect()
    };
    let (notes, clashes) = merge_map(&map(b), &map(o), &map(t), |_| theirs);
    if !clashes.is_empty() {
        *clash = true;
    }
    let mut used = HashSet::new();
    let mut next = notes.values().map(|n| n.id).max().unwrap_or(0) + 1;
    let notes = notes
        .into_values()
        .map(|mut n| {
            if !used.insert(n.id) {
                n.id = next;
                used.insert(next);
                next += 1;
            }
            n
        })
        .collect();
    crate::storyboard::PanelReview { status, notes }
}

/// Add their board versions this board lacks (same name, time and panels
/// count as one). Each version's drawings are kept in the page histories
/// beside the head, or in a removed panel's kept history. Returns how many
/// were added.
fn import_versions(
    board: &mut Storyboard,
    pages: &mut [ProjectPage],
    theirs: &Storyboard,
    theirs_pages: &HashMap<PageId, &ProjectPage>,
) -> usize {
    let mut added = 0;
    for version in &theirs.versions.list {
        let layout: Vec<_> = version.layout.iter().map(|m| m.id).collect();
        let known = board.versions.list.iter().any(|v| {
            v.name == version.name
                && v.time == version.time
                && v.layout.iter().map(|m| m.id).eq(layout.iter().copied())
        });
        if known || board.versions.list.len() >= MAX_VERSIONS {
            continue;
        }
        let docs: Option<Vec<(PageId, Document)>> = version
            .pages
            .iter()
            .map(|(page, commit)| {
                let graph = theirs_pages
                    .get(page)
                    .map(|p| &p.graph)
                    .or_else(|| theirs.versions.retired.get(page))?;
                Some((*page, graph.commit(*commit)?.doc.clone()))
            })
            .collect();
        let Some(docs) = docs else {
            continue;
        };
        let mut commits = BTreeMap::new();
        for (page, doc) in docs {
            let graph = match pages.iter_mut().find(|p| p.meta.id == page) {
                Some(p) => &mut p.graph,
                None => board
                    .versions
                    .retired
                    .entry(page)
                    .or_insert_with(|| Graph::new(doc.clone(), version.name.clone())),
            };
            commits.insert(page, graph.keep(&doc, &version.name));
        }
        let id = board.versions.next_id.max(1);
        board.versions.next_id = id + 1;
        board
            .versions
            .list
            .push(crate::storyboard_versions::BoardVersion {
                id,
                pages: commits,
                ..version.clone()
            });
        added += 1;
    }
    board.versions.list.sort_by_key(|v| (v.time, v.id));
    added
}

#[cfg(test)]
#[path = "storyboard_merge_tests.rs"]
mod tests;
