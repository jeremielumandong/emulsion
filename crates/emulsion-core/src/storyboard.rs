//! Storyboard projects. Each panel is a project page; this holds what a page
//! cannot: the act → sequence → scene grouping, captions, timing and shot data.
//!
//! Order always comes from the project's page layout. Groups only record
//! membership, and every group must cover a contiguous run of panels, so page
//! moves can never leave the outline and the pages disagreeing: `reconcile`
//! repairs membership after any layout change.
use crate::project::PageId;
pub use crate::storyboard_animatic::{AnimaticFrame, BurnIn, BurnInPosition, RenderArea};
pub use crate::storyboard_naming::{
    CaptionPreset, Naming, Preferences, RenumberScope, ThumbnailGrid,
};
pub use crate::storyboard_stage::{Frame, LightTable, StageGuides};
pub use crate::storyboard_text::{Caption, FindOptions};
pub use crate::timeline::{FrameRate, Timeline, Transition, TransitionKind};
use serde::{Deserialize, Serialize};
use std::collections::{BTreeMap, HashSet};

pub type GroupId = u64;
pub type CaptionId = u64;

/// Longest panel: ten minutes at the highest frame rate.
pub const MAX_PANEL_FRAMES: u32 = 10 * 60 * 120;
pub const MAX_CAPTION_FIELDS: usize = 32;
pub const MAX_CAPTION_CHARS: usize = 4000;
pub const TAG_COLORS: u8 = 8;

#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
pub struct Settings {
    pub width: u32,
    pub height: u32,
    pub frame_rate: FrameRate,
    /// Duration given to new panels.
    pub panel_frames: u32,
}

impl Settings {
    pub fn new(width: u32, height: u32) -> Self {
        Self {
            width,
            height,
            frame_rate: FrameRate::whole(24),
            panel_frames: 48,
        }
    }
    fn validate(&self) -> Result<(), String> {
        if self.width == 0 || self.height == 0 {
            return Err("Storyboard resolution must be at least 1 × 1.".into());
        }
        self.frame_rate.validate()?;
        check_frames(self.panel_frames)
    }
}

fn check_frames(frames: u32) -> Result<(), String> {
    if !(1..=MAX_PANEL_FRAMES).contains(&frames) {
        return Err(format!(
            "Panel duration must be 1–{MAX_PANEL_FRAMES} frames."
        ));
    }
    Ok(())
}

pub(crate) fn check_name(name: &str, what: &str) -> Result<(), String> {
    if name.trim().is_empty() || name.chars().count() > 200 || name.chars().any(char::is_control) {
        return Err(format!("{what} names must be 1–200 characters."));
    }
    Ok(())
}

#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
pub struct Act {
    pub name: String,
}

#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
pub struct Sequence {
    pub act: GroupId,
    pub name: String,
}

#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
pub struct Scene {
    pub sequence: GroupId,
    pub name: String,
    /// Protects every panel in the scene.
    #[serde(default)]
    pub locked: bool,
}

#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
pub struct CaptionField {
    pub id: CaptionId,
    pub name: String,
    pub multiline: bool,
    /// Included in printed and PDF boards.
    pub print: bool,
}

#[derive(Clone, Copy, Debug, Default, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum ShotSize {
    #[default]
    Unset,
    ExtremeWide,
    Wide,
    Full,
    Medium,
    MediumClose,
    CloseUp,
    ExtremeClose,
    Insert,
}

#[derive(Clone, Copy, Debug, Default, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum CameraAngle {
    #[default]
    Unset,
    Eye,
    High,
    Low,
    Overhead,
    Dutch,
    Pov,
}

#[derive(Clone, Copy, Debug, Default, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum PanelStatus {
    #[default]
    Rough,
    Clean,
    Approved,
}

#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
pub struct Panel {
    pub scene: GroupId,
    pub frames: u32,
    #[serde(default)]
    pub captions: BTreeMap<CaptionId, Caption>,
    #[serde(default)]
    pub size: ShotSize,
    #[serde(default)]
    pub angle: CameraAngle,
    #[serde(default)]
    pub status: PanelStatus,
    /// Index into the fixed tag palette.
    #[serde(default)]
    pub tag: Option<u8>,
    /// Refuses drawing, data changes and removal until unlocked.
    #[serde(default)]
    pub locked: bool,
    /// Marks a thumbnail sheet: rough frames drawn in a grid, later turned
    /// into panels. Sheets do not count towards running time.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub thumbnails: Option<ThumbnailGrid>,
    /// How the animatic enters this panel from the one before.
    #[serde(default, skip_serializing_if = "Transition::is_cut")]
    pub transition: Transition,
}

impl Panel {
    pub fn new(scene: GroupId, frames: u32) -> Self {
        Self {
            scene,
            frames,
            captions: BTreeMap::new(),
            size: ShotSize::Unset,
            angle: CameraAngle::Unset,
            status: PanelStatus::Rough,
            tag: None,
            locked: false,
            thumbnails: None,
            transition: Transition::default(),
        }
    }
}

