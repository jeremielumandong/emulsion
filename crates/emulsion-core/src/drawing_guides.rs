//! Drawing guides for Drawing Assist, stored with each document (so each
//! storyboard panel keeps its own): a grid, an isometric grid, 1-, 2- or
//! 3-point perspective, 4- or 5-point curvilinear (fish-eye) perspective,
//! a straight-edge ruler, and named guide sets that can be switched.
//!
//! The guide geometry lives here so the canvas, the overlay and MCP agree:
//! [`GuideKind::lines`] draws a guide, and [`GuideKind::curves`] lists the
//! lines and circles a stroke starting at a point may follow.

use serde::{Deserialize, Serialize};

pub type Pt = (f64, f64);

/// Most guides shown at once, and in one saved set.
pub const MAX_GUIDES: usize = 16;
/// Most saved guide sets per document.
pub const MAX_GUIDE_SETS: usize = 64;

#[derive(Clone, Debug, Default, PartialEq, Serialize, Deserialize)]
#[serde(tag = "kind", rename_all = "snake_case")]
pub enum GuideKind {
    #[default]
    Off,
    /// Square grid, spacing in document pixels.
    Grid { size: f64 },
    /// 30° isometric grid, spacing in document pixels.
    Isometric { size: f64 },
    /// One to three vanishing points, in document pixels (may lie off
    /// the canvas).
    Perspective { points: Vec<Pt> },
    /// Curvilinear (fish-eye) perspective: vanishing points left, right,
    /// above and below `center` at `radius`; lines bend into circular arcs
    /// through opposite points. With `five`, lines also radiate from the
    /// centre (the fifth point).
    Curvilinear { center: Pt, radius: f64, five: bool },
}

/// A line or circle a stroke can be held to.
#[derive(Clone, Copy, Debug, PartialEq)]
pub enum AssistCurve {
    /// Through `origin` along the unit vector `dir`.
    Line {
        origin: Pt,
        dir: Pt,
    },
    Circle {
        center: Pt,
        radius: f64,
    },
}

/// A straight edge between two handles that strokes snap to.
#[derive(Clone, Copy, Debug, PartialEq, Serialize, Deserialize)]
pub struct Ruler {
    pub a: Pt,
    pub b: Pt,
    #[serde(default = "yes")]
    pub enabled: bool,
}

fn yes() -> bool {
    true
}

/// Named guides that can be switched to as a group.
#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
pub struct GuideSet {
    pub name: String,
    pub guides: Vec<GuideKind>,
}

/// A document's drawing guides: those shown now, the ruler, and the saved
/// sets. Not part of Undo, like the colour palette.
#[derive(Clone, Debug, Default, PartialEq, Serialize, Deserialize)]
#[serde(default)]
pub struct DrawingGuides {
    #[serde(skip_serializing_if = "Vec::is_empty")]
    pub guides: Vec<GuideKind>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub ruler: Option<Ruler>,
    #[serde(skip_serializing_if = "Vec::is_empty")]
    pub sets: Vec<GuideSet>,
    /// The set last switched to, if the guides still come from it.
    #[serde(skip_serializing_if = "Option::is_none")]
    pub active_set: Option<usize>,
}

fn norm(v: Pt) -> Pt {
    let l = v.0.hypot(v.1);
    if l < 1e-9 {
        (1.0, 0.0)
    } else {
        (v.0 / l, v.1 / l)
    }
}

fn finite(p: Pt) -> bool {
    p.0.is_finite() && p.1.is_finite()
}

impl AssistCurve {
    /// The nearest point on the curve to `p`.
    pub fn project(&self, p: Pt) -> Pt {
        match *self {
            AssistCurve::Line { origin: o, dir: d } => {
                let t = (p.0 - o.0) * d.0 + (p.1 - o.1) * d.1;
                (o.0 + d.0 * t, o.1 + d.1 * t)
            }
            AssistCurve::Circle { center: c, radius } => {
                let v = norm((p.0 - c.0, p.1 - c.1));
                (c.0 + v.0 * radius, c.1 + v.1 * radius)
            }
        }
    }

    /// The unit direction of the curve at (or nearest to) `p`.
    pub fn tangent(&self, p: Pt) -> Pt {
        match *self {
            AssistCurve::Line { dir, .. } => dir,
            AssistCurve::Circle { center: c, .. } => {
                let v = norm((p.0 - c.0, p.1 - c.1));
                (-v.1, v.0)
            }
        }
    }

