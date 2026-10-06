use super::*;

#[test]
fn catalogue_metadata_matches_literal_pre_refactor_contract() {
    use std::fmt::Write;

    // Literal snapshot transcribed from 0f95e26028f48423261679e721f857bd193b9067.
    // Header: filter key | label | complete serialized default descriptor.
    // Parameter: key | label | min..max | step | default value | [unit].
    // Keep expected data independent of catalogue(), params(), and set_param().
    const EXPECTED: &str = r#"gaussian_blur|Gaussian blur|{"kind":"gaussian-blur","radius":5.0}
  radius|radius|0..100|0.1|5|[px]
box_blur|Box blur|{"kind":"box-blur","radius":5.0}
  radius|radius|0..100|0.1|5|[px]
motion_blur|Motion blur|{"kind":"motion-blur","angle":0.0,"distance":20.0}
  angle|angle|-180..180|1|0|[°]
  distance|distance|0..200|1|20|[px]
lens_blur|Lens blur|{"kind":"lens-blur","radius":8.0}
  radius|radius|0..40|0.5|8|[px]
unsharp_mask|Unsharp mask|{"kind":"unsharp-mask","amount":100.0,"radius":1.5,"threshold":0.0}
  amount|amount|0..500|1|100|[%]
  radius|radius|0.1..50|0.1|1.5|[px]
  threshold|threshold|0..255|1|0|[]
smart_sharpen|Smart sharpen|{"kind":"smart-sharpen","amount":80.0,"radius":1.0}
  amount|amount|0..500|1|80|[%]
  radius|radius|0.1..20|0.1|1|[px]
add_noise|Add noise|{"kind":"add-noise","amount":10.0,"monochrome":true}
  amount|amount|0..100|0.5|10|[%]
  monochrome|monochrome|0..1|1|1|[on]
reduce_noise|Reduce noise|{"kind":"reduce-noise","strength":5.0,"detail":50.0}
  strength|strength|0..10|0.5|5|[]
  detail|preserve detail|0..100|1|50|[%]
high_pass|High pass|{"kind":"high-pass","radius":10.0}
  radius|radius|0.1..100|0.1|10|[px]
lens_correction|Lens correction|{"kind":"lens-correction","distortion":0.0,"vignette":0.0}
  distortion|distortion|-100..100|1|0|[]
  vignette|vignette|-100..100|1|0|[]
lens_profile|Lens profile|{"kind":"lens-profile","a":0.0,"b":0.0,"c":0.0,"k1":0.0,"k2":0.0,"k3":0.0,"scale":1.0,"distortion":100.0,"vignette":100.0}
  distortion|distortion|0..150|1|100|[%]
  vignette|vignette|0..150|1|100|[%]
emboss|Emboss|{"kind":"emboss","angle":135.0,"height":3.0,"amount":100.0}
  angle|angle|-180..180|1|135|[°]
  height|height|1..20|1|3|[px]
  amount|amount|1..500|1|100|[%]
find_edges|Find edges|{"kind":"find-edges"}
invert|Invert|{"kind":"invert"}
pinch|Pinch|{"kind":"pinch","amount":50.0}
  amount|amount|-100..100|1|50|[%]
twirl|Twirl|{"kind":"twirl","angle":90.0}
  angle|angle|-999..999|1|90|[°]
wave|Wave|{"kind":"wave","amplitude":10.0,"wavelength":60.0}
  amplitude|amplitude|0..200|1|10|[px]
  wavelength|wavelength|2..500|1|60|[px]
enhance|Enhance|{"kind":"enhance","amount":50.0,"sky":0.0}
  amount|amount|0..100|1|50|[%]
  sky|sky|0..100|1|0|[%]
structure|Structure|{"kind":"structure","amount":40.0,"softness":30.0}
  amount|amount|-100..100|1|40|[%]
  softness|softness|0..100|1|30|[%]
glow|Glow|{"kind":"glow","amount":40.0,"radius":20.0,"threshold":50.0}
  amount|amount|0..100|1|40|[%]
  radius|radius|1..100|1|20|[%]
  threshold|threshold|0..100|1|50|[%]
orton|Mystical|{"kind":"orton","amount":40.0,"radius":15.0}
  amount|amount|0..100|1|40|[%]
  radius|radius|1..100|1|15|[%]
sunrays|Sunrays|{"kind":"sunrays","x":70.0,"y":20.0,"amount":50.0,"length":60.0,"warmth":60.0}
  x|sun x|0..100|1|70|[%]
  y|sun y|0..100|1|20|[%]
  amount|amount|0..100|1|50|[%]
  length|length|0..100|1|60|[%]
  warmth|warmth|0..100|1|60|[%]
atmosphere|Atmosphere|{"kind":"atmosphere","amount":30.0,"spread":50.0}
  amount|amount|-100..100|1|30|[%]
  spread|spread|0..100|1|50|[%]
skin_smooth|Skin smoothing|{"kind":"skin-smooth","amount":50.0,"radius":30.0,"detail":30.0}
  amount|amount|0..100|1|50|[%]
  radius|radius|1..100|1|30|[%]
  detail|detail|0..100|1|30|[%]
golden_hour|Golden hour|{"kind":"golden-hour","amount":50.0}
  amount|amount|0..100|1|50|[%]
dramatic|Dramatic|{"kind":"dramatic","amount":50.0}
  amount|amount|0..100|1|50|[%]
"#;

    let mut actual = String::new();
    for filter in Filter::catalogue() {
        writeln!(
            actual,
            "{}|{}|{}",
            filter.key(),
            filter.label(),
            serde_json::to_string(&filter).unwrap(),
        )
        .unwrap();
        for p in filter.params() {
            writeln!(
                actual,
                "  {}|{}|{}..{}|{}|{}|[{}]",
                p.key, p.label, p.min, p.max, p.step, p.value, p.unit,
            )
            .unwrap();
        }
    }
    assert_eq!(actual, EXPECTED);
}

