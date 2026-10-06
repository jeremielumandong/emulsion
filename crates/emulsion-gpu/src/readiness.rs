//! Small, real production dispatches with independent known-result oracles.
//!
//! These checks deliberately bypass performance routing, never correctness
//! validation. The startup coordinator runs them once with its bounded startup
//! allowance and again with the unchanged ordinary operation deadlines.

use crate::GpuContext;
use crate::brush_backend::BrushFactory;
use crate::startup::Capability;
use anyhow::{Context, Result, ensure};
use emulsion_filters::Filter;
use emulsion_raster::blend::BlendSpace;
use emulsion_raster::color::{f_to_px, px_to_f};
use emulsion_raster::composite::{CompositeNode, CompositeTree, NodeContent, render_tile_cpu};
use emulsion_raster::paint::{BrushBlend, Clip};
use emulsion_raster::paint_accel::{PaintBatch, PaintCompositor, PersistentFactory, ResolvedDab};
use emulsion_raster::{BlendMode, Raster, TILE_PX, TileCoord};
use std::sync::Arc;

pub(crate) fn validate(gpu: &Arc<GpuContext>, capability: Capability) -> Result<()> {
    ensure!(
        gpu.available(),
        "GPU became unavailable before readiness check"
    );
    let before = gpu.dispatch_count();
    let persistent_before = gpu.persistent_dispatch_count();
    match capability {
        Capability::Composite => composite(gpu)?,
        Capability::Filters => filters(gpu)?,
        Capability::Paint => paint(gpu)?,
        Capability::PersistentPaint => persistent_paint(gpu)?,
        Capability::Screen => screen(gpu)?,
    }
    let dispatched = match capability {
        Capability::PersistentPaint => gpu.persistent_dispatch_count() > persistent_before,
        _ => gpu.dispatch_count() > before,
    };
    ensure!(
        dispatched,
        "Readiness fixture completed without a GPU dispatch"
    );
    ensure!(
        gpu.available(),
        "GPU became unavailable during readiness check"
    );
    Ok(())
}

fn check_floats(actual: &[[f32; 4]], expected: &[[f32; 4]], tolerance: f32) -> Result<()> {
    ensure!(
        actual.len() == expected.len(),
        "Readiness output length mismatch"
    );
    ensure!(
        expected.iter().flatten().any(|&value| value != 0.0),
        "Readiness fixture must have nonzero expected output"
    );
    for (index, (actual, expected)) in actual.iter().zip(expected).enumerate() {
        for channel in 0..4 {
            ensure!(
                actual[channel].is_finite()
                    && expected[channel].is_finite()
                    && (actual[channel] - expected[channel]).abs() <= tolerance,
                "Readiness pixel {index}, channel {channel}: {} != {}",
                actual[channel],
                expected[channel]
            );
        }
    }
    Ok(())
}

fn check_rgba16(actual: &[[u16; 4]], expected: &[[u16; 4]], tolerance: u16) -> Result<()> {
    ensure!(
        actual.len() == expected.len(),
        "Readiness output length mismatch"
    );
    ensure!(
        expected.iter().flatten().any(|&value| value != 0),
        "Readiness fixture must have nonzero expected output"
    );
    // Integer readbacks are finite by representation; compare every channel.
    for (index, (actual, expected)) in actual.iter().zip(expected).enumerate() {
        for channel in 0..4 {
            ensure!(
                actual[channel].abs_diff(expected[channel]) <= tolerance,
                "Readiness pixel {index}, channel {channel}: {} != {}",
                actual[channel],
                expected[channel]
            );
        }
    }
    Ok(())
}

fn node(id: u64, fill: [f32; 4]) -> CompositeNode {
    CompositeNode {
        id,
        visible: true,
        opacity: 1.0,
        blend: BlendMode::Normal,
        blending: Default::default(),
        mask: None,
        clip_to: None,
        clip_rect: None,
        content: NodeContent::Fill(fill),
    }
}

