//! The scene model (SG1): objects, camera, lights, ground and sky, with
//! validation, limits, stable ids and gizmo-friendly editing helpers.

use std::collections::{BTreeMap, BTreeSet};

use glam::{Quat, Vec3};
use serde::{Deserialize, Serialize};

use crate::camera::Camera;
use crate::error::SceneError;
use crate::mannequin::{MannequinKind, MannequinParams};
use crate::math::{Rotation, Transform};
use crate::pose::{FacePreset, Limb, Pose, PosePreset};
use crate::props::{BuiltinProp, PropKind};
use crate::skeleton::{Bone, JointRotation};

/// Current scene format version.
pub const SCENE_VERSION: u32 = 1;

/// Hard limits that keep scenes interactive and files bounded.
pub mod limits {
    /// Objects per scene.
    pub const MAX_OBJECTS: usize = 256;
    /// Triangles per scene after tessellation.
    pub const MAX_TRIANGLES: usize = 2_000_000;
    /// Bytes of one imported model file.
    pub const MAX_ASSET_BYTES: usize = 64 * 1024 * 1024;
    /// Triangles of one imported model.
    pub const MAX_ASSET_TRIANGLES: usize = 1_000_000;
    /// Absolute coordinate bound in metres.
    pub const MAX_COORDINATE: f32 = 100_000.0;
    /// Length of names in characters.
    pub const MAX_NAME: usize = 200;
}

/// Stable object identifier, unique within a scene and never reused.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, PartialOrd, Ord, Serialize, Deserialize)]
#[serde(transparent)]
pub struct ObjectId(pub u64);

/// An sRGB colour with 8-bit channels.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, Serialize, Deserialize)]
#[serde(transparent)]
pub struct Rgb(pub [u8; 3]);

impl Rgb {
    pub const WHITE: Rgb = Rgb([255, 255, 255]);
    pub const CHARACTER: Rgb = Rgb([214, 219, 228]);
    pub const PROP: Rgb = Rgb([200, 196, 188]);

    /// Linear-light components 0..1.
    pub fn to_linear(self) -> Vec3 {
        let f = |c: u8| {
            let c = c as f32 / 255.0;
            if c <= 0.04045 {
                c / 12.92
            } else {
                ((c + 0.055) / 1.055).powf(2.4)
            }
        };
        Vec3::new(f(self.0[0]), f(self.0[1]), f(self.0[2]))
    }
}

/// A posable figure (SG2, SG3, SG8).
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[serde(default)]
pub struct Character {
    pub body: MannequinParams,
    pub pose: Pose,
    /// Two-bone IK targets in world space (wrist / ankle positions),
    /// applied after the pose.
    pub ik: Vec<IkTarget>,
    /// World point the head turns toward.
    pub look_at: Option<Vec3>,
    pub face: FacePreset,
    /// Shift the figure vertically so its lowest point rests on the ground
    /// (y = 0 of the object). Ignored while a foot has an IK target.
    pub ground_lock: bool,
}

impl Default for Character {
    fn default() -> Self {
        Character {
            body: MannequinParams::default(),
            pose: PosePreset::Stand.pose(),
            ik: Vec::new(),
            look_at: None,
            face: FacePreset::Neutral,
            ground_lock: true,
        }
    }
}

impl Character {
    pub fn of(kind: MannequinKind) -> Self {
        Character {
            body: MannequinParams::of(kind),
            ..Default::default()
        }
    }

    pub fn with_pose(mut self, preset: PosePreset) -> Self {
        self.pose = preset.pose();
        self
    }
}

/// An IK goal for one limb.
#[derive(Debug, Clone, Copy, PartialEq, Serialize, Deserialize)]
pub struct IkTarget {
    pub limb: Limb,
    /// World-space target for the wrist or ankle.
    pub target: Vec3,
    /// World-space bend direction for the elbow/knee (optional).
    #[serde(default)]
    pub pole: Option<Vec3>,
}

/// Where a prop's geometry comes from.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[serde(tag = "source", rename_all = "snake_case")]
pub enum Prop {
    /// A parametric built-in.
    Builtin(BuiltinProp),
    /// An imported glTF/OBJ model from the [`crate::AssetLibrary`].
    Model(ModelRef),
}