/// Display names, shared by the panel inspector and exports.
pub const SHOT_SIZES: [(ShotSize, &str); 9] = [
    (ShotSize::Unset, "Not set"),
    (ShotSize::ExtremeWide, "Extreme wide"),
    (ShotSize::Wide, "Wide"),
    (ShotSize::Full, "Full"),
    (ShotSize::Medium, "Medium"),
    (ShotSize::MediumClose, "Medium close-up"),
    (ShotSize::CloseUp, "Close-up"),
    (ShotSize::ExtremeClose, "Extreme close-up"),
    (ShotSize::Insert, "Insert"),
];

pub const CAMERA_ANGLES: [(CameraAngle, &str); 7] = [
    (CameraAngle::Unset, "Not set"),
    (CameraAngle::Eye, "Eye level"),
    (CameraAngle::High, "High"),
    (CameraAngle::Low, "Low"),
    (CameraAngle::Overhead, "Overhead"),
    (CameraAngle::Dutch, "Dutch"),
    (CameraAngle::Pov, "Point of view"),
];

pub const PANEL_STATUSES: [(PanelStatus, &str); 3] = [
    (PanelStatus::Rough, "Rough"),
    (PanelStatus::Clean, "Clean"),
    (PanelStatus::Approved, "Approved"),
];

/// The fixed tag palette `Panel::tag` indexes: name and RGB.
pub const TAG_PALETTE: [(&str, u32); TAG_COLORS as usize] = [
    ("Red", 0xE5484D),
    ("Orange", 0xF76B15),
    ("Yellow", 0xFFC53D),
    ("Green", 0x30A46C),
    ("Teal", 0x12A594),
    ("Blue", 0x0090FF),
    ("Purple", 0x8E4EC6),
    ("Grey", 0x8B8D98),
];

/// A level of the outline.
#[derive(Clone, Copy, Debug, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum Level {
    Scene,
    Sequence,
    Act,
}

/// One scene in outline order, with its panels in page order.
#[derive(Clone, Debug, PartialEq)]
pub struct OutlineScene {
    pub act: GroupId,
    pub sequence: GroupId,
    pub scene: GroupId,
    pub panels: Vec<PageId>,
}

#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
pub struct Storyboard {
    pub settings: Settings,
    pub captions: Vec<CaptionField>,
    pub acts: BTreeMap<GroupId, Act>,
    pub sequences: BTreeMap<GroupId, Sequence>,
    pub scenes: BTreeMap<GroupId, Scene>,
    pub panels: BTreeMap<PageId, Panel>,
    /// Allocator shared by groups and caption fields.
    pub next_id: u64,
    #[serde(default)]
    pub naming: Naming,
    /// Layers Smart add carries into the next panel, by name.
    #[serde(default)]
    pub smart_add_layers: Vec<String>,
    /// Safe areas, field guide and overscan drawn on the Stage.
    #[serde(default)]
    pub stage: StageGuides,
    /// The board's colour palette.
    #[serde(default = "default_palette")]
    pub palette: Vec<[u8; 3]>,
    /// The project library of reusable drawings. Its drawings are stored in
    /// the package beside the storyboard data.
    #[serde(
        default,
        skip_serializing_if = "crate::storyboard_library::Library::is_empty"
    )]
    pub library: crate::storyboard_library::Library,
    /// Audio tracks and the sounds they play, timed against the panels.
    #[serde(default, skip_serializing_if = "Timeline::is_empty")]
    pub timeline: Timeline,
}

fn default_palette() -> Vec<[u8; 3]> {
    crate::storyboard_stage::DEFAULT_PALETTE.to_vec()
}

impl Storyboard {
    /// One act, sequence and scene holding every panel, with the usual caption
    /// fields.
    pub fn new(settings: Settings, panels: &[PageId]) -> Self {
        Self::with_preferences(settings, panels, &Preferences::default())
    }

    /// Like `new`, starting from the user's storyboard preferences: naming,
    /// caption fields, Smart add layers and the duration of new panels.
    pub fn with_preferences(
        mut settings: Settings,
        panels: &[PageId],
        preferences: &Preferences,
    ) -> Self {
        let frames = (preferences.panel_seconds * settings.frame_rate.fps()).round();
        settings.panel_frames = (frames as u32).clamp(1, MAX_PANEL_FRAMES);
        let frames = settings.panel_frames;
        let captions: Vec<_> = preferences
            .captions
            .iter()
            .take(MAX_CAPTION_FIELDS)
            .zip(1..)
            .map(|(preset, id)| CaptionField {
                id,
                name: preset.name.clone(),
                multiline: preset.multiline,
                print: preset.print,
            })
            .collect();
        let mut board = Self {
            settings,
            next_id: captions.len() as u64 + 1,
            captions,
            acts: BTreeMap::new(),
            sequences: BTreeMap::new(),
            scenes: BTreeMap::new(),
            panels: BTreeMap::new(),
            naming: preferences.naming.clone(),
            smart_add_layers: preferences.smart_add_layers.clone(),
            stage: preferences.stage.clone(),
            palette: preferences.palette.clone(),
            library: Default::default(),
            timeline: Timeline::default(),
        };
        let scene = board.add_default_groups();
        if let Some(scene) = board.scenes.get_mut(&scene) {
            scene.name = board.naming.scene_name(0);
        }
        board.panels = panels
            .iter()
            .map(|&id| (id, Panel::new(scene, frames)))
            .collect();
        board
    }

