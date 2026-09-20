use anyhow::{Context, Result, bail, ensure};
use std::collections::{HashMap, HashSet};
use std::sync::atomic::{AtomicBool, AtomicU64, Ordering};
use std::sync::{Arc, Mutex};
use std::time::Duration;

const MAX_DISPATCH_BYTES: usize = 256 * 1024 * 1024;
const MAX_STAGED_DISPATCH_BYTES: usize = 384 * 1024 * 1024;

/// Shared compute queue; results are committed only after successful readback.
pub struct GpuContext {
    device: wgpu::Device,
    queue: wgpu::Queue,
    name: String,
    failed: Arc<AtomicBool>,
    state: Mutex<State>,
    tile_preparation: Mutex<()>,
    dispatches: AtomicU64,
}

#[derive(Default)]
struct State {
    pipelines: HashMap<&'static str, wgpu::ComputePipeline>,
    unsupported: HashSet<&'static str>,
}

impl GpuContext {
    pub fn new() -> Result<Self> {
        let software = std::env::var("EMULSION_GPU").as_deref() == Ok("software");
        ensure!(
            std::env::var("EMULSION_GPU").as_deref() != Ok("cpu"),
            "CPU requested"
        );
        let instance = wgpu::Instance::new(wgpu::InstanceDescriptor::new_without_display_handle());
        let mut adapters = pollster::block_on(instance.enumerate_adapters(wgpu::Backends::all()));
        adapters.retain(|adapter| {
            (adapter.get_info().device_type == wgpu::DeviceType::Cpu) == software
        });
        adapters.sort_by_key(|adapter| match adapter.get_info().device_type {
            wgpu::DeviceType::DiscreteGpu => 0,
            wgpu::DeviceType::IntegratedGpu => 1,
            wgpu::DeviceType::VirtualGpu => 2,
            wgpu::DeviceType::Other => 3,
            wgpu::DeviceType::Cpu => 4,
        });
        for adapter in adapters {
            let limits = wgpu::Limits::default();
            if !limits.check_limits(&adapter.limits()) {
                continue;
            }
            let result = pollster::block_on(adapter.request_device(&wgpu::DeviceDescriptor {
                label: Some("Emulsion image compute"),
                required_features: wgpu::Features::empty(),
                required_limits: limits,
                memory_hints: wgpu::MemoryHints::MemoryUsage,
                trace: wgpu::Trace::Off,
                experimental_features: wgpu::ExperimentalFeatures::disabled(),
            }));
            let (device, queue) = match result {
                Ok(device) => device,
                Err(error) => {
                    tracing::warn!(%error, "Compute device rejected; trying next adapter");
                    continue;
                }
            };
            let failed = Arc::new(AtomicBool::new(false));
            device.set_device_lost_callback({
                let failed = failed.clone();
                move |reason, message| {
                    failed.store(true, Ordering::Relaxed);
                    if reason != wgpu::DeviceLostReason::Destroyed {
                        tracing::warn!(?reason, %message, "Compute device lost; using CPU");
                    }
                }
            });
            device.on_uncaptured_error(Arc::new({
                let failed = failed.clone();
                move |error| {
                    failed.store(true, Ordering::Relaxed);
                    tracing::warn!(%error, "Compute device error; using CPU");
                }
            }));
            return Ok(Self {
                device,
                queue,
                name: adapter.get_info().name,
                failed,
                state: Mutex::new(State::default()),
                tile_preparation: Mutex::new(()),
                dispatches: AtomicU64::new(0),
            });
        }
        bail!(
            "No suitable {} compute adapter",
            if software { "software" } else { "hardware" }
        )
    }

    pub fn name(&self) -> &str {
        &self.name
    }
    pub fn available(&self) -> bool {
        !self.failed.load(Ordering::Relaxed)
    }
    pub fn dispatch_count(&self) -> u64 {
        self.dispatches.load(Ordering::Relaxed)
    }

