//! Opt-in GPU canvas (experimental), enabled with `EMULSION_GPU_CANVAS=1`.
//!
//! Draws the document with `emulsion-engine` on GPUI's own wgpu device and
//! hands GPUI the resulting texture, instead of compositing 256 px tiles on the
//! CPU and uploading each as an image. See `spikes/vello-canvas/RESULTS.md` for
//! the measurements this is based on.
//!
//! It renders only. Editing still works, but every document change recompiles
//! the engine's program and atlas, because the engine has no incremental
//! structural update yet; strokes are not routed to the GPU brush here.
//!
//! The path refuses whenever it cannot reproduce the CPU result, and the caller
//! falls back to [`crate::viewport`]: a rotated view, a document using features
//! the engine does not implement, or a platform without the shared-device
//! patch. A refusal is sticky, so the fallback costs one frame at most.

use crate::viewport::View;

/// Whether the experimental GPU canvas was requested.
///
/// Follows the `EMULSION_GPU*` convention: an environment variable read once,
/// with no settings-UI equivalent.
pub fn enabled() -> bool {
    use std::sync::OnceLock;
    static ON: OnceLock<bool> = OnceLock::new();
    *ON.get_or_init(|| {
        cfg!(target_os = "linux") && std::env::var("EMULSION_GPU_CANVAS").is_ok_and(|v| v == "1")
    })
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

#[cfg(target_os = "linux")]
pub use linux::{Canvas, paint};

#[cfg(not(target_os = "linux"))]
pub use stub::Canvas;

#[cfg(not(target_os = "linux"))]
mod stub {
    /// Placeholder so `Status` has one shape on every platform; never built,
    /// because [`super::enabled`] is false off Linux.
    pub enum Canvas {}
}

#[cfg(target_os = "linux")]
mod linux {
    use super::Status;
    use crate::viewport::View;
    use emulsion_core::Document;
    use emulsion_engine::gpu::Gpu;
    use emulsion_engine::vector::VectorSpace;
    use emulsion_engine::{Camera, Engine, Output};
    use gpui_kit::*;
    use std::sync::Arc;

    /// GPUI's Linux renderer composites this format without conversion.
    const FORMAT: wgpu::TextureFormat = wgpu::TextureFormat::Rgba8Unorm;

    pub struct Canvas {
        gpu: Arc<Gpu>,
        engine: Engine,
        target: wgpu::TextureView,
        size: (u32, u32),
        /// Document revision the program and atlas were compiled at.
        revision: u64,
    }

    impl Canvas {
        fn build(doc: &Document, size: (u32, u32), revision: u64) -> anyhow::Result<Self> {
            let shared = gpui_wgpu::shared_gpu()
                .ok_or_else(|| anyhow::anyhow!("GPUI has not published its wgpu device"))?;
            let gpu = Gpu::from_shared(&shared, None);
            let engine = Engine::new(gpu.clone(), doc, None, VectorSpace::Srgb, true, true, size)?;
            let unsupported = &engine.canvas.unsupported;
            if !unsupported.is_empty() {
                anyhow::bail!(
                    "document uses unsupported features: {}",
                    unsupported.join("; ")
                );
            }
            let target = make_target(&gpu, size);
            Ok(Self {
                gpu,
                engine,
                target,
                size,
                revision,
            })
        }

        fn resize(&mut self, size: (u32, u32)) {
            if self.size != size {
                self.target = make_target(&self.gpu, size);
                self.size = size;
            }
            self.engine.screen = size;
        }
    }

    fn make_target(gpu: &Gpu, size: (u32, u32)) -> wgpu::TextureView {
        gpu.device
            .create_texture(&wgpu::TextureDescriptor {
                label: Some("emulsion gpu canvas"),
                size: wgpu::Extent3d {
                    width: size.0,
                    height: size.1,
                    depth_or_array_layers: 1,
                },
                mip_level_count: 1,
                sample_count: 1,
                dimension: wgpu::TextureDimension::D2,
                format: FORMAT,
                usage: wgpu::TextureUsages::RENDER_ATTACHMENT
                    | wgpu::TextureUsages::TEXTURE_BINDING,
                view_formats: &[],
            })
            .create_view(&Default::default())
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

        // Any document change recompiles: the engine can replace tiles of an
        // existing raster, but not a changed node graph.
        let stale = matches!(status, Status::Active(c) if c.revision != revision);
        if matches!(status, Status::Untried) || stale {
            match Canvas::build(doc, size, revision) {
                Ok(canvas) => {
                    tracing::info!(
                        "gpu canvas active: {} at rev {revision}, {}x{} device px, {} MiB textures",
                        canvas.gpu.describe(),
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

        canvas.resize(size);
        canvas.engine.camera = Camera {
            center: [view.center.0, view.center.1],
            zoom: view.device_zoom(scale),
        };
        let target = canvas.target.clone();
        if let Err(err) = canvas.engine.render(&target, FORMAT, Output::Encoded) {
            tracing::warn!("gpu canvas render failed, using the CPU path: {err:#}");
            *status = Status::Refused;
            return false;
        }
        window.paint_external_texture(bounds, ExternalTexture(Arc::new(target)));
        true
    }
}

#[cfg(not(target_os = "linux"))]
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
