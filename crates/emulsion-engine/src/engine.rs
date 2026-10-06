//! One document on one device: atlas, compiled canvas, Vello layer, brush
//! pipelines, and the per-frame sequence (mips → Vello → composite → submit).

use crate::atlas::Atlas;
use crate::brush::GpuBrush;
use crate::cache::CompositeCache;
use crate::canvas::Canvas;
use crate::compositor::{Camera, Compositor, ViewUniform};
use crate::gpu::Gpu;
use crate::vector::{VectorLayer, VectorSpace};
use emulsion_core::{Document, NodeId};
use std::sync::Arc;
use std::time::Instant;

/// Where the composite pass writes.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Output {
    /// 8-bit surface; the shader sRGB-encodes.
    Encoded,
    /// An `*Srgb` surface; hardware encodes.
    HardwareSrgb,
    /// Premultiplied linear f32 for fidelity readback.
    Raw,
}

#[derive(Clone, Copy, Debug, Default)]
pub struct FrameTimes {
    pub cpu_ms: f64,
    pub mips_ms: f64,
    pub vector_encode_ms: f64,
    pub vector_render_ms: f64,
    pub composite_ms: f64,
    pub mip_tiles: usize,
    pub cache_fills: usize,
    pub uploaded_bytes: u64,
    pub visible_vectors: usize,
    pub vector_runs: usize,
}

pub struct Engine {
    pub gpu: Arc<Gpu>,
    pub atlas: Atlas,
    pub canvas: Canvas,
    pub compositor: Compositor,
    pub vectors: VectorLayer,
    pub brush: GpuBrush,
    pub camera: Camera,
    pub screen: (u32, u32),
    pub hud: Vec<String>,
    /// Flattened composite tiles for the program's cacheable prefix, or
    /// `None` to recomposite every layer every frame.
    pub cache: Option<CompositeCache>,
    cached_ops: u32,
    cache_enabled: bool,
    pending: Option<wgpu::CommandEncoder>,
    /// The document structure this program was compiled from, so a reload can
    /// tell a pixel edit from a change that needs a new program.
    signature: Vec<crate::canvas::NodeSig>,
    diagram_doc: Option<Document>,
    vello: bool,
    paint_node: Option<NodeId>,
}

/// Enough slots for every visible tile, including a partially visible border.
/// Above 128 MiB the engine composites directly instead of caching a partial
/// viewport (which would otherwise leave holes in the rendered document).
fn cache_slots(document: (u32, u32), screen: (u32, u32), camera: Camera) -> u32 {
    let span = (emulsion_raster::TILE << camera.level()) as f64;
    let columns = ((screen.0 as f64 / (camera.zoom * span)).ceil() as u32).saturating_add(1);
    let rows = ((screen.1 as f64 / (camera.zoom * span)).ceil() as u32).saturating_add(1);
    let (dx, dy) = emulsion_raster::composite::tiles_at(document.0, document.1, camera.level());
    columns
        .min(dx as u32)
        .saturating_mul(rows.min(dy as u32))
        .max(1)
}

impl Engine {
    pub fn new(
        gpu: Arc<Gpu>,
        doc: &Document,
        paint_node: Option<NodeId>,
        space: VectorSpace,
        vello: bool,
        cache: bool,
        screen: (u32, u32),
    ) -> anyhow::Result<Self> {
        crate::canvas::ensure_blend_space_supported(doc.blend_space)?;
        gpu.ensure_alive()?;
        let t = Instant::now();
        // Room for painting a full layer's worth of new tiles.
        let headroom =
            if paint_node.is_none() && vello && crate::canvas::diagram_vector_supported(doc) {
                1 // Native diagram vectors need no document-sized raster paint reserve.
            } else {
                doc.width.div_ceil(256) * doc.height.div_ceil(256) + 64
            };
        let (canvas, atlas) = Canvas::compile(doc, &gpu, paint_node, headroom, vello)?;
        let mut compositor = Compositor::new(gpu.clone());
        compositor.set_program(&canvas);
        let vectors = VectorLayer::new(gpu.clone(), &canvas, space)?;
        let brush = GpuBrush::new(gpu.clone());
        for s in &canvas.sources {
            tracing::debug!(
                name = s.name,
                tiles = s.slots.iter().flatten().count(),
                "source"
            );
        }
        tracing::info!(
            sources = canvas.sources.len(),
            ops = canvas.ops.len(),
            vector_runs = canvas.runs.len(),
            vector_objects = canvas.vector_count(),
            atlas_tiles = atlas.used(),
            atlas_pages = atlas.pages(),
            ms = t.elapsed().as_millis() as u64,
            "document on GPU"
        );
        let cached_ops = if cache {
            canvas.cacheable_prefix() as u32
        } else {
            0
        };
        let camera = Camera::fit((doc.width, doc.height), screen);
        let slots = cache_slots((doc.width, doc.height), screen, camera);
        let cache_enabled = cache;
        let cache =
            (cached_ops > 0 && slots <= 256).then(|| CompositeCache::new(gpu.clone(), slots));
        Ok(Self {
            camera,
            cache_enabled,
            cache,
            cached_ops,
            gpu,
            atlas,
            canvas,
            compositor,
            vectors,
            brush,
            screen,
            hud: Vec::new(),
            pending: None,
            signature: Canvas::signature(doc)?,
            diagram_doc: (vello && crate::canvas::diagram_vector_supported(doc))
                .then(|| doc.clone()),
            vello,
            paint_node,
        })
    }

