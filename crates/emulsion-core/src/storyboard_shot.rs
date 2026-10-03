//! Shot Generator sets on storyboard panels (phase 11: SG1–SG8, C6–C8,
//! C10–C13, L6, V6). The 3D engine is `emulsion-scene`; this module stores
//! its sets in the board and turns them into panel layers.
//!
//! # Design
//!
//! **One set per panel.** [`Panel::shot`](crate::storyboard::Panel) holds an
//! optional [`PanelShot`]: an `emulsion_scene::Scene` (characters, props,
//! lights, the shot camera) and how it renders into the panel. Storyboarder
//! keeps one set per board too. Panels of a scene often share a set and differ
//! in the camera, but a per-panel set keeps every panel's reference layer
//! self-contained: duplicating a panel copies its set (a continuity starting
//! point), panels can be moved, extracted, merged and pasted with their sets,
//! and editing one panel never re-renders another.
//!
//! **Models and poses are project-wide.** Imported glTF/GLB/OBJ files live
//! once in the board's [`ShotLibrary`], keyed by a content hash that a
//! `ModelRef::asset` names; the package stores them as `models/{id}.{ext}`.
//! Their bytes are kept in memory beside the board (shared, so Undo states
//! cost nothing) and parsed into an `AssetLibrary` when rendering. A set
//! edit drops models no set uses any more; Undo brings them back. Custom
//! poses are saved in the same library rather than in Settings: they travel
//! with the project to collaborators and the assistant, and saving one is an
//! ordinary Undo step.
//!
//! **Reference layer (C8, SG1).** The set renders through its own camera at
//! the panel's resolution into a locked, reduced-opacity raster layer named
//! [`REFERENCE_LAYER`] just above the paper. Rendering again replaces its
//! pixels in place; the layer's id is remembered in [`PanelShot::layer`].
//! A snapshot (C13) is an ordinary editable layer instead.
//!
//! **Layers that follow the set (C12).** A [`LayerAttachment`] ties a panel
//! layer to a point on an object or a mannequin bone (a surface point picked
//! in the viewport works too). When the set changes, the point is projected
//! through the new camera and the layer moves with it, scaling with its
//! distance, in the same Undo step as the set edit.
//!
//! **Layer depth (L6, C6).** [`Panel::depth`](crate::storyboard::Panel) gives
//! layers a depth behind (or in front of) the panel plane, in multiples of
//! the camera's distance to that plane. The 2D scene camera is then read as a
//! 3D camera (a 35 mm lens on Super 35, pan moves it sideways and zoom
//! dollies it) and [`parallax_transform`] projects each layer through it, so
//! far layers move less than near ones. At rest every layer looks exactly as
//! drawn.
use crate::project::PageId;
use crate::storyboard::{CameraState, Storyboard};
use crate::{Command, Document, NodeId};
use emulsion_raster::Raster;
use emulsion_scene::{
    AssetLibrary, Bone, Camera, FilmBack, ModelFormat, ObjectId, ObjectKind, Pose, PreparedScene,
    Prop, RenderOptions, RenderStyle, RgbaImage, Scene, attachment_matrix, horizontal_fov_deg,
    import_bytes, prepare, project_point, render,
};
use glam::{DAffine2, DVec2, Vec3, dvec2};
use serde::{Deserialize, Serialize};
use std::collections::{BTreeMap, BTreeSet};
use std::sync::Arc;

/// The reference layer's name.
pub const REFERENCE_LAYER: &str = "Shot Generator";
/// A snapshot layer's name.
pub const SNAPSHOT_LAYER: &str = "Shot Generator snapshot";
/// Most models one project keeps.
pub const MAX_MODELS: usize = 64;
/// Total model bytes one project keeps (the `AssetLibrary` budget).
pub const MAX_MODEL_BYTES: usize = 4 * emulsion_scene::limits::MAX_ASSET_BYTES;
/// Most saved custom poses.
pub const MAX_POSES: usize = 500;
/// Layer depth range, in multiples of the camera's distance to the panel.
pub const DEPTH_RANGE: std::ops::RangeInclusive<f64> = -0.9..=100.;
/// Most layers attached to one set.
pub const MAX_ATTACHMENTS: usize = 256;
/// The lens the 2D scene camera is read as for layer depth.
pub const PARALLAX_FOCAL_MM: f32 = 35.;
/// Focal lengths offered in pickers (mm).
pub const LENSES: [f32; 10] = [14., 18., 24., 35., 50., 65., 85., 100., 135., 200.];

