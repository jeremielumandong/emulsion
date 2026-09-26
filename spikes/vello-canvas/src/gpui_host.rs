//! Embedding spike: the engine renders the canvas into a GPU texture that
//! GPUI's own renderer composites beside ordinary GPUI chrome. No readback,
//! no `paint_image`.
//!
//! - Linux: the engine runs on GPUI's wgpu device and paints an external
//!   texture.
//! - macOS: GPUI renders with Metal directly, so the engine keeps its own wgpu
//!   (Metal) device and renders into IOSurface-backed textures that GPUI
//!   paints as BGRA surfaces. On Apple silicon both are the same GPU.
//! - Windows: GPUI renders with Direct3D 11, so the engine keeps its own wgpu
//!   (D3D12) device on GPUI's adapter and renders into NT-shared textures
//!   that GPUI opens and draws as external textures.
//!
//! Scripted runs use the same scripts as the standalone window. A frame's
//! time is the interval between successive canvas paints: GPUI's layout,
//! paint, its own draw and present, and the wait for the GPU. Latency runs
//! from an input event to GPU completion of the GPUI frame that showed it.

use crate::bench::{Kind, Report, Script, Series};
use emulsion_core::{Document, NodeId};
use emulsion_engine::brush::{CpuStroke, GpuStroke, Readback, test_brush};
use emulsion_engine::compositor::Camera;
use emulsion_engine::engine::{Engine, FrameTimes, Output};
use emulsion_engine::gpu::TileFormat;
use emulsion_engine::vector::VectorSpace;
use gpui_kit::*;
use std::cell::RefCell;
use std::collections::VecDeque;
use std::rc::Rc;
use std::time::{Duration, Instant};

pub struct Options {
    /// Canvas area in logical pixels; the window adds the chrome around it.
    pub size: (u32, u32),
    pub tiles: Option<TileFormat>,
    pub space: VectorSpace,
    pub vello: bool,
    pub cache: bool,
    pub script: Option<(Kind, Option<usize>)>,
    pub title: String,
}

const SIDEBAR: f32 = 260.0;
const TOOLBAR: f32 = 36.0;

enum Painting {
    Idle,
    Cpu(Box<CpuStroke>),
    Gpu(Box<GpuStroke>),
}

/// The frame whose GPU completion the next paint waits for.
struct Pending {
    start: Instant,
    /// Time in this frame's canvas paint, after the wait.
    paint_ms: f64,
    times: FrameTimes,
    scripted: bool,
    events: Vec<Instant>,
}

struct State {
    options: Options,
    doc: Document,
    paint_node: Option<NodeId>,
    engine: Option<Engine>,
    script: Option<Script>,
    target: Option<backend::Target>,
    pending: Option<Pending>,
    frames: VecDeque<f64>,
    last_times: FrameTimes,
    report: Option<Report>,
    error: Option<anyhow::Error>,
    bounds: Option<Bounds<Pixels>>,
    scale: f32,
    painting: Painting,
    gpu_brush: bool,
    stroke_start: Option<Instant>,
    events: Vec<Instant>,
    latency: Series,
    readback: Option<(Readback, usize, bool)>,
    panning: Option<Point<Pixels>>,
    frame: usize,
}

fn device_size(bounds: Bounds<Pixels>, scale: f32) -> (u32, u32) {
    (
        ((f32::from(bounds.size.width) * scale).round() as u32).max(1),
        ((f32::from(bounds.size.height) * scale).round() as u32).max(1),
    )
}

/// Where the platforms differ: the device, waiting for GPUI's previous frame,
/// the render target and how it is painted.
#[cfg(target_os = "linux")]
mod backend {
    use anyhow::Context as _;
    use emulsion_engine::gpu::{Gpu, TileFormat};
    use gpui_kit::*;
    use std::sync::Arc;

    pub const FORMAT: wgpu::TextureFormat = wgpu::TextureFormat::Rgba8Unorm;
    /// Extra lines for the report.
    pub const NOTES: &[&str] = &[];

    /// The present mode GPUI's Linux backends configure.
    pub fn presentation() -> &'static str {
        if std::env::var_os("WAYLAND_DISPLAY").is_some() {
            "Wayland, Mailbox"
        } else {
            "X11, Fifo (vsync)"
        }
    }

    /// GPUI has created its device by the first paint; the engine adopts it.
    pub fn device(tiles: Option<TileFormat>) -> anyhow::Result<Arc<Gpu>> {
        let shared = gpui_wgpu::shared_gpu().context("GPUI has not published its wgpu device")?;
        Ok(Gpu::from_shared(&shared, tiles))
    }

    /// The previous GPUI frame, including its present, is complete once
    /// everything submitted to the shared device so far has run.
    pub fn wait_for_previous_frame(gpu: Option<&Gpu>) {
        if let Some(gpu) = gpu {
            gpu.wait();
        }
    }

    /// One texture; the shared queue orders the engine's frame before GPUI's.
    pub fn after_render(_: &Gpu) {}

    pub struct Target {
        view: wgpu::TextureView,
        pub size: (u32, u32),
    }

    impl Target {
        pub fn new(gpu: &Gpu, size: (u32, u32)) -> anyhow::Result<Self> {
            let texture = gpu.device.create_texture(&wgpu::TextureDescriptor {
                label: Some("embedded canvas"),
                size: wgpu::Extent3d {
                    width: size.0,
                    height: size.1,
                    depth_or_array_layers: 1,
                },
                mip_level_count: 1,
                sample_count: 1,
                dimension: wgpu::TextureDimension::D2,
                format: FORMAT,
                usage: wgpu::TextureUsages::RENDER_ATTACHMENT
                    | wgpu::TextureUsages::TEXTURE_BINDING,
                view_formats: &[],
            });
            Ok(Self {
                view: texture.create_view(&Default::default()),
                size,
            })
        }

        /// The texture this frame renders into.
        pub fn next(&mut self) -> wgpu::TextureView {
            self.view.clone()
        }

        pub fn paint(&self, window: &mut Window, bounds: Bounds<Pixels>) {
            window.paint_external_texture(bounds, ExternalTexture(Arc::new(self.view.clone())));
        }
    }
}

