// Added by Emulsion: shared textures and frame completion for the canvas embedding spike.
//! Application-owned textures from another D3D device, composited by GPUI.
//!
//! An application renders on its own device (the canvas spike uses wgpu on
//! D3D12) into a texture created with an NT shared handle. It paints it with
//! `Window::paint_external_texture(bounds, ExternalTexture(Arc::new(SharedTexture { .. })))`.
//! The renderer opens the handle on its D3D11 device once, caches the view by
//! [`SharedTexture::id`], and draws it like an image. The two devices do not
//! order their work: the application finishes its GPU work before painting,
//! and calls [`wait_for_submitted_frames`] before rendering into a texture
//! GPUI may still be sampling.

use std::cell::RefCell;
use std::collections::{HashMap, HashSet};
use std::sync::Mutex;

use gpui::{
    AtlasTextureId, AtlasTextureKind, AtlasTile, Bounds, Corners, DevicePixels, PaintSurface,
    PolychromeSprite, TileId, point, size,
};
use windows::Win32::Foundation::{HANDLE, LUID, TRUE};
use windows::Win32::Graphics::Direct3D11::{
    D3D11_QUERY_DESC, D3D11_QUERY_EVENT, D3D11_TEXTURE2D_DESC, ID3D11Device, ID3D11Device1,
    ID3D11DeviceContext, ID3D11Query, ID3D11ShaderResourceView, ID3D11Texture2D,
};
use windows::Win32::Graphics::Dxgi::IDXGIAdapter1;
use windows::core::{BOOL, Interface};

/// A 2D `B8G8R8A8_UNORM` texture shared through an NT handle.
#[derive(Clone, Copy, Debug)]
pub struct SharedTexture {
    /// From `ID3D12Device::CreateSharedHandle` or `IDXGIResource1::CreateSharedHandle`.
    /// The application keeps it open for as long as it may be painted.
    pub handle: usize,
    /// Unique among the application's shared textures. Handle values are
    /// reused once closed, so the renderer caches by this instead.
    pub id: u64,
}

static ADAPTER_LUID: Mutex<Option<LUID>> = Mutex::new(None);

pub(crate) fn set_adapter(adapter: &IDXGIAdapter1) {
    if let Ok(desc) = unsafe { adapter.GetDesc1() } {
        *ADAPTER_LUID.lock().unwrap() = Some(desc.AdapterLuid);
    }
}

/// The DXGI adapter GPUI renders with, as `(LowPart, HighPart)`. A shared
/// texture must come from a device on the same adapter.
pub fn adapter_luid() -> Option<(u32, i32)> {
    let luid = (*ADAPTER_LUID.lock().unwrap())?;
    Some((luid.LowPart, luid.HighPart))
}

thread_local! {
    /// The query ended after the most recent present, on the render thread.
    static LAST_FRAME: RefCell<Option<(ID3D11DeviceContext, ID3D11Query)>> =
        const { RefCell::new(None) };
}

/// One event query per renderer, ended after every present.
#[derive(Default)]
pub(crate) struct FrameFence {
    query: Option<ID3D11Query>,
}

impl FrameFence {
    pub(crate) fn submitted(&mut self, device: &ID3D11Device, context: &ID3D11DeviceContext) {
        if self.query.is_none() {
            let desc = D3D11_QUERY_DESC {
                Query: D3D11_QUERY_EVENT,
                MiscFlags: 0,
            };
            let mut query = None;
            if unsafe { device.CreateQuery(&desc, Some(&mut query)) }.is_err() {
                return;
            }
            self.query = query;
        }
        let Some(query) = &self.query else { return };
        unsafe { context.End(query) };
        LAST_FRAME.with(|f| *f.borrow_mut() = Some((context.clone(), query.clone())));
    }

    /// Queries belong to the device; forget them when it is replaced.
    pub(crate) fn reset(&mut self) {
        self.query = None;
        LAST_FRAME.with(|f| *f.borrow_mut() = None);
    }
}

/// Block until the GPU has finished the last frame GPUI presented on this
/// thread. Returns at once when nothing has been presented yet.
pub fn wait_for_submitted_frames() {
    let Some((context, query)) = LAST_FRAME.with(|f| f.borrow().clone()) else {
        return;
    };
    let mut flush = 0;
    loop {
        // `GetData` reports "not yet" as S_FALSE, which windows-rs maps to
        // Ok, so read the event's BOOL instead.
        let mut done = BOOL(0);
        let result = unsafe {
            context.GetData(
                &query,
                Some(&mut done as *mut BOOL as *mut _),
                std::mem::size_of::<BOOL>() as u32,
                flush,
            )
        };
        if result.is_err() || done == TRUE {
            return;
        }
        // Flush once (the default flag), then poll without flushing.
        flush = windows::Win32::Graphics::Direct3D11::D3D11_ASYNC_GETDATA_DONOTFLUSH.0 as u32;
        std::thread::yield_now();
    }
}