    fn allocate(&mut self) -> u64 {
        let id = self.next_id;
        self.next_id += 1;
        id
    }

    fn add_act(&mut self) -> GroupId {
        let id = self.allocate();
        let name = format!("Act {}", self.acts.len() + 1);
        self.acts.insert(id, Act { name });
        id
    }
    fn add_sequence(&mut self, act: GroupId) -> GroupId {
        let id = self.allocate();
        let name = format!("Sequence {}", self.sequences.len() + 1);
        self.sequences.insert(id, Sequence { act, name });
        id
    }
    pub(crate) fn add_scene(&mut self, sequence: GroupId) -> GroupId {
        let id = self.allocate();
        let name = self.naming.scene_name(self.scenes.len());
        self.scenes.insert(
            id,
            Scene {
                sequence,
                name,
                locked: false,
            },
        );
        id
    }
    /// A fresh act, sequence and scene; returns the scene.
    fn add_default_groups(&mut self) -> GroupId {
        let act = self.add_act();
        let sequence = self.add_sequence(act);
        self.add_scene(sequence)
    }

    /// A new blank panel at the project resolution: a white background layer.
    pub fn blank_panel(&self) -> Result<crate::Document, String> {
        crate::creation::CanvasSpec {
            name: "Panel".into(),
            kind: crate::creation::CanvasKind::Storyboard,
            width: f64::from(self.settings.width),
            height: f64::from(self.settings.height),
            ..Default::default()
        }
        .create()
    }

    pub fn caption(&self, name: &str) -> Option<CaptionId> {
        self.captions
            .iter()
            .find(|c| c.name.eq_ignore_ascii_case(name))
            .map(|c| c.id)
    }

    /// Scenes in page order. `layout` must satisfy `validate`.
    pub fn outline(&self, layout: &[PageId]) -> Vec<OutlineScene> {
        let mut out: Vec<OutlineScene> = Vec::new();
        for &id in layout {
            let Some(panel) = self.panels.get(&id) else {
                continue;
            };
            match out.last_mut() {
                Some(last) if last.scene == panel.scene => last.panels.push(id),
                _ => {
                    let sequence = self.scenes[&panel.scene].sequence;
                    out.push(OutlineScene {
                        act: self.sequences[&sequence].act,
                        sequence,
                        scene: panel.scene,
                        panels: vec![id],
                    });
                }
            }
        }
        out
    }

    /// Rename an act, sequence or scene.
    pub fn rename(&mut self, group: GroupId, name: &str) -> Result<(), String> {
        let name = name.trim().to_string();
        let slot = if let Some(act) = self.acts.get_mut(&group) {
            &mut act.name
        } else if let Some(sequence) = self.sequences.get_mut(&group) {
            &mut sequence.name
        } else if let Some(scene) = self.scenes.get_mut(&group) {
            &mut scene.name
        } else {
            return Err("No act, sequence or scene has that ID.".into());
        };
        check_name(&name, "Group")?;
        *slot = name;
        Ok(())
    }

    /// Start a new group at `panel`: it and the rest of its enclosing group of
    /// that level move into the new group. Splitting a sequence or act also
    /// starts a scene (and sequence) at `panel` when it is mid-group. Returns
    /// the new group.
    pub fn split(
        &mut self,
        layout: &[PageId],
        panel: PageId,
        level: Level,
        name: Option<&str>,
    ) -> Result<GroupId, String> {
        if let Some(name) = name {
            check_name(name, "Group")?;
        }
        let scene = self
            .panels
            .get(&panel)
            .ok_or("Panel does not exist.")?
            .scene;
        let starts_scene = self.outline(layout).iter().any(|s| s.panels[0] == panel);
        if !starts_scene {
            let sequence = self.scenes[&scene].sequence;
            let new = self.add_scene(sequence);
            if self.naming.insert_letters {
                let previous = &self.scenes[&scene].name;
                let taken = |n: &str| self.scenes.values().any(|s| s.name == n);
                if let Some(name) = crate::storyboard_naming::inserted_name(previous, taken) {
                    self.scenes.get_mut(&new).unwrap().name = name;
                }
            }
            let at = layout.iter().position(|id| *id == panel).unwrap();
            for id in &layout[at..] {
                let p = self.panels.get_mut(id).unwrap();
                if p.scene != scene {
                    break;
                }
                p.scene = new;
            }
            if level == Level::Scene {
                if let Some(name) = name {
                    self.scenes.get_mut(&new).unwrap().name = name.into();
                }
                return Ok(new);
            }
        } else if level == Level::Scene {
            return Err("That panel already starts a scene.".into());
        }
        let outline = self.outline(layout);
        let at = outline.iter().position(|s| s.panels[0] == panel).unwrap();
        let first = &outline[at];
        let starts_sequence = at == 0 || outline[at - 1].sequence != first.sequence;
        if !starts_sequence {
            let new = self.add_sequence(first.act);
            for s in outline[at..]
                .iter()
                .take_while(|s| s.sequence == first.sequence)
            {
                self.scenes.get_mut(&s.scene).unwrap().sequence = new;
            }
            if level == Level::Sequence {
                if let Some(name) = name {
                    self.sequences.get_mut(&new).unwrap().name = name.into();
                }
                return Ok(new);
            }
        } else if level == Level::Sequence {
            return Err("That panel already starts a sequence.".into());
        }
        let outline = self.outline(layout);
        let first = &outline[at];
        if at == 0 || outline[at - 1].act != first.act {
            return Err("That panel already starts an act.".into());
        }
        let new = self.add_act();
        let mut moved = HashSet::new();
        for s in outline[at..].iter().take_while(|s| s.act == first.act) {
            if moved.insert(s.sequence) {
                self.sequences.get_mut(&s.sequence).unwrap().act = new;
            }
        }
        if let Some(name) = name {
            self.acts.get_mut(&new).unwrap().name = name.into();
        }
        Ok(new)
    }