fn disabled() -> FilterStyle {
    FilterStyle {
        enabled: false,
        ..Default::default()
    }
}

fn assert_grid(footprint: StackFootprint, size: (u64, u64), spread: i32, active: bool) {
    assert_eq!(footprint.size, size);
    assert_eq!(footprint.offset, (-spread, -spread));
    assert_eq!(footprint.spread, spread);
    assert_eq!(footprint.active, active);
}

#[test]
fn empty_bypassed_and_disabled_stacks_keep_the_source_grid() {
    let filters = [Filter::GaussianBlur { radius: 100.0 }, Filter::Invert];
    for size in [(17, 9), (0, 0), (0, 7), (u32::MAX, u32::MAX)] {
        let expected = (u64::from(size.0), u64::from(size.1));
        for footprint in [
            stack_footprint(size, &[], &[], true),
            stack_footprint(size, &[], &[FilterStyle::default()], true),
            stack_footprint(size, &filters, &[], false),
            stack_footprint(size, &filters, &[disabled(), disabled()], true),
        ] {
            assert_grid(footprint, expected, 0, false);
            assert_eq!(footprint.checked_raster_size(), Some(size));
        }
    }
}

#[test]
fn defaults_mixed_stages_and_zero_opacity_have_exact_grids() {
    let filters = [
        Filter::GaussianBlur { radius: 2.25 },
        Filter::BoxBlur { radius: 1.25 },
        Filter::Invert,
    ];
    assert_grid(
        stack_footprint((17, 9), &filters, &[], true),
        (35, 27),
        9,
        true,
    );
    assert_grid(
        stack_footprint((17, 9), &filters, &[disabled()], true),
        (21, 13),
        2,
        true,
    );
    assert_grid(
        stack_footprint((17, 9), &filters, &[disabled(), disabled()], true),
        (17, 9),
        0,
        true,
    );
    assert_grid(
        stack_footprint(
            (17, 9),
            &filters,
            &[
                FilterStyle {
                    opacity: 0.0,
                    blend: BlendMode::Dissolve,
                    ..Default::default()
                },
                disabled(),
            ],
            true,
        ),
        (31, 23),
        7,
        true,
    );
    // Extra styles cannot enable a nonexistent stage or change a real stage.
    assert_grid(
        stack_footprint(
            (17, 9),
            &filters[..1],
            &[disabled(), FilterStyle::default()],
            true,
        ),
        (17, 9),
        0,
        false,
    );
    let footprint = stack_footprint((17, 9), &filters, &[FilterStyle::default(); 4], true);
    assert_grid(footprint, (35, 27), 9, true);
    assert_eq!(footprint.checked_raster_size(), Some((35, 27)));
    assert_eq!(footprint.checked_pixel_count(), Some(945));
    assert_eq!(footprint.checked_dense_bytes(), Some(15_120));
}

