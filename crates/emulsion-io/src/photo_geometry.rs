//! Line-based automatic leveling and keystone correction. No model or scene
//! guessing: reject low-structure images and fit the actual development warp.
use crate::{IoError, Result};
use emulsion_core::raw::DevelopParams;
use emulsion_raster::Raster;
#[derive(Clone, Copy)]
struct Line {
    a: [f32; 2],
    b: [f32; 2],
    vertical: bool,
    weight: f32,
}
pub fn automatic(image: &Raster, params: DevelopParams) -> Result<DevelopParams> {
    let rgba = image::RgbaImage::from_raw(image.width(), image.height(), image.to_srgba8())
        .ok_or_else(|| IoError::Manifest("Invalid perspective input".into()))?;
    let small = image::DynamicImage::ImageRgba8(rgba)
        .resize(400, 400, image::imageops::FilterType::Triangle)
        .to_luma8();
    let (w, h) = small.dimensions();
    if w < 16 || h < 16 {
        return Err(IoError::Manifest(
            "Image is too small to detect straight lines".into(),
        ));
    }
    let mut edges = vec![];
    for y in 1..h - 1 {
        for x in 1..w - 1 {
            let gx = small.get_pixel(x + 1, y)[0] as f32 - small.get_pixel(x - 1, y)[0] as f32;
            let gy = small.get_pixel(x, y + 1)[0] as f32 - small.get_pixel(x, y - 1)[0] as f32;
            let strength = gx.hypot(gy);
            if strength > 40. {
                edges.push((x as f32, y as f32, strength));
            }
        }
    }
    edges.sort_by(|a, b| b.2.total_cmp(&a.2));
    edges.truncate(12000);
    let radius = ((w * w + h * h) as f32).sqrt().ceil() as usize;
    let stride = radius * 2 + 1;
    let mut peaks = vec![];
    for vertical in [true, false] {
        for degree in -20..=20 {
            let angle = (degree as f32 + if vertical { 0. } else { 90. }).to_radians();
            let (c, s) = (angle.cos(), angle.sin());
            let mut votes = vec![0u16; stride];
            for &(x, y, _) in &edges {
                let rho = (x * c + y * s).round() as isize + radius as isize;
                if rho >= 0 && (rho as usize) < stride {
                    votes[rho as usize] += 1;
                }
            }
            for (rho, &n) in votes.iter().enumerate() {
                if n >= w.min(h) as u16 / 5 {
                    peaks.push((n, degree, vertical, rho as f32 - radius as f32));
                }
            }
        }
    }
    peaks.sort_by_key(|a| std::cmp::Reverse(a.0));
    let mut lines: Vec<Line> = vec![];
    let mut selected: Vec<(i32, bool, f32)> = vec![];
    let scale = w.min(h) as f32 / 2.;
    for (votes, degree, vertical, rho) in peaks {
        if selected
            .iter()
            .any(|&(d, v, r)| v == vertical && (d - degree).abs() < 4 && (r - rho).abs() < 12.)
        {
            continue;
        }
        let angle = (degree as f32 + if vertical { 0. } else { 90. }).to_radians();
        let (c, s) = (angle.cos(), angle.sin());
        let points: Vec<_> = edges
            .iter()
            .filter(|&&(x, y, _)| (x * c + y * s - rho).abs() < 1.5)
            .collect();
        if points.len() < 10 {
            continue;
        }
        let project = |p: &&(f32, f32, f32)| -p.0 * s + p.1 * c;
        let a = points
            .iter()
            .min_by(|a, b| project(a).total_cmp(&project(b)))
            .unwrap();
        let b = points
            .iter()
            .max_by(|a, b| project(a).total_cmp(&project(b)))
            .unwrap();
        if (project(a) - project(b)).abs() < w.min(h) as f32 * 0.25 {
            continue;
        }
        let normalize =
            |p: &&(f32, f32, f32)| [(p.0 - w as f32 / 2.) / scale, (p.1 - h as f32 / 2.) / scale];
        lines.push(Line {
            a: normalize(a),
            b: normalize(b),
            vertical,
            weight: votes as f32,
        });
        selected.push((degree, vertical, rho));
        if lines.len() == 10 {
            break;
        }
    }
    if lines.len() < 2 {
        return Err(IoError::Manifest(
            "Not enough strong straight lines for automatic perspective; use manual controls"
                .into(),
        ));
    }
    fit_lines(&lines, params)
}