fn composite(gpu: &GpuContext) -> Result<()> {
    let mut root = node(2, [0.2, 0.4, 0.6, 1.0]);
    root.blend = BlendMode::Multiply;
    root.opacity = 0.5;
    let mut member = node(3, [0.8, 0.2, 0.4, 1.0]);
    member.clip_to = Some(1);
    let tree = CompositeTree {
        knockout_background: None,
        width: 1,
        height: 1,
        space: BlendSpace::Linear,
        nodes: vec![node(1, [0.5, 0.25, 0.75, 1.0]), root, member],
    };
    let tile = TileCoord::new(0, 0);
    let expected = render_tile_cpu(&tree, 0, tile);
    // Independent grouped-clipping oracle: blend the opaque member with the
    // backdrop using the root's Multiply mode and apply root opacity once.
    check_floats(&expected[..1], &[[0.45, 0.15, 0.525, 1.0]], 3e-5)?;
    let actual = crate::compositor::render_tile_gpu(gpu, &tree, 0, tile)?
        .context("Compositor readiness dispatch declined")?;
    ensure!(
        actual.len() == TILE_PX,
        "Incomplete compositor readiness tile"
    );
    check_floats(&actual, &expected, 3e-5)
}

fn filters(gpu: &GpuContext) -> Result<()> {
    let mut input = vec![[0.0; 4]; 9];
    input[4] = [0.45, 0.18, 0.09, 0.9];
    // A radius-one 3x3 box filter spreads this single central impulse equally
    // over all nine pixels. This is an independent math oracle, not a call
    // through an installed accelerator or a zero-strength identity operation.
    let expected = [[0.05, 0.02, 0.01, 0.1]; 9];
    let actual = gpu
        .apply_filter_gpu(&Filter::BoxBlur { radius: 1.0 }, 3, 3, &input)
        .context("Filter readiness dispatch declined")?;
    check_floats(&actual, &expected, 3e-5)
}

fn paint(gpu: &GpuContext) -> Result<()> {
    let base = Raster::empty(2, 1, f_to_px([0.12, 0.18, 0.06, 0.6]));
    let mut accumulated = vec![[0.0; 6]; TILE_PX];
    accumulated[0] = [0.2, 0.1, 0.05, 0.4, 0.5, 0.0];
    accumulated[1] = [0.04, 0.08, 0.12, 0.2, 0.25, 0.0];
    let tiles = [(TileCoord::new(0, 0), accumulated.as_slice())];
    let clip: Clip = Arc::new(|x, _| if x == 0 { 0.5 } else { 0.25 });
    let batch = PaintBatch {
        base: &base,
        tiles: &tiles,
        clip: Some(&clip),
        opacity: 0.6,
        blend: BrushBlend::Normal,
        erase: false,
        alpha_lock: false,
    };
    let mut expected = vec![base.fill(); TILE_PX];
    let destination = px_to_f(base.fill());
    for (index, clipping) in [0.5, 0.25].into_iter().enumerate() {
        let source: [f32; 4] = std::array::from_fn(|c| accumulated[index][c] * 0.6 * clipping);
        expected[index] = f_to_px(std::array::from_fn(|c| {
            source[c] + destination[c] * (1.0 - source[3])
        }));
    }
    ensure!(
        expected[0] != base.fill(),
        "Paint fixture did not alter its base"
    );
    let actual = gpu
        .composite_paint(&batch)
        .context("Paint readiness dispatch declined")?;
    ensure!(actual.len() == 1, "Paint readiness tile count mismatch");
    check_rgba16(&actual[0], &expected, 1)
}

