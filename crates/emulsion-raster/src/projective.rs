//! Checked, allocation-free projective geometry. This module does not change
//! legacy affine placement, rendering, persistence, or the existing warp API.
//!
//! Map validity and sampling-domain validity are deliberately separate. See
//! `docs/technical/projective-geometry.md` for numerical and integration limits.

use crate::geom::IRect;
use glam::{DAffine2, DVec2};
use thiserror::Error;

const PIVOT_MARGIN: f64 = 64.0 * f64::EPSILON;
const DENOMINATOR_MARGIN: f64 = 128.0 * f64::EPSILON;
const ROUND_OFF: f64 = 16.0 * f64::EPSILON;
const CARTESIAN_ABSOLUTE_ERROR: f64 = 1e-9;
const CARTESIAN_RELATIVE_ERROR: f64 = 64.0 * f64::EPSILON;
const INVERSE_RESIDUAL: f64 = 1e-10;
const DOMAIN_RATIO: f64 = 1e-12;
const REPROJECTION_RESIDUAL: f64 = 1e-9;
const MAX_REPROJECTION_TOLERANCE: f64 = 1e-4;

/// A checked map can still be unsupported on a particular sampling domain.
#[derive(Clone, Copy, Debug, Error, PartialEq, Eq)]
pub enum ProjectiveError {
    #[error("projective input or result is not finite")]
    NonFinite,
    #[error("projective matrix is singular")]
    Singular,
    #[error("projective calculation exceeds supported floating-point precision")]
    PrecisionLoss,
    #[error("rectangle must have finite, positive, representable extents")]
    InvalidRectangle,
    #[error("ordered quad must be strictly convex and nondegenerate")]
    InvalidQuad,
    #[error("projective horizon touches or crosses the domain")]
    Horizon,
    #[error("projective domain is too close to a horizon or numerically unstable")]
    UnsafeDomain,
    #[error("projective map is not exactly affine")]
    NotAffine,
    #[error("projected bounds cannot be represented safely as an integer rectangle")]
    BoundsOverflow,
}

/// Finite positive-area rectangle, ordered by increasing x and y.
#[derive(Clone, Copy, Debug, PartialEq)]
pub struct ProjectiveRect {
    min: DVec2,
    max: DVec2,
}

impl ProjectiveRect {
    pub fn new(min: DVec2, max: DVec2) -> Result<Self, ProjectiveError> {
        let size = max - min;
        if !min.is_finite()
            || !max.is_finite()
            || !size.is_finite()
            || size.x <= 0.0
            || size.y <= 0.0
        {
            return Err(ProjectiveError::InvalidRectangle);
        }
        Ok(Self { min, max })
    }

    pub fn min(self) -> DVec2 {
        self.min
    }

    pub fn max(self) -> DVec2 {
        self.max
    }

    pub fn size(self) -> DVec2 {
        self.max - self.min
    }

    /// TL, TR, BR, BL in source coordinates (y increases downwards).
    pub fn corners(self) -> [DVec2; 4] {
        [
            self.min,
            DVec2::new(self.max.x, self.min.y),
            self.max,
            DVec2::new(self.min.x, self.max.y),
        ]
    }
}

/// Conservative floating-point bounds; these do not approve any allocation.
#[derive(Clone, Copy, Debug, PartialEq)]
pub struct ProjectiveBounds {
    min: DVec2,
    max: DVec2,
}

impl ProjectiveBounds {
    pub fn min(self) -> DVec2 {
        self.min
    }

    pub fn max(self) -> DVec2 {
        self.max
    }

    /// Round out only after validating endpoints, differences and representable
    /// right/bottom coordinates. Apply canvas, pixel and work budgets separately.
    pub fn to_irect(self) -> Result<IRect, ProjectiveError> {
        fn endpoint(value: f64) -> Result<i32, ProjectiveError> {
            if !value.is_finite() || value < f64::from(i32::MIN) || value > f64::from(i32::MAX) {
                return Err(ProjectiveError::BoundsOverflow);
            }
            // The caller rounded first; both inclusive limits are exactly
            // representable in f64. This cast cannot saturate or truncate.
            Ok(value as i32)
        }
        let x0 = endpoint(self.min.x.floor())?;
        let y0 = endpoint(self.min.y.floor())?;
        let x1 = endpoint(self.max.x.ceil())?;
        let y1 = endpoint(self.max.y.ceil())?;
        let w = i32::try_from(i64::from(x1) - i64::from(x0))
            .map_err(|_| ProjectiveError::BoundsOverflow)?;
        let h = i32::try_from(i64::from(y1) - i64::from(y0))
            .map_err(|_| ProjectiveError::BoundsOverflow)?;
        if w <= 0 || h <= 0 {
            return Err(ProjectiveError::BoundsOverflow);
        }
        Ok(IRect::new(x0, y0, w, h))
    }
}

/// A rectangle that has passed the no-horizon and denominator-conditioning
/// checks. This is a snapshot, not a reusable approval for an expanded domain.
#[derive(Clone, Copy, Debug, PartialEq)]
pub struct MappedProjectiveRect {
    corners: [DVec2; 4],
    bounds: ProjectiveBounds,
    denominator_ratio: f64,
}

impl MappedProjectiveRect {
    pub fn corners(self) -> [DVec2; 4] {
        self.corners
    }

    pub fn bounds(self) -> ProjectiveBounds {
        self.bounds
    }

