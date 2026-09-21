//! Brush strokes: a painting engine for ink, pencil, chalk, markers,
//! watercolour, oils, airbrush, erasers and smudging.
//!
//! A stroke stamps dabs along its path. Each dab is an ellipse (size,
//! roundness, angle) with a hardness falloff, modulated by a procedural
//! paper grain fixed to the canvas, and carries its own colour, so wet
//! media can pick up and mix what is already there. Dabs accumulate into a
//! premultiplied paint buffer: flow builds coverage where dabs overlap and
//! opacity caps the whole stroke, so a stroke at 50 % never darkens past
//! 50 % however often it crosses itself. Painted pixels are always
//! recomputed from the layer as it was when the stroke began, which keeps
//! the live preview exact and lets the ends be re-tapered when the stroke
//! finishes.
//!
//! Pressure comes from the caller when it has it; without a tablet the
//! engine derives it from speed (faster = lighter), which reads as pressure
//! for inking.

use crate::BlendMode;
use crate::blend::{BlendSpace, blend_px};
use crate::color;
use crate::geom::{IRect, TileCoord};
use crate::image::{Mask, Raster};
use crate::tile::{TILE, TILE_PX};
use glam::{DAffine2, dvec2};
use serde::{Deserialize, Serialize};
use std::collections::{HashMap, HashSet};
use std::sync::Arc;

/// Paper texture under the brush.
#[derive(Clone, Copy, Debug, PartialEq, Eq, Hash, Default, Serialize, Deserialize)]
pub enum GrainKind {
    #[default]
    None,
    /// Fine tooth, like drawing paper.
    Paper,
    /// Woven, like canvas.
    Canvas,
    /// Coarse, broken, like chalk on a board.
    Chalk,
    /// Random specks, like a dry brush.
    Speckle,
    /// Streaks along the stroke, like the hairs of a loaded brush.
    Bristle,
    /// Screentone: a regular grid of dots fixed to the canvas; `scale` is
    /// the dot pitch and `grain_strength` the ink area fraction (0 = empty,
    /// 1 = solid). Dots grow into their neighbours at high densities.
    Halftone,
    /// Parallel hatching lines fixed to the canvas at 45°; `scale` is the
    /// line pitch.
    Hatch,
    /// Two hatching directions crossing.
    CrossHatch,
}

/// How the stroke composites onto the layer.
#[derive(Clone, Copy, Debug, PartialEq, Eq, Default, Serialize, Deserialize)]
pub enum BrushBlend {
    #[default]
    Normal,
    /// Darkens where strokes overlap, like a marker.
    Multiply,
    /// Only paints where the layer is transparent.
    Behind,
}

#[derive(Clone, Copy, Debug, PartialEq, Serialize, Deserialize)]
#[serde(default)]
pub struct Brush {
    /// Diameter in pixels.
    pub size: f32,
    /// Fraction of the radius at full strength, 0–1.
    pub hardness: f32,
    /// Cap on the whole stroke, 0–1.
    pub opacity: f32,
    /// Build-up per dab, 0–1.
    pub flow: f32,
    /// Dab distance as a fraction of the diameter.
    pub spacing: f32,

    // ── Tip ──
    /// Ellipse ratio, 0.05–1 (1 = round).
    pub roundness: f32,
    /// Tip angle in degrees.
    pub angle: f32,
    /// Add the stroke direction to the tip angle (chisel tips).
    pub follow_path: bool,

    // ── Grain ──
    pub grain: GrainKind,
    /// Grain feature size in pixels.
    pub grain_scale: f32,
    /// How much grain shows, 0–1. For Halftone, the fraction of area inked.
    pub grain_strength: f32,

    // ── Dynamics ──
    /// How much pressure shrinks the tip, 0–1.
    pub size_pressure: f32,
    /// How much pressure thins the flow, 0–1.
    pub flow_pressure: f32,
    /// Without a tablet: how much speed reads as light pressure, 0–1.
    pub speed_thins: f32,
    /// Thin the first and last this many pixels of the stroke.
    pub taper_start: f32,
    pub taper_end: f32,
    /// Path smoothing, 0–1.
    pub stabilizer: f32,
    /// How much pen tilt shapes the tip, 0–1: a tilted pen widens and
    /// flattens the dab along the tilt, like the side of a pencil.
    pub tilt: f32,
    /// Pressure response: effective = pressure ^ curve. Below 1 a light
    /// touch already paints strongly; above 1 it takes a firm press.
    pub pressure_curve: f32,

    // ── Jitter ──
    /// Random size variation, 0–1.
    pub size_jitter: f32,
    /// Random offset as a fraction of the size, 0–1.
    pub scatter: f32,
    /// Random hue and lightness variation, 0–1.
    pub color_jitter: f32,

    // ── Medium ──
    /// How much each dab picks up what is under it, 0–1 (oils, watercolour).
    pub wetness: f32,
    /// Pigment pools at the edge of the stroke, 0–1 (watercolour).
    pub edge_darken: f32,
    /// Paint thickness lit from the top-left, 0–1 (oils, impasto).
    pub relief: f32,
    pub blend: BrushBlend,
    /// Image tip from the texture registry (0 = the round procedural tip).
    pub tip: u32,
    /// Image grain from the registry, tiled over the canvas (0 = `grain`).
    pub grain_tex: u32,
}

impl Default for Brush {
    fn default() -> Self {
        Self {
            size: 40.0,
            hardness: 0.8,
            opacity: 1.0,
            flow: 1.0,
            spacing: 0.12,
            roundness: 1.0,
            angle: 0.0,
            follow_path: false,
            grain: GrainKind::None,
            grain_scale: 4.0,
            grain_strength: 0.0,
            size_pressure: 0.0,
            flow_pressure: 0.0,
            speed_thins: 0.0,
            taper_start: 0.0,
            taper_end: 0.0,
            tilt: 0.0,
            pressure_curve: 1.0,
            stabilizer: 0.0,
            size_jitter: 0.0,
            scatter: 0.0,
            color_jitter: 0.0,
            wetness: 0.0,
            edge_darken: 0.0,
            relief: 0.0,
            blend: BrushBlend::Normal,
            tip: 0,
            grain_tex: 0,
        }
    }
}

impl Brush {
    /// Clamp every field into its working range.
    pub fn sanitized(mut self) -> Self {
        let u = |v: f32| {
            if v.is_finite() {
                v.clamp(0.0, 1.0)
            } else {
                0.0
            }
        };
        self.size = if self.size.is_finite() {
            self.size.clamp(1.0, 1000.0)
        } else {
            40.0
        };
        self.hardness = u(self.hardness);
        self.opacity = u(self.opacity).max(0.01);
        self.flow = u(self.flow).max(0.01);
        self.spacing = if self.spacing.is_finite() {
            self.spacing.clamp(0.02, 2.0)
        } else {
            0.12
        };
        self.roundness = if self.roundness.is_finite() {
            self.roundness.clamp(0.05, 1.0)
        } else {
            1.0
        };
        self.angle = if self.angle.is_finite() {
            self.angle.rem_euclid(360.0)
        } else {
            0.0
        };
        self.grain_scale = if self.grain_scale.is_finite() {
            self.grain_scale.clamp(1.0, 64.0)
        } else {
            4.0
        };
        self.grain_strength = u(self.grain_strength);
        self.relief = u(self.relief);
        self.tilt = u(self.tilt);
        self.pressure_curve = if self.pressure_curve.is_finite() {
            self.pressure_curve.clamp(0.25, 4.0)
        } else {
            1.0
        };
        self.size_pressure = u(self.size_pressure);
        self.flow_pressure = u(self.flow_pressure);
        self.speed_thins = u(self.speed_thins);
        self.taper_start = if self.taper_start.is_finite() {
            self.taper_start.clamp(0.0, 2000.0)
        } else {
            0.0
        };
        self.taper_end = if self.taper_end.is_finite() {
            self.taper_end.clamp(0.0, 2000.0)
        } else {
            0.0
        };
        self.stabilizer = u(self.stabilizer);
        self.size_jitter = u(self.size_jitter);
        self.scatter = u(self.scatter);
        self.color_jitter = u(self.color_jitter);
        self.wetness = u(self.wetness);
        self.edge_darken = u(self.edge_darken);
        self
    }
}

/// What the stroke puts down.
#[derive(Clone, Debug)]
pub enum Ink {
    /// Premultiplied linear colour (alpha 1 for opaque paint).
    Color([f32; 4]),
    Erase,
    /// Copy from the layer itself at an offset (clone stamp).
    Clone {
        dx: f32,
        dy: f32,
    },
    /// Pick up the colour under the brush and drag it along.
    Smudge,
}

/// Selection coverage for a layer pixel, 0–1.
pub type Clip = Arc<dyn Fn(i32, i32) -> f32 + Send + Sync>;

/// The composited image under a layer pixel, premultiplied linear.
pub type Backdrop = Arc<dyn Fn(i32, i32) -> [f32; 4] + Send + Sync>;

/// One input sample, in layer pixels.
#[derive(Clone, Copy, Debug)]
struct Sample {
    x: f32,
    y: f32,
    pressure: f32,
    /// Pen tilt in degrees from vertical, x and y.
    tilt: (f32, f32),
}

/// Accumulated paint: premultiplied colour, alpha = coverage.
/// Per pixel: accumulated premultiplied ink (0–3) and coverage (4).
/// Premultiplied colour, coverage, and paint thickness (unbounded flow sum).
type PaintTile = Vec<[f32; 6]>;

struct PersistentPaint {
    backend: Box<dyn crate::paint_accel::PersistentStroke>,
    brush: Brush,
    journal: Vec<crate::paint_accel::ResolvedDab>,
    submitted: usize,
}

pub struct Stroke {
    persistent_factory: Option<Arc<dyn crate::paint_accel::PersistentFactory>>,
    persistent: Option<PersistentPaint>,
    pub brush: Brush,
    ink: Ink,
    base: Arc<Raster>,
    clip: Option<Clip>,
    alpha_lock: bool,
    paint: HashMap<TileCoord, PaintTile>,
    pending: HashSet<TileCoord>,
    /// Layer-space bounds of everything painted so far.
    pub touched: IRect,
    /// Smoothed input path, as stamped.
    path: Vec<Sample>,
    /// Raw input for the stabilizer and speed.
    raw_last: Option<(f32, f32, f64)>,
    /// Every raw input point (x, y, time ms, pressure), for QuickShape.
    raw: Vec<(f32, f32, f64, f32)>,
    smooth: Option<(f32, f32)>,
    speed: f32,
    /// Stamping state along `path`.
    last: Option<Sample>,
    /// Remaining path distance to the next dab, carried across input samples.
    carry: f32,
    distance: f32,
    dir: f32,
    rng: u64,
    seed: u64,
    /// Colour a smudge or wet brush is carrying.
    load: Option<[f32; 4]>,
    /// The image under this layer, for wet brushes to pick up where the
    /// layer is transparent. Layer pixel → premultiplied colour.
    backdrop: Option<Backdrop>,
    dabs: u32,
    /// Mirror axes in layer pixels.
    mirror_x: Option<f32>,
    mirror_y: Option<f32>,
    /// Rotational symmetry: centre and number of copies.
    radial: Option<((f32, f32), u32)>,
    /// Layer pixels to the coordinate space containing symmetry axes.
    symmetry_space: DAffine2,
    finished: bool,
}

#[inline]
fn falloff(d: f32, hardness: f32) -> f32 {
    if d >= 1.0 {
        return 0.0;
    }
    let h = hardness.clamp(0.0, 0.99);
    if d <= h {
        return 1.0;
    }
    let t = (d - h) / (1.0 - h);
    1.0 - t * t * (3.0 - 2.0 * t)
}

#[inline]
fn hash2(x: i32, y: i32, seed: u32) -> f32 {
    let mut h = (x as u32).wrapping_mul(0x8da6_b343)
        ^ (y as u32).wrapping_mul(0xd825_5f9d)
        ^ seed.wrapping_mul(0x9e37_79b9);
    h ^= h >> 15;
    h = h.wrapping_mul(0x2c1b_3c6d);
    h ^= h >> 12;
    h = h.wrapping_mul(0x297a_2d39);
    h ^= h >> 15;
    (h & 0x00ff_ffff) as f32 / 16_777_216.0
}

/// Smooth value noise in [0,1] at `scale` pixels per cell.
fn value_noise(x: f32, y: f32, scale: f32, seed: u32) -> f32 {
    let (fx, fy) = (x / scale, y / scale);
    let (ix, iy) = (fx.floor(), fy.floor());
    let (tx, ty) = (fx - ix, fy - iy);
    let (sx, sy) = (tx * tx * (3.0 - 2.0 * tx), ty * ty * (3.0 - 2.0 * ty));
    let (ix, iy) = (ix as i32, iy as i32);
    let a = hash2(ix, iy, seed);
    let b = hash2(ix + 1, iy, seed);
    let c = hash2(ix, iy + 1, seed);
    let d = hash2(ix + 1, iy + 1, seed);
    let top = a + (b - a) * sx;
    let bot = c + (d - c) * sx;
    top + (bot - top) * sy
}

