//! Triangle meshes and the procedural primitives that mannequins and props are
//! built from. Triangles wind counter-clockwise seen from outside.

use std::f32::consts::{PI, TAU};

use glam::{Mat3, Mat4, Quat, Vec3};
use serde::{Deserialize, Serialize};

use crate::math::Aabb;

/// An indexed triangle mesh with per-vertex normals.
#[derive(Debug, Clone, Default, PartialEq, Serialize, Deserialize)]
pub struct Mesh {
    pub positions: Vec<Vec3>,
    pub normals: Vec<Vec3>,
    pub indices: Vec<[u32; 3]>,
}

impl Mesh {
    pub fn triangle_count(&self) -> usize {
        self.indices.len()
    }

    pub fn bounds(&self) -> Aabb {
        Aabb::from_points(self.positions.iter())
    }

    /// Appends `other`, re-indexing its triangles.
    pub fn append(&mut self, other: &Mesh) {
        let base = self.positions.len() as u32;
        self.positions.extend_from_slice(&other.positions);
        self.normals.extend_from_slice(&other.normals);
        self.indices.extend(
            other
                .indices
                .iter()
                .map(|t| [t[0] + base, t[1] + base, t[2] + base]),
        );
    }

    /// Appends `other` transformed by `m`.
    pub fn append_transformed(&mut self, other: &Mesh, m: &Mat4) {
        let mut t = other.clone();
        t.transform(m);
        self.append(&t);
    }

    /// Transforms positions and normals in place (normals by the inverse
    /// transpose; a mirroring matrix flips the winding to stay outward).
    pub fn transform(&mut self, m: &Mat4) {
        let nm = Mat3::from_mat4(*m).inverse().transpose();
        for p in &mut self.positions {
            *p = m.transform_point3(*p);
        }
        for n in &mut self.normals {
            *n = (nm * *n).normalize_or_zero();
        }
        if Mat3::from_mat4(*m).determinant() < 0.0 {
            for t in &mut self.indices {
                t.swap(1, 2);
            }
        }
    }

    /// Returns a transformed copy.
    pub fn transformed(&self, m: &Mat4) -> Mesh {
        let mut t = self.clone();
        t.transform(m);
        t
    }

    /// Recomputes smooth (area-weighted) vertex normals.
    pub fn compute_normals(&mut self) {
        let mut acc = vec![Vec3::ZERO; self.positions.len()];
        for t in &self.indices {
            let [a, b, c] = t.map(|i| self.positions[i as usize]);
            let n = (b - a).cross(c - a);
            for &i in t {
                acc[i as usize] += n;
            }
        }
        self.normals = acc
            .into_iter()
            .map(|n| n.try_normalize().unwrap_or(Vec3::Y))
            .collect();
    }

    /// Drops triangles that reference missing vertices or repeat a vertex,
    /// and recomputes normals when they are missing or non-finite.
    pub(crate) fn sanitize(&mut self) {
        let n = self.positions.len() as u32;
        self.indices
            .retain(|t| t.iter().all(|&i| i < n) && t[0] != t[1] && t[1] != t[2] && t[0] != t[2]);
        if self.normals.len() != self.positions.len() || self.normals.iter().any(|v| !v.is_finite())
        {
            self.compute_normals();
        }
    }
}

/// Mesh tessellation density for curved primitives.
#[derive(Debug, Clone, Copy)]
pub(crate) struct Detail {
    pub segments: u32,
    pub rings: u32,
}

pub(crate) const DETAIL: Detail = Detail {
    segments: 12,
    rings: 4,
};