#[cfg(target_os = "macos")]
mod backend {
    use anyhow::{Context as _, anyhow, ensure};
    use core_foundation::base::{CFType, TCFType};
    use core_foundation::boolean::CFBoolean;
    use core_foundation::dictionary::CFDictionary;
    use core_foundation::string::CFString;
    use core_video::pixel_buffer::{
        CVPixelBuffer, CVPixelBufferRef, kCVPixelBufferIOSurfacePropertiesKey,
        kCVPixelBufferMetalCompatibilityKey, kCVPixelFormatType_32BGRA,
    };
    use emulsion_engine::gpu::{Gpu, TileFormat};
    use gpui_kit::*;
    use objc2_io_surface::IOSurfaceRef;
    use objc2_metal::{
        MTLDevice as _, MTLPixelFormat, MTLStorageMode, MTLTextureDescriptor, MTLTextureType,
        MTLTextureUsage,
    };
    use std::ffi::c_void;
    use std::sync::Arc;

    /// GPUI's layer is non-sRGB `BGRA8Unorm`, so the shader-encoded values
    /// pass straight through.
    pub const FORMAT: wgpu::TextureFormat = wgpu::TextureFormat::Bgra8Unorm;
    /// Surfaces in flight: the one GPUI may still be sampling, and more.
    const RING: usize = 3;
    pub const NOTES: &[&str] = &[
        "macOS: the canvas renders on its own wgpu Metal device into IOSurface-backed BGRA textures that GPUI paints as surfaces; the wgpu and GPUI queues are not ordered, so each canvas frame waits for its own GPU work (device.poll) before GPUI samples it",
    ];

    #[link(name = "CoreVideo", kind = "framework")]
    unsafe extern "C" {
        /// A *Get* function: the pixel buffer keeps ownership. core-video's
        /// `get_io_surface` wraps it under the create rule and over-releases.
        fn CVPixelBufferGetIOSurface(buffer: CVPixelBufferRef) -> *mut c_void;
    }

