//! Content-based RAW recognition and decoder-reported camera variant metadata.
use crate::{IoError, Result};
use rawler::{
    decoders::{RawDecodeParams, WellKnownIFD},
    rawsource::RawSource,
};
use std::{
    collections::{BTreeMap, HashSet},
    io::{Read, Seek, SeekFrom},
    path::Path,
};

fn malformed(message: impl Into<String>) -> IoError {
    IoError::MalformedRaw(message.into())
}

pub(crate) fn decoder_error(error: rawler::RawlerError) -> IoError {
    match error {
        rawler::RawlerError::Unsupported {
            what,
            make,
            model,
            mode,
        } => IoError::UnsupportedRaw(format!("{make} {model} ({mode}): {what}")),
        rawler::RawlerError::DecoderFailed(message) => {
            // rawler 0.8 reports these known unsupported codecs as decode
            // failures. Do not present them as evidence of a corrupt file.
            let mode = match message.as_str() {
                "NEF compression Some(HighEfficencyStar) is not supported" => Some("HE★"),
                "NEF compression Some(HighEfficency) is not supported" => Some("HE"),
                _ => None,
            };
            if let Some(mode) = mode {
                IoError::UnsupportedRaw(format!(
                    "Nikon High Efficiency ({mode}) compression is not supported by Emulsion's RAW decoder. Convert this photo to a supported DNG or a 16-bit TIFF using software that supports Nikon HE/HE★. For future photos, select Lossless compression in the camera."
                ))
            } else {
                malformed(message)
            }
        }
    }
}

pub(crate) fn guarded<T>(f: impl FnOnce() -> Result<T>) -> Result<T> {
    std::panic::catch_unwind(std::panic::AssertUnwindSafe(f))
        .unwrap_or_else(|_| Err(malformed("decoder rejected malformed RAW data")))
}

