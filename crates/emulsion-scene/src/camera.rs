//! Cameras and lens maths (SG5, C6, V6).
//!
//! A camera has a position and a yaw/pitch/roll orientation (same convention
//! as objects: yaw 0 looks along +Z), a focal length in millimetres and a film
//! back (sensor size). The film back's **width** sets the horizontal field of
//! view; the image's aspect ratio sets the vertical extent ("horizontal fit"),
//! so a 35 mm lens frames the same width at any board size.

use glam::{Quat, Vec2, Vec3};
use serde::{Deserialize, Serialize};

use crate::math::{Aabb, quat_to_yaw_pitch_roll, yaw_pitch_roll_quat};

/// Sensor (film back) size in millimetres.
#[derive(Debug, Clone, Copy, PartialEq, Serialize, Deserialize)]
pub struct FilmBack {
    pub width_mm: f32,
    pub height_mm: f32,
}

impl FilmBack {
    /// Super 35 (the default; 24.89 × 18.66 mm).
    pub const SUPER_35: FilmBack = FilmBack {
        width_mm: 24.89,
        height_mm: 18.66,
    };
    /// Full frame 35 mm still (36 × 24 mm).
    pub const FULL_FRAME: FilmBack = FilmBack {
        width_mm: 36.0,
        height_mm: 24.0,
    };
    /// Super 16 (12.52 × 7.41 mm).
    pub const SUPER_16: FilmBack = FilmBack {
        width_mm: 12.52,
        height_mm: 7.41,
    };

    /// A Super 35-wide film back with the given aspect ratio (width / height),
    /// e.g. the board's aspect.
    pub fn super35_for_aspect(aspect: f32) -> FilmBack {
        let a = if aspect.is_finite() && aspect > 0.05 {
            aspect
        } else {
            16.0 / 9.0
        };
        FilmBack {
            width_mm: 24.89,
            height_mm: 24.89 / a,
        }
    }
}

impl Default for FilmBack {
    fn default() -> Self {
        FilmBack::SUPER_35
    }
}

/// Perspective or orthographic projection.
#[derive(Debug, Clone, Copy, Default, PartialEq, Serialize, Deserialize)]
#[serde(tag = "type", rename_all = "snake_case")]
pub enum Projection {
    #[default]
    Perspective,
    /// Parallel projection showing `height` metres vertically (top/side views).
    Orthographic { height: f32 },
}

/// Common prime lenses (mm), for UI pickers and the Shot Explorer.
pub const PRIME_LENSES: [f32; 9] = [14.0, 18.0, 24.0, 35.0, 50.0, 65.0, 85.0, 100.0, 135.0];

/// Focal length range accepted by validation (mm).
pub const FOCAL_RANGE: (f32, f32) = (4.0, 1200.0);

/// A scene camera.
#[derive(Debug, Clone, Copy, PartialEq, Serialize, Deserialize)]
#[serde(default)]
pub struct Camera {
    pub position: Vec3,
    /// Degrees; 0 looks along +Z, positive turns toward +X.
    pub yaw: f32,
    /// Degrees; positive tilts up.
    pub pitch: f32,
    /// Degrees; positive rolls the top of the frame toward the right (dutch).
    pub roll: f32,
    pub focal_length_mm: f32,
    pub film_back: FilmBack,
    pub projection: Projection,
    /// Near clip distance in metres.
    pub near: f32,
    /// Far distance in metres (the ground and grid fade out by here).
    pub far: f32,
}

impl Default for Camera {
    fn default() -> Self {
        let mut c = Camera {
            position: Vec3::new(0.0, 1.6, 5.0),
            yaw: 0.0,
            pitch: 0.0,
            roll: 0.0,
            focal_length_mm: 35.0,
            film_back: FilmBack::default(),
            projection: Projection::Perspective,
            near: 0.05,
            far: 500.0,
        };
        c.look_at(Vec3::new(0.0, 1.0, 0.0));
        c
    }
}

/// Horizontal field of view (degrees) of a focal length on a film-back width.
pub fn horizontal_fov_deg(focal_length_mm: f32, film_width_mm: f32) -> f32 {
    2.0 * (film_width_mm / (2.0 * focal_length_mm))
        .atan()
        .to_degrees()
}

