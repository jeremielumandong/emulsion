//! Derived layer masks are shared across geometry, style, and compositor queries.
//! Warm results are byte bounded; oversized live results use weak references.
use crate::geometry_error::GeometryError;
use crate::mapping::Mapping2;
use crate::smart_support::{MappingKey, SmartSupport, SmartSupportPreflight};
use crate::{Node, NodeKind};
use emulsion_raster::Mask;
use emulsion_raster::projective_mask_sample::{
    MaskGridKey, MaskGridPlan, MaskOutputGrid, MaskSampleError, prepare_mask_grid,
};
use parking_lot::Mutex;
use std::sync::{Arc, OnceLock, Weak};

#[derive(Clone, PartialEq, Eq)]
struct VectorKey {
    source: usize,
    matrix: [u64; 6],
    properties: [u32; 2],
    flags: u8,
}
#[derive(Clone, PartialEq, Eq)]
struct Key {
    source: usize,
    content_id: u64,
    raw_size: (u32, u32),
    width: u32,
    height: u32,
    offset: (i32, i32),
    matrix: MappingKey,
    properties: [u32; 2],
    intrinsic: bool,
    variant: u8,
    secondary: usize,
    flags: u8,
    vector: Option<VectorKey>,
}
enum Source {
    Mask(Weak<Mask>),
    Vector(Weak<emulsion_raster::vector::Path>),
    Components(Weak<Mask>, Weak<emulsion_raster::vector::Path>),
}
impl Source {
    fn alive(&self) -> bool {
        match self {
            Self::Mask(m) => m.strong_count() > 0,
            Self::Vector(p) => p.strong_count() > 0,
            Self::Components(a, b) => a.strong_count() > 0 && b.strong_count() > 0,
        }
    }
}
struct Entry {
    key: Key,
    source: Source,
    result: Weak<Mask>,
    warm: Option<Arc<Mask>>,
    bytes: usize,
}
struct Cache {
    entries: Vec<Entry>,
    budget: usize,
}
impl Cache {
    fn new(budget: usize) -> Self {
        Self {
            entries: vec![],
            budget,
        }
    }
    fn get(&mut self, key: &Key) -> Option<Arc<Mask>> {
        let index = self.entries.iter().position(|e| e.key == *key)?;
        let entry = self.entries.remove(index);
        if !entry.source.alive() {
            return None;
        }
        let result = entry.result.upgrade()?;
        self.entries.push(entry);
        Some(result)
    }
    fn insert(&mut self, key: Key, source: &Arc<Mask>, result: &Arc<Mask>) {
        self.insert_source(key, Source::Mask(Arc::downgrade(source)), result);
    }
    fn insert_source(&mut self, key: Key, source: Source, result: &Arc<Mask>) {
        self.entries
            .retain(|e| e.key != key && e.source.alive() && e.result.strong_count() > 0);
        // Every mip owns whole tiles, even for a one-pixel sparse mask.
        // Count the complete possible tiled pyramid rather than assuming a
        // dense-image 4/3 ratio (which undercounts sparse, tiny planes).
        let bytes = estimated_bytes(result);
        if bytes <= self.budget {
            let mut retained: usize = self
                .entries
                .iter()
                .filter(|e| e.warm.is_some())
                .map(|e| e.bytes)
                .sum();
            for entry in &mut self.entries {
                if retained.saturating_add(bytes) <= self.budget {
                    break;
                }
                if entry.warm.take().is_some() {
                    retained = retained.saturating_sub(entry.bytes);
                }
            }
        }
        self.entries.push(Entry {
            key,
            source,
            result: Arc::downgrade(result),
            warm: (bytes <= self.budget).then(|| result.clone()),
            bytes,
        });
        if self.entries.len() > 128 {
            self.entries.remove(0);
        }
    }
}
fn estimated_bytes(mask: &Mask) -> usize {
    (0..=mask.max_level())
        .map(|level| {
            let (x, y) = mask.tiles_at(level);
            (x as usize)
                .saturating_mul(y as usize)
                .saturating_mul(emulsion_raster::TILE_PX + 128)
        })
        .fold(256usize, usize::saturating_add)
}

fn cache() -> &'static Mutex<Cache> {
    static CACHE: OnceLock<Mutex<Cache>> = OnceLock::new();
    CACHE.get_or_init(|| Mutex::new(Cache::new(32 * 1024 * 1024)))
}

