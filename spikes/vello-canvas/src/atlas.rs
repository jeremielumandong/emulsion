//! GPU-resident document tiles.
//!
//! Every 256×256 document tile lives in a slot of a 2D array texture whose
//! layers ("pages") are 2048×2048. A page's mip chain is a per-tile mip chain:
//! tiles are 256-aligned, so for levels 0–8 a 2×2 box reduction never mixes
//! neighbouring tiles. That is exactly Emulsion's CPU mip definition (level k
//! of a tile equals level k of the whole image), so zoomed-out views sample
//! the same values the CPU compositor would.
//!
//! Tiles are deduplicated by their `Arc` identity, like the CPU planes share
//! them; painting on the GPU copies a shared slot first.

use crate::gpu::{Gpu, TileFormat, f16_to_f32, f32_to_f16};
use emulsion_raster::TILE;
use std::collections::{BTreeSet, HashMap};
use std::sync::Arc;
use std::sync::atomic::{AtomicU64, Ordering};

pub const PAGE: u32 = 2048;
pub const PER_ROW: u32 = PAGE / TILE;
pub const PER_PAGE: u32 = PER_ROW * PER_ROW;
/// 256 → 1 texel per tile.
pub const MIPS: u32 = 9;
pub const TILE_BYTES: u64 = (TILE * TILE * 8) as u64;

pub type Slot = u32;

pub struct Atlas {
    gpu: Arc<Gpu>,
    pub texture: wgpu::Texture,
    /// All pages and mips, for sampling.
    pub view: wgpu::TextureView,
    pages: u32,
    free: Vec<Slot>,
    refs: Vec<u32>,
    by_ptr: HashMap<usize, Slot>,
    /// The CPU tile each slot mirrors. Holding the `Arc` keeps its address
    /// from being reused by another allocation while it is a lookup key.
    slot_tile: Vec<Option<Arc<[[u16; 4]]>>>,
    /// Slots whose mips 1.. are stale.
    dirty: BTreeSet<Slot>,
    page_views: HashMap<(u32, u32), wgpu::TextureView>,
    mip_pipeline: wgpu::RenderPipeline,
    mip_layout: wgpu::BindGroupLayout,
    mip_groups: HashMap<(u32, u32), wgpu::BindGroup>,
    mip_slots: wgpu::Buffer,
    mip_levels: Vec<wgpu::Buffer>,
    pub uploaded: AtomicU64,
}

pub fn slot_origin(slot: Slot) -> (u32, u32, u32) {
    let local = slot % PER_PAGE;
    (
        slot / PER_PAGE,
        (local % PER_ROW) * TILE,
        (local / PER_ROW) * TILE,
    )
}