#[test]
fn spread_is_sanitized_and_capped_per_stage_and_per_stack() {
    for (filter, spread) in [
        (Filter::GaussianBlur { radius: f32::NAN }, 15),
        (
            Filter::GaussianBlur {
                radius: f32::INFINITY,
            },
            15,
        ),
        (Filter::GaussianBlur { radius: -1.0 }, 0),
        (Filter::GaussianBlur { radius: f32::MAX }, 250),
        (Filter::BoxBlur { radius: 1.01 }, 2),
        (Filter::LensBlur { radius: f32::MAX }, 40),
        (
            Filter::MotionBlur {
                angle: f32::NAN,
                distance: f32::NEG_INFINITY,
            },
            10,
        ),
        (
            Filter::UnsharpMask {
                amount: 0.0,
                radius: -1.0,
                threshold: 0.0,
            },
            1,
        ),
        (
            Filter::SmartSharpen {
                amount: 0.0,
                radius: f32::MAX,
            },
            40,
        ),
        (
            Filter::Emboss {
                angle: 0.0,
                height: -1.0,
                amount: 0.0,
            },
            1,
        ),
        (
            Filter::Wave {
                amplitude: f32::MAX,
                wavelength: 0.0,
            },
            200,
        ),
    ] {
        assert_grid(
            stack_footprint((11, 7), &[filter], &[], true),
            (11 + 2 * spread as u64, 7 + 2 * spread as u64),
            spread,
            true,
        );
    }
    let filters = vec![Filter::GaussianBlur { radius: f32::MAX }; 64];
    assert_grid(
        stack_footprint((11, 7), &filters, &[], true),
        (511, 507),
        250,
        true,
    );
    let mut styles = vec![disabled(); filters.len()];
    styles[31] = FilterStyle {
        opacity: 0.0,
        ..Default::default()
    };
    assert_grid(
        stack_footprint((11, 7), &filters, &styles, true),
        (511, 507),
        250,
        true,
    );
    styles[31] = disabled();
    assert_grid(
        stack_footprint((11, 7), &filters, &styles, true),
        (11, 7),
        0,
        false,
    );
}

#[test]
fn wide_dimensions_expose_overflow_without_allocating_or_applying_limits() {
    let filters = [Filter::GaussianBlur { radius: 100.0 }];
    let max = u64::from(u32::MAX);
    let footprint = stack_footprint((u32::MAX, u32::MAX), &filters, &[], true);
    assert_grid(footprint, (max + 500, max + 500), 250, true);
    assert_eq!(footprint.checked_raster_size(), None);
    assert_eq!(footprint.checked_pixel_count(), None);
    assert_eq!(footprint.checked_dense_bytes(), None);

    let unpadded = stack_footprint((u32::MAX, u32::MAX), &filters, &[], false);
    assert_eq!(unpadded.checked_pixel_count(), Some(max * max));
    assert_eq!(unpadded.checked_dense_bytes(), None);

    let narrow = stack_footprint((u32::MAX, 1), &filters, &[], true);
    assert_eq!(narrow.checked_pixel_count(), Some((max + 500) * 501));
    assert_eq!(narrow.checked_raster_size(), None);
    let edge = stack_footprint((u32::MAX - 500, 1), &filters, &[], true);
    assert_eq!(edge.checked_raster_size(), Some((u32::MAX, 501)));

    // Resource policies belong to callers, including for grids beyond 30,000
    // per side / 400 MP. A footprint is not a projective-admission certificate.
    let large = stack_footprint((40_000, 40_000), &filters, &[], true);
    assert_eq!(large.checked_raster_size(), Some((40_500, 40_500)));
    assert_eq!(large.checked_pixel_count(), Some(1_640_250_000));
    assert_eq!(
        large.checked_dense_bytes(),
        usize::try_from(26_244_000_000u64)
            .ok()
            .filter(|bytes| *bytes <= isize::MAX as usize),
    );

    // Also distinguish arithmetic fitting usize from Vec's isize layout bound.
    if usize::BITS == 64 {
        let layout_overflow = stack_footprint((1 << 30, 1 << 30), &[], &[], true);
        assert_eq!(layout_overflow.checked_pixel_count(), Some(1 << 60));
        assert_eq!(layout_overflow.checked_dense_bytes(), None);
        let layout_overflow = stack_footprint((1 << 29, 1 << 30), &[], &[], true);
        assert_eq!(layout_overflow.checked_pixel_count(), Some(1 << 59));
        assert_eq!(layout_overflow.checked_dense_bytes(), None);
    }
    assert_grid(
        stack_footprint((0, 0), &filters, &[], true),
        (500, 500),
        250,
        true,
    );
    assert_eq!(
        stack_footprint((0, 0), &[], &[], true).checked_dense_bytes(),
        Some(0)
    );
}

