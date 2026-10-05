//! Small, read-only guards around the mask layouts ag-psd 0.3 cannot retain.
//!
//! This is not a second PSD decoder. It walks length-delimited base layer
//! and Lr16/Lr32 records without interpreting pixels or vector data. See Adobe's
//! Layer and Mask Information tables:
//! <https://www.adobe.com/devnet-apps/photoshop/fileformatashtml/>.
//! Real-mask headers precede parameters in actual Photoshop files, despite the
//! table's ordering, and are selected by channel -3, not the mask block length:
//! <https://github.com/psd-tools/psd-tools/issues/693>.

use std::fmt;

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub(super) enum GuardError {
    Malformed(&'static str),
    UnsupportedLayout(&'static str),
}

impl fmt::Display for GuardError {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            Self::Malformed(message) => write!(f, "malformed PSD: {message}"),
            Self::UnsupportedLayout(message) => write!(f, "unsupported PSD: {message}"),
        }
    }
}

type GuardResult<T> = std::result::Result<T, GuardError>;

#[derive(Clone, Copy)]
struct Cursor<'a>(&'a [u8]);

impl<'a> Cursor<'a> {
    fn take(&mut self, length: usize) -> GuardResult<&'a [u8]> {
        let (head, tail) = self
            .0
            .split_at_checked(length)
            .ok_or(GuardError::Malformed("truncated section"))?;
        self.0 = tail;
        Ok(head)
    }

    fn number(&mut self, length: usize) -> GuardResult<u64> {
        Ok(self
            .take(length)?
            .iter()
            .fold(0, |value, byte| (value << 8) | u64::from(*byte)))
    }

    fn length(&mut self, width: usize) -> GuardResult<usize> {
        usize::try_from(self.number(width)?)
            .map_err(|_| GuardError::Malformed("section length exceeds address space"))
    }

    fn section(&mut self, width: usize) -> GuardResult<&'a [u8]> {
        let length = self.length(width)?;
        self.take(length)
    }

    fn byte(&mut self) -> GuardResult<u8> {
        Ok(self.take(1)?[0])
    }

    fn signed_short(&mut self) -> GuardResult<i16> {
        let bytes = self.take(2)?;
        Ok(i16::from_be_bytes([bytes[0], bytes[1]]))
    }
}

struct Sections<'a> {
    length_width: usize,
    channels: usize,
    width: u32,
    height: u32,
    depth: u16,
    mode: u16,
    prefix: &'a [u8],
    layer_and_mask: &'a [u8],
    composite: &'a [u8],
}

fn sections(bytes: &[u8]) -> GuardResult<Sections<'_>> {
    let mut input = Cursor(bytes);
    if input.take(4)? != b"8BPS" {
        return Err(GuardError::Malformed("invalid signature"));
    }
    let length_width = match input.number(2)? {
        1 => 4,
        2 => 8,
        _ => return Err(GuardError::UnsupportedLayout("unknown file version")),
    };
    if input.take(6)?.iter().any(|byte| *byte != 0) {
        return Err(GuardError::Malformed("nonzero reserved header bytes"));
    }
    let channels = input.length(2)?;
    let height = input.number(4)? as u32;
    let width = input.number(4)? as u32;
    let depth = input.number(2)? as u16;
    let mode = input.number(2)? as u16;
    let maximum = if length_width == 8 { 300_000 } else { 30_000 };
    if channels == 0
        || channels > 56
        || width == 0
        || height == 0
        || width > maximum
        || height > maximum
    {
        return Err(GuardError::Malformed(
            "invalid image dimensions or channel count",
        ));
    }
    if !matches!(depth, 1 | 8 | 16 | 32) {
        return Err(GuardError::UnsupportedLayout("unknown sample depth"));
    }
    input.section(4)?; // Color-mode data stays byte-for-byte intact.
    input.section(4)?; // Resources, including version_info, stay intact too.
    let prefix_length = bytes.len() - input.0.len();
    let layer_and_mask = input.section(length_width)?;
    Ok(Sections {
        length_width,
        channels,
        width,
        height,
        depth,
        mode,
        prefix: &bytes[..prefix_length],
        layer_and_mask,
        composite: input.0,
    })
}

fn mask_flags_reason(flags: u8, real: bool) -> Option<&'static str> {
    if flags & 0x04 != 0 {
        Some(if real {
            "inverted real raster mask"
        } else {
            "inverted raster mask"
        })
    } else if flags & 0xe0 != 0 {
        Some("unknown raster-mask flags")
    } else if flags & 0x08 != 0 {
        Some("raster mask rendered from other data")
    } else {
        None
    }
}

