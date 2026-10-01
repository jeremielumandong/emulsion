//! Storyboard projects. Each panel is a project page; this holds what a page
//! cannot: the act → sequence → scene grouping, captions, timing and shot data.
//!
//! Order always comes from the project's page layout. Groups only record
//! membership, and every group must cover a contiguous run of panels, so page
//! moves can never leave the outline and the pages disagreeing: `reconcile`
//! repairs membership after any layout change.
use crate::project::PageId;
use serde::{Deserialize, Serialize};
use std::collections::{BTreeMap, HashSet};

pub type GroupId = u64;
pub type CaptionId = u64;

/// Longest panel: ten minutes at the highest frame rate.
pub const MAX_PANEL_FRAMES: u32 = 10 * 60 * 120;
pub const MAX_CAPTION_FIELDS: usize = 32;
pub const MAX_CAPTION_CHARS: usize = 4000;
pub const TAG_COLORS: u8 = 8;

#[derive(Clone, Copy, Debug, PartialEq, Eq, Serialize, Deserialize)]
pub struct FrameRate {
    pub num: u32,
    pub den: u32,
}

impl FrameRate {
    pub const PRESETS: [Self; 9] = [
        Self::ntsc(24),
        Self::whole(24),
        Self::whole(25),
        Self::ntsc(30),
        Self::whole(30),
        Self::whole(48),
        Self::whole(50),
        Self::ntsc(60),
        Self::whole(60),
    ];
    pub const fn whole(fps: u32) -> Self {
        Self { num: fps, den: 1 }
    }
    /// The NTSC rate just below `fps`, such as 23.976 for 24.
    pub const fn ntsc(fps: u32) -> Self {
        Self {
            num: fps * 1000,
            den: 1001,
        }
    }
    pub fn fps(self) -> f64 {
        f64::from(self.num) / f64::from(self.den)
    }
    pub fn validate(self) -> Result<(), String> {
        if !matches!(self.den, 1 | 1001) || self.num == 0 || !(1. ..=120.).contains(&self.fps()) {
            return Err("Frame rate must be 1–120 fps, whole or NTSC (×1000/1001).".into());
        }
        Ok(())
    }
    pub fn frames_to_ms(self, frames: u32) -> f64 {
        f64::from(frames) * 1000. / self.fps()
    }
    pub fn label(self) -> String {
        if self.den == 1 {
            format!("{} fps", self.num)
        } else {
            format!("{:.3} fps", self.fps())
        }
    }
}

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

fn check_name(name: &str, what: &str) -> Result<(), String> {
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
    pub captions: BTreeMap<CaptionId, String>,
    #[serde(default)]
    pub size: ShotSize,
    #[serde(default)]
    pub angle: CameraAngle,
    #[serde(default)]
    pub status: PanelStatus,
    /// Index into the fixed tag palette.
    #[serde(default)]
    pub tag: Option<u8>,
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
        }
    }
}

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
}

impl Storyboard {
    /// One act, sequence and scene holding every panel, with the usual caption
    /// fields.
    pub fn new(settings: Settings, panels: &[PageId]) -> Self {
        let frames = settings.panel_frames;
        let captions = [
            ("Action", true, true),
            ("Dialogue", true, true),
            ("Slugging", false, true),
            ("Notes", true, false),
        ]
        .into_iter()
        .zip(1..)
        .map(|((name, multiline, print), id)| CaptionField {
            id,
            name: name.into(),
            multiline,
            print,
        })
        .collect();
        let mut board = Self {
            settings,
            captions,
            acts: BTreeMap::new(),
            sequences: BTreeMap::new(),
            scenes: BTreeMap::new(),
            panels: BTreeMap::new(),
            next_id: 5,
        };
        let scene = board.add_default_groups();
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
    fn add_scene(&mut self, sequence: GroupId) -> GroupId {
        let id = self.allocate();
        let name = (self.scenes.len() + 1).to_string();
        self.scenes.insert(id, Scene { sequence, name });
        id
    }
    /// A fresh act, sequence and scene; returns the scene.
    fn add_default_groups(&mut self) -> GroupId {
        let act = self.add_act();
        let sequence = self.add_sequence(act);
        self.add_scene(sequence)
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

    /// Total running time in frames.
    pub fn total_frames(&self) -> u64 {
        self.panels.values().map(|p| u64::from(p.frames)).sum()
    }

    pub fn validate(&self, layout: &[PageId]) -> Result<(), String> {
        self.settings.validate()?;
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
            for (id, text) in &panel.captions {
                let Some(field) = self.captions.iter().find(|c| c.id == *id) else {
                    return Err("A caption refers to a missing caption field.".into());
                };
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
