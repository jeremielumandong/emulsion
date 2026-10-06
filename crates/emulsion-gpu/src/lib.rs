//! Selective GPU image operations with the existing CPU algorithms as fallback.
//! CPU documents remain authoritative for undo, saving, and recovery.
#[cfg(test)]
mod atlas_sampling_tests;
pub mod brush_backend;
mod compositor;
mod context;
mod filters;
mod paint;
pub mod persistent_paint;
#[cfg(test)]
mod persistent_parity;
mod readiness;
pub mod screen;
mod startup;

pub use context::GpuContext;
use startup::{Capability, StartupControl, StartupProfile};
pub use startup::{InitializationHandle, InitializationStatus, STARTUP_BUDGET};
use std::sync::{Arc, OnceLock, Weak, atomic::AtomicBool};

pub const CRATE: &str = "emulsion-gpu";
static GPU: OnceLock<Weak<GpuContext>> = OnceLock::new();
static INITIALIZATION: OnceLock<InitializationHandle> = OnceLock::new();
static PROFILE: OnceLock<StartupProfile> = OnceLock::new();

/// Explicit compute preference overrides measured performance routing, but
/// never correctness, device, or memory limits. Resolve launch settings once.
pub(crate) fn gpu_preferred() -> bool {
    PROFILE
        .get_or_init(StartupProfile::from_environment)
        .preferred
}

/// Start at most one worker and return immediately. Contexts and hooks remain
/// unavailable while the selected production capabilities are being validated.
/// `cpu` creates no worker/device; `software` admits CPU compute adapters only.
/// This does not initialize the native engine or GPUI presentation renderer.
pub fn begin_initialize() -> InitializationHandle {
    INITIALIZATION
        .get_or_init(|| startup::begin(*PROFILE.get_or_init(StartupProfile::from_environment)))
        .clone()
}

/// Compatibility boundary for non-UI tools. Never joins the GPU worker, even
/// after cancellation/timeout. UI code must use `begin_initialize` instead.
pub fn initialize() {
    begin_initialize().wait();
}

pub fn context() -> Option<Arc<GpuContext>> {
    INITIALIZATION
        .get()
        .filter(|handle| handle.control.ready())?;
    GPU.get()?.upgrade().filter(|gpu| gpu.available())
}

/// Experimental synchronous screen readback is exposed only when its exact
/// pipeline was selected and validated before the shared readiness commit.
pub fn screen_context() -> Option<Arc<GpuContext>> {
    PROFILE
        .get()
        .filter(|profile| profile.capabilities & Capability::Screen.bit() != 0)?;
    context()
}

/// Irreversible hooks hold weak device ownership. If registration conflicts
/// part way through, the guards always decline and worker cleanup releases the
/// prepared device; no failed partial hook keeps buffers or pipelines alive.
struct PublishedHooks {
    gpu: Weak<GpuContext>,
    control: Arc<StartupControl>,
    persistent_active: Arc<AtomicBool>,
}
impl PublishedHooks {
    fn gpu(&self) -> Option<Arc<GpuContext>> {
        if !self.control.ready() {
            return None;
        }
        self.gpu.upgrade().filter(|gpu| gpu.available())
    }
}
impl emulsion_raster::composite::TileAccelerator for PublishedHooks {
    fn render_tile(
        &self,
        tree: &emulsion_raster::composite::CompositeTree,
        level: u32,
        tile: emulsion_raster::TileCoord,
    ) -> Option<emulsion_raster::tile::FTile> {
        emulsion_raster::composite::TileAccelerator::render_tile(
            self.gpu()?.as_ref(),
            tree,
            level,
            tile,
        )
    }
}
impl emulsion_filters::FilterAccelerator for PublishedHooks {
    fn apply(
        &self,
        filter: &emulsion_filters::Filter,
        width: usize,
        height: usize,
        pixels: &[[f32; 4]],
    ) -> Option<Vec<[f32; 4]>> {
        emulsion_filters::FilterAccelerator::apply(
            self.gpu()?.as_ref(),
            filter,
            width,
            height,
            pixels,
        )
    }
}
impl emulsion_raster::paint_accel::PaintCompositor for PublishedHooks {
    fn composite_paint(
        &self,
        batch: &emulsion_raster::paint_accel::PaintBatch<'_>,
    ) -> Option<Vec<Vec<[u16; 4]>>> {
        emulsion_raster::paint_accel::PaintCompositor::composite_paint(self.gpu()?.as_ref(), batch)
    }
}
impl emulsion_raster::paint_accel::PersistentFactory for PublishedHooks {
    fn start(
        &self,
        base: &emulsion_raster::Raster,
        opacity: f32,
    ) -> Option<Box<dyn emulsion_raster::paint_accel::PersistentStroke>> {
        emulsion_raster::paint_accel::PersistentFactory::start(
            &brush_backend::BrushFactory::with_permit(self.gpu()?, self.persistent_active.clone()),
            base,
            opacity,
        )
    }
}

