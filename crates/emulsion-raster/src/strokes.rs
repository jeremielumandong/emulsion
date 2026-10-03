//! Vector stroke layers: pencil lines kept as editable centrelines whose width
//! and opacity vary point by point, plus filled regions painted under them.
//!
//! Each segment renders as a tapered capsule with its width and opacity
//! interpolated along it; a stroke takes the strongest coverage of its
//! segments, so joints never double up. Fills use the vector fill rasterizer.
use crate::color;
use crate::geom::IRect;
use crate::image::Raster;
use crate::quickshape::Shape;
use crate::vector::{Pt, fill_coverage};
use glam::DAffine2;
use rayon::prelude::*;
use serde::{Deserialize, Serialize};

pub const MAX_STROKES: usize = 20_000;
pub const MAX_POINTS: usize = 500_000;
pub const MAX_FILLS: usize = 10_000;
pub const MAX_WIDTH: f32 = 2000.;

fn one() -> f32 {
    1.
}

/// A centreline point. `width` scales the stroke's width (pressure) and
/// `opacity` its colour's alpha, both from 0 to 1 or more for width.
#[derive(Clone, Copy, Debug, PartialEq, Serialize, Deserialize)]
pub struct StrokePoint {
    pub x: f64,
    pub y: f64,
    #[serde(default = "one")]
    pub width: f32,
    #[serde(default = "one")]
    pub opacity: f32,
}

impl StrokePoint {
    pub fn new(x: f64, y: f64) -> Self {
        Self {
            x,
            y,
            width: 1.,
            opacity: 1.,
        }
    }
    fn pt(&self) -> Pt {
        (self.x, self.y)
    }
}

#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
pub struct Stroke {
    pub points: Vec<StrokePoint>,
    /// Straight sRGB colour with alpha.
    pub color: [u8; 4],
    /// Full width in pixels where a point's `width` is 1.
    pub width: f32,
    #[serde(default)]
    pub closed: bool,
}

/// A filled area, as closed outlines filled with the non-zero rule.
#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
pub struct StrokeFill {
    pub outlines: Vec<Vec<Pt>>,
    pub color: [u8; 4],
}

/// Everything a vector stroke layer draws: fills first, then strokes in order.
#[derive(Clone, Debug, Default, PartialEq, Serialize, Deserialize)]
pub struct StrokeSet {
    pub strokes: Vec<Stroke>,
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    pub fills: Vec<StrokeFill>,
}

/// How the retouch brush changes the strokes it passes over.
#[derive(Clone, Copy, Debug, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum Retouch {
    Thicker,
    Thinner,
    Opaquer,
    Fainter,
    Smooth,
}

fn finite(p: Pt) -> bool {
    p.0.is_finite() && p.1.is_finite()
}

impl Stroke {
    pub fn new(color: [u8; 4], width: f32) -> Self {
        Self {
            points: Vec::new(),
            color,
            width,
            closed: false,
        }
    }
    fn radius(&self, point: &StrokePoint) -> f64 {
        f64::from(self.width * point.width) / 2.
    }
    /// Bounds of the drawn stroke, in pixels.
    pub fn bounds(&self) -> Option<IRect> {
        let mut out: Option<(f64, f64, f64, f64)> = None;
        for p in &self.points {
            let r = self.radius(p) + 1.;
            let b = (p.x - r, p.y - r, p.x + r, p.y + r);
            out = Some(match out {
                None => b,
                Some(o) => (o.0.min(b.0), o.1.min(b.1), o.2.max(b.2), o.3.max(b.3)),
            });
        }
        out.map(|(x0, y0, x1, y1)| {
            IRect::new(
                x0.floor() as i32,
                y0.floor() as i32,
                (x1.ceil() - x0.floor()) as i32,
                (y1.ceil() - y0.floor()) as i32,
            )
        })
    }
    /// A stroke through the vertices of a QuickShape shape (line, polyline,
    /// polygon) or around its outline (circle, ellipse), so the Line,
    /// Rectangle, Ellipse and Polyline tools draw editable centrelines.
    pub fn from_shape(shape: &Shape, color: [u8; 4], width: f32) -> Self {
        let (points, closed): (Vec<(f32, f32)>, bool) = match shape {
            Shape::Line(a, b) => (vec![*a, *b], false),
            Shape::Polyline(v) => (v.clone(), false),
            Shape::Polygon(v) => (v.clone(), true),
            Shape::Circle { .. } | Shape::Ellipse { .. } => {
                let mut ring = shape.outline(1., (0., 0.));
                // Enough points for a smooth curve, and no more.
                let length: f32 = ring
                    .windows(2)
                    .map(|w| (w[1].0 - w[0].0).hypot(w[1].1 - w[0].1))
                    .sum();
                let n = ((length / 6.).ceil() as usize).clamp(16, 256);
                let step = ring.len() as f32 / n as f32;
                ring = (0..n)
                    .map(|i| ring[((i as f32 * step) as usize).min(ring.len() - 1)])
                    .collect();
                (ring, true)
            }
        };
        Self {
            points: points
                .into_iter()
                .map(|(x, y)| StrokePoint::new(f64::from(x), f64::from(y)))
                .collect(),
            closed,
            ..Self::new(color, width)
        }
    }

