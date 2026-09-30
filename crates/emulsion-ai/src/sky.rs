//! Semantic sky segmentation using the published MIT-licensed U-2-Net model.
use crate::{
    jobs::Job,
    models,
    prep::{Map, Planes, guided_filter},
    runner::{self, RunError},
};
use emulsion_raster::{Mask, Raster};
pub fn mask(image: &Raster, job: &Job) -> Result<Mask, RunError> {
    let spec = models::spec("skyseg")
        .ok_or_else(|| RunError::Other("Sky model manifest missing".into()))?;
    let model = runner::model(&models::file_path(spec, &spec.files[0]))?;
    job.set_stage("Segmenting sky");
    job.check()?;
    let planes = Planes::from_raster(image).flattened(0.5);
    let input = planes
        .resized(320, 320)
        .to_nchw([0.485, 0.456, 0.406], [0.229, 0.224, 0.225]);
    let output = model.run(&[(&model.inputs[0], input)])?;
    job.check()?;
    let tensor = output
        .get(&model.outputs[0])
        .ok_or_else(|| RunError::Shape(spec.id.into(), "missing sky output".into()))?;
    if tensor.shape() != [1, 1, 320, 320] || tensor.iter().any(|v| !v.is_finite()) {
        return Err(RunError::Shape(
            spec.id.into(),
            "invalid sky output dimensions or values".into(),
        ));
    }
    let map = Map::from_hw(tensor).resized(planes.w, planes.h);
    job.progress(0.8);
    let mask = guided_filter(&planes, &map, 4, 1e-3).to_mask();
    job.check()?;
    job.progress(1.);
    Ok(mask)
}

/// Built-in skies for sky replacement, drawn procedurally so they fit any
/// canvas without shipping image assets.
#[derive(Clone, Copy, Debug, PartialEq, Eq, Hash)]
pub enum SkyPreset {
    Blue,
    Clouds,
    GoldenHour,
    Sunset,
    Dusk,
    Stormy,
}

impl SkyPreset {
    pub const ALL: [SkyPreset; 6] = [
        SkyPreset::Blue,
        SkyPreset::Clouds,
        SkyPreset::GoldenHour,
        SkyPreset::Sunset,
        SkyPreset::Dusk,
        SkyPreset::Stormy,
    ];

    pub fn label(self) -> &'static str {
        match self {
            SkyPreset::Blue => "Clear blue",
            SkyPreset::Clouds => "Fair clouds",
            SkyPreset::GoldenHour => "Golden hour",
            SkyPreset::Sunset => "Sunset",
            SkyPreset::Dusk => "Blue hour",
            SkyPreset::Stormy => "Stormy",
        }
    }

    pub fn key(self) -> &'static str {
        match self {
            SkyPreset::Blue => "blue",
            SkyPreset::Clouds => "clouds",
            SkyPreset::GoldenHour => "golden_hour",
            SkyPreset::Sunset => "sunset",
            SkyPreset::Dusk => "dusk",
            SkyPreset::Stormy => "stormy",
        }
    }

    pub fn from_key(key: &str) -> Option<SkyPreset> {
        Self::ALL.into_iter().find(|p| p.key() == key)
    }

    /// Zenith, middle and horizon colours (sRGB 0–1), cloud cover (0–1),
    /// cloud colour, and sun glow strength.
    fn look(self) -> ([[f32; 3]; 3], f32, [f32; 3], f32) {
        match self {
            SkyPreset::Blue => (
                [[0.16, 0.36, 0.72], [0.35, 0.58, 0.88], [0.72, 0.84, 0.95]],
                0.0,
                [1.0, 1.0, 1.0],
                0.0,
            ),
            SkyPreset::Clouds => (
                [[0.20, 0.42, 0.78], [0.42, 0.63, 0.90], [0.76, 0.86, 0.95]],
                0.45,
                [0.98, 0.98, 0.99],
                0.0,
            ),
            SkyPreset::GoldenHour => (
                [[0.30, 0.45, 0.72], [0.85, 0.70, 0.55], [1.0, 0.80, 0.52]],
                0.3,
                [1.0, 0.86, 0.66],
                0.6,
            ),
            SkyPreset::Sunset => (
                [[0.20, 0.20, 0.45], [0.85, 0.42, 0.38], [1.0, 0.62, 0.30]],
                0.4,
                [0.98, 0.55, 0.42],
                1.0,
            ),
            SkyPreset::Dusk => (
                [[0.05, 0.08, 0.25], [0.20, 0.25, 0.52], [0.62, 0.50, 0.62]],
                0.15,
                [0.45, 0.40, 0.55],
                0.2,
            ),
            SkyPreset::Stormy => (
                [[0.22, 0.24, 0.28], [0.40, 0.43, 0.47], [0.62, 0.64, 0.66]],
                0.8,
                [0.50, 0.52, 0.56],
                0.0,
            ),
        }
    }
}