    /// Adopt a changed document, reusing the atlas.
    ///
    /// The engine has no incremental structural update: a node added, removed,
    /// reordered, or given a different blend, opacity, clip or mask needs a new
    /// program. Rebuilding the whole engine would also rebuild the atlas and
    /// re-upload every tile, which for a 4K document is over a gigabyte per
    /// edit. This rebuilds the program against the existing atlas instead, so
    /// tiles the edit left alone are re-acquired by `Arc` identity and never
    /// travel to the GPU again.
    ///
    /// On failure the engine is left as it was and the caller should rebuild
    /// from scratch; the usual cause is the atlas having no room for the new
    /// document, since it was sized for the old one plus headroom.
    pub fn reload(
        &mut self,
        doc: &Document,
        paint_node: Option<NodeId>,
        vello: bool,
    ) -> anyhow::Result<()> {
        crate::canvas::ensure_blend_space_supported(doc.blend_space)?;
        self.gpu.ensure_alive()?;
        // Moving native diagram paths changes neither compositing nor raster
        // sources. Avoid rebuilding a composite tree merely to discover that.
        if vello
            && self.vello
            && self.paint_node == paint_node
            && let Some(previous) = self.diagram_doc.as_ref()
            && let Some(changes) = diagram_vector_changes(previous, doc)
        {
            for (id, kind) in changes {
                for item in self
                    .canvas
                    .runs
                    .iter_mut()
                    .flatten()
                    .filter(|item| item.node == id)
                {
                    item.kind = kind.clone();
                }
                self.vectors.edit(id, |old| *old = kind);
            }
            self.diagram_doc = Some(doc.clone());
            // A later structural edit must not compare against a stale tree.
            self.signature.clear();
            return Ok(());
        }
        self.diagram_doc = None;
        // Pixel-only edits keep the program and tile tables. Masked or placed
        // sources rebake their changed pixels before replacing atlas tiles.
        let t0 = Instant::now();
        let after = Canvas::signature(doc)?;
        if self.canvas.width == doc.width
            && self.canvas.height == doc.height
            && self.canvas.space == doc.blend_space
            && self.canvas.knockout_background == doc.psd_background
            && self.vello == vello
            && self.paint_node == paint_node
            && let Some(changed) = Canvas::pixels_only_change(&self.signature, &after)
        {
            let (vectors, pixels): (Vec<_>, Vec<_>) = changed.into_iter().partition(|id| {
                self.canvas
                    .runs
                    .iter()
                    .flatten()
                    .any(|item| item.node == *id)
            });
            let mut current = if vectors.is_empty() {
                Default::default()
            } else {
                crate::canvas::vector_nodes(doc)
            };
            // A style edit can make a formerly Vello-drawn node ineligible.
            // Such transitions need a new program, not an in-place fragment.
            if vectors.iter().all(|id| current.contains_key(id))
                && self
                    .canvas
                    .replace_pixels(doc, &pixels, &self.gpu.queue, &mut self.atlas)?
            {
                for id in vectors {
                    let kind = current.remove(&id).expect("checked vector");
                    for item in self
                        .canvas
                        .runs
                        .iter_mut()
                        .flatten()
                        .filter(|item| item.node == id)
                    {
                        item.kind = kind.clone();
                    }
                    self.vectors.edit(id, |previous| *previous = kind);
                }
                self.signature = after;
                tracing::debug!(
                    ms = t0.elapsed().as_secs_f64() * 1e3,
                    "reload: content only"
                );
                return Ok(());
            }
        }

        let vectors_before = self.canvas.vector_signature();
        self.canvas
            .recompile(doc, &self.gpu, &mut self.atlas, paint_node, vello)?;
        self.signature = after;
        self.vello = vello;
        self.paint_node = paint_node;
        // The compositor holds the serialised op program. Without this the
        // shader keeps running the previous document's ops, so a structural
        // change -- a layer added, removed or reordered -- recompiles and
        // invalidates correctly and still draws the old picture.
        self.compositor.set_program(&self.canvas);
        // The vector layer holds one encoded Vello fragment per path and text
        // box, plus the R-tree used to cull them, all built from the previous
        // canvas. Paths and text added or changed by the pen and type tools
        // live only there, so without this they never reach the screen.
        // Re-encoding is skipped for documents with no vector content, which
        // is every purely raster document.
        self.diagram_doc =
            (vello && crate::canvas::diagram_vector_supported(doc)).then(|| doc.clone());
        let t_recompile = t0.elapsed().as_secs_f64() * 1e3;
        let t1 = Instant::now();
        let resynced = vectors_before != self.canvas.vector_signature();
        if resynced {
            self.vectors.resync(&self.canvas);
        }
        tracing::debug!(
            recompile_ms = t_recompile,
            resync_ms = t1.elapsed().as_secs_f64() * 1e3,
            resynced,
            objects = self.vectors.objects.len(),
            "reload: rebuilt"
        );
        self.cached_ops = if self.cache_enabled {
            self.canvas.cacheable_prefix() as u32
        } else {
            0
        };
        // `recompile` records only the tiles whose identity changed, and
        // `render` drains that into the composite cache, so an edit costs the
        // cache the tiles it touched rather than the whole view.
        Ok(())
    }

