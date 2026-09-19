//! Flattened export.

use crate::{IoError, Result, write_atomic};
use emulsion_core::Document;
use emulsion_raster::composite::flatten;
use image::codecs::jpeg::JpegEncoder;
use image::codecs::png::{CompressionType, FilterType, PngEncoder};
use image::codecs::tiff::TiffEncoder;
use image::codecs::webp::WebPEncoder;
use image::{ExtendedColorType, ImageEncoder};
use std::io::{BufWriter, Write};
use std::path::Path;

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum ExportFormat {
    Png,
    Jpeg,
    Webp,
    Tiff,
    /// Layered Photoshop document.
    Psd,
}

impl ExportFormat {
    pub fn from_path(path: &Path) -> Option<Self> {
        let ext = path.extension()?.to_string_lossy().to_ascii_lowercase();
        Some(match ext.as_str() {
            "png" => Self::Png,
            "jpg" | "jpeg" => Self::Jpeg,
            "webp" => Self::Webp,
            "tif" | "tiff" => Self::Tiff,
            "psd" | "psb" => Self::Psd,
            _ => return None,
        })
    }

    pub fn supports_16bit(self) -> bool {
        matches!(self, Self::Png | Self::Tiff)
    }
}

#[derive(Clone, Copy, Debug)]
pub struct ExportOptions {
    /// 8 or 16. Formats without 16-bit support write 8.
    pub depth: u8,
    /// 1–100, JPEG only.
    pub jpeg_quality: u8,
}

impl ExportOptions {
    pub fn for_doc(doc: &Document) -> Self {
        Self {
            depth: doc.source_depth,
            jpeg_quality: 92,
        }
    }
}

/// Encode a straight-alpha sRGBA8 buffer as PNG bytes.
pub fn png8(w: u32, h: u32, rgba: &[u8]) -> Result<Vec<u8>> {
    let mut out = Vec::new();
    PngEncoder::new_with_quality(&mut out, CompressionType::Fast, FilterType::Adaptive)
        .write_image(rgba, w, h, ExtendedColorType::Rgba8)?;
    Ok(out)
}

/// Encode a straight-alpha sRGBA16 buffer as PNG bytes.
pub fn png16(w: u32, h: u32, rgba: &[u16]) -> Result<Vec<u8>> {
    let mut out = Vec::new();
    // Encoders take 16-bit samples as native-endian bytes.
    let bytes: Vec<u8> = rgba.iter().flat_map(|v| v.to_ne_bytes()).collect();
    PngEncoder::new_with_quality(&mut out, CompressionType::Fast, FilterType::Adaptive)
        .write_image(&bytes, w, h, ExtendedColorType::Rgba16)?;
    Ok(out)
}

/// Encode an 8-bit grey buffer as PNG bytes.
pub fn png_gray(w: u32, h: u32, px: &[u8]) -> Result<Vec<u8>> {
    let mut out = Vec::new();
    PngEncoder::new_with_quality(&mut out, CompressionType::Fast, FilterType::Adaptive)
        .write_image(px, w, h, ExtendedColorType::L8)?;
    Ok(out)
}

/// Write the flattened document to `path`.
pub fn export(doc: &Document, path: &Path, opts: ExportOptions) -> Result<()> {
    let format = ExportFormat::from_path(path)
        .ok_or_else(|| IoError::Unsupported(path.display().to_string()))?;
    if format == ExportFormat::Psd {
        return crate::psd::write(doc, path);
    }
    let flat = flatten(&doc.composite_tree(), 0);
    let (w, h) = (doc.width, doc.height);
    let wide = opts.depth == 16 && format.supports_16bit();
    write_atomic(path, |f| {
        let mut out = BufWriter::new(f);
        match format {
            ExportFormat::Psd => unreachable!("handled above"),
            ExportFormat::Png => {
                let bytes = if wide {
                    png16(w, h, &flat.to_srgba16())?
                } else {
                    png8(w, h, &flat.to_srgba8())?
                };
                out.write_all(&bytes)?;
            }
            ExportFormat::Jpeg => {
                // JPEG has no alpha: composite over white.
                let rgba = flat.to_srgba8();
                let rgb: Vec<u8> = rgba
                    .as_chunks::<4>()
                    .0
                    .iter()
                    .flat_map(|p| {
                        let a = p[3] as u32;
                        [0, 1, 2].map(|i| ((p[i] as u32 * a + 255 * (255 - a) + 127) / 255) as u8)
                    })
                    .collect();
                JpegEncoder::new_with_quality(&mut out, opts.jpeg_quality.clamp(1, 100))
                    .write_image(&rgb, w, h, ExtendedColorType::Rgb8)?;
            }
            ExportFormat::Webp => {
                WebPEncoder::new_lossless(&mut out).write_image(
                    &flat.to_srgba8(),
                    w,
                    h,
                    ExtendedColorType::Rgba8,
                )?;
            }
            ExportFormat::Tiff => {
                let mut buf = std::io::Cursor::new(Vec::new());
                if wide {
                    let px = flat.to_srgba16();
                    let bytes: Vec<u8> = px.iter().flat_map(|v| v.to_ne_bytes()).collect();
                    TiffEncoder::new(&mut buf).write_image(
                        &bytes,
                        w,
                        h,
                        ExtendedColorType::Rgba16,
                    )?;
                } else {
                    TiffEncoder::new(&mut buf).write_image(
                        &flat.to_srgba8(),
                        w,
                        h,
                        ExtendedColorType::Rgba8,
                    )?;
                }
                out.write_all(buf.get_ref())?;
            }
        }
        out.flush()?;
        Ok(())
    })
}
