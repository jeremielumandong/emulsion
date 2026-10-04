//! Byte-bounded effective Smart raster cache. Mask edits never render filters.
use crate::{Node, NodeKind};
use emulsion_raster::{Mask, Raster};
use parking_lot::Mutex;
use std::sync::{Arc, OnceLock, Weak};
#[derive(Clone, PartialEq, Eq)]
struct Key {
    source: usize,
    source_id: u64,
    filtered: usize,
    filtered_id: u64,
    mask: usize,
    mask_id: u64,
    offset: (i32, i32),
    dimensions: [u32; 6],
    matrix: [u64; 6],
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
pub(crate) fn effective_pixels(node: &Node) -> Option<Arc<Raster>> {
    let NodeKind::Smart {
        source,
        cache: filtered,
        offset,
        filters,
        filter_mask,
        ..
    } = &node.kind
    else {
        return None;
    };
    let Some(mask) = filter_mask
        .as_ref()
        .filter(|mask| mask.enabled && mask.properties.density != 0.)
    else {
        return Some(filtered.clone());
    };
    if filters.is_empty() || (mask.pixels.tile_count() == 0 && mask.pixels.fill() == 255) {
        return Some(filtered.clone());
    }
    let key = Key {
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
        matrix: mask.transform.map(f64::to_bits),
        properties: [
            mask.properties.density.to_bits(),
            mask.properties.feather.to_bits(),
        ],
    };
    if let Some(result) = cache().lock().get(&key) {
        return Some(result);
    }
    // Neither projection, feathering nor pixel mixing holds the shared mutex.
    let coverage = crate::smart_filter_mask::for_inspection(node)?;
    if coverage.tile_count() == 0 && coverage.fill() == 255 {
        // A projected-white result aliases authoritative F. Do not retain that
        // source through the derived cache's warm strong result reference.
        return Some(filtered.clone());
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
            std::array::from_fn(|channel| {
                ((u32::from(original[channel]) * (255 - m) + u32::from(full[channel]) * m + 127)
                    / 255) as u16
            })
        },
    ));
    let mut cache = cache().lock();
    if let Some(existing) = cache.get(&key) {
        return Some(existing);
    }
    cache.insert(key, source, filtered, &mask.pixels, &result);
    Some(result)
}

#[cfg(test)]
mod tests {
    use super::*;
    fn key(source: &Arc<Raster>, filtered: &Arc<Raster>, mask: &Arc<Mask>) -> Key {
        Key {
            source: Arc::as_ptr(source) as usize,
            source_id: source.content_id(),
            filtered: Arc::as_ptr(filtered) as usize,
            filtered_id: filtered.content_id(),
            mask: Arc::as_ptr(mask) as usize,
            mask_id: mask.content_id(),
            offset: (0, 0),
            dimensions: [1; 6],
            matrix: [0; 6],
            properties: [0; 2],
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
}
