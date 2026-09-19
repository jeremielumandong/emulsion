//! Perspective warps (Distort): map a plane's rectangle onto any convex
//! quadrilateral in document space.

use crate::geom::IRect;
use crate::image::{Pix, Plane};
use rayon::prelude::*;

/// A 3×3 projective matrix, row-major, h[8] = 1.
pub type Homography = [f64; 9];

/// The homography taking `src[i]` to `dst[i]` for four point pairs. None
/// when the points are degenerate (three in a line).
pub fn homography(src: [(f64, f64); 4], dst: [(f64, f64); 4]) -> Option<Homography> {
    // Solve A·h = b for the eight unknowns.
    let mut a = [[0.0f64; 9]; 8];
    for i in 0..4 {
        let ((x, y), (u, v)) = (src[i], dst[i]);
        a[2 * i] = [x, y, 1.0, 0.0, 0.0, 0.0, -u * x, -u * y, u];
        a[2 * i + 1] = [0.0, 0.0, 0.0, x, y, 1.0, -v * x, -v * y, v];
    }
    for col in 0..8 {
        let pivot = (col..8).max_by(|&r, &s| a[r][col].abs().total_cmp(&a[s][col].abs()))?;
        if a[pivot][col].abs() < 1e-12 {
            return None;
        }
        a.swap(col, pivot);
        let prow = a[col];
        for (r, row) in a.iter_mut().enumerate() {
            if r != col {
                let f = row[col] / prow[col];
                for (v, p) in row.iter_mut().zip(prow).skip(col) {
                    *v -= f * p;
                }
            }
        }
    }
    let mut h = [0.0; 9];
    for i in 0..8 {
        h[i] = a[i][8] / a[i][i];
    }
    h[8] = 1.0;
    h.iter().all(|v| v.is_finite()).then_some(h)
}

pub fn apply(h: &Homography, (x, y): (f64, f64)) -> (f64, f64) {
    let w = h[6] * x + h[7] * y + h[8];
    (
        (h[0] * x + h[1] * y + h[2]) / w,
        (h[3] * x + h[4] * y + h[5]) / w,
    )
}

pub fn invert(h: &Homography) -> Option<Homography> {
    let [a, b, c, d, e, f, g, hh, i] = *h;
    let det = a * (e * i - f * hh) - b * (d * i - f * g) + c * (d * hh - e * g);
    if det.abs() < 1e-15 {
        return None;
    }
    let m = [
        (e * i - f * hh) / det,
        (c * hh - b * i) / det,
        (b * f - c * e) / det,
        (f * g - d * i) / det,
        (a * i - c * g) / det,
        (c * d - a * f) / det,
        (d * hh - e * g) / det,
        (b * g - a * hh) / det,
        (a * e - b * d) / det,
    ];
    Some(m.map(|v| v / m[8]))
}

/// Bilinear blend of four samples.
pub trait Blend4: Pix {
    fn blend4(p: [Self; 4], fx: f32, fy: f32) -> Self;
}

impl Blend4 for [u16; 4] {
    fn blend4(p: [Self; 4], fx: f32, fy: f32) -> Self {
        let w = [
            (1.0 - fx) * (1.0 - fy),
            fx * (1.0 - fy),
            (1.0 - fx) * fy,
            fx * fy,
        ];
        [0, 1, 2, 3].map(|c| {
            (0..4)
                .map(|k| p[k][c] as f32 * w[k])
                .sum::<f32>()
                .round()
                .clamp(0.0, 65535.0) as u16
        })
    }
}

impl Blend4 for u8 {
    fn blend4(p: [Self; 4], fx: f32, fy: f32) -> Self {
        let w = [
            (1.0 - fx) * (1.0 - fy),
            fx * (1.0 - fy),
            (1.0 - fx) * fy,
            fx * fy,
        ];
        (0..4)
            .map(|k| p[k] as f32 * w[k])
            .sum::<f32>()
            .round()
            .clamp(0.0, 255.0) as u8
    }
}