    /// Conservative minimum/maximum absolute denominator over the rectangle.
    pub fn denominator_ratio(self) -> f64 {
        self.denominator_ratio
    }
}

/// Row-major H acting on column vectors: `(u,v,w) = H * (x,y,1)`.
///
/// Private coefficients are finite, canonical and checked for invertibility.
/// Canonicalization uses the first largest-magnitude coefficient, not H[8].
/// PartialEq is exact coefficient equality, not approximate map equivalence.
/// There is intentionally no serialization or unchecked constructor.
#[derive(Clone, Copy, Debug, PartialEq)]
pub struct Projective2 {
    coefficients: [f64; 9],
}

impl Default for Projective2 {
    fn default() -> Self {
        Self::IDENTITY
    }
}

impl Projective2 {
    pub const IDENTITY: Self = Self {
        coefficients: [1.0, 0.0, 0.0, 0.0, 1.0, 0.0, 0.0, 0.0, 1.0],
    };

    pub fn from_row_major(coefficients: [f64; 9]) -> Result<Self, ProjectiveError> {
        let coefficients = canonicalize(coefficients)?;
        checked_inverse(coefficients)?;
        Ok(Self { coefficients })
    }

    pub fn to_row_major(self) -> [f64; 9] {
        self.coefficients
    }

    /// Explicit bridge for new projective paths. Legacy affine callers keep
    /// their original DAffine2 and original arithmetic unchanged.
    pub fn from_affine(affine: DAffine2) -> Result<Self, ProjectiveError> {
        let [a, d, b, e, c, f] = affine.to_cols_array();
        Self::from_row_major([a, b, c, d, e, f, 0.0, 0.0, 1.0])
    }

    /// No epsilon-based perspective removal: the bottom row must be (0,0,w).
    pub fn to_affine(self) -> Result<DAffine2, ProjectiveError> {
        let [a, b, c, d, e, f, g, h, w] = self.coefficients;
        if g != 0.0 || h != 0.0 || w == 0.0 {
            return Err(ProjectiveError::NotAffine);
        }
        let coefficients = [a, d, b, e, c, f].map(|v| v / w);
        if !coefficients.iter().all(|v| v.is_finite()) {
            return Err(ProjectiveError::NonFinite);
        }
        Ok(DAffine2::from_cols_array(&coefficients))
    }

    pub fn inverse(self) -> Result<Self, ProjectiveError> {
        Ok(Self {
            coefficients: checked_inverse(self.coefficients)?,
        })
    }

    /// `self * rhs`, applying rhs first. A meaningful identity does not perturb
    /// the other operand's coefficients. No operator offers unchecked math.
    pub fn compose(self, rhs: Self) -> Result<Self, ProjectiveError> {
        if self == Self::IDENTITY {
            return Ok(rhs);
        }
        if rhs == Self::IDENTITY {
            return Ok(self);
        }
        Self::from_row_major(multiply(self.coefficients, rhs.coefficients)?)
    }

    /// Evaluate the stored coefficient map with a per-coordinate error budget
    /// of `1e-9 + 64 * EPSILON * abs(result)`. Numerator cancellation can cause
    /// PrecisionLoss even with a well-conditioned nonzero denominator.
    pub fn map_point(self, point: DVec2) -> Result<DVec2, ProjectiveError> {
        if !point.is_finite() {
            return Err(ProjectiveError::NonFinite);
        }
        if self == Self::IDENTITY {
            return Ok(point);
        }
        let scale = input_scale(point.abs().max_element());
        let values = self.evaluate(point, scale)?;
        values[2].check_denominator()?;
        let result = DVec2::new(
            values[0].value / values[2].value,
            values[1].value / values[2].value,
        );
        if !result.is_finite() {
            return Err(ProjectiveError::NonFinite);
        }
        check_cartesian_accuracy(result.x, quotient_bounds(values[0], values[2])?)?;
        check_cartesian_accuracy(result.y, quotient_bounds(values[1], values[2])?)?;
        Ok(result)
    }

    /// Validate the entire rectangle, including corners outside the current
    /// canvas or visible content. A later filter-expanded rectangle needs its
    /// own check. No resource limits or allocations are hidden in this method.
    pub fn map_rect(self, rect: ProjectiveRect) -> Result<MappedProjectiveRect, ProjectiveError> {
        let source = rect.corners();
        if self == Self::IDENTITY {
            return Ok(MappedProjectiveRect {
                corners: source,
                bounds: ProjectiveBounds {
                    min: rect.min,
                    max: rect.max,
                },
                denominator_ratio: 1.0,
            });
        }
        let scale = input_scale(rect.min.abs().max(rect.max.abs()).max_element());
        let mut values = [[Dot::ZERO; 3]; 4];
        for (value, point) in values.iter_mut().zip(source) {
            *value = self.evaluate(point, scale)?;
            value[2].check_denominator()?;
        }
        let negative = values[0][2].value.is_sign_negative();
        if values
            .iter()
            .any(|v| v[2].value.is_sign_negative() != negative)
        {
            return Err(ProjectiveError::Horizon);
        }
        let minimum = values
            .iter()
            .map(|v| v[2].value.abs() - v[2].error)
            .fold(f64::INFINITY, f64::min);
        let maximum = values
            .iter()
            .map(|v| v[2].value.abs() + v[2].error)
            .fold(0.0, f64::max);
        let denominator_ratio = minimum / maximum;
        if denominator_ratio < DOMAIN_RATIO {
            return Err(ProjectiveError::UnsafeDomain);
        }
        let mut corners = [DVec2::ZERO; 4];
        let mut min = DVec2::splat(f64::INFINITY);
        let mut max = DVec2::splat(f64::NEG_INFINITY);
        for (corner, v) in corners.iter_mut().zip(values) {
            *corner = DVec2::new(v[0].value / v[2].value, v[1].value / v[2].value);
            let (x0, x1) = quotient_bounds(v[0], v[2])?;
            let (y0, y1) = quotient_bounds(v[1], v[2])?;
            check_cartesian_accuracy(corner.x, (x0, x1))?;
            check_cartesian_accuracy(corner.y, (y0, y1))?;
            min = min.min(DVec2::new(x0, y0));
            max = max.max(DVec2::new(x1, y1));
        }
        Ok(MappedProjectiveRect {
            corners,
            bounds: ProjectiveBounds { min, max },
            denominator_ratio,
        })
    }