impl Prop {
    pub fn builtin(kind: PropKind) -> Prop {
        Prop::Builtin(BuiltinProp::new(kind))
    }
}

/// Reference to an imported model plus its pose (C7, C10).
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct ModelRef {
    pub asset: String,
    /// Extra local rotation per node/joint name, applied on top of the
    /// model's rest pose (poses existing glTF skins).
    #[serde(default)]
    pub joint_rotations: BTreeMap<String, JointRotation>,
}

/// Light role. The toon renderer treats all lights as directional; the light
/// shines along its object's forward (+Z) direction.
#[derive(Debug, Clone, Copy, Default, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum LightKind {
    #[default]
    Key,
    Fill,
    Rim,
}

/// A directional light (SG7).
#[derive(Debug, Clone, Copy, PartialEq, Serialize, Deserialize)]
#[serde(default)]
pub struct Light {
    pub kind: LightKind,
    /// 0..4; 1 is a normal key light.
    pub intensity: f32,
}

impl Default for Light {
    fn default() -> Self {
        Light {
            kind: LightKind::Key,
            intensity: 1.0,
        }
    }
}

/// What an object is.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[serde(tag = "type", rename_all = "snake_case")]
pub enum ObjectKind {
    Character(Character),
    Prop(Prop),
    Light(Light),
}

/// One object in the scene.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct SceneObject {
    pub id: ObjectId,
    #[serde(default)]
    pub name: String,
    #[serde(default)]
    pub transform: Transform,
    #[serde(default = "yes")]
    pub visible: bool,
    /// Base colour used by the toon render.
    #[serde(default = "default_color")]
    pub color: Rgb,
    /// Characters and props: whether they cast shadows. Lights: whether a
    /// key light casts shadows (fill and rim lights never do).
    #[serde(default = "yes", skip_serializing_if = "is_true")]
    pub casts_shadows: bool,
    #[serde(flatten)]
    pub kind: ObjectKind,
}

fn yes() -> bool {
    true
}

fn is_true(v: &bool) -> bool {
    *v
}

fn default_color() -> Rgb {
    Rgb::PROP
}

impl SceneObject {
    pub fn character(&self) -> Option<&Character> {
        match &self.kind {
            ObjectKind::Character(c) => Some(c),
            _ => None,
        }
    }

    pub fn character_mut(&mut self) -> Option<&mut Character> {
        match &mut self.kind {
            ObjectKind::Character(c) => Some(c),
            _ => None,
        }
    }
}

/// Ground, sky and ambient light.
#[derive(Debug, Clone, Copy, PartialEq, Serialize, Deserialize)]
#[serde(default)]
pub struct Environment {
    pub sky_color: Rgb,
    /// Colour at the horizon (the sky blends toward it).
    pub horizon_color: Rgb,
    pub ground_color: Rgb,
    pub show_ground: bool,
    pub show_grid: bool,
    /// Grid spacing in metres.
    pub grid_spacing: f32,
    pub show_horizon: bool,
    /// Ambient light 0..1.
    pub ambient: f32,
}

impl Default for Environment {
    fn default() -> Self {
        Environment {
            sky_color: Rgb([236, 240, 246]),
            horizon_color: Rgb([250, 250, 250]),
            ground_color: Rgb([226, 224, 220]),
            show_ground: true,
            show_grid: true,
            grid_spacing: 1.0,
            show_horizon: true,
            ambient: 0.35,
        }
    }
}

/// A 3D set: objects, the shot camera and the environment. Units are metres,
/// Y is up and the ground is the plane y = 0.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[serde(default)]
pub struct Scene {
    pub version: u32,
    pub objects: Vec<SceneObject>,
    pub camera: Camera,
    pub environment: Environment,
    /// Next id to hand out (ids are never reused).
    pub next_id: u64,
    /// The shot the camera was last framed as (see [`Scene::current_shot`]).
    #[serde(skip_serializing_if = "Option::is_none")]
    pub framing: Option<crate::shot::Framing>,
}

impl Default for Scene {
    fn default() -> Self {
        Scene {
            version: SCENE_VERSION,
            objects: Vec::new(),
            camera: Camera::default(),
            environment: Environment::default(),
            next_id: 1,
            framing: None,
        }
    }
}