// Read a bounded number of TIFF entries and a two-byte RAW codec marker.
// Camera Make alone is deliberately insufficient: exported TIFFs retain EXIF.
#[derive(Default)]
struct TiffProbe {
    raw: bool,
    previews: Vec<(u32, u32)>,
    orientation: Option<u8>,
    compression: Option<u32>,
    camera_tags: bool,
    nikon_he: bool,
    bits_per_sample: Option<u32>,
}
fn tiff_probe(file: &mut std::fs::File, header: &[u8]) -> Result<TiffProbe> {
    let mut result = TiffProbe::default();
    let le = header.starts_with(b"II");
    let u16v = |v: &[u8]| {
        if le {
            u16::from_le_bytes([v[0], v[1]])
        } else {
            u16::from_be_bytes([v[0], v[1]])
        }
    };
    let u32v = |v: &[u8]| {
        if le {
            u32::from_le_bytes(v[..4].try_into().unwrap())
        } else {
            u32::from_be_bytes(v[..4].try_into().unwrap())
        }
    };
    let mut pending = vec![u32v(&header[4..8])];
    let mut visited = HashSet::new();
    while let Some(offset) = pending.pop() {
        if offset == 0 {
            continue;
        }
        if !visited.insert(offset) || visited.len() > 32 {
            return Err(malformed("cyclic or excessive TIFF directories"));
        }
        file.seek(SeekFrom::Start(offset as u64))?;
        let mut count = [0; 2];
        file.read_exact(&mut count)
            .map_err(|_| malformed("truncated TIFF directory"))?;
        let count = u16v(&count) as usize;
        if count > 4096 {
            return Err(malformed("excessive TIFF directory entries"));
        }
        let mut entries = vec![0; count * 12 + 4];
        file.read_exact(&mut entries)
            .map_err(|_| malformed("truncated TIFF entries"))?;
        let mut jpeg_offset = None;
        let mut jpeg_length = None;
        let mut raw_ifd = false;
        let mut compression = None;
        let mut strip_offset = None;
        let mut bits_per_sample = None;
        for entry in entries[..count * 12].as_chunks::<12>().0 {
            let tag = u16v(entry);
            if tag == 258 && u32v(&entry[4..]) == 1 {
                bits_per_sample = match u16v(&entry[2..]) {
                    3 => Some(u32::from(u16v(&entry[8..]))),
                    4 => Some(u32v(&entry[8..])),
                    _ => None,
                };
            }
            if tag == 271 || tag == 272 {
                result.camera_tags = true;
            }
            if tag == 50706 || tag == 33422 {
                result.raw = true;
            } // DNGVersion / CFA pattern
            if tag == 33422 {
                raw_ifd = true;
            }
            if tag == 262 && u16v(&entry[8..]) == 32803 {
                result.raw = true;
                raw_ifd = true;
            }
            if tag == 259 && u32v(&entry[4..]) == 1 {
                compression = Some(if u16v(&entry[2..]) == 3 {
                    u16v(&entry[8..]) as u32
                } else {
                    u32v(&entry[8..])
                });
                if compression == Some(34713) {
                    result.raw = true;
                    raw_ifd = true;
                }
            }
            if tag == 274 && result.orientation.is_none() {
                result.orientation = u8::try_from(u16v(&entry[8..])).ok();
            }
            if u16v(&entry[2..]) == 4 && u32v(&entry[4..]) == 1 {
                if tag == 273 {
                    strip_offset = Some(u32v(&entry[8..]));
                }
                if tag == 513 {
                    jpeg_offset = Some(u32v(&entry[8..]));
                }
                if tag == 514 {
                    jpeg_length = Some(u32v(&entry[8..]));
                }
            }
            if tag == 330 && u16v(&entry[2..]) == 4 {
                let n = u32v(&entry[4..8]) as usize;
                if n > 32 {
                    return Err(malformed("excessive TIFF subdirectories"));
                }
                if n == 1 {
                    pending.push(u32v(&entry[8..]));
                } else if n > 1 {
                    file.seek(SeekFrom::Start(u32v(&entry[8..]) as u64))?;
                    let mut offsets = vec![0; n * 4];
                    file.read_exact(&mut offsets)
                        .map_err(|_| malformed("truncated TIFF subdirectory offsets"))?;
                    pending.extend(offsets.as_chunks::<4>().0.iter().map(|offset| u32v(offset)));
                }
            }
        }
        if let (Some(offset), Some(length)) = (jpeg_offset, jpeg_length) {
            result.previews.push((offset, length));
        }
        if raw_ifd {
            result.compression = compression;
            if compression == Some(34713)
                && let Some(offset) = strip_offset
            {
                file.seek(SeekFrom::Start(u64::from(offset)))?;
                let mut marker = [0; 2];
                if file.read_exact(&mut marker).is_ok() && marker == [0xff, 0x10] {
                    result.nikon_he = true;
                    result.bits_per_sample = bits_per_sample;
                }
            }
        }
        pending.push(u32v(&entries[count * 12..]));
    }
    Ok(result)
}

/// Recognize RAW content, using the extension only for otherwise unknown files.
/// A JPEG named `.nef` and an ordinary TIFF are still ordinary images.
pub fn is_raw(path: &Path) -> Result<bool> {
    let mut file = std::fs::File::open(path)?;
    let mut header = [0; 64];
    let n = file.read(&mut header)?;
    let h = &header[..n];
    if h.starts_with(b"FUJIFILMCCD-RAW")
        || h.starts_with(b"\0MRM")
        || h.starts_with(b"FOVb")
        || (h.len() >= 14 && &h[6..14] == b"HEAPCCDR")
        || (h.len() >= 12 && &h[8..12] == b"CR\x02\0")
        || h.starts_with(b"IIRO")
        || h.starts_with(b"IIRS")
        || h.starts_with(b"MMOR")
        || h.starts_with(b"IIU\0")
    {
        return Ok(true);
    }
    if h.len() >= 12
        && &h[4..8] == b"ftyp"
        && h[8..]
            .as_chunks::<4>()
            .0
            .iter()
            .any(|brand| brand == b"crx ")
    {
        return Ok(true);
    }
    if h.len() >= 8 && (h.starts_with(b"II*\0") || h.starts_with(b"MM\0*")) {
        let probe = tiff_probe(&mut file, h)?;
        // Legacy proprietary TIFF RAWs can omit CFA tags. If a camera file
        // has an explicit RAW suffix, require actual sensor decoding rather
        // than accidentally importing its root JPEG/RGB preview as the image.
        let raw_hint = path
            .extension()
            .map(|v| v.to_string_lossy().to_ascii_lowercase())
            .is_some_and(|v| crate::raw::RAW_EXTENSIONS.contains(&v.as_str()));
        return Ok(probe.raw || (raw_hint && probe.camera_tags));
    }
    if image::guess_format(h).is_ok() {
        return Ok(false);
    }
    Ok(path
        .extension()
        .map(|v| v.to_string_lossy().to_ascii_lowercase())
        .is_some_and(|v| crate::raw::RAW_EXTENSIONS.contains(&v.as_str())))
}

