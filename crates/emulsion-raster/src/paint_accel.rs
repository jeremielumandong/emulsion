//! Optional batched stroke compositing. CPU pixels remain authoritative.
//!
//! The GPU crate installs this hook without making raster storage depend on a
//! graphics API. Declining or failing a batch leaves the CPU path unchanged.

use crate::paint::{BrushBlend, Clip};
use crate::{Raster, TileCoord};
use std::sync::{Arc, OnceLock};

pub struct PaintBatch<'a> {
    pub base: &'a Raster,
    pub tiles: &'a [(TileCoord, &'a [[f32; 6]])],
    pub clip: Option<&'a Clip>,
    pub opacity: f32,
    pub blend: BrushBlend,
    pub erase: bool,
    pub alpha_lock: bool,
}

pub trait PaintCompositor: Send + Sync {
    /// Return complete RGBA16 tiles in request order, or decline atomically.
    fn composite_paint(&self, batch: &PaintBatch<'_>) -> Option<Vec<Vec<[u16; 4]>>>;
}

static COMPOSITOR: OnceLock<Arc<dyn PaintCompositor>> = OnceLock::new();

/// Install once after a usable GPU context has been established.
pub fn install(compositor: Arc<dyn PaintCompositor>) {
    let _ = COMPOSITOR.set(compositor);
}

pub(crate) fn compositor() -> Option<&'static dyn PaintCompositor> {
    COMPOSITOR.get().map(AsRef::as_ref)
}
