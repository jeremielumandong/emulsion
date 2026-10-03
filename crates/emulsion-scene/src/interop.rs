//! Helpers for the Shot Generator UI and the storyboard core: one-call
//! rendering, projecting points (C12 layer attachment), ray picking (viewport
//! selection) and joint handles for posing gizmos.

use glam::{Mat4, Quat, Vec2, Vec3};

use crate::camera::{Camera, ScreenPoint};
use crate::error::SceneError;
use crate::import::AssetLibrary;
use crate::prepare::{PartLabel, PreparedScene, prepare};
use crate::render::{RenderOptions, RenderStyle, RgbaImage, render};
use crate::scene::{ObjectId, Scene};
use crate::skeleton::Bone;

/// Renders the scene through its own camera: the one-call entry point for
/// snapshots (C8) and thumbnails.
pub fn render_to_rgba(
    scene: &Scene,
    assets: &AssetLibrary,
    width: u32,
    height: u32,
    style: RenderStyle,
) -> Result<RgbaImage, SceneError> {
    let prepared = prepare(scene, assets)?;
    Ok(render(
        &prepared,
        &scene.camera,
        width,
        height,
        &RenderOptions::style(style),
    ))
}

/// Projects a world point into an image of `width × height` seen through
/// `camera`. `None` when the point is behind the camera.
pub fn project_point(camera: &Camera, width: u32, height: u32, point: Vec3) -> Option<ScreenPoint> {
    camera.view(width, height).project(point)
}

/// What a pixel hit.
#[derive(Debug, Clone, PartialEq)]
pub struct PickHit {
    pub object: ObjectId,
    pub part: PartLabel,
    /// World-space hit point.
    pub point: Vec3,
    /// World-space surface normal facing the camera.
    pub normal: Vec3,
    /// Distance from the ray origin.
    pub distance: f32,
}

impl PickHit {
    /// The mannequin bone that was hit, if any.
    pub fn bone(&self) -> Option<Bone> {
        match self.part {
            PartLabel::Bone(b) => Some(b),
            _ => None,
        }
    }
}

/// Casts a ray through pixel `(x, y)` (pixel coordinates, top-left origin)
/// and returns the nearest surface hit.
pub fn pick(
    prepared: &PreparedScene,
    camera: &Camera,
    width: u32,
    height: u32,
    x: f32,
    y: f32,
) -> Option<PickHit> {
    let view = camera.view(width, height);
    let (o, d) = view.ray(x, y);
    let mut best: Option<PickHit> = None;
    for m in &prepared.meshes {
        let Some(enter) = m.bounds.ray_hit(o, d) else {
            continue;
        };
        if best.as_ref().is_some_and(|b| enter > b.distance) {
            continue;
        }
        for (ti, t) in m.mesh.indices.iter().enumerate() {
            let [a, b, c] = t.map(|i| m.mesh.positions[i as usize]);
            let Some(dist) = ray_triangle(o, d, a, b, c) else {
                continue;
            };
            if dist < view.near || best.as_ref().is_some_and(|h| dist >= h.distance) {
                continue;
            }
            let mut n = (b - a).cross(c - a).normalize_or_zero();
            if n.dot(d) > 0.0 {
                n = -n;
            }
            let part = m
                .triangle_parts
                .get(ti)
                .and_then(|p| m.parts.get(*p as usize))
                .cloned()
                .unwrap_or(PartLabel::Whole);
            best = Some(PickHit {
                object: m.id,
                part,
                point: o + d * dist,
                normal: n,
                distance: dist,
            });
        }
    }
    best
}

