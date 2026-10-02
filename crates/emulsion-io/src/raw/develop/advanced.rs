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
/// Independent primary calibration: mix each channel's chromatic contribution
/// around the neutral axis. Neutral input remains neutral at every setting.
pub(super) fn calibrate(pixel: [f32; 3], p: &DevelopParams) -> [f32; 3] {
    if p.process_version < 2 {
        return pixel;
    }
    let gray = luminance(pixel);
    let mut out = pixel;
    for c in 0..3 {
        let [h, s] = p.calibration[c];
        let chroma = pixel[c] - gray;
        let a = (c + 1) % 3;
        let b = (c + 2) % 3;
        out[c] += chroma * s * 0.5;
        out[a] += chroma * h * 0.35 - chroma * s * 0.25;
        out[b] -= chroma * h * 0.35 + chroma * s * 0.25;
    }
    let tint = p.shadow_tint * (1. - gray.clamp(0., 1.)).powi(2) * gray * 0.25;
    out[0] += tint;
    out[1] -= tint;
    out[2] += tint;
    out
}

/// Black-and-white conversion with per-hue brightness, like a B&W mixer:
/// neutrals keep their luminance and only coloured areas move.
pub(super) fn gray_mix(pixel: [f32; 3], p: &DevelopParams) -> f32 {
    let gray = luminance(pixel);
    if p.gray_mixer == [0.; 8] {
        return gray;
    }
    let [h, s, _] = hsv(pixel.map(|v| v.max(0.)));
    let centers = [0., 30., 60., 120., 180., 240., 270., 300.];
    let (mut adjustment, mut sum) = (0., 0.);
    for (center, value) in centers.into_iter().zip(p.gray_mixer) {
        let distance = (h - center + 180.).rem_euclid(360.) - 180.;
        let weight = (1. - distance.abs() / 60.).max(0.);
        adjustment += value * weight;
        sum += weight;
    }
    if sum > 0. {
        adjustment /= sum;
    }
    gray * 2f32.powf(adjustment * s.clamp(0., 1.) * 1.5)
}

fn grain_hash(x: i32, y: i32, seed: u32) -> f32 {
    let mut h = (x as u32).wrapping_mul(0x8da6_b343)
        ^ (y as u32).wrapping_mul(0xd816_3841)
        ^ seed.wrapping_mul(0xcb1a_b31f);
    h ^= h >> 13;
    h = h.wrapping_mul(0x5bd1_e995);
    h ^= h >> 15;
    h as f32 / u32::MAX as f32 * 2. - 1.
}

fn grain_noise(x: f32, y: f32, seed: u32) -> f32 {
    let (ix, iy) = (x.floor(), y.floor());
    let smooth = |t: f32| t * t * (3. - 2. * t);
    let (fx, fy) = (smooth(x - ix), smooth(y - iy));
    let (ix, iy) = (ix as i32, iy as i32);
    let top = grain_hash(ix, iy, seed) * (1. - fx) + grain_hash(ix + 1, iy, seed) * fx;
    let bottom = grain_hash(ix, iy + 1, seed) * (1. - fx) + grain_hash(ix + 1, iy + 1, seed) * fx;
    top * (1. - fy) + bottom * fy
}

/// Luminance-only film grain, applied last so sharpening never sees it.
/// Size scales with resolution, so previews and exports read alike.
pub(super) fn grain(input: Raster, p: &DevelopParams, cancel: &AtomicBool) -> Result<Raster> {
    let [amount, size, roughness] = p.grain;
    if amount <= 0. {
        return Ok(input);
    }
    cancelled(cancel)?;
    let (w, h) = (input.width(), input.height());
    let scale = (w.min(h) as f32 / 4000.).max(0.05);
    let cell = ((1. + size * 3.) * scale).max(0.35);
    let strength = amount * 0.12;
    let fine = roughness * 0.6;
    Ok(Raster::from_fn(w, h, [0; 4], |x, y| {
        let px = input.get(x, y);
        if px[3] == 0 {
            return px;
        }
        let (fx, fy) = (x as f32 / cell, y as f32 / cell);
        let noise = grain_noise(fx, fy, 1) * (1. - fine) + grain_noise(fx * 2., fy * 2., 2) * fine;
        let alpha = px[3] as f32 / 65535.;
        let linear = [px[0], px[1], px[2]].map(|v| v as f32 / 65535. / alpha);
        let level = luminance(linear).clamp(0., 1.).powf(1. / 2.2);
        // Strongest in the midtones, as film grain reads.
        let delta = noise * strength * (0.35 + 2.6 * level * (1. - level));
        let mut out = px;
        for c in 0..3 {
            let v = (linear[c].clamp(0., 1.).powf(1. / 2.2) + delta).clamp(0., 1.);
            out[c] = (v.powf(2.2) * alpha * 65535. + 0.5) as u16;
        }
        out
    }))
}

