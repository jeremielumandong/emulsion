//! Render the actual GPUI tile shader against a tightly packed atlas. Canvas
//! tiles must not pick up adjacent images when zoom or display scale magnifies
//! their edge texels.
use wgpu::util::DeviceExt;

const SHADER: &str = concat!(
    include_str!("../../../vendor/gpui/gpui-pre-wgpu/src/shaders.wgsl"),
    include_str!("../../../vendor/gpui/gpui-pre-wgpu/src/shaders_storage.wgsl"),
);

#[test]
fn canvas_tile_edges_do_not_sample_neighboring_atlas_images() {
    let Some(gpu) = crate::test_gpu() else { return };
    gpu.with_device(|device, queue| {
        let shader = device.create_shader_module(wgpu::ShaderModuleDescriptor {
            label: Some("GPUI canvas tile regression"),
            source: wgpu::ShaderSource::Wgsl(SHADER.into()),
        });
        let layout = |entries: &[wgpu::BindGroupLayoutEntry]| {
            device.create_bind_group_layout(&wgpu::BindGroupLayoutDescriptor {
                label: None,
                entries,
            })
        };
        let buffer_layout = |ty| {
            layout(&[wgpu::BindGroupLayoutEntry {
                binding: 0,
                visibility: wgpu::ShaderStages::VERTEX_FRAGMENT,
                ty: wgpu::BindingType::Buffer {
                    ty,
                    has_dynamic_offset: false,
                    min_binding_size: None,
                },
                count: None,
            }])
        };
        let globals_layout = buffer_layout(wgpu::BufferBindingType::Uniform);
        let sprites_layout = buffer_layout(wgpu::BufferBindingType::Storage { read_only: true });
        let images_layout = layout(&[
            wgpu::BindGroupLayoutEntry {
                binding: 0,
                visibility: wgpu::ShaderStages::VERTEX_FRAGMENT,
                ty: wgpu::BindingType::Texture {
                    sample_type: wgpu::TextureSampleType::Float { filterable: true },
                    view_dimension: wgpu::TextureViewDimension::D2,
                    multisampled: false,
                },
                count: None,
            },
            wgpu::BindGroupLayoutEntry {
                binding: 1,
                visibility: wgpu::ShaderStages::FRAGMENT,
                ty: wgpu::BindingType::Sampler(wgpu::SamplerBindingType::Filtering),
                count: None,
            },
        ]);
        let pipeline_layout = device.create_pipeline_layout(&wgpu::PipelineLayoutDescriptor {
            label: None,
            bind_group_layouts: &[
                Some(&globals_layout),
                Some(&sprites_layout),
                Some(&images_layout),
            ],
            immediate_size: 0,
        });
        let pipeline = device.create_render_pipeline(&wgpu::RenderPipelineDescriptor {
            label: None,
            layout: Some(&pipeline_layout),
            vertex: wgpu::VertexState {
                module: &shader,
                entry_point: Some("vs_poly_sprite"),
                compilation_options: Default::default(),
                buffers: &[],
            },
            primitive: wgpu::PrimitiveState {
                topology: wgpu::PrimitiveTopology::TriangleStrip,
                ..Default::default()
            },
            depth_stencil: None,
            multisample: Default::default(),
            fragment: Some(wgpu::FragmentState {
                module: &shader,
                entry_point: Some("fs_poly_sprite"),
                compilation_options: Default::default(),
                targets: &[Some(wgpu::ColorTargetState {
                    format: wgpu::TextureFormat::Rgba8Unorm,
                    blend: None,
                    write_mask: wgpu::ColorWrites::ALL,
                })],
            }),
            multiview_mask: None,
            cache: None,
        });
        let texture = |width, usage| {
            device.create_texture(&wgpu::TextureDescriptor {
                label: None,
                size: wgpu::Extent3d {
                    width,
                    height: width,
                    depth_or_array_layers: 1,
                },
                mip_level_count: 1,
                sample_count: 1,
                dimension: wgpu::TextureDimension::D2,
                format: wgpu::TextureFormat::Rgba8Unorm,
                usage,
                view_formats: &[],
            })
        };
        for tile_color in [[255u8; 4], [0, 0, 0, 255]] {
            let atlas = texture(
                260,
                wgpu::TextureUsages::TEXTURE_BINDING | wgpu::TextureUsages::COPY_DST,
            );
            // Opposite-colored neighboring atlas images must not bleed into the tile.
            let mut pixels = vec![0u8; 260 * 260 * 4];
            for y in 0..260 {
                for x in 0..260 {
                    let color = if (2..258).contains(&x) && (2..258).contains(&y) {
                        tile_color
                    } else {
                        [
                            255 - tile_color[0],
                            255 - tile_color[1],
                            255 - tile_color[2],
                            255,
                        ]
                    };
                    pixels[(y * 260 + x) * 4..(y * 260 + x + 1) * 4].copy_from_slice(&color);
                }
            }
            queue.write_texture(
                atlas.as_image_copy(),
                &pixels,
                wgpu::TexelCopyBufferLayout {
                    offset: 0,
                    bytes_per_row: Some(260 * 4),
                    rows_per_image: None,
                },
                atlas.size(),
            );
            let atlas_view = atlas.create_view(&Default::default());
            let sampler = device.create_sampler(&wgpu::SamplerDescriptor {
                mag_filter: wgpu::FilterMode::Linear,
                min_filter: wgpu::FilterMode::Linear,
                ..Default::default()
            });
            let images = device.create_bind_group(&wgpu::BindGroupDescriptor {
                label: None,
                layout: &pipeline.get_bind_group_layout(2),
                entries: &[
                    wgpu::BindGroupEntry {
                        binding: 0,
                        resource: wgpu::BindingResource::TextureView(&atlas_view),
                    },
                    wgpu::BindGroupEntry {
                        binding: 1,
                        resource: wgpu::BindingResource::Sampler(&sampler),
                    },
                ],
            });
            // 81% zoom at 1x and 1.333x display scale, plus the magnified tile
            // fallback shown while the crisp screen image settles.
            for width in [207u32, 277, 384] {
                let target = texture(
                    width,
                    wgpu::TextureUsages::RENDER_ATTACHMENT | wgpu::TextureUsages::COPY_SRC,
                );
                let target_view = target.create_view(&Default::default());
                let buffer = |words: &[u32], usage| {
                    device.create_buffer_init(&wgpu::util::BufferInitDescriptor {
                        label: None,
                        contents: bytemuck::cast_slice(words),
                        usage,
                    })
                };
                let globals = buffer(
                    &[(width as f32).to_bits(), (width as f32).to_bits(), 0, 0],
                    wgpu::BufferUsages::UNIFORM,
                );
                let mut sprite = [0u32; 24];
                sprite[3] = 1f32.to_bits(); // opacity
                for index in [6, 7, 10, 11] {
                    sprite[index] = (width as f32).to_bits();
                }
                sprite[20..24].copy_from_slice(&[2, 2, 256, 256]); // atlas bounds
                let sprites = buffer(&sprite, wgpu::BufferUsages::STORAGE);
                let bind = |index, buf: &wgpu::Buffer| {
                    device.create_bind_group(&wgpu::BindGroupDescriptor {
                        label: None,
                        layout: &pipeline.get_bind_group_layout(index),
                        entries: &[wgpu::BindGroupEntry {
                            binding: 0,
                            resource: buf.as_entire_binding(),
                        }],
                    })
                };
                let global_group = bind(0, &globals);
                let sprite_group = bind(1, &sprites);
                let stride = (width * 4).div_ceil(256) * 256;
                let readback = device.create_buffer(&wgpu::BufferDescriptor {
                    label: None,
                    size: (stride * width) as u64,
                    usage: wgpu::BufferUsages::COPY_DST | wgpu::BufferUsages::MAP_READ,
                    mapped_at_creation: false,
                });
                let mut encoder = device.create_command_encoder(&Default::default());
                {
                    let mut pass = encoder.begin_render_pass(&wgpu::RenderPassDescriptor {
                        color_attachments: &[Some(wgpu::RenderPassColorAttachment {
                            view: &target_view,
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
                    pass.set_bind_group(0, &global_group, &[]);
                    pass.set_bind_group(1, &sprite_group, &[]);
                    pass.set_bind_group(2, &images, &[]);
                    pass.draw(0..4, 0..1);
                }
                encoder.copy_texture_to_buffer(
                    target.as_image_copy(),
                    wgpu::TexelCopyBufferInfo {
                        buffer: &readback,
                        layout: wgpu::TexelCopyBufferLayout {
                            offset: 0,
                            bytes_per_row: Some(stride),
                            rows_per_image: None,
                        },
                    },
                    target.size(),
                );
                let submission = queue.submit([encoder.finish()]);
                let (tx, rx) = std::sync::mpsc::sync_channel(1);
                readback
                    .slice(..)
                    .map_async(wgpu::MapMode::Read, move |result| {
                        tx.send(result).unwrap();
                    });
                device.poll(wgpu::PollType::Wait {
                    submission_index: Some(submission),
                    timeout: Some(std::time::Duration::from_secs(10)),
                })?;
                rx.recv_timeout(std::time::Duration::from_secs(10))??;
                let data = readback.slice(..).get_mapped_range();
                for y in 0..width as usize {
                    for x in 0..width as usize {
                        let i = y * stride as usize + x * 4;
                        assert_eq!(
                            &data[i..i + 4],
                            &tile_color,
                            "atlas bleed at ({x}, {y}), tile width {width}"
                        );
                    }
                }
                drop(data);
                readback.unmap();
            }
        }
        Ok(())
    })
    .unwrap();
}
