//! Built-in parametric props and set pieces (SG4).
//!
//! Every built-in is described by its overall size (width along X, height
//! along Y, depth along Z, metres) plus a few kind-specific options. The
//! origin is the centre of the footprint on the ground; the front faces +Z
//! (a chair's seat faces +Z, a car drives toward +Z).

use glam::{Mat4, Quat, Vec3};
use serde::{Deserialize, Serialize};

use crate::mesh::{self, Mesh};

/// Built-in prop kinds.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum PropKind {
    Box,
    Cylinder,
    Sphere,
    Wall,
    Floor,
    Door,
    WindowFrame,
    Table,
    Chair,
    Bed,
    Sofa,
    Car,
    Tree,
    LampPost,
    Stairs,
}

impl PropKind {
    pub const ALL: [PropKind; 15] = [
        PropKind::Box,
        PropKind::Cylinder,
        PropKind::Sphere,
        PropKind::Wall,
        PropKind::Floor,
        PropKind::Door,
        PropKind::WindowFrame,
        PropKind::Table,
        PropKind::Chair,
        PropKind::Bed,
        PropKind::Sofa,
        PropKind::Car,
        PropKind::Tree,
        PropKind::LampPost,
        PropKind::Stairs,
    ];

    pub fn name(self) -> &'static str {
        match self {
            PropKind::Box => "box",
            PropKind::Cylinder => "cylinder",
            PropKind::Sphere => "sphere",
            PropKind::Wall => "wall",
            PropKind::Floor => "floor",
            PropKind::Door => "door",
            PropKind::WindowFrame => "window_frame",
            PropKind::Table => "table",
            PropKind::Chair => "chair",
            PropKind::Bed => "bed",
            PropKind::Sofa => "sofa",
            PropKind::Car => "car",
            PropKind::Tree => "tree",
            PropKind::LampPost => "lamp_post",
            PropKind::Stairs => "stairs",
        }
    }

    /// Typical real-world size (width, height, depth) in metres.
    pub fn default_size(self) -> Vec3 {
        let v = Vec3::new;
        match self {
            PropKind::Box => v(0.5, 0.5, 0.5),
            PropKind::Cylinder => v(0.5, 1.0, 0.5),
            PropKind::Sphere => v(0.5, 0.5, 0.5),
            PropKind::Wall => v(4.0, 2.6, 0.15),
            PropKind::Floor => v(6.0, 0.02, 6.0),
            PropKind::Door => v(0.9, 2.1, 0.12),
            PropKind::WindowFrame => v(1.2, 1.2, 0.1),
            PropKind::Table => v(1.4, 0.75, 0.8),
            PropKind::Chair => v(0.45, 0.9, 0.45),
            PropKind::Bed => v(1.6, 0.9, 2.1),
            PropKind::Sofa => v(2.0, 0.85, 0.9),
            PropKind::Car => v(1.8, 1.45, 4.5),
            PropKind::Tree => v(3.0, 6.0, 3.0),
            PropKind::LampPost => v(0.4, 4.5, 0.4),
            PropKind::Stairs => v(1.0, 1.44, 2.4),
        }
    }
}

/// A parametric built-in prop.
#[derive(Debug, Clone, Copy, PartialEq, Serialize, Deserialize)]
pub struct BuiltinProp {
    pub kind: PropKind,
    /// Overall size (width, height, depth); `None` uses the kind's default.
    #[serde(default)]
    pub size: Option<Vec3>,
    /// Stairs: number of steps (default from the height, ~18 cm risers).
    #[serde(default)]
    pub steps: Option<u32>,
    /// Door: opening angle in degrees (0 = closed).
    #[serde(default)]
    pub open: f32,
    /// Table: round top instead of rectangular.
    #[serde(default)]
    pub round: bool,
}

impl BuiltinProp {
    pub fn new(kind: PropKind) -> Self {
        BuiltinProp {
            kind,
            size: None,
            steps: None,
            open: 0.0,
            round: false,
        }
    }

    pub fn with_size(mut self, size: Vec3) -> Self {
        self.size = Some(size);
        self
    }

    /// The effective size.
    pub fn size(&self) -> Vec3 {
        self.size.unwrap_or(self.kind.default_size())
    }

    pub fn validate(&self) -> Result<(), String> {
        let s = self.size();
        if !s.is_finite() || s.min_element() <= 0.0 || s.max_element() > 1000.0 {
            return Err("size must be within 0..1000 m".into());
        }
        if self.steps.is_some_and(|n| n == 0 || n > 200) {
            return Err("steps must be 1..200".into());
        }
        if !self.open.is_finite() {
            return Err("open angle must be finite".into());
        }
        Ok(())
    }