// Keep the pre-refactor allocating sanitizer as a test-only compatibility
// oracle. Production uses one shared parameter/default traversal.
fn legacy_sanitized(filter: &Filter) -> Filter {
    let mut out = filter.clone();
    let default = Filter::catalogue()
        .into_iter()
        .find(|d| d.key() == filter.key());
    for spec in filter.params() {
        let value = if spec.value.is_finite() {
            spec.value
        } else {
            default
                .as_ref()
                .and_then(|d| d.params().into_iter().find(|s| s.key == spec.key))
                .map_or(spec.min, |s| s.value)
        };
        out.set_param(spec.key, value);
    }
    if let Filter::LensProfile {
        a,
        b,
        c,
        k1,
        k2,
        k3,
        scale,
        ..
    } = &mut out
    {
        for value in [a, b, c, k1, k2, k3] {
            if !value.is_finite() {
                *value = 0.0;
            }
        }
        if !scale.is_finite() {
            *scale = 1.0;
        }
    }
    out
}

// Explicit raw inputs cover every variant, including fields intentionally not
// exposed by params(). Do not use set_param here: it would clamp before testing.
fn hostile_variants(v: f32) -> Vec<Filter> {
    vec![
        Filter::GaussianBlur { radius: v },
        Filter::BoxBlur { radius: v },
        Filter::MotionBlur {
            angle: v,
            distance: v,
        },
        Filter::LensBlur { radius: v },
        Filter::UnsharpMask {
            amount: v,
            radius: v,
            threshold: v,
        },
        Filter::SmartSharpen {
            amount: v,
            radius: v,
        },
        Filter::AddNoise {
            amount: v,
            monochrome: false,
        },
        Filter::ReduceNoise {
            strength: v,
            detail: v,
        },
        Filter::HighPass { radius: v },
        Filter::LensCorrection {
            distortion: v,
            vignette: v,
        },
        Filter::LensProfile {
            a: v,
            b: v,
            c: v,
            k1: v,
            k2: v,
            k3: v,
            scale: v,
            distortion: v,
            vignette: v,
        },
        Filter::Emboss {
            angle: v,
            height: v,
            amount: v,
        },
        Filter::FindEdges,
        Filter::Invert,
        Filter::Pinch { amount: v },
        Filter::Twirl { angle: v },
        Filter::Wave {
            amplitude: v,
            wavelength: v,
        },
        Filter::Enhance { amount: v, sky: v },
        Filter::Structure {
            amount: v,
            softness: v,
        },
        Filter::Glow {
            amount: v,
            radius: v,
            threshold: v,
        },
        Filter::Orton {
            amount: v,
            radius: v,
        },
        Filter::Sunrays {
            x: v,
            y: v,
            amount: v,
            length: v,
            warmth: v,
        },
        Filter::Atmosphere {
            amount: v,
            spread: v,
        },
        Filter::SkinSmooth {
            amount: v,
            radius: v,
            detail: v,
        },
        Filter::GoldenHour { amount: v },
        Filter::Dramatic { amount: v },
    ]
}