/// Paper grain at a canvas position, 0–1 (1 = the brush lands fully).
pub fn grain(kind: GrainKind, x: f32, y: f32, scale: f32) -> f32 {
    match kind {
        GrainKind::None => 1.0,
        GrainKind::Paper => {
            let n = 0.6 * value_noise(x, y, scale, 1) + 0.4 * value_noise(x, y, scale * 0.5, 2);
            (n * 1.4 - 0.1).clamp(0.0, 1.0)
        }
        GrainKind::Canvas => {
            let weave = 0.5
                + 0.25
                    * ((x / scale * std::f32::consts::PI).sin()
                        + (y / scale * std::f32::consts::PI).sin());
            let n = value_noise(x, y, scale * 2.0, 3);
            (weave * 0.7 + n * 0.3).clamp(0.0, 1.0)
        }
        GrainKind::Chalk => {
            let n = 0.5 * value_noise(x, y, scale, 4) + 0.5 * value_noise(x, y, scale * 0.35, 5);
            // High contrast: the tooth catches or it does not.
            ((n - 0.45) * 4.0 + 0.5).clamp(0.0, 1.0)
        }
        GrainKind::Speckle => {
            let n = hash2(
                (x / scale.max(1.0)).floor() as i32,
                (y / scale.max(1.0)).floor() as i32,
                6,
            );
            if n > 0.55 { 1.0 } else { n * 0.3 }
        }
        // Sampled in the dab's own frame; see `Stroke::stamp`.
        GrainKind::Bristle => 1.0,
        GrainKind::Halftone => {
            // Distance to the nearest dot centre on a 45° grid.
            let (u, v) = (
                (x + y) / std::f32::consts::SQRT_2,
                (x - y) / std::f32::consts::SQRT_2,
            );
            let (fu, fv) = (
                (u / scale).rem_euclid(1.0) - 0.5,
                (v / scale).rem_euclid(1.0) - 0.5,
            );
            1.0 - halftone_area(fu.hypot(fv) as f64) as f32
        }
        GrainKind::Hatch => {
            let u = (x + y) / std::f32::consts::SQRT_2;
            let f = (u / scale).rem_euclid(1.0);
            if f < 0.28 { 1.0 } else { 0.0 }
        }
        GrainKind::CrossHatch => {
            let (u, v) = (
                (x + y) / std::f32::consts::SQRT_2,
                (x - y) / std::f32::consts::SQRT_2,
            );
            let (fu, fv) = ((u / scale).rem_euclid(1.0), (v / scale).rem_euclid(1.0));
            if fu < 0.24 || fv < 0.24 { 1.0 } else { 0.0 }
        }
    }
}

/// Area of a centred circle clipped to one unit square of the tone grid.
fn halftone_area(radius: f64) -> f64 {
    if radius >= std::f64::consts::FRAC_1_SQRT_2 {
        return 1.0;
    }
    let area = std::f64::consts::PI * radius * radius;
    if radius <= 0.5 {
        area
    } else {
        // Subtract the four circular segments beyond the cell's edges.
        area - 4.0
            * (radius * radius * (0.5 / radius).acos() - 0.5 * (radius * radius - 0.25).sqrt())
    }
}

/// Invert area once per render, not for every pixel or dab.
fn halftone_radius(density: f32) -> f32 {
    let density = density.clamp(0.0, 1.0) as f64;
    if density <= std::f64::consts::FRAC_PI_4 {
        return (density / std::f64::consts::PI).sqrt() as f32;
    }
    if density >= 1.0 {
        return std::f32::consts::FRAC_1_SQRT_2;
    }
    let (mut lo, mut hi) = (0.5, std::f64::consts::FRAC_1_SQRT_2);
    for _ in 0..24 {
        let mid = (lo + hi) * 0.5;
        if halftone_area(mid) < density {
            lo = mid;
        } else {
            hi = mid;
        }
    }
    ((lo + hi) * 0.5) as f32
}

/// Pixel-area coverage of a page-fixed ink pattern. Apply after accumulating
/// dabs so overlapping stamps cannot fill antialiased pattern boundaries.
/// Page-fixed patterns (screentone, hatching) depend only on the pixel
/// position, so each tile's coverage is computed once and shared across
/// strokes and pointer moves instead of sixteen samples per pixel per
/// render. A few hundred tiles are kept; older ones are dropped.
type PatternKey = (GrainKind, u32, u32, TileCoord);

fn pattern_cache() -> &'static std::sync::Mutex<HashMap<PatternKey, Arc<[f32]>>> {
    static C: std::sync::OnceLock<std::sync::Mutex<HashMap<PatternKey, Arc<[f32]>>>> =
        std::sync::OnceLock::new();
    C.get_or_init(|| std::sync::Mutex::new(HashMap::new()))
}

fn pattern_tile(kind: GrainKind, scale: f32, radius: f32, c: TileCoord) -> Arc<[f32]> {
    let key = (kind, scale.to_bits(), radius.to_bits(), c);
    if let Some(t) = pattern_cache().lock().unwrap().get(&key) {
        return t.clone();
    }
    let t = TILE as i32;
    let (ox, oy) = (c.x * t, c.y * t);
    // Sixteen samples per pixel over a whole tile is real work; rows go
    // wide across the cores.
    use rayon::prelude::*;
    let mut v = vec![0.0f32; TILE_PX];
    v.par_chunks_mut(TILE as usize)
        .enumerate()
        .for_each(|(row, out)| {
            let y = (oy + row as i32) as f32;
            for (col, o) in out.iter_mut().enumerate() {
                *o = pattern_coverage(kind, (ox + col as i32) as f32, y, scale, radius);
            }
        });
    let tile: Arc<[f32]> = v.into();
    let mut cache = pattern_cache().lock().unwrap();
    if cache.len() >= 256 {
        cache.clear();
    }
    cache.insert(key, tile.clone());
    tile
}

fn pattern_coverage(kind: GrainKind, x: f32, y: f32, scale: f32, radius: f32) -> f32 {
    // Coverage of the pixel at (x, y) by the pattern, antialiased over one
    // pixel from the signed distance to the ink edge: one evaluation per
    // pixel instead of a 4×4 supersample.
    let (px, py) = (x + 0.5, y + 0.5);
    let u = (px + py) * std::f32::consts::FRAC_1_SQRT_2;
    let v = (px - py) * std::f32::consts::FRAC_1_SQRT_2;
    // Signed distance (pixels) from the pixel centre to the edge of the
    // ink band centred on each cell boundary, positive inside the ink.
    let band = |w: f32, half: f32| -> f32 {
        let f = (w / scale).rem_euclid(1.0);
        let g = f.min(1.0 - f);
        (half - g) * scale
    };
    match kind {
        GrainKind::Halftone => {
            if radius <= 0.0 {
                return 0.0;
            }
            if radius >= std::f32::consts::FRAC_1_SQRT_2 {
                return 1.0;
            }
            // Dots are small and overlap at heavy tones, where a distance
            // ramp under-counts the concave gaps; a 3×3 sample keeps the
            // density calibration exact at a fraction of the old 4×4.
            let r2 = radius * radius;
            let mut inked = 0u32;
            for sy in [-1.0f32 / 3.0, 0.0, 1.0 / 3.0] {
                for sx in [-1.0f32 / 3.0, 0.0, 1.0 / 3.0] {
                    let (qx, qy) = (px + sx, py + sy);
                    let fu =
                        ((qx + qy) * std::f32::consts::FRAC_1_SQRT_2 / scale).rem_euclid(1.0) - 0.5;
                    let fv =
                        ((qx - qy) * std::f32::consts::FRAC_1_SQRT_2 / scale).rem_euclid(1.0) - 0.5;
                    inked += (fu * fu + fv * fv <= r2) as u32;
                }
            }
            inked as f32 / 9.0
        }
        GrainKind::Hatch => (band(u, 0.28) + 0.5).clamp(0.0, 1.0),
        GrainKind::CrossHatch => {
            let a = (band(u, 0.28) + 0.5).clamp(0.0, 1.0);
            let b = (band(v, 0.28) + 0.5).clamp(0.0, 1.0);
            a.max(b)
        }
        _ => grain(kind, x, y, scale),
    }
}

/// Bristle streaks: vary across the stroke (`across`, in pixels), stay
/// nearly constant along it, and shift a little from dab to dab.
fn bristle(across: f32, along: f32, scale: f32, dab: u32) -> f32 {
    let n = 0.7 * value_noise(across, dab as f32 * 0.35, scale * 0.5, 7)
        + 0.3 * value_noise(across, along * 0.15, scale * 1.5, 8);
    ((n - 0.35) * 2.2).clamp(0.0, 1.0)
}

fn rotate_hue(p: [f32; 4], amount: f32, light: f32) -> [f32; 4] {
    // Cheap hue shift in linear RGB via the YIQ plane, then a lightness scale.
    let (r, g, b) = (p[0], p[1], p[2]);
    let (yy, i, q) = (
        0.299 * r + 0.587 * g + 0.114 * b,
        0.596 * r - 0.274 * g - 0.322 * b,
        0.211 * r - 0.523 * g + 0.312 * b,
    );
    let (s, c) = (amount * std::f32::consts::TAU).sin_cos();
    let (i2, q2) = (i * c - q * s, i * s + q * c);
    let l = (1.0 + light).max(0.0);
    let out = [
        (yy + 0.956 * i2 + 0.621 * q2) * l,
        (yy - 0.272 * i2 - 0.647 * q2) * l,
        (yy - 1.106 * i2 + 1.703 * q2) * l,
    ];
    [
        out[0].clamp(0.0, p[3].max(1.0)),
        out[1].clamp(0.0, p[3].max(1.0)),
        out[2].clamp(0.0, p[3].max(1.0)),
        p[3],
    ]
}

impl Stroke {
    pub fn new(base: Arc<Raster>, brush: Brush, ink: Ink, clip: Option<Clip>) -> Self {
        Self::new_with_persistent(base, brush, ink, clip, crate::paint_accel::persistent())
    }

    /// Explicit backend selection for parity tests and callers requiring CPU painting.
    pub fn new_with_persistent(
        base: Arc<Raster>,
        brush: Brush,
        ink: Ink,
        clip: Option<Clip>,
        factory: Option<Arc<dyn crate::paint_accel::PersistentFactory>>,
    ) -> Self {
        let seed = 0x5EED_0000 ^ (base.width() as u64) << 20 ^ base.height() as u64;
        Self {
            persistent_factory: factory,
            persistent: None,
            brush: brush.sanitized(),
            ink,
            base,
            clip,
            alpha_lock: false,
            paint: HashMap::new(),
            pending: HashSet::new(),
            touched: IRect::default(),
            path: Vec::new(),
            raw_last: None,
            raw: Vec::new(),
            smooth: None,
            speed: 0.0,
            last: None,
            carry: 0.0,
            distance: 0.0,
            dir: 0.0,
            rng: seed,
            seed,
            load: None,
            backdrop: None,
            dabs: 0,
            mirror_x: None,
            mirror_y: None,
            radial: None,
            symmetry_space: DAffine2::IDENTITY,
            finished: false,
        }
    }

    /// Whether this stroke is currently accumulating dabs on the optional backend.
    pub fn uses_persistent(&self) -> bool {
        self.persistent.is_some()
    }

    fn persistent_eligible(&self) -> bool {
        let b = self.brush;
        b.size >= 400.0
            && self.base.width() <= 1024
            && self.base.height() <= 1024
            && b.roundness == 1.0
            && b.angle == 0.0
            && !b.follow_path
            && b.grain == GrainKind::None
            && b.grain_tex == 0
            && b.tip == 0
            && b.grain_strength == 0.0
            && b.wetness == 0.0
            && b.edge_darken == 0.0
            && b.relief == 0.0
            && b.blend == BrushBlend::Normal
            && b.size_pressure == 0.0
            && b.flow_pressure == 0.0
            && b.speed_thins == 0.0
            && b.taper_start == 0.0
            && b.taper_end == 0.0
            && b.tilt == 0.0
            && b.size_jitter == 0.0
            && b.scatter == 0.0
            && b.color_jitter == 0.0
            && matches!(self.ink, Ink::Color(_))
            && self.clip.is_none()
            && !self.alpha_lock
            && self.mirror_x.is_none()
            && self.mirror_y.is_none()
            && self.radial.is_none()
            && self.symmetry_space == DAffine2::IDENTITY
    }

    fn check_persistent_brush(&mut self) {
        if self
            .persistent
            .as_ref()
            .is_some_and(|p| p.brush != self.brush)
        {
            self.recover_persistent();
        }
    }

    fn replay_journal(&mut self, journal: &[crate::paint_accel::ResolvedDab], brush: Brush) {
        let saved = self.brush;
        self.brush = brush;
        for dab in journal {
            self.brush.hardness = dab.hardness;
            self.stamp(
                (dab.center[0], dab.center[1]),
                dab.radius * 2.0,
                0.0,
                dab.flow,
                dab.color,
                DAffine2::IDENTITY,
            );
        }
        self.brush = saved;
    }

    fn recover_persistent(&mut self) {
        self.persistent_factory = None;
        if let Some(p) = self.persistent.take() {
            let count = self.dabs;
            self.replay_journal(&p.journal, p.brush);
            self.dabs = count;
        }
    }

    pub fn base(&self) -> &Arc<Raster> {
        &self.base
    }

    /// Preserve the layer's alpha, including partially transparent edges.
    pub fn set_alpha_lock(&mut self, enabled: bool) {
        if self.persistent.is_some() {
            self.recover_persistent();
        }
        self.alpha_lock = enabled;
    }

    /// For clone strokes: where to copy from, relative to the brush, in
    /// layer pixels.
    pub fn set_clone_offset(&mut self, dx: f32, dy: f32) {
        if self.persistent.is_some() {
            self.recover_persistent();
        }
        if let Ink::Clone { dx: a, dy: b } = &mut self.ink {
            *a = dx;
            *b = dy;
        }
    }

