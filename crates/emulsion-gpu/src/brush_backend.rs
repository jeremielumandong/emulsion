//! Adapter for brush-specific routing in the raster engine.
use crate::GpuContext;
use crate::persistent_paint::{Dab, PersistentPaint};
use emulsion_raster::Raster;
use emulsion_raster::paint_accel::{PersistentFactory, PersistentStroke, ResolvedDab};
use std::sync::{
    Arc,
    atomic::{AtomicBool, Ordering},
};

/// One active persistent stroke per factory bounds retained GPU memory.
pub struct BrushFactory {
    gpu: Arc<GpuContext>,
    active: Arc<AtomicBool>,
}

impl BrushFactory {
    pub fn new(gpu: Arc<GpuContext>) -> Self {
        Self {
            gpu,
            active: Arc::new(AtomicBool::new(false)),
        }
    }
}

struct Permit(Arc<AtomicBool>);
impl Drop for Permit {
    fn drop(&mut self) {
        self.0.store(false, Ordering::Release);
    }
}

struct Session {
    paint: PersistentPaint,
    _permit: Permit,
}

impl PersistentFactory for BrushFactory {
    fn start(&self, base: &Raster, opacity: f32) -> Option<Box<dyn PersistentStroke>> {
        if !self.gpu.available()
            || base.width() == 0
            || base.height() == 0
            || base.width() > 1024
            || base.height() > 1024
            || self
                .active
                .compare_exchange(false, true, Ordering::AcqRel, Ordering::Acquire)
                .is_err()
        {
            return None;
        }
        let permit = Permit(self.active.clone());
        let paint = PersistentPaint::new(
            self.gpu.clone(),
            base.width(),
            base.height(),
            &base.to_pixels(),
            opacity,
        )
        .map_err(
            |error| tracing::debug!(%error, "Persistent brush initialization declined; using CPU"),
        )
        .ok()?;
        Some(Box::new(Session {
            paint,
            _permit: permit,
        }))
    }
}

impl PersistentStroke for Session {
    fn append(&mut self, dabs: &[ResolvedDab]) -> bool {
        for batch in dabs.chunks(1024) {
            let packed: Vec<_> = batch
                .iter()
                .map(|dab| Dab {
                    center: dab.center,
                    radius: dab.radius,
                    hardness: dab.hardness,
                    flow: dab.flow,
                    color: dab.color,
                })
                .collect();
            if let Err(error) = self.paint.append(&packed) {
                tracing::debug!(%error, "Persistent brush failed; replaying on CPU");
                return false;
            }
        }
        true
    }

    fn preview(&mut self) -> Option<Vec<[u16; 4]>> {
        self.paint
            .preview()
            .map_err(|error| tracing::debug!(%error, "Persistent preview failed; replaying on CPU"))
            .ok()
    }
}