fn hash2(x: i32, y: i32, seed: u32) -> f32 {
    let mut h = (x as u32).wrapping_mul(0x8da6_b343)
        ^ (y as u32).wrapping_mul(0xd825_5f9d)
        ^ seed.wrapping_mul(0x9e37_79b9);
    h ^= h >> 15;
    h = h.wrapping_mul(0x2c1b_3c6d);
    h ^= h >> 12;
    (h & 0x00ff_ffff) as f32 / 16_777_216.0
}

fn value_noise(x: f32, y: f32, seed: u32) -> f32 {
    let (ix, iy) = (x.floor() as i32, y.floor() as i32);
    let (fx, fy) = (x - ix as f32, y - iy as f32);
    let s = |t: f32| t * t * (3.0 - 2.0 * t);
    let (sx, sy) = (s(fx), s(fy));
    let a = hash2(ix, iy, seed);
    let b = hash2(ix + 1, iy, seed);
    let c = hash2(ix, iy + 1, seed);
    let d = hash2(ix + 1, iy + 1, seed);
    (a + (b - a) * sx) * (1.0 - sy) + (c + (d - c) * sx) * sy
}

fn fbm(x: f32, y: f32, seed: u32) -> f32 {
    let (mut v, mut amp, mut f, mut norm) = (0.0, 0.5, 1.0, 0.0);
    for o in 0..5 {
        v += value_noise(x * f, y * f, seed + o) * amp;
        norm += amp;
        amp *= 0.5;
        f *= 2.0;
    }
    v / norm
}

fn mix3(a: [f32; 3], b: [f32; 3], t: f32) -> [f32; 3] {
    [0, 1, 2].map(|i| a[i] + (b[i] - a[i]) * t)
}

fn srgb_px(c: [f32; 3]) -> [u16; 4] {
    use emulsion_raster::color;
    color::f_to_px([
        color::srgb_to_linear(c[0].clamp(0.0, 1.0)),
        color::srgb_to_linear(c[1].clamp(0.0, 1.0)),
        color::srgb_to_linear(c[2].clamp(0.0, 1.0)),
        1.0,
    ])
}

/// Draw `preset` at `w × h` with its horizon at row `horizon` (0–1 of the
/// height): a zenith-to-horizon gradient, soft clouds and, for low sun, a
/// glow near the horizon.
pub fn render_preset(preset: SkyPreset, w: u32, h: u32, horizon: f32) -> Raster {
    let ([zenith, mid, low], cover, cloud, sun) = preset.look();
    let hy = (horizon.clamp(0.1, 1.0) * h as f32).max(1.0);
    let scale = 3.0 / w.max(h).max(1) as f32;
    let (sun_x, sun_y) = (w as f32 * 0.68, hy * 0.92);
    let sun_r = w.max(h) as f32 * 0.35;
    Raster::from_fn(w, h, [0; 4], |x, y| {
        let (fx, fy) = (x as f32 + 0.5, y as f32 + 0.5);
        let t = (fy / hy).clamp(0.0, 1.0);
        let mut c = if t < 0.55 {
            mix3(zenith, mid, t / 0.55)
        } else {
            mix3(mid, low, (t - 0.55) / 0.45)
        };
        if sun > 0.0 {
            let d = ((fx - sun_x).powi(2) + (fy - sun_y).powi(2)).sqrt() / sun_r;
            let g = (1.0 - d).max(0.0).powi(3) * sun;
            c = mix3(c, [1.0, 0.93, 0.78], g.min(1.0));
        }
        if cover > 0.0 {
            // Stretch clouds horizontally and flatten them toward the horizon.
            let n = fbm(fx * scale * 0.6, fy * scale * (1.4 + t * 1.6), 7);
            let edge = 1.0 - cover;
            let density = ((n - edge * 0.75) / 0.22).clamp(0.0, 1.0) * (1.0 - t * 0.35);
            let shade = fbm(fx * scale * 0.6, (fy + 8.0) * scale * 1.8, 7);
            let lit = mix3(
                cloud,
                [cloud[0] * 0.72, cloud[1] * 0.72, cloud[2] * 0.76],
                shade,
            );
            c = mix3(c, lit, density);
        }
        srgb_px(c)
    })
}

