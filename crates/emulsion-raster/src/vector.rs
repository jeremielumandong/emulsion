//! Vector paths: cubic Bézier subpaths with anchors and handles, the way
//! Inkscape and the Photoshop pen draw them.
//!
//! A path is data; a Path node in a document keeps one and rasterizes it
//! whenever it changes, so it composites like pixels but stays editable.
//! Fill uses the non-zero winding rule; strokes have round joins and caps.
//! Paths read and write SVG path data (`M L H V C S Q T Z`), which is how
//! the assistant draws them.

use crate::color;
use crate::geom::IRect;
use crate::image::{Mask, Raster};
use glam::{DAffine2, dvec2};
use serde::{Deserialize, Serialize};

pub type Pt = (f64, f64);

/// One point on a path, with its incoming and outgoing handles in
/// absolute coordinates. A corner has both handles on the point.
#[derive(Clone, Copy, Debug, PartialEq, Serialize, Deserialize)]
pub struct Anchor {
    pub p: Pt,
    pub h_in: Pt,
    pub h_out: Pt,
    /// Handles stay opposite each other when one moves.
    pub smooth: bool,
}

impl Anchor {
    pub fn corner(p: Pt) -> Self {
        Self {
            p,
            h_in: p,
            h_out: p,
            smooth: false,
        }
    }

    pub fn has_handles(&self) -> bool {
        dist(self.h_in, self.p) > 1e-6 || dist(self.h_out, self.p) > 1e-6
    }
}

#[derive(Clone, Debug, PartialEq, Default, Serialize, Deserialize)]
pub struct SubPath {
    pub anchors: Vec<Anchor>,
    pub closed: bool,
}

#[derive(Clone, Debug, PartialEq, Default, Serialize, Deserialize)]
pub struct Path {
    pub subpaths: Vec<SubPath>,
}

/// How a Path node draws itself. Colours are straight sRGB with alpha.
#[derive(Clone, Copy, Debug, PartialEq, Serialize, Deserialize)]
#[serde(default)]
pub struct PathStyle {
    pub stroke: Option<[u8; 4]>,
    /// Stroke width in document pixels.
    pub width: f32,
    pub fill: Option<[u8; 4]>,
}

impl Default for PathStyle {
    fn default() -> Self {
        Self {
            stroke: Some([10, 10, 11, 255]),
            width: 3.0,
            fill: None,
        }
    }
}

impl PathStyle {
    pub fn sanitized(mut self) -> Self {
        self.width = if self.width.is_finite() {
            self.width.clamp(0.0, 500.0)
        } else {
            3.0
        };
        self
    }
}

pub const MAX_ANCHORS: usize = 20_000;

#[inline]
fn dist(a: Pt, b: Pt) -> f64 {
    (a.0 - b.0).hypot(a.1 - b.1)
}

#[inline]
fn lerp(a: Pt, b: Pt, t: f64) -> Pt {
    (a.0 + (b.0 - a.0) * t, a.1 + (b.1 - a.1) * t)
}

/// Point on a cubic at `t`.
pub fn cubic_at(p0: Pt, p1: Pt, p2: Pt, p3: Pt, t: f64) -> Pt {
    let u = 1.0 - t;
    let (a, b, c, d) = (u * u * u, 3.0 * u * u * t, 3.0 * u * t * t, t * t * t);
    (
        a * p0.0 + b * p1.0 + c * p2.0 + d * p3.0,
        a * p0.1 + b * p1.1 + c * p2.1 + d * p3.1,
    )
}

/// The segment from anchor `i` to the next, as cubic control points.
fn segment(sp: &SubPath, i: usize) -> Option<(Pt, Pt, Pt, Pt)> {
    let n = sp.anchors.len();
    let j = if i + 1 < n {
        i + 1
    } else if sp.closed && n > 1 {
        0
    } else {
        return None;
    };
    let (a, b) = (&sp.anchors[i], &sp.anchors[j]);
    Some((a.p, a.h_out, b.h_in, b.p))
}

