//! Spatial RAW effects share the editor's CPU filter kernels. All-zero settings
//! bypass this stage, preserving the output of existing documents exactly.
use super::*;
use emulsion_filters::{Filter, apply_pixels_cpu};

pub(super) fn apply(
    w: usize,
    h: usize,
    pixels: Vec<[f32; 3]>,
    params: &DevelopParams,
    cancel: &AtomicBool,
) -> Result<Vec<[f32; 3]>> {
    let mut pixels: Vec<[f32; 4]> = pixels.into_iter().map(|p| [p[0], p[1], p[2], 1.]).collect();
    if params.noise_reduction > 0. {
        cancelled(cancel)?;
        let original = if params.process_version >= 2 && params.luminance_contrast > 0. {
            Some(pixels.clone())
        } else {
            None
        };
        pixels = apply_pixels_cpu(
            &Filter::ReduceNoise {
                strength: params.noise_reduction * 10.,
                detail: if params.process_version == 1 {
                    50.
                } else {
                    params.luminance_detail * 100.
                },
            },
            w,
            h,
            pixels,
        )
        .ok_or_else(|| invalid("RAW noise reduction failed"))?;
        normalize(&mut pixels);
        if let Some(original) = original {
            for (p, src) in pixels.iter_mut().zip(original) {
                let y = luminance([p[0], p[1], p[2]]);
                let sy = luminance([src[0], src[1], src[2]]);
                let weight = ((sy - y).abs() * 20.).clamp(0., 1.) * params.luminance_contrast;
                for channel in p.iter_mut().take(3) {
                    *channel += (sy - y) * weight;
                }
            }
        }
    }
    if params.process_version >= 2 && params.color_noise_reduction > 0. {
        cancelled(cancel)?;
        let radius = 0.5 + params.color_noise_smoothness * 2.5;
        let blur = apply_pixels_cpu(&Filter::GaussianBlur { radius }, w, h, pixels.clone())
            .ok_or_else(|| invalid("Chroma denoise failed"))?;
        pixels
            .par_iter_mut()
            .zip(blur.par_iter())
            .for_each(|(p, b)| {
                let src = [p[0], p[1], p[2]];
                let dst = [b[0], b[1], b[2]].map(|v| v / b[3].max(1e-6));
                let y = luminance(src);
                let by = luminance(dst);
                let edge = (y - by).abs();
                let weight =
                    params.color_noise_reduction / (1. + edge * params.color_noise_detail * 100.);
                for c in 0..3 {
                    p[c] = y + (src[c] - y) * (1. - weight) + (dst[c] - by) * weight;
                }
            });
    }
    for (index, (amount, radius)) in [
        (params.texture, 1.5),
        (params.clarity, 12.),
        (
            params.sharpening * 1.5,
            if params.process_version == 1 {
                0.8
            } else {
                params.sharpening_radius
            },
        ),
    ]
    .into_iter()
    .enumerate()
    {
        if amount == 0. {
            continue;
        }
        cancelled(cancel)?;
        let blur = apply_pixels_cpu(&Filter::GaussianBlur { radius }, w, h, pixels.clone())
            .ok_or_else(|| invalid("RAW detail filter failed"))?;
        pixels
            .par_iter_mut()
            .zip(blur.par_iter())
            .for_each(|(p, b)| {
                // Normalize the blur at image boundaries so a constant field stays
                // constant, without dark rims from the filter's transparent padding.
                let alpha = b[3].max(1e-6);
                let edge = ((p[0] - b[0] / alpha).abs()
                    + (p[1] - b[1] / alpha).abs()
                    + (p[2] - b[2] / alpha).abs())
                    / 3.;
                let weight = if index == 2 && params.process_version >= 2 {
                    let mask = if params.sharpening_masking == 0. {
                        1.
                    } else {
                        (edge / (params.sharpening_masking * 0.08).max(1e-6)).clamp(0., 1.)
                    };
                    mask * (0.5 + params.sharpening_detail)
                } else {
                    1.
                };
                for ch in 0..3 {
                    p[ch] += amount * weight * (p[ch] - b[ch] / alpha);
                }
            });
    }
    cancelled(cancel)?;
    Ok(pixels.into_iter().map(|p| [p[0], p[1], p[2]]).collect())
}
fn normalize(pixels: &mut [[f32; 4]]) {
    for p in pixels {
        let alpha = p[3].max(1e-6);
        for c in &mut p[..3] {
            *c /= alpha;
        }
        p[3] = 1.;
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    #[test]
    fn spatial_controls_preserve_constant_fields_and_dimensions() {
        let pixels = vec![[0.3, 0.4, 0.5]; 17 * 13];
        for params in [
            DevelopParams {
                texture: 0.7,
                clarity: 0.6,
                sharpening: 0.4,
                ..Default::default()
            },
            DevelopParams {
                noise_reduction: 0.8,
                ..Default::default()
            },
        ] {
            let out = apply(17, 13, pixels.clone(), &params, &AtomicBool::new(false)).unwrap();
            assert_eq!(out.len(), pixels.len());
            for (a, b) in out.iter().zip(&pixels) {
                for i in 0..3 {
                    assert!((a[i] - b[i]).abs() < 1e-4, "{a:?} != {b:?}");
                }
            }
        }
    }
    #[test]
    fn noise_reduction_reduces_low_amplitude_noise_and_detail_is_not_identity() {
        let pixels: Vec<_> = (0..32 * 24)
            .map(|i| [0.4 + if i % 2 == 0 { 0.015 } else { -0.015 }; 3])
            .collect();
        let denoised = apply(
            32,
            24,
            pixels.clone(),
            &DevelopParams {
                noise_reduction: 0.8,
                ..Default::default()
            },
            &AtomicBool::new(false),
        )
        .unwrap();
        let variance = |v: &[[f32; 3]]| v.iter().map(|p| (p[0] - 0.4).powi(2)).sum::<f32>();
        assert!(variance(&denoised) < variance(&pixels));
        let sharp = apply(
            32,
            24,
            pixels.clone(),
            &DevelopParams {
                sharpening: 0.5,
                ..Default::default()
            },
            &AtomicBool::new(false),
        )
        .unwrap();
        assert!(variance(&sharp) > variance(&pixels));
    }
}