    /// Map ordered source corners TL, TR, BR, BL to a strictly convex quad.
    /// Either destination winding is accepted. Reprojection is checked in both
    /// directions; valid but insufficiently precise configurations are refused.
    pub fn rect_to_quad(source: ProjectiveRect, quad: [DVec2; 4]) -> Result<Self, ProjectiveError> {
        if !quad.iter().all(|point| point.is_finite()) {
            return Err(ProjectiveError::NonFinite);
        }
        let min = quad.iter().copied().fold(quad[0], DVec2::min);
        let max = quad.iter().copied().fold(quad[0], DVec2::max);
        let destination =
            ProjectiveRect::new(min, max).map_err(|_| ProjectiveError::InvalidQuad)?;
        let size = destination.size();
        let q = quad.map(|point| (point - quad[0]) / size);
        validate_convex_quad(q)?;

        let d1 = q[1] - q[2];
        let d2 = q[3] - q[2];
        let d3 = q[0] - q[1] + q[2] - q[3];
        let denominator = d1.perp_dot(d2);
        let cancellation = (d1.x * d2.y).abs() + (d1.y * d2.x).abs();
        if denominator.abs() <= PIVOT_MARGIN * cancellation {
            return Err(ProjectiveError::InvalidQuad);
        }
        // Fixing H[8]=1 is safe only in these normalized unit-square
        // coordinates: its origin is a finite, prescribed destination vertex.
        let g = d3.perp_dot(d2) / denominator;
        let h = d1.perp_dot(d3) / denominator;
        let unit = Self::from_row_major([
            q[1].x - q[0].x + g * q[1].x,
            q[3].x - q[0].x + h * q[3].x,
            q[0].x,
            q[1].y - q[0].y + g * q[1].y,
            q[3].y - q[0].y + h * q[3].y,
            q[0].y,
            g,
            h,
            1.0,
        ])?;
        let source_size = source.size();
        let source_to_unit = Self::from_affine(DAffine2::from_cols_array(&[
            1.0 / source_size.x,
            0.0,
            0.0,
            1.0 / source_size.y,
            -source.min.x / source_size.x,
            -source.min.y / source_size.y,
        ]))?;
        let unit_to_destination = Self::from_affine(DAffine2::from_cols_array(&[
            size.x, 0.0, 0.0, size.y, quad[0].x, quad[0].y,
        ]))?;
        let result = unit_to_destination.compose(unit)?.compose(source_to_unit)?;
        let mapped = result.map_rect(source)?;
        let inverse = result.inverse()?;
        let forward_tolerance = reprojection_tolerance(destination)?;
        let backward_tolerance = reprojection_tolerance(source)?;
        for ((actual, wanted), original) in
            mapped.corners.into_iter().zip(quad).zip(source.corners())
        {
            check_reprojection(actual, wanted, size, forward_tolerance)?;
            check_reprojection(
                inverse.map_point(wanted)?,
                original,
                source_size,
                backward_tolerance,
            )?;
        }
        Ok(result)
    }

    fn evaluate(self, point: DVec2, scale: f64) -> Result<[Dot; 3], ProjectiveError> {
        let original = [point.x, point.y, 1.0];
        let point = original.map(|v| v / scale);
        if original.into_iter().zip(point).any(|(a, b)| b * scale != a) {
            // Division by a power of two is exact unless subnormal rounding
            // loses information. Verify reversibility rather than allowing
            // that error to be amplified by numerator cancellation.
            return Err(ProjectiveError::PrecisionLoss);
        }
        let mut result = [Dot::ZERO; 3];
        for (result, row) in result.iter_mut().zip(self.coefficients.as_chunks::<3>().0) {
            *result = Dot::new([row[0], row[1], row[2]], point)?;
        }
        Ok(result)
    }
}

fn input_scale(magnitude: f64) -> f64 {
    // Both callers provide finite nonnegative magnitudes. Retain only the
    // exponent of max(magnitude, 1), producing an exactly represented power
    // of two with normalized input coordinates smaller than two.
    f64::from_bits(magnitude.max(1.0).to_bits() & 0x7ff0_0000_0000_0000)
}

fn canonicalize(mut matrix: [f64; 9]) -> Result<[f64; 9], ProjectiveError> {
    if !matrix.iter().all(|v| v.is_finite()) {
        return Err(ProjectiveError::NonFinite);
    }
    let mut pivot: f64 = 0.0;
    for &value in &matrix {
        if value.abs() > pivot.abs() {
            pivot = value;
        }
    }
    if pivot == 0.0 {
        return Err(ProjectiveError::Singular);
    }
    for value in &mut matrix {
        let original = *value;
        *value /= pivot;
        if original != 0.0 && !value.is_normal() {
            return Err(ProjectiveError::PrecisionLoss);
        }
        if *value == 0.0 {
            *value = 0.0;
        }
    }
    Ok(matrix)
}

