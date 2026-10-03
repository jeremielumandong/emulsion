//! Posing (SG3, SG8, C10): pose data, hand shapes, face presets, two-bone IK,
//! head look-at and the built-in pose library.

use std::collections::BTreeMap;

use glam::{Quat, Vec2, Vec3};
use serde::{Deserialize, Serialize};

use crate::mannequin::MannequinParams;
use crate::skeleton::{BONE_COUNT, Bone, JointRotation, Rig, forward_kinematics};

/// A hand shape applied to the finger bones.
#[derive(Debug, Clone, Copy, Default, PartialEq, Eq, Hash, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum HandShape {
    Open,
    #[default]
    Relaxed,
    Fist,
    Point,
    Grip,
}

impl HandShape {
    pub const ALL: [HandShape; 5] = [
        HandShape::Open,
        HandShape::Relaxed,
        HandShape::Fist,
        HandShape::Point,
        HandShape::Grip,
    ];

    pub fn name(self) -> &'static str {
        match self {
            HandShape::Open => "open",
            HandShape::Relaxed => "relaxed",
            HandShape::Fist => "fist",
            HandShape::Point => "point",
            HandShape::Grip => "grip",
        }
    }

    /// Finger rotations for the **left** hand (mirror them for the right):
    /// `[thumb1, thumb2, index1, index2, fingers1, fingers2]`.
    pub fn left_fingers(self) -> [JointRotation; 6] {
        let j = JointRotation::new;
        match self {
            HandShape::Open => [
                j(0.0, 0.0, 8.0),
                j(0.0, 0.0, 0.0),
                j(0.0, 0.0, 0.0),
                j(0.0, 0.0, 0.0),
                j(0.0, 0.0, 0.0),
                j(0.0, 0.0, 0.0),
            ],
            HandShape::Relaxed => [
                j(0.0, 0.0, -10.0),
                j(0.0, 0.0, -10.0),
                j(0.0, 0.0, -15.0),
                j(0.0, 0.0, -20.0),
                j(0.0, 0.0, -22.0),
                j(0.0, 0.0, -25.0),
            ],
            HandShape::Fist => [
                j(-20.0, 0.0, -45.0),
                j(0.0, 0.0, -40.0),
                j(0.0, 0.0, -85.0),
                j(0.0, 0.0, -90.0),
                j(0.0, 0.0, -85.0),
                j(0.0, 0.0, -90.0),
            ],
            HandShape::Point => [
                j(-20.0, 0.0, -45.0),
                j(0.0, 0.0, -40.0),
                j(0.0, 0.0, 0.0),
                j(0.0, 0.0, 0.0),
                j(0.0, 0.0, -85.0),
                j(0.0, 0.0, -90.0),
            ],
            HandShape::Grip => [
                j(-10.0, 0.0, -35.0),
                j(0.0, 0.0, -20.0),
                j(0.0, 0.0, -50.0),
                j(0.0, 0.0, -55.0),
                j(0.0, 0.0, -55.0),
                j(0.0, 0.0, -60.0),
            ],
        }
    }
}

const LEFT_FINGERS: [Bone; 6] = [
    Bone::Thumb1L,
    Bone::Thumb2L,
    Bone::Index1L,
    Bone::Index2L,
    Bone::Fingers1L,
    Bone::Fingers2L,
];

/// A facial expression drawn as strokes on the mannequin head (SG8).
#[derive(Debug, Clone, Copy, Default, PartialEq, Eq, Hash, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum FacePreset {
    #[default]
    Neutral,
    Happy,
    Sad,
    Angry,
    Surprised,
    Scared,
    /// No features drawn.
    Blank,
}

impl FacePreset {
    pub const ALL: [FacePreset; 7] = [
        FacePreset::Neutral,
        FacePreset::Happy,
        FacePreset::Sad,
        FacePreset::Angry,
        FacePreset::Surprised,
        FacePreset::Scared,
        FacePreset::Blank,
    ];