    pub fn presentation() -> &'static str {
        "macOS, CAMetalLayer (display-linked)"
    }

    /// The engine's own device on the Metal backend.
    pub fn device(tiles: Option<TileFormat>) -> anyhow::Result<Arc<Gpu>> {
        let mut desc = wgpu::InstanceDescriptor::new_without_display_handle_from_env();
        desc.backends = wgpu::Backends::METAL;
        Gpu::new(wgpu::Instance::new(desc), None, tiles)
    }

    /// Wait for GPUI's committed frames, then for the engine's own work.
    pub fn wait_for_previous_frame(gpu: Option<&Gpu>) {
        gpui_apple::wait_for_submitted_frames();
        if let Some(gpu) = gpu {
            gpu.wait();
        }
    }

    /// The two queues are not ordered: finish the canvas before GPUI can
    /// sample it.
    pub fn after_render(gpu: &Gpu) {
        gpu.wait();
    }

    struct Slot {
        buffer: CVPixelBuffer,
        view: wgpu::TextureView,
        _texture: wgpu::Texture,
    }

    pub struct Target {
        slots: Vec<Slot>,
        current: usize,
        pub size: (u32, u32),
    }

    impl Target {
        pub fn new(gpu: &Gpu, size: (u32, u32)) -> anyhow::Result<Self> {
            let slots = (0..RING)
                .map(|_| slot(gpu, size))
                .collect::<anyhow::Result<_>>()?;
            Ok(Self {
                slots,
                current: 0,
                size,
            })
        }

        /// Move to the next surface in the ring and return its texture.
        pub fn next(&mut self) -> wgpu::TextureView {
            self.current = (self.current + 1) % self.slots.len();
            self.slots[self.current].view.clone()
        }

        pub fn paint(&self, window: &mut Window, bounds: Bounds<Pixels>) {
            window.paint_surface(bounds, self.slots[self.current].buffer.clone());
        }
    }

    /// One IOSurface-backed pixel buffer and the wgpu texture over it.
    fn slot(gpu: &Gpu, (width, height): (u32, u32)) -> anyhow::Result<Slot> {
        let key = |k| unsafe { CFString::wrap_under_get_rule(k) };
        let io_surface_properties: CFDictionary<CFString, CFType> =
            CFDictionary::from_CFType_pairs(&[]);
        let options: CFDictionary<CFString, CFType> = CFDictionary::from_CFType_pairs(&[
            (
                key(unsafe { kCVPixelBufferIOSurfacePropertiesKey }),
                io_surface_properties.as_CFType(),
            ),
            (
                key(unsafe { kCVPixelBufferMetalCompatibilityKey }),
                CFBoolean::true_value().as_CFType(),
            ),
        ]);
        let buffer = CVPixelBuffer::new(
            kCVPixelFormatType_32BGRA,
            width as usize,
            height as usize,
            Some(&options),
        )
        .map_err(|status| anyhow!("CVPixelBufferCreate failed ({status})"))?;
        // Owned by the pixel buffer: not released here.
        let surface = unsafe { CVPixelBufferGetIOSurface(buffer.as_concrete_TypeRef()) };
        ensure!(!surface.is_null(), "pixel buffer has no IOSurface");

        let hal = unsafe { gpu.device.as_hal::<wgpu::hal::api::Metal>() }
            .context("the engine's device is not on the Metal backend")?;
        let desc = unsafe {
            MTLTextureDescriptor::texture2DDescriptorWithPixelFormat_width_height_mipmapped(
                MTLPixelFormat::BGRA8Unorm,
                width as usize,
                height as usize,
                false,
            )
        };
        desc.setTextureType(MTLTextureType::Type2D);
        desc.setUsage(MTLTextureUsage::RenderTarget | MTLTextureUsage::ShaderRead);
        desc.setStorageMode(MTLStorageMode::Shared);
        let surface = unsafe { &*(surface as *const IOSurfaceRef) };
        let raw = hal
            .raw_device()
            .newTextureWithDescriptor_iosurface_plane(&desc, surface, 0)
            .context("newTextureWithDescriptor:iosurface:plane: failed")?;
        drop(hal);
        let hal_texture = unsafe {
            wgpu::hal::metal::Device::texture_from_raw(
                raw,
                FORMAT,
                MTLTextureType::Type2D,
                1,
                1,
                wgpu::hal::CopyExtent {
                    width,
                    height,
                    depth: 1,
                },
            )
        };
        let texture = unsafe {
            gpu.device.create_texture_from_hal::<wgpu::hal::api::Metal>(
                hal_texture,
                &wgpu::TextureDescriptor {
                    label: Some("embedded canvas (IOSurface)"),
                    size: wgpu::Extent3d {
                        width,
                        height,
                        depth_or_array_layers: 1,
                    },
                    mip_level_count: 1,
                    sample_count: 1,
                    dimension: wgpu::TextureDimension::D2,
                    format: FORMAT,
                    usage: wgpu::TextureUsages::RENDER_ATTACHMENT
                        | wgpu::TextureUsages::TEXTURE_BINDING,
                    view_formats: &[],
                },
            )
        };
        Ok(Slot {
            buffer,
            view: texture.create_view(&Default::default()),
            _texture: texture,
        })
    }
}

#[cfg(target_os = "windows")]
mod backend {
    use anyhow::{Context as _, anyhow};
    use emulsion_engine::gpu::{Gpu, TileFormat};
    use gpui_kit::*;
    use gpui_windows::SharedTexture;
    use std::sync::Arc;
    use std::sync::atomic::{AtomicU64, Ordering};
    use windows::Win32::Foundation::{CloseHandle, GENERIC_ALL, HANDLE};
    use windows::Win32::Graphics::Direct3D12::{
        D3D12_HEAP_FLAG_SHARED, D3D12_HEAP_PROPERTIES, D3D12_HEAP_TYPE_DEFAULT,
        D3D12_RESOURCE_DESC, D3D12_RESOURCE_DIMENSION_TEXTURE2D,
        D3D12_RESOURCE_FLAG_ALLOW_RENDER_TARGET, D3D12_RESOURCE_FLAG_ALLOW_SIMULTANEOUS_ACCESS,
        D3D12_RESOURCE_STATE_COMMON, D3D12_TEXTURE_LAYOUT_UNKNOWN, ID3D12Resource,
    };
    use windows::Win32::Graphics::Dxgi::Common::{DXGI_FORMAT_B8G8R8A8_UNORM, DXGI_SAMPLE_DESC};
    use windows::core::PCWSTR;

    /// GPUI's swap chain is non-sRGB `B8G8R8A8_UNORM`, so the shader-encoded
    /// values pass straight through.
    pub const FORMAT: wgpu::TextureFormat = wgpu::TextureFormat::Bgra8Unorm;
    /// Textures in flight: the one GPUI may still be sampling, and more.
    const RING: usize = 3;
    pub const NOTES: &[&str] = &[
        "Windows: the canvas renders on its own wgpu D3D12 device, on GPUI's adapter, into NT-shared BGRA textures that GPUI's D3D11 renderer opens and draws; the two devices' queues are not ordered, so each canvas frame waits for its own GPU work (device.poll) before GPUI samples it, and for GPUI's last present (an event query) before rendering",
    ];

