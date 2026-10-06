//! Retained-pixel projective sampling, shared by every blend profile.
//!
//! Per output tile: one TILE_PX sample grid, at most eight retained tile handles
//! per plane, and at most four tile lookups per output pixel per plane. Neither
//! an inverse source window nor a forward document AABB is ever allocated.
//! Plane::tile still owns its source-sized lazy mip pyramid: its recursive work
//! and retained storage are bounded by each admitted 30,000-side / 400-MP
//! source or independent mask, not by this small lookup cache. External masks
//! require CompositeTree::validate_projective_resources after raw mutations.

use crate::color;
use crate::composite::LazyRaster;
use crate::geom::{IRect, TileCoord};
use crate::image::{Mask, Pix, Plane, Raster, Tile};
use crate::projective::{Projective2, ProjectiveDifferential, ProjectiveError, ProjectiveRect};
use crate::tile::{FTile, TILE, TILE_PX};
use glam::{DAffine2, DVec2};
use std::sync::Arc;
use thiserror::Error;

#[derive(Clone, Copy, Debug, Error, PartialEq, Eq)]
pub enum ProjectivePixelError {
    #[error(transparent)]
    Geometry(#[from] ProjectiveError),
    #[error("projective pixels require positive source dimensions within 30,000 sides and 400 MP")]
    SourceDimensions,
    #[error(
        "projective scenes require every mask plane to have positive dimensions within 30,000 sides and 400 MP"
    )]
    MaskDimensions,
    #[error("projective mapping, lazy source and decoded raster dimensions must agree")]
    SourceMismatch,
}

/// Resource admission shared by checked sources and externally attached masks.
/// These are the existing document limits, not bounds on a projected world AABB.
pub(crate) fn valid_plane_size(width: u32, height: u32) -> bool {
    width > 0
        && height > 0
        && width <= 30_000
        && height <= 30_000
        && u64::from(width) * u64::from(height) <= 400_000_000
}

/// Checked cache-pixel-to-document geometry. Use NodeContent::projective_pixels
/// to validate the lazy and decoded source before publishing a candidate.
///
/// This initial renderer also admits the union of every available mip's
/// bilinear support. A forward pole in that expanded rectangle is explicitly
/// unsupported, even if the exact image rectangle alone would be finite.
/// Bounds include that support halo, and are never allocation dimensions.
#[derive(Clone, Copy, Debug, PartialEq)]
pub struct ProjectivePixelMapping {
    forward: Projective2,
    inverse: Projective2,
    size: (u32, u32),
    intrinsic: ProjectiveRect,
    bounds: IRect,
    anchor: DVec2,
    differential: ProjectiveDifferential,
}

impl ProjectivePixelMapping {
    pub fn new(
        width: u32,
        height: u32,
        forward: Projective2,
    ) -> Result<Self, ProjectivePixelError> {
        if !valid_plane_size(width, height) {
            return Err(ProjectivePixelError::SourceDimensions);
        }
        let intrinsic = ProjectiveRect::new(DVec2::ZERO, DVec2::new(width as f64, height as f64))?;
        forward.map_rect(intrinsic)?.bounds().to_irect()?;
        let max_level = 31 - width.max(height).leading_zeros();
        let step = (1_u32 << max_level) as f64;
        // At level L the nonzero bilinear support is (-.5, size(L)+.5)
        // in level coordinates. Rounded-up mip dimensions matter for odd sizes.
        let support = ProjectiveRect::new(
            DVec2::splat(-0.5 * step),
            DVec2::new((width as f64 / step).ceil(), (height as f64 / step).ceil()) * step
                + DVec2::splat(0.5 * step),
        )?;
        let projected = forward.map_rect(support)?;
        let bounds = projected.bounds().to_irect()?;
        let anchor = (projected.bounds().min() + projected.bounds().max()) * 0.5;
        let inverse = forward.inverse()?;
        // Rebase before inversion: rounding a globally translated inverse first
        // can discard useful source precision under large offsets/minification.
        let local_forward =
            Projective2::from_affine(DAffine2::from_translation(-anchor))?.compose(forward)?;
        let local_inverse = local_forward.inverse()?;
        // Independently certify against the original forward coefficients,
        // including rebasing/inversion/composition and runtime arithmetic.
        let differential = local_inverse.checked_differential(forward, anchor, support)?;
        Ok(Self {
            forward,
            inverse,
            size: (width, height),
            intrinsic,
            bounds,
            anchor,
            differential,
        })
    }

