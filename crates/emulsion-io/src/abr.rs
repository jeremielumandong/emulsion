//! Bounded import of sampled Photoshop ABR tips, not Photoshop brush dynamics.
//! Binary field layout cross-checked against Krita's published ABR reader:
//! https://github.com/KDE/krita/blob/master/libs/brush/kis_abr_brush_collection.cpp
//! This reader uses checked slices, strict scanline lengths and allocation limits.
use crate::{IoError, Result, brushset::Imported};
use emulsion_raster::{
    library::BrushPreset,
    paint::{Brush, textures},
};
use std::{io::Read, path::Path};

fn error(message: impl Into<String>) -> IoError {
    IoError::Unsupported(message.into())
}
struct Bytes<'a>(&'a [u8]);
impl<'a> Bytes<'a> {
    fn take(&mut self, n: usize) -> Result<&'a [u8]> {
        if n > self.0.len() {
            return Err(error("Truncated ABR data"));
        }
        let (head, tail) = self.0.split_at(n);
        self.0 = tail;
        Ok(head)
    }
    fn u8(&mut self) -> Result<u8> {
        Ok(self.take(1)?[0])
    }
    fn u16(&mut self) -> Result<u16> {
        Ok(u16::from_be_bytes(self.take(2)?.try_into().unwrap()))
    }
    fn u32(&mut self) -> Result<u32> {
        Ok(u32::from_be_bytes(self.take(4)?.try_into().unwrap()))
    }
    fn i32(&mut self) -> Result<i32> {
        Ok(self.u32()? as i32)
    }
}

pub fn import(path: &Path) -> Result<Vec<Imported>> {
    const MAX_FILE: u64 = 128 << 20;
    let file = std::fs::File::open(path)?;
    if file.metadata()?.len() > MAX_FILE {
        return Err(error("ABR exceeds 128 MiB"));
    }
    let mut bytes = Vec::new();
    file.take(MAX_FILE + 1).read_to_end(&mut bytes)?;
    if bytes.len() as u64 > MAX_FILE {
        return Err(error("ABR exceeds 128 MiB"));
    }
    parse(
        &bytes,
        &path.file_stem().unwrap_or_default().to_string_lossy(),
    )
}

fn parse(bytes: &[u8], label: &str) -> Result<Vec<Imported>> {
    let mut input = Bytes(bytes);
    let version = input.u16()?;
    let mut records: Vec<(String, &[u8])> = Vec::new();
    let mut skipped = 0usize;
    match version {
        1 | 2 => {
            let count = input.u16()? as usize;
            if count > 10000 {
                return Err(error("Too many ABR samples"));
            }
            for index in 0..count {
                let kind = input.u16()?;
                let len = input.u32()? as usize;
                let mut record = Bytes(input.take(len)?);
                if kind != 2 {
                    skipped += 1;
                    continue;
                }
                record.take(6)?;
                let name = if version == 2 {
                    let chars = record.u32()? as usize;
                    if chars > 4096 {
                        return Err(error("ABR name exceeds limit"));
                    }
                    let units: Vec<u16> = record
                        .take(chars * 2)?
                        .as_chunks::<2>()
                        .0
                        .iter()
                        .map(|c| u16::from_be_bytes([c[0], c[1]]))
                        .collect();
                    String::from_utf16_lossy(&units)
                        .trim_end_matches('\0')
                        .to_owned()
                } else {
                    String::new()
                };
                record.take(9)?;
                records.push((
                    if name.trim().is_empty() {
                        format!("{label} {}", index + 1)
                    } else {
                        name
                    },
                    record.0,
                ));
            }
        }
        6 => {
            let subversion = input.u16()?;
            if !matches!(subversion, 1 | 2) {
                return Err(error(format!("ABR 6.{subversion} is unsupported")));
            }
            while !input.0.is_empty() {
                if input.take(4)? != b"8BIM" {
                    return Err(error("Invalid ABR resource signature"));
                }
                let kind = input.take(4)?;
                let len = input.u32()? as usize;
                let mut block = Bytes(input.take(len)?);
                if kind != b"samp" {
                    continue;
                }
                while !block.0.is_empty() {
                    let len = block.u32()? as usize;
                    let mut sample = Bytes(block.take(len)?);
                    block.take((4 - len % 4) % 4)?;
                    sample.take(if subversion == 1 { 47 } else { 301 })?;
                    records.push((format!("{label} {}", records.len() + 1), sample.0));
                    if records.len() > 10000 {
                        return Err(error("Too many ABR samples"));
                    }
                }
            }
        }
        _ => {
            return Err(error(format!(
                "ABR version {version} is unsupported; sampled tips from versions 1, 2 and 6 are supported"
            )));
        }
    }
    if records.is_empty() {
        return Err(error(format!(
            "No supported sampled tips in ABR; {skipped} procedural records skipped"
        )));
    }
    let mut total_pixels = 0usize;
    let mut result = Vec::new();
    for (name, bytes) in records {
        let mut sample = Bytes(bytes);
        let top = sample.i32()?;
        let left = sample.i32()?;
        let bottom = sample.i32()?;
        let right = sample.i32()?;
        let width = i64::from(right) - i64::from(left);
        let height = i64::from(bottom) - i64::from(top);
        if width <= 0 || height <= 0 || width > 4096 || height > 4096 {
            return Err(error("ABR tip bounds must be 1 through 4096 pixels"));
        }
        let (width, height) = (width as usize, height as usize);
        let area = width * height;
        total_pixels += area;
        if total_pixels > 64 * 1024 * 1024 {
            return Err(error("ABR exceeds 64 megapixels of decoded tips"));
        }
        let depth = sample.u16()?;
        if depth != 8 {
            return Err(error(format!(
                "ABR sample depth {depth} is unsupported; only 8-bit tips are imported"
            )));
        }
        let compression = sample.u8()?;
        let pixels = match compression {
            0 => sample.take(area)?.to_vec(),
            1 => {
                let row_lengths: Vec<usize> = (0..height)
                    .map(|_| sample.u16().map(usize::from))
                    .collect::<Result<_>>()?;
                let mut pixels = Vec::with_capacity(area);
                for row_len in row_lengths {
                    let mut row = Bytes(sample.take(row_len)?);
                    let start = pixels.len();
                    while !row.0.is_empty() {
                        match row.u8()? as i8 {
                            -128 => {}
                            n if n >= 0 => {
                                let count = n as usize + 1;
                                if pixels.len() - start + count > width {
                                    return Err(error("ABR RLE scanline exceeds width"));
                                }
                                pixels.extend_from_slice(row.take(count)?);
                            }
                            n => {
                                let count = (1 - i16::from(n)) as usize;
                                if pixels.len() - start + count > width {
                                    return Err(error("ABR RLE scanline exceeds width"));
                                }
                                let pixel = row.u8()?;
                                pixels.resize(pixels.len() + count, pixel);
                            }
                        }
                    }
                    if pixels.len() - start != width {
                        return Err(error("ABR RLE scanline is incomplete"));
                    }
                }
                pixels
            }
            _ => {
                return Err(error(format!(
                    "ABR compression {compression} is unsupported"
                )));
            }
        };
        let image = image::GrayImage::from_raw(width as u32, height as u32, pixels)
            .ok_or_else(|| error("Invalid ABR tip"))?;
        let mut png = std::io::Cursor::new(Vec::new());
        image.write_to(&mut png, image::ImageFormat::Png)?;
        let png = png.into_inner();
        let brush = Brush {
            tip: textures::id_for(&png),
            size: width.max(height) as f32,
            hardness: 1.,
            ..Brush::default()
        }
        .sanitized();
        let mut warnings = vec![format!(
            "ABR {version}: sampled tip only; Photoshop dynamics, descriptors and procedural settings are not converted"
        )];
        if skipped > 0 {
            warnings.push(format!(
                "Skipped {skipped} procedural/unknown brush records"
            ));
        }
        result.push(Imported {
            preset: BrushPreset {
                name,
                category: label.into(),
                note: "Imported Photoshop sampled tip".into(),
                brush,
            },
            shape_png: Some(png),
            grain_png: None,
            warnings,
            source_set: None,
        });
    }
    Ok(result)
}

