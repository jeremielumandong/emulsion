//! Explicit output conversion; never changes the document's working space.
use super::{ExportFormat, ExportOptions, develop_document};
use crate::{IoError, Result, write_atomic};
use emulsion_core::Document;
use emulsion_raster::{Raster, composite::flatten};
use image::ImageEncoder;
use moxcms::{ColorProfile, Layout, TransformOptions};
use std::{borrow::Cow, io::Write, path::Path};

#[cfg(test)]
#[path = "export_workflow_tests.rs"]
mod tests;

#[derive(Clone, Copy, Debug, Default, PartialEq, Eq)]
pub enum ExportScale {
    #[default]
    Full,
    Half,
    Quarter,
}
impl ExportScale {
    pub fn dimensions(self, width: u32, height: u32) -> (u32, u32) {
        let divisor = match self {
            Self::Full => 1,
            Self::Half => 2,
            Self::Quarter => 4,
        };
        (
            width.div_ceil(divisor).max(1),
            height.div_ceil(divisor).max(1),
        )
    }
}

#[derive(Clone, Copy, Debug, Default, PartialEq, Eq)]
pub enum ExportColorSpace {
    #[default]
    Srgb,
    AdobeRgb,
}

#[derive(Clone, Copy, Debug, Default, PartialEq, Eq)]
pub struct ExportWorkflow {
    pub scale: ExportScale,
    pub color_space: ExportColorSpace,
    /// Pixels per inch metadata only; never resamples by itself. 1–1200.
    pub dpi: Option<u16>,
}

fn failed(error: impl std::fmt::Display) -> IoError {
    IoError::Unsupported(format!("Export workflow: {error}"))
}

fn resized(flat: Raster, scale: ExportScale) -> Result<Raster> {
    if scale == ExportScale::Full {
        return Ok(flat);
    }
    let (w, h) = scale.dimensions(flat.width(), flat.height());
    // Filter linear, premultiplied samples, so edges do not acquire gamma or
    // transparent-color halos. Quantization is deferred to the output encoder.
    let pixels: Vec<f32> = flat
        .to_pixels()
        .into_iter()
        .flatten()
        .map(|v| v as f32 / 65535.0)
        .collect();
    let input =
        image::ImageBuffer::<image::Rgba<f32>, _>::from_raw(flat.width(), flat.height(), pixels)
            .ok_or_else(|| failed("invalid resize buffer"))?;
    let output = image::imageops::resize(&input, w, h, image::imageops::FilterType::Lanczos3);
    let pixels: Vec<[u16; 4]> = output
        .pixels()
        .map(|p| {
            let alpha = p.0[3].clamp(0.0, 1.0);
            [
                p.0[0].clamp(0.0, alpha),
                p.0[1].clamp(0.0, alpha),
                p.0[2].clamp(0.0, alpha),
                alpha,
            ]
            .map(|v| (v * 65535.0 + 0.5) as u16)
        })
        .collect();
    Ok(Raster::from_pixels(w, h, [0; 4], &pixels))
}

fn converted(flat: &Raster, space: ExportColorSpace, opaque: bool) -> Result<(Vec<u16>, Vec<u8>)> {
    let mut pixels = flat.to_srgba16();
    if opaque {
        // Composite over white in linear light before converting output space.
        for p in pixels.as_chunks_mut::<4>().0 {
            let alpha = p[3] as f32 / 65535.0;
            for v in &mut p[..3] {
                let linear = emulsion_raster::color::srgb_to_linear(*v as f32 / 65535.0);
                *v = (emulsion_raster::color::linear_to_srgb(linear * alpha + 1.0 - alpha)
                    * 65535.0
                    + 0.5) as u16;
            }
            p[3] = u16::MAX;
        }
    }
    let profile = match space {
        ExportColorSpace::Srgb => ColorProfile::new_srgb(),
        ExportColorSpace::AdobeRgb => ColorProfile::new_adobe_rgb(),
    };
    if space != ExportColorSpace::Srgb {
        let transform = ColorProfile::new_srgb()
            .create_transform_16bit(
                Layout::Rgba,
                &profile,
                Layout::Rgba,
                TransformOptions::default(),
            )
            .map_err(failed)?;
        let mut converted = vec![0; pixels.len()];
        transform
            .transform(&pixels, &mut converted)
            .map_err(failed)?;
        for (before, after) in pixels
            .as_chunks::<4>()
            .0
            .iter()
            .zip(converted.as_chunks_mut::<4>().0.iter_mut())
        {
            after[3] = before[3];
        }
        pixels = converted;
    }
    Ok((pixels, profile.encode().map_err(failed)?))
}

