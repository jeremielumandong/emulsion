//! Selective GPU image operations with the existing CPU algorithms as fallback.
//! CPU documents remain authoritative for undo, saving, and recovery.
pub mod brush_backend;
mod compositor;
mod context;
mod filters;
mod paint;
pub mod persistent_paint;
#[cfg(test)]
mod persistent_parity;
pub mod screen;

pub use context::GpuContext;
use std::sync::{Arc, OnceLock};

pub const CRATE: &str = "emulsion-gpu";
static GPU: OnceLock<Option<Arc<GpuContext>>> = OnceLock::new();

/// Explicit compute preference overrides measured performance routing, but
/// never correctness, device, or memory limits.
pub(crate) fn gpu_preferred() -> bool {
    matches!(
        std::env::var("EMULSION_GPU").as_deref(),
        Ok("force" | "software")
    )
}

/// Initialize off the UI thread. `EMULSION_GPU=cpu` disables acceleration;
/// `force` attempts every supported compute operation; `software` does the same
/// on CPU adapters for shader validation. Neither affects GPUI presentation.
pub fn initialize() {
    GPU.get_or_init(|| {
        if std::env::var("EMULSION_GPU").as_deref() == Ok("cpu") {
            tracing::info!("Image processing uses CPU (explicit override)");
            return None;
        }
        match GpuContext::new() {
            Ok(context) => {
                tracing::info!(device = context.name(), "GPU image acceleration enabled");
                let context = Arc::new(context);
                emulsion_raster::composite::install_accelerator(context.clone());
                // Full stroke-tile upload/readback measured slower than the CPU
                // reference. Keep it opt-in until stroke tiles stay GPU-resident.
                if std::env::var("EMULSION_GPU_BRUSHES").as_deref() == Ok("1") {
                    emulsion_raster::paint_accel::install(context.clone());
                }
                if std::env::var("EMULSION_GPU_BRUSHES").as_deref() == Ok("persistent") {
                    emulsion_raster::paint_accel::install_persistent(Arc::new(
                        brush_backend::BrushFactory::new(context.clone()),
                    ));
                }
                emulsion_filters::install_accelerator(context.clone());
                Some(context)
            }
            Err(error) => {
                tracing::info!(%error, "GPU image acceleration unavailable; using CPU");
                None
            }
        }
    });
}

pub fn context() -> Option<&'static Arc<GpuContext>> {
    GPU.get()
        .and_then(Option::as_ref)
        .filter(|gpu| gpu.available())
}

/// Screen-image compute currently requires synchronous readback on the UI
/// thread. Keep it experimental until end-to-end viewport latency is measured.
pub fn screen_context() -> Option<&'static Arc<GpuContext>> {
    gpu_preferred().then(context).flatten()
}

impl emulsion_raster::composite::TileAccelerator for GpuContext {
    fn render_tile(
        &self,
        tree: &emulsion_raster::composite::CompositeTree,
        level: u32,
        tile: emulsion_raster::TileCoord,
    ) -> Option<emulsion_raster::tile::FTile> {
        if !self.available() {
            return None;
        }
        // A viewport batch may request dozens of tiles in parallel. Bound CPU
        // preparation memory and avoid serial GPU readback queues for all of
        // them; busy requests immediately use the existing parallel CPU path.
        let _permit = self.tile_permit()?;
        compositor::render_tile(self, tree, level, tile)
            .ok()
            .flatten()
            .filter(|pixels| pixels.iter().flatten().all(|value| value.is_finite()))
    }
}

#[cfg(test)]
pub(crate) fn test_gpu() -> Option<Arc<GpuContext>> {
    static TEST_GPU: OnceLock<Option<Arc<GpuContext>>> = OnceLock::new();
    TEST_GPU
        .get_or_init(|| match GpuContext::new() {
            Ok(gpu) => Some(Arc::new(gpu)),
            Err(error) => {
                assert!(
                    std::env::var("EMULSION_REQUIRE_GPU_TESTS").as_deref() != Ok("1"),
                    "GPU tests required: {error:#}"
                );
                eprintln!("Skipping GPU parity checks: {error:#}");
                None
            }
        })
        .clone()
}