    /// Total running time in frames. Thumbnail sheets do not count.
    pub fn total_frames(&self) -> u64 {
        self.panels
            .values()
            .filter(|p| p.thumbnails.is_none())
            .map(|p| u64::from(p.frames))
            .sum()
    }

    /// Whether the panel, or its scene, is locked.
    pub fn is_locked(&self, panel: PageId) -> bool {
        self.panels
            .get(&panel)
            .is_some_and(|p| p.locked || self.scenes.get(&p.scene).is_some_and(|s| s.locked))
    }

    /// Check that `next` keeps every lock of this board: a locked panel's
    /// data stays the same, and protection only ends when the panel's own
    /// lock or its scene's lock is cleared, never by regrouping. Regrouping
    /// cannot pull existing unlocked panels into a locked scene either.
    pub fn check_locks_kept(&self, next: &Storyboard) -> Result<(), String> {
        for (id, before) in &self.panels {
            let Some(after) = next.panels.get(id) else {
                continue;
            };
            if !self.is_locked(*id) {
                if after.scene != before.scene && next.is_locked(*id) && !after.locked {
                    return Err(
                        "That would pull panels into a locked scene. Unlock it first.".into(),
                    );
                }
                continue;
            }
            if next.is_locked(*id) {
                let unchanged = Panel {
                    scene: before.scene,
                    locked: before.locked,
                    ..after.clone()
                } == *before;
                if !unchanged {
                    return Err("That panel is locked. Unlock it to change it.".into());
                }
                continue;
            }
            let own = before.locked && !after.locked;
            let scene = self.scenes[&before.scene].locked
                && next.scenes.get(&before.scene).is_some_and(|s| !s.locked);
            if !own && !scene {
                return Err(
                    "That would move a locked panel out of its locked scene. Unlock it first."
                        .into(),
                );
            }
        }
        Ok(())
    }

    /// Merge a group into the group of the same level just before it, so it
    /// disappears. Returns the group that absorbed it.
    pub fn join(&mut self, layout: &[PageId], group: GroupId) -> Result<GroupId, String> {
        let outline = self.outline(layout);
        let level_of = |s: &OutlineScene| -> GroupId {
            if self.scenes.contains_key(&group) {
                s.scene
            } else if self.sequences.contains_key(&group) {
                s.sequence
            } else {
                s.act
            }
        };
        if !self.scenes.contains_key(&group)
            && !self.sequences.contains_key(&group)
            && !self.acts.contains_key(&group)
        {
            return Err("No act, sequence or scene has that ID.".into());
        }
        let first = outline
            .iter()
            .position(|s| level_of(s) == group)
            .ok_or("That group has no panels.")?;
        let into = outline[..first]
            .iter()
            .rev()
            .map(level_of)
            .next()
            .ok_or("The first group has nothing before it to join.")?;
        if self.scenes.contains_key(&group) {
            for panel in self.panels.values_mut().filter(|p| p.scene == group) {
                panel.scene = into;
            }
            self.scenes.remove(&group);
        } else if self.sequences.contains_key(&group) {
            for scene in self.scenes.values_mut().filter(|s| s.sequence == group) {
                scene.sequence = into;
            }
            self.sequences.remove(&group);
        } else {
            for sequence in self.sequences.values_mut().filter(|s| s.act == group) {
                sequence.act = into;
            }
            self.acts.remove(&group);
        }
        self.reconcile(layout);
        Ok(into)
    }

