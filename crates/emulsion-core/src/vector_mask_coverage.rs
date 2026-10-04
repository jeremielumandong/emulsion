//! Bounded nonzero fill directly from cubic scanline crossings. A fixed global
//! polyline tessellation cannot resolve small visible portions of very large
//! off-canvas curves; solving monotonic crossings retains their local shape.
use emulsion_raster::{
    IRect, Mask,
    vector::{Path, Pt},
};
use glam::{DAffine2, DVec2, dvec2};

#[derive(Clone, Copy)]
enum Edge {
    Line {
        a: Pt,
        b: Pt,
    },
    Cubic {
        points: [Pt; 4],
        interval: [f64; 2],
        y: [f64; 2],
        direction: i32,
    },
}

pub(super) struct CoveragePath {
    // At most three monotonic pieces per authored cubic, bounded by MAX_ANCHORS.
    edges: Vec<Edge>,
    low: DVec2,
    high: DVec2,
}

/// Convex interpolation avoids overflowing b-a for large opposite coordinates.
fn mix(a: f64, b: f64, t: f64) -> f64 {
    a * (1.0 - t) + b * t
}
fn cubic(values: [f64; 4], t: f64) -> f64 {
    let a = mix(values[0], values[1], t);
    let b = mix(values[1], values[2], t);
    let c = mix(values[2], values[3], t);
    mix(mix(a, b, t), mix(b, c, t), t)
}

fn extrema(values: [f64; 4]) -> Vec<f64> {
    // Rescaling bounds the derivative coefficients without changing its roots.
    let scale = values.iter().map(|v| v.abs()).fold(1.0, f64::max);
    let [p0, p1, p2, p3] = values.map(|v| v / scale);
    let a = -p0 + 3.0 * p1 - 3.0 * p2 + p3;
    let b = 2.0 * (p0 - 2.0 * p1 + p2);
    let c = p1 - p0;
    let mut roots = Vec::with_capacity(2);
    if a == 0.0 {
        if b != 0.0 {
            roots.push(-c / b);
        }
    } else {
        let discriminant = b * b - 4.0 * a * c;
        if discriminant >= 0.0 {
            let q = -0.5 * (b + discriminant.sqrt().copysign(b));
            if q == 0.0 {
                roots.push(-b / (2.0 * a));
            } else {
                roots.push(q / a);
                roots.push(c / q);
            }
        }
    }
    roots.retain(|t| t.is_finite() && *t > 0.0 && *t < 1.0);
    roots.sort_by(f64::total_cmp);
    roots.dedup();
    roots
}

impl CoveragePath {
    pub(super) fn new(path: &Path, affine: DAffine2) -> Self {
        let mut out = Self {
            edges: Vec::new(),
            low: DVec2::splat(f64::INFINITY),
            high: DVec2::splat(f64::NEG_INFINITY),
        };
        let point = |p: Pt| {
            let p = affine.transform_point2(dvec2(p.0, p.1));
            (p.x, p.y)
        };
        for subpath in &path.subpaths {
            if subpath.anchors.len() < 2 {
                continue;
            }
            for pair in subpath.anchors.windows(2) {
                out.push(
                    [
                        point(pair[0].p),
                        point(pair[0].h_out),
                        point(pair[1].h_in),
                        point(pair[1].p),
                    ],
                    pair[0].p == pair[0].h_out && pair[1].h_in == pair[1].p,
                );
            }
            let first = &subpath.anchors[0];
            let last = subpath.anchors.last().unwrap();
            if subpath.closed {
                out.push(
                    [
                        point(last.p),
                        point(last.h_out),
                        point(first.h_in),
                        point(first.p),
                    ],
                    last.p == last.h_out && first.h_in == first.p,
                );
            } else {
                // Open subpaths stay open in data, but their fill closes by line.
                out.push(
                    [point(last.p), point(last.p), point(first.p), point(first.p)],
                    true,
                );
            }
        }
        out
    }
    fn push(&mut self, points: [Pt; 4], line: bool) {
        for p in points {
            self.low = self.low.min(dvec2(p.0, p.1));
            self.high = self.high.max(dvec2(p.0, p.1));
        }
        if line {
            self.edges.push(Edge::Line {
                a: points[0],
                b: points[3],
            });
            return;
        }
        // Reversed copies of the same cubic must produce bit-identical roots
        // so opposite winding cancels even at an antialias sample boundary.
        let mut reversed = points;
        reversed.reverse();
        let order = points
            .iter()
            .zip(&reversed)
            .map(|(a, b)| a.0.total_cmp(&b.0).then_with(|| a.1.total_cmp(&b.1)))
            .find(|order| !order.is_eq())
            .unwrap_or(std::cmp::Ordering::Equal);
        let direction = if order.is_gt() { -1 } else { 1 };
        let points = if direction < 0 { reversed } else { points };
        let values = points.map(|p| p.1);
        let mut starts = vec![0.0];
        starts.extend(extrema(values));
        starts.push(1.0);
        for pair in starts.windows(2) {
            let interval = [pair[0], pair[1]];
            let y = interval.map(|t| cubic(values, t));
            if y[0] != y[1] {
                self.edges.push(Edge::Cubic {
                    points,
                    interval,
                    y,
                    direction,
                });
            }
        }
    }

