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
}

/// A vector layer's pixels, rendered on demand.
///
/// Cloning shares the rendered result, so passing a node around does not
/// duplicate the work or the pixels.
#[derive(Clone, Debug)]
pub struct VectorRaster {
    ready: Arc<OnceLock<Arc<Raster>>>,
    source: Source,
}

impl VectorRaster {
    /// Pixels for a path, rendered when first needed.
    pub fn path(path: Arc<Path>, style: PathStyle, w: u32, h: u32) -> Self {
        Self {
            ready: Arc::new(OnceLock::new()),
            source: Source::Path { path, style, w, h },
        }
    }

    /// Pixels for a text layer, rendered when first needed.
    pub fn text(spec: Arc<crate::text::TextSpec>, w: u32, h: u32) -> Self {
        Self {
            ready: Arc::new(OnceLock::new()),
            source: Source::Text { spec, w, h },
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
        self.ready.get_or_init(|| match &self.source {
            Source::Path { path, style, w, h } => Arc::new(path.rasterize(style, *w, *h)),
            Source::Text { spec, w, h } => Arc::new(crate::text::rasterize(spec, *w, *h)),
        })
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
        match &self.source {
            Source::Path { w, h, .. } | Source::Text { w, h, .. } => (*w, *h),
        }
    }
}
