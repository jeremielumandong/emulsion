//! Byte-bounded effective Smart raster cache. Mask edits never render filters.
use crate::geometry_error::GeometryError;
use crate::smart_support::MappingKey;
use crate::{Node, NodeKind};
use emulsion_raster::blend::BlendSpace;
use emulsion_raster::{Mask, Raster, color};
use parking_lot::Mutex;
use std::sync::{Arc, OnceLock, Weak};
#[derive(Clone, PartialEq, Eq)]
struct Key {
    space: BlendSpace,
    source: usize,
    source_id: u64,
    filtered: usize,
    filtered_id: u64,
    mask: usize,
    mask_id: u64,
    offset: (i32, i32),
    dimensions: [u32; 6],
    matrix: MappingKey,
    properties: [u32; 2],
}
struct Entry {
    key: Key,
    source: Weak<Raster>,
    filtered: Weak<Raster>,
    mask: Weak<Mask>,
    result: Weak<Raster>,
    warm: Option<Arc<Raster>>,
    bytes: usize,
}
impl Entry {
    fn alive(&self) -> bool {
        self.source.strong_count() > 0
            && self.filtered.strong_count() > 0
            && self.mask.strong_count() > 0
    }
}
#[derive(Default)]
struct Cache {
    entries: Vec<Entry>,
}
const BUDGET: usize = 32 * 1024 * 1024;
impl Cache {
    fn get(&mut self, key: &Key) -> Option<Arc<Raster>> {
        let index = self.entries.iter().position(|entry| &entry.key == key)?;
        let entry = self.entries.remove(index);
        if !entry.alive() {
            return None;
        }
        let result = entry.result.upgrade()?;
        self.entries.push(entry);
        Some(result)
    }
    fn insert(
        &mut self,
        key: Key,
        source: &Arc<Raster>,
        filtered: &Arc<Raster>,
        mask: &Arc<Mask>,
        result: &Arc<Raster>,
    ) {
        self.entries
            .retain(|e| e.key != key && e.alive() && e.result.strong_count() > 0);
        // Count every possible tiled mip allocation; tiny sparse planes must
        // not be undercounted by a dense width*height estimate.
        let bytes = (0..=result.max_level())
            .map(|level| {
                let (x, y) = result.tiles_at(level);
                (x as usize)
                    .saturating_mul(y as usize)
                    .saturating_mul(emulsion_raster::TILE_PX * 8 + 128)
            })
            .fold(256usize, usize::saturating_add);
        if bytes <= BUDGET {
            let mut retained: usize = self
                .entries
                .iter()
                .filter(|e| e.warm.is_some())
                .map(|e| e.bytes)
                .sum();
            for entry in &mut self.entries {
                if retained.saturating_add(bytes) <= BUDGET {
                    break;
                }
                if entry.warm.take().is_some() {
                    retained = retained.saturating_sub(entry.bytes);
                }
            }
        }
        self.entries.push(Entry {
            key,
            source: Arc::downgrade(source),
            filtered: Arc::downgrade(filtered),
            mask: Arc::downgrade(mask),
            result: Arc::downgrade(result),
            warm: (bytes <= BUDGET).then(|| result.clone()),
            bytes,
        });
        if self.entries.len() > 128 {
            self.entries.remove(0);
        }
    }
}
fn cache() -> &'static Mutex<Cache> {
    static CACHE: OnceLock<Mutex<Cache>> = OnceLock::new();
    CACHE.get_or_init(|| Mutex::new(Cache::default()))
}
pub(crate) fn effective_pixels(
    node: &Node,
    space: BlendSpace,
) -> Result<Option<Arc<Raster>>, GeometryError> {
    // H does not affect mixing or its cache key, but must be admitted before a
    // hit, as must every disabled, uniform or latent component descriptor.
    let support = crate::composite_mask_cache::sampling_support(node)?;
    let NodeKind::Smart {
        source,
        cache: filtered,
        offset,
        filters,
        filter_styles,
        filters_enabled,
        filter_mask,
        ..
    } = &node.kind
    else {
        return Ok(None);
    };
    if !crate::smart::has_active_filters(filters, filter_styles, *filters_enabled) {
        return Ok(Some(source.clone()));
    }
    let Some(mask) = filter_mask
        .as_ref()
        .filter(|mask| mask.enabled && mask.properties.density != 0.)
    else {
        return Ok(Some(filtered.clone()));
    };
    if filters.is_empty() || (mask.pixels.tile_count() == 0 && mask.pixels.fill() == 255) {
        return Ok(Some(filtered.clone()));
    }
    let key = Key {
        space,
        source: Arc::as_ptr(source) as usize,
        source_id: source.content_id(),
        filtered: Arc::as_ptr(filtered) as usize,
        filtered_id: filtered.content_id(),
        mask: Arc::as_ptr(&mask.pixels) as usize,
        mask_id: mask.pixels.content_id(),
        offset: *offset,
        dimensions: [
            source.width(),
            source.height(),
            filtered.width(),
            filtered.height(),
            mask.pixels.width(),
            mask.pixels.height(),
        ],
        matrix: mask.transform.into(),
        properties: [
            mask.properties.density.to_bits(),
            mask.properties.feather.to_bits(),
        ],
    };
    if let Some(result) = cache().lock().get(&key) {
        return Ok(Some(result));
    }
    // Neither projection, feathering nor pixel mixing holds the shared mutex.
    let coverage = crate::composite_mask_cache::derive_raster_mask_with_plan(
        &mask.pixels,
        mask.properties,
        mask.transform,
        crate::composite_mask_cache::MaskGrid {
            width: filtered.width(),
            height: filtered.height(),
            offset: *offset,
        },
        support
            .as_ref()
            .and_then(|support| support.filter_mask())
            .and_then(|support| support.projective_plan()),
    )?;
    if coverage.tile_count() == 0 && coverage.fill() == 255 {
        // A projected-white result aliases authoritative F. Do not retain that
        // source through the derived cache's warm strong result reference.
        return Ok(Some(filtered.clone()));
    }
    let result = Arc::new(crate::smart_filter_mask::build_plane(
        filtered.width(),
        filtered.height(),
        [0; 4],
        |x, y| {
            let (sx, sy) = (
                i64::from(x) + i64::from(offset.0),
                i64::from(y) + i64::from(offset.1),
            );
            let original = if sx >= 0
                && sy >= 0
                && sx < i64::from(source.width())
                && sy < i64::from(source.height())
            {
                source.get(sx as u32, sy as u32)
            } else {
                [0; 4]
            };
            let full = filtered.get(x, y);
            let m = u32::from(coverage.get(x, y));
            if space == BlendSpace::PhotoshopSrgbV1 {
                return encoded_mix(original, full, m);
            }
            std::array::from_fn(|channel| {
                ((u32::from(original[channel]) * (255 - m) + u32::from(full[channel]) * m + 127)
                    / 255) as u16
            })
        },
    ));
    let mut cache = cache().lock();
    if let Some(existing) = cache.get(&key) {
        return Ok(Some(existing));
    }
    cache.insert(key, source, filtered, &mask.pixels, &result);
    Ok(Some(result))
}

