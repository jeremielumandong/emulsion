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

fn box_blur_1d(src: &[f32], dst: &mut [f32], n: usize, stride: usize, count: usize, r: usize) {
    for line in 0..count {
        let base = line * if stride == 1 { n } else { 1 };
        let at = |i: usize| src[base + i * stride];
        let mut sum = 0.0;
        for i in 0..=r.min(n - 1) {
            sum += at(i);
        }
        sum += at(0) * r as f32;
        for i in 0..n {
            dst[base + i * stride] = sum / (2 * r + 1) as f32;
            let add = at((i + r + 1).min(n - 1));
            let sub = at(i.saturating_sub(r));
            sum += add - sub;
        }
    }
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
    for _ in 0..3 {
        box_blur_1d(&a, &mut b, w, 1, h, r);
        box_blur_1d(&b, &mut a, h, w, w, r);
    }
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
}