    /// Rename scenes by the naming rules: scene `i` of the outline takes
    /// number `i`, so renumbering part of the board agrees with the whole.
    /// Returns new panel names when `panels` is set. Locked scenes and panels
    /// keep their names.
    pub fn renumber(
        &mut self,
        layout: &[PageId],
        scope: &RenumberScope,
        scenes: bool,
        panels: bool,
    ) -> Result<BTreeMap<PageId, String>, String> {
        if let RenumberScope::Groups(groups) = scope {
            if groups.is_empty() {
                return Err("Choose at least one group to renumber.".into());
            }
            for group in groups {
                if !self.scenes.contains_key(group)
                    && !self.sequences.contains_key(group)
                    && !self.acts.contains_key(group)
                {
                    return Err("No act, sequence or scene has that ID.".into());
                }
            }
        }
        let in_scope = |s: &OutlineScene| match scope {
            RenumberScope::All => true,
            RenumberScope::Groups(groups) => [s.scene, s.sequence, s.act]
                .iter()
                .any(|g| groups.contains(g)),
        };
        let mut names = BTreeMap::new();
        let mut number = 0;
        for (index, outline) in self.outline(layout).iter().enumerate() {
            let selected = in_scope(outline);
            if selected && scenes && !self.scenes[&outline.scene].locked {
                self.scenes.get_mut(&outline.scene).unwrap().name = self.naming.scene_name(index);
            }
            if self.naming.panels_per_scene {
                number = 0;
            }
            for &panel in &outline.panels {
                number += 1;
                if selected && panels && !self.is_locked(panel) {
                    names.insert(panel, self.naming.panel_name(number));
                }
            }
        }
        Ok(names)
    }

    /// Add a caption field at the end; returns its ID.
    pub fn add_caption_field(
        &mut self,
        name: &str,
        multiline: bool,
        print: bool,
    ) -> Result<CaptionId, String> {
        if self.captions.len() >= MAX_CAPTION_FIELDS {
            return Err(format!("Use at most {MAX_CAPTION_FIELDS} caption fields."));
        }
        let name = name.trim();
        check_name(name, "Caption field")?;
        if self.caption(name).is_some() {
            return Err("Caption field names must be unique.".into());
        }
        let id = self.allocate();
        self.captions.push(CaptionField {
            id,
            name: name.into(),
            multiline,
            print,
        });
        Ok(id)
    }

    /// Remove a caption field and its text on every panel.
    pub fn remove_caption_field(&mut self, id: CaptionId) -> Result<(), String> {
        let at = self
            .captions
            .iter()
            .position(|c| c.id == id)
            .ok_or("No caption field has that ID.")?;
        self.captions.remove(at);
        for panel in self.panels.values_mut() {
            panel.captions.remove(&id);
        }
        Ok(())
    }

    /// Move a caption field to `index` in the field order.
    pub fn move_caption_field(&mut self, id: CaptionId, index: usize) -> Result<(), String> {
        let at = self
            .captions
            .iter()
            .position(|c| c.id == id)
            .ok_or("No caption field has that ID.")?;
        if index >= self.captions.len() {
            return Err("Caption field position is out of range.".into());
        }
        let field = self.captions.remove(at);
        self.captions.insert(index, field);
        Ok(())
    }

    /// Every match of `query` in captions, in page order, then field order.
    /// `field` limits the search to one caption field.
    pub fn find(
        &self,
        layout: &[PageId],
        query: &str,
        field: Option<CaptionId>,
        options: FindOptions,
    ) -> Vec<(PageId, CaptionId, std::ops::Range<usize>)> {
        let mut out = Vec::new();
        for id in layout {
            let Some(panel) = self.panels.get(id) else {
                continue;
            };
            for caption in &self.captions {
                if field.is_some_and(|f| f != caption.id) {
                    continue;
                }
                if let Some(text) = panel.captions.get(&caption.id) {
                    for range in crate::storyboard_text::find(&text.text, query, options) {
                        out.push((*id, caption.id, range));
                    }
                }
            }
        }
        out
    }

    /// Replace every match on unlocked panels, keeping caption formatting.
    /// Returns how many were replaced and how many locked panels were skipped.
    pub fn replace_all(
        &mut self,
        layout: &[PageId],
        query: &str,
        replacement: &str,
        field: Option<CaptionId>,
        options: FindOptions,
    ) -> (usize, usize) {
        let (mut replaced, mut skipped) = (0, HashSet::new());
        for (panel, caption, range) in self.find(layout, query, field, options).into_iter().rev() {
            if self.is_locked(panel) {
                skipped.insert(panel);
                continue;
            }
            let text = self
                .panels
                .get_mut(&panel)
                .unwrap()
                .captions
                .get_mut(&caption)
                .unwrap();
            text.replace_range(range, replacement);
            replaced += 1;
        }
        for panel in self.panels.values_mut() {
            panel.captions.retain(|_, c| !c.text.is_empty());
        }
        (replaced, skipped.len())
    }

