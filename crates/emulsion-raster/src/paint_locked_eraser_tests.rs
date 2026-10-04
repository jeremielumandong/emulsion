//! Locked Photo Eraser keeps its coverage semantics and replaces only RGB.
use super::*;
use crate::paint_accel::{PaintBatch, PaintCompositor, PersistentFactory, PersistentStroke};

fn hard_eraser(base: Arc<Raster>) -> Stroke {
    Stroke::new_with_persistent(
        base,
        Brush {
            size: 48.0,
            hardness: 1.0,
            ..Brush::default()
        },
        Ink::Erase,
        None,
        None,
    )
}

fn uniform_coverage(stroke: &mut Stroke, coverage: f32) {
    for y in 0..stroke.base.height().div_ceil(TILE) {
        for x in 0..stroke.base.width().div_ceil(TILE) {
            let coord = TileCoord {
                x: x as i32,
                y: y as i32,
            };
            stroke.paint.insert(
                coord,
                vec![[0., 0., 0., coverage, coverage, coverage]; TILE_PX],
            );
            stroke.pending.insert(coord);
        }
    }
}

#[test]
fn locked_eraser_keeps_every_u16_alpha_and_respects_coverage_opacity_and_selection() {
    // Exercise all 65,536 alpha values, including the lowest-coverage edges.
    let base = Arc::new(Raster::from_fn(256, 256, [0; 4], |x, y| {
        let alpha = (y * 256 + x) as u16;
        [alpha / 3, alpha / 5, alpha / 7, alpha]
    }));
    let background = [0.13, 0.72, 0.31];
    let clip: Clip = Arc::new(|x, _| [0.0, 0.25, 0.5, 1.0][x as usize % 4]);
    for opacity in [0.0, 0.37, 1.0] {
        for coverage in [0.63, 1.0] {
            let mut stroke = hard_eraser(base.clone());
            stroke.brush.opacity = opacity;
            stroke.clip = Some(clip.clone());
            stroke.set_alpha_lock(true);
            stroke.set_locked_erase_background(Some(background));
            uniform_coverage(&mut stroke, coverage);
            let (result, dirty) = stroke.render_with_compositor(&base, None);
            for y in 0..256 {
                for x in 0..256 {
                    let prior = base.get(x, y);
                    let b = color::px_to_f(prior);
                    let k = coverage * opacity * clip(x as i32, y as i32);
                    let mut expected = prior;
                    for ch in 0..3 {
                        expected[ch] =
                            color::f_to_u16(background[ch] * b[3] * k + b[ch] * (1.0 - k));
                    }
                    assert_eq!(result.get(x, y), expected, "{x},{y}; {opacity}, {coverage}");
                }
            }
            assert_eq!(dirty.is_empty(), opacity == 0.0);
        }
    }
}

#[test]
fn locked_eraser_ignores_brush_blends_wetness_color_jitter_and_relief() {
    let base = Arc::new(Raster::solid(64, 64, [0.2, 0.1, 0.05, 0.5]));
    let background = [0.8, 0.3, 0.6];
    let render = |blend, pigment_effects| {
        let mut stroke = hard_eraser(base.clone());
        stroke.brush.hardness = 0.3;
        stroke.brush.flow = 0.43;
        stroke.brush.opacity = 0.64;
        stroke.brush.blend = blend;
        // Dilution is an existing flow modifier even for Erase. Hold that
        // coverage setting equal while varying only pigment effects below.
        stroke.brush.advanced.wet.dilution = 0.8;
        if pigment_effects {
            stroke.brush.wetness = 0.85;
            stroke.brush.color_jitter = 1.0;
            stroke.brush.relief = 0.9;
            stroke.brush.advanced.wet.pull = 0.7;
            stroke.brush.advanced.color.stamp_hue = 1.0;
            stroke.brush.advanced.color.stroke_saturation = 1.0;
            stroke.brush.advanced.color.pressure_lightness = 1.0;
        }
        stroke.set_alpha_lock(true);
        stroke.set_locked_erase_background(Some(background));
        for (x, y) in [(20., 24.), (32., 32.), (42., 36.)] {
            stroke.point_full(x, y, Some(1.0), None, None);
        }
        stroke.finish();
        stroke.render_with_compositor(&base, None).0
    };
    let reference = render(BrushBlend::Normal, false);
    assert_ne!(reference.to_pixels(), base.to_pixels());
    for &blend in BrushBlend::MENU {
        assert_eq!(
            render(blend, true).to_pixels(),
            reference.to_pixels(),
            "{blend:?}"
        );
    }
}

