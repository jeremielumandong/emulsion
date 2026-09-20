//! Experimental persistent GPU dry-brush engine, used by the opt-in brush router.
//!
//! Base pixels are uploaded once; subsequent submissions upload only ordered dab
//! descriptors. CPU readback happens only at preview boundaries. Callers must
//! retain their stroke journal for recovery: a device error invalidates a session.
use crate::GpuContext;
use anyhow::{Context, Result, bail, ensure};
use std::{sync::Arc, time::Duration};

const MAX_DABS: usize = 1024;

/// Resolved circular dab. Color is premultiplied linear RGBA in 0..=1.
#[derive(Clone, Copy, Debug)]
pub struct Dab {
    pub center: [f32; 2],
    pub radius: f32,
    pub hardness: f32,
    pub flow: f32,
    pub color: [f32; 4],
}
impl Dab {
    fn validate(self) -> Result<()> {
        ensure!(
            self.center
                .iter()
                .all(|v| v.is_finite() && v.abs() <= 1_000_000.0),
            "Invalid dab center"
        );
        ensure!(
            self.radius.is_finite() && (0.3..=4096.0).contains(&self.radius),
            "Invalid dab radius"
        );
        ensure!(
            [self.hardness, self.flow]
                .iter()
                .chain(self.color.iter())
                .all(|v| v.is_finite() && (0.0..=1.0).contains(v)),
            "Invalid dab values"
        );
        ensure!(
            self.color[..3].iter().all(|&c| c <= self.color[3]),
            "Color must be premultiplied"
        );
        Ok(())
    }
    fn packed(self) -> [f32; 12] {
        [
            self.center[0],
            self.center[1],
            self.radius,
            self.hardness,
            self.color[0],
            self.color[1],
            self.color[2],
            self.color[3],
            self.flow,
            0.0,
            0.0,
            0.0,
        ]
    }
}

/// Bounded prototype: dimensions up to 1024, about 40 MiB maximum per session.
/// Unsupported brush features stay on CPU. The raster router owns recovery and
/// returns CPU rasters to the existing editor history.
pub struct PersistentPaint {
    gpu: Arc<GpuContext>,
    width: u32,
    height: u32,
    opacity: f32,
    params: wgpu::Buffer,
    dabs: wgpu::Buffer,
    output: wgpu::Buffer,
    readback: wgpu::Buffer,
    pipeline: wgpu::ComputePipeline,
    group: wgpu::BindGroup,
    valid: bool,
}

