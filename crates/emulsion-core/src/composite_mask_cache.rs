//! Derived layer masks are shared across geometry, style, and compositor queries.
//! Warm results are byte bounded; oversized live results use weak references.
use crate::{Node, NodeKind};
use emulsion_raster::Mask;
use parking_lot::Mutex;
use std::sync::{Arc, OnceLock, Weak};

#[derive(Clone, PartialEq, Eq)]
struct Key {
    source: usize,
    content_id: u64,
    width: u32,
    height: u32,
    offset: (i32, i32),
    matrix: [u64; 6],
}
struct Entry {
    key: Key,
    source: Weak<Mask>,
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
        if entry.source.strong_count() == 0 {
            return None;
        }
        let result = entry.result.upgrade()?;
        self.entries.push(entry);
        Some(result)
    }
    fn insert(&mut self, key: Key, source: &Arc<Mask>, result: &Arc<Mask>) {
        self.entries
            .retain(|e| e.key != key && e.source.strong_count() > 0 && e.result.strong_count() > 0);
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
            source: Arc::downgrade(source),
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

pub(crate) fn composite_mask(node: &Node) -> Option<Arc<Mask>> {
    let mask = node.mask_enabled.then_some(node.mask.as_ref()).flatten()?;
    let identity = node.mask_transform == crate::node::default_mask_transform();
    let (w, h, offset) = match &node.kind {
        NodeKind::Raster { raster, .. } => (raster.width(), raster.height(), (0, 0)),
        NodeKind::Smart { cache, offset, .. } => (cache.width(), cache.height(), *offset),
        NodeKind::Text { cache, .. } | NodeKind::Path { cache, .. } => {
            (cache.width(), cache.height(), (0, 0))
        }
        _ => (mask.width(), mask.height(), (0, 0)),
    };
    // Preserve the direct-mask path, including its original finite dimensions.
    if identity
        && (!matches!(node.kind, NodeKind::Smart { .. })
            || (offset == (0, 0) && (w, h) == (mask.width(), mask.height())))
    {
        return Some(mask.clone());
    }
    // An allocation-free fill plane is constant both inside and outside its
    // bounds (outside mask sampling uses fill), under every affine transform.
    if mask.tile_count() == 0 && (w, h) == (mask.width(), mask.height()) {
        return Some(mask.clone());
    }
    let key = Key {
        source: Arc::as_ptr(mask) as usize,
        content_id: mask.content_id(),
        width: w,
        height: h,
        offset,
        matrix: node.mask_transform.map(f64::to_bits),
    };
    if let Some(result) = cache().lock().get(&key) {
        return Some(result);
    }
    let derived = if mask.tile_count() == 0 {
        Mask::empty(w, h, mask.fill())
    } else if identity {
        Mask::from_fn(w, h, mask.fill(), |x, y| {
            let sx = x as i64 + offset.0 as i64;
            let sy = y as i64 + offset.1 as i64;
            if sx < 0 || sy < 0 || sx >= mask.width() as i64 || sy >= mask.height() as i64 {
                mask.fill()
            } else {
                mask.get(sx as u32, sy as u32)
            }
        })
    } else {
        let inverse = glam::DAffine2::from_cols_array(&node.mask_transform).inverse();
        Mask::from_fn(w, h, mask.fill(), |x, y| {
            crate::transform::sample_mask(
                mask,
                inverse.transform_point2(glam::dvec2(
                    x as f64 + offset.0 as f64 + 0.5,
                    y as f64 + offset.1 as f64 + 0.5,
                )),
            )
        })
    };
    let result = Arc::new(derived);
    let mut cache = cache().lock();
    // Concurrent readers may finish the same resampling together; share the
    // first published allocation rather than returning competing identities.
    if let Some(existing) = cache.get(&key) {
        return Some(existing);
    }
    cache.insert(key, mask, &result);
    Some(result)
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
        n.mask_transform[4] = 4.;
        n
    }
    #[test]
    fn repeated_queries_share_derived_mask_and_changes_invalidate() {
        let mut n = node();
        let a = Document::composite_mask(&n).unwrap();
        let b = Document::composite_mask(&n).unwrap();
        assert!(Arc::ptr_eq(&a, &b));
        assert_eq!(a.get(5, 2), 0);
        assert_eq!(a.get(1, 2), 255);
        n.mask_transform[4] = 8.;
        let c = Document::composite_mask(&n).unwrap();
        assert!(!Arc::ptr_eq(&a, &c));
        assert_eq!(c.get(5, 2), 255);
        n.mask = Some(Arc::new(Mask::from_fn(32, 24, 255, |x, _| {
            if x < 3 { 0 } else { 255 }
        })));
        let d = Document::composite_mask(&n).unwrap();
        assert!(!Arc::ptr_eq(&c, &d));
        if let NodeKind::Raster { raster, .. } = &mut n.kind {
            *raster = Arc::new(Raster::empty(48, 24, [0; 4]));
        }
        let e = Document::composite_mask(&n).unwrap();
        assert_eq!(e.width(), 48);
        assert!(!Arc::ptr_eq(&d, &e));
        n.mask_enabled = false;
        assert!(Document::composite_mask(&n).is_none());
    }
    #[test]
    fn identity_and_uniform_transforms_preserve_source_and_outside_fill() {
        let mut n = node();
        n.mask_transform = crate::node::default_mask_transform();
        assert!(Arc::ptr_eq(
            n.mask.as_ref().unwrap(),
            &Document::composite_mask(&n).unwrap()
        ));
        for fill in [0, 127, 255] {
            n.mask = Some(Arc::new(Mask::empty(32, 24, fill)));
            n.mask_transform = [0.5, 0., 0., 0.5, 900., -70.];
            let a = Document::composite_mask(&n).unwrap();
            assert!(Arc::ptr_eq(n.mask.as_ref().unwrap(), &a));
            if let NodeKind::Raster { raster, .. } = &mut n.kind {
                *raster = Arc::new(Raster::empty(48, 24, [0; 4]));
            }
            let b = Document::composite_mask(&n).unwrap();
            assert_eq!(
                (b.width(), b.height(), b.tile_count(), b.fill()),
                (48, 24, 0, fill)
            );
            assert!(Arc::ptr_eq(&b, &Document::composite_mask(&n).unwrap()));
            if let NodeKind::Raster { raster, .. } = &mut n.kind {
                *raster = Arc::new(Raster::empty(32, 24, [0; 4]));
            }
        }
    }
    fn key(source: &Arc<Mask>, id: u64) -> Key {
        Key {
            source: Arc::as_ptr(source) as usize,
            content_id: source.content_id(),
            width: 32,
            height: 24,
            offset: (0, 0),
            matrix: [id; 6],
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
        n.mask_transform = crate::node::default_mask_transform();
        d.nodes.push(n);
        crate::Command::ConvertToSmart { id: 1 }
            .apply(&mut d)
            .unwrap();
        if let NodeKind::Smart { cache, offset, .. } = &mut d.nodes[0].kind {
            *cache = Arc::new(Raster::empty(40, 24, [0; 4]));
            *offset = (-4, 0);
        }
        let a = Document::composite_mask(&d.nodes[0]).unwrap();
        assert_eq!((a.width(), a.get(1, 2), a.get(5, 2)), (40, 255, 0));
        assert!(Arc::ptr_eq(
            &a,
            &Document::composite_mask(&d.nodes[0]).unwrap()
        ));
        if let NodeKind::Smart { offset, .. } = &mut d.nodes[0].kind {
            *offset = (4, 0);
        }
        let b = Document::composite_mask(&d.nodes[0]).unwrap();
        assert!(!Arc::ptr_eq(&a, &b));
        assert_eq!(b.get(1, 2), 0);
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
}