/// Transfer straight RGB, premultiply in encoded coordinates, then crossfade.
/// Keep the pre-existing integer alpha rounding exactly. Using alpha-weighted
/// encoded numerators avoids dividing by zero or transferring premultiplied RGB.
/// This is a stack-mask operation only, not per-filter blend/opacity semantics.
fn encoded_mix(original: [u16; 4], full: [u16; 4], m: u32) -> [u16; 4] {
    if m == 0 || original == full {
        return original;
    }
    if m == 255 {
        return full;
    }
    let source_weight = u32::from(original[3]) * (255 - m);
    let filtered_weight = u32::from(full[3]) * m;
    let weight = source_weight + filtered_weight;
    let alpha = ((weight + 127) / 255) as u16;
    if alpha == 0 {
        return [0; 4];
    }
    let mut result = [0, 0, 0, alpha];
    for (channel, value) in result[..3].iter_mut().enumerate() {
        let encoded = |pixel: [u16; 4]| {
            if pixel[3] == 0 {
                0.
            } else {
                color::linear_to_srgb(f32::from(pixel[channel]) / f32::from(pixel[3]))
            }
        };
        let straight = (encoded(original) * source_weight as f32
            + encoded(full) * filtered_weight as f32)
            / weight as f32;
        *value = (color::srgb_to_linear(straight).clamp(0., 1.) * f32::from(alpha) + 0.5) as u16;
    }
    result
}

