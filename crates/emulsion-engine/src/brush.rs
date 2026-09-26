//! Brush tests.
//!
//! A: Emulsion's CPU `Stroke` stamps dabs; only tiles whose identity changed
//!    are uploaded.
//! B: round dabs are drawn straight into the layer's atlas tiles; the CPU
//!    raster is read back asynchronously when the stroke ends.

use crate::atlas::{Atlas, PER_PAGE, Slot, TILE_BYTES, decode_tile, slot_origin};
use crate::canvas::Canvas;
use crate::gpu::Gpu;
use emulsion_raster::paint::{Brush, Ink, Stroke};
use emulsion_raster::{IRect, Raster, TILE, TileCoord};
use std::collections::{BTreeMap, BTreeSet};
use std::sync::atomic::{AtomicBool, Ordering};
use std::sync::{Arc, Mutex};
use std::time::Instant;

/// The large soft round brush both tests paint with.
pub fn test_brush(size: f32) -> Brush {
    Brush {
        size,
        hardness: 0.0,
        flow: 0.35,
        spacing: 0.12,
        opacity: 1.0,
        ..Brush::default()
    }
}

pub const INK: [f32; 4] = [0.8, 0.12, 0.05, 1.0];

/// Tiles overlapping `rect`.
pub fn tiles_in(rect: IRect) -> Vec<TileCoord> {
    if rect.is_empty() {
        return Vec::new();
    }
    let t = TILE as i32;
    let mut out = Vec::new();
    for ty in rect.y.div_euclid(t)..=(rect.bottom() - 1).div_euclid(t) {
        for tx in rect.x.div_euclid(t)..=(rect.right() - 1).div_euclid(t) {
            out.push(TileCoord::new(tx, ty));
        }
    }
    out
}

// ── Test A ──

pub struct CpuStroke {
    stroke: Stroke,
    source: usize,
}

#[derive(Clone, Copy, Debug, Default)]
pub struct CpuStrokeFrame {
    pub render_ms: f64,
    pub upload_ms: f64,
    pub tiles: usize,
}

impl CpuStroke {
    pub fn begin(canvas: &Canvas, source: usize, brush: Brush) -> Self {
        let base = canvas.sources[source].raster.clone();
        Self {
            stroke: Stroke::new(base, brush, Ink::Color(INK), None),
            source,
        }
    }

    pub fn point(&mut self, x: f32, y: f32, time_ms: f64) {
        self.stroke.point_at(x, y, None, Some(time_ms));
    }

    pub fn finish(&mut self) {
        self.stroke.finish();
    }

    /// Composite new paint into the layer and upload changed tiles.
    pub fn render(
        &mut self,
        canvas: &mut Canvas,
        atlas: &mut Atlas,
        queue: &wgpu::Queue,
    ) -> anyhow::Result<CpuStrokeFrame> {
        let t = Instant::now();
        let current = canvas.sources[self.source].raster.clone();
        let (raster, dirty) = {
            let _span = tracing::info_span!("stamp").entered();
            self.stroke.render(&current)
        };
        let render_ms = t.elapsed().as_secs_f64() * 1e3;
        let t = Instant::now();
        let tiles = {
            let _span = tracing::info_span!("upload").entered();
            canvas.replace_raster(
                queue,
                atlas,
                self.source,
                Arc::new(raster),
                Some(&tiles_in(dirty)),
            )?
        };
        Ok(CpuStrokeFrame {
            render_ms,
            upload_ms: t.elapsed().as_secs_f64() * 1e3,
            tiles,
        })
    }
}

// ── Test B ──

#[repr(C)]
#[derive(Clone, Copy, bytemuck::Pod, bytemuck::Zeroable)]
struct Quad {
    rect: [f32; 4],
    origin: [f32; 4],
    dab: [f32; 4],
    color: [f32; 4],
    extra: [f32; 4],
}

pub struct GpuBrush {
    gpu: Arc<Gpu>,
    over: wgpu::RenderPipeline,
    clear: wgpu::RenderPipeline,
    layout: wgpu::BindGroupLayout,
    quads: wgpu::Buffer,
    capacity: usize,
}

