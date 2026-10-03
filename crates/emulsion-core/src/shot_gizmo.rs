//! On-screen transform gizmos for the Shot Generator's viewport (phase 11):
//! the handle geometry, hit-testing and drag maths, kept free of UI so they
//! can be tested exactly.
//!
//! A [`Gizmo`] sits on an object's origin and is sized to a constant length
//! on screen ([`AXIS_PX`]). **Move** shows an arrow per axis and a square per
//! pair of axes (drag in that plane); **Rotate** shows a ring per axis;
//! **Scale** shows a line per axis and a square in the middle that scales
//! all three together. Move and Rotate use the object's own axes or the
//! world's; Scale always uses the object's own. A [`GizmoDrag`] turns a
//! pointer position into the object's new transform, from where it was
//! when the drag started (so a drag never accumulates rounding), snapped
//! with [`MOVE_SNAP`], [`ROTATE_SNAP`] and [`SCALE_SNAP`] on request.
use emulsion_scene::{Rotation, Transform, View};
use glam::{Quat, Vec2, Vec3};

/// What the gizmo changes.
#[derive(Clone, Copy, Debug, PartialEq, Eq, Hash)]
pub enum GizmoMode {
    Move,
    Rotate,
    Scale,
}

/// One part of a gizmo. Axes are numbered 0 = X, 1 = Y, 2 = Z.
#[derive(Clone, Copy, Debug, PartialEq, Eq, Hash)]
pub enum Handle {
    /// Move or scale along one axis.
    Axis(usize),
    /// Move in the plane across one axis (the axis is its normal).
    Plane(usize),
    /// Turn about one axis.
    Ring(usize),
    /// Scale along every axis at once.
    Uniform,
}

impl Handle {
    /// The axis it draws in the colour of, if any.
    pub fn axis(self) -> Option<usize> {
        match self {
            Handle::Axis(a) | Handle::Plane(a) | Handle::Ring(a) => Some(a),
            Handle::Uniform => None,
        }
    }
}

/// A handle's length on screen, in pixels.
pub const AXIS_PX: f32 = 80.;
/// How close (pixels) the pointer must be to a handle to grab it.
pub const HIT_PX: f32 = 7.;
/// Half the uniform-scale square, in pixels.
const UNIFORM_PX: f32 = 7.;
/// Plane squares span this share of the axis length.
const PLANE_FROM: f32 = 0.25;
const PLANE_TO: f32 = 0.45;
/// Rings are this share of the axis length across (radius).
const RING: f32 = 0.9;
const RING_SEGMENTS: usize = 64;
/// Snap steps with Shift: metres, degrees and a share of the start scale.
pub const MOVE_SNAP: f32 = 0.1;
pub const ROTATE_SNAP: f32 = 15.;
pub const SCALE_SNAP: f32 = 0.1;
/// Smallest scale factor a drag reaches.
const MIN_FACTOR: f32 = 0.01;

/// A handle as drawn: a polyline in picture pixels.
#[derive(Clone, Debug, PartialEq)]
pub struct HandleShape {
    pub handle: Handle,
    pub points: Vec<Vec2>,
    /// The last point joins the first.
    pub closed: bool,
    /// A filled shape (plane squares, the uniform square); grabbed inside.
    pub filled: bool,
}

/// A gizmo on one object, seen through one view.
#[derive(Clone, Copy, Debug, PartialEq)]
pub struct Gizmo {
    pub mode: GizmoMode,
    pub origin: Vec3,
    /// Unit axes in the world: the object's own or the world's.
    pub axes: [Vec3; 3],
    /// Axis length in metres, so it shows [`AXIS_PX`] long.
    pub size: f32,
}

fn screen(view: &View, p: Vec3) -> Option<Vec2> {
    view.project(p).map(|s| Vec2::new(s.x, s.y))
}

/// Distance from `p` to the segment `a`–`b`.
fn segment_distance(p: Vec2, a: Vec2, b: Vec2) -> f32 {
    let ab = b - a;
    let t = if ab.length_squared() < 1e-9 {
        0.
    } else {
        ((p - a).dot(ab) / ab.length_squared()).clamp(0., 1.)
    };
    (a + ab * t - p).length()
}

