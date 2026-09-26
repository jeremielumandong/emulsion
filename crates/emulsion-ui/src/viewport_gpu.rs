//! Opt-in GPU canvas (experimental), enabled with `EMULSION_GPU_CANVAS=1`.
//!
//! Draws the document with `emulsion-engine` on the GPU and hands the result to
//! GPUI's renderer, instead of compositing 256 px tiles on the CPU and
//! uploading each as an image. See `spikes/vello-canvas/RESULTS.md` for the
//! measurements this is based on.
//!
//! Hosting is platform-specific and lives in [`emulsion_engine::host`], shared
//! with the spike so both use one implementation: GPUI's own wgpu device on
//! Linux, an IOSurface on macOS, a shared D3D12 resource on Windows.
//!
//! It renders only. Editing works, but every document change recompiles the
//! engine's program and atlas, because the engine has no incremental structural
//! update yet; strokes are not routed to the GPU brush here.
//!
//! The path refuses whenever it cannot reproduce the CPU result, and the caller
//! falls back to [`crate::viewport`]: a rotated view, a document using features
//! the engine does not implement, or a host that cannot supply a device. A
//! refusal is sticky, so the fallback costs one frame at most.

use crate::viewport::View;

/// Platforms with a GPUI hosting backend.
const HOSTED: bool = cfg!(any(
    target_os = "linux",
    target_os = "macos",
    target_os = "windows"
));

/// Whether the experimental GPU canvas was requested.
///
/// Follows the `EMULSION_GPU*` convention: an environment variable read once,
/// with no settings-UI equivalent.
pub fn enabled() -> bool {
    use std::sync::OnceLock;
    static ON: OnceLock<bool> = OnceLock::new();
    *ON.get_or_init(|| HOSTED && std::env::var("EMULSION_GPU_CANVAS").is_ok_and(|v| v == "1"))
}

/// What the canvas decided, remembered across frames.
#[derive(Default)]
pub enum Status {
    /// Not tried yet: the prepaint may skip CPU tiles and let the paint try.
    #[default]
    Untried,
    /// Engine is live and painting.
    Active(Box<Canvas>),
    /// Engine cannot draw this document or view; use the CPU path.
    Refused,
}

impl Status {
    /// Whether the CPU tile path should stand down this frame.
    ///
    /// `Untried` counts, so the first frame doesn't composite tiles that the
    /// engine is about to make redundant. A refusal is sticky and puts the CPU
    /// path back permanently.
    pub fn defers_to_gpu(&self, view: &View) -> bool {
        enabled() && view.rotation.rem_euclid(360.0) == 0.0 && !matches!(self, Self::Refused)
    }
}

#[cfg(any(target_os = "linux", target_os = "macos", target_os = "windows"))]
pub use hosted::{Canvas, paint};

#[cfg(not(any(target_os = "linux", target_os = "macos", target_os = "windows")))]
pub use unhosted::{Canvas, paint};

/// No GPUI hosting backend on this platform; [`enabled`] is already false.
#[cfg(not(any(target_os = "linux", target_os = "macos", target_os = "windows")))]
mod unhosted {
    use super::{Status, View};

    pub enum Canvas {}

    pub fn paint(
        _status: &mut Status,
        _doc: &emulsion_core::Document,
        _revision: u64,
        _view: &View,
        _bounds: gpui_kit::Bounds<gpui_kit::Pixels>,
        _window: &mut gpui_kit::Window,
    ) -> bool {
        false
    }
}

#[cfg(any(target_os = "linux", target_os = "macos", target_os = "windows"))]
mod hosted {
    use super::Status;
    use crate::viewport::View;
    use emulsion_core::Document;
    use emulsion_engine::gpu::Gpu;
    use emulsion_engine::host::backend;
    use emulsion_engine::vector::VectorSpace;
    use emulsion_engine::{Camera, Engine, Output};
    use gpui_kit::*;
    use std::sync::Arc;

    pub struct Canvas {
        gpu: Arc<Gpu>,
        engine: Engine,
        target: backend::Target,
        /// Document revision the program and atlas were compiled at.
        revision: u64,
    }

    impl Canvas {
        fn build(doc: &Document, size: (u32, u32), revision: u64) -> anyhow::Result<Self> {
            let gpu = backend::device(None)?;
            let engine = Engine::new(gpu.clone(), doc, None, VectorSpace::Srgb, true, true, size)?;
            let unsupported = &engine.canvas.unsupported;
            if !unsupported.is_empty() {
                anyhow::bail!(
                    "document uses unsupported features: {}",
                    unsupported.join("; ")
                );
            }
            let target = backend::Target::new(&gpu, size)?;
            Ok(Self {
                gpu,
                engine,
                target,
                revision,
            })
        }