fn mask_reason(mask: &[u8], has_real_channel: bool) -> GuardResult<Option<&'static str>> {
    if mask.is_empty() {
        return if has_real_channel {
            Err(GuardError::Malformed(
                "real-mask channel has no mask record",
            ))
        } else {
            Ok(None)
        };
    }
    let mut input = Cursor(mask);
    input.take(16)?; // Rectangle.
    let default_color = input.byte()?;
    let flags = input.byte()?;
    let mut reason = mask_flags_reason(flags, false);
    if !matches!(default_color, 0 | 255) {
        reason = reason.or(Some("non-binary raster-mask default color"));
    }
    // Only a -3 channel establishes that these bytes describe a real mask.
    // A long parameter block without -3 must never be examined as real flags.
    if has_real_channel {
        let real_flags = input.byte()?;
        let real_default_color = input.byte()?;
        input.take(16)?; // Real rectangle.
        reason = reason.or(mask_flags_reason(real_flags, true));
        if !matches!(real_default_color, 0 | 255) {
            reason = reason.or(Some("non-binary real raster-mask default color"));
        }
        reason = reason.or(Some("combined raster-mask channels"));
    }
    if flags & 0x10 != 0 {
        let parameters = input.byte()?;
        if parameters & 0xf0 != 0 {
            reason = reason.or(Some("unknown raster-mask parameter flags"));
        }
        for (flag, length) in [(1, 1), (2, 8), (4, 1), (8, 8)] {
            if parameters & flag != 0 {
                input.take(length)?;
            }
        }
        // ag-psd first consumes an 18-byte real-mask record whenever this
        // remainder is long enough, even if there is no -3 channel. It can
        // either fail or return wrong mask metadata, so guard both outcomes.
        if !has_real_channel && mask.len() >= 36 {
            reason = reason.or(Some(
                "raster-mask parameter layout requires appearance recovery",
            ));
        }
    }
    // Writers use either 2-byte or 4-byte padding. Larger unexplained payloads
    // are unsafe to drop, but do not establish an undocumented mask semantic.
    if input.0.len() > 3 || input.0.iter().any(|byte| *byte != 0) {
        reason = reason.or(Some("unrecognized raster-mask payload"));
    }
    Ok(reason)
}

/// Inspect PSD/PSB base and document-level Lr16/Lr32 layer records for mask
/// state that must not become an editable mask through ag-psd 0.3. `Some` calls
/// for an explicitly labelled saved-appearance import; `Err` means malformed
/// framing or an unsupported layout, not a discovered mask feature. This does
/// not validate every PSD feature or interpret vector records.
///
/// No early return is made after discovering a flag: all remaining records and
/// declared channel lengths are checked, so truncation cannot masquerade as an
/// unsupported but structurally sound mask.
pub(super) fn unsupported_mask_reason(bytes: &[u8]) -> GuardResult<Option<&'static str>> {
    let layout = sections(bytes)?;
    let mut reason = None;
    visit_layer_info(&layout, |data| {
        let found = scan_layer_records(data, layout.length_width)?;
        reason = reason.or(found);
        Ok(())
    })?;
    Ok(reason)
}

/// Visit the base layer-info body and any document-level high-depth bodies.
/// The tagged body starts at the signed layer count, without a second length.
/// `8B64` always carries an eight-byte size; in PSB the Adobe large-key list
/// does too, even under `8BIM`. Use the dependency's authoritative key list
/// (which additionally recognizes cinf) rather than misaligning later blocks.
fn visit_layer_info(
    layout: &Sections<'_>,
    mut visit: impl FnMut(&[u8]) -> GuardResult<()>,
) -> GuardResult<()> {
    let mut input = Cursor(layout.layer_and_mask);
    if input.0.is_empty() {
        return Ok(());
    }
    let base = input.section(layout.length_width)?;
    visit(base)?;
    input.take(base.len() % 2)?;
    if !input.0.is_empty() && !input.0.starts_with(b"8BIM") && !input.0.starts_with(b"8B64") {
        input.section(4)?; // Global layer mask, absent in some producers.
    }
    while !input.0.is_empty() {
        // Document-level blocks may have extra zero alignment bytes (including
        // four-byte padding); never search arbitrary bytes for a signature.
        while input.0.first() == Some(&0) {
            input.take(1)?;
        }
        if input.0.is_empty() {
            break;
        }
        let signature = input.take(4)?;
        if signature != b"8BIM" && signature != b"8B64" {
            return Err(GuardError::Malformed(
                "invalid document tagged-block signature",
            ));
        }
        let key = std::str::from_utf8(input.take(4)?)
            .map_err(|_| GuardError::Malformed("invalid document tagged-block key"))?;
        let wide = signature == b"8B64"
            || (layout.length_width == 8 && ag_psd::additional_info::is_large_key(key));
        let body = input.section(if wide { 8 } else { 4 })?;
        if matches!(key, "Lr16" | "Lr32") {
            if body.is_empty() {
                return Err(GuardError::Malformed("empty alternative layer-info record"));
            }
            visit(body)?;
        }
        input.take(body.len() % 2)?;
    }
    Ok(())
}

fn has_merged_alpha(layout: &Sections<'_>) -> GuardResult<bool> {
    let mut alpha = false;
    visit_layer_info(layout, |data| {
        if !data.is_empty() {
            alpha |= Cursor(data).signed_short()? < 0;
        }
        Ok(())
    })?;
    Ok(alpha)
}