/// How the set renders into its panel.
#[derive(Clone, Copy, Debug, PartialEq, Serialize, Deserialize)]
#[serde(default)]
pub struct ReferenceSettings {
    pub style: RenderStyle,
    /// The reference layer's opacity, 0.05–1.
    pub opacity: f32,
    /// Render the reference layer again whenever the set changes.
    pub auto_update: bool,
}

impl Default for ReferenceSettings {
    fn default() -> Self {
        Self {
            style: RenderStyle::Toon,
            opacity: 0.5,
            auto_update: true,
        }
    }
}

/// A panel layer tied to a point of the set (C12).
#[derive(Clone, Copy, Debug, PartialEq, Serialize, Deserialize)]
pub struct LayerAttachment {
    pub object: ObjectId,
    /// A mannequin bone the layer follows, instead of the object's origin.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub bone: Option<Bone>,
    /// The attached point in the object's (or bone's) frame, in metres.
    #[serde(default)]
    pub local: [f32; 3],
    /// Where the point last showed on the panel, in panel pixels.
    pub screen: [f64; 2],
    /// Its distance from the camera then, in metres.
    pub depth: f64,
}

/// A panel's 3D set and how it renders into the panel.
#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
pub struct PanelShot {
    pub set: Scene,
    #[serde(default)]
    pub reference: ReferenceSettings,
    /// The reference layer last rendered from this set.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub layer: Option<NodeId>,
    /// Layers that follow the set, by layer.
    #[serde(default, skip_serializing_if = "BTreeMap::is_empty")]
    pub attachments: BTreeMap<NodeId, LayerAttachment>,
}

impl PanelShot {
    /// An empty set with a key and fill light, its camera's film back
    /// shaped like the board (`aspect` = width / height).
    pub fn new(aspect: f32) -> Self {
        let mut set = Scene::new();
        set.camera.film_back = FilmBack::super35_for_aspect(aspect);
        Self {
            set,
            reference: ReferenceSettings::default(),
            layer: None,
            attachments: BTreeMap::new(),
        }
    }

    pub fn validate(&self) -> Result<(), String> {
        self.set
            .validate()
            .map_err(|e| format!("Shot Generator set: {e}"))?;
        let opacity = self.reference.opacity;
        if !(opacity.is_finite() && (0.05..=1.).contains(&opacity)) {
            return Err("The reference layer's opacity is 5–100%.".into());
        }
        if self.attachments.len() > MAX_ATTACHMENTS {
            return Err(format!(
                "At most {MAX_ATTACHMENTS} layers can follow one set."
            ));
        }
        for a in self.attachments.values() {
            let finite = a.local.iter().all(|v| v.is_finite())
                && a.screen.iter().all(|v| v.is_finite())
                && a.depth.is_finite();
            if !finite {
                return Err("A layer attachment has invalid values.".into());
            }
        }
        Ok(())
    }

    /// The model assets the set uses.
    pub fn model_ids(&self) -> impl Iterator<Item = &str> {
        self.set.objects.iter().filter_map(|o| match &o.kind {
            ObjectKind::Prop(Prop::Model(m)) => Some(m.asset.as_str()),
            _ => None,
        })
    }

    /// The set rendered through its camera for the reference layer: `width
    /// × height` straight-alpha RGBA with a transparent sky and ground.
    pub fn render_reference(
        &self,
        assets: &AssetLibrary,
        width: u32,
        height: u32,
    ) -> Result<RgbaImage, String> {
        let prepared = prepare(&self.set, assets).map_err(|e| e.to_string())?;
        Ok(self.render_prepared(&prepared, width, height))
    }

    /// [`Self::render_reference`] from an already prepared set.
    pub fn render_prepared(&self, prepared: &PreparedScene, width: u32, height: u32) -> RgbaImage {
        // Supersample smaller panels; 4K panels are sharp enough as they are.
        let supersample = if u64::from(width) * u64::from(height) <= 2_200_000 {
            2
        } else {
            1
        };
        let options = RenderOptions {
            transparent_background: true,
            supersample,
            ..RenderOptions::style(self.reference.style)
        };
        render(prepared, &self.set.camera, width, height, &options)
    }

