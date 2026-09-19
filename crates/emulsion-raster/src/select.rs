//! Selections as 8-bit coverage masks in document space, and the operations
//! that build and modify them.

use crate::color;
use crate::geom::IRect;
use crate::image::{Mask, Raster};

#[derive(Clone, Copy, Debug, PartialEq, Eq, Default)]
pub enum Combine {
    #[default]
    Replace,
    Add,
    Subtract,
    Intersect,
}

/// Rasterise a closed polygon with 4×4 supersampling. Even-odd fill.
pub fn polygon(w: u32, h: u32, pts: &[(f32, f32)]) -> Mask {
    if pts.len() < 3 {
        return Mask::empty(w, h, 0);
    }
    let (mut x0, mut y0, mut x1, mut y1) = (f32::MAX, f32::MAX, f32::MIN, f32::MIN);
    for (x, y) in pts {
        x0 = x0.min(*x);
        y0 = y0.min(*y);
        x1 = x1.max(*x);
        y1 = y1.max(*y);
    }
    let r = IRect::new(
        x0.floor() as i32,
        y0.floor() as i32,
        (x1.ceil() - x0.floor()) as i32 + 1,
        (y1.ceil() - y0.floor()) as i32 + 1,
    )
    .intersect(&IRect::new(0, 0, w as i32, h as i32));
    if r.is_empty() {
        return Mask::empty(w, h, 0);
    }
    const SS: usize = 4;
    let mut acc = vec![0u16; (r.w * r.h) as usize];
    let mut xs: Vec<f32> = Vec::new();
    for row in 0..r.h {
        for sub in 0..SS {
            let sy = r.y as f32 + row as f32 + (sub as f32 + 0.5) / SS as f32;
            xs.clear();
            for i in 0..pts.len() {
                let (ax, ay) = pts[i];
                let (bx, by) = pts[(i + 1) % pts.len()];
                if (ay <= sy && by > sy) || (by <= sy && ay > sy) {
                    xs.push(ax + (sy - ay) / (by - ay) * (bx - ax));
                }
            }
            xs.sort_by(f32::total_cmp);
            for pair in xs.as_chunks::<2>().0 {
                // Horizontal coverage is exact per pixel; vertical is supersampled.
                let (a, b) = (
                    (pair[0] - r.x as f32).max(0.0),
                    (pair[1] - r.x as f32).min(r.w as f32),
                );
                if b <= a {
                    continue;
                }
                let (ia, ib) = (a.floor() as i32, (b.ceil() as i32).min(r.w));
                for px in ia..ib {
                    let cov = (b.min(px as f32 + 1.0) - a.max(px as f32)).clamp(0.0, 1.0);
                    let i = (row * r.w + px) as usize;
                    acc[i] = acc[i].saturating_add((cov * 255.0 / SS as f32).round() as u16);
                }
            }
        }
    }
    let dense: Vec<u8> = acc.into_iter().map(|v| v.min(255) as u8).collect();
    Mask::empty(w, h, 0).write_rect(r, &dense)
}

pub fn rect(w: u32, h: u32, x: f32, y: f32, rw: f32, rh: f32) -> Mask {
    polygon(w, h, &[(x, y), (x + rw, y), (x + rw, y + rh), (x, y + rh)])
}

pub fn ellipse(w: u32, h: u32, x: f32, y: f32, rw: f32, rh: f32) -> Mask {
    let (cx, cy, rx, ry) = (x + rw / 2.0, y + rh / 2.0, rw / 2.0, rh / 2.0);
    let n = ((rx.max(ry) * 0.8) as usize).clamp(24, 720);
    let pts: Vec<(f32, f32)> = (0..n)
        .map(|i| {
            let t = i as f32 / n as f32 * std::f32::consts::TAU;
            (cx + rx * t.cos(), cy + ry * t.sin())
        })
        .collect();
    polygon(w, h, &pts)
}