    /// Map the centreline through `m`; the width scales with its area.
    pub fn transform(&mut self, m: DAffine2) {
        let scale = m.matrix2.determinant().abs().sqrt() as f32;
        self.width = (self.width * scale).min(MAX_WIDTH);
        for p in &mut self.points {
            let q = m.transform_point2(glam::dvec2(p.x, p.y));
            (p.x, p.y) = (q.x, q.y);
        }
    }

    fn segments(&self) -> impl Iterator<Item = (StrokePoint, StrokePoint)> + '_ {
        let n = self.points.len();
        let closing = if self.closed && n > 2 {
            n
        } else {
            n.saturating_sub(1)
        };
        (0..closing.max(usize::from(n == 1))).map(move |i| {
            let a = self.points[i];
            (a, self.points.get(i + 1).copied().unwrap_or(self.points[0]))
        })
    }
    /// Distance from `pt` to the drawn stroke's edge (negative inside).
    pub fn distance(&self, pt: Pt) -> f64 {
        self.segments()
            .map(|(a, b)| {
                let (t, d) = project(pt, a.pt(), b.pt());
                d - lerp(self.radius(&a), self.radius(&b), t)
            })
            .fold(f64::INFINITY, f64::min)
    }

    /// The points of the stroke, closed strokes opened at their first
    /// point, marked inside or outside the circle, with points added where
    /// segments cross its edge (outside) and between (inside), so a split
    /// follows the eraser and not the spacing of the points.
    fn cut_by_circle(&self, center: Pt, radius: f64) -> Vec<(StrokePoint, bool)> {
        let inside = |p: &StrokePoint| (p.x - center.0).hypot(p.y - center.1) <= radius;
        let mut out = Vec::with_capacity(self.points.len() + 4);
        let at = |a: StrokePoint, b: StrokePoint, t: f64| StrokePoint {
            x: lerp(a.x, b.x, t),
            y: lerp(a.y, b.y, t),
            width: lerp(f64::from(a.width), f64::from(b.width), t) as f32,
            opacity: lerp(f64::from(a.opacity), f64::from(b.opacity), t) as f32,
        };
        let n = self.points.len();
        let closed = self.closed && n > 2;
        let segments = if closed { n } else { n.saturating_sub(1) };
        for i in 0..n {
            let a = self.points[i];
            out.push((a, inside(&a)));
            if i >= segments {
                continue;
            }
            let b = self.points[(i + 1) % n];
            // Solve |a + t(b - a) - c| = r for t in (0, 1).
            let (dx, dy) = (b.x - a.x, b.y - a.y);
            let (fx, fy) = (a.x - center.0, a.y - center.1);
            let qa = dx * dx + dy * dy;
            let qb = 2. * (fx * dx + fy * dy);
            let qc = fx * fx + fy * fy - radius * radius;
            let disc = qb * qb - 4. * qa * qc;
            if qa == 0. || disc <= 0. {
                continue;
            }
            let root = disc.sqrt();
            let (t0, t1) = ((-qb - root) / (2. * qa), (-qb + root) / (2. * qa));
            let (lo, hi) = (t0.max(0.), t1.min(1.));
            if lo >= hi {
                continue;
            }
            if t0 > 0. {
                out.push((at(a, b, t0), false));
            }
            out.push((at(a, b, (lo + hi) / 2.), true));
            if t1 < 1. {
                out.push((at(a, b, t1), false));
            }
        }
        if closed {
            // Opened at the first point, which now also ends it.
            out.push((self.points[0], inside(&self.points[0])));
        }
        out
    }

    /// Laplacian smoothing that keeps the ends of open strokes in place.
    pub fn smooth(&mut self, strength: f64, iterations: u32) {
        let strength = strength.clamp(0., 1.);
        let n = self.points.len();
        if n < 3 {
            return;
        }
        for _ in 0..iterations.min(50) {
            let previous = self.points.clone();
            for i in 0..n {
                let (before, after) = match (i, self.closed) {
                    (0, false) => continue,
                    (i, false) if i == n - 1 => continue,
                    (0, true) => (n - 1, 1),
                    (i, true) if i == n - 1 => (i - 1, 0),
                    (i, _) => (i - 1, i + 1),
                };
                let (a, b) = (previous[before], previous[after]);
                let p = &mut self.points[i];
                p.x += strength * ((a.x + b.x) / 2. - p.x);
                p.y += strength * ((a.y + b.y) / 2. - p.y);
            }
        }
    }

    /// Remove points that lie within `tolerance` pixels of the line through
    /// their neighbours (Ramer–Douglas–Peucker), keeping width and opacity
    /// changes larger than `tolerance` as a fraction.
    pub fn simplify(&mut self, tolerance: f64) {
        let n = self.points.len();
        if n < 3 || tolerance.is_nan() || tolerance <= 0. {
            return;
        }
        let mut keep = vec![false; n];
        keep[0] = true;
        keep[n - 1] = true;
        let mut stack = vec![(0, n - 1)];
        while let Some((a, b)) = stack.pop() {
            let (pa, pb) = (self.points[a], self.points[b]);
            let mut worst = (0., a);
            for i in a + 1..b {
                let p = self.points[i];
                let (t, d) = project(p.pt(), pa.pt(), pb.pt());
                let dw = (f64::from(p.width) - lerp(f64::from(pa.width), f64::from(pb.width), t))
                    .abs()
                    * f64::from(self.width);
                let dop = (f64::from(p.opacity)
                    - lerp(f64::from(pa.opacity), f64::from(pb.opacity), t))
                .abs();
                let error = d.max(dw / 2.).max(if dop > tolerance.min(1.) {
                    f64::INFINITY
                } else {
                    0.
                });
                if error > worst.0 {
                    worst = (error, i);
                }
            }
            if worst.0 > tolerance {
                keep[worst.1] = true;
                stack.push((a, worst.1));
                stack.push((worst.1, b));
            }
        }
        let mut index = 0;
        self.points.retain(|_| {
            index += 1;
            keep[index - 1]
        });
    }

    /// The outline of the drawn stroke as one closed polygon (with round
    /// ends), for turning a pencil line into a filled shape.
    pub fn outline(&self) -> Vec<Pt> {
        let n = self.points.len();
        if n == 0 {
            return Vec::new();
        }
        let cap = |c: &StrokePoint, r: f64, from: f64, out: &mut Vec<Pt>| {
            for k in 0..=8 {
                // Sweep from the left edge around the outside of the end.
                let a = from - std::f64::consts::PI * f64::from(k) / 8.;
                out.push((c.x + r * a.cos(), c.y + r * a.sin()));
            }
        };
        if n == 1 {
            let mut out = Vec::new();
            cap(&self.points[0], self.radius(&self.points[0]), 0., &mut out);
            cap(
                &self.points[0],
                self.radius(&self.points[0]),
                std::f64::consts::PI,
                &mut out,
            );
            return out;
        }
        let normal = |i: usize| -> (f64, f64) {
            let a = self.points[i.saturating_sub(1)];
            let b = self.points[(i + 1).min(n - 1)];
            let (dx, dy) = (b.x - a.x, b.y - a.y);
            let len = dx.hypot(dy);
            if len == 0. {
                (0., 0.)
            } else {
                (-dy / len, dx / len)
            }
        };
        let mut left = Vec::with_capacity(n);
        let mut right = Vec::with_capacity(n);
        for (i, p) in self.points.iter().enumerate() {
            let (nx, ny) = normal(i);
            let r = self.radius(p);
            left.push((p.x + nx * r, p.y + ny * r));
            right.push((p.x - nx * r, p.y - ny * r));
        }
        let mut out = left;
        let (last, first) = (self.points[n - 1], self.points[0]);
        let end = normal(n - 1);
        cap(&last, self.radius(&last), end.1.atan2(end.0), &mut out);
        out.pop();
        right.reverse();
        out.extend(right);
        let start = normal(0);
        cap(
            &first,
            self.radius(&first),
            (-start.1).atan2(-start.0),
            &mut out,
        );
        out.pop();
        out
    }
}

