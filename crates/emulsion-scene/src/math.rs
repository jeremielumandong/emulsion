//! Transforms, rotations and bounding boxes.
//!
//! Conventions used throughout the crate: metres, right-handed, **Y up**.
//! Objects (characters, props, cameras, lights) face **+Z** at zero rotation,
//! so a character's left hand points toward +X and its right toward -X.

use glam::{EulerRot, Mat4, Quat, Vec3};
use serde::{Deserialize, Serialize};

/// A rotation in a form that is pleasant to edit by hand (yaw/pitch/roll in
/// degrees) or exact (a quaternion). Both serialize; JSON chooses by keys:
/// `{"yaw":30,"pitch":0,"roll":0}` or `{"quat":[x,y,z,w]}`.
#[derive(Debug, Clone, Copy, PartialEq, Serialize, Deserialize)]
#[serde(untagged)]
pub enum Rotation {
    /// Yaw about +Y (positive turns +Z toward +X, i.e. to the object's left),
    /// then pitch (positive tips the nose up), then roll about the forward
    /// axis (positive leans the top toward the object's right). Degrees.
    Euler { yaw: f32, pitch: f32, roll: f32 },
    /// An exact quaternion `[x, y, z, w]`.
    Quaternion { quat: Quat },
}

impl Default for Rotation {
    fn default() -> Self {
        Rotation::Euler {
            yaw: 0.0,
            pitch: 0.0,
            roll: 0.0,
        }
    }
}

impl Rotation {
    /// A yaw/pitch/roll rotation in degrees.
    pub fn euler(yaw: f32, pitch: f32, roll: f32) -> Self {
        Rotation::Euler { yaw, pitch, roll }
    }

    /// The rotation as a unit quaternion.
    pub fn to_quat(self) -> Quat {
        match self {
            Rotation::Euler { yaw, pitch, roll } => yaw_pitch_roll_quat(yaw, pitch, roll),
            Rotation::Quaternion { quat } => {
                if quat.length_squared() > 1e-12 && quat.is_finite() {
                    quat.normalize()
                } else {
                    Quat::IDENTITY
                }
            }
        }
    }

    /// Yaw, pitch and roll in degrees (converting a quaternion if needed).
    pub fn to_yaw_pitch_roll(self) -> (f32, f32, f32) {
        match self {
            Rotation::Euler { yaw, pitch, roll } => (yaw, pitch, roll),
            Rotation::Quaternion { quat } => quat_to_yaw_pitch_roll(quat),
        }
    }

    pub(crate) fn is_finite(self) -> bool {
        match self {
            Rotation::Euler { yaw, pitch, roll } => {
                yaw.is_finite() && pitch.is_finite() && roll.is_finite()
            }
            Rotation::Quaternion { quat } => quat.is_finite(),
        }
    }
}

/// Builds the quaternion for yaw/pitch/roll in degrees (see [`Rotation`]).
pub fn yaw_pitch_roll_quat(yaw: f32, pitch: f32, roll: f32) -> Quat {
    Quat::from_euler(
        EulerRot::YXZ,
        yaw.to_radians(),
        -pitch.to_radians(),
        roll.to_radians(),
    )
}

/// Inverse of [`yaw_pitch_roll_quat`], degrees.
pub fn quat_to_yaw_pitch_roll(q: Quat) -> (f32, f32, f32) {
    let (y, x, z) = q.normalize().to_euler(EulerRot::YXZ);
    (y.to_degrees(), -x.to_degrees(), z.to_degrees())
}

/// Position, rotation and scale of an object in the scene (metres).
#[derive(Debug, Clone, Copy, PartialEq, Serialize, Deserialize)]
#[serde(default)]
pub struct Transform {
    pub position: Vec3,
    pub rotation: Rotation,
    /// Per-axis scale; must be positive.
    pub scale: Vec3,
}

impl Default for Transform {
    fn default() -> Self {
        Transform {
            position: Vec3::ZERO,
            rotation: Rotation::default(),
            scale: Vec3::ONE,
        }
    }
}

impl Transform {
    /// A transform at `position` with no rotation and unit scale.
    pub fn at(position: Vec3) -> Self {
        Transform {
            position,
            ..Default::default()
        }
    }

    /// Same, turned by `yaw` degrees about +Y.
    pub fn at_yaw(position: Vec3, yaw: f32) -> Self {
        Transform {
            position,
            rotation: Rotation::euler(yaw, 0.0, 0.0),
            scale: Vec3::ONE,
        }
    }

    /// The object-to-world matrix.
    pub fn matrix(&self) -> Mat4 {
        Mat4::from_scale_rotation_translation(self.scale, self.rotation.to_quat(), self.position)
    }

    /// The unit forward (+Z) direction in world space.
    pub fn forward(&self) -> Vec3 {
        self.rotation.to_quat() * Vec3::Z
    }