impl Atlas {
    pub fn new(gpu: Arc<Gpu>, slots: u32) -> Self {
        let max_pages = gpu.device.limits().max_texture_array_layers;
        let pages = slots.div_ceil(PER_PAGE).clamp(1, max_pages);
        let device = &gpu.device;
        let texture = device.create_texture(&wgpu::TextureDescriptor {
            label: Some("tile atlas"),
            size: wgpu::Extent3d {
                width: PAGE,
                height: PAGE,
                depth_or_array_layers: pages,
            },
            mip_level_count: MIPS,
            sample_count: 1,
            dimension: wgpu::TextureDimension::D2,
            format: gpu.tile_format.wgpu(),
            usage: wgpu::TextureUsages::TEXTURE_BINDING
                | wgpu::TextureUsages::RENDER_ATTACHMENT
                | wgpu::TextureUsages::COPY_DST
                | wgpu::TextureUsages::COPY_SRC,
            view_formats: &[],
        });
        let view = texture.create_view(&wgpu::TextureViewDescriptor {
            dimension: Some(wgpu::TextureViewDimension::D2Array),
            ..Default::default()
        });
        let shader = device.create_shader_module(wgpu::ShaderModuleDescriptor {
            label: Some("tile mips"),
            source: wgpu::ShaderSource::Wgsl(include_str!("mips.wgsl").into()),
        });
        let mip_layout = device.create_bind_group_layout(&wgpu::BindGroupLayoutDescriptor {
            label: Some("tile mips"),
            entries: &[
                wgpu::BindGroupLayoutEntry {
                    binding: 0,
                    visibility: wgpu::ShaderStages::FRAGMENT,
                    ty: wgpu::BindingType::Texture {
                        sample_type: wgpu::TextureSampleType::Float { filterable: false },
                        view_dimension: wgpu::TextureViewDimension::D2,
                        multisampled: false,
                    },
                    count: None,
                },
                wgpu::BindGroupLayoutEntry {
                    binding: 1,
                    visibility: wgpu::ShaderStages::VERTEX | wgpu::ShaderStages::FRAGMENT,
                    ty: wgpu::BindingType::Buffer {
                        ty: wgpu::BufferBindingType::Storage { read_only: true },
                        has_dynamic_offset: false,
                        min_binding_size: None,
                    },
                    count: None,
                },
                wgpu::BindGroupLayoutEntry {
                    binding: 2,
                    visibility: wgpu::ShaderStages::VERTEX | wgpu::ShaderStages::FRAGMENT,
                    ty: wgpu::BindingType::Buffer {
                        ty: wgpu::BufferBindingType::Uniform,
                        has_dynamic_offset: false,
                        min_binding_size: None,
                    },
                    count: None,
                },
            ],
        });
        let layout = device.create_pipeline_layout(&wgpu::PipelineLayoutDescriptor {
            label: Some("tile mips"),
            bind_group_layouts: &[Some(&mip_layout)],
            immediate_size: 0,
        });
        let mip_pipeline = device.create_render_pipeline(&wgpu::RenderPipelineDescriptor {
            label: Some("tile mips"),
            layout: Some(&layout),
            vertex: wgpu::VertexState {
                module: &shader,
                entry_point: Some("vs"),
                compilation_options: Default::default(),
                buffers: &[],
            },
            fragment: Some(wgpu::FragmentState {
                module: &shader,
                entry_point: Some(match gpu.tile_format {
                    TileFormat::Unorm16 => "fs_unorm16",
                    TileFormat::Float16 => "fs_float",
                }),
                compilation_options: Default::default(),
                targets: &[Some(gpu.tile_format.wgpu().into())],
            }),
            primitive: Default::default(),
            depth_stencil: None,
            multisample: Default::default(),
            multiview_mask: None,
            cache: None,
        });
        let mip_slots = device.create_buffer(&wgpu::BufferDescriptor {
            label: Some("mip slots"),
            size: (pages * PER_PAGE * 4) as u64,
            usage: wgpu::BufferUsages::STORAGE | wgpu::BufferUsages::COPY_DST,
            mapped_at_creation: false,
        });
        let mip_levels = (0..MIPS)
            .map(|mip| {
                wgpu::util::DeviceExt::create_buffer_init(
                    device,
                    &wgpu::util::BufferInitDescriptor {
                        label: Some("mip level"),
                        contents: bytemuck::cast_slice(&[mip, 0, 0, 0]),
                        usage: wgpu::BufferUsages::UNIFORM,
                    },
                )
            })
            .collect();
        let total = pages * PER_PAGE;
        Self {
            gpu,
            texture,
            view,
            pages,
            free: (0..total).rev().collect(),
            refs: vec![0; total as usize],
            by_ptr: HashMap::new(),
            slot_tile: vec![None; total as usize],
            dirty: BTreeSet::new(),
            page_views: HashMap::new(),
            mip_pipeline,
            mip_layout,
            mip_groups: HashMap::new(),
            mip_slots,
            mip_levels,
            uploaded: AtomicU64::new(0),
        }
    }

    pub fn pages(&self) -> u32 {
        self.pages
    }

    pub fn capacity(&self) -> u32 {
        self.pages * PER_PAGE
    }

    pub fn used(&self) -> u32 {
        self.capacity() - self.free.len() as u32
    }

    /// Texture bytes including mips.
    pub fn bytes(&self) -> u64 {
        let base = PAGE as u64 * PAGE as u64 * 8 * self.pages as u64;
        base + base / 3
    }

    pub fn take_uploaded(&self) -> u64 {
        self.uploaded.swap(0, Ordering::Relaxed)
    }

    /// Slot holding `tile`, uploading it the first time it is seen.
    pub fn acquire(&mut self, tile: &Arc<[[u16; 4]]>) -> Option<Slot> {
        let ptr = tile.as_ptr() as usize;
        if let Some(&slot) = self.by_ptr.get(&ptr) {
            self.refs[slot as usize] += 1;
            return Some(slot);
        }
        let slot = self.alloc()?;
        self.upload(slot, tile);
        self.by_ptr.insert(ptr, slot);
        self.slot_tile[slot as usize] = Some(tile.clone());
        Some(slot)
    }

