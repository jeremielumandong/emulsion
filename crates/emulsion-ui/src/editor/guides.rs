//! Drawing guides (Procreate's Drawing Guide): a 2D grid, an isometric
//! grid, or one-, two- or three-point perspective drawn over the canvas,
//! with optional Drawing Assist that locks each brush stroke to the
//! nearest guide direction. Vanishing points are dragged on the canvas.

use super::*;

#[derive(Clone, Debug, PartialEq)]
pub enum GuideKind {
    Off,
    /// Square grid, spacing in document pixels.
    Grid {
        size: f64,
    },
    /// 30° isometric grid, spacing in document pixels.
    Isometric {
        size: f64,
    },
    /// One to three vanishing points, in document pixels (may lie off
    /// the canvas).
    Perspective {
        points: Vec<(f64, f64)>,
    },
}

#[derive(Clone, Debug)]
pub struct GuideState {
    pub kind: GuideKind,
    /// Drawing Assist: strokes follow the guide's directions.
    pub assist: bool,
    /// The direction the current stroke is locked to: origin and unit
    /// direction in document pixels. `None` until the hand has moved far
    /// enough to tell.
    pub lock: Option<((f64, f64), (f64, f64))>,
    /// Where the current stroke began, for choosing the lock.
    pub start: Option<(f64, f64)>,
}

impl Default for GuideState {
    fn default() -> Self {
        Self {
            kind: GuideKind::Off,
            assist: false,
            lock: None,
            start: None,
        }
    }
}

/// A document-space polyline.
pub(crate) type Polyline = Vec<(f64, f64)>;

/// Screen pixels within which a vanishing point can be grabbed.
const HANDLE_PX: f64 = 10.0;
/// How far (screen px) the hand moves before a stroke's direction is chosen.
const LOCK_PX: f64 = 8.0;

fn norm(v: (f64, f64)) -> (f64, f64) {
    let l = v.0.hypot(v.1);
    if l < 1e-9 {
        (1.0, 0.0)
    } else {
        (v.0 / l, v.1 / l)
    }
}

impl GuideKind {
    pub fn label(&self) -> &'static str {
        match self {
            GuideKind::Off => "guide",
            GuideKind::Grid { .. } => "grid",
            GuideKind::Isometric { .. } => "isometric",
            GuideKind::Perspective { points } => match points.len() {
                1 => "1-point",
                2 => "2-point",
                _ => "3-point",
            },
        }
    }

    /// Default guides for a canvas of this size, in the order the chip
    /// cycles through them.
    pub fn cycle(&self, w: f64, h: f64) -> GuideKind {
        let step = (w.min(h) / 12.0).round().max(8.0);
        match self {
            GuideKind::Off => GuideKind::Grid { size: step },
            GuideKind::Grid { .. } => GuideKind::Isometric { size: step },
            GuideKind::Isometric { .. } => GuideKind::Perspective {
                points: vec![(w / 2.0, h / 2.0)],
            },
            GuideKind::Perspective { points } if points.len() == 1 => GuideKind::Perspective {
                points: vec![(-w * 0.3, h * 0.45), (w * 1.3, h * 0.45)],
            },
            GuideKind::Perspective { points } if points.len() == 2 => GuideKind::Perspective {
                points: vec![(-w * 0.3, h * 0.4), (w * 1.3, h * 0.4), (w / 2.0, h * 2.6)],
            },
            GuideKind::Perspective { .. } => GuideKind::Off,
        }
    }

    /// Directions a stroke starting at `at` may follow (unit vectors).
    pub fn directions(&self, at: (f64, f64)) -> Vec<(f64, f64)> {
        match self {
            GuideKind::Off => Vec::new(),
            GuideKind::Grid { .. } => vec![(1.0, 0.0), (0.0, 1.0)],
            GuideKind::Isometric { .. } => {
                let (c, s) = (30f64.to_radians().cos(), 30f64.to_radians().sin());
                vec![(c, -s), (c, s), (0.0, 1.0)]
            }
            GuideKind::Perspective { points } => {
                let mut v: Vec<(f64, f64)> = points
                    .iter()
                    .map(|p| norm((p.0 - at.0, p.1 - at.1)))
                    .collect();
                // Verticals stay vertical until a third point takes over.
                if points.len() < 3 {
                    v.push((0.0, 1.0));
                }
                if points.len() < 2 {
                    v.push((1.0, 0.0));
                }
                v
            }
        }
    }

    /// The guide as document-space polylines over a `w`×`h` canvas.
    pub fn lines(&self, w: f64, h: f64) -> Vec<Vec<(f64, f64)>> {
        let mut out = Vec::new();
        match self {
            GuideKind::Off => {}
            GuideKind::Grid { size } => {
                let s = size.max(2.0);
                let mut x = 0.0;
                while x <= w {
                    out.push(vec![(x, 0.0), (x, h)]);
                    x += s;
                }
                let mut y = 0.0;
                while y <= h {
                    out.push(vec![(0.0, y), (w, y)]);
                    y += s;
                }
            }
            GuideKind::Isometric { size } => {
                let s = size.max(2.0);
                let t = 30f64.to_radians().tan();
                // Verticals.
                let mut x = 0.0;
                while x <= w {
                    out.push(vec![(x, 0.0), (x, h)]);
                    x += s;
                }
                // Two families of 30° lines; intercepts spaced so they
                // meet the verticals in a regular rhombus lattice.
                let dy = s * t * 2.0;
                let span = w * t;
                let mut c = -span;
                while c <= h + span {
                    out.push(clip_line((0.0, c), (w, c + span), w, h));
                    out.push(clip_line((0.0, c + span), (w, c), w, h));
                    c += dy;
                }
                out.retain(|l| l.len() == 2);
            }
            GuideKind::Perspective { points } => {
                let n = 24usize;
                for p in points {
                    // Rays from the vanishing point through points spread
                    // along the canvas edges.
                    let mut targets = Vec::with_capacity(n * 2);
                    for i in 0..=n {
                        let f = i as f64 / n as f64;
                        targets.push((w * f, 0.0));
                        targets.push((w * f, h));
                        targets.push((0.0, h * f));
                        targets.push((w, h * f));
                    }
                    for t in targets {
                        let d = norm((t.0 - p.0, t.1 - p.1));
                        let far = (p.0 + d.0 * (w + h) * 4.0, p.1 + d.1 * (w + h) * 4.0);
                        let l = clip_line(*p, far, w, h);
                        if l.len() == 2 {
                            out.push(l);
                        }
                    }
                }
                if points.len() < 3 {
                    // The horizon through the vanishing points.
                    let y = points.iter().map(|p| p.1).sum::<f64>() / points.len() as f64;
                    out.push(vec![(0.0, y), (w, y)]);
                }
            }
        }
        out
    }
}