impl Scene {
    /// An empty scene with a key and a fill light.
    pub fn new() -> Scene {
        let mut s = Scene::default();
        s.add_default_lights();
        s
    }

    /// Adds the standard key (front-left, high) and fill (front-right, low) lights.
    pub fn add_default_lights(&mut self) {
        let key = self.add(
            "Key light",
            ObjectKind::Light(Light {
                kind: LightKind::Key,
                intensity: 0.9,
            }),
        );
        self.set_transform(
            key,
            Transform {
                position: Vec3::new(-3.0, 5.0, -4.0),
                rotation: Rotation::euler(35.0, -50.0, 0.0),
                scale: Vec3::ONE,
            },
        );
        let fill = self.add(
            "Fill light",
            ObjectKind::Light(Light {
                kind: LightKind::Fill,
                intensity: 0.35,
            }),
        );
        self.set_transform(
            fill,
            Transform {
                position: Vec3::new(4.0, 2.0, -3.0),
                rotation: Rotation::euler(-130.0, -20.0, 0.0),
                scale: Vec3::ONE,
            },
        );
    }

    /// Adds an object and returns its new id. Characters get the character
    /// colour; everything else the prop colour.
    pub fn add(&mut self, name: &str, kind: ObjectKind) -> ObjectId {
        let next = self
            .objects
            .iter()
            .map(|o| o.id.0 + 1)
            .max()
            .unwrap_or(1)
            .max(self.next_id);
        let id = ObjectId(next);
        self.next_id = next + 1;
        let color = match kind {
            ObjectKind::Character(_) => Rgb::CHARACTER,
            ObjectKind::Light(_) => Rgb::WHITE,
            ObjectKind::Prop(_) => Rgb::PROP,
        };
        self.objects.push(SceneObject {
            id,
            name: name.chars().take(limits::MAX_NAME).collect(),
            transform: Transform::default(),
            visible: true,
            color,
            casts_shadows: true,
            kind,
        });
        id
    }

    /// Adds a character standing at `position` facing `yaw` degrees.
    pub fn add_character(
        &mut self,
        name: &str,
        character: Character,
        position: Vec3,
        yaw: f32,
    ) -> ObjectId {
        let id = self.add(name, ObjectKind::Character(character));
        self.set_transform(id, Transform::at_yaw(position, yaw));
        id
    }

    /// Adds a built-in prop at `position` turned `yaw` degrees.
    pub fn add_prop(&mut self, name: &str, prop: Prop, position: Vec3, yaw: f32) -> ObjectId {
        let id = self.add(name, ObjectKind::Prop(prop));
        self.set_transform(id, Transform::at_yaw(position, yaw));
        id
    }

    /// Removes an object; returns it when it existed.
    pub fn remove(&mut self, id: ObjectId) -> Option<SceneObject> {
        let i = self.objects.iter().position(|o| o.id == id)?;
        Some(self.objects.remove(i))
    }

    pub fn object(&self, id: ObjectId) -> Option<&SceneObject> {
        self.objects.iter().find(|o| o.id == id)
    }

    pub fn object_mut(&mut self, id: ObjectId) -> Option<&mut SceneObject> {
        self.objects.iter_mut().find(|o| o.id == id)
    }

    /// Finds an object by (case-insensitive) name.
    pub fn find(&self, name: &str) -> Option<&SceneObject> {
        self.objects
            .iter()
            .find(|o| o.name.eq_ignore_ascii_case(name))
    }

    pub fn character(&self, id: ObjectId) -> Option<&Character> {
        self.object(id).and_then(|o| o.character())
    }

    pub fn character_mut(&mut self, id: ObjectId) -> Option<&mut Character> {
        self.object_mut(id).and_then(|o| o.character_mut())
    }

    /// Ids of all characters, in object order.
    pub fn character_ids(&self) -> Vec<ObjectId> {
        self.objects
            .iter()
            .filter(|o| matches!(o.kind, ObjectKind::Character(_)))
            .map(|o| o.id)
            .collect()
    }

    // ---- Gizmo-friendly setters. Each returns false for an unknown id. ----

    pub fn set_transform(&mut self, id: ObjectId, t: Transform) -> bool {
        self.object_mut(id).map(|o| o.transform = t).is_some()
    }