/// Fit two to eight guides in untransformed, normalized source coordinates.
/// Each guide is classified against its nearest horizontal or vertical axis.
pub fn guided(
    guides: &[[[f32; 2]; 2]],
    dimensions: (u32, u32),
    params: DevelopParams,
) -> Result<DevelopParams> {
    if !(2..=8).contains(&guides.len()) || dimensions.0 == 0 || dimensions.1 == 0 {
        return Err(IoError::Manifest(
            "Draw two to eight perspective guides".into(),
        ));
    }
    let (w, h) = (dimensions.0 as f32, dimensions.1 as f32);
    let scale = w.min(h) * 0.5;
    let mut lines = Vec::with_capacity(guides.len());
    for [a, b] in guides {
        if a.iter()
            .chain(b)
            .any(|v| !v.is_finite() || !(0.0..=1.0).contains(v))
        {
            return Err(IoError::Manifest(
                "Perspective guides must lie inside the source".into(),
            ));
        }
        let normalize = |p: [f32; 2]| [(p[0] - 0.5) * w / scale, (p[1] - 0.5) * h / scale];
        let (a, b) = (normalize(*a), normalize(*b));
        let (dx, dy) = (b[0] - a[0], b[1] - a[1]);
        if dx.hypot(dy) < 0.1 {
            return Err(IoError::Manifest(
                "Perspective guides must span at least 5% of the short image side".into(),
            ));
        }
        lines.push(Line {
            a,
            b,
            vertical: dy.abs() > dx.abs(),
            weight: 1.,
        });
    }
    fit_lines(&lines, params)
}

fn fit_lines(lines: &[Line], mut params: DevelopParams) -> Result<DevelopParams> {
    let vertical = lines.iter().filter(|l| l.vertical).count();
    let horizontal = lines.len() - vertical;
    let score = |p: [f32; 3]| {
        let (s, c) = p[0].to_radians().sin_cos();
        let transform = |[x, y]: [f32; 2]| {
            let (rx, ry) = (c * x - s * y, s * x + c * y);
            let d = 1. - p[1] * rx - p[2] * ry;
            if d < 0.2 {
                return None;
            }
            Some([rx / d, ry / d])
        };
        let mut error = 0.;
        let mut weight = 0.;
        for l in lines {
            let (Some(a), Some(b)) = (transform(l.a), transform(l.b)) else {
                return f32::INFINITY;
            };
            let (dx, dy) = (a[0] - b[0], a[1] - b[1]);
            let residual = if l.vertical { dx } else { dy };
            error += (residual * residual / (dx * dx + dy * dy).max(1e-6)).min(0.1) * l.weight;
            weight += l.weight;
        }
        error / weight + 0.0001 * (p[1] * p[1] + p[2] * p[2])
    };
    let mut fit = [0.; 3];
    let mut best = score(fit);
    for refinement in 0..7 {
        let steps = [5., 0.1, 0.1].map(|v| v / 2f32.powi(refinement));
        for _ in 0..12 {
            let before = fit;
            for axis in 0..3 {
                if axis == 1 && horizontal < 2 || axis == 2 && vertical < 2 {
                    continue;
                }
                for direction in [-1., 1.] {
                    let mut candidate = fit;
                    candidate[axis] += steps[axis] * direction;
                    let limit = if axis == 0 { 20. } else { 0.5 };
                    if candidate[axis].abs() > limit {
                        continue;
                    }
                    let cost = score(candidate);
                    if cost < best {
                        best = cost;
                        fit = candidate;
                    }
                }
            }
            if before == fit {
                break;
            }
        }
    }
    params.straighten = fit[0];
    params.perspective = [fit[1], fit[2]];
    params.validate().map_err(|s| IoError::Manifest(s.into()))?;
    Ok(params)
}

#[cfg(test)]
mod guided_tests {
    use super::*;

    #[test]
    fn recovers_rotation_and_two_axis_perspective() {
        let angle = 6f32.to_radians();
        let (s, c) = angle.sin_cos();
        let inverse = |[x, y]: [f32; 2]| {
            let denominator = 1. + 0.12 * x - 0.10 * y;
            [
                0.5 + (c * x + s * y) / denominator / 2.,
                0.5 + (-s * x + c * y) / denominator / 2.,
            ]
        };
        let guides = [
            [[-0.5, -0.5], [-0.5, 0.5]],
            [[0.5, -0.5], [0.5, 0.5]],
            [[-0.5, -0.5], [0.5, -0.5]],
            [[-0.5, 0.5], [0.5, 0.5]],
        ]
        .map(|line| line.map(inverse));
        let params = DevelopParams {
            exposure: 1.,
            rotation: 3,
            ..Default::default()
        };
        let fitted = guided(&guides, (1000, 1000), params).unwrap();
        assert!((fitted.straighten - 6.).abs() < 0.2, "{fitted:?}");
        assert!((fitted.perspective[0] - 0.12).abs() < 0.02);
        assert!((fitted.perspective[1] + 0.10).abs() < 0.02);
        assert_eq!(fitted.exposure, params.exposure);
        assert_eq!(fitted.rotation, params.rotation);
    }

    #[test]
    fn rejects_invalid_and_degenerate_guides() {
        let params = DevelopParams::default();
        assert!(guided(&[], (100, 100), params).is_err());
        assert!(guided(&[[[0.5; 2]; 2]; 2], (100, 100), params).is_err());
        assert!(guided(&[[[f32::NAN; 2]; 2]; 2], (100, 100), params).is_err());
    }
}