/// Focal length (mm) giving a horizontal field of view on a film-back width.
pub fn focal_length_for_fov(hfov_deg: f32, film_width_mm: f32) -> f32 {
    film_width_mm / (2.0 * (hfov_deg.to_radians() * 0.5).tan())
}

impl Camera {
    /// Orientation quaternion (local +Z = view direction).
    pub fn rotation(&self) -> Quat {
        yaw_pitch_roll_quat(self.yaw, self.pitch, self.roll)
    }

    /// Unit view direction.
    pub fn forward(&self) -> Vec3 {
        self.rotation() * Vec3::Z
    }

    /// Unit up direction of the frame.
    pub fn up(&self) -> Vec3 {
        self.rotation() * Vec3::Y
    }

    /// Unit right direction of the frame.
    pub fn right(&self) -> Vec3 {
        self.rotation() * Vec3::NEG_X
    }

    /// Aims the camera at `target`, keeping roll.
    pub fn look_at(&mut self, target: Vec3) {
        let d = target - self.position;
        if d.length_squared() < 1e-12 || !d.is_finite() {
            return;
        }
        self.yaw = d.x.atan2(d.z).to_degrees();
        self.pitch = d.y.atan2((d.x * d.x + d.z * d.z).sqrt()).to_degrees();
    }

    /// Sets the orientation from a quaternion.
    pub fn set_rotation(&mut self, q: Quat) {
        let (y, p, r) = quat_to_yaw_pitch_roll(q);
        self.yaw = y;
        self.pitch = p;
        self.roll = r;
    }

    /// Horizontal field of view in degrees.
    pub fn horizontal_fov_deg(&self) -> f32 {
        horizontal_fov_deg(self.focal_length_mm, self.film_back.width_mm)
    }

    /// Vertical field of view in degrees for an image of `aspect` (w / h).
    pub fn vertical_fov_deg(&self, aspect: f32) -> f32 {
        let th = self.film_back.width_mm / (2.0 * self.focal_length_mm) / aspect.max(1e-3);
        2.0 * th.atan().to_degrees()
    }

    /// Vertical field of view in degrees on the film back's own aspect.
    pub fn film_vertical_fov_deg(&self) -> f32 {
        2.0 * (self.film_back.height_mm / (2.0 * self.focal_length_mm))
            .atan()
            .to_degrees()
    }

    /// Sets the focal length for a horizontal field of view.
    pub fn set_horizontal_fov(&mut self, hfov_deg: f32) {
        self.focal_length_mm = focal_length_for_fov(hfov_deg, self.film_back.width_mm);
    }

    /// The view for an image of `width × height` pixels.
    pub fn view(&self, width: u32, height: u32) -> View {
        let w = width.max(1) as f32;
        let h = height.max(1) as f32;
        let aspect = w / h;
        let q = self.rotation();
        let kind = match self.projection {
            Projection::Perspective => {
                let f = self.focal_length_mm.max(0.1);
                let tx = self.film_back.width_mm.max(0.1) / (2.0 * f);
                ViewKind::Perspective {
                    tan_half: Vec2::new(tx, tx / aspect),
                }
            }
            Projection::Orthographic { height } => {
                let hh = height.max(1e-3) * 0.5;
                ViewKind::Orthographic {
                    half: Vec2::new(hh * aspect, hh),
                }
            }
        };
        View {
            origin: self.position,
            right: q * Vec3::NEG_X,
            up: q * Vec3::Y,
            forward: q * Vec3::Z,
            kind,
            width: w,
            height: h,
            near: self.near.max(1e-4),
            far: self.far.max(self.near + 1e-3),
        }
    }

    /// An orthographic camera looking straight down at `bounds` (V6).
    /// Screen up is +Z (the direction characters face at yaw 0).
    pub fn top_view(bounds: &Aabb, aspect: f32) -> Camera {
        let (c, s) = bounds_or_default(bounds);
        let span = s.z.max(s.x / aspect.max(0.1)) * 1.15 + 0.5;
        Camera {
            position: Vec3::new(c.x, bounds.max.y.max(2.0) + 50.0, c.z),
            yaw: 0.0,
            pitch: -90.0,
            roll: 0.0,
            projection: Projection::Orthographic { height: span },
            near: 0.01,
            far: 1000.0,
            ..Camera::default()
        }
    }

