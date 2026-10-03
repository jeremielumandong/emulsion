//! The mannequin skeleton: bone set, joint limits, rest rig and forward
//! kinematics.
//!
//! Every bone's rest frame is aligned with the character frame (X = the
//! character's left, Y = up, Z = forward), so a joint rotation is easy to read:
//! rotations are Euler angles in degrees applied as `Rx(x) · Rz(z) · Ry(y)`
//! (twist about the bone's own axis first, then side swing, then front/back
//! swing). For the main bones that means:
//!
//! | Bone | `x` | `y` | `z` |
//! | --- | --- | --- | --- |
//! | spine, chest, neck, head | + bends forward | + turns left | + leans right |
//! | upper arm / leg (left) | − swings forward | twist | + raises outward |
//! | lower arm (left) | − bends the elbow | twist | — |
//! | lower leg | + bends the knee | — | — |
//! | fingers (left) | — | — | − curls |
//!
//! Right-side bones are the mirror image: their `y` and `z` signs flip.

use glam::{EulerRot, Quat, Vec3};
use serde::{Deserialize, Serialize};

use crate::mannequin::MannequinParams;

macro_rules! bones {
    ($($v:ident => $name:literal, $parent:expr;)*) => {
        /// A bone of the built-in mannequin skeleton. The declaration order
        /// lists parents before children.
        #[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, PartialOrd, Ord, Serialize, Deserialize)]
        #[serde(rename_all = "snake_case")]
        pub enum Bone { $($v),* }

        impl Bone {
            /// Every bone, parents before children.
            pub const ALL: &'static [Bone] = &[$(Bone::$v),*];

            /// Stable snake_case name (as serialized).
            pub fn name(self) -> &'static str {
                match self { $(Bone::$v => $name),* }
            }

            /// The parent bone; `None` for the hips (root).
            pub fn parent(self) -> Option<Bone> {
                match self { $(Bone::$v => $parent),* }
            }
        }
    };
}

bones! {
    Hips => "hips", None;
    Spine => "spine", Some(Bone::Hips);
    Chest => "chest", Some(Bone::Spine);
    Neck => "neck", Some(Bone::Chest);
    Head => "head", Some(Bone::Neck);
    ShoulderL => "shoulder_l", Some(Bone::Chest);
    UpperArmL => "upper_arm_l", Some(Bone::ShoulderL);
    LowerArmL => "lower_arm_l", Some(Bone::UpperArmL);
    HandL => "hand_l", Some(Bone::LowerArmL);
    Thumb1L => "thumb1_l", Some(Bone::HandL);
    Thumb2L => "thumb2_l", Some(Bone::Thumb1L);
    Index1L => "index1_l", Some(Bone::HandL);
    Index2L => "index2_l", Some(Bone::Index1L);
    Fingers1L => "fingers1_l", Some(Bone::HandL);
    Fingers2L => "fingers2_l", Some(Bone::Fingers1L);
    ShoulderR => "shoulder_r", Some(Bone::Chest);
    UpperArmR => "upper_arm_r", Some(Bone::ShoulderR);
    LowerArmR => "lower_arm_r", Some(Bone::UpperArmR);
    HandR => "hand_r", Some(Bone::LowerArmR);
    Thumb1R => "thumb1_r", Some(Bone::HandR);
    Thumb2R => "thumb2_r", Some(Bone::Thumb1R);
    Index1R => "index1_r", Some(Bone::HandR);
    Index2R => "index2_r", Some(Bone::Index1R);
    Fingers1R => "fingers1_r", Some(Bone::HandR);
    Fingers2R => "fingers2_r", Some(Bone::Fingers1R);
    UpperLegL => "upper_leg_l", Some(Bone::Hips);
    LowerLegL => "lower_leg_l", Some(Bone::UpperLegL);
    FootL => "foot_l", Some(Bone::LowerLegL);
    UpperLegR => "upper_leg_r", Some(Bone::Hips);
    LowerLegR => "lower_leg_r", Some(Bone::UpperLegR);
    FootR => "foot_r", Some(Bone::LowerLegR);
}

/// Number of bones in the mannequin skeleton.
pub const BONE_COUNT: usize = 31;

/// Which side of the body a bone is on.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Side {
    Center,
    Left,
    Right,
}

impl Bone {
    /// Index into [`Bone::ALL`] (and into per-bone arrays).
    pub fn index(self) -> usize {
        self as usize
    }

    /// Looks a bone up by its serialized name.
    pub fn from_name(name: &str) -> Option<Bone> {
        Bone::ALL.iter().copied().find(|b| b.name() == name)
    }

