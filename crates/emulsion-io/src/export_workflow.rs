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
    Double,
    Quadruple,
}
impl ExportScale {
    pub fn dimensions(self, width: u32, height: u32) -> (u32, u32) {
        if matches!(self, Self::Double | Self::Quadruple) {
            let factor = if self == Self::Double { 2 } else { 4 };
            return (width.saturating_mul(factor), height.saturating_mul(factor));
        }
        let divisor = match self {
            Self::Full => 1,
            Self::Half => 2,
            Self::Quarter => 4,
            Self::Double | Self::Quadruple => unreachable!(),
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
    ProPhoto,
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
    crate::import::check_size(w, h)?;
    if u64::from(w) * u64::from(h) > 64_000_000 {
        return Err(failed(
            "Resized export exceeds 64 megapixels; choose a smaller scale.",
        ));
    }
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

fn diagram_raster(doc: &Document, scale: ExportScale) -> Result<Raster> {
    let (w, h) = scale.dimensions(doc.width, doc.height);
    crate::import::check_size(w, h)?;
    if u64::from(w) * u64::from(h) > 64_000_000 {
        return Err(failed(
            "Diagram export exceeds 64 megapixels; choose a smaller scale or vector PDF.",
        ));
    }
    let svg = crate::project_export::vector_svg(doc)?;
    let tree =
        resvg::usvg::Tree::from_data(&svg, &crate::svg_vectors::options()).map_err(failed)?;
    let mut pixmap = resvg::tiny_skia::Pixmap::new(w, h)
        .ok_or_else(|| failed("Could not allocate diagram export"))?;
    resvg::render(
        &tree,
        resvg::tiny_skia::Transform::from_scale(
            w as f32 / doc.width as f32,
            h as f32 / doc.height as f32,
        ),
        &mut pixmap.as_mut(),
    );
    let mut rgba = Vec::with_capacity(w as usize * h as usize * 4);
    for pixel in pixmap.pixels() {
        let c = pixel.demultiply();
        rgba.extend_from_slice(&[c.red(), c.green(), c.blue(), c.alpha()]);
    }
    Ok(Raster::from_srgba8(w, h, &rgba))
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
        ExportColorSpace::ProPhoto => ColorProfile::new_pro_photo_rgb(),
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
    Ok((
        pixels,
        crate::icc::encode_profile(&profile).map_err(failed)?,
    ))
}

fn converted_wide(
    flat: &Raster,
    space: ExportColorSpace,
    opaque: bool,
) -> Result<(Vec<u16>, Vec<u8>)> {
    use crate::photo_color::{Space, convert_float};
    let output = match space {
        ExportColorSpace::Srgb => Space::Srgb,
        ExportColorSpace::AdobeRgb => Space::AdobeRgb,
        ExportColorSpace::ProPhoto => Space::ProPhoto,
    };
    let pixels = flat.to_pixels();
    let mut rgb: Vec<_> = pixels
        .iter()
        .map(|p| {
            [0, 1, 2].map(|c| {
                if opaque {
                    p[c] as f32 / 65535. + 1. - p[3] as f32 / 65535.
                } else if p[3] > 0 {
                    p[c] as f32 / p[3] as f32
                } else {
                    0.
                }
            })
        })
        .collect();
    convert_float(&mut rgb, Space::ProPhoto, output)?;
    let mut values = Vec::with_capacity(pixels.len() * 4);
    for (p, color) in pixels.iter().zip(rgb) {
        for v in color {
            let v = v.clamp(0., 1.);
            let encoded = match output {
                Space::Srgb => emulsion_raster::color::linear_to_srgb(v),
                Space::AdobeRgb => v.powf(256. / 563.),
                Space::ProPhoto => v.powf(1. / 1.8),
            };
            values.push((encoded * 65535.).round() as u16);
        }
        values.push(if opaque { u16::MAX } else { p[3] });
    }
    Ok((
        values,
        crate::icc::encode_profile(&output.profile()).map_err(failed)?,
    ))
}

/// Develop linked originals before compositing. A single photographic source can
/// retain its ProPhoto working gamut through placement, opacity, masks and export.
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
    let preserve_wide = workflow.color_space != ExportColorSpace::Srgb
        && doc.raw.as_ref().is_some_and(|r| r.params.wide_gamut);
    let flat = if preserve_wide {
        let raw = doc.raw.as_ref().unwrap();
        if doc.nodes.len() != 1
            || doc.nodes[0].id != raw.node_id
            || !matches!(doc.nodes[0].kind, emulsion_core::NodeKind::Raster { .. })
        {
            return Err(failed(
                "Wide-gamut Photo export currently supports the linked photographic layer. Export layered artwork as sRGB, or export the developed original from Library.",
            ));
        }
        let source =
            crate::photo_develop::PhotoSource::load_verified(&raw.source, &raw.source_sha256)?;
        let mut developed = doc.clone();
        if let emulsion_core::NodeKind::Raster { raster, .. } = &mut developed.nodes[0].kind {
            *raster = std::sync::Arc::new(source.develop_working(&raw.params)?);
        }
        resized(flatten(&developed.composite_tree(), 0), workflow.scale)?
    } else {
        let developed = develop_document(doc)?;
        if developed.diagram.is_some()
            || (matches!(workflow.scale, ExportScale::Double | ExportScale::Quadruple)
                && developed.raw.is_none()
                && developed.nodes.iter().any(|n| {
                    matches!(
                        n.kind,
                        emulsion_core::NodeKind::Path { .. } | emulsion_core::NodeKind::Text { .. }
                    )
                }))
        {
            diagram_raster(&developed, workflow.scale)?
        } else {
            resized(flatten(&developed.composite_tree(), 0), workflow.scale)?
        }
    };
    let (w, h) = (flat.width(), flat.height());
    let (pixels, icc) = if preserve_wide {
        converted_wide(&flat, workflow.color_space, format == ExportFormat::Jpeg)?
    } else {
        converted(&flat, workflow.color_space, format == ExportFormat::Jpeg)?
    };
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
