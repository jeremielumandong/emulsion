//! Shared color and geometric development, always in linear working pixels.
use super::*;

fn hsv(rgb: [f32; 3]) -> [f32; 3] {
    let max = rgb.into_iter().fold(0., f32::max);
    let min = rgb.into_iter().fold(f32::INFINITY, f32::min);
    let d = max - min;
    let h = if d < 1e-8 {
        0.
    } else if max == rgb[0] {
        ((rgb[1] - rgb[2]) / d).rem_euclid(6.)
    } else if max == rgb[1] {
        (rgb[2] - rgb[0]) / d + 2.
    } else {
        (rgb[0] - rgb[1]) / d + 4.
    };
    [h * 60., if max > 0. { d / max } else { 0. }, max]
}
fn rgb([h, s, v]: [f32; 3]) -> [f32; 3] {
    let h = h.rem_euclid(360.) / 60.;
    let c = v * s;
    let x = c * (1. - (h.rem_euclid(2.) - 1.).abs());
    let m = v - c;
    let p = match h as u32 {
        0 => [c, x, 0.],
        1 => [x, c, 0.],
        2 => [0., c, x],
        3 => [0., x, c],
        4 => [x, 0., c],
        _ => [c, 0., x],
    };
    p.map(|v| v + m)
}
pub(super) fn color(mut pixel: [f32; 3], p: &DevelopParams) -> [f32; 3] {
    if p.hsl != [[0.; 3]; 8] {
        let [h, s, v] = hsv(pixel);
        let centers = [0., 30., 60., 120., 180., 240., 270., 300.];
        let mut adjustment = [0.; 3];
        let mut sum = 0.;
        for (center, values) in centers.into_iter().zip(p.hsl) {
            let distance = (h - center + 180.).rem_euclid(360.) - 180.;
            let weight = (1. - distance.abs() / 60.).max(0.);
            for c in 0..3 {
                adjustment[c] += values[c] * weight;
            }
            sum += weight;
        }
        if sum > 0. {
            adjustment = adjustment.map(|v| v / sum);
        }
        pixel = rgb([
            h + adjustment[0] * 45.,
            (s * (1. + adjustment[1])).clamp(0., 1.),
            (v * 2f32.powf(adjustment[2])).max(0.),
        ]);
    }
    if p.grading != [[0.; 3]; 3] {
        let l = luminance(pixel).clamp(0., 1.).sqrt();
        let weights = [
            (1. - 2. * l).max(0.),
            1. - (2. * l - 1.).abs(),
            (2. * l - 1.).max(0.),
        ];
        for ([h, s, light], weight) in p.grading.into_iter().zip(weights) {
            let tint = rgb([h, 1., 1.]);
            let mean = luminance(tint);
            for c in 0..3 {
                pixel[c] =
                    (pixel[c] + (tint[c] - mean) * s * weight * 0.25) * 2f32.powf(light * weight);
            }
        }
    }
    pixel
}
pub(super) fn local(
    mut pixel: [f32; 3],
    x: f32,
    y: f32,
    p: &DevelopParams,
    bitmaps: &[Option<emulsion_raster::Mask>],
) -> [f32; 3] {
    for (index, m) in p.masks.iter().enumerate() {
        let w = if let Some(mask) = &bitmaps[index] {
            if !m.enabled {
                0.
            } else {
                let value = mask.get(
                    ((x * mask.width() as f32) as u32).min(mask.width() - 1),
                    ((y * mask.height() as f32) as u32).min(mask.height() - 1),
                ) as f32
                    / 255.;
                if m.inverted { 1. - value } else { value }
            }
        } else {
            m.weight(x, y)
        };
        if w == 0. {
            continue;
        }
        let gray = luminance(pixel);
        pixel = pixel
            .map(|v| (gray + (v - gray) * (1. + m.saturation * w)) * 2f32.powf(m.exposure * w));
        pixel[0] *= 2f32.powf(m.temperature * w * 0.7);
        pixel[2] *= 2f32.powf(-m.temperature * w * 0.7);
    }
    pixel
}

