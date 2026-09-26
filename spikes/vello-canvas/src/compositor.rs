//! The screen pass: every visible document pixel runs the canvas program
//! against atlas tiles and Vello targets, then gets a checkerboard, sRGB
//! encoding and the HUD. One pass, no intermediate targets.

use crate::canvas::Canvas;
use crate::gpu::Gpu;
use std::collections::HashMap;
use std::sync::Arc;

/// `emulsion-gpu`'s parity-tested blend kernels, reused verbatim.
const UPSTREAM: &str = include_str!("../../../crates/emulsion-gpu/src/composite.wgsl");

fn shader_source() -> String {
    let start = UPSTREAM
        .find("fn burn(")
        .expect("emulsion-gpu composite.wgsl defines burn()");
    let end = UPSTREAM
        .find("// Exact low/high-word")
        .expect("emulsion-gpu composite.wgsl marks the dissolve hash");
    format!(
        "@group(0) @binding(1) var<storage, read> program: array<u32>;\n{}\n{}",
        &UPSTREAM[start..end],
        include_str!("composite.wgsl")
    )
}

#[repr(C)]
#[derive(Clone, Copy, Debug, Default, bytemuck::Pod, bytemuck::Zeroable)]
pub struct ViewUniform {
    pub screen: [f32; 2],
    pub doc: [f32; 2],
    pub origin: [f32; 2],
    pub scale: f32,
    pub level: u32,
    pub hud: [f32; 2],
    pub checker: f32,
    pub output: u32,
    pub vector_space: u32,
    pub runs: u32,
    /// Leading ops served by the composite cache (0: cache off).
    pub cached_ops: u32,
    /// Cache tiles per row at `level`.
    pub cache_columns: u32,
}

/// Pan and zoom. `zoom` is screen pixels per document pixel.
#[derive(Clone, Copy, Debug, PartialEq)]
pub struct Camera {
    pub center: [f64; 2],
    pub zoom: f64,
}

impl Camera {
    pub fn fit(doc: (u32, u32), screen: (u32, u32)) -> Self {
        let zoom = (screen.0 as f64 / doc.0 as f64).min(screen.1 as f64 / doc.1 as f64) * 0.95;
        Self {
            center: [doc.0 as f64 / 2.0, doc.1 as f64 / 2.0],
            zoom,
        }
    }

    /// Document coordinate at the screen's top-left. At integral zoom the
    /// origin snaps to whole pixels so 100% views map 1:1.
    pub fn origin(&self, screen: (u32, u32)) -> [f64; 2] {
        let o = [
            self.center[0] - screen.0 as f64 / 2.0 / self.zoom,
            self.center[1] - screen.1 as f64 / 2.0 / self.zoom,
        ];
        if (self.zoom - self.zoom.round()).abs() < 1e-9 && self.zoom >= 1.0 {
            let z = self.zoom;
            [(o[0] * z).round() / z, (o[1] * z).round() / z]
        } else {
            o
        }
    }

    /// Mip level whose pixels are at least one screen pixel, as the CPU
    /// viewport picks tile levels.
    pub fn level(&self) -> u32 {
        if self.zoom >= 1.0 {
            0
        } else {
            ((1.0 / self.zoom).log2().floor() as u32).min(8)
        }
    }

    /// Document → screen affine (kurbo order: a, b, c, d, e, f).
    pub fn affine(&self, screen: (u32, u32)) -> [f64; 6] {
        let o = self.origin(screen);
        [
            self.zoom,
            0.0,
            0.0,
            self.zoom,
            -o[0] * self.zoom,
            -o[1] * self.zoom,
        ]
    }

    /// Visible document rectangle (x0, y0, x1, y1).
    pub fn visible(&self, screen: (u32, u32)) -> [f64; 4] {
        let o = self.origin(screen);
        [
            o[0],
            o[1],
            o[0] + screen.0 as f64 / self.zoom,
            o[1] + screen.1 as f64 / self.zoom,
        ]
    }
}

pub struct Compositor {
    gpu: Arc<Gpu>,
    shared: wgpu::BindGroupLayout,
    screen: wgpu::BindGroupLayout,
    fill: wgpu::BindGroupLayout,
    screen_layout: wgpu::PipelineLayout,
    shader: wgpu::ShaderModule,
    pipelines: HashMap<wgpu::TextureFormat, wgpu::RenderPipeline>,
    fill_pipeline: wgpu::RenderPipeline,
    uniform: wgpu::Buffer,
    program: wgpu::Buffer,
    program_words: usize,
}

