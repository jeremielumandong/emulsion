//! Deliberately selected embedded metadata: never copy thumbnails, maker notes,
//! serial numbers or original orientation/crop geometry into rendered exports.
use crate::{IoError, Result};
use exif::{Context, Field, In, Tag, Value};
use std::path::Path;
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq, serde::Serialize, serde::Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum Policy {
    #[default]
    None,
    Copyright,
    Camera,
    CameraAndLocation,
}
pub fn build(source: &Path, policy: Policy) -> Result<Option<Vec<u8>>> {
    if policy == Policy::None {
        return Ok(None);
    }
    let source = crate::photo_develop::original_path(source)?;
    let mut input = std::io::BufReader::new(std::fs::File::open(&source)?);
    let metadata = match exif::Reader::new().read_from_container(&mut input) {
        Ok(v) => v,
        Err(exif::Error::NotFound(_)) => return Ok(None),
        Err(e) => {
            return Err(IoError::Manifest(format!(
                "Cannot read export metadata: {e}"
            )));
        }
    };
    let mut fields: Vec<Field> = metadata
        .fields()
        .filter(|f| {
            if f.ifd_num != In::PRIMARY {
                return false;
            }
            if [Tag::Artist, Tag::Copyright].contains(&f.tag) {
                return true;
            }
            if policy == Policy::Copyright {
                return false;
            }
            [
                Tag::Make,
                Tag::Model,
                Tag::LensMake,
                Tag::LensModel,
                Tag::DateTimeOriginal,
                Tag::OffsetTimeOriginal,
                Tag::ExposureTime,
                Tag::FNumber,
                Tag::PhotographicSensitivity,
                Tag::FocalLength,
                Tag::FocalLengthIn35mmFilm,
                Tag::ExposureProgram,
                Tag::ExposureBiasValue,
                Tag::Flash,
                Tag::MeteringMode,
                Tag::WhiteBalance,
            ]
            .contains(&f.tag)
                || (policy == Policy::CameraAndLocation && f.tag.context() == Context::Gps)
        })
        .cloned()
        .collect();
    if fields.is_empty() {
        return Ok(None);
    }
    fields.push(Field {
        tag: Tag::Orientation,
        ifd_num: In::PRIMARY,
        value: Value::Short(vec![1]),
    });
    fields.push(Field {
        tag: Tag::ColorSpace,
        ifd_num: In::PRIMARY,
        value: Value::Short(vec![1]),
    });
    let mut writer = exif::experimental::Writer::new();
    for field in &fields {
        writer.push_field(field);
    }
    let mut bytes = std::io::Cursor::new(Vec::new());
    writer
        .write(&mut bytes, true)
        .map_err(|e| IoError::Manifest(e.to_string()))?;
    if bytes.get_ref().len() > 60 * 1024 {
        return Err(IoError::Manifest("Selected metadata exceeds 60 KiB".into()));
    }
    Ok(Some(bytes.into_inner()))
}
/// Merge a generated EXIF directory into our newly encoded little-endian TIFF.
/// Existing image offsets stay unchanged; only the header's directory link moves.
pub(crate) fn tiff(mut image: Vec<u8>, exif: &[u8]) -> Result<Vec<u8>> {
    let bad = || IoError::Manifest("Invalid TIFF metadata layout".into());
    if image.get(..4) != Some(b"II*\0") || exif.get(..4) != Some(b"II*\0") {
        return Err(bad());
    }
    fn u16at(b: &[u8], p: usize) -> Option<u16> {
        Some(u16::from_le_bytes(b.get(p..p + 2)?.try_into().ok()?))
    }
    fn u32at(b: &[u8], p: usize) -> Option<u32> {
        Some(u32::from_le_bytes(b.get(p..p + 4)?.try_into().ok()?))
    }
    fn relocate(b: &mut [u8], at: usize, base: u32, depth: usize) -> Option<Vec<[u8; 12]>> {
        if depth > 4 {
            return None;
        }
        let n = u16at(b, at)? as usize;
        let mut out = Vec::new();
        for i in 0..n {
            let pos = at + 2 + 12 * i;
            let tag = u16at(b, pos)?;
            let ty = u16at(b, pos + 2)?;
            let count = u32at(b, pos + 4)?;
            let size = match ty {
                1 | 2 | 6 | 7 => 1,
                3 | 8 => 2,
                4 | 9 | 11 => 4,
                5 | 10 | 12 => 8,
                _ => return None,
            };
            let pointer = matches!(tag, 34665 | 34853 | 40965);
            let value = u32at(b, pos + 8)?;
            if pointer {
                relocate(b, value as usize, base, depth + 1)?;
            }
            if pointer || count.checked_mul(size)? > 4 {
                let shifted = value.checked_add(base)?;
                b.get_mut(pos + 8..pos + 12)?
                    .copy_from_slice(&shifted.to_le_bytes());
            }
            out.push(b.get(pos..pos + 12)?.try_into().ok()?);
        }
        Some(out)
    }
    let at = u32at(&image, 4).ok_or_else(bad)? as usize;
    let n = u16at(&image, at).ok_or_else(bad)? as usize;
    let mut entries = std::collections::BTreeMap::new();
    for i in 0..n {
        let p = at + 2 + 12 * i;
        let entry: [u8; 12] = image.get(p..p + 12).ok_or_else(bad)?.try_into().unwrap();
        entries.insert(u16::from_le_bytes([entry[0], entry[1]]), entry);
    }
    let next = u32at(&image, at + 2 + 12 * n).ok_or_else(bad)?;
    if !image.len().is_multiple_of(2) {
        image.push(0);
    }
    let base = u32::try_from(image.len()).map_err(|_| bad())?;
    let mut metadata = exif.to_vec();
    let root = u32at(&metadata, 4).ok_or_else(bad)? as usize;
    for entry in relocate(&mut metadata, root, base, 0).ok_or_else(bad)? {
        entries.insert(u16::from_le_bytes([entry[0], entry[1]]), entry);
    }
    image.extend(metadata);
    let new_root = u32::try_from(image.len()).map_err(|_| bad())?;
    image[4..8].copy_from_slice(&new_root.to_le_bytes());
    image.extend((entries.len() as u16).to_le_bytes());
    for entry in entries.values() {
        image.extend(entry);
    }
    image.extend(next.to_le_bytes());
    Ok(image)
}