impl StrokeSet {
    pub fn point_count(&self) -> usize {
        self.strokes.iter().map(|s| s.points.len()).sum()
    }

    pub fn validate(&self) -> Result<(), String> {
        if self.strokes.len() > MAX_STROKES
            || self.point_count() > MAX_POINTS
            || self.fills.len() > MAX_FILLS
        {
            return Err(format!(
                "A stroke layer holds at most {MAX_STROKES} strokes, {MAX_POINTS} points and {MAX_FILLS} fills."
            ));
        }
        for stroke in &self.strokes {
            if stroke.points.is_empty() {
                return Err("A stroke needs at least one point.".into());
            }
            if !stroke.width.is_finite() || !(0. ..=MAX_WIDTH).contains(&stroke.width) {
                return Err(format!("Stroke width must be 0–{MAX_WIDTH} pixels."));
            }
            for p in &stroke.points {
                if !finite(p.pt())
                    || !p.width.is_finite()
                    || !(0. ..=8.).contains(&p.width)
                    || !p.opacity.is_finite()
                    || !(0. ..=1.).contains(&p.opacity)
                {
                    return Err(
                        "Stroke points need finite positions, width 0–8 and opacity 0–1.".into(),
                    );
                }
            }
        }
        for fill in &self.fills {
            if fill.outlines.iter().flatten().any(|p| !finite(*p))
                || fill.outlines.iter().map(Vec::len).sum::<usize>() > MAX_POINTS
            {
                return Err("Fill outlines need finite points.".into());
            }
        }
        Ok(())
    }