    /// Let wet brushes and smudges see the image under this layer, so they
    /// mix with the photo (or the layers below), not only with this layer.
    pub fn set_backdrop(&mut self, backdrop: Backdrop) {
        if self.persistent.is_some() {
            self.recover_persistent();
        }
        self.backdrop = Some(backdrop);
    }

    /// Also stamp every dab mirrored across x = `x` and/or y = `y`.
    pub fn set_mirror(&mut self, x: Option<f32>, y: Option<f32>) {
        if self.persistent.is_some() {
            self.recover_persistent();
        }
        self.mirror_x = x;
        self.mirror_y = y;
    }

    /// Interpret mirror axes and radial centers in this space (usually the
    /// document), transforming both dab positions and tip shapes back to pixels.
    pub fn set_symmetry_space(&mut self, layer_to_space: DAffine2) {
        if self.persistent.is_some() {
            self.recover_persistent();
        }
        self.symmetry_space = layer_to_space;
    }

    /// Also stamp every dab rotated `n` ways around `center` (mandalas).
    /// `n < 2` turns it off.
    pub fn set_radial(&mut self, center: (f32, f32), n: u32) {
        if self.persistent.is_some() {
            self.recover_persistent();
        }
        self.radial = (n >= 2).then_some((center, n.min(64)));
    }

    fn rand(&mut self) -> f32 {
        self.rng ^= self.rng << 13;
        self.rng ^= self.rng >> 7;
        self.rng ^= self.rng << 17;
        (self.rng >> 40) as f32 / (1u64 << 24) as f32
    }

    /// The layer with the paint so far, at one pixel.
    fn current_at(&self, x: i32, y: i32) -> [f32; 4] {
        let b = self.base.bounds();
        if x < b.x || y < b.y || x >= b.right() || y >= b.bottom() {
            return [0.0; 4];
        }
        let layer = color::px_to_f(self.base.get(x as u32, y as u32));
        let under = match &self.backdrop {
            Some(bd) => {
                let b = bd(x, y);
                [0, 1, 2, 3].map(|i| layer[i] + b[i] * (1.0 - layer[3]))
            }
            None => layer,
        };
        let t = TILE as i32;
        let c = TileCoord::new(x.div_euclid(t), y.div_euclid(t));
        match self.paint.get(&c) {
            Some(tile) => {
                let p = tile[((y - c.y * t) * t + (x - c.x * t)) as usize];
                let k = p[4].min(1.0) * self.brush.opacity;
                if k <= 0.0 {
                    return under;
                }
                let s = p[4].max(1e-6);
                let ink = [p[0] / s, p[1] / s, p[2] / s, p[3] / s];
                [0, 1, 2, 3].map(|i| ink[i] * k + under[i] * (1.0 - ink[3] * k))
            }
            None => under,
        }
    }

    /// Average colour under a dab, from a few taps.
    fn pick(&self, cx: f32, cy: f32, r: f32) -> [f32; 4] {
        let mut acc = [0.0f32; 4];
        let taps = [(0.0, 0.0), (0.5, 0.0), (-0.5, 0.0), (0.0, 0.5), (0.0, -0.5)];
        for (ox, oy) in taps {
            let p = self.current_at((cx + ox * r).floor() as i32, (cy + oy * r).floor() as i32);
            for i in 0..4 {
                acc[i] += p[i] / taps.len() as f32;
            }
        }
        acc
    }

    /// Stamp one dab and its mirrors and rotations.
    fn dab(&mut self, cx: f32, cy: f32, pressure: f32, tilt: (f32, f32), taper: f32) {
        let b = self.brush;
        let jitter = 1.0 - b.size_jitter * self.rand();
        let mut size = b.size * (1.0 - b.size_pressure * (1.0 - pressure)) * taper * jitter;
        let flow = b.flow * (1.0 - b.flow_pressure * (1.0 - pressure));
        let (sx, sy) = if b.scatter > 0.0 {
            let a = self.rand() * std::f32::consts::TAU;
            let d = self.rand() * b.scatter * b.size;
            (cx + a.cos() * d, cy + a.sin() * d)
        } else {
            (cx, cy)
        };
        let mut angle = if b.follow_path {
            b.angle + self.dir
        } else {
            b.angle
        };
        // Tilt: 60° from vertical counts as fully laid down. The dab widens
        // along the tilt and flattens across it.
        let lean = (tilt.0.hypot(tilt.1) / 60.0).clamp(0.0, 1.0) * b.tilt;
        let saved_round = self.brush.roundness;
        if lean > 0.0 {
            size *= 1.0 + lean * 1.5;
            angle = tilt.1.atan2(tilt.0).to_degrees();
            self.brush.roundness = (saved_round * (1.0 - lean * 0.7)).max(0.05);
        }
        let colour = self.dab_colour(sx, sy, size / 2.0);
        let mut stamps = vec![DAffine2::IDENTITY];
        if let Some(mx) = self.mirror_x {
            stamps.push(
                DAffine2::from_translation(dvec2(2.0 * mx as f64, 0.0))
                    * DAffine2::from_scale(dvec2(-1.0, 1.0)),
            );
        }
        if let Some(my) = self.mirror_y {
            let reflection = DAffine2::from_translation(dvec2(0.0, 2.0 * my as f64))
                * DAffine2::from_scale(dvec2(1.0, -1.0));
            for transform in stamps.clone() {
                stamps.push(reflection * transform);
            }
        }
        if let Some(((ox, oy), n)) = self.radial {
            let base = stamps.clone();
            for k in 1..n {
                let center = dvec2(ox as f64, oy as f64);
                let rotation = DAffine2::from_translation(center)
                    * DAffine2::from_angle(k as f64 / n as f64 * std::f64::consts::TAU)
                    * DAffine2::from_translation(-center);
                for transform in &base {
                    stamps.push(rotation * *transform);
                }
            }
        }
        let inverse = self.symmetry_space.inverse();
        for transform in stamps {
            let transform = if transform == DAffine2::IDENTITY {
                transform
            } else {
                inverse * transform * self.symmetry_space
            };
            self.stamp((sx, sy), size, angle, flow, colour, transform);
        }
        self.brush.roundness = saved_round;
    }

    fn dab_colour(&mut self, cx: f32, cy: f32, r: f32) -> [f32; 4] {
        match self.ink.clone() {
            Ink::Erase | Ink::Clone { .. } => [0.0, 0.0, 0.0, 1.0],
            Ink::Smudge => {
                let under = self.pick(cx, cy, r);
                let w = 0.5 + 0.5 * self.brush.wetness;
                let load = match self.load {
                    None => under,
                    Some(l) => [0, 1, 2, 3].map(|i| l[i] * w + under[i] * (1.0 - w)),
                };
                self.load = Some(load);
                load
            }
            Ink::Color(mut c) => {
                if self.brush.color_jitter > 0.0 {
                    let (h, l) = (
                        (self.rand() - 0.5) * 0.15 * self.brush.color_jitter,
                        (self.rand() - 0.5) * 0.5 * self.brush.color_jitter,
                    );
                    c = rotate_hue(c, h, l);
                }
                if self.brush.wetness > 0.0 {
                    let under = self.pick(cx, cy, r);
                    // Only mix with paint, not with transparency.
                    let w = self.brush.wetness * under[3].min(1.0);
                    let mixed = [0, 1, 2, 3].map(|i| c[i] * (1.0 - w) + under[i] * w);
                    let load = match self.load {
                        None => mixed,
                        Some(l) => [0, 1, 2, 3].map(|i| l[i] * 0.6 + mixed[i] * 0.4),
                    };
                    self.load = Some(load);
                    // Keep the ink's alpha so wet strokes still cover.
                    let a = c[3].max(1e-6);
                    let la = load[3].max(1e-6);
                    return [load[0] / la * a, load[1] / la * a, load[2] / la * a, a];
                }
                c
            }
        }
    }

    fn stamp(
        &mut self,
        center: (f32, f32),
        size: f32,
        angle_deg: f32,
        flow: f32,
        colour: [f32; 4],
        transform: DAffine2,
    ) {
        let r = (size / 2.0).max(0.3);
        let center = transform.transform_point2(dvec2(center.0 as f64, center.1 as f64));
        let (cx, cy) = (center.x as f32, center.y as f32);
        let m = transform.matrix2;
        let (rx, ry) = (
            r * (m.x_axis.x.abs() + m.y_axis.x.abs()) as f32,
            r * (m.x_axis.y.abs() + m.y_axis.y.abs()) as f32,
        );
        let b = IRect::new(
            (cx - rx).floor() as i32,
            (cy - ry).floor() as i32,
            (2.0 * rx).ceil() as i32 + 2,
            (2.0 * ry).ceil() as i32 + 2,
        )
        .intersect(&self.base.bounds());
        if b.is_empty() {
            return;
        }
        self.check_persistent_brush();
        if self.persistent.is_none()
            && self.paint.is_empty()
            && let Some(factory) = self.persistent_factory.take()
            && self.persistent_eligible()
            && let Some(backend) = factory.start(&self.base, self.brush.opacity)
        {
            self.persistent = Some(PersistentPaint {
                backend,
                brush: self.brush,
                journal: Vec::new(),
                submitted: 0,
            });
        }
        if self
            .persistent
            .as_ref()
            .is_some_and(|p| p.journal.len() >= 65536)
        {
            self.recover_persistent();
        }
        self.touched = self.touched.union(&b);
        let t = TILE as i32;
        if let Some(p) = &mut self.persistent {
            p.journal.push(crate::paint_accel::ResolvedDab {
                center: [cx, cy],
                radius: r,
                hardness: self.brush.hardness,
                flow: flow.clamp(0.0, 1.0),
                color: colour,
            });
            for ty in b.y.div_euclid(t)..=(b.bottom() - 1).div_euclid(t) {
                for tx in b.x.div_euclid(t)..=(b.right() - 1).div_euclid(t) {
                    self.pending.insert(TileCoord::new(tx, ty));
                }
            }
            self.dabs = self.dabs.wrapping_add(1);
            return;
        }
        let flow = flow.clamp(0.0, 1.0);
        let (hard, round) = (self.brush.hardness, self.brush.roundness);
        let (s, c) = (-angle_deg.to_radians()).sin_cos();
        let inverse = m.inverse();
        let (xx, xy, yx, yy) = (
            inverse.x_axis.x as f32,
            inverse.y_axis.x as f32,
            inverse.x_axis.y as f32,
            inverse.y_axis.y as f32,
        );
        let (gk, gs, gstr) = (
            self.brush.grain,
            self.brush.grain_scale,
            self.brush.grain_strength,
        );
        let seed = self.seed as u32;
        let tip = textures::get(self.brush.tip);
        self.dabs = self.dabs.wrapping_add(1);
        let dab_no = self.dabs;
        // Integrate sharp procedural edges over the pixel. Sampling just its
        // centre can miss a thin dab entirely when it lands between centres.
        let footprint = std::f32::consts::FRAC_1_SQRT_2
            * (xx.abs() + xy.abs()).max(yx.abs() + yy.abs())
            / (r * round);
        let antialias =
            (1.0 - hard) * r * round < 1.0 || r * round < 2.0 || transform != DAffine2::IDENTITY;
        for ty in b.y.div_euclid(t)..=(b.bottom() - 1).div_euclid(t) {
            for tx in b.x.div_euclid(t)..=(b.right() - 1).div_euclid(t) {
                let coord = TileCoord::new(tx, ty);
                let tile = self
                    .paint
                    .entry(coord)
                    .or_insert_with(|| vec![[0.0; 6]; TILE_PX]);
                let tr = IRect::new(tx * t, ty * t, t, t).intersect(&b);
                for y in tr.y..tr.bottom() {
                    for x in tr.x..tr.right() {
                        let (ox, oy) = (x as f32 + 0.5 - cx, y as f32 + 0.5 - cy);
                        let (ox, oy) = (ox * xx + oy * xy, ox * yx + oy * yy);
                        let (rx, ry) = (ox * c - oy * s, (ox * s + oy * c) / round);
                        let d = (rx * rx + ry * ry).sqrt() / r;
                        let shape = match &tip {
                            // Image tips: sample the alpha in the dab's frame.
                            Some(t) => {
                                let u = rx / r.max(0.5) * 0.5 + 0.5;
                                let v = ry / r.max(0.5) * 0.5 + 0.5;
                                if !(0.0..1.0).contains(&u) || !(0.0..1.0).contains(&v) {
                                    0.0
                                } else {
                                    // Hardness sharpens the tip's own edges.
                                    let s = t.sample(u, v);
                                    ((s - 0.5) * (1.0 + hard * 3.0) + 0.5).clamp(0.0, 1.0)
                                        * r.min(1.0)
                                }
                            }
                            None if antialias && d + footprint > hard && d - footprint < 1.0 => {
                                let mut coverage = 0.0;
                                for sy in [-0.375, -0.125, 0.125, 0.375] {
                                    for sx in [-0.375, -0.125, 0.125, 0.375] {
                                        let (sx, sy) = (sx * xx + sy * xy, sx * yx + sy * yy);
                                        let u = rx + sx * c - sy * s;
                                        let v = ry + (sx * s + sy * c) / round;
                                        coverage += falloff(u.hypot(v) / r, hard);
                                    }
                                }
                                coverage / 16.0
                            }
                            None => falloff(d, hard),
                        };
                        let mut a = shape * flow;
                        if a <= 0.0 {
                            continue;
                        }
                        let mut thick = a;
                        // Bristles live in the dab's frame and follow the stroke;
                        // the same bristles persist so their streaks build up.
                        if gstr > 0.0 && gk == GrainKind::Bristle {
                            let g = bristle(ry, rx, gs, seed.wrapping_add(dab_no / 12));
                            // Bristles leave shallow gaps in the paint but carry
                            // most of its thickness, so the body stays opaque
                            // while the ridges catch the light.
                            thick = a * (0.3 + 1.7 * g);
                            a *= 1.0 - gstr * 0.3 * (1.0 - g);
                        }
                        // Page patterns and paper-like grains apply to the whole
                        // stroke in `render`, preserving gaps between the marks.
                        if a <= 0.0005 {
                            continue;
                        }
                        let p = &mut tile[((y - ty * t) * t + (x - tx * t)) as usize];
                        for i in 0..4 {
                            p[i] = colour[i] * a + p[i] * (1.0 - a);
                        }
                        p[4] = a + p[4] * (1.0 - a);
                        p[5] += thick;
                    }
                }
                self.pending.insert(coord);
            }
        }
    }