#[test]
fn parameter_setters_keep_boundaries_and_nonfinite_behavior() {
    for filter in Filter::catalogue() {
        for param in filter.params() {
            for value in [
                param.min.next_down(),
                param.min,
                param.min.next_up(),
                param.max.next_down(),
                param.max,
                param.max.next_up(),
                -0.0,
                f32::NAN,
                f32::INFINITY,
                f32::NEG_INFINITY,
            ] {
                let mut changed = filter.clone();
                assert!(changed.set_param(param.key, value));
                let actual = changed
                    .params()
                    .into_iter()
                    .find(|p| p.key == param.key)
                    .unwrap();
                let clamped = value.clamp(param.min, param.max);
                let expected = if param.key == "monochrome" {
                    b2f(clamped >= 0.5)
                } else {
                    clamped
                };
                assert_eq!(
                    actual.value.to_bits(),
                    expected.to_bits(),
                    "{}: {} / {value}",
                    filter.key(),
                    param.key
                );
            }
        }
        let mut unchanged = filter.clone();
        assert!(!unchanged.set_param("unknown", 1.0));
        assert_eq!(unchanged, filter);
        if matches!(filter, Filter::LensProfile { .. }) {
            // These retained coefficients were never editable ParamSpecs.
            for key in ["a", "b", "c", "k1", "k2", "k3", "scale"] {
                assert!(!unchanged.set_param(key, 2.0));
                assert_eq!(unchanged, filter);
            }
        }
    }
}

#[test]
fn allocation_free_sanitization_matches_every_legacy_variant_and_boundary() {
    let catalogue = Filter::catalogue();
    let keys: Vec<_> = catalogue.iter().map(Filter::key).collect();
    assert_eq!(
        keys,
        hostile_variants(0.0)
            .iter()
            .map(Filter::key)
            .collect::<Vec<_>>()
    );
    let mut values = vec![
        f32::NAN,
        f32::INFINITY,
        f32::NEG_INFINITY,
        f32::MIN,
        f32::MAX,
        -0.0,
    ];
    for filter in &catalogue {
        assert_eq!(filter.sanitized(), *filter);
        for param in filter.params() {
            for boundary in [param.min, param.max, param.value] {
                values.extend([boundary.next_down(), boundary, boundary.next_up()]);
            }
        }
    }
    for value in values {
        for filter in hostile_variants(value) {
            let actual = filter.sanitized();
            let expected = legacy_sanitized(&filter);
            assert_eq!(actual, expected, "{}: {value}", filter.key());
            for (a, b) in actual.params().iter().zip(expected.params()) {
                assert_eq!(a.key, b.key);
                assert_eq!(
                    a.value.to_bits(),
                    b.value.to_bits(),
                    "{}: {}",
                    filter.key(),
                    a.key
                );
            }
            assert_eq!(
                stack_footprint((17, 9), std::slice::from_ref(&filter), &[], true).spread,
                expected.spread(),
                "{}: {value}",
                filter.key(),
            );
        }
    }
    for monochrome in [false, true] {
        let filter = Filter::AddNoise {
            amount: f32::NAN,
            monochrome,
        };
        assert_eq!(filter.sanitized(), legacy_sanitized(&filter));
    }
}