fn multiply(a: [f64; 9], b: [f64; 9]) -> Result<[f64; 9], ProjectiveError> {
    let mut result = [0.0; 9];
    for (index, value) in result.iter_mut().enumerate() {
        let (row, col) = (index / 3, index % 3);
        let mut correction = 0.0;
        for k in 0..3 {
            let (a, b) = (a[row * 3 + k], b[k * 3 + col]);
            let product = a * b;
            if a != 0.0 && b != 0.0 && !product.is_normal() {
                return Err(ProjectiveError::PrecisionLoss);
            }
            // Compensate both product and summation rounding. Translation
            // cancellation should not needlessly destroy low-order terms.
            let sum = *value + product;
            let added = sum - *value;
            correction += (*value - (sum - added)) + (product - added) + a.mul_add(b, -product);
            *value = sum;
        }
        *value += correction;
        if !value.is_finite() {
            return Err(ProjectiveError::NonFinite);
        }
    }
    Ok(result)
}

fn checked_inverse(matrix: [f64; 9]) -> Result<[f64; 9], ProjectiveError> {
    // A = R * E * C. Large translation and independent scale must not appear
    // as an arbitrary raw determinant threshold or a raw condition number.
    let mut row_scale = [0.0_f64; 3];
    let mut column_scale = [0.0_f64; 3];
    let mut balanced = [0.0; 9];
    for (r, row) in matrix.as_chunks::<3>().0.iter().enumerate() {
        row_scale[r] = row.iter().map(|v| v.abs()).fold(0.0, f64::max);
        if row_scale[r] == 0.0 {
            return Err(ProjectiveError::Singular);
        }
        for (c, &value) in row.iter().enumerate() {
            balanced[r * 3 + c] = value / row_scale[r];
            column_scale[c] = column_scale[c].max(balanced[r * 3 + c].abs());
        }
    }
    if column_scale.contains(&0.0) {
        return Err(ProjectiveError::Singular);
    }
    for (index, value) in balanced.iter_mut().enumerate() {
        *value /= column_scale[index % 3];
    }
    let mut augmented = [[0.0; 6]; 3];
    for (r, row) in augmented.iter_mut().enumerate() {
        row[..3].copy_from_slice(&balanced[r * 3..r * 3 + 3]);
        row[r + 3] = 1.0;
    }
    for col in 0..3 {
        let mut pivot = col;
        for (r, row) in augmented.iter().enumerate().skip(col + 1) {
            if row[col].abs() > augmented[pivot][col].abs() {
                pivot = r;
            }
        }
        let divisor = augmented[pivot][col];
        if divisor == 0.0 {
            return Err(ProjectiveError::Singular);
        }
        if divisor.abs() <= PIVOT_MARGIN {
            return Err(ProjectiveError::PrecisionLoss);
        }
        augmented.swap(col, pivot);
        for value in &mut augmented[col] {
            *value /= divisor;
        }
        let pivot_row = augmented[col];
        for (r, row) in augmented.iter_mut().enumerate() {
            if r != col {
                let factor = row[col];
                for (value, pivot_value) in row.iter_mut().zip(pivot_row) {
                    *value -= factor * pivot_value;
                }
                row[col] = 0.0;
            }
        }
    }
    let mut inverse = [0.0; 9];
    for (r, row) in augmented.iter().enumerate() {
        inverse[r * 3..r * 3 + 3].copy_from_slice(&row[3..]);
    }
    check_inverse_residual(balanced, inverse)?;

    // A^-1 = C^-1 * E^-1 * R^-1. Reassemble in exponent space so
    // an otherwise representable projective inverse need not overflow first.
    let mut factored = [Scaled::ZERO; 9];
    for (index, value) in factored.iter_mut().enumerate() {
        *value = Scaled::new(inverse[index])?
            .divide(column_scale[index / 3])?
            .divide(row_scale[index % 3])?;
    }
    let result = canonicalize_scaled(factored)?;

    // Check the actual rounded output inverse in the same balanced basis.
    for (index, value) in factored.iter_mut().enumerate() {
        *value = Scaled::new(result[index])?
            .multiply(column_scale[index / 3])?
            .multiply(row_scale[index % 3])?;
    }
    let recovered = canonicalize_scaled(factored)?;
    let mut pivot = 0;
    for (index, value) in inverse.iter().enumerate().skip(1) {
        if value.abs() > inverse[pivot].abs() {
            pivot = index;
        }
    }
    // Use a fixed reference entry: rounded equal-magnitude entries can change
    // which sign canonicalization selects, without changing the map.
    let scale = inverse[pivot] / recovered[pivot];
    check_inverse_residual(balanced, recovered.map(|v| v * scale))?;
    Ok(result)
}

fn check_inverse_residual(a: [f64; 9], inverse: [f64; 9]) -> Result<(), ProjectiveError> {
    for (left, right) in [(a, inverse), (inverse, a)] {
        let product = multiply(left, right)?;
        for (index, value) in product.into_iter().enumerate() {
            let expected = if index / 3 == index % 3 { 1.0 } else { 0.0 };
            if (value - expected).abs() > INVERSE_RESIDUAL {
                return Err(ProjectiveError::PrecisionLoss);
            }
        }
    }
    Ok(())
}

