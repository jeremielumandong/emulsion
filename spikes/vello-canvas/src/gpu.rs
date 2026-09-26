//! One wgpu device and queue shared by the compositor, the brush passes and Vello.

use anyhow::{Context, Result};
use std::sync::Arc;

/// Storage format for document tiles on the GPU.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum TileFormat {
    /// Bit-exact with the CPU's `[u16; 4]` tiles; uploads are a memcpy.
    Unorm16,
    /// Half floats: universally renderable, but 11-bit mantissa and a CPU
    /// conversion on every upload and readback.
    Float16,
}

impl TileFormat {
    pub fn wgpu(self) -> wgpu::TextureFormat {
        match self {
            Self::Unorm16 => wgpu::TextureFormat::Rgba16Unorm,
            Self::Float16 => wgpu::TextureFormat::Rgba16Float,
        }
    }

    pub fn parse(s: &str) -> Option<Self> {
        match s {
            "unorm16" => Some(Self::Unorm16),
            "float16" => Some(Self::Float16),
            _ => None,
        }
    }

    pub fn label(self) -> &'static str {
        match self {
            Self::Unorm16 => "unorm16",
            Self::Float16 => "float16",
        }
    }
}

pub struct Gpu {
    pub adapter: wgpu::Adapter,
    pub device: wgpu::Device,
    pub queue: wgpu::Queue,
    pub tile_format: TileFormat,
}

impl Gpu {
    /// Pick an adapter able to present to `surface` when one is given.
    /// `requested` forces a tile format; otherwise 16-bit unorm is used when
    /// the adapter can render and blend into it.
    pub fn new(
        instance: wgpu::Instance,
        surface: Option<&wgpu::Surface<'_>>,
        requested: Option<TileFormat>,
    ) -> Result<Arc<Self>> {
        let adapter = pollster::block_on(instance.request_adapter(&wgpu::RequestAdapterOptions {
            power_preference: wgpu::PowerPreference::HighPerformance,
            force_fallback_adapter: false,
            compatible_surface: surface,
        }))
        .context("no wgpu adapter")?;
        let unorm_features = adapter.get_texture_format_features(wgpu::TextureFormat::Rgba16Unorm);
        // Rendering and blending into Rgba16Unorm are adapter-specific
        // format features on top of the 16-bit-norm feature.
        let unorm_features_needed = wgpu::Features::TEXTURE_FORMAT_16BIT_NORM
            | wgpu::Features::TEXTURE_ADAPTER_SPECIFIC_FORMAT_FEATURES;
        let unorm_ok = adapter.features().contains(unorm_features_needed)
            && unorm_features.allowed_usages.contains(
                wgpu::TextureUsages::RENDER_ATTACHMENT | wgpu::TextureUsages::TEXTURE_BINDING,
            )
            && unorm_features
                .flags
                .contains(wgpu::TextureFormatFeatureFlags::BLENDABLE);
        let tile_format = match requested {
            Some(TileFormat::Unorm16) if !unorm_ok => {
                anyhow::bail!("adapter cannot render and blend into Rgba16Unorm")
            }
            Some(format) => format,
            None if unorm_ok => TileFormat::Unorm16,
            None => TileFormat::Float16,
        };
        let mut features = wgpu::Features::empty();
        if tile_format == TileFormat::Unorm16 {
            features |= unorm_features_needed;
        }
        let limits = wgpu::Limits {
            max_texture_array_layers: adapter.limits().max_texture_array_layers,
            max_storage_buffer_binding_size: adapter
                .limits()
                .max_storage_buffer_binding_size
                .min(1 << 30),
            ..wgpu::Limits::default()
        };
        let (device, queue) = pollster::block_on(adapter.request_device(&wgpu::DeviceDescriptor {
            label: Some("vello-canvas spike"),
            required_features: features,
            required_limits: limits,
            memory_hints: wgpu::MemoryHints::Performance,
            trace: wgpu::Trace::Off,
            experimental_features: wgpu::ExperimentalFeatures::disabled(),
        }))
        .context("request device")?;
        device.on_uncaptured_error(Arc::new(|error| {
            tracing::error!(%error, "wgpu error");
        }));
        let info = adapter.get_info();
        tracing::info!(
            adapter = info.name,
            backend = ?info.backend,
            driver = info.driver,
            driver_info = info.driver_info,
            tiles = tile_format.label(),
            "GPU ready"
        );
        Ok(Arc::new(Self {
            adapter,
            device,
            queue,
            tile_format,
        }))
    }