    pub fn set_position(&mut self, id: ObjectId, p: Vec3) -> bool {
        self.object_mut(id)
            .map(|o| o.transform.position = p)
            .is_some()
    }

    /// Moves an object by a world-space delta (translate gizmo drag).
    pub fn translate(&mut self, id: ObjectId, delta: Vec3) -> bool {
        self.object_mut(id)
            .map(|o| o.transform.position += delta)
            .is_some()
    }

    /// Sets yaw/pitch/roll in degrees.
    pub fn set_rotation_euler(&mut self, id: ObjectId, yaw: f32, pitch: f32, roll: f32) -> bool {
        self.object_mut(id)
            .map(|o| o.transform.rotation = Rotation::euler(yaw, pitch, roll))
            .is_some()
    }

    /// Rotates about a world axis through the object's origin (rotate gizmo
    /// ring drag). Keeps the yaw/pitch/roll form.
    pub fn rotate_about(&mut self, id: ObjectId, axis: Vec3, degrees: f32) -> bool {
        let Some(axis) = axis.try_normalize() else {
            return false;
        };
        self.object_mut(id)
            .map(|o| {
                let q = Quat::from_axis_angle(axis, degrees.to_radians())
                    * o.transform.rotation.to_quat();
                let (y, p, r) = crate::math::quat_to_yaw_pitch_roll(q);
                o.transform.rotation = Rotation::euler(y, p, r);
            })
            .is_some()
    }

    /// Sets a uniform or per-axis scale (clamped positive).
    pub fn set_scale(&mut self, id: ObjectId, s: Vec3) -> bool {
        let s = s.max(Vec3::splat(1e-3));
        self.object_mut(id).map(|o| o.transform.scale = s).is_some()
    }

    /// Sets one joint of a character's pose, clamped to joint limits.
    pub fn set_joint(&mut self, id: ObjectId, bone: Bone, r: JointRotation) -> bool {
        self.character_mut(id)
            .map(|c| c.pose.set(bone, r))
            .is_some()
    }

    /// Sets (or replaces) a world-space IK target for one limb.
    pub fn set_ik_target(&mut self, id: ObjectId, limb: Limb, target: Vec3) -> bool {
        self.character_mut(id)
            .map(|c| {
                c.ik.retain(|t| t.limb != limb);
                c.ik.push(IkTarget {
                    limb,
                    target,
                    pole: None,
                });
            })
            .is_some()
    }

    /// Bakes IK targets and look-at into the stored pose and clears them, so
    /// the pose can be saved or edited further with FK.
    pub fn bake_pose(&mut self, id: ObjectId) -> bool {
        let Some(obj) = self.object(id) else {
            return false;
        };
        let Some(posed) = crate::character::pose_character(obj) else {
            return false;
        };
        let pose = posed.pose;
        self.character_mut(id)
            .map(|c| {
                c.pose = pose;
                c.ik.clear();
                c.look_at = None;
            })
            .is_some()
    }