pub fn all(w: u32, h: u32) -> Mask {
    Mask::empty(w, h, 255)
}

/// Pixel-wise combination of two same-sized masks.
pub fn combine(a: Option<&Mask>, b: &Mask, mode: Combine) -> Mask {
    let Some(a) = a else {
        return match mode {
            Combine::Replace | Combine::Add => b.clone(),
            Combine::Subtract | Combine::Intersect => Mask::empty(b.width(), b.height(), 0),
        };
    };
    if mode == Combine::Replace {
        return b.clone();
    }
    let f = |x: u8, y: u8| -> u8 {
        let (x, y) = (x as u32, y as u32);
        match mode {
            Combine::Add => (x + y - x * y / 255).min(255) as u8,
            Combine::Subtract => (x * (255 - y) / 255) as u8,
            Combine::Intersect => (x * y / 255) as u8,
            Combine::Replace => y as u8,
        }
    };
    let (w, h) = (b.width(), b.height());
    let fill = f(a.fill(), b.fill());
    let region = a.tile_bounds().union(&b.tile_bounds());
    let (ad, bd) = (a.read_rect(region), b.read_rect(region));
    let out: Vec<u8> = ad.iter().zip(&bd).map(|(x, y)| f(*x, *y)).collect();
    Mask::empty(w, h, fill).write_rect(region, &out)
}

pub fn invert(m: &Mask) -> Mask {
    let region = m.bounds();
    let d: Vec<u8> = m.read_rect(region).into_iter().map(|v| 255 - v).collect();
    Mask::empty(m.width(), m.height(), 255 - m.fill()).write_rect(region, &d)
}

/// The mask's non-zero area, in pixels (coarse: whole stored tiles, or the
/// whole plane when the fill is non-zero).
pub fn extent(m: &Mask) -> IRect {
    if m.fill() != 0 {
        m.bounds()
    } else {
        m.tile_bounds()
    }
}

/// Move, scale and rotate a selection. `to_new` maps old document
/// positions to new ones. Coverage is resampled bilinearly, so edges stay
/// soft; a pure whole-pixel move copies exactly.
pub fn transform(m: &Mask, to_new: glam::DAffine2) -> Mask {
    let (w, h) = (m.width(), m.height());
    let e = extent(m);
    if e.is_empty() {
        return Mask::empty(w, h, 0);
    }
    let t = to_new.translation;
    let integer_move =
        to_new.matrix2 == glam::DMat2::IDENTITY && t.x.fract() == 0.0 && t.y.fract() == 0.0;
    let corners = [
        (e.x, e.y),
        (e.right(), e.y),
        (e.x, e.bottom()),
        (e.right(), e.bottom()),
    ]
    .map(|(x, y)| to_new.transform_point2(glam::dvec2(x as f64, y as f64)));
    let (mut x0, mut y0, mut x1, mut y1) = (f64::MAX, f64::MAX, f64::MIN, f64::MIN);
    for c in corners {
        x0 = x0.min(c.x);
        y0 = y0.min(c.y);
        x1 = x1.max(c.x);
        y1 = y1.max(c.y);
    }
    let out_rect = IRect::new(
        x0.floor() as i32 - 1,
        y0.floor() as i32 - 1,
        (x1.ceil() - x0.floor()) as i32 + 2,
        (y1.ceil() - y0.floor()) as i32 + 2,
    )
    .intersect(&m.bounds());
    let blank = Mask::empty(w, h, 0);
    if out_rect.is_empty() {
        return blank;
    }
    if integer_move {
        let src = IRect::new(
            out_rect.x - t.x as i32,
            out_rect.y - t.y as i32,
            out_rect.w,
            out_rect.h,
        );
        return blank.write_rect(out_rect, &m.read_rect(src));
    }
    let src = m.read_rect(e);
    let inv = to_new.inverse();
    let sample = |x: f64, y: f64| -> f32 {
        let (fx, fy) = (x - 0.5 - e.x as f64, y - 0.5 - e.y as f64);
        let (ix, iy) = (fx.floor(), fy.floor());
        let (ax, ay) = ((fx - ix) as f32, (fy - iy) as f32);
        let at = |x: i64, y: i64| -> f32 {
            if x < 0 || y < 0 || x >= e.w as i64 || y >= e.h as i64 {
                0.0
            } else {
                src[(y * e.w as i64 + x) as usize] as f32
            }
        };
        let (ix, iy) = (ix as i64, iy as i64);
        let top = at(ix, iy) * (1.0 - ax) + at(ix + 1, iy) * ax;
        let bot = at(ix, iy + 1) * (1.0 - ax) + at(ix + 1, iy + 1) * ax;
        top * (1.0 - ay) + bot * ay
    };
    let mut out = vec![0u8; (out_rect.w * out_rect.h) as usize];
    for y in 0..out_rect.h {
        for x in 0..out_rect.w {
            let p = inv.transform_point2(glam::dvec2(
                (out_rect.x + x) as f64 + 0.5,
                (out_rect.y + y) as f64 + 0.5,
            ));
            out[(y * out_rect.w + x) as usize] = sample(p.x, p.y).round().clamp(0.0, 255.0) as u8;
        }
    }
    blank.write_rect(out_rect, &out)
}