    pub fn presentation() -> &'static str {
        if std::env::var_os("GPUI_DISABLE_DIRECT_COMPOSITION").is_some() {
            "Windows, D3D11 flip swap chain (DwmFlush-paced)"
        } else {
            "Windows, D3D11 + DirectComposition (DwmFlush-paced)"
        }
    }

    /// The engine's own D3D12 device on the adapter GPUI chose: a shared
    /// texture cannot cross adapters.
    pub fn device(tiles: Option<TileFormat>) -> anyhow::Result<Arc<Gpu>> {
        let luid = gpui_windows::adapter_luid().context("GPUI has not chosen an adapter")?;
        let mut desc = wgpu::InstanceDescriptor::new_without_display_handle_from_env();
        desc.backends = wgpu::Backends::DX12;
        let instance = wgpu::Instance::new(desc);
        let adapter = pollster::block_on(instance.enumerate_adapters(wgpu::Backends::DX12))
            .into_iter()
            .find(|adapter| {
                let Some(hal) = (unsafe { adapter.as_hal::<wgpu::hal::api::Dx12>() }) else {
                    return false;
                };
                unsafe { hal.raw_adapter().GetDesc1() }
                    .is_ok_and(|d| (d.AdapterLuid.LowPart, d.AdapterLuid.HighPart) == luid)
            })
            .ok_or_else(|| anyhow!("no D3D12 adapter matches GPUI's (software rendering?)"))?;
        Gpu::from_adapter(adapter, tiles)
    }

    /// Wait for GPUI's last present, then for the engine's own work.
    pub fn wait_for_previous_frame(gpu: Option<&Gpu>) {
        gpui_windows::wait_for_submitted_frames();
        if let Some(gpu) = gpu {
            gpu.wait();
        }
    }

    /// The two queues are not ordered: finish the canvas before GPUI can
    /// sample it.
    pub fn after_render(gpu: &Gpu) {
        gpu.wait();
    }

    static NEXT_ID: AtomicU64 = AtomicU64::new(1);

    struct Slot {
        shared: SharedTexture,
        view: wgpu::TextureView,
        _texture: wgpu::Texture,
    }

    impl Drop for Slot {
        fn drop(&mut self) {
            // GPUI holds its own reference to the resource once opened.
            let _ = unsafe { CloseHandle(HANDLE(self.shared.handle as *mut _)) };
        }
    }

    pub struct Target {
        slots: Vec<Slot>,
        current: usize,
        pub size: (u32, u32),
    }

    impl Target {
        pub fn new(gpu: &Gpu, size: (u32, u32)) -> anyhow::Result<Self> {
            let slots = (0..RING)
                .map(|_| slot(gpu, size))
                .collect::<anyhow::Result<_>>()?;
            Ok(Self {
                slots,
                current: 0,
                size,
            })
        }

        /// Move to the next texture in the ring and return it.
        pub fn next(&mut self) -> wgpu::TextureView {
            self.current = (self.current + 1) % self.slots.len();
            self.slots[self.current].view.clone()
        }

        pub fn paint(&self, window: &mut Window, bounds: Bounds<Pixels>) {
            let shared = self.slots[self.current].shared;
            window.paint_external_texture(bounds, ExternalTexture(Arc::new(shared)));
        }
    }

    /// One shareable D3D12 texture, its NT handle, and wgpu's view of it.
    fn slot(gpu: &Gpu, (width, height): (u32, u32)) -> anyhow::Result<Slot> {
        let hal = unsafe { gpu.device.as_hal::<wgpu::hal::api::Dx12>() }
            .context("the engine's device is not on the D3D12 backend")?;
        let raw = hal.raw_device();
        let heap = D3D12_HEAP_PROPERTIES {
            Type: D3D12_HEAP_TYPE_DEFAULT,
            ..Default::default()
        };
        let desc = D3D12_RESOURCE_DESC {
            Dimension: D3D12_RESOURCE_DIMENSION_TEXTURE2D,
            Alignment: 0,
            Width: width as u64,
            Height: height,
            DepthOrArraySize: 1,
            MipLevels: 1,
            Format: DXGI_FORMAT_B8G8R8A8_UNORM,
            SampleDesc: DXGI_SAMPLE_DESC {
                Count: 1,
                Quality: 0,
            },
            Layout: D3D12_TEXTURE_LAYOUT_UNKNOWN,
            // Simultaneous access lets D3D11 read it without a state handoff.
            Flags: D3D12_RESOURCE_FLAG_ALLOW_RENDER_TARGET
                | D3D12_RESOURCE_FLAG_ALLOW_SIMULTANEOUS_ACCESS,
        };
        let mut resource: Option<ID3D12Resource> = None;
        unsafe {
            raw.CreateCommittedResource(
                &heap,
                D3D12_HEAP_FLAG_SHARED,
                &desc,
                D3D12_RESOURCE_STATE_COMMON,
                None,
                &mut resource,
            )
        }
        .context("CreateCommittedResource (shared) failed")?;
        let resource = resource.context("no shared resource")?;
        let handle =
            unsafe { raw.CreateSharedHandle(&resource, None, GENERIC_ALL.0, PCWSTR::null()) }
                .context("CreateSharedHandle failed")?;
        drop(hal);
        let size = wgpu::Extent3d {
            width,
            height,
            depth_or_array_layers: 1,
        };
        let hal_texture = unsafe {
            wgpu::hal::dx12::Device::texture_from_raw(
                resource,
                FORMAT,
                wgpu::TextureDimension::D2,
                size,
                1,
                1,
            )
        };
        let texture = unsafe {
            gpu.device.create_texture_from_hal::<wgpu::hal::api::Dx12>(
                hal_texture,
                &wgpu::TextureDescriptor {
                    label: Some("embedded canvas (shared)"),
                    size,
                    mip_level_count: 1,
                    sample_count: 1,
                    dimension: wgpu::TextureDimension::D2,
                    format: FORMAT,
                    usage: wgpu::TextureUsages::RENDER_ATTACHMENT
                        | wgpu::TextureUsages::TEXTURE_BINDING,
                    view_formats: &[],
                },
            )
        };
        Ok(Slot {
            shared: SharedTexture {
                handle: handle.0 as usize,
                id: NEXT_ID.fetch_add(1, Ordering::Relaxed),
            },
            view: texture.create_view(&Default::default()),
            _texture: texture,
        })
    }
}

