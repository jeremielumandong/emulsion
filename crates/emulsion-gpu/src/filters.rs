//! GPU kernels for the linear-premultiplied filter pipeline.

use crate::GpuContext;
use emulsion_filters::{Filter, FilterAccelerator};

const SHADER: &str = include_str!("filters.wgsl");

impl GpuContext {
    fn filter_pass(
        &self,
        pixels: &[[f32; 4]],
        auxiliary: &[[f32; 4]],
        params: [u32; 8],
        kernel: &[f32],
    ) -> Option<Vec<[f32; 4]>> {
        let bytes = self
            .run(
                "filters",
                SHADER,
                &[
                    bytemuck::cast_slice(pixels),
                    bytemuck::cast_slice(auxiliary),
                    bytemuck::cast_slice(&params),
                    bytemuck::cast_slice(kernel),
                ],
                pixels.len().checked_mul(16)?,
                u32::try_from(pixels.len().div_ceil(64)).ok()?,
            )
            .ok()?;
        if bytes.len() != pixels.len() * 16 {
            return None;
        }
        // Readback bytes do not promise f32 alignment.
        Some(
            bytes
                .as_chunks::<16>()
                .0
                .iter()
                .map(|bytes| bytemuck::pod_read_unaligned(bytes))
                .collect(),
        )
    }

    fn blur_pixels(
        &self,
        pixels: &[[f32; 4]],
        width: u32,
        height: u32,
        radius: f32,
        gaussian: bool,
    ) -> Option<Vec<[f32; 4]>> {
        // Malformed or unbounded programmatic parameters use the existing CPU
        // path instead of risking an excessively long shader dispatch.
        if !radius.is_finite() || !(0.0..=250.0).contains(&radius) {
            return None;
        }
        let kernel = if gaussian {
            if radius <= 0.05 {
                return Some(pixels.to_vec());
            }
            let sigma = (radius / 2.0).max(0.3);
            let r = (sigma * 3.0).ceil() as i32;
            let mut kernel: Vec<f32> = (-r..=r)
                .map(|i| (-(i * i) as f32 / (2.0 * sigma * sigma)).exp())
                .collect();
            let sum: f32 = kernel.iter().sum();
            for weight in &mut kernel {
                *weight /= sum;
            }
            kernel
        } else {
            let r = radius.round() as usize;
            if r == 0 {
                return Some(pixels.to_vec());
            }
            vec![1.0 / (2 * r + 1) as f32; 2 * r + 1]
        };
        let mut params = [width, height, 0, kernel.len() as u32, 0, 0, 0, 0];
        let horizontal = self.filter_pass(pixels, &[[0.0; 4]], params, &kernel)?;
        params[2] = 1;
        self.filter_pass(&horizontal, &[[0.0; 4]], params, &kernel)
    }
}

impl FilterAccelerator for GpuContext {
    fn apply(
        &self,
        filter: &Filter,
        width: usize,
        height: usize,
        pixels: &[[f32; 4]],
    ) -> Option<Vec<[f32; 4]>> {
        if !crate::gpu_preferred() && !prefer_gpu_filter(filter, width, height) {
            return None;
        }
        self.apply_filter_gpu(filter, width, height, pixels)
    }
}

/// Synchronous upload/readback currently pays off reliably for bilateral noise
/// reduction at these sizes. Other kernels remain available in explicit GPU
/// mode, while the automatic path avoids measured regressions in simple effects.
fn prefer_gpu_filter(filter: &Filter, width: usize, height: usize) -> bool {
    matches!(filter, Filter::ReduceNoise { strength, .. } if *strength > 0.0)
        && width
            .checked_mul(height)
            .is_some_and(|count| count >= 512 * 512)
}

