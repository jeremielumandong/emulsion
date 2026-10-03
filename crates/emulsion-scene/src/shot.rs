//! Shot sizes, camera angles and framing (SG5), plus the Shot Explorer (SG6).

use glam::{Quat, Vec3};
use serde::{Deserialize, Serialize};

use crate::camera::Camera;
use crate::character::{PosedCharacter, pose_character};
use crate::error::SceneError;
use crate::import::AssetLibrary;
use crate::math::Aabb;
use crate::scene::{ObjectId, ObjectKind, Prop, Scene};
use crate::skeleton::Bone;

/// Standard shot sizes, tightest first.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, PartialOrd, Ord, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum ShotSize {
    /// Eyes / a detail.
    ExtremeCloseUp,
    /// Head and the top of the shoulders.
    CloseUp,
    /// Head to mid-chest.
    MediumCloseUp,
    /// Head to the waist.
    Medium,
    /// Head to the knees ("cowboy").
    MediumWide,
    /// The whole figure.
    Wide,
    /// The figure small in its surroundings.
    ExtremeWide,
}

impl ShotSize {
    pub const ALL: [ShotSize; 7] = [
        ShotSize::ExtremeCloseUp,
        ShotSize::CloseUp,
        ShotSize::MediumCloseUp,
        ShotSize::Medium,
        ShotSize::MediumWide,
        ShotSize::Wide,
        ShotSize::ExtremeWide,
    ];

    pub fn abbreviation(self) -> &'static str {
        match self {
            ShotSize::ExtremeCloseUp => "ECU",
            ShotSize::CloseUp => "CU",
            ShotSize::MediumCloseUp => "MCU",
            ShotSize::Medium => "MS",
            ShotSize::MediumWide => "MWS",
            ShotSize::Wide => "WS",
            ShotSize::ExtremeWide => "EWS",
        }
    }

    pub fn label(self) -> &'static str {
        match self {
            ShotSize::ExtremeCloseUp => "extreme close-up",
            ShotSize::CloseUp => "close-up",
            ShotSize::MediumCloseUp => "medium close-up",
            ShotSize::Medium => "medium shot",
            ShotSize::MediumWide => "medium wide shot",
            ShotSize::Wide => "wide shot",
            ShotSize::ExtremeWide => "extreme wide shot",
        }
    }

    /// A lens that suits the size (used by the Shot Explorer).
    pub fn suggested_focal_length(self) -> f32 {
        match self {
            ShotSize::ExtremeCloseUp => 100.0,
            ShotSize::CloseUp => 85.0,
            ShotSize::MediumCloseUp => 65.0,
            ShotSize::Medium => 50.0,
            ShotSize::MediumWide => 35.0,
            ShotSize::Wide => 24.0,
            ShotSize::ExtremeWide => 18.0,
        }
    }

    /// Frame height as a multiple of a prop's bounding diameter.
    fn prop_frame(self) -> f32 {
        match self {
            ShotSize::ExtremeCloseUp => 0.22,
            ShotSize::CloseUp => 0.38,
            ShotSize::MediumCloseUp => 0.5,
            ShotSize::Medium => 0.65,
            ShotSize::MediumWide => 0.85,
            ShotSize::Wide => 1.15,
            ShotSize::ExtremeWide => 4.0,
        }
    }
}

/// Camera angle presets.
#[derive(Debug, Clone, Copy, Default, PartialEq, Eq, Hash, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum CameraAngle {
    #[default]
    EyeLevel,
    Low,
    High,
    BirdsEye,
    WormsEye,
    Dutch,
    /// Over the secondary character's shoulder toward the subject.
    OverTheShoulder,
    /// Frames the subject and the secondary character together.
    TwoShot,
}

impl CameraAngle {
    pub const ALL: [CameraAngle; 8] = [
        CameraAngle::EyeLevel,
        CameraAngle::Low,
        CameraAngle::High,
        CameraAngle::BirdsEye,
        CameraAngle::WormsEye,
        CameraAngle::Dutch,
        CameraAngle::OverTheShoulder,
        CameraAngle::TwoShot,
    ];

