//! A checked, finite-grid, level-zero sampler for relative projective masks.
//!
//! Unlike retained pixel content, the intrinsic mask's forward support may
//! cross a horizon. The authority is the stored C, interpreted as exact binary
//! coefficients: adj(C) * (output_center + integer_cache_origin, 1). No rounded
//! inverse, independently rounded composition, or forward AABB defines C.
//!
//! Preparation visits every output center, using the same evaluator as later
//! derivation. Work is O(output pixels), with constant certification scratch
//! and one output tile of derivation scratch. At the admitted 400-MP ceiling
//! the two passes can be expensive; this primitive makes no latency/gesture
//! guarantee. Callers should cache plans by their complete key and may cancel
//! at tile boundaries. There is no silently smaller resource/capability cap.

use crate::projective::{Dot, Projective2, ProjectiveError, bounded_row};
use crate::projective_sample::valid_plane_size;
use crate::{Mask, TILE, TILE_PX, TileCoord};
use glam::DVec2;
use thiserror::Error;

const COORDINATE_ERROR: f64 = 1e-6;

#[derive(Clone, Copy, Debug, PartialEq, Eq, Hash)]
pub struct MaskOutputGrid {
    pub size: (u32, u32),
    /// Output pixel zero in retained-source coordinates.
    pub offset: (i32, i32),
}

/// Complete geometry/support identity, suitable for an immutable plan cache.
/// Coverage caches additionally key the processed plane/content/properties.
#[derive(Clone, Copy, Debug, PartialEq, Eq, Hash)]
pub struct MaskGridKey {
    pub authored_coefficients: [u64; 9],
    pub raw_size: (u32, u32),
    pub processed_origin: (i32, i32),
    pub processed_size: (u32, u32),
    pub output: MaskOutputGrid,
}

#[derive(Clone, Copy, Debug, Error, PartialEq, Eq)]
pub enum MaskSampleError {
    #[error(transparent)]
    Geometry(#[from] ProjectiveError),
    #[error("mask and output dimensions must be positive, at most 30,000 per side and 400 MP")]
    Dimensions,
    #[error("processed feather support exceeds supported mask dimensions")]
    ProcessedDimensions,
    #[error("processed mask dimensions do not match the certified support")]
    ProcessedMismatch,
    #[error("mask sample lies outside the certified output grid")]
    OutputCoordinate,
    #[error("authored projective mask sample cannot be certified within 1e-6 intrinsic pixels")]
    UncertainSample,
    #[error("projective mask preparation or derivation was cancelled")]
    Cancelled,
}

#[derive(Clone, Copy, Debug, PartialEq)]
pub enum MaskGridSample {
    /// Level-zero coordinate in the processed plane, including feather halo.
    Detail(DVec2),
    /// Proved outside processed bilinear support (or at an inverse pole).
    Exterior,
}

/// No public unchecked constructor; every center was certified before return.
#[derive(Clone, Debug)]
pub struct MaskGridPlan {
    key: MaskGridKey,
    authored: Projective2,
    adjugate: [Dot; 9],
    halo: f64,
}

pub fn prepare_mask_grid(
    authored: Projective2,
    raw_size: (u32, u32),
    processed_halo: u32,
    output: MaskOutputGrid,
) -> Result<MaskGridPlan, MaskSampleError> {
    prepare_mask_grid_with_control(authored, raw_size, processed_halo, output, || true)
}

/// The control hook runs before each output tile, including the first. False
/// aborts without publishing a plan. It neither changes acceptance nor supplies
/// a numerical fallback. The largest unchecked batch is TILE_PX centers.
pub fn prepare_mask_grid_with_control(
    authored: Projective2,
    raw_size: (u32, u32),
    processed_halo: u32,
    output: MaskOutputGrid,
    mut keep_going: impl FnMut() -> bool,
) -> Result<MaskGridPlan, MaskSampleError> {
    if !valid_plane_size(raw_size.0, raw_size.1) || !valid_plane_size(output.size.0, output.size.1)
    {
        return Err(MaskSampleError::Dimensions);
    }
    let twice_halo = processed_halo
        .checked_mul(2)
        .ok_or(MaskSampleError::ProcessedDimensions)?;
    let processed_size = (
        raw_size.0.checked_add(twice_halo),
        raw_size.1.checked_add(twice_halo),
    );
    let (Some(width), Some(height)) = processed_size else {
        return Err(MaskSampleError::ProcessedDimensions);
    };
    if !valid_plane_size(width, height) {
        return Err(MaskSampleError::ProcessedDimensions);
    }
    // Valid processed dimensions imply halo < 15,000, so negation is exact.
    let halo = processed_halo as i32;
    let coefficients = authored.to_row_major();
    let plan = MaskGridPlan {
        key: MaskGridKey {
            authored_coefficients: coefficients.map(f64::to_bits),
            raw_size,
            processed_origin: (-halo, -halo),
            processed_size: (width, height),
            output,
        },
        authored,
        adjugate: bounded_adjugate(coefficients)?,
        halo: f64::from(halo),
    };
    for ty in (0..output.size.1).step_by(TILE as usize) {
        for tx in (0..output.size.0).step_by(TILE as usize) {
            if !keep_going() {
                return Err(MaskSampleError::Cancelled);
            }
            for y in ty..(ty + TILE).min(output.size.1) {
                for x in tx..(tx + TILE).min(output.size.0) {
                    plan.sample(x, y)?;
                }
            }
        }
    }
    Ok(plan)
}

impl MaskGridPlan {
    pub fn key(&self) -> MaskGridKey {
        self.key
    }

