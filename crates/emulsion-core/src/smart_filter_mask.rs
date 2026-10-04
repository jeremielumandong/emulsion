//! One independently editable raster mask gates the entire Smart Filter stack.
//! The raw filter cache remains authoritative; derived pixels are disposable.
use crate::{MaskProperties, Node, NodeKind};
use emulsion_raster::{Mask, Raster};
use glam::{DAffine2, dvec2};
use std::sync::Arc;

#[derive(Clone, Debug)]
pub struct SmartFilterMask {
    pub pixels: Arc<Mask>,
    pub enabled: bool,
    pub linked: bool,
    /// Intrinsic mask pixels to Smart source pixels, never cache coordinates.
    pub transform: [f64; 6],
    pub properties: MaskProperties,
}
impl PartialEq for SmartFilterMask {
    fn eq(&self, other: &Self) -> bool {
        Arc::ptr_eq(&self.pixels, &other.pixels)
            && self.enabled == other.enabled
            && self.linked == other.linked
            && self.transform == other.transform
            && self.properties == other.properties
    }
}
impl SmartFilterMask {
    pub fn new(pixels: Arc<Mask>) -> Self {
        Self {
            pixels,
            enabled: true,
            linked: true,
            transform: crate::node::default_mask_transform(),
            properties: MaskProperties::default(),
        }
    }
    pub fn valid(&self) -> bool {
        valid_size(self.pixels.width(), self.pixels.height())
            && self.properties.valid()
            && crate::vector_mask::valid_transform(self.transform)
    }
}
fn valid_size(width: u32, height: u32) -> bool {
    width > 0
        && height > 0
        && width <= crate::document::MAX_SIDE
        && height <= crate::document::MAX_SIDE
        && u64::from(width) * u64::from(height) <= crate::document::MAX_PIXELS
}
pub fn descriptor(node: &Node) -> Option<&SmartFilterMask> {
    match &node.kind {
        NodeKind::Smart { filter_mask, .. } => filter_mask.as_ref(),
        _ => None,
    }
}
pub fn to_document(node: &Node) -> Option<DAffine2> {
    descriptor(node).map(|mask| {
        crate::transform::local_to_document(node) * DAffine2::from_cols_array(&mask.transform)
    })
}
pub(crate) fn preserve_world(node: &mut Node, world: Option<DAffine2>) {
    if to_document(node) == world {
        return;
    }
    let inverse = crate::transform::local_to_document(node).inverse();
    if let (
        NodeKind::Smart {
            filter_mask: Some(mask),
            ..
        },
        Some(world),
    ) = (&mut node.kind, world)
    {
        mask.transform = (inverse * world).to_cols_array();
    }
}
/// Inspect projected coverage even when the component is disabled or dormant.
pub fn for_inspection(node: &Node) -> Option<Arc<Mask>> {
    let NodeKind::Smart {
        filter_mask: Some(mask),
        cache,
        offset,
        ..
    } = &node.kind
    else {
        return None;
    };
    Some(crate::composite_mask_cache::derive_raster_mask(
        &mask.pixels,
        mask.properties,
        mask.transform,
        crate::composite_mask_cache::MaskGrid {
            width: cache.width(),
            height: cache.height(),
            offset: *offset,
        },
    ))
}
/// Canonical appearance consumed by compositor, effects, bounds and exports.
/// Black restores the finite original source; white reveals the complete F.
pub fn effective_pixels(node: &Node) -> Option<Arc<Raster>> {
    crate::smart_filter_mask_cache::effective_pixels(node)
}