impl GpuBrush {
    pub fn new(gpu: Arc<Gpu>) -> Self {
        let device = &gpu.device;
        let shader = device.create_shader_module(wgpu::ShaderModuleDescriptor {
            label: Some("gpu brush"),
            source: wgpu::ShaderSource::Wgsl(include_str!("brush.wgsl").into()),
        });
        let layout = device.create_bind_group_layout(&wgpu::BindGroupLayoutDescriptor {
            label: Some("gpu brush"),
            entries: &[wgpu::BindGroupLayoutEntry {
                binding: 0,
                visibility: wgpu::ShaderStages::VERTEX | wgpu::ShaderStages::FRAGMENT,
                ty: wgpu::BindingType::Buffer {
                    ty: wgpu::BufferBindingType::Storage { read_only: true },
                    has_dynamic_offset: false,
                    min_binding_size: None,
                },
                count: None,
            }],
        });
        let pipeline_layout = device.create_pipeline_layout(&wgpu::PipelineLayoutDescriptor {
            label: Some("gpu brush"),
            bind_group_layouts: &[Some(&layout)],
            immediate_size: 0,
        });
        let pipeline = |entry: &str, blend: Option<wgpu::BlendState>| {
            device.create_render_pipeline(&wgpu::RenderPipelineDescriptor {
                label: Some(entry),
                layout: Some(&pipeline_layout),
                vertex: wgpu::VertexState {
                    module: &shader,
                    entry_point: Some("vs"),
                    compilation_options: Default::default(),
                    buffers: &[],
                },
                fragment: Some(wgpu::FragmentState {
                    module: &shader,
                    entry_point: Some(entry),
                    compilation_options: Default::default(),
                    targets: &[Some(wgpu::ColorTargetState {
                        format: gpu.tile_format.wgpu(),
                        blend,
                        write_mask: wgpu::ColorWrites::ALL,
                    })],
                }),
                primitive: Default::default(),
                depth_stencil: None,
                multisample: Default::default(),
                multiview_mask: None,
                cache: None,
            })
        };
        let over = pipeline(
            "fs_dab",
            Some(wgpu::BlendState::PREMULTIPLIED_ALPHA_BLENDING),
        );
        let clear = pipeline("fs_clear", None);
        let capacity = 4096;
        let quads = Self::buffer(device, capacity);
        Self {
            gpu,
            over,
            clear,
            layout,
            quads,
            capacity,
        }
    }

    fn buffer(device: &wgpu::Device, capacity: usize) -> wgpu::Buffer {
        device.create_buffer(&wgpu::BufferDescriptor {
            label: Some("brush quads"),
            size: (capacity * std::mem::size_of::<Quad>()) as u64,
            usage: wgpu::BufferUsages::STORAGE | wgpu::BufferUsages::COPY_DST,
            mapped_at_creation: false,
        })
    }
}

/// Dab placement for a brush with no dynamics, identical to the CPU
/// `Stroke`'s spacing: a dab at the first point, then one every
/// `size × spacing` pixels of path, carrying the remainder across samples.
pub struct DabPath {
    step: f32,
    last: Option<(f32, f32)>,
    carry: f32,
}

impl DabPath {
    pub fn new(brush: &Brush) -> Self {
        Self {
            step: (brush.size * brush.spacing).max(0.5),
            last: None,
            carry: 0.0,
        }
    }

    pub fn feed(&mut self, x: f32, y: f32, out: &mut Vec<(f32, f32)>) {
        match self.last {
            None => {
                out.push((x, y));
                self.carry = self.step;
            }
            Some((lx, ly)) => {
                let (dx, dy) = (x - lx, y - ly);
                let len = (dx * dx + dy * dy).sqrt();
                let mut d = self.carry;
                while len > 0.0 && d <= len {
                    let f = d / len;
                    out.push((lx + dx * f, ly + dy * f));
                    d += self.step;
                }
                self.carry = d - len;
            }
        }
        self.last = Some((x, y));
    }
}

pub struct GpuStroke {
    source: usize,
    brush: Brush,
    path: DabPath,
    pending: Vec<(f32, f32)>,
    touched: BTreeMap<TileCoord, Slot>,
    pub dabs: usize,
}

#[derive(Clone, Copy, Debug, Default)]
pub struct GpuStrokeFrame {
    pub dabs: usize,
    pub quads: usize,
    pub tiles: usize,
    pub record_ms: f64,
}