fn segment_count(sp: &SubPath) -> usize {
    match sp.anchors.len() {
        0 | 1 => 0,
        n if sp.closed => n,
        n => n - 1,
    }
}

impl SubPath {
    /// The subpath as a polyline, curves subdivided to about `tol` pixels.
    pub fn flatten(&self, tol: f64) -> Vec<Pt> {
        let mut out = Vec::new();
        let Some(first) = self.anchors.first() else {
            return out;
        };
        out.push(first.p);
        for i in 0..segment_count(self) {
            let (p0, p1, p2, p3) = segment(self, i).expect("counted");
            if dist(p0, p1) < 1e-9 && dist(p2, p3) < 1e-9 {
                out.push(p3);
                continue;
            }
            let approx = dist(p0, p1) + dist(p1, p2) + dist(p2, p3);
            let n = ((approx / tol.max(0.25)).sqrt() * 2.0)
                .ceil()
                .clamp(4.0, 96.0) as usize;
            for k in 1..=n {
                out.push(cubic_at(p0, p1, p2, p3, k as f64 / n as f64));
            }
        }
        out
    }
}

impl Path {
    pub fn is_empty(&self) -> bool {
        self.subpaths.iter().all(|s| s.anchors.is_empty())
    }

    pub fn anchor_count(&self) -> usize {
        self.subpaths.iter().map(|s| s.anchors.len()).sum()
    }

    /// Every subpath as a polyline with its closed flag.
    pub fn flatten(&self, tol: f64) -> Vec<(Vec<Pt>, bool)> {
        self.subpaths
            .iter()
            .filter(|s| !s.anchors.is_empty())
            .map(|s| (s.flatten(tol), s.closed))
            .collect()
    }

    /// Bounds of the drawn path, including handles' reach and the stroke.
    pub fn bounds(&self, style: &PathStyle) -> IRect {
        let (mut x0, mut y0, mut x1, mut y1) = (f64::MAX, f64::MAX, f64::MIN, f64::MIN);
        for sp in &self.subpaths {
            for a in &sp.anchors {
                for q in [a.p, a.h_in, a.h_out] {
                    x0 = x0.min(q.0);
                    y0 = y0.min(q.1);
                    x1 = x1.max(q.0);
                    y1 = y1.max(q.1);
                }
            }
        }
        if x0 > x1 {
            return IRect::default();
        }
        let pad = if style.stroke.is_some() {
            style.width as f64 / 2.0 + 2.0
        } else {
            2.0
        };
        IRect::new(
            (x0 - pad).floor() as i32,
            (y0 - pad).floor() as i32,
            (x1 - x0 + 2.0 * pad).ceil() as i32 + 1,
            (y1 - y0 + 2.0 * pad).ceil() as i32 + 1,
        )
    }

    pub fn translate(&mut self, dx: f64, dy: f64) {
        self.transform(DAffine2::from_translation(dvec2(dx, dy)));
    }

    pub fn transform(&mut self, m: DAffine2) {
        let f = |p: &mut Pt| {
            let q = m.transform_point2(dvec2(p.0, p.1));
            *p = (q.x, q.y);
        };
        for sp in &mut self.subpaths {
            for a in &mut sp.anchors {
                f(&mut a.p);
                f(&mut a.h_in);
                f(&mut a.h_out);
            }
        }
    }

    // ── Editing ──

    /// Which anchor or handle is within `tol` of `pt`. Handles win over
    /// anchors so they can be grabbed when they sit close.
    pub fn hit(&self, pt: Pt, tol: f64) -> Option<Hit> {
        let mut best: Option<(f64, Hit)> = None;
        let mut consider = |d: f64, h: Hit| {
            if d <= tol && best.as_ref().is_none_or(|(b, _)| d < *b) {
                best = Some((d, h));
            }
        };
        for (si, sp) in self.subpaths.iter().enumerate() {
            for (ai, a) in sp.anchors.iter().enumerate() {
                if a.has_handles() {
                    consider(dist(pt, a.h_out) * 0.9, Hit::HandleOut(si, ai));
                    consider(dist(pt, a.h_in) * 0.9, Hit::HandleIn(si, ai));
                }
                consider(dist(pt, a.p), Hit::Anchor(si, ai));
            }
        }
        best.map(|(_, h)| h)
    }

