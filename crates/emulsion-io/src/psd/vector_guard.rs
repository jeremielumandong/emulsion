//! Preserve evidence that ag-psd's permissive path parser would discard.
use super::mask_guard::GuardError;

type Result<T> = std::result::Result<T, GuardError>;
fn malformed(message: &'static str) -> GuardError {
    GuardError::Malformed(message)
}
fn word(bytes: &[u8]) -> u16 {
    u16::from_be_bytes([bytes[0], bytes[1]])
}

pub(super) fn reason(bytes: &[u8]) -> Result<Option<&'static str>> {
    let header = bytes
        .get(..8)
        .ok_or(malformed("truncated vector-mask header"))?;
    if header[..4] != 3u32.to_be_bytes() {
        return Ok(Some("unknown vector-mask version"));
    }
    let flags = u32::from_be_bytes(header[4..8].try_into().expect("four bytes"));
    let mut reason = (flags & !7 != 0).then_some("unknown vector-mask flags");
    let (records, remainder) = bytes[8..].as_chunks::<26>();
    // Some independent writers align path data to four bytes. Only the exact
    // two zero alignment bytes are accepted, never a partial unknown record.
    if !remainder.is_empty() && remainder != [0, 0] {
        return Err(malformed("truncated vector-mask path record"));
    }
    let mut pending = 0u16;
    let mut open = false;
    let mut contours = 0usize;
    let mut knots = 0usize;
    let mut fill = false;
    let mut initial = false;
    for record in records {
        let selector = word(record);
        if pending != 0 && !matches!(selector, 1 | 2 | 4 | 5) {
            return Err(malformed("vector-mask knot count exceeds its contour"));
        }
        match selector {
            0 | 3 => {
                pending = word(&record[2..]);
                open = selector == 3;
                contours += 1;
                if contours > 1 {
                    reason = reason.or(Some("compound vector-mask paths"));
                }
                if pending < 2 {
                    reason = reason.or(Some("degenerate vector-mask contour"));
                }
                if word(&record[4..]) != 1 {
                    reason = reason.or(Some("unsupported vector-mask Boolean operation"));
                }
                if !matches!(word(&record[6..]), 1 | 2) {
                    reason = reason.or(Some("unknown vector-mask fill rule"));
                }
                if record[8..].iter().any(|b| *b != 0) {
                    reason = reason.or(Some("unsupported vector-mask subpath metadata"));
                }
            }
            1 | 2 | 4 | 5 => {
                if pending == 0 {
                    return Err(malformed("vector-mask knot has no declared contour"));
                }
                if (selector >= 4) != open {
                    return Err(malformed(
                        "vector-mask knot closure differs from its contour",
                    ));
                }
                pending -= 1;
                knots += 1;
                if knots > emulsion_raster::vector::MAX_ANCHORS {
                    reason = reason.or(Some("vector-mask path exceeds native complexity"));
                }
                for number in record[2..].as_chunks::<4>().0 {
                    let value = i32::from_be_bytes(*number);
                    if !(-(16 << 24)..(16 << 24)).contains(&value) {
                        reason = reason.or(Some("vector-mask coordinate exceeds Adobe path range"));
                    }
                }
            }
            6 => {
                if fill || contours != 0 || record[2..].iter().any(|b| *b != 0) {
                    reason = reason.or(Some("unsupported vector-mask path-fill record"));
                }
                fill = true;
            }
            8 => {
                if initial
                    || contours != 0
                    || word(&record[2..]) > 1
                    || record[4..].iter().any(|b| *b != 0)
                {
                    reason = reason.or(Some("unsupported vector-mask initial-fill record"));
                }
                initial = true;
            }
            _ => {
                reason = reason.or(Some("unsupported vector-mask path record"));
            }
        }
    }
    if pending != 0 {
        return Err(malformed("vector-mask knot count exceeds its payload"));
    }
    if !fill || !initial {
        reason = reason.or(Some("missing vector-mask fill metadata"));
    }
    Ok(reason)
}