pub struct Readback {
    buffer: wgpu::Buffer,
    coords: Vec<(TileCoord, Slot)>,
    ready: Arc<AtomicBool>,
    failed: Arc<Mutex<Option<String>>>,
    pub started: Instant,
    pub bytes: u64,
}

impl GpuStroke {
    pub fn begin(source: usize, brush: Brush) -> Self {
        Self {
            source,
            path: DabPath::new(&brush),
            brush,
            pending: Vec::new(),
            touched: BTreeMap::new(),
            dabs: 0,
        }
    }

    pub fn point(&mut self, x: f32, y: f32) {
        self.path.feed(x, y, &mut self.pending);
    }

    /// Record draws for the dabs placed since the last frame.
    pub fn render(
        &mut self,
        brush: &mut GpuBrush,
        canvas: &mut Canvas,
        atlas: &mut Atlas,
        encoder: &mut wgpu::CommandEncoder,
    ) -> anyhow::Result<GpuStrokeFrame> {
        let _span = tracing::info_span!("stamp").entered();
        let t = Instant::now();
        let dabs = std::mem::take(&mut self.pending);
        if dabs.is_empty() {
            return Ok(GpuStrokeFrame::default());
        }
        let r = (self.brush.size / 2.0).max(0.3);
        let bounds = IRect::new(0, 0, canvas.width as i32, canvas.height as i32);
        let mut clears: BTreeMap<u32, Vec<Quad>> = BTreeMap::new();
        let mut draws: BTreeMap<u32, Vec<Quad>> = BTreeMap::new();
        let mut tiles = BTreeSet::new();
        let mut painted = BTreeSet::new();
        let t_px = TILE as i32;
        for &(cx, cy) in &dabs {
            let b = IRect::new(
                (cx - r).floor() as i32,
                (cy - r).floor() as i32,
                (2.0 * r).ceil() as i32 + 2,
                (2.0 * r).ceil() as i32 + 2,
            )
            .intersect(&bounds);
            for c in tiles_in(b) {
                let slot = match self.touched.get(&c) {
                    Some(&slot) => slot,
                    None => {
                        let Some((slot, fresh)) =
                            canvas.own_tile(&brush.gpu.queue, encoder, atlas, self.source, c)?
                        else {
                            continue;
                        };
                        let (_, ax, ay) = slot_origin(slot);
                        if fresh {
                            clears.entry(slot / PER_PAGE).or_default().push(Quad {
                                rect: [
                                    (c.x * t_px) as f32,
                                    (c.y * t_px) as f32,
                                    ((c.x + 1) * t_px) as f32,
                                    ((c.y + 1) * t_px) as f32,
                                ],
                                origin: [
                                    (c.x * t_px) as f32,
                                    (c.y * t_px) as f32,
                                    ax as f32,
                                    ay as f32,
                                ],
                                dab: [0.0; 4],
                                color: [0.0; 4],
                                extra: [0.0; 4],
                            });
                        }
                        self.touched.insert(c, slot);
                        slot
                    }
                };
                tiles.insert(slot);
                painted.insert(c);
                let tile = IRect::new(c.x * t_px, c.y * t_px, t_px, t_px).intersect(&b);
                let (_, ax, ay) = slot_origin(slot);
                draws.entry(slot / PER_PAGE).or_default().push(Quad {
                    rect: [
                        tile.x as f32,
                        tile.y as f32,
                        tile.right() as f32,
                        tile.bottom() as f32,
                    ],
                    origin: [
                        (c.x * t_px) as f32,
                        (c.y * t_px) as f32,
                        ax as f32,
                        ay as f32,
                    ],
                    dab: [cx, cy, r, self.brush.hardness],
                    color: INK,
                    extra: [self.brush.flow, 0.0, 0.0, 0.0],
                });
            }
        }
        // Upload all quads once: per page, clears then dabs, in dab order.
        let mut all: Vec<Quad> = Vec::new();
        let mut passes: Vec<(u32, std::ops::Range<u32>, std::ops::Range<u32>)> = Vec::new();
        let pages: BTreeSet<u32> = clears.keys().chain(draws.keys()).copied().collect();
        for page in pages {
            let c0 = all.len() as u32;
            all.extend(clears.remove(&page).unwrap_or_default());
            let d0 = all.len() as u32;
            all.extend(draws.remove(&page).unwrap_or_default());
            passes.push((page, c0..d0, d0..all.len() as u32));
        }
        if all.len() > brush.capacity {
            brush.capacity = all.len().next_power_of_two();
            brush.quads = GpuBrush::buffer(&brush.gpu.device, brush.capacity);
        }
        brush
            .gpu
            .queue
            .write_buffer(&brush.quads, 0, bytemuck::cast_slice(&all));
        let group = brush
            .gpu
            .device
            .create_bind_group(&wgpu::BindGroupDescriptor {
                label: Some("gpu brush"),
                layout: &brush.layout,
                entries: &[wgpu::BindGroupEntry {
                    binding: 0,
                    resource: brush.quads.as_entire_binding(),
                }],
            });
        for (page, clear, draw) in passes {
            let target = atlas.page_view(page, 0);
            let mut pass = encoder.begin_render_pass(&wgpu::RenderPassDescriptor {
                label: Some("gpu brush"),
                color_attachments: &[Some(wgpu::RenderPassColorAttachment {
                    view: &target,
                    depth_slice: None,
                    resolve_target: None,
                    ops: wgpu::Operations {
                        load: wgpu::LoadOp::Load,
                        store: wgpu::StoreOp::Store,
                    },
                })],
                ..Default::default()
            });
            pass.set_bind_group(0, &group, &[]);
            if !clear.is_empty() {
                pass.set_pipeline(&brush.clear);
                pass.draw(0..6, clear);
            }
            if !draw.is_empty() {
                pass.set_pipeline(&brush.over);
                pass.draw(0..6, draw);
            }
        }
        for &slot in &tiles {
            atlas.mark_dirty(slot);
        }
        for c in painted {
            canvas.mark_tile(c);
        }
        self.dabs += dabs.len();
        Ok(GpuStrokeFrame {
            dabs: dabs.len(),
            quads: all.len(),
            tiles: tiles.len(),
            record_ms: t.elapsed().as_secs_f64() * 1e3,
        })
    }