use backend::presentation;

impl State {
    /// Canvas-local device position → document position.
    fn to_doc(&self, p: Point<Pixels>) -> Option<(f32, f32)> {
        let (engine, bounds) = (self.engine.as_ref()?, self.bounds?);
        let o = engine.camera.origin(engine.screen);
        let x = f32::from(p.x - bounds.origin.x) as f64 * self.scale as f64;
        let y = f32::from(p.y - bounds.origin.y) as f64 * self.scale as f64;
        Some((
            (o[0] + x / engine.camera.zoom) as f32,
            (o[1] + y / engine.camera.zoom) as f32,
        ))
    }

    fn stroke_point(&mut self, p: Point<Pixels>) {
        let Some((x, y)) = self.to_doc(p) else { return };
        let t = self
            .stroke_start
            .map_or(0.0, |s| s.elapsed().as_secs_f64() * 1e3);
        match &mut self.painting {
            Painting::Cpu(s) => s.point(x, y, t),
            Painting::Gpu(s) => s.point(x, y),
            Painting::Idle => return,
        }
        self.events.push(Instant::now());
    }

    /// Finish the previous frame, advance, and render this one into the
    /// target GPUI should composite.
    fn frame(&mut self, bounds: Bounds<Pixels>, scale: f32) -> anyhow::Result<()> {
        self.bounds = Some(bounds);
        self.scale = scale;
        let size = device_size(bounds, scale);

        let waited = Instant::now();
        {
            let _span = tracing::info_span!("gpu_wait").entered();
            backend::wait_for_previous_frame(self.engine.as_ref().map(|e| &*e.gpu));
        }
        // A frame runs from here to the same point of the next paint: its own
        // work, GPUI's layout, draw and present of it, and the GPU finishing.
        let done = Instant::now();
        let wait_ms = done.duration_since(waited).as_secs_f64() * 1e3;
        let start = done;
        if let Some(p) = self.pending.take() {
            let ms = start.duration_since(p.start).as_secs_f64() * 1e3;
            tracing::debug!(
                frame = self.frame,
                ms,
                fills = p.times.cache_fills,
                mips = p.times.mip_tiles,
                "embedded frame"
            );
            self.frames.push_back(ms);
            if self.frames.len() > 120 {
                self.frames.pop_front();
            }
            let engine = self.engine.as_mut().expect("engine");
            if p.scripted
                && let Some(script) = &mut self.script
            {
                script.after_frame(engine, ms, done, &p.times)?;
                if let Some(record) = script.report.trace.last_mut() {
                    record.split = Some((wait_ms, p.paint_ms));
                }
                if script.done() {
                    let mut report = std::mem::take(&mut script.report);
                    report.mode = format!(
                        "GPUI embedded, {} ({})",
                        engine.gpu.describe(),
                        presentation()
                    );
                    report
                        .notes
                        .extend(backend::NOTES.iter().map(|n| n.to_string()));
                    report.notes.push(format!(
                        "canvas {}x{} device px at scale {scale}; GPU textures {:.0} MiB (atlas {} tiles in {} pages, {})",
                        size.0,
                        size.1,
                        engine.texture_bytes() as f64 / 1048576.0,
                        engine.atlas.used(),
                        engine.atlas.pages(),
                        engine.gpu.tile_format.label(),
                    ));
                    self.report = Some(report);
                }
            }
            for e in p.events {
                self.latency
                    .push(done.duration_since(e).as_secs_f64() * 1e3);
            }
            if let Some((readback, _, mapped)) = &mut self.readback {
                if !*mapped {
                    readback.map();
                    *mapped = true;
                }
                let _ = engine.gpu.device.poll(wgpu::PollType::Poll);
                if readback.is_ready()
                    && let Some(source) = engine.canvas.paint
                {
                    let (readback, dabs, _) = self.readback.take().expect("readback");
                    let ms = readback.started.elapsed().as_secs_f64() * 1e3;
                    readback.complete(
                        &engine.gpu,
                        &mut engine.canvas,
                        &mut engine.atlas,
                        source,
                    )?;
                    tracing::info!(ms, dabs, "GPU stroke read back");
                }
            }
        }

        // First paint: build the engine (on GPUI's device where it has one).
        let warmup = self.engine.is_none();
        if warmup {
            let gpu = backend::device(self.options.tiles)?;
            tracing::info!(
                gpu = gpu.describe(),
                tiles = gpu.tile_format.label(),
                presentation = presentation(),
                "embedded canvas device"
            );
            let mut engine = Engine::new(
                gpu,
                &self.doc,
                self.paint_node,
                self.options.space,
                self.options.vello,
                self.options.cache,
                size,
            )?;
            if let Some((kind, frames)) = self.options.script {
                self.script = Some(Script::new(kind, &mut engine, frames));
            }
            self.engine = Some(engine);
        }
        let engine = self.engine.as_mut().expect("engine");
        engine.screen = size;
        if self.target.as_ref().is_none_or(|t| t.size != size) {
            self.target = Some(backend::Target::new(&engine.gpu, size)?);
        }

        // Scripts start after the warm-up frame, as in the standalone runs.
        let scripted = !warmup && self.script.as_ref().is_some_and(|s| !s.done());
        if scripted {
            self.script.as_mut().expect("script").before_frame(engine)?;
        } else {
            match &mut self.painting {
                Painting::Cpu(s) => {
                    s.render(&mut engine.canvas, &mut engine.atlas, &engine.gpu.queue)?;
                }
                Painting::Gpu(s) => {
                    let (b, c, a, e) = engine.brush_parts();
                    s.render(b, c, a, e)?;
                }
                Painting::Idle => {}
            }
        }
        let view = self.target.as_mut().expect("target").next();
        let times = engine.render(&view, backend::FORMAT, Output::Encoded)?;
        backend::after_render(&engine.gpu);
        self.last_times = times;
        self.pending = Some(Pending {
            start,
            paint_ms: start.elapsed().as_secs_f64() * 1e3,
            times,
            scripted,
            events: std::mem::take(&mut self.events),
        });
        self.frame += 1;
        Ok(())
    }