#[cfg(test)]
mod tests {
    use super::*;
    fn sample(compressed: bool) -> Vec<u8> {
        let mut v = Vec::new();
        for n in [0i32, 0, 2, 2] {
            v.extend(n.to_be_bytes());
        }
        v.extend(8u16.to_be_bytes());
        v.push(u8::from(compressed));
        if compressed {
            v.extend([0, 3, 0, 3, 1, 0, 255, 1, 64, 128]);
        } else {
            v.extend([0, 255, 64, 128]);
        }
        v
    }
    fn legacy(version: u16, compressed: bool) -> Vec<u8> {
        let mut body = vec![0; 6];
        if version == 2 {
            body.extend(4u32.to_be_bytes());
            for c in "Tip\0".encode_utf16() {
                body.extend(c.to_be_bytes());
            }
        }
        body.extend([0; 9]);
        body.extend(sample(compressed));
        let mut out = version.to_be_bytes().to_vec();
        out.extend(1u16.to_be_bytes());
        out.extend(2u16.to_be_bytes());
        out.extend((body.len() as u32).to_be_bytes());
        out.extend(body);
        out
    }
    #[test]
    fn abr_legacy_raw_rle_and_names() {
        for version in [1, 2] {
            for compressed in [false, true] {
                let brushes = parse(&legacy(version, compressed), "Pack").unwrap();
                let pixels = image::load_from_memory(brushes[0].shape_png.as_ref().unwrap())
                    .unwrap()
                    .to_luma8();
                assert_eq!(pixels.as_raw(), &[0, 255, 64, 128]);
                assert_eq!(
                    brushes[0].preset.name,
                    if version == 2 { "Tip" } else { "Pack 1" }
                );
            }
        }
    }
    #[test]
    fn abr_v6_samples_and_bad_bounds() {
        for sub in [1u16, 2] {
            let mut body = vec![0; if sub == 1 { 47 } else { 301 }];
            body.extend(sample(false));
            let mut block = (body.len() as u32).to_be_bytes().to_vec();
            block.extend(&body);
            block.resize(4 + body.len().div_ceil(4) * 4, 0);
            let mut bytes = 6u16.to_be_bytes().to_vec();
            bytes.extend(sub.to_be_bytes());
            bytes.extend(b"8BIMsamp");
            bytes.extend((block.len() as u32).to_be_bytes());
            bytes.extend(block);
            assert_eq!(parse(&bytes, "V6").unwrap().len(), 1);
        }
        let mut bad = legacy(1, false);
        bad.truncate(bad.len() - 2);
        assert!(parse(&bad, "bad").is_err());
        let mut bad = legacy(1, false);
        bad[33..37].copy_from_slice(&i32::MAX.to_be_bytes());
        assert!(parse(&bad, "bad").is_err());
        assert!(parse(&[0, 9, 0, 1], "unsupported").is_err());
    }
}
