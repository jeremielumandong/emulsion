//! Evaluating a [`Character`]: pose → ground lock → IK → look-at → bones.

use glam::{Mat4, Vec3};

use crate::mannequin::{self, MannequinMesh, part_matrix};
use crate::pose::{IkResult, Limb, Pose, apply_look_at, solve_two_bone_ik};
use crate::scene::{Character, ObjectId, SceneObject};
use crate::skeleton::{BONE_COUNT, Bone, BonePose, Rig, forward_kinematics};

/// A character after posing, with bones in character space and the object
/// transform to place them in the world.
#[derive(Debug, Clone)]
pub struct PosedCharacter {
    pub id: ObjectId,
    /// The effective pose (IK and look-at baked in, ground lock in `hips_offset`).
    pub pose: Pose,
    pub rig: Rig,
    /// Bones in character space.
    pub bones: [BonePose; BONE_COUNT],
    /// Character-to-world matrix.
    pub world: Mat4,
    /// Result of each IK target, in the order they were applied.
    pub ik_results: Vec<(Limb, IkResult)>,
}

impl PosedCharacter {
    /// World position of a bone's joint.
    pub fn joint_world(&self, b: Bone) -> Vec3 {
        self.world.transform_point3(self.bones[b.index()].head)
    }

    /// World position of a bone's tip.
    pub fn tip_world(&self, b: Bone) -> Vec3 {
        self.world.transform_point3(self.bones[b.index()].tail)
    }

    /// World position of the top of the head.
    pub fn crown_world(&self) -> Vec3 {
        self.tip_world(Bone::Head)
    }

    /// World position between the eyes.
    pub fn eyes_world(&self) -> Vec3 {
        let h = self.bones[Bone::Head.index()];
        self.world.transform_point3(
            h.head
                + h.rotation
                    * Vec3::new(0.0, self.rig.head_height * 0.55, self.rig.head_height * 0.3),
        )
    }

    /// World direction the face points.
    pub fn facing_world(&self) -> Vec3 {
        let h = self.bones[Bone::Head.index()];
        self.world
            .transform_vector3(h.rotation * Vec3::Z)
            .normalize_or_zero()
    }

    /// World direction the body (chest) faces, flattened onto the ground.
    pub fn body_facing_world(&self) -> Vec3 {
        let c = self.bones[Bone::Chest.index()];
        let f = self.world.transform_vector3(c.rotation * Vec3::Z);
        Vec3::new(f.x, 0.0, f.z).try_normalize().unwrap_or(Vec3::Z)
    }
}

/// Evaluates one character object; `None` when the object is not a character.
pub fn pose_character(obj: &SceneObject) -> Option<PosedCharacter> {
    let c = obj.character()?;
    let mm = mannequin::generate(&c.body);
    Some(evaluate(obj.id, c, &obj.transform.matrix(), &mm))
}

/// Evaluates a character with an already generated mannequin.
pub fn evaluate(id: ObjectId, c: &Character, world: &Mat4, mm: &MannequinMesh) -> PosedCharacter {
    let rig = mm.rig.clone();
    let h = rig.height;
    let mut pose = c.pose.clone();
    pose.clamp_to_limits();
    let inv = world.inverse();
    let foot_ik = c.ik.iter().any(|t| t.limb.is_leg());
    if c.ground_lock && !foot_ik {
        let bones = forward_kinematics(&rig, &pose.local_rotations(), pose.hips_offset * h);
        let min_y = lowest_point(mm, &bones);
        if min_y.is_finite() {
            pose.hips_offset.y -= min_y / h;
        }
    }
    let mut ik_results = Vec::new();
    for t in &c.ik {
        let target = inv.transform_point3(t.target);
        let pole = t.pole.map(|p| inv.transform_vector3(p));
        let r = solve_two_bone_ik(&rig, &mut pose, t.limb, target, pole, t.limb.is_leg());
        ik_results.push((t.limb, r));
    }
    if let Some(target) = c.look_at {
        apply_look_at(&rig, &mut pose, inv.transform_point3(target));
    }
    let bones = forward_kinematics(&rig, &pose.local_rotations(), pose.hips_offset * h);
    PosedCharacter {
        id,
        pose,
        rig,
        bones,
        world: *world,
        ik_results,
    }
}