/// Frames an unpainted texture stays open, so a small ring of textures
/// painted in turn is not reopened every frame.
const KEEP_FRAMES: u64 = 16;

struct Opened {
    handle: usize,
    view: ID3D11ShaderResourceView,
    size: (i32, i32),
    last_used: u64,
}

/// Shared textures opened on the renderer's device, and this frame's views.
#[derive(Default)]
pub(crate) struct SurfaceTextures {
    opened: HashMap<u64, Opened>,
    failed: HashSet<u64>,
    frame: u64,
    views: Vec<Option<ID3D11ShaderResourceView>>,
}

impl SurfaceTextures {
    /// Open this frame's textures and describe each surface as a sprite
    /// covering its whole texture, for the polychrome sprite shaders. Surfaces
    /// without a usable texture get no view and are skipped when drawn.
    pub(crate) fn prepare(
        &mut self,
        device: &ID3D11Device,
        surfaces: &[PaintSurface],
    ) -> Vec<PolychromeSprite> {
        self.frame += 1;
        self.views.clear();
        let mut sprites = Vec::with_capacity(surfaces.len());
        for surface in surfaces {
            let opened = surface
                .texture
                .0
                .downcast_ref::<SharedTexture>()
                .and_then(|shared| self.open(device, shared));
            let (view, (width, height)) = match opened {
                Some((view, size)) => (Some(view), size),
                None => (None, (0, 0)),
            };
            self.views.push(view);
            sprites.push(PolychromeSprite {
                order: surface.order,
                pad: 0,
                grayscale: false.into(),
                opacity: 1.0,
                bounds: surface.bounds,
                content_mask: surface.content_mask.clone(),
                corner_radii: Corners::default(),
                tile: AtlasTile {
                    texture_id: AtlasTextureId {
                        index: 0,
                        kind: AtlasTextureKind::Polychrome,
                    },
                    tile_id: TileId(0),
                    padding: 0,
                    bounds: Bounds {
                        origin: point(DevicePixels(0), DevicePixels(0)),
                        size: size(DevicePixels(width), DevicePixels(height)),
                    },
                },
            });
        }
        let frame = self.frame;
        self.opened
            .retain(|_, opened| frame - opened.last_used < KEEP_FRAMES);
        sprites
    }

    /// The view for the surface at `index` in this frame's scene.
    pub(crate) fn view(&self, index: usize) -> Option<&ID3D11ShaderResourceView> {
        self.views.get(index)?.as_ref()
    }

    /// Views belong to the device; drop them when it is replaced.
    pub(crate) fn clear(&mut self) {
        self.opened.clear();
        self.failed.clear();
        self.views.clear();
    }

    fn open(
        &mut self,
        device: &ID3D11Device,
        shared: &SharedTexture,
    ) -> Option<(ID3D11ShaderResourceView, (i32, i32))> {
        if let Some(opened) = self.opened.get_mut(&shared.id)
            && opened.handle == shared.handle
        {
            opened.last_used = self.frame;
            return Some((opened.view.clone(), opened.size));
        }
        if self.failed.contains(&shared.id) {
            return None;
        }
        match open_shared(device, shared.handle) {
            Ok((view, size)) => {
                self.opened.insert(
                    shared.id,
                    Opened {
                        handle: shared.handle,
                        view: view.clone(),
                        size,
                        last_used: self.frame,
                    },
                );
                Some((view, size))
            }
            Err(error) => {
                log::error!("Cannot open shared texture {}: {error:#}", shared.id);
                self.failed.insert(shared.id);
                None
            }
        }
    }
}

fn open_shared(
    device: &ID3D11Device,
    handle: usize,
) -> anyhow::Result<(ID3D11ShaderResourceView, (i32, i32))> {
    let device1: ID3D11Device1 = device.cast()?;
    let texture: ID3D11Texture2D =
        unsafe { device1.OpenSharedResource1(HANDLE(handle as *mut _)) }?;
    let mut desc = D3D11_TEXTURE2D_DESC::default();
    unsafe { texture.GetDesc(&mut desc) };
    let mut view = None;
    unsafe { device.CreateShaderResourceView(&texture, None, Some(&mut view)) }?;
    let view = view.ok_or_else(|| anyhow::anyhow!("no shader resource view"))?;
    Ok((view, (desc.Width as i32, desc.Height as i32)))
}