    /// Checks the scene against the model's rules and [`limits`].
    pub fn validate(&self) -> Result<(), SceneError> {
        if self.version > SCENE_VERSION {
            return Err(SceneError::Invalid(format!(
                "scene version {} is newer than supported {}",
                self.version, SCENE_VERSION
            )));
        }
        if self.objects.len() > limits::MAX_OBJECTS {
            return Err(SceneError::Limit(format!(
                "{} objects (max {})",
                self.objects.len(),
                limits::MAX_OBJECTS
            )));
        }
        if !self.camera.is_valid() {
            return Err(SceneError::Invalid("camera has invalid values".into()));
        }
        if self.camera.position.abs().max_element() > limits::MAX_COORDINATE {
            return Err(SceneError::Invalid("camera is too far away".into()));
        }
        let env = &self.environment;
        if !(env.grid_spacing.is_finite() && env.grid_spacing > 0.0 && env.ambient.is_finite()) {
            return Err(SceneError::Invalid("environment has invalid values".into()));
        }
        let mut ids = BTreeSet::new();
        for o in &self.objects {
            if !ids.insert(o.id) {
                return Err(SceneError::Invalid(format!(
                    "duplicate object id {}",
                    o.id.0
                )));
            }
            if o.name.chars().count() > limits::MAX_NAME {
                return Err(SceneError::Invalid(format!(
                    "object {} name too long",
                    o.id.0
                )));
            }
            if !o.transform.is_valid() {
                return Err(SceneError::Invalid(format!(
                    "object {} has an invalid transform",
                    o.id.0
                )));
            }
            if o.transform.position.abs().max_element() > limits::MAX_COORDINATE {
                return Err(SceneError::Invalid(format!(
                    "object {} is too far away",
                    o.id.0
                )));
            }
            match &o.kind {
                ObjectKind::Character(c) => {
                    let b = &c.body;
                    let vals = [
                        b.build,
                        b.head_size,
                        b.leg_length,
                        b.arm_length,
                        b.shoulder_width,
                        b.hip_width,
                    ];
                    if vals.iter().any(|v| !v.is_finite())
                        || b.height.is_some_and(|h| !h.is_finite())
                    {
                        return Err(SceneError::Invalid(format!(
                            "character {} has invalid body values",
                            o.id.0
                        )));
                    }
                    if c.body.clamped() != c.body {
                        return Err(SceneError::Invalid(format!(
                            "character {} body sliders are out of range",
                            o.id.0
                        )));
                    }
                    if !c.pose.joints.values().all(|r| r.is_finite())
                        || !c.pose.hips_offset.is_finite()
                    {
                        return Err(SceneError::Invalid(format!(
                            "character {} pose is not finite",
                            o.id.0
                        )));
                    }
                    if c.ik
                        .iter()
                        .any(|t| !t.target.is_finite() || t.pole.is_some_and(|p| !p.is_finite()))
                        || c.look_at.is_some_and(|p| !p.is_finite())
                    {
                        return Err(SceneError::Invalid(format!(
                            "character {} has invalid targets",
                            o.id.0
                        )));
                    }
                }
                ObjectKind::Prop(Prop::Builtin(p)) => p
                    .validate()
                    .map_err(|e| SceneError::Invalid(format!("prop {}: {e}", o.id.0)))?,
                ObjectKind::Prop(Prop::Model(m)) => {
                    if m.asset.is_empty() || !m.joint_rotations.values().all(|r| r.is_finite()) {
                        return Err(SceneError::Invalid(format!("model {} is invalid", o.id.0)));
                    }
                }
                ObjectKind::Light(l) => {
                    if !(l.intensity.is_finite() && (0.0..=4.0).contains(&l.intensity)) {
                        return Err(SceneError::Invalid(format!(
                            "light {} intensity out of range",
                            o.id.0
                        )));
                    }
                }
            }
        }
        if let Some(max) = ids.last()
            && max.0 >= self.next_id
        {
            return Err(SceneError::Invalid(
                "next_id must exceed every object id".into(),
            ));
        }
        Ok(())
    }

    /// Parses JSON and validates. A missing or stale `next_id` is derived
    /// from the ids present (hand-written and assistant-built sets omit it).
    pub fn from_json(json: &str) -> Result<Scene, SceneError> {
        let mut s: Scene =
            serde_json::from_str(json).map_err(|e| SceneError::Parse(e.to_string()))?;
        let max = s.objects.iter().map(|o| o.id.0).max().unwrap_or(0);
        s.next_id = s.next_id.max(max + 1);
        s.validate()?;
        Ok(s)
    }

    /// Serializes to pretty JSON.
    pub fn to_json(&self) -> String {
        serde_json::to_string_pretty(self).unwrap_or_default()
    }

    /// Repairs a deserialized scene in place: fixes `next_id`, clamps sliders
    /// and joints and drops non-finite values. Use before [`Scene::validate`]
    /// on data from older or hand-written files.
    pub fn normalize(&mut self) {
        let max = self.objects.iter().map(|o| o.id.0).max().unwrap_or(0);
        self.next_id = self.next_id.max(max + 1);
        for o in &mut self.objects {
            if let ObjectKind::Character(c) = &mut o.kind {
                c.body = c.body.clamped();
                c.pose.clamp_to_limits();
            }
            if !o.transform.is_valid() {
                o.transform = Transform::at(if o.transform.position.is_finite() {
                    o.transform.position
                } else {
                    Vec3::ZERO
                });
            }
        }
    }
}
