//! Radius-independent layer-effect kernels. Large circular morphology uses an
//! octagonal approximation; partial alpha is preserved without thresholding.
use rayon::prelude::*;
use std::collections::VecDeque;

pub(super) fn blur(a: &[f32], w: usize, h: usize, radius: f32) -> Vec<f32> {
    if radius < 0.3 || w == 0 || h == 0 {
        return a.to_vec();
    }
    let sigma = (radius / 2.).max(0.3);
    if sigma < 2. {
        return exact_blur(a, w, h, sigma);
    }
    let ideal = (4. * sigma * sigma + 1.).sqrt();
    let mut lower = ideal.floor() as usize;
    if lower.is_multiple_of(2) {
        lower = lower.saturating_sub(1)
    }
    lower = lower.max(1);
    let lf = lower as f32;
    let count = ((12. * sigma * sigma - 3. * lf * lf - 12. * lf - 9.) / (-4. * lf - 4.))
        .round()
        .clamp(0., 3.) as usize;
    let mut src = a.to_vec();
    let mut tmp = vec![0.; a.len()];
    for pass in 0..3 {
        let width = if pass < count { lower } else { lower + 2 };
        let r = width / 2;
        if r == 0 {
            continue;
        }
        box_horizontal(&src, &mut tmp, w, r);
        box_vertical(&tmp, &mut src, w, h, r);
    }
    src
}
fn box_horizontal(src: &[f32], out: &mut [f32], w: usize, r: usize) {
    let divisor = (2 * r + 1) as f64;
    out.par_chunks_mut(w)
        .zip(src.par_chunks(w))
        .for_each(|(dst, row)| {
            let mut sum: f64 = row[..(r + 1).min(w)].iter().map(|v| *v as f64).sum();
            for x in 0..w {
                dst[x] = (sum / divisor) as f32;
                if x >= r {
                    sum -= row[x - r] as f64
                }
                if x + r + 1 < w {
                    sum += row[x + r + 1] as f64
                }
            }
        });
}
fn box_vertical(src: &[f32], out: &mut [f32], w: usize, h: usize, r: usize) {
    let divisor = (2 * r + 1) as f64;
    let mut sums = vec![0f64; w];
    for row in src.chunks(w).take((r + 1).min(h)) {
        for (x, v) in row.iter().enumerate() {
            sums[x] += *v as f64
        }
    }
    for y in 0..h {
        for x in 0..w {
            out[y * w + x] = (sums[x] / divisor) as f32;
        }
        if y >= r {
            for x in 0..w {
                sums[x] -= src[(y - r) * w + x] as f64
            }
        }
        if y + r + 1 < h {
            for x in 0..w {
                sums[x] += src[(y + r + 1) * w + x] as f64
            }
        }
    }
}
fn exact_blur(a: &[f32], w: usize, h: usize, sigma: f32) -> Vec<f32> {
    let r = (sigma * 3.).ceil() as isize;
    let mut kernel: Vec<_> = (-r..=r)
        .map(|i| (-(i * i) as f32 / (2. * sigma * sigma)).exp())
        .collect();
    let sum: f32 = kernel.iter().sum();
    kernel.iter_mut().for_each(|v| *v /= sum);
    let mut tmp = vec![0.; a.len()];
    let mut out = vec![0.0f32; a.len()];
    tmp.par_chunks_mut(w).enumerate().for_each(|(y, row)| {
        for (x, v) in row.iter_mut().enumerate() {
            for (k, weight) in kernel.iter().enumerate() {
                let sx = x as isize + k as isize - r;
                if sx >= 0 && (sx as usize) < w {
                    *v += a[y * w + sx as usize] * weight
                }
            }
        }
    });
    out.par_chunks_mut(w).enumerate().for_each(|(y, row)| {
        for (x, v) in row.iter_mut().enumerate() {
            for (k, weight) in kernel.iter().enumerate() {
                let sy = y as isize + k as isize - r;
                if sy >= 0 && (sy as usize) < h {
                    *v += tmp[sy as usize * w + x] * weight
                }
            }
        }
    });
    out
}