    /// Ties layer `node` to `object` (or one of its bones) at `point` (a
    /// world point, such as a surface point picked in the viewport; the
    /// attachment's origin when `None`), as seen on a `width × height`
    /// panel now.
    pub fn attach(
        &mut self,
        prepared: &PreparedScene,
        node: NodeId,
        object: ObjectId,
        bone: Option<Bone>,
        point: Option<Vec3>,
        (width, height): (u32, u32),
    ) -> Result<(), String> {
        let m = attachment_matrix(&self.set, prepared, object, bone)
            .ok_or("That object is not in the set.")?;
        let world = point.unwrap_or_else(|| m.transform_point3(Vec3::ZERO));
        let local = m.inverse().transform_point3(world);
        let seen = project_point(&self.set.camera, width, height, world)
            .ok_or("That point is behind the camera.")?;
        self.attachments.insert(
            node,
            LayerAttachment {
                object,
                bone,
                local: local.to_array(),
                screen: [f64::from(seen.x), f64::from(seen.y)],
                depth: f64::from(seen.depth),
            },
        );
        self.validate()
    }

    /// Moves the attached layers of `doc` to where their points show now
    /// (the set in `prepared`), scaling them with their distance. Drops
    /// attachments whose layer or object is gone. Returns whether a layer
    /// moved.
    pub fn follow(
        &mut self,
        prepared: &PreparedScene,
        doc: &mut Document,
        (width, height): (u32, u32),
    ) -> Result<bool, String> {
        let set = &self.set;
        self.attachments
            .retain(|node, a| doc.node(*node).is_some() && set.object(a.object).is_some());
        // Locked layers hold still until unlocked.
        let held = |node: NodeId| {
            let locks = doc.layer_locks(node);
            doc.locked_ancestor(node).is_some() || locks.position
        };
        let perspective = matches!(
            self.set.camera.projection,
            emulsion_scene::Projection::Perspective
        );
        let mut moves = Vec::new();
        for (node, a) in &mut self.attachments {
            if held(*node) {
                continue;
            }
            let Some(m) = attachment_matrix(&self.set, prepared, a.object, a.bone) else {
                continue;
            };
            let world = m.transform_point3(Vec3::from_array(a.local));
            let Some(seen) = project_point(&self.set.camera, width, height, world) else {
                continue;
            };
            let (old, new) = (
                dvec2(a.screen[0], a.screen[1]),
                dvec2(f64::from(seen.x), f64::from(seen.y)),
            );
            let depth = f64::from(seen.depth);
            let scale = if perspective && depth > 1e-6 && a.depth > 1e-6 {
                (a.depth / depth).clamp(0.05, 20.)
            } else {
                1.
            };
            let m = DAffine2::from_translation(new)
                * DAffine2::from_scale(DVec2::splat(scale))
                * DAffine2::from_translation(-old);
            a.screen = new.to_array();
            a.depth = depth;
            if !m.abs_diff_eq(DAffine2::IDENTITY, 1e-9) {
                moves.push((*node, m));
            }
        }
        if moves.is_empty() {
            return Ok(false);
        }
        crate::motion::with_layers_unlocked(doc, |doc| {
            for (node, m) in &moves {
                crate::transform::transform_nodes(doc, &[*node], m.to_cols_array())
                    .map_err(|e| e.to_string())?;
            }
            Ok(())
        })?;
        Ok(true)
    }
}

/// An imported model file kept in the project.
#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
pub struct ModelAsset {
    /// The file's name without its extension.
    pub name: String,
    pub format: ModelFormat,
    /// The file's bytes; the package stores them beside the board.
    #[serde(skip)]
    pub data: Arc<[u8]>,
}

impl ModelAsset {
    pub fn extension(&self) -> &'static str {
        match self.format {
            ModelFormat::Gltf => "gltf",
            ModelFormat::Glb => "glb",
            ModelFormat::Obj => "obj",
        }
    }
}

