//! Histogram-based automatic corrections expressed as editable adjustments.
//! These implement tonal strategies, not Photoshop's proprietary algorithms.
use crate::{Adjustment, Mask, Raster, color::linear_to_srgb};

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum AutoCorrection {
    Tone,
    Contrast,
    Color,
}
impl AutoCorrection {
    pub fn label(self) -> &'static str {
        match self {
            Self::Tone => "Auto Tone",
            Self::Contrast => "Auto Contrast",
            Self::Color => "Auto Color",
        }
    }
}

/// Analyze straight sRGB, weighted by alpha and optional selection coverage.
/// Mask coordinates match the raster. Each histogram tail clips 0.1%.
/// Empty/flat inputs produce identity; large images use a bounded regular grid.
pub fn auto_correction(
    image: &Raster,
    selection: Option<&Mask>,
    mode: AutoCorrection,
) -> Adjustment {
    let mut hist = [[0.0_f64; 256]; 3];
    let mut luma_hist = [0.0_f64; 256];
    let mut total = 0.0;
    visit(image, selection, |rgb, weight| {
        for c in 0..3 {
            hist[c][bin(rgb[c])] += weight;
        }
        luma_hist[bin(luma(rgb))] += weight;
        total += weight;
    });
    let endpoints = hist.map(|h| bounds(&h, total, 0.001));
    if total == 0.0 || endpoints.iter().all(|[lo, hi]| hi - lo < 1.0) {
        return Adjustment::Curves {
            master: identity(),
            red: identity(),
            green: identity(),
            blue: identity(),
        };
    }
    if mode == AutoCorrection::Contrast {
        return Adjustment::Levels {
            in_black: endpoints.iter().map(|p| p[0]).fold(255.0_f32, f32::min),
            in_white: endpoints.iter().map(|p| p[1]).fold(0.0_f32, f32::max),
            gamma: 1.0,
            out_black: 0.0,
            out_white: 255.0,
        };
    }
    let mut endpoints = endpoints;
    let mut exponent = [1.0_f32; 3];
    if mode == AutoCorrection::Color {
        // Average extreme tonal bands rather than using different colored
        // objects as the black and white reference for each channel.
        let [dark, light] = bounds(&luma_hist, total, 0.01);
        let mut sums = [[0.0_f64; 3]; 2];
        let mut weights = [0.0; 2];
        visit(image, selection, |rgb, weight| {
            let brightness = bin(luma(rgb)) as f32;
            for (i, matches) in [brightness <= dark, brightness >= light]
                .into_iter()
                .enumerate()
            {
                if matches {
                    weights[i] += weight;
                    for (sum, value) in sums[i].iter_mut().zip(rgb) {
                        *sum += value as f64 * weight;
                    }
                }
            }
        });
        for (c, endpoint) in endpoints.iter_mut().enumerate() {
            let lo = (sums[0][c] / weights[0].max(f64::EPSILON)) as f32 * 255.0;
            let hi = (sums[1][c] / weights[1].max(f64::EPSILON)) as f32 * 255.0;
            if hi - lo >= 1.0 {
                *endpoint = [lo, hi];
            }
        }
        let mut neutral = [0.0_f64; 3];
        let mut neutral_weight = 0.0;
        visit(image, selection, |rgb, weight| {
            let rgb = std::array::from_fn::<_, 3, _>(|c| stretch(rgb[c], endpoints[c]));
            let min = rgb.into_iter().fold(1.0_f32, f32::min);
            let max = rgb.into_iter().fold(0.0_f32, f32::max);
            if (0.15..0.85).contains(&luma(rgb)) && max - min < 0.25 {
                let weight = weight * (1.0 - (max - min) as f64 / 0.25);
                neutral_weight += weight;
                for c in 0..3 {
                    neutral[c] += rgb[c] as f64 * weight;
                }
            }
        });
        if neutral_weight > 0.0 {
            let average = neutral.map(|v| (v / neutral_weight) as f32);
            let target = luma(average).clamp(0.05, 0.95);
            exponent = average.map(|v| (target.ln() / v.clamp(0.05, 0.95).ln()).clamp(0.5, 2.0));
        }
    }
    let curves = std::array::from_fn::<_, 3, _>(|c| {
        let [lo, hi] = endpoints[c];
        if hi - lo < 1.0 {
            return identity();
        }
        (0..=16)
            .map(|i| {
                let t = i as f32 / 16.0;
                [lo + t * (hi - lo), 255.0 * t.powf(exponent[c])]
            })
            .collect()
    });
    let [red, green, blue] = curves;
    Adjustment::Curves {
        master: identity(),
        red,
        green,
        blue,
    }
}
fn identity() -> Vec<[f32; 2]> {
    vec![[0.0, 0.0], [255.0, 255.0]]
}
fn stretch(value: f32, [lo, hi]: [f32; 2]) -> f32 {
    if hi - lo < 1.0 {
        value
    } else {
        ((value * 255.0 - lo) / (hi - lo)).clamp(0.0, 1.0)
    }
}
fn bin(value: f32) -> usize {
    (value.clamp(0.0, 1.0) * 255.0).round() as usize
}
fn luma(rgb: [f32; 3]) -> f32 {
    rgb[0] * 0.2126 + rgb[1] * 0.7152 + rgb[2] * 0.0722
}
fn bounds(hist: &[f64; 256], total: f64, clip: f64) -> [f32; 2] {
    let endpoint = |reverse: bool| {
        let mut sum = 0.0;
        for j in 0..256 {
            let i = if reverse { 255 - j } else { j };
            sum += hist[i];
            if sum > total * clip {
                return i as f32;
            }
        }
        if reverse { 255.0 } else { 0.0 }
    };
    [endpoint(false), endpoint(true)]
}
fn visit(image: &Raster, mask: Option<&Mask>, mut f: impl FnMut([f32; 3], f64)) {
    let area = mask.map_or_else(
        || image.bounds(),
        |m| image.bounds().intersect(&m.coverage_bounds()),
    );
    let step = ((area.w as f64 * area.h as f64 / 1_000_000.0).sqrt().ceil() as usize).max(1);
    for y in (area.y as u32..area.bottom() as u32).step_by(step) {
        for x in (area.x as u32..area.right() as u32).step_by(step) {
            let p = image.get(x, y);
            let coverage = mask.map_or(255, |m| {
                if x < m.width() && y < m.height() {
                    m.get(x, y)
                } else {
                    0
                }
            });
            if p[3] == 0 || coverage == 0 {
                continue;
            }
            let rgb = [p[0], p[1], p[2]]
                .map(|v| linear_to_srgb((v as f32 / p[3] as f32).clamp(0.0, 1.0)));
            f(rgb, p[3] as f64 / 65535.0 * coverage as f64 / 255.0);
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::color::srgb_to_linear;

    fn raster(colors: &[[u8; 4]]) -> Raster {
        Raster::from_srgba8(colors.len() as u32, 1, colors.as_flattened())
    }
    fn corrected(a: &Adjustment, rgb: [u8; 3]) -> [f32; 3] {
        a.prepare()
            .apply(rgb.map(|v| srgb_to_linear(v as f32 / 255.0)))
            .map(|v| linear_to_srgb(v) * 255.0)
    }
    #[test]
    fn tone_stretches_channels_contrast_uses_shared_endpoints() {
        let image = raster(&[
            [40, 60, 80, 255],
            [100, 120, 140, 255],
            [160, 180, 200, 255],
        ]);
        let tone = auto_correction(&image, None, AutoCorrection::Tone);
        let contrast = auto_correction(&image, None, AutoCorrection::Contrast);
        let t = corrected(&tone, [100, 120, 140]);
        assert!(t.iter().all(|v| (*v - 127.5).abs() < 1.0), "{t:?}");
        let c = corrected(&contrast, [100, 120, 140]);
        assert!(c[0] < c[1] && c[1] < c[2]);
        assert!((c[1] - c[0] - (c[2] - c[1])).abs() < 1.0);
    }
    #[test]
    fn color_neutralizes_midtone_cast_beyond_tone() {
        let image = raster(&[
            [20, 20, 20, 255],
            [145, 125, 115, 255],
            [230, 230, 230, 255],
        ]);
        let tone = corrected(
            &auto_correction(&image, None, AutoCorrection::Tone),
            [145, 125, 115],
        );
        let color = corrected(
            &auto_correction(&image, None, AutoCorrection::Color),
            [145, 125, 115],
        );
        assert!(tone[0] - tone[2] > 30.0);
        assert!((color[0] - color[2]).abs() < 2.0, "{color:?}");
    }
    #[test]
    fn empty_transparent_and_flat_inputs_are_identity() {
        for image in [
            Raster::transparent(0, 0),
            Raster::transparent(4, 4),
            raster(&[[80, 110, 160, 255]; 4]),
        ] {
            for mode in [
                AutoCorrection::Tone,
                AutoCorrection::Contrast,
                AutoCorrection::Color,
            ] {
                let output = corrected(&auto_correction(&image, None, mode), [80, 110, 160]);
                assert!(
                    output
                        .iter()
                        .zip([80.0, 110.0, 160.0])
                        .all(|(a, b)| (a - b).abs() < 1.0)
                );
            }
        }
    }
    #[test]
    fn selection_and_transparency_exclude_unrelated_pixels() {
        let base = raster(&[[60, 70, 80, 255], [180, 190, 200, 255]]);
        let image = raster(&[
            [60, 70, 80, 255],
            [180, 190, 200, 255],
            [0, 0, 0, 255],
            [255, 255, 255, 0],
        ]);
        let mask = Mask::empty(2, 1, 255);
        for mode in [
            AutoCorrection::Tone,
            AutoCorrection::Contrast,
            AutoCorrection::Color,
        ] {
            assert_eq!(
                auto_correction(&base, None, mode),
                auto_correction(&image, Some(&mask), mode)
            );
        }
    }
    #[test]
    fn small_selection_on_large_image_is_not_skipped_by_sampling() {
        let rect = crate::IRect::new(101, 103, 2, 1);
        let base = raster(&[[60, 70, 80, 255], [180, 190, 200, 255]]);
        let image =
            Raster::transparent(4000, 3000).write_rect(rect, &[base.get(0, 0), base.get(1, 0)]);
        let mask = Mask::empty(4000, 3000, 0).write_rect(rect, &[255, 255]);
        for mode in [
            AutoCorrection::Tone,
            AutoCorrection::Contrast,
            AutoCorrection::Color,
        ] {
            assert_eq!(
                auto_correction(&base, None, mode),
                auto_correction(&image, Some(&mask), mode)
            );
        }
    }
    #[test]
    fn sparse_outliers_are_clipped() {
        let mut colors = vec![[80, 80, 80, 255]; 1000];
        colors.extend(vec![[180, 180, 180, 255]; 1000]);
        colors.extend([[0, 0, 0, 255], [255, 255, 255, 255]]);
        let a = auto_correction(&raster(&colors), None, AutoCorrection::Contrast);
        let Adjustment::Levels {
            in_black, in_white, ..
        } = a
        else {
            panic!("expected Levels")
        };
        assert_eq!((in_black, in_white), (80.0, 180.0));
    }
    #[test]
    fn correction_curves_remain_finite_and_monotonic() {
        let image = raster(&[[0, 50, 70, 255], [80, 110, 150, 255], [210, 180, 230, 255]]);
        for mode in [
            AutoCorrection::Tone,
            AutoCorrection::Contrast,
            AutoCorrection::Color,
        ] {
            let a = auto_correction(&image, None, mode);
            let mut previous = [0.0; 3];
            for v in 0..=255 {
                let output = corrected(&a, [v; 3]);
                for c in 0..3 {
                    assert!(output[c].is_finite() && output[c] + 0.01 >= previous[c]);
                }
                previous = output;
            }
        }
    }
}