/// Quick Select: grow a region from brushed seed pixels, following colour.
///
/// A priority flood from the seeds: each step costs the colour difference
/// between neighbours plus the distance from the seeds' mean colour, so the
/// region spreads through similar colour and stops at edges. `strength`
/// (1–100) is how far it may spread. `image` is straight sRGBA8.
pub fn quick_select(image: &[u8], w: u32, h: u32, seeds: &[(u32, u32)], strength: f32) -> Mask {
    use std::cmp::Reverse;
    use std::collections::BinaryHeap;
    let (wu, hu) = (w as usize, h as usize);
    let n = wu * hu;
    let px = |i: usize| -> [f32; 3] {
        [
            image[i * 4] as f32,
            image[i * 4 + 1] as f32,
            image[i * 4 + 2] as f32,
        ]
    };
    let seeds: Vec<usize> = seeds
        .iter()
        .filter(|(x, y)| *x < w && *y < h)
        .map(|(x, y)| *y as usize * wu + *x as usize)
        .collect();
    if seeds.is_empty() {
        return Mask::empty(w, h, 0);
    }
    let mut mean = [0f32; 3];
    for &i in &seeds {
        let p = px(i);
        for c in 0..3 {
            mean[c] += p[c] / seeds.len() as f32;
        }
    }
    // Typical spread of the brushed colours sets the scale of "different".
    let spread = seeds
        .iter()
        .map(|&i| {
            let p = px(i);
            ((p[0] - mean[0]).powi(2) + (p[1] - mean[1]).powi(2) + (p[2] - mean[2]).powi(2)).sqrt()
        })
        .fold(0.0f32, f32::max)
        .max(12.0);
    let limit = strength.clamp(1.0, 100.0) * 6.0;
    let mut cost = vec![f32::MAX; n];
    let mut heap = BinaryHeap::new();
    for &i in &seeds {
        cost[i] = 0.0;
        heap.push(Reverse((0u32, i)));
    }
    let dist = |a: [f32; 3], b: [f32; 3]| {
        ((a[0] - b[0]).powi(2) + (a[1] - b[1]).powi(2) + (a[2] - b[2]).powi(2)).sqrt()
    };
    while let Some(Reverse((c, i))) = heap.pop() {
        let c = c as f32 / 16.0;
        if c > cost[i] {
            continue;
        }
        let (x, y) = (i % wu, i / wu);
        let pi = px(i);
        for (dx, dy) in [(-1i32, 0i32), (1, 0), (0, -1), (0, 1)] {
            let (nx, ny) = (x as i32 + dx, y as i32 + dy);
            if nx < 0 || ny < 0 || nx as usize >= wu || ny as usize >= hu {
                continue;
            }
            let j = ny as usize * wu + nx as usize;
            let pj = px(j);
            let from_model = (dist(pj, mean) - spread).max(0.0);
            let step = 0.05 + dist(pi, pj) * 0.6 + from_model * 0.4;
            let nc = c + step;
            if nc < cost[j] && nc <= limit {
                cost[j] = nc;
                heap.push(Reverse(((nc * 16.0) as u32, j)));
            }
        }
    }
    let data: Vec<u8> = cost
        .iter()
        .map(|c| if *c <= limit { 255 } else { 0 })
        .collect();
    Mask::from_gray8(w, h, &data)
}