    pub fn side(self) -> Side {
        let n = self.name();
        if n.ends_with("_l") {
            Side::Left
        } else if n.ends_with("_r") {
            Side::Right
        } else {
            Side::Center
        }
    }

    /// The same bone on the other side (centre bones map to themselves).
    pub fn mirror(self) -> Bone {
        let n = self.name();
        let other = if let Some(s) = n.strip_suffix("_l") {
            format!("{s}_r")
        } else if let Some(s) = n.strip_suffix("_r") {
            format!("{s}_l")
        } else {
            return self;
        };
        Bone::from_name(&other).unwrap_or(self)
    }

    /// True for the finger and thumb bones.
    pub fn is_finger(self) -> bool {
        let n = self.name();
        n.starts_with("thumb") || n.starts_with("index") || n.starts_with("fingers")
    }

    /// Joint limits for this bone, degrees.
    pub fn limits(self) -> JointLimits {
        let left = match self.side() {
            Side::Right => self.mirror(),
            _ => self,
        };
        let l = left_limits(left);
        if self.side() == Side::Right {
            // Mirroring negates y and z, which swaps and negates their bounds.
            JointLimits {
                min: Vec3::new(l.min.x, -l.max.y, -l.max.z),
                max: Vec3::new(l.max.x, -l.min.y, -l.min.z),
            }
        } else {
            l
        }
    }
}

/// Per-axis joint limits in degrees (see the module docs for axes).
#[derive(Debug, Clone, Copy, PartialEq)]
pub struct JointLimits {
    pub min: Vec3,
    pub max: Vec3,
}

impl JointLimits {
    pub fn clamp(&self, r: JointRotation) -> JointRotation {
        let v = r.as_vec3().clamp(self.min, self.max);
        JointRotation::new(v.x, v.y, v.z)
    }

    pub fn contains(&self, r: JointRotation, tolerance: f32) -> bool {
        let v = r.as_vec3();
        v.cmpge(self.min - Vec3::splat(tolerance)).all()
            && v.cmple(self.max + Vec3::splat(tolerance)).all()
    }
}

fn lim(min: [f32; 3], max: [f32; 3]) -> JointLimits {
    JointLimits {
        min: Vec3::from(min),
        max: Vec3::from(max),
    }
}

fn left_limits(b: Bone) -> JointLimits {
    use Bone::*;
    match b {
        Hips => lim([-180.0; 3], [180.0; 3]),
        Spine => lim([-30.0, -35.0, -25.0], [50.0, 35.0, 25.0]),
        Chest => lim([-25.0, -35.0, -20.0], [40.0, 35.0, 20.0]),
        Neck => lim([-40.0, -50.0, -30.0], [50.0, 50.0, 30.0]),
        Head => lim([-40.0, -45.0, -30.0], [40.0, 45.0, 30.0]),
        ShoulderL => lim([-20.0, -20.0, -15.0], [20.0, 20.0, 30.0]),
        UpperArmL => lim([-180.0, -90.0, -50.0], [70.0, 90.0, 90.0]),
        LowerArmL => lim([-155.0, -90.0, 0.0], [0.0, 90.0, 0.0]),
        HandL => lim([-80.0, -30.0, -70.0], [80.0, 30.0, 70.0]),
        Thumb1L | Thumb2L => lim([-60.0, -30.0, -60.0], [60.0, 30.0, 30.0]),
        Index1L | Index2L | Fingers1L | Fingers2L => lim([-20.0, -5.0, -90.0], [20.0, 5.0, 15.0]),
        UpperLegL => lim([-130.0, -45.0, -30.0], [40.0, 45.0, 70.0]),
        LowerLegL => lim([0.0, 0.0, 0.0], [155.0, 0.0, 0.0]),
        FootL => lim([-45.0, -30.0, -25.0], [50.0, 30.0, 25.0]),
        _ => lim([-180.0; 3], [180.0; 3]),
    }
}

/// A joint rotation in degrees (see the module docs for the axes).
#[derive(Debug, Clone, Copy, Default, PartialEq, Serialize, Deserialize)]
#[serde(default)]
pub struct JointRotation {
    pub x: f32,
    pub y: f32,
    pub z: f32,
}

impl JointRotation {
    pub const ZERO: JointRotation = JointRotation {
        x: 0.0,
        y: 0.0,
        z: 0.0,
    };

    pub const fn new(x: f32, y: f32, z: f32) -> Self {
        JointRotation { x, y, z }
    }

    pub fn as_vec3(self) -> Vec3 {
        Vec3::new(self.x, self.y, self.z)
    }