/// Whether `p` lies inside the polygon `points`.
fn inside(p: Vec2, points: &[Vec2]) -> bool {
    let mut odd = false;
    let mut j = points.len().wrapping_sub(1);
    for i in 0..points.len() {
        let (a, b) = (points[i], points[j]);
        if (a.y > p.y) != (b.y > p.y) && p.x < (b.x - a.x) * (p.y - a.y) / (b.y - a.y) + a.x {
            odd = !odd;
        }
        j = i;
    }
    odd
}

fn snap(value: f32, step: f32) -> f32 {
    (value / step).round() * step
}

impl HandleShape {
    /// How far `p` is from the shape (0 inside a filled one).
    pub fn distance(&self, p: Vec2) -> f32 {
        if self.filled && self.points.len() > 2 && inside(p, &self.points) {
            return 0.;
        }
        let n = self.points.len();
        let segments = if self.closed { n } else { n.saturating_sub(1) };
        (0..segments)
            .map(|i| segment_distance(p, self.points[i], self.points[(i + 1) % n]))
            .fold(f32::INFINITY, f32::min)
    }
}

impl Gizmo {
    /// The gizmo for an object at `t`, with its own axes when `local` (Scale
    /// always uses them). `None` when the object is behind the camera.
    pub fn new(mode: GizmoMode, t: &Transform, local: bool, view: &View) -> Option<Self> {
        let seen = view.project(t.position)?;
        let size = view.pixel_size_at(seen.depth) * AXIS_PX;
        let axes = if local || mode == GizmoMode::Scale {
            let q = t.rotation.to_quat();
            [q * Vec3::X, q * Vec3::Y, q * Vec3::Z]
        } else {
            [Vec3::X, Vec3::Y, Vec3::Z]
        };
        (size.is_finite() && size > 0.).then_some(Self {
            mode,
            origin: t.position,
            axes,
            size,
        })
    }

    /// The two axes spanning the plane across axis `a`.
    fn others(&self, a: usize) -> (Vec3, Vec3) {
        (self.axes[(a + 1) % 3], self.axes[(a + 2) % 3])
    }

    /// Every handle as drawn, in the order they are grabbed (first wins
    /// ties): the filled squares, then axes, then rings.
    pub fn shapes(&self, view: &View) -> Vec<HandleShape> {
        let o = self.origin;
        let s = self.size;
        let mut out = Vec::new();
        let line = |handle, to: Vec3| {
            Some(HandleShape {
                handle,
                points: vec![screen(view, o)?, screen(view, to)?],
                closed: false,
                filled: false,
            })
        };
        match self.mode {
            GizmoMode::Move => {
                for a in 0..3 {
                    let (u, v) = self.others(a);
                    let corners = [
                        (PLANE_FROM, PLANE_FROM),
                        (PLANE_TO, PLANE_FROM),
                        (PLANE_TO, PLANE_TO),
                        (PLANE_FROM, PLANE_TO),
                    ];
                    let points: Option<Vec<Vec2>> = corners
                        .iter()
                        .map(|(x, y)| screen(view, o + (u * *x + v * *y) * s))
                        .collect();
                    if let Some(points) = points {
                        out.push(HandleShape {
                            handle: Handle::Plane(a),
                            points,
                            closed: true,
                            filled: true,
                        });
                    }
                }
                out.extend((0..3).filter_map(|a| line(Handle::Axis(a), o + self.axes[a] * s)));
            }
            GizmoMode::Scale => {
                if let Some(c) = screen(view, o) {
                    let d = UNIFORM_PX;
                    out.push(HandleShape {
                        handle: Handle::Uniform,
                        points: vec![
                            c + Vec2::new(-d, -d),
                            c + Vec2::new(d, -d),
                            c + Vec2::new(d, d),
                            c + Vec2::new(-d, d),
                        ],
                        closed: true,
                        filled: true,
                    });
                }
                out.extend((0..3).filter_map(|a| line(Handle::Axis(a), o + self.axes[a] * s)));
            }
            GizmoMode::Rotate => {
                for a in 0..3 {
                    let points: Vec<Vec2> = self
                        .ring_points(a)
                        .into_iter()
                        .filter_map(|p| screen(view, p))
                        .collect();
                    if points.len() > 2 {
                        out.push(HandleShape {
                            handle: Handle::Ring(a),
                            points,
                            closed: true,
                            filled: false,
                        });
                    }
                }
            }
        }
        out
    }