/// Edge strength of a straight sRGBA8 image, 0–1 per pixel (Sobel on luma).
pub fn edges(image: &[u8], w: u32, h: u32) -> Vec<f32> {
    let (wu, hu) = (w as usize, h as usize);
    let luma: Vec<f32> = image
        .as_chunks::<4>()
        .0
        .iter()
        .map(|p| 0.299 * p[0] as f32 + 0.587 * p[1] as f32 + 0.114 * p[2] as f32)
        .collect();
    let at = |x: isize, y: isize| {
        luma[y.clamp(0, hu as isize - 1) as usize * wu + x.clamp(0, wu as isize - 1) as usize]
    };
    let mut out = vec![0.0; wu * hu];
    let mut max = 1.0f32;
    for y in 0..hu as isize {
        for x in 0..wu as isize {
            let gx = at(x + 1, y - 1) + 2.0 * at(x + 1, y) + at(x + 1, y + 1)
                - at(x - 1, y - 1)
                - 2.0 * at(x - 1, y)
                - at(x - 1, y + 1);
            let gy = at(x - 1, y + 1) + 2.0 * at(x, y + 1) + at(x + 1, y + 1)
                - at(x - 1, y - 1)
                - 2.0 * at(x, y - 1)
                - at(x + 1, y - 1);
            let g = (gx * gx + gy * gy).sqrt();
            max = max.max(g);
            out[y as usize * wu + x as usize] = g;
        }
    }
    for v in &mut out {
        *v /= max;
    }
    out
}

/// Magnetic lasso: the cheapest path from `a` to `b` along strong edges,
/// searched within `margin` pixels of the box the two points span.
pub fn live_wire(
    edge: &[f32],
    w: u32,
    h: u32,
    a: (u32, u32),
    b: (u32, u32),
    margin: u32,
) -> Vec<(u32, u32)> {
    use std::cmp::Reverse;
    use std::collections::BinaryHeap;
    let (wu, hu) = (w as usize, h as usize);
    let x0 = a.0.min(b.0).saturating_sub(margin) as usize;
    let y0 = a.1.min(b.1).saturating_sub(margin) as usize;
    let x1 = ((a.0.max(b.0) + margin) as usize).min(wu - 1);
    let y1 = ((a.1.max(b.1) + margin) as usize).min(hu - 1);
    let (bw, bh) = (x1 - x0 + 1, y1 - y0 + 1);
    let local = |x: usize, y: usize| (y - y0) * bw + (x - x0);
    let mut cost = vec![f32::MAX; bw * bh];
    let mut from = vec![usize::MAX; bw * bh];
    let (sa, sb) = (
        local(a.0 as usize, a.1 as usize),
        local(b.0 as usize, b.1 as usize),
    );
    cost[sa] = 0.0;
    let mut heap = BinaryHeap::from([Reverse((0u32, sa))]);
    while let Some(Reverse((c, i))) = heap.pop() {
        let c = c as f32 / 64.0;
        if i == sb {
            break;
        }
        if c > cost[i] {
            continue;
        }
        let (lx, ly) = (i % bw, i / bw);
        for (dx, dy) in [
            (-1i32, 0i32),
            (1, 0),
            (0, -1),
            (0, 1),
            (-1, -1),
            (1, 1),
            (-1, 1),
            (1, -1),
        ] {
            let (nx, ny) = (lx as i32 + dx, ly as i32 + dy);
            if nx < 0 || ny < 0 || nx as usize >= bw || ny as usize >= bh {
                continue;
            }
            let j = ny as usize * bw + nx as usize;
            let e = edge[(ny as usize + y0) * wu + nx as usize + x0];
            let len = if dx != 0 && dy != 0 { 1.414 } else { 1.0 };
            let nc = c + len * (0.08 + (1.0 - e).powi(2));
            if nc < cost[j] {
                cost[j] = nc;
                from[j] = i;
                heap.push(Reverse(((nc * 64.0) as u32, j)));
            }
        }
    }
    let mut path = Vec::new();
    let mut i = sb;
    while i != usize::MAX {
        path.push(((i % bw + x0) as u32, (i / bw + y0) as u32));
        if i == sa {
            break;
        }
        i = from[i];
    }
    path.reverse();
    path
}