    /// A curve stays in its control hull. If no edge hull touches this closed
    /// rectangle, winding is constant throughout it; one point determines the
    /// exact interior/exterior value. Conservative overlap returns unknown.
    pub(super) fn constant_rect(&self, low: DVec2, high: DVec2) -> (Option<u8>, u64) {
        if self.edges.is_empty()
            || high.x < self.low.x
            || high.y < self.low.y
            || low.x > self.high.x
            || low.y > self.high.y
        {
            return (Some(0), 1);
        }
        let mut work = 1u64;
        for edge in &self.edges {
            work += 1;
            let (a, b) = edge.bounds();
            if a.x <= high.x && b.x >= low.x && a.y <= high.y && b.y >= low.y {
                return (None, work);
            }
        }
        let p = low * 0.5 + high * 0.5;
        let mut winding = 0;
        for edge in &self.edges {
            work += edge.solve_cost();
            if let Some((x, direction)) = edge.crossing(p.y)
                && x <= p.x
            {
                winding += direction;
            }
        }
        (Some(if winding == 0 { 0 } else { 255 }), work)
    }
    /// Upper bound for loops used by this scan converter: sample writes, edge
    /// checks per subscanline, and at most 56 cubic evaluations per crossing.
    pub(super) fn work_for_window(&self, origin: Pt, size: (u32, u32)) -> u64 {
        let rows = u64::from(size.1) * 4;
        let mut work = u64::from(size.0)
            .saturating_mul(u64::from(size.1))
            .saturating_mul(8);
        let edges = self.edges.len() as u64;
        let sort_and_partial = edges.max(1).ilog2() as u64 * 2 + 12;
        work = work.saturating_add(rows.saturating_mul(edges).saturating_mul(sort_and_partial));
        for edge in &self.edges {
            let (a, b) = edge.y_range();
            let height = b.min(origin.1 + f64::from(size.1)) - a.max(origin.1);
            if height >= 0.0 {
                let samples = (height * 4.0).ceil() as u64 + 2;
                work = work.saturating_add(samples.min(rows).saturating_mul(edge.solve_cost()));
            }
        }
        work
    }

    pub(super) fn rasterize_window(&self, origin: Pt, size: (u32, u32)) -> Mask {
        let result = Mask::empty(size.0, size.1, 0);
        if self.edges.is_empty() || size.0 == 0 || size.1 == 0 {
            return result;
        }
        // Clamp floating control bounds before converting integers. Neither
        // off-canvas hull size nor curve length controls allocation size.
        let left = (self.low.x - origin.0)
            .floor()
            .clamp(0.0, f64::from(size.0)) as i32;
        let right = (self.high.x - origin.0)
            .ceil()
            .clamp(0.0, f64::from(size.0)) as i32;
        let top = (self.low.y - origin.1)
            .floor()
            .clamp(0.0, f64::from(size.1)) as i32;
        let bottom = (self.high.y - origin.1)
            .ceil()
            .clamp(0.0, f64::from(size.1)) as i32;
        let bounds = IRect::new(left, top, right - left, bottom - top);
        if bounds.is_empty() {
            return result;
        }
        let mut coverage = vec![0u16; bounds.w as usize * bounds.h as usize];
        let mut crossings = Vec::with_capacity(self.edges.len());
        const S: usize = 4;
        for row in 0..bounds.h {
            for sy in 0..S {
                let y = origin.1 + f64::from(bounds.y + row) + (sy as f64 + 0.5) / S as f64;
                crossings.clear();
                for edge in &self.edges {
                    if let Some(crossing) = edge.crossing(y) {
                        crossings.push(crossing);
                    }
                }
                crossings.sort_by(|a, b| a.0.total_cmp(&b.0));
                let mut winding = 0i32;
                for pair in crossings.windows(2) {
                    winding += pair[0].1;
                    if winding == 0 {
                        continue;
                    }
                    let ca = (pair[0].0 - origin.0 - f64::from(bounds.x)) * S as f64;
                    let cb = (pair[1].0 - origin.0 - f64::from(bounds.x)) * S as f64;
                    // Match the existing 4×4 fill sample and byte rounding rule.
                    let start = ca.round().clamp(0.0, bounds.w as f64 * S as f64) as usize;
                    let end = cb.round().clamp(0.0, bounds.w as f64 * S as f64) as usize;
                    if start < end {
                        let base = row as usize * bounds.w as usize;
                        let (a, b) = (start / S, end / S);
                        if a == b {
                            coverage[base + a] += (end - start) as u16;
                        } else {
                            let first = if start.is_multiple_of(S) {
                                a
                            } else {
                                coverage[base + a] += (S - start % S) as u16;
                                a + 1
                            };
                            for x in first..b {
                                coverage[base + x] += S as u16;
                            }
                            if !end.is_multiple_of(S) {
                                coverage[base + b] += (end % S) as u16;
                            }
                        }
                    }
                }
            }
        }
        let values = coverage
            .into_iter()
            .map(|v| ((u32::from(v) * 255) / (S * S) as u32).min(255) as u8)
            .collect::<Vec<_>>();
        result.write_rect(bounds, &values)
    }
}