/// A signed normalized significand and binary exponent. Only used for
/// rescaling, not for geometry arithmetic or a wider-precision promise.
#[derive(Clone, Copy)]
struct Scaled {
    fraction: f64,
    exponent: i32,
}

impl Scaled {
    const ZERO: Self = Self {
        fraction: 0.0,
        exponent: 0,
    };

    fn new(value: f64) -> Result<Self, ProjectiveError> {
        if !value.is_finite() {
            return Err(ProjectiveError::NonFinite);
        }
        if value == 0.0 {
            return Ok(Self::ZERO);
        }
        // Scaling a subnormal by 2^52 is exact and makes it normal.
        let (value, adjustment) = if value.is_subnormal() {
            (value * 4_503_599_627_370_496.0, -52)
        } else {
            (value, 0)
        };
        let bits = value.to_bits();
        let exponent = ((bits >> 52) & 0x7ff) as i32 - 1023 + adjustment;
        let fraction = f64::from_bits((bits & 0x800f_ffff_ffff_ffff) | (1023_u64 << 52));
        Ok(Self { fraction, exponent })
    }

    fn multiply(self, other: f64) -> Result<Self, ProjectiveError> {
        let other = Self::new(other)?;
        let mut result = Self::new(self.fraction * other.fraction)?;
        result.exponent += self.exponent + other.exponent;
        Ok(result)
    }

    fn divide(self, other: f64) -> Result<Self, ProjectiveError> {
        let other = Self::new(other)?;
        let mut result = Self::new(self.fraction / other.fraction)?;
        result.exponent += self.exponent - other.exponent;
        Ok(result)
    }
}

fn canonicalize_scaled(values: [Scaled; 9]) -> Result<[f64; 9], ProjectiveError> {
    let mut pivot = Scaled::ZERO;
    for value in values {
        if value.fraction != 0.0
            && (pivot.fraction == 0.0
                || value.exponent > pivot.exponent
                || (value.exponent == pivot.exponent
                    && value.fraction.abs() > pivot.fraction.abs()))
        {
            pivot = value;
        }
    }
    if pivot.fraction == 0.0 {
        return Err(ProjectiveError::Singular);
    }
    let mut result = [0.0; 9];
    for (result, value) in result.iter_mut().zip(values) {
        if value.fraction != 0.0 {
            let exponent = value.exponent - pivot.exponent;
            if exponent < -1022 {
                return Err(ProjectiveError::PrecisionLoss);
            }
            *result = (value.fraction / pivot.fraction) * 2.0_f64.powi(exponent);
            if !result.is_normal() {
                return Err(ProjectiveError::PrecisionLoss);
            }
        }
    }
    canonicalize(result)
}

#[derive(Clone, Copy, Debug)]
pub(crate) struct Dot {
    pub(crate) value: f64,
    magnitude: f64,
    pub(crate) error: f64,
}

impl Dot {
    pub(crate) const ZERO: Self = Self {
        value: 0.0,
        magnitude: 0.0,
        error: 0.0,
    };

    pub(crate) fn new(a: [f64; 3], b: [f64; 3]) -> Result<Self, ProjectiveError> {
        let mut result = Self::ZERO;
        let mut correction = 0.0;
        let mut correction_magnitude = 0.0;
        for (a, b) in a.into_iter().zip(b) {
            let product = a * b;
            if a != 0.0 && b != 0.0 && product == 0.0 {
                return Err(ProjectiveError::PrecisionLoss);
            }
            // FMA recovers product roundoff; TwoSum recovers addition
            // roundoff. In particular, do not discard a small numerator
            // obtained by subtracting two large, nearly equal terms.
            let product_error = a.mul_add(b, -product);
            let sum = result.value + product;
            let added = sum - result.value;
            let sum_error = (result.value - (sum - added)) + (product - added);
            correction += sum_error + product_error;
            correction_magnitude += sum_error.abs() + product_error.abs();
            result.value = sum;
            result.magnitude += product.abs();
        }
        result.value += correction;
        if !result.value.is_finite() || !result.magnitude.is_finite() {
            return Err(ProjectiveError::NonFinite);
        }
        // Input scaling was checked exact. The residual bound accounts for
        // correction accumulation and the final rounding, rather than using
        // a first-order bound on the much larger uncancelled products. Eight
        // least subnormals cover absolute product/FMA/final-rounding errors.
        result.error =
            (ROUND_OFF * result.value.abs() + ROUND_OFF * correction_magnitude + f64::from_bits(8))
                .next_up();
        Ok(result)
    }

    fn check_denominator(self) -> Result<(), ProjectiveError> {
        if self.value == 0.0 {
            return Err(ProjectiveError::Horizon);
        }
        if self.value.abs() <= (DENOMINATOR_MARGIN * self.magnitude).max(self.error) {
            return Err(ProjectiveError::UnsafeDomain);
        }
        Ok(())
    }
}

fn check_cartesian_accuracy(value: f64, (min, max): (f64, f64)) -> Result<(), ProjectiveError> {
    let uncertainty = (value - min).max(max - value);
    let budget = CARTESIAN_ABSOLUTE_ERROR + CARTESIAN_RELATIVE_ERROR * value.abs();
    if !value.is_finite() || !uncertainty.is_finite() || uncertainty > budget {
        return Err(ProjectiveError::PrecisionLoss);
    }
    Ok(())
}