/// Exact bounding box of coverage above zero.
pub fn bounds(m: &Mask) -> IRect {
    let e = extent(m);
    if e.is_empty() {
        return e;
    }
    let d = m.read_rect(e);
    let (mut x0, mut y0, mut x1, mut y1) = (i32::MAX, i32::MAX, i32::MIN, i32::MIN);
    for y in 0..e.h {
        for x in 0..e.w {
            if d[(y * e.w + x) as usize] > 0 {
                x0 = x0.min(x);
                y0 = y0.min(y);
                x1 = x1.max(x);
                y1 = y1.max(y);
            }
        }
    }
    if x1 < x0 {
        return IRect::default();
    }
    IRect::new(e.x + x0, e.y + y0, x1 - x0 + 1, y1 - y0 + 1)
}

/// One row of a running box blur, edges clamped.
fn box_blur_line(src: &[f32], dst: &mut [f32], r: usize) {
    let n = src.len();
    let mut sum: f32 = src[..=r.min(n - 1)].iter().sum();
    sum += src[0] * r as f32;
    let inv = 1.0 / (2 * r + 1) as f32;
    for i in 0..n {
        dst[i] = sum * inv;
        let add = src[(i + r + 1).min(n - 1)];
        let sub = src[i.saturating_sub(r)];
        sum += add - sub;
    }
}

/// Box-blur every row of a `w`×`h` buffer, rows in parallel.
fn box_blur_rows(src: &[f32], dst: &mut [f32], w: usize, r: usize) {
    use rayon::prelude::*;
    dst.par_chunks_mut(w)
        .zip(src.par_chunks(w))
        .for_each(|(d, s)| box_blur_line(s, d, r));
}

/// `w`×`h` → `h`×`w`, output rows in parallel.
fn transpose(src: &[f32], w: usize, h: usize) -> Vec<f32> {
    use rayon::prelude::*;
    let mut out = vec![0.0f32; w * h];
    out.par_chunks_mut(h).enumerate().for_each(|(x, col)| {
        for (y, v) in col.iter_mut().enumerate() {
            *v = src[y * w + x];
        }
    });
    out
}