    pub fn name(self) -> &'static str {
        match self {
            FacePreset::Neutral => "neutral",
            FacePreset::Happy => "happy",
            FacePreset::Sad => "sad",
            FacePreset::Angry => "angry",
            FacePreset::Surprised => "surprised",
            FacePreset::Scared => "scared",
            FacePreset::Blank => "blank",
        }
    }

    /// Strokes in face coordinates: `u` across (+ = the character's left),
    /// `v` up, both in -1..1 over the front of the head ellipsoid.
    pub fn strokes(self) -> Vec<Vec<Vec2>> {
        let mut s: Vec<Vec<Vec2>> = Vec::new();
        if self == FacePreset::Blank {
            return s;
        }
        let eye_r = match self {
            FacePreset::Surprised | FacePreset::Scared => 0.09,
            _ => 0.055,
        };
        for side in [-1.0f32, 1.0] {
            s.push(circle(Vec2::new(0.34 * side, 0.05), eye_r, 8));
            // Brows: (inner height, outer height).
            let (inner, outer) = match self {
                FacePreset::Neutral => (0.26, 0.27),
                FacePreset::Happy => (0.3, 0.28),
                FacePreset::Sad => (0.33, 0.22),
                FacePreset::Angry => (0.17, 0.3),
                FacePreset::Surprised => (0.38, 0.37),
                FacePreset::Scared => (0.38, 0.3),
                FacePreset::Blank => unreachable!(),
            };
            s.push(vec![
                Vec2::new(0.16 * side, inner),
                Vec2::new(0.5 * side, outer),
            ]);
        }
        let mouth_y = -0.45;
        let mouth: Vec<Vec2> = match self {
            FacePreset::Neutral => vec![Vec2::new(-0.18, mouth_y), Vec2::new(0.18, mouth_y)],
            FacePreset::Happy => mouth_arc(mouth_y + 0.08, 0.26, -0.16),
            FacePreset::Sad => mouth_arc(mouth_y - 0.06, 0.22, 0.12),
            FacePreset::Angry => vec![
                Vec2::new(-0.2, mouth_y - 0.03),
                Vec2::new(-0.07, mouth_y),
                Vec2::new(0.07, mouth_y),
                Vec2::new(0.2, mouth_y - 0.03),
            ],
            FacePreset::Surprised => circle(Vec2::new(0.0, mouth_y - 0.02), 0.1, 10),
            FacePreset::Scared => vec![
                Vec2::new(-0.2, mouth_y),
                Vec2::new(-0.1, mouth_y + 0.04),
                Vec2::new(0.0, mouth_y),
                Vec2::new(0.1, mouth_y + 0.04),
                Vec2::new(0.2, mouth_y),
            ],
            FacePreset::Blank => unreachable!(),
        };
        s.push(mouth);
        s
    }
}

fn circle(c: Vec2, r: f32, n: usize) -> Vec<Vec2> {
    (0..=n)
        .map(|i| {
            let a = std::f32::consts::TAU * i as f32 / n as f32;
            c + Vec2::new(a.cos(), a.sin()) * r
        })
        .collect()
}

/// A mouth arc: centre height, half width, sag (negative = smile).
fn mouth_arc(y: f32, half: f32, sag: f32) -> Vec<Vec2> {
    (0..=6)
        .map(|i| {
            let t = i as f32 / 6.0 * 2.0 - 1.0;
            Vec2::new(t * half, y + sag * (1.0 - t * t))
        })
        .collect()
}

/// A body pose: joint rotations (bones not listed are at rest), a hips
/// offset and hand shapes. Poses are plain data and serialize to JSON.
#[derive(Debug, Clone, Default, PartialEq, Serialize, Deserialize)]
#[serde(default)]
pub struct Pose {
    pub name: String,
    pub joints: BTreeMap<Bone, JointRotation>,
    /// Hips translation from rest in units of figure height (so the same pose
    /// fits a child and an adult). Ground lock usually overrides `y`.
    pub hips_offset: Vec3,
    pub left_hand: HandShape,
    pub right_hand: HandShape,
}

impl Pose {
    /// The rest pose (arms down, standing straight).
    pub fn rest() -> Pose {
        Pose {
            name: "rest".into(),
            ..Default::default()
        }
    }

    /// The rotation of `bone` (explicit joint, else hand shape, else zero).
    pub fn rotation(&self, bone: Bone) -> JointRotation {
        if let Some(r) = self.joints.get(&bone) {
            return *r;
        }
        if bone.is_finger() {
            let left = if bone.side() == crate::skeleton::Side::Left {
                bone
            } else {
                bone.mirror()
            };
            let shape = if left == bone {
                self.left_hand
            } else {
                self.right_hand
            };
            let i = LEFT_FINGERS.iter().position(|b| *b == left).unwrap_or(0);
            let r = shape.left_fingers()[i];
            return if left == bone { r } else { r.mirrored() };
        }
        JointRotation::ZERO
    }

    /// Sets a joint, clamped to its limits.
    pub fn set(&mut self, bone: Bone, r: JointRotation) {
        let r = if r.is_finite() {
            r
        } else {
            JointRotation::ZERO
        };
        self.joints.insert(bone, bone.limits().clamp(r));
    }

    /// Clamps every joint to its limits.
    pub fn clamp_to_limits(&mut self) {
        for (b, r) in self.joints.iter_mut() {
            *r = b.limits().clamp(if r.is_finite() {
                *r
            } else {
                JointRotation::ZERO
            });
        }
        if !self.hips_offset.is_finite() {
            self.hips_offset = Vec3::ZERO;
        }
    }

    /// True when every joint is within its limits (± `tolerance` degrees).
    pub fn within_limits(&self, tolerance: f32) -> bool {
        Bone::ALL
            .iter()
            .all(|b| b.limits().contains(self.rotation(*b), tolerance))
    }

    /// Local rotation of every bone as quaternions.
    pub fn local_rotations(&self) -> [Quat; BONE_COUNT] {
        let mut q = [Quat::IDENTITY; BONE_COUNT];
        for &b in Bone::ALL {
            q[b.index()] = self.rotation(b).to_quat();
        }
        q
    }