    /// An orthographic side view of `bounds` from +X (the scene's left).
    pub fn side_view(bounds: &Aabb, aspect: f32) -> Camera {
        let (c, s) = bounds_or_default(bounds);
        let span = s.y.max(s.z / aspect.max(0.1)) * 1.15 + 0.5;
        Camera {
            position: Vec3::new(bounds.max.x.max(c.x) + 50.0, c.y, c.z),
            yaw: -90.0,
            pitch: 0.0,
            roll: 0.0,
            projection: Projection::Orthographic { height: span },
            near: 0.01,
            far: 1000.0,
            ..Camera::default()
        }
    }

    /// An orthographic front view of `bounds`, looking along +Z.
    pub fn front_view(bounds: &Aabb, aspect: f32) -> Camera {
        let (c, s) = bounds_or_default(bounds);
        let span = s.y.max(s.x / aspect.max(0.1)) * 1.15 + 0.5;
        Camera {
            position: Vec3::new(c.x, c.y, bounds.min.z.min(c.z) - 50.0),
            yaw: 0.0,
            pitch: 0.0,
            roll: 0.0,
            projection: Projection::Orthographic { height: span },
            near: 0.01,
            far: 1000.0,
            ..Camera::default()
        }
    }

    pub(crate) fn is_valid(&self) -> bool {
        self.position.is_finite()
            && self.yaw.is_finite()
            && self.pitch.is_finite()
            && self.roll.is_finite()
            && (FOCAL_RANGE.0..=FOCAL_RANGE.1).contains(&self.focal_length_mm)
            && self.film_back.width_mm.is_finite()
            && self.film_back.width_mm > 0.5
            && self.film_back.height_mm.is_finite()
            && self.film_back.height_mm > 0.5
            && self.near.is_finite()
            && self.near > 0.0
            && self.far.is_finite()
            && self.far > self.near
            && match self.projection {
                Projection::Perspective => true,
                Projection::Orthographic { height } => height.is_finite() && height > 0.0,
            }
    }
}

fn bounds_or_default(b: &Aabb) -> (Vec3, Vec3) {
    if b.is_empty() {
        (Vec3::new(0.0, 1.0, 0.0), Vec3::splat(4.0))
    } else {
        (b.center(), b.size())
    }
}

/// Projection parameters of a [`View`].
#[derive(Debug, Clone, Copy, PartialEq)]
pub enum ViewKind {
    /// Tangents of the half field of view, horizontally and vertically.
    Perspective { tan_half: Vec2 },
    /// Half extents of the view in metres.
    Orthographic { half: Vec2 },
}

/// A camera resolved for a particular image size: everything needed to
/// project points and cast rays.
#[derive(Debug, Clone, Copy, PartialEq)]
pub struct View {
    pub origin: Vec3,
    pub right: Vec3,
    pub up: Vec3,
    pub forward: Vec3,
    pub kind: ViewKind,
    pub width: f32,
    pub height: f32,
    pub near: f32,
    pub far: f32,
}

/// A point projected onto the image.
#[derive(Debug, Clone, Copy, PartialEq)]
pub struct ScreenPoint {
    /// Pixel coordinates (0,0 = top-left corner of the image).
    pub x: f32,
    pub y: f32,
    /// Distance along the view direction (metres).
    pub depth: f32,
}

impl View {
    /// World → camera space (x right, y up, z forward).
    pub fn to_camera(&self, p: Vec3) -> Vec3 {
        let d = p - self.origin;
        Vec3::new(d.dot(self.right), d.dot(self.up), d.dot(self.forward))
    }

    /// Camera space → normalized device coordinates (-1..1, y up).
    pub fn camera_to_ndc(&self, c: Vec3) -> Vec2 {
        match self.kind {
            ViewKind::Perspective { tan_half } => {
                Vec2::new(c.x / (c.z * tan_half.x), c.y / (c.z * tan_half.y))
            }
            ViewKind::Orthographic { half } => Vec2::new(c.x / half.x, c.y / half.y),
        }
    }

    /// NDC → pixel coordinates.
    pub fn ndc_to_pixel(&self, n: Vec2) -> Vec2 {
        Vec2::new(
            (n.x + 1.0) * 0.5 * self.width,
            (1.0 - n.y) * 0.5 * self.height,
        )
    }

