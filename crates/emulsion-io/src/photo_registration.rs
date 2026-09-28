//! Bounded feature registration shared by HDR and planar panoramas.
use crate::{IoError, Result, photo_hdr::FloatImage};
use std::sync::atomic::{AtomicBool, Ordering};
pub const IDENTITY: [f64; 9] = [1., 0., 0., 0., 1., 0., 0., 0., 1.];
pub fn project(m: &[f64; 9], x: f64, y: f64) -> Option<[f64; 2]> {
    let d = m[6] * x + m[7] * y + m[8];
    if !d.is_finite() || d.abs() < 1e-8 {
        return None;
    }
    let p = [
        (m[0] * x + m[1] * y + m[2]) / d,
        (m[3] * x + m[4] * y + m[5]) / d,
    ];
    p.iter().all(|v| v.is_finite()).then_some(p)
}
fn bad(s: &str) -> IoError {
    IoError::Unsupported(s.into())
}
#[derive(Clone)]
struct Feature {
    p: [f64; 2],
    descriptor: [f32; 49],
}
fn features(image: &FloatImage, cancel: &AtomicBool) -> Result<Vec<Feature>> {
    let small = image.resized(640);
    let (w, h) = (small.width as usize, small.height as usize);
    if w < 20 || h < 20 {
        return Err(bad("Image is too small for registration"));
    }
    let gray: Vec<_> = small
        .pixels
        .iter()
        .map(|p| {
            (0.2126 * p[0] + 0.7152 * p[1] + 0.0722 * p[2])
                .max(1e-5)
                .ln()
        })
        .collect();
    let mut candidates = Vec::new();
    for y in (8..h - 8).step_by(2) {
        if cancel.load(Ordering::Relaxed) {
            return Err(bad("Registration cancelled"));
        }
        for x in (8..w - 8).step_by(2) {
            let (mut xx, mut yy, mut xy) = (0., 0., 0.);
            for dy in -1isize..=1 {
                for dx in -1isize..=1 {
                    let i = (y as isize + dy) as usize * w + (x as isize + dx) as usize;
                    let gx = gray[i + 1] - gray[i - 1];
                    let gy = gray[i + w] - gray[i - w];
                    xx += gx * gx;
                    yy += gy * gy;
                    xy += gx * gy;
                }
            }
            let score = (xx * yy - xy * xy) - 0.05 * (xx + yy).powi(2);
            if score > 0.0001 {
                candidates.push((score, x, y));
            }
        }
    }
    candidates.sort_unstable_by(|a, b| b.0.total_cmp(&a.0));
    let mut chosen: Vec<(usize, usize)> = Vec::new();
    let mut out = Vec::new();
    for (_, x, y) in candidates {
        if chosen
            .iter()
            .any(|&(a, b)| a.abs_diff(x) < 8 && b.abs_diff(y) < 8)
        {
            continue;
        }
        let mut descriptor = [0.; 49];
        for j in 0..7 {
            for i in 0..7 {
                descriptor[j * 7 + i] = gray[(y + j * 2 - 6) * w + x + i * 2 - 6];
            }
        }
        let mean = descriptor.iter().sum::<f32>() / 49.;
        let norm = descriptor
            .iter()
            .map(|v| (v - mean).powi(2))
            .sum::<f32>()
            .sqrt();
        if norm < 0.01 {
            continue;
        }
        for v in &mut descriptor {
            *v = (*v - mean) / norm;
        }
        out.push(Feature {
            p: [x as f64 / w as f64, y as f64 / h as f64],
            descriptor,
        });
        chosen.push((x, y));
        if out.len() == 500 {
            break;
        }
    }
    Ok(out)
}
fn solve(mut a: [[f64; 9]; 8]) -> Option<[f64; 9]> {
    for c in 0..8 {
        let pivot = (c..8).max_by(|&x, &y| a[x][c].abs().total_cmp(&a[y][c].abs()))?;
        a.swap(c, pivot);
        let div = a[c][c];
        if div.abs() < 1e-12 {
            return None;
        }
        for value in &mut a[c][c..] {
            *value /= div;
        }
        let pivot_row = a[c];
        for (r, row) in a.iter_mut().enumerate() {
            if r != c {
                let k = row[c];
                for (value, pivot) in row[c..].iter_mut().zip(&pivot_row[c..]) {
                    *value -= k * pivot;
                }
            }
        }
    }
    Some([
        a[0][8], a[1][8], a[2][8], a[3][8], a[4][8], a[5][8], a[6][8], a[7][8], 1.,
    ])
}
fn fit(pairs: &[([f64; 2], [f64; 2])]) -> Option<[f64; 9]> {
    let mut normal = [[0.; 9]; 8];
    for &([x, y], [u, v]) in pairs {
        for row in [
            [x, y, 1., 0., 0., 0., -u * x, -u * y, u],
            [0., 0., 0., x, y, 1., -v * x, -v * y, v],
        ] {
            for i in 0..8 {
                for j in 0..9 {
                    normal[i][j] += row[i] * row[j];
                }
            }
        }
    }
    solve(normal)
}
pub fn register(
    reference: &FloatImage,
    other: &FloatImage,
    cancel: &AtomicBool,
) -> Result<[f64; 9]> {
    let a = features(reference, cancel)?;
    let b = features(other, cancel)?;
    let distance = |a: &Feature, b: &Feature| {
        a.descriptor
            .iter()
            .zip(b.descriptor)
            .map(|(x, y)| (x - y).powi(2))
            .sum::<f32>()
    };
    let mut pairs = Vec::new();
    for fa in &a {
        let mut best = (f32::INFINITY, 0);
        let mut second = f32::INFINITY;
        for (j, fb) in b.iter().enumerate() {
            let d = distance(fa, fb);
            if d < best.0 {
                second = best.0;
                best = (d, j);
            } else if d < second {
                second = d;
            }
        }
        if best.0 < second * 0.75 && best.0 < 0.8 {
            let fb = &b[best.1];
            if a.iter().all(|other| distance(other, fb) >= best.0 - 1e-6) {
                pairs.push((fa.p, fb.p));
            }
        }
    }
    if pairs.len() < 8 {
        return Err(bad(
            "Not enough matching detail; use overlapping images with textured edges",
        ));
    }
    let mut best = Vec::new();
    let mut rng = 0x7352abcd1234u64;
    for _ in 0..1600 {
        if cancel.load(Ordering::Relaxed) {
            return Err(bad("Registration cancelled"));
        }
        let mut sample = Vec::new();
        let mut indices = Vec::new();
        while sample.len() < 4 {
            rng ^= rng << 13;
            rng ^= rng >> 7;
            rng ^= rng << 17;
            let i = rng as usize % pairs.len();
            if !indices.contains(&i) {
                indices.push(i);
                sample.push(pairs[i]);
            }
        }
        let Some(matrix) = fit(&sample) else {
            continue;
        };
        let inliers: Vec<_> = pairs
            .iter()
            .copied()
            .filter(|&(p, q)| {
                project(&matrix, p[0], p[1])
                    .is_some_and(|r| (r[0] - q[0]).hypot(r[1] - q[1]) < 0.008)
            })
            .collect();
        if inliers.len() > best.len() {
            best = inliers;
        }
    }
    if best.len() < 8 {
        return Err(bad("Could not establish a stable geometric alignment"));
    }
    fit(&best).ok_or_else(|| bad("Degenerate image alignment"))
}