    pub(crate) fn tile_permit(&self) -> Option<std::sync::MutexGuard<'_, ()>> {
        self.tile_preparation.try_lock().ok()
    }

    /// Read-only input storage bindings followed by one read-write output.
    /// Large jobs dispatch in 2D; kernels flatten using num_workgroups.x * 64.
    pub(crate) fn run(
        &self,
        key: &'static str,
        shader: &str,
        inputs: &[&[u8]],
        output_bytes: usize,
        workgroups: u32,
    ) -> Result<Vec<u8>> {
        ensure!(self.available(), "Compute device disabled");
        validate_dispatch(inputs, output_bytes, workgroups, &self.device.limits())?;
        let mut state = self
            .state
            .lock()
            .map_err(|_| anyhow::anyhow!("Compute lock poisoned"))?;
        ensure!(self.available(), "Compute device disabled");
        ensure!(
            !state.unsupported.contains(key),
            "Shader disabled after previous failure"
        );
        let result = self.run_locked(&mut state, key, shader, inputs, output_bytes, workgroups);
        if let Err(error) = &result {
            #[cfg(test)]
            eprintln!("GPU operation {key} failed: {error:#}");
            state.unsupported.insert(key);
            tracing::warn!(operation = key, %error, "GPU operation failed; using CPU for this operation");
        }
        result
    }

    fn run_locked(
        &self,
        state: &mut State,
        key: &'static str,
        shader: &str,
        inputs: &[&[u8]],
        output_bytes: usize,
        workgroups: u32,
    ) -> Result<Vec<u8>> {
        if !state.pipelines.contains_key(key) {
            let validation = self.device.push_error_scope(wgpu::ErrorFilter::Validation);
            let module = self
                .device
                .create_shader_module(wgpu::ShaderModuleDescriptor {
                    label: Some(key),
                    source: wgpu::ShaderSource::Wgsl(shader.into()),
                });
            if let Some(error) = pollster::block_on(validation.pop()) {
                bail!("Shader validation: {error}");
            }
            let validation = self.device.push_error_scope(wgpu::ErrorFilter::Validation);
            let pipeline = self
                .device
                .create_compute_pipeline(&wgpu::ComputePipelineDescriptor {
                    label: Some(key),
                    layout: None,
                    module: &module,
                    entry_point: Some("main"),
                    compilation_options: Default::default(),
                    cache: None,
                });
            if let Some(error) = pollster::block_on(validation.pop()) {
                bail!("Pipeline validation: {error}");
            }
            state.pipelines.insert(key, pipeline);
        }
        let pipeline = &state.pipelines[key];
        let validation = self.device.push_error_scope(wgpu::ErrorFilter::Validation);
        let oom = self.device.push_error_scope(wgpu::ErrorFilter::OutOfMemory);
        let buffers: Vec<_> = inputs
            .iter()
            .map(|contents| {
                self.device.create_buffer(&wgpu::BufferDescriptor {
                    label: Some(key),
                    size: contents.len() as u64,
                    usage: wgpu::BufferUsages::STORAGE | wgpu::BufferUsages::COPY_DST,
                    mapped_at_creation: false,
                })
            })
            .collect();
        let output = self.device.create_buffer(&wgpu::BufferDescriptor {
            label: Some(key),
            size: output_bytes as u64,
            usage: wgpu::BufferUsages::STORAGE | wgpu::BufferUsages::COPY_SRC,
            mapped_at_creation: false,
        });
        let readback = self.device.create_buffer(&wgpu::BufferDescriptor {
            label: Some("Emulsion readback"),
            size: output_bytes as u64,
            usage: wgpu::BufferUsages::COPY_DST | wgpu::BufferUsages::MAP_READ,
            mapped_at_creation: false,
        });
        // Check allocations before touching buffers. create_buffer_init maps
        // immediately and can panic on a failed allocation despite error scopes.
        let oom = pollster::block_on(oom.pop());
        let validation = pollster::block_on(validation.pop());
        if let Some(error) = oom.or(validation) {
            bail!("Compute buffer allocation: {error}");
        }
        ensure!(self.available(), "Compute device lost during allocation");

        let validation = self.device.push_error_scope(wgpu::ErrorFilter::Validation);
        let oom = self.device.push_error_scope(wgpu::ErrorFilter::OutOfMemory);
        for (buffer, contents) in buffers.iter().zip(inputs) {
            // Unlike an infallible mapped-range accessor, this returns None if
            // validation or staging allocation fails (including device loss).
            let size =
                wgpu::BufferSize::new(contents.len() as u64).context("Empty compute upload")?;
            let Some(mut upload) = self.queue.write_buffer_with(buffer, 0, size) else {
                // Release any earlier queued staging writes even when this
                // operation falls back and no later GPU work is submitted.
                let submission = self.queue.submit([]);
                if self
                    .device
                    .poll(wgpu::PollType::Wait {
                        submission_index: Some(submission),
                        timeout: Some(Duration::from_secs(2)),
                    })
                    .is_err()
                {
                    self.failed.store(true, Ordering::Relaxed);
                }
                let oom = pollster::block_on(oom.pop());
                let validation = pollster::block_on(validation.pop());
                bail!("Compute upload allocation failed: {:?}", oom.or(validation));
            };
            upload.copy_from_slice(contents);
        }
        let entries: Vec<_> = buffers
            .iter()
            .chain(std::iter::once(&output))
            .enumerate()
            .map(|(binding, buffer)| wgpu::BindGroupEntry {
                binding: binding as u32,
                resource: buffer.as_entire_binding(),
            })
            .collect();
        let group = self.device.create_bind_group(&wgpu::BindGroupDescriptor {
            label: Some(key),
            layout: &pipeline.get_bind_group_layout(0),
            entries: &entries,
        });
        let mut encoder = self
            .device
            .create_command_encoder(&wgpu::CommandEncoderDescriptor { label: Some(key) });
        {
            let mut pass = encoder.begin_compute_pass(&wgpu::ComputePassDescriptor {
                label: Some(key),
                timestamp_writes: None,
            });
            pass.set_pipeline(pipeline);
            pass.set_bind_group(0, &group, &[]);
            let x = workgroups.min(self.device.limits().max_compute_workgroups_per_dimension);
            pass.dispatch_workgroups(x, workgroups.div_ceil(x), 1);
        }
        encoder.copy_buffer_to_buffer(&output, 0, &readback, 0, output_bytes as u64);
        let submission = self.queue.submit([encoder.finish()]);
        let (tx, rx) = std::sync::mpsc::sync_channel(1);
        readback
            .slice(..)
            .map_async(wgpu::MapMode::Read, move |result| {
                let _ = tx.send(result);
            });
        let polled = self.device.poll(wgpu::PollType::Wait {
            submission_index: Some(submission),
            timeout: Some(Duration::from_secs(2)),
        });
        let oom = pollster::block_on(oom.pop());
        let validation = pollster::block_on(validation.pop());
        if let Err(error) = polled {
            self.failed.store(true, Ordering::Relaxed);
            bail!("Compute device did not complete: {error}");
        }
        if let Some(error) = oom.or(validation) {
            bail!("Compute dispatch: {error}");
        }
        rx.recv_timeout(Duration::from_secs(2))
            .context("Readback timed out")??;
        let bytes = readback.slice(..).get_mapped_range().to_vec();
        readback.unmap();
        self.dispatches.fetch_add(1, Ordering::Relaxed);
        Ok(bytes)
    }
}