    /// The nearest point on the outline within `tol`: subpath, segment and t.
    pub fn nearest_on_curve(&self, pt: Pt, tol: f64) -> Option<(usize, usize, f64)> {
        let mut best: Option<(f64, (usize, usize, f64))> = None;
        for (si, sp) in self.subpaths.iter().enumerate() {
            for seg in 0..segment_count(sp) {
                let (p0, p1, p2, p3) = segment(sp, seg).expect("counted");
                for k in 0..=32 {
                    let t = k as f64 / 32.0;
                    let d = dist(pt, cubic_at(p0, p1, p2, p3, t));
                    if d <= tol && best.as_ref().is_none_or(|(b, _)| d < *b) {
                        best = Some((d, (si, seg, t)));
                    }
                }
            }
        }
        best.map(|(_, r)| r)
    }

    /// Split segment `seg` of subpath `si` at `t`, adding an anchor there.
    /// Returns the new anchor's index.
    pub fn insert_at(&mut self, si: usize, seg: usize, t: f64) -> Option<usize> {
        let sp = self.subpaths.get_mut(si)?;
        let (p0, p1, p2, p3) = segment(sp, seg)?;
        let (a, b, c) = (lerp(p0, p1, t), lerp(p1, p2, t), lerp(p2, p3, t));
        let (d, e) = (lerp(a, b, t), lerp(b, c, t));
        let m = lerp(d, e, t);
        let j = (seg + 1) % sp.anchors.len();
        sp.anchors[seg].h_out = a;
        sp.anchors[j].h_in = c;
        let new = Anchor {
            p: m,
            h_in: d,
            h_out: e,
            smooth: true,
        };
        sp.anchors.insert(seg + 1, new);
        Some(seg + 1)
    }

    /// Remove an anchor; drops the subpath when it empties.
    pub fn remove_anchor(&mut self, si: usize, ai: usize) {
        if let Some(sp) = self.subpaths.get_mut(si)
            && ai < sp.anchors.len()
        {
            sp.anchors.remove(ai);
            if sp.anchors.is_empty() {
                self.subpaths.remove(si);
            }
        }
    }

    // ── Rasterizing ──

    /// Coverage of the filled path (non-zero winding), 0–255.
    pub fn fill_mask(&self, w: u32, h: u32) -> Mask {
        let polys: Vec<Vec<Pt>> = self.flatten(0.5).into_iter().map(|(p, _)| p).collect();
        fill_coverage(&polys, w, h)
    }

    /// Coverage of the stroked outline, 0–255.
    pub fn stroke_mask(&self, width: f64, w: u32, h: u32) -> Mask {
        let polys = self.flatten(0.5);
        stroke_coverage(&polys, width, w, h)
    }

    /// Draw the path with `style` onto a transparent document-sized layer.
    pub fn rasterize(&self, style: &PathStyle, w: u32, h: u32) -> Raster {
        let style = style.sanitized();
        let b = self
            .bounds(&style)
            .intersect(&IRect::new(0, 0, w as i32, h as i32));
        if b.is_empty() || self.is_empty() {
            return Raster::transparent(w, h);
        }
        // Straight to 16-bit pixels, one row at a time in parallel: the
        // fill goes down first, the stroke over it.
        use rayon::prelude::*;
        let fill = style.fill.map(|c| {
            (
                self.fill_mask(w, h).read_rect(b),
                color::srgba8_to_premul(c),
            )
        });
        let stroke = match (style.stroke, style.width > 0.0) {
            (Some(c), true) => Some((
                self.stroke_mask(style.width as f64, w, h).read_rect(b),
                color::srgba8_to_premul(c),
            )),
            _ => None,
        };
        let bw = b.w as usize;
        let mut out: Vec<[u16; 4]> = vec![[0; 4]; (b.w * b.h) as usize];
        out.par_chunks_mut(bw).enumerate().for_each(|(row, o)| {
            let base = row * bw;
            for (i, px) in o.iter_mut().enumerate() {
                let mut acc = [0.0f32; 4];
                if let Some((m, col)) = &fill {
                    let k = m[base + i] as f32 / 255.0;
                    if k > 0.0 {
                        for c in 0..4 {
                            acc[c] = col[c] * k;
                        }
                    }
                }
                if let Some((m, col)) = &stroke {
                    let k = m[base + i] as f32 / 255.0;
                    if k > 0.0 {
                        for c in 0..4 {
                            acc[c] = col[c] * k + acc[c] * (1.0 - col[3] * k);
                        }
                    }
                }
                if acc[3] > 0.0 {
                    *px = color::f_to_px(acc);
                }
            }
        });
        Raster::transparent(w, h).write_rect(b, &out)
    }