#[cfg(test)]
mod tests {
    use super::*;
    #[test]
    fn registers_overlapping_photographic_texture_and_cancels() {
        let w = 256u32;
        let h = 192u32;
        let scene = |x: u32, y: u32| {
            let mut v = ((x / 5) as u64 * 73856093) ^ ((y / 5) as u64 * 19349663);
            v ^= v >> 13;
            (v % 1000) as f32 / 1200. + 0.05
        };
        let image = |shift: u32| FloatImage {
            width: w,
            height: h,
            pixels: (0..h)
                .flat_map(|y| (0..w).map(move |x| [scene(x + shift, y); 3]))
                .collect(),
        };
        let a = image(0);
        let b = image(48);
        let m = register(&a, &b, &AtomicBool::new(false)).unwrap();
        let q = project(&m, 0.5, 0.5).unwrap();
        assert!((q[0] - (0.5 - 48. / 255.)).abs() < 0.015, "{m:?}");
        assert!((q[1] - 0.5).abs() < 0.01);
        assert!(register(&a, &b, &AtomicBool::new(true)).is_err());
    }
    #[test]
    fn fits_rotation_and_perspective_and_rejects_degenerate_points() {
        let expected = [0.99, -0.08, 0.12, 0.08, 1.01, -0.05, 0.04, -0.03, 1.];
        let pairs: Vec<_> = (0..5)
            .flat_map(|y| (0..5).map(move |x| [x as f64 / 4., y as f64 / 4.]))
            .map(|p| (p, project(&expected, p[0], p[1]).unwrap()))
            .collect();
        let result = fit(&pairs).unwrap();
        for (a, b) in result.iter().zip(expected) {
            assert!((a - b).abs() < 1e-7);
        }
        assert!(fit(&[([0.; 2], [0.; 2]); 4]).is_none());
    }
}