    pub fn authored(&self) -> Projective2 {
        self.authored
    }

    /// No integer conversion or pixel lookup occurs until classification.
    pub fn sample(&self, x: u32, y: u32) -> Result<MaskGridSample, MaskSampleError> {
        if x >= self.key.output.size.0 || y >= self.key.output.size.1 {
            return Err(MaskSampleError::OutputCoordinate);
        }
        let point = self.authored_center(x, y);
        let mut values = [Dot::ZERO; 3];
        for (r, value) in values.iter_mut().enumerate() {
            *value = bounded_row(
                [
                    self.adjugate[3 * r],
                    self.adjugate[3 * r + 1],
                    self.adjugate[3 * r + 2],
                ],
                point,
            )?;
        }
        if let Some(sample) = self.classify_bounded(values) {
            return Ok(sample);
        }
        self.classify_exact(point)
    }

    fn authored_center(&self, x: u32, y: u32) -> DVec2 {
        let center = |index: u32, origin: i32| {
            // Side <= 30,000, origin is i32: this odd integer has magnitude
            // < 2^33. Conversion and multiplication by 1/2 are both exact.
            let twice = 2 * (i64::from(origin) + i64::from(index)) + 1;
            twice as f64 * 0.5
        };
        DVec2::new(
            center(x, self.key.output.offset.0),
            center(y, self.key.output.offset.1),
        )
    }

    fn classify_bounded(&self, values: [Dot; 3]) -> Option<MaskGridSample> {
        let intervals = values.map(Interval::from_dot);
        let d = intervals[2];
        let sizes = [self.key.processed_size.0, self.key.processed_size.1];
        // A division-free exterior proof remains valid when d contains zero:
        // |n| > max(|intrinsic support endpoints|) * |d| excludes all detail,
        // including either side of the pole. Strictly nonzero n also excludes
        // the indeterminate homogeneous vector when the exact d is zero.
        for (axis, size) in sizes.into_iter().enumerate() {
            let radius = (-0.5 - self.halo)
                .abs()
                .max(f64::from(size) + 0.5 - self.halo);
            if intervals[axis].min_abs() > (radius * d.max_abs()).next_up() {
                return Some(MaskGridSample::Exterior);
            }
        }
        if d.lo <= 0.0 && d.hi >= 0.0 {
            return None;
        }
        let bounds = [
            intervals[0].divide(d)?.translate(self.halo),
            intervals[1].divide(d)?.translate(self.halo),
        ];
        if (0..2)
            .any(|axis| bounds[axis].hi <= -0.5 || bounds[axis].lo >= f64::from(sizes[axis]) + 0.5)
        {
            return Some(MaskGridSample::Exterior);
        }
        let coordinate = DVec2::new(
            values[0].value / values[2].value,
            values[1].value / values[2].value,
        ) + DVec2::splat(self.halo);
        self.accept_detail(coordinate, bounds)
    }

    fn accept_detail(&self, coordinate: DVec2, bounds: [Interval; 2]) -> Option<MaskGridSample> {
        let sizes = [self.key.processed_size.0, self.key.processed_size.1];
        for (axis, size) in sizes.into_iter().enumerate() {
            let b = bounds[axis];
            let value = coordinate[axis];
            let upper = f64::from(size) + 0.5;
            if !value.is_finite()
                || !b.lo.is_finite()
                || !b.hi.is_finite()
                || b.lo <= -0.5
                || b.hi >= upper
                || value <= -0.5
                || value >= upper
                || (value - b.lo).max(b.hi - value).next_up() > COORDINATE_ERROR
            {
                return None;
            }
        }
        Some(MaskGridSample::Detail(coordinate))
    }

    fn classify_exact(&self, point: DVec2) -> Result<MaskGridSample, MaskSampleError> {
        let adjugate = exact_adjugate(self.authored.to_row_major())?;
        let mut values = [Expansion::ZERO; 3];
        for (r, value) in values.iter_mut().enumerate() {
            *value = adjugate[3 * r]
                .scale(point.x)?
                .add(adjugate[3 * r + 1].scale(point.y)?)?
                .add(adjugate[3 * r + 2])?;
        }
        let denominator = values[2];
        let sign = denominator.sign();
        if sign == 0 {
            // Projective2 guarantees invertibility; verify a nonzero numerator
            // rather than interpreting an uncertain/zero vector as exterior.
            return if values[0].sign() != 0 || values[1].sign() != 0 {
                Ok(MaskGridSample::Exterior)
            } else {
                Err(MaskSampleError::UncertainSample)
            };
        }
        let sizes = [self.key.processed_size.0, self.key.processed_size.1];
        for (axis, size) in sizes.into_iter().enumerate() {
            let lower = -0.5 - self.halo;
            let upper = f64::from(size) + 0.5 - self.halo;
            if quotient_compare(values[axis], denominator, lower)? <= 0
                || quotient_compare(values[axis], denominator, upper)? >= 0
            {
                return Ok(MaskGridSample::Exterior);
            }
        }
        // Exact signs alone are insufficient for Detail. Also enclose the
        // quotient and final halo addition, wholly inside bilinear support.
        let bounds = [
            exact_quotient_bounds(values[0], denominator)?.translate(self.halo),
            exact_quotient_bounds(values[1], denominator)?.translate(self.halo),
        ];
        let coordinate = DVec2::new(
            values[0].estimate() / denominator.estimate() + self.halo,
            values[1].estimate() / denominator.estimate() + self.halo,
        );
        self.accept_detail(coordinate, bounds)
            .ok_or(MaskSampleError::UncertainSample)
    }

