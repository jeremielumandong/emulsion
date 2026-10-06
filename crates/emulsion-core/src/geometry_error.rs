//! Explicit failures at generalized geometry and rendering boundaries.
use crate::smart_support::SmartSupportError;
use emulsion_raster::projective::ProjectiveError;
use emulsion_raster::projective_mask_sample::MaskSampleError;
use emulsion_raster::projective_sample::ProjectivePixelError;

#[derive(Clone, Copy, Debug, thiserror::Error, PartialEq, Eq)]
pub enum GeometryError {
    #[error(transparent)]
    Mapping(#[from] ProjectiveError),
    #[error(transparent)]
    Support(#[from] SmartSupportError),
    #[error(transparent)]
    MaskSampling(#[from] MaskSampleError),
    #[error(transparent)]
    Pixels(#[from] ProjectivePixelError),
    #[error("{operation} is unavailable: {reason}")]
    Unsupported {
        operation: &'static str,
        reason: &'static str,
    },
    #[error("the node is not a Smart layer")]
    NotSmart,
}

impl GeometryError {
    pub fn retained_projective(operation: &'static str) -> Self {
        Self::Unsupported {
            operation,
            reason: "the layer retains projective placement or component metadata (including dormant mask mappings)",
        }
    }
}