#[cfg(test)]
mod tests {
    use super::*;
    fn key(source: &Arc<Raster>, filtered: &Arc<Raster>, mask: &Arc<Mask>) -> Key {
        Key {
            space: BlendSpace::Linear,
            source: Arc::as_ptr(source) as usize,
            source_id: source.content_id(),
            filtered: Arc::as_ptr(filtered) as usize,
            filtered_id: filtered.content_id(),
            mask: Arc::as_ptr(mask) as usize,
            mask_id: mask.content_id(),
            offset: (0, 0),
            dimensions: [1; 6],
            matrix: MappingKey::Affine([0; 6]),
            properties: [0; 2],
        }
    }
    #[test]
    fn encoded_mix_preserves_sub_byte_alpha_and_exact_endpoints() {
        for a in [[0; 4], [1; 4], [11, 2, 7, 17], [40000, 1, 65535, 65535]] {
            for b in [[0; 4], [1; 4], [200, 83, 107, 257], [65535; 4]] {
                for m in 0..=255 {
                    let pixel = encoded_mix(a, b, m);
                    let alpha =
                        ((u32::from(a[3]) * (255 - m) + u32::from(b[3]) * m + 127) / 255) as u16;
                    assert_eq!(pixel[3], alpha);
                    assert!(pixel[..3].iter().all(|v| *v <= alpha));
                    if alpha == 0 {
                        assert_eq!(pixel, [0; 4]);
                    }
                    if m == 0 {
                        assert_eq!(pixel, a);
                    }
                    if m == 255 {
                        assert_eq!(pixel, b);
                    }
                }
            }
        }
    }
    #[test]
    fn warm_effective_cache_accounts_complete_mips_and_stays_byte_bounded() {
        let mut cache = Cache::default();
        let source = Arc::new(Raster::transparent(1, 1));
        let filtered = Arc::new(Raster::transparent(1, 1));
        let masks: Vec<_> = (0..20).map(|i| Arc::new(Mask::empty(1, 1, i))).collect();
        let results: Vec<_> = (0..20)
            .map(|_| Arc::new(Raster::transparent(256, 256)))
            .collect();
        for (mask, result) in masks.iter().zip(&results) {
            cache.insert(
                key(&source, &filtered, mask),
                &source,
                &filtered,
                mask,
                result,
            );
        }
        assert!(
            cache
                .entries
                .iter()
                .filter(|e| e.warm.is_some())
                .map(|e| e.bytes)
                .sum::<usize>()
                <= BUDGET
        );
        assert!(cache.entries.iter().any(|e| e.warm.is_none()));
        assert!(cache.entries.iter().all(|e| e.bytes > 256 * 256 * 8));
        assert!(Arc::ptr_eq(
            &cache.get(&key(&source, &filtered, &masks[0])).unwrap(),
            &results[0]
        ));
    }
    #[test]
    fn oversized_results_are_weakly_reused_without_retaining_source_or_deleted_nodes() {
        let mut cache = Cache::default();
        let source = Arc::new(Raster::transparent(1, 1));
        let filtered = Arc::new(Raster::transparent(1, 1));
        let mask = Arc::new(Mask::empty(1, 1, 0));
        let result = Arc::new(Raster::transparent(4096, 4096));
        let key = key(&source, &filtered, &mask);
        cache.insert(key.clone(), &source, &filtered, &mask, &result);
        assert!(cache.entries[0].warm.is_none());
        assert!(Arc::ptr_eq(&cache.get(&key).unwrap(), &result));
        assert_eq!(Arc::strong_count(&source), 1);
        assert_eq!(Arc::strong_count(&filtered), 1);
        assert_eq!(Arc::strong_count(&mask), 1);
        drop(source);
        assert!(cache.get(&key).is_none());
        assert!(cache.entries.is_empty());
    }
    #[test]
    fn expired_oversized_result_is_not_revived_from_a_stale_cache_key() {
        let mut cache = Cache::default();
        let source = Arc::new(Raster::transparent(1, 1));
        let filtered = Arc::new(Raster::transparent(1, 1));
        let mask = Arc::new(Mask::empty(1, 1, 0));
        let result = Arc::new(Raster::transparent(4096, 4096));
        let key = key(&source, &filtered, &mask);
        cache.insert(key.clone(), &source, &filtered, &mask, &result);
        drop(result);
        assert!(cache.get(&key).is_none());
        assert!(cache.entries.is_empty());
    }