fn texture(binding: u32, dimension: wgpu::TextureViewDimension) -> wgpu::BindGroupLayoutEntry {
    wgpu::BindGroupLayoutEntry {
        binding,
        visibility: wgpu::ShaderStages::FRAGMENT,
        ty: wgpu::BindingType::Texture {
            sample_type: wgpu::TextureSampleType::Float { filterable: false },
            view_dimension: dimension,
            multisampled: false,
        },
        count: None,
    }
}

fn storage(binding: u32) -> wgpu::BindGroupLayoutEntry {
    wgpu::BindGroupLayoutEntry {
        binding,
        visibility: wgpu::ShaderStages::VERTEX | wgpu::ShaderStages::FRAGMENT,
        ty: wgpu::BindingType::Buffer {
            ty: wgpu::BufferBindingType::Storage { read_only: true },
            has_dynamic_offset: false,
            min_binding_size: None,
        },
        count: None,
    }
}

fn view_entry(binding: u32, view: &wgpu::TextureView) -> wgpu::BindGroupEntry<'_> {
    wgpu::BindGroupEntry {
        binding,
        resource: wgpu::BindingResource::TextureView(view),
    }
}

impl Compositor {
    pub fn new(gpu: Arc<Gpu>) -> Self {
        let device = &gpu.device;
        let shader = device.create_shader_module(wgpu::ShaderModuleDescriptor {
            label: Some("composite"),
            source: wgpu::ShaderSource::Wgsl(shader_source().into()),
        });
        use wgpu::TextureViewDimension::{D2, D2Array};
        let layout = |label, entries: &[wgpu::BindGroupLayoutEntry]| {
            device.create_bind_group_layout(&wgpu::BindGroupLayoutDescriptor {
                label: Some(label),
                entries,
            })
        };
        let shared = layout(
            "composite shared",
            &[
                wgpu::BindGroupLayoutEntry {
                    binding: 0,
                    visibility: wgpu::ShaderStages::VERTEX | wgpu::ShaderStages::FRAGMENT,
                    ty: wgpu::BindingType::Buffer {
                        ty: wgpu::BufferBindingType::Uniform,
                        has_dynamic_offset: false,
                        min_binding_size: None,
                    },
                    count: None,
                },
                storage(1),
                storage(2),
                texture(3, D2Array),
            ],
        );
        let screen = layout(
            "composite screen",
            &[
                texture(0, D2Array),
                texture(1, D2),
                storage(2),
                texture(3, D2Array),
            ],
        );
        let fill = layout("composite fill", &[texture(0, D2Array), storage(4)]);
        let pipeline_layout = |groups: &[Option<&wgpu::BindGroupLayout>]| {
            device.create_pipeline_layout(&wgpu::PipelineLayoutDescriptor {
                label: Some("composite"),
                bind_group_layouts: groups,
                immediate_size: 0,
            })
        };
        let screen_layout = pipeline_layout(&[Some(&shared), Some(&screen)]);
        let fill_layout = pipeline_layout(&[Some(&shared), Some(&fill)]);
        let fill_pipeline = device.create_render_pipeline(&wgpu::RenderPipelineDescriptor {
            label: Some("cache fill"),
            layout: Some(&fill_layout),
            vertex: wgpu::VertexState {
                module: &shader,
                entry_point: Some("vs_fill"),
                compilation_options: Default::default(),
                buffers: &[],
            },
            fragment: Some(wgpu::FragmentState {
                module: &shader,
                entry_point: Some("fs_fill"),
                compilation_options: Default::default(),
                targets: &[Some(gpu.tile_format.wgpu().into())],
            }),
            primitive: Default::default(),
            depth_stencil: None,
            multisample: Default::default(),
            multiview_mask: None,
            cache: None,
        });
        let uniform = device.create_buffer(&wgpu::BufferDescriptor {
            label: Some("view"),
            size: std::mem::size_of::<ViewUniform>() as u64,
            usage: wgpu::BufferUsages::UNIFORM | wgpu::BufferUsages::COPY_DST,
            mapped_at_creation: false,
        });
        let program = Self::program_buffer(device, 1024);
        Self {
            gpu,
            shared,
            screen,
            fill,
            screen_layout,
            shader,
            pipelines: HashMap::new(),
            fill_pipeline,
            uniform,
            program,
            program_words: 1024,
        }
    }

    fn program_buffer(device: &wgpu::Device, words: usize) -> wgpu::Buffer {
        device.create_buffer(&wgpu::BufferDescriptor {
            label: Some("program"),
            size: words as u64 * 4,
            usage: wgpu::BufferUsages::STORAGE | wgpu::BufferUsages::COPY_DST,
            mapped_at_creation: false,
        })
    }