    fn stats(&self) -> String {
        let mut s = Series::default();
        for t in &self.frames {
            s.push(*t);
        }
        let Some(engine) = &self.engine else {
            return "starting…".into();
        };
        let latency = if self.latency.len() == 0 {
            "–".to_string()
        } else {
            format!(
                "{:.1}/{:.1} ms",
                self.latency.quantile(0.5),
                self.latency.quantile(0.99)
            )
        };
        format!(
            "frame {:.1}/{:.1} ms p50/p99 · cpu {:.2} ms · zoom {:.0}% · brush {} · stroke latency {latency} · {} tiles · {}",
            s.quantile(0.5),
            s.quantile(0.99),
            self.last_times.cpu_ms,
            engine.camera.zoom * 100.0,
            if self.gpu_brush { "B (GPU)" } else { "A (CPU)" },
            engine.gpu.tile_format.label(),
            presentation(),
        )
    }
}

type Shared = Rc<RefCell<State>>;

struct CanvasView {
    state: Shared,
}

impl Render for CanvasView {
    fn render(&mut self, _window: &mut Window, cx: &mut Context<Self>) -> impl IntoElement {
        let state = self.state.clone();
        div()
            .id("canvas")
            .size_full()
            .on_mouse_down(
                MouseButton::Left,
                cx.listener(|this, e: &MouseDownEvent, _, _| {
                    let mut s = this.state.borrow_mut();
                    if s.script.is_some() || s.readback.is_some() {
                        return;
                    }
                    let Some(source) = s.engine.as_ref().and_then(|e| e.canvas.paint) else {
                        return;
                    };
                    let brush = test_brush(crate::bench::BRUSH_SIZE);
                    s.painting = if s.gpu_brush {
                        Painting::Gpu(Box::new(GpuStroke::begin(source, brush)))
                    } else {
                        let canvas = &s.engine.as_ref().expect("engine").canvas;
                        Painting::Cpu(Box::new(CpuStroke::begin(canvas, source, brush)))
                    };
                    s.stroke_start = Some(Instant::now());
                    s.latency = Series::default();
                    s.stroke_point(e.position);
                }),
            )
            .on_mouse_up(
                MouseButton::Left,
                cx.listener(|this, _: &MouseUpEvent, _, _| {
                    let mut s = this.state.borrow_mut();
                    let s = &mut *s;
                    let Some(engine) = s.engine.as_mut() else {
                        return;
                    };
                    let result = match std::mem::replace(&mut s.painting, Painting::Idle) {
                        Painting::Cpu(mut stroke) => {
                            stroke.finish();
                            stroke
                                .render(&mut engine.canvas, &mut engine.atlas, &engine.gpu.queue)
                                .map(|_| ())
                        }
                        Painting::Gpu(mut stroke) => {
                            let (b, c, a, e) = engine.brush_parts();
                            let r = stroke.render(b, c, a, e).map(|_| ());
                            let dabs = stroke.dabs;
                            let (_, _, atlas, encoder) = engine.brush_parts();
                            s.readback = Some((stroke.finish(atlas, encoder), dabs, false));
                            r
                        }
                        Painting::Idle => Ok(()),
                    };
                    if let Err(e) = result {
                        s.error = Some(e);
                    }
                }),
            )
            .on_mouse_down(
                MouseButton::Right,
                cx.listener(|this, e: &MouseDownEvent, _, _| {
                    this.state.borrow_mut().panning = Some(e.position);
                }),
            )
            .on_mouse_up(
                MouseButton::Right,
                cx.listener(|this, _: &MouseUpEvent, _, _| {
                    this.state.borrow_mut().panning = None;
                }),
            )
            .on_mouse_move(cx.listener(|this, e: &MouseMoveEvent, _, _| {
                let mut s = this.state.borrow_mut();
                if let Some(last) = s.panning {
                    let (scale, delta) = (s.scale as f64, e.position - last);
                    if let Some(engine) = &mut s.engine {
                        let k = scale / engine.camera.zoom;
                        engine.camera.center[0] -= f32::from(delta.x) as f64 * k;
                        engine.camera.center[1] -= f32::from(delta.y) as f64 * k;
                    }
                    s.panning = Some(e.position);
                }
                s.stroke_point(e.position);
            }))
            .on_scroll_wheel(cx.listener(|this, e: &ScrollWheelEvent, _, _| {
                let mut s = this.state.borrow_mut();
                let steps = -f32::from(e.delta.pixel_delta(px(20.)).y) as f64 / 20.0;
                let Some(before) = s.to_doc(e.position) else {
                    return;
                };
                if let Some(engine) = &mut s.engine {
                    engine.camera.zoom =
                        (engine.camera.zoom * 1.15f64.powf(steps)).clamp(0.02, 64.0);
                }
                let Some(after) = s.to_doc(e.position) else {
                    return;
                };
                if let Some(engine) = &mut s.engine {
                    engine.camera.center[0] += (before.0 - after.0) as f64;
                    engine.camera.center[1] += (before.1 - after.1) as f64;
                }
            }))
            .child(
                canvas(
                    |_, _, _| (),
                    move |bounds, (), window, cx| {
                        let result = state.borrow_mut().frame(bounds, window.scale_factor());
                        match result {
                            Ok(()) => {
                                if let Some(target) = &state.borrow().target {
                                    target.paint(window, bounds);
                                }
                            }
                            Err(e) => state.borrow_mut().error = Some(e),
                        }
                        let s = state.borrow();
                        if s.report.is_some() || s.error.is_some() {
                            cx.defer(|cx| cx.quit());
                        } else {
                            window.request_animation_frame();
                        }
                    },
                )
                .size_full(),
            )
    }
}

