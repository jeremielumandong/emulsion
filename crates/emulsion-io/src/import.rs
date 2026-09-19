//! Import common image formats as a one-node document.

use crate::{IoError, Result};
use emulsion_core::document::{MAX_PIXELS, MAX_SIDE};
use emulsion_core::{Command, Document, Node, command::Slot};
use emulsion_raster::{Placement, Raster};
use image::{ColorType, DynamicImage, ImageDecoder, ImageReader, metadata::Orientation};
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
    let mut decoder = reader.into_decoder()?;
    let (w, h) = decoder.dimensions();
    check_size(w, h)?;
    let orientation = decoder.orientation().unwrap_or(Orientation::NoTransforms);
    let mut img = DynamicImage::from_decoder(decoder)?;
    img.apply_orientation(orientation);
    from_dynamic(img)
}

/// Import `path` as a new document with one raster node.
pub fn import(path: &Path) -> Result<Document> {
    let decoded = decode(path)?;
    let name = path
        .file_stem()
        .map(|s| s.to_string_lossy().into_owned())
        .unwrap_or_else(|| "Image".into());
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

/// Import an encoded image held in memory as a new one-node document.
pub fn import_bytes(name: &str, bytes: &[u8]) -> Result<Document> {
    let img = image::load_from_memory(bytes)?;
    check_size(img.width(), img.height())?;
    let decoded = from_dynamic(img)?;
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