/// Warp `src` so its corners (TL, TR, BR, BL) land on `quad` in document
/// space. Returns the new plane and where its top-left sits. Pixels outside
/// the source come out as `transparent`. None when the quad is degenerate
/// or its bounds are too large.
pub fn warp<P: Blend4>(
    src: &Plane<P>,
    quad: [(f64, f64); 4],
    transparent: P,
) -> Option<(Plane<P>, IRect)> {
    let (w, h) = (src.width() as f64, src.height() as f64);
    let h_fwd = homography([(0.0, 0.0), (w, 0.0), (w, h), (0.0, h)], quad)?;
    let inv = invert(&h_fwd)?;
    let (mut x0, mut y0, mut x1, mut y1) = (f64::MAX, f64::MAX, f64::MIN, f64::MIN);
    for (x, y) in quad {
        x0 = x0.min(x);
        y0 = y0.min(y);
        x1 = x1.max(x);
        y1 = y1.max(y);
    }
    let bounds = IRect::new(
        x0.floor() as i32,
        y0.floor() as i32,
        (x1.ceil() - x0.floor()).max(1.0) as i32,
        (y1.ceil() - y0.floor()).max(1.0) as i32,
    );
    if bounds.w > 30_000 || bounds.h > 30_000 || bounds.w as u64 * bounds.h as u64 > 400_000_000 {
        return None;
    }
    let dense = src.read_rect(src.bounds());
    let (sw, sh) = (src.width() as i64, src.height() as i64);
    let at = |x: i64, y: i64| -> P {
        if x < 0 || y < 0 || x >= sw || y >= sh {
            transparent
        } else {
            dense[(y * sw + x) as usize]
        }
    };
    let rows: Vec<Vec<P>> = (0..bounds.h)
        .into_par_iter()
        .map(|row| {
            (0..bounds.w)
                .map(|col| {
                    let (u, v) = apply(
                        &inv,
                        ((bounds.x + col) as f64 + 0.5, (bounds.y + row) as f64 + 0.5),
                    );
                    let (fx, fy) = (u - 0.5, v - 0.5);
                    if !(fx > -1.0 && fy > -1.0 && fx < w && fy < h) {
                        return transparent;
                    }
                    let (ix, iy) = (fx.floor() as i64, fy.floor() as i64);
                    let p = [
                        at(ix, iy),
                        at(ix + 1, iy),
                        at(ix, iy + 1),
                        at(ix + 1, iy + 1),
                    ];
                    P::blend4(p, (fx - ix as f64) as f32, (fy - iy as f64) as f32)
                })
                .collect()
        })
        .collect();
    let flat: Vec<P> = rows.into_iter().flatten().collect();
    Some((
        Plane::from_pixels(bounds.w as u32, bounds.h as u32, transparent, &flat),
        bounds,
    ))
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::image::Raster;

    #[test]
    fn homography_round_trips_the_corners() {
        let src = [(0.0, 0.0), (10.0, 0.0), (10.0, 10.0), (0.0, 10.0)];
        let dst = [(5.0, 5.0), (40.0, 0.0), (35.0, 30.0), (0.0, 20.0)];
        let h = homography(src, dst).unwrap();
        for (s, d) in src.iter().zip(&dst) {
            let p = apply(&h, *s);
            assert!((p.0 - d.0).abs() < 1e-9 && (p.1 - d.1).abs() < 1e-9);
        }
        let back = invert(&h).unwrap();
        let p = apply(&back, dst[2]);
        assert!((p.0 - 10.0).abs() < 1e-9 && (p.1 - 10.0).abs() < 1e-9);
    }

    #[test]
    fn identity_quad_keeps_pixels() {
        let r = Raster::solid(20, 10, [0.5, 0.25, 0.125, 1.0]);
        let (out, b) = warp(
            &r,
            [(0.0, 0.0), (20.0, 0.0), (20.0, 10.0), (0.0, 10.0)],
            [0; 4],
        )
        .unwrap();
        assert_eq!(b, IRect::new(0, 0, 20, 10));
        assert_eq!(out.get(10, 5), r.get(10, 5));
    }

    #[test]
    fn a_trapezoid_is_transparent_outside() {
        let r = Raster::solid(20, 20, [1.0, 1.0, 1.0, 1.0]);
        let (out, b) = warp(
            &r,
            [(5.0, 0.0), (15.0, 0.0), (20.0, 20.0), (0.0, 20.0)],
            [0; 4],
        )
        .unwrap();
        assert_eq!((b.w, b.h), (20, 20));
        assert_eq!(out.get(0, 0)[3], 0, "outside the top edge");
        assert!(out.get(10, 10)[3] > 60000, "inside");
    }
}