fn composite_key(node: &Node, size: (u32, u32)) -> Result<Option<Key>, GeometryError> {
    let Some(raster) = node.mask.as_ref().filter(|_| node.mask_enabled) else {
        return Ok(None);
    };
    let Some(vector) = node.vector_mask.as_ref().filter(|m| m.enabled) else {
        return Ok(None);
    };
    let (width, height, offset) = output_grid(node, size)?;
    Ok(Some(Key {
        source: Arc::as_ptr(raster) as usize,
        content_id: raster.content_id(),
        raw_size: (raster.width(), raster.height()),
        width,
        height,
        offset,
        matrix: node.mask_transform.into(),
        properties: [
            node.mask_properties.density.to_bits(),
            node.mask_properties.feather.to_bits(),
        ],
        intrinsic: false,
        variant: 2,
        secondary: 0,
        flags: 0,
        vector: Some(VectorKey {
            source: Arc::as_ptr(&vector.path) as usize,
            matrix: vector.transform.map(f64::to_bits),
            properties: [
                vector.properties.density.to_bits(),
                vector.properties.feather.to_bits(),
            ],
            flags: u8::from(vector.inverted) | ((vector.empty_coverage as u8) << 1),
        }),
    }))
}
pub(crate) fn composite_mask(
    node: &Node,
    size: (u32, u32),
) -> Result<Option<Arc<Mask>>, GeometryError> {
    // This also checks disabled and latent descriptors before a coverage hit;
    // H is deliberately absent from the reusable source-grid coverage key.
    let support = sampling_support(node)?;
    // The combined entry is keyed by authoritative sources, not disposable
    // component rasters: oversized components can disappear while the live
    // combined output must retain a stable identity and remain reusable.
    let key = composite_key(node, size)?;
    if let Some(key) = &key
        && let Some(result) = cache().lock().get(key)
    {
        return Ok(Some(result));
    }
    let raster = if node.mask_enabled {
        mask_for_inspection_with_support(node, size, support.as_ref())?
    } else {
        None
    };
    let vector = if node.vector_mask.as_ref().is_some_and(|m| m.enabled) {
        vector_mask_for_inspection(node, size)?
    } else {
        None
    };
    let result = match (raster, vector) {
        (None, None) => return Ok(None),
        (Some(mask), None) | (None, Some(mask)) => mask,
        (Some(a), Some(b)) => combine(&a, &b),
    };
    // A reveal-all vector component can return the original raster Arc.
    // Do not turn that identity fast path into warm ownership of source data.
    if node
        .mask
        .as_ref()
        .is_some_and(|raw| Arc::ptr_eq(raw, &result))
    {
        return Ok(Some(result));
    }
    if let Some(key) = key {
        let mut cache = cache().lock();
        if let Some(existing) = cache.get(&key) {
            return Ok(Some(existing));
        }
        cache.insert_source(
            key,
            Source::Components(
                Arc::downgrade(node.mask.as_ref().unwrap()),
                Arc::downgrade(&node.vector_mask.as_ref().unwrap().path),
            ),
            &result,
        );
    }
    Ok(Some(result))
}

/// A component's output plane. Offset is output pixel zero in layer source
/// coordinates, so the same projection can later serve Smart stack masks.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub(crate) struct MaskGrid {
    pub width: u32,
    pub height: u32,
    pub offset: (i32, i32),
}
/// Admit every retained projective descriptor before a visual shortcut. Pure
/// Legacy/Affine nodes keep their existing compatibility path. The central
/// support cache is keyed independently of the derived coverage cache.
pub(crate) fn sampling_support(node: &Node) -> Result<Option<SmartSupport>, GeometryError> {
    if !node.has_projective_metadata() {
        return Ok(None);
    }
    if !matches!(node.kind, NodeKind::Smart { .. }) {
        return Err(GeometryError::Unsupported {
            operation: "derive component mask",
            reason: "projective component mappings require a Smart owner",
        });
    }
    match crate::smart_support::validate_node(node)? {
        SmartSupportPreflight::LegacyCompatibility => Ok(None),
        SmartSupportPreflight::Projective(support) => Ok(Some(support)),
    }
}

pub(crate) fn mask_for_inspection(
    node: &Node,
    size: (u32, u32),
) -> Result<Option<Arc<Mask>>, GeometryError> {
    let support = sampling_support(node)?;
    mask_for_inspection_with_support(node, size, support.as_ref())
}

fn mask_for_inspection_with_support(
    node: &Node,
    size: (u32, u32),
    support: Option<&SmartSupport>,
) -> Result<Option<Arc<Mask>>, GeometryError> {
    let Some(mask) = node.mask.as_ref() else {
        return Ok(None);
    };
    let (width, height, offset) = output_grid(node, size)?;
    Ok(Some(derive_raster_mask_with_plan(
        mask,
        node.mask_properties,
        node.mask_transform,
        MaskGrid {
            width,
            height,
            offset,
        },
        support.and_then(|support| support.raster_mask().projective_plan()),
    )?))
}

/// Canonical intrinsic-to-output grid map shared by raster/vector components.
/// Compose the integer Smart-cache basis change before inversion, so a cache
/// rebased into Raster content uses identical floating arithmetic at byte ties.
pub(crate) fn mask_to_output(transform: [f64; 6], offset: (i32, i32)) -> glam::DAffine2 {
    let affine = glam::DAffine2::from_cols_array(&transform);
    if offset == (0, 0) {
        affine
    } else {
        glam::DAffine2::from_translation(glam::dvec2(-f64::from(offset.0), -f64::from(offset.1)))
            * affine
    }
}

/// Generalized basis change. The affine branch uses the exact legacy product;
/// projective composition stays checked and retains its representation.
pub(crate) fn mapping_to_output(
    transform: Mapping2,
    offset: (i32, i32),
) -> Result<Mapping2, GeometryError> {
    match transform {
        Mapping2::Affine(affine) => Ok(Mapping2::from_affine(mask_to_output(
            affine.to_cols_array(),
            offset,
        ))?),
        Mapping2::Projective(_) => Ok(transform.in_cache(offset)?),
    }
}

/// Test convenience for deriving without an existing support certificate.
/// Production callers pass the preflight plan through derive_raster_mask_with_plan.
/// Enabled state belongs to the caller; admission precedes uniform shortcuts.
#[cfg(test)]
pub(crate) fn derive_raster_mask(
    mask: &Arc<Mask>,
    properties: crate::MaskProperties,
    transform: Mapping2,
    output: MaskGrid,
) -> Result<Arc<Mask>, GeometryError> {
    derive_raster_mask_with_plan(mask, properties, transform, output, None)
}