    fn pipeline(&mut self, format: wgpu::TextureFormat) -> &wgpu::RenderPipeline {
        let device = &self.gpu.device;
        self.pipelines.entry(format).or_insert_with(|| {
            device.create_render_pipeline(&wgpu::RenderPipelineDescriptor {
                label: Some("composite"),
                layout: Some(&self.screen_layout),
                vertex: wgpu::VertexState {
                    module: &self.shader,
                    entry_point: Some("vs"),
                    compilation_options: Default::default(),
                    buffers: &[],
                },
                fragment: Some(wgpu::FragmentState {
                    module: &self.shader,
                    entry_point: Some("fs"),
                    compilation_options: Default::default(),
                    targets: &[Some(format.into())],
                }),
                primitive: Default::default(),
                depth_stencil: None,
                multisample: Default::default(),
                multiview_mask: None,
                cache: None,
            })
        })
    }

    pub fn set_program(&mut self, canvas: &Canvas) {
        let words = canvas.program();
        if words.len() > self.program_words {
            self.program_words = words.len().next_power_of_two();
            self.program = Self::program_buffer(&self.gpu.device, self.program_words);
        }
        self.gpu
            .queue
            .write_buffer(&self.program, 0, bytemuck::cast_slice(&words));
    }

    /// Write the frame's view and bind what both passes share.
    pub fn begin(
        &self,
        canvas: &Canvas,
        atlas: &wgpu::TextureView,
        view: ViewUniform,
    ) -> wgpu::BindGroup {
        self.gpu
            .queue
            .write_buffer(&self.uniform, 0, bytemuck::bytes_of(&view));
        self.gpu
            .device
            .create_bind_group(&wgpu::BindGroupDescriptor {
                label: Some("composite shared"),
                layout: &self.shared,
                entries: &[
                    wgpu::BindGroupEntry {
                        binding: 0,
                        resource: self.uniform.as_entire_binding(),
                    },
                    wgpu::BindGroupEntry {
                        binding: 1,
                        resource: self.program.as_entire_binding(),
                    },
                    wgpu::BindGroupEntry {
                        binding: 2,
                        resource: canvas.tables.as_entire_binding(),
                    },
                    view_entry(3, atlas),
                ],
            })
    }

    /// Record cache-fill draws for instances `range` of `fills`.
    pub fn fill(
        &self,
        pass: &mut wgpu::RenderPass<'_>,
        shared: &wgpu::BindGroup,
        group: &wgpu::BindGroup,
        range: std::ops::Range<u32>,
    ) {
        pass.set_pipeline(&self.fill_pipeline);
        pass.set_bind_group(0, shared, &[]);
        pass.set_bind_group(1, group, &[]);
        pass.draw(0..6, range);
    }

    pub fn fill_group(&self, vectors: &wgpu::TextureView, fills: &wgpu::Buffer) -> wgpu::BindGroup {
        self.gpu
            .device
            .create_bind_group(&wgpu::BindGroupDescriptor {
                label: Some("composite fill"),
                layout: &self.fill,
                entries: &[
                    view_entry(0, vectors),
                    wgpu::BindGroupEntry {
                        binding: 4,
                        resource: fills.as_entire_binding(),
                    },
                ],
            })
    }

    /// Record the screen pass into `target`.
    #[allow(clippy::too_many_arguments)]
    pub fn draw(
        &mut self,
        encoder: &mut wgpu::CommandEncoder,
        target: &wgpu::TextureView,
        format: wgpu::TextureFormat,
        shared: &wgpu::BindGroup,
        vectors: &wgpu::TextureView,
        hud: &wgpu::TextureView,
        cache_table: &wgpu::Buffer,
        cache: &wgpu::TextureView,
    ) {
        let group = self
            .gpu
            .device
            .create_bind_group(&wgpu::BindGroupDescriptor {
                label: Some("composite screen"),
                layout: &self.screen,
                entries: &[
                    view_entry(0, vectors),
                    view_entry(1, hud),
                    wgpu::BindGroupEntry {
                        binding: 2,
                        resource: cache_table.as_entire_binding(),
                    },
                    view_entry(3, cache),
                ],
            });
        let pipeline = self.pipeline(format).clone();
        let mut pass = encoder.begin_render_pass(&wgpu::RenderPassDescriptor {
            label: Some("composite"),
            color_attachments: &[Some(wgpu::RenderPassColorAttachment {
                view: target,
                depth_slice: None,
                resolve_target: None,
                ops: wgpu::Operations {
                    load: wgpu::LoadOp::Clear(wgpu::Color::BLACK),
                    store: wgpu::StoreOp::Store,
                },
            })],
            ..Default::default()
        });
        pass.set_pipeline(&pipeline);
        pass.set_bind_group(0, shared, &[]);
        pass.set_bind_group(1, &group, &[]);
        pass.draw(0..3, 0..1);
    }
}