    /// Generates the prop mesh in object space.
    pub fn mesh(&self) -> Mesh {
        let s = self.size();
        let s = if s.is_finite() {
            s.clamp(Vec3::splat(1e-3), Vec3::splat(1000.0))
        } else {
            self.kind.default_size()
        };
        let (w, h, d) = (s.x, s.y, s.z);
        let mut m = Mesh::default();
        let boxed = |m: &mut Mesh, size: Vec3, at: Vec3| {
            m.append_transformed(&mesh::cuboid(size), &Mat4::from_translation(at));
        };
        match self.kind {
            PropKind::Box | PropKind::Wall | PropKind::Floor => m = mesh::cuboid_on_ground(s),
            PropKind::Cylinder => {
                m = mesh::cylinder(0.5, h);
                m.transform(&Mat4::from_scale(Vec3::new(w, 1.0, d)));
            }
            PropKind::Sphere => {
                m = mesh::ellipsoid(s * 0.5);
                m.transform(&Mat4::from_translation(Vec3::new(0.0, h * 0.5, 0.0)));
            }
            PropKind::Door => {
                let jamb = (w * 0.08).min(0.08);
                let inner_w = (w - 2.0 * jamb).max(0.01);
                boxed(
                    &mut m,
                    Vec3::new(jamb, h, d),
                    Vec3::new(-(w - jamb) * 0.5, h * 0.5, 0.0),
                );
                boxed(
                    &mut m,
                    Vec3::new(jamb, h, d),
                    Vec3::new((w - jamb) * 0.5, h * 0.5, 0.0),
                );
                boxed(
                    &mut m,
                    Vec3::new(w, jamb, d),
                    Vec3::new(0.0, h - jamb * 0.5, 0.0),
                );
                let panel_h = (h - jamb).max(0.01);
                let t = (d * 0.4).min(0.045);
                let panel = mesh::cuboid(Vec3::new(inner_w, panel_h, t)).transformed(
                    &Mat4::from_translation(Vec3::new(inner_w * 0.5, panel_h * 0.5, 0.0)),
                );
                let hinge = Vec3::new(-inner_w * 0.5, 0.0, 0.0);
                let open = self.open.clamp(-180.0, 180.0).to_radians();
                m.append_transformed(
                    &panel,
                    &(Mat4::from_translation(hinge)
                        * Mat4::from_quat(Quat::from_rotation_y(-open))),
                );
                // Handle.
                let handle = mesh::sphere(t * 0.6).transformed(&Mat4::from_translation(Vec3::new(
                    inner_w * 0.88,
                    panel_h * 0.48,
                    t,
                )));
                m.append_transformed(
                    &handle,
                    &(Mat4::from_translation(hinge)
                        * Mat4::from_quat(Quat::from_rotation_y(-open))),
                );
            }
            PropKind::WindowFrame => {
                let f = (w.min(h) * 0.07).min(0.08);
                boxed(
                    &mut m,
                    Vec3::new(f, h, d),
                    Vec3::new(-(w - f) * 0.5, h * 0.5, 0.0),
                );
                boxed(
                    &mut m,
                    Vec3::new(f, h, d),
                    Vec3::new((w - f) * 0.5, h * 0.5, 0.0),
                );
                boxed(&mut m, Vec3::new(w, f, d), Vec3::new(0.0, f * 0.5, 0.0));
                boxed(&mut m, Vec3::new(w, f, d), Vec3::new(0.0, h - f * 0.5, 0.0));
                boxed(
                    &mut m,
                    Vec3::new(f * 0.5, h, d * 0.5),
                    Vec3::new(0.0, h * 0.5, 0.0),
                );
                boxed(
                    &mut m,
                    Vec3::new(w, f * 0.5, d * 0.5),
                    Vec3::new(0.0, h * 0.5, 0.0),
                );
            }
            PropKind::Table => {
                let top = (h * 0.06).min(0.05);
                if self.round {
                    let mut t = mesh::cylinder(0.5, top);
                    t.transform(
                        &(Mat4::from_translation(Vec3::new(0.0, h - top, 0.0))
                            * Mat4::from_scale(Vec3::new(w, 1.0, d))),
                    );
                    m.append(&t);
                    m.append(&mesh::cylinder(w.min(d) * 0.06, h - top));
                    let mut foot = mesh::cylinder(w.min(d) * 0.25, 0.03);
                    foot.transform(&Mat4::IDENTITY);
                    m.append(&foot);
                } else {
                    boxed(
                        &mut m,
                        Vec3::new(w, top, d),
                        Vec3::new(0.0, h - top * 0.5, 0.0),
                    );
                    let leg = (w.min(d) * 0.06).min(0.06);
                    for (sx, sz) in [(-1.0, -1.0), (1.0, -1.0), (-1.0, 1.0), (1.0, 1.0)] {
                        boxed(
                            &mut m,
                            Vec3::new(leg, h - top, leg),
                            Vec3::new(sx * (w * 0.5 - leg), (h - top) * 0.5, sz * (d * 0.5 - leg)),
                        );
                    }
                }
            }
            PropKind::Chair => {
                let seat_h = h * 0.5;
                let t = 0.04f32.min(seat_h * 0.2);
                boxed(
                    &mut m,
                    Vec3::new(w, t, d),
                    Vec3::new(0.0, seat_h - t * 0.5, 0.0),
                );
                let leg = (w.min(d) * 0.08).min(0.04);
                for (sx, sz) in [(-1.0, -1.0), (1.0, -1.0), (-1.0, 1.0), (1.0, 1.0)] {
                    boxed(
                        &mut m,
                        Vec3::new(leg, seat_h - t, leg),
                        Vec3::new(
                            sx * (w * 0.5 - leg * 0.5),
                            (seat_h - t) * 0.5,
                            sz * (d * 0.5 - leg * 0.5),
                        ),
                    );
                }
                let back_h = h - seat_h;
                boxed(
                    &mut m,
                    Vec3::new(w, back_h, t),
                    Vec3::new(0.0, seat_h + back_h * 0.5, -d * 0.5 + t * 0.5),
                );
            }
            PropKind::Bed => {
                let base = h * 0.35;
                boxed(
                    &mut m,
                    Vec3::new(w, base, d),
                    Vec3::new(0.0, base * 0.5, 0.0),
                );
                let matt = h * 0.18;
                boxed(
                    &mut m,
                    Vec3::new(w * 0.96, matt, d * 0.92),
                    Vec3::new(0.0, base + matt * 0.5, d * 0.03),
                );
                let pillow = mesh::ellipsoid(Vec3::new(w * 0.3, matt * 0.4, d * 0.07));
                m.append_transformed(
                    &pillow,
                    &Mat4::from_translation(Vec3::new(0.0, base + matt, -d * 0.36)),
                );
                boxed(
                    &mut m,
                    Vec3::new(w, h, 0.06),
                    Vec3::new(0.0, h * 0.5, -d * 0.5 + 0.03),
                );
            }
            PropKind::Sofa => {
                let seat = h * 0.5;
                let back_d = d * 0.22;
                let arm_w = w * 0.08;
                boxed(
                    &mut m,
                    Vec3::new(w, seat, d),
                    Vec3::new(0.0, seat * 0.5, 0.0),
                );
                boxed(
                    &mut m,
                    Vec3::new(w, h, back_d),
                    Vec3::new(0.0, h * 0.5, -(d - back_d) * 0.5),
                );
                for sx in [-1.0, 1.0] {
                    boxed(
                        &mut m,
                        Vec3::new(arm_w, seat * 1.35, d),
                        Vec3::new(sx * (w - arm_w) * 0.5, seat * 0.675, 0.0),
                    );
                }
            }
            PropKind::Car => {
                let wheel_r = (h * 0.22).min(d * 0.1);
                let body_h = h * 0.45;
                let body_y = wheel_r * 0.9;
                boxed(
                    &mut m,
                    Vec3::new(w, body_h, d),
                    Vec3::new(0.0, body_y + body_h * 0.5, 0.0),
                );
                let cabin_h = h - body_y - body_h;
                let cabin = mesh::cuboid(Vec3::new(w * 0.86, cabin_h, d * 0.48));
                m.append_transformed(
                    &taper_top(cabin, 0.82),
                    &Mat4::from_translation(Vec3::new(
                        0.0,
                        body_y + body_h + cabin_h * 0.5,
                        -d * 0.06,
                    )),
                );
                for (sx, sz) in [(-1.0, -1.0), (1.0, -1.0), (-1.0, 1.0), (1.0, 1.0)] {
                    let mut wheel = mesh::cylinder(wheel_r, w * 0.14);
                    wheel.transform(
                        &(Mat4::from_translation(Vec3::new(
                            sx * w * 0.5 + if sx > 0.0 { -w * 0.12 } else { -w * 0.02 },
                            wheel_r,
                            sz * d * 0.32,
                        )) * Mat4::from_quat(Quat::from_rotation_z(
                            -std::f32::consts::FRAC_PI_2,
                        ))),
                    );
                    m.append(&wheel);
                }
            }
            PropKind::Tree => {
                let trunk_h = h * 0.45;
                m.append(&mesh::cone_frustum(w * 0.06, w * 0.045, trunk_h + h * 0.1));
                let canopy = mesh::ellipsoid(Vec3::new(w * 0.5, h * 0.33, d * 0.5));
                m.append_transformed(
                    &canopy,
                    &Mat4::from_translation(Vec3::new(0.0, h * 0.67, 0.0)),
                );
            }
            PropKind::LampPost => {
                let r = (w.min(d) * 0.15).min(0.08);
                m.append(&mesh::cylinder(r * 1.6, 0.3_f32.min(h * 0.1)));
                m.append(&mesh::cylinder(r, h));
                let arm = w.max(0.3) * 0.8;
                boxed(
                    &mut m,
                    Vec3::new(r, r, arm),
                    Vec3::new(0.0, h - r * 0.5, arm * 0.5),
                );
                boxed(
                    &mut m,
                    Vec3::new(w * 0.6, h * 0.04, d * 0.6),
                    Vec3::new(0.0, h - r - h * 0.02, arm),
                );
            }
            PropKind::Stairs => {
                let n = self
                    .steps
                    .unwrap_or(((h / 0.18).round() as u32).clamp(1, 200))
                    .clamp(1, 200);
                let rise = h / n as f32;
                let run = d / n as f32;
                for i in 0..n {
                    let top = rise * (i + 1) as f32;
                    // Solid steps: each block reaches the ground, so the
                    // staircase reads as a stepped mass.
                    boxed(
                        &mut m,
                        Vec3::new(w, top, run),
                        Vec3::new(0.0, top * 0.5, -d * 0.5 + run * (i as f32 + 0.5)),
                    );
                }
            }
        }
        m
    }
}