    pub fn forward(self) -> Projective2 {
        self.forward
    }
    pub fn inverse(self) -> Projective2 {
        self.inverse
    }
    pub fn size(self) -> (u32, u32) {
        self.size
    }
    pub fn intrinsic(self) -> ProjectiveRect {
        self.intrinsic
    }
    pub fn bounds(self) -> IRect {
        self.bounds
    }

    /// Resolve once without holding an initializer across Rayon work. This
    /// rejects incorrectly declared deferred dimensions before tree publication.
    pub fn validate_source(self, source: &LazyRaster) -> Result<(), ProjectivePixelError> {
        if source.size() != self.size {
            return Err(ProjectivePixelError::SourceMismatch);
        }
        let raster = source.get();
        if (raster.width(), raster.height()) != self.size {
            return Err(ProjectivePixelError::SourceMismatch);
        }
        Ok(())
    }
}

/// An immutable, checked source/mapping pairing. Private fields prevent a
/// mapping from being paired with a different-sized or incorrectly declared
/// source. External CompositeNode masks are not certified by this payload;
/// CompositeTree::validate_projective_resources admits those separately.
#[derive(Clone)]
pub struct ProjectivePixels {
    raster: LazyRaster,
    mapping: Arc<ProjectivePixelMapping>,
}
impl ProjectivePixels {
    pub fn new(raster: LazyRaster, forward: Projective2) -> Result<Self, ProjectivePixelError> {
        let (width, height) = raster.size();
        let mapping = ProjectivePixelMapping::new(width, height, forward)?;
        mapping.validate_source(&raster)?;
        Ok(Self {
            raster,
            mapping: Arc::new(mapping),
        })
    }
    pub fn raster(&self) -> &LazyRaster {
        &self.raster
    }
    pub fn mapping(&self) -> &ProjectivePixelMapping {
        &self.mapping
    }
}

#[derive(Clone, Copy)]
struct Sample {
    point: DVec2,
    log_footprint: f64,
}

/// Largest singular value, with scaled arithmetic to avoid squared overflow.
/// Row-major 2x2 Jacobian; its coefficients were bounded at admission.
fn largest_singular_value(j: [f64; 4]) -> f64 {
    let scale = j.into_iter().map(f64::abs).fold(0.0, f64::max);
    if scale == 0.0 {
        return 0.0;
    }
    let [a, b, c, d] = j.map(|v| v / scale);
    let x = a * a + c * c;
    let y = b * b + d * d;
    let z = a * b + c * d;
    ((x + y + (x - y).hypot(2.0 * z)) * 0.5).sqrt() * scale
}

pub(crate) struct ProjectiveTile {
    samples: Vec<Option<Sample>>,
}

