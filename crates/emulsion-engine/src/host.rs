//! GPUI hosting: how the engine's canvas texture reaches GPUI's renderer.
//!
//! One backend per platform, each exposing the same shape so a host can be
//! written once:
//!
//! - **Linux** — GPUI renders with wgpu, so the engine adopts GPUI's device
//!   ([`crate::gpu::Gpu::from_shared`]) and the texture is handed over
//!   directly with `paint_external_texture`.
//! - **macOS** — GPUI renders with Metal and wgpu is a separate client, so the
//!   two share an IOSurface, painted with the existing `paint_surface`.
//! - **Windows** — GPUI renders with DirectX, so the engine's texture is a
//!   shared D3D12 resource.
//!
//! Each backend provides `FORMAT`, `NOTES`, `presentation()`, `device()`,
//! `wait_for_previous_frame()`, `after_render()` and a `Target` with
//! `new`/`next`/`paint`.

#[cfg(target_os = "linux")]
pub mod backend {
    use crate::gpu::{Gpu, TileFormat};
    use anyhow::Context as _;
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
        let gpu = Gpu::from_shared(&shared, tiles);
        gpu.ensure_alive()?;
        Ok(gpu)
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

        /// The texture this frame renders into. On a ring backend this advances
        /// to the next slot.
        pub fn acquire(&mut self) -> wgpu::TextureView {
            self.view.clone()
        }

        pub fn paint(&self, window: &mut Window, bounds: Bounds<Pixels>) {
            window.paint_external_texture(bounds, ExternalTexture(Arc::new(self.view.clone())));
        }
    }
}