/// Scale `sky` to cover `w × h`, keeping its top edge, so a photographed
/// sky's upper part lands in the picture's sky.
pub fn fit_image(sky: &Raster, w: u32, h: u32) -> Raster {
    let (sw, sh) = (sky.width().max(1) as f32, sky.height().max(1) as f32);
    let s = (w as f32 / sw).max(h as f32 / sh);
    let ox = (sw * s - w as f32) / 2.0;
    let px = sky.to_pixels();
    let (iw, ih) = (sky.width() as usize, sky.height() as usize);
    Raster::from_fn(w, h, [0; 4], |x, y| {
        let sx = ((x as f32 + 0.5 + ox) / s - 0.5).clamp(0.0, sw - 1.0);
        let sy = ((y as f32 + 0.5) / s - 0.5).clamp(0.0, sh - 1.0);
        let (x0, y0) = (sx.floor() as usize, sy.floor() as usize);
        let (x1, y1) = ((x0 + 1).min(iw - 1), (y0 + 1).min(ih - 1));
        let (tx, ty) = (sx - x0 as f32, sy - y0 as f32);
        let g = |x: usize, y: usize| px[y * iw + x];
        let mut out = [0u16; 4];
        for (k, o) in out.iter_mut().enumerate() {
            let top = g(x0, y0)[k] as f32 * (1.0 - tx) + g(x1, y0)[k] as f32 * tx;
            let bot = g(x0, y1)[k] as f32 * (1.0 - tx) + g(x1, y1)[k] as f32 * tx;
            *o = (top * (1.0 - ty) + bot * ty).round() as u16;
        }
        out[3] = u16::MAX;
        out
    })
}

/// Where the sky ends: the lowest row (0–1 of the height) at which at
/// least 2 % of the row is still sky. `None` when there is no sky.
pub fn horizon(mask: &Mask) -> Option<f32> {
    let (w, h) = (mask.width() as usize, mask.height() as usize);
    if w == 0 || h == 0 {
        return None;
    }
    let px = mask.to_pixels();
    let lowest = (0..h).rev().find(|&y| {
        let row = &px[y * w..(y + 1) * w];
        row.iter().filter(|&&v| v > 127).count() * 50 >= w
    })?;
    Some((lowest + 1) as f32 / h as f32)
}

/// Fraction of the picture the mask covers.
pub fn coverage(mask: &Mask) -> f32 {
    let px = mask.to_pixels();
    if px.is_empty() {
        return 0.0;
    }
    px.iter().map(|&v| v as f32).sum::<f32>() / (px.len() as f32 * 255.0)
}

