//! Parametric mannequins (SG2): body types, sliders and the procedurally
//! generated, rigidly skinned figure mesh.

use glam::{Mat4, Vec3};
use serde::{Deserialize, Serialize};

use crate::mesh::{self, Mesh};
use crate::skeleton::{BONE_COUNT, Bone, BonePose, Rig};

/// Built-in body type.
#[derive(Debug, Clone, Copy, Default, PartialEq, Eq, Hash, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum MannequinKind {
    AdultMale,
    AdultFemale,
    #[default]
    AdultNeutral,
    Child,
}

impl MannequinKind {
    pub const ALL: [MannequinKind; 4] = [
        MannequinKind::AdultMale,
        MannequinKind::AdultFemale,
        MannequinKind::AdultNeutral,
        MannequinKind::Child,
    ];

    /// Typical standing height in metres.
    pub fn default_height(self) -> f32 {
        match self {
            MannequinKind::AdultMale => 1.78,
            MannequinKind::AdultFemale => 1.65,
            MannequinKind::AdultNeutral => 1.72,
            MannequinKind::Child => 1.2,
        }
    }

    pub fn label(self) -> &'static str {
        match self {
            MannequinKind::AdultMale => "Adult male",
            MannequinKind::AdultFemale => "Adult female",
            MannequinKind::AdultNeutral => "Adult",
            MannequinKind::Child => "Child",
        }
    }
}

/// Body sliders. Multipliers are 1.0 for the type's average figure.
#[derive(Debug, Clone, Copy, PartialEq, Serialize, Deserialize)]
#[serde(default)]
pub struct MannequinParams {
    pub kind: MannequinKind,
    /// Standing height in metres (0.5–2.5); `None` uses the type's default.
    pub height: Option<f32>,
    /// 0 = slim, 0.5 = average, 1 = heavy.
    pub build: f32,
    /// Head size multiplier (0.7–1.5).
    pub head_size: f32,
    /// Leg length multiplier (0.8–1.2); the torso takes up the difference.
    pub leg_length: f32,
    /// Arm length multiplier (0.8–1.2).
    pub arm_length: f32,
    /// Shoulder width multiplier (0.7–1.4).
    pub shoulder_width: f32,
    /// Hip width multiplier (0.7–1.4).
    pub hip_width: f32,
}

impl Default for MannequinParams {
    fn default() -> Self {
        MannequinParams {
            kind: MannequinKind::default(),
            height: None,
            build: 0.5,
            head_size: 1.0,
            leg_length: 1.0,
            arm_length: 1.0,
            shoulder_width: 1.0,
            hip_width: 1.0,
        }
    }
}

/// Slider ranges, used by validation and the UI.
pub mod ranges {
    pub const HEIGHT: (f32, f32) = (0.5, 2.5);
    pub const BUILD: (f32, f32) = (0.0, 1.0);
    pub const HEAD_SIZE: (f32, f32) = (0.7, 1.5);
    pub const LIMB_LENGTH: (f32, f32) = (0.8, 1.2);
    pub const WIDTH: (f32, f32) = (0.7, 1.4);
}

/// Derived body measurements in metres.
#[derive(Debug, Clone, Copy, PartialEq)]
pub struct Proportions {
    pub height: f32,
    pub head_height: f32,
    pub neck_length: f32,
    pub neck_base_y: f32,
    pub ankle_y: f32,
    pub leg_length: f32,
    pub upper_arm: f32,
    pub forearm: f32,
    pub hand: f32,
    pub hand_width: f32,
    pub foot: f32,
    pub shoulder_half: f32,
    pub hip_half: f32,
    /// Limb thickness multiplier from `build`.
    pub girth: f32,
    /// Torso depth/width multiplier (female and child figures differ).
    pub chest_width: f32,
}

impl MannequinParams {
    /// A default figure of the given type.
    pub fn of(kind: MannequinKind) -> Self {
        MannequinParams {
            kind,
            ..Default::default()
        }
    }

    /// Height in metres (explicit or the type's default).
    pub fn height_m(&self) -> f32 {
        self.height.unwrap_or(self.kind.default_height())
    }

    /// Returns the parameters with every slider clamped to its range.
    pub fn clamped(&self) -> Self {
        let c = |v: f32, r: (f32, f32)| {
            if v.is_finite() {
                v.clamp(r.0, r.1)
            } else {
                (r.0 + r.1) * 0.5
            }
        };
        MannequinParams {
            kind: self.kind,
            height: self.height.map(|h| c(h, ranges::HEIGHT)),
            build: c(self.build, ranges::BUILD),
            head_size: c(self.head_size, ranges::HEAD_SIZE),
            leg_length: c(self.leg_length, ranges::LIMB_LENGTH),
            arm_length: c(self.arm_length, ranges::LIMB_LENGTH),
            shoulder_width: c(self.shoulder_width, ranges::WIDTH),
            hip_width: c(self.hip_width, ranges::WIDTH),
        }
    }

