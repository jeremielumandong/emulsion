//! Compare the experimental kernel against the application's actual brush.
use crate::persistent_paint::{Dab, PersistentPaint};
use emulsion_raster::Raster;
use emulsion_raster::paint::{Brush, Ink, Stroke};
use std::sync::Arc;

#[test]
fn persistent_dabs_match_application_brush() {
    let Some(gpu) = crate::test_gpu() else { return };
    for (radius, hardness, center) in [
        (0.5, 1.0, [31.0, 31.0]),
        (0.8, 0.2, [30.3, 31.7]),
        (1.7, 0.99, [30.3, 31.7]),
        (12.0, 0.4, [30.3, 31.7]),
        (25.0, 1.0, [4.2, 5.8]),
    ] {
        for fill in [[0; 4], [7000, 10000, 15000, 30000]] {
            let base = Arc::new(Raster::empty(64, 64, fill));
            let color = [0.3, 0.1, 0.2, 0.6];
            let mut reference = Stroke::new(
                base.clone(),
                Brush {
                    size: radius * 2.0,
                    hardness,
                    flow: 0.37,
                    opacity: 0.63,
                    ..Brush::default()
                },
                Ink::Color(color),
                None,
            );
            reference.point(center[0], center[1]);
            reference.finish();
            let expected = reference.render_with_compositor(&base, None).0.to_pixels();
            let mut session =
                PersistentPaint::new(gpu.clone(), 64, 64, &base.to_pixels(), 0.63).unwrap();
            session
                .append(&[Dab {
                    center,
                    radius,
                    hardness,
                    flow: 0.37,
                    color,
                }])
                .unwrap();
            let actual = session.preview().unwrap();
            assert_eq!(actual.len(), expected.len());
            for (index, (actual, expected)) in actual.iter().zip(&expected).enumerate() {
                for channel in 0..4 {
                    assert!(
                        actual[channel].abs_diff(expected[channel]) <= 2,
                        "radius {radius}, hardness {hardness}, pixel {index}, channel {channel}: {} != {}",
                        actual[channel],
                        expected[channel]
                    );
                }
            }
        }
    }
}

/// Real Stroke CPU and existing GPU compositor versus the persistent prototype.
/// Includes rasterization and each preview's CPU result, excludes GPUI display.
#[test]
#[ignore = "manual release-mode application brush comparison"]
fn benchmark_persistent_application_brush() {
    use std::time::Instant;
    let Some(gpu) = crate::test_gpu() else { return };
    for (label, size, spacing) in [("small", 40.0, 0.1), ("large", 400.0, 0.02)] {
        let base = Arc::new(Raster::empty(512, 512, [7000, 10000, 15000, 30000]));
        let base_pixels = base.to_pixels();
        let color = [0.3, 0.1, 0.2, 0.6];
        let brush = Brush {
            size,
            spacing,
            hardness: 0.8,
            flow: 0.37,
            opacity: 0.63,
            ..Brush::default()
        };
        let mut totals = [0.0; 3];
        let mut startup = 0.0;
        for iteration in 0..7 {
            let mut results = Vec::new();
            for offset in 0..3 {
                let mode = (iteration + offset) % 3;
                let mut stroke = Stroke::new(base.clone(), brush, Ink::Color(color), None);
                let start = Instant::now();
                let mut persistent = (mode == 2).then(|| {
                    PersistentPaint::new(gpu.clone(), 512, 512, &base_pixels, 0.63).unwrap()
                });
                if mode == 2 && iteration > 0 {
                    startup += start.elapsed().as_secs_f64();
                }
                let mut current = (*base).clone();
                let mut actual = Vec::new();
                let start = Instant::now();
                for frame in 0..8 {
                    let x = 128.0 + frame as f32 * 32.0;
                    if let Some(session) = persistent.as_mut() {
                        let step = size * spacing;
                        let count = if frame == 0 {
                            1
                        } else {
                            (32.0 / step) as usize
                        };
                        let dabs: Vec<_> = (0..count)
                            .map(|i| Dab {
                                center: [x - (count - 1 - i) as f32 * step, 256.0],
                                radius: size / 2.0,
                                hardness: brush.hardness,
                                flow: brush.flow,
                                color,
                            })
                            .collect();
                        session.append(&dabs).unwrap();
                        actual = session.preview().unwrap();
                    } else {
                        stroke.point(x, 256.0);
                        current = stroke
                            .render_with_compositor(&current, (mode == 1).then_some(gpu.as_ref()))
                            .0;
                    }
                }
                let elapsed = start.elapsed().as_secs_f64();
                if iteration > 0 {
                    totals[mode] += elapsed;
                }
                if mode != 2 {
                    actual = current.to_pixels();
                }
                results.push(actual);
            }
            for result in &results[1..] {
                for (a, b) in results[0].iter().flatten().zip(result.iter().flatten()) {
                    assert!(a.abs_diff(*b) <= 2, "{label}: {a} != {b}");
                }
            }
        }
        eprintln!(
            "persistent-application-{label}: CPU {:.3} ms/update, existing GPU route {:.3} ms/update, persistent GPU {:.3} ms/update; persistent setup {:.3} ms/stroke",
            totals[0] * 1000.0 / 48.0,
            totals[1] * 1000.0 / 48.0,
            totals[2] * 1000.0 / 48.0,
            startup * 1000.0 / 6.0
        );
    }
}

