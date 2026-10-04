//! Shared native/history metadata for a dedicated Smart Filter mask.
//! The resource is a PNG name in the manifest and a pooled plane index in history.
use crate::{IoError, Result};
use emulsion_core::{MaskProperties, SmartFilterMask};
use emulsion_raster::Mask;
use serde::{Deserialize, Serialize};
use std::sync::Arc;

#[derive(Serialize, Deserialize)]
pub(crate) struct FilterMaskData<P> {
    pub pixels: P,
    pub width: u32,
    pub height: u32,
    pub fill: u8,
    pub enabled: bool,
    pub linked: bool,
    pub transform: [f64; 6],
    pub properties: MaskProperties,
}

impl<P> FilterMaskData<P> {
    pub fn encode(mask: &SmartFilterMask, pixels: P) -> Self {
        Self {
            pixels,
            width: mask.pixels.width(),
            height: mask.pixels.height(),
            fill: mask.pixels.fill(),
            enabled: mask.enabled,
            linked: mask.linked,
            transform: mask.transform,
            properties: mask.properties,
        }
    }

    /// Run before allocating resource pixels, including disabled/dormant masks.
    pub fn validate(&self) -> Result<()> {
        crate::import::check_size(self.width, self.height)?;
        let affine = glam::DAffine2::from_cols_array(&self.transform);
        let determinant = affine.matrix2.determinant();
        if !self.properties.valid()
            || !self.transform.iter().all(|v| v.is_finite())
            || !determinant.is_finite()
            || determinant.abs() < 1e-12
            || !affine
                .inverse()
                .to_cols_array()
                .iter()
                .all(|v| v.is_finite())
            || ![-1e9, 1e9].into_iter().all(|x| {
                [-1e9, 1e9].into_iter().all(|y| {
                    affine.transform_point2(glam::dvec2(x, y)).is_finite()
                        && affine
                            .inverse()
                            .transform_point2(glam::dvec2(x, y))
                            .is_finite()
                })
            })
        {
            return Err(IoError::Manifest(
                "invalid Smart Filter mask properties or affine".into(),
            ));
        }
        Ok(())
    }

    pub fn decode(self, pixels: Arc<Mask>) -> Result<SmartFilterMask> {
        self.validate()?;
        if (pixels.width(), pixels.height(), pixels.fill()) != (self.width, self.height, self.fill)
        {
            return Err(IoError::Manifest(
                "Smart Filter mask plane does not match its descriptor".into(),
            ));
        }
        Ok(SmartFilterMask {
            pixels,
            enabled: self.enabled,
            linked: self.linked,
            transform: self.transform,
            properties: self.properties,
        })
    }
}

impl FilterMaskData<String> {
    pub fn validate_resource(&self) -> Result<()> {
        self.validate()?;
        let Some(name) = self.pixels.strip_prefix("emulsion/filter-mask-") else {
            return Err(IoError::Manifest(
                "invalid Smart Filter mask resource".into(),
            ));
        };
        if !name.ends_with(".png") || name.contains(['/', '\\']) || name == ".png" {
            return Err(IoError::Manifest(
                "invalid Smart Filter mask resource".into(),
            ));
        }
        Ok(())
    }

    fn png_decoder<'a>(
        &self,
        bytes: &'a [u8],
    ) -> Result<image::codecs::png::PngDecoder<std::io::Cursor<&'a [u8]>>> {
        use image::ImageDecoder as _;
        self.validate_resource()?;
        let decoder = image::codecs::png::PngDecoder::new(std::io::Cursor::new(bytes))?;
        if decoder.dimensions() != (self.width, self.height)
            || decoder.color_type() != image::ColorType::L8
        {
            return Err(IoError::Manifest(
                "Smart Filter mask PNG does not match its descriptor".into(),
            ));
        }
        Ok(decoder)
    }

    pub fn validate_png(&self, bytes: &[u8]) -> Result<()> {
        self.png_decoder(bytes).map(|_| ())
    }

    pub fn decode_png(&self, bytes: &[u8]) -> Result<Arc<Mask>> {
        use image::ImageDecoder as _;
        let decoder = self.png_decoder(bytes)?;
        // The PNG header and descriptor have both passed the native size gate.
        let mut values = vec![0; self.width as usize * self.height as usize];
        decoder.read_image(&mut values)?;
        Ok(Arc::new(Mask::from_pixels(
            self.width,
            self.height,
            self.fill,
            &values,
        )))
    }
}