    pub fn label(self) -> &'static str {
        match self {
            CameraAngle::EyeLevel => "eye-level",
            CameraAngle::Low => "low-angle",
            CameraAngle::High => "high-angle",
            CameraAngle::BirdsEye => "bird's-eye",
            CameraAngle::WormsEye => "worm's-eye",
            CameraAngle::Dutch => "dutch",
            CameraAngle::OverTheShoulder => "over-the-shoulder",
            CameraAngle::TwoShot => "two-shot",
        }
    }

    /// Camera elevation above the subject in degrees and roll.
    fn elevation_roll(self) -> (f32, f32) {
        match self {
            CameraAngle::EyeLevel | CameraAngle::TwoShot => (0.0, 0.0),
            CameraAngle::Low => (-22.0, 0.0),
            CameraAngle::High => (28.0, 0.0),
            CameraAngle::BirdsEye => (82.0, 0.0),
            CameraAngle::WormsEye => (-55.0, 0.0),
            CameraAngle::Dutch => (0.0, 18.0),
            CameraAngle::OverTheShoulder => (6.0, 0.0),
        }
    }

    pub fn needs_secondary(self) -> bool {
        matches!(self, CameraAngle::OverTheShoulder | CameraAngle::TwoShot)
    }
}

/// Which side of the subject the camera is on, relative to where the subject
/// faces.
#[derive(Debug, Clone, Copy, Default, PartialEq, Eq, Hash, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum ShotSide {
    #[default]
    Front,
    /// Three-quarter view from the subject's left.
    FrontLeft,
    FrontRight,
    /// Profile from the subject's left.
    Left,
    Right,
    BackLeft,
    BackRight,
    Back,
}

impl ShotSide {
    pub const ALL: [ShotSide; 8] = [
        ShotSide::Front,
        ShotSide::FrontLeft,
        ShotSide::FrontRight,
        ShotSide::Left,
        ShotSide::Right,
        ShotSide::BackLeft,
        ShotSide::BackRight,
        ShotSide::Back,
    ];

    /// Azimuth from the subject's facing, degrees (positive toward its left).
    pub fn azimuth(self) -> f32 {
        match self {
            ShotSide::Front => 0.0,
            ShotSide::FrontLeft => 40.0,
            ShotSide::FrontRight => -40.0,
            ShotSide::Left => 90.0,
            ShotSide::Right => -90.0,
            ShotSide::BackLeft => 140.0,
            ShotSide::BackRight => -140.0,
            ShotSide::Back => 180.0,
        }
    }

    pub fn label(self) -> &'static str {
        match self {
            ShotSide::Front => "front",
            ShotSide::FrontLeft => "three-quarter left",
            ShotSide::FrontRight => "three-quarter right",
            ShotSide::Left => "left profile",
            ShotSide::Right => "right profile",
            ShotSide::BackLeft => "from behind left",
            ShotSide::BackRight => "from behind right",
            ShotSide::Back => "from behind",
        }
    }
}

/// Everything that defines a framed shot.
#[derive(Debug, Clone, Copy, PartialEq, Serialize, Deserialize)]
pub struct ShotSpec {
    pub size: ShotSize,
    #[serde(default)]
    pub angle: CameraAngle,
    #[serde(default)]
    pub side: ShotSide,
    pub subject: ObjectId,
    /// Second character for over-the-shoulder and two-shots.
    #[serde(default)]
    pub secondary: Option<ObjectId>,
    /// Frame a body part of a character subject instead of the figure
    /// (e.g. the right hand for "extreme close-up of a hand").
    #[serde(default)]
    pub focus: Option<Bone>,
    /// Lens; `None` keeps the scene camera's focal length.
    #[serde(default)]
    pub focal_length_mm: Option<f32>,
    /// Widen the frame to include every visible character (group shots).
    #[serde(default)]
    pub group: bool,
}

impl ShotSpec {
    pub fn new(subject: ObjectId, size: ShotSize) -> Self {
        ShotSpec {
            size,
            angle: CameraAngle::EyeLevel,
            side: ShotSide::Front,
            subject,
            secondary: None,
            focus: None,
            focal_length_mm: None,
            group: false,
        }
    }

    /// A readable name such as "Low-angle close-up, three-quarter left".
    pub fn name(&self) -> String {
        let mut s = if self.angle == CameraAngle::EyeLevel {
            self.size.label().to_string()
        } else if self.angle == CameraAngle::TwoShot {
            format!("{} two-shot", self.size.label().trim_end_matches(" shot"))
        } else {
            format!("{} {}", self.angle.label(), self.size.label())
        };
        if self.angle != CameraAngle::OverTheShoulder && self.side != ShotSide::Front {
            s.push_str(", ");
            s.push_str(self.side.label());
        }
        let mut c = s.chars();
        match c.next() {
            Some(f) => f.to_uppercase().collect::<String>() + c.as_str(),
            None => s,
        }
    }
}

/// The region a shot has to show.
struct Target {
    /// Point the camera aims at.
    center: Vec3,
    /// Height of the frame at the subject (metres).
    frame_height: f32,
    /// Minimum width of the frame (two-shots).
    frame_width: f32,
    /// Horizontal facing of the subject.
    facing: Vec3,
}