    /// Mirror image: left and right swap.
    pub fn mirrored(&self) -> Pose {
        let mut joints = BTreeMap::new();
        for (b, r) in &self.joints {
            joints.insert(b.mirror(), r.mirrored());
        }
        Pose {
            name: if self.name.is_empty() {
                String::new()
            } else {
                format!("{} (mirrored)", self.name)
            },
            joints,
            hips_offset: self.hips_offset * Vec3::new(-1.0, 1.0, 1.0),
            left_hand: self.right_hand,
            right_hand: self.left_hand,
        }
    }

    /// Blends two poses: `t = 0` gives `a`, `t = 1` gives `b`. Joints are
    /// slerped (fingers included); hand shapes switch at the midpoint.
    pub fn blend(a: &Pose, b: &Pose, t: f32) -> Pose {
        let t = if t.is_finite() {
            t.clamp(0.0, 1.0)
        } else {
            0.0
        };
        let mut joints = BTreeMap::new();
        for &bone in Bone::ALL {
            let ra = a.rotation(bone);
            let rb = b.rotation(bone);
            if ra == JointRotation::ZERO && rb == JointRotation::ZERO {
                continue;
            }
            let q = ra.to_quat().slerp(rb.to_quat(), t);
            joints.insert(bone, bone.limits().clamp(JointRotation::from_quat(q)));
        }
        Pose {
            name: format!("{} → {}", a.name, b.name),
            joints,
            hips_offset: a.hips_offset.lerp(b.hips_offset, t),
            left_hand: if t < 0.5 { a.left_hand } else { b.left_hand },
            right_hand: if t < 0.5 { a.right_hand } else { b.right_hand },
        }
    }
}

/// A limb that two-bone IK can drive.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, PartialOrd, Ord, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum Limb {
    LeftArm,
    RightArm,
    LeftLeg,
    RightLeg,
}

impl Limb {
    pub const ALL: [Limb; 4] = [Limb::LeftArm, Limb::RightArm, Limb::LeftLeg, Limb::RightLeg];

    /// (upper, lower, end) bones. The IK target is the end bone's joint
    /// (wrist or ankle).
    pub fn bones(self) -> (Bone, Bone, Bone) {
        match self {
            Limb::LeftArm => (Bone::UpperArmL, Bone::LowerArmL, Bone::HandL),
            Limb::RightArm => (Bone::UpperArmR, Bone::LowerArmR, Bone::HandR),
            Limb::LeftLeg => (Bone::UpperLegL, Bone::LowerLegL, Bone::FootL),
            Limb::RightLeg => (Bone::UpperLegR, Bone::LowerLegR, Bone::FootR),
        }
    }

    pub fn is_leg(self) -> bool {
        matches!(self, Limb::LeftLeg | Limb::RightLeg)
    }

    /// Default bend direction (character space) for the middle joint.
    pub fn default_pole(self) -> Vec3 {
        match self {
            Limb::LeftArm => Vec3::new(0.4, -0.3, -1.0),
            Limb::RightArm => Vec3::new(-0.4, -0.3, -1.0),
            Limb::LeftLeg => Vec3::new(0.1, 0.0, 1.0),
            Limb::RightLeg => Vec3::new(-0.1, 0.0, 1.0),
        }
    }

    /// Sign of the hinge flex about local X (elbows bend with negative x,
    /// knees with positive).
    fn flex_sign(self) -> f32 {
        if self.is_leg() { 1.0 } else { -1.0 }
    }
}

/// Outcome of an IK solve.
#[derive(Debug, Clone, Copy, PartialEq)]
pub struct IkResult {
    /// Distance from the end joint to the target after solving (metres).
    pub error: f32,
    /// False when the target is out of reach or limits stopped the limb.
    pub reached: bool,
}