        fn resize(&mut self, size: (u32, u32)) -> anyhow::Result<()> {
            if self.target.size != size {
                self.target = backend::Target::new(&self.gpu, size)?;
            }
            self.engine.screen = size;
            Ok(())
        }
    }

    /// Render the document into the scene. Returns false when the caller must
    /// fall back to the CPU tile path; `status` is left `Refused` in that case,
    /// so it only costs one frame.
    pub fn paint(
        status: &mut Status,
        doc: &Document,
        revision: u64,
        view: &View,
        bounds: Bounds<Pixels>,
        window: &mut Window,
    ) -> bool {
        if !status.defers_to_gpu(view) {
            return false;
        }
        let scale = window.scale_factor();
        let size = (
            ((f32::from(bounds.size.width) * scale).round() as u32).max(1),
            ((f32::from(bounds.size.height) * scale).round() as u32).max(1),
        );

        // A changed document needs a new program. Rebuild it against the atlas
        // already on the GPU, so unchanged tiles are re-acquired by `Arc`
        // identity rather than re-uploaded; only fall back to a full rebuild if
        // that fails, which usually means the atlas has no room for the new
        // document.
        if let Status::Active(canvas) = status
            && canvas.revision != revision
        {
            match canvas.engine.reload(doc, None, true) {
                Ok(()) => {
                    canvas.revision = revision;
                    tracing::debug!(
                        "gpu canvas reloaded at rev {revision}: {} dirty rect(s), {} atlas tiles",
                        canvas.engine.canvas.dirty.len(),
                        canvas.engine.atlas.used(),
                    );
                }
                Err(err) => {
                    tracing::info!("gpu canvas reload failed, rebuilding: {err:#}");
                    *status = Status::Untried;
                }
            }
        }

        if matches!(status, Status::Untried) {
            match Canvas::build(doc, size, revision) {
                Ok(canvas) => {
                    tracing::info!(
                        "gpu canvas active: {} ({}) at rev {revision}, {}x{} device px, {} MiB textures",
                        canvas.gpu.describe(),
                        backend::presentation(),
                        size.0,
                        size.1,
                        canvas.engine.texture_bytes() / (1024 * 1024),
                    );
                    *status = Status::Active(Box::new(canvas));
                }
                Err(err) => {
                    tracing::info!("gpu canvas unavailable, using the CPU path: {err:#}");
                    *status = Status::Refused;
                    return false;
                }
            }
        }
        let Status::Active(canvas) = status else {
            return false;
        };

        // The previous frame's GPU work must be done before its texture is
        // drawn into again. On macOS the two queues are unordered, so this also
        // waits for GPUI itself.
        backend::wait_for_previous_frame(Some(&canvas.gpu));

        if let Err(err) = canvas.resize(size) {
            tracing::warn!("gpu canvas resize failed, using the CPU path: {err:#}");
            *status = Status::Refused;
            return false;
        }
        canvas.engine.camera = Camera {
            center: [view.center.0, view.center.1],
            zoom: view.device_zoom(scale),
        };
        let target = canvas.target.acquire();
        let frame_start = std::time::Instant::now();
        match canvas
            .engine
            .render(&target, backend::FORMAT, Output::Encoded)
        {
            Ok(times) => {
                let ms = frame_start.elapsed().as_secs_f64() * 1e3;
                if ms > 8.0 {
                    tracing::debug!(
                        total_ms = ms,
                        cpu_ms = times.cpu_ms,
                        vector_encode_ms = times.vector_encode_ms,
                        vector_render_ms = times.vector_render_ms,
                        composite_ms = times.composite_ms,
                        mips_ms = times.mips_ms,
                        cache_fills = times.cache_fills,
                        visible_vectors = times.visible_vectors,
                        cached_ops = canvas.engine.cached_ops(),
                        total_ops = canvas.engine.canvas.ops.len(),
                        runs = canvas.engine.canvas.runs.len(),
                        "slow engine frame"
                    );
                }
            }
            Err(err) => {
                tracing::warn!("gpu canvas render failed, using the CPU path: {err:#}");
                *status = Status::Refused;
                return false;
            }
        }
        backend::after_render(&canvas.gpu);
        canvas.target.paint(window, bounds);
        true
    }
}