fn quotient_bounds(numerator: Dot, denominator: Dot) -> Result<(f64, f64), ProjectiveError> {
    let mut min = f64::INFINITY;
    let mut max = f64::NEG_INFINITY;
    for n in [
        numerator.value - numerator.error,
        numerator.value + numerator.error,
    ] {
        for d in [
            denominator.value - denominator.error,
            denominator.value + denominator.error,
        ] {
            let value = n / d;
            min = min.min(value.next_down());
            max = max.max(value.next_up());
        }
    }
    if !min.is_finite() || !max.is_finite() {
        return Err(ProjectiveError::NonFinite);
    }
    Ok((min, max))
}

fn validate_convex_quad(quad: [DVec2; 4]) -> Result<(), ProjectiveError> {
    let mut winding = None;
    for i in 0..4 {
        let a = quad[(i + 1) % 4] - quad[i];
        let b = quad[(i + 2) % 4] - quad[(i + 1) % 4];
        let cross = a.perp_dot(b);
        let magnitude = (a.x * b.y).abs() + (a.y * b.x).abs();
        if !cross.is_finite() || cross.abs() <= PIVOT_MARGIN * magnitude {
            return Err(ProjectiveError::InvalidQuad);
        }
        if winding.is_some_and(|negative| negative != cross.is_sign_negative()) {
            return Err(ProjectiveError::InvalidQuad);
        }
        winding = Some(cross.is_sign_negative());
    }
    Ok(())
}

fn reprojection_tolerance(rect: ProjectiveRect) -> Result<DVec2, ProjectiveError> {
    let allowance = rect.min.abs().max(rect.max.abs()) / rect.size() * (32.0 * f64::EPSILON)
        + DVec2::splat(REPROJECTION_RESIDUAL);
    if !allowance.is_finite() || allowance.max_element() > MAX_REPROJECTION_TOLERANCE {
        return Err(ProjectiveError::PrecisionLoss);
    }
    Ok(allowance)
}

fn check_reprojection(
    actual: DVec2,
    wanted: DVec2,
    extent: DVec2,
    tolerance: DVec2,
) -> Result<(), ProjectiveError> {
    let error = (actual - wanted).abs() / extent;
    if !error.is_finite() || error.x > tolerance.x || error.y > tolerance.y {
        return Err(ProjectiveError::PrecisionLoss);
    }
    Ok(())
}

/// Construction certificate for G * Translate(-anchor) * F. Bounds include
/// every stored coefficient and both matrix products, not only rounded G's
/// subsequent point evaluation.
struct InverseCertificate {
    quad: [DVec2; 4],
    quad_error: DVec2,
    point_error: DVec2,
    relative_jacobian_error: f64,
}

fn exact_matrix(values: [f64; 9]) -> [Dot; 9] {
    values.map(|value| Dot {
        value,
        magnitude: value.abs(),
        error: 0.0,
    })
}

fn bounded_product(a: [Dot; 9], b: [Dot; 9]) -> Result<[Dot; 9], ProjectiveError> {
    let mut output = [Dot::ZERO; 9];
    for (index, out) in output.iter_mut().enumerate() {
        let (r, c) = (index / 3, index % 3);
        let left: [Dot; 3] = std::array::from_fn(|k| a[r * 3 + k]);
        let right: [Dot; 3] = std::array::from_fn(|k| b[k * 3 + c]);
        *out = Dot::new(left.map(|v| v.value), right.map(|v| v.value))?;
        for (a, b) in left.into_iter().zip(right) {
            for error in [
                a.value.abs() * b.error,
                b.value.abs() * a.error,
                a.error * b.error,
            ] {
                out.error = (out.error + error.next_up()).next_up();
            }
        }
        if !out.error.is_finite() {
            return Err(ProjectiveError::PrecisionLoss);
        }
    }
    Ok(output)
}

pub(crate) fn bounded_row(row: [Dot; 3], point: DVec2) -> Result<Dot, ProjectiveError> {
    let input = [point.x, point.y, 1.0];
    let mut result = Dot::new(row.map(|v| v.value), input)?;
    for (value, input) in row.into_iter().zip(input) {
        result.error = (result.error + (value.error * input.abs()).next_up()).next_up();
    }
    Ok(result)
}

fn difference_bound(a: Dot, b: Dot) -> Result<f64, ProjectiveError> {
    let result = Dot::new([a.value, -b.value, 0.0], [1.0, 1.0, 0.0])?;
    Ok([result.error, a.error, b.error]
        .into_iter()
        .fold(result.value.abs(), |sum, error| (sum + error).next_up()))
}

