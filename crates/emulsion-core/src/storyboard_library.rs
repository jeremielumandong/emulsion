//! The storyboard library: reusable drawings such as characters, props and
//! backgrounds. A **layer item** holds one or more layers taken from a panel,
//! at their positions in the frame; placing it puts copies on top of the
//! active panel. A **panel item** holds a whole panel; placing it adds a new
//! panel after the active one. A **scene item** holds a whole scene; placing
//! it adds a new scene after the active panel's scene.
//!
//! Panel and scene items keep their animation (`ItemAnimation`): a panel
//! item saved from an animated panel keeps its duration, layer keyframes,
//! layer comps and the scene camera's keys over it, re-timed to the panel;
//! a scene item keeps every panel's timing, captions, keyframes and comps
//! and the scene camera. Whole documents are copied, so layer IDs in the
//! keyframes and comps stay valid. Items saved before animation have none
//! and place as they always did.
//!
//! The project library lives on the `Storyboard`, so it travels with the
//! `.emu` file and its changes share project Undo like every other storyboard
//! edit. The personal library, shared by every storyboard, is kept on disk by
//! `emulsion-io` and holds the same kinds of item.
use crate::command::Slot;
use crate::motion::{self, Easing, KeyView};
use crate::project::{MAX_PROJECT_PIXELS, PageId, ProjectEditor};
use crate::storyboard::{
    CameraKey, CameraState, CaptionField, FrameRate, GroupId, LayerProperty, MAX_PANEL_FRAMES,
    Panel, SceneCamera, Settings, Storyboard,
};
use crate::storyboard_naming::{centred_frame, fit_document};
use crate::storyboard_shot::ShotLibrary;
use crate::{Document, Editor, NodeId, fragment::Fragment};
use serde::{Deserialize, Serialize};
use std::collections::{BTreeMap, HashSet};
use std::sync::Arc;

pub type ItemId = u64;

/// Most items one project library holds.
pub const MAX_ITEMS: usize = 500;
/// Most tags on one item.
pub const MAX_TAGS: usize = 50;
/// Most panels one scene item holds.
pub const MAX_SCENE_PANELS: usize = 500;

#[derive(Clone, Copy, Debug, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum ItemKind {
    /// Layers placed on top of the active panel.
    Layers,
    /// A whole panel, placed as a new panel.
    Panel,
    /// A whole scene, placed as a new scene.
    Scene,
}

impl ItemKind {
    pub fn label(self) -> &'static str {
        match self {
            Self::Layers => "Layers",
            Self::Panel => "Panel",
            Self::Scene => "Scene",
        }
    }
}

/// What a panel or scene item keeps besides its drawings.
#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
pub struct ItemAnimation {
    /// The rate the frames below count at; placing converts them, keeping
    /// their time.
    pub frame_rate: FrameRate,
    /// The caption fields the panels' captions use.
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    pub fields: Vec<CaptionField>,
    /// A scene item's scene name.
    #[serde(default, skip_serializing_if = "String::is_empty")]
    pub scene: String,
    /// One per drawing, in order.
    pub panels: Vec<ItemPanel>,
    /// Camera keys from the start of the scene (a scene item) or of the
    /// panel (a panel item).
    #[serde(default, skip_serializing_if = "SceneCamera::is_empty")]
    pub camera: SceneCamera,
    /// The 3D models the panels' Shot Generator sets use, for placing them
    /// in another project (a personal library item keeps their files
    /// beside it; the project library leaves them in the project's own).
    #[serde(default, skip_serializing_if = "ShotLibrary::is_empty")]
    pub models: ShotLibrary,
}

#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
pub struct ItemPanel {
    pub name: String,
    /// Panel data: timing, captions, shot data, layer keyframes and comps.
    /// Its scene is not used.
    pub panel: Panel,
}

impl ItemAnimation {
    /// Whether anything moves or switches: keys, shake or comps.
    pub fn is_animated(&self) -> bool {
        !self.camera.is_empty()
            || self
                .panels
                .iter()
                .any(|p| !p.panel.motion.is_empty() || !p.panel.comps.is_empty())
    }