    /// Points around ring `a` in the world.
    fn ring_points(&self, a: usize) -> Vec<Vec3> {
        let (u, v) = self.others(a);
        let r = self.size * RING;
        (0..RING_SEGMENTS)
            .map(|i| {
                let t = i as f32 / RING_SEGMENTS as f32 * std::f32::consts::TAU;
                self.origin + (u * t.cos() + v * t.sin()) * r
            })
            .collect()
    }

    /// The handle under picture point `p`, if one is within [`HIT_PX`].
    pub fn hit(&self, view: &View, p: Vec2) -> Option<Handle> {
        let mut best: Option<(f32, Handle)> = None;
        for shape in self.shapes(view) {
            let d = shape.distance(p);
            if d <= HIT_PX && best.is_none_or(|(b, _)| d < b) {
                best = Some((d, shape.handle));
            }
        }
        best.map(|(_, h)| h)
    }
}

/// A gizmo drag in progress: where it started and from what.
#[derive(Clone, Copy, Debug, PartialEq)]
pub struct GizmoDrag {
    pub gizmo: Gizmo,
    pub handle: Handle,
    pub from: Transform,
    /// Where the pointer started, in picture pixels.
    pub start: Vec2,
    /// Move: where the drag started on the axis (metres) or the plane.
    grab: Vec3,
    /// Rotate: the ring's direction on screen where it was grabbed.
    tangent: Vec2,
}

/// The parameter along the line `origin + axis·t` closest to a ray.
fn axis_param(origin: Vec3, axis: Vec3, (ro, rd): (Vec3, Vec3)) -> Option<f32> {
    let w = origin - ro;
    let b = axis.dot(rd);
    let denom = 1. - b * b;
    (denom > 1e-4).then(|| (b * rd.dot(w) - axis.dot(w)) / denom)
}

/// Where a ray meets the plane through `point` across `normal`.
fn plane_hit(point: Vec3, normal: Vec3, (ro, rd): (Vec3, Vec3)) -> Option<Vec3> {
    let d = rd.dot(normal);
    if d.abs() < 1e-5 {
        return None;
    }
    let t = (point - ro).dot(normal) / d;
    (t.is_finite() && t > 0.).then(|| ro + rd * t)
}

impl GizmoDrag {
    /// Start dragging `handle` of `gizmo` (on an object at `from`) at
    /// picture point `at`. `None` when the handle cannot be dragged from
    /// this view (an axis pointing straight at the camera).
    pub fn begin(
        gizmo: Gizmo,
        handle: Handle,
        from: Transform,
        view: &View,
        at: Vec2,
    ) -> Option<Self> {
        let ray = view.ray(at.x, at.y);
        let (grab, tangent) = match handle {
            Handle::Axis(a) if gizmo.mode == GizmoMode::Move => {
                let t = axis_param(gizmo.origin, gizmo.axes[a], ray)?;
                (Vec3::splat(t), Vec2::ZERO)
            }
            Handle::Plane(a) => (plane_hit(gizmo.origin, gizmo.axes[a], ray)?, Vec2::ZERO),
            Handle::Ring(a) => {
                // The ring's direction on screen at the point grabbed.
                let near = gizmo
                    .ring_points(a)
                    .into_iter()
                    .filter_map(|p| Some((p, screen(view, p)?)))
                    .min_by(|x, y| (x.1 - at).length().total_cmp(&(y.1 - at).length()))?;
                let along = gizmo.axes[a]
                    .cross(near.0 - gizmo.origin)
                    .normalize_or_zero();
                let ahead = screen(view, near.0 + along * gizmo.size * 0.05)?;
                let mut tangent = (ahead - near.1).normalize_or_zero();
                if tangent == Vec2::ZERO {
                    let centre = screen(view, gizmo.origin)?;
                    let out = (at - centre).normalize_or_zero();
                    tangent = Vec2::new(-out.y, out.x);
                }
                (Vec3::ZERO, tangent)
            }
            _ => (Vec3::ZERO, Vec2::ZERO),
        };
        Some(Self {
            gizmo,
            handle,
            from,
            start: at,
            grab,
            tangent,
        })
    }