/// Develop and composite at full resolution, then resize and convert a copy.
/// Adobe RGB output preserves existing sRGB colors; it cannot restore gamut
/// already clipped by the document's bounded linear-sRGB working raster.
pub fn export_with_workflow(
    doc: &Document,
    path: &Path,
    opts: ExportOptions,
    workflow: ExportWorkflow,
) -> Result<()> {
    let format = ExportFormat::from_path(path).ok_or_else(|| failed("unknown output format"))?;
    if !matches!(
        format,
        ExportFormat::Png | ExportFormat::Jpeg | ExportFormat::Tiff | ExportFormat::Webp
    ) {
        if workflow != ExportWorkflow::default() {
            return Err(failed(
                "size, output profile, and resolution options are supported for PNG, JPEG, TIFF, and WebP only",
            ));
        }
        return super::export(doc, path, opts);
    }
    if workflow.dpi.is_some_and(|dpi| !(1..=1200).contains(&dpi)) {
        return Err(failed("resolution must be between 1 and 1200 ppi"));
    }
    if format == ExportFormat::Webp && workflow.dpi.is_some() {
        return Err(failed(
            "WebP resolution metadata is not supported; choose PNG, JPEG, or TIFF",
        ));
    }
    crate::ora::ensure_not_raw_original(doc, path)?;
    let developed = develop_document(doc)?;
    let flat = resized(flatten(&developed.composite_tree(), 0), workflow.scale)?;
    let (w, h) = (flat.width(), flat.height());
    let (pixels, icc) = converted(&flat, workflow.color_space, format == ExportFormat::Jpeg)?;
    let wide = opts.depth == 16 && format.supports_16bit();
    let bytes8 = || {
        // Reuse the raster's display quantizer for exact sRGB preview/export
        // agreement; converting through encoded 16-bit can round differently.
        if workflow.color_space == ExportColorSpace::Srgb {
            let rgba = flat.to_srgba8();
            if format != ExportFormat::Jpeg || rgba.as_chunks::<4>().0.iter().all(|p| p[3] == 255) {
                return rgba;
            }
        }
        pixels
            .iter()
            .map(|v| ((*v as u32 + 128) / 257) as u8)
            .collect::<Vec<_>>()
    };
    write_atomic(path, |out| {
        match format {
            ExportFormat::Png => {
                let mut info = png::Info::with_size(w, h);
                info.color_type = png::ColorType::Rgba;
                info.bit_depth = if wide {
                    png::BitDepth::Sixteen
                } else {
                    png::BitDepth::Eight
                };
                info.icc_profile = Some(Cow::Owned(icc));
                if let Some(dpi) = workflow.dpi {
                    let ppm = (dpi as f64 / 0.0254).round() as u32;
                    info.pixel_dims = Some(png::PixelDimensions {
                        xppu: ppm,
                        yppu: ppm,
                        unit: png::Unit::Meter,
                    });
                }
                let mut encoder = png::Encoder::with_info(out, info)
                    .map_err(failed)?
                    .write_header()
                    .map_err(failed)?;
                let bytes = if wide {
                    pixels.iter().flat_map(|v| v.to_be_bytes()).collect()
                } else {
                    bytes8()
                };
                encoder.write_image_data(&bytes).map_err(failed)?;
                encoder.finish().map_err(failed)?;
            }
            ExportFormat::Jpeg => {
                let mut encoder = image::codecs::jpeg::JpegEncoder::new_with_quality(
                    out,
                    opts.jpeg_quality.clamp(1, 100),
                );
                encoder.set_icc_profile(icc).map_err(failed)?;
                if let Some(dpi) = workflow.dpi {
                    encoder.set_pixel_density(image::codecs::jpeg::PixelDensity::dpi(dpi));
                }
                let rgb: Vec<_> = bytes8()
                    .as_chunks::<4>()
                    .0
                    .iter()
                    .flat_map(|p| p[..3].iter().copied())
                    .collect();
                encoder.write_image(&rgb, w, h, image::ExtendedColorType::Rgb8)?;
            }
            ExportFormat::Webp => {
                let mut encoder = image::codecs::webp::WebPEncoder::new_lossless(out);
                encoder.set_icc_profile(icc).map_err(failed)?;
                encoder.write_image(&bytes8(), w, h, image::ExtendedColorType::Rgba8)?;
            }
            ExportFormat::Tiff => {
                let mut buffer = std::io::Cursor::new(Vec::new());
                let mut encoder = tiff::encoder::TiffEncoder::new(&mut buffer).map_err(failed)?;
                macro_rules! write_tiff {
                    ($color:ty,$data:expr) => {{
                        let mut image = encoder.new_image::<$color>(w, h).map_err(failed)?;
                        image
                            .encoder()
                            .write_tag(tiff::tags::Tag::IccProfile, icc.as_slice())
                            .map_err(failed)?;
                        // Our RGBA buffers are straight (unassociated) alpha.
                        image
                            .encoder()
                            .write_tag(tiff::tags::Tag::ExtraSamples, &[2u16][..])
                            .map_err(failed)?;
                        if let Some(dpi) = workflow.dpi {
                            image
                                .encoder()
                                .write_tag(tiff::tags::Tag::ResolutionUnit, 2u16)
                                .map_err(failed)?;
                            for tag in [tiff::tags::Tag::XResolution, tiff::tags::Tag::YResolution]
                            {
                                image
                                    .encoder()
                                    .write_tag(
                                        tag,
                                        tiff::encoder::Rational {
                                            n: dpi as u32,
                                            d: 1,
                                        },
                                    )
                                    .map_err(failed)?;
                            }
                        }
                        image.write_data($data).map_err(failed)?;
                    }};
                }
                if wide {
                    write_tiff!(tiff::encoder::colortype::RGBA16, &pixels);
                } else {
                    write_tiff!(tiff::encoder::colortype::RGBA8, &bytes8());
                }
                out.write_all(buffer.get_ref())?;
            }
            _ => unreachable!(),
        }
        Ok(())
    })
}