    /// Continue the stroke to (x, y) in layer pixels, at full pressure and
    /// with no timing (speed dynamics stay off).
    pub fn point(&mut self, x: f32, y: f32) {
        self.point_at(x, y, None, None);
    }

    /// Continue the stroke to (x, y) in layer pixels. `pressure` is 0–1
    /// when the input device reports it; `time_ms` lets speed stand in for
    /// pressure when it does not.
    pub fn point_at(&mut self, x: f32, y: f32, pressure: Option<f32>, time_ms: Option<f64>) {
        self.point_full(x, y, pressure, None, time_ms);
    }

    /// `point_at` with the pen's tilt (degrees from vertical, x and y).
    pub fn point_full(
        &mut self,
        x: f32,
        y: f32,
        pressure: Option<f32>,
        tilt: Option<(f32, f32)>,
        time_ms: Option<f64>,
    ) {
        if self.finished {
            return;
        }
        // Speed, for pressure without a tablet.
        if let (Some((lx, ly, lt)), Some(t)) = (self.raw_last, time_ms) {
            let dt = (t - lt).max(1.0) as f32;
            let v = ((x - lx).hypot(y - ly) / dt).min(20.0);
            self.speed = self.speed * 0.7 + v * 0.3;
        }
        self.raw_last = Some((x, y, time_ms.unwrap_or(0.0)));
        let pressure = pressure.unwrap_or_else(|| {
            // ~1 px/ms is a relaxed hand; 6 px/ms a fast flick.
            let fast = ((self.speed - 0.8) / 5.0).clamp(0.0, 1.0);
            1.0 - self.brush.speed_thins * fast * 0.85
        });
        let pressure = if (self.brush.pressure_curve - 1.0).abs() > 1e-3 {
            pressure.clamp(0.0, 1.0).powf(self.brush.pressure_curve)
        } else {
            pressure
        };
        self.raw.push((x, y, time_ms.unwrap_or(0.0), pressure));
        // Stabilizer: the stamped point lags behind the pointer.
        let k = 1.0 - self.brush.stabilizer * 0.92;
        let (sx, sy) = match self.smooth {
            None => (x, y),
            Some((px, py)) => (px + (x - px) * k, py + (y - py) * k),
        };
        self.smooth = Some((sx, sy));
        self.advance(Sample {
            x: sx,
            y: sy,
            pressure,
            tilt: tilt.unwrap_or((0.0, 0.0)),
        });
    }

    /// The raw input so far: (x, y, time ms, pressure).
    pub fn raw_points(&self) -> &[(f32, f32, f64, f32)] {
        &self.raw
    }

    /// How long (ms) the pointer has rested within `radius` px of where it
    /// is now, as of `now_ms`. Zero while it is still travelling.
    pub fn held_ms(&self, now_ms: f64, radius: f32) -> f64 {
        let Some(&(x, y, _, _)) = self.raw.last() else {
            return 0.0;
        };
        // The first point (from the end) that lies outside the rest radius
        // ends the hold; the hold began with the point after it.
        let mut start = self.raw[0].2;
        for w in self.raw.windows(2).rev() {
            if (w[0].0 - x).hypot(w[0].1 - y) > radius {
                start = w[1].2;
                break;
            }
        }
        (now_ms - start).max(0.0)
    }

    /// Has the stroke been ended (by `finish` or `replay`)?
    pub fn is_finished(&self) -> bool {
        self.finished
    }

    /// Throw away what was painted and stamp along `pts` instead, at a
    /// steady `pressure`: QuickShape re-drawing the hand's path as the
    /// shape it meant. Stabilizer and tapers are off (a snapped shape has
    /// no wobble to smooth and no ends to thin). The stroke is finished
    /// afterwards, so later input is ignored until the pointer lifts.
    pub fn replay(&mut self, pts: &[(f32, f32)], pressure: f32) {
        self.recover_persistent();
        let touched: Vec<TileCoord> = self.paint.keys().copied().collect();
        for tile in self.paint.values_mut() {
            tile.fill([0.0; 6]);
        }
        self.pending.extend(touched);
        self.path.clear();
        self.last = None;
        self.carry = 0.0;
        self.distance = 0.0;
        self.dir = 0.0;
        self.rng = self.seed;
        self.load = None;
        self.smooth = None;
        let saved = self.brush;
        self.brush.stabilizer = 0.0;
        self.brush.taper_start = 0.0;
        self.brush.taper_end = 0.0;
        for &(x, y) in pts {
            self.advance(Sample {
                x,
                y,
                pressure,
                tilt: (0.0, 0.0),
            });
        }
        self.brush = saved;
        self.finished = true;
    }

    fn advance(&mut self, s: Sample) {
        self.check_persistent_brush();
        self.path.push(s);
        self.stamp_segment(s, None);
    }

    /// Stamp from the previous input sample to `s`. `total` is the stroke
    /// length when known (a finished stroke), which enables the end taper.
    fn stamp_segment(&mut self, s: Sample, total: Option<f32>) {
        let step = |pressure: f32, taper: f32, b: &Brush| {
            let size = b.size * (1.0 - b.size_pressure * (1.0 - pressure)) * taper;
            (size * b.spacing).max(0.5)
        };
        let taper = |dist: f32, b: &Brush| -> f32 {
            let mut t = 1.0f32;
            if b.taper_start > 0.0 {
                t = t.min((dist / b.taper_start).clamp(0.0, 1.0));
            }
            if let (Some(total), true) = (total, b.taper_end > 0.0) {
                t = t.min(((total - dist) / b.taper_end).clamp(0.0, 1.0));
            }
            // Never vanish entirely: a hairline still reads.
            0.08 + 0.92 * t
        };
        match self.last {
            None => {
                let t = taper(0.0, &self.brush);
                self.dab(s.x, s.y, s.pressure, s.tilt, t);
                self.carry = step(s.pressure, t, &self.brush);
            }
            Some(last) => {
                let (dx, dy) = (s.x - last.x, s.y - last.y);
                let len = (dx * dx + dy * dy).sqrt();
                if len > 0.01 {
                    self.dir = dy.atan2(dx).to_degrees();
                }
                let mut d = self.carry;
                while len > 0.0 && d <= len {
                    let f = d / len;
                    let t = taper(self.distance + d, &self.brush);
                    let pressure = last.pressure + (s.pressure - last.pressure) * f;
                    let tilt = (
                        last.tilt.0 + (s.tilt.0 - last.tilt.0) * f,
                        last.tilt.1 + (s.tilt.1 - last.tilt.1) * f,
                    );
                    self.dab(last.x + dx * f, last.y + dy * f, pressure, tilt, t);
                    d += step(pressure, t, &self.brush);
                }
                self.carry = d - len;
                self.distance += len;
            }
        }
        self.last = Some(s);
    }

    /// The pointer lifted. Catches the stabilizer up to the last input and
    /// re-tapers the end when the brush asks for it. Returns whether the
    /// paint changed, so the caller knows to render once more.
    pub fn finish(&mut self) -> bool {
        self.check_persistent_brush();
        if self.finished {
            return false;
        }
        self.finished = true;
        let mut changed = false;
        if let (Some((rx, ry, _)), Some((sx, sy))) = (self.raw_last, self.smooth)
            && (rx - sx).hypot(ry - sy) > 0.5
            && self.brush.stabilizer > 0.0
        {
            let p = self.path.last().map_or(1.0, |s| s.pressure);
            let tilt = self.path.last().map_or((0.0, 0.0), |s| s.tilt);
            // Finish the line in a few steps so the taper below sees them.
            for i in 1..=4 {
                let f = i as f32 / 4.0;
                self.advance(Sample {
                    x: sx + (rx - sx) * f,
                    y: sy + (ry - sy) * f,
                    pressure: p,
                    tilt,
                });
            }
            changed = true;
        }
        if self.brush.taper_end > 0.0 && self.path.len() > 1 {
            // Replay the whole path knowing its length.
            let path = std::mem::take(&mut self.path);
            let total = path
                .windows(2)
                .map(|w| (w[1].x - w[0].x).hypot(w[1].y - w[0].y))
                .sum::<f32>();
            let touched: Vec<TileCoord> = self.paint.keys().copied().collect();
            for tile in self.paint.values_mut() {
                tile.fill([0.0; 6]);
            }
            self.pending.extend(touched);
            self.last = None;
            self.carry = 0.0;
            self.distance = 0.0;
            self.rng = self.seed;
            self.load = None;
            for s in &path {
                self.stamp_segment(*s, Some(total));
            }
            self.path = path;
            changed = true;
        }
        changed
    }

    /// Apply everything painted since the last call to `current` (which must
    /// descend from the stroke's base). Returns the new layer and the
    /// layer-space rectangle that changed.
    pub fn render(&mut self, current: &Raster) -> (Raster, IRect) {
        self.check_persistent_brush();
        if self.persistent.is_some() {
            if self.pending.is_empty() {
                return (current.clone(), IRect::default());
            }
            let p = self.persistent.as_mut().unwrap();
            let output = if p.backend.append(&p.journal[p.submitted..]) {
                p.submitted = p.journal.len();
                p.backend.preview()
            } else {
                None
            };
            if let Some(pixels) = output
                && pixels.len() == self.base.width() as usize * self.base.height() as usize
            {
                let mut changes = Vec::new();
                let mut dirty = IRect::default();
                let t = TILE as i32;
                for c in self.pending.drain() {
                    let mut tile = current
                        .base_tile(c)
                        .map_or_else(|| vec![current.fill(); TILE_PX], |p| p.to_vec());
                    let bounds = IRect::new(c.x * t, c.y * t, t, t).intersect(&self.base.bounds());
                    let mut changed = false;
                    for y in bounds.y..bounds.bottom() {
                        for x in bounds.x..bounds.right() {
                            let i = ((y - c.y * t) * t + x - c.x * t) as usize;
                            let value =
                                pixels[y as usize * self.base.width() as usize + x as usize];
                            changed |= tile[i] != value;
                            tile[i] = value;
                        }
                    }
                    if changed {
                        changes.push((c, Some(tile)));
                        dirty = dirty.union(&bounds);
                    }
                }
                return (current.with_changes(changes), dirty);
            }
            self.recover_persistent();
        }
        self.render_with_compositor(current, crate::paint_accel::compositor())
    }