    pub fn describe(&self) -> String {
        let info = self.adapter.get_info();
        format!(
            "{} ({:?}, {:?}, driver {} {})",
            info.name, info.device_type, info.backend, info.driver, info.driver_info
        )
    }

    /// Bytes the backend's allocator reports as allocated, when it keeps a report.
    pub fn allocated_bytes(&self) -> Option<u64> {
        self.device
            .generate_allocator_report()
            .map(|report| report.total_allocated_bytes)
    }

    /// Block until all submitted work has finished.
    pub fn wait(&self) {
        let _ = self.device.poll(wgpu::PollType::wait_indefinitely());
    }
}

pub fn instance() -> wgpu::Instance {
    wgpu::Instance::new(wgpu::InstanceDescriptor::new_without_display_handle_from_env())
}

/// f32 → IEEE half, round to nearest even. Inputs are finite.
pub fn f32_to_f16(value: f32) -> u16 {
    let bits = value.to_bits();
    let sign = ((bits >> 16) & 0x8000) as u16;
    let exp = ((bits >> 23) & 0xff) as i32;
    let mant = bits & 0x007f_ffff;
    if exp == 0xff {
        return sign | 0x7c00 | if mant != 0 { 0x200 } else { 0 };
    }
    let e = exp - 127 + 15;
    if e >= 0x1f {
        return sign | 0x7c00;
    }
    if e <= 0 {
        if e < -10 {
            return sign;
        }
        let m = mant | 0x0080_0000;
        let shift = (14 - e) as u32;
        let half = 1u32 << (shift - 1);
        let rest = m & ((1u32 << shift) - 1);
        let mut out = m >> shift;
        if rest > half || (rest == half && out & 1 == 1) {
            out += 1;
        }
        return sign | out as u16;
    }
    let mut out = ((e as u32) << 10) | (mant >> 13);
    let rest = mant & 0x1fff;
    if rest > 0x1000 || (rest == 0x1000 && out & 1 == 1) {
        out += 1;
    }
    sign | out as u16
}

pub fn f16_to_f32(half: u16) -> f32 {
    let sign = ((half as u32) & 0x8000) << 16;
    let exp = ((half >> 10) & 0x1f) as u32;
    let mant = (half & 0x3ff) as u32;
    let bits = match (exp, mant) {
        (0, 0) => sign,
        (0, _) => {
            // Subnormal: normalise.
            let mut e = 127 - 15 + 1;
            let mut m = mant;
            while m & 0x400 == 0 {
                m <<= 1;
                e -= 1;
            }
            sign | (e << 23) | ((m & 0x3ff) << 13)
        }
        (0x1f, _) => sign | 0x7f80_0000 | (mant << 13),
        _ => sign | ((exp + 127 - 15) << 23) | (mant << 13),
    };
    f32::from_bits(bits)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn half_round_trip_is_nearest() {
        for v in [0.0f32, 1.0, 0.5, 1e-5, 0.333, 65504.0, 6.1e-5, 3.0e-7] {
            let h = f32_to_f16(v);
            let back = f16_to_f32(h);
            let next = f16_to_f32(h + 1);
            let prev = if h > 0 { f16_to_f32(h - 1) } else { back };
            assert!((back - v).abs() <= (next - v).abs(), "{v}");
            assert!((back - v).abs() <= (prev - v).abs(), "{v}");
        }
    }
}