    /// Projects a world point; `None` when it is behind the near plane.
    pub fn project(&self, p: Vec3) -> Option<ScreenPoint> {
        let c = self.to_camera(p);
        if !c.is_finite() || c.z < self.near && matches!(self.kind, ViewKind::Perspective { .. }) {
            return None;
        }
        let s = self.ndc_to_pixel(self.camera_to_ndc(c));
        Some(ScreenPoint {
            x: s.x,
            y: s.y,
            depth: c.z,
        })
    }

    /// The ray through pixel coordinates `(x, y)` (use `+0.5` for centres):
    /// origin and unit direction.
    pub fn ray(&self, x: f32, y: f32) -> (Vec3, Vec3) {
        let nx = x / self.width * 2.0 - 1.0;
        let ny = 1.0 - y / self.height * 2.0;
        match self.kind {
            ViewKind::Perspective { tan_half } => {
                let d = self.forward + self.right * (nx * tan_half.x) + self.up * (ny * tan_half.y);
                (self.origin, d.normalize())
            }
            ViewKind::Orthographic { half } => {
                let o = self.origin + self.right * (nx * half.x) + self.up * (ny * half.y);
                (o, self.forward)
            }
        }
    }

    /// World size of one pixel at `depth` metres (vertical).
    pub fn pixel_size_at(&self, depth: f32) -> f32 {
        match self.kind {
            ViewKind::Perspective { tan_half } => 2.0 * tan_half.y * depth / self.height,
            ViewKind::Orthographic { half } => 2.0 * half.y / self.height,
        }
    }

    pub fn is_perspective(&self) -> bool {
        matches!(self.kind, ViewKind::Perspective { .. })
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn lens_maths() {
        // 50 mm on full frame: 39.6° horizontal.
        let h = horizontal_fov_deg(50.0, 36.0);
        assert!((h - 39.6).abs() < 0.05, "{h}");
        // 24.89 mm wide Super 35 at 35 mm ≈ 39.2°.
        let c = Camera {
            focal_length_mm: 35.0,
            ..Camera::default()
        };
        assert!(
            (c.horizontal_fov_deg() - 39.15).abs() < 0.1,
            "{}",
            c.horizontal_fov_deg()
        );
        assert!((focal_length_for_fov(h, 36.0) - 50.0).abs() < 1e-3);
        let v = c.vertical_fov_deg(16.0 / 9.0);
        assert!(v < c.horizontal_fov_deg());
        // Longer lens, narrower view.
        let mut t = c;
        t.focal_length_mm = 135.0;
        assert!(t.horizontal_fov_deg() < 11.0);
    }

    #[test]
    fn projection_and_rays_agree() {
        let mut c = Camera {
            position: Vec3::new(1.0, 2.0, -6.0),
            ..Camera::default()
        };
        c.look_at(Vec3::new(0.0, 1.0, 0.0));
        c.roll = 12.0;
        let v = c.view(640, 360);
        let p = Vec3::new(0.4, 1.3, 0.5);
        let s = v.project(p).unwrap();
        let (o, d) = v.ray(s.x, s.y);
        let t = (p - o).dot(d);
        assert!((o + d * t - p).length() < 1e-4);
        // The look-at target lands in the centre.
        let s = v.project(Vec3::new(0.0, 1.0, 0.0)).unwrap();
        assert!((s.x - 320.0).abs() < 0.01 && (s.y - 180.0).abs() < 0.01);
        // Screen right is the camera's right.
        let r = v
            .project(Vec3::new(0.0, 1.0, 0.0) + c.right() * 0.1)
            .unwrap();
        assert!(r.x > 320.0);
    }

    #[test]
    fn ortho_views() {
        let b = Aabb {
            min: Vec3::new(-2.0, 0.0, -1.0),
            max: Vec3::new(2.0, 2.0, 3.0),
        };
        let top = Camera::top_view(&b, 1.5).view(300, 200);
        let p = top.project(Vec3::new(0.0, 0.0, 3.0)).unwrap();
        assert!(p.y < 100.0, "+Z is up in the top view");
        let side = Camera::side_view(&b, 1.5).view(300, 200);
        let p = side.project(Vec3::new(0.0, 2.0, 1.0)).unwrap();
        assert!(p.y < 100.0);
    }
}