/// A shot spec together with the camera it produced, kept on the [`Scene`]
/// so panels can show which framing the camera is in.
#[derive(Debug, Clone, Copy, PartialEq, Serialize, Deserialize)]
pub struct Framing {
    pub spec: ShotSpec,
    pub camera: Camera,
}

impl Scene {
    /// Moves the camera to `camera`, remembering that it frames `spec`.
    pub fn apply_shot(&mut self, spec: ShotSpec, camera: Camera) {
        self.camera = camera;
        self.framing = Some(Framing { spec, camera });
    }

    /// The spec the camera was last framed with, while the camera has not
    /// moved since (a free move, a lens change or a slider clears it).
    pub fn current_shot(&self) -> Option<&ShotSpec> {
        let f = self.framing.as_ref()?;
        let (a, b) = (&f.camera, &self.camera);
        let same = (a.position - b.position).length() < 1e-4
            && (a.yaw - b.yaw).abs() < 1e-3
            && (a.pitch - b.pitch).abs() < 1e-3
            && (a.roll - b.roll).abs() < 1e-3
            && (a.focal_length_mm - b.focal_length_mm).abs() < 1e-3
            && a.projection == b.projection;
        same.then_some(&f.spec)
    }
}

/// Frames `spec` in `scene`: returns a camera that keeps the scene camera's
/// film back (and lens unless the spec sets one). `aspect` is the board's
/// width / height.
///
/// When the framed camera would sit inside an object or the subject's head
/// is hidden behind one, the camera is nudged (dollied in along its view
/// line with the lens widened to keep the framing, raised a little or
/// swung a few degrees); when no small nudge helps the plain framing is
/// returned.
pub fn frame_shot(
    scene: &Scene,
    assets: &AssetLibrary,
    spec: &ShotSpec,
    aspect: f32,
) -> Result<Camera, SceneError> {
    let layout = layout(scene, assets, spec, aspect)?;
    let fixed = Obstacles::new(scene, assets).and_then(|o| o.fix(&layout, &GENTLE));
    Ok(fixed.unwrap_or_else(|| layout.place(0.0, 0.0, 1.0)))
}

/// Camera nudges tried in order, each dollied in as far as needed:
/// (yaw°, elevation°).
const GENTLE: Nudges = Nudges {
    turns: &[
        (0.0, 0.0),
        (0.0, 8.0),
        (8.0, 0.0),
        (-8.0, 0.0),
        (0.0, 15.0),
        (12.0, 8.0),
        (-12.0, 8.0),
        (0.0, -8.0),
    ],
    closest: 0.4,
    clear_of_subject: false,
};

/// The Explorer may move further from the asked-for setup.
const EXPLORE: Nudges = Nudges {
    turns: &[
        (0.0, 0.0),
        (0.0, 10.0),
        (12.0, 0.0),
        (-12.0, 0.0),
        (0.0, 20.0),
        (20.0, 10.0),
        (-20.0, 10.0),
        (25.0, 0.0),
        (-25.0, 0.0),
        (0.0, -10.0),
        (0.0, 30.0),
        (35.0, 15.0),
        (-35.0, 15.0),
    ],
    closest: 0.25,
    clear_of_subject: true,
};

struct Nudges {
    turns: &'static [(f32, f32)],
    /// The nearest the camera may dolly in, as a fraction of the framed
    /// distance (the lens widens to keep the framing).
    closest: f32,
    /// Keep the camera out of the subject's own box too (on a wide lens a
    /// close-up legitimately sits within a seated figure's box).
    clear_of_subject: bool,
}

/// How far past an obstruction a dollied camera stops (metres).
const PUSH_MARGIN: f32 = 0.2;

/// Where a framed camera goes, before obstacle checks.
struct Layout {
    /// The scene camera with the spec's lens and projection.
    cam: Camera,
    center: Vec3,
    /// Horizontal unit direction from the subject toward the camera.
    horiz: Vec3,
    /// Elevation of the camera above `center`, radians.
    elev: f32,
    dist: f32,
    roll: f32,
    /// Points that must be visible (the subject's head, or the focus).
    required: Vec<Vec3>,
    /// Points that should be visible when possible (the chest).
    wanted: Vec<Vec3>,
    subject: ObjectId,
    /// Objects allowed between the camera and the subject (the subject
    /// itself, the shoulder of an over-the-shoulder shot).
    ignore: Vec<ObjectId>,
}