    /// Distance from `p` to the curve.
    pub fn distance(&self, p: Pt) -> f64 {
        let q = self.project(p);
        (p.0 - q.0).hypot(p.1 - q.1)
    }

    /// Of `curves`, the one whose direction at `at` best matches the hand's
    /// direction `hand` (either way along it).
    pub fn best(curves: &[AssistCurve], at: Pt, hand: Pt) -> Option<AssistCurve> {
        let hand = norm(hand);
        curves.iter().copied().max_by(|a, b| {
            let score = |c: &AssistCurve| {
                let t = c.tangent(at);
                (t.0 * hand.0 + t.1 * hand.1).abs()
            };
            score(a).total_cmp(&score(b))
        })
    }
}

/// The circle through `a`, `b` and `p`, or the line through `p` parallel
/// to `a`–`b` when `p` lies on the line `a`–`b` (the arc's limit).
fn circle_through(a: Pt, b: Pt, p: Pt) -> AssistCurve {
    let (ax, ay, bx, by, px, py) = (a.0, a.1, b.0, b.1, p.0, p.1);
    let d = 2.0 * (ax * (by - py) + bx * (py - ay) + px * (ay - by));
    let chord = (bx - ax).hypot(by - ay).max(1e-9);
    if d.abs() < 1e-6 * chord * chord {
        return AssistCurve::Line {
            origin: p,
            dir: norm((bx - ax, by - ay)),
        };
    }
    let (a2, b2, p2) = (ax * ax + ay * ay, bx * bx + by * by, px * px + py * py);
    let ux = (a2 * (by - py) + b2 * (py - ay) + p2 * (ay - by)) / d;
    let uy = (a2 * (px - bx) + b2 * (ax - px) + p2 * (bx - ax)) / d;
    AssistCurve::Circle {
        center: (ux, uy),
        radius: (px - ux).hypot(py - uy),
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
            GuideKind::Curvilinear { five: false, .. } => "4-point",
            GuideKind::Curvilinear { five: true, .. } => "5-point",
        }
    }

    /// Default guides for a canvas of this size, in the order the chip
    /// cycles through them.
    pub fn cycle(&self, w: f64, h: f64) -> GuideKind {
        let step = (w.min(h) / 12.0).round().max(8.0);
        let curvilinear = |five| GuideKind::Curvilinear {
            center: (w / 2.0, h / 2.0),
            radius: w.max(h) * 0.6,
            five,
        };
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
            GuideKind::Perspective { .. } => curvilinear(false),
            GuideKind::Curvilinear { five: false, .. } => curvilinear(true),
            GuideKind::Curvilinear { five: true, .. } => GuideKind::Off,
        }
    }

    /// The draggable points: vanishing points, or a curvilinear guide's
    /// centre followed by its right-hand vanishing point (the radius).
    pub fn handles(&self) -> Vec<Pt> {
        match self {
            GuideKind::Perspective { points } => points.clone(),
            GuideKind::Curvilinear { center, radius, .. } => {
                vec![*center, (center.0 + radius, center.1)]
            }
            _ => Vec::new(),
        }
    }

    /// Move handle `i` (see [`GuideKind::handles`]) to `p`.
    pub fn move_handle(&mut self, i: usize, p: Pt) {
        if !finite(p) {
            return;
        }
        match self {
            GuideKind::Perspective { points } => {
                if let Some(q) = points.get_mut(i) {
                    *q = p;
                }
            }
            GuideKind::Curvilinear { center, radius, .. } => match i {
                0 => *center = p,
                1 => *radius = (p.0 - center.0).hypot(p.1 - center.1).max(8.0),
                _ => {}
            },
            _ => {}
        }
    }

    /// Curvilinear vanishing points: left, right, top, bottom.
    fn rim(center: Pt, radius: f64) -> [Pt; 4] {
        let (cx, cy) = center;
        [
            (cx - radius, cy),
            (cx + radius, cy),
            (cx, cy - radius),
            (cx, cy + radius),
        ]
    }

    /// The lines and circles a stroke starting at `at` may follow.
    pub fn curves(&self, at: Pt) -> Vec<AssistCurve> {
        let line = |dir: Pt| AssistCurve::Line {
            origin: at,
            dir: norm(dir),
        };
        match self {
            GuideKind::Off => Vec::new(),
            GuideKind::Grid { .. } => vec![line((1.0, 0.0)), line((0.0, 1.0))],
            GuideKind::Isometric { .. } => {
                let (c, s) = (30f64.to_radians().cos(), 30f64.to_radians().sin());
                vec![line((c, -s)), line((c, s)), line((0.0, 1.0))]
            }
            GuideKind::Perspective { points } => {
                let mut v: Vec<AssistCurve> = points
                    .iter()
                    .map(|p| line((p.0 - at.0, p.1 - at.1)))
                    .collect();
                // Verticals stay vertical until a third point takes over.
                if points.len() < 3 {
                    v.push(line((0.0, 1.0)));
                }
                if points.len() < 2 {
                    v.push(line((1.0, 0.0)));
                }
                v
            }
            GuideKind::Curvilinear {
                center,
                radius,
                five,
            } => {
                let [l, r, t, b] = Self::rim(*center, *radius);
                let mut v = vec![circle_through(l, r, at), circle_through(t, b, at)];
                if *five {
                    v.push(line((at.0 - center.0, at.1 - center.1)));
                }
                v
            }
        }
    }

    /// The guide as document-space polylines over a `w`×`h` canvas.
    pub fn lines(&self, w: f64, h: f64) -> Vec<Vec<Pt>> {
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
                    let mut targets = Vec::with_capacity(n * 4 + 4);
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
                if !points.is_empty() && points.len() < 3 {
                    // The horizon through the vanishing points.
                    let y = points.iter().map(|p| p.1).sum::<f64>() / points.len() as f64;
                    out.push(vec![(0.0, y), (w, y)]);
                }
            }
            GuideKind::Curvilinear {
                center,
                radius,
                five,
            } => {
                let (c, r) = (*center, radius.max(1.0));
                let [left, right, top, bottom] = Self::rim(c, r);
                let n = 8;
                for i in -n..=n {
                    // Arcs crossing the axes at even steps, like a lens's
                    // meridians and parallels; the middle ones are the
                    // horizon and the vertical.
                    let off = r * f64::from(i) / f64::from(n);
                    for (a, b, cross) in [
                        (left, right, (c.0, c.1 + off)),
                        (top, bottom, (c.0 + off, c.1)),
                    ] {
                        let curve = circle_through(a, b, cross);
                        out.extend(curve_polylines(&curve, a, b, cross, w, h));
                    }
                }
                if *five {
                    for k in 0..24 {
                        let t = f64::from(k) / 24.0 * std::f64::consts::TAU;
                        let far = (c.0 + t.cos() * (w + h) * 2.0, c.1 + t.sin() * (w + h) * 2.0);
                        let l = clip_line(c, far, w, h);
                        if l.len() == 2 {
                            out.push(l);
                        }
                    }
                }
            }
        }
        out
    }

    fn valid(&self) -> bool {
        match self {
            GuideKind::Off => true,
            GuideKind::Grid { size } | GuideKind::Isometric { size } => {
                size.is_finite() && *size > 0.0
            }
            GuideKind::Perspective { points } => {
                (1..=3).contains(&points.len()) && points.iter().all(|p| finite(*p))
            }
            GuideKind::Curvilinear { center, radius, .. } => {
                finite(*center) && radius.is_finite() && *radius > 0.0
            }
        }
    }
}