    /// The local rotation quaternion `Rx(x) · Rz(z) · Ry(y)`.
    pub fn to_quat(self) -> Quat {
        Quat::from_euler(
            EulerRot::XZY,
            self.x.to_radians(),
            self.z.to_radians(),
            self.y.to_radians(),
        )
    }

    /// Converts a local rotation quaternion back to degrees.
    pub fn from_quat(q: Quat) -> Self {
        let (x, z, y) = q.normalize().to_euler(EulerRot::XZY);
        JointRotation::new(x.to_degrees(), y.to_degrees(), z.to_degrees())
    }

    /// Mirror image across the character's YZ plane (for the other side).
    pub fn mirrored(self) -> Self {
        JointRotation::new(self.x, -self.y, -self.z)
    }

    pub fn is_finite(self) -> bool {
        self.as_vec3().is_finite()
    }
}

/// The rest (unposed) skeleton of one mannequin, in character space
/// (origin on the ground between the feet).
#[derive(Debug, Clone, PartialEq)]
pub struct Rig {
    /// Joint position of each bone at rest.
    pub heads: [Vec3; BONE_COUNT],
    /// Vector from each bone's head to its tail at rest.
    pub tails: [Vec3; BONE_COUNT],
    /// Total figure height in metres.
    pub height: f32,
    /// Head height (chin to crown) in metres.
    pub head_height: f32,
}

impl Rig {
    /// Builds the rest rig for the given body parameters.
    pub fn new(p: &MannequinParams) -> Rig {
        let pr = p.proportions();
        let h = pr.height;
        let mut heads = [Vec3::ZERO; BONE_COUNT];
        let mut tails = [Vec3::ZERO; BONE_COUNT];
        let mut set = |b: Bone, head: Vec3, tail: Vec3| {
            heads[b.index()] = head;
            tails[b.index()] = tail;
        };
        let hip_y = pr.ankle_y + pr.leg_length;
        let torso = pr.neck_base_y - hip_y;
        let spine_y = hip_y + 0.18 * torso;
        let chest_y = hip_y + 0.5 * torso;
        set(
            Bone::Hips,
            Vec3::new(0.0, hip_y, 0.0),
            Vec3::new(0.0, spine_y - hip_y, 0.0),
        );
        set(
            Bone::Spine,
            Vec3::new(0.0, spine_y, 0.0),
            Vec3::new(0.0, chest_y - spine_y, 0.0),
        );
        set(
            Bone::Chest,
            Vec3::new(0.0, chest_y, 0.0),
            Vec3::new(0.0, pr.neck_base_y - chest_y, 0.0),
        );
        set(
            Bone::Neck,
            Vec3::new(0.0, pr.neck_base_y, 0.0),
            Vec3::new(0.0, pr.neck_length, 0.0),
        );
        let head_base = pr.neck_base_y + pr.neck_length;
        set(
            Bone::Head,
            Vec3::new(0.0, head_base, 0.0),
            Vec3::new(0.0, h - head_base, 0.0),
        );

        let shoulder_y = hip_y + 0.9 * torso;
        let thigh = pr.leg_length * 0.51;
        let shin = pr.leg_length - thigh;
        for (side, sx) in [(Side::Left, 1.0f32), (Side::Right, -1.0f32)] {
            let pick = |l: Bone| if side == Side::Left { l } else { l.mirror() };
            let clav = Vec3::new(0.02 * h * sx, shoulder_y - 0.01 * h, 0.0);
            let sh = Vec3::new(pr.shoulder_half * sx, shoulder_y, -0.01 * h);
            set(pick(Bone::ShoulderL), clav, sh - clav);
            let elbow = sh + Vec3::new(0.012 * h * sx, -pr.upper_arm, 0.0);
            set(pick(Bone::UpperArmL), sh, elbow - sh);
            let wrist = elbow + Vec3::new(0.0, -pr.forearm, 0.01 * h);
            set(pick(Bone::LowerArmL), elbow, wrist - elbow);
            let palm = pr.hand * 0.5;
            let knuckle = wrist + Vec3::new(0.0, -palm, 0.0);
            set(pick(Bone::HandL), wrist, knuckle - wrist);
            let f1 = pr.hand * 0.28;
            let f2 = pr.hand * 0.22;
            let index_root = knuckle + Vec3::new(0.0, 0.0, pr.hand_width * 0.3);
            set(pick(Bone::Index1L), index_root, Vec3::new(0.0, -f1, 0.0));
            set(
                pick(Bone::Index2L),
                index_root + Vec3::new(0.0, -f1, 0.0),
                Vec3::new(0.0, -f2, 0.0),
            );
            let fing_root = knuckle + Vec3::new(0.0, 0.0, -pr.hand_width * 0.15);
            set(pick(Bone::Fingers1L), fing_root, Vec3::new(0.0, -f1, 0.0));
            set(
                pick(Bone::Fingers2L),
                fing_root + Vec3::new(0.0, -f1, 0.0),
                Vec3::new(0.0, -f2, 0.0),
            );
            let thumb_root = wrist + Vec3::new(-0.004 * h * sx, -palm * 0.25, pr.hand_width * 0.45);
            let t1 = Vec3::new(-0.15 * sx, -0.75, 0.65).normalize() * pr.hand * 0.25;
            set(pick(Bone::Thumb1L), thumb_root, t1);
            set(
                pick(Bone::Thumb2L),
                thumb_root + t1,
                Vec3::new(0.0, -0.8, 0.6).normalize() * pr.hand * 0.2,
            );

            let hip = Vec3::new(pr.hip_half * sx, hip_y, 0.0);
            let knee = hip + Vec3::new(0.0, -thigh, 0.004 * h);
            let ankle = Vec3::new(pr.hip_half * sx * 0.95, pr.ankle_y, -0.004 * h);
            set(pick(Bone::UpperLegL), hip, knee - hip);
            set(pick(Bone::LowerLegL), knee, ankle - knee);
            let _ = shin;
            set(
                pick(Bone::FootL),
                ankle,
                Vec3::new(0.0, -pr.ankle_y * 0.75, pr.foot * 0.72),
            );
        }
        Rig {
            heads,
            tails,
            height: h,
            head_height: pr.head_height,
        }
    }