fn scan_layer_records(data: &[u8], length_width: usize) -> GuardResult<Option<&'static str>> {
    let mut input = Cursor(data);
    if input.0.is_empty() {
        return Ok(None);
    }
    let count = usize::from(input.signed_short()?.unsigned_abs());
    let mut channel_bytes = 0usize;
    let mut reason = None;
    for _ in 0..count {
        input.take(16)?;
        let channels = input.length(2)?;
        if channels > 56 {
            return Err(GuardError::UnsupportedLayout(
                "layer channel count exceeds 56",
            ));
        }
        let mut has_real_channel = false;
        let mut has_user_channel = false;
        for _ in 0..channels {
            let id = input.signed_short()?;
            has_real_channel |= id == -3;
            has_user_channel |= id == -2;
            let length = input.length(length_width)?;
            channel_bytes = channel_bytes
                .checked_add(length)
                .ok_or(GuardError::Malformed("channel lengths overflow"))?;
        }
        if input.take(4)? != b"8BIM" {
            return Err(GuardError::Malformed("invalid layer blend signature"));
        }
        input.take(8)?; // Blend key, opacity, clipping, flags and filler.
        let mut extra = Cursor(input.section(4)?);
        let mask = extra.section(4)?;
        if has_user_channel && mask.is_empty() {
            return Err(GuardError::Malformed(
                "raster-mask channel has no mask record",
            ));
        }
        let found = mask_reason(mask, has_real_channel)?;
        reason = reason.or(found);
        extra.section(4)?; // Blending ranges.
        let name_length = usize::from(extra.byte()?);
        extra.take(name_length)?;
        extra.take((4 - (name_length + 1) % 4) % 4)?;
        // Additional tagged records belong to the decoder, not this scanner.
    }
    input.take(channel_bytes)?;
    // The enclosing section already bounds any final padding. Per-layer
    // vector/tagged records are not interpreted as another layer-info body.
    Ok(reason)
}

/// Make an in-memory decoding candidate with the original header, color data,
/// resources and saved composite, and an empty layer/mask section. No pixels or
/// layer records are synthesized. The caller must decode this candidate, reject
/// version_info.has_real_merged_data == false, and clearly label the result as
/// flattened appearance with source layers/masks unavailable.
///
/// This deliberately excludes alpha/spot channels: stripping a negative layer
/// count loses ag-psd's merged-alpha/white-matte interpretation. It also excludes
/// higher sample depths and modes whose composite decoding differs. Raw and RLE
/// completeness are checked here because ag-psd tolerates short PackBits rows;
/// ZIP stream integrity is checked by ag-psd's exact-size composite ZIP reader.
pub(super) fn saved_composite_only(bytes: &[u8]) -> GuardResult<Vec<u8>> {
    let layout = sections(bytes)?;
    if layout.depth != 8 || !matches!((layout.mode, layout.channels), (3, 3) | (1, 1)) {
        return Err(GuardError::UnsupportedLayout(
            "appearance recovery requires 8-bit RGB with three channels or grayscale with one channel; alpha/spot channels cannot be preserved",
        ));
    }
    crate::import::check_size(layout.width, layout.height).map_err(|_| {
        GuardError::UnsupportedLayout("appearance recovery dimensions exceed image limits")
    })?;
    if has_merged_alpha(&layout)? {
        return Err(GuardError::UnsupportedLayout(
            "appearance recovery cannot preserve merged alpha",
        ));
    }
    validate_composite(&layout)?;
    let length = layout
        .prefix
        .len()
        .checked_add(layout.length_width)
        .and_then(|length| length.checked_add(layout.composite.len()))
        .ok_or(GuardError::Malformed("composite length overflow"))?;
    let mut output = Vec::new();
    output.try_reserve_exact(length).map_err(|_| {
        GuardError::UnsupportedLayout("appearance recovery allocation is too large")
    })?;
    output.extend_from_slice(layout.prefix);
    // The only new bytes are a documented zero layer/mask section length.
    output.extend_from_slice(&[0; 8][..layout.length_width]);
    output.extend_from_slice(layout.composite);
    Ok(output)
}

/// Validate the original saved composite before using an already-decoded
/// appearance. Unlike `saved_composite_only`, this does not discard layer or
/// merged-alpha metadata, and accepts RGB/RGBA and grayscale/gray-alpha at
/// 8/16/32 bits. An extra composite plane requires a negative layer count in
/// the base or Lr16/Lr32 records, establishing merged alpha rather than an
/// auxiliary channel. It validates completeness without changing pixel values.
/// ZIP integrity still depends on the successful exact-size ag-psd decode;
/// raw/RLE require this additional check because short rows are tolerated there.
pub(super) fn validate_saved_composite(bytes: &[u8]) -> GuardResult<()> {
    validate_composite(&sections(bytes)?)
}