    /// A fresh slot nobody shares, contents undefined.
    pub fn alloc(&mut self) -> Option<Slot> {
        let slot = self.free.pop()?;
        self.refs[slot as usize] = 1;
        Some(slot)
    }

    pub fn release(&mut self, slot: Slot) {
        let r = &mut self.refs[slot as usize];
        debug_assert!(*r > 0);
        *r -= 1;
        if *r == 0 {
            self.detach(slot);
            self.dirty.remove(&slot);
            self.free.push(slot);
        }
    }

    pub fn is_shared(&self, slot: Slot) -> bool {
        self.refs[slot as usize] > 1
    }

    /// The slot's contents are about to diverge from any CPU tile.
    pub fn detach(&mut self, slot: Slot) {
        if let Some(tile) = self.slot_tile[slot as usize].take() {
            self.by_ptr.remove(&(tile.as_ptr() as usize));
        }
    }

    /// Record that `slot` now mirrors `tile` (after a GPU paint was read back).
    pub fn attach(&mut self, slot: Slot, tile: &Arc<[[u16; 4]]>) {
        self.detach(slot);
        let ptr = tile.as_ptr() as usize;
        if let std::collections::hash_map::Entry::Vacant(e) = self.by_ptr.entry(ptr) {
            e.insert(slot);
            self.slot_tile[slot as usize] = Some(tile.clone());
        }
    }

    fn upload(&mut self, slot: Slot, tile: &[[u16; 4]]) {
        let (page, x, y) = slot_origin(slot);
        let converted;
        let bytes: &[u8] = match self.gpu.tile_format {
            TileFormat::Unorm16 => bytemuck::cast_slice(tile),
            TileFormat::Float16 => {
                converted = tile
                    .iter()
                    .flat_map(|p| p.map(|v| f32_to_f16(v as f32 / 65535.0)))
                    .collect::<Vec<u16>>();
                bytemuck::cast_slice(&converted)
            }
        };
        self.gpu.queue.write_texture(
            wgpu::TexelCopyTextureInfo {
                texture: &self.texture,
                mip_level: 0,
                origin: wgpu::Origin3d { x, y, z: page },
                aspect: wgpu::TextureAspect::All,
            },
            bytes,
            wgpu::TexelCopyBufferLayout {
                offset: 0,
                bytes_per_row: Some(TILE * 8),
                rows_per_image: Some(TILE),
            },
            wgpu::Extent3d {
                width: TILE,
                height: TILE,
                depth_or_array_layers: 1,
            },
        );
        self.uploaded.fetch_add(TILE_BYTES, Ordering::Relaxed);
        self.dirty.insert(slot);
    }

    pub fn mark_dirty(&mut self, slot: Slot) {
        self.dirty.insert(slot);
    }

    /// Copy `from` into a new slot on the GPU.
    pub fn duplicate(&mut self, encoder: &mut wgpu::CommandEncoder, from: Slot) -> Option<Slot> {
        let to = self.alloc()?;
        let (fp, fx, fy) = slot_origin(from);
        let (tp, tx, ty) = slot_origin(to);
        for mip in 0..MIPS {
            let size = TILE >> mip;
            encoder.copy_texture_to_texture(
                wgpu::TexelCopyTextureInfo {
                    texture: &self.texture,
                    mip_level: mip,
                    origin: wgpu::Origin3d {
                        x: fx >> mip,
                        y: fy >> mip,
                        z: fp,
                    },
                    aspect: wgpu::TextureAspect::All,
                },
                wgpu::TexelCopyTextureInfo {
                    texture: &self.texture,
                    mip_level: mip,
                    origin: wgpu::Origin3d {
                        x: tx >> mip,
                        y: ty >> mip,
                        z: tp,
                    },
                    aspect: wgpu::TextureAspect::All,
                },
                wgpu::Extent3d {
                    width: size,
                    height: size,
                    depth_or_array_layers: 1,
                },
            );
        }
        Some(to)
    }

    /// Render-target view of one page at one mip level.
    pub fn page_view(&mut self, page: u32, mip: u32) -> wgpu::TextureView {
        self.page_views
            .entry((page, mip))
            .or_insert_with(|| {
                self.texture.create_view(&wgpu::TextureViewDescriptor {
                    label: Some("atlas page"),
                    dimension: Some(wgpu::TextureViewDimension::D2),
                    base_mip_level: mip,
                    mip_level_count: Some(1),
                    base_array_layer: page,
                    array_layer_count: Some(1),
                    ..Default::default()
                })
            })
            .clone()
    }