impl Layout {
    fn place(&self, yaw_deg: f32, elev_deg: f32, dist_factor: f32) -> Camera {
        let mut cam = self.cam;
        let horiz = if yaw_deg == 0.0 {
            self.horiz
        } else {
            Quat::from_rotation_y(yaw_deg.to_radians()) * self.horiz
        };
        let e = if elev_deg == 0.0 {
            self.elev
        } else {
            (self.elev + elev_deg.to_radians()).clamp(-80f32.to_radians(), 88f32.to_radians())
        };
        let dir = horiz * e.cos() + Vec3::Y * e.sin();
        let dist = (self.dist * dist_factor).max(cam.near * 4.0);
        if dist_factor != 1.0 {
            // Keep the framing: the lens widens as the camera moves in.
            cam.focal_length_mm = (cam.focal_length_mm * dist / self.dist)
                .clamp(crate::camera::FOCAL_RANGE.0, crate::camera::FOCAL_RANGE.1);
        }
        cam.position = self.center + dir * dist;
        if cam.position.y < 0.08 {
            cam.position.y = 0.08;
        }
        cam.look_at(self.center);
        cam.roll = self.roll;
        cam
    }
}

/// The set's surfaces, for keeping cameras out of objects.
struct Obstacles {
    prepared: crate::prepare::PreparedScene,
    bounds: Vec<(ObjectId, Aabb)>,
}

/// How far a camera keeps from an object's bounds (metres).
const CLEARANCE: f32 = 0.06;

impl Obstacles {
    fn new(scene: &Scene, assets: &AssetLibrary) -> Option<Obstacles> {
        let prepared = crate::prepare::prepare(scene, assets).ok()?;
        let mut bounds: Vec<(ObjectId, Aabb)> = Vec::new();
        for m in &prepared.meshes {
            match bounds.iter_mut().find(|(id, _)| *id == m.id) {
                Some((_, b)) => *b = b.union(&m.bounds),
                None => bounds.push((m.id, m.bounds)),
            }
        }
        Some(Obstacles { prepared, bounds })
    }

    /// The first nudge that keeps the camera out of objects with the
    /// required points in view, preferring one that also sees the wanted
    /// points; `None` when none works.
    fn fix(&self, layout: &Layout, nudges: &Nudges) -> Option<Camera> {
        let mut fallback = None;
        for &(yaw, elev) in nudges.turns {
            match self.dolly(layout, yaw, elev, nudges) {
                Some((cam, true)) => return Some(cam),
                Some((cam, false)) => {
                    fallback.get_or_insert(cam);
                }
                None => {}
            }
        }
        fallback
    }

    /// Places the camera at a nudge and pushes it along its view line past
    /// whatever it is inside of or that hides a required point.
    fn dolly(
        &self,
        layout: &Layout,
        yaw: f32,
        elev: f32,
        nudges: &Nudges,
    ) -> Option<(Camera, bool)> {
        let mut factor = 1.0;
        for _ in 0..6 {
            let cam = layout.place(yaw, elev, factor);
            let eye = cam.position;
            if nudges.clear_of_subject
                && self
                    .bounds
                    .iter()
                    .any(|(id, b)| *id == layout.subject && contains(&grow(b), eye))
            {
                // Dollying in only goes deeper into the subject's box.
                return None;
            }
            if let Some(all) = self.check(eye, layout) {
                return Some((cam, all));
            }
            let view = layout.center - eye;
            let len = view.length();
            if len < 1e-3 {
                return None;
            }
            let d = view / len;
            let mut reach = 0.0f32;
            for (_, b) in self.containing(eye, layout) {
                reach = reach.max(exit_distance(&grow(b), eye, d));
            }
            for p in &layout.required {
                let seg = (*p - eye).length();
                if let Some(t) = self.last_hit(eye, *p, &layout.ignore) {
                    reach = reach.max(t / seg.max(1e-4) * len);
                }
            }
            let next = factor * (len - reach - PUSH_MARGIN) / len;
            if reach <= 0.0 || next < nudges.closest {
                return None;
            }
            factor = next;
        }
        None
    }

    /// `None` when the camera is inside an object or a required point is
    /// hidden; else whether every wanted point is in view too.
    fn check(&self, eye: Vec3, layout: &Layout) -> Option<bool> {
        if self.inside(eye, layout) {
            return None;
        }
        if layout
            .required
            .iter()
            .any(|p| self.blocked(eye, *p, &layout.ignore))
        {
            return None;
        }
        Some(
            !layout
                .wanted
                .iter()
                .any(|p| self.blocked(eye, *p, &layout.ignore)),
        )
    }

    fn inside(&self, eye: Vec3, layout: &Layout) -> bool {
        self.containing(eye, layout).next().is_some()
    }