    /// Render with an explicit backend for routing and CPU/GPU parity benchmarks.
    pub fn render_with_compositor(
        &mut self,
        current: &Raster,
        compositor: Option<&dyn crate::paint_accel::PaintCompositor>,
    ) -> (Raster, IRect) {
        self.recover_persistent();
        let t = TILE as i32;
        let mut changes = Vec::new();
        let mut dirty = IRect::default();
        let opacity = self.brush.opacity.clamp(0.0, 1.0);
        let mode = match self.brush.blend {
            BrushBlend::Normal | BrushBlend::Behind => BlendMode::Normal,
            BrushBlend::Multiply => BlendMode::Multiply,
        };
        let (gk, gs, gstr) = (
            self.brush.grain,
            self.brush.grain_scale,
            self.brush.grain_strength,
        );
        let grain_tex = textures::get(self.brush.grain_tex);
        let textured = gstr > 0.0
            && (grain_tex.is_some()
                || matches!(
                    gk,
                    GrainKind::Paper | GrainKind::Canvas | GrainKind::Chalk | GrainKind::Speckle
                ));
        let patterned = grain_tex.is_none()
            && (gk == GrainKind::Halftone
                || (gstr > 0.0 && matches!(gk, GrainKind::Hatch | GrainKind::CrossHatch)));
        let tone_radius = if patterned && gk == GrainKind::Halftone {
            halftone_radius(gstr)
        } else {
            0.0
        };
        let edge = self.brush.edge_darken;
        let relief = self.brush.relief;
        let base_fill = self.base.fill();
        // Batch whole changed tiles once per render, never once per dab. Small
        // strokes avoid upload/readback overhead; advanced media keep the exact
        // reference implementation until their kernels have parity coverage.
        if (4..=32).contains(&self.pending.len())
            && !textured
            && !patterned
            && edge == 0.0
            && relief == 0.0
            && !matches!(self.ink, Ink::Clone { .. })
            && let Some(compositor) = compositor
        {
            let tiles: Vec<_> = self
                .pending
                .iter()
                .map(|&coord| (coord, self.paint[&coord].as_slice()))
                .collect();
            let batch = crate::paint_accel::PaintBatch {
                base: &self.base,
                tiles: &tiles,
                clip: self.clip.as_ref(),
                opacity,
                blend: self.brush.blend,
                erase: matches!(self.ink, Ink::Erase),
                alpha_lock: self.alpha_lock,
            };
            if let Some(output) = compositor.composite_paint(&batch)
                && output.len() == tiles.len()
                && output.iter().all(|tile| tile.len() == TILE_PX)
            {
                for ((coord, _), out) in tiles.iter().zip(output) {
                    let unchanged = current.base_tile(*coord).map_or_else(
                        || out.iter().all(|p| *p == current.fill()),
                        |tile| tile.as_ref() == out.as_slice(),
                    );
                    if !unchanged {
                        dirty = dirty.union(&IRect::new(coord.x * t, coord.y * t, t, t));
                        changes.push((*coord, Some(out)));
                    }
                }
                self.pending.clear();
                return (
                    current.with_changes(changes),
                    dirty.intersect(&current.bounds()),
                );
            }
        }
        for c in std::mem::take(&mut self.pending) {
            let paint = &self.paint[&c];
            let src = self
                .base
                .base_tile(c)
                .map(|t| t.to_vec())
                .unwrap_or_else(|| vec![base_fill; TILE_PX]);
            let mut out = src.clone();
            let pattern = patterned.then(|| pattern_tile(gk, gs, tone_radius, c));
            // Paint thickness of a neighbour inside this tile, compressed so
            // heavy strokes still show ridges; for relief lighting.
            let cov = |i: usize, dx: i32, dy: i32| -> f32 {
                let (x, y) = (i as i32 % t + dx, i as i32 / t + dy);
                let j = if x < 0 || y < 0 || x >= t || y >= t {
                    i
                } else {
                    (y * t + x) as usize
                };
                (1.0 + paint[j][5]).ln()
            };
            for (i, p) in paint.iter().enumerate() {
                if p[4] <= 0.0 {
                    continue;
                }
                let (x, y) = (c.x * t + (i as i32 % t), c.y * t + (i as i32 / t));
                let raw = p[4].min(1.0);
                let mut k = raw;
                if textured {
                    // The paper's tooth as a height the paint must reach: one
                    // light pass catches only the peaks, and scrubbing or
                    // pressing (more thickness) fills the valleys.
                    let g = match &grain_tex {
                        // Image grain tiles across the canvas at `grain_scale`
                        // pixels per texture pixel.
                        Some(t) => t.tiled(x as f32 / gs.max(0.1), y as f32 / gs.max(0.1)),
                        None => grain(gk, x as f32, y as f32, gs),
                    };
                    let depth = gstr * (1.0 - g);
                    let fill = (((1.0 + p[5]).ln() - 1.2 * depth) / 0.4).clamp(0.0, 1.0);
                    k = raw * fill;
                }
                // Pigment pools where coverage falls off: the rim of a wash.
                let rim = if edge > 0.0 {
                    4.0 * raw * (1.0 - raw)
                } else {
                    0.0
                };
                k *= 1.0 + edge * rim * 0.6;
                k = k.min(1.0) * opacity;
                if let Some(pattern) = &pattern {
                    k *= pattern[i];
                }
                if let Some(clip) = &self.clip {
                    k *= clip(x, y);
                }
                if k <= 0.0 {
                    continue;
                }
                let b = color::px_to_f(src[i]);
                if self.alpha_lock && b[3] <= 0.0 {
                    continue;
                }
                let a = p[4].max(1e-6);
                // The dab colour at full coverage, then scaled by k.
                let mut ink = [p[0] / a, p[1] / a, p[2] / a, p[3] / a];
                if edge > 0.0 {
                    let dark = 1.0 - edge * rim * 0.35;
                    for c in ink.iter_mut().take(3) {
                        *c *= dark;
                    }
                }
                if relief > 0.0 {
                    // Thickness lit from the top-left: slopes facing the
                    // light brighten, slopes away from it darken.
                    let gx = cov(i, 1, 0) - cov(i, -1, 0);
                    let gy = cov(i, 0, 1) - cov(i, 0, -1);
                    let light = (-gx - gy) * 0.7;
                    let l = (1.0 + relief * light * 1.8).clamp(0.35, 1.6);
                    let alpha = ink[3];
                    for c in ink.iter_mut().take(3) {
                        *c = (*c * l).min(alpha);
                    }
                }
                let o = match &self.ink {
                    Ink::Color(_) | Ink::Smudge => {
                        let k = if self.brush.blend == BrushBlend::Behind {
                            k * (1.0 - b[3].min(1.0))
                        } else {
                            k
                        };
                        let s = ink.map(|v| v * k);
                        if self.alpha_lock {
                            let opaque = [b[0] / b[3], b[1] / b[3], b[2] / b[3], 1.0];
                            let mixed = blend_px(mode, BlendSpace::Linear, opaque, s, 0.0);
                            [mixed[0] * b[3], mixed[1] * b[3], mixed[2] * b[3], b[3]]
                        } else {
                            blend_px(mode, BlendSpace::Linear, b, s, 0.0)
                        }
                    }
                    Ink::Erase if self.alpha_lock => b,
                    Ink::Erase => b.map(|v| v * (1.0 - k)),
                    Ink::Clone { dx, dy } => {
                        let (sx, sy) = ((x as f32 + dx).round(), (y as f32 + dy).round());
                        if sx < 0.0
                            || sy < 0.0
                            || sx >= self.base.width() as f32
                            || sy >= self.base.height() as f32
                        {
                            continue;
                        }
                        let s = color::px_to_f(self.base.get(sx as u32, sy as u32));
                        if self.alpha_lock {
                            let a = s[3];
                            [
                                s[0] * k * b[3] + b[0] * (1.0 - a * k),
                                s[1] * k * b[3] + b[1] * (1.0 - a * k),
                                s[2] * k * b[3] + b[2] * (1.0 - a * k),
                                b[3],
                            ]
                        } else {
                            [0, 1, 2, 3].map(|ch| s[ch] * k + b[ch] * (1.0 - k))
                        }
                    }
                };
                out[i] = color::f_to_px(o.map(|v| v.clamp(0.0, 1.0)));
            }
            // A touched tile need not change pixels (selection, alpha lock,
            // identical colour). Compare against the live image, not the
            // stroke base: finishing a taper can restore earlier paint.
            let unchanged = current.base_tile(c).map_or_else(
                || out.iter().all(|p| *p == current.fill()),
                |tile| tile.as_ref() == out.as_slice(),
            );
            if unchanged {
                continue;
            }
            dirty = dirty.union(&IRect::new(c.x * t, c.y * t, t, t));
            changes.push((c, Some(out)));
        }
        (
            current.with_changes(changes),
            dirty.intersect(&current.bounds()),
        )
    }

    /// Stroke coverage as a mask in layer space (for healing).
    pub fn coverage(&self) -> Mask {
        if let Some(p) = &self.persistent {
            let mut cpu = Self::new_with_persistent(
                self.base.clone(),
                self.brush,
                self.ink.clone(),
                self.clip.clone(),
                None,
            );
            cpu.replay_journal(&p.journal, p.brush);
            return cpu.coverage();
        }
        let mut m = Mask::empty(self.base.width(), self.base.height(), 0);
        for (c, tile) in &self.paint {
            m.set_tile(
                *c,
                tile.iter()
                    .enumerate()
                    .map(|(i, p)| {
                        let x = c.x * TILE as i32 + (i % TILE as usize) as i32;
                        let y = c.y * TILE as i32 + (i / TILE as usize) as i32;
                        let clip = self.clip.as_ref().map_or(1.0, |clip| clip(x, y));
                        let alpha = if self.alpha_lock {
                            self.base.get(x as u32, y as u32)[3] as f32 / 65535.0
                        } else {
                            1.0
                        };
                        (p[4].min(1.0) * self.brush.opacity * clip * alpha * 255.0).round() as u8
                    })
                    .collect(),
            );
        }
        m
    }
}

/// Paint `color` (premultiplied linear) over `region` of `base`, weighted by
/// `coverage` (0–1 per layer pixel). Used for bucket fills and filling a
/// selection.
pub fn fill_color(
    base: &Raster,
    region: IRect,
    coverage: &(dyn Fn(i32, i32) -> f32 + Sync),
    color: [f32; 4],
) -> (Raster, IRect) {
    use rayon::prelude::*;
    let region = region.intersect(&base.bounds());
    if region.is_empty() {
        return (base.clone(), region);
    }
    let t = TILE as i32;
    let coords: Vec<TileCoord> = (region.y.div_euclid(t)..=(region.bottom() - 1).div_euclid(t))
        .flat_map(|ty| {
            (region.x.div_euclid(t)..=(region.right() - 1).div_euclid(t))
                .map(move |tx| TileCoord::new(tx, ty))
        })
        .collect();
    let changes: Vec<(TileCoord, Option<Vec<[u16; 4]>>)> = coords
        .into_par_iter()
        .map(|c| {
            let mut out = base
                .base_tile(c)
                .map(|t| t.to_vec())
                .unwrap_or_else(|| vec![base.fill(); TILE_PX]);
            let tr = IRect::new(c.x * t, c.y * t, t, t).intersect(&region);
            for y in tr.y..tr.bottom() {
                for x in tr.x..tr.right() {
                    let k = coverage(x, y).clamp(0.0, 1.0);
                    if k <= 0.0 {
                        continue;
                    }
                    let i = ((y - c.y * t) * t + (x - c.x * t)) as usize;
                    let b = color::px_to_f(out[i]);
                    out[i] =
                        color::f_to_px([0, 1, 2, 3].map(|ch| color[ch] * k + b[ch] * (1.0 - k)));
                }
            }
            (c, Some(out))
        })
        .collect();
    (base.with_changes(changes), region)
}

#[cfg(test)]
mod tests {
    #[test]
    fn accelerated_paint_failure_is_atomic_and_restoration_uses_current_pixels() {
        use crate::paint_accel::{PaintBatch, PaintCompositor};
        use std::sync::atomic::{AtomicUsize, Ordering};

        struct Backend {
            mode: u8,
            calls: AtomicUsize,
        }
        impl PaintCompositor for Backend {
            fn composite_paint(&self, batch: &PaintBatch<'_>) -> Option<Vec<Vec<[u16; 4]>>> {
                self.calls.fetch_add(1, Ordering::SeqCst);
                match self.mode {
                    0 => None,
                    1 => Some(vec![vec![[0; 4]]]),
                    _ => Some(vec![vec![batch.base.fill(); TILE_PX]; batch.tiles.len()]),
                }
            }
        }
        let base = Arc::new(Raster::transparent(512, 512));
        let make_stroke = || {
            let mut stroke = Stroke::new(base.clone(), Brush::default(), opaque_red(), None);
            for y in 0..2 {
                for x in 0..2 {
                    let coord = TileCoord { x, y };
                    let mut paint = vec![[0.0; 6]; TILE_PX];
                    paint[0] = [1.0, 0.0, 0.0, 1.0, 1.0, 1.0];
                    stroke.paint.insert(coord, paint);
                    stroke.pending.insert(coord);
                }
            }
            stroke
        };
        let (reference, expected_dirty) = make_stroke().render_with_compositor(&base, None);
        assert!(!expected_dirty.is_empty());
        for mode in [0, 1] {
            let backend = Backend {
                mode,
                calls: AtomicUsize::new(0),
            };
            let mut stroke = make_stroke();
            let (actual, dirty) = stroke.render_with_compositor(&base, Some(&backend));
            assert_eq!(backend.calls.load(Ordering::SeqCst), 1);
            assert_eq!(actual.to_pixels(), reference.to_pixels());
            assert_eq!(dirty, expected_dirty);
            assert!(stroke.pending.is_empty());
        }
        // A taper replay can restore the initial pixels. GPU output must be
        // compared with the live image, not discarded as equal to stroke base.
        let backend = Backend {
            mode: 2,
            calls: AtomicUsize::new(0),
        };
        let (restored, dirty) = make_stroke().render_with_compositor(&reference, Some(&backend));
        assert_eq!(restored.to_pixels(), base.to_pixels());
        assert_eq!(dirty, expected_dirty);
        let (_, dirty) = make_stroke().render_with_compositor(&base, Some(&backend));
        assert!(dirty.is_empty(), "GPU no-op must not create an edit");
    }

    use super::*;
    struct MockPersistentFactory {
        fail_append: bool,
        fail_preview: usize,
        malformed: bool,
        starts: Arc<std::sync::atomic::AtomicUsize>,
    }

    struct MockPersistent {
        cpu: Stroke,
        fail_append: bool,
        fail_preview: usize,
        previews: usize,
        malformed: bool,
    }

    impl crate::paint_accel::PersistentFactory for MockPersistentFactory {
        fn start(
            &self,
            base: &Raster,
            opacity: f32,
        ) -> Option<Box<dyn crate::paint_accel::PersistentStroke>> {
            self.starts
                .fetch_add(1, std::sync::atomic::Ordering::Relaxed);
            Some(Box::new(MockPersistent {
                cpu: Stroke::new_with_persistent(
                    Arc::new(base.clone()),
                    Brush {
                        opacity,
                        ..Brush::default()
                    },
                    Ink::Color([0.0; 4]),
                    None,
                    None,
                ),
                fail_append: self.fail_append,
                fail_preview: self.fail_preview,
                previews: 0,
                malformed: self.malformed,
            }))
        }
    }

