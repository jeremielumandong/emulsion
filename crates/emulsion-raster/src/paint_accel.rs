//! Optional batched composition and persistent stroke accumulation.
//!
//! Graphics backends install these hooks without coupling raster storage to a
//! graphics API. Persistent strokes retain a CPU dab journal for recovery;
//! rendering still produces immutable CPU rasters for history and presentation.

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

/// Fully resolved dry round dab. CPU input dynamics remain authoritative.
#[derive(Clone, Copy, Debug)]
pub struct ResolvedDab {
    pub center: [f32; 2],
    pub radius: f32,
    pub hardness: f32,
    pub flow: f32,
    pub color: [f32; 4],
}

pub trait PersistentStroke: Send + Sync {
    fn append(&mut self, dabs: &[ResolvedDab]) -> bool;
    /// Complete row-major RGBA16 image. Failure triggers CPU journal recovery.
    fn preview(&mut self) -> Option<Vec<[u16; 4]>>;
}

pub trait PersistentFactory: Send + Sync {
    fn start(&self, base: &Raster, opacity: f32) -> Option<Box<dyn PersistentStroke>>;
}

static PERSISTENT: OnceLock<Arc<dyn PersistentFactory>> = OnceLock::new();

pub fn install_persistent(factory: Arc<dyn PersistentFactory>) {
    let _ = PERSISTENT.set(factory);
}

pub(crate) fn persistent() -> Option<Arc<dyn PersistentFactory>> {
    PERSISTENT.get().cloned()
}