impl Edge {
    fn bounds(self) -> (DVec2, DVec2) {
        match self {
            Self::Line { a, b } => (
                dvec2(a.0.min(b.0), a.1.min(b.1)),
                dvec2(a.0.max(b.0), a.1.max(b.1)),
            ),
            Self::Cubic { points, .. } => {
                let low = points
                    .iter()
                    .fold(DVec2::splat(f64::INFINITY), |a, p| a.min(dvec2(p.0, p.1)));
                let high = points.iter().fold(DVec2::splat(f64::NEG_INFINITY), |a, p| {
                    a.max(dvec2(p.0, p.1))
                });
                (low, high)
            }
        }
    }
    fn y_range(self) -> (f64, f64) {
        match self {
            Self::Line { a, b } => (a.1.min(b.1), a.1.max(b.1)),
            Self::Cubic { y, .. } => (y[0].min(y[1]), y[0].max(y[1])),
        }
    }
    fn solve_cost(self) -> u64 {
        match self {
            Self::Line { .. } => 1,
            Self::Cubic { .. } => 57,
        }
    }
    fn crossing(self, scan_y: f64) -> Option<(f64, i32)> {
        let (x, y, direction) = match self {
            Self::Line { a, b } => {
                if (a.1 <= scan_y) == (b.1 <= scan_y) {
                    return None;
                }
                let original_y = [a.1, b.1];
                let (a, b) = if a.1 < b.1 { (a, b) } else { (b, a) };
                let delta = b.1 - a.1;
                let t = if delta.is_finite() {
                    (scan_y - a.1) / delta
                } else {
                    let scale = a.1.abs().max(b.1.abs()).max(scan_y.abs()).max(1.0);
                    (scan_y / scale - a.1 / scale) / (b.1 / scale - a.1 / scale)
                };
                (mix(a.0, b.0, t), original_y, 1)
            }
            Self::Cubic {
                points,
                interval,
                y,
                direction,
            } => {
                if (y[0] <= scan_y) == (y[1] <= scan_y) {
                    return None;
                }
                let mut lo = interval[0];
                let mut hi = interval[1];
                let increasing = y[1] > y[0];
                let values = points.map(|p| p.1);
                // Fixed work per crossing; monotonic bracketing cannot jump to
                // another root or miss a narrow extremum near the viewport.
                if scan_y == y[0] {
                    hi = lo;
                }
                if scan_y == y[1] {
                    lo = hi;
                }
                for _ in 0..56 {
                    let mid = lo + (hi - lo) * 0.5;
                    if mid == lo || mid == hi {
                        break;
                    }
                    if (cubic(values, mid) < scan_y) == increasing {
                        lo = mid;
                    } else {
                        hi = mid;
                    }
                }
                (
                    cubic(points.map(|p| p.0), lo + (hi - lo) * 0.5),
                    y,
                    direction,
                )
            }
        };
        Some((x, direction * if y[1] > y[0] { 1 } else { -1 }))
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use emulsion_raster::vector::{Anchor, SubPath};
    fn arch(radius: f64) -> Path {
        let mut a = Anchor::corner((-radius, 1.5 * radius));
        a.h_out = (-radius, -0.5 * radius);
        let mut b = Anchor::corner((radius, 1.5 * radius));
        b.h_in = (radius, -0.5 * radius);
        Path {
            subpaths: vec![SubPath {
                anchors: vec![a, b],
                closed: true,
            }],
        }
    }
    // Independent stable polynomial for this curve, centered on its extremum:
    // x=R*(3d-4d³), y=6R*d², where d=t-.5.
    fn arch_y(radius: f64, x: f64) -> f64 {
        let (mut lo, mut hi) = (-0.5, 0.5);
        for _ in 0..64 {
            let d = (lo + hi) * 0.5;
            if radius * (3.0 * d - 4.0 * d * d * d) < x {
                lo = d;
            } else {
                hi = d;
            }
        }
        let d = (lo + hi) * 0.5;
        6.0 * radius * d * d
    }
    #[test]
    fn huge_off_canvas_cubic_keeps_subpixel_local_edge_instead_of_uniform_chord() {
        let radius = 1e8;
        let path = arch(radius);
        let coverage = CoveragePath::new(&path, DAffine2::from_translation(dvec2(5000., 0.)));
        let rendered = coverage.rasterize_window((0., 0.), (32, 24));
        for y in 0..24 {
            for x in 0..32 {
                let mut count = 0;
                for sy in 0..4 {
                    for sx in 0..4 {
                        let px = f64::from(x) + (sx as f64 + 0.5) / 4.0 - 5000.;
                        let py = f64::from(y) + (sy as f64 + 0.5) / 4.0;
                        count += u32::from(py >= arch_y(radius, px));
                    }
                }
                assert_eq!(
                    rendered.get(x, y),
                    (count * 255 / 16) as u8,
                    "pixel {x},{y}"
                );
            }
        }
        assert_eq!(rendered.get(0, 0), 191);
        assert_eq!(rendered.get(0, 1), 255);
        assert!(coverage.edges.len() <= path.anchor_count() * 3);
    }
    #[test]
    fn cubic_crossings_match_independent_parabola_under_affine_and_tile_origins() {
        let (w, h) = (24.0, 18.0);
        let mut a = Anchor::corner((0., h));
        a.h_out = (w / 3., -h / 3.);
        let mut b = Anchor::corner((w, h));
        b.h_in = (2. * w / 3., -h / 3.);
        let path = Path {
            subpaths: vec![SubPath {
                anchors: vec![a, b],
                closed: true,
            }],
        };
        for affine in [
            DAffine2::IDENTITY,
            DAffine2::from_scale_angle_translation(dvec2(1.2, 0.8), 0.27, dvec2(6., 3.)),
        ] {
            let coverage = CoveragePath::new(&path, affine);
            let full = coverage.rasterize_window((0., 0.), (40, 32));
            for y in 0..32 {
                for x in 0..40 {
                    let mut count = 0;
                    for sy in 0..4 {
                        for sx in 0..4 {
                            let p = affine.inverse().transform_point2(dvec2(
                                f64::from(x) + (sx as f64 + 0.5) / 4.,
                                f64::from(y) + (sy as f64 + 0.5) / 4.,
                            ));
                            let edge = h * (2. * p.x / w - 1.).powi(2);
                            count += u32::from(p.x >= 0. && p.x < w && p.y >= edge && p.y < h);
                        }
                    }
                    assert_eq!(
                        full.get(x, y),
                        (count * 255 / 16) as u8,
                        "pixel {x},{y}, {affine:?}"
                    );
                }
            }
            let window = coverage.rasterize_window((7., 5.), (13, 11));
            for y in 0..11 {
                for x in 0..13 {
                    assert_eq!(window.get(x, y), full.get(x + 7, y + 5));
                }
            }
        }
    }
    #[test]
    fn cubic_tangent_closed_and_implicit_fill_use_nonzero_winding() {
        let mut path = arch(8.0);
        let full = CoveragePath::new(&path, DAffine2::from_translation(dvec2(8., 0.)));
        path.subpaths[0].closed = false;
        let open = CoveragePath::new(&path, DAffine2::from_translation(dvec2(8., 0.)));
        assert_eq!(
            full.rasterize_window((0., 0.), (16, 12)).to_gray8(),
            open.rasterize_window((0., 0.), (16, 12)).to_gray8()
        );
        let y = 0.0;
        let mut crossings = full
            .edges
            .iter()
            .filter_map(|e| e.crossing(y))
            .collect::<Vec<_>>();
        crossings.sort_by(|a, b| a.0.total_cmp(&b.0));
        assert_eq!(crossings.iter().map(|p| p.1).sum::<i32>(), 0);
    }
    #[test]
    fn reversed_identical_cubics_cancel_exactly_at_antialias_boundaries() {
        let mut path = arch(8.0);
        let mut reverse = path.subpaths[0].clone();
        reverse.anchors.reverse();
        for anchor in &mut reverse.anchors {
            std::mem::swap(&mut anchor.h_in, &mut anchor.h_out);
        }
        path.subpaths.push(reverse);
        for x in [0., 0.125, 0.375, 0.5, 2.25] {
            let rendered =
                CoveragePath::new(&path, DAffine2::from_translation(dvec2(x + 8., 0.125)))
                    .rasterize_window((0., 0.), (24, 16));
            assert!(rendered.to_gray8().into_iter().all(|v| v == 0));
        }
    }
}