pub(crate) fn derive_raster_mask_with_plan(
    mask: &Arc<Mask>,
    properties: crate::MaskProperties,
    transform: Mapping2,
    output: MaskGrid,
    prepared: Option<&MaskGridPlan>,
) -> Result<Arc<Mask>, GeometryError> {
    let identity =
        matches!(transform, Mapping2::Affine(affine) if affine == glam::DAffine2::IDENTITY);
    let (w, h, offset) = (output.width, output.height, output.offset);
    // Preserve both original affine Arc fast paths, including raw dimensions.
    if matches!(transform, Mapping2::Affine(_)) {
        if properties.is_default()
            && identity
            && offset == (0, 0)
            && (w, h) == (mask.width(), mask.height())
        {
            return Ok(mask.clone());
        }
        if mask.tile_count() == 0
            && properties.density == 1.0
            && (w, h) == (mask.width(), mask.height())
        {
            return Ok(mask.clone());
        }
    }
    let key = Key {
        source: Arc::as_ptr(mask) as usize,
        content_id: mask.content_id(),
        raw_size: (mask.width(), mask.height()),
        width: w,
        height: h,
        offset,
        matrix: transform.into(),
        properties: [properties.density.to_bits(), properties.feather.to_bits()],
        intrinsic: false,
        variant: 0,
        secondary: 0,
        flags: 0,
        vector: None,
    };
    if let Some(result) = cache().lock().get(&key) {
        return Ok(result);
    }
    // Certify against the authored C and the actual intrinsic processing halo
    // before allocating feather/detail planes, even for all-white/density-zero.
    let owned_plan;
    let plan = match transform {
        Mapping2::Affine(_) => None,
        Mapping2::Projective(projective) => {
            projective.inverse()?;
            let layout = properties
                .checked_processing_layout((mask.width(), mask.height()), mask.tile_count() > 0)
                .map_err(|_| GeometryError::Unsupported {
                    operation: "derive projective component mask",
                    reason: "invalid intrinsic mask processing dimensions or properties",
                })?;
            let grid = MaskOutputGrid {
                size: (w, h),
                offset,
            };
            let expected = MaskGridKey {
                authored_coefficients: projective.to_row_major().map(f64::to_bits),
                raw_size: (mask.width(), mask.height()),
                processed_origin: layout.origin,
                processed_size: layout.size,
                output: grid,
            };
            if let Some(prepared) = prepared {
                if prepared.key() != expected {
                    return Err(MaskSampleError::ProcessedMismatch.into());
                }
                Some(prepared)
            } else {
                owned_plan = prepare_mask_grid(
                    projective,
                    (mask.width(), mask.height()),
                    layout.halo,
                    grid,
                )?;
                Some(&owned_plan)
            }
        }
    };
    let (coverage, halo) = if !properties.is_default() && properties.density > 0.0 {
        processed_source(mask, properties)
    } else {
        (mask.clone(), 0)
    };
    let derived = if properties.density == 0.0 {
        Mask::empty(w, h, 255)
    } else if coverage.tile_count() == 0 {
        Mask::empty(w, h, coverage.fill())
    } else if let Some(plan) = plan {
        // The plan preserves f64 level-zero bilinear byte arithmetic and
        // classifies exterior before any integer conversion or tile lookup.
        plan.derive(&coverage)?
    } else if identity
        && offset == (0, 0)
        && halo == 0
        && (w, h) == (coverage.width(), coverage.height())
    {
        (*coverage).clone()
    } else {
        let Mapping2::Affine(affine) = transform else {
            unreachable!("projective detail always has a certified plan");
        };
        let inverse = mask_to_output(affine.to_cols_array(), offset).inverse();
        Mask::from_fn(w, h, coverage.fill(), |x, y| {
            let source = inverse.transform_point2(glam::dvec2(x as f64 + 0.5, y as f64 + 0.5))
                + glam::dvec2(halo as f64, halo as f64);
            crate::transform::sample_mask(&coverage, source)
        })
    };
    let result = Arc::new(derived);
    let mut cache = cache().lock();
    // Concurrent readers share the first completely published allocation.
    if let Some(existing) = cache.get(&key) {
        return Ok(existing);
    }
    cache.insert(key, mask, &result);
    Ok(result)
}

pub(crate) fn output_grid(
    node: &Node,
    size: (u32, u32),
) -> Result<(u32, u32, (i32, i32)), GeometryError> {
    Ok(match &node.kind {
        NodeKind::Raster { raster, .. } => (raster.width(), raster.height(), (0, 0)),
        NodeKind::Smart { .. } => {
            let grid = crate::smart_support::output_grid(node)?;
            (grid.size.0, grid.size.1, grid.offset)
        }
        NodeKind::Text { cache, .. }
        | NodeKind::Path { cache, .. }
        | NodeKind::Strokes { cache, .. } => {
            let (w, h) = cache.size();
            (w, h, (0, 0))
        }
        _ => (size.0, size.1, (0, 0)),
    })
}