    /// Rebuild mips 1.. of every dirty slot. Returns the number of tiles.
    pub fn build_mips(&mut self, encoder: &mut wgpu::CommandEncoder) -> usize {
        if self.dirty.is_empty() {
            return 0;
        }
        let slots: Vec<Slot> = std::mem::take(&mut self.dirty).into_iter().collect();
        self.gpu
            .queue
            .write_buffer(&self.mip_slots, 0, bytemuck::cast_slice(&slots));
        // Slots are sorted, so each page is one contiguous instance range.
        let mut ranges: Vec<(u32, std::ops::Range<u32>)> = Vec::new();
        for (i, &slot) in slots.iter().enumerate() {
            let page = slot / PER_PAGE;
            match ranges.last_mut() {
                Some((p, r)) if *p == page => r.end = i as u32 + 1,
                _ => ranges.push((page, i as u32..i as u32 + 1)),
            }
        }
        for mip in 1..MIPS {
            for (page, range) in &ranges {
                let target = self.page_view(*page, mip);
                let source = self.page_view(*page, mip - 1);
                let group = self
                    .mip_groups
                    .entry((*page, mip))
                    .or_insert_with(|| {
                        self.gpu
                            .device
                            .create_bind_group(&wgpu::BindGroupDescriptor {
                                label: Some("tile mips"),
                                layout: &self.mip_layout,
                                entries: &[
                                    wgpu::BindGroupEntry {
                                        binding: 0,
                                        resource: wgpu::BindingResource::TextureView(&source),
                                    },
                                    wgpu::BindGroupEntry {
                                        binding: 1,
                                        resource: self.mip_slots.as_entire_binding(),
                                    },
                                    wgpu::BindGroupEntry {
                                        binding: 2,
                                        resource: self.mip_levels[mip as usize].as_entire_binding(),
                                    },
                                ],
                            })
                    })
                    .clone();
                let mut pass = encoder.begin_render_pass(&wgpu::RenderPassDescriptor {
                    label: Some("tile mips"),
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
                pass.set_pipeline(&self.mip_pipeline);
                pass.set_bind_group(0, &group, &[]);
                pass.draw(0..6, range.clone());
            }
        }
        slots.len()
    }

    /// Queue a copy of mip 0 of `slots` into a mappable buffer.
    pub fn readback(&self, encoder: &mut wgpu::CommandEncoder, slots: &[Slot]) -> wgpu::Buffer {
        let buffer = self.gpu.device.create_buffer(&wgpu::BufferDescriptor {
            label: Some("tile readback"),
            size: (slots.len().max(1) as u64) * TILE_BYTES,
            usage: wgpu::BufferUsages::COPY_DST | wgpu::BufferUsages::MAP_READ,
            mapped_at_creation: false,
        });
        for (i, &slot) in slots.iter().enumerate() {
            let (page, x, y) = slot_origin(slot);
            encoder.copy_texture_to_buffer(
                wgpu::TexelCopyTextureInfo {
                    texture: &self.texture,
                    mip_level: 0,
                    origin: wgpu::Origin3d { x, y, z: page },
                    aspect: wgpu::TextureAspect::All,
                },
                wgpu::TexelCopyBufferInfo {
                    buffer: &buffer,
                    layout: wgpu::TexelCopyBufferLayout {
                        offset: i as u64 * TILE_BYTES,
                        bytes_per_row: Some(TILE * 8),
                        rows_per_image: Some(TILE),
                    },
                },
                wgpu::Extent3d {
                    width: TILE,
                    height: TILE,
                    depth_or_array_layers: 1,
                },
            );
        }
        buffer
    }
}

/// Decode one tile of a mapped readback buffer to CPU pixels.
pub fn decode_tile(format: TileFormat, bytes: &[u8]) -> Vec<[u16; 4]> {
    let words: Vec<u16> = bytes
        .as_chunks::<2>()
        .0
        .iter()
        .map(|b| u16::from_le_bytes(*b))
        .collect();
    words
        .as_chunks::<4>()
        .0
        .iter()
        .map(|p| match format {
            TileFormat::Unorm16 => [p[0], p[1], p[2], p[3]],
            TileFormat::Float16 => p
                .iter()
                .map(|&h| (f16_to_f32(h).clamp(0.0, 1.0) * 65535.0 + 0.5) as u16)
                .collect::<Vec<_>>()
                .try_into()
                .unwrap(),
        })
        .collect()
}