    /// The object's transform with the pointer at picture point `at`;
    /// with `snap`, in whole steps.
    pub fn update(&self, view: &View, at: Vec2, snap_on: bool) -> Transform {
        let g = &self.gizmo;
        let mut t = self.from;
        let ray = view.ray(at.x, at.y);
        let moved = at - self.start;
        match (g.mode, self.handle) {
            (GizmoMode::Move, Handle::Axis(a)) => {
                if let Some(now) = axis_param(g.origin, g.axes[a], ray) {
                    let mut d = now - self.grab.x;
                    if snap_on {
                        d = snap(d, MOVE_SNAP);
                    }
                    t.position = self.from.position + g.axes[a] * d;
                }
            }
            (GizmoMode::Move, Handle::Plane(a)) => {
                if let Some(hit) = plane_hit(g.origin, g.axes[a], ray) {
                    let delta = hit - self.grab;
                    let (u, v) = g.others(a);
                    let (mut du, mut dv) = (delta.dot(u), delta.dot(v));
                    if snap_on {
                        (du, dv) = (snap(du, MOVE_SNAP), snap(dv, MOVE_SNAP));
                    }
                    t.position = self.from.position + u * du + v * dv;
                }
            }
            (GizmoMode::Rotate, Handle::Ring(a)) => {
                let mut degrees = (moved.dot(self.tangent) / (AXIS_PX * RING)).to_degrees();
                if snap_on {
                    degrees = snap(degrees, ROTATE_SNAP);
                }
                let q = Quat::from_axis_angle(g.axes[a], degrees.to_radians())
                    * self.from.rotation.to_quat();
                t.rotation = match self.from.rotation {
                    Rotation::Euler { .. } => {
                        let (y, p, r) = Rotation::Quaternion { quat: q }.to_yaw_pitch_roll();
                        Rotation::euler(y, p, r)
                    }
                    Rotation::Quaternion { .. } => Rotation::Quaternion { quat: q },
                };
            }
            (GizmoMode::Scale, Handle::Axis(a)) => {
                let (Some(o), Some(tip)) = (
                    screen(view, g.origin),
                    screen(view, g.origin + g.axes[a] * g.size),
                ) else {
                    return t;
                };
                let along = tip - o;
                if along.length() < 1. {
                    return t;
                }
                let factor = scale_factor(moved.dot(along) / along.length_squared(), snap_on);
                t.scale[a] = self.from.scale[a] * factor;
            }
            (GizmoMode::Scale, Handle::Uniform) => {
                let factor = scale_factor((moved.x - moved.y) / AXIS_PX, snap_on);
                t.scale = self.from.scale * factor;
            }
            _ => {}
        }
        t
    }
}

/// A scale factor from a drag of `amount` (1 = one handle length).
fn scale_factor(amount: f32, snap_on: bool) -> f32 {
    let factor = 1. + amount;
    let factor = if snap_on {
        snap(factor, SCALE_SNAP).max(SCALE_SNAP)
    } else {
        factor
    };
    factor.max(MIN_FACTOR)
}

#[cfg(test)]
mod tests {
    use super::*;
    use emulsion_scene::Camera;

    /// A camera 5 m in front of the origin, looking at it (down -Z).
    fn view() -> View {
        let camera = Camera {
            position: Vec3::new(0., 1., 5.),
            yaw: 180.,
            pitch: 0.,
            ..Camera::default()
        };
        camera.view(800, 450)
    }

    fn at_origin() -> Transform {
        Transform::at(Vec3::new(0., 1., 0.))
    }

    fn tip(g: &Gizmo, view: &View, a: usize) -> Vec2 {
        screen(view, g.origin + g.axes[a] * g.size).unwrap()
    }