fn validate_composite(layout: &Sections<'_>) -> GuardResult<()> {
    if !matches!(layout.depth, 8 | 16 | 32)
        || !matches!((layout.mode, layout.channels), (3, 3 | 4) | (1, 1 | 2))
    {
        return Err(GuardError::UnsupportedLayout(
            "saved appearance requires 8/16/32-bit RGB or grayscale with at most one alpha channel",
        ));
    }
    crate::import::check_size(layout.width, layout.height).map_err(|_| {
        GuardError::UnsupportedLayout("saved appearance dimensions exceed image limits")
    })?;
    let alpha = has_merged_alpha(layout)?;
    let base_channels = if layout.mode == 3 { 3 } else { 1 };
    if layout.channels > base_channels && !alpha {
        return Err(GuardError::UnsupportedLayout(
            "extra composite channel is not identified as merged alpha",
        ));
    }
    if layout.channels == base_channels && alpha {
        return Err(GuardError::Malformed(
            "merged alpha has no composite channel",
        ));
    }
    let mut input = Cursor(layout.composite);
    let compression = input.number(2)?;
    let width = usize::try_from(layout.width)
        .map_err(|_| GuardError::Malformed("width exceeds address space"))?;
    let height = usize::try_from(layout.height)
        .map_err(|_| GuardError::Malformed("height exceeds address space"))?;
    let row_bytes = width
        .checked_mul(usize::from(layout.depth / 8))
        .ok_or(GuardError::Malformed("composite row byte count overflow"))?;
    let rows = height
        .checked_mul(layout.channels)
        .ok_or(GuardError::Malformed("composite row count overflow"))?;
    match compression {
        0 => {
            let samples = row_bytes
                .checked_mul(rows)
                .ok_or(GuardError::Malformed("composite sample count overflow"))?;
            input.take(samples)?;
        }
        1 => {
            let entry_width = if layout.length_width == 8 { 4 } else { 2 };
            let table_size = rows
                .checked_mul(entry_width)
                .ok_or(GuardError::Malformed("composite row table overflow"))?;
            let mut lengths = Cursor(input.take(table_size)?);
            for _ in 0..rows {
                let length = lengths.length(entry_width)?;
                let mut row = Cursor(input.take(length)?);
                let mut decoded = 0usize;
                while !row.0.is_empty() {
                    let code = row.byte()?;
                    let count = match code {
                        0..=127 => {
                            let count = usize::from(code) + 1;
                            row.take(count)?;
                            count
                        }
                        128 => 0,
                        129..=255 => {
                            row.take(1)?;
                            257 - usize::from(code)
                        }
                    };
                    decoded = decoded
                        .checked_add(count)
                        .ok_or(GuardError::Malformed("composite RLE row overflow"))?;
                    if decoded > row_bytes {
                        return Err(GuardError::Malformed("composite RLE row is too long"));
                    }
                }
                if decoded != row_bytes {
                    return Err(GuardError::Malformed("composite RLE row is truncated"));
                }
            }
        }
        2 | 3 => {
            if input.0.is_empty() {
                return Err(GuardError::Malformed("missing composite ZIP stream"));
            }
        }
        _ => {
            return Err(GuardError::UnsupportedLayout(
                "unknown composite compression",
            ));
        }
    }
    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;

    // Independently specified binary records, deliberately not written by
    // ag-psd: otherwise reader/writer layout mistakes can cancel each other.
    fn length(output: &mut Vec<u8>, value: usize, psb: bool) {
        if psb {
            output.extend_from_slice(&(value as u64).to_be_bytes());
        } else {
            output.extend_from_slice(&(value as u32).to_be_bytes());
        }
    }

    fn rectangle(output: &mut Vec<u8>) {
        for value in [0i32, 0, 1, 1] {
            output.extend_from_slice(&value.to_be_bytes());
        }
    }

    fn mask(flags: u8, real_flags: Option<u8>, parameters: &[u8]) -> Vec<u8> {
        let mut output = Vec::new();
        rectangle(&mut output);
        output.extend_from_slice(&[255, flags]);
        if let Some(flags) = real_flags {
            output.extend_from_slice(&[flags, 255]);
            rectangle(&mut output);
        }
        output.extend_from_slice(parameters);
        while output.len() % 4 != 0 {
            output.push(0);
        }
        output
    }

    fn file(
        psb: bool,
        mask: &[u8],
        real_channel: bool,
        resources: &[u8],
        composite: &[u8],
    ) -> Vec<u8> {
        file_at_depth(psb, mask, real_channel, resources, composite, 8)
    }

    fn file_at_depth(
        psb: bool,
        mask: &[u8],
        real_channel: bool,
        resources: &[u8],
        composite: &[u8],
        depth: u16,
    ) -> Vec<u8> {
        let sample_bytes = usize::from(depth / 8);
        let mut record = Vec::new();
        rectangle(&mut record);
        let channels: &[i16] = if real_channel { &[-2, -3] } else { &[-2] };
        record.extend_from_slice(&(channels.len() as u16).to_be_bytes());
        for id in channels {
            record.extend_from_slice(&id.to_be_bytes());
            length(&mut record, 2 + sample_bytes, psb); // raw compression and one sample
        }
        record.extend_from_slice(b"8BIMnorm\xff\0\0\0");
        let mut extra = Vec::new();
        length(&mut extra, mask.len(), false);
        extra.extend_from_slice(mask);
        extra.extend_from_slice(&[0; 4]); // empty blending ranges
        extra.extend_from_slice(b"\x01M\0\0"); // padded Pascal name
        length(&mut record, extra.len(), false);
        record.extend_from_slice(&extra);
        let mut info = vec![0, 1]; // one layer, no merged alpha
        info.extend_from_slice(&record);
        for _ in channels {
            info.extend_from_slice(&[0, 0]);
            info.extend(std::iter::repeat_n(127, sample_bytes));
        }
        if info.len() % 2 != 0 {
            info.push(0);
        }
        let mut layers = Vec::new();
        length(&mut layers, info.len(), psb);
        layers.extend_from_slice(&info);
        layers.extend_from_slice(&[0; 4]); // empty global layer mask
        let mut output = b"8BPS".to_vec();
        output.extend_from_slice(&(if psb { 2u16 } else { 1u16 }).to_be_bytes());
        output.extend_from_slice(&[0; 6]);
        output.extend_from_slice(&3u16.to_be_bytes());
        output.extend_from_slice(&1u32.to_be_bytes());
        output.extend_from_slice(&1u32.to_be_bytes());
        output.extend_from_slice(&depth.to_be_bytes());
        output.extend_from_slice(&3u16.to_be_bytes());
        output.extend_from_slice(&[0; 4]); // empty color-mode data
        length(&mut output, resources.len(), false);
        output.extend_from_slice(resources);
        length(&mut output, layers.len(), psb);
        output.extend_from_slice(&layers);
        output.extend_from_slice(composite);
        output
    }

    const RAW: &[u8] = &[0, 0, 20, 80, 140];

    #[test]
    fn plain_masks_allow_link_and_disable_flags_in_psd_and_psb() {
        for psb in [false, true] {
            for flags in 0..4 {
                assert_eq!(
                    unsupported_mask_reason(&file(psb, &mask(flags, None, &[]), false, &[], RAW)),
                    Ok(None)
                );
            }
        }
    }

    #[test]
    fn primary_and_real_inversion_are_not_silently_ignored() {
        for psb in [false, true] {
            let primary = file(psb, &mask(4, None, &[]), false, &[], RAW);
            assert_eq!(
                unsupported_mask_reason(&primary),
                Ok(Some("inverted raster mask"))
            );
            let real = file(psb, &mask(0, Some(4), &[]), true, &[], RAW);
            assert_eq!(
                unsupported_mask_reason(&real),
                Ok(Some("inverted real raster mask"))
            );
        }
    }

    #[test]
    fn unknown_and_rendered_mask_flags_request_saved_appearance() {
        for flags in [0x08, 0x20, 0x40, 0x80] {
            let bytes = file(false, &mask(flags, None, &[]), false, &[], RAW);
            assert!(matches!(unsupported_mask_reason(&bytes), Ok(Some(_))));
        }
        let bytes = file(false, &mask(0x10, None, &[0x80]), false, &[], RAW);
        assert_eq!(
            unsupported_mask_reason(&bytes),
            Ok(Some("unknown raster-mask parameter flags"))
        );
    }

    #[test]
    fn gray_mask_defaults_require_appearance_and_channel_counts_are_bounded() {
        for psb in [false, true] {
            let mut primary = mask(0, None, &[]);
            primary[16] = 128;
            assert_eq!(
                unsupported_mask_reason(&file(psb, &primary, false, &[], RAW)),
                Ok(Some("non-binary raster-mask default color"))
            );
            let mut real = mask(0, Some(0), &[]);
            real[19] = 128;
            assert_eq!(
                unsupported_mask_reason(&file(psb, &real, true, &[], RAW)),
                Ok(Some("non-binary real raster-mask default color"))
            );
            let mut bytes = file(psb, &mask(0, None, &[]), false, &[], RAW);
            let offset = if psb { 68 } else { 60 };
            bytes[offset..offset + 2].copy_from_slice(&57u16.to_be_bytes());
            assert_eq!(
                unsupported_mask_reason(&bytes),
                Err(GuardError::UnsupportedLayout(
                    "layer channel count exceeds 56"
                ))
            );
        }
    }

    #[test]
    fn long_parameters_without_minus_three_are_not_real_flags() {
        // The first parameter byte deliberately has inversion's bit set, but
        // represents parameter presence, not the real-mask flag byte.
        let mut parameters = vec![0x0f, 200];
        parameters.extend_from_slice(&3.0f64.to_be_bytes());
        parameters.push(191);
        parameters.extend_from_slice(&6.0f64.to_be_bytes());
        for psb in [false, true] {
            let bytes = file(psb, &mask(0x10, None, &parameters), false, &[], RAW);
            assert_eq!(
                unsupported_mask_reason(&bytes),
                Ok(Some(
                    "raster-mask parameter layout requires appearance recovery"
                ))
            );
            let with_real = file(psb, &mask(0x10, Some(4), &parameters), true, &[], RAW);
            assert_eq!(
                unsupported_mask_reason(&with_real),
                Ok(Some("inverted real raster mask"))
            );
        }
    }

    #[test]
    fn short_user_parameters_keep_the_editable_path() {
        let mut parameters = vec![3, 128];
        parameters.extend_from_slice(&2.5f64.to_be_bytes());
        let bytes = file(false, &mask(0x10, None, &parameters), false, &[], RAW);
        assert_eq!(unsupported_mask_reason(&bytes), Ok(None));
    }

    #[test]
    fn incomplete_mask_fields_are_malformed_even_when_flags_are_unsupported() {
        let mut short = mask(4, None, &[]);
        short.truncate(17);
        let cases = [
            (short, false),
            (mask(0, None, &[]), true),
            (mask(0x14, None, &[2, 0]), false),
        ];
        for (mask, real) in cases {
            assert!(matches!(
                unsupported_mask_reason(&file(false, &mask, real, &[], RAW)),
                Err(GuardError::Malformed(_))
            ));
        }
    }

    #[test]
    fn orphan_mask_channel_and_extreme_layer_count_are_malformed() {
        let bytes = file(false, &[], false, &[], RAW);
        assert!(matches!(
            unsupported_mask_reason(&bytes),
            Err(GuardError::Malformed(_))
        ));
        let mut bytes = file(false, &mask(0, None, &[]), false, &[], RAW);
        bytes[42..44].copy_from_slice(&i16::MIN.to_be_bytes());
        assert!(matches!(
            unsupported_mask_reason(&bytes),
            Err(GuardError::Malformed(_))
        ));
    }

    #[test]
    fn resources_and_composite_are_preserved_exactly() {
        // A version-info resource (1057), false hasRealMergedData, empty
        // UTF-16 writer/reader strings, and fileVersion=1. Retaining this lets
        // the caller reject an absent genuine merged image after decoding.
        let mut resources = b"8BIM\x04\x21\0\0\0\0\0\x11".to_vec();
        resources.extend_from_slice(&[0, 0, 0, 1, 0]);
        resources.extend_from_slice(&[0; 8]);
        resources.extend_from_slice(&[0, 0, 0, 1, 0]); // data plus even padding
        for psb in [false, true] {
            let original = file(psb, &mask(4, None, &[]), false, &resources, RAW);
            let stripped = saved_composite_only(&original).unwrap();
            let old = sections(&original).unwrap();
            let new = sections(&stripped).unwrap();
            assert_eq!(new.prefix, old.prefix);
            assert_eq!(new.composite, RAW);
            assert!(new.layer_and_mask.is_empty());
            assert_eq!(
                stripped.len(),
                old.prefix.len() + old.length_width + RAW.len()
            );
            assert_eq!(unsupported_mask_reason(&stripped), Ok(None));
        }
    }

    #[test]
    fn every_truncated_prefix_is_rejected_by_recovery() {
        for psb in [false, true] {
            let bytes = file(psb, &mask(4, None, &[]), false, &[], RAW);
            for end in 0..bytes.len() {
                assert!(
                    saved_composite_only(&bytes[..end]).is_err(),
                    "accepted prefix {end}"
                );
            }
            let composite_start = bytes.len() - RAW.len();
            for end in 0..composite_start {
                assert!(
                    unsupported_mask_reason(&bytes[..end]).is_err(),
                    "accepted layer prefix {end}"
                );
            }
        }
    }

    #[test]
    fn huge_lengths_fail_without_allocation_or_overflow() {
        for psb in [false, true] {
            let mut bytes = file(psb, &mask(0, None, &[]), false, &[], RAW);
            let width = if psb { 8 } else { 4 };
            bytes[34..34 + width].fill(0xff);
            assert!(matches!(
                unsupported_mask_reason(&bytes),
                Err(GuardError::Malformed(_))
            ));
            assert!(matches!(
                saved_composite_only(&bytes),
                Err(GuardError::Malformed(_))
            ));
        }
        let mut bytes = file(true, &mask(0, None, &[]), false, &[], RAW);
        // Start of first channel length: header 34, two PSB length fields,
        // layer count 2, rectangle 16, channel count 2, channel ID 2.
        bytes[72..80].fill(0xff);
        assert!(matches!(
            unsupported_mask_reason(&bytes),
            Err(GuardError::Malformed(_))
        ));
    }

    #[test]
    fn alpha_spots_high_depth_and_non_rgb_modes_are_not_recovered() {
        let original = file(false, &mask(4, None, &[]), false, &[], RAW);
        for (channels, depth, mode) in [
            (4u16, 8u16, 3u16),
            (5, 8, 3),
            (2, 8, 1),
            (3, 16, 3),
            (4, 8, 4),
        ] {
            let mut bytes = original.clone();
            bytes[12..14].copy_from_slice(&channels.to_be_bytes());
            bytes[22..24].copy_from_slice(&depth.to_be_bytes());
            bytes[24..26].copy_from_slice(&mode.to_be_bytes());
            assert!(matches!(
                saved_composite_only(&bytes),
                Err(GuardError::UnsupportedLayout(_))
            ));
        }
        let mut negative_count = original;
        negative_count[42..44].copy_from_slice(&(-1i16).to_be_bytes());
        assert!(matches!(
            saved_composite_only(&negative_count),
            Err(GuardError::UnsupportedLayout(_))
        ));
    }

    #[test]
    fn one_channel_grayscale_is_recoverable() {
        let mut bytes = file(false, &mask(4, None, &[]), false, &[], &[0, 0, 77]);
        bytes[12..14].copy_from_slice(&1u16.to_be_bytes());
        bytes[24..26].copy_from_slice(&1u16.to_be_bytes());
        assert!(saved_composite_only(&bytes).is_ok());
    }

    #[test]
    fn missing_composites_and_short_rle_rows_are_rejected() {
        for psb in [false, true] {
            for composite in [
                &[][..],
                &[0][..],
                &[0, 0][..],
                &[0, 1][..],
                &[0, 2][..],
                &[0, 3][..],
            ] {
                assert!(
                    saved_composite_only(&file(psb, &mask(0, None, &[]), false, &[], composite))
                        .is_err()
                );
            }
            let mut composite = vec![0, 1];
            for _ in 0..3 {
                if psb {
                    composite.extend_from_slice(&2u32.to_be_bytes());
                } else {
                    composite.extend_from_slice(&2u16.to_be_bytes());
                }
            }
            composite.extend_from_slice(&[0, 20, 0, 80, 0, 140]);
            assert!(
                saved_composite_only(&file(psb, &mask(0, None, &[]), false, &[], &composite))
                    .is_ok()
            );
            // Valid byte lengths, but first row emits no pixels at all.
            let first_row = if psb { 14 } else { 8 };
            composite[first_row..first_row + 2].fill(128);
            assert!(matches!(
                saved_composite_only(&file(psb, &mask(0, None, &[]), false, &[], &composite)),
                Err(GuardError::Malformed(_))
            ));
        }
    }

    fn tagged_file(
        psb: bool,
        key: &[u8; 4],
        signature: &[u8; 4],
        flags: u8,
        alpha: bool,
        body_limit: Option<usize>,
    ) -> Vec<u8> {
        let depth = if key == b"Lr16" { 16 } else { 32 };
        let channels = if alpha { 4 } else { 3 };
        let mut composite = vec![0, 0];
        composite.extend(std::iter::repeat_n(127, channels * usize::from(depth / 8)));
        let original = file_at_depth(psb, &mask(flags, None, &[]), false, &[], &composite, depth);
        let layout = sections(&original).unwrap();
        let mut body = Cursor(layout.layer_and_mask)
            .section(layout.length_width)
            .unwrap()
            .to_vec();
        if alpha {
            body[..2].copy_from_slice(&(-1i16).to_be_bytes());
        }
        if let Some(limit) = body_limit {
            body.truncate(limit);
        }
        let mut layers = Vec::new();
        length(&mut layers, 0, psb); // Empty base list: actual layers are tagged.
        layers.extend_from_slice(&[0; 4]); // Empty global mask.
        layers.extend_from_slice(b"8BIMabcd\0\0\0\x03\x09\x08\x07\0");
        layers.extend_from_slice(&[0; 2]); // Additional global alignment.
        layers.extend_from_slice(signature);
        layers.extend_from_slice(key);
        length(&mut layers, body.len(), psb || signature == b"8B64");
        layers.extend_from_slice(&body);
        if body.len() % 2 != 0 {
            layers.push(0);
        }
        let mut output = layout.prefix.to_vec();
        output[12..14].copy_from_slice(&(channels as u16).to_be_bytes());
        length(&mut output, layers.len(), psb);
        output.extend_from_slice(&layers);
        output.extend_from_slice(&composite);
        output
    }

    #[test]
    fn alternative_high_depth_records_cannot_hide_mask_flags() {
        for psb in [false, true] {
            for key in [b"Lr16", b"Lr32"] {
                for signature in [b"8BIM", b"8B64"] {
                    for (flags, expected) in [
                        (0, None),
                        (4, Some("inverted raster mask")),
                        (0x80, Some("unknown raster-mask flags")),
                    ] {
                        let bytes = tagged_file(psb, key, signature, flags, false, None);
                        assert_eq!(unsupported_mask_reason(&bytes), Ok(expected));
                        assert_eq!(validate_saved_composite(&bytes), Ok(()));
                    }
                }
            }
        }
    }

    #[test]
    fn alternative_record_truncation_is_malformed_even_after_inversion() {
        for psb in [false, true] {
            for key in [b"Lr16", b"Lr32"] {
                for signature in [b"8BIM", b"8B64"] {
                    let full = tagged_file(psb, key, signature, 4, false, None);
                    let mut body_size = 0;
                    visit_layer_info(&sections(&full).unwrap(), |body| {
                        body_size = body_size.max(body.len());
                        Ok(())
                    })
                    .unwrap();
                    for limit in [0, 1, 18, 30, 50, body_size - 2] {
                        let bytes = tagged_file(psb, key, signature, 4, false, Some(limit));
                        assert!(matches!(
                            unsupported_mask_reason(&bytes),
                            Err(GuardError::Malformed(_))
                        ));
                    }
                    let mut bytes = tagged_file(psb, key, signature, 4, false, None);
                    let key_offset = bytes.windows(4).position(|word| word == key).unwrap();
                    let width = if psb || signature == b"8B64" { 8 } else { 4 };
                    bytes[key_offset + 4..key_offset + 4 + width].fill(0xff);
                    assert!(matches!(
                        unsupported_mask_reason(&bytes),
                        Err(GuardError::Malformed(_))
                    ));
                }
            }
        }
    }

    #[test]
    fn extra_original_composite_plane_requires_actual_merged_alpha() {
        for psb in [false, true] {
            for (mode, channels) in [(3u16, 4u16), (1, 2)] {
                for depth in [8u16, 16, 32] {
                    let mut composite = vec![0, 0];
                    composite.extend(std::iter::repeat_n(
                        127,
                        usize::from(channels * (depth / 8)),
                    ));
                    let mut bytes =
                        file_at_depth(psb, &mask(4, None, &[]), false, &[], &composite, depth);
                    bytes[12..14].copy_from_slice(&channels.to_be_bytes());
                    bytes[24..26].copy_from_slice(&mode.to_be_bytes());
                    assert!(matches!(
                        validate_saved_composite(&bytes),
                        Err(GuardError::UnsupportedLayout(_))
                    ));
                    let count_offset = if psb { 50 } else { 42 };
                    bytes[count_offset..count_offset + 2].copy_from_slice(&(-1i16).to_be_bytes());
                    assert_eq!(validate_saved_composite(&bytes), Ok(()));
                    assert!(saved_composite_only(&bytes).is_err());
                    bytes.pop();
                    assert!(matches!(
                        validate_saved_composite(&bytes),
                        Err(GuardError::Malformed(_))
                    ));
                }
            }
        }
    }

    #[test]
    fn alternative_negative_count_establishes_original_merged_alpha() {
        for psb in [false, true] {
            for key in [b"Lr16", b"Lr32"] {
                let bytes = tagged_file(psb, key, b"8BIM", 4, true, None);
                assert_eq!(validate_saved_composite(&bytes), Ok(()));
                let layout = sections(&bytes).unwrap();
                assert!(has_merged_alpha(&layout).unwrap());
            }
        }
    }

    #[test]
    fn original_rgba_rle_requires_complete_sample_bytes_in_every_plane() {
        for psb in [false, true] {
            for depth in [8u16, 16, 32] {
                let sample_bytes = usize::from(depth / 8);
                let mut composite = vec![0, 1];
                for _ in 0..4 {
                    // Row counts are always PSD u16 / PSB u32, independently
                    // of sample depth. Each row has one literal packet.
                    if psb {
                        composite.extend_from_slice(&((sample_bytes + 1) as u32).to_be_bytes());
                    } else {
                        composite.extend_from_slice(&((sample_bytes + 1) as u16).to_be_bytes());
                    }
                }
                let first_row = composite.len();
                for _ in 0..4 {
                    composite.push((sample_bytes - 1) as u8);
                    composite.extend(std::iter::repeat_n(127, sample_bytes));
                }
                let mut bytes =
                    file_at_depth(psb, &mask(4, None, &[]), false, &[], &composite, depth);
                bytes[12..14].copy_from_slice(&4u16.to_be_bytes());
                let count_offset = if psb { 50 } else { 42 };
                bytes[count_offset..count_offset + 2].copy_from_slice(&(-1i16).to_be_bytes());
                assert_eq!(validate_saved_composite(&bytes), Ok(()));
                let start = bytes.len() - composite.len() + first_row;
                let mut no_op = bytes.clone();
                no_op[start..start + sample_bytes + 1].fill(128);
                assert!(matches!(
                    validate_saved_composite(&no_op),
                    Err(GuardError::Malformed(_))
                ));
                // Correct encoded byte count, but the alpha plane itself
                // emits no samples. Valid RGB rows must not hide this.
                let alpha_start = start + 3 * (sample_bytes + 1);
                bytes[alpha_start..alpha_start + sample_bytes + 1].fill(128);
                assert!(matches!(
                    validate_saved_composite(&bytes),
                    Err(GuardError::Malformed(_))
                ));
                if sample_bytes > 1 {
                    let mut short = no_op;
                    short[start] = (sample_bytes - 2) as u8;
                    short[start + 1..start + sample_bytes].fill(127);
                    assert!(matches!(
                        validate_saved_composite(&short),
                        Err(GuardError::Malformed(_))
                    ));
                }
            }
        }
    }
}