pub(crate) fn vector_mask_for_inspection(
    node: &Node,
    size: (u32, u32),
) -> Result<Option<Arc<Mask>>, GeometryError> {
    let _ = sampling_support(node)?;
    let Some(mask) = node.vector_mask.as_ref() else {
        return Ok(None);
    };
    let (w, h, offset) = output_grid(node, size)?;
    let key = Key {
        source: Arc::as_ptr(&mask.path) as usize,
        content_id: 0,
        raw_size: (0, 0),
        width: w,
        height: h,
        offset,
        matrix: MappingKey::Affine(mask.transform.map(f64::to_bits)),
        properties: [
            mask.properties.density.to_bits(),
            mask.properties.feather.to_bits(),
        ],
        intrinsic: false,
        variant: 1,
        secondary: 0,
        flags: u8::from(mask.inverted) | ((mask.empty_coverage as u8) << 1),
        vector: None,
    };
    if let Some(result) = cache().lock().get(&key) {
        return Ok(Some(result));
    }
    let result = Arc::new(crate::vector_mask::render(mask, (w, h), offset));
    let mut cache = cache().lock();
    if let Some(existing) = cache.get(&key) {
        return Ok(Some(existing));
    }
    cache.insert_source(key, Source::Vector(Arc::downgrade(&mask.path)), &result);
    Ok(Some(result))
}

/// Independent mask coverage multiplies with nearest-byte rounding. Missing,
/// disabled and zero-density components are the multiplicative identity.
fn combine(a: &Arc<Mask>, b: &Arc<Mask>) -> Arc<Mask> {
    if a.tile_count() == 0 && a.fill() == 255 {
        return b.clone();
    }
    if b.tile_count() == 0 && b.fill() == 255 {
        return a.clone();
    }
    let product = |a: u8, b: u8| ((u16::from(a) * u16::from(b) + 127) / 255) as u8;
    Arc::new(Mask::from_fn(
        a.width(),
        a.height(),
        product(a.fill(), b.fill()),
        |x, y| product(a.get(x, y), b.get(x, y)),
    ))
}

