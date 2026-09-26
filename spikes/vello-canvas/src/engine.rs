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
    pending: Option<wgpu::CommandEncoder>,
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
        let t = Instant::now();
        // Room for painting a full layer's worth of new tiles.
        let headroom = doc.width.div_ceil(256) * doc.height.div_ceil(256) + 64;
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
        // Four views' worth of tiles at 100% on a 2560×1600 screen.
        let cache = (cached_ops > 0).then(|| CompositeCache::new(gpu.clone(), 4 * 11 * 8));
        Ok(Self {
            camera: Camera::fit((doc.width, doc.height), screen),
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
        })
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
    #[cfg(test)]
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
        let vector_view = self.vectors.view(self.screen).clone();
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