    pub fn derive(&self, processed: &Mask) -> Result<Mask, MaskSampleError> {
        self.derive_with_control(processed, || true)
    }

    /// The processed plane already includes intrinsic density/feather once.
    /// Its actual fill (including density changes) is used outside support.
    /// A failure/cancellation drops the local result; no partial plane escapes.
    pub fn derive_with_control(
        &self,
        processed: &Mask,
        mut keep_going: impl FnMut() -> bool,
    ) -> Result<Mask, MaskSampleError> {
        if (processed.width(), processed.height()) != self.key.processed_size {
            return Err(MaskSampleError::ProcessedMismatch);
        }
        let (width, height) = self.key.output.size;
        let fill = processed.fill();
        let mut output = Mask::empty(width, height, fill);
        for ty in (0..height).step_by(TILE as usize) {
            for tx in (0..width).step_by(TILE as usize) {
                if !keep_going() {
                    return Err(MaskSampleError::Cancelled);
                }
                let mut tile = vec![fill; TILE_PX];
                for y in ty..(ty + TILE).min(height) {
                    for x in tx..(tx + TILE).min(width) {
                        tile[((y - ty) * TILE + x - tx) as usize] = match self.sample(x, y)? {
                            MaskGridSample::Exterior => fill,
                            MaskGridSample::Detail(point) => sample_level_zero(processed, point),
                        };
                    }
                }
                output.set_tile(TileCoord::new((tx / TILE) as i32, (ty / TILE) as i32), tile);
            }
        }
        Ok(output)
    }
}

// adj(C)[i] = C[a]*C[b] - C[c]*C[d]. Keep the same term order for
// compensated bounds and exact fallback; never invert a rounded Q instead.
const COFACTORS: [[usize; 4]; 9] = [
    [4, 8, 5, 7],
    [2, 7, 1, 8],
    [1, 5, 2, 4],
    [5, 6, 3, 8],
    [0, 8, 2, 6],
    [2, 3, 0, 5],
    [3, 7, 4, 6],
    [1, 6, 0, 7],
    [0, 4, 1, 3],
];

fn bounded_adjugate(c: [f64; 9]) -> Result<[Dot; 9], MaskSampleError> {
    let mut adjugate = [Dot::ZERO; 9];
    for (result, [a, b, d, e]) in adjugate.iter_mut().zip(COFACTORS) {
        *result = Dot::new([c[a], -c[d], 0.0], [c[b], c[e], 0.0])?;
    }
    Ok(adjugate)
}

#[derive(Clone, Copy, Debug)]
struct Interval {
    lo: f64,
    hi: f64,
}

impl Interval {
    fn from_dot(value: Dot) -> Self {
        Self {
            lo: (value.value - value.error).next_down(),
            hi: (value.value + value.error).next_up(),
        }
    }
    fn min_abs(self) -> f64 {
        if self.lo <= 0.0 && self.hi >= 0.0 {
            0.0
        } else {
            self.lo.abs().min(self.hi.abs())
        }
    }
    fn max_abs(self) -> f64 {
        self.lo.abs().max(self.hi.abs())
    }
    fn divide(self, denominator: Self) -> Option<Self> {
        let quotients = [
            self.lo / denominator.lo,
            self.lo / denominator.hi,
            self.hi / denominator.lo,
            self.hi / denominator.hi,
        ];
        if !quotients.into_iter().all(f64::is_finite) {
            return None;
        }
        Some(Self {
            lo: quotients
                .into_iter()
                .fold(f64::INFINITY, f64::min)
                .next_down(),
            hi: quotients
                .into_iter()
                .fold(f64::NEG_INFINITY, f64::max)
                .next_up(),
        })
    }
    fn translate(self, halo: f64) -> Self {
        // Error-free TwoSum allows exact representable boundaries to remain
        // exact; a blanket nextafter would needlessly straddle those edges.
        let (lo, low_error) = two_sum(self.lo, halo);
        let (hi, high_error) = two_sum(self.hi, halo);
        Self {
            lo: if low_error < 0.0 { lo.next_down() } else { lo },
            hi: if high_error > 0.0 { hi.next_up() } else { hi },
        }
    }
}

/// Bounded exact expansion fallback. A cofactor has <=4 components, a row
/// <=20, and numerator - scalar*denominator <=60. No arbitrary precision
/// allocation or unbounded search is used. Unsupported product exponent range
/// refuses instead of silently dropping a tiny exact component.
#[derive(Clone, Copy)]
struct Expansion {
    values: [f64; 64],
    len: usize,
}

impl Expansion {
    const ZERO: Self = Self {
        values: [0.0; 64],
        len: 0,
    };