    // ── SVG path data ──

    /// The path as SVG `d` data, absolute coordinates.
    pub fn to_svg(&self) -> String {
        let f = |v: f64| {
            let r = (v * 100.0).round() / 100.0;
            if r.fract() == 0.0 {
                format!("{r:.0}")
            } else {
                format!("{r}")
            }
        };
        let mut s = String::new();
        for sp in &self.subpaths {
            let Some(first) = sp.anchors.first() else {
                continue;
            };
            s.push_str(&format!("M {} {}", f(first.p.0), f(first.p.1)));
            for i in 0..segment_count(sp) {
                let (p0, p1, p2, p3) = segment(sp, i).expect("counted");
                let straight = dist(p0, p1) < 1e-9 && dist(p2, p3) < 1e-9;
                // Z already draws a straight closing segment.
                if sp.closed && i + 1 == segment_count(sp) && straight {
                    break;
                }
                if straight {
                    s.push_str(&format!(" L {} {}", f(p3.0), f(p3.1)));
                } else {
                    s.push_str(&format!(
                        " C {} {} {} {} {} {}",
                        f(p1.0),
                        f(p1.1),
                        f(p2.0),
                        f(p2.1),
                        f(p3.0),
                        f(p3.1)
                    ));
                }
            }
            if sp.closed {
                s.push_str(" Z");
            }
        }
        s
    }