    /// Check the panels and camera by the storyboard's own rules, as a
    /// storyboard of their own at `width` × `height`.
    fn validate(&self, width: u32, height: u32) -> Result<(), String> {
        if self.panels.is_empty() || self.panels.len() > MAX_SCENE_PANELS {
            return Err(format!("A library item holds 1–{MAX_SCENE_PANELS} panels."));
        }
        if !self.scene.is_empty() {
            crate::storyboard::check_name(&self.scene, "Scene")?;
        }
        let ids: Vec<PageId> = (1..=self.panels.len() as u64).collect();
        let mut settings = Settings::new(width, height);
        settings.frame_rate = self.frame_rate;
        let mut board = Storyboard::new(settings, &ids);
        board.captions = self.fields.clone();
        let scene = board.panels[&1].scene;
        for (id, item) in ids.iter().zip(&self.panels) {
            crate::storyboard::check_name(&item.name, "Panel")?;
            board.panels.insert(
                *id,
                Panel {
                    scene,
                    locked: false,
                    ..item.panel.clone()
                },
            );
        }
        if !self.camera.is_empty() {
            board.cameras.insert(scene, self.camera.clone());
        }
        self.models.validate()?;
        board.validate(&ids)
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
    /// Timing, keyframes, comps and camera of a panel or scene item.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub animation: Option<ItemAnimation>,
    /// A scene item's drawings after the first (`doc`), stored beside it.
    #[serde(skip)]
    pub more: Vec<Arc<Document>>,
}

/// Stands in for a drawing until the package reader fills it in; it never
/// validates, so a drawing missing from a package is an error.
fn unloaded() -> Arc<Document> {
    Arc::new(Document::new(0, 0))
}

impl PartialEq for LibraryItem {
    fn eq(&self, other: &Self) -> bool {
        let same = |a: &Arc<Document>, b: &Arc<Document>| Arc::ptr_eq(a, b) || a == b;
        self.id == other.id
            && self.name == other.name
            && self.tags == other.tags
            && self.kind == other.kind
            && self.animation == other.animation
            && same(&self.doc, &other.doc)
            && self.more.len() == other.more.len()
            && self.more.iter().zip(&other.more).all(|(a, b)| same(a, b))
    }
}

impl LibraryItem {
    /// A drawing to save, before it has a name and an ID.
    pub fn drawing(kind: ItemKind, doc: Document) -> Self {
        Self {
            id: 0,
            name: String::new(),
            tags: Vec::new(),
            kind,
            doc: Arc::new(doc),
            animation: None,
            more: Vec::new(),
        }
    }

    pub fn matches(&self, query: &str) -> bool {
        matches(&self.name, &self.tags, query)
    }

    /// Every drawing: the one drawing, or each panel of a scene item.
    pub fn drawings(&self) -> impl Iterator<Item = &Arc<Document>> {
        std::iter::once(&self.doc).chain(&self.more)
    }

    /// Whether placing it brings keyframes, comps or camera moves.
    pub fn is_animated(&self) -> bool {
        self.animation
            .as_ref()
            .is_some_and(ItemAnimation::is_animated)
    }

    /// Check the drawings and animation, without the name and tags.
    pub fn validate_content(&self) -> Result<(), String> {
        for doc in self.drawings() {
            doc.validate().map_err(|e| e.to_string())?;
            if doc.width == 0 || doc.height == 0 {
                return Err(format!("Library item {} has no drawing.", self.name));
            }
            if !doc.nodes.iter().any(|n| n.parent.is_none()) {
                return Err(format!("Library item {} has no layers.", self.name));
            }
            if (doc.width, doc.height) != (self.doc.width, self.doc.height) {
                return Err("A scene item's panels share one size.".into());
            }
        }
        let panels = self.animation.as_ref().map(|a| a.panels.len());
        let fits = match self.kind {
            ItemKind::Layers => panels.is_none() && self.more.is_empty(),
            ItemKind::Panel => panels.is_none_or(|n| n == 1) && self.more.is_empty(),
            ItemKind::Scene => panels == Some(self.more.len() + 1),
        };
        if !fits {
            return Err(format!(
                "Library item {} does not match its kind.",
                self.name
            ));
        }
        match &self.animation {
            Some(animation) => animation.validate(self.doc.width, self.doc.height),
            None => Ok(()),
        }
    }

    fn clear_selection(&mut self) {
        for doc in std::iter::once(&mut self.doc).chain(&mut self.more) {
            if doc.selection.is_some() {
                Arc::make_mut(doc).selection = None;
            }
        }
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
        self.add_item(name, tags, LibraryItem::drawing(kind, doc))
    }