    /// Queue the painted tiles for readback. Submit `encoder` before polling.
    pub fn finish(self, atlas: &Atlas, encoder: &mut wgpu::CommandEncoder) -> Readback {
        let coords: Vec<(TileCoord, Slot)> = self.touched.into_iter().collect();
        let slots: Vec<Slot> = coords.iter().map(|(_, s)| *s).collect();
        let buffer = atlas.readback(encoder, &slots);
        Readback {
            buffer,
            bytes: coords.len() as u64 * TILE_BYTES,
            coords,
            ready: Arc::new(AtomicBool::new(false)),
            failed: Arc::new(Mutex::new(None)),
            started: Instant::now(),
        }
    }
}

impl Readback {
    /// Request the mapping; call once after the copy has been submitted.
    pub fn map(&self) {
        let ready = self.ready.clone();
        let failed = self.failed.clone();
        self.buffer
            .slice(..)
            .map_async(wgpu::MapMode::Read, move |result| {
                if let Err(e) = result {
                    *failed.lock().unwrap() = Some(e.to_string());
                }
                ready.store(true, Ordering::Release);
            });
    }

    pub fn is_ready(&self) -> bool {
        self.ready.load(Ordering::Acquire)
    }

    /// Fold the read-back tiles into the CPU raster (which `canvas` adopts
    /// without re-uploading).
    pub fn complete(
        self,
        gpu: &Gpu,
        canvas: &mut Canvas,
        atlas: &mut Atlas,
        source: usize,
    ) -> anyhow::Result<Arc<Raster>> {
        if let Some(e) = self.failed.lock().unwrap().take() {
            anyhow::bail!("readback failed: {e}");
        }
        let data = self.buffer.slice(..).get_mapped_range();
        let changes: Vec<(TileCoord, Option<Vec<[u16; 4]>>)> = self
            .coords
            .iter()
            .enumerate()
            .map(|(i, (c, _))| {
                let bytes = &data[i * TILE_BYTES as usize..(i + 1) * TILE_BYTES as usize];
                (*c, Some(decode_tile(gpu.tile_format, bytes)))
            })
            .collect();
        drop(data);
        self.buffer.unmap();
        let raster = Arc::new(canvas.sources[source].raster.with_changes(changes));
        canvas.adopt_raster(atlas, source, raster.clone(), &self.coords);
        Ok(raster)
    }
}