/// Two-bone IK in character space. Updates `pose` so the limb's end joint
/// (wrist/ankle) reaches `target`, bending toward `pole` (a direction; `None`
/// uses [`Limb::default_pole`]). The middle joint is a true hinge and all
/// results respect joint limits. With `keep_end_orientation`, the hand/foot
/// keeps its previous world orientation (feet stay flat).
pub fn solve_two_bone_ik(
    rig: &Rig,
    pose: &mut Pose,
    limb: Limb,
    target: Vec3,
    pole: Option<Vec3>,
    keep_end_orientation: bool,
) -> IkResult {
    let (upper, lower, end) = limb.bones();
    let hips = pose.hips_offset * rig.height;
    let before = forward_kinematics(rig, &pose.local_rotations(), hips);
    if !target.is_finite() {
        let e = (before[end.index()].head - target).length();
        return IkResult {
            error: e,
            reached: false,
        };
    }
    let end_world_before = before[end.index()].rotation;
    let parent_rot = upper
        .parent()
        .map(|p| before[p.index()].rotation)
        .unwrap_or(Quat::IDENTITY);
    let root = before[upper.index()].head;
    let a = rig.offset(lower);
    let b = rig.offset(end);
    let to_target = target - root;
    let dist = to_target.length();

    // Hinge angle by bisection on |a + Rx(φ)·b| = dist (monotonic from straight).
    let flex = limb.flex_sign();
    let lim = lower.limits();
    let max_flex = if flex > 0.0 { lim.max.x } else { -lim.min.x };
    let reach = |phi: f32| (a + Quat::from_rotation_x((flex * phi).to_radians()) * b).length();
    let (mut lo, mut hi) = (0.0f32, max_flex);
    let mut phi = if dist >= reach(0.0) {
        0.0
    } else if dist <= reach(max_flex) {
        max_flex
    } else {
        for _ in 0..48 {
            let mid = 0.5 * (lo + hi);
            if reach(mid) > dist {
                lo = mid;
            } else {
                hi = mid;
            }
        }
        0.5 * (lo + hi)
    };
    if !phi.is_finite() {
        phi = 0.0;
    }
    let lower_local = Quat::from_rotation_x((flex * phi).to_radians());
    let v_local = a + lower_local * b;

    let dir = to_target.try_normalize().unwrap_or(Vec3::NEG_Y);
    let q1 = rotation_arc(v_local.normalize_or_zero(), dir);
    let pole = pole
        .unwrap_or(limb.default_pole())
        .try_normalize()
        .unwrap_or(limb.default_pole().normalize());
    let elbow = q1 * a;
    let pe = (elbow - dir * elbow.dot(dir)).try_normalize();
    let pp = (pole - dir * pole.dot(dir)).try_normalize();
    // The elbow/knee may sit anywhere on a circle around the target axis.
    // Prefer the pole side, but pick the nearest angle whose shoulder/hip
    // rotation stays within joint limits so clamping cannot pull the limb
    // off target.
    let preferred = match (pe, pp) {
        (Some(pe), Some(pp)) => pe.cross(pp).dot(dir).atan2(pe.dot(pp)),
        _ => 0.0,
    };
    let ul = upper.limits();
    let violation = |angle: f32| {
        let w = Quat::from_axis_angle(dir, angle) * q1;
        let r = JointRotation::from_quat(parent_rot.inverse() * w).as_vec3();
        (ul.min - r).max(Vec3::ZERO).element_sum() + (r - ul.max).max(Vec3::ZERO).element_sum()
    };
    let mut best = (violation(preferred), preferred);
    const STEPS: i32 = 90;
    for i in 1..=STEPS {
        for sign in [1.0f32, -1.0] {
            let delta = sign * std::f32::consts::PI * i as f32 / STEPS as f32;
            let v = violation(preferred + delta);
            if v + 1e-3 < best.0 {
                best = (v, preferred + delta);
            }
        }
    }
    let world_upper = Quat::from_axis_angle(dir, best.1) * q1;
    let upper_local = parent_rot.inverse() * world_upper;
    pose.set(upper, JointRotation::from_quat(upper_local));
    pose.set(lower, JointRotation::from_quat(lower_local));
    if keep_end_orientation {
        let after = forward_kinematics(rig, &pose.local_rotations(), hips);
        let end_local = after[lower.index()].rotation.inverse() * end_world_before;
        pose.set(end, JointRotation::from_quat(end_local));
    }
    let after = forward_kinematics(rig, &pose.local_rotations(), hips);
    let error = (after[end.index()].head - target).length();
    IkResult {
        error,
        reached: error < 0.01 * rig.height.max(0.1),
    }
}

fn rotation_arc(from: Vec3, to: Vec3) -> Quat {
    if from == Vec3::ZERO || to == Vec3::ZERO {
        return Quat::IDENTITY;
    }
    if from.dot(to) < -0.999_999 {
        let axis = from.any_orthonormal_vector();
        return Quat::from_axis_angle(axis, std::f32::consts::PI);
    }
    Quat::from_rotation_arc(from, to)
}

/// Turns the neck and head toward `target` (character space). The neck takes
/// 40% of the turn, the head the rest; both respect their limits.
pub fn apply_look_at(rig: &Rig, pose: &mut Pose, target: Vec3) {
    if !target.is_finite() {
        return;
    }
    // Start from a neutral neck/head so look-at does not accumulate.
    let mut base = pose.clone();
    base.joints.remove(&Bone::Neck);
    base.joints.remove(&Bone::Head);
    let bones = forward_kinematics(rig, &base.local_rotations(), base.hips_offset * rig.height);
    let chest = bones[Bone::Chest.index()].rotation;
    let head = bones[Bone::Head.index()];
    let eye = head.head + head.rotation * Vec3::new(0.0, rig.head_height * 0.55, 0.0);
    let d = chest.inverse() * (target - eye);
    if d.length_squared() < 1e-8 {
        return;
    }
    let yaw = d.x.atan2(d.z).to_degrees();
    let pitch = (-d.y).atan2((d.x * d.x + d.z * d.z).sqrt()).to_degrees();
    let neck_old = pose.rotation(Bone::Neck);
    let head_old = pose.rotation(Bone::Head);
    let nl = Bone::Neck.limits();
    let neck = nl.clamp(JointRotation::new(pitch * 0.4, yaw * 0.4, neck_old.z));
    let head_rot = JointRotation::new(pitch - neck.x, yaw - neck.y, head_old.z);
    pose.set(Bone::Neck, neck);
    pose.set(Bone::Head, head_rot);
}

