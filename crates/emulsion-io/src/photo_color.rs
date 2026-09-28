//! Color-space boundaries for Library development and output.
use crate::{IoError, Result};
use emulsion_raster::Raster;
use moxcms::ColorProfile;
use serde::{Deserialize, Serialize};
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum Space {
    #[default]
    Srgb,
    AdobeRgb,
    ProPhoto,
}
impl Space {
    pub fn profile(self) -> ColorProfile {
        match self {
            Self::Srgb => ColorProfile::new_srgb(),
            Self::AdobeRgb => ColorProfile::new_adobe_rgb(),
            Self::ProPhoto => ColorProfile::new_pro_photo_rgb(),
        }
    }
}
fn bad(e: impl std::fmt::Display) -> IoError {
    IoError::Unsupported(format!("Photo color conversion: {e}"))
}
/// Float conversion precedes quantization, retaining negative and super-white values.
pub fn convert_float(pixels: &mut [[f32; 3]], from: Space, to: Space) -> Result<()> {
    if from == to {
        return Ok(());
    }
    let matrix = |space: Space| {
        let p = space.profile();
        let cols = [p.red_colorant, p.green_colorant, p.blue_colorant]
            .map(|v| glam::DVec3::new(v.x, v.y, v.z));
        glam::DMat3::from_cols(cols[0], cols[1], cols[2])
    };
    // Profile colorants share the D50 PCS. A direct matrix preserves values
    // outside [0,1], unlike bounded ICC TRC lookup stages.
    let transform = matrix(to).inverse() * matrix(from);
    for p in pixels {
        let out = transform * glam::DVec3::new(p[0] as f64, p[1] as f64, p[2] as f64);
        *p = [out.x as f32, out.y as f32, out.z as f32];
    }

    Ok(())
}
pub fn convert_raster(raster: Raster, from: Space, to: Space) -> Result<Raster> {
    if from == to {
        return Ok(raster);
    }
    let mut pixels = raster.to_pixels();
    let mut rgb: Vec<_> = pixels
        .iter()
        .map(|p| {
            [0, 1, 2].map(|c| {
                if p[3] > 0 {
                    p[c] as f32 / p[3] as f32
                } else {
                    0.
                }
            })
        })
        .collect();
    convert_float(&mut rgb, from, to)?;
    for (p, rgb) in pixels.iter_mut().zip(rgb) {
        for c in 0..3 {
            p[c] = (rgb[c].clamp(0., 1.) * p[3] as f32 + 0.5) as u16;
        }
    }
    Ok(Raster::from_pixels(
        raster.width(),
        raster.height(),
        [0; 4],
        &pixels,
    ))
}
/// Encode from the declared linear working space, embedding the matching output ICC.
pub fn export(
    raster: &Raster,
    working: Space,
    output: Space,
    path: &std::path::Path,
    depth: u8,
    quality: u8,
    exif: Option<&[u8]>,
) -> Result<()> {
    use image::{ExtendedColorType, ImageEncoder};
    let format =
        crate::export::ExportFormat::from_path(path).ok_or_else(|| bad("unknown output format"))?;
    use crate::export::ExportFormat as F;
    if !matches!(format, F::Png | F::Jpeg | F::Tiff | F::Webp) {
        return Err(bad("color output requires PNG, JPEG, TIFF or WebP"));
    }
    let profile = output.profile();
    let icc = profile.encode().map_err(bad)?;

    let pixels = raster.to_pixels();
    let mut values = Vec::with_capacity(pixels.len() * 4);
    for chunk in pixels.chunks(65536) {
        let input: Vec<_> = chunk
            .iter()
            .flat_map(|p| {
                [0, 1, 2].map(|c| {
                    let a = p[3] as f32 / 65535.;
                    if format == F::Jpeg {
                        p[c] as f32 / 65535. + 1. - a
                    } else if a > 0. {
                        p[c] as f32 / p[3] as f32
                    } else {
                        0.
                    }
                })
            })
            .collect();
        let mut rgb: Vec<[f32; 3]> = input.chunks_exact(3).map(|p| [p[0], p[1], p[2]]).collect();
        convert_float(&mut rgb, working, output)?;
        let encode = |v: f32| {
            let v = v.max(0.);
            match output {
                Space::Srgb => {
                    if v <= 0.0031308 {
                        v * 12.92
                    } else {
                        1.055 * v.powf(1. / 2.4) - 0.055
                    }
                }
                Space::AdobeRgb => v.powf(256. / 563.),
                Space::ProPhoto => v.powf(1. / 1.8),
            }
        };
        for (p, rgb) in chunk.iter().zip(rgb.iter()) {
            values.extend(
                rgb.iter()
                    .map(|v| (encode(*v).clamp(0., 1.) * 65535. + 0.5) as u16),
            );
            values.push(if format == F::Jpeg { 65535 } else { p[3] });
        }
    }
    let wide = depth == 16 && matches!(format, F::Png | F::Tiff);
    let (w, h) = (raster.width(), raster.height());
    let bytes8 = || {
        values
            .iter()
            .map(|v| ((*v as u32 + 128) / 257) as u8)
            .collect::<Vec<_>>()
    };
    crate::write_atomic(path, |file| {
        let tag = |encoder: &mut dyn ImageEncoder| -> Result<()> {
            encoder
                .set_icc_profile(icc.clone())
                .map_err(image::ImageError::Unsupported)?;
            if let Some(exif) = exif {
                encoder
                    .set_exif_metadata(exif.to_vec())
                    .map_err(image::ImageError::Unsupported)?;
            }
            Ok(())
        };
        match format {
            F::Png => {
                let mut e = image::codecs::png::PngEncoder::new(file);
                tag(&mut e)?;
                if wide {
                    let bytes: Vec<_> = values.iter().flat_map(|v| v.to_ne_bytes()).collect();
                    e.write_image(&bytes, w, h, ExtendedColorType::Rgba16)?;
                } else {
                    e.write_image(&bytes8(), w, h, ExtendedColorType::Rgba8)?;
                }
            }
            F::Tiff => {
                let mut e = image::codecs::tiff::TiffEncoder::new(file);
                tag(&mut e)?;
                if wide {
                    let bytes: Vec<_> = values.iter().flat_map(|v| v.to_ne_bytes()).collect();
                    e.write_image(&bytes, w, h, ExtendedColorType::Rgba16)?;
                } else {
                    e.write_image(&bytes8(), w, h, ExtendedColorType::Rgba8)?;
                }
            }
            F::Jpeg => {
                let rgba = bytes8();
                let rgb: Vec<_> = rgba
                    .chunks_exact(4)
                    .flat_map(|p| p[..3].iter().copied())
                    .collect();
                let mut e = image::codecs::jpeg::JpegEncoder::new_with_quality(file, quality);
                tag(&mut e)?;
                e.write_image(&rgb, w, h, ExtendedColorType::Rgb8)?;
            }
            F::Webp => {
                let mut e = image::codecs::webp::WebPEncoder::new_lossless(file);
                tag(&mut e)?;
                e.write_image(&bytes8(), w, h, ExtendedColorType::Rgba8)?;
            }
            _ => unreachable!(),
        }
        Ok(())
    })
}
#[cfg(test)]
mod tests {
    use super::*;
    #[test]
    fn wide_primary_survives_before_display_clipping() {
        let original = [[0.12, 0.8, 0.04]];
        let mut srgb = original;
        convert_float(&mut srgb, Space::ProPhoto, Space::Srgb).unwrap();
        assert!(srgb[0].iter().any(|v| *v < 0. || *v > 1.));
        convert_float(&mut srgb, Space::Srgb, Space::ProPhoto).unwrap();
        for c in 0..3 {
            assert!((srgb[0][c] - original[0][c]).abs() < 0.001);
        }
    }
    #[test]
    fn wide_export_tags_and_preserves_precision() {
        use image::ImageDecoder;
        let dir = tempfile::tempdir().unwrap();
        let r = Raster::solid(2, 2, [0.12, 0.8, 0.04, 1.]);
        let file = dir.path().join("wide.png");
        export(&r, Space::ProPhoto, Space::ProPhoto, &file, 16, 92, None).unwrap();
        let mut decoder = image::ImageReader::open(&file)
            .unwrap()
            .with_guessed_format()
            .unwrap()
            .into_decoder()
            .unwrap();
        assert_eq!(decoder.color_type(), image::ColorType::Rgba16);
        let icc = decoder.icc_profile().unwrap().unwrap();
        assert!(ColorProfile::new_from_slice(&icc).is_ok());
        let rgba = image::open(file).unwrap().into_rgba16();
        let green = rgba.get_pixel(0, 0)[1] as f32 / 65535.;
        assert!((green - 0.8f32.powf(1. / 1.8)).abs() < 0.002);
    }
}