impl GpuContext {
    fn apply_filter_gpu(
        &self,
        filter: &Filter,
        width: usize,
        height: usize,
        pixels: &[[f32; 4]],
    ) -> Option<Vec<[f32; 4]>> {
        if !self.available() {
            return None;
        }
        if width == 0 || height == 0 || width.checked_mul(height)? != pixels.len() {
            return None;
        }
        let (width, height) = (u32::try_from(width).ok()?, u32::try_from(height).ok()?);
        let effect = |mode, args: &[f32]| {
            if args.iter().any(|v| !v.is_finite()) {
                return None;
            }
            self.filter_pass(
                pixels,
                &[[0.0; 4]],
                [width, height, mode, 0, 0, 0, 0, 0],
                args,
            )
        };
        match *filter {
            Filter::GaussianBlur { radius } => {
                self.blur_pixels(pixels, width, height, radius, true)
            }
            Filter::BoxBlur { radius } => self.blur_pixels(pixels, width, height, radius, false),
            Filter::UnsharpMask {
                amount,
                radius,
                threshold,
            } => {
                if !amount.is_finite() || !threshold.is_finite() {
                    return None;
                }
                let blurred = self.blur_pixels(pixels, width, height, radius, true)?;
                self.filter_pass(
                    pixels,
                    &blurred,
                    [
                        width,
                        height,
                        2,
                        0,
                        (amount / 100.0).to_bits(),
                        (threshold / 255.0).to_bits(),
                        0,
                        0,
                    ],
                    &[0.0],
                )
            }
            Filter::SmartSharpen { amount, radius } => {
                if !amount.is_finite() {
                    return None;
                }
                let blurred = self.blur_pixels(pixels, width, height, radius, true)?;
                self.filter_pass(
                    pixels,
                    &blurred,
                    [width, height, 3, 0, (amount / 100.0).to_bits(), 0, 0, 0],
                    &[0.0],
                )
            }
            Filter::HighPass { radius } => {
                let blurred = self.blur_pixels(pixels, width, height, radius, true)?;
                self.filter_pass(pixels, &blurred, [width, height, 4, 0, 0, 0, 0, 0], &[0.0])
            }
            Filter::FindEdges => self.filter_pass(
                pixels,
                &[[0.0; 4]],
                [width, height, 5, 0, 0, 0, 0, 0],
                &[0.0],
            ),
            Filter::MotionBlur { angle, distance } => {
                if !distance.is_finite() || distance > 500.0 {
                    return None;
                }
                let (s, c) = angle.to_radians().sin_cos();
                effect(6, &[distance.round().max(1.0), c, s])
            }
            Filter::LensBlur { radius } => {
                // Bound the quadratic work per pixel to avoid GPU watchdogs.
                if !radius.is_finite() || radius > 32.0 {
                    return None;
                }
                let r = radius.round().max(0.0) as i32;
                let count = (-r..=r)
                    .flat_map(|y| (-r..=r).map(move |x| (x, y)))
                    .filter(|(x, y)| x * x + y * y <= r * r)
                    .count();
                effect(7, &[r as f32, 1.0 / count as f32])
            }
            Filter::AddNoise { amount, monochrome } => effect(
                8,
                &[amount / 100.0 * 0.6, if monochrome { 1.0 } else { 0.0 }],
            ),
            Filter::ReduceNoise { strength, detail } => effect(
                9,
                &[
                    (0.02 + strength / 10.0 * 0.25) * (1.0 - detail / 100.0 * 0.6),
                    strength,
                ],
            ),
            Filter::Emboss {
                angle,
                height,
                amount,
            } => {
                let (s, c) = angle.to_radians().sin_cos();
                effect(10, &[c * height, -s * height, amount / 100.0])
            }
            Filter::Pinch { amount } => effect(11, &[amount / 100.0]),
            Filter::Twirl { angle } => effect(12, &[angle.to_radians()]),
            Filter::Wave {
                amplitude,
                wavelength,
            } => effect(13, &[amplitude, wavelength.max(2.0)]),
            Filter::LensCorrection {
                distortion,
                vignette,
            } => effect(14, &[distortion / 100.0 * 0.5, vignette / 100.0]),
            Filter::LensProfile {
                a,
                b,
                c,
                k1,
                k2,
                k3,
                scale,
                distortion,
                vignette,
            } => {
                let kd = distortion / 100.0;
                let kv = vignette / 100.0;
                effect(
                    15,
                    &[
                        a * kd,
                        b * kd,
                        c * kd,
                        k1 * kv,
                        k2 * kv,
                        k3 * kv,
                        if scale > 0.0 { scale } else { 1.0 },
                    ],
                )
            }
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use emulsion_raster::{Raster, color};

    #[test]
    fn automatic_filter_policy_uses_measured_wins_only() {
        let reduce = Filter::ReduceNoise {
            strength: 6.0,
            detail: 40.0,
        };
        assert!(!prefer_gpu_filter(&reduce, 511, 512));
        assert!(prefer_gpu_filter(&reduce, 512, 512));
        assert!(prefer_gpu_filter(&reduce, 2048, 2048));
        assert!(!prefer_gpu_filter(&reduce, usize::MAX, 2));
        assert!(!prefer_gpu_filter(
            &Filter::ReduceNoise {
                strength: 0.0,
                detail: 40.0
            },
            2048,
            2048
        ));
        assert!(!prefer_gpu_filter(
            &Filter::GaussianBlur { radius: 20.0 },
            2048,
            2048
        ));
        assert!(!prefer_gpu_filter(
            &Filter::Twirl { angle: 87.0 },
            2048,
            2048
        ));
        assert!(!prefer_gpu_filter(
            &Filter::AddNoise {
                amount: 20.0,
                monochrome: false
            },
            2048,
            2048
        ));
    }

    #[test]
    #[ignore = "manual release-mode GPU/CPU latency benchmark"]
    fn benchmark_filters() {
        use std::{hint::black_box, time::Instant};
        let gpu = crate::test_gpu().expect("benchmark requires a GPU");
        let filters = [
            Filter::GaussianBlur { radius: 5.0 },
            Filter::GaussianBlur { radius: 20.0 },
            Filter::Twirl { angle: 87.0 },
            Filter::AddNoise {
                amount: 20.0,
                monochrome: false,
            },
            Filter::ReduceNoise {
                strength: 6.0,
                detail: 40.0,
            },
        ];
        for side in [512usize, 2048] {
            let pixels: Vec<[f32; 4]> = (0..side * side)
                .map(|i| {
                    let a = [0.0, 0.2, 0.7, 1.0][i % 4];
                    [
                        (i % 23) as f32 / 23.0 * a,
                        (i % 31) as f32 / 31.0 * a,
                        (i % 17) as f32 / 17.0 * a,
                        a,
                    ]
                })
                .collect();
            for filter in &filters {
                // Both backends receive identical dense pixels/dimensions.
                // Exclude the CPU ownership clone from timing. GPU timings
                // include upload, dispatch, synchronization, and readback.
                black_box(
                    gpu.apply_filter_gpu(filter, side, side, &pixels)
                        .expect("GPU warmup"),
                );
                black_box(
                    emulsion_filters::apply_pixels_cpu(filter, side, side, pixels.clone()).unwrap(),
                );
                let mut cpu_times = Vec::new();
                let mut gpu_times = Vec::new();
                for _ in 0..3 {
                    let owned = pixels.clone();
                    let start = Instant::now();
                    black_box(
                        emulsion_filters::apply_pixels_cpu(filter, side, side, owned).unwrap(),
                    );
                    cpu_times.push(start.elapsed().as_secs_f64() * 1000.0);
                    let start = Instant::now();
                    black_box(
                        gpu.apply_filter_gpu(filter, side, side, &pixels)
                            .expect("GPU benchmark dispatch"),
                    );
                    gpu_times.push(start.elapsed().as_secs_f64() * 1000.0);
                }
                cpu_times.sort_by(f64::total_cmp);
                gpu_times.sort_by(f64::total_cmp);
                println!(
                    "FILTER_BENCH {side}x{side} {filter:?}: CPU {:.3}ms GPU {:.3}ms GPU/CPU {:.3}",
                    cpu_times[1],
                    gpu_times[1],
                    gpu_times[1] / cpu_times[1]
                );
            }
        }
    }

    #[test]
    fn gpu_filters_match_cpu_including_transparency_and_padded_edges() {
        let Some(gpu) = crate::test_gpu() else {
            return;
        };
        let filters = [
            Filter::GaussianBlur { radius: 2.3 },
            Filter::BoxBlur { radius: 2.0 },
            Filter::UnsharpMask {
                amount: 130.0,
                radius: 1.5,
                threshold: 4.0,
            },
            Filter::SmartSharpen {
                amount: 80.0,
                radius: 1.2,
            },
            Filter::HighPass { radius: 2.5 },
            Filter::FindEdges,
            Filter::MotionBlur {
                angle: 31.0,
                distance: 4.2,
            },
            Filter::LensBlur { radius: 3.0 },
            Filter::AddNoise {
                amount: 18.0,
                monochrome: true,
            },
            Filter::AddNoise {
                amount: 31.0,
                monochrome: false,
            },
            Filter::ReduceNoise {
                strength: 7.0,
                detail: 40.0,
            },
            Filter::Emboss {
                angle: 135.0,
                height: 1.5,
                amount: 80.0,
            },
            Filter::Pinch { amount: 35.0 },
            Filter::Pinch { amount: -25.0 },
            Filter::Twirl { angle: 87.0 },
            Filter::Wave {
                amplitude: 2.0,
                wavelength: 11.0,
            },
            Filter::LensCorrection {
                distortion: -14.0,
                vignette: 12.0,
            },
            Filter::LensProfile {
                a: 0.02,
                b: -0.01,
                c: 0.03,
                k1: 0.08,
                k2: 0.02,
                k3: 0.0,
                scale: 1.2,
                distortion: 75.0,
                vignette: 80.0,
            },
            Filter::LensProfile {
                a: 0.0,
                b: 0.0,
                c: 0.0,
                k1: 0.08,
                k2: 0.02,
                k3: 0.0,
                scale: 1.0,
                distortion: 0.0,
                vignette: 60.0,
            },
        ];
        // Non-workgroup-aligned sizes, degenerate axes, opaque and transparent
        // pixels exercise shader bounds checks and unpremultiplication.
        for (width, height) in [(1, 1), (1, 9), (17, 13)] {
            let source_pixels: Vec<[u16; 4]> = (0..width * height)
                .map(|i| {
                    let alpha = [0.0, 0.2, 0.7, 1.0][i % 4];
                    color::f_to_px([
                        ((i * 7 % 23) as f32 / 23.0) * alpha,
                        ((i * 11 % 31) as f32 / 31.0) * alpha,
                        ((i * 3 % 17) as f32 / 17.0) * alpha,
                        alpha,
                    ])
                })
                .collect();
            let source = Raster::from_pixels(width as u32, height as u32, [0; 4], &source_pixels);
            for filter in &filters {
                let (cpu, offset) =
                    emulsion_filters::apply_stack(&source, std::slice::from_ref(filter));
                let spread = filter.spread() as usize;
                assert_eq!(offset, (-(spread as i32), -(spread as i32)));
                let (w, h) = (width + 2 * spread, height + 2 * spread);
                let mut padded = vec![[0.0; 4]; w * h];
                for y in 0..height {
                    for x in 0..width {
                        padded[(y + spread) * w + x + spread] =
                            color::px_to_f(source_pixels[y * width + x]);
                    }
                }
                let actual = gpu
                    .apply_filter_gpu(filter, w, h, &padded)
                    .expect("supported filter must dispatch on GPU");
                let expected = cpu.to_pixels();
                assert_eq!(actual.len(), expected.len());
                for (i, (actual, expected)) in actual.into_iter().zip(expected).enumerate() {
                    for (channel, (a, b)) in
                        color::f_to_px(actual).into_iter().zip(expected).enumerate()
                    {
                        assert!(
                            a.abs_diff(b) <= 4,
                            "{filter:?}, {width}x{height}, pixel {i}, channel {channel}: GPU {a}, CPU {b}"
                        );
                    }
                }
            }
        }
    }

    #[test]
    fn expensive_or_invalid_filters_decline_for_cpu_fallback() {
        let Some(gpu) = crate::test_gpu() else {
            return;
        };
        assert!(
            gpu.apply_filter_gpu(&Filter::LensBlur { radius: 100.0 }, 1, 1, &[[1.0; 4]])
                .is_none()
        );
        assert!(
            gpu.apply_filter_gpu(
                &Filter::GaussianBlur { radius: f32::NAN },
                1,
                1,
                &[[1.0; 4]]
            )
            .is_none()
        );
        assert!(
            gpu.apply_filter_gpu(&Filter::FindEdges, 2, 2, &[[1.0; 4]])
                .is_none()
        );
        assert!(
            gpu.apply_filter_gpu(&Filter::FindEdges, 0, 0, &[])
                .is_none()
        );
    }
}