/// Feather edges by `radius` pixels (three box passes ≈ Gaussian).
pub fn feather(m: &Mask, radius: f32) -> Mask {
    if radius < 0.5 {
        return m.clone();
    }
    let r = (radius / 1.7).round().max(1.0) as usize;
    let pad = (r * 3 + 2) as i32;
    let e = extent(m);
    if e.is_empty() {
        return m.clone();
    }
    let region =
        IRect::new(e.x - pad, e.y - pad, e.w + 2 * pad, e.h + 2 * pad).intersect(&m.bounds());
    let (w, h) = (region.w as usize, region.h as usize);
    let mut a: Vec<f32> = m.read_rect(region).into_iter().map(|v| v as f32).collect();
    let mut b = vec![0.0; a.len()];
    // Box passes commute, so all three horizontal passes run first, then
    // the buffer is transposed once for the three vertical ones.
    for _ in 0..3 {
        box_blur_rows(&a, &mut b, w, r);
        std::mem::swap(&mut a, &mut b);
    }
    let mut a = transpose(&a, w, h);
    let mut b = vec![0.0; a.len()];
    for _ in 0..3 {
        box_blur_rows(&a, &mut b, h, r);
        std::mem::swap(&mut a, &mut b);
    }
    let a = transpose(&a, h, w);
    let out: Vec<u8> = a
        .into_iter()
        .map(|v| v.round().clamp(0.0, 255.0) as u8)
        .collect();
    m.write_rect(region, &out)
}

/// Grow (`r > 0`) or shrink (`r < 0`) by a square max/min filter.
pub fn grow(m: &Mask, r: i32) -> Mask {
    if r == 0 {
        return m.clone();
    }
    let k = r.unsigned_abs() as i32;
    let e = extent(m);
    let region = IRect::new(e.x - k, e.y - k, e.w + 2 * k, e.h + 2 * k).intersect(&m.bounds());
    if region.is_empty() {
        return m.clone();
    }
    let (w, h) = (region.w as usize, region.h as usize);
    let src = m.read_rect(region);
    let pick = |a: u8, b: u8| if r > 0 { a.max(b) } else { a.min(b) };
    let mut tmp = src.clone();
    for y in 0..h {
        for x in 0..w {
            let mut v = src[y * w + x];
            for d in 1..=k as usize {
                if x >= d {
                    v = pick(v, src[y * w + x - d]);
                }
                if x + d < w {
                    v = pick(v, src[y * w + x + d]);
                }
            }
            tmp[y * w + x] = v;
        }
    }
    let mut out = tmp.clone();
    for y in 0..h {
        for x in 0..w {
            let mut v = tmp[y * w + x];
            for d in 1..=k as usize {
                if y >= d {
                    v = pick(v, tmp[(y - d) * w + x]);
                }
                if y + d < h {
                    v = pick(v, tmp[(y + d) * w + x]);
                }
            }
            out[y * w + x] = v;
        }
    }
    m.write_rect(region, &out)
}

/// Select pixels whose colour is within `tolerance` (0–255, per channel on
/// sRGB values) of the pixel at (x, y). `image` is straight sRGBA8.
pub fn by_color(
    image: &[u8],
    w: u32,
    h: u32,
    x: u32,
    y: u32,
    tolerance: u8,
    contiguous: bool,
) -> Mask {
    let idx = |x: u32, y: u32| ((y * w + x) * 4) as usize;
    let seed = &image[idx(x, y)..idx(x, y) + 4];
    let tol = tolerance as i32;
    let close = |i: usize| {
        let p = &image[i..i + 4];
        (0..4).all(|c| (p[c] as i32 - seed[c] as i32).abs() <= tol)
    };
    let mut out = vec![0u8; (w * h) as usize];
    if contiguous {
        let mut stack = vec![(x, y)];
        out[(y * w + x) as usize] = 255;
        while let Some((cx, cy)) = stack.pop() {
            for (nx, ny) in [
                (cx.wrapping_sub(1), cy),
                (cx + 1, cy),
                (cx, cy.wrapping_sub(1)),
                (cx, cy + 1),
            ] {
                if nx >= w || ny >= h {
                    continue;
                }
                let o = (ny * w + nx) as usize;
                if out[o] == 0 && close(idx(nx, ny)) {
                    out[o] = 255;
                    stack.push((nx, ny));
                }
            }
        }
    } else {
        for py in 0..h {
            for px in 0..w {
                if close(idx(px, py)) {
                    out[(py * w + px) as usize] = 255;
                }
            }
        }
    }
    Mask::from_pixels(w, h, 0, &out)
}

