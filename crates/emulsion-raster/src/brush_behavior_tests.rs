use crate::{Raster, paint::*, preview::*};
use serde::Deserialize;
use std::sync::Arc;

fn pixels(raster: &Raster) -> Vec<[u16; 4]> {
    (0..raster.height())
        .flat_map(|y| (0..raster.width()).map(move |x| raster.get(x, y)))
        .collect()
}

fn samples() -> Vec<StrokeSample> {
    (0..17)
        .map(|i| StrokeSample {
            x: 14.0 + i as f32 * 6.0,
            y: 32.0 + (i as f32 * 0.3).sin() * 8.0,
            time_ms: i as f64,
            pressure: Some(0.4),
            tilt: Some((40.0, 20.0)),
        })
        .collect()
}

fn brush() -> Brush {
    Brush {
        size: 18.0,
        hardness: 0.6,
        flow: 0.3,
        spacing: 0.35,
        ..Brush::default()
    }
}

fn draw(brush: Brush) -> Vec<[u16; 4]> {
    pixels(&render_stroke(
        Arc::new(Raster::solid(128, 64, [0.0; 4])),
        brush,
        PreviewMode::Paint([0.8, 0.2, 0.1, 1.0]),
        &samples(),
        1234,
    ))
}

#[test]
fn legacy_brush_schema_defaults_to_neutral_advanced_controls() {
    let fields = [
        ("size", 18.0_f32),
        ("flow", 0.3),
        ("spacing", 0.35),
        ("hardness", 0.6),
    ];
    let legacy = Brush::deserialize(serde::de::value::MapDeserializer::<
        _,
        serde::de::value::Error,
    >::new(fields.into_iter()))
    .unwrap();
    assert_eq!(legacy, brush());
    assert_eq!(draw(legacy), draw(brush()));
}

#[test]
fn studio_preview_matches_incremental_canvas_strokes_and_seeded_replay() {
    let mut brush = brush();
    brush.advanced.path.lateral_jitter = 0.4;
    brush.advanced.path.spacing_jitter = 0.5;
    brush.advanced.shape.count = 3;
    brush.advanced.shape.count_jitter = 0.7;
    brush.taper_end = 25.0;
    let base = Arc::new(Raster::solid(128, 64, [0.0; 4]));
    let inputs = samples();
    let mode = PreviewMode::Paint([0.8, 0.2, 0.1, 1.0]);
    let expected = render_stroke(base.clone(), brush, mode, &inputs, 42);
    let mut stroke = Stroke::new_with_persistent(
        base.clone(),
        brush,
        Ink::Color([0.8, 0.2, 0.1, 1.0]),
        None,
        None,
    );
    assert!(stroke.set_seed(42));
    let mut current = (*base).clone();
    for input in &inputs {
        stroke.point_full(
            input.x,
            input.y,
            input.pressure,
            input.tilt,
            Some(input.time_ms),
        );
        current = stroke.render_with_compositor(&current, None).0;
    }
    assert!(!stroke.set_seed(88));
    stroke.finish();
    current = stroke.render_with_compositor(&current, None).0;
    assert_eq!(pixels(&current), pixels(&expected));
    assert_eq!(
        pixels(&expected),
        pixels(&render_stroke(base.clone(), brush, mode, &inputs, 42))
    );
    assert_ne!(
        pixels(&expected),
        pixels(&render_stroke(base, brush, mode, &inputs, 43))
    );
}