    /// Bounds of everything drawn, in pixels.
    pub fn bounds(&self) -> Option<IRect> {
        let mut out: Option<IRect> = None;
        let mut add = |r: IRect| {
            out = Some(match out {
                None => r,
                Some(o) => o.union(&r),
            })
        };
        for stroke in &self.strokes {
            if let Some(b) = stroke.bounds() {
                add(b);
            }
        }
        for p in self.fills.iter().flat_map(|f| f.outlines.iter().flatten()) {
            add(IRect::new(p.0.floor() as i32, p.1.floor() as i32, 2, 2));
        }
        out
    }

    pub fn transform(&mut self, m: DAffine2) {
        for stroke in &mut self.strokes {
            stroke.transform(m);
        }
        for p in self
            .fills
            .iter_mut()
            .flat_map(|f| f.outlines.iter_mut().flatten())
        {
            let q = m.transform_point2(glam::dvec2(p.0, p.1));
            *p = (q.x, q.y);
        }
    }

    pub fn translate(&mut self, dx: f64, dy: f64) {
        self.transform(DAffine2::from_translation(glam::dvec2(dx, dy)));
    }

    /// The topmost stroke within `tolerance` pixels of `pt`.
    pub fn hit(&self, pt: Pt, tolerance: f64) -> Option<usize> {
        self.strokes
            .iter()
            .rposition(|s| s.distance(pt) <= tolerance)
    }

    /// Erase the parts of strokes within `radius` of `center`, splitting
    /// strokes where the eraser crosses them, even between two points.
    /// Returns whether anything changed.
    pub fn erase(&mut self, center: Pt, radius: f64) -> bool {
        if radius.is_nan() || radius <= 0. || !finite(center) {
            return false;
        }
        let mut changed = false;
        let mut out = Vec::with_capacity(self.strokes.len());
        for stroke in self.strokes.drain(..) {
            let marked = stroke.cut_by_circle(center, radius);
            if !marked.iter().any(|(_, inside)| *inside) {
                out.push(stroke);
                continue;
            }
            changed = true;
            let wraps = stroke.closed && !marked[0].1;
            let mut runs: Vec<Vec<StrokePoint>> = vec![Vec::new()];
            for (p, inside) in marked {
                if !inside {
                    runs.last_mut().unwrap().push(p);
                } else if !runs.last().unwrap().is_empty() {
                    runs.push(Vec::new());
                }
            }
            if wraps && runs.len() > 1 && !runs.last().unwrap().is_empty() {
                // A closed stroke opens where it was cut: its last run goes
                // on through the first point into its first run.
                let mut last = runs.pop().unwrap();
                last.extend(runs[0].drain(1..));
                runs[0] = last;
            }
            out.extend(
                runs.into_iter()
                    .filter(|r| !r.is_empty())
                    .map(|points| Stroke {
                        points,
                        closed: false,
                        ..stroke.clone()
                    }),
            );
        }
        self.strokes = out;
        changed
    }

