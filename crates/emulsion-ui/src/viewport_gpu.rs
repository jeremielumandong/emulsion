//! Experimental GPU canvas, enabled by default; `EMULSION_GPU_CANVAS=0` disables it.
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
//! Pixel edits update existing atlas sources; structural changes retain
//! unchanged tiles. Compatible brush strokes render into the atlas and read
//! back once at commit, with a replay journal for CPU recovery.
//!
//! The path refuses whenever it cannot reproduce the CPU result, and the caller
//! falls back to [`crate::viewport`]: a rotated view, a document using features
//! the engine does not implement, or a host that cannot supply a device. A
//! refusal is retried after document/device changes; transient failures back off.

use crate::viewport::View;

/// Platforms with a GPUI hosting backend.
const HOSTED: bool = cfg!(any(
    target_os = "linux",
    target_os = "macos",
    target_os = "windows"
));

/// Whether the GPU canvas should draw.
///
/// On wherever there is a hosting backend; `EMULSION_GPU_CANVAS=0` falls back
/// to the CPU tile path. Follows the `EMULSION_GPU*` convention: an
/// environment variable read once, with no settings-UI equivalent.
///
/// The path refuses on its own for anything it cannot draw faithfully, so a
/// document it does not support still renders -- see [`Status`].
pub fn enabled() -> bool {
    use std::sync::OnceLock;
    static ON: OnceLock<bool> = OnceLock::new();
    *ON.get_or_init(|| {
        // Never under test: the unit tests drive GPUI headless, with no
        // surface to present and no frame loop to drive the engine, so a
        // canvas that renders on the GPU there just hangs them.
        HOSTED
            && !cfg!(test)
            && match std::env::var("EMULSION_GPU_CANVAS") {
                Ok(v) => v != "0",
                Err(_) => true,
            }
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
    Refused {
        reason: String,
        revision: u64,
        generation: Option<u64>,
        retry_at: Option<std::time::Instant>,
    },
}

fn device_generation() -> Option<u64> {
    #[cfg(target_os = "linux")]
    {
        gpui_wgpu::shared_gpu().map(|shared| shared.generation)
    }
    #[cfg(not(target_os = "linux"))]
    {
        None
    }
}

impl Status {
    #[cfg(feature = "canvas-bench")]
    pub(crate) fn texture_bytes(&self) -> u64 {
        match self {
            #[cfg(any(target_os = "linux", target_os = "macos", target_os = "windows"))]
            Self::Active(canvas) => canvas.engine.texture_bytes(),
            _ => 0,
        }
    }

    fn retry_due(&self, revision: u64, generation: Option<u64>, now: std::time::Instant) -> bool {
        match self {
            Self::Refused {
                revision: old,
                generation: previous,
                retry_at,
                ..
            } => {
                *previous != generation
                    || match retry_at {
                        // A drag changes revision every frame. Respect the backoff
                        // across those revisions instead of rebuilding GPU targets.
                        Some(at) => now >= *at,
                        None => *old != revision,
                    }
            }
            _ => true,
        }
    }

    fn refuse(&mut self, reason: String, revision: u64, transient: bool) {
        *self = Self::Refused {
            reason,
            revision,
            generation: device_generation(),
            retry_at: transient
                .then(|| std::time::Instant::now() + std::time::Duration::from_secs(2)),
        };
    }

    pub fn defers_to_gpu(&self, view: &View, revision: u64) -> bool {
        enabled()
            && view.rotation.rem_euclid(360.0) == 0.0
            && self.retry_due(revision, device_generation(), std::time::Instant::now())
    }

    pub fn renderer_notice(&self, view: &View) -> Option<(&'static str, String)> {
        if !enabled() {
            return None;
        }
        if view.rotation.rem_euclid(360.0) != 0.0 {
            return Some(("CPU canvas", "Rotated views use CPU rendering.".into()));
        }
        match self {
            Self::Refused { reason, .. } => Some(("CPU canvas", reason.clone())),
            #[cfg(any(target_os = "linux", target_os = "macos", target_os = "windows"))]
            Self::Active(canvas) if !canvas.engine.canvas.rasterized.is_empty() => Some((
                "Compatibility rendering",
                canvas.engine.canvas.rasterized.join("; "),
            )),
            _ => None,
        }
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

    impl Status {
        pub(crate) fn begin_brush(
            &mut self,
            _: &emulsion_core::Document,
            _: u64,
            _: emulsion_core::NodeId,
            _: emulsion_raster::paint::Brush,
            _: &emulsion_raster::paint::Ink,
        ) -> bool {
            false
        }
        pub(crate) fn brush_point(&mut self, _: emulsion_core::NodeId, _: f32, _: f32) {}
        pub(crate) fn brush_alive(&self, _: emulsion_core::NodeId) -> bool {
            false
        }
        pub(crate) fn flush_brush(&mut self, _: emulsion_core::NodeId) -> anyhow::Result<()> {
            anyhow::bail!("GPU brush unavailable")
        }
        pub(crate) fn finish_brush(
            &mut self,
            _: emulsion_core::NodeId,
        ) -> anyhow::Result<std::sync::Arc<emulsion_raster::Raster>> {
            anyhow::bail!("GPU brush unavailable")
        }
        pub(crate) fn cancel_brush(&mut self) {}
    }

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

    struct LiveBrush {
        node: emulsion_core::NodeId,
        source: usize,
        stroke: emulsion_engine::brush::GpuStroke,
    }

    pub struct Canvas {
        live_brush: Option<LiveBrush>,
        gpu: Arc<Gpu>,
        pub(super) engine: Engine,
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
                live_brush: None,
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

    impl Status {
        pub(crate) fn begin_brush(
            &mut self,
            doc: &Document,
            revision: u64,
            node: emulsion_core::NodeId,
            brush: emulsion_raster::paint::Brush,
            ink: &emulsion_raster::paint::Ink,
        ) -> bool {
            if !super::enabled() || !emulsion_engine::brush::supports_brush(brush, ink) {
                return false;
            }
            let Self::Active(canvas) = self else {
                return false;
            };
            if canvas.live_brush.is_some() || canvas.gpu.is_lost() {
                return false;
            }
            if canvas.revision != revision {
                if canvas.engine.reload(doc, None, true).is_err()
                    || !canvas.engine.canvas.unsupported.is_empty()
                {
                    *self = Self::Untried;
                    return false;
                }
                canvas.revision = revision;
            }
            let Some(source) = canvas
                .engine
                .canvas
                .sources
                .iter()
                .position(|source| source.node == Some(node))
            else {
                return false;
            };
            let (color, erase) = match ink {
                emulsion_raster::paint::Ink::Color(color) => (*color, false),
                emulsion_raster::paint::Ink::Erase => ([0.0, 0.0, 0.0, 1.0], true),
                _ => return false,
            };
            canvas.live_brush = Some(LiveBrush {
                node,
                source,
                stroke: emulsion_engine::brush::GpuStroke::with_ink(source, brush, color, erase),
            });
            true
        }

        pub(crate) fn brush_point(&mut self, node: emulsion_core::NodeId, x: f32, y: f32) {
            if let Self::Active(canvas) = self
                && let Some(live) = canvas.live_brush.as_mut().filter(|live| live.node == node)
            {
                live.stroke.point(x, y);
            }
        }

        pub(crate) fn brush_alive(&self, node: emulsion_core::NodeId) -> bool {
            matches!(self, Self::Active(canvas) if !canvas.gpu.is_lost()
                && canvas.live_brush.as_ref().is_some_and(|live| live.node == node))
        }

        pub(crate) fn flush_brush(&mut self, node: emulsion_core::NodeId) -> anyhow::Result<()> {
            let Self::Active(canvas) = self else {
                anyhow::bail!("GPU brush unavailable");
            };
            canvas.gpu.ensure_alive()?;
            let live = canvas
                .live_brush
                .as_mut()
                .filter(|live| live.node == node)
                .ok_or_else(|| anyhow::anyhow!("GPU brush interrupted"))?;
            let (brush, document, atlas, encoder) = canvas.engine.brush_parts();
            live.stroke.render(brush, document, atlas, encoder)?;
            // Each upload of the reusable dab buffer must have its own submit.
            canvas.engine.flush();
            Ok(())
        }

        pub(crate) fn finish_brush(
            &mut self,
            node: emulsion_core::NodeId,
        ) -> anyhow::Result<Arc<emulsion_raster::Raster>> {
            self.flush_brush(node)?;
            let Self::Active(canvas) = self else {
                unreachable!()
            };
            let live = canvas.live_brush.take().expect("flushed brush");
            if !live.stroke.has_tiles() {
                return Ok(canvas.engine.canvas.sources[live.source].raster.clone());
            }
            let (_, _, atlas, encoder) = canvas.engine.brush_parts();
            let readback = live.stroke.finish(atlas, encoder);
            canvas.engine.flush();
            readback.map();
            canvas.gpu.wait();
            readback.complete(
                &canvas.gpu,
                &mut canvas.engine.canvas,
                &mut canvas.engine.atlas,
                live.source,
            )
        }

        pub(crate) fn cancel_brush(&mut self) {
            if matches!(self, Self::Active(canvas) if canvas.live_brush.is_some()) {
                // The CPU document still holds the pre-stroke pixels. Rebuild
                // from it; never let detached painted slots survive cancellation.
                *self = Self::Untried;
            }
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
        if !status.defers_to_gpu(view, revision) {
            return false;
        }
        if matches!(status, Status::Refused { .. })
            || matches!(status, Status::Active(canvas) if canvas.gpu.is_lost())
        {
            *status = Status::Untried;
        }
        let scale = window.scale_factor();
        let size = (
            ((f32::from(bounds.size.width) * scale).round() as u32).max(1),
            ((f32::from(bounds.size.height) * scale).round() as u32).max(1),
        );

        if matches!(status, Status::Active(canvas) if canvas.live_brush.is_some() && canvas.revision != revision)
        {
            *status = Status::Untried;
        }
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
                    if !canvas.engine.canvas.unsupported.is_empty() {
                        let reason = format!(
                            "CPU rendering: {}",
                            canvas.engine.canvas.unsupported.join("; ")
                        );
                        status.refuse(reason, revision, false);
                        return false;
                    }
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
                    let reason = format!("{err:#}");
                    let transient = !reason.starts_with("document uses unsupported features:");
                    status.refuse(reason, revision, transient);
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
            status.refuse(format!("{err:#}"), revision, true);
            return false;
        }
        if canvas.gpu.is_lost() {
            status.refuse("Graphics device is recovering.".into(), revision, true);
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
                status.refuse(format!("{err:#}"), revision, true);
                return false;
            }
        }
        backend::after_render(&canvas.gpu);
        if canvas.gpu.is_lost() {
            status.refuse("Graphics device is recovering.".into(), revision, true);
            return false;
        }
        // The shared engine includes its benchmark stage outside the document.
        // Keep that area owned by GPUI so dark/light/external themes match the
        // CPU canvas. The hosted GPU path only accepts unrotated views.
        let origin = view.doc_to_screen((0., 0.), &bounds);
        let document = gpui_kit::Bounds::new(
            gpui_kit::point(gpui_kit::px(origin.0 as f32), gpui_kit::px(origin.1 as f32)),
            gpui_kit::size(
                gpui_kit::px((doc.width as f64 * view.zoom) as f32),
                gpui_kit::px((doc.height as f64 * view.zoom) as f32),
            ),
        );
        window.with_content_mask(
            Some(gpui_kit::ContentMask {
                bounds: document.intersect(&bounds),
            }),
            |window| {
                canvas.target.paint(window, bounds);
            },
        );
        true
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::time::{Duration, Instant};

    #[test]
    fn unsupported_documents_retry_only_after_a_change_or_device_replacement() {
        let now = Instant::now();
        let status = Status::Refused {
            reason: "adjustment layer".into(),
            revision: 7,
            generation: Some(1),
            retry_at: None,
        };
        assert!(!status.retry_due(7, Some(1), now + Duration::from_secs(10)));
        assert!(status.retry_due(8, Some(1), now));
        assert!(status.retry_due(7, Some(2), now));
    }

    #[test]
    fn transient_failures_back_off_and_then_retry_without_a_document_edit() {
        let now = Instant::now();
        let status = Status::Refused {
            reason: "device recovering".into(),
            revision: 7,
            generation: None,
            retry_at: Some(now + Duration::from_secs(2)),
        };
        assert!(!status.retry_due(7, None, now));
        assert!(!status.retry_due(8, None, now));
        assert!(status.retry_due(8, Some(1), now));
        assert!(status.retry_due(7, None, now + Duration::from_secs(2)));
    }
}