#[test]
fn locked_eraser_policy_requires_both_erase_ink_and_alpha_lock() {
    let base = Arc::new(Raster::solid(64, 64, [0.2, 0.1, 0., 0.5]));
    for locked in [false, true] {
        for background in [None, Some([0.0, 1.0, 0.0])] {
            let mut stroke = hard_eraser(base.clone());
            stroke.set_alpha_lock(locked);
            stroke.set_locked_erase_background(background);
            stroke.point(32., 32.);
            let (result, dirty) = stroke.render_with_compositor(&base, None);
            let expected = if !locked {
                [0; 4]
            } else if background.is_some() {
                [0, base.get(32, 32)[3], 0, base.get(32, 32)[3]]
            } else {
                base.get(32, 32)
            };
            assert_eq!(result.get(32, 32), expected);
            assert_eq!(dirty.is_empty(), locked && background.is_none());
        }
        let render_color = |background| {
            let mut stroke = hard_eraser(base.clone());
            stroke.ink = Ink::Color([1., 0., 0., 1.]);
            stroke.set_alpha_lock(locked);
            stroke.set_locked_erase_background(background);
            stroke.point(32., 32.);
            stroke.render_with_compositor(&base, None).0.to_pixels()
        };
        assert_eq!(render_color(None), render_color(Some([0., 1., 0.])));
    }
    let transparent = Arc::new(Raster::transparent(64, 64));
    let mut stroke = hard_eraser(transparent.clone());
    stroke.set_alpha_lock(true);
    stroke.set_locked_erase_background(Some([1.; 3]));
    stroke.point(32., 32.);
    let (result, dirty) = stroke.render_with_compositor(&transparent, None);
    assert_eq!(result.to_pixels(), transparent.to_pixels());
    assert!(dirty.is_empty());
}

#[test]
fn locked_eraser_dual_brush_replay_retains_background_and_can_restore_preview() {
    let base = Arc::new(Raster::solid(64, 64, [0.2, 0.1, 0., 0.5]));
    for blend in [DualBlend::Normal, DualBlend::Multiply, DualBlend::Screen] {
        let mut stroke = hard_eraser(base.clone());
        stroke.brush.blend = BrushBlend::Behind;
        stroke.set_alpha_lock(true);
        stroke.set_locked_erase_background(Some([0., 1., 0.]));
        assert!(stroke.set_secondary(
            Brush {
                size: 32.,
                hardness: 1.,
                ..Brush::default()
            },
            blend
        ));
        stroke.point(32., 32.);
        let (painted, dirty) = stroke.render_with_compositor(&base, None);
        assert!(!dirty.is_empty());
        assert_eq!(
            painted.get(32, 32),
            [0, base.get(32, 32)[3], 0, base.get(32, 32)[3]]
        );
        for (before, after) in base.to_pixels().iter().zip(painted.to_pixels()) {
            assert_eq!(before[3], after[3]);
        }
        // QuickShape/taper rerendering must use captured pigment, not erase.
        stroke.replay(&[(32., 32.)], 1.0);
        assert_eq!(
            stroke.render_with_compositor(&painted, None).0.to_pixels(),
            painted.to_pixels()
        );
        stroke.clip = Some(Arc::new(|_, _| 0.0));
        stroke.pending.extend(stroke.paint.keys().copied());
        let (restored, dirty) = stroke.render_with_compositor(&painted, None);
        assert!(!dirty.is_empty());
        assert_eq!(restored.to_pixels(), base.to_pixels());
    }
}

struct UnsupportedLockedEraserBackend;

impl PaintCompositor for UnsupportedLockedEraserBackend {
    fn composite_paint(&self, _: &PaintBatch<'_>) -> Option<Vec<Vec<[u16; 4]>>> {
        panic!("locked Eraser must bypass compositor without background support")
    }
}

impl PersistentFactory for UnsupportedLockedEraserBackend {
    fn start(&self, _: &Raster, _: f32) -> Option<Box<dyn PersistentStroke>> {
        panic!("locked Eraser must bypass persistent pigment backend")
    }
}

#[test]
fn locked_eraser_uses_cpu_when_accelerators_cannot_represent_background() {
    let base = Arc::new(Raster::solid(TILE * 2, TILE * 2, [0.2, 0.1, 0., 0.5]));
    for blend in [BrushBlend::Normal, BrushBlend::Multiply, BrushBlend::Behind] {
        let mut stroke = Stroke::new_with_persistent(
            base.clone(),
            Brush {
                size: 400.,
                blend,
                ..Brush::default()
            },
            Ink::Erase,
            None,
            Some(Arc::new(UnsupportedLockedEraserBackend)),
        );
        stroke.set_alpha_lock(true);
        stroke.set_locked_erase_background(Some([0., 1., 0.]));
        stroke.point(TILE as f32, TILE as f32);
        assert!(!stroke.uses_persistent());
        uniform_coverage(&mut stroke, 1.0);
        assert_eq!(
            stroke.pending.len(),
            4,
            "would otherwise enter batch compositor"
        );
        let (result, dirty) =
            stroke.render_with_compositor(&base, Some(&UnsupportedLockedEraserBackend));
        assert!(!dirty.is_empty());
        assert_eq!(
            result.get(TILE, TILE),
            [0, base.get(TILE, TILE)[3], 0, base.get(TILE, TILE)[3]]
        );
    }
}