    /// Bounds of the centrelines of `indices` (minimum and maximum corner).
    pub fn centreline_bounds(&self, indices: &[usize]) -> Option<(Pt, Pt)> {
        indices
            .iter()
            .filter_map(|i| self.strokes.get(*i))
            .flat_map(|s| &s.points)
            .fold(None, |b: Option<(Pt, Pt)>, p| {
                Some(match b {
                    None => (p.pt(), p.pt()),
                    Some((lo, hi)) => (
                        (lo.0.min(p.x), lo.1.min(p.y)),
                        (hi.0.max(p.x), hi.1.max(p.y)),
                    ),
                })
            })
    }

    /// Strokes with a centreline point inside the rectangle from `a` to `b`
    /// or a segment crossing it: what a marquee drag selects.
    pub fn in_rect(&self, a: Pt, b: Pt) -> Vec<usize> {
        let (lo, hi) = ((a.0.min(b.0), a.1.min(b.1)), (a.0.max(b.0), a.1.max(b.1)));
        let inside = |p: Pt| p.0 >= lo.0 && p.0 <= hi.0 && p.1 >= lo.1 && p.1 <= hi.1;
        let edges = [
            (lo, (hi.0, lo.1)),
            ((hi.0, lo.1), hi),
            (hi, (lo.0, hi.1)),
            ((lo.0, hi.1), lo),
        ];
        (0..self.strokes.len())
            .filter(|&i| {
                self.strokes[i].segments().any(|(p, q)| {
                    inside(p.pt())
                        || inside(q.pt())
                        || edges
                            .iter()
                            .any(|(e0, e1)| crosses(p.pt(), q.pt(), *e0, *e1))
                })
            })
            .collect()
    }

    /// The centreline point of one of `among` nearest to `pt`, within
    /// `tolerance` pixels: (stroke, point).
    pub fn point_near(&self, pt: Pt, tolerance: f64, among: &[usize]) -> Option<(usize, usize)> {
        among
            .iter()
            .filter_map(|&i| Some((i, self.strokes.get(i)?)))
            .flat_map(|(i, s)| s.points.iter().enumerate().map(move |(j, p)| (i, j, p)))
            .map(|(i, j, p)| ((p.x - pt.0).hypot(p.y - pt.1), i, j))
            .filter(|(d, _, _)| *d <= tolerance)
            .min_by(|a, b| a.0.total_cmp(&b.0))
            .map(|(_, i, j)| (i, j))
    }

    /// The pencil-retouch brush: change width or opacity, or smooth, for the
    /// points within `radius` of `center`, strongest at the centre. Returns
    /// whether anything changed.
    pub fn retouch(&mut self, center: Pt, radius: f64, mode: Retouch, amount: f32) -> bool {
        if radius.is_nan() || radius <= 0. || !finite(center) {
            return false;
        }
        let amount = amount.clamp(0., 1.);
        let mut changed = false;
        for stroke in &mut self.strokes {
            let weights: Vec<f32> = stroke
                .points
                .iter()
                .map(|p| {
                    let d = (p.x - center.0).hypot(p.y - center.1);
                    (1. - d / radius).max(0.) as f32
                })
                .collect();
            if weights.iter().all(|w| *w == 0.) {
                continue;
            }
            changed = true;
            if mode == Retouch::Smooth {
                let mut smoothed = stroke.clone();
                smoothed.smooth(f64::from(amount), 1);
                for ((p, s), w) in stroke.points.iter_mut().zip(&smoothed.points).zip(&weights) {
                    p.x += (s.x - p.x) * f64::from(*w);
                    p.y += (s.y - p.y) * f64::from(*w);
                }
                continue;
            }
            for (p, w) in stroke.points.iter_mut().zip(&weights) {
                let k = 1. + amount * w;
                match mode {
                    Retouch::Thicker => p.width = (p.width * k).min(8.),
                    Retouch::Thinner => p.width /= k,
                    Retouch::Opaquer => p.opacity = (p.opacity * k).min(1.),
                    Retouch::Fainter => p.opacity /= k,
                    Retouch::Smooth => unreachable!(),
                }
            }
        }
        changed
    }

    /// Turn strokes into fills of their outlines (pencil line to brush
    /// shape), keeping their colour; the strokes are removed.
    pub fn outline_strokes(&mut self, indices: &[usize]) {
        let mut chosen: Vec<usize> = indices
            .iter()
            .copied()
            .filter(|i| *i < self.strokes.len())
            .collect();
        chosen.sort_unstable();
        chosen.dedup();
        for i in chosen.into_iter().rev() {
            let stroke = self.strokes.remove(i);
            self.fills.push(StrokeFill {
                outlines: vec![stroke.outline()],
                color: stroke.color,
            });
        }
    }