    #[test]
    fn handles_are_a_constant_size_on_screen() {
        let v = view();
        let near = Gizmo::new(GizmoMode::Move, &at_origin(), false, &v).unwrap();
        let far_t = Transform::at(Vec3::new(0., 1., -10.));
        let far = Gizmo::new(GizmoMode::Move, &far_t, false, &v).unwrap();
        for g in [near, far] {
            let o = screen(&v, g.origin).unwrap();
            let len = (tip(&g, &v, 1) - o).length();
            assert!((len - AXIS_PX).abs() < 0.5, "{len}");
        }
        assert!(far.size > near.size * 2.9);
        // Behind the camera there is no gizmo.
        let behind = Transform::at(Vec3::new(0., 1., 9.));
        assert!(Gizmo::new(GizmoMode::Move, &behind, false, &v).is_none());
    }

    #[test]
    fn hit_testing_finds_axes_planes_rings_and_the_uniform_square() {
        let v = view();
        let t = at_origin();
        let g = Gizmo::new(GizmoMode::Move, &t, false, &v).unwrap();
        let o = screen(&v, g.origin).unwrap();
        // Along the X arrow (screen right), near its tip.
        let x = tip(&g, &v, 0);
        assert_eq!(g.hit(&v, o.lerp(x, 0.8)), Some(Handle::Axis(0)));
        assert_eq!(
            g.hit(&v, o.lerp(x, 0.8) + Vec2::new(0., 4.)),
            Some(Handle::Axis(0))
        );
        // The XY square (across Z) sits up and to the right.
        let y = tip(&g, &v, 1);
        let mid = o + (x - o) * 0.35 + (y - o) * 0.35;
        assert_eq!(g.hit(&v, mid), Some(Handle::Plane(2)));
        // Nothing far away.
        assert_eq!(g.hit(&v, o + Vec2::new(-200., 150.)), None);
        // Rotate: the Z ring faces the camera as a circle around the centre
        // (the X and Y rings are seen edge-on, as lines through it).
        let r = Gizmo::new(GizmoMode::Rotate, &t, false, &v).unwrap();
        let on_ring = o + Vec2::new(1., -1.) * (AXIS_PX * RING * std::f32::consts::FRAC_1_SQRT_2);
        assert_eq!(r.hit(&v, on_ring), Some(Handle::Ring(2)));
        // Scale: the middle square scales uniformly, axes scale one axis.
        let s = Gizmo::new(GizmoMode::Scale, &t, false, &v).unwrap();
        assert_eq!(s.hit(&v, o + Vec2::new(2., 3.)), Some(Handle::Uniform));
        assert_eq!(s.hit(&v, o.lerp(y, 0.9)), Some(Handle::Axis(1)));
    }

    #[test]
    fn moving_along_an_axis_follows_the_pointer_and_snaps() {
        let v = view();
        let t = at_origin();
        let g = Gizmo::new(GizmoMode::Move, &t, false, &v).unwrap();
        let o = screen(&v, g.origin).unwrap();
        let x = tip(&g, &v, 0);
        let start = o.lerp(x, 0.8);
        let drag = GizmoDrag::begin(g, Handle::Axis(0), t, &v, start).unwrap();
        // Dragging one handle length right (and some down, off the axis)
        // moves one handle length along X only.
        let moved = drag.update(&v, start + (x - o) + Vec2::new(0., 30.), false);
        let d = moved.position - t.position;
        assert!(
            (d.x - g.size).abs() < 1e-3 && d.y.abs() < 1e-6 && d.z.abs() < 1e-6,
            "{d}"
        );
        // With snapping, whole 10 cm steps.
        let snapped = drag.update(&v, start + (x - o) * 0.37, true);
        let dx = snapped.position.x - t.position.x;
        assert!(
            (dx / MOVE_SNAP - (dx / MOVE_SNAP).round()).abs() < 1e-4,
            "{dx}"
        );
        assert!(dx > 0.);
        // Dragging back to the start puts it back exactly.
        assert_eq!(drag.update(&v, start, false), t);
    }