// A separate cache entry shares intrinsic feather across mask/content moves.
// Store its full support halo, including coverage beyond the original bounds;
// otherwise an affine could not move that blurred coverage back into view.
fn processed_source(mask: &Arc<Mask>, properties: crate::MaskProperties) -> (Arc<Mask>, i32) {
    let radius = properties.feather;
    let halo = properties.processing_halo(mask.tile_count() > 0) as i32;
    let width = mask.width() + 2 * halo as u32;
    let height = mask.height() + 2 * halo as u32;
    let key = Key {
        source: Arc::as_ptr(mask) as usize,
        content_id: mask.content_id(),
        raw_size: (mask.width(), mask.height()),
        width,
        height,
        offset: (-halo, -halo),
        matrix: Mapping2::IDENTITY.into(),
        properties: [properties.density.to_bits(), radius.to_bits()],
        intrinsic: true,
        variant: 0,
        secondary: 0,
        flags: 0,
        vector: None,
    };
    if let Some(result) = cache().lock().get(&key) {
        return (result, halo);
    }
    let feathered = if halo == 0 {
        (**mask).clone()
    } else {
        let detail = mask.tile_bounds().intersect(&mask.bounds());
        let region = emulsion_raster::IRect::new(
            detail.x,
            detail.y,
            detail.w + 2 * halo,
            detail.h + 2 * halo,
        );
        emulsion_raster::select::feather_sampled_region(
            width,
            height,
            mask.fill(),
            region,
            radius,
            |x, y| {
                let (x, y) = (x - halo, y - halo);
                if x < 0 || y < 0 || x >= mask.width() as i32 || y >= mask.height() as i32 {
                    mask.fill()
                } else {
                    mask.get(x as u32, y as u32)
                }
            },
        )
    };
    // Density is linear and commutes with affine sampling. Quantize intrinsic
    // coverage once, so linked and unlinked node kinds share the same plane.
    let result = Arc::new(
        crate::MaskProperties {
            feather: 0.0,
            ..properties
        }
        .apply(&feathered),
    );
    let mut cache = cache().lock();
    if let Some(existing) = cache.get(&key) {
        return (existing, halo);
    }
    cache.insert(key, mask, &result);
    (result, halo)
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::Document;
    use emulsion_raster::{Placement, Raster};
    fn node() -> Node {
        let mut n = Node::raster(
            1,
            "image",
            Arc::new(Raster::empty(32, 24, [0; 4])),
            Placement::default(),
        );
        n.mask = Some(Arc::new(Mask::from_fn(32, 24, 255, |x, y| {
            if x < 8 && y < 8 { 0 } else { 255 }
        })));
        n.mask_transform = Mapping2::Affine(glam::DAffine2::from_translation(glam::dvec2(4., 0.)));
        n
    }
    #[test]
    fn repeated_queries_share_derived_mask_and_changes_invalidate() {
        let mut n = node();
        let a = Document::new(32, 24).composite_mask(&n).unwrap().unwrap();
        let b = Document::new(32, 24).composite_mask(&n).unwrap().unwrap();
        assert!(Arc::ptr_eq(&a, &b));
        assert_eq!(a.get(5, 2), 0);
        assert_eq!(a.get(1, 2), 255);
        n.mask_transform = Mapping2::Affine(glam::DAffine2::from_translation(glam::dvec2(8., 0.)));
        let c = Document::new(32, 24).composite_mask(&n).unwrap().unwrap();
        assert!(!Arc::ptr_eq(&a, &c));
        assert_eq!(c.get(5, 2), 255);
        n.mask = Some(Arc::new(Mask::from_fn(32, 24, 255, |x, _| {
            if x < 3 { 0 } else { 255 }
        })));
        let d = Document::new(32, 24).composite_mask(&n).unwrap().unwrap();
        assert!(!Arc::ptr_eq(&c, &d));
        if let NodeKind::Raster { raster, .. } = &mut n.kind {
            *raster = Arc::new(Raster::empty(48, 24, [0; 4]));
        }
        let e = Document::new(32, 24).composite_mask(&n).unwrap().unwrap();
        assert_eq!(e.width(), 48);
        assert!(!Arc::ptr_eq(&d, &e));
        n.mask_enabled = false;
        assert!(Document::new(32, 24).composite_mask(&n).unwrap().is_none());
    }
    #[test]
    fn identity_and_uniform_transforms_preserve_source_and_outside_fill() {
        let mut n = node();
        n.mask_transform = Mapping2::IDENTITY;
        assert!(Arc::ptr_eq(
            n.mask.as_ref().unwrap(),
            &Document::new(32, 24).composite_mask(&n).unwrap().unwrap()
        ));
        for fill in [0, 127, 255] {
            n.mask = Some(Arc::new(Mask::empty(32, 24, fill)));
            n.mask_transform =
                Mapping2::from_affine_columns([0.5, 0., 0., 0.5, 900., -70.]).unwrap();
            let a = Document::new(32, 24).composite_mask(&n).unwrap().unwrap();
            assert!(Arc::ptr_eq(n.mask.as_ref().unwrap(), &a));
            if let NodeKind::Raster { raster, .. } = &mut n.kind {
                *raster = Arc::new(Raster::empty(48, 24, [0; 4]));
            }
            let b = Document::new(32, 24).composite_mask(&n).unwrap().unwrap();
            assert_eq!(
                (b.width(), b.height(), b.tile_count(), b.fill()),
                (48, 24, 0, fill)
            );
            assert!(Arc::ptr_eq(
                &b,
                &Document::new(32, 24).composite_mask(&n).unwrap().unwrap()
            ));
            if let NodeKind::Raster { raster, .. } = &mut n.kind {
                *raster = Arc::new(Raster::empty(32, 24, [0; 4]));
            }
        }
    }
    fn key(source: &Arc<Mask>, id: u64) -> Key {
        Key {
            source: Arc::as_ptr(source) as usize,
            content_id: source.content_id(),
            raw_size: (source.width(), source.height()),
            width: 32,
            height: 24,
            offset: (0, 0),
            matrix: MappingKey::Affine([id; 6]),
            properties: [1.0f32.to_bits(), 0.0f32.to_bits()],
            intrinsic: false,
            variant: 0,
            secondary: 0,
            flags: 0,
            vector: None,
        }
    }
    #[test]
    fn weak_cache_reuses_live_large_masks_and_does_not_pin_source_or_result() {
        let source = Arc::new(Mask::empty(32, 24, 255));
        let weak_source = Arc::downgrade(&source);
        let derived = Arc::new(Mask::from_fn(32, 24, 0, |_, _| 127));
        let weak = Arc::downgrade(&derived);
        let key = key(&source, 0);
        let mut cache = Cache::new(0);
        cache.insert(key.clone(), &source, &derived);
        assert!(Arc::ptr_eq(&derived, &cache.get(&key).unwrap()));
        drop(derived);
        assert!(weak.upgrade().is_none());
        assert!(cache.get(&key).is_none());
        drop(source);
        assert!(weak_source.upgrade().is_none());
    }
    #[test]
    fn warm_cache_is_byte_bounded_and_never_retains_original_mask() {
        let source = Arc::new(Mask::empty(32, 24, 255));
        let weak = Arc::downgrade(&source);
        let mut cache = Cache::new(1_000_000);
        for id in 0..20 {
            let derived = Arc::new(Mask::from_fn(32, 24, 0, |_, _| 127));
            cache.insert(key(&source, id), &source, &derived);
            let bytes: usize = cache
                .entries
                .iter()
                .filter(|e| e.warm.is_some())
                .map(|e| e.bytes)
                .sum();
            assert!(bytes <= cache.budget);
        }
        drop(source);
        assert!(weak.upgrade().is_none());
        let old = cache.entries.last().unwrap().key.clone();
        assert!(cache.get(&old).is_none());
    }
    #[test]
    fn smart_offset_dimensions_and_transform_are_cached() {
        let mut d = Document::new(32, 24);
        let mut n = node();
        n.mask_transform = Mapping2::IDENTITY;
        d.nodes.push(n);
        crate::Command::ConvertToSmart { id: 1 }
            .apply(&mut d)
            .unwrap();
        if let NodeKind::Smart {
            cache,
            offset,
            filters,
            ..
        } = &mut d.nodes[0].kind
        {
            // This fixture exercises an active cache; bypass uses source/zero.
            filters.push(emulsion_filters::Filter::Invert);
            *cache = Arc::new(Raster::empty(40, 24, [0; 4]));
            *offset = (-4, 0);
        }
        let a = d.composite_mask(&d.nodes[0]).unwrap().unwrap();
        assert_eq!((a.width(), a.get(1, 2), a.get(5, 2)), (40, 255, 0));
        assert!(Arc::ptr_eq(
            &a,
            &d.composite_mask(&d.nodes[0]).unwrap().unwrap()
        ));
        if let NodeKind::Smart { offset, .. } = &mut d.nodes[0].kind {
            *offset = (4, 0);
        }
        let b = d.composite_mask(&d.nodes[0]).unwrap().unwrap();
        assert!(!Arc::ptr_eq(&a, &b));
        assert_eq!(b.get(1, 2), 0);
    }
    #[test]
    fn vector_and_pair_cache_entries_share_budget_without_pinning_geometry() {
        let source = Arc::new(Mask::empty(32, 24, 0));
        let path = Arc::new(emulsion_raster::vector::Path::default());
        let weak_path = Arc::downgrade(&path);
        let derived = Arc::new(Mask::from_fn(32, 24, 0, |_, _| 127));
        let mut k = key(&source, 0);
        k.variant = 1;
        k.source = Arc::as_ptr(&path) as usize;
        let mut cache = Cache::new(estimated_bytes(&derived));
        cache.insert_source(k.clone(), Source::Vector(Arc::downgrade(&path)), &derived);
        assert!(Arc::ptr_eq(&derived, &cache.get(&k).unwrap()));
        for id in 1..10 {
            cache.insert(key(&source, id), &source, &derived);
            assert!(
                cache
                    .entries
                    .iter()
                    .filter(|e| e.warm.is_some())
                    .map(|e| e.bytes)
                    .sum::<usize>()
                    <= cache.budget
            );
        }
        drop(path);
        assert!(weak_path.upgrade().is_none());
        assert!(cache.get(&k).is_none());
        let a = Arc::new(Mask::empty(32, 24, 80));
        let b = Arc::new(Mask::empty(32, 24, 120));
        let combined = combine(&a, &b);
        assert_eq!(combined.fill(), 38);
        let mut n = node();
        n.vector_mask = Some(crate::VectorMask::default());
        let combined_key = composite_key(&n, (32, 24)).unwrap().unwrap();
        let mut weak_only = Cache::new(0);
        weak_only.insert_source(
            combined_key.clone(),
            Source::Components(
                Arc::downgrade(n.mask.as_ref().unwrap()),
                Arc::downgrade(&n.vector_mask.as_ref().unwrap().path),
            ),
            &combined,
        );
        drop(a);
        drop(b); // disposable component planes are not retained
        assert!(Arc::ptr_eq(
            &combined,
            &weak_only.get(&combined_key).unwrap()
        ));
        n.vector_mask = None;
        assert!(weak_only.get(&combined_key).is_none());
    }

    #[test]
    fn reveal_all_vector_combination_never_pins_original_raster_source() {
        let mut n = node();
        n.mask_transform = Mapping2::IDENTITY;
        n.vector_mask = Some(crate::VectorMask::default());
        let weak = Arc::downgrade(n.mask.as_ref().unwrap());
        let result = composite_mask(&n, (32, 24)).unwrap().unwrap();
        assert!(Arc::ptr_eq(&result, n.mask.as_ref().unwrap()));
        drop(result);
        drop(n);
        assert!(weak.upgrade().is_none());
    }

    #[test]
    fn smart_rasterize_canonical_grid_preserves_exact_coverage_and_appearance() {
        for properties in [
            crate::MaskProperties::default(),
            crate::MaskProperties {
                density: 0.45,
                feather: 2.0,
            },
        ] {
            for mask_transform in [
                crate::node::default_mask_transform(),
                [1.0, 0.0, 0.1, 1.0, 1.25, -0.5],
                glam::DAffine2::from_scale_angle_translation(
                    glam::dvec2(1.2, 0.8),
                    0.23,
                    glam::dvec2(2.25, -1.75),
                )
                .to_cols_array(),
            ] {
                for placement in [
                    Placement::at(6.0, 5.0),
                    Placement {
                        x: 6.0,
                        y: 5.0,
                        scale_x: 1.3,
                        scale_y: 0.9,
                        rotation: 17.0,
                        ..Default::default()
                    },
                ] {
                    for enabled in [false, true] {
                        let mut doc = Document::new(40, 30);
                        let mut node = Node::smart(
                            1,
                            "Expanded Smart cache",
                            Arc::new(Raster::solid(12, 8, [1.0, 0.0, 0.0, 1.0])),
                            vec![emulsion_filters::Filter::GaussianBlur { radius: 1.5 }],
                            placement,
                        );
                        node.mask = Some(Arc::new(Mask::from_fn(12, 8, 0, |x, y| {
                            ((x * 17 + y * 23) % 256) as u8
                        })));
                        node.mask_transform =
                            Mapping2::from_affine_columns(mask_transform).unwrap();
                        node.mask_properties = properties;
                        node.mask_enabled = enabled;
                        let NodeKind::Smart { cache, offset, .. } = &node.kind else {
                            unreachable!()
                        };
                        let size = (cache.width(), cache.height());
                        let offset = *offset;
                        assert_ne!(offset, (0, 0));
                        let raw = node.mask.clone().unwrap();
                        let world = crate::transform::mask_to_document(&node)
                            .unwrap()
                            .require_affine("legacy fixture")
                            .unwrap();
                        let before_mask = doc.mask_for_inspection(&node).unwrap().unwrap();
                        doc.nodes.push(node);
                        doc.next_id = 2;
                        doc.validate().unwrap();
                        let before = emulsion_raster::composite::flatten(&doc.composite_tree(), 0)
                            .to_srgba8();
                        crate::Command::Rasterize { id: 1 }.apply(&mut doc).unwrap();
                        let node = doc.node(1).unwrap();
                        let after_mask = doc.mask_for_inspection(node).unwrap().unwrap();
                        assert!(Arc::ptr_eq(node.mask.as_ref().unwrap(), &raw));
                        assert_eq!(node.mask_properties, properties);
                        assert_eq!(node.mask_enabled, enabled);
                        assert_eq!((raw.width(), raw.height()), (12, 8));
                        assert_eq!((after_mask.width(), after_mask.height()), size);
                        assert_eq!(before_mask.to_gray8(), after_mask.to_gray8());
                        assert_eq!(before_mask.fill(), after_mask.fill());
                        assert_eq!(
                            node.mask_transform
                                .require_affine("legacy fixture")
                                .unwrap()
                                .to_cols_array(),
                            (glam::DAffine2::from_translation(glam::dvec2(
                                -f64::from(offset.0),
                                -f64::from(offset.1)
                            )) * glam::DAffine2::from_cols_array(&mask_transform))
                            .to_cols_array()
                        );
                        for (actual, expected) in crate::transform::mask_to_document(node)
                            .unwrap()
                            .require_affine("legacy fixture")
                            .unwrap()
                            .to_cols_array()
                            .into_iter()
                            .zip(world.to_cols_array())
                        {
                            assert!((actual - expected).abs() < 1e-9);
                        }
                        assert_eq!(
                            emulsion_raster::composite::flatten(&doc.composite_tree(), 0)
                                .to_srgba8(),
                            before
                        );
                    }
                }
            }
        }
    }

    #[test]
    fn budget_covers_materialized_sparse_mip_tiles() {
        let source = Arc::new(Mask::empty(512, 512, 255));
        let derived = Arc::new(Mask::from_fn(512, 512, 0, |x, y| {
            if x == 0 && y == 0 { 255 } else { 0 }
        }));
        let estimated = estimated_bytes(&derived);
        let mut cache = Cache::new(estimated);
        cache.insert(key(&source, 0), &source, &derived);
        for level in 1..=derived.max_level() {
            let (w, h) = derived.tiles_at(level);
            for y in 0..h {
                for x in 0..w {
                    let _ = derived.tile(level, emulsion_raster::TileCoord::new(x, y));
                }
            }
        }
        let actual: usize = derived
            .buffer_allocations()
            .iter()
            .map(|(_, bytes)| bytes)
            .sum();
        assert!(
            actual <= estimated,
            "sparse mips {actual} exceed budget estimate {estimated}"
        );
        assert!(
            actual > derived.tile_count() * emulsion_raster::TILE_PX * 2,
            "regression exceeds the former incorrect two-times-base bound"
        );
        assert_eq!(cache.entries.iter().filter(|e| e.warm.is_some()).count(), 1);
    }

    // Authored source-only integration regressions; not executed in this slice.
    #[test]
    fn projective_masks_use_actual_intrinsic_feather_density_and_legacy_bilinear_bytes() {
        use emulsion_raster::projective::Projective2;
        let affine = glam::DAffine2::from_cols_array(&[2., 0., 0., 0.5, 0.25, -0.25]);
        let projective = Mapping2::Projective(Projective2::from_affine(affine).unwrap());
        let output = MaskGrid {
            width: 13,
            height: 9,
            offset: (-3, -2),
        };
        let properties = crate::MaskProperties {
            density: 0.6,
            feather: 2.0,
        };
        for fill in [0, 255] {
            let raw = Arc::new(Mask::from_fn(5, 4, fill, |x, y| {
                ((x * 47 + y * 71) % 256) as u8
            }));
            let raw_id = raw.content_id();
            let (processed, halo) = processed_source(&raw, properties);
            assert!(halo > 0);
            let derived = derive_raster_mask(&raw, properties, projective, output).unwrap();
            let legacy =
                derive_raster_mask(&raw, properties, Mapping2::Affine(affine), output).unwrap();
            let inverse = mask_to_output(affine.to_cols_array(), output.offset).inverse();
            for y in 0..output.height {
                for x in 0..output.width {
                    let point = inverse
                        .transform_point2(glam::dvec2(f64::from(x) + 0.5, f64::from(y) + 0.5))
                        + glam::dvec2(f64::from(halo), f64::from(halo));
                    let expected = crate::transform::sample_mask(&processed, point);
                    assert_eq!(derived.get(x, y), expected);
                    assert_eq!(legacy.get(x, y), expected);
                }
            }
            assert_eq!(derived.fill(), processed.fill());
            assert_eq!(raw.content_id(), raw_id);
            assert!(!Arc::ptr_eq(&derived, &legacy));
            assert!(Arc::ptr_eq(
                &derived,
                &derive_raster_mask(&raw, properties, projective, output).unwrap()
            ));
            // Outer placement and relative mapping are absent from this cache.
            assert!(Arc::ptr_eq(
                &processed,
                &processed_source(&raw, properties).0
            ));
        }
    }

    #[test]
    fn relative_horizon_cancellation_keeps_raw_off_grid_detail_and_mixed_states() {
        use crate::mapping::SmartPlacement;
        use emulsion_raster::projective::Projective2;
        let h = Projective2::from_row_major([1., 0., 0., 0., 1., 0., 0.125, 0., 1.]).unwrap();
        let c = h.inverse().unwrap();
        assert!(
            Mapping2::Projective(c)
                .map_rect(crate::mapping::source_rect((16, 4)).unwrap())
                .is_err()
        );
        for placement in [
            SmartPlacement::Legacy(Placement::default()),
            SmartPlacement::Projective(h),
        ] {
            for fill in [0, 255] {
                let raw = Arc::new(Mask::from_fn(16, 4, fill, |x, y| {
                    ((x * 17 + y * 31) % 256) as u8
                }));
                let mut node = Node::smart(
                    1,
                    "Cancellation",
                    Arc::new(Raster::transparent(4, 4)),
                    vec![],
                    Placement::default(),
                );
                node.mask = Some(raw.clone());
                node.mask_transform = Mapping2::Projective(c);
                if let NodeKind::Smart {
                    placement: current,
                    filter_mask,
                    ..
                } = &mut node.kind
                {
                    *current = placement;
                    let mut descriptor = crate::SmartFilterMask::new(raw.clone());
                    descriptor.transform = Mapping2::Projective(c);
                    descriptor.enabled = false;
                    *filter_mask = Some(descriptor);
                }
                let coverage = mask_for_inspection(&node, (4, 4)).unwrap().unwrap();
                let filter = crate::smart_filter_mask::for_inspection(&node)
                    .unwrap()
                    .unwrap();
                assert!(Arc::ptr_eq(&coverage, &filter));
                for y in 0..4 {
                    for x in 0..4 {
                        let q = glam::dvec2(f64::from(x) + 0.5, f64::from(y) + 0.5);
                        // Independent algebraic inverse C^-1(q) = q/(1+q.x/8).
                        assert_eq!(
                            coverage.get(x, y),
                            crate::transform::sample_mask(&raw, q / (1. + q.x / 8.))
                        );
                    }
                }
                assert!(Arc::ptr_eq(node.mask.as_ref().unwrap(), &raw));
                assert_eq!(raw.get(15, 3), ((15 * 17 + 3 * 31) % 256) as u8);
                assert_eq!(node.mask_transform, Mapping2::Projective(c));
            }
        }
    }

    #[test]
    fn inverse_poles_preserve_processed_exterior_fill() {
        use emulsion_raster::projective::Projective2;
        let c = Projective2::from_row_major([0.5, 0., 0., 0., 0.5, 0., 1., 0., -1.]).unwrap();
        let raw = Arc::new(Mask::from_fn(4, 4, 255, |_, _| 0));
        let properties = crate::MaskProperties {
            density: 0.5,
            feather: 0.0,
        };
        let coverage = derive_raster_mask(
            &raw,
            properties,
            Mapping2::Projective(c),
            MaskGrid {
                width: 4,
                height: 3,
                offset: (-1, -1),
            },
        )
        .unwrap();
        assert_eq!(coverage.fill(), 255);
        assert_eq!(coverage.get(0, 0), 128);
        assert_eq!(coverage.get(2, 1), 128);
        for y in 0..3 {
            assert_eq!(coverage.get(1, y), 255);
        }
    }

    #[test]
    fn unsafe_uniform_and_density_zero_projective_descriptors_do_not_bypass_certification() {
        use emulsion_raster::projective::Projective2;
        let tiny = 2.0_f64.powi(-600);
        let c = Projective2::from_row_major([1., 0., 0., 0., tiny, 0., 0., 0., tiny]).unwrap();
        let raw = Arc::new(Mask::empty(4, 4, 255));
        for density in [0., 1.] {
            let result = derive_raster_mask(
                &raw,
                crate::MaskProperties {
                    density,
                    feather: 0.,
                },
                Mapping2::Projective(c),
                MaskGrid {
                    width: 4,
                    height: 4,
                    offset: (0, 0),
                },
            );
            assert!(result.is_err());
        }
    }

    #[test]
    fn changed_owner_support_is_checked_before_reusing_coverage() {
        use crate::mapping::SmartPlacement;
        use emulsion_raster::projective::Projective2;
        let mut node = Node::smart(
            1,
            "Projected coverage",
            Arc::new(Raster::transparent(4, 4)),
            vec![],
            Placement::default(),
        );
        node.mask = Some(Arc::new(Mask::from_fn(4, 4, 255, |x, y| {
            if x == y { 0 } else { 255 }
        })));
        node.mask_transform = Mapping2::Projective(Projective2::IDENTITY);
        let first = mask_for_inspection(&node, (4, 4)).unwrap().unwrap();
        assert!(Arc::ptr_eq(
            &first,
            &mask_for_inspection(&node, (4, 4)).unwrap().unwrap()
        ));
        if let NodeKind::Smart { placement, .. } = &mut node.kind {
            *placement = SmartPlacement::Projective(
                Projective2::from_row_major([1., 0., 0., 0., 1., 0., -0.5, 0., 1.]).unwrap(),
            );
        }
        assert!(mask_for_inspection(&node, (4, 4)).is_err());
        assert!(composite_mask(&node, (4, 4)).is_err());
        if let NodeKind::Smart { placement, .. } = &mut node.kind {
            *placement = SmartPlacement::Legacy(Placement::default());
        }
        node.vector_mask = Some(crate::VectorMask {
            enabled: false,
            ..Default::default()
        });
        assert!(mask_for_inspection(&node, (4, 4)).is_err());
        assert!(vector_mask_for_inspection(&node, (4, 4)).is_err());
    }

    #[test]
    fn latent_projective_map_has_no_plane_but_still_enforces_owner_and_vector_rules() {
        use emulsion_raster::projective::Projective2;
        let mut node = Node::smart(
            1,
            "Latent",
            Arc::new(Raster::transparent(4, 4)),
            vec![],
            Placement::default(),
        );
        node.mask_transform = Mapping2::Projective(Projective2::IDENTITY);
        assert!(mask_for_inspection(&node, (4, 4)).unwrap().is_none());
        assert!(composite_mask(&node, (4, 4)).unwrap().is_none());
        node.vector_mask = Some(crate::VectorMask::default());
        assert!(composite_mask(&node, (4, 4)).is_err());
        node.vector_mask = None;
        node.kind = NodeKind::Raster {
            raster: Arc::new(Raster::transparent(4, 4)),
            placement: Placement::default(),
        };
        assert!(mask_for_inspection(&node, (4, 4)).is_err());
    }
}