#[test]
fn routed_stroke_matches_cpu_and_releases_session_on_drop() {
    use crate::brush_backend::BrushFactory;
    use emulsion_raster::paint_accel::PersistentFactory;
    let Some(gpu) = crate::test_gpu() else { return };
    let factory: Arc<dyn PersistentFactory> = Arc::new(BrushFactory::new(gpu));
    let base = Arc::new(Raster::empty(512, 512, [7000, 10000, 15000, 30000]));
    let brush = Brush {
        size: 400.0,
        spacing: 0.02,
        flow: 0.37,
        opacity: 0.63,
        ..Brush::default()
    };
    let ink = Ink::Color([0.3, 0.1, 0.2, 0.6]);
    let mut accelerated = Stroke::new_with_persistent(
        base.clone(),
        brush,
        ink.clone(),
        None,
        Some(factory.clone()),
    );
    let mut cpu = Stroke::new_with_persistent(base.clone(), brush, ink.clone(), None, None);
    let mut actual = (*base).clone();
    let mut expected = (*base).clone();
    for x in [128.0, 160.0, 192.0, 224.0] {
        accelerated.point(x, 256.0);
        cpu.point(x, 256.0);
        actual = accelerated.render(&actual).0;
        expected = cpu.render_with_compositor(&expected, None).0;
        assert!(accelerated.uses_persistent());
        for (a, b) in actual
            .to_pixels()
            .iter()
            .flatten()
            .zip(expected.to_pixels().iter().flatten())
        {
            assert!(a.abs_diff(*b) <= 2, "{a} != {b}");
        }
    }
    // Another stroke must decline while this factory retains its one session.
    assert!(factory.start(&base, 1.0).is_none());
    // Read-only CPU consumers (healing) still see stroke coverage.
    assert_eq!(
        accelerated.coverage().to_pixels(),
        cpu.coverage().to_pixels()
    );
    // QuickShape removes old marks and replays correctly through the CPU path.
    let shape = [(30.0, 30.0), (45.0, 40.0)];
    accelerated.replay(&shape, 1.0);
    cpu.replay(&shape, 1.0);
    actual = accelerated.render(&actual).0;
    expected = cpu.render_with_compositor(&expected, None).0;
    assert_eq!(actual.to_pixels(), expected.to_pixels());
    drop(accelerated);
    assert!(factory.start(&base, 1.0).is_some());
}

/// End-to-end raster route including lazy setup, CPU geometry and tile updates.
#[test]
#[ignore = "manual release-mode integrated brush latency"]
fn benchmark_routed_persistent_brush() {
    use crate::brush_backend::BrushFactory;
    use std::time::Instant;
    let Some(gpu) = crate::test_gpu() else { return };
    let factory = Arc::new(BrushFactory::new(gpu));
    for size in [40.0, 400.0] {
        let mut elapsed = [0.0; 2];
        for iteration in 0..7 {
            let base = Arc::new(Raster::empty(512, 512, [7000, 10000, 15000, 30000]));
            let mut results = Vec::new();
            for offset in 0..2 {
                let mode = (iteration + offset) % 2;
                let start = Instant::now();
                let mut stroke = Stroke::new_with_persistent(
                    base.clone(),
                    Brush {
                        size,
                        spacing: 0.02,
                        flow: 0.37,
                        opacity: 0.63,
                        ..Brush::default()
                    },
                    Ink::Color([0.3, 0.1, 0.2, 0.6]),
                    None,
                    (mode == 1).then(|| {
                        factory.clone() as Arc<dyn emulsion_raster::paint_accel::PersistentFactory>
                    }),
                );
                let mut current = (*base).clone();
                for frame in 0..8 {
                    stroke.point(128.0 + frame as f32 * 32.0, 256.0);
                    current = stroke.render(&current).0;
                }
                stroke.finish();
                current = stroke.render(&current).0;
                if iteration > 0 {
                    elapsed[mode] += start.elapsed().as_secs_f64();
                }
                assert_eq!(stroke.uses_persistent(), mode == 1 && size >= 400.0);
                results.push(current.to_pixels());
            }
            for (a, b) in results[0].iter().flatten().zip(results[1].iter().flatten()) {
                assert!(a.abs_diff(*b) <= 2);
            }
        }
        eprintln!(
            "routed {size}px including setup and commit: CPU {:.3} ms/stroke, routed {:.3} ms/stroke",
            elapsed[0] * 1000.0 / 6.0,
            elapsed[1] * 1000.0 / 6.0
        );
    }
}
