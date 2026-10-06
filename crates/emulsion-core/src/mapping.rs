//! Authoritative Smart placement and component mapping geometry.
//!
//! These runtime types have no wire serialization. Checked
//! representation, finite sampling domains and resource admission are separate
//! contracts. See `docs/technical/smart-mapping.md` before integrating them.

use emulsion_raster::Placement;
use emulsion_raster::projective::{Projective2, ProjectiveBounds, ProjectiveError, ProjectiveRect};
use glam::{DAffine2, DVec2};

/// A component/world map. Affine coefficients are retained without a
/// projective round trip; projective coefficients are checked by their type.
///
/// Public affine construction is permitted for migration. Retention/extraction
/// only requires finite coefficients; native adapters must still apply their
/// existing compatibility validation. Nontrivial operations have stricter
/// checked admission. Equality is exact representation equality, not approximate
/// geometric equivalence.
#[derive(Clone, Copy, Debug, PartialEq)]
pub enum Mapping2 {
    Affine(DAffine2),
    Projective(Projective2),
}

impl Default for Mapping2 {
    fn default() -> Self {
        Self::IDENTITY
    }
}

impl Mapping2 {
    pub const IDENTITY: Self = Self::Affine(DAffine2::IDENTITY);

    /// Variant-sensitive extraction. Projective identity remains projective.
    pub fn affine(self) -> Option<DAffine2> {
        match self {
            Self::Affine(value) => Some(value),
            Self::Projective(_) => None,
        }
    }

    pub fn require_affine(self, operation: &'static str) -> Result<DAffine2, crate::GeometryError> {
        self.affine()
            .ok_or_else(|| crate::GeometryError::retained_projective(operation))
    }

    /// Retain finite legacy coefficients; this is not operation admission.
    pub fn from_affine(affine: DAffine2) -> Result<Self, ProjectiveError> {
        let mapping = Self::Affine(affine);
        mapping.validate_representation()?;
        Ok(mapping)
    }

    /// Six columns in the existing mask order: a, d, b, e, c, f.
    pub fn from_affine_columns(columns: [f64; 6]) -> Result<Self, ProjectiveError> {
        Self::from_affine(DAffine2::from_cols_array(&columns))
    }

    /// Retention only, deliberately weaker than operation/domain admission.
    /// Does not replace legacy native validation or certify invertibility.
    pub fn validate_representation(self) -> Result<(), ProjectiveError> {
        if let Self::Affine(affine) = self
            && !affine.is_finite()
        {
            return Err(ProjectiveError::NonFinite);
        }
        Ok(())
    }

    /// Representation admission for new checked operations, not for reading an
    /// old document. Uses existing scale-aware validation without a sampling
    /// envelope; success does not certify a point or uniform numerical domain.
    pub fn validate_for_operation(self) -> Result<(), ProjectiveError> {
        self.to_projective()?;
        Ok(())
    }

    pub fn is_identity(self) -> bool {
        match self {
            Self::Affine(affine) => affine == DAffine2::IDENTITY,
            Self::Projective(projective) => projective == Projective2::IDENTITY,
        }
    }

    /// Explicit promotion. The returned canonical matrix can round affine
    /// coefficients; it must not replace stored legacy affine coefficients.
    pub fn to_projective(self) -> Result<Projective2, ProjectiveError> {
        match self {
            Self::Affine(affine) => Projective2::from_affine(affine),
            Self::Projective(projective) => Ok(projective),
        }
    }

    /// No perspective approximation or implicit identity fallback. Extracting
    /// an affine does not change the variant of the stored mapping.
    pub fn try_affine(self) -> Result<DAffine2, ProjectiveError> {
        self.validate_representation()?;
        match self {
            Self::Affine(affine) => Ok(affine),
            Self::Projective(projective) => projective.to_affine(),
        }
    }