/// Imported models and saved custom poses, shared by every panel's set.
#[derive(Clone, Debug, Default, PartialEq, Serialize, Deserialize)]
pub struct ShotLibrary {
    #[serde(default, skip_serializing_if = "BTreeMap::is_empty")]
    pub models: BTreeMap<String, ModelAsset>,
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    pub poses: Vec<Pose>,
}

/// A model's id: the first 16 hex digits of its SHA-256, so importing the
/// same file twice keeps one copy.
pub fn model_id(bytes: &[u8]) -> String {
    use sha2::{Digest, Sha256};
    Sha256::digest(bytes)
        .iter()
        .take(8)
        .map(|b| format!("{b:02x}"))
        .collect()
}

fn model_error(e: emulsion_scene::SceneError) -> String {
    let text = e.to_string();
    if text.contains("external buffer") {
        "This .gltf keeps its data in separate files. Export it as .glb (binary glTF) or with embedded buffers, then import it again.".into()
    } else {
        format!("The model could not be imported: {text}")
    }
}

impl ShotLibrary {
    pub fn is_empty(&self) -> bool {
        self.models.is_empty() && self.poses.is_empty()
    }

    /// The package entry of model `id`.
    pub fn entry_name(&self, id: &str) -> Option<String> {
        let asset = self.models.get(id)?;
        Some(format!("models/{id}.{}", asset.extension()))
    }

    /// Bytes of every model.
    pub fn model_bytes(&self) -> usize {
        self.models.values().map(|m| m.data.len()).sum()
    }

    /// Checks and keeps a model file (`file_name` gives its name and
    /// format), returning its id. Self-contained files only: `.glb`, `.obj`
    /// and `.gltf` with embedded buffers.
    pub fn add_model(&mut self, file_name: &str, bytes: Vec<u8>) -> Result<String, String> {
        let path = std::path::Path::new(file_name);
        let extension = path.extension().and_then(|e| e.to_str());
        let format = ModelFormat::detect(extension, &bytes)
            .ok_or("Choose a glTF (.gltf, .glb) or OBJ (.obj) model.")?;
        let name: String = path
            .file_stem()
            .and_then(|s| s.to_str())
            .unwrap_or("Model")
            .chars()
            .take(emulsion_scene::limits::MAX_NAME)
            .collect();
        import_bytes(&name, format, &bytes, None).map_err(model_error)?;
        let id = model_id(&bytes);
        if self.models.contains_key(&id) {
            return Ok(id);
        }
        if self.models.len() >= MAX_MODELS {
            return Err(format!("A project keeps at most {MAX_MODELS} models."));
        }
        if self.model_bytes() + bytes.len() > MAX_MODEL_BYTES {
            return Err(format!(
                "A project keeps at most {} MB of models.",
                MAX_MODEL_BYTES >> 20
            ));
        }
        self.models.insert(
            id.clone(),
            ModelAsset {
                name,
                format,
                data: bytes.into(),
            },
        );
        Ok(id)
    }

    /// Every model parsed, for rendering. A model that fails (it was
    /// checked when imported and opened) is left out and shows as a
    /// placeholder.
    pub fn assets(&self) -> AssetLibrary {
        let mut library = AssetLibrary::new();
        for (id, asset) in &self.models {
            if let Ok(model) = import_bytes(&asset.name, asset.format, &asset.data, None) {
                library.insert(id.clone(), model).ok();
            }
        }
        library
    }

    /// Checks every model's bytes parse (after opening a package).
    pub fn check_models(&self) -> Result<(), String> {
        for (id, asset) in &self.models {
            if model_id(&asset.data) != *id {
                return Err(format!("Model “{}” is damaged.", asset.name));
            }
            import_bytes(&asset.name, asset.format, &asset.data, None)
                .map_err(|e| format!("Model “{}”: {}", asset.name, model_error(e)))?;
        }
        Ok(())
    }

    /// Saves `pose` as custom pose `name`, replacing one of that name.
    pub fn save_pose(&mut self, name: &str, pose: &Pose) -> Result<(), String> {
        let name = name.trim();
        if name.is_empty() || name.chars().count() > 100 {
            return Err("Name the pose (up to 100 characters).".into());
        }
        let mut pose = pose.clone();
        pose.name = name.into();
        if let Some(existing) = self.poses.iter_mut().find(|p| p.name == name) {
            *existing = pose;
        } else if self.poses.len() >= MAX_POSES {
            return Err(format!("A project keeps at most {MAX_POSES} poses."));
        } else {
            self.poses.push(pose);
        }
        Ok(())
    }