    /// Add an item made by `LibraryItem::drawing` or a capture, under
    /// `name`; returns its ID.
    pub fn add_item(
        &mut self,
        name: &str,
        tags: &[String],
        mut item: LibraryItem,
    ) -> Result<ItemId, String> {
        if self.items.len() >= MAX_ITEMS {
            return Err(format!("A library holds at most {MAX_ITEMS} items."));
        }
        item.id = self.next_id;
        item.name = name.trim().into();
        item.tags = clean_tags(tags)?;
        self.items.push(item);
        self.next_id += 1;
        Ok(self.next_id - 1)
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
            item.validate_content()?;
            pixels += item
                .drawings()
                .map(|d| u64::from(d.width) * u64::from(d.height))
                .sum::<u64>();
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
    /// A new scene and its panels; the first is active.
    Scene { scene: GroupId, panels: Vec<PageId> },
}

/// The keyed camera at `frame`, before shake.
fn camera_state(camera: &SceneCamera, frame: f64, rest: CameraState) -> CameraState {
    let channel = |get: fn(&CameraKey) -> f64, rest: f64| {
        motion::sample_by(&camera.keys, frame, |k| KeyView {
            time: k.frame as f64,
            value: get(k),
            easing: k.easing,
            curve: k.curve,
        })
        .unwrap_or(rest)
    };
    CameraState {
        x: channel(|k| k.x, rest.x),
        y: channel(|k| k.y, rest.y),
        zoom: channel(|k| k.zoom, rest.zoom),
        rotation: channel(|k| k.rotation, rest.rotation),
    }
}

fn state_of(key: &CameraKey) -> CameraState {
    CameraState {
        x: key.x,
        y: key.y,
        zoom: key.zoom,
        rotation: key.rotation,
    }
}

/// Where `panel` starts within its scene, in frames: the playing panels of
/// the scene before it.
fn scene_offset(board: &Storyboard, layout: &[PageId], panel: PageId) -> u64 {
    let scene = board.panels[&panel].scene;
    layout
        .iter()
        .take_while(|id| **id != panel)
        .filter_map(|id| board.panels.get(id))
        .filter(|p| p.scene == scene && p.thumbnails.is_none())
        .map(|p| u64::from(p.frames))
        .sum()
}

/// The scene camera over `panel`, re-timed to start with it: the keys
/// within the panel, with the camera sampled at its start and end when it
/// moves across them. Empty when the camera is at rest throughout.
pub(crate) fn panel_camera(board: &Storyboard, layout: &[PageId], panel: PageId) -> SceneCamera {
    let p = &board.panels[&panel];
    let Some(camera) = board.cameras.get(&p.scene).filter(|c| !c.keys.is_empty()) else {
        return SceneCamera::default();
    };
    if p.thumbnails.is_some() {
        return SceneCamera::default();
    }
    let rest = board.rest_camera();
    let start = scene_offset(board, layout, panel);
    let end = start + u64::from(p.frames);
    let mut keys = Vec::new();
    if !camera.keys.iter().any(|k| k.frame == start) {
        let mut key = CameraKey::at(0, camera_state(camera, start as f64, rest));
        if let Some(before) = camera.keys.iter().rev().find(|k| k.frame < start) {
            key.easing = before.easing;
            key.curve = before.curve;
        }
        keys.push(key);
    }
    keys.extend(
        camera
            .keys
            .iter()
            .filter(|k| (start..=end).contains(&k.frame))
            .map(|k| CameraKey {
                frame: k.frame - start,
                ..*k
            }),
    );
    if camera.keys.iter().any(|k| k.frame > end)
        && keys.last().is_some_and(|k| k.frame < end - start)
    {
        keys.push(CameraKey::at(
            end - start,
            camera_state(camera, end as f64, rest),
        ));
    }
    if keys.iter().all(|k| state_of(k) == rest) {
        return SceneCamera::default();
    }
    SceneCamera { keys, shake: None }
}

/// `new` (keys from the start of a new panel `len` frames long) spliced into
/// `old` at scene frame `at`: the old keys from `at` on move `len` later,
/// the old camera holds until the new panel and resumes after it.
fn splice(
    old: Option<&SceneCamera>,
    new: &SceneCamera,
    at: u64,
    len: u64,
    rest: CameraState,
) -> SceneCamera {
    let old = old.cloned().unwrap_or_default();
    let mut keys = BTreeMap::new();
    for key in &old.keys {
        let frame = if key.frame < at {
            key.frame
        } else {
            key.frame + len
        };
        keys.insert(frame, CameraKey { frame, ..*key });
    }
    if at > 0 {
        let hold = keys
            .entry(at - 1)
            .or_insert_with(|| CameraKey::at(at - 1, camera_state(&old, (at - 1) as f64, rest)));
        hold.easing = Easing::Step;
        hold.curve = None;
    }
    keys.entry(at + len).or_insert_with(|| {
        let mut key = CameraKey::at(at + len, camera_state(&old, at as f64, rest));
        if let Some(before) = old.keys.iter().rev().find(|k| k.frame < at) {
            key.easing = before.easing;
            key.curve = before.curve;
        }
        key
    });
    let mut placed: Vec<CameraKey> = new.keys.iter().filter(|k| k.frame < len).copied().collect();
    if placed.first().is_none_or(|k| k.frame > 0) {
        placed.insert(0, CameraKey::at(0, camera_state(new, 0., rest)));
    }
    if new.keys.iter().any(|k| k.frame >= len) && placed.last().is_some_and(|k| k.frame + 1 < len) {
        placed.push(CameraKey::at(
            len - 1,
            camera_state(new, (len - 1) as f64, rest),
        ));
    }
    if let Some(last) = placed.last_mut() {
        last.easing = Easing::Step;
        last.curve = None;
    }
    for key in placed {
        let frame = at + key.frame;
        keys.insert(frame, CameraKey { frame, ..key });
    }
    SceneCamera {
        keys: keys.into_values().collect(),
        shake: old.shake,
    }
}

/// How a drawing's pixels map into the board's frame: the crop's corner and
/// the scale, when the drawing is another size.
#[derive(Clone, Copy)]
struct Fit {
    x: f64,
    y: f64,
    scale: f64,
}

impl Fit {
    fn point(self, x: f64, y: f64) -> (f64, f64) {
        ((x - self.x) * self.scale, (y - self.y) * self.scale)
    }
}

/// `doc` fitted to `width` × `height` like imported panels, and how its
/// pixels moved.
fn fit(doc: &Document, width: u32, height: u32) -> (Document, Fit) {
    if (doc.width, doc.height) == (width, height) {
        let same = Fit {
            x: 0.,
            y: 0.,
            scale: 1.,
        };
        return (doc.clone(), same);
    }
    let rect = centred_frame(doc, width, height);
    let fitted = fit_document(doc, rect, width, height);
    let fit = Fit {
        x: f64::from(rect.x),
        y: f64::from(rect.y),
        scale: f64::from(width) / f64::from(rect.w.max(1)),
    };
    (fitted, fit)
}

/// A frame at `from` as a frame at `to`, keeping its time.
fn retime(frame: u64, from: FrameRate, to: FrameRate) -> u64 {
    if from == to {
        frame
    } else {
        (frame as f64 * to.fps() / from.fps()).round() as u64
    }
}

/// Drop keys that land on the frame of the key before them.
fn one_per_frame<K>(keys: &mut Vec<K>, frame: impl Fn(&K) -> u64) {
    let mut last = None;
    keys.retain(|k| {
        let f = frame(k);
        let keep = last.is_none_or(|l| f > l);
        last = Some(f);
        keep
    });
}

/// A saved panel in the board's frame rate and frame.
fn convert_panel(panel: &Panel, from: FrameRate, to: FrameRate, fit: Fit) -> Panel {
    let mut panel = panel.clone();
    let seconds = f64::from(panel.frames) / from.fps();
    panel.frames = ((seconds * to.fps()).round() as u32).clamp(1, MAX_PANEL_FRAMES);
    panel.transition.frames = panel.transition.frames.min(panel.frames);
    for layer in panel.motion.values_mut() {
        if let Some([x, y]) = layer.pivot {
            let (x, y) = fit.point(x, y);
            layer.pivot = Some([x, y]);
        }
        for track in &mut layer.tracks {
            let scale = matches!(track.property, LayerProperty::X | LayerProperty::Y);
            for key in &mut track.keys {
                key.frame = retime(key.frame, from, to);
                if scale {
                    key.value *= fit.scale;
                }
            }
            one_per_frame(&mut track.keys, |k| k.frame);
        }
    }
    panel
}

/// A saved camera in the board's frame rate and frame.
fn convert_camera(camera: &SceneCamera, from: FrameRate, to: FrameRate, fit: Fit) -> SceneCamera {
    let mut camera = camera.clone();
    for key in &mut camera.keys {
        key.frame = retime(key.frame, from, to);
        (key.x, key.y) = fit.point(key.x, key.y);
    }
    one_per_frame(&mut camera.keys, |k| k.frame);
    if let Some(shake) = &mut camera.shake {
        shake.amplitude = (shake.amplitude * fit.scale).min(10_000.);
    }
    camera
}

impl ProjectEditor {
    fn library_board(&self) -> Result<&Storyboard, String> {
        self.storyboard()
            .ok_or_else(|| "This is not a storyboard project.".into())
    }

