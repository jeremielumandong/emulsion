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
        pixels = apply_pixels_cpu(
            &Filter::ReduceNoise {
                strength: params.noise_reduction * 10.,
                detail: 50.,
            },
            w,
            h,
            pixels,
        )
        .ok_or_else(|| invalid("RAW noise reduction failed"))?;
        normalize(&mut pixels);
    }
    for (amount, radius) in [
        (params.texture, 1.5),
        (params.clarity, 12.),
        (params.sharpening * 1.5, 0.8),
    ] {
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
                for ch in 0..3 {
                    p[ch] += amount * (p[ch] - b[ch] / alpha);
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
