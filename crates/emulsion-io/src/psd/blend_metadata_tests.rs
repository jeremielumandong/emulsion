use super::*;

fn source() -> BlendRange {
    BlendRange {
        black: 25.0 / 255.0,
        black_fade: 51.0 / 255.0,
        white_fade: 204.0 / 255.0,
        white: 230.0 / 255.0,
    }
}

fn info(ranges: BlendingRanges) -> LayerAdditionalInfo {
    LayerAdditionalInfo {
        blending_ranges: Some(ranges),
        ..Default::default()
    }
}

#[test]
fn each_single_active_channel_roundtrips_without_becoming_gray() {
    for channel in [
        BlendIfChannel::Gray,
        BlendIfChannel::Red,
        BlendIfChannel::Green,
        BlendIfChannel::Blue,
    ] {
        let gate = BlendIf {
            channel,
            source: source(),
            backdrop: source(),
        };
        let encoded = export(gate).unwrap();
        if channel != BlendIfChannel::Gray {
            assert_eq!(
                encoded.composite_gray_blend_source,
                [0.0, 0.0, 255.0, 255.0]
            );
            assert_eq!(encoded.ranges.len(), 4);
            assert_eq!(encoded.ranges[3].source_range, [0.0, 0.0, 255.0, 255.0]);
            assert_eq!(encoded.ranges[3].dest_range, [0.0, 0.0, 255.0, 255.0]);
        }
        assert_eq!(import(&info(encoded)), Some(gate));
    }
}

#[test]
fn absent_or_neutral_ranges_are_identity() {
    assert_eq!(
        import(&LayerAdditionalInfo::default()),
        Some(BlendIf::default())
    );
    for channel in [
        BlendIfChannel::Gray,
        BlendIfChannel::Red,
        BlendIfChannel::Blue,
    ] {
        let ranges = export(BlendIf {
            channel,
            ..Default::default()
        })
        .unwrap();
        assert_eq!(import(&info(ranges)), Some(BlendIf::default()));
    }
}

#[test]
fn combined_gray_and_channel_or_two_channels_require_appearance() {
    let gate = BlendIf {
        channel: BlendIfChannel::Red,
        source: source(),
        ..Default::default()
    };
    let mut ranges = export(gate).unwrap();
    ranges.composite_gray_blend_source = range_out(source());
    assert!(import(&info(ranges)).is_none());

    let mut ranges = export(gate).unwrap();
    ranges.ranges[1].dest_range = range_out(source());
    assert!(import(&info(ranges)).is_none());
}

#[test]
fn unknown_channel_and_malformed_ranges_require_appearance() {
    let mut ranges = export(BlendIf {
        channel: BlendIfChannel::Red,
        source: source(),
        ..Default::default()
    })
    .unwrap();
    ranges.ranges[3] = ranges.ranges[0].clone();
    assert!(import(&info(ranges)).is_none());

    for malformed in [
        vec![],
        vec![0.0, 0.0, 255.0],
        vec![0.0, 0.0, 255.0, 255.0, 255.0],
        vec![0.0, f64::NAN, 255.0, 255.0],
        vec![0.0, f64::INFINITY, 255.0, 255.0],
        vec![-1.0, 0.0, 255.0, 255.0],
        vec![0.0, 0.0, 255.0, 256.0],
        vec![0.0, 1.5, 255.0, 255.0],
        vec![51.0, 25.0, 204.0, 230.0],
    ] {
        let mut ranges = export(BlendIf::default()).unwrap();
        ranges.composite_gray_blend_source = malformed;
        assert!(import(&info(ranges)).is_none());
    }
}

#[test]
fn fourth_exact_identity_range_does_not_flatten_rgb_layers() {
    let gate = BlendIf {
        channel: BlendIfChannel::Red,
        source: source(),
        ..Default::default()
    };
    let mut ranges = export(gate).unwrap();
    assert_eq!(import(&info(ranges.clone())), Some(gate));
    let mut neutral = ranges.clone();
    neutral.ranges[0] = neutral.ranges[3].clone();
    assert_eq!(import(&info(neutral)), Some(BlendIf::default()));
    for malformed in [
        vec![0.0, 0.0, 254.0, 255.0],
        vec![0.0, 1.0, 255.0, 255.0],
        vec![0.0, 0.0, 255.0],
        vec![0.0, f64::NAN, 255.0, 255.0],
    ] {
        for backdrop in [false, true] {
            let mut bad = ranges.clone();
            if backdrop {
                bad.ranges[3].dest_range = malformed.clone();
            } else {
                bad.ranges[3].source_range = malformed.clone();
            }
            assert!(import(&info(bad)).is_none());
        }
    }
    ranges.ranges.push(ranges.ranges[3].clone());
    assert!(
        import(&info(ranges)).is_none(),
        "fifth range is not admitted"
    );
}

#[test]
fn quantization_retains_channel_and_stays_within_half_a_byte() {
    let gate = BlendIf {
        channel: BlendIfChannel::Green,
        source: BlendRange {
            black: 0.123,
            black_fade: 0.321,
            white_fade: 0.678,
            white: 0.987,
        },
        ..Default::default()
    };
    let restored = import(&info(export(gate).unwrap())).unwrap();
    assert_eq!(restored.channel, gate.channel);
    for (before, after) in [
        (gate.source.black, restored.source.black),
        (gate.source.black_fade, restored.source.black_fade),
        (gate.source.white_fade, restored.source.white_fade),
        (gate.source.white, restored.source.white),
    ] {
        assert!((before - after).abs() <= 0.5 / 255.0 + f32::EPSILON);
    }
}

#[test]
fn invalid_native_ranges_cannot_be_exported() {
    let mut gate = BlendIf::default();
    gate.source.black = f32::NAN;
    assert!(export(gate).is_none());
}

#[test]
fn deep_or_unknown_knockout_never_silently_becomes_shallow() {
    for bytes in [[0, 0, 0, 0], [1, 0, 0, 0]] {
        assert!(!unsupported_knockout_record(&bytes));
    }
    for bytes in [
        &[][..],
        &[2, 0, 0, 0],
        &[3, 0, 0, 0],
        &[1],
        &[0, 1, 0, 0],
        &[0, 0, 0, 0, 0],
    ] {
        assert!(unsupported_knockout_record(bytes));
    }
    let mut blending = BlendingOptions::default();
    assert!(!needs_appearance(&blending));
    blending.knockout = Knockout::Shallow;
    assert!(!needs_appearance(&blending));
    blending.knockout = Knockout::Deep;
    assert!(needs_appearance(&blending));
}

#[test]
fn typed_knockout_parser_preserves_all_three_documented_values() {
    for (value, expected) in [
        (0, Knockout::None),
        (1, Knockout::Shallow),
        (2, Knockout::Deep),
    ] {
        assert_eq!(knockout_record(&[value, 0, 0, 0]), Some(expected));
    }
    for bytes in [&[3, 0, 0, 0][..], &[2, 1, 0, 0], &[2], &[2, 0, 0, 0, 0]] {
        assert_eq!(knockout_record(bytes), None);
    }
}
