//! A byte-bounded warm cache plus weak references to effects already on screen.
//! Large effects are reusable without keeping deleted layers alive.
use super::{Key, Rendered, RenderedEffect};
use crate::style_options::PatternImage;
use crate::{Document, Node, NodeKind};
use emulsion_raster::{BlendMode, IRect, Mask, Raster};
use std::sync::{Arc, Weak};

struct EffectRef {
    raster: Weak<Raster>,
    rect: IRect,
    blend: BlendMode,
}
impl EffectRef {
    fn new(effect: &RenderedEffect) -> Self {
        Self {
            raster: Arc::downgrade(&effect.raster),
            rect: effect.rect,
            blend: effect.blend,
        }
    }
    fn upgrade(&self) -> Option<RenderedEffect> {
        Some(RenderedEffect {
            raster: self.raster.upgrade()?,
            rect: self.rect,
            blend: self.blend,
        })
    }
}

enum SourceRef {
    Raster(Weak<Raster>),
    Mask(Weak<Mask>),
    Pattern(Weak<PatternImage>),
}
impl SourceRef {
    fn alive(&self) -> bool {
        match self {
            Self::Raster(v) => v.strong_count() > 0,
            Self::Mask(v) => v.strong_count() > 0,
            Self::Pattern(v) => v.strong_count() > 0,
        }
    }
}

struct Entry {
    key: Key,
    warm: Option<Arc<Rendered>>,
    bytes: usize,
    below: Vec<EffectRef>,
    above: Vec<EffectRef>,
    sources: Vec<SourceRef>,
}
impl Entry {
    fn upgrade(&self) -> Option<Arc<Rendered>> {
        // Weak source references also prevent address reuse from producing a false hit.
        if !self.sources.iter().all(SourceRef::alive) {
            return None;
        }
        if let Some(warm) = &self.warm {
            return Some(warm.clone());
        }
        Some(Arc::new(Rendered {
            below: self
                .below
                .iter()
                .map(EffectRef::upgrade)
                .collect::<Option<_>>()?,
            above: self
                .above
                .iter()
                .map(EffectRef::upgrade)
                .collect::<Option<_>>()?,
        }))
    }
}

#[derive(Default)]
pub(super) struct Memo {
    entries: Vec<Entry>,
}
impl Memo {
    const BUDGET: usize = 64 * 1024 * 1024;
    #[cfg(test)]
    pub(super) fn evict(&mut self, key: &Key) {
        self.entries.retain(|entry| &entry.key != key);
    }
    pub(super) fn get(&mut self, key: &Key) -> Option<Arc<Rendered>> {
        let index = self.entries.iter().position(|entry| &entry.key == key)?;
        let entry = self.entries.remove(index);
        let rendered = entry.upgrade()?;
        self.entries.push(entry);
        Some(rendered)
    }
    pub(super) fn insert(
        &mut self,
        key: Key,
        rendered: &Arc<Rendered>,
        doc: &Document,
        node: &Node,
    ) {
        self.entries
            .retain(|entry| entry.key != key && entry.upgrade().is_some());
        let bytes = super::rendered_bytes(rendered);
        let mut retained: usize = self
            .entries
            .iter()
            .filter(|e| e.warm.is_some())
            .map(|e| e.bytes)
            .sum();
        if bytes <= Self::BUDGET {
            for entry in &mut self.entries {
                if retained.saturating_add(bytes) <= Self::BUDGET {
                    break;
                }
                if entry.warm.take().is_some() {
                    retained -= entry.bytes;
                }
            }
        }
        let ids = doc.subtree(node.id);
        let mut sources = Vec::new();
        for node in std::iter::once(node).chain(
            doc.nodes
                .iter()
                .filter(|n| n.id != node.id && ids.contains(&n.id)),
        ) {
            match &node.kind {
                NodeKind::Raster { raster, .. } => {
                    sources.push(SourceRef::Raster(Arc::downgrade(raster)))
                }
                NodeKind::Smart { cache, .. }
                | NodeKind::Text { cache, .. }
                | NodeKind::Path { cache, .. } => {
                    sources.push(SourceRef::Raster(Arc::downgrade(cache)))
                }
                _ => {}
            }
            if let Some(mask) = &node.mask {
                sources.push(SourceRef::Mask(Arc::downgrade(mask)));
            }
            for option in &node.style_options {
                if let Some(pattern) = &option.pattern.image {
                    sources.push(SourceRef::Pattern(Arc::downgrade(pattern)));
                }
            }
        }
        self.entries.push(Entry {
            key,
            warm: (bytes <= Self::BUDGET).then(|| rendered.clone()),
            bytes,
            below: rendered.below.iter().map(EffectRef::new).collect(),
            above: rendered.above.iter().map(EffectRef::new).collect(),
            sources,
        });
        if self.entries.len() > 128 {
            self.entries.remove(0);
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    fn effect(side: i32) -> Arc<Rendered> {
        Arc::new(Rendered {
            below: vec![],
            above: vec![RenderedEffect {
                raster: Arc::new(Raster::empty(side as u32, side as u32, [0; 4])),
                rect: IRect::new(0, 0, side, side),
                blend: BlendMode::Normal,
            }],
        })
    }
    #[test]
    fn oversized_effects_reuse_live_rasters_without_retaining_deleted_layers() {
        let mut cache = Memo::default();
        let doc = Document::new(4096, 4096);
        let node = Node::raster(
            1,
            "test",
            Arc::new(Raster::empty(1, 1, [0; 4])),
            Default::default(),
        );
        let key = (0, 0, 0, "large".into());
        let rendered = effect(4096);
        let live_tree_raster = rendered.above[0].raster.clone();
        cache.insert(key.clone(), &rendered, &doc, &node);
        drop(rendered);
        let reused = cache.get(&key).unwrap();
        assert!(Arc::ptr_eq(&reused.above[0].raster, &live_tree_raster));
        drop(reused);
        drop(live_tree_raster);
        assert!(cache.get(&key).is_none());
    }
    #[test]
    fn repeated_style_edits_stay_within_byte_budget() {
        let mut cache = Memo::default();
        let doc = Document::new(1024, 1024);
        let node = Node::raster(
            1,
            "test",
            Arc::new(Raster::empty(1, 1, [0; 4])),
            Default::default(),
        );
        for revision in 0..40 {
            cache.insert((0, 0, revision, String::new()), &effect(1024), &doc, &node);
            let retained: usize = cache
                .entries
                .iter()
                .filter(|e| e.warm.is_some())
                .map(|e| e.bytes)
                .sum();
            assert!(retained <= Memo::BUDGET);
        }
    }
}