    // Authored source-only regressions; not executed in this slice.
    fn projected_node() -> Node {
        use crate::mapping::Mapping2;
        use emulsion_raster::projective::Projective2;
        let mut node = Node::smart(
            1,
            "Projected stack mask",
            Arc::new(Raster::from_fn(4, 4, [0; 4], |x, y| {
                [1000 + x as u16 * 100, 2000 + y as u16 * 100, 3000, 65535]
            })),
            vec![emulsion_filters::Filter::Invert],
            emulsion_raster::Placement::default(),
        );
        let mut mask = crate::SmartFilterMask::new(Arc::new(Mask::from_fn(4, 4, 255, |x, y| {
            if x == y { 64 } else { 192 }
        })));
        mask.transform = Mapping2::Projective(Projective2::IDENTITY);
        if let NodeKind::Smart { filter_mask, .. } = &mut node.kind {
            *filter_mask = Some(mask);
        }
        node
    }

    #[test]
    fn linked_projective_content_motion_reuses_effective_pixels_and_raw_filter_cache() {
        use crate::mapping::{Mapping2, SmartPlacement};
        use emulsion_raster::projective::Projective2;
        let mut node = projected_node();
        let first = effective_pixels(&node, BlendSpace::Linear)
            .unwrap()
            .unwrap();
        let (raw, relative) = match &node.kind {
            NodeKind::Smart {
                cache,
                filter_mask: Some(mask),
                ..
            } => (cache.clone(), mask.transform),
            _ => unreachable!(),
        };
        if let NodeKind::Smart { placement, .. } = &mut node.kind {
            *placement = SmartPlacement::Projective(
                Projective2::from_row_major([1., 0., 2., 0., 1., -3., 0.015625, 0., 1.]).unwrap(),
            );
        }
        let moved = effective_pixels(&node, BlendSpace::Linear)
            .unwrap()
            .unwrap();
        assert!(Arc::ptr_eq(&first, &moved));
        if let NodeKind::Smart {
            cache,
            filter_mask: Some(mask),
            ..
        } = &mut node.kind
        {
            assert!(Arc::ptr_eq(cache, &raw));
            assert_eq!(mask.transform, relative);
            mask.linked = false;
            mask.transform = Mapping2::Projective(
                Projective2::from_affine(glam::DAffine2::from_translation(glam::dvec2(1., 0.)))
                    .unwrap(),
            );
        }
        let compensated = effective_pixels(&node, BlendSpace::Linear)
            .unwrap()
            .unwrap();
        assert!(!Arc::ptr_eq(&first, &compensated));
        assert!(Arc::ptr_eq(
            &compensated,
            &effective_pixels(&node, BlendSpace::Linear)
                .unwrap()
                .unwrap()
        ));
        if let NodeKind::Smart { cache, .. } = &node.kind {
            assert!(Arc::ptr_eq(cache, &raw));
        }
    }