fn publish(
    gpu: Arc<GpuContext>,
    profile: StartupProfile,
    control: Arc<StartupControl>,
) -> anyhow::Result<()> {
    anyhow::ensure!(gpu.available(), "Compute device failed at publication");
    // Diagnose pre-existing owners before doing any registration. Each actual
    // OnceLock set is also checked, so a concurrent owner cannot be overlooked.
    anyhow::ensure!(
        !emulsion_raster::composite::accelerator_installed()
            && !emulsion_filters::accelerator_installed(),
        "An image accelerator was already installed"
    );
    if profile.capabilities & Capability::Paint.bit() != 0 {
        anyhow::ensure!(
            !emulsion_raster::paint_accel::compositor_installed(),
            "A paint compositor was already installed"
        );
    }
    if profile.capabilities & Capability::PersistentPaint.bit() != 0 {
        anyhow::ensure!(
            !emulsion_raster::paint_accel::persistent_installed(),
            "A persistent brush factory was already installed"
        );
    }
    let hooks = Arc::new(PublishedHooks {
        gpu: Arc::downgrade(&gpu),
        control,
        persistent_active: Arc::new(AtomicBool::new(false)),
    });
    anyhow::ensure!(
        emulsion_raster::composite::try_install_accelerator(hooks.clone()),
        "Compositor registration conflict"
    );
    anyhow::ensure!(
        emulsion_filters::try_install_accelerator(hooks.clone()),
        "Filter registration conflict"
    );
    if profile.capabilities & Capability::Paint.bit() != 0 {
        anyhow::ensure!(
            emulsion_raster::paint_accel::try_install(hooks.clone()),
            "Paint registration conflict"
        );
    }
    if profile.capabilities & Capability::PersistentPaint.bit() != 0 {
        anyhow::ensure!(
            emulsion_raster::paint_accel::try_install_persistent(hooks),
            "Persistent brush registration conflict"
        );
    }
    anyhow::ensure!(gpu.available(), "Compute device failed during publication");
    GPU.set(Arc::downgrade(&gpu))
        .map_err(|_| anyhow::anyhow!("Compute context registration conflict"))
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
        .get_or_init(|| {
            match startup::PreparedContext::prepare(
                StartupProfile::test_profile(),
                &StartupControl::new(),
            ) {
                Ok(prepared) => Some(prepared.into_context()),
                Err(error) => {
                    assert!(
                        std::env::var("EMULSION_REQUIRE_GPU_TESTS").as_deref() != Ok("1"),
                        "GPU tests required: {error:#}"
                    );
                    eprintln!("Skipping GPU parity checks: {error:#}");
                    None
                }
            }
        })
        .clone()
}

#[cfg(test)]
mod publication_tests {
    use super::*;
    use emulsion_filters::FilterAccelerator;
    use emulsion_raster::composite::TileAccelerator;
    use emulsion_raster::paint_accel::{PaintCompositor, PersistentFactory};

    #[test]
    fn actual_pending_proxies_immediately_decline_all_cpu_operations() {
        let control = StartupControl::new();
        let hooks = PublishedHooks {
            gpu: Weak::new(),
            control: control.clone(),
            persistent_active: Arc::new(AtomicBool::new(false)),
        };
        let tree = emulsion_raster::composite::CompositeTree {
            nodes: Vec::new(),
            width: 1,
            height: 1,
            space: emulsion_raster::blend::BlendSpace::Linear,
            knockout_background: None,
        };
        let base = emulsion_raster::Raster::empty(1, 1, [0; 4]);
        let batch = emulsion_raster::paint_accel::PaintBatch {
            base: &base,
            tiles: &[],
            clip: None,
            opacity: 1.0,
            blend: emulsion_raster::paint::BrushBlend::Normal,
            erase: false,
            alpha_lock: false,
        };
        for _ in 0..3 {
            assert!(
                hooks
                    .render_tile(&tree, 0, emulsion_raster::TileCoord::new(0, 0))
                    .is_none()
            );
            assert!(
                hooks
                    .apply(
                        &emulsion_filters::Filter::Invert,
                        1,
                        1,
                        &[[0.2, 0.1, 0.0, 0.5]]
                    )
                    .is_none()
            );
            assert!(hooks.composite_paint(&batch).is_none());
            assert!(hooks.start(&base, 1.0).is_none());
            assert!(context().is_none());
            assert!(screen_context().is_none());
        }
        InitializationHandle { control }.cancel();
        assert!(hooks.gpu().is_none());
        assert!(hooks.start(&base, 1.0).is_none());
    }
    #[test]
    fn live_prepared_device_still_cannot_bypass_pending_or_cancelled_proxy() {
        let Some(gpu) = test_gpu() else {
            return;
        };
        assert!(gpu.available());
        let control = StartupControl::new();
        let hooks = PublishedHooks {
            gpu: Arc::downgrade(&gpu),
            control: control.clone(),
            persistent_active: Arc::new(AtomicBool::new(false)),
        };
        assert!(hooks.gpu.upgrade().is_some());
        assert!(
            hooks.gpu().is_none(),
            "a live prepared device is insufficient before publication"
        );
        let base = emulsion_raster::Raster::empty(1, 1, [0; 4]);
        assert!(hooks.start(&base, 1.0).is_none());
        InitializationHandle { control }.cancel();
        assert!(
            hooks.gpu().is_none(),
            "cancellation must continue declining a still-live device"
        );
        assert_eq!(
            gpu.persistent_sessions
                .load(std::sync::atomic::Ordering::SeqCst),
            0
        );
    }
}