    fn push(self, value: f64) -> Result<Self, MaskSampleError> {
        let mut result = Self::ZERO;
        let mut accumulator = value;
        for &component in &self.values[..self.len] {
            let (sum, error) = two_sum(accumulator, component);
            if !sum.is_finite() {
                return Err(MaskSampleError::UncertainSample);
            }
            if error != 0.0 {
                result.append(error)?;
            }
            accumulator = sum;
        }
        if accumulator != 0.0 {
            result.append(accumulator)?;
        }
        Ok(result)
    }
    fn append(&mut self, value: f64) -> Result<(), MaskSampleError> {
        if self.len == self.values.len() {
            return Err(MaskSampleError::UncertainSample);
        }
        self.values[self.len] = value;
        self.len += 1;
        Ok(())
    }
    fn product(a: f64, b: f64) -> Result<Self, MaskSampleError> {
        if !a.is_finite() || !b.is_finite() {
            return Err(MaskSampleError::UncertainSample);
        }
        if a == 0.0 || b == 0.0 {
            return Ok(Self::ZERO);
        }
        // The exact product's least significant bit must remain representable.
        // Then product + FMA residual is an exact error-free transformation,
        // including subnormal residuals. Checking only rounded product != 0
        // would not be sufficient.
        if least_exponent(a) + least_exponent(b) < -1074 {
            return Err(MaskSampleError::UncertainSample);
        }
        let product = a * b;
        if !product.is_finite() {
            return Err(MaskSampleError::UncertainSample);
        }
        Self::ZERO.push(a.mul_add(b, -product))?.push(product)
    }
    fn add(mut self, other: Self) -> Result<Self, MaskSampleError> {
        for &value in &other.values[..other.len] {
            self = self.push(value)?;
        }
        Ok(self)
    }
    fn scale(self, factor: f64) -> Result<Self, MaskSampleError> {
        let mut result = Self::ZERO;
        for &value in &self.values[..self.len] {
            result = result.add(Self::product(value, factor)?)?;
        }
        Ok(result)
    }
    fn sign(self) -> i8 {
        if self.len == 0 {
            0
        } else if self.values[self.len - 1] > 0.0 {
            1
        } else {
            -1
        }
    }
    fn estimate(self) -> f64 {
        self.values[..self.len].iter().copied().sum()
    }
}

fn two_sum(a: f64, b: f64) -> (f64, f64) {
    let sum = a + b;
    let virtual_b = sum - a;
    (sum, (a - (sum - virtual_b)) + (b - virtual_b))
}

fn least_exponent(value: f64) -> i32 {
    let bits = value.to_bits() & 0x7fff_ffff_ffff_ffff;
    let exponent = ((bits >> 52) & 0x7ff) as i32;
    let fraction = bits & 0x000f_ffff_ffff_ffff;
    let (significand, base) = if exponent == 0 {
        (fraction, -1074)
    } else {
        (fraction | (1_u64 << 52), exponent - 1023 - 52)
    };
    base + significand.trailing_zeros() as i32
}

fn exact_adjugate(c: [f64; 9]) -> Result<[Expansion; 9], MaskSampleError> {
    let mut adjugate = [Expansion::ZERO; 9];
    for (result, [a, b, d, e]) in adjugate.iter_mut().zip(COFACTORS) {
        *result = Expansion::product(c[a], c[b])?.add(Expansion::product(-c[d], c[e])?)?;
    }
    Ok(adjugate)
}

fn quotient_compare(n: Expansion, d: Expansion, candidate: f64) -> Result<i8, MaskSampleError> {
    Ok(n.add(d.scale(-candidate)?)?.sign() * d.sign())
}

fn exact_quotient_bounds(n: Expansion, d: Expansion) -> Result<Interval, MaskSampleError> {
    let mut candidate = n.estimate() / d.estimate();
    if !candidate.is_finite() {
        return Err(MaskSampleError::UncertainSample);
    }
    let mut comparison = quotient_compare(n, d, candidate)?;
    if comparison == 0 {
        return Ok(Interval {
            lo: candidate,
            hi: candidate,
        });
    }
    // A short bounded refinement, not a data-dependent unbounded search.
    // If expansion estimation ever exceeds this bracket, refuse preparation.
    for _ in 0..16 {
        let next = if comparison > 0 {
            candidate.next_up()
        } else {
            candidate.next_down()
        };
        if !next.is_finite() {
            break;
        }
        let next_comparison = quotient_compare(n, d, next)?;
        if next_comparison == 0 {
            return Ok(Interval { lo: next, hi: next });
        }
        if comparison != next_comparison {
            return Ok(Interval {
                lo: candidate.min(next),
                hi: candidate.max(next),
            });
        }
        candidate = next;
        comparison = next_comparison;
    }
    Err(MaskSampleError::UncertainSample)
}

// Deliberately the existing core::transform::sample_mask operation order and
// f64 arithmetic. No narrowing, mip selection, or universal-zero fill policy.
fn sample_level_zero(mask: &Mask, source: DVec2) -> u8 {
    let p = source - DVec2::splat(0.5);
    let x = p.x.floor();
    let y = p.y.floor();
    let get = |x: f64, y: f64| -> f64 {
        if x < 0.0 || y < 0.0 || x >= f64::from(mask.width()) || y >= f64::from(mask.height()) {
            f64::from(mask.fill())
        } else {
            f64::from(mask.get(x as u32, y as u32))
        }
    };
    let ax = p.x - x;
    let ay = p.y - y;
    let top = get(x, y) * (1.0 - ax) + get(x + 1.0, y) * ax;
    let bot = get(x, y + 1.0) * (1.0 - ax) + get(x + 1.0, y + 1.0) * ax;
    (top * (1.0 - ay) + bot * ay).round().clamp(0.0, 255.0) as u8
}

#[cfg(test)]
mod tests {
    // Core-adapter integration obligations (not claimed by these primitive
    // fixtures): use core's real processed_source feather/density results,
    // including its actual halo/fill, and compare derived bytes directly with
    // core::transform::sample_mask on safe affine-equivalent mappings. Raster
    // must not depend on core merely to run those higher-level regressions.
    use super::*;
    use crate::projective::ProjectiveRect;
    use crate::projective_sample::ProjectivePixelMapping;
    use glam::DAffine2;