    /// `self * rhs`, applying rhs first. Two affine operands use the original
    /// DAffine2 multiplication, without normalizing their result. Mixed operands
    /// stay projective, except an identity delta retains the other operand.
    /// If both are identities, rhs wins (the baseline for left-composition).
    pub fn compose(self, rhs: Self) -> Result<Self, ProjectiveError> {
        self.validate_representation()?;
        rhs.validate_representation()?;
        if self.is_identity() {
            return Ok(rhs);
        }
        if rhs.is_identity() {
            return Ok(self);
        }
        match (self, rhs) {
            (Self::Affine(left), Self::Affine(right)) => {
                // Check the projective product for unsupported numerical range,
                // but retain the exact legacy affine product, not this result.
                self.to_projective()?.compose(rhs.to_projective()?)?;
                let result = Self::from_affine(left * right)?;
                result.validate_for_operation()?;
                Ok(result)
            }
            _ => Ok(Self::Projective(
                self.to_projective()?.compose(rhs.to_projective()?)?,
            )),
        }
    }

    /// Keep legacy inverse arithmetic. A representable projective inverse is
    /// not necessarily a representable DAffine2 inverse; such an affine inverse
    /// fails rather than silently promoting or returning nonfinite coefficients.
    pub fn inverse(self) -> Result<Self, ProjectiveError> {
        self.validate_representation()?;
        if self.is_identity() {
            return Ok(self);
        }
        match self {
            Self::Affine(affine) => {
                self.validate_for_operation()?;
                let determinant = affine.matrix2.determinant();
                if !determinant.is_finite() {
                    return Err(ProjectiveError::NonFinite);
                }
                if !determinant.is_normal() {
                    // Projective admission already proved nonsingularity;
                    // zero/subnormal determinants lose range or relative
                    // precision in the legacy inverse arithmetic.
                    return Err(ProjectiveError::PrecisionLoss);
                }
                let result = Self::from_affine(affine.inverse())?;
                result.validate_for_operation()?;
                Ok(result)
            }
            Self::Projective(projective) => Ok(Self::Projective(projective.inverse()?)),
        }
    }

    pub fn map_point(self, point: DVec2) -> Result<DVec2, ProjectiveError> {
        self.validate_for_operation()?;
        match self {
            Self::Affine(affine) => Ok(affine_point(affine, point)?.0),
            Self::Projective(projective) => projective.map_point(point),
        }
    }

    /// Conservative geometric bounds from checked corner evaluations. The
    /// projective branch also excludes a horizon throughout the rectangle.
    ///
    /// Success is NOT uniform point-evaluation or sampling admission: an
    /// interior `map_point` can still return PrecisionLoss (for example, affine
    /// numerator cancellation near a zero crossing). A consumer that requires
    /// every sample to succeed needs a separate numerical-domain certificate;
    /// this bounds result cannot serve as that certificate.
    ///
    /// Choose the full relevant domain, not its visible/opaque subset. Relative
    /// mask maps need not support their whole intrinsic rectangle; select the
    /// relevant composed world/sampling map before requesting these bounds.
    pub fn map_rect(self, rect: ProjectiveRect) -> Result<MappedRect, ProjectiveError> {
        self.validate_for_operation()?;
        match self {
            Self::Projective(projective) => {
                let mapped = projective.map_rect(rect)?;
                Ok(MappedRect {
                    corners: mapped.corners(),
                    bounds: mapped.bounds(),
                })
            }
            Self::Affine(affine) => {
                if affine == DAffine2::IDENTITY {
                    let mapped = Projective2::IDENTITY.map_rect(rect)?;
                    return Ok(MappedRect {
                        corners: rect.corners(),
                        bounds: mapped.bounds(),
                    });
                }
                let mut corners = [DVec2::ZERO; 4];
                let mut min = DVec2::splat(f64::INFINITY);
                let mut max = DVec2::splat(f64::NEG_INFINITY);
                for (corner, source) in corners.iter_mut().zip(rect.corners()) {
                    let (point, error) = affine_point(affine, source)?;
                    *corner = point;
                    let low = point - error;
                    let high = point + error;
                    min = min.min(DVec2::new(low.x.next_down(), low.y.next_down()));
                    max = max.max(DVec2::new(high.x.next_up(), high.y.next_up()));
                }
                // Reuse the checked bounds type and integer conversion. The
                // identity bridge does not re-evaluate or normalize the affine.
                let envelope = ProjectiveRect::new(min, max)?;
                Ok(MappedRect {
                    corners,
                    bounds: Projective2::IDENTITY.map_rect(envelope)?.bounds(),
                })
            }
        }
    }