    /// The objects whose (grown) bounds hold `eye`.
    fn containing<'a>(
        &'a self,
        eye: Vec3,
        layout: &'a Layout,
    ) -> impl Iterator<Item = &'a (ObjectId, Aabb)> + 'a {
        self.bounds.iter().filter(move |(id, b)| {
            // The subject's own box (a close-up on a wide lens sits inside
            // a seated figure's box) and objects around the subject (a
            // room) only count through the line-of-sight test.
            !layout.ignore.contains(id)
                && !layout.required.iter().any(|p| contains(b, *p))
                && contains(&grow(b), eye)
        })
    }

    /// Whether a surface (other than `ignore`'s) lies between `from` and `to`.
    fn blocked(&self, from: Vec3, to: Vec3, ignore: &[ObjectId]) -> bool {
        self.hits(from, to, ignore).next().is_some()
    }

    /// The distance from `from` to the furthest surface before `to`.
    fn last_hit(&self, from: Vec3, to: Vec3, ignore: &[ObjectId]) -> Option<f32> {
        self.hits(from, to, ignore).reduce(f32::max)
    }

    fn hits<'a>(
        &'a self,
        from: Vec3,
        to: Vec3,
        ignore: &'a [ObjectId],
    ) -> impl Iterator<Item = f32> + 'a {
        let d = to - from;
        let len = d.length();
        let d = if len < 1e-4 { Vec3::ZERO } else { d / len };
        let end = len - 0.01;
        self.prepared
            .meshes
            .iter()
            .filter(move |m| {
                len >= 1e-4
                    && !ignore.contains(&m.id)
                    && m.bounds.ray_hit(from, d).is_some_and(|t| t <= end)
            })
            .flat_map(move |m| {
                m.mesh.indices.iter().filter_map(move |t| {
                    let [a, b, c] = t.map(|i| m.mesh.positions[i as usize]);
                    crate::interop::ray_triangle(from, d, a, b, c).filter(|t| *t < end)
                })
            })
    }
}

fn grow(b: &Aabb) -> Aabb {
    Aabb {
        min: b.min - Vec3::splat(CLEARANCE),
        max: b.max + Vec3::splat(CLEARANCE),
    }
}

/// Where a ray starting inside `b` leaves it.
fn exit_distance(b: &Aabb, origin: Vec3, d: Vec3) -> f32 {
    let inv = d.recip();
    let t0 = (b.min - origin) * inv;
    let t1 = (b.max - origin) * inv;
    let far = t0.max(t1);
    far.x.min(far.y).min(far.z).max(0.0)
}

fn contains(b: &Aabb, p: Vec3) -> bool {
    p.cmpge(b.min).all() && p.cmple(b.max).all()
}