    fn grid(size: (u32, u32), offset: (i32, i32)) -> MaskOutputGrid {
        MaskOutputGrid { size, offset }
    }

    fn detail(sample: MaskGridSample) -> DVec2 {
        match sample {
            MaskGridSample::Detail(point) => point,
            MaskGridSample::Exterior => panic!("expected certified detail"),
        }
    }

    #[test]
    fn cancellation_keeps_intrinsic_detail_across_forward_horizon() {
        let h =
            Projective2::from_row_major([1.0, 0.0, 0.0, 0.0, 1.0, 0.0, 0.125, 0.0, 1.0]).unwrap();
        let c = h.inverse().unwrap();
        let original = c.to_row_major();
        assert!(ProjectivePixelMapping::new(4, 4, h).is_ok());
        assert!(
            c.map_rect(ProjectiveRect::new(DVec2::ZERO, DVec2::new(16.0, 4.0)).unwrap())
                .is_err()
        );
        assert_eq!(h.compose(c).unwrap(), Projective2::IDENTITY);
        let plan = prepare_mask_grid(c, (16, 4), 0, grid((4, 4), (0, 0))).unwrap();
        // Independent Fraction oracle: (x,y)/(1+x/8), not a rounded inverse.
        for (x, y, expected) in [
            (0, 0, DVec2::splat(8.0 / 17.0)),
            (3, 3, DVec2::splat(56.0 / 23.0)),
        ] {
            assert!(
                (detail(plan.sample(x, y).unwrap()) - expected)
                    .abs()
                    .max_element()
                    <= COORDINATE_ERROR
            );
        }
        for fill in [0, 255] {
            let raw = Mask::from_fn(16, 4, fill, |x, y| ((x * 17 + y * 31) % 256) as u8);
            let id = raw.content_id();
            let retained_off_grid = raw.get(15, 3);
            let result = plan.derive(&raw).unwrap();
            for y in 0..4 {
                for x in 0..4 {
                    let q = DVec2::new(f64::from(x) + 0.5, f64::from(y) + 0.5);
                    assert_eq!(
                        result.get(x, y),
                        sample_level_zero(&raw, q / (1.0 + q.x / 8.0))
                    );
                }
            }
            assert_eq!(raw.content_id(), id);
            assert_eq!(raw.get(15, 3), retained_off_grid);
            assert_eq!(result.fill(), fill);
        }
        assert_eq!(plan.authored().to_row_major(), original);
    }

    #[test]
    fn exact_inverse_poles_and_both_denominator_signs_preserve_white_fill() {
        let c =
            Projective2::from_row_major([0.5, 0.0, 0.0, 0.0, 0.5, 0.0, 1.0, 0.0, -1.0]).unwrap();
        let plan = prepare_mask_grid(c, (4, 4), 0, grid((4, 3), (-1, -1))).unwrap();
        // adj(C) gives q/(q.x-.5): the middle column is exactly the pole.
        for y in 0..3 {
            assert_eq!(plan.sample(1, y).unwrap(), MaskGridSample::Exterior);
        }
        assert_eq!(detail(plan.sample(0, 0).unwrap()), DVec2::splat(0.5));
        assert_eq!(detail(plan.sample(2, 1).unwrap()), DVec2::new(1.5, 0.5));
        let raw = Mask::from_fn(4, 4, 255, |_, _| 0);
        let output = plan.derive(&raw).unwrap();
        assert_eq!(output.get(0, 0), 0);
        assert_eq!(output.get(2, 1), 0);
        for y in 0..3 {
            assert_eq!(output.get(1, y), 255);
        }
    }