    fn layout_order(&self) -> Vec<PageId> {
        self.page_list().iter().map(|m| m.id).collect()
    }

    fn panel_name(&self, id: PageId) -> String {
        self.page_list()
            .iter()
            .find(|m| m.id == id)
            .map_or_else(|| "Panel".into(), |m| m.name.clone())
    }

    /// `panel` as a library item: its drawing and, when it is animated
    /// (layer keyframes or comps, or the scene camera moving over it) or
    /// has a Shot Generator set or layers in depth, its duration, shot
    /// data, keyframes, comps, set (with its models) and the camera keys
    /// over it, re-timed to the panel. Captions stay with the board.
    pub fn capture_panel_item(&self, panel: PageId) -> Result<LibraryItem, String> {
        let doc = self.page(panel).ok_or("Panel does not exist.")?.doc.clone();
        let mut item = LibraryItem::drawing(ItemKind::Panel, doc);
        let Some(board) = self.storyboard() else {
            return Ok(item);
        };
        let p = board.panels.get(&panel).ok_or("Panel does not exist.")?;
        let camera = panel_camera(board, &self.layout_order(), panel);
        let plain = p.motion.is_empty() && p.comps.is_empty() && camera.is_empty();
        if plain && p.shot.is_none() && p.depth.is_empty() {
            return Ok(item);
        }
        item.animation = Some(ItemAnimation {
            frame_rate: board.settings.frame_rate,
            fields: Vec::new(),
            scene: String::new(),
            panels: vec![ItemPanel {
                name: self.panel_name(panel),
                panel: Panel {
                    scene: 0,
                    locked: false,
                    captions: BTreeMap::new(),
                    transition: Default::default(),
                    ..p.clone()
                },
            }],
            camera,
            models: board.models_of([p]),
        });
        Ok(item)
    }