impl Projective2 {
    fn certify_inverse(
        self,
        forward: Self,
        anchor: DVec2,
        support: ProjectiveRect,
    ) -> Result<InverseCertificate, ProjectiveError> {
        let translated = bounded_product(
            exact_matrix([1.0, 0.0, -anchor.x, 0.0, 1.0, -anchor.y, 0.0, 0.0, 1.0]),
            exact_matrix(forward.coefficients),
        )?;
        let residual = bounded_product(exact_matrix(self.coefficients), translated)?;
        let mut quad = [DVec2::ZERO; 4];
        let mut quad_error = DVec2::ZERO;
        let mut denominator_min = f64::INFINITY;
        let mut sign = 0.0;
        for (out, point) in quad.iter_mut().zip(support.corners()) {
            let denominator = bounded_row([residual[6], residual[7], residual[8]], point)?;
            denominator.check_denominator()?;
            if sign != 0.0 && denominator.value.signum() != sign {
                return Err(ProjectiveError::Horizon);
            }
            sign = denominator.value.signum();
            denominator_min =
                denominator_min.min((denominator.value.abs() - denominator.error).next_down());
            let mut values = [Dot::ZERO; 3];
            for (r, value) in values.iter_mut().enumerate() {
                *value = bounded_row(
                    [
                        translated[r * 3],
                        translated[r * 3 + 1],
                        translated[r * 3 + 2],
                    ],
                    point,
                )?;
            }
            values[2].check_denominator()?;
            *out = DVec2::new(
                values[0].value / values[2].value,
                values[1].value / values[2].value,
            );
            for axis in 0..2 {
                let (low, high) = quotient_bounds(values[axis], values[2])?;
                quad_error[axis] =
                    quad_error[axis].max((out[axis] - low).max(high - out[axis]).next_up());
            }
        }
        if denominator_min <= 0.0 || !denominator_min.is_finite() {
            return Err(ProjectiveError::UnsafeDomain);
        }
        let a = residual.map(|v| (v.value.abs() + v.error).next_up());
        let (dx, dy) = (
            difference_bound(residual[0], residual[8])?,
            difference_bound(residual[4], residual[8])?,
        );
        let extent = support.min.abs().max(support.max.abs());
        let (x, y) = (extent.x, extent.y);
        // G(F(s))-s has quadratic numerators. Corner reprojection alone cannot
        // certify these residuals; bound every x², xy and y² term on the domain.
        let numerator = DVec2::new(
            dx * x + a[1] * y + a[2] + a[6] * x * x + a[7] * x * y,
            a[3] * x + dy * y + a[5] + a[6] * x * y + a[7] * y * y,
        ) * (1.0 + 64.0 * f64::EPSILON);
        let point_error = numerator / denominator_min * (1.0 + 16.0 * f64::EPSILON);
        let derivative = [
            (dx + 2.0 * a[6] * x + a[7] * y + point_error.x * a[6]) / denominator_min,
            (a[1] + a[7] * x + point_error.x * a[7]) / denominator_min,
            (a[3] + a[6] * y + point_error.y * a[6]) / denominator_min,
            (dy + a[6] * x + 2.0 * a[7] * y + point_error.y * a[7]) / denominator_min,
        ];
        let relative_jacobian_error =
            derivative.into_iter().fold(0.0_f64, f64::hypot) * (1.0 + 64.0 * f64::EPSILON);
        if !point_error.is_finite()
            || !quad_error.is_finite()
            || !relative_jacobian_error.is_finite()
            || point_error.max_element() >= 1e-6
            || relative_jacobian_error >= 1e-6
        {
            return Err(ProjectiveError::PrecisionLoss);
        }
        // J_G * J_F = J_residual, so ||J_residual-I|| bounds the relative
        // inverse-Jacobian error in operator norm. Frobenius bounds that norm.
        Ok(InverseCertificate {
            quad,
            quad_error,
            point_error,
            relative_jacobian_error,
        })
    }
}

/// A restricted inverse-evaluation domain for retained-pixel sampling.
///
/// Admission combines inverse construction, local document subtraction and
/// evaluation in one 1e-6 source-pixel budget, and certifies relative Jacobian
/// operator-norm error within 1e-6, over the original forward support quad (not
/// its enclosing rectangle). This stricter render capability is separate from
/// general projective geometry. A local document frame avoids losing useful
/// precision solely because the layer is far from the canvas.
#[derive(Clone, Copy, Debug, PartialEq)]
pub(crate) struct ProjectiveDifferential {
    coefficients: [f64; 9],
    input_min: DVec2,
    input_max: DVec2,
    output: ProjectiveRect,
    denominator_sign: f64,
    denominator_min: f64,
}