    /// How many leading ops the composite cache covers. Ops after this run
    /// per pixel, every frame.
    pub fn cached_ops(&self) -> u32 {
        self.cached_ops
    }

    /// Split borrows for recording GPU brush work.
    pub fn brush_parts(
        &mut self,
    ) -> (
        &mut GpuBrush,
        &mut Canvas,
        &mut Atlas,
        &mut wgpu::CommandEncoder,
    ) {
        let encoder = self.pending.get_or_insert_with(|| {
            self.gpu
                .device
                .create_command_encoder(&wgpu::CommandEncoderDescriptor {
                    label: Some("frame"),
                })
        });
        (&mut self.brush, &mut self.canvas, &mut self.atlas, encoder)
    }

    /// Submit pending work without drawing a frame.
    ///
    /// Brush dabs are recorded into a frame encoder that [`Self::render`]
    /// normally submits. A host that paints without presenting, and the
    /// fidelity checks, need that work on the queue anyway.
    pub fn flush(&mut self) {
        if let Some(encoder) = self.pending.take() {
            self.gpu.queue.submit([encoder.finish()]);
        }
    }

    /// Record and submit one frame into `target`.
    pub fn render(
        &mut self,
        target: &wgpu::TextureView,
        format: wgpu::TextureFormat,
        output: Output,
    ) -> anyhow::Result<FrameTimes> {
        self.gpu.ensure_alive()?;
        let slots = cache_slots(
            (self.canvas.width, self.canvas.height),
            self.screen,
            self.camera,
        );
        if self.cached_ops == 0 || slots > 256 {
            self.cache = None;
        } else if self
            .cache
            .as_ref()
            .is_none_or(|cache| cache.capacity() < slots || cache.capacity() > slots.max(64) * 3)
        {
            self.cache = Some(CompositeCache::new(self.gpu.clone(), slots));
        }
        let start = Instant::now();
        let mut times = FrameTimes::default();
        let mut encoder = self.pending.take().unwrap_or_else(|| {
            self.gpu
                .device
                .create_command_encoder(&wgpu::CommandEncoderDescriptor {
                    label: Some("frame"),
                })
        });
        let t = Instant::now();
        times.mip_tiles = {
            let _span = tracing::info_span!("mips").entered();
            self.atlas.build_mips(&mut encoder)
        };
        times.mips_ms = t.elapsed().as_secs_f64() * 1e3;
        for rect in std::mem::take(&mut self.canvas.dirty) {
            if let Some(cache) = &mut self.cache {
                cache.invalidate(rect);
            }
        }
        times.vector_runs = self.vectors.render(
            self.camera.affine(self.screen),
            self.camera.visible(self.screen),
            self.screen,
        )?;
        let v = self.vectors.take_stats();
        times.vector_encode_ms = v.encode_ms;
        times.vector_render_ms = v.render_ms;
        times.visible_vectors = v.visible;
        let t = Instant::now();
        let origin = self.camera.origin(self.screen);
        let level = self.camera.level();
        let space = self.vectors.space;
        let runs = self.vectors.run_count() as u32;
        let lines = if output == Output::Raw {
            Vec::new()
        } else {
            self.hud.clone()
        };
        let (hud_view, hud_size) = {
            let (view, size) = self.vectors.hud(&lines)?;
            (view.clone(), size)
        };
        let vector_view = self.vectors.view(self.screen)?.clone();
        let view = ViewUniform {
            screen: [self.screen.0 as f32, self.screen.1 as f32],
            doc: [self.canvas.width as f32, self.canvas.height as f32],
            origin: [origin[0] as f32, origin[1] as f32],
            scale: (1.0 / self.camera.zoom) as f32,
            level,
            hud: hud_size,
            checker: 8.0,
            output: match output {
                Output::Encoded => 0,
                Output::Raw => 1,
                Output::HardwareSrgb => 2,
            },
            vector_space: u32::from(space == VectorSpace::Linear),
            runs,
            cached_ops: if self.cache.is_some() {
                self.cached_ops
            } else {
                0
            },
            cache_columns: emulsion_raster::composite::tiles_at(
                self.canvas.width,
                self.canvas.height,
                level,
            )
            .0 as u32,
        };
        let shared = self.compositor.begin(&self.canvas, &self.atlas.view, view);
        if let Some(cache) = &mut self.cache {
            let _span = tracing::info_span!("cache_fill").entered();
            times.cache_fills = cache
                .update(
                    &mut encoder,
                    (self.canvas.width, self.canvas.height),
                    level,
                    self.camera.visible(self.screen),
                    &self.compositor,
                    &shared,
                    &vector_view,
                )
                .filled;
        }
        {
            let _span = tracing::info_span!("composite").entered();
            let (table, cache_view) = match &self.cache {
                Some(c) => (&c.table, &c.view),
                None => (&self.canvas.tables, &self.atlas.view),
            };
            self.compositor.draw(
                &mut encoder,
                target,
                format,
                &shared,
                &vector_view,
                &hud_view,
                table,
                cache_view,
            );
        }
        {
            let _span = tracing::info_span!("submit").entered();
            self.gpu.queue.submit([encoder.finish()]);
        }
        times.composite_ms = t.elapsed().as_secs_f64() * 1e3;
        times.uploaded_bytes = self.atlas.take_uploaded();
        times.cpu_ms = start.elapsed().as_secs_f64() * 1e3;
        Ok(times)
    }