    /// Body measurements for these sliders.
    pub fn proportions(&self) -> Proportions {
        let p = self.clamped();
        let h = p.height_m();
        let (head_frac, leg_share, shoulder, hip, chest_w) = match p.kind {
            MannequinKind::AdultMale => (0.13, 0.575, 0.118, 0.052, 1.05),
            MannequinKind::AdultFemale => (0.13, 0.585, 0.102, 0.058, 0.92),
            MannequinKind::AdultNeutral => (0.13, 0.58, 0.11, 0.055, 1.0),
            MannequinKind::Child => (0.175, 0.535, 0.105, 0.052, 0.95),
        };
        let head_height = h * head_frac * p.head_size;
        let neck_length = h * if p.kind == MannequinKind::Child {
            0.032
        } else {
            0.04
        };
        let ankle_y = 0.045 * h;
        let body = h - head_height - neck_length - ankle_y;
        let leg_length = body * leg_share * p.leg_length;
        let neck_base_y = h - head_height - neck_length;
        let arm =
            h * if p.kind == MannequinKind::Child {
                0.42
            } else {
                0.44
            } * p.arm_length;
        Proportions {
            height: h,
            head_height,
            neck_length,
            neck_base_y,
            ankle_y,
            leg_length,
            upper_arm: arm * 0.42,
            forearm: arm * 0.335,
            hand: arm * 0.245,
            hand_width: h * 0.05,
            foot: h * 0.15,
            shoulder_half: h * shoulder * p.shoulder_width,
            hip_half: h * hip * p.hip_width,
            girth: 0.75 + 0.6 * p.build,
            chest_width: chest_w,
        }
    }
}

/// One rigid piece of a mannequin: a mesh in its bone's rest frame (relative
/// to the bone head) that follows that bone.
#[derive(Debug, Clone)]
pub struct MannequinPart {
    pub bone: Bone,
    pub mesh: Mesh,
}

/// A generated figure: rest rig plus rigid parts.
#[derive(Debug, Clone)]
pub struct MannequinMesh {
    pub rig: Rig,
    pub parts: Vec<MannequinPart>,
}

impl MannequinMesh {
    pub fn triangle_count(&self) -> usize {
        self.parts.iter().map(|p| p.mesh.triangle_count()).sum()
    }

    /// Bakes the posed figure into one mesh in character space.
    pub fn posed(&self, bones: &[BonePose; BONE_COUNT]) -> Mesh {
        let mut out = Mesh::default();
        for part in &self.parts {
            out.append_transformed(&part.mesh, &part_matrix(&bones[part.bone.index()]));
        }
        out
    }
}

/// Matrix placing a part (built relative to its bone head at rest) on a posed bone.
pub(crate) fn part_matrix(b: &BonePose) -> Mat4 {
    Mat4::from_rotation_translation(b.rotation, b.head)
}