/// Lowest y of the posed figure's surface (character space).
fn lowest_point(mm: &MannequinMesh, bones: &[BonePose; BONE_COUNT]) -> f32 {
    let mut min = f32::INFINITY;
    for part in &mm.parts {
        let m = part_matrix(&bones[part.bone.index()]);
        for p in &part.mesh.positions {
            min = min.min(m.transform_point3(*p).y);
        }
    }
    min
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::pose::PosePreset;
    use crate::scene::{ObjectKind, Scene};

    fn posed(preset: PosePreset) -> (PosedCharacter, MannequinMesh) {
        let c = Character::default().with_pose(preset);
        let mm = mannequin::generate(&c.body);
        (evaluate(ObjectId(1), &c, &Mat4::IDENTITY, &mm), mm)
    }

    #[test]
    fn ground_lock_rests_every_pose_on_the_ground() {
        for preset in PosePreset::ALL {
            let (p, mm) = posed(preset);
            let low = lowest_point(&mm, &p.bones);
            assert!(low.abs() < 1e-3, "{preset:?}: {low}");
        }
    }

    #[test]
    fn poses_mean_what_they_say() {
        let h = Character::default().body.height_m();
        let (stand, _) = posed(PosePreset::Stand);
        let (sit, _) = posed(PosePreset::Sit);
        let (lie, _) = posed(PosePreset::LieDown);
        let (crouch, _) = posed(PosePreset::Crouch);
        let (point, _) = posed(PosePreset::Point);
        let (wave, _) = posed(PosePreset::Wave);
        let (run, _) = posed(PosePreset::Run);
        let head = |p: &PosedCharacter| p.crown_world().y;
        assert!((head(&stand) - h).abs() < 0.03 * h);
        assert!(
            head(&sit) < 0.8 * h && head(&sit) > 0.6 * h,
            "sit {}",
            head(&sit)
        );
        assert!(head(&lie) < 0.3 * h, "lie {}", head(&lie));
        assert!(head(&crouch) < 0.75 * h);
        // Point: right wrist in front of the chest at about shoulder height.
        let w = point.joint_world(Bone::HandR);
        assert!(w.z > 0.25 * h && w.y > 0.7 * h, "point wrist {w}");
        // Wave: right wrist above the shoulder.
        assert!(wave.joint_world(Bone::HandR).y > wave.joint_world(Bone::UpperArmR).y + 0.1 * h);
        // Run: left knee forward of the hips.
        assert!(run.joint_world(Bone::LowerLegL).z > run.joint_world(Bone::Hips).z + 0.1 * h);
    }

    #[test]
    fn world_ik_targets_and_look_at() {
        let mut s = Scene::new();
        let id = s.add_character("A", Character::default(), Vec3::new(2.0, 0.0, 1.0), 90.0);
        let target = Vec3::new(2.35, 1.25, 0.85);
        s.set_ik_target(id, Limb::LeftArm, target);
        if let Some(c) = s.character_mut(id) {
            c.look_at = Some(Vec3::new(10.0, 1.6, 1.0));
        }
        let p = pose_character(s.object(id).unwrap()).unwrap();
        assert!((p.joint_world(Bone::HandL) - target).length() < 0.01);
        assert!(p.facing_world().x > 0.9, "{}", p.facing_world());
        assert!(matches!(
            s.object(id).unwrap().kind,
            ObjectKind::Character(_)
        ));
        assert!(s.bake_pose(id));
        let c = s.character(id).unwrap();
        assert!(c.ik.is_empty() && c.look_at.is_none());
        let again = pose_character(s.object(id).unwrap()).unwrap();
        assert!((again.joint_world(Bone::HandL) - target).length() < 0.01);
    }
}