/// Box centred on the origin with flat faces.
pub fn cuboid(size: Vec3) -> Mesh {
    let h = size * 0.5;
    let mut m = Mesh::default();
    // (normal, u axis, v axis) chosen so u × v = normal (CCW from outside).
    let faces = [
        (Vec3::X, Vec3::NEG_Z, Vec3::Y),
        (Vec3::NEG_X, Vec3::Z, Vec3::Y),
        (Vec3::Y, Vec3::X, Vec3::NEG_Z),
        (Vec3::NEG_Y, Vec3::X, Vec3::Z),
        (Vec3::Z, Vec3::X, Vec3::Y),
        (Vec3::NEG_Z, Vec3::NEG_X, Vec3::Y),
    ];
    for (n, u, v) in faces {
        let base = m.positions.len() as u32;
        let c = n * h;
        let uu = u * h;
        let vv = v * h;
        for (su, sv) in [(-1.0, -1.0), (1.0, -1.0), (1.0, 1.0), (-1.0, 1.0)] {
            m.positions.push(c + uu * su + vv * sv);
            m.normals.push(n);
        }
        m.indices.push([base, base + 1, base + 2]);
        m.indices.push([base, base + 2, base + 3]);
    }
    m
}

/// Box whose base sits on y = 0, centred in x and z.
pub fn cuboid_on_ground(size: Vec3) -> Mesh {
    cuboid(size).transformed(&Mat4::from_translation(Vec3::new(0.0, size.y * 0.5, 0.0)))
}

/// Ellipsoid centred on the origin.
pub fn ellipsoid(radii: Vec3) -> Mesh {
    ellipsoid_detail(radii, DETAIL.segments, DETAIL.rings * 2)
}

/// Sphere centred on the origin.
pub fn sphere(radius: f32) -> Mesh {
    ellipsoid(Vec3::splat(radius))
}

fn ellipsoid_detail(radii: Vec3, segments: u32, rings: u32) -> Mesh {
    let mut m = Mesh::default();
    let rings = rings.max(2);
    for r in 0..=rings {
        let phi = PI * r as f32 / rings as f32; // 0 at top
        let (sp, cp) = phi.sin_cos();
        for s in 0..=segments {
            let th = TAU * s as f32 / segments as f32;
            let (st, ct) = th.sin_cos();
            let unit = Vec3::new(sp * st, cp, sp * ct);
            m.positions.push(unit * radii);
            m.normals.push((unit / radii).normalize_or_zero());
        }
    }
    grid_indices(&mut m, segments, rings);
    m
}

/// Indices for a (segments+1) × (rows+1) vertex grid whose rows go from +Y
/// down and whose columns go around +Y counter-clockwise seen from above.
fn grid_indices(m: &mut Mesh, segments: u32, rows: u32) {
    let w = segments + 1;
    for r in 0..rows {
        for s in 0..segments {
            let a = r * w + s;
            let b = a + 1;
            let c = a + w;
            let d = c + 1;
            m.indices.push([a, c, b]);
            m.indices.push([b, c, d]);
        }
    }
    m.indices.retain(|t| {
        let [a, b, c] = t.map(|i| m.positions[i as usize]);
        (b - a).cross(c - a).length_squared() > 1e-14
    });
}

/// A capsule with possibly different end radii (a "tapered capsule"),
/// running from the origin to `dir * length`. Used for limbs.
pub fn tapered_capsule(dir: Vec3, length: f32, r0: f32, r1: f32) -> Mesh {
    let seg = DETAIL.segments;
    let hemi = DETAIL.rings;
    let mut m = Mesh::default();
    // Profile: (y, radius, normal_y_component, normal_radial_component) from top (end) to bottom.
    let mut profile: Vec<(f32, f32, f32, f32)> = Vec::new();
    for i in 0..=hemi {
        let a = PI * 0.5 * (1.0 - i as f32 / hemi as f32); // 90° .. 0°
        profile.push((length + r1 * a.sin(), r1 * a.cos(), a.sin(), a.cos()));
    }
    for i in 0..=hemi {
        let a = -PI * 0.5 * (i as f32 / hemi as f32); // 0° .. -90°
        profile.push((r0 * a.sin(), r0 * a.cos(), a.sin(), a.cos()));
    }
    for &(y, r, ny, nr) in &profile {
        for s in 0..=seg {
            let th = TAU * s as f32 / seg as f32;
            let (st, ct) = th.sin_cos();
            m.positions.push(Vec3::new(r * st, y, r * ct));
            m.normals
                .push(Vec3::new(nr * st, ny, nr * ct).normalize_or_zero());
        }
    }
    grid_indices(&mut m, seg, profile.len() as u32 - 1);
    orient_y_to(&mut m, dir);
    m
}