    /// Bytes of GPU textures this engine created (atlas with mips, Vello
    /// targets). Vello's internal buffers are not included.
    pub fn texture_bytes(&self) -> u64 {
        self.atlas.bytes()
            + self.vectors.target_bytes()
            + self.cache.as_ref().map_or(0, CompositeCache::bytes)
    }
}

/// An offscreen colour target, for headless runs and fidelity readback.
pub struct Offscreen {
    pub texture: wgpu::Texture,
    pub view: wgpu::TextureView,
    pub format: wgpu::TextureFormat,
    pub size: (u32, u32),
}

impl Offscreen {
    pub fn new(gpu: &Gpu, size: (u32, u32), format: wgpu::TextureFormat) -> Self {
        let texture = gpu.device.create_texture(&wgpu::TextureDescriptor {
            label: Some("offscreen"),
            size: wgpu::Extent3d {
                width: size.0,
                height: size.1,
                depth_or_array_layers: 1,
            },
            mip_level_count: 1,
            sample_count: 1,
            dimension: wgpu::TextureDimension::D2,
            format,
            usage: wgpu::TextureUsages::RENDER_ATTACHMENT | wgpu::TextureUsages::COPY_SRC,
            view_formats: &[],
        });
        let view = texture.create_view(&Default::default());
        Self {
            texture,
            view,
            format,
            size,
        }
    }

