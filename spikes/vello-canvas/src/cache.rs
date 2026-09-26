//! GPU composite cache: flattened document tiles per mip level, like the
//! GPUI canvas's tile cache but filled on the GPU.
//!
//! A cache tile holds the composite of the canvas program's cacheable prefix
//! (see `Canvas::cacheable_prefix`) for one 256×256 tile at one level. The
//! screen pass then samples one texel for that prefix instead of running
//! every layer. Tiles are filled only when visible and missing, and dropped
//! at every level when painting changes pixels under them.

use crate::atlas::{PER_PAGE, Slot};
use crate::compositor::Compositor;
use crate::gpu::Gpu;
use emulsion_raster::composite::tiles_at;
use emulsion_raster::{IRect, TILE, TileCoord};
use std::collections::HashMap;
use std::sync::Arc;

const NONE: u32 = u32::MAX;

pub struct CompositeCache {
    gpu: Arc<Gpu>,
    /// Kept alongside its views.
    _texture: wgpu::Texture,
    pub view: wgpu::TextureView,
    pages: u32,
    page_views: Vec<wgpu::TextureView>,
    free: Vec<Slot>,
    /// (level, level tile) → (slot, frame last used).
    entries: HashMap<(u32, TileCoord), (Slot, u64)>,
    frame: u64,
    pub table: wgpu::Buffer,
    table_len: usize,
    pub fills: wgpu::Buffer,
    fills_len: usize,
}

#[derive(Clone, Copy, Debug, Default)]
pub struct CacheFrame {
    pub filled: usize,
}

impl CompositeCache {
    /// Room for `slots` tiles (rounded up to whole pages).
    pub fn new(gpu: Arc<Gpu>, slots: u32) -> Self {
        let pages = slots
            .div_ceil(PER_PAGE)
            .clamp(1, gpu.device.limits().max_texture_array_layers);
        let texture = gpu.device.create_texture(&wgpu::TextureDescriptor {
            label: Some("composite cache"),
            size: wgpu::Extent3d {
                width: crate::atlas::PAGE,
                height: crate::atlas::PAGE,
                depth_or_array_layers: pages,
            },
            mip_level_count: 1,
            sample_count: 1,
            dimension: wgpu::TextureDimension::D2,
            format: gpu.tile_format.wgpu(),
            usage: wgpu::TextureUsages::TEXTURE_BINDING | wgpu::TextureUsages::RENDER_ATTACHMENT,
            view_formats: &[],
        });
        let view = texture.create_view(&wgpu::TextureViewDescriptor {
            dimension: Some(wgpu::TextureViewDimension::D2Array),
            ..Default::default()
        });
        let page_views = (0..pages)
            .map(|p| {
                texture.create_view(&wgpu::TextureViewDescriptor {
                    dimension: Some(wgpu::TextureViewDimension::D2),
                    base_array_layer: p,
                    array_layer_count: Some(1),
                    ..Default::default()
                })
            })
            .collect();
        let buffer = |label, bytes: usize| {
            gpu.device.create_buffer(&wgpu::BufferDescriptor {
                label: Some(label),
                size: bytes.max(16) as u64,
                usage: wgpu::BufferUsages::STORAGE | wgpu::BufferUsages::COPY_DST,
                mapped_at_creation: false,
            })
        };
        let table = buffer("cache table", 4096 * 4);
        let fills = buffer("cache fills", 1024 * 16);
        Self {
            free: (0..pages * PER_PAGE).rev().collect(),
            gpu,
            _texture: texture,
            view,
            pages,
            page_views,
            entries: HashMap::new(),
            frame: 0,
            table,
            table_len: 4096,
            fills,
            fills_len: 1024,
        }
    }

    pub fn bytes(&self) -> u64 {
        crate::atlas::PAGE as u64 * crate::atlas::PAGE as u64 * 8 * self.pages as u64
    }

    /// Drop every cached tile, at every level, that overlaps `rect`.
    pub fn invalidate(&mut self, rect: IRect) {
        if rect.is_empty() {
            return;
        }
        let free = &mut self.free;
        self.entries.retain(|(level, c), (slot, _)| {
            let span = (TILE << level) as i32;
            let tile = IRect::new(c.x * span, c.y * span, span, span);
            let keep = tile.intersect(&rect).is_empty();
            if !keep {
                free.push(*slot);
            }
            keep
        });
    }