fn layout(
    scene: &Scene,
    assets: &AssetLibrary,
    spec: &ShotSpec,
    aspect: f32,
) -> Result<Layout, SceneError> {
    let aspect = if aspect.is_finite() && aspect > 0.05 {
        aspect
    } else {
        16.0 / 9.0
    };
    let subject = scene
        .object(spec.subject)
        .ok_or_else(|| SceneError::NotFound(format!("object {}", spec.subject.0)))?;
    let mut cam = scene.camera;
    cam.projection = crate::camera::Projection::Perspective;
    cam.roll = 0.0;
    if let Some(f) = spec.focal_length_mm {
        cam.focal_length_mm = f.clamp(crate::camera::FOCAL_RANGE.0, crate::camera::FOCAL_RANGE.1);
    }
    let secondary = match spec.secondary {
        Some(id) if spec.angle.needs_secondary() => scene.object(id).and_then(pose_character),
        _ => None,
    };
    let posed = pose_character(subject);
    let mut t = match &posed {
        Some(p) => character_target(p, spec),
        None => prop_target(scene, assets, spec.subject, spec.size),
    };
    let t_focus = t.center;
    if let (CameraAngle::TwoShot, Some(other)) = (spec.angle, &secondary) {
        let o = character_target(
            other,
            &ShotSpec {
                focus: None,
                ..*spec
            },
        );
        let a = t.center;
        let b = o.center;
        t.center = (a + b) * 0.5;
        t.frame_height = t.frame_height.max(o.frame_height) + (a.y - b.y).abs();
        let side_dir = Vec3::new(b.x - a.x, 0.0, b.z - a.z);
        t.frame_width = side_dir.length() + t.frame_height * 0.8;
        // Look at the pair from the side so both read.
        let perp = Vec3::new(-side_dir.z, 0.0, side_dir.x)
            .try_normalize()
            .unwrap_or(t.facing);
        t.facing = if perp.dot(t.facing + o.facing) >= 0.0 {
            perp
        } else {
            -perp
        };
    }

    let az = spec.side.azimuth().to_radians();
    if spec.group && posed.is_some() && spec.focus.is_none() && !spec.angle.needs_secondary() {
        let others: Vec<Target> = scene
            .objects
            .iter()
            .filter(|o| o.visible && o.id != spec.subject)
            .filter_map(pose_character)
            .map(|p| {
                character_target(
                    &p,
                    &ShotSpec {
                        focus: None,
                        ..*spec
                    },
                )
            })
            .collect();
        if !others.is_empty() {
            let horiz = Quat::from_rotation_y(az) * t.facing;
            let right = Vec3::Y.cross(horiz).normalize_or_zero();
            let all: Vec<&Target> = std::iter::once(&t).chain(others.iter()).collect();
            let (mut x0, mut x1, mut y0, mut y1) = (
                f32::INFINITY,
                f32::NEG_INFINITY,
                f32::INFINITY,
                f32::NEG_INFINITY,
            );
            let mut sum = Vec3::ZERO;
            for a in &all {
                let x = a.center.dot(right);
                x0 = x0.min(x);
                x1 = x1.max(x);
                y0 = y0.min(a.center.y - a.frame_height * 0.5);
                y1 = y1.max(a.center.y + a.frame_height * 0.5);
                sum += a.center;
            }
            let mut c = sum / all.len() as f32;
            c += right * ((x0 + x1) * 0.5 - c.dot(right));
            c.y = (y0 + y1) * 0.5;
            let body = all.iter().map(|a| a.frame_height).fold(0.0f32, f32::max) * 0.45;
            t.center = c;
            t.frame_height = y1 - y0;
            t.frame_width = (x1 - x0) + body;
        }
    }

    let vfov = cam.vertical_fov_deg(aspect).to_radians();
    let hfov = cam.horizontal_fov_deg().to_radians();
    let dist_v = (t.frame_height * 0.5) / (vfov * 0.5).tan();
    let dist_h = (t.frame_width * 0.5) / (hfov * 0.5).tan();
    let dist = dist_v.max(dist_h).max(cam.near * 4.0);

    let (elev, roll) = spec.angle.elevation_roll();
    let horiz = Quat::from_rotation_y(az) * t.facing;
    let (required, wanted) = match &posed {
        Some(_) if spec.focus.is_some() => (vec![t_focus], Vec::new()),
        Some(p) => {
            let head = (p.joint_world(Bone::Head) + p.crown_world()) * 0.5;
            let chest = p.joint_world(Bone::Chest);
            let wanted = if spec.size >= ShotSize::MediumCloseUp {
                vec![chest]
            } else {
                Vec::new()
            };
            (vec![head], wanted)
        }
        None => (vec![t_focus], Vec::new()),
    };
    let mut layout = Layout {
        cam,
        center: t.center,
        horiz,
        elev: elev.to_radians(),
        dist,
        roll,
        required,
        wanted,
        subject: spec.subject,
        ignore: vec![spec.subject],
    };
    if let (CameraAngle::OverTheShoulder, Some(other)) = (spec.angle, &secondary) {
        // Camera behind the secondary character's shoulder (the one on the
        // side chosen by `side`: left/front-left use its right shoulder).
        let shoulder_bone = if spec.side.azimuth() >= 0.0 {
            Bone::UpperArmR
        } else {
            Bone::UpperArmL
        };
        let shoulder = other.joint_world(shoulder_bone);
        let out = (shoulder - other.joint_world(Bone::Neck)) * Vec3::new(1.0, 0.0, 1.0);
        let anchor = shoulder + out.normalize_or_zero() * 0.12 + Vec3::Y * 0.05;
        let to = anchor - t.center;
        let dir = to.normalize_or_zero();
        layout.dist = dist.max(to.length() + 0.45);
        layout.horiz = Vec3::new(dir.x, 0.0, dir.z)
            .try_normalize()
            .unwrap_or(horiz);
        layout.elev = dir.y.clamp(-1.0, 1.0).asin();
        layout.roll = 0.0;
        layout.ignore.push(other.id);
    }
    Ok(layout)
}