    impl crate::paint_accel::PersistentStroke for MockPersistent {
        fn append(&mut self, dabs: &[crate::paint_accel::ResolvedDab]) -> bool {
            // Partial application must still recover every dab exactly once.
            for dab in dabs {
                self.cpu.brush.hardness = dab.hardness;
                self.cpu.stamp(
                    (dab.center[0], dab.center[1]),
                    dab.radius * 2.0,
                    0.0,
                    dab.flow,
                    dab.color,
                    DAffine2::IDENTITY,
                );
                if self.fail_append {
                    return false;
                }
            }
            true
        }
        fn preview(&mut self) -> Option<Vec<[u16; 4]>> {
            self.previews += 1;
            if self.previews == self.fail_preview {
                return None;
            }
            if self.malformed {
                return Some(Vec::new());
            }
            let base = self.cpu.base.clone();
            self.cpu.pending.extend(self.cpu.paint.keys().copied());
            let (image, _) = self.cpu.render_with_compositor(&base, None);
            Some(
                (0..image.height())
                    .flat_map(|y| {
                        let image = &image;
                        (0..image.width()).map(move |x| image.get(x, y))
                    })
                    .collect(),
            )
        }
    }

    fn persistent_test_pair(
        fail_append: bool,
        fail_preview: usize,
        malformed: bool,
    ) -> (Stroke, Stroke) {
        let base = Arc::new(Raster::solid(320, 256, [0.05, 0.1, 0.15, 0.5]));
        let brush = Brush {
            size: 400.0,
            hardness: 0.2,
            flow: 0.3,
            opacity: 0.6,
            ..Brush::default()
        };
        let ink = Ink::Color([0.4, 0.1, 0.2, 0.7]);
        let factory = MockPersistentFactory {
            fail_append,
            fail_preview,
            malformed,
            starts: Arc::new(std::sync::atomic::AtomicUsize::new(0)),
        };
        (
            Stroke::new_with_persistent(
                base.clone(),
                brush,
                ink.clone(),
                None,
                Some(Arc::new(factory)),
            ),
            Stroke::new_with_persistent(base, brush, ink, None, None),
        )
    }

    fn assert_same_raster(a: &Raster, b: &Raster) {
        for y in 0..a.height() {
            for x in 0..a.width() {
                assert_eq!(a.get(x, y), b.get(x, y), "pixel {x},{y}");
            }
        }
    }

    #[test]
    fn persistent_brush_selects_supported_types_only() {
        let variants = [
            Brush {
                size: 399.0,
                ..Brush::default()
            },
            Brush {
                size: 400.0,
                wetness: 0.1,
                ..Brush::default()
            },
            Brush {
                size: 400.0,
                tip: 12,
                ..Brush::default()
            },
            Brush {
                size: 400.0,
                grain: GrainKind::Halftone,
                ..Brush::default()
            },
            Brush {
                size: 400.0,
                taper_end: 10.0,
                ..Brush::default()
            },
            Brush {
                size: 400.0,
                tilt: 0.5,
                ..Brush::default()
            },
            Brush {
                size: 400.0,
                blend: BrushBlend::Multiply,
                ..Brush::default()
            },
        ];
        for brush in variants {
            let (mut selected, _) = persistent_test_pair(false, 0, false);
            selected.brush = brush;
            selected.point(120.0, 120.0);
            assert!(!selected.uses_persistent(), "{brush:?}");
            assert!(!selected.paint.is_empty());
        }
        for feature in 0..6 {
            let (mut selected, _) = persistent_test_pair(false, 0, false);
            match feature {
                0 => selected.set_alpha_lock(true),
                1 => selected.set_mirror(Some(120.0), None),
                2 => selected.set_radial((120.0, 120.0), 2),
                3 => selected.clip = Some(Arc::new(|_, _| 1.0)),
                4 => selected.ink = Ink::Erase,
                _ => selected.set_symmetry_space(DAffine2::from_scale(glam::dvec2(2.0, 1.0))),
            }
            selected.point(120.0, 120.0);
            assert!(!selected.uses_persistent());
        }
        let (mut selected, _) = persistent_test_pair(false, 0, false);
        selected.point(120.0, 120.0);
        assert!(selected.uses_persistent());
        assert!(
            selected.paint.is_empty(),
            "successful routing skips CPU rasterization"
        );
    }

    #[test]
    fn persistent_brush_recovers_failed_and_partial_batches() {
        for (fail_append, fail_preview, malformed) in [
            (true, 0, false),
            (false, 1, false),
            (false, 2, false),
            (false, 0, true),
        ] {
            let (mut accelerated, mut cpu) =
                persistent_test_pair(fail_append, fail_preview, malformed);
            let mut live = (*cpu.base).clone();
            let mut reference = live.clone();
            for (x, y) in [(90.0, 90.0), (170.0, 130.0), (240.0, 170.0)] {
                accelerated.point(x, y);
                cpu.point(x, y);
                live = accelerated.render(&live).0;
                reference = cpu.render_with_compositor(&reference, None).0;
                assert_same_raster(&live, &reference);
            }
            assert!(!accelerated.uses_persistent());
        }
    }

    #[test]
    fn persistent_brush_preview_coverage_replay_and_cancel_preserve_base() {
        let (mut accelerated, mut cpu) = persistent_test_pair(false, 0, false);
        let original = accelerated.base.clone();
        accelerated.point(80.0, 80.0);
        cpu.point(80.0, 80.0);
        accelerated.point(220.0, 140.0);
        cpu.point(220.0, 140.0);
        let mask = accelerated.coverage();
        let reference_mask = cpu.coverage();
        for y in 0..mask.height() {
            for x in 0..mask.width() {
                assert_eq!(mask.get(x, y), reference_mask.get(x, y));
            }
        }
        assert!(accelerated.uses_persistent());
        let (live, dirty) = accelerated.render(&original);
        let (reference, expected_dirty) = cpu.render_with_compositor(&original, None);
        assert_eq!(dirty, expected_dirty);
        assert_same_raster(&live, &reference);
        assert!(accelerated.render(&live).1.is_empty());
        accelerated.replay(&[(5.0, 5.0)], 1.0);
        cpu.replay(&[(5.0, 5.0)], 1.0);
        assert_same_raster(
            &accelerated.render(&live).0,
            &cpu.render_with_compositor(&reference, None).0,
        );
        assert!(!accelerated.uses_persistent());
        // Cancellation restores the immutable original, with no backend-owned mutations.
        assert_same_raster(&original, &Raster::solid(320, 256, [0.05, 0.1, 0.15, 0.5]));
    }

    #[test]
    fn persistent_brush_mutation_and_explicit_cpu_render_materialize_journal() {
        for use_setter in [false, true] {
            let (mut accelerated, mut cpu) = persistent_test_pair(false, 0, false);
            accelerated.point(90.0, 90.0);
            cpu.point(90.0, 90.0);
            if use_setter {
                accelerated.set_alpha_lock(true);
                cpu.set_alpha_lock(true);
            } else {
                accelerated.brush.hardness = 0.9;
                cpu.brush.hardness = 0.9;
                accelerated.brush.wetness = 0.5;
                cpu.brush.wetness = 0.5;
            }
            accelerated.point(240.0, 150.0);
            cpu.point(240.0, 150.0);
            let base = cpu.base.clone();
            assert_same_raster(
                &accelerated.render(&base).0,
                &cpu.render_with_compositor(&base, None).0,
            );
            assert!(!accelerated.uses_persistent());
        }
        let (mut accelerated, mut cpu) = persistent_test_pair(false, 0, false);
        accelerated.point(90.0, 90.0);
        cpu.point(90.0, 90.0);
        let base = cpu.base.clone();
        assert_same_raster(
            &accelerated.render_with_compositor(&base, None).0,
            &cpu.render_with_compositor(&base, None).0,
        );
        assert!(!accelerated.uses_persistent());
    }

    #[test]
    fn partial_fill_keeps_implicit_solid_pixels_outside_selection() {
        let base = Raster::solid(300, 200, [1.0, 0.0, 0.0, 1.0]);
        let (filled, _) = fill_color(
            &base,
            IRect::new(20, 20, 30, 30),
            &|x, _| if x < 35 { 1.0 } else { 0.0 },
            [0.0, 0.0, 1.0, 1.0],
        );
        assert_eq!(filled.get(25, 25), [0, 0, 65535, 65535]);
        assert_eq!(filled.get(40, 25), base.get(40, 25));
        assert_eq!(filled.get(10, 10), base.get(10, 10));
        assert_eq!(filled.get(290, 190), base.get(290, 190));
    }

    #[test]
    fn fill_color_respects_coverage() {
        let base = Raster::transparent(40, 40);
        let (r, dirty) = fill_color(
            &base,
            IRect::new(0, 0, 40, 40),
            &|x, _| if x < 20 { 1.0 } else { 0.0 },
            [0.0, 1.0, 0.0, 1.0],
        );
        assert_eq!(r.get(5, 5)[1], 65535);
        assert_eq!(r.get(30, 5), [0; 4]);
        assert_eq!(dirty, IRect::new(0, 0, 40, 40));
    }

    fn opaque_red() -> Ink {
        Ink::Color([1.0, 0.0, 0.0, 1.0])
    }

    fn hard(size: f32) -> Brush {
        Brush {
            size,
            hardness: 1.0,
            ..Default::default()
        }
    }

    fn pattern_sheet(mut brush: Brush, overlap: bool) -> Raster {
        let base = Arc::new(Raster::transparent(128, 128));
        brush.size = 1000.0;
        brush.hardness = 1.0;
        brush.spacing = 0.02;
        let mut stroke = Stroke::new(base.clone(), brush, opaque_red(), None);
        stroke.point(64.0, 64.0);
        if overlap {
            stroke.point(0.0, 64.0);
            stroke.point(128.0, 64.0);
            stroke.point(0.0, 64.0);
        }
        stroke.finish();
        stroke.render(&base).0
    }

    fn ink_fraction(raster: &Raster) -> f64 {
        (0..128)
            .flat_map(|y| (0..128).map(move |x| (x, y)))
            .map(|(x, y)| raster.get(x, y)[3] as f64 / 65535.0)
            .sum::<f64>()
            / (128.0 * 128.0)
    }

    #[test]
    fn screentone_presets_deliver_named_ink_coverage() {
        for (name, expected) in [
            ("Screentone 20%", 0.2),
            ("Screentone 40%", 0.4),
            ("Screentone 60%", 0.6),
        ] {
            let brush = crate::library::find(name).unwrap().brush;
            let sheet = pattern_sheet(brush, false);
            let actual = ink_fraction(&sheet);
            assert!((actual - expected).abs() < 0.01, "{name}: actual {actual}");
            eprintln!("{name}: {:.2}% measured ink coverage", actual * 100.0);
        }
    }

    #[test]
    fn screentone_density_is_monotonic_through_empty_and_solid() {
        let mut previous = Raster::transparent(128, 128);
        for density in [0.0, 0.001, 0.01, 0.2, 0.4, 0.6, 0.8, 0.9, 0.99, 0.999, 1.0] {
            let sheet = pattern_sheet(
                Brush {
                    grain: GrainKind::Halftone,
                    grain_scale: 6.0,
                    grain_strength: density,
                    ..Brush::default()
                },
                false,
            );
            let actual = ink_fraction(&sheet);
            assert!(
                (actual - density as f64).abs() < 0.01,
                "density {density}: actual {actual}"
            );
            for y in 0..128 {
                for x in 0..128 {
                    let alpha = sheet.get(x, y)[3];
                    assert!(
                        alpha >= previous.get(x, y)[3],
                        "density {density} lost ink at {x},{y}"
                    );
                    if density == 0.0 {
                        assert_eq!(alpha, 0);
                    }
                    if density == 1.0 {
                        assert_eq!(alpha, 65535);
                    }
                }
            }
            previous = sheet;
        }
    }

    #[test]
    fn manga_patterns_are_periodic_across_negative_grid_coordinates() {
        let pitch = 6.0;
        let shift = pitch * std::f32::consts::FRAC_1_SQRT_2;
        for kind in [GrainKind::Halftone, GrainKind::Hatch, GrainKind::CrossHatch] {
            for (x, y) in [(-3.17, 2.41), (1.23, 4.56), (-7.31, -8.91)] {
                let original = pattern_coverage(kind, x, y, pitch, halftone_radius(0.4));
                for (dx, dy) in [
                    (shift, shift),
                    (shift, -shift),
                    (-shift, shift),
                    (-shift, -shift),
                ] {
                    let shifted =
                        pattern_coverage(kind, x + dx, y + dy, pitch, halftone_radius(0.4));
                    assert!(
                        (original - shifted).abs() < 1e-4,
                        "{kind:?}: shift {dx},{dy} at {x},{y}: {original} vs {shifted}"
                    );
                    assert!(
                        (grain(kind, x, y, pitch) - grain(kind, x + dx, y + dy, pitch)).abs()
                            < 1e-5
                    );
                }
            }
        }
    }

    #[test]
    fn manga_pattern_antialiasing_survives_overlapping_dabs() {
        for kind in [GrainKind::Halftone, GrainKind::Hatch, GrainKind::CrossHatch] {
            let brush = Brush {
                grain: kind,
                grain_strength: if kind == GrainKind::Halftone {
                    0.4
                } else {
                    1.0
                },
                grain_scale: 6.0,
                ..Brush::default()
            };
            let single = pattern_sheet(brush, false);
            let overlapping = pattern_sheet(brush, true);
            let mut partial = 0;
            for y in 0..128 {
                for x in 0..128 {
                    let pixel = single.get(x, y);
                    partial += usize::from(pixel[3] > 0 && pixel[3] < 65535);
                    assert_eq!(
                        pixel,
                        overlapping.get(x, y),
                        "{kind:?}: dab overlap changed pattern at {x},{y}"
                    );
                }
            }
            assert!(
                partial > 100,
                "{kind:?} needs antialiased interior pattern boundaries"
            );
        }
    }