struct Toolbar {
    state: Shared,
    _refresh: Task<()>,
}

impl Toolbar {
    fn new(state: Shared, cx: &mut Context<Self>) -> Self {
        let refresh = cx.spawn(async move |this, cx| {
            loop {
                cx.background_executor()
                    .timer(Duration::from_millis(250))
                    .await;
                if this.update(cx, |_, cx| cx.notify()).is_err() {
                    break;
                }
            }
        });
        Self {
            state,
            _refresh: refresh,
        }
    }
}

fn button(id: &'static str, label: impl Into<SharedString>) -> Stateful<Div> {
    div()
        .id(id)
        .px_2()
        .py_1()
        .rounded_sm()
        .bg(rgb(0x3a3a3a))
        .hover(|s| s.bg(rgb(0x4a4a4a)))
        .cursor_pointer()
        .child(label.into())
}

impl Render for Toolbar {
    fn render(&mut self, _window: &mut Window, cx: &mut Context<Self>) -> impl IntoElement {
        let s = self.state.borrow();
        let brush = if s.gpu_brush {
            "Brush B: GPU"
        } else {
            "Brush A: CPU"
        };
        let stats = s.stats();
        drop(s);
        div()
            .h(px(TOOLBAR))
            .flex()
            .items_center()
            .gap_2()
            .px_2()
            .bg(rgb(0x2b2b2b))
            .text_sm()
            .child(button("fit", "Fit").on_click(cx.listener(|this, _, _, _| {
                let mut s = this.state.borrow_mut();
                if let Some(e) = &mut s.engine {
                    e.camera = Camera::fit((e.canvas.width, e.canvas.height), e.screen);
                }
            })))
            .child(
                button("actual", "100%").on_click(cx.listener(|this, _, _, _| {
                    if let Some(e) = &mut this.state.borrow_mut().engine {
                        e.camera.zoom = 1.0;
                    }
                })),
            )
            .child(
                button("brush", brush).on_click(cx.listener(|this, _, _, cx| {
                    let mut s = this.state.borrow_mut();
                    s.gpu_brush = !s.gpu_brush;
                    cx.notify();
                })),
            )
            .child(div().text_color(rgb(0xa0a0a0)).child(stats))
    }
}