    /// `scene` as a library item: every panel's drawing, duration,
    /// captions, shot data, transitions within the scene, keyframes and
    /// comps, and the scene camera.
    pub fn capture_scene_item(&self, scene: GroupId) -> Result<LibraryItem, String> {
        let board = self.library_board()?;
        let group = board.scenes.get(&scene).ok_or("No scene has that ID.")?;
        let panels = board
            .outline(&self.layout_order())
            .into_iter()
            .find(|s| s.scene == scene)
            .map(|s| s.panels)
            .unwrap_or_default();
        if panels.is_empty() || panels.len() > MAX_SCENE_PANELS {
            return Err(format!("A scene item holds 1–{MAX_SCENE_PANELS} panels."));
        }
        let used: HashSet<_> = panels
            .iter()
            .flat_map(|id| board.panels[id].captions.keys())
            .collect();
        let docs = panels
            .iter()
            .map(|id| {
                Ok(Arc::new(
                    self.page(*id).ok_or("Panel does not exist.")?.doc.clone(),
                ))
            })
            .collect::<Result<Vec<_>, String>>()?;
        let items = panels
            .iter()
            .enumerate()
            .map(|(i, id)| {
                let mut panel = Panel {
                    scene: 0,
                    locked: false,
                    ..board.panels[id].clone()
                };
                if i == 0 {
                    // The way in belongs to the scene before.
                    panel.transition = Default::default();
                }
                ItemPanel {
                    name: self.panel_name(*id),
                    panel,
                }
            })
            .collect();
        Ok(LibraryItem {
            id: 0,
            name: String::new(),
            tags: Vec::new(),
            kind: ItemKind::Scene,
            doc: docs[0].clone(),
            animation: Some(ItemAnimation {
                frame_rate: board.settings.frame_rate,
                fields: board
                    .captions
                    .iter()
                    .filter(|f| used.contains(&f.id))
                    .cloned()
                    .collect(),
                scene: group.name.clone(),
                models: board.models_of(panels.iter().map(|id| &board.panels[id])),
                panels: items,
                camera: board.cameras.get(&scene).cloned().unwrap_or_default(),
            }),
            more: docs[1..].to_vec(),
        })
    }

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

    /// Add a whole panel, with its animation, to the project library, one
    /// Undo step.
    pub fn add_library_panel(
        &mut self,
        panel: PageId,
        name: &str,
        tags: &[String],
    ) -> Result<ItemId, String> {
        let item = self.capture_panel_item(panel)?;
        self.add_library_entry(name, tags, item)
    }

    /// Add a whole scene to the project library, one Undo step.
    pub fn add_library_scene(
        &mut self,
        scene: GroupId,
        name: &str,
        tags: &[String],
    ) -> Result<ItemId, String> {
        let item = self.capture_scene_item(scene)?;
        self.add_library_entry(name, tags, item)
    }

    /// Add a drawing to the project library, one Undo step.
    pub fn add_library_item(
        &mut self,
        name: &str,
        tags: &[String],
        kind: ItemKind,
        doc: Document,
    ) -> Result<ItemId, String> {
        self.add_library_entry(name, tags, LibraryItem::drawing(kind, doc))
    }