#[cfg(target_os = "macos")]
pub mod backend {
    use crate::gpu::{Gpu, TileFormat};
    use anyhow::{Context as _, anyhow, ensure};
    use core_foundation::base::{CFType, TCFType};
    use core_foundation::boolean::CFBoolean;
    use core_foundation::dictionary::CFDictionary;
    use core_foundation::string::CFString;
    use core_video::pixel_buffer::{
        CVPixelBuffer, CVPixelBufferRef, kCVPixelBufferIOSurfacePropertiesKey,
        kCVPixelBufferMetalCompatibilityKey, kCVPixelFormatType_32BGRA,
    };
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
        pub fn acquire(&mut self) -> wgpu::TextureView {
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
pub mod backend {
    use crate::gpu::{Gpu, TileFormat};
    use anyhow::{Context as _, anyhow};
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
    /// texture cannot cross adapters. Retain one device per hosting thread so
    /// opening a document does not discard wgpu's per-device DX12 shader cache.
    pub fn device(tiles: Option<TileFormat>) -> anyhow::Result<Arc<Gpu>> {
        let luid = gpui_windows::adapter_luid().context("GPUI has not chosen an adapter")?;
        device_for_adapter(luid, tiles)
    }

    struct CachedDevice {
        luid: (u32, i32),
        tiles: Option<TileFormat>,
        gpu: Arc<Gpu>,
    }

    thread_local! {
        // Keep only the device, never a document's engine, atlas or textures.
        // A strong reference preserves compiled shaders after the last tab closes.
        static DEVICE: std::cell::RefCell<Option<CachedDevice>> = const {
            std::cell::RefCell::new(None)
        };
    }

    fn device_for_adapter(luid: (u32, i32), tiles: Option<TileFormat>) -> anyhow::Result<Arc<Gpu>> {
        DEVICE.with(|cached| {
            let mut cached = cached.borrow_mut();
            if let Some(device) = cached.as_ref()
                && device.luid == luid
                && device.tiles == tiles
                && !device.gpu.is_lost()
            {
                return Ok(device.gpu.clone());
            }
            // Retire stale cache entries even if obtaining a replacement fails.
            *cached = None;
            let gpu = create_device(luid, tiles)?;
            *cached = Some(CachedDevice {
                luid,
                tiles,
                gpu: gpu.clone(),
            });
            Ok(gpu)
        })
    }

    fn create_device(luid: (u32, i32), tiles: Option<TileFormat>) -> anyhow::Result<Arc<Gpu>> {
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
        pub fn acquire(&mut self) -> wgpu::TextureView {
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

    #[cfg(test)]
    mod tests {
        use super::{DEVICE, Gpu, TileFormat, create_device, device_for_adapter};
        use std::sync::Arc;

        fn adapter_luid() -> (u32, i32) {
            let mut desc = wgpu::InstanceDescriptor::new_without_display_handle_from_env();
            desc.backends = wgpu::Backends::DX12;
            let gpu = Gpu::new(wgpu::Instance::new(desc), None, None).unwrap();
            let hal = unsafe { gpu.adapter.as_hal::<wgpu::hal::api::Dx12>() }.unwrap();
            let desc = unsafe { hal.raw_adapter().GetDesc1() }.unwrap();
            (desc.AdapterLuid.LowPart, desc.AdapterLuid.HighPart)
        }

        #[test]
        #[ignore = "requires a Windows D3D12 adapter"]
        fn windows_canvas_device_cache_survives_tabs_and_replaces_lost_devices() {
            let luid = adapter_luid();
            let first = device_for_adapter(luid, None).unwrap();
            let next = device_for_adapter(luid, None).unwrap();
            assert!(Arc::ptr_eq(&first, &next));
            let weak = Arc::downgrade(&first);
            drop(first);
            drop(next);
            let retained = weak
                .upgrade()
                .expect("closing tabs must preserve the shader cache");
            let next = device_for_adapter(luid, None).unwrap();
            assert!(Arc::ptr_eq(&retained, &next));

            let other_format = device_for_adapter(luid, Some(TileFormat::Float16)).unwrap();
            assert!(!Arc::ptr_eq(&next, &other_format));
            assert_eq!(other_format.tile_format, TileFormat::Float16);
            other_format.device.destroy();
            let _ = other_format.device.poll(wgpu::PollType::Poll);
            assert!(other_format.is_lost());
            let recovered = device_for_adapter(luid, Some(TileFormat::Float16)).unwrap();
            assert!(!Arc::ptr_eq(&other_format, &recovered));
            assert!(!recovered.is_lost());

            assert!(device_for_adapter((luid.0 ^ u32::MAX, luid.1), None).is_err());
            assert!(DEVICE.with(|cache| cache.borrow().is_none()));
            assert!(device_for_adapter(luid, None).is_ok());
        }

        #[test]
        #[ignore = "measures Windows canvas startup; run serially on an idle machine"]
        fn windows_canvas_startup_benchmark() {
            use crate::{Engine, Offscreen, Output, vector::VectorSpace};
            use emulsion_core::{Command, Document, Node, command::Slot, text::TextSpec};
            use emulsion_raster::{Placement, Raster};
            use std::time::Instant;
            let luid = adapter_luid();
            for vector in [false, true] {
                let mut doc = Document::new(3840, 2160);
                let node = if vector {
                    Node::text(
                        0,
                        "Vector artwork",
                        TextSpec {
                            text: "Editable vector artwork".into(),
                            size: 150.,
                            x: 300.,
                            y: 400.,
                            ..Default::default()
                        },
                        doc.width,
                        doc.height,
                    )
                } else {
                    Node::raster(
                        0,
                        "Photo",
                        Arc::new(Raster::solid(doc.width, doc.height, [0.1, 0.2, 0.3, 1.])),
                        Placement::default(),
                    )
                };
                Command::AddNode {
                    node: Box::new(node),
                    slot: Slot::TOP,
                }
                .apply(&mut doc)
                .unwrap();
                DEVICE.with(|cache| *cache.borrow_mut() = None);
                let mut reference = None;
                for reuse in [false, true, true, false, true] {
                    let start = Instant::now();
                    let gpu = if reuse {
                        device_for_adapter(luid, None)
                    } else {
                        create_device(luid, None)
                    }
                    .unwrap();
                    let output = Offscreen::new(&gpu, (1280, 720), wgpu::TextureFormat::Rgba8Unorm);
                    let mut engine = Engine::new(
                        gpu.clone(),
                        &doc,
                        None,
                        VectorSpace::Srgb,
                        true,
                        true,
                        (1280, 720),
                    )
                    .unwrap();
                    engine
                        .render(&output.view, output.format, Output::Encoded)
                        .unwrap();
                    gpu.wait();
                    eprintln!(
                        "windows_canvas_startup vector={vector} reuse_device={reuse} ms={:.2}",
                        start.elapsed().as_secs_f64() * 1000.
                    );
                    // Readback is outside the timed interval. Reusing the device
                    // must preserve the same image across independent engines.
                    let pixels = output.read(&gpu).unwrap();
                    if let Some(expected) = &reference {
                        assert_eq!(&pixels, expected);
                    } else {
                        reference = Some(pixels);
                    }
                }
            }
        }
    }
}