fn validate_dispatch(
    inputs: &[&[u8]],
    output: usize,
    groups: u32,
    limits: &wgpu::Limits,
) -> Result<()> {
    ensure!(
        groups > 0 && output > 0 && output.is_multiple_of(4),
        "Empty or unaligned output"
    );
    ensure!(
        inputs.len() < limits.max_storage_buffers_per_shader_stage as usize,
        "Too many storage buffers"
    );
    let mut input_bytes = 0usize;
    for input in inputs {
        ensure!(
            !input.is_empty() && input.len().is_multiple_of(4),
            "Empty or unaligned input"
        );
        ensure!(
            input.len() <= limits.max_storage_buffer_binding_size as usize,
            "Input exceeds device limit"
        );
        input_bytes = input_bytes
            .checked_add(input.len())
            .context("Input size overflow")?;
    }
    ensure!(
        output <= limits.max_storage_buffer_binding_size as usize,
        "Output exceeds device limit"
    );
    validate_memory_budget(input_bytes, output)?;
    ensure!(
        groups.div_ceil(limits.max_compute_workgroups_per_dimension)
            <= limits.max_compute_workgroups_per_dimension,
        "Dispatch exceeds workgroup limits"
    );
    Ok(())
}

fn validate_memory_budget(inputs: usize, output: usize) -> Result<()> {
    let primary = output
        .checked_mul(2)
        .and_then(|bytes| bytes.checked_add(inputs))
        .context("Dispatch size overflow")?;
    let staged = primary
        .checked_add(inputs)
        .context("Staging size overflow")?;
    ensure!(
        primary <= MAX_DISPATCH_BYTES,
        "Dispatch exceeds primary memory budget"
    );
    ensure!(
        staged <= MAX_STAGED_DISPATCH_BYTES,
        "Dispatch exceeds staging memory budget"
    );
    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;
    #[test]
    fn rejects_malformed_and_oversized_jobs_before_allocation() {
        let limits = wgpu::Limits::default();
        assert!(validate_dispatch(&[&[0; 16]], 16, 1, &limits).is_ok());
        assert!(validate_dispatch(&[&[]], 16, 1, &limits).is_err());
        assert!(validate_dispatch(&[&[0; 3]], 16, 1, &limits).is_err());
        assert!(validate_dispatch(&[&[0; 16]], 0, 1, &limits).is_err());
        assert!(validate_dispatch(&[&[0; 16]], MAX_DISPATCH_BYTES, 1, &limits).is_err());
    }

    #[test]
    fn counts_upload_staging_in_aggregate_memory_budget() {
        const MIB: usize = 1024 * 1024;
        assert!(validate_memory_budget(128 * MIB, 64 * MIB).is_ok());
        assert!(validate_memory_budget(128 * MIB + 4, 64 * MIB).is_err());
        // Fits the primary 256MiB cap, but staging raises it above 384MiB.
        assert!(validate_memory_budget(200 * MIB, 28 * MIB).is_err());
        assert!(validate_memory_budget(usize::MAX, 1).is_err());
        assert!(validate_memory_budget(1, usize::MAX).is_err());
    }
    #[test]
    fn invalid_shader_does_not_disable_other_operations() {
        let Some(gpu) = crate::test_gpu() else { return };
        assert!(
            gpu.run("invalid-test-shader", "invalid WGSL", &[&[0; 16]], 16, 1)
                .is_err()
        );
        assert!(gpu.available());
    }

    #[test]
    fn lost_device_rejects_compute_and_preserves_cpu_fallback() {
        // Do not destroy the shared parity-test device.
        if crate::test_gpu().is_none() {
            return;
        }
        let gpu = GpuContext::new().expect("dedicated device for loss test");
        gpu.device.destroy();
        let _ = gpu.device.poll(wgpu::PollType::Poll);
        assert!(!gpu.available());
        assert!(gpu.run("lost-device", "", &[&[0; 16]], 16, 1).is_err());
        use emulsion_raster::composite::{CompositeTree, TileAccelerator};
        let tree = CompositeTree {
            width: 256,
            height: 256,
            space: emulsion_raster::blend::BlendSpace::Linear,
            nodes: vec![],
        };
        assert!(
            gpu.render_tile(&tree, 0, emulsion_raster::TileCoord::new(0, 0))
                .is_none()
        );
    }
}