    #[test]
    fn exact_support_boundaries_are_exterior_without_an_epsilon() {
        for offset in [(-1, 0), (4, 0), (0, -1), (0, 4)] {
            let plan =
                prepare_mask_grid(Projective2::IDENTITY, (4, 4), 0, grid((1, 1), offset)).unwrap();
            assert_eq!(plan.sample(0, 0).unwrap(), MaskGridSample::Exterior);
        }
        // One exact ULP inside/outside is classified by the authored dyadic
        // map, or rejected if a wholly-in-support enclosure is unavailable.
        for translation in [1.0_f64.next_down(), 1.0, 1.0_f64.next_up()] {
            let c =
                Projective2::from_row_major([1.0, 0.0, translation, 0.0, 1.0, 0.0, 0.0, 0.0, 1.0])
                    .unwrap();
            if let Ok(plan) = prepare_mask_grid(c, (4, 4), 0, grid((1, 1), (0, 0))) {
                let point = plan.sample(0, 0).unwrap();
                if translation < 1.0 {
                    assert!(matches!(point, MaskGridSample::Detail(_)));
                } else {
                    assert_eq!(point, MaskGridSample::Exterior);
                }
            }
        }
    }

    #[test]
    fn feather_halo_and_density_processed_fill_are_included_once() {
        let c = Projective2::IDENTITY;
        let without_halo = prepare_mask_grid(c, (2, 2), 0, grid((1, 1), (-1, 0))).unwrap();
        assert_eq!(without_halo.sample(0, 0).unwrap(), MaskGridSample::Exterior);
        let plan = prepare_mask_grid(c, (2, 2), 2, grid((4, 1), (-3, 0))).unwrap();
        // A caller-supplied processed plane stands for the already feathered,
        // density-adjusted result. Its halo has actual detail outside raw bounds.
        let processed = Mask::from_fn(6, 6, 173, |x, _| if x == 1 { 91 } else { 173 });
        assert_eq!(plan.key().processed_origin, (-2, -2));
        assert_eq!(plan.key().processed_size, (6, 6));
        let output = plan.derive(&processed).unwrap();
        assert_eq!(output.fill(), 173);
        assert_eq!(output.get(0, 0), 173);
        assert_eq!(output.get(2, 0), 91);
        assert_eq!(detail(plan.sample(2, 0).unwrap()), DVec2::new(1.5, 2.5));
        assert_eq!(
            plan.derive(&Mask::empty(2, 2, 173)).unwrap_err(),
            MaskSampleError::ProcessedMismatch
        );
    }

    #[test]
    fn safe_affine_equivalent_projective_maps_keep_bilinear_byte_order() {
        let affine = DAffine2::from_cols_array(&[2.0, 0.0, 0.0, 0.5, 0.25, -0.25]);
        let c = Projective2::from_affine(affine).unwrap();
        let inverse = affine.inverse();
        let plan = prepare_mask_grid(c, (5, 4), 0, grid((7, 5), (0, 0))).unwrap();
        for fill in [0, 255] {
            let mask = Mask::from_fn(5, 4, fill, |x, y| ((x * 47 + y * 71) % 256) as u8);
            let actual = plan.derive(&mask).unwrap();
            for y in 0..5 {
                for x in 0..7 {
                    let source = inverse
                        .transform_point2(DVec2::new(f64::from(x) + 0.5, f64::from(y) + 0.5));
                    assert_eq!(actual.get(x, y), sample_level_zero(&mask, source));
                }
            }
        }
    }

    #[test]
    fn certified_coordinates_are_not_narrowed_before_byte_sampling() {
        let translation = -29_998.0 - (1.0 / 512.0 + 1.0 / 65_536.0);
        let c = Projective2::from_affine(DAffine2::from_translation(DVec2::new(translation, 0.0)))
            .unwrap();
        let plan = prepare_mask_grid(c, (30_000, 1), 0, grid((1, 1), (0, 0))).unwrap();
        let mask = Mask::from_fn(30_000, 1, 0, |x, _| if x == 29_999 { 255 } else { 0 });
        let point = detail(plan.sample(0, 0).unwrap());
        assert_eq!(sample_level_zero(&mask, point), 1);
        assert_eq!(
            sample_level_zero(
                &mask,
                DVec2::new(f64::from(point.x as f32), f64::from(point.y as f32))
            ),
            0
        );
        assert_eq!(plan.derive(&mask).unwrap().get(0, 0), 1);
    }