/// The part of segment `a`–`b` inside the canvas rectangle.
fn clip_line(a: (f64, f64), b: (f64, f64), w: f64, h: f64) -> Vec<(f64, f64)> {
    let (mut t0, mut t1) = (0.0f64, 1.0f64);
    let (dx, dy) = (b.0 - a.0, b.1 - a.1);
    for (p, q) in [(-dx, a.0), (dx, w - a.0), (-dy, a.1), (dy, h - a.1)] {
        if p.abs() < 1e-12 {
            if q < 0.0 {
                return Vec::new();
            }
            continue;
        }
        let r = q / p;
        if p < 0.0 {
            if r > t1 {
                return Vec::new();
            }
            t0 = t0.max(r);
        } else {
            if r < t0 {
                return Vec::new();
            }
            t1 = t1.min(r);
        }
    }
    if t0 > t1 {
        return Vec::new();
    }
    vec![
        (a.0 + dx * t0, a.1 + dy * t0),
        (a.0 + dx * t1, a.1 + dy * t1),
    ]
}

impl EditorView {
    /// Begin a stroke at `d` (document pixels): remember where, so the
    /// assist can pick a direction once the hand commits to one.
    pub(crate) fn assist_begin(&mut self, d: (f64, f64)) {
        self.tools.guide.lock = None;
        self.tools.guide.start =
            (self.tools.guide.assist && self.tools.guide.kind != GuideKind::Off).then_some(d);
    }

    /// Where a stroke point lands with Drawing Assist: on the locked guide
    /// line once the hand has shown a direction, else where it is.
    pub(crate) fn assist_point(&mut self, d: (f64, f64)) -> (f64, f64) {
        let Some(start) = self.tools.guide.start else {
            return d;
        };
        if self.tools.guide.lock.is_none() {
            let moved = (d.0 - start.0).hypot(d.1 - start.1) * self.view.zoom;
            if moved < LOCK_PX {
                return start;
            }
            let hand = norm((d.0 - start.0, d.1 - start.1));
            let dir = self
                .tools
                .guide
                .kind
                .directions(start)
                .into_iter()
                .max_by(|a, b| {
                    let da = (a.0 * hand.0 + a.1 * hand.1).abs();
                    let db = (b.0 * hand.0 + b.1 * hand.1).abs();
                    da.total_cmp(&db)
                });
            match dir {
                Some(dir) => self.tools.guide.lock = Some((start, dir)),
                None => return d,
            }
        }
        let (o, dir) = self.tools.guide.lock.unwrap();
        let t = (d.0 - o.0) * dir.0 + (d.1 - o.1) * dir.1;
        (o.0 + dir.0 * t, o.1 + dir.1 * t)
    }