impl ProjectiveTile {
    pub(crate) fn new(mapping: &ProjectivePixelMapping, scale: f64, ox: i64, oy: i64) -> Self {
        let mut samples = vec![None; TILE_PX];
        let bounds = mapping.bounds;
        let low = DVec2::new(bounds.x as f64, bounds.y as f64);
        let high = DVec2::new(bounds.right() as f64, bounds.bottom() as f64);
        for (index, sample) in samples.iter_mut().enumerate() {
            let point = DVec2::new(
                ox as f64 + (index % TILE as usize) as f64 + 0.5,
                oy as f64 + (index / TILE as usize) as f64 + 0.5,
            ) * scale;
            if point.cmplt(low).any() || point.cmpgt(high).any() {
                continue;
            }
            let Some((point, jacobian)) = mapping.differential.sample(point - mapping.anchor)
            else {
                continue;
            };
            // floor(log2(max(1, sigma_max(Jinverse) * document_pixel_step))),
            // capped to each plane's levels. Isotropic and deterministic: this
            // can blur anisotropic directions and is not Photoshop interpolation
            // equivalence. Legacy affine determinant-based mip choice is intact.
            // Add logarithms rather than overflow the footprint multiplication.
            let footprint = largest_singular_value(jacobian);
            let log_footprint = if footprint > 0.0 {
                (footprint.log2() + scale.log2()).max(0.0)
            } else {
                // Defensive deterministic fallback for an unexpected arithmetic
                // degeneration: the coarsest available mip, never a transparent
                // hole or an unexplained mip-zero sample. Admission proves a
                // positive differential, so valid descriptors do not need it.
                f64::INFINITY
            };
            *sample = Some(Sample {
                point,
                log_footprint,
            });
        }
        Self { samples }
    }

    pub(crate) fn raster(&self, raster: &Raster, dst: &mut FTile) -> bool {
        let mut cache = TileCache::new(raster);
        let mut touched = false;
        for (out, sample) in dst.iter_mut().zip(&self.samples) {
            let Some(sample) = sample else {
                continue;
            };
            let Some(grid) = sample.grid(raster) else {
                continue;
            };
            let values = cache.quad(grid, [0; 4]).map(color::px_to_f);
            *out = std::array::from_fn(|c| interpolate(values.map(|p| p[c]), grid.ax, grid.ay));
            touched = true;
        }
        touched
    }

    pub(crate) fn mask(&self, mask: &Mask) -> Vec<f32> {
        let fill = mask.fill();
        let mut out = vec![fill as f32 / 255.0; TILE_PX];
        let mut cache = TileCache::new(mask);
        for (out, sample) in out.iter_mut().zip(&self.samples) {
            let Some(sample) = sample else {
                continue;
            };
            let Some(grid) = sample.grid(mask) else {
                continue;
            };
            let values = cache.quad(grid, fill).map(f32::from);
            *out = interpolate(values, grid.ax, grid.ay) / 255.0;
        }
        out
    }

    /// Full intrinsic rectangle, including exact integer mip rounding at odd
    /// image edges. No source-sized solid Raster or tile map is allocated.
    pub(crate) fn shape(&self, raster: &Raster) -> Vec<f32> {
        self.samples
            .iter()
            .map(|sample| {
                let Some(grid) = sample.and_then(|s| s.grid(raster)) else {
                    return 0.0;
                };
                let (w, h) = raster.level_size(grid.level);
                let values = [(0, 0), (1, 0), (0, 1), (1, 1)].map(|(dx, dy)| {
                    let (x, y) = (grid.x + dx, grid.y + dy);
                    if x >= 0 && y >= 0 && x < i64::from(w) && y < i64::from(h) {
                        let step = 1_u32 << grid.level;
                        let nx = raster.width().saturating_sub(x as u32 * step).min(step);
                        let ny = raster.height().saturating_sub(y as u32 * step).min(step);
                        reduced_opaque(grid.level, nx, ny) as f32 / 65535.0
                    } else {
                        0.0
                    }
                });
                interpolate(values, grid.ax, grid.ay)
            })
            .collect()
    }
}

// Exactly the Plane::avg4 pyramid for a finite opaque rectangle with zero
// exterior. Full/empty quadrants stop immediately. At most one corner quadrant
// continues per level, with O(level^2) scalar work and O(level) stack, level<=14.
fn reduced_opaque(level: u32, width: u32, height: u32) -> u16 {
    if width == 0 || height == 0 {
        return 0;
    }
    let step = 1_u32 << level;
    if width == step && height == step {
        return u16::MAX;
    }
    let half = step / 2;
    let (x0, x1) = (width.min(half), width.saturating_sub(half));
    let (y0, y1) = (height.min(half), height.saturating_sub(half));
    if width == step {
        return ((u32::from(reduced_opaque(level - 1, half, y0))
            + u32::from(reduced_opaque(level - 1, half, y1))
            + 1)
            >> 1) as u16;
    }
    if height == step {
        return ((u32::from(reduced_opaque(level - 1, x0, half))
            + u32::from(reduced_opaque(level - 1, x1, half))
            + 1)
            >> 1) as u16;
    }
    let sum: u32 = [(x0, y0), (x1, y0), (x0, y1), (x1, y1)]
        .into_iter()
        .map(|(w, h)| u32::from(reduced_opaque(level - 1, w, h)))
        .sum();
    ((sum + 2) >> 2) as u16
}