pub(super) fn geometry(input: Raster, p: &DevelopParams, cancel: &AtomicBool) -> Result<Raster> {
    if p.crop == [0., 0., 1., 1.]
        && p.straighten == 0.
        && p.perspective == [0.; 2]
        && p.distortion == 0.
        && p.aberration == [0.; 2]
        && p.lens_profile.is_none()
    {
        return Ok(input);
    }
    let (w, h) = (input.width(), input.height());
    let [left, top, right, bottom] = p.crop;
    let (ow, oh) = (
        ((right - left) * w as f32).round().max(1.) as u32,
        ((bottom - top) * h as f32).round().max(1.) as u32,
    );
    let (sin, cos) = p.straighten.to_radians().sin_cos();
    let scale = w.min(h) as f32 / 2.;
    let pixels = input.to_pixels();
    let sample = |x: f32, y: f32, channel: usize| -> f32 {
        if !x.is_finite()
            || !y.is_finite()
            || x < -0.5
            || y < -0.5
            || x > w as f32 - 0.5
            || y > h as f32 - 0.5
        {
            return 0.;
        }
        let x = x.clamp(0., (w - 1) as f32);
        let y = y.clamp(0., (h - 1) as f32);
        let (ix, iy) = (x.floor() as u32, y.floor() as u32);
        let (fx, fy) = (x - ix as f32, y - iy as f32);
        let value = |x: u32, y: u32| pixels[(y * w + x) as usize][channel] as f32;
        let a = value(ix, iy) * (1. - fx) + value((ix + 1).min(w - 1), iy) * fx;
        let b = value(ix, (iy + 1).min(h - 1)) * (1. - fx)
            + value((ix + 1).min(w - 1), (iy + 1).min(h - 1)) * fx;
        a * (1. - fy) + b * fy
    };
    let mut out = vec![[0u16; 4]; ow as usize * oh as usize];
    out.par_chunks_mut(ow as usize)
        .enumerate()
        .try_for_each(|(y, row)| -> Result<()> {
            cancelled(cancel)?;
            for (x, pixel) in row.iter_mut().enumerate() {
                let nx = (left * w as f32
                    + (x as f32 + 0.5) * (right - left) * w as f32 / ow as f32
                    - w as f32 / 2.)
                    / scale;
                let ny = (top * h as f32
                    + (y as f32 + 0.5) * (bottom - top) * h as f32 / oh as f32
                    - h as f32 / 2.)
                    / scale;
                let denominator = 1. + p.perspective[0] * nx + p.perspective[1] * ny;
                if denominator <= 0.05 {
                    continue;
                }
                let (rx, ry) = (
                    (cos * nx + sin * ny) / denominator,
                    (-sin * nx + cos * ny) / denominator,
                );
                let radius = (rx * rx + ry * ry).sqrt();
                let mut radial = 1. + p.distortion * radius * radius;
                let mut vignette = 1.;
                let mut tca = [1.; 2];
                if let Some(lens) = p.lens_profile {
                    let r = radius * lens.scale;
                    let [a, b, c] = lens.distortion;
                    radial *= a * r.powi(3) + b * r * r + c * r + 1. - a - b - c;
                    let [k1, k2, k3] = lens.vignette;
                    vignette = (1. + k1 * r * r + k2 * r.powi(4) + k3 * r.powi(6))
                        .max(0.05)
                        .recip();
                    let [br, cr, vr, bb, cb, vb] = lens.tca;
                    tca = [br * r * r + cr * r + vr, bb * r * r + cb * r + vb];
                }
                for (c, value) in pixel.iter_mut().enumerate() {
                    let factor = radial
                        * match c {
                            0 => (1. + p.aberration[0]) * tca[0],
                            2 => (1. + p.aberration[1]) * tca[1],
                            _ => 1.,
                        };
                    *value = sample(
                        rx * factor * scale + w as f32 / 2. - 0.5,
                        ry * factor * scale + h as f32 / 2. - 0.5,
                        c,
                    )
                    .mul_add(if c < 3 { vignette } else { 1. }, 0.)
                    .round()
                    .clamp(0., 65535.) as u16;
                }
                let alpha = pixel[3];
                for channel in &mut pixel[..3] {
                    *channel = (*channel).min(alpha);
                }
            }
            Ok(())
        })?;
    Ok(Raster::from_pixels(ow, oh, [0; 4], &out))
}