    /// Parse SVG path data. Supports M L H V C S Q T Z, absolute and relative.
    pub fn from_svg(d: &str) -> Result<Path, String> {
        let mut toks: Vec<Token> = Vec::new();
        let mut num = String::new();
        let flush = |num: &mut String, toks: &mut Vec<Token>| -> Result<(), String> {
            if !num.is_empty() {
                let v: f64 = num.parse().map_err(|_| format!("bad number {num:?}"))?;
                toks.push(Token::Num(v));
                num.clear();
            }
            Ok(())
        };
        for ch in d.chars() {
            match ch {
                'a'..='z' | 'A'..='Z' if ch != 'e' && ch != 'E' => {
                    flush(&mut num, &mut toks)?;
                    toks.push(Token::Cmd(ch));
                }
                ',' | ' ' | '\n' | '\t' | '\r' => flush(&mut num, &mut toks)?,
                '-' | '+' if !num.is_empty() && !num.ends_with(['e', 'E']) => {
                    flush(&mut num, &mut toks)?;
                    num.push(ch);
                }
                '.' if num.contains('.') && !num.contains(['e', 'E']) => {
                    flush(&mut num, &mut toks)?;
                    num.push(ch);
                }
                _ => num.push(ch),
            }
        }
        flush(&mut num, &mut toks)?;

        let mut path = Path::default();
        let mut cur: Option<SubPath> = None;
        let mut pos: Pt = (0.0, 0.0);
        let mut start: Pt = (0.0, 0.0);
        let mut last_ctrl: Option<Pt> = None;
        let mut cmd: Option<char> = None;
        let mut i = 0;
        let take = |i: &mut usize, n: usize| -> Result<Vec<f64>, String> {
            let mut v = Vec::with_capacity(n);
            for _ in 0..n {
                match toks.get(*i) {
                    Some(Token::Num(x)) if x.is_finite() => {
                        v.push(*x);
                        *i += 1;
                    }
                    _ => return Err("path data ends in the middle of a command".into()),
                }
            }
            Ok(v)
        };
        // Extend the current subpath by a cubic ending at `p3`.
        fn cubic(cur: &mut Option<SubPath>, pos: Pt, c1: Pt, c2: Pt, p3: Pt) {
            let sp = cur.get_or_insert_with(|| SubPath {
                anchors: vec![Anchor::corner(pos)],
                closed: false,
            });
            if let Some(last) = sp.anchors.last_mut() {
                last.h_out = c1;
                last.smooth = last.has_handles() && dist(last.h_in, last.p) > 1e-9;
            }
            sp.anchors.push(Anchor {
                p: p3,
                h_in: c2,
                h_out: p3,
                smooth: false,
            });
        }
        while i < toks.len() {
            let c = match toks[i] {
                Token::Cmd(c) => {
                    i += 1;
                    cmd = Some(c);
                    c
                }
                Token::Num(_) => match cmd {
                    Some('M') => 'L',
                    Some('m') => 'l',
                    Some(c) => c,
                    None => return Err("path data must start with M".into()),
                },
            };
            if cur.is_none() && !c.eq_ignore_ascii_case(&'M') {
                return Err("path data must start with M".into());
            }
            let rel = c.is_ascii_lowercase();
            let r = |v: f64, base: f64| if rel { base + v } else { v };
            match c.to_ascii_uppercase() {
                'M' => {
                    if let Some(sp) = cur.take()
                        && !sp.anchors.is_empty()
                    {
                        path.subpaths.push(sp);
                    }
                    let v = take(&mut i, 2)?;
                    pos = (r(v[0], pos.0), r(v[1], pos.1));
                    start = pos;
                    cur = Some(SubPath {
                        anchors: vec![Anchor::corner(pos)],
                        closed: false,
                    });
                    last_ctrl = None;
                }
                'L' | 'H' | 'V' => {
                    let p = match c.to_ascii_uppercase() {
                        'L' => {
                            let v = take(&mut i, 2)?;
                            (r(v[0], pos.0), r(v[1], pos.1))
                        }
                        'H' => (r(take(&mut i, 1)?[0], pos.0), pos.1),
                        _ => (pos.0, r(take(&mut i, 1)?[0], pos.1)),
                    };
                    cubic(&mut cur, pos, pos, p, p);
                    pos = p;
                    last_ctrl = None;
                }
                'C' => {
                    let v = take(&mut i, 6)?;
                    let (c1, c2, p) = (
                        (r(v[0], pos.0), r(v[1], pos.1)),
                        (r(v[2], pos.0), r(v[3], pos.1)),
                        (r(v[4], pos.0), r(v[5], pos.1)),
                    );
                    cubic(&mut cur, pos, c1, c2, p);
                    last_ctrl = Some(c2);
                    pos = p;
                }
                'S' => {
                    let v = take(&mut i, 4)?;
                    let c1 = last_ctrl.map_or(pos, |lc| (2.0 * pos.0 - lc.0, 2.0 * pos.1 - lc.1));
                    let (c2, p) = (
                        (r(v[0], pos.0), r(v[1], pos.1)),
                        (r(v[2], pos.0), r(v[3], pos.1)),
                    );
                    cubic(&mut cur, pos, c1, c2, p);
                    last_ctrl = Some(c2);
                    pos = p;
                }
                'Q' | 'T' => {
                    let (q, p) = if c.eq_ignore_ascii_case(&'Q') {
                        let v = take(&mut i, 4)?;
                        (
                            (r(v[0], pos.0), r(v[1], pos.1)),
                            (r(v[2], pos.0), r(v[3], pos.1)),
                        )
                    } else {
                        let v = take(&mut i, 2)?;
                        (
                            last_ctrl.map_or(pos, |lc| (2.0 * pos.0 - lc.0, 2.0 * pos.1 - lc.1)),
                            (r(v[0], pos.0), r(v[1], pos.1)),
                        )
                    };
                    // Quadratic → cubic.
                    let c1 = (
                        pos.0 + 2.0 / 3.0 * (q.0 - pos.0),
                        pos.1 + 2.0 / 3.0 * (q.1 - pos.1),
                    );
                    let c2 = (p.0 + 2.0 / 3.0 * (q.0 - p.0), p.1 + 2.0 / 3.0 * (q.1 - p.1));
                    cubic(&mut cur, pos, c1, c2, p);
                    last_ctrl = Some(q);
                    pos = p;
                }
                'Z' => {
                    if let Some(mut sp) = cur.take() {
                        // A closing segment back to the start becomes the wrap-around.
                        if sp.anchors.len() > 1
                            && dist(sp.anchors.last().expect("non-empty").p, start) < 1e-6
                        {
                            let last = sp.anchors.pop().expect("non-empty");
                            sp.anchors[0].h_in = last.h_in;
                            sp.anchors[0].smooth = sp.anchors[0].has_handles();
                        }
                        sp.closed = true;
                        path.subpaths.push(sp);
                    }
                    pos = start;
                    last_ctrl = None;
                }
                'A' => return Err("arcs (A) are not supported; use C curves".into()),
                other => return Err(format!("unknown path command {other:?}")),
            }
            if path.anchor_count() + cur.as_ref().map_or(0, |s| s.anchors.len()) > MAX_ANCHORS {
                return Err(format!("more than {MAX_ANCHORS} anchors"));
            }
        }
        if let Some(sp) = cur.take()
            && !sp.anchors.is_empty()
        {
            path.subpaths.push(sp);
        }
        if path.is_empty() {
            return Err("the path has no points".into());
        }
        Ok(path)
    }
}

