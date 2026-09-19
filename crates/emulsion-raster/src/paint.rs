//! Brush strokes.
//!
//! A stroke stamps round dabs along its path into a coverage buffer. Flow
//! builds coverage up where dabs overlap; opacity caps the whole stroke, so a
//! stroke at 50 % opacity never darkens past 50 % however often it crosses
//! itself. The painted pixels are always recomputed from the layer as it was
//! when the stroke began, which keeps the live preview exact and cheap.

use crate::color;
use crate::geom::{IRect, TileCoord};
use crate::image::{Mask, Raster};
use crate::tile::{TILE, TILE_PX};
use serde::{Deserialize, Serialize};
use std::collections::{HashMap, HashSet};
use std::sync::Arc;

#[derive(Clone, Copy, Debug, PartialEq, Serialize, Deserialize)]
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
}

impl Default for Brush {
    fn default() -> Self {
        Self {
            size: 40.0,
            hardness: 0.8,
            opacity: 1.0,
            flow: 1.0,
            spacing: 0.12,
        }
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
}

/// Selection coverage for a layer pixel, 0–1.
pub type Clip = Arc<dyn Fn(i32, i32) -> f32 + Send + Sync>;

pub struct Stroke {
    pub brush: Brush,
    ink: Ink,
    base: Arc<Raster>,
    clip: Option<Clip>,
    cov: HashMap<TileCoord, Vec<f32>>,
    last: Option<(f32, f32)>,
    carry: f32,
    pending: HashSet<TileCoord>,
    /// Layer-space bounds of everything painted so far.
    pub touched: IRect,
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

impl Stroke {
    pub fn new(base: Arc<Raster>, brush: Brush, ink: Ink, clip: Option<Clip>) -> Self {
        Self {
            brush,
            ink,
            base,
            clip,
            cov: HashMap::new(),
            last: None,
            carry: 0.0,
            pending: HashSet::new(),
            touched: IRect::default(),
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

    fn dab(&mut self, cx: f32, cy: f32) {
        let r = (self.brush.size / 2.0).max(0.5);
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
        let flow = self.brush.flow.clamp(0.0, 1.0);
        for ty in b.y.div_euclid(t)..=(b.bottom() - 1).div_euclid(t) {
            for tx in b.x.div_euclid(t)..=(b.right() - 1).div_euclid(t) {
                let c = TileCoord::new(tx, ty);
                let tile = self.cov.entry(c).or_insert_with(|| vec![0.0; TILE_PX]);
                let tr = IRect::new(tx * t, ty * t, t, t).intersect(&b);
                for y in tr.y..tr.bottom() {
                    for x in tr.x..tr.right() {
                        let d = ((x as f32 + 0.5 - cx).powi(2) + (y as f32 + 0.5 - cy).powi(2))
                            .sqrt()
                            / r;
                        let a = falloff(d, self.brush.hardness) * flow;
                        if a > 0.0 {
                            let v = &mut tile[((y - ty * t) * t + (x - tx * t)) as usize];
                            *v += (1.0 - *v) * a;
                        }
                    }
                }
                self.pending.insert(c);
            }
        }
    }

    /// Continue the stroke to (x, y) in layer pixels.
    pub fn point(&mut self, x: f32, y: f32) {
        let step = (self.brush.size * self.brush.spacing).max(0.5);
        match self.last {
            None => {
                self.dab(x, y);
                self.carry = 0.0;
            }
            Some((lx, ly)) => {
                let (dx, dy) = (x - lx, y - ly);
                let len = (dx * dx + dy * dy).sqrt();
                let mut d = step - self.carry;
                while d <= len {
                    let t = d / len;
                    self.dab(lx + dx * t, ly + dy * t);
                    d += step;
                }
                self.carry = len - (d - step);
            }
        }
        self.last = Some((x, y));
    }

    /// Apply everything painted since the last call to `current` (which must
    /// descend from the stroke's base). Returns the new layer and the
    /// layer-space rectangle that changed.
    pub fn render(&mut self, current: &Raster) -> (Raster, IRect) {
        let t = TILE as i32;
        let mut changes = Vec::new();
        let mut dirty = IRect::default();
        let opacity = self.brush.opacity.clamp(0.0, 1.0);
        for c in std::mem::take(&mut self.pending) {
            let cov = &self.cov[&c];
            let src = self
                .base
                .base_tile(c)
                .map(|t| t.to_vec())
                .unwrap_or_else(|| vec![[0u16; 4]; TILE_PX]);
            let mut out = src.clone();
            for (i, k) in cov.iter().enumerate() {
                if *k <= 0.0 {
                    continue;
                }
                let (x, y) = (c.x * t + (i as i32 % t), c.y * t + (i as i32 / t));
                let mut k = k.min(1.0) * opacity;
                if let Some(clip) = &self.clip {
                    k *= clip(x, y);
                }
                if k <= 0.0 {
                    continue;
                }
                let b = color::px_to_f(src[i]);
                let o = match &self.ink {
                    Ink::Color(p) => [0, 1, 2, 3].map(|ch| p[ch] * k + b[ch] * (1.0 - k)),
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
                out[i] = color::f_to_px(o);
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
        for (c, cov) in &self.cov {
            m.set_tile(
                *c,
                cov.iter()
                    .map(|v| (v.min(1.0) * 255.0).round() as u8)
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

    #[test]
    fn a_line_paints_along_its_path_only() {
        let base = Arc::new(Raster::transparent(300, 300));
        let mut s = Stroke::new(
            base.clone(),
            Brush {
                size: 10.0,
                hardness: 1.0,
                ..Default::default()
            },
            opaque_red(),
            None,
        );
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
    fn erase_and_selection_clip() {
        let base = Arc::new(Raster::solid(64, 64, [0.0, 0.0, 1.0, 1.0]));
        let clip: Clip = Arc::new(|x, _| if x < 32 { 1.0 } else { 0.0 });
        let mut s = Stroke::new(
            base.clone(),
            Brush {
                size: 64.0,
                hardness: 1.0,
                ..Default::default()
            },
            Ink::Erase,
            Some(clip),
        );
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
            Brush {
                size: 8.0,
                hardness: 1.0,
                ..Default::default()
            },
            Ink::Clone { dx: -40.0, dy: 0.0 },
            None,
        );
        s.point(48.0, 32.0);
        let (r, _) = s.render(&base);
        assert_eq!(r.get(48, 32)[0], 65535, "red cloned from x - 40");
    }
}