/// Cylinder from y = 0 to `height` with flat caps.
pub fn cylinder(radius: f32, height: f32) -> Mesh {
    cone_frustum(radius, radius, height)
}

/// Truncated cone from y = 0 (radius `r0`) to `height` (radius `r1`), capped.
pub fn cone_frustum(r0: f32, r1: f32, height: f32) -> Mesh {
    let seg = DETAIL.segments + 4;
    let mut m = Mesh::default();
    let slope = (r0 - r1) / height.max(1e-6);
    for (y, r) in [(height, r1), (0.0, r0)] {
        for s in 0..=seg {
            let th = TAU * s as f32 / seg as f32;
            let (st, ct) = th.sin_cos();
            m.positions.push(Vec3::new(r * st, y, r * ct));
            m.normals.push(Vec3::new(st, slope, ct).normalize());
        }
    }
    grid_indices(&mut m, seg, 1);
    for (y, r, n) in [(height, r1, Vec3::Y), (0.0, r0, Vec3::NEG_Y)] {
        if r <= 1e-6 {
            continue;
        }
        let c = m.positions.len() as u32;
        m.positions.push(Vec3::new(0.0, y, 0.0));
        m.normals.push(n);
        for s in 0..=seg {
            let th = TAU * s as f32 / seg as f32;
            let (st, ct) = th.sin_cos();
            m.positions.push(Vec3::new(r * st, y, r * ct));
            m.normals.push(n);
        }
        for s in 0..seg {
            let a = c + 1 + s;
            if n.y > 0.0 {
                m.indices.push([c, a, a + 1]);
            } else {
                m.indices.push([c, a + 1, a]);
            }
        }
    }
    m
}

/// A one-sided quad in the XZ plane facing +Y, centred on the origin.
pub fn plane(width: f32, depth: f32) -> Mesh {
    let (w, d) = (width * 0.5, depth * 0.5);
    Mesh {
        positions: vec![
            Vec3::new(-w, 0.0, d),
            Vec3::new(w, 0.0, d),
            Vec3::new(w, 0.0, -d),
            Vec3::new(-w, 0.0, -d),
        ],
        normals: vec![Vec3::Y; 4],
        indices: vec![[0, 1, 2], [0, 2, 3]],
    }
}

/// Rotates a mesh built along +Y so that +Y points along `dir`.
fn orient_y_to(m: &mut Mesh, dir: Vec3) {
    let d = dir.normalize_or_zero();
    if d == Vec3::ZERO || (d - Vec3::Y).length_squared() < 1e-12 {
        return;
    }
    let q = if (d + Vec3::Y).length_squared() < 1e-12 {
        Quat::from_rotation_x(PI)
    } else {
        Quat::from_rotation_arc(Vec3::Y, d)
    };
    m.transform(&Mat4::from_quat(q));
}

#[cfg(test)]
mod tests {
    use super::*;

    fn outward(m: &Mesh) -> bool {
        let c = m.bounds().center();
        m.indices.iter().all(|t| {
            let [a, b, cc] = t.map(|i| m.positions[i as usize]);
            let n = (b - a).cross(cc - a);
            let mid = (a + b + cc) / 3.0;
            n.dot(mid - c) >= -1e-6
        })
    }

    #[test]
    fn primitives_wind_outward() {
        assert!(outward(&cuboid(Vec3::new(1.0, 2.0, 3.0))));
        assert!(outward(&sphere(1.0)));
        assert!(outward(&ellipsoid(Vec3::new(0.3, 0.5, 0.2))));
        assert!(outward(&cylinder(0.5, 2.0)));
        let cap = tapered_capsule(Vec3::NEG_Y, 1.0, 0.2, 0.1);
        assert!(outward(&cap));
        assert!(cap.bounds().min.y < -1.0);
    }

    #[test]
    fn mirror_transform_keeps_outward_winding() {
        let m = cuboid(Vec3::ONE).transformed(&Mat4::from_scale(Vec3::new(-1.0, 1.0, 1.0)));
        assert!(outward(&m));
    }
}
