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
use serde::{Deserialize, Serialize};
use std::collections::{HashMap, HashSet};
use std::sync::Arc;

/// Paper texture under the brush.
#[derive(Clone, Copy, Debug, PartialEq, Eq, Default, Serialize, Deserialize)]
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
    /// How much grain shows, 0–1.
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
    /// Pigment pools at the edge of each dab, 0–1 (watercolour).
    pub edge_darken: f32,
    pub blend: BrushBlend,
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
            stabilizer: 0.0,
            size_jitter: 0.0,
            scatter: 0.0,
            color_jitter: 0.0,
            wetness: 0.0,
            edge_darken: 0.0,
            blend: BrushBlend::Normal,
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
}

/// Accumulated paint: premultiplied colour, alpha = coverage.
/// Per pixel: accumulated premultiplied ink (0–3) and coverage (4).
type PaintTile = Vec<[f32; 5]>;

pub struct Stroke {
    pub brush: Brush,
    ink: Ink,
    base: Arc<Raster>,
    clip: Option<Clip>,
    paint: HashMap<TileCoord, PaintTile>,
    pending: HashSet<TileCoord>,
    /// Layer-space bounds of everything painted so far.
    pub touched: IRect,
    /// Smoothed input path, as stamped.
    path: Vec<Sample>,
    /// Raw input for the stabilizer and speed.
    raw_last: Option<(f32, f32, f64)>,
    smooth: Option<(f32, f32)>,
    speed: f32,
    /// Stamping state along `path`.
    last: Option<(f32, f32)>,
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
        let seed = 0x5EED_0000 ^ (base.width() as u64) << 20 ^ base.height() as u64;
        Self {
            brush: brush.sanitized(),
            ink,
            base,
            clip,
            paint: HashMap::new(),
            pending: HashSet::new(),
            touched: IRect::default(),
            path: Vec::new(),
            raw_last: None,
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
            finished: false,
        }
    }

    pub fn base(&self) -> &Arc<Raster> {
        &self.base
    }

    /// For clone strokes: where to copy from, relative to the brush, in
    /// layer pixels.
    pub fn set_clone_offset(&mut self, dx: f32, dy: f32) {
        if let Ink::Clone { dx: a, dy: b } = &mut self.ink {
            *a = dx;
            *b = dy;
        }
    }

    /// Let wet brushes and smudges see the image under this layer, so they
    /// mix with the photo (or the layers below), not only with this layer.
    pub fn set_backdrop(&mut self, backdrop: Backdrop) {
        self.backdrop = Some(backdrop);
    }

    /// Also stamp every dab mirrored across x = `x` and/or y = `y`.
    pub fn set_mirror(&mut self, x: Option<f32>, y: Option<f32>) {
        self.mirror_x = x;
        self.mirror_y = y;
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

    /// Stamp one dab and its mirrors.
    fn dab(&mut self, cx: f32, cy: f32, pressure: f32, taper: f32) {
        let b = self.brush;
        let jitter = 1.0 - b.size_jitter * self.rand();
        let size = b.size * (1.0 - b.size_pressure * (1.0 - pressure)) * taper * jitter;
        let flow = b.flow * (1.0 - b.flow_pressure * (1.0 - pressure));
        let (sx, sy) = if b.scatter > 0.0 {
            let a = self.rand() * std::f32::consts::TAU;
            let d = self.rand() * b.scatter * b.size;
            (cx + a.cos() * d, cy + a.sin() * d)
        } else {
            (cx, cy)
        };
        let angle = if b.follow_path {
            b.angle + self.dir
        } else {
            b.angle
        };
        let colour = self.dab_colour(sx, sy, size / 2.0);
        self.stamp(sx, sy, size, angle, flow, colour);
        if let Some(mx) = self.mirror_x {
            self.stamp(2.0 * mx - sx, sy, size, -angle, flow, colour);
        }
        if let Some(my) = self.mirror_y {
            self.stamp(sx, 2.0 * my - sy, size, -angle, flow, colour);
            if let Some(mx) = self.mirror_x {
                self.stamp(2.0 * mx - sx, 2.0 * my - sy, size, angle, flow, colour);
            }
        }
    }

    /// The premultiplied colour this dab lays down.
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

    fn stamp(&mut self, cx: f32, cy: f32, size: f32, angle_deg: f32, flow: f32, colour: [f32; 4]) {
        let r = (size / 2.0).max(0.3);
        let b = IRect::new(
            (cx - r).floor() as i32,
            (cy - r).floor() as i32,
            (2.0 * r).ceil() as i32 + 2,
            (2.0 * r).ceil() as i32 + 2,
        )
        .intersect(&self.base.bounds());
        if b.is_empty() {
            return;
        }
        self.touched = self.touched.union(&b);
        let t = TILE as i32;
        let flow = flow.clamp(0.0, 1.0);
        let (hard, round) = (self.brush.hardness, self.brush.roundness);
        let (s, c) = (-angle_deg.to_radians()).sin_cos();
        let (gk, gs, gstr) = (
            self.brush.grain,
            self.brush.grain_scale,
            self.brush.grain_strength,
        );
        let edge = self.brush.edge_darken;
        self.dabs = self.dabs.wrapping_add(1);
        let dab_no = self.dabs;
        // Tiny tips: spread the dab over the pixel so thin lines stay continuous.
        let aa = if r < 1.0 { r } else { 1.0 };
        for ty in b.y.div_euclid(t)..=(b.bottom() - 1).div_euclid(t) {
            for tx in b.x.div_euclid(t)..=(b.right() - 1).div_euclid(t) {
                let coord = TileCoord::new(tx, ty);
                let tile = self
                    .paint
                    .entry(coord)
                    .or_insert_with(|| vec![[0.0; 5]; TILE_PX]);
                let tr = IRect::new(tx * t, ty * t, t, t).intersect(&b);
                for y in tr.y..tr.bottom() {
                    for x in tr.x..tr.right() {
                        let (ox, oy) = (x as f32 + 0.5 - cx, y as f32 + 0.5 - cy);
                        let (rx, ry) = (ox * c - oy * s, (ox * s + oy * c) / round);
                        let d = (rx * rx + ry * ry).sqrt() / r.max(0.5);
                        let mut a = falloff(d, hard) * flow * aa;
                        if a <= 0.0 {
                            continue;
                        }
                        if edge > 0.0 {
                            // Thin in the middle, pooled towards the rim.
                            let pool = 0.35 + 0.65 * d * d;
                            a *= 1.0 - edge + edge * pool * 1.3;
                        }
                        if gstr > 0.0 {
                            let g = if gk == GrainKind::Bristle {
                                bristle(ry, rx, gs, dab_no)
                            } else {
                                grain(gk, x as f32, y as f32, gs)
                            };
                            a *= 1.0 - gstr + gstr * g;
                        }
                        if a <= 0.0005 {
                            continue;
                        }
                        let p = &mut tile[((y - ty * t) * t + (x - tx * t)) as usize];
                        for i in 0..4 {
                            p[i] = colour[i] * a + p[i] * (1.0 - a);
                        }
                        p[4] = a + p[4] * (1.0 - a);
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
        });
    }

    fn advance(&mut self, s: Sample) {
        self.path.push(s);
        self.stamp_segment(s, None);
    }

    /// Stamp from the last stamped point to `s`. `total` is the stroke
    /// length when known (a finished stroke), which enables the end taper.
    fn stamp_segment(&mut self, s: Sample, total: Option<f32>) {
        let step = (self.brush.size * self.brush.spacing).max(0.5);
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
                self.dab(s.x, s.y, s.pressure, t);
                self.carry = 0.0;
            }
            Some((lx, ly)) => {
                let (dx, dy) = (s.x - lx, s.y - ly);
                let len = (dx * dx + dy * dy).sqrt();
                if len > 0.01 {
                    self.dir = dy.atan2(dx).to_degrees();
                }
                let mut d = step - self.carry;
                while d <= len {
                    let f = d / len;
                    let t = taper(self.distance + d, &self.brush);
                    self.dab(lx + dx * f, ly + dy * f, s.pressure, t);
                    d += step;
                }
                self.carry = len - (d - step);
                self.distance += len;
            }
        }
        self.last = Some((s.x, s.y));
    }

    /// The pointer lifted. Catches the stabilizer up to the last input and
    /// re-tapers the end when the brush asks for it. Returns whether the
    /// paint changed, so the caller knows to render once more.
    pub fn finish(&mut self) -> bool {
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
            // Finish the line in a few steps so the taper below sees them.
            for i in 1..=4 {
                let f = i as f32 / 4.0;
                self.advance(Sample {
                    x: sx + (rx - sx) * f,
                    y: sy + (ry - sy) * f,
                    pressure: p,
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
                tile.fill([0.0; 5]);
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
        let t = TILE as i32;
        let mut changes = Vec::new();
        let mut dirty = IRect::default();
        let opacity = self.brush.opacity.clamp(0.0, 1.0);
        let mode = match self.brush.blend {
            BrushBlend::Normal | BrushBlend::Behind => BlendMode::Normal,
            BrushBlend::Multiply => BlendMode::Multiply,
        };
        for c in std::mem::take(&mut self.pending) {
            let paint = &self.paint[&c];
            let src = self
                .base
                .base_tile(c)
                .map(|t| t.to_vec())
                .unwrap_or_else(|| vec![[0u16; 4]; TILE_PX]);
            let mut out = src.clone();
            for (i, p) in paint.iter().enumerate() {
                if p[4] <= 0.0 {
                    continue;
                }
                let (x, y) = (c.x * t + (i as i32 % t), c.y * t + (i as i32 / t));
                let mut k = p[4].min(1.0) * opacity;
                if let Some(clip) = &self.clip {
                    k *= clip(x, y);
                }
                if k <= 0.0 {
                    continue;
                }
                let b = color::px_to_f(src[i]);
                let a = p[4].max(1e-6);
                // The dab colour at full coverage, then scaled by k.
                let ink = [p[0] / a, p[1] / a, p[2] / a, p[3] / a];
                let o = match &self.ink {
                    Ink::Color(_) | Ink::Smudge => {
                        let k = if self.brush.blend == BrushBlend::Behind {
                            k * (1.0 - b[3].min(1.0))
                        } else {
                            k
                        };
                        let s = ink.map(|v| v * k);
                        blend_px(mode, BlendSpace::Linear, b, s, 0.0)
                    }
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
                        [0, 1, 2, 3].map(|ch| s[ch] * k + b[ch] * (1.0 - k))
                    }
                };
                out[i] = color::f_to_px(o.map(|v| v.clamp(0.0, 1.0)));
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
        let mut m = Mask::empty(self.base.width(), self.base.height(), 0);
        for (c, tile) in &self.paint {
            m.set_tile(
                *c,
                tile.iter()
                    .map(|p| (p[4].min(1.0) * 255.0).round() as u8)
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
                .unwrap_or_else(|| vec![[0u16; 4]; TILE_PX]);
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
    use super::*;

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