    /// Corner/geometric bounds only, with the same numerical-domain limitation
    /// as `map_rect`. Finite or integer-representable bounds do not admit every
    /// interior point, a sampler or any allocation.
    pub fn bounds(self, rect: ProjectiveRect) -> Result<ProjectiveBounds, ProjectiveError> {
        Ok(self.map_rect(rect)?.bounds())
    }

    /// Cache pixels -> document: Hsource * Translate(offset). The zero-offset
    /// branch preserves this representation exactly, including signed zeros.
    /// This is not a replacement for legacy smart::cache_placement arithmetic.
    pub fn with_source_offset(self, offset: (i32, i32)) -> Result<Self, ProjectiveError> {
        self.validate_representation()?;
        if offset == (0, 0) {
            return Ok(self);
        }
        self.compose(Self::Affine(DAffine2::from_translation(DVec2::new(
            f64::from(offset.0),
            f64::from(offset.1),
        ))))
    }

    /// Intrinsic mask pixels -> cache pixels: Translate(-offset) * Csource.
    /// Negate after conversion to f64, so i32::MIN is supported without overflow.
    pub fn in_cache(self, offset: (i32, i32)) -> Result<Self, ProjectiveError> {
        self.validate_representation()?;
        if offset == (0, 0) {
            return Ok(self);
        }
        Self::Affine(DAffine2::from_translation(DVec2::new(
            -f64::from(offset.0),
            -f64::from(offset.1),
        )))
        .compose(self)
    }
}

/// Ordered TL, TR, BR, BL corners and conservative geometric bounds, not a
/// uniform numerical-domain certificate. Interior point evaluation may still
/// fail. Bounds do not authorize sampling/allocation and may be larger than
/// the exact corner AABB.
#[derive(Clone, Copy, Debug, PartialEq)]
pub struct MappedRect {
    corners: [DVec2; 4],
    bounds: ProjectiveBounds,
}

impl MappedRect {
    pub fn corners(self) -> [DVec2; 4] {
        self.corners
    }

    pub fn bounds(self) -> ProjectiveBounds {
        self.bounds
    }
}

/// The single future authority for Smart source -> document placement.
/// Legacy retains all seven fields; Projective is independent of source/cache
/// dimensions. No Deref, serde, default-affine accessor or lossy decomposition.
#[derive(Clone, Copy, Debug, PartialEq)]
pub enum SmartPlacement {
    Legacy(Placement),
    Projective(Projective2),
}

impl Default for SmartPlacement {
    fn default() -> Self {
        Self::Legacy(Placement::default())
    }
}

impl SmartPlacement {
    pub fn legacy(self) -> Option<Placement> {
        match self {
            Self::Legacy(value) => Some(value),
            Self::Projective(_) => None,
        }
    }

    pub fn require_legacy(
        self,
        operation: &'static str,
    ) -> Result<Placement, crate::GeometryError> {
        self.legacy()
            .ok_or_else(|| crate::GeometryError::retained_projective(operation))
    }

    /// Source dimensions must be positive. Legacy uses Placement::to_doc with
    /// these dimensions exactly; never pass expanded filter-cache dimensions.
    /// Document source limits and finite support are separate admission checks.
    pub fn source_to_document(self, source_size: (u32, u32)) -> Result<Mapping2, ProjectiveError> {
        source_rect(source_size)?;
        match self {
            Self::Legacy(placement) => {
                Mapping2::from_affine(placement.to_doc(source_size.0, source_size.1))
            }
            Self::Projective(projective) => Ok(Mapping2::Projective(projective)),
        }
    }

    pub fn try_affine(self, source_size: (u32, u32)) -> Result<DAffine2, ProjectiveError> {
        self.source_to_document(source_size)?.try_affine()
    }

    pub fn cache_to_document(
        self,
        source_size: (u32, u32),
        offset: (i32, i32),
    ) -> Result<Mapping2, ProjectiveError> {
        self.source_to_document(source_size)?
            .with_source_offset(offset)
    }

    /// Explicit projective operation: Hnew = delta * Hold. Identity retains the
    /// exact input variant/fields. A meaningful delta promotes Legacy even when
    /// the resulting map happens to be affine. This does not implement legacy
    /// affine editing: those callers must keep their existing Placement path.
    /// Source/cache domain and complete candidate validation remain mandatory.
    pub fn left_compose_projective(
        self,
        delta: Projective2,
        source_size: (u32, u32),
    ) -> Result<Self, ProjectiveError> {
        if delta == Projective2::IDENTITY {
            return Ok(self);
        }
        let baseline = self.source_to_document(source_size)?;
        Ok(Self::Projective(delta.compose(baseline.to_projective()?)?))
    }
}