// Frozen pre-refactor stack/grid algorithm. Its independent padding and sum
// are intentional test oracles, never a second production footprint path.
fn legacy_apply_stack(
    source: &Raster,
    stack: &[Filter],
    styles: &[FilterStyle],
) -> (Raster, (i32, i32)) {
    if !stack
        .iter()
        .enumerate()
        .any(|(index, _)| styles.get(index).is_none_or(|style| style.enabled))
    {
        return (source.clone(), (0, 0));
    }
    let stack: Vec<Filter> = stack.iter().map(legacy_sanitized).collect();
    let spread = stack
        .iter()
        .enumerate()
        .filter(|(index, _)| styles.get(*index).is_none_or(|style| style.enabled))
        .map(|(_, filter)| filter.spread())
        .sum::<i32>()
        .min(MAX_SPREAD);
    let (source_w, source_h) = (source.width() as usize, source.height() as usize);
    let source_px: Vec<[f32; 4]> = source.rows_par(1, [0.0; 4], |row, dst| {
        for (p, o) in row.iter().zip(dst.iter_mut()) {
            *o = color::px_to_f(*p);
        }
    });
    let n = spread as usize;
    let (w, h) = (source_w + 2 * n, source_h + 2 * n);
    let mut px = vec![[0.0; 4]; w * h];
    for y in 0..source_h {
        px[(y + n) * w + n..(y + n) * w + n + source_w]
            .copy_from_slice(&source_px[y * source_w..(y + 1) * source_w]);
    }
    let mut img = Image { w, h, px };
    for (index, filter) in stack.iter().enumerate() {
        let style = styles.get(index).copied().unwrap_or_default().sanitized();
        if !style.enabled {
            continue;
        }
        let before = (style.opacity < 1.0 || style.blend != BlendMode::Normal).then(|| img.clone());
        img = apply_one(filter, img);
        if let Some(before) = before {
            let width = img.w;
            for (pixel, (filtered, base)) in img.px.iter_mut().zip(before.px.iter()).enumerate() {
                let noise = if style.blend == BlendMode::Dissolve {
                    dissolve_noise(
                        (pixel % width) as i32 - spread,
                        (pixel / width) as i32 - spread,
                        index as u64,
                    )
                } else {
                    0.0
                };
                let source = filtered.map(|channel| channel * style.opacity);
                *filtered = blend_px(style.blend, BlendSpace::Linear, *base, source, noise);
            }
        }
    }
    let out: Vec<_> = img
        .px
        .iter()
        .map(|p| color::f_to_px(p.map(|v| v.clamp(0.0, 1.0))))
        .collect();
    (
        Raster::from_pixels(img.w as u32, img.h as u32, [0; 4], &out),
        (-spread, -spread),
    )
}

#[test]
fn renderer_matches_prior_pixels_and_grid_for_styled_stacks() {
    let source = Raster::from_fn(7, 5, [0; 4], |x, y| {
        [x as u16 * 800, y as u16 * 700, 2000, 32768]
    });
    let filters = [
        Filter::GaussianBlur { radius: 1.25 },
        Filter::Invert,
        Filter::BoxBlur { radius: 0.5 },
    ];
    let dissolve = FilterStyle {
        opacity: 0.5,
        blend: BlendMode::Dissolve,
        ..Default::default()
    };
    let cases: &[(&[Filter], &[FilterStyle])] = &[
        (&[], &[]),
        (&filters, &[]),
        (&filters, &[FilterStyle::default(); 3]),
        (&filters, &[disabled(); 3]),
        (&filters, &[disabled()]),
        (
            &filters,
            &[FilterStyle {
                opacity: 0.0,
                ..Default::default()
            }],
        ),
        (&filters, &[disabled(), dissolve]),
        (&filters, &[dissolve, disabled(), dissolve]),
        (
            &filters,
            &[FilterStyle {
                opacity: f32::NAN,
                blend: BlendMode::PassThrough,
                ..Default::default()
            }],
        ),
        (&[Filter::GaussianBlur { radius: f32::NAN }], &[]),
        (
            &[
                Filter::Wave {
                    amplitude: 200.0,
                    wavelength: 60.0,
                },
                Filter::Wave {
                    amplitude: 200.0,
                    wavelength: 60.0,
                },
            ],
            &[],
        ),
    ];
    for &(stack, styles) in cases {
        let footprint = stack_footprint((source.width(), source.height()), stack, styles, true);
        let (expected, expected_offset) = legacy_apply_stack(&source, stack, styles);
        let (actual, actual_offset) = apply_stack_styled(&source, stack, styles);
        assert_eq!(actual_offset, expected_offset);
        assert_eq!(footprint.offset, expected_offset);
        assert_eq!(
            footprint.checked_raster_size(),
            Some((expected.width(), expected.height()))
        );
        assert_eq!(
            (actual.width(), actual.height()),
            (expected.width(), expected.height())
        );
        assert_eq!(
            actual.to_pixels(),
            expected.to_pixels(),
            "{stack:?} / {styles:?}"
        );
        if !footprint.active {
            assert_eq!(actual.content_id(), source.content_id());
        }
    }
}