fn character_target(p: &PosedCharacter, spec: &ShotSpec) -> Target {
    let hh = p.rig.head_height * p.world.transform_vector3(Vec3::Y).length();
    let facing = p.body_facing_world();
    if let Some(bone) = spec.focus {
        let (a, b) = focus_extent(p, bone);
        let len = (b - a).length().max(hh * 0.3);
        let mult = match spec.size {
            ShotSize::ExtremeCloseUp => 1.6,
            ShotSize::CloseUp => 3.0,
            ShotSize::MediumCloseUp => 5.0,
            ShotSize::Medium => 7.0,
            ShotSize::MediumWide => 10.0,
            ShotSize::Wide => 14.0,
            ShotSize::ExtremeWide => 40.0,
        };
        return Target {
            center: (a + b) * 0.5,
            frame_height: len * mult,
            frame_width: 0.0,
            facing,
        };
    }
    let crown = p.crown_world();
    let up = p
        .world
        .transform_vector3(p.bones[Bone::Head.index()].rotation * Vec3::Y)
        .normalize_or_zero();
    let top = crown + up * hh * 0.02;
    let bottom = match spec.size {
        ShotSize::ExtremeCloseUp => {
            let eyes = p.eyes_world();
            return Target {
                center: eyes,
                frame_height: hh * 0.62,
                frame_width: 0.0,
                facing,
            };
        }
        ShotSize::CloseUp => p.joint_world(Bone::Head) - up * hh * 0.35,
        ShotSize::MediumCloseUp => (p.joint_world(Bone::Chest) + p.joint_world(Bone::Neck)) * 0.5,
        ShotSize::Medium => p.joint_world(Bone::Spine),
        ShotSize::MediumWide => {
            (p.joint_world(Bone::LowerLegL) + p.joint_world(Bone::LowerLegR)) * 0.5
        }
        ShotSize::Wide | ShotSize::ExtremeWide => {
            let mut low = p.tip_world(Bone::FootL);
            for b in [
                Bone::FootR,
                Bone::LowerLegL,
                Bone::LowerLegR,
                Bone::Hips,
                Bone::HandL,
                Bone::HandR,
            ] {
                let q = p.joint_world(b);
                if q.y < low.y {
                    low = q;
                }
            }
            let fr = p.tip_world(Bone::FootR);
            if fr.y < low.y {
                low = fr;
            }
            Vec3::new(low.x, low.y.min(p.world.w_axis.y), low.z)
        }
    };
    let span = (top - bottom).length().max(hh);
    // The subject spans 1/(1+HEADROOM) of the frame: the cut sits on the
    // bottom edge, headroom above.
    let frame = span * (1.0 + HEADROOM);
    let mut center = bottom + (top - bottom) * ((1.0 + HEADROOM) * 0.5);
    let mut frame_height = frame;
    if spec.size == ShotSize::ExtremeWide {
        frame_height = span * 6.0;
        center = (top + bottom) * 0.5;
    }
    Target {
        center,
        frame_height,
        frame_width: 0.0,
        facing,
    }
}

/// Headroom above the crown as a fraction of the framed span.
pub const HEADROOM: f32 = 0.12;

fn focus_extent(p: &PosedCharacter, bone: Bone) -> (Vec3, Vec3) {
    let tip = match bone {
        Bone::HandL => p.tip_world(Bone::Fingers2L),
        Bone::HandR => p.tip_world(Bone::Fingers2R),
        Bone::FootL | Bone::FootR => p.tip_world(bone),
        Bone::Head => p.crown_world(),
        _ => p.tip_world(bone),
    };
    (p.joint_world(bone), tip)
}

fn prop_target(scene: &Scene, assets: &AssetLibrary, id: ObjectId, size: ShotSize) -> Target {
    let b = object_bounds(scene, assets, id);
    let (c, r) = if b.is_empty() {
        (Vec3::Y, 0.5)
    } else {
        (b.center(), (b.size().length() * 0.5).max(0.05))
    };
    let facing = scene
        .object(id)
        .map(|o| {
            let f = o.transform.forward();
            Vec3::new(f.x, 0.0, f.z).try_normalize().unwrap_or(Vec3::Z)
        })
        .unwrap_or(Vec3::Z);
    let fh = r * size.prop_frame() * 2.0;
    Target {
        center: c,
        frame_height: fh,
        frame_width: fh,
        facing,
    }
}