fn scoped<T>(device: &wgpu::Device, f: impl FnOnce() -> Result<T>) -> Result<T> {
    let validation = device.push_error_scope(wgpu::ErrorFilter::Validation);
    let oom = device.push_error_scope(wgpu::ErrorFilter::OutOfMemory);
    let result = f();
    let error = pollster::block_on(oom.pop()).or(pollster::block_on(validation.pop()));
    if let Some(error) = error {
        bail!("Persistent paint GPU error: {error}");
    }
    result
}
fn upload(queue: &wgpu::Queue, buffer: &wgpu::Buffer, data: &[u8]) -> Result<()> {
    let mut staging = queue
        .write_buffer_with(
            buffer,
            0,
            wgpu::BufferSize::new(data.len() as u64).context("Empty upload")?,
        )
        .context("GPU staging allocation failed")?;
    staging.copy_from_slice(data);
    Ok(())
}
fn wait(device: &wgpu::Device, submission: wgpu::SubmissionIndex) -> Result<()> {
    device
        .poll(wgpu::PollType::Wait {
            submission_index: Some(submission),
            timeout: Some(Duration::from_secs(2)),
        })
        .context("Persistent paint GPU timeout")?;
    Ok(())
}
impl PersistentPaint {
    pub fn new(
        gpu: Arc<GpuContext>,
        width: u32,
        height: u32,
        base: &[[u16; 4]],
        opacity: f32,
    ) -> Result<Self> {
        ensure!(
            width > 0 && height > 0 && width <= 1024 && height <= 1024,
            "Prototype dimensions must be 1..=1024"
        );
        ensure!(
            base.len() == width as usize * height as usize,
            "Base dimensions mismatch"
        );
        ensure!(
            opacity.is_finite() && (0.0..=1.0).contains(&opacity),
            "Invalid opacity"
        );
        ensure!(
            base.iter().all(|p| p[..3].iter().all(|v| *v <= p[3])),
            "Base must be premultiplied"
        );
        let (params, dabs, output, readback, pipeline, group) =
            gpu.with_device(|device, queue| {
                let pixels = u64::from(width) * u64::from(height);
                let (params, base_buffer, dabs, accum, output, readback) = scoped(device, || {
                    let buffer = |size, usage| {
                        device.create_buffer(&wgpu::BufferDescriptor {
                            label: Some("Persistent paint"),
                            size,
                            usage,
                            mapped_at_creation: false,
                        })
                    };
                    let input = wgpu::BufferUsages::STORAGE | wgpu::BufferUsages::COPY_DST;
                    Ok((
                        buffer(16, input),
                        buffer(pixels * 8, input),
                        buffer(MAX_DABS as u64 * 48, input),
                        buffer(pixels * 16, wgpu::BufferUsages::STORAGE),
                        buffer(pixels * 8, input | wgpu::BufferUsages::COPY_SRC),
                        buffer(
                            pixels * 8,
                            wgpu::BufferUsages::COPY_DST | wgpu::BufferUsages::MAP_READ,
                        ),
                    ))
                })?;
                let (pipeline, group) = scoped(device, || {
                    let module = device.create_shader_module(wgpu::ShaderModuleDescriptor {
                        label: Some("Persistent dry brush"),
                        source: wgpu::ShaderSource::Wgsl(
                            include_str!("persistent_paint.wgsl").into(),
                        ),
                    });
                    let pipeline =
                        device.create_compute_pipeline(&wgpu::ComputePipelineDescriptor {
                            label: Some("Persistent dry brush"),
                            layout: None,
                            module: &module,
                            entry_point: Some("main"),
                            compilation_options: Default::default(),
                            cache: None,
                        });
                    Ok(pipeline)
                })
                .and_then(|pipeline| {
                    scoped(device, || {
                        let entries: Vec<_> = [&params, &base_buffer, &dabs, &accum, &output]
                            .into_iter()
                            .enumerate()
                            .map(|(i, b)| wgpu::BindGroupEntry {
                                binding: i as u32,
                                resource: b.as_entire_binding(),
                            })
                            .collect();
                        let group = device.create_bind_group(&wgpu::BindGroupDescriptor {
                            label: Some("Persistent dry brush"),
                            layout: &pipeline.get_bind_group_layout(0),
                            entries: &entries,
                        });
                        Ok((pipeline, group))
                    })
                })?;
                scoped(device, || {
                    upload(queue, &base_buffer, bytemuck::cast_slice(base))?;
                    upload(queue, &output, bytemuck::cast_slice(base))?;
                    wait(device, queue.submit([]))
                })?;
                Ok((params, dabs, output, readback, pipeline, group))
            })?;
        Ok(Self {
            gpu,
            width,
            height,
            opacity,
            params,
            dabs,
            output,
            readback,
            pipeline,
            group,
            valid: true,
        })
    }
    /// Submits only new dabs in order; the accumulation stays on the GPU.
    /// Invalid input is rejected before any mutation. GPU errors invalidate the session.
    pub fn append(&mut self, dabs: &[Dab]) -> Result<()> {
        ensure!(
            self.valid,
            "Persistent paint session invalidated; replay on CPU"
        );
        ensure!(dabs.len() <= MAX_DABS, "Too many dabs in one submission");
        for dab in dabs {
            dab.validate()?;
        }
        if dabs.is_empty() {
            return Ok(());
        }
        let packed: Vec<_> = dabs.iter().map(|d| d.packed()).collect();
        let result = self.gpu.with_device(|device, queue| {
            scoped(device, || {
                upload(queue, &self.dabs, bytemuck::cast_slice(&packed))?;
                upload(
                    queue,
                    &self.params,
                    bytemuck::cast_slice(&[
                        self.width,
                        self.height,
                        dabs.len() as u32,
                        self.opacity.to_bits(),
                    ]),
                )?;
                let mut encoder =
                    device.create_command_encoder(&wgpu::CommandEncoderDescriptor::default());
                {
                    let mut pass =
                        encoder.begin_compute_pass(&wgpu::ComputePassDescriptor::default());
                    pass.set_pipeline(&self.pipeline);
                    pass.set_bind_group(0, &self.group, &[]);
                    pass.dispatch_workgroups((self.width * self.height).div_ceil(64), 1, 1);
                }
                wait(device, queue.submit([encoder.finish()]))
            })
        });
        if result.is_err() {
            self.valid = false;
        }
        result
    }
    /// Downloads premultiplied linear RGBA16 for the existing CPU document path.
    pub fn preview(&mut self) -> Result<Vec<[u16; 4]>> {
        ensure!(
            self.valid,
            "Persistent paint session invalidated; replay on CPU"
        );
        let result = self.gpu.with_device(|device, queue| {
            scoped(device, || {
                let mut encoder =
                    device.create_command_encoder(&wgpu::CommandEncoderDescriptor::default());
                encoder.copy_buffer_to_buffer(
                    &self.output,
                    0,
                    &self.readback,
                    0,
                    u64::from(self.width) * u64::from(self.height) * 8,
                );
                let submission = queue.submit([encoder.finish()]);
                let (tx, rx) = std::sync::mpsc::sync_channel(1);
                self.readback
                    .slice(..)
                    .map_async(wgpu::MapMode::Read, move |result| {
                        let _ = tx.send(result);
                    });
                if let Err(error) = wait(device, submission) {
                    self.readback.unmap();
                    return Err(error);
                }
                let mapped = rx
                    .recv_timeout(Duration::from_secs(2))
                    .context("Readback callback timeout")
                    .and_then(|r| r.context("Readback mapping failed"));
                if let Err(error) = mapped {
                    self.readback.unmap();
                    return Err(error);
                }
                let data = self.readback.slice(..).get_mapped_range();
                let result = bytemuck::cast_slice::<u8, [u16; 4]>(&data).to_vec();
                drop(data);
                self.readback.unmap();
                Ok(result)
            })
        });
        if result.is_err() {
            self.valid = false;
        }
        result
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    fn reference(
        width: u32,
        height: u32,
        base: &[[u16; 4]],
        opacity: f32,
        dabs: &[Dab],
    ) -> Vec<[u16; 4]> {
        let mut paint = vec![[0.0f32; 4]; base.len()];
        let falloff = |d: f32, h: f32| {
            let h = h.min(0.99);
            if d >= 1.0 {
                0.0
            } else if d <= h {
                1.0
            } else {
                let t = (d - h) / (1.0 - h);
                1.0 - t * t * (3.0 - 2.0 * t)
            }
        };
        // Bound CPU rasterization to each dab, as the editor's stamp does.
        for dab in dabs {
            let r = dab.radius;
            let left = (dab.center[0] - r).floor() as i32;
            let top = (dab.center[1] - r).floor() as i32;
            let extent = (r * 2.0).ceil() as i32 + 2;
            for y in top.max(0)..(top + extent).min(height as i32) {
                for x in left.max(0)..(left + extent).min(width as i32) {
                    let dx = x as f32 + 0.5 - dab.center[0];
                    let dy = y as f32 + 0.5 - dab.center[1];
                    let d = (dx * dx + dy * dy).sqrt() / r;
                    let footprint = std::f32::consts::FRAC_1_SQRT_2 / r;
                    let mut shape = falloff(d, dab.hardness);
                    if ((1.0 - dab.hardness) * r < 1.0 || r < 2.0)
                        && d + footprint > dab.hardness
                        && d - footprint < 1.0
                    {
                        shape = 0.0;
                        for sy in [-0.375, -0.125, 0.125, 0.375] {
                            for sx in [-0.375, -0.125, 0.125, 0.375] {
                                shape += falloff((dx + sx).hypot(dy + sy) / r, dab.hardness);
                            }
                        }
                        shape /= 16.0;
                    }
                    let a = shape * dab.flow;
                    if a <= 0.0005 {
                        continue;
                    }
                    let pixel = &mut paint[(y as u32 * width + x as u32) as usize];
                    for (i, v) in pixel.iter_mut().enumerate() {
                        *v = dab.color[i] * a + *v * (1.0 - a);
                    }
                }
            }
        }
        base.iter()
            .zip(paint)
            .map(|(base, p)| {
                std::array::from_fn(|i| {
                    ((p[i] * opacity + base[i] as f32 * (1.0 / 65535.0) * (1.0 - p[3] * opacity))
                        .clamp(0.0, 1.0)
                        * 65535.0
                        + 0.5) as u16
                })
            })
            .collect()
    }
    fn assert_close(a: &[[u16; 4]], b: &[[u16; 4]]) {
        assert_eq!(a.len(), b.len());
        for (i, (a, b)) in a.iter().zip(b).enumerate() {
            for c in 0..4 {
                assert!(
                    a[c].abs_diff(b[c]) <= 2,
                    "pixel {i} channel {c}: {} != {}",
                    a[c],
                    b[c]
                );
            }
        }
    }
    #[test]
    fn incremental_order_transparency_validation_and_journal_recovery() {
        let Some(gpu) = crate::test_gpu() else { return };
        let base = vec![[1200, 5000, 2000, 20000]; 96 * 64];
        let dabs = [
            Dab {
                center: [20.25, 25.75],
                radius: 17.0,
                hardness: 0.4,
                flow: 0.6,
                color: [0.4, 0.1, 0.0, 0.5],
            },
            Dab {
                center: [28.1, 28.3],
                radius: 22.0,
                hardness: 1.0,
                flow: 0.9,
                color: [0.0, 0.2, 0.6, 0.7],
            },
            Dab {
                center: [40.2, 28.5],
                radius: 0.5,
                hardness: 0.7,
                flow: 0.8,
                color: [0.8, 0.0, 0.0, 0.8],
            },
            Dab {
                center: [-0.2, 0.3],
                radius: 2.3,
                hardness: 1.0,
                flow: 1.0,
                color: [0.0, 0.5, 0.0, 0.5],
            },
        ];
        let mut stroke = PersistentPaint::new(gpu.clone(), 96, 64, &base, 0.73).unwrap();
        assert_eq!(stroke.preview().unwrap(), base);
        for end in 1..=dabs.len() {
            stroke.append(&dabs[end - 1..end]).unwrap();
            assert_close(
                &stroke.preview().unwrap(),
                &reference(96, 64, &base, 0.73, &dabs[..end]),
            );
        }
        let before = stroke.preview().unwrap();
        let mut invalid = dabs[0];
        invalid.flow = f32::NAN;
        assert!(stroke.append(&[dabs[0], invalid]).is_err());
        assert_eq!(stroke.preview().unwrap(), before);
        assert!(stroke.append(&vec![dabs[0]; MAX_DABS + 1]).is_err());
        assert_eq!(stroke.preview().unwrap(), before);
        assert!(PersistentPaint::new(gpu.clone(), 1025, 1, &[], 1.0).is_err());
        assert!(PersistentPaint::new(gpu, 96, 64, &base, f32::INFINITY).is_err());
        // Simulate session invalidation: caller's retained resolved-dab journal
        // reconstructs the same stroke without relying on any GPU state.
        stroke.valid = false;
        assert!(stroke.append(&dabs[..1]).is_err());
        assert!(stroke.preview().is_err());
        assert_close(&before, &reference(96, 64, &base, 0.73, &dabs));
    }
}