enum Token {
    Cmd(char),
    Num(f64),
}

/// What is under the pointer.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Hit {
    Anchor(usize, usize),
    HandleIn(usize, usize),
    HandleOut(usize, usize),
}

/// Non-zero winding fill of closed polylines, 4×4 supersampled.
pub fn fill_coverage(polys: &[Vec<Pt>], w: u32, h: u32) -> Mask {
    let mask = Mask::empty(w, h, 0);
    let (mut x0, mut y0, mut x1, mut y1) = (f64::MAX, f64::MAX, f64::MIN, f64::MIN);
    for p in polys.iter().flatten() {
        x0 = x0.min(p.0);
        y0 = y0.min(p.1);
        x1 = x1.max(p.0);
        y1 = y1.max(p.1);
    }
    if x0 > x1 {
        return mask;
    }
    let b = IRect::new(
        x0.floor() as i32 - 1,
        y0.floor() as i32 - 1,
        (x1 - x0).ceil() as i32 + 3,
        (y1 - y0).ceil() as i32 + 3,
    )
    .intersect(&mask.bounds());
    if b.is_empty() {
        return mask;
    }
    const S: usize = 4;
    let mut acc = vec![0u16; (b.w * b.h) as usize];
    let mut crossings: Vec<(f64, i32)> = Vec::new();
    for row in 0..b.h {
        for sy in 0..S {
            let y = b.y as f64 + row as f64 + (sy as f64 + 0.5) / S as f64;
            crossings.clear();
            for poly in polys {
                let n = poly.len();
                if n < 2 {
                    continue;
                }
                for k in 0..n {
                    let (a, c) = (poly[k], poly[(k + 1) % n]);
                    if (a.1 <= y) == (c.1 <= y) {
                        continue;
                    }
                    let t = (y - a.1) / (c.1 - a.1);
                    crossings.push((a.0 + (c.0 - a.0) * t, if c.1 > a.1 { 1 } else { -1 }));
                }
            }
            if crossings.is_empty() {
                continue;
            }
            crossings.sort_by(|p, q| p.0.total_cmp(&q.0));
            let mut wind = 0;
            for k in 0..crossings.len() {
                wind += crossings[k].1;
                if wind == 0 || k + 1 >= crossings.len() {
                    continue;
                }
                let (xa, xb) = (crossings[k].0, crossings[k + 1].0);
                // Horizontal coverage in S sub-samples per pixel.
                let (ca, cb) = ((xa - b.x as f64) * S as f64, (xb - b.x as f64) * S as f64);
                let (sa, sb) = (
                    ca.round().max(0.0) as i64,
                    cb.round().min((b.w as usize * S) as f64) as i64,
                );
                for sx in sa..sb {
                    acc[(row * b.w) as usize + (sx as usize / S)] += 1;
                }
            }
        }
    }
    let data: Vec<u8> = acc
        .into_iter()
        .map(|v| ((v as u32 * 255) / (S * S) as u32).min(255) as u8)
        .collect();
    mask.write_rect(b, &data)
}