    pub fn validate(&self) -> Result<(), String> {
        if self.models.len() > MAX_MODELS || self.model_bytes() > MAX_MODEL_BYTES {
            return Err(format!(
                "A project keeps at most {MAX_MODELS} models and {} MB of them.",
                MAX_MODEL_BYTES >> 20
            ));
        }
        for (id, asset) in &self.models {
            if id.len() != 16 || !id.bytes().all(|b| b.is_ascii_hexdigit()) {
                return Err("A model has an invalid id.".into());
            }
            if asset.name.chars().count() > emulsion_scene::limits::MAX_NAME {
                return Err("A model's name is too long.".into());
            }
        }
        if self.poses.len() > MAX_POSES {
            return Err(format!("A project keeps at most {MAX_POSES} poses."));
        }
        let mut names = BTreeSet::new();
        for pose in &self.poses {
            if pose.name.trim().is_empty() || !names.insert(pose.name.as_str()) {
                return Err("Custom poses need unique names.".into());
            }
            if !pose.joints.values().all(|r| r.is_finite()) || !pose.hips_offset.is_finite() {
                return Err(format!("Pose “{}” has invalid values.", pose.name));
            }
        }
        Ok(())
    }
}

impl Storyboard {
    /// Width / height of the panels.
    pub fn aspect(&self) -> f32 {
        self.settings.width as f32 / self.settings.height.max(1) as f32
    }

    /// Sets, layer depths and the shot library (part of `validate`).
    pub(crate) fn validate_shots(&self) -> Result<(), String> {
        self.shot_library.validate()?;
        for panel in self.panels.values() {
            if let Some(shot) = &panel.shot {
                shot.validate()?;
            }
            if panel.depth.len() > crate::storyboard_motion::MAX_ANIMATED_LAYERS
                || panel.depth.values().any(|d| !DEPTH_RANGE.contains(d))
            {
                return Err(format!(
                    "Layer depth is {}–{} times the camera's distance.",
                    DEPTH_RANGE.start(),
                    DEPTH_RANGE.end()
                ));
            }
        }
        Ok(())
    }

    /// Models some panel's set uses.
    pub fn used_models(&self) -> BTreeSet<String> {
        self.panels
            .values()
            .filter_map(|p| p.shot.as_deref())
            .flat_map(|s| s.model_ids().map(str::to_string))
            .collect()
    }

    /// Drops models that no set uses.
    pub fn prune_models(&mut self) {
        let used = self.used_models();
        self.shot_library.models.retain(|id, _| used.contains(id));
    }

    /// Sets `node`'s depth on `panel` (0 removes it).
    pub fn set_layer_depth(
        &mut self,
        panel: PageId,
        node: NodeId,
        depth: f64,
    ) -> Result<(), String> {
        if !DEPTH_RANGE.contains(&depth) {
            return Err(format!(
                "Layer depth is {}–{} times the camera's distance.",
                DEPTH_RANGE.start(),
                DEPTH_RANGE.end()
            ));
        }
        let p = self.panels.get_mut(&panel).ok_or("Panel does not exist.")?;
        if depth == 0. {
            p.depth.remove(&node);
        } else {
            p.depth.insert(node, depth);
        }
        Ok(())
    }

    /// Whether `panel` has layers in depth, so camera moves show parallax.
    pub fn has_parallax(&self, panel: PageId) -> bool {
        self.panels.get(&panel).is_some_and(|p| !p.depth.is_empty())
    }