/// Expand the raw plane to cover the current filter footprint without moving
/// or resampling existing mask pixels. Padding and paint must commit together.
/// Interactive growth is capped at 16 MP (native storage supports up to 400 MP).
/// This avoids an inverse tiny affine allocating a huge editable stroke plane.
pub fn pad_to_cache(node: &Node) -> Result<SmartFilterMask, &'static str> {
    let NodeKind::Smart {
        filter_mask: Some(mask),
        cache,
        offset,
        ..
    } = &node.kind
    else {
        return Err("Smart Filter mask is missing");
    };
    if !mask.valid() {
        return Err("invalid Smart Filter mask");
    }
    let inverse = crate::composite_mask_cache::mask_to_output(mask.transform, *offset).inverse();
    let mut lo = dvec2(0., 0.);
    let mut hi = dvec2(
        f64::from(mask.pixels.width()),
        f64::from(mask.pixels.height()),
    );
    for corner in [
        dvec2(0., 0.),
        dvec2(f64::from(cache.width()), 0.),
        dvec2(0., f64::from(cache.height())),
        dvec2(f64::from(cache.width()), f64::from(cache.height())),
    ] {
        let point = inverse.transform_point2(corner);
        if !point.is_finite() {
            return Err("Smart Filter mask editing extent is not finite");
        }
        lo = lo.min(point.floor());
        hi = hi.max(point.ceil());
    }
    if lo == dvec2(0., 0.)
        && hi
            == dvec2(
                f64::from(mask.pixels.width()),
                f64::from(mask.pixels.height()),
            )
    {
        return Ok(mask.clone());
    }
    let size = hi - lo;
    if !size.is_finite()
        || size.x < 1.
        || size.y < 1.
        || size.max_element() > f64::from(crate::document::MAX_SIDE)
        || size.x * size.y > 16_000_000.
    {
        return Err(
            "Smart Filter mask editing extent exceeds 30000 pixels per side or 16 MP; reduce mask scale or filter spread",
        );
    }
    let (x0, y0) = (lo.x as i64, lo.y as i64);
    let mut padded = mask.clone();
    padded.pixels = Arc::new(build_plane(
        size.x as u32,
        size.y as u32,
        mask.pixels.fill(),
        |x, y| {
            let (sx, sy) = (i64::from(x) + x0, i64::from(y) + y0);
            if sx >= 0
                && sy >= 0
                && sx < i64::from(mask.pixels.width())
                && sy < i64::from(mask.pixels.height())
            {
                mask.pixels.get(sx as u32, sy as u32)
            } else {
                mask.pixels.fill()
            }
        },
    ));
    padded.transform = (DAffine2::from_cols_array(&mask.transform)
        * DAffine2::from_translation(lo))
    .to_cols_array();
    if !padded.valid() {
        return Err("invalid padded Smart Filter mask");
    }
    Ok(padded)
}

/// Bounded materialization for selection-derived editable planes. Constant
/// reveal/hide planes should use `Mask::empty` and retain native storage limits.
pub fn sampled_edit_plane(
    width: u32,
    height: u32,
    fill: u8,
    sample: impl Fn(u32, u32) -> u8,
) -> Result<Mask, &'static str> {
    if !valid_size(width, height) || u64::from(width) * u64::from(height) > 16_000_000 {
        return Err(
            "Smart Filter mask sampled editing plane exceeds 30000 pixels per side or 16 MP",
        );
    }
    Ok(build_plane(width, height, fill, sample))
}

/// Materialize one tile at a time, including constant planes: no dense plane
/// temporary and no vector retaining tiles which equal the sparse fill.
pub(crate) fn build_plane<P: emulsion_raster::image::Pix>(
    width: u32,
    height: u32,
    fill: P,
    sample: impl Fn(u32, u32) -> P,
) -> emulsion_raster::image::Plane<P> {
    use emulsion_raster::{TILE, TILE_PX, TileCoord};
    let mut out = emulsion_raster::image::Plane::empty(width, height, fill);
    let (cols, rows) = out.tiles_at(0);
    for row in 0..rows {
        for col in 0..cols {
            let mut tile = vec![fill; TILE_PX];
            let (x0, y0) = (col as u32 * TILE, row as u32 * TILE);
            for y in 0..TILE.min(height - y0) {
                for x in 0..TILE.min(width - x0) {
                    tile[(y * TILE + x) as usize] = sample(x0 + x, y0 + y);
                }
            }
            if tile.iter().any(|p| *p != fill) {
                out.set_tile(TileCoord::new(col, row), tile);
            }
        }
    }
    out
}
