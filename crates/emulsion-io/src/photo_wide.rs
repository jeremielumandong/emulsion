//! Preserve embedded RGB gamut before converting to the Library working space.
use crate::{IoError, Result};
use emulsion_raster::Raster;
use image::ImageDecoder;
use moxcms::{ColorProfile, DataColorSpace, Layout, TransformOptions};
pub fn decode(path: &std::path::Path) -> Result<Raster> {
    let reader = image::ImageReader::open(path)?.with_guessed_format()?;
    let mut decoder = reader.into_decoder()?;
    let (w, h) = decoder.dimensions();
    crate::import::check_size(w, h)?;
    let profile = decoder
        .icc_profile()?
        .map(|bytes| ColorProfile::new_from_slice(&bytes))
        .transpose()
        .map_err(|e| IoError::Unsupported(format!("RGB profile: {e}")))?
        .unwrap_or_else(ColorProfile::new_srgb);
    if profile.color_space != DataColorSpace::Rgb {
        return Err(IoError::Unsupported(
            "Wide-gamut import requires RGB input".into(),
        ));
    }
    let orientation = decoder
        .orientation()
        .unwrap_or(image::metadata::Orientation::NoTransforms);
    let mut image = image::DynamicImage::from_decoder(decoder)?;
    image.apply_orientation(orientation);
    let rgba = image.to_rgba32f();
    let (w, h) = rgba.dimensions();
    let input: Vec<f32> = rgba.pixels().flat_map(|p| [p[0], p[1], p[2]]).collect();
    let transform = profile
        .create_transform_f32(
            Layout::Rgb,
            &ColorProfile::new_pro_photo_rgb(),
            Layout::Rgb,
            TransformOptions::default(),
        )
        .map_err(|e| IoError::Unsupported(format!("Wide RGB transform: {e}")))?;
    let mut encoded = vec![0.; input.len()];
    transform
        .transform(&input, &mut encoded)
        .map_err(|e| IoError::Unsupported(e.to_string()))?;
    let pixels = encoded
        .as_chunks::<3>()
        .0
        .iter()
        .zip(rgba.pixels())
        .map(|(rgb, p)| {
            let a = p[3].clamp(0., 1.);
            [
                (rgb[0].max(0.).powf(1.8).min(1.) * a * 65535.).round() as u16,
                (rgb[1].max(0.).powf(1.8).min(1.) * a * 65535.).round() as u16,
                (rgb[2].max(0.).powf(1.8).min(1.) * a * 65535.).round() as u16,
                (a * 65535.).round() as u16,
            ]
        })
        .collect::<Vec<_>>();
    Ok(Raster::from_pixels(w, h, [0; 4], &pixels))
}

/// Explicit non-sRGB RGB profiles warrant preserving the original gamut in
/// Photo documents. An sRGB tag adds no gamut, and a wide round trip would
/// shift its primaries.
pub fn has_rgb_profile(path: &std::path::Path) -> Result<bool> {
    let mut decoder = image::ImageReader::open(path)?
        .with_guessed_format()?
        .into_decoder()?;
    if matches!(
        decoder.color_type(),
        image::ColorType::Rgb32F | image::ColorType::Rgba32F
    ) {
        return Ok(false);
    }
    Ok(decoder
        .icc_profile()?
        .filter(|b| !crate::icc::is_srgb(b))
        .and_then(|b| ColorProfile::new_from_slice(&b).ok())
        .is_some_and(|p| p.color_space == DataColorSpace::Rgb))
}