pub(super) fn parametric(pixel: [f32; 3], p: &DevelopParams) -> [f32; 3] {
    if p.process_version < 2 || p.parametric == [0.; 4] {
        return pixel;
    }
    let l = luminance(pixel).max(0.);
    if l <= 1e-8 || l >= 1. {
        return pixel;
    }
    let x = l.powf(1. / 2.2);
    let [a, b, c] = p.parametric_splits;
    let centers = [0., a, b, c, 1.];
    let mut adjustment = 0.;
    // Overlapping smooth lobes keep split boundaries continuous.
    for i in 0..4 {
        let center = (centers[i] + centers[i + 1]) * 0.5;
        let radius = (centers[i + 1] - centers[i]) * 0.5 + 0.18;
        let t = (1. - (x - center).abs() / radius).clamp(0., 1.);
        adjustment += p.parametric[i] * t * t * (3. - 2. * t);
    }
    let y = (x + adjustment * x * (1. - x) * 0.5)
        .clamp(0., 1.)
        .powf(2.2);
    pixel.map(|v| v * y / l)
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
        let mut weights = [
            (1. - 2. * l).max(0.),
            1. - (2. * l - 1.).abs(),
            (2. * l - 1.).max(0.),
        ];
        if p.process_version >= 2 && (p.grading_balance != 0. || p.grading_blending != 0.5) {
            let shifted = (l - p.grading_balance * 0.4).clamp(0., 1.);
            let width = 0.25 + p.grading_blending * 0.75;
            weights = [0., 0.5, 1.].map(|center| (1. - (shifted - center).abs() / width).max(0.));
            let total = weights.iter().sum::<f32>().max(1e-6);
            weights = weights.map(|v| v / total);
        }
        for ([h, s, light], weight) in p.grading.into_iter().zip(weights) {
            let tint = rgb([h, 1., 1.]);
            let mean = luminance(tint);
            for c in 0..3 {
                pixel[c] =
                    (pixel[c] + (tint[c] - mean) * s * weight * 0.25) * 2f32.powf(light * weight);
            }
        }
    }
    if p.process_version >= 2 && p.global_grading != [0.; 3] {
        let [h, s, l] = p.global_grading;
        let tint = rgb([h, 1., 1.]);
        let mean = luminance(tint);
        for c in 0..3 {
            pixel[c] = (pixel[c] + (tint[c] - mean) * s * 0.25) * 2f32.powf(l);
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
    let image = geometry_unrotated(input, p, cancel)?;
    if p.rotation == 0 {
        return Ok(image);
    }
    let (w, h) = (image.width(), image.height());
    let (ow, oh) = if p.rotation % 2 == 1 { (h, w) } else { (w, h) };
    let input = image.to_pixels();
    let mut output = vec![[0; 4]; input.len()];
    output
        .par_chunks_mut(ow as usize)
        .enumerate()
        .try_for_each(|(y, row)| -> Result<()> {
            cancelled(cancel)?;
            for (x, pixel) in row.iter_mut().enumerate() {
                let (sx, sy) = match p.rotation {
                    1 => (y as u32, h - 1 - x as u32),
                    2 => (w - 1 - x as u32, h - 1 - y as u32),
                    3 => (w - 1 - y as u32, x as u32),
                    _ => unreachable!("validated rotation"),
                };
                *pixel = input[(sy * w + sx) as usize];
            }
            Ok(())
        })?;
    Ok(Raster::from_pixels(ow, oh, [0; 4], &output))
}
fn geometry_unrotated(input: Raster, p: &DevelopParams, cancel: &AtomicBool) -> Result<Raster> {
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

#[cfg(test)]
mod orientation_tests {
    use super::*;
    #[test]
    fn quarter_turns_preserve_exact_pixels_alpha_and_crop_dimensions() {
        let pixels = [
            [101, 202, 303, 500],
            [400, 500, 600, 700],
            [700, 800, 900, 1000],
            [1000, 1100, 1200, 1300],
            [1300, 1400, 1500, 1600],
            [1600, 1700, 1800, 1900],
        ];
        let input = Raster::from_pixels(3, 2, [0; 4], &pixels);
        for (rotation, indices) in [
            (1, vec![3, 0, 4, 1, 5, 2]),
            (2, vec![5, 4, 3, 2, 1, 0]),
            (3, vec![2, 5, 1, 4, 0, 3]),
        ] {
            let params = DevelopParams {
                rotation,
                ..Default::default()
            };
            let out = super::super::render_raster(&input, &params).unwrap();
            assert_eq!(
                (out.width(), out.height()),
                if rotation % 2 == 1 { (2, 3) } else { (3, 2) }
            );
            assert_eq!(
                out.to_pixels(),
                indices.into_iter().map(|i| pixels[i]).collect::<Vec<_>>()
            );
        }
        let mut cycle = input.clone();
        for _ in 0..4 {
            cycle = super::super::render_raster(
                &cycle,
                &DevelopParams {
                    rotation: 1,
                    ..Default::default()
                },
            )
            .unwrap();
        }
        assert_eq!(cycle.to_pixels(), pixels);
        let out = geometry(
            input,
            &DevelopParams {
                crop: [0., 0., 2. / 3., 1.],
                rotation: 1,
                ..Default::default()
            },
            &AtomicBool::new(false),
        )
        .unwrap();
        assert_eq!((out.width(), out.height()), (2, 2));
        assert_eq!(
            out.to_pixels(),
            vec![pixels[3], pixels[0], pixels[4], pixels[1]]
        );
    }
}

#[cfg(test)]
mod film_tests {
    use super::*;

    #[test]
    fn gray_mixer_moves_coloured_tones_and_keeps_neutrals() {
        let blue = [0.05, 0.1, 0.6];
        let red = [0.6, 0.1, 0.05];
        let gray = [0.3; 3];
        let mut p = DevelopParams {
            saturation: -1.,
            ..DevelopParams::default()
        };
        assert_eq!(gray_mix(blue, &p), luminance(blue));
        p.gray_mixer[5] = -1.;
        p.gray_mixer[0] = 1.;
        assert!(gray_mix(blue, &p) < luminance(blue) * 0.6);
        assert!(gray_mix(red, &p) > luminance(red) * 1.5);
        assert!((gray_mix(gray, &p) - luminance(gray)).abs() < 1e-6);
    }

    #[test]
    fn grain_is_deterministic_luminance_texture_that_zero_disables() {
        let input = Raster::solid(64, 48, [0.2, 0.2, 0.2, 1.]);
        let none = AtomicBool::new(false);
        let mut p = DevelopParams::default();
        let same = grain(input.clone(), &p, &none).unwrap();
        assert_eq!(same.to_pixels(), input.to_pixels());
        p.grain = [0.6, 0.3, 0.6];
        let a = grain(input.clone(), &p, &none).unwrap().to_pixels();
        let b = grain(input.clone(), &p, &none).unwrap().to_pixels();
        assert_eq!(a, b);
        let base = input.get(0, 0)[0] as f64;
        let mean = a.iter().map(|px| px[0] as f64).sum::<f64>() / a.len() as f64;
        let spread = a.iter().map(|px| (px[0] as f64 - mean).abs()).sum::<f64>() / a.len() as f64;
        assert!(spread > 200., "grain should be visible: {spread}");
        assert!(
            (mean - base).abs() / base < 0.1,
            "grain should not shift exposure"
        );
        // Luminance only: channels move together on a neutral.
        assert!(a.iter().all(|px| px[0] == px[1] && px[1] == px[2]));
    }
}