#[test]
fn path_shape_and_dynamics_controls_change_real_pixels() {
    let baseline = draw(brush());
    let changes: &[fn(&mut Brush)] = &[
        |b| b.advanced.path.lateral_jitter = 0.7,
        |b| b.advanced.path.linear_jitter = 0.7,
        |b| b.advanced.path.spacing_jitter = 0.8,
        |b| b.advanced.path.falloff = 50.0,
        |b| b.advanced.shape.count = 3,
        |b| b.advanced.dynamics.pressure_opacity = 0.9,
        |b| b.advanced.dynamics.speed_size = 0.9,
        |b| b.advanced.dynamics.speed_opacity = 0.9,
        |b| b.advanced.dynamics.tilt_opacity = 0.9,
        |b| b.advanced.dynamics.opacity_jitter = 0.9,
        |b| b.advanced.wet.charge = 30.0,
        |b| b.advanced.wet.dilution = 0.7,
    ];
    for (index, change) in changes.iter().enumerate() {
        let mut altered = brush();
        change(&mut altered);
        assert_ne!(
            baseline,
            draw(altered),
            "control {index} must change its rendered stroke"
        );
    }
    let mut pressured = brush();
    pressured.size_pressure = 0.8;
    let previous = draw(pressured);
    pressured.advanced.dynamics.pressure = ResponseCurve([0.0, 0.7, 0.9, 1.0, 1.0]);
    assert_ne!(previous, draw(pressured));
}

#[test]
fn shape_flips_and_rotation_transform_asymmetric_image_tips() {
    let id = 0xaab1_7001;
    textures::register(
        id,
        textures::Texture::from_gray8(
            4,
            4,
            &[255, 255, 0, 0, 255, 100, 0, 0, 255, 255, 255, 0, 0, 0, 0, 0],
        )
        .unwrap(),
    );
    let original = Brush { tip: id, ..brush() };
    let baseline = draw(original);
    for field in 0..3 {
        let mut b = original;
        match field {
            0 => b.advanced.shape.flip_x = true,
            1 => b.advanced.shape.flip_y = true,
            _ => b.advanced.shape.rotation_jitter = 0.8,
        }
        assert_ne!(baseline, draw(b));
    }
}

#[test]
fn grain_coordinates_and_tone_controls_are_applied() {
    let original = Brush {
        grain: GrainKind::Canvas,
        grain_strength: 0.85,
        ..brush()
    };
    let baseline = draw(original);
    let changes: &[fn(&mut GrainSettings)] = &[
        |g| g.mode = GrainMode::Moving,
        |g| g.scale = 2.5,
        |g| g.rotation = 37.0,
        |g| g.brightness = 0.35,
        |g| g.contrast = 2.0,
    ];
    for (index, change) in changes.iter().enumerate() {
        let mut b = original;
        change(&mut b.advanced.grain);
        assert_ne!(baseline, draw(b), "grain control {index}");
    }
    let mut b = original;
    b.advanced.grain.mode = GrainMode::Moving;
    let without_phase = draw(b);
    b.advanced.grain.offset_jitter = 0.8;
    assert_ne!(without_phase, draw(b));
}

#[test]
fn accumulating_rendering_builds_beyond_whole_stroke_opacity_cap() {
    let b = Brush {
        opacity: 0.2,
        spacing: 0.04,
        flow: 0.7,
        ..brush()
    };
    let glaze = draw(b);
    assert!(glaze.iter().all(|p| p[3] <= 13108));
    let mut accumulating = b;
    accumulating.advanced.rendering = RenderingMode::Accumulating;
    let paint = draw(accumulating);
    assert!(paint.iter().any(|p| p[3] > 20000));
}

#[test]
fn invalid_inputs_and_settings_cannot_create_nonfinite_strokes() {
    let mut b = brush();
    b.advanced.shape.count = 0;
    b.advanced.grain.scale = f32::NAN;
    b.advanced.dynamics.pressure = ResponseCurve([f32::NAN; 5]);
    b.advanced.wet.charge = f32::INFINITY;
    let b = b.sanitized();
    assert_eq!(b.advanced.shape.count, 1);
    assert_eq!(b.advanced.grain.scale, 1.0);
    assert_eq!(b.advanced.wet.charge, 0.0);
    let base = Arc::new(Raster::solid(32, 32, [0.0; 4]));
    let invalid = [StrokeSample {
        x: f32::NAN,
        y: 4.0,
        time_ms: 0.0,
        pressure: None,
        tilt: None,
    }];
    let result = render_stroke(base.clone(), b, PreviewMode::Paint([1.0; 4]), &invalid, 1);
    assert_eq!(pixels(&base), pixels(&result));
}