    #[test]
    fn image_grain_replaces_manga_patterns_including_zero_strength() {
        let texture_id = textures::id_for(b"manga image grain precedence regression");
        textures::register(texture_id, textures::Texture::from_gray8(2, 1, &[0, 255]));
        for strength in [0.0, 0.4] {
            let brush = Brush {
                grain_tex: texture_id,
                grain_scale: 6.0,
                grain_strength: strength,
                ..Brush::default()
            };
            let expected = pattern_sheet(brush, false);
            let coverage = ink_fraction(&expected);
            if strength == 0.0 {
                assert_eq!(coverage, 1.0, "zero strength disables the image grain");
            } else {
                assert!(
                    coverage > 0.5 && coverage < 0.95,
                    "the reference must visibly use the image texture: {coverage}"
                );
            }
            for kind in [GrainKind::Halftone, GrainKind::Hatch, GrainKind::CrossHatch] {
                let actual = pattern_sheet(
                    Brush {
                        grain: kind,
                        ..brush
                    },
                    false,
                );
                for y in 0..128 {
                    for x in 0..128 {
                        assert_eq!(
                            actual.get(x, y),
                            expected.get(x, y),
                            "{kind:?} must defer to the image grain at strength {strength}, pixel {x},{y}"
                        );
                    }
                }
            }
        }
    }

    #[test]
    fn a_line_paints_along_its_path_only() {
        let base = Arc::new(Raster::transparent(300, 300));
        let mut s = Stroke::new(base.clone(), hard(10.0), opaque_red(), None);
        s.point(20.0, 50.0);
        s.point(280.0, 50.0);
        let (r, dirty) = s.render(&base);
        assert!(color::px_to_f(r.get(150, 50))[0] > 0.99);
        assert_eq!(r.get(150, 80), [0; 4]);
        assert!(dirty.x <= 20 && dirty.right() >= 280);
    }

    #[test]
    fn thin_round_dabs_cover_pixels_at_integer_coordinates() {
        let base = Arc::new(Raster::transparent(32, 32));
        for pressure in [1.0, 0.1] {
            let mut s = Stroke::new(
                base.clone(),
                Brush {
                    size_pressure: 1.0,
                    ..hard(1.0)
                },
                opaque_red(),
                None,
            );
            s.point_at(16.0, 16.0, Some(pressure), None);
            let (r, _) = s.render(&base);
            let alpha = r.get(16, 16)[3];
            assert!(
                alpha > 0 && alpha < 32000,
                "a thin dab has partial pixel coverage"
            );
            for (x, y) in [(15, 15), (15, 16), (16, 15)] {
                assert_eq!(r.get(x, y)[3], alpha, "coverage is symmetric");
            }
            let area = 4.0 * alpha as f32 / 65535.0;
            let radius = (pressure / 2.0_f32).max(0.3);
            assert!((area - std::f32::consts::PI * radius * radius).abs() < 0.1);
        }
    }

    #[test]
    fn pressure_and_tilt_interpolate_between_sparse_samples() {
        let base = Arc::new(Raster::transparent(240, 100));
        for tilt_dynamics in [false, true] {
            let brush = Brush {
                size_pressure: if tilt_dynamics { 0.0 } else { 1.0 },
                tilt: if tilt_dynamics { 1.0 } else { 0.0 },
                ..hard(20.0)
            };
            let draw = |segments: usize| {
                let mut stroke = Stroke::new(base.clone(), brush, opaque_red(), None);
                for i in 0..=segments {
                    let f = i as f32 / segments as f32;
                    stroke.point_full(
                        20.0 + 200.0 * f,
                        50.0,
                        Some(0.1 + 0.9 * f),
                        Some((60.0 * f, 0.0)),
                        None,
                    );
                }
                stroke.render(&base).0
            };
            let sparse = draw(1);
            let dense = draw(20);
            for x in [40, 100, 180] {
                assert_eq!(
                    band(&sparse, x),
                    band(&dense, x),
                    "sample density must not change dynamics at {x}"
                );
            }
            if !tilt_dynamics {
                assert!(band(&sparse, 40) < band(&sparse, 100));
                assert!(band(&sparse, 100) < band(&sparse, 180));
            }
        }
    }

    #[test]
    fn pressure_and_taper_shrink_dab_spacing_to_keep_tips_connected() {
        let base = Arc::new(Raster::transparent(240, 100));
        for tapered in [false, true] {
            let brush = Brush {
                size_pressure: 1.0,
                taper_start: if tapered { 100.0 } else { 0.0 },
                taper_end: if tapered { 100.0 } else { 0.0 },
                ..hard(100.0)
            };
            let mut stroke = Stroke::new(base.clone(), brush, opaque_red(), None);
            let pressure = if tapered { 0.1 } else { 0.01 };
            stroke.point_at(20.0, 50.0, Some(pressure), None);
            stroke.point_at(220.0, 50.0, Some(pressure), None);
            stroke.finish();
            let (r, _) = stroke.render(&base);
            for x in 20..220 {
                assert!(r.get(x, 50)[3] > 0, "gap at x={x}, tapered={tapered}");
            }
        }
    }

    #[test]
    fn opacity_caps_overlapping_dabs() {
        let base = Arc::new(Raster::transparent(64, 64));
        let brush = Brush {
            size: 20.0,
            hardness: 1.0,
            opacity: 0.5,
            flow: 1.0,
            spacing: 0.05,
            ..Default::default()
        };
        let mut s = Stroke::new(base.clone(), brush, opaque_red(), None);
        for _ in 0..3 {
            s.point(10.0, 32.0);
            s.point(54.0, 32.0);
        }
        let (r, _) = s.render(&base);
        let a = color::px_to_f(r.get(32, 32))[3];
        assert!((a - 0.5).abs() < 0.01, "alpha {a}");
    }

    #[test]
    fn translucent_ink_keeps_full_coverage() {
        let base = Arc::new(Raster::transparent(64, 64));
        let mut s = Stroke::new(
            base.clone(),
            hard(30.0),
            Ink::Color([0.5, 0.0, 0.0, 0.5]),
            None,
        );
        s.point(32.0, 32.0);
        assert_eq!(
            s.coverage().get(32, 32),
            255,
            "coverage is where the brush went"
        );
        let (r, _) = s.render(&base);
        let a = color::px_to_f(r.get(32, 32))[3];
        assert!(
            (a - 0.5).abs() < 0.01,
            "the paint itself is half transparent: {a}"
        );
    }

    #[test]
    fn erase_and_selection_clip() {
        let base = Arc::new(Raster::solid(64, 64, [0.0, 0.0, 1.0, 1.0]));
        let clip: Clip = Arc::new(|x, _| if x < 32 { 1.0 } else { 0.0 });
        let mut s = Stroke::new(base.clone(), hard(64.0), Ink::Erase, Some(clip));
        s.point(32.0, 32.0);
        let (r, _) = s.render(&base);
        assert_eq!(r.get(10, 32)[3], 0, "erased inside the selection");
        assert_eq!(r.get(50, 32)[3], 65535, "untouched outside it");
    }

    #[test]
    fn clone_copies_from_offset() {
        let base = Arc::new(Raster::from_fn(64, 64, [0; 4], |x, _| {
            if x < 16 {
                [65535, 0, 0, 65535]
            } else {
                [0, 0, 0, 65535]
            }
        }));
        let mut s = Stroke::new(
            base.clone(),
            hard(8.0),
            Ink::Clone { dx: -40.0, dy: 0.0 },
            None,
        );
        s.point(48.0, 32.0);
        let (r, _) = s.render(&base);
        assert_eq!(r.get(48, 32)[0], 65535, "red cloned from x - 40");
    }

    #[test]
    fn no_op_strokes_keep_dirty_empty_and_existing_tiles() {
        let base = Arc::new(Raster::solid(32, 32, [0.2, 0.1, 0.0, 0.5]));
        for (ink, alpha_lock, clip) in [
            (Ink::Erase, true, None),
            (opaque_red(), false, Some(Arc::new(|_, _| 0.0_f32) as Clip)),
        ] {
            let mut stroke = Stroke::new(base.clone(), hard(12.0), ink, clip);
            stroke.set_alpha_lock(alpha_lock);
            stroke.point(16.0, 16.0);
            let (result, dirty) = stroke.render(&base);
            assert!(dirty.is_empty());
            for (coord, tile) in base.base_tiles() {
                assert!(Arc::ptr_eq(tile, result.base_tile(*coord).unwrap()));
            }
        }
    }

    #[test]
    fn rendering_can_restore_current_pixels_to_the_stroke_base() {
        let base = Arc::new(Raster::transparent(32, 32));
        let mut stroke = Stroke::new(base.clone(), hard(12.0), opaque_red(), None);
        stroke.point(16.0, 16.0);
        let (painted, dirty) = stroke.render(&base);
        assert!(!dirty.is_empty());
        assert!(painted.get(16, 16)[3] > 0);
        // Model a live rerender that removes the stroke's previous coverage.
        // Comparing output with `base` would incorrectly skip this restoration.
        stroke.clip = Some(Arc::new(|_, _| 0.0));
        stroke.pending.extend(stroke.paint.keys().copied());
        let (restored, dirty) = stroke.render(&painted);
        assert!(!dirty.is_empty());
        assert_eq!(restored.to_srgba8(), base.to_srgba8());
    }

    #[test]
    fn alpha_lock_preserves_translucent_edges_for_paint_erase_and_clone() {
        let base = Arc::new(Raster::solid(32, 32, [0.2, 0.1, 0.0, 0.5]));
        for ink in [opaque_red(), Ink::Erase, Ink::Clone { dx: 4.0, dy: 0.0 }] {
            let mut stroke = Stroke::new(base.clone(), hard(12.0), ink, None);
            stroke.set_alpha_lock(true);
            stroke.point(16.0, 16.0);
            let (result, _) = stroke.render(&base);
            for y in 0..32 {
                for x in 0..32 {
                    assert_eq!(result.get(x, y)[3], base.get(x, y)[3]);
                }
            }
        }
        let transparent = Arc::new(Raster::transparent(32, 32));
        let mut stroke = Stroke::new(transparent.clone(), hard(12.0), opaque_red(), None);
        stroke.set_alpha_lock(true);
        stroke.point(16.0, 16.0);
        assert_eq!(stroke.render(&transparent).0.get(16, 16), [0; 4]);
    }

    #[test]
    fn healing_coverage_includes_selection_opacity_and_alpha_lock() {
        let base = Arc::new(Raster::solid(32, 32, [0.0, 0.0, 0.0, 0.5]));
        let mut stroke = Stroke::new(
            base,
            Brush {
                opacity: 0.5,
                ..hard(24.0)
            },
            opaque_red(),
            Some(Arc::new(|x, _| if x < 16 { 1.0 } else { 0.0 })),
        );
        stroke.set_alpha_lock(true);
        stroke.point(16.0, 16.0);
        let coverage = stroke.coverage();
        assert!((coverage.get(12, 16) as i32 - 64).abs() <= 1);
        assert_eq!(coverage.get(20, 16), 0);
    }

    #[test]
    fn symmetry_uses_document_axes_on_rotated_nonuniform_layers() {
        let base = Arc::new(Raster::transparent(128, 128));
        let center = dvec2(64.0, 64.0);
        let to_doc = DAffine2::from_translation(center)
            * DAffine2::from_angle(std::f64::consts::FRAC_PI_4)
            * DAffine2::from_scale(dvec2(1.5, 0.75))
            * DAffine2::from_translation(-center);
        let local = dvec2(48.0, 52.0);
        let doc = to_doc.transform_point2(local);
        let expected = to_doc
            .inverse()
            .transform_point2(dvec2(128.0 - doc.x, doc.y));
        let mut stroke = Stroke::new(base.clone(), hard(5.0), opaque_red(), None);
        stroke.set_symmetry_space(to_doc);
        stroke.set_mirror(Some(64.0), None);
        stroke.point(local.x as f32, local.y as f32);
        let (result, _) = stroke.render(&base);
        assert!(result.get(expected.x.floor() as u32, expected.y.floor() as u32)[3] > 50000);
        assert_eq!(
            result.get(80, 52)[3],
            0,
            "the former layer-space reflection must not be painted"
        );
        // Compare reflected samples in document space, including the shaped footprint.
        for delta in [dvec2(0.0, 0.0), dvec2(1.0, 0.0), dvec2(0.0, 1.0)] {
            let p = local + delta;
            let d = to_doc.transform_point2(p);
            let q = to_doc.inverse().transform_point2(dvec2(128.0 - d.x, d.y));
            assert!(result.get(q.x.floor() as u32, q.y.floor() as u32)[3] > 0);
        }
    }

    /// Width of the painted band at column x.
    fn band(r: &Raster, x: u32) -> u32 {
        (0..r.height()).filter(|y| r.get(x, *y)[3] > 32000).count() as u32
    }

    #[test]
    fn tapered_ends_are_thinner_after_finish() {
        let base = Arc::new(Raster::transparent(400, 100));
        let brush = Brush {
            taper_start: 100.0,
            taper_end: 100.0,
            spacing: 0.05,
            ..hard(20.0)
        };
        let mut s = Stroke::new(base.clone(), brush, opaque_red(), None);
        s.point(20.0, 50.0);
        s.point(380.0, 50.0);
        let (r, _) = s.render(&base);
        assert!(band(&r, 30) < band(&r, 200), "start tapers live");
        assert_eq!(
            band(&r, 370),
            band(&r, 200),
            "end is full width before finish"
        );
        assert!(s.finish());
        let (r, _) = s.render(&r);
        assert!(band(&r, 370) < band(&r, 200), "end tapers after finish");
        assert!(band(&r, 200) >= 19);
    }

