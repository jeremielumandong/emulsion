//! Exercise production accelerator registration in a fresh process.
//!
//! `EMULSION_GPU=software .../backend_smoke` requires a software compute adapter
//! and actual GPU dispatches. `EMULSION_GPU=cpu .../backend_smoke` verifies the
//! explicit CPU fallback. Other modes require a usable hardware adapter.

use anyhow::{Context, Result, ensure};
use emulsion_filters::{Filter, apply_stack};
use emulsion_raster::blend::BlendSpace;
use emulsion_raster::color::f_to_px;
use emulsion_raster::composite::{CompositeNode, CompositeTree, NodeContent, render_tile};
use emulsion_raster::paint::{Brush, Clip, Ink, Stroke};
use emulsion_raster::{Adjustment, BlendMode, Placement, Raster, TileCoord};
use std::sync::Arc;
use std::time::Instant;

fn node(id: u64, content: NodeContent) -> CompositeNode {
    CompositeNode {
        id,
        visible: true,
        opacity: 1.0,
        blend: BlendMode::Normal,
        blending: Default::default(),
        mask: None,
        clip_to: None,
        clip_rect: None,
        content,
    }
}

fn final_paint_stroke(base: Arc<Raster>) -> Stroke {
    let clip: Clip = Arc::new(|x, _| if x < 256 { 0.75 } else { 0.25 });
    let mut stroke = Stroke::new(
        base,
        Brush {
            size: 40.0,
            hardness: 1.0,
            flow: 0.37,
            opacity: 0.63,
            ..Brush::default()
        },
        Ink::Color([0.3, 0.1, 0.2, 0.6]),
        Some(clip),
    );
    // Cross four tile boundaries to exercise the actual installed final-paint
    // router, whose existing policy declines fewer than four changed tiles.
    stroke.point(256.0, 256.0);
    stroke.finish();
    stroke
}

fn persistent_stroke(base: Arc<Raster>) -> Stroke {
    let mut stroke = Stroke::new(
        base,
        Brush {
            size: 400.0,
            hardness: 0.8,
            flow: 0.37,
            opacity: 0.63,
            ..Brush::default()
        },
        Ink::Color([0.3, 0.1, 0.2, 0.6]),
        None,
    );
    stroke.point(8.0, 8.0);
    stroke.finish();
    stroke
}

fn check_rgba16(actual: &Raster, expected: &Raster, tolerance: u16, label: &str) -> Result<()> {
    ensure!(actual.width() == expected.width() && actual.height() == expected.height());
    let actual = actual.to_pixels();
    let expected = expected.to_pixels();
    ensure!(
        actual.len() == expected.len(),
        "{label}: output length mismatch"
    );
    ensure!(expected.iter().flatten().any(|&channel| channel != 0));
    for (index, (actual, expected)) in actual.iter().zip(&expected).enumerate() {
        for channel in 0..4 {
            ensure!(
                actual[channel].abs_diff(expected[channel]) <= tolerance,
                "{label} pixel {index}, channel {channel}: {} vs {}",
                actual[channel],
                expected[channel]
            );
        }
    }
    Ok(())
}

fn check_screen(gpu: &emulsion_gpu::GpuContext) -> Result<()> {
    let pixels: Vec<u8> = (0u32..65536)
        .flat_map(|index| (0xff00_0000 | index).to_le_bytes())
        .collect();
    let view = emulsion_gpu::screen::View {
        size: [2, 2],
        document: [256, 256],
        origin: [5.25, 7.25],
        dx: [1.0, 0.0],
        dy: [0.0, 1.0],
        wipe: None,
    };
    let tiles = [emulsion_gpu::screen::Tile {
        x: 0,
        y: 0,
        before: false,
        bgra: &pixels,
    }];
    let before = gpu.dispatch_count();
    let actual = gpu
        .sample_screen(&view, &tiles)
        .context("Published screen capability declined")?;
    ensure!(
        gpu.dispatch_count() > before,
        "Screen sampling did not dispatch after Ready"
    );
    ensure!(actual.len() == 4, "Screen output length mismatch");
    for (index, &actual) in actual.iter().enumerate() {
        let x = index as u32 % 2;
        let y = index as u32 / 2;
        let expected = 0xff00_0000 | ((7 + y) * 256 + 5 + x);
        ensure!(
            actual == [expected, 0],
            "Screen parity pixel {index}: {actual:?}"
        );
    }
    Ok(())
}