impl Sample {
    fn grid<P: Pix>(self, plane: &Plane<P>) -> Option<Grid> {
        let level = self.log_footprint.floor().min(plane.max_level() as f64) as u32;
        let point = self.point / (1_u32 << level) as f64;
        let (w, h) = plane.level_size(level);
        // Check finite support BEFORE floor/cast/tile fetch. A bilinear center
        // just outside the mathematical image can still contribute an edge.
        if !point.is_finite()
            || point.x <= -0.5
            || point.y <= -0.5
            || point.x >= w as f64 + 0.5
            || point.y >= h as f64 + 0.5
        {
            return None;
        }
        let p = point - DVec2::splat(0.5);
        let floored = p.floor();
        Some(Grid {
            level,
            x: floored.x as i64,
            y: floored.y as i64,
            ax: (p.x - floored.x) as f32,
            ay: (p.y - floored.y) as f32,
        })
    }
}

#[derive(Clone, Copy)]
struct Grid {
    level: u32,
    x: i64,
    y: i64,
    ax: f32,
    ay: f32,
}

fn interpolate([a, b, c, d]: [f32; 4], x: f32, y: f32) -> f32 {
    let top = a + (b - a) * x;
    let bottom = c + (d - c) * x;
    top + (bottom - top) * y
}

const CACHE_CAPACITY: usize = 8;
struct CacheEntry<P: Pix> {
    level: u32,
    coord: TileCoord,
    tile: Option<Tile<P>>,
}
struct TileCache<'a, P: Pix> {
    plane: &'a Plane<P>,
    entries: [Option<CacheEntry<P>>; CACHE_CAPACITY],
    next: usize,
    #[cfg(test)]
    fetches: usize,
}
impl<'a, P: Pix> TileCache<'a, P> {
    fn new(plane: &'a Plane<P>) -> Self {
        Self {
            plane,
            entries: std::array::from_fn(|_| None),
            next: 0,
            #[cfg(test)]
            fetches: 0,
        }
    }
    fn get(&mut self, level: u32, x: i64, y: i64, outside: P) -> P {
        let (w, h) = self.plane.level_size(level);
        if x < 0 || y < 0 || x >= i64::from(w) || y >= i64::from(h) {
            return outside;
        }
        let coord = TileCoord::new((x / i64::from(TILE)) as i32, (y / i64::from(TILE)) as i32);
        let found = self.entries.iter().position(|entry| {
            entry
                .as_ref()
                .is_some_and(|e| e.level == level && e.coord == coord)
        });
        let index = found.unwrap_or_else(|| {
            let index = self.next;
            self.next = (self.next + 1) % CACHE_CAPACITY;
            self.entries[index] = Some(CacheEntry {
                level,
                coord,
                tile: self.plane.tile(level, coord),
            });
            #[cfg(test)]
            {
                self.fetches += 1;
            }
            index
        });
        self.entries[index]
            .as_ref()
            .unwrap()
            .tile
            .as_ref()
            .map_or(self.plane.fill(), |tile| {
                tile[((y % i64::from(TILE)) * i64::from(TILE) + x % i64::from(TILE)) as usize]
            })
    }
    fn quad(&mut self, g: Grid, outside: P) -> [P; 4] {
        [(0, 0), (1, 0), (0, 1), (1, 1)]
            .map(|(dx, dy)| self.get(g.level, g.x + dx, g.y + dy, outside))
    }
}

#[cfg(test)]
#[path = "projective_sample_tests.rs"]
mod tests;