    fn alloc(&mut self, protect: u64) -> Option<Slot> {
        if let Some(slot) = self.free.pop() {
            return Some(slot);
        }
        // Evict the least recently used tile not needed this frame.
        let victim = self
            .entries
            .iter()
            .filter(|(_, (_, used))| *used < protect)
            .min_by_key(|(_, (_, used))| *used)
            .map(|(k, _)| *k)?;
        self.entries.remove(&victim).map(|(slot, _)| slot)
    }

    /// Make every visible tile at `level` present, recording fills into
    /// `encoder`, and write the lookup table the screen pass reads.
    #[allow(clippy::too_many_arguments)]
    pub fn update(
        &mut self,
        encoder: &mut wgpu::CommandEncoder,
        doc: (u32, u32),
        level: u32,
        visible: [f64; 4],
        compositor: &Compositor,
        shared: &wgpu::BindGroup,
        vectors: &wgpu::TextureView,
    ) -> CacheFrame {
        self.frame += 1;
        let (columns, rows) = tiles_at(doc.0, doc.1, level);
        let span = (TILE << level) as f64;
        let x0 = (visible[0] / span).floor().max(0.0) as i32;
        let y0 = (visible[1] / span).floor().max(0.0) as i32;
        let x1 = ((visible[2] / span).ceil() as i32).min(columns);
        let y1 = ((visible[3] / span).ceil() as i32).min(rows);
        let mut fills: Vec<[u32; 4]> = Vec::new();
        let mut table = vec![NONE; (columns * rows).max(1) as usize];
        for y in y0..y1 {
            for x in x0..x1 {
                let key = (level, TileCoord::new(x, y));
                let slot = match self.entries.get_mut(&key) {
                    Some((slot, used)) => {
                        *used = self.frame;
                        *slot
                    }
                    None => {
                        let Some(slot) = self.alloc(self.frame) else {
                            tracing::warn!("composite cache too small for the view");
                            continue;
                        };
                        self.entries.insert(key, (slot, self.frame));
                        fills.push([slot, level, x as u32, y as u32]);
                        slot
                    }
                };
                table[(y * columns + x) as usize] = slot;
            }
        }
        if table.len() > self.table_len {
            self.table_len = table.len().next_power_of_two();
            self.table = self.gpu.device.create_buffer(&wgpu::BufferDescriptor {
                label: Some("cache table"),
                size: self.table_len as u64 * 4,
                usage: wgpu::BufferUsages::STORAGE | wgpu::BufferUsages::COPY_DST,
                mapped_at_creation: false,
            });
        }
        self.gpu
            .queue
            .write_buffer(&self.table, 0, bytemuck::cast_slice(&table));
        if !fills.is_empty() {
            fills.sort_by_key(|f| f[0]);
            if fills.len() > self.fills_len {
                self.fills_len = fills.len().next_power_of_two();
                self.fills = self.gpu.device.create_buffer(&wgpu::BufferDescriptor {
                    label: Some("cache fills"),
                    size: self.fills_len as u64 * 16,
                    usage: wgpu::BufferUsages::STORAGE | wgpu::BufferUsages::COPY_DST,
                    mapped_at_creation: false,
                });
            }
            self.gpu
                .queue
                .write_buffer(&self.fills, 0, bytemuck::cast_slice(&fills));
            let group = compositor.fill_group(vectors, &self.fills);
            let mut start = 0;
            while start < fills.len() {
                let page = fills[start][0] / PER_PAGE;
                let end = start
                    + fills[start..]
                        .iter()
                        .take_while(|f| f[0] / PER_PAGE == page)
                        .count();
                let mut pass = encoder.begin_render_pass(&wgpu::RenderPassDescriptor {
                    label: Some("cache fill"),
                    color_attachments: &[Some(wgpu::RenderPassColorAttachment {
                        view: &self.page_views[page as usize],
                        depth_slice: None,
                        resolve_target: None,
                        ops: wgpu::Operations {
                            load: wgpu::LoadOp::Load,
                            store: wgpu::StoreOp::Store,
                        },
                    })],
                    ..Default::default()
                });
                compositor.fill(&mut pass, shared, &group, start as u32..end as u32);
                start = end;
            }
        }
        CacheFrame {
            filled: fills.len(),
        }
    }
}