    pub fn validate(&self, layout: &[PageId]) -> Result<(), String> {
        self.settings.validate()?;
        self.naming.validate()?;
        self.stage.validate()?;
        self.timeline.validate()?;
        crate::storyboard_stage::validate_palette(&self.palette)?;
        self.library.validate()?;
        if self.smart_add_layers.len() > 64
            || self
                .smart_add_layers
                .iter()
                .any(|n| n.trim().is_empty() || n.chars().count() > 200)
        {
            return Err("Smart add takes up to 64 layer names of 1–200 characters.".into());
        }
        if self.captions.len() > MAX_CAPTION_FIELDS {
            return Err(format!("Use at most {MAX_CAPTION_FIELDS} caption fields."));
        }
        let mut ids = HashSet::new();
        let groups = self
            .acts
            .keys()
            .chain(self.sequences.keys())
            .chain(self.scenes.keys())
            .chain(self.captions.iter().map(|c| &c.id));
        for &id in groups {
            if id == 0 || id >= self.next_id || !ids.insert(id) {
                return Err("Storyboard IDs must be unique and allocated.".into());
            }
        }
        let mut names = HashSet::new();
        for caption in &self.captions {
            check_name(&caption.name, "Caption field")?;
            if !names.insert(caption.name.to_lowercase()) {
                return Err("Caption field names must be unique.".into());
            }
        }
        for act in self.acts.values() {
            check_name(&act.name, "Act")?;
        }
        for sequence in self.sequences.values() {
            check_name(&sequence.name, "Sequence")?;
            if !self.acts.contains_key(&sequence.act) {
                return Err("A sequence refers to a missing act.".into());
            }
        }
        for scene in self.scenes.values() {
            check_name(&scene.name, "Scene")?;
            if !self.sequences.contains_key(&scene.sequence) {
                return Err("A scene refers to a missing sequence.".into());
            }
        }
        if self.panels.len() != layout.len()
            || layout.iter().any(|id| !self.panels.contains_key(id))
        {
            return Err("Every page must be exactly one storyboard panel.".into());
        }
        for panel in self.panels.values() {
            check_frames(panel.frames)?;
            if !self.scenes.contains_key(&panel.scene) {
                return Err("A panel refers to a missing scene.".into());
            }
            if panel.tag.is_some_and(|t| t >= TAG_COLORS) {
                return Err("Panel tag colour is out of range.".into());
            }
            if panel.transition.frames > panel.frames {
                return Err("A transition cannot be longer than its panel.".into());
            }
            if let Some(grid) = panel.thumbnails {
                grid.validate(self.settings.width, self.settings.height)?;
            }
            for (id, text) in &panel.captions {
                let Some(field) = self.captions.iter().find(|c| c.id == *id) else {
                    return Err("A caption refers to a missing caption field.".into());
                };
                text.validate()?;
                let text = &text.text;
                if text.chars().count() > MAX_CAPTION_CHARS {
                    return Err(format!(
                        "Captions are limited to {MAX_CAPTION_CHARS} characters."
                    ));
                }
                if text
                    .chars()
                    .any(|c| c.is_control() && !(field.multiline && matches!(c, '\n' | '\t')))
                {
                    return Err(format!("{} contains control characters.", field.name));
                }
            }
        }
        // Each group covers one contiguous run and none is empty.
        let outline = self.outline(layout);
        for (level, keys) in [
            ("scene", outline.iter().map(|s| s.scene).collect::<Vec<_>>()),
            ("sequence", outline.iter().map(|s| s.sequence).collect()),
            ("act", outline.iter().map(|s| s.act).collect()),
        ] {
            let mut seen = HashSet::new();
            let mut previous = None;
            for key in keys {
                if previous != Some(key) && !seen.insert(key) {
                    return Err(format!(
                        "Each {level} must be one continuous run of panels."
                    ));
                }
                previous = Some(key);
            }
            let count = match level {
                "scene" => self.scenes.len(),
                "sequence" => self.sequences.len(),
                _ => self.acts.len(),
            };
            if seen.len() != count {
                return Err(format!("Every {level} must contain at least one panel."));
            }
        }
        Ok(())
    }

