//! Independent third-party PSD metadata evidence, not a native-rendering golden.
//!
//! The untouched upstream fixture uses four active Gray/R/G/B gates and must
//! remain unsupported by our one-gate model. Byte-edited single-gate variants
//! below only check channel mapping; the authoring app did not render those variants.

use super::blend_metadata;
use ag_psd::psd::{ColorMode, LayerAdditionalInfo, Psd, ReadOptions};
use emulsion_raster::composite::{BlendIf, BlendIfChannel, BlendRange};
use sha2::{Digest, Sha256};

const SOURCE: &[u8] =
    include_bytes!("../../tests/fixtures/psd/blend-if/photoshop-blend-if-4b-roundtrip.psd");
const SOURCE_SHA256: &str = "ba61934630531a9fb7943d66872d16c9a60804a0cb9471b04a4a389d8c98e881";
// Offsets independently read from the original PSD layer-record structure.
const NORMAL_CHANNELS_OFFSET: usize = 23_416;
const NORMAL_RANGES_OFFSET: usize = 23_464;
const IDENTITY_PAIR: [u8; 8] = [0, 0, 255, 255, 0, 0, 255, 255];
// Pinned against the original bytes and upstream psd_structure_tests.cpp.
// Each entry is This Layer followed by Underlying Layer.
const EXPECTED_PAIRS: [[u8; 8]; 5] = [
    [11, 37, 201, 239, 19, 53, 187, 227], // Composite Gray
    [3, 33, 203, 233, 13, 43, 193, 223],  // Red
    [5, 35, 205, 235, 15, 45, 195, 225],  // Green
    [7, 37, 207, 237, 17, 47, 197, 227],  // Blue
    IDENTITY_PAIR,                        // Fourth per-channel pair
];

fn original_bytes() -> &'static [u8] {
    assert_eq!(SOURCE.len(), 26_210);
    assert_eq!(
        Sha256::digest(SOURCE)
            .iter()
            .map(|byte| format!("{byte:02x}"))
            .collect::<String>(),
        SOURCE_SHA256
    );
    SOURCE
}

fn decode_metadata(bytes: &[u8]) -> Psd {
    ag_psd::read_psd(
        bytes,
        &ReadOptions {
            skip_layer_image_data: Some(true),
            skip_composite_image_data: Some(true),
            skip_thumbnail: Some(true),
            skip_linked_files_data: Some(true),
            ..Default::default()
        },
    )
    .unwrap()
}

fn layer_info<'a>(psd: &'a Psd, name: &str) -> &'a LayerAdditionalInfo {
    &psd.children
        .as_ref()
        .unwrap()
        .iter()
        .find(|layer| layer.additional_info.name.as_deref() == Some(name))
        .unwrap()
        .additional_info
}

fn normalized_range(values: &[u8]) -> BlendRange {
    assert_eq!(values.len(), 4);
    let value = |index: usize| (f64::from(values[index]) / 255.0) as f32;
    BlendRange {
        black: value(0),
        black_fade: value(1),
        white_fade: value(2),
        white: value(3),
    }
}

#[test]
fn untouched_photoshop_fixture_pins_hash_range_bytes_and_alpha_first_records() {
    let bytes = original_bytes();
    assert_eq!(&bytes[..6], b"8BPS\0\x01");
    assert_eq!(&bytes[22..26], &[0, 8, 0, 3], "RGB, eight bits per channel");
    let creator = b"Adobe Photoshop 27.8 (Windows)";
    assert!(bytes.windows(creator.len()).any(|window| window == creator));
    assert_eq!(
        &bytes[NORMAL_RANGES_OFFSET - 4..NORMAL_RANGES_OFFSET],
        &[0, 0, 0, 40]
    );
    for (index, pair) in EXPECTED_PAIRS.iter().enumerate() {
        let start = NORMAL_RANGES_OFFSET + index * 8;
        assert_eq!(&bytes[start..start + 8], pair);
    }
    let channel_ids: Vec<i16> = (0..4)
        .map(|index| {
            let start = NORMAL_CHANNELS_OFFSET + index * 6;
            i16::from_be_bytes(bytes[start..start + 2].try_into().unwrap())
        })
        .collect();
    assert_eq!(channel_ids, [-1, 0, 1, 2]);
}

#[test]
fn photoshop_gray_rgb_and_neutral_tail_decode_but_multiple_gates_stay_unsupported() {
    let psd = decode_metadata(original_bytes());
    assert_eq!(psd.color_mode, Some(ColorMode::Rgb));
    assert_eq!(psd.bits_per_channel, Some(8.0));
    let info = layer_info(&psd, "Blend If Normal");
    let ranges = info.blending_ranges.as_ref().unwrap();
    assert_eq!(
        ranges.composite_gray_blend_source,
        EXPECTED_PAIRS[0][..4]
            .iter()
            .copied()
            .map(f64::from)
            .collect::<Vec<_>>()
    );
    assert_eq!(
        ranges.composite_graph_blend_destination_range,
        EXPECTED_PAIRS[0][4..]
            .iter()
            .copied()
            .map(f64::from)
            .collect::<Vec<_>>()
    );
    assert_eq!(ranges.ranges.len(), 4);
    for (range, expected) in ranges.ranges.iter().zip(&EXPECTED_PAIRS[1..]) {
        assert_eq!(
            range.source_range,
            expected[..4]
                .iter()
                .copied()
                .map(f64::from)
                .collect::<Vec<_>>()
        );
        assert_eq!(
            range.dest_range,
            expected[4..]
                .iter()
                .copied()
                .map(f64::from)
                .collect::<Vec<_>>()
        );
    }
    assert!(
        blend_metadata::import(info).is_none(),
        "the original four active gates cannot become one native gate"
    );
}

#[test]
fn byte_edited_single_gate_derivatives_verify_channel_mapping_only() {
    let original = original_bytes();
    for (selected, channel) in [
        BlendIfChannel::Gray,
        BlendIfChannel::Red,
        BlendIfChannel::Green,
        BlendIfChannel::Blue,
    ]
    .into_iter()
    .enumerate()
    {
        let mut bytes = original.to_vec();
        for (index, pair) in bytes[NORMAL_RANGES_OFFSET..NORMAL_RANGES_OFFSET + 40]
            .as_chunks_mut::<8>()
            .0
            .iter_mut()
            .enumerate()
        {
            pair.copy_from_slice(if index == selected {
                &EXPECTED_PAIRS[index]
            } else {
                &IDENTITY_PAIR
            });
        }
        let psd = decode_metadata(&bytes);
        let info = layer_info(&psd, "Blend If Normal");
        assert_eq!(
            blend_metadata::import(info),
            Some(BlendIf {
                channel,
                source: normalized_range(&EXPECTED_PAIRS[selected][..4]),
                backdrop: normalized_range(&EXPECTED_PAIRS[selected][4..]),
            })
        );
    }
}

#[test]
fn original_photoshop_neutral_fourth_channel_does_not_create_an_active_gate() {
    let psd = decode_metadata(original_bytes());
    let info = layer_info(&psd, "Background");
    assert_eq!(info.blending_ranges.as_ref().unwrap().ranges.len(), 4);
    assert_eq!(blend_metadata::import(info), Some(BlendIf::default()));
}