/// Metadata reflects the RAW image IFD, never the embedded preview's bit depth.
pub fn metadata(path: &Path) -> Result<emulsion_core::raw::RawMetadata> {
    guarded(|| {
        let source = RawSource::new(path)?;
        let decoder = rawler::get_decoder(&source).map_err(decoder_error)?;
        let meta = decoder
            .raw_metadata(&source, &RawDecodeParams::default())
            .map_err(decoder_error)?;
        let mut result = emulsion_core::raw::RawMetadata {
            make: meta.make,
            model: meta.model,
            format: format!("{:?}", decoder.format_hint()),
            compression: "unknown".into(),
            sensor: "unknown".into(),
            decoder: "rawler 0.8.0 + Nikon HE experimental (0f044c2c30d7)".into(),
            ..Default::default()
        };
        if let Some(ifd) = decoder.ifd(WellKnownIFD::Raw).map_err(decoder_error)? {
            let values: BTreeMap<u16, &rawler::formats::tiff::Entry> =
                ifd.entries().iter().map(|(k, v)| (*k, v)).collect();
            let number = |tag| {
                values
                    .get(&tag)
                    .filter(|e| e.count() > 0)
                    .map(|e| e.force_u32(0))
            };
            result.width = number(256).unwrap_or(0);
            result.height = number(257).unwrap_or(0);
            result.bits_per_sample = number(258).unwrap_or(0);
            if let Some(code) = number(259) {
                result.compression = match code {
                    1 => "uncompressed".into(),
                    7 => "JPEG (TIFF 7)".into(),
                    8 => "deflate".into(),
                    lossy @ 34892 => format!("lossy JPEG ({lossy})"),
                    other => format!("TIFF compression {other}"),
                };
            }
            if let Some(entry) = values.get(&33421).filter(|e| e.count() >= 2) {
                let dims = (entry.force_u32(0), entry.force_u32(1));
                result.sensor = match dims {
                    (2, 2) => "Bayer".into(),
                    (6, 6) => "6×6 CFA".into(),
                    (w, h) => format!("CFA {w}×{h}"),
                };
            } else if number(262) == Some(34892) {
                result.sensor = "linear RGB".into();
            }
        }
        if result.compression == "unknown" {
            let mut file = std::fs::File::open(path)?;
            let mut header = [0; 8];
            if file.read_exact(&mut header).is_ok()
                && (header.starts_with(b"II*\0") || header.starts_with(b"MM\0*"))
            {
                let probe = tiff_probe(&mut file, &header)?;
                if result.bits_per_sample == 0 {
                    result.bits_per_sample = probe.bits_per_sample.unwrap_or(0);
                }
                if probe.nikon_he {
                    result.compression = "Nikon High Efficiency (HE/HE★)".into();
                    result.warnings.push("Experimental Nikon HE/HE★ decoding: color and tone reconstruction are approximate and have not been validated against a reference decoder for this photo.".into());
                } else if let Some(code) = probe.compression {
                    result.compression = match code {
                        1 => "uncompressed".into(),
                        34713 => "Nikon compressed (TIFF 34713; submode unverified)".into(),
                        other => format!("TIFF compression {other}"),
                    };
                }
            }
        }
        if result.bits_per_sample == 0
            || result.compression == "unknown"
            || result.sensor == "unknown"
        {
            result.warnings.push("Some variant fields are unavailable from this decoder; compatibility is unverified.".into());
        }
        Ok(result)
    })
}