    /// Repair membership after the page layout changed: drop removed panels,
    /// give new panels the scene before them, keep every group contiguous and
    /// remove groups left empty. Panels dropped between two parts of a scene
    /// join that scene; likewise scenes within a sequence and sequences
    /// within an act.
    pub fn reconcile(&mut self, layout: &[PageId]) {
        let present: HashSet<_> = layout.iter().copied().collect();
        self.panels.retain(|id, _| present.contains(id));
        let known = layout
            .iter()
            .filter_map(|id| self.panels.get(id))
            .map(|p| p.scene)
            .chain(self.scenes.keys().copied())
            .find(|scene| self.scenes.contains_key(scene));
        let mut current = match known {
            Some(scene) => scene,
            None => self.add_default_groups(),
        };
        let frames = self.settings.panel_frames;
        let mut scenes = Vec::with_capacity(layout.len());
        for &id in layout {
            let panel = self
                .panels
                .entry(id)
                .or_insert_with(|| Panel::new(current, frames));
            if !self.scenes.contains_key(&panel.scene) {
                panel.scene = current;
            }
            current = panel.scene;
            scenes.push(current);
        }
        for (id, scene) in layout.iter().zip(contiguous(&scenes)) {
            self.panels.get_mut(id).unwrap().scene = scene;
        }
        let scene_order = runs(layout.iter().map(|id| self.panels[id].scene));
        self.scenes.retain(|id, _| scene_order.contains(id));

        let known = scene_order
            .iter()
            .map(|id| self.scenes[id].sequence)
            .find(|sequence| self.sequences.contains_key(sequence));
        let mut current = match known {
            Some(sequence) => sequence,
            None => {
                let act = self.acts.keys().next().copied();
                let act = act.unwrap_or_else(|| self.add_act());
                self.add_sequence(act)
            }
        };
        let mut sequences = Vec::with_capacity(scene_order.len());
        for id in &scene_order {
            let scene = self.scenes.get_mut(id).unwrap();
            if !self.sequences.contains_key(&scene.sequence) {
                scene.sequence = current;
            }
            current = scene.sequence;
            sequences.push(current);
        }
        for (id, sequence) in scene_order.iter().zip(contiguous(&sequences)) {
            self.scenes.get_mut(id).unwrap().sequence = sequence;
        }
        let sequence_order = runs(scene_order.iter().map(|id| self.scenes[id].sequence));
        self.sequences.retain(|id, _| sequence_order.contains(id));

        let known = sequence_order
            .iter()
            .map(|id| self.sequences[id].act)
            .find(|act| self.acts.contains_key(act));
        let mut current = match known {
            Some(act) => act,
            None => self.add_act(),
        };
        let mut acts = Vec::with_capacity(sequence_order.len());
        for id in &sequence_order {
            let sequence = self.sequences.get_mut(id).unwrap();
            if !self.acts.contains_key(&sequence.act) {
                sequence.act = current;
            }
            current = sequence.act;
            acts.push(current);
        }
        for (id, act) in sequence_order.iter().zip(contiguous(&acts)) {
            self.sequences.get_mut(id).unwrap().act = act;
        }
        let acts: HashSet<_> = self.sequences.values().map(|s| s.act).collect();
        self.acts.retain(|id, _| acts.contains(id));
    }
}

/// Distinct keys in order of their runs.
fn runs(keys: impl Iterator<Item = GroupId>) -> Vec<GroupId> {
    let mut out: Vec<GroupId> = Vec::new();
    for key in keys {
        if out.last() != Some(&key) {
            out.push(key);
        }
    }
    out
}

/// Regroup so that every group is one run: whatever lies between two runs of
/// the same group joins it.
fn contiguous(keys: &[GroupId]) -> Vec<GroupId> {
    let mut stack: Vec<(GroupId, Vec<usize>)> = Vec::new();
    for (index, &key) in keys.iter().enumerate() {
        match stack.iter().rposition(|(k, _)| *k == key) {
            Some(at) => {
                let absorbed: Vec<usize> =
                    stack.drain(at + 1..).flat_map(|(_, items)| items).collect();
                let run = &mut stack[at].1;
                run.extend(absorbed);
                run.push(index);
            }
            None => stack.push((key, vec![index])),
        }
    }
    let mut out = vec![0; keys.len()];
    for (key, items) in stack {
        for index in items {
            out[index] = key;
        }
    }
    out
}

#[cfg(test)]
mod tests {
    use super::*;

    fn board(panels: &[PageId]) -> Storyboard {
        Storyboard::new(Settings::new(1920, 1080), panels)
    }

    /// Put `panels` in a new scene, sequence and act.
    fn split(board: &mut Storyboard, panels: &[PageId]) -> GroupId {
        let scene = board.add_default_groups();
        for id in panels {
            board.panels.get_mut(id).unwrap().scene = scene;
        }
        scene
    }

    #[test]
    fn new_boards_are_valid_with_default_captions_and_timing() {
        let b = board(&[1, 2, 3]);
        b.validate(&[1, 2, 3]).unwrap();
        assert_eq!(b.outline(&[1, 2, 3]).len(), 1);
        assert_eq!(b.total_frames(), 144);
        assert!(b.caption("dialogue").is_some());
        assert_eq!(b.settings.frame_rate.frames_to_ms(48), 2000.);
        assert!(b.validate(&[1, 2]).is_err());
    }

    #[test]
    fn frame_rates_accept_whole_and_ntsc_only() {
        for rate in FrameRate::PRESETS {
            rate.validate().unwrap();
        }
        assert_eq!(FrameRate::ntsc(24).label(), "23.976 fps");
        assert!(FrameRate { num: 24, den: 2 }.validate().is_err());
        assert!(FrameRate::whole(0).validate().is_err());
        assert!(FrameRate::whole(240).validate().is_err());
    }

    #[test]
    fn groups_must_be_contiguous_and_non_empty() {
        let mut b = board(&[1, 2, 3]);
        split(&mut b, &[1, 3]);
        assert!(b.validate(&[1, 2, 3]).unwrap_err().contains("continuous"));
        b.validate(&[1, 3, 2]).unwrap();
        let mut b = board(&[1]);
        b.add_default_groups();
        assert!(b.validate(&[1]).unwrap_err().contains("at least one panel"));
    }