fn main() -> Result<()> {
    let mut require_vulkan = false;
    for argument in std::env::args().skip(1) {
        ensure!(
            argument == "--require-vulkan",
            "Unknown backend_smoke argument: {argument}"
        );
        require_vulkan = true;
    }
    let mode = std::env::var("EMULSION_GPU").unwrap_or_default();
    let cpu_requested = mode == "cpu";
    let screen_requested = matches!(mode.as_str(), "force" | "software");
    let automatic_hardware = !cpu_requested && !screen_requested;
    let brushes = std::env::var("EMULSION_GPU_BRUSHES").unwrap_or_default();
    let pixels: Vec<_> = (0..64 * 64)
        .map(|i| {
            let alpha = if i % 11 == 0 { 0.0 } else { 0.8 };
            f_to_px([
                (i % 64) as f32 / 63.0 * alpha,
                (i / 64) as f32 / 63.0 * alpha,
                0.2 * alpha,
                alpha,
            ])
        })
        .collect();
    let source = Arc::new(Raster::from_pixels(64, 64, [0; 4], &pixels));
    // One source plus a nonzero HueSaturation adjustment satisfies the existing
    // automatic compositor's worthwhile-work heuristic as well as forced mode.
    let tree = CompositeTree {
        knockout_background: None,
        width: 64,
        height: 64,
        space: BlendSpace::Linear,
        nodes: vec![
            node(
                1,
                NodeContent::Pixels {
                    raster: source.clone().into(),
                    placement: Placement::default(),
                },
            ),
            node(
                2,
                NodeContent::Adjust(Arc::new(
                    Adjustment::HueSaturation {
                        hue: 35.0,
                        saturation: 15.0,
                        lightness: -5.0,
                    }
                    .prepare(),
                )),
            ),
        ],
    };
    let (filter_source, stack) = if automatic_hardware {
        // Automatic routing currently selects only nonzero ReduceNoise at
        // 512x512 or larger. Preserve that policy and use a genuinely eligible
        // fixture instead of requiring the ineligible small LensBlur to run.
        let filter_pixels: Vec<_> = (0..512 * 512)
            .map(|index| pixels[(index / 512 % 64) * 64 + index % 64])
            .collect();
        (
            Arc::new(Raster::from_pixels(512, 512, [0; 4], &filter_pixels)),
            [Filter::ReduceNoise {
                strength: 7.0,
                detail: 40.0,
            }],
        )
    } else {
        // Keep the original forced/software/CPU fixture and its parity gates.
        (source.clone(), [Filter::LensBlur { radius: 4.0 }])
    };
    let tile = TileCoord { x: 0, y: 0 };

    // No hooks have been installed in this process: these are real CPU
    // baselines, including filter padding and RGBA16 conversion.
    ensure!(emulsion_gpu::context().is_none());
    ensure!(emulsion_gpu::screen_context().is_none());
    ensure!(!emulsion_raster::composite::accelerator_installed());
    ensure!(!emulsion_filters::accelerator_installed());
    ensure!(!emulsion_raster::paint_accel::compositor_installed());
    ensure!(!emulsion_raster::paint_accel::persistent_installed());
    let expected_tile = render_tile(&tree, 0, tile);
    let (expected_filter, expected_offset) = apply_stack(&filter_source, &stack);
    if automatic_hardware {
        ensure!(
            expected_filter.to_pixels() != filter_source.to_pixels(),
            "Automatic ReduceNoise fixture was a no-op"
        );
    }
    let paint_base = Arc::new(Raster::empty(512, 512, [7000, 10000, 15000, 30000]));
    let expected_paint = (brushes == "1")
        .then(|| final_paint_stroke(paint_base.clone()).render_with_compositor(&paint_base, None));
    let persistent_base = Arc::new(Raster::empty(16, 16, [7000, 10000, 15000, 30000]));
    let expected_persistent = (brushes == "persistent").then(|| {
        persistent_stroke(persistent_base.clone()).render_with_compositor(&persistent_base, None)
    });

    let started = Instant::now();
    let initialization = emulsion_gpu::begin_initialize();
    println!(
        "Backend smoke initialization kickoff: {:?}",
        started.elapsed()
    );
    // Read contexts before status: a Pending snapshot then proves these earlier
    // observations preceded publication, without racing a just-completed commit.
    let pending_context = emulsion_gpu::context();
    let pending_screen = emulsion_gpu::screen_context();
    if matches!(
        initialization.status(),
        emulsion_gpu::InitializationStatus::Pending
    ) {
        ensure!(
            pending_context.is_none(),
            "Pending startup exposed a context"
        );
        ensure!(
            pending_screen.is_none(),
            "Pending startup exposed screen compute"
        );
    }
    // This compatibility wrapper must wait on the same bounded attempt.
    emulsion_gpu::initialize();
    let status = initialization.status();
    println!(
        "Backend smoke startup: status={status:?}, elapsed={:?}",
        started.elapsed()
    );
    let owned_context = emulsion_gpu::context();
    let context = owned_context.as_ref();
    if cpu_requested {
        ensure!(
            matches!(
                status,
                emulsion_gpu::InitializationStatus::DisabledByConfiguration
            ),
            "CPU override did not bypass startup: {status:?}"
        );
        ensure!(context.is_none(), "CPU override created a GPU context");
        ensure!(
            emulsion_gpu::screen_context().is_none(),
            "CPU override exposed screen compute"
        );
        ensure!(!emulsion_raster::composite::accelerator_installed());
        ensure!(!emulsion_filters::accelerator_installed());
        ensure!(!emulsion_raster::paint_accel::compositor_installed());
        ensure!(!emulsion_raster::paint_accel::persistent_installed());
    } else {
        ensure!(
            matches!(status, emulsion_gpu::InitializationStatus::Ready),
            "GPU startup failed: {status:?}"
        );
        let gpu = context.context("GPU smoke requires an available compute adapter")?;
        println!("Backend smoke adapter: {:?}", gpu.adapter_info());
        if require_vulkan {
            ensure!(
                gpu.adapter_info().backend == wgpu::Backend::Vulkan,
                "This acceptance run requires a Vulkan compute adapter"
            );
        }
        if mode == "software" {
            ensure!(
                gpu.adapter_info().device_type == wgpu::DeviceType::Cpu,
                "Software acceptance requires a CPU compute adapter"
            );
        }
        let mut prepared = gpu.prepared_capabilities();
        prepared.sort_unstable();
        println!("Backend smoke prepared capabilities: {prepared:?}");
        let mut expected = vec!["composite", "filters"];
        if screen_requested {
            expected.push("screen");
        }
        match brushes.as_str() {
            "1" => expected.push("paint"),
            "persistent" => expected.push("persistent-paint"),
            _ => {}
        }
        expected.sort_unstable();
        ensure!(
            prepared == expected,
            "Prepared capabilities {prepared:?} do not match requested {expected:?}"
        );
        ensure!(emulsion_raster::composite::accelerator_installed());
        ensure!(emulsion_filters::accelerator_installed());
        ensure!(emulsion_raster::paint_accel::compositor_installed() == (brushes == "1"));
        ensure!(emulsion_raster::paint_accel::persistent_installed() == (brushes == "persistent"));
        let minimum_dispatches =
            6 + u64::from(screen_requested) * 2 + u64::from(brushes == "1") * 2;
        ensure!(
            gpu.dispatch_count() >= minimum_dispatches,
            "Readiness lacks preparation and ordinary-validation dispatch evidence"
        );
        if brushes == "persistent" {
            ensure!(
                gpu.persistent_dispatch_count() >= 4,
                "Persistent readiness lacks fresh-session dispatch evidence"
            );
        } else {
            ensure!(
                gpu.persistent_dispatch_count() == 0,
                "Unselected persistent capability executed during startup"
            );
        }
    }
    let startup_dispatches = context.map_or(0, |gpu| gpu.dispatch_count());
    let startup_persistent_dispatches = context.map_or(0, |gpu| gpu.persistent_dispatch_count());
    println!(
        "Backend smoke readiness dispatch baselines: ordinary={startup_dispatches}, persistent={startup_persistent_dispatches}"
    );
    let before = context.map_or(0, |gpu| gpu.dispatch_count());
    let actual_tile = render_tile(&tree, 0, tile);
    if let Some(gpu) = context {
        ensure!(
            gpu.dispatch_count() > before,
            "render_tile silently used CPU instead of the installed GPU hook"
        );
    }
    ensure!(actual_tile.len() == expected_tile.len());
    for (actual, expected) in actual_tile
        .iter()
        .flatten()
        .zip(expected_tile.iter().flatten())
    {
        ensure!(
            actual.is_finite() && (actual - expected).abs() <= 0.0003,
            "compositor parity: {actual} vs {expected}"
        );
    }

    let before = context.map_or(0, |gpu| gpu.dispatch_count());
    let (actual_filter, actual_offset) = apply_stack(&filter_source, &stack);
    if let Some(gpu) = context {
        ensure!(
            gpu.dispatch_count() > before,
            "apply_stack silently used CPU instead of the installed GPU hook"
        );
    }
    ensure!(actual_offset == expected_offset);
    ensure!(
        actual_filter.width() == expected_filter.width()
            && actual_filter.height() == expected_filter.height()
    );
    check_rgba16(&actual_filter, &expected_filter, 3, "filter parity")?;

    if let Some((expected, expected_dirty)) = expected_paint {
        let before = context.map_or(0, |gpu| gpu.dispatch_count());
        let mut stroke = final_paint_stroke(paint_base.clone());
        let (actual, actual_dirty) = stroke.render(&paint_base);
        ensure!(
            !stroke.uses_persistent(),
            "Final-paint mode used the persistent factory"
        );
        ensure!(actual_dirty == expected_dirty && !actual_dirty.is_empty());
        ensure!(
            actual.to_pixels() != paint_base.to_pixels(),
            "Paint smoke was a no-op"
        );
        if let Some(gpu) = context {
            ensure!(
                gpu.dispatch_count() > before,
                "Stroke::render bypassed the installed final-paint hook"
            );
        }
        check_rgba16(&actual, &expected, 1, "installed final-paint hook")?;
        println!("Backend smoke final-paint routing passed.");
    }

    if let Some((expected, expected_dirty)) = expected_persistent {
        // A second routed stroke after Drop must acquire the installed factory's
        // released permit and reuse the already prepared production pipeline.
        for session in 0..2 {
            let before = context.map_or(0, |gpu| gpu.persistent_dispatch_count());
            let mut stroke = persistent_stroke(persistent_base.clone());
            let (actual, actual_dirty) = stroke.render(&persistent_base);
            ensure!(
                stroke.uses_persistent() != cpu_requested,
                "Incorrect installed persistent-factory routing"
            );
            ensure!(actual_dirty == expected_dirty && !actual_dirty.is_empty());
            ensure!(
                actual.to_pixels() != persistent_base.to_pixels(),
                "Persistent smoke was a no-op"
            );
            if let Some(gpu) = context {
                ensure!(
                    gpu.persistent_dispatch_count() > before,
                    "Persistent session {session} did not dispatch after Ready"
                );
            }
            check_rgba16(&actual, &expected, 2, "installed persistent factory")?;
            drop(stroke);
        }
        println!("Backend smoke persistent routing and permit release passed.");
    }

    if screen_requested {
        let screen = emulsion_gpu::screen_context()
            .context("Ready did not expose the requested screen capability")?;
        check_screen(&screen)?;
        println!("Backend smoke experimental screen sampling passed.");
    } else {
        ensure!(
            emulsion_gpu::screen_context().is_none(),
            "Unrequested screen capability was published"
        );
    }
    println!(
        "Backend smoke post-ready dispatch deltas: ordinary={}, persistent={}",
        context.map_or(0, |gpu| gpu.dispatch_count() - startup_dispatches),
        context.map_or(0, |gpu| gpu.persistent_dispatch_count()
            - startup_persistent_dispatches)
    );
    println!(
        "Backend smoke passed: production compositor/filter hooks and requested optional capabilities, {} mode, brushes={brushes:?}.",
        if cpu_requested { "CPU" } else { "GPU" },
    );
    Ok(())
}