#[test]
fn dual_strokes_combine_masks_and_match_preview_across_incremental_updates() {
    let primary = Brush {
        size: 20.0,
        opacity: 0.7,
        ..brush()
    };
    let mut secondary = Brush {
        size: 32.0,
        opacity: 0.6,
        grain: GrainKind::Paper,
        grain_strength: 0.5,
        ..brush()
    };
    secondary.advanced.path.lateral_jitter = 0.2;
    secondary.taper_end = 35.0;
    let base = Arc::new(Raster::transparent(128, 64));
    let inputs = samples();
    let mut outputs = Vec::new();
    for blend in [DualBlend::Normal, DualBlend::Multiply, DualBlend::Screen] {
        let expected = render_dual_stroke(
            base.clone(),
            primary,
            secondary,
            blend,
            PreviewMode::Paint([0.6, 0.2, 0.1, 1.0]),
            &inputs,
            123,
        );
        let mut stroke = Stroke::new_with_persistent(
            base.clone(),
            primary,
            Ink::Color([0.6, 0.2, 0.1, 1.0]),
            None,
            None,
        );
        assert!(stroke.set_secondary(secondary, blend));
        assert!(stroke.set_seed(123));
        let mut current = (*base).clone();
        for sample in &inputs {
            stroke.point_full(
                sample.x,
                sample.y,
                sample.pressure,
                sample.tilt,
                Some(sample.time_ms),
            );
            current = stroke.render_with_compositor(&current, None).0;
        }
        assert!(!stroke.set_secondary(brush(), blend));
        stroke.finish();
        current = stroke.render_with_compositor(&current, None).0;
        assert_eq!(pixels(&expected), pixels(&current));
        let (repeated, dirty) = stroke.render_with_compositor(&current, None);
        assert_eq!(dirty, crate::IRect::default());
        assert_eq!(pixels(&current), pixels(&repeated));
        assert!(stroke.coverage().to_pixels().iter().any(|v| *v > 0));
        outputs.push(pixels(&expected));
    }
    assert_ne!(outputs[0], outputs[1]);
    assert_ne!(outputs[0], outputs[2]);
    assert!(base.to_pixels().iter().all(|p| p[3] == 0));
}

#[test]
fn dual_eraser_respects_clip_alpha_lock_and_replay_restores_old_pixels() {
    let base = Arc::new(Raster::solid(128, 64, [0.3, 0.1, 0.2, 0.6]));
    let clip: Clip = Arc::new(|x, _| if x < 64 { 1.0 } else { 0.0 });
    let mut stroke =
        Stroke::new_with_persistent(base.clone(), brush(), Ink::Erase, Some(clip), None);
    stroke.set_secondary(
        Brush {
            size: 30.0,
            ..brush()
        },
        DualBlend::Multiply,
    );
    stroke.set_alpha_lock(true);
    stroke.point(15.0, 25.0);
    stroke.point(110.0, 25.0);
    let locked = stroke.render_with_compositor(&base, None).0;
    assert_eq!(pixels(&locked), pixels(&base));
    stroke.set_alpha_lock(false);
    stroke.replay(&[(15.0, 25.0), (110.0, 25.0)], 1.0);
    let erased = stroke.render_with_compositor(&locked, None).0;
    assert!(erased.get(35, 25)[3] < base.get(35, 25)[3]);
    assert_eq!(erased.get(85, 25), base.get(85, 25));
    stroke.replay(&[(15.0, 48.0), (110.0, 48.0)], 1.0);
    let moved = stroke.render_with_compositor(&erased, None).0;
    assert_eq!(moved.get(35, 25), base.get(35, 25));
    assert!(moved.get(35, 48)[3] < base.get(35, 48)[3]);
}

