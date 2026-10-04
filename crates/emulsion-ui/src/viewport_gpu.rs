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
        previous: Option<Box<PreviousFrame>>,
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

    pub fn defers_to_gpu(&self, view: &View, revision: u64) -> bool {
        enabled()
            && view.upright()
            && self.retry_due(revision, device_generation(), std::time::Instant::now())
    }

    pub fn renderer_notice(&self, view: &View) -> Option<(&'static str, String)> {
        if !enabled() {
            return None;
        }
        if !view.upright() {
            return Some((
                "CPU canvas",
                "Rotated and flipped views use CPU rendering.".into(),
            ));
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
pub use hosted::{Canvas, PreviousFrame, paint, paint_previous};

#[cfg(not(any(target_os = "linux", target_os = "macos", target_os = "windows")))]
pub use unhosted::{Canvas, PreviousFrame, paint, paint_previous};

/// No GPUI hosting backend on this platform; [`enabled`] is already false.
#[cfg(not(any(target_os = "linux", target_os = "macos", target_os = "windows")))]
mod unhosted {
    use super::{Status, View};

    pub enum Canvas {}
    pub enum PreviousFrame {}

    pub fn paint_previous(
        _: &mut Status,
        _: &View,
        _: gpui_kit::Bounds<gpui_kit::Pixels>,
        _: Option<u64>,
        _: Option<u64>,
        _: &mut gpui_kit::Window,
    ) -> bool {
        false
    }

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
        last_frame: Option<(View, (u32, u32))>,
    }

    /// Retain just the presented texture while CPU tiles arrive, not the engine/atlas.
    pub struct PreviousFrame {
        gpu: Arc<Gpu>,
        target: backend::Target,
        view: View,
        document_size: (u32, u32),
        cpu_revision: Option<u64>,
    }

    impl PreviousFrame {
        fn covered(&mut self, requested: u64, oldest: Option<u64>) -> bool {
            // Progress may finish a few drag revisions behind the pointer. A
            // complete CPU frame newer than this handoff is enough to resume it.
            let first = *self.cpu_revision.get_or_insert(requested);
            oldest.is_some_and(|revision| revision >= first)
        }
    }

    impl Canvas {
        fn build(doc: &Document, size: (u32, u32), revision: u64) -> anyhow::Result<Self> {
            let gpu = backend::device(None)?;
            Self::build_on(gpu, doc, size, revision)
        }

        fn build_on(
            gpu: Arc<Gpu>,
            doc: &Document,
            size: (u32, u32),
            revision: u64,
        ) -> anyhow::Result<Self> {
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
                last_frame: None,
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
        fn refuse(&mut self, reason: String, revision: u64, transient: bool) {
            let previous = match std::mem::take(self) {
                Self::Active(canvas) if !transient && !canvas.gpu.is_lost() => {
                    let Canvas {
                        gpu,
                        target,
                        last_frame,
                        ..
                    } = *canvas;
                    last_frame.map(|(view, document_size)| {
                        Box::new(PreviousFrame {
                            gpu,
                            target,
                            view,
                            document_size,
                            cpu_revision: None,
                        })
                    })
                }
                Self::Refused { previous, .. } => previous,
                _ => None,
            };
            *self = Self::Refused {
                reason,
                revision,
                previous,
                generation: super::device_generation(),
                retry_at: transient
                    .then(|| std::time::Instant::now() + std::time::Duration::from_secs(2)),
            };
        }

        /// Reload the current GPU document. False hands this frame to CPU;
        /// true either keeps the GPU ready or permits the ordinary rebuild path.
        fn reload_document(&mut self, doc: &Document, revision: u64) -> bool {
            if let Self::Active(canvas) = self
                && canvas.revision != revision
            {
                match canvas.engine.reload(doc, None, true) {
                    Ok(()) => {
                        if !canvas.engine.canvas.unsupported.is_empty() {
                            let reason = format!(
                                "CPU rendering: {}",
                                canvas.engine.canvas.unsupported.join("; ")
                            );
                            self.refuse(reason, revision, false);
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
                        // A larger inserted picture can exhaust the old atlas. Keep
                        // its last frame if rebuilding also needs a CPU fallback.
                        self.refuse(
                            format!("GPU canvas reload failed: {err:#}"),
                            revision,
                            false,
                        );
                    }
                }
            }
            true
        }

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
    /// paint the CPU fallback in the same frame. A retained successful target
    /// bridges the handoff while the first replacement CPU tiles are prepared.
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
        if matches!(status, Status::Active(canvas) if canvas.gpu.is_lost()) {
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
        if !status.reload_document(doc, revision) {
            return false;
        }

        if matches!(status, Status::Untried | Status::Refused { .. }) {
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
        canvas.last_frame = Some((*view, (doc.width, doc.height)));
        paint_target(
            &canvas.target,
            *view,
            (doc.width, doc.height),
            bounds,
            window,
        );
        true
    }

    /// Fill holes during a GPU-to-CPU handoff with the last successful frame.
    /// Cached CPU tiles replace it immediately, and release it once coverage is complete.
    pub fn paint_previous(
        status: &mut Status,
        view: &View,
        bounds: Bounds<Pixels>,
        covered_revision: Option<u64>,
        requested_revision: Option<u64>,
        window: &mut Window,
    ) -> bool {
        let Status::Refused {
            previous,
            generation,
            ..
        } = status
        else {
            return false;
        };
        let scale = window.scale_factor();
        let size = (
            ((f32::from(bounds.size.width) * scale).round() as u32).max(1),
            ((f32::from(bounds.size.height) * scale).round() as u32).max(1),
        );
        if requested_revision.is_none()
            || previous
                .as_mut()
                .is_some_and(|frame| frame.covered(requested_revision.unwrap(), covered_revision))
            || *generation != super::device_generation()
            || previous.as_ref().is_some_and(|frame| {
                frame.gpu.is_lost() || frame.view != *view || frame.target.size != size
            })
        {
            *previous = None;
        }
        if let Some(frame) = previous {
            paint_target(
                &frame.target,
                frame.view,
                frame.document_size,
                bounds,
                window,
            );
            return true;
        }
        false
    }

    fn paint_target(
        target: &backend::Target,
        view: View,
        document_size: (u32, u32),
        bounds: Bounds<Pixels>,
        window: &mut Window,
    ) {
        // The engine's benchmark stage must never replace the editor's theme.
        let origin = view.doc_to_screen((0., 0.), &bounds);
        let document = Bounds::new(
            point(px(origin.0 as f32), px(origin.1 as f32)),
            size(
                px((document_size.0 as f64 * view.zoom) as f32),
                px((document_size.1 as f64 * view.zoom) as f32),
            ),
        );
        window.with_content_mask(
            Some(ContentMask {
                bounds: document.intersect(&bounds),
            }),
            |window| target.paint(window, bounds),
        );
    }
    #[cfg(test)]
    mod grouped_clip_handoff_tests {
        //! Exercise the actual hosted build/reload/refusal decisions. Headless GPUI
        //! intentionally disables paint(), so this ignored test requires a real device.
        use super::{Arc, Camera, Canvas, Document, Gpu, Output, Status, View, backend};
        use emulsion_core::{Command, Node};
        use emulsion_raster::blend::BlendSpace;
        use emulsion_raster::composite::render_tile_cpu;
        use emulsion_raster::{BlendMode, Placement, Raster, TileCoord};

        fn close(actual: [f32; 4], expected: [f32; 4]) {
            for (a, b) in actual.into_iter().zip(expected) {
                assert!((a - b).abs() < 6e-5, "{actual:?} != {expected:?}");
            }
        }

        fn cpu(doc: &Document) -> [f32; 4] {
            render_tile_cpu(&doc.composite_tree(), 0, TileCoord::new(0, 0))[0]
        }

        fn gpu_pixel(canvas: &mut Canvas) -> [f32; 4] {
            canvas.engine.camera = Camera {
                center: [8., 8.],
                zoom: 1.,
            };
            let output = emulsion_engine::Offscreen::new(
                &canvas.gpu,
                (16, 16),
                wgpu::TextureFormat::Rgba32Float,
            );
            canvas
                .engine
                .render(&output.view, output.format, Output::Raw)
                .unwrap();
            let bytes = output.read(&canvas.gpu).unwrap();
            std::array::from_fn(|c| f32::from_le_bytes(bytes[c * 4..c * 4 + 4].try_into().unwrap()))
        }

        fn present(canvas: &mut Canvas) {
            canvas
                .engine
                .render(&canvas.target.acquire(), backend::FORMAT, Output::Encoded)
                .unwrap();
            canvas.gpu.wait();
            canvas.last_frame = Some((
                View {
                    center: (8., 8.),
                    zoom: 1.,
                    rotation: 0.,
                    ..Default::default()
                },
                (16, 16),
            ));
        }

        #[test]
        #[ignore = "Requires a host GPU; run serially with EMULSION_REQUIRE_GPU_TESTS=1"]
        fn grouped_clip_hosted_creation_reload_release_and_restore_never_keep_stale_gpu_pixels() {
            let gpu =
                Gpu::new(emulsion_engine::gpu::instance(), None, None).expect("host GPU required");
            let mut doc = Document::new(16, 16);
            doc.blend_space = BlendSpace::Linear;
            for (id, name, color) in [
                (1, "Backdrop", [0.5, 0.25, 0.75, 1.0]),
                (2, "Multiply base", [0.2, 0.4, 0.6, 1.0]),
                (3, "Clip member", [0.8, 0.2, 0.4, 1.0]),
            ] {
                doc.nodes.push(Node::raster(
                    id,
                    name,
                    Arc::new(Raster::solid(16, 16, color)),
                    Placement::default(),
                ));
            }
            doc.node_mut(2).unwrap().blend = BlendMode::Multiply;
            let mut canvas = Canvas::build_on(gpu.clone(), &doc, (16, 16), 1).unwrap();
            assert!(canvas.engine.canvas.unsupported.is_empty());
            close(gpu_pixel(&mut canvas), [0.8, 0.2, 0.4, 1.0]);
            present(&mut canvas);
            let mut status = Status::Active(Box::new(canvas));

            Command::SetClip {
                id: 3,
                clip_to: Some(2),
            }
            .apply(&mut doc)
            .unwrap();
            let grouped = doc.clone();
            assert!(
                !status.reload_document(&doc, 2),
                "production reload must hand grouped scene to CPU"
            );
            let previous_ptr = match &status {
                Status::Refused {
                    reason,
                    revision: 2,
                    previous: Some(previous),
                    ..
                } => {
                    assert!(reason.contains("grouped clipping requires the CPU compositor"));
                    std::ptr::from_ref(previous.as_ref())
                }
                _ => panic!("missing hosted GPU-to-CPU handoff"),
            };
            close(cpu(&doc), [0.4, 0.05, 0.3, 1.0]);
            let initial_load = Canvas::build_on(gpu.clone(), &doc, (16, 16), 2)
                .err()
                .expect("initial grouped load must use CPU");
            assert!(initial_load.to_string().contains("grouped clipping"));

            doc.node_mut(2).unwrap().opacity = 0.5;
            let retry = Canvas::build_on(gpu.clone(), &doc, (16, 16), 3)
                .err()
                .expect("changed grouped scene remains ineligible");
            status.refuse(retry.to_string(), 3, false);
            close(cpu(&doc), [0.45, 0.15, 0.525, 1.0]);
            let Status::Refused {
                previous: Some(previous),
                ..
            } = &mut status
            else {
                panic!("lost previous frame")
            };
            assert_eq!(std::ptr::from_ref(previous.as_ref()), previous_ptr);
            assert!(
                !previous.covered(3, Some(2)),
                "stale CPU tiles must not end the handoff"
            );
            assert!(
                previous.covered(3, Some(3)),
                "fresh CPU pixels must replace retained GPU frame"
            );

            Command::SetClip {
                id: 3,
                clip_to: None,
            }
            .apply(&mut doc)
            .unwrap();
            let mut released = Canvas::build_on(gpu.clone(), &doc, (16, 16), 4).unwrap();
            assert!(released.engine.canvas.unsupported.is_empty());
            close(gpu_pixel(&mut released), [0.8, 0.2, 0.4, 1.0]);
            present(&mut released);
            status = Status::Active(Box::new(released));
            assert!(
                !status.reload_document(&grouped, 5),
                "restoring clipping must refuse the new GPU program"
            );
            close(cpu(&grouped), [0.4, 0.05, 0.3, 1.0]);
            assert!(matches!(
                status,
                Status::Refused {
                    revision: 5,
                    previous: Some(_),
                    ..
                }
            ));
        }
    }

    #[cfg(test)]
    mod gpu_handoff_tests {
        use super::{
            Arc, Camera, Canvas, Document, Engine, Gpu, Output, Status, VectorSpace, View, backend,
        };

        #[test]
        #[ignore = "Requires a host GPU; run serially"]
        fn picture_edit_keeps_last_gpu_target_across_fallback_retries() {
            use emulsion_core::{Command, Node, command::Slot};
            use emulsion_raster::{BlendMode, Placement, Raster};
            let gpu = Gpu::new(emulsion_engine::gpu::instance(), None, None)
                .expect("host GPU required for canvas handoff regression");
            let mut doc = Document::new(64, 64);
            Command::AddNode {
                node: Box::new(Node::raster(
                    0,
                    "Background",
                    Arc::new(Raster::solid(64, 64, [0., 0., 1., 1.])),
                    Placement::default(),
                )),
                slot: Slot::TOP,
            }
            .apply(&mut doc)
            .unwrap();
            let mut engine = Engine::new(
                gpu.clone(),
                &doc,
                None,
                VectorSpace::Srgb,
                true,
                true,
                (64, 64),
            )
            .unwrap();
            assert!(engine.canvas.unsupported.is_empty());
            engine.camera = Camera {
                center: [32., 32.],
                zoom: 1.,
            };
            let output =
                emulsion_engine::Offscreen::new(&gpu, (64, 64), wgpu::TextureFormat::Rgba8Unorm);
            engine
                .render(&output.view, output.format, Output::Raw)
                .unwrap();
            let backdrop = output.read(&gpu).unwrap();
            let picture = Command::AddNode {
                node: Box::new(Node::raster(
                    0,
                    "Picture",
                    Arc::new(Raster::solid(32, 32, [1., 0., 0., 1.])),
                    Placement::default(),
                )),
                slot: Slot::TOP,
            }
            .apply(&mut doc)
            .unwrap()
            .unwrap();
            // Insertion, pixel replacement and continuous movement must keep
            // the unchanged background and current picture on every GPU frame.
            for step in 0..6 {
                let color = if step % 2 == 0 {
                    [1., 0., 0., 1.]
                } else {
                    [0., 1., 0., 1.]
                };
                let emulsion_core::NodeKind::Raster { raster, .. } =
                    &mut doc.nodes.iter_mut().find(|n| n.id == picture).unwrap().kind
                else {
                    unreachable!()
                };
                *raster = Arc::new(Raster::solid(32, 32, color));
                Command::TranslateNode {
                    id: picture,
                    dx: 2.,
                    dy: 0.,
                }
                .apply(&mut doc)
                .unwrap();
                engine.reload(&doc, None, true).unwrap();
                engine
                    .render(&output.view, output.format, Output::Raw)
                    .unwrap();
                let pixels = output.read(&gpu).unwrap();
                let corner = (63 * 64 + 63) * 4;
                assert_eq!(
                    &pixels[corner..corner + 4],
                    &backdrop[corner..corner + 4],
                    "background changed during picture edit {step}"
                );
                let center = (20 * 64 + 20) * 4;
                let expected = if step % 2 == 0 {
                    [255, 0, 0, 255]
                } else {
                    [0, 255, 0, 255]
                };
                assert_eq!(
                    &pixels[center..center + 4],
                    &expected,
                    "picture update missing at {step}"
                );
            }
            let mut target = backend::Target::new(&gpu, (64, 64)).unwrap();
            engine
                .render(&target.acquire(), backend::FORMAT, Output::Encoded)
                .unwrap();
            gpu.wait();
            let view = View {
                center: (32., 32.),
                zoom: 1.,
                rotation: 0.,
                ..Default::default()
            };
            let mut canvas = Canvas {
                gpu,
                engine,
                target,
                revision: 1,
                live_brush: None,
                last_frame: Some((view, (64, 64))),
            };
            // A real unsupported appearance on an inserted picture triggers the
            // same reload refusal as the hosted editor, with a successful image
            // already on the target. The old refusal discarded that target.
            doc.nodes
                .iter_mut()
                .find(|n| n.id == picture)
                .unwrap()
                .blend = BlendMode::Dissolve;
            canvas.engine.reload(&doc, None, true).unwrap();
            assert!(!canvas.engine.canvas.unsupported.is_empty());
            let mut status = Status::Active(Box::new(canvas));
            status.refuse("picture blending requires CPU".into(), 2, false);
            let frame = match &status {
                Status::Refused {
                    previous: Some(previous),
                    ..
                } => {
                    assert_eq!(previous.view, view);
                    assert_eq!(previous.document_size, (64, 64));
                    assert_eq!(previous.target.size, (64, 64));
                    std::ptr::from_ref(previous.as_ref())
                }
                _ => panic!("GPU-to-CPU handoff dropped the last presented picture"),
            };
            for revision in 3..8 {
                status.refuse("picture blending requires CPU".into(), revision, false);
                let Status::Refused {
                    previous: Some(previous),
                    ..
                } = &status
                else {
                    panic!("a drag retry discarded the picture before CPU tiles arrived");
                };
                assert_eq!(std::ptr::from_ref(previous.as_ref()), frame);
            }
            let Status::Refused {
                previous: Some(previous),
                ..
            } = &mut status
            else {
                unreachable!()
            };
            assert!(!previous.covered(10, None));
            assert!(
                !previous.covered(11, Some(9)),
                "old cached tiles would flash an obsolete picture"
            );
            assert!(
                previous.covered(12, Some(10)),
                "completed CPU progress must take over during a continuous drag"
            );
        }
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
            previous: None,
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
            previous: None,
        };
        assert!(!status.retry_due(7, None, now));
        assert!(!status.retry_due(8, None, now));
        assert!(status.retry_due(8, Some(1), now));
        assert!(status.retry_due(7, None, now + Duration::from_secs(2)));
    }
}
