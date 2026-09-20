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
    /// Layered GIMP document (8-bit).
    Xcf,
    Bmp,
    Gif,
    Tga,
    /// PPM without alpha; PAM keeps it.
    Pnm,
    /// Windows icon, at most 256 px a side.
    Ico,
    /// Radiance HDR, float RGB without alpha.
    Hdr,
    /// OpenEXR, float RGBA.
    Exr,
    Qoi,
    Farbfeld,
    /// Written by a converter on this machine (AVIF, HEIC, JPEG XL, PDF…):
    /// see `external::encoders`.
    External(&'static str),
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
            "xcf" => Self::Xcf,
            "bmp" => Self::Bmp,
            "gif" => Self::Gif,
            "tga" => Self::Tga,
            "ppm" | "pnm" | "pam" => Self::Pnm,
            "ico" => Self::Ico,
            "hdr" => Self::Hdr,
            "exr" => Self::Exr,
            "qoi" => Self::Qoi,
            "ff" => Self::Farbfeld,
            e => Self::External(
                crate::external::EXPORT_EXTENSIONS
                    .iter()
                    .copied()
                    .find(|x| *x == e)?,
            ),
        })
    }

    pub fn supports_16bit(self) -> bool {
        matches!(self, Self::Png | Self::Tiff | Self::Exr | Self::Farbfeld)
    }

    /// Whether the format keeps transparency.
    pub fn keeps_alpha(self) -> bool {
        !matches!(self, Self::Jpeg | Self::Hdr | Self::External("pdf"))
    }

    /// Whether this machine can write the format right now.
    pub fn available(self) -> bool {
        match self {
            Self::External(ext) => crate::external::can_encode(ext),
            _ => true,
        }
    }

    /// Extensions that export, in-process first, then the converter-backed
    /// ones a tool on this machine handles.
    pub fn exportable_extensions() -> Vec<&'static str> {
        let mut v: Vec<&str> = vec![
            "png", "jpg", "webp", "tif", "psd", "xcf", "bmp", "gif", "tga", "ppm", "ico", "hdr",
            "exr", "qoi", "ff",
        ];
        v.extend(
            crate::external::EXPORT_EXTENSIONS
                .iter()
                .copied()
                .filter(|e| crate::external::can_encode(e)),
        );
        v
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
    if format == ExportFormat::Xcf {
        return crate::xcf::write(doc, path);
    }
    let flat = flatten(&doc.composite_tree(), 0);
    let (w, h) = (doc.width, doc.height);
    let wide = opts.depth == 16 && format.supports_16bit();
    if let ExportFormat::External(ext) = format {
        // A PNG at the source depth, handed to the converter.
        let png = if wide {
            png16(w, h, &flat.to_srgba16())?
        } else {
            png8(w, h, &flat.to_srgba8())?
        };
        return crate::external::encode(&png, ext, path, opts.jpeg_quality);
    }
    // Formats without alpha composite over white.
    let over_white = |rgba: &[u8]| -> Vec<u8> {
        rgba.as_chunks::<4>()
            .0
            .iter()
            .flat_map(|p| {
                let a = p[3] as u32;
                [0, 1, 2].map(|i| ((p[i] as u32 * a + 255 * (255 - a) + 127) / 255) as u8)
            })
            .collect()
    };
    write_atomic(path, |f| {
        let mut out = BufWriter::new(f);
        match format {
            ExportFormat::Psd | ExportFormat::Xcf | ExportFormat::External(_) => {
                unreachable!("handled above")
            }
            ExportFormat::Bmp | ExportFormat::Gif | ExportFormat::Tga | ExportFormat::Qoi => {
                let img = image::RgbaImage::from_raw(w, h, flat.to_srgba8())
                    .ok_or_else(|| IoError::Unsupported("export buffer".into()))?;
                let fmt = match format {
                    ExportFormat::Bmp => image::ImageFormat::Bmp,
                    ExportFormat::Gif => image::ImageFormat::Gif,
                    ExportFormat::Tga => image::ImageFormat::Tga,
                    _ => image::ImageFormat::Qoi,
                };
                let mut buf = std::io::Cursor::new(Vec::new());
                image::DynamicImage::ImageRgba8(img).write_to(&mut buf, fmt)?;
                out.write_all(buf.get_ref())?;
            }
            ExportFormat::Pnm => {
                // PAM keeps alpha, but few readers open it; PPM travels.
                let rgb = over_white(&flat.to_srgba8());
                let img = image::RgbImage::from_raw(w, h, rgb)
                    .ok_or_else(|| IoError::Unsupported("export buffer".into()))?;
                let mut buf = std::io::Cursor::new(Vec::new());
                image::DynamicImage::ImageRgb8(img).write_to(&mut buf, image::ImageFormat::Pnm)?;
                out.write_all(buf.get_ref())?;
            }
            ExportFormat::Ico => {
                let img = image::RgbaImage::from_raw(w, h, flat.to_srgba8())
                    .ok_or_else(|| IoError::Unsupported("export buffer".into()))?;
                let mut img = image::DynamicImage::ImageRgba8(img);
                if w > 256 || h > 256 {
                    img = img.resize(256, 256, image::imageops::FilterType::Lanczos3);
                }
                let mut buf = std::io::Cursor::new(Vec::new());
                img.write_to(&mut buf, image::ImageFormat::Ico)?;
                out.write_all(buf.get_ref())?;
            }
            ExportFormat::Farbfeld => {
                let img =
                    image::ImageBuffer::<image::Rgba<u16>, _>::from_raw(w, h, flat.to_srgba16())
                        .ok_or_else(|| IoError::Unsupported("export buffer".into()))?;
                let mut buf = std::io::Cursor::new(Vec::new());
                image::DynamicImage::ImageRgba16(img)
                    .write_to(&mut buf, image::ImageFormat::Farbfeld)?;
                out.write_all(buf.get_ref())?;
            }
            ExportFormat::Exr | ExportFormat::Hdr => {
                // Float formats take the linear values straight from the raster.
                let px = flat.to_srgba16();
                let lin = |v: u16| emulsion_raster::color::srgb_to_linear(v as f32 / 65535.0);
                let mut buf = std::io::Cursor::new(Vec::new());
                if format == ExportFormat::Exr {
                    let data: Vec<f32> = px
                        .as_chunks::<4>()
                        .0
                        .iter()
                        .flat_map(|p| [lin(p[0]), lin(p[1]), lin(p[2]), p[3] as f32 / 65535.0])
                        .collect();
                    let img = image::ImageBuffer::<image::Rgba<f32>, _>::from_raw(w, h, data)
                        .ok_or_else(|| IoError::Unsupported("export buffer".into()))?;
                    image::DynamicImage::ImageRgba32F(img)
                        .write_to(&mut buf, image::ImageFormat::OpenExr)?;
                } else {
                    let data: Vec<f32> = px
                        .as_chunks::<4>()
                        .0
                        .iter()
                        .flat_map(|p| {
                            let a = p[3] as f32 / 65535.0;
                            [0, 1, 2].map(|i| lin(p[i]) * a + (1.0 - a))
                        })
                        .collect();
                    let img = image::ImageBuffer::<image::Rgb<f32>, _>::from_raw(w, h, data)
                        .ok_or_else(|| IoError::Unsupported("export buffer".into()))?;
                    image::DynamicImage::ImageRgb32F(img)
                        .write_to(&mut buf, image::ImageFormat::Hdr)?;
                }
                out.write_all(buf.get_ref())?;
            }
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
                let rgb = over_white(&flat.to_srgba8());
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
