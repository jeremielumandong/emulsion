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
use emulsion_raster::{Adjustment, BlendMode, Placement, Raster, TileCoord};
use std::sync::Arc;

fn node(id: u64, content: NodeContent) -> CompositeNode {
    CompositeNode {
        id,
        visible: true,
        opacity: 1.0,
        blend: BlendMode::Normal,
        blending: Default::default(),
        mask: None,
        clip_to: None,
        content,
    }
}

fn main() -> Result<()> {
    let cpu_requested = std::env::var("EMULSION_GPU").as_deref() == Ok("cpu");
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
    let tree = CompositeTree {
        width: 64,
        height: 64,
        space: BlendSpace::Linear,
        nodes: vec![
            node(
                1,
                NodeContent::Pixels {
                    raster: source.clone(),
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
    let stack = [Filter::LensBlur { radius: 4.0 }];
    let tile = TileCoord { x: 0, y: 0 };

    // No hooks have been installed in this process: these are real CPU
    // baselines, including filter padding and RGBA16 conversion.
    ensure!(emulsion_gpu::context().is_none());
    let expected_tile = render_tile(&tree, 0, tile);
    let (expected_filter, expected_offset) = apply_stack(&source, &stack);

    emulsion_gpu::initialize();
    let context = emulsion_gpu::context();
    if cpu_requested {
        ensure!(context.is_none(), "CPU override created a GPU context");
    } else {
        let gpu = context.context("GPU smoke requires an available compute adapter")?;
        println!("Backend smoke adapter: {}", gpu.name());
    }
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
    let (actual_filter, actual_offset) = apply_stack(&source, &stack);
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
    for (actual, expected) in actual_filter
        .to_pixels()
        .iter()
        .flatten()
        .zip(expected_filter.to_pixels().iter().flatten())
    {
        ensure!(
            actual.abs_diff(*expected) <= 3,
            "filter parity: {actual} vs {expected}"
        );
    }
    println!(
        "Backend smoke passed: production compositor and filter hooks, {} mode.",
        if cpu_requested { "CPU" } else { "GPU" }
    );
    Ok(())
}