/// Round-joined, round-capped stroke coverage from a signed distance to
/// each polyline segment.
/// A segment with its padded pixel box: ends, x0, x1, y0, y1.
type SegBox = ((f64, f64), (f64, f64), i32, i32, i32, i32);

pub fn stroke_coverage(polys: &[(Vec<Pt>, bool)], width: f64, w: u32, h: u32) -> Mask {
    let mask = Mask::empty(w, h, 0);
    let hw = (width / 2.0).max(0.0);
    let mut segs: Vec<(Pt, Pt)> = Vec::new();
    for (poly, closed) in polys {
        let n = poly.len();
        if n == 1 {
            segs.push((poly[0], poly[0]));
        }
        for k in 0..n.saturating_sub(1) {
            segs.push((poly[k], poly[k + 1]));
        }
        if *closed && n > 2 {
            segs.push((poly[n - 1], poly[0]));
        }
    }
    if segs.is_empty() {
        return mask;
    }
    let (mut x0, mut y0, mut x1, mut y1) = (f64::MAX, f64::MAX, f64::MIN, f64::MIN);
    for (a, c) in &segs {
        x0 = x0.min(a.0.min(c.0));
        y0 = y0.min(a.1.min(c.1));
        x1 = x1.max(a.0.max(c.0));
        y1 = y1.max(a.1.max(c.1));
    }
    let pad = hw + 1.5;
    let b = IRect::new(
        (x0 - pad).floor() as i32,
        (y0 - pad).floor() as i32,
        (x1 - x0 + 2.0 * pad).ceil() as i32 + 1,
        (y1 - y0 + 2.0 * pad).ceil() as i32 + 1,
    )
    .intersect(&mask.bounds());
    if b.is_empty() {
        return mask;
    }
    // Rows in parallel: each row visits only the segments whose padded
    // box reaches it, and keeps the greatest coverage.
    use rayon::prelude::*;
    let seg_boxes: Vec<SegBox> = segs
        .iter()
        .map(|(a, c)| {
            let sb = IRect::new(
                (a.0.min(c.0) - pad).floor() as i32,
                (a.1.min(c.1) - pad).floor() as i32,
                ((a.0 - c.0).abs() + 2.0 * pad).ceil() as i32 + 1,
                ((a.1 - c.1).abs() + 2.0 * pad).ceil() as i32 + 1,
            )
            .intersect(&b);
            (*a, *c, sb.x, sb.right(), sb.y, sb.bottom())
        })
        .collect();
    let mut data = vec![0u8; (b.w * b.h) as usize];
    data.par_chunks_mut(b.w as usize)
        .enumerate()
        .for_each(|(row, out)| {
            let y = b.y + row as i32;
            let py = y as f64 + 0.5;
            for &(a, c, x0, x1, y0, y1) in &seg_boxes {
                if y < y0 || y >= y1 {
                    continue;
                }
                let (dx, dy) = (c.0 - a.0, c.1 - a.1);
                let len2 = dx * dx + dy * dy;
                for x in x0..x1 {
                    let px = x as f64 + 0.5;
                    let t = if len2 > 1e-12 {
                        ((px - a.0) * dx + (py - a.1) * dy) / len2
                    } else {
                        0.0
                    }
                    .clamp(0.0, 1.0);
                    let (qx, qy) = (a.0 + dx * t, a.1 + dy * t);
                    let d = (px - qx).hypot(py - qy);
                    let cov = (hw + 0.5 - d).clamp(0.0, 1.0);
                    if cov > 0.0 {
                        let v = (cov * 255.0).round() as u8;
                        let o = &mut out[(x - b.x) as usize];
                        *o = (*o).max(v);
                    }
                }
            }
        });
    mask.write_rect(b, &data)
}