    pub(crate) fn assist_end(&mut self) {
        self.tools.guide.lock = None;
        self.tools.guide.start = None;
    }

    /// The vanishing point under `pos`, if the guide has one there.
    pub(crate) fn vanishing_hit(&self, pos: Point<Pixels>) -> Option<usize> {
        let GuideKind::Perspective { points } = &self.tools.guide.kind else {
            return None;
        };
        let b = self.canvas_bounds()?;
        if !b.contains(&pos) {
            return None;
        }
        let (sx, sy) = (f32::from(pos.x) as f64, f32::from(pos.y) as f64);
        points.iter().position(|p| {
            let s = self.view.doc_to_screen(*p, &b);
            (s.0 - sx).hypot(s.1 - sy) <= HANDLE_PX
        })
    }

    pub(crate) fn move_vanishing(&mut self, i: usize, d: (f64, f64)) {
        if let GuideKind::Perspective { points } = &mut self.tools.guide.kind
            && let Some(p) = points.get_mut(i)
        {
            *p = d;
        }
    }

    /// Guide geometry for the overlay: lines and vanishing-point handles.
    pub(crate) fn guide_overlay(&self) -> (Vec<Polyline>, Vec<(f64, f64)>) {
        let (w, h) = (self.editor.doc.width as f64, self.editor.doc.height as f64);
        let mut lines = self.tools.guide.kind.lines(w, h);
        // Symmetry axes show while mirroring or radial symmetry is on.
        if self.tools.mirror_x {
            lines.push(vec![(w / 2.0, 0.0), (w / 2.0, h)]);
        }
        if self.tools.mirror_y {
            lines.push(vec![(0.0, h / 2.0), (w, h / 2.0)]);
        }
        if self.tools.symmetry >= 2 {
            let n = self.tools.symmetry;
            let r = w.hypot(h);
            for k in 0..n {
                let a = k as f64 / n as f64 * std::f64::consts::TAU - std::f64::consts::FRAC_PI_2;
                let l = clip_line(
                    (w / 2.0, h / 2.0),
                    (w / 2.0 + a.cos() * r, h / 2.0 + a.sin() * r),
                    w,
                    h,
                );
                if l.len() == 2 {
                    lines.push(l);
                }
            }
        }
        let handles = match &self.tools.guide.kind {
            GuideKind::Perspective { points } => points.clone(),
            _ => Vec::new(),
        };
        (lines, handles)
    }
}

#[cfg(test)]
mod tests {
    use super::{GuideKind, clip_line};

    #[test]
    fn guides_cycle_and_draw_inside_the_canvas() {
        let mut k = GuideKind::Off;
        let mut seen = Vec::new();
        for _ in 0..6 {
            k = k.cycle(400.0, 300.0);
            seen.push(k.label());
            for l in k.lines(400.0, 300.0) {
                for (x, y) in l {
                    assert!((-1e-6..=400.0 + 1e-6).contains(&x), "{x}");
                    assert!((-1e-6..=300.0 + 1e-6).contains(&y), "{y}");
                }
            }
        }
        assert_eq!(
            seen,
            [
                "grid",
                "isometric",
                "1-point",
                "2-point",
                "3-point",
                "guide"
            ]
        );
        let iso = GuideKind::Isometric { size: 40.0 };
        let d = iso.directions((0.0, 0.0));
        assert_eq!(d.len(), 3);
        assert!((d[0].1 + 0.5).abs() < 1e-9, "30° up: {:?}", d[0]);
        let p = GuideKind::Perspective {
            points: vec![(200.0, 150.0)],
        };
        let d = p.directions((0.0, 150.0));
        assert!((d[0].0 - 1.0).abs() < 1e-9 && d[0].1.abs() < 1e-9);
        assert_eq!(
            clip_line((-10.0, 5.0), (50.0, 5.0), 40.0, 40.0),
            vec![(0.0, 5.0), (40.0, 5.0)]
        );
        assert!(clip_line((-10.0, -5.0), (50.0, -5.0), 40.0, 40.0).is_empty());
    }
}
