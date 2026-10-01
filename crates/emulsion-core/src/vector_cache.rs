//! Rasterised pixels for a vector layer, produced on first use.
//!
//! A text or path layer keeps document-space pixels so the CPU compositor has
//! something to draw. Producing them costs 12 ms for modest text on a 4K
//! document and rises with glyph coverage, and a transform used to pay it
//! eagerly — so dragging a text box rasterised the whole document on every
//! pointer move.
//!
//! Transforming or restyling now only records what the pixels should be. They
//! are rendered when something asks for them, which a renderer that draws the
//! vector directly — the GPU canvas, via Vello — never does.

use emulsion_raster::Raster;
use emulsion_raster::vector::{Path, PathStyle};
use std::sync::{Arc, OnceLock};

/// What a vector layer's pixels are rendered from.
// Source is allocated once behind Arc; retaining the bounded Copy paint avoids
// a second per-path allocation, while cloned caches share this immutable source.
#[allow(clippy::large_enum_variant)]
#[derive(Clone, Debug)]
enum Source {
    Path {
        path: Arc<Path>,
        style: PathStyle,
        w: u32,
        h: u32,
    },
    Text {
        spec: Arc<crate::text::TextSpec>,
        w: u32,
        h: u32,
    },
    Strokes {
        strokes: Arc<emulsion_raster::strokes::StrokeSet>,
        w: u32,
        h: u32,
    },
}

/// A vector layer's pixels, rendered on demand.
///
/// Cloning shares the published result. Concurrent cold requests may duplicate
/// rasterization work, but use the same pixels once publication completes.
#[derive(Clone, Debug)]
pub struct VectorRaster {
    ready: Arc<OnceLock<Arc<Raster>>>,
    source: Arc<Source>,
}

impl VectorRaster {
    /// Pixels for a path, rendered when first needed.
    pub fn path(path: Arc<Path>, style: PathStyle, w: u32, h: u32) -> Self {
        Self {
            ready: Arc::new(OnceLock::new()),
            source: Arc::new(Source::Path { path, style, w, h }),
        }
    }

    /// Pixels for a text layer, rendered when first needed.
    pub fn text(spec: Arc<crate::text::TextSpec>, w: u32, h: u32) -> Self {
        Self {
            ready: Arc::new(OnceLock::new()),
            source: Arc::new(Source::Text { spec, w, h }),
        }
    }

    /// Pixels for a vector stroke layer, rendered when first needed.
    pub fn strokes(strokes: Arc<emulsion_raster::strokes::StrokeSet>, w: u32, h: u32) -> Self {
        Self {
            ready: Arc::new(OnceLock::new()),
            source: Arc::new(Source::Strokes { strokes, w, h }),
        }
    }

    /// Pixels that are already rendered, for a caller that has them in hand.
    pub fn rendered(raster: Arc<Raster>, source_of: &Self) -> Self {
        let ready = Arc::new(OnceLock::new());
        let _ = ready.set(raster);
        Self {
            ready,
            source: source_of.source.clone(),
        }
    }

    /// The pixels, rendering them if this is the first ask.
    pub fn pixels(&self) -> &Arc<Raster> {
        if let Some(raster) = self.ready.get() {
            return raster;
        }
        // Rasterization can enter Rayon. A stolen task may ask for these same
        // pixels, so only publication (never rasterization) holds OnceLock's
        // initialization lock. Simultaneous cold requests can duplicate work.
        let raster = match self.source.as_ref() {
            Source::Path { path, style, w, h } => Arc::new(path.rasterize(style, *w, *h)),
            Source::Text { spec, w, h } => Arc::new(crate::text::rasterize(spec, *w, *h)),
            Source::Strokes { strokes, w, h } => Arc::new(strokes.rasterize(*w, *h)),
        };
        self.ready.get_or_init(|| raster)
    }

    /// Inspect allocated pixels without invoking the CPU rasterizer. Memory
    /// accounting must not create caches that a Vello-only edit does not need.
    pub fn rendered_pixels(&self) -> Option<&Arc<Raster>> {
        self.ready.get()
    }

    /// A stable identity for this cache, for callers that key on the pixels
    /// without needing them. A new cache -- made by any edit -- gets a new
    /// identity, which is what such callers are really asking about.
    pub fn id(&self) -> usize {
        Arc::as_ptr(&self.ready) as *const u8 as usize
    }

    /// Whether the pixels exist already, so a caller can avoid forcing them.
    pub fn is_rendered(&self) -> bool {
        self.ready.get().is_some()
    }

    /// The document size these pixels are rendered at.
    pub fn size(&self) -> (u32, u32) {
        match self.source.as_ref() {
            Source::Path { w, h, .. }
            | Source::Text { w, h, .. }
            | Source::Strokes { w, h, .. } => (*w, *h),
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    #[test]
    fn cold_vector_callers_publish_one_shared_cache_and_warm_reads_are_stable() {
        let cache = VectorRaster::path(
            Arc::new(emulsion_raster::vector_geometry::rectangle(
                0., 0., 32., 32.,
            )),
            PathStyle {
                fill: Some([255; 4]),
                stroke: None,
                ..Default::default()
            },
            32,
            32,
        );
        assert!(!cache.is_rendered());
        let barrier = Arc::new(std::sync::Barrier::new(4));
        let handles: Vec<_> = (0..4)
            .map(|_| {
                let cache = cache.clone();
                let barrier = barrier.clone();
                std::thread::spawn(move || {
                    barrier.wait();
                    cache.pixels().clone()
                })
            })
            .collect();
        let pixels: Vec<_> = handles.into_iter().map(|h| h.join().unwrap()).collect();
        for raster in &pixels {
            assert!(Arc::ptr_eq(raster, cache.pixels()));
            assert_eq!(raster.get(16, 16), [65535; 4]);
        }
        assert!(cache.is_rendered());
        assert!(Arc::ptr_eq(
            cache.rendered_pixels().unwrap(),
            cache.pixels()
        ));
    }
}