#[cfg(test)]
mod tests {
    use super::*;

    fn square() -> Path {
        Path::from_svg("M 10 10 L 50 10 L 50 50 L 10 50 Z").unwrap()
    }

    #[test]
    fn svg_round_trips_and_parses_relative_and_curves() {
        let p = square();
        assert_eq!(p.subpaths.len(), 1);
        assert!(p.subpaths[0].closed);
        assert_eq!(
            p.anchor_count(),
            4,
            "the closing L back to start folds into Z"
        );
        assert_eq!(p.to_svg(), "M 10 10 L 50 10 L 50 50 L 10 50 Z");
        let c =
            Path::from_svg("m 10,20 c 5 -10, 15 -10, 20 0 s 15 10 20 0 q 5 5 10 0 t 10 0").unwrap();
        assert_eq!(c.anchor_count(), 5);
        let back = Path::from_svg(&c.to_svg()).unwrap();
        for (a, b) in c.subpaths[0].anchors.iter().zip(&back.subpaths[0].anchors) {
            assert!(dist(a.p, b.p) < 0.02 && dist(a.h_out, b.h_out) < 0.02);
        }
        assert!(Path::from_svg("M 0 0 A 1 1 0 0 0 1 1").is_err());
        assert!(Path::from_svg("L 1 2").is_err());
    }

    #[test]
    fn fill_and_stroke_cover_the_right_pixels() {
        let p = square();
        let f = p.fill_mask(64, 64);
        assert_eq!(f.get(30, 30), 255, "inside");
        assert_eq!(f.get(5, 30), 0, "outside");
        assert_eq!(f.get(9, 30), 0, "pixel-aligned edges are crisp");
        let tri = Path::from_svg("M 10 10 L 50 10 L 10 50 Z")
            .unwrap()
            .fill_mask(64, 64);
        assert!(
            (60..=200).contains(&tri.get(29, 30)),
            "the diagonal is antialiased: {}",
            tri.get(29, 30)
        );
        let s = p.stroke_mask(4.0, 64, 64);
        assert_eq!(s.get(30, 10), 255, "on the top edge");
        assert_eq!(s.get(30, 30), 0, "the interior is not stroked");
        let style = PathStyle {
            stroke: Some([255, 0, 0, 255]),
            width: 2.0,
            fill: Some([0, 0, 255, 255]),
        };
        let r = p.rasterize(&style, 64, 64);
        let inside = color::px_to_f(r.get(30, 30));
        let edge = color::px_to_f(r.get(30, 10));
        assert!(inside[2] > 0.99 && inside[0] < 0.01);
        assert!(edge[0] > 0.99, "stroke draws over the fill: {edge:?}");
        assert_eq!(r.get(2, 2), [0; 4]);
    }

    #[test]
    fn hit_insert_and_remove() {
        let mut p = square();
        assert_eq!(p.hit((11.0, 9.0), 4.0), Some(Hit::Anchor(0, 0)));
        assert_eq!(p.hit((30.0, 30.0), 4.0), None);
        let (si, seg, t) = p.nearest_on_curve((30.0, 10.5), 3.0).unwrap();
        assert_eq!((si, seg), (0, 0));
        assert!((t - 0.5).abs() < 0.05);
        let idx = p.insert_at(si, seg, t).unwrap();
        assert_eq!(idx, 1);
        assert_eq!(p.anchor_count(), 5);
        assert!(dist(p.subpaths[0].anchors[1].p, (30.0, 10.0)) < 0.5);
        p.remove_anchor(0, 1);
        assert_eq!(p.anchor_count(), 4);
        let mut q = Path::from_svg("M 0 0 L 10 0").unwrap();
        q.translate(5.0, 5.0);
        assert_eq!(q.to_svg(), "M 5 5 L 15 5");
    }
}