    #[test]
    fn plane_moves_stay_in_the_plane_and_local_axes_follow_the_object() {
        let v = view();
        let t = at_origin();
        let g = Gizmo::new(GizmoMode::Move, &t, false, &v).unwrap();
        let o = screen(&v, g.origin).unwrap();
        let start = o + Vec2::new(25., -25.);
        let drag = GizmoDrag::begin(g, Handle::Plane(2), t, &v, start).unwrap();
        let moved = drag.update(&v, start + Vec2::new(40., -20.), false);
        let d = moved.position - t.position;
        assert!(d.z.abs() < 1e-5 && d.x > 0. && d.y > 0., "{d}");
        // A turned object's local X axis turns with it.
        let mut turned = t;
        turned.rotation = Rotation::euler(90., 0., 0.);
        let local = Gizmo::new(GizmoMode::Move, &turned, true, &v).unwrap();
        assert!(
            local.axes[0].abs_diff_eq(-Vec3::Z, 1e-5),
            "{}",
            local.axes[0]
        );
        let world = Gizmo::new(GizmoMode::Move, &turned, false, &v).unwrap();
        assert_eq!(world.axes[0], Vec3::X);
    }

    #[test]
    fn rings_turn_about_their_axis_and_snap_to_15_degrees() {
        let v = view();
        let t = at_origin();
        let g = Gizmo::new(GizmoMode::Rotate, &t, false, &v).unwrap();
        let o = screen(&v, g.origin).unwrap();
        // The Y ring is seen edge-on from the front; grab it at the right.
        let start = o + Vec2::new(AXIS_PX * RING, 0.);
        let drag = GizmoDrag::begin(g, Handle::Ring(1), t, &v, start).unwrap();
        let turned = drag.update(&v, start + Vec2::new(30., 0.), false);
        let (yaw, pitch, roll) = turned.rotation.to_yaw_pitch_roll();
        assert!(yaw.abs() > 5. && pitch.abs() < 1e-3 && roll.abs() < 1e-3);
        let snapped = drag.update(&v, start + Vec2::new(30., 0.), true);
        let (yaw, _, _) = snapped.rotation.to_yaw_pitch_roll();
        assert!(
            (yaw / ROTATE_SNAP - (yaw / ROTATE_SNAP).round()).abs() < 1e-3,
            "{yaw}"
        );
        // The Z ring faces the camera: dragging along it turns about Z.
        let top = o + Vec2::new(0., -AXIS_PX * RING);
        let drag = GizmoDrag::begin(g, Handle::Ring(2), t, &v, top).unwrap();
        let rolled = drag.update(&v, top + Vec2::new(20., 0.), false);
        let (yaw, pitch, roll) = rolled.rotation.to_yaw_pitch_roll();
        assert!(roll.abs() > 5. && yaw.abs() < 1e-3 && pitch.abs() < 1e-3);
    }

    #[test]
    fn scaling_one_axis_or_all_and_snapping_to_tenths() {
        let v = view();
        let t = at_origin();
        let g = Gizmo::new(GizmoMode::Scale, &t, false, &v).unwrap();
        let o = screen(&v, g.origin).unwrap();
        let y = tip(&g, &v, 1);
        let drag = GizmoDrag::begin(g, Handle::Axis(1), t, &v, y).unwrap();
        // Pulling the Y handle out by its own length doubles the height.
        let tall = drag.update(&v, y + (y - o), false);
        assert!((tall.scale.y - 2.).abs() < 1e-3 && tall.scale.x == 1. && tall.scale.z == 1.);
        let snapped = drag.update(&v, y + (y - o) * 0.33, true);
        assert!((snapped.scale.y - 1.3).abs() < 1e-5, "{}", snapped.scale.y);
        // It never reaches zero.
        let flat = drag.update(&v, o - (y - o) * 3., false);
        assert!(flat.scale.y > 0.);
        let drag = GizmoDrag::begin(g, Handle::Uniform, t, &v, o).unwrap();
        let big = drag.update(&v, o + Vec2::new(AXIS_PX / 2., 0.), false);
        assert!(
            big.scale.abs_diff_eq(Vec3::splat(1.5), 1e-4),
            "{}",
            big.scale
        );
    }
}