    /// Length of a bone.
    pub fn length(&self, b: Bone) -> f32 {
        self.tails[b.index()].length()
    }

    /// Rest offset of `b`'s head from its parent's head.
    pub fn offset(&self, b: Bone) -> Vec3 {
        match b.parent() {
            Some(p) => self.heads[b.index()] - self.heads[p.index()],
            None => self.heads[b.index()],
        }
    }
}

/// World (character-space) placement of one bone after posing.
#[derive(Debug, Clone, Copy, PartialEq)]
pub struct BonePose {
    /// Joint position.
    pub head: Vec3,
    /// Accumulated rotation from the character frame.
    pub rotation: Quat,
    /// Tip position.
    pub tail: Vec3,
}

/// Forward kinematics: local joint rotations to character-space bones.
pub fn forward_kinematics(
    rig: &Rig,
    local: &[Quat; BONE_COUNT],
    hips_offset: Vec3,
) -> [BonePose; BONE_COUNT] {
    let mut out = [BonePose {
        head: Vec3::ZERO,
        rotation: Quat::IDENTITY,
        tail: Vec3::ZERO,
    }; BONE_COUNT];
    for &b in Bone::ALL {
        let i = b.index();
        let (head, rot) = match b.parent() {
            None => (rig.heads[i] + hips_offset, local[i]),
            Some(p) => {
                let pp = out[p.index()];
                (
                    pp.head + pp.rotation * rig.offset(b),
                    pp.rotation * local[i],
                )
            }
        };
        out[i] = BonePose {
            head,
            rotation: rot,
            tail: head + rot * rig.tails[i],
        };
    }
    out
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn bone_table_is_consistent() {
        assert_eq!(Bone::ALL.len(), BONE_COUNT);
        for (i, b) in Bone::ALL.iter().enumerate() {
            assert_eq!(b.index(), i);
            assert_eq!(Bone::from_name(b.name()), Some(*b));
            assert_eq!(b.mirror().mirror(), *b);
            if let Some(p) = b.parent() {
                assert!(p.index() < i, "{b:?} parent after child");
            }
        }
    }

    #[test]
    fn euler_order_is_x_z_y() {
        let r = JointRotation::new(-90.0, 0.0, 0.0);
        // Left arm hanging down swings forward for negative x.
        let v = r.to_quat() * Vec3::NEG_Y;
        assert!(v.z > 0.99, "{v}");
        let r = JointRotation::new(0.0, 0.0, 90.0);
        let v = r.to_quat() * Vec3::NEG_Y;
        assert!(v.x > 0.99, "positive z raises the left arm outward: {v}");
        let back = JointRotation::from_quat(JointRotation::new(-40.0, 25.0, 30.0).to_quat());
        assert!(
            (back.as_vec3() - Vec3::new(-40.0, 25.0, 30.0)).length() < 1e-2,
            "{back:?}"
        );
    }

    #[test]
    fn right_limits_mirror_left() {
        let l = Bone::UpperArmL.limits();
        let r = Bone::UpperArmR.limits();
        assert_eq!(r.min.z, -l.max.z);
        assert_eq!(r.max.z, -l.min.z);
        assert_eq!(r.min.x, l.min.x);
    }
}