/// Embedded camera preview for browsing only. Development never uses this image.
pub fn embedded_preview(path: &Path) -> Result<Option<image::DynamicImage>> {
    use image::ImageDecoder;
    let mut file = std::fs::File::open(path)?;
    let mut header = [0; 96];
    let n = file.read(&mut header)?;
    let h = &header[..n];
    let mut probe = if n >= 8 && (h.starts_with(b"II*\0") || h.starts_with(b"MM\0*")) {
        tiff_probe(&mut file, h)?
    } else if n >= 92 && h.starts_with(b"FUJIFILMCCD-RAW") {
        TiffProbe {
            raw: true,
            previews: vec![(
                u32::from_be_bytes(h[84..88].try_into().unwrap()),
                u32::from_be_bytes(h[88..92].try_into().unwrap()),
            )],
            orientation: None,
            ..Default::default()
        }
    } else {
        return Ok(None);
    };
    probe
        .previews
        .sort_by_key(|(_, length)| std::cmp::Reverse(*length));
    for (offset, length) in probe.previews {
        if length == 0 || length > 32 << 20 {
            continue;
        }
        if u64::from(offset) + u64::from(length) > file.metadata()?.len() {
            continue;
        }
        file.seek(SeekFrom::Start(offset as u64))?;
        let mut bytes = vec![0; length as usize];
        file.read_exact(&mut bytes)?;
        let decoded = (|| -> Result<image::DynamicImage> {
            let mut reader = image::ImageReader::with_format(
                std::io::Cursor::new(bytes),
                image::ImageFormat::Jpeg,
            );
            let mut limits = image::Limits::default();
            limits.max_alloc = Some(128 << 20);
            limits.max_image_width = Some(16384);
            limits.max_image_height = Some(16384);
            reader.limits(limits);
            let mut decoder = reader.into_decoder()?;
            let (w, h) = decoder.dimensions();
            crate::import::check_size(w, h)?;
            let orientation = probe
                .orientation
                .and_then(image::metadata::Orientation::from_exif)
                .unwrap_or(
                    decoder
                        .orientation()
                        .unwrap_or(image::metadata::Orientation::NoTransforms),
                );
            let mut preview = image::DynamicImage::from_decoder(decoder)?;
            preview.apply_orientation(orientation);
            Ok(preview)
        })();
        if let Ok(preview) = decoded {
            return Ok(Some(preview));
        }
    }
    Ok(None)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn nikon_high_efficiency_is_unsupported_not_corrupt() {
        for (variant, label) in [("HighEfficency", "HE"), ("HighEfficencyStar", "HE★")] {
            let error = decoder_error(rawler::RawlerError::DecoderFailed(format!(
                "NEF compression Some({variant}) is not supported"
            )));
            assert!(matches!(&error, IoError::UnsupportedRaw(_)));
            let message = error.to_string();
            assert!(message.contains(&format!("({label})")));
            assert!(message.contains("16-bit TIFF"));
            assert!(!message.contains("corrupt"));
        }
        assert!(matches!(
            decoder_error(rawler::RawlerError::DecoderFailed("truncated data".into())),
            IoError::MalformedRaw(_)
        ));
    }

    struct Fixture(std::path::PathBuf);
    impl Fixture {
        fn new(extension: &str, bytes: &[u8]) -> Self {
            static NEXT: std::sync::atomic::AtomicU64 = std::sync::atomic::AtomicU64::new(0);
            let path = std::env::temp_dir().join(format!(
                "emulsion-raw-probe-{}-{}.{}",
                std::process::id(),
                NEXT.fetch_add(1, std::sync::atomic::Ordering::Relaxed),
                extension
            ));
            std::fs::write(&path, bytes).unwrap();
            Self(path)
        }
    }
    impl Drop for Fixture {
        fn drop(&mut self) {
            let _ = std::fs::remove_file(&self.0);
        }
    }
    fn tiff(tag: u16, value: u32) -> Vec<u8> {
        let mut data = b"II*\0\x08\0\0\0\x01\0".to_vec();
        data.extend(tag.to_le_bytes());
        data.extend(4u16.to_le_bytes());
        data.extend(1u32.to_le_bytes());
        data.extend(value.to_le_bytes());
        data.extend(0u32.to_le_bytes());
        data
    }
    #[test]
    fn renamed_dng_and_cfa_tiff_are_raw() {
        for tag in [50706, 33422] {
            let fixture = Fixture::new("bin", &tiff(tag, 1));
            assert!(is_raw(&fixture.0).unwrap());
        }
    }

    #[test]
    fn nikon_jpeg_xs_marker_distinguishes_high_efficiency_from_lossless() {
        for (marker, expected) in [([0xff, 0x10], true), ([0, 0], false)] {
            let mut bytes = b"II*\0\x08\0\0\0\x03\0".to_vec();
            for (tag, value) in [(258u16, 14u32), (259, 34713), (273, 50)] {
                bytes.extend(tag.to_le_bytes());
                bytes.extend(4u16.to_le_bytes());
                bytes.extend(1u32.to_le_bytes());
                bytes.extend(value.to_le_bytes());
            }
            bytes.extend(0u32.to_le_bytes());
            bytes.extend(marker);
            let fixture = Fixture::new("nef", &bytes);
            let mut file = std::fs::File::open(&fixture.0).unwrap();
            let probe = tiff_probe(&mut file, &bytes[..8]).unwrap();
            assert_eq!(probe.nikon_he, expected);
            assert_eq!(probe.compression, Some(34713));
            assert_eq!(probe.bits_per_sample, expected.then_some(14));
        }
    }
    #[test]
    fn ordinary_tiff_and_jpeg_are_not_raw_even_with_raw_extension() {
        let fixture = Fixture::new("nef", &tiff(256, 1));
        assert!(!is_raw(&fixture.0).unwrap());
        let jpeg = Fixture::new("nef", b"\xff\xd8\xff\xe0\0\x10JFIF\0");
        assert!(!is_raw(&jpeg.0).unwrap());
    }
    #[test]
    fn truncated_and_cyclic_tiff_fail_without_panicking() {
        let short = Fixture::new("dng", b"II*\0\x08\0\0\0");
        assert!(matches!(is_raw(&short.0), Err(IoError::MalformedRaw(_))));
        let mut bytes = tiff(256, 1);
        bytes[22..26].copy_from_slice(&8u32.to_le_bytes());
        let cycle = Fixture::new("tif", &bytes);
        assert!(matches!(is_raw(&cycle.0), Err(IoError::MalformedRaw(_))));
    }
    #[test]
    fn corrupt_raw_metadata_fails_without_panicking() {
        let fixture = Fixture::new("nef", b"bad");
        assert!(metadata(&fixture.0).is_err());
        assert!(embedded_preview(&fixture.0).unwrap().is_none());
    }

    #[test]
    fn embedded_jpeg_is_bounded_and_oriented() {
        let mut jpeg = Vec::new();
        image::codecs::jpeg::JpegEncoder::new(&mut jpeg)
            .encode(&[90; 2 * 3 * 3], 2, 3, image::ExtendedColorType::Rgb8)
            .unwrap();
        let mut bytes = b"II*\0\x08\0\0\0\x03\0".to_vec();
        for (tag, kind, value) in [
            (274u16, 3u16, 6u32),
            (513, 4, 50),
            (514, 4, jpeg.len() as u32),
        ] {
            bytes.extend(tag.to_le_bytes());
            bytes.extend(kind.to_le_bytes());
            bytes.extend(1u32.to_le_bytes());
            bytes.extend(value.to_le_bytes());
        }
        bytes.extend(0u32.to_le_bytes());
        bytes.extend(jpeg);
        let fixture = Fixture::new("dng", &bytes);
        let preview = embedded_preview(&fixture.0).unwrap().unwrap();
        assert_eq!((preview.width(), preview.height()), (3, 2));
        bytes[42..46].copy_from_slice(&(64u32 << 20).to_le_bytes());
        let excessive = Fixture::new("dng", &bytes);
        assert!(embedded_preview(&excessive.0).unwrap().is_none());
    }
}