    /// `doc` (panel `panel`) with its layers in depth placed for the
    /// camera `state` (L6): each layer gets the parallax of its depth.
    /// Layers behind the camera are hidden. Unchanged at rest.
    pub fn parallax_panel(
        &self,
        panel: PageId,
        doc: &Document,
        state: CameraState,
    ) -> Result<Document, String> {
        let mut out = doc.clone();
        let Some(p) = self.panels.get(&panel) else {
            return Ok(out);
        };
        if p.depth.is_empty() || state == self.rest_camera() {
            return Ok(out);
        }
        let size = (self.settings.width, self.settings.height);
        crate::motion::with_layers_unlocked(&mut out, |out| {
            for (&id, &depth) in &p.depth {
                if out.node(id).is_none() {
                    continue;
                }
                match parallax_transform(size, state, depth) {
                    Some(m) => {
                        if !m.abs_diff_eq(DAffine2::IDENTITY, 1e-9) {
                            crate::transform::transform_nodes(out, &[id], m.to_cols_array())
                                .map_err(|e| e.to_string())?;
                        }
                    }
                    None => {
                        Command::SetVisible { id, visible: false }
                            .apply(out)
                            .map_err(|e| e.to_string())?;
                    }
                }
            }
            Ok(())
        })?;
        Ok(out)
    }
}

/// The 3D camera the 2D scene camera `state` stands for on a panel
/// `width` pixels wide, in a world where panel point (x, y) at depth z is
/// (-x, -y, z) and the panel plane lies at z = 0: a 35 mm lens whose view at
/// rest exactly covers the panel. Also returns its distance to the panel
/// plane at rest, in pixels.
pub fn parallax_camera(width: u32, state: CameraState) -> (Camera, f64) {
    let film = FilmBack::SUPER_35;
    let half = (horizontal_fov_deg(PARALLAX_FOCAL_MM, film.width_mm) * 0.5).to_radians();
    let distance = f64::from(width) * 0.5 / f64::from(half.tan());
    let zoom = state.zoom.max(1e-3);
    let camera = Camera {
        position: Vec3::new(-state.x as f32, -state.y as f32, -(distance / zoom) as f32),
        yaw: 0.,
        pitch: 0.,
        roll: 0.,
        focal_length_mm: PARALLAX_FOCAL_MM,
        film_back: film,
        projection: emulsion_scene::Projection::Perspective,
        near: 1e-3,
        far: 1e9,
    };
    (camera, distance)
}

/// The parallax of a layer at `depth` (multiples of the camera's distance
/// to the panel; 0 is the panel plane) under the camera `state`, as an
/// affine map of panel pixels that the 2D camera then shows like any layer.
/// Identity at rest; `None` when the layer is behind the camera.
pub fn parallax_transform(size: (u32, u32), state: CameraState, depth: f64) -> Option<DAffine2> {
    let (width, height) = (f64::from(size.0), f64::from(size.1));
    let (camera, distance) = parallax_camera(size.0, state);
    let rest = dvec2(width / 2., height / 2.);
    let center = rest;
    let zoom = state.zoom.max(1e-3);
    // A layer at depth z is drawn so that, seen from the rest camera, it
    // covers what it was drawn on: it is scaled up by (1 + depth).
    let shown = |p: DVec2| -> Option<DVec2> {
        let world = rest + (p - rest) * (1. + depth);
        let point = Vec3::new(-world.x as f32, -world.y as f32, (depth * distance) as f32);
        let s = project_point(&camera, size.0, size.1, point)?;
        Some(dvec2(state.x, state.y) + (dvec2(f64::from(s.x), f64::from(s.y)) - center) / zoom)
    };
    let (dx, dy) = (width / 2., height / 2.);
    let o = shown(rest)?;
    let x = (shown(rest + dvec2(dx, 0.))? - o) / dx;
    let y = (shown(rest + dvec2(0., dy))? - o) / dy;
    let m = DAffine2::from_cols(x, y, o - x * rest.x - y * rest.y);
    // f32 projection: snap what is the identity at rest.
    Some(if m.abs_diff_eq(DAffine2::IDENTITY, 2e-3) {
        DAffine2::IDENTITY
    } else {
        m
    })
}

/// A snake_case name as a label: "lamp_post" → "Lamp post".
pub fn label_of(snake: &str) -> String {
    let words = snake.replace('_', " ");
    let mut c = words.chars();
    c.next()
        .map(|f| f.to_uppercase().collect::<String>() + c.as_str())
        .unwrap_or_default()
}

/// A rendered set as a layer's pixels.
pub fn image_raster(image: &RgbaImage) -> Arc<Raster> {
    Arc::new(Raster::from_srgba8(
        image.width,
        image.height,
        &image.pixels,
    ))
}

#[cfg(test)]
#[path = "storyboard_shot_tests.rs"]
mod tests;