/// World bounds of an object without preparing the whole scene.
pub fn object_bounds(scene: &Scene, assets: &AssetLibrary, id: ObjectId) -> Aabb {
    let Some(o) = scene.object(id) else {
        return Aabb::EMPTY;
    };
    let m = o.transform.matrix();
    match &o.kind {
        ObjectKind::Prop(Prop::Builtin(p)) => p.mesh().bounds().transformed(&m),
        ObjectKind::Prop(Prop::Model(r)) => assets
            .get(&r.asset)
            .map(|a| a.bounds())
            .unwrap_or(Aabb {
                min: Vec3::new(-0.25, 0.0, -0.25),
                max: Vec3::new(0.25, 0.5, 0.25),
            })
            .transformed(&m),
        ObjectKind::Character(_) => match pose_character(o) {
            Some(p) => {
                let pts: Vec<Vec3> = Bone::ALL
                    .iter()
                    .flat_map(|b| [p.joint_world(*b), p.tip_world(*b)])
                    .collect();
                let mut b = Aabb::from_points(pts.iter());
                let pad = p.rig.head_height * 0.4;
                b.min -= Vec3::splat(pad);
                b.max += Vec3::splat(pad);
                b.min.y = b.min.y.max(m.w_axis.y);
                b
            }
            None => Aabb::EMPTY,
        },
        ObjectKind::Light(_) => Aabb {
            min: o.transform.position - Vec3::splat(0.1),
            max: o.transform.position + Vec3::splat(0.1),
        },
    }
}

/// A proposed camera setup from the Shot Explorer.
#[derive(Debug, Clone, PartialEq)]
pub struct ShotProposal {
    pub name: String,
    pub spec: ShotSpec,
    pub camera: Camera,
}

/// Proposes up to `count` varied camera setups (size × angle × side × lens)
/// on `subject`. The sequence is deterministic; with another character in
/// the scene, over-the-shoulder and two-shots are included.
pub fn explore_shots(
    scene: &Scene,
    assets: &AssetLibrary,
    subject: ObjectId,
    count: usize,
    aspect: f32,
) -> Result<Vec<ShotProposal>, SceneError> {
    let subj = scene
        .object(subject)
        .ok_or_else(|| SceneError::NotFound(format!("object {}", subject.0)))?;
    let is_char = subj.character().is_some();
    let secondary = if is_char {
        let here = subj.transform.position;
        scene
            .objects
            .iter()
            .filter(|o| o.id != subject && o.character().is_some() && o.visible)
            .min_by(|a, b| {
                let da = (a.transform.position - here).length_squared();
                let db = (b.transform.position - here).length_squared();
                da.total_cmp(&db).then(a.id.cmp(&b.id))
            })
            .map(|o| o.id)
    } else {
        None
    };
    let sizes = [
        ShotSize::Medium,
        ShotSize::CloseUp,
        ShotSize::Wide,
        ShotSize::MediumCloseUp,
        ShotSize::MediumWide,
        ShotSize::ExtremeCloseUp,
        ShotSize::ExtremeWide,
    ];
    let mut angles = vec![
        CameraAngle::EyeLevel,
        CameraAngle::Low,
        CameraAngle::High,
        CameraAngle::Dutch,
        CameraAngle::BirdsEye,
        CameraAngle::WormsEye,
    ];
    if secondary.is_some() {
        angles.insert(1, CameraAngle::OverTheShoulder);
        angles.insert(4, CameraAngle::TwoShot);
    }
    let sides = [
        ShotSide::FrontLeft,
        ShotSide::Front,
        ShotSide::FrontRight,
        ShotSide::Left,
        ShotSide::Right,
        ShotSide::BackLeft,
        ShotSide::BackRight,
    ];
    let obstacles = Obstacles::new(scene, assets);
    let mut seen = std::collections::HashSet::new();
    let mut out = Vec::new();
    let total = sizes.len() * angles.len() * sides.len();
    let mut i = 0usize;
    while out.len() < count.min(total) && i < total * 4 {
        let size = sizes[i % sizes.len()];
        let angle = angles[(i * 3 + i / sizes.len()) % angles.len()];
        let side = sides[(i * 5 + i / angles.len()) % sides.len()];
        i += 1;
        // Extreme close-ups and bird's-eye views read badly from behind.
        if matches!(side, ShotSide::BackLeft | ShotSide::BackRight)
            && (size <= ShotSize::CloseUp || angle == CameraAngle::BirdsEye)
        {
            continue;
        }
        if !is_char && size == ShotSize::ExtremeCloseUp {
            continue;
        }
        if !seen.insert((size, angle, side)) {
            continue;
        }
        let spec = ShotSpec {
            size,
            angle,
            side,
            subject,
            secondary: if angle.needs_secondary() {
                secondary
            } else {
                None
            },
            focus: None,
            focal_length_mm: Some(size.suggested_focal_length()),
            group: false,
        };
        let layout = layout(scene, assets, &spec, aspect)?;
        let camera = match &obstacles {
            Some(o) => match o.fix(&layout, &EXPLORE) {
                Some(camera) => camera,
                // Every nearby setup is inside something or hidden.
                None => continue,
            },
            None => layout.place(0.0, 0.0, 1.0),
        };
        out.push(ShotProposal {
            name: spec.name(),
            spec,
            camera,
        });
    }
    Ok(out)
}