/// The arc of `curve` from `a` to `b` that passes `via`, as polylines
/// inside the canvas.
fn curve_polylines(curve: &AssistCurve, a: Pt, b: Pt, via: Pt, w: f64, h: f64) -> Vec<Vec<Pt>> {
    let points: Vec<Pt> = match *curve {
        AssistCurve::Line { .. } => {
            let l = clip_line(a, b, w, h);
            return if l.len() == 2 { vec![l] } else { Vec::new() };
        }
        AssistCurve::Circle { center, radius } => {
            let tau = std::f64::consts::TAU;
            let angle = |p: Pt| (p.1 - center.1).atan2(p.0 - center.0);
            let start = angle(a);
            let to_b = (angle(b) - start).rem_euclid(tau);
            let to_via = (angle(via) - start).rem_euclid(tau);
            // Go round the way that passes `via`.
            let sweep = if to_via <= to_b { to_b } else { to_b - tau };
            let steps = 96;
            (0..=steps)
                .map(|k| {
                    let t = start + sweep * f64::from(k) / f64::from(steps);
                    (center.0 + t.cos() * radius, center.1 + t.sin() * radius)
                })
                .collect()
        }
    };
    let inside = |p: &Pt| p.0 >= 0.0 && p.1 >= 0.0 && p.0 <= w && p.1 <= h;
    let mut out = Vec::new();
    let mut run: Vec<Pt> = Vec::new();
    for p in points {
        if inside(&p) {
            run.push(p);
        } else if run.len() > 1 {
            out.push(std::mem::take(&mut run));
        } else {
            run.clear();
        }
    }
    if run.len() > 1 {
        out.push(run);
    }
    out
}

