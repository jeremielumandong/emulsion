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

    /// Intrinsic processing support, shared with the retained coverage cache.
    /// Density zero takes the existing constant-white shortcut; detail-free
    /// planes have no finite feather support to expand. Descriptor validation
    /// and projective sampling admission must still happen before shortcuts.
    pub(crate) fn processing_halo(self, has_detail: bool) -> u32 {
        if self.density > 0.0 && self.feather >= 0.5 && has_detail {
            3 * (self.feather / 1.7).round().max(1.0) as u32
        } else {
            0
        }
    }

    /// Allocation-free upper bounds for the existing intrinsic processing
    /// implementation. No document resource cap is imposed here: projective
    /// admission applies its own limits before processing. The byte lengths
    /// describe individual buffers, not a peak-memory or availability promise.
    pub fn checked_processing_layout(
        self,
        raw_size: (u32, u32),
        has_detail: bool,
    ) -> Result<MaskProcessingLayout, MaskProcessingError> {
        if !self.valid() {
            return Err(MaskProcessingError::Properties);
        }
        let halo = self.processing_halo(has_detail);
        let padding = halo.checked_mul(2).ok_or(MaskProcessingError::Layout)?;
        let size = (
            raw_size
                .0
                .checked_add(padding)
                .ok_or(MaskProcessingError::Layout)?,
            raw_size
                .1
                .checked_add(padding)
                .ok_or(MaskProcessingError::Layout)?,
        );
        let halo_i32 = i32::try_from(halo).map_err(|_| MaskProcessingError::Layout)?;
        let dense_coverage_bytes = if has_detail && self.density > 0.0 && self.density != 1.0 {
            Some(processing_bytes::<u8>(
                u64::from(size.0),
                u64::from(size.1),
            )?)
        } else {
            None
        };
        let feather_work = if halo > 0 {
            // feather_sampled_region retains at most 64 columns, and another
            // support halo around the processed region for its three passes.
            let strip_width = u64::from(size.0.min(64));
            let extended_height = u64::from(size.1) + u64::from(padding);
            Some(MaskFeatherWork {
                strip_f32_bytes: processing_bytes::<f32>(strip_width, extended_height)?,
                row_f32_bytes: processing_bytes::<f32>(strip_width + u64::from(padding), 1)?,
                column_f32_bytes: processing_bytes::<f32>(extended_height, 1)?,
                output_strip_bytes: processing_bytes::<u8>(strip_width, u64::from(size.1))?,
            })
        } else {
            None
        };
        Ok(MaskProcessingLayout {
            size,
            origin: (-halo_i32, -halo_i32),
            halo,
            dense_coverage_bytes,
            feather_work,
        })
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

#[derive(Clone, Copy, Debug, PartialEq, Eq, Hash)]
pub struct MaskProcessingLayout {
    pub size: (u32, u32),
    pub origin: (i32, i32),
    pub halo: u32,
    pub dense_coverage_bytes: Option<usize>,
    pub feather_work: Option<MaskFeatherWork>,
}

#[derive(Clone, Copy, Debug, PartialEq, Eq, Hash)]
pub struct MaskFeatherWork {
    pub strip_f32_bytes: usize,
    pub row_f32_bytes: usize,
    pub column_f32_bytes: usize,
    pub output_strip_bytes: usize,
}

#[derive(Clone, Copy, Debug, thiserror::Error, PartialEq, Eq)]
pub enum MaskProcessingError {
    #[error("invalid intrinsic mask density or feather")]
    Properties,
    #[error("intrinsic mask processing dimensions or buffer layout overflow")]
    Layout,
}

fn processing_bytes<T>(width: u64, height: u64) -> Result<usize, MaskProcessingError> {
    let count = width
        .checked_mul(height)
        .and_then(|n| usize::try_from(n).ok())
        .ok_or(MaskProcessingError::Layout)?;
    std::alloc::Layout::array::<T>(count)
        .map(|layout| layout.size())
        .map_err(|_| MaskProcessingError::Layout)
}