    /// Add a captured item to the project library, one Undo step.
    pub fn add_library_entry(
        &mut self,
        name: &str,
        tags: &[String],
        mut item: LibraryItem,
    ) -> Result<ItemId, String> {
        // A drawing is not a selection.
        item.clear_selection();
        // The project keeps the models its library's sets use.
        let models = item
            .animation
            .as_mut()
            .map(|a| std::mem::take(&mut a.models))
            .unwrap_or_default();
        let mut id = 0;
        self.edit_storyboard(|b| {
            b.shot_library.adopt_models(&models)?;
            id = b.library.add_item(name, tags, item)?;
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
            ItemKind::Scene => Err("A scene item places with its panels.".into()),
        }
    }

    /// Place an item from either library, one Undo step. Items without
    /// animation place like `place_drawing`. An animated panel item becomes
    /// a new panel after the active one, in its scene, with its duration,
    /// keyframes and comps; its camera keys join the scene camera over the
    /// new panel. A scene item becomes a new scene after the active panel's
    /// scene with its panels, captions, keyframes, comps and camera.
    /// Frames keep their time at this board's frame rate; drawings at
    /// another resolution are fitted to the frame, keys with them.
    pub fn place_item(&mut self, item: &LibraryItem) -> Result<Placed, String> {
        item.validate_content()?;
        match (&item.animation, item.kind) {
            (None, kind) => self.place_drawing(kind, &item.doc),
            (Some(animation), ItemKind::Panel) => self.place_animated_panel(&item.doc, animation),
            (Some(animation), ItemKind::Scene) => self.place_scene(item, animation),
            (Some(_), ItemKind::Layers) => Err("That library item is damaged.".into()),
        }
    }

    fn place_animated_panel(
        &mut self,
        doc: &Document,
        animation: &ItemAnimation,
    ) -> Result<Placed, String> {
        let board = self.library_board()?;
        let layout = self.layout_order();
        let active = self.active_page();
        let (width, height) = (board.settings.width, board.settings.height);
        let rate = board.settings.frame_rate;
        let (doc, fit) = fit(doc, width, height);
        let scene = board
            .panels
            .get(&active)
            .ok_or("Panel does not exist.")?
            .scene;
        let panel = Panel {
            scene,
            locked: false,
            captions: BTreeMap::new(),
            transition: Default::default(),
            ..convert_panel(&animation.panels[0].panel, animation.frame_rate, rate, fit)
        };
        let mut next = Storyboard::clone(board);
        next.shot_library.adopt_models(&animation.models)?;
        let camera = convert_camera(&animation.camera, animation.frame_rate, rate, fit);
        if !camera.keys.is_empty() && panel.thumbnails.is_none() {
            let before = &board.panels[&active];
            let at = scene_offset(board, &layout, active)
                + if before.thumbnails.is_none() {
                    u64::from(before.frames)
                } else {
                    0
                };
            let spliced = splice(
                board.cameras.get(&scene),
                &camera,
                at,
                u64::from(panel.frames),
                board.rest_camera(),
            );
            next.cameras.insert(scene, spliced);
        }
        let name = self.next_panel_name(active);
        let ids = self.insert_into(next, Some(active), vec![(name, doc, panel)], &[], &[])?;
        Ok(Placed::Panel(ids[0]))
    }

    fn place_scene(
        &mut self,
        item: &LibraryItem,
        animation: &ItemAnimation,
    ) -> Result<Placed, String> {
        let board = self.library_board()?;
        let layout = self.layout_order();
        let (width, height) = (board.settings.width, board.settings.height);
        let rate = board.settings.frame_rate;
        let mut next = Storyboard::clone(board);
        next.shot_library.adopt_models(&animation.models)?;
        let fields = crate::project::adopt_fields(
            &mut next,
            &animation.fields,
            animation.fields.iter().map(|f| f.id),
        )?;
        let outline = next.outline(&layout);
        let active = self.active_page();
        let landing = outline
            .iter()
            .find(|s| s.panels.contains(&active))
            .or(outline.last())
            .ok_or("The storyboard has no scenes.")?;
        let after = landing.panels.last().copied();
        let scene = next.add_scene(landing.sequence);
        if !animation.scene.is_empty() {
            next.scenes.get_mut(&scene).unwrap().name = animation.scene.clone();
        }
        let (_, fitting) = fit(&item.doc, width, height);
        let camera = convert_camera(&animation.camera, animation.frame_rate, rate, fitting);
        if !camera.is_empty() {
            next.cameras.insert(scene, camera);
        }
        let items = item
            .drawings()
            .zip(&animation.panels)
            .map(|(doc, saved)| {
                let (doc, fit) = fit(doc, width, height);
                let panel = convert_panel(&saved.panel, animation.frame_rate, rate, fit);
                let captions = panel
                    .captions
                    .iter()
                    .filter_map(|(id, text)| Some((*fields.get(id)?, text.clone())))
                    .collect();
                let panel = Panel {
                    scene,
                    locked: false,
                    captions,
                    ..panel
                };
                (saved.name.clone(), doc, panel)
            })
            .collect();
        let panels = self.insert_into(next, after, items, &[], &[])?;
        Ok(Placed::Scene { scene, panels })
    }

    /// Place a project library item; see `place_item`.
    pub fn place_library_item(&mut self, id: ItemId) -> Result<Placed, String> {
        let item = self
            .storyboard()
            .ok_or("This is not a storyboard project.")?
            .library
            .item(id)
            .ok_or("No library item has that ID.")?
            .clone();
        self.place_item(&item)
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

    /// Panels 1 and 2 of 24 frames in one scene; Hero on panel 1 slides
    /// right over its first 10 frames, a comp hides it, and the camera pans
    /// linearly across the scene.
    fn animated() -> (ProjectEditor, NodeId) {
        use crate::storyboard::{LayerMotion, MotionKey, PropertyTrack};
        let mut p = board();
        let hero = hero(&p);
        let action = p.storyboard().unwrap().caption("Action").unwrap();
        p.execute(Command::SetVisible {
            id: hero,
            visible: false,
        })
        .unwrap();
        p.capture_comp(1, "Empty").unwrap();
        p.execute(Command::SetVisible {
            id: hero,
            visible: true,
        })
        .unwrap();
        // Durations first: keyframes set in the same edit would stretch.
        p.edit_storyboard(|b| {
            for id in [1, 2] {
                b.panels.get_mut(&id).unwrap().frames = 24;
            }
            Ok(())
        })
        .unwrap();
        p.edit_storyboard(|b| {
            let rest = b.rest_camera();
            let one = b.panels.get_mut(&1).unwrap();
            one.captions.insert(action, "Hero slides in".into());
            let key = |frame, value| MotionKey {
                frame,
                value,
                easing: Easing::Linear,
                curve: None,
            };
            one.motion.insert(
                hero,
                LayerMotion {
                    pivot: Some([10., 10.]),
                    tracks: vec![PropertyTrack {
                        property: LayerProperty::X,
                        keys: vec![key(0, -20.), key(10, 0.)],
                    }],
                },
            );
            let scene = one.scene;
            b.cameras.insert(
                scene,
                SceneCamera {
                    keys: vec![
                        CameraKey {
                            easing: Easing::Linear,
                            ..CameraKey::at(0, rest)
                        },
                        CameraKey::at(48, CameraState { x: 64., ..rest }),
                    ],
                    shake: None,
                },
            );
            Ok(())
        })
        .unwrap();
        (p, hero)
    }

    fn layout(p: &ProjectEditor) -> Vec<PageId> {
        p.page_list().iter().map(|m| m.id).collect()
    }

    #[test]
    fn animated_panel_items_keep_keys_comps_and_the_camera_over_them() {
        let (mut p, hero) = animated();
        let id = p.add_library_panel(1, "Slide", &[]).unwrap();
        let item = p.storyboard().unwrap().library.item(id).unwrap().clone();
        assert!(item.is_animated());
        let saved = item.animation.as_ref().unwrap();
        assert_eq!(saved.panels[0].panel.frames, 24);
        assert!(saved.panels[0].panel.captions.is_empty());
        // Re-timed to the panel: its start and where the pan is at its end.
        let frames: Vec<_> = saved.camera.keys.iter().map(|k| k.frame).collect();
        assert_eq!(frames, [0, 24]);
        assert!((saved.camera.keys[1].x - 48.).abs() < 1e-9);

        let before = p.storyboard().unwrap().clone();
        let old = layout(&p);
        p.set_active_page(2).unwrap();
        let Placed::Panel(new) = p.place_library_item(id).unwrap() else {
            panic!()
        };
        assert_eq!(layout(&p), [1, 2, new]);
        let board = p.storyboard().unwrap();
        let panel = &board.panels[&new];
        assert_eq!(panel.frames, 24);
        assert_eq!(panel.scene, board.panels[&1].scene);
        // The whole document is copied, so the keyed and comp layer IDs hold.
        assert!(p.page(new).unwrap().doc.node(hero).is_some());
        assert_eq!(panel.motion[&hero], before.panels[&1].motion[&hero]);
        assert_eq!(panel.comps, before.panels[&1].comps);
        let order = layout(&p);
        // The panels before keep their camera; the new one pans like panel 1.
        for f in [0., 10., 30., 47.] {
            assert_eq!(board.camera_at(&order, f), before.camera_at(&old, f), "{f}");
        }
        assert_eq!(board.camera_at(&order, 48.).x, 32.);
        assert!((board.camera_at(&order, 58.).x - before.camera_at(&old, 10.).x).abs() < 0.5);
        // The old end of the pan moves past the new panel.
        let last = board.cameras[&panel.scene].keys.last().unwrap();
        assert_eq!((last.frame, last.x), (72, 64.));
        let animated = board
            .animate_panel(new, &p.page(new).unwrap().doc, 0.)
            .unwrap();
        assert!(animated != p.page(new).unwrap().doc);
        p.snapshot().unwrap().validate().unwrap();
        // One Undo step takes the panel and its camera keys back.
        assert!(p.undo());
        assert_eq!(layout(&p), old);
        assert_eq!(p.storyboard().unwrap().cameras, before.cameras);
    }

    #[test]
    fn scene_items_place_a_whole_animated_scene_as_one_undo_step() {
        let (mut p, hero) = animated();
        let scene = p.storyboard().unwrap().panels[&1].scene;
        let id = p
            .add_library_scene(scene, "Opening", &["intro".into()])
            .unwrap();
        let item = p.storyboard().unwrap().library.item(id).unwrap().clone();
        assert_eq!(item.kind, ItemKind::Scene);
        assert_eq!(item.drawings().count(), 2);
        let before = p.storyboard().unwrap().clone();
        let old = layout(&p);
        let undo_depth = p.can_undo();
        p.set_active_page(1).unwrap();
        let Placed::Scene { scene: new, panels } = p.place_library_item(id).unwrap() else {
            panic!()
        };
        assert_eq!(layout(&p), [1, 2, panels[0], panels[1]]);
        assert_eq!(p.active_page(), panels[0]);
        let board = p.storyboard().unwrap();
        assert_eq!(board.scenes.len(), before.scenes.len() + 1);
        assert_eq!(board.scenes[&new].name, before.scenes[&scene].name);
        assert_eq!(board.cameras[&new], before.cameras[&scene]);
        let first = &board.panels[&panels[0]];
        assert_eq!(first.scene, new);
        assert_eq!(first.frames, 24);
        assert_eq!(first.motion, before.panels[&1].motion);
        assert_eq!(first.comps, before.panels[&1].comps);
        let action = board.caption("Action").unwrap();
        assert_eq!(first.captions[&action].text, "Hero slides in");
        assert!(p.page(panels[0]).unwrap().doc.node(hero).is_some());
        // The new scene's camera plays like the original.
        let order = layout(&p);
        assert_eq!(
            board.camera_at(&order, 48. + 30.),
            before.camera_at(&old, 30.)
        );
        p.snapshot().unwrap().validate().unwrap();
        assert!(p.undo());
        assert_eq!(layout(&p), old);
        assert_eq!(*p.storyboard().unwrap(), before);
        assert_eq!(p.can_undo(), undo_depth);
        assert!(p.redo());
        assert_eq!(
            p.storyboard().unwrap().scenes.len(),
            before.scenes.len() + 1
        );
    }

    #[test]
    fn animated_items_keep_their_time_at_another_frame_rate() {
        let (mut p, hero) = animated();
        let scene = p.storyboard().unwrap().panels[&1].scene;
        let id = p.add_library_scene(scene, "Opening", &[]).unwrap();
        p.edit_storyboard(|b| {
            b.settings.frame_rate = crate::storyboard::FrameRate::whole(48);
            Ok(())
        })
        .unwrap();
        let Placed::Scene { scene: new, panels } = p.place_library_item(id).unwrap() else {
            panic!()
        };
        let board = p.storyboard().unwrap();
        assert_eq!(board.panels[&panels[0]].frames, 48);
        let keys: Vec<_> = board.panels[&panels[0]].motion[&hero].tracks[0]
            .keys
            .iter()
            .map(|k| k.frame)
            .collect();
        assert_eq!(keys, [0, 20]);
        assert_eq!(board.cameras[&new].keys[1].frame, 96);
    }

    #[test]
    fn items_without_animation_load_and_place_as_before() {
        let mut p = board();
        let id = p.add_library_panel(1, "Castle", &[]).unwrap();
        let item = p.storyboard().unwrap().library.item(id).unwrap().clone();
        assert!(item.animation.is_none() && !item.is_animated());
        let old: LibraryItem =
            serde_json::from_str(r#"{"id":4,"name":"Old","tags":[],"kind":"panel"}"#).unwrap();
        assert!(old.animation.is_none() && old.more.is_empty());
        let json = serde_json::to_value(&item).unwrap();
        assert!(json.get("animation").is_none());
        // Kinds and their animation must agree.
        let mut broken = item.clone();
        broken.kind = ItemKind::Scene;
        assert!(broken.validate_content().is_err());
        let stamp = p.stamp();
        assert!(p.place_item(&broken).is_err());
        assert!(p.add_library_scene(999, "x", &[]).is_err());
        assert_eq!(p.stamp(), stamp);
    }
}