/// A mask from a raster's alpha (Photoshop's "load selection from layer").
pub fn from_alpha(r: &Raster) -> Mask {
    let region = r.tile_bounds();
    let d: Vec<u8> = r
        .read_rect(region)
        .into_iter()
        .map(|p| (color::u16_to_f(p[3]) * 255.0).round() as u8)
        .collect();
    Mask::empty(r.width(), r.height(), 0).write_rect(region, &d)
}

/// Horizontal and vertical edge segments of the 50 % contour, in the mask's
/// pixel units scaled by `2^level` (for marching ants at a mip level).
pub fn outline(m: &Mask, level: u32) -> Vec<(f32, f32, f32, f32)> {
    let s = (1u32 << level) as f32;
    let (lw, lh) = m.level_size(level);
    let e = extent(m);
    if e.is_empty() {
        return vec![];
    }
    let t = crate::tile::TILE as i32;
    let d = 1i32 << level;
    let region = IRect::new(e.x / d - 1, e.y / d - 1, e.w / d + 3, e.h / d + 3)
        .intersect(&IRect::new(0, 0, lw as i32, lh as i32));
    let at = |x: i32, y: i32| -> bool {
        if x < 0 || y < 0 || x >= lw as i32 || y >= lh as i32 {
            return false;
        }
        match m.tile(level, crate::geom::TileCoord::new(x / t, y / t)) {
            Some(tile) => tile[((y % t) * t + (x % t)) as usize] >= 128,
            None => m.fill() >= 128,
        }
    };
    let mut segs = Vec::new();
    for y in region.y..=region.bottom() {
        let mut run: Option<i32> = None;
        for x in region.x..=region.right() {
            let edge = at(x, y) != at(x, y - 1);
            match (edge, run) {
                (true, None) => run = Some(x),
                (false, Some(x0)) => {
                    segs.push((x0 as f32 * s, y as f32 * s, x as f32 * s, y as f32 * s));
                    run = None;
                }
                _ => {}
            }
        }
    }
    for x in region.x..=region.right() {
        let mut run: Option<i32> = None;
        for y in region.y..=region.bottom() {
            let edge = at(x, y) != at(x - 1, y);
            match (edge, run) {
                (true, None) => run = Some(y),
                (false, Some(y0)) => {
                    segs.push((x as f32 * s, y0 as f32 * s, x as f32 * s, y as f32 * s));
                    run = None;
                }
                _ => {}
            }
        }
    }
    segs
}

#[cfg(test)]
mod tests {
    use super::*;

    fn count(m: &Mask) -> u32 {
        m.to_gray8().iter().filter(|v| **v >= 128).count() as u32
    }

    #[test]
    fn rect_and_ellipse_areas() {
        let r = rect(100, 100, 10.0, 10.0, 30.0, 20.0);
        assert_eq!(count(&r), 600);
        assert_eq!(bounds(&r), IRect::new(10, 10, 30, 20));
        let e = ellipse(200, 200, 0.0, 0.0, 200.0, 200.0);
        let area = count(&e) as f32;
        assert!((area - std::f32::consts::PI * 10000.0).abs() / area < 0.01);
    }

    #[test]
    fn combine_modes() {
        let a = rect(50, 50, 0.0, 0.0, 30.0, 50.0);
        let b = rect(50, 50, 20.0, 0.0, 30.0, 50.0);
        assert_eq!(count(&combine(Some(&a), &b, Combine::Add)), 2500);
        assert_eq!(count(&combine(Some(&a), &b, Combine::Intersect)), 500);
        assert_eq!(count(&combine(Some(&a), &b, Combine::Subtract)), 1000);
        assert_eq!(count(&invert(&a)), 1000);
    }