    #[test]
    fn construction_error_is_not_reset_at_a_rounded_composed_inverse() {
        // Exact dyadic Fraction oracle: see tests/projective_mask_oracle.py.
        // Both C and rounded Q inverse pass representation/residual admission.
        // In either direction runtime-only bounds on Q^-1 prove the WRONG side.
        let cases = [
            (
                [
                    0x3e112e0d06b7e388,
                    0,
                    0x3ff0000000000000,
                    0,
                    0x3e012e0d06b7e395,
                    0,
                    0xbc812e0d06b7e395,
                    0,
                    0x3e012e0d06b7e395,
                ],
                2_000_000_011,
                false,
                DVec2::new(16.50000000060021, 0.4999995082616806),
            ),
            (
                [
                    0xbe112e0be64fd7b8,
                    0,
                    0x3ff0000000000000,
                    0,
                    0xbe012e0be64fd7c5,
                    0,
                    0x3c212e0be64fd7c5,
                    0,
                    0xbe012e0be64fd7c5,
                ],
                -2_000_000_011,
                true,
                DVec2::new(16.499999491484587, 0.499999992316589),
            ),
        ];
        for (bits, origin, authored_detail, oracle) in cases {
            let c = Projective2::from_row_major(bits.map(f64::from_bits)).unwrap();
            assert!(c.inverse().is_ok());
            let translation = Projective2::from_affine(DAffine2::from_translation(DVec2::new(
                -f64::from(origin),
                0.0,
            )))
            .unwrap();
            let rounded_inverse = translation.compose(c).unwrap().inverse().unwrap();
            let r = rounded_inverse.to_row_major();
            let q = DVec2::splat(0.5);
            let numerator = bounded_row(
                [r[0], r[1], r[2]].map(|value| {
                    let mut exact = Dot::ZERO;
                    exact.value = value;
                    exact
                }),
                q,
            )
            .unwrap();
            let denominator = bounded_row(
                [r[6], r[7], r[8]].map(|value| {
                    let mut exact = Dot::ZERO;
                    exact.value = value;
                    exact
                }),
                q,
            )
            .unwrap();
            let naive = Interval::from_dot(numerator)
                .divide(Interval::from_dot(denominator))
                .unwrap();
            if authored_detail {
                assert!(naive.lo > 16.5);
            } else {
                assert!(naive.hi < 16.5);
            }
            // Precision refusal is valid; a false exterior/detail result is not.
            if let Ok(plan) = prepare_mask_grid(c, (16, 4), 0, grid((1, 1), (origin, 0))) {
                match plan.sample(0, 0).unwrap() {
                    MaskGridSample::Exterior => assert!(!authored_detail),
                    MaskGridSample::Detail(point) => {
                        assert!(authored_detail);
                        // Oracle constants are rounded from exact rationals;
                        // one oracle ULP is far smaller than this strict budget.
                        assert!((point - oracle).abs().max_element() < 0.999_999e-6);
                    }
                }
            }
        }
    }

    #[test]
    fn near_zero_numerator_and_near_poles_do_not_create_in_support_holes() {
        for g in [0.125, -0.125] {
            let c =
                Projective2::from_row_major([1.0, 0.0, 0.5, 0.0, 1.0, 0.0, g, 0.0, 1.0]).unwrap();
            let plan = prepare_mask_grid(c, (4, 4), 0, grid((1, 1), (0, 0))).unwrap();
            let point = detail(plan.sample(0, 0).unwrap());
            assert_eq!(point.x, 0.0);
            assert!((point.y - 0.5).abs() < COORDINATE_ERROR);
        }
        for pole in [0.5_f64.next_down(), 0.5, 0.5_f64.next_up()] {
            let c = Projective2::from_row_major([pole, 0.0, 0.0, 0.0, 0.5, 0.0, 1.0, 0.0, -1.0])
                .unwrap();
            let plan = prepare_mask_grid(c, (16, 4), 0, grid((1, 1), (0, 0))).unwrap();
            assert_eq!(plan.sample(0, 0).unwrap(), MaskGridSample::Exterior);
        }
    }

    #[test]
    fn unsupported_construction_range_refuses_even_a_constant_fill_plane() {
        let tiny = 2.0_f64.powi(-600);
        let c =
            Projective2::from_row_major([1.0, 0.0, 0.0, 0.0, tiny, 0.0, 0.0, 0.0, tiny]).unwrap();
        assert!(c.inverse().is_ok());
        assert!(prepare_mask_grid(c, (4, 4), 0, grid((1, 1), (0, 0))).is_err());
        // Exact fallback must never call an underflowed FMA decomposition exact.
        assert!(Expansion::product(2.0_f64.powi(-600), 2.0_f64.powi(-600)).is_err());
        assert_eq!(
            Expansion::product(2.0_f64.powi(-537), 2.0_f64.powi(-537))
                .unwrap()
                .estimate()
                .to_bits(),
            1
        );
    }

    #[test]
    fn exact_expansion_checks_signed_zero_subnormal_and_overflow_boundaries() {
        let tiny = f64::from_bits(1);
        assert_eq!(Expansion::product(-0.0, f64::MAX).unwrap().sign(), 0);
        assert_eq!(Expansion::product(-tiny, 1.0).unwrap().estimate(), -tiny);
        assert!(Expansion::product(tiny, 0.5).is_err());
        assert!(Expansion::product(f64::MAX, 2.0).is_err());
        assert!(Expansion::product(0.0, f64::INFINITY).is_err());
        let max = Expansion::product(f64::MAX, 1.0).unwrap();
        assert!(max.add(max).is_err());
        let positive = Expansion::product(tiny, 1.0).unwrap();
        let negative = Expansion::product(-tiny, 1.0).unwrap();
        assert_eq!(positive.add(negative).unwrap().sign(), 0);
    }