pub(super) fn dilate(a: &[f32], w: usize, h: usize, radius: f32) -> Vec<f32> {
    if radius <= 0. || w == 0 || h == 0 {
        return a.to_vec();
    }
    if radius <= 2. {
        return exact_disk(a, w, h, radius);
    }
    // Minkowski sum of horizontal, vertical and both diagonal segments.
    // Axis and 45-degree support match a circle; intermediate angles differ
    // by at most 8.3% before integer rounding.
    let diagonal = (radius * (1. - std::f32::consts::FRAC_1_SQRT_2)).round() as usize;
    let axial = (radius - 2. * diagonal as f32).round().max(0.) as usize;
    let mut src = a.to_vec();
    let mut dst = vec![0.; a.len()];
    for (dx, dy, r) in [
        (1, 0, axial),
        (0, 1, axial),
        (1, 1, diagonal),
        (-1, 1, diagonal),
    ] {
        if r == 0 {
            continue;
        }
        max_direction(&src, &mut dst, w, h, dx, dy, r);
        std::mem::swap(&mut src, &mut dst);
    }
    src
}
fn exact_disk(a: &[f32], w: usize, h: usize, radius: f32) -> Vec<f32> {
    let r = radius.ceil() as isize;
    let offsets: Vec<_> = (-r..=r)
        .flat_map(|dy| (-r..=r).map(move |dx| (dx, dy)))
        .filter(|(x, y)| ((x * x + y * y) as f32).sqrt() <= radius + 0.5)
        .collect();
    let mut out = vec![0.0f32; a.len()];
    out.par_chunks_mut(w).enumerate().for_each(|(y, row)| {
        for (x, v) in row.iter_mut().enumerate() {
            for (dx, dy) in &offsets {
                let sx = x as isize + dx;
                let sy = y as isize + dy;
                if sx >= 0 && sy >= 0 && (sx as usize) < w && (sy as usize) < h {
                    *v = (*v).max(a[sy as usize * w + sx as usize]);
                }
            }
        }
    });
    out
}
fn max_direction(src: &[f32], out: &mut [f32], w: usize, h: usize, dx: isize, dy: isize, r: usize) {
    let mut starts = Vec::with_capacity(w + h);
    if dy == 0 {
        starts.extend((0..h).map(|y| (0, y)))
    } else if dx == 0 {
        starts.extend((0..w).map(|x| (x, 0)))
    } else {
        starts.extend((0..w).map(|x| (x, 0)));
        let x = if dx > 0 { 0 } else { w - 1 };
        starts.extend((1..h).map(|y| (x, y)));
    }
    let mut queue: VecDeque<(usize, f32)> = VecDeque::new();
    for (x, y) in starts {
        let len = if dy == 0 {
            w
        } else if dx == 0 {
            h
        } else if dx > 0 {
            (w - x).min(h - y)
        } else {
            (x + 1).min(h - y)
        };
        let index = |i: usize| {
            ((y as isize + dy * i as isize) as usize) * w + (x as isize + dx * i as isize) as usize
        };
        queue.clear();
        let mut next = 0;
        for center in 0..len {
            let right = (center + r + 1).min(len);
            while next < right {
                let v = src[index(next)];
                while queue.back().is_some_and(|(_, old)| *old <= v) {
                    queue.pop_back();
                }
                queue.push_back((next, v));
                next += 1;
            }
            let left = center.saturating_sub(r);
            while queue.front().is_some_and(|(i, _)| *i < left) {
                queue.pop_front();
            }
            out[index(center)] = queue.front().map_or(0., |(_, v)| *v);
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    #[test]
    fn tiny_disk_matches_reference_with_partial_coverage() {
        let (w, h) = (9, 7);
        let a: Vec<_> = (0..w * h).map(|i| ((i * 17) % 31) as f32 / 31.).collect();
        for r in [0., 0.5, 1., 1.5, 2.] {
            let out = dilate(&a, w, h, r);
            for y in 0..h {
                for x in 0..w {
                    let mut expected = 0f32;
                    for sy in 0..h {
                        for sx in 0..w {
                            let distance = ((sx as f32 - x as f32).powi(2)
                                + (sy as f32 - y as f32).powi(2))
                            .sqrt();
                            if distance <= r + 0.5 {
                                expected = expected.max(a[sy * w + sx]);
                            }
                        }
                    }
                    assert_eq!(out[y * w + x], expected, "r={r} ({x},{y})");
                }
            }
        }
    }
    #[test]
    fn octagon_is_symmetric_rounded_and_keeps_alpha() {
        let w = 65;
        let mut a = vec![0.; w * w];
        a[32 * w + 32] = 0.37;
        let out = dilate(&a, w, w, 16.);
        assert_eq!(out[32 * w + 48], 0.37);
        assert_eq!(out[48 * w + 48], 0.);
        for y in 0..w {
            for x in 0..w {
                let v = out[y * w + x];
                assert!(v == 0. || v == 0.37);
                assert_eq!(v, out[x * w + y]);
                assert_eq!(v, out[(64 - y) * w + 64 - x]);
                let d = ((x as f32 - 32.).powi(2) + (y as f32 - 32.).powi(2)).sqrt();
                if d <= 14. {
                    assert_eq!(v, 0.37)
                }
                if d >= 19. {
                    assert_eq!(v, 0.)
                }
            }
        }
    }
    #[test]
    fn box_gaussian_is_symmetric_mass_preserving_and_close_to_gaussian() {
        let w = 129;
        let mut a = vec![0.; w * w];
        a[64 * w + 64] = 1.;
        let out = blur(&a, w, w, 12.);
        assert!((out.iter().sum::<f32>() - 1.).abs() < 0.0001);
        for y in 0..w {
            for x in 0..w {
                assert!((out[y * w + x] - out[x * w + y]).abs() < 1e-7);
            }
        }
        let sigma = 6.;
        let reference = 1. / (2. * std::f32::consts::PI * sigma * sigma);
        assert!((out[64 * w + 64] - reference).abs() / reference < 0.15);
        let edge = blur(&vec![1.; 9 * 9], 9, 9, 12.);
        assert!(edge[0] < edge[4 * 9 + 4]);
        assert!(edge.iter().all(|v| *v >= 0. && *v <= 1.));
    }
    #[test]
    fn large_radius_wide_image_is_finite() {
        let (w, h) = (4096, 64);
        let a = vec![0.42; w * h];
        let b = blur(&a, w, h, 200.);
        let d = dilate(&a, w, h, 200.);
        assert!(b.iter().all(|v| v.is_finite() && *v >= 0. && *v <= 0.42));
        assert!(d.iter().all(|v| *v == 0.42));
    }
    #[test]
    #[ignore = "manual 4096x4096 kernel timing; no machine-dependent threshold"]
    fn large_image_timings() {
        let (w, h) = (4096, 4096);
        let a = vec![0.42; w * h];
        let start = std::time::Instant::now();
        let b = blur(&a, w, h, 200.);
        eprintln!("4096x4096 blur radius200: {:?}", start.elapsed());
        let start = std::time::Instant::now();
        let d = dilate(&a, w, h, 200.);
        eprintln!(
            "4096x4096 octagonal dilation radius200: {:?}",
            start.elapsed()
        );
        assert_eq!(d[w * h / 2], 0.42);
        assert!(b[w * h / 2].is_finite());
    }
}