    /// Draw onto a transparent layer of `w` × `h`.
    pub fn rasterize(&self, w: u32, h: u32) -> Raster {
        let empty = Raster::transparent(w, h);
        let canvas = IRect::new(0, 0, w as i32, h as i32);
        let Some(b) = self.bounds().map(|b| b.intersect(&canvas)) else {
            return empty;
        };
        if b.is_empty() {
            return empty;
        }
        let mut out = vec![[0f32; 4]; b.w as usize * b.h as usize];
        for fill in &self.fills {
            let mask = fill_coverage(&fill.outlines, w, h);
            let color = linear(fill.color);
            for row in 0..b.h {
                for col in 0..b.w {
                    let k = f32::from(mask.get((b.x + col) as u32, (b.y + row) as u32)) / 255.;
                    over(&mut out[(row * b.w + col) as usize], color, k);
                }
            }
        }
        for stroke in &self.strokes {
            let Some(sb) = stroke.bounds().map(|s| s.intersect(&b)) else {
                continue;
            };
            if sb.is_empty() {
                continue;
            }
            let coverage = stroke_coverage(stroke, sb);
            let color = linear(stroke.color);
            for row in 0..sb.h {
                for col in 0..sb.w {
                    let k = coverage[(row * sb.w + col) as usize];
                    if k > 0. {
                        let index = ((sb.y - b.y + row) * b.w + sb.x - b.x + col) as usize;
                        over(&mut out[index], color, k);
                    }
                }
            }
        }
        let px: Vec<_> = out.into_iter().map(color::f_to_px).collect();
        empty.write_rect(b, &px)
    }
}

/// Coverage of one stroke over `area`, each segment's strongest value.
fn stroke_coverage(stroke: &Stroke, area: IRect) -> Vec<f32> {
    let mut coverage = vec![0f32; area.w as usize * area.h as usize];
    let segments: Vec<_> = stroke.segments().collect();
    coverage
        .par_chunks_mut(area.w as usize)
        .enumerate()
        .for_each(|(row, line)| {
            let y = f64::from(area.y) + row as f64 + 0.5;
            for (a, b) in &segments {
                let (ra, rb) = (stroke.radius(a), stroke.radius(b));
                let reach = ra.max(rb) + 1.;
                if y < a.y.min(b.y) - reach || y > a.y.max(b.y) + reach {
                    continue;
                }
                let x0 = ((a.x.min(b.x) - reach).floor() as i32 - area.x).max(0);
                let x1 = ((a.x.max(b.x) + reach).ceil() as i32 - area.x).min(area.w);
                for col in x0..x1 {
                    let x = f64::from(area.x + col) + 0.5;
                    let (t, d) = project((x, y), a.pt(), b.pt());
                    let r = lerp(ra, rb, t);
                    let cover = (r - d + 0.5).clamp(0., 1.).min(r * 2.);
                    let alpha =
                        cover as f32 * lerp(f64::from(a.opacity), f64::from(b.opacity), t) as f32;
                    let cell = &mut line[col as usize];
                    *cell = cell.max(alpha);
                }
            }
        });
    coverage
}

/// Do segments `a`–`b` and `c`–`d` intersect?
fn crosses(a: Pt, b: Pt, c: Pt, d: Pt) -> bool {
    let side = |p: Pt, q: Pt, r: Pt| (q.0 - p.0) * (r.1 - p.1) - (q.1 - p.1) * (r.0 - p.0);
    let (d1, d2) = (side(c, d, a), side(c, d, b));
    let (d3, d4) = (side(a, b, c), side(a, b, d));
    d1 * d2 < 0. && d3 * d4 < 0.
}

/// Where `p` projects onto segment `a`–`b` (0–1) and its distance from it.
fn project(p: Pt, a: Pt, b: Pt) -> (f64, f64) {
    let (dx, dy) = (b.0 - a.0, b.1 - a.1);
    let len2 = dx * dx + dy * dy;
    let t = if len2 == 0. {
        0.
    } else {
        (((p.0 - a.0) * dx + (p.1 - a.1) * dy) / len2).clamp(0., 1.)
    };
    let (cx, cy) = (a.0 + t * dx, a.1 + t * dy);
    (t, (p.0 - cx).hypot(p.1 - cy))
}

fn lerp(a: f64, b: f64, t: f64) -> f64 {
    a + (b - a) * t
}

/// Straight sRGB to linear premultiplied.
fn linear(c: [u8; 4]) -> [f32; 4] {
    let a = f32::from(c[3]) / 255.;
    [
        color::srgb_to_linear(f32::from(c[0]) / 255.) * a,
        color::srgb_to_linear(f32::from(c[1]) / 255.) * a,
        color::srgb_to_linear(f32::from(c[2]) / 255.) * a,
        a,
    ]
}