/// Shrinks the top face of a box mesh (car cabin windscreen slope).
fn taper_top(mut m: Mesh, factor: f32) -> Mesh {
    for p in &mut m.positions {
        if p.y > 0.0 {
            p.z *= factor;
        }
    }
    m.compute_normals_flat_boxes();
    m
}

impl Mesh {
    /// Recomputes per-face normals for meshes whose faces have their own
    /// vertices (like [`mesh::cuboid`]).
    pub(crate) fn compute_normals_flat_boxes(&mut self) {
        for t in &self.indices {
            let [a, b, c] = t.map(|i| self.positions[i as usize]);
            let n = (b - a).cross(c - a).normalize_or_zero();
            for &i in t {
                self.normals[i as usize] = n;
            }
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn every_kind_builds_within_its_size() {
        for kind in PropKind::ALL {
            let p = BuiltinProp::new(kind);
            p.validate().unwrap();
            let m = p.mesh();
            assert!(m.triangle_count() > 0, "{kind:?}");
            let b = m.bounds();
            let s = p.size();
            assert!(b.min.y > -0.01, "{kind:?} below ground: {b:?}");
            assert!(b.max.y <= s.y + 0.02, "{kind:?} too tall: {b:?}");
            assert!(b.size().x <= s.x * 1.05 + 0.05, "{kind:?} too wide: {b:?}");
            assert!(m.positions.iter().all(|p| p.is_finite()));
        }
    }

    #[test]
    fn door_opens_around_hinge() {
        let mut d = BuiltinProp::new(PropKind::Door);
        let closed = d.mesh().bounds();
        d.open = 90.0;
        let open = d.mesh().bounds();
        assert!(open.max.z > closed.max.z + 0.5);
    }

    #[test]
    fn bad_sizes_are_rejected() {
        let p = BuiltinProp::new(PropKind::Box).with_size(Vec3::new(1.0, -1.0, 1.0));
        assert!(p.validate().is_err());
        let p = BuiltinProp::new(PropKind::Box).with_size(Vec3::new(f32::NAN, 1.0, 1.0));
        assert!(p.validate().is_err());
        assert!(p.mesh().positions.iter().all(|v| v.is_finite()));
    }
}