/// Average sRGB colour of `image` weighted by `mask`, 0–255; used to tint
/// the foreground so it matches a new sky's light.
pub fn mean_color(image: &Raster, mask: &Mask) -> [u8; 3] {
    use emulsion_raster::color;
    let (px, m) = (image.to_pixels(), mask.to_pixels());
    let (mut acc, mut n) = ([0.0f64; 3], 0.0f64);
    for (p, &w) in px.iter().zip(&m).step_by(7) {
        let f = color::px_to_f(*p);
        if w == 0 || f[3] <= 1e-4 {
            continue;
        }
        let wt = w as f64 / 255.0;
        for k in 0..3 {
            acc[k] += color::linear_to_srgb(f[k] / f[3]) as f64 * wt;
        }
        n += wt;
    }
    if n <= 0.0 {
        return [128, 128, 128];
    }
    acc.map(|v| ((v / n).clamp(0.0, 1.0) * 255.0).round() as u8)
}

/// The same colour as a Photo filter's hue (degrees) and saturation (0–100).
pub fn hue_saturation(rgb: [u8; 3]) -> (f32, f32) {
    let [r, g, b] = rgb.map(|v| v as f32 / 255.0);
    let (max, min) = (r.max(g).max(b), r.min(g).min(b));
    let d = max - min;
    if d <= 1e-6 {
        return (0.0, 0.0);
    }
    let h = if max == r {
        60.0 * (((g - b) / d).rem_euclid(6.0))
    } else if max == g {
        60.0 * ((b - r) / d + 2.0)
    } else {
        60.0 * ((r - g) / d + 4.0)
    };
    (h, (d / max * 100.0).clamp(0.0, 100.0))
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn presets_render_at_canvas_size_and_are_opaque() {
        for preset in SkyPreset::ALL {
            let r = render_preset(preset, 64, 40, 0.6);
            assert_eq!((r.width(), r.height()), (64, 40));
            assert!(r.to_pixels().iter().all(|p| p[3] == u16::MAX));
            assert_eq!(SkyPreset::from_key(preset.key()), Some(preset));
        }
    }

    #[test]
    fn preset_is_darker_at_the_zenith_than_at_the_horizon() {
        let r = render_preset(SkyPreset::Blue, 8, 100, 1.0);
        let l = |y| {
            let p = r.get(4, y);
            p[0] as u32 + p[1] as u32 + p[2] as u32
        };
        assert!(l(2) < l(95));
    }

    #[test]
    fn horizon_is_the_lowest_row_with_sky() {
        let mut px = vec![0u8; 10 * 20];
        for v in px.iter_mut().take(10 * 8) {
            *v = 255;
        }
        let m = Mask::from_gray8(10, 20, &px);
        assert_eq!(horizon(&m), Some(8.0 / 20.0));
        assert!((coverage(&m) - 0.4).abs() < 1e-3);
        assert_eq!(horizon(&Mask::from_gray8(4, 4, &[0; 16])), None);
    }

    #[test]
    fn fitted_sky_covers_the_canvas() {
        let src = Raster::solid(30, 10, [0.2, 0.3, 0.8, 1.0]);
        let fit = fit_image(&src, 50, 40);
        assert_eq!((fit.width(), fit.height()), (50, 40));
        assert!(
            fit.to_pixels()
                .iter()
                .all(|p| p[3] == u16::MAX && p[2] > p[0])
        );
    }

    #[test]
    fn mean_colour_follows_the_mask() {
        let img = Raster::from_fn(4, 2, [0; 4], |_, y| {
            if y == 0 {
                srgb_px([1.0, 0.0, 0.0])
            } else {
                srgb_px([0.0, 0.0, 1.0])
            }
        });
        let m = Mask::from_gray8(4, 2, &[255, 255, 255, 255, 0, 0, 0, 0]);
        let c = mean_color(&img, &m);
        assert!(c[0] > 200 && c[2] < 30, "{c:?}");
        let (h, s) = hue_saturation([255, 0, 0]);
        assert!(h.abs() < 1e-3 && (s - 100.0).abs() < 1e-3);
    }
    #[test]
    #[ignore = "requires installed sky model"]
    fn installed_sky_inference() {
        let r = emulsion_raster::Raster::solid(32, 24, [0.1, 0.4, 0.8, 1.]);
        let mask = super::mask(&r, &crate::jobs::Job::new()).unwrap();
        assert_eq!((mask.width(), mask.height()), (32, 24));
    }
}