/// Möller–Trumbore, two-sided.
fn ray_triangle(o: Vec3, d: Vec3, a: Vec3, b: Vec3, c: Vec3) -> Option<f32> {
    let e1 = b - a;
    let e2 = c - a;
    let p = d.cross(e2);
    let det = e1.dot(p);
    if det.abs() < 1e-12 {
        return None;
    }
    let inv = 1.0 / det;
    let s = o - a;
    let u = s.dot(p) * inv;
    if !(0.0..=1.0).contains(&u) {
        return None;
    }
    let q = s.cross(e1);
    let v = d.dot(q) * inv;
    if v < 0.0 || u + v > 1.0 {
        return None;
    }
    let t = e2.dot(q) * inv;
    (t > 0.0).then_some(t)
}

/// A posable joint drawn as a handle in the viewport.
#[derive(Debug, Clone, Copy, PartialEq)]
pub struct JointHandle {
    pub object: ObjectId,
    pub bone: Bone,
    pub world: Vec3,
    pub screen: ScreenPoint,
}

/// Screen positions of every character joint (for FK handles).
pub fn joint_handles(
    prepared: &PreparedScene,
    camera: &Camera,
    width: u32,
    height: u32,
) -> Vec<JointHandle> {
    let view = camera.view(width, height);
    let mut out = Vec::new();
    for c in &prepared.characters {
        for &b in Bone::ALL {
            let world = c.joint_world(b);
            if let Some(screen) = view.project(world) {
                out.push(JointHandle {
                    object: c.id,
                    bone: b,
                    world,
                    screen,
                });
            }
        }
    }
    out
}

/// The joint handle nearest to `(x, y)` within `radius` pixels, preferring
/// the one nearest the camera on ties.
pub fn pick_joint(
    prepared: &PreparedScene,
    camera: &Camera,
    width: u32,
    height: u32,
    x: f32,
    y: f32,
    radius: f32,
) -> Option<JointHandle> {
    let p = Vec2::new(x, y);
    joint_handles(prepared, camera, width, height)
        .into_iter()
        .filter(|h| (Vec2::new(h.screen.x, h.screen.y) - p).length() <= radius)
        .min_by(|a, b| {
            let da = (Vec2::new(a.screen.x, a.screen.y) - p).length();
            let db = (Vec2::new(b.screen.x, b.screen.y) - p).length();
            da.total_cmp(&db)
                .then(a.screen.depth.total_cmp(&b.screen.depth))
        })
}

/// World matrix of an attachment point: an object's origin, or a bone of a
/// character (C12: 2D layers parented to models follow this matrix).
pub fn attachment_matrix(
    scene: &Scene,
    prepared: &PreparedScene,
    object: ObjectId,
    bone: Option<Bone>,
) -> Option<Mat4> {
    let obj = scene.object(object)?;
    match (bone, prepared.character(object)) {
        (Some(b), Some(c)) => {
            let bp = c.bones[b.index()];
            Some(c.world * Mat4::from_rotation_translation(bp.rotation, bp.head))
        }
        _ => Some(obj.transform.matrix()),
    }
}

/// A plane on a model's surface for creating a 2D layer there (C12):
/// origin at the hit point, `normal` out of the surface, `right`/`up`
/// spanning the plane (up as close to world up as possible).
#[derive(Debug, Clone, Copy, PartialEq)]
pub struct SurfaceFrame {
    pub origin: Vec3,
    pub normal: Vec3,
    pub right: Vec3,
    pub up: Vec3,
}

impl SurfaceFrame {
    pub fn from_hit(hit: &PickHit) -> SurfaceFrame {
        let n = hit.normal;
        let ref_up = if n.y.abs() > 0.95 {
            Vec3::NEG_Z
        } else {
            Vec3::Y
        };
        let right = ref_up.cross(n).normalize_or_zero();
        let up = n.cross(right).normalize_or_zero();
        SurfaceFrame {
            origin: hit.point,
            normal: n,
            right,
            up,
        }
    }

    /// The frame as a matrix (X = right, Y = up, Z = normal).
    pub fn matrix(&self) -> Mat4 {
        let q = Quat::from_mat3(&glam::Mat3::from_cols(self.right, self.up, self.normal));
        Mat4::from_rotation_translation(q, self.origin)
    }
}