    #[test]
    fn support_keys_and_integer_origins_are_complete_and_exact() {
        let base =
            prepare_mask_grid(Projective2::IDENTITY, (4, 4), 0, grid((1, 1), (0, 0))).unwrap();
        let halo =
            prepare_mask_grid(Projective2::IDENTITY, (4, 4), 1, grid((1, 1), (0, 0))).unwrap();
        assert_ne!(base.key(), halo.key());
        for origin in [i32::MIN, i32::MAX] {
            let plan = prepare_mask_grid(
                Projective2::IDENTITY,
                (4, 4),
                0,
                grid((1, 1), (origin, origin)),
            )
            .unwrap();
            assert_eq!(
                plan.authored_center(0, 0),
                DVec2::splat(f64::from(origin) + 0.5)
            );
            assert_eq!(plan.sample(0, 0).unwrap(), MaskGridSample::Exterior);
            assert_ne!(base.key(), plan.key());
        }
        assert_eq!(
            base.sample(1, 0).unwrap_err(),
            MaskSampleError::OutputCoordinate
        );
    }

    #[test]
    fn derivation_spans_tiles_and_initializes_partial_tile_padding_to_fill() {
        let (width, height) = (TILE + 3, TILE + 2);
        let fill = 173;
        let processed = Mask::from_fn(width, height, fill, |x, y| ((x + 3 * y) % 128) as u8);
        let plan = prepare_mask_grid(
            Projective2::IDENTITY,
            (width, height),
            0,
            grid((width, height), (0, 0)),
        )
        .unwrap();
        let output = plan.derive(&processed).unwrap();
        assert_eq!(output.tile_count(), 4);
        assert_eq!(output.fill(), fill);
        for y in 0..height {
            for x in 0..width {
                assert_eq!(output.get(x, y), processed.get(x, y));
            }
        }
        // Inspect actual stored tiles, including every padded row/column of
        // the three edge tiles, rather than merely out-of-bounds get behavior.
        for (coord, tile) in output.base_tiles() {
            for ly in 0..TILE {
                for lx in 0..TILE {
                    let x = coord.x as u32 * TILE + lx;
                    let y = coord.y as u32 * TILE + ly;
                    let expected = if x < width && y < height {
                        processed.get(x, y)
                    } else {
                        fill
                    };
                    assert_eq!(tile[(ly * TILE + lx) as usize], expected);
                }
            }
        }
    }

    #[test]
    fn cancelling_after_a_derived_tile_returns_no_plane_and_preserves_source() {
        let size = (TILE + 1, 2);
        let processed = Mask::from_fn(size.0, size.1, 173, |x, y| ((x + 3 * y) % 128) as u8);
        let snapshot = processed.clone();
        let plan = prepare_mask_grid(Projective2::IDENTITY, size, 0, grid(size, (0, 0))).unwrap();
        let mut checkpoints = 0;
        let result = plan.derive_with_control(&processed, || {
            checkpoints += 1;
            // This second checkpoint is reached only after the first tile
            // has been sampled and inserted into the still-private result.
            checkpoints < 2
        });
        assert_eq!(checkpoints, 2);
        assert!(matches!(result, Err(MaskSampleError::Cancelled)));
        assert_eq!(processed.content_id(), snapshot.content_id());
        assert_eq!(processed.fill(), snapshot.fill());
        assert_eq!(processed.tile_count(), snapshot.tile_count());
        for (coord, original) in snapshot.base_tiles() {
            let current = processed.base_tile(*coord).unwrap();
            assert!(std::sync::Arc::ptr_eq(original, current));
            assert_eq!(original.as_ref(), current.as_ref());
        }
        for y in 0..size.1 {
            for x in 0..size.0 {
                assert_eq!(processed.get(x, y), ((x + 3 * y) % 128) as u8);
            }
        }
    }

    #[test]
    fn cancellation_is_bounded_deterministic_and_never_publishes_partial_work() {
        let mut checkpoints = 0;
        let result = prepare_mask_grid_with_control(
            Projective2::IDENTITY,
            (TILE + 1, 1),
            0,
            grid((TILE + 1, 1), (0, 0)),
            || {
                checkpoints += 1;
                checkpoints < 2
            },
        );
        assert_eq!(checkpoints, 2);
        assert_eq!(result.unwrap_err(), MaskSampleError::Cancelled);
        // Maximum admitted grid is not replaced by a smaller hidden CPU cap.
        assert_eq!(
            prepare_mask_grid_with_control(
                Projective2::IDENTITY,
                (1, 1),
                0,
                grid((20_000, 20_000), (0, 0)),
                || false
            )
            .unwrap_err(),
            MaskSampleError::Cancelled
        );
        let plan =
            prepare_mask_grid(Projective2::IDENTITY, (1, 1), 0, grid((1, 1), (0, 0))).unwrap();
        assert_eq!(
            plan.derive_with_control(&Mask::empty(1, 1, 255), || false)
                .unwrap_err(),
            MaskSampleError::Cancelled
        );
        assert_eq!(
            prepare_mask_grid(Projective2::IDENTITY, (0, 1), 0, grid((1, 1), (0, 0))).unwrap_err(),
            MaskSampleError::Dimensions
        );
        assert_eq!(
            prepare_mask_grid(Projective2::IDENTITY, (30_000, 1), 1, grid((1, 1), (0, 0)))
                .unwrap_err(),
            MaskSampleError::ProcessedDimensions
        );
        assert_eq!(
            prepare_mask_grid(
                Projective2::IDENTITY,
                (1, 1),
                u32::MAX,
                grid((1, 1), (0, 0))
            )
            .unwrap_err(),
            MaskSampleError::ProcessedDimensions
        );
    }
}