    #[test]
    fn captions_are_validated_against_their_fields() {
        let mut b = board(&[1]);
        let dialogue = b.caption("Dialogue").unwrap();
        let slug = b.caption("Slugging").unwrap();
        let panel = b.panels.get_mut(&1).unwrap();
        panel.captions.insert(dialogue, "Line one\nLine two".into());
        b.validate(&[1]).unwrap();
        b.panels
            .get_mut(&1)
            .unwrap()
            .captions
            .insert(slug, "a\nb".into());
        assert!(b.validate(&[1]).is_err());
        b.panels
            .get_mut(&1)
            .unwrap()
            .captions
            .insert(slug, "ok".into());
        b.panels
            .get_mut(&1)
            .unwrap()
            .captions
            .insert(99, "x".into());
        assert!(b.validate(&[1]).is_err());
    }

    #[test]
    fn reconcile_follows_inserts_removals_and_moves() {
        let mut b = board(&[1, 2, 3]);
        let second = split(&mut b, &[3]);
        // A new page joins the scene before it.
        b.reconcile(&[1, 2, 4, 3]);
        b.validate(&[1, 2, 4, 3]).unwrap();
        assert_eq!(b.panels[&4].scene, b.panels[&1].scene);
        // A page moved into the middle of another scene joins that scene.
        b.reconcile(&[1, 3, 2, 4]);
        b.validate(&[1, 3, 2, 4]).unwrap();
        assert_eq!(b.panels[&3].scene, b.panels[&1].scene);
        assert!(!b.scenes.contains_key(&second));
        assert_eq!((b.acts.len(), b.sequences.len(), b.scenes.len()), (1, 1, 1));
        // Removing pages drops their panels.
        b.reconcile(&[2]);
        b.validate(&[2]).unwrap();
        assert_eq!(b.panels.len(), 1);
    }

    #[test]
    fn reconcile_keeps_whole_groups_that_move_together() {
        let mut b = board(&[1, 2, 3, 4]);
        let scene = split(&mut b, &[3, 4]);
        b.reconcile(&[3, 4, 1, 2]);
        b.validate(&[3, 4, 1, 2]).unwrap();
        assert_eq!(b.outline(&[3, 4, 1, 2])[0].scene, scene);
        assert_eq!(b.acts.len(), 2);
    }

    #[test]
    fn reconcile_recovers_from_unknown_groups() {
        let mut b = board(&[1]);
        b.scenes.clear();
        b.sequences.clear();
        b.acts.clear();
        b.panels.clear();
        b.reconcile(&[7, 8]);
        b.validate(&[7, 8]).unwrap();
        assert_eq!(b.panels.len(), 2);
    }

    #[test]
    fn reconcile_repairs_dangling_parents() {
        let mut b = board(&[1, 2]);
        b.sequences.clear();
        b.acts.clear();
        b.panels.get_mut(&2).unwrap().scene = 999;
        b.reconcile(&[1, 2]);
        b.validate(&[1, 2]).unwrap();
        assert_eq!((b.acts.len(), b.sequences.len(), b.scenes.len()), (1, 1, 1));
    }

    #[test]
    fn splitting_starts_groups_mid_scene_and_at_boundaries() {
        let layout = [1, 2, 3, 4, 5];
        let mut b = board(&layout);
        let scene = b.split(&layout, 3, Level::Scene, Some("2A")).unwrap();
        b.validate(&layout).unwrap();
        assert_eq!(b.scenes[&scene].name, "2A");
        assert_eq!(b.outline(&layout)[1].panels, [3, 4, 5]);
        assert!(b.split(&layout, 3, Level::Scene, None).is_err());
        // A sequence split mid-scene also splits the scene.
        let sequence = b.split(&layout, 5, Level::Sequence, Some("Chase")).unwrap();
        b.validate(&layout).unwrap();
        let outline = b.outline(&layout);
        assert_eq!(outline.len(), 3);
        assert_eq!(outline[2].sequence, sequence);
        assert_eq!(outline[1].panels, [3, 4]);
        let act = b.split(&layout, 3, Level::Act, None).unwrap();
        b.validate(&layout).unwrap();
        let outline = b.outline(&layout);
        assert!(outline[1..].iter().all(|s| s.act == act));
        assert_eq!(b.sequences.len(), 3);
        assert!(b.split(&layout, 1, Level::Act, None).is_err());
        b.rename(act, "Act Two").unwrap();
        assert!(b.rename(999, "x").is_err());
        assert!(b.rename(act, " ").is_err());
    }

    #[test]
    fn regrouping_absorbs_what_lies_between_two_runs_of_a_group() {
        assert_eq!(contiguous(&[1, 2, 1, 1]), [1, 1, 1, 1]);
        assert_eq!(contiguous(&[1, 2, 1, 2]), [1, 1, 1, 2]);
        assert_eq!(contiguous(&[1, 2, 3, 2, 1]), [1, 1, 1, 1, 1]);
        assert_eq!(contiguous(&[2, 2, 1, 1]), [2, 2, 1, 1]);
        assert_eq!(runs([5, 5, 3, 5].into_iter()), [5, 3, 5]);
    }

    #[test]
    fn serde_round_trips() {
        let mut b = board(&[1, 2]);
        b.panels.get_mut(&2).unwrap().size = ShotSize::CloseUp;
        let json = serde_json::to_string(&b).unwrap();
        let back: Storyboard = serde_json::from_str(&json).unwrap();
        assert_eq!(back, b);
    }
}