/// Built-in pose presets (our own data).
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum PosePreset {
    Stand,
    RelaxedStand,
    Walk,
    Run,
    Sit,
    SitOnFloor,
    Kneel,
    Crouch,
    Point,
    Wave,
    Reach,
    ArmsCrossed,
    HandsOnHips,
    FightStance,
    Fall,
    LieDown,
    PhoneCall,
    Carry,
}

impl PosePreset {
    pub const ALL: [PosePreset; 18] = [
        PosePreset::Stand,
        PosePreset::RelaxedStand,
        PosePreset::Walk,
        PosePreset::Run,
        PosePreset::Sit,
        PosePreset::SitOnFloor,
        PosePreset::Kneel,
        PosePreset::Crouch,
        PosePreset::Point,
        PosePreset::Wave,
        PosePreset::Reach,
        PosePreset::ArmsCrossed,
        PosePreset::HandsOnHips,
        PosePreset::FightStance,
        PosePreset::Fall,
        PosePreset::LieDown,
        PosePreset::PhoneCall,
        PosePreset::Carry,
    ];

    /// Stable snake_case name.
    pub fn name(self) -> &'static str {
        match self {
            PosePreset::Stand => "stand",
            PosePreset::RelaxedStand => "relaxed_stand",
            PosePreset::Walk => "walk",
            PosePreset::Run => "run",
            PosePreset::Sit => "sit",
            PosePreset::SitOnFloor => "sit_on_floor",
            PosePreset::Kneel => "kneel",
            PosePreset::Crouch => "crouch",
            PosePreset::Point => "point",
            PosePreset::Wave => "wave",
            PosePreset::Reach => "reach",
            PosePreset::ArmsCrossed => "arms_crossed",
            PosePreset::HandsOnHips => "hands_on_hips",
            PosePreset::FightStance => "fight_stance",
            PosePreset::Fall => "fall",
            PosePreset::LieDown => "lie_down",
            PosePreset::PhoneCall => "phone_call",
            PosePreset::Carry => "carry",
        }
    }

    pub fn from_name(name: &str) -> Option<PosePreset> {
        PosePreset::ALL.iter().copied().find(|p| p.name() == name)
    }

    /// Builds the preset's pose.
    pub fn pose(self) -> Pose {
        let mut b = Builder::new(self.name());
        use Bone::*;
        match self {
            PosePreset::Stand => {
                b.sym(UpperArmL, 0.0, 0.0, 6.0)
                    .sym(LowerArmL, -8.0, 0.0, 0.0);
            }
            PosePreset::RelaxedStand => {
                b.j(Hips, 0.0, 5.0, 3.0)
                    .j(Spine, 0.0, -3.0, -3.0)
                    .j(Chest, 0.0, -4.0, 0.0)
                    .sym(UpperArmL, 3.0, 0.0, 5.0)
                    .sym(LowerArmL, -15.0, 0.0, 0.0)
                    .j(UpperLegL, -8.0, 0.0, 4.0)
                    .j(LowerLegL, 15.0, 0.0, 0.0)
                    .j(Head, 0.0, 6.0, 4.0);
            }
            PosePreset::Walk => {
                b.j(UpperLegL, -25.0, 0.0, 0.0)
                    .j(LowerLegL, 8.0, 0.0, 0.0)
                    .j(FootL, -10.0, 0.0, 0.0)
                    .j(UpperLegR, 18.0, 0.0, 0.0)
                    .j(LowerLegR, 25.0, 0.0, 0.0)
                    .j(FootR, 12.0, 0.0, 0.0)
                    .j(UpperArmL, 18.0, 0.0, 5.0)
                    .j(LowerArmL, -15.0, 0.0, 0.0)
                    .j(UpperArmR, -22.0, 0.0, -5.0)
                    .j(LowerArmR, -30.0, 0.0, 0.0)
                    .j(Chest, 0.0, 5.0, 0.0);
            }
            PosePreset::Run => {
                b.j(Spine, 12.0, 0.0, 0.0)
                    .j(Head, -10.0, 0.0, 0.0)
                    .j(UpperLegL, -65.0, 0.0, 0.0)
                    .j(LowerLegL, 75.0, 0.0, 0.0)
                    .j(FootL, 10.0, 0.0, 0.0)
                    .j(UpperLegR, 25.0, 0.0, 0.0)
                    .j(LowerLegR, 95.0, 0.0, 0.0)
                    .j(FootR, 30.0, 0.0, 0.0)
                    .j(UpperArmL, 45.0, 0.0, 10.0)
                    .j(LowerArmL, -95.0, 0.0, 0.0)
                    .j(UpperArmR, -50.0, 0.0, -10.0)
                    .j(LowerArmR, -95.0, 0.0, 0.0)
                    .hands(HandShape::Fist, HandShape::Fist);
            }
            PosePreset::Sit => {
                b.sym(UpperLegL, -90.0, 0.0, 4.0)
                    .sym(LowerLegL, 90.0, 0.0, 0.0)
                    .sym(UpperArmL, -25.0, 0.0, 8.0)
                    .sym(LowerArmL, -55.0, 0.0, 0.0)
                    .j(Spine, 4.0, 0.0, 0.0)
                    .offset(0.0, -0.23, 0.0);
            }
            PosePreset::SitOnFloor => {
                b.sym(UpperLegL, -82.0, 0.0, 16.0)
                    .sym(LowerLegL, 55.0, 0.0, 0.0)
                    .sym(UpperArmL, 25.0, 0.0, 15.0)
                    .sym(LowerArmL, -5.0, 0.0, 0.0)
                    .j(Spine, -6.0, 0.0, 0.0)
                    .hands(HandShape::Open, HandShape::Open)
                    .offset(0.0, -0.44, 0.0);
            }
            PosePreset::Kneel => {
                b.j(UpperLegL, -90.0, 0.0, 5.0)
                    .j(LowerLegL, 90.0, 0.0, 0.0)
                    .j(UpperLegR, 5.0, 0.0, 0.0)
                    .j(LowerLegR, 90.0, 0.0, 0.0)
                    .j(FootR, 40.0, 0.0, 0.0)
                    .sym(UpperArmL, -20.0, 0.0, 8.0)
                    .sym(LowerArmL, -60.0, 0.0, 0.0)
                    .offset(0.0, -0.25, 0.0);
            }
            PosePreset::Crouch => {
                b.sym(UpperLegL, -110.0, 0.0, 12.0)
                    .sym(LowerLegL, 130.0, 0.0, 0.0)
                    .sym(FootL, -20.0, 0.0, 0.0)
                    .j(Spine, 30.0, 0.0, 0.0)
                    .j(Chest, 10.0, 0.0, 0.0)
                    .j(Head, -25.0, 0.0, 0.0)
                    .sym(UpperArmL, -55.0, 0.0, 10.0)
                    .sym(LowerArmL, -50.0, 0.0, 0.0)
                    .offset(0.0, -0.32, 0.0);
            }
            PosePreset::Point => {
                b.sym(UpperArmL, 0.0, 0.0, 6.0)
                    .sym(LowerArmL, -8.0, 0.0, 0.0)
                    .ik(Limb::RightArm, Vec3::new(-0.13, 0.8, 0.31), None)
                    .j(Head, 0.0, -4.0, 0.0)
                    .hands(HandShape::Relaxed, HandShape::Point);
            }
            PosePreset::Wave => {
                b.sym(UpperArmL, 0.0, 0.0, 6.0)
                    .sym(LowerArmL, -8.0, 0.0, 0.0)
                    .ik(
                        Limb::RightArm,
                        Vec3::new(-0.25, 0.97, 0.05),
                        Some(Vec3::new(-1.0, -0.6, -0.2)),
                    )
                    .j(Head, 0.0, -6.0, 4.0)
                    .hands(HandShape::Relaxed, HandShape::Open);
            }
            PosePreset::Reach => {
                b.j(Spine, 6.0, 0.0, 0.0)
                    .ik(Limb::LeftArm, Vec3::new(0.09, 0.9, 0.29), None)
                    .ik(Limb::RightArm, Vec3::new(-0.09, 0.9, 0.29), None)
                    .j(Head, -12.0, 0.0, 0.0)
                    .hands(HandShape::Open, HandShape::Open);
            }
            PosePreset::ArmsCrossed => {
                b.ik(
                    Limb::LeftArm,
                    Vec3::new(-0.06, 0.68, 0.11),
                    Some(Vec3::new(1.0, -0.4, -0.2)),
                )
                .ik(
                    Limb::RightArm,
                    Vec3::new(0.06, 0.70, 0.12),
                    Some(Vec3::new(-1.0, -0.4, -0.2)),
                )
                .hands(HandShape::Fist, HandShape::Fist);
            }
            PosePreset::HandsOnHips => {
                b.ik(
                    Limb::LeftArm,
                    Vec3::new(0.12, 0.56, 0.01),
                    Some(Vec3::new(1.0, 0.0, -0.4)),
                )
                .ik(
                    Limb::RightArm,
                    Vec3::new(-0.12, 0.56, 0.01),
                    Some(Vec3::new(-1.0, 0.0, -0.4)),
                )
                .hands(HandShape::Grip, HandShape::Grip);
            }
            PosePreset::FightStance => {
                b.j(Spine, 8.0, 10.0, 0.0)
                    .j(Head, 0.0, -10.0, 0.0)
                    .j(UpperLegL, -25.0, 0.0, 8.0)
                    .j(LowerLegL, 25.0, 0.0, 0.0)
                    .j(UpperLegR, 15.0, 0.0, -8.0)
                    .j(LowerLegR, 25.0, 0.0, 0.0)
                    .ik(Limb::LeftArm, Vec3::new(0.04, 0.8, 0.24), None)
                    .ik(Limb::RightArm, Vec3::new(-0.06, 0.76, 0.15), None)
                    .hands(HandShape::Fist, HandShape::Fist);
            }
            PosePreset::Fall => {
                b.j(Hips, -35.0, 0.0, 0.0)
                    .j(Head, 20.0, 0.0, 0.0)
                    .sym(UpperArmL, -140.0, 0.0, 35.0)
                    .sym(LowerArmL, -30.0, 0.0, 0.0)
                    .j(UpperLegL, -40.0, 0.0, 0.0)
                    .j(LowerLegL, 60.0, 0.0, 0.0)
                    .j(UpperLegR, 10.0, 0.0, 0.0)
                    .j(LowerLegR, 30.0, 0.0, 0.0)
                    .hands(HandShape::Open, HandShape::Open);
            }
            PosePreset::LieDown => {
                b.j(Hips, -90.0, 0.0, 0.0)
                    .j(Head, 5.0, 0.0, 0.0)
                    .sym(UpperArmL, 0.0, 0.0, 10.0)
                    .sym(FootL, -20.0, 0.0, 0.0)
                    .offset(0.0, -0.4, 0.0);
            }
            PosePreset::PhoneCall => {
                b.sym(UpperArmL, 0.0, 0.0, 6.0)
                    .sym(LowerArmL, -8.0, 0.0, 0.0)
                    .ik(
                        Limb::RightArm,
                        Vec3::new(-0.085, 0.885, 0.03),
                        Some(Vec3::new(-0.6, -1.0, 0.1)),
                    )
                    .j(Head, 4.0, -6.0, -6.0)
                    .hands(HandShape::Relaxed, HandShape::Grip);
            }
            PosePreset::Carry => {
                b.j(Spine, -5.0, 0.0, 0.0)
                    .ik(
                        Limb::LeftArm,
                        Vec3::new(0.12, 0.62, 0.22),
                        Some(Vec3::new(1.0, -1.0, 0.0)),
                    )
                    .ik(
                        Limb::RightArm,
                        Vec3::new(-0.12, 0.62, 0.22),
                        Some(Vec3::new(-1.0, -1.0, 0.0)),
                    )
                    .hands(HandShape::Grip, HandShape::Grip);
            }
        }
        b.pose
    }
}

