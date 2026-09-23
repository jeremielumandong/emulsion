//! Import common image formats as a one-node document.

use crate::{IoError, Result};
use emulsion_core::document::{MAX_PIXELS, MAX_SIDE};
use emulsion_core::{Command, Document, Node, command::Slot};
use emulsion_raster::{Placement, Raster};
use image::{ColorType, DynamicImage, ImageDecoder, ImageReader, metadata::Orientation};
use std::io::{BufRead, Cursor, Seek};
use std::path::Path;
use std::sync::Arc;

/// Decoded pixels ready for a raster node.
pub struct Decoded {
    pub raster: Raster,
    pub depth: u8,
}

pub fn check_size(w: u32, h: u32) -> Result<()> {
    if w == 0 || h == 0 || w > MAX_SIDE || h > MAX_SIDE || w as u64 * h as u64 > MAX_PIXELS {
        return Err(IoError::TooLarge(w, h));
    }
    Ok(())
}

fn is_16bit(c: ColorType) -> bool {
    matches!(
        c,
        ColorType::L16
            | ColorType::La16
            | ColorType::Rgb16
            | ColorType::Rgba16
            | ColorType::Rgb32F
            | ColorType::Rgba32F
    )
}

/// Convert a decoded image to a raster, keeping 16-bit precision when the
/// source has it.
pub fn from_dynamic(img: DynamicImage) -> Result<Decoded> {
    let (w, h) = (img.width(), img.height());
    check_size(w, h)?;
    if is_16bit(img.color()) {
        let buf = img.into_rgba16();
        Ok(Decoded {
            raster: Raster::from_srgba16(w, h, buf.as_raw()),
            depth: 16,
        })
    } else {
        let buf = img.into_rgba8();
        Ok(Decoded {
            raster: Raster::from_srgba8(w, h, buf.as_raw()),
            depth: 8,
        })
    }
}

/// Decode a file, applying EXIF orientation.
pub fn decode(path: &Path) -> Result<Decoded> {
    let reader = ImageReader::open(path)?.with_guessed_format()?;
    if reader.format().is_none() {
        return Err(IoError::Unsupported(path.display().to_string()));
    }
    decode_reader(reader)
}

/// Keep CMYK TIFF ink samples until their ICC transform has been applied.
fn decode_reader<R: BufRead + Seek>(mut reader: ImageReader<R>) -> Result<Decoded> {
    if reader.format() == Some(image::ImageFormat::Tiff) {
        let mut stream = reader.into_inner();
        if let Some(decoded) = decode_cmyk_tiff(&mut stream)? {
            return Ok(decoded);
        }
        stream.rewind()?;
        reader = ImageReader::with_format(stream, image::ImageFormat::Tiff);
    }
    decode_with(reader.into_decoder()?)
}

fn tiff_error(error: tiff::TiffError) -> IoError {
    image::ImageError::Decoding(image::error::DecodingError::new(
        image::ImageFormat::Tiff.into(),
        error,
    ))
    .into()
}

fn decode_cmyk_tiff<R: std::io::Read + Seek>(stream: R) -> Result<Option<Decoded>> {
    use tiff::decoder::{Decoder, DecodingResult};
    use tiff::tags::Tag;
    let mut decoder = Decoder::new(stream).map_err(tiff_error)?;
    let color = decoder.colortype().map_err(tiff_error)?;
    if !matches!(color, tiff::ColorType::CMYK(_)) {
        return Ok(None);
    }
    let (w, h) = decoder.dimensions().map_err(tiff_error)?;
    check_size(w, h)?;
    if !matches!(color, tiff::ColorType::CMYK(8 | 16)) {
        return Err(IoError::Unsupported(
            "CMYK TIFF requires 8-bit or 16-bit samples".into(),
        ));
    }
    if decoder
        .get_tag_unsigned::<u16>(Tag::Unknown(332))
        .unwrap_or(1)
        != 1
    {
        return Err(IoError::Unsupported(
            "TIFF separated inks are not CMYK".into(),
        ));
    }
    let profile = decoder.get_tag_u8_vec(Tag::IccProfile).ok();
    let orientation = decoder
        .get_tag_unsigned::<u8>(Tag::Orientation)
        .ok()
        .and_then(Orientation::from_exif)
        .unwrap_or(Orientation::NoTransforms);
    let planar = decoder
        .get_tag_unsigned::<u16>(Tag::PlanarConfiguration)
        .unwrap_or(1)
        == 2;
    let mut samples = DecodingResult::U8(Vec::new());
    decoder
        .read_image_to_buffer(&mut samples)
        .map_err(tiff_error)?;
    let count = w as usize * h as usize;
    // Reject incomplete planes before indexing or constructing the image buffer.
    fn interleave<T: Copy>(samples: Vec<T>, count: usize, planar: bool) -> Result<Vec<T>> {
        if samples.len() != count * 4 {
            return Err(IoError::Unsupported("incomplete CMYK TIFF samples".into()));
        }
        if planar {
            Ok((0..count)
                .flat_map(|pixel| (0..4).map(move |ink| (pixel, ink)))
                .map(|(pixel, ink)| samples[ink * count + pixel])
                .collect())
        } else {
            Ok(samples)
        }
    }
    let mut image = match samples {
        DecodingResult::U8(samples) => {
            let samples = interleave(samples, count, planar)?;
            let rgba = crate::icc::cmyk_to_srgba8(profile.as_deref(), &samples);
            DynamicImage::ImageRgba8(image::RgbaImage::from_raw(w, h, rgba).unwrap())
        }
        DecodingResult::U16(samples) => {
            let samples = interleave(samples, count, planar)?;
            let rgba = crate::icc::cmyk_to_srgba16(profile.as_deref(), &samples);
            DynamicImage::ImageRgba16(image::ImageBuffer::from_raw(w, h, rgba).unwrap())
        }
        _ => {
            return Err(IoError::Unsupported(
                "unsupported CMYK TIFF sample format".into(),
            ));
        }
    };
    image.apply_orientation(orientation);
    from_dynamic(image).map(Some)
}