fn over(dst: &mut [f32; 4], src: [f32; 4], k: f32) {
    for i in 0..4 {
        dst[i] = src[i] * k + dst[i] * (1. - src[3] * k);
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn line(points: &[(f64, f64, f32)], width: f32) -> Stroke {
        Stroke {
            points: points
                .iter()
                .map(|&(x, y, w)| StrokePoint {
                    width: w,
                    ..StrokePoint::new(x, y)
                })
                .collect(),
            ..Stroke::new([0, 0, 0, 255], width)
        }
    }

    #[test]
    fn pressure_changes_the_drawn_width_along_a_stroke() {
        let set = StrokeSet {
            strokes: vec![line(&[(10., 20., 0.2), (90., 20., 1.)], 10.)],
            fills: Vec::new(),
        };
        set.validate().unwrap();
        let r = set.rasterize(100, 40);
        let alpha = |x, y| r.get(x, y)[3];
        // Thin near the start, full width near the end.
        assert!(alpha(12, 20) > 60000 && alpha(12, 24) == 0);
        assert!(alpha(88, 24) > 60000 && alpha(88, 27) == 0);
        assert_eq!(alpha(5, 20), 0);
    }

    #[test]
    fn joints_do_not_darken_and_opacity_varies() {
        let mut stroke = line(&[(10., 10., 1.), (30., 10., 1.), (30., 30., 1.)], 6.);
        stroke.color = [0, 0, 0, 255];
        stroke.points[0].opacity = 0.5;
        let set = StrokeSet {
            strokes: vec![stroke],
            fills: Vec::new(),
        };
        let r = set.rasterize(40, 40);
        let corner = r.get(30, 10)[3];
        let middle = r.get(30, 20)[3];
        assert_eq!(corner, middle, "segments meet without doubling");
        assert!(r.get(11, 10)[3] < r.get(28, 10)[3]);
    }

    #[test]
    fn fills_draw_under_strokes_and_outlines_cover_the_stroke() {
        let mut set = StrokeSet {
            strokes: vec![line(&[(5., 20., 1.), (35., 20., 1.)], 8.)],
            fills: vec![StrokeFill {
                outlines: vec![vec![(0., 0.), (40., 0.), (40., 40.), (0., 40.)]],
                color: [255, 0, 0, 255],
            }],
        };
        let r = set.rasterize(40, 40);
        assert!(r.get(2, 2)[0] > 60000 && r.get(20, 20)[0] < 1000);
        set.fills.clear();
        let before = set.rasterize(40, 40);
        set.outline_strokes(&[0]);
        assert!(set.strokes.is_empty() && set.fills.len() == 1);
        let after = set.rasterize(40, 40);
        for (x, y) in [(20, 20), (20, 23), (6, 20), (34, 20)] {
            assert!(before.get(x, y)[3] > 60000, "{x},{y}");
            assert!(after.get(x, y)[3] > 50000, "{x},{y}");
        }
        assert_eq!(after.get(20, 30)[3], 0);
    }

    #[test]
    fn smoothing_simplifying_and_erasing_edit_centrelines() {
        let mut zig = line(
            &[
                (0., 0., 1.),
                (10., 4., 1.),
                (20., 0., 1.),
                (30., 4., 1.),
                (40., 0., 1.),
            ],
            2.,
        );
        let before = zig.points[2].y;
        zig.smooth(1., 3);
        assert_eq!(zig.points[0].pt(), (0., 0.));
        assert!(zig.points[2].y > before);
        let mut straight = line(
            &(0..20).map(|i| (f64::from(i), 0., 1.)).collect::<Vec<_>>(),
            2.,
        );
        straight.simplify(0.1);
        assert_eq!(straight.points.len(), 2);
        let mut set = StrokeSet {
            strokes: vec![line(
                &(0..=10)
                    .map(|i| (f64::from(i) * 10., 0., 1.))
                    .collect::<Vec<_>>(),
                2.,
            )],
            fills: Vec::new(),
        };
        assert!(set.erase((50., 0.), 5.));
        assert_eq!(set.strokes.len(), 2);
        assert_eq!(set.hit((20., 1.), 1.), Some(0));
        assert_eq!(set.hit((50., 0.), 1.), None);
    }

    #[test]
    fn retouch_changes_points_near_the_brush_only() {
        let mut set = StrokeSet {
            strokes: vec![line(&[(0., 0., 1.), (50., 0., 1.), (100., 0., 1.)], 4.)],
            fills: Vec::new(),
        };
        assert!(set.retouch((50., 0.), 10., Retouch::Thicker, 1.));
        let points = &set.strokes[0].points;
        assert!(points[1].width > 1.9 && points[0].width == 1.);
        assert!(set.retouch((0., 0.), 10., Retouch::Fainter, 1.));
        assert!(set.strokes[0].points[0].opacity < 0.6);
        assert!(!set.retouch((500., 500.), 10., Retouch::Thinner, 1.));
    }

    #[test]
    fn shapes_become_strokes_and_the_eraser_cuts_between_points() {
        let red = [255, 0, 0, 255];
        let line = Stroke::from_shape(&Shape::Line((0., 10.), (100., 10.)), red, 4.);
        assert_eq!(line.points.len(), 2);
        assert!(!line.closed);
        let rect = Stroke::from_shape(
            &Shape::Polygon(vec![(10., 10.), (50., 10.), (50., 40.), (10., 40.)]),
            red,
            2.,
        );
        assert!(rect.closed && rect.points.len() == 4);
        let ellipse = Stroke::from_shape(
            &Shape::Ellipse {
                center: (50., 50.),
                radii: (30., 20.),
                angle: 0.,
            },
            red,
            2.,
        );
        assert!(ellipse.closed && ellipse.points.len() >= 16);
        for p in &ellipse.points {
            let e = ((p.x - 50.) / 30.).powi(2) + ((p.y - 50.) / 20.).powi(2);
            assert!((e - 1.).abs() < 0.05, "{p:?}");
        }
        // Two points 100 px apart: the eraser in the middle still splits.
        let mut set = StrokeSet {
            strokes: vec![line, rect],
            fills: Vec::new(),
        };
        set.validate().unwrap();
        assert!(set.erase((50., 10.), 5.));
        assert_eq!(set.strokes.len(), 3, "line in two, rectangle opened");
        let (left, right) = (&set.strokes[0], &set.strokes[1]);
        assert!((left.points.last().unwrap().x - 45.).abs() < 1e-9);
        assert!((right.points[0].x - 55.).abs() < 1e-9);
        let opened = &set.strokes[2];
        assert!(!opened.closed);
        assert!(
            opened
                .points
                .iter()
                .all(|p| (p.x - 50.).hypot(p.y - 10.) >= 5. - 1e-9)
        );
        assert!(!set.erase((500., 500.), 5.));
    }

    #[test]
    fn selection_helpers_find_strokes_points_and_bounds() {
        let mut set = StrokeSet {
            strokes: vec![
                line(&[(0., 0., 1.), (100., 0., 1.)], 2.),
                line(&[(10., 50., 1.), (20., 60., 1.)], 2.),
            ],
            fills: Vec::new(),
        };
        // A marquee crossed by a long segment selects it.
        assert_eq!(set.in_rect((40., -5.), (60., 5.)), vec![0]);
        assert_eq!(set.in_rect((0., 40.), (30., 70.)), vec![1]);
        assert!(set.in_rect((200., 200.), (210., 210.)).is_empty());
        assert_eq!(set.point_near((19., 59.), 3., &[0, 1]), Some((1, 1)));
        assert_eq!(set.point_near((19., 59.), 3., &[0]), None);
        assert_eq!(
            set.centreline_bounds(&[0, 1]),
            Some(((0., 0.), (100., 60.)))
        );
        set.strokes[1].transform(DAffine2::from_translation(glam::dvec2(5., 0.)));
        assert_eq!(set.strokes[1].points[0].pt(), (15., 50.));
        assert_eq!(set.strokes[0].points[0].pt(), (0., 0.));
    }

    #[test]
    fn transforms_scale_widths_and_validation_rejects_bad_data() {
        let mut set = StrokeSet {
            strokes: vec![line(&[(1., 1., 1.)], 4.)],
            fills: Vec::new(),
        };
        set.transform(DAffine2::from_scale(glam::dvec2(2., 2.)));
        assert_eq!(set.strokes[0].width, 8.);
        assert_eq!(set.strokes[0].points[0].pt(), (2., 2.));
        set.strokes[0].points[0].x = f64::NAN;
        assert!(set.validate().is_err());
        let json = r#"{"strokes":[{"points":[{"x":1,"y":2}],"color":[0,0,0,255],"width":3}]}"#;
        let set: StrokeSet = serde_json::from_str(json).unwrap();
        assert_eq!(set.strokes[0].points[0].opacity, 1.);
        set.validate().unwrap();
    }
}
