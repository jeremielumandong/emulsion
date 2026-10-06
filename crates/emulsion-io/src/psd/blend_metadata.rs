//! The bounded Blend If subset represented by one native tonal gate.
//!
//! PSD stores composite-gray ranges followed by channel-order ranges. Native
//! state can retain a single active Gray/Red/Green/Blue pair, not the product
//! of independently active pairs. Reject the latter instead of dropping gates.
//! See Adobe's "Layer blending ranges data" and ag-psd's BlendingRanges.

use ag_psd::psd::{BlendingRange as PsdRange, BlendingRanges, LayerAdditionalInfo};
use emulsion_raster::composite::{BlendIf, BlendIfChannel, BlendRange, BlendingOptions, Knockout};

fn range_in(values: &[f64]) -> Option<BlendRange> {
    if values.len() != 4
        || values
            .iter()
            .any(|v| !v.is_finite() || v.fract() != 0.0 || !(0.0..=255.0).contains(v))
    {
        return None;
    }
    let range = BlendRange {
        black: (values[0] / 255.0) as f32,
        black_fade: (values[1] / 255.0) as f32,
        white_fade: (values[2] / 255.0) as f32,
        white: (values[3] / 255.0) as f32,
    };
    range.valid().then_some(range)
}

/// `None` means appearance fallback is required. This accepts RGB channel
/// ordering only; callers must separately gate the document's color mode.
pub(super) fn import(info: &LayerAdditionalInfo) -> Option<BlendIf> {
    let Some(ranges) = &info.blending_ranges else {
        return Some(BlendIf::default());
    };
    // Real RGB(A) Photoshop files can carry a fourth, default channel range
    // (including the independently authored smartobject-layer.psd fixture).
    // The exact full-range pair is an identity regardless of its channel's
    // meaning. Do not map a non-neutral fourth range onto any native channel,
    // or extend this evidence to arbitrary additional channel records.
    if ranges.ranges.len() > 4
        || ranges.ranges.get(3).is_some_and(|range| {
            range.source_range != [0.0, 0.0, 255.0, 255.0]
                || range.dest_range != [0.0, 0.0, 255.0, 255.0]
        })
    {
        return None;
    }
    let mut result = BlendIf::default();
    let mut active = false;
    let gray = (
        range_in(&ranges.composite_gray_blend_source)?,
        range_in(&ranges.composite_graph_blend_destination_range)?,
    );
    let mut add = |channel, source, backdrop| {
        if source == BlendRange::default() && backdrop == BlendRange::default() {
            return Some(());
        }
        if active {
            return None;
        }
        active = true;
        result = BlendIf {
            channel,
            source,
            backdrop,
        };
        Some(())
    };
    add(BlendIfChannel::Gray, gray.0, gray.1)?;
    for (range, channel) in ranges.ranges.iter().zip([
        BlendIfChannel::Red,
        BlendIfChannel::Green,
        BlendIfChannel::Blue,
    ]) {
        add(
            channel,
            range_in(&range.source_range)?,
            range_in(&range.dest_range)?,
        )?;
    }
    Some(result)
}

fn range_out(range: BlendRange) -> Vec<f64> {
    [range.black, range.black_fade, range.white_fade, range.white]
        .map(|value| (f64::from(value) * 255.0).round())
        .to_vec()
}

/// Write one native gate without assigning its channel thresholds to gray.
/// A neutral gate canonicalizes to Gray on import, since PSD stores gates,
/// not the last channel selected in the Blending Options dialog.
pub(super) fn export(blend_if: BlendIf) -> Option<BlendingRanges> {
    if !blend_if.source.valid() || !blend_if.backdrop.valid() {
        return None;
    }
    let neutral = range_out(BlendRange::default());
    let mut result = BlendingRanges {
        composite_gray_blend_source: neutral.clone(),
        composite_graph_blend_destination_range: neutral.clone(),
        // Match the independently authored RGB(A) Photoshop record: Gray,
        // R/G/B, then a neutral fourth pair. Do not rely on Photoshop supplying
        // omitted per-channel defaults in shorter self-authored records.
        ranges: vec![
            PsdRange {
                source_range: neutral.clone(),
                dest_range: neutral,
            };
            4
        ],
    };
    if blend_if.channel == BlendIfChannel::Gray {
        result.composite_gray_blend_source = range_out(blend_if.source);
        result.composite_graph_blend_destination_range = range_out(blend_if.backdrop);
    } else {
        let channel = match blend_if.channel {
            BlendIfChannel::Red => 0,
            BlendIfChannel::Green => 1,
            BlendIfChannel::Blue => 2,
            BlendIfChannel::Gray => unreachable!(),
        };
        result.ranges[channel] = PsdRange {
            source_range: range_out(blend_if.source),
            dest_range: range_out(blend_if.backdrop),
        };
    }
    Some(result)
}

/// ag-psd 0.3 stores knockout as bool and would silently change Deep to
/// Shallow. A native Deep layer must retain its complete rendered appearance.
pub(super) fn needs_appearance(blending: &BlendingOptions) -> bool {
    blending.knockout == Knockout::Deep
        || !blending.blend_if.source.valid()
        || !blending.blend_if.backdrop.valid()
}

/// Inspect the raw knko block before ag-psd converts every nonzero value to
/// true. Independent Photoshop-authored fixtures use 0/1/2 for None/Shallow/
/// Deep. Unknown values and malformed padding also cannot become Shallow.
pub(super) fn knockout_record(bytes: &[u8]) -> Option<Knockout> {
    if bytes.len() != 4 || bytes[1..].iter().any(|byte| *byte != 0) {
        return None;
    }
    match bytes[0] {
        0 => Some(Knockout::None),
        1 => Some(Knockout::Shallow),
        2 => Some(Knockout::Deep),
        _ => None,
    }
}

#[cfg(test)]
pub(super) fn unsupported_knockout_record(bytes: &[u8]) -> bool {
    !matches!(
        knockout_record(bytes),
        Some(Knockout::None | Knockout::Shallow)
    )
}

#[cfg(test)]
#[path = "blend_metadata_tests.rs"]
mod tests;