#[test]
fn wet_pull_retains_picked_up_color_over_a_changing_backdrop() {
    let base = Arc::new(Raster::from_fn(128, 64, [0; 4], |x, _| {
        if x < 55 {
            [0, 0, 65535, 65535]
        } else {
            [65535, 65535, 0, 65535]
        }
    }));
    let primary = Brush {
        wetness: 0.8,
        ..brush()
    };
    let mut carried = primary;
    carried.advanced.wet.pull = 1.0;
    for mode in [
        PreviewMode::Paint([0.7, 0.1, 0.1, 1.0]),
        PreviewMode::Smudge,
    ] {
        let ordinary = render_stroke(base.clone(), primary, mode, &samples(), 1);
        let retained = render_stroke(base.clone(), carried, mode, &samples(), 1);
        assert_ne!(pixels(&ordinary), pixels(&retained));
        assert!(retained.get(100, 32)[2] > ordinary.get(100, 32)[2]);
    }
}

#[test]
fn independent_color_channels_taper_and_properties_have_rendered_effects() {
    let baseline = draw(brush());
    let colors: &[fn(&mut ColorDynamicsSettings)] = &[
        |c| c.stamp_hue = 0.7,
        |c| c.stamp_saturation = 0.7,
        |c| c.stamp_lightness = 0.7,
        |c| c.stroke_hue = 0.7,
        |c| c.stroke_saturation = 0.7,
        |c| c.stroke_lightness = 0.7,
        |c| c.pressure_hue = 0.7,
        |c| c.pressure_saturation = 0.7,
        |c| c.pressure_lightness = 0.7,
    ];
    for (index, change) in colors.iter().enumerate() {
        let mut b = brush();
        change(&mut b.advanced.color);
        let output = draw(b);
        assert_ne!(baseline, output, "color channel {index}");
        assert!(output.iter().all(|p| p[..3].iter().all(|v| *v <= p[3])));
    }
    let mut tapered = brush();
    tapered.taper_start = 40.0;
    tapered.taper_end = 40.0;
    let original_taper = draw(tapered);
    tapered.advanced.taper.opacity = 1.0;
    assert_ne!(original_taper, draw(tapered));
    tapered.advanced.taper.opacity = 0.0;
    tapered.advanced.taper.tip_curve = 3.0;
    assert_ne!(original_taper, draw(tapered));
    let mut limited = brush();
    limited.advanced.properties.max_size = 5.0;
    assert_ne!(baseline, draw(limited));
    limited = brush();
    limited.advanced.properties.min_size = 30.0;
    assert_ne!(baseline, draw(limited));
    limited = brush();
    limited.advanced.properties.max_opacity = 0.1;
    assert!(draw(limited).iter().all(|p| p[3] <= 6554));
    limited.opacity = 0.05;
    limited.advanced.properties.max_opacity = 1.0;
    let faint = draw(limited);
    limited.advanced.properties.min_opacity = 0.8;
    assert_ne!(faint, draw(limited));
}

#[test]
fn multi_stage_stabilization_smooths_position_and_pressure_and_finishes_at_pointer() {
    let base = Arc::new(Raster::transparent(128, 64));
    let mut b = Brush {
        size: 12.0,
        size_pressure: 0.7,
        ..brush()
    };
    let mut inputs = samples();
    for (index, sample) in inputs.iter_mut().enumerate() {
        sample.time_ms = index as f64 * 12.0;
        sample.y = if index % 2 == 0 { 22.0 } else { 42.0 };
        sample.pressure = Some(if index % 2 == 0 { 0.2 } else { 1.0 });
    }
    let mode = PreviewMode::Paint([0.8, 0.2, 0.1, 1.0]);
    let raw = render_stroke(base.clone(), b, mode, &inputs, 1);
    b.advanced.stabilization = StabilizationSettings {
        stages: 3,
        amount: 0.5,
        pressure: 0.0,
    };
    let positional = render_stroke(base.clone(), b, mode, &inputs, 1);
    assert_ne!(pixels(&raw), pixels(&positional));
    b.advanced.stabilization.pressure = 0.8;
    let smoothed = render_stroke(base, b, mode, &inputs, 1);
    assert_ne!(pixels(&positional), pixels(&smoothed));
    let endpoint = inputs.last().unwrap();
    assert!(smoothed.get(endpoint.x as u32 - 1, endpoint.y as u32)[3] > 0);
}