    #[test]
    fn bypass_uses_source_zero_grid_for_both_components_and_retains_obsolete_raw_cache() {
        use crate::mapping::Mapping2;
        use emulsion_raster::projective::Projective2;
        let mut node = projected_node();
        node.mask = Some(Arc::new(Mask::from_fn(4, 4, 0, |x, y| {
            (x * 17 + y * 31) as u8
        })));
        node.mask_transform = Mapping2::Projective(Projective2::IDENTITY);
        let (source, retained) = if let NodeKind::Smart {
            source,
            cache,
            offset,
            filters_enabled,
            ..
        } = &mut node.kind
        {
            *cache = Arc::new(Raster::transparent(10, 8));
            *offset = (-3, -2);
            *filters_enabled = false;
            (source.clone(), cache.clone())
        } else {
            unreachable!()
        };
        assert_eq!(
            crate::smart_support::output_grid(&node).unwrap(),
            crate::smart_support::SmartOutputGrid {
                size: (4, 4),
                offset: (0, 0)
            }
        );
        assert!(Arc::ptr_eq(
            &source,
            &effective_pixels(&node, BlendSpace::Linear)
                .unwrap()
                .unwrap()
        ));
        let layer = crate::composite_mask_cache::mask_for_inspection(&node, (40, 40))
            .unwrap()
            .unwrap();
        let filter = crate::smart_filter_mask::for_inspection(&node)
            .unwrap()
            .unwrap();
        assert_eq!((layer.width(), layer.height()), (4, 4));
        assert_eq!((filter.width(), filter.height()), (4, 4));
        assert_eq!(layer.get(0, 0), 0);
        assert_eq!(filter.get(0, 0), 64);
        if let NodeKind::Smart {
            cache,
            offset,
            filters_enabled,
            ..
        } = &mut node.kind
        {
            assert!(Arc::ptr_eq(cache, &retained));
            assert_eq!(*offset, (-3, -2));
            *filters_enabled = true;
        }
        // The same retained raw resource is now an invalid active footprint.
        assert!(effective_pixels(&node, BlendSpace::Linear).is_err());
        assert!(crate::smart_filter_mask::for_inspection(&node).is_err());
    }

    #[test]
    fn effective_cache_checks_changed_owner_support_and_dormant_descriptors_before_shortcuts() {
        use crate::mapping::{Mapping2, SmartPlacement};
        use emulsion_raster::projective::Projective2;
        let mut node = projected_node();
        let _warm = effective_pixels(&node, BlendSpace::Linear)
            .unwrap()
            .unwrap();
        if let NodeKind::Smart { placement, .. } = &mut node.kind {
            *placement = SmartPlacement::Projective(
                Projective2::from_row_major([1., 0., 0., 0., 1., 0., -0.5, 0., 1.]).unwrap(),
            );
        }
        assert!(effective_pixels(&node, BlendSpace::Linear).is_err());
        let tiny = 2.0_f64.powi(-600);
        let unsafe_map = Mapping2::Projective(
            Projective2::from_row_major([1., 0., 0., 0., tiny, 0., 0., 0., tiny]).unwrap(),
        );
        for enabled in [false, true] {
            for filters_enabled in [false, true] {
                for density in [0., 1.] {
                    let mut node = projected_node();
                    if let NodeKind::Smart {
                        filter_mask: Some(mask),
                        filters_enabled: root,
                        ..
                    } = &mut node.kind
                    {
                        mask.transform = unsafe_map;
                        mask.pixels = Arc::new(Mask::empty(4, 4, 255));
                        mask.enabled = enabled;
                        mask.properties.density = density;
                        *root = filters_enabled;
                    }
                    assert!(effective_pixels(&node, BlendSpace::Linear).is_err());
                    assert!(crate::smart_filter_mask::for_inspection(&node).is_err());
                }
            }
        }
    }

    #[test]
    fn effective_cache_mapping_keys_preserve_variants_and_signed_zero_bits() {
        use crate::mapping::Mapping2;
        use emulsion_raster::projective::Projective2;
        let source = Arc::new(Raster::transparent(1, 1));
        let filtered = Arc::new(Raster::transparent(1, 1));
        let mask = Arc::new(Mask::empty(1, 1, 128));
        let mut affine = key(&source, &filtered, &mask);
        affine.matrix = Mapping2::IDENTITY.into();
        let mut projected = affine.clone();
        projected.matrix = Mapping2::Projective(Projective2::IDENTITY).into();
        assert!(affine != projected);
        let mut signed = affine.clone();
        signed.matrix = Mapping2::from_affine_columns([1., -0., 0., 1., 0., 0.])
            .unwrap()
            .into();
        assert!(affine != signed);
    }
}
