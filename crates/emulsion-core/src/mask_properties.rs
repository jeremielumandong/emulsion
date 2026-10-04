//! Non-destructive raster-mask coverage controls.
use emulsion_raster::Mask;
use serde::{Deserialize, Serialize};

/// Feather radius limit, in intrinsic stored-mask pixels before the mask affine.
/// Mask affine and layer placement transform the resulting coverage together. This is a native bound,
/// not a claim of another application's kernel or document-space units.
pub use emulsion_raster::select::MAX_MASK_FEATHER;

#[derive(Clone, Copy, Debug, PartialEq, Serialize, Deserialize)]
#[serde(default)]
pub struct MaskProperties {
    /// One retains coverage; zero reveals the entire infinite mask plane.
    pub density: f32,
    /// Three-box Gaussian approximation radius in intrinsic stored-mask pixels.
    pub feather: f32,
}
impl Default for MaskProperties {
    fn default() -> Self {
        Self {
            density: 1.0,
            feather: 0.0,
        }
    }
}
impl MaskProperties {
    pub fn valid(self) -> bool {
        self.density.is_finite()
            && (0.0..=1.0).contains(&self.density)
            && self.feather.is_finite()
            && (0.0..=MAX_MASK_FEATHER).contains(&self.feather)
    }
    pub fn is_default(&self) -> bool {
        *self == Self::default()
    }

    pub(crate) fn apply(self, mask: &Mask) -> Mask {
        if self.density == 0.0 {
            return Mask::empty(mask.width(), mask.height(), 255);
        }
        let feathered = emulsion_raster::select::feather_with_fill(mask, self.feather);
        if self.density == 1.0 {
            return feathered;
        }
        let coverage = |v: u8| (255.0 - self.density * (255.0 - f32::from(v))).round() as u8;
        let fill = coverage(feathered.fill());
        let region = feathered.tile_bounds().intersect(&feathered.bounds());
        let values = feathered
            .read_rect(region)
            .into_iter()
            .map(coverage)
            .collect::<Vec<_>>();
        Mask::empty(feathered.width(), feathered.height(), fill).write_rect(region, &values)
    }
}