fn persistent_paint(gpu: &Arc<GpuContext>) -> Result<()> {
    let factory = BrushFactory::new(gpu.clone());
    let base = Raster::empty(3, 2, [7000, 10000, 15000, 30000]);
    let destination = px_to_f(base.fill());
    // Two fresh sessions on the same factory prove permit release and that
    // preparation retains no first-session accumulation in the second session.
    for color in [[0.3, 0.1, 0.2, 0.6], [0.1, 0.4, 0.2, 0.7]] {
        let mut session = factory
            .start(&base, 0.63)
            .context("Persistent readiness session declined")?;
        ensure!(
            factory.start(&base, 0.63).is_none(),
            "Persistent factory allowed concurrent readiness sessions"
        );
        let before = gpu.persistent_dispatch_count();
        ensure!(
            session.append(&[ResolvedDab {
                center: [1.5, 1.0],
                radius: 8.0,
                hardness: 1.0,
                flow: 0.37,
                color,
            }]),
            "Persistent readiness append failed"
        );
        let actual = session
            .preview()
            .context("Persistent readiness preview failed")?;
        ensure!(
            gpu.persistent_dispatch_count() > before,
            "Persistent readiness append did not dispatch"
        );
        // All six pixel centers are well inside the hard dab. Accumulation is
        // color * flow, and composition is ordinary premultiplied source-over.
        let source = color.map(|value| value * 0.37 * 0.63);
        let pixel = f_to_px(std::array::from_fn(|c| {
            source[c] + destination[c] * (1.0 - source[3])
        }));
        ensure!(
            pixel != base.fill(),
            "Persistent fixture did not alter its base"
        );
        check_rgba16(&actual, &[pixel; 6], 2)?;
        drop(session);
    }
    Ok(())
}

fn screen(gpu: &GpuContext) -> Result<()> {
    let pixels: Vec<u8> = (0..TILE_PX as u32)
        .flat_map(|index| (0xff00_0000 | index).to_le_bytes())
        .collect();
    let previous: Vec<u8> = (0..TILE_PX as u32)
        .flat_map(|index| (0x8800_0000 | index).to_le_bytes())
        .collect();
    let tiles = [
        crate::screen::Tile {
            x: 0,
            y: 0,
            before: false,
            bgra: &pixels,
        },
        crate::screen::Tile {
            x: 0,
            y: 0,
            before: true,
            bgra: &previous,
        },
    ];
    let view = crate::screen::View {
        size: [3, 2],
        document: [256, 256],
        origin: [2.25, 3.25],
        dx: [1.0, 0.0],
        dy: [0.0, 1.0],
        wipe: Some(1),
    };
    let actual = gpu
        .sample_screen(&view, &tiles)
        .context("Screen readiness dispatch declined")?;
    ensure!(actual.len() == 6, "Screen readiness output length mismatch");
    for (index, &pixel) in actual.iter().enumerate() {
        let x = index as u32 % 3;
        let y = index as u32 / 3;
        let expected = (if x == 0 { 0x8800_0000 } else { 0xff00_0000 }) | ((3 + y) * 256 + 2 + x);
        ensure!(
            pixel == [expected, 0],
            "Screen readiness pixel {index}: {pixel:?}"
        );
    }
    Ok(())
}

#[cfg(test)]
mod tests {
    use super::{check_floats, check_rgba16};

    #[test]
    fn known_result_checks_reject_incomplete_wrong_or_nonfinite_output() {
        let expected = [[0.1, 0.2, 0.3, 0.5]];
        assert!(check_floats(&expected, &expected, 3e-5).is_ok());
        assert!(check_floats(&[], &expected, 3e-5).is_err());
        assert!(check_floats(&[[0.0; 4]], &expected, 3e-5).is_err());
        for invalid in [f32::NAN, f32::INFINITY, f32::NEG_INFINITY] {
            let mut actual = expected;
            actual[0][2] = invalid;
            assert!(check_floats(&actual, &expected, 3e-5).is_err());
        }
        assert!(check_floats(&[[0.0; 4]], &[[0.0; 4]], 3e-5).is_err());
        let expected = [[7000, 10000, 15000, 30000]];
        assert!(check_rgba16(&expected, &expected, 1).is_ok());
        assert!(check_rgba16(&[], &expected, 1).is_err());
        assert!(check_rgba16(&[[0; 4]], &expected, 1).is_err());
        assert!(check_rgba16(&[[0; 4]], &[[0; 4]], 1).is_err());
    }
}