struct Host {
    toolbar: Entity<Toolbar>,
    canvas: Entity<CanvasView>,
    layers: Vec<SharedString>,
}

impl Render for Host {
    fn render(&mut self, _window: &mut Window, _cx: &mut Context<Self>) -> impl IntoElement {
        div()
            .size_full()
            .flex()
            .flex_col()
            .bg(rgb(0x232323))
            .text_color(rgb(0xdedede))
            .child(self.toolbar.clone())
            .child(
                div()
                    .flex()
                    .flex_1()
                    .min_h_0()
                    .child(
                        div()
                            .w(px(SIDEBAR))
                            .flex()
                            .flex_col()
                            .flex_none()
                            .overflow_hidden()
                            .bg(rgb(0x2b2b2b))
                            .text_sm()
                            .children(self.layers.iter().map(|name| {
                                div()
                                    .h(px(24.))
                                    .px_2()
                                    .flex()
                                    .items_center()
                                    .border_b_1()
                                    .border_color(rgb(0x333333))
                                    .child(name.clone())
                            })),
                    )
                    .child(div().flex_1().h_full().child(self.canvas.clone())),
            )
    }
}

/// Receives the scripted report, if any, when the app quits.
pub type Finish = Box<dyn FnOnce(Option<Report>) -> anyhow::Result<()>>;

/// Hand the outcome to `finish`, once.
fn complete(state: &Shared, finish: &RefCell<Option<Finish>>) -> Option<anyhow::Result<()>> {
    let finish = finish.borrow_mut().take()?;
    let mut s = state.borrow_mut();
    Some(match s.error.take() {
        Some(e) => Err(e),
        None => finish(s.report.take()),
    })
}

/// Open the GPUI window and run until the script finishes (or the window
/// closes), then pass the scripted report, if any, to `finish`.
///
/// `finish` runs from GPUI's quit hook: on macOS quitting terminates the
/// process, so `run` may never return there. A failure then exits with 1.
pub fn run(
    options: Options,
    doc: Document,
    paint_node: Option<NodeId>,
    finish: Finish,
) -> anyhow::Result<()> {
    let (w, h) = options.size;
    let title = options.title.clone();
    // Top of the stack first, as a layers panel shows it.
    let layers: Vec<SharedString> = doc
        .nodes
        .iter()
        .rev()
        .take(80)
        .map(|n| SharedString::from(n.name.clone()))
        .collect();
    let state: Shared = Rc::new(RefCell::new(State {
        options,
        doc,
        paint_node,
        engine: None,
        script: None,
        target: None,
        pending: None,
        frames: VecDeque::new(),
        last_times: FrameTimes::default(),
        report: None,
        error: None,
        bounds: None,
        scale: 1.0,
        painting: Painting::Idle,
        gpu_brush: false,
        stroke_start: None,
        events: Vec::new(),
        latency: Series::default(),
        readback: None,
        panning: None,
        frame: 0,
    }));
    let app_state = state.clone();
    let finish = Rc::new(RefCell::new(Some(finish)));
    let outcome: Rc<RefCell<Option<anyhow::Result<()>>>> = Rc::default();
    let (quit_state, quit_finish, quit_outcome) = (state.clone(), finish.clone(), outcome.clone());
    gpui_kit::application().run(move |cx| {
        cx.on_app_quit(move |_| {
            if let Some(result) = complete(&quit_state, &quit_finish) {
                if cfg!(target_os = "macos")
                    && let Err(e) = &result
                {
                    eprintln!("Error: {e:?}");
                    std::process::exit(1);
                }
                *quit_outcome.borrow_mut() = Some(result);
            }
            async {}
        })
        .detach();
        cx.on_window_closed(|cx, _| {
            if cx.windows().is_empty() {
                cx.quit();
            }
        })
        .detach();
        let bounds = Bounds::centered(
            None,
            size(px(w as f32 + SIDEBAR), px(h as f32 + TOOLBAR)),
            cx,
        );
        let options = WindowOptions {
            window_bounds: Some(WindowBounds::Windowed(bounds)),
            // GPUI caps unfocused windows at ~30 fps (on macOS, every third
            // vsync). Scripted runs are unattended, so never throttle.
            inactive_frame_interval: None,
            titlebar: Some(TitlebarOptions {
                title: Some(title.into()),
                ..Default::default()
            }),
            ..Default::default()
        };
        let opened = cx.open_window(options, |_window, cx| {
            let canvas = cx.new(|_| CanvasView {
                state: app_state.clone(),
            });
            let toolbar = cx.new(|cx| Toolbar::new(app_state.clone(), cx));
            cx.new(|_| Host {
                toolbar,
                canvas,
                layers,
            })
        });
        match opened {
            // Come to the front, as a user clicking into the canvas would.
            Ok(_) => cx.activate(true),
            Err(e) => {
                app_state.borrow_mut().error = Some(e);
                cx.quit();
            }
        }
    });
    let result = outcome.borrow_mut().take();
    result
        .or_else(|| complete(&state, &finish))
        .unwrap_or(Ok(()))
}