/// Generates the rig and the figure mesh for these sliders.
pub fn generate(params: &MannequinParams) -> MannequinMesh {
    let rig = Rig::new(params);
    let pr = params.proportions();
    let h = pr.height;
    let g = pr.girth;
    let mut parts = Vec::with_capacity(40);
    let mut add = |bone: Bone, mesh: Mesh| parts.push(MannequinPart { bone, mesh });
    let tail = |b: Bone| rig.tails[b.index()];
    let at = |m: Mesh, p: Vec3| m.transformed(&Mat4::from_translation(p));
    let cw = pr.chest_width;
    let female = params.kind == MannequinKind::AdultFemale;

    // Torso.
    let pelvis = Vec3::new(
        pr.hip_half * 1.9 * (0.85 + 0.3 * g),
        h * 0.065,
        h * 0.06 * g,
    );
    add(
        Bone::Hips,
        at(mesh::ellipsoid(pelvis), Vec3::new(0.0, 0.01 * h, 0.0)),
    );
    let spine_len = rig.length(Bone::Spine);
    add(
        Bone::Spine,
        at(
            mesh::ellipsoid(Vec3::new(
                h * 0.085 * g * cw,
                spine_len * 0.75,
                h * 0.055 * g,
            )),
            Vec3::new(0.0, spine_len * 0.5, 0.0),
        ),
    );
    let chest_len = rig.length(Bone::Chest);
    let chest_r = Vec3::new(
        (pr.shoulder_half * 0.85).max(h * 0.08) * (0.85 + 0.2 * g),
        chest_len * 0.62,
        h * 0.065 * g * cw,
    );
    add(
        Bone::Chest,
        at(
            mesh::ellipsoid(chest_r),
            Vec3::new(0.0, chest_len * 0.45, 0.0),
        ),
    );
    if female {
        for sx in [-1.0, 1.0] {
            add(
                Bone::Chest,
                at(
                    mesh::ellipsoid(Vec3::new(h * 0.032, h * 0.028, h * 0.022) * (0.85 + 0.25 * g)),
                    Vec3::new(sx * h * 0.04, chest_len * 0.3, chest_r.z * 0.72),
                ),
            );
        }
    }
    add(
        Bone::Neck,
        mesh::tapered_capsule(
            Vec3::Y,
            rig.length(Bone::Neck),
            h * 0.028 * g.sqrt(),
            h * 0.025 * g.sqrt(),
        ),
    );
    let hh = pr.head_height;
    add(
        Bone::Head,
        at(mesh::ellipsoid(head_radii(hh)), head_center(hh)),
    );
    // A small nose shows which way the head faces.
    add(
        Bone::Head,
        at(
            mesh::ellipsoid(Vec3::new(hh * 0.06, hh * 0.09, hh * 0.08)),
            Vec3::new(0.0, hh * 0.42, hh * 0.38),
        ),
    );

    // Arms and hands.
    for left in [true, false] {
        let s = |b: Bone| if left { b } else { b.mirror() };
        let arm_r = h * 0.032 * g;
        add(
            s(Bone::ShoulderL),
            mesh::tapered_capsule(
                tail(s(Bone::ShoulderL)),
                rig.length(s(Bone::ShoulderL)),
                h * 0.025 * g,
                h * 0.036 * g,
            ),
        );
        add(
            s(Bone::UpperArmL),
            mesh::tapered_capsule(
                tail(s(Bone::UpperArmL)),
                rig.length(s(Bone::UpperArmL)),
                arm_r,
                arm_r * 0.8,
            ),
        );
        add(
            s(Bone::LowerArmL),
            mesh::tapered_capsule(
                tail(s(Bone::LowerArmL)),
                rig.length(s(Bone::LowerArmL)),
                arm_r * 0.78,
                arm_r * 0.55,
            ),
        );
        let palm_len = rig.length(s(Bone::HandL));
        add(
            s(Bone::HandL),
            at(
                mesh::ellipsoid(Vec3::new(
                    pr.hand_width * 0.2,
                    palm_len * 0.6,
                    pr.hand_width * 0.5,
                )),
                Vec3::new(0.0, -palm_len * 0.5, 0.0),
            ),
        );
        let fr = pr.hand_width * 0.14;
        for (b, r) in [
            (Bone::Index1L, fr),
            (Bone::Index2L, fr * 0.85),
            (Bone::Fingers1L, fr * 1.35),
            (Bone::Fingers2L, fr * 1.15),
            (Bone::Thumb1L, fr * 1.1),
            (Bone::Thumb2L, fr * 0.95),
        ] {
            let b = s(b);
            let mut m = mesh::tapered_capsule(tail(b), rig.length(b), r, r * 0.9);
            if matches!(
                b,
                Bone::Fingers1L | Bone::Fingers2L | Bone::Fingers1R | Bone::Fingers2R
            ) {
                // The grouped middle/ring/little fingers are flattened into a mitten.
                m.transform(&Mat4::from_scale(Vec3::new(0.75, 1.0, 1.9)));
            }
            add(b, m);
        }
    }

    // Legs and feet.
    for left in [true, false] {
        let s = |b: Bone| if left { b } else { b.mirror() };
        let leg_r = h * 0.05 * g;
        add(
            s(Bone::UpperLegL),
            mesh::tapered_capsule(
                tail(s(Bone::UpperLegL)),
                rig.length(s(Bone::UpperLegL)),
                leg_r,
                leg_r * 0.68,
            ),
        );
        add(
            s(Bone::LowerLegL),
            mesh::tapered_capsule(
                tail(s(Bone::LowerLegL)),
                rig.length(s(Bone::LowerLegL)),
                leg_r * 0.66,
                leg_r * 0.42,
            ),
        );
        let foot_tail = tail(s(Bone::FootL));
        let foot_len = pr.foot;
        let foot = mesh::ellipsoid(Vec3::new(
            h * 0.03 * g.sqrt(),
            pr.ankle_y * 0.55,
            foot_len * 0.52,
        ));
        add(
            s(Bone::FootL),
            at(foot, Vec3::new(0.0, foot_tail.y * 0.8, foot_len * 0.28)),
        );
    }
    MannequinMesh { rig, parts }
}

/// Head ellipsoid radii for a head of height `hh` (chin to crown).
pub(crate) fn head_radii(hh: f32) -> Vec3 {
    Vec3::new(hh * 0.36, hh * 0.5, hh * 0.42)
}

/// Head ellipsoid centre relative to the head bone (top of the neck).
pub(crate) fn head_center(hh: f32) -> Vec3 {
    Vec3::new(0.0, hh * 0.5, hh * 0.02)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn heights_match_sliders() {
        for kind in MannequinKind::ALL {
            let p = MannequinParams::of(kind);
            let m = generate(&p);
            let b = m
                .posed(&crate::skeleton::forward_kinematics(
                    &m.rig,
                    &[glam::Quat::IDENTITY; BONE_COUNT],
                    Vec3::ZERO,
                ))
                .bounds();
            let h = p.height_m();
            assert!(
                (b.max.y - h).abs() < 0.02 * h,
                "{kind:?} top {} vs {h}",
                b.max.y
            );
            assert!(b.min.y.abs() < 0.02 * h, "{kind:?} feet at {}", b.min.y);
        }
    }
}