/// The part of segment `a`–`b` inside the canvas rectangle.
pub fn clip_line(a: Pt, b: Pt, w: f64, h: f64) -> Vec<Pt> {
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

impl Ruler {
    /// A ruler across the middle half of a `w`×`h` canvas.
    pub fn centered(w: f64, h: f64) -> Self {
        Ruler {
            a: (w * 0.25, h * 0.5),
            b: (w * 0.75, h * 0.5),
            enabled: true,
        }
    }

    /// The ruler's line, when a stroke starting at `at` is within `reach`
    /// document pixels of the ruler (between its handles, or just beyond).
    pub fn snap(&self, at: Pt, reach: f64) -> Option<AssistCurve> {
        if !self.enabled {
            return None;
        }
        let (dx, dy) = (self.b.0 - self.a.0, self.b.1 - self.a.1);
        let len = dx.hypot(dy);
        if len < 1e-6 {
            return None;
        }
        let dir = (dx / len, dy / len);
        let t = (at.0 - self.a.0) * dir.0 + (at.1 - self.a.1) * dir.1;
        if t < -reach || t > len + reach {
            return None;
        }
        let line = AssistCurve::Line {
            origin: self.a,
            dir,
        };
        (line.distance(at) <= reach).then_some(line)
    }
}

impl DrawingGuides {
    /// Guides that draw and snap: those shown, without Off.
    pub fn active(&self) -> impl Iterator<Item = &GuideKind> {
        self.guides.iter().filter(|g| **g != GuideKind::Off)
    }

    /// The first guide (the one the guide chip cycles), or Off.
    pub fn primary(&self) -> GuideKind {
        self.guides.first().cloned().unwrap_or_default()
    }

    /// Replace the first guide, keeping any others.
    pub fn set_primary(&mut self, kind: GuideKind) {
        match self.guides.first_mut() {
            Some(g) => *g = kind,
            None => self.guides.push(kind),
        }
        if self.guides.len() == 1 && self.guides[0] == GuideKind::Off {
            self.guides.clear();
        }
        self.active_set = None;
    }

    /// Every guide's handles in order, then the ruler's two.
    pub fn handles(&self) -> Vec<Pt> {
        let mut v: Vec<Pt> = self.guides.iter().flat_map(|g| g.handles()).collect();
        if let Some(r) = self.ruler.filter(|r| r.enabled) {
            v.extend([r.a, r.b]);
        }
        v
    }

    /// Move handle `i` of [`DrawingGuides::handles`] to `p`.
    pub fn move_handle(&mut self, mut i: usize, p: Pt) {
        for g in &mut self.guides {
            let n = g.handles().len();
            if i < n {
                g.move_handle(i, p);
                return;
            }
            i -= n;
        }
        if let Some(r) = self.ruler.as_mut().filter(|r| r.enabled)
            && finite(p)
        {
            match i {
                0 => r.a = p,
                1 => r.b = p,
                _ => {}
            }
        }
    }

    /// Lines and circles a stroke starting at `at` may follow, from every
    /// guide shown.
    pub fn curves(&self, at: Pt) -> Vec<AssistCurve> {
        self.active().flat_map(|g| g.curves(at)).collect()
    }

    /// Save the guides shown as a set called `name`, replacing a set of the
    /// same name. Returns its index.
    pub fn save_set(&mut self, name: &str) -> Result<usize, String> {
        let name = name.trim();
        if name.is_empty() {
            return Err("A guide set needs a name".into());
        }
        let guides: Vec<GuideKind> = self.active().cloned().collect();
        if guides.is_empty() {
            return Err("Show a guide before saving a set".into());
        }
        let set = GuideSet {
            name: name.into(),
            guides,
        };
        let i = match self.sets.iter().position(|s| s.name == set.name) {
            Some(i) => {
                self.sets[i] = set;
                i
            }
            None => {
                if self.sets.len() >= MAX_GUIDE_SETS {
                    return Err(format!("At most {MAX_GUIDE_SETS} guide sets"));
                }
                self.sets.push(set);
                self.sets.len() - 1
            }
        };
        self.active_set = Some(i);
        Ok(i)
    }

    /// Show set `i`'s guides in place of the current ones.
    pub fn switch_to(&mut self, i: usize) -> bool {
        let Some(set) = self.sets.get(i) else {
            return false;
        };
        self.guides = set.guides.clone();
        self.active_set = Some(i);
        true
    }

    pub fn delete_set(&mut self, i: usize) -> bool {
        if i >= self.sets.len() {
            return false;
        }
        self.sets.remove(i);
        self.active_set = match self.active_set {
            Some(a) if a == i => None,
            Some(a) if a > i => Some(a - 1),
            a => a,
        };
        true
    }

    /// A short name for the next set saved.
    pub fn next_set_name(&self) -> String {
        (1..)
            .map(|n| format!("Guides {n}"))
            .find(|n| self.sets.iter().all(|s| &s.name != n))
            .expect("an unused name")
    }

    pub fn validate(&self) -> Result<(), String> {
        if self.guides.len() > MAX_GUIDES {
            return Err(format!("At most {MAX_GUIDES} guides at once"));
        }
        if self.sets.len() > MAX_GUIDE_SETS {
            return Err(format!("At most {MAX_GUIDE_SETS} guide sets"));
        }
        for set in &self.sets {
            if set.name.trim().is_empty() || set.guides.len() > MAX_GUIDES {
                return Err(format!("Guide set {:?} is invalid", set.name));
            }
            if !set.guides.iter().all(GuideKind::valid) {
                return Err(format!("Guide set {:?} has an invalid guide", set.name));
            }
        }
        if !self.guides.iter().all(GuideKind::valid) {
            return Err("Invalid guide".into());
        }
        if let Some(r) = self.ruler
            && !(finite(r.a) && finite(r.b))
        {
            return Err("Invalid ruler".into());
        }
        if self.active_set.is_some_and(|i| i >= self.sets.len()) {
            return Err("The active guide set does not exist".into());
        }
        Ok(())
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn close(a: Pt, b: Pt) -> bool {
        (a.0 - b.0).abs() < 1e-6 && (a.1 - b.1).abs() < 1e-6
    }

    #[test]
    fn guides_cycle_through_curvilinear_and_draw_inside_the_canvas() {
        let mut k = GuideKind::Off;
        let mut seen = Vec::new();
        for _ in 0..8 {
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
                "4-point",
                "5-point",
                "guide"
            ]
        );
        let iso = GuideKind::Isometric { size: 40.0 };
        let c = iso.curves((0.0, 0.0));
        assert_eq!(c.len(), 3);
        assert!((c[0].tangent((0.0, 0.0)).1 + 0.5).abs() < 1e-9, "30° up");
        let p = GuideKind::Perspective {
            points: vec![(200.0, 150.0)],
        };
        assert!(close(
            p.curves((0.0, 150.0))[0].tangent((0.0, 0.0)),
            (1.0, 0.0)
        ));
        assert_eq!(
            clip_line((-10.0, 5.0), (50.0, 5.0), 40.0, 40.0),
            vec![(0.0, 5.0), (40.0, 5.0)]
        );
        assert!(clip_line((-10.0, -5.0), (50.0, -5.0), 40.0, 40.0).is_empty());
    }

    #[test]
    fn curvilinear_strokes_follow_arcs_through_opposite_vanishing_points() {
        let g = GuideKind::Curvilinear {
            center: (100.0, 100.0),
            radius: 80.0,
            five: false,
        };
        let start = (100.0, 60.0);
        let curves = g.curves(start);
        assert_eq!(curves.len(), 2);
        // The "horizontal" arc passes through the start and both side points.
        let AssistCurve::Circle { center, radius } = curves[0] else {
            panic!("an arc off the horizon")
        };
        for p in [start, (20.0, 100.0), (180.0, 100.0)] {
            assert!(((p.0 - center.0).hypot(p.1 - center.1) - radius).abs() < 1e-6);
        }
        // A sideways hand picks it, and points snap onto it.
        let arc = AssistCurve::best(&curves, start, (1.0, 0.1)).unwrap();
        assert_eq!(arc, curves[0]);
        let snapped = arc.project((140.0, 90.0));
        assert!(arc.distance(snapped) < 1e-9);
        assert!(snapped.1 < 90.0, "pulled up onto the bulging arc");
        // A downward hand picks the vertical arc through top and bottom.
        let down = AssistCurve::best(&curves, start, (0.05, 1.0)).unwrap();
        assert_eq!(down, curves[1]);
        // On the horizon itself the arc is the straight horizon.
        let flat = g.curves((60.0, 100.0));
        assert!(matches!(flat[0], AssistCurve::Line { dir, .. } if close(dir, (1.0, 0.0))));
        // Five-point adds lines radiating from the centre.
        let five = GuideKind::Curvilinear {
            center: (100.0, 100.0),
            radius: 80.0,
            five: true,
        };
        let c = five.curves((130.0, 140.0));
        assert_eq!(c.len(), 3);
        assert!(c[2].distance((100.0, 100.0)) < 1e-9, "radial line");
        assert!(close(c[2].project((160.0, 180.0)), (160.0, 180.0)));
        // The handles move the centre and set the radius.
        let mut moved = five.clone();
        assert_eq!(moved.handles(), vec![(100.0, 100.0), (180.0, 100.0)]);
        moved.move_handle(1, (100.0, 150.0));
        moved.move_handle(0, (90.0, 90.0));
        assert_eq!(
            moved,
            GuideKind::Curvilinear {
                center: (90.0, 90.0),
                radius: 50.0,
                five: true
            }
        );
    }

    #[test]
    fn rulers_snap_only_nearby_strokes_onto_their_line() {
        let r = Ruler {
            a: (10.0, 10.0),
            b: (110.0, 10.0),
            enabled: true,
        };
        let line = r.snap((50.0, 14.0), 6.0).expect("close to the edge");
        assert!(close(line.project((80.0, 30.0)), (80.0, 10.0)));
        assert!(r.snap((50.0, 30.0), 6.0).is_none(), "too far away");
        assert!(r.snap((130.0, 10.0), 6.0).is_none(), "beyond the end");
        assert!(r.snap((114.0, 10.0), 6.0).is_some(), "just beyond the end");
        let off = Ruler {
            enabled: false,
            ..r
        };
        assert!(off.snap((50.0, 10.0), 6.0).is_none());
    }

    #[test]
    fn guide_sets_save_switch_and_survive_serde() {
        let mut d = DrawingGuides::default();
        assert!(d.save_set("Empty").is_err());
        d.set_primary(GuideKind::Grid { size: 20.0 });
        d.guides.push(GuideKind::Curvilinear {
            center: (5.0, 5.0),
            radius: 9.0,
            five: true,
        });
        assert_eq!(d.save_set("Street").unwrap(), 0);
        d.guides = vec![GuideKind::Perspective {
            points: vec![(1.0, 2.0)],
        }];
        let name = d.next_set_name();
        assert_eq!(name, "Guides 1");
        assert_eq!(d.save_set(&name).unwrap(), 1);
        assert!(d.switch_to(0));
        assert_eq!(d.guides.len(), 2);
        assert_eq!(d.active_set, Some(0));
        d.ruler = Some(Ruler::centered(100.0, 50.0));
        assert_eq!(d.handles().len(), 2 + 2, "curvilinear and ruler handles");
        d.move_handle(3, (90.0, 40.0));
        assert_eq!(d.ruler.unwrap().b, (90.0, 40.0));
        d.validate().unwrap();
        let json = serde_json::to_string(&d).unwrap();
        let back: DrawingGuides = serde_json::from_str(&json).unwrap();
        assert_eq!(back, d);
        assert_eq!(
            serde_json::from_str::<DrawingGuides>("{}").unwrap(),
            DrawingGuides::default()
        );
        assert!(d.delete_set(0));
        assert_eq!(d.active_set, None);
        assert_eq!(d.sets[0].name, "Guides 1");
        d.active_set = Some(4);
        assert!(d.validate().is_err());
    }
}