/// Decode through any `image` decoder (the built-in ones, or JPEG XL),
/// honouring EXIF orientation and an embedded ICC profile.
pub fn decode_with(mut decoder: impl ImageDecoder) -> Result<Decoded> {
    let (w, h) = decoder.dimensions();
    check_size(w, h)?;
    let orientation = decoder.orientation().unwrap_or(Orientation::NoTransforms);
    let icc = decoder.icc_profile().ok().flatten();
    let mut img = DynamicImage::from_decoder(decoder)?;
    img.apply_orientation(orientation);
    let mut decoded = from_dynamic(img)?;
    if let Some(icc) = icc {
        decoded.raster = crate::icc::to_srgb_raster(&icc, decoded.raster, decoded.depth);
    }
    Ok(decoded)
}

/// Import `path` as a new document with one raster node.
pub fn import(path: &Path) -> Result<Document> {
    document_from(path, decode(path)?)
}

/// A one-layer document from `decoded`, named after `path` and carrying
/// its EXIF facts.
pub fn document_from(path: &Path, decoded: Decoded) -> Result<Document> {
    let name = path
        .file_stem()
        .map(|s| s.to_string_lossy().into_owned())
        .unwrap_or_else(|| "Image".into());
    let mut doc = Document::new(decoded.raster.width(), decoded.raster.height());
    doc.source_depth = decoded.depth;
    doc.info = crate::exif::read(path);
    let node = Node::raster(0, name, Arc::new(decoded.raster), Placement::default());
    Command::AddNode {
        node: Box::new(node),
        slot: Slot::TOP,
    }
    .apply(&mut doc)
    .map_err(|e| IoError::Manifest(e.to_string()))?;
    Ok(doc)
}

/// Import an encoded image held in memory as a new one-node document.
pub fn import_bytes(name: &str, bytes: &[u8]) -> Result<Document> {
    let reader = ImageReader::new(Cursor::new(bytes)).with_guessed_format()?;
    let decoded = decode_reader(reader)?;
    let mut doc = Document::new(decoded.raster.width(), decoded.raster.height());
    doc.source_depth = decoded.depth;
    let node = Node::raster(0, name, Arc::new(decoded.raster), Placement::default());
    Command::AddNode {
        node: Box::new(node),
        slot: Slot::TOP,
    }
    .apply(&mut doc)
    .map_err(|e| IoError::Manifest(e.to_string()))?;
    Ok(doc)
}

#[cfg(test)]
mod cmyk_tests {
    use super::*;
    use tiff::encoder::{TiffEncoder, colortype};
    use tiff::tags::Tag;

    #[test]
    fn cmyk_tiff_16_keeps_precision_and_orientation_in_memory() {
        let mut bytes = Cursor::new(Vec::new());
        {
            let mut encoder = TiffEncoder::new(&mut bytes).unwrap();
            let mut image = encoder.new_image::<colortype::CMYK16>(2, 1).unwrap();
            image.encoder().write_tag(Tag::Orientation, 6u16).unwrap();
            image.write_data(&[1, 0, 0, 0, 0, 0, 0, 65535]).unwrap();
        }
        bytes.set_position(0);
        let decoded =
            decode_reader(ImageReader::with_format(bytes, image::ImageFormat::Tiff)).unwrap();
        assert_eq!(decoded.depth, 16);
        assert_eq!((decoded.raster.width(), decoded.raster.height()), (1, 2));
        assert_eq!(
            decoded.raster.to_srgba16(),
            [65534, 65535, 65535, 65535, 0, 0, 0, 65535]
        );
    }

    #[test]
    fn separated_non_cmyk_inks_are_rejected() {
        let mut bytes = Cursor::new(Vec::new());
        {
            let mut encoder = TiffEncoder::new(&mut bytes).unwrap();
            let mut image = encoder.new_image::<colortype::CMYK8>(1, 1).unwrap();
            image.encoder().write_tag(Tag::Unknown(332), 2u16).unwrap();
            image.write_data(&[0, 0, 0, 0]).unwrap();
        }
        assert!(import_bytes("inks", bytes.get_ref()).is_err());
    }
}