    pub(crate) fn is_valid(&self) -> bool {
        self.position.is_finite()
            && self.rotation.is_finite()
            && self.scale.is_finite()
            && self.scale.min_element() > 0.0
    }
}

/// An axis-aligned bounding box.
#[derive(Debug, Clone, Copy, PartialEq, Serialize, Deserialize)]
pub struct Aabb {
    pub min: Vec3,
    pub max: Vec3,
}

impl Aabb {
    /// The empty box (union identity).
    pub const EMPTY: Aabb = Aabb {
        min: Vec3::splat(f32::INFINITY),
        max: Vec3::splat(f32::NEG_INFINITY),
    };

    pub fn is_empty(&self) -> bool {
        self.min.x > self.max.x || self.min.y > self.max.y || self.min.z > self.max.z
    }

    pub fn include(&mut self, p: Vec3) {
        self.min = self.min.min(p);
        self.max = self.max.max(p);
    }

    pub fn union(&self, other: &Aabb) -> Aabb {
        Aabb {
            min: self.min.min(other.min),
            max: self.max.max(other.max),
        }
    }

    pub fn from_points<'a>(points: impl IntoIterator<Item = &'a Vec3>) -> Aabb {
        let mut b = Aabb::EMPTY;
        for p in points {
            b.include(*p);
        }
        b
    }

    pub fn center(&self) -> Vec3 {
        (self.min + self.max) * 0.5
    }

    pub fn size(&self) -> Vec3 {
        if self.is_empty() {
            Vec3::ZERO
        } else {
            self.max - self.min
        }
    }

    /// The eight corners.
    pub fn corners(&self) -> [Vec3; 8] {
        let (a, b) = (self.min, self.max);
        [
            Vec3::new(a.x, a.y, a.z),
            Vec3::new(b.x, a.y, a.z),
            Vec3::new(a.x, b.y, a.z),
            Vec3::new(b.x, b.y, a.z),
            Vec3::new(a.x, a.y, b.z),
            Vec3::new(b.x, a.y, b.z),
            Vec3::new(a.x, b.y, b.z),
            Vec3::new(b.x, b.y, b.z),
        ]
    }

    /// The box of this box's corners after `m`.
    pub fn transformed(&self, m: &Mat4) -> Aabb {
        if self.is_empty() {
            return *self;
        }
        let pts = self.corners().map(|c| m.transform_point3(c));
        Aabb::from_points(pts.iter())
    }

    /// Ray/box slab test; returns the entry distance when hit.
    pub fn ray_hit(&self, origin: Vec3, dir: Vec3) -> Option<f32> {
        let inv = dir.recip();
        let t0 = (self.min - origin) * inv;
        let t1 = (self.max - origin) * inv;
        let tmin = t0.min(t1);
        let tmax = t0.max(t1);
        let near = tmin.x.max(tmin.y).max(tmin.z).max(0.0);
        let far = tmax.x.min(tmax.y).min(tmax.z);
        (near <= far).then_some(near)
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn yaw_pitch_roll_conventions() {
        let q = yaw_pitch_roll_quat(90.0, 0.0, 0.0);
        assert!(
            (q * Vec3::Z - Vec3::X).length() < 1e-5,
            "yaw turns +Z toward +X"
        );
        let q = yaw_pitch_roll_quat(0.0, 30.0, 0.0);
        assert!((q * Vec3::Z).y > 0.4, "pitch up raises the nose");
        let q = yaw_pitch_roll_quat(0.0, 0.0, 20.0);
        // Right is -X; positive roll leans the top toward the right.
        assert!((q * Vec3::Y).x < -0.3);
        let (y, p, r) = quat_to_yaw_pitch_roll(yaw_pitch_roll_quat(25.0, -12.0, 7.0));
        assert!((y - 25.0).abs() < 1e-3 && (p + 12.0).abs() < 1e-3 && (r - 7.0).abs() < 1e-3);
    }

    #[test]
    fn rotation_serde_both_forms() {
        let e: Rotation = serde_json::from_str(r#"{"yaw":10,"pitch":2,"roll":0}"#).unwrap();
        assert_eq!(e, Rotation::euler(10.0, 2.0, 0.0));
        let q: Rotation = serde_json::from_str(r#"{"quat":[0,0,0,1]}"#).unwrap();
        assert_eq!(q.to_quat(), Quat::IDENTITY);
    }

    #[test]
    fn aabb_ray() {
        let b = Aabb {
            min: Vec3::splat(-1.0),
            max: Vec3::splat(1.0),
        };
        assert!(b.ray_hit(Vec3::new(0.0, 0.0, -5.0), Vec3::Z).is_some());
        assert!(b.ray_hit(Vec3::new(3.0, 0.0, -5.0), Vec3::Z).is_none());
    }
}
