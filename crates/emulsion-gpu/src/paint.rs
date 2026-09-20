//! Batched final compositing for ordinary brush, eraser, and mask strokes.
//! Dab generation, wet pigment pickup, taper replay, and history stay on CPU.

use crate::GpuContext;
use emulsion_raster::paint::BrushBlend;
use emulsion_raster::paint_accel::{PaintBatch, PaintCompositor};
use emulsion_raster::{TILE, TILE_PX};

impl PaintCompositor for GpuContext {
    fn composite_paint(&self, batch: &PaintBatch<'_>) -> Option<Vec<Vec<[u16; 4]>>> {
        if !self.available()
            || batch.tiles.is_empty()
            || batch.tiles.len() > 32
            || batch.tiles.iter().any(|(_, tile)| tile.len() != TILE_PX)
        {
            return None;
        }
        if !batch.opacity.is_finite() {
            return None;
        }
        let dense_threshold = batch.tiles.len() * TILE_PX / 2;
        let active = batch
            .tiles
            .iter()
            .flat_map(|(_, pixels)| pixels.iter())
            .filter(|pixel| pixel[4] > 0.0)
            .take(dense_threshold)
            .count();
        // Dense strokes avoid the index and scatter overhead. Sparse strokes
        // transfer only covered pixels, which can be a tiny fraction of a tile.
        let sparse = active < dense_threshold;
        let count = if sparse {
            active
        } else {
            batch.tiles.len() * TILE_PX
        };
        // Rebuild sparse output from the stroke's immutable base, never the
        // previous preview: taper replay can remove previously painted pixels.
        let mut output: Vec<Vec<[u16; 4]>> = if sparse {
            batch
                .tiles
                .iter()
                .map(|(coord, _)| {
                    batch
                        .base
                        .base_tile(*coord)
                        .map_or_else(|| vec![batch.base.fill(); TILE_PX], |tile| tile.to_vec())
                })
                .collect()
        } else {
            Vec::new()
        };
        let mut indices = Vec::<u32>::with_capacity(if sparse { active } else { 0 });
        let mut source = Vec::<[u32; 2]>::with_capacity(count);
        let mut paint = Vec::<[f32; 5]>::with_capacity(if sparse { count } else { 0 });
        // Preserve the original contiguous dense upload; repacking/scattering
        // every painted pixel costs more than omitting one unused channel saves.
        let mut dense_paint = Vec::<[f32; 6]>::with_capacity(if sparse { 0 } else { count });
        let mut clips = Vec::<f32>::with_capacity(count);
        for (tile_index, &(coord, accumulated)) in batch.tiles.iter().enumerate() {
            let base_tile = batch.base.base_tile(coord);
            for (i, pixel) in accumulated.iter().enumerate() {
                if sparse && !pixel[4].is_finite() {
                    return None;
                }
                if sparse && pixel[4] <= 0.0 {
                    continue;
                }
                if sparse {
                    let pigment: [f32; 5] = pixel[..5].try_into().ok()?;
                    if !pigment.iter().all(|value| value.is_finite()) {
                        return None;
                    }
                    paint.push(pigment);
                } else {
                    dense_paint.push(*pixel);
                }
                let clip = if pixel[4] > 0.0 {
                    batch.clip.map_or(1.0, |clip| {
                        clip(
                            coord.x * TILE as i32 + (i % TILE as usize) as i32,
                            coord.y * TILE as i32 + (i / TILE as usize) as i32,
                        )
                    })
                } else {
                    0.0
                };
                if sparse && !clip.is_finite() {
                    return None;
                }
                let base = base_tile.map_or(batch.base.fill(), |tile| tile[i]);
                if sparse {
                    indices.push((tile_index * TILE_PX + i) as u32);
                }
                source.push([
                    u32::from(base[0]) | (u32::from(base[1]) << 16),
                    u32::from(base[2]) | (u32::from(base[3]) << 16),
                ]);
                clips.push(clip);
            }
        }
        if count == 0 {
            return Some(output);
        }
        let flags = u32::from(batch.erase)
            | (u32::from(batch.alpha_lock) << 1)
            | (u32::from(batch.blend == BrushBlend::Behind) << 2)
            | (u32::from(batch.blend == BrushBlend::Multiply) << 3);
        let params = [
            count as u32,
            flags,
            batch.opacity.to_bits(),
            if sparse { 5 } else { 6 },
        ];
        let paint_bytes: &[u8] = if sparse {
            bytemuck::cast_slice(&paint)
        } else {
            bytemuck::cast_slice(&dense_paint)
        };
        let bytes = self
            .run_with_reuse(
                "paint-composite",
                include_str!("paint.wgsl"),
                &[
                    bytemuck::cast_slice(&params),
                    bytemuck::cast_slice(&source),
                    paint_bytes,
                    bytemuck::cast_slice(&clips),
                ],
                count * 8,
                (count as u32).div_ceil(64),
                // Same-executable A/B tests found repeatable wins for dense
                // four-tile batches; sparse and larger jobs stayed faster or
                // indistinguishable with fresh buffers.
                !sparse && batch.tiles.len() == 4,
            )
            .ok()?;
        if bytes.len() != count * 8 {
            return None;
        }
        if !sparse {
            return Some(
                bytes
                    .as_chunks::<{ TILE_PX * 8 }>()
                    .0
                    .iter()
                    .map(|tile| {
                        tile.as_chunks::<8>()
                            .0
                            .iter()
                            .map(|pixel| {
                                std::array::from_fn(|i| {
                                    u16::from_le_bytes([pixel[i * 2], pixel[i * 2 + 1]])
                                })
                            })
                            .collect()
                    })
                    .collect(),
            );
        }
        for (&index, pixel) in indices.iter().zip(bytes.as_chunks::<8>().0) {
            let index = index as usize;
            output[index / TILE_PX][index % TILE_PX] =
                std::array::from_fn(|i| u16::from_le_bytes([pixel[i * 2], pixel[i * 2 + 1]]));
        }
        Some(output)
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use emulsion_raster::blend::{BlendSpace, blend_px};
    use emulsion_raster::color::{f_to_px, px_to_f};
    use emulsion_raster::paint::Clip;
    use emulsion_raster::{BlendMode, Raster, TileCoord};
    use std::sync::Arc;

    /// Run with `cargo test -p emulsion-gpu --release benchmark_stroke_compositing
    /// -- --ignored --nocapture`. Includes upload, packing, dispatch, readback,
    /// and immutable-tile replacement; excludes identical CPU dab generation.
    #[test]
    #[ignore = "manual release-mode GPU/CPU latency benchmark"]
    fn benchmark_stroke_compositing() {
        use emulsion_raster::paint::{Brush, Ink, Stroke};
        use std::sync::atomic::{AtomicUsize, Ordering};
        use std::time::{Duration, Instant};

        let Some(gpu) = crate::test_gpu() else {
            return;
        };
        struct Measured<'a> {
            gpu: &'a GpuContext,
            calls: AtomicUsize,
        }
        impl PaintCompositor for Measured<'_> {
            fn composite_paint(&self, batch: &PaintBatch<'_>) -> Option<Vec<Vec<[u16; 4]>>> {
                self.calls.fetch_add(1, Ordering::Relaxed);
                Some(
                    self.gpu
                        .composite_paint(batch)
                        .expect("benchmark must execute GPU"),
                )
            }
        }
        let measured = Measured {
            gpu: &gpu,
            calls: AtomicUsize::new(0),
        };
        for (label, dimension, brush_size) in [
            ("sparse-4-tiles", 512, 40.0),
            ("dense-4-tiles", 512, 1000.0),
            ("dense-16-tiles", 1024, 1000.0),
        ] {
            let base = Arc::new(Raster::empty(
                dimension,
                dimension,
                f_to_px([0.1, 0.2, 0.3, 0.6]),
            ));
            let make_stroke = || {
                let mut stroke = Stroke::new(
                    base.clone(),
                    Brush {
                        size: brush_size,
                        hardness: 1.0,
                        opacity: 0.43,
                        ..Brush::default()
                    },
                    Ink::Color([0.5, 0.1, 0.2, 0.7]),
                    None,
                );
                stroke.point(dimension as f32 / 2.0, dimension as f32 / 2.0);
                stroke.finish();
                stroke
            };
            make_stroke().render_with_compositor(&base, Some(&measured));
            let mut dab_time = Duration::ZERO;
            let mut cpu_time = Duration::ZERO;
            let mut gpu_time = Duration::ZERO;
            const ITERATIONS: u32 = 6;
            for _ in 0..ITERATIONS {
                let started = Instant::now();
                let mut cpu = make_stroke();
                dab_time += started.elapsed();
                let mut accelerated = make_stroke();
                let started = Instant::now();
                let (expected, expected_dirty) = cpu.render_with_compositor(&base, None);
                cpu_time += started.elapsed();
                let started = Instant::now();
                let (actual, actual_dirty) =
                    accelerated.render_with_compositor(&base, Some(&measured));
                gpu_time += started.elapsed();
                assert_eq!(actual_dirty, expected_dirty);
                for (actual, expected) in actual.to_pixels().into_iter().zip(expected.to_pixels()) {
                    for (actual, expected) in actual.into_iter().zip(expected) {
                        assert!(
                            actual.abs_diff(expected) <= 1,
                            "stroke parity: {actual} vs {expected}"
                        );
                    }
                }
            }
            assert_eq!(
                measured.calls.swap(0, Ordering::Relaxed),
                ITERATIONS as usize + 1
            );
            eprintln!(
                "{label}: CPU dabs {:.3} ms, CPU composition {:.3} ms, GPU composition {:.3} ms, GPU/CPU {:.2}",
                dab_time.as_secs_f64() * 1000.0 / f64::from(ITERATIONS),
                cpu_time.as_secs_f64() * 1000.0 / f64::from(ITERATIONS),
                gpu_time.as_secs_f64() * 1000.0 / f64::from(ITERATIONS),
                gpu_time.as_secs_f64() / cpu_time.as_secs_f64()
            );
        }
    }

    /// Compare buffer allocation with reuse in the same binary and device.
    /// Run alone in release mode; includes packing, transfers, readback, and
    /// immutable tile replacement, but excludes dab generation and parity checks.
    #[test]
    #[ignore = "manual release-mode GPU buffer reuse A/B benchmark"]
    fn benchmark_stroke_buffer_reuse() {
        use emulsion_raster::paint::{Brush, Ink, Stroke};
        use std::time::{Duration, Instant};

        let Some(gpu) = crate::test_gpu() else {
            return;
        };
        struct RequiredGpu<'a>(&'a GpuContext);
        impl PaintCompositor for RequiredGpu<'_> {
            fn composite_paint(&self, batch: &PaintBatch<'_>) -> Option<Vec<Vec<[u16; 4]>>> {
                Some(self.0.composite_paint(batch).expect("GPU must execute"))
            }
        }
        let required_gpu = RequiredGpu(&gpu);
        const ITERATIONS: usize = 12;
        for (label, dimension, brush_size) in [
            ("sparse-4-tiles", 512, 40.0),
            ("dense-4-tiles", 512, 1000.0),
            ("dense-16-tiles", 1024, 1000.0),
        ] {
            let base = Arc::new(Raster::empty(
                dimension,
                dimension,
                f_to_px([0.1, 0.2, 0.3, 0.6]),
            ));
            let make_stroke = |sample: usize| {
                // Change both the dirty footprint and source data between frames.
                // Repeating three sizes allows every required capacity to warm.
                let offset = (sample % 3) as f32 - 1.0;
                let mut stroke = Stroke::new(
                    base.clone(),
                    Brush {
                        size: brush_size,
                        hardness: 1.0,
                        opacity: 0.43,
                        ..Brush::default()
                    },
                    Ink::Color([0.5 + offset * 0.03, 0.1, 0.2, 0.7]),
                    None,
                );
                stroke.point(
                    dimension as f32 / 2.0 + offset * 2.0,
                    dimension as f32 / 2.0 - offset * 3.0,
                );
                stroke.finish();
                stroke
            };
            for reuse in [false, true] {
                gpu.set_reuse_buffers(reuse);
                for sample in 0..6 {
                    make_stroke(sample).render_with_compositor(&base, Some(&required_gpu));
                }
            }
            // Modes: CPU, fresh GPU buffers, reused GPU buffers. Rotate their
            // order to avoid consistently favoring one with a hotter CPU/device.
            let mut elapsed = [Duration::ZERO; 3];
            let mut allocations = [0_u64; 3];
            let mut latencies = [Vec::new(), Vec::new(), Vec::new()];
            for sample in 0..ITERATIONS {
                let mut outputs = [None, None, None];
                for step in 0..3 {
                    let mode = (sample + step) % 3;
                    let mut stroke = make_stroke(sample);
                    gpu.set_reuse_buffers(mode == 2);
                    let before_allocations = gpu.scratch_allocation_count();
                    let before_dispatches = gpu.dispatch_count();
                    let compositor = (mode != 0).then_some(&required_gpu as &dyn PaintCompositor);
                    let start = Instant::now();
                    let output = stroke.render_with_compositor(&base, compositor);
                    let duration = start.elapsed();
                    elapsed[mode] += duration;
                    latencies[mode].push(duration.as_secs_f64() * 1000.0);
                    allocations[mode] += gpu.scratch_allocation_count() - before_allocations;
                    assert_eq!(
                        gpu.dispatch_count() - before_dispatches,
                        u64::from(mode != 0),
                        "{label}: mode {mode} must execute its intended backend"
                    );
                    outputs[mode] = Some(output);
                }
                let (expected, expected_dirty) = outputs[0].as_ref().unwrap();
                let expected_pixels = expected.to_pixels();
                for (mode, output) in outputs.iter().enumerate().skip(1) {
                    let (actual, dirty) = output.as_ref().unwrap();
                    assert_eq!(dirty, expected_dirty, "{label}: mode {mode} dirty tiles");
                    let actual_pixels = actual.to_pixels();
                    assert_eq!(actual_pixels.len(), expected_pixels.len());
                    for (actual, expected) in actual_pixels.iter().zip(&expected_pixels) {
                        for (actual, expected) in actual.iter().zip(expected) {
                            assert!(
                                actual.abs_diff(*expected) <= 1,
                                "{label}: mode {mode} sample {sample}: {actual} vs {expected}"
                            );
                        }
                    }
                }
            }
            assert_eq!(allocations[0], 0);
            assert!(allocations[1] >= ITERATIONS as u64);
            assert_eq!(allocations[2], 0, "all reusable capacities were warmed");
            for samples in &mut latencies {
                samples.sort_by(f64::total_cmp);
            }
            let means = elapsed.map(|d| d.as_secs_f64() * 1000.0 / ITERATIONS as f64);
            let medians = latencies.map(|v| (v[ITERATIONS / 2 - 1] + v[ITERATIONS / 2]) / 2.0);
            eprintln!(
                "{label} changing strokes ({ITERATIONS} iterations): mean CPU {:.3} ms, fresh GPU {:.3} ms, reused GPU {:.3} ms; median CPU {:.3} ms, fresh GPU {:.3} ms, reused GPU {:.3} ms; allocations fresh {}, reused {}; reuse/fresh {:.2}, reuse/CPU {:.2}",
                means[0],
                means[1],
                means[2],
                medians[0],
                medians[1],
                medians[2],
                allocations[1],
                allocations[2],
                means[2] / means[1],
                means[2] / means[0]
            );
        }
        gpu.reset_reuse_buffers();
    }

    #[test]
    fn sparse_paint_and_zero_coverage_restore_stroke_base() {
        let Some(gpu) = crate::test_gpu() else {
            return;
        };
        let fill = [1234, 5678, 9012, 40000];
        let coord = TileCoord { x: 1, y: 0 };
        let original: Vec<_> = (0..TILE_PX)
            .map(|i| [i as u16, 1200, 3200, 65535])
            .collect();
        let base =
            Raster::empty(TILE * 2, TILE, fill).with_changes(vec![(coord, Some(original.clone()))]);
        let mut accumulated = vec![[0.0; 6]; TILE_PX];
        accumulated[17] = [0.25, 0.0, 0.0, 0.5, 1.0, 999.0];
        let empty = vec![[0.0; 6]; TILE_PX];
        let render = |accumulated: &[[f32; 6]]| {
            gpu.composite_paint(&PaintBatch {
                base: &base,
                tiles: &[(TileCoord { x: 0, y: 0 }, &empty), (coord, accumulated)],
                clip: None,
                opacity: 1.0,
                blend: BrushBlend::Normal,
                erase: false,
                alpha_lock: false,
            })
        };
        let output = render(&accumulated).expect("sparse GPU composition");
        assert_eq!(output[0], vec![fill; TILE_PX]);
        for i in 0..TILE_PX {
            let expected = if i == 17 {
                f_to_px(blend_px(
                    BlendMode::Normal,
                    BlendSpace::Linear,
                    px_to_f(original[i]),
                    [0.25, 0.0, 0.0, 0.5],
                    0.0,
                ))
            } else {
                original[i]
            };
            for (actual, expected) in output[1][i].into_iter().zip(expected) {
                assert!(actual.abs_diff(expected) <= 1);
            }
        }
        // Simulate taper replay removing previously painted coverage. Returning
        // a previous preview here would incorrectly retain pixel 17's paint.
        let restored = render(&empty).expect("zero coverage restores base");
        assert_eq!(restored[0], vec![fill; TILE_PX]);
        assert_eq!(restored[1], original);
        accumulated[17][4] = f32::NAN;
        assert!(render(&accumulated).is_none());
        accumulated[17][4] = 1.0;
        accumulated[17][0] = f32::INFINITY;
        assert!(render(&accumulated).is_none());
    }

    #[test]
    fn paint_matches_cpu_reference_with_clipping_and_alpha_lock() {
        let Some(gpu) = crate::test_gpu() else {
            return;
        };
        let pixels: Vec<_> = (0..TILE_PX)
            .map(|i| {
                let alpha = (i % 5) as f32 / 4.0;
                f_to_px([alpha * 0.2, alpha * 0.4, alpha * 0.7, alpha])
            })
            .collect();
        let base = Raster::empty(TILE, TILE, [0; 4])
            .with_changes(vec![(TileCoord { x: 0, y: 0 }, Some(pixels.clone()))]);
        let accumulated: Vec<_> = (0..TILE_PX)
            .map(|i| {
                let coverage = [0.0, 0.00001, 0.25, 0.75, 1.0, 2.0][i % 6];
                [
                    0.45 * coverage,
                    0.1 * coverage,
                    0.2 * coverage,
                    0.6 * coverage,
                    coverage,
                    coverage * 2.0,
                ]
            })
            .collect();
        let tiles = [(TileCoord { x: 0, y: 0 }, accumulated.as_slice())];
        let clip: Clip = Arc::new(|x, y| ((x + y) % 4) as f32 / 3.0);
        for (blend, erase, alpha_lock) in [
            (BrushBlend::Normal, false, false),
            (BrushBlend::Behind, false, false),
            (BrushBlend::Normal, false, true),
            (BrushBlend::Behind, false, true),
            (BrushBlend::Multiply, false, false),
            (BrushBlend::Multiply, false, true),
            (BrushBlend::Normal, true, false),
            (BrushBlend::Normal, true, true),
        ] {
            let batch = PaintBatch {
                base: &base,
                tiles: &tiles,
                clip: Some(&clip),
                opacity: 0.43,
                blend,
                erase,
                alpha_lock,
            };
            let output = gpu.composite_paint(&batch).expect("GPU paint dispatch");
            assert_eq!(output.len(), 1);
            let mode = if blend == BrushBlend::Multiply {
                BlendMode::Multiply
            } else {
                BlendMode::Normal
            };
            for (i, (&original, p)) in pixels.iter().zip(&accumulated).enumerate() {
                let base = px_to_f(original);
                let mut k = p[4].min(1.0)
                    * batch.opacity
                    * clip((i % TILE as usize) as i32, (i / TILE as usize) as i32);
                let expected = if p[4] <= 0.0 || k <= 0.0 || (alpha_lock && base[3] <= 0.0) {
                    original
                } else if erase {
                    if alpha_lock {
                        original
                    } else {
                        f_to_px(base.map(|v| v * (1.0 - k)))
                    }
                } else {
                    if blend == BrushBlend::Behind {
                        k *= 1.0 - base[3].min(1.0);
                    }
                    let ink = [p[0], p[1], p[2], p[3]].map(|v| v / p[4].max(1e-6) * k);
                    if alpha_lock {
                        let opaque = [base[0] / base[3], base[1] / base[3], base[2] / base[3], 1.0];
                        let mixed = blend_px(mode, BlendSpace::Linear, opaque, ink, 0.0);
                        f_to_px([
                            mixed[0] * base[3],
                            mixed[1] * base[3],
                            mixed[2] * base[3],
                            base[3],
                        ])
                    } else {
                        f_to_px(blend_px(mode, BlendSpace::Linear, base, ink, 0.0))
                    }
                };
                for (actual, expected) in output[0][i].into_iter().zip(expected) {
                    assert!(
                        actual.abs_diff(expected) <= 1,
                        "pixel {i}: {actual} vs {expected}, {blend:?}, erase={erase}, alpha_lock={alpha_lock}"
                    );
                }
            }
        }
    }
}