/// Geometry only; 30,000-side/400-MP and render-work limits belong to candidate
/// admission. This helper allocates nothing and does not authorize a raster.
pub fn source_rect(size: (u32, u32)) -> Result<ProjectiveRect, ProjectiveError> {
    ProjectiveRect::new(
        DVec2::ZERO,
        DVec2::new(f64::from(size.0), f64::from(size.1)),
    )
}

/// Intrinsic component -> document: Hsource * Csource. The Smart cache offset
/// is deliberately absent. No intrinsic-domain check is implied by composition.
pub fn mask_to_document(
    source_to_document: Mapping2,
    mask_to_source: Mapping2,
) -> Result<Mapping2, ProjectiveError> {
    source_to_document.compose(mask_to_source)
}

/// Preserve an unlinked mask's world map when content alone moves. Linked
/// components instead retain C exactly; whole-document coordinate changes also
/// retain C regardless of link state. Enabled/dormant state is irrelevant.
///
/// Computes inverse(Hnew) * (Hold * Cold), with the existing affine grouping.
/// Exact unchanged source maps retain Cold. A relative C may contain a horizon;
/// validate its relevant composed world/sampling domain separately. This is a
/// geometry guarantee, not byte-identical two-stage interpolated mask coverage.
pub fn preserve_mask_world(
    old_source_to_document: Mapping2,
    new_source_to_document: Mapping2,
    mask_to_source: Mapping2,
) -> Result<Mapping2, ProjectiveError> {
    old_source_to_document.validate_representation()?;
    new_source_to_document.validate_representation()?;
    mask_to_source.validate_representation()?;
    if old_source_to_document == new_source_to_document {
        return Ok(mask_to_source);
    }
    let world = mask_to_document(old_source_to_document, mask_to_source)?;
    let result = new_source_to_document.inverse()?.compose(world)?;
    retain_component_variant(mask_to_source, result)
}

/// Derived composition may simplify an exact identity intermediate; publishing
/// a compensated authored component must still retain its projective marker.
pub(crate) fn retain_component_variant(
    before: Mapping2,
    after: Mapping2,
) -> Result<Mapping2, ProjectiveError> {
    if matches!(before, Mapping2::Projective(_)) && matches!(after, Mapping2::Affine(_)) {
        Ok(Mapping2::Projective(after.to_projective()?))
    } else {
        Ok(after)
    }
}

/// Preserve DAffine2's evaluation, with a conservative error bound for two
/// products/two sums (also valid when glam uses FMA). Severe cancellation or
/// range loss fails; no normalized projective answer replaces the affine one.
fn affine_point(affine: DAffine2, point: DVec2) -> Result<(DVec2, DVec2), ProjectiveError> {
    if !point.is_finite() {
        return Err(ProjectiveError::NonFinite);
    }
    if affine == DAffine2::IDENTITY {
        return Ok((point, DVec2::ZERO));
    }
    let result = affine.transform_point2(point);
    if !result.is_finite() {
        return Err(ProjectiveError::NonFinite);
    }
    let mut error = DVec2::ZERO;
    for axis in 0..2 {
        let coefficients = [affine.matrix2.x_axis[axis], affine.matrix2.y_axis[axis]];
        let mut magnitude = affine.translation[axis].abs();
        for (coefficient, coordinate) in coefficients.into_iter().zip([point.x, point.y]) {
            let product = coefficient * coordinate;
            if !product.is_finite() {
                return Err(ProjectiveError::NonFinite);
            }
            if coefficient != 0.0 && coordinate != 0.0 && product == 0.0 {
                return Err(ProjectiveError::PrecisionLoss);
            }
            magnitude += product.abs();
        }
        error[axis] = (16.0 * f64::EPSILON * magnitude + f64::from_bits(8)).next_up();
        let budget = 1e-9 + 64.0 * f64::EPSILON * result[axis].abs();
        if !error[axis].is_finite() || error[axis] > budget {
            return Err(ProjectiveError::PrecisionLoss);
        }
    }
    Ok((result, error))
}

#[cfg(test)]
#[path = "mapping_tests.rs"]
mod tests;