/// The whole built-in library, in [`PosePreset::ALL`] order.
pub fn pose_library() -> Vec<Pose> {
    PosePreset::ALL.iter().map(|p| p.pose()).collect()
}

/// Helper for authoring presets; IK targets are in figure-height units on
/// the reference (average adult) rig.
struct Builder {
    pose: Pose,
    rig: Rig,
}

impl Builder {
    fn new(name: &str) -> Self {
        Builder {
            pose: Pose {
                name: name.into(),
                ..Default::default()
            },
            rig: Rig::new(&MannequinParams::default()),
        }
    }
    fn j(&mut self, b: Bone, x: f32, y: f32, z: f32) -> &mut Self {
        self.pose.set(b, JointRotation::new(x, y, z));
        self
    }
    fn sym(&mut self, left: Bone, x: f32, y: f32, z: f32) -> &mut Self {
        let r = JointRotation::new(x, y, z);
        self.pose.set(left, r);
        self.pose.set(left.mirror(), r.mirrored());
        self
    }
    fn hands(&mut self, l: HandShape, r: HandShape) -> &mut Self {
        self.pose.left_hand = l;
        self.pose.right_hand = r;
        self
    }
    fn offset(&mut self, x: f32, y: f32, z: f32) -> &mut Self {
        self.pose.hips_offset = Vec3::new(x, y, z);
        self
    }
    fn ik(&mut self, limb: Limb, target_h: Vec3, pole: Option<Vec3>) -> &mut Self {
        let target = target_h * self.rig.height;
        solve_two_bone_ik(&self.rig, &mut self.pose, limb, target, pole, false);
        self
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn rig() -> Rig {
        Rig::new(&MannequinParams::default())
    }

    #[test]
    fn library_is_complete_and_within_limits() {
        let lib = pose_library();
        assert_eq!(lib.len(), 18);
        for (p, preset) in lib.iter().zip(PosePreset::ALL) {
            assert_eq!(p.name, preset.name());
            assert!(p.within_limits(0.01), "{} exceeds limits", p.name);
            assert_eq!(PosePreset::from_name(preset.name()), Some(preset));
            let json = serde_json::to_string(p).unwrap();
            let back: Pose = serde_json::from_str(&json).unwrap();
            assert_eq!(&back, p);
        }
    }

    #[test]
    fn mirroring_swaps_sides_and_round_trips() {
        let p = PosePreset::Point.pose();
        let m = p.mirrored();
        assert_eq!(m.left_hand, HandShape::Point);
        let rig = rig();
        let a = forward_kinematics(&rig, &p.local_rotations(), Vec3::ZERO);
        let b = forward_kinematics(&rig, &m.local_rotations(), Vec3::ZERO);
        for &bone in Bone::ALL {
            let pa = a[bone.index()].tail;
            let pb = b[bone.mirror().index()].tail;
            assert!(
                (pa * Vec3::new(-1.0, 1.0, 1.0) - pb).length() < 1e-3,
                "{bone:?}"
            );
        }
        let mm = m.mirrored();
        assert_eq!(mm.joints, p.joints);
    }

    #[test]
    fn ik_reaches_targets_within_limits() {
        let rig = rig();
        let h = rig.height;
        let cases = [
            (Limb::RightArm, Vec3::new(-0.2, 0.8, 0.25)),
            (Limb::LeftArm, Vec3::new(0.25, 0.95, 0.1)),
            (Limb::LeftArm, Vec3::new(0.0, 0.65, 0.15)),
            (Limb::RightArm, Vec3::new(-0.3, 0.6, -0.05)),
            (Limb::LeftLeg, Vec3::new(0.06, 0.15, 0.2)),
            (Limb::RightLeg, Vec3::new(-0.08, 0.25, 0.25)),
        ];
        for (limb, t) in cases {
            let mut pose = Pose::rest();
            let r = solve_two_bone_ik(&rig, &mut pose, limb, t * h, None, true);
            assert!(r.reached && r.error < 0.005, "{limb:?} {t}: {r:?}");
            assert!(pose.within_limits(0.01), "{limb:?}: {:?}", pose.joints);
        }
        // Out of reach: the limb straightens toward the target.
        let mut pose = Pose::rest();
        let r = solve_two_bone_ik(
            &rig,
            &mut pose,
            Limb::LeftArm,
            Vec3::new(2.0, 1.4, 0.0),
            None,
            false,
        );
        assert!(!r.reached);
        let bones = forward_kinematics(&rig, &pose.local_rotations(), Vec3::ZERO);
        let dir =
            (bones[Bone::HandL.index()].head - bones[Bone::UpperArmL.index()].head).normalize();
        let want = (Vec3::new(2.0, 1.4, 0.0) - bones[Bone::UpperArmL.index()].head).normalize();
        assert!(dir.dot(want) > 0.99);
    }

    #[test]
    fn elbows_bend_toward_pole() {
        let rig = rig();
        let mut pose = Pose::rest();
        let t = Vec3::new(-0.1, 0.75, 0.2) * rig.height;
        solve_two_bone_ik(
            &rig,
            &mut pose,
            Limb::RightArm,
            t,
            Some(Vec3::new(-1.0, 0.0, 0.0)),
            false,
        );
        let b = forward_kinematics(&rig, &pose.local_rotations(), Vec3::ZERO);
        let sh = b[Bone::UpperArmR.index()].head;
        let el = b[Bone::LowerArmR.index()].head;
        let mid = (sh + t) * 0.5;
        assert!(el.x < mid.x, "elbow should bend outward (-x)");
    }

    #[test]
    fn look_at_turns_head() {
        let rig = rig();
        let mut pose = Pose::rest();
        apply_look_at(&rig, &mut pose, Vec3::new(3.0, 1.6, 1.0));
        let total = pose.rotation(Bone::Neck).y + pose.rotation(Bone::Head).y;
        assert!(total > 50.0, "{total}");
        assert!(pose.within_limits(0.01));
        let mut down = Pose::rest();
        apply_look_at(&rig, &mut down, Vec3::new(0.0, 0.0, 1.0));
        assert!(down.rotation(Bone::Neck).x + down.rotation(Bone::Head).x > 30.0);
    }

    #[test]
    fn blend_midpoint() {
        let a = PosePreset::Stand.pose();
        let b = PosePreset::Walk.pose();
        let m = Pose::blend(&a, &b, 0.5);
        let x = m.rotation(Bone::UpperLegL).x;
        assert!((x - (-12.5)).abs() < 0.5, "{x}");
        assert_eq!(
            Pose::blend(&a, &b, 0.0).rotation(Bone::UpperLegL),
            a.rotation(Bone::UpperLegL)
        );
    }

    #[test]
    fn hand_shapes_curl_fingers() {
        let mut p = Pose::rest();
        p.left_hand = HandShape::Fist;
        p.right_hand = HandShape::Fist;
        assert!(p.rotation(Bone::Index1L).z < -60.0);
        assert!(p.rotation(Bone::Index1R).z > 60.0);
        p.right_hand = HandShape::Point;
        assert_eq!(p.rotation(Bone::Index1R), JointRotation::ZERO);
        assert!(p.within_limits(0.01));
    }

    #[test]
    fn face_presets_have_strokes() {
        for f in FacePreset::ALL {
            let s = f.strokes();
            if f == FacePreset::Blank {
                assert!(s.is_empty());
            } else {
                assert_eq!(s.len(), 5, "{f:?}");
            }
        }
    }
}