impl Projective2 {
    pub(crate) fn checked_differential(
        self,
        forward: Self,
        anchor: DVec2,
        output: ProjectiveRect,
    ) -> Result<ProjectiveDifferential, ProjectiveError> {
        let certificate = self.certify_inverse(forward, anchor, output)?;
        let quad = certificate.quad;
        validate_convex_quad(quad)?;
        let input_min = quad.into_iter().fold(quad[0], DVec2::min) - certificate.quad_error;
        let input_max = quad.into_iter().fold(quad[0], DVec2::max) + certificate.quad_error;
        let magnitude = input_min.abs().max(input_max.abs());
        // Output centers are exact binary coordinates in the admitted integer
        // document bounds. This includes rounding of document_point - anchor;
        // cancellation itself is exact where Sterbenz's lemma applies.
        let input_error = magnitude * (2.0 * f64::EPSILON) + DVec2::splat(1e-290);
        // Bounded render coordinates and coefficients prevent overflow in all
        // products below. Tiny coefficients with unsupported relative precision
        // are explicitly refused, rather than choosing mip zero after overflow.
        if !magnitude.is_finite()
            || magnitude.max_element() > 1e12
            || self
                .coefficients
                .iter()
                .any(|v| *v != 0.0 && v.abs() < 1e-100)
        {
            return Err(ProjectiveError::PrecisionLoss);
        }
        let mut minimum = f64::INFINITY;
        let mut sign = 0.0;
        for point in quad {
            let values = self.evaluate(point, 1.0)?;
            values[2].check_denominator()?;
            let s = values[2].value.signum();
            if sign != 0.0 && s != sign {
                return Err(ProjectiveError::Horizon);
            }
            sign = s;
            minimum = minimum.min(values[2].value.abs() - values[2].error);
            self.map_point(point)?;
        }
        let mut sums = [0.0; 3];
        for (sum, row) in sums.iter_mut().zip(self.coefficients.as_chunks::<3>().0) {
            *sum = row[0].abs() * magnitude.x + row[1].abs() * magnitude.y + row[2].abs();
        }
        // A first-order bound is intentionally more conservative than map_point's
        // compensated pointwise estimate. It admits every interior pixel at once.
        let mut errors = sums.map(|v| (64.0 * f64::EPSILON * v + 1e-290).next_up());
        for (error, row) in errors.iter_mut().zip(self.coefficients.as_chunks::<3>().0) {
            *error =
                (*error + row[0].abs() * input_error.x + row[1].abs() * input_error.y).next_up();
        }
        minimum -= errors[2]
            + self.coefficients[6].abs() * certificate.quad_error.x
            + self.coefficients[7].abs() * certificate.quad_error.y;
        if minimum <= 1e-100 || minimum <= 4.0 * errors[2] {
            return Err(ProjectiveError::UnsafeDomain);
        }
        let limit = output.min.abs().max(output.max.abs()) + DVec2::ONE;
        let error = DVec2::new(errors[0], errors[1]) / minimum
            + limit * (errors[2] / minimum + 16.0 * f64::EPSILON);
        if !error.is_finite() || (error + certificate.point_error).max_element() > 1e-6 {
            return Err(ProjectiveError::PrecisionLoss);
        }
        let [a, b, _, d, e, _, g, h, _] = self.coefficients;
        let derivative_bound = [
            (a.abs() + g.abs() * limit.x) / minimum,
            (b.abs() + h.abs() * limit.x) / minimum,
            (d.abs() + g.abs() * limit.y) / minimum,
            (e.abs() + h.abs() * limit.y) / minimum,
        ];
        if derivative_bound
            .iter()
            .any(|v| !v.is_finite() || *v > 1e100)
        {
            return Err(ProjectiveError::PrecisionLoss);
        }
        // Prove at least one Jacobian entry stays nonzero, and bound absolute
        // derivative error relative to that lower bound on sigma_max. This
        // conservatively rejects domains whose differential cannot be certified
        // without subdivision; it never substitutes mip zero for lost precision.
        let mut sigma_lower: f64 = 0.0;
        let mut derivative_error: f64 = 0.0;
        for (index, (coefficient, perspective, axis)) in
            [(a, g, 0), (b, h, 0), (d, g, 1), (e, h, 1)]
                .into_iter()
                .enumerate()
        {
            let terms = coefficient.abs() + perspective.abs() * limit[axis];
            let numerator_error = 64.0 * f64::EPSILON * terms + perspective.abs() * error[axis];
            let ends = [output.min[axis], output.max[axis]]
                .map(|v| (-perspective).mul_add(v, coefficient));
            if ends[0].signum() == ends[1].signum() {
                let lower = (ends[0].abs().min(ends[1].abs()) - numerator_error).max(0.0);
                sigma_lower = sigma_lower.max(lower / (sums[2] + errors[2]));
            }
            derivative_error = derivative_error.max(
                (numerator_error + derivative_bound[index] * errors[2]) / minimum
                    + 16.0 * f64::EPSILON * derivative_bound[index],
            );
        }
        if !sigma_lower.is_finite()
            || sigma_lower <= 1e-100
            || !derivative_error.is_finite()
            || certificate.relative_jacobian_error
                + (2.0 * derivative_error / sigma_lower)
                    * (1.0 + certificate.relative_jacobian_error)
                > 1e-6
        {
            return Err(ProjectiveError::PrecisionLoss);
        }
        Ok(ProjectiveDifferential {
            coefficients: self.coefficients,
            input_min: input_min - input_error,
            input_max: input_max + input_error,
            output,
            denominator_sign: sign,
            denominator_min: minimum,
        })
    }
}

impl ProjectiveDifferential {
    /// None means exterior support, including an inverse horizon in the quad's
    /// enclosing AABB. No integer conversion or source access occurs here.
    /// In-support arithmetic is admitted uniformly by checked_differential;
    /// there is no pointwise precision-error-to-transparent fallback.
    pub(crate) fn sample(self, point: DVec2) -> Option<(DVec2, [f64; 4])> {
        if !point.is_finite()
            || point.cmplt(self.input_min).any()
            || point.cmpgt(self.input_max).any()
        {
            return None;
        }
        let [a, b, c, d, e, f, g, h, i] = self.coefficients;
        let w = g.mul_add(point.x, h.mul_add(point.y, i));
        if w * self.denominator_sign < self.denominator_min {
            return None;
        }
        let mapped = DVec2::new(
            a.mul_add(point.x, b.mul_add(point.y, c)) / w,
            d.mul_add(point.x, e.mul_add(point.y, f)) / w,
        );
        if mapped.cmplt(self.output.min).any() || mapped.cmpgt(self.output.max).any() {
            return None;
        }
        // Quotient rule without an unchecked determinant, squaring a tiny
        // denominator or normalizing by H[8]. The admission bounds cover each
        // intermediate. FMA retains near-cancelling derivative numerators.
        let jacobian = [
            (-g).mul_add(mapped.x, a) / w,
            (-h).mul_add(mapped.x, b) / w,
            (-g).mul_add(mapped.y, d) / w,
            (-h).mul_add(mapped.y, e) / w,
        ];
        Some((mapped, jacobian))
    }
}

#[cfg(test)]
#[path = "projective_tests.rs"]
mod tests;