    #[test]
    fn grow_shrink_feather() {
        let a = rect(100, 100, 40.0, 40.0, 20.0, 20.0);
        assert_eq!(bounds(&grow(&a, 5)), IRect::new(35, 35, 30, 30));
        assert_eq!(bounds(&grow(&a, -5)), IRect::new(45, 45, 10, 10));
        let f = feather(&a, 6.0);
        assert!(f.get(50, 50) > 240, "centre stays selected");
        assert!(
            f.get(40, 50) > 60 && f.get(40, 50) < 200,
            "edge is soft: {}",
            f.get(40, 50)
        );
    }

    #[test]
    fn color_range() {
        let (w, h) = (20u32, 10u32);
        let img: Vec<u8> = (0..h)
            .flat_map(|_| {
                (0..w).flat_map(|x| {
                    if !(10..15).contains(&x) {
                        [200u8, 10, 10, 255]
                    } else {
                        [10, 10, 200, 255]
                    }
                })
            })
            .collect();
        assert_eq!(
            count(&by_color(&img, w, h, 0, 0, 20, true)),
            100,
            "contiguous stops at the blue band"
        );
        assert_eq!(count(&by_color(&img, w, h, 0, 0, 20, false)), 150);
    }

    #[test]
    fn outline_of_a_square() {
        let a = rect(64, 64, 10.0, 10.0, 20.0, 20.0);
        let segs = outline(&a, 0);
        let len: f32 = segs
            .iter()
            .map(|(x0, y0, x1, y1)| (x1 - x0).abs() + (y1 - y0).abs())
            .sum();
        assert_eq!(len, 80.0);
    }

    #[test]
    fn transform_moves_and_scales() {
        let m = rect(100, 100, 10.0, 10.0, 20.0, 20.0);
        let moved = transform(&m, glam::DAffine2::from_translation(glam::dvec2(30.0, 5.0)));
        assert_eq!(bounds(&moved), IRect::new(40, 15, 20, 20));
        let s = glam::DAffine2::from_translation(glam::dvec2(20.0, 20.0))
            * glam::DAffine2::from_scale(glam::dvec2(2.0, 2.0))
            * glam::DAffine2::from_translation(glam::dvec2(-20.0, -20.0));
        let b = bounds(&transform(&m, s));
        assert!((b.w - 40).abs() <= 2 && b.x.abs() <= 1, "{b:?}");
    }

    #[test]
    fn quick_select_stops_at_a_colour_edge() {
        // Left: noisy red; right: blue.
        let (w, h) = (60u32, 40u32);
        let img: Vec<u8> = (0..w * h)
            .flat_map(|i| {
                let (x, y) = (i % w, i / w);
                let n = ((x * 7 + y * 13) % 9) as u8;
                if x < 30 {
                    [200 + n, 30, 30, 255]
                } else {
                    [30, 40, 200, 255]
                }
            })
            .collect();
        let m = quick_select(&img, w, h, &[(5, 5), (10, 20), (15, 30)], 40.0);
        assert_eq!(m.get(25, 35), 255, "spreads through the red");
        assert_eq!(m.get(40, 20), 0, "does not cross into the blue");
    }

    #[test]
    fn live_wire_follows_an_edge() {
        // A vertical edge at x = 20; a straight line would cut the corner.
        let (w, h) = (40u32, 40u32);
        let img: Vec<u8> = (0..w * h)
            .flat_map(|i| {
                if i % w < 20 {
                    [0, 0, 0, 255]
                } else {
                    [255, 255, 255, 255]
                }
            })
            .collect();
        let e = edges(&img, w, h);
        let path = live_wire(&e, w, h, (19, 2), (19, 37), 10);
        assert_eq!(path.first(), Some(&(19, 2)));
        assert_eq!(path.last(), Some(&(19, 37)));
        assert!(path.iter().all(|(x, _)| (18..=21).contains(x)), "{path:?}");
    }
}