    /// Read the whole target back (blocking). Rows are tightly packed.
    pub fn read(&self, gpu: &Gpu) -> anyhow::Result<Vec<u8>> {
        let bpp = self
            .format
            .block_copy_size(None)
            .ok_or_else(|| anyhow::anyhow!("format has no block size"))?;
        let row = self.size.0 * bpp;
        let padded = row.div_ceil(256) * 256;
        let buffer = gpu.device.create_buffer(&wgpu::BufferDescriptor {
            label: Some("offscreen readback"),
            size: padded as u64 * self.size.1 as u64,
            usage: wgpu::BufferUsages::COPY_DST | wgpu::BufferUsages::MAP_READ,
            mapped_at_creation: false,
        });
        let mut encoder = gpu
            .device
            .create_command_encoder(&wgpu::CommandEncoderDescriptor { label: None });
        encoder.copy_texture_to_buffer(
            self.texture.as_image_copy(),
            wgpu::TexelCopyBufferInfo {
                buffer: &buffer,
                layout: wgpu::TexelCopyBufferLayout {
                    offset: 0,
                    bytes_per_row: Some(padded),
                    rows_per_image: Some(self.size.1),
                },
            },
            wgpu::Extent3d {
                width: self.size.0,
                height: self.size.1,
                depth_or_array_layers: 1,
            },
        );
        gpu.queue.submit([encoder.finish()]);
        let (tx, rx) = std::sync::mpsc::channel();
        buffer.slice(..).map_async(wgpu::MapMode::Read, move |r| {
            let _ = tx.send(r);
        });
        gpu.wait();
        rx.recv()??;
        let data = buffer.slice(..).get_mapped_range();
        let mut out = Vec::with_capacity((row * self.size.1) as usize);
        for y in 0..self.size.1 as usize {
            out.extend_from_slice(&data[y * padded as usize..y * padded as usize + row as usize]);
        }
        Ok(out)
    }
}

/// None means a full compile is necessary; empty means no visual changes.
fn diagram_vector_changes(
    before: &Document,
    after: &Document,
) -> Option<Vec<(NodeId, crate::canvas::VectorKind)>> {
    use crate::canvas::{VectorKind, path_supported, text_supported};
    use emulsion_core::NodeKind;
    if crate::canvas::ensure_blend_space_supported(after.blend_space).is_err()
        || before.width != after.width
        || before.height != after.height
        || before.blend_space != after.blend_space
        || before.psd_background != after.psd_background
        || before.design != after.design
        || before.nodes.len() != after.nodes.len()
    {
        return None;
    }
    let mut changed = Vec::new();
    for (a, b) in before.nodes.iter().zip(&after.nodes) {
        if a == b {
            continue;
        }
        let mut metadata = a.clone();
        metadata.kind = b.kind.clone();
        if metadata != *b {
            return None;
        }
        let kind = match (&a.kind, &b.kind) {
            (NodeKind::Path { .. }, NodeKind::Path { path, style, .. })
                if path_supported(style) =>
            {
                VectorKind::Path {
                    path: path.clone(),
                    style: *style,
                }
            }
            (NodeKind::Text { .. }, NodeKind::Text { spec, .. }) if text_supported(spec) => {
                VectorKind::Text { spec: spec.clone() }
            }
            _ => return None,
        };
        changed.push((b.id, kind));
    }
    Some(changed)
}

#[cfg(test)]
mod diagram_updates {
    use super::*;
    #[test]
    fn diagram_fast_reload_rejects_profile_and_background_changes() {
        let before = emulsion_core::diagram_library::TEMPLATES[0]
            .build()
            .unwrap();
        let mut after = before.clone();
        after.blend_space = emulsion_raster::blend::BlendSpace::PhotoshopSrgbV1;
        assert!(diagram_vector_changes(&before, &after).is_none());
        assert!(diagram_vector_changes(&after, &after).is_none());
        after = before.clone();
        after.psd_background = Some(1);
        assert!(diagram_vector_changes(&before, &after).is_none());
    }

    #[test]
    fn diagram_movement_updates_vectors_but_group_opacity_requires_recompile() {
        let doc = emulsion_core::diagram_library::TEMPLATES[0]
            .build()
            .unwrap();
        let id = *doc.diagram.as_ref().unwrap().shapes.keys().next().unwrap();
        let mut e = emulsion_core::Editor::new(doc.clone(), None);
        e.execute(emulsion_core::Command::TranslateNode { id, dx: 7., dy: 3. })
            .unwrap();
        let changes = diagram_vector_changes(&doc, &e.doc).unwrap();
        assert!(!changes.is_empty());
        assert!(changes.len() < e.doc.nodes.len());
        e.doc.node_mut(id).unwrap().opacity = 0.5;
        assert!(diagram_vector_changes(&doc, &e.doc).is_none());
    }
}