    #[test]
    fn speed_reads_as_light_pressure() {
        let base = Arc::new(Raster::transparent(400, 100));
        let brush = Brush {
            speed_thins: 1.0,
            size_pressure: 1.0,
            spacing: 0.05,
            ..hard(20.0)
        };
        let slow = {
            let mut s = Stroke::new(base.clone(), brush, opaque_red(), None);
            for i in 0..=40 {
                s.point_at(20.0 + i as f32 * 8.0, 50.0, None, Some(i as f64 * 40.0));
            }
            band(&s.render(&base).0, 300)
        };
        let fast = {
            let mut s = Stroke::new(base.clone(), brush, opaque_red(), None);
            for i in 0..=40 {
                s.point_at(20.0 + i as f32 * 8.0, 50.0, None, Some(i as f64 * 1.0));
            }
            band(&s.render(&base).0, 300)
        };
        assert!(fast < slow, "fast {fast} vs slow {slow}");
        let real = {
            let mut s = Stroke::new(base.clone(), brush, opaque_red(), None);
            s.point_at(20.0, 50.0, Some(0.2), None);
            s.point_at(380.0, 50.0, Some(0.2), None);
            band(&s.render(&base).0, 300)
        };
        assert!(real < slow, "reported pressure wins over speed");
    }

    #[test]
    fn grain_breaks_up_coverage() {
        let base = Arc::new(Raster::transparent(200, 200));
        let brush = Brush {
            grain: GrainKind::Chalk,
            grain_strength: 1.0,
            grain_scale: 6.0,
            ..hard(60.0)
        };
        let mut s = Stroke::new(base.clone(), brush, opaque_red(), None);
        s.point(100.0, 100.0);
        let (r, _) = s.render(&base);
        let inside: Vec<f32> = (80..120)
            .flat_map(|y| (80..120).map(move |x| (x, y)))
            .map(|(x, y)| color::px_to_f(r.get(x, y))[3])
            .collect();
        let full = inside.iter().filter(|a| **a > 0.95).count();
        let empty = inside.iter().filter(|a| **a < 0.05).count();
        assert!(
            full > 100 && empty > 100,
            "chalk leaves both tooth and gaps: {full} full, {empty} empty of {}",
            inside.len()
        );
        assert!((1.0 - grain(GrainKind::None, 3.0, 4.0, 4.0)).abs() < 1e-6);
    }

    #[test]
    fn elliptical_tip_is_wider_along_its_angle() {
        let base = Arc::new(Raster::transparent(100, 100));
        let brush = Brush {
            roundness: 0.25,
            angle: 0.0,
            ..hard(40.0)
        };
        let mut s = Stroke::new(base.clone(), brush, opaque_red(), None);
        s.point(50.0, 50.0);
        let (r, _) = s.render(&base);
        let wide = (0..100).filter(|x| r.get(*x, 50)[3] > 32000).count();
        let tall = (0..100).filter(|y| r.get(50, *y)[3] > 32000).count();
        assert!(wide >= 38 && tall <= 12, "wide {wide} tall {tall}");
    }

    #[test]
    fn smudge_drags_colour_and_wet_paint_mixes() {
        // Left red, right blue.
        let base = Arc::new(Raster::from_fn(200, 100, [0; 4], |x, _| {
            if x < 100 {
                [65535, 0, 0, 65535]
            } else {
                [0, 0, 65535, 65535]
            }
        }));
        let mut s = Stroke::new(
            base.clone(),
            Brush {
                wetness: 0.9,
                spacing: 0.1,
                ..hard(20.0)
            },
            Ink::Smudge,
            None,
        );
        s.point(90.0, 50.0);
        s.point(140.0, 50.0);
        let (r, _) = s.render(&base);
        let p = color::px_to_f(r.get(110, 50));
        assert!(p[0] > 0.2, "red dragged into the blue: {p:?}");

        // Wet green over the red picks up red; dry green does not.
        let paint = |wet: f32| {
            let mut s = Stroke::new(
                base.clone(),
                Brush {
                    wetness: wet,
                    ..hard(20.0)
                },
                Ink::Color([0.0, 1.0, 0.0, 1.0]),
                None,
            );
            s.point(50.0, 50.0);
            color::px_to_f(s.render(&base).0.get(50, 50))
        };
        assert!(paint(0.0)[0] < 0.01);
        assert!(paint(0.8)[0] > 0.4, "{:?}", paint(0.8));
    }

    #[test]
    fn mirror_and_multiply() {
        let base = Arc::new(Raster::solid(200, 100, [1.0, 1.0, 0.0, 1.0]));
        let mut s = Stroke::new(
            base.clone(),
            Brush {
                blend: BrushBlend::Multiply,
                ..hard(10.0)
            },
            Ink::Color([0.0, 1.0, 1.0, 1.0]),
            None,
        );
        s.set_mirror(Some(100.0), None);
        s.point(40.0, 50.0);
        let (r, _) = s.render(&base);
        let a = color::px_to_f(r.get(40, 50));
        let b = color::px_to_f(r.get(160, 50));
        assert!(
            a[0] < 0.01 && a[1] > 0.99 && a[2] < 0.01,
            "cyan × yellow = green: {a:?}"
        );
        assert_eq!(a, b, "mirrored across x = 100");
    }

    #[test]
    fn replay_repaints_along_the_new_path_only() {
        let base = Arc::new(Raster::transparent(64, 64));
        let mut s = Stroke::new(base.clone(), hard(4.0), opaque_red(), None);
        for i in 0..20 {
            s.point_at(
                8.0 + i as f32 * 2.0,
                8.0 + (i as f32 * 0.9).sin() * 6.0,
                None,
                Some(i as f64 * 10.0),
            );
        }
        // Resting: the last few points sit within a pixel of each other.
        for i in 0..5 {
            s.point_at(
                46.0,
                8.0 + i as f32 * 0.2,
                None,
                Some(200.0 + i as f64 * 100.0),
            );
        }
        assert_eq!(s.raw_points().len(), 25);
        assert!(s.held_ms(700.0, 2.0) >= 490.0, "{}", s.held_ms(700.0, 2.0));
        assert!(s.held_ms(210.0, 2.0) < 20.0);
        let (before, _) = s.render(&base);
        assert!(band(&before, 20) > 0, "the wobble painted off the line");
        let line: Vec<(f32, f32)> = (0..39).map(|i| (8.0 + i as f32, 40.0)).collect();
        s.replay(&line, 1.0);
        assert!(s.is_finished());
        let (after, dirty) = s.render(&before);
        assert!(!dirty.is_empty());
        let old_rows = (0..20).filter(|y| after.get(20, *y)[3] > 32000).count();
        assert_eq!(old_rows, 0, "old paint gone");
        assert!(
            after.get(20, 40)[3] > 0 && after.get(30, 40)[3] > 0,
            "new line present"
        );
        s.point_at(0.0, 0.0, None, None);
        let (_, d2) = s.render(&after);
        assert!(d2.is_empty(), "locked after replay");
    }

    #[test]
    fn radial_symmetry_and_tilt() {
        let base = Arc::new(Raster::transparent(200, 200));
        let mut s = Stroke::new(base.clone(), hard(6.0), opaque_red(), None);
        s.set_radial((100.0, 100.0), 4);
        s.point(100.0, 40.0);
        s.point(100.0, 60.0);
        let (r, _) = s.render(&base);
        // The stroke above centre appears right, below and left of it too.
        assert!(r.get(100, 50)[3] > 0);
        assert!(r.get(150, 100)[3] > 0, "rotated 90°");
        assert!(r.get(100, 150)[3] > 0, "rotated 180°");
        assert!(r.get(50, 100)[3] > 0, "rotated 270°");
        assert_eq!(r.get(140, 140)[3], 0);

        let mut b = hard(10.0);
        b.tilt = 1.0;
        let mut upright = Stroke::new(base.clone(), b, opaque_red(), None);
        upright.point_full(100.0, 100.0, Some(1.0), Some((0.0, 0.0)), None);
        let (u, _) = upright.render(&base);
        let mut leaning = Stroke::new(base.clone(), b, opaque_red(), None);
        leaning.point_full(100.0, 100.0, Some(1.0), Some((60.0, 0.0)), None);
        let (l, _) = leaning.render(&base);
        let width = |r: &Raster, y: u32| (0..200).filter(|x| r.get(*x, y)[3] > 32000).count();
        let height = |r: &Raster, x: u32| (0..200).filter(|y| r.get(x, *y)[3] > 32000).count();
        assert!(
            width(&l, 100) > width(&u, 100) + 4,
            "{} vs {}",
            width(&l, 100),
            width(&u, 100)
        );
        assert!(
            height(&l, 100) < height(&u, 100),
            "flattened across the tilt"
        );
    }

    #[test]
    fn stabilizer_catches_up_on_finish() {
        let base = Arc::new(Raster::transparent(300, 100));
        let mut s = Stroke::new(
            base.clone(),
            Brush {
                stabilizer: 0.9,
                ..hard(10.0)
            },
            opaque_red(),
            None,
        );
        s.point(20.0, 50.0);
        s.point(200.0, 50.0);
        let (r, _) = s.render(&base);
        assert_eq!(r.get(195, 50)[3], 0, "the line lags behind the pointer");
        assert!(s.finish());
        let (r, _) = s.render(&r);
        assert!(
            r.get(195, 50)[3] > 0,
            "and reaches it when the pointer lifts"
        );
    }
}

/// Grey images used as brush tips and grains, shared by id so a `Brush`
/// stays a small `Copy` value. Ids are assigned by whoever imports them
/// (a hash of the image), and persist as PNGs in the brush library.
pub mod textures {
    use std::collections::HashMap;
    use std::sync::{Arc, Mutex, OnceLock};

    /// A grey texture, 0–1 per pixel.
    pub struct Texture {
        pub width: u32,
        pub height: u32,
        pub data: Vec<f32>,
    }

    impl Texture {
        /// From 8-bit grey or alpha; `data.len() == w * h`.
        pub fn from_gray8(width: u32, height: u32, data: &[u8]) -> Texture {
            Texture {
                width,
                height,
                data: data.iter().map(|v| *v as f32 / 255.0).collect(),
            }
        }

        /// Bilinear sample at `u, v` in 0–1.
        pub fn sample(&self, u: f32, v: f32) -> f32 {
            let x = u * self.width as f32 - 0.5;
            let y = v * self.height as f32 - 0.5;
            let (x0, y0) = (x.floor(), y.floor());
            let (fx, fy) = (x - x0, y - y0);
            let px = |xi: i32, yi: i32| -> f32 {
                let xi = xi.clamp(0, self.width as i32 - 1) as usize;
                let yi = yi.clamp(0, self.height as i32 - 1) as usize;
                self.data[yi * self.width as usize + xi]
            };
            let (x0, y0) = (x0 as i32, y0 as i32);
            let top = px(x0, y0) + (px(x0 + 1, y0) - px(x0, y0)) * fx;
            let bot = px(x0, y0 + 1) + (px(x0 + 1, y0 + 1) - px(x0, y0 + 1)) * fx;
            top + (bot - top) * fy
        }

        /// Sample with wrap-around, `x, y` in texture pixels.
        pub fn tiled(&self, x: f32, y: f32) -> f32 {
            let u = (x / self.width as f32).rem_euclid(1.0);
            let v = (y / self.height as f32).rem_euclid(1.0);
            self.sample(u, v)
        }
    }

    fn registry() -> &'static Mutex<HashMap<u32, Arc<Texture>>> {
        static R: OnceLock<Mutex<HashMap<u32, Arc<Texture>>>> = OnceLock::new();
        R.get_or_init(Default::default)
    }

    /// Register a texture under `id` (0 is reserved for "none").
    pub fn register(id: u32, t: Texture) {
        if id != 0 {
            registry()
                .lock()
                .unwrap_or_else(|e| e.into_inner())
                .insert(id, Arc::new(t));
        }
    }

    pub fn get(id: u32) -> Option<Arc<Texture>> {
        if id == 0 {
            return None;
        }
        registry()
            .lock()
            .unwrap_or_else(|e| e.into_inner())
            .get(&id)
            .cloned()
    }

    /// A stable id for image bytes.
    pub fn id_for(bytes: &[u8]) -> u32 {
        let mut h: u32 = 0x811c_9dc5;
        for b in bytes {
            h ^= *b as u32;
            h = h.wrapping_mul(0x0100_0193);
        }
        h.max(1)
    }

    #[cfg(test)]
    mod tests {
        use super::*;

        #[test]
        fn textures_sample_and_tile() {
            let t = Texture::from_gray8(2, 1, &[0, 255]);
            assert!((t.sample(0.25, 0.5) - 0.0).abs() < 1e-6);
            assert!((t.sample(0.75, 0.5) - 1.0).abs() < 1e-6);
            assert!((t.sample(0.5, 0.5) - 0.5).abs() < 1e-6);
            assert!(
                (t.tiled(2.5, 0.5) - 0.0).abs() < 1e-6 && (t.tiled(3.5, 0.5) - 1.0).abs() < 1e-6
            );
            let id = id_for(b"abc");
            register(id, t);
            assert!(get(id).is_some() && get(0).is_none());
        }
    }
}
